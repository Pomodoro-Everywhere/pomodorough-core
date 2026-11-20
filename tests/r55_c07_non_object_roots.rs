use pomodorough_core::{CoreError, dispatch_envelope_json, dispatch_json};
use serde_json::Value;
use std::panic::catch_unwind;

// R55-C07: fourteen operations decoded their request straight into a Serde
// struct. Serde reads a JSON array into a struct field by field, so `[]` or a
// positional array returned `ok:true` for several of them, and `null`, numbers,
// strings and booleans returned raw Serde text instead of the named message.
// Each fixture row is one complete public request. `root` rows pin the named
// message; `control` rows carry the complete envelope, byte-identical before
// and after the fix; `unknown-field` rows pin the `deny_unknown_fields`
// rejections added where no client sends extra fields.

/// Operation and the name its non-object root error uses.
const OPERATIONS: [(&str, &str); 14] = [
    ("selectedTask.classify", "selected task envelope"),
    ("bootstrap.plan.v1", "bootstrap plan"),
    ("task.identity.v1", "task identity"),
    ("hlc.head.v1", "hlc head"),
    ("hlc.tick.v1", "hlc tick"),
    ("uuidv7.fromParts.v1", "UUIDv7 parts"),
    ("timer.reduce.v1", "timer reduction"),
    ("timer.reduce", "timer fixture"),
    ("timer.replay.page.v1", "timer replay page"),
    ("workspace.intent.v1", "workspace intent"),
    ("sync.batchPlan.v1", "batch plan"),
    ("timer.completionPlan.v1", "completion plan"),
    ("timer.completionState.v1", "completion state"),
    ("clock.observe.v1", "clock observation"),
];
const ROOTS: [&str; 9] = [
    "[]", "[{}]", "[[]]", "\"x\"", "7", "true", "null", " \n[]", "\t false",
];
const ROOT_LABELS: [&str; 6] = [
    "array-empty",
    "array-positional",
    "null",
    "number",
    "string",
    "boolean",
];
const DENIED: [&str; 6] = [
    "selectedTask.classify",
    "bootstrap.plan.v1",
    "task.identity.v1",
    "hlc.tick.v1",
    "uuidv7.fromParts.v1",
    "timer.reduce",
];

fn fixture_rows() -> Vec<Value> {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/r55-c07-non-object-roots-v1.json")).unwrap();
    fixture["rows"].as_array().unwrap().clone()
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key].as_str().unwrap()
}

#[test]
fn every_row_returns_its_exact_envelope_without_unwinding() {
    for row in fixture_rows() {
        let (name, operation, input) = (
            text(&row, "name"),
            text(&row, "operation"),
            text(&row, "input"),
        );
        let envelope = catch_unwind(|| dispatch_envelope_json(operation, input))
            .unwrap_or_else(|_| panic!("{name} unwound"));
        assert_eq!(envelope, text(&row, "expected"), "{name}");
    }
}

#[test]
fn every_non_object_root_names_the_request() {
    for (operation, root) in OPERATIONS {
        for input in ROOTS {
            let error = dispatch_json(operation, input).unwrap_err();
            let CoreError::InvalidInput(reason) = error else {
                panic!("{operation} {input:?}: expected InvalidInput, got {error:?}");
            };
            assert_eq!(
                reason,
                format!("{root} must be a JSON object"),
                "{operation} {input:?}"
            );
        }
    }
}

#[test]
fn malformed_non_object_roots_keep_the_syntax_error() {
    for (operation, _) in OPERATIONS {
        for input in ["[1,", "nul", "[]x"] {
            let error = dispatch_json(operation, input).unwrap_err();
            assert!(
                matches!(error, CoreError::Json(_)),
                "{operation} {input:?}: {error:?}"
            );
        }
    }
}

#[test]
fn unknown_fields_are_rejected_only_where_denied() {
    for row in fixture_rows() {
        if row["kind"] != "unknown-field" {
            continue;
        }
        let operation = text(&row, "operation");
        assert!(DENIED.contains(&operation), "{operation}");
        let error = dispatch_json(operation, text(&row, "input")).unwrap_err();
        let CoreError::Json(reason) = error else {
            panic!("{operation}: expected a JSON error, got {error:?}");
        };
        assert!(
            reason
                .to_string()
                .starts_with("unknown field `futureField`"),
            "{operation}: {reason}"
        );
    }
}

#[test]
fn case_matrix_covers_every_root_of_every_operation() {
    let rows = fixture_rows();
    for (operation, _) in OPERATIONS {
        let names: Vec<&str> = rows
            .iter()
            .filter(|row| row["operation"] == operation)
            .map(|row| text(row, "name"))
            .collect();
        let mut expected: Vec<String> = ROOT_LABELS
            .iter()
            .map(|label| format!("{operation}-root-{label}"))
            .collect();
        expected.push(format!("{operation}-valid"));
        expected.push(format!("{operation}-empty-object"));
        if DENIED.contains(&operation) {
            expected.push(format!("{operation}-unknown-field"));
        }
        assert_eq!(names, expected, "{operation}");
        let valid = rows
            .iter()
            .find(|row| row["name"] == format!("{operation}-valid"))
            .unwrap();
        assert!(
            text(valid, "expected").starts_with("{\"ok\":true,"),
            "{operation}"
        );
    }
    let count = |kind: &str| rows.iter().filter(|row| row["kind"] == kind).count();
    assert_eq!(
        (
            rows.len(),
            count("root"),
            count("control"),
            count("unknown-field")
        ),
        (118, 84, 28, 6)
    );
}

#[test]
fn large_and_deep_array_roots_name_the_request() {
    let large = format!("[{}0]", "0,".repeat(99_999));
    let deep = format!("{}{}", "[".repeat(100), "]".repeat(100));
    let too_deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    for (operation, root) in OPERATIONS {
        for input in [&large, &deep] {
            let error = dispatch_json(operation, input).unwrap_err();
            let CoreError::InvalidInput(reason) = error else {
                panic!("{operation}: expected InvalidInput, got {error:?}");
            };
            assert_eq!(
                reason,
                format!("{root} must be a JSON object"),
                "{operation}"
            );
        }
        // Serde's recursion limit still stops the parse before any root check.
        let error = dispatch_json(operation, &too_deep).unwrap_err();
        assert!(
            error.to_string().contains("recursion limit exceeded"),
            "{operation}: {error:?}"
        );
    }
}

fn valid_row(operation: &str) -> Value {
    let name = format!("{operation}-valid");
    fixture_rows()
        .into_iter()
        .find(|row| row["name"] == name.as_str())
        .unwrap()
}

/// An object root skips the strict parse, so the decoder each operation had
/// before R55-C07 still decides duplicate keys: Serde's value decode keeps the
/// last key, and a typed decode reports the duplicate field.
#[test]
fn object_roots_with_duplicate_keys_keep_the_legacy_decoder() {
    let reduce = valid_row("timer.reduce.v1");
    let duplicated =
        text(&reduce, "input").replacen('{', r#"{"futureField":1,"futureField":2,"#, 1);
    assert_eq!(
        dispatch_envelope_json("timer.reduce.v1", &duplicated),
        text(&reduce, "expected")
    );
    let parts = valid_row("uuidv7.fromParts.v1");
    let duplicated = text(&parts, "input").replacen('{', r#"{"timestampMs":0,"#, 1);
    assert_eq!(
        dispatch_envelope_json("uuidv7.fromParts.v1", &duplicated),
        r#"{"error":"invalid shared-core JSON: duplicate field `timestampMs` at line 1 column 30","ok":false}"#
    );
}
