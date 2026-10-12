use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

const DECIMAL_RAW: &str = "90.49999999999999";
const QUEUES: [(&str, &str); 5] = [
    ("commands", "pending"),
    ("taskOperations", "pendingTaskOperations"),
    ("durationOperations", "pendingDurationOperations"),
    ("autoStartOperations", "pendingAutoStartOperations"),
    ("selectedTaskOperations", "pendingSelectedTaskOperations"),
];

fn host_decimal() -> f64 {
    DECIMAL_RAW.parse::<f64>().unwrap()
}

fn base_request() -> Value {
    json!({"local": {}, "sent": {}, "response": {
        "acknowledgements": [], "taskAcknowledgements": [],
        "durationAcknowledgements": [], "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [], "revision": 1,
        "canonicalTimer": null, "history": [], "tasks": [],
        "durationsMs": {"focus": 1500000, "short_break": 300000, "long_break": 900000},
        "autoStartBreaks": false, "selectedTaskId": null,
        "serverTime": "2026-07-20T12:00:10Z",
        "serverHlcWallMs": 1784548810000_i64, "serverHlcCounter": 0}})
}

fn base_operation(queue: &str) -> Value {
    let mut value = json!({"id": "frozen", "deviceId": "device",
        "occurredAt": "2026-07-20T12:00:00Z",
        "hlcWallMs": 1784548800000_i64, "hlcCounter": 0});
    let fields = match queue {
        "commands" => json!({"deviceSequence": 1, "timerId": "timer-a",
            "type": "start", "phase": "focus", "plannedDurationMs": 60000,
            "observedElapsedMs": 0}),
        "taskOperations" => json!({"type": "delete", "taskId": "task"}),
        "durationOperations" => json!({"phase": "focus", "durationMs": 60000}),
        "autoStartOperations" => json!({"enabled": true}),
        "selectedTaskOperations" => json!({"taskId": null}),
        _ => unreachable!(),
    };
    value
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    value["extension"] = json!({"value": "__DECIMAL__"});
    value
}

fn raw_input(queue: &str) -> String {
    let mut input = base_request();
    input["local"][queue] = json!([base_operation(queue)]);
    input.to_string().replace("\"__DECIMAL__\"", DECIMAL_RAW)
}

fn output_value(operation: &str, raw: &str) -> (String, Value) {
    let out = dispatch_json(operation, raw).unwrap();
    let value: Value = serde_json::from_str(&out).unwrap();
    (out, value)
}

fn check_envelope(operation: &str, raw: &str, pending: &str, host: f64) {
    let env = dispatch_envelope_json(operation, raw);
    assert!(env.contains(DECIMAL_RAW), "{operation} envelope raw");
    let envelope: Value = serde_json::from_str(&env).unwrap();
    assert_eq!(envelope["ok"], json!(true));
    let kept = &envelope["value"][pending][0]["extension"]["value"];
    assert_eq!(kept.as_f64().unwrap(), host, "{operation} envelope host");
}

fn check_reopen(operation: &str, raw: &str, pending: &str, first: &Value) {
    let mut reopen: Value = serde_json::from_str(raw).unwrap();
    reopen["local"] = json!({});
    let queue = QUEUES.iter().find(|(_, p)| *p == pending).unwrap().0;
    reopen["local"][queue] = first[pending].clone();
    reopen["response"]["serverHlcCounter"] = json!(99);
    let (second_str, second) = output_value(operation, &reopen.to_string());
    assert!(second_str.contains(DECIMAL_RAW), "{operation} reopen raw");
    assert_eq!(second[pending], first[pending], "{operation} reopen equal");
}

fn check_queue(queue: &str, pending: &str) {
    let host = host_decimal();
    assert!(host != 90.5, "host must distinguish decimal");
    let raw = raw_input(queue);
    assert!(raw.contains(DECIMAL_RAW), "input raw token");
    for operation in ["reconcile.rebase.v2", "reconcile.rebase.v3"] {
        let (out_str, out) = output_value(operation, &raw);
        assert!(out_str.contains(DECIMAL_RAW), "{operation} {queue} raw");
        assert_eq!(out[pending][0]["id"], json!("frozen"));
        let kept = &out[pending][0]["extension"]["value"];
        assert_eq!(kept.as_f64().unwrap(), host, "{operation} {queue} host");
        assert!(kept.as_f64().unwrap() != 90.5, "{operation} {queue}");
        check_envelope(operation, &raw, pending, host);
        check_reopen(operation, &raw, pending, &out);
    }
}

#[test]
fn frozen_decimal_preserved_in_all_retained_queues() {
    for (queue, pending) in QUEUES {
        check_queue(queue, pending);
    }
}
