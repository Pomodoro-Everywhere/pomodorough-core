use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::CoreError;
use crate::sync_projection::{
    AutoStartOperation, DurationOperation, SelectedTaskOperation, Task, TaskOperation,
    deserialize_duration_map, is_canonical_task_identity, is_valid_duration_map, replay_auto_start,
    replay_durations, replay_selected_task, replay_tasks, validate_duration_fields,
    validate_operation_clock, validate_selected_task_fields, validate_task_operation_fields,
};
use crate::timer::{CanonicalTimer, HistoryItem, Outcome, WireCommand, replay};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectionApplyInput {
    base: ProjectionBase,
    pending: ProjectionPending,
    now: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectionBase {
    #[serde(default)]
    canonical_timer: Option<CanonicalTimer>,
    #[serde(default)]
    history: Vec<HistoryItem>,
    #[serde(default)]
    tasks: Vec<Task>,
    #[serde(deserialize_with = "deserialize_duration_map")]
    durations_ms: BTreeMap<String, i64>,
    auto_start_breaks: bool,
    #[serde(default)]
    selected_task_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectionPending {
    #[serde(default)]
    commands: Vec<WireCommand>,
    #[serde(default)]
    task_operations: Vec<TaskOperation>,
    #[serde(default)]
    duration_operations: Vec<DurationOperation>,
    #[serde(default)]
    auto_start_operations: Vec<AutoStartOperation>,
    #[serde(default)]
    selected_task_operations: Vec<SelectedTaskOperation>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectionApplyOutput {
    canonical_timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    tasks: Vec<Task>,
    durations_ms: BTreeMap<String, i64>,
    auto_start_breaks: bool,
    selected_task_id: Option<String>,
    timer_outcomes: BTreeMap<String, Outcome>,
    winning_operation_ids: WinningOperationIds,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WinningOperationIds {
    tasks: BTreeMap<String, String>,
    durations: BTreeMap<String, String>,
    auto_start: Option<String>,
    selected_task: Option<String>,
}

pub(crate) fn apply_v2_json(input: &str) -> Result<String, CoreError> {
    let value = crate::strict_json::parse(input)?;
    validate_projection_shape(&value)?;
    let input: ProjectionApplyInput = serde_json::from_value(value)?;
    validate_base_durations(&input.base.durations_ms)?;
    validate_projection_input(&input)?;

    let timer = replay(
        input.base.canonical_timer,
        input.base.history,
        input.pending.commands,
        &input.now,
    )?;
    let tasks = replay_tasks(input.base.tasks, input.pending.task_operations)?;
    let durations = replay_durations(
        Some(input.base.durations_ms),
        input.pending.duration_operations,
    )?;
    let auto_start = replay_auto_start(
        input.base.auto_start_breaks,
        input.pending.auto_start_operations,
    )?;
    let active_task_ids = tasks
        .tasks
        .iter()
        .map(|task| task.id.clone())
        .collect::<BTreeSet<_>>();
    let selected_task = replay_selected_task(
        input.base.selected_task_id,
        input.pending.selected_task_operations,
        active_task_ids,
    )?;

    Ok(serde_json::to_string(&ProjectionApplyOutput {
        canonical_timer: timer.canonical_timer,
        history: timer.history,
        tasks: tasks.tasks,
        durations_ms: durations.durations_ms,
        auto_start_breaks: auto_start.auto_start_breaks,
        selected_task_id: selected_task.selected_task_id,
        timer_outcomes: timer.outcomes,
        winning_operation_ids: WinningOperationIds {
            tasks: tasks.winning_operation_ids,
            durations: durations.winning_operation_ids,
            auto_start: auto_start.winning_operation_id,
            selected_task: selected_task.winning_operation_id,
        },
    })?)
}

fn validate_projection_shape(value: &serde_json::Value) -> Result<(), CoreError> {
    let root = crate::strict_json::object(value, "projection")?;
    let base = crate::strict_json::object_field(root, "base", "base")?;
    let pending = crate::strict_json::object_field(root, "pending", "pending")?;
    validate_projection_base_shape(base)?;
    validate_queue_shape(pending, "pending")
}

fn validate_projection_base_shape(
    base: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), CoreError> {
    let timer =
        crate::strict_json::nullable_object_field(base, "canonicalTimer", "base.canonicalTimer")?;
    if let Some(timer) = timer {
        crate::strict_json::nullable_object_field(
            timer,
            "lastIntent",
            "base.canonicalTimer.lastIntent",
        )?;
    }
    crate::strict_json::object_array_field(base, "history", "base.history", false)?;
    crate::strict_json::object_array_field(base, "tasks", "base.tasks", false)?;
    crate::strict_json::object_field(base, "durationsMs", "base.durationsMs")?;
    Ok(())
}

fn validate_queue_shape(
    queues: &serde_json::Map<String, serde_json::Value>,
    path: &str,
) -> Result<(), CoreError> {
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

fn validate_projection_input(input: &ProjectionApplyInput) -> Result<(), CoreError> {
    validate_base_tasks(&input.base.tasks)?;
    validate_base_selected_task(&input.base.selected_task_id)?;
    validate_task_operations(&input.pending.task_operations)?;
    validate_duration_operations(&input.pending.duration_operations)?;
    validate_auto_start_operations(&input.pending.auto_start_operations)?;
    validate_selected_task_operations(&input.pending.selected_task_operations)
}

fn validate_base_tasks(tasks: &[Task]) -> Result<(), CoreError> {
    let mut task_ids = BTreeSet::new();
    for task in tasks {
        if !is_canonical_task_identity(&task.id, &task.title)? || !task_ids.insert(&task.id) {
            return Err(CoreError::InvalidInput(
                "invalid base task identity or title".into(),
            ));
        }
    }
    Ok(())
}

fn validate_base_selected_task(selected_task_id: &Option<String>) -> Result<(), CoreError> {
    if selected_task_id
        .as_ref()
        .is_some_and(|task_id| task_id.is_empty())
    {
        return Err(CoreError::InvalidInput(
            "invalid base selected task identity".into(),
        ));
    }
    Ok(())
}

fn validate_task_operations(operations: &[TaskOperation]) -> Result<(), CoreError> {
    for operation in operations {
        validate_operation_clock(&operation.clock)?;
        validate_task_operation_fields(operation)?;
    }
    Ok(())
}

fn validate_duration_operations(operations: &[DurationOperation]) -> Result<(), CoreError> {
    for operation in operations {
        validate_operation_clock(&operation.clock)?;
        validate_duration_fields(operation)?;
    }
    Ok(())
}

fn validate_auto_start_operations(operations: &[AutoStartOperation]) -> Result<(), CoreError> {
    for operation in operations {
        validate_operation_clock(&operation.clock)?;
    }
    Ok(())
}

fn validate_selected_task_operations(
    operations: &[SelectedTaskOperation],
) -> Result<(), CoreError> {
    for operation in operations {
        validate_operation_clock(&operation.clock)?;
        validate_selected_task_fields(operation)?;
    }
    Ok(())
}

fn validate_base_durations(durations: &BTreeMap<String, i64>) -> Result<(), CoreError> {
    if !is_valid_duration_map(durations) {
        return Err(CoreError::InvalidInput("invalid base durations".into()));
    }
    Ok(())
}
