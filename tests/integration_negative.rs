use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

#[test]
fn invalid_task_identity_fails_closed_without_poisoning_later_dispatches() {
    let invalid = json!({
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
            "taskOperations": [{
                "id": "operation-integration-0001",
                "deviceId": "device-integration-0001",
                "taskId": "wrong-task-identity",
                "type": "upsert",
                "title": "Café",
                "occurredAt": "1970-01-01T00:00:01Z",
                "hlcWallMs": 1000,
                "hlcCounter": 0
            }],
            "durationOperations": [],
            "autoStartOperations": [],
            "selectedTaskOperations": []
        },
        "now": "1970-01-01T00:00:01Z"
    });
    let envelope: Value = serde_json::from_str(&dispatch_envelope_json(
        "projection.apply.v2",
        &invalid.to_string(),
    ))
    .expect("error envelope JSON");

    assert_eq!(envelope["ok"], false);
    assert!(
        envelope["error"]
            .as_str()
            .is_some_and(|error| error.contains("invalid task identity or title"))
    );

    let version: Value =
        serde_json::from_str(&dispatch_json("core.version", "{}").expect("subsequent dispatch"))
            .expect("version JSON");
    assert_eq!(version["schemaVersion"], 1);
}

#[test]
fn exhausted_clock_fails_closed_without_poisoning_later_identity_generation() {
    let failure: Value = serde_json::from_str(&dispatch_envelope_json(
        "hlc.tick.v1",
        r#"{"local":{"wallMs":1,"counter":9007199254740991},"physicalNowMs":1}"#,
    ))
    .expect("clock error envelope");
    assert_eq!(failure["ok"], false);

    let identity: Value = serde_json::from_str(
        &dispatch_json(
            "uuidv7.fromParts.v1",
            r#"{"timestampMs":1,"randomValueHex":"0000000000000000001"}"#,
        )
        .expect("subsequent UUID dispatch"),
    )
    .expect("UUID JSON");
    assert_eq!(identity["timestampMs"], 1);
    assert_eq!(&identity["uuid"].as_str().unwrap()[14..15], "7");
}

fn empty_queues() -> Value {
    json!({
        "commands": [],
        "taskOperations": [],
        "durationOperations": [],
        "autoStartOperations": [],
        "selectedTaskOperations": []
    })
}

fn canonical_response() -> Value {
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
        "serverTime": "2026-08-25T12:00:00Z",
        "serverHlcWallMs": 1_787_659_200_000_i64,
        "serverHlcCounter": 0
    })
}

fn operation_clock(id: &str) -> Value {
    json!({
        "id": id,
        "deviceId": "device-a",
        "taskId": "task-a",
        "type": "delete",
        "title": "",
        "occurredAt": "2026-08-25T12:00:00Z",
        "hlcWallMs": 1_787_659_200_001_i64,
        "hlcCounter": 0
    })
}

fn reconcile(
    local: Value,
    sent: Value,
    response: Value,
    dependencies: Value,
) -> Result<Value, String> {
    dispatch_json(
        "reconcile.rebase.v1",
        &json!({
            "local": local,
            "sent": sent,
            "response": response,
            "timerDependencies": dependencies
        })
        .to_string(),
    )
    .map_err(|error| error.to_string())
    .and_then(|encoded| serde_json::from_str(&encoded).map_err(|error| error.to_string()))
}

#[test]
fn reconciliation_rejects_each_canonical_and_retained_clock_boundary() {
    let mut cases = Vec::new();
    let mut negative_skew = canonical_response();
    negative_skew["serverHlcWallMs"] = json!(1_787_659_199_999_i64);
    cases.push((empty_queues(), negative_skew, "server HLC"));

    let mut empty_selection = canonical_response();
    empty_selection["selectedTaskId"] = json!("");
    cases.push((empty_queues(), empty_selection, "selectedTaskId"));

    for (field, value) in [("id", json!("")), ("title", json!(""))] {
        let mut response = canonical_response();
        response["tasks"] = json!([{"id": "task-a", "title": "Task"}]);
        response["tasks"][0][field] = value;
        cases.push((empty_queues(), response, "tasks"));
    }

    for value in [json!(59_999), json!(60_001)] {
        let mut response = canonical_response();
        response["durationsMs"]["focus"] = value;
        cases.push((empty_queues(), response, "durationsMs"));
    }

    for (field, value) in [
        ("deviceId", json!("")),
        ("hlcWallMs", json!(-1)),
        ("hlcCounter", json!(-1)),
    ] {
        let mut operation = operation_clock("operation-a");
        operation[field] = value;
        let mut local = empty_queues();
        local["taskOperations"] = json!([operation]);
        cases.push((local, canonical_response(), "retained operation clock"));
    }

    let mut empty_local_id = empty_queues();
    empty_local_id["taskOperations"] = json!([operation_clock("")]);
    cases.push((
        empty_local_id,
        canonical_response(),
        "local taskOperations identities",
    ));

    for (local, response, expected) in cases {
        let error = reconcile(local, empty_queues(), response, json!([])).unwrap_err();
        assert!(
            error.contains(expected),
            "expected {expected:?}, got {error}"
        );
    }
}

#[test]
fn reconciliation_rejects_structurally_invalid_acknowledgement_sets() {
    let mut sent = empty_queues();
    sent["taskOperations"] = json!([{"id": "sent-a"}, {"id": "sent-a"}]);
    let error = reconcile(empty_queues(), sent, canonical_response(), json!([])).unwrap_err();
    assert!(error.contains("taskAcknowledgements"));

    let mut sent = empty_queues();
    sent["taskOperations"] = json!([{"id": ""}]);
    let mut response = canonical_response();
    response["taskAcknowledgements"] = json!([{
        "operationId": "",
        "outcome": "applied",
        "reason": ""
    }]);
    assert!(reconcile(empty_queues(), sent, response, json!([])).is_err());

    for acknowledgement in [
        json!("not-an-object"),
        json!({"operationId": "sent-a", "reason": ""}),
    ] {
        let mut sent = empty_queues();
        sent["taskOperations"] = json!([{"id": "sent-a"}]);
        let mut response = canonical_response();
        response["taskAcknowledgements"] = json!([acknowledgement]);
        assert!(reconcile(empty_queues(), sent, response, json!([])).is_err());
    }

    let mut sent = empty_queues();
    sent["taskOperations"] = json!([{"id": "sent-a"}, {"id": "sent-b"}]);
    for ids in [["sent-a", "foreign"], ["sent-a", "sent-a"]] {
        let mut response = canonical_response();
        response["taskAcknowledgements"] = Value::Array(
            ids.into_iter()
                .map(|id| json!({"operationId": id, "outcome": "applied", "reason": ""}))
                .collect(),
        );
        assert!(reconcile(empty_queues(), sent.clone(), response, json!([])).is_err());
    }
}

fn timer_command(id: &str, kind: &str, phase: &str, sequence: i64) -> Value {
    json!({
        "id": id,
        "deviceId": "device-a",
        "deviceSequence": sequence,
        "timerId": format!("timer-{id}"),
        "type": kind,
        "phase": phase,
        "plannedDurationMs": 300_000,
        "occurredAt": "2026-08-25T12:00:00Z",
        "hlcWallMs": 1_787_659_200_001_i64 + sequence,
        "hlcCounter": 0,
        "observedElapsedMs": 0
    })
}

#[test]
fn reconciliation_rejects_each_timer_dependency_identity_boundary() {
    let mut local = empty_queues();
    local["commands"] = json!([
        timer_command("parent", "finish", "focus", 1),
        timer_command("child", "start", "short_break", 2)
    ]);
    for dependency in [
        json!({"operationId": "", "dependsOnOperationId": "parent"}),
        json!({"operationId": "child", "dependsOnOperationId": ""}),
        json!({"operationId": "child", "dependsOnOperationId": "child"}),
        json!({"operationId": "missing", "dependsOnOperationId": "parent"}),
        json!({"operationId": "child", "dependsOnOperationId": "missing"}),
    ] {
        let error = reconcile(
            local.clone(),
            empty_queues(),
            canonical_response(),
            json!([dependency]),
        )
        .unwrap_err();
        assert!(error.contains("dependency graph"), "{error}");
    }

    let duplicate = json!([
        {"operationId": "child", "dependsOnOperationId": "parent"},
        {"operationId": "child", "dependsOnOperationId": "parent"}
    ]);
    assert!(reconcile(local, empty_queues(), canonical_response(), duplicate).is_err());
}

fn projection_input() -> Value {
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
        "now": "2026-08-25T12:00:00Z"
    })
}

fn projection_error(input: Value) -> String {
    dispatch_json("projection.apply.v2", &input.to_string())
        .unwrap_err()
        .to_string()
}

#[test]
fn projection_dispatch_rejects_each_operation_clock_boundary() {
    for (field, value) in [
        ("id", json!("")),
        ("deviceId", json!("")),
        ("hlcWallMs", json!(-1)),
        ("hlcCounter", json!(-1)),
        ("occurredAt", json!("not-a-time")),
    ] {
        let mut input = projection_input();
        let mut operation = operation_clock("operation-a");
        operation[field] = value;
        input["pending"]["taskOperations"] = json!([operation]);
        let error = projection_error(input);
        assert!(
            error.contains("invalid operation clock"),
            "{field}: {error}"
        );
    }
}

#[test]
fn projection_dispatch_rejects_distinct_domain_validation_boundaries() {
    let identity: Value = serde_json::from_str(
        &dispatch_json("task.identity.v1", r#"{"title":"Valid task"}"#).unwrap(),
    )
    .unwrap();

    let mut wrong_title = projection_input();
    wrong_title["base"]["tasks"] = json!([{"id": identity["id"], "title": "Other"}]);

    let mut duplicate = projection_input();
    duplicate["base"]["tasks"] = json!([
        {"id": identity["id"], "title": identity["title"]},
        {"id": identity["id"], "title": identity["title"]}
    ]);

    let mut wrong_upsert_title = projection_input();
    let mut operation = operation_clock("upsert-a");
    operation["type"] = json!("upsert");
    operation["taskId"] = identity["id"].clone();
    operation["title"] = json!("Other");
    wrong_upsert_title["pending"]["taskOperations"] = json!([operation]);

    let mut wrong_phase = projection_input();
    let mut duration = operation_clock("duration-a");
    duration["phase"] = json!("rest");
    duration["durationMs"] = json!(60_000);
    wrong_phase["pending"]["durationOperations"] = json!([duration.clone()]);

    let mut wrong_duration = projection_input();
    duration["phase"] = json!("focus");
    duration["durationMs"] = json!(59_999);
    wrong_duration["pending"]["durationOperations"] = json!([duration]);

    let mut extra_base_duration = projection_input();
    extra_base_duration["base"]["durationsMs"]["rest"] = json!(60_000);

    for (input, expected) in [
        (wrong_title, "invalid base task identity or title"),
        (duplicate, "invalid base task identity or title"),
        (wrong_upsert_title, "invalid task identity or title"),
        (wrong_phase, "invalid duration operation"),
        (wrong_duration, "invalid duration operation"),
        (extra_base_duration, "invalid base durations"),
    ] {
        let error = projection_error(input);
        assert!(
            error.contains(expected),
            "expected {expected:?}, got {error}"
        );
    }
}

fn generated_dependency() -> Value {
    json!({
        "operationId": "child",
        "dependsOnOperationId": "parent",
        "generatedBreak": true,
        "sourceDayStart": "2026-08-25T00:00:00Z",
        "sourceDayEnd": "2026-08-26T00:00:00Z"
    })
}

#[test]
fn reconciliation_rejects_each_generated_break_shape_boundary() {
    let parent = timer_command("parent", "finish", "focus", 1);
    let child = timer_command("child", "start", "short_break", 2);
    let cases = [
        ("child", "type", json!("pause")),
        ("child", "phase", json!("focus")),
        ("parent", "type", json!("start")),
        ("parent", "phase", json!("short_break")),
    ];
    for (target, field, value) in cases {
        let mut parent = parent.clone();
        let mut child = child.clone();
        if target == "parent" {
            parent[field] = value;
        } else {
            child[field] = value;
        }
        let mut local = empty_queues();
        local["commands"] = json!([parent, child]);
        let error = reconcile(
            local,
            empty_queues(),
            canonical_response(),
            json!([generated_dependency()]),
        )
        .unwrap_err();
        assert!(error.contains("generated break dependency"), "{error}");
    }

    for (start, end) in [
        ("2026-08-25T00:00:00Z", "2026-08-25T00:00:00Z"),
        ("2026-08-24T00:00:00Z", "2026-08-26T03:00:00Z"),
    ] {
        let mut dependency = generated_dependency();
        dependency["sourceDayStart"] = json!(start);
        dependency["sourceDayEnd"] = json!(end);
        let mut local = empty_queues();
        local["commands"] = json!([parent.clone(), child.clone()]);
        assert!(
            reconcile(
                local,
                empty_queues(),
                canonical_response(),
                json!([dependency])
            )
            .is_err()
        );
    }
}

fn canonical_generated_response() -> Value {
    let mut response = canonical_response();
    response["acknowledgements"] = json!([{
        "commandId": "parent",
        "outcome": "applied",
        "reason": ""
    }]);
    response["canonicalTimer"] = json!({
        "id": "timer-parent",
        "phase": "focus",
        "status": "completed",
        "plannedDurationMs": 300_000,
        "elapsedAtAnchorMs": 300_000,
        "anchorAt": "2026-08-25T12:00:00Z",
        "lastIntent": {
            "type": "finish",
            "commandId": "parent",
            "occurredAt": "2026-08-25T12:00:00Z"
        }
    });
    response
}

#[test]
fn reconciliation_requires_exact_canonical_evidence_for_generated_break_promotion() {
    let parent = timer_command("parent", "finish", "focus", 1);
    let child = timer_command("child", "start", "short_break", 2);
    let mut local = empty_queues();
    local["commands"] = json!([parent.clone(), child.clone()]);
    let mut sent = empty_queues();
    sent["commands"] = json!([parent]);

    let accepted = reconcile(
        local.clone(),
        sent.clone(),
        canonical_generated_response(),
        json!([generated_dependency()]),
    )
    .unwrap();
    assert_eq!(accepted["promotedTimerOperationIds"], json!(["child"]));

    for (field, value) in [
        ("id", json!("different-timer")),
        ("phase", json!("short_break")),
        ("status", json!("cancelled")),
    ] {
        let mut response = canonical_generated_response();
        response["canonicalTimer"][field] = value;
        let output = reconcile(
            local.clone(),
            sent.clone(),
            response,
            json!([generated_dependency()]),
        )
        .unwrap();
        assert_eq!(output["droppedTimerIds"], json!(["timer-child"]));
    }

    for (field, value) in [
        ("type", json!("pause")),
        ("commandId", json!("different-command")),
    ] {
        let mut response = canonical_generated_response();
        response["canonicalTimer"]["lastIntent"][field] = value;
        let output = reconcile(
            local.clone(),
            sent.clone(),
            response,
            json!([generated_dependency()]),
        )
        .unwrap();
        assert_eq!(output["droppedTimerIds"], json!(["timer-child"]));
    }
}
