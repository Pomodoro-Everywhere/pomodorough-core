use pomodorough_core::dispatch_json;
use serde_json::json;

const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_COMMANDS: usize = 10_000;

fn command(id: usize) -> serde_json::Value {
    json!({
        "id": format!("cmd-{id}"),
        "deviceId": "device-a",
        "deviceSequence": 1,
        "timerId": "timer-a",
        "type": "start",
        "phase": "focus",
        "plannedDurationMs": 300_000,
        "occurredAt": "2026-07-15T10:00:00Z",
        "hlcWallMs": 1,
        "hlcCounter": 0,
        "observedElapsedMs": 0
    })
}

fn history_item(id: usize) -> serde_json::Value {
    json!({
        "id": format!("h-{id}"),
        "timerId": format!("t-{id}"),
        "phase": "focus",
        "status": "cancelled",
        "plannedDurationMs": 300_000,
        "endedAt": "2026-07-15T10:00:00Z"
    })
}

#[test]
fn dispatch_rejects_oversized_input_before_parsing() {
    let oversized = "x".repeat(MAX_INPUT_BYTES + 1);
    let error = dispatch_json("timer.reduce.v1", &oversized).unwrap_err();
    assert!(error.to_string().contains("exceeds"), "{error}");
}

#[test]
fn dispatch_rejects_oversized_operation() {
    let operation = "x".repeat(257);
    let error = dispatch_json(&operation, "{}").unwrap_err();
    assert!(error.to_string().contains("exceeds"), "{error}");
}

#[test]
fn timer_reduce_v1_rejects_too_many_commands() {
    let commands: Vec<_> = (0..MAX_COMMANDS + 1).map(command).collect();
    let input = json!({"commands": commands, "now": "2026-07-15T10:00:00Z"}).to_string();
    let error = dispatch_json("timer.reduce.v1", &input).unwrap_err();
    assert!(error.to_string().contains("commands exceed"), "{error}");
}

#[test]
fn timer_reduce_v1_rejects_too_many_history_items() {
    let history: Vec<_> = (0..MAX_COMMANDS + 1).map(history_item).collect();
    let input =
        json!({"commands": [], "history": history, "now": "2026-07-15T10:00:00Z"}).to_string();
    let error = dispatch_json("timer.reduce.v1", &input).unwrap_err();
    assert!(error.to_string().contains("history exceeds"), "{error}");
}

#[test]
fn timer_fixture_rejects_too_many_commands() {
    let commands: Vec<_> = (0..MAX_COMMANDS + 1)
        .map(|i| {
            json!({
                "id": format!("cmd-{i}"),
                "sequence": 1,
                "deviceId": "device-a",
                "timerId": "timer-a",
                "type": "start",
                "phase": "focus",
                "durationMs": 300_000,
                "atMs": 0,
                "wallMs": 1,
                "counter": 0,
                "elapsedMs": 0
            })
        })
        .collect();
    let input =
        json!({"epoch": "2026-07-15T10:00:00Z", "nowMs": 0, "commands": commands}).to_string();
    let error = dispatch_json("timer.reduce", &input).unwrap_err();
    assert!(error.to_string().contains("commands exceed"), "{error}");
}

#[test]
fn hlc_head_rejects_too_many_observed_clocks() {
    let observed: Vec<_> = (0..MAX_COMMANDS + 1)
        .map(|_| json!({"wallMs": 1, "counter": 0}))
        .collect();
    let input = json!({"physicalNowMs": 1, "observed": observed}).to_string();
    let error = dispatch_json("hlc.head.v1", &input).unwrap_err();
    assert!(error.to_string().contains("observed exceeds"), "{error}");
}

#[test]
fn bootstrap_rejects_too_many_history_entries() {
    let history: Vec<_> = (0..MAX_COMMANDS + 1)
        .map(|i| json!({"status": "completed", "timerId": format!("t-{i}")}))
        .collect();
    let input = json!({"localHistory": history, "remoteHistory": []}).to_string();
    let error = dispatch_json("bootstrap.plan.v1", &input).unwrap_err();
    assert!(error.to_string().contains("history exceeds"), "{error}");
}

#[test]
fn small_valid_inputs_still_pass_wire_compat() {
    let input = json!({
        "commands": [command(0)],
        "now": "2026-07-15T10:00:01Z",
    })
    .to_string();
    let output: serde_json::Value =
        serde_json::from_str(&dispatch_json("timer.reduce.v1", &input).unwrap()).unwrap();
    assert_eq!(output["canonicalTimer"]["status"], "running");

    let head: serde_json::Value = serde_json::from_str(
        &dispatch_json("hlc.head.v1", r#"{"physicalNowMs":5,"observed":[]}"#).unwrap(),
    )
    .unwrap();
    assert_eq!(head["wallMs"], 5);

    let plan: serde_json::Value = serde_json::from_str(
        &dispatch_json(
            "bootstrap.plan.v1",
            r#"{"localHistory":[],"remoteHistory":[]}"#,
        )
        .unwrap(),
    )
    .unwrap();
    assert!(plan.get("mode").is_some());
}
