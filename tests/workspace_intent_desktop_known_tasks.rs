use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../fixtures/workspace-intent-desktop-known-tasks-v1.json"
    ))
    .unwrap()
}

fn request() -> Value {
    let source: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let mut input = source["request"].clone();
    input["compatibility"] = json!("desktopStorage");
    input["intent"] = json!({"kind": "deleteTask", "taskId": fixture()["knownTasks"][0]["id"]});
    input["ownership"] = json!({"ownerId": "account-a", "expectedOwnerId": "account-a"});
    input["durability"] = json!({"outgoingDurationOperationIds": []});
    input["knownTasks"] = fixture()["knownTasks"].clone();
    input["identities"]["commandUuids"] = json!([
        "019f7f65-dd10-7000-8000-000000000001",
        "019f7f65-dd10-7000-8000-000000000002",
        "019f7f65-dd10-7000-8000-000000000003"
    ]);
    input
}

fn plan(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("workspace.intent.v1", &input.to_string()).unwrap())
        .unwrap()
}

fn case_request(case: &Value) -> Value {
    let mut input = request();
    let task = input["knownTasks"][0].clone();
    if case["current"] == true {
        input["workspace"]["base"]["tasks"] = json!([task]);
    }
    if case["selected"] == true {
        input["workspace"]["base"]["selectedTaskId"] = task["id"].clone();
    }
    if case["status"] != "idle" {
        let source: Value =
            serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
        let mut timer = source["timer"].clone();
        timer["status"] = case["status"].clone();
        timer["phase"] = case["phase"].clone();
        timer["taskId"] = task["id"].clone();
        if case["phase"] != "focus" {
            timer["plannedDurationMs"] = json!(120000);
            timer["taskId"] = Value::Null;
        }
        if case["status"] == "completed" {
            timer["elapsedAtAnchorMs"] = json!(60000);
        }
        input["workspace"]["base"]["canonicalTimer"] = timer;
    }
    if let Some(known) = case.get("knownTasks") {
        input["knownTasks"] = known.clone();
    }
    if let Some(id) = case.get("taskId") {
        input["intent"]["taskId"] = id.clone();
    }
    if let Some(history) = case.get("history") {
        input["workspace"]["base"]["history"] = history.clone();
    }
    input
}

#[test]
fn shared_desktop_cache_matrix_preserves_canonical_base_and_atomic_groups() {
    for case in fixture()["cases"].as_array().unwrap() {
        let input = case_request(case);
        let output = plan(&input);
        assert_eq!(
            output["workspace"]["base"], input["workspace"]["base"],
            "{case}"
        );
        assert_eq!(output["selection"], input["selection"], "{case}");
        assert_groups(case, &output);
        if case["taskOperations"] == 0 {
            assert_noop(&input, &output);
        } else {
            assert_eq!(output["outcome"], "planned");
            assert_eq!(
                output["effectsAfterCommit"],
                json!([{"kind": "launchSync"}])
            );
            assert_eq!(output["operations"]["taskOperations"][0]["type"], "delete");
            assert!(
                output["operations"]["taskOperations"][0]
                    .get("title")
                    .is_none()
            );
            assert_eq!(output["projection"]["tasks"], json!([]));
        }
    }
}

fn assert_groups(case: &Value, output: &Value) {
    for domain in ["taskOperations", "selectedTaskOperations", "commands"] {
        let operations = output["operations"][domain].as_array().unwrap();
        assert_eq!(
            operations.len(),
            case[domain].as_u64().unwrap() as usize,
            "{case}"
        );
        let ids: Vec<_> = operations
            .iter()
            .map(|operation| &operation["id"])
            .collect();
        assert_eq!(output["atomicOperationIds"][domain], json!(ids), "{case}");
        assert!(
            output["groupOutcomes"][domain]
                .as_array()
                .unwrap()
                .iter()
                .all(|outcome| outcome["outcome"] == "applied")
        );
    }
}

fn assert_queued_case(name: &str, output: &Value) {
    let fixture = fixture();
    let case = fixture["queuedCases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap();
    for (index, domain) in case["domains"].as_array().unwrap().iter().enumerate() {
        let domain = domain.as_str().unwrap();
        let op = &output["operations"][domain][0];
        assert!(
            op["id"]
                .as_str()
                .unwrap()
                .ends_with(case["uuidSuffixes"][index].as_str().unwrap())
        );
        assert_eq!(op["hlcCounter"], case["hlcCounters"][index]);
        assert_eq!(
            output["groupOutcomes"][domain][0]["outcome"],
            case["outcomes"][index]
        );
    }
}

fn assert_noop(input: &Value, output: &Value) {
    assert_eq!(output["outcome"], "noop");
    for field in ["workspace", "allocation", "selection", "observation"] {
        assert_eq!(output[field], input[field], "{field}");
    }
    for field in [
        "commands",
        "atomicCommandIds",
        "ownershipWrites",
        "effectsAfterCommit",
        "retiredDurationOperationIds",
    ] {
        assert_eq!(output[field], json!([]), "{field}");
    }
    for field in [
        "operations",
        "durableOperations",
        "atomicOperationIds",
        "groupOutcomes",
    ] {
        assert!(
            output[field]
                .as_object()
                .unwrap()
                .values()
                .all(|queue| queue == &json!([]))
        );
    }
}

#[test]
fn known_cache_records_validate_identity_shape_and_duplicate_normalized_titles() {
    let task = fixture()["knownTasks"][0].clone();
    let invalid = [
        Value::Null,
        json!({}),
        json!([null]),
        json!([{}]),
        json!([{"id": task["id"]}]),
        json!([{"id": task["id"], "title": 1}]),
        json!([{"id": "wrong", "title": task["title"]}]),
        json!([{"id": task["id"], "title": "\n\t"}]),
        json!([{"id": task["id"], "title": "x".repeat(513)}]),
        json!([task, {"id": task["id"], "title": "Cafe\u{301}"}]),
        json!([task, {"id": "unrelated", "title": "Another task"}]),
    ];
    for cache in invalid {
        let mut input = request();
        input["knownTasks"] = cache.clone();
        assert!(
            dispatch_json("workspace.intent.v1", &input.to_string()).is_err(),
            "{cache}"
        );
    }
}

#[test]
fn non_desktop_profiles_reject_known_tasks_even_null_and_empty() {
    for profile in ["appleWorkspace", "androidCoordinator", "pwaStorage"] {
        for cache in [json!([]), Value::Null, fixture()["knownTasks"].clone()] {
            let mut input = request();
            input["compatibility"] = json!(profile);
            input["knownTasks"] = cache;
            assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
        }
    }
}

#[test]
fn cache_is_not_selection_or_upsert_authority_and_omission_preserves_legacy_admission() {
    let mut input = request();
    input.as_object_mut().unwrap().remove("knownTasks");
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["knownTasks"] = fixture()["knownTasks"].clone();
    input["intent"]["kind"] = json!("selectTask");
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["intent"] = json!({"kind": "addAndSelectTask", "title": "Cafe\u{301}"});
    let output = plan(&input);
    assert_eq!(output["operations"]["taskOperations"][0]["type"], "upsert");
    assert_eq!(output["operations"]["taskOperations"][0]["title"], "Café");
    input["workspace"]["base"]["tasks"] = fixture()["knownTasks"].clone();
    assert_eq!(plan(&input)["operations"]["taskOperations"], json!([]));
}

#[test]
fn repeated_deleted_groups_survive_json_restart_and_keep_every_retained_payload() {
    let mut input = request();
    let first = plan(&input);
    input["workspace"] = first["workspace"].clone();
    input["allocation"] = first["allocation"].clone();
    input["identities"]["commandUuids"] = json!(["019f7f65-dd10-7000-8000-000000000002"]);
    let retained = input["workspace"]["local"]["taskOperations"][0]
        .as_object_mut()
        .unwrap();
    retained.insert("extension".into(), json!({"exactRetry": null}));
    let restarted: Value = serde_json::from_str(&input.to_string()).unwrap();
    let second = plan(&restarted);
    let queued = second["workspace"]["local"]["taskOperations"]
        .as_array()
        .unwrap();
    assert_eq!(queued.len(), 2);
    assert_eq!(
        queued[0],
        restarted["workspace"]["local"]["taskOperations"][0]
    );
    assert_eq!(queued[1], second["operations"]["taskOperations"][0]);
    assert_eq!(second["projection"]["tasks"], json!([]));
    assert_eq!(second["workspace"]["base"], request()["workspace"]["base"]);
    input["workspace"]["neverSent"]["taskOperations"] = json!([]);
    let queued_after_claim = plan(&input);
    assert_queued_case("cacheClaimRestart", &queued_after_claim);
    assert_eq!(
        queued_after_claim["workspace"]["local"]["taskOperations"][0],
        queued[0]
    );
    assert_eq!(
        queued_after_claim["groupOutcomes"]["taskOperations"][0]["outcome"],
        "queued"
    );
    assert_eq!(
        queued_after_claim["workspace"]["neverSent"]["taskOperations"],
        json!([queued_after_claim["operations"]["taskOperations"][0]["id"]])
    );
}

#[test]
fn historical_task_attribution_does_not_resurrect_multiple_deleted_tasks() {
    let mut input = request();
    let mut history =
        serde_json::from_str::<Value>(include_str!("../fixtures/workspace-intent-v1.json"))
            .unwrap()["retainedCompletion"]
            .clone();
    history["taskId"] = input["knownTasks"][0]["id"].clone();
    let mut another = history.clone();
    another["id"] = json!("other-history");
    another["timerId"] = json!("other-timer");
    input["workspace"]["base"]["history"] = json!([history, another]);
    let mut projected = input["workspace"].clone();
    projected["now"] = input["clock"]["physicalNow"].clone();
    let before: Value = serde_json::from_str(
        &dispatch_json("workspace.project.v1", &projected.to_string()).unwrap(),
    )
    .unwrap();
    let output = plan(&input);
    assert_eq!(
        output["projection"]["history"],
        before["workspace"]["history"]
    );
    assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
    assert_eq!(output["projection"]["tasks"], json!([]));
    assert_eq!(output["operations"]["selectedTaskOperations"], json!([]));
    assert_eq!(output["commands"], json!([]));
}

#[test]
fn desktop_timer_restart_accepts_raw_cache_without_promoting_it_to_canonical_tasks() {
    for profile in ["desktopStorage", "desktopTerminal"] {
        let mut input = request();
        input["compatibility"] = json!(profile);
        input["intent"] = json!({"kind": "restart"});
        input.as_object_mut().unwrap().remove("ownership");
        input.as_object_mut().unwrap().remove("durability");
        input["identities"]["commandUuids"]
            .as_array_mut()
            .unwrap()
            .pop();
        let source: Value =
            serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
        input["workspace"]["base"]["history"] = json!([source["retainedCompletion"]]);
        input["requestedTimer"] = source["timer"].clone();
        input["requestedTimer"]["status"] = json!("completed");
        input["requestedTimer"]["elapsedAtAnchorMs"] = json!(60000);
        let output = plan(&input);
        assert_eq!(output["commands"].as_array().unwrap().len(), 2);
        assert_eq!(output["commands"][0]["type"], "clear");
        assert_eq!(output["commands"][1]["type"], "start");
        assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
        assert_eq!(output["projection"]["tasks"], json!([]));
    }
}

#[test]
fn unrelated_possibly_sent_queues_remain_exact_during_cache_only_delete() {
    let mut input = request();
    let source: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-projection-v1.json")).unwrap();
    input["workspace"] = source["request"].clone();
    input["workspace"].as_object_mut().unwrap().remove("now");
    input["workspace"]["local"]["taskOperations"] = json!([]);
    input["workspace"]["neverSent"] = json!({});
    input["allocation"]["hlc"]["counter"] = json!(11);
    let output = plan(&input);
    for domain in [
        "commands",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ] {
        assert_eq!(
            output["workspace"]["local"][domain],
            input["workspace"]["local"][domain]
        );
        assert_eq!(output["operations"][domain], json!([]));
    }
    assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
    assert_eq!(output["operations"]["taskOperations"][0]["type"], "delete");
}

#[test]
fn selected_delete_cannot_return_partial_group_and_queues_behind_possibly_sent_retarget() {
    let case = fixture()["cases"][3].clone();
    let mut input = case_request(&case);
    input["identities"]["commandUuids"]
        .as_array_mut()
        .unwrap()
        .pop();
    let original = input.clone();
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    assert_eq!(input, original);
    input = case_request(&case);
    let first = plan(&input);
    input["workspace"] = first["workspace"].clone();
    input["allocation"] = first["allocation"].clone();
    input["observation"] = first["observation"].clone();
    input["workspace"]["neverSent"]["commands"] = json!([]);
    input["workspace"]["local"]["commands"][0]["extension"] = json!({"exactRetry": true});
    // Canonical selection still requires a new retarget while the old one is possibly sent.
    input["workspace"]["local"]["taskOperations"] = json!([]);
    input["workspace"]["neverSent"]["taskOperations"] = json!([]);
    input["workspace"]["local"]["selectedTaskOperations"] = json!([]);
    input["workspace"]["neverSent"]["selectedTaskOperations"] = json!([]);
    input["identities"]["commandUuids"] = json!([
        "019f7f65-dd10-7000-8000-000000000004",
        "019f7f65-dd10-7000-8000-000000000005",
        "019f7f65-dd10-7000-8000-000000000006"
    ]);
    let original = input.clone();
    let queued = plan(&input);
    assert_eq!(
        queued["workspace"]["local"]["commands"][0],
        original["workspace"]["local"]["commands"][0]
    );
    assert_eq!(queued["groupOutcomes"]["commands"][0]["outcome"], "queued");
    assert_eq!(
        queued["projection"]["canonicalTimer"]["taskId"],
        input["workspace"]["base"]["canonicalTimer"]["taskId"]
    );
    assert_eq!(input, original);
}

#[test]
fn absent_canonical_head_queues_valid_delete_without_optimistic_projection() {
    for current in [false, true] {
        let mut input = request();
        if current {
            input["workspace"]["base"]["tasks"] = fixture()["knownTasks"].clone();
        }
        input["workspace"]["canonicalHead"] = Value::Null;
        let queued = plan(&input);
        assert_queued_case("nullHead", &queued);
        assert_eq!(queued["outcome"], "planned");
        assert_eq!(
            queued["projection"]["tasks"],
            input["workspace"]["base"]["tasks"]
        );
        assert_eq!(
            queued["projection"]["winningOperationIds"]["tasks"],
            json!({})
        );
        assert_eq!(
            queued["groupOutcomes"]["taskOperations"][0]["outcome"],
            "queued"
        );
        assert_eq!(queued["workspace"]["canonicalHead"], Value::Null);
        assert_eq!(queued["workspace"]["base"], input["workspace"]["base"]);
    }
}

#[test]
fn claimed_selection_restart_delete_keeps_exact_three_member_group_and_safe_display() {
    let mut input = case_request(&fixture()["cases"][3]);
    input["intent"] = json!({"kind": "selectTask", "taskId": fixture()["knownTasks"][0]["id"]});
    let selected = plan(&input);
    input["workspace"] = selected["workspace"].clone();
    input["allocation"] = selected["allocation"].clone();
    input["observation"] = selected["observation"].clone();
    input["workspace"]["neverSent"] = json!({});
    input["intent"] = json!({"kind": "deleteTask", "taskId": fixture()["knownTasks"][0]["id"]});
    input["identities"]["commandUuids"] = json!([
        "019f7f65-dd10-7000-8000-000000000003",
        "019f7f65-dd10-7000-8000-000000000004",
        "019f7f65-dd10-7000-8000-000000000005"
    ]);
    let input: Value = serde_json::from_str(&input.to_string()).unwrap();
    let queued = plan(&input);
    assert_queued_case("selectedClaimRestart", &queued);
    for (domain, counter, suffix, outcome) in [
        ("taskOperations", 2, "003", "applied"),
        ("selectedTaskOperations", 3, "004", "queued"),
        ("commands", 4, "005", "queued"),
    ] {
        let op = &queued["operations"][domain][0];
        assert!(op["id"].as_str().unwrap().ends_with(suffix));
        assert_eq!(op["hlcCounter"], counter);
        assert_eq!(queued["groupOutcomes"][domain][0]["outcome"], outcome);
        assert_eq!(queued["workspace"]["neverSent"][domain], json!([op["id"]]));
        assert_eq!(
            queued["workspace"]["local"][domain].as_array().unwrap()[..input["workspace"]["local"]
                [domain]
                .as_array()
                .unwrap()
                .len()],
            input["workspace"]["local"][domain].as_array().unwrap()[..]
        );
    }
    assert_eq!(queued["commands"][0]["deviceSequence"], 9);
    assert_eq!(queued["commands"][0]["observedElapsedMs"], 15000);
    assert_eq!(
        queued["projection"]["canonicalTimer"]["taskId"],
        input["workspace"]["base"]["canonicalTimer"]["taskId"]
    );
    assert_eq!(queued["workspace"]["base"], input["workspace"]["base"]);
}

#[test]
fn queue_only_delete_keeps_malformed_causal_ownership_and_identity_boundaries() {
    let mut input = request();
    input["workspace"]["canonicalHead"] = Value::Null;
    for corrupt in ["owner", "dependencies", "proof", "allocation", "cache"] {
        let mut invalid = input.clone();
        match corrupt {
            "owner" => invalid["ownership"]["ownerId"] = json!("different-account"),
            "dependencies" => {
                invalid["workspace"]["timerDependencies"] = json!([
                {"operationId": "missing", "dependsOnOperationId": "missing-parent"}])
            }
            "proof" => invalid["workspace"]["neverSent"] = json!({"taskOperations": ["missing"]}),
            "allocation" => invalid["identities"]["commandUuids"] = json!([]),
            _ => invalid["knownTasks"][0]["id"] = json!("forged-task"),
        }
        let original = invalid.clone();
        assert!(
            dispatch_json("workspace.intent.v1", &invalid.to_string()).is_err(),
            "{corrupt}"
        );
        assert_eq!(invalid, original);
    }
}

#[test]
fn shared_admission_extends_valid_groups_without_promoting_known_cache() {
    for profile in [
        "appleWorkspace",
        "androidCoordinator",
        "pwaStorage",
        "desktopStorage",
    ] {
        let mut input = request();
        input["compatibility"] = json!(profile);
        input.as_object_mut().unwrap().remove("knownTasks");
        input["workspace"]["base"]["tasks"] = fixture()["knownTasks"].clone();
        input["workspace"]["canonicalHead"] = Value::Null;
        if profile == "desktopStorage" {
            input["intent"] = json!({"kind": "upsertTask", "title": "Café"});
        }
        let output = plan(&input);
        assert_eq!(output["outcome"], "planned", "{profile}");
        assert_eq!(
            output["groupOutcomes"]["taskOperations"][0]["outcome"],
            "queued"
        );
        assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
    }
}

#[test]
fn claimed_cancel_causally_blocks_new_retarget_despite_running_safe_display() {
    let mut input = case_request(&fixture()["cases"][3]);
    let mut cancel = input.clone();
    cancel["intent"] = json!({"kind": "cancel"});
    cancel.as_object_mut().unwrap().remove("ownership");
    cancel.as_object_mut().unwrap().remove("durability");
    cancel["identities"]["commandUuids"]
        .as_array_mut()
        .unwrap()
        .pop();
    let cancelled = plan(&cancel);
    input["workspace"] = cancelled["workspace"].clone();
    input["workspace"]["neverSent"]["commands"] = json!([]);
    input["allocation"] = cancelled["allocation"].clone();
    input["observation"] = cancelled["observation"].clone();
    input["identities"]["commandUuids"] = json!([
        "019f7f65-dd10-7000-8000-000000000002",
        "019f7f65-dd10-7000-8000-000000000003",
        "019f7f65-dd10-7000-8000-000000000004"
    ]);
    let mut projection = input["workspace"].clone();
    projection["now"] = input["clock"]["physicalNow"].clone();
    let displayed: Value = serde_json::from_str(
        &dispatch_json("workspace.project.v1", &projection.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        displayed["workspace"]["canonicalTimer"]["status"],
        "running"
    );
    let original = input.clone();
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    assert_eq!(input, original);
}
