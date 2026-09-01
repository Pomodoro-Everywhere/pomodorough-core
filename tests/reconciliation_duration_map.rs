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

fn canonical_response(durations_ms: Value) -> Value {
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
        "durationsMs": durations_ms,
        "autoStartBreaks": false,
        "selectedTaskId": null,
        "serverTime": "2026-07-20T12:00:10Z",
        "serverHlcWallMs": 1_784_548_810_000_i64,
        "serverHlcCounter": 0
    })
}

fn rebase(durations_ms: Value) -> Result<Value, String> {
    let input = json!({
        "local": queues(),
        "sent": queues(),
        "response": canonical_response(durations_ms)
    });
    dispatch_json("reconcile.rebase.v1", &input.to_string())
        .map_err(|error| error.to_string())
        .and_then(|output| serde_json::from_str(&output).map_err(|error| error.to_string()))
}

fn valid_durations() -> Value {
    json!({
        "focus": 1_500_000,
        "short_break": 300_000,
        "long_break": 900_000
    })
}

#[test]
fn reconciliation_rejects_extra_duration_key() {
    let mut durations = valid_durations();
    durations["rest"] = json!(300_000);

    assert!(rebase(durations).is_err());
}

#[test]
fn reconciliation_rejects_each_missing_duration_key() {
    for phase in PHASES {
        let mut durations = valid_durations();
        durations.as_object_mut().unwrap().remove(phase);

        assert!(rebase(durations).is_err(), "missing {phase} was accepted");
    }
}

#[test]
fn reconciliation_rejects_malformed_duration_maps() {
    for durations in [
        Value::Null,
        json!([]),
        json!({"focus": "60000", "short_break": 300_000, "long_break": 900_000}),
        json!({"focus": 61_000, "short_break": 300_000, "long_break": 900_000}),
    ] {
        assert!(rebase(durations.clone()).is_err(), "accepted {durations}");
    }
}

#[test]
fn reconciliation_accepts_duration_boundaries_for_each_phase() {
    for phase in PHASES {
        for duration_ms in [60_000, 10_800_000] {
            let mut durations = valid_durations();
            durations[phase] = json!(duration_ms);

            assert!(
                rebase(durations).is_ok(),
                "rejected {phase} boundary {duration_ms}"
            );
        }
    }
}
