use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

// R50-C03: overnight PWA reopen supplies only the current civil-day interval.
// The read model must render the overdue timer instead of throwing
// missing-read-model-calendar-interval for the source day it was not given.
// The platform owns civil calendar/DST; Core keeps UTC arithmetic.

fn workspace_with(timer: &Value, history: &Value) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/pwa-natural-completion-v1.json")).unwrap();
    let mut workspace = fixture["receipts"][0]["request"]["input"]["workspace"].clone();
    workspace["base"]["canonicalTimer"] = timer.clone();
    workspace["base"]["history"] = history.clone();
    workspace
}

fn running_timer(anchor: &str, elapsed: i64) -> Value {
    json!({"anchorAt": anchor, "elapsedAtAnchorMs": elapsed,
        "id": "overnight-timer", "phase": "focus", "plannedDurationMs": 60000,
        "startedByDeviceId": "device-a", "status": "running",
        "lastIntent": {"commandId": "cmd-start", "deviceId": "device-a",
            "occurredAt": anchor, "type": "start"}})
}

fn read(timer: &Value, history: &Value, at: &str, intervals: &Value) -> Result<Value, String> {
    let input = json!({"profile": "pwaStorage",
        "source": {"kind": "workspace", "value": workspace_with(timer, history)},
        "selectedPhase": "focus",
        "selection": {"phase": "focus", "generation": "0", "explicit": false},
        "lifecycle": {}, "observedAt": at, "calendarIntervals": intervals});
    dispatch_json("workspace.readModel.v1", &input.to_string())
        .map(|output| serde_json::from_str(&output).unwrap())
        .map_err(|error| error.to_string())
}

fn current_day_only() -> Value {
    json!([{"start": "2026-07-21T00:00:00Z", "end": "2026-07-22T00:00:00Z"}])
}

fn both_days() -> Value {
    json!([{"start": "2026-07-20T00:00:00Z", "end": "2026-07-21T00:00:00Z"},
        {"start": "2026-07-21T00:00:00Z", "end": "2026-07-22T00:00:00Z"}])
}

#[test]
fn overnight_reopen_with_only_current_day_renders_overdue_timer() {
    let timer = running_timer("2026-07-20T12:00:00Z", 5000);
    let model = read(
        &timer,
        &json!([]),
        "2026-07-21T00:01:00Z",
        &current_day_only(),
    )
    .unwrap();
    assert_eq!(model["canonical"]["status"], "completed");
    assert_eq!(model["canonical"]["remainingMs"], 0.0);
    assert_eq!(model["display"]["phase"], "focus");
    assert_eq!(model["cadence"]["completedFocusToday"], 0);
    let intents = model["availableIntents"].as_array().unwrap();
    assert!(intents.contains(&json!("finish")));
}

#[test]
fn overnight_reopen_with_source_day_selects_break() {
    let timer = running_timer("2026-07-20T12:00:00Z", 5000);
    let model = read(&timer, &json!([]), "2026-07-21T00:01:00Z", &both_days()).unwrap();
    assert_eq!(model["canonical"]["status"], "completed");
    assert_eq!(model["display"]["phase"], "short_break");
    assert_eq!(model["cadence"]["completedFocusToday"], 0);
}

#[test]
fn dst_25h_overnight_current_day_only_renders() {
    let timer = running_timer("2026-11-01T12:00:00Z", 5000);
    let current = json!([{"start": "2026-11-02T05:00:00Z", "end": "2026-11-03T05:00:00Z"}]);
    let model = read(&timer, &json!([]), "2026-11-02T06:00:00Z", &current).unwrap();
    assert_eq!(model["canonical"]["status"], "completed");
    assert_eq!(model["display"]["phase"], "focus");
    let both = json!([{"start": "2026-11-01T04:00:00Z", "end": "2026-11-02T05:00:00Z"},
        {"start": "2026-11-02T05:00:00Z", "end": "2026-11-03T05:00:00Z"}]);
    let model = read(&timer, &json!([]), "2026-11-02T06:00:00Z", &both).unwrap();
    assert_eq!(model["display"]["phase"], "short_break");
}

#[test]
fn overnight_with_prior_day_history_counts_today_zero_total_two() {
    // Prior-day completed row plus the overnight timer's own natural
    // completion (completed on the source day): total counts both, today
    // counts neither. Proves the history row survives projection and the
    // day boundary splits total vs today.
    let timer = running_timer("2026-07-20T12:00:00Z", 5000);
    let history = json!([{"id": "history-prior", "timerId": "timer-prior",
        "phase": "focus", "status": "completed", "plannedDurationMs": 60000,
        "completedAt": "2026-07-20T11:00:00Z"}]);
    let model = read(
        &timer,
        &history,
        "2026-07-21T00:01:00Z",
        &current_day_only(),
    )
    .unwrap();
    assert_eq!(model["canonical"]["status"], "completed");
    assert_eq!(model["display"]["phase"], "focus");
    assert_eq!(model["cadence"]["completedFocusTotal"], 2);
    assert_eq!(model["cadence"]["completedFocusToday"], 0);
}
