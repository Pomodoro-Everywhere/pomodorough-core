use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn clock(id: &str) -> Value {
    json!({
        "id": id,
        "deviceId": "device-a",
        "occurredAt": "2026-08-22T12:00:00Z",
        "hlcWallMs": 2_000,
        "hlcCounter": 0
    })
}

fn valid_input() -> Value {
    json!({
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
            "taskOperations": [],
            "durationOperations": [],
            "autoStartOperations": [],
            "selectedTaskOperations": []
        },
        "now": "2026-08-22T12:00:00Z"
    })
}

fn assert_invalid(input: Value, expected: &str) {
    let error = dispatch_json("projection.apply.v2", &input.to_string()).unwrap_err();
    assert!(
        error.to_string().contains(expected),
        "expected {expected:?}, got {error}"
    );
}

#[test]
fn projection_apply_v2_rejects_inconsistent_canonical_base_state() {
    let mut invalid_task = valid_input();
    invalid_task["base"]["tasks"] = json!([{"id": "wrong-id", "title": "Task"}]);
    assert_invalid(invalid_task, "invalid base task identity or title");

    let mut empty_selection = valid_input();
    empty_selection["base"]["selectedTaskId"] = json!("");
    assert_invalid(empty_selection, "invalid base selected task identity");

    let mut missing_duration = valid_input();
    missing_duration["base"]["durationsMs"]
        .as_object_mut()
        .unwrap()
        .remove("long_break");
    assert_invalid(missing_duration, "invalid base durations");
}

#[test]
fn projection_apply_v2_rejects_malformed_pending_domain_values() {
    let mut empty_task_id = clock("task-empty");
    empty_task_id["taskId"] = json!("");
    empty_task_id["type"] = json!("delete");
    empty_task_id["title"] = json!("");

    let mut invalid_duration = clock("duration-invalid");
    invalid_duration["phase"] = json!("focus");
    invalid_duration["durationMs"] = json!(59_999);

    let mut omitted_selection = clock("selection-omitted");
    omitted_selection.as_object_mut().unwrap().remove("taskId");

    let mut invalid_auto_start_clock = clock("auto-invalid-clock");
    invalid_auto_start_clock["enabled"] = json!(true);
    invalid_auto_start_clock["deviceId"] = json!("");

    for (field, operation, expected) in [
        ("taskOperations", empty_task_id, "invalid task identity"),
        (
            "durationOperations",
            invalid_duration,
            "invalid duration operation",
        ),
        (
            "selectedTaskOperations",
            omitted_selection,
            "invalid selected task operation",
        ),
        (
            "autoStartOperations",
            invalid_auto_start_clock,
            "invalid operation clock",
        ),
    ] {
        let mut input = valid_input();
        input["pending"][field] = json!([operation]);
        assert_invalid(input, expected);
    }
}
