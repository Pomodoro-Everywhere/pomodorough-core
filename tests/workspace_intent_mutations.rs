use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn request(profile: &str, intent: Value) -> Value {
    let mut input: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let mut input = input["request"].take();
    input["compatibility"] = json!(profile);
    input["intent"] = intent;
    input["ownership"] = json!({"expectedOwnerId": "account-a", "ownerId": "account-a"});
    input["durability"] = json!({"outgoingDurationOperationIds": [], "localTabId": "tab-a"});
    if (profile == "appleWorkspace" && input["intent"]["kind"] == "setDuration")
        || input["intent"]["kind"] == "changeDuration"
    {
        input["localDurationsMs"] = input["workspace"]["base"]["durationsMs"].clone();
    }
    input
}

fn plan(input: &Value) -> Value {
    serde_json::from_str(
        &dispatch_json("workspace.intent.v1", &input.to_string())
            .unwrap_or_else(|error| panic!("{error}: {input}")),
    )
    .unwrap()
}

fn task(title: &str) -> Value {
    serde_json::from_str(
        &dispatch_json("task.identity.v1", &json!({"title": title}).to_string()).unwrap(),
    )
    .unwrap()
}

#[test]
fn shared_entrypoint_matrix_covers_active_focus_break_and_terminal() {
    let matrix: Value = serde_json::from_str(include_str!(
        "../fixtures/workspace-intent-mutations-v1.json"
    ))
    .unwrap();
    let identity = task("Café");
    for row in matrix["selection"].as_array().unwrap() {
        let profile = row["profile"].as_str().unwrap();
        let mut input = request(
            profile,
            json!({"kind": "selectTask", "taskId": identity["id"]}),
        );
        input["workspace"]["base"]["tasks"] =
            json!([{"id": identity["id"], "title": identity["title"]}]);
        let mut timer: Value =
            serde_json::from_str::<Value>(include_str!("../fixtures/workspace-intent-v1.json"))
                .unwrap()["timer"]
                .clone();
        timer["status"] = row["status"].clone();
        timer["phase"] = row["phase"].clone();
        if row["status"] == "completed" {
            timer["elapsedAtAnchorMs"] = json!(60000);
        }
        if row["phase"] != "focus" {
            timer["plannedDurationMs"] = json!(120000);
        }
        input["workspace"]["base"]["canonicalTimer"] = timer;
        let output = plan(&input);
        let commands: Vec<_> = output["operations"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value["type"].clone())
            .collect();
        assert_eq!(
            json!(commands),
            row["commands"],
            "{profile}/{}",
            row["phase"]
        );
        assert_eq!(
            output["operations"]["selectedTaskOperations"]
                .as_array()
                .unwrap()
                .len(),
            row["selected"].as_u64().unwrap() as usize
        );
    }
    for row in matrix["duration"].as_array().unwrap() {
        let profile = row["profile"].as_str().unwrap();
        let intent = if profile == "androidCoordinator" {
            json!({"kind": "changeDuration", "phase": "focus", "delta": 1})
        } else {
            json!({"kind": "setDuration", "phase": "focus", "minutes": 40})
        };
        let mut input = request(profile, intent);
        let mut timer: Value =
            serde_json::from_str::<Value>(include_str!("../fixtures/workspace-intent-v1.json"))
                .unwrap()["timer"]
                .clone();
        timer["status"] = row["status"].clone();
        if row["status"] == "completed" {
            timer["elapsedAtAnchorMs"] = json!(60000);
        }
        input["workspace"]["base"]["canonicalTimer"] = timer;
        let output = plan(&input);
        let commands: Vec<_> = output["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["type"].clone())
            .collect();
        assert_eq!(
            json!(commands),
            row["commands"],
            "{profile}/{}",
            row["status"]
        );
        assert_eq!(
            output["projection"]["durationsMs"]["focus"],
            row["duration"]
        );
        if profile == "appleWorkspace" && row["status"] == "completed" {
            assert_eq!(
                output["effectsAfterCommit"],
                json!([
                    {"kind": "launchSync"}, {"kind": "cancelAlarm", "timerId": "existing-timer"}
                ])
            );
        }
    }
}

#[test]
fn duplicate_task_title_selects_existing_without_upsert() {
    let identity = task("Café");
    let mut input = request(
        "androidCoordinator",
        json!({"kind": "addAndSelectTask", "title": "Cafe\u{301}"}),
    );
    input["workspace"]["base"]["tasks"] =
        json!([{"id": identity["id"], "title": identity["title"]}]);
    let output = plan(&input);
    assert_eq!(output["operations"]["taskOperations"], json!([]));
    assert_eq!(output["projection"]["selectedTaskId"], identity["id"]);
    assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
}

#[test]
fn android_existing_unicode_task_selects_or_retargets_while_new_active_task_stays_blocked() {
    let matrix: Value = serde_json::from_str(include_str!(
        "../fixtures/workspace-intent-mutations-v1.json"
    ))
    .unwrap();
    let identity = task("Café");
    let timer: Value =
        serde_json::from_str::<Value>(include_str!("../fixtures/workspace-intent-v1.json"))
            .unwrap()["timer"]
            .clone();
    for case in matrix["androidAdd"].as_array().unwrap() {
        let mut input = request(
            "androidCoordinator",
            json!({"kind": "addAndSelectTask", "title": case["title"]}),
        );
        if case["existing"] == true {
            input["workspace"]["base"]["tasks"] =
                json!([{"id": identity["id"], "title": identity["title"]}]);
        }
        if case["selected"] == true {
            input["workspace"]["base"]["selectedTaskId"] = identity["id"].clone();
        }
        if case["status"] != "idle" {
            let mut active = timer.clone();
            active["status"] = case["status"].clone();
            active["phase"] = case["phase"].clone();
            if case["timerTask"] == true {
                active["taskId"] = identity["id"].clone();
            }
            input["workspace"]["base"]["canonicalTimer"] = active;
        }
        let output = plan(&input);
        for (domain, expected) in [
            ("taskOperations", "tasks"),
            ("selectedTaskOperations", "selection"),
            ("commands", "commands"),
        ] {
            assert_eq!(
                output["operations"][domain].as_array().unwrap().len(),
                case[expected].as_u64().unwrap() as usize,
                "{case}/{domain}"
            );
        }
        if case["existing"] == true {
            assert_eq!(
                output["projection"]["selectedTaskId"], identity["id"],
                "{case}"
            );
        }
        if case["commands"] == 1 {
            assert_eq!(output["commands"][0]["type"], "retarget", "{case}");
            assert_eq!(output["commands"][0]["taskId"], identity["id"], "{case}");
        }
    }
}

#[test]
fn android_duration_delta_uses_local_setting_even_when_projection_differs() {
    let matrix: Value = serde_json::from_str(include_str!(
        "../fixtures/workspace-intent-mutations-v1.json"
    ))
    .unwrap();
    for case in matrix["androidDurationDivergence"].as_array().unwrap() {
        let mut input = request(
            "androidCoordinator",
            json!({"kind": "changeDuration", "phase": "focus", "delta": case["delta"]}),
        );
        input["localDurationsMs"]["focus"] = case["localMs"].clone();
        input["workspace"]["base"]["durationsMs"]["focus"] = case["projectedMs"].clone();
        let output = plan(&input);
        let operations = output["operations"]["durationOperations"]
            .as_array()
            .unwrap();
        if case["operationMs"].is_null() {
            assert!(operations.is_empty(), "{case}");
            assert_eq!(output["workspace"], input["workspace"], "{case}");
            assert_eq!(output["allocation"], input["allocation"], "{case}");
        } else {
            assert_eq!(operations.len(), 1, "{case}");
            assert_eq!(operations[0]["durationMs"], case["operationMs"], "{case}");
            assert_eq!(
                output["groupOutcomes"]["durationOperations"][0]["outcome"], "applied",
                "{case}"
            );
        }
    }
}

#[test]
fn android_divergent_duration_supersession_preserves_outgoing_exact_retry() {
    let mut input = request(
        "androidCoordinator",
        json!({"kind": "changeDuration", "phase": "focus", "delta": 1}),
    );
    let first = plan(&input);
    let old = first["operations"]["durationOperations"][0].clone();
    input["workspace"] = first["workspace"].clone();
    input["allocation"] = first["allocation"].clone();
    input["identities"]["commandUuids"] = json!(["019f7f65-dd10-7000-8000-000000000002"]);
    // Settings read stays at 60k while queued local projection is already 120k.
    assert_eq!(input["localDurationsMs"]["focus"], 60000);
    assert_eq!(first["projection"]["durationsMs"]["focus"], 120000);
    input["durability"]["outgoingDurationOperationIds"] = json!([old["id"]]);
    assert!(
        dispatch_json("workspace.intent.v1", &input.to_string())
            .unwrap_err()
            .to_string()
            .contains("possibly-sent duration")
    );
    assert_eq!(input["workspace"]["local"]["durationOperations"][0], old);
    input["durability"]["outgoingDurationOperationIds"] = json!([]);
    let next = plan(&input);
    assert_eq!(next["retiredDurationOperationIds"], json!([old["id"]]));
    assert_eq!(
        next["operations"]["durationOperations"][0]["durationMs"],
        120000
    );
    assert_eq!(
        next["workspace"]["local"]["durationOperations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn android_repair_does_not_change_other_add_and_duration_profiles() {
    let timer: Value =
        serde_json::from_str::<Value>(include_str!("../fixtures/workspace-intent-v1.json"))
            .unwrap()["timer"]
            .clone();
    for profile in ["desktopStorage", "pwaStorage"] {
        let mut input = request(
            profile,
            json!({"kind": "addAndSelectTask", "title": "Cafe\u{301}"}),
        );
        input["workspace"]["base"]["canonicalTimer"] = timer.clone();
        input["identities"]["commandUuids"] = json!([
            "019f7f65-dd10-7000-8000-000000000001",
            "019f7f65-dd10-7000-8000-000000000002",
            "019f7f65-dd10-7000-8000-000000000003"
        ]);
        let output = plan(&input);
        assert_eq!(
            output["operations"]["taskOperations"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{profile}"
        );
        assert_eq!(
            output["operations"]["selectedTaskOperations"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{profile}"
        );
        assert_eq!(output["commands"][0]["type"], "retarget", "{profile}");
    }
    let mut apple = request(
        "appleWorkspace",
        json!({"kind": "setDuration", "phase": "focus", "minutes": 2}),
    );
    apple["localDurationsMs"]["focus"] = json!(120000);
    assert_eq!(plan(&apple)["outcome"], "noop");
}

#[test]
fn five_queue_task_select_and_active_focus_retarget_are_atomic() {
    let identity = task("Café");
    let mut input = request(
        "appleWorkspace",
        json!({"kind": "upsertTask", "title": "Cafe\u{301}"}),
    );
    let upsert = plan(&input);
    assert_eq!(
        upsert["operations"]["taskOperations"][0]["taskId"],
        identity["id"]
    );
    assert_eq!(
        upsert["operations"]["taskOperations"][0]["title"],
        identity["title"]
    );
    assert_eq!(upsert["projection"]["tasks"][0]["id"], identity["id"]);
    input["workspace"] = upsert["workspace"].clone();
    input["allocation"] = upsert["allocation"].clone();
    input["identities"]["commandUuids"] = json!([
        "019f7f65-dd10-7000-8000-000000000002",
        "019f7f65-dd10-7000-8000-000000000003"
    ]);
    input["intent"] = json!({"kind": "selectTask", "taskId": identity["id"]});
    input["workspace"]["base"]["canonicalTimer"] =
        serde_json::from_str::<Value>(include_str!("../fixtures/workspace-intent-v1.json"))
            .unwrap()["timer"]
            .clone();
    let selected = plan(&input);
    assert_eq!(
        selected["operations"]["selectedTaskOperations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(selected["operations"]["commands"][0]["type"], "retarget");
    assert_eq!(
        selected["operations"]["commands"][0]["taskId"],
        identity["id"]
    );
    assert_eq!(
        selected["projection"]["canonicalTimer"]["taskId"],
        identity["id"]
    );
    assert_eq!(selected["projection"]["selectedTaskId"], identity["id"]);
    assert_eq!(
        selected["atomicOperationIds"]["commands"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        selected["atomicOperationIds"]["selectedTaskOperations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        selected["workspace"]["local"]["taskOperations"],
        upsert["workspace"]["local"]["taskOperations"]
    );
    let retarget = &selected["operations"]["commands"][0];
    assert_eq!(
        retarget["id"],
        "command-019f7f65-dd10-7000-8000-000000000003"
    );
    assert_eq!(retarget["occurredAt"], input["clock"]["occurredAt"]);
    assert_eq!(retarget["deviceSequence"], 8);
    assert_eq!(retarget["hlcWallMs"], 1784548810000_i64);
    assert_eq!(retarget["hlcCounter"], 2);
    assert_eq!(selected["selection"], input["selection"]);
}

#[test]
fn delete_selected_task_deselects_in_same_plan_without_touching_break() {
    let identity = task("Café");
    let mut input = request(
        "androidCoordinator",
        json!({"kind": "deleteTask", "taskId": identity["id"]}),
    );
    input["workspace"]["base"]["tasks"] =
        json!([{"id": identity["id"], "title": identity["title"]}]);
    input["workspace"]["base"]["selectedTaskId"] = identity["id"].clone();
    input["workspace"]["base"]["canonicalTimer"] = json!({"id": "timer-break", "phase": "short_break", "status": "paused", "plannedDurationMs": 120000, "elapsedAtAnchorMs": 5000, "anchorAt": "2026-07-20T12:00:00Z"});
    let output = plan(&input);
    assert_eq!(output["projection"]["tasks"], json!([]));
    assert!(output["projection"]["selectedTaskId"].is_null());
    assert_eq!(
        output["operations"]["selectedTaskOperations"][0]["taskId"],
        Value::Null
    );
    assert_eq!(output["operations"]["commands"], json!([]));
    assert_eq!(output["projection"]["canonicalTimer"]["id"], "timer-break");
}

#[test]
fn preference_duration_and_auto_start_use_raw_workspace() {
    let mut input = request(
        "appleWorkspace",
        json!({"kind": "setDuration", "phase": "focus", "minutes": 181}),
    );
    let duration = plan(&input);
    assert_eq!(
        duration["operations"]["durationOperations"][0]["durationMs"],
        10_800_000
    );
    assert_eq!(duration["projection"]["durationsMs"]["focus"], 10_800_000);
    input["intent"] = json!({"kind": "setAutoStart", "enabled": true});
    input.as_object_mut().unwrap().remove("localDurationsMs");
    let auto = plan(&input);
    assert_eq!(auto["projection"]["autoStartBreaks"], true);
    assert_eq!(
        auto["operations"]["autoStartOperations"][0]["enabled"],
        true
    );
    assert_eq!(auto["operations"]["durationOperations"], json!([]));
}

#[test]
fn stale_account_and_outgoing_duration_supersession_fail_closed() {
    let mut input = request(
        "appleWorkspace",
        json!({"kind": "setDuration", "phase": "focus", "minutes": 40}),
    );
    input["ownership"]["ownerId"] = json!("account-b");
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["ownership"]["ownerId"] = json!("account-a");
    let first = plan(&input);
    input["workspace"] = first["workspace"].clone();
    input["allocation"] = first["allocation"].clone();
    input["identities"]["commandUuids"] = json!(["019f7f65-dd10-7000-8000-000000000002"]);
    input["intent"]["minutes"] = json!(41);
    let id = first["operations"]["durationOperations"][0]["id"].clone();
    input["durability"]["outgoingDurationOperationIds"] = json!([id]);
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["durability"]["outgoingDurationOperationIds"] = json!([]);
    let next = plan(&input);
    assert_eq!(
        next["workspace"]["local"]["durationOperations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(next["retiredDurationOperationIds"], json!([id]));
    assert_eq!(next["projection"]["durationsMs"]["focus"], 2_460_000);
}

#[test]
fn frozen_duration_without_proof_is_retained_beside_new_intent() {
    let mut input = request(
        "desktopStorage",
        json!({"kind": "setDuration", "phase": "focus", "minutes": 41}),
    );
    let first = plan(&input);
    input["workspace"] = first["workspace"].clone();
    input["allocation"] = first["allocation"].clone();
    input["identities"]["commandUuids"] = json!(["019f7f65-dd10-7000-8000-000000000002"]);
    input["workspace"]["neverSent"]["durationOperations"] = json!([]);
    input["intent"]["minutes"] = json!(42);
    let output = plan(&input);
    assert_eq!(
        output["workspace"]["local"]["durationOperations"][0],
        first["operations"]["durationOperations"][0]
    );
    assert_eq!(output["retiredDurationOperationIds"], json!([]));
    assert_eq!(
        output["groupOutcomes"]["durationOperations"][0]["outcome"],
        "queued"
    );
}

#[test]
fn pwa_duration_supersession_requires_same_tab_and_proof() {
    let mut input = request(
        "pwaStorage",
        json!({"kind": "setDuration", "phase": "focus", "minutes": 41}),
    );
    let first = plan(&input);
    let original = first["operations"]["durationOperations"][0].clone();
    assert_eq!(original["ownerId"], "tab-a");
    input["workspace"] = first["workspace"].clone();
    input["allocation"] = first["allocation"].clone();
    input["identities"]["commandUuids"] = json!(["019f7f65-dd10-7000-8000-000000000002"]);
    input["intent"]["minutes"] = json!(42);
    input["durability"]["localTabId"] = json!("tab-b");
    let foreign_tab = plan(&input);
    assert_eq!(
        foreign_tab["workspace"]["local"]["durationOperations"][0],
        original
    );
    assert_eq!(foreign_tab["retiredDurationOperationIds"], json!([]));
    input["durability"]["localTabId"] = json!("tab-a");
    let next = plan(&input);
    assert_eq!(next["retiredDurationOperationIds"], json!([original["id"]]));
    assert_eq!(
        next["workspace"]["local"]["durationOperations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    input.as_object_mut().unwrap().remove("durability");
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
}

#[test]
fn full_five_queue_projection_keeps_frozen_wire_extensions_exact() {
    let source: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-projection-v1.json")).unwrap();
    let mut input = request(
        "appleWorkspace",
        json!({"kind": "setAutoStart", "enabled": false}),
    );
    input["workspace"] = source["request"].clone();
    input["workspace"].as_object_mut().unwrap().remove("now");
    input["allocation"]["hlc"]["counter"] = json!(11);
    let output = plan(&input);
    for name in [
        "commands",
        "taskOperations",
        "durationOperations",
        "selectedTaskOperations",
    ] {
        assert_eq!(
            output["workspace"]["local"][name], input["workspace"]["local"][name],
            "{name}"
        );
    }
    assert_eq!(
        output["workspace"]["local"]["commands"][0]["extension"],
        json!({"keep": null})
    );
    assert_eq!(
        output["workspace"]["local"]["autoStartOperations"][0],
        input["workspace"]["local"]["autoStartOperations"][0]
    );
    assert_eq!(output["projection"]["autoStartBreaks"], false);
    assert_eq!(
        output["groupOutcomes"]["autoStartOperations"][0]["outcome"],
        "applied"
    );
}

#[test]
fn apple_auto_start_queues_behind_frozen_same_domain_without_faking_projection() {
    let mut input = request(
        "appleWorkspace",
        json!({"kind": "setAutoStart", "enabled": true}),
    );
    input["workspace"]["local"]["autoStartOperations"] = json!([{
        "id": "frozen-auto", "deviceId": "device-local", "enabled": false,
        "occurredAt": "2026-07-20T12:00:00Z", "hlcWallMs": 1784548800000_i64,
        "hlcCounter": 1, "extension": {"retry": "exact"}
    }]);
    input["allocation"]["hlc"]["counter"] = json!(1);
    let output = plan(&input);
    assert_eq!(output["outcome"], "planned");
    assert_eq!(
        output["groupOutcomes"]["autoStartOperations"][0]["outcome"],
        "queued"
    );
    assert_eq!(output["projection"]["autoStartBreaks"], false);
    assert_eq!(
        output["workspace"]["local"]["autoStartOperations"][0],
        input["workspace"]["local"]["autoStartOperations"][0]
    );
    assert_eq!(
        output["effectsAfterCommit"],
        json!([{"kind": "launchSync"}])
    );
}

#[test]
fn pwa_add_and_select_active_focus_requires_three_identities_and_tab_for_duration() {
    let mut input = request(
        "pwaStorage",
        json!({"kind": "addAndSelectTask", "title": "Café"}),
    );
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    input["workspace"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    input["identities"]["commandUuids"] = json!([
        "019f7f65-dd10-7000-8000-000000000001",
        "019f7f65-dd10-7000-8000-000000000002",
        "019f7f65-dd10-7000-8000-000000000003"
    ]);
    let output = plan(&input);
    assert_eq!(
        output["groupOutcomes"]["taskOperations"][0]["outcome"],
        "applied"
    );
    assert_eq!(
        output["groupOutcomes"]["selectedTaskOperations"][0]["outcome"],
        "applied"
    );
    assert_eq!(output["groupOutcomes"]["commands"][0]["outcome"], "applied");
    assert_eq!(output["allocation"]["deviceSequence"], 8);
    input["intent"] = json!({"kind": "setDuration", "phase": "focus", "minutes": 40});
    input["durability"]["localTabId"] = Value::Null;
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
}

#[test]
fn mutation_boundary_rejects_unknown_owner_outgoing_and_invalid_ranges() {
    let mut input = request(
        "desktopStorage",
        json!({"kind": "setDuration", "phase": "focus", "minutes": 0}),
    );
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["intent"]["minutes"] = json!(40);
    input["durability"]["outgoingDurationOperationIds"] = json!(["missing"]);
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["durability"]["outgoingDurationOperationIds"] = json!([]);
    input.as_object_mut().unwrap().remove("ownership");
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    let mut input = request(
        "appleWorkspace",
        json!({"kind": "upsertTask", "title": "\n\t"}),
    );
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["intent"]["title"] = json!("A".repeat(513));
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    let mut input = request("pwaStorage", json!({"kind": "selectTask"}));
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["intent"]["taskId"] = Value::Null;
    assert_eq!(plan(&input)["outcome"], "noop");
    input["ownership"]
        .as_object_mut()
        .unwrap()
        .remove("ownerId");
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    let input = request(
        "androidCoordinator",
        json!({"kind": "changeDuration", "phase": "focus", "delta": i64::MAX}),
    );
    assert_eq!(
        plan(&input)["projection"]["durationsMs"]["focus"],
        10_800_000
    );
}

#[test]
fn local_duration_settings_and_desktop_repeat_preserve_entrypoint_decisions() {
    let mut apple = request(
        "appleWorkspace",
        json!({"kind": "setDuration", "phase": "focus", "minutes": 40}),
    );
    apple["localDurationsMs"]["focus"] = json!(2_400_000);
    assert_eq!(plan(&apple)["outcome"], "noop");
    apple.as_object_mut().unwrap().remove("localDurationsMs");
    assert!(dispatch_json("workspace.intent.v1", &apple.to_string()).is_err());

    let mut android = request(
        "androidCoordinator",
        json!({"kind": "changeDuration", "phase": "focus", "delta": 1}),
    );
    android["localDurationsMs"]["focus"] = json!(2_400_000);
    assert_eq!(
        plan(&android)["projection"]["durationsMs"]["focus"],
        2_460_000
    );

    let duration = request(
        "desktopStorage",
        json!({"kind": "setDuration", "phase": "focus", "minutes": 1}),
    );
    assert_eq!(
        plan(&duration)["operations"]["durationOperations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let auto_start = request(
        "desktopStorage",
        json!({"kind": "setAutoStart", "enabled": false}),
    );
    assert_eq!(
        plan(&auto_start)["operations"]["autoStartOperations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn active_focus_delete_profile_divergences_are_explicit() {
    let identity = task("Café");
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    for (profile, expected_selected, expected_commands) in [
        ("appleWorkspace", 1, 0),
        ("androidCoordinator", 1, 0),
        ("desktopStorage", 1, 1),
        ("pwaStorage", 0, 0),
    ] {
        let mut input = request(
            profile,
            json!({"kind": "deleteTask", "taskId": identity["id"]}),
        );
        input["identities"]["commandUuids"] = json!([
            "019f7f65-dd10-7000-8000-000000000001",
            "019f7f65-dd10-7000-8000-000000000002",
            "019f7f65-dd10-7000-8000-000000000003"
        ]);
        input["workspace"]["base"]["tasks"] =
            json!([{"id": identity["id"], "title": identity["title"]}]);
        input["workspace"]["base"]["selectedTaskId"] = identity["id"].clone();
        input["workspace"]["base"]["canonicalTimer"] = fixture["timer"].clone();
        input["workspace"]["base"]["canonicalTimer"]["taskId"] = identity["id"].clone();
        let output = plan(&input);
        assert_eq!(
            output["operations"]["selectedTaskOperations"]
                .as_array()
                .unwrap()
                .len(),
            expected_selected,
            "{profile}"
        );
        assert_eq!(
            output["commands"].as_array().unwrap().len(),
            expected_commands,
            "{profile}"
        );
        assert!(
            output["projection"]["selectedTaskId"].is_null(),
            "{profile}"
        );
        assert_eq!(
            output["projection"]["canonicalTimer"]["taskId"],
            if profile == "desktopStorage" {
                Value::Null
            } else {
                identity["id"].clone()
            },
            "{profile}"
        );
    }
}

#[test]
fn durable_payloads_match_profile_serializers_without_rewriting_wire_projection() {
    let identity = task("Café");
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    for profile in [
        "appleWorkspace",
        "androidCoordinator",
        "desktopStorage",
        "pwaStorage",
    ] {
        let mut input = request(
            profile,
            json!({"kind": "selectTask", "taskId": identity["id"]}),
        );
        input["workspace"]["base"]["tasks"] =
            json!([{"id": identity["id"], "title": identity["title"]}]);
        input["workspace"]["base"]["canonicalTimer"] = fixture["timer"].clone();
        let output = plan(&input);
        let wire = &output["operations"];
        let durable = &output["durableOperations"];
        assert_eq!(wire["commands"][0]["deviceId"], "device-local");
        assert_eq!(
            wire["selectedTaskOperations"][0]["deviceId"],
            "device-local"
        );
        let command_device = durable["commands"][0].get("deviceId");
        let selected_device = durable["selectedTaskOperations"][0].get("deviceId");
        assert_eq!(
            command_device.is_some(),
            profile == "pwaStorage",
            "{profile}"
        );
        assert_eq!(
            selected_device.is_some(),
            profile != "androidCoordinator",
            "{profile}"
        );
        assert_eq!(durable["commands"][0]["id"], wire["commands"][0]["id"]);
        assert_eq!(
            durable["commands"][0]["occurredAt"],
            wire["commands"][0]["occurredAt"]
        );
        assert_eq!(
            durable["commands"][0]["hlcWallMs"],
            wire["commands"][0]["hlcWallMs"]
        );
    }
}
