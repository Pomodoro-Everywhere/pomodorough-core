use serde_json::{Value, json};

fn request(profile: &str, status: &str) -> Value {
    let fixtures: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json"))
            .expect("workspace fixture");
    let mut input = fixtures["request"].clone();
    input.as_object_mut().unwrap().remove("intent");
    input["stage"] = json!("finishCommit");
    input["compatibility"] = json!(profile);
    input["selection"]["explicit"] = json!(false);
    input["identities"]["timerUuid"] = Value::Null;
    input["identities"]["commandUuids"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    input["ownership"] = Value::Null;
    let mut timer = fixtures["timer"].clone();
    timer["status"] = json!(status);
    input["workspace"]["base"]["canonicalTimer"] = timer.clone();
    input["requestedTimer"] = timer;
    input
}

fn call(input: &Value) -> Result<Value, String> {
    pomodorough_core::dispatch_json("workspace.completionMutation.v1", &input.to_string())
        .map(|output| serde_json::from_str(&output).unwrap())
        .map_err(|error| error.to_string())
}

fn automatic(profile: &str) -> Value {
    let mut input = request(profile, "running");
    input["stage"] = json!("automaticFinishCommit");
    input["clock"]["occurredAt"] = json!("2026-07-20T12:01:00Z");
    input["clock"]["physicalNow"] = json!("2026-07-20T12:01:00Z");
    input["clock"]["observedAt"] = json!("2026-07-20T12:01:00Z");
    input["identities"]["commandUuids"] = json!(["019f7f66-a060-7000-8000-000000000001"]);
    input["ownership"] = json!({"timerId": "existing-timer", "deviceId": "device-local"});
    if profile == "pwaStorage" {
        input["ownership"]["tabId"] = json!("tab-local");
        input["ownership"]["leaseExpiresAtMs"] = json!(1784548870000_i64);
        input["localTabId"] = json!("tab-local");
        input["leaseNowMs"] = json!(1784548860000_i64);
    }
    input
}

fn pwa_parity_request(name: &str) -> Value {
    let mut input = automatic("pwaStorage");
    match name {
        "missing-canonical-foreign-start" => {
            input["ownership"] = Value::Null;
            input["workspace"]["base"]["canonicalTimer"] = Value::Null;
            input["workspace"]["canonicalHead"] =
                json!({"wallMs": 1784548799000_i64, "counter": 0});
            input["workspace"]["local"]["commands"] = json!([{
                "id": "start-foreign", "deviceId": "device-foreign", "deviceSequence": 1,
                "timerId": "existing-timer", "type": "start", "phase": "focus",
                "plannedDurationMs": 60000, "observedElapsedMs": 0,
                "occurredAt": "2026-07-20T12:00:00Z", "hlcWallMs": 1784548800000_i64,
                "hlcCounter": 0
            }]);
            input["workspace"]["neverSent"] = json!({"commands": ["start-foreign"]});
            let mut replay = input["workspace"].clone();
            replay["now"] = json!("1970-01-01T00:00:00Z");
            let projected =
                pomodorough_core::dispatch_json("workspace.project.v1", &replay.to_string())
                    .unwrap();
            let projected: Value = serde_json::from_str(&projected).unwrap();
            input["requestedTimer"] = projected["workspace"]["canonicalTimer"].clone();
        }
        "manual-foreign-timer-owner" => {
            input["stage"] = json!("finishCommit");
            input["ownership"]["timerId"] = json!("other-timer");
        }
        "foreign-owner-denied" => input["ownership"]["deviceId"] = json!("device-foreign"),
        "peer-lease-live" => input["ownership"]["tabId"] = json!("tab-peer"),
        "same-tab-missing-lease" => {
            input["ownership"]
                .as_object_mut()
                .unwrap()
                .remove("leaseExpiresAtMs");
        }
        _ => panic!("unknown PWA parity case: {name}"),
    }
    input
}

#[test]
fn pwa_production_parity_vectors_preserve_exact_noop_and_retry_shape() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/completion-mutation-v1.json")).unwrap();
    for case in fixture["pwaParityCases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let input = pwa_parity_request(name);
        let result = call(&input).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(result["outcome"], case["expectedOutcome"], "{name}");
        assert_eq!(result["reason"], case["expectedReason"], "{name}");
        assert_eq!(
            result.get("retryAtMs").is_some(),
            case["retryAtMsPresent"],
            "{name}"
        );
        if case["retryAtMsPresent"] == true {
            assert_eq!(result["retryAtMs"], case["retryAtMs"], "{name}");
        }
        if result["outcome"] == "planned" {
            assert_eq!(result["commands"][0]["type"], "finish", "{name}");
            assert_eq!(
                result["ownershipWrites"],
                json!([{"kind": "removeTimerOwner"}]),
                "{name}"
            );
        } else {
            assert_eq!(result["allocation"], input["allocation"], "{name}");
            assert_eq!(result["workspace"], input["workspace"], "{name}");
            assert_eq!(result["effectsAfterCommit"], json!([]), "{name}");
        }
    }
}

#[test]
fn manual_finish_ignores_unrelated_owner_record_for_all_centralized_clients() {
    for profile in ["pwaStorage", "appleWorkspace", "androidCoordinator"] {
        let mut input = request(profile, "running");
        input["ownership"] = json!({"timerId": "other-timer", "deviceId": "device-foreign"});
        let result = call(&input).unwrap();
        assert_eq!(result["outcome"], "planned", "{profile}");
        assert_eq!(result["commands"][0]["type"], "finish", "{profile}");
    }
}

#[test]
fn automatic_owner_vectors_use_shared_strict_schema() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/completion-mutation-v1.json")).unwrap();
    for case in fixture["automaticCases"].as_array().unwrap() {
        let profile = case["profile"].as_str().unwrap();
        let mut input = automatic(profile);
        if case["ownerDeviceId"].is_null() {
            input["ownership"] = Value::Null;
        } else {
            input["ownership"]["deviceId"] = case["ownerDeviceId"].clone();
        }
        if profile == "pwaStorage" {
            input["leaseNowMs"] = case["leaseNowMs"].clone();
            if !input["ownership"].is_null() {
                input["ownership"]["tabId"] = case["ownerTabId"].clone();
                if case["leaseExpiresAtMs"].is_null() {
                    input["ownership"]
                        .as_object_mut()
                        .unwrap()
                        .remove("leaseExpiresAtMs");
                } else {
                    input["ownership"]["leaseExpiresAtMs"] = case["leaseExpiresAtMs"].clone();
                }
            }
        }
        let result = call(&input).unwrap_or_else(|error| panic!("{}: {error}", case["name"]));
        let expected = case["expected"].as_str().unwrap();
        assert_eq!(
            if result["outcome"] == "planned" {
                "planned"
            } else {
                result["reason"].as_str().unwrap()
            },
            expected,
            "{}",
            case["name"]
        );
        if profile == "pwaStorage" {
            assert_eq!(
                result.get("retryAtMs"),
                case.get("retryAtMs"),
                "{}",
                case["name"]
            );
        } else {
            assert_eq!(result["retryAtMs"], Value::Null, "{}", case["name"]);
        }
        if result["outcome"] == "noop" {
            assert_eq!(result["allocation"], input["allocation"]);
            assert!(result["commands"].as_array().unwrap().is_empty());
            assert!(result["ownershipWrites"].as_array().unwrap().is_empty());
            assert!(result["effectsAfterCommit"].as_array().unwrap().is_empty());
        }
    }
}

#[test]
fn automatic_finish_waits_for_expiry_and_keeps_manual_bypass() {
    for profile in ["appleWorkspace", "androidCoordinator", "pwaStorage"] {
        let mut input = automatic(profile);
        let planned = call(&input).unwrap();
        assert_eq!(planned["outcome"], "planned", "{profile}");
        assert_eq!(planned["commands"][0]["type"], "finish");
        if profile == "pwaStorage" {
            assert!(planned.get("retryAtMs").is_none());
        } else {
            assert_eq!(planned["retryAtMs"], Value::Null);
        }
        assert_eq!(
            planned["atomicCommandIds"],
            json!([planned["commands"][0]["id"]])
        );
        assert_eq!(planned["effectsAfterCommit"][0]["kind"], "launchSync");
        assert_eq!(planned["effectsAfterCommit"][1]["kind"], "cancelAlarm");
        assert_eq!(
            planned["ownershipWrites"],
            if profile == "pwaStorage" {
                json!([{"kind": "removeTimerOwner"}])
            } else {
                json!([])
            }
        );
        input["clock"]["observedAt"] = json!("2026-07-20T12:00:54.999Z");
        input["clock"]["physicalNow"] = input["clock"]["observedAt"].clone();
        input["clock"]["occurredAt"] = input["clock"]["observedAt"].clone();
        let ms = chrono::DateTime::parse_from_rfc3339("2026-07-20T12:00:54.999Z")
            .unwrap()
            .timestamp_millis();
        input["identities"]["commandUuids"] = json!([format!(
            "{:08x}-{:04x}-7000-8000-000000000001",
            ms >> 16,
            ms & 0xffff
        )]);
        let early = call(&input).unwrap();
        assert_eq!(early["outcome"], "noop", "{profile}");
        assert_eq!(early["allocation"], input["allocation"]);
        assert_eq!(early["effectsAfterCommit"], json!([]));
        input["stage"] = json!("finishCommit");
        assert_eq!(call(&input).unwrap()["outcome"], "planned", "{profile}");
    }
}

#[test]
fn automatic_owner_lease_and_identity_are_core_decisions() {
    let mut input = automatic("pwaStorage");
    input["ownership"]["tabId"] = json!("tab-peer");
    let denied = call(&input).unwrap();
    assert_eq!(denied["outcome"], "noop");
    assert_eq!(denied["reason"], "not_owner");
    assert_eq!(denied["retryAtMs"], json!(1784548870000_i64));
    assert_eq!(denied["workspace"], input["workspace"]);
    assert_eq!(denied["allocation"], input["allocation"]);
    assert_eq!(denied["effectsAfterCommit"], json!([]));
    input["leaseNowMs"] = json!(1784548870000_i64);
    assert_eq!(call(&input).unwrap()["outcome"], "planned");
    input["ownership"]["deviceId"] = json!("device-foreign");
    let denied = call(&input).unwrap();
    assert_eq!(denied["reason"], "not_owner");
    assert!(denied.get("retryAtMs").is_none());
    for profile in ["appleWorkspace", "androidCoordinator"] {
        let mut input = automatic(profile);
        input["ownership"]["deviceId"] = json!("device-foreign");
        assert_eq!(call(&input).unwrap()["reason"], "notOwner");
    }
}

#[test]
fn automatic_missing_pwa_owner_claim_is_bounded_by_start_origin() {
    let mut input = automatic("pwaStorage");
    input["ownership"] = Value::Null;
    assert_eq!(call(&input).unwrap()["outcome"], "planned");
    input["workspace"]["base"]["canonicalTimer"]["startedByDeviceId"] = json!("device-foreign");
    input["requestedTimer"]["startedByDeviceId"] = json!("device-foreign");
    assert_eq!(call(&input).unwrap()["reason"], "not_owner");
    input["workspace"]["base"]["canonicalTimer"]
        .as_object_mut()
        .unwrap()
        .remove("startedByDeviceId");
    input["requestedTimer"]
        .as_object_mut()
        .unwrap()
        .remove("startedByDeviceId");
    assert_eq!(call(&input).unwrap()["reason"], "not_owner");
}

#[test]
fn automatic_auto_start_requires_break_candidates_and_rejects_invalid_lease() {
    let mut input = automatic("pwaStorage");
    input["workspace"]["base"]["autoStartBreaks"] = json!(true);
    assert!(call(&input).is_err());
    input["workspace"]["base"]["autoStartBreaks"] = json!(false);
    input["ownership"]["leaseExpiresAtMs"] = json!(-1);
    assert!(call(&input).is_err());
    input["ownership"]["leaseExpiresAtMs"] = json!(1784548870000_i64);
    input["ownerGranted"] = json!(true);
    assert!(call(&input).is_err());
}

#[test]
fn automatic_paused_peer_and_concurrent_finish_never_spend_identity() {
    let mut input = automatic("pwaStorage");
    input["requestedTimer"]["status"] = json!("paused");
    input["workspace"]["base"]["canonicalTimer"]["status"] = json!("paused");
    let paused = call(&input).unwrap();
    assert_eq!(paused["reason"], "notExpired");
    assert_eq!(paused["allocation"], input["allocation"]);
    let mut input = automatic("androidCoordinator");
    input["workspace"]["base"]["canonicalTimer"]["lastIntent"] = json!({
        "type": "finish", "commandId": "concurrent-finish", "occurredAt": "2026-07-20T12:00:20Z"
    });
    let concurrent = call(&input).unwrap();
    assert_eq!(concurrent["reason"], "staleTimer");
    assert_eq!(concurrent["allocation"], input["allocation"]);
    assert_eq!(concurrent["effectsAfterCommit"], json!([]));
    input["requestedTimer"] = input["workspace"]["base"]["canonicalTimer"].clone();
    assert_eq!(call(&input).unwrap()["reason"], "staleTimer");
    input["requestedTimer"]["lastIntent"] = json!({
        "type": "start", "commandId": "concurrent-finish", "occurredAt": "2026-07-20T12:00:20Z"
    });
    assert_eq!(call(&input).unwrap()["reason"], "staleTimer");
}

#[test]
fn automatic_pwa_monotonic_expiry_ignores_wall_clock_jump() {
    let mut input = automatic("pwaStorage");
    input["observation"]["monotonicAnchor"] = json!({
        "timerId": "existing-timer", "anchorAt": "2026-07-20T12:00:00Z",
        "elapsedAtAnchorMs": 5000, "sampledTrustedNowMs": 1784548810000_i64,
        "sampledMonotonicMs": 100, "continuityId": "tab-lifetime"
    });
    input["clock"]["continuityId"] = json!("tab-lifetime");
    input["clock"]["monotonicNowMs"] = json!(1000);
    assert_eq!(call(&input).unwrap()["reason"], "notExpired");
    input["clock"]["monotonicNowMs"] = json!(45100);
    input["clock"]["occurredAt"] = json!("2026-07-20T12:00:30Z");
    input["clock"]["physicalNow"] = json!("2026-07-20T12:00:30Z");
    input["clock"]["observedAt"] = json!("2026-07-20T12:00:30Z");
    let ms = chrono::DateTime::parse_from_rfc3339("2026-07-20T12:00:30Z")
        .unwrap()
        .timestamp_millis();
    input["identities"]["commandUuids"] = json!([format!(
        "{:08x}-{:04x}-7000-8000-000000000001",
        ms >> 16,
        ms & 0xffff
    )]);
    let result = call(&input).unwrap();
    assert_eq!(result["outcome"], "planned");
    assert_eq!(result["commands"][0]["observedElapsedMs"], 60000);
}

#[test]
fn automatic_effective_auto_start_enable_requires_break_candidates() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-projection-v1.json")).unwrap();
    let mut input = automatic("pwaStorage");
    let mut enable = fixture["request"]["local"]["autoStartOperations"][0].clone();
    enable["enabled"] = json!(true);
    input["workspace"]["local"]["autoStartOperations"] = json!([enable.clone()]);
    input["workspace"]["neverSent"]["autoStartOperations"] = json!([enable["id"]]);
    assert!(call(&input).is_err());
}

#[test]
fn automatic_claims_missing_pwa_owner_from_retained_local_start() {
    let mut input = automatic("pwaStorage");
    input["ownership"] = Value::Null;
    input["workspace"]["base"]["canonicalTimer"] = Value::Null;
    input["workspace"]["canonicalHead"] = json!({"wallMs": 1784548799000_i64, "counter": 0});
    input["workspace"]["local"]["commands"] = json!([{
        "id": "start-local", "deviceId": "device-local", "deviceSequence": 1,
        "timerId": "existing-timer", "type": "start", "phase": "focus",
        "plannedDurationMs": 60000, "observedElapsedMs": 0,
        "occurredAt": "2026-07-20T12:00:00Z", "hlcWallMs": 1784548800000_i64,
        "hlcCounter": 0
    }]);
    input["workspace"]["neverSent"] = json!({"commands": ["start-local"]});
    let mut replay = input["workspace"].clone();
    replay["now"] = json!("1970-01-01T00:00:00Z");
    let projected =
        pomodorough_core::dispatch_json("workspace.project.v1", &replay.to_string()).unwrap();
    let projected: Value = serde_json::from_str(&projected).unwrap();
    input["requestedTimer"] = projected["workspace"]["canonicalTimer"].clone();
    assert_eq!(call(&input).unwrap()["outcome"], "planned");
    input["workspace"]["local"]["commands"][0]["deviceId"] = json!("device-foreign");
    input["workspace"]["neverSent"] = json!({"commands": ["start-local"]});
    assert_eq!(call(&input).unwrap()["outcome"], "planned");
}

#[test]
fn automatic_wrong_owner_timer_returns_not_owner_without_allocating() {
    let mut input = automatic("pwaStorage");
    input["ownership"]["timerId"] = json!("other-timer");
    let denied = call(&input).unwrap();
    assert_eq!(denied["reason"], "not_owner");
    assert!(denied.get("retryAtMs").is_none());
    assert_eq!(denied["allocation"], input["allocation"]);
    assert_eq!(denied["observation"], input["observation"]);
    assert_eq!(denied["effectsAfterCommit"], json!([]));
    input["stage"] = json!("finishCommit");
    assert_eq!(call(&input).unwrap()["outcome"], "planned");
}

#[test]
fn automatic_expired_running_start_intent_is_not_a_prior_finish() {
    for profile in ["appleWorkspace", "androidCoordinator", "pwaStorage"] {
        let mut input = automatic(profile);
        let start = json!({"type": "start", "commandId": "start-1",
            "occurredAt": "2026-07-20T12:00:00Z"});
        input["workspace"]["base"]["canonicalTimer"]["lastIntent"] = start.clone();
        input["requestedTimer"]["lastIntent"] = start;
        let result = call(&input).unwrap();
        assert_eq!(result["outcome"], "planned", "{profile}");
        assert_eq!(result["commands"][0]["type"], "finish");
    }
}

#[test]
fn automatic_rejects_duplicate_raw_schema_fields() {
    let mut raw = automatic("pwaStorage").to_string();
    raw = raw.replacen(
        "\"stage\":\"automaticFinishCommit\"",
        "\"stage\":\"automaticFinishCommit\",\"stage\":\"finishCommit\"",
        1,
    );
    assert!(pomodorough_core::dispatch_json("workspace.completionMutation.v1", &raw).is_err());
}

fn snapshot_case(name: &str) -> Value {
    let fixtures: Value =
        serde_json::from_str(include_str!("../fixtures/completion-mutation-v1.json")).unwrap();
    fixtures["snapshotCases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap()
        .clone()
}

fn snapshot_request(case: &Value) -> Value {
    let mut input = request(case["profile"].as_str().unwrap(), "running");
    for change in case["changes"].as_array().unwrap() {
        let mut field = &mut input;
        for key in change["path"].as_str().unwrap().split('.') {
            field = &mut field[key];
        }
        *field = change["value"].clone();
    }
    input
}

fn assert_stale_snapshot(name: &str) {
    let case = snapshot_case(name);
    let input = snapshot_request(&case);
    let result = call(&input).unwrap();
    assert_eq!(result["outcome"], case["outcome"], "{name}");
    assert_eq!(result["reason"], "staleTimer", "{name}");
    assert_eq!(result["workspace"], input["workspace"], "{name}");
    assert_eq!(result["allocation"], input["allocation"], "{name}");
    assert_eq!(result["commands"], json!([]), "{name}");
    assert_eq!(result["effectsAfterCommit"], json!([]), "{name}");
}

#[test]
fn changed_task_is_stale() {
    assert_stale_snapshot("desktop-task-changed");
}

#[test]
fn retargeted_timer_is_stale() {
    assert_stale_snapshot("desktop-retarget-stale");
}

#[test]
fn same_intent_id_cannot_hide_changed_type() {
    assert_stale_snapshot("apple-intent-type-changed");
}

#[test]
fn same_intent_id_cannot_hide_changed_time() {
    assert_stale_snapshot("android-intent-time-changed");
}

#[test]
fn missing_intent_is_stale_against_present_intent() {
    assert_stale_snapshot("pwa-intent-presence-changed");
}

#[test]
fn workspace_now_cannot_override_planner_owned_time() {
    let input = snapshot_request(&snapshot_case("extraneous-workspace-now"));
    assert!(call(&input).is_err());
}

#[test]
fn matching_no_task_retarget_and_deadline_snapshots_still_finish() {
    for name in [
        "no-task-same-snapshot",
        "retarget-current-snapshot",
        "android-same-resume-intent",
        "pwa-same-start-intent",
        "manual-at-deadline",
    ] {
        let case = snapshot_case(name);
        let result = call(&snapshot_request(&case)).unwrap();
        assert_eq!(result["outcome"], case["outcome"], "{name}");
        assert_eq!(result["commands"][0]["type"], "finish", "{name}");
    }
}

#[test]
fn projected_retarget_keeps_its_valid_intent_and_rejects_old_task_snapshot() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../fixtures/completion-mutation-v1.json")).unwrap();
    let mut input = request("desktopStorage", "running");
    input["workspace"]["base"]["canonicalTimer"]["taskId"] = json!("task-before-retarget");
    input["workspace"]["base"]["canonicalTimer"]["lastIntent"] =
        json!({"type": "start", "commandId": "start-1", "occurredAt": "2026-07-20T12:00:00Z"});
    input["requestedTimer"] = input["workspace"]["base"]["canonicalTimer"].clone();
    input["workspace"]["local"]["commands"] = json!([fixtures["retargetCommand"]]);
    input["workspace"]["neverSent"] = json!({"commands": ["retarget-1"]});
    let mut replay = input["workspace"].clone();
    replay["now"] = json!("1970-01-01T00:00:00Z");
    let projected =
        pomodorough_core::dispatch_json("workspace.project.v1", &replay.to_string()).unwrap();
    let projected: Value = serde_json::from_str(&projected).unwrap();
    let current = projected["workspace"]["canonicalTimer"].clone();
    assert_eq!(current["taskId"], "task-retargeted");
    assert_eq!(current["lastIntent"], input["requestedTimer"]["lastIntent"]);
    assert_eq!(call(&input).unwrap()["outcome"], "noop");
    input["requestedTimer"] = current;
    assert_eq!(call(&input).unwrap()["outcome"], "planned");
}

#[test]
fn manual_finish_is_a_core_owned_atomic_command() {
    let result = call(&request("appleWorkspace", "running")).expect("finish plan");
    assert_eq!(result["outcome"], "planned");
    assert_eq!(result["commands"][0]["type"], "finish");
    assert_eq!(result["selection"]["phase"], "short_break");
    assert_eq!(
        result["atomicCommandIds"],
        json!(["command-019f7f65-dd10-7000-8000-000000000001"])
    );
    assert_eq!(
        result["completionRecords"]["phaseAdvance"]["generation"],
        "6"
    );
    assert_eq!(result["durableCommands"][0].get("deviceId"), None);
    assert_eq!(
        result["projection"]["history"][0]["commandId"],
        result["commands"][0]["id"]
    );
}

#[test]
fn profiles_and_active_states_use_exactly_one_finish() {
    let cases: Value =
        serde_json::from_str(include_str!("../fixtures/completion-mutation-v1.json")).unwrap();
    for profile in cases["profiles"].as_array().unwrap() {
        for status in cases["states"].as_array().unwrap() {
            let mut input = request(profile.as_str().unwrap(), status.as_str().unwrap());
            if status == "paused" {
                input["requestedTimer"]["elapsedAtAnchorMs"] = json!(18000);
                input["workspace"]["base"]["canonicalTimer"]["elapsedAtAnchorMs"] = json!(18000);
            }
            let result = call(&input).unwrap_or_else(|error| panic!("{profile}/{status}: {error}"));
            assert_eq!(result["outcome"], "planned", "{profile}/{status}");
            assert_eq!(result["commands"].as_array().unwrap().len(), 1);
            assert_eq!(result["commands"][0]["type"], "finish");
            assert_eq!(
                result["workspace"]["timerDependencies"],
                input["workspace"]["timerDependencies"]
            );
            assert_eq!(result["selection"]["phase"], "short_break");
            assert_eq!(
                result["commands"][0]["observedElapsedMs"],
                if status == "paused" {
                    json!(18000)
                } else {
                    json!(15000)
                }
            );
            assert_eq!(
                result["projection"]["canonicalTimer"]["status"],
                "completed"
            );
            assert_eq!(
                result["durableCommands"][0].get("deviceId").is_some(),
                profile == "pwaStorage"
            );
        }
    }
}

#[test]
fn focus_count_from_zero_through_twelve_and_break_to_focus() {
    let cases: Value =
        serde_json::from_str(include_str!("../fixtures/completion-mutation-v1.json")).unwrap();
    for count in 0..=12 {
        let mut input = request("androidCoordinator", "running");
        for index in 0..count {
            let mut history = json!({"id": format!("past-{index}"), "timerId": format!("past-timer-{index}"),
                "phase": "focus", "status": "completed", "plannedDurationMs": 60000,
                "completedAt": "2026-07-20T11:00:00Z"});
            history["commandId"] = json!(format!("past-finish-{index}"));
            input["workspace"]["base"]["history"]
                .as_array_mut()
                .unwrap()
                .push(history);
        }
        let result = call(&input).unwrap();
        assert_eq!(
            result["selection"]["phase"], cases["focusPhases"][count],
            "count={count}"
        );
    }
    for phase in ["short_break", "long_break"] {
        let mut input = request("pwaStorage", "running");
        input["requestedTimer"]["phase"] = json!(phase);
        input["workspace"]["base"]["canonicalTimer"]["phase"] = json!(phase);
        input["selection"]["phase"] = json!(phase);
        assert_eq!(call(&input).unwrap()["selection"]["phase"], "focus");
    }
}

#[test]
fn deadline_and_early_finish_preserve_exact_command_provenance() {
    for at in [
        "2026-07-20T12:00:10Z",
        "2026-07-20T12:00:59.999Z",
        "2026-07-20T12:01:00Z",
        "2026-07-20T12:01:00.001Z",
    ] {
        let mut input = request("pwaStorage", "running");
        input["clock"]["occurredAt"] = json!(at);
        input["clock"]["physicalNow"] = json!(at);
        input["clock"]["observedAt"] = json!(at);
        let ms = chrono::DateTime::parse_from_rfc3339(at)
            .unwrap()
            .timestamp_millis();
        input["identities"]["commandUuids"] = json!([format!(
            "{:08x}-{:04x}-7000-8000-000000000001",
            ms >> 16,
            ms & 0xffff
        )]);
        let result = call(&input).unwrap();
        assert_eq!(result["outcome"], "planned", "{at}");
        assert_eq!(
            result["projection"]["history"][0]["commandId"],
            result["commands"][0]["id"]
        );
        assert_eq!(
            result["commands"][0]["observedElapsedMs"],
            if at < "2026-07-20T12:01:00Z" {
                json!(if at.ends_with("10Z") { 15000 } else { 60000 })
            } else {
                json!(60000)
            }
        );
    }
}

#[test]
fn stale_duplicate_and_explicit_choice_do_not_allocate_unwanted_work() {
    let mut input = request("appleWorkspace", "running");
    input["selection"]["explicit"] = json!(true);
    let result = call(&input).unwrap();
    assert_eq!(result["selection"], input["selection"]);
    assert!(result["completionRecords"]["phaseAdvance"].is_null());
    for change in ["id", "phase", "anchorAt"] {
        let mut stale = request("desktopStorage", "running");
        stale["requestedTimer"][change] = match change {
            "id" => json!("different"),
            "phase" => json!("long_break"),
            _ => json!("2026-07-20T11:59:59Z"),
        };
        let result = call(&stale).unwrap();
        assert_eq!(result["outcome"], "noop");
        assert_eq!(result["workspace"], stale["workspace"]);
        assert_eq!(result["allocation"], stale["allocation"]);
        assert!(result["commands"].as_array().unwrap().is_empty());
        assert!(result["effectsAfterCommit"].as_array().unwrap().is_empty());
    }
    let mut duplicate = request("appleWorkspace", "running");
    duplicate["workspace"]["local"]["commands"] = json!([result["commands"][0]]);
    duplicate["workspace"]["neverSent"] = json!({"commands": [result["commands"][0]["id"]]});
    duplicate["identities"]["commandUuids"] = json!(["019f7f65-dd10-7000-8000-000000000002"]);
    duplicate["allocation"]["lastUuid"] = json!("019f7f65-dd10-7000-8000-000000000001");
    assert_eq!(call(&duplicate).unwrap()["outcome"], "noop");
    let mut foreign_owner = request("pwaStorage", "running");
    foreign_owner["ownership"] = json!({"timerId": "other-timer", "deviceId": "device-local",
        "tabId": "tab-a", "leaseExpiresAtMs": 1784548820000_i64});
    assert_eq!(call(&foreign_owner).unwrap()["outcome"], "planned");
}

#[test]
fn unsupported_and_corrupt_boundaries_fail_without_partial_writes() {
    let mut cases = Vec::new();
    let mut value = request("androidCoordinator", "running");
    value["replicationMode"] = json!("iroh");
    cases.push(value);
    let mut value = request("appleWorkspace", "running");
    value["workspace"]["base"]["autoStartBreaks"] = json!(true);
    cases.push(value);
    let mut value = request("pwaStorage", "running");
    value["workspace"]["base"]["canonicalTimer"]["dependsOnCommandId"] = json!("parent");
    cases.push(value);
    let mut value = request("pwaStorage", "running");
    value["requestedTimer"]["dependsOnCommandId"] = json!("parent");
    cases.push(value);
    let mut value = request("pwaStorage", "running");
    value.as_object_mut().unwrap().remove("ownership");
    cases.push(value);
    let mut value = request("appleWorkspace", "running");
    value["workspace"]["local"]
        .as_object_mut()
        .unwrap()
        .remove("taskOperations");
    cases.push(value);
    let mut value = request("appleWorkspace", "running");
    value["identities"]["commandUuids"] = json!(["invalid"]);
    cases.push(value);
    let mut value = request("androidCoordinator", "running");
    value["calendarIntervals"] = json!([]);
    cases.push(value);
    let mut value = request("androidCoordinator", "running");
    value["identities"]["commandUuids"] = json!(["019f7f65-dd11-7000-8000-000000000001"]);
    cases.push(value);
    let mut value = request("appleWorkspace", "running");
    value["calendarIntervals"][0]["end"] = json!("2026-07-20T00:00:00Z");
    cases.push(value);
    for (index, value) in cases.iter().enumerate() {
        assert!(call(value).is_err(), "case {index}");
    }
}

#[test]
fn all_five_retained_domains_choose_effective_auto_start_and_keep_payloads() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-projection-v1.json")).unwrap();
    let mut input = request("androidCoordinator", "running");
    let mut pending = fixture["request"]["local"].clone();
    pending["commands"] = json!([{"id": "pause", "deviceId": "device-a", "deviceSequence": 1,
        "timerId": "existing-timer", "type": "pause", "phase": "focus", "plannedDurationMs": 60000,
        "occurredAt": "2026-07-20T12:00:01Z", "hlcWallMs": 1784548801000_i64,
        "hlcCounter": 0, "observedElapsedMs": 6000}]);
    pending["autoStartOperations"][0]["enabled"] = json!(false);
    input["workspace"]["local"] = pending.clone();
    input["workspace"]["base"]["autoStartBreaks"] = json!(true);
    input["workspace"]["base"]["tasks"] = fixture["request"]["base"]["tasks"].clone();
    input["workspace"]["base"]["selectedTaskId"] =
        fixture["request"]["base"]["selectedTaskId"].clone();
    input["workspace"]["neverSent"] = fixture["request"]["neverSent"].clone();
    input["workspace"]["neverSent"]["commands"] = json!(["pause"]);
    let mut workspace = input["workspace"].clone();
    workspace["now"] = json!("1970-01-01T00:00:00Z");
    let projected =
        pomodorough_core::dispatch_json("workspace.project.v1", &workspace.to_string()).unwrap();
    let projected: Value = serde_json::from_str(&projected).unwrap();
    input["requestedTimer"] = projected["workspace"]["canonicalTimer"].clone();
    let result = call(&input).unwrap();
    assert_eq!(result["outcome"], "planned");
    assert_eq!(result["projection"]["autoStartBreaks"], false);
    assert_eq!(result["projection"]["durationsMs"]["focus"], 1800000);
    assert_eq!(result["projection"]["tasks"], json!([]));
    assert!(result["projection"]["selectedTaskId"].is_null());
    for name in [
        "taskOperations",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ] {
        assert_eq!(result["workspace"]["local"][name], pending[name]);
    }
    input["workspace"]["canonicalHead"] = json!({"wallMs": 1784548800000_i64, "counter": 11});
    let result = call(&input).unwrap();
    assert_eq!(result["projection"]["autoStartBreaks"], true);
    assert_eq!(result["commands"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["allocation"]["lastUuid"],
        input["identities"]["commandUuids"][0]
    );
}

#[test]
fn pwa_monotonic_observation_prevents_wall_jump_from_forging_expiry() {
    let mut input = request("pwaStorage", "running");
    input["observation"]["monotonicAnchor"] = json!({"timerId": "existing-timer",
        "anchorAt": "2026-07-20T12:00:00Z", "elapsedAtAnchorMs": 5000,
        "sampledTrustedNowMs": 1784548810000_i64, "sampledMonotonicMs": 100,
        "continuityId": "tab-lifetime"});
    input["clock"]["continuityId"] = json!("tab-lifetime");
    input["clock"]["monotonicNowMs"] = json!(1100);
    for name in ["occurredAt", "physicalNow", "observedAt"] {
        input["clock"][name] = json!("2026-07-20T12:02:00Z");
    }
    let ms = chrono::DateTime::parse_from_rfc3339("2026-07-20T12:02:00Z")
        .unwrap()
        .timestamp_millis();
    input["identities"]["commandUuids"] = json!([format!(
        "{:08x}-{:04x}-7000-8000-000000000001",
        ms >> 16,
        ms & 0xffff
    )]);
    let result = call(&input).unwrap();
    assert_eq!(result["commands"][0]["observedElapsedMs"], 16000);
    assert_eq!(result["outcome"], "planned");
}
