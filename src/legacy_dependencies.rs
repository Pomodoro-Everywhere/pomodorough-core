//! Bounded reconstruction of local dependency metadata. Retained wire records stay intact.
use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::CoreError;

mod evidence;
mod graph;
mod provenance;
mod schema;
mod validation;

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Profile {
    PwaStorage,
    AndroidCentralized,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    profile: Profile,
    workspace: Value,
    ownership: Value,
    device_id: String,
    outgoing: Value,
    calendar_intervals: Vec<Interval>,
    source_acknowledgements: Vec<Acknowledgement>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Interval {
    start: String,
    end: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Acknowledgement {
    command_id: String,
    outcome: String,
    #[serde(default)]
    reason: Option<String>,
}

struct Plan {
    dependencies: Vec<Value>,
    classifications: Vec<Value>,
    unresolved: Vec<Value>,
}

fn invalid(reason: &str) -> CoreError {
    CoreError::InvalidInput(reason.into())
}

pub(crate) fn plan_json(raw: &str) -> Result<String, CoreError> {
    let raw = crate::legacy_preferences::json::parse(raw)?;
    crate::strict_json::shape::validate(&raw, &schema::REQUEST, "legacy dependency plan")?;
    schema::records(&raw)?;
    let input: Input = serde_json::from_value(raw)?;
    let projection = validation::request(&input)?;
    let commands: BTreeMap<_, _> = input.workspace["local"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["id"].as_str().unwrap(), row))
        .collect();
    let mut plan = graph::plan(&input, &commands, &projection)?;
    if let Some(reason) = evidence::body_issue(&input)? {
        plan.unresolved
            .push(json!({"reason": reason, "outgoing": input.outgoing}));
    }
    output(&input, plan)
}

fn output(input: &Input, plan: Plan) -> Result<String, CoreError> {
    let blocked = !plan.unresolved.is_empty();
    let dependencies = Value::Array(plan.dependencies);
    let changed = input.workspace["timerDependencies"] != dependencies;
    let mut workspace = input.workspace.clone();
    let mut writes = vec![];
    if !blocked {
        workspace["timerDependencies"] = dependencies.clone();
        validation::project(&workspace, input)?;
        if changed {
            writes.push(json!({"kind": "recordTimerDependencies", "value": dependencies}));
        }
    }
    Ok(json!({"schemaVersion": 1,
        "outcome": if blocked {"blocked"} else if changed {"planned"} else {"noop"},
        "workspace": workspace, "ownership": input.ownership, "outgoing": input.outgoing,
        "outgoingAction": "preserve", "wireAction": "preserve", "metadataWrites": writes,
        "timerDependencies": if blocked {Value::Null} else {dependencies.clone()},
        "validatedDependencies": dependencies, "classifications": plan.classifications,
        "recovery": {"status": if blocked {"blocked"} else {"ready"}, "blocksSync": blocked,
            "blocksMutations": blocked, "automaticRepair": false, "unresolved": plan.unresolved},
        "effectsAfterCommit": []})
    .to_string())
}
