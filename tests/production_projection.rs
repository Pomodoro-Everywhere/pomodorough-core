use pomodorough_core::dispatch_json;
use serde_json::{Map, Value, json};

fn timestamp(offset_ms: i64) -> String {
    let hours = 12 + offset_ms / 3_600_000;
    let minutes = offset_ms % 3_600_000 / 60_000;
    let seconds = offset_ms % 60_000 / 1_000;
    let milliseconds = offset_ms % 1_000;
    if milliseconds == 0 {
        format!("2026-07-20T{hours:02}:{minutes:02}:{seconds:02}Z")
    } else {
        let fraction = format!("{milliseconds:03}")
            .trim_end_matches('0')
            .to_owned();
        format!("2026-07-20T{hours:02}:{minutes:02}:{seconds:02}.{fraction}Z")
    }
}

fn dispatch(operation: &str, input: Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn permutations<T: Clone>(values: &[T]) -> Vec<Vec<T>> {
    if values.is_empty() {
        return vec![vec![]];
    }
    let mut result = Vec::new();
    for index in 0..values.len() {
        let mut rest = values.to_vec();
        let value = rest.remove(index);
        for mut suffix in permutations(&rest) {
            let mut permutation = vec![value.clone()];
            permutation.append(&mut suffix);
            result.push(permutation);
        }
    }
    result
}

fn operation_clock(id: &str, device_id: &str, wall: i64, counter: i64) -> Map<String, Value> {
    Map::from_iter([
        ("id".to_owned(), json!(id)),
        ("deviceId".to_owned(), json!(device_id)),
        ("occurredAt".to_owned(), json!(timestamp(0))),
        ("hlcWallMs".to_owned(), json!(wall)),
        ("hlcCounter".to_owned(), json!(counter)),
    ])
}

#[test]
fn production_projection_operations_are_versioned_and_keep_legacy_dispatch_stable() {
    for operation in [
        "task.reduce.v1",
        "duration.reduce.v1",
        "autoStart.reduce.v1",
        "selectedTask.reduce.v1",
    ] {
        dispatch(operation, json!({"operations": []}));
    }
    dispatch(
        "selectedTask.reduce",
        json!({"operations": [], "activeTaskIds": []}),
    );
    dispatch(
        "projection.reduce",
        json!({
            "taskOperations": [],
            "durationOperations": [],
            "autoStartOperations": [{
                "id": "legacy-auto",
                "deviceId": "device-a",
                "wallMs": 1,
                "counter": 0,
                "enabled": false
            }]
        }),
    );
}

#[test]
fn task_reduce_v1_is_lww_sorted_and_returns_winning_operation_ids() {
    let operations = vec![
        json!({
            "id": "operation-old",
            "deviceId": "device-a",
            "taskId": "task-a",
            "type": "upsert",
            "title": "Zulu",
            "occurredAt": timestamp(0),
            "hlcWallMs": 100,
            "hlcCounter": 0
        }),
        json!({
            "id": "operation-delete",
            "deviceId": "device-a",
            "taskId": "task-a",
            "type": "delete",
            "occurredAt": timestamp(1_000),
            "hlcWallMs": 200,
            "hlcCounter": 0
        }),
        json!({
            "id": "operation-revive",
            "deviceId": "device-a",
            "taskId": "task-a",
            "type": "upsert",
            "title": "Zulu",
            "occurredAt": timestamp(2_000),
            "hlcWallMs": 200,
            "hlcCounter": 0
        }),
        json!({
            "id": "operation-beta",
            "deviceId": "device-b",
            "taskId": "task-b",
            "type": "upsert",
            "title": "Alpha",
            "occurredAt": timestamp(3_000),
            "hlcWallMs": 300,
            "hlcCounter": 0
        }),
    ];
    let expected = json!({
        "tasks": [
            {"id": "task-b", "title": "Alpha"},
            {"id": "task-a", "title": "Zulu"}
        ],
        "winningOperationIds": {
            "task-a": "operation-revive",
            "task-b": "operation-beta"
        }
    });
    for order in permutations(&operations) {
        assert_eq!(
            dispatch("task.reduce.v1", json!({"operations": order})),
            expected
        );
    }
    assert_eq!(
        dispatch("task.reduce.v1", json!({"operations": []})),
        json!({"tasks": [], "winningOperationIds": {}})
    );
}

#[test]
fn duration_reduce_v1_applies_server_defaults_and_returns_winners_by_phase() {
    let operations = vec![
        json!({
            "id": "duration-old",
            "deviceId": "device-a",
            "phase": "focus",
            "durationMs": 1_800_000,
            "occurredAt": timestamp(0),
            "hlcWallMs": 100,
            "hlcCounter": 0
        }),
        json!({
            "id": "duration-new",
            "deviceId": "device-b",
            "phase": "focus",
            "durationMs": 2_700_000,
            "occurredAt": timestamp(1_000),
            "hlcWallMs": 100,
            "hlcCounter": 0
        }),
        json!({
            "id": "duration-short",
            "deviceId": "device-a",
            "phase": "short_break",
            "durationMs": 600_000,
            "occurredAt": timestamp(2_000),
            "hlcWallMs": 200,
            "hlcCounter": 0
        }),
    ];
    let expected = json!({
        "durationsMs": {
            "focus": 2_700_000,
            "short_break": 600_000,
            "long_break": 900_000
        },
        "winningOperationIds": {
            "focus": "duration-new",
            "short_break": "duration-short"
        }
    });
    for order in permutations(&operations) {
        assert_eq!(
            dispatch("duration.reduce.v1", json!({"operations": order})),
            expected
        );
    }
    assert_eq!(
        dispatch("duration.reduce.v1", json!({"operations": []})),
        json!({
            "durationsMs": {
                "focus": 1_500_000,
                "short_break": 300_000,
                "long_break": 900_000
            },
            "winningOperationIds": {}
        })
    );
}

#[test]
fn auto_start_reduce_v1_defaults_false_and_returns_winner() {
    let operations = vec![
        json!({
            "id": "auto-a",
            "deviceId": "device-z",
            "enabled": true,
            "occurredAt": timestamp(0),
            "hlcWallMs": 100,
            "hlcCounter": 0
        }),
        json!({
            "id": "auto-z",
            "deviceId": "device-z",
            "enabled": false,
            "occurredAt": timestamp(0),
            "hlcWallMs": 100,
            "hlcCounter": 0
        }),
    ];
    for order in permutations(&operations) {
        assert_eq!(
            dispatch("autoStart.reduce.v1", json!({"operations": order})),
            json!({"autoStartBreaks": false, "winningOperationId": "auto-z"})
        );
    }
    assert_eq!(
        dispatch("autoStart.reduce.v1", json!({"operations": []})),
        json!({"autoStartBreaks": false, "winningOperationId": null})
    );
}

#[test]
fn selected_task_reduce_v1_preserves_missing_null_and_value_semantics() {
    assert_eq!(
        dispatch(
            "selectedTask.reduce.v1",
            json!({
                "operations": [{
                    "id": "selection-clear",
                    "deviceId": "device-a",
                    "taskId": null,
                    "occurredAt": timestamp(0),
                    "hlcWallMs": 100,
                    "hlcCounter": 0
                }],
                "activeTaskIds": ["task-a"]
            })
        ),
        json!({"selectedTaskId": null, "winningOperationId": "selection-clear"})
    );
    assert_eq!(
        dispatch(
            "selectedTask.reduce.v1",
            json!({
                "operations": [{
                    "id": "selection-value",
                    "deviceId": "device-a",
                    "taskId": "task-a",
                    "occurredAt": timestamp(0),
                    "hlcWallMs": 100,
                    "hlcCounter": 0
                }],
                "activeTaskIds": ["task-a"]
            })
        ),
        json!({"selectedTaskId": "task-a", "winningOperationId": "selection-value"})
    );
    assert_eq!(
        dispatch(
            "selectedTask.reduce.v1",
            json!({
                "operations": [{
                    "id": "selection-deleted",
                    "deviceId": "device-a",
                    "taskId": "task-deleted",
                    "occurredAt": timestamp(0),
                    "hlcWallMs": 100,
                    "hlcCounter": 0
                }],
                "activeTaskIds": ["task-a"]
            })
        ),
        json!({"selectedTaskId": null, "winningOperationId": "selection-deleted"})
    );
    assert_eq!(
        dispatch(
            "selectedTask.reduce.v1",
            json!({"operations": [], "activeTaskIds": []})
        ),
        json!({"selectedTaskId": null, "winningOperationId": null})
    );

    let error = dispatch_json(
        "selectedTask.reduce.v1",
        &json!({
            "operations": [{
                "id": "selection-missing",
                "deviceId": "device-a",
                "occurredAt": timestamp(0),
                "hlcWallMs": 100,
                "hlcCounter": 0
            }],
            "activeTaskIds": []
        })
        .to_string(),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "missing required projection value: operations.taskId"
    );
}

#[test]
fn selected_task_reduce_v1_converges_by_hlc_device_and_operation_id() {
    let operations = vec![
        json!({
            "id": "selected-old",
            "deviceId": "device-z",
            "taskId": null,
            "occurredAt": timestamp(0),
            "hlcWallMs": 100,
            "hlcCounter": 0
        }),
        json!({
            "id": "selected-device",
            "deviceId": "device-z",
            "taskId": "task-a",
            "occurredAt": timestamp(0),
            "hlcWallMs": 200,
            "hlcCounter": 0
        }),
        json!({
            "id": "selected-id-z",
            "deviceId": "device-z",
            "taskId": "task-b",
            "occurredAt": timestamp(0),
            "hlcWallMs": 200,
            "hlcCounter": 1
        }),
    ];
    for order in permutations(&operations) {
        assert_eq!(
            dispatch(
                "selectedTask.reduce.v1",
                json!({"operations": order, "activeTaskIds": ["task-a", "task-b"]})
            ),
            json!({"selectedTaskId": "task-b", "winningOperationId": "selected-id-z"})
        );
    }
}

fn production_operation(operation: &Value, kind: &str) -> Value {
    let mut output = operation.as_object().unwrap().clone();
    output.insert(
        "occurredAt".to_owned(),
        json!(timestamp(operation["atMs"].as_i64().unwrap())),
    );
    output.insert("hlcWallMs".to_owned(), operation["wallMs"].clone());
    output.insert("hlcCounter".to_owned(), operation["counter"].clone());
    output.remove("atMs");
    output.remove("wallMs");
    output.remove("counter");
    if kind == "task" && output["type"] == "delete" {
        output.remove("title");
    }
    Value::Object(output)
}

#[test]
fn production_projection_reducers_match_every_fixture_permutation_and_winner() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/convergence-v1.json")).unwrap();
    for fixture_case in fixture["projectionCases"].as_array().unwrap() {
        let expected = &fixture_case["expected"];

        let tasks = fixture_case["taskOperations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|operation| production_operation(operation, "task"))
            .collect::<Vec<_>>();
        for order in permutations(&tasks) {
            let result = dispatch("task.reduce.v1", json!({"operations": order}));
            assert_eq!(result["tasks"], expected["tasks"]);
            assert_eq!(
                result["winningOperationIds"],
                json!({
                    "8d42fcde-20c0-8634-b2f6-4ef6a1162f71": "task-operation-d",
                    "dbbd578d-e71b-8d4c-8525-426366e4bb07": "task-operation-b"
                })
            );
        }

        let durations = fixture_case["durationOperations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|operation| production_operation(operation, "duration"))
            .collect::<Vec<_>>();
        for order in permutations(&durations) {
            let result = dispatch("duration.reduce.v1", json!({"operations": order}));
            assert_eq!(result["durationsMs"], expected["durationsMs"]);
            assert_eq!(
                result["winningOperationIds"],
                json!({
                    "focus": "duration-operation-b",
                    "short_break": "duration-operation-c",
                    "long_break": "duration-operation-d"
                })
            );
        }

        let auto_start = fixture_case["autoStartOperations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|operation| production_operation(operation, "autoStart"))
            .collect::<Vec<_>>();
        for order in permutations(&auto_start) {
            let result = dispatch("autoStart.reduce.v1", json!({"operations": order}));
            assert_eq!(result["autoStartBreaks"], expected["autoStartBreaks"]);
            assert_eq!(
                result["winningOperationId"],
                "00000000-0000-7000-8000-000000000003"
            );
        }
    }
}

#[test]
fn production_projection_operations_decode_real_occurred_at_and_hlc_fields() {
    let mut task = operation_clock("task-operation", "device-a", 10, 2);
    task.extend([
        ("taskId".to_owned(), json!("task-a")),
        ("type".to_owned(), json!("upsert")),
        ("title".to_owned(), json!("Alpha")),
    ]);
    assert_eq!(
        dispatch(
            "task.reduce.v1",
            json!({"operations": [Value::Object(task)]})
        )["winningOperationIds"]["task-a"],
        "task-operation"
    );
}
