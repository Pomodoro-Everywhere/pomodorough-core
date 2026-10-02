use serde_json::{Value, json};

use crate::CoreError;
use model::{Compatibility, Input, Intent, ReplicationMode};

mod admission;
mod allocation;
pub(crate) mod completion_mutation;
mod known_tasks;
mod model;
mod monotonic;
mod mutation;
mod policy;
mod projection;

fn invalid(message: &str) -> CoreError {
    CoreError::InvalidInput(message.into())
}

pub(crate) fn plan_json(raw: &str) -> Result<String, CoreError> {
    let value = crate::strict_json::parse(raw)?;
    let input: Input = serde_json::from_value(value.clone())?;
    known_tasks::validate(&input, &value)?;
    validate_intent_metadata(&input, &value)?;
    validate(&input)?;
    if input.intent.is_workspace_mutation() {
        return mutation::plan(&input);
    }
    let (before, observed) = monotonic::before(&input)?;
    let decision = projection::entrypoint(&input, &before, &observed, &input.clock.physical_now)?;
    let target = policy::target(&input, &decision);
    policy::validate_target(&input, &target["canonicalTimer"])?;
    let mut selection = policy::selection(&input, &decision)?;
    let (workspace, allocation, mut observation, commands) =
        prepare_commands(&input, &target, observed)?;
    if input.compatibility == Compatibility::AppleWorkspace
        && commands.iter().any(|command| command["type"] == "start")
    {
        selection.explicit = false;
    }
    let after = projection::after(&input, &before, &workspace, &observation, &commands)?;
    policy::after_commands(
        &input,
        &decision,
        &after["admission"]["workspace"],
        &commands,
        &mut selection,
    )?;
    monotonic::after(&input, &after["workspace"], &mut observation)?;
    let changed = !commands.is_empty()
        || serde_json::to_value(&selection)? != serde_json::to_value(&input.selection)?;
    let effects = effects(&input, &commands, changed, &before)?;
    Ok(
        json!({"schemaVersion": 1, "outcome": if changed {"planned"} else {"noop"},
        "reason": if changed {""} else {policy::no_change_reason(&input, &target)},
        "workspace": workspace, "selection": selection, "allocation": allocation,
         "observation": observation, "commands": commands,
         "commandOutcomes": admission::command_outcomes(&after["workspace"], &commands),
        "timerObservation": monotonic::timer_observation(&input, &after["workspace"], &observation)?,
        "atomicCommandIds": commands.iter().map(|command| &command["id"]).collect::<Vec<_>>(),
        "ownershipWrites": ownership_writes(&input, &commands),
        "projection": after["workspace"], "effectsAfterCommit": effects})
        .to_string(),
    )
}

fn validate_intent_metadata(input: &Input, value: &Value) -> Result<(), CoreError> {
    if !input.intent.is_workspace_mutation() {
        if value.get("ownership").is_some()
            || value.get("durability").is_some()
            || value.get("localDurationsMs").is_some()
        {
            return Err(invalid(
                "unexpected task mutation metadata for timer intent",
            ));
        }
        return Ok(());
    }
    if matches!(input.intent, Intent::SelectTask { .. }) && value["intent"].get("taskId").is_none()
    {
        return Err(invalid("selectTask requires explicit taskId or null"));
    }
    if let Some(owner) = value.get("ownership") {
        if owner.get("ownerId").is_none() || owner.get("expectedOwnerId").is_none() {
            return Err(invalid("ownership requires both raw identities"));
        }
    }
    Ok(())
}

fn prepare_commands(
    input: &Input,
    before: &Value,
    mut observation: model::Observation,
) -> Result<(Value, model::Allocation, model::Observation, Vec<Value>), CoreError> {
    let kinds = policy::commands(input, &before["canonicalTimer"]);
    let mut workspace = input.workspace.clone();
    let mut allocation = input.allocation.clone();
    let mut commands = Vec::new();
    for (index, kind) in kinds.iter().enumerate() {
        allocation::advance(input, &mut allocation)?;
        let command = allocation::command(input, &allocation, before, &observation, *kind, index)?;
        append(&mut workspace, &command)?;
        observation.command_times.insert(
            command["id"].as_str().unwrap().into(),
            input.clock.physical_now.clone(),
        );
        allocation.last_uuid = Some(input.identities.command_uuids[index].clone());
        commands.push(command);
    }
    Ok((workspace, allocation, observation, commands))
}

fn validate(input: &Input) -> Result<(), CoreError> {
    let workspace = input
        .workspace
        .as_object()
        .ok_or_else(|| invalid("workspace must be an object"))?;
    if workspace.contains_key("now") {
        return Err(invalid("workspace.now is owned by intent planner"));
    }
    policy::validate_generation(input)?;
    allocation::validate(input)?;
    let physical = crate::timer::parse_time(&input.clock.physical_now)?;
    let observed = crate::timer::parse_time(&input.clock.observed_at)?;
    crate::timer::parse_time(&input.clock.occurred_at)?;
    monotonic::validate(input)?;
    if observed > physical {
        return Err(invalid("observation is later than physical now"));
    }
    // Validate the raw wire aggregate before consulting any physical-time overlay.
    projection::project(&input.workspace, &input.clock.occurred_at)?;
    validate_observation(input)?;
    if let Some(timer) = &input.requested_timer {
        crate::timer::validate_canonical_timer(timer)?;
    }
    Ok(())
}

fn validate_observation(input: &Input) -> Result<(), CoreError> {
    if let Some(anchor) = &input.observation.canonical_anchor_at {
        crate::timer::parse_time(anchor)?;
        if input.workspace["base"]["canonicalTimer"].is_null() {
            return Err(invalid("physical anchor without canonical timer"));
        }
    }
    let commands = input.workspace["local"]["commands"].as_array().unwrap();
    for (id, at) in &input.observation.command_times {
        crate::timer::parse_time(at)?;
        if !commands.iter().any(|command| command["id"] == *id) {
            return Err(invalid("physical command time without retained command"));
        }
    }
    Ok(())
}

pub(super) fn append(workspace: &mut Value, command: &Value) -> Result<(), CoreError> {
    let timer_id = &command["timerId"];
    if command["type"] == "start" {
        let base = &workspace["base"];
        let exists = base["canonicalTimer"]["id"] == *timer_id
            || base["history"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["timerId"] == *timer_id)
            || workspace["local"]["commands"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["timerId"] == *timer_id);
        if exists {
            return Err(invalid("allocated timer identity already exists"));
        }
    }
    let dependency = inherited_dependency(workspace, command);
    workspace["local"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(command.clone());
    if workspace.get("neverSent").is_none() {
        workspace["neverSent"] = json!({});
    }
    if workspace["neverSent"].get("commands").is_none() {
        workspace["neverSent"]["commands"] = json!([]);
    }
    workspace["neverSent"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(command["id"].clone());
    if let Some(parent) = dependency {
        workspace["timerDependencies"]
            .as_array_mut()
            .unwrap()
            .push(json!({
            "operationId": command["id"], "dependsOnOperationId": parent}));
    }
    Ok(())
}

fn inherited_dependency(workspace: &Value, command: &Value) -> Option<Value> {
    if command["type"] == "start" {
        return None;
    }
    let retained = workspace["local"]["commands"].as_array().unwrap();
    let edges = workspace["timerDependencies"].as_array().unwrap();
    let latest = retained
        .iter()
        .filter(|item| item["timerId"] == command["timerId"])
        .max_by_key(|item| {
            (
                item["hlcWallMs"].as_i64(),
                item["hlcCounter"].as_i64(),
                item["deviceId"].as_str(),
                item["id"].as_str(),
            )
        })?;
    edges
        .iter()
        .any(|edge| edge["operationId"] == latest["id"])
        .then(|| latest["id"].clone())
}

fn effects(
    input: &Input,
    commands: &[Value],
    changed: bool,
    before: &Value,
) -> Result<Vec<Value>, CoreError> {
    if !changed {
        return Ok(vec![]);
    }
    let mut effects = vec![];
    if !commands.is_empty() {
        effects.push(json!({"kind": "launchSync"}));
    }
    for command in commands {
        let kind = match command["type"].as_str().unwrap() {
            "start" => "scheduleAlarm",
            "resume" => "resumeAlarm",
            "pause" => "pauseAlarm",
            _ => "cancelAlarm",
        };
        let mut effect = json!({"kind": kind, "timerId": command["timerId"]});
        if matches!(kind, "scheduleAlarm" | "resumeAlarm") {
            let remaining = command["plannedDurationMs"].as_i64().unwrap()
                - command["observedElapsedMs"].as_i64().unwrap();
            effect["durationMs"] = json!(remaining.max(1000));
            effect["phase"] = command["phase"].clone();
        }
        if !effects.contains(&effect) {
            effects.push(effect);
        }
    }
    if matches!(input.intent, Intent::Clear) && !before["canonicalTimer"].is_null() {
        effects.push(
            json!({"kind": "clearCompletionAlert", "timerId": before["canonicalTimer"]["id"]}),
        );
    }
    Ok(effects)
}

fn ownership_writes(input: &Input, commands: &[Value]) -> Vec<Value> {
    let mut writes = vec![];
    for command in commands {
        if command["type"] == "start" {
            let kind = match input.replication_mode {
                ReplicationMode::Centralized => Some("recordStart"),
                ReplicationMode::Iroh
                    if matches!(
                        input.compatibility,
                        Compatibility::AppleWorkspace | Compatibility::AndroidCoordinator
                    ) =>
                {
                    Some("recordLocalStart")
                }
                ReplicationMode::Iroh => None,
            };
            if let Some(kind) = kind {
                writes.push(json!({"kind": kind, "timerId": command["timerId"],
                    "deviceId": input.allocation.device_id, "startCommandId": command["id"]}));
            }
        }
    }
    if input.compatibility == Compatibility::PwaStorage
        && matches!(input.intent, Intent::CancelAndClear)
        && !commands.is_empty()
    {
        writes.push(json!({"kind": "removeTimerOwner"}));
    }
    writes
}
