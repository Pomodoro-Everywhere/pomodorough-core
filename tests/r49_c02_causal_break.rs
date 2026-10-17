use pomodorough_core::dispatch_envelope_json;
use serde_json::{Value, json};

struct Scenario {
    name: &'static str,
    start: &'static str,
    end: &'static str,
    source_at: &'static str,
    history: Vec<Value>,
    order: Vec<usize>,
    phase: &'static str,
    published_phase: &'static str,
}

fn completion(index: usize, at: &str) -> Value {
    json!({"id": format!("timer-{index}"), "timerId": format!("timer-{index}"),
        "commandId": format!("finish-{index}"), "phase": "focus", "status": "completed",
        "plannedDurationMs": 60000, "completedAt": at, "endedAt": at})
}

fn ordinary() -> Scenario {
    Scenario {
        name: "fourth source",
        start: "2026-07-20T00:00:00Z",
        end: "2026-07-21T00:00:00Z",
        source_at: "2026-07-20T12:00:00Z",
        history: (1..=4)
            .map(|n| completion(n, &format!("2026-07-20T{:02}:00:00Z", n + 8)))
            .collect(),
        order: vec![3, 2, 1, 0],
        phase: "long_break",
        published_phase: "long_break",
    }
}

fn source_timer(case: &Scenario) -> Value {
    json!({"id": "timer-4", "phase": "focus", "status": "completed",
        "plannedDurationMs": 60000, "elapsedAtAnchorMs": 60000, "anchorAt": case.source_at,
        "lastIntent": {"type": "finish", "commandId": "finish-4", "occurredAt": case.source_at}})
}

fn planner_input(case: &Scenario) -> Value {
    let projection = json!({"canonicalTimer": source_timer(case), "history": case.history});
    json!({"kind": "generatedBreak", "source": {"commandId": "finish-4", "timerId": "timer-4"},
        "canonical": projection, "optimistic": projection, "sourceFinishPending": false,
        "requireCanonical": true, "dayStart": case.start, "dayEnd": case.end})
}

fn planner_expected(eligible: bool, accepted: bool, phase: Option<&str>) -> Value {
    json!({"expired": false, "commandEligible": false, "reserveGeneratedBreak": false,
        "selectedPhase": null, "queueAutoBreak": false, "generatedBreakEligible": eligible,
        "generatedBreakPhase": phase, "sourceAlreadyAccepted": accepted})
}

fn source_command(case: &Scenario) -> Value {
    let wall = chrono::DateTime::parse_from_rfc3339(case.source_at)
        .unwrap()
        .timestamp_millis();
    json!({"id": "finish-4", "deviceId": "device-a", "deviceSequence": 1,
        "timerId": "timer-4", "type": "finish", "phase": "focus", "plannedDurationMs": 60000,
        "observedElapsedMs": 60000, "occurredAt": case.source_at, "hlcWallMs": wall, "hlcCounter": 0})
}

fn generated_start(case: &Scenario) -> Value {
    let mut command = source_command(case);
    for (key, value) in [
        ("id", json!("start-break")),
        ("timerId", json!("break-a")),
        ("type", json!("start")),
        ("phase", json!("short_break")),
        ("deviceSequence", json!(2)),
        ("hlcCounter", json!(1)),
        ("observedElapsedMs", json!(0)),
    ] {
        command[key] = value;
    }
    command
}

fn queues(commands: Value) -> Value {
    json!({"commands": commands, "taskOperations": [], "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []})
}

fn reconcile_input(case: &Scenario) -> Value {
    let server_wall = chrono::DateTime::parse_from_rfc3339(case.end)
        .unwrap()
        .timestamp_millis();
    json!({"local": queues(json!([source_command(case), generated_start(case)])),
        "sent": queues(json!([source_command(case)])), "neverSent": {"commands": ["start-break"]},
        "timerDependencies": [{"operationId": "start-break", "dependsOnOperationId": "finish-4",
            "generatedBreak": true, "sourceDayStart": case.start, "sourceDayEnd": case.end}],
        "response": {"acknowledgements": [{"commandId": "finish-4", "outcome": "applied"}],
            "taskAcknowledgements": [], "durationAcknowledgements": [], "autoStartAcknowledgements": [],
            "selectedTaskAcknowledgements": [], "revision": 1, "canonicalTimer": source_timer(case),
            "history": case.history, "tasks": [], "selectedTaskId": null,
            "durationsMs": {"focus": 60000, "short_break": 120000, "long_break": 180000},
            "autoStartBreaks": true, "serverTime": case.end,
            "serverHlcWallMs": server_wall, "serverHlcCounter": 0}})
}

fn reconcile_expected(case: &Scenario, input: &Value, accepted: bool) -> Value {
    let response = &input["response"];
    let history = projected_history(case, response);
    let mut timer = response["canonicalTimer"].clone();
    timer["anchorAt"] = json!(utc_time(case.source_at));
    let mut start = generated_start(case);
    start["phase"] = json!(case.phase);
    start["plannedDurationMs"] = response["durationsMs"][case.phase].clone();
    let pending = if accepted { json!([start]) } else { json!([]) };
    let workspace = json!({"canonicalTimer": timer, "history": history,
        "tasks": [], "durationsMs": response["durationsMs"], "autoStartBreaks": true,
        "selectedTaskId": null, "timerOutcomes": {},
        "winningOperationIds": {"tasks": {}, "durations": {}, "autoStart": null, "selectedTask": null}});
    json!({"schemaVersion": 3, "revision": 1, "pending": pending,
        "pendingTaskOperations": [], "pendingDurationOperations": [], "pendingAutoStartOperations": [],
        "pendingSelectedTaskOperations": [], "pendingTimerDependencies": [],
        "promotedTimerOperationIds": if accepted {json!(["start-break"])} else {json!([])},
        "droppedTimerOperationIds": if accepted {json!([])} else {json!(["start-break"])},
        "droppedTimerIds": if accepted {json!([])} else {json!(["break-a"])},
        "baseTimer": response["canonicalTimer"], "baseHistory": response["history"],
        "baseTasks": [], "baseDurationsMs": response["durationsMs"], "baseAutoStartBreaks": true,
        "baseSelectedTaskId": null, "timer": workspace["canonicalTimer"], "history": history,
        "tasks": [], "durationsMs": response["durationsMs"], "autoStartBreaks": true,
        "selectedTaskId": null, "projectionPending": queues(json!([])),
        "canonicalResponse": response, "workspace": workspace})
}

fn projected_history(case: &Scenario, response: &Value) -> Vec<Value> {
    case.order
        .iter()
        .map(|&index| {
            let mut row = response["history"][index].clone();
            for field in ["completedAt", "endedAt"] {
                if let Some(timestamp) = row[field].as_str() {
                    row[field] = json!(utc_time(timestamp));
                }
            }
            row
        })
        .collect()
}

fn utc_time(timestamp: &str) -> String {
    let time = chrono::DateTime::parse_from_rfc3339(timestamp)
        .unwrap()
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true);
    if let Some((whole, fraction)) = time.strip_suffix('Z').unwrap().split_once('.') {
        format!("{whole}.{}Z", fraction.trim_end_matches('0'))
    } else {
        time
    }
}

fn assert_dispatch(
    name: &str,
    operation: &str,
    input: &Value,
    expected: Value,
    published: Value,
) -> Value {
    let raw = dispatch_envelope_json(operation, &input.to_string());
    let actual: Value = serde_json::from_str(&raw).unwrap();
    if std::env::var_os("R49_C02_RECEIPTS").is_some() {
        println!(
            "RECEIPT={}",
            json!({"name": name, "operation": operation,
            "input": input.to_string(), "native": raw, "published": published})
        );
    }
    assert_eq!(
        actual,
        json!({"ok": true, "value": expected}),
        "{name}: {operation}"
    );
    actual["value"].clone()
}

fn assert_preserved(name: &str, operation: &str, input: &Value, expected: Value) -> Value {
    assert_dispatch(name, operation, input, expected.clone(), expected)
}

fn assert_preserved_error(name: &str, operation: &str, input: &Value, expected: Value) {
    let raw = dispatch_envelope_json(operation, &input.to_string());
    if std::env::var_os("R49_C02_RECEIPTS").is_some() {
        println!(
            "RECEIPT={}",
            json!({"name": name, "operation": operation, "input": input.to_string(),
                "native": raw, "publishedEnvelope": expected})
        );
    }
    assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), expected);
}

fn assert_cross_operation(case: &Scenario) {
    let rebase = reconcile_input(case);
    let expected = reconcile_expected(case, &rebase, true);
    let result = assert_preserved(case.name, "reconcile.rebase.v3", &rebase, expected);
    let plan = assert_dispatch(
        case.name,
        "timer.completionPlan.v1",
        &planner_input(case),
        planner_expected(true, true, Some(case.phase)),
        planner_expected(true, true, Some(case.published_phase)),
    );
    assert_eq!(
        plan["generatedBreakPhase"], result["pending"][0]["phase"],
        "{}",
        case.name
    );
    assert_eq!(
        result["pending"][0]["plannedDurationMs"],
        rebase["response"]["durationsMs"][case.phase]
    );
}

#[test]
fn delayed_later_focus_does_not_change_fourth_source_phase() {
    let mut case = ordinary();
    assert_cross_operation(&case);
    case.name = "delayed fifth focus";
    case.history.push(completion(5, "2026-07-20T13:00:00Z"));
    case.order.insert(0, 4);
    case.published_phase = "short_break";
    assert_cross_operation(&case);
    case.name = "reordered delayed fifth focus";
    case.history.reverse();
    case.order = vec![0, 1, 2, 3, 4];
    assert_cross_operation(&case);
}

#[test]
fn equal_time_uses_command_identity_independent_of_timestamp_text() {
    let mut case = ordinary();
    case.name = "equal time after source";
    case.history
        .push(completion(5, "2026-07-20T12:00:00+00:00"));
    case.history[4]["endedAt"] = json!("2026-07-20T12:00:00Z");
    case.order.insert(1, 4);
    case.published_phase = "short_break";
    assert_cross_operation(&case);
    case.name = "equal time before source";
    case.history[4]["commandId"] = json!("finish-0");
    case.phase = "short_break";
    assert_cross_operation(&case);
}

#[test]
fn identical_time_and_command_keys_keep_stable_order_and_exact_timer_identity() {
    let mut case = ordinary();
    case.name = "same command key after source";
    case.history.push(completion(5, case.source_at));
    case.history[4]["commandId"] = json!("finish-4");
    case.order.insert(1, 4);
    case.published_phase = "short_break";
    assert_cross_operation(&case);
    case.name = "same command key before exact source";
    case.history.swap(3, 4);
    case.order = vec![4, 3, 2, 1, 0];
    case.phase = "short_break";
    let input = reconcile_input(&case);
    let expected = reconcile_expected(&case, &input, true);
    let mut published = expected.clone();
    published["pending"][0]["phase"] = json!("long_break");
    published["pending"][0]["plannedDurationMs"] = json!(180000);
    assert_dispatch(
        case.name,
        "reconcile.rebase.v3",
        &input,
        expected,
        published,
    );
    let expected = planner_expected(true, true, Some("short_break"));
    assert_preserved(
        case.name,
        "timer.completionPlan.v1",
        &planner_input(&case),
        expected,
    );
}

#[test]
fn canonical_only_source_keeps_reconciliation_fallback_but_not_planner_eligibility() {
    let mut case = ordinary();
    case.name = "canonical-only source";
    case.history.pop();
    case.order = vec![2, 1, 0];
    let input = reconcile_input(&case);
    let mut expected = reconcile_expected(&case, &input, true);
    for path in ["/history", "/workspace/history"] {
        expected
            .pointer_mut(path)
            .unwrap()
            .as_array_mut()
            .unwrap()
            .insert(0, completion(4, case.source_at));
    }
    assert_preserved(case.name, "reconcile.rebase.v3", &input, expected);
    let expected = planner_expected(false, false, None);
    assert_preserved(
        case.name,
        "timer.completionPlan.v1",
        &planner_input(&case),
        expected,
    );
}

#[test]
fn out_of_day_source_preserves_each_operations_existing_boundary() {
    let case = ordinary();
    for (start, end) in [
        ("2026-07-20T00:00:00Z", case.source_at),
        ("2026-07-20T12:00:00.001Z", case.end),
    ] {
        let mut input = planner_input(&case);
        input["dayStart"] = json!(start);
        input["dayEnd"] = json!(end);
        let expected = planner_expected(true, true, Some("short_break"));
        assert_preserved(
            "out of day source",
            "timer.completionPlan.v1",
            &input,
            expected,
        );
        let mut rebase = reconcile_input(&case);
        rebase["timerDependencies"][0]["sourceDayStart"] = json!(start);
        rebase["timerDependencies"][0]["sourceDayEnd"] = json!(end);
        let expected = json!({"ok": false,
            "error": "invalid shared-core input: generated break source day excludes canonical completion"});
        assert_preserved_error(
            "out of day source",
            "reconcile.rebase.v3",
            &rebase,
            expected,
        );
    }
}

#[test]
fn missing_command_identity_uses_timer_identity_for_ties() {
    let mut case = ordinary();
    case.name = "natural focus tie after source";
    case.history.push(completion(5, case.source_at));
    case.history[4].as_object_mut().unwrap().remove("commandId");
    case.order.insert(1, 4);
    case.published_phase = "short_break";
    assert_cross_operation(&case);
    case.name = "natural focus tie before source";
    case.history[4]["timerId"] = json!("a-natural");
    case.history[4]["id"] = json!("a-natural");
    case.order = vec![4, 3, 2, 1, 0];
    case.phase = "short_break";
    assert_cross_operation(&case);
}

fn calendar_case(
    name: &'static str,
    start: &'static str,
    end: &'static str,
    times: [&'static str; 6],
) -> Scenario {
    Scenario {
        name,
        start,
        end,
        source_at: times[3],
        history: times
            .iter()
            .enumerate()
            .map(|(n, at)| completion(n + 1, at))
            .collect(),
        order: vec![5, 4, 3, 2, 1, 0],
        phase: "long_break",
        published_phase: "short_break",
    }
}

#[test]
fn midnight_and_dst_days_keep_half_open_source_day_bounds() {
    for case in [
        calendar_case(
            "midnight source",
            "2026-07-20T00:00:00Z",
            "2026-07-21T00:00:00Z",
            [
                "2026-07-20T00:00:00Z",
                "2026-07-20T01:00:00Z",
                "2026-07-20T02:00:00Z",
                "2026-07-20T23:59:59.999Z",
                "2026-07-20T23:59:59.9999Z",
                "2026-07-21T00:00:00Z",
            ],
        ),
        calendar_case(
            "23-hour DST source day",
            "2026-03-08T00:00:00-05:00",
            "2026-03-09T00:00:00-04:00",
            [
                "2026-03-08T00:00:00-05:00",
                "2026-03-08T01:30:00-05:00",
                "2026-03-08T03:00:00-04:00",
                "2026-03-08T23:58:00-04:00",
                "2026-03-08T23:59:00-04:00",
                "2026-03-09T00:00:00-04:00",
            ],
        ),
        calendar_case(
            "25-hour DST source day",
            "2026-11-01T00:00:00-04:00",
            "2026-11-02T00:00:00-05:00",
            [
                "2026-11-01T00:00:00-04:00",
                "2026-11-01T01:30:00-04:00",
                "2026-11-01T01:30:00-05:00",
                "2026-11-01T23:58:00-05:00",
                "2026-11-01T23:59:00-05:00",
                "2026-11-02T00:00:00-05:00",
            ],
        ),
    ] {
        assert_cross_operation(&case);
    }
}

#[test]
fn missing_or_wrong_source_identity_never_authorizes_planner() {
    let case = ordinary();
    for (field, value) in [
        ("commandId", json!("not-finish-4")),
        ("timerId", json!("not-timer-4")),
    ] {
        let mut input = planner_input(&case);
        input["source"][field] = value;
        let expected = planner_expected(false, false, None);
        assert_preserved(field, "timer.completionPlan.v1", &input, expected);
    }
    assert_missing_source_identity(&case);
}

fn assert_missing_source_identity(case: &Scenario) {
    let mut plan = planner_input(case);
    let mut rebase = reconcile_input(case);
    for projection in ["canonical", "optimistic"] {
        plan[projection]["history"][3]
            .as_object_mut()
            .unwrap()
            .remove("commandId");
        plan[projection]["canonicalTimer"]
            .as_object_mut()
            .unwrap()
            .remove("lastIntent");
    }
    rebase["response"]["history"][3]
        .as_object_mut()
        .unwrap()
        .remove("commandId");
    rebase["response"]["canonicalTimer"]
        .as_object_mut()
        .unwrap()
        .remove("lastIntent");
    let expected = planner_expected(false, false, None);
    assert_preserved(
        "missing source identity",
        "timer.completionPlan.v1",
        &plan,
        expected,
    );
    let expected = reconcile_expected(case, &rebase, false);
    assert_preserved(
        "missing source identity",
        "reconcile.rebase.v3",
        &rebase,
        expected,
    );
}

#[test]
fn pending_source_uses_optimistic_history_but_accepted_source_uses_canonical() {
    let mut case = ordinary();
    case.history.push(completion(5, "2026-07-20T13:00:00Z"));
    let mut input = planner_input(&case);
    input["requireCanonical"] = json!(false);
    input["sourceFinishPending"] = json!(true);
    input["canonical"] = json!({"canonicalTimer": null, "history": []});
    assert_dispatch(
        "pending optimistic source",
        "timer.completionPlan.v1",
        &input,
        planner_expected(true, false, Some("long_break")),
        planner_expected(true, false, Some("short_break")),
    );
    input["requireCanonical"] = json!(true);
    let expected = planner_expected(false, false, None);
    assert_preserved(
        "required missing canonical source",
        "timer.completionPlan.v1",
        &input,
        expected,
    );
    input = planner_input(&case);
    input["optimistic"] = json!({"canonicalTimer": null, "history": []});
    assert_dispatch(
        "accepted canonical source",
        "timer.completionPlan.v1",
        &input,
        planner_expected(true, true, Some("long_break")),
        planner_expected(true, true, Some("short_break")),
    );
}
