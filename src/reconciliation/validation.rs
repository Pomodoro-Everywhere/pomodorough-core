use std::collections::{BTreeMap, BTreeSet};

use chrono::DateTime;
use serde_json::Value;

use crate::sync_projection::Task;
use crate::{CoreError, SelectedTaskField, timer};

use super::{CanonicalResponse, LocalQueues, MAX_CLOCK_SKEW_MS, MAX_SAFE_INTEGER};

pub(super) fn required_response_fields(input: &Value) -> Result<(), CoreError> {
    let response = input
        .get("response")
        .and_then(Value::as_object)
        .ok_or_else(|| CoreError::InvalidInput("missing canonical response".into()))?;
    for field in [
        "acknowledgements",
        "taskAcknowledgements",
        "durationAcknowledgements",
        "autoStartAcknowledgements",
        "selectedTaskAcknowledgements",
        "revision",
        "canonicalTimer",
        "history",
        "tasks",
        "durationsMs",
        "autoStartBreaks",
        "selectedTaskId",
        "serverTime",
        "serverHlcWallMs",
        "serverHlcCounter",
    ] {
        if !response.contains_key(field) {
            return Err(CoreError::MissingProjection(match field {
                "canonicalTimer" => "response.canonicalTimer",
                "selectedTaskId" => "response.selectedTaskId",
                "revision" => "response.revision",
                "history" => "response.history",
                "tasks" => "response.tasks",
                "durationsMs" => "response.durationsMs",
                "autoStartBreaks" => "response.autoStartBreaks",
                "serverTime" => "response.serverTime",
                "serverHlcWallMs" => "response.serverHlcWallMs",
                "serverHlcCounter" => "response.serverHlcCounter",
                "acknowledgements" => "response.acknowledgements",
                "taskAcknowledgements" => "response.taskAcknowledgements",
                "durationAcknowledgements" => "response.durationAcknowledgements",
                "autoStartAcknowledgements" => "response.autoStartAcknowledgements",
                "selectedTaskAcknowledgements" => "response.selectedTaskAcknowledgements",
                _ => unreachable!(),
            }));
        }
    }
    Ok(())
}

pub(super) fn canonical_response(response: &CanonicalResponse) -> Result<(), CoreError> {
    if !(0..=MAX_SAFE_INTEGER).contains(&response.revision) {
        return invalid_response("revision");
    }
    let server_time = DateTime::parse_from_rfc3339(&response.server_time)
        .map_err(|_| CoreError::InvalidTimestamp(response.server_time.clone()))?;
    let server_time_ms = server_time.timestamp_millis();
    let Some(clock_skew_ms) = response.server_hlc_wall_ms.checked_sub(server_time_ms) else {
        return invalid_response("server HLC");
    };
    if !(0..=MAX_CLOCK_SKEW_MS).contains(&clock_skew_ms)
        || !(0..=MAX_SAFE_INTEGER).contains(&response.server_hlc_wall_ms)
        || !(0..=MAX_SAFE_INTEGER).contains(&response.server_hlc_counter)
    {
        return invalid_response("server HLC");
    }
    timer::validate_replay_state(&response.canonical_timer.0, &response.history)?;
    validate_tasks(&response.tasks)?;
    validate_durations(&response.durations_ms)?;
    let task_ids = response
        .tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<BTreeSet<_>>();
    match &response.selected_task_id {
        SelectedTaskField::Selected(task_id)
            if task_id.is_empty() || !task_ids.contains(task_id.as_str()) =>
        {
            invalid_response("selectedTaskId")
        }
        SelectedTaskField::Omitted => Err(CoreError::MissingProjection("response.selectedTaskId")),
        SelectedTaskField::Deselected | SelectedTaskField::Selected(_) => Ok(()),
    }
}

pub(super) fn local_queue_ids(local: &LocalQueues) -> Result<(), CoreError> {
    validate_unique_local_ids(
        "taskOperations",
        local
            .task_operations
            .iter()
            .map(|operation| operation.clock.id.as_str()),
    )?;
    validate_unique_local_ids(
        "durationOperations",
        local
            .duration_operations
            .iter()
            .map(|operation| operation.clock.id.as_str()),
    )?;
    validate_unique_local_ids(
        "autoStartOperations",
        local
            .auto_start_operations
            .iter()
            .map(|operation| operation.clock.id.as_str()),
    )?;
    validate_unique_local_ids(
        "selectedTaskOperations",
        local
            .selected_task_operations
            .iter()
            .map(|operation| operation.clock.id.as_str()),
    )
}

fn validate_unique_local_ids<'a>(
    field: &str,
    identifiers: impl IntoIterator<Item = &'a str>,
) -> Result<(), CoreError> {
    let mut seen = BTreeSet::new();
    for identifier in identifiers {
        if identifier.is_empty() || !seen.insert(identifier) {
            return Err(CoreError::InvalidInput(format!(
                "invalid local {field} identities"
            )));
        }
    }
    Ok(())
}

fn validate_tasks(tasks: &[Task]) -> Result<(), CoreError> {
    let mut ids = BTreeSet::new();
    if tasks
        .iter()
        .any(|task| task.id.is_empty() || task.title.is_empty() || !ids.insert(task.id.as_str()))
    {
        return invalid_response("tasks");
    }
    Ok(())
}

fn validate_durations(durations: &BTreeMap<String, i64>) -> Result<(), CoreError> {
    for phase in ["focus", "short_break", "long_break"] {
        let Some(duration) = durations.get(phase) else {
            return invalid_response("durationsMs");
        };
        if !(60_000..=10_800_000).contains(duration) || duration % 60_000 != 0 {
            return invalid_response("durationsMs");
        }
    }
    Ok(())
}

fn invalid_response<T>(field: &str) -> Result<T, CoreError> {
    Err(CoreError::InvalidInput(format!(
        "invalid canonical response {field}"
    )))
}
