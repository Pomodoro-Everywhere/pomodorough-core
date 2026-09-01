use std::panic::{AssertUnwindSafe, catch_unwind};

use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const OPERATION_QUEUES: [&str; 5] = [
    "commands",
    "taskOperations",
    "durationOperations",
    "autoStartOperations",
    "selectedTaskOperations",
];
const ACKNOWLEDGEMENT_QUEUES: [&str; 5] = [
    "acknowledgements",
    "taskAcknowledgements",
    "durationAcknowledgements",
    "autoStartAcknowledgements",
    "selectedTaskAcknowledgements",
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
    json!({"focus": 1_500_000, "short_break": 300_000, "long_break": 900_000})
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

fn projection() -> Value {
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

fn rebase() -> Value {
    json!({"local": queues(), "sent": queues(), "response": response(), "timerDependencies": []})
}

fn replace_pointer(mut input: Value, pointer: &str, replacement: Value) -> Value {
    *input.pointer_mut(pointer).expect("fixture pointer") = replacement;
    input
}

fn raw_at_pointer(input: Value, pointer: &str, raw: &str) -> String {
    let marked = replace_pointer(input, pointer, json!("C2_ROUND5_MARKER"));
    marked.to_string().replacen(r#""C2_ROUND5_MARKER""#, raw, 1)
}

fn assert_duplicate(operation: &str, raw: &str, field: &str) {
    let error = dispatch_json(operation, raw).unwrap_err().to_string();
    assert!(
        error.contains("duplicate field"),
        "unexpected error: {error}"
    );
    assert!(error.contains(field), "expected {field:?}, got {error}");
}

fn assert_container(operation: &str, input: Value, path: &str) {
    let error = dispatch_json(operation, &input.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains(path), "expected {path:?}, got {error}");
    assert!(
        error.contains("must be a JSON"),
        "unexpected error: {error}"
    );
}

#[test]
fn every_projection_and_rebase_queue_rejects_duplicate_operation_keys() {
    for field in OPERATION_QUEUES {
        for duplicated in [
            r#"[{"id":null,"id":"valid"}]"#,
            r#"[{"id":"valid","id":null}]"#,
        ] {
            let projection_raw =
                raw_at_pointer(projection(), &format!("/pending/{field}"), duplicated);
            assert_duplicate("projection.apply.v2", &projection_raw, "id");

            for container in ["local", "sent"] {
                let raw = raw_at_pointer(rebase(), &format!("/{container}/{field}"), duplicated);
                assert_duplicate("reconcile.rebase.v1", &raw, "id");
            }
        }
    }
}

#[test]
fn every_acknowledgement_queue_rejects_nested_unknown_duplicates() {
    for field in ACKNOWLEDGEMENT_QUEUES {
        for duplicated in [
            r#"[{"metadata":{"flags":[{"extra":0,"extra":1}]}}]"#,
            r#"[{"metadata":{"flags":[{"extra":1,"extra":0}]}}]"#,
        ] {
            let raw = raw_at_pointer(rebase(), &format!("/response/{field}"), duplicated);
            assert_duplicate("reconcile.rebase.v1", &raw, "extra");
        }
    }
}

#[test]
fn canonical_state_and_dependencies_reject_both_duplicate_orders() {
    for duplicated in [
        r#"{"lastIntent":null,"lastIntent":{}}"#,
        r#"{"lastIntent":{},"lastIntent":null}"#,
    ] {
        let projection_raw = raw_at_pointer(projection(), "/base/canonicalTimer", duplicated);
        assert_duplicate("projection.apply.v2", &projection_raw, "lastIntent");
        let rebase_raw = raw_at_pointer(rebase(), "/response/canonicalTimer", duplicated);
        assert_duplicate("reconcile.rebase.v1", &rebase_raw, "lastIntent");
    }

    for duplicated in [
        r#"[{"operationId":null,"operationId":"operation-id"}]"#,
        r#"[{"operationId":"operation-id","operationId":null}]"#,
    ] {
        let raw = raw_at_pointer(rebase(), "/timerDependencies", duplicated);
        assert_duplicate("reconcile.rebase.v1", &raw, "operationId");
    }
}

#[test]
fn projection_rejects_wrong_container_types() {
    for (pointer, replacement, path) in [
        ("/base", json!([]), "base"),
        ("/pending", json!([]), "pending"),
        ("/base/canonicalTimer", json!([]), "base.canonicalTimer"),
        ("/base/history", json!({}), "base.history"),
        ("/base/history", json!([[]]), "base.history[]"),
        ("/base/tasks", json!({}), "base.tasks"),
        ("/base/tasks", json!([[]]), "base.tasks[]"),
        ("/base/durationsMs", json!([]), "base.durationsMs"),
    ] {
        assert_container(
            "projection.apply.v2",
            replace_pointer(projection(), pointer, replacement),
            path,
        );
    }
    for field in OPERATION_QUEUES {
        for (replacement, suffix) in [(json!({}), ""), (json!([[]]), "[]")] {
            let path = format!("pending.{field}{suffix}");
            let input = replace_pointer(projection(), &format!("/pending/{field}"), replacement);
            assert_container("projection.apply.v2", input, &path);
        }
    }
}

#[test]
fn reconciliation_rejects_wrong_container_types() {
    for (pointer, replacement, path) in [
        ("/local", json!([]), "local"),
        ("/sent", json!([]), "sent"),
        ("/response", json!([]), "response"),
        ("/timerDependencies", json!({}), "timerDependencies"),
        ("/timerDependencies", json!([[]]), "timerDependencies[]"),
        (
            "/response/canonicalTimer",
            json!([]),
            "response.canonicalTimer",
        ),
        ("/response/history", json!({}), "response.history"),
        ("/response/history", json!([[]]), "response.history[]"),
        ("/response/tasks", json!({}), "response.tasks"),
        ("/response/tasks", json!([[]]), "response.tasks[]"),
        ("/response/durationsMs", json!([]), "response.durationsMs"),
    ] {
        assert_container(
            "reconcile.rebase.v1",
            replace_pointer(rebase(), pointer, replacement),
            path,
        );
    }
    let pending = replace_pointer(rebase(), "/local", json!([]));
    let mut pending = pending.as_object().unwrap().clone();
    let queues = pending.remove("local").unwrap();
    pending.insert("pending".into(), queues);
    assert_container("reconcile.rebase.v1", Value::Object(pending), "pending");
}

#[test]
fn every_rebase_nested_queue_rejects_wrong_container_types() {
    for container in ["local", "sent"] {
        for field in OPERATION_QUEUES {
            for (replacement, suffix) in [(json!({}), ""), (json!([[]]), "[]")] {
                let path = format!("{container}.{field}{suffix}");
                let input =
                    replace_pointer(rebase(), &format!("/{container}/{field}"), replacement);
                assert_container("reconcile.rebase.v1", input, &path);
            }
        }
    }
    for field in ACKNOWLEDGEMENT_QUEUES {
        for (replacement, suffix) in [(json!({}), ""), (json!([[]]), "[]")] {
            let path = format!("response.{field}{suffix}");
            let input = replace_pointer(rebase(), &format!("/response/{field}"), replacement);
            assert_container("reconcile.rebase.v1", input, &path);
        }
    }
}

#[test]
fn intentionally_optional_projection_and_queue_fields_remain_compatible() {
    let mut projection = projection();
    for field in ["canonicalTimer", "history", "tasks", "selectedTaskId"] {
        projection["base"].as_object_mut().unwrap().remove(field);
    }
    projection["pending"] = json!({});
    dispatch_json("projection.apply.v2", &projection.to_string()).unwrap();

    for local_name in ["local", "pending"] {
        let input = json!({local_name: {}, "sent": {}, "response": response()});
        dispatch_json("reconcile.rebase.v1", &input.to_string()).unwrap();
    }
}

#[test]
fn standalone_projection_adapters_use_same_strict_structure_rules() {
    for operation in [
        "task.reduce.v1",
        "duration.reduce.v1",
        "autoStart.reduce.v1",
        "selectedTask.reduce.v1",
    ] {
        assert_duplicate(operation, r#"{"operations":[{"x":0,"x":1}]}"#, "x");
        assert_container(operation, json!({"operations": {}}), "operations");
        assert_container(operation, json!({"operations": [[]]}), "operations[]");
    }
    assert_duplicate(
        "projection.reduce",
        r#"{"taskOperations":[{"x":0,"x":1}],"durationOperations":[],"autoStartOperations":[]}"#,
        "x",
    );
    assert_duplicate(
        "selectedTask.reduce",
        r#"{"operations":[{"x":0,"x":1}],"activeTaskIds":[]}"#,
        "x",
    );
}

#[test]
fn excessive_json_nesting_fails_closed_without_panicking() {
    let mut nested = "0".to_owned();
    for _ in 0..256 {
        nested = format!(r#"{{"layer":[{nested}]}}"#);
    }
    let raw = format!(r#"{{"base":{nested},"pending":{{}},"now":"2026-08-22T12:00:00Z"}}"#);
    let result = catch_unwind(AssertUnwindSafe(|| {
        dispatch_json("projection.apply.v2", &raw)
    }));
    assert!(result.is_ok(), "deep JSON must not panic");
    assert!(result.unwrap().is_err(), "deep JSON must fail closed");
}
