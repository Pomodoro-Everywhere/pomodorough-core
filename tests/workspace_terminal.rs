use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-terminal-v1.json")).unwrap()
}

fn replace_fields(value: &mut Value, overrides: &Value) {
    if let Some(fields) = overrides.as_object() {
        for (name, field) in fields {
            value[name] = field.clone();
        }
    }
}

fn request(case: &Value) -> Value {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    let mut timer = fixture["timer"].clone();
    let mut history = fixture["history"].clone();
    replace_fields(&mut timer, &case["timerOverrides"]);
    replace_fields(&mut history, &case["historyOverrides"]);
    request["base"]["canonicalTimer"] = if case["cleared"] == true {
        Value::Null
    } else {
        timer
    };
    request["base"]["history"] = if case["missingHistory"] == true {
        json!([])
    } else {
        json!([history])
    };
    if case["sameTimeSibling"] == true {
        let mut sibling = fixture["history"].clone();
        sibling["id"] = json!("aaa-sibling");
        sibling["timerId"] = json!("aaa-sibling");
        sibling["commandId"] = json!("finish-sibling");
        request["base"]["history"]
            .as_array_mut()
            .unwrap()
            .push(sibling);
    }
    if case["commands"].is_array() {
        request["local"]["commands"] = json!([fixture["command"]]);
        if case["safe"] == true {
            request["neverSent"]["commands"] = json!([fixture["command"]["id"]]);
        }
    }
    if case["retainedCommands"].is_array() {
        request["local"]["commands"] = case["retainedCommands"].clone();
    }
    request
}

fn call(operation: &str, input: &Value) -> Result<Value, String> {
    dispatch_json(operation, &input.to_string())
        .map(|output| serde_json::from_str(&output).unwrap())
        .map_err(|error| error.to_string())
}

fn project(input: &Value) -> Value {
    call("workspace.project.v1", input).unwrap_or_else(|error| panic!("{error}: {input}"))
}

fn serialized_options(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.retain(|_, field| !field.is_null());
    }
    if let Some(array) = value.as_array_mut() {
        for field in array {
            *field = serialized_options(field.clone());
        }
    }
    value
}

#[test]
fn shared_raw_terminal_fixture_accepts_exact_pairs_and_preserves_display() {
    for case in fixture()["cases"].as_array().unwrap() {
        let input = request(case);
        let result = project(&input);
        let workspace = &result["workspace"];
        let expected = if case["expectedTimer"] == "null" {
            Value::Null
        } else {
            serialized_options(input["base"]["canonicalTimer"].clone())
        };
        assert_eq!(workspace["canonicalTimer"], expected, "{}", case["name"]);
        let expected_history = if case["missingHistory"] == true {
            let mut row = fixture()["history"].clone();
            replace_fields(&mut row, &case["expectedHistoryOverrides"]);
            json!([row])
        } else {
            input["base"]["history"].clone()
        };
        let expected_history = serialized_options(expected_history);
        let mut expected_rows = expected_history.as_array().unwrap().clone();
        expected_rows.sort_by_key(|row| row["timerId"].as_str().unwrap().to_owned());
        assert_eq!(
            workspace["history"],
            json!(expected_rows),
            "{}",
            case["name"]
        );
        let mut reopened = input.clone();
        reopened["base"]["canonicalTimer"] = workspace["canonicalTimer"].clone();
        reopened["base"]["history"] = workspace["history"].clone();
        reopened["local"]["commands"] = json!([]);
        reopened["neverSent"] = json!({});
        assert_eq!(project(&reopened)["workspace"]["canonicalTimer"], expected);
        assert_eq!(
            project(&reopened)["workspace"]["history"],
            workspace["history"]
        );
    }
}

#[test]
fn shared_conflicting_terminal_pairs_fail_closed() {
    for case in fixture()["rejections"].as_array().unwrap() {
        let error = call("workspace.project.v1", &request(case)).unwrap_err();
        assert_eq!(
            error,
            fixture()["conflictError"].as_str().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn public_reducers_keep_strict_overlap_contract() {
    let input = request(&json!({}));
    for operation in ["timer.reduce.v1", "projection.apply.v2"] {
        let raw = if operation == "timer.reduce.v1" {
            json!({"canonicalTimer": input["base"]["canonicalTimer"], "history": input["base"]["history"], "commands": [], "now": input["now"]})
        } else {
            json!({"base": input["base"], "pending": input["local"], "now": input["now"]})
        };
        assert!(
            call(operation, &raw)
                .unwrap_err()
                .contains("canonical timer overlaps timer history")
        );
    }
}

fn workspace_source(input: &Value) -> Value {
    let mut source = input.clone();
    source.as_object_mut().unwrap().remove("now");
    source
}

fn read_request(input: &Value, profile: &str) -> Value {
    json!({"profile": profile, "source": {"kind": "workspace", "value": workspace_source(input)},
        "selectedPhase": "focus", "observedAt": input["now"],
        "calendarIntervals": [{"start": "2026-07-20T00:00:00Z", "end": "2026-07-21T00:00:00Z"}]})
}

fn intent_request(input: &Value, profile: &str, action: &str) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let mut intent = fixture["request"].clone();
    intent["workspace"] = workspace_source(input);
    intent["compatibility"] = json!(profile);
    intent["intent"] = json!({"kind": action});
    if action == "restart" {
        intent["requestedTimer"] = input["base"]["canonicalTimer"].clone();
    }
    intent
}

#[test]
fn read_model_and_intent_use_same_raw_pair_in_every_profile() {
    for case in fixture()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["commands"].is_null())
    {
        let input = request(case);
        let expected = project(&input)["workspace"].clone();
        for profile in [
            "appleWorkspace",
            "desktopStorage",
            "desktopTerminal",
            "androidCoordinator",
            "pwaStorage",
        ] {
            let read = call("workspace.readModel.v1", &read_request(&input, profile)).unwrap();
            assert_eq!(
                read["canonical"]["timerId"],
                expected["canonicalTimer"]
                    .get("id")
                    .cloned()
                    .unwrap_or(Value::Null)
            );
            assert_eq!(
                read["canonical"]["status"],
                expected["canonicalTimer"]
                    .get("status")
                    .cloned()
                    .unwrap_or(json!("idle"))
            );
            let action =
                if expected["canonicalTimer"].is_null() || case["retainedCommands"].is_array() {
                    "pause"
                } else if profile.starts_with("desktop") {
                    "restart"
                } else {
                    "clear"
                };
            let intent = call(
                "workspace.intent.v1",
                &intent_request(&input, profile, action),
            )
            .unwrap();
            if profile.starts_with("desktop") && action == "restart" {
                assert_eq!(intent["commands"][0]["type"], "clear");
                assert_eq!(intent["commands"][1]["type"], "start");
                assert_eq!(intent["projection"]["canonicalTimer"]["status"], "running");
            } else if !intent["commands"].as_array().unwrap().is_empty() {
                assert!(intent["projection"]["canonicalTimer"].is_null());
            }
            assert_eq!(intent["projection"]["history"], expected["history"]);
            assert_eq!(intent["workspace"]["base"], input["base"]);
        }
    }
}

#[test]
fn conflicts_fail_across_composed_boundaries_without_clearing_input() {
    for case in fixture()["rejections"].as_array().unwrap() {
        let input = request(case);
        for profile in ["appleWorkspace", "desktopStorage", "androidCoordinator"] {
            assert!(
                call("workspace.readModel.v1", &read_request(&input, profile)).is_err(),
                "{case}"
            );
            assert!(
                call(
                    "workspace.intent.v1",
                    &intent_request(&input, profile, "clear")
                )
                .is_err(),
                "{case}"
            );
        }
    }
}

#[test]
fn terminal_pair_does_not_weaken_c02_or_history_identity_validation() {
    for collision in [
        "duplicate-id",
        "duplicate-timer",
        "canonical-alias",
        "history-alias",
    ] {
        let mut input = request(&json!({}));
        let mut sibling = fixture()["history"].clone();
        sibling["id"] = json!("other-history");
        sibling["timerId"] = json!("other-timer");
        match collision {
            "duplicate-id" | "canonical-alias" => {
                sibling["id"] = fixture()["history"]["id"].clone()
            }
            "duplicate-timer" => sibling["timerId"] = fixture()["timer"]["id"].clone(),
            _ => input["base"]["history"][0]["id"] = json!("other-timer"),
        }
        input["base"]["history"]
            .as_array_mut()
            .unwrap()
            .push(sibling);
        assert!(call("workspace.project.v1", &input).is_err(), "{collision}");
    }
    let mut input = request(&json!({"historyOverrides": {"id": "reserved-history"}}));
    let mut start = fixture()["command"].clone();
    start["type"] = json!("start");
    start["timerId"] = json!("reserved-history");
    input["local"]["commands"] = json!([start]);
    input["neverSent"]["commands"] = json!([fixture()["command"]["id"]]);
    let output = project(&input);
    assert_eq!(
        output["workspace"]["timerOutcomes"]["clear-existing"]["outcome"],
        "rejected"
    );
    assert_eq!(
        output["workspace"]["canonicalTimer"],
        input["base"]["canonicalTimer"]
    );
    assert_eq!(output["workspace"]["history"], input["base"]["history"]);
}

#[test]
fn retained_terminal_command_conflicts_fail_even_when_domain_is_suppressed() {
    for (field, value) in [
        ("timerId", json!("other-timer")),
        ("type", json!("cancel")),
        ("occurredAt", json!("2026-07-20T12:00:01Z")),
    ] {
        let mut input = request(&json!({}));
        let mut finish = fixture()["command"].clone();
        finish["id"] = json!("finish-existing");
        finish["type"] = json!("finish");
        finish["occurredAt"] = fixture()["timer"]["anchorAt"].clone();
        finish[field] = value;
        input["local"]["commands"] = json!([finish]);
        assert!(
            call("workspace.project.v1", &input)
                .unwrap_err()
                .contains("conflicting workspace terminal"),
            "{field}"
        );
    }
}

#[test]
fn legacy_history_identity_and_terminal_elapsed_survive_resume_then_finish() {
    let mut input = request(&fixture()["cases"][4]);
    input["base"]["history"][0]["id"] = json!("legacy-history");
    let mut resume = fixture()["command"].clone();
    resume["type"] = json!("resume");
    resume["observedElapsedMs"] = json!(17000);
    input["local"]["commands"] = json!([resume]);
    input["neverSent"]["commands"] = json!([fixture()["command"]["id"]]);
    input["now"] = json!("2026-07-20T12:00:11Z");
    let resumed = project(&input);
    assert_eq!(
        resumed["workspace"]["canonicalTimer"]["elapsedAtAnchorMs"],
        17000
    );
    assert_eq!(
        resumed["workspace"]["canonicalTimer"]["startedByDeviceId"],
        "device-local"
    );
    assert_eq!(resumed["workspace"]["history"], json!([]));
    let mut finish = fixture()["command"].clone();
    finish["type"] = json!("finish");
    finish["id"] = json!("refinish");
    finish["deviceSequence"] = json!(9);
    finish["hlcCounter"] = json!(1);
    input["local"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(finish);
    input["neverSent"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("refinish"));
    let output = project(&input);
    assert_eq!(output["workspace"]["history"][0]["id"], "legacy-history");
    assert_eq!(output["workspace"]["history"][0]["commandId"], "refinish");
}

#[test]
fn equivalent_offset_timestamps_are_exact_instants() {
    let mut input = request(&json!({}));
    input["base"]["canonicalTimer"]["anchorAt"] = json!("2026-07-20T14:00:00+02:00");
    input["base"]["canonicalTimer"]["lastIntent"]["occurredAt"] = json!("2026-07-20T12:00:00.000Z");
    let mut expected = fixture()["timer"].clone();
    expected["lastIntent"]["occurredAt"] = json!("2026-07-20T12:00:00.000Z");
    assert_eq!(project(&input)["workspace"]["canonicalTimer"], expected);
}

#[test]
fn prior_intent_command_and_cancel_elapsed_are_checked_when_retained() {
    for (field, value) in [
        ("type", json!("pause")),
        ("timerId", json!("other-timer")),
        ("phase", json!("short_break")),
        ("plannedDurationMs", json!(120000)),
        ("occurredAt", json!("2026-07-20T11:59:01Z")),
    ] {
        let mut input = request(&fixture()["cases"][2]);
        let mut start = fixture()["command"].clone();
        start["id"] = json!("start-existing");
        start["type"] = json!("start");
        start["occurredAt"] = json!("2026-07-20T11:59:00Z");
        start[field] = value;
        input["local"]["commands"] = json!([start]);
        assert_eq!(
            call("workspace.project.v1", &input).unwrap_err(),
            fixture()["conflictError"]
        );
    }
    let mut input = request(&fixture()["cases"][4]);
    let mut cancel = fixture()["command"].clone();
    cancel["id"] = json!("cancel-existing");
    cancel["type"] = json!("cancel");
    cancel["occurredAt"] = fixture()["timer"]["anchorAt"].clone();
    cancel["observedElapsedMs"] = json!(17001);
    input["local"]["commands"] = json!([cancel]);
    assert_eq!(
        call("workspace.project.v1", &input).unwrap_err(),
        fixture()["conflictError"]
    );
}

#[test]
fn actual_reducer_terminal_output_is_supported_persisted_workspace_input() {
    for kind in ["finish", "cancel", "deadline"] {
        let mut start = fixture()["command"].clone();
        start["id"] = json!("start-existing");
        start["type"] = json!("start");
        start["occurredAt"] = json!("2026-07-20T11:59:00Z");
        start["observedElapsedMs"] = json!(0);
        let mut commands = vec![start];
        if kind != "deadline" {
            let mut terminal = fixture()["command"].clone();
            terminal["type"] = json!(kind);
            terminal["deviceSequence"] = json!(9);
            terminal["hlcCounter"] = json!(1);
            terminal["occurredAt"] = json!("2026-07-20T11:59:17Z");
            terminal["observedElapsedMs"] = json!(17000);
            commands.push(terminal);
        }
        let reduced = call(
            "timer.reduce.v1",
            &json!({"commands": commands, "now": "2026-07-20T12:00:00Z"}),
        )
        .unwrap();
        let mut input = fixture()["request"].clone();
        input["base"]["canonicalTimer"] = reduced["canonicalTimer"].clone();
        input["base"]["history"] = reduced["history"].clone();
        let projected = project(&input);
        assert_eq!(
            projected["workspace"]["canonicalTimer"],
            reduced["canonicalTimer"]
        );
        assert_eq!(projected["workspace"]["history"], reduced["history"]);
    }
}

#[test]
fn superseded_session_without_prior_intent_uses_real_reducer_provenance() {
    let mut input = fixture()["request"].clone();
    input["base"]["canonicalTimer"] = json!({"id": "existing-timer", "phase": "focus", "status": "running",
        "plannedDurationMs": 60000, "elapsedAtAnchorMs": 0, "anchorAt": "2026-07-20T12:00:00Z"});
    let mut replacement = fixture()["command"].clone();
    replacement["type"] = json!("start");
    replacement["timerId"] = json!("replacement");
    let reduced = call(
        "timer.reduce.v1",
        &json!({"canonicalTimer": input["base"]["canonicalTimer"],
        "commands": [replacement], "now": "2026-07-20T12:00:11Z"}),
    )
    .unwrap();
    let restored = call(
        "timer.replay.page.v1",
        &json!({"sessions": reduced["sessions"], "currentTimerId": "existing-timer",
        "commands": [], "after": null, "now": "2026-07-20T12:00:11Z"}),
    )
    .unwrap();
    input["base"]["canonicalTimer"] = restored["canonicalTimer"].clone();
    input["base"]["history"] = restored["history"].clone();
    let projected = project(&input);
    assert_eq!(
        projected["workspace"]["canonicalTimer"],
        restored["canonicalTimer"]
    );
    assert_eq!(projected["workspace"]["history"], restored["history"]);
}
