use pomodorough_core::dispatch_envelope_json;
use serde_json::Value;

#[test]
fn binding_envelope_contains_json_value_or_error() {
    let success: Value =
        serde_json::from_str(&dispatch_envelope_json("core.version", "{}")).unwrap();
    assert_eq!(success["ok"], true);
    assert_eq!(success["value"]["schemaVersion"], 1);
    assert_eq!(success["value"]["coreVersion"], "0.1.4");

    let failure: Value =
        serde_json::from_str(&dispatch_envelope_json("missing.operation", "{}")).unwrap();
    assert_eq!(failure["ok"], false);
    assert_eq!(
        failure["error"],
        "unsupported shared-core operation: missing.operation"
    );
}
