use std::collections::{BTreeMap, BTreeSet};

use chrono::DateTime;
use serde::{Deserialize, Serialize};

use crate::{CoreError, SelectedTaskField};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationClock {
    id: String,
    device_id: String,
    occurred_at: String,
    hlc_wall_ms: i64,
    hlc_counter: i64,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskOperation {
    #[serde(flatten)]
    clock: OperationClock,
    task_id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Serialize)]
struct Task {
    id: String,
    title: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskReductionOutput {
    tasks: Vec<Task>,
    winning_operation_ids: BTreeMap<String, String>,
}

pub(crate) fn reduce_tasks_v1_json(input: &str) -> Result<String, CoreError> {
    let input: OperationsInput<TaskOperation> = serde_json::from_str(input)?;
    for operation in &input.operations {
        validate_operation_clock(&operation.clock)?;
    }

    let mut winners: BTreeMap<String, TaskOperation> = BTreeMap::new();
    for operation in input.operations {
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
    let mut tasks = winners
        .into_values()
        .filter(|operation| operation.kind == "upsert")
        .map(|operation| Task {
            id: operation.task_id,
            title: operation.title,
        })
        .collect::<Vec<_>>();
    tasks.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(serde_json::to_string(&TaskReductionOutput {
        tasks,
        winning_operation_ids,
    })?)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DurationOperation {
    #[serde(flatten)]
    clock: OperationClock,
    phase: String,
    duration_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DurationReductionOutput {
    durations_ms: BTreeMap<String, i64>,
    winning_operation_ids: BTreeMap<String, String>,
}

pub(crate) fn reduce_durations_v1_json(input: &str) -> Result<String, CoreError> {
    let input: OperationsInput<DurationOperation> = serde_json::from_str(input)?;
    for operation in &input.operations {
        validate_operation_clock(&operation.clock)?;
    }

    let mut winners: BTreeMap<String, DurationOperation> = BTreeMap::new();
    for operation in input.operations {
        let replace = winners.get(&operation.phase).is_none_or(|current| {
            operation_clock_key(&operation.clock) > operation_clock_key(&current.clock)
        });
        if replace {
            winners.insert(operation.phase.clone(), operation);
        }
    }

    let mut durations_ms = BTreeMap::from([
        ("focus".to_owned(), 1_500_000),
        ("short_break".to_owned(), 300_000),
        ("long_break".to_owned(), 900_000),
    ]);
    let mut winning_operation_ids = BTreeMap::new();
    for (phase, operation) in winners {
        winning_operation_ids.insert(phase.clone(), operation.clock.id);
        if let Some(duration) = durations_ms.get_mut(&phase) {
            *duration = operation.duration_ms;
        }
    }

    Ok(serde_json::to_string(&DurationReductionOutput {
        durations_ms,
        winning_operation_ids,
    })?)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoStartOperation {
    #[serde(flatten)]
    clock: OperationClock,
    enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutoStartReductionOutput {
    auto_start_breaks: bool,
    winning_operation_id: Option<String>,
}

pub(crate) fn reduce_auto_start_v1_json(input: &str) -> Result<String, CoreError> {
    let input: OperationsInput<AutoStartOperation> = serde_json::from_str(input)?;
    for operation in &input.operations {
        validate_operation_clock(&operation.clock)?;
    }
    let winner = input.operations.into_iter().max_by(|left, right| {
        operation_clock_key(&left.clock).cmp(&operation_clock_key(&right.clock))
    });
    let output = match winner {
        Some(operation) => AutoStartReductionOutput {
            auto_start_breaks: operation.enabled,
            winning_operation_id: Some(operation.clock.id),
        },
        None => AutoStartReductionOutput {
            auto_start_breaks: false,
            winning_operation_id: None,
        },
    };
    Ok(serde_json::to_string(&output)?)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectedTaskOperation {
    #[serde(flatten)]
    clock: OperationClock,
    #[serde(default)]
    task_id: SelectedTaskField,
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
struct SelectedTaskReductionOutput {
    selected_task_id: Option<String>,
    winning_operation_id: Option<String>,
}

pub(crate) fn reduce_selected_task_v1_json(input: &str) -> Result<String, CoreError> {
    let input: SelectedTaskReductionInput = serde_json::from_str(input)?;
    for operation in &input.operations {
        validate_operation_clock(&operation.clock)?;
        if operation.task_id == SelectedTaskField::Omitted {
            return Err(CoreError::MissingProjection("operations.taskId"));
        }
    }

    let winner = input.operations.into_iter().max_by(|left, right| {
        operation_clock_key(&left.clock).cmp(&operation_clock_key(&right.clock))
    });
    let output = match winner {
        Some(operation) => {
            let selected_task_id = match operation.task_id {
                SelectedTaskField::Selected(task_id)
                    if input.active_task_ids.contains(&task_id) =>
                {
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
            selected_task_id: None,
            winning_operation_id: None,
        },
    };
    Ok(serde_json::to_string(&output)?)
}
