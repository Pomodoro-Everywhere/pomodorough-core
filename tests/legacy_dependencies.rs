use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

const OPERATION: &str = "workspace.legacyDependencyPlan.v1";

fn receipts() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("../fixtures/legacy-dependencies-v1.json")).unwrap()
        ["receipts"]
        .as_array()
        .unwrap()
        .clone()
}

fn request(name: &str) -> Value {
    receipts()
        .into_iter()
        .find(|row| row["name"] == name)
        .unwrap()["input"]
        .clone()
}

fn plan(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(OPERATION, &input.to_string()).unwrap()).unwrap()
}

#[test]
fn exact_raw_production_inputs_match_complete_native_envelopes() {
    for receipt in receipts() {
        let raw = receipt["inputRaw"].as_str().unwrap();
        assert_eq!(
            dispatch_envelope_json(OPERATION, raw),
            receipt["completeNativeEnvelope"].as_str().unwrap(),
            "complete return for {}",
            receipt["name"]
        );
        let input: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(input, receipt["input"]);
    }
}

#[test]
fn missing_provenance_retains_every_record_and_returns_operative_recovery() {
    let input = request("missing-parent");
    let output = plan(&input);
    assert_eq!(output["workspace"], input["workspace"]);
    assert_eq!(output["outgoing"], input["outgoing"]);
    assert_eq!(output["metadataWrites"], json!([]));
    assert_eq!(output["timerDependencies"], Value::Null);
    assert_eq!(output["recovery"]["blocksSync"], true);
    assert_eq!(output["recovery"]["blocksMutations"], true);
    assert_eq!(output["recovery"]["automaticRepair"], false);
    assert_eq!(
        output["recovery"]["unresolved"][0]["reason"],
        "missingSourceProvenance"
    );
    assert_eq!(
        output["recovery"]["unresolved"][0]["command"],
        input["workspace"]["local"]["commands"][0]
    );
}

#[test]
fn raw_native_entity_metadata_upgrades_only_proven_internal_edges() {
    let mut input = request("sibling-proven");
    input["profile"] = json!("androidCentralized");
    for command in input["workspace"]["local"]["commands"]
        .as_array_mut()
        .unwrap()
    {
        let object = command.as_object_mut().unwrap();
        if let Some(parent) = object.remove("dependsOnCommandId") {
            object.insert("generatedByFinishCommandId".into(), parent);
        }
        object.remove("generatedBreak");
        object.remove("deviceId");
        object.insert("neverSent".into(), json!(true));
    }
    let output = plan(&input);
    assert_eq!(output["outcome"], "planned");
    assert_eq!(output["workspace"]["local"], input["workspace"]["local"]);
    let pause = output["timerDependencies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["operationId"] == "legacy-break-pause")
        .unwrap();
    assert_eq!(pause["dependsOnOperationId"], "legacy-break-start");
    input["workspace"]["neverSent"]["commands"] = json!(["legacy-finish", "legacy-break-start"]);
    input["workspace"]["local"]["commands"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == "legacy-break-pause")
        .unwrap()["neverSent"] = json!(false);
    assert_eq!(
        plan(&input)["recovery"]["unresolved"][0]["reason"],
        "possiblyDeliveredSibling"
    );
}

#[test]
fn canonical_source_requires_saved_identity_exact_completion_and_acknowledgement() {
    let input = request("canonical-source-applied");
    for outcome in ["applied", "ignored"] {
        let mut input = input.clone();
        input["sourceAcknowledgements"][0]["outcome"] = json!(outcome);
        assert_eq!(plan(&input)["timerDependencies"], json!([]));
        assert_eq!(plan(&input)["outgoing"], input["outgoing"]);
    }
    let mut missing_ack = input.clone();
    missing_ack["sourceAcknowledgements"] = json!([]);
    assert_eq!(
        plan(&missing_ack)["recovery"]["unresolved"][0]["reason"],
        "sourceAcknowledgementRequired"
    );
    let mut natural = input.clone();
    natural["workspace"]["base"]["history"][0]["commandId"] = Value::Null;
    assert_eq!(
        plan(&natural)["recovery"]["unresolved"][0]["reason"],
        "missingSourceProvenance"
    );
    let mut rejected = input;
    rejected["sourceAcknowledgements"][0]["outcome"] = json!("rejected");
    assert_eq!(
        plan(&rejected)["recovery"]["unresolved"][0]["reason"],
        "sourceRejectedRequiresDecision"
    );
}

#[test]
fn canonical_payload_difference_requires_decision_without_queue_rewrite() {
    let mut input = request("canonical-source-applied");
    input["workspace"]["base"]["durationsMs"]["short_break"] = json!(600_000);
    let output = plan(&input);
    assert_eq!(
        output["recovery"]["unresolved"][0]["reason"],
        "canonicalPayloadDecisionRequired"
    );
    assert_eq!(output["workspace"], input["workspace"]);
    assert_eq!(output["outgoing"], input["outgoing"]);
}

#[test]
fn supplied_calendar_is_required_and_must_contain_exact_causal_completion() {
    let mut input = request("complete");
    input["calendarIntervals"] = json!([]);
    assert_eq!(
        plan(&input)["recovery"]["unresolved"][0]["reason"],
        "missingCalendarEvidence"
    );
    let command = input["workspace"]["local"]["commands"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == "legacy-break-start")
        .unwrap();
    command["sourceDayStart"] = json!("2026-09-01T00:00:00Z");
    command["sourceDayEnd"] = json!("2026-09-02T00:00:00Z");
    assert!(
        dispatch_json(OPERATION, &input.to_string())
            .unwrap_err()
            .to_string()
            .contains("excludes exact completion")
    );
}

#[test]
fn count_is_causal_at_source_not_all_later_completions_in_day() {
    let mut input = request("complete");
    input["workspace"]["base"]["history"] = json!((0..3).map(|index| json!({
        "id": format!("later-{index}"), "timerId": format!("later-{index}"), "commandId": format!("later-finish-{index}"),
        "phase": "focus", "status": "completed", "plannedDurationMs": 1500000,
        "completedAt": "2026-08-31T13:00:00Z", "endedAt": "2026-08-31T13:00:00Z"
    })).collect::<Vec<_>>());
    assert_eq!(
        plan(&input)["classifications"][0]["sourcePhaseAfter"],
        "short_break"
    );
}

#[test]
fn explicit_empty_local_metadata_does_not_resurrect_old_wire_dependency() {
    let mut input = request("missing-parent");
    input["workspace"]["timerDependencies"] = json!([]);
    let output = plan(&input);
    assert_eq!(output["outcome"], "noop");
    assert_eq!(output["workspace"], input["workspace"]);
}

#[test]
fn exact_canonical_finish_without_history_is_evidence_without_persisted_phantom_rows() {
    let mut input = request("canonical-source-applied");
    let source = input["workspace"]["base"]["history"][0].clone();
    input["workspace"]["base"]["history"] = json!([]);
    input["workspace"]["base"]["canonicalTimer"] = json!({"id": source["timerId"], "phase": "focus",
        "status": "completed", "plannedDurationMs": source["plannedDurationMs"],
        "elapsedAtAnchorMs": source["plannedDurationMs"], "anchorAt": source["completedAt"],
        "startedByDeviceId": input["deviceId"], "lastIntent": {"type": "finish",
            "commandId": "legacy-finish", "occurredAt": source["completedAt"]}});
    let output = plan(&input);
    assert_eq!(output["outcome"], "planned");
    assert_eq!(output["timerDependencies"], json!([]));
    assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
    assert_eq!(output["workspace"]["local"], input["workspace"]["local"]);
    assert_eq!(output["outgoing"], input["outgoing"]);
}

#[test]
fn frozen_generated_payload_difference_returns_blocked_recovery() {
    let mut input = request("complete");
    let start = input["workspace"]["local"]["commands"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == "legacy-break-start")
        .unwrap();
    start["phase"] = json!("long_break");
    input["workspace"]["neverSent"]["commands"] = json!(["legacy-finish"]);
    let output = plan(&input);
    assert_eq!(output["outcome"], "blocked");
    assert_eq!(
        output["recovery"]["unresolved"][0]["reason"],
        "possiblyDeliveredPayloadDecisionRequired"
    );
    assert_eq!(output["workspace"], input["workspace"]);
}

#[test]
fn original_numeric_extensions_and_saved_body_survive_single_pass_envelope() {
    let mut input = request("sibling-proven");
    input["workspace"]["local"]["commands"][0]["extension"] =
        json!({"number": 90.49999999999999, "array": [90.49999999999999]});
    let raw = input.to_string();
    let envelope = dispatch_envelope_json(OPERATION, &raw);
    assert!(envelope.contains("90.49999999999999"));
    assert!(!envelope.contains("90.5"));
    let original = request("canonical-source-applied");
    let output = plan(&original);
    assert_eq!(output["outgoing"]["body"], original["outgoing"]["body"]);
}

#[test]
fn raw_physical_observation_selects_platform_interval_without_rewriting_wire_time() {
    let mut input = request("complete");
    for command in input["workspace"]["local"]["commands"]
        .as_array_mut()
        .unwrap()
    {
        command["physicalOccurredAt"] = json!("2026-08-30T14:00:00Z");
    }
    input["calendarIntervals"] =
        json!([{"start": "2026-08-30T00:00:00Z", "end": "2026-08-31T00:00:00Z"}]);
    let output = plan(&input);
    assert_eq!(output["outcome"], "planned");
    assert_eq!(
        output["classifications"][0]["sourceCompletedAt"],
        "2026-08-30T14:00:00Z"
    );
    assert_eq!(
        output["timerDependencies"][0]["sourceDayStart"],
        "2026-08-30T00:00:00Z"
    );
    assert_eq!(output["workspace"]["local"], input["workspace"]["local"]);
}

#[test]
fn proven_legacy_sibling_after_saved_source_ack_uses_retained_start_without_wire_rewrite() {
    let mut input = request("canonical-source-applied");
    let mut pause = input["workspace"]["local"]["commands"][0].clone();
    pause["id"] = json!("legacy-break-pause");
    pause["type"] = json!("pause");
    pause["generatedBreak"] = json!(false);
    pause["deviceSequence"] = json!(pause["deviceSequence"].as_i64().unwrap() + 1);
    pause["hlcCounter"] = json!(pause["hlcCounter"].as_i64().unwrap() + 1);
    input["workspace"]["local"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(pause);
    input["workspace"]["neverSent"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("legacy-break-pause"));
    let output = plan(&input);
    assert_eq!(output["outcome"], "planned");
    assert_eq!(
        output["timerDependencies"],
        json!([{"operationId": "legacy-break-pause", "dependsOnOperationId": "legacy-break-start"}])
    );
    assert_eq!(output["workspace"]["local"], input["workspace"]["local"]);
    assert_eq!(output["outgoing"], input["outgoing"]);
}
