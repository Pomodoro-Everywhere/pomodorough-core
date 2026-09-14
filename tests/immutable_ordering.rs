use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const WALL: i64 = 1_784_548_800_000;

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn queues(operations: Vec<Value>) -> Value {
    json!({"commands": [], "taskOperations": [], "durationOperations": [],
        "autoStartOperations": operations, "selectedTaskOperations": []})
}

fn operation(id: &str, device: &str, counter: i64, enabled: bool) -> Value {
    json!({"id": id, "deviceId": device, "occurredAt": "2026-07-20T12:00:00Z",
        "hlcWallMs": WALL, "hlcCounter": counter, "enabled": enabled})
}

fn request(operations: Vec<Value>) -> Value {
    json!({"local": queues(operations), "sent": queues(vec![]), "timerDependencies": [],
        "response": {"revision": 9, "canonicalTimer": null, "history": [], "tasks": [],
            "durationsMs": {"focus": 1500000, "short_break": 300000, "long_break": 900000},
            "autoStartBreaks": false, "selectedTaskId": null,
            "serverTime": "2026-07-20T12:00:00Z", "serverHlcWallMs": WALL, "serverHlcCounter": 10,
            "acknowledgements": [], "taskAcknowledgements": [], "durationAcknowledgements": [],
            "autoStartAcknowledgements": [], "selectedTaskAcknowledgements": []}})
}

fn restart(input: &Value) -> Value {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "pomo-immutable-ordering-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&path, input.to_string()).unwrap();
    let encoded = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    serde_json::from_slice(&encoded).unwrap()
}

#[test]
fn acknowledged_newer_write_remains_a_barrier_after_second_call_and_disk_restart() {
    let older = operation("older-true", "same-device", 0, true);
    let newer = operation("newer-false", "same-device", 1, false);
    let mut input = request(vec![older.clone(), newer.clone()]);
    input["sent"] = queues(vec![newer.clone()]);
    input["neverSent"] = json!({"autoStartOperations": ["older-true"]});
    input["response"]["autoStartAcknowledgements"] = json!([{
        "operationId": "newer-false", "outcome": "applied", "reason": ""}]);
    for attempt in 0..3 {
        let output = call("reconcile.rebase.v2", &input);
        assert_eq!(output["pendingAutoStartOperations"], json!([older]));
        assert_eq!(output["autoStartBreaks"], false, "attempt {attempt}");
        assert_eq!(
            output["projectionPending"]["autoStartOperations"],
            json!([])
        );
        let replay = call(
            "autoStart.reduce.v1",
            &json!({"operations": [older, newer]}),
        );
        assert_eq!(replay["autoStartBreaks"], false);
        input["local"] = queues(
            output["pendingAutoStartOperations"]
                .as_array()
                .unwrap()
                .clone(),
        );
        input["sent"] = queues(vec![]);
        input["response"]["autoStartAcknowledgements"] = json!([]);
        if attempt == 1 {
            input = restart(&input);
        }
    }
}

#[test]
fn equal_hlc_preferences_keep_authoritative_device_before_id_order() {
    let a = operation("z-id", "a-device", 0, true);
    let z = operation("a-id", "z-device", 0, false);
    for operations in [vec![a.clone(), z.clone()], vec![z.clone(), a.clone()]] {
        let mut input = request(operations.clone());
        input["neverSent"] = json!({"autoStartOperations": ["z-id", "a-id"]});
        // Both writes are strictly newer than this canonical head; projection
        // must run the authoritative reducer without synthesizing new clocks.
        input["response"]["serverTime"] = json!("2026-07-20T11:59:59Z");
        input["response"]["serverHlcWallMs"] = json!(WALL - 1000);
        input["response"]["serverHlcCounter"] = json!(0);
        for attempt in 0..3 {
            let output = call("reconcile.rebase.v2", &input);
            assert_eq!(output["pendingAutoStartOperations"], json!(operations));
            assert_eq!(
                output["projectionPending"]["autoStartOperations"],
                json!(operations)
            );
            assert_eq!(output["autoStartBreaks"], false);
            let replay = call("autoStart.reduce.v1", &json!({"operations": operations}));
            assert_eq!(replay["winningOperationId"], "a-id");
            assert_eq!(replay["autoStartBreaks"], output["autoStartBreaks"]);
            input["local"] = queues(
                output["pendingAutoStartOperations"]
                    .as_array()
                    .unwrap()
                    .clone(),
            );
            if attempt == 1 {
                input = restart(&input);
            }
        }
        // V1 is deliberately isolated: pin its shipped compatibility behavior.
        let legacy = call("reconcile.rebase.v1", &request(vec![a.clone(), z.clone()]));
        assert_eq!(legacy["autoStartBreaks"], true);
    }
}

#[test]
fn canonical_head_ties_and_mixed_old_new_queues_are_not_optimistically_replayed() {
    for counter in [9, 10, 11] {
        let op = operation("local-true", "same-device", counter, true);
        let mut input = request(vec![op.clone()]);
        input["neverSent"] = json!({"autoStartOperations": ["local-true"]});
        let output = call("reconcile.rebase.v2", &input);
        assert_eq!(output["pendingAutoStartOperations"], json!([op]));
        assert_eq!(output["autoStartBreaks"], counter > 10);
    }
    let mut input = request(vec![
        operation("older", "device", 0, true),
        operation("newer", "device", 11, true),
    ]);
    input["neverSent"] = json!({"autoStartOperations": ["older", "newer"]});
    let output = call("reconcile.rebase.v2", &input);
    assert_eq!(output["autoStartBreaks"], false);
    assert_eq!(
        output["projectionPending"]["autoStartOperations"],
        json!([])
    );
}
