use pomodorough_core::dispatch_json;
use serde_json::Value;

#[test]
fn hlc_tick_and_observe_follow_hybrid_logical_clock_rules() {
    let cases = [
        (
            r#"{"local":{"wallMs":100,"counter":2},"physicalNowMs":101}"#,
            serde_json::json!({"wallMs":101,"counter":0}),
        ),
        (
            r#"{"local":{"wallMs":100,"counter":2},"physicalNowMs":99}"#,
            serde_json::json!({"wallMs":100,"counter":3}),
        ),
        (
            r#"{"local":{"wallMs":100,"counter":2},"remote":{"wallMs":100,"counter":7},"physicalNowMs":99}"#,
            serde_json::json!({"wallMs":100,"counter":8}),
        ),
        (
            r#"{"local":{"wallMs":100,"counter":2},"remote":{"wallMs":120,"counter":4},"physicalNowMs":110}"#,
            serde_json::json!({"wallMs":120,"counter":5}),
        ),
        (
            r#"{"local":{"wallMs":120,"counter":4},"remote":{"wallMs":100,"counter":9},"physicalNowMs":110}"#,
            serde_json::json!({"wallMs":120,"counter":5}),
        ),
        (
            r#"{"local":{"wallMs":100,"counter":2},"remote":{"wallMs":110,"counter":9},"physicalNowMs":120}"#,
            serde_json::json!({"wallMs":120,"counter":0}),
        ),
    ];

    for (input, expected) in cases {
        let actual: Value =
            serde_json::from_str(&dispatch_json("hlc.tick.v1", input).unwrap()).unwrap();
        assert_eq!(actual, expected, "input {input}");
    }
}

#[test]
fn hlc_rejects_negative_and_javascript_unsafe_values() {
    for input in [
        r#"{"local":{"wallMs":-1,"counter":0},"physicalNowMs":0}"#,
        r#"{"local":{"wallMs":0,"counter":-1},"physicalNowMs":0}"#,
        r#"{"local":{"wallMs":9007199254740992,"counter":0},"physicalNowMs":0}"#,
    ] {
        assert!(
            dispatch_json("hlc.tick.v1", input).is_err(),
            "input {input}"
        );
    }
}

#[test]
fn uuidv7_from_parts_matches_rfc9562_fixture() {
    let fixture: Value =
        serde_json::from_slice(include_bytes!("../fixtures/uuidv7-v1.json")).unwrap();
    let input = serde_json::json!({
        "timestampMs": fixture["rfc9562"]["timestampMs"],
        "randomValueHex": fixture["rfc9562"]["randomValueHex"],
    });
    let actual: Value =
        serde_json::from_str(&dispatch_json("uuidv7.fromParts.v1", &input.to_string()).unwrap())
            .unwrap();
    assert_eq!(actual["uuid"], fixture["rfc9562"]["uuid"]);
    assert_eq!(actual["timestampMs"], fixture["rfc9562"]["timestampMs"]);
}
