use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::{CoreError, Input, Profile, invalid};

pub(super) const DOMAINS: [&str; 5] = [
    "commands",
    "taskOperations",
    "durationOperations",
    "autoStartOperations",
    "selectedTaskOperations",
];

pub(super) fn request(input: &Input) -> Result<Value, CoreError> {
    if input.device_id.is_empty()
        || input.ownership["ownerId"] != input.ownership["expectedOwnerId"]
    {
        return Err(invalid("stale legacy dependency ownership or device"));
    }
    for field in ["ownerId", "expectedOwnerId"] {
        if input.ownership[field].as_str() == Some("") {
            return Err(invalid("invalid account ownership identity"));
        }
    }
    if !input.outgoing.is_null()
        && input.outgoing.get("ownerId") != Some(&input.ownership["ownerId"])
    {
        return Err(invalid("stale outgoing ownership"));
    }
    calendar(input)?;
    let mut workspace = input.workspace.clone();
    workspace["timerDependencies"] = json!([]);
    project(&workspace, input)?;
    owner(input)?;
    let projected = source_projection(&workspace, input)?;
    super::evidence::validate_claim(input)?;
    Ok(projected)
}

fn source_projection(workspace: &Value, input: &Input) -> Result<Value, CoreError> {
    let decorated = decorated(workspace, input);
    let request =
        json!({"base": decorated["base"], "pending": decorated["local"], "now": workspace["now"]})
            .to_string();
    let mut times = BTreeMap::new();
    for command in workspace["local"]["commands"].as_array().unwrap() {
        if let Some(time) = command
            .get("physicalOccurredAt")
            .filter(|time| !time.is_null())
        {
            let time = time
                .as_str()
                .ok_or_else(|| invalid("invalid physical source time"))?;
            crate::timer::parse_time(time)?;
            times.insert(command["id"].as_str().unwrap().to_owned(), time.to_owned());
        }
    }
    let raw = if times.is_empty() {
        crate::projection::apply_workspace_json(&request)?
    } else {
        crate::projection::apply_observed_workspace_json(
            &request,
            &crate::timer::workspace::observation::Observation {
                canonical_anchor_at: None,
                command_times: &times,
            },
        )?
    };
    crate::legacy_preferences::json::parse(&raw)
}

pub(super) fn project(workspace: &Value, input: &Input) -> Result<(), CoreError> {
    crate::reconciliation::workspace::project_json(&decorated(workspace, input).to_string())?;
    Ok(())
}

fn owner(input: &Input) -> Result<(), CoreError> {
    let owner = &input.ownership["timerOwner"];
    if !owner.is_null()
        && ["timerId", "deviceId"]
            .iter()
            .any(|field| owner[*field].as_str().is_none_or(str::is_empty))
    {
        return Err(invalid("invalid stored timer ownership"));
    }
    if owner["leaseExpiresAtMs"]
        .as_i64()
        .is_some_and(|lease| lease < 0)
    {
        return Err(invalid("invalid stored timer lease"));
    }
    if input.workspace["displayContext"]["profile"] != "pwaStorage"
        || input.workspace["displayContext"]
            .get("projectionPending")
            .is_none()
    {
        return Err(invalid(
            "legacy dependency plan requires raw display context",
        ));
    }
    if input.profile == Profile::AndroidCentralized {
        for command in input.workspace["local"]["commands"].as_array().unwrap() {
            if let Some(proof) = command.get("neverSent") {
                if *proof
                    != super::evidence::never_sent(
                        input,
                        "commands",
                        command["id"].as_str().unwrap(),
                    )
                {
                    return Err(invalid("native row proof differs from delivery proof"));
                }
            }
        }
    }
    Ok(())
}

fn decorated(workspace: &Value, input: &Input) -> Value {
    let mut copy = workspace.clone();
    for field in ["local", "displayContext"] {
        let queues = if field == "local" {
            &mut copy[field]
        } else {
            &mut copy[field]["projectionPending"]
        };
        if queues.is_null() {
            continue;
        }
        for domain in DOMAINS {
            for operation in queues[domain].as_array_mut().unwrap() {
                if operation.get("deviceId").is_none()
                    && (domain != "commands" || input.profile == Profile::AndroidCentralized)
                {
                    operation["deviceId"] = json!(input.device_id);
                }
            }
        }
    }
    copy
}

fn calendar(input: &Input) -> Result<(), CoreError> {
    let mut previous_end = None;
    let mut ranges = input
        .calendar_intervals
        .iter()
        .map(|range| {
            Ok((
                crate::timer::parse_time(&range.start)?,
                crate::timer::parse_time(&range.end)?,
            ))
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    ranges.sort();
    for (start, end) in ranges {
        if !(1..=26 * 60 * 60 * 1000).contains(&(end - start).num_milliseconds())
            || previous_end.is_some_and(|previous| start < previous)
        {
            return Err(invalid("invalid legacy calendar intervals"));
        }
        previous_end = Some(end);
    }
    let mut acknowledgements = BTreeSet::new();
    for ack in &input.source_acknowledgements {
        if ack.command_id.is_empty() || !acknowledgements.insert(&ack.command_id) {
            return Err(invalid(
                "duplicate or invalid legacy source acknowledgement",
            ));
        }
        let _ = &ack.reason;
    }
    Ok(())
}
