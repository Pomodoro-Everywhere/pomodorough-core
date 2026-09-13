use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn timer_command(id: &str, timer: &str, sequence: i64, counter: i64) -> Value {
    json!({"id": id, "deviceId": "device-a", "deviceSequence": sequence,
        "timerId": timer, "type": "start", "phase": "focus",
        "plannedDurationMs": 60000, "occurredAt": "2026-09-13T12:00:01Z",
        "hlcWallMs": 1789300800000_i64, "hlcCounter": counter,
        "observedElapsedMs": 0})
}

fn empty_queues() -> Value {
    json!({"commands": [], "taskOperations": [], "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []})
}

fn rebase_response(acknowledgements: Value) -> Value {
    json!({
        "acknowledgements": acknowledgements, "taskAcknowledgements": [],
        "durationAcknowledgements": [], "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [], "revision": 9,
        "canonicalTimer": null, "history": [], "tasks": [],
        "durationsMs": {"focus": 1_500_000, "short_break": 300_000,
            "long_break": 900_000},
        "autoStartBreaks": false, "selectedTaskId": null,
        "serverTime": "2026-09-13T12:00:10Z",
        "serverHlcWallMs": 1789300810000_i64, "serverHlcCounter": 7})
}

const QUEUES: [&str; 5] = [
    "commands",
    "taskOperations",
    "durationOperations",
    "autoStartOperations",
    "selectedTaskOperations",
];

// Fixture ops omit `deviceId` (it lives on the envelope); the rebase queues
// require it, so stamp the envelope device onto every queued operation.
fn fixture_local(fixture: &Value) -> Value {
    let device = fixture["syncRequest"]["deviceId"].clone();
    let mut local = empty_queues();
    for queue in QUEUES {
        let mut operations = fixture["syncRequest"][queue].clone();
        for operation in operations.as_array_mut().unwrap() {
            operation["deviceId"] = device.clone();
        }
        local[queue] = operations;
    }
    local
}

fn sent_ids(local: &Value) -> Value {
    let mut sent = empty_queues();
    for queue in QUEUES {
        sent[queue] = json!(
            local[queue]
                .as_array()
                .unwrap()
                .iter()
                .map(|operation| json!({"id": operation["id"]}))
                .collect::<Vec<_>>()
        );
    }
    sent
}

// C29: the spec fixture ships reason-less `applied` acks; pushing them
// verbatim through `reconcile.rebase.v1` must succeed (strong repair — the
// fixture itself is left untouched).
#[test]
fn c29_protocol_fixture_reasonless_applied_acks_rebase() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/protocol-fixtures-v1.json")).unwrap();
    let local = fixture_local(&fixture);
    let response = fixture["syncResponse"].clone();
    for field in [
        "acknowledgements",
        "taskAcknowledgements",
        "durationAcknowledgements",
        "autoStartAcknowledgements",
        "selectedTaskAcknowledgements",
    ] {
        assert!(response[field][0].get("reason").is_none());
    }
    let input = json!({"local": local.clone(), "sent": sent_ids(&local),
        "response": response, "timerDependencies": []});
    let output: Value =
        serde_json::from_str(&dispatch_json("reconcile.rebase.v1", &input.to_string()).unwrap())
            .unwrap();
    assert_eq!(output["pending"], json!([]));
}

// C29: only `rejected` outcomes must carry a `reason`; `applied`/`ignored`
// stay reason-less while rejected-without-reason still fails closed.
#[test]
fn c29_rejected_ack_requires_reason() {
    let mut local = empty_queues();
    local["commands"] = json!([timer_command("command-a", "timer-a", 1, 0)]);
    let sent = json!({"commands": [{"id": "command-a"}],
        "taskOperations": [], "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []});
    for outcome in ["applied", "ignored"] {
        let input = json!({"local": local.clone(), "sent": sent.clone(),
            "response": rebase_response(json!([{"commandId": "command-a",
                "outcome": outcome}])),
            "timerDependencies": []});
        dispatch_json("reconcile.rebase.v1", &input.to_string()).unwrap();
    }
    let missing = json!({"local": local.clone(), "sent": sent.clone(),
        "response": rebase_response(json!([{"commandId": "command-a",
            "outcome": "rejected"}])),
        "timerDependencies": []});
    let error = dispatch_json("reconcile.rebase.v1", &missing.to_string()).unwrap_err();
    assert!(error.to_string().contains("acknowledgements"), "{error}");
    let reasoned = json!({"local": local, "sent": sent,
        "response": rebase_response(json!([{"commandId": "command-a",
            "outcome": "rejected", "reason": "conflict"}])),
        "timerDependencies": []});
    dispatch_json("reconcile.rebase.v1", &reasoned.to_string()).unwrap();
}

// C30: duplicate timer command ids fail `timer.reduce.v1` and the paged path
// even when HLC order is strictly increasing (ordering alone can't catch it).
#[test]
fn c30_duplicates_rejected_by_reduce_and_page() {
    let twins = json!([
        timer_command("command-dup", "timer-a", 1, 0),
        timer_command("command-dup", "timer-b", 2, 1)
    ]);
    let error = dispatch_json(
        "timer.reduce.v1",
        &json!({"commands": twins.clone(), "now": "2026-09-13T12:00:02Z"}).to_string(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("duplicate timer command"), "{error}");
    let mut input = json!({"commands": twins, "sessions": [],
        "currentTimerId": null, "after": null});
    let error = dispatch_json("timer.replay.page.v1", &input.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains("duplicate timer command"), "{error}");
    input["commands"] = json!([timer_command("command-dup", "timer-a", 1, 0)]);
    dispatch_json("timer.replay.page.v1", &input.to_string()).unwrap();
}

// C30: the same duplicates fail reconciliation queues and the shared
// `timer::replay` used by `projection.apply.v2`.
#[test]
fn c30_duplicates_rejected_by_rebase_and_projection() {
    let mut local = empty_queues();
    local["commands"] = json!([
        timer_command("command-dup", "timer-a", 1, 0),
        timer_command("command-dup", "timer-b", 2, 1)
    ]);
    let input = json!({"local": local, "sent": empty_queues(),
        "response": rebase_response(json!([])), "timerDependencies": []});
    let error = dispatch_json("reconcile.rebase.v1", &input.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains("duplicate timer command"), "{error}");
    let start = timer_command("command-dup", "timer-a", 1, 0);
    let mut twin = timer_command("command-dup", "timer-b", 2, 1);
    twin["taskId"] = Value::Null;
    let input = json!({"base": {"canonicalTimer": null, "history": [],
            "tasks": [], "durationsMs": {"focus": 1_200_000,
                "short_break": 300_000, "long_break": 900_000},
            "autoStartBreaks": false, "selectedTaskId": null},
        "pending": {"commands": [start, twin], "taskOperations": [],
            "durationOperations": [], "autoStartOperations": [],
            "selectedTaskOperations": []},
        "now": "2026-09-13T12:00:02Z"});
    let error = dispatch_json("projection.apply.v2", &input.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains("duplicate timer command"), "{error}");
}

// C31: unknown fields are tolerated by both the full and paged replay paths,
// including a newer host's extended `after` cursor; projections still agree.
#[test]
fn c31_unknown_fields_accepted_by_reduce_and_page() {
    let mut first = timer_command("command-00000000", "timer-a", 1, 0);
    first["futureField"] = json!("server v2 metadata");
    let mut second = timer_command("command-00000001", "timer-a", 2, 1);
    second["futureField"] = json!(7);
    let now = "2026-09-13T12:00:02Z";
    let full: Value = serde_json::from_str(
        &dispatch_json(
            "timer.reduce.v1",
            &json!({"commands": [first.clone(), second.clone()],
                "now": now, "futureTopLevel": true})
            .to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    let page1: Value = serde_json::from_str(
        &dispatch_json(
            "timer.replay.page.v1",
            &json!({"commands": [first], "sessions": [],
                "currentTimerId": null, "after": null,
                "futureTopLevel": {"nested": [1]}})
            .to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut after = page1["after"].clone();
    after["futureCursor"] = json!("persisted by newer host");
    let page2: Value = serde_json::from_str(
        &dispatch_json(
            "timer.replay.page.v1",
            &json!({"commands": [second], "sessions": page1["sessions"],
                "currentTimerId": page1["currentTimerId"], "after": after,
                "now": now})
            .to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    // Each page only reports its own commands' outcomes; merge like the
    // multi-page round-trip test before comparing against the full reduce.
    let mut outcomes = page1["outcomes"].as_object().unwrap().clone();
    outcomes.extend(page2["outcomes"].as_object().unwrap().clone());
    for field in ["sessions", "history", "canonicalTimer"] {
        assert_eq!(page2[field], full[field], "{field}");
    }
    assert_eq!(Value::Object(outcomes), full["outcomes"]);
}
