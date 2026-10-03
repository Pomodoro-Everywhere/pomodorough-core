use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

fn request() -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/reconciliation-terminal-v3.json")).unwrap();
    let response: Value =
        serde_json::from_str(fixture["http"]["responseRaw"].as_str().unwrap()).unwrap();
    json!({"local":fixture["local"],"sent":fixture["local"],"response":response,
        "neverSent":fixture["neverSent"],"timerDependencies":fixture["timerDependencies"]})
}

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

#[test]
fn actual_http_finish_preserves_raw_evidence_and_projects_terminal_metadata() {
    let input = request();
    let before = input.clone();
    let output = call("reconcile.rebase.v3", &input);
    let envelope: Value = serde_json::from_str(&dispatch_envelope_json(
        "reconcile.rebase.v3",
        &input.to_string(),
    ))
    .unwrap();
    assert_eq!(envelope, json!({"ok":true,"value":output}));
    assert_output_schema(&output);
    assert_eq!(output["schemaVersion"], 3);
    assert_eq!(output["canonicalResponse"], input["response"]);
    assert_eq!(output["baseTimer"], input["response"]["canonicalTimer"]);
    assert_eq!(output["baseHistory"], input["response"]["history"]);
    assert_eq!(output["timer"]["startedByDeviceId"], "device-0001");
    assert_eq!(
        output["timer"]["lastIntent"]["commandId"],
        input["local"]["commands"][0]["id"]
    );
    assert_eq!(output["history"], input["response"]["history"]);
    assert_eq!(output["pending"], json!([]));
    assert_eq!(input, before);
    let expected = call("workspace.project.v1", &workspace_request(&input, &output));
    assert_eq!(output["workspace"], expected["workspace"]);
    assert_eq!(output["projectionPending"], expected["projectionPending"]);
}

fn assert_output_schema(output: &Value) {
    let mut fields = vec![
        "revision",
        "pending",
        "pendingTaskOperations",
        "pendingDurationOperations",
        "pendingAutoStartOperations",
        "pendingSelectedTaskOperations",
        "pendingTimerDependencies",
        "promotedTimerOperationIds",
        "droppedTimerOperationIds",
        "droppedTimerIds",
        "baseTimer",
        "baseHistory",
        "baseTasks",
        "baseDurationsMs",
        "baseAutoStartBreaks",
        "baseSelectedTaskId",
        "projectionPending",
        "timer",
        "history",
        "tasks",
        "durationsMs",
        "autoStartBreaks",
        "selectedTaskId",
        "schemaVersion",
        "canonicalResponse",
        "workspace",
    ];
    fields.sort();
    assert_eq!(
        output
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        fields
    );
    assert_eq!(
        output["workspace"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "autoStartBreaks",
            "canonicalTimer",
            "durationsMs",
            "history",
            "selectedTaskId",
            "tasks",
            "timerOutcomes",
            "winningOperationIds"
        ]
    );
}

fn workspace_request(input: &Value, output: &Value) -> Value {
    let response = &input["response"];
    json!({"base":{"canonicalTimer":response["canonicalTimer"],"history":response["history"],
        "tasks":response["tasks"],"durationsMs":response["durationsMs"],
        "autoStartBreaks":response["autoStartBreaks"],"selectedTaskId":response["selectedTaskId"]},
        "local":output["projectionPending"],"neverSent":{},
        "canonicalHead":{"wallMs":response["serverHlcWallMs"],"counter":response["serverHlcCounter"]},
        "timerDependencies":[],"now":response["serverTime"]})
}

#[test]
fn strict_old_reconciliation_still_rejects_actual_http_overlap() {
    for operation in ["reconcile.rebase.v1", "reconcile.rebase.v2"] {
        assert_eq!(
            dispatch_json(operation, &request().to_string())
                .unwrap_err()
                .to_string(),
            "invalid shared-core input: canonical timer overlaps timer history"
        );
    }
}

fn terminal_request(case: &Value) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-terminal-v1.json")).unwrap();
    let mut input = request();
    input["local"] = fixture["request"]["local"].clone();
    input["sent"] = input["local"].clone();
    input["response"]["acknowledgements"] = json!([]);
    let mut timer = fixture["timer"].clone();
    let mut row = fixture["history"].clone();
    for (value, overrides) in [
        (&mut timer, &case["timerOverrides"]),
        (&mut row, &case["historyOverrides"]),
    ] {
        if let Some(fields) = overrides.as_object() {
            value.as_object_mut().unwrap().extend(fields.clone());
        }
    }
    input["response"]["canonicalTimer"] = if case["cleared"] == true {
        Value::Null
    } else {
        timer
    };
    input["response"]["history"] = if case["missingHistory"] == true {
        json!([])
    } else {
        json!([row])
    };
    if case["sameTimeSibling"] == true {
        let mut sibling = fixture["history"].clone();
        sibling["id"] = json!("aaa-sibling");
        sibling["timerId"] = json!("aaa-sibling");
        sibling["commandId"] = json!("finish-sibling");
        input["response"]["history"]
            .as_array_mut()
            .unwrap()
            .push(sibling);
    }
    if case["commands"].is_array() {
        input["local"]["commands"] = json!([fixture["command"]]);
        if case["safe"] == true {
            input["neverSent"]["commands"] = json!([fixture["command"]["id"]]);
        }
    }
    if case["retainedCommands"].is_array() {
        input["local"]["commands"] = case["retainedCommands"].clone();
    }
    input["response"]["serverTime"] = json!("2026-07-20T12:00:00Z");
    input["response"]["serverHlcWallMs"] = json!(1784548800000_i64);
    input
}

#[test]
fn terminal_pair_matrix_uses_same_workspace_semantics_and_keeps_raw_base() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-terminal-v1.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let input = terminal_request(case);
        let output = call("reconcile.rebase.v3", &input);
        let mut workspace = workspace_request(&input, &output);
        workspace["local"] = input["local"].clone();
        workspace["neverSent"] = input["neverSent"].clone();
        let expected = call("workspace.project.v1", &workspace);
        assert_eq!(
            output["workspace"], expected["workspace"],
            "{}",
            case["name"]
        );
        assert_eq!(
            output["projectionPending"], expected["projectionPending"],
            "{}",
            case["name"]
        );
        assert_eq!(output["baseTimer"], input["response"]["canonicalTimer"]);
        assert_eq!(output["canonicalResponse"], input["response"]);
    }
}

#[test]
fn conflicting_terminal_pairs_and_c02_identity_collision_fail_without_repair() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-terminal-v1.json")).unwrap();
    for case in fixture["rejections"].as_array().unwrap() {
        let input = terminal_request(case);
        let error = dispatch_json("reconcile.rebase.v3", &input.to_string())
            .unwrap_err()
            .to_string();
        let workspace = workspace_request(&input, &json!({"projectionPending":input["local"]}));
        let expected = dispatch_json("workspace.project.v1", &workspace.to_string())
            .unwrap_err()
            .to_string();
        assert_eq!(error, expected, "{}", case["name"]);
        if case["missingHistory"] != true {
            assert!(
                dispatch_json("reconcile.rebase.v2", &input.to_string()).is_err(),
                "{}",
                case["name"]
            );
        }
    }
}

#[test]
fn raw_canonical_extensions_and_native_intent_metadata_survive_as_evidence() {
    let mut input = request();
    input["response"]["canonicalTimer"]["lastIntent"]["deviceId"] = json!("origin-device");
    input["response"]["canonicalTimer"]["lastIntent"]["extension"] =
        json!({"empty":"","null":null});
    input["response"]["canonicalTimer"]["extension"] = json!([null, "", {"nested":false}]);
    input["response"]["history"][0]["extension"] = json!({"kept":true});
    let output = call("reconcile.rebase.v3", &input);
    assert_eq!(output["canonicalResponse"], input["response"]);
    assert_eq!(output["baseTimer"], input["response"]["canonicalTimer"]);
    assert_eq!(output["baseHistory"], input["response"]["history"]);
    assert_eq!(
        output["workspace"]["canonicalTimer"]["lastIntent"]["deviceId"],
        "origin-device"
    );
    for invalid in [json!(""), json!(false), json!(9), json!({})] {
        input["response"]["canonicalTimer"]["lastIntent"]["deviceId"] = invalid;
        assert!(dispatch_json("reconcile.rebase.v3", &input.to_string()).is_err());
    }
}

fn retained_operation(queue: &str, id: &str) -> Value {
    let mut operation = json!({"id":id,"deviceId":"retained-device",
        "occurredAt":"2026-10-02T12:02:58Z","hlcWallMs":1790942578000_i64,"hlcCounter":0,
        "extension":{"null":null,"empty":"","nested":[false,{}]}});
    let fields = match queue {
        "commands" => json!({"deviceSequence":1,"timerId":"new-timer","type":"start",
            "phase":"focus","plannedDurationMs":60000,"observedElapsedMs":0,"taskId":null}),
        "taskOperations" => json!({"type":"delete","taskId":"deleted-task","title":""}),
        "durationOperations" => json!({"phase":"focus","durationMs":120000}),
        "autoStartOperations" => json!({"enabled":true}),
        "selectedTaskOperations" => json!({"taskId":null}),
        _ => unreachable!(),
    };
    operation
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    operation
}

#[test]
fn partial_delivery_proof_retains_all_five_raw_queues_and_suppresses_whole_domain() {
    let mut input = request();
    for (queue, pending) in [
        ("commands", "pending"),
        ("taskOperations", "pendingTaskOperations"),
        ("durationOperations", "pendingDurationOperations"),
        ("autoStartOperations", "pendingAutoStartOperations"),
        ("selectedTaskOperations", "pendingSelectedTaskOperations"),
    ] {
        let a = retained_operation(queue, "frozen");
        let mut b = retained_operation(queue, "never-sent");
        b["hlcCounter"] = json!(1);
        if queue == "commands" {
            b["deviceSequence"] = json!(2);
            b["timerId"] = json!("another-timer");
        }
        input["local"][queue]
            .as_array_mut()
            .unwrap()
            .extend([a.clone(), b.clone()]);
        input["neverSent"][queue] = json!(["never-sent"]);
        let before = input.clone();
        let output = call("reconcile.rebase.v3", &input);
        assert_eq!(output[pending], json!([a, b]));
        assert_eq!(output["projectionPending"][queue], json!([]));
        assert_eq!(input, before);
    }
    let output = call("reconcile.rebase.v3", &input);
    assert_eq!(output["timer"]["status"], "completed");
    assert_eq!(output["workspace"]["timerOutcomes"], json!({}));
}

#[test]
fn safe_raw_queues_preserve_omission_null_and_extensions_without_clock_changes() {
    let mut input = request();
    for queue in [
        "commands",
        "taskOperations",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ] {
        let mut operation = retained_operation(queue, queue);
        if queue == "taskOperations" {
            operation.as_object_mut().unwrap().remove("title");
        }
        input["local"][queue]
            .as_array_mut()
            .unwrap()
            .push(operation.clone());
        input["neverSent"][queue] = json!([queue]);
        let output = call("reconcile.rebase.v3", &input);
        assert_eq!(output["projectionPending"][queue], json!([operation]));
    }
}

#[test]
fn completion_state_consumes_original_raw_canonical_pair_after_v3() {
    let input = request();
    let output = call("reconcile.rebase.v3", &input);
    let canonical = &output["canonicalResponse"];
    let acknowledgements: Vec<_> = canonical["acknowledgements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ack| json!({"commandId":ack["commandId"],"outcome":ack["outcome"]}))
        .collect();
    let mut state = json!({"kind":"install","compatibility":"desktopD03", "beforeHistory":[],
        "afterHistory":canonical["history"],"canonicalTimer":canonical["canonicalTimer"],
        "selection":{"phase":"focus","generation":"0","explicit":false},
        "pending":{"commandIds":output["pending"],"sendableCommandIds":[],"otherOperationIds":[]},
        "advances":[],"acknowledgements":acknowledgements,"discardedCommandIds":[],
        "referenceTime":canonical["serverTime"],
        "calendarIntervals":[{"start":"2026-10-02T00:00:00Z","end":"2026-10-03T00:00:00Z"}]});
    let result = call("timer.completionState.v1", &state);
    assert_eq!(result["reason"], "completionSelected");
    assert_eq!(
        result["source"]["commandId"],
        canonical["history"][0]["commandId"]
    );
    assert_eq!(result["selection"]["phase"], "short_break");
    state["compatibility"] = json!("pwaRejectedFinish");
    state["selection"]["phase"] = json!("short_break");
    state["sentContext"] = json!({"kind":"pwa","commands":input["sent"]["commands"],
        "rollbackHistory":canonical["history"]});
    let pwa = call("timer.completionState.v1", &state);
    assert_eq!(pwa["reason"], "sentFinishesReconciled");
    assert_eq!(pwa["selection"]["phase"], "short_break");
}

#[test]
fn raw_boundary_ack_sets_delivery_proof_and_suppressed_payloads_fail_closed() {
    let mut inputs = vec![];
    let mut missing = request();
    missing["response"]["acknowledgements"] = json!([]);
    inputs.push(missing);
    let mut extra = request();
    extra["response"]["acknowledgements"][0]["commandId"] = json!("wrong");
    inputs.push(extra);
    let mut duplicate = request();
    let ack = duplicate["response"]["acknowledgements"][0].clone();
    duplicate["response"]["acknowledgements"]
        .as_array_mut()
        .unwrap()
        .push(ack);
    inputs.push(duplicate);
    let mut proof = request();
    proof["neverSent"]["commands"] = json!([proof["local"]["commands"][0]["id"]]);
    inputs.push(proof);
    let mut malformed = request();
    malformed["local"]["commands"][0]["phase"] = json!("unknown");
    inputs.push(malformed);
    let mut time = request();
    time["local"]["commands"][0]["occurredAt"] = json!("2026-10-02T12:02:57.006Z");
    inputs.push(time);
    let mut hlc = request();
    hlc["response"]["serverHlcCounter"] = json!(9007199254740992_i64);
    inputs.push(hlc);
    for input in inputs {
        assert!(dispatch_json("reconcile.rebase.v3", &input.to_string()).is_err());
    }
    let raw = request().to_string().replacen(
        "\"elapsedAtAnchorMs\":1500000",
        "\"elapsedAtAnchorMs\":1500000,\"elapsedAtAnchorMs\":1500000",
        1,
    );
    assert!(
        dispatch_json("reconcile.rebase.v3", &raw)
            .unwrap_err()
            .to_string()
            .contains("duplicate field")
    );
}
