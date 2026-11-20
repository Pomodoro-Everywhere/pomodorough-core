use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::strict_json::shape::{Field, Shape};

pub(super) const ALLOCATION_OBJECTS: Shape =
    Shape::Fields(&[Field::optional("hlc", Shape::Object)]);
pub(super) const OBSERVATION_OBJECTS: Shape = Shape::Fields(&[
    Field::optional("commandTimes", Shape::Object),
    Field::optional("monotonicAnchor", Shape::Nullable(&Shape::Object)),
]);
/// Object positions of a request without the opt-in lifecycle, which Serde
/// would otherwise read from a positional JSON array. The lifecycle request
/// is admitted by `completion_schema::intent::REQUEST` instead. `workspace`
/// keeps its own downstream checks and their shipped messages.
pub(super) const LEGACY_OBJECTS: Shape = Shape::Fields(&[
    Field::optional("intent", Shape::Object),
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
    Field::optional("durability", Shape::Nullable(&Shape::Object)),
    Field::optional("localDurationsMs", Shape::Nullable(&Shape::Object)),
    Field::optional("knownTasks", Shape::Nullable(&Shape::Array(&Shape::Object))),
]);

/// Array positions Serde would otherwise refuse with its own text. They are
/// checked after the object positions above, so every message those checks
/// return keeps its precedence. Elements keep their typed decoder messages.
pub(super) const IDENTITY_ARRAYS: Shape =
    Shape::Fields(&[Field::optional("commandUuids", Shape::Array(&Shape::Any))]);
const DURABILITY_ARRAYS: Shape = Shape::Fields(&[Field::optional(
    "outgoingDurationOperationIds",
    Shape::Array(&Shape::Any),
)]);
pub(super) const INTENT_ARRAYS: Shape = Shape::Fields(&[
    Field::optional("identities", IDENTITY_ARRAYS),
    Field::optional("durability", Shape::Nullable(&DURABILITY_ARRAYS)),
]);

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) enum Compatibility {
    AppleWorkspace,
    AndroidCoordinator,
    DesktopStorage,
    DesktopTerminal,
    PwaStorage,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) enum ReplicationMode {
    Centralized,
    Iroh,
}

#[derive(Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum Intent {
    Start,
    Pause,
    Resume,
    Cancel,
    CancelAndClear,
    Clear,
    Restart,
    SelectPhase { phase: Phase },
    Skip,
    UpsertTask { title: String },
    AddAndSelectTask { title: String },
    DeleteTask { task_id: String },
    RenameTask { task_id: String, title: String },
    SelectTask { task_id: RequiredNullable<String> },
    SetDuration { phase: Phase, minutes: i64 },
    ChangeDuration { phase: Phase, delta: i64 },
    SetAutoStart { enabled: bool },
}

impl Intent {
    pub(super) fn is_workspace_mutation(&self) -> bool {
        matches!(
            self,
            Self::UpsertTask { .. }
                | Self::AddAndSelectTask { .. }
                | Self::DeleteTask { .. }
                | Self::RenameTask { .. }
                | Self::SelectTask { .. }
                | Self::SetDuration { .. }
                | Self::ChangeDuration { .. }
                | Self::SetAutoStart { .. }
        )
    }
}

#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub(super) struct RequiredNullable<T>(pub Option<T>);

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub(super) enum Phase {
    #[serde(rename = "focus")]
    Focus,
    #[serde(rename = "short_break")]
    ShortBreak,
    #[serde(rename = "long_break")]
    LongBreak,
}

impl Phase {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::ShortBreak => "short_break",
            Self::LongBreak => "long_break",
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Selection {
    pub phase: Phase,
    pub generation: String,
    pub explicit: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Allocation {
    pub device_id: String,
    pub device_sequence: i64,
    pub hlc: Hlc,
    pub last_uuid: Option<String>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Hlc {
    pub wall_ms: i64,
    pub counter: i64,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Observation {
    pub canonical_anchor_at: Option<String>,
    pub command_times: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monotonic_anchor: Option<MonotonicAnchor>,
    #[serde(skip)]
    pub running_timer: Option<crate::timer::workspace::retarget_observation::RunningTimer>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct MonotonicAnchor {
    pub timer_id: String,
    pub anchor_at: String,
    pub elapsed_at_anchor_ms: i64,
    pub sampled_trusted_now_ms: i64,
    pub sampled_monotonic_ms: f64,
    pub continuity_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Clock {
    pub occurred_at: String,
    pub physical_now: String,
    pub observed_at: String,
    #[serde(default)]
    pub monotonic_now_ms: Option<f64>,
    #[serde(default)]
    pub continuity_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Identities {
    pub command_uuids: Vec<String>,
    pub timer_uuid: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Input {
    pub compatibility: Compatibility,
    pub replication_mode: ReplicationMode,
    pub intent: Intent,
    pub workspace: Value,
    pub requested_timer: Option<crate::timer::CanonicalTimer>,
    pub selection: Selection,
    pub allocation: Allocation,
    pub observation: Observation,
    pub clock: Clock,
    pub identities: Identities,
    pub calendar_intervals: Vec<Interval>,
    pub ownership: Option<Ownership>,
    pub durability: Option<Durability>,
    pub local_durations_ms: Option<BTreeMap<String, i64>>,
    pub known_tasks: Option<Vec<KnownTask>>,
    #[serde(skip)]
    pub lifecycle: Option<super::completion_mutation::CompletionLifecycle>,
}

#[derive(Deserialize)]
pub(super) struct KnownTask {
    pub id: String,
    pub title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Ownership {
    pub expected_owner_id: RequiredNullable<String>,
    pub owner_id: RequiredNullable<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Durability {
    pub outgoing_duration_operation_ids: Vec<String>,
    pub local_tab_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Interval {
    pub start: String,
    pub end: String,
}

#[derive(Clone, Copy)]
pub(super) enum CommandKind {
    Start,
    Finish,
    Pause,
    Resume,
    Cancel,
    Clear,
    Retarget,
}

impl CommandKind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Finish => "finish",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Cancel => "cancel",
            Self::Clear => "clear",
            Self::Retarget => "retarget",
        }
    }
}
