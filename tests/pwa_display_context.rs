use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/pwa-display-context-v1.json")).unwrap()
}

fn metadata(records: &Value, key: &str) -> Value {
    records["meta"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["key"] == key)
        .map_or(Value::Null, |record| record["value"].clone())
}

fn workspace(index: usize) -> Value {
    let records = &fixture()["observations"][index]["persisted"];
    let snapshot = metadata(records, "snapshot");
    let base: Value = [
        "canonicalTimer",
        "history",
        "tasks",
        "durationsMs",
        "autoStartBreaks",
        "selectedTaskId",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), snapshot[name].clone()))
    .collect();
    json!({"base": base, "local": {"commands": records["pending"], "taskOperations": records["pendingTasks"],
        "durationOperations": records["pendingDurations"], "autoStartOperations": records["pendingAutoStarts"],
        "selectedTaskOperations": records["pendingSelectedTasks"]}, "canonicalHead": metadata(records, "canonicalHead"),
        "neverSent": metadata(records, "deliveryProof"), "timerDependencies": metadata(records, "timerDependencies"),
        "displayContext": {"profile": "pwaStorage", "projectionPending": metadata(records, "projectionPending")},
        "now": "2026-08-31T12:00:01Z"})
}

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn read(mut workspace: Value) -> Value {
    workspace.as_object_mut().unwrap().remove("now");
    call(
        "workspace.readModel.v1",
        &json!({"profile":"pwaStorage", "source":{"kind":"workspace", "value":workspace},
        "selectedPhase":"focus", "observedAt":"2026-08-31T12:00:01Z",
        "calendarIntervals":[{"start":"2026-08-31T00:00:00Z", "end":"2026-09-01T00:00:00Z"}]}),
    )
}

#[test]
fn actual_persisted_pwa_inputs_display_without_delivery_permission() {
    for index in [0, 1] {
        let input = workspace(index);
        let before = input.clone();
        let output = call("workspace.project.v1", &input);
        assert_eq!(output["workspace"]["canonicalTimer"]["status"], "running");
        assert_eq!(
            output["displayContext"]["projectionPending"]["commands"],
            input["local"]["commands"]
        );
        assert_eq!(output["projectionPending"]["commands"], json!([]));
        assert_eq!(input, before);
        let model = read(input);
        assert_eq!(model["canonical"]["status"], "running");
        assert_eq!(
            model["availableIntents"],
            json!(["pause", "finish", "cancel", "cancelAndClear"])
        );
    }
}

#[test]
fn original_contract_without_context_still_suppresses_both_observations() {
    for index in [0, 1] {
        let mut input = workspace(index);
        input.as_object_mut().unwrap().remove("displayContext");
        let output = call("workspace.project.v1", &input);
        assert_eq!(output["workspace"]["canonicalTimer"], Value::Null);
        assert!(output.get("displayContext").is_none());
        assert_eq!(read(input)["canonical"]["status"], "idle");
    }
}

#[test]
fn stale_context_fails_before_it_can_create_a_timer() {
    let mut input = workspace(0);
    input["local"]["commands"] = json!([]);
    let envelope: Value = serde_json::from_str(&dispatch_envelope_json(
        "workspace.project.v1",
        &input.to_string(),
    ))
    .unwrap();
    assert_eq!(envelope["ok"], false);
    assert!(
        envelope["error"]
            .as_str()
            .unwrap()
            .contains("retained payloads")
    );
}

#[test]
fn raw_records_require_exact_extensions_not_typed_equivalence() {
    let mut input = workspace(0);
    input["local"]["commands"][0]["extension"] =
        json!({"null":null,"empty":"","nested":[false,{}]});
    input["displayContext"]["projectionPending"]["commands"][0] =
        input["local"]["commands"][0].clone();
    let output = call("workspace.project.v1", &input);
    assert_eq!(output["displayContext"], input["displayContext"]);
    input["displayContext"]["projectionPending"]["commands"][0]
        .as_object_mut()
        .unwrap()
        .remove("extension");
    assert!(dispatch_json("workspace.project.v1", &input.to_string()).is_err());
}

#[test]
fn stored_timer_records_do_not_hide_bad_preference_wire_values() {
    let mut input = workspace(0);
    let clock = &input["local"]["commands"][0];
    input["local"]["durationOperations"] = json!([{"id":"bad-duration","deviceId":"other", "phase":"focus",
        "durationMs":1,"hlcWallMs":clock["hlcWallMs"],"hlcCounter":clock["hlcCounter"],"occurredAt":clock["occurredAt"]}]);
    assert!(dispatch_json("workspace.project.v1", &input.to_string()).is_err());
}

#[test]
fn shared_bootstrap_selector_rejects_hidden_malformed_command_without_panicking() {
    let mut input: Value =
        serde_json::from_str(include_str!("../fixtures/bootstrap-workspace-v1.json")).unwrap();
    let mut request = input["request"].take();
    let mut workspace = workspace(0);
    workspace["local"]["commands"][0]["id"] = Value::Null;
    let mut stored = workspace["local"].clone();
    stored["commands"] = json!([]);
    workspace.as_object_mut().unwrap().remove("displayContext");
    request["profile"] = json!("pwaStorage");
    request["local"]["workspace"] = workspace;
    request["local"]["projectionPending"] = stored;
    assert!(dispatch_json("bootstrap.workspacePlan.v1", &request.to_string()).is_err());
}
