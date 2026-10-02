use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn request() -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/bootstrap-workspace-v1.json")).unwrap()
        ["request"]
        .clone()
}

fn rejects(input: &Value, message: &str) {
    let error = dispatch_json("bootstrap.workspacePlan.v1", &input.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains(message), "{error}");
}

#[test]
fn derived_classification_is_not_accepted_at_any_raw_boundary() {
    for name in [
        "hasLocalState",
        "hasRemoteState",
        "localHistoryCount",
        "remoteHistoryCount",
        "completedHistoryCount",
        "projectionResult",
        "hasState",
        "displayHistoryCount",
        "classification",
        "plan",
        "horizon",
        "projectionHorizon",
    ] {
        for path in [
            vec![],
            vec!["local"],
            vec!["remote"],
            vec!["local", "preferences"],
            vec!["local", "workspace"],
            vec!["local", "workspace", "base"],
        ] {
            let mut input = request();
            let mut target = &mut input;
            for field in path {
                target = &mut target[field];
            }
            target[name] = json!(true);
            rejects(&input, "requires raw state");
        }
    }
}

#[test]
fn all_queues_and_delivery_records_are_required_and_fail_closed() {
    for field in [
        "commands",
        "taskOperations",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ] {
        let mut input = request();
        input["local"]["workspace"]["local"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        rejects(&input, "requires recovery");
        input["local"]["workspace"]["local"][field] = json!([null]);
        rejects(&input, "requires recovery");
    }
    for field in ["canonicalHead", "timerDependencies", "now"] {
        let mut input = request();
        input["local"]["workspace"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        rejects(&input, "requires recovery");
    }
    let mut input = request();
    input["local"]["workspace"]["neverSent"] = json!({"commands": ["unknown"]});
    rejects(&input, "requires recovery");
    input["local"]["workspace"]["neverSent"] = json!({"bogus": []});
    rejects(&input, "requires recovery");
}

#[test]
fn malformed_shapes_limits_and_owner_identity_are_rejected() {
    for field in [
        "history",
        "tasks",
        "durationsMs",
        "autoStartBreaks",
        "selectedTaskId",
    ] {
        let mut input = request();
        input["remote"].as_object_mut().unwrap().remove(field);
        rejects(&input, "missing remote");
    }
    let mut input = request();
    input["remote"]["history"] = json!(vec![json!({}); 10_001]);
    rejects(&input, "exceeds bootstrap limit");
    input = request();
    input["local"]["preferences"]["durationsMs"]["focus"] = json!(true);
    rejects(&input, "invalid bootstrap durationsMs");
    input = request();
    input["local"]["ownerId"] = json!("saved-owner");
    input["currentUserId"] = Value::Null;
    rejects(&input, "missing currentUserId");
    input = request();
    input["profile"] = json!("normalized");
    rejects(&input, "unknown variant");
}

#[test]
fn duplicate_json_fields_are_rejected_before_native_classification() {
    let input = request().to_string().replacen(
        "\"profile\":\"appleWorkspace\"",
        "\"profile\":\"appleWorkspace\",\"profile\":\"pwaStorage\"",
        1,
    );
    assert!(
        dispatch_json("bootstrap.workspacePlan.v1", &input)
            .unwrap_err()
            .to_string()
            .contains("duplicate field")
    );
}

#[test]
fn legacy_tasks_selection_and_history_do_not_require_canonical_uuid_or_title_hash() {
    let mut input = request();
    input["profile"] = json!("desktopStorage");
    input["local"]["knownTasks"] = json!([{"id": "not-a-uuid", "title": "Deleted"}]);
    input["local"]["workspace"]["base"]["tasks"] = json!([{"id": "old", "title": "Legacy"}]);
    input["local"]["preferences"]["selectedTaskId"] = json!("missing-legacy-task");
    input["remote"]["history"] = json!([{}, {"id": "non-uuid", "status": "completed"}, {"id": "cancelled", "status": "cancelled"}]);
    let output: Value = serde_json::from_str(
        &dispatch_json("bootstrap.workspacePlan.v1", &input.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        output["plan"],
        json!({"mode": "choose", "localHistoryCount": 0, "remoteHistoryCount": 1})
    );
}

#[test]
fn malformed_remote_timer_and_unprojectable_legacy_timer_history_need_explicit_recovery() {
    let mut input = request();
    input["remote"]["canonicalTimer"] = json!({});
    rejects(&input, "remote timer requires recovery");
    input = request();
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/bootstrap-workspace-v1.json")).unwrap();
    input["local"]["workspace"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    input["local"]["workspace"]["base"]["history"] =
        json!([{"id": "legacy", "status": "completed"}]);
    rejects(&input, "workspace requires recovery");
}

#[test]
fn malformed_raw_minute_preferences_are_not_hidden_by_meaningful_queues_or_tasks() {
    let mut input = request();
    input["profile"] = json!("androidRepository");
    input["local"]["preferences"]["durationsMs"] = Value::Null;
    input["local"]["preferences"]["focusMinutes"] = json!(0);
    input["local"]["workspace"]["base"]["tasks"] = json!([{"id": "legacy"}]);
    rejects(&input, "invalid focusMinutes");
}

#[test]
fn raw_owners_and_preferences_cannot_be_omitted_as_if_empty() {
    for path in [
        vec!["local", "ownerId"],
        vec!["currentUserId"],
        vec!["local", "preferences", "selectedTaskId"],
        vec!["local", "preferences", "durationsMs"],
    ] {
        let mut input = request();
        let mut parent = &mut input;
        for field in &path[..path.len() - 1] {
            parent = &mut parent[*field];
        }
        parent
            .as_object_mut()
            .unwrap()
            .remove(path.last().unwrap().to_owned());
        assert!(dispatch_json("bootstrap.workspacePlan.v1", &input.to_string()).is_err());
    }
}
