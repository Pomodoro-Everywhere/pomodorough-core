use pomodorough_core::dispatch_json;
use serde_json::Value;

#[test]
fn dispatcher_exposes_versioned_cross_language_operations() {
    let version: Value =
        serde_json::from_str(&dispatch_json("core.version", "{}").unwrap()).unwrap();
    assert_eq!(
        version,
        serde_json::json!({"schemaVersion":1,"coreVersion":"0.55.0"})
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

#[test]
fn dispatcher_exposes_task_rename_operations() {
    let empty = r#"{"canonicalTimer":null,"history":[],"sessions":[],"taskOperations":[]}"#;
    let carried: Value =
        serde_json::from_str(&dispatch_json("task.carryHistory.v1", empty).unwrap()).unwrap();
    assert_eq!(
        carried,
        serde_json::json!({"canonicalTimer": null, "history": []})
    );

    let rename = |title: &str| {
        serde_json::json!({"operations": [{
            "id": "rename-1", "deviceId": "device-a", "occurredAt": "2026-03-08T07:00:00Z",
            "hlcWallMs": 1, "hlcCounter": 0,
            "taskId": "8d42fcde-20c0-8634-b2f6-4ef6a1162f71", "type": "rename", "title": title
        }]})
        .to_string()
    };
    let reduced: Value =
        serde_json::from_str(&dispatch_json("task.reduce.v1", &rename("Beta")).unwrap()).unwrap();
    assert_eq!(reduced["tasks"], serde_json::json!([]));
    assert_eq!(
        reduced["winningOperationIds"]["8d42fcde-20c0-8634-b2f6-4ef6a1162f71"],
        "rename-1"
    );
    let error = dispatch_json("task.reduce.v1", &rename(" Beta\u{0007}")).unwrap_err();
    assert!(error.to_string().contains("invalid task rename"), "{error}");
}
