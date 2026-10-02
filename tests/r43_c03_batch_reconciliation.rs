use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn empty_queues() -> Value {
    json!({"commands":[],"taskOperations":[],"durationOperations":[],
        "autoStartOperations":[],"selectedTaskOperations":[]})
}

fn command(id: &str, kind: &str, timer: &str, sequence: u64) -> Value {
    let mut command = json!({"id":id,"type":kind,"timerId":timer,
        "deviceId":"device-a","deviceSequence":sequence,"phase":"focus",
        "plannedDurationMs":60000,"observedElapsedMs":0,
        "occurredAt":"2026-07-20T12:00:01Z", "hlcWallMs":1784548801000_u64,
        "hlcCounter":sequence,"extension":{"null":null,"empty":""}});
    if kind == "retarget" {
        command["taskId"] = Value::Null;
    }
    command
}

fn plan(local: &Value, dependencies: &Value) -> Value {
    let mut queues = empty_queues();
    queues["commands"] = local["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| {
            json!({"id":command["id"],"deviceId":command["deviceId"],
            "deviceSequence":command["deviceSequence"],"hlcWallMs":command["hlcWallMs"],
            "hlcCounter":command["hlcCounter"]})
        })
        .collect();
    let dependencies: Vec<_> = dependencies.as_array().unwrap().iter().map(|edge| {
        json!({"operationId":edge["operationId"],"dependsOnOperationId":edge["dependsOnOperationId"]})
    }).collect();
    call(
        "sync.batchPlan.v1",
        &json!({"kind":"new","mode":"sync","queues":queues,
        "limits":{"perDomain":256,"total":512},"nextDomain":"commands",
        "timerDependencies":dependencies}),
    )
}

fn response(acks: Value, timer: Value, history: Value) -> Value {
    json!({"acknowledgements":acks,"taskAcknowledgements":[],"durationAcknowledgements":[],
        "autoStartAcknowledgements":[],"selectedTaskAcknowledgements":[],"revision":1,
        "canonicalTimer":timer,"history":history,"tasks":[],"selectedTaskId":null,
        "durationsMs":{"focus":60000,"short_break":300000,"long_break":900000},
        "autoStartBreaks":true,"serverTime":"2026-07-20T12:00:02Z",
        "serverHlcWallMs":1784548802000_u64,"serverHlcCounter":0})
}

fn reconcile(local: &Value, dependencies: &Value, response: Value, never_sent: &[&str]) -> Value {
    let selected = plan(local, dependencies)["selected"]["commands"].clone();
    let mut sent = empty_queues();
    sent["commands"] = local["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|command| selected.as_array().unwrap().contains(&command["id"]))
        .cloned()
        .collect();
    call(
        "reconcile.rebase.v2",
        &json!({"local":local,"sent":sent,"response":response,
        "timerDependencies":dependencies,"neverSent":{"commands":never_sent}}),
    )
}

#[test]
fn held_retarget_uses_real_acknowledgement_promotion_and_keeps_original_payload() {
    let start = command("start", "start", "focus", 1);
    let retarget = command("retarget", "retarget", "focus", 2);
    let mut local = empty_queues();
    local["commands"] = json!([retarget.clone(), start.clone()]);
    let deps = json!([{"operationId":"retarget","dependsOnOperationId":"start"}]);
    assert_eq!(
        plan(&local, &deps)["selected"]["commands"],
        json!(["start"])
    );
    let reduced = call(
        "timer.reduce.v1",
        &json!({"commands":[start],"now":"2026-07-20T12:00:02Z"}),
    );
    let response = response(
        json!([{"commandId":"start","outcome":"applied"}]),
        reduced["canonicalTimer"].clone(),
        reduced["history"].clone(),
    );
    let rebased = reconcile(&local, &deps, response, &[]);
    assert_eq!(rebased["pending"], json!([retarget]));
    assert_eq!(rebased["pendingTimerDependencies"], json!([]));
    local["commands"] = rebased["pending"].clone();
    assert_eq!(
        plan(&local, &rebased["pendingTimerDependencies"])["selected"]["commands"],
        json!(["retarget"])
    );
}

#[test]
fn generated_finish_start_waits_for_exact_completion_reconciliation() {
    let finish = command("finish", "finish", "focus", 1);
    let mut start = command("generated-start", "start", "break", 2);
    start["phase"] = json!("short_break");
    start["plannedDurationMs"] = json!(300000);
    let mut retarget = command("retarget", "retarget", "break", 3);
    retarget["phase"] = json!("short_break");
    retarget["plannedDurationMs"] = json!(300000);
    let mut local = empty_queues();
    local["commands"] = json!([retarget, start, finish]);
    let deps = json!([
        {"operationId":"generated-start","dependsOnOperationId":"finish","generatedBreak":true,
            "sourceDayStart":"2026-07-20T00:00:00Z","sourceDayEnd":"2026-07-21T00:00:00Z"},
        {"operationId":"retarget","dependsOnOperationId":"generated-start"}]);
    assert_eq!(
        plan(&local, &deps)["selected"]["commands"],
        json!(["finish"])
    );
    let history = json!([{"id":"focus","timerId":"focus","phase":"focus","status":"completed",
        "plannedDurationMs":60000,"commandId":"finish","completedAt":"2026-07-20T12:00:01Z"}]);
    let response = response(
        json!([{"commandId":"finish","outcome":"applied"}]),
        Value::Null,
        history,
    );
    let rebased = reconcile(&local, &deps, response, &["generated-start", "retarget"]);
    assert_eq!(
        rebased["pendingTimerDependencies"],
        json!([{
            "operationId": "retarget", "dependsOnOperationId": "generated-start"
        }])
    );
    local["commands"] = rebased["pending"].clone();
    let before = local.clone();
    let after = plan(&local, &rebased["pendingTimerDependencies"]);
    assert_eq!(after["selected"]["commands"], json!(["generated-start"]));
    // Selection leaves retained payloads untouched while holding retarget.
    assert_eq!(local, before);
}
