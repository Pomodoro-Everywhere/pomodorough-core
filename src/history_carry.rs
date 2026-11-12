//! History carryover on task rename (R54-C04).
//!
//! Task identity hashes the title, so a rename gives the task a new ID. A
//! `rename` task operation records the edge from the old ID to the new one.
//! Each edge moves the finished history that started at or before the rename
//! from the old ID to the new ID. Edges apply in reducer order
//! `(hlcWallMs, hlcCounter, deviceId, id)`, so every device and the server
//! reach the same result from the same operation log, whatever order the
//! operations arrived in.
//!
//! A row's start is the reducer session `startedAt` for its timer, whatever
//! the row's status. A row without a session falls back to its end time
//! (`completedAt` when completed, `endedAt` otherwise). The end is never
//! before the start, so a missing session can only move fewer rows.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::CoreError;
use crate::sync_projection::{
    TaskOperation, operation_clock_key, renamed_task_id, validate_standalone_timestamps,
    validate_task_log,
};
use crate::timer::{CanonicalTimer, HistoryItem, WireSession, parse_time};

/// One rename: rows of `from` that started at or before `at` move to `to`.
#[derive(Debug)]
pub(crate) struct RenameEdge {
    from: String,
    to: String,
    at: DateTime<Utc>,
}

/// Start time of each reducer session, keyed by timer ID.
pub(crate) type SessionStarts = BTreeMap<String, DateTime<Utc>>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CarryHistoryInput {
    canonical_timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    sessions: Vec<WireSession>,
    task_operations: Vec<TaskOperation>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CarryHistoryOutput {
    canonical_timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
}

/// `task.carryHistory.v1`: apply every rename in a complete task operation
/// log to the history and canonical timer of a full timer reduction.
pub(crate) fn carry_history_json(input: &str) -> Result<String, CoreError> {
    let value = strict_carry_input(input)?;
    let input: CarryHistoryInput = serde_json::from_value(value)?;
    validate_task_log(&input.task_operations)?;
    validate_standalone_timestamps(input.task_operations.iter().map(|op| &op.clock))?;
    crate::timer::workspace::validate_pair(input.canonical_timer.as_ref(), &input.history)?;
    require_uncarried(
        input.canonical_timer.as_ref(),
        &input.history,
        &input.sessions,
    )?;
    let edges = rename_edges(&input.task_operations)?;
    let starts = session_starts(&input.sessions)?;
    let mut output = CarryHistoryOutput {
        canonical_timer: input.canonical_timer,
        history: input.history,
    };
    carry(
        &edges,
        &starts,
        &mut output.history,
        output.canonical_timer.as_mut(),
    )?;
    Ok(serde_json::to_string(&output)?)
}

fn strict_carry_input(input: &str) -> Result<serde_json::Value, CoreError> {
    let value = crate::strict_json::parse(input)?;
    let root = crate::strict_json::object(&value, "carry input")?;
    if !root.contains_key("canonicalTimer") {
        return Err(CoreError::InvalidInput(
            "canonicalTimer must be a JSON object or null".into(),
        ));
    }
    crate::strict_json::nullable_object_field(root, "canonicalTimer", "canonicalTimer")?;
    for field in ["history", "sessions", "taskOperations"] {
        crate::strict_json::object_array_field(root, field, field, true)?;
    }
    Ok(value)
}

/// Carrying is not idempotent, so the input must be uncarried reducer output.
/// Reducer sessions keep the raw task, so a row or terminal timer whose
/// `taskId` differs from its own session was carried already (or was edited
/// after the reduction) and fails closed. A row without a session cannot be
/// checked.
fn require_uncarried(
    canonical_timer: Option<&CanonicalTimer>,
    history: &[HistoryItem],
    sessions: &[WireSession],
) -> Result<(), CoreError> {
    let session_tasks = sessions
        .iter()
        .map(|session| (session.timer_id.as_str(), &session.task_id))
        .collect::<BTreeMap<_, _>>();
    let rows = history.iter().map(|row| (&row.timer_id, &row.task_id));
    let timer = canonical_timer.map(|timer| (&timer.id, &timer.task_id));
    for (timer_id, task_id) in rows.chain(timer) {
        if session_tasks
            .get(timer_id.as_str())
            .is_some_and(|session_task| *session_task != task_id)
        {
            return Err(CoreError::InvalidInput(format!(
                "history for timer {timer_id} disagrees with its session taskId; \
                 pass uncarried reducer output"
            )));
        }
    }
    Ok(())
}

/// The rename edges of a validated task log, in reducer order.
pub(crate) fn rename_edges(operations: &[TaskOperation]) -> Result<Vec<RenameEdge>, CoreError> {
    let mut renames = operations
        .iter()
        .filter(|operation| operation.kind == "rename")
        .collect::<Vec<_>>();
    renames.sort_by(|left, right| {
        operation_clock_key(&left.clock).cmp(&operation_clock_key(&right.clock))
    });
    renames
        .into_iter()
        .map(|operation| {
            Ok(RenameEdge {
                from: operation.task_id.clone(),
                to: renamed_task_id(operation)?,
                at: parse_time(&operation.clock.occurred_at)?,
            })
        })
        .collect()
}

/// Start time of each session. A timer ID may appear only once.
pub(crate) fn session_starts(sessions: &[WireSession]) -> Result<SessionStarts, CoreError> {
    let mut starts = SessionStarts::new();
    for session in sessions {
        let start = parse_time(&session.started_at)?;
        if starts.insert(session.timer_id.clone(), start).is_some() {
            return Err(CoreError::InvalidInput("duplicate session timerId".into()));
        }
    }
    Ok(starts)
}

/// Move history rows and a terminal canonical timer along the rename edges.
/// Only `taskId` changes; every other field and the row order stay the same.
pub(crate) fn carry(
    edges: &[RenameEdge],
    starts: &SessionStarts,
    history: &mut [HistoryItem],
    canonical_timer: Option<&mut CanonicalTimer>,
) -> Result<(), CoreError> {
    if edges.is_empty() {
        return Ok(());
    }
    let row_starts = history
        .iter()
        .map(|row| row_start(row, starts))
        .collect::<Result<Vec<_>, _>>()?;
    let timer_start = match &canonical_timer {
        Some(timer) => terminal_timer_start(timer, history, &row_starts, starts)?,
        None => None,
    };
    let mut timer_task = canonical_timer;
    for edge in edges {
        for (row, start) in history.iter_mut().zip(&row_starts) {
            move_along(&mut row.task_id, *start, edge);
        }
        if let (Some(timer), Some(start)) = (timer_task.as_deref_mut(), timer_start) {
            move_along(&mut timer.task_id, start, edge);
        }
    }
    Ok(())
}

fn move_along(task_id: &mut Option<String>, start: DateTime<Utc>, edge: &RenameEdge) {
    if task_id.as_deref() == Some(edge.from.as_str()) && start <= edge.at {
        *task_id = Some(edge.to.clone());
    }
}

fn row_start(row: &HistoryItem, starts: &SessionStarts) -> Result<DateTime<Utc>, CoreError> {
    if let Some(start) = starts.get(&row.timer_id) {
        return Ok(*start);
    }
    let end = if row.status == "completed" {
        row.completed_at.as_deref()
    } else {
        row.ended_at.as_deref()
    };
    let end = end.ok_or_else(|| CoreError::InvalidInput("history row has no end time".into()))?;
    parse_time(end)
}

/// Start of a terminal canonical timer. It equals the start of its own
/// history row when one exists, so the timer and its row always move
/// together. A live timer never moves: its task follows the planner retarget.
fn terminal_timer_start(
    timer: &CanonicalTimer,
    history: &[HistoryItem],
    row_starts: &[DateTime<Utc>],
    starts: &SessionStarts,
) -> Result<Option<DateTime<Utc>>, CoreError> {
    if !matches!(
        timer.status.as_str(),
        "completed" | "cancelled" | "superseded"
    ) {
        return Ok(None);
    }
    if let Some(index) = history.iter().position(|row| row.timer_id == timer.id) {
        return Ok(Some(row_starts[index]));
    }
    match starts.get(&timer.id) {
        Some(start) => Ok(Some(*start)),
        None => parse_time(&timer.anchor_at).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer::format_time;

    fn edge(from: &str, to: &str, at: &str) -> RenameEdge {
        RenameEdge {
            from: from.into(),
            to: to.into(),
            at: parse_time(at).unwrap(),
        }
    }

    #[test]
    fn edge_bound_is_inclusive_and_formats_round_trip() {
        let at = "2026-11-12T10:00:00Z";
        let mut task = Some("a".to_owned());
        move_along(&mut task, parse_time(at).unwrap(), &edge("a", "b", at));
        assert_eq!(task.as_deref(), Some("b"));
        assert_eq!(format_time(&parse_time(at).unwrap()), at);
    }
}
