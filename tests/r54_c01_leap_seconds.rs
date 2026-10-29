use chrono::DateTime;
use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const NOW: &str = "2017-01-01T00:01:00Z";
const REOPENS: usize = 4;
const LEAP_START: &str = "2016-12-31T23:59:60.25Z";
const LEAP_END: &str = "2016-12-31T23:59:60.75Z";
const FINE_LEAP: &str = "2016-12-31T23:59:60.123456789Z";

fn timestamps() -> Vec<[String; 2]> {
    serde_json::from_str(include_str!("../fixtures/r54-c01-timestamps.json")).unwrap()
}

fn is_leap(stamp: &str) -> bool {
    &stamp[17..19] == "60"
}

fn call(operation: &str, input: &Value) -> Result<Value, String> {
    dispatch_json(operation, &input.to_string())
        .map(|raw| serde_json::from_str(&raw).unwrap())
        .map_err(|error| error.to_string())
}

fn request(stamp: &str, status: &str) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-projection-v1.json")).unwrap();
    let mut input = fixture["request"].clone();
    for queue in input["local"].as_object_mut().unwrap().values_mut() {
        *queue = json!([]);
    }
    input["timerDependencies"] = json!([]);
    input["neverSent"] = json!({});
    input["now"] = json!(NOW);
    input["base"]["canonicalTimer"] = json!({"id": "leap", "phase": "focus",
        "status": status, "plannedDurationMs": 60000, "elapsedAtAnchorMs": 0,
        "anchorAt": stamp});
    input["base"]["history"] = json!([]);
    if status != "paused" {
        let kind = if status == "completed" {
            "finish"
        } else {
            "cancel"
        };
        input["base"]["canonicalTimer"]["elapsedAtAnchorMs"] =
            json!(if status == "completed" { 60000 } else { 1234 });
        input["base"]["canonicalTimer"]["lastIntent"] =
            json!({"type": kind, "commandId": kind, "occurredAt": stamp});
        input["base"]["history"] = json!([{"id": "leap", "timerId": "leap",
            "commandId": kind, "phase": "focus", "status": status,
            "plannedDurationMs": 60000}]);
        if status == "completed" {
            input["base"]["history"][0]["completedAt"] = json!(stamp);
        } else {
            input["base"]["history"][0]["endedAt"] = json!(stamp);
        }
    }
    input
}

fn project(input: &Value) -> Value {
    call("workspace.project.v1", input).unwrap()["workspace"].clone()
}

fn reopen(mut input: Value, first: &Value, label: &str) {
    for round in 0..REOPENS {
        for key in ["canonicalTimer", "history"] {
            input["base"][key] = first[key].clone();
        }
        assert_eq!(project(&input), *first, "{label} reopen {round}");
    }
}

fn assert_same_instant(stamp: &str, spelled: &Value) {
    let input = DateTime::parse_from_rfc3339(stamp).unwrap();
    let output = DateTime::parse_from_rfc3339(spelled.as_str().unwrap()).unwrap();
    assert_eq!(output, input, "{stamp} -> {spelled}");
}

fn assert_reopens(status: &str) {
    for [stamp, expected] in timestamps() {
        let label = format!("{status}/{stamp}");
        let input = request(&stamp, status);
        let first = project(&input);
        let timer = &first["canonicalTimer"];
        assert_eq!(timer["anchorAt"], expected, "{label}");
        assert_same_instant(&stamp, &timer["anchorAt"]);
        if status != "paused" {
            assert_eq!(timer["lastIntent"]["occurredAt"], stamp, "{label}");
        }
        if status == "completed" {
            assert_eq!(first["history"][0]["completedAt"], expected, "{label}");
        } else if status == "cancelled" {
            assert_eq!(first["history"][0]["endedAt"], expected, "{label}");
        }
        reopen(input, &first, &label);
    }
}

fn command(kind: &str, at: &str, sequence: i64, wall_ms: i64) -> Value {
    json!({"id": kind, "deviceId": "device-a", "deviceSequence": sequence,
        "timerId": "leap", "type": kind, "phase": "focus", "plannedDurationMs": 3_600_000,
        "occurredAt": at, "hlcWallMs": wall_ms, "hlcCounter": 0, "observedElapsedMs": 0})
}

/// Applies commands to an empty base, then reopens the applied timer and history.
fn applied_then_reopened(commands: Value, label: &str) -> Value {
    let mut input = request("2016-12-31T23:59:00Z", "paused");
    input["base"]["canonicalTimer"] = Value::Null;
    let mut pending = input["local"].clone();
    pending["commands"] = commands;
    let applied = call(
        "projection.apply.v2",
        &json!({"base": input["base"], "pending": pending, "now": NOW}),
    )
    .unwrap();
    for key in ["canonicalTimer", "history"] {
        input["base"][key] = applied[key].clone();
    }
    let first = project(&input);
    for key in ["canonicalTimer", "history"] {
        assert_eq!(first[key], applied[key], "{label} {key}");
    }
    reopen(input, &first, label);
    first
}

#[test]
fn timestamp_fixture_keeps_leap_offset_and_control_cases() {
    let stamps = timestamps();
    let count = |keep: fn(&[String; 2]) -> bool| stamps.iter().filter(|pair| keep(pair)).count();
    assert_eq!(stamps.len(), 24);
    assert_eq!(count(|[stamp, _]| is_leap(stamp)), 15);
    assert_eq!(count(|[stamp, _]| !stamp.ends_with('Z')), 8);
    assert_eq!(
        count(|[_, expected]| is_leap(expected) && !expected.contains("T23:59:60")),
        2
    );
    assert_eq!(count(|[_, expected]| expected.ends_with('Z')), 24);
}

#[test]
fn paused_leap_and_ordinary_timestamps_survive_repeated_reopen() {
    assert_reopens("paused");
}

#[test]
fn explicit_finish_and_cancel_pairs_preserve_fractional_precision() {
    assert_reopens("completed");
    assert_reopens("cancelled");
}

#[test]
fn command_created_terminal_records_keep_leap_spellings() {
    let start = command("start", "2016-12-31T23:59:00Z", 1, 1_483_228_740_000);
    for (kind, status, field) in [
        ("finish", "completed", "completedAt"),
        ("cancel", "cancelled", "endedAt"),
    ] {
        for (at, expected) in [
            ("2016-12-31T23:59:60Z", "2016-12-31T23:59:60Z"),
            ("2016-12-31T23:59:60.5Z", "2016-12-31T23:59:60.5Z"),
            ("2017-01-01T04:59:60.75+05:00", "2016-12-31T23:59:60.75Z"),
        ] {
            let label = format!("{kind}/{at}");
            let end = command(kind, at, 2, 1_483_228_800_500);
            let first = applied_then_reopened(json!([start, end]), &label);
            let timer = &first["canonicalTimer"];
            assert_eq!(timer["status"], status, "{label}");
            assert_eq!(timer["anchorAt"], expected, "{label}");
            assert_eq!(timer["lastIntent"]["occurredAt"], expected, "{label}");
            assert_eq!(first["history"][0][field], expected, "{label}");
        }
    }
}

#[test]
fn command_created_running_and_paused_timers_keep_leap_anchors() {
    let start = command("start", LEAP_START, 1, 1_483_228_800_250);
    let running = applied_then_reopened(json!([start]), "running")["canonicalTimer"].clone();
    assert_eq!(running["status"], "running");
    assert_eq!(running["anchorAt"], LEAP_START);
    assert_eq!(running["lastIntent"]["occurredAt"], LEAP_START);
    let offset_end = "2017-01-01T04:59:60.75+05:00";
    let pause = command("pause", offset_end, 2, 1_483_228_800_750);
    let paused = applied_then_reopened(json!([start, pause]), "paused")["canonicalTimer"].clone();
    assert_eq!(paused["status"], "paused");
    assert_eq!(paused["anchorAt"], LEAP_END);
    assert_eq!(paused["lastIntent"]["occurredAt"], LEAP_END);
}

#[test]
fn legacy_timer_reduce_sessions_keep_leap_fractions() {
    let start = command("start", LEAP_START, 1, 1_483_228_800_250);
    let finish = command("finish", LEAP_END, 2, 1_483_228_800_750);
    let reduce = |input: Value| call("timer.reduce.v1", &input).unwrap();
    let output = reduce(json!({"commands": [start, finish], "now": NOW}));
    assert_eq!(output["sessions"].as_array().unwrap().len(), 1);
    let session = &output["sessions"][0];
    assert_eq!(session["startedAt"], LEAP_START);
    assert_eq!(session["anchorAt"], LEAP_END);
    assert_eq!(session["endedAt"], LEAP_END);
    assert_eq!(output["canonicalTimer"]["anchorAt"], LEAP_END);
    assert_eq!(output["history"][0]["completedAt"], LEAP_END);
    // The legacy reducer rejects a timer that overlaps history, so reopen each alone.
    for key in ["canonicalTimer", "history"] {
        for round in 0..REOPENS {
            let again = reduce(json!({"commands": [], "now": NOW, key: output[key]}));
            assert_eq!(again[key], output[key], "{key} reopen {round}");
        }
    }
}

#[test]
fn terminal_intent_mismatch_is_rejected_in_both_directions() {
    for (anchor, intent) in [
        (FINE_LEAP, "2016-12-31T23:59:60.123456788Z"),
        (FINE_LEAP, "2016-12-31T23:59:60.12345679Z"),
        ("2016-12-31T23:59:60Z", "2016-12-31T23:59:59.999999999Z"),
        ("2016-12-31T23:59:59.999999999Z", "2016-12-31T23:59:60Z"),
        // Same fraction one second away across the leap: Unix-nanos and
        // leap-stripped equality would both accept these.
        ("2016-12-31T23:59:60.5Z", "2016-12-31T23:59:59.5Z"),
        ("2016-12-31T23:59:59.5Z", "2016-12-31T23:59:60.5Z"),
        ("2016-12-31T23:59:60.5Z", "2017-01-01T00:00:00.5Z"),
        ("2017-01-01T00:00:00.5Z", "2016-12-31T23:59:60.5Z"),
    ] {
        for status in ["completed", "cancelled"] {
            let mut input = request(anchor, status);
            input["base"]["canonicalTimer"]["lastIntent"]["occurredAt"] = json!(intent);
            let error = call("workspace.project.v1", &input).unwrap_err();
            assert!(
                error.contains("conflicting workspace terminal timer/history"),
                "{status}/{anchor}/{intent}: {error}"
            );
        }
    }
}
