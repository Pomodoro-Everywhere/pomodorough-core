use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::strict_json::shape::{Field, Shape};
use crate::workspace_intent::model::{
    ALLOCATION_OBJECTS, Allocation, Clock, Compatibility, Identities, Input, Intent, Interval,
    OBSERVATION_OBJECTS, Observation, ReplicationMode, Selection,
};

#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct State {
    #[serde(default)]
    pub(super) consumed_completions: Vec<Completion>,
    #[serde(default)]
    pub(super) pending_breaks: Vec<Trigger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) finish_evidence: Option<Vec<super::evidence::FinishEvidence>>,
}

impl State {
    pub(crate) fn has_finish_evidence(&self) -> bool {
        self.finish_evidence.is_some()
    }

    pub(crate) fn natural_consumed(&self, timer_id: &str, phase: &str) -> bool {
        self.consumed_completions.iter().any(|item| {
            item.timer_id == timer_id && item.phase == phase && item.command_id.is_none()
        })
    }

    pub(crate) fn consumed(&self, timer_id: &str, phase: &str) -> bool {
        self.consumed_completions
            .iter()
            .any(|item| item.timer_id == timer_id && item.phase == phase)
    }

    pub(crate) fn finished(&self, timer_id: &str, phase: &str) -> bool {
        self.consumed_completions.iter().any(|item| {
            item.timer_id == timer_id && item.phase == phase && item.command_id.is_some()
        })
    }

    pub(crate) fn remember(&mut self, row: &crate::timer::HistoryItem) {
        let completion = Completion {
            timer_id: row.timer_id.clone(),
            command_id: row.command_id.clone(),
            phase: row.phase.clone(),
        };
        if !self.consumed_completions.contains(&completion) {
            self.consumed_completions.push(completion);
        }
    }
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Completion {
    pub timer_id: String,
    pub command_id: Option<String>,
    pub phase: String,
}

impl Completion {
    pub fn from_row(row: &Value) -> Self {
        Self {
            timer_id: row["timerId"].as_str().unwrap().into(),
            command_id: row["commandId"].as_str().map(str::to_owned),
            phase: row["phase"].as_str().unwrap().into(),
        }
    }
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Trigger {
    pub finish_command_id: String,
    pub timer_id: String,
    pub finish_device_sequence: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved_timer_uuid: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Stage {
    ExpiryObservation,
    DeferredBreakOpportunity,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum Event {
    Observation {},
    Opportunity {},
    CanonicalInstalled {
        acknowledgements: Vec<Acknowledgement>,
        discarded_command_ids: Vec<String>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Acknowledgement {
    pub command_id: String,
    pub outcome: Outcome,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Outcome {
    Applied,
    Ignored,
    Rejected,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Session {
    // An absent identity must not silently remove the canonical barrier.
    #[serde(deserialize_with = "Option::<String>::deserialize")]
    pub user_id: Option<String>,
    pub authenticated: bool,
}

const FINISH_EVIDENCE_OBJECTS: Shape = Shape::Fields(&[
    Field::optional("command", Shape::Object),
    Field::optional("sourceTimer", crate::timer::TIMER_OBJECTS),
    Field::optional("sourceHistory", Shape::Object),
]);
const STATE_OBJECTS: Shape = Shape::Fields(&[
    Field::optional("consumedCompletions", Shape::Array(&Shape::Object)),
    Field::optional("pendingBreaks", Shape::Array(&Shape::Object)),
    Field::optional(
        "finishEvidence",
        Shape::Nullable(&Shape::Array(&FINISH_EVIDENCE_OBJECTS)),
    ),
]);
/// Object positions of a lifecycle-stage request, which Serde would otherwise
/// read from a positional JSON array. `workspace` and `previousWorkspace` keep
/// their own downstream checks and their shipped messages.
pub(super) const REQUEST_OBJECTS: Shape = Shape::Fields(&[
    Field::optional("previousObservation", Shape::Nullable(&OBSERVATION_OBJECTS)),
    Field::optional(
        "requestedTimer",
        Shape::Nullable(&crate::timer::TIMER_OBJECTS),
    ),
    Field::optional("selection", Shape::Object),
    Field::optional("allocation", ALLOCATION_OBJECTS),
    Field::optional("observation", OBSERVATION_OBJECTS),
    Field::optional("clock", Shape::Object),
    Field::optional("identities", Shape::Object),
    Field::optional("calendarIntervals", Shape::Array(&Shape::Object)),
    Field::optional("ownership", Shape::Nullable(&Shape::Object)),
    Field::optional("lifecycle", STATE_OBJECTS),
    Field::optional(
        "event",
        Shape::Fields(&[Field::optional(
            "acknowledgements",
            Shape::Array(&Shape::Object),
        )]),
    ),
    Field::optional("centralizedSession", Shape::Object),
]);

/// Array positions of a lifecycle-stage request that Serde would otherwise
/// refuse with its own text. Checked after `REQUEST_OBJECTS`, so its messages
/// keep their precedence.
pub(super) const REQUEST_ARRAYS: Shape = Shape::Fields(&[Field::optional(
    "identities",
    crate::workspace_intent::model::IDENTITY_ARRAYS,
)]);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    pub stage: Stage,
    pub compatibility: Compatibility,
    pub replication_mode: ReplicationMode,
    pub workspace: Value,
    #[serde(default)]
    pub previous_workspace: Option<Value>,
    #[serde(default)]
    pub previous_observation: Option<Observation>,
    #[serde(default)]
    pub requested_timer: Option<crate::timer::CanonicalTimer>,
    pub selection: Selection,
    pub allocation: Allocation,
    pub observation: Observation,
    pub clock: Clock,
    pub identities: Identities,
    pub calendar_intervals: Vec<Interval>,
    pub ownership: Option<super::super::Ownership>,
    pub lifecycle: State,
    pub event: Event,
    pub centralized_session: Session,
}

impl Request {
    pub fn context(&self) -> Input {
        Input {
            compatibility: self.compatibility,
            replication_mode: if self.replication_mode == ReplicationMode::Iroh {
                ReplicationMode::Iroh
            } else {
                ReplicationMode::Centralized
            },
            intent: Intent::Pause,
            workspace: self.workspace.clone(),
            requested_timer: self.requested_timer.clone(),
            selection: self.selection.clone(),
            allocation: self.allocation.clone(),
            observation: self.observation.clone(),
            clock: Clock {
                occurred_at: self.clock.occurred_at.clone(),
                physical_now: self.clock.physical_now.clone(),
                observed_at: self.clock.observed_at.clone(),
                monotonic_now_ms: self.clock.monotonic_now_ms,
                continuity_id: self.clock.continuity_id.clone(),
            },
            identities: Identities {
                command_uuids: self.identities.command_uuids.clone(),
                timer_uuid: self.identities.timer_uuid.clone(),
            },
            calendar_intervals: self
                .calendar_intervals
                .iter()
                .map(|day| Interval {
                    start: day.start.clone(),
                    end: day.end.clone(),
                })
                .collect(),
            ownership: None,
            durability: None,
            local_durations_ms: None,
            known_tasks: None,
            lifecycle: None,
        }
    }

    pub fn canonical_barrier(&self) -> bool {
        self.replication_mode == ReplicationMode::Centralized
            && self.centralized_session.user_id.is_some()
            && self.centralized_session.authenticated
    }
}

#[cfg(test)]
mod shape_guards {
    use crate::completion_schema::{self as schema, tests::assert_fields};

    #[test]
    fn lifecycle_decoder_fields_require_shared_shape_guards() {
        assert_fields::<super::State>(&schema::LIFECYCLE, &[]);
        assert_fields::<super::Completion>(&schema::COMPLETION, &[]);
        assert_fields::<super::Trigger>(&schema::TRIGGER, &[]);
    }
}
