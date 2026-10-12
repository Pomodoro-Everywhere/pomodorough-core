use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const DOMAINS: [&str; 5] = [
    "commands",
    "taskOperations",
    "durationOperations",
    "autoStartOperations",
    "selectedTaskOperations",
];

fn descriptor(index: usize, domain: usize) -> Value {
    let mut op = json!({"id": format!("op-{index:05}"), "deviceId": "device-a",
        "hlcWallMs": index + 1, "hlcCounter": 0});
    if domain == 0 {
        op["deviceSequence"] = json!(index + 1);
    }
    op
}

fn new_request() -> Value {
    let queues: serde_json::Map<_, _> = DOMAINS
        .iter()
        .enumerate()
        .map(|(d, n)| ((*n).into(), json!([descriptor(0, d)])))
        .collect();
    json!({"kind": "new", "mode": "sync", "nextDomain": "commands",
        "limits": {"perDomain": 256, "total": 512},
        "queues": queues, "timerDependencies": []})
}

fn saved_request() -> Value {
    let new = new_request();
    let mut queues = serde_json::Map::new();
    for name in DOMAINS {
        queues.insert(name.into(), json!(["op-00000"]));
    }
    json!({"kind": "saved", "mode": "sync",
        "limits": new["limits"], "queues": queues})
}

fn call(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("sync.batchPlan.v1", &input.to_string()).unwrap()).unwrap()
}

fn assert_rejected(input: &Value, first: &str, second: &str) {
    let err = dispatch_json("sync.batchPlan.v1", &input.to_string())
        .expect_err("accepted malformed batch shape");
    let text = err.to_string();
    assert!(text.contains(first), "{text}");
    assert!(text.contains(second), "{text}");
}

#[test]
fn new_rejects_positional_records_and_keeps_control() {
    let base = new_request();
    let mut limits = base.clone();
    limits["limits"] = json!([256, 512]);
    assert_rejected(&limits, "limits", "object");
    let mut desc = base.clone();
    desc["queues"]["commands"] = json!([["op-00000", "device-a", 1, 0, 1]]);
    assert_rejected(&desc, "queues.commands[]", "object");
    let mut dep = base.clone();
    dep["queues"]["commands"] = json!([descriptor(0, 0), descriptor(1, 0)]);
    dep["timerDependencies"] = json!([["op-00001", "op-00000"]]);
    assert_rejected(&dep, "timerDependencies[]", "object");
    let out = call(&base);
    assert_eq!(out["status"], "planned");
    assert_eq!(out["selected"]["commands"], json!(["op-00000"]));
    assert_eq!(call(&base), out);
}

#[test]
fn new_rejects_object_enums_and_keeps_control() {
    let base = new_request();
    let mut mode = base.clone();
    mode["mode"] = json!({"sync": null});
    assert_rejected(&mode, "mode", "string enum");
    let mut next = base.clone();
    next["nextDomain"] = json!({"commands": null});
    assert_rejected(&next, "nextDomain", "string enum");
    let mut merge = base.clone();
    merge["mode"] = json!({"merge": null});
    merge["limits"] = json!({"perDomain": 4096, "total": 8192});
    assert_rejected(&merge, "mode", "string enum");
    let out = call(&base);
    assert_eq!(out["status"], "planned");
    assert_eq!(out["nextDomain"], "commands");
}

#[test]
fn saved_rejects_shapes_and_keeps_exact_claim() {
    let base = saved_request();
    let mut limits = base.clone();
    limits["limits"] = json!([256, 512]);
    assert_rejected(&limits, "limits", "object");
    let mut mode = base.clone();
    mode["mode"] = json!({"sync": null});
    assert_rejected(&mode, "mode", "string enum");
    let mut ids = base.clone();
    ids["queues"]["commands"] = json!([["op-00000"]]);
    assert_rejected(&ids, "queues.commands[]", "string");
    let out = call(&base);
    assert_eq!(out["status"], "replay_saved");
    assert_eq!(out["selected"], base["queues"]);
    assert!(out["nextDomain"].is_null());
    assert_eq!(call(&base), out);
}

#[test]
fn fairness_limits_barriers_and_replay_preserved() {
    let mut tiny = new_request();
    tiny["limits"] = json!({"perDomain": 1, "total": 1});
    let out = call(&tiny);
    assert_eq!(out["selected"]["commands"], json!(["op-00000"]));
    assert_eq!(out["nextDomain"], "taskOperations");
    let mut big = new_request();
    big["mode"] = json!("merge");
    big["limits"] = json!({"perDomain": 4096, "total": 8192});
    big["queues"]["commands"] = json!((0..5000).map(|i| descriptor(i, 0)).collect::<Vec<_>>());
    assert_eq!(call(&big)["status"], "oversized");
    let mut held = new_request();
    held["queues"]["commands"] = json!([descriptor(0, 0), descriptor(1, 0)]);
    held["timerDependencies"] =
        json!([{"operationId": "op-00001", "dependsOnOperationId": "op-00000"}]);
    let barrier = call(&held);
    assert_eq!(barrier["heldTimerOperationId"], "op-00001");
    assert_eq!(call(&held), barrier);
    let saved = saved_request();
    assert_eq!(call(&saved)["selected"], saved["queues"]);
}

#[test]
fn unknown_fields_still_rejected() {
    let base = new_request();
    let mut root = base.clone();
    root["extension"] = json!(1);
    assert_rejected(&root, "extension", "unknown");
    let mut desc = base.clone();
    desc["queues"]["commands"][0]["extra"] = json!(1);
    assert!(dispatch_json("sync.batchPlan.v1", &desc.to_string()).is_err());
    let mut limits = base.clone();
    limits["limits"]["other"] = json!(1);
    assert!(dispatch_json("sync.batchPlan.v1", &limits.to_string()).is_err());
    let saved = saved_request();
    for extra in ["nextDomain", "timerDependencies"] {
        let mut input = saved.clone();
        input[extra] = json!([]);
        assert!(dispatch_json("sync.batchPlan.v1", &input.to_string()).is_err());
    }
}
