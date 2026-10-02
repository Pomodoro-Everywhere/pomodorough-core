use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn merge(target: &mut Value, patch: &Value) {
    if let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            merge(target.entry(key).or_insert(Value::Null), value);
        }
    } else {
        *target = patch.clone();
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/completion-sent-v1.json")).unwrap()
}

#[test]
fn android_requires_acknowledgement_timer_presence() {
    let mut input = fixture()["base"].clone();
    assert!(dispatch_json("timer.completionState.v1", &input.to_string()).is_ok());
    input["sentContext"]
        .as_object_mut()
        .unwrap()
        .remove("acknowledgementTimer");
    assert!(dispatch_json("timer.completionState.v1", &input.to_string()).is_err());
}

#[test]
fn android_requires_next_projection_timer_presence() {
    let mut input = fixture()["base"].clone();
    assert!(dispatch_json("timer.completionState.v1", &input.to_string()).is_ok());
    input["sentContext"]
        .as_object_mut()
        .unwrap()
        .remove("nextProjectionTimer");
    assert!(dispatch_json("timer.completionState.v1", &input.to_string()).is_err());
}

#[test]
fn sent_profiles_preserve_completion_plan_history_rejection() {
    let f = fixture();
    for case in f["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["error"].is_string())
    {
        let input = request(&f, case);
        let error = dispatch_json("timer.completionState.v1", &input.to_string()).unwrap_err();
        assert!(
            matches!(error, pomodorough_core::CoreError::InvalidInput(ref message)
            if message == "invalid timer history"),
            "{}: {error:?}",
            case["name"]
        );
    }
}

#[test]
fn sent_history_validation_follows_production_dispatch_boundary() {
    let f = fixture();
    for case in f["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["error"].is_string())
    {
        let mut input = request(&f, case);
        input["acknowledgements"] = json!([]);
        assert_eq!(plan(&input)["selection"], input["selection"]);
    }
    let mut input = request(&f, &f["cases"][0]);
    input["selection"]["generation"] = json!("3");
    assert_eq!(plan(&input)["reason"], "selectionChangedSinceSend");
    let mut input = request(&f, &f["cases"][1]);
    input["acknowledgements"][0]["outcome"] = json!("applied");
    assert_eq!(plan(&input)["selection"], input["selection"]);
}

fn request(f: &Value, case: &Value) -> Value {
    let mut input = f["base"].clone();
    if case["profile"] == "pwaRejectedFinish" {
        input["compatibility"] = case["profile"].clone();
        input["sentContext"] = json!({"kind":"pwa","commands":[],"rollbackHistory":[]});
        if case["history"] == true {
            input["sentContext"]["rollbackHistory"] = json!([f["history"]]);
        }
    }
    input["sentContext"]["commands"] = json!(
        case["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| f["commands"][id.as_str().unwrap()].clone())
            .collect::<Vec<_>>()
    );
    for (flag, field) in [("canonical", "canonicalTimer"), ("history", "afterHistory")] {
        if case[flag] == true {
            input[field] = if flag == "history" {
                json!([f["history"]])
            } else {
                f["timer"].clone()
            };
        }
    }
    for (flag, field) in [
        ("ackTimer", "acknowledgementTimer"),
        ("projected", "nextProjectionTimer"),
        ("ackHistory", "acknowledgementHistory"),
    ] {
        if case[flag] == true {
            input["sentContext"][field] = if flag == "ackHistory" {
                json!([f["history"]])
            } else {
                f["timer"].clone()
            };
        }
    }
    merge(&mut input, &case["patch"]);
    input
}

fn plan(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("timer.completionState.v1", &input.to_string()).unwrap())
        .unwrap()
}

#[test]
fn shared_android_pwa_captured_send_fixtures() {
    let f = fixture();
    for case in f["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["error"].is_null())
    {
        let input = request(&f, case);
        let result = plan(&input);
        assert_eq!(
            result["selection"]["phase"], case["phase"],
            "{}",
            case["name"]
        );
        assert_eq!(
            result["selection"]["generation"],
            input["selection"]["generation"]
        );
        assert_eq!(
            result["selection"]["explicit"],
            input["selection"]["explicit"]
        );
        assert!(result["advances"].as_array().unwrap().is_empty());
        assert_eq!(plan(&input), result);
    }
}

#[test]
fn restart_without_sent_commands_does_not_reconsume_response() {
    let f = fixture();
    for case in f["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["error"].is_null())
    {
        let mut input = request(&f, case);
        let result = plan(&input);
        input["selection"] = result["selection"].clone();
        input["beforeHistory"] = input["afterHistory"].clone();
        input["sentContext"]["commands"] = json!([]);
        input["acknowledgements"] = json!([]);
        assert_eq!(
            plan(&input)["selection"],
            result["selection"],
            "{}",
            case["name"]
        );
    }
}

#[test]
fn context_profile_and_raw_identity_validation() {
    let f = fixture();
    let base = request(&f, &f["cases"][3]);
    for patch in [
        json!({"sentContext":null}),
        json!({"compatibility":"appleAp04"}),
        json!({"sentContext":{"selectionAtSend":{"generation":"-1"}}}),
        json!({"sentContext":{"extra":true}}),
        json!({"advances":[{"commandId":"a","timerId":"t","previousPhase":"focus","advancedPhase":"short_break","generation":"2"}]}),
    ] {
        let mut input = base.clone();
        merge(&mut input, &patch);
        assert!(dispatch_json("timer.completionState.v1", &input.to_string()).is_err());
    }
    for patch in [
        json!({"id":""}),
        json!({"timerId":""}),
        json!({"phase":"invalid"}),
        json!({"deviceSequence":9007199254740992u64}),
        json!({"occurredAt":"invalid"}),
    ] {
        let mut input = base.clone();
        merge(&mut input["sentContext"]["commands"][0], &patch);
        assert!(dispatch_json("timer.completionState.v1", &input.to_string()).is_err());
    }
}

#[test]
fn ordered_ties_distinguish_android_id_order_from_pwa_stable_order() {
    let f = fixture();
    let mut input = request(&f, &f["cases"][9]);
    input["sentContext"]["commands"][0]["deviceSequence"] = json!(1);
    assert_eq!(plan(&input)["selection"]["phase"], "focus");
    input["compatibility"] = json!("pwaRejectedFinish");
    input["selection"]["phase"] = json!("focus");
    input["sentContext"] =
        json!({"kind":"pwa","commands":input["sentContext"]["commands"],"rollbackHistory":[]});
    assert_eq!(plan(&input)["selection"]["phase"], "focus");
    input["sentContext"]["commands"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(plan(&input)["selection"]["phase"], "short_break");
}
