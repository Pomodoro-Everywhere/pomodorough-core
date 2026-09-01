use std::collections::BTreeSet;
use std::fmt;

use serde::de::{Error as _, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::CoreError;
use crate::sync_projection::{
    AutoStartOperation, DurationOperation, SelectedTaskOperation, TaskOperation,
};
use crate::timer::WireCommand;

use super::timer_dependencies::TimerDependencyResolution;
use super::{CanonicalResponse, Identified, LocalQueues, SentQueues};

#[derive(Default)]
pub(super) struct Acknowledgement {
    command_id: AcknowledgementString,
    operation_id: AcknowledgementString,
    outcome: AcknowledgementString,
    reason: AcknowledgementString,
}

impl Acknowledgement {
    pub(super) fn identifier(&self, field: &str) -> Option<&str> {
        match field {
            "commandId" => self.command_id.as_deref(),
            "operationId" => self.operation_id.as_deref(),
            _ => None,
        }
    }

    pub(super) fn outcome(&self) -> Option<&str> {
        self.outcome.as_deref()
    }
}

#[derive(Default)]
struct AcknowledgementString(Option<String>);

impl AcknowledgementString {
    fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

impl<'de> Deserialize<'de> for AcknowledgementString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(AcknowledgementStringVisitor)
    }
}

struct AcknowledgementStringVisitor;

impl<'de> Visitor<'de> for AcknowledgementStringVisitor {
    type Value = AcknowledgementString;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any acknowledgement field value")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(AcknowledgementString(Some(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(AcknowledgementString(Some(value)))
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(AcknowledgementString(None))
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(AcknowledgementString(None))
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(AcknowledgementString(None))
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(AcknowledgementString(None))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(AcknowledgementString(None))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(AcknowledgementString(None))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence.next_element::<IgnoredAny>()?.is_some() {}
        Ok(AcknowledgementString(None))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(AcknowledgementString(None))
    }
}

impl<'de> Deserialize<'de> for Acknowledgement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(AcknowledgementVisitor)
    }
}

struct AcknowledgementVisitor;

impl<'de> Visitor<'de> for AcknowledgementVisitor {
    type Value = Acknowledgement;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an acknowledgement object with unique fields")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut acknowledgement = Acknowledgement::default();
        let mut fields = BTreeSet::new();
        while let Some(field) = map.next_key::<String>()? {
            if !fields.insert(field.clone()) {
                return Err(A::Error::custom(format!("duplicate field `{field}`")));
            }
            match field.as_str() {
                "commandId" => acknowledgement.command_id = map.next_value()?,
                "operationId" => acknowledgement.operation_id = map.next_value()?,
                "outcome" => acknowledgement.outcome = map.next_value()?,
                "reason" => acknowledgement.reason = map.next_value()?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(acknowledgement)
    }
}

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
    acknowledgements: &[Acknowledgement],
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
        let Some(identifier) = acknowledgement.identifier(id_field) else {
            return invalid_set(field);
        };
        let Some(outcome) = acknowledgement.outcome() else {
            return invalid_set(field);
        };
        if acknowledgement.reason.as_deref().is_none()
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
