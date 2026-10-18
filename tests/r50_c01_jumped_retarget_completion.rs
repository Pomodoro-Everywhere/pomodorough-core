use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const CONTINUITY: &str = "browser-session-1";
const SEED_WALL: &str = "2026-07-20T12:00:10Z";
const ORDINARY_WALL: &str = "2026-07-20T12:00:11Z";
const JUMP_WALL: &str = "2026-07-20T12:01:20Z";
const MANUAL_WALL: &str = "2026-07-20T12:01:21Z";
const FINISH_WALL: &str = "2026-07-20T12:02:21Z";

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap()
}

fn running_timer() -> Value {
    let mut timer = fixture()["timer"].clone();
    timer["lastIntent"] = json!({"type": "start", "commandId": "start-original",
        "occurredAt": "2026-07-20T12:00:00Z"});
    timer
}

fn uuids(wall: &str, count: usize) -> Value {
    let ms = match wall {
        SEED_WALL => 1_784_548_810_000_i64,
        ORDINARY_WALL => 1_784_548_811_000_i64,
        JUMP_WALL => 1_784_548_880_000_i64,
        MANUAL_WALL => 1_784_548_881_000_i64,
        FINISH_WALL => 1_784_548_941_000_i64,
        _ => panic!("unknown test wall"),
    };
    let prefix = format!("{ms:012x}");
    json!(
        (1..=count)
            .map(|i| format!("{}-{}-7000-8000-{i:012x}", &prefix[..8], &prefix[8..]))
            .collect::<Vec<_>>()
    )
}

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(
        &dispatch_json(operation, &input.to_string())
            .unwrap_or_else(|error| panic!("{error}: {input}")),
    )
    .unwrap()
}

fn intent(prior: Option<&Value>, wall: &str, mono: f64, kind: Value) -> Value {
    let base = fixture()["request"].clone();
    let mut input = base;
    input["compatibility"] = json!("pwaStorage");
    input["workspace"]["base"]["canonicalTimer"] = running_timer();
    input["workspace"]["displayContext"] =
        json!({"profile": "pwaStorage", "projectionPending": null});
    input["lifecycle"] =
        json!({"consumedCompletions": [], "pendingBreaks": [], "finishEvidence": []});
    if let Some(prior) = prior {
        for field in [
            "workspace",
            "allocation",
            "observation",
            "selection",
            "lifecycle",
        ] {
            input[field] = prior[field].clone();
        }
    }
    input["intent"] = kind;
    input["ownership"] = json!({"ownerId": null, "expectedOwnerId": null});
    input["durability"] = json!({"outgoingDurationOperationIds": [], "localTabId": "tab-a"});
    for field in ["occurredAt", "physicalNow", "observedAt"] {
        input["clock"][field] = json!(wall);
    }
    input["clock"]["monotonicNowMs"] = json!(mono);
    input["clock"]["continuityId"] = json!(CONTINUITY);
    input["identities"]["commandUuids"] = uuids(wall, 3);
    call("workspace.intent.v1", &input)
}

fn finish_input(prior: &Value, automatic: bool, mono: f64) -> Value {
    let mut input = fixture()["request"].clone();
    input.as_object_mut().unwrap().remove("intent");
    input["compatibility"] = json!("pwaStorage");
    input["stage"] = json!(if automatic {
        "automaticFinishCommit"
    } else {
        "finishCommit"
    });
    for field in [
        "workspace",
        "allocation",
        "observation",
        "selection",
        "lifecycle",
    ] {
        input[field] = prior[field].clone();
    }
    input["requestedTimer"] = prior["projection"]["canonicalTimer"].clone();
    input["ownership"] = json!({"timerId": "existing-timer", "deviceId": "device-local",
        "tabId": "tab-a", "leaseExpiresAtMs": 1784549200000_i64});
    input["localTabId"] = json!("tab-a");
    input["leaseNowMs"] = json!(1784548941000_i64);
    input["leaseDurationMs"] = json!(30000);
    let wall = if automatic { FINISH_WALL } else { MANUAL_WALL };
    for field in ["occurredAt", "physicalNow", "observedAt"] {
        input["clock"][field] = json!(wall);
    }
    input["clock"]["monotonicNowMs"] = json!(mono);
    input["clock"]["continuityId"] = json!(CONTINUITY);
    input["identities"]["commandUuids"] = uuids(wall, 2);
    input
}

fn finish(prior: &Value, automatic: bool, mono: f64) -> Value {
    call(
        "workspace.completionMutation.v1",
        &finish_input(prior, automatic, mono),
    )
}

fn seed() -> Value {
    intent(
        None,
        SEED_WALL,
        100.0,
        json!({"kind": "setAutoStart", "enabled": false}),
    )
}

fn select(wall: &str) -> Value {
    let prior = seed();
    intent(
        Some(&prior),
        wall,
        1100.0,
        json!({"kind": "addAndSelectTask", "title": "new task"}),
    )
}

#[test]
fn jumped_wall_retarget_admits_then_automatic_finish_plans() {
    let selected = select(JUMP_WALL);
    let task = selected["projection"]["tasks"][0]["id"].clone();
    assert_eq!(selected["projection"]["canonicalTimer"]["taskId"], task);
    assert_eq!(
        selected["projection"]["canonicalTimer"]["status"],
        "running"
    );
    let retarget = selected["operations"]["commands"][0].clone();
    assert_eq!(retarget["type"], "retarget");
    assert_eq!(
        selected["groupOutcomes"]["commands"][0]["outcome"],
        "applied"
    );
    let result = finish(&selected, true, 45100.0);
    assert_eq!(result["outcome"], "planned");
    assert_eq!(result["commands"].as_array().unwrap().len(), 1);
    assert_eq!(result["commands"][0]["type"], "finish");
    assert_eq!(result["commands"][0]["timerId"], "existing-timer");
    let history = &result["projection"]["history"][0];
    assert_eq!(history["commandId"], result["commands"][0]["id"]);
    assert_eq!(history["timerId"], "existing-timer");
    assert_eq!(history["taskId"], task);
    assert_eq!(history["status"], "completed");
    let manual = finish(&selected, false, 2100.0);
    assert_eq!(manual["outcome"], "planned");
    assert_eq!(manual["commands"].as_array().unwrap().len(), 1);
    assert_eq!(manual["commands"][0]["type"], "finish");
    assert_eq!(manual["commands"][0]["timerId"], "existing-timer");
    let manual_history = &manual["projection"]["history"][0];
    assert_eq!(manual_history["commandId"], manual["commands"][0]["id"]);
    assert_eq!(manual_history["timerId"], "existing-timer");
    assert_eq!(manual_history["taskId"], task);
    assert_eq!(manual_history["status"], "completed");
}

#[test]
fn ordinary_wall_automatic_finish_still_plans() {
    let selected = select(ORDINARY_WALL);
    assert_eq!(
        selected["projection"]["canonicalTimer"]["status"],
        "running"
    );
    let result = finish(&selected, true, 45100.0);
    assert_eq!(result["outcome"], "planned");
    assert_eq!(result["commands"][0]["type"], "finish");
}

#[test]
fn tampered_presented_timer_stays_stale_after_jump() {
    let selected = select(JUMP_WALL);
    for (field, bad) in [("taskId", json!("wrong")), ("id", json!("other-timer"))] {
        let mut input = finish_input(&selected, true, 45100.0);
        input["requestedTimer"][field] = bad;
        let result = call("workspace.completionMutation.v1", &input);
        assert_eq!(result["reason"], "staleTimer");
        assert_eq!(result["commands"], json!([]));
    }
}

#[test]
fn tampered_monotonic_anchor_stays_noop_after_jump() {
    let selected = select(JUMP_WALL);
    for (field, bad) in [
        ("timerId", "other-timer"),
        ("continuityId", "other-session"),
    ] {
        let mut input = finish_input(&selected, true, 45100.0);
        input["observation"]["monotonicAnchor"][field] = json!(bad);
        let result = call("workspace.completionMutation.v1", &input);
        assert_eq!(result["outcome"], "noop");
        assert_eq!(result["commands"], json!([]));
    }
}
