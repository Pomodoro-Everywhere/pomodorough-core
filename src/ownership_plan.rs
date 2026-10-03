//! Raw PWA ownership planning at the host's guarded storage transaction boundary.
mod schema;
use chrono::{DateTime, SecondsFormat};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::timer_ownership::{self, MAX_SAFE_MS, Ownership};
use crate::{CoreError, strict_json};

#[derive(Deserialize)]
enum Profile {
    #[serde(rename = "pwaStorage")]
    PwaStorage,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Action {
    Install {},
    Renew {
        #[serde(rename = "timerId")]
        timer_id: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Clock {
    now_ms: i64,
    lease_duration_ms: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    profile: Profile,
    action: Action,
    workspace: Value,
    ownership: Option<Ownership>,
    local_device_id: String,
    local_tab_id: String,
    clock: Clock,
}

fn invalid(reason: &str) -> CoreError {
    CoreError::InvalidInput(reason.into())
}

pub(crate) fn plan_json(raw: &str) -> Result<String, CoreError> {
    let raw = strict_json::parse(raw)?;
    strict_json::shape::validate(&raw, &schema::REQUEST, "")?;
    let request: Request = serde_json::from_value(raw.clone())?;
    let at = validate(&request)?;
    let projected = project(&request.workspace, &at)?;
    // Natural deadline expiry is not an explicit Finish. Keep the peer lease
    // until completion commits, even when the observed timer reads completed.
    let unexpired = project(&request.workspace, "1970-01-01T00:00:00Z")?;
    let mut result = json!({"schemaVersion": 1, "ownership": raw["ownership"],
        "workspace": request.workspace, "renewed": false, "reason": "",
        "ownershipWrites": [], "effectsAfterCommit": []});
    plan(
        &request,
        &projected["canonicalTimer"],
        &unexpired["canonicalTimer"],
        &mut result,
    )?;
    Ok(result.to_string())
}

fn validate(request: &Request) -> Result<String, CoreError> {
    let Profile::PwaStorage = request.profile;
    if request.local_device_id.is_empty() || request.local_tab_id.is_empty() {
        return Err(invalid("missing local ownership identity"));
    }
    if matches!(&request.action, Action::Renew { timer_id } if timer_id.is_empty()) {
        return Err(invalid("missing renewal timer identity"));
    }
    if !(0..=MAX_SAFE_MS).contains(&request.clock.now_ms)
        || !(1..=MAX_SAFE_MS).contains(&request.clock.lease_duration_ms)
    {
        return Err(invalid("invalid ownership clock"));
    }
    if request.workspace.get("now").is_some() || request.workspace.get("displayContext").is_none() {
        return Err(invalid(
            "ownership requires raw workspace display context without now",
        ));
    }
    validate_owner(request.ownership.as_ref())?;
    DateTime::from_timestamp_millis(request.clock.now_ms)
        .map(|at| at.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or_else(|| invalid("ownership time out of range"))
}

fn validate_owner(owner: Option<&Ownership>) -> Result<(), CoreError> {
    if owner.is_some_and(|owner| {
        owner.timer_id.is_empty()
            || owner.device_id.is_empty()
            || owner.tab_id.as_deref().is_some_and(str::is_empty)
            || owner
                .lease_expires_at_ms
                .is_some_and(|ms| !(0..=MAX_SAFE_MS).contains(&ms))
    }) {
        return Err(invalid("invalid raw timer owner"));
    }
    Ok(())
}

fn project(workspace: &Value, at: &str) -> Result<Value, CoreError> {
    let mut raw = workspace.clone();
    raw["now"] = json!(at);
    let result = crate::reconciliation::workspace::project_json(&raw.to_string())?;
    Ok(serde_json::from_str::<Value>(&result)?["workspace"].clone())
}

fn active(timer: &Value) -> bool {
    matches!(timer["status"].as_str(), Some("running" | "paused"))
}

fn plan(
    request: &Request,
    timer: &Value,
    unexpired: &Value,
    result: &mut Value,
) -> Result<(), CoreError> {
    if let Some(owner) = &request.ownership {
        if !active(unexpired) || owner.timer_id != unexpired["id"] {
            result["ownership"] = Value::Null;
            result["ownershipWrites"] = json!([{"kind": "removeTimerOwner"}]);
            result["reason"] = json!("staleOwner");
            return Ok(());
        }
        return plan_existing(request, owner, unexpired, result);
    }
    if !active(timer)
        || !timer_ownership::can_claim_missing(&request.workspace, timer, &request.local_device_id)
    {
        result["reason"] = json!("notClaimable");
        return Ok(());
    }
    write_owner(request, &timer["id"], result)?;
    // The PWA first installs the actual missing owner, then checks the heartbeat's
    // presented ID. A stale heartbeat returns false without losing that write.
    if matches!(&request.action, Action::Renew { timer_id } if *timer_id != timer["id"]) {
        result["renewed"] = json!(false);
        result["reason"] = json!("staleTimer");
    } else if matches!(request.action, Action::Renew { .. }) {
        // Preserve both original transaction writes: missing-owner installation
        // precedes renewal. A fault at either write must abort the entire group.
        let write = result["ownershipWrites"][0].clone();
        result["ownershipWrites"]
            .as_array_mut()
            .unwrap()
            .push(write);
    }
    Ok(())
}

fn plan_existing(
    request: &Request,
    owner: &Ownership,
    timer: &Value,
    result: &mut Value,
) -> Result<(), CoreError> {
    let Action::Renew { timer_id } = &request.action else {
        return Ok(());
    };
    if *timer_id != timer["id"] {
        result["reason"] = json!("staleTimer");
        return Ok(());
    }
    let (owned, retry) = timer_ownership::owns_lease(
        owner,
        &timer["id"],
        &request.local_device_id,
        Some(&request.local_tab_id),
        request.clock.now_ms,
    );
    if !owned {
        result["reason"] = json!("notOwner");
        if let Some(at) = retry {
            result["retryAtMs"] = json!(at);
        }
        return Ok(());
    }
    write_owner(request, &timer["id"], result)
}

fn write_owner(request: &Request, timer_id: &Value, result: &mut Value) -> Result<(), CoreError> {
    let write = timer_ownership::record_write(
        timer_id,
        &request.local_device_id,
        Some(&request.local_tab_id),
        request.clock.now_ms,
        request.clock.lease_duration_ms,
    )?;
    let mut owner = write.clone();
    owner.as_object_mut().unwrap().remove("kind");
    result["ownership"] = owner;
    result["ownershipWrites"] = json!([write]);
    result["renewed"] = json!(matches!(request.action, Action::Renew { .. }));
    Ok(())
}
