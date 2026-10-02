use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const OPERATION: &str = "workspace.project.v1";
const WALL: i64 = 1_784_548_800_000;
const QUEUES: [&str; 5] = [
    "commands",
    "taskOperations",
    "durationOperations",
    "autoStartOperations",
    "selectedTaskOperations",
];

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn request() -> Value {
    json!({"base": {"canonicalTimer": null, "history": [],
        "tasks": [{"id": "33f9d32c-a7ee-8aa9-897a-13e19bc4e5d4", "title": "Ship release"}],
        "durationsMs": {"focus": 1500000, "short_break": 300000, "long_break": 900000},
        "autoStartBreaks": false, "selectedTaskId": "33f9d32c-a7ee-8aa9-897a-13e19bc4e5d4"},
        "local": {"commands": [], "taskOperations": [], "durationOperations": [],
            "autoStartOperations": [], "selectedTaskOperations": []},
        "canonicalHead": {"wallMs": WALL, "counter": 10},
        "neverSent": {}, "timerDependencies": [], "now": "2026-07-20T12:00:00Z"})
}

fn operation(queue: &str, id: &str, counter: i64) -> Value {
    let mut value = json!({"id": id, "deviceId": "device-a",
        "occurredAt": "2026-07-20T12:00:00Z", "hlcWallMs": WALL, "hlcCounter": counter});
    let fields = match queue {
        "commands" => json!({"deviceSequence": counter, "timerId": "timer-a",
            "type": "start", "phase": "focus", "plannedDurationMs": 60000,
            "observedElapsedMs": 0}),
        "taskOperations" => {
            json!({"type": "delete", "taskId": "33f9d32c-a7ee-8aa9-897a-13e19bc4e5d4"})
        }
        "durationOperations" => json!({"phase": "focus", "durationMs": 1800000}),
        "autoStartOperations" => json!({"enabled": true}),
        "selectedTaskOperations" => json!({"taskId": null}),
        _ => unreachable!(),
    };
    value
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    value
}

#[test]
fn baseline_gap_unfiltered_projection_replays_unsafe_duration() {
    let mut input = request();
    input["local"]["durationOperations"] = json!([operation("durationOperations", "old", 0)]);
    let unsafe_projection = call(
        "projection.apply.v2",
        &json!({
        "base": input["base"], "pending": input["local"], "now": input["now"]}),
    );
    assert_eq!(unsafe_projection["durationsMs"]["focus"], 1800000);
    let safe = call(OPERATION, &input);
    assert_eq!(safe["workspace"]["durationsMs"]["focus"], 1500000);
    assert_eq!(safe["projectionPending"]["durationOperations"], json!([]));
}

#[test]
fn all_domains_require_complete_proof_and_strictly_newer_clocks() {
    for queue in QUEUES {
        for counter in [9, 10, 11] {
            for proven in [false, true] {
                let mut input = request();
                input["local"][queue] = json!([operation(queue, "one", counter)]);
                if proven {
                    input["neverSent"][queue] = json!(["one"]);
                }
                let output = call(OPERATION, &input);
                let expected = if proven && counter > 10 {
                    input["local"][queue].clone()
                } else {
                    json!([])
                };
                assert_eq!(
                    output["projectionPending"][queue], expected,
                    "{queue}/{counter}/{proven}"
                );
                assert_domain_state(&input, &output, queue, proven && counter > 10);
            }
        }
    }
}

fn assert_domain_state(input: &Value, output: &Value, queue: &str, eligible: bool) {
    if queue == "commands" {
        assert_eq!(output["workspace"]["canonicalTimer"].is_null(), !eligible);
        if eligible {
            assert_eq!(output["workspace"]["canonicalTimer"]["status"], "running");
        }
        return;
    }
    let (field, optimistic) = match queue {
        "taskOperations" => ("tasks", json!([])),
        "durationOperations" => (
            "durationsMs",
            json!({"focus": 1800000, "short_break": 300000, "long_break": 900000}),
        ),
        "autoStartOperations" => ("autoStartBreaks", json!(true)),
        "selectedTaskOperations" => ("selectedTaskId", Value::Null),
        _ => unreachable!(),
    };
    let expected = if eligible {
        &optimistic
    } else {
        &input["base"][field]
    };
    assert_eq!(&output["workspace"][field], expected, "{queue}/{eligible}");
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-projection-v1.json")).unwrap()
}

#[test]
fn shared_workspace_fixtures_match_production_dispatch() {
    let fixture = fixture();
    for case in fixture["cases"].as_array().unwrap() {
        let mut input = fixture["request"].clone();
        input
            .as_object_mut()
            .unwrap()
            .extend(case["overrides"].as_object().unwrap().clone());
        let output = call(OPERATION, &input);
        assert_eq!(
            output["workspace"], case["expectedWorkspace"],
            "{}",
            case["name"]
        );
        for queue in QUEUES {
            let eligible = case["expectedEligibleQueues"]
                .as_array()
                .unwrap()
                .contains(&json!(queue));
            let expected = if eligible {
                input["local"][queue].clone()
            } else {
                json!([])
            };
            assert_eq!(
                output["projectionPending"][queue], expected,
                "{queue}/{}",
                case["name"]
            );
        }
    }
}

fn restart(input: &Value) -> Value {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "workspace-projection-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&path, input.to_string()).unwrap();
    let restored = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    std::fs::remove_file(path).unwrap();
    restored
}

#[test]
fn partial_proof_or_stale_sibling_blocks_whole_domain_after_later_mutation_and_restart() {
    for queue in QUEUES {
        for stale in [false, true] {
            let mut input = request();
            let old = operation(queue, "older", if stale { 9 } else { 11 });
            input["local"][queue] = json!([old]);
            if stale {
                input["neverSent"][queue] = json!(["older"]);
            }
            assert_eq!(
                call(OPERATION, &input)["projectionPending"][queue],
                json!([])
            );
            input["local"][queue]
                .as_array_mut()
                .unwrap()
                .push(operation(queue, "fresh", 12));
            input["neverSent"][queue] = if stale {
                json!(["older", "fresh"])
            } else {
                json!(["fresh"])
            };
            input = restart(&input);
            let saved = input.clone();
            let output = call(OPERATION, &input);
            assert_eq!(
                output["projectionPending"][queue],
                json!([]),
                "{queue}/{stale}"
            );
            assert_domain_state(&input, &output, queue, false);
            assert_eq!(input, saved);
            assert_eq!(output, call(OPERATION, &restart(&input)));
        }
    }
}

#[test]
fn fresh_mutations_in_each_domain_are_recomputed_instead_of_cached() {
    for queue in QUEUES {
        let mut input = request();
        assert_eq!(
            call(OPERATION, &input)["projectionPending"][queue],
            json!([])
        );
        input["local"][queue] = json!([operation(queue, "fresh", 11)]);
        input["neverSent"][queue] = json!(["fresh"]);
        let output = call(OPERATION, &restart(&input));
        assert_eq!(output["projectionPending"][queue], input["local"][queue]);
        input["neverSent"] = json!({});
        assert_eq!(
            call(OPERATION, &restart(&input))["projectionPending"][queue],
            json!([])
        );
    }
}

#[test]
fn canonical_duration_base_survives_claim_unrelated_mutation_and_restart() {
    let mut input = request();
    input["local"]["durationOperations"] = json!([operation("durationOperations", "duration", 11)]);
    input["neverSent"]["durationOperations"] = json!(["duration"]);
    let optimistic = call(OPERATION, &input);
    assert_eq!(optimistic["workspace"]["durationsMs"]["focus"], 1800000);
    input["neverSent"] = json!({});
    input["local"]["autoStartOperations"] =
        json!([operation("autoStartOperations", "unrelated", 12)]);
    input["neverSent"]["autoStartOperations"] = json!(["unrelated"]);
    let output = call(OPERATION, &restart(&input));
    assert_eq!(output["workspace"]["durationsMs"]["focus"], 1500000);
    assert_eq!(output["workspace"]["autoStartBreaks"], true);
    // D04 cannot be repaired from proof alone: a valid but optimistic base has
    // already lost the canonical value. Pin that persistence precondition.
    input["base"]["durationsMs"] = optimistic["workspace"]["durationsMs"].clone();
    assert_eq!(
        call(OPERATION, &input)["workspace"]["durationsMs"]["focus"],
        1800000
    );
}

fn reconciliation_request(input: &Value) -> Value {
    let mut response = input["base"].clone();
    response.as_object_mut().unwrap().extend(
        json!({"revision": 1,
        "serverTime": input["now"], "serverHlcWallMs": input["canonicalHead"]["wallMs"],
        "serverHlcCounter": input["canonicalHead"]["counter"], "acknowledgements": [],
        "taskAcknowledgements": [], "durationAcknowledgements": [],
        "autoStartAcknowledgements": [], "selectedTaskAcknowledgements": []})
        .as_object()
        .unwrap()
        .clone(),
    );
    json!({"local": input["local"], "neverSent": input["neverSent"], "sent": {},
        "timerDependencies": input["timerDependencies"], "response": response})
}

fn assert_reconciliation_equivalence(input: &Value) {
    let output = call(OPERATION, input);
    let reconciled = call("reconcile.rebase.v2", &reconciliation_request(input));
    // Reconciliation's projection queues use normalized typed objects; compare
    // reducer semantics separately from workspace's exact raw queue contract.
    let projection = call(
        "projection.apply.v2",
        &json!({"base": input["base"],
        "pending": reconciled["projectionPending"], "now": input["now"]}),
    );
    let mut domain = output["workspace"].clone();
    if let Some(intent) = domain
        .get_mut("canonicalTimer")
        .and_then(Value::as_object_mut)
        .and_then(|timer| timer.get_mut("lastIntent"))
        .and_then(Value::as_object_mut)
    {
        if let Some(device) = intent.remove("deviceId") {
            let source = output["projectionPending"]["commands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|command| command["id"] == intent["commandId"])
                .map(|command| &command["deviceId"])
                .unwrap_or(&input["base"]["canonicalTimer"]["lastIntent"]["deviceId"]);
            assert_eq!(&device, source);
        }
    }
    // Native device metadata is an additive workspace output contract. Compare
    // every reducer field after independently checking that metadata's source.
    assert_eq!(domain, projection);
    for (field, reconcile_field) in [
        ("canonicalTimer", "timer"),
        ("history", "history"),
        ("tasks", "tasks"),
        ("durationsMs", "durationsMs"),
        ("autoStartBreaks", "autoStartBreaks"),
        ("selectedTaskId", "selectedTaskId"),
    ] {
        assert_eq!(domain[field], reconciled[reconcile_field], "{field}");
    }
    for queue in QUEUES {
        assert_eq!(
            output["projectionPending"][queue].as_array().unwrap().len(),
            reconciled["projectionPending"][queue]
                .as_array()
                .unwrap()
                .len()
        );
    }
}

#[test]
fn all_five_domains_match_reconciliation_for_safe_stale_and_partial_evidence() {
    for queue in QUEUES {
        for counter in [9, 10, 11] {
            for proof in [json!({}), json!({queue: ["one"]})] {
                let mut input = request();
                input["local"][queue] = json!([operation(queue, "one", counter)]);
                input["neverSent"] = proof;
                assert_reconciliation_equivalence(&input);
                input["local"][queue]
                    .as_array_mut()
                    .unwrap()
                    .push(operation(queue, "two", 12));
                assert_reconciliation_equivalence(&input);
            }
        }
    }
    assert_reconciliation_equivalence(&fixture()["request"]);
}

fn assert_invalid(input: &Value) {
    assert!(
        dispatch_json(OPERATION, &input.to_string()).is_err(),
        "accepted {input}"
    );
}

#[test]
fn required_complete_base_queues_dependencies_and_head_fail_closed() {
    for field in ["base", "local", "canonicalHead", "timerDependencies", "now"] {
        let mut input = request();
        input.as_object_mut().unwrap().remove(field);
        assert_invalid(&input);
    }
    for field in [
        "canonicalTimer",
        "history",
        "tasks",
        "durationsMs",
        "autoStartBreaks",
        "selectedTaskId",
    ] {
        let mut input = request();
        input["base"].as_object_mut().unwrap().remove(field);
        assert_invalid(&input);
    }
    for queue in QUEUES {
        let mut input = request();
        input["local"].as_object_mut().unwrap().remove(queue);
        assert_invalid(&input);
        for invalid in [
            Value::Null,
            json!({}),
            json!(false),
            json!([null]),
            json!([[]]),
        ] {
            input["local"][queue] = invalid;
            assert_invalid(&input);
        }
    }
    for head in [
        json!({}),
        json!({"wallMs": WALL}),
        json!({"counter": 0}),
        json!({"wallMs": -1, "counter": 0}),
        json!({"wallMs": WALL, "counter": -1}),
        json!({"wallMs": 9007199254740992_i64, "counter": 0}),
        json!({"wallMs": WALL, "counter": 9007199254740992_i64}),
        json!({"wallMs": WALL, "counter": 0, "extra": 1}),
        json!([]),
        json!(false),
    ] {
        let mut input = request();
        input["canonicalHead"] = head;
        assert_invalid(&input);
    }
}

#[test]
fn unknown_control_fields_and_malformed_base_are_rejected() {
    for pointer in ["/extra", "/base/extra", "/local/extra"] {
        let mut input = request();
        let (parent, field) = pointer.rsplit_once('/').unwrap();
        input.pointer_mut(parent).unwrap()[field] = json!([]);
        assert_invalid(&input);
    }
    for (pointer, value) in [
        ("/now", json!("bad-time")),
        ("/base/history", Value::Null),
        ("/base/tasks", json!([{}])),
        ("/base/durationsMs/focus", json!(1)),
        ("/base/autoStartBreaks", Value::Null),
        ("/base/selectedTaskId", json!("missing-task")),
        ("/base/canonicalTimer", json!({})),
        ("/timerDependencies", Value::Null),
        ("/timerDependencies", json!([null])),
    ] {
        let mut input = request();
        *input.pointer_mut(pointer).unwrap() = value;
        assert_invalid(&input);
    }
}

#[test]
fn proof_is_validated_even_without_a_covering_head() {
    for queue in QUEUES {
        for proof in [
            Value::Null,
            json!([]),
            json!({"unknown": []}),
            json!({queue: null}),
            json!({queue: [1]}),
            json!({queue: ["absent"]}),
            json!({queue: ["one", "one"]}),
        ] {
            let mut input = request();
            input["canonicalHead"] = Value::Null;
            input["local"][queue] = json!([operation(queue, "one", 11)]);
            input["neverSent"] = proof;
            assert_invalid(&input);
        }
        let mut input = request();
        input["local"][queue] = json!([operation(queue, "one", 11)]);
        input.as_object_mut().unwrap().remove("neverSent");
        assert_eq!(
            call(OPERATION, &input)["projectionPending"][queue],
            json!([])
        );
    }
}

#[test]
fn suppressed_operations_still_receive_identity_clock_and_payload_validation() {
    for queue in QUEUES {
        let mut input = request();
        input["local"][queue] = json!([operation(queue, "one", 11), operation(queue, "one", 12)]);
        assert_invalid(&input);
        for (field, invalid) in [
            ("id", json!("")),
            ("deviceId", json!("")),
            ("occurredAt", json!("invalid")),
            ("hlcWallMs", json!(-1)),
            ("hlcCounter", json!(9007199254740992_i64)),
        ] {
            input["local"][queue] = json!([operation(queue, "one", 11)]);
            input["local"][queue][0][field] = invalid;
            assert_invalid(&input);
        }
        let (field, invalid) = match queue {
            "commands" => ("plannedDurationMs", json!(0)),
            "taskOperations" => ("type", json!("not-an-operation")),
            "durationOperations" => ("durationMs", json!(123)),
            "autoStartOperations" => ("enabled", json!("true")),
            "selectedTaskOperations" => ("taskId", json!("")),
            _ => unreachable!(),
        };
        input["local"][queue] = json!([operation(queue, "one", 11)]);
        input["local"][queue][0][field] = invalid;
        assert_invalid(&input);
    }
}

#[test]
fn duplicate_json_fields_fail_before_raw_payload_retention() {
    let input = fixture()["request"].to_string();
    for (original, duplicate) in [
        ("\"id\":\"start\"", "\"id\":\"start\",\"id\":\"other\""),
        ("\"keep\":null", "\"keep\":null,\"keep\":true"),
        ("\"counter\":10", "\"counter\":10,\"counter\":0"),
        ("\"focus\":1500000", "\"focus\":1500000,\"focus\":1800000"),
    ] {
        let duplicate = input.replace(original, duplicate);
        assert_ne!(duplicate, input);
        assert!(dispatch_json(OPERATION, &duplicate).is_err());
    }
}

#[test]
fn safe_raw_objects_preserve_extensions_and_omission_null_empty_distinctions() {
    for queue in QUEUES {
        let mut input = request();
        let mut value = operation(queue, "same-id", 11);
        value["extension"] =
            json!({"id": "fake", "type": "fake", "values": [null, {}, [], "", false]});
        value["futureField"] = Value::Null;
        input["local"][queue] = json!([value]);
        input["neverSent"][queue] = json!(["same-id"]);
        let saved = restart(&input);
        let output = call(OPERATION, &saved);
        assert_eq!(output["projectionPending"][queue], saved["local"][queue]);
        if queue == "taskOperations" {
            input["local"][queue][0]["title"] = json!("");
            assert_eq!(
                call(OPERATION, &input)["projectionPending"][queue],
                input["local"][queue]
            );
        }
    }
}

fn dependent_request() -> Value {
    let mut input = request();
    let parent = operation("commands", "parent", 11);
    let mut child = operation("commands", "child", 12);
    child["type"] = json!("pause");
    input["local"]["commands"] = json!([child, parent]);
    input["neverSent"]["commands"] = json!(["child", "parent"]);
    input["timerDependencies"] =
        json!([{"operationId": "child", "dependsOnOperationId": "parent"}]);
    input
}

#[test]
fn dependencies_use_causal_order_not_array_order_and_match_reconciliation() {
    let input = dependent_request();
    let output = call(OPERATION, &input);
    assert_eq!(output["workspace"]["canonicalTimer"]["status"], "paused");
    assert_eq!(
        output["projectionPending"]["commands"],
        input["local"]["commands"]
    );
    assert_reconciliation_equivalence(&input);
    let mut unsafe_parent = input.clone();
    unsafe_parent["neverSent"]["commands"] = json!(["child"]);
    assert_eq!(
        call(OPERATION, &unsafe_parent)["workspace"]["canonicalTimer"],
        Value::Null
    );
    assert_reconciliation_equivalence(&unsafe_parent);
}

#[test]
fn invalid_dependency_graphs_and_immutable_order_fail_even_when_suppressed() {
    for dependencies in [
        json!([{"operationId": "child", "dependsOnOperationId": "absent"}]),
        json!([{"operationId": "absent", "dependsOnOperationId": "parent"}]),
        json!([{"operationId": "child", "dependsOnOperationId": "child"}]),
        json!([{"operationId": "child", "dependsOnOperationId": "parent"},
            {"operationId": "parent", "dependsOnOperationId": "child"}]),
        json!([{"operationId": "child", "dependsOnOperationId": "parent"},
            {"operationId": "child", "dependsOnOperationId": "parent"}]),
        json!([{"operationId": "child", "dependsOnOperationId": "parent", "generatedBreak": true}]),
    ] {
        let mut input = dependent_request();
        input["neverSent"] = json!({});
        input["timerDependencies"] = dependencies;
        assert_invalid(&input);
    }
    for field in ["hlcCounter", "deviceSequence"] {
        let mut input = dependent_request();
        input["neverSent"] = json!({});
        input["local"]["commands"][0][field] = json!(10);
        assert_invalid(&input);
    }
    let mut cross_device = dependent_request();
    cross_device["local"]["commands"][0]["deviceId"] = json!("other-device");
    cross_device["local"]["commands"][0]["hlcCounter"] = json!(10);
    assert_invalid(&cross_device);
}

#[test]
fn terminal_finish_cancel_clear_and_automatic_completion_need_no_client_reconstruction() {
    for (kind, status) in [("finish", "completed"), ("cancel", "cancelled")] {
        let mut input = dependent_request();
        input["local"]["commands"][0]["type"] = json!(kind);
        input["local"]["commands"][0]["observedElapsedMs"] = json!(1000);
        let output = call(OPERATION, &input);
        assert_eq!(output["workspace"]["canonicalTimer"]["status"], status);
        assert_eq!(output["workspace"]["history"].as_array().unwrap().len(), 1);
        assert_reconciliation_equivalence(&input);
        assert_eq!(output, call(OPERATION, &restart(&input)));
        let mut clear = operation("commands", "clear", 13);
        clear["type"] = json!("clear");
        input["local"]["commands"]
            .as_array_mut()
            .unwrap()
            .push(clear);
        input["neverSent"]["commands"] = json!(["parent", "child", "clear"]);
        let cleared = call(OPERATION, &input);
        assert_eq!(cleared["workspace"]["canonicalTimer"], Value::Null);
        assert_eq!(
            cleared["workspace"]["history"],
            output["workspace"]["history"]
        );
    }
}

#[test]
fn automatic_completion_and_canonical_overlap_validation_stay_in_core() {
    let mut input = request();
    input["local"]["commands"] = json!([operation("commands", "start", 11)]);
    input["neverSent"]["commands"] = json!(["start"]);
    input["now"] = json!("2026-07-20T12:01:00Z");
    let completed = call(OPERATION, &input);
    assert_eq!(
        completed["workspace"]["canonicalTimer"]["status"],
        "completed"
    );
    assert_eq!(
        completed["workspace"]["history"].as_array().unwrap().len(),
        1
    );
    // The workspace boundary accepts the exact persisted terminal pair.
    for field in ["canonicalTimer", "history"] {
        input["base"][field] = completed["workspace"][field].clone();
    }
    input["local"]["commands"] = json!([]);
    input["neverSent"] = json!({});
    let reopened = call(OPERATION, &input);
    for field in ["canonicalTimer", "history"] {
        assert_eq!(reopened["workspace"][field], completed["workspace"][field]);
    }
    assert_eq!(reopened["workspace"]["timerOutcomes"], json!({}));
}

#[test]
fn generated_break_dependencies_share_validation_and_projection_with_reconciliation() {
    let mut input = dependent_request();
    input["base"]["canonicalTimer"] =
        fixture()["cases"][0]["expectedWorkspace"]["canonicalTimer"].clone();
    input["local"]["commands"][1]["type"] = json!("finish");
    input["local"]["commands"][0]["type"] = json!("start");
    input["local"]["commands"][0]["timerId"] = json!("break-timer");
    input["local"]["commands"][0]["phase"] = json!("short_break");
    input["local"]["commands"][0]["plannedDurationMs"] = json!(300000);
    input["timerDependencies"][0]["generatedBreak"] = json!(true);
    input["timerDependencies"][0]["sourceDayStart"] = json!("2026-07-20T00:00:00Z");
    input["timerDependencies"][0]["sourceDayEnd"] = json!("2026-07-21T00:00:00Z");
    assert_reconciliation_equivalence(&input);
    let projected = call(OPERATION, &input);
    assert_eq!(
        projected["workspace"]["canonicalTimer"]["id"],
        "break-timer"
    );
    assert_eq!(
        projected["workspace"]["history"].as_array().unwrap().len(),
        1
    );
    input["neverSent"] = json!({});
    assert_reconciliation_equivalence(&input);
    input["timerDependencies"][0]["sourceDayEnd"] = json!("2026-07-19T00:00:00Z");
    assert_invalid(&input);
}

#[test]
fn acknowledged_barriers_survive_post_reconcile_projection_in_every_domain() {
    for (queue, pending_field, ack_field) in [
        ("commands", "pending", "acknowledgements"),
        (
            "taskOperations",
            "pendingTaskOperations",
            "taskAcknowledgements",
        ),
        (
            "durationOperations",
            "pendingDurationOperations",
            "durationAcknowledgements",
        ),
        (
            "autoStartOperations",
            "pendingAutoStartOperations",
            "autoStartAcknowledgements",
        ),
        (
            "selectedTaskOperations",
            "pendingSelectedTaskOperations",
            "selectedTaskAcknowledgements",
        ),
    ] {
        let mut input = request();
        input["local"][queue] =
            json!([operation(queue, "older", 9), operation(queue, "acked", 10)]);
        input["neverSent"][queue] = json!(["older"]);
        let mut reconciliation = reconciliation_request(&input);
        reconciliation["sent"][queue] = json!([{"id": "acked"}]);
        let id_field = if queue == "commands" {
            "commandId"
        } else {
            "operationId"
        };
        reconciliation["response"][ack_field] = json!([{id_field: "acked", "outcome": "ignored"}]);
        let reconciled = call("reconcile.rebase.v2", &reconciliation);
        input["local"][queue] = reconciled[pending_field].clone();
        let output = call(OPERATION, &restart(&input));
        assert_eq!(output["projectionPending"], reconciled["projectionPending"]);
        assert_reconciliation_equivalence(&input);
        input["local"][queue]
            .as_array_mut()
            .unwrap()
            .push(operation(queue, "fresh", 11));
        input["neverSent"][queue] = json!(["older", "fresh"]);
        assert_eq!(
            call(OPERATION, &restart(&input))["projectionPending"][queue],
            json!([])
        );
    }
}
