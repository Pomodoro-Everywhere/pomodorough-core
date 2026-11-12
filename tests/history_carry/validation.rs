//! `task.carryHistory.v1` fails closed on malformed input.
use super::*;

fn valid_input() -> Value {
    let reduced = reduce(finished("timer-b", "Café", 0, 25).to_vec());
    json!({
        "canonicalTimer": reduced["canonicalTimer"],
        "history": reduced["history"],
        "sessions": reduced["sessions"],
        "taskOperations": [rename("r1", "device-a", "Café", "Espresso", 30)]
    })
}

fn rejects(input: &Value, expected: &str) {
    let error = call("task.carryHistory.v1", input).unwrap_err();
    assert!(
        error.contains(expected),
        "expected {expected:?}, got {error}"
    );
}

#[test]
fn carry_history_accepts_the_valid_input() {
    let carried = call("task.carryHistory.v1", &valid_input()).unwrap();
    assert_eq!(
        tasks_by_timer(&carried["history"])["timer-b"],
        id("Espresso")
    );
}

#[test]
fn carry_history_rejects_unknown_and_missing_top_level_fields() {
    let mut unknown = valid_input();
    unknown["now"] = json!(at(30));
    rejects(&unknown, "unknown field `now`");
    for field in ["history", "sessions", "taskOperations"] {
        let mut missing = valid_input();
        missing.as_object_mut().unwrap().remove(field);
        rejects(&missing, &format!("{field} must be a JSON array"));
        let mut wrong = valid_input();
        wrong[field] = json!({});
        rejects(&wrong, &format!("{field} must be a JSON array"));
    }
    let mut missing_timer = valid_input();
    missing_timer
        .as_object_mut()
        .unwrap()
        .remove("canonicalTimer");
    rejects(
        &missing_timer,
        "canonicalTimer must be a JSON object or null",
    );
    let mut null_timer = valid_input();
    null_timer["canonicalTimer"] = Value::Null;
    assert!(call("task.carryHistory.v1", &null_timer).is_ok());
    rejects(&json!([]), "carry input must be a JSON object");
}

/// A session field the reducer never writes fails closed, both in the carry
/// input and in the replay page that reads the same session form back.
#[test]
fn sessions_reject_unknown_fields() {
    let mut carry_input = valid_input();
    carry_input["sessions"][0]["accountId"] = json!("account-1");
    rejects(&carry_input, "unknown field `accountId`");
    let reduced = reduce(finished("timer-b", "Café", 0, 25).to_vec());
    let mut page = json!({"commands": [], "sessions": reduced["sessions"],
        "currentTimerId": null, "after": null, "now": at(30)});
    assert!(call("timer.replay.page.v1", &page).is_ok());
    page["sessions"][0]["accountId"] = json!("account-1");
    let error = call("timer.replay.page.v1", &page).unwrap_err();
    assert!(error.contains("unknown field `accountId`"), "{error}");
}

#[test]
fn carry_history_rejects_malformed_operations_and_sessions() {
    let mut bad_rename = valid_input();
    bad_rename["taskOperations"][0]["title"] = json!("Café");
    rejects(&bad_rename, "invalid task rename");
    let mut duplicate_op = valid_input();
    let op = duplicate_op["taskOperations"][0].clone();
    duplicate_op["taskOperations"] = json!([op.clone(), op]);
    rejects(&duplicate_op, "invalid local taskOperations identities");
    let mut bad_time = valid_input();
    bad_time["taskOperations"][0]["occurredAt"] = json!("yesterday");
    rejects(&bad_time, "invalid operation clock");
    let mut duplicate_session = valid_input();
    let session = duplicate_session["sessions"][0].clone();
    duplicate_session["sessions"] = json!([session.clone(), session]);
    rejects(&duplicate_session, "duplicate session timerId");
    let mut bad_start = valid_input();
    bad_start["sessions"][0]["startedAt"] = json!("soon");
    rejects(&bad_start, "soon");
    let mut partial_session = valid_input();
    partial_session["sessions"][0]
        .as_object_mut()
        .unwrap()
        .remove("startedAt");
    rejects(&partial_session, "startedAt");
}

#[test]
fn carry_history_rejects_a_terminal_timer_that_disagrees_with_its_row() {
    let mut conflict = valid_input();
    conflict["canonicalTimer"]["taskId"] = json!(id("Other"));
    rejects(&conflict, "conflicting workspace terminal timer/history");
}

/// Carrying is not idempotent. With `B -> C` at 10 and `A -> B` at 20, an A
/// row that started at 0 belongs on B. A second pass would move it on to C.
fn reverse_chain() -> (Value, Vec<Value>) {
    let reduced = reduce(finished("old-a", "A", 0, 5).to_vec());
    let log = vec![
        rename("r1", "device-a", "B", "C", 10),
        rename("r2", "device-a", "A", "B", 20),
    ];
    (reduced, log)
}

#[test]
fn carry_history_rejects_already_carried_rows_and_timer() {
    let (reduced, log) = reverse_chain();
    let carried = carry(&reduced, log.clone()).unwrap();
    assert_eq!(tasks_by_timer(&carried["history"])["old-a"], id("B"));
    let mut again = reduced.clone();
    again["history"] = carried["history"].clone();
    again["canonicalTimer"] = carried["canonicalTimer"].clone();
    let error = carry(&again, log.clone()).unwrap_err();
    assert!(
        error.contains("history for timer old-a disagrees with its session taskId"),
        "{error}"
    );
    // A carried terminal timer alone is caught too.
    let mut timer_only = reduced.clone();
    timer_only["history"] = json!([]);
    timer_only["canonicalTimer"] = carried["canonicalTimer"].clone();
    let error = carry(&timer_only, log).unwrap_err();
    assert!(
        error.contains("disagrees with its session taskId"),
        "{error}"
    );
}

#[test]
fn carry_history_cannot_check_rows_without_a_session() {
    // Without sessions Core cannot tell carried rows from raw ones. The end
    // time stands in for the start, so the second pass moves the row again.
    let (reduced, log) = reverse_chain();
    let mut stripped = reduced.clone();
    stripped["sessions"] = json!([]);
    let once = carry(&stripped, log.clone()).unwrap();
    assert_eq!(tasks_by_timer(&once["history"])["old-a"], id("B"));
    let mut again = stripped.clone();
    again["history"] = once["history"].clone();
    again["canonicalTimer"] = once["canonicalTimer"].clone();
    let twice = carry(&again, log).unwrap();
    assert_eq!(tasks_by_timer(&twice["history"])["old-a"], id("C"));
}

#[test]
fn single_operation_gate_rejects_what_the_full_log_rejects() {
    // The server checks one candidate with `task.reduce.v1` before it stores
    // it, so a stored operation can never block later full reductions.
    let valid = rename("r1", "device-a", "Café", "Espresso", 30);
    let mut cases = Vec::new();
    for title in [json!("Café"), json!("\u{0007}Espresso"), json!("")] {
        let mut op = valid.clone();
        op["title"] = title;
        cases.push((op, "invalid task rename"));
    }
    for (field, value, expected) in [
        ("taskId", json!(""), "invalid task identity"),
        ("occurredAt", json!("yesterday"), "yesterday"),
        ("type", json!("archive"), "invalid task operation type"),
        ("title", json!(null), "expected a string"),
    ] {
        let mut op = valid.clone();
        op[field] = value;
        cases.push((op, expected));
    }
    for (op, expected) in cases {
        let single = call("task.reduce.v1", &json!({"operations": [op.clone()]}));
        let mut input = valid_input();
        input["taskOperations"] = json!([op.clone()]);
        let full = call("task.carryHistory.v1", &input);
        let (single, full) = (single.unwrap_err(), full.unwrap_err());
        assert!(single.contains(expected), "{op}: {single}");
        // Parse errors may word the same fault differently; both reject.
        if !matches!(expected, "yesterday" | "expected a string") {
            assert_eq!(single, full, "{op}");
        }
    }
    assert!(call("task.reduce.v1", &json!({"operations": [valid]})).is_ok());
}
