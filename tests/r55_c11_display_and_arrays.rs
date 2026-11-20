use pomodorough_core::{CoreError, dispatch_envelope_json, dispatch_json};
use serde_json::Value;
use std::panic::catch_unwind;

// R55-C11: a stored display context decoded its contents with plain Serde, so
// a missing `profile`, another profile or an extra field answered "invalid
// shared-core JSON: missing field `profile`" and similar text in seven
// operations. Six top-level arrays likewise answered "invalid type: ...,
// expected a sequence". Every fixture row is one complete public request:
// `error` and `order` rows pin the Core message that replaced raw Serde text;
// `map` rows pin the refusal of the alternate enum map form
// {"pwaStorage": null}, which Serde accepted; `kept`, `pin` and `precedence`
// rows pin envelopes that must not move (including the R55-C10 single-fault
// envelopes and double-fault precedence); `element` and `control` rows stay
// byte-identical.

/// Operations whose stored display context this fix shapes, with its path.
const DISPLAY: [(&str, &str); 7] = [
    ("reconcile.rebase.v3", "displayContext"),
    ("workspace.project.v1", "displayContext"),
    ("workspace.readModel.v1", "source.value.displayContext"),
    (
        "workspace.completionMutation.v1",
        "workspace.displayContext",
    ),
    ("workspace.intent.v1", "workspace.displayContext"),
    (
        "workspace.legacyDependencyPlan.v1",
        "workspace.displayContext",
    ),
    ("workspace.legacyPreferences.v1", "workspace.displayContext"),
];

/// Array positions this fix shapes, by operation.
const ARRAYS: [(&str, &str); 6] = [
    ("bootstrap.plan.v1", "localHistory"),
    ("bootstrap.plan.v1", "remoteHistory"),
    ("workspace.completionMutation.v1", "identities.commandUuids"),
    (
        "workspace.intent.v1",
        "durability.outgoingDurationOperationIds",
    ),
    ("workspace.intent.v1", "identities.commandUuids"),
    (
        "workspace.legacyPreferences.v1",
        "identities.operationUuids",
    ),
];

/// R55-C10 envelopes that a moved check would change, by path.
const C10_PINS: [(&str, &str); 10] = [
    ("pin", "local.workspace"),
    ("pin", "local.workspace.base"),
    ("pin", "local.workspace.base.canonicalTimer"),
    ("pin", "remote.canonicalTimer"),
    ("precedence", "remote.history"),
    ("precedence", "local.workspace.base.history"),
    ("precedence", "local.ownerId"),
    ("precedence", "remote.hasState"),
    ("precedence", "workspace.canonicalHead"),
    ("precedence", "workspace.local"),
];

fn fixture_rows() -> Vec<Value> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../fixtures/r55-c11-display-and-arrays-v1.json"
    ))
    .unwrap();
    fixture["rows"].as_array().unwrap().clone()
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key].as_str().unwrap()
}

fn kind(row: &Value) -> &str {
    text(row, "kind")
}

fn has_row(rows: &[Value], operation: &str, path: &str, kind: &str) -> bool {
    rows.iter()
        .any(|row| row["operation"] == operation && row["kind"] == kind && row["path"] == path)
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
fn shaped_rows_name_their_path_instead_of_serde_text() {
    for row in fixture_rows() {
        let name = text(&row, "name");
        match kind(&row) {
            "error" | "order" => assert!(
                text(&row, "before").contains("invalid shared-core JSON: "),
                "{name} did not leak raw Serde text before R55-C11"
            ),
            "map" => assert!(
                text(&row, "before").starts_with("{\"ok\":true,"),
                "{name} was not accepted before R55-C11"
            ),
            _ => continue,
        }
        let error = dispatch_json(text(&row, "operation"), text(&row, "input")).unwrap_err();
        let CoreError::InvalidInput(reason) = error else {
            panic!("{name}: expected InvalidInput, got {error:?}");
        };
        assert_eq!(reason, text(&row, "message"), "{name}");
    }
}

#[test]
fn pinned_and_unchanged_rows_keep_their_stack_envelope() {
    for row in fixture_rows() {
        let name = text(&row, "name");
        assert!(row.get("before").is_some(), "{name}");
        match kind(&row) {
            "kept" | "pin" | "precedence" => assert_eq!(
                text(&row, "expected"),
                format!(
                    "{{\"error\":\"invalid shared-core input: {}\",\"ok\":false}}",
                    text(&row, "message")
                ),
                "{name}"
            ),
            "control" => assert!(text(&row, "expected").starts_with("{\"ok\":true,")),
            "element" => assert!(text(&row, "expected").starts_with("{\"error\":")),
            _ => continue,
        }
        assert_eq!(text(&row, "expected"), text(&row, "before"), "{name}");
    }
}

#[test]
fn case_matrix_covers_every_position_and_operation() {
    let rows = fixture_rows();
    for (operation, display) in DISPLAY {
        let extra = format!("{display}.x");
        assert!(has_row(&rows, operation, &extra, "error"), "{operation}");
        if operation != "workspace.legacyPreferences.v1" {
            let profile = format!("{display}.profile");
            let count = rows
                .iter()
                .filter(|row| row["operation"] == operation && row["path"] == profile.as_str())
                .count();
            assert!(count >= 2, "{operation} profile rows");
        }
    }
    for (operation, path) in ARRAYS {
        assert!(
            has_row(&rows, operation, path, "error"),
            "{operation} {path}"
        );
    }
    for operation in ["workspace.completionMutation.v1", "workspace.intent.v1"] {
        let null = format!("{operation}-identities.commandUuids-null");
        assert!(
            rows.iter().any(|row| row["name"] == null.as_str()),
            "{null}"
        );
    }
    for (kind, path) in C10_PINS {
        assert!(
            rows.iter()
                .any(|row| row["kind"] == kind && row["path"] == path),
            "{path}"
        );
    }
    let operations = DISPLAY.iter().chain(ARRAYS.iter()).map(|(op, _)| *op);
    for operation in operations.chain(["bootstrap.workspacePlan.v1"]) {
        assert!(
            has_row_kind(&rows, operation, "control"),
            "{operation} control"
        );
    }
}

#[test]
fn case_matrix_counts_every_kind() {
    let rows = fixture_rows();
    let count = |kind: &str| rows.iter().filter(|row| row["kind"] == kind).count();
    let kinds = [
        "control",
        "error",
        "map",
        "kept",
        "element",
        "order",
        "precedence",
        "pin",
    ];
    assert_eq!(kinds.map(count), [9, 30, 3, 2, 4, 1, 9, 4]);
    assert_eq!(rows.len(), 62);
}

fn has_row_kind(rows: &[Value], operation: &str, kind: &str) -> bool {
    rows.iter()
        .any(|row| row["operation"] == operation && row["kind"] == kind)
}
