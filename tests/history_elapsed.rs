//! Honest history: a terminal timer's history row carries the time that
//! really elapsed, and read-model totals count that time.
use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const PLANNED_MS: i64 = 1_500_000;
const CONFLICT: &str = "invalid shared-core input: conflicting workspace terminal timer/history";

fn at(offset_ms: i64) -> String {
    let minutes = offset_ms / 60_000;
    let seconds = offset_ms % 60_000 / 1_000;
    format!("2026-07-15T10:{minutes:02}:{seconds:02}Z")
}

fn command(id: &str, timer_id: &str, kind: &str, at_ms: i64, observed_elapsed_ms: i64) -> Value {
    json!({
        "id": id, "deviceId": "device-a", "deviceSequence": at_ms / 1_000 + 1,
        "timerId": timer_id, "type": kind, "phase": "focus",
        "plannedDurationMs": PLANNED_MS, "occurredAt": at(at_ms),
        "hlcWallMs": 1_000 + at_ms, "hlcCounter": 0,
        "observedElapsedMs": observed_elapsed_ms
    })
}

fn call(operation: &str, input: &Value) -> Result<Value, String> {
    dispatch_json(operation, &input.to_string())
        .map(|output| serde_json::from_str(&output).unwrap())
        .map_err(|error| error.to_string())
}

fn reduce(commands: Vec<Value>, now_ms: i64) -> Value {
    call(
        "timer.reduce.v1",
        &json!({"commands": commands, "now": at(now_ms)}),
    )
    .unwrap()
}

fn row<'a>(result: &'a Value, timer_id: &str) -> &'a Value {
    result["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["timerId"] == timer_id)
        .unwrap_or_else(|| panic!("missing history row {timer_id}: {result}"))
}

#[test]
fn finishing_early_records_real_elapsed_in_history_and_timer() {
    let result = reduce(
        vec![
            command("start", "timer-a", "start", 0, 0),
            command("finish", "timer-a", "finish", 600_000, 0),
        ],
        700_000,
    );
    let finished = row(&result, "timer-a");
    assert_eq!(finished["status"], "completed");
    assert_eq!(finished["plannedDurationMs"], PLANNED_MS);
    assert_eq!(finished["elapsedMs"], 600_000);
    assert_eq!(finished["completedAt"], at(600_000));
    assert_eq!(result["canonicalTimer"]["status"], "completed");
    assert_eq!(result["canonicalTimer"]["elapsedAtAnchorMs"], 600_000);
    assert_eq!(result["sessions"][0]["elapsedAtAnchorMs"], 600_000);
}

#[test]
fn finish_counts_running_time_and_excludes_pauses() {
    let paused_then_finished = reduce(
        vec![
            command("start", "timer-a", "start", 0, 0),
            command("pause", "timer-a", "pause", 300_000, 300_000),
            command("resume", "timer-a", "resume", 1_200_000, 300_000),
            command("finish", "timer-a", "finish", 1_320_000, 0),
        ],
        1_400_000,
    );
    assert_eq!(row(&paused_then_finished, "timer-a")["elapsedMs"], 420_000);

    let finished_while_paused = reduce(
        vec![
            command("start", "timer-a", "start", 0, 0),
            command("pause", "timer-a", "pause", 300_000, 240_000),
            command("finish", "timer-a", "finish", 1_200_000, 0),
        ],
        1_400_000,
    );
    assert_eq!(row(&finished_while_paused, "timer-a")["elapsedMs"], 240_000);
}

#[test]
fn finish_never_trusts_its_observed_elapsed() {
    for observed in [-100, 0, 1, 900_000, PLANNED_MS, 9_007_199_254_740_991] {
        let result = reduce(
            vec![
                command("start", "timer-a", "start", 0, 0),
                command("finish", "timer-a", "finish", 600_000, observed),
            ],
            700_000,
        );
        assert_eq!(
            row(&result, "timer-a")["elapsedMs"],
            600_000,
            "observed {observed}"
        );
    }
}

#[test]
fn finish_at_or_after_the_deadline_records_the_full_plan() {
    for finish_at in [PLANNED_MS, PLANNED_MS + 1_000, PLANNED_MS + 600_000] {
        let result = reduce(
            vec![
                command("start", "timer-a", "start", 0, 0),
                command("finish", "timer-a", "finish", finish_at, 0),
            ],
            finish_at,
        );
        let finished = row(&result, "timer-a");
        assert_eq!(finished["elapsedMs"], PLANNED_MS, "finish at {finish_at}");
        assert_eq!(finished["commandId"], "finish");
    }
    let one_second_early = reduce(
        vec![
            command("start", "timer-a", "start", 0, 0),
            command("finish", "timer-a", "finish", PLANNED_MS - 1_000, 0),
        ],
        PLANNED_MS,
    );
    assert_eq!(
        row(&one_second_early, "timer-a")["elapsedMs"],
        PLANNED_MS - 1_000
    );
}

#[test]
fn every_terminal_status_records_bounded_elapsed() {
    for (observed, expected) in [(120_000, 120_000), (-5, 0), (PLANNED_MS + 1, PLANNED_MS)] {
        let cancelled = reduce(
            vec![
                command("start", "timer-a", "start", 0, 0),
                command("cancel", "timer-a", "cancel", 200_000, observed),
            ],
            300_000,
        );
        assert_eq!(row(&cancelled, "timer-a")["status"], "cancelled");
        assert_eq!(row(&cancelled, "timer-a")["elapsedMs"], expected);
    }
    let superseded = reduce(
        vec![
            command("start-a", "timer-a", "start", 0, 0),
            command("start-b", "timer-b", "start", 300_000, 0),
        ],
        400_000,
    );
    assert_eq!(row(&superseded, "timer-a")["status"], "superseded");
    assert_eq!(row(&superseded, "timer-a")["elapsedMs"], 300_000);
    let expired = reduce(
        vec![command("start", "timer-a", "start", 0, 0)],
        PLANNED_MS + 5_000,
    );
    assert_eq!(row(&expired, "timer-a")["status"], "completed");
    assert_eq!(row(&expired, "timer-a")["elapsedMs"], PLANNED_MS);
    assert!(row(&expired, "timer-a").get("commandId").is_none());
}

#[test]
fn finish_on_an_ended_session_keeps_the_elapsed_recorded_at_its_end() {
    let result = reduce(
        vec![
            command("start", "timer-a", "start", 0, 0),
            command("cancel", "timer-a", "cancel", 200_000, 180_000),
            command("finish", "timer-a", "finish", 900_000, 0),
        ],
        1_000_000,
    );
    let finished = row(&result, "timer-a");
    assert_eq!(finished["status"], "completed");
    assert_eq!(finished["elapsedMs"], 180_000);
}

#[test]
fn finish_on_an_ended_row_without_elapsed_completes_it_with_the_legacy_value() {
    for (status, expected) in [
        ("cancelled", 0),
        ("superseded", 0),
        ("completed", PLANNED_MS),
    ] {
        let result = call(
            "timer.reduce.v1",
            &json!({"history": [legacy_row("ended", status)],
                "commands": [command("finish", "ended", "finish", 30_000, 12_345)],
                "now": at(60_000)}),
        )
        .unwrap();
        let finished = row(&result, "ended");
        assert_eq!(finished["status"], "completed", "{status}");
        assert_eq!(finished["elapsedMs"], expected, "{status}");
        assert_eq!(finished["commandId"], "finish", "{status}");
    }
}

fn legacy_row(timer_id: &str, status: &str) -> Value {
    let mut row = json!({"id": timer_id, "timerId": timer_id, "phase": "focus",
        "status": status, "plannedDurationMs": PLANNED_MS, "endedAt": at(0)});
    if status == "completed" {
        row["completedAt"] = json!(at(0));
    }
    row
}

fn replay_history(history: Value) -> Result<Value, String> {
    call(
        "timer.reduce.v1",
        &json!({"history": history, "commands": [], "now": at(60_000)}),
    )
}

#[test]
fn history_row_without_elapsed_loads_by_the_legacy_rule() {
    let result = replay_history(json!([
        legacy_row("done", "completed"),
        legacy_row("dropped", "cancelled"),
        legacy_row("replaced", "superseded"),
    ]))
    .unwrap();
    assert_eq!(row(&result, "done")["elapsedMs"], PLANNED_MS);
    assert_eq!(row(&result, "dropped")["elapsedMs"], 0);
    assert_eq!(row(&result, "replaced")["elapsedMs"], 0);
}

#[test]
fn recorded_elapsed_survives_replay_unchanged() {
    let mut early = legacy_row("early", "completed");
    early["elapsedMs"] = json!(600_000);
    let mut dropped = legacy_row("dropped", "cancelled");
    dropped["elapsedMs"] = json!(42_000);
    let first = replay_history(json!([early, dropped])).unwrap();
    assert_eq!(row(&first, "early")["elapsedMs"], 600_000);
    assert_eq!(row(&first, "dropped")["elapsedMs"], 42_000);
    let second = replay_history(first["history"].clone()).unwrap();
    assert_eq!(second["history"], first["history"]);
}

#[test]
fn history_elapsed_outside_the_plan_fails_closed() {
    for elapsed in [json!(-1), json!(PLANNED_MS + 1), json!(i64::MAX)] {
        let mut invalid = legacy_row("early", "completed");
        invalid["elapsedMs"] = elapsed.clone();
        let error = replay_history(json!([invalid])).unwrap_err();
        assert!(
            error.contains("invalid timer history"),
            "{elapsed}: {error}"
        );
    }
    for elapsed in [json!("600000"), json!(1.5), json!(true), json!({})] {
        let mut invalid = legacy_row("early", "completed");
        invalid["elapsedMs"] = elapsed.clone();
        assert!(replay_history(json!([invalid])).is_err(), "{elapsed}");
    }
    for (elapsed, expected) in [(0, 0), (PLANNED_MS, PLANNED_MS)] {
        let mut bound = legacy_row("early", "completed");
        bound["elapsedMs"] = json!(elapsed);
        let result = replay_history(json!([bound])).unwrap();
        assert_eq!(row(&result, "early")["elapsedMs"], expected);
    }
}

/// Explicit null means the same as a missing field, as it does for every
/// other optional field of the row: stores with nullable columns write it for
/// rows that predate the field.
#[test]
fn explicit_null_elapsed_loads_as_a_row_without_the_field() {
    let nulled = |timer_id: &str, status: &str| {
        let mut row = legacy_row(timer_id, status);
        row["elapsedMs"] = Value::Null;
        row
    };
    let result = replay_history(json!([
        nulled("done", "completed"),
        nulled("dropped", "cancelled"),
        nulled("replaced", "superseded"),
    ]))
    .unwrap();
    assert_eq!(row(&result, "done")["elapsedMs"], PLANNED_MS);
    assert_eq!(row(&result, "dropped")["elapsedMs"], 0);
    assert_eq!(row(&result, "replaced")["elapsedMs"], 0);

    let mut paired = workspace(json!({"elapsedAtAnchorMs": 59_999}), json!({}));
    paired["base"]["history"][0]["elapsedMs"] = Value::Null;
    let result = call("workspace.project.v1", &paired).unwrap();
    assert_eq!(result["workspace"]["history"][0]["elapsedMs"], 59_999);

    let mut unpaired = focus_today("legacy", None);
    unpaired["elapsedMs"] = Value::Null;
    let model = read_model(json!([unpaired]), Value::Null);
    assert_eq!(model["cadence"]["completedFocusTodayElapsedMs"], PLANNED_MS);
}

#[test]
fn history_elapsed_above_the_signed_range_fails_closed_at_decode() {
    for elapsed in [9_223_372_036_854_775_808_u64, u64::MAX] {
        let expected =
            format!("invalid shared-core JSON: invalid value: integer `{elapsed}`, expected i64");
        let mut invalid = legacy_row("early", "completed");
        invalid["elapsedMs"] = json!(elapsed);
        let error = replay_history(json!([invalid])).unwrap_err();
        assert!(error.starts_with(&expected), "{elapsed}: {error}");

        let mut paired = workspace(json!({}), json!({}));
        paired["base"]["history"][0]["elapsedMs"] = json!(elapsed);
        let error = call("workspace.project.v1", &paired).unwrap_err();
        assert!(error.starts_with(&expected), "{elapsed}: {error}");
    }
}

fn terminal_fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-terminal-v1.json")).unwrap()
}

/// Workspace with one terminal timer and, unless `history` is null, its row.
fn workspace(timer_overrides: Value, history_overrides: Value) -> Value {
    let fixture = terminal_fixture();
    let mut request = fixture["request"].clone();
    let mut timer = fixture["timer"].clone();
    for (name, field) in timer_overrides.as_object().unwrap() {
        timer[name] = field.clone();
    }
    request["base"]["canonicalTimer"] = timer;
    if let Some(overrides) = history_overrides.as_object() {
        let mut history = fixture["history"].clone();
        for (name, field) in overrides {
            history[name] = field.clone();
        }
        history.as_object_mut().unwrap().retain(|_, v| !v.is_null());
        request["base"]["history"] = json!([history]);
    }
    request
}

#[test]
fn workspace_accepts_a_finished_early_pair_and_keeps_its_elapsed() {
    let input = workspace(
        json!({"elapsedAtAnchorMs": 20_000}),
        json!({"elapsedMs": 20_000}),
    );
    let result = call("workspace.project.v1", &input).unwrap();
    assert_eq!(
        result["workspace"]["canonicalTimer"]["elapsedAtAnchorMs"],
        20_000
    );
    assert_eq!(result["workspace"]["history"][0]["elapsedMs"], 20_000);
    assert_eq!(
        result["workspace"]["history"][0]["plannedDurationMs"],
        60_000
    );

    let standalone = workspace(json!({"elapsedAtAnchorMs": 20_000}), Value::Null);
    let result = call("workspace.project.v1", &standalone).unwrap();
    assert_eq!(result["workspace"]["history"][0]["status"], "completed");
    assert_eq!(result["workspace"]["history"][0]["elapsedMs"], 20_000);
}

#[test]
fn workspace_row_without_elapsed_takes_the_canonical_elapsed() {
    let cancel = json!({"type": "cancel", "commandId": "cancel-existing",
        "occurredAt": "2026-07-20T12:00:00Z"});
    let input = workspace(
        json!({"status": "cancelled", "elapsedAtAnchorMs": 17_000, "lastIntent": cancel}),
        json!({"status": "cancelled", "commandId": "cancel-existing", "completedAt": null,
            "elapsedMs": null}),
    );
    let result = call("workspace.project.v1", &input).unwrap();
    assert_eq!(result["workspace"]["history"][0]["elapsedMs"], 17_000);
}

#[test]
fn workspace_rejects_elapsed_that_conflicts_with_the_terminal_timer() {
    let start = json!({"type": "start", "commandId": "start-existing",
        "occurredAt": "2026-07-20T11:59:00Z"});
    let conflicts = [
        // Row and canonical timer disagree about the same session.
        workspace(
            json!({"elapsedAtAnchorMs": 20_000}),
            json!({"elapsedMs": 20_001}),
        ),
        workspace(json!({}), json!({"elapsedMs": 59_999})),
        // Only a Finish command may complete a timer before its deadline.
        workspace(
            json!({"elapsedAtAnchorMs": 20_000, "lastIntent": start}),
            json!({"elapsedMs": 20_000, "commandId": null}),
        ),
        workspace(
            json!({"elapsedAtAnchorMs": 20_000, "lastIntent": null}),
            json!({"elapsedMs": 20_000, "commandId": null}),
        ),
        workspace(
            json!({"elapsedAtAnchorMs": 20_000, "lastIntent": start}),
            Value::Null,
        ),
    ];
    for input in conflicts {
        let error = call("workspace.project.v1", &input).unwrap_err();
        assert_eq!(error, CONFLICT, "{}", input["base"]);
    }
}

/// A strict path replays a history row without its canonical timer, so a
/// cancelled row without the field gets 0. A client must not store that row
/// next to the raw timer: the pair then disagrees, and Core fails closed.
#[test]
fn strict_replay_of_a_pre_field_cancelled_row_conflicts_with_its_raw_timer() {
    let cancel = json!({"type": "cancel", "commandId": "cancel-existing",
        "occurredAt": "2026-07-20T12:00:00Z"});
    let raw = workspace(
        json!({"status": "cancelled", "elapsedAtAnchorMs": 17_000, "lastIntent": cancel}),
        json!({"status": "cancelled", "commandId": "cancel-existing", "completedAt": null}),
    );
    let pre_field_row = raw["base"]["history"][0].clone();
    assert!(pre_field_row.get("elapsedMs").is_none());
    let paired = call("workspace.project.v1", &raw).unwrap();
    assert_eq!(paired["workspace"]["history"][0]["elapsedMs"], 17_000);

    let mut strict_base = raw["base"].clone();
    strict_base["canonicalTimer"] = Value::Null;
    let strict_outputs = [
        call(
            "timer.reduce.v1",
            &json!({"history": [pre_field_row], "commands": [], "now": raw["now"]}),
        ),
        call(
            "projection.apply.v2",
            &json!({"base": strict_base, "pending": raw["local"], "now": raw["now"]}),
        ),
    ];
    for output in strict_outputs {
        let strict_row = output.unwrap()["history"][0].clone();
        assert_eq!(strict_row["status"], "cancelled");
        assert_eq!(strict_row["elapsedMs"], 0);
        let mut stored = raw.clone();
        stored["base"]["history"] = json!([strict_row]);
        let error = call("workspace.project.v1", &stored).unwrap_err();
        assert_eq!(error, CONFLICT);
    }
}

fn study_task_id() -> String {
    call("task.identity.v1", &json!({"title": "Study"})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn read_model(history: Value, timer: Value) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/read-model-v1.json")).unwrap();
    let mut request = fixture["request"].clone();
    let base = &mut request["source"]["value"]["base"];
    base["tasks"] = json!([{"id": study_task_id(), "title": "Study"}]);
    base["history"] = history;
    base["canonicalTimer"] = timer;
    call("workspace.readModel.v1", &request).unwrap()
}

fn focus_today(id: &str, elapsed: Option<i64>) -> Value {
    let mut row = json!({"id": id, "timerId": id, "phase": "focus", "status": "completed",
        "plannedDurationMs": PLANNED_MS, "completedAt": "2026-03-08T06:00:00Z",
        "taskId": study_task_id()});
    if let Some(elapsed) = elapsed {
        row["elapsedMs"] = json!(elapsed);
    }
    row
}

#[test]
fn read_model_totals_count_real_elapsed_time() {
    let history = json!([
        focus_today("early", Some(600_000)),
        focus_today("full", Some(PLANNED_MS)),
        focus_today("legacy", None),
    ]);
    let model = read_model(history, Value::Null);
    assert_eq!(model["cadence"]["completedFocusToday"], 3);
    assert_eq!(
        model["cadence"]["completedFocusTodayElapsedMs"],
        600_000 + 2 * PLANNED_MS
    );
    let task = &model["tasks"]["completedFocusTodayByTask"][study_task_id()];
    assert_eq!(task["count"], 3);
    assert_eq!(task["elapsedMs"], 600_000 + 2 * PLANNED_MS);
    assert!(
        model["cadence"]
            .get("completedFocusTodayPlannedDurationMs")
            .is_none()
    );
    assert!(task.get("plannedDurationMs").is_none());
}

#[test]
fn read_model_reports_real_elapsed_for_a_finished_early_timer() {
    let timer = json!({"id": "early", "phase": "focus", "status": "completed",
        "plannedDurationMs": PLANNED_MS, "elapsedAtAnchorMs": 600_000,
        "anchorAt": "2026-03-08T06:00:00Z", "startedByDeviceId": "device-a",
        "lastIntent": {"type": "finish", "commandId": "finish-early",
            "occurredAt": "2026-03-08T06:00:00Z"}});
    let model = read_model(json!([]), timer);
    assert_eq!(model["canonical"]["status"], "completed");
    assert_eq!(model["canonical"]["elapsedMs"], 600_000.0);
    assert_eq!(model["canonical"]["remainingMs"], 900_000.0);
    assert_eq!(model["cadence"]["completedFocusTodayElapsedMs"], 600_000);
}
