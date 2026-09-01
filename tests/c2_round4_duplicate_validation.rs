use pomodorough_core::{CoreError, dispatch_json};
use serde_json::{Value, json};

const ACKNOWLEDGEMENT_DOMAINS: [(&str, &str, &str); 5] = [
    ("commands", "acknowledgements", "commandId"),
    ("taskOperations", "taskAcknowledgements", "operationId"),
    (
        "durationOperations",
        "durationAcknowledgements",
        "operationId",
    ),
    (
        "autoStartOperations",
        "autoStartAcknowledgements",
        "operationId",
    ),
    (
        "selectedTaskOperations",
        "selectedTaskAcknowledgements",
        "operationId",
    ),
];

fn queues() -> Value {
    json!({
        "commands": [],
        "taskOperations": [],
        "durationOperations": [],
        "autoStartOperations": [],
        "selectedTaskOperations": []
    })
}

fn durations() -> Value {
    json!({
        "focus": 1_500_000,
        "short_break": 300_000,
        "long_break": 900_000
    })
}

fn response() -> Value {
    json!({
        "acknowledgements": [],
        "taskAcknowledgements": [],
        "durationAcknowledgements": [],
        "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [],
        "revision": 1,
        "canonicalTimer": null,
        "history": [],
        "tasks": [],
        "durationsMs": durations(),
        "autoStartBreaks": false,
        "selectedTaskId": null,
        "serverTime": "2026-07-20T12:00:10Z",
        "serverHlcWallMs": 1_784_548_810_000_i64,
        "serverHlcCounter": 0
    })
}

fn rebase_input() -> Value {
    json!({"local": queues(), "sent": queues(), "response": response()})
}

fn projection_input() -> Value {
    json!({
        "base": {
            "canonicalTimer": null,
            "history": [],
            "tasks": [],
            "durationsMs": durations(),
            "autoStartBreaks": false,
            "selectedTaskId": null
        },
        "pending": queues(),
        "now": "2026-08-22T12:00:00Z"
    })
}

fn assert_duplicate_error(operation: &str, raw: &str, field: &str) {
    let error = dispatch_json(operation, raw).unwrap_err().to_string();
    assert!(
        error.contains("duplicate field"),
        "unexpected error: {error}"
    );
    assert!(error.contains(field), "expected {field:?}, got {error}");
}

fn acknowledgement_raw(id_field: &str, duplicate_field: &str) -> String {
    let identity = format!(r#""{id_field}":"sent-id""#);
    let duplicate = match duplicate_field {
        "identity" => format!(r#"{identity},"{id_field}":"other-id""#),
        "outcome" => r#""outcome":"applied","outcome":"ignored""#.into(),
        "reason" => r#""reason":"","reason":"duplicate""#.into(),
        "metadata" => r#""metadata":true,"metadata":false"#.into(),
        _ => unreachable!(),
    };
    let fields = match duplicate_field {
        "identity" => format!(r#"{duplicate},"outcome":"applied","reason":"""#),
        "outcome" => format!(r#"{identity},{duplicate},"reason":"""#),
        "reason" => format!(r#"{identity},"outcome":"applied",{duplicate}"#),
        "metadata" => format!(r#"{identity},"outcome":"applied","reason":"",{duplicate}"#),
        _ => unreachable!(),
    };
    format!("{{{fields}}}")
}

fn raw_rebase_with_acknowledgement(
    sent_field: &str,
    acknowledgement_field: &str,
    acknowledgement: &str,
) -> String {
    let mut input = rebase_input();
    input["sent"][sent_field] = json!([{"id": "sent-id"}]);
    input["response"][acknowledgement_field] = json!("ACKNOWLEDGEMENT_MARKER");
    input.to_string().replacen(
        r#""ACKNOWLEDGEMENT_MARKER""#,
        &format!("[{acknowledgement}]"),
        1,
    )
}

fn malformed_clock_operation(fields: Value) -> Value {
    let mut operation = json!({
        "id": "operation-id",
        "deviceId": "device-id",
        "occurredAt": "not-a-timestamp",
        "hlcWallMs": 1,
        "hlcCounter": 0
    });
    operation.as_object_mut().unwrap().extend(
        fields
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    operation
}

#[test]
fn duplicate_inner_duration_keys_are_rejected_before_map_collapse() {
    for duplicated in [
        r#""focus":1,"focus":1500000,"short_break":300000,"long_break":900000"#,
        r#""focus":1500000,"focus":1,"short_break":300000,"long_break":900000"#,
    ] {
        let projection = projection_input();
        let durations = projection["base"]["durationsMs"].to_string();
        let target = format!(r#""durationsMs":{durations}"#);
        let replacement = format!(r#""durationsMs":{{{duplicated}}}"#);
        let raw = projection.to_string().replacen(&target, &replacement, 1);
        assert_duplicate_error("projection.apply.v2", &raw, "focus");

        let rebase = rebase_input();
        let durations = rebase["response"]["durationsMs"].to_string();
        let target = format!(r#""durationsMs":{durations}"#);
        let raw = rebase.to_string().replacen(&target, &replacement, 1);
        assert_duplicate_error("reconcile.rebase.v1", &raw, "focus");
    }
}

#[test]
fn duplicate_root_response_is_rejected_before_required_field_checks() {
    let input = rebase_input();
    let response = input["response"].to_string();
    let target = format!(r#""response":{response}"#);
    for replacement in [
        format!(r#""response":null,{target}"#),
        format!(r#"{target},"response":null"#),
    ] {
        let raw = input.to_string().replacen(&target, &replacement, 1);
        assert_duplicate_error("reconcile.rebase.v1", &raw, "response");
    }
}

#[test]
fn every_acknowledgement_object_rejects_duplicate_known_and_unknown_fields() {
    for (sent_field, acknowledgement_field, id_field) in ACKNOWLEDGEMENT_DOMAINS {
        for duplicate_field in ["identity", "outcome", "reason", "metadata"] {
            let acknowledgement = acknowledgement_raw(id_field, duplicate_field);
            let raw = raw_rebase_with_acknowledgement(
                sent_field,
                acknowledgement_field,
                &acknowledgement,
            );
            let expected = if duplicate_field == "identity" {
                id_field
            } else {
                duplicate_field
            };
            assert_duplicate_error("reconcile.rebase.v1", &raw, expected);
        }
    }
}

#[test]
fn acknowledgements_still_allow_unique_unknown_fields() {
    for (sent_field, acknowledgement_field, id_field) in ACKNOWLEDGEMENT_DOMAINS {
        let acknowledgement = format!(
            r#"{{"{id_field}":"sent-id","outcome":"applied","reason":"","metadata":true}}"#
        );
        let raw =
            raw_rebase_with_acknowledgement(sent_field, acknowledgement_field, &acknowledgement);
        dispatch_json("reconcile.rebase.v1", &raw).unwrap();
    }
}

#[test]
fn standalone_reducers_preserve_invalid_timestamp_errors() {
    for (operation, fields, active_task_ids) in [
        (
            "task.reduce.v1",
            json!({"taskId": "task-id", "type": "delete"}),
            None,
        ),
        (
            "duration.reduce.v1",
            json!({"phase": "focus", "durationMs": 60_000}),
            None,
        ),
        ("autoStart.reduce.v1", json!({"enabled": true}), None),
        (
            "selectedTask.reduce.v1",
            json!({"taskId": null}),
            Some(json!([])),
        ),
    ] {
        let mut input = json!({"operations": [malformed_clock_operation(fields)]});
        if let Some(active_task_ids) = active_task_ids {
            input["activeTaskIds"] = active_task_ids;
        }
        let error = dispatch_json(operation, &input.to_string()).unwrap_err();
        assert!(
            matches!(&error, CoreError::InvalidTimestamp(timestamp) if timestamp == "not-a-timestamp"),
            "{operation} returned {error}"
        );
    }
}

#[test]
fn projection_keeps_strict_invalid_clock_mapping() {
    let mut input = projection_input();
    input["pending"]["autoStartOperations"] =
        json!([malformed_clock_operation(json!({"enabled": true}))]);
    let error = dispatch_json("projection.apply.v2", &input.to_string()).unwrap_err();
    assert!(
        matches!(&error, CoreError::InvalidInput(message) if message == "invalid operation clock")
    );
}
