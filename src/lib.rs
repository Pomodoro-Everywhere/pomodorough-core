use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

mod batch_plan;
mod bootstrap;
mod clock;
mod completion_plan;
mod completion_schema;
mod fixture_projection;
mod legacy_dependencies;
mod legacy_preferences;
mod ownership_plan;
mod projection;
mod read_model;
mod reconciliation;
mod strict_json;
mod sync_projection;
mod task;
mod timer;
mod timer_ownership;
mod trusted_clock;
mod workspace_intent;

#[cfg(any(target_arch = "wasm32", test))]
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
    check_input_len(input)?;
    let envelope: SelectedTaskEnvelope = serde_json::from_str(input)?;
    Ok(match envelope.selected_task_id {
        SelectedTaskField::Omitted => "omitted".into(),
        SelectedTaskField::Deselected => "deselected".into(),
        SelectedTaskField::Selected(id) => format!("selected:{id}"),
    })
}

/// Maximum native JSON input accepted before deserialization.
/// Matches the WASM ABI buffer cap so native and WASM reject the same class.
pub(crate) const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
/// Maximum timer commands accepted in one reduction.
/// History stays unbounded so a valid account never blocks on row count;
/// the 16 MiB JSON input cap still bounds request memory.
pub(crate) const MAX_COMMANDS: usize = 10_000;
/// Maximum HLC clocks accepted in one head computation.
pub(crate) const MAX_OBSERVED_CLOCKS: usize = 10_000;
/// Maximum bootstrap task and queue entries accepted per side.
/// History arrays are intentionally unbounded; this still bounds tasks,
/// known tasks, and pending queues.
pub(crate) const MAX_BOOTSTRAP_HISTORY: usize = 10_000;
/// Maximum operation name bytes accepted by native dispatch.
/// Matches the WASM ABI operation cap.
pub(crate) const MAX_OPERATION_BYTES: usize = 256;

pub(crate) fn check_input_len(input: &str) -> Result<(), CoreError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(CoreError::InvalidInput(format!(
            "input exceeds {MAX_INPUT_BYTES} bytes"
        )));
    }
    Ok(())
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

/// Move a required field out without a Null fallback. History stays owned by
/// the returned value, never truncated. The error carries only operation,
/// byte-length, and row-count context, never field contents or titles.
pub(crate) fn take_required_field(
    root: &mut serde_json::Value,
    field: &'static str,
    op: &'static str,
    history: usize,
    queued: usize,
) -> Result<serde_json::Value, CoreError> {
    match root.get_mut(field).map(serde_json::Value::take) {
        Some(value) => Ok(value),
        None => {
            let bytes = serde_json::to_string(&root)
                .map(|body| body.len())
                .unwrap_or(0);
            Err(CoreError::InvalidInput(format!(
                "missing required workspace field op={op} field={field} \
                 history={history} queued={queued} bytes={bytes}"
            )))
        }
    }
}

/// Move the composed `workspace` out of a projection envelope. A missing or
/// Null workspace previously flowed downstream as Null and failed later as a
/// generic type error; fail here with operation and count context instead.
pub(crate) fn take_projection_workspace(
    output: &mut serde_json::Value,
    op: &'static str,
    history: usize,
    queued: usize,
) -> Result<serde_json::Value, CoreError> {
    match output.get_mut("workspace").map(serde_json::Value::take) {
        Some(workspace) if !workspace.is_null() => Ok(workspace),
        _ => {
            let bytes = serde_json::to_string(&output)
                .map(|body| body.len())
                .unwrap_or(0);
            Err(CoreError::InvalidInput(format!(
                "missing projection workspace op={op} \
                 history={history} queued={queued} bytes={bytes}"
            )))
        }
    }
}

pub fn dispatch_envelope_json(operation: &str, input: &str) -> String {
    match dispatch_json(operation, input) {
        Ok(value) => {
            if matches!(
                operation,
                "clock.observe.v1"
                    | "workspace.legacyPreferences.v1"
                    | "workspace.legacyDependencyPlan.v1"
            ) || (operation == "workspace.ownershipPlan.v1"
                && serde_json::from_str::<serde_json::Value>(input)
                    .is_ok_and(|input| input["action"]["kind"] == "release"))
            {
                return format!("{{\"ok\":true,\"value\":{value}}}");
            }
            let value = serde_json::from_str::<serde_json::Value>(&value)
                .unwrap_or(serde_json::Value::String(value));
            serde_json::json!({"ok": true, "value": value}).to_string()
        }
        Err(error) => serde_json::json!({"ok": false, "error": error.to_string()}).to_string(),
    }
}

pub fn dispatch_json(operation: &str, input: &str) -> Result<String, CoreError> {
    if operation.len() > MAX_OPERATION_BYTES {
        return Err(CoreError::InvalidInput(format!(
            "operation exceeds {MAX_OPERATION_BYTES} bytes"
        )));
    }
    check_input_len(input)?;
    match operation {
        "core.version" => Ok(serde_json::json!({
            "schemaVersion": 1,
            "coreVersion": env!("CARGO_PKG_VERSION"),
        })
        .to_string()),
        "timer.reduce" => reduce_timer_fixture_case_json(input),
        "timer.reduce.v1" => timer::reduce_timer_v1_json(input),
        "timer.replay.page.v1" => timer::replay_page::reduce_json(input),
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
        "reconcile.rebase.v2" => reconciliation::rebase_v2_json(input),
        "reconcile.rebase.v3" => reconciliation::terminal::rebase_json(input),
        "workspace.project.v1" => reconciliation::workspace::project_json(input),
        "workspace.ownershipPlan.v1" => ownership_plan::plan_json(input),
        "workspace.readModel.v1" => read_model::read_json(input),
        "workspace.intent.v1" => workspace_intent::plan_json(input),
        "workspace.legacyPreferences.v1" => legacy_preferences::plan_json(input),
        "workspace.legacyDependencyPlan.v1" => legacy_dependencies::plan_json(input),
        "workspace.completionMutation.v1" => {
            workspace_intent::completion_mutation::plan_json(input)
        }
        "bootstrap.plan.v1" => bootstrap::plan_v1_json(input),
        "bootstrap.workspacePlan.v1" => bootstrap::workspace::plan_json(input),
        "sync.batchPlan.v1" => batch_plan::plan_json(input),
        "timer.completionPlan.v1" => completion_plan::plan_v1_json(input),
        "timer.completionState.v1" => completion_plan::state::plan_json(input),
        "hlc.head.v1" => clock::head_json(input),
        "clock.observe.v1" => trusted_clock::observe_json(input),
        "hlc.tick.v1" => clock::tick_json(input),
        "uuidv7.fromParts.v1" => clock::uuid_v7_from_parts_json(input),
        other => Err(CoreError::UnsupportedOperation(other.to_owned())),
    }
}

#[cfg(test)]
mod sweep_helper_tests {
    use super::{take_projection_workspace, take_required_field};
    use serde_json::{Value, json};

    #[test]
    fn missing_field_reports_operation_and_counts_without_contents() {
        let mut root = json!({"unrelated": [{"title": "SweepSecretTitle-7f3a"}]});
        let error = take_required_field(&mut root, "base", "op.test", 3, 2)
            .unwrap_err()
            .to_string();
        assert!(error.contains("op=op.test"), "{error}");
        assert!(error.contains("field=base"), "{error}");
        assert!(error.contains("history=3"), "{error}");
        assert!(error.contains("queued=2"), "{error}");
        assert!(error.contains("bytes="), "{error}");
        assert!(!error.contains("SweepSecretTitle-7f3a"), "{error}");
    }

    #[test]
    fn present_field_moves_by_value_without_truncation() {
        let mut root = json!({"base": {"history": [1, 2, 3]}});
        let moved = take_required_field(&mut root, "base", "op.test", 3, 0).unwrap();
        assert_eq!(moved, json!({"history": [1, 2, 3]}));
        assert!(root["base"].is_null());
    }

    #[test]
    fn missing_or_null_workspace_reports_operation_and_counts() {
        for output in [json!({}), json!({"workspace": Value::Null})] {
            let mut output = output;
            let error = take_projection_workspace(&mut output, "op.test", 5, 1)
                .unwrap_err()
                .to_string();
            assert!(error.contains("op=op.test"), "{error}");
            assert!(error.contains("history=5"), "{error}");
            assert!(error.contains("queued=1"), "{error}");
        }
    }

    #[test]
    fn present_workspace_moves_by_value() {
        let mut output = json!({"workspace": {"history": [1, 2]}, "other": true});
        let moved = take_projection_workspace(&mut output, "op.test", 2, 0).unwrap();
        assert_eq!(moved, json!({"history": [1, 2]}));
        assert!(output["workspace"].is_null());
    }
}
