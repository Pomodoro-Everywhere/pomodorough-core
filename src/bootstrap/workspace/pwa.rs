use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::Local;
use crate::{CoreError, strict_json, timer::WireCommand};

pub(super) fn commands(local: &Local) -> Result<Vec<WireCommand>, CoreError> {
    let retained = &local.workspace["local"];
    let Some(stored) = &local.projection_pending else {
        return Ok(serde_json::from_value(retained["commands"].clone())?);
    };
    validate_stored(retained, stored)?;
    let mut commands: Vec<WireCommand> = serde_json::from_value(stored["commands"].clone())?;
    let identities: BTreeSet<_> = commands.iter().map(|command| command.id.clone()).collect();
    let proof = &local.workspace["neverSent"]["commands"];
    for value in retained["commands"].as_array().unwrap() {
        let command: WireCommand = serde_json::from_value(value.clone())?;
        let proven = proof
            .as_array()
            .is_some_and(|proof| proof.iter().any(|id| id == &command.id));
        if !identities.contains(&command.id)
            && proven
            && newer_than_head(&command, &local.workspace["canonicalHead"])
        {
            commands.push(command);
        }
    }
    Ok(commands)
}

fn validate_stored(retained: &Value, stored: &Value) -> Result<(), CoreError> {
    let object = strict_json::object(stored, "projectionPending")?;
    let queues = retained.as_object().unwrap();
    if object.len() != queues.len() {
        return Err(recovery("incomplete persisted projection queues"));
    }
    for (name, queue) in queues {
        strict_json::object_array_field(object, name, name, true)?;
        let retained_by_id: BTreeMap<_, _> = queue
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|operation| operation["id"].as_str().map(|id| (id, operation)))
            .collect();
        let mut identities = BTreeSet::new();
        for operation in stored[name].as_array().unwrap() {
            let id = operation["id"]
                .as_str()
                .ok_or_else(|| recovery("invalid persisted projection identity"))?;
            if !identities.insert(id) || retained_by_id.get(id).copied() != Some(operation) {
                return Err(recovery(
                    "persisted projection does not match retained payloads",
                ));
            }
        }
    }
    Ok(())
}

fn newer_than_head(command: &WireCommand, head: &Value) -> bool {
    if head.is_null() {
        return true;
    }
    head["wallMs"]
        .as_i64()
        .zip(head["counter"].as_i64())
        .is_some_and(|head| (command.hlc_wall_ms, command.hlc_counter) > head)
}

fn recovery(reason: &str) -> CoreError {
    CoreError::InvalidInput(format!("bootstrap workspace requires recovery: {reason}"))
}
