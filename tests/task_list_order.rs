//! The order of the task list in Core output: natural, case-insensitive title
//! order, through the public dispatch operations that return a task list.
use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn task(title: &str) -> Value {
    call("task.identity.v1", &json!({ "title": title }))
}

fn upsert(title: &str, wall: i64) -> Value {
    let task = task(title);
    json!({
        "id": format!("upsert-{wall}"),
        "deviceId": "device-a",
        "occurredAt": "2026-08-22T12:00:00Z",
        "hlcWallMs": wall,
        "hlcCounter": 0,
        "taskId": task["id"],
        "type": "upsert",
        "title": task["title"]
    })
}

fn upserts<'a>(titles: impl Iterator<Item = &'a str>) -> Vec<Value> {
    titles
        .zip(2_000..)
        .map(|(title, wall)| upsert(title, wall))
        .collect()
}

fn titles(output: &Value) -> Vec<&str> {
    output["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap())
        .collect()
}

/// Reduces the titles from the given order and from its reverse; the list
/// must not depend on the order in which the operations arrive.
fn assert_reduced_list(expected: &[&str]) {
    for arrival in [
        upserts(expected.iter().copied()),
        upserts(expected.iter().rev().copied()),
    ] {
        let output = call("task.reduce.v1", &json!({ "operations": arrival }));
        assert_eq!(titles(&output), expected);
    }
}

#[test]
fn task_reduce_lists_bulk_tasks_by_number() {
    let bulk = (1..=12)
        .map(|number| format!("Bulk task {number}"))
        .collect::<Vec<_>>();
    assert_reduced_list(&bulk.iter().map(String::as_str).collect::<Vec<_>>());
}

#[test]
fn task_reduce_lists_titles_without_regard_to_case() {
    assert_reduced_list(&["apple", "Banana", "cherry 2", "Cherry 10", "Zebra"]);
    assert_reduced_list(&["École 2", "école 10", "Яблоко 2", "яблоко 10"]);
}

#[test]
fn task_reduce_lists_capital_sharp_s_with_sharp_s() {
    assert_reduced_list(&["STRASSE 2", "STRAẞE 3", "Straße 4", "strasse 10"]);
}

#[test]
fn task_reduce_ranks_digits_and_letters_among_ascii_punctuation() {
    assert_reduced_list(&[
        "Task/", "Task1", "Task10", "Task:", "Task@", "Task[", "Task_1", "Task`", "TaskA", "Taska",
        "Task{",
    ]);
}

#[test]
fn task_reduce_folds_dotted_and_dotless_i_without_a_turkish_rule() {
    assert_reduced_list(&["I 2", "ı 3", "i 10", "iz", "İ 4", "j"]);
}

#[test]
fn task_reduce_compares_long_and_zero_padded_numbers_by_value() {
    let above_u64 = format!("Run {}0", u64::MAX);
    assert_reduced_list(&[
        "Run 007",
        "Run 7",
        "Run 08",
        &format!("Run {}", u64::MAX),
        &above_u64,
    ]);
}

#[test]
fn projection_apply_v2_lists_base_and_pending_tasks_in_one_order() {
    let base = ["Task 10", "task 2"].map(task);
    let input = json!({
        "base": {
            "canonicalTimer": null,
            "history": [],
            "tasks": base,
            "durationsMs": { "focus": 1_200_000, "short_break": 300_000, "long_break": 900_000 },
            "autoStartBreaks": false,
            "selectedTaskId": null
        },
        "pending": {
            "commands": [],
            "taskOperations": upserts(["Task 9", "TASK 1"].into_iter()),
            "durationOperations": [],
            "autoStartOperations": [],
            "selectedTaskOperations": []
        },
        "now": "2026-08-22T12:00:00Z"
    });
    let output = call("projection.apply.v2", &input);
    assert_eq!(titles(&output), ["TASK 1", "task 2", "Task 9", "Task 10"]);
}
