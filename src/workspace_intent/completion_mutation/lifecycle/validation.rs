use std::collections::BTreeSet;

use super::{
    CoreError, invalid,
    model::{Event, Request, Stage, State},
};
use crate::workspace_intent::{
    allocation,
    model::{Compatibility, Input, ReplicationMode},
    policy,
};

pub(super) fn request(request: &Request, input: &Input) -> Result<(), CoreError> {
    supported(request)?;
    policy::validate_generation(input)?;
    allocation::validate(input)?;
    super::super::calendar_bounds(&request.calendar_intervals)?;
    for stamp in [
        &input.clock.occurred_at,
        &input.clock.physical_now,
        &input.clock.observed_at,
    ] {
        crate::timer::parse_time(stamp)?;
    }
    if crate::timer::parse_time(&input.clock.observed_at)?
        > crate::timer::parse_time(&input.clock.physical_now)?
    {
        return Err(invalid("observation is later than physical now"));
    }
    workspace(input, &input.workspace)?;
    crate::workspace_intent::validate_observation(input)?;
    if let Some(previous) = &request.previous_workspace {
        workspace(input, previous)?;
        let mut old_input = request.context();
        old_input.workspace = previous.clone();
        old_input.observation = request.previous_observation.clone().unwrap();
        crate::workspace_intent::validate_observation(&old_input)?;
    }
    if let Some(timer) = &request.requested_timer {
        crate::timer::validate_canonical_timer(timer)?;
    }
    if request
        .centralized_session
        .user_id
        .as_deref()
        .is_some_and(str::is_empty)
    {
        return Err(invalid("invalid centralized session identity"));
    }
    ownership(request)?;
    state(&request.lifecycle)?;
    reservations(request)?;
    event(&request.event)
}

fn supported(request: &Request) -> Result<(), CoreError> {
    if request.compatibility == Compatibility::PwaStorage
        || request.clock.monotonic_now_ms.is_some()
        || request.clock.continuity_id.is_some()
        || request.observation.monotonic_anchor.is_some()
        || request
            .previous_observation
            .as_ref()
            .is_some_and(|old| old.monotonic_anchor.is_some())
    {
        return Err(invalid("unsupported staged completion profile"));
    }
    match request.stage {
        Stage::ExpiryObservation => expiry_context(request),
        Stage::DeferredBreakOpportunity => deferred_context(request),
    }
}

fn expiry_context(request: &Request) -> Result<(), CoreError> {
    if request.replication_mode != ReplicationMode::Iroh
        || request.previous_workspace.is_none()
        || request.previous_observation.is_none()
        || !matches!(request.event, Event::Observation {})
    {
        return Err(invalid(
            "expiry observation requires raw Iroh old and new workspace",
        ));
    }
    Ok(())
}

fn deferred_context(request: &Request) -> Result<(), CoreError> {
    let iroh = request.replication_mode == ReplicationMode::Iroh;
    if request.previous_workspace.is_some() != iroh
        || request.previous_observation.is_some() != iroh
        || request.requested_timer.is_some()
        || matches!(request.event, Event::Observation {})
        || (iroh && !matches!(request.event, Event::Opportunity {}))
    {
        return Err(invalid("unsupported deferred break context"));
    }
    match (request.compatibility, &request.replication_mode) {
        (Compatibility::AppleWorkspace, ReplicationMode::Iroh)
        | (
            Compatibility::DesktopStorage | Compatibility::DesktopTerminal,
            ReplicationMode::Centralized,
        ) => Ok(()),
        _ => Err(invalid("unsupported deferred break context")),
    }
}

fn reservations(request: &Request) -> Result<(), CoreError> {
    for trigger in &request.lifecycle.pending_breaks {
        if let Some(uuid) = &trigger.reserved_timer_uuid {
            if request.replication_mode != ReplicationMode::Iroh {
                return Err(invalid("unexpected centralized break reservation"));
            }
            let mut input = request.context();
            input.identities.timer_uuid = Some(uuid.clone());
            allocation::validate(&input)?;
        }
    }
    Ok(())
}

fn workspace(input: &Input, workspace: &serde_json::Value) -> Result<(), CoreError> {
    if workspace.get("now").is_some() {
        return Err(invalid("workspace.now is owned by completion planner"));
    }
    super::project(input, workspace, &input.clock.occurred_at)?;
    Ok(())
}

fn ownership(request: &Request) -> Result<(), CoreError> {
    if request.ownership.as_ref().is_some_and(|owner| {
        owner.timer_id.is_empty()
            || owner.device_id.is_empty()
            || owner.tab_id.is_some()
            || owner.lease_expires_at_ms.is_some()
    }) {
        return Err(invalid("invalid staged completion ownership"));
    }
    Ok(())
}

pub(in crate::workspace_intent::completion_mutation) fn state(
    state: &State,
) -> Result<(), CoreError> {
    let mut completions = BTreeSet::new();
    for item in &state.consumed_completions {
        if item.timer_id.is_empty()
            || item.command_id.as_deref().is_some_and(str::is_empty)
            || !matches!(item.phase.as_str(), "focus" | "short_break" | "long_break")
            || !completions.insert((&item.timer_id, &item.command_id, &item.phase))
        {
            return Err(invalid("invalid consumed completion identity"));
        }
    }
    let mut commands = BTreeSet::new();
    let mut timers = BTreeSet::new();
    for trigger in &state.pending_breaks {
        if let Some(uuid) = &trigger.reserved_timer_uuid {
            allocation::validate_uuid(uuid, Some(b'4'))?;
        }
        if trigger.finish_command_id.is_empty()
            || trigger.timer_id.is_empty()
            || !(1..=9_007_199_254_740_991).contains(&trigger.finish_device_sequence)
            || !commands.insert(&trigger.finish_command_id)
            || !timers.insert(&trigger.timer_id)
        {
            return Err(invalid("invalid pending break trigger"));
        }
    }
    Ok(())
}

fn event(event: &Event) -> Result<(), CoreError> {
    let Event::CanonicalInstalled {
        acknowledgements,
        discarded_command_ids,
    } = event
    else {
        return Ok(());
    };
    let mut ids = BTreeSet::new();
    if acknowledgements
        .iter()
        .any(|ack| ack.command_id.is_empty() || !ids.insert(&ack.command_id))
    {
        return Err(invalid("invalid completion acknowledgements"));
    }
    ids.clear();
    if discarded_command_ids
        .iter()
        .any(|id| id.is_empty() || !ids.insert(id))
    {
        return Err(invalid("invalid discarded completion identities"));
    }
    Ok(())
}
