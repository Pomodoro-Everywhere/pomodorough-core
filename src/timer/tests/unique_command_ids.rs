//! R55-C06 Low T3: each timer replay keeps its own duplicate id check. Public
//! callers reach the strict `replay` only after `replay_public` or the
//! reconciliation queue check has run, so only a direct call proves that the
//! strict replay and the workspace replay still reject duplicates on their own.
use serde_json::json;

use super::{WireCommand, replay, replay_public, workspace};
use crate::CoreError;

fn command(id: &str, timer: &str, counter: i64) -> WireCommand {
    serde_json::from_value(json!({"id": id, "deviceId": "device-a",
        "deviceSequence": counter + 1, "timerId": timer, "type": "start",
        "phase": "focus", "plannedDurationMs": 60000,
        "occurredAt": "2026-09-13T12:00:01Z", "hlcWallMs": 1789300800000_i64,
        "hlcCounter": counter, "observedElapsedMs": 0}))
    .unwrap()
}

fn twins() -> Vec<WireCommand> {
    vec![
        command("command-dup", "timer-a", 0),
        command("command-dup", "timer-b", 1),
    ]
}

fn assert_duplicate<T: std::fmt::Debug>(result: Result<T, CoreError>, caller: &str) {
    match result {
        Err(CoreError::InvalidInput(message)) => {
            assert_eq!(message, "duplicate timer command", "{caller}");
        }
        other => panic!("{caller}: {other:?}"),
    }
}

#[test]
fn every_replay_rejects_duplicate_ids_by_itself() {
    let now = "2026-09-13T12:00:02Z";
    assert_duplicate(replay(None, vec![], twins(), now), "strict replay");
    assert_duplicate(replay_public(None, vec![], twins(), now), "public replay");
    assert_duplicate(
        workspace::replay(None, vec![], twins(), now),
        "workspace replay",
    );
    let distinct = vec![
        command("command-a", "timer-a", 0),
        command("command-b", "timer-b", 1),
    ];
    assert!(replay(None, vec![], distinct, now).is_ok());
}

#[test]
fn strict_replay_checks_ids_before_the_time() {
    assert_duplicate(replay(None, vec![], twins(), "not-a-time"), "strict replay");
    assert_duplicate(
        workspace::replay(None, vec![], twins(), "not-a-time"),
        "workspace replay",
    );
}
