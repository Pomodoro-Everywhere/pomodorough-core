use pomodorough_core::dispatch_json;
use serde_json::Value;

const VALID_DURATIONS: &str =
    r#""durationsMs":{"focus":1500000,"long_break":900000,"short_break":300000}"#;
const INVALID_DURATIONS: &str =
    r#""durationsMs":{"focus":1,"long_break":900000,"short_break":300000}"#;
const VALID_OPERATION_DURATION: &str = r#""durationMs":1800000"#;
const INVALID_OPERATION_DURATION: &str = r#""durationMs":1"#;

const VALID_RAW_REBASE: &str = r#"{
    "local": {
        "commands": [],
        "taskOperations": [],
        "durationOperations": [{
            "id": "duration-local",
            "deviceId": "device-a",
            "phase": "focus",
            "durationMs": 1800000,
            "occurredAt": "2026-07-20T12:00:11Z",
            "hlcWallMs": 1784548811000,
            "hlcCounter": 0
        }],
        "autoStartOperations": [],
        "selectedTaskOperations": []
    },
    "sent": {
        "commands": [],
        "taskOperations": [],
        "durationOperations": [],
        "autoStartOperations": [],
        "selectedTaskOperations": []
    },
    "response": {
        "acknowledgements": [],
        "taskAcknowledgements": [],
        "durationAcknowledgements": [],
        "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [],
        "revision": 9,
        "canonicalTimer": null,
        "history": [],
        "tasks": [],
        "durationsMs": {"focus": 1500000, "short_break": 300000, "long_break": 900000},
        "autoStartBreaks": false,
        "selectedTaskId": null,
        "serverTime": "2026-07-20T12:00:10Z",
        "serverHlcWallMs": 1784548810000,
        "serverHlcCounter": 0
    },
    "timerDependencies": []
}"#;

fn dispatch(raw: &str) -> Result<Value, String> {
    dispatch_json("reconcile.rebase.v1", raw)
        .map_err(|error| error.to_string())
        .and_then(|output| serde_json::from_str(&output).map_err(|error| error.to_string()))
}

fn replace_unique(raw: &str, target: &str, replacement: &str) -> String {
    assert_eq!(raw.matches(target).count(), 1, "target must be unique");
    raw.replacen(target, replacement, 1)
}

fn compact(raw: &str) -> String {
    serde_json::from_str::<Value>(raw).unwrap().to_string()
}

fn assert_duplicate_rejected(raw: &str, field: &str) {
    let error = dispatch(raw).unwrap_err();
    assert!(
        error.starts_with("invalid shared-core JSON: duplicate field"),
        "unexpected error: {error}"
    );
    assert!(error.contains(field), "expected {field:?}, got {error}");
}

#[test]
fn raw_rebase_accepts_valid_payload_alias_and_unknown_fields() {
    let output = dispatch(VALID_RAW_REBASE).unwrap();
    assert_eq!(output["durationsMs"]["focus"], 1_800_000);

    let alias = VALID_RAW_REBASE.replacen(r#""local""#, r#""pending""#, 1);
    assert!(dispatch(&alias).is_ok());

    let with_unknowns = VALID_RAW_REBASE
        .replacen('{', r#"{"unknownRoot":true,"#, 1)
        .replacen(r#""local": {"#, r#""local":{"unknownQueue":true,"#, 1)
        .replacen(r#"[{"#, r#"[{"unknownOperation":true,"#, 1)
        .replacen(
            r#""response": {"#,
            r#""response":{"unknownResponse":true,"#,
            1,
        );
    assert!(dispatch(&with_unknowns).is_ok());
}

#[test]
fn raw_rebase_rejects_response_durations_duplicate_in_both_orders() {
    let compact = compact(VALID_RAW_REBASE);
    for fields in [
        format!("{INVALID_DURATIONS},{VALID_DURATIONS}"),
        format!("{VALID_DURATIONS},{INVALID_DURATIONS}"),
    ] {
        let duplicate = replace_unique(&compact, VALID_DURATIONS, &fields);
        assert_duplicate_rejected(&duplicate, "durationsMs");
    }
}

#[test]
fn raw_rebase_rejects_queued_duration_duplicate_in_both_orders() {
    let compact = compact(VALID_RAW_REBASE);
    for fields in [
        format!("{INVALID_OPERATION_DURATION},{VALID_OPERATION_DURATION}"),
        format!("{VALID_OPERATION_DURATION},{INVALID_OPERATION_DURATION}"),
    ] {
        let duplicate = replace_unique(&compact, VALID_OPERATION_DURATION, &fields);
        assert_duplicate_rejected(&duplicate, "durationMs");
    }
}

#[test]
fn raw_rebase_rejects_duplicate_queue_and_local_alias_fields() {
    let compact = compact(VALID_RAW_REBASE);
    let operation = compact
        .split(r#""durationOperations":["#)
        .nth(1)
        .and_then(|suffix| suffix.split(']').next())
        .unwrap();
    let queue = format!(r#""durationOperations":[],"durationOperations":[{operation}]"#);
    let duplicate_queue = replace_unique(
        &compact,
        &format!(r#""durationOperations":[{operation}]"#),
        &queue,
    );
    assert_duplicate_rejected(&duplicate_queue, "durationOperations");

    let local = serde_json::from_str::<Value>(VALID_RAW_REBASE).unwrap()["local"].to_string();
    let duplicate_alias =
        compact.replacen(r#""local":"#, &format!(r#""pending":{local},"local":"#), 1);
    assert_duplicate_rejected(&duplicate_alias, "local");
}
