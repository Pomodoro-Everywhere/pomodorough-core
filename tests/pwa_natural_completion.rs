use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/pwa-natural-completion-v1.json")).unwrap()
}

fn request(index: usize) -> Value {
    fixture()["receipts"][index]["request"]["input"].clone()
}

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn finish(input: &Value) -> Value {
    call("workspace.completionMutation.v1", input)
}

fn read(input: &Value) -> Value {
    call(
        "workspace.readModel.v1",
        &json!({"profile": "pwaStorage",
        "source": {"kind": "workspace", "value": input["workspace"]},
        "selectedPhase": input["selection"]["phase"], "selection": input["selection"],
        "lifecycle": input.get("lifecycle").cloned().unwrap_or(json!({})),
        "observedAt": input["clock"]["observedAt"], "calendarIntervals": input["calendarIntervals"]}),
    )
}

#[test]
fn four_raw_public_requests_admit_finish_without_recounting_or_canonical_rewrite() {
    let observations = fixture();
    for (index, receipt) in observations["receipts"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let input = request(index);
        assert_eq!(
            serde_json::from_str::<Value>(receipt["request"]["inputRaw"].as_str().unwrap())
                .unwrap(),
            input
        );
        assert_eq!(
            receipt["completeProductionReturn"],
            receipt["completeFrozenReturn"]
        );
        assert_eq!(receipt["completeProductionReturn"]["reason"], "staleTimer");
        let output = finish(&input);
        assert_eq!(output["outcome"], "planned");
        assert_eq!(output["commands"].as_array().unwrap().len(), 1);
        assert_eq!(output["commands"][0]["type"], "finish");
        assert_eq!(
            output["commands"][0]["phase"],
            input["requestedTimer"]["phase"]
        );
        assert_eq!(
            output["commands"][0]["plannedDurationMs"],
            input["requestedTimer"]["plannedDurationMs"]
        );
        assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
        assert_eq!(output["selection"]["phase"], "short_break");
        let history = &output["projection"]["history"][0];
        assert_eq!(
            history["id"],
            input["workspace"]["base"]["history"][0]["id"]
        );
        assert_eq!(history["commandId"], output["commands"][0]["id"]);
        assert_eq!(
            output["lifecycle"]["consumedCompletions"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let model = read(
            &json!({"workspace": output["workspace"], "selection": output["selection"],
            "clock": input["clock"], "calendarIntervals": input["calendarIntervals"], "lifecycle": output["lifecycle"]}),
        );
        assert_eq!(model["cadence"]["completedFocusTotal"], 1);
        assert_eq!(model["cadence"]["completedFocusToday"], 1);
    }
}

#[test]
fn natural_read_advances_display_and_keeps_finish_until_obligation_is_dispatched() {
    let input = request(0);
    let model = read(&input);
    assert_eq!(model["canonical"]["status"], "completed");
    assert_eq!(model["display"]["phase"], "short_break");
    assert!(
        model["availableIntents"]
            .as_array()
            .unwrap()
            .contains(&json!("finish"))
    );
    assert_eq!(model["cadence"]["completedFocusTotal"], 1);
}

#[test]
fn owner_lease_applies_to_natural_manual_and_automatic_finish() {
    for stage in ["finishCommit", "automaticFinishCommit"] {
        let mut input = request(0);
        input["stage"] = json!(stage);
        input["ownership"]["tabId"] = json!("peer");
        let now = input["leaseNowMs"].as_i64().unwrap();
        input["ownership"]["leaseExpiresAtMs"] = json!(now + 1);
        let blocked = finish(&input);
        assert_eq!(blocked["reason"], "not_owner");
        assert_eq!(blocked["retryAtMs"], now + 1);
        assert_eq!(blocked["allocation"], input["allocation"]);
        input["ownership"]["leaseExpiresAtMs"] = json!(now);
        assert_eq!(finish(&input)["outcome"], "planned");
        input["ownership"]["deviceId"] = json!("foreign");
        assert_eq!(finish(&input)["reason"], "not_owner");
        input["ownership"] = Value::Null;
        assert_eq!(finish(&input)["outcome"], "planned");
    }
}

#[test]
fn repeat_restart_generation_and_frozen_finish_do_not_allocate_twice() {
    let mut input = request(0);
    let output = finish(&input);
    input["workspace"] = output["workspace"].clone();
    input["allocation"] = output["allocation"].clone();
    input["observation"] = output["observation"].clone();
    input["lifecycle"] = output["lifecycle"].clone();
    input["selection"]["generation"] = json!("4");
    input["identities"]["commandUuids"][0] = input["identities"]["commandUuids"][1].clone();
    input["identities"]["commandUuids"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    input["workspace"]["neverSent"]["commands"] = json!([]);
    input["workspace"]["displayContext"]["projectionPending"]["commands"] = json!([]);
    let retry = finish(&serde_json::from_str::<Value>(&input.to_string()).unwrap());
    assert_eq!(retry["outcome"], "noop");
    assert_eq!(retry["allocation"], input["allocation"]);
    assert_eq!(retry["workspace"], input["workspace"]);
    assert_eq!(retry["commands"], json!([]));
}

#[test]
fn explicit_choice_and_consumed_presentation_preserve_selection_but_finish_bookkeeping() {
    let mut input = request(0);
    input["workspace"]["base"]["autoStartBreaks"] = json!(true);
    input["selection"] = json!({"phase": "long_break", "generation": "7", "explicit": true});
    let output = finish(&input);
    assert_eq!(output["selection"], input["selection"]);
    assert_eq!(output["commands"].as_array().unwrap().len(), 1);
    input["selection"]["explicit"] = json!(false);
    input["lifecycle"] = json!({"consumedCompletions": [{"timerId": input["requestedTimer"]["id"],
        "commandId": null, "phase": "focus"}], "pendingBreaks": [], "finishEvidence": []});
    let output = finish(&input);
    assert_eq!(output["commands"].as_array().unwrap().len(), 2);
    assert_eq!(output["selection"], input["selection"]);
    input["lifecycle"] = output["lifecycle"].clone();
    assert_eq!(finish(&input)["reason"], "alreadyConsumed");
}

#[test]
fn exact_terminal_evidence_is_required_and_explicit_finish_stays_stale() {
    for field in ["id", "phase", "plannedDurationMs", "anchorAt", "taskId"] {
        let mut input = request(0);
        input["requestedTimer"][field] = match field {
            "phase" => json!("long_break"),
            "plannedDurationMs" => json!(1800000),
            "anchorAt" => json!("2026-08-31T12:25:01Z"),
            _ => json!("wrong"),
        };
        assert_eq!(finish(&input)["reason"], "staleTimer");
    }
    let mut input = request(0);
    input["workspace"]["base"]["history"] = json!([]);
    assert_eq!(finish(&input)["reason"], "staleTimer");
    let output = finish(&request(0));
    input = request(0);
    input["workspace"]["base"]["canonicalTimer"] = output["projection"]["canonicalTimer"].clone();
    input["workspace"]["base"]["history"] = output["projection"]["history"].clone();
    input["requestedTimer"] = output["projection"]["canonicalTimer"].clone();
    assert_eq!(finish(&input)["reason"], "staleTimer");
}

fn install(input: &Value) -> Value {
    json!({"kind": "install", "compatibility": "pwaRejectedFinish", "beforeHistory": [],
        "afterHistory": input["workspace"]["base"]["history"], "canonicalTimer": input["requestedTimer"],
        "selection": input["selection"], "lifecycle": {}, "advances": [], "acknowledgements": [],
        "discardedCommandIds": [], "referenceTime": input["clock"]["observedAt"],
        "calendarIntervals": input["calendarIntervals"],
        "pending": {"commandIds": [], "sendableCommandIds": [], "otherOperationIds": []},
        "sentContext": {"kind": "pwa", "commands": [], "rollbackHistory": []}})
}

#[test]
fn installation_consumes_natural_or_remote_finish_once_without_overwriting_explicit_choice() {
    let mut input = install(&request(0));
    let first = call("timer.completionState.v1", &input);
    assert_eq!(first["selection"]["phase"], "short_break");
    input["selection"] = json!({"phase": "focus", "generation": "8", "explicit": true});
    input["lifecycle"] = first["lifecycle"].clone();
    input["canonicalTimer"]["lastIntent"] = json!({"type": "finish", "commandId": "remote-finish",
        "occurredAt": input["canonicalTimer"]["anchorAt"]});
    input["afterHistory"][0]["commandId"] = json!("remote-finish");
    let repeat = call("timer.completionState.v1", &input);
    assert_eq!(repeat["selection"], input["selection"]);
    assert_eq!(repeat["lifecycle"], input["lifecycle"]);
    input["lifecycle"] = json!({});
    let explicit = call("timer.completionState.v1", &input);
    assert_eq!(explicit["reason"], "explicitSelection");
    assert_eq!(explicit["selection"], input["selection"]);
    input["selection"]["explicit"] = json!(false);
    let remote = call("timer.completionState.v1", &input);
    assert_eq!(remote["selection"]["phase"], "short_break");
    assert_eq!(remote["source"]["commandId"], "remote-finish");
}

#[test]
fn resume_expiry_task_totals_and_fourth_focus_use_original_session() {
    let mut input = request(0);
    let task: Value = serde_json::from_str(include_str!(
        "../fixtures/workspace-intent-desktop-known-tasks-v1.json"
    ))
    .unwrap();
    let task = &task["knownTasks"][0];
    input["workspace"]["base"]["tasks"] = json!([task]);
    input["workspace"]["base"]["canonicalTimer"]["taskId"] = task["id"].clone();
    input["workspace"]["base"]["canonicalTimer"]["lastIntent"]["type"] = json!("resume");
    input["workspace"]["base"]["history"][0]["taskId"] = task["id"].clone();
    let row = input["workspace"]["base"]["history"][0].clone();
    for index in 0..3 {
        let mut prior = row.clone();
        prior["id"] = json!(format!("prior-{index}"));
        prior["timerId"] = prior["id"].clone();
        input["workspace"]["base"]["history"]
            .as_array_mut()
            .unwrap()
            .push(prior);
    }
    input["requestedTimer"] = input["workspace"]["base"]["canonicalTimer"].clone();
    input["workspace"]["base"]["autoStartBreaks"] = json!(true);
    let result = finish(&input);
    assert_eq!(result["commands"][1]["phase"], "long_break");
    assert_eq!(result["projection"]["history"].as_array().unwrap().len(), 4);
    input["workspace"] = result["workspace"].clone();
    input["selection"] = result["selection"].clone();
    let model = read(&input);
    assert_eq!(model["cadence"]["completedFocusTotal"], 4);
    assert_eq!(
        model["tasks"]["completedFocusTodayByTask"][task["id"].as_str().unwrap()]["count"],
        4
    );
    assert_eq!(
        model["tasks"]["completedFocusTodayByTask"][task["id"].as_str().unwrap()]["plannedDurationMs"],
        6000000
    );
}

#[test]
fn raw_go_http_finish_replaces_natural_provenance_without_second_completion() {
    let http = fixture()["http"].clone();
    for name in ["naturalResponse", "finishResponse"] {
        let raw = http[format!("{name}Raw")].as_str().unwrap();
        assert_eq!(serde_json::from_str::<Value>(raw).unwrap(), http[name]);
    }
    assert_eq!(
        http["finishResponse"]["acknowledgements"][0]["outcome"],
        "applied"
    );
    assert_eq!(
        http["finishResponse"]["history"][0]["id"],
        http["naturalResponse"]["history"][0]["id"]
    );
    assert_eq!(
        http["finishResponse"]["history"].as_array().unwrap().len(),
        1
    );
    let mut input = request(0);
    for key in [
        "canonicalTimer",
        "history",
        "tasks",
        "durationsMs",
        "autoStartBreaks",
        "selectedTaskId",
    ] {
        input["workspace"]["base"][key] = http["naturalResponse"][key].clone();
    }
    input["requestedTimer"] = http["naturalResponse"]["canonicalTimer"].clone();
    input["allocation"]["deviceId"] = http["startRequest"]["deviceId"].clone();
    input["ownership"] = Value::Null;
    input["observation"]["canonicalAnchorAt"] = input["requestedTimer"]["anchorAt"].clone();
    input["calendarIntervals"] =
        json!([{"start": "2026-10-03T00:00:00Z", "end": "2026-10-04T00:00:00Z"}]);
    input["clock"] = json!({"occurredAt": "2026-10-03T17:23:50Z", "physicalNow": "2026-10-03T17:23:50Z", "observedAt": "2026-10-03T17:23:50Z"});
    input["identities"]["commandUuids"] = json!(["01a28cce-27f0-7000-8000-000000000001"]);
    let error = dispatch_json("workspace.completionMutation.v1", &input.to_string());
    assert!(
        error.is_err(),
        "wrong UUID timestamp must fail before planning"
    );
    input["identities"]["commandUuids"] = json!([format!(
        "{:08x}-{:04x}-7000-8000-000000000001",
        1791048230000_i64 >> 16,
        1791048230000_i64 & 0xffff
    )]);
    let result = finish(&input);
    assert_eq!(result["workspace"]["base"], input["workspace"]["base"]);
    assert_eq!(
        result["projection"]["history"][0]["id"],
        http["naturalResponse"]["history"][0]["id"]
    );
    assert_eq!(result["projection"]["history"].as_array().unwrap().len(), 1);
}

#[test]
fn finish_ack_outcomes_cannot_rollback_exact_natural_completion() {
    let source = request(0);
    let mut input = install(&source);
    input["selection"]["phase"] = json!("short_break");
    input["beforeHistory"] = input["afterHistory"].clone();
    input["sentContext"]["rollbackHistory"] = input["afterHistory"].clone();
    input["sentContext"]["commands"] = json!([{"id": "pending-finish", "timerId": source["requestedTimer"]["id"],
        "type": "finish", "phase": "focus", "deviceSequence": 9, "occurredAt": source["clock"]["occurredAt"]}]);
    for outcome in ["applied", "ignored", "rejected"] {
        input["acknowledgements"] = json!([{"commandId": "pending-finish", "outcome": outcome}]);
        let result = call("timer.completionState.v1", &input);
        assert_eq!(result["selection"], input["selection"]);
        assert_eq!(input["afterHistory"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn frozen_replacement_timer_prevents_natural_finish_while_display_still_shows_source() {
    let mut input = request(0);
    input["workspace"]["local"]["commands"] = json!([{"id": "claimed-replacement", "deviceId": "p222-device",
        "deviceSequence": 9, "timerId": "replacement", "type": "start", "phase": "focus",
        "plannedDurationMs": 1500000, "occurredAt": input["clock"]["occurredAt"],
        "hlcWallMs": input["leaseNowMs"], "hlcCounter": 0, "observedElapsedMs": 0}]);
    input["allocation"]["deviceSequence"] = json!(9);
    input["allocation"]["hlc"] = json!({"wallMs": input["leaseNowMs"], "counter": 0});
    let result = finish(&input);
    assert_eq!(result["reason"], "staleTimer");
    assert_eq!(result["commands"], json!([]));
    assert_eq!(result["workspace"], input["workspace"]);
    assert_eq!(result["allocation"], input["allocation"]);
}
