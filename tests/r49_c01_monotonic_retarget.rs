use pomodorough_core::dispatch_envelope_json;
use serde_json::{Value, json};

const BASE_WALL: &str = "2026-07-20T12:00:10Z";
const ORDINARY_WALL: &str = "2026-07-20T12:00:11Z";
const JUMP_WALL: &str = "2026-07-20T12:01:20Z";
const TASK_ID: &str = "111db2c9-a2cf-8af7-beb4-cbd9fd252b6b";
const CONTINUITY: &str = "browser-session-1";

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap()
}

fn queues() -> Value {
    json!({"commands": [], "taskOperations": [], "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []})
}

fn request(prior: Option<&Value>, intent: Value, at: &str, monotonic: f64) -> Value {
    let mut input = fixture()["request"].clone();
    input["compatibility"] = json!("pwaStorage");
    input["intent"] = intent;
    input["workspace"]["base"]["canonicalTimer"] = fixture()["timer"].clone();
    if let Some(prior) = prior {
        for field in ["workspace", "allocation", "observation"] {
            input[field] = prior[field].clone();
        }
    }
    input["ownership"] = json!({"ownerId": "account-a", "expectedOwnerId": "account-a"});
    input["durability"] = json!({"outgoingDurationOperationIds": [], "localTabId": "tab-a"});
    for field in ["occurredAt", "physicalNow", "observedAt"] {
        input["clock"][field] = json!(at);
    }
    input["clock"]["monotonicNowMs"] = json!(monotonic);
    input["clock"]["continuityId"] = json!(CONTINUITY);
    let wall = chrono::DateTime::parse_from_rfc3339(at)
        .unwrap()
        .timestamp_millis();
    let prefix = format!("{wall:012x}");
    input["identities"]["commandUuids"] = json!(
        (1..=3)
            .map(|index| { format!("{}-{}-7000-8000-{index:012x}", &prefix[..8], &prefix[8..]) })
            .collect::<Vec<_>>()
    );
    input
}

fn dispatch(operation: &str, input: &Value) -> Value {
    if let Ok(oracle) = std::env::var("R49_C01_BASELINE_ORACLE") {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut command = if oracle.ends_with(".mjs") {
            let mut command = Command::new("node");
            command.arg(oracle);
            command
        } else {
            Command::new(oracle)
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(
            child.stdin.take().unwrap(),
            "{}",
            json!({
                "operation": operation, "input": input.to_string()
            })
        )
        .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        return serde_json::from_slice(&output.stdout).unwrap();
    }
    serde_json::from_str(&dispatch_envelope_json(operation, &input.to_string())).unwrap()
}

fn envelope(input: &Value) -> Value {
    dispatch("workspace.intent.v1", input)
}

fn operation(input: &Value, index: usize, counter: i64, fields: Value) -> Value {
    let mut operation = fields;
    operation["id"] = input["identities"]["commandUuids"][index].clone();
    operation["deviceId"] = json!("device-local");
    operation["occurredAt"] = json!(format!(
        "{}.000Z",
        input["clock"]["occurredAt"]
            .as_str()
            .unwrap()
            .trim_end_matches('Z')
    ));
    operation["hlcWallMs"] = json!(
        chrono::DateTime::parse_from_rfc3339(input["clock"]["occurredAt"].as_str().unwrap())
            .unwrap()
            .timestamp_millis()
    );
    operation["hlcCounter"] = json!(counter);
    operation
}

fn expected(
    input: &Value,
    operations: Value,
    projection: Value,
    observation: Value,
    timer_observation: Value,
    allocation: Value,
) -> Value {
    let mut workspace = input["workspace"].clone();
    let mut ids = queues();
    let mut outcomes = queues();
    append_expected(&mut workspace, &operations, &mut ids, &mut outcomes);
    json!({"ok": true, "value": {
        "schemaVersion": 1, "outcome": "planned", "reason": "", "workspace": workspace,
        "selection": input["selection"], "allocation": allocation, "observation": observation,
        "commands": operations["commands"], "atomicCommandIds": ids["commands"],
        "operations": operations, "durableOperations": operations, "atomicOperationIds": ids,
        "retiredDurationOperationIds": [], "groupOutcomes": outcomes, "ownershipWrites": [],
        "projection": projection, "timerObservation": timer_observation,
        "effectsAfterCommit": [{"kind": "launchSync"}]
    }})
}

fn append_expected(
    workspace: &mut Value,
    operations: &Value,
    ids: &mut Value,
    outcomes: &mut Value,
) {
    for (domain, operations) in operations.as_object().unwrap() {
        for operation in operations.as_array().unwrap() {
            workspace["local"][domain]
                .as_array_mut()
                .unwrap()
                .push(operation.clone());
            if workspace["neverSent"].get(domain).is_none() {
                workspace["neverSent"][domain] = json!([]);
            }
            workspace["neverSent"][domain]
                .as_array_mut()
                .unwrap()
                .push(operation["id"].clone());
            ids[domain]
                .as_array_mut()
                .unwrap()
                .push(operation["id"].clone());
            outcomes[domain]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": operation["id"], "outcome": "applied"}));
        }
    }
}

fn allocation(input: &Value, index: usize, counter: i64, sequence: i64) -> Value {
    json!({"deviceId": "device-local", "deviceSequence": sequence,
        "hlc": {"wallMs": chrono::DateTime::parse_from_rfc3339(input["clock"]["occurredAt"].as_str().unwrap()).unwrap().timestamp_millis(), "counter": counter},
        "lastUuid": input["identities"]["commandUuids"][index]})
}

fn observation(elapsed: i64, remaining: i64, deadline: Value) -> Value {
    json!({"timerId": "existing-timer", "elapsedMs": elapsed, "remainingMs": remaining,
        "deadlineAt": deadline})
}

fn seed() -> Value {
    let input = request(
        None,
        json!({"kind": "setAutoStart", "enabled": true}),
        BASE_WALL,
        100.0,
    );
    let mut operations = queues();
    operations["autoStartOperations"] = json!([operation(&input, 0, 0, json!({"enabled": true}))]);
    let projection = json!({"canonicalTimer": fixture()["timer"], "history": [], "tasks": [],
        "selectedTaskId": null, "durationsMs": input["workspace"]["base"]["durationsMs"],
        "autoStartBreaks": true, "timerOutcomes": {}, "winningOperationIds": {
            "autoStart": operations["autoStartOperations"][0]["id"], "durations": {},
            "selectedTask": null, "tasks": {}}});
    let sampled = json!({"canonicalAnchorAt": null, "commandTimes": {}, "monotonicAnchor": {
        "timerId": "existing-timer", "anchorAt": "2026-07-20T12:00:00Z", "elapsedAtAnchorMs": 5000,
        "sampledTrustedNowMs": 1784548810000_i64, "sampledMonotonicMs": 100.0,
        "continuityId": CONTINUITY}});
    let expected = expected(
        &input,
        operations,
        projection,
        sampled,
        observation(15000, 45000, json!("2026-07-20T12:00:55.000Z")),
        allocation(&input, 0, 0, 7),
    );
    let actual = envelope(&input);
    assert_eq!(actual, expected, "complete anchor seed");
    actual["value"].clone()
}

fn task_operations(input: &Value) -> Value {
    let mut operations = queues();
    operations["taskOperations"] = json!([operation(
        input,
        0,
        0,
        json!({"taskId": TASK_ID, "type": "upsert", "title": "new task"})
    )]);
    operations
}

fn task_projection(prior: &Value, operations: &Value) -> Value {
    let mut projection = prior["projection"].clone();
    projection["tasks"] = json!([{"id": TASK_ID, "title": "new task"}]);
    projection["winningOperationIds"]["tasks"][TASK_ID] =
        operations["taskOperations"][0]["id"].clone();
    projection
}

fn selected_expected(
    prior: &Value,
    input: &Value,
    mut operations: Value,
    mut projection: Value,
    first_index: usize,
) -> Value {
    operations["selectedTaskOperations"] = json!([operation(
        input,
        first_index,
        first_index as i64,
        json!({"taskId": TASK_ID})
    )]);
    let index = first_index + 1;
    let command = operation(
        input,
        index,
        index as i64,
        json!({"type": "retarget", "taskId": TASK_ID,
        "deviceSequence": 8, "timerId": "existing-timer", "phase": "focus",
        "plannedDurationMs": 60000, "observedElapsedMs": 16000}),
    );
    operations["commands"] = json!([command]);
    projection["canonicalTimer"]["taskId"] = json!(TASK_ID);
    projection["selectedTaskId"] = json!(TASK_ID);
    projection["timerOutcomes"][command["id"].as_str().unwrap()] =
        json!({"outcome": "applied", "reason": ""});
    projection["winningOperationIds"]["selectedTask"] =
        operations["selectedTaskOperations"][0]["id"].clone();
    let mut sampled = prior["observation"].clone();
    sampled["commandTimes"][command["id"].as_str().unwrap()] =
        input["clock"]["physicalNow"].clone();
    expected(
        input,
        operations,
        projection,
        sampled,
        observation(16000, 44000, json!("2026-07-20T12:00:55.000Z")),
        allocation(input, index, index as i64, 8),
    )
}

fn assert_replay(output: &Value, now: &str, projection: &Value) {
    let mut input = output["workspace"].clone();
    input["now"] = json!(now);
    assert_eq!(
        dispatch("workspace.project.v1", &input),
        json!({"ok": true, "value": {
            "workspace": projection, "projectionPending": input["local"]
        }})
    );
}

fn assert_live_read(output: &Value) {
    let input = json!({"profile": "pwaStorage", "source": {
        "kind": "workspace", "value": output["workspace"]}, "selectedPhase": "focus",
        "observedAt": JUMP_WALL, "calendarIntervals": fixture()["request"]["calendarIntervals"],
        "monotonic": {"nowMs": 1100.0, "continuityId": CONTINUITY,
            "anchor": output["observation"]["monotonicAnchor"]}});
    let progress = 16000.0 / 60000.0;
    assert_eq!(
        dispatch("workspace.readModel.v1", &input),
        json!({"ok": true, "value": {
            "schemaVersion": 1, "canonical": {"timerId": "existing-timer", "phase": "focus",
                "status": "running", "plannedDurationMs": 60000, "elapsedMs": 16000.0,
                "remainingMs": 44000.0, "deadlineAt": "2026-07-20T12:00:55Z",
                "progress": progress, "observedElapsedMs": 16000},
            "display": {"phase": "focus", "status": "running", "plannedDurationMs": 60000,
                "elapsedMs": 16000.0, "remainingMs": 44000.0, "progress": progress,
                "remainingSecondsCeil": 44},
            "availableIntents": ["pause", "finish", "cancel", "cancelAndClear"],
            "cadence": {"completedFocusToday": 0, "completedFocusTodayPlannedDurationMs": 0,
                "completedFocusTotal": 0, "longBreakProgress": 0,
                "nextCompletedFocusBreakPhase": "short_break", "skipDestination": "short_break"},
            "tasks": {"total": 1, "selectedTaskId": TASK_ID,
                "completedFocusTodayByTask": {TASK_ID: {"count": 0, "plannedDurationMs": 0}}}
        }})
    );
}

#[test]
fn add_and_select_keeps_live_monotonic_timer_and_complete_atomic_group_after_wall_jump() {
    let prior = seed();
    for wall in [ORDINARY_WALL, JUMP_WALL] {
        let input = request(
            Some(&prior),
            json!({"kind": "addAndSelectTask", "title": "new task"}),
            wall,
            1100.0,
        );
        let operations = task_operations(&input);
        let projection = task_projection(&prior, &operations);
        let expected = selected_expected(&prior, &input, operations, projection, 1);
        let actual = envelope(&input);
        assert_eq!(actual["ok"], true, "{wall}: {actual}");
        assert_eq!(actual, expected, "{wall}");
        if wall == ORDINARY_WALL {
            assert_replay(
                &actual["value"],
                ORDINARY_WALL,
                &expected["value"]["projection"],
            );
        }
        assert_live_read(&actual["value"]);
    }
}

#[test]
fn select_existing_task_keeps_live_monotonic_retarget_after_wall_jump() {
    let mut prior = seed();
    let task = json!([{"id": TASK_ID, "title": "new task"}]);
    prior["workspace"]["base"]["tasks"] = task.clone();
    prior["projection"]["tasks"] = task;
    for wall in [ORDINARY_WALL, JUMP_WALL] {
        let input = request(
            Some(&prior),
            json!({"kind": "selectTask", "taskId": TASK_ID}),
            wall,
            1100.0,
        );
        let actual = envelope(&input);
        assert_eq!(actual["ok"], true, "{wall}: {actual}");
        assert_eq!(
            actual,
            selected_expected(&prior, &input, queues(), prior["projection"].clone(), 0),
            "{wall}"
        );
    }
}

#[test]
fn no_command_upsert_preserves_r48_monotonic_timing_after_wall_jump() {
    let prior = seed();
    let input = request(
        Some(&prior),
        json!({"kind": "upsertTask", "title": "new task"}),
        JUMP_WALL,
        1100.0,
    );
    let operations = task_operations(&input);
    let projection = task_projection(&prior, &operations);
    assert_eq!(
        envelope(&input),
        expected(
            &input,
            operations,
            projection,
            prior["observation"].clone(),
            observation(16000, 44000, json!("2026-07-20T12:00:55.000Z")),
            allocation(&input, 0, 0, 7)
        )
    );
}

fn expired_expected(prior: &Value, input: &Value) -> Value {
    let mut operations = task_operations(input);
    operations["selectedTaskOperations"] =
        json!([operation(input, 1, 1, json!({"taskId": TASK_ID}))]);
    let mut projection = task_projection(prior, &operations);
    projection["selectedTaskId"] = json!(TASK_ID);
    projection["winningOperationIds"]["selectedTask"] =
        operations["selectedTaskOperations"][0]["id"].clone();
    projection["canonicalTimer"]["status"] = json!("completed");
    projection["canonicalTimer"]["elapsedAtAnchorMs"] = json!(60000);
    projection["canonicalTimer"]["anchorAt"] = json!("2026-07-20T12:00:55Z");
    projection["history"] = json!([{"id": "existing-timer", "timerId": "existing-timer", "phase": "focus",
        "status": "completed", "plannedDurationMs": 60000, "completedAt": "2026-07-20T12:00:55Z",
        "endedAt": "2026-07-20T12:00:55Z"}]);
    expected(
        input,
        operations,
        projection,
        json!({"canonicalAnchorAt": null, "commandTimes": {}}),
        observation(60000, 0, Value::Null),
        allocation(input, 1, 1, 7),
    )
}

#[test]
fn genuine_expiry_or_lost_continuity_completes_without_retarget() {
    let prior = seed();
    for control in [
        "expired",
        "wall-only",
        "changed-continuity",
        "backwards-monotonic",
    ] {
        let mut input = request(
            Some(&prior),
            json!({"kind": "addAndSelectTask", "title": "new task"}),
            JUMP_WALL,
            1100.0,
        );
        match control {
            "expired" => input["clock"]["monotonicNowMs"] = json!(60100.0),
            "wall-only" => {
                input["clock"]
                    .as_object_mut()
                    .unwrap()
                    .remove("monotonicNowMs");
            }
            "changed-continuity" => input["clock"]["continuityId"] = json!("browser-session-2"),
            _ => input["clock"]["monotonicNowMs"] = json!(50.0),
        }
        assert_eq!(
            envelope(&input),
            expired_expected(&prior, &input),
            "{control}"
        );
    }
}

#[test]
fn retained_cancel_still_blocks_atomic_retarget_and_invalid_proof_fails_closed() {
    let prior = seed();
    let mut input = request(
        Some(&prior),
        json!({"kind": "addAndSelectTask", "title": "new task"}),
        JUMP_WALL,
        1100.0,
    );
    let mut cancel = operation(
        &input,
        0,
        0,
        json!({"type": "cancel", "timerId": "existing-timer",
        "phase": "focus", "plannedDurationMs": 60000, "observedElapsedMs": 16000, "deviceSequence": 8}),
    );
    cancel["id"] = json!("claimed-cancel");
    input["workspace"]["local"]["commands"] = json!([cancel]);
    assert_eq!(
        envelope(&input),
        json!({"ok": false,
        "error": "invalid shared-core input: workspace group failed retained-ledger admission"})
    );
    input["workspace"]["local"]["commands"] = json!([]);
    input["workspace"]["neverSent"]["commands"] = json!(["absent-command"]);
    assert_eq!(
        envelope(&input),
        json!({"ok": false,
        "error": "invalid shared-core input: invalid neverSent identity or delivery claim"})
    );
}

#[test]
fn retarget_does_not_hide_real_deadline_or_resurrect_inactive_timer() {
    let prior = seed();
    let input = request(
        Some(&prior),
        json!({"kind": "addAndSelectTask", "title": "new task"}),
        ORDINARY_WALL,
        1100.0,
    );
    let actual = envelope(&input);
    assert_eq!(actual["ok"], true);
    let selected = actual["value"].clone();
    let mut completed = selected["projection"].clone();
    let terminal = expired_expected(&prior, &input)["value"]["projection"].clone();
    completed["canonicalTimer"] = terminal["canonicalTimer"].clone();
    completed["canonicalTimer"]["taskId"] = json!(TASK_ID);
    completed["history"] = terminal["history"].clone();
    completed["history"][0]["taskId"] = json!(TASK_ID);
    assert_replay(&selected, JUMP_WALL, &completed);
    let mut inactive = selected.clone();
    inactive["workspace"]["base"]["canonicalTimer"] = terminal["canonicalTimer"].clone();
    inactive["workspace"]["base"]["history"] = terminal["history"].clone();
    completed["canonicalTimer"] = terminal["canonicalTimer"].clone();
    completed["history"] = terminal["history"].clone();
    let id = selected["commands"][0]["id"].as_str().unwrap();
    completed["timerOutcomes"][id] = json!({"outcome": "ignored",
        "reason": "timer is not the active focus timer"});
    assert_replay(&inactive, ORDINARY_WALL, &completed);
}

#[test]
fn fractional_live_retarget_does_not_use_rounded_wire_elapsed_for_expiry() {
    let prior = seed();
    let input = request(
        Some(&prior),
        json!({"kind": "addAndSelectTask", "title": "new task"}),
        JUMP_WALL,
        45099.5,
    );
    let operations = task_operations(&input);
    let projection = task_projection(&prior, &operations);
    let mut expected = selected_expected(&prior, &input, operations, projection, 1);
    for path in [
        "/value/commands/0",
        "/value/operations/commands/0",
        "/value/durableOperations/commands/0",
        "/value/workspace/local/commands/0",
    ] {
        expected.pointer_mut(path).unwrap()["observedElapsedMs"] = json!(60000);
    }
    expected["value"]["timerObservation"]["elapsedMs"] = json!(59999.5);
    expected["value"]["timerObservation"]["remainingMs"] = json!(0.5);
    let actual = envelope(&input);
    assert_eq!(actual["ok"], true, "{actual}");
    assert_eq!(actual, expected);
}

#[test]
fn returned_retarget_workspace_keeps_monotonic_timing_on_next_mutation() {
    let prior = seed();
    let input = request(
        Some(&prior),
        json!({"kind": "addAndSelectTask", "title": "new task"}),
        JUMP_WALL,
        1100.0,
    );
    let selected = envelope(&input);
    assert_eq!(selected["ok"], true, "{selected}");
    let prior = &selected["value"];
    let next = request(
        Some(prior),
        json!({"kind": "upsertTask", "title": "new task"}),
        "2026-07-20T12:01:21Z",
        2100.0,
    );
    let operations = task_operations(&next);
    let projection = task_projection(prior, &operations);
    assert_eq!(
        envelope(&next),
        expected(
            &next,
            operations,
            projection,
            prior["observation"].clone(),
            observation(17000, 43000, json!("2026-07-20T12:00:55.000Z")),
            allocation(&next, 0, 0, 8)
        )
    );
}

#[test]
fn standalone_retarget_keeps_occurrence_expiry_and_page_error_precedence() {
    let input = request(
        None,
        json!({"kind": "selectTask", "taskId": TASK_ID}),
        JUMP_WALL,
        1100.0,
    );
    let command = operation(
        &input,
        1,
        1,
        json!({"type": "retarget", "taskId": TASK_ID,
        "deviceSequence": 8, "timerId": "existing-timer", "phase": "focus",
        "plannedDurationMs": 60000, "observedElapsedMs": 16000}),
    );
    let session = replay_session();
    let mut projected_session = session.clone();
    projected_session["status"] = json!("completed");
    projected_session["elapsedAtAnchorMs"] = json!(60000);
    projected_session["anchorAt"] = json!("2026-07-20T12:00:55Z");
    projected_session["endedAt"] = json!("2026-07-20T12:00:55Z");
    let terminal = expired_expected(&seed(), &input)["value"]["projection"].clone();
    let reduction = json!({"canonicalTimer": terminal["canonicalTimer"], "history": terminal["history"],
        "sessions": [projected_session], "outcomes": {command["id"].as_str().unwrap(): {
            "outcome": "ignored", "reason": "timer is not the active focus timer"}}});
    assert_full_replay(&command, &reduction);
    let mut page = json!({"sessions": [session], "currentTimerId": "existing-timer", "after": null,
        "commands": [command], "now": ORDINARY_WALL});
    let mut expected = reduction;
    expected["currentTimerId"] = json!("existing-timer");
    expected["after"] = json!({"hlcWallMs": command["hlcWallMs"], "hlcCounter": 1,
        "deviceId": "device-local", "id": command["id"]});
    assert_eq!(
        dispatch("timer.replay.page.v1", &page),
        json!({"ok": true, "value": expected})
    );
    assert_page_error_precedence(&mut page);
}

fn replay_session() -> Value {
    json!({"timerId": "existing-timer", "phase": "focus", "status": "running",
        "plannedDurationMs": 60000, "elapsedAtAnchorMs": 5000,
        "anchorAt": "2026-07-20T12:00:00Z", "startedAt": "2026-07-20T12:00:00Z",
        "startedByDeviceId": "device-local", "lastCommandId": ""})
}

fn assert_full_replay(command: &Value, expected: &Value) {
    let input =
        json!({"canonicalTimer": fixture()["timer"], "commands": [command], "now": ORDINARY_WALL});
    assert_eq!(
        dispatch("timer.reduce.v1", &input),
        json!({"ok": true, "value": expected})
    );
}

fn assert_page_error_precedence(page: &mut Value) {
    page["now"] = json!("invalid-time");
    page["commands"][0]["deviceSequence"] = json!(0);
    assert_eq!(
        dispatch("timer.replay.page.v1", page),
        json!({"ok": false,
        "error": "invalid shared-core input: invalid timer command"})
    );
}

#[test]
fn returned_retarget_requires_matching_live_evidence_and_raw_projection_keeps_release_contract() {
    let prior = seed();
    let input = request(
        Some(&prior),
        json!({"kind": "addAndSelectTask", "title": "new task"}),
        JUMP_WALL,
        1100.0,
    );
    let selected = envelope(&input);
    assert_eq!(selected["ok"], true);
    let output = &selected["value"];
    let mut raw_projection = output["projection"].clone();
    let terminal = expired_expected(&prior, &input)["value"]["projection"].clone();
    raw_projection["canonicalTimer"] = terminal["canonicalTimer"].clone();
    raw_projection["history"] = terminal["history"].clone();
    let id = output["commands"][0]["id"].as_str().unwrap();
    raw_projection["timerOutcomes"][id] =
        json!({"outcome": "ignored", "reason": "timer is not the active focus timer"});
    assert_replay(output, ORDINARY_WALL, &raw_projection);
    for invalid in ["no-sample", "wrong-continuity", "backwards", "stale-anchor"] {
        let mut read = json!({"profile": "pwaStorage", "source": {"kind": "workspace", "value": output["workspace"]},
            "selectedPhase": "focus", "observedAt": JUMP_WALL, "calendarIntervals": fixture()["request"]["calendarIntervals"],
            "monotonic": {"nowMs": 1100.0, "continuityId": CONTINUITY, "anchor": output["observation"]["monotonicAnchor"]}});
        match invalid {
            "no-sample" => {
                read.as_object_mut().unwrap().remove("monotonic");
            }
            "wrong-continuity" => read["monotonic"]["continuityId"] = json!("other-session"),
            "backwards" => read["monotonic"]["nowMs"] = json!(50.0),
            _ => read["monotonic"]["anchor"]["anchorAt"] = json!("2026-07-20T11:59:59Z"),
        }
        let actual = dispatch("workspace.readModel.v1", &read);
        assert_eq!(actual["ok"], true, "{invalid}: {actual}");
        assert_eq!(
            actual["value"]["canonical"]["status"], "completed",
            "{invalid}"
        );
    }
}
