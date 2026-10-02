use serde_json::{Value, json};

use super::{
    CoreError,
    model::{Completion, Request},
    output,
};
use crate::workspace_intent::model::{Compatibility, Input};

pub(super) fn plan(request: &Request, input: &Input) -> Result<Value, CoreError> {
    let mut result = output::empty(request, input, "notExpired")?;
    let previous = request.previous_workspace.as_ref().unwrap();
    let observation = request.previous_observation.as_ref().unwrap();
    let before =
        super::observe(input, previous, observation, "1970-01-01T00:00:00Z")?["workspace"].clone();
    let before_timer = &before["canonicalTimer"];
    if stale(request, before_timer)? {
        result["reason"] = json!("staleTimer");
        return Ok(result);
    }
    let expected =
        super::observe(input, previous, observation, &input.clock.observed_at)?["workspace"]
            .clone();
    let timer = &result["projection"]["canonicalTimer"];
    if before_timer["status"] != "running" || timer["status"] != "completed" {
        return Ok(result);
    }
    if expected["canonicalTimer"] != *timer || timer["lastIntent"]["type"] == "finish" {
        result["reason"] = json!("staleTimer");
        return Ok(result);
    }
    let Some(source) = source(&result["projection"], timer) else {
        return Ok(result);
    };
    let completion = Completion::from_row(source);
    if consumed(request, &completion, &before) {
        result["reason"] = json!("alreadyConsumed");
        return Ok(result);
    }
    consume(request, input, source.clone(), completion, &mut result)?;
    Ok(result)
}

fn stale(request: &Request, timer: &Value) -> Result<bool, CoreError> {
    let Some(presented) = &request.requested_timer else {
        return Ok(false);
    };
    let presented = serde_json::to_value(presented)?;
    Ok([
        "id",
        "status",
        "phase",
        "plannedDurationMs",
        "anchorAt",
        "elapsedAtAnchorMs",
        "taskId",
    ]
    .iter()
    .any(|key| presented[*key] != timer[*key])
        || !crate::timer::workspace::same_intent(&presented["lastIntent"], &timer["lastIntent"]))
}

fn source<'a>(workspace: &'a Value, timer: &Value) -> Option<&'a Value> {
    workspace["history"].as_array().unwrap().iter().find(|row| {
        row["timerId"] == timer["id"]
            && row["phase"] == timer["phase"]
            && row["status"] == "completed"
            && row["completedAt"].as_str().is_some()
    })
}

fn consumed(request: &Request, completion: &Completion, before: &Value) -> bool {
    request.lifecycle.consumed_completions.contains(completion)
        || before["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["status"] == "completed" && Completion::from_row(row) == *completion)
}

fn consume(
    request: &Request,
    input: &Input,
    source: Value,
    completion: Completion,
    result: &mut Value,
) -> Result<(), CoreError> {
    let phase = super::phase(request, &result["projection"], &source)?;
    let generated = source["phase"] == "focus"
        && result["projection"]["autoStartBreaks"] == true
        && owns(request, &result["projection"]["canonicalTimer"]);
    if generated
        && request.compatibility == Compatibility::AppleWorkspace
        && crate::timer::parse_time(&input.clock.occurred_at)?
            < crate::timer::parse_time(source["completedAt"].as_str().unwrap())?
    {
        result["reason"] = json!("startBeforeCompletion");
        return Ok(());
    }
    let mut state = request.lifecycle.clone();
    state.consumed_completions.push(completion);
    result["source"] = source.clone();
    result["sourceStatus"] = json!("naturalExpiry");
    result["nextPhase"] = json!(phase);
    result["reason"] = json!("");
    select(request, &source, &phase, result)?;
    output::lifecycle(result, &state, &[])?;
    if generated {
        output::start(request, input, &phase, None, &source, result)?;
    }
    Ok(())
}

fn select(
    request: &Request,
    source: &Value,
    phase: &str,
    result: &mut Value,
) -> Result<(), CoreError> {
    if request.compatibility == Compatibility::AndroidCoordinator
        || request.selection.phase.name() != source["phase"]
        || (request.compatibility == Compatibility::AppleWorkspace && request.selection.explicit)
    {
        return Ok(());
    }
    result["selection"]["phase"] = json!(phase);
    Ok(())
}

fn owns(request: &Request, timer: &Value) -> bool {
    let local = &request.allocation.device_id;
    match request.compatibility {
        Compatibility::AndroidCoordinator => request
            .ownership
            .as_ref()
            .is_some_and(|owner| owner.timer_id == timer["id"] && owner.device_id == *local),
        Compatibility::AppleWorkspace => request
            .ownership
            .as_ref()
            .map_or(timer["startedByDeviceId"] == *local, |owner| {
                owner.timer_id == timer["id"] && owner.device_id == *local
            }),
        Compatibility::DesktopStorage | Compatibility::DesktopTerminal => {
            timer["startedByDeviceId"] == *local
        }
        Compatibility::PwaStorage => false,
    }
}
