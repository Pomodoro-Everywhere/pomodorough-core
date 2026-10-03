//! Raw persisted PWA display records are not delivery evidence.
use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::delivery::QUEUES;
use crate::{CoreError, strict_json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Context {
    profile: Profile,
    projection_pending: super::super::RequiredNullable<Value>,
}

#[derive(Deserialize)]
enum Profile {
    #[serde(rename = "pwaStorage")]
    PwaStorage,
}

fn invalid(reason: &str) -> CoreError {
    CoreError::InvalidInput(reason.into())
}

pub(crate) fn validate_profile(workspace: &Value, pwa: bool) -> Result<(), CoreError> {
    if !pwa && workspace.get("displayContext").is_some() {
        return Err(invalid("display context requires PWA profile"));
    }
    Ok(())
}

pub(crate) fn stored(workspace: &Value) -> Result<Option<Value>, CoreError> {
    let Some(raw) = workspace.get("displayContext") else {
        return Ok(None);
    };
    let object = strict_json::object(raw, "displayContext")?;
    if !object.contains_key("projectionPending") {
        return Err(invalid("missing displayContext.projectionPending"));
    }
    let context: Context = serde_json::from_value(raw.clone())?;
    let Profile::PwaStorage = context.profile;
    Ok(Some(context.projection_pending.0.unwrap_or(Value::Null)))
}

pub(crate) fn context(queues: Value) -> Value {
    json!({"profile": "pwaStorage", "projectionPending": queues})
}

pub(crate) fn queues(workspace: &Value, stored: Option<&Value>) -> Result<Value, CoreError> {
    let retained = &workspace["local"];
    let Some(stored) = stored.filter(|stored| !stored.is_null()) else {
        return Ok(retained.clone());
    };
    validate_stored(retained, stored)?;
    let mut display = stored.clone();
    let identities: BTreeSet<_> = stored["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| command["id"].as_str().unwrap())
        .collect();
    let proof = &workspace["neverSent"]["commands"];
    for command in retained["commands"].as_array().unwrap() {
        // Bootstrap consults this selector before its complete timer replay.
        let parsed: crate::timer::WireCommand = serde_json::from_value(command.clone())?;
        let proven = proof
            .as_array()
            .is_some_and(|proof| proof.contains(&command["id"]));
        if !identities.contains(parsed.id.as_str())
            && proven
            && newer_than_head(&parsed, &workspace["canonicalHead"])
        {
            display["commands"]
                .as_array_mut()
                .unwrap()
                .push(command.clone());
        }
    }
    Ok(display)
}

fn validate_stored(retained: &Value, stored: &Value) -> Result<(), CoreError> {
    let object = strict_json::object(stored, "projectionPending")?;
    if object.len() != QUEUES.len() {
        return Err(invalid("incomplete persisted projection queues"));
    }
    // Keep bootstrap's established sorted-domain error precedence.
    for name in retained.as_object().unwrap().keys() {
        strict_json::object_array_field(object, name, name, true)?;
        let retained_by_id: BTreeMap<_, _> = retained[name]
            .as_array()
            .unwrap()
            .iter()
            .map(|operation| (operation["id"].as_str(), operation))
            .collect();
        let mut identities = BTreeSet::new();
        for operation in stored[name].as_array().unwrap() {
            let id = operation["id"]
                .as_str()
                .ok_or_else(|| invalid("invalid persisted projection identity"))?;
            if !identities.insert(id) || retained_by_id.get(&Some(id)).copied() != Some(operation) {
                return Err(invalid(
                    "persisted projection does not match retained payloads",
                ));
            }
        }
    }
    Ok(())
}

fn newer_than_head(command: &crate::timer::WireCommand, head: &Value) -> bool {
    if head.is_null() {
        return true;
    }
    head["wallMs"]
        .as_i64()
        .zip(head["counter"].as_i64())
        .is_some_and(|head| (command.hlc_wall_ms, command.hlc_counter) > head)
}

// Only Core may call this after validating the original complete ledger.
// ACKs, drops and proven rewrites resolve records from the remaining raw rows.
pub(crate) fn trim(stored: &Value, retained: &Value) -> Value {
    if stored.is_null() {
        return Value::Null;
    }
    let mut trimmed = stored.clone();
    for name in QUEUES {
        let current: BTreeMap<_, _> = retained[name]
            .as_array()
            .unwrap()
            .iter()
            .map(|operation| (operation["id"].as_str().unwrap(), operation))
            .collect();
        trimmed[name] = json!(
            stored[name]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|operation| current.get(operation["id"].as_str().unwrap()).copied())
                .collect::<Vec<_>>()
        );
    }
    trimmed
}

pub(crate) fn trim_workspace(workspace: &mut Value) -> Result<(), CoreError> {
    if let Some(stored) = stored(workspace)? {
        workspace["displayContext"] = context(trim(&stored, &workspace["local"]));
    }
    Ok(())
}

pub(crate) fn persist_result(raw: String) -> Result<String, CoreError> {
    let mut result: Value = serde_json::from_str(&raw)?;
    if result["workspace"].get("displayContext").is_none() {
        return Ok(raw);
    }
    let stored = stored(&result["workspace"])?;
    let selected = queues(&result["workspace"], stored.as_ref())?;
    result["workspace"]["displayContext"] = context(selected);
    Ok(result.to_string())
}

pub(crate) fn admit_mutation_domains(
    workspace: &mut Value,
    operations: &Value,
    now: &str,
) -> Result<(), CoreError> {
    let Some(stored) = stored(workspace)? else {
        return Ok(());
    };
    let mut request = workspace.clone();
    request["now"] = json!(now);
    let projected: Value = serde_json::from_str(&super::project_json(&request.to_string())?)?;
    let mut selected = queues(workspace, Some(&stored))?;
    for name in QUEUES.into_iter().filter(|name| *name != "commands") {
        if !operations[name].as_array().unwrap().is_empty()
            && !projected["projectionPending"][name]
                .as_array()
                .unwrap()
                .is_empty()
        {
            selected[name] = projected["projectionPending"][name].clone();
        }
    }
    workspace["displayContext"] = context(selected);
    Ok(())
}
