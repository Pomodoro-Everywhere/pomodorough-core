use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn finish_request() -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/bootstrap-workspace-v1.json")).unwrap();
    let mut input = fixture["request"].clone();
    let start = fixture["command"].clone();
    let mut finish = start.clone();
    finish["type"] = json!("finish");
    finish["id"] = json!("finish-command");
    finish["deviceSequence"] = json!(2);
    finish["hlcCounter"] = json!(1);
    finish["occurredAt"] = json!("2026-09-21T12:00:10Z");
    finish["observedElapsedMs"] = json!(60000);
    input["local"]["workspace"]["local"]["commands"] = json!([start, finish]);
    input
}

fn call(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("bootstrap.workspacePlan.v1", &input.to_string()).unwrap())
        .unwrap()
}

#[test]
fn native_bootstrap_profiles_project_retained_queues_without_changing_delivery_safe_workspace() {
    let mut input = finish_request();
    for profile in [
        "appleWorkspace",
        "androidRepository",
        "desktopStorage",
        "pwaStorage",
    ] {
        input["profile"] = json!(profile);
        let expected = usize::from(profile != "appleWorkspace");
        assert_eq!(
            call(&input)["classification"]["local"]["completedHistoryCount"],
            expected
        );
    }
    let projected: Value = serde_json::from_str(
        &dispatch_json(
            "workspace.project.v1",
            &input["local"]["workspace"].to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(projected["projectionPending"]["commands"], json!([]));
    assert_eq!(projected["workspace"]["history"], json!([]));
}

#[test]
fn pwa_persisted_display_queues_and_fresh_proof_follow_actual_owner_projection() {
    for retained_display in [false, true] {
        for fresh in [false, true] {
            for head in [Value::Null, json!({"wallMs":1790000000000_i64,"counter":1})] {
                let mut input = finish_request();
                input["profile"] = json!("pwaStorage");
                let queues = input["local"]["workspace"]["local"].clone();
                let mut stored = queues.clone();
                if !retained_display {
                    stored["commands"] = json!([]);
                }
                input["local"]["projectionPending"] = stored;
                input["local"]["workspace"]["canonicalHead"] = head.clone();
                if fresh {
                    input["local"]["workspace"]["neverSent"] =
                        json!({"commands":["start-legacy","finish-command"]});
                }
                let expected = usize::from(retained_display || (fresh && head.is_null()));
                assert_eq!(
                    call(&input)["classification"]["local"]["completedHistoryCount"],
                    expected
                );
            }
        }
    }
}

#[test]
fn persisted_display_records_cannot_forge_or_rewrite_retained_payloads() {
    let mut input = finish_request();
    input["profile"] = json!("pwaStorage");
    input["local"]["projectionPending"] = input["local"]["workspace"]["local"].clone();
    input["local"]["projectionPending"]["commands"][0]["observedElapsedMs"] = json!(17);
    let error = dispatch_json("bootstrap.workspacePlan.v1", &input.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains("requires recovery"), "{error}");
    input["profile"] = json!("androidRepository");
    assert!(
        dispatch_json("bootstrap.workspacePlan.v1", &input.to_string())
            .unwrap_err()
            .to_string()
            .contains("requires pwaStorage")
    );
}

#[test]
fn malformed_complete_queues_fail_before_pwa_subset_or_android_horizon_derivation() {
    for profile in ["pwaStorage", "androidRepository"] {
        for malformed in [Value::Null, json!({}), json!(42), json!([false])] {
            let mut input = finish_request();
            input["profile"] = json!(profile);
            if profile == "pwaStorage" {
                input["local"]["projectionPending"] = input["local"]["workspace"]["local"].clone();
            }
            input["local"]["workspace"]["local"]["commands"] = malformed;
            assert!(
                dispatch_json("bootstrap.workspacePlan.v1", &input.to_string())
                    .unwrap_err()
                    .to_string()
                    .contains("requires recovery")
            );
        }
    }
}
