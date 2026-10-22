use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

// R51-C01: the composed pipeline moves the full history by value
// (`take()`, no clone, no truncation) inside the WASM budget. This tracked
// test pins the native half: 50,000 valid rows render with exact totals.
// The WASM memory half is not assertable natively; it lives in scratch
// evidence (`check-R51-C01.md`, `/tmp/check50k.cjs`): 7.35 MB input,
// native/WASM raw parity, genuine oversize breach still hard-fails.

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/read-model-v1.json")).unwrap()
}

fn task_id() -> String {
    let identity =
        dispatch_json("task.identity.v1", &json!({"title": "Study"}).to_string()).unwrap();
    serde_json::from_str::<Value>(&identity).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn fifty_thousand_row_history_renders_without_truncation() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    let id = task_id();
    request["source"]["value"]["base"]["tasks"] = json!([{"id": id, "title": "Study"}]);
    let mut history = Vec::with_capacity(50_000);
    for index in 0..50_000 {
        history.push(
            json!({"id": format!("history-{index}"), "timerId": format!("timer-{index}"),
            "phase": "focus", "status": "completed", "plannedDurationMs": 1500000,
            "taskId": id, "completedAt": "2026-03-08T06:00:00Z"}),
        );
    }
    request["source"]["value"]["base"]["history"] = json!(history);
    let input = request.to_string();
    assert!(
        input.len() < 16 * 1024 * 1024,
        "input bytes={}",
        input.len()
    );
    let result: Value =
        serde_json::from_str(&dispatch_json("workspace.readModel.v1", &input).unwrap()).unwrap();
    assert_eq!(result["cadence"]["completedFocusTotal"], 50_000);
    assert_eq!(result["cadence"]["completedFocusToday"], 50_000);
    assert_eq!(
        result["cadence"]["completedFocusTodayPlannedDurationMs"]
            .as_i64()
            .unwrap(),
        75_000_000_000_i64
    );
    assert_eq!(
        result["tasks"]["completedFocusTodayByTask"][&id]["count"],
        50_000
    );
}
