use pomodorough_core::dispatch_json;
use serde_json::Value;

#[test]
fn dispatcher_exposes_versioned_cross_language_operations() {
    let version: Value =
        serde_json::from_str(&dispatch_json("core.version", "{}").unwrap()).unwrap();
    assert_eq!(
        version,
        serde_json::json!({"schemaVersion":1,"coreVersion":"0.11.0"})
    );

    let selected: Value = serde_json::from_str(
        &dispatch_json(
            "selectedTask.reduce",
            r#"{"operations":[{"id":"a","deviceId":"device-a","taskId":"task-a","wallMs":1,"counter":0}],"activeTaskIds":["task-a"]}"#,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(selected, serde_json::json!({"selectedTaskId":"task-a"}));
}

#[test]
fn dispatcher_rejects_unknown_operations() {
    let error = dispatch_json("unknown", "{}").unwrap_err();
    assert_eq!(
        error.to_string(),
        "unsupported shared-core operation: unknown"
    );
}
