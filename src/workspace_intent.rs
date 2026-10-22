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
mod pwa_selection;

pub(crate) use allocation::validate_uuid;

fn invalid(message: &str) -> CoreError {
    CoreError::InvalidInput(message.into())
}

pub(crate) fn plan_json(raw: &str) -> Result<String, CoreError> {
    crate::reconciliation::workspace::display::persist_result(plan_raw(raw)?)
}

fn plan_raw(raw: &str) -> Result<String, CoreError> {
    let value = crate::strict_json::parse(raw)?;
    let mut input = pwa_selection::parse(&value)?;
    known_tasks::validate(&input, &value)?;
    validate_intent_metadata(&input, &value)?;
    // The decoded input owns every planning field; release the request tree
    // before the composed projections materialize full history copies.
    drop(value);
    validate(&input)?;
    if let Some(lifecycle) = &input.lifecycle {
        completion_mutation::lifecycle_evidence(lifecycle, &input.workspace)?;
    }
    let result = if input.intent.is_workspace_mutation() {
        mutation::plan(&mut input)?
    } else {
        plan_timer(&mut input)?
    };
    pwa_selection::with_lifecycle(&input, result)
}

fn plan_timer(input: &mut Input) -> Result<String, CoreError> {
    let (before, observed) = monotonic::before(input)?;
    let decision = projection::entrypoint(input, &before, &observed, &input.clock.physical_now)?;
    let target = policy::target(input, &decision);
    policy::validate_target(input, &target["canonicalTimer"])?;
    let mut selection = policy::selection(input, &decision)?;
    let (workspace, allocation, mut observation, commands) =
        prepare_commands(input, &target, observed)?;
    // The staged base history is fully installed in the prepared workspace;
    // release the request copy before the composed tail runs.
    let staged_base = std::mem::replace(&mut input.workspace["base"], Value::Null);
    if input.compatibility == Compatibility::AppleWorkspace
        && commands.iter().any(|command| command["type"] == "start")
    {
        selection.explicit = false;
    }
    let clear_timer = (matches!(input.intent, Intent::Clear)
        && !before["canonicalTimer"].is_null())
    .then(|| before["canonicalTimer"]["id"].clone());
    let after = projection::after(input, before, &workspace, &observation, &commands)?;
    policy::after_commands(
        input,
        &decision,
        &after["admission"]["workspace"],
        &commands,
        &mut selection,
    )?;
    // Retarget evidence may replay the live request workspace, so stage the
    // base back for that call and release it again immediately after.
    input.workspace["base"] = staged_base;
    monotonic::after(input, &after["workspace"], &mut observation)?;
    drop(std::mem::replace(&mut input.workspace["base"], Value::Null));
    // Release the full pre-command trees before assembling the moved output.
    drop(decision);
    finish_timer_plan(
        input,
        TimerCompletion {
            workspace,
            allocation,
            observation,
            commands,
            selection,
            target,
            after,
            clear_timer,
        },
    )
}

struct TimerCompletion {
    workspace: Value,
    allocation: model::Allocation,
    observation: model::Observation,
    commands: Vec<Value>,
    selection: model::Selection,
    target: Value,
    after: Value,
    clear_timer: Option<Value>,
}

fn finish_timer_plan(input: &Input, completion: TimerCompletion) -> Result<String, CoreError> {
    let TimerCompletion {
        workspace,
        allocation,
        observation,
        commands,
        selection,
        target,
        mut after,
        clear_timer,
    } = completion;
    let changed = !commands.is_empty()
        || serde_json::to_value(&selection)? != serde_json::to_value(&input.selection)?;
    let effects = effects(input, &commands, changed, clear_timer)?;
    let reason = if changed {
        String::new()
    } else {
        policy::no_change_reason(input, &target).to_owned()
    };
    let outcomes = admission::command_outcomes(&after["workspace"], &commands);
    let timer_observation = monotonic::timer_observation(input, &after["workspace"], &observation)?;
    let atomic_command_ids = serde_json::to_value(
        commands
            .iter()
            .map(|command| &command["id"])
            .collect::<Vec<_>>(),
    )?;
    let ownership_writes = serde_json::Value::from(ownership_writes(input, &commands));
    let outcomes = serde_json::to_value(&outcomes)?;
    drop(target);
    let projection = after["workspace"].take();
    assemble_timer_envelope(TimerEnvelope {
        workspace,
        allocation,
        observation,
        commands,
        selection,
        outcomes,
        timer_observation,
        atomic_command_ids,
        ownership_writes,
        projection,
        reason,
        effects,
        changed,
    })
}

struct TimerEnvelope {
    workspace: Value,
    allocation: model::Allocation,
    observation: model::Observation,
    commands: Vec<Value>,
    selection: model::Selection,
    outcomes: Value,
    timer_observation: Value,
    atomic_command_ids: Value,
    ownership_writes: Value,
    projection: Value,
    reason: String,
    effects: Vec<Value>,
    changed: bool,
}

fn assemble_timer_envelope(envelope: TimerEnvelope) -> Result<String, CoreError> {
    let TimerEnvelope {
        workspace,
        allocation,
        observation,
        commands,
        selection,
        outcomes,
        timer_observation,
        atomic_command_ids,
        ownership_writes,
        projection,
        reason,
        effects,
        changed,
    } = envelope;
    // Map inserts move large subtrees; the json! macro would clone them.
    let mut output = serde_json::Map::with_capacity(12);
    output.insert("schemaVersion".into(), serde_json::Value::from(1));
    output.insert(
        "outcome".into(),
        serde_json::Value::from(if changed { "planned" } else { "noop" }),
    );
    output.insert("reason".into(), serde_json::Value::from(reason));
    output.insert("workspace".into(), workspace);
    output.insert("selection".into(), serde_json::to_value(&selection)?);
    output.insert("allocation".into(), serde_json::to_value(&allocation)?);
    output.insert("observation".into(), serde_json::to_value(&observation)?);
    output.insert("commands".into(), serde_json::Value::from(commands));
    output.insert("commandOutcomes".into(), outcomes);
    output.insert("timerObservation".into(), timer_observation);
    output.insert("atomicCommandIds".into(), atomic_command_ids);
    output.insert("ownershipWrites".into(), ownership_writes);
    output.insert("projection".into(), projection);
    output.insert(
        "effectsAfterCommit".into(),
        serde_json::Value::from(effects),
    );
    Ok(serde_json::Value::Object(output).to_string())
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
    crate::reconciliation::workspace::display::validate_profile(
        &input.workspace,
        input.compatibility == Compatibility::PwaStorage,
    )?;
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
    clear_timer: Option<Value>,
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
    if matches!(input.intent, Intent::Clear) && clear_timer.is_some() {
        effects.push(json!({"kind": "clearCompletionAlert", "timerId": clear_timer}));
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
