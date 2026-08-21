use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

mod bootstrap;
mod clock;
mod reconciliation;
mod sync_projection;
mod task;
mod timer;

#[cfg(target_arch = "wasm32")]
mod wasm_abi;

pub use timer::reduce_timer_fixture_case_json;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SelectedTaskField {
    #[default]
    Omitted,
    Deselected,
    Selected(String),
}

impl<'de> Deserialize<'de> for SelectedTaskField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = SelectedTaskField;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("null or a selected task identifier")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(SelectedTaskField::Deselected)
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(SelectedTaskField::Deselected)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SelectedTaskField::Selected(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(SelectedTaskField::Selected(value))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl Serialize for SelectedTaskField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Omitted | Self::Deselected => serializer.serialize_none(),
            Self::Selected(task_id) => serializer.serialize_str(task_id),
        }
    }
}

impl SelectedTaskField {
    fn is_omitted(&self) -> bool {
        self == &Self::Omitted
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectedTaskEnvelope {
    #[serde(default)]
    selected_task_id: SelectedTaskField,
}

pub fn classify_selected_task_field_json(input: &str) -> Result<String, CoreError> {
    let envelope: SelectedTaskEnvelope = serde_json::from_str(input)?;
    Ok(match envelope.selected_task_id {
        SelectedTaskField::Omitted => "omitted".into(),
        SelectedTaskField::Deselected => "deselected".into(),
        SelectedTaskField::Selected(id) => format!("selected:{id}"),
    })
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid shared-core JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid RFC 3339 timestamp: {0}")]
    InvalidTimestamp(String),
    #[error("missing required projection value: {0}")]
    MissingProjection(&'static str),
    #[error("unsupported shared-core operation: {0}")]
    UnsupportedOperation(String),
    #[error("invalid shared-core input: {0}")]
    InvalidInput(String),
}

pub fn dispatch_envelope_json(operation: &str, input: &str) -> String {
    match dispatch_json(operation, input) {
        Ok(value) => {
            let value = serde_json::from_str::<serde_json::Value>(&value)
                .unwrap_or(serde_json::Value::String(value));
            serde_json::json!({"ok": true, "value": value}).to_string()
        }
        Err(error) => serde_json::json!({"ok": false, "error": error.to_string()}).to_string(),
    }
}

pub fn dispatch_json(operation: &str, input: &str) -> Result<String, CoreError> {
    match operation {
        "core.version" => Ok(serde_json::json!({
            "schemaVersion": 1,
            "coreVersion": env!("CARGO_PKG_VERSION"),
        })
        .to_string()),
        "timer.reduce" => reduce_timer_fixture_case_json(input),
        "timer.reduce.v1" => timer::reduce_timer_v1_json(input),
        "projection.reduce" => reduce_projection_fixture_case_json(input),
        "task.reduce.v1" => sync_projection::reduce_tasks_v1_json(input),
        "task.identity.v1" => task::identity_json(input),
        "duration.reduce.v1" => sync_projection::reduce_durations_v1_json(input),
        "autoStart.reduce.v1" => sync_projection::reduce_auto_start_v1_json(input),
        "selectedTask.reduce" => reduce_selected_task_json(input),
        "selectedTask.reduce.v1" => sync_projection::reduce_selected_task_v1_json(input),
        "selectedTask.classify" => classify_selected_task_field_json(input),
        "reconcile.rebase.v1" => reconciliation::rebase_v1_json(input),
        "bootstrap.plan.v1" => bootstrap::plan_v1_json(input),
        "hlc.tick.v1" => clock::tick_json(input),
        "uuidv7.fromParts.v1" => clock::uuid_v7_from_parts_json(input),
        other => Err(CoreError::UnsupportedOperation(other.to_owned())),
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectionInput {
    task_operations: Vec<TaskOperation>,
    duration_operations: Vec<DurationOperation>,
    auto_start_operations: Vec<AutoStartOperation>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationClock {
    id: String,
    device_id: String,
    wall_ms: i64,
    counter: i64,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DurationOperation {
    #[serde(flatten)]
    clock: OperationClock,
    phase: String,
    duration_ms: i64,
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
struct ProjectionOutput {
    tasks: Vec<ProjectedTask>,
    durations_ms: BTreeMap<String, i64>,
    auto_start_breaks: bool,
}

#[derive(Debug, Serialize)]
struct ProjectedTask {
    id: String,
    title: String,
}

pub fn reduce_projection_fixture_case_json(input: &str) -> Result<String, CoreError> {
    let input: ProjectionInput = serde_json::from_str(input)?;

    let mut latest_tasks: BTreeMap<String, TaskOperation> = BTreeMap::new();
    for operation in input.task_operations {
        let replace = latest_tasks.get(&operation.task_id).is_none_or(|current| {
            operation_clock_key(&operation.clock) > operation_clock_key(&current.clock)
        });
        if replace {
            latest_tasks.insert(operation.task_id.clone(), operation);
        }
    }
    let tasks = latest_tasks
        .into_values()
        .filter(|operation| operation.kind == "upsert")
        .map(|operation| ProjectedTask {
            id: operation.task_id,
            title: operation.title,
        })
        .collect();

    let mut latest_durations: BTreeMap<String, DurationOperation> = BTreeMap::new();
    for operation in input.duration_operations {
        let replace = latest_durations
            .get(&operation.phase)
            .is_none_or(|current| {
                operation_clock_key(&operation.clock) > operation_clock_key(&current.clock)
            });
        if replace {
            latest_durations.insert(operation.phase.clone(), operation);
        }
    }
    let durations_ms = latest_durations
        .into_iter()
        .map(|(phase, operation)| (phase, operation.duration_ms))
        .collect();

    let auto_start_breaks = input
        .auto_start_operations
        .into_iter()
        .max_by(|left, right| {
            operation_clock_key(&left.clock).cmp(&operation_clock_key(&right.clock))
        })
        .ok_or(CoreError::MissingProjection("autoStartOperations"))?
        .enabled;

    Ok(serde_json::to_string(&ProjectionOutput {
        tasks,
        durations_ms,
        auto_start_breaks,
    })?)
}

fn operation_clock_key(clock: &OperationClock) -> (i64, i64, &str, &str) {
    (
        clock.wall_ms,
        clock.counter,
        clock.device_id.as_str(),
        clock.id.as_str(),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectedTaskReductionInput {
    operations: Vec<SelectedTaskOperation>,
    active_task_ids: BTreeSet<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectedTaskOperation {
    #[serde(flatten)]
    clock: OperationClock,
    task_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectedTaskReductionOutput {
    selected_task_id: Option<String>,
}

pub fn reduce_selected_task_json(input: &str) -> Result<String, CoreError> {
    let input: SelectedTaskReductionInput = serde_json::from_str(input)?;
    let selected_task_id = input
        .operations
        .into_iter()
        .max_by(|left, right| {
            operation_clock_key(&left.clock).cmp(&operation_clock_key(&right.clock))
        })
        .and_then(|operation| operation.task_id)
        .filter(|task_id| input.active_task_ids.contains(task_id));
    Ok(serde_json::to_string(&SelectedTaskReductionOutput {
        selected_task_id,
    })?)
}
