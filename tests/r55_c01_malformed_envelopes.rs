use pomodorough_core::{CoreError, dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};
use std::panic::catch_unwind;

// R55-C01: three workspace operations indexed caller JSON before checking its
// shape and unwound (a WASM trap) instead of returning an error. Each row of
// the fixture is one complete public request. `malformed` rows unwound before
// the fix; `control` rows carry the complete envelope captured before the fix,
// so accepted inputs and earlier rejections must stay byte-identical. The
// completion lifecycle has two sites because each workspace field is rejected
// under its own name. `scripts/r55_c01_malformed_envelopes_fixture.py`
// regenerates the fixture byte-identical from its six verbatim seed rows; run it
// with `--check` after `cargo build --example artifact_parity_oracle`.

/// Site, the one rejection its malformed rows share, malformed rows, controls.
const SITES: [(&str, &str, usize, usize); 4] = [
    (
        "completionWorkspace",
        "workspace must be a JSON object",
        12,
        10,
    ),
    (
        "completionPreviousWorkspace",
        "previousWorkspace must be a JSON object",
        8,
        5,
    ),
    ("dependencyQueues", "missing raw legacy queue", 8, 7),
    (
        "preferenceIdentity",
        "invalid retained legacy identity",
        12,
        7,
    ),
];

/// Operation and lifecycle stage of every accepted control, site by site.
const ACCEPTED_CONTROLS: [(&str, &str); 8] = [
    ("workspace.completionMutation.v1", "expiryObservation"),
    (
        "workspace.completionMutation.v1",
        "deferredBreakOpportunity",
    ),
    (
        "workspace.completionMutation.v1",
        "deferredBreakOpportunity",
    ),
    ("workspace.legacyDependencyPlan.v1", ""),
    ("workspace.legacyDependencyPlan.v1", ""),
    ("workspace.legacyDependencyPlan.v1", ""),
    ("workspace.legacyDependencyPlan.v1", ""),
    ("workspace.legacyPreferences.v1", ""),
];

/// Names of the four R55 Core audit probe rows, replayed verbatim.
const AUDIT_ROWS: [&str; 4] = [
    "completion-deferred-workspace-array",
    "completion-deferred-workspace-boolean",
    "dependencies-stored-queues-empty",
    "preferences-auto-start-id-missing",
];

fn fixture_rows() -> Vec<Value> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../fixtures/r55-c01-malformed-envelopes-v1.json"
    ))
    .unwrap();
    fixture["rows"].as_array().unwrap().clone()
}

fn rows(site: &str, kind: &str) -> Vec<Value> {
    fixture_rows()
        .into_iter()
        .filter(|row| row["site"] == site && row["kind"] == kind)
        .collect()
}

fn accepted_control(row: &Value) -> (String, String) {
    let input: Value = serde_json::from_str(text(row, "input")).unwrap();
    let stage = input["stage"].as_str().unwrap_or_default();
    (text(row, "operation").to_owned(), stage.to_owned())
}

fn text<'a>(row: &'a Value, field: &str) -> &'a str {
    row[field].as_str().unwrap()
}

fn envelope(row: &Value) -> String {
    catch_unwind(|| dispatch_envelope_json(text(row, "operation"), text(row, "input")))
        .unwrap_or_else(|_| panic!("{} unwound", row["name"]))
}

fn assert_malformed_rows_rejected(site: &str) {
    let (_, reason, malformed, _) = SITES.into_iter().find(|row| row.0 == site).unwrap();
    let rows = rows(site, "malformed");
    assert_eq!(rows.len(), malformed, "{site} malformed case matrix");
    let rejected = json!({"ok": false, "error": format!("invalid shared-core input: {reason}")});
    for row in rows {
        let actual = envelope(&row);
        let parsed: Value = serde_json::from_str(&actual).unwrap();
        assert_eq!(parsed, rejected, "{}", row["name"]);
        assert_eq!(actual, text(&row, "expected"), "{}", row["name"]);
        let error = dispatch_json(text(&row, "operation"), text(&row, "input")).unwrap_err();
        assert!(
            matches!(&error, CoreError::InvalidInput(text) if text == reason),
            "{}: {error:?}",
            row["name"]
        );
    }
}

#[test]
fn completion_lifecycle_rejects_non_object_workspace_without_unwinding() {
    assert_malformed_rows_rejected("completionWorkspace");
}

#[test]
fn completion_lifecycle_names_previous_workspace_in_its_own_rejection() {
    assert_malformed_rows_rejected("completionPreviousWorkspace");
}

#[test]
fn legacy_dependency_plan_rejects_incomplete_stored_queues_without_unwinding() {
    assert_malformed_rows_rejected("dependencyQueues");
}

#[test]
fn legacy_preferences_reject_retained_row_without_identity_without_unwinding() {
    assert_malformed_rows_rejected("preferenceIdentity");
}

#[test]
fn controls_keep_the_complete_envelopes_captured_before_the_fix() {
    let mut accepted = Vec::new();
    for (site, _, _, controls) in SITES {
        let rows = rows(site, "control");
        assert_eq!(rows.len(), controls, "{site} control case matrix");
        for row in rows {
            let actual = envelope(&row);
            assert_eq!(actual, text(&row, "expected"), "{}", row["name"]);
            if actual.starts_with(r#"{"ok":true,"#) {
                accepted.push(accepted_control(&row));
            }
        }
    }
    // Every operation and both lifecycle stages keep an accepted request, so
    // the tightening cannot pass by rejecting everything.
    let expected = ACCEPTED_CONTROLS.map(|(operation, stage)| (operation.into(), stage.into()));
    assert_eq!(accepted, expected, "accepted controls");
}

#[test]
fn case_matrix_covers_every_fixture_row() {
    let checked: usize = SITES
        .iter()
        .map(|(site, ..)| rows(site, "malformed").len() + rows(site, "control").len())
        .sum();
    let expected: usize = SITES.iter().map(|site| site.2 + site.3).sum();
    assert_eq!(checked, expected, "rows in checked sites");
    assert_eq!(fixture_rows().len(), expected, "rows in the fixture");
}

#[test]
fn audit_probe_rows_are_among_the_malformed_cases() {
    let names: Vec<_> = SITES
        .iter()
        .flat_map(|(site, ..)| rows(site, "malformed"))
        .map(|row| text(&row, "name").to_owned())
        .collect();
    for name in AUDIT_ROWS {
        assert!(names.iter().any(|known| known == name), "{name} missing");
    }
}
