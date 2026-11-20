use pomodorough_core::{CoreError, dispatch_envelope_json, dispatch_json};
use serde_json::Value;
use std::panic::catch_unwind;

// R55-C10: R55-C08 shaped nested objects across the public operations, but
// `bootstrap.workspacePlan.v1` still decoded both timers' `lastIntent` with
// plain Serde, so an array, a number or a string there answered "bootstrap ...
// requires recovery: invalid shared-core JSON: invalid type: ...". A null
// display context of `workspace.legacyDependencyPlan.v1` likewise reached
// Serde as "missing field `profile`". Every fixture row is one complete public
// request: error rows pin the C08 message "<path> must be a JSON object";
// `control` rows are ok:true bases, byte-identical before and after the fix;
// `nullable` rows set an optional `lastIntent` or a whole `canonicalTimer` to
// null, which still means "absent"; the `order` row breaks both intents and
// pins the local one first.

/// Every position this fix shapes, by operation, with one row per kind.
const POSITIONS: [(&str, &str, &str); 12] = [
    (
        "bootstrap.workspacePlan.v1",
        "local.workspace.base.canonicalTimer.lastIntent",
        "array",
    ),
    (
        "bootstrap.workspacePlan.v1",
        "local.workspace.base.canonicalTimer.lastIntent",
        "number",
    ),
    (
        "bootstrap.workspacePlan.v1",
        "local.workspace.base.canonicalTimer.lastIntent",
        "string",
    ),
    (
        "bootstrap.workspacePlan.v1",
        "remote.canonicalTimer.lastIntent",
        "array",
    ),
    (
        "bootstrap.workspacePlan.v1",
        "remote.canonicalTimer.lastIntent",
        "number",
    ),
    (
        "bootstrap.workspacePlan.v1",
        "remote.canonicalTimer.lastIntent",
        "string",
    ),
    (
        "bootstrap.workspacePlan.v1",
        "local.workspace.base.canonicalTimer.lastIntent",
        "order",
    ),
    (
        "workspace.legacyDependencyPlan.v1",
        "workspace.displayContext",
        "null",
    ),
    (
        "bootstrap.workspacePlan.v1",
        "local.workspace.base.canonicalTimer.lastIntent",
        "nullable",
    ),
    (
        "bootstrap.workspacePlan.v1",
        "remote.canonicalTimer.lastIntent",
        "nullable",
    ),
    (
        "bootstrap.workspacePlan.v1",
        "local.workspace.base.canonicalTimer",
        "nullable",
    ),
    (
        "bootstrap.workspacePlan.v1",
        "remote.canonicalTimer",
        "nullable",
    ),
];

fn fixture_rows() -> Vec<Value> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../fixtures/r55-c10-nested-last-intent-v1.json"
    ))
    .unwrap();
    fixture["rows"].as_array().unwrap().clone()
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key].as_str().unwrap()
}

fn is_error_row(row: &Value) -> bool {
    !matches!(row["kind"].as_str(), Some("control" | "nullable"))
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
fn every_non_object_names_its_nested_path_instead_of_serde_text() {
    for row in fixture_rows().iter().filter(|row| is_error_row(row)) {
        let name = text(row, "name");
        let error = dispatch_json(text(row, "operation"), text(row, "input")).unwrap_err();
        let CoreError::InvalidInput(reason) = error else {
            panic!("{name}: expected InvalidInput, got {error:?}");
        };
        assert_eq!(
            reason,
            format!("{} must be a JSON object", text(row, "path")),
            "{name}"
        );
        assert!(
            text(row, "before").contains("invalid shared-core JSON: "),
            "{name} did not leak raw Serde text before R55-C10"
        );
    }
}

#[test]
fn controls_and_null_intents_stay_accepted_and_unchanged() {
    for row in fixture_rows() {
        let name = text(&row, "name");
        match row["kind"].as_str() {
            Some("control") => {}
            Some("nullable") => {
                assert_eq!(text(&row, "expected"), text(&row, "before"), "{name}");
                let field = text(&row, "path").rsplit('.').next().unwrap();
                assert!(
                    text(&row, "input").contains(&format!("\"{field}\":null")),
                    "{name}"
                );
            }
            _ => continue,
        }
        assert!(
            text(&row, "expected").starts_with("{\"ok\":true,"),
            "{name}"
        );
    }
}

#[test]
fn case_matrix_covers_every_position_and_operation() {
    let rows = fixture_rows();
    for (operation, path, kind) in POSITIONS {
        assert!(
            rows.iter().any(|row| text(row, "operation") == operation
                && row["kind"] == kind
                && row.get("path").and_then(Value::as_str) == Some(path)),
            "{operation} {path} has no {kind} row"
        );
    }
    for operation in [
        "bootstrap.workspacePlan.v1",
        "workspace.legacyDependencyPlan.v1",
    ] {
        assert!(
            rows.iter()
                .any(|row| row["kind"] == "control" && row["operation"] == operation),
            "{operation} has no valid control"
        );
    }
    let count = |kind: &str| rows.iter().filter(|row| row["kind"] == kind).count();
    let kinds = [
        "control", "array", "number", "string", "nullable", "order", "null",
    ];
    assert_eq!(kinds.map(count), [2, 2, 2, 2, 4, 1, 1]);
    assert_eq!(rows.len(), 14);
}
