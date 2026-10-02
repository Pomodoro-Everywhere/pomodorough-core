use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/bootstrap-workspace-v1.json")).unwrap()
}

fn call(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("bootstrap.workspacePlan.v1", &input.to_string()).unwrap())
        .unwrap()
}

fn operation(queue: &str) -> Value {
    let fixture = fixture();
    let mut value = fixture["operationClock"].clone();
    let fields = match queue {
        "commands" => return fixture["command"].clone(),
        "taskOperations" => json!({"taskId": "legacy-task", "type": "delete"}),
        "durationOperations" => json!({"phase": "focus", "durationMs": 1_500_000}),
        "autoStartOperations" => json!({"enabled": false}),
        "selectedTaskOperations" => json!({"taskId": null}),
        _ => unreachable!(),
    };
    value
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    value
}

#[test]
fn every_domain_is_meaningful_even_when_default_reset_or_possibly_delivered() {
    for profile in [
        "appleWorkspace",
        "androidRepository",
        "desktopStorage",
        "pwaStorage",
    ] {
        for queue in [
            "commands",
            "taskOperations",
            "durationOperations",
            "autoStartOperations",
            "selectedTaskOperations",
        ] {
            for delivery in ["never_sent", "unknown", "covered", "newer", "retired_proof"] {
                let mut input = queue_request(queue, delivery);
                input["profile"] = json!(profile);
                let original = input.clone();
                let output = call(&input);
                assert_eq!(
                    output["classification"]["local"],
                    json!({"hasState": true,
                    "completedHistoryCount": 0, "displayHistoryCount": 0}),
                    "{profile}/{queue}/{delivery}"
                );
                assert_eq!(
                    output["plan"],
                    json!({"mode": "auto", "strategy": "merge", "reason": "local_state_only"})
                );
                assert_eq!(input, original);
            }
        }
    }
}

fn queue_request(queue: &str, delivery: &str) -> Value {
    let mut input = fixture()["request"].clone();
    let value = operation(queue);
    let workspace = &mut input["local"]["workspace"];
    workspace["local"][queue] = json!([value]);
    match delivery {
        "never_sent" => workspace["neverSent"][queue] = json!([value["id"]]),
        "covered" => {
            workspace["canonicalHead"] = json!({"wallMs": 1_790_000_000_000_i64, "counter": 0})
        }
        "newer" => {
            workspace["canonicalHead"] = json!({"wallMs": 1_789_999_999_999_i64, "counter": 0})
        }
        _ => {}
    }
    input
}

fn terminal_input() -> Value {
    let mut input = fixture()["request"].clone();
    input["local"]["workspace"]["base"]["canonicalTimer"] = fixture()["timer"].clone();
    input
}

#[test]
fn standalone_terminal_and_matching_history_have_same_complete_result() {
    let input = terminal_input();
    let output = call(&input);
    assert_eq!(
        output["classification"]["local"]["completedHistoryCount"],
        1
    );
    assert_eq!(
        output["plan"],
        json!({"mode": "auto", "strategy": "replace_remote", "reason": "local_only"})
    );
    let mut reopened = input.clone();
    let raw = dispatch_json(
        "timer.reduce.v1",
        &json!({"canonicalTimer": input["local"]["workspace"]["base"]["canonicalTimer"],
        "history": [], "commands": [], "now": input["local"]["workspace"]["now"]})
        .to_string(),
    )
    .unwrap();
    reopened["local"]["workspace"]["base"]["history"] =
        serde_json::from_str::<Value>(&raw).unwrap()["history"].clone();
    assert_eq!(call(&reopened), output);
    reopened["remote"]["selectedTaskId"] = json!("legacy-selected");
    reopened["profile"] = json!("androidRepository");
    assert_eq!(
        call(&reopened)["plan"],
        json!({"mode": "choose", "localHistoryCount": 1, "remoteHistoryCount": 0})
    );
}

#[test]
fn raw_finish_count_obeys_safe_delivery_projection_and_survives_proof_retirement() {
    let mut input = fixture()["request"].clone();
    let start = fixture()["command"].clone();
    let mut finish = start.clone();
    finish["id"] = json!("finish-legacy");
    finish["type"] = json!("finish");
    finish["deviceSequence"] = json!(2);
    finish["hlcCounter"] = json!(1);
    finish["occurredAt"] = json!("2026-09-21T12:00:10Z");
    finish["observedElapsedMs"] = json!(60_000);
    input["local"]["workspace"]["local"]["commands"] = json!([start, finish]);
    let mut safe = input.clone();
    safe["local"]["workspace"]["neverSent"]["commands"] = json!(["start-legacy", "finish-legacy"]);
    safe["local"]["workspace"]["canonicalHead"] =
        json!({"wallMs": 1_789_999_999_999_i64, "counter": 0});
    assert_eq!(
        call(&input)["classification"]["local"]["completedHistoryCount"],
        0
    );
    let completed = call(&safe);
    assert_eq!(
        completed["classification"]["local"]["completedHistoryCount"], 1,
        "{completed} raw={safe}"
    );
    let projected: Value = serde_json::from_str(
        &dispatch_json(
            "workspace.project.v1",
            &safe["local"]["workspace"].to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    safe["local"]["workspace"]["base"] = projected["workspace"].clone();
    safe["local"]["workspace"]["neverSent"] = json!({});
    safe["local"]["workspace"]["canonicalHead"] =
        json!({"wallMs": 1_789_999_999_999_i64, "counter": 0});
    assert_eq!(call(&safe), completed);
    safe["remote"]["history"] = json!([{"id": "remote", "status": "completed"}]);
    assert_eq!(
        call(&safe)["plan"],
        json!({"mode": "choose", "localHistoryCount": 1, "remoteHistoryCount": 1})
    );
}

#[test]
fn android_raw_minute_settings_and_pwa_raw_defaults_keep_profile_policy() {
    let mut input = fixture()["request"].clone();
    input["profile"] = json!("androidRepository");
    input["local"]["preferences"]["durationsMs"] = Value::Null;
    assert_eq!(call(&input)["classification"]["local"]["hasState"], false);
    input["local"]["preferences"]["focusMinutes"] = json!(30);
    assert_eq!(call(&input)["classification"]["local"]["hasState"], true);
    input["local"]["preferences"]["durationsMs"] =
        fixture()["request"]["remote"]["durationsMs"].clone();
    assert_eq!(call(&input)["classification"]["local"]["hasState"], false);
    input["profile"] = json!("pwaStorage");
    input["local"]["preferences"]["defaultDurationsMs"] = input["remote"]["durationsMs"].clone();
    input["local"]["preferences"]["defaultDurationsMs"]["focus"] = json!(1_800_000);
    assert_eq!(call(&input)["classification"]["local"]["hasState"], true);
    assert_eq!(call(&input)["classification"]["remote"]["hasState"], true);
}

#[test]
fn every_terminal_pair_and_owner_change_returns_classification_without_changing_v1_precedence() {
    for (status, kind) in [
        ("completed", "finish"),
        ("cancelled", "cancel"),
        ("superseded", "start"),
    ] {
        let mut input = terminal_pair(status, kind);
        for profile in [
            "appleWorkspace",
            "androidRepository",
            "desktopStorage",
            "pwaStorage",
        ] {
            input["profile"] = json!(profile);
            let classification = call(&input)["classification"].clone();
            assert_eq!(
                classification["local"]["completedHistoryCount"],
                usize::from(status == "completed")
            );
            for (owner, plan) in [
                (
                    "incoming-owner",
                    json!({"mode": "normal_sync", "reason": "same_owner"}),
                ),
                (
                    "previous-owner",
                    json!({"mode": "auto", "strategy": "keep_remote", "reason": "different_owner"}),
                ),
            ] {
                input["local"]["ownerId"] = json!(owner);
                let output = call(&input);
                assert_eq!(output["classification"], classification);
                assert_eq!(output["plan"], plan);
            }
        }
    }
}

fn terminal_pair(status: &str, kind: &str) -> Value {
    let mut input = terminal_input();
    let mut history = fixture()["history"].clone();
    let timer = &mut input["local"]["workspace"]["base"]["canonicalTimer"];
    timer["status"] = json!(status);
    timer["lastIntent"]["type"] = json!(kind);
    history["status"] = json!(status);
    if status != "completed" {
        history.as_object_mut().unwrap().remove("completedAt");
        timer["elapsedAtAnchorMs"] = json!(17_000);
        history["elapsedMs"] = json!(17_000);
    }
    if status == "superseded" {
        timer["supersededByTimerId"] = json!("newer");
        history["supersededByTimerId"] = json!("newer");
        timer["lastIntent"]["commandId"] = json!("original-start");
        timer["lastIntent"]["occurredAt"] = timer["startedAt"].clone();
        history["commandId"] = json!("newer-start");
    }
    input["local"]["workspace"]["base"]["history"] = json!([history]);
    input
}
