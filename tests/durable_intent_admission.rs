use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const PROFILES: [&str; 5] = [
    "appleWorkspace",
    "androidCoordinator",
    "desktopStorage",
    "desktopTerminal",
    "pwaStorage",
];
const DOMAINS: [&str; 5] = [
    "commands",
    "taskOperations",
    "durationOperations",
    "autoStartOperations",
    "selectedTaskOperations",
];

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap()
}

fn request(profile: &str, intent: Value) -> Value {
    let mut input = fixture()["request"].clone();
    input["compatibility"] = json!(profile);
    input["intent"] = intent;
    if input["intent"]["kind"].as_str().unwrap().ends_with("Task")
        || matches!(
            input["intent"]["kind"].as_str(),
            Some("setDuration" | "changeDuration" | "setAutoStart")
        )
    {
        input["ownership"] = json!({"ownerId": null, "expectedOwnerId": null});
        input["durability"] = json!({"outgoingDurationOperationIds": [], "localTabId": "tab-a"});
    }
    input
}

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(
        &dispatch_json(operation, &input.to_string())
            .unwrap_or_else(|error| panic!("{error}: {input}")),
    )
    .unwrap()
}

fn plan(input: &Value) -> Value {
    call("workspace.intent.v1", input)
}

fn task() -> Value {
    let identity = call("task.identity.v1", &json!({"title": "Café"}));
    json!({"id": identity["id"], "title": identity["title"]})
}

fn resume_request(input: &mut Value, output: &Value, first: u64) {
    input["workspace"] = output["workspace"].clone();
    input["allocation"] = output["allocation"].clone();
    input["observation"] = output["observation"].clone();
    input["identities"]["commandUuids"] = json!(
        (first..first + 3)
            .map(|i| format!("019f7f65-dd10-7000-8000-{i:012x}"))
            .collect::<Vec<_>>()
    );
}

fn claim(input: &mut Value, domain: &str) {
    input["workspace"]["neverSent"][domain] = json!([]);
}

fn assert_retained(input: &Value, output: &Value) {
    assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
    assert_eq!(
        output["workspace"]["canonicalHead"],
        input["workspace"]["canonicalHead"]
    );
    for domain in DOMAINS {
        let old = input["workspace"]["local"][domain].as_array().unwrap();
        assert_eq!(
            &output["workspace"]["local"][domain].as_array().unwrap()[..old.len()],
            old
        );
        for op in old {
            let proof = &input["workspace"]["neverSent"][domain];
            if !proof.as_array().is_some_and(|ids| ids.contains(&op["id"])) {
                assert!(
                    !output["workspace"]["neverSent"][domain]
                        .as_array()
                        .is_some_and(|ids| ids.contains(&op["id"]))
                );
            }
        }
    }
    let restored: Value = serde_json::from_str(&input.to_string()).unwrap();
    assert_eq!(output, &plan(&restored), "complete restart output");
}

#[test]
fn desktop_selection_queues_after_realistic_selection_and_retarget_claim() {
    let mut input = request(
        "desktopStorage",
        json!({"kind": "selectTask", "taskId": null}),
    );
    input["workspace"]["base"]["canonicalTimer"] = fixture()["timer"].clone();
    let first = plan(&input);
    resume_request(&mut input, &first, 3);
    claim(&mut input, "commands");
    claim(&mut input, "selectedTaskOperations");
    let output = plan(&input);
    assert_eq!(output["outcome"], "planned");
    assert_eq!(output["groupOutcomes"]["commands"][0]["outcome"], "queued");
    assert_eq!(
        output["groupOutcomes"]["selectedTaskOperations"][0]["outcome"],
        "queued"
    );
    assert_retained(&input, &output);
}

#[test]
fn every_task_and_setting_domain_queues_with_null_head_and_claimed_ledger() {
    for profile in PROFILES.into_iter().filter(|p| *p != "desktopTerminal") {
        for intent in [
            json!({"kind": "upsertTask", "title": "Café"}),
            json!({"kind": "selectTask", "taskId": task()["id"]}),
            json!({"kind": "deleteTask", "taskId": task()["id"]}),
            json!({"kind": "setAutoStart", "enabled": true}),
        ] {
            let mut input = request(profile, intent);
            input["workspace"]["base"]["tasks"] = json!([task()]);
            input["workspace"]["canonicalHead"] = Value::Null;
            let output = plan(&input);
            assert_eq!(output["outcome"], "planned", "{profile}");
            for outcomes in output["groupOutcomes"].as_object().unwrap().values() {
                assert!(
                    outcomes
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|op| op["outcome"] == "queued")
                );
            }
            assert_retained(&input, &output);
        }
    }
}

#[test]
fn timer_commands_queue_after_claim_and_with_null_head_in_every_profile() {
    for profile in PROFILES {
        for (intent, status) in [
            ("pause", "running"),
            ("resume", "paused"),
            ("cancel", "running"),
            ("clear", "completed"),
            ("start", "idle"),
        ] {
            let mut input = request(profile, json!({"kind": intent}));
            if status != "idle" {
                input["workspace"]["base"]["canonicalTimer"] = fixture()["timer"].clone();
                input["workspace"]["base"]["canonicalTimer"]["status"] = json!(status);
                if status == "completed" {
                    input["workspace"]["base"]["canonicalTimer"]["elapsedAtAnchorMs"] =
                        json!(60000);
                }
            }
            if profile == "desktopTerminal" && intent == "cancel" {
                input["requestedTimer"] = input["workspace"]["base"]["canonicalTimer"].clone();
            }
            input["workspace"]["canonicalHead"] = Value::Null;
            let output = plan(&input);
            assert!(
                !output["commands"].as_array().unwrap().is_empty(),
                "{profile}/{intent}"
            );
            assert!(
                output["commandOutcomes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|op| op["outcome"] == "queued")
            );
            assert_retained(&input, &output);
        }
    }
}

#[test]
fn valid_timer_group_still_rejects_ignored_member_behind_claim() {
    let mut input = request("desktopStorage", json!({"kind": "cancel"}));
    input["workspace"]["base"]["canonicalTimer"] = fixture()["timer"].clone();
    let first = plan(&input);
    resume_request(&mut input, &first, 2);
    input["intent"] = json!({"kind": "selectTask", "taskId": null});
    input["ownership"] = json!({"ownerId": null, "expectedOwnerId": null});
    input["durability"] = json!({"outgoingDurationOperationIds": []});
    claim(&mut input, "commands");
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
}

#[test]
fn finish_queues_without_fabricating_safe_history_or_losing_completion_records() {
    for profile in PROFILES {
        let mut input = request(profile, json!({"kind": "pause"}));
        input.as_object_mut().unwrap().remove("intent");
        input["stage"] = json!("finishCommit");
        input["requestedTimer"] = fixture()["timer"].clone();
        input["workspace"]["base"]["canonicalTimer"] = input["requestedTimer"].clone();
        input["workspace"]["canonicalHead"] = Value::Null;
        input["ownership"] = Value::Null;
        input["identities"]["commandUuids"] = json!(["019f7f65-dd10-7000-8000-000000000001"]);
        let output = call("workspace.completionMutation.v1", &input);
        assert_eq!(output["outcome"], "planned", "{profile}");
        assert_eq!(output["commandOutcomes"][0]["outcome"], "queued");
        assert_eq!(output["projection"]["history"], json!([]));
        assert_eq!(output["projection"]["canonicalTimer"]["status"], "running");
        assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
        let restored = serde_json::from_str(&input.to_string()).unwrap();
        assert_eq!(output, call("workspace.completionMutation.v1", &restored));
    }
}

#[test]
fn pending_possibly_delivered_start_keeps_original_pause_entrypoints() {
    for profile in PROFILES {
        for null_head in [false, true] {
            let mut input = request(profile, json!({"kind": "start"}));
            let first = plan(&input);
            resume_request(&mut input, &first, 2);
            input["identities"]["commandUuids"] = json!(["019f7f65-dd10-7000-8000-000000000002"]);
            claim(&mut input, "commands");
            if null_head {
                input["workspace"]["canonicalHead"] = Value::Null;
            }
            input["intent"] = json!({"kind": "pause"});
            let output = plan(&input);
            let supported =
                profile == "androidCoordinator" || null_head && profile != "appleWorkspace";
            assert_eq!(
                output["outcome"],
                if supported { "planned" } else { "noop" },
                "{profile}/{null_head}"
            );
            assert!(output["projection"]["canonicalTimer"].is_null());
            if supported {
                assert_eq!(
                    output["commands"][0]["timerId"],
                    first["commands"][0]["timerId"]
                );
                assert_eq!(output["commandOutcomes"][0]["outcome"], "queued");
            }
            assert_retained(&input, &output);
        }
    }
}

#[test]
fn claimed_durations_are_not_coalesced_or_given_new_proof() {
    for profile in PROFILES.into_iter().filter(|p| *p != "desktopTerminal") {
        let intent = if profile == "androidCoordinator" {
            json!({"kind": "changeDuration", "phase": "focus", "delta": 1})
        } else {
            json!({"kind": "setDuration", "phase": "focus", "minutes": 2})
        };
        let mut input = request(profile, intent);
        if matches!(profile, "appleWorkspace" | "androidCoordinator") {
            input["localDurationsMs"] = input["workspace"]["base"]["durationsMs"].clone();
        }
        let first = plan(&input);
        resume_request(&mut input, &first, 2);
        claim(&mut input, "durationOperations");
        input["durability"]["outgoingDurationOperationIds"] =
            json!([first["operations"]["durationOperations"][0]["id"]]);
        if profile != "androidCoordinator" {
            input["intent"]["minutes"] = json!(3);
        }
        let output = plan(&input);
        assert_eq!(output["retiredDurationOperationIds"], json!([]));
        assert_eq!(
            output["groupOutcomes"]["durationOperations"][0]["outcome"],
            "queued"
        );
        assert_retained(&input, &output);
    }
}

#[test]
fn stale_losing_and_invalid_groups_fail_atomically_even_if_safe_view_would_accept() {
    let mut input = request(
        "desktopStorage",
        json!({"kind": "setAutoStart", "enabled": true}),
    );
    input["workspace"]["local"]["autoStartOperations"] = json!([{
        "id": "future-auto", "deviceId": "device-local", "enabled": false,
        "occurredAt": "2026-07-20T12:00:20Z", "hlcWallMs": 1784548820000_i64, "hlcCounter": 0
    }]);
    let original = input.clone();
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    assert_eq!(input, original);
    for corrupt in ["owner", "dependency", "proof", "shape"] {
        let mut bad = request(
            "desktopStorage",
            json!({"kind": "selectTask", "taskId": null}),
        );
        match corrupt {
            "owner" => bad["ownership"]["ownerId"] = json!("different-owner"),
            "dependency" => {
                bad["workspace"]["timerDependencies"] =
                    json!([{"operationId": "missing", "dependsOnOperationId": "missing"}])
            }
            "proof" => bad["workspace"]["neverSent"]["commands"] = json!(["missing"]),
            _ => bad["workspace"]["local"]["commands"] = json!([{}]),
        }
        let original = bad.clone();
        assert!(
            dispatch_json("workspace.intent.v1", &bad.to_string()).is_err(),
            "{corrupt}"
        );
        assert_eq!(bad, original);
    }
}

#[test]
fn grouped_add_reports_queued_selection_when_safe_tasks_do_not_include_new_task() {
    for profile in ["desktopStorage", "androidCoordinator", "pwaStorage"] {
        let mut input = request(profile, json!({"kind": "upsertTask", "title": "Café"}));
        let first = plan(&input);
        resume_request(&mut input, &first, 2);
        claim(&mut input, "taskOperations");
        input["intent"] = json!({"kind": "addAndSelectTask", "title": "Durable task"});
        let output = plan(&input);
        assert_eq!(output["outcome"], "planned");
        assert_eq!(
            output["groupOutcomes"]["taskOperations"][0]["outcome"],
            "queued"
        );
        assert_eq!(
            output["groupOutcomes"]["selectedTaskOperations"][0]["outcome"],
            "queued"
        );
        // A selected-task operation can win its domain while its target task
        // is hidden. A winning ID alone is not an applied display assignment.
        assert_eq!(
            output["projection"]["winningOperationIds"]["selectedTask"],
            output["operations"]["selectedTaskOperations"][0]["id"]
        );
        assert!(output["projection"]["selectedTaskId"].is_null());
        assert_retained(&input, &output);
    }
}

#[test]
fn automatic_wrong_owner_does_not_spend_a_group_behind_claimed_work_and_null_head() {
    for profile in PROFILES {
        let mut input = request(profile, json!({"kind": "pause"}));
        input.as_object_mut().unwrap().remove("intent");
        input["stage"] = json!("automaticFinishCommit");
        input["requestedTimer"] = fixture()["timer"].clone();
        input["workspace"]["base"]["canonicalTimer"] = fixture()["timer"].clone();
        input["workspace"]["canonicalHead"] = Value::Null;
        input["workspace"]["local"]["commands"] = json!([{
            "id": "claimed-retarget", "deviceId": "device-local", "deviceSequence": 7,
            "timerId": "existing-timer", "type": "retarget", "phase": "focus",
            "plannedDurationMs": 60000, "observedElapsedMs": 5000, "taskId": null,
            "occurredAt": "2026-07-20T12:00:00Z", "hlcWallMs": 1784548800000_i64, "hlcCounter": 0
        }]);
        for field in ["occurredAt", "physicalNow", "observedAt"] {
            input["clock"][field] = json!("2026-07-20T12:00:55Z");
        }
        input["ownership"] = json!({"timerId": "existing-timer", "deviceId": "other-device"});
        input["identities"]["commandUuids"] = json!(["019f7f65-dd10-7000-8000-000000000001"]);
        if profile == "pwaStorage" {
            input["localTabId"] = json!("tab-a");
            input["leaseNowMs"] = json!(1784548855000_i64);
        }
        let output = call("workspace.completionMutation.v1", &input);
        assert_eq!(output["outcome"], "noop", "{profile}");
        assert_eq!(
            output["reason"],
            if profile == "pwaStorage" {
                "not_owner"
            } else {
                "notOwner"
            }
        );
        assert_eq!(output["workspace"], input["workspace"]);
        assert_eq!(output["allocation"], input["allocation"]);
        assert_eq!(output["commandOutcomes"], json!([]));
        assert_eq!(output["effectsAfterCommit"], json!([]));
    }
}

#[test]
fn storage_selection_accepts_a_task_established_only_by_claimed_upsert() {
    for profile in ["desktopStorage", "pwaStorage"] {
        let mut input = request(profile, json!({"kind": "upsertTask", "title": "Café"}));
        let first = plan(&input);
        resume_request(&mut input, &first, 2);
        claim(&mut input, "taskOperations");
        input["intent"] = json!({"kind": "selectTask", "taskId": task()["id"]});
        let output = plan(&input);
        assert_eq!(output["outcome"], "planned");
        assert_eq!(
            output["groupOutcomes"]["selectedTaskOperations"][0]["outcome"],
            "queued"
        );
        assert_eq!(output["projection"]["tasks"], json!([]));
        assert!(output["projection"]["selectedTaskId"].is_null());
        assert_retained(&input, &output);
    }
}

#[test]
fn null_head_pending_start_pause_uses_the_supported_live_monotonic_observation() {
    let mut input = request("pwaStorage", json!({"kind": "start"}));
    let first = plan(&input);
    resume_request(&mut input, &first, 2);
    claim(&mut input, "commands");
    input["workspace"]["canonicalHead"] = Value::Null;
    input["intent"] = json!({"kind": "pause"});
    input["clock"] = json!({"occurredAt": "2026-07-20T12:00:11Z", "observedAt": "2026-07-20T12:30:00Z",
        "physicalNow": "2026-07-20T12:30:00Z", "monotonicNowMs": 2000, "continuityId": "live-page"});
    input["observation"]["monotonicAnchor"] = json!({"timerId": first["commands"][0]["timerId"],
        "anchorAt": first["projection"]["canonicalTimer"]["anchorAt"], "elapsedAtAnchorMs": 0,
        "sampledTrustedNowMs": 1784548810000_i64, "sampledMonotonicMs": 1000, "continuityId": "live-page"});
    input["identities"]["commandUuids"] = json!(["019f7f65-e0f8-7000-8000-000000000001"]);
    let output = plan(&input);
    assert_eq!(output["outcome"], "planned");
    assert_eq!(output["commands"][0]["observedElapsedMs"], 1000);
    assert_eq!(output["commandOutcomes"][0]["outcome"], "queued");
    assert!(output["projection"]["canonicalTimer"].is_null());
    assert_retained(&input, &output);
}
