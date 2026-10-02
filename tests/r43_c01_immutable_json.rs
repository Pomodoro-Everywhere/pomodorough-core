use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const QUEUES: [(&str, &str); 5] = [
    ("commands", "pending"),
    ("taskOperations", "pendingTaskOperations"),
    ("durationOperations", "pendingDurationOperations"),
    ("autoStartOperations", "pendingAutoStartOperations"),
    ("selectedTaskOperations", "pendingSelectedTaskOperations"),
];

fn request() -> Value {
    json!({"local": {}, "sent": {}, "response": {
        "acknowledgements": [], "taskAcknowledgements": [],
        "durationAcknowledgements": [], "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [], "revision": 1,
        "canonicalTimer": null, "history": [], "tasks": [],
        "durationsMs": {"focus": 1500000, "short_break": 300000, "long_break": 900000},
        "autoStartBreaks": false, "selectedTaskId": null,
        "serverTime": "2026-07-20T12:00:10Z",
        "serverHlcWallMs": 1784548810000_i64, "serverHlcCounter": 7}})
}

fn operation(queue: &str) -> Value {
    let mut value = json!({"id": "shared-id", "deviceId": "device-a",
        "occurredAt": "2026-07-20T12:00:00.000Z",
        "hlcWallMs": 1784548800000_i64, "hlcCounter": 0});
    let fields = match queue {
        "commands" => json!({"deviceSequence": 1, "timerId": "timer-a",
            "type": "start", "phase": "focus", "plannedDurationMs": 60000,
            "observedElapsedMs": 0}),
        "taskOperations" => json!({"type": "delete", "taskId": "task-a"}),
        "durationOperations" => json!({"phase": "focus", "durationMs": 60000}),
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

fn call(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("reconcile.rebase.v2", &input.to_string()).unwrap())
        .unwrap()
}

#[test]
fn retained_delete_preserves_omitted_empty_and_nonempty_titles() {
    for title in [None, Some(""), Some("accepted delete title")] {
        let mut input = request();
        let mut delete = operation("taskOperations");
        if let Some(title) = title {
            delete["title"] = json!(title);
        }
        input["local"]["taskOperations"] = json!([delete]);
        let output = call(&input);
        assert_eq!(
            output["pendingTaskOperations"],
            input["local"]["taskOperations"]
        );
        input["local"]["taskOperations"] =
            serde_json::from_str(&output["pendingTaskOperations"].to_string()).unwrap();
        input["response"]["serverHlcCounter"] = json!(99);
        assert_eq!(
            call(&input)["pendingTaskOperations"],
            output["pendingTaskOperations"]
        );
    }
}

fn assert_retained(queue: &str, sent: bool) {
    let mut input = request();
    let mut original = operation(queue);
    original["extension"] = json!({"id": "not-the-operation-id", "type": "invalid",
        "taskId": "not-the-task-id", "nested": [null, {}, [], "", 42, false]});
    original["futureFlag"] = Value::Null;
    input["local"][queue] = json!([original]);
    if sent {
        input["sent"][queue] = json!([{"id": "shared-id"}]);
        // A missing acknowledgement rejects the response. The client retains
        // its local queue and retries without claiming it was never sent.
        let error = dispatch_json("reconcile.rebase.v2", &input.to_string()).unwrap_err();
        assert!(
            error.to_string().contains("Acknowledgements set")
                || error.to_string().contains("acknowledgements set")
        );
        input["sent"] = json!({});
    }
    let output_name = QUEUES.iter().find(|(name, _)| *name == queue).unwrap().1;
    let output = call(&input);
    assert_eq!(
        output[output_name], input["local"][queue],
        "{queue}, sent={sent}"
    );
    assert_eq!(output["projectionPending"][queue], json!([]));
    input["local"][queue] = serde_json::from_str(&output[output_name].to_string()).unwrap();
    input["sent"] = json!({});
    input["response"]["serverHlcCounter"] = json!(99);
    assert_eq!(
        call(&input)[output_name],
        output[output_name],
        "restart {queue}"
    );
}

#[test]
fn retained_commands_preserve_extensions() {
    for sent in [false, true] {
        assert_retained("commands", sent);
    }
}

#[test]
fn retained_tasks_preserve_extensions() {
    for sent in [false, true] {
        assert_retained("taskOperations", sent);
    }
}

#[test]
fn retained_durations_preserve_extensions() {
    for sent in [false, true] {
        assert_retained("durationOperations", sent);
    }
}

#[test]
fn retained_auto_start_preserves_extensions() {
    for sent in [false, true] {
        assert_retained("autoStartOperations", sent);
    }
}

#[test]
fn retained_selected_task_preserves_extensions() {
    for sent in [false, true] {
        assert_retained("selectedTaskOperations", sent);
    }
}

#[test]
fn original_objects_are_keyed_by_domain_and_validated_identity() {
    let mut input = request();
    for (queue, _) in QUEUES {
        let mut original = operation(queue);
        original["extension"] = json!({"domain": queue, "id": "forged"});
        input["local"][queue] = json!([original]);
    }
    let output = call(&input);
    for (queue, pending) in QUEUES {
        assert_eq!(output[pending], input["local"][queue], "{queue}");
    }
}

#[test]
fn extensions_cannot_hide_duplicate_or_invalid_operation_ids() {
    for (queue, _) in QUEUES {
        let mut input = request();
        let original = operation(queue);
        let mut duplicate = original.clone();
        duplicate["extension"] = json!({"id": "unique-extension-id"});
        input["local"][queue] = json!([original, duplicate]);
        assert!(
            dispatch_json("reconcile.rebase.v2", &input.to_string()).is_err(),
            "{queue}"
        );
        input["local"][queue] = json!([operation(queue)]);
        input["local"][queue][0]["id"] = json!("");
        input["local"][queue][0]["extension"] = json!({"id": "valid-extension-id"});
        assert!(
            dispatch_json("reconcile.rebase.v2", &input.to_string()).is_err(),
            "{queue}"
        );
    }
}

#[test]
fn acknowledged_objects_are_not_restored_from_original_payloads() {
    for (queue, pending) in QUEUES {
        let mut input = request();
        let mut original = operation(queue);
        original["extension"] = json!({"id": "forged"});
        input["local"][queue] = json!([original]);
        input["sent"][queue] = json!([{"id": "shared-id"}]);
        let (acks, id_field) = match queue {
            "commands" => ("acknowledgements", "commandId"),
            "taskOperations" => ("taskAcknowledgements", "operationId"),
            "durationOperations" => ("durationAcknowledgements", "operationId"),
            "autoStartOperations" => ("autoStartAcknowledgements", "operationId"),
            "selectedTaskOperations" => ("selectedTaskAcknowledgements", "operationId"),
            _ => unreachable!(),
        };
        input["response"][acks] = json!([{id_field: "shared-id", "outcome": "ignored"}]);
        assert_eq!(call(&input)[pending], json!([]), "{queue}");
    }
}

#[test]
fn duplicate_json_identity_fields_are_rejected_before_retention() {
    for (queue, _) in QUEUES {
        let mut input = request();
        input["local"][queue] = json!([operation(queue)]);
        let wire = input.to_string().replace(
            "\"id\":\"shared-id\"",
            "\"id\":\"shared-id\",\"id\":\"forged\"",
        );
        let error = dispatch_json("reconcile.rebase.v2", &wire).unwrap_err();
        assert!(
            error.to_string().contains("duplicate field"),
            "{queue}: {error}"
        );
    }
}

#[test]
fn extensions_do_not_change_typed_projection_or_validate_bad_values() {
    for (queue, _) in QUEUES {
        let mut input = request();
        input["local"][queue] = json!([operation(queue)]);
        input["local"][queue][0]["hlcWallMs"] = json!(1784548810001_i64);
        input["neverSent"] = json!({queue: ["shared-id"]});
        let baseline = call(&input);
        input["local"][queue][0]["extension"] = json!({"id": "forged",
            "taskId": "forged-task", "phase": "long_break", "enabled": false});
        assert_eq!(call(&input), baseline, "typed projection {queue}");
        input["local"][queue][0]["hlcCounter"] = json!(-1);
        input["local"][queue][0]["extension"]["hlcCounter"] = json!(0);
        assert!(
            dispatch_json("reconcile.rebase.v2", &input.to_string()).is_err(),
            "{queue}"
        );
    }
}

#[test]
fn pending_alias_retains_only_unacknowledged_original_objects() {
    let mut input = request();
    for (queue, _) in QUEUES {
        let mut original = operation(queue);
        original["extension"] = json!({"queue": queue});
        input["local"][queue] = json!([original]);
    }
    // Acknowledging another sent operation must not rewrite retained local ones.
    input["sent"]["commands"] = json!([{"id": "older-command"}]);
    input["response"]["acknowledgements"] =
        json!([{"commandId": "older-command", "outcome": "ignored"}]);
    input["pending"] = input.as_object_mut().unwrap().remove("local").unwrap();
    let output = call(&input);
    for (queue, pending) in QUEUES {
        assert_eq!(output[pending], input["pending"][queue], "{queue}");
    }
}
