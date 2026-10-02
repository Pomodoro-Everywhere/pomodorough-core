use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/completion-lifecycle-v1.json")).unwrap()
}

#[test]
fn documented_full_request_fixture_plans_exactly_one_start() {
    let input: Value = serde_json::from_str(include_str!(
        "../fixtures/completion-lifecycle-request-v1.json"
    ))
    .unwrap();
    let result = call(&input).unwrap();
    assert_eq!(result["schemaVersion"], 1);
    assert_eq!(
        result["atomicCommandIds"],
        json!(["command-019f7f66-8cd8-7000-8000-000000000010"])
    );
    assert_eq!(result["commands"][0]["type"], "start");
    assert_eq!(result["nextPhase"], "short_break");
    assert_eq!(
        result["lifecycle"]["consumedCompletions"],
        json!([
        {"timerId": "existing-timer", "commandId": null, "phase": "focus"}])
    );
}

#[test]
fn trusted_occurrence_before_physical_expiry_keeps_platform_admission_distinct() {
    for profile in ["appleWorkspace", "androidCoordinator", "desktopStorage"] {
        let mut input = expiry(profile);
        let physical = input["clock"]["physicalNow"].clone();
        set_clock(&mut input, "2026-07-20T12:00:54.999Z");
        input["clock"]["physicalNow"] = physical.clone();
        input["clock"]["observedAt"] = physical;
        let result = call(&input).unwrap();
        if profile == "appleWorkspace" {
            assert_eq!(result["reason"], "startBeforeCompletion");
            assert_eq!(result["outcome"], "noop");
            assert_eq!(result["lifecycle"], input["lifecycle"]);
            untouched(&input, &result);
        } else {
            assert_eq!(result["commands"][0]["type"], "start");
            assert_eq!(
                result["commands"][0]["occurredAt"],
                "2026-07-20T12:00:54.999Z"
            );
        }
    }
}

fn base(profile: &str) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let mut input = fixture["request"].clone();
    input.as_object_mut().unwrap().remove("intent");
    input["compatibility"] = json!(profile);
    input["workspace"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    input["workspace"]["base"]["autoStartBreaks"] = json!(true);
    input["requestedTimer"] = fixture["timer"].clone();
    input["ownership"] = json!({"timerId": "existing-timer", "deviceId": "device-local"});
    input["selection"]["explicit"] = json!(false);
    input["identities"]["commandUuids"] = json!(["019f7f65-dd10-7000-8000-000000000001"]);
    input["lifecycle"] = json!({"consumedCompletions": [], "pendingBreaks": []});
    input
}

fn set_clock(input: &mut Value, at: &str) {
    for field in ["occurredAt", "observedAt", "physicalNow"] {
        input["clock"][field] = json!(at);
    }
    let ms = chrono::DateTime::parse_from_rfc3339(at)
        .unwrap()
        .timestamp_millis();
    let hex = format!("{ms:012x}");
    input["identities"]["commandUuids"] = json!([format!(
        "{}-{}-7000-8000-000000000010",
        &hex[..8],
        &hex[8..]
    )]);
}

fn expiry(profile: &str) -> Value {
    let mut input = base(profile);
    input["stage"] = json!("expiryObservation");
    input["replicationMode"] = json!("iroh");
    input["previousWorkspace"] = input["workspace"].clone();
    input["previousObservation"] = input["observation"].clone();
    input["event"] = fixture()["event"].clone();
    input["centralizedSession"] = fixture()["centralizedSession"].clone();
    set_clock(&mut input, "2026-07-20T12:00:55.000Z");
    input
}

fn finish(profile: &str, mode: &str, automatic: bool) -> (Value, Value) {
    let mut input = base(profile);
    input["replicationMode"] = json!(mode);
    input["stage"] = json!(if automatic {
        "automaticFinishCommit"
    } else {
        "finishCommit"
    });
    if automatic {
        set_clock(&mut input, "2026-07-20T12:00:55.000Z");
    }
    let result = call(&input).unwrap();
    (input, result)
}

fn deferred(input: &Value, result: &Value) -> Value {
    let mut next = input.clone();
    next["stage"] = json!("deferredBreakOpportunity");
    next.as_object_mut().unwrap().remove("requestedTimer");
    next["workspace"] = result["workspace"].clone();
    next["allocation"] = result["allocation"].clone();
    next["observation"] = result["observation"].clone();
    next["selection"] = result["selection"].clone();
    next["lifecycle"] = result["lifecycle"].clone();
    if input["replicationMode"] == "iroh" {
        next["previousWorkspace"] = result["workspace"].clone();
        next["previousObservation"] = result["observation"].clone();
    }
    next["event"] = json!({"kind": "opportunity"});
    next["centralizedSession"] = fixture()["centralizedSession"].clone();
    set_clock(&mut next, "2026-07-20T12:01:00.000Z");
    next
}

fn call(input: &Value) -> Result<Value, String> {
    pomodorough_core::dispatch_json("workspace.completionMutation.v1", &input.to_string())
        .map(|output| serde_json::from_str(&output).unwrap())
        .map_err(|error| error.to_string())
}

fn prior(input: &mut Value, count: usize) {
    let offset = input["workspace"]["base"]["history"]
        .as_array()
        .unwrap()
        .len();
    for index in offset..offset + count {
        input["workspace"]["base"]["history"]
            .as_array_mut()
            .unwrap()
            .push(json!({
            "id": format!("history-{index}"), "timerId": format!("past-{index}"),
            "commandId": format!("finish-{index}"), "phase": "focus", "status": "completed",
            "plannedDurationMs": 60000, "completedAt": "2026-07-20T11:00:00Z"}));
    }
}

fn install_source(input: &mut Value, finish: &Value) {
    input["workspace"]["base"]["canonicalTimer"] = finish["projection"]["canonicalTimer"].clone();
    input["workspace"]["base"]["history"] = finish["projection"]["history"].clone();
    input["workspace"]["local"]["commands"] = json!([]);
    input["workspace"]["neverSent"]["commands"] = json!([]);
    input["observation"]["commandTimes"] = json!({});
}

#[test]
fn accepted_staged_finish_uses_same_raw_terminal_pair_as_workspace_read_and_intent() {
    for (profile, mode) in [
        ("appleWorkspace", "iroh"),
        ("desktopStorage", "centralized"),
    ] {
        let (input, first) = finish(profile, mode, false);
        let mut opportunity = deferred(&input, &first);
        install_source(&mut opportunity, &first);
        if mode == "iroh" {
            opportunity["previousWorkspace"] = opportunity["workspace"].clone();
            opportunity["previousObservation"] = opportunity["observation"].clone();
        }
        let mut raw = opportunity["workspace"].clone();
        raw["now"] = opportunity["clock"]["occurredAt"].clone();
        let projected: Value = serde_json::from_str(
            &pomodorough_core::dispatch_json("workspace.project.v1", &raw.to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            projected["workspace"]["canonicalTimer"],
            first["projection"]["canonicalTimer"]
        );
        assert_eq!(
            projected["workspace"]["history"],
            first["projection"]["history"]
        );
        let read = json!({"profile": profile, "source": {"kind": "workspace", "value": opportunity["workspace"]},
            "selectedPhase": opportunity["selection"]["phase"], "observedAt": opportunity["clock"]["observedAt"],
            "calendarIntervals": opportunity["calendarIntervals"]});
        let read: Value = serde_json::from_str(
            &pomodorough_core::dispatch_json("workspace.readModel.v1", &read.to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(read["canonical"]["status"], "completed");
        let result = call(&opportunity).unwrap();
        assert_eq!(result["commands"][0]["type"], "start");
        assert_eq!(
            result["workspace"]["base"],
            opportunity["workspace"]["base"]
        );
        assert_eq!(
            result["projection"]["history"],
            projected["workspace"]["history"]
        );
    }
}

#[test]
fn staged_terminal_overlap_rejects_task_and_identity_conflicts_before_allocation() {
    let (input, first) = finish("desktopStorage", "centralized", false);
    for (field, value) in [
        ("taskId", json!("other-task")),
        ("timerId", json!("other-timer")),
        ("phase", json!("short_break")),
        ("commandId", json!("other-command")),
        ("completedAt", json!("2026-07-20T12:00:11Z")),
    ] {
        let mut opportunity = deferred(&input, &first);
        install_source(&mut opportunity, &first);
        opportunity["workspace"]["base"]["history"][0][field] = value;
        assert!(
            call(&opportunity)
                .unwrap_err()
                .contains("conflicting workspace terminal"),
            "{field}"
        );
    }
}

fn untouched(input: &Value, result: &Value) {
    for field in ["workspace", "allocation", "observation"] {
        assert_eq!(result[field], input[field], "{field}");
    }
    assert_eq!(result["commands"], json!([]));
}

#[test]
fn checker_apple_expiry_start_preserves_complete_explicit_selection() {
    let mut input = expiry("appleWorkspace");
    input["selection"] = json!({"phase": "focus", "generation": "5", "explicit": true});
    let result = call(&input).unwrap();
    assert_eq!(result["commands"][0]["type"], "start");
    assert_eq!(result["commands"][0]["phase"], "short_break");
    assert_eq!(result["selection"], input["selection"]);
}

fn explicit_apple_manual_finish() -> (Value, Value) {
    let mut input = base("appleWorkspace");
    input["replicationMode"] = json!("iroh");
    input["stage"] = json!("finishCommit");
    input["selection"] = json!({"phase": "long_break", "generation": "5", "explicit": true});
    let first = call(&input).unwrap();
    (input, first)
}

#[test]
fn checker_apple_manual_two_stage_preserves_complete_explicit_selection() {
    let (input, first) = explicit_apple_manual_finish();
    assert_eq!(first["commands"][0]["type"], "finish");
    assert_eq!(first["selection"], input["selection"]);
    let second = call(&deferred(&input, &first)).unwrap();
    assert_eq!(second["commands"][0]["type"], "start");
    assert_eq!(second["commands"][0]["phase"], "short_break");
    assert_eq!(second["selection"], input["selection"]);
    assert_ne!(first["atomicCommandIds"], second["atomicCommandIds"]);
}

#[test]
fn checker_apple_explicit_selection_survives_start_persistence_and_restart() {
    let (input, first) = explicit_apple_manual_finish();
    let second = call(&deferred(&input, &first)).unwrap();
    let persisted: Value = serde_json::from_str(&second.to_string()).unwrap();
    let mut restarted = deferred(&input, &persisted);
    restarted["identities"]["commandUuids"] = json!([]);
    let result = call(&restarted).unwrap();
    assert_eq!(result["outcome"], "noop");
    assert_eq!(result["reason"], "noPendingBreak");
    assert_eq!(result["selection"], input["selection"]);
    untouched(&restarted, &result);
}

#[test]
fn checker_observation_event_rejects_extra_metadata() {
    for (field, value) in [
        ("sourceAccepted", json!(true)),
        ("generateAutoBreak", json!(true)),
        ("acknowledgements", json!([])),
        ("discardedCommandIds", json!([])),
        ("extension", json!({"ignored": true})),
    ] {
        let mut input = expiry("appleWorkspace");
        input["event"][field] = value;
        let result = call(&input);
        assert!(
            result.is_err(),
            "observation accepted {field}: {:?}",
            result.as_ref().map(|output| &output["atomicCommandIds"])
        );
    }
}

#[test]
fn checker_opportunity_event_rejects_extra_metadata() {
    let (input, first) = finish("desktopStorage", "centralized", false);
    for (field, value) in [
        ("sourceAccepted", json!(true)),
        ("generateAutoBreak", json!(true)),
        ("acknowledgements", json!([])),
        ("discardedCommandIds", json!([])),
        ("extension", json!({"ignored": true})),
    ] {
        let mut opportunity = deferred(&input, &first);
        opportunity["event"][field] = value;
        let result = call(&opportunity);
        assert!(
            result.is_err(),
            "opportunity accepted {field}: {:?}",
            result.as_ref().map(|output| &output["atomicCommandIds"])
        );
    }
}

#[test]
fn checker_opportunity_cannot_ignore_rejected_ack_metadata() {
    let (input, first) = finish("desktopStorage", "centralized", false);
    let mut opportunity = deferred(&input, &first);
    opportunity["event"] = json!({"kind": "canonicalInstalled", "acknowledgements": [
        {"commandId": first["commands"][0]["id"], "outcome": "rejected"}], "discardedCommandIds": []});
    let rejected = call(&opportunity).unwrap();
    assert_eq!(rejected["retiredTriggerIds"], first["atomicCommandIds"]);
    assert_eq!(rejected["commands"], json!([]));
    opportunity["event"]["kind"] = json!("opportunity");
    let result = call(&opportunity);
    assert!(
        result.is_err(),
        "opportunity ignored rejected ACK: {:?}",
        result.as_ref().map(|output| &output["atomicCommandIds"])
    );
}

#[test]
fn checker_missing_session_user_id_is_rejected_before_optimistic_start() {
    let (input, first) = finish("desktopStorage", "centralized", false);
    for authenticated in [true, false] {
        let mut opportunity = deferred(&input, &first);
        opportunity["centralizedSession"] = json!({"authenticated": authenticated});
        let result = call(&opportunity);
        assert!(
            result.is_err(),
            "missing userId admitted Start: {:?}",
            result.as_ref().map(|output| &output["atomicCommandIds"])
        );
        assert!(result.unwrap_err().contains("missing field `userId`"));
    }
}

#[test]
fn checker_explicit_null_and_present_session_user_id_keep_original_barrier() {
    let (input, first) = finish("desktopStorage", "centralized", false);
    for (user_id, authenticated, blocked) in [
        (Value::Null, false, false),
        (Value::Null, true, false),
        (json!("user"), false, false),
        (json!("user"), true, true),
    ] {
        let mut opportunity = deferred(&input, &first);
        opportunity["centralizedSession"] =
            json!({"userId": user_id, "authenticated": authenticated});
        let result = call(&opportunity).unwrap();
        if blocked {
            assert_eq!(result["reason"], "canonicalBarrier");
            assert_eq!(result["lifecycle"], opportunity["lifecycle"]);
            untouched(&opportunity, &result);
        } else {
            assert_eq!(result["commands"][0]["type"], "start");
            assert_eq!(result["sourceStatus"], "pending");
            assert!(!result["completionRecords"]["provisionalBreak"].is_null());
        }
    }
}

#[test]
fn iroh_deadline_minus_exact_plus_millisecond_is_start_only_in_all_profiles() {
    for profile in fixture()["profiles"].as_array().unwrap() {
        for case in fixture()["deadlineCases"].as_array().unwrap() {
            let mut input = expiry(profile.as_str().unwrap());
            set_clock(&mut input, case["observedAt"].as_str().unwrap());
            let result = call(&input).unwrap();
            assert_eq!(
                result["commands"].as_array().unwrap().len(),
                case["commands"].as_u64().unwrap() as usize
            );
            assert_eq!(
                result["lifecycle"]["consumedCompletions"]
                    .as_array()
                    .unwrap()
                    .len(),
                case["consumed"].as_u64().unwrap() as usize
            );
            assert!(
                result["commands"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|command| command["type"] == "start")
            );
            if case["consumed"] == 0 {
                untouched(&input, &result);
            }
        }
    }
}

#[test]
fn iroh_cadence_uses_effective_history_and_settings() {
    for (count, phase) in [
        (2, "short_break"),
        (3, "long_break"),
        (6, "short_break"),
        (7, "long_break"),
    ] {
        let mut input = expiry("appleWorkspace");
        prior(&mut input, count);
        input["previousWorkspace"] = input["workspace"].clone();
        let result = call(&input).unwrap();
        assert_eq!(result["commands"][0]["phase"], phase);
        assert_eq!(
            result["commands"][0]["plannedDurationMs"],
            input["workspace"]["base"]["durationsMs"][phase]
        );
        assert_eq!(result["workspace"]["timerDependencies"], json!([]));
        assert_eq!(result["completionRecords"]["provisionalBreak"], Value::Null);
    }
}

#[test]
fn natural_completion_consumes_once_across_repeat_and_restart() {
    let mut input = expiry("desktopStorage");
    input["workspace"]["base"]["autoStartBreaks"] = json!(false);
    input["previousWorkspace"] = input["workspace"].clone();
    let result = call(&input).unwrap();
    assert_eq!(result["outcome"], "planned");
    assert_eq!(result["commands"], json!([]));
    assert_eq!(result["allocation"], input["allocation"]);
    input["lifecycle"] = result["lifecycle"].clone();
    let repeated = call(&input).unwrap();
    assert_eq!(repeated["reason"], "alreadyConsumed");
    untouched(&input, &repeated);
    input["previousWorkspace"]["base"]["canonicalTimer"] =
        result["projection"]["canonicalTimer"].clone();
    input["previousWorkspace"]["base"]["history"] = result["projection"]["history"].clone();
    assert_eq!(call(&input).unwrap()["commands"], json!([]));
}

#[test]
fn installed_natural_completion_keeps_terminal_projection_without_reconsumption() {
    let mut input = expiry("appleWorkspace");
    input["workspace"]["base"]["autoStartBreaks"] = json!(false);
    input["previousWorkspace"] = input["workspace"].clone();
    let natural = call(&input).unwrap();
    input["workspace"]["base"]["canonicalTimer"] = natural["projection"]["canonicalTimer"].clone();
    input["workspace"]["base"]["history"] = natural["projection"]["history"].clone();
    let installed = call(&input).unwrap();
    assert_eq!(
        installed["projection"]["canonicalTimer"]["status"],
        "completed"
    );
    assert_eq!(installed["lifecycle"], natural["lifecycle"]);
    input["lifecycle"] = installed["lifecycle"].clone();
    input["previousWorkspace"] = input["workspace"].clone();
    input.as_object_mut().unwrap().remove("requestedTimer");
    let restarted = call(&input).unwrap();
    assert_eq!(
        restarted["projection"]["canonicalTimer"]["id"],
        "existing-timer"
    );
    assert_eq!(restarted["outcome"], "noop");
    untouched(&input, &restarted);
}

#[test]
fn expiry_rejects_stale_requested_timer_and_new_pause_or_replacement() {
    for key in ["anchorAt", "plannedDurationMs", "id", "lastIntent"] {
        let mut input = expiry("appleWorkspace");
        input["requestedTimer"][key] = match key {
            "anchorAt" => json!("2026-07-20T11:59:59Z"),
            "plannedDurationMs" => json!(120000),
            "id" => json!("other"),
            _ => {
                json!({"type": "resume", "commandId": "different", "occurredAt": "2026-07-20T12:00:00Z"})
            }
        };
        let result = call(&input).unwrap();
        assert_eq!(result["reason"], "staleTimer");
        untouched(&input, &result);
    }
    for status in ["paused", "running"] {
        let mut input = expiry("appleWorkspace");
        input["workspace"]["base"]["canonicalTimer"]["status"] = json!(status);
        input["workspace"]["base"]["canonicalTimer"]["id"] = json!("replacement");
        untouched(&input, &call(&input).unwrap());
    }
}

#[test]
fn foreign_owner_still_consumes_expiry_without_start_and_explicit_selection_is_preserved() {
    for profile in fixture()["profiles"].as_array().unwrap() {
        let mut input = expiry(profile.as_str().unwrap());
        input["ownership"]["deviceId"] = json!("foreign");
        input["workspace"]["base"]["canonicalTimer"]["startedByDeviceId"] = json!("foreign");
        input["requestedTimer"]["startedByDeviceId"] = json!("foreign");
        input["previousWorkspace"] = input["workspace"].clone();
        let result = call(&input).unwrap();
        assert_eq!(result["commands"], json!([]));
        assert_eq!(
            result["lifecycle"]["consumedCompletions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    let mut input = expiry("appleWorkspace");
    input["selection"]["explicit"] = json!(true);
    input["workspace"]["base"]["autoStartBreaks"] = json!(false);
    input["previousWorkspace"] = input["workspace"].clone();
    assert_eq!(call(&input).unwrap()["selection"], input["selection"]);
}

#[test]
fn apple_iroh_manual_finish_then_start_are_two_distinct_commits() {
    let (input, first) = finish("appleWorkspace", "iroh", false);
    assert_eq!(first["commands"].as_array().unwrap().len(), 1);
    assert_eq!(first["commands"][0]["type"], "finish");
    assert_eq!(first["allocation"]["deviceSequence"], 8);
    assert_eq!(first["completionRecords"]["phaseAdvance"], Value::Null);
    assert_eq!(first["effectsAfterCommit"], json!([{"kind": "launchSync"}]));
    let opportunity = deferred(&input, &first);
    let second = call(&opportunity).unwrap();
    assert_eq!(second["commands"].as_array().unwrap().len(), 1);
    assert_eq!(second["commands"][0]["type"], "start");
    assert_eq!(second["allocation"]["deviceSequence"], 9);
    assert_eq!(
        second["effectsAfterCommit"][1],
        json!({"kind": "cancelAlarm", "timerId": "existing-timer"})
    );
    assert_eq!(second["workspace"]["timerDependencies"], json!([]));
    assert_eq!(second["completionRecords"]["provisionalBreak"], Value::Null);
    assert_eq!(second["lifecycle"]["pendingBreaks"], json!([]));
    assert_ne!(first["atomicCommandIds"], second["atomicCommandIds"]);
}

#[test]
fn desktop_manual_and_automatic_finish_only_create_deferred_trigger() {
    for profile in ["desktopStorage", "desktopTerminal"] {
        for automatic in [false, true] {
            let (input, result) = finish(profile, "centralized", automatic);
            assert_eq!(result["commands"].as_array().unwrap().len(), 1);
            assert_eq!(result["commands"][0]["type"], "finish");
            assert_eq!(
                result["lifecycle"]["pendingBreaks"][0],
                result["completionRecords"]["pendingAutoBreak"]
            );
            assert_eq!(
                result["completionRecords"]["phaseAdvance"]["selectedPhaseVersion"],
                input["selection"]["generation"]
            );
            assert_eq!(
                result["allocation"]["lastUuid"],
                input["identities"]["commandUuids"][0]
            );
        }
    }
}

#[test]
fn canonical_barrier_waits_for_finish_and_pending_auto_start_settings() {
    let (input, first) = finish("desktopStorage", "centralized", true);
    let mut opportunity = deferred(&input, &first);
    opportunity["centralizedSession"] = json!({"userId": "user", "authenticated": true});
    let blocked = call(&opportunity).unwrap();
    assert_eq!(blocked["reason"], "canonicalBarrier");
    untouched(&opportunity, &blocked);
    install_source(&mut opportunity, &first);
    opportunity["workspace"]["local"]["autoStartOperations"] = json!([{
        "id": "preference", "deviceId": "device-local", "hlcWallMs": 1784548860000_i64,
        "hlcCounter": 0, "occurredAt": "2026-07-20T12:01:00Z", "enabled": false}]);
    assert_eq!(call(&opportunity).unwrap()["reason"], "canonicalBarrier");
    opportunity["workspace"]["local"]["autoStartOperations"] = json!([]);
    let accepted = call(&opportunity).unwrap();
    assert_eq!(accepted["sourceStatus"], "accepted");
    assert_eq!(accepted["commands"][0]["type"], "start");
    assert_eq!(accepted["workspace"]["timerDependencies"], json!([]));
}

#[test]
fn optimistic_start_is_provisional_and_exact_source_ack_corrects_fourth_focus() {
    let (mut input, _) = finish("desktopStorage", "centralized", false);
    prior(&mut input, 2);
    let first = call(&input).unwrap();
    let mut opportunity = deferred(&input, &first);
    let provisional = call(&opportunity).unwrap();
    assert_eq!(provisional["sourceStatus"], "pending");
    assert_eq!(provisional["commands"][0]["phase"], "short_break");
    assert_eq!(
        provisional["workspace"]["timerDependencies"][0]["dependsOnOperationId"],
        first["commands"][0]["id"]
    );
    install_source(&mut opportunity, &first);
    prior(&mut opportunity, 1);
    let accepted = call(&opportunity).unwrap();
    assert_eq!(accepted["sourceStatus"], "accepted");
    assert_eq!(accepted["commands"][0]["phase"], "long_break");
    assert_eq!(accepted["workspace"]["timerDependencies"], json!([]));
    assert_eq!(
        accepted["completionRecords"]["provisionalBreak"],
        Value::Null
    );
}

#[test]
fn rejected_discarded_and_nonexact_ack_drop_trigger_without_allocation() {
    let (input, first) = finish("desktopStorage", "centralized", false);
    for (outcome, discarded) in [("rejected", false), ("applied", true), ("ignored", false)] {
        let mut opportunity = deferred(&input, &first);
        opportunity["event"] = json!({"kind": "canonicalInstalled", "acknowledgements": [
            {"commandId": first["commands"][0]["id"], "outcome": outcome}],
            "discardedCommandIds": if discarded {json!([first["commands"][0]["id"]])} else {json!([])}});
        let result = call(&opportunity).unwrap();
        assert_eq!(result["outcome"], "planned");
        assert_eq!(result["lifecycle"]["pendingBreaks"], json!([]));
        assert_eq!(result["retiredTriggerIds"], first["atomicCommandIds"]);
        untouched(&opportunity, &result);
    }
}

#[test]
fn unrelated_ack_keeps_canonical_barrier_and_repeat_after_start_is_noop() {
    let (input, first) = finish("desktopStorage", "centralized", false);
    let mut opportunity = deferred(&input, &first);
    opportunity["centralizedSession"] = json!({"userId": "user", "authenticated": true});
    opportunity["event"] = json!({"kind": "canonicalInstalled", "acknowledgements": [
        {"commandId": "unrelated", "outcome": "applied"}], "discardedCommandIds": []});
    assert_eq!(
        call(&opportunity).unwrap()["lifecycle"],
        opportunity["lifecycle"]
    );
    opportunity["centralizedSession"]["authenticated"] = json!(false);
    let started = call(&opportunity).unwrap();
    let mut retry = deferred(&input, &started);
    retry["identities"]["commandUuids"] = json!([]);
    let repeated = call(&retry).unwrap();
    assert_eq!(repeated["reason"], "noPendingBreak");
    untouched(&retry, &repeated);
    retry["lifecycle"] = started["lifecycle"].clone();
    assert_eq!(call(&retry).unwrap()["commands"], json!([]));
}

#[test]
fn later_unrelated_local_command_drops_trigger_but_foreign_sequence_does_not() {
    let (input, first) = finish("desktopStorage", "centralized", false);
    for (device, expected) in [("device-local", "triggerDropped"), ("foreign", "")] {
        let mut opportunity = deferred(&input, &first);
        let mut extra = first["commands"][0].clone();
        extra["id"] = json!("later-finish");
        extra["deviceId"] = json!(device);
        extra["deviceSequence"] = json!(999);
        extra["timerId"] = json!("unrelated");
        opportunity["workspace"]["local"]["commands"]
            .as_array_mut()
            .unwrap()
            .push(extra);
        opportunity["workspace"]["neverSent"]["commands"]
            .as_array_mut()
            .unwrap()
            .push(json!("later-finish"));
        let result = call(&opportunity).unwrap();
        assert_eq!(result["reason"], expected);
        assert_eq!(
            result["commands"].as_array().unwrap().len(),
            usize::from(device == "foreign")
        );
    }
}

#[test]
fn iroh_replays_frozen_domain_commands_while_centralized_defers_unsafe_source() {
    let (input, first) = finish("appleWorkspace", "iroh", false);
    let mut opportunity = deferred(&input, &first);
    opportunity["workspace"]["neverSent"]["commands"] = json!([]);
    opportunity["workspace"]["canonicalHead"] = Value::Null;
    opportunity["workspace"]["local"]["commands"][0]["extension"] = json!({"keep": null});
    let original = opportunity["workspace"]["local"]["commands"][0].clone();
    let started = call(&opportunity).unwrap();
    assert_eq!(started["commands"][0]["type"], "start");
    assert_eq!(started["workspace"]["local"]["commands"][0], original);
    opportunity["compatibility"] = json!("desktopStorage");
    opportunity["replicationMode"] = json!("centralized");
    opportunity
        .as_object_mut()
        .unwrap()
        .remove("previousWorkspace");
    opportunity
        .as_object_mut()
        .unwrap()
        .remove("previousObservation");
    opportunity["lifecycle"]["pendingBreaks"][0]
        .as_object_mut()
        .unwrap()
        .remove("reservedTimerUuid");
    let suppressed = call(&opportunity).unwrap();
    assert_eq!(suppressed["reason"], "waitingForSource");
    untouched(&opportunity, &suppressed);
}

#[test]
fn invalid_lifecycle_and_caller_policy_fields_fail_closed() {
    for field in [
        "sourceAccepted",
        "generateAutoBreak",
        "ownerGranted",
        "proposedProjection",
    ] {
        let mut input = expiry("appleWorkspace");
        input[field] = json!(true);
        assert!(call(&input).is_err(), "{field}");
    }
    let mut input = expiry("appleWorkspace");
    input["lifecycle"]["consumedCompletions"] = json!([
        {"timerId": "a", "commandId": null, "phase": "focus"},
        {"timerId": "a", "commandId": null, "phase": "focus"}]);
    assert!(call(&input).is_err());
    input = expiry("appleWorkspace");
    input["workspace"]["now"] = json!("2026-07-20T12:00:55Z");
    assert!(call(&input).is_err());
    input = expiry("appleWorkspace");
    input["clock"]["physicalNow"] = json!("2026-07-20T12:00:54Z");
    assert!(call(&input).is_err());
    input = expiry("appleWorkspace");
    input["allocation"]["deviceSequence"] = json!(9_007_199_254_740_991_i64);
    assert!(call(&input).is_err());
}

#[test]
fn android_expiry_changes_selection_and_generation_only_when_start_commits() {
    for phase in ["focus", "short_break", "long_break"] {
        for owned in [false, true] {
            let mut input = expiry("androidCoordinator");
            input["selection"]["phase"] = json!(phase);
            input["selection"]["explicit"] = json!(true);
            if !owned {
                input["ownership"] = Value::Null;
            }
            let result = call(&input).unwrap();
            assert_eq!(result["nextPhase"], "short_break");
            if owned {
                assert_eq!(result["selection"]["phase"], "short_break");
                assert_eq!(
                    result["selection"]["generation"],
                    if phase == "short_break" { "5" } else { "6" }
                );
            } else {
                assert_eq!(result["selection"], input["selection"]);
                assert_eq!(result["commands"], json!([]));
            }
        }
    }
}

#[test]
fn stage_context_and_nested_metadata_validation_fail_before_mutation() {
    let changes = [
        ("replicationMode", json!("centralized")),
        ("compatibility", json!("pwaStorage")),
        ("previousObservation", Value::Null),
        ("event", json!({"kind": "opportunity"})),
        (
            "ownership",
            json!({"timerId": "existing-timer", "deviceId": "device-local", "tabId": "tab"}),
        ),
        (
            "centralizedSession",
            json!({"userId": "", "authenticated": true}),
        ),
        ("calendarIntervals", json!([])),
        (
            "lifecycle",
            json!({"pendingBreaks": [
            {"finishCommandId": "x", "timerId": "x", "finishDeviceSequence": 1, "reservedTimerUuid": "bad"}]}),
        ),
    ];
    for (field, value) in changes {
        let mut input = expiry("appleWorkspace");
        input[field] = value;
        assert!(call(&input).is_err(), "{field}");
    }
    let mut input = expiry("appleWorkspace");
    input["clock"]["monotonicNowMs"] = json!(1);
    assert!(call(&input).is_err());
    let (finish_input, first) = finish("desktopStorage", "centralized", false);
    let mut input = deferred(&finish_input, &first);
    input["lifecycle"]["pendingBreaks"][0]["finishDeviceSequence"] = json!(1);
    assert!(
        call(&input)
            .unwrap_err()
            .contains("mismatches retained finish")
    );
    input = deferred(&finish_input, &first);
    input["event"] = json!({"kind": "canonicalInstalled", "discardedCommandIds": ["x", "x"], "acknowledgements": []});
    assert!(call(&input).is_err());
    input = deferred(&finish_input, &first);
    input["event"] = json!({"kind": "canonicalInstalled", "discardedCommandIds": [], "acknowledgements": [
        {"commandId": "x", "outcome": "ignored"}, {"commandId": "x", "outcome": "applied"}]});
    assert!(call(&input).is_err());
}

#[test]
fn apple_split_commit_freezes_timer_entropy_phase_and_duration() {
    let (input, first) = finish("appleWorkspace", "iroh", false);
    let mut opportunity = deferred(&input, &first);
    opportunity["workspace"]["base"]["durationsMs"]["short_break"] = json!(240000);
    prior(&mut opportunity, 3);
    let started = call(&opportunity).unwrap();
    assert_eq!(started["commands"][0]["phase"], "short_break");
    assert_eq!(started["commands"][0]["plannedDurationMs"], 120000);
    assert_eq!(
        started["commands"][0]["timerId"],
        "timer-12345678-1234-4234-8234-123456789012"
    );
    opportunity["identities"]["timerUuid"] = json!("12345678-1234-4234-8234-123456789013");
    assert!(
        call(&opportunity)
            .unwrap_err()
            .contains("changed reserved timer entropy")
    );
}

#[test]
fn desktop_accepts_presented_natural_completion_but_not_changed_terminal_snapshot() {
    for automatic in [false, true] {
        let (mut input, _) = finish("desktopStorage", "centralized", automatic);
        set_clock(&mut input, "2026-07-20T12:00:55.000Z");
        let natural = call(&expiry("desktopStorage")).unwrap()["source"].clone();
        input["requestedTimer"]["status"] = json!("completed");
        input["requestedTimer"]["anchorAt"] = natural["completedAt"].clone();
        input["requestedTimer"]["elapsedAtAnchorMs"] = json!(60000);
        let result = call(&input).unwrap();
        assert_eq!(result["commands"][0]["type"], "finish");
        input["requestedTimer"]["anchorAt"] = json!("2026-07-20T12:00:54.999Z");
        let stale = call(&input).unwrap();
        assert_eq!(stale["reason"], "staleTimer");
        untouched(&input, &stale);
    }
}

#[test]
fn queue_retires_discarded_head_then_materializes_next_source_in_order() {
    let (input, first) = finish("desktopStorage", "centralized", false);
    let mut opportunity = deferred(&input, &first);
    opportunity["lifecycle"]["pendingBreaks"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            json!({
        "finishCommandId": "discarded", "timerId": "past-timer", "finishDeviceSequence": 1}),
        );
    opportunity["event"] = json!({"kind": "canonicalInstalled", "acknowledgements": [],
        "discardedCommandIds": ["discarded"]});
    let result = call(&opportunity).unwrap();
    assert_eq!(
        result["retiredTriggerIds"],
        json!(["discarded", first["commands"][0]["id"]])
    );
    assert_eq!(result["commands"].as_array().unwrap().len(), 1);
    assert_eq!(result["lifecycle"]["pendingBreaks"], json!([]));
}

#[test]
fn later_same_timer_finish_does_not_retire_trigger_without_exact_source() {
    let (input, first) = finish("desktopStorage", "centralized", false);
    let mut opportunity = deferred(&input, &first);
    let mut repeated = first["commands"][0].clone();
    repeated["id"] = json!("same-timer-repeat");
    repeated["deviceSequence"] = json!(9);
    repeated["hlcCounter"] = json!(2);
    opportunity["workspace"]["local"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(repeated);
    opportunity["workspace"]["neverSent"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("same-timer-repeat"));
    let result = call(&opportunity).unwrap();
    assert_eq!(result["reason"], "waitingForSource");
    assert_eq!(result["lifecycle"], opportunity["lifecycle"]);
}

fn source_rebase(first: &Value, generated: &Value, outcome: &str, history: Value) -> Value {
    let mut sent = first["workspace"]["local"].clone();
    sent["commands"] = first["commands"].clone();
    let response = json!({"acknowledgements": [{"commandId": first["commands"][0]["id"], "outcome": outcome,
        "reason": if outcome == "rejected" {"conflict"} else {""}}],
        "taskAcknowledgements": [], "durationAcknowledgements": [], "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [], "revision": 1,
        "canonicalTimer": null,
        "history": history, "tasks": [], "selectedTaskId": null,
        "durationsMs": first["projection"]["durationsMs"], "autoStartBreaks": true,
        "serverTime": "2026-07-20T12:01:01Z", "serverHlcWallMs": 1784548861000_i64, "serverHlcCounter": 0});
    let input = json!({"local": generated["workspace"]["local"], "sent": sent, "response": response,
        "neverSent": {"commands": generated["atomicCommandIds"]}, "timerDependencies": generated["workspace"]["timerDependencies"]});
    serde_json::from_str(
        &pomodorough_core::dispatch_json("reconcile.rebase.v2", &input.to_string()).unwrap(),
    )
    .unwrap()
}

#[test]
fn staged_desktop_start_rebases_fourth_focus_and_rejects_source_without_rewriting_frozen_finish() {
    let (mut input, _) = finish("desktopStorage", "centralized", false);
    prior(&mut input, 2);
    let first = call(&input).unwrap();
    let generated = call(&deferred(&input, &first)).unwrap();
    let mut canonical = first["projection"]["history"].clone();
    canonical.as_array_mut().unwrap().push(
        json!({"id": "foreign-history", "timerId": "foreign-timer",
        "commandId": "foreign-finish", "phase": "focus", "status": "completed",
        "plannedDurationMs": 60000, "completedAt": "2026-07-20T11:30:00Z"}),
    );
    let rebased = source_rebase(&first, &generated, "applied", canonical);
    assert_eq!(rebased["pending"][0]["id"], generated["commands"][0]["id"]);
    assert_eq!(
        rebased["pending"][0]["timerId"],
        generated["commands"][0]["timerId"]
    );
    assert_eq!(rebased["pending"][0]["phase"], "long_break");
    assert_eq!(rebased["pending"][0]["plannedDurationMs"], 180000);
    assert_eq!(
        rebased["promotedTimerOperationIds"],
        generated["atomicCommandIds"]
    );
    let rejected = source_rebase(&first, &generated, "rejected", json!([]));
    assert_eq!(rejected["pending"], json!([]));
    assert_eq!(
        rejected["droppedTimerOperationIds"],
        generated["atomicCommandIds"]
    );
}
