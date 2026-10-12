use pomodorough_core::dispatch_envelope_json;
use serde_json::{Value, json};
use std::panic::catch_unwind;

const OPERATION: &str = "workspace.completionMutation.v1";

fn natural_request(index: usize, stage: &str) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/pwa-natural-completion-v1.json")).unwrap();
    let mut input = fixture["receipts"][index]["request"]["input"].clone();
    input["stage"] = json!(stage);
    input
}

fn envelope(input: &Value) -> Value {
    let result = catch_unwind(|| dispatch_envelope_json(OPERATION, &input.to_string()));
    serde_json::from_str(&result.expect("completion dispatcher must not unwind")).unwrap()
}

fn error(message: &str) -> Value {
    json!({"ok": false, "error": message})
}

fn omit(input: &mut Value, parent: &str, field: &str) {
    assert!(
        input
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field)
            .is_some(),
        "fixture must contain {parent}/{field}"
    );
}

fn assert_omission(input: &Value, parent: &str, field: &str, message: &str) {
    let expected_valid = envelope(input);
    assert_eq!(expected_valid["ok"], true);
    assert_eq!(expected_valid["value"]["outcome"], "planned");
    assert_eq!(expected_valid["value"]["commands"][0]["type"], "finish");
    let mut malformed = input.clone();
    omit(&mut malformed, parent, field);
    let rejected = catch_unwind(|| dispatch_envelope_json(OPERATION, &malformed.to_string()));
    assert_eq!(
        envelope(input),
        expected_valid,
        "valid subsequent call changed"
    );
    let rejected = rejected.unwrap_or_else(|_| panic!("{parent}/{field} unwound"));
    assert_eq!(
        serde_json::from_str::<Value>(&rejected).unwrap(),
        error(message)
    );
}

#[test]
fn omitted_local_returns_error_without_unwind_then_valid_finish() {
    for index in 0..4 {
        for stage in ["finishCommit", "automaticFinishCommit"] {
            assert_omission(
                &natural_request(index, stage),
                "/workspace",
                "local",
                "invalid shared-core input: local must be a JSON object",
            );
        }
    }
}

#[test]
fn omitted_commands_returns_error_without_unwind_then_valid_finish() {
    for index in 0..4 {
        for stage in ["finishCommit", "automaticFinishCommit"] {
            assert_omission(
                &natural_request(index, stage),
                "/workspace/local",
                "commands",
                "invalid shared-core input: local.commands must be a JSON array",
            );
        }
    }
}

const REQUIRED_FIELDS: &[(&str, &str, &str)] = &[
    (
        "/workspace",
        "base",
        "invalid shared-core input: base must be a JSON object",
    ),
    (
        "/workspace",
        "canonicalHead",
        "missing required projection value: canonicalHead",
    ),
    (
        "/workspace",
        "timerDependencies",
        "invalid shared-core input: timerDependencies must be a JSON array",
    ),
    (
        "/workspace/base",
        "canonicalTimer",
        "invalid shared-core input: missing base.canonicalTimer",
    ),
    (
        "/workspace/base",
        "history",
        "invalid shared-core input: missing base.history",
    ),
    (
        "/workspace/base",
        "tasks",
        "invalid shared-core input: missing base.tasks",
    ),
    (
        "/workspace/base",
        "durationsMs",
        "invalid shared-core input: missing base.durationsMs",
    ),
    (
        "/workspace/base",
        "autoStartBreaks",
        "invalid shared-core input: missing base.autoStartBreaks",
    ),
    (
        "/workspace/base",
        "selectedTaskId",
        "invalid shared-core input: missing base.selectedTaskId",
    ),
    (
        "/workspace/local",
        "taskOperations",
        "invalid shared-core input: local.taskOperations must be a JSON array",
    ),
    (
        "/workspace/local",
        "durationOperations",
        "invalid shared-core input: local.durationOperations must be a JSON array",
    ),
    (
        "/workspace/local",
        "autoStartOperations",
        "invalid shared-core input: local.autoStartOperations must be a JSON array",
    ),
    (
        "/workspace/local",
        "selectedTaskOperations",
        "invalid shared-core input: local.selectedTaskOperations must be a JSON array",
    ),
];

#[test]
fn omitted_related_workspace_fields_return_error_only_envelopes() {
    for stage in ["finishCommit", "automaticFinishCommit"] {
        for (parent, field, message) in REQUIRED_FIELDS {
            assert_omission(&natural_request(0, stage), parent, field, message);
        }
    }
}

const OBSERVATION_ERRORS: &[(&str, &str, &str)] = &[
    (
        "/clock/occurredAt",
        r#""bad-clock""#,
        "invalid RFC 3339 timestamp: bad-clock",
    ),
    (
        "/calendarIntervals/0/end",
        r#""1970-01-01T00:00:00Z""#,
        "invalid shared-core input: invalid completion calendar interval",
    ),
    (
        "/clock/physicalNow",
        r#""2026-08-31T12:24:59Z""#,
        "invalid shared-core input: observation is later than physical now",
    ),
    (
        "/boundaryRetry",
        r#"{"originalObservedAt":"2026-08-31T12:25:00Z","measuredElapsedMs":1500000}"#,
        "invalid shared-core input: invalid Apple millisecond boundary retry",
    ),
    (
        "/ownership/deviceId",
        r#""""#,
        "invalid shared-core input: invalid completion ownership",
    ),
    (
        "/leaseNowMs",
        "-1",
        "invalid shared-core input: invalid completion lease context",
    ),
];

#[test]
fn independent_observation_errors_keep_precedence_over_missing_local() {
    let original = natural_request(0, "finishCommit");
    for &(path, raw, message) in OBSERVATION_ERRORS {
        let mut input = original.clone();
        let value = serde_json::from_str(raw).unwrap();
        if path == "/boundaryRetry" {
            input["boundaryRetry"] = value;
        } else {
            *input.pointer_mut(path).unwrap() = value;
        }
        let expected = error(message);
        assert_eq!(envelope(&input), expected, "{path}");
        omit(&mut input, "/workspace", "local");
        assert_eq!(envelope(&input), expected, "{path} with missing local");
    }
}

#[test]
fn observation_semantics_keep_precedence_over_workspace_payload_errors() {
    let mut input = natural_request(0, "finishCommit");
    input["workspace"]["base"]["autoStartBreaks"] = json!("invalid");
    input["observation"]["commandTimes"] = json!({"absent-command": input["clock"]["physicalNow"]});
    assert_eq!(
        envelope(&input),
        error("invalid shared-core input: physical command time without retained command")
    );
    input["observation"]["commandTimes"] = json!({});
    input["workspace"]["base"]["canonicalTimer"] = Value::Null;
    assert_eq!(
        envelope(&input),
        error("invalid shared-core input: physical anchor without canonical timer")
    );
}

#[test]
fn required_structure_precedes_workspace_dependent_observation_checks() {
    let mut input = natural_request(0, "finishCommit");
    input["observation"]["commandTimes"] = json!({"absent-command": input["clock"]["physicalNow"]});
    omit(&mut input, "/workspace/local", "commands");
    assert_eq!(
        envelope(&input),
        error("invalid shared-core input: local.commands must be a JSON array")
    );
    let mut input = natural_request(0, "finishCommit");
    omit(&mut input, "/workspace/base", "canonicalTimer");
    assert_eq!(
        envelope(&input),
        error("invalid shared-core input: missing base.canonicalTimer")
    );
}

#[test]
fn valid_owner_denial_noop_survives_malformed_sibling() {
    for stage in ["finishCommit", "automaticFinishCommit"] {
        let mut input = natural_request(0, stage);
        input["ownership"]["deviceId"] = json!("foreign-device");
        let expected = envelope(&input);
        assert_eq!(expected["ok"], true);
        assert_eq!(expected["value"]["outcome"], "noop");
        assert_eq!(expected["value"]["reason"], "not_owner");
        for field in ["workspace", "allocation", "selection", "observation"] {
            assert_eq!(expected["value"][field], input[field], "{field}");
        }
        for field in [
            "commands",
            "atomicCommandIds",
            "ownershipWrites",
            "effectsAfterCommit",
        ] {
            assert_eq!(expected["value"][field], json!([]), "{field}");
        }
        let mut malformed = input.clone();
        omit(&mut malformed, "/workspace/local", "commands");
        let rejected = catch_unwind(|| dispatch_envelope_json(OPERATION, &malformed.to_string()));
        assert_eq!(envelope(&input), expected);
        assert_eq!(
            serde_json::from_str::<Value>(&rejected.expect("malformed sibling must not unwind"))
                .unwrap(),
            error("invalid shared-core input: local.commands must be a JSON array")
        );
    }
}
