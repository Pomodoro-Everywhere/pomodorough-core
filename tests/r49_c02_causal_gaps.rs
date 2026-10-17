//! R49-C02 causal-gap hardening for the shared focus-count fix.
//!
//! Scope is hardening, not bug repro. `endedAt-varies`, `moved-out`,
//! `non-focus`, and `empty-tie` are preservation controls: their complete
//! native envelopes are byte-identical on official Core v0.48.0. Each test
//! also carries one planner variant that is red on official, proving the
//! causal count: `exclusion-plus-fifth` (official `long_break`),
//! `non-focus-plus-fifth` (official `short_break`), `unicode-tie`
//! (official `short_break`). Reconciler envelopes are byte-identical on
//! official for every scenario below; the pre-existing
//! `r49_c02_causal_break` gate carries reconciler red-cover.
//! Every scenario asserts complete native envelopes on both operations plus
//! cross-operation phase agreement, so a regression that keeps the phase
//! while corrupting history order, promotion/drop sets, durations, or
//! suppression fails here.
use pomodorough_core::dispatch_envelope_json;
use serde_json::{Value, json};

fn completion(index: usize, at: &str) -> Value {
    json!({"id": format!("timer-{index}"), "timerId": format!("timer-{index}"),
        "commandId": format!("finish-{index}"), "phase": "focus", "status": "completed",
        "plannedDurationMs": 60000, "completedAt": at, "endedAt": at})
}

fn base_history() -> Vec<Value> {
    (1..=4)
        .map(|n| completion(n, &format!("2026-07-20T{:02}:00:00Z", n + 8)))
        .collect()
}

fn source_timer() -> Value {
    json!({"id": "timer-4", "phase": "focus", "status": "completed", "plannedDurationMs": 60000,
        "elapsedAtAnchorMs": 60000, "anchorAt": "2026-07-20T12:00:00Z",
        "lastIntent": {"type": "finish", "commandId": "finish-4", "occurredAt": "2026-07-20T12:00:00Z"}})
}

fn planner(history: &[Value]) -> Value {
    let projection = json!({"canonicalTimer": source_timer(), "history": history});
    json!({"kind": "generatedBreak", "source": {"commandId": "finish-4", "timerId": "timer-4"},
        "canonical": projection, "optimistic": projection, "sourceFinishPending": false,
        "requireCanonical": true, "dayStart": "2026-07-20T00:00:00Z", "dayEnd": "2026-07-21T00:00:00Z"})
}

fn planner_expected(phase: &str) -> Value {
    json!({"expired": false, "commandEligible": false, "reserveGeneratedBreak": false,
        "selectedPhase": null, "queueAutoBreak": false, "generatedBreakEligible": true,
        "generatedBreakPhase": phase, "sourceAlreadyAccepted": true})
}

fn assert_planner(name: &str, history: &[Value], phase: &str) {
    let raw = dispatch_envelope_json("timer.completionPlan.v1", &planner(history).to_string());
    let actual: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        actual,
        json!({"ok": true, "value": planner_expected(phase)}),
        "{name}"
    );
}

fn source_command() -> Value {
    json!({"id": "finish-4", "deviceId": "device-a", "deviceSequence": 1, "timerId": "timer-4",
        "type": "finish", "phase": "focus", "plannedDurationMs": 60000, "observedElapsedMs": 60000,
        "occurredAt": "2026-07-20T12:00:00Z", "hlcWallMs": 1784500800000_i64, "hlcCounter": 0})
}

fn rebase_input(history: Vec<Value>) -> Value {
    let wall = chrono::DateTime::parse_from_rfc3339("2026-07-21T00:00:00Z")
        .unwrap()
        .timestamp_millis();
    let cmd = source_command();
    let mut start = cmd.clone();
    for (key, value) in [
        ("id", json!("start-break")),
        ("timerId", json!("break-a")),
        ("type", json!("start")),
        ("phase", json!("short_break")),
        ("deviceSequence", json!(2)),
        ("hlcCounter", json!(1)),
        ("observedElapsedMs", json!(0)),
    ] {
        start[key] = value;
    }
    json!({"local": {"commands": [cmd.clone(), start], "taskOperations": [],
        "durationOperations": [], "autoStartOperations": [], "selectedTaskOperations": []},
        "sent": {"commands": [cmd], "taskOperations": [], "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []},
        "neverSent": {"commands": ["start-break"]},
        "timerDependencies": [{"operationId": "start-break", "dependsOnOperationId": "finish-4",
            "generatedBreak": true, "sourceDayStart": "2026-07-20T00:00:00Z",
            "sourceDayEnd": "2026-07-21T00:00:00Z"}],
        "response": {"acknowledgements": [{"commandId": "finish-4", "outcome": "applied"}],
        "taskAcknowledgements": [], "durationAcknowledgements": [], "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [], "revision": 1, "canonicalTimer": source_timer(),
        "history": history, "tasks": [], "selectedTaskId": null,
        "durationsMs": {"focus": 60000, "short_break": 120000, "long_break": 180000},
        "autoStartBreaks": true, "serverTime": "2026-07-21T00:00:00Z",
        "serverHlcWallMs": wall, "serverHlcCounter": 0}})
}

fn utc_time(timestamp: &str) -> String {
    let time = chrono::DateTime::parse_from_rfc3339(timestamp)
        .unwrap()
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true);
    if let Some((whole, fraction)) = time.strip_suffix('Z').unwrap().split_once('.') {
        format!("{whole}.{}Z", fraction.trim_end_matches('0'))
    } else {
        time
    }
}

// Preserved projection shape, byte-identical on official v0.48.0 for every
// scenario below: rows are permuted by the literal `order`, timestamps are
// normalized to canonical UTC text, and a row carrying `completedAt` projects
// that instant as `endedAt`. Rows without completion keep `endedAt`. The
// count/sort/filter decision under test is never recomputed here.
fn projected_history(input: &[Value], order: &[usize]) -> Vec<Value> {
    order
        .iter()
        .map(|&index| {
            let mut row = input[index].clone();
            for field in ["completedAt", "endedAt"] {
                if let Some(stamp) = row[field].as_str() {
                    row[field] = json!(utc_time(stamp));
                }
            }
            if row["completedAt"].is_string() {
                row["endedAt"] = row["completedAt"].clone();
            }
            row
        })
        .collect()
}

fn reconcile_expected(input: &Value, history: Vec<Value>, phase: &str) -> Value {
    let response = &input["response"];
    let mut timer = response["canonicalTimer"].clone();
    timer["anchorAt"] = json!(utc_time(
        response["canonicalTimer"]["anchorAt"].as_str().unwrap()
    ));
    let mut start = input["local"]["commands"][1].clone();
    start["phase"] = json!(phase);
    start["plannedDurationMs"] = response["durationsMs"][phase].clone();
    let workspace = json!({"canonicalTimer": timer, "history": history,
        "tasks": [], "durationsMs": response["durationsMs"], "autoStartBreaks": true,
        "selectedTaskId": null, "timerOutcomes": {},
        "winningOperationIds": {"tasks": {}, "durations": {}, "autoStart": null, "selectedTask": null}});
    json!({"schemaVersion": 3, "revision": 1, "pending": [start],
        "pendingTaskOperations": [], "pendingDurationOperations": [], "pendingAutoStartOperations": [],
        "pendingSelectedTaskOperations": [], "pendingTimerDependencies": [],
        "promotedTimerOperationIds": ["start-break"], "droppedTimerOperationIds": [],
        "droppedTimerIds": [], "baseTimer": response["canonicalTimer"], "baseHistory": response["history"],
        "baseTasks": [], "baseDurationsMs": response["durationsMs"], "baseAutoStartBreaks": true,
        "baseSelectedTaskId": null, "timer": workspace["canonicalTimer"], "history": workspace["history"],
        "tasks": [], "durationsMs": response["durationsMs"], "autoStartBreaks": true,
        "selectedTaskId": null, "projectionPending": {"commands": [], "taskOperations": [],
        "durationOperations": [], "autoStartOperations": [], "selectedTaskOperations": []},
        "canonicalResponse": response, "workspace": workspace})
}

fn assert_rebase(name: &str, input: &Value, order: &[usize], phase: &str) -> Value {
    let history = projected_history(input["response"]["history"].as_array().unwrap(), order);
    let raw = dispatch_envelope_json("reconcile.rebase.v3", &input.to_string());
    let actual: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        actual,
        json!({"ok": true, "value": reconcile_expected(input, history, phase)}),
        "{name}"
    );
    actual["value"].clone()
}

fn assert_both(name: &str, history: Vec<Value>, order: &[usize], phase: &str) {
    assert_planner(name, &history, phase);
    let input = rebase_input(history);
    let result = assert_rebase(name, &input, order, phase);
    assert_eq!(
        result["pending"][0]["phase"],
        json!(phase),
        "{name}: cross-operation phase"
    );
    assert_eq!(
        result["pending"][0]["plannedDurationMs"], input["response"]["durationsMs"][phase],
        "{name}: cross-operation duration"
    );
}

#[test]
fn completed_at_takes_precedence_over_differing_ended_at() {
    let mut history = base_history();
    history[0]["endedAt"] = json!("2026-07-21T01:00:00Z");
    history[1]["endedAt"] = json!("2026-07-19T23:00:00Z");
    assert_both("endedAt-varies", history, &[3, 2, 1, 0], "long_break");
    let mut history = base_history();
    history[0]["completedAt"] = json!("2026-07-19T10:00:00Z");
    assert_both("moved-out", history, &[3, 2, 1, 0], "short_break");
    // Baseline-red on official (planner `long_break`): the out-of-day row is
    // excluded by `completedAt` despite its in-day `endedAt`, and the later
    // fifth focus exposes the causal count (source third, not fourth).
    let mut history = base_history();
    history[0]["completedAt"] = json!("2026-07-19T10:00:00Z");
    history[0]["endedAt"] = json!("2026-07-20T09:00:00Z");
    history.push(completion(5, "2026-07-20T13:00:00Z"));
    assert_both(
        "exclusion-plus-fifth",
        history,
        &[4, 3, 2, 1, 0],
        "short_break",
    );
}

#[test]
fn later_non_focus_rows_do_not_change_source_phase() {
    let history = non_focus_history();
    assert_both("non-focus", history, &[6, 5, 4, 3, 2, 1, 0], "long_break");
    // Baseline-red on official (planner `short_break`): non-focus rows stay
    // excluded while the later fifth focus proves the source is still fourth
    // under the causal count instead of fifth under the whole-day count.
    let mut history = non_focus_history();
    history.push(completion(5, "2026-07-20T16:00:00Z"));
    assert_both(
        "non-focus-plus-fifth",
        history,
        &[7, 6, 5, 4, 3, 2, 1, 0],
        "long_break",
    );
}

fn non_focus_history() -> Vec<Value> {
    let mut history = base_history();
    history.push(
        json!({"id": "timer-break", "timerId": "timer-break", "phase": "short_break",
        "status": "completed", "plannedDurationMs": 120000,
        "completedAt": "2026-07-20T13:00:00Z", "endedAt": "2026-07-20T13:00:00Z"}),
    );
    history.push(
        json!({"id": "timer-cancel", "timerId": "timer-cancel", "phase": "focus",
        "status": "cancelled", "plannedDurationMs": 60000, "endedAt": "2026-07-20T14:00:00Z"}),
    );
    history.push(
        json!({"id": "timer-super", "timerId": "timer-super", "phase": "focus",
        "status": "superseded", "plannedDurationMs": 60000, "endedAt": "2026-07-20T15:00:00Z"}),
    );
    history
}

#[test]
fn empty_and_unicode_tie_keys_sort_deterministically() {
    let mut history = base_history();
    let mut other = completion(5, "2026-07-20T12:00:00Z");
    other["commandId"] = json!("");
    history.push(other);
    assert_both("empty-tie", history, &[3, 4, 2, 1, 0], "short_break");
    // Baseline-red on official (planner `short_break`): the Unicode key sorts
    // after the source, so the source stays fourth under timestamp/identity
    // order instead of falling fifth under the whole-day count.
    let mut history = base_history();
    let mut other = completion(5, "2026-07-20T12:00:00Z");
    other["commandId"] = json!("finish-é-9");
    history.push(other);
    assert_both("unicode-tie", history, &[3, 4, 2, 1, 0], "long_break");
}
