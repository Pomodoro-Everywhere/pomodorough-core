use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::{CoreError, Input, invalid, validation::DOMAINS};

pub(super) fn validate_claim(input: &Input) -> Result<(), CoreError> {
    let mut claimed = BTreeMap::new();
    for queues in claim_queues(input)? {
        super::schema::claim(&queues)?;
        let mut workspace = input.workspace.clone();
        workspace["timerDependencies"] = json!([]);
        workspace["displayContext"]["projectionPending"] = Value::Null;
        workspace["neverSent"] = json!({});
        workspace["local"] = json!({});
        for domain in DOMAINS {
            workspace["local"][domain] = queues.get(domain).cloned().unwrap_or_else(|| json!([]));
            for row in workspace["local"][domain].as_array().unwrap() {
                claim_member(input, domain, row, &mut claimed)?;
            }
        }
        // HTTP command records omit deviceId. It belongs to the enclosing
        // request; decorate only the validation copy, never the saved body.
        for command in workspace["local"]["commands"].as_array_mut().unwrap() {
            if command.get("deviceId").is_none() {
                command["deviceId"] = json!(input.device_id);
            }
        }
        super::validation::project(&workspace, input)?;
    }
    claim_ids(input)?;
    for ack in &input.source_acknowledgements {
        if !claimed.contains_key(&("commands", ack.command_id.clone())) {
            return Err(invalid(
                "source acknowledgement has no saved command evidence",
            ));
        }
    }
    Ok(())
}

fn claim_member(
    input: &Input,
    domain: &'static str,
    row: &Value,
    claimed: &mut BTreeMap<(&'static str, String), Value>,
) -> Result<(), CoreError> {
    let id = row["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| invalid("invalid outgoing identity"))?;
    if let Some(previous) = claimed.insert((domain, id.to_owned()), row.clone()) {
        if !same_wire(domain, &previous, row) {
            return Err(invalid("conflicting saved outgoing records"));
        }
    }
    if never_sent(input, domain, id) {
        return Err(invalid("outgoing identity conflicts with never-sent proof"));
    }
    if domain == "commands" {
        time(row)?;
    }
    retained_claim(input, domain, row)
}

fn claim_ids(input: &Input) -> Result<(), CoreError> {
    let Some(queues) = input.outgoing.get("queueIds") else {
        return Ok(());
    };
    let queues = crate::strict_json::object(queues, "outgoing queueIds")?;
    for (domain, ids) in queues {
        if !DOMAINS.contains(&domain.as_str()) {
            return Err(invalid("unknown outgoing identity queue"));
        }
        let mut seen = BTreeSet::new();
        for id in ids
            .as_array()
            .ok_or_else(|| invalid("outgoing identity queue must be an array"))?
        {
            let id = id
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| invalid("invalid outgoing identity"))?;
            if !seen.insert(id) || never_sent(input, domain, id) {
                return Err(invalid("invalid outgoing identity or delivery proof"));
            }
        }
    }
    Ok(())
}

fn claim_queues(input: &Input) -> Result<Vec<Value>, CoreError> {
    let mut result = vec![];
    for field in ["sent", "payload"] {
        if let Some(queues) = input.outgoing.get(field) {
            result.push(queues.clone());
        }
    }
    if let Some(body) = input.outgoing.get("body") {
        let body = crate::legacy_preferences::json::parse(body.as_str().unwrap())?;
        if body
            .get("deviceId")
            .is_some_and(|id| id != &input.device_id)
        {
            return Err(invalid("saved request belongs to another device"));
        }
        result.push(body);
    }
    Ok(result)
}

pub(super) fn saved_command(input: &Input, id: &str) -> Result<Option<Value>, CoreError> {
    Ok(claim_queues(input)?
        .iter()
        .flat_map(|queues| queues["commands"].as_array().into_iter().flatten())
        .find(|row| row["id"] == id)
        .cloned())
}

pub(super) fn body_issue(input: &Input) -> Result<Option<&'static str>, CoreError> {
    if input.outgoing.is_null() {
        return Ok(None);
    }
    let Some(body) = input.outgoing.get("body") else {
        return Ok(Some("savedRequestBodyMissing"));
    };
    let body = crate::legacy_preferences::json::parse(body.as_str().unwrap())?;
    if body["deviceId"].as_str() != Some(input.device_id.as_str())
        || !DOMAINS.iter().any(|domain| body.get(domain).is_some())
    {
        return Ok(Some("savedRequestBodyIncomplete"));
    }
    for field in ["sent", "payload", "queueIds"] {
        if let Some(claim) = input.outgoing.get(field) {
            if !body_matches_claim(&body, claim, field == "queueIds") {
                return Ok(Some("savedRequestBodyIncomplete"));
            }
        }
    }
    if input.source_acknowledgements.iter().any(|ack| {
        !body["commands"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["id"] == ack.command_id))
    }) {
        return Ok(Some("savedRequestBodyIncomplete"));
    }
    Ok(None)
}

fn body_matches_claim(body: &Value, claim: &Value, identities_only: bool) -> bool {
    for domain in DOMAINS {
        let Some(claimed) = claim.get(domain).and_then(Value::as_array) else {
            continue;
        };
        let empty = vec![];
        let sent = body[domain].as_array().unwrap_or(&empty);
        if claimed.len() != sent.len() {
            return false;
        }
        for row in claimed {
            let id = if identities_only { row } else { &row["id"] };
            let Some(sent) = sent.iter().find(|sent| sent["id"] == *id) else {
                return false;
            };
            if !identities_only && !same_wire(domain, row, sent) {
                return false;
            }
        }
    }
    true
}

fn retained_claim(input: &Input, domain: &str, saved: &Value) -> Result<(), CoreError> {
    let Some(local) = input.workspace["local"][domain]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == saved["id"])
    else {
        return Ok(());
    };
    if !same_wire(domain, saved, local)
        || saved
            .get("deviceId")
            .zip(local.get("deviceId"))
            .is_some_and(|(saved, local)| saved != local)
    {
        return Err(invalid(
            "saved outgoing record differs from retained wire record",
        ));
    }
    Ok(())
}

fn same_wire(domain: &str, left: &Value, right: &Value) -> bool {
    wire_fields(domain)
        .iter()
        .all(|field| left[*field] == right[*field])
}

fn wire_fields(domain: &str) -> &'static [&'static str] {
    match domain {
        "commands" => &[
            "id",
            "deviceSequence",
            "timerId",
            "taskId",
            "type",
            "phase",
            "plannedDurationMs",
            "occurredAt",
            "hlcWallMs",
            "hlcCounter",
            "observedElapsedMs",
        ],
        "taskOperations" => &[
            "id",
            "taskId",
            "type",
            "title",
            "occurredAt",
            "hlcWallMs",
            "hlcCounter",
        ],
        "durationOperations" => &[
            "id",
            "phase",
            "durationMs",
            "occurredAt",
            "hlcWallMs",
            "hlcCounter",
        ],
        "autoStartOperations" => &["id", "enabled", "occurredAt", "hlcWallMs", "hlcCounter"],
        "selectedTaskOperations" => &["id", "taskId", "occurredAt", "hlcWallMs", "hlcCounter"],
        _ => unreachable!(),
    }
}

pub(super) fn never_sent(input: &Input, domain: &str, id: &str) -> bool {
    input.workspace["neverSent"][domain]
        .as_array()
        .is_some_and(|proof| proof.iter().any(|entry| entry == id))
}

pub(super) fn time(command: &Value) -> Result<DateTime<Utc>, CoreError> {
    let time = command
        .get("physicalOccurredAt")
        .filter(|value| !value.is_null())
        .unwrap_or(&command["occurredAt"]);
    crate::timer::parse_time(
        time.as_str()
            .ok_or_else(|| invalid("invalid legacy source time"))?,
    )
}

pub(super) fn completion<'a>(projection: &'a Value, id: &str) -> Option<&'a Value> {
    projection["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["commandId"] == id && row["phase"] == "focus" && row["status"] == "completed"
        })
}

pub(super) fn canonical_completion(input: &Input, id: &str) -> Option<Value> {
    if let Some(row) = completion(&input.workspace["base"], id) {
        return Some(row.clone());
    }
    let timer = &input.workspace["base"]["canonicalTimer"];
    (timer["phase"] == "focus" && timer["status"] == "completed"
        && timer["lastIntent"]["type"] == "finish" && timer["lastIntent"]["commandId"] == id)
        .then(|| json!({"id": timer["id"], "timerId": timer["id"], "commandId": id,
            "taskId": timer.get("taskId").cloned().unwrap_or(Value::Null),
            "phase": "focus", "status": "completed", "plannedDurationMs": timer["plannedDurationMs"],
            "completedAt": timer["anchorAt"], "endedAt": timer["anchorAt"]}))
}

pub(super) fn bounds(
    input: &Input,
    edge: &Value,
    source: &Value,
) -> Result<Option<Value>, CoreError> {
    let at = completion_time(source)?;
    let explicit = edge.get("sourceDayStart").zip(edge.get("sourceDayEnd"));
    if edge.get("sourceDayStart").is_some() != edge.get("sourceDayEnd").is_some() {
        return Err(invalid("incomplete legacy source day"));
    }
    if let Some((start, end)) = explicit {
        let first = crate::timer::parse_time(start.as_str().unwrap())?;
        let last = crate::timer::parse_time(end.as_str().unwrap())?;
        if !(1..=26 * 60 * 60 * 1000).contains(&(last - first).num_milliseconds())
            || at < first
            || at >= last
        {
            return Err(invalid("legacy source day excludes exact completion"));
        }
        return Ok(Some(json!({"sourceDayStart": start, "sourceDayEnd": end})));
    }
    for interval in &input.calendar_intervals {
        if at >= crate::timer::parse_time(&interval.start)?
            && at < crate::timer::parse_time(&interval.end)?
        {
            return Ok(Some(
                json!({"sourceDayStart": interval.start, "sourceDayEnd": interval.end}),
            ));
        }
    }
    Ok(None)
}

pub(super) fn completion_time(source: &Value) -> Result<DateTime<Utc>, CoreError> {
    crate::timer::parse_time(
        source["completedAt"]
            .as_str()
            .or(source["endedAt"].as_str())
            .ok_or_else(|| invalid("missing exact completion time"))?,
    )
}

pub(super) fn expected_phase(
    projection: &Value,
    source: &Value,
    bounds: &Value,
) -> Result<String, CoreError> {
    let source_at = completion_time(source)?;
    let key = source["commandId"].as_str().unwrap();
    let history: Vec<crate::timer::HistoryItem> =
        serde_json::from_value(projection["history"].clone())?;
    let mut causal = vec![];
    for row in history {
        let Some(at) = row.completed_at.as_deref().or(row.ended_at.as_deref()) else {
            continue;
        };
        let at = crate::timer::parse_time(at)?;
        let identity = row.command_id.as_deref().unwrap_or(&row.timer_id);
        if at < source_at || (at == source_at && identity <= key) {
            causal.push(row);
        }
    }
    if !causal
        .iter()
        .any(|row| row.command_id.as_deref() == Some(key))
    {
        causal.push(serde_json::from_value(source.clone())?);
    }
    crate::completion_plan::phase_after(
        "focus",
        &causal,
        (
            crate::timer::parse_time(bounds["sourceDayStart"].as_str().unwrap())?,
            crate::timer::parse_time(bounds["sourceDayEnd"].as_str().unwrap())?,
        ),
    )
}
