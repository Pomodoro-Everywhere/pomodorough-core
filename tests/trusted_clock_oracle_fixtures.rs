use pomodorough_core::dispatch_json;
use serde_json::Value;

fn fields(actual: &Value, expected: &Value, name: &str) {
    if let Some(object) = expected.as_object() {
        for (key, value) in object {
            fields(&actual[key], value, name)
        }
    } else {
        assert_eq!(actual, expected, "{name}")
    }
}

#[test]
fn raw_native_oracle_fixtures_preserve_existing_core_clock_results() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/clock-observe-oracle-v1.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let request = case["request"].as_str().unwrap();
        let output: Value =
            serde_json::from_str(&dispatch_json("clock.observe.v1", request).unwrap()).unwrap();
        fields(&output, &case["expected"], case["name"].as_str().unwrap());
        let input: Value = serde_json::from_str(request).unwrap();
        assert!(input.get("nativeObservations").is_none());
        assert!(case["nativeObservations"].is_object());
    }
}
