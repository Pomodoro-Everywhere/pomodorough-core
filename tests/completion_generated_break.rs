use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../fixtures/completion-generated-break-v1.json"
    ))
    .unwrap()
}

fn request(profile: &str, automatic: bool) -> Value {
    let base: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let cases = fixture();
    let mut input = base["request"].clone();
    input.as_object_mut().unwrap().remove("intent");
    input["stage"] = json!(if automatic {
        "automaticFinishCommit"
    } else {
        "finishCommit"
    });
    input["compatibility"] = json!(profile);
    input["workspace"]["base"]["autoStartBreaks"] = json!(true);
    input["workspace"]["base"]["canonicalTimer"] = base["timer"].clone();
    input["requestedTimer"] = base["timer"].clone();
    input["selection"]["explicit"] = json!(false);
    input["identities"]["commandUuids"] = if automatic {
        cases["commandUuids"].clone()
    } else {
        json!([
            "019f7f65-dd10-7000-8000-000000000001",
            "019f7f65-dd10-7000-8000-000000000002"
        ])
    };
    input["identities"]["timerUuid"] = cases["timerUuid"].clone();
    input["ownership"] = json!({"timerId": "existing-timer", "deviceId": "device-local"});
    if automatic {
        for key in ["occurredAt", "physicalNow", "observedAt"] {
            input["clock"][key] = json!("2026-07-20T12:01:00Z");
        }
    }
    if profile == "pwaStorage" {
        input["localTabId"] = json!("tab-local");
        input["leaseNowMs"] = json!(1784548860000_i64);
        input["leaseDurationMs"] = cases["leaseDurationMs"].clone();
        input["ownership"]["tabId"] = json!("tab-local");
        input["ownership"]["leaseExpiresAtMs"] = json!(1784548870000_i64);
    }
    input
}

fn call(input: &Value) -> Result<Value, String> {
    pomodorough_core::dispatch_json("workspace.completionMutation.v1", &input.to_string())
        .map(|output| serde_json::from_str(&output).unwrap())
        .map_err(|error| error.to_string())
}

fn prior_focuses(input: &mut Value, count: usize) {
    for index in 0..count {
        input["workspace"]["base"]["history"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": format!("past-{index}"), "timerId": format!("past-timer-{index}"),
                "commandId": format!("past-finish-{index}"), "phase": "focus",
                "status": "completed", "plannedDurationMs": 60000,
                "completedAt": "2026-07-20T11:00:00Z"
            }));
    }
}

fn assert_batch(input: &Value, phase: &Value) -> Value {
    let result = call(input)
        .unwrap_or_else(|error| panic!("{} {}: {error}", input["compatibility"], input["stage"]));
    assert_eq!(result["outcome"], "planned");
    let commands = result["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0]["type"], "finish");
    assert_eq!(commands[1]["type"], "start");
    assert_eq!(commands[1]["phase"], *phase);
    assert_eq!(
        commands[1]["plannedDurationMs"],
        result["projection"]["durationsMs"][phase.as_str().unwrap()]
    );
    assert_eq!(commands[1]["observedElapsedMs"], 0);
    assert_eq!(commands[1]["taskId"], Value::Null);
    assert_eq!(
        commands[1]["deviceSequence"].as_i64(),
        commands[0]["deviceSequence"].as_i64().map(|n| n + 1)
    );
    assert_eq!(commands[1]["hlcWallMs"], commands[0]["hlcWallMs"]);
    assert_eq!(
        commands[1]["hlcCounter"].as_i64(),
        commands[0]["hlcCounter"].as_i64().map(|n| n + 1)
    );
    assert_eq!(
        result["allocation"]["lastUuid"],
        input["identities"]["commandUuids"][1]
    );
    assert_eq!(
        result["atomicCommandIds"],
        json!([commands[0]["id"], commands[1]["id"]])
    );
    assert_eq!(
        result["workspace"]["neverSent"]["commands"],
        result["atomicCommandIds"]
    );
    assert_eq!(
        result["workspace"]["timerDependencies"][0],
        json!({
            "operationId": commands[1]["id"], "dependsOnOperationId": commands[0]["id"],
            "generatedBreak": true, "sourceDayStart": "2026-07-20T00:00:00Z",
            "sourceDayEnd": "2026-07-21T00:00:00Z"
        })
    );
    assert_eq!(
        result["projection"]["canonicalTimer"]["id"],
        commands[1]["timerId"]
    );
    assert_eq!(
        result["projection"]["timerOutcomes"][commands[0]["id"].as_str().unwrap()]["outcome"],
        "applied"
    );
    assert_eq!(
        result["projection"]["timerOutcomes"][commands[1]["id"].as_str().unwrap()]["outcome"],
        "applied"
    );
    result
}

#[test]
fn generated_break_counts_and_profiles_keep_atomic_provenance() {
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        for automatic in [false, true] {
            for case in fixture()["focusCounts"].as_array().unwrap() {
                let mut input = request(profile, automatic);
                prior_focuses(&mut input, case["prior"].as_u64().unwrap() as usize);
                let result = assert_batch(&input, &case["next"]);
                let commands = &result["commands"];
                assert_eq!(result["selection"]["phase"], case["next"]);
                if profile == "appleWorkspace" {
                    assert_eq!(
                        result["completionRecords"]["provisionalBreak"]["startCommandId"],
                        commands[1]["id"]
                    );
                    assert_eq!(
                        result["completionRecords"]["phaseAdvance"]["finishCommandId"],
                        commands[0]["id"]
                    );
                    assert!(commands[1].get("generatedBreak").is_none());
                    assert_eq!(
                        result["ownershipWrites"],
                        json!([{"kind": "recordStart",
                        "timerId": commands[1]["timerId"], "deviceId": "device-local", "startCommandId": commands[1]["id"]}])
                    );
                } else {
                    assert!(result["completionRecords"]["provisionalBreak"].is_null());
                }
                if profile == "pwaStorage" {
                    assert_eq!(commands[1]["dependsOnCommandId"], commands[0]["id"]);
                    assert_eq!(commands[1]["generatedBreak"], true);
                    assert_eq!(
                        result["ownershipWrites"][0]["leaseExpiresAtMs"],
                        1784548890000_i64
                    );
                } else {
                    assert!(result["durableCommands"][1].get("deviceId").is_none());
                    assert!(commands[1].get("dependsOnCommandId").is_none());
                }
                if profile == "androidCoordinator" {
                    assert_eq!(
                        result["ownershipWrites"],
                        json!([{"kind": "setOwnedTimerId", "timerId": commands[1]["timerId"]}])
                    );
                }
            }
        }
    }
}

#[test]
fn break_completion_and_foreign_manual_finish_do_not_spend_break_candidates() {
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        let mut input = request(profile, false);
        input["requestedTimer"]["phase"] = json!("short_break");
        input["workspace"]["base"]["canonicalTimer"]["phase"] = json!("short_break");
        assert_eq!(
            call(&input).unwrap()["commands"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            call(&input).unwrap()["allocation"]["lastUuid"],
            input["identities"]["commandUuids"][0]
        );
        if profile != "pwaStorage" {
            let mut input = request(profile, false);
            input["ownership"]["deviceId"] = json!("device-foreign");
            let result = call(&input).unwrap();
            assert_eq!(result["commands"].as_array().unwrap().len(), 1);
            assert_eq!(result["allocation"]["deviceSequence"], 8);
        }
    }
}

#[test]
fn noop_and_invalid_generated_reservations_do_not_commit() {
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        let mut input = request(profile, true);
        for key in ["observedAt", "physicalNow", "occurredAt"] {
            input["clock"][key] = json!("2026-07-20T12:00:54.999Z");
        }
        let result = call(&input).unwrap();
        assert_eq!(result["reason"], "notExpired");
        assert_eq!(result["allocation"], input["allocation"]);
        input["requestedTimer"]["status"] = json!("paused");
        input["workspace"]["base"]["canonicalTimer"]["status"] = json!("paused");
        assert_eq!(call(&input).unwrap()["reason"], "notExpired");
        input = request(profile, false);
        input["identities"]["commandUuids"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(call(&input).is_err());
        input = request(profile, false);
        input["allocation"]["deviceSequence"] = json!(9_007_199_254_740_990_i64);
        assert!(call(&input).is_err());
    }
}

#[test]
fn pending_settings_choose_effective_auto_start_and_break_duration() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-projection-v1.json")).unwrap();
    let mut input = request("androidCoordinator", true);
    input["workspace"]["base"]["autoStartBreaks"] = json!(false);
    let mut auto = fixture["request"]["local"]["autoStartOperations"][0].clone();
    let mut duration = fixture["request"]["local"]["durationOperations"][0].clone();
    duration["phase"] = json!("short_break");
    duration["durationMs"] = json!(180000);
    input["workspace"]["local"]["autoStartOperations"] = json!([auto]);
    input["workspace"]["local"]["durationOperations"] = json!([duration]);
    input["workspace"]["neverSent"] =
        json!({"autoStartOperations": ["auto"], "durationOperations": ["duration"]});
    let result = assert_batch(&input, &json!("short_break"));
    assert_eq!(result["commands"][1]["plannedDurationMs"], 180000);
    input["workspace"]["canonicalHead"]["counter"] = json!(11);
    assert_eq!(
        call(&input).unwrap()["commands"].as_array().unwrap().len(),
        1
    );
    input["workspace"]["canonicalHead"]["counter"] = json!(0);
    auto["enabled"] = json!(false);
    input["workspace"]["base"]["autoStartBreaks"] = json!(true);
    input["workspace"]["local"]["autoStartOperations"] = json!([auto]);
    assert_eq!(
        call(&input).unwrap()["commands"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn generated_break_inherits_parent_for_later_finish() {
    for profile in ["pwaStorage", "androidCoordinator"] {
        let first = assert_batch(&request(profile, false), &json!("short_break"));
        let mut input = request(profile, false);
        input["workspace"] = first["workspace"].clone();
        input["allocation"] = first["allocation"].clone();
        input["observation"] = first["observation"].clone();
        input["requestedTimer"] = first["projection"]["canonicalTimer"].clone();
        input["selection"] = first["selection"].clone();
        for key in ["occurredAt", "observedAt", "physicalNow"] {
            input["clock"][key] = json!("2026-07-20T12:00:11Z");
        }
        input["identities"]["commandUuids"] = json!(["019f7f65-e0f8-7000-8000-000000000001"]);
        input["identities"]["timerUuid"] = Value::Null;
        let result = call(&input).unwrap_or_else(|error| panic!("{profile}: {error}"));
        assert_eq!(result["commands"].as_array().unwrap().len(), 1);
        assert_eq!(result["commands"][0]["type"], "finish");
        assert_eq!(
            result["workspace"]["timerDependencies"][1]["dependsOnOperationId"],
            first["commands"][1]["id"]
        );
        assert_eq!(
            result["workspace"]["timerDependencies"][1]["operationId"],
            result["commands"][0]["id"]
        );
        assert_eq!(
            result["commands"][0].get("dependsOnCommandId").is_some(),
            profile == "pwaStorage"
        );
    }
}

#[test]
fn duplicate_retry_and_owner_denial_leave_allocation_and_proof_unchanged() {
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        let mut input = request(profile, true);
        input["ownership"]["deviceId"] = json!("device-foreign");
        let denied = call(&input).unwrap();
        assert_eq!(denied["outcome"], "noop");
        assert_eq!(denied["workspace"], input["workspace"]);
        assert_eq!(denied["allocation"], input["allocation"]);
        input["ownership"]["deviceId"] = json!("device-local");
        let first = assert_batch(&input, &json!("short_break"));
        input["workspace"] = first["workspace"].clone();
        input["allocation"] = first["allocation"].clone();
        input["observation"] = first["observation"].clone();
        input["identities"]["commandUuids"] = json!([
            "019f7f66-a060-7000-8000-000000000003",
            "019f7f66-a060-7000-8000-000000000004"
        ]);
        let retry = call(&input).unwrap();
        assert_eq!(retry["outcome"], "noop");
        assert_eq!(retry["allocation"], input["allocation"]);
        assert_eq!(retry["workspace"], input["workspace"]);
        assert!(retry["effectsAfterCommit"].as_array().unwrap().is_empty());
    }
}

#[test]
fn second_allocation_overflow_fails_as_whole_batch() {
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        let mut input = request(profile, true);
        input["allocation"]["deviceSequence"] = json!(9_007_199_254_740_990_i64);
        let error = call(&input).unwrap_err();
        assert!(
            error.contains("sequence") || error.contains("JavaScript"),
            "{profile}: {error}"
        );
        input = request(profile, true);
        input["allocation"]["hlc"] = json!({"wallMs": 1784548860000_i64,
            "counter": 9_007_199_254_740_990_i64});
        assert!(call(&input).is_err(), "{profile}");
        input = request(profile, true);
        input["identities"]["commandUuids"][1] = input["identities"]["commandUuids"][0].clone();
        assert!(call(&input).is_err(), "duplicate command UUID: {profile}");
    }
}

#[test]
fn apple_boundary_retry_requires_measured_expiry_and_exact_ten_milliseconds() {
    let mut input = request("appleWorkspace", true);
    input["requestedTimer"]["elapsedAtAnchorMs"] = json!(5000);
    input["workspace"]["base"]["canonicalTimer"]["elapsedAtAnchorMs"] = json!(5000);
    for key in ["occurredAt", "physicalNow"] {
        input["clock"][key] = json!("2026-07-20T12:00:54.999Z");
    }
    input["clock"]["observedAt"] = json!("2026-07-20T12:00:55.009Z");
    input["identities"]["commandUuids"] = json!([
        "019f7f66-8cd7-7000-8000-000000000001",
        "019f7f66-8cd7-7000-8000-000000000002"
    ]);
    assert!(call(&input).is_err());
    input["boundaryRetry"] = json!({"originalObservedAt": "2026-07-20T12:00:54.999Z",
        "measuredElapsedMs": 60000});
    assert_eq!(
        call(&input).unwrap()["commands"].as_array().unwrap().len(),
        2
    );
    input["boundaryRetry"]["measuredElapsedMs"] = json!(59999);
    assert!(call(&input).is_err());
    input["boundaryRetry"]["measuredElapsedMs"] = json!(60000);
    input["clock"]["observedAt"] = json!("2026-07-20T12:00:55.008Z");
    assert!(call(&input).is_err());
}

#[test]
fn explicit_apple_selection_keeps_phase_but_stages_provisional_break() {
    let mut input = request("appleWorkspace", false);
    input["selection"]["explicit"] = json!(true);
    let result = assert_batch(&input, &json!("short_break"));
    assert_eq!(result["selection"], input["selection"]);
    assert!(result["completionRecords"]["phaseAdvance"].is_null());
    assert_eq!(
        result["completionRecords"]["provisionalBreak"]["startCommandId"],
        result["commands"][1]["id"]
    );
}

#[test]
fn staged_profiles_finish_only_and_retain_break_opportunity() {
    let mut input = request("appleWorkspace", false);
    input["replicationMode"] = json!("iroh");
    let result = call(&input).unwrap();
    assert_eq!(result["commands"].as_array().unwrap().len(), 1);
    assert_eq!(result["commands"][0]["type"], "finish");
    assert_eq!(
        result["lifecycle"]["pendingBreaks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for profile in ["desktopStorage", "desktopTerminal"] {
        for automatic in [false, true] {
            input = request(profile, automatic);
            let result = call(&input).unwrap();
            assert_eq!(
                result["commands"].as_array().unwrap().len(),
                1,
                "{profile}/{automatic}"
            );
            assert_eq!(
                result["completionRecords"]["pendingAutoBreak"]["finishCommandId"],
                result["commands"][0]["id"]
            );
        }
    }
}

#[test]
fn peer_tab_lease_blocks_generated_batch_until_expiry() {
    let mut input = request("pwaStorage", true);
    input["ownership"]["tabId"] = json!("tab-peer");
    let denied = call(&input).unwrap();
    assert_eq!(denied["reason"], "not_owner");
    assert_eq!(denied["retryAtMs"], 1784548870000_i64);
    assert_eq!(denied["allocation"], input["allocation"]);
    assert_eq!(denied["workspace"], input["workspace"]);
    input["leaseNowMs"] = json!(1784548870000_i64);
    assert_eq!(
        call(&input).unwrap()["ownershipWrites"][0]["leaseExpiresAtMs"],
        1784548900000_i64
    );
    input["leaseDurationMs"] = json!(9_007_199_254_740_991_i64);
    assert!(call(&input).is_err());
}

#[test]
fn rejected_finish_cascades_generated_start_through_real_reconciliation() {
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        let input = request(profile, false);
        let result = assert_batch(&input, &json!("short_break"));
        let finish = &result["commands"][0];
        let start = &result["commands"][1];
        let response = json!({"acknowledgements": [{"commandId": finish["id"],
            "outcome": "rejected", "reason": "conflict"}],
            "taskAcknowledgements": [], "durationAcknowledgements": [],
            "autoStartAcknowledgements": [], "selectedTaskAcknowledgements": [],
            "revision": 1, "canonicalTimer": input["workspace"]["base"]["canonicalTimer"],
            "history": [], "tasks": [], "selectedTaskId": null,
            "durationsMs": input["workspace"]["base"]["durationsMs"],
            "autoStartBreaks": true, "serverTime": "2026-07-20T12:00:11Z",
            "serverHlcWallMs": 1784548811000_i64, "serverHlcCounter": 0});
        let mut sent = input["workspace"]["local"].clone();
        sent["commands"] = json!([finish]);
        let rebase = json!({"local": result["workspace"]["local"], "sent": sent,
            "response": response, "timerDependencies": result["workspace"]["timerDependencies"],
            "neverSent": {"commands": [start["id"]]}});
        let reduced: Value = serde_json::from_str(
            &pomodorough_core::dispatch_json("reconcile.rebase.v2", &rebase.to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(reduced["pending"], json!([]), "{profile}");
        assert_eq!(reduced["pendingTimerDependencies"], json!([]), "{profile}");
        assert!(
            reduced["droppedTimerOperationIds"]
                .as_array()
                .is_some_and(|ids| ids.contains(&start["id"])),
            "{profile}"
        );
    }
}

fn break_finish_input(profile: &str, first: &Value) -> Value {
    let mut input = request(profile, false);
    input["workspace"] = first["workspace"].clone();
    input["allocation"] = first["allocation"].clone();
    input["observation"] = first["observation"].clone();
    input["requestedTimer"] = first["projection"]["canonicalTimer"].clone();
    input["selection"] = first["selection"].clone();
    input["ownership"]["timerId"] = first["commands"][1]["timerId"].clone();
    for key in ["occurredAt", "observedAt", "physicalNow"] {
        input["clock"][key] = json!("2026-07-20T12:00:11Z");
    }
    input["identities"]["commandUuids"] = json!(["019f7f65-e0f8-7000-8000-000000000001"]);
    input["identities"]["timerUuid"] = Value::Null;
    input
}

fn finish_and_break(profile: &str) -> (Value, Value) {
    let first = assert_batch(&request(profile, false), &json!("short_break"));
    let followup = call(&break_finish_input(profile, &first)).unwrap();
    assert_eq!(followup["commands"][0]["type"], "finish");
    (first, followup)
}

fn central_response(acknowledgements: Value, canonical_timer: Value, history: Value) -> Value {
    json!({"acknowledgements": acknowledgements, "taskAcknowledgements": [],
        "durationAcknowledgements": [], "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [], "revision": 1,
        "canonicalTimer": canonical_timer, "history": history,
        "tasks": [], "selectedTaskId": null,
        "durationsMs": {"focus": 60000, "short_break": 120000, "long_break": 180000},
        "autoStartBreaks": true, "serverTime": "2026-07-20T12:00:12Z",
        "serverHlcWallMs": 1784548812000_i64, "serverHlcCounter": 0})
}

fn rebase_pending(
    local: &Value,
    dependencies: &Value,
    sent: Value,
    response: Value,
    proof: Value,
) -> Value {
    let input = json!({"local": local, "sent": sent, "response": response,
        "timerDependencies": dependencies, "neverSent": {"commands": proof}});
    serde_json::from_str(
        &pomodorough_core::dispatch_json("reconcile.rebase.v2", &input.to_string()).unwrap(),
    )
    .unwrap()
}

#[test]
fn followup_break_finish_depends_on_exact_generated_start_in_all_profiles() {
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        let (first, followup) = finish_and_break(profile);
        let start = &first["commands"][1];
        let finish = &followup["commands"][0];
        let edge = &followup["workspace"]["timerDependencies"][1];
        assert_eq!(edge["operationId"], finish["id"], "{profile}");
        assert_eq!(edge["dependsOnOperationId"], start["id"], "{profile}");
        assert_eq!(edge["generatedBreak"], Value::Null, "{profile}");
        assert_eq!(
            finish.get("dependsOnCommandId").is_some(),
            profile == "pwaStorage"
        );
        if profile == "pwaStorage" {
            assert_eq!(finish["dependsOnCommandId"], start["id"]);
            assert_eq!(
                followup["durableCommands"][0]["dependsOnCommandId"],
                start["id"]
            );
        }
        assert_eq!(
            followup["workspace"]["neverSent"]["commands"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
}

#[test]
fn pause_then_finish_chains_to_latest_break_command_independent_of_queue_order() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        let first = assert_batch(&request(profile, false), &json!("short_break"));
        let mut pause = fixture["request"].clone();
        pause["compatibility"] = json!(profile);
        pause["intent"] = json!({"kind": "pause"});
        pause["workspace"] = first["workspace"].clone();
        pause["selection"] = first["selection"].clone();
        pause["allocation"] = first["allocation"].clone();
        pause["observation"] = first["observation"].clone();
        pause["requestedTimer"] = first["projection"]["canonicalTimer"].clone();
        pause["identities"] = json!({"commandUuids": ["019f7f65-e0f8-7000-8000-000000000001"],
            "timerUuid": null});
        for key in ["occurredAt", "observedAt", "physicalNow"] {
            pause["clock"][key] = json!("2026-07-20T12:00:11Z");
        }
        let paused: Value = serde_json::from_str(
            &pomodorough_core::dispatch_json("workspace.intent.v1", &pause.to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            paused["workspace"]["timerDependencies"][1]["dependsOnOperationId"],
            first["commands"][1]["id"],
            "{profile}"
        );
        for reversed in [false, true] {
            let mut finish = request(profile, false);
            finish["workspace"] = paused["workspace"].clone();
            if reversed {
                finish["workspace"]["local"]["commands"]
                    .as_array_mut()
                    .unwrap()
                    .swap(1, 2);
            }
            finish["allocation"] = paused["allocation"].clone();
            finish["observation"] = paused["observation"].clone();
            finish["selection"] = paused["selection"].clone();
            finish["requestedTimer"] = paused["projection"]["canonicalTimer"].clone();
            finish["ownership"]["timerId"] = first["commands"][1]["timerId"].clone();
            finish["identities"] = json!({"commandUuids": ["019f7f65-e4e0-7000-8000-000000000001"],
                "timerUuid": null});
            for key in ["occurredAt", "observedAt", "physicalNow"] {
                finish["clock"][key] = json!("2026-07-20T12:00:12Z");
            }
            let result =
                call(&finish).unwrap_or_else(|error| panic!("{profile}/{reversed}: {error}"));
            assert_eq!(
                result["workspace"]["timerDependencies"][2]["dependsOnOperationId"],
                paused["commands"][0]["id"],
                "{profile}/{reversed}"
            );
        }
    }
}

#[test]
fn focus_ack_does_not_promote_followup_before_exact_start_ack() {
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        let (first, followup) = finish_and_break(profile);
        let focus = &first["commands"][0];
        let start = &first["commands"][1];
        let later = followup["commands"][0].clone();
        let mut sent = first["workspace"]["local"].clone();
        sent["commands"] = json!([focus]);
        let response = central_response(
            json!([{"commandId": focus["id"], "outcome": "applied"}]),
            Value::Null,
            json!([first["projection"]["history"][0]]),
        );
        let result = rebase_pending(
            &followup["workspace"]["local"],
            &followup["workspace"]["timerDependencies"],
            sent,
            response,
            json!([start["id"], later["id"]]),
        );
        assert_eq!(
            result["promotedTimerOperationIds"],
            json!([start["id"]]),
            "{profile}"
        );
        assert_eq!(
            result["pendingTimerDependencies"],
            json!([{
                "operationId": later["id"], "dependsOnOperationId": start["id"]
            }]),
            "{profile}"
        );
        assert_eq!(
            selected_timer_ids(&result["pending"], &result["pendingTimerDependencies"]),
            json!([start["id"]]),
            "delivery gate {profile}"
        );
        let mut expected = json!([start, later]);
        if profile == "pwaStorage" {
            for command in expected.as_array_mut().unwrap() {
                command
                    .as_object_mut()
                    .unwrap()
                    .remove("dependsOnCommandId");
                command.as_object_mut().unwrap().remove("generatedBreak");
            }
        }
        assert_eq!(result["pending"], expected, "wire fields {profile}");
        assert_eq!(
            followup["workspace"]["neverSent"]["commands"],
            json!([focus["id"], start["id"], later["id"]]),
            "proof {profile}"
        );
    }
}

fn held_break(profile: &str) -> (Value, Value, Value) {
    let (first, followup) = finish_and_break(profile);
    let focus = &first["commands"][0];
    let start = &first["commands"][1];
    let later = &followup["commands"][0];
    let mut sent = request(profile, false)["workspace"]["local"].clone();
    sent["commands"] = json!([focus]);
    let response = central_response(
        json!([{"commandId": focus["id"], "outcome": "applied"}]),
        Value::Null,
        json!([first["projection"]["history"][0]]),
    );
    let held = rebase_pending(
        &followup["workspace"]["local"],
        &followup["workspace"]["timerDependencies"],
        sent,
        response,
        json!([start["id"], later["id"]]),
    );
    assert_eq!(
        held["pendingTimerDependencies"],
        json!([{
            "operationId": later["id"], "dependsOnOperationId": start["id"]
        }])
    );
    (first, followup, held)
}

fn pending_queues(commands: Value) -> Value {
    json!({"commands": commands, "taskOperations": [], "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []})
}

fn selected_timer_ids(commands: &Value, dependencies: &Value) -> Value {
    let positions: Vec<_> = commands
        .as_array()
        .unwrap()
        .iter()
        .map(|command| {
            json!({
        "id": command["id"], "deviceId": command["deviceId"],
        "deviceSequence": command["deviceSequence"], "hlcWallMs": command["hlcWallMs"],
        "hlcCounter": command["hlcCounter"]})
        })
        .collect();
    let edges: Vec<_> = dependencies.as_array().unwrap().iter().map(|edge| json!({
        "operationId": edge["operationId"], "dependsOnOperationId": edge["dependsOnOperationId"]
    })).collect();
    let request = json!({"kind": "new", "mode": "sync", "queues": pending_queues(json!(positions)),
        "limits": {"perDomain": 256, "total": 512}, "nextDomain": "commands",
        "timerDependencies": edges});
    let result: Value = serde_json::from_str(
        &pomodorough_core::dispatch_json("sync.batchPlan.v1", &request.to_string()).unwrap(),
    )
    .unwrap();
    result["selected"]["commands"].clone()
}

#[test]
fn rejected_start_drops_unsent_followup_after_focus_was_applied() {
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        let (first, followup, held) = held_break(profile);
        let start = &first["commands"][1];
        let later = &followup["commands"][0];
        let sent = pending_queues(json!([held["pending"][0]]));
        let response = central_response(
            json!([{"commandId": start["id"],
            "outcome": "rejected", "reason": "conflict"}]),
            Value::Null,
            json!([first["projection"]["history"][0]]),
        );
        let rejected = rebase_pending(
            &pending_queues(held["pending"].clone()),
            &held["pendingTimerDependencies"],
            sent,
            response,
            json!([later["id"]]),
        );
        assert_eq!(rejected["pending"], json!([]), "{profile}");
        assert_eq!(rejected["pendingTimerDependencies"], json!([]), "{profile}");
        assert_eq!(
            rejected["droppedTimerOperationIds"],
            json!([later["id"]]),
            "{profile}"
        );
        assert_eq!(
            rejected["promotedTimerOperationIds"],
            json!([]),
            "{profile}"
        );
        assert_eq!(
            selected_timer_ids(&rejected["pending"], &rejected["pendingTimerDependencies"]),
            json!([]),
            "{profile}"
        );
        assert_eq!(
            held["pendingTimerDependencies"][0]["dependsOnOperationId"],
            start["id"]
        );
        assert_eq!(
            followup["workspace"]["neverSent"]["commands"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
}

#[test]
fn applied_start_releases_followup_once_then_ack_consumes_it() {
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        let (first, followup, held) = held_break(profile);
        let start = &first["commands"][1];
        let later = &followup["commands"][0];
        let history = json!([first["projection"]["history"][0]]);
        let start_result: Value = serde_json::from_str(
            &pomodorough_core::dispatch_json(
                "timer.reduce.v1",
                &json!({"commands": [start], "history": history,
                "now": "2026-07-20T12:00:12Z"})
                .to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        let response = central_response(
            json!([{"commandId": start["id"], "outcome": "applied"}]),
            start_result["canonicalTimer"].clone(),
            history.clone(),
        );
        let accepted = rebase_pending(
            &pending_queues(held["pending"].clone()),
            &held["pendingTimerDependencies"],
            pending_queues(json!([held["pending"][0]])),
            response,
            json!([later["id"]]),
        );
        assert_eq!(
            accepted["promotedTimerOperationIds"],
            json!([later["id"]]),
            "{profile}"
        );
        assert_eq!(accepted["pendingTimerDependencies"], json!([]), "{profile}");
        assert_eq!(
            accepted["pending"],
            json!([held["pending"][1]]),
            "{profile}"
        );
        assert_eq!(
            selected_timer_ids(&accepted["pending"], &accepted["pendingTimerDependencies"]),
            json!([later["id"]]),
            "{profile}"
        );
        let completed: Value = serde_json::from_str(
            &pomodorough_core::dispatch_json(
                "timer.reduce.v1",
                &json!({"commands": [start, later], "history": history,
                "now": "2026-07-20T12:00:12Z"})
                .to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        let response = central_response(
            json!([{"commandId": later["id"], "outcome": "applied"}]),
            Value::Null,
            completed["history"].clone(),
        );
        let consumed = rebase_pending(
            &pending_queues(accepted["pending"].clone()),
            &accepted["pendingTimerDependencies"],
            pending_queues(accepted["pending"].clone()),
            response,
            json!([]),
        );
        assert_eq!(consumed["pending"], json!([]), "{profile}");
        assert_eq!(
            consumed["promotedTimerOperationIds"],
            json!([]),
            "{profile}"
        );
    }
}

#[test]
fn unrelated_applied_barrier_does_not_release_held_followup() {
    for profile in ["pwaStorage", "androidCoordinator", "appleWorkspace"] {
        let (first, followup, held) = held_break(profile);
        let start = &first["commands"][1];
        let later = &followup["commands"][0];
        let other = json!({"id": "other-barrier", "deviceId": "device-foreign",
            "deviceSequence": 1, "timerId": "other-timer", "type": "clear", "phase": "focus",
            "plannedDurationMs": 60000, "observedElapsedMs": 0,
            "occurredAt": "2026-07-20T12:00:11.500Z", "hlcWallMs": 1784548811500_i64,
            "hlcCounter": 0});
        let mut local = held["pending"].as_array().unwrap().clone();
        local.push(other.clone());
        let response = central_response(
            json!([{"commandId": other["id"], "outcome": "applied"}]),
            Value::Null,
            json!([first["projection"]["history"][0]]),
        );
        let result = rebase_pending(
            &pending_queues(json!(local)),
            &held["pendingTimerDependencies"],
            pending_queues(json!([other])),
            response,
            json!([start["id"], later["id"]]),
        );
        assert_eq!(result["promotedTimerOperationIds"], json!([]), "{profile}");
        assert_eq!(
            result["pendingTimerDependencies"], held["pendingTimerDependencies"],
            "{profile}"
        );
        assert_eq!(result["pending"], held["pending"], "{profile}");
        assert_eq!(
            selected_timer_ids(&result["pending"], &result["pendingTimerDependencies"]),
            json!([start["id"]]),
            "{profile}"
        );
    }
}
