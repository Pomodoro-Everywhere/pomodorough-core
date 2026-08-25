use pomodorough_core::{classify_selected_task_field_json, dispatch_json};
use serde_json::{Value, json};

fn valid_timer_command() -> Value {
    json!({
        "id": "command-valid",
        "deviceId": "device-valid",
        "deviceSequence": 1,
        "timerId": "timer-valid",
        "type": "start",
        "phase": "focus",
        "plannedDurationMs": 60_000,
        "occurredAt": "2026-08-25T12:00:00Z",
        "hlcWallMs": 1,
        "hlcCounter": 0,
        "observedElapsedMs": 0
    })
}

fn timer_reduction(command: Value) -> Result<String, String> {
    dispatch_json(
        "timer.reduce.v1",
        &json!({"commands": [command], "now": "2026-08-25T12:00:01Z"}).to_string(),
    )
    .map_err(|error| error.to_string())
}

#[test]
fn selected_task_classification_rejects_non_string_wire_values() {
    for input in [
        r#"{"selectedTaskId":false}"#,
        r#"{"selectedTaskId":7}"#,
        r#"{"selectedTaskId":{}}"#,
        r#"{"selectedTaskId":[]}"#,
    ] {
        assert!(
            classify_selected_task_field_json(input).is_err(),
            "accepted {input}"
        );
    }
}

#[test]
fn selected_task_classification_rejects_malformed_or_trailing_json() {
    for input in [
        r#"{"selectedTaskId":"unterminated}"#,
        r#"{"selectedTaskId":"task-0001"} {}"#,
        r#"{"selectedTaskId":"\ud800"}"#,
    ] {
        assert!(
            classify_selected_task_field_json(input).is_err(),
            "accepted {input}"
        );
    }
}

#[test]
fn timer_dispatch_rejects_each_invalid_command_boundary() {
    let mutations = [
        ("id", json!("")),
        ("deviceId", json!("")),
        ("timerId", json!("")),
        ("deviceSequence", json!(0)),
        ("deviceSequence", json!(9_007_199_254_740_992_i64)),
        ("phase", json!("rest")),
        ("plannedDurationMs", json!(59_999)),
        ("plannedDurationMs", json!(14_400_001)),
        ("hlcWallMs", json!(0)),
        ("hlcCounter", json!(-1)),
        ("observedElapsedMs", json!(9_007_199_254_740_992_i64)),
    ];
    for (field, value) in mutations {
        let mut command = valid_timer_command();
        command[field] = value;
        let error = timer_reduction(command).unwrap_err();
        assert!(error.contains("invalid timer command"), "{field}: {error}");
    }
}

fn canonical_timer() -> Value {
    json!({
        "id": "timer-canonical",
        "phase": "focus",
        "status": "running",
        "plannedDurationMs": 60_000,
        "elapsedAtAnchorMs": 0,
        "anchorAt": "2026-08-25T12:00:00Z"
    })
}

fn replay_state(canonical: Value, history: Value) -> Result<String, String> {
    dispatch_json(
        "timer.reduce.v1",
        &json!({
            "commands": [],
            "canonicalTimer": canonical,
            "history": history,
            "now": "2026-08-25T12:00:01Z"
        })
        .to_string(),
    )
    .map_err(|error| error.to_string())
}

#[test]
fn timer_dispatch_rejects_each_invalid_canonical_timer_boundary() {
    let mutations = [
        ("id", json!("")),
        ("phase", json!("rest")),
        ("status", json!("unknown")),
        ("plannedDurationMs", json!(59_999)),
        ("elapsedAtAnchorMs", json!(-1)),
        ("elapsedAtAnchorMs", json!(60_001)),
        ("anchorAt", json!("not-a-time")),
    ];
    for (field, value) in mutations {
        let mut timer = canonical_timer();
        timer[field] = value;
        let error = replay_state(timer, json!([])).unwrap_err();
        assert!(
            error.contains("invalid canonical timer"),
            "{field}: {error}"
        );
    }

    for (field, value) in [
        ("type", json!("")),
        ("commandId", json!("")),
        ("occurredAt", json!("not-a-time")),
    ] {
        let mut timer = canonical_timer();
        timer["lastIntent"] = json!({
            "type": "start",
            "commandId": "command-start",
            "occurredAt": "2026-08-25T12:00:00Z"
        });
        timer["lastIntent"][field] = value;
        let error = replay_state(timer, json!([])).unwrap_err();
        assert!(
            error.contains("invalid canonical timer intent"),
            "{field}: {error}"
        );
    }
}

fn completed_history() -> Value {
    json!({
        "id": "history-valid",
        "timerId": "timer-history",
        "phase": "focus",
        "status": "completed",
        "plannedDurationMs": 60_000,
        "completedAt": "2026-08-25T12:00:00Z"
    })
}

#[test]
fn timer_dispatch_rejects_each_invalid_history_boundary() {
    let mutations = [
        ("id", json!("")),
        ("timerId", json!("")),
        ("phase", json!("rest")),
        ("status", json!("running")),
        ("plannedDurationMs", json!(59_999)),
        ("completedAt", Value::Null),
        ("completedAt", json!("not-a-time")),
        ("endedAt", json!("not-a-time")),
    ];
    for (field, value) in mutations {
        let mut history = completed_history();
        history[field] = value;
        let error = replay_state(Value::Null, json!([history])).unwrap_err();
        assert!(error.contains("invalid timer history"), "{field}: {error}");
    }

    for duplicate_field in ["id", "timerId"] {
        let first = completed_history();
        let mut second = completed_history();
        second["id"] = json!("history-second");
        second["timerId"] = json!("timer-second");
        second[duplicate_field] = first[duplicate_field].clone();
        assert!(replay_state(Value::Null, json!([first, second])).is_err());
    }
}
