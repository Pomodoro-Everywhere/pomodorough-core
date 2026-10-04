//! Imports raw PWA preferences as legacy zero-clock writes, not new user intents.
use serde::Deserialize;
use serde_json::{Value, json};

use crate::CoreError;

pub(crate) mod json;
mod number;
mod projection;
mod settings;
mod validation;

const DOMAINS: [&str; 5] = [
    "commands",
    "taskOperations",
    "durationOperations",
    "autoStartOperations",
    "selectedTaskOperations",
];
const EPOCH: &str = "1970-01-01T00:00:00.000Z";

#[derive(Deserialize)]
enum Profile {
    #[serde(rename = "pwaStorage")]
    PwaStorage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    profile: Profile,
    workspace: Value,
    settings: Value,
    ownership: Ownership,
    device_id: String,
    identities: Identities,
    outgoing: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ownership {
    owner_id: Value,
    expected_owner_id: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Identities {
    operation_uuids: Vec<String>,
}

fn invalid(reason: &str) -> CoreError {
    CoreError::InvalidInput(reason.into())
}

fn empty_queues() -> Value {
    json!({"commands": [], "taskOperations": [], "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []})
}

pub(crate) fn plan_json(raw: &str) -> Result<String, CoreError> {
    let raw = json::parse(raw)?;
    validation::shape(&raw)?;
    let input: Input = serde_json::from_value(raw)?;
    let mut occupied = validation::request(&input)?;
    let before = projection::project(&input.workspace, &input.device_id)?;
    let migration = settings::migrate(&input.settings)?;
    let mut operations = empty_queues();
    for (index, (domain, mut operation)) in migration.operations.into_iter().enumerate() {
        let id = input
            .identities
            .operation_uuids
            .get(index)
            .ok_or_else(|| invalid("insufficient legacy operation identities"))?;
        if !occupied.insert(id.clone()) {
            return Err(invalid("legacy operation identity already exists"));
        }
        operation["id"] = json!(id);
        operation["occurredAt"] = json!(EPOCH);
        operation["hlcWallMs"] = json!(0);
        operation["hlcCounter"] = json!(0);
        operations[domain].as_array_mut().unwrap().push(operation);
    }
    output(&input, migration.settings, operations, &before)
}

fn output(
    input: &Input,
    settings: Value,
    operations: Value,
    before: &Value,
) -> Result<String, CoreError> {
    let (mut workspace, identities) = append(&input.workspace, &operations);
    let count: usize = DOMAINS
        .iter()
        .map(|domain| identities[*domain].as_array().unwrap().len())
        .sum();
    if count != 0 {
        // Freeze the original display membership before appending imports. An
        // installed head covers zero-clock imports, even when the old context is null.
        workspace["displayContext"] = json!({"profile": "pwaStorage", "projectionPending":
            projection::raw_members(&input.workspace, &before["displayContext"]["projectionPending"])?});
    }
    let projected = projection::project(&workspace, &input.device_id)?;
    if count != 0 {
        workspace["displayContext"]["projectionPending"] = projection::raw_members(
            &workspace,
            &projected["displayContext"]["projectionPending"],
        )?;
    }
    let write_settings = settings != input.settings;
    Ok(json!({"schemaVersion": 1, "outcome": if write_settings || count != 0 {"planned"} else {"noop"},
        "settings": settings, "writeSettings": write_settings, "workspace": workspace,
        "operations": operations, "operationIds": identities, "consumedIdentityCount": count,
        "outgoing": input.outgoing, "outgoingAction": "preserve",
        "projection": projected["workspace"], "effectsAfterCommit": if count != 0 {
            vec![json!({"kind": "launchSync"})] } else {vec![]}}).to_string())
}

fn append(original: &Value, operations: &Value) -> (Value, Value) {
    let mut workspace = original.clone();
    let mut identities = empty_queues();
    for domain in DOMAINS {
        for operation in operations[domain].as_array().unwrap() {
            workspace["local"][domain]
                .as_array_mut()
                .unwrap()
                .push(operation.clone());
            if workspace["neverSent"].get(domain).is_none() {
                workspace["neverSent"][domain] = json!([]);
            }
            workspace["neverSent"][domain]
                .as_array_mut()
                .unwrap()
                .push(operation["id"].clone());
            identities[domain]
                .as_array_mut()
                .unwrap()
                .push(operation["id"].clone());
        }
    }
    (workspace, identities)
}
