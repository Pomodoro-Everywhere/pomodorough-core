use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

const OPERATION: &str = "workspace.legacyPreferences.v1";

fn input() -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/legacy-preferences-v1.json")).unwrap()
        ["request"].clone()
}

#[test]
fn numeric_neighbors_and_string_controls_use_correct_binary64_rounding() {
    for (token, expected) in [
        ("90.49999999999999", 5_400_000),
        ("90.5", 5_460_000),
        ("90.50000000000001", 5_460_000),
    ] {
        for value in [json!(token.parse::<f64>().unwrap()), json!(token)] {
            let mut input = input();
            input["settings"] = json!({"durations": {
                "focus": value, "short_break": value, "long_break": value}});
            let raw = dispatch_json(OPERATION, &input.to_string()).unwrap();
            let result: Value = serde_json::from_str(&raw).unwrap();
            for phase in ["focus", "short_break", "long_break"] {
                let operation = result["operations"]["durationOperations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|operation| operation["phase"] == phase)
                    .unwrap();
                assert_eq!(operation["durationMs"], expected);
            }
        }
    }
}

#[test]
fn output_and_binding_envelope_preserve_unknown_numeric_fields() {
    let mut input = input();
    input["settings"] = json!({"durationSyncBootstrapped": true, "autoStartSyncBootstrapped": true,
        "selectedTaskSyncBootstrapped": true, "peerOnlySetting": {
            "number": "90.49999999999999".parse::<f64>().unwrap()}});
    input["identities"]["operationUuids"] = json!([]);
    let wire = input.to_string();
    let output = dispatch_json(OPERATION, &wire).unwrap();
    assert!(output.contains("\"number\":90.49999999999999"));
    assert!(output.contains("\"writeSettings\":false"));
    assert_eq!(
        dispatch_envelope_json(OPERATION, &wire),
        format!("{{\"ok\":true,\"value\":{output}}}")
    );
}

#[test]
fn duplicate_keys_integer_tokens_depth_and_range_errors_still_fail_closed() {
    let input = input().to_string();
    for token in ["1.0", "9007199254740992", "18446744073709551616", "1e400"] {
        let raw = input.replacen("\"counter\":0", &format!("\"counter\":{token}"), 1);
        assert!(dispatch_json(OPERATION, &raw).is_err(), "{token}");
    }
    for raw in [
        "{\"duplicate\":90.49999999999999,\"duplicate\":0}".into(),
        format!("{}0{}", "[".repeat(128), "]".repeat(128)),
        format!("{input} trailing"),
    ] {
        assert!(dispatch_json(OPERATION, &raw).is_err());
    }
}
