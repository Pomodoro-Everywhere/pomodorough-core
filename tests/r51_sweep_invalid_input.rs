use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

// R51 sweep: bare-trap/Null-fallback sites now fail as `InvalidInput` with
// `op=`/`history=`/`queued=` context (helpers in `src/lib.rs`, covered by
// direct unit tests there). Via public ops the shape/serde layers fire first,
// so these tests pin the reachable fail-closed behavior: `is_err` plus no
// history-contents leak. Helper error branches stay unreachable publicly by
// construction: `validate_shape` rejects a missing base before
// `take_required_field`, and composed projections always emit a workspace
// envelope before `take_projection_workspace`. Wildcard intents never reach
// dispatch: `Intent` uses `deny_unknown_fields`, so serde rejects unknown
// kinds first. The `index == 0` + missing-prior and PWA running-timer guard
// branches are defensive for the same reason: the output policy keeps the
// prior workspace exactly when `index == 0`, and both checks read the same
// immutable `before` value.

fn read_fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/read-model-v1.json")).unwrap()
}

fn intent_fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap()
}

#[test]
fn projection_with_missing_base_fails_closed_without_contents() {
    let read = read_fixture();
    let mut workspace = read["request"]["source"]["value"].clone();
    workspace["neverSent"] = json!({"marker": "SweepSecretTitle-7f3a"});
    workspace.as_object_mut().unwrap().remove("base");
    workspace["now"] = read["request"]["observedAt"].clone();
    let error = dispatch_json("workspace.project.v1", &workspace.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains("base"), "{error}");
    assert!(!error.contains("SweepSecretTitle-7f3a"), "{error}");
}

#[test]
fn read_model_with_null_or_missing_workspace_fails_closed() {
    let read = read_fixture();
    let mut request = read["request"].clone();
    request["source"] = json!({"kind": "workspace", "value": Value::Null});
    let error = dispatch_json("workspace.readModel.v1", &request.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains("source.value"), "{error}");

    request["source"] = json!({"kind": "workspace"});
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
}

#[test]
fn unknown_intent_kind_fails_closed_before_dispatch() {
    let intent = intent_fixture();
    let mut request = intent["request"].clone();
    request["intent"] = json!({"kind": "notARealIntent"});
    assert!(dispatch_json("workspace.intent.v1", &request.to_string()).is_err());
}
