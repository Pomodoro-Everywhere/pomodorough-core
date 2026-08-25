use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const SERVER_WALL_MS: i64 = 1_784_548_800_000;
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

fn empty_queues() -> Value {
    json!({
        "commands": [],
        "taskOperations": [],
        "durationOperations": [],
        "autoStartOperations": [],
        "selectedTaskOperations": []
    })
}

fn clock(id: &str) -> Value {
    json!({
        "id": id,
        "deviceId": "device-a",
        "occurredAt": "2026-07-20T12:00:01Z",
        "hlcWallMs": SERVER_WALL_MS + 1_000,
        "hlcCounter": 0
    })
}

fn response() -> Value {
    json!({
        "acknowledgements": [],
        "taskAcknowledgements": [],
        "durationAcknowledgements": [],
        "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [],
        "revision": 1,
        "canonicalTimer": null,
        "history": [],
        "tasks": [],
        "durationsMs": {
            "focus": 1_500_000,
            "short_break": 300_000,
            "long_break": 900_000
        },
        "autoStartBreaks": false,
        "selectedTaskId": null,
        "serverTime": "2026-07-20T12:00:10Z",
        "serverHlcWallMs": SERVER_WALL_MS + 10_000,
        "serverHlcCounter": 0
    })
}

fn reconcile(local: Value, response: Value) -> Result<Value, String> {
    let input = json!({"local": local, "sent": empty_queues(), "response": response});
    dispatch_json("reconcile.rebase.v1", &input.to_string())
        .map_err(|error| error.to_string())
        .and_then(|encoded| serde_json::from_str(&encoded).map_err(|error| error.to_string()))
}

fn operation_for(field: &str) -> Value {
    let mut operation = clock(field);
    match field {
        "taskOperations" => {
            operation["taskId"] = json!("task-a");
            operation["type"] = json!("delete");
            operation["title"] = json!("");
        }
        "durationOperations" => {
            operation["phase"] = json!("focus");
            operation["durationMs"] = json!(1_800_000);
        }
        "autoStartOperations" => operation["enabled"] = json!(true),
        "selectedTaskOperations" => operation["taskId"] = Value::Null,
        _ => unreachable!(),
    }
    operation
}

#[test]
fn reconcile_rejects_invalid_retained_clocks_in_every_operation_queue() {
    for field in [
        "taskOperations",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ] {
        let mut operation = operation_for(field);
        operation["deviceId"] = json!("");
        let mut local = empty_queues();
        local[field] = json!([operation]);

        let error = reconcile(local, response()).unwrap_err();
        assert!(
            error.contains("invalid retained operation clock"),
            "{field} returned {error}"
        );
    }
}

#[test]
fn reconcile_rejects_malformed_occurrence_when_rebasing_a_stale_clock() {
    let mut operation = operation_for("taskOperations");
    operation["occurredAt"] = json!("not-a-timestamp");
    let mut local = empty_queues();
    local["taskOperations"] = json!([operation]);

    let error = reconcile(local, response()).unwrap_err();
    assert!(
        error.contains("not-a-timestamp"),
        "unexpected error: {error}"
    );
}

#[test]
fn reconcile_rolls_an_exhausted_server_counter_into_the_next_millisecond() {
    let mut local = empty_queues();
    local["autoStartOperations"] = json!([operation_for("autoStartOperations")]);
    let mut canonical = response();
    canonical["serverHlcCounter"] = json!(MAX_SAFE_INTEGER);

    let output = reconcile(local, canonical).unwrap();
    assert_eq!(
        output["pendingAutoStartOperations"][0]["hlcWallMs"],
        SERVER_WALL_MS + 10_001
    );
    assert_eq!(output["pendingAutoStartOperations"][0]["hlcCounter"], 0);
    assert_eq!(
        output["pendingAutoStartOperations"][0]["occurredAt"],
        "2026-07-20T12:00:01Z"
    );
}

#[test]
fn reconcile_rejects_unsafe_canonical_projection_boundaries() {
    let mut unknown_selection = response();
    unknown_selection["selectedTaskId"] = json!("missing-task");

    let mut duplicate_tasks = response();
    duplicate_tasks["tasks"] = json!([
        {"id": "duplicate", "title": "First"},
        {"id": "duplicate", "title": "Second"}
    ]);

    let mut missing_duration = response();
    missing_duration["durationsMs"]
        .as_object_mut()
        .unwrap()
        .remove("long_break");

    let mut fractional_duration = response();
    fractional_duration["durationsMs"]["focus"] = json!(60_001);

    let mut overflowing_clock_delta = response();
    overflowing_clock_delta["serverHlcWallMs"] = json!(i64::MIN);

    for (canonical, expected) in [
        (unknown_selection, "selectedTaskId"),
        (duplicate_tasks, "tasks"),
        (missing_duration, "durationsMs"),
        (fractional_duration, "durationsMs"),
        (overflowing_clock_delta, "server HLC"),
    ] {
        let error = reconcile(empty_queues(), canonical).unwrap_err();
        assert!(
            error.contains(expected),
            "expected {expected:?}, got {error}"
        );
    }
}
