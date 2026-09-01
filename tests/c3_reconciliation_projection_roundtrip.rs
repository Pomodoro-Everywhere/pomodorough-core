use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

const SERVER_TIME: &str = "2026-07-20T12:00:10Z";
const SERVER_WALL_MS: i64 = 1_784_548_810_000;

fn task(title: &str) -> Value {
    let input = json!({ "title": title }).to_string();
    let direct: Value =
        serde_json::from_str(&dispatch_json("task.identity.v1", &input).unwrap()).unwrap();
    let envelope: Value =
        serde_json::from_str(&dispatch_envelope_json("task.identity.v1", &input)).unwrap();
    assert_eq!(envelope, json!({ "ok": true, "value": direct }));
    direct
}

fn queues() -> Value {
    json!({
        "commands": [],
        "taskOperations": [],
        "durationOperations": [],
        "autoStartOperations": [],
        "selectedTaskOperations": []
    })
}

fn durations() -> Value {
    json!({
        "focus": 1_200_000,
        "short_break": 300_000,
        "long_break": 900_000
    })
}

fn response(tasks: Value) -> Value {
    json!({
        "acknowledgements": [],
        "taskAcknowledgements": [],
        "durationAcknowledgements": [],
        "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [],
        "revision": 7,
        "canonicalTimer": null,
        "history": [],
        "tasks": tasks,
        "durationsMs": durations(),
        "autoStartBreaks": false,
        "selectedTaskId": null,
        "serverTime": SERVER_TIME,
        "serverHlcWallMs": SERVER_WALL_MS,
        "serverHlcCounter": 0
    })
}

fn operation_clock(id: &str, wall_offset_ms: i64) -> Value {
    json!({
        "id": id,
        "deviceId": "device-c3",
        "occurredAt": "2026-07-20T12:00:11Z",
        "hlcWallMs": SERVER_WALL_MS + wall_offset_ms,
        "hlcCounter": 0
    })
}

fn rebase_input(local: Value, sent: Value, response: Value) -> Value {
    json!({
        "local": local,
        "sent": sent,
        "response": response,
        "timerDependencies": []
    })
}

fn projection_input(pending: Value) -> Value {
    json!({
        "base": {
            "canonicalTimer": null,
            "history": [],
            "tasks": [],
            "durationsMs": durations(),
            "autoStartBreaks": false,
            "selectedTaskId": null
        },
        "pending": pending,
        "now": SERVER_TIME
    })
}

fn queue_value(field: &str, id: &str) -> Value {
    let mut operation = operation_clock(id, 1_000);
    match field {
        "commands" => {
            operation["deviceSequence"] = json!(1);
            operation["timerId"] = json!("timer-c3");
            operation["type"] = json!("start");
            operation["phase"] = json!("focus");
            operation["plannedDurationMs"] = json!(1_200_000);
            operation["observedElapsedMs"] = json!(0);
        }
        "taskOperations" => {
            operation["taskId"] = json!("task-c3");
            operation["type"] = json!("delete");
            operation["title"] = json!("");
        }
        "durationOperations" => {
            operation["phase"] = json!("focus");
            operation["durationMs"] = json!(1_200_000);
        }
        "autoStartOperations" => operation["enabled"] = json!(true),
        "selectedTaskOperations" => operation["taskId"] = Value::Null,
        _ => unreachable!(),
    }
    operation
}

fn acknowledgement_fields(field: &str) -> (&str, &str) {
    match field {
        "commands" => ("acknowledgements", "commandId"),
        "taskOperations" => ("taskAcknowledgements", "operationId"),
        "durationOperations" => ("durationAcknowledgements", "operationId"),
        "autoStartOperations" => ("autoStartAcknowledgements", "operationId"),
        "selectedTaskOperations" => ("selectedTaskAcknowledgements", "operationId"),
        _ => unreachable!(),
    }
}

fn acknowledged_rebase_input(field: &str, operation: Value) -> Value {
    let id = operation["id"].as_str().unwrap().to_owned();
    let (response_field, id_field) = acknowledgement_fields(field);
    let mut local = queues();
    local[field] = json!([operation]);
    let mut sent = queues();
    sent[field] = json!([{ "id": id.clone() }]);
    let mut canonical = response(json!([]));
    canonical[response_field] = json!([{
        (id_field): id,
        "outcome": "applied",
        "reason": ""
    }]);
    rebase_input(local, sent, canonical)
}

fn assert_operation_rejects(operation: &str, input: &Value) {
    let encoded = input.to_string();
    let direct = dispatch_json(operation, &encoded).unwrap_err().to_string();
    let envelope: Value =
        serde_json::from_str(&dispatch_envelope_json(operation, &encoded)).unwrap();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"], direct);
}

fn assert_rebase_projection_reject(field: &str, operation: Value) {
    let mut pending = queues();
    pending[field] = json!([operation.clone()]);
    assert_operation_rejects("projection.apply.v2", &projection_input(pending));
    let mut local = queues();
    local[field] = json!([operation.clone()]);
    assert_operation_rejects(
        "reconcile.rebase.v1",
        &rebase_input(local, queues(), response(json!([]))),
    );
    assert_operation_rejects(
        "reconcile.rebase.v1",
        &acknowledged_rebase_input(field, operation),
    );
}

fn dispatch_both(operation: &str, input: &Value) -> Value {
    let encoded = input.to_string();
    let direct: Value = serde_json::from_str(&dispatch_json(operation, &encoded).unwrap()).unwrap();
    let envelope: Value =
        serde_json::from_str(&dispatch_envelope_json(operation, &encoded)).unwrap();
    assert_eq!(envelope, json!({ "ok": true, "value": direct }));
    direct
}

fn assert_both_reject(operation: &str, input: &Value, message: &str) {
    let encoded = input.to_string();
    let direct = dispatch_json(operation, &encoded).unwrap_err().to_string();
    let envelope: Value =
        serde_json::from_str(&dispatch_envelope_json(operation, &encoded)).unwrap();
    assert!(
        direct.contains(message),
        "unexpected direct error: {direct}"
    );
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"], direct);
}

fn projection_from_rebase(rebased: &Value) -> Value {
    json!({
        "base": {
            "canonicalTimer": rebased["baseTimer"],
            "history": rebased["baseHistory"],
            "tasks": rebased["baseTasks"],
            "durationsMs": rebased["baseDurationsMs"],
            "autoStartBreaks": rebased["baseAutoStartBreaks"],
            "selectedTaskId": rebased["baseSelectedTaskId"]
        },
        "pending": {
            "commands": rebased["pending"],
            "taskOperations": rebased["pendingTaskOperations"],
            "durationOperations": rebased["pendingDurationOperations"],
            "autoStartOperations": rebased["pendingAutoStartOperations"],
            "selectedTaskOperations": rebased["pendingSelectedTaskOperations"]
        },
        "now": SERVER_TIME
    })
}

fn assert_projection_matches_rebase(rebased: &Value) {
    let projected = dispatch_both("projection.apply.v2", &projection_from_rebase(rebased));
    for (rebase_field, projection_field) in [
        ("timer", "canonicalTimer"),
        ("history", "history"),
        ("tasks", "tasks"),
        ("durationsMs", "durationsMs"),
        ("autoStartBreaks", "autoStartBreaks"),
        ("selectedTaskId", "selectedTaskId"),
    ] {
        assert_eq!(rebased[rebase_field], projected[projection_field]);
    }
}

#[test]
fn c3_rebase_round_trips_through_projection_for_nested_task_references() {
    let alpha = task("Alpha");
    let beta = task("Beta");
    let mut local = queues();
    let mut upsert = operation_clock("task-beta", 1_000);
    upsert["taskId"] = beta["id"].clone();
    upsert["type"] = json!("upsert");
    upsert["title"] = beta["title"].clone();
    let mut selection = operation_clock("select-beta", 1_003);
    selection["taskId"] = beta["id"].clone();
    let mut duration = operation_clock("duration-short", 1_001);
    duration["phase"] = json!("short_break");
    duration["durationMs"] = json!(600_000);
    let mut auto_start = operation_clock("auto-start", 1_002);
    auto_start["enabled"] = json!(true);
    let mut start = operation_clock("timer-beta", 1_004);
    start["deviceSequence"] = json!(1);
    start["timerId"] = json!("timer-beta");
    start["type"] = json!("start");
    start["phase"] = json!("focus");
    start["plannedDurationMs"] = json!(1_200_000);
    start["observedElapsedMs"] = json!(0);
    start["taskId"] = beta["id"].clone();
    local["commands"] = json!([start]);
    local["taskOperations"] = json!([upsert]);
    local["durationOperations"] = json!([duration]);
    local["autoStartOperations"] = json!([auto_start]);
    local["selectedTaskOperations"] = json!([selection]);

    let input = rebase_input(
        local,
        queues(),
        response(json!([{ "id": alpha["id"], "title": alpha["title"] }])),
    );
    let rebased = dispatch_both("reconcile.rebase.v1", &input);
    assert_eq!(rebased["timer"]["taskId"], beta["id"]);
    assert_eq!(rebased["selectedTaskId"], beta["id"]);
    assert_projection_matches_rebase(&rebased);
}

#[test]
fn c3_acknowledged_queue_filtering_round_trips_through_projection() {
    for field in [
        "commands",
        "taskOperations",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ] {
        let operation = queue_value(field, &format!("acknowledged-{field}"));
        let rebased = dispatch_both(
            "reconcile.rebase.v1",
            &acknowledged_rebase_input(field, operation),
        );
        assert_projection_matches_rebase(&rebased);
    }
}

#[test]
fn c3_reconciliation_validates_acknowledged_and_retained_queue_clocks() {
    for field in [
        "commands",
        "taskOperations",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ] {
        let mut operation = queue_value(field, &format!("invalid-clock-{field}"));
        operation["hlcWallMs"] = if field == "commands" {
            json!(0)
        } else {
            json!(-1)
        };
        assert_rebase_projection_reject(field, operation);
    }
}

#[test]
fn c3_reconciliation_validates_acknowledged_and_retained_domain_values() {
    let mut command = queue_value("commands", "invalid-phase");
    command["phase"] = json!("custom");
    let mut task = queue_value("taskOperations", "invalid-task-type");
    task["type"] = json!("replace");
    let mut duration = queue_value("durationOperations", "invalid-duration-phase");
    duration["phase"] = json!("custom");
    let mut selected = queue_value("selectedTaskOperations", "invalid-selection");
    selected["taskId"] = json!("");

    for (field, operation) in [
        ("commands", command),
        ("taskOperations", task),
        ("durationOperations", duration),
        ("selectedTaskOperations", selected),
    ] {
        assert_rebase_projection_reject(field, operation);
    }
}

#[test]
fn c3_reconciliation_validates_every_acknowledged_timer_command_field() {
    let mutations = [
        ("id", json!("")),
        ("deviceId", json!("")),
        ("deviceSequence", json!(0)),
        ("timerId", json!("")),
        ("phase", json!("custom")),
        ("plannedDurationMs", json!(1)),
        ("occurredAt", json!("not-a-timestamp")),
        ("hlcWallMs", json!(0)),
        ("hlcCounter", json!(-1)),
        ("observedElapsedMs", json!(9_007_199_254_740_992_i64)),
    ];
    for (field, value) in mutations {
        let mut command = queue_value("commands", &format!("invalid-{field}"));
        command[field] = value;
        assert_rebase_projection_reject("commands", command);
    }
}

#[test]
fn c3_reconciliation_validates_acknowledged_commands_before_dependency_resolution() {
    let mut acknowledged = queue_value("commands", "acknowledged-parent");
    acknowledged["phase"] = json!("custom");
    let retained = queue_value("commands", "retained-child");
    let mut input = acknowledged_rebase_input("commands", acknowledged);
    input["local"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(retained);
    input["timerDependencies"] = json!([{
        "operationId": "retained-child",
        "dependsOnOperationId": "acknowledged-parent"
    }]);

    assert_operation_rejects("reconcile.rebase.v1", &input);
}

#[test]
fn c3_reconciliation_rejects_forged_canonical_and_queued_task_ids() {
    let forged_base = rebase_input(
        queues(),
        queues(),
        response(json!([{ "id": "forged-task", "title": "Alpha" }])),
    );
    assert_both_reject(
        "reconcile.rebase.v1",
        &forged_base,
        "invalid canonical response tasks",
    );

    let mut projection = projection_from_rebase(&json!({
        "baseTimer": null,
        "baseHistory": [],
        "baseTasks": [{ "id": "forged-task", "title": "Alpha" }],
        "baseDurationsMs": durations(),
        "baseAutoStartBreaks": false,
        "baseSelectedTaskId": null,
        "pending": [],
        "pendingTaskOperations": [],
        "pendingDurationOperations": [],
        "pendingAutoStartOperations": [],
        "pendingSelectedTaskOperations": []
    }));
    assert_both_reject(
        "projection.apply.v2",
        &projection,
        "invalid base task identity or title",
    );

    let mut forged = operation_clock("forged-upsert", 1_000);
    forged["taskId"] = json!("forged-task");
    forged["type"] = json!("upsert");
    forged["title"] = json!("Beta");
    let mut local = queues();
    local["taskOperations"] = json!([forged.clone()]);
    let mut sent = queues();
    sent["taskOperations"] = json!([{ "id": "forged-upsert" }]);
    let mut acknowledged_response = response(json!([]));
    acknowledged_response["taskAcknowledgements"] = json!([{
        "operationId": "forged-upsert",
        "outcome": "applied",
        "reason": ""
    }]);
    let acknowledged = rebase_input(local, sent, acknowledged_response);
    assert_both_reject(
        "reconcile.rebase.v1",
        &acknowledged,
        "invalid task identity or title",
    );

    projection["base"]["tasks"] = json!([]);
    projection["pending"]["taskOperations"] = json!([forged]);
    assert_both_reject(
        "projection.apply.v2",
        &projection,
        "invalid task identity or title",
    );
}

#[test]
fn c3_reconciliation_preserves_clock_error_precedence_for_retained_tasks() {
    let mut forged = operation_clock("forged-retained", 1_000);
    forged["deviceId"] = json!("");
    forged["taskId"] = json!("forged-task");
    forged["type"] = json!("upsert");
    forged["title"] = json!("Beta");
    let mut local = queues();
    local["taskOperations"] = json!([forged]);
    let input = rebase_input(local, queues(), response(json!([])));
    assert_both_reject(
        "reconcile.rebase.v1",
        &input,
        "invalid retained operation clock",
    );
}

#[test]
fn c3_reconciliation_and_projection_require_exact_duration_phase_sets() {
    for invalid in [
        json!({ "focus": 1_200_000, "short_break": 300_000 }),
        json!({
            "focus": 1_200_000,
            "short_break": 300_000,
            "long_break": 900_000,
            "custom": 600_000
        }),
    ] {
        let mut canonical = response(json!([]));
        canonical["durationsMs"] = invalid.clone();
        let rebase = rebase_input(queues(), queues(), canonical);
        assert_both_reject(
            "reconcile.rebase.v1",
            &rebase,
            "invalid canonical response durationsMs",
        );

        let projection = json!({
            "base": {
                "canonicalTimer": null,
                "history": [],
                "tasks": [],
                "durationsMs": invalid,
                "autoStartBreaks": false,
                "selectedTaskId": null
            },
            "pending": queues(),
            "now": SERVER_TIME
        });
        assert_both_reject("projection.apply.v2", &projection, "invalid base durations");
    }
}

#[test]
fn c3_reconciliation_rejects_noncanonical_title_normalization() {
    let canonical_id = task("Alpha")["id"].clone();
    let input = rebase_input(
        queues(),
        queues(),
        response(json!([{ "id": canonical_id, "title": " Alpha " }])),
    );
    assert_both_reject(
        "reconcile.rebase.v1",
        &input,
        "invalid canonical response tasks",
    );
}
