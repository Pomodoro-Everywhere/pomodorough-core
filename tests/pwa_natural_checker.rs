use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/pwa-natural-checker-v1.json")).unwrap()
}

fn corrected(case: &Value) -> Value {
    serde_json::from_str(&dispatch_envelope_json(
        case["operation"].as_str().unwrap(),
        case["inputRaw"].as_str().unwrap(),
    ))
    .unwrap()
}

fn install(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("timer.completionState.v1", &input.to_string()).unwrap())
        .unwrap()
}

#[test]
fn checker_exact_twelve_failures_are_fixed_without_partial_results_or_input_changes() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 12);
    for case in cases {
        let input: Value = serde_json::from_str(case["inputRaw"].as_str().unwrap()).unwrap();
        let untouched = input.clone();
        let result = corrected(case);
        assert_eq!(case["priorReturn"]["ok"], true);
        if case["name"].as_str().unwrap().starts_with("late-ack-") {
            assert_eq!(result["ok"], true);
            assert_eq!(
                result["value"]["selection"], input["selection"],
                "{}",
                case["name"]
            );
            assert_eq!(result["value"]["lifecycle"], input["lifecycle"]);
            assert_eq!(case["priorReturn"]["value"]["selection"]["phase"], "focus");
        } else {
            assert_eq!(result["ok"], false, "{}", case["name"]);
            assert_eq!(result.as_object().unwrap().len(), 2);
            assert!(
                result.get("value").is_none(),
                "no partial plan: {}",
                case["name"]
            );
        }
        assert_eq!(input, untouched);
    }
}

fn replaced_request() -> Value {
    let raw = fixture()["cases"][0]["inputRaw"]
        .as_str()
        .unwrap()
        .to_owned();
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn consumed_natural_session_survives_all_acknowledgements_provenance_and_selection_versions() {
    for intent in ["start", "resume", "finish", "absent"] {
        for outcome in ["applied", "ignored", "rejected"] {
            for explicit in [false, true] {
                for generation in ["0", "20", "9223372036854775807"] {
                    let mut input = replaced_request();
                    input["selection"] = json!({"phase": "short_break", "generation": generation, "explicit": explicit});
                    if intent == "absent" {
                        input["canonicalTimer"]
                            .as_object_mut()
                            .unwrap()
                            .remove("lastIntent");
                    } else {
                        input["canonicalTimer"]["lastIntent"]["type"] = json!(intent);
                        input["canonicalTimer"]["lastIntent"]["commandId"] =
                            json!("remote-provenance");
                    }
                    if intent == "finish" {
                        input["afterHistory"][0]["commandId"] = json!("remote-provenance");
                    } else {
                        input["afterHistory"][0]
                            .as_object_mut()
                            .unwrap()
                            .remove("commandId");
                    }
                    input["lifecycle"]["consumedCompletions"]
                        .as_array_mut()
                        .unwrap()
                        .truncate(1);
                    input["acknowledgements"][0]["outcome"] = json!(outcome);
                    let result = install(&input);
                    assert_eq!(result["selection"], input["selection"]);
                    assert_eq!(result["lifecycle"], input["lifecycle"]);
                    let restarted: Value = serde_json::from_str(&input.to_string()).unwrap();
                    assert_eq!(install(&restarted), result);
                    assert_counts(&input);
                }
            }
        }
    }
}

fn assert_counts(input: &Value) {
    let raw: Value =
        serde_json::from_str(include_str!("../fixtures/pwa-natural-completion-v1.json")).unwrap();
    let mut workspace = raw["receipts"][0]["request"]["input"]["workspace"].clone();
    workspace["base"]["canonicalTimer"] = input["canonicalTimer"].clone();
    workspace["base"]["history"] = input["afterHistory"].clone();
    let request = json!({"profile": "pwaStorage", "source": {"kind": "workspace", "value": workspace},
        "selection": input["selection"], "selectedPhase": input["selection"]["phase"], "lifecycle": input["lifecycle"],
        "observedAt": input["referenceTime"], "calendarIntervals": input["calendarIntervals"]});
    let output: Value = serde_json::from_str(
        &dispatch_json("workspace.readModel.v1", &request.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(output["cadence"]["completedFocusTotal"], 1);
    assert_eq!(output["cadence"]["completedFocusToday"], 1);
    assert_eq!(
        output["cadence"]["completedFocusTodayPlannedDurationMs"],
        1500000
    );
}

#[test]
fn null_consumption_preserves_new_implicit_phase_after_corrected_terminal_time_and_restart() {
    for phase in ["focus", "short_break", "long_break"] {
        let mut input = replaced_request();
        input["selection"] = json!({"phase": phase, "generation": "20", "explicit": false});
        input["lifecycle"]["consumedCompletions"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        input["canonicalTimer"]["anchorAt"] = json!("2026-08-31T12:25:01Z");
        input["canonicalTimer"]["lastIntent"]["occurredAt"] =
            input["canonicalTimer"]["anchorAt"].clone();
        input["afterHistory"][0]["completedAt"] = input["canonicalTimer"]["anchorAt"].clone();
        input["afterHistory"][0]["endedAt"] = input["canonicalTimer"]["anchorAt"].clone();
        input["referenceTime"] = json!("2026-08-31T12:25:02Z");
        let first = install(&input);
        assert_eq!(first["selection"], input["selection"]);
        input["lifecycle"] = first["lifecycle"].clone();
        input = serde_json::from_str(&input.to_string()).unwrap();
        assert_eq!(install(&input), first);
        assert_counts(&input);
    }
}

#[test]
fn consumed_natural_identity_does_not_protect_another_session_or_phase() {
    for field in ["timerId", "phase"] {
        let mut input = replaced_request();
        for item in input["lifecycle"]["consumedCompletions"]
            .as_array_mut()
            .unwrap()
        {
            item[field] = json!(if field == "phase" {
                "long_break"
            } else {
                "other-timer"
            });
        }
        input["beforeHistory"] = json!([]);
        input["selection"]["explicit"] = json!(false);
        assert_eq!(install(&input)["selection"]["phase"], "focus");
    }
}

#[test]
fn raw_extension_arrays_and_nullable_provenance_fields_keep_existing_semantics() {
    let raw: Value =
        serde_json::from_str(include_str!("../fixtures/pwa-natural-completion-v1.json")).unwrap();
    let mut input = raw["receipts"][0]["request"]["input"].clone();
    let extensions = json!({"kept": [null, [], {"nested": [1, false, ""]}]});
    input["workspace"]["base"]["canonicalTimer"]["extension"] = extensions.clone();
    input["workspace"]["base"]["canonicalTimer"]["lastIntent"]["extension"] = extensions.clone();
    input["workspace"]["base"]["history"][0]["extension"] = extensions;
    input["workspace"]["base"]["canonicalTimer"]["taskId"] = Value::Null;
    input["workspace"]["base"]["history"][0]["taskId"] = Value::Null;
    input["workspace"]["base"]["history"][0]["commandId"] = Value::Null;
    input["requestedTimer"] = input["workspace"]["base"]["canonicalTimer"].clone();
    let output: Value = serde_json::from_str(
        &dispatch_json("workspace.completionMutation.v1", &input.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(output["outcome"], "planned");
    assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
    assert!(
        output["workspace"]["base"]["history"][0]
            .get("commandId")
            .unwrap()
            .is_null()
    );
    assert_eq!(output["projection"]["history"].as_array().unwrap().len(), 1);
}
