use serde_json::{Value, json};

use super::{
    CoreError, invalid,
    model::{Request, State, Trigger},
};
use crate::workspace_intent::{
    admission, allocation,
    model::{CommandKind, Compatibility, Input, Observation},
};

struct StartBatch {
    workspace: Value,
    allocation: crate::workspace_intent::model::Allocation,
    observation: Observation,
    command: Value,
}

pub(super) fn empty(request: &Request, input: &Input, reason: &str) -> Result<Value, CoreError> {
    Ok(
        json!({"schemaVersion": 1, "outcome": "noop", "reason": reason,
        "workspace": input.workspace, "selection": input.selection,
        "allocation": input.allocation, "observation": input.observation,
        "projection": super::observe(input, &input.workspace, &input.observation, &input.clock.observed_at)?["workspace"],
         "commands": [], "durableCommands": [], "atomicCommandIds": [],
         "commandOutcomes": [],
        "completionRecords": {"phaseAdvance": null, "pendingAutoBreak": null, "provisionalBreak": null},
        "ownershipWrites": [], "effectsAfterCommit": [], "lifecycle": request.lifecycle,
         "source": null, "sourceStatus": null, "nextPhase": null, "retiredTriggerIds": []}),
    )
}

pub(super) fn lifecycle(
    result: &mut Value,
    state: &State,
    retired: &[String],
) -> Result<(), CoreError> {
    super::validate_state(state)?;
    if result["lifecycle"] != serde_json::to_value(state)? || !retired.is_empty() {
        result["outcome"] = json!("planned");
    }
    result["lifecycle"] = serde_json::to_value(state)?;
    result["retiredTriggerIds"] = json!(retired);
    Ok(())
}

pub(super) fn start(
    request: &Request,
    input: &Input,
    phase: &str,
    dependency: Option<&Trigger>,
    source: &Value,
    result: &mut Value,
) -> Result<(), CoreError> {
    let mut start_input = request.context();
    start_input.selection.phase = serde_json::from_value(json!(phase))?;
    let mut state = input.allocation.clone();
    allocation::advance(input, &mut state)?;
    let mut command = allocation::command(
        &start_input,
        &state,
        &result["projection"],
        &input.observation,
        CommandKind::Start,
        0,
    )?;
    normalize_occurrence(input.compatibility, &mut command)?;
    let mut workspace = input.workspace.clone();
    crate::workspace_intent::append(&mut workspace, &command)?;
    record_dependency(
        request,
        dependency,
        source,
        &command,
        &mut workspace,
        result,
    )?;
    let mut observation = input.observation.clone();
    observation.command_times.insert(
        command["id"].as_str().unwrap().into(),
        input.clock.physical_now.clone(),
    );
    state.last_uuid = input.identities.command_uuids.first().cloned();
    install_start(
        request,
        input,
        StartBatch {
            workspace,
            allocation: state,
            observation,
            command,
        },
        result,
    )
}

fn install_start(
    request: &Request,
    input: &Input,
    batch: StartBatch,
    result: &mut Value,
) -> Result<(), CoreError> {
    let StartBatch {
        workspace,
        allocation,
        observation,
        command,
    } = batch;
    let after = after(input, &workspace, &observation, &command)?;
    let mut durable = command.clone();
    durable.as_object_mut().unwrap().remove("deviceId");
    result["outcome"] = json!("planned");
    result["reason"] = json!("");
    result["workspace"] = workspace;
    result["projection"] = after["workspace"].clone();
    result["allocation"] = serde_json::to_value(allocation)?;
    result["observation"] = serde_json::to_value(observation)?;
    if input.compatibility != Compatibility::AppleWorkspace {
        result["selection"]["phase"] = command["phase"].clone();
        advance_start_selection(request, &command, result);
    }
    result["commands"] = json!([command]);
    result["durableCommands"] = json!([durable]);
    result["atomicCommandIds"] = json!([command["id"]]);
    result["commandOutcomes"] = json!(admission::command_outcomes(
        &after["workspace"],
        std::slice::from_ref(&command)
    ));
    result["ownershipWrites"] = ownership(request, &command);
    result["effectsAfterCommit"] = json!([{"kind": "launchSync"},
        {"kind": "scheduleAlarm", "timerId": command["timerId"], "phase": command["phase"],
            "durationMs": command["plannedDurationMs"]}]);
    if input.compatibility == Compatibility::AppleWorkspace
        && matches!(request.stage, super::model::Stage::DeferredBreakOpportunity)
    {
        let cancel = json!({"kind": "cancelAlarm", "timerId": result["source"]["timerId"]});
        result["effectsAfterCommit"]
            .as_array_mut()
            .unwrap()
            .insert(1, cancel);
    }
    Ok(())
}

fn advance_start_selection(request: &Request, command: &Value, result: &mut Value) {
    if request.compatibility == Compatibility::AndroidCoordinator
        && request.selection.phase.name() != command["phase"]
    {
        result["selection"]["generation"] = json!(
            request
                .selection
                .generation
                .parse::<i64>()
                .unwrap()
                .wrapping_add(1)
                .to_string()
        );
    }
}

fn after(
    input: &Input,
    workspace: &Value,
    observation: &Observation,
    command: &Value,
) -> Result<Value, CoreError> {
    let after = super::observe(input, workspace, observation, &input.clock.physical_now)?;
    admission::commands(
        input,
        workspace,
        std::slice::from_ref(command),
        observation,
        &input.clock.physical_now,
    )?;
    Ok(after)
}

fn normalize_occurrence(profile: Compatibility, command: &mut Value) -> Result<(), CoreError> {
    if matches!(
        profile,
        Compatibility::DesktopStorage | Compatibility::DesktopTerminal
    ) {
        command["occurredAt"] = json!(
            crate::timer::parse_time(command["occurredAt"].as_str().unwrap())?
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        );
    }
    Ok(())
}

fn record_dependency(
    request: &Request,
    trigger: Option<&Trigger>,
    source: &Value,
    command: &Value,
    workspace: &mut Value,
    result: &mut Value,
) -> Result<(), CoreError> {
    let Some(trigger) = trigger else {
        return Ok(());
    };
    let day = super::day(request, source["completedAt"].as_str().unwrap())?;
    workspace["timerDependencies"]
        .as_array_mut()
        .ok_or_else(|| invalid("missing timer dependencies"))?
        .push(
            json!({"operationId": command["id"], "dependsOnOperationId": trigger.finish_command_id,
            "generatedBreak": true, "sourceDayStart": day.start, "sourceDayEnd": day.end}),
        );
    result["completionRecords"]["provisionalBreak"] = json!({
        "focusTimerId": trigger.timer_id, "finishCommandId": trigger.finish_command_id,
        "breakTimerId": command["timerId"], "startCommandId": command["id"],
        "selectedPhaseVersion": request.selection.generation});
    Ok(())
}

fn ownership(request: &Request, command: &Value) -> Value {
    use crate::workspace_intent::model::ReplicationMode;
    let kind = match (request.compatibility, &request.replication_mode) {
        (Compatibility::AppleWorkspace, ReplicationMode::Iroh) => "recordLocalStart",
        (Compatibility::AndroidCoordinator, _) => "setOwnedTimerId",
        (_, ReplicationMode::Centralized) => "recordStart",
        _ => return json!([]),
    };
    json!([{"kind": kind, "timerId": command["timerId"], "deviceId": request.allocation.device_id,
        "startCommandId": command["id"]}])
}
