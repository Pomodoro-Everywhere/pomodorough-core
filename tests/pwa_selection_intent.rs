use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn natural() -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/pwa-natural-completion-v1.json")).unwrap();
    fixture["receipts"][0]["request"]["input"].clone()
}

fn intent(phase: &str) -> Value {
    let mut input = natural();
    for key in [
        "stage",
        "ownership",
        "localTabId",
        "leaseNowMs",
        "leaseDurationMs",
    ] {
        input.as_object_mut().unwrap().remove(key);
    }
    input["intent"] = json!({"kind": "selectPhase", "phase": phase});
    input["lifecycle"] = json!({"consumedCompletions": [], "pendingBreaks": []});
    input
}

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn plan(input: &Value) -> Value {
    call("workspace.intent.v1", input)
}

fn read(input: &Value, at: &str) -> Value {
    call(
        "workspace.readModel.v1",
        &json!({"profile": "pwaStorage",
        "source": {"kind": "workspace", "value": input["workspace"]},
        "selection": input["selection"], "selectedPhase": input["selection"]["phase"],
        "lifecycle": input["lifecycle"], "observedAt": at, "calendarIntervals": input["calendarIntervals"]}),
    )
}

fn assert_choice(input: &Value, output: &Value, generation: &str, phase: &str) {
    assert_eq!(output["outcome"], "planned");
    assert_eq!(
        output["selection"],
        json!({"phase": phase, "generation": generation, "explicit": true})
    );
    for key in ["workspace", "allocation"] {
        assert_eq!(output[key], input[key]);
    }
    for key in [
        "commands",
        "atomicCommandIds",
        "ownershipWrites",
        "effectsAfterCommit",
    ] {
        assert_eq!(output[key], json!([]));
    }
    assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
}

#[test]
fn same_and_different_phase_choices_are_durable_even_without_unused_identities() {
    for phase in ["focus", "short_break", "long_break"] {
        let mut input = intent(phase);
        input["identities"] = json!({"commandUuids": [], "timerUuid": null});
        let output = plan(&input);
        assert_choice(&input, &output, "1", phase);
        assert_eq!(output["observation"], input["observation"]);
        assert_eq!(
            output["lifecycle"]["consumedCompletions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        input["selection"] = output["selection"].clone();
        input["lifecycle"] = output["lifecycle"].clone();
        let restart: Value = serde_json::from_str(&input.to_string()).unwrap();
        assert_eq!(
            read(&restart, input["clock"]["observedAt"].as_str().unwrap())["display"]["phase"],
            phase
        );
        assert_choice(&input, &plan(&input), "2", phase);
    }
}

fn running(input: &mut Value, status: &str) {
    input["workspace"]["base"]["history"] = json!([]);
    let timer = &mut input["workspace"]["base"]["canonicalTimer"];
    timer["status"] = json!(status);
    timer["anchorAt"] = json!("2026-08-31T12:00:00Z");
    timer["elapsedAtAnchorMs"] = json!(0);
    let requested = timer.clone();
    input["observation"]["canonicalAnchorAt"] = requested["anchorAt"].clone();
    input["requestedTimer"] = requested;
    input["clock"] = json!({"occurredAt": "2026-08-31T12:01:00Z", "physicalNow": "2026-08-31T12:01:00Z", "observedAt": "2026-08-31T12:01:00Z"});
}

#[test]
fn user_choice_before_and_after_expiry_protects_same_phase_without_clearing_timer() {
    for after in [false, true] {
        let mut input = intent("focus");
        if !after {
            running(&mut input, "running");
        }
        let output = plan(&input);
        assert_choice(&input, &output, "1", "focus");
        input["selection"] = output["selection"].clone();
        input["lifecycle"] = output["lifecycle"].clone();
        let model = read(&input, "2026-08-31T12:25:00Z");
        assert_eq!(model["display"]["phase"], "focus");
        assert_eq!(model["canonical"]["status"], "completed");
        assert_eq!(model["cadence"]["completedFocusTotal"], 1);
    }
}

#[test]
fn repeated_skip_uses_core_cadence_and_only_advances_choice_generation() {
    for count in [0, 3] {
        let mut input = intent("focus");
        input["workspace"]["base"]["canonicalTimer"] = Value::Null;
        input["requestedTimer"] = Value::Null;
        input["observation"]["canonicalAnchorAt"] = Value::Null;
        let row = input["workspace"]["base"]["history"][0].clone();
        input["workspace"]["base"]["history"] = json!(
            (0..count)
                .map(|n| {
                    let mut row = row.clone();
                    row["id"] = json!(format!("prior-{n}"));
                    row["timerId"] = row["id"].clone();
                    row
                })
                .collect::<Vec<_>>()
        );
        input["intent"] = json!({"kind": "skip"});
        for (index, phase) in [
            if count == 3 {
                "long_break"
            } else {
                "short_break"
            },
            "focus",
            if count == 3 {
                "long_break"
            } else {
                "short_break"
            },
        ]
        .iter()
        .enumerate()
        {
            let output = plan(&input);
            assert_choice(&input, &output, &(index + 1).to_string(), phase);
            input["selection"] = output["selection"].clone();
            input["lifecycle"] = output["lifecycle"].clone();
        }
    }
}

#[test]
fn choice_generation_is_exact_checked_and_unrelated_noops_do_not_allocate() {
    for (generation, next) in [
        ("0", "1"),
        ("9007199254740991", "9007199254740992"),
        ("9223372036854775806", "9223372036854775807"),
    ] {
        let mut input = intent("focus");
        input["selection"]["generation"] = json!(generation);
        assert_choice(&input, &plan(&input), next, "focus");
    }
    let mut input = intent("focus");
    input["selection"]["generation"] = json!("9223372036854775807");
    assert!(
        dispatch_json("workspace.intent.v1", &input.to_string())
            .unwrap_err()
            .to_string()
            .contains("selection generation exhausted")
    );
    input["intent"] = json!({"kind": "restart"});
    let output = plan(&input);
    assert_eq!(output["outcome"], "noop");
    for key in ["workspace", "allocation", "selection", "lifecycle"] {
        assert_eq!(output[key], input[key]);
    }
    assert_eq!(output["commands"], json!([]));
    assert_eq!(output["effectsAfterCommit"], json!([]));
}

#[test]
fn raw_invalid_choices_states_and_generations_fail_without_partial_output() {
    for value in [
        json!(""),
        json!("invalid"),
        json!({"focus": null}),
        json!(["focus"]),
        Value::Null,
    ] {
        let mut input = intent("focus");
        input["intent"]["phase"] = value;
        assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    }
    for generation in ["-1", "01", "+1", "", "9223372036854775808"] {
        let mut input = intent("focus");
        input["selection"]["generation"] = json!(generation);
        assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    }
    for status in ["idle", "invalid"] {
        let mut input = intent("focus");
        input["workspace"]["base"]["canonicalTimer"]["status"] = json!(status);
        assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    }
    for field in ["plannedDurationMs", "elapsedAtAnchorMs"] {
        let mut input = intent("focus");
        input["workspace"]["base"]["canonicalTimer"][field] = json!(-1);
        assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    }
}

#[test]
fn opting_out_keeps_legacy_same_phase_noop_and_implicit_different_phase() {
    for phase in ["focus", "long_break"] {
        let mut input = intent(phase);
        input.as_object_mut().unwrap().remove("lifecycle");
        let output = plan(&input);
        assert_eq!(
            output["outcome"],
            if phase == "focus" { "noop" } else { "planned" }
        );
        assert_eq!(
            output["selection"],
            json!({"phase": phase, "generation": "0", "explicit": false})
        );
        assert!(output.get("lifecycle").is_none());
    }
}

#[test]
fn lifecycle_context_keeps_terminal_clear_command_and_default_wire_contract() {
    let mut input = intent("focus");
    input["intent"] = json!({"kind": "clear"});
    let mut output = plan(&input);
    assert_eq!(output["commands"].as_array().unwrap().len(), 1);
    assert_eq!(output["commands"][0]["type"], "clear");
    assert_eq!(
        output["allocation"]["deviceSequence"],
        input["allocation"]["deviceSequence"].as_i64().unwrap() + 1
    );
    assert_eq!(output["lifecycle"], input["lifecycle"]);
    output.as_object_mut().unwrap().remove("lifecycle");
    input.as_object_mut().unwrap().remove("lifecycle");
    assert_eq!(output, plan(&input));
}

#[test]
fn exact_registered_public_requests_match_complete_shared_source_returns() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/pwa-selection-public-v1.json")).unwrap();
    for receipt in fixture["receipts"].as_array().unwrap() {
        for field in [
            "selected",
            "legacyCompletePlanner",
            "currentCall",
            "frozenCall",
        ] {
            let call = &receipt[field];
            assert_eq!(
                serde_json::from_str::<Value>(call["inputRaw"].as_str().unwrap()).unwrap(),
                call["input"]
            );
            let output =
                dispatch_json("workspace.intent.v1", call["inputRaw"].as_str().unwrap()).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&output).unwrap(),
                call["completeReturn"]
            );
        }
        assert_eq!(
            receipt["completeCurrentReturn"],
            receipt["completeFrozenReturn"]
        );
        assert_eq!(
            receipt["currentCall"]["inputRaw"],
            receipt["frozenCall"]["inputRaw"]
        );
    }
}

#[test]
fn late_ack_after_clear_or_replacement_preserves_core_owned_same_phase_choice() {
    let mut input = intent("short_break");
    input["lifecycle"]["finishEvidence"] = json!([]);
    let chosen = plan(&input);
    let mut finish = natural();
    finish["selection"] = chosen["selection"].clone();
    finish["lifecycle"] = chosen["lifecycle"].clone();
    let finished = call("workspace.completionMutation.v1", &finish);
    let mut repeat = input.clone();
    repeat["selection"] = finished["selection"].clone();
    repeat["lifecycle"] = finished["lifecycle"].clone();
    let chosen_again = plan(&repeat);
    for outcome in ["applied", "ignored", "rejected"] {
        let install = json!({"kind": "install", "compatibility": "pwaRejectedFinish", "beforeHistory": [],
            "afterHistory": finished["projection"]["history"], "canonicalTimer": null,
            "selection": chosen_again["selection"], "lifecycle": chosen_again["lifecycle"],
            "pending": {"commandIds": [], "sendableCommandIds": [], "otherOperationIds": []},
            "advances": [], "acknowledgements": [{"commandId": finished["commands"][0]["id"], "outcome": outcome}],
            "discardedCommandIds": [], "referenceTime": input["clock"]["observedAt"], "calendarIntervals": input["calendarIntervals"],
            "sentContext": {"kind": "pwa", "commands": finished["commands"], "rollbackHistory": finished["projection"]["history"]}});
        let result = call("timer.completionState.v1", &install);
        assert_eq!(result["selection"], chosen_again["selection"]);
        assert_eq!(result["lifecycle"], chosen_again["lifecycle"]);
    }
}
