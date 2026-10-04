use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/pwa-cycle-public-v1.json")).unwrap()
}

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn cycle() -> Value {
    fixture()["receipts"][0].clone()
}

#[test]
fn exact_public_cycle_inputs_match_complete_returns_and_raw_record_observations() {
    for receipt in fixture()["receipts"].as_array().unwrap() {
        if let Some(calls) = receipt["calls"].as_array() {
            assert_eq!(receipt["before"], receipt["after"]);
            assert_eq!(receipt["publicActionReturn"], false);
            for input in calls {
                assert_eq!(
                    dispatch_envelope_json(
                        input["operation"].as_str().unwrap(),
                        input["inputRaw"].as_str().unwrap()
                    ),
                    input["envelopeRaw"]
                );
            }
            continue;
        }
        let cycles = if receipt.get("first").is_some() {
            vec![&receipt["first"], &receipt["second"]]
        } else {
            vec![receipt]
        };
        for cycle in cycles {
            assert_cycle(cycle);
        }
    }
}

fn assert_cycle(cycle: &Value) {
    for field in ["choice", "start", "read", "finish"] {
        let receipt = &cycle[field];
        assert_eq!(
            serde_json::from_str::<Value>(receipt["inputRaw"].as_str().unwrap()).unwrap(),
            receipt["input"]
        );
        let output = dispatch_envelope_json(
            receipt["operation"].as_str().unwrap(),
            receipt["inputRaw"].as_str().unwrap(),
        );
        assert_eq!(output, receipt["envelopeRaw"]);
    }
    assert_eq!(
        cycle["start"]["completeReturn"]["selection"]["explicit"],
        false
    );
    assert_eq!(
        cycle["start"]["completeReturn"]["selection"]["generation"],
        cycle["choice"]["completeReturn"]["selection"]["generation"]
    );
    let stored = cycle["chosenRecords"]["meta"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == "completionState")
        .unwrap();
    assert_eq!(
        cycle["start"]["input"]["selection"],
        stored["value"]["selection"]
    );
    assert_eq!(
        cycle["start"]["input"]["lifecycle"],
        stored["value"]["lifecycle"]
    );
}

#[test]
fn old_completed_base_does_not_hide_new_natural_session_or_its_finish() {
    let cycle = cycle();
    let mut read = cycle["read"]["input"].clone();
    assert_ne!(
        read["source"]["value"]["base"]["canonicalTimer"]["id"],
        cycle["start"]["completeReturn"]["commands"][0]["timerId"]
    );
    let result = call("workspace.readModel.v1", &read);
    assert_eq!(result["display"]["phase"], "short_break");
    assert_eq!(result["cadence"]["completedFocusTotal"], 2);
    assert!(
        result["availableIntents"]
            .as_array()
            .unwrap()
            .contains(&json!("finish"))
    );
    read["selection"]["explicit"] = json!(true);
    assert_eq!(
        call("workspace.readModel.v1", &read)["display"]["phase"],
        "focus"
    );
    read["selection"]["explicit"] = json!(false);
    read["lifecycle"]["consumedCompletions"].as_array_mut().unwrap().push(json!({
        "timerId": cycle["start"]["completeReturn"]["commands"][0]["timerId"], "commandId": null, "phase": "focus"}));
    assert_eq!(
        call("workspace.readModel.v1", &read)["display"]["phase"],
        "focus"
    );
}

#[test]
fn durable_original_finish_survives_remote_provenance_change_and_canonical_clear() {
    let cycle = cycle();
    let finish = &cycle["finish"]["completeReturn"];
    let mut read = cycle["read"]["input"].clone();
    read["lifecycle"] = finish["lifecycle"].clone();
    let mut timer = finish["projection"]["canonicalTimer"].clone();
    timer["lastIntent"]["commandId"] = json!("remote-finish");
    read["source"]["value"]["base"]["history"] = finish["projection"]["history"].clone();
    for row in read["source"]["value"]["base"]["history"]
        .as_array_mut()
        .unwrap()
    {
        if row["timerId"] == timer["id"] {
            row["commandId"] = json!("remote-finish");
        }
    }
    read["source"]["value"]["local"]["commands"] = json!([]);
    read["source"]["value"]["neverSent"]["commands"] = json!([]);
    read["source"]["value"]["displayContext"]["projectionPending"]["commands"] = json!([]);
    for current in [timer.clone(), Value::Null] {
        read["source"]["value"]["base"]["canonicalTimer"] = current;
        let result = call("workspace.readModel.v1", &read);
        assert_eq!(result["cadence"]["completedFocusTotal"], 2);
    }
    read["source"]["value"]["base"]["canonicalTimer"] = timer;
    read["lifecycle"]["finishEvidence"] = json!([]);
    assert!(
        dispatch_json("workspace.readModel.v1", &read.to_string())
            .unwrap_err()
            .to_string()
            .contains("consumed Finish lacks")
    );
}

#[test]
fn an_invented_marker_cannot_suppress_an_obligation_before_owner_admission() {
    let cycle = cycle();
    let mut read = cycle["read"]["input"].clone();
    read["lifecycle"] = json!({"consumedCompletions": [{"timerId": cycle["start"]["completeReturn"]["commands"][0]["timerId"],
        "commandId": "fabricated-finish", "phase": "focus"}]});
    let mut finish = cycle["finish"]["input"].clone();
    finish["lifecycle"] = read["lifecycle"].clone();
    finish["ownership"]["deviceId"] = json!("foreign");
    for (operation, input) in [
        ("workspace.readModel.v1", read),
        ("workspace.completionMutation.v1", finish),
    ] {
        let raw = dispatch_envelope_json(operation, &input.to_string());
        let envelope: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(envelope["ok"], false);
        assert!(envelope.get("value").is_none());
        assert!(
            envelope["error"]
                .as_str()
                .unwrap()
                .contains("consumed Finish lacks")
        );
    }
}

#[test]
fn new_start_resets_only_opted_in_choice_and_noop_restart_keeps_generation() {
    let cycle = cycle();
    let mut start = cycle["start"]["input"].clone();
    let result = call("workspace.intent.v1", &start);
    assert_eq!(result["selection"]["explicit"], false);
    start.as_object_mut().unwrap().remove("lifecycle");
    let legacy = call("workspace.intent.v1", &start);
    assert_eq!(legacy["selection"], start["selection"]);
    let mut restart = cycle["start"]["input"].clone();
    restart["intent"] = json!({"kind": "restart"});
    let noop = call("workspace.intent.v1", &restart);
    assert_eq!(noop["outcome"], "noop");
    assert_eq!(noop["selection"], restart["selection"]);
    assert_eq!(noop["allocation"], restart["allocation"]);
}
