use crate::strict_json::shape::{Field, Shape, Variant};

const OWNER: &[Field] = &[
    Field::required("timerId", Shape::String),
    Field::required("deviceId", Shape::String),
    Field::optional("tabId", Shape::Nullable(&Shape::String)),
    Field::optional("leaseExpiresAtMs", Shape::Nullable(&Shape::Integer)),
];

const CLOCK: &[Field] = &[
    Field::required("nowMs", Shape::Integer),
    Field::required("leaseDurationMs", Shape::Integer),
];

const ACTIONS: &[Variant] = &[
    Variant {
        name: "install",
        fields: &[],
    },
    Variant {
        name: "renew",
        fields: &[Field::required("timerId", Shape::String)],
    },
];

const QUEUES: &[Field] = &[
    Field::required("commands", Shape::Array(&Shape::Object)),
    Field::required("taskOperations", Shape::Array(&Shape::Object)),
    Field::required("durationOperations", Shape::Array(&Shape::Object)),
    Field::required("autoStartOperations", Shape::Array(&Shape::Object)),
    Field::required("selectedTaskOperations", Shape::Array(&Shape::Object)),
];

const PROOF: &[Field] = &[
    Field::optional("commands", Shape::Array(&Shape::String)),
    Field::optional("taskOperations", Shape::Array(&Shape::String)),
    Field::optional("durationOperations", Shape::Array(&Shape::String)),
    Field::optional("autoStartOperations", Shape::Array(&Shape::String)),
    Field::optional("selectedTaskOperations", Shape::Array(&Shape::String)),
];

const BASE: &[Field] = &[
    Field::required("canonicalTimer", Shape::Nullable(&Shape::Object)),
    Field::required("history", Shape::Array(&Shape::Object)),
    Field::required("tasks", Shape::Array(&Shape::Object)),
    Field::required("durationsMs", Shape::Object),
    Field::required("autoStartBreaks", Shape::Boolean),
    Field::required("selectedTaskId", Shape::Nullable(&Shape::String)),
];

const HEAD: &[Field] = &[
    Field::required("wallMs", Shape::Integer),
    Field::required("counter", Shape::Integer),
];

const DISPLAY: &[Field] = &[
    Field::required("profile", Shape::StringEnum(&["pwaStorage"])),
    Field::required("projectionPending", Shape::Nullable(&Shape::Record(QUEUES))),
];

const WORKSPACE: &[Field] = &[
    Field::required("base", Shape::Record(BASE)),
    Field::required("local", Shape::Record(QUEUES)),
    Field::required("canonicalHead", Shape::Nullable(&Shape::Record(HEAD))),
    Field::required("neverSent", Shape::Record(PROOF)),
    Field::required("timerDependencies", Shape::Array(&Shape::Object)),
    Field::required("displayContext", Shape::Record(DISPLAY)),
];

pub(super) const REQUEST: Shape = Shape::Record(&[
    Field::required("profile", Shape::StringEnum(&["pwaStorage"])),
    Field::required(
        "action",
        Shape::TaggedObject {
            tag: "kind",
            variants: ACTIONS,
        },
    ),
    Field::required("workspace", Shape::Record(WORKSPACE)),
    Field::required("ownership", Shape::Nullable(&Shape::Record(OWNER))),
    Field::required("localDeviceId", Shape::String),
    Field::required("localTabId", Shape::String),
    Field::required("clock", Shape::Record(CLOCK)),
]);

#[cfg(test)]
mod tests;
