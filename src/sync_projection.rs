use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use chrono::DateTime;
use serde::de::{Error as _, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{CoreError, SelectedTaskField, clock::validate_hlc_values};

pub(crate) const DURATION_PHASES: [&str; 3] = ["focus", "short_break", "long_break"];

pub(crate) fn is_valid_duration(phase: &str, duration_ms: i64) -> bool {
    DURATION_PHASES.contains(&phase)
        && (60_000..=10_800_000).contains(&duration_ms)
        && duration_ms % 60_000 == 0
}

pub(crate) fn is_valid_duration_map(durations: &BTreeMap<String, i64>) -> bool {
    durations.len() == DURATION_PHASES.len()
        && DURATION_PHASES.iter().all(|phase| {
            durations
                .get(*phase)
                .is_some_and(|duration| is_valid_duration(phase, *duration))
        })
}

pub(crate) fn deserialize_duration_map<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, i64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct DurationMapVisitor;

    impl<'de> Visitor<'de> for DurationMapVisitor {
        type Value = BTreeMap<String, i64>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a duration map with unique phase keys")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut durations = BTreeMap::new();
            while let Some((phase, duration_ms)) = map.next_entry::<String, i64>()? {
                if durations.contains_key(&phase) {
                    return Err(A::Error::custom(format!("duplicate field `{phase}`")));
                }
                durations.insert(phase, duration_ms);
            }
            Ok(durations)
        }
    }

    deserializer.deserialize_map(DurationMapVisitor)
}

fn select_clock_winners<T, K>(
    operations: Vec<T>,
    group_key: impl Fn(&T) -> K,
    compare_clocks: impl Fn(&T, &T) -> Ordering,
) -> BTreeMap<K, T>
where
    K: Ord,
{
    let mut winners = BTreeMap::new();
    for operation in operations {
        let key = group_key(&operation);
        let replace = winners
            .get(&key)
            .is_none_or(|current| compare_clocks(&operation, current).is_gt());
        if replace {
            winners.insert(key, operation);
        }
    }
    winners
}

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

pub(crate) fn validate_operation_clock(clock: &OperationClock) -> Result<(), CoreError> {
    if clock.id.is_empty()
        || clock.device_id.is_empty()
        || validate_hlc_values(clock.hlc_wall_ms, clock.hlc_counter).is_err()
        || validate_operation_timestamp(clock).is_err()
    {
        return Err(CoreError::InvalidInput("invalid operation clock".into()));
    }
    Ok(())
}

pub(crate) fn validate_operation_timestamp(clock: &OperationClock) -> Result<(), CoreError> {
    DateTime::parse_from_rfc3339(&clock.occurred_at)
        .map(|_| ())
        .map_err(|_| CoreError::InvalidTimestamp(clock.occurred_at.clone()))
}

fn validate_standalone_timestamps<'a>(
    clocks: impl IntoIterator<Item = &'a OperationClock>,
) -> Result<(), CoreError> {
    for clock in clocks {
        validate_operation_timestamp(clock)?;
    }
    Ok(())
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
    let value = strict_operations_input(input, false)?;
    let input: OperationsInput<TaskOperation> = serde_json::from_value(value)?;
    validate_standalone_timestamps(input.operations.iter().map(|operation| &operation.clock))?;
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
        validate_task_id(&operation.task_id)?;
    }

    let winners = select_clock_winners(
        operations,
        |operation| operation.task_id.clone(),
        |left, right| operation_clock_key(&left.clock).cmp(&operation_clock_key(&right.clock)),
    );
    let winning_operation_ids = winners
        .iter()
        .map(|(task_id, operation)| (task_id.clone(), operation.clock.id.clone()))
        .collect();
    let mut tasks = apply_task_operations(base_tasks, winners)
        .into_values()
        .collect::<Vec<_>>();
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

pub(crate) fn validate_task_id(task_id: &str) -> Result<(), CoreError> {
    if task_id.is_empty() {
        return Err(CoreError::InvalidInput("invalid task identity".into()));
    }
    Ok(())
}

pub(crate) fn is_canonical_task_identity(task_id: &str, title: &str) -> Result<bool, CoreError> {
    let (expected_id, normalized_title) = crate::task::identity(title)?;
    Ok(task_id == expected_id && title == normalized_title)
}

pub(crate) fn validate_task_operation_fields(operation: &TaskOperation) -> Result<(), CoreError> {
    validate_task_id(&operation.task_id)?;
    match operation.kind.as_str() {
        "upsert" if is_canonical_task_identity(&operation.task_id, &operation.title)? => Ok(()),
        "upsert" => Err(CoreError::InvalidInput(
            "invalid task identity or title".into(),
        )),
        "delete" => Ok(()),
        _ => Err(CoreError::InvalidInput(
            "invalid task operation type".into(),
        )),
    }
}

fn apply_task_operations(
    base_tasks: Vec<Task>,
    winners: BTreeMap<String, TaskOperation>,
) -> BTreeMap<String, Task> {
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
    tasks_by_id
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
    let value = strict_operations_input(input, false)?;
    let input: OperationsInput<DurationOperation> = serde_json::from_value(value)?;
    validate_standalone_timestamps(input.operations.iter().map(|operation| &operation.clock))?;
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
        validate_duration_fields(operation)?;
    }

    let winners = select_clock_winners(
        operations,
        |operation| operation.phase.clone(),
        |left, right| operation_clock_key(&left.clock).cmp(&operation_clock_key(&right.clock)),
    );

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

pub(crate) fn validate_duration_fields(operation: &DurationOperation) -> Result<(), CoreError> {
    if !is_valid_duration(&operation.phase, operation.duration_ms) {
        return Err(CoreError::InvalidInput("invalid duration operation".into()));
    }
    Ok(())
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
    let value = strict_operations_input(input, false)?;
    let input: OperationsInput<AutoStartOperation> = serde_json::from_value(value)?;
    validate_standalone_timestamps(input.operations.iter().map(|operation| &operation.clock))?;
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
    let value = strict_operations_input(input, true)?;
    let input: SelectedTaskReductionInput = serde_json::from_value(value)?;
    validate_standalone_timestamps(input.operations.iter().map(|operation| &operation.clock))?;
    Ok(serde_json::to_string(&replay_selected_task(
        None,
        input.operations,
        input.active_task_ids,
    )?)?)
}

fn strict_operations_input(
    input: &str,
    selected_task: bool,
) -> Result<serde_json::Value, CoreError> {
    let value = crate::strict_json::parse(input)?;
    let root = crate::strict_json::object(&value, "reducer input")?;
    crate::strict_json::object_array_field(root, "operations", "operations", false)?;
    if selected_task {
        crate::strict_json::array_field(root, "activeTaskIds", "activeTaskIds", false)?;
    }
    Ok(value)
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

pub(crate) fn validate_selected_task_fields(
    operation: &SelectedTaskOperation,
) -> Result<(), CoreError> {
    match &operation.task_id {
        SelectedTaskField::Selected(task_id) if !task_id.is_empty() => Ok(()),
        SelectedTaskField::Deselected => Ok(()),
        _ => Err(CoreError::InvalidInput(
            "invalid selected task operation".into(),
        )),
    }
}
