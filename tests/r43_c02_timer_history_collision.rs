use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const ACTIVE_NOW: &str = "2026-09-13T12:00:02Z";
const COMPLETED_NOW: &str = "2026-09-13T12:02:00Z";

fn command(id: &str, kind: &str, timer_id: &str, counter: i64) -> Value {
    json!({"id": id, "deviceId": "device-a", "deviceSequence": counter + 1,
        "timerId": timer_id, "type": kind, "phase": "focus",
        "plannedDurationMs": 60000, "observedElapsedMs": 0,
        "occurredAt": "2026-09-13T12:00:01Z",
        "hlcWallMs": 1789300801000_i64, "hlcCounter": counter})
}

fn history(id: &str, timer_id: &str) -> Value {
    json!({"id": id, "timerId": timer_id, "phase": "focus",
        "status": "completed", "plannedDurationMs": 60000,
        "completedAt": "2026-09-13T11:59:00Z"})
}

fn reduce(input: Value) -> Value {
    serde_json::from_str(&dispatch_json("timer.reduce.v1", &input.to_string()).unwrap()).unwrap()
}

fn assert_roundtrip(output: &Value, now: &str) {
    let next = reduce(
        json!({"commands": [], "canonicalTimer": output["canonicalTimer"],
        "history": output["history"], "now": now}),
    );
    assert_eq!(next["canonicalTimer"], output["canonicalTimer"]);
    assert_eq!(next["history"], output["history"]);
}

#[test]
fn new_start_rejects_legacy_history_id_collision_before_and_after_deadline() {
    for now in [COMPLETED_NOW, ACTIVE_NOW] {
        let base = reduce(json!({"commands": [],
            "history": [history("new-timer", "old-timer")], "now": now}));
        let output = reduce(
            json!({"commands": [command("start", "start", "new-timer", 0)],
            "history": base["history"], "now": now}),
        );
        assert_eq!(
            output["outcomes"]["start"]["outcome"], "rejected",
            "{output}"
        );
        assert_eq!(output["history"], base["history"]);
        assert_eq!(output["sessions"], base["sessions"]);
        assert!(output["canonicalTimer"].is_null());
        assert_roundtrip(&output, now);
    }
}

#[test]
fn output_history_after_deadline_is_valid_input_to_next_reduce() {
    let output = reduce(
        json!({"commands": [command("start", "start", "new-timer", 0)],
        "history": [history("new-timer", "old-timer")], "now": COMPLETED_NOW}),
    );
    let next = reduce(json!({"commands": [], "history": output["history"],
        "now": COMPLETED_NOW}));
    assert_eq!(next["history"], output["history"]);
}

#[test]
fn rejected_start_does_not_supersede_unrelated_active_timer() {
    let base = reduce(
        json!({"commands": [command("keep", "start", "current-timer", 0)],
        "history": [history("new-timer", "old-timer")], "now": ACTIVE_NOW}),
    );
    let output = reduce(
        json!({"commands": [command("collision", "start", "new-timer", 1)],
        "canonicalTimer": base["canonicalTimer"], "history": base["history"],
        "now": ACTIVE_NOW}),
    );
    assert_eq!(output["outcomes"]["collision"]["outcome"], "rejected");
    assert_eq!(output["canonicalTimer"], base["canonicalTimer"]);
    assert_eq!(output["history"], base["history"]);
    assert_roundtrip(&output, ACTIVE_NOW);
}

#[test]
fn new_start_rejects_collision_with_reactivated_legacy_session() {
    let resume = command("resume", "resume", "old-timer", 0);
    let base = reduce(json!({"commands": [resume.clone()],
        "history": [history("new-timer", "old-timer")], "now": ACTIVE_NOW}));
    let output = reduce(
        json!({"commands": [resume, command("start", "start", "new-timer", 1)],
        "history": [history("new-timer", "old-timer")], "now": ACTIVE_NOW}),
    );
    assert_eq!(output["outcomes"]["start"]["outcome"], "rejected");
    assert_eq!(output["canonicalTimer"], base["canonicalTimer"]);
    assert_eq!(output["sessions"], base["sessions"]);
    assert_eq!(output["history"], base["history"]);
    assert_roundtrip(&output, ACTIVE_NOW);
}

#[test]
fn same_session_start_remains_applied_for_active_and_completed_sessions() {
    for history_id in ["old-timer", "legacy-history"] {
        for active in [false, true] {
            let mut commands = Vec::new();
            if active {
                commands.push(command("resume", "resume", "old-timer", 0));
            }
            commands.push(command("restart", "start", "old-timer", 1));
            let output = reduce(json!({"commands": commands,
                "history": [history(history_id, "old-timer")], "now": ACTIVE_NOW}));
            assert_eq!(output["outcomes"]["restart"]["outcome"], "applied");
            assert_eq!(output["canonicalTimer"]["id"], "old-timer");
            assert_eq!(output["canonicalTimer"]["status"], "running");
            assert_eq!(output["sessions"].as_array().unwrap().len(), 1);
            assert!(output["history"].as_array().unwrap().is_empty());
            assert_roundtrip(&output, ACTIVE_NOW);
        }
    }
}

#[test]
fn noncolliding_start_preserves_legacy_history_and_completed_output_roundtrips() {
    for now in [ACTIVE_NOW, COMPLETED_NOW] {
        let base = reduce(json!({"commands": [],
            "history": [history("legacy-history", "old-timer")], "now": now}));
        let mut commands = vec![command("start", "start", "new-timer", 0)];
        if now == COMPLETED_NOW {
            commands.push(command("finish", "finish", "new-timer", 1));
            commands.push(command("clear", "clear", "new-timer", 2));
        }
        let output = reduce(json!({"commands": commands, "history": base["history"], "now": now}));
        assert_eq!(output["outcomes"]["start"]["outcome"], "applied");
        let items = output["history"].as_array().unwrap();
        assert!(items.contains(&base["history"][0]));
        assert_eq!(items.len(), if now == ACTIVE_NOW { 1 } else { 2 });
        if now == COMPLETED_NOW {
            assert_eq!(items[0]["id"], "new-timer");
            assert_eq!(items[0]["timerId"], "new-timer");
        }
        assert_roundtrip(&output, now);
    }
}
