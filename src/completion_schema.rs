//! Shared representation admission. Typed decoders keep field and scalar semantics.
use crate::strict_json::shape::{Field, Shape};

pub(crate) mod evidence;
pub(crate) mod intent;

const fn scalar(name: &'static str) -> Field {
    Field::optional(name, Shape::Scalar)
}

pub(crate) const SELECTION: Shape = Shape::Fields(&[
    Field::optional("phase", Shape::String),
    scalar("generation"),
    scalar("explicit"),
]);
pub(crate) const INTENT: Shape = Shape::Fields(&[
    Field::optional("type", Shape::String),
    scalar("commandId"),
    scalar("occurredAt"),
]);
pub(crate) const TIMER: Shape = Shape::Fields(&[
    scalar("id"),
    scalar("taskId"),
    scalar("phase"),
    scalar("status"),
    scalar("plannedDurationMs"),
    scalar("elapsedAtAnchorMs"),
    scalar("anchorAt"),
    scalar("startedByDeviceId"),
    Field::optional("lastIntent", Shape::Nullable(&INTENT)),
]);
pub(crate) const HISTORY: Shape = Shape::Fields(&[
    scalar("id"),
    scalar("timerId"),
    scalar("taskId"),
    scalar("commandId"),
    scalar("phase"),
    scalar("status"),
    scalar("plannedDurationMs"),
    scalar("completedAt"),
    scalar("endedAt"),
]);
pub(crate) const SENT_COMMAND: Shape = Shape::Fields(&[
    scalar("id"),
    scalar("timerId"),
    Field::optional("type", Shape::String),
    scalar("phase"),
    scalar("deviceSequence"),
    scalar("occurredAt"),
    scalar("physicalOccurredAt"),
]);
const COMMAND: Shape = Shape::Fields(&[
    scalar("id"),
    scalar("timerId"),
    scalar("deviceId"),
    scalar("deviceSequence"),
    scalar("taskId"),
    Field::optional("type", Shape::String),
    Field::optional("phase", Shape::String),
    scalar("plannedDurationMs"),
    scalar("observedElapsedMs"),
    scalar("occurredAt"),
    scalar("hlcWallMs"),
    scalar("hlcCounter"),
]);
const DURATION: Shape = Shape::Fields(&[Field::optional("phase", Shape::String)]);
pub(crate) const QUEUES: Shape = Shape::Fields(&[
    Field::optional("commands", Shape::Array(&COMMAND)),
    Field::optional("taskOperations", Shape::Array(&Shape::Object)),
    Field::optional("durationOperations", Shape::Array(&DURATION)),
    Field::optional("autoStartOperations", Shape::Array(&Shape::Object)),
    Field::optional("selectedTaskOperations", Shape::Array(&Shape::Object)),
]);
const BASE: Shape = Shape::Fields(&[
    Field::optional("canonicalTimer", Shape::Nullable(&TIMER)),
    Field::optional("history", Shape::Array(&HISTORY)),
    Field::optional("tasks", Shape::Array(&Shape::Object)),
    Field::optional("durationsMs", Shape::Object),
    scalar("autoStartBreaks"),
    scalar("selectedTaskId"),
]);
pub(crate) const HLC: Shape = Shape::Fields(&[scalar("wallMs"), scalar("counter")]);
const DISPLAY: Shape = Shape::Fields(&[
    Field::optional("profile", Shape::String),
    Field::optional("projectionPending", Shape::Nullable(&QUEUES)),
]);
pub(crate) const WORKSPACE: Shape = Shape::Fields(&[
    Field::optional("base", BASE),
    Field::optional("local", QUEUES),
    Field::optional("canonicalHead", Shape::Nullable(&HLC)),
    Field::optional("neverSent", Shape::Object),
    Field::optional("timerDependencies", Shape::Array(&Shape::Object)),
    Field::optional("displayContext", Shape::Nullable(&DISPLAY)),
]);
pub(crate) const ALLOCATION: Shape = Shape::Fields(&[
    scalar("deviceId"),
    scalar("deviceSequence"),
    Field::optional("hlc", HLC),
    scalar("lastUuid"),
]);
pub(crate) const ANCHOR: Shape = Shape::Fields(&[
    scalar("timerId"),
    scalar("anchorAt"),
    scalar("elapsedAtAnchorMs"),
    scalar("sampledTrustedNowMs"),
    scalar("sampledMonotonicMs"),
    scalar("continuityId"),
]);
pub(crate) const OBSERVATION: Shape = Shape::Fields(&[
    scalar("canonicalAnchorAt"),
    Field::optional("commandTimes", Shape::Object),
    Field::optional("monotonicAnchor", Shape::Nullable(&ANCHOR)),
]);
pub(crate) const CLOCK: Shape = Shape::Fields(&[
    scalar("occurredAt"),
    scalar("physicalNow"),
    scalar("observedAt"),
    scalar("monotonicNowMs"),
    scalar("continuityId"),
]);
pub(crate) const IDENTITIES: Shape = Shape::Fields(&[
    Field::optional("commandUuids", Shape::Array(&Shape::Scalar)),
    scalar("timerUuid"),
]);
pub(crate) const INTERVAL: Shape = Shape::Fields(&[scalar("start"), scalar("end")]);
pub(crate) const OWNER: Shape = Shape::Fields(&[
    scalar("timerId"),
    scalar("deviceId"),
    scalar("tabId"),
    scalar("leaseExpiresAtMs"),
]);
pub(crate) const COMPLETION: Shape =
    Shape::Fields(&[scalar("timerId"), scalar("commandId"), scalar("phase")]);
pub(crate) const TRIGGER: Shape = Shape::Fields(&[
    scalar("finishCommandId"),
    scalar("timerId"),
    scalar("finishDeviceSequence"),
    scalar("reservedTimerUuid"),
]);
pub(crate) const LIFECYCLE: Shape = Shape::Fields(&[
    Field::optional("consumedCompletions", Shape::Array(&COMPLETION)),
    Field::optional("pendingBreaks", Shape::Array(&TRIGGER)),
    Field::optional("finishEvidence", Shape::Array(&evidence::FINISH)),
]);
pub(crate) const BOUNDARY_RETRY: Shape =
    Shape::Fields(&[scalar("originalObservedAt"), scalar("measuredElapsedMs")]);
pub(crate) const FINISH: Shape = Shape::Fields(&[
    Field::optional("stage", Shape::String),
    Field::optional("compatibility", Shape::String),
    Field::optional("replicationMode", Shape::String),
    Field::optional("workspace", WORKSPACE),
    Field::optional("requestedTimer", TIMER),
    Field::optional("selection", SELECTION),
    Field::optional("allocation", ALLOCATION),
    Field::optional("observation", OBSERVATION),
    Field::optional("clock", CLOCK),
    Field::optional("identities", IDENTITIES),
    Field::optional("calendarIntervals", Shape::Array(&INTERVAL)),
    Field::optional("ownership", Shape::Nullable(&OWNER)),
    scalar("localTabId"),
    scalar("leaseNowMs"),
    scalar("leaseDurationMs"),
    Field::optional("boundaryRetry", Shape::Nullable(&BOUNDARY_RETRY)),
    Field::optional("lifecycle", LIFECYCLE),
]);
pub(crate) const SOURCE: Shape = Shape::Fields(&[
    Field::optional("kind", Shape::String),
    Field::optional("value", WORKSPACE),
]);
pub(crate) const MONOTONIC: Shape = Shape::Fields(&[
    scalar("nowMs"),
    scalar("continuityId"),
    Field::optional("anchor", ANCHOR),
]);
pub(crate) const READ: Shape = Shape::Fields(&[
    Field::optional("profile", Shape::String),
    Field::optional("source", SOURCE),
    scalar("selectedPhase"),
    scalar("observedAt"),
    Field::optional("calendarIntervals", Shape::Array(&INTERVAL)),
    Field::optional("monotonic", Shape::Nullable(&MONOTONIC)),
    Field::optional("selection", SELECTION),
    Field::optional("lifecycle", LIFECYCLE),
]);
pub(crate) const PENDING: Shape = Shape::Fields(&[
    Field::optional("commandIds", Shape::Array(&Shape::Scalar)),
    Field::optional("sendableCommandIds", Shape::Array(&Shape::Scalar)),
    Field::optional("otherOperationIds", Shape::Array(&Shape::Scalar)),
]);
pub(crate) const ACK: Shape = Shape::Fields(&[
    scalar("commandId"),
    Field::optional("outcome", Shape::String),
]);
pub(crate) const ADVANCE: Shape = Shape::Fields(&[
    scalar("commandId"),
    scalar("timerId"),
    scalar("previousPhase"),
    scalar("advancedPhase"),
    scalar("generation"),
]);
pub(crate) const SENT: Shape = Shape::Fields(&[
    Field::optional("kind", Shape::String),
    Field::optional("commands", Shape::Array(&SENT_COMMAND)),
    Field::optional("rollbackHistory", Shape::Array(&HISTORY)),
    Field::optional("selectionAtSend", Shape::Nullable(&SELECTION)),
    Field::optional("acknowledgementHistory", Shape::Array(&HISTORY)),
    Field::optional("acknowledgementTimer", Shape::Nullable(&TIMER)),
    Field::optional("nextProjectionTimer", Shape::Nullable(&TIMER)),
]);
pub(crate) const INSTALL: Shape = Shape::Fields(&[
    Field::optional("compatibility", Shape::String),
    Field::optional("beforeHistory", Shape::Array(&HISTORY)),
    Field::optional("afterHistory", Shape::Array(&HISTORY)),
    Field::optional("canonicalTimer", Shape::Nullable(&TIMER)),
    Field::optional("selection", SELECTION),
    Field::optional("pending", PENDING),
    Field::optional("advances", Shape::Array(&ADVANCE)),
    Field::optional("acknowledgements", Shape::Array(&ACK)),
    Field::optional("discardedCommandIds", Shape::Array(&Shape::Scalar)),
    scalar("referenceTime"),
    Field::optional("calendarIntervals", Shape::Array(&INTERVAL)),
    Field::optional("sentContext", Shape::Nullable(&SENT)),
    Field::optional("lifecycle", LIFECYCLE),
]);

#[cfg(test)]
pub(crate) mod tests;
