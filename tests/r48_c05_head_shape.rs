use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const WALL: i64 = 1_784_548_800_000;

fn malformed_heads() -> Vec<Value> {
    vec![json!([WALL, 10]), json!([WALL, 0]), json!([0, 0])]
}

fn assert_head_rejected(operation: &str, input: &Value) {
    let err = dispatch_json(operation, &input.to_string())
        .expect_err(&format!("{operation} accepted positional head"));
    let text = err.to_string();
    assert!(text.contains("canonicalHead"), "{operation} {text}");
    assert!(text.contains("object"), "{operation} {text}");
}

fn project_request() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-projection-v1.json")).unwrap()
}

fn read_request() -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/read-model-v1.json")).unwrap()["request"]
        .clone()
}

fn intent_request() -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/workspace-intent-v1.json")).unwrap()
        ["request"]
        .clone()
}

fn completion_request() -> Value {
    let fixtures: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let mut input = fixtures["request"].clone();
    input.as_object_mut().unwrap().remove("intent");
    input["stage"] = json!("finishCommit");
    input["compatibility"] = json!("appleWorkspace");
    input["selection"]["explicit"] = json!(false);
    input["identities"]["timerUuid"] = Value::Null;
    input["identities"]["commandUuids"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    input["ownership"] = Value::Null;
    let mut timer = fixtures["timer"].clone();
    timer["status"] = json!("running");
    input["workspace"]["base"]["canonicalTimer"] = timer.clone();
    input["requestedTimer"] = timer;
    input
}

fn ownership_request() -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/pwa-ownership-plan-v1.json")).unwrap()
        ["request"]
        .clone()
}

fn legacy_prefs_request() -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/legacy-preferences-v1.json")).unwrap()
        ["request"]
        .clone()
}

fn legacy_deps_input() -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/legacy-dependencies-v1.json")).unwrap();
    fixture["receipts"][0]["input"].clone()
}

fn bootstrap_request() -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/bootstrap-workspace-v1.json")).unwrap()
        ["request"]
        .clone()
}

#[test]
fn project_rejects_positional_and_keeps_controls() {
    let base = project_request()["request"].clone();
    for head in malformed_heads() {
        let mut input = base.clone();
        input["canonicalHead"] = head;
        assert_head_rejected("workspace.project.v1", &input);
    }
    let mut null_input = base.clone();
    null_input["canonicalHead"] = Value::Null;
    let null_out: Value = serde_json::from_str(
        &dispatch_json("workspace.project.v1", &null_input.to_string()).unwrap(),
    )
    .unwrap();
    assert!(null_out.get("projectionPending").is_some());
    assert!(null_out.get("workspace").is_some());
    let mut obj_input = base.clone();
    obj_input["canonicalHead"] = json!({"wallMs": WALL, "counter": 10});
    let obj_out: Value = serde_json::from_str(
        &dispatch_json("workspace.project.v1", &obj_input.to_string()).unwrap(),
    )
    .unwrap();
    assert!(obj_out.get("projectionPending").is_some());
    for queue in [
        "commands",
        "taskOperations",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ] {
        assert!(obj_out["projectionPending"].get(queue).is_some());
    }
    assert!(obj_out["workspace"]["tasks"].is_array());
}

#[test]
fn read_model_rejects_positional_and_keeps_controls() {
    for head in malformed_heads() {
        let mut input = read_request();
        input["source"]["value"]["canonicalHead"] = head;
        assert_head_rejected("workspace.readModel.v1", &input);
    }
    let mut null_input = read_request();
    null_input["source"]["value"]["canonicalHead"] = Value::Null;
    assert!(dispatch_json("workspace.readModel.v1", &null_input.to_string()).is_ok());
    let mut obj_input = read_request();
    obj_input["source"]["value"]["canonicalHead"] = json!({"wallMs": 1, "counter": 0});
    assert!(dispatch_json("workspace.readModel.v1", &obj_input.to_string()).is_ok());
}

#[test]
fn intent_rejects_positional_and_keeps_controls() {
    for head in malformed_heads() {
        let mut input = intent_request();
        input["workspace"]["canonicalHead"] = head;
        assert_head_rejected("workspace.intent.v1", &input);
    }
    let mut null_input = intent_request();
    null_input["workspace"]["canonicalHead"] = Value::Null;
    assert!(dispatch_json("workspace.intent.v1", &null_input.to_string()).is_ok());
    let obj_input = intent_request();
    assert!(dispatch_json("workspace.intent.v1", &obj_input.to_string()).is_ok());
}

#[test]
fn completion_mutation_rejects_positional_and_keeps_controls() {
    for head in malformed_heads() {
        let mut input = completion_request();
        input["workspace"]["canonicalHead"] = head;
        assert_head_rejected("workspace.completionMutation.v1", &input);
    }
    let mut null_input = completion_request();
    null_input["workspace"]["canonicalHead"] = Value::Null;
    assert!(dispatch_json("workspace.completionMutation.v1", &null_input.to_string()).is_ok());
    let obj_input = completion_request();
    assert!(dispatch_json("workspace.completionMutation.v1", &obj_input.to_string()).is_ok());
}

#[test]
fn ownership_rejects_positional_and_keeps_controls() {
    for head in malformed_heads() {
        let mut input = ownership_request();
        input["workspace"]["canonicalHead"] = head;
        assert_head_rejected("workspace.ownershipPlan.v1", &input);
    }
    let mut null_input = ownership_request();
    null_input["workspace"]["canonicalHead"] = Value::Null;
    assert!(dispatch_json("workspace.ownershipPlan.v1", &null_input.to_string()).is_ok());
    let mut obj_input = ownership_request();
    obj_input["workspace"]["canonicalHead"] = json!({"wallMs": WALL, "counter": 0});
    let out: Value = serde_json::from_str(
        &dispatch_json("workspace.ownershipPlan.v1", &obj_input.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(out["workspace"], obj_input["workspace"]);
}

#[test]
fn legacy_preferences_rejects_positional_and_keeps_controls() {
    for head in malformed_heads() {
        let mut input = legacy_prefs_request();
        input["workspace"]["canonicalHead"] = head;
        assert_head_rejected("workspace.legacyPreferences.v1", &input);
    }
    let mut null_input = legacy_prefs_request();
    null_input["workspace"]["canonicalHead"] = Value::Null;
    assert!(dispatch_json("workspace.legacyPreferences.v1", &null_input.to_string()).is_ok());
    let obj_input = legacy_prefs_request();
    assert!(dispatch_json("workspace.legacyPreferences.v1", &obj_input.to_string()).is_ok());
}

#[test]
fn legacy_dependencies_rejects_positional_and_keeps_controls() {
    for head in malformed_heads() {
        let mut input = legacy_deps_input();
        input["workspace"]["canonicalHead"] = head;
        assert_head_rejected("workspace.legacyDependencyPlan.v1", &input);
    }
    let mut null_input = legacy_deps_input();
    null_input["workspace"]["canonicalHead"] = Value::Null;
    assert!(dispatch_json("workspace.legacyDependencyPlan.v1", &null_input.to_string()).is_ok());
    let obj_input = legacy_deps_input();
    assert!(dispatch_json("workspace.legacyDependencyPlan.v1", &obj_input.to_string()).is_ok());
}

#[test]
fn bootstrap_rejects_positional_and_keeps_controls() {
    for head in malformed_heads() {
        let mut input = bootstrap_request();
        input["local"]["workspace"]["canonicalHead"] = head;
        let err = dispatch_json("bootstrap.workspacePlan.v1", &input.to_string())
            .expect_err("bootstrap accepted positional head");
        assert!(err.to_string().contains("canonicalHead"));
    }
    let mut null_input = bootstrap_request();
    null_input["local"]["workspace"]["canonicalHead"] = Value::Null;
    assert!(dispatch_json("bootstrap.workspacePlan.v1", &null_input.to_string()).is_ok());
    let mut obj_input = bootstrap_request();
    obj_input["local"]["workspace"]["canonicalHead"] =
        json!({"wallMs": 1790000000002_i64, "counter": 0});
    assert!(dispatch_json("bootstrap.workspacePlan.v1", &obj_input.to_string()).is_ok());
}
