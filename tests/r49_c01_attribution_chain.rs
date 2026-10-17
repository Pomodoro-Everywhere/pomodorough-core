use serde_json::{Value, json};

#[path = "r49_c01_attribution_chain/persisted.rs"]
mod persisted;
use persisted::dispatch;

const TASK: &str = "111db2c9-a2cf-8af7-beb4-cbd9fd252b6b";
const CONTINUITY: &str = "browser-session-1";

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap()
}

fn intent(prior: Option<&Value>, kind: Value, wall: &str, mono: f64) -> Value {
    let mut input = fixture()["request"].clone();
    input["compatibility"] = json!("pwaStorage");
    input["intent"] = kind;
    input["workspace"]["base"]["canonicalTimer"] = fixture()["timer"].clone();
    if let Some(prior) = prior {
        let prior = persisted::reopen(prior);
        for field in ["workspace", "allocation", "observation", "selection"] {
            input[field] = prior[field].clone();
        }
    }
    input["clock"] = json!({"occurredAt": wall, "physicalNow": wall, "observedAt": wall,
        "monotonicNowMs": mono, "continuityId": CONTINUITY});
    allocate_identities(&mut input, wall);
    let output = dispatch("workspace.intent.v1", &input);
    assert_retained(&input, &output);
    output
}

fn allocate_identities(input: &mut Value, wall: &str) {
    let millis = chrono::DateTime::parse_from_rfc3339(wall)
        .unwrap()
        .timestamp_millis();
    let prefix = format!("{millis:012x}");
    let mutation = matches!(
        input["intent"]["kind"].as_str(),
        Some("setAutoStart" | "addAndSelectTask")
    );
    input["identities"]["commandUuids"] = json!(
        (1..=if mutation { 3 } else { 2 })
            .map(|n| format!("{}-{}-7000-8000-{n:012x}", &prefix[..8], &prefix[8..]))
            .collect::<Vec<_>>()
    );
    if mutation {
        input["ownership"] = json!({"ownerId": "account-a", "expectedOwnerId": "account-a"});
        input["durability"] = json!({"outgoingDurationOperationIds": [], "localTabId": "tab-a"});
    }
}

fn assert_retained(input: &Value, output: &Value) {
    for field in ["base", "canonicalHead", "timerDependencies"] {
        assert_eq!(
            output["workspace"][field], input["workspace"][field],
            "{field}"
        );
    }
    assert_eq!(output["selection"], input["selection"]);
    for (domain, queue) in input["workspace"]["local"].as_object().unwrap() {
        let returned = output["workspace"]["local"][domain].as_array().unwrap();
        let old = queue.as_array().unwrap();
        assert_eq!(&returned[..old.len()], old);
        let appended = &returned[old.len()..];
        if domain == "commands" {
            assert_eq!(appended, output["commands"].as_array().unwrap());
        } else if !appended.is_empty() {
            assert_eq!(
                appended,
                output["durableOperations"][domain].as_array().unwrap()
            );
        }
        let mut proof = input["workspace"]["neverSent"][domain]
            .as_array()
            .cloned()
            .unwrap_or_default();
        proof.extend(appended.iter().map(|row| row["id"].clone()));
        assert_eq!(
            output["workspace"]["neverSent"][domain]
                .as_array()
                .cloned()
                .unwrap_or_default(),
            proof,
            "delivery proof: {domain}"
        );
    }
}

fn selected(wall: &str, mono: f64) -> Value {
    let seed = intent(
        None,
        json!({"kind": "setAutoStart", "enabled": true}),
        "2026-07-20T12:00:10Z",
        100.0,
    );
    let output = intent(
        Some(&seed),
        json!({"kind": "addAndSelectTask", "title": "new task"}),
        wall,
        mono,
    );
    assert_eq!(output["projection"]["canonicalTimer"]["taskId"], TASK);
    assert_eq!(output["timerObservation"]["remainingMs"], 45100.0 - mono);
    output
}

fn read(prior: &Value, mono: f64) -> Value {
    dispatch("workspace.readModel.v1", &read_input(prior, mono))
}

fn read_input(prior: &Value, mono: f64) -> Value {
    let prior = persisted::reopen(prior);
    let mut input = json!({"profile": "pwaStorage", "source": {
        "kind": "workspace", "value": prior["workspace"]}, "selectedPhase": "focus",
        "observedAt": "2026-07-20T12:02:25Z",
        "calendarIntervals": fixture()["request"]["calendarIntervals"]});
    if let Some(anchor) = prior["observation"].get("monotonicAnchor") {
        input["monotonic"] = json!({"nowMs": mono, "continuityId": CONTINUITY, "anchor": anchor});
    }
    input
}

fn assert_credit(prior: &Value, mono: f64, count: u64) {
    let first = read(prior, mono);
    assert_eq!(first, read(prior, mono), "complete repeated read envelope");
    assert_eq!(first["cadence"]["completedFocusTotal"], count);
    assert_eq!(
        first["tasks"]["completedFocusTodayByTask"][TASK],
        json!({"count": count, "plannedDurationMs": 60000 * count})
    );
}

fn assert_task(output: &Value) {
    assert_eq!(output["projection"]["canonicalTimer"]["taskId"], TASK);
}

#[test]
fn pause_cancel_returns_keep_attribution_through_next_mutation_resume_and_expiry() {
    for wall in ["2026-07-20T12:00:11Z", "2026-07-20T12:01:20Z"] {
        let chosen = selected(wall, 1100.0);
        for kind in ["pause", "cancel"] {
            let lifecycle = intent(
                Some(&chosen),
                json!({"kind": kind}),
                "2026-07-20T12:01:21Z",
                2100.0,
            );
            assert_task(&lifecycle);
            let next = intent(
                Some(&lifecycle),
                json!({"kind": "setAutoStart", "enabled": false}),
                "2026-07-20T12:01:22Z",
                3100.0,
            );
            assert_task(&next);
            assert_eq!(
                next["projection"]["canonicalTimer"],
                lifecycle["projection"]["canonicalTimer"]
            );
            if kind == "pause" {
                assert_resumed_credit(&next);
            } else {
                assert_credit(&next, 47100.0, 0);
            }
        }
    }
}

fn assert_resumed_credit(paused: &Value) {
    let resumed = intent(
        Some(paused),
        json!({"kind": "resume"}),
        "2026-07-20T12:01:23Z",
        4100.0,
    );
    assert_task(&resumed);
    assert_eq!(read(&resumed, 5100.0)["canonical"]["elapsedMs"], 18000.0);
    assert_credit(&resumed, 47099.5, 0);
    assert_credit(&resumed, 47100.0, 1);
    assert_credit(&resumed, 47100.5, 1);
}

#[test]
fn fractional_selection_credits_task_at_exact_and_after_monotonic_deadline() {
    for wall in ["2026-07-20T12:00:11Z", "2026-07-20T12:01:20Z"] {
        let chosen = selected(wall, 45099.5);
        assert_eq!(chosen["commands"][0]["observedElapsedMs"], 60000);
        assert_credit(&chosen, 45099.5, 0);
        assert_eq!(read(&chosen, 45099.5)["canonical"]["remainingMs"], 0.5);
        assert_credit(&chosen, 45100.0, 1);
        assert_credit(&chosen, 45100.5, 1);
    }
}

#[test]
fn resumed_lineage_requires_exact_live_evidence() {
    let chosen = selected("2026-07-20T12:01:20Z", 1100.0);
    let paused = intent(
        Some(&chosen),
        json!({"kind": "pause"}),
        "2026-07-20T12:01:21Z",
        2100.0,
    );
    let resumed = intent(
        Some(&paused),
        json!({"kind": "resume"}),
        "2026-07-20T12:01:23Z",
        4100.0,
    );
    let mut wall = resumed.clone();
    wall["observation"]
        .as_object_mut()
        .unwrap()
        .remove("monotonicAnchor");
    let expected = read(&wall, 47100.0);
    for (field, value) in [
        ("timerId", json!("other-timer")),
        ("anchorAt", json!("2026-07-20T12:01:22Z")),
        ("elapsedAtAnchorMs", json!(16999)),
        ("continuityId", json!("other-session")),
        ("sampledMonotonicMs", json!(47101.0)),
    ] {
        let mut invalid = resumed.clone();
        invalid["observation"]["monotonicAnchor"][field] = value;
        assert_eq!(
            read(&invalid, 47100.0),
            expected,
            "complete fallback: {field}"
        );
    }
}

#[test]
fn repeated_pause_resume_reopens_raw_state_and_keeps_fractional_deadlines() {
    for wall in ["2026-07-20T12:00:11Z", "2026-07-20T12:01:20Z"] {
        let mut current = selected(wall, 1100.0);
        for (pause, setting, resume, mono, deadline, elapsed) in [
            ("12:01:21", "12:01:22", "12:01:23", 2100.0, 47100.5, 17000),
            ("12:01:24", "12:01:25", "12:01:26", 5100.5, 49100.5, 18000),
            ("12:01:27", "12:01:28", "12:01:29", 8100.5, 51100.5, 19000),
        ] {
            current = intent(
                Some(&current),
                json!({"kind": "pause"}),
                &format!("2026-07-20T{pause}Z"),
                mono,
            );
            assert_task(&current);
            let paused_timer = current["projection"]["canonicalTimer"].clone();
            current = intent(
                Some(&current),
                json!({"kind": "setAutoStart", "enabled": false}),
                &format!("2026-07-20T{setting}Z"),
                mono + 1000.0,
            );
            assert_eq!(current["projection"]["canonicalTimer"], paused_timer);
            let resume_mono = if elapsed == 17000 {
                4100.5
            } else {
                mono + 2000.0
            };
            current = intent(
                Some(&current),
                json!({"kind": "resume"}),
                &format!("2026-07-20T{resume}Z"),
                resume_mono,
            );
            assert_task(&current);
            assert_resume_sample(&current, resume, resume_mono, elapsed);
            assert_resumed_boundaries(&current, deadline);
        }
    }
}

fn assert_resumed_boundaries(current: &Value, deadline: f64) {
    assert_deadline_samples(
        current,
        deadline,
        &[
            deadline - 0.5,
            deadline - 0.001,
            deadline,
            deadline + 0.001,
            deadline + 0.5,
        ],
    );
    let expected = read(current, deadline);
    let mut reordered = persisted::reopen(current);
    reordered["workspace"]["local"]["commands"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(
        read(&reordered, deadline),
        expected,
        "complete reordered lifecycle return"
    );
}

fn assert_resume_sample(output: &Value, wall: &str, mono: f64, elapsed: u64) {
    let wall = format!("2026-07-20T{wall}Z");
    assert_eq!(
        output["observation"]["monotonicAnchor"],
        json!({
        "timerId": "existing-timer", "anchorAt": wall,
        "elapsedAtAnchorMs": elapsed, "sampledMonotonicMs": mono,
        "sampledTrustedNowMs": chrono::DateTime::parse_from_rfc3339(&wall)
            .unwrap().timestamp_millis(), "continuityId": CONTINUITY})
    );
}

fn assert_deadline_samples(prior: &Value, deadline: f64, samples: &[f64]) {
    for &sample in samples {
        let output = read(prior, sample);
        let expired = sample >= deadline;
        assert_eq!(
            output["canonical"]["status"],
            if expired { "completed" } else { "running" }
        );
        let remaining = output["canonical"]["remainingMs"].as_f64().unwrap();
        // Equivalent binary64 elapsed expressions can differ below 1e-8ms.
        // Status and credit must still cross at the exact representable sample.
        assert!((remaining - (deadline - sample).max(0.0)).abs() < 1e-8);
        assert_eq!(remaining > 0.0, !expired);
        assert_eq!(output["cadence"]["completedFocusTotal"], u64::from(expired));
        assert_eq!(
            output["tasks"]["completedFocusTodayByTask"][TASK],
            json!({"count": u64::from(expired), "plannedDurationMs": 60000 * u64::from(expired)})
        );
    }
}

#[test]
fn fractional_sample_origin_and_selection_boundary_do_not_round_task_eligibility() {
    for origin in [100.25, 100.5, 100.75] {
        let seed = intent(
            None,
            json!({"kind": "setAutoStart", "enabled": true}),
            "2026-07-20T12:00:10Z",
            origin,
        );
        let deadline: f64 = origin + 45000.0;
        for sample in [
            deadline - 0.5,
            deadline - 0.001,
            deadline.next_down(),
            deadline,
            deadline.next_up(),
        ] {
            let chosen = intent(
                Some(&seed),
                json!({"kind": "addAndSelectTask", "title": "new task"}),
                "2026-07-20T12:01:20Z",
                sample,
            );
            if sample < deadline {
                assert_task(&chosen);
                assert_eq!(chosen["commands"][0]["observedElapsedMs"], 60000);
                assert_deadline_samples(
                    &chosen,
                    deadline,
                    &[
                        deadline - 0.5,
                        deadline - 0.001,
                        deadline.next_down(),
                        deadline,
                        deadline.next_up(),
                        deadline + 0.5,
                    ],
                );
            } else {
                assert_late_selection(&chosen, sample);
            }
        }
    }
}

fn assert_late_selection(chosen: &Value, sample: f64) {
    assert_eq!(chosen["commands"], json!([]));
    assert_eq!(
        chosen["projection"]["canonicalTimer"]["taskId"],
        Value::Null
    );
    let output = read(chosen, sample);
    assert_eq!(output["cadence"]["completedFocusTotal"], 1);
    assert_eq!(
        output["tasks"]["completedFocusTodayByTask"][TASK],
        json!({"count": 0, "plannedDurationMs": 0})
    );
}
