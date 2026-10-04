use std::collections::BTreeSet;

use serde_json::Value;

use super::{CoreError, DOMAINS, Input, Profile, invalid};

pub(super) fn shape(raw: &Value) -> Result<(), CoreError> {
    let root = crate::strict_json::object(raw, "legacy migration")?;
    if raw["profile"].as_str() != Some("pwaStorage") {
        return Err(invalid("legacy migration requires pwaStorage profile"));
    }
    crate::strict_json::object_field(root, "ownership", "ownership")?;
    crate::strict_json::object_field(root, "identities", "identities")?;
    let workspace = crate::strict_json::object_field(root, "workspace", "workspace")?;
    crate::strict_json::nullable_object_field(workspace, "canonicalHead", "canonicalHead")?;
    let display = crate::strict_json::object_field(workspace, "displayContext", "displayContext")?;
    if display.get("profile").and_then(Value::as_str) != Some("pwaStorage") {
        return Err(invalid(
            "legacy display context requires pwaStorage profile",
        ));
    }
    Ok(())
}

pub(super) fn request(input: &Input) -> Result<BTreeSet<String>, CoreError> {
    let Profile::PwaStorage = input.profile;
    crate::strict_json::object(&input.settings, "settings")?;
    owner(&input.ownership.owner_id)?;
    owner(&input.ownership.expected_owner_id)?;
    if input.ownership.owner_id != input.ownership.expected_owner_id {
        return Err(invalid("stale account ownership"));
    }
    if input.device_id.is_empty() || input.identities.operation_uuids.len() > 5 {
        return Err(invalid("invalid legacy allocation"));
    }
    let mut supplied = BTreeSet::new();
    for uuid in &input.identities.operation_uuids {
        crate::workspace_intent::validate_uuid(uuid, Some(b'4'))?;
        if !supplied.insert(uuid) {
            return Err(invalid("duplicate legacy operation identity"));
        }
    }
    crate::reconciliation::workspace::validate_shape(&input.workspace)?;
    if input.workspace.get("displayContext").is_none() || input.workspace.get("neverSent").is_none()
    {
        return Err(invalid(
            "legacy migration requires raw display context and delivery proof",
        ));
    }
    let stored = crate::reconciliation::workspace::display::stored(&input.workspace)?;
    crate::reconciliation::workspace::display::queues(&input.workspace, stored.as_ref())?;
    let mut occupied = BTreeSet::new();
    for domain in DOMAINS {
        for operation in input.workspace["local"][domain].as_array().unwrap() {
            let id = operation["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| invalid("invalid retained legacy identity"))?;
            occupied.insert(id.to_owned());
        }
    }
    occupied.extend(outgoing(input)?);
    Ok(occupied)
}

fn owner(value: &Value) -> Result<(), CoreError> {
    if !value.is_null() && value.as_str().is_none_or(str::is_empty) {
        return Err(invalid("invalid account ownership identity"));
    }
    Ok(())
}

fn outgoing(input: &Input) -> Result<BTreeSet<String>, CoreError> {
    let mut ids = BTreeSet::new();
    if input.outgoing.is_null() {
        return Ok(ids);
    }
    let outgoing = crate::strict_json::object(&input.outgoing, "outgoing")?;
    if outgoing.get("ownerId") != Some(&input.ownership.owner_id) {
        return Err(invalid("stale outgoing ownership"));
    }
    for field in ["sent", "payload"] {
        if let Some(queues) = outgoing.get(field) {
            claim_queues(queues, input, &mut ids)?;
        }
    }
    if let Some(body) = outgoing.get("body") {
        let body = body
            .as_str()
            .ok_or_else(|| invalid("outgoing body must be a string"))?;
        claim_queues(&crate::strict_json::parse(body)?, input, &mut ids)?;
    }
    if let Some(queues) = outgoing.get("queueIds") {
        claim_ids(queues, input, &mut ids)?;
    }
    // Missing saved bodies stay missing. Importing preferences grants no right
    // to rebuild, rotate, send, retire, or otherwise rewrite an existing claim.
    Ok(ids)
}

fn claim_queues(
    queues: &Value,
    input: &Input,
    ids: &mut BTreeSet<String>,
) -> Result<(), CoreError> {
    crate::strict_json::object(queues, "outgoing queues")?;
    for domain in DOMAINS {
        let Some(queue) = queues.get(domain) else {
            continue;
        };
        let queue = queue
            .as_array()
            .ok_or_else(|| invalid("invalid outgoing queue"))?;
        for operation in queue {
            let id = operation["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| invalid("invalid outgoing identity"))?;
            claimed(domain, id, input, ids)?;
        }
    }
    Ok(())
}

fn claim_ids(queues: &Value, input: &Input, ids: &mut BTreeSet<String>) -> Result<(), CoreError> {
    let queues = crate::strict_json::object(queues, "outgoing queueIds")?;
    if queues.keys().any(|name| !DOMAINS.contains(&name.as_str())) {
        return Err(invalid("unknown outgoing identity queue"));
    }
    for (domain, queue) in queues {
        for id in queue
            .as_array()
            .ok_or_else(|| invalid("invalid outgoing identity queue"))?
        {
            let id = id
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| invalid("invalid outgoing identity"))?;
            claimed(domain, id, input, ids)?;
        }
    }
    Ok(())
}

fn claimed(
    domain: &str,
    id: &str,
    input: &Input,
    ids: &mut BTreeSet<String>,
) -> Result<(), CoreError> {
    if input.workspace["neverSent"][domain]
        .as_array()
        .is_some_and(|proof| proof.iter().any(|item| item == id))
    {
        return Err(invalid(
            "legacy outgoing identity conflicts with never-sent proof",
        ));
    }
    ids.insert(id.to_owned());
    Ok(())
}
