//! R54-C02: four-digit local years must not normalize, complete, or derive
//! deadlines outside the UTC years that the next RFC 3339 read accepts.

use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const REOPENS: usize = 4;
const TIMER_OUT_OF_RANGE: &str = "invalid shared-core input: timer timestamp out of range";
const LAST_INSTANT: &str = "9999-12-31T23:59:59.999999999Z";
const LAST_LEAP: &str = "9999-12-31T23:59:60.999999999Z";
const LAST_DAY: &str = r#"[{"start": "9999-12-31T00:00:00Z", "end": "9999-12-31T23:59:59.999Z"}]"#;

fn call(operation: &str, input: &Value) -> Result<Value, String> {
    dispatch_json(operation, &input.to_string())
        .map(|raw| serde_json::from_str(&raw).unwrap())
        .map_err(|error| error.to_string())
}

fn timer(anchor: &str, status: &str) -> Value {
    json!({"id": "year", "phase": "focus", "status": status,
        "plannedDurationMs": 60000, "elapsedAtAnchorMs": 0, "anchorAt": anchor})
}

fn completed_pair(stamp: &str) -> (Value, Value) {
    let mut timer = timer(stamp, "completed");
    timer["elapsedAtAnchorMs"] = json!(60000);
    timer["lastIntent"] = json!({"type": "finish", "commandId": "finish", "occurredAt": stamp});
    let history = json!({"id": "year", "timerId": "year", "commandId": "finish",
        "phase": "focus", "status": "completed", "plannedDurationMs": 60000, "completedAt": stamp});
    (timer, history)
}

fn command(kind: &str, at: &str) -> Value {
    json!({"id": kind, "deviceId": "device-a", "deviceSequence": 1, "timerId": "year",
        "type": kind, "phase": "focus", "plannedDurationMs": 60000, "occurredAt": at,
        "hlcWallMs": 1, "hlcCounter": 0, "observedElapsedMs": 0})
}

fn reduce(input: &Value) -> Result<Value, String> {
    call("timer.reduce.v1", input)
}

fn workspace(timer: Value, history: Value, now: &str) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-projection-v1.json")).unwrap();
    let mut input = fixture["request"].clone();
    for queue in input["local"].as_object_mut().unwrap().values_mut() {
        *queue = json!([]);
    }
    input["timerDependencies"] = json!([]);
    input["neverSent"] = json!({});
    input["now"] = json!(now);
    input["base"]["canonicalTimer"] = timer;
    input["base"]["history"] = history;
    input
}

fn project(input: &Value) -> Result<Value, String> {
    call("workspace.project.v1", input).map(|output| output["workspace"].clone())
}

/// Reopens one legacy output field alone, because the legacy reducer rejects
/// a timer that overlaps history.
fn reopen_legacy(output: &Value, key: &str, now: &str, label: &str) {
    for round in 0..REOPENS {
        let again = reduce(&json!({"commands": [], "now": now, key: output[key]})).unwrap();
        assert_eq!(again[key], output[key], "{label} {key} reopen {round}");
    }
}

fn reopen_workspace(mut input: Value, first: &Value, label: &str) {
    for round in 0..REOPENS {
        for key in ["canonicalTimer", "history"] {
            input["base"][key] = first[key].clone();
        }
        assert_eq!(project(&input).unwrap(), *first, "{label} reopen {round}");
    }
}

#[test]
fn legacy_reducer_rejects_offsets_that_normalize_outside_four_digit_years() {
    let (_, completed_history) = completed_pair("9999-12-31T23:30:00-01:00");
    for (label, input, error) in [
        (
            "review now overflow",
            json!({"commands": [], "now": "9999-12-31T23:59:59-00:01",
                "canonicalTimer": timer("9999-12-31T23:59:30Z", "running")}),
            "invalid RFC 3339 timestamp: 9999-12-31T23:59:59-00:01",
        ),
        (
            "now underflow",
            json!({"commands": [], "now": "0000-01-01T00:30:00+01:00",
                "canonicalTimer": timer("0000-01-01T00:00:00Z", "paused")}),
            "invalid RFC 3339 timestamp: 0000-01-01T00:30:00+01:00",
        ),
        (
            "command underflow",
            json!({"commands": [command("start", "0000-01-01T00:30:00+01:00")],
                "now": "0000-01-01T01:00:00+01:00"}),
            "invalid RFC 3339 timestamp: 0000-01-01T00:30:00+01:00",
        ),
        (
            "command overflow",
            json!({"commands": [command("start", "9999-12-31T23:30:00-01:00")],
                "now": "9999-12-31T23:59:59Z"}),
            "invalid RFC 3339 timestamp: 9999-12-31T23:30:00-01:00",
        ),
        (
            "anchor overflow",
            json!({"commands": [], "now": "9999-12-31T23:59:59Z",
                "canonicalTimer": timer("9999-12-31T23:30:00-01:00", "paused")}),
            "invalid shared-core input: invalid canonical timer",
        ),
        (
            "history overflow",
            json!({"commands": [], "now": "9999-12-31T23:59:59Z",
                "history": [completed_history]}),
            "invalid shared-core input: invalid timer history",
        ),
    ] {
        assert_eq!(reduce(&input).unwrap_err(), error, "{label}");
    }
}

#[test]
fn legacy_reducer_rejects_each_input_one_nanosecond_past_the_year_bounds() {
    // Each offset keeps the local year in range while the UTC instant is one
    // nanosecond outside it.
    for (stamp, invalid_stamp) in [
        (
            "0000-01-01T00:59:59.999999999+01:00",
            "invalid RFC 3339 timestamp: 0000-01-01T00:59:59.999999999+01:00",
        ),
        (
            "9999-12-31T23:00:00-01:00",
            "invalid RFC 3339 timestamp: 9999-12-31T23:00:00-01:00",
        ),
    ] {
        let (_, history) = completed_pair(stamp);
        for (label, input, error) in [
            ("now", json!({"commands": [], "now": stamp}), invalid_stamp),
            (
                "paused anchor",
                json!({"commands": [], "now": LAST_INSTANT,
                    "canonicalTimer": timer(stamp, "paused")}),
                "invalid shared-core input: invalid canonical timer",
            ),
            (
                "command",
                json!({"commands": [command("start", stamp)], "now": LAST_INSTANT}),
                invalid_stamp,
            ),
            (
                "history",
                json!({"commands": [], "now": LAST_INSTANT, "history": [history]}),
                "invalid shared-core input: invalid timer history",
            ),
        ] {
            assert_eq!(reduce(&input).unwrap_err(), error, "{stamp} {label}");
        }
    }
}

#[test]
fn legacy_natural_completion_past_9999_is_rejected_before_emission() {
    // Chrono counts the leap second as elapsed, so a one-minute timer
    // anchored at 23:59:00 completes at 10000-01-01T00:00:00Z.
    for (label, input) in [
        (
            "canonical running timer",
            json!({"commands": [], "now": "9999-12-31T23:59:60.5Z",
                "canonicalTimer": timer("9999-12-31T23:59:00Z", "running")}),
        ),
        (
            "command-started timer",
            json!({"commands": [command("start", "9999-12-31T23:59:00Z")],
                "now": "9999-12-31T23:59:60.5Z"}),
        ),
    ] {
        assert_eq!(reduce(&input).unwrap_err(), TIMER_OUT_OF_RANGE, "{label}");
    }
}

/// A later start for another timer, so the reducer completes or supersedes
/// the earlier session at this start's time.
fn superseding_start(at: &str) -> Value {
    let mut start = command("start", at);
    start["id"] = json!("second");
    start["timerId"] = json!("next");
    start["deviceSequence"] = json!(2);
    start["hlcWallMs"] = json!(2);
    start
}

#[test]
fn legacy_superseding_start_bounds_the_session_it_completes() {
    // The leap-second start completes the earlier one-minute timer, which is
    // then no longer current, so the bound must cover every session.
    let overflow = reduce(&json!({"now": LAST_LEAP, "commands": [
        command("start", "9999-12-31T23:59:00Z"), superseding_start("9999-12-31T23:59:60.5Z")]}));
    assert_eq!(overflow.unwrap_err(), TIMER_OUT_OF_RANGE);

    let exact = reduce(&json!({"now": LAST_LEAP, "commands": [
        command("start", "9999-12-31T23:58:59.999999999Z"),
        superseding_start("9999-12-31T23:59:60.5Z")]}))
    .unwrap();
    assert_eq!(exact["canonicalTimer"]["id"], "next");
    assert_eq!(exact["canonicalTimer"]["status"], "running");
    assert_eq!(
        exact["canonicalTimer"]["anchorAt"],
        "9999-12-31T23:59:60.5Z"
    );
    assert_eq!(exact["history"][0]["timerId"], "year");
    assert_eq!(exact["history"][0]["completedAt"], LAST_INSTANT);
    assert_eq!(exact["sessions"][1]["timerId"], "year");
    assert_eq!(exact["sessions"][1]["status"], "completed");
    assert_eq!(exact["sessions"][1]["endedAt"], LAST_INSTANT);
    reopen_legacy(&exact, "canonicalTimer", LAST_LEAP, "superseding");
    reopen_legacy(&exact, "history", LAST_LEAP, "superseding");
}

fn replay_page(input: &Value) -> Result<Value, String> {
    call("timer.replay.page.v1", input)
}

fn first_page(start_at: &str, now: &str) -> Value {
    json!({"commands": [command("start", start_at)], "sessions": [],
        "currentTimerId": null, "after": null, "now": now})
}

#[test]
fn replay_page_natural_completion_past_9999_is_rejected_before_emission() {
    // Paged replay completes the current timer and projects directly,
    // without the reducer's finish step.
    let overflow = first_page("9999-12-31T23:59:00Z", "9999-12-31T23:59:60.5Z");
    assert_eq!(replay_page(&overflow).unwrap_err(), TIMER_OUT_OF_RANGE);

    let first = replay_page(&first_page("9999-12-31T23:58:59.999999999Z", LAST_INSTANT)).unwrap();
    assert_eq!(first["canonicalTimer"]["status"], "completed");
    assert_eq!(first["canonicalTimer"]["anchorAt"], LAST_INSTANT);
    assert_eq!(first["history"][0]["completedAt"], LAST_INSTANT);
    assert_eq!(first["sessions"][0]["endedAt"], LAST_INSTANT);
    assert_eq!(first["currentTimerId"], "year");
    assert_eq!(
        first["after"],
        json!({"hlcWallMs": 1, "hlcCounter": 0, "deviceId": "device-a", "id": "start"})
    );
    assert_eq!(
        first["outcomes"],
        json!({"start": {"outcome": "applied", "reason": ""}})
    );
    // A page reports outcomes only for its own commands.
    let mut reopened = first.clone();
    reopened["outcomes"] = json!({});
    let next_page = json!({"commands": [], "sessions": first["sessions"],
        "currentTimerId": "year", "after": first["after"], "now": LAST_INSTANT});
    for round in 0..REOPENS {
        assert_eq!(
            replay_page(&next_page).unwrap(),
            reopened,
            "replay page reopen {round}"
        );
    }
}

#[test]
fn legacy_reducer_accepts_and_reopens_exact_year_bounds() {
    let lower = reduce(&json!({"now": "0000-01-01T00:00:00Z",
        "commands": [command("start", "0000-01-01T01:00:00+01:00")]}))
    .unwrap();
    assert_eq!(lower["canonicalTimer"]["anchorAt"], "0000-01-01T00:00:00Z");
    assert_eq!(lower["sessions"][0]["startedAt"], "0000-01-01T00:00:00Z");
    reopen_legacy(&lower, "canonicalTimer", "0000-01-01T00:00:00Z", "lower");

    let upper = reduce(&json!({"commands": [], "now": LAST_INSTANT,
        "canonicalTimer": timer("9999-12-31T23:58:59.999999999Z", "running")}))
    .unwrap();
    assert_eq!(upper["canonicalTimer"]["status"], "completed");
    assert_eq!(upper["canonicalTimer"]["anchorAt"], LAST_INSTANT);
    assert_eq!(upper["history"][0]["completedAt"], LAST_INSTANT);
    assert_eq!(upper["sessions"][0]["endedAt"], LAST_INSTANT);
    reopen_legacy(&upper, "canonicalTimer", LAST_INSTANT, "upper");
    reopen_legacy(&upper, "history", LAST_INSTANT, "upper");

    let mut running = timer("9999-12-31T23:59:00Z", "running");
    running["plannedDurationMs"] = json!(120000);
    let leap = reduce(
        &json!({"commands": [command("pause", LAST_LEAP)], "now": LAST_LEAP,
        "canonicalTimer": running}),
    )
    .unwrap();
    assert_eq!(leap["canonicalTimer"]["status"], "paused");
    assert_eq!(leap["canonicalTimer"]["anchorAt"], LAST_LEAP);
    reopen_legacy(&leap, "canonicalTimer", LAST_LEAP, "leap");
}

#[test]
fn workspace_projection_rejects_offsets_that_normalize_outside_four_digit_years() {
    let (terminal, terminal_history) = completed_pair("9999-12-31T23:30:00-01:00");
    let mut queued = workspace(Value::Null, json!([]), "9999-12-31T23:59:59Z");
    queued["local"]["commands"] = json!([command("start", "9999-12-31T23:30:00-01:00")]);
    for (label, input, error) in [
        (
            "review now overflow",
            workspace(
                timer("9999-12-31T23:59:30Z", "running"),
                json!([]),
                "9999-12-31T23:59:59-00:01",
            ),
            "invalid RFC 3339 timestamp: 9999-12-31T23:59:59-00:01",
        ),
        (
            "paused now overflow",
            workspace(
                timer("9999-12-31T23:00:00Z", "paused"),
                json!([]),
                "9999-12-31T23:30:00-01:00",
            ),
            "invalid RFC 3339 timestamp: 9999-12-31T23:30:00-01:00",
        ),
        (
            "anchor underflow",
            workspace(
                timer("0000-01-01T00:30:00+01:00", "paused"),
                json!([]),
                "2017-01-01T00:00:00Z",
            ),
            "invalid shared-core input: invalid canonical timer",
        ),
        (
            "terminal overflow",
            workspace(terminal, json!([terminal_history]), "9999-12-31T23:59:59Z"),
            "invalid shared-core input: invalid canonical timer",
        ),
        (
            "queued command overflow",
            queued,
            "invalid RFC 3339 timestamp: 9999-12-31T23:30:00-01:00",
        ),
    ] {
        assert_eq!(project(&input).unwrap_err(), error, "{label}");
    }
}

#[test]
fn workspace_natural_completion_past_9999_is_rejected_before_emission() {
    let input = workspace(
        timer("9999-12-31T23:59:00Z", "running"),
        json!([]),
        "9999-12-31T23:59:60.5Z",
    );
    assert_eq!(project(&input).unwrap_err(), TIMER_OUT_OF_RANGE);
}

#[test]
fn workspace_projection_accepts_and_reopens_exact_year_bounds() {
    let lower = workspace(
        timer("0000-01-01T00:00:00Z", "paused"),
        json!([]),
        "0000-01-01T01:00:00+01:00",
    );
    let first = project(&lower).unwrap();
    assert_eq!(first["canonicalTimer"]["anchorAt"], "0000-01-01T00:00:00Z");
    reopen_workspace(lower, &first, "lower");

    let leap = workspace(timer(LAST_LEAP, "paused"), json!([]), LAST_LEAP);
    let first = project(&leap).unwrap();
    assert_eq!(first["canonicalTimer"]["anchorAt"], LAST_LEAP);
    reopen_workspace(leap, &first, "leap");

    let upper = workspace(
        timer("9999-12-31T23:58:59.999999999Z", "running"),
        json!([]),
        LAST_INSTANT,
    );
    let first = project(&upper).unwrap();
    assert_eq!(first["canonicalTimer"]["status"], "completed");
    assert_eq!(first["canonicalTimer"]["anchorAt"], LAST_INSTANT);
    assert_eq!(first["history"][0]["completedAt"], LAST_INSTANT);
    assert_eq!(first["history"][0]["endedAt"], LAST_INSTANT);
    reopen_workspace(upper, &first, "upper");
}

fn read_model(anchor: &str, observed: &str, monotonic: Option<Value>) -> Result<Value, String> {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/read-model-v1.json")).unwrap();
    let mut input = fixture["request"].clone();
    input["source"]["value"]["base"]["canonicalTimer"] = timer(anchor, "running");
    input["observedAt"] = json!(observed);
    input["calendarIntervals"] = serde_json::from_str(LAST_DAY).unwrap();
    if let Some(monotonic) = monotonic {
        input["profile"] = json!("pwaStorage");
        input["monotonic"] = monotonic;
    }
    call("workspace.readModel.v1", &input).map(|output| output["canonical"].clone())
}

fn monotonic(anchor: &str, sampled_trusted_ms: i64, now_ms: f64) -> Option<Value> {
    Some(json!({"nowMs": now_ms, "continuityId": "session",
        "anchor": {"timerId": "year", "anchorAt": anchor, "elapsedAtAnchorMs": 0,
            "sampledTrustedNowMs": sampled_trusted_ms, "sampledMonotonicMs": 100.0,
            "continuityId": "session"}}))
}

#[test]
fn read_model_deadline_stays_inside_four_digit_years() {
    // 23:59:30 + 60 s is 10000-01-01T00:00:30Z.
    assert_eq!(
        read_model("9999-12-31T23:59:30Z", "9999-12-31T23:59:40Z", None).unwrap_err(),
        "invalid shared-core input: read model deadline out of range"
    );
    // 23:59:00 + 60 s is exactly 10000-01-01T00:00:00Z.
    assert_eq!(
        read_model("9999-12-31T23:59:00Z", "9999-12-31T23:59:00Z", None).unwrap_err(),
        "invalid shared-core input: read model deadline out of range"
    );
    let last = read_model("9999-12-31T23:58:59.999Z", "9999-12-31T23:59:30Z", None).unwrap();
    assert_eq!(last["status"], "running");
    assert_eq!(last["deadlineAt"], "9999-12-31T23:59:59.999Z");
}

#[test]
fn read_model_monotonic_replay_time_stays_inside_four_digit_years() {
    // Trusted sample 23:59:40 is 10 s after the anchor; 35 s more monotonic
    // time replays at 10000-01-01T00:00:15Z.
    let past = monotonic("9999-12-31T23:59:30Z", 253_402_300_780_000, 35_100.0);
    assert_eq!(
        read_model("9999-12-31T23:59:30Z", "9999-12-31T23:59:40Z", past).unwrap_err(),
        "invalid shared-core input: read model replay time out of range"
    );
    // 20 s more monotonic time replays at exactly 10000-01-01T00:00:00Z; one
    // millisecond less replays in range and fails only on the deadline.
    for (now_ms, error) in [
        (
            20_100.0,
            "invalid shared-core input: read model replay time out of range",
        ),
        (
            20_099.0,
            "invalid shared-core input: read model deadline out of range",
        ),
    ] {
        let sample = monotonic("9999-12-31T23:59:30Z", 253_402_300_780_000, now_ms);
        let replay = read_model("9999-12-31T23:59:30Z", "9999-12-31T23:59:40Z", sample);
        assert_eq!(replay.unwrap_err(), error, "nowMs {now_ms}");
    }
    // Trusted sample 23:59:30.000 is 30.001 s after the anchor; 29.999 s more
    // replays and completes exactly at 23:59:59.999.
    let last = monotonic("9999-12-31T23:58:59.999Z", 253_402_300_770_000, 30_099.0);
    let view = read_model("9999-12-31T23:58:59.999Z", "9999-12-31T23:59:30Z", last).unwrap();
    assert_eq!(view["status"], "completed");
    assert_eq!(view["elapsedMs"], 60_000.0);
}

fn pwa_observation(anchor: &str, now: &str, wall_ms: i64) -> Result<Value, String> {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let mut input = fixture["request"].clone();
    input["compatibility"] = json!("pwaStorage");
    input["intent"] = json!({"kind": "selectPhase", "phase": "focus"});
    input["workspace"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    input["workspace"]["base"]["canonicalTimer"]["anchorAt"] = json!(anchor);
    input["workspace"]["canonicalHead"] = json!({"wallMs": wall_ms, "counter": 0});
    input["allocation"]["hlc"] = json!({"wallMs": wall_ms, "counter": 0});
    for field in ["occurredAt", "physicalNow", "observedAt"] {
        input["clock"][field] = json!(now);
    }
    input["clock"]["monotonicNowMs"] = json!(100.0);
    input["clock"]["continuityId"] = json!("browser-session-1");
    input["calendarIntervals"] = serde_json::from_str(LAST_DAY).unwrap();
    call("workspace.intent.v1", &input).map(|output| output["timerObservation"].clone())
}

#[test]
fn pwa_timer_observation_deadline_stays_inside_four_digit_years() {
    // The fixture timer has 55 s left: 23:59:30 + 55 s is 10000-01-01T00:00:25Z.
    assert_eq!(
        pwa_observation(
            "9999-12-31T23:59:30Z",
            "9999-12-31T23:59:40Z",
            253_402_300_780_000
        )
        .unwrap_err(),
        "invalid shared-core input: monotonic timer observation out of range"
    );
    // 23:59:05 + 55 s is exactly 10000-01-01T00:00:00Z.
    assert_eq!(
        pwa_observation(
            "9999-12-31T23:59:05Z",
            "9999-12-31T23:59:15Z",
            253_402_300_755_000
        )
        .unwrap_err(),
        "invalid shared-core input: monotonic timer observation out of range"
    );
    let last = pwa_observation(
        "9999-12-31T23:59:04.999Z",
        "9999-12-31T23:59:14.999Z",
        253_402_300_754_999,
    )
    .unwrap();
    assert_eq!(
        last,
        json!({"timerId": "existing-timer", "elapsedMs": 15000, "remainingMs": 45000,
            "deadlineAt": "9999-12-31T23:59:59.999Z"})
    );
}

/// Apple adapter request whose retained start was physically observed 30
/// minutes after its canonical time, so the overlay shifts the paused
/// anchor by 30 minutes.
fn apple_overlay(replication_mode: &str, anchor: &str) -> Result<Value, String> {
    const INTENT_AT: &str = "9999-12-31T23:00:00Z";
    const PHYSICAL_AT: &str = "9999-12-31T23:30:00Z";
    const INTENT_WALL_MS: i64 = 253_402_297_200_000;
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let mut input = fixture["request"].clone();
    input["replicationMode"] = json!(replication_mode);
    input["intent"] = json!({"kind": "selectPhase", "phase": "focus"});
    let mut timer = fixture["timer"].clone();
    timer["status"] = json!("paused");
    timer["anchorAt"] = json!(anchor);
    timer["lastIntent"] =
        json!({"type": "start", "commandId": "retained-start", "occurredAt": INTENT_AT});
    input["workspace"]["base"]["canonicalTimer"] = timer;
    input["workspace"]["local"]["commands"] = json!([{"id": "retained-start",
        "deviceId": "device-local", "deviceSequence": 6, "timerId": "existing-timer",
        "type": "start", "phase": "focus", "plannedDurationMs": 60000, "occurredAt": INTENT_AT,
        "hlcWallMs": INTENT_WALL_MS, "hlcCounter": 0, "observedElapsedMs": 0}]);
    input["workspace"]["canonicalHead"] = json!({"wallMs": INTENT_WALL_MS, "counter": 0});
    input["allocation"]["hlc"] = json!({"wallMs": INTENT_WALL_MS, "counter": 0});
    input["observation"] =
        json!({"canonicalAnchorAt": null, "commandTimes": {"retained-start": PHYSICAL_AT}});
    for field in ["occurredAt", "physicalNow", "observedAt"] {
        input["clock"][field] = json!(PHYSICAL_AT);
    }
    input["calendarIntervals"] = serde_json::from_str(LAST_DAY).unwrap();
    call("workspace.intent.v1", &input).map(|output| output["projection"]["canonicalTimer"].clone())
}

#[test]
fn apple_observation_overlay_anchor_stays_inside_four_digit_years() {
    // 23:30:00 shifts to exactly 10000-01-01T00:00:00Z. Centralized Apple
    // replays at the shifted anchor, so parsing that replay time rejects it.
    // Iroh replays at physical now, so only the session bound rejects it.
    for (replication_mode, error) in [
        (
            "centralized",
            "invalid RFC 3339 timestamp: +10000-01-01T00:00:00Z",
        ),
        ("iroh", TIMER_OUT_OF_RANGE),
    ] {
        let overflow = apple_overlay(replication_mode, "9999-12-31T23:30:00Z");
        assert_eq!(overflow.unwrap_err(), error, "{replication_mode}");
        let last = apple_overlay(replication_mode, "9999-12-31T23:29:59.999999999Z").unwrap();
        assert_eq!(
            last,
            json!({"id": "existing-timer", "phase": "focus", "status": "paused",
                "plannedDurationMs": 60000, "elapsedAtAnchorMs": 5000, "anchorAt": LAST_INSTANT,
                "startedByDeviceId": "device-local", "lastIntent": {"type": "start",
                    "commandId": "retained-start", "occurredAt": "9999-12-31T23:00:00Z"}}),
            "{replication_mode}"
        );
    }
}
