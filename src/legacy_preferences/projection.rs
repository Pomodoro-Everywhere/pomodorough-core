use serde_json::{Value, json};

use super::{CoreError, DOMAINS, invalid};

pub(super) fn project(workspace: &Value, device_id: &str) -> Result<Value, CoreError> {
    let mut wire = workspace.clone();
    decorate(&mut wire["local"], device_id)?;
    let stored = &mut wire["displayContext"]["projectionPending"];
    if !stored.is_null() {
        decorate(stored, device_id)?;
    }
    Ok(serde_json::from_str(
        &crate::reconciliation::workspace::project_json(&wire.to_string())?,
    )?)
}

fn decorate(queues: &mut Value, device_id: &str) -> Result<(), CoreError> {
    for domain in DOMAINS {
        let queue = queues[domain]
            .as_array_mut()
            .ok_or_else(|| invalid("missing raw legacy queue"))?;
        for operation in queue {
            let operation = operation
                .as_object_mut()
                .ok_or_else(|| invalid("invalid raw legacy operation"))?;
            if domain != "commands" && !operation.contains_key("deviceId") {
                operation.insert("deviceId".into(), json!(device_id));
            }
        }
    }
    Ok(())
}

pub(super) fn raw_members(workspace: &Value, selected: &Value) -> Result<Value, CoreError> {
    let mut raw = super::empty_queues();
    for domain in DOMAINS {
        for operation in selected[domain]
            .as_array()
            .ok_or_else(|| invalid("invalid legacy display result"))?
        {
            let original = workspace["local"][domain]
                .as_array()
                .unwrap()
                .iter()
                .find(|original| original["id"] == operation["id"])
                .ok_or_else(|| invalid("legacy display identity is not retained"))?;
            raw[domain].as_array_mut().unwrap().push(original.clone());
        }
    }
    Ok(raw)
}
