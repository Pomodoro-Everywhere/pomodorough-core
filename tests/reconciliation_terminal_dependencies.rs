use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn queues() -> Value {
    json!({"commands":[],"taskOperations":[],"durationOperations":[],
        "autoStartOperations":[],"selectedTaskOperations":[]})
}

fn generated_plan() -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let mut input = fixture["request"].clone();
    input.as_object_mut().unwrap().remove("intent");
    input["compatibility"] = json!("pwaStorage");
    input["stage"] = json!("finishCommit");
    input["selection"]["explicit"] = json!(false);
    input["workspace"]["base"]["autoStartBreaks"] = json!(true);
    let task = call(
        "task.identity.v1",
        &json!({"title":"Actual attributed focus"}),
    );
    let mut timer = fixture["timer"].clone();
    timer["taskId"] = task["id"].clone();
    input["workspace"]["base"]["canonicalTimer"] = timer.clone();
    input["requestedTimer"] = timer;
    input["workspace"]["base"]["tasks"] = json!([task]);
    input["workspace"]["base"]["history"] = (0..3).map(|i| json!({"id":format!("past-{i}"),
        "timerId":format!("past-{i}"),"phase":"focus","status":"completed", "plannedDurationMs":60000,
        "completedAt":"2026-07-20T11:00:00Z"})).collect();
    input["ownership"] = json!({"timerId":"existing-timer","deviceId":"device-local", "tabId":"tab-local",
        "leaseExpiresAtMs":1784548870000_i64});
    input["localTabId"] = json!("tab-local");
    input["leaseNowMs"] = json!(1784548810000_i64);
    input["leaseDurationMs"] = json!(30000);
    call("workspace.completionMutation.v1", &input)
}

fn response(projection: &Value, id: &Value, outcome: &str, counter: i64) -> Value {
    json!({"acknowledgements":[{"commandId":id,"outcome":outcome,"reason":""}],
        "taskAcknowledgements":[],"durationAcknowledgements":[],"autoStartAcknowledgements":[],
        "selectedTaskAcknowledgements":[],"revision":2,"canonicalTimer":projection["canonicalTimer"],
        "history":projection["history"],"tasks":projection["tasks"],"durationsMs":projection["durationsMs"],
        "autoStartBreaks":projection["autoStartBreaks"],"selectedTaskId":projection["selectedTaskId"],
        "serverTime":"2026-07-20T12:00:10Z","serverHlcWallMs":1784548810000_i64,"serverHlcCounter":counter})
}

fn first_input() -> Value {
    let plan = generated_plan();
    assert_eq!(plan["commands"].as_array().unwrap().len(), 2);
    assert_eq!(plan["commands"][1]["phase"], "long_break");
    assert!(plan["commands"][0].get("taskId").is_none());
    let mut sent = queues();
    sent["commands"] = json!([plan["commands"][0]]);
    let canonical = call(
        "projection.apply.v2",
        &json!({"base":plan["workspace"]["base"],
        "pending":sent,"now":"2026-07-20T12:00:10Z"}),
    );
    let mut local = plan["workspace"]["local"].clone();
    for command in local["commands"].as_array_mut().unwrap() {
        command["extension"] = json!({"null":null,"empty":"","nested":[{},false]});
    }
    let mut child = local["commands"][1].clone();
    child["id"] = json!("pause-child");
    child["type"] = json!("pause");
    child["deviceSequence"] = json!(10);
    child["hlcCounter"] = json!(3);
    local["commands"].as_array_mut().unwrap().push(child);
    let start = &plan["commands"][1];
    let mut dependencies = plan["workspace"]["timerDependencies"].clone();
    dependencies
        .as_array_mut()
        .unwrap()
        .push(json!({"operationId":"pause-child","dependsOnOperationId":start["id"]}));
    json!({"local":local,"sent":sent,
        "response":response(&canonical,&plan["commands"][0]["id"],"applied",plan["commands"][0]["hlcCounter"].as_i64().unwrap()),
        "timerDependencies":dependencies,"neverSent":{"commands":[start["id"],"pause-child"]}})
}

fn first_step() -> (Value, Value) {
    let input = first_input();
    let local = &input["local"];
    let start = &local["commands"][1];
    let canonical = &input["response"];
    let output = call("reconcile.rebase.v3", &input);
    assert_eq!(
        output["pending"],
        json!([local["commands"][1], local["commands"][2]])
    );
    assert_eq!(output["promotedTimerOperationIds"], json!([start["id"]]));
    assert_eq!(
        output["pendingTimerDependencies"],
        json!([{"operationId":"pause-child","dependsOnOperationId":start["id"]}])
    );
    assert_eq!(
        output["baseTimer"]["taskId"],
        canonical["canonicalTimer"]["taskId"]
    );
    assert_eq!(output["workspace"]["canonicalTimer"]["status"], "paused");
    (input, output)
}

fn second_step(input: &Value, first: &Value, outcome: &str, proof: bool) -> Value {
    let mut local = queues();
    local["commands"] = first["pending"].clone();
    let mut sent = queues();
    sent["commands"] = json!([first["pending"][0]]);
    let canonical = if outcome == "applied" {
        let base = json!({"canonicalTimer":input["response"]["canonicalTimer"],"history":input["response"]["history"],
            "tasks":input["response"]["tasks"],"durationsMs":input["response"]["durationsMs"],
            "autoStartBreaks":true,"selectedTaskId":null});
        call("workspace.project.v1", &json!({"base":base,"local":sent,"neverSent":{"commands":[first["pending"][0]["id"]]},
            "canonicalHead":{"wallMs":1784548810000_i64,"counter":input["response"]["serverHlcCounter"]},"timerDependencies":[],
            "now":"2026-07-20T12:00:10Z"}))["workspace"].clone()
    } else {
        json!({"canonicalTimer":input["response"]["canonicalTimer"],"history":input["response"]["history"],
            "tasks":input["response"]["tasks"],"durationsMs":input["response"]["durationsMs"],
            "autoStartBreaks":true,"selectedTaskId":null})
    };
    json!({"local":local,"sent":sent,"response":response(&canonical,&first["pending"][0]["id"],outcome,first["pending"][0]["hlcCounter"].as_i64().unwrap()),
        "timerDependencies":first["pendingTimerDependencies"],
        "neverSent":{"commands":if proof {json!(["pause-child"])} else {json!([])}}})
}

#[test]
fn exact_finish_ack_then_generated_start_reject_drops_only_never_sent_child() {
    let (input, first) = first_step();
    let second = second_step(&input, &first, "rejected", true);
    let before = second.clone();
    let output = call("reconcile.rebase.v3", &second);
    assert_eq!(output["pending"], json!([]));
    assert_eq!(output["droppedTimerOperationIds"], json!(["pause-child"]));
    assert_eq!(output["promotedTimerOperationIds"], json!([]));
    assert_eq!(output["pendingTimerDependencies"], json!([]));
    assert_eq!(output["timer"]["status"], "completed");
    assert_eq!(second, before);
    let frozen = second_step(&input, &first, "rejected", false);
    assert_eq!(
        dispatch_json("reconcile.rebase.v3", &frozen.to_string())
            .unwrap_err()
            .to_string(),
        "invalid shared-core input: reconciliation would discard a possibly delivered dependent"
    );
}

#[test]
fn exact_finish_ack_then_generated_start_apply_promotes_child_and_keeps_payload() {
    let (input, first) = first_step();
    let second = second_step(&input, &first, "applied", true);
    let output = call("reconcile.rebase.v3", &second);
    assert_eq!(output["pending"], json!([first["pending"][1]]));
    assert_eq!(output["promotedTimerOperationIds"], json!(["pause-child"]));
    assert_eq!(output["droppedTimerOperationIds"], json!([]));
    assert_eq!(output["pendingTimerDependencies"], json!([]));
    assert_eq!(output["projectionPending"]["commands"], output["pending"]);
    assert_eq!(output["timer"]["status"], "paused");
    assert_eq!(output["canonicalResponse"], second["response"]);
}

#[test]
fn generated_normalization_changes_only_authorized_fields_and_preserves_raw_extensions() {
    let mut input = first_input();
    for command in input["local"]["commands"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .skip(1)
    {
        command["phase"] = json!("short_break");
        command["plannedDurationMs"] = json!(120000);
    }
    let before = input.clone();
    let output = call("reconcile.rebase.v3", &input);
    for (retained, original) in output["pending"].as_array().unwrap().iter().zip(
        before["local"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1),
    ) {
        let mut expected = original.clone();
        expected["phase"] = json!("long_break");
        expected["plannedDurationMs"] = json!(180000);
        assert_eq!(*retained, expected);
    }
    assert_eq!(input, before);
    input["neverSent"] = json!({});
    assert_eq!(
        dispatch_json("reconcile.rebase.v3", &input.to_string())
            .unwrap_err()
            .to_string(),
        "invalid shared-core input: reconciliation would rewrite a possibly delivered operation"
    );
}
