//! R54-C04: a rename moves finished history to the new task identity.
//!
//! Shared builders for the carryover scenarios in `tests/history_carry/`.
use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

#[path = "history_carry/boundary.rs"]
mod boundary;
#[path = "history_carry/full_log.rs"]
mod full_log;
#[path = "history_carry/projection.rs"]
mod projection;
#[path = "history_carry/rebase.rs"]
mod rebase;
#[path = "history_carry/reused_base.rs"]
mod reused_base;
#[path = "history_carry/validation.rs"]
mod validation;

pub(crate) const PLANNED_MS: i64 = 1_500_000;
/// 2026-03-08T06:00:00Z, inside the read-model fixture's reference day.
pub(crate) const WALL: i64 = 1_772_949_600_000;

pub(crate) fn call(operation: &str, input: &Value) -> Result<Value, String> {
    dispatch_json(operation, &input.to_string())
        .map(|output| serde_json::from_str(&output).unwrap())
        .map_err(|error| error.to_string())
}

pub(crate) fn id(title: &str) -> String {
    call("task.identity.v1", &json!({"title": title})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// RFC 3339 time `minutes` after 06:00 on the reference day.
pub(crate) fn at(minutes: i64) -> String {
    format!("2026-03-08T{:02}:{:02}:00Z", 6 + minutes / 60, minutes % 60)
}

fn wall(minutes: i64) -> i64 {
    WALL + minutes * 60_000
}

/// A rename of `from` to `title` at `minutes`, sent by `device`.
pub(crate) fn rename(op_id: &str, device: &str, from: &str, title: &str, minutes: i64) -> Value {
    json!({
        "id": op_id, "deviceId": device, "occurredAt": at(minutes),
        "hlcWallMs": wall(minutes), "hlcCounter": 0,
        "taskId": id(from), "type": "rename", "title": title
    })
}

pub(crate) fn upsert(op_id: &str, title: &str, minutes: i64) -> Value {
    json!({
        "id": op_id, "deviceId": "device-a", "occurredAt": at(minutes),
        "hlcWallMs": wall(minutes), "hlcCounter": 1,
        "taskId": id(title), "type": "upsert", "title": title
    })
}

/// A focus timer command on `device` for the task titled `title`.
pub(crate) fn command(device: &str, sequence: i64, timer: &str, kind: &str, minutes: i64) -> Value {
    json!({
        "id": format!("{timer}-{kind}"), "deviceId": device, "deviceSequence": sequence,
        "timerId": timer, "type": kind, "phase": "focus",
        "plannedDurationMs": PLANNED_MS, "occurredAt": at(minutes),
        "hlcWallMs": wall(minutes), "hlcCounter": 2, "observedElapsedMs": 0
    })
}

pub(crate) fn assigned(mut command: Value, title: &str) -> Value {
    command["taskId"] = json!(id(title));
    command
}

/// A finished focus row that ran from `start` to `end` minutes.
pub(crate) fn finished(timer: &str, title: &str, start: i64, end: i64) -> [Value; 2] {
    [
        assigned(command("device-b", start + 1, timer, "start", start), title),
        command("device-b", end + 1, timer, "finish", end),
    ]
}

pub(crate) fn reduce(commands: Vec<Value>) -> Value {
    call(
        "timer.reduce.v1",
        &json!({"commands": commands, "now": at(120)}),
    )
    .unwrap()
}

/// `task.carryHistory.v1` over a `timer.reduce.v1` result and a task log.
pub(crate) fn carry(reduced: &Value, task_operations: Vec<Value>) -> Result<Value, String> {
    call(
        "task.carryHistory.v1",
        &json!({
            "canonicalTimer": reduced["canonicalTimer"],
            "history": reduced["history"],
            "sessions": reduced["sessions"],
            "taskOperations": task_operations
        }),
    )
}

/// Task ID of each history row, keyed by timer ID.
pub(crate) fn tasks_by_timer(history: &Value) -> std::collections::BTreeMap<String, String> {
    history
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let task = row["taskId"].as_str().unwrap_or("").to_owned();
            (row["timerId"].as_str().unwrap().to_owned(), task)
        })
        .collect()
}
