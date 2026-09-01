use std::collections::{BTreeMap, BTreeSet};

use chrono::DateTime;
use serde::de::Error as _;
use serde_json::{Map, Value};

use crate::sync_projection::{
    Task, is_canonical_task_identity, is_valid_duration_map, validate_duration_fields,
    validate_operation_clock, validate_operation_timestamp, validate_selected_task_fields,
    validate_task_operation_fields,
};
use crate::{CoreError, SelectedTaskField, timer};

use super::{CanonicalResponse, LocalQueues, MAX_CLOCK_SKEW_MS, MAX_SAFE_INTEGER};

const REQUIRED_RESPONSE_FIELDS: [(&str, &str); 15] = [
    ("acknowledgements", "response.acknowledgements"),
    ("taskAcknowledgements", "response.taskAcknowledgements"),
    (
        "durationAcknowledgements",
        "response.durationAcknowledgements",
    ),
    (
        "autoStartAcknowledgements",
        "response.autoStartAcknowledgements",
    ),
    (
        "selectedTaskAcknowledgements",
        "response.selectedTaskAcknowledgements",
    ),
    ("revision", "response.revision"),
    ("canonicalTimer", "response.canonicalTimer"),
    ("history", "response.history"),
    ("tasks", "response.tasks"),
    ("durationsMs", "response.durationsMs"),
    ("autoStartBreaks", "response.autoStartBreaks"),
    ("selectedTaskId", "response.selectedTaskId"),
    ("serverTime", "response.serverTime"),
    ("serverHlcWallMs", "response.serverHlcWallMs"),
    ("serverHlcCounter", "response.serverHlcCounter"),
];

pub(super) fn request_structure(input: &Value) -> Result<(), CoreError> {
    let root = crate::strict_json::object(input, "reconciliation")?;
    let local = local_queues(root)?;
    let sent = crate::strict_json::object_field(root, "sent", "sent")?;
    let response = crate::strict_json::object_field(root, "response", "response")?;
    required_response_fields(response)?;
    queue_shape(local, "local")?;
    queue_shape(sent, "sent")?;
    response_shape(response)?;
    crate::strict_json::object_array_field(root, "timerDependencies", "timerDependencies", false)
}

fn required_response_fields(response: &Map<String, Value>) -> Result<(), CoreError> {
    for (field, projection) in REQUIRED_RESPONSE_FIELDS {
        if !response.contains_key(field) {
            return Err(CoreError::MissingProjection(projection));
        }
    }
    Ok(())
}

fn local_queues(root: &Map<String, Value>) -> Result<&Map<String, Value>, CoreError> {
    match (root.get("local"), root.get("pending")) {
        (Some(_), Some(_)) => Err(CoreError::Json(serde_json::Error::custom(
            "duplicate field `local`",
        ))),
        (Some(value), None) => crate::strict_json::object(value, "local"),
        (None, Some(value)) => crate::strict_json::object(value, "pending"),
        (None, None) => Err(CoreError::InvalidInput("missing local queues".into())),
    }
}

fn queue_shape(queues: &Map<String, Value>, path: &str) -> Result<(), CoreError> {
    for field in [
        "commands",
        "taskOperations",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ] {
        crate::strict_json::object_array_field(queues, field, &format!("{path}.{field}"), false)?;
    }
    Ok(())
}

fn response_shape(response: &Map<String, Value>) -> Result<(), CoreError> {
    for field in [
        "acknowledgements",
        "taskAcknowledgements",
        "durationAcknowledgements",
        "autoStartAcknowledgements",
        "selectedTaskAcknowledgements",
        "history",
        "tasks",
    ] {
        crate::strict_json::object_array_field(
            response,
            field,
            &format!("response.{field}"),
            true,
        )?;
    }
    crate::strict_json::object_field(response, "durationsMs", "response.durationsMs")?;
    let timer = crate::strict_json::nullable_object_field(
        response,
        "canonicalTimer",
        "response.canonicalTimer",
    )?;
    if let Some(timer) = timer {
        crate::strict_json::nullable_object_field(
            timer,
            "lastIntent",
            "response.canonicalTimer.lastIntent",
        )?;
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

pub(super) fn local_queue_values(
    local: &LocalQueues,
    response: &CanonicalResponse,
) -> Result<(), CoreError> {
    timer::replay(
        response.canonical_timer.0.clone(),
        response.history.clone(),
        local.commands.clone(),
        &response.server_time,
    )?;
    for operation in &local.task_operations {
        validate_operation_timestamp(&operation.clock)?;
        validate_operation_clock(&operation.clock)?;
        validate_task_operation_fields(operation)?;
    }
    for operation in &local.duration_operations {
        validate_operation_timestamp(&operation.clock)?;
        validate_operation_clock(&operation.clock)?;
        validate_duration_fields(operation)?;
    }
    for operation in &local.auto_start_operations {
        validate_operation_timestamp(&operation.clock)?;
        validate_operation_clock(&operation.clock)?;
    }
    for operation in &local.selected_task_operations {
        validate_operation_timestamp(&operation.clock)?;
        validate_operation_clock(&operation.clock)?;
        validate_selected_task_fields(operation)?;
    }
    Ok(())
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
    for task in tasks {
        let canonical = is_canonical_task_identity(&task.id, &task.title).unwrap_or(false);
        if !canonical || !ids.insert(task.id.as_str()) {
            return invalid_response("tasks");
        }
    }
    Ok(())
}

fn validate_durations(durations: &BTreeMap<String, i64>) -> Result<(), CoreError> {
    if !is_valid_duration_map(durations) {
        return invalid_response("durationsMs");
    }
    Ok(())
}

fn invalid_response<T>(field: &str) -> Result<T, CoreError> {
    Err(CoreError::InvalidInput(format!(
        "invalid canonical response {field}"
    )))
}
