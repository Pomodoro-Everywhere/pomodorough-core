use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::CoreError;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimerReductionInput {
    #[serde(default)]
    commands: Vec<WireCommand>,
    now: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireCommand {
    id: String,
    device_id: String,
    device_sequence: i64,
    timer_id: String,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    phase: String,
    planned_duration_ms: i64,
    occurred_at: String,
    hlc_wall_ms: i64,
    hlc_counter: i64,
    observed_elapsed_ms: i64,
}

impl WireCommand {
    fn into_command(self) -> Result<Command, CoreError> {
        let _ = self.device_sequence;
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Intent {
    #[serde(rename = "type")]
    kind: String,
    command_id: String,
    occurred_at: String,
}

#[derive(Clone, Debug)]
struct Session {
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
struct Outcome {
    outcome: String,
    reason: String,
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
struct TimerReductionOutput {
    canonical_timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    sessions: Vec<WireSession>,
    outcomes: BTreeMap<String, Outcome>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalTimer {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
    phase: String,
    status: String,
    planned_duration_ms: i64,
    elapsed_at_anchor_ms: i64,
    anchor_at: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    started_by_device_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_intent: Option<Intent>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryItem {
    id: String,
    timer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_id: Option<String>,
    phase: String,
    status: String,
    planned_duration_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ended_at: Option<String>,
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
    let now = parse_time(&input.now)?;
    let commands = input
        .commands
        .into_iter()
        .map(WireCommand::into_command)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(serde_json::to_string(&reduce(commands, now))?)
}

fn reduce(mut commands: Vec<Command>, now: DateTime<Utc>) -> TimerReductionOutput {
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

    let mut sessions: BTreeMap<String, Session> = BTreeMap::new();
    let mut outcomes = BTreeMap::new();
    let mut current_id: Option<String> = None;

    for command in commands {
        if let Some(current) = current_id.as_ref().and_then(|id| sessions.get_mut(id)) {
            auto_complete(current, &command.occurred_at);
        }
        let intent = Intent {
            kind: command.kind.clone(),
            command_id: command.id.clone(),
            occurred_at: format_time(&command.occurred_at),
        };

        match command.kind.as_str() {
            "start" => {
                if sessions.contains_key(&command.timer_id) {
                    outcomes.insert(command.id, Outcome::ignored("timer already exists"));
                    continue;
                }
                if let Some(current) = current_id.as_ref().and_then(|id| sessions.get_mut(id)) {
                    if is_active(current) {
                        supersede(
                            current,
                            &command.occurred_at,
                            &command.timer_id,
                            &command.id,
                        );
                    }
                }
                let timer_id = command.timer_id.clone();
                sessions.insert(
                    timer_id.clone(),
                    Session {
                        timer_id: timer_id.clone(),
                        task_id: command.task_id,
                        phase: command.phase,
                        status: "running".into(),
                        planned_duration_ms: command.planned_duration_ms,
                        elapsed_at_anchor_ms: 0,
                        anchor_at: command.occurred_at,
                        started_at: command.occurred_at,
                        started_by_device_id: command.device_id,
                        ended_at: None,
                        last_command_id: command.id.clone(),
                        terminal_command_id: None,
                        superseded_by_timer_id: None,
                        last_intent: Some(intent),
                    },
                );
                current_id = Some(timer_id);
                outcomes.insert(command.id, Outcome::applied());
            }
            "pause" => {
                let Some(target) = sessions.get_mut(&command.timer_id) else {
                    outcomes.insert(
                        command.id,
                        Outcome::ignored("timer is not the active running timer"),
                    );
                    continue;
                };
                if current_id.as_deref() != Some(command.timer_id.as_str())
                    || target.status != "running"
                {
                    outcomes.insert(
                        command.id,
                        Outcome::ignored("timer is not the active running timer"),
                    );
                    continue;
                }
                target.status = "paused".into();
                target.elapsed_at_anchor_ms =
                    clamp(command.observed_elapsed_ms, 0, target.planned_duration_ms);
                target.anchor_at = command.occurred_at;
                target.last_command_id = command.id.clone();
                target.last_intent = Some(intent);
                outcomes.insert(command.id, Outcome::applied());
            }
            "resume" => {
                let resumable = sessions.get(&command.timer_id).is_some_and(|target| {
                    matches!(target.status.as_str(), "paused" | "superseded")
                });
                if !resumable {
                    outcomes.insert(command.id, Outcome::ignored("timer cannot be resumed"));
                    continue;
                }
                if let Some(current) = current_id.as_ref().and_then(|id| sessions.get_mut(id)) {
                    if current.timer_id != command.timer_id && is_active(current) {
                        supersede(
                            current,
                            &command.occurred_at,
                            &command.timer_id,
                            &command.id,
                        );
                    }
                }
                let target = sessions.get_mut(&command.timer_id).expect("checked above");
                target.status = "running".into();
                target.elapsed_at_anchor_ms =
                    clamp(command.observed_elapsed_ms, 0, target.planned_duration_ms);
                target.anchor_at = command.occurred_at;
                target.ended_at = None;
                target.terminal_command_id = None;
                target.superseded_by_timer_id = None;
                target.last_command_id = command.id.clone();
                target.last_intent = Some(intent);
                current_id = Some(target.timer_id.clone());
                outcomes.insert(command.id, Outcome::applied());
            }
            "finish" | "cancel" => {
                let Some(target) = sessions.get_mut(&command.timer_id) else {
                    outcomes.insert(command.id, Outcome::ignored("timer is not active"));
                    continue;
                };
                if command.kind == "finish"
                    && current_id.as_deref() == Some(command.timer_id.as_str())
                    && target.status == "completed"
                    && target.terminal_command_id.is_none()
                {
                    target.last_command_id = command.id.clone();
                    target.terminal_command_id = Some(command.id.clone());
                    target.last_intent = Some(intent);
                    outcomes.insert(command.id, Outcome::applied());
                    continue;
                }
                if current_id.as_deref() != Some(command.timer_id.as_str()) || !is_active(target) {
                    outcomes.insert(command.id, Outcome::ignored("timer is not active"));
                    continue;
                }
                if command.kind == "finish" {
                    target.status = "completed".into();
                    target.elapsed_at_anchor_ms = target.planned_duration_ms;
                } else {
                    target.status = "cancelled".into();
                    target.elapsed_at_anchor_ms =
                        clamp(command.observed_elapsed_ms, 0, target.planned_duration_ms);
                }
                target.anchor_at = command.occurred_at;
                target.ended_at = Some(command.occurred_at);
                target.last_command_id = command.id.clone();
                target.terminal_command_id = Some(command.id.clone());
                target.last_intent = Some(intent);
                outcomes.insert(command.id, Outcome::applied());
            }
            "clear" => {
                let Some(target) = sessions.get_mut(&command.timer_id) else {
                    outcomes.insert(command.id, Outcome::ignored("timer cannot be cleared"));
                    continue;
                };
                if current_id.as_deref() != Some(command.timer_id.as_str()) || is_active(target) {
                    outcomes.insert(command.id, Outcome::ignored("timer cannot be cleared"));
                    continue;
                }
                target.last_command_id = command.id.clone();
                target.last_intent = Some(intent);
                current_id = None;
                outcomes.insert(command.id, Outcome::applied());
            }
            _ => {
                outcomes.insert(command.id, Outcome::rejected("unsupported command type"));
            }
        }
    }

    if let Some(current) = current_id.as_ref().and_then(|id| sessions.get_mut(id)) {
        auto_complete(current, &now);
    }

    let canonical_timer = current_id
        .as_ref()
        .and_then(|id| sessions.get(id))
        .map(canonical);

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
    let history = terminal
        .into_iter()
        .map(|session| {
            let ended_at = session.ended_at.as_ref().map(format_time);
            HistoryItem {
                id: session.timer_id.clone(),
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
        })
        .collect();
    let sessions = sessions.values().map(wire_session).collect();

    TimerReductionOutput {
        canonical_timer,
        history,
        sessions,
        outcomes,
    }
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
        session.elapsed_at_anchor_ms + delta,
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

fn parse_time(value: &str) -> Result<DateTime<Utc>, CoreError> {
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
    let commands = input
        .commands
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
        .collect();
    let reduction = reduce(commands, now);

    let timer = reduction.canonical_timer.map(|timer| FixtureTimer {
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
    });
    let mut history = Vec::with_capacity(reduction.history.len());
    for item in reduction.history {
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

    Ok(serde_json::to_string(&FixtureOutput { timer, history })?)
}
