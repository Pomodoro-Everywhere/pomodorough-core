use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

const OPERATION: &str = "workspace.legacyDependencyPlan.v1";

fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!(
        "../fixtures/legacy-dependency-residual-v1.json"
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
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
    assert_eq!(output["recovery"]["automaticRepair"], false);
    let mut reopened = input.clone();
    reopened["workspace"] = output["workspace"].clone();
    assert_eq!(plan(&reopened), *output);
}

fn body_rebase(input: &Value, dependencies: &Value) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/reconciliation-terminal-v3.json")).unwrap();
    let mut response: Value =
        serde_json::from_str(fixture["http"]["responseRaw"].as_str().unwrap()).unwrap();
    for (field, value) in input["workspace"]["base"].as_object().unwrap() {
        response[field] = value.clone();
    }
    for field in [
        "acknowledgements",
        "taskAcknowledgements",
        "durationAcknowledgements",
        "autoStartAcknowledgements",
        "selectedTaskAcknowledgements",
    ] {
        response[field] = json!([]);
    }
    response["serverTime"] = input["workspace"]["now"].clone();
    response["serverHlcWallMs"] = input["outgoing"]["sent"]["commands"][0]["hlcWallMs"].clone();
    response["serverHlcCounter"] = input["outgoing"]["sent"]["commands"][0]["hlcCounter"].clone();
    let head = &input["workspace"]["canonicalHead"];
    if !head.is_null() {
        response["serverHlcWallMs"] = head["wallMs"].clone();
        response["serverHlcCounter"] = head["counter"].clone();
    }
    let sent: serde_json::Map<String, Value> = input["workspace"]["local"]
        .as_object()
        .unwrap()
        .keys()
        .map(|domain| (domain.clone(), json!([])))
        .collect();
    json!({"local": input["workspace"]["local"], "sent": sent, "response": response,
        "timerDependencies": dependencies, "neverSent": input["workspace"]["neverSent"]})
}

#[test]
fn original_six_residual_inputs_block_before_metadata_commit_and_after_reopen() {
    assert_eq!(cases().len(), 6);
    for case in cases() {
        let raw = case["inputRaw"].as_str().unwrap();
        let input: Value = serde_json::from_str(raw).unwrap();
        let envelope: Value =
            serde_json::from_str(&dispatch_envelope_json(OPERATION, raw)).unwrap();
        assert_eq!(envelope["ok"], true, "{}", case["name"]);
        assert_blocked(&input, &envelope["value"]);
        let expected = if case["name"].as_str().unwrap().starts_with("body-") {
            "savedRequestBodyIncomplete"
        } else {
            "possiblyDeliveredPayloadDecisionRequired"
        };
        assert_eq!(
            envelope["value"]["recovery"]["unresolved"][0]["reason"],
            expected
        );
        if case["rebaseInput"].is_null() {
            assert!(
                dispatch_json(
                    "reconcile.rebase.v3",
                    &body_rebase(&input, &envelope["value"]["timerDependencies"]).to_string()
                )
                .is_err()
            );
        }
    }
}

#[test]
fn original_finished_batch_compositions_reach_real_rebase_wire_rewrite_boundary() {
    for case in cases()
        .into_iter()
        .filter(|case| !case["rebaseInput"].is_null())
    {
        let result =
            dispatch_envelope_json("reconcile.rebase.v3", &case["rebaseInput"].to_string());
        let envelope: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(envelope, case["originalRebaseEnvelope"]);
        assert!(
            envelope["error"]
                .as_str()
                .unwrap()
                .contains("rewrite a possibly delivered operation")
        );
        let input: Value = serde_json::from_str(case["inputRaw"].as_str().unwrap()).unwrap();
        let output = plan(&input);
        assert_blocked(&input, &output);
        let mut forbidden = case["rebaseInput"].clone();
        forbidden["timerDependencies"] = output["timerDependencies"].clone();
        assert!(dispatch_json("reconcile.rebase.v3", &forbidden.to_string()).is_err());
    }
}

#[test]
fn proven_finished_batch_members_normalize_but_migration_preserves_their_raw_records() {
    for case in cases()
        .into_iter()
        .filter(|case| !case["rebaseInput"].is_null())
    {
        let mut input: Value = serde_json::from_str(case["inputRaw"].as_str().unwrap()).unwrap();
        input["workspace"]["neverSent"]["commands"]
            .as_array_mut()
            .unwrap()
            .push(json!("legacy-break-pause"));
        let output = plan(&input);
        assert_eq!(output["outcome"], "planned");
        assert_eq!(output["workspace"]["local"], input["workspace"]["local"]);
        let mut rebase = case["rebaseInput"].clone();
        rebase["timerDependencies"] = output["timerDependencies"].clone();
        rebase["neverSent"]["commands"]
            .as_array_mut()
            .unwrap()
            .push(json!("legacy-break-pause"));
        let rebased: Value = serde_json::from_str(
            &dispatch_json("reconcile.rebase.v3", &rebase.to_string()).unwrap(),
        )
        .unwrap();
        let start = input["workspace"]["local"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "legacy-break-start")
            .unwrap();
        for row in rebased["pending"].as_array().unwrap() {
            assert_eq!(row["phase"], start["phase"]);
            assert_eq!(row["plannedDurationMs"], start["plannedDurationMs"]);
            assert!(
                row["observedElapsedMs"].as_i64().unwrap()
                    <= start["plannedDurationMs"].as_i64().unwrap()
            );
        }
    }
}

#[test]
fn reducer_ignored_finish_fields_and_raw_extensions_are_not_blanket_rejected() {
    let case = cases()
        .into_iter()
        .find(|case| !case["rebaseInput"].is_null())
        .unwrap();
    let mut input: Value = serde_json::from_str(case["inputRaw"].as_str().unwrap()).unwrap();
    input["workspace"]["neverSent"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("legacy-break-pause"));
    let fields = json!({"phase": "focus", "plannedDurationMs": 60000, "observedElapsedMs": 900000});
    let mut rebase = case["rebaseInput"].clone();
    for queues in [&mut input["workspace"]["local"], &mut rebase["local"]] {
        let finish = queues["commands"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["id"] == "legacy-break-finish")
            .unwrap();
        for (name, value) in fields.as_object().unwrap() {
            finish[name] = value.clone();
        }
        finish["extension"] = json!({"number": 90.49999999999999, "nested": [null, false]});
    }
    let raw = dispatch_json(OPERATION, &input.to_string()).unwrap();
    assert!(raw.contains("\"number\":90.49999999999999"));
    let output: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(output["outcome"], "planned");
    // Compare both complete records through the same test decoder. Raw output
    // above separately verifies the number that Serde's Value decoder rounds.
    let decoded: Value = serde_json::from_str(&input.to_string()).unwrap();
    assert_eq!(output["workspace"]["local"], decoded["workspace"]["local"]);
    rebase["timerDependencies"] = output["timerDependencies"].clone();
    rebase["neverSent"]["commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("legacy-break-pause"));
    assert!(dispatch_json("reconcile.rebase.v3", &rebase.to_string()).is_ok());
}

#[test]
fn intact_explicit_body_identity_preserves_discharge_and_rebase_under_all_heads() {
    let full: Value =
        serde_json::from_str(include_str!("../fixtures/legacy-dependencies-v1.json")).unwrap();
    let original = &full["receipts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "canonical-source-applied")
        .unwrap()["input"];
    for case in cases()
        .into_iter()
        .filter(|case| case["rebaseInput"].is_null())
    {
        let mut input: Value = serde_json::from_str(case["inputRaw"].as_str().unwrap()).unwrap();
        input["outgoing"]["body"] = original["outgoing"]["body"].clone();
        let output = plan(&input);
        assert_eq!(output["outcome"], "planned");
        assert_eq!(output["outgoing"], input["outgoing"]);
        assert_eq!(output["timerDependencies"], json!([]));
        let rebase = body_rebase(&input, &output["timerDependencies"]);
        let result: Value = serde_json::from_str(
            &dispatch_json("reconcile.rebase.v3", &rebase.to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(result["pending"], input["workspace"]["local"]["commands"]);
    }
}

#[test]
fn finished_batch_uses_start_payload_even_when_canonical_cadence_and_duration_differ() {
    let case = cases()
        .into_iter()
        .find(|case| !case["rebaseInput"].is_null())
        .unwrap();
    let mut input: Value = serde_json::from_str(case["inputRaw"].as_str().unwrap()).unwrap();
    let mut rebase = case["rebaseInput"].clone();
    for queues in [&mut input["workspace"]["local"], &mut rebase["local"]] {
        for row in queues["commands"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .filter(|row| row["id"] != "legacy-finish")
        {
            row["phase"] = json!("long_break");
            row["plannedDurationMs"] = json!(60000);
            row["observedElapsedMs"] = json!(0);
        }
    }
    let output = plan(&input);
    assert_eq!(output["outcome"], "planned");
    assert_eq!(output["workspace"]["local"], input["workspace"]["local"]);
    rebase["timerDependencies"] = output["timerDependencies"].clone();
    let result: Value =
        serde_json::from_str(&dispatch_json("reconcile.rebase.v3", &rebase.to_string()).unwrap())
            .unwrap();
    let retained = rebase["local"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["id"] != "legacy-finish")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(result["pending"], json!(retained));
}

#[test]
fn saved_pause_claim_remains_exact_when_finished_batch_requires_recovery() {
    for case in cases()
        .into_iter()
        .filter(|case| !case["rebaseInput"].is_null())
    {
        let mut input: Value = serde_json::from_str(case["inputRaw"].as_str().unwrap()).unwrap();
        let pause = input["workspace"]["local"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "legacy-break-pause")
            .unwrap();
        let sent = json!({"commands": [pause]});
        let body = format!(
            " {{\n\"deviceId\":{},\"commands\":{}\n}} ",
            input["deviceId"], sent["commands"]
        );
        input["outgoing"] = json!({"ownerId": input["ownership"]["ownerId"], "sent": sent,
            "body": body, "queueIds": {"commands": ["legacy-break-pause"]}, "extension": {"retained": null}});
        let output = plan(&input);
        assert_blocked(&input, &output);
        assert_eq!(output["outgoing"]["body"], body);
        assert_eq!(
            output["recovery"]["unresolved"][0]["reason"],
            "possiblyDeliveredPayloadDecisionRequired"
        );
    }
}
