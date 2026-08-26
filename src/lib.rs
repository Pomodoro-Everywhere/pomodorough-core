use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

mod bootstrap;
mod clock;
mod completion_plan;
mod fixture_projection;
mod projection;
mod reconciliation;
mod sync_projection;
mod task;
mod timer;

#[cfg(target_arch = "wasm32")]
mod wasm_abi;

pub use fixture_projection::{reduce_projection_fixture_case_json, reduce_selected_task_json};
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
        "projection.reduce" => fixture_projection::reduce_projection_fixture_case_json(input),
        "projection.apply.v2" => projection::apply_v2_json(input),
        "task.reduce.v1" => sync_projection::reduce_tasks_v1_json(input),
        "task.identity.v1" => task::identity_json(input),
        "duration.reduce.v1" => sync_projection::reduce_durations_v1_json(input),
        "autoStart.reduce.v1" => sync_projection::reduce_auto_start_v1_json(input),
        "selectedTask.reduce" => fixture_projection::reduce_selected_task_json(input),
        "selectedTask.reduce.v1" => sync_projection::reduce_selected_task_v1_json(input),
        "selectedTask.classify" => classify_selected_task_field_json(input),
        "reconcile.rebase.v1" => reconciliation::rebase_v1_json(input),
        "bootstrap.plan.v1" => bootstrap::plan_v1_json(input),
        "timer.completionPlan.v1" => completion_plan::plan_v1_json(input),
        "hlc.tick.v1" => clock::tick_json(input),
        "uuidv7.fromParts.v1" => clock::uuid_v7_from_parts_json(input),
        other => Err(CoreError::UnsupportedOperation(other.to_owned())),
    }
}
