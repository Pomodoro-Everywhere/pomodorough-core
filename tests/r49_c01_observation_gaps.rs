//! R49-C01 observation-gap hardening for the monotonic anchor/reading guards.
//!
//! Scope is preservation controls, not bug repro. All three tests pass
//! unchanged on official Core v0.48.0: malformed and misused observation
//! evidence fails identically in both builds. Error envelopes are asserted
//! exactly (only keys `["error", "ok"]`), so success cannot hide inside a
//! partial match. The live R49-C01 regression cover lives in
//! `r49_c01_monotonic_retarget` and `r49_c01_replay_boundaries`.
use pomodorough_core::dispatch_envelope_json;
use serde_json::{Value, json};

const BASE: &str = "2026-07-20T12:00:10Z";
const JUMP: &str = "2026-07-20T12:01:20Z";
const CONTINUITY: &str = "browser-session-1";

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap()
}

fn dispatch(operation: &str, input: &Value) -> Value {
    if let Ok(oracle) = std::env::var("R49_C01_BASELINE_ORACLE") {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("node")
            .arg(oracle)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(
            child.stdin.take().unwrap(),
            "{}",
            json!({"operation": operation, "input": input.to_string()})
        )
        .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        return serde_json::from_slice(&output.stdout).unwrap();
    }
    serde_json::from_str(&dispatch_envelope_json(operation, &input.to_string())).unwrap()
}

fn seed() -> Value {
    let mut input = fixture()["request"].clone();
    input["compatibility"] = json!("pwaStorage");
    input["intent"] = json!({"kind": "setAutoStart", "enabled": true});
    input["workspace"]["base"]["canonicalTimer"] = fixture()["timer"].clone();
    input["ownership"] = json!({"ownerId": "account-a", "expectedOwnerId": "account-a"});
    input["durability"] = json!({"outgoingDurationOperationIds": [], "localTabId": "tab-a"});
    for field in ["occurredAt", "physicalNow", "observedAt"] {
        input["clock"][field] = json!(BASE);
    }
    input["clock"]["monotonicNowMs"] = json!(100.0);
    input["clock"]["continuityId"] = json!(CONTINUITY);
    let output = dispatch("workspace.intent.v1", &input);
    assert_eq!(output["ok"], true);
    output["value"].clone()
}

fn live_intent(prior: &Value) -> Value {
    let mut input = fixture()["request"].clone();
    input["compatibility"] = json!("pwaStorage");
    input["intent"] = json!({"kind": "addAndSelectTask", "title": "new task"});
    for field in ["workspace", "allocation", "observation"] {
        input[field] = prior[field].clone();
    }
    input["workspace"]["base"]["canonicalTimer"] = fixture()["timer"].clone();
    input["ownership"] = json!({"ownerId": "account-a", "expectedOwnerId": "account-a"});
    input["durability"] = json!({"outgoingDurationOperationIds": [], "localTabId": "tab-a"});
    for field in ["occurredAt", "physicalNow", "observedAt"] {
        input["clock"][field] = json!(JUMP);
    }
    input["clock"]["monotonicNowMs"] = json!(1100.0);
    input["clock"]["continuityId"] = json!(CONTINUITY);
    input["identities"]["commandUuids"] = json!([
        "019f7f65-dd10-7000-8000-000000000001",
        "019f7f65-dd10-7000-8000-000000000002",
        "019f7f65-dd10-7000-8000-000000000003"
    ]);
    input
}

fn live_read(prior: &Value) -> Value {
    json!({"profile": "pwaStorage", "source": {"kind": "workspace", "value": prior["workspace"]},
        "selectedPhase": "focus", "observedAt": JUMP,
        "calendarIntervals": fixture()["request"]["calendarIntervals"],
        "monotonic": {"nowMs": 1100.0, "continuityId": CONTINUITY,
            "anchor": prior["observation"]["monotonicAnchor"]}})
}

fn assert_error_only(output: &Value, label: &str) {
    assert_eq!(output["ok"], false, "{label}: {output}");
    assert_eq!(
        output.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["error", "ok"],
        "{label}"
    );
}

#[test]
fn malformed_anchor_fields_fail_closed_on_intent_and_read_model() {
    let prior = seed();
    let anchor = "/observation/monotonicAnchor";
    let read_anchor = "/monotonic/anchor";
    let cases = [
        ("empty-timer", "timerId", json!("")),
        ("bad-anchor-time", "anchorAt", json!("not-a-time")),
        ("negative-elapsed", "elapsedAtAnchorMs", json!(-1)),
        (
            "unsafe-trusted",
            "sampledTrustedNowMs",
            json!(9007199254740992_i64),
        ),
        ("empty-continuity", "continuityId", json!("")),
    ];
    for (label, field, value) in cases {
        let mut intent = live_intent(&prior);
        let mut read = live_read(&prior);
        *intent.pointer_mut(&format!("{anchor}/{field}")).unwrap() = value.clone();
        *read.pointer_mut(&format!("{read_anchor}/{field}")).unwrap() = value;
        assert_error_only(
            &dispatch("workspace.intent.v1", &intent),
            &format!("intent/{label}"),
        );
        assert_error_only(
            &dispatch("workspace.readModel.v1", &read),
            &format!("read/{label}"),
        );
    }
}

#[test]
fn out_of_range_readings_fail_closed_on_intent_and_read_model() {
    let prior = seed();
    let cases = [
        (
            "negative-now",
            "/clock/monotonicNowMs",
            json!(-1.0),
            "/monotonic/nowMs",
            json!(-1.0),
        ),
        (
            "unsafe-now",
            "/clock/monotonicNowMs",
            json!(9007199254740992.0),
            "/monotonic/nowMs",
            json!(9007199254740992.0),
        ),
        (
            "empty-reading-continuity",
            "/clock/continuityId",
            json!(""),
            "/monotonic/continuityId",
            json!(""),
        ),
    ];
    for (label, intent_path, intent_value, read_path, read_value) in cases {
        let mut intent = live_intent(&prior);
        let mut read = live_read(&prior);
        *intent.pointer_mut(intent_path).unwrap() = intent_value;
        *read.pointer_mut(read_path).unwrap() = read_value;
        assert_error_only(
            &dispatch("workspace.intent.v1", &intent),
            &format!("intent/{label}"),
        );
        assert_error_only(
            &dispatch("workspace.readModel.v1", &read),
            &format!("read/{label}"),
        );
    }
}

#[test]
fn private_evidence_and_profile_misuse_fail_closed() {
    let prior = seed();
    let mut intent = live_intent(&prior);
    intent["observation"]["runningTimer"] = json!({"timerId": "existing-timer"});
    assert_error_only(
        &dispatch("workspace.intent.v1", &intent),
        "intent/private-injection",
    );
    let mut read = live_read(&prior);
    read["monotonic"]["runningTimer"] = json!({"timerId": "existing-timer"});
    assert_error_only(
        &dispatch("workspace.readModel.v1", &read),
        "read/private-injection",
    );
    let mut intent = live_intent(&prior);
    intent["clock"]
        .as_object_mut()
        .unwrap()
        .remove("continuityId");
    assert_error_only(
        &dispatch("workspace.intent.v1", &intent),
        "intent/missing-continuity",
    );
    let mut intent = live_intent(&prior);
    intent["compatibility"] = json!("desktopStorage");
    assert_error_only(
        &dispatch("workspace.intent.v1", &intent),
        "intent/non-pwa-profile",
    );
    let mut read = live_read(&prior);
    read["profile"] = json!("desktopStorage");
    assert_error_only(
        &dispatch("workspace.readModel.v1", &read),
        "read/non-pwa-profile",
    );
}
