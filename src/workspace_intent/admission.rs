use serde::Serialize;
use serde_json::{Value, json};

use super::model::{Compatibility, Input, Observation};
use super::{CoreError, invalid, projection};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum DisplayOutcome {
    Applied,
    Queued,
}

#[derive(Serialize)]
pub(super) struct OperationOutcome<'a> {
    id: &'a str,
    outcome: DisplayOutcome,
}

pub(super) fn group(
    input: &Input,
    workspace: &Value,
    operations: &Value,
    observation: &Observation,
    now: &str,
) -> Result<Value, CoreError> {
    // Admission replays the complete ledger, including possibly delivered work.
    // It neither restores delivery proof nor supplies the safe display result.
    let projected = projection::observed(
        workspace,
        observation,
        input.compatibility,
        projection::ReplayDomains::Retained,
        now,
    )?["workspace"]
        .clone();
    for (domain, queue) in operations.as_object().unwrap() {
        for operation in queue.as_array().unwrap() {
            if !applied(&projected, domain, operation)
                || !matches_value(&projected, domain, operation)
            {
                return Err(invalid("workspace group failed retained-ledger admission"));
            }
        }
    }
    Ok(json!({"workspace": projected}))
}

pub(super) fn commands(
    input: &Input,
    workspace: &Value,
    commands: &[Value],
    observation: &Observation,
    now: &str,
) -> Result<Value, CoreError> {
    group(
        input,
        workspace,
        &json!({"commands": commands}),
        observation,
        now,
    )
}

pub(super) fn validate_selection_task(
    input: &Input,
    before: &Value,
    task_id: Option<&str>,
) -> Result<(), CoreError> {
    let Some(task_id) = task_id else {
        return Ok(());
    };
    if before["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["id"] == task_id)
    {
        return Ok(());
    }
    if selection_task_active(input, task_id)? {
        return Ok(());
    }
    Err(invalid("selected task is not active"))
}

fn selection_task_active(input: &Input, task_id: &str) -> Result<bool, CoreError> {
    if !matches!(
        input.compatibility,
        Compatibility::DesktopStorage | Compatibility::PwaStorage
    ) {
        return Ok(false);
    }
    // These storage entrypoints validate selection against their prospective
    // complete queues. A claimed Upsert can establish a valid task without
    // making that task eligible for safe display. The known-task cache cannot.
    let retained = group(
        input,
        &input.workspace,
        &json!({}),
        &input.observation,
        &input.clock.physical_now,
    )?;
    Ok(retained["workspace"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["id"] == task_id))
}

fn matches_value(projected: &Value, domain: &str, operation: &Value) -> bool {
    match domain {
        "taskOperations" => {
            let task = projected["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|task| task["id"] == operation["taskId"]);
            if operation["type"] == "upsert" {
                task == Some(&json!({"id": operation["taskId"], "title": operation["title"]}))
            } else {
                task.is_none()
            }
        }
        "durationOperations" => {
            projected["durationsMs"][operation["phase"].as_str().unwrap()]
                == operation["durationMs"]
        }
        "autoStartOperations" => projected["autoStartBreaks"] == operation["enabled"],
        "selectedTaskOperations" => projected["selectedTaskId"] == operation["taskId"],
        "commands" => true,
        _ => unreachable!("internal operation domain"),
    }
}

pub(super) fn command_outcomes<'a>(
    projected: &Value,
    commands: &'a [Value],
) -> Vec<OperationOutcome<'a>> {
    commands
        .iter()
        .map(|command| outcome(projected, "commands", command))
        .collect()
}

pub(super) fn outcome<'a>(
    projected: &Value,
    domain: &str,
    operation: &'a Value,
) -> OperationOutcome<'a> {
    OperationOutcome {
        id: operation["id"]
            .as_str()
            .expect("allocated operation identity"),
        outcome: if applied(projected, domain, operation)
            && matches_value(projected, domain, operation)
        {
            DisplayOutcome::Applied
        } else {
            DisplayOutcome::Queued
        },
    }
}

pub(super) fn applied(projected: &Value, domain: &str, operation: &Value) -> bool {
    let winner = match domain {
        "commands" => &projected["timerOutcomes"][operation["id"].as_str().unwrap()]["outcome"],
        "taskOperations" => {
            &projected["winningOperationIds"]["tasks"][operation["taskId"].as_str().unwrap()]
        }
        "durationOperations" => {
            &projected["winningOperationIds"]["durations"][operation["phase"].as_str().unwrap()]
        }
        "autoStartOperations" => &projected["winningOperationIds"]["autoStart"],
        _ => &projected["winningOperationIds"]["selectedTask"],
    };
    if domain == "commands" {
        winner == "applied"
    } else {
        winner == &operation["id"]
    }
}

#[cfg(test)]
#[path = "mutation/tests/admission.rs"]
mod tests;
