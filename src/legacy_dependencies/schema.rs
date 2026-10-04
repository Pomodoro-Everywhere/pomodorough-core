use serde_json::Value;

use crate::CoreError;
use crate::strict_json::shape::{Field, Shape, validate};

const fn optional(name: &'static str) -> Field {
    Field::optional(name, Shape::Scalar)
}

const COMMAND: Shape = Shape::Fields(&[
    optional("id"),
    optional("deviceId"),
    optional("deviceSequence"),
    optional("timerId"),
    optional("taskId"),
    Field::optional("type", Shape::String),
    Field::optional("phase", Shape::String),
    optional("plannedDurationMs"),
    optional("occurredAt"),
    optional("physicalOccurredAt"),
    optional("hlcWallMs"),
    optional("hlcCounter"),
    optional("observedElapsedMs"),
    Field::optional("dependsOnCommandId", Shape::Nullable(&Shape::String)),
    Field::optional(
        "generatedByFinishCommandId",
        Shape::Nullable(&Shape::String),
    ),
    Field::optional("generatedBreak", Shape::Boolean),
    Field::optional("neverSent", Shape::Boolean),
    Field::optional("sourceDayStart", Shape::String),
    Field::optional("sourceDayEnd", Shape::String),
]);
const OPERATION: Shape = Shape::Fields(&[
    optional("id"),
    optional("deviceId"),
    optional("occurredAt"),
    optional("hlcWallMs"),
    optional("hlcCounter"),
    optional("taskId"),
    optional("title"),
    optional("durationMs"),
    optional("enabled"),
    Field::optional("type", Shape::String),
    Field::optional("phase", Shape::String),
]);
const QUEUES: Shape = Shape::Fields(&[
    Field::optional("commands", Shape::Array(&COMMAND)),
    Field::optional("taskOperations", Shape::Array(&OPERATION)),
    Field::optional("durationOperations", Shape::Array(&OPERATION)),
    Field::optional("autoStartOperations", Shape::Array(&OPERATION)),
    Field::optional("selectedTaskOperations", Shape::Array(&OPERATION)),
]);
const DEPENDENCY: Shape = Shape::Fields(&[
    Field::required("operationId", Shape::String),
    Field::required("dependsOnOperationId", Shape::String),
    Field::optional("generatedBreak", Shape::Boolean),
    Field::optional("sourceDayStart", Shape::String),
    Field::optional("sourceDayEnd", Shape::String),
]);
const OWNER: Shape = Shape::Fields(&[
    Field::required("timerId", Shape::String),
    Field::required("deviceId", Shape::String),
    Field::optional("tabId", Shape::Nullable(&Shape::String)),
    Field::optional("leaseExpiresAtMs", Shape::Nullable(&Shape::Integer)),
]);
const OWNERSHIP: Shape = Shape::Record(&[
    Field::required("ownerId", Shape::Nullable(&Shape::String)),
    Field::required("expectedOwnerId", Shape::Nullable(&Shape::String)),
    Field::required("timerOwner", Shape::Nullable(&OWNER)),
]);
const ACK: Shape = Shape::Record(&[
    Field::required("commandId", Shape::String),
    Field::required(
        "outcome",
        Shape::StringEnum(&["applied", "ignored", "rejected"]),
    ),
    Field::optional("reason", Shape::Nullable(&Shape::String)),
]);
const INTERVAL: Shape = Shape::Record(&[
    Field::required("start", Shape::String),
    Field::required("end", Shape::String),
]);
pub(super) const REQUEST: Shape = Shape::Record(&[
    Field::required(
        "profile",
        Shape::StringEnum(&["pwaStorage", "androidCentralized"]),
    ),
    Field::required("workspace", Shape::Object),
    Field::required("ownership", OWNERSHIP),
    Field::required("deviceId", Shape::String),
    Field::required("outgoing", Shape::Nullable(&Shape::Object)),
    Field::required("calendarIntervals", Shape::Array(&INTERVAL)),
    Field::required("sourceAcknowledgements", Shape::Array(&ACK)),
]);

pub(super) fn records(raw: &Value) -> Result<(), CoreError> {
    let workspace = crate::strict_json::object(&raw["workspace"], "workspace")?;
    for field in [
        "timerDependencies",
        "neverSent",
        "canonicalHead",
        "displayContext",
        "now",
    ] {
        if !workspace.contains_key(field) {
            return Err(super::invalid(
                "legacy dependency plan requires complete raw workspace",
            ));
        }
    }
    let mut admitted = raw["workspace"].clone();
    admitted["timerDependencies"] = serde_json::json!([]);
    crate::reconciliation::workspace::validate_shape(&admitted)?;
    validate(&admitted, &crate::completion_schema::WORKSPACE, "workspace")?;
    validate(&raw["workspace"]["local"], &QUEUES, "local")?;
    let stored = &raw["workspace"]["displayContext"]["projectionPending"];
    if !stored.is_null() {
        validate(stored, &QUEUES, "projectionPending")?;
    }
    validate(
        &raw["workspace"]["timerDependencies"],
        &Shape::Nullable(&Shape::Array(&DEPENDENCY)),
        "timerDependencies",
    )?;
    if !raw["outgoing"].is_null() {
        for field in ["sent", "payload"] {
            if let Some(value) = raw["outgoing"].get(field) {
                claim(value)?;
            }
        }
        if let Some(body) = raw["outgoing"].get("body") {
            let body = body
                .as_str()
                .ok_or_else(|| super::invalid("outgoing body must be a string"))?;
            claim(&crate::legacy_preferences::json::parse(body)?)?;
        }
    }
    Ok(())
}

pub(super) fn claim(value: &Value) -> Result<(), CoreError> {
    validate(value, &QUEUES, "outgoing queues")
}

#[cfg(test)]
mod tests;
