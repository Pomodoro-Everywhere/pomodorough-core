use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CoreError, MAX_BOOTSTRAP_HISTORY, check_input_len};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapPlanInput {
    #[serde(default)]
    local_owner_id: Option<String>,
    #[serde(default)]
    current_user_id: Option<String>,
    #[serde(default)]
    local_history: Vec<Value>,
    #[serde(default)]
    remote_history: Vec<Value>,
    #[serde(default)]
    has_local_state: bool,
    #[serde(default)]
    has_remote_state: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapPlanOutput {
    mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    strategy: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    local_history_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote_history_count: Option<usize>,
}

pub(crate) fn plan_v1_json(input: &str) -> Result<String, CoreError> {
    check_input_len(input)?;
    let input: BootstrapPlanInput = serde_json::from_str(input)?;
    if input.local_history.len() > MAX_BOOTSTRAP_HISTORY
        || input.remote_history.len() > MAX_BOOTSTRAP_HISTORY
    {
        return Err(CoreError::InvalidInput(format!(
            "history exceeds {MAX_BOOTSTRAP_HISTORY}"
        )));
    }
    let output = plan(input)?;
    Ok(serde_json::to_string(&output)?)
}

fn plan(input: BootstrapPlanInput) -> Result<BootstrapPlanOutput, CoreError> {
    if let Some(local_owner_id) = input
        .local_owner_id
        .as_deref()
        .filter(|owner_id| !owner_id.is_empty())
    {
        let current_user_id = input
            .current_user_id
            .as_deref()
            .filter(|user_id| !user_id.is_empty())
            .ok_or_else(|| CoreError::InvalidInput("missing currentUserId".into()))?;
        if current_user_id == local_owner_id {
            return Ok(automatic(None, "same_owner", "normal_sync"));
        }
        return Ok(automatic(Some("keep_remote"), "different_owner", "auto"));
    }

    let local_history_count = completed_history_count(&input.local_history);
    let remote_history_count = completed_history_count(&input.remote_history);
    let local_state_exists = input.has_local_state || local_history_count > 0;
    let remote_state_exists = input.has_remote_state || remote_history_count > 0;

    if (local_history_count > 0 && remote_state_exists)
        || (remote_history_count > 0 && local_state_exists)
    {
        return Ok(BootstrapPlanOutput {
            mode: "choose",
            strategy: None,
            reason: None,
            local_history_count: Some(local_history_count),
            remote_history_count: Some(remote_history_count),
        });
    }
    if local_history_count > 0 {
        return Ok(automatic(Some("replace_remote"), "local_only", "auto"));
    }
    if remote_history_count > 0 {
        return Ok(automatic(Some("keep_remote"), "remote_only", "auto"));
    }
    if local_state_exists {
        Ok(automatic(Some("merge"), "local_state_only", "auto"))
    } else {
        Ok(automatic(Some("keep_remote"), "empty", "auto"))
    }
}

fn automatic(
    strategy: Option<&'static str>,
    reason: &'static str,
    mode: &'static str,
) -> BootstrapPlanOutput {
    BootstrapPlanOutput {
        mode,
        strategy,
        reason: Some(reason),
        local_history_count: None,
        remote_history_count: None,
    }
}

fn completed_history_count(history: &[Value]) -> usize {
    let mut identities = BTreeSet::new();
    for item in history {
        if item.get("status").and_then(Value::as_str) != Some("completed") {
            continue;
        }
        let identity = item
            .get("timerId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(|value| format!("timer:{value}"))
            .or_else(|| {
                item.get("id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(|value| format!("id:{value}"))
            });
        if let Some(identity) = identity {
            identities.insert(identity);
        }
    }
    identities.len()
}
