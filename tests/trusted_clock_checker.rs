use pomodorough_core::dispatch_json;
use serde_json::Value;

fn check(group: &str) {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/clock-observe-checker-v1.json")).unwrap();
    for case in fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["group"] == group)
    {
        let result = dispatch_json("clock.observe.v1", case["request"].as_str().unwrap());
        let name = case["name"].as_str().unwrap();
        if case["reject"] == true {
            assert!(result.is_err(), "{name}: {result:?}");
            continue;
        }
        let actual: Value =
            serde_json::from_str(&result.unwrap_or_else(|error| panic!("{name}: {error}")))
                .unwrap();
        fields(&actual, &case["expected"], name);
    }
}

fn fields(actual: &Value, expected: &Value, name: &str) {
    if let Some(object) = expected.as_object() {
        for (key, value) in object {
            fields(&actual[key], value, name)
        }
    } else if actual.is_number() && expected.is_number() {
        assert_eq!(actual.as_f64(), expected.as_f64(), "{name}")
    } else {
        assert_eq!(actual, expected, "{name}")
    }
}

#[test]
fn checker_rejects_raw_fractional_integral_readings() {
    check("tokens")
}
#[test]
fn checker_rejects_profile_observation_extensions_including_null() {
    check("profiles")
}
#[test]
fn checker_desktop_formatter_overflow_keeps_original_anchor() {
    mapping("desktop-format-year-overflow-keeps-original")
}
#[test]
fn checker_android_mapper_accepts_pre_epoch_instants() {
    mapping("android-pre-epoch-physical-anchor")
}
#[test]
fn checker_pwa_mapper_keeps_trusted_anchor() {
    mapping("pwa-anchor-remains-trusted-domain")
}
#[test]
fn checker_apple_midpoint_is_not_a_persisted_safe_integer() {
    check("appleMidpoint")
}
#[test]
fn checker_apple_raw_date_preserves_native_fractional_anchor() {
    check("appleDate")
}
#[test]
fn checker_desktop_absent_receipt_timings_keep_sample_and_server_context() {
    check("desktopReceipt")
}

fn mapping(name: &str) {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/clock-observe-checker-v1.json")).unwrap();
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap();
    let result = dispatch_json("clock.observe.v1", case["request"].as_str().unwrap()).unwrap();
    fields(
        &serde_json::from_str(&result).unwrap(),
        &case["expected"],
        name,
    );
}

fn integral_request(profile: &str, field: &str, token: &str) -> String {
    let state = if profile == "desktopTrustedClock" {
        r#"{"sample":null}"#
    } else {
        "{}"
    };
    let (action, observation) = if field == "monotonicMs" {
        (
            "current",
            format!(r#""reading":{{"wallMs":1000000,"monotonicMs":{token}}}"#),
        )
    } else {
        let sent = if field == "requestMonotonicMs" {
            token
        } else {
            "20000"
        };
        let received = if field == "responseMonotonicMs" {
            token
        } else {
            "20003"
        };
        (
            "sample",
            format!(
                r#""server":{{"serverTimeMs":1000100,"serverHlcWallMs":1000100,
            "requestWallMs":1000000,"responseWallMs":1000003,
            "requestMonotonicMs":{sent},"responseMonotonicMs":{received}}}"#
            ),
        )
    };
    format!(
        r#"{{"schemaVersion":1,"compatibility":"{profile}","action":"{action}","state":{state},{observation}}}"#
    )
}

#[test]
fn checker_integral_tokens_are_exact_across_current_and_both_receipt_readings() {
    for profile in ["androidTrustedClock", "desktopTrustedClock"] {
        for field in ["monotonicMs", "requestMonotonicMs", "responseMonotonicMs"] {
            for token in [
                "20000.000000000001",
                "9007199254740991.1",
                "9007199254740992",
                "20000.0",
                "2e4",
                "\"20000\"",
                "true",
                "-1",
            ] {
                let input = integral_request(profile, field, token);
                assert!(
                    dispatch_json("clock.observe.v1", &input).is_err(),
                    "{profile}/{field}/{token}"
                );
            }
        }
        for token in ["0", "20000", "9007199254740991"] {
            assert!(
                dispatch_json(
                    "clock.observe.v1",
                    &integral_request(profile, "monotonicMs", token)
                )
                .is_ok()
            );
        }
    }
}

#[test]
fn checker_profile_presence_matrix_rejects_null_and_populated_foreign_observations() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/clock-observe-v1.json")).unwrap();
    for (profile, reading_fields, server_fields) in [
        (
            "appleTrustedClock",
            vec!["wallMs", "monotonicMs", "bootId"],
            vec![
                "responseWallMs",
                "requestMonotonicMs",
                "responseMonotonicMs",
                "requestSequence",
            ],
        ),
        (
            "androidTrustedClock",
            vec!["wallSeconds", "uptimeSeconds"],
            vec![
                "requestUptimeSeconds",
                "responseUptimeSeconds",
                "requestSequence",
            ],
        ),
        (
            "desktopTrustedClock",
            vec!["wallSeconds", "uptimeSeconds", "bootId"],
            vec![
                "requestUptimeSeconds",
                "responseUptimeSeconds",
                "requestSequence",
            ],
        ),
        (
            "pwaTrustedClock",
            vec!["wallSeconds", "uptimeSeconds", "bootId"],
            vec![
                "requestUptimeSeconds",
                "responseUptimeSeconds",
                "requestMonotonicMs",
                "responseMonotonicMs",
            ],
        ),
    ] {
        for (container, fields) in [("reading", reading_fields), ("server", server_fields)] {
            for field in fields {
                for value in [Value::Null, Value::from(1)] {
                    let mut input = fixture["templates"][profile].clone();
                    input["action"] = Value::from("sample");
                    input["server"] = fixture["servers"][profile].clone();
                    input[container][field] = value;
                    assert!(
                        dispatch_json("clock.observe.v1", &input.to_string()).is_err(),
                        "{profile}/{container}/{field}"
                    );
                }
            }
        }
    }
}

#[test]
fn checker_raw_date_observations_are_exclusive_and_apple_only_even_when_null() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/clock-observe-v1.json")).unwrap();
    for profile in [
        "appleTrustedClock",
        "androidTrustedClock",
        "desktopTrustedClock",
        "pwaTrustedClock",
    ] {
        for value in [Value::Null, Value::from(1000.1009)] {
            let mut input = fixture["templates"][profile].clone();
            input["trustedAnchorSeconds"] = value;
            if profile != "appleTrustedClock" {
                assert!(dispatch_json("clock.observe.v1", &input.to_string()).is_err());
            }
            input["trustedAnchorMs"] = Value::Null;
            assert!(dispatch_json("clock.observe.v1", &input.to_string()).is_err());
        }
    }
}
