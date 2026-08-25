use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::CoreError;
use crate::sync_projection::{
    AutoStartOperation, DurationOperation, OperationClock, SelectedTaskOperation, Task,
    TaskOperation, replay_auto_start, replay_durations, replay_selected_task, replay_tasks,
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
    let input: ProjectionApplyInput = serde_json::from_str(input)?;
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
        let (expected_id, normalized_title) = crate::task::identity(&task.title)?;
        if task.id != expected_id || task.title != normalized_title || !task_ids.insert(&task.id) {
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
        validate_clock(&operation.clock)?;
        if operation.task_id.is_empty() {
            return Err(CoreError::InvalidInput("invalid task identity".into()));
        }
        match operation.kind.as_str() {
            "upsert" => {
                let (expected_id, normalized_title) = crate::task::identity(&operation.title)?;
                if operation.task_id != expected_id || operation.title != normalized_title {
                    return Err(CoreError::InvalidInput(
                        "invalid task identity or title".into(),
                    ));
                }
            }
            "delete" => {}
            _ => {
                return Err(CoreError::InvalidInput(
                    "invalid task operation type".into(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_duration_operations(operations: &[DurationOperation]) -> Result<(), CoreError> {
    for operation in operations {
        validate_clock(&operation.clock)?;
        if !matches!(
            operation.phase.as_str(),
            "focus" | "short_break" | "long_break"
        ) || !(60_000..=14_400_000).contains(&operation.duration_ms)
        {
            return Err(CoreError::InvalidInput("invalid duration operation".into()));
        }
    }
    Ok(())
}

fn validate_auto_start_operations(operations: &[AutoStartOperation]) -> Result<(), CoreError> {
    for operation in operations {
        validate_clock(&operation.clock)?;
    }
    Ok(())
}

fn validate_selected_task_operations(
    operations: &[SelectedTaskOperation],
) -> Result<(), CoreError> {
    for operation in operations {
        validate_clock(&operation.clock)?;
        match &operation.task_id {
            crate::SelectedTaskField::Selected(task_id) if !task_id.is_empty() => {}
            crate::SelectedTaskField::Deselected => {}
            _ => {
                return Err(CoreError::InvalidInput(
                    "invalid selected task operation".into(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_clock(clock: &OperationClock) -> Result<(), CoreError> {
    const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
    if clock.id.is_empty()
        || clock.device_id.is_empty()
        || !(0..=MAX_SAFE_INTEGER).contains(&clock.hlc_wall_ms)
        || !(0..=MAX_SAFE_INTEGER).contains(&clock.hlc_counter)
        || chrono::DateTime::parse_from_rfc3339(&clock.occurred_at).is_err()
    {
        return Err(CoreError::InvalidInput("invalid operation clock".into()));
    }
    Ok(())
}

fn validate_base_durations(durations: &BTreeMap<String, i64>) -> Result<(), CoreError> {
    let phases = ["focus", "long_break", "short_break"];
    if durations.len() != phases.len()
        || phases.iter().any(|phase| {
            !durations
                .get(*phase)
                .is_some_and(|duration| (60_000..=14_400_000).contains(duration))
        })
    {
        return Err(CoreError::InvalidInput("invalid base durations".into()));
    }
    Ok(())
}
