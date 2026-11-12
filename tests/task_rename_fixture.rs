//! Pins `fixtures/task-rename-v1.json`, the wire cases clients and the server
//! use to adopt the `rename` task operation and `task.carryHistory.v1`.
use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/task-rename-v1.json")).unwrap()
}

fn call(operation: &Value, input: &Value) -> Result<Value, String> {
    dispatch_json(operation.as_str().unwrap(), &input.to_string())
        .map(|output| serde_json::from_str(&output).unwrap())
        .map_err(|error| error.to_string())
}

#[test]
fn fixture_outputs_are_exact_core_results() {
    let fixture = fixture();
    for case in ["taskReduce", "carryHistory"] {
        let case = &fixture[case];
        assert_eq!(
            call(&case["operation"], &case["input"]).unwrap(),
            case["output"]
        );
    }
}

#[test]
fn fixture_rename_moves_only_the_run_that_started_before_it() {
    let fixture = fixture();
    let rename = &fixture["renameOperation"];
    let new_id = call(
        &json!("task.identity.v1"),
        &json!({"title": rename["title"]}),
    )
    .unwrap();
    let carried = &fixture["carryHistory"]["output"];
    let before = &fixture["carryHistory"]["input"]["history"];
    let moved: Vec<_> = carried["history"]
        .as_array()
        .unwrap()
        .iter()
        .zip(before.as_array().unwrap())
        .map(|(after, before)| {
            (
                before["taskId"] == rename["taskId"],
                after["taskId"] == new_id["id"],
            )
        })
        .collect();
    assert_eq!(moved, [(true, false), (true, true)]);
    assert_eq!(carried["canonicalTimer"]["taskId"], rename["taskId"]);
}

#[test]
fn fixture_invalid_renames_fail_closed() {
    let fixture = fixture();
    for case in fixture["invalidRenames"].as_array().unwrap() {
        let mut rename = fixture["renameOperation"].clone();
        rename["title"] = case["title"].clone();
        let error = call(&json!("task.reduce.v1"), &json!({"operations": [rename]})).unwrap_err();
        assert!(
            error.contains(case["error"].as_str().unwrap()),
            "{case}: {error}"
        );
    }
}
