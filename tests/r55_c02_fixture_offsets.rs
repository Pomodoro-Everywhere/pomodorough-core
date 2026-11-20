use pomodorough_core::{CoreError, dispatch_envelope_json, reduce_timer_fixture_case_json};
use serde_json::Value;
use std::panic::catch_unwind;

// R55-C02: the legacy fixture operation `timer.reduce` added caller-supplied
// millisecond offsets (`nowMs`, each command's `atMs`) to its epoch with
// unchecked Chrono arithmetic and unwound (a WASM trap) when the instant did
// not fit `DateTime<Utc>`. Each fixture row is one complete public request.
// `overflow` rows unwound before the fix; `control` rows carry the complete
// envelope captured before the fix, so accepted inputs and earlier rejections
// (four-digit-year timestamps, non-i64 JSON numbers, command validation order)
// stay byte-identical.

/// Field, overflow rows, control rows.
const FIELDS: [(&str, usize, usize); 3] = [("nowMs", 6, 6), ("commands.atMs", 7, 11), ("", 0, 1)];

fn fixture_rows() -> Vec<Value> {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/r55-c02-fixture-offsets-v1.json")).unwrap();
    fixture["rows"].as_array().unwrap().clone()
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key].as_str().unwrap()
}

fn rows(kind: &str) -> Vec<Value> {
    fixture_rows()
        .into_iter()
        .filter(|row| row["kind"] == kind)
        .collect()
}

#[test]
fn every_row_returns_its_exact_envelope_without_unwinding() {
    for row in fixture_rows() {
        let (name, operation, input) = (
            text(&row, "name"),
            text(&row, "operation"),
            text(&row, "input"),
        );
        assert_eq!(operation, "timer.reduce", "{name}");
        let envelope = catch_unwind(|| dispatch_envelope_json(operation, input))
            .unwrap_or_else(|_| panic!("{name} unwound"));
        assert_eq!(envelope, text(&row, "expected"), "{name}");
    }
}

#[test]
fn overflow_rows_name_the_offset_field_as_invalid_input() {
    for row in rows("overflow") {
        let name = text(&row, "name");
        let field = text(&row, "field");
        let error = reduce_timer_fixture_case_json(text(&row, "input")).unwrap_err();
        let CoreError::InvalidInput(reason) = error else {
            panic!("{name}: expected InvalidInput, got {error:?}");
        };
        assert_eq!(reason, format!("{field} out of range"), "{name}");
    }
}

#[test]
fn last_representable_now_offsets_keep_reducing() {
    for name in ["now-last-upper", "now-last-lower"] {
        let row = fixture_rows()
            .into_iter()
            .find(|row| row["name"] == name)
            .unwrap();
        let output = reduce_timer_fixture_case_json(text(&row, "input")).unwrap();
        let envelope: Value = serde_json::from_str(text(&row, "expected")).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&output).unwrap(),
            envelope["value"],
            "{name}"
        );
    }
}

#[test]
fn case_matrix_covers_both_fields_and_both_bounds() {
    for (field, overflow, control) in FIELDS {
        let count = |kind: &str| {
            rows(kind)
                .iter()
                .filter(|row| row["field"] == field)
                .count()
        };
        assert_eq!(
            (count("overflow"), count("control")),
            (overflow, control),
            "{field}"
        );
    }
    let names: Vec<String> = fixture_rows()
        .iter()
        .map(|row| text(row, "name").to_owned())
        .collect();
    for field in ["now", "at"] {
        for bound in [
            "i64-max",
            "i64-min",
            "first-past-upper",
            "first-past-lower",
            "last-upper",
            "last-lower",
        ] {
            let name = format!("{field}-{bound}");
            assert!(names.contains(&name), "missing row {name}");
        }
    }
}
