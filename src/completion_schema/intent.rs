//! Concrete admission for the opt-in intent contract. Legacy requests keep their decoder.
use super::*;
use crate::strict_json::shape::Variant;

const PHASE: Shape = Shape::StringEnum(&["focus", "short_break", "long_break"]);
pub(crate) const SELECTION: Shape = Shape::Record(&[
    Field::required("phase", PHASE),
    Field::required("generation", Shape::String),
    Field::required("explicit", Shape::Boolean),
]);

const ACTIONS: &[Variant] = &[
    Variant {
        name: "start",
        fields: &[],
    },
    Variant {
        name: "pause",
        fields: &[],
    },
    Variant {
        name: "resume",
        fields: &[],
    },
    Variant {
        name: "cancel",
        fields: &[],
    },
    Variant {
        name: "cancelAndClear",
        fields: &[],
    },
    Variant {
        name: "clear",
        fields: &[],
    },
    Variant {
        name: "restart",
        fields: &[],
    },
    Variant {
        name: "selectPhase",
        fields: &[Field::required("phase", PHASE)],
    },
    Variant {
        name: "skip",
        fields: &[],
    },
    Variant {
        name: "upsertTask",
        fields: &[Field::required("title", Shape::String)],
    },
    Variant {
        name: "addAndSelectTask",
        fields: &[Field::required("title", Shape::String)],
    },
    Variant {
        name: "deleteTask",
        fields: &[Field::required("taskId", Shape::String)],
    },
    Variant {
        name: "selectTask",
        fields: &[Field::required("taskId", Shape::Nullable(&Shape::String))],
    },
    Variant {
        name: "setDuration",
        fields: &[
            Field::required("phase", PHASE),
            Field::required("minutes", Shape::Integer),
        ],
    },
    Variant {
        name: "changeDuration",
        fields: &[
            Field::required("phase", PHASE),
            Field::required("delta", Shape::Integer),
        ],
    },
    Variant {
        name: "setAutoStart",
        fields: &[Field::required("enabled", Shape::Boolean)],
    },
];

pub(crate) const REQUEST: Shape = Shape::Record(&[
    Field::required("compatibility", Shape::StringEnum(&["pwaStorage"])),
    Field::required("replicationMode", Shape::StringEnum(&["centralized"])),
    Field::required(
        "intent",
        Shape::TaggedObject {
            tag: "kind",
            variants: ACTIONS,
        },
    ),
    Field::required("workspace", WORKSPACE),
    Field::optional("requestedTimer", Shape::Nullable(&TIMER)),
    Field::required("selection", SELECTION),
    Field::required("allocation", ALLOCATION),
    Field::required("observation", OBSERVATION),
    Field::required("clock", CLOCK),
    Field::required("identities", IDENTITIES),
    Field::required("calendarIntervals", Shape::Array(&INTERVAL)),
    Field::optional("ownership", Shape::Nullable(&Shape::Object)),
    Field::optional("durability", Shape::Nullable(&Shape::Object)),
    Field::optional("localDurationsMs", Shape::Nullable(&Shape::Object)),
    Field::optional("knownTasks", Shape::Nullable(&Shape::Array(&Shape::Object))),
    Field::required("lifecycle", LIFECYCLE),
]);

#[cfg(test)]
mod tests;
