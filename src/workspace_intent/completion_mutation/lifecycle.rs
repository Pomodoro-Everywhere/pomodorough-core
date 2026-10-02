use serde_json::{Value, json};

use super::{CoreError, Request as FinishRequest, invalid};
use crate::workspace_intent::{
    model::{Input, ReplicationMode},
    projection,
};

mod deferred;
mod expiry;
mod model;
mod output;
mod validation;

pub(super) use model::State;
use model::{Request, Stage, Trigger};
pub(super) use validation::state as validate_state;

pub(super) fn plan_json(value: Value) -> Result<String, CoreError> {
    if value.get("ownership").is_none() {
        return Err(invalid("missing completion ownership"));
    }
    let request: Request = serde_json::from_value(value)?;
    let input = request.context();
    validation::request(&request, &input)?;
    let result = match request.stage {
        Stage::ExpiryObservation => expiry::plan(&request, &input)?,
        Stage::DeferredBreakOpportunity => deferred::plan(&request, &input)?,
    };
    Ok(result.to_string())
}

pub(super) fn project(input: &Input, workspace: &Value, now: &str) -> Result<Value, CoreError> {
    let safe = projection::project(workspace, now)?;
    // Iroh owns a local domain aggregate, not a canonical server-head barrier.
    // Validate proofs and dependencies above, then replay every retained domain.
    let result = if input.replication_mode == ReplicationMode::Centralized {
        safe
    } else {
        let raw = json!({"base": workspace["base"], "pending": workspace["local"], "now": now});
        json!({"workspace": serde_json::from_str::<Value>(
            &crate::projection::apply_workspace_json(&raw.to_string())?)?,
            "projectionPending": workspace["local"]})
    };
    Ok(result)
}

pub(super) fn observe(
    input: &Input,
    workspace: &Value,
    observation: &crate::workspace_intent::model::Observation,
    now: &str,
) -> Result<Value, CoreError> {
    let domains = if input.replication_mode == ReplicationMode::Centralized {
        projection::ReplayDomains::Safe
    } else {
        projection::ReplayDomains::Retained
    };
    projection::observed(workspace, observation, input.compatibility, domains, now)
}

pub(super) fn finish_opportunity(
    request: &FinishRequest,
    queue_break: bool,
    result: &mut Value,
) -> Result<(), CoreError> {
    let mut state = request.lifecycle.clone();
    if queue_break && !request.supports_generated_break() {
        let finish = &result["commands"][0];
        let trigger = Trigger {
            finish_command_id: finish["id"].as_str().unwrap().into(),
            timer_id: finish["timerId"].as_str().unwrap().into(),
            finish_device_sequence: finish["deviceSequence"].as_i64().unwrap(),
            reserved_timer_uuid: if request.replication_mode == ReplicationMode::Iroh {
                Some(
                    request
                        .identities
                        .timer_uuid
                        .clone()
                        .ok_or_else(|| invalid("Iroh continuation requires timer entropy"))?,
                )
            } else {
                None
            },
        };
        state.pending_breaks.push(trigger.clone());
        validate_state(&state)?;
        result["completionRecords"]["pendingAutoBreak"] = serde_json::to_value(trigger)?;
        if request.replication_mode == ReplicationMode::Iroh {
            result["effectsAfterCommit"] = json!([{"kind": "launchSync"}]);
        }
    }
    result["lifecycle"] = serde_json::to_value(state)?;
    Ok(())
}

fn day<'a>(
    request: &'a Request,
    at: &str,
) -> Result<&'a crate::workspace_intent::model::Interval, CoreError> {
    let at = crate::timer::parse_time(at)?;
    request
        .calendar_intervals
        .iter()
        .find(|day| {
            let start = crate::timer::parse_time(&day.start).expect("calendar validated");
            let end = crate::timer::parse_time(&day.end).expect("calendar validated");
            start <= at && at < end
        })
        .ok_or_else(|| invalid("missing completion calendar interval"))
}

fn phase(request: &Request, workspace: &Value, source: &Value) -> Result<String, CoreError> {
    let bounds = day(request, source["completedAt"].as_str().unwrap())?;
    let context = json!({"kind": "finishApplied", "source": {
        "commandId": source["commandId"].as_str().unwrap_or(source["timerId"].as_str().unwrap()),
        "timerId": source["timerId"], "phase": source["phase"], "occurredAt": source["completedAt"]},
        "history": workspace["history"], "autoStartBreaks": workspace["autoStartBreaks"],
        "localDeviceId": request.allocation.device_id, "ownership": null,
        "dayStart": bounds.start, "dayEnd": bounds.end});
    let plan: Value =
        serde_json::from_str(&crate::completion_plan::plan_v1_json(&context.to_string())?)?;
    Ok(plan["selectedPhase"].as_str().unwrap().into())
}
