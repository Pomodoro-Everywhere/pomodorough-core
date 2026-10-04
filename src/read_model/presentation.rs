use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::{Input, Profile, clock::TimerView, invalid};
use crate::{CoreError, timer::HistoryItem};

pub(super) struct Cadence {
    today: usize,
    total: usize,
    planned_duration_today_ms: i64,
    day: (DateTime<Utc>, DateTime<Utc>),
}

pub(super) fn cadence(
    history: &[HistoryItem],
    day: (DateTime<Utc>, DateTime<Utc>),
) -> Result<Cadence, CoreError> {
    let mut today = 0;
    let mut total = 0;
    let mut planned_duration_today_ms = 0_i64;
    for item in history
        .iter()
        .filter(|item| item.status == "completed" && item.phase == "focus")
    {
        total += 1;
        let at = item
            .completed_at
            .as_deref()
            .or(item.ended_at.as_deref())
            .ok_or_else(|| invalid("missing focus completion time"))?;
        let at = crate::timer::parse_time(at)?;
        if at >= day.0 && at < day.1 {
            today += 1;
            planned_duration_today_ms = planned_duration_today_ms
                .checked_add(item.planned_duration_ms)
                .ok_or_else(|| invalid("read model daily focus duration overflow"))?;
        }
    }
    Ok(Cadence {
        today,
        total,
        planned_duration_today_ms,
        day,
    })
}

pub(super) fn render(
    input: &Input,
    workspace: &Value,
    history: &[HistoryItem],
    canonical: &TimerView,
    cadence: Cadence,
) -> Result<Value, CoreError> {
    let status = canonical.status.as_str();
    let source = natural_source(input, workspace, history)?;
    let selected_phase = natural_phase(input, source, history, cadence.day)?;
    let display = display(input, workspace, canonical, &selected_phase)?;
    let mut available = intents(input.profile, status);
    if source.is_some_and(|row| !input.lifecycle.finished(&row.timer_id, &row.phase))
        && !retained_finish(input, source.unwrap())
    {
        available.push("finish");
    }
    let selected = selected_phase.as_str();
    let skip = if selected == "focus" {
        if cadence.today % 4 == 3 {
            "long_break"
        } else {
            "short_break"
        }
    } else {
        "focus"
    };
    let next_break = if cadence.today > 0 && cadence.today % 4 == 0 {
        "long_break"
    } else {
        "short_break"
    };
    let task_counts = task_counts(workspace, history, cadence.day)?;
    Ok(
        json!({"schemaVersion": 1, "canonical": canonical, "display": display,
        "availableIntents": available,
        "cadence": {"completedFocusToday": cadence.today,
            "completedFocusTodayPlannedDurationMs": cadence.planned_duration_today_ms,
            "completedFocusTotal": cadence.total,
            "longBreakProgress": if cadence.today == 0 { 0 } else { (cadence.today - 1) % 4 + 1 },
            "nextCompletedFocusBreakPhase": next_break, "skipDestination": skip},
        "tasks": {"total": workspace["tasks"].as_array().unwrap().len(),
            "selectedTaskId": workspace["selectedTaskId"], "completedFocusTodayByTask": task_counts}}),
    )
}

fn display(
    input: &Input,
    workspace: &Value,
    canonical: &TimerView,
    selected: &str,
) -> Result<Value, CoreError> {
    let duration = workspace["durationsMs"][selected]
        .as_i64()
        .ok_or_else(|| invalid("missing selected phase duration"))?;
    let status = canonical.status.as_str();
    let reset = matches!(status, "idle" | "completed")
        || input.profile == Profile::AppleWorkspace && matches!(status, "cancelled" | "superseded");
    let display_status = if reset && input.profile == Profile::AppleWorkspace {
        "idle"
    } else {
        status
    };
    Ok(if reset {
        json!({"phase": selected, "status": display_status, "plannedDurationMs": duration,
            "elapsedMs": 0.0, "remainingMs": duration as f64, "progress": 0.0,
            "remainingSecondsCeil": (duration + 999) / 1000})
    } else {
        let elapsed = if status == "cancelled"
            && matches!(
                input.profile,
                Profile::DesktopStorage | Profile::DesktopTerminal
            ) {
            0.0
        } else {
            canonical.elapsed_ms
        };
        let remaining = canonical.planned_duration_ms as f64 - elapsed;
        json!({"phase": canonical.phase, "status": status,
            "plannedDurationMs": canonical.planned_duration_ms, "elapsedMs": elapsed,
            "remainingMs": remaining, "progress": elapsed / canonical.planned_duration_ms as f64,
            "remainingSecondsCeil": (remaining / 1000.0).ceil() as i64})
    })
}

fn natural_source<'a>(
    input: &Input,
    workspace: &Value,
    history: &'a [HistoryItem],
) -> Result<Option<&'a HistoryItem>, CoreError> {
    if input.profile != Profile::PwaStorage {
        return Ok(None);
    }
    let super::Source::Workspace(raw) = &input.source;
    if crate::timer::workspace::missing_natural_history(raw, &workspace["canonicalTimer"]) {
        return Ok(None);
    }
    let timer: Option<crate::timer::CanonicalTimer> =
        serde_json::from_value(workspace["canonicalTimer"].clone())?;
    crate::timer::workspace::natural_completion(timer.as_ref(), history)
}

fn retained_finish(input: &Input, row: &HistoryItem) -> bool {
    let super::Source::Workspace(workspace) = &input.source;
    workspace["local"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["type"] == "finish" && command["timerId"] == row.timer_id)
}

fn natural_phase(
    input: &Input,
    source: Option<&HistoryItem>,
    history: &[HistoryItem],
    day: (DateTime<Utc>, DateTime<Utc>),
) -> Result<String, CoreError> {
    let Some(source) = source.filter(|row| {
        row.phase == input.selected_phase
            && !input
                .selection
                .as_ref()
                .is_some_and(|selection| selection.explicit)
            && !input.lifecycle.consumed(&row.timer_id, &row.phase)
    }) else {
        return Ok(input.selected_phase.clone());
    };
    crate::completion_plan::phase_after(&source.phase, history, day)
}

fn task_counts(
    workspace: &Value,
    history: &[HistoryItem],
    day: (DateTime<Utc>, DateTime<Utc>),
) -> Result<Value, CoreError> {
    let mut counts = serde_json::Map::new();
    for task in workspace["tasks"]
        .as_array()
        .ok_or_else(|| invalid("missing read model tasks"))?
    {
        let id = task["id"]
            .as_str()
            .ok_or_else(|| invalid("invalid read model task"))?;
        counts.insert(id.into(), json!({"count": 0, "plannedDurationMs": 0}));
    }
    for item in history
        .iter()
        .filter(|item| item.status == "completed" && item.phase == "focus")
    {
        let at = item
            .completed_at
            .as_deref()
            .ok_or_else(|| invalid("missing task completion time"))?;
        let at = crate::timer::parse_time(at)?;
        if at < day.0 || at >= day.1 {
            continue;
        }
        if let Some(count) = item.task_id.as_ref().and_then(|id| counts.get_mut(id)) {
            count["count"] = json!(count["count"].as_u64().unwrap() + 1);
            count["plannedDurationMs"] =
                json!(count["plannedDurationMs"].as_i64().unwrap() + item.planned_duration_ms);
        }
    }
    Ok(Value::Object(counts))
}

fn intents(profile: Profile, status: &str) -> Vec<&'static str> {
    let mut available = Vec::new();
    match status {
        "running" => available.extend(["pause", "finish", "cancel", "cancelAndClear"]),
        "paused" => available.extend(["resume", "finish", "cancel", "cancelAndClear"]),
        "superseded" if profile == Profile::AndroidCoordinator => {
            available.extend(["start", "resume"])
        }
        "idle" | "completed" | "cancelled" | "superseded" => {
            if matches!(profile, Profile::DesktopStorage | Profile::DesktopTerminal)
                && status != "idle"
            {
                available.push("restart");
            } else {
                available.push("start");
            }
            if status != "idle"
                && profile != Profile::PwaStorage
                && !(profile == Profile::AndroidCoordinator && status == "superseded")
            {
                available.push("clear");
            }
        }
        _ => {}
    }
    if !matches!(status, "running" | "paused") || profile == Profile::AppleWorkspace {
        available.push("selectPhase");
    }
    if profile == Profile::AppleWorkspace && !matches!(status, "running" | "paused") {
        available.push("skip");
    }
    available
}
