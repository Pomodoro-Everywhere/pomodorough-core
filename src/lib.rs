use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

#[cfg(target_arch = "wasm32")]
mod wasm_abi;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SelectedTaskField {
    #[default]
    Omitted,
    Deselected,
    Selected(String),
}

impl<'de> Deserialize<'de> for SelectedTaskField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = SelectedTaskField;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("null or a selected task identifier")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(SelectedTaskField::Deselected)
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(SelectedTaskField::Deselected)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SelectedTaskField::Selected(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(SelectedTaskField::Selected(value))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectedTaskEnvelope {
    #[serde(default)]
    selected_task_id: SelectedTaskField,
}

pub fn classify_selected_task_field_json(input: &str) -> Result<String, CoreError> {
    let envelope: SelectedTaskEnvelope = serde_json::from_str(input)?;
    Ok(match envelope.selected_task_id {
        SelectedTaskField::Omitted => "omitted".into(),
        SelectedTaskField::Deselected => "deselected".into(),
        SelectedTaskField::Selected(id) => format!("selected:{id}"),
    })
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid shared-core JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing required projection value: {0}")]
    MissingProjection(&'static str),
    #[error("unsupported shared-core operation: {0}")]
    UnsupportedOperation(String),
}

pub fn dispatch_envelope_json(operation: &str, input: &str) -> String {
    match dispatch_json(operation, input) {
        Ok(value) => {
            let value = serde_json::from_str::<serde_json::Value>(&value)
                .unwrap_or(serde_json::Value::String(value));
            serde_json::json!({"ok": true, "value": value}).to_string()
        }
        Err(error) => serde_json::json!({"ok": false, "error": error.to_string()}).to_string(),
    }
}

pub fn dispatch_json(operation: &str, input: &str) -> Result<String, CoreError> {
    match operation {
        "core.version" => Ok(serde_json::json!({
            "schemaVersion": 1,
            "coreVersion": env!("CARGO_PKG_VERSION"),
        })
        .to_string()),
        "timer.reduce" => reduce_timer_fixture_case_json(input),
        "projection.reduce" => reduce_projection_fixture_case_json(input),
        "selectedTask.reduce" => reduce_selected_task_json(input),
        "selectedTask.classify" => classify_selected_task_field_json(input),
        other => Err(CoreError::UnsupportedOperation(other.to_owned())),
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectionInput {
    task_operations: Vec<TaskOperation>,
    duration_operations: Vec<DurationOperation>,
    auto_start_operations: Vec<AutoStartOperation>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationClock {
    id: String,
    device_id: String,
    wall_ms: i64,
    counter: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskOperation {
    #[serde(flatten)]
    clock: OperationClock,
    task_id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    title: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DurationOperation {
    #[serde(flatten)]
    clock: OperationClock,
    phase: String,
    duration_ms: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoStartOperation {
    #[serde(flatten)]
    clock: OperationClock,
    enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectionOutput {
    tasks: Vec<ProjectedTask>,
    durations_ms: BTreeMap<String, i64>,
    auto_start_breaks: bool,
}

#[derive(Debug, Serialize)]
struct ProjectedTask {
    id: String,
    title: String,
}

pub fn reduce_projection_fixture_case_json(input: &str) -> Result<String, CoreError> {
    let input: ProjectionInput = serde_json::from_str(input)?;

    let mut latest_tasks: BTreeMap<String, TaskOperation> = BTreeMap::new();
    for operation in input.task_operations {
        let replace = latest_tasks.get(&operation.task_id).is_none_or(|current| {
            operation_clock_key(&operation.clock) > operation_clock_key(&current.clock)
        });
        if replace {
            latest_tasks.insert(operation.task_id.clone(), operation);
        }
    }
    let tasks = latest_tasks
        .into_values()
        .filter(|operation| operation.kind == "upsert")
        .map(|operation| ProjectedTask {
            id: operation.task_id,
            title: operation.title,
        })
        .collect();

    let mut latest_durations: BTreeMap<String, DurationOperation> = BTreeMap::new();
    for operation in input.duration_operations {
        let replace = latest_durations
            .get(&operation.phase)
            .is_none_or(|current| {
                operation_clock_key(&operation.clock) > operation_clock_key(&current.clock)
            });
        if replace {
            latest_durations.insert(operation.phase.clone(), operation);
        }
    }
    let durations_ms = latest_durations
        .into_iter()
        .map(|(phase, operation)| (phase, operation.duration_ms))
        .collect();

    let auto_start_breaks = input
        .auto_start_operations
        .into_iter()
        .max_by(|left, right| {
            operation_clock_key(&left.clock).cmp(&operation_clock_key(&right.clock))
        })
        .ok_or(CoreError::MissingProjection("autoStartOperations"))?
        .enabled;

    Ok(serde_json::to_string(&ProjectionOutput {
        tasks,
        durations_ms,
        auto_start_breaks,
    })?)
}

fn operation_clock_key(clock: &OperationClock) -> (i64, i64, &str, &str) {
    (
        clock.wall_ms,
        clock.counter,
        clock.device_id.as_str(),
        clock.id.as_str(),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectedTaskReductionInput {
    operations: Vec<SelectedTaskOperation>,
    active_task_ids: BTreeSet<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectedTaskOperation {
    #[serde(flatten)]
    clock: OperationClock,
    task_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectedTaskReductionOutput {
    selected_task_id: Option<String>,
}

pub fn reduce_selected_task_json(input: &str) -> Result<String, CoreError> {
    let input: SelectedTaskReductionInput = serde_json::from_str(input)?;
    let selected_task_id = input
        .operations
        .into_iter()
        .max_by(|left, right| {
            operation_clock_key(&left.clock).cmp(&operation_clock_key(&right.clock))
        })
        .and_then(|operation| operation.task_id)
        .filter(|task_id| input.active_task_ids.contains(task_id));
    Ok(serde_json::to_string(&SelectedTaskReductionOutput {
        selected_task_id,
    })?)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureInput {
    #[allow(dead_code)]
    epoch: String,
    now_ms: i64,
    commands: Vec<Command>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Command {
    id: String,
    #[allow(dead_code)]
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

#[derive(Clone, Debug)]
struct Session {
    timer_id: String,
    task_id: Option<String>,
    phase: String,
    status: String,
    duration_ms: i64,
    elapsed_ms: i64,
    anchor_ms: i64,
    ended_ms: Option<i64>,
    last_command_id: String,
    terminal_command_id: Option<String>,
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
    let output = reduce(input.commands, input.now_ms);
    Ok(serde_json::to_string(&output)?)
}

fn reduce(mut commands: Vec<Command>, now_ms: i64) -> FixtureOutput {
    commands.sort_by(|left, right| {
        (
            left.wall_ms,
            left.counter,
            left.device_id.as_str(),
            left.id.as_str(),
        )
            .cmp(&(
                right.wall_ms,
                right.counter,
                right.device_id.as_str(),
                right.id.as_str(),
            ))
    });

    let mut sessions: BTreeMap<String, Session> = BTreeMap::new();
    let mut current_id: Option<String> = None;

    for command in commands {
        if let Some(current) = current_id.as_ref().and_then(|id| sessions.get_mut(id)) {
            auto_complete(current, command.at_ms);
        }

        match command.kind.as_str() {
            "start" => {
                if sessions.contains_key(&command.timer_id) {
                    continue;
                }
                if let Some(current) = current_id.as_ref().and_then(|id| sessions.get_mut(id)) {
                    if is_active(current) {
                        supersede(current, command.at_ms, &command.id);
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
                        duration_ms: command.duration_ms,
                        elapsed_ms: 0,
                        anchor_ms: command.at_ms,
                        ended_ms: None,
                        last_command_id: command.id,
                        terminal_command_id: None,
                    },
                );
                current_id = Some(timer_id);
            }
            "pause" => {
                let Some(target) = sessions.get_mut(&command.timer_id) else {
                    continue;
                };
                if current_id.as_deref() != Some(command.timer_id.as_str())
                    || target.status != "running"
                {
                    continue;
                }
                target.status = "paused".into();
                target.elapsed_ms = clamp(command.elapsed_ms, 0, target.duration_ms);
                target.anchor_ms = command.at_ms;
                target.last_command_id = command.id;
            }
            "resume" => {
                let resumable = sessions.get(&command.timer_id).is_some_and(|target| {
                    matches!(target.status.as_str(), "paused" | "superseded")
                });
                if !resumable {
                    continue;
                }
                if let Some(current) = current_id.as_ref().and_then(|id| sessions.get_mut(id)) {
                    if current.timer_id != command.timer_id && is_active(current) {
                        supersede(current, command.at_ms, &command.id);
                    }
                }
                let target = sessions.get_mut(&command.timer_id).expect("checked above");
                target.status = "running".into();
                target.elapsed_ms = clamp(command.elapsed_ms, 0, target.duration_ms);
                target.anchor_ms = command.at_ms;
                target.ended_ms = None;
                target.terminal_command_id = None;
                target.last_command_id = command.id;
                current_id = Some(target.timer_id.clone());
            }
            "finish" | "cancel" => {
                let Some(target) = sessions.get_mut(&command.timer_id) else {
                    continue;
                };
                if command.kind == "finish"
                    && current_id.as_deref() == Some(command.timer_id.as_str())
                    && target.status == "completed"
                    && target.terminal_command_id.is_none()
                {
                    target.last_command_id = command.id.clone();
                    target.terminal_command_id = Some(command.id);
                    continue;
                }
                if current_id.as_deref() != Some(command.timer_id.as_str()) || !is_active(target) {
                    continue;
                }
                if command.kind == "finish" {
                    target.status = "completed".into();
                    target.elapsed_ms = target.duration_ms;
                } else {
                    target.status = "cancelled".into();
                    target.elapsed_ms = clamp(command.elapsed_ms, 0, target.duration_ms);
                }
                target.anchor_ms = command.at_ms;
                target.ended_ms = Some(command.at_ms);
                target.last_command_id = command.id.clone();
                target.terminal_command_id = Some(command.id);
            }
            "clear" => {
                let Some(target) = sessions.get_mut(&command.timer_id) else {
                    continue;
                };
                if current_id.as_deref() != Some(command.timer_id.as_str()) || is_active(target) {
                    continue;
                }
                target.last_command_id = command.id;
                current_id = None;
            }
            _ => {}
        }
    }

    if let Some(current) = current_id.as_ref().and_then(|id| sessions.get_mut(id)) {
        auto_complete(current, now_ms);
    }

    let timer = current_id
        .as_ref()
        .and_then(|id| sessions.get(id))
        .map(|session| FixtureTimer {
            id: session.timer_id.clone(),
            status: session.status.clone(),
            phase: session.phase.clone(),
            duration_ms: session.duration_ms,
            elapsed_ms: clamp(session.elapsed_ms, 0, session.duration_ms),
            anchor_ms: session.anchor_ms,
            last_command_id: session.last_command_id.clone(),
            task_id: session.task_id.clone(),
        });

    let mut terminal: Vec<&Session> = sessions
        .values()
        .filter(|session| {
            matches!(
                session.status.as_str(),
                "completed" | "cancelled" | "superseded"
            )
        })
        .collect();
    terminal.sort_by(|left, right| {
        right
            .ended_ms
            .cmp(&left.ended_ms)
            .then_with(|| left.timer_id.cmp(&right.timer_id))
    });
    let history = terminal
        .into_iter()
        .map(|session| FixtureHistory {
            timer_id: session.timer_id.clone(),
            status: session.status.clone(),
            phase: session.phase.clone(),
            duration_ms: session.duration_ms,
            command_id: session.terminal_command_id.clone(),
            ended_ms: session.ended_ms.unwrap_or(session.anchor_ms),
            task_id: session.task_id.clone(),
        })
        .collect();

    FixtureOutput { timer, history }
}

fn auto_complete(session: &mut Session, at_ms: i64) {
    if session.status != "running" || elapsed_at(session, at_ms) < session.duration_ms {
        return;
    }
    let remaining = (session.duration_ms - session.elapsed_ms).max(0);
    let completed_at = session.anchor_ms + remaining;
    session.status = "completed".into();
    session.elapsed_ms = session.duration_ms;
    session.anchor_ms = completed_at;
    session.ended_ms = Some(completed_at);
}

fn supersede(session: &mut Session, at_ms: i64, command_id: &str) {
    if session.status == "running" {
        session.elapsed_ms = elapsed_at(session, at_ms);
    }
    session.status = "superseded".into();
    session.anchor_ms = at_ms;
    session.ended_ms = Some(at_ms);
    session.last_command_id = command_id.into();
    session.terminal_command_id = Some(command_id.into());
}

fn elapsed_at(session: &Session, at_ms: i64) -> i64 {
    let delta = (at_ms - session.anchor_ms).max(0);
    clamp(session.elapsed_ms + delta, 0, session.duration_ms)
}

fn is_active(session: &Session) -> bool {
    matches!(session.status.as_str(), "running" | "paused")
}

fn clamp(value: i64, minimum: i64, maximum: i64) -> i64 {
    value.max(minimum).min(maximum)
}
