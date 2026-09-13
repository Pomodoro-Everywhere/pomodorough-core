use super::*;

const PAGE_SIZE: usize = 256;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
struct Order {
    hlc_wall_ms: i64,
    hlc_counter: i64,
    device_id: String,
    id: String,
}

impl From<&Command> for Order {
    fn from(command: &Command) -> Self {
        Self {
            hlc_wall_ms: command.hlc_wall_ms,
            hlc_counter: command.hlc_counter,
            device_id: command.device_id.clone(),
            id: command.id.clone(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    commands: Vec<WireCommand>,
    sessions: Vec<WireSession>,
    current_timer_id: Option<String>,
    after: Option<Order>,
    now: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    #[serde(flatten)]
    projection: TimerReductionOutput,
    current_timer_id: Option<String>,
    after: Option<Order>,
}

// Replay the full sorted log; the host retains untouched sessions verbatim.
// Never feed projected history back here: it omits resumable session state.
// C31: unknown fields stay tolerated (like `timer.reduce.v1` and the C27 ack
// precedent) so a newer host can extend commands or persist a richer `after`
// cursor without breaking paged replay. `PAGE_SIZE`/ordering checks remain.
pub(crate) fn reduce_json(input: &str) -> Result<String, CoreError> {
    let input: Input = serde_json::from_value(crate::strict_json::parse(input)?)?;
    if input.commands.len() > PAGE_SIZE || input.sessions.len() > PAGE_SIZE + 1 {
        return Err(CoreError::InvalidInput(
            "timer replay page exceeds limit".into(),
        ));
    }
    super::check_unique_command_ids(input.commands.iter().map(|command| command.id.as_str()))?;
    let mut state = restore(input.sessions, input.current_timer_id)?;
    let mut after = input.after;
    for wire in input.commands {
        let command = wire.into_command()?;
        let order = Order::from(&command);
        if after.as_ref().is_some_and(|previous| previous >= &order) {
            return Err(CoreError::InvalidInput(
                "timer replay page is not ordered".into(),
            ));
        }
        after = Some(order);
        state.apply(command);
    }
    if let Some(now) = input.now {
        state.auto_complete_current(&parse_time(&now)?);
    }
    Ok(serde_json::to_string(&Output {
        projection: state.project()?,
        current_timer_id: state.current_id,
        after,
    })?)
}

fn restore(
    sessions: Vec<WireSession>,
    current_id: Option<String>,
) -> Result<ReductionState, CoreError> {
    let mut restored = BTreeMap::new();
    for wire in sessions {
        let session = restore_session(wire)?;
        if restored.insert(session.timer_id.clone(), session).is_some() {
            return Err(CoreError::InvalidInput("duplicate replay session".into()));
        }
    }
    if current_id
        .as_ref()
        .is_some_and(|id| !restored.contains_key(id))
    {
        return Err(CoreError::InvalidInput(
            "missing current replay session".into(),
        ));
    }
    Ok(ReductionState {
        sessions: restored,
        current_id,
        outcomes: BTreeMap::new(),
    })
}

fn restore_session(wire: WireSession) -> Result<Session, CoreError> {
    let session = Session {
        history_id: wire.timer_id.clone(),
        timer_id: wire.timer_id,
        task_id: wire.task_id,
        phase: wire.phase,
        status: wire.status,
        planned_duration_ms: wire.planned_duration_ms,
        elapsed_at_anchor_ms: wire.elapsed_at_anchor_ms,
        anchor_at: parse_time(&wire.anchor_at)?,
        started_at: parse_time(&wire.started_at)?,
        started_by_device_id: wire.started_by_device_id,
        ended_at: wire.ended_at.as_deref().map(parse_time).transpose()?,
        last_command_id: wire.last_command_id,
        terminal_command_id: wire.terminal_command_id,
        superseded_by_timer_id: wire.superseded_by_timer_id,
        last_intent: wire.last_intent,
    };
    validate_canonical_timer(&canonical(&session))?;
    let terminal = !is_active(&session);
    if terminal != session.ended_at.is_some() {
        return Err(CoreError::InvalidInput(
            "invalid replay session metadata".into(),
        ));
    }
    // Empty last_command_id means "no intent": session_from_canonical and
    // session_from_history emit "" when the source carries no commandId, so
    // Core-produced full-replay sessions must restore. No agreement with
    // last_intent is required: supersede and retarget advance last_command_id
    // while deliberately preserving the lifecycle intent for older readers.
    Ok(session)
}
