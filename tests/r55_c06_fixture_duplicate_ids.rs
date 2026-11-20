use pomodorough_core::{CoreError, dispatch_envelope_json, reduce_timer_fixture_case_json};
use serde_json::{Value, json};

// R55-C06: the legacy fixture operation `timer.reduce` reduced two commands
// with the same id, while `timer.reduce.v1` and every other timer entry point
// reject them with "duplicate timer command". Each fixture row pairs one
// `timer.reduce` request with the `timer.reduce.v1` request that carries the
// same commands. `duplicate` rows must fail both operations with the same
// envelope, also when the input has a later time or command error; `control`
// rows carry the complete envelopes captured before the fix.

const DUPLICATE: &str =
    r#"{"error":"invalid shared-core input: duplicate timer command","ok":false}"#;

fn fixture_rows() -> Vec<Value> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../fixtures/r55-c06-fixture-duplicate-ids-v1.json"
    ))
    .unwrap();
    fixture["rows"].as_array().unwrap().clone()
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap()
}

#[test]
fn every_row_returns_its_exact_envelope_on_both_operations() {
    for row in fixture_rows() {
        let name = text(&row, "name");
        for (operation, side) in [("timer.reduce", "fixture"), ("timer.reduce.v1", "v1")] {
            let envelope = dispatch_envelope_json(operation, text(&row[side], "input"));
            assert_eq!(envelope, text(&row[side], "expected"), "{name} {operation}");
        }
    }
}

#[test]
fn duplicate_rows_fail_both_operations_with_the_v1_envelope() {
    for row in fixture_rows()
        .iter()
        .filter(|row| row["kind"] == "duplicate")
    {
        let name = text(row, "name");
        let legacy = dispatch_envelope_json("timer.reduce", text(&row["fixture"], "input"));
        let public = dispatch_envelope_json("timer.reduce.v1", text(&row["v1"], "input"));
        assert_eq!(legacy, public, "{name}");
        assert_eq!(legacy, DUPLICATE, "{name}");
        let error = reduce_timer_fixture_case_json(text(&row["fixture"], "input")).unwrap_err();
        let CoreError::InvalidInput(reason) = error else {
            panic!("{name}: expected InvalidInput, got {error:?}");
        };
        assert_eq!(reason, "duplicate timer command", "{name}");
    }
}

#[test]
fn command_count_limit_wins_over_duplicate_ids_on_both_operations() {
    let command = json!({"id": "c1", "sequence": 1, "deviceId": "device-a",
        "timerId": "timer-a", "type": "start", "phase": "focus", "durationMs": 60000,
        "atMs": 0, "wallMs": 101, "counter": 0, "elapsedMs": 0});
    let wire = json!({"id": "c1", "deviceId": "device-a", "deviceSequence": 1,
        "timerId": "timer-a", "taskId": null, "type": "start", "phase": "focus",
        "plannedDurationMs": 60000, "occurredAt": "2026-07-20T12:00:00Z",
        "hlcWallMs": 101, "hlcCounter": 0, "observedElapsedMs": 0});
    let expected = r#"{"error":"invalid shared-core input: commands exceed 10000","ok":false}"#;
    for (operation, input) in [
        (
            "timer.reduce",
            json!({"epoch": "2026-07-20T12:00:00.000Z", "nowMs": 1000,
                "commands": vec![command; 10_001]}),
        ),
        (
            "timer.reduce.v1",
            json!({"now": "2026-07-20T12:00:01Z", "commands": vec![wire; 10_001]}),
        ),
    ] {
        let envelope = dispatch_envelope_json(operation, &input.to_string());
        assert_eq!(envelope, expected, "{operation}");
    }
}

#[test]
fn case_matrix_covers_duplicates_precedence_and_lookalike_ids() {
    let rows = fixture_rows();
    let count = |kind: &str| rows.iter().filter(|row| row["kind"] == kind).count();
    assert_eq!((count("duplicate"), count("control")), (13, 10));
    let names: Vec<&str> = rows.iter().map(|row| text(row, "name")).collect();
    for name in [
        "dup-adjacent",
        "dup-across-devices",
        "dup-third-repeats-first",
        "dup-beats-bad-epoch",
        "dup-beats-now-overflow",
        "dup-beats-at-overflow",
        "dup-beats-unknown-kind",
        "empty-commands",
        "case-distinct-ids",
        "missing-time-field-beats-dup",
    ] {
        assert!(names.contains(&name), "missing row {name}");
    }
}
