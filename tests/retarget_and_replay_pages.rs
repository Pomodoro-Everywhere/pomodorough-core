use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn command(id: usize, kind: &str, timer: &str) -> Value {
    json!({"id": format!("command-{id:08}"), "deviceId": "device-a", "deviceSequence": id + 1,
        "timerId": timer, "type": kind, "phase": "focus", "plannedDurationMs": 60000,
        "occurredAt": "2026-09-13T12:00:01.123456789Z", "hlcWallMs": 1789300800000_i64,
        "hlcCounter": id, "observedElapsedMs": 1234})
}

fn call(operation: &str, input: Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

#[test]
fn retarget_preserves_start_and_pause_timing_and_unassigns_history() {
    let mut start = command(0, "start", "timer-a");
    start["taskId"] = json!("task-original");
    let pause = command(1, "pause", "timer-a");
    let base = call(
        "timer.reduce.v1",
        json!({"commands": [start.clone(), pause.clone()],
        "now": "2026-09-13T12:00:02Z"}),
    );
    let mut retarget = command(2, "retarget", "timer-a");
    retarget["taskId"] = Value::Null;
    let result = call(
        "timer.reduce.v1",
        json!({"commands": [start.clone(), pause, retarget.clone()],
        "now": "2026-09-13T12:00:02Z"}),
    );
    for field in ["anchorAt", "elapsedAtAnchorMs", "status", "lastIntent"] {
        assert_eq!(
            result["canonicalTimer"][field],
            base["canonicalTimer"][field]
        );
    }
    assert!(result["canonicalTimer"].get("taskId").is_none());
    assert_eq!(start["taskId"], "task-original");
    let result = call(
        "timer.reduce.v1",
        json!({"commands": [start, retarget, command(3,"finish","timer-a")],
        "now": "2026-09-13T12:00:02Z"}),
    );
    assert!(result["history"][0].get("taskId").is_none());
    assert_eq!(result["history"][0]["commandId"], "command-00000003");
}

#[test]
fn retarget_validates_presence_and_ignores_inactive_or_break_targets() {
    for invalid in [None, Some(json!("")), Some(json!(false))] {
        let mut retarget = command(1, "retarget", "timer-a");
        if let Some(value) = invalid {
            retarget["taskId"] = value;
        }
        assert!(
            dispatch_json(
                "timer.reduce.v1",
                &json!({"commands": [retarget],
            "now": "2026-09-13T12:00:02Z"})
                .to_string()
            )
            .is_err()
        );
    }
    for phase in ["focus", "short_break"] {
        let mut start = command(0, "start", "timer-a");
        start["phase"] = json!(phase);
        let mut retarget = command(2, "retarget", "timer-a");
        retarget["taskId"] = json!("task-next");
        let result = call(
            "timer.reduce.v1",
            json!({"commands": [start,
            command(1,"finish","timer-a"), retarget], "now": "2026-09-13T12:00:02Z"}),
        );
        assert_eq!(result["outcomes"]["command-00000002"]["outcome"], "ignored");
    }
}

#[test]
fn replay_pages_match_full_replay_across_clear_supersede_and_resurrection() {
    let kinds = [
        "start", "pause", "start", "resume", "cancel", "clear", "resume", "finish",
    ];
    let commands: Vec<_> = (0..128)
        .map(|id| {
            command(
                id,
                kinds[id % kinds.len()],
                if id % 3 == 0 { "timer-a" } else { "timer-b" },
            )
        })
        .collect();
    let now = "2026-09-13T12:04:00Z";
    let full = call("timer.reduce.v1", json!({"commands": commands, "now": now}));
    let mut page = json!({"sessions": [], "currentTimerId": null, "after": null});
    let mut outcomes = serde_json::Map::new();
    for (index, batch) in commands.chunks(7).enumerate() {
        let mut input = json!({"commands": batch, "sessions": page["sessions"],
            "currentTimerId": page["currentTimerId"], "after": page["after"]});
        if (index + 1) * 7 >= commands.len() {
            input["now"] = json!(now);
        }
        page = call("timer.replay.page.v1", input);
        outcomes.extend(page["outcomes"].as_object().unwrap().clone());
    }
    for field in ["sessions", "history", "canonicalTimer"] {
        assert_eq!(page[field], full[field], "{field}");
    }
    assert_eq!(Value::Object(outcomes), full["outcomes"]);
}

#[test]
fn replay_page_rejects_duplicate_or_reversed_order_and_missing_current() {
    let base = json!({"sessions": [], "currentTimerId": null, "after": null});
    for commands in [
        vec![command(1, "start", "a"), command(1, "start", "a")],
        vec![command(2, "start", "a"), command(1, "start", "a")],
        vec![command(1, "start", "a"); 257],
    ] {
        let mut input = base.clone();
        input["commands"] = json!(commands);
        assert!(dispatch_json("timer.replay.page.v1", &input.to_string()).is_err());
    }
    let mut input = base;
    input["commands"] = json!([]);
    input["currentTimerId"] = json!("absent");
    assert!(dispatch_json("timer.replay.page.v1", &input.to_string()).is_err());
}

const REBASE_WALL_MS: i64 = 1_784_548_800_000;

fn immutable_request() -> Value {
    let start = rebase_command("command-start", "start", 1, Some("task-original"));
    let mut retarget = rebase_command("command-retarget", "retarget", 2, Some("task-next"));
    retarget["hlcCounter"] = json!(1);
    json!({"local": rebase_queues(vec![start, retarget]), "sent": rebase_queues(vec![]),
        "response": rebase_response(json!([])), "timerDependencies": []})
}

#[test]
fn immutable_reconciliation_does_not_discard_a_possibly_delivered_dependent() {
    let mut input = immutable_request();
    input["sent"]["commands"] = json!([{"id": "command-start"}]);
    input["response"]["acknowledgements"] = json!([{
        "commandId": "command-start", "outcome": "rejected", "reason": "conflict"}]);
    input["timerDependencies"] = json!([{
        "operationId": "command-retarget", "dependsOnOperationId": "command-start"}]);
    let error = dispatch_json("reconcile.rebase.v2", &input.to_string()).unwrap_err();
    assert!(error.to_string().contains("possibly delivered dependent"));
    input["neverSent"] = json!({"commands": ["command-retarget"]});
    let output = call("reconcile.rebase.v2", input);
    assert_eq!(output["pending"], json!([]));
    assert_eq!(
        output["droppedTimerOperationIds"],
        json!(["command-retarget"])
    );
}

#[test]
fn immutable_reconciliation_freezes_every_operation_domain() {
    let mut input = immutable_request();
    let operation = json!({"id": "operation-a", "deviceId": "device-a", "occurredAt": "2026-07-20T11:53:20Z",
        "hlcWallMs": REBASE_WALL_MS - 400_000, "hlcCounter": 0});
    for (queue, fields) in [
        (
            "taskOperations",
            json!({"type": "delete", "taskId": "task-a"}),
        ),
        (
            "durationOperations",
            json!({"phase": "focus", "durationMs": 60000}),
        ),
        ("autoStartOperations", json!({"enabled": true})),
        ("selectedTaskOperations", json!({"taskId": null})),
    ] {
        let mut value = operation.clone();
        value
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        input["local"][queue] = json!([value]);
    }
    let output = call("reconcile.rebase.v2", input);
    for queue in [
        "pendingTaskOperations",
        "pendingDurationOperations",
        "pendingAutoStartOperations",
        "pendingSelectedTaskOperations",
    ] {
        for field in ["id", "occurredAt", "hlcWallMs", "hlcCounter"] {
            assert_eq!(output[queue][0][field], operation[field], "{queue}.{field}");
        }
    }
    assert!(output["pendingSelectedTaskOperations"][0]["taskId"].is_null());
}

#[test]
fn immutable_reconciliation_preserves_unknown_delivery_and_causal_order() {
    let input = immutable_request();
    let result = call("reconcile.rebase.v2", input.clone());
    assert_eq!(result["pending"], input["local"]["commands"]);
    let reduced = call(
        "timer.reduce.v1",
        json!({"commands": result["pending"],
        "now": "2026-07-20T11:53:21Z"}),
    );
    assert_eq!(reduced["canonicalTimer"]["taskId"], "task-next");
    let mut restarted: Value = serde_json::from_str(&input.to_string()).unwrap();
    restarted["local"]["commands"] = result["pending"].clone();
    restarted["response"]["serverHlcCounter"] = json!(99);
    let retry = call("reconcile.rebase.v2", restarted);
    assert_eq!(retry["pending"], input["local"]["commands"]);
}

#[test]
fn immutable_reconciliation_accepts_start_ack_without_rewriting_remaining_retarget() {
    let mut input = immutable_request();
    let canonical = call(
        "timer.reduce.v1",
        json!({"commands": [input["local"]["commands"][0]],
        "now": "2026-07-20T11:53:21Z"}),
    );
    input["response"]["canonicalTimer"] = canonical["canonicalTimer"].clone();
    input["response"]["serverTime"] = json!("2026-07-20T11:53:21Z");
    input["response"]["serverHlcWallMs"] = json!(REBASE_WALL_MS - 399_000);
    input["response"]["acknowledgements"] = json!([{
        "commandId": "command-start", "outcome": "applied", "reason": ""}]);
    input["sent"]["commands"] = json!([{"id": "command-start"}]);
    let output = call("reconcile.rebase.v2", input.clone());
    assert_eq!(output["pending"], json!([input["local"]["commands"][1]]));
    assert_eq!(output["timer"]["taskId"], "task-original");
}

#[test]
fn immutable_reconciliation_does_not_replay_lost_ack_start_over_newer_pause() {
    let mut input = immutable_request();
    input["response"]["canonicalTimer"] = json!({"id": "timer-a", "taskId": "remote-task", "phase": "focus",
        "status": "paused", "plannedDurationMs": 60000, "elapsedAtAnchorMs": 12000,
        "anchorAt": "2026-07-20T12:00:00Z", "startedByDeviceId": "device-a",
        "lastIntent": {"type": "pause", "commandId": "remote-pause", "occurredAt": "2026-07-20T12:00:00Z"}});
    let output = call("reconcile.rebase.v2", input.clone());
    assert_eq!(output["pending"], input["local"]["commands"]);
    assert_eq!(output["timer"], input["response"]["canonicalTimer"]);
    assert_eq!(output["projectionPending"], rebase_queues(vec![]));
}

#[test]
fn immutable_reconciliation_rejects_reversed_cross_device_dependencies() {
    let mut input = immutable_request();
    input["local"]["commands"][1]["deviceId"] = json!("device-b");
    input["timerDependencies"] = json!([{
        "operationId": "command-start", "dependsOnOperationId": "command-retarget"}]);
    let error = dispatch_json("reconcile.rebase.v2", &input.to_string()).unwrap_err();
    assert!(error.to_string().contains("not causally ordered"));
}

#[test]
fn immutable_reconciliation_never_rebases_even_wholly_never_sent_chains() {
    let mut input = immutable_request();
    input["neverSent"] = json!({"commands": ["command-start"]});
    let result = call("reconcile.rebase.v2", input.clone());
    assert_eq!(result["pending"], input["local"]["commands"]);
    input["neverSent"]["commands"] = json!(["command-start", "command-retarget"]);
    let result = call("reconcile.rebase.v2", input.clone());
    assert_eq!(result["pending"], input["local"]["commands"]);
    assert_eq!(result["projectionPending"]["commands"], json!([]));
}

#[test]
fn immutable_reconciliation_rejects_false_delivery_claims_and_reordered_clocks() {
    for claims in [
        json!({"commands": ["absent"]}),
        json!({"commands": ["command-start", "command-start"]}),
        json!({"commands": null}),
        json!({"typo": []}),
    ] {
        let mut input = immutable_request();
        input["neverSent"] = claims;
        assert!(dispatch_json("reconcile.rebase.v2", &input.to_string()).is_err());
    }
    let mut input = immutable_request();
    input["sent"]["commands"] = json!([{"id": "command-start"}]);
    input["neverSent"] = json!({"commands": ["command-start"]});
    assert!(dispatch_json("reconcile.rebase.v2", &input.to_string()).is_err());
    let mut input = immutable_request();
    input["local"]["commands"][1]["hlcCounter"] = json!(0);
    let error = dispatch_json("reconcile.rebase.v2", &input.to_string()).unwrap_err();
    assert!(error.to_string().contains("device sequence"));
}

fn rebase_command(id: &str, kind: &str, sequence: i64, task: Option<&str>) -> Value {
    let mut cmd = json!({"id": id, "deviceId": "device-a", "deviceSequence": sequence,
        "timerId": "timer-a", "type": kind, "phase": "focus", "plannedDurationMs": 60000,
        "occurredAt": "2026-07-20T11:53:20Z", "hlcWallMs": REBASE_WALL_MS - 400_000,
        "hlcCounter": 0, "observedElapsedMs": 0});
    if let Some(task) = task {
        cmd["taskId"] = json!(task);
    }
    cmd
}

fn rebase_queues(commands: Vec<Value>) -> Value {
    json!({"commands": commands, "taskOperations": [], "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []})
}

fn rebase_response(history: Value) -> Value {
    json!({
        "acknowledgements": [], "taskAcknowledgements": [], "durationAcknowledgements": [],
        "autoStartAcknowledgements": [], "selectedTaskAcknowledgements": [],
        "revision": 9, "canonicalTimer": null, "history": history, "tasks": [],
        "durationsMs": {"focus": 1_500_000, "short_break": 300_000, "long_break": 900_000},
        "autoStartBreaks": false, "selectedTaskId": null,
        "serverTime": "2026-07-20T12:00:10Z",
        "serverHlcWallMs": REBASE_WALL_MS + 10_000, "serverHlcCounter": 7})
}

fn rebase(local: Value) -> Value {
    let input = json!({"local": local, "sent": rebase_queues(vec![]),
        "response": rebase_response(json!([])), "timerDependencies": []});
    serde_json::from_str(&dispatch_json("reconcile.rebase.v1", &input.to_string()).unwrap())
        .unwrap()
}

// C19: retarget joins the HLC rebase chain in device-sequence order instead of
// keeping a stale clock that sorts before its causal start.
#[test]
fn retarget_participates_in_hlc_rebase_ordering() {
    let local = rebase_queues(vec![
        rebase_command("command-start", "start", 1, Some("task-original")),
        rebase_command("command-retarget", "retarget", 2, Some("task-next")),
    ]);
    let output = rebase(local);
    assert_eq!(output["pending"][0]["id"], "command-start");
    assert_eq!(output["pending"][0]["hlcWallMs"], REBASE_WALL_MS + 10_000);
    assert_eq!(output["pending"][0]["hlcCounter"], 8);
    assert_eq!(output["pending"][1]["id"], "command-retarget");
    assert_eq!(output["pending"][1]["hlcWallMs"], REBASE_WALL_MS + 10_000);
    assert_eq!(output["pending"][1]["hlcCounter"], 9);
    // Reduce must apply the retarget (not reject as unsupported) and move task.
    let mut start = command(0, "start", "timer-a");
    start["taskId"] = json!("task-original");
    let mut retarget = command(1, "retarget", "timer-a");
    retarget["taskId"] = json!("task-next");
    let reduced = call(
        "timer.reduce.v1",
        json!({"commands": [start, retarget], "now": "2026-09-13T12:00:02Z"}),
    );
    assert_eq!(
        reduced["outcomes"]["command-00000001"]["outcome"],
        "applied"
    );
    assert_eq!(reduced["canonicalTimer"]["taskId"], "task-next");
}

// C20: empty Selected task identity is InvalidInput for every command kind,
// matching validate_selected_task_fields/canonical_response.
#[test]
fn empty_selected_task_id_rejected_for_every_command_kind() {
    for kind in [
        "start", "pause", "resume", "finish", "cancel", "clear", "retarget",
    ] {
        let mut cmd = command(0, kind, "timer-a");
        cmd["taskId"] = json!("");
        let error = dispatch_json(
            "timer.reduce.v1",
            &json!({"commands": [cmd], "now": "2026-09-13T12:00:02Z"}).to_string(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("invalid shared-core input"),
            "{kind}: {error}"
        );
    }
    let mut start = command(0, "start", "timer-a");
    start["taskId"] = json!("task-x");
    dispatch_json(
        "timer.reduce.v1",
        &json!({"commands": [start], "now": "2026-09-13T12:00:02Z"}).to_string(),
    )
    .unwrap();
}

// C21: project() omits an empty startedByDeviceId, so restore() must accept a
// missing field; history-derived sessions round-trip through paged replay.
#[test]
fn history_page_round_trip_restores_omitted_device_attribution() {
    let full = call(
        "timer.reduce.v1",
        json!({"commands": [], "history": [{
            "id": "timer-a", "timerId": "timer-a", "phase": "focus",
            "status": "completed", "plannedDurationMs": 60000,
            "completedAt": "2026-09-13T12:00:02Z"}],
        "now": "2026-09-13T12:00:02Z"}),
    );
    assert!(full["sessions"][0].get("startedByDeviceId").is_none());
    let page = call(
        "timer.replay.page.v1",
        json!({"commands": [], "sessions": full["sessions"],
            "currentTimerId": null, "after": null, "now": "2026-09-13T12:00:02Z"}),
    );
    for field in ["sessions", "history", "canonicalTimer"] {
        assert_eq!(page[field], full[field], "{field}");
    }
}

// C22: Core-produced sessions without an intent carry lastCommandId ""; paged
// restore accepts the absence but still rejects a mismatched intent identity.
#[test]
fn paged_restore_accepts_core_session_without_intent() {
    let full = call(
        "timer.reduce.v1",
        json!({"commands": [],
        "canonicalTimer": {"id": "timer-a", "phase": "focus", "status": "running",
            "plannedDurationMs": 60000, "elapsedAtAnchorMs": 0,
            "anchorAt": "2026-09-13T12:00:01Z"},
        "history": [], "now": "2026-09-13T12:00:02Z"}),
    );
    assert_eq!(full["sessions"][0]["lastCommandId"], "");
    assert!(full["sessions"][0].get("lastIntent").is_none());
    let page = call(
        "timer.replay.page.v1",
        json!({"commands": [], "sessions": full["sessions"],
            "currentTimerId": "timer-a", "after": null, "now": "2026-09-13T12:00:02Z"}),
    );
    assert_eq!(page["canonicalTimer"]["id"], "timer-a");
    let mut terminal = full["sessions"][0].clone();
    terminal["status"] = json!("completed");
    let error = dispatch_json(
        "timer.replay.page.v1",
        &json!({"commands": [], "sessions": [terminal],
            "currentTimerId": "timer-a", "after": null})
        .to_string(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("invalid replay session metadata"), "{error}");
}

// C23: a generated-break batch carrying a retarget reconciles (reduce ignores a
// retarget off the active focus timer, so rebase must not hard-fail it).
#[test]
fn generated_break_batch_with_retarget_reconciles_like_reduce() {
    let mut finish = command(0, "finish", "focus-four");
    finish["phase"] = json!("focus");
    finish["plannedDurationMs"] = json!(1_500_000);
    let mut start = command(1, "start", "break-generated");
    start["phase"] = json!("short_break");
    let mut pause = command(2, "pause", "break-generated");
    pause["phase"] = json!("short_break");
    let mut retarget = command(3, "retarget", "break-generated");
    retarget["taskId"] = json!("task-next");
    let commands = vec![finish.clone(), start, pause, retarget];
    let history = json!([{"id": "history-four", "timerId": "focus-four",
        "commandId": "command-00000000", "phase": "focus", "status": "completed",
        "plannedDurationMs": 1_500_000, "completedAt": "2026-09-13T12:00:04Z"}]);
    let mut response = rebase_response(history);
    response["acknowledgements"] = json!([{"commandId": "command-00000000", "outcome": "ignored",
            "reason": "already completed"}]);
    response["serverTime"] = json!("2026-09-13T12:00:10Z");
    response["serverHlcWallMs"] = json!(1789300810000_i64);
    let input = json!({"local": rebase_queues(commands.clone()),
        "sent": rebase_queues(vec![commands[0].clone()]),
        "response": response,
        "timerDependencies": [
            {"operationId": "command-00000001",
             "dependsOnOperationId": "command-00000000", "generatedBreak": true,
             "sourceDayStart": "2026-09-13T00:00:00Z",
             "sourceDayEnd": "2026-09-14T00:00:00Z"},
            {"operationId": "command-00000002",
             "dependsOnOperationId": "command-00000001"},
            {"operationId": "command-00000003",
             "dependsOnOperationId": "command-00000001"}]});
    let error = dispatch_json("reconcile.rebase.v2", &input.to_string()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("rewrite a possibly delivered operation")
    );
    let mut never_sent = input.clone();
    never_sent["neverSent"] =
        json!({"commands": ["command-00000001", "command-00000002", "command-00000003"]});
    let safe = call("reconcile.rebase.v2", never_sent);
    assert_eq!(safe["droppedTimerOperationIds"], json!([]));
    let output: Value =
        serde_json::from_str(&dispatch_json("reconcile.rebase.v1", &input.to_string()).unwrap())
            .unwrap();
    assert_eq!(
        output["promotedTimerOperationIds"],
        json!(["command-00000001", "command-00000002", "command-00000003"])
    );
    assert_eq!(output["droppedTimerOperationIds"], json!([]));
    let reduced = call(
        "timer.reduce.v1",
        json!({"commands": commands, "now": "2026-09-13T12:00:10Z"}),
    );
    assert_eq!(
        reduced["outcomes"]["command-00000003"]["outcome"],
        "ignored"
    );
}
