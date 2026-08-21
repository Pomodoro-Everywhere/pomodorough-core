use pomodorough_core::dispatch_json;
use serde_json::{Map, Value, json};

fn timestamp(day: u32, hour: i64, offset_ms: i64) -> String {
    let total_ms = hour * 3_600_000 + offset_ms;
    let hours = total_ms / 3_600_000;
    let minutes = total_ms % 3_600_000 / 60_000;
    let seconds = total_ms % 60_000 / 1_000;
    let milliseconds = total_ms % 1_000;
    if milliseconds == 0 {
        format!("2026-07-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
    } else {
        let fraction = format!("{milliseconds:03}")
            .trim_end_matches('0')
            .to_owned();
        format!("2026-07-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{fraction}Z")
    }
}

#[allow(clippy::too_many_arguments)]
fn command(
    id: &str,
    device_id: &str,
    timer_id: &str,
    kind: &str,
    sequence: i64,
    wall_ms: i64,
    at_ms: i64,
    observed_elapsed_ms: i64,
) -> Value {
    json!({
        "id": id,
        "deviceId": device_id,
        "deviceSequence": sequence,
        "timerId": timer_id,
        "type": kind,
        "phase": "focus",
        "plannedDurationMs": 300_000,
        "occurredAt": timestamp(15, 10, at_ms),
        "hlcWallMs": wall_ms,
        "hlcCounter": 0,
        "observedElapsedMs": observed_elapsed_ms
    })
}

fn with_counter(mut command: Value, counter: i64) -> Value {
    command["hlcCounter"] = json!(counter);
    command
}

fn reduce(commands: Vec<Value>, now_ms: i64) -> Value {
    let input = json!({
        "commands": commands,
        "now": timestamp(15, 10, now_ms),
    });
    serde_json::from_str(&dispatch_json("timer.reduce.v1", &input.to_string()).unwrap()).unwrap()
}

fn permutations<T: Clone>(values: &[T]) -> Vec<Vec<T>> {
    if values.is_empty() {
        return vec![vec![]];
    }
    let mut result = Vec::new();
    for index in 0..values.len() {
        let mut rest = values.to_vec();
        let value = rest.remove(index);
        for mut suffix in permutations(&rest) {
            let mut permutation = vec![value.clone()];
            permutation.append(&mut suffix);
            result.push(permutation);
        }
    }
    result
}

fn session<'a>(result: &'a Value, timer_id: &str) -> Option<&'a Value> {
    result["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["timerId"] == timer_id)
}

fn assert_outcome(result: &Value, command_id: &str, outcome: &str, reason: &str) {
    assert_eq!(
        result["outcomes"][command_id],
        json!({"outcome": outcome, "reason": reason}),
        "outcome for {command_id}"
    );
}

#[test]
fn timer_reduce_v1_latest_action_wins_for_existing_timer() {
    let cases = [
        ("finish", "pause", "paused"),
        ("finish", "resume", "running"),
        ("finish", "cancel", "cancelled"),
        ("cancel", "finish", "completed"),
        ("finish", "start", "running"),
    ];

    for (earlier, latest, expected_status) in cases {
        let result = reduce(
            vec![
                command("start", "device-a", "timer-a", "start", 1, 100, 0, 0),
                command(
                    "earlier", "device-a", "timer-a", earlier, 2, 200, 1_000, 1_000,
                ),
                command(
                    "latest", "device-b", "timer-a", latest, 1, 300, 2_000, 2_000,
                ),
            ],
            3_000,
        );
        assert_outcome(&result, "latest", "applied", "");
        assert_eq!(result["canonicalTimer"]["status"], expected_status);
        assert_eq!(
            result["canonicalTimer"]["lastIntent"]["commandId"],
            "latest"
        );
    }

    let cleared = reduce(
        vec![
            command("start", "device-a", "timer-a", "start", 1, 100, 0, 0),
            command(
                "clear", "device-b", "timer-a", "clear", 1, 200, 1_000, 1_000,
            ),
        ],
        2_000,
    );
    assert_outcome(&cleared, "clear", "applied", "");
    assert!(cleared["canonicalTimer"].is_null());
}

#[test]
fn timer_reduce_v1_is_deterministic_across_server_arrival_cases() {
    let cases = [
        vec![
            command("command-a", "device-a", "timer-a", "start", 1, 100, 0, 0),
            command(
                "command-b",
                "device-a",
                "timer-a",
                "pause",
                2,
                200,
                60_000,
                60_000,
            ),
            command(
                "command-c",
                "device-b",
                "timer-b",
                "start",
                1,
                300,
                120_000,
                0,
            ),
            command(
                "command-d",
                "device-a",
                "timer-a",
                "resume",
                3,
                400,
                180_000,
                60_000,
            ),
            command(
                "command-e",
                "device-a",
                "timer-a",
                "finish",
                4,
                500,
                240_000,
                240_000,
            ),
        ],
        vec![
            command("command-a", "device-a", "timer-a", "start", 1, 100, 0, 0),
            command(
                "command-b",
                "device-b",
                "timer-a",
                "start",
                1,
                200,
                1_000,
                0,
            ),
            command(
                "command-c",
                "device-a",
                "timer-a",
                "cancel",
                2,
                300,
                2_000,
                2_000,
            ),
            command(
                "command-d",
                "device-a",
                "timer-a",
                "clear",
                3,
                400,
                3_000,
                2_000,
            ),
        ],
        vec![
            with_counter(
                command(
                    "command-z",
                    "device-a",
                    "timer-a",
                    "pause",
                    2,
                    100,
                    1_000,
                    1_000,
                ),
                1,
            ),
            command("command-a", "device-a", "timer-a", "start", 1, 100, 0, 0),
            command("command-b", "device-b", "timer-b", "start", 1, 100, 0, 0),
            with_counter(
                command(
                    "command-y",
                    "device-b",
                    "timer-b",
                    "cancel",
                    2,
                    100,
                    2_000,
                    2_000,
                ),
                1,
            ),
        ],
    ];

    for commands in cases {
        let expected = reduce(commands.clone(), 300_000);
        for arrival_order in permutations(&commands) {
            assert_eq!(reduce(arrival_order, 300_000), expected);
        }
    }
}

#[test]
fn timer_reduce_v1_returns_full_authoritative_wire_result() {
    let result = reduce(
        vec![command(
            "command-a",
            "device-a",
            "timer-a",
            "start",
            17,
            100,
            0,
            0,
        )],
        120_000,
    );

    assert_eq!(
        result,
        json!({
            "canonicalTimer": {
                "id": "timer-a",
                "phase": "focus",
                "status": "running",
                "plannedDurationMs": 300_000,
                "elapsedAtAnchorMs": 0,
                "anchorAt": "2026-07-15T10:00:00Z",
                "startedByDeviceId": "device-a",
                "lastIntent": {
                    "type": "start",
                    "commandId": "command-a",
                    "occurredAt": "2026-07-15T10:00:00Z"
                }
            },
            "history": [],
            "sessions": [{
                "timerId": "timer-a",
                "phase": "focus",
                "status": "running",
                "plannedDurationMs": 300_000,
                "elapsedAtAnchorMs": 0,
                "anchorAt": "2026-07-15T10:00:00Z",
                "startedAt": "2026-07-15T10:00:00Z",
                "startedByDeviceId": "device-a",
                "lastCommandId": "command-a",
                "lastIntent": {
                    "type": "start",
                    "commandId": "command-a",
                    "occurredAt": "2026-07-15T10:00:00Z"
                }
            }],
            "outcomes": {
                "command-a": {"outcome": "applied", "reason": ""}
            }
        })
    );
}

#[test]
fn timer_reduce_v1_uses_complete_hybrid_clock_ordering_not_device_sequence() {
    let cases = [
        (
            vec![
                command(
                    "wall-high",
                    "device-a",
                    "shared-timer",
                    "start",
                    1,
                    101,
                    0,
                    0,
                ),
                command(
                    "wall-low",
                    "device-z",
                    "shared-timer",
                    "start",
                    1,
                    100,
                    1_000,
                    0,
                ),
            ],
            "wall-high",
        ),
        (
            vec![
                with_counter(
                    command(
                        "counter-high",
                        "device-a",
                        "shared-timer",
                        "start",
                        1,
                        100,
                        0,
                        0,
                    ),
                    1,
                ),
                command(
                    "counter-low",
                    "device-a",
                    "shared-timer",
                    "start",
                    2,
                    100,
                    0,
                    0,
                ),
            ],
            "counter-high",
        ),
        (
            vec![
                command(
                    "command-a",
                    "device-b",
                    "shared-timer",
                    "start",
                    1,
                    100,
                    0,
                    0,
                ),
                command(
                    "command-z",
                    "device-a",
                    "shared-timer",
                    "start",
                    1,
                    100,
                    0,
                    0,
                ),
            ],
            "command-a",
        ),
        (
            vec![
                command(
                    "command-b",
                    "device-a",
                    "shared-timer",
                    "start",
                    1,
                    100,
                    0,
                    0,
                ),
                command(
                    "command-a",
                    "device-a",
                    "shared-timer",
                    "start",
                    99,
                    100,
                    0,
                    0,
                ),
            ],
            "command-b",
        ),
    ];

    for (commands, winner) in cases {
        let result = reduce(commands.clone(), 0);
        for input in commands {
            assert_eq!(
                result["outcomes"][input["id"].as_str().unwrap()]["outcome"],
                "applied"
            );
        }
        assert_eq!(result["canonicalTimer"]["lastIntent"]["commandId"], winner);
    }
}

#[test]
fn timer_reduce_v1_does_not_mutate_input_value() {
    let commands = vec![
        command(
            "command-b",
            "device-b",
            "timer-b",
            "start",
            1,
            200,
            1_000,
            0,
        ),
        command("command-a", "device-a", "timer-a", "start", 1, 100, 0, 0),
    ];
    let input = json!({"commands": commands, "now": timestamp(15, 10, 60_000)});
    let expected = input.clone();
    dispatch_json("timer.reduce.v1", &input.to_string()).unwrap();
    assert_eq!(input, expected);
}

#[test]
fn timer_reduce_v1_retains_starting_device_and_latest_intent() {
    let result = reduce(
        vec![
            command(
                "start-owner",
                "device-owner",
                "timer-owner",
                "start",
                1,
                100,
                0,
                0,
            ),
            command(
                "pause-peer",
                "device-peer",
                "timer-owner",
                "pause",
                1,
                200,
                60_000,
                60_000,
            ),
        ],
        120_000,
    );
    assert_eq!(
        result["canonicalTimer"]["startedByDeviceId"],
        "device-owner"
    );
    assert_eq!(
        result["canonicalTimer"]["lastIntent"]["commandId"],
        "pause-peer"
    );
}

#[test]
fn timer_reduce_v1_history_is_newest_first_at_fractional_precision() {
    let result = reduce(
        vec![
            command("start-a", "device-a", "timer-a", "start", 1, 100, 0, 0),
            command(
                "cancel-a", "device-a", "timer-a", "cancel", 2, 200, 1_000, 1_000,
            ),
            command("start-b", "device-a", "timer-b", "start", 3, 300, 1_050, 0),
            command(
                "cancel-b", "device-a", "timer-b", "cancel", 4, 400, 1_100, 50,
            ),
        ],
        2_000,
    );
    assert_eq!(result["history"][0]["timerId"], "timer-b");
    assert_eq!(result["history"][1]["timerId"], "timer-a");
    assert_eq!(
        result["history"][0],
        json!({
            "id": "timer-b",
            "timerId": "timer-b",
            "commandId": "cancel-b",
            "phase": "focus",
            "status": "cancelled",
            "plannedDurationMs": 300_000,
            "endedAt": "2026-07-15T10:00:01.1Z"
        })
    );
}

#[test]
fn timer_reduce_v1_clamps_transitions_and_auto_completes() {
    let paused = reduce(
        vec![
            command("command-a", "device-a", "timer-a", "start", 1, 100, 0, 0),
            command(
                "command-b",
                "device-a",
                "timer-a",
                "pause",
                2,
                200,
                30_000,
                999_999,
            ),
        ],
        3_600_000,
    );
    assert_eq!(paused["canonicalTimer"]["status"], "paused");
    assert_eq!(paused["canonicalTimer"]["elapsedAtAnchorMs"], 300_000);

    let completed = reduce(
        vec![command(
            "command-c",
            "device-a",
            "timer-c",
            "start",
            3,
            300,
            0,
            0,
        )],
        360_000,
    );
    assert_eq!(completed["canonicalTimer"]["status"], "completed");
    assert_eq!(completed["canonicalTimer"]["elapsedAtAnchorMs"], 300_000);
    assert_eq!(completed["history"][0]["status"], "completed");
    assert_eq!(completed["history"][0]["commandId"], Value::Null);
    assert_eq!(
        completed["history"][0]["completedAt"],
        "2026-07-15T10:05:00Z"
    );
}

#[test]
fn timer_reduce_v1_finish_at_deadline_claims_auto_completion() {
    let result = reduce(
        vec![
            command("command-a", "device-a", "timer-a", "start", 1, 100, 0, 0),
            command(
                "command-b",
                "device-a",
                "timer-a",
                "finish",
                2,
                200,
                300_000,
                300_000,
            ),
        ],
        300_000,
    );
    assert_outcome(&result, "command-b", "applied", "");
    assert_eq!(result["history"][0]["commandId"], "command-b");
    assert_eq!(result["history"][0]["endedAt"], "2026-07-15T10:05:00Z");
}

#[test]
fn timer_reduce_v1_latest_start_and_resume_supersede_active_timer() {
    let result = reduce(
        vec![
            command("command-a", "device-a", "timer-a", "start", 1, 100, 0, 0),
            command(
                "command-b",
                "device-a",
                "timer-a",
                "pause",
                2,
                200,
                60_000,
                60_000,
            ),
            command(
                "command-c",
                "device-b",
                "timer-b",
                "start",
                1,
                300,
                120_000,
                0,
            ),
            command(
                "command-d",
                "device-a",
                "timer-a",
                "resume",
                3,
                400,
                180_000,
                90_000,
            ),
        ],
        180_000,
    );
    assert_eq!(result["canonicalTimer"]["id"], "timer-a");
    assert_eq!(result["canonicalTimer"]["status"], "running");
    let timer_b = session(&result, "timer-b").unwrap();
    assert_eq!(timer_b["status"], "superseded");
    assert_eq!(timer_b["supersededByTimerId"], "timer-a");
}

#[test]
fn timer_reduce_v1_running_canonical_keeps_anchor_elapsed() {
    let result = reduce(
        vec![command(
            "command-a",
            "device-a",
            "timer-a",
            "start",
            1,
            100,
            0,
            0,
        )],
        120_000,
    );
    assert_eq!(result["canonicalTimer"]["elapsedAtAnchorMs"], 0);
    assert_eq!(result["canonicalTimer"]["anchorAt"], "2026-07-15T10:00:00Z");
}

#[test]
fn timer_reduce_v1_clear_removes_canonical_and_preserves_history() {
    let result = reduce(
        vec![
            command("command-a", "device-a", "timer-a", "start", 1, 100, 0, 0),
            command(
                "command-b",
                "device-a",
                "timer-a",
                "finish",
                2,
                200,
                60_000,
                60_000,
            ),
            command(
                "command-c",
                "device-a",
                "timer-a",
                "clear",
                3,
                300,
                120_000,
                60_000,
            ),
        ],
        120_000,
    );
    assert_eq!(result["canonicalTimer"], Value::Null);
    assert_eq!(result["history"].as_array().unwrap().len(), 1);
    assert_eq!(result["history"][0]["timerId"], "timer-a");
    assert_outcome(&result, "command-c", "applied", "");
}

#[test]
fn timer_reduce_v1_task_association_comes_only_from_start() {
    let mut start = command("command-a", "device-a", "timer-a", "start", 1, 100, 0, 0);
    start["taskId"] = json!("task-0000001");
    let mut finish = command(
        "command-b",
        "device-a",
        "timer-a",
        "finish",
        2,
        200,
        60_000,
        60_000,
    );
    finish["taskId"] = json!("task-ignored");

    assert_eq!(
        reduce(vec![start.clone()], 0)["canonicalTimer"]["taskId"],
        "task-0000001"
    );
    assert_eq!(
        reduce(vec![start, finish], 60_000)["history"][0]["taskId"],
        "task-0000001"
    );
}

#[test]
fn timer_reduce_v1_matches_server_command_outcomes() {
    let start = command("start", "device-a", "timer-a", "start", 1, 100, 0, 0);
    let cases = [
        (
            vec![
                start.clone(),
                command(
                    "duplicate",
                    "device-a",
                    "timer-a",
                    "start",
                    2,
                    200,
                    1_000,
                    0,
                ),
            ],
            "duplicate",
            "applied",
            "",
        ),
        (
            vec![command(
                "pause", "device-a", "timer-a", "pause", 1, 100, 0, 0,
            )],
            "pause",
            "ignored",
            "timer is not the active running timer",
        ),
        (
            vec![
                start.clone(),
                command("resume", "device-a", "timer-a", "resume", 2, 200, 1_000, 0),
            ],
            "resume",
            "applied",
            "",
        ),
        (
            vec![command(
                "finish", "device-a", "timer-a", "finish", 1, 100, 0, 0,
            )],
            "finish",
            "ignored",
            "timer is not active",
        ),
        (
            vec![
                start,
                command("clear", "device-a", "timer-a", "clear", 2, 200, 1_000, 0),
            ],
            "clear",
            "applied",
            "",
        ),
        (
            vec![command(
                "unsupported",
                "device-a",
                "timer-a",
                "skip",
                1,
                100,
                0,
                0,
            )],
            "unsupported",
            "rejected",
            "unsupported command type",
        ),
    ];
    for (commands, target, outcome, reason) in cases {
        assert_outcome(&reduce(commands, 2_000), target, outcome, reason);
    }
}

#[test]
fn timer_reduce_v1_cancel_clamps_invalid_observed_elapsed() {
    for (observed, expected) in [(-1, 0), (999_999, 300_000)] {
        let result = reduce(
            vec![
                command("start", "device-a", "timer-a", "start", 1, 100, 0, 0),
                command(
                    "cancel", "device-a", "timer-a", "cancel", 2, 200, 1_000, observed,
                ),
            ],
            1_000,
        );
        assert_eq!(result["canonicalTimer"]["status"], "cancelled");
        assert_eq!(result["canonicalTimer"]["elapsedAtAnchorMs"], expected);
    }
}

fn matrix_setup(state: &str) -> Vec<Value> {
    let start = command(
        "setup-start",
        "device-setup",
        "timer-state",
        "start",
        1,
        100,
        0,
        0,
    );
    match state {
        "absent" => vec![],
        "running" => vec![start],
        "paused" => vec![
            start,
            command(
                "setup-pause",
                "device-setup",
                "timer-state",
                "pause",
                2,
                200,
                1_000,
                1_000,
            ),
        ],
        "completed" => vec![
            start,
            command(
                "setup-finish",
                "device-setup",
                "timer-state",
                "finish",
                2,
                200,
                1_000,
                1_000,
            ),
        ],
        "cancelled" => vec![
            start,
            command(
                "setup-cancel",
                "device-setup",
                "timer-state",
                "cancel",
                2,
                200,
                1_000,
                1_000,
            ),
        ],
        "superseded" => vec![
            start,
            command(
                "setup-replacement",
                "device-setup",
                "timer-current",
                "start",
                2,
                200,
                1_000,
                0,
            ),
        ],
        _ => panic!("unknown matrix state"),
    }
}

fn matrix_outcome(state: &str, kind: &str, target: &str) -> (&'static str, &'static str) {
    let same = target == "same";
    if kind == "start" || (same && state != "absent") {
        return ("applied", "");
    }
    match kind {
        "pause" => ("ignored", "timer is not the active running timer"),
        "resume" => ("ignored", "timer cannot be resumed"),
        "finish" | "cancel" => ("ignored", "timer is not active"),
        "clear" => ("ignored", "timer cannot be cleared"),
        _ => panic!("unknown matrix command"),
    }
}

fn matrix_state_session(state: &str, kind: &str, target: &str) -> Option<&'static str> {
    if state == "absent" {
        return (kind == "start" && target == "same").then_some("running");
    }
    if target == "foreign" {
        if kind == "start" && matches!(state, "running" | "paused") {
            return Some("superseded");
        }
        return Some(match state {
            "running" => "running",
            "paused" => "paused",
            "completed" => "completed",
            "cancelled" => "cancelled",
            "superseded" => "superseded",
            _ => unreachable!(),
        });
    }
    match kind {
        "start" => Some("running"),
        "pause" => Some("paused"),
        "resume" => Some("running"),
        "finish" => Some("completed"),
        "cancel" => Some("cancelled"),
        _ => matrix_state_session(state, "unchanged", "foreign"),
    }
}

fn matrix_canonical(state: &str, kind: &str, target: &str) -> Option<(&'static str, &'static str)> {
    if target == "foreign" {
        if kind == "start" {
            return Some(("timer-foreign", "running"));
        }
        return match state {
            "absent" => None,
            "superseded" => Some(("timer-current", "running")),
            "running" => Some(("timer-state", "running")),
            "paused" => Some(("timer-state", "paused")),
            "completed" => Some(("timer-state", "completed")),
            "cancelled" => Some(("timer-state", "cancelled")),
            _ => unreachable!(),
        };
    }
    if state == "absent" {
        return (kind == "start").then_some(("timer-state", "running"));
    }
    if kind == "clear" {
        return (state == "superseded").then_some(("timer-current", "running"));
    }
    Some((
        "timer-state",
        matrix_state_session(state, kind, target).unwrap(),
    ))
}

#[test]
fn timer_reduce_v1_matches_complete_server_state_command_target_matrix() {
    let states = [
        "absent",
        "running",
        "paused",
        "completed",
        "cancelled",
        "superseded",
    ];
    let command_types = ["start", "pause", "resume", "finish", "cancel", "clear"];
    let targets = ["same", "foreign"];
    let mut cases = 0;

    for state in states {
        for kind in command_types {
            for target in targets {
                let mut commands = matrix_setup(state);
                let target_id = if target == "same" {
                    "timer-state"
                } else {
                    "timer-foreign"
                };
                commands.push(command(
                    "matrix-action",
                    "device-action",
                    target_id,
                    kind,
                    99,
                    1_000,
                    10_000,
                    10_000,
                ));
                let result = reduce(commands, 11_000);
                let (outcome, reason) = matrix_outcome(state, kind, target);
                assert_outcome(&result, "matrix-action", outcome, reason);

                let expected_state = matrix_state_session(state, kind, target);
                assert_eq!(
                    session(&result, "timer-state").map(|value| value["status"].as_str().unwrap()),
                    expected_state,
                    "state session for {state}/{kind}/{target}"
                );
                let foreign = session(&result, "timer-foreign");
                let want_foreign = target == "foreign" && kind == "start";
                assert_eq!(foreign.is_some(), want_foreign);
                if let Some(foreign) = foreign {
                    assert_eq!(foreign["status"], "running");
                }

                match matrix_canonical(state, kind, target) {
                    None => assert_eq!(result["canonicalTimer"], Value::Null),
                    Some((id, status)) => {
                        assert_eq!(result["canonicalTimer"]["id"], id);
                        assert_eq!(result["canonicalTimer"]["status"], status);
                    }
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 72);
}

fn production_fixture_command(command: &Value) -> Value {
    let mut result = Map::new();
    for (target, source) in [
        ("id", "id"),
        ("deviceSequence", "sequence"),
        ("deviceId", "deviceId"),
        ("timerId", "timerId"),
        ("type", "type"),
        ("phase", "phase"),
        ("plannedDurationMs", "durationMs"),
        ("hlcWallMs", "wallMs"),
        ("hlcCounter", "counter"),
        ("observedElapsedMs", "elapsedMs"),
    ] {
        result.insert(target.to_owned(), command[source].clone());
    }
    result.insert(
        "occurredAt".to_owned(),
        json!(timestamp(20, 12, command["atMs"].as_i64().unwrap())),
    );
    if let Some(task_id) = command.get("taskId") {
        result.insert("taskId".to_owned(), task_id.clone());
    }
    Value::Object(result)
}

fn normalize_fixture_result(result: &Value, expected: &Value) -> Value {
    let timer = expected.get("timer").map(|timer| {
        let mut value = json!({
            "id": result["canonicalTimer"]["id"].clone(),
            "status": result["canonicalTimer"]["status"].clone(),
            "phase": result["canonicalTimer"]["phase"].clone(),
            "durationMs": result["canonicalTimer"]["plannedDurationMs"].clone(),
            "elapsedMs": result["canonicalTimer"]["elapsedAtAnchorMs"].clone(),
            "anchorMs": timer["anchorMs"].clone(),
            "lastCommandId": result["canonicalTimer"]["lastIntent"]["commandId"].clone(),
        });
        assert_eq!(
            result["canonicalTimer"]["anchorAt"],
            timestamp(20, 12, timer["anchorMs"].as_i64().unwrap())
        );
        if let Some(task_id) = result["canonicalTimer"].get("taskId") {
            value["taskId"] = task_id.clone();
        }
        value
    });

    let history = result["history"]
        .as_array()
        .unwrap()
        .iter()
        .zip(expected["history"].as_array().unwrap())
        .map(|(actual, wanted)| {
            assert_eq!(
                actual["endedAt"],
                timestamp(20, 12, wanted["endedMs"].as_i64().unwrap())
            );
            let mut value = json!({
                "timerId": actual["timerId"].clone(),
                "status": actual["status"].clone(),
                "phase": actual["phase"].clone(),
                "durationMs": actual["plannedDurationMs"].clone(),
                "endedMs": wanted["endedMs"].clone(),
            });
            if let Some(command_id) = actual.get("commandId") {
                value["commandId"] = command_id.clone();
            }
            if let Some(task_id) = actual.get("taskId") {
                value["taskId"] = task_id.clone();
            }
            value
        })
        .collect::<Vec<_>>();

    let mut normalized = json!({"history": history});
    if let Some(timer) = timer {
        normalized["timer"] = timer;
    }
    normalized
}

#[test]
fn timer_reduce_v1_matches_every_server_convergence_fixture_permutation() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/convergence-v1.json")).unwrap();
    assert_eq!(fixture["version"], 2);

    for fixture_case in fixture["cases"].as_array().unwrap() {
        let commands = fixture_case["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(production_fixture_command)
            .collect::<Vec<_>>();
        for arrival_order in permutations(&commands) {
            let input = json!({
                "commands": arrival_order,
                "now": timestamp(20, 12, fixture_case["nowMs"].as_i64().unwrap()),
            });
            let result: Value = serde_json::from_str(
                &dispatch_json("timer.reduce.v1", &input.to_string()).unwrap(),
            )
            .unwrap();
            assert_eq!(
                normalize_fixture_result(&result, &fixture_case["expected"]),
                fixture_case["expected"],
                "fixture case {}",
                fixture_case["name"]
            );
            assert_eq!(
                result["outcomes"].as_object().unwrap().len(),
                commands.len()
            );
            assert!(!result["sessions"].is_null());
        }
    }
}

#[test]
fn timer_reduce_v1_rejects_invalid_supplied_state() {
    let now = timestamp(15, 10, 0);
    let unsafe_timer = json!({
        "id": "unsafe",
        "phase": "focus",
        "status": "running",
        "plannedDurationMs": i64::MAX,
        "elapsedAtAnchorMs": 0,
        "anchorAt": now
    });
    assert!(
        dispatch_json(
            "timer.reduce.v1",
            &json!({
                "commands": [],
                "canonicalTimer": unsafe_timer,
                "history": [],
                "now": now
            })
            .to_string()
        )
        .is_err()
    );

    let valid_timer = json!({
        "id": "overlap",
        "phase": "focus",
        "status": "running",
        "plannedDurationMs": 60_000,
        "elapsedAtAnchorMs": 0,
        "anchorAt": now
    });
    let history = json!([{
        "id": "history-overlap",
        "timerId": "overlap",
        "phase": "focus",
        "status": "completed",
        "plannedDurationMs": 60_000,
        "completedAt": now
    }]);
    assert!(
        dispatch_json(
            "timer.reduce.v1",
            &json!({
                "commands": [],
                "canonicalTimer": valid_timer,
                "history": history,
                "now": now
            })
            .to_string()
        )
        .is_err()
    );
}
