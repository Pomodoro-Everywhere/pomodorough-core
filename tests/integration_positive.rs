use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

#[test]
fn task_identity_output_flows_into_task_projection() {
    let identity: Value = serde_json::from_str(
        &dispatch_json("task.identity.v1", r#"{"title":"\u0000Cafe\u0301\u001f"}"#)
            .expect("task identity"),
    )
    .expect("identity JSON");
    let operation = json!({
        "id": "operation-integration-0001",
        "deviceId": "device-integration-0001",
        "taskId": identity["id"],
        "type": "upsert",
        "title": identity["title"],
        "occurredAt": "1970-01-01T00:00:01Z",
        "hlcWallMs": 1000,
        "hlcCounter": 0
    });

    let projected: Value = serde_json::from_str(
        &dispatch_json(
            "task.reduce.v1",
            &json!({"operations": [operation]}).to_string(),
        )
        .expect("task projection"),
    )
    .expect("projection JSON");

    assert_eq!(
        projected["tasks"],
        json!([{
            "id": identity["id"],
            "title": "Café"
        }])
    );
    assert_eq!(
        projected["winningOperationIds"][identity["id"].as_str().unwrap()],
        "operation-integration-0001"
    );
}

#[test]
fn hybrid_clock_output_flows_into_uuidv7_identity() {
    let clock: Value = serde_json::from_str(
        &dispatch_json(
            "hlc.tick.v1",
            r#"{"local":{"wallMs":1700000000000,"counter":4},"physicalNowMs":1700000000001}"#,
        )
        .expect("HLC tick"),
    )
    .expect("clock JSON");
    let identity: Value = serde_json::from_str(
        &dispatch_json(
            "uuidv7.fromParts.v1",
            &json!({
                "timestampMs": clock["wallMs"],
                "randomValueHex": "0123456789abcdef012"
            })
            .to_string(),
        )
        .expect("UUIDv7 identity"),
    )
    .expect("UUID JSON");

    assert_eq!(identity["timestampMs"], clock["wallMs"]);
    assert_eq!(identity["uuid"].as_str().unwrap().len(), 36);
    assert_eq!(&identity["uuid"].as_str().unwrap()[14..15], "7");
}
