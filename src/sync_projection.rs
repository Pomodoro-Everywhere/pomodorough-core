use std::collections::{BTreeMap, BTreeSet};

use chrono::DateTime;
use serde::{Deserialize, Serialize};

use crate::{CoreError, SelectedTaskField};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperationClock {
    pub(crate) id: String,
    pub(crate) device_id: String,
    pub(crate) occurred_at: String,
    pub(crate) hlc_wall_ms: i64,
    pub(crate) hlc_counter: i64,
}

fn operation_clock_key(clock: &OperationClock) -> (i64, i64, &str, &str) {
    (
        clock.hlc_wall_ms,
        clock.hlc_counter,
        clock.device_id.as_str(),
        clock.id.as_str(),
    )
}

fn validate_operation_clock(clock: &OperationClock) -> Result<(), CoreError> {
    DateTime::parse_from_rfc3339(&clock.occurred_at)
        .map(|_| ())
        .map_err(|_| CoreError::InvalidTimestamp(clock.occurred_at.clone()))
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct OperationsInput<T> {
    #[serde(default)]
    operations: Vec<T>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskOperation {
    #[serde(flatten)]
    pub(crate) clock: OperationClock,
    pub(crate) task_id: String,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) title: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Task {
    pub(crate) id: String,
    pub(crate) title: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskReductionOutput {
    pub(crate) tasks: Vec<Task>,
    pub(crate) winning_operation_ids: BTreeMap<String, String>,
}

pub(crate) fn reduce_tasks_v1_json(input: &str) -> Result<String, CoreError> {
    let input: OperationsInput<TaskOperation> = serde_json::from_str(input)?;
    Ok(serde_json::to_string(&replay_tasks(
        Vec::new(),
        input.operations,
    )?)?)
}

pub(crate) fn replay_tasks(
    base_tasks: Vec<Task>,
    operations: Vec<TaskOperation>,
) -> Result<TaskReductionOutput, CoreError> {
    for operation in &operations {
        validate_operation_clock(&operation.clock)?;
    }

    let mut winners: BTreeMap<String, TaskOperation> = BTreeMap::new();
    for operation in operations {
        let replace = winners.get(&operation.task_id).is_none_or(|current| {
            operation_clock_key(&operation.clock) > operation_clock_key(&current.clock)
        });
        if replace {
            winners.insert(operation.task_id.clone(), operation);
        }
    }

    let winning_operation_ids = winners
        .iter()
        .map(|(task_id, operation)| (task_id.clone(), operation.clock.id.clone()))
        .collect();
    let mut tasks_by_id = base_tasks
        .into_iter()
        .map(|task| (task.id.clone(), task))
        .collect::<BTreeMap<_, _>>();
    for operation in winners.into_values() {
        match operation.kind.as_str() {
            "upsert" => {
                tasks_by_id.insert(
                    operation.task_id.clone(),
                    Task {
                        id: operation.task_id,
                        title: operation.title,
                    },
                );
            }
            "delete" => {
                tasks_by_id.remove(&operation.task_id);
            }
            _ => {}
        }
    }
    let mut tasks = tasks_by_id.into_values().collect::<Vec<_>>();
    tasks.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(TaskReductionOutput {
        tasks,
        winning_operation_ids,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DurationOperation {
    #[serde(flatten)]
    pub(crate) clock: OperationClock,
    pub(crate) phase: String,
    pub(crate) duration_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DurationReductionOutput {
    pub(crate) durations_ms: BTreeMap<String, i64>,
    pub(crate) winning_operation_ids: BTreeMap<String, String>,
}

pub(crate) fn reduce_durations_v1_json(input: &str) -> Result<String, CoreError> {
    let input: OperationsInput<DurationOperation> = serde_json::from_str(input)?;
    Ok(serde_json::to_string(&replay_durations(
        None,
        input.operations,
    )?)?)
}

pub(crate) fn replay_durations(
    base_durations_ms: Option<BTreeMap<String, i64>>,
    operations: Vec<DurationOperation>,
) -> Result<DurationReductionOutput, CoreError> {
    for operation in &operations {
        validate_operation_clock(&operation.clock)?;
    }

    let mut winners: BTreeMap<String, DurationOperation> = BTreeMap::new();
    for operation in operations {
        let replace = winners.get(&operation.phase).is_none_or(|current| {
            operation_clock_key(&operation.clock) > operation_clock_key(&current.clock)
        });
        if replace {
            winners.insert(operation.phase.clone(), operation);
        }
    }

    let mut durations_ms = base_durations_ms.unwrap_or_else(|| {
        BTreeMap::from([
            ("focus".to_owned(), 1_500_000),
            ("short_break".to_owned(), 300_000),
            ("long_break".to_owned(), 900_000),
        ])
    });
    let mut winning_operation_ids = BTreeMap::new();
    for (phase, operation) in winners {
        winning_operation_ids.insert(phase.clone(), operation.clock.id);
        if let Some(duration) = durations_ms.get_mut(&phase) {
            *duration = operation.duration_ms;
        }
    }

    Ok(DurationReductionOutput {
        durations_ms,
        winning_operation_ids,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutoStartOperation {
    #[serde(flatten)]
    pub(crate) clock: OperationClock,
    pub(crate) enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutoStartReductionOutput {
    pub(crate) auto_start_breaks: bool,
    pub(crate) winning_operation_id: Option<String>,
}

pub(crate) fn reduce_auto_start_v1_json(input: &str) -> Result<String, CoreError> {
    let input: OperationsInput<AutoStartOperation> = serde_json::from_str(input)?;
    Ok(serde_json::to_string(&replay_auto_start(
        false,
        input.operations,
    )?)?)
}

pub(crate) fn replay_auto_start(
    base_auto_start_breaks: bool,
    operations: Vec<AutoStartOperation>,
) -> Result<AutoStartReductionOutput, CoreError> {
    for operation in &operations {
        validate_operation_clock(&operation.clock)?;
    }
    let winner = operations.into_iter().max_by(|left, right| {
        operation_clock_key(&left.clock).cmp(&operation_clock_key(&right.clock))
    });
    let output = match winner {
        Some(operation) => AutoStartReductionOutput {
            auto_start_breaks: operation.enabled,
            winning_operation_id: Some(operation.clock.id),
        },
        None => AutoStartReductionOutput {
            auto_start_breaks: base_auto_start_breaks,
            winning_operation_id: None,
        },
    };
    Ok(output)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectedTaskOperation {
    #[serde(flatten)]
    pub(crate) clock: OperationClock,
    #[serde(default, skip_serializing_if = "SelectedTaskField::is_omitted")]
    pub(crate) task_id: SelectedTaskField,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectedTaskReductionInput {
    #[serde(default)]
    operations: Vec<SelectedTaskOperation>,
    #[serde(default)]
    active_task_ids: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectedTaskReductionOutput {
    pub(crate) selected_task_id: Option<String>,
    pub(crate) winning_operation_id: Option<String>,
}

pub(crate) fn reduce_selected_task_v1_json(input: &str) -> Result<String, CoreError> {
    let input: SelectedTaskReductionInput = serde_json::from_str(input)?;
    Ok(serde_json::to_string(&replay_selected_task(
        None,
        input.operations,
        input.active_task_ids,
    )?)?)
}

pub(crate) fn replay_selected_task(
    base_selected_task_id: Option<String>,
    operations: Vec<SelectedTaskOperation>,
    active_task_ids: BTreeSet<String>,
) -> Result<SelectedTaskReductionOutput, CoreError> {
    for operation in &operations {
        validate_operation_clock(&operation.clock)?;
        if operation.task_id == SelectedTaskField::Omitted {
            return Err(CoreError::MissingProjection("operations.taskId"));
        }
    }

    let winner = operations.into_iter().max_by(|left, right| {
        operation_clock_key(&left.clock).cmp(&operation_clock_key(&right.clock))
    });
    let output = match winner {
        Some(operation) => {
            let selected_task_id = match operation.task_id {
                SelectedTaskField::Selected(task_id) if active_task_ids.contains(&task_id) => {
                    Some(task_id)
                }
                SelectedTaskField::Deselected | SelectedTaskField::Selected(_) => None,
                SelectedTaskField::Omitted => unreachable!("validated above"),
            };
            SelectedTaskReductionOutput {
                selected_task_id,
                winning_operation_id: Some(operation.clock.id),
            }
        }
        None => SelectedTaskReductionOutput {
            selected_task_id: base_selected_task_id
                .filter(|task_id| active_task_ids.contains(task_id)),
            winning_operation_id: None,
        },
    };
    Ok(output)
}
