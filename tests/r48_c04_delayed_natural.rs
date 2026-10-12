use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const NATURAL_ID: &str = "2a6985a7-4922-4c04-86dd-0024cf84b5cd";
const AUG_INTERVALS: &str = "aug";
const DST_INTERVALS: &str = "dst";

fn base_workspace() -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/pwa-natural-completion-v1.json")).unwrap();
    fixture["receipts"][0]["request"]["input"]["workspace"].clone()
}

fn natural_timer() -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/pwa-natural-completion-v1.json")).unwrap();
    fixture["receipts"][0]["request"]["input"]["requestedTimer"].clone()
}

fn history_row(id: &str, at: &str) -> Value {
    json!({"id": id, "timerId": id, "phase": "focus", "status": "completed",
        "plannedDurationMs": 1500000, "completedAt": at, "endedAt": at})
}

fn four_history(prefix: &str, times: [&str; 4]) -> Value {
    let mut rows = vec![];
    for (i, at) in times.iter().enumerate().take(3) {
        rows.push(history_row(&format!("{prefix}-{i}"), at));
    }
    let mut natural = history_row(NATURAL_ID, times[3]);
    natural["id"] = json!(NATURAL_ID);
    rows.push(natural);
    json!(rows)
}

fn intervals(kind: &str) -> Value {
    if kind == DST_INTERVALS {
        json!([{"start": "2026-11-01T04:00:00Z", "end": "2026-11-02T05:00:00Z"},
            {"start": "2026-11-02T05:00:00Z", "end": "2026-11-03T05:00:00Z"}])
    } else {
        json!([{"start": "2026-08-31T00:00:00Z", "end": "2026-09-01T00:00:00Z"},
            {"start": "2026-09-01T00:00:00Z", "end": "2026-09-02T00:00:00Z"}])
    }
}

fn workspace_with(history: &Value) -> Value {
    let mut workspace = base_workspace();
    workspace["base"]["history"] = history.clone();
    workspace["base"]["canonicalTimer"] = natural_timer();
    workspace
}

fn read(workspace: &Value, selection: &Value, lifecycle: &Value, at: &str, kind: &str) -> Value {
    let input = json!({"profile": "pwaStorage",
        "source": {"kind": "workspace", "value": workspace},
        "selectedPhase": selection["phase"], "selection": selection,
        "lifecycle": lifecycle, "observedAt": at, "calendarIntervals": intervals(kind)});
    serde_json::from_str(&dispatch_json("workspace.readModel.v1", &input.to_string()).unwrap())
        .unwrap()
}

fn install(history: &Value, selection: &Value, lifecycle: &Value, at: &str, kind: &str) -> Value {
    let input = json!({"kind": "install", "compatibility": "pwaRejectedFinish",
        "beforeHistory": [], "afterHistory": history,
        "canonicalTimer": natural_timer(), "selection": selection,
        "lifecycle": lifecycle, "advances": [], "acknowledgements": [],
        "discardedCommandIds": [], "referenceTime": at,
        "calendarIntervals": intervals(kind),
        "pending": {"commandIds": [], "sendableCommandIds": [], "otherOperationIds": []},
        "sentContext": {"kind": "pwa", "commands": [], "rollbackHistory": []}});
    serde_json::from_str(&dispatch_json("timer.completionState.v1", &input.to_string()).unwrap())
        .unwrap()
}

fn implicit() -> Value {
    json!({"phase": "focus", "generation": "0", "explicit": false})
}

#[test]
fn delayed_overnight_read_matches_install_long_break() {
    let history = four_history(
        "prior",
        [
            "2026-08-31T09:00:00Z",
            "2026-08-31T10:00:00Z",
            "2026-08-31T11:00:00Z",
            "2026-08-31T12:25:00Z",
        ],
    );
    let workspace = workspace_with(&history);
    let at = "2026-09-01T00:01:00Z";
    let model = read(&workspace, &implicit(), &json!({}), at, AUG_INTERVALS);
    let state = install(&history, &implicit(), &json!({}), at, AUG_INTERVALS);
    assert_eq!(model["cadence"]["completedFocusTotal"], 4);
    assert_eq!(model["cadence"]["completedFocusToday"], 0);
    assert_eq!(state["reason"], "completionSelected");
    assert_eq!(state["selection"]["phase"], "long_break");
    assert_eq!(model["display"]["phase"], state["selection"]["phase"]);
}

#[test]
fn same_day_read_matches_install_long_break() {
    let history = four_history(
        "same",
        [
            "2026-08-31T09:00:00Z",
            "2026-08-31T10:00:00Z",
            "2026-08-31T11:00:00Z",
            "2026-08-31T12:25:00Z",
        ],
    );
    let workspace = workspace_with(&history);
    let at = "2026-08-31T12:26:00Z";
    let model = read(&workspace, &implicit(), &json!({}), at, AUG_INTERVALS);
    let state = install(&history, &implicit(), &json!({}), at, AUG_INTERVALS);
    assert_eq!(model["cadence"]["completedFocusToday"], 4);
    assert_eq!(model["cadence"]["completedFocusTotal"], 4);
    assert_eq!(state["selection"]["phase"], "long_break");
    assert_eq!(model["display"]["phase"], state["selection"]["phase"]);
}

#[test]
fn explicit_choice_preserved_in_delayed_read_and_install() {
    let history = four_history(
        "exp",
        [
            "2026-08-31T09:00:00Z",
            "2026-08-31T10:00:00Z",
            "2026-08-31T11:00:00Z",
            "2026-08-31T12:25:00Z",
        ],
    );
    let workspace = workspace_with(&history);
    let at = "2026-09-01T00:01:00Z";
    let selection = json!({"phase": "focus", "generation": "7", "explicit": true});
    let model = read(&workspace, &selection, &json!({}), at, AUG_INTERVALS);
    let state = install(&history, &selection, &json!({}), at, AUG_INTERVALS);
    assert_eq!(model["display"]["phase"], "focus");
    assert_eq!(state["selection"], selection);
    assert_eq!(state["reason"], "explicitSelection");
    assert_eq!(model["display"]["phase"], state["selection"]["phase"]);
}

#[test]
fn consumed_natural_preserved_in_delayed_read_and_install() {
    let history = four_history(
        "con",
        [
            "2026-08-31T09:00:00Z",
            "2026-08-31T10:00:00Z",
            "2026-08-31T11:00:00Z",
            "2026-08-31T12:25:00Z",
        ],
    );
    let workspace = workspace_with(&history);
    let at = "2026-09-01T00:01:00Z";
    let lifecycle = json!({"consumedCompletions":
        [{"timerId": NATURAL_ID, "commandId": null, "phase": "focus"}],
        "pendingBreaks": []});
    let model = read(&workspace, &implicit(), &lifecycle, at, AUG_INTERVALS);
    let state = install(&history, &implicit(), &lifecycle, at, AUG_INTERVALS);
    assert_eq!(model["display"]["phase"], "focus");
    assert_eq!(state["selection"]["phase"], "focus");
    assert_eq!(model["display"]["phase"], state["selection"]["phase"]);
}

#[test]
fn dst_delayed_read_matches_install_long_break() {
    let history = four_history(
        "dst",
        [
            "2026-11-01T12:00:00Z",
            "2026-11-01T13:00:00Z",
            "2026-11-01T14:00:00Z",
            "2026-11-01T15:00:00Z",
        ],
    );
    let timer = json!({
        "anchorAt": "2026-11-01T15:00:00Z", "elapsedAtAnchorMs": 1500000,
        "id": NATURAL_ID,
        "lastIntent": {"commandId": "01a057b0-ce00-7000-8000-000000000015",
            "deviceId": "p222-device", "occurredAt": "2026-11-01T14:35:00Z", "type": "start"},
        "phase": "focus", "plannedDurationMs": 1500000,
        "startedByDeviceId": "p222-device", "status": "completed"});
    let mut workspace = workspace_with(&history);
    workspace["base"]["canonicalTimer"] = timer.clone();
    let at = "2026-11-02T06:00:00Z";
    let model = read(&workspace, &implicit(), &json!({}), at, DST_INTERVALS);
    let input = json!({"kind": "install", "compatibility": "pwaRejectedFinish",
        "beforeHistory": [], "afterHistory": history,
        "canonicalTimer": timer, "selection": implicit(),
        "lifecycle": {}, "advances": [], "acknowledgements": [],
        "discardedCommandIds": [], "referenceTime": at,
        "calendarIntervals": intervals(DST_INTERVALS),
        "pending": {"commandIds": [], "sendableCommandIds": [], "otherOperationIds": []},
        "sentContext": {"kind": "pwa", "commands": [], "rollbackHistory": []}});
    let state: Value = serde_json::from_str(
        &dispatch_json("timer.completionState.v1", &input.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(model["cadence"]["completedFocusToday"], 0);
    assert_eq!(model["cadence"]["completedFocusTotal"], 4);
    assert_eq!(state["selection"]["phase"], "long_break");
    assert_eq!(model["display"]["phase"], state["selection"]["phase"]);
}
