use pomodorough_core::{CoreError, dispatch_envelope_json, dispatch_json};
use serde_json::Value;
use std::panic::catch_unwind;

// R55-C05: a request root that is not a JSON object reached the shared shape
// validator with an empty path, so three operations answered
// `invalid shared-core input:  must be a JSON object` (no name, two spaces).
// Each fixture row is one complete public request. `root` rows pin the named
// message; `control` rows carry the complete envelope captured before the fix:
// valid requests, object roots, nested field paths, and the root messages of
// other operations. R55-C07 re-pinned three of those control rows to its named
// root message: `other-workspace.intent.v1-root-array`,
// `other-timer.completionState.v1-root-array` and
// `other-sync.batchPlan.v1-root-array`.

/// Operation and the name its non-object root error uses.
const OPERATIONS: [(&str, &str); 3] = [
    ("workspace.completionMutation.v1", "completion mutation"),
    ("workspace.readModel.v1", "read model"),
    ("workspace.ownershipPlan.v1", "ownership plan"),
];
const ROOTS: [&str; 6] = ["[]", "[{}]", "\"x\"", "7", "true", "null"];

fn fixture_rows() -> Vec<Value> {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/r55-c05-non-object-roots-v1.json")).unwrap();
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
                panic!("{operation} {input}: expected InvalidInput, got {error:?}");
            };
            assert_eq!(
                reason,
                format!("{root} must be a JSON object"),
                "{operation} {input}"
            );
        }
    }
}

#[test]
fn case_matrix_covers_every_root_of_every_operation() {
    let rows = fixture_rows();
    for (operation, _) in OPERATIONS {
        let mut inputs: Vec<&str> = rows
            .iter()
            .filter(|row| row["kind"] == "root" && row["operation"] == operation)
            .map(|row| text(row, "input"))
            .collect();
        inputs.sort_unstable();
        let mut expected = ROOTS.to_vec();
        expected.sort_unstable();
        assert_eq!(inputs, expected, "{operation}");
    }
    let controls = rows.iter().filter(|row| row["kind"] == "control").count();
    assert_eq!((rows.len(), controls), (37, 19));
}

#[test]
fn recorded_shape_error_corrections_match_the_dispatcher() {
    let changes: Value = serde_json::from_str(include_str!(
        "../fixtures/pwa-completion-shape-errors-v1.json"
    ))
    .unwrap();
    for change in changes.as_array().unwrap() {
        let name = format!("{}/{}", text(change, "operation"), text(change, "case"));
        let envelope = dispatch_envelope_json(text(change, "operation"), text(change, "inputRaw"));
        assert_eq!(envelope, text(change, "correctedEnvelope"), "{name}");
    }
}
