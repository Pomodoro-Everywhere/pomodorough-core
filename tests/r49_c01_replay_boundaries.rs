use pomodorough_core::dispatch_envelope_json;
use serde_json::{Value, json};

const START: &str = "2026-07-20T12:00:00Z";
const JUMP: &str = "2026-07-20T12:01:20Z";
const NOW: &str = "2026-07-20T12:00:11Z";

fn call(operation: &str, input: &Value) -> Value {
    let raw = match std::env::var("R49_C01_REPLAY_ORACLE") {
        Ok(oracle) => external_dispatch(&oracle, operation, input),
        Err(_) => dispatch_envelope_json(operation, &input.to_string()),
    };
    let output: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(output["ok"], true, "{operation}: {output}");
    output["value"].clone()
}

fn external_dispatch(oracle: &str, operation: &str, input: &Value) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(oracle)
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
    String::from_utf8(output.stdout).unwrap()
}

fn command(index: usize, at: &str) -> Value {
    json!({"id": format!("command-{index:04}"), "deviceId": "device-local",
        "deviceSequence": index + 1, "timerId": "existing-timer", "taskId": format!("task-{index}"),
        "type": "retarget", "phase": "focus", "plannedDurationMs": 60000,
        "occurredAt": at, "hlcWallMs": 1784548880000_i64, "hlcCounter": index,
        "observedElapsedMs": 16000})
}

fn log(count: usize, retarget_at: &str) -> Vec<Value> {
    let mut start = command(0, START);
    start["type"] = json!("start");
    start["observedElapsedMs"] = json!(0);
    start["taskId"] = json!("task-original");
    start["hlcWallMs"] = json!(1784548800000_i64);
    std::iter::once(start)
        .chain((1..count).map(|i| command(i, retarget_at)))
        .collect()
}

fn page(commands: &[Value], prior: &Value, now: Option<&str>) -> Value {
    let mut input = json!({"commands": commands, "sessions": prior["sessions"],
        "currentTimerId": prior["currentTimerId"], "after": prior["after"]});
    if let Some(now) = now {
        input["now"] = json!(now);
    }
    call("timer.replay.page.v1", &input)
}

fn paged(commands: &[Value], lengths: &[usize], now: &str, clock_only: bool) -> Value {
    assert_eq!(lengths.iter().sum::<usize>(), commands.len());
    let mut prior = json!({"sessions": [], "currentTimerId": null, "after": null});
    let mut outcomes = serde_json::Map::new();
    let mut offset = 0;
    for (index, length) in lengths.iter().enumerate() {
        assert!((1..=256).contains(length));
        let final_clock = (index + 1 == lengths.len() && !clock_only).then_some(now);
        prior = page(&commands[offset..offset + length], &prior, final_clock);
        outcomes.extend(prior["outcomes"].as_object().unwrap().clone());
        offset += length;
    }
    if clock_only {
        prior = page(&[], &prior, Some(now));
        assert_eq!(prior["outcomes"], json!({}));
    }
    prior.as_object_mut().unwrap().remove("currentTimerId");
    prior.as_object_mut().unwrap().remove("after");
    prior["outcomes"] = Value::Object(outcomes);
    prior
}

fn partitions(count: usize, size: usize) -> Vec<usize> {
    (0..count)
        .step_by(size)
        .map(|offset| size.min(count - offset))
        .collect()
}

#[test]
fn actual_257_command_log_keeps_full_and_multipage_complete_returns_equal() {
    for at in [NOW, JUMP] {
        let commands = log(257, at);
        let full = call(
            "timer.reduce.v1",
            &json!({"commands": commands, "now": NOW}),
        );
        assert_eq!(
            paged(&commands, &[256, 1], NOW, false),
            full,
            "real nonfinal page parity"
        );
        assert_eq!(
            paged(&commands, &[256, 1], NOW, true),
            full,
            "final clock-only page parity"
        );
        if at == JUMP {
            assert_eq!(
                full["canonicalTimer"]["status"], "completed",
                "post-deadline Retarget must use occurrence"
            );
            assert_eq!(full, post_deadline_expected(&commands));
        }
        for lengths in [
            vec![256, 1],
            vec![1, 256],
            vec![128, 128, 1],
            partitions(257, 7),
        ] {
            for clock_only in [false, true] {
                assert_eq!(
                    paged(&commands, &lengths, NOW, clock_only),
                    full,
                    "{at} {lengths:?} {clock_only}"
                );
            }
        }
    }
}

#[test]
fn page_boundaries_and_deadline_clocks_never_change_full_replay() {
    for count in [255, 256, 258, 513] {
        for at in [NOW, JUMP] {
            let commands = log(count, at);
            for now in [
                NOW,
                "2026-07-20T12:00:59.999Z",
                "2026-07-20T12:01:00Z",
                JUMP,
            ] {
                let full = call(
                    "timer.reduce.v1",
                    &json!({"commands": commands, "now": now}),
                );
                for size in [1, 255, 256] {
                    let lengths = partitions(count, size);
                    for clock_only in [false, true] {
                        assert_eq!(
                            paged(&commands, &lengths, now, clock_only),
                            full,
                            "count={count} size={size} at={at} now={now} clock_only={clock_only}"
                        );
                    }
                }
            }
        }
    }
}

fn post_deadline_expected(commands: &[Value]) -> Value {
    let outcomes: serde_json::Map<_, _> = commands
        .iter()
        .skip(1)
        .map(|command| {
            (
                command["id"].as_str().unwrap().to_owned(),
                json!({"outcome": "ignored",
            "reason": "timer is not the active focus timer"}),
            )
        })
        .chain(std::iter::once((
            "command-0000".into(),
            json!({"outcome": "applied", "reason": ""}),
        )))
        .collect();
    let intent = json!({"type": "start", "commandId": "command-0000", "occurredAt": START});
    json!({"canonicalTimer": {"id": "existing-timer", "taskId": "task-original", "phase": "focus",
        "status": "completed", "plannedDurationMs": 60000, "elapsedAtAnchorMs": 60000,
        "anchorAt": "2026-07-20T12:01:00Z", "startedByDeviceId": "device-local", "lastIntent": intent},
        "history": [{"id": "existing-timer", "timerId": "existing-timer", "taskId": "task-original",
            "phase": "focus", "status": "completed", "plannedDurationMs": 60000,
            "completedAt": "2026-07-20T12:01:00Z", "endedAt": "2026-07-20T12:01:00Z"}],
        "sessions": [{"timerId": "existing-timer", "taskId": "task-original", "phase": "focus",
            "status": "completed", "plannedDurationMs": 60000, "elapsedAtAnchorMs": 60000,
            "anchorAt": "2026-07-20T12:01:00Z", "startedAt": START, "startedByDeviceId": "device-local",
            "endedAt": "2026-07-20T12:01:00Z", "lastCommandId": "command-0000", "lastIntent": intent}],
        "outcomes": outcomes})
}
