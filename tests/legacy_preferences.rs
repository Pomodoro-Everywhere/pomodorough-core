use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn request() -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/legacy-preferences-v1.json")).unwrap()
        ["request"].clone()
}

fn plan(input: &Value) -> Value {
    serde_json::from_str(
        &dispatch_json("workspace.legacyPreferences.v1", &input.to_string()).unwrap(),
    )
    .unwrap()
}

#[test]
fn legacy_import_keeps_remote_head_and_emits_exact_zero_clock_payloads() {
    let input = request();
    let result = plan(&input);
    assert_eq!(result["workspace"]["base"], input["workspace"]["base"]);
    assert_eq!(result["projection"]["durationsMs"]["focus"], 2_100_000);
    assert_eq!(result["consumedIdentityCount"], 5);
    for (index, (phase, duration)) in [
        ("focus", 1_800_000),
        ("short_break", 480_000),
        ("long_break", 1_200_000),
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            result["operations"]["durationOperations"][index],
            json!({
                "id": input["identities"]["operationUuids"][index], "ownerId": "bootstrap",
                "phase": phase, "durationMs": duration, "occurredAt": "1970-01-01T00:00:00.000Z",
                "hlcWallMs": 0, "hlcCounter": 0
            })
        );
    }
    assert_eq!(
        result["operations"]["autoStartOperations"][0],
        json!({
            "id": input["identities"]["operationUuids"][3], "enabled": false,
            "occurredAt": "1970-01-01T00:00:00.000Z", "hlcWallMs": 0, "hlcCounter": 0
        })
    );
    assert_eq!(
        result["operations"]["selectedTaskOperations"][0]["taskId"],
        Value::Null
    );
    assert!(
        result["operations"]["autoStartOperations"][0]
            .get("deviceId")
            .is_none()
    );
    assert_eq!(
        result["workspace"]["displayContext"]["projectionPending"],
        input["workspace"]["local"]
    );
}

#[test]
fn completed_markers_make_restart_an_exact_noop() {
    let mut input = request();
    let first = plan(&input);
    input["workspace"] = first["workspace"].clone();
    input["settings"] = first["settings"].clone();
    input["identities"]["operationUuids"] = json!([]);
    let restarted = plan(&input);
    assert_eq!(restarted["outcome"], "noop");
    assert_eq!(restarted["writeSettings"], false);
    assert_eq!(restarted["workspace"], input["workspace"]);
    assert_eq!(restarted["settings"], input["settings"]);
    assert_eq!(restarted["consumedIdentityCount"], 0);
    assert_eq!(restarted["effectsAfterCommit"], json!([]));
}

#[test]
fn all_five_retained_ledgers_and_proof_remain_exact() {
    let mut input = request();
    let original: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-projection-v1.json")).unwrap();
    input["workspace"] = original["request"].clone();
    input["workspace"]["displayContext"] =
        json!({"profile": "pwaStorage", "projectionPending": input["workspace"]["local"]});
    input["workspace"]["neverSent"] = json!({});
    let output = plan(&input);
    for domain in [
        "commands",
        "taskOperations",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ] {
        let retained = input["workspace"]["local"][domain].as_array().unwrap();
        let after = output["workspace"]["local"][domain].as_array().unwrap();
        assert_eq!(&after[..retained.len()], retained);
    }
    assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
    assert_eq!(
        output["workspace"]["canonicalHead"],
        input["workspace"]["canonicalHead"]
    );
    assert_eq!(
        output["workspace"]["timerDependencies"],
        input["workspace"]["timerDependencies"]
    );
    assert_eq!(
        output["workspace"]["neverSent"]["durationOperations"],
        json!([
            input["identities"]["operationUuids"][0],
            input["identities"]["operationUuids"][1],
            input["identities"]["operationUuids"][2]
        ])
    );
}

#[test]
fn migration_rejects_errors_before_returning_writes() {
    for (field, replacement) in [
        ("settings", Value::Null),
        ("deviceId", json!("")),
        ("outgoing", json!({"ownerId": "wrong"})),
    ] {
        let mut input = request();
        input[field] = replacement;
        assert!(dispatch_json("workspace.legacyPreferences.v1", &input.to_string()).is_err());
    }
    let mut input = request();
    input["settings"]["durations"]["focus"] = json!([{ "toString": null }]);
    assert!(
        dispatch_json("workspace.legacyPreferences.v1", &input.to_string())
            .unwrap_err()
            .to_string()
            .contains("cannot convert")
    );
}
