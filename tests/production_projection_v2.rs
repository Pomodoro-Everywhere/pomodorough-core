use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn task_identity(title: &str) -> Value {
    serde_json::from_str(
        &dispatch_json("task.identity.v1", &json!({ "title": title }).to_string()).unwrap(),
    )
    .unwrap()
}

fn clock(id: &str, wall: i64) -> Value {
    json!({
        "id": id,
        "deviceId": "device-a",
        "occurredAt": "2026-08-22T12:00:00Z",
        "hlcWallMs": wall,
        "hlcCounter": 0
    })
}

#[test]
fn projection_apply_v2_reduces_base_and_pending_domains_together() {
    let base_task = task_identity("Base task");
    let pending_task = task_identity("Pending task");
    let mut upsert = clock("task-upsert", 2_000);
    upsert["taskId"] = pending_task["id"].clone();
    upsert["type"] = json!("upsert");
    upsert["title"] = pending_task["title"].clone();
    let mut duration = clock("duration-short", 2_001);
    duration["phase"] = json!("short_break");
    duration["durationMs"] = json!(600_000);
    let mut auto_start = clock("auto-enable", 2_002);
    auto_start["enabled"] = json!(true);
    let mut selection = clock("select-pending", 2_003);
    selection["taskId"] = pending_task["id"].clone();
    let mut start = clock("timer-start", 2_004);
    start["deviceSequence"] = json!(1);
    start["timerId"] = json!("timer-focus");
    start["type"] = json!("start");
    start["phase"] = json!("focus");
    start["plannedDurationMs"] = json!(1_200_000);
    start["observedElapsedMs"] = json!(0);
    start["taskId"] = pending_task["id"].clone();

    let input = json!({
        "base": {
            "canonicalTimer": null,
            "history": [],
            "tasks": [{ "id": base_task["id"], "title": base_task["title"] }],
            "durationsMs": {
                "focus": 1_200_000,
                "short_break": 300_000,
                "long_break": 900_000
            },
            "autoStartBreaks": false,
            "selectedTaskId": base_task["id"]
        },
        "pending": {
            "commands": [start],
            "taskOperations": [upsert],
            "durationOperations": [duration],
            "autoStartOperations": [auto_start],
            "selectedTaskOperations": [selection]
        },
        "now": "2026-08-22T12:00:00Z"
    });

    let output: Value =
        serde_json::from_str(&dispatch_json("projection.apply.v2", &input.to_string()).unwrap())
            .unwrap();

    assert_eq!(output["canonicalTimer"]["id"], "timer-focus");
    assert_eq!(output["canonicalTimer"]["status"], "running");
    assert_eq!(output["canonicalTimer"]["taskId"], pending_task["id"]);
    assert_eq!(output["history"], json!([]));
    assert_eq!(output["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(output["durationsMs"]["focus"], 1_200_000);
    assert_eq!(output["durationsMs"]["short_break"], 600_000);
    assert_eq!(output["autoStartBreaks"], true);
    assert_eq!(output["selectedTaskId"], pending_task["id"]);
    assert_eq!(
        output["winningOperationIds"]["selectedTask"],
        "select-pending"
    );
    assert_eq!(output["timerOutcomes"]["timer-start"]["outcome"], "applied");
}

#[test]
fn projection_apply_v2_rejects_malformed_operations_instead_of_ignoring_them() {
    let task = task_identity("Task");
    let mut malformed = clock("bad-task", 2_000);
    malformed["taskId"] = task["id"].clone();
    malformed["type"] = json!("rename");
    malformed["title"] = task["title"].clone();
    let input = json!({
        "base": {
            "canonicalTimer": null,
            "history": [],
            "tasks": [],
            "durationsMs": {
                "focus": 1_500_000,
                "short_break": 300_000,
                "long_break": 900_000
            },
            "autoStartBreaks": false,
            "selectedTaskId": null
        },
        "pending": {
            "commands": [],
            "taskOperations": [malformed],
            "durationOperations": [],
            "autoStartOperations": [],
            "selectedTaskOperations": []
        },
        "now": "2026-08-22T12:00:00Z"
    });

    let error = dispatch_json("projection.apply.v2", &input.to_string()).unwrap_err();
    assert!(error.to_string().contains("invalid task operation type"));
}
