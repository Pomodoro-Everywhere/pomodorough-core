//! R55-C06 Lows T2 and T3: every timer entry point returns the exact duplicate
//! envelope, and duplicate ids win over the errors that later checks would
//! report. `timer.replay.page.v1` checks ids after the page limit and before any
//! command is decoded or ordered. `timer.reduce.v1` checks ids before the
//! canonical timer and history overlap decision. Each duplicate row has a
//! control with distinct ids that shows the error the duplicates hide.
use pomodorough_core::dispatch_envelope_json;
use serde_json::{Value, json};

const DUPLICATE: &str =
    r#"{"error":"invalid shared-core input: duplicate timer command","ok":false}"#;
const BAD_TIME: &str = r#"{"error":"invalid RFC 3339 timestamp: not-a-time","ok":false}"#;
const UNORDERED: &str =
    r#"{"error":"invalid shared-core input: timer replay page is not ordered","ok":false}"#;
const OVERLAP: &str =
    r#"{"error":"invalid shared-core input: canonical timer overlaps timer history","ok":false}"#;

fn envelope(operation: &str, input: &Value) -> String {
    dispatch_envelope_json(operation, &input.to_string())
}

fn command(id: &str, timer: &str, sequence: i64, counter: i64) -> Value {
    json!({"id": id, "deviceId": "device-a", "deviceSequence": sequence,
        "timerId": timer, "type": "start", "phase": "focus",
        "plannedDurationMs": 60000, "occurredAt": "2026-09-13T12:00:01Z",
        "hlcWallMs": 1789300800000_i64, "hlcCounter": counter,
        "observedElapsedMs": 0})
}

fn page(commands: Vec<Value>) -> Value {
    json!({"commands": commands, "sessions": [], "currentTimerId": null, "after": null})
}

fn with_time(mut command: Value, occurred_at: &str) -> Value {
    command["occurredAt"] = json!(occurred_at);
    command
}

/// `(name, commands, expected envelope prefix)`. A duplicate row pins the whole
/// envelope; its distinct-id control pins the error class it hides.
fn page_cases() -> Vec<(&'static str, Vec<Value>, &'static str)> {
    let [a, b, c] = [
        command("command-a", "timer-a", 1, 0),
        command("command-b", "timer-b", 2, 1),
        command("command-a", "timer-c", 3, 2),
    ];
    let bad_time = with_time(command("command-a", "timer-b", 2, 1), "not-a-time");
    let late = with_time(command("command-b", "timer-b", 2, 1), "not-a-time");
    let mut unordered = command("command-b", "timer-b", 2, 0);
    unordered["hlcWallMs"] = json!(1789300799999_i64);
    vec![
        (
            "adjacent",
            vec![a.clone(), command("command-a", "timer-b", 2, 1)],
            DUPLICATE,
        ),
        (
            "non-adjacent",
            vec![a.clone(), b.clone(), c.clone()],
            DUPLICATE,
        ),
        (
            "duplicate beats bad time",
            vec![a.clone(), bad_time],
            DUPLICATE,
        ),
        ("bad time control", vec![a.clone(), late], BAD_TIME),
        (
            "duplicate beats order",
            vec![a.clone(), unordered.clone(), c],
            DUPLICATE,
        ),
        ("order control", vec![a.clone(), unordered], UNORDERED),
        ("distinct control", vec![a, b], r#"{"ok":true,"#),
    ]
}

#[test]
fn replay_page_pins_the_duplicate_envelope_before_decoding() {
    for (name, commands, expected) in page_cases() {
        let actual = envelope("timer.replay.page.v1", &page(commands));
        if expected == DUPLICATE {
            assert_eq!(actual, DUPLICATE, "{name}");
        } else {
            assert!(actual.starts_with(expected), "{name}: {actual}");
            assert_ne!(actual, DUPLICATE, "{name}");
        }
    }
}

fn queues(commands: Vec<Value>) -> Value {
    json!({"commands": commands, "taskOperations": [], "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []})
}

fn rebase(commands: Vec<Value>) -> Value {
    json!({"local": queues(commands), "sent": queues(vec![]), "timerDependencies": [],
        "response": {
            "acknowledgements": [], "taskAcknowledgements": [],
            "durationAcknowledgements": [], "autoStartAcknowledgements": [],
            "selectedTaskAcknowledgements": [], "revision": 9,
            "canonicalTimer": null, "history": [], "tasks": [],
            "durationsMs": {"focus": 1_500_000, "short_break": 300_000, "long_break": 900_000},
            "autoStartBreaks": false, "selectedTaskId": null,
            "serverTime": "2026-09-13T12:00:10Z",
            "serverHlcWallMs": 1789300810000_i64, "serverHlcCounter": 7}})
}

fn projection(commands: Vec<Value>) -> Value {
    json!({"base": {"canonicalTimer": null, "history": [], "tasks": [],
            "durationsMs": {"focus": 1_200_000, "short_break": 300_000, "long_break": 900_000},
            "autoStartBreaks": false, "selectedTaskId": null},
        "pending": queues(commands), "now": "2026-09-13T12:00:02Z"})
}

#[test]
fn reconciliation_queue_and_projection_pin_the_duplicate_envelope() {
    let twins = || {
        let mut twin = command("command-dup", "timer-b", 2, 1);
        twin["taskId"] = Value::Null;
        vec![command("command-dup", "timer-a", 1, 0), twin]
    };
    let late = |id: &str| vec![with_time(command(id, "timer-a", 1, 0), "not-a-time")];
    let mut bad = twins();
    bad[1] = with_time(bad[1].clone(), "not-a-time");
    for (operation, input, control) in [
        (
            "reconcile.rebase.v1",
            rebase(twins()),
            rebase(late("command-a")),
        ),
        (
            "reconcile.rebase.v1",
            rebase(bad.clone()),
            rebase(late("command-b")),
        ),
        (
            "projection.apply.v2",
            projection(twins()),
            projection(late("command-a")),
        ),
        (
            "projection.apply.v2",
            projection(bad.clone()),
            projection(late("command-b")),
        ),
    ] {
        assert_eq!(envelope(operation, &input), DUPLICATE, "{operation}");
        assert_eq!(envelope(operation, &control), BAD_TIME, "{operation}");
    }
}

fn duration(id: &str) -> Value {
    json!({"id": id, "deviceId": "device-a", "phase": "focus", "durationMs": 1_800_000,
        "occurredAt": "2026-09-13T12:00:03Z", "hlcWallMs": 1789300803000_i64, "hlcCounter": 0})
}

/// The rebase queue check runs before the sibling queues are checked, so twin
/// commands win over twin duration operations. The strict replay check alone
/// would come too late and report the duration queue instead.
#[test]
fn rebase_reports_twin_commands_before_twin_sibling_operations() {
    let mut input = rebase(vec![
        command("command-dup", "timer-a", 1, 0),
        command("command-dup", "timer-b", 2, 1),
    ]);
    input["local"]["durationOperations"] =
        json!([duration("duration-dup"), duration("duration-dup")]);
    assert_eq!(envelope("reconcile.rebase.v1", &input), DUPLICATE);
    input["local"]["commands"][1]["id"] = json!("command-b");
    assert_eq!(
        envelope("reconcile.rebase.v1", &input),
        r#"{"error":"invalid shared-core input: invalid local durationOperations identities","ok":false}"#
    );
}

fn terminal_fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-terminal-v1.json")).unwrap()
}

fn merged(mut value: Value, overrides: Value) -> Value {
    for (name, field) in overrides.as_object().unwrap() {
        value[name] = field.clone();
    }
    value
        .as_object_mut()
        .unwrap()
        .retain(|_, field| !field.is_null());
    value
}

/// A running canonical timer that overlaps its own cancelled history row: the
/// workspace terminal validator rejects the pair, so it gets the overlap error.
fn overlapping_pair() -> (Value, Value) {
    let fixture = terminal_fixture();
    let cancel = json!({"type": "cancel", "commandId": "cancel-existing",
        "occurredAt": "2026-07-20T12:00:00Z"});
    let timer = merged(
        fixture["timer"].clone(),
        json!({"status": "running",
        "elapsedAtAnchorMs": 10_000, "lastIntent": cancel}),
    );
    let row = merged(
        fixture["history"].clone(),
        json!({"status": "cancelled",
        "commandId": "cancel-existing", "completedAt": null}),
    );
    (timer, row)
}

#[test]
fn duplicate_ids_win_over_the_public_overlap_error() {
    let (timer, row) = overlapping_pair();
    let now = terminal_fixture()["request"]["now"].clone();
    let reduce = |commands: Vec<Value>| json!({"canonicalTimer": timer, "history": [row], "commands": commands, "now": now});
    let twins = vec![
        command("command-dup", "timer-x", 1, 0),
        command("command-dup", "timer-y", 2, 1),
    ];
    assert_eq!(envelope("timer.reduce.v1", &reduce(vec![])), OVERLAP);
    assert_eq!(envelope("timer.reduce.v1", &reduce(twins)), DUPLICATE);
}
