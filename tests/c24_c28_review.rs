use pomodorough_core::{dispatch_json, reduce_timer_fixture_case_json};
use serde_json::{Value, json};

const SERVER_TIME: &str = "2026-07-20T12:00:10Z";
const SERVER_WALL_MS: i64 = 1_784_548_810_000;

fn selected_op(id: &str, task_id: Value, wall_offset_ms: i64) -> Value {
    json!({
        "id": id,
        "deviceId": "device-c24",
        "occurredAt": "2026-07-20T12:00:11Z",
        "hlcWallMs": SERVER_WALL_MS + wall_offset_ms,
        "hlcCounter": 0,
        "taskId": task_id
    })
}

#[test]
fn c24_empty_selected_task_rejected_through_reduce_v1() {
    let input = json!({
        "operations": [selected_op("select-empty", json!(""), 1_000)],
        "activeTaskIds": []
    });
    let error = dispatch_json("selectedTask.reduce.v1", &input.to_string()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("invalid selected task operation"),
        "unexpected error: {error}"
    );
    let deselect = json!({
        "operations": [selected_op("deselect", Value::Null, 1_000)],
        "activeTaskIds": []
    });
    let output: Value = serde_json::from_str(
        &dispatch_json("selectedTask.reduce.v1", &deselect.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(output["selectedTaskId"], Value::Null);
}

fn fixture_input(mutate: impl FnOnce(&mut Value)) -> String {
    let mut command = json!({
        "id": "c1",
        "sequence": 1,
        "deviceId": "device-a",
        "timerId": "timer-a",
        "type": "start",
        "phase": "focus",
        "durationMs": 60000,
        "atMs": 0,
        "wallMs": 100,
        "counter": 0,
        "elapsedMs": 0
    });
    mutate(&mut command);
    json!({
        "epoch": "2026-07-20T12:00:00.000Z",
        "nowMs": 1_000,
        "commands": [command]
    })
    .to_string()
}

#[test]
fn c25_fixture_commands_enforce_production_validation() {
    let valid: Value =
        serde_json::from_str(&reduce_timer_fixture_case_json(&fixture_input(|_| {})).unwrap())
            .unwrap();
    assert_eq!(valid["timer"]["status"], json!("running"));
    for (case, field, value, message) in [
        ("empty-id", "id", json!(""), "invalid timer command"),
        (
            "bad-sequence",
            "sequence",
            json!(0),
            "invalid timer command",
        ),
        (
            "bad-phase",
            "phase",
            json!("custom"),
            "invalid timer command",
        ),
        (
            "empty-task",
            "taskId",
            json!(""),
            "invalid timer task identity",
        ),
    ] {
        let error = reduce_timer_fixture_case_json(&fixture_input(|command| {
            command[field] = value.clone();
        }))
        .unwrap_err();
        assert!(
            error.to_string().contains(message),
            "{case}: unexpected error: {error}"
        );
    }
}

fn timer_command(id: &str, timer_id: &str, kind: &str, sequence: i64, wall_ms: i64) -> Value {
    json!({
        "id": id,
        "deviceId": "device-a",
        "deviceSequence": sequence,
        "timerId": timer_id,
        "type": kind,
        "phase": "focus",
        "plannedDurationMs": 300_000,
        "occurredAt": "2026-07-15T10:00:00Z",
        "hlcWallMs": wall_ms,
        "hlcCounter": 0,
        "observedElapsedMs": 0
    })
}

#[test]
fn c26_unknown_timer_kind_rejects_command_but_applies_siblings() {
    let input = json!({
        "commands": [
            timer_command("start-a", "timer-a", "start", 1, 100),
            timer_command("typo", "timer-b", "strat", 1, 200)
        ],
        "now": "2026-07-15T10:05:00Z"
    });
    let output: Value =
        serde_json::from_str(&dispatch_json("timer.reduce.v1", &input.to_string()).unwrap())
            .unwrap();
    assert_eq!(
        output["outcomes"]["typo"],
        json!({"outcome": "rejected", "reason": "unsupported command type"})
    );
    assert_eq!(
        output["outcomes"]["start-a"],
        json!({"outcome": "applied", "reason": ""})
    );
    assert_eq!(output["canonicalTimer"]["id"], json!("timer-a"));
}

fn empty_queues() -> Value {
    json!({
        "commands": [],
        "taskOperations": [],
        "durationOperations": [],
        "autoStartOperations": [],
        "selectedTaskOperations": []
    })
}

fn valid_response(tasks: Value, acknowledgements: Value) -> Value {
    json!({
        "acknowledgements": acknowledgements,
        "taskAcknowledgements": [],
        "durationAcknowledgements": [],
        "autoStartAcknowledgements": [],
        "selectedTaskAcknowledgements": [],
        "revision": 7,
        "canonicalTimer": null,
        "history": [],
        "tasks": tasks,
        "durationsMs": {"focus": 1_200_000, "short_break": 300_000, "long_break": 900_000},
        "autoStartBreaks": false,
        "selectedTaskId": null,
        "serverTime": SERVER_TIME,
        "serverHlcWallMs": SERVER_WALL_MS,
        "serverHlcCounter": 0
    })
}

#[test]
fn c27_acknowledgement_tolerates_unknown_fields() {
    let mut local = empty_queues();
    local["commands"] = json!([timer_command(
        "cmd-ack-1",
        "timer-ack-1",
        "start",
        1,
        SERVER_WALL_MS + 1_000
    )]);
    let mut sent = empty_queues();
    sent["commands"] = json!([{"id": "cmd-ack-1"}]);
    let acknowledgements = json!([{
        "commandId": "cmd-ack-1",
        "outcome": "applied",
        "reason": "",
        "futureNote": "server v2 field",
        "futureObject": {"nested": [1, 2]}
    }]);
    let input = json!({
        "local": local,
        "sent": sent,
        "response": valid_response(json!([]), acknowledgements),
        "timerDependencies": []
    });
    let output: Value =
        serde_json::from_str(&dispatch_json("reconcile.rebase.v1", &input.to_string()).unwrap())
            .unwrap();
    assert_eq!(output["pending"], json!([]));
}

#[test]
fn c28_canonical_title_failures_collapse_to_generic_tasks_error() {
    for tasks in [
        json!([{"id": "task-x", "title": ""}]),
        json!([{"id": "task-x", "title": "a".repeat(513)}]),
    ] {
        let input = json!({
            "local": empty_queues(),
            "sent": empty_queues(),
            "response": valid_response(tasks, json!([])),
            "timerDependencies": []
        });
        let error = dispatch_json("reconcile.rebase.v1", &input.to_string()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid canonical response tasks"),
            "unexpected error: {error}"
        );
    }
}
