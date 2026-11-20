use pomodorough_core::{CoreError, dispatch_envelope_json, dispatch_json};
use serde_json::Value;
use std::panic::catch_unwind;

// R55-C08: R55-C07 refused a non-object request root, but Serde still read a
// nested struct from a positional JSON array (`"local":[1700000000000,0]`
// returned `ok:true`), and a null or a number there answered with raw Serde
// text. Every fixture row is one complete public request built from an ok:true
// base: `array`, `null` and `number` rows replace one nested object and pin
// "<path> must be a JSON object"; `control` rows are the bases themselves,
// byte-identical before and after the fix; `tolerant` rows put a positional
// array where the operation deliberately skips a malformed persisted value.
// Where no ok:true base holds an object at a position, the row first puts a
// plausible one there; the shape check runs before the typed decode, so the
// row still isolates that position.
// `null` rows cover every required position of the array matrix. `nullable`
// rows set an `Option` position to null: null must keep meaning "absent", so
// those rows keep their c07 envelope and never get the C08 message.

/// Every nested object position, by operation, that must refuse a non-object.
const PATHS: [(&str, &str); 75] = [
    ("hlc.head.v1", "observed[]"),
    ("hlc.tick.v1", "local"),
    ("hlc.tick.v1", "remote"),
    ("timer.reduce", "commands[]"),
    ("timer.reduce.v1", "commands[]"),
    ("timer.reduce.v1", "canonicalTimer"),
    ("timer.reduce.v1", "canonicalTimer.lastIntent"),
    ("timer.reduce.v1", "history[]"),
    ("timer.replay.page.v1", "commands[]"),
    ("timer.replay.page.v1", "sessions[]"),
    ("timer.replay.page.v1", "sessions[].lastIntent"),
    ("timer.replay.page.v1", "after"),
    ("workspace.intent.v1", "intent"),
    ("workspace.intent.v1", "selection"),
    ("workspace.intent.v1", "allocation"),
    ("workspace.intent.v1", "observation"),
    ("workspace.intent.v1", "clock"),
    ("workspace.intent.v1", "identities"),
    ("workspace.intent.v1", "calendarIntervals[]"),
    ("workspace.intent.v1", "allocation.hlc"),
    ("workspace.intent.v1", "observation.commandTimes"),
    ("workspace.intent.v1", "observation.monotonicAnchor"),
    ("workspace.intent.v1", "requestedTimer"),
    ("workspace.intent.v1", "requestedTimer.lastIntent"),
    ("workspace.intent.v1", "ownership"),
    ("workspace.intent.v1", "durability"),
    ("workspace.intent.v1", "localDurationsMs"),
    ("workspace.intent.v1", "knownTasks[]"),
    ("timer.completionPlan.v1", "beforeTimer"),
    ("timer.completionPlan.v1", "projectedTimer"),
    ("timer.completionPlan.v1", "requestedTimer"),
    ("timer.completionPlan.v1", "ownership"),
    ("timer.completionPlan.v1", "beforeTimer.lastIntent"),
    ("timer.completionPlan.v1", "history[]"),
    ("timer.completionPlan.v1", "source"),
    ("timer.completionPlan.v1", "canonical"),
    ("timer.completionPlan.v1", "canonical.canonicalTimer"),
    ("timer.completionPlan.v1", "canonical.history[]"),
    ("timer.completionPlan.v1", "optimistic"),
    ("timer.completionPlan.v1", "optimistic.canonicalTimer"),
    ("clock.observe.v1", "state"),
    ("clock.observe.v1", "state.anchor"),
    ("clock.observe.v1", "state.requestSample"),
    ("clock.observe.v1", "state.runtime"),
    ("clock.observe.v1", "reading"),
    ("clock.observe.v1", "server"),
    ("clock.observe.v1", "state.sample"),
    ("task.carryHistory.v1", "canonicalTimer.lastIntent"),
    ("task.carryHistory.v1", "sessions[].lastIntent"),
    ("workspace.completionMutation.v1", "previousObservation"),
    (
        "workspace.completionMutation.v1",
        "previousObservation.commandTimes",
    ),
    (
        "workspace.completionMutation.v1",
        "previousObservation.monotonicAnchor",
    ),
    ("workspace.completionMutation.v1", "requestedTimer"),
    (
        "workspace.completionMutation.v1",
        "requestedTimer.lastIntent",
    ),
    ("workspace.completionMutation.v1", "selection"),
    ("workspace.completionMutation.v1", "allocation"),
    ("workspace.completionMutation.v1", "allocation.hlc"),
    ("workspace.completionMutation.v1", "observation"),
    (
        "workspace.completionMutation.v1",
        "observation.commandTimes",
    ),
    (
        "workspace.completionMutation.v1",
        "observation.monotonicAnchor",
    ),
    ("workspace.completionMutation.v1", "clock"),
    ("workspace.completionMutation.v1", "identities"),
    ("workspace.completionMutation.v1", "calendarIntervals[]"),
    ("workspace.completionMutation.v1", "ownership"),
    ("workspace.completionMutation.v1", "lifecycle"),
    (
        "workspace.completionMutation.v1",
        "lifecycle.consumedCompletions[]",
    ),
    (
        "workspace.completionMutation.v1",
        "lifecycle.pendingBreaks[]",
    ),
    (
        "workspace.completionMutation.v1",
        "lifecycle.finishEvidence[]",
    ),
    (
        "workspace.completionMutation.v1",
        "lifecycle.finishEvidence[].command",
    ),
    (
        "workspace.completionMutation.v1",
        "lifecycle.finishEvidence[].sourceTimer",
    ),
    (
        "workspace.completionMutation.v1",
        "lifecycle.finishEvidence[].sourceHistory",
    ),
    (
        "workspace.completionMutation.v1",
        "lifecycle.finishEvidence[].sourceTimer.lastIntent",
    ),
    ("workspace.completionMutation.v1", "event"),
    (
        "workspace.completionMutation.v1",
        "event.acknowledgements[]",
    ),
    ("workspace.completionMutation.v1", "centralizedSession"),
];
/// Positions that skip a malformed persisted value instead of refusing it.
/// Bootstrap counts only well-formed completed rows, so one bad persisted row
/// never blocks a valid account. A Desktop local-only read and a Desktop
/// restore discard a malformed trusted sample; the PWA ignores a malformed
/// clock offset; legacy preferences read a non-object duration map, which is
/// untyped JSON and never reached Serde, as no saved durations.
const TOLERANT: [(&str, &str); 6] = [
    ("bootstrap.plan.v1", "localHistory[]"),
    ("bootstrap.plan.v1", "remoteHistory[]"),
    ("clock.observe.v1", "state.sample"),
    ("clock.observe.v1", "state.sample"),
    ("clock.observe.v1", "state.clockOffset"),
    ("workspace.legacyPreferences.v1", "settings.durations"),
];

/// Nullable positions where the base request stays `ok:true` with null.
/// `workspace.intent.v1` `durability` is nullable too, but no request accepts
/// a null there: a timer intent must omit the field and a task mutation must
/// carry one, so its row pins the "missing outgoing durability" error instead.
const NULLABLE_OK: [(&str, &str); 7] = [
    ("hlc.tick.v1", "remote"),
    ("timer.replay.page.v1", "after"),
    ("workspace.intent.v1", "requestedTimer"),
    ("timer.completionPlan.v1", "ownership"),
    ("workspace.completionMutation.v1", "requestedTimer"),
    ("workspace.completionMutation.v1", "ownership"),
    (
        "workspace.completionMutation.v1",
        "lifecycle.finishEvidence",
    ),
];
/// Array-matrix positions with no null row. Their array rows put an object into
/// a base whose request variant does not decode that field, so a null there
/// reaches only Serde's unknown-field error.
const NO_NULL_ROW: [(&str, &str); 3] = [
    ("timer.completionPlan.v1", "requestedTimer"),
    ("timer.completionPlan.v1", "canonical.canonicalTimer"),
    ("timer.completionPlan.v1", "optimistic.canonicalTimer"),
];

fn fixture_rows() -> Vec<Value> {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/r55-c08-nested-objects-v1.json")).unwrap();
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
fn every_non_object_names_its_nested_path() {
    for row in fixture_rows() {
        if row["kind"] == "control" || row["kind"] == "tolerant" || row["kind"] == "nullable" {
            continue;
        }
        let name = text(&row, "name");
        let error = dispatch_json(text(&row, "operation"), text(&row, "input")).unwrap_err();
        let CoreError::InvalidInput(reason) = error else {
            panic!("{name}: expected InvalidInput, got {error:?}");
        };
        assert_eq!(
            reason,
            format!("{} must be a JSON object", text(&row, "path")),
            "{name}"
        );
    }
}

#[test]
fn every_control_and_tolerant_row_is_accepted() {
    for row in fixture_rows() {
        if row["kind"] == "control" || row["kind"] == "tolerant" {
            assert!(
                text(&row, "expected").starts_with("{\"ok\":true,"),
                "{}",
                text(&row, "name")
            );
        }
    }
}

#[test]
fn case_matrix_covers_every_nested_path() {
    let rows = fixture_rows();
    let arrays: Vec<(&str, &str)> = rows
        .iter()
        .filter(|row| row["kind"] == "array")
        .map(|row| (text(row, "operation"), text(row, "path")))
        .collect();
    assert_eq!(arrays, PATHS.to_vec());
    let tolerant: Vec<(&str, &str)> = rows
        .iter()
        .filter(|row| row["kind"] == "tolerant")
        .map(|row| (text(row, "operation"), text(row, "path")))
        .collect();
    assert_eq!(tolerant, TOLERANT.to_vec());
    for (operation, _) in PATHS {
        assert!(
            rows.iter()
                .any(|row| row["kind"] == "control" && row["operation"] == operation),
            "{operation} has no valid control"
        );
    }
    let count = |kind: &str| rows.iter().filter(|row| row["kind"] == kind).count();
    assert_eq!(
        (
            count("array"),
            count("null"),
            count("number"),
            count("nullable")
        ),
        (PATHS.len(), 43, 15, 31)
    );
}

fn paths_of<'a>(rows: &'a [Value], kind: &str) -> Vec<(&'a str, &'a str)> {
    rows.iter()
        .filter(|row| row["kind"] == kind)
        .map(|row| (text(row, "operation"), text(row, "path")))
        .collect()
}

#[test]
fn every_array_matrix_position_has_one_null_row() {
    let rows = fixture_rows();
    let (nulls, nullables) = (paths_of(&rows, "null"), paths_of(&rows, "nullable"));
    for position in PATHS {
        let rows = nulls
            .iter()
            .chain(&nullables)
            .filter(|row| **row == position);
        let expected = usize::from(!NO_NULL_ROW.contains(&position));
        assert_eq!(rows.count(), expected, "{position:?}");
    }
}

#[test]
fn nullable_positions_keep_null_as_absent() {
    let rows = fixture_rows();
    for row in rows.iter().filter(|row| row["kind"] == "nullable") {
        let (name, path) = (text(row, "name"), text(row, "path"));
        let input: Value = serde_json::from_str(text(row, "input")).unwrap();
        assert!(text(row, "input").contains(":null"), "{name}");
        assert_eq!(text(row, "expected"), text(row, "before"), "{name}");
        assert!(
            !text(row, "expected").contains(&format!("{path} must be a JSON object")),
            "{name}: {input}"
        );
    }
    let nullables = paths_of(&rows, "nullable");
    for position in NULLABLE_OK {
        let row = rows
            .iter()
            .find(|row| {
                row["kind"] == "nullable" && (text(row, "operation"), text(row, "path")) == position
            })
            .unwrap_or_else(|| panic!("no nullable row for {position:?}"));
        assert!(nullables.contains(&position));
        assert!(
            text(row, "expected").starts_with("{\"ok\":true,"),
            "{position:?}"
        );
    }
}

#[test]
fn pwa_control_uses_its_valid_clock_offset_sample() {
    let rows = fixture_rows();
    let output = |name: &str| -> Value {
        let row = rows.iter().find(|row| row["name"] == name).unwrap();
        serde_json::from_str(text(row, "expected")).unwrap()
    };
    // Same reading (wall 1 000 000 ms); only the stored clockOffset differs.
    let sampled = output("clock.observe.v1-valid-26");
    let unsampled = output("clock.observe.v1-valid-3");
    assert_eq!(sampled["value"]["trustedNowMs"], 1_000_100);
    assert_eq!(
        sampled["value"]["state"]["runtime"]["identity"],
        "100:1:999000"
    );
    assert_eq!(unsampled["value"]["trustedNowMs"], 1_000_000);
    assert_eq!(unsampled["value"]["state"]["runtime"]["identity"], "local");
}
