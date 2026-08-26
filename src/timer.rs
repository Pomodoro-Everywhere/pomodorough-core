use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::CoreError;

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimerReductionInput {
    #[serde(default)]
    commands: Vec<WireCommand>,
    #[serde(default)]
    canonical_timer: Option<CanonicalTimer>,
    #[serde(default)]
    history: Vec<HistoryItem>,
    now: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireCommand {
    pub(crate) id: String,
    pub(crate) device_id: String,
    pub(crate) device_sequence: i64,
    pub(crate) timer_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task_id: Option<String>,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) phase: String,
    pub(crate) planned_duration_ms: i64,
    pub(crate) occurred_at: String,
    pub(crate) hlc_wall_ms: i64,
    pub(crate) hlc_counter: i64,
    pub(crate) observed_elapsed_ms: i64,
}

impl WireCommand {
    fn into_command(self) -> Result<Command, CoreError> {
        if self.id.is_empty()
            || self.device_id.is_empty()
            || self.timer_id.is_empty()
            || !(1..=MAX_SAFE_INTEGER).contains(&self.device_sequence)
            || !matches!(self.phase.as_str(), "focus" | "short_break" | "long_break")
            || !(60_000..=14_400_000).contains(&self.planned_duration_ms)
            || !(1..=MAX_SAFE_INTEGER).contains(&self.hlc_wall_ms)
            || !(0..=MAX_SAFE_INTEGER).contains(&self.hlc_counter)
            || !(-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&self.observed_elapsed_ms)
        {
            return Err(CoreError::InvalidInput("invalid timer command".into()));
        }
        Ok(Command {
            id: self.id,
            device_id: self.device_id,
            timer_id: self.timer_id,
            task_id: self.task_id,
            kind: self.kind,
            phase: self.phase,
            planned_duration_ms: self.planned_duration_ms,
            occurred_at: parse_time(&self.occurred_at)?,
            hlc_wall_ms: self.hlc_wall_ms,
            hlc_counter: self.hlc_counter,
            observed_elapsed_ms: self.observed_elapsed_ms,
        })
    }
}

#[derive(Clone, Debug)]
struct Command {
    id: String,
    device_id: String,
    timer_id: String,
    task_id: Option<String>,
    kind: String,
    phase: String,
    planned_duration_ms: i64,
    occurred_at: DateTime<Utc>,
    hlc_wall_ms: i64,
    hlc_counter: i64,
    observed_elapsed_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Intent {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) command_id: String,
    pub(crate) occurred_at: String,
}

#[derive(Clone, Debug)]
struct Session {
    history_id: String,
    timer_id: String,
    task_id: Option<String>,
    phase: String,
    status: String,
    planned_duration_ms: i64,
    elapsed_at_anchor_ms: i64,
    anchor_at: DateTime<Utc>,
    started_at: DateTime<Utc>,
    started_by_device_id: String,
    ended_at: Option<DateTime<Utc>>,
    last_command_id: String,
    terminal_command_id: Option<String>,
    superseded_by_timer_id: Option<String>,
    last_intent: Option<Intent>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub(crate) outcome: String,
    pub(crate) reason: String,
}

impl Outcome {
    fn applied() -> Self {
        Self {
            outcome: "applied".into(),
            reason: String::new(),
        }
    }

    fn ignored(reason: &str) -> Self {
        Self {
            outcome: "ignored".into(),
            reason: reason.into(),
        }
    }

    fn rejected(reason: &str) -> Self {
        Self {
            outcome: "rejected".into(),
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TimerReductionOutput {
    pub(crate) canonical_timer: Option<CanonicalTimer>,
    pub(crate) history: Vec<HistoryItem>,
    sessions: Vec<WireSession>,
    pub(crate) outcomes: BTreeMap<String, Outcome>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CanonicalTimer {
    pub(crate) id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) task_id: Option<String>,
    pub(crate) phase: String,
    pub(crate) status: String,
    pub(crate) planned_duration_ms: i64,
    pub(crate) elapsed_at_anchor_ms: i64,
    pub(crate) anchor_at: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) started_by_device_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_intent: Option<Intent>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryItem {
    pub(crate) id: String,
    pub(crate) timer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) command_id: Option<String>,
    pub(crate) phase: String,
    pub(crate) status: String,
    pub(crate) planned_duration_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ended_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireSession {
    timer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
    phase: String,
    status: String,
    planned_duration_ms: i64,
    elapsed_at_anchor_ms: i64,
    anchor_at: String,
    started_at: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    started_by_device_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ended_at: Option<String>,
    last_command_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal_command_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    superseded_by_timer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_intent: Option<Intent>,
}

pub(crate) fn reduce_timer_v1_json(input: &str) -> Result<String, CoreError> {
    let input: TimerReductionInput = serde_json::from_str(input)?;
    validate_replay_state(&input.canonical_timer, &input.history)?;
    let now = parse_time(&input.now)?;
    let commands = input
        .commands
        .into_iter()
        .map(WireCommand::into_command)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(serde_json::to_string(&replay_parsed(
        input.canonical_timer,
        input.history,
        commands,
        now,
    )?)?)
}

fn reduce(commands: Vec<Command>, now: DateTime<Utc>) -> TimerReductionOutput {
    reduce_from_state(commands, now, BTreeMap::new(), None)
}

pub(crate) fn replay(
    canonical_timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    commands: Vec<WireCommand>,
    now: &str,
) -> Result<TimerReductionOutput, CoreError> {
    validate_replay_state(&canonical_timer, &history)?;
    let now = parse_time(now)?;
    let commands = commands
        .into_iter()
        .map(WireCommand::into_command)
        .collect::<Result<Vec<_>, _>>()?;
    replay_parsed(canonical_timer, history, commands, now)
}

pub(crate) fn validate_replay_state(
    canonical_timer: &Option<CanonicalTimer>,
    history: &[HistoryItem],
) -> Result<(), CoreError> {
    if let Some(timer) = canonical_timer {
        validate_canonical_timer(timer)?;
    }
    let timer_ids = validate_history(history)?;
    if canonical_timer
        .as_ref()
        .is_some_and(|timer| timer_ids.contains(timer.id.as_str()))
    {
        return Err(CoreError::InvalidInput(
            "canonical timer overlaps timer history".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_canonical_timer(timer: &CanonicalTimer) -> Result<(), CoreError> {
    if timer.id.is_empty()
        || !matches!(timer.phase.as_str(), "focus" | "short_break" | "long_break")
        || !matches!(
            timer.status.as_str(),
            "running" | "paused" | "completed" | "cancelled" | "superseded"
        )
        || !(60_000..=14_400_000).contains(&timer.planned_duration_ms)
        || !(0..=timer.planned_duration_ms).contains(&timer.elapsed_at_anchor_ms)
        || parse_time(&timer.anchor_at).is_err()
    {
        return Err(CoreError::InvalidInput("invalid canonical timer".into()));
    }
    if timer.last_intent.as_ref().is_some_and(|intent| {
        intent.kind.is_empty()
            || intent.command_id.is_empty()
            || parse_time(&intent.occurred_at).is_err()
    }) {
        return Err(CoreError::InvalidInput(
            "invalid canonical timer intent".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_history(history: &[HistoryItem]) -> Result<BTreeSet<&str>, CoreError> {
    let mut history_ids = BTreeSet::new();
    let mut timer_ids = BTreeSet::new();
    for item in history {
        let completed_at_valid = item
            .completed_at
            .as_deref()
            .is_some_and(|value| parse_time(value).is_ok());
        let ended_at_valid = item
            .ended_at
            .as_deref()
            .is_some_and(|value| parse_time(value).is_ok());
        if item.id.is_empty()
            || item.timer_id.is_empty()
            || !history_ids.insert(item.id.as_str())
            || !timer_ids.insert(item.timer_id.as_str())
            || !matches!(item.phase.as_str(), "focus" | "short_break" | "long_break")
            || !matches!(
                item.status.as_str(),
                "completed" | "cancelled" | "superseded"
            )
            || !(60_000..=14_400_000).contains(&item.planned_duration_ms)
            || (item.status == "completed" && !completed_at_valid)
            || (item.status != "completed" && !ended_at_valid)
            || (item.completed_at.is_some() && !completed_at_valid)
            || (item.ended_at.is_some() && !ended_at_valid)
        {
            return Err(CoreError::InvalidInput("invalid timer history".into()));
        }
    }
    Ok(timer_ids)
}

fn replay_parsed(
    canonical_timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    commands: Vec<Command>,
    now: DateTime<Utc>,
) -> Result<TimerReductionOutput, CoreError> {
    let mut sessions = BTreeMap::new();
    for item in history {
        let session = session_from_history(item)?;
        sessions.insert(session.timer_id.clone(), session);
    }
    let current_id = canonical_timer.as_ref().map(|timer| timer.id.clone());
    if let Some(timer) = canonical_timer {
        let session = session_from_canonical(timer)?;
        sessions.insert(session.timer_id.clone(), session);
    }
    Ok(reduce_from_state(commands, now, sessions, current_id))
}

fn reduce_from_state(
    commands: Vec<Command>,
    now: DateTime<Utc>,
    sessions: BTreeMap<String, Session>,
    current_id: Option<String>,
) -> TimerReductionOutput {
    let mut reduction = ReductionState {
        sessions,
        current_id,
        outcomes: BTreeMap::new(),
    };
    for command in sorted_commands(commands) {
        reduction.apply(command);
    }
    reduction.finish(now)
}

struct ReductionState {
    sessions: BTreeMap<String, Session>,
    current_id: Option<String>,
    outcomes: BTreeMap<String, Outcome>,
}

impl ReductionState {
    fn apply(&mut self, command: Command) {
        self.auto_complete_current(&command.occurred_at);
        let intent = command_intent(&command);
        match command.kind.as_str() {
            "start" => self.apply_start(command, intent),
            "pause" => self.apply_activation(command, intent, "paused"),
            "resume" => self.apply_activation(command, intent, "running"),
            "finish" | "cancel" => self.apply_terminal(command, intent),
            "clear" => self.apply_clear(command, intent),
            _ => {
                self.outcomes
                    .insert(command.id, Outcome::rejected("unsupported command type"));
            }
        }
    }

    fn apply_start(&mut self, command: Command, intent: Intent) {
        self.supersede_active(&command);
        let timer_id = command.timer_id.clone();
        let command_id = command.id.clone();
        self.sessions
            .insert(timer_id.clone(), started_session(command, intent));
        self.current_id = Some(timer_id);
        self.outcomes.insert(command_id, Outcome::applied());
    }

    fn apply_activation(&mut self, command: Command, intent: Intent, status: &str) {
        if !self.sessions.contains_key(&command.timer_id) {
            let reason = if status == "paused" {
                "timer is not the active running timer"
            } else {
                "timer cannot be resumed"
            };
            self.outcomes.insert(command.id, Outcome::ignored(reason));
            return;
        }
        self.supersede_active(&command);
        let target = self
            .sessions
            .get_mut(&command.timer_id)
            .expect("checked above");
        target.status = status.into();
        target.elapsed_at_anchor_ms =
            clamp(command.observed_elapsed_ms, 0, target.planned_duration_ms);
        transition_session(target, &command, intent, SessionTransition::Active);
        self.current_id = Some(target.timer_id.clone());
        self.outcomes.insert(command.id, Outcome::applied());
    }

    fn apply_terminal(&mut self, command: Command, intent: Intent) {
        if !self.sessions.contains_key(&command.timer_id) {
            self.outcomes
                .insert(command.id, Outcome::ignored("timer is not active"));
            return;
        }
        self.supersede_active(&command);
        let target = self
            .sessions
            .get_mut(&command.timer_id)
            .expect("checked above");
        if command.kind == "finish" {
            target.status = "completed".into();
            target.elapsed_at_anchor_ms = target.planned_duration_ms;
        } else {
            target.status = "cancelled".into();
            target.elapsed_at_anchor_ms =
                clamp(command.observed_elapsed_ms, 0, target.planned_duration_ms);
        }
        transition_session(target, &command, intent, SessionTransition::Terminal);
        self.current_id = Some(target.timer_id.clone());
        self.outcomes.insert(command.id, Outcome::applied());
    }

    fn apply_clear(&mut self, command: Command, intent: Intent) {
        let Some(target) = self.sessions.get_mut(&command.timer_id) else {
            self.outcomes
                .insert(command.id, Outcome::ignored("timer cannot be cleared"));
            return;
        };
        target.last_command_id = command.id.clone();
        target.last_intent = Some(intent);
        if self.current_id.as_deref() == Some(command.timer_id.as_str()) {
            self.current_id = None;
        }
        self.outcomes.insert(command.id, Outcome::applied());
    }

    fn supersede_active(&mut self, command: &Command) {
        if let Some(current) = self
            .current_id
            .as_ref()
            .and_then(|id| self.sessions.get_mut(id))
            .filter(|current| current.timer_id != command.timer_id && is_active(current))
        {
            supersede(
                current,
                &command.occurred_at,
                &command.timer_id,
                &command.id,
            );
        }
    }

    fn auto_complete_current(&mut self, at: &DateTime<Utc>) {
        if let Some(current) = self
            .current_id
            .as_ref()
            .and_then(|id| self.sessions.get_mut(id))
        {
            auto_complete(current, at);
        }
    }

    fn finish(mut self, now: DateTime<Utc>) -> TimerReductionOutput {
        self.auto_complete_current(&now);
        let canonical_timer = self
            .current_id
            .as_ref()
            .and_then(|id| self.sessions.get(id))
            .map(canonical);
        let history = projected_history(&self.sessions);
        let sessions = self.sessions.values().map(wire_session).collect();
        TimerReductionOutput {
            canonical_timer,
            history,
            sessions,
            outcomes: self.outcomes,
        }
    }
}

fn sorted_commands(mut commands: Vec<Command>) -> Vec<Command> {
    commands.sort_by(|left, right| {
        (
            left.hlc_wall_ms,
            left.hlc_counter,
            left.device_id.as_str(),
            left.id.as_str(),
        )
            .cmp(&(
                right.hlc_wall_ms,
                right.hlc_counter,
                right.device_id.as_str(),
                right.id.as_str(),
            ))
    });
    commands
}

fn command_intent(command: &Command) -> Intent {
    Intent {
        kind: command.kind.clone(),
        command_id: command.id.clone(),
        occurred_at: format_time(&command.occurred_at),
    }
}

fn started_session(command: Command, intent: Intent) -> Session {
    Session {
        history_id: command.timer_id.clone(),
        timer_id: command.timer_id,
        task_id: command.task_id,
        phase: command.phase,
        status: "running".into(),
        planned_duration_ms: command.planned_duration_ms,
        elapsed_at_anchor_ms: 0,
        anchor_at: command.occurred_at,
        started_at: command.occurred_at,
        started_by_device_id: command.device_id,
        ended_at: None,
        last_command_id: command.id,
        terminal_command_id: None,
        superseded_by_timer_id: None,
        last_intent: Some(intent),
    }
}

enum SessionTransition {
    Active,
    Terminal,
}

fn transition_session(
    target: &mut Session,
    command: &Command,
    intent: Intent,
    transition: SessionTransition,
) {
    target.anchor_at = command.occurred_at;
    let terminal = matches!(transition, SessionTransition::Terminal);
    target.ended_at = terminal.then_some(command.occurred_at);
    target.last_command_id = command.id.clone();
    target.terminal_command_id = terminal.then(|| command.id.clone());
    target.superseded_by_timer_id = None;
    target.last_intent = Some(intent);
}

fn projected_history(sessions: &BTreeMap<String, Session>) -> Vec<HistoryItem> {
    let mut terminal = sessions
        .values()
        .filter(|session| {
            matches!(
                session.status.as_str(),
                "completed" | "cancelled" | "superseded"
            )
        })
        .collect::<Vec<_>>();
    terminal.sort_by(|left, right| {
        right
            .ended_at
            .cmp(&left.ended_at)
            .then_with(|| left.timer_id.cmp(&right.timer_id))
    });
    terminal.into_iter().map(history_item).collect()
}

fn history_item(session: &Session) -> HistoryItem {
    let ended_at = session.ended_at.as_ref().map(format_time);
    HistoryItem {
        id: session.history_id.clone(),
        timer_id: session.timer_id.clone(),
        task_id: session.task_id.clone(),
        command_id: session.terminal_command_id.clone(),
        phase: session.phase.clone(),
        status: session.status.clone(),
        planned_duration_ms: session.planned_duration_ms,
        completed_at: (session.status == "completed").then(|| {
            ended_at
                .clone()
                .expect("terminal completed session has an end time")
        }),
        ended_at,
    }
}

fn session_from_canonical(timer: CanonicalTimer) -> Result<Session, CoreError> {
    let anchor_at = parse_time(&timer.anchor_at)?;
    if let Some(intent) = &timer.last_intent {
        parse_time(&intent.occurred_at)?;
    }
    let terminal = matches!(
        timer.status.as_str(),
        "completed" | "cancelled" | "superseded"
    );
    let last_command_id = timer
        .last_intent
        .as_ref()
        .map(|intent| intent.command_id.clone())
        .unwrap_or_default();
    let terminal_command_id = terminal
        .then(|| last_command_id.clone())
        .filter(|command_id| !command_id.is_empty());
    Ok(Session {
        history_id: timer.id.clone(),
        timer_id: timer.id,
        task_id: timer.task_id,
        phase: timer.phase,
        status: timer.status,
        planned_duration_ms: timer.planned_duration_ms,
        elapsed_at_anchor_ms: timer.elapsed_at_anchor_ms,
        anchor_at,
        started_at: anchor_at,
        started_by_device_id: timer.started_by_device_id,
        ended_at: terminal.then_some(anchor_at),
        last_command_id,
        terminal_command_id,
        superseded_by_timer_id: None,
        last_intent: timer.last_intent,
    })
}

fn session_from_history(item: HistoryItem) -> Result<Session, CoreError> {
    let ended_at = if item.status == "completed" {
        item.completed_at.as_deref().or(item.ended_at.as_deref())
    } else {
        item.ended_at.as_deref()
    }
    .ok_or(CoreError::MissingProjection("history.endedAt"))?;
    let ended_at = parse_time(ended_at)?;
    let elapsed_at_anchor_ms = if item.status == "completed" {
        item.planned_duration_ms
    } else {
        0
    };
    Ok(Session {
        history_id: item.id,
        timer_id: item.timer_id,
        task_id: item.task_id,
        phase: item.phase,
        status: item.status,
        planned_duration_ms: item.planned_duration_ms,
        elapsed_at_anchor_ms,
        anchor_at: ended_at,
        started_at: ended_at,
        started_by_device_id: String::new(),
        ended_at: Some(ended_at),
        last_command_id: item.command_id.clone().unwrap_or_default(),
        terminal_command_id: item.command_id,
        superseded_by_timer_id: None,
        last_intent: None,
    })
}

fn canonical(session: &Session) -> CanonicalTimer {
    CanonicalTimer {
        id: session.timer_id.clone(),
        task_id: session.task_id.clone(),
        phase: session.phase.clone(),
        status: session.status.clone(),
        planned_duration_ms: session.planned_duration_ms,
        elapsed_at_anchor_ms: clamp(session.elapsed_at_anchor_ms, 0, session.planned_duration_ms),
        anchor_at: format_time(&session.anchor_at),
        started_by_device_id: session.started_by_device_id.clone(),
        last_intent: session.last_intent.clone(),
    }
}

fn wire_session(session: &Session) -> WireSession {
    WireSession {
        timer_id: session.timer_id.clone(),
        task_id: session.task_id.clone(),
        phase: session.phase.clone(),
        status: session.status.clone(),
        planned_duration_ms: session.planned_duration_ms,
        elapsed_at_anchor_ms: session.elapsed_at_anchor_ms,
        anchor_at: format_time(&session.anchor_at),
        started_at: format_time(&session.started_at),
        started_by_device_id: session.started_by_device_id.clone(),
        ended_at: session.ended_at.as_ref().map(format_time),
        last_command_id: session.last_command_id.clone(),
        terminal_command_id: session.terminal_command_id.clone(),
        superseded_by_timer_id: session.superseded_by_timer_id.clone(),
        last_intent: session.last_intent.clone(),
    }
}

fn auto_complete(session: &mut Session, at: &DateTime<Utc>) {
    if session.status != "running" || elapsed_at(session, at) < session.planned_duration_ms {
        return;
    }
    let remaining = (session.planned_duration_ms - session.elapsed_at_anchor_ms).max(0);
    let completed_at = session.anchor_at + Duration::milliseconds(remaining);
    session.status = "completed".into();
    session.elapsed_at_anchor_ms = session.planned_duration_ms;
    session.anchor_at = completed_at;
    session.ended_at = Some(completed_at);
}

fn supersede(session: &mut Session, at: &DateTime<Utc>, replacement_id: &str, command_id: &str) {
    if session.status == "running" {
        session.elapsed_at_anchor_ms = elapsed_at(session, at);
    }
    session.status = "superseded".into();
    session.anchor_at = *at;
    session.ended_at = Some(*at);
    session.last_command_id = command_id.into();
    session.terminal_command_id = Some(command_id.into());
    session.superseded_by_timer_id = Some(replacement_id.into());
}

fn elapsed_at(session: &Session, at: &DateTime<Utc>) -> i64 {
    let delta = if at > &session.anchor_at {
        at.signed_duration_since(session.anchor_at)
            .num_milliseconds()
    } else {
        0
    };
    clamp(
        session.elapsed_at_anchor_ms.saturating_add(delta),
        0,
        session.planned_duration_ms,
    )
}

fn is_active(session: &Session) -> bool {
    matches!(session.status.as_str(), "running" | "paused")
}

fn clamp(value: i64, minimum: i64, maximum: i64) -> i64 {
    value.max(minimum).min(maximum)
}

pub(crate) fn parse_time(value: &str) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| CoreError::InvalidTimestamp(value.to_owned()))
}

fn format_time(value: &DateTime<Utc>) -> String {
    let mut result = value.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanoseconds = value.nanosecond();
    if nanoseconds != 0 {
        let fraction = format!("{nanoseconds:09}").trim_end_matches('0').to_owned();
        result.push('.');
        result.push_str(&fraction);
    }
    result.push('Z');
    result
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureInput {
    epoch: String,
    now_ms: i64,
    commands: Vec<FixtureCommand>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureCommand {
    id: String,
    sequence: i64,
    device_id: String,
    timer_id: String,
    #[serde(rename = "type")]
    kind: String,
    phase: String,
    duration_ms: i64,
    at_ms: i64,
    wall_ms: i64,
    counter: i64,
    elapsed_ms: i64,
    #[serde(default)]
    task_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct FixtureOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    timer: Option<FixtureTimer>,
    history: Vec<FixtureHistory>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FixtureTimer {
    id: String,
    status: String,
    phase: String,
    duration_ms: i64,
    elapsed_ms: i64,
    anchor_ms: i64,
    last_command_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FixtureHistory {
    timer_id: String,
    status: String,
    phase: String,
    duration_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_id: Option<String>,
    ended_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
}

pub fn reduce_timer_fixture_case_json(input: &str) -> Result<String, CoreError> {
    let input: FixtureInput = serde_json::from_str(input)?;
    let epoch = parse_time(&input.epoch)?;
    let now = epoch + Duration::milliseconds(input.now_ms);
    let reduction = reduce(fixture_commands(input.commands, epoch), now);
    let timer = reduction
        .canonical_timer
        .map(|timer| fixture_timer(timer, epoch));
    let history = fixture_history(reduction.history, epoch)?;
    Ok(serde_json::to_string(&FixtureOutput { timer, history })?)
}

fn fixture_commands(commands: Vec<FixtureCommand>, epoch: DateTime<Utc>) -> Vec<Command> {
    commands
        .into_iter()
        .map(|command| {
            let _ = command.sequence;
            Command {
                id: command.id,
                device_id: command.device_id,
                timer_id: command.timer_id,
                task_id: command.task_id,
                kind: command.kind,
                phase: command.phase,
                planned_duration_ms: command.duration_ms,
                occurred_at: epoch + Duration::milliseconds(command.at_ms),
                hlc_wall_ms: command.wall_ms,
                hlc_counter: command.counter,
                observed_elapsed_ms: command.elapsed_ms,
            }
        })
        .collect()
}

fn fixture_timer(timer: CanonicalTimer, epoch: DateTime<Utc>) -> FixtureTimer {
    FixtureTimer {
        id: timer.id,
        status: timer.status,
        phase: timer.phase,
        duration_ms: timer.planned_duration_ms,
        elapsed_ms: timer.elapsed_at_anchor_ms,
        anchor_ms: parse_time(&timer.anchor_at)
            .expect("reducer emitted a valid timestamp")
            .signed_duration_since(epoch)
            .num_milliseconds(),
        last_command_id: timer
            .last_intent
            .map(|intent| intent.command_id)
            .unwrap_or_default(),
        task_id: timer.task_id,
    }
}

fn fixture_history(
    items: Vec<HistoryItem>,
    epoch: DateTime<Utc>,
) -> Result<Vec<FixtureHistory>, CoreError> {
    let mut history = Vec::with_capacity(items.len());
    for item in items {
        let ended_at = item
            .ended_at
            .as_deref()
            .ok_or(CoreError::MissingProjection("history.endedAt"))?;
        history.push(FixtureHistory {
            timer_id: item.timer_id,
            status: item.status,
            phase: item.phase,
            duration_ms: item.planned_duration_ms,
            command_id: item.command_id,
            ended_ms: parse_time(ended_at)?
                .signed_duration_since(epoch)
                .num_milliseconds(),
            task_id: item.task_id,
        });
    }
    Ok(history)
}
