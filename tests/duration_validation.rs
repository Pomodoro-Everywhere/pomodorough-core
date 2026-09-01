use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const PHASES: [&str; 3] = ["focus", "short_break", "long_break"];

fn queues() -> Value {
    json!({
        "commands": [],
        "taskOperations": [],
        "durationOperations": [],
        "autoStartOperations": [],
        "selectedTaskOperations": []
    })
}

fn durations() -> Value {
    json!({
        "focus": 1_500_000,
        "short_break": 300_000,
        "long_break": 900_000
    })
}

fn projection_input() -> Value {
    json!({
        "base": {
            "canonicalTimer": null,
            "history": [],
            "tasks": [],
            "durationsMs": durations(),
            "autoStartBreaks": false,
            "selectedTaskId": null
        },
        "pending": queues(),
        "now": "2026-08-22T12:00:00Z"
    })
}

fn canonical_response() -> Value {
    json!({
        "acknowledgements": [],
        "taskAcknowledgements": [],
        "durationAcknowledgements": [],
        "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [],
        "revision": 1,
        "canonicalTimer": null,
        "history": [],
        "tasks": [],
        "durationsMs": durations(),
        "autoStartBreaks": false,
        "selectedTaskId": null,
        "serverTime": "2026-07-20T12:00:10Z",
        "serverHlcWallMs": 1_784_548_810_000_i64,
        "serverHlcCounter": 0
    })
}

fn dispatch(operation: &str, input: Value) -> Result<Value, String> {
    dispatch_json(operation, &input.to_string())
        .map_err(|error| error.to_string())
        .and_then(|output| serde_json::from_str(&output).map_err(|error| error.to_string()))
}

fn project_base(phase: &str, duration_ms: i64) -> Result<Value, String> {
    let mut input = projection_input();
    input["base"]["durationsMs"][phase] = json!(duration_ms);
    dispatch("projection.apply.v2", input)
}

fn project_operation(phase: &str, duration_ms: i64) -> Result<Value, String> {
    let operation = json!({
        "id": "duration-operation",
        "deviceId": "device-a",
        "occurredAt": "2026-08-22T12:00:00Z",
        "hlcWallMs": 2_000,
        "hlcCounter": 0,
        "phase": phase,
        "durationMs": duration_ms
    });
    let mut input = projection_input();
    input["pending"]["durationOperations"] = json!([operation]);
    dispatch("projection.apply.v2", input)
}

fn rebase(phase: &str, duration_ms: i64) -> Result<Value, String> {
    let mut response = canonical_response();
    response["durationsMs"][phase] = json!(duration_ms);
    let input = json!({"local": queues(), "sent": queues(), "response": response});
    dispatch("reconcile.rebase.v1", input)
}

fn assert_contract(phase: &str, duration_ms: i64, expected: bool) {
    for (route, result) in [
        ("projection base", project_base(phase, duration_ms)),
        (
            "projection operation",
            project_operation(phase, duration_ms),
        ),
        ("rebase", rebase(phase, duration_ms)),
    ] {
        assert_eq!(
            result.is_ok(),
            expected,
            "{route} phase={phase} durationMs={duration_ms}: {result:?}"
        );
    }
}

#[test]
fn projection_and_rebase_accept_inclusive_duration_boundaries_for_every_phase() {
    for phase in PHASES {
        for duration_ms in [60_000, 10_800_000] {
            assert_contract(phase, duration_ms, true);
        }
    }
}

#[test]
fn projection_and_rebase_reject_invalid_duration_boundaries_for_every_phase() {
    for phase in PHASES {
        for duration_ms in [59_999, 61_000, 10_800_001, 10_860_000, 14_400_000] {
            assert_contract(phase, duration_ms, false);
        }
    }
}
