use std::collections::BTreeSet;

use serde_json::Value;

use crate::CoreError;
use crate::sync_projection::{
    AutoStartOperation, DurationOperation, SelectedTaskOperation, TaskOperation,
};
use crate::timer::WireCommand;

use super::timer_dependencies::TimerDependencyResolution;
use super::{CanonicalResponse, Identified, LocalQueues, SentQueues};

pub(super) struct AcknowledgedIds {
    commands: BTreeSet<String>,
    tasks: BTreeSet<String>,
    durations: BTreeSet<String>,
    auto_start: BTreeSet<String>,
    selected_task: BTreeSet<String>,
}

pub(super) struct PendingQueues {
    pub(super) commands: Vec<WireCommand>,
    pub(super) tasks: Vec<TaskOperation>,
    pub(super) durations: Vec<DurationOperation>,
    pub(super) auto_start: Vec<AutoStartOperation>,
    pub(super) selected_task: Vec<SelectedTaskOperation>,
}

pub(super) fn validate(
    sent: &SentQueues,
    response: &CanonicalResponse,
) -> Result<AcknowledgedIds, CoreError> {
    Ok(AcknowledgedIds {
        commands: validate_set(
            "acknowledgements",
            &sent.commands,
            &response.acknowledgements,
            "commandId",
        )?,
        tasks: validate_set(
            "taskAcknowledgements",
            &sent.task_operations,
            &response.task_acknowledgements,
            "operationId",
        )?,
        durations: validate_set(
            "durationAcknowledgements",
            &sent.duration_operations,
            &response.duration_acknowledgements,
            "operationId",
        )?,
        auto_start: validate_set(
            "autoStartAcknowledgements",
            &sent.auto_start_operations,
            &response.auto_start_acknowledgements,
            "operationId",
        )?,
        selected_task: validate_set(
            "selectedTaskAcknowledgements",
            &sent.selected_task_operations,
            &response.selected_task_acknowledgements,
            "operationId",
        )?,
    })
}

pub(super) fn filter_pending(
    local: LocalQueues,
    acknowledged: &AcknowledgedIds,
    timer_resolution: &TimerDependencyResolution,
) -> PendingQueues {
    PendingQueues {
        commands: local
            .commands
            .into_iter()
            .filter(|operation| {
                !acknowledged.commands.contains(&operation.id)
                    && !timer_resolution
                        .dropped_operation_ids
                        .contains(&operation.id)
            })
            .collect(),
        tasks: local
            .task_operations
            .into_iter()
            .filter(|operation| !acknowledged.tasks.contains(&operation.clock.id))
            .collect(),
        durations: local
            .duration_operations
            .into_iter()
            .filter(|operation| !acknowledged.durations.contains(&operation.clock.id))
            .collect(),
        auto_start: local
            .auto_start_operations
            .into_iter()
            .filter(|operation| !acknowledged.auto_start.contains(&operation.clock.id))
            .collect(),
        selected_task: local
            .selected_task_operations
            .into_iter()
            .filter(|operation| !acknowledged.selected_task.contains(&operation.clock.id))
            .collect(),
    }
}

fn validate_set(
    field: &str,
    sent: &[Identified],
    acknowledgements: &[Value],
    id_field: &str,
) -> Result<BTreeSet<String>, CoreError> {
    let expected_ids = sent
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    if expected_ids.len() != sent.len()
        || expected_ids.iter().any(|identifier| identifier.is_empty())
        || acknowledgements.len() != expected_ids.len()
    {
        return invalid_set(field);
    }

    let mut acknowledged_ids = BTreeSet::new();
    for acknowledgement in acknowledgements {
        let Some(object) = acknowledgement.as_object() else {
            return invalid_set(field);
        };
        let Some(identifier) = object.get(id_field).and_then(Value::as_str) else {
            return invalid_set(field);
        };
        let Some(outcome) = object.get("outcome").and_then(Value::as_str) else {
            return invalid_set(field);
        };
        if object.get("reason").and_then(Value::as_str).is_none()
            || !matches!(outcome, "applied" | "ignored" | "rejected")
            || !expected_ids.contains(identifier)
            || !acknowledged_ids.insert(identifier.to_owned())
        {
            return invalid_set(field);
        }
    }
    Ok(acknowledged_ids)
}

fn invalid_set<T>(field: &str) -> Result<T, CoreError> {
    Err(CoreError::InvalidInput(format!("invalid {field} set")))
}
