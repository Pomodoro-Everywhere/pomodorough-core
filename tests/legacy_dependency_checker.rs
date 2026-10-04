use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

const OPERATION: &str = "workspace.legacyDependencyPlan.v1";

fn request(name: &str) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/legacy-dependencies-v1.json")).unwrap();
    fixture["receipts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap()["input"]
        .clone()
}

fn plan(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(OPERATION, &input.to_string()).unwrap()).unwrap()
}

fn assert_blocked(input: &Value, output: &Value) {
    assert_eq!(output["outcome"], "blocked");
    assert_eq!(output["workspace"], input["workspace"]);
    assert_eq!(output["outgoing"], input["outgoing"]);
    assert_eq!(output["ownership"], input["ownership"]);
    assert_eq!(output["metadataWrites"], json!([]));
    assert_eq!(output["timerDependencies"], Value::Null);
    assert_eq!(output["recovery"]["blocksSync"], true);
    assert_eq!(output["recovery"]["blocksMutations"], true);
    assert_eq!(
        plan(input),
        *output,
        "reopen must keep complete blocked return"
    );
}

#[test]
fn exact_nine_independent_checker_failures_are_denied_without_wire_changes() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../fixtures/legacy-dependency-checker-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 9);
    for case in fixture["cases"].as_array().unwrap() {
        let input: Value = serde_json::from_str(case["inputRaw"].as_str().unwrap()).unwrap();
        let envelope: Value = serde_json::from_str(&dispatch_envelope_json(
            OPERATION,
            case["inputRaw"].as_str().unwrap(),
        ))
        .unwrap();
        if case["expected"] == "blocked" {
            assert_eq!(envelope["ok"], true, "{}", case["name"]);
        }
        if envelope["ok"] == true {
            assert_blocked(&input, &envelope["value"]);
        } else {
            assert!(envelope["error"].as_str().unwrap().contains("legacy"));
            assert!(envelope.get("value").is_none());
        }
    }
}

#[test]
fn equivalent_offsets_and_exact_nanoseconds_are_valid_source_completion_evidence() {
    let mut offset = request("canonical-source-applied");
    offset["workspace"]["base"]["history"][0]["completedAt"] =
        json!("2026-08-31T14:00:00.000000000+02:00");
    assert_eq!(plan(&offset)["outcome"], "planned");
    let mut precise = request("canonical-source-applied");
    let at = "2026-08-31T12:00:00.000000001Z";
    for field in ["completedAt", "endedAt"] {
        precise["workspace"]["base"]["history"][0][field] = json!(at);
    }
    precise["workspace"]["local"]["commands"][0]["occurredAt"] = json!(at);
    precise["workspace"]["now"] = json!(at);
    precise["outgoing"]["sent"]["commands"][0]["occurredAt"] = json!(at);
    let mut body: Value =
        serde_json::from_str(precise["outgoing"]["body"].as_str().unwrap()).unwrap();
    body["commands"][0]["occurredAt"] = json!(at);
    precise["outgoing"]["body"] = json!(body.to_string());
    assert_eq!(plan(&precise)["outcome"], "planned");
    assert_eq!(plan(&precise)["outgoing"], precise["outgoing"]);
}

#[test]
fn saved_physical_observation_cannot_replace_delivered_finish_occurrence() {
    let mut input = request("canonical-source-applied");
    input["outgoing"]["sent"]["commands"][0]["physicalOccurredAt"] = json!("2026-08-30T12:00:00Z");
    let mut body: Value =
        serde_json::from_str(input["outgoing"]["body"].as_str().unwrap()).unwrap();
    body["commands"][0]["physicalOccurredAt"] =
        input["outgoing"]["sent"]["commands"][0]["physicalOccurredAt"].clone();
    input["outgoing"]["body"] = json!(body.to_string());
    assert_eq!(plan(&input)["outcome"], "planned");
    input["workspace"]["base"]["history"][0]["completedAt"] = json!("2026-08-30T12:00:00Z");
    input["workspace"]["base"]["history"][0]["endedAt"] = json!("2026-08-30T12:00:00Z");
    assert!(
        dispatch_json(OPERATION, &input.to_string())
            .unwrap_err()
            .to_string()
            .contains("completion time")
    );
}

#[test]
fn known_rejection_blocks_even_when_dependency_metadata_is_empty() {
    for retained in [true, false] {
        let mut input = request("canonical-source-applied");
        input["sourceAcknowledgements"][0]["outcome"] = json!("rejected");
        input["workspace"]["timerDependencies"] = json!([]);
        if retained {
            let mut source = input["outgoing"]["sent"]["commands"][0].clone();
            source["deviceId"] = input["deviceId"].clone();
            input["workspace"]["local"]["commands"]
                .as_array_mut()
                .unwrap()
                .push(source);
        }
        let output = plan(&input);
        assert_blocked(&input, &output);
        assert_eq!(
            output["recovery"]["unresolved"][0]["reason"],
            "sourceRejectedRequiresDecision"
        );
        assert_eq!(
            output["recovery"]["unresolved"][0]["operationId"],
            "legacy-finish"
        );
    }
}

#[test]
fn unrecognized_or_incomplete_saved_body_never_authorizes_ready_recovery() {
    for body in [
        "{}",
        "{\"redacted\":true}",
        "{\"deviceId\":\"p222-device\",\"commands\":[]}",
    ] {
        let mut input = request("canonical-source-applied");
        input["outgoing"]["body"] = json!(body);
        let output = plan(&input);
        assert_blocked(&input, &output);
        assert_eq!(
            output["recovery"]["unresolved"][0]["reason"],
            "savedRequestBodyIncomplete"
        );
    }
}

#[test]
fn finished_generated_batch_keeps_original_payload_like_existing_reconciliation() {
    let mut input = request("sibling-proven");
    let mut finish = input["workspace"]["local"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "legacy-break-pause")
        .unwrap()
        .clone();
    finish["id"] = json!("legacy-break-finish");
    finish["type"] = json!("finish");
    finish["deviceSequence"] = json!(finish["deviceSequence"].as_i64().unwrap() + 1);
    finish["hlcCounter"] = json!(finish["hlcCounter"].as_i64().unwrap() + 1);
    finish["dependsOnCommandId"] = json!("legacy-break-pause");
    for command in input["workspace"]["local"]["commands"]
        .as_array_mut()
        .unwrap()
    {
        if command["timerId"] == finish["timerId"] {
            command["plannedDurationMs"] = json!(60000);
        }
    }
    finish["plannedDurationMs"] = json!(60000);
    input["workspace"]["local"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(finish);
    input["workspace"]["neverSent"]["commands"] =
        json!(["legacy-finish", "legacy-break-pause", "legacy-break-finish"]);
    let output = plan(&input);
    assert_eq!(output["outcome"], "planned");
    assert_eq!(output["workspace"]["local"], input["workspace"]["local"]);
}

#[test]
fn frozen_direct_descendant_cannot_hide_a_generated_batch_payload_rewrite() {
    for (field, value) in [
        ("plannedDurationMs", json!(60000)),
        ("observedElapsedMs", json!(900000)),
    ] {
        let mut input = request("sibling-proven");
        let pause = input["workspace"]["local"]["commands"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["id"] == "legacy-break-pause")
            .unwrap();
        pause["dependsOnCommandId"] = json!("legacy-break-start");
        pause[field] = value;
        input["workspace"]["neverSent"]["commands"] =
            json!(["legacy-finish", "legacy-break-start"]);
        let output = plan(&input);
        assert_blocked(&input, &output);
        assert_eq!(
            output["recovery"]["unresolved"][0]["reason"],
            "possiblyDeliveredPayloadDecisionRequired"
        );
    }
}

#[test]
fn unacknowledged_duration_cannot_hide_or_create_a_frozen_payload_conflict() {
    for (duration, expected) in [(60000, "blocked"), (300000, "planned")] {
        let mut input = request("complete");
        input["workspace"]["local"]["commands"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["id"] == "legacy-break-start")
            .unwrap()["plannedDurationMs"] = json!(duration);
        input["workspace"]["neverSent"]["commands"] = json!(["legacy-finish"]);
        input["workspace"]["local"]["durationOperations"] = json!([{"id": "unsent-short-duration",
            "deviceId": input["deviceId"], "phase": "short_break", "durationMs": 60000,
            "occurredAt": input["workspace"]["now"], "hlcWallMs": 1788177600000i64, "hlcCounter": 10}]);
        input["workspace"]["neverSent"]["durationOperations"] = json!(["unsent-short-duration"]);
        let output = plan(&input);
        assert_eq!(output["outcome"], expected);
        assert_eq!(output["workspace"]["local"], input["workspace"]["local"]);
    }
}
