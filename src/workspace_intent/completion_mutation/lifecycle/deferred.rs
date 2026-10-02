use serde_json::{Value, json};

use super::{
    CoreError, invalid,
    model::{Event, Outcome, Request, Trigger},
    output,
};
use crate::workspace_intent::model::{Compatibility, Input, ReplicationMode};

pub(super) fn plan(request: &Request, input: &Input) -> Result<Value, CoreError> {
    let mut result = output::empty(request, input, "noPendingBreak")?;
    let mut state = request.lifecycle.clone();
    let mut retired = vec![];
    while let Some(trigger) = state.pending_breaks.first().cloned() {
        validate_retained_source(request, &trigger)?;
        if rejected(request, &trigger) || later_command(request, &trigger) {
            state.pending_breaks.remove(0);
            retired.push(trigger.finish_command_id);
            result["reason"] = json!("triggerDropped");
            continue;
        }
        let pending = source_pending(request, &trigger);
        let canonical = canonical(request);
        let accepted = !pending && exact_source(&canonical, &trigger).is_some();
        result["sourceStatus"] = json!(if accepted { "accepted" } else { "pending" });
        if barrier(request, pending) {
            result["reason"] = json!("canonicalBarrier");
            break;
        }
        let selected = if request.canonical_barrier() || accepted {
            &canonical
        } else {
            &result["projection"]
        };
        let Some(source) = exact_source(selected, &trigger).cloned() else {
            result["reason"] = json!("waitingForSource");
            break;
        };
        let phase = super::phase(request, selected, &source)?;
        state.pending_breaks.remove(0);
        retired.push(trigger.finish_command_id.clone());
        result["source"] = source.clone();
        materialize(request, input, &trigger, &source, &phase, &mut result)?;
        break;
    }
    output::lifecycle(&mut result, &state, &retired)?;
    Ok(result)
}

fn materialize(
    request: &Request,
    input: &Input,
    trigger: &Trigger,
    source: &Value,
    phase: &str,
    result: &mut Value,
) -> Result<(), CoreError> {
    result["nextPhase"] = json!(phase);
    if let Some((saved_phase, duration)) = saved_break(request, input, trigger, source)? {
        result["nextPhase"] = json!(saved_phase);
        result["projection"]["durationsMs"][&saved_phase] = json!(duration);
        return output::start(request, input, &saved_phase, None, source, result);
    }
    let provisional = request.replication_mode == ReplicationMode::Centralized
        && result["sourceStatus"] != "accepted";
    output::start(
        request,
        input,
        phase,
        provisional.then_some(trigger),
        source,
        result,
    )
}

fn saved_break(
    request: &Request,
    input: &Input,
    trigger: &Trigger,
    source: &Value,
) -> Result<Option<(String, i64)>, CoreError> {
    if request.replication_mode == ReplicationMode::Centralized {
        if trigger.reserved_timer_uuid.is_some() {
            return Err(invalid("unexpected centralized break reservation"));
        }
        return Ok(None);
    }
    if trigger.reserved_timer_uuid.is_none()
        || trigger.reserved_timer_uuid != input.identities.timer_uuid
    {
        return Err(invalid("Iroh continuation changed reserved timer entropy"));
    }
    let previous = request.previous_workspace.as_ref().unwrap();
    let saved = super::observe(
        input,
        previous,
        request.previous_observation.as_ref().unwrap(),
        &input.clock.observed_at,
    )?["workspace"]
        .clone();
    let row = exact_source(&saved, trigger)
        .ok_or_else(|| invalid("Iroh continuation lacks committed finish source"))?;
    if row != source {
        return Err(invalid("Iroh continuation changed finish provenance"));
    }
    let phase = super::phase(request, &saved, row)?;
    let duration = saved["durationsMs"][&phase].as_i64().unwrap();
    Ok(Some((phase, duration)))
}

fn source_pending(request: &Request, trigger: &Trigger) -> bool {
    request.workspace["local"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["id"] == trigger.finish_command_id)
}

fn barrier(request: &Request, pending: bool) -> bool {
    request.canonical_barrier()
        && (pending
            || !request.workspace["local"]["autoStartOperations"]
                .as_array()
                .unwrap()
                .is_empty())
}

fn canonical(request: &Request) -> Value {
    // Canonical acceptance uses the installed raw state, not a re-projection
    // that evicts a terminal timer already represented in its history.
    request.workspace["base"].clone()
}

fn exact_source<'a>(workspace: &'a Value, trigger: &Trigger) -> Option<&'a Value> {
    let timer = &workspace["canonicalTimer"];
    if timer["id"] != trigger.timer_id
        || timer["phase"] != "focus"
        || timer["status"] != "completed"
    {
        return None;
    }
    workspace["history"].as_array().unwrap().iter().find(|row| {
        row["timerId"] == trigger.timer_id
            && row["commandId"] == trigger.finish_command_id
            && row["phase"] == "focus"
            && row["status"] == "completed"
            && row["completedAt"].as_str().is_some()
    })
}

fn later_command(request: &Request, trigger: &Trigger) -> bool {
    request.workspace["local"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| {
            command["deviceId"] == request.allocation.device_id
                && command["deviceSequence"].as_i64().unwrap() > trigger.finish_device_sequence
                && !(command["type"] == "finish" && command["timerId"] == trigger.timer_id)
        })
}

fn rejected(request: &Request, trigger: &Trigger) -> bool {
    let Event::CanonicalInstalled {
        acknowledgements,
        discarded_command_ids,
    } = &request.event
    else {
        return false;
    };
    if discarded_command_ids.contains(&trigger.finish_command_id) {
        return true;
    }
    acknowledgements
        .iter()
        .find(|ack| ack.command_id == trigger.finish_command_id)
        .is_some_and(|ack| {
            matches!(ack.outcome, Outcome::Rejected)
                || exact_source(&canonical(request), trigger).is_none()
        })
}

fn validate_retained_source(request: &Request, trigger: &Trigger) -> Result<(), CoreError> {
    let commands = request.workspace["local"]["commands"].as_array().unwrap();
    let Some(command) = commands
        .iter()
        .find(|command| command["id"] == trigger.finish_command_id)
    else {
        return Ok(());
    };
    if command["type"] != "finish"
        || command["phase"] != "focus"
        || command["timerId"] != trigger.timer_id
        || command["deviceId"] != request.allocation.device_id
        || command["deviceSequence"] != trigger.finish_device_sequence
    {
        return Err(invalid("pending break trigger mismatches retained finish"));
    }
    if request.compatibility == Compatibility::AndroidCoordinator {
        return Err(invalid("Android deferred finish is already atomic"));
    }
    Ok(())
}
