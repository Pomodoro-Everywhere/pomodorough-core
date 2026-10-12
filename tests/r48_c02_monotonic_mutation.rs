use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const JUMP_WALL: &str = "2026-07-20T12:01:20Z";
const BASE_WALL: &str = "2026-07-20T12:00:10Z";
const CONTINUITY: &str = "browser-session-1";

fn running_timer() -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    fixture["timer"].clone()
}

fn mutation_input(intent: Value, uuids: Value) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let mut input = fixture["request"].clone();
    input["compatibility"] = json!("pwaStorage");
    input["replicationMode"] = json!("centralized");
    input["intent"] = intent;
    input["workspace"]["base"]["canonicalTimer"] = running_timer();
    input["ownership"] = json!({"expectedOwnerId": "account-a", "ownerId": "account-a"});
    input["durability"] = json!({"outgoingDurationOperationIds": [], "localTabId": "tab-a"});
    input["clock"]["occurredAt"] = json!(BASE_WALL);
    input["clock"]["physicalNow"] = json!(BASE_WALL);
    input["clock"]["observedAt"] = json!(BASE_WALL);
    input["clock"]["monotonicNowMs"] = json!(100.0);
    input["clock"]["continuityId"] = json!(CONTINUITY);
    input["identities"]["commandUuids"] = uuids;
    input
}

fn plan(input: &Value) -> Value {
    serde_json::from_str(
        &dispatch_json("workspace.intent.v1", &input.to_string())
            .unwrap_or_else(|error| panic!("{error}: {input}")),
    )
    .unwrap()
}

fn jumped_mutation(prior: &Value, intent: Value, uuids: Value, monotonic: f64) -> Value {
    let mut input = mutation_input(intent, uuids);
    input["workspace"] = prior["workspace"].clone();
    input["allocation"] = prior["allocation"].clone();
    input["observation"] = prior["observation"].clone();
    for field in ["occurredAt", "physicalNow", "observedAt"] {
        input["clock"][field] = json!(JUMP_WALL);
    }
    input["clock"]["monotonicNowMs"] = json!(monotonic);
    input["clock"]["continuityId"] = json!(CONTINUITY);
    input
}

fn assert_running(output: &Value, elapsed: i64, remaining: i64, anchor: &Value) {
    assert_eq!(output["projection"]["canonicalTimer"]["status"], "running");
    assert_eq!(
        output["projection"]["canonicalTimer"]["id"],
        "existing-timer"
    );
    assert_eq!(output["projection"]["history"], json!([]));
    assert_eq!(output["timerObservation"]["elapsedMs"], json!(elapsed));
    assert_eq!(output["timerObservation"]["remainingMs"], json!(remaining));
    assert_eq!(output["observation"]["monotonicAnchor"], *anchor);
}

fn read_running(workspace: &Value, anchor: &Value, monotonic: f64) -> Value {
    let input = json!({
        "profile": "pwaStorage",
        "source": {"kind": "workspace", "value": workspace},
        "selectedPhase": "focus",
        "observedAt": JUMP_WALL,
        "calendarIntervals": [{"start": "2026-07-20T00:00:00Z", "end": "2026-07-21T00:00:00Z"}],
        "monotonic": {"nowMs": monotonic, "continuityId": CONTINUITY, "anchor": anchor},
    });
    serde_json::from_str(&dispatch_json("workspace.readModel.v1", &input.to_string()).unwrap())
        .unwrap()
}

fn pause_from(prior: &Value, uuids: Value, monotonic: f64) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let mut input = fixture["request"].clone();
    input["compatibility"] = json!("pwaStorage");
    input["replicationMode"] = json!("centralized");
    input["intent"] = json!({"kind": "pause"});
    input["workspace"] = prior["workspace"].clone();
    input["allocation"] = prior["allocation"].clone();
    input["observation"] = prior["observation"].clone();
    for field in ["occurredAt", "physicalNow", "observedAt"] {
        input["clock"][field] = json!(JUMP_WALL);
    }
    input["clock"]["monotonicNowMs"] = json!(monotonic);
    input["clock"]["continuityId"] = json!(CONTINUITY);
    input["identities"]["commandUuids"] = uuids;
    input
}

fn base_uuids(suffixes: &[&str]) -> Value {
    json!(
        suffixes
            .iter()
            .map(|s| format!("019f7f65-dd10-7000-8000-00000000000{s}"))
            .collect::<Vec<_>>()
    )
}

fn jumped_uuids(suffixes: &[&str]) -> Value {
    json!(
        suffixes
            .iter()
            .map(|s| format!("019f7f66-ee80-7000-8000-00000000000{s}"))
            .collect::<Vec<_>>()
    )
}

#[test]
fn pwa_settings_mutation_preserves_monotonic_timer_through_read_and_pause() {
    let first = plan(&mutation_input(
        json!({"kind": "setAutoStart", "enabled": true}),
        base_uuids(&["1", "2", "3"]),
    ));
    let anchor = first["observation"]["monotonicAnchor"].clone();
    assert_running(&first, 15000, 45000, &anchor);
    let second = plan(&jumped_mutation(
        &first,
        json!({"kind": "setDuration", "phase": "focus", "minutes": 40}),
        jumped_uuids(&["1", "2", "3"]),
        1100.0,
    ));
    assert_eq!(second["operations"]["commands"], json!([]));
    assert_eq!(second["projection"]["durationsMs"]["focus"], 2_400_000);
    assert_running(&second, 16000, 44000, &anchor);
    let read = read_running(&second["workspace"], &anchor, 1100.0);
    assert_eq!(read["canonical"]["status"], "running");
    assert_eq!(read["canonical"]["elapsedMs"], 16000.0);
    assert_eq!(read["canonical"]["remainingMs"], 44000.0);
    assert!(
        read["availableIntents"]
            .as_array()
            .unwrap()
            .contains(&json!("pause"))
    );
    let paused = plan(&pause_from(&second, jumped_uuids(&["4"]), 2100.0));
    assert_eq!(paused["commands"][0]["type"], "pause");
    assert_eq!(paused["commands"][0]["observedElapsedMs"], 17000);
    assert_eq!(paused["projection"]["canonicalTimer"]["status"], "paused");
}

#[test]
fn pwa_task_mutation_preserves_monotonic_timer_at_wall_jump() {
    let first = plan(&mutation_input(
        json!({"kind": "setAutoStart", "enabled": true}),
        base_uuids(&["1", "2", "3"]),
    ));
    let anchor = first["observation"]["monotonicAnchor"].clone();
    let second = plan(&jumped_mutation(
        &first,
        json!({"kind": "upsertTask", "title": "Wall jump task"}),
        jumped_uuids(&["1", "2", "3"]),
        1100.0,
    ));
    assert_eq!(second["operations"]["commands"], json!([]));
    assert_eq!(
        second["operations"]["taskOperations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_running(&second, 16000, 44000, &anchor);
    assert_eq!(second["projection"]["tasks"].as_array().unwrap().len(), 1);
    let paused = plan(&pause_from(&second, jumped_uuids(&["4"]), 1100.0));
    assert_eq!(paused["commands"][0]["observedElapsedMs"], 16000);
}

#[test]
fn pwa_mutation_noop_keeps_protected_projection() {
    let first = plan(&mutation_input(
        json!({"kind": "setAutoStart", "enabled": true}),
        base_uuids(&["1", "2", "3"]),
    ));
    let anchor = first["observation"]["monotonicAnchor"].clone();
    let mut repeat = jumped_mutation(
        &first,
        json!({"kind": "setAutoStart", "enabled": true}),
        jumped_uuids(&["4", "5", "6"]),
        1100.0,
    );
    let noop = plan(&repeat);
    assert_eq!(noop["outcome"], "noop");
    assert_eq!(noop["workspace"], repeat["workspace"]);
    assert_eq!(noop["observation"]["monotonicAnchor"], anchor);
    assert_eq!(noop["projection"]["canonicalTimer"]["status"], "running");
    assert_eq!(noop["timerObservation"]["elapsedMs"], json!(16000));
    repeat["clock"]
        .as_object_mut()
        .unwrap()
        .remove("monotonicNowMs");
    repeat["clock"]
        .as_object_mut()
        .unwrap()
        .remove("continuityId");
    let wall_only = plan(&repeat);
    assert_eq!(
        wall_only["projection"]["canonicalTimer"]["status"],
        "completed"
    );
}

#[test]
fn pwa_mutation_real_expiry_still_completes_and_blocks_pause() {
    let first = plan(&mutation_input(
        json!({"kind": "setAutoStart", "enabled": true}),
        base_uuids(&["1", "2", "3"]),
    ));
    let expired = plan(&jumped_mutation(
        &first,
        json!({"kind": "upsertTask", "title": "Genuine expiry"}),
        jumped_uuids(&["1", "2", "3"]),
        60000.0,
    ));
    assert_eq!(
        expired["projection"]["canonicalTimer"]["status"],
        "completed"
    );
    assert_eq!(expired["timerObservation"]["remainingMs"], json!(0));
    assert_eq!(expired["observation"]["monotonicAnchor"], Value::Null);
    let paused = plan(&pause_from(&expired, jumped_uuids(&["4"]), 61000.0));
    assert_eq!(paused["outcome"], "noop");
    assert_eq!(paused["commands"], json!([]));
}
