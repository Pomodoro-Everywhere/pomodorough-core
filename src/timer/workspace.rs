use super::{
    BTreeMap, CanonicalTimer, Command, CoreError, HistoryItem, Session, TimerReductionOutput,
    WireCommand, check_command_counts, check_unique_command_ids, clamp, parse_time,
    reduce_from_state, session_from_canonical, session_from_history, validate_canonical_timer,
    validate_history,
};

pub(crate) mod observation;

struct Prepared {
    commands: Vec<Command>,
    sessions: BTreeMap<String, Session>,
    current_id: Option<String>,
    devices: BTreeMap<String, String>,
}

pub(crate) fn replay(
    timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    commands: Vec<WireCommand>,
    now: &str,
) -> Result<TimerReductionOutput, CoreError> {
    let result = finish(prepare(timer, history, commands)?, now)?;
    // The raw workspace boundary must not return a timer/history aggregate that
    // its own next zero-command read rejects, including after deadline completion.
    validate_pair(result.canonical_timer.as_ref(), &result.history)?;
    Ok(result)
}

fn prepare(
    timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    commands: Vec<WireCommand>,
) -> Result<Prepared, CoreError> {
    check_command_counts(commands.len(), history.len())?;
    check_unique_command_ids(commands.iter().map(|command| command.id.as_str()))?;
    let matching = validate_pair(timer.as_ref(), &history)?;
    if let (Some(timer), Some(row)) = (&timer, matching) {
        validate_retained_provenance(timer, row, &commands)?;
    }
    let mut devices: BTreeMap<String, String> = commands
        .iter()
        .map(|command| (command.id.clone(), command.device_id.clone()))
        .collect();
    if let Some(intent) = timer.as_ref().and_then(|timer| timer.last_intent.as_ref()) {
        if let Some(device) = intent
            .native_extensions
            .get("deviceId")
            .and_then(serde_json::Value::as_str)
        {
            devices.insert(intent.command_id.clone(), device.to_owned());
        }
    }
    let commands = commands
        .into_iter()
        .map(WireCommand::into_command)
        .collect::<Result<Vec<_>, _>>()?;
    let current_id = timer.as_ref().map(|timer| timer.id.clone());
    let sessions = seed_sessions(timer, history)?;
    Ok(Prepared {
        commands,
        sessions,
        current_id,
        devices,
    })
}

fn finish(prepared: Prepared, now: &str) -> Result<TimerReductionOutput, CoreError> {
    let mut result = reduce_from_state(
        prepared.commands,
        parse_time(now)?,
        prepared.sessions,
        prepared.current_id,
    )?;
    restore_native_device(&mut result, &prepared.devices);
    Ok(result)
}

pub(crate) fn validate_native_device(value: &serde_json::Value) -> Result<(), CoreError> {
    if let Some(device) = value["base"]["canonicalTimer"]["lastIntent"].get("deviceId") {
        if !device.is_null() && !device.as_str().is_some_and(|device| !device.is_empty()) {
            return Err(CoreError::InvalidInput(
                "invalid native intent device identity".into(),
            ));
        }
    }
    Ok(())
}

fn restore_native_device(result: &mut TimerReductionOutput, devices: &BTreeMap<String, String>) {
    if let Some(intent) = result
        .canonical_timer
        .as_mut()
        .and_then(|timer| timer.last_intent.as_mut())
    {
        if intent.device_id.is_none() {
            intent.device_id = devices.get(&intent.command_id).cloned();
        }
    }
}

fn conflict() -> CoreError {
    CoreError::InvalidInput("conflicting workspace terminal timer/history".into())
}

fn validate_pair<'a>(
    timer: Option<&CanonicalTimer>,
    history: &'a [HistoryItem],
) -> Result<Option<&'a HistoryItem>, CoreError> {
    if let Some(timer) = timer {
        validate_canonical_timer(timer)?;
    }
    let (_, timer_ids) = validate_history(history)?;
    if history
        .iter()
        .any(|row| row.id != row.timer_id && timer_ids.contains(row.id.as_str()))
    {
        return Err(conflict());
    }
    let Some(timer) = timer else { return Ok(None) };
    // A history ID may name its own session, never a different canonical session.
    if history
        .iter()
        .any(|row| row.id == timer.id && row.timer_id != timer.id)
    {
        return Err(conflict());
    }
    let Some(row) = history.iter().find(|row| row.timer_id == timer.id) else {
        validate_standalone(timer)?;
        return Ok(None);
    };
    if row.status != timer.status
        || row.phase != timer.phase
        || row.task_id != timer.task_id
        || row.planned_duration_ms != timer.planned_duration_ms
        || !matches!(
            timer.status.as_str(),
            "completed" | "cancelled" | "superseded"
        )
    {
        return Err(conflict());
    }
    validate_terminal_time(timer, row)?;
    validate_terminal_intent(timer, row.command_id.as_deref())?;
    Ok(Some(row))
}

fn validate_standalone(timer: &CanonicalTimer) -> Result<(), CoreError> {
    if !matches!(
        timer.status.as_str(),
        "completed" | "cancelled" | "superseded"
    ) {
        return Ok(());
    }
    validate_terminal_elapsed(timer)?;
    let terminal_command = timer
        .last_intent
        .as_ref()
        .filter(|intent| matches!(intent.kind.as_str(), "finish" | "cancel"))
        .map(|intent| intent.command_id.as_str());
    validate_terminal_intent(timer, terminal_command)
}

fn validate_terminal_time(timer: &CanonicalTimer, row: &HistoryItem) -> Result<(), CoreError> {
    let anchor = parse_time(&timer.anchor_at)?;
    for stamp in [&row.completed_at, &row.ended_at].into_iter().flatten() {
        if parse_time(stamp)? != anchor {
            return Err(conflict());
        }
    }
    validate_terminal_elapsed(timer)?;
    if timer.status != "completed" && row.completed_at.is_some() {
        return Err(conflict());
    }
    if row.command_id.as_ref().is_some_and(String::is_empty) {
        return Err(conflict());
    }
    Ok(())
}

fn validate_terminal_elapsed(timer: &CanonicalTimer) -> Result<(), CoreError> {
    if timer.status == "completed" && timer.elapsed_at_anchor_ms != timer.planned_duration_ms {
        return Err(conflict());
    }
    Ok(())
}

fn validate_terminal_intent(
    timer: &CanonicalTimer,
    terminal_command: Option<&str>,
) -> Result<(), CoreError> {
    let Some(intent) = &timer.last_intent else {
        return if terminal_command.is_none() || timer.status == "superseded" {
            Ok(())
        } else {
            Err(conflict())
        };
    };
    let at = parse_time(&intent.occurred_at)?;
    let anchor = parse_time(&timer.anchor_at)?;
    let exact_command = terminal_command == Some(intent.command_id.as_str());
    let valid = match (timer.status.as_str(), intent.kind.as_str()) {
        ("completed", "finish") | ("cancelled", "cancel") => exact_command && at == anchor,
        // Deadline completion preserves the prior active intent, not a finish command.
        ("completed", "start" | "resume") => terminal_command.is_none() && at <= anchor,
        // Superseding another session changes terminal provenance but preserves its intent.
        ("superseded", "start" | "pause" | "resume") => !exact_command,
        _ => false,
    };
    if !valid {
        return Err(conflict());
    }
    Ok(())
}

fn validate_retained_provenance(
    timer: &CanonicalTimer,
    row: &HistoryItem,
    commands: &[WireCommand],
) -> Result<(), CoreError> {
    for command in commands {
        validate_prior_command(timer, command)?;
        if row.command_id.as_deref() == Some(command.id.as_str()) {
            validate_terminal_command(timer, command)?;
        }
    }
    Ok(())
}

fn validate_terminal_command(
    timer: &CanonicalTimer,
    command: &WireCommand,
) -> Result<(), CoreError> {
    if timer.status == "superseded" {
        return validate_superseding_command(timer, command);
    }
    let expected_kind = if timer.status == "completed" {
        "finish"
    } else {
        "cancel"
    };
    if command.kind != expected_kind
        || command.timer_id != timer.id
        || parse_time(&command.occurred_at)? != parse_time(&timer.anchor_at)?
        || (timer.status == "cancelled"
            && clamp(command.observed_elapsed_ms, 0, timer.planned_duration_ms)
                != timer.elapsed_at_anchor_ms)
    {
        return Err(conflict());
    }
    Ok(())
}

fn validate_superseding_command(
    timer: &CanonicalTimer,
    command: &WireCommand,
) -> Result<(), CoreError> {
    if command.timer_id == timer.id
        || !matches!(
            command.kind.as_str(),
            "start" | "pause" | "resume" | "finish" | "cancel"
        )
        || parse_time(&command.occurred_at)? != parse_time(&timer.anchor_at)?
    {
        return Err(conflict());
    }
    Ok(())
}

fn validate_prior_command(timer: &CanonicalTimer, command: &WireCommand) -> Result<(), CoreError> {
    let Some(intent) = &timer.last_intent else {
        return Ok(());
    };
    if intent.command_id == command.id
        && (intent.kind != command.kind
            || timer.id != command.timer_id
            || (command.kind == "start"
                && (timer.phase != command.phase
                    || timer.planned_duration_ms != command.planned_duration_ms))
            || parse_time(&intent.occurred_at)? != parse_time(&command.occurred_at)?)
    {
        return Err(conflict());
    }
    // A later retarget may change task attribution without changing lastIntent.
    Ok(())
}

fn seed_sessions(
    timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
) -> Result<BTreeMap<String, Session>, CoreError> {
    let mut sessions = BTreeMap::new();
    for row in history {
        let session = session_from_history(row)?;
        sessions.insert(session.timer_id.clone(), session);
    }
    if let Some(timer) = timer {
        let mut session = canonical_session(timer)?;
        if let Some(terminal) = sessions.get(&session.timer_id) {
            // The validated row owns history identity and terminal command. The
            // canonical object owns elapsed, starter and last lifecycle intent.
            session.history_id.clone_from(&terminal.history_id);
            session
                .terminal_command_id
                .clone_from(&terminal.terminal_command_id);
            session.ended_at = terminal.ended_at;
        }
        sessions.insert(session.timer_id.clone(), session);
    }
    Ok(sessions)
}

fn canonical_session(mut timer: CanonicalTimer) -> Result<Session, CoreError> {
    if let Some(intent) = &mut timer.last_intent {
        intent.device_id = intent
            .native_extensions
            .get("deviceId")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
    }
    let mut session = session_from_canonical(timer)?;
    if session.ended_at.is_some()
        && session
            .last_intent
            .as_ref()
            .is_some_and(|intent| matches!(intent.kind.as_str(), "start" | "pause" | "resume"))
    {
        // Prior active intents do not identify a terminal command.
        session.terminal_command_id = None;
    }
    Ok(session)
}

pub(crate) fn same_intent(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    // Native device metadata describes origin, not lifecycle identity.
    left.is_null() == right.is_null()
        && ["type", "commandId", "occurredAt"]
            .iter()
            .all(|field| left[*field] == right[*field])
}
