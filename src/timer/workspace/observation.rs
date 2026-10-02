use chrono::{DateTime, Utc};

use super::{
    BTreeMap, CanonicalTimer, CoreError, HistoryItem, Session, TimerReductionOutput, WireCommand,
    finish, parse_time, prepare,
};

pub(crate) struct Observation<'a> {
    pub canonical_anchor_at: Option<&'a str>,
    pub command_times: &'a BTreeMap<String, String>,
}

impl Observation<'_> {
    pub(crate) fn canonical_anchor(
        &self,
        timer: &CanonicalTimer,
    ) -> Result<DateTime<Utc>, CoreError> {
        if let Some(anchor) = self.canonical_anchor_at {
            return parse_time(anchor);
        }
        let anchor = parse_time(&timer.anchor_at)?;
        let Some(intent) = &timer.last_intent else {
            return Ok(anchor);
        };
        let Some(physical) = self.command_times.get(&intent.command_id) else {
            return Ok(anchor);
        };
        let offset = parse_time(physical)? - parse_time(&intent.occurred_at)?;
        anchor
            .checked_add_signed(offset)
            .ok_or_else(|| CoreError::InvalidInput("physical timer anchor out of range".into()))
    }
}

pub(crate) fn replay(
    timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    commands: Vec<WireCommand>,
    now: &str,
    observation: &Observation<'_>,
) -> Result<TimerReductionOutput, CoreError> {
    // Prepare validates the unmodified wire pair and commands before observations
    // enter the private replay state. Saved history is never used as an overlay.
    let mut prepared = prepare(timer.clone(), history, commands)?;
    let anchor = timer
        .as_ref()
        .map(|timer| observation.canonical_anchor(timer))
        .transpose()?;
    let wire_session = prepared
        .current_id
        .as_ref()
        .and_then(|id| prepared.sessions.get(id))
        .cloned();
    if let (Some(id), Some(anchor)) = (&prepared.current_id, anchor) {
        prepared
            .sessions
            .get_mut(id)
            .expect("seeded current session")
            .anchor_at = anchor;
    }
    for command in &mut prepared.commands {
        if let Some(at) = observation.command_times.get(&command.id) {
            command.occurred_at = parse_time(at)?;
        }
    }
    let mut result = finish(prepared, now)?;
    if let Some(session) = wire_session {
        preserve_terminal_provenance(&mut result, &session);
    }
    Ok(result)
}

fn preserve_terminal_provenance(result: &mut TimerReductionOutput, wire: &Session) {
    if wire.ended_at.is_none() {
        return;
    }
    for row in &mut result.history {
        if row.id == wire.history_id
            && row.timer_id == wire.timer_id
            && row.command_id == wire.terminal_command_id
            && row.status == wire.status
            && row.task_id == wire.task_id
            && row.phase == wire.phase
            && row.planned_duration_ms == wire.planned_duration_ms
        {
            let stamp = wire.ended_at.as_ref().map(crate::timer::format_time);
            row.ended_at.clone_from(&stamp);
            row.completed_at = (wire.status == "completed").then_some(stamp).flatten();
        }
    }
    let Some(timer) = result
        .canonical_timer
        .as_mut()
        .filter(|timer| timer.id == wire.timer_id && timer.status == wire.status)
    else {
        return;
    };
    if timer
        .last_intent
        .as_ref()
        .map(|intent| (&intent.command_id, &intent.kind))
        == wire
            .last_intent
            .as_ref()
            .map(|intent| (&intent.command_id, &intent.kind))
    {
        let device = timer
            .last_intent
            .as_ref()
            .and_then(|intent| intent.device_id.clone());
        timer.last_intent.clone_from(&wire.last_intent);
        if let Some(intent) = &mut timer.last_intent {
            if intent.device_id.is_none() {
                intent.device_id = device;
            }
        }
    }
}
