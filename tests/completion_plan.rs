use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const START: &str = "2026-08-25T00:00:00.000Z";
const END: &str = "2026-08-26T00:00:00.000Z";

fn timer(id: &str, phase: &str, status: &str) -> Value {
    json!({
        "id": id,
        "phase": phase,
        "status": status,
        "plannedDurationMs": 1_500_000,
        "elapsedAtAnchorMs": if status == "completed" { 1_500_000 } else { 0 },
        "anchorAt": "2026-08-25T12:00:00.000Z",
        "startedByDeviceId": "owner",
    })
}

fn completion(index: usize, at: &str) -> Value {
    json!({
        "id": format!("history-{index}"),
        "timerId": format!("timer-{index}"),
        "commandId": format!("finish-{index}"),
        "phase": "focus",
        "status": "completed",
        "plannedDurationMs": 1_500_000,
        "completedAt": at,
        "endedAt": at,
    })
}

fn plan(input: Value) -> Value {
    let output = dispatch_json("timer.completionPlan.v1", &input.to_string()).unwrap();
    serde_json::from_str(&output).unwrap()
}

fn ownership(owner: &str) -> Value {
    json!({"timerId": "timer-4", "ownerDeviceId": owner})
}

fn expiry(history: Vec<Value>, auto_start: bool, owner: &str) -> Value {
    plan(expiry_input(history, auto_start, owner))
}

fn expiry_input(history: Vec<Value>, auto_start: bool, owner: &str) -> Value {
    json!({
        "kind": "expiry",
        "beforeTimer": timer("timer-4", "focus", "running"),
        "projectedTimer": timer("timer-4", "focus", "completed"),
        "history": history,
        "selectedPhase": "focus",
        "autoStartBreaks": auto_start,
        "localDeviceId": "owner",
        "ownership": ownership(owner),
        "dayStart": START,
        "dayEnd": END,
    })
}

fn plan_error(input: Value) -> String {
    dispatch_json("timer.completionPlan.v1", &input.to_string())
        .unwrap_err()
        .to_string()
}

#[test]
fn expiry_uses_reordered_history_and_cadence_boundary() {
    let mut history = vec![
        completion(4, "2026-08-25T12:00:00.000Z"),
        completion(2, "2026-08-25T08:00:00.000Z"),
        completion(1, "2026-08-25T07:00:00.000Z"),
        completion(3, "2026-08-25T09:00:00.000Z"),
        completion(5, "2026-08-24T23:59:59.999Z"),
    ];
    let result = expiry(history.clone(), true, "owner");
    assert_eq!(result["expired"], true);
    assert_eq!(result["selectedPhase"], "long_break");
    assert_eq!(result["generatedBreakPhase"], "long_break");
    history.remove(1);
    assert_eq!(
        expiry(history, true, "owner")["selectedPhase"],
        "short_break"
    );
}

#[test]
fn expiry_auto_start_requires_owner_and_enabled_setting() {
    let history = vec![completion(4, "2026-08-25T12:00:00.000Z")];
    let nonowner = expiry(history.clone(), true, "peer");
    let disabled = expiry(history, false, "owner");
    assert_eq!(nonowner["selectedPhase"], "short_break");
    assert!(nonowner["generatedBreakPhase"].is_null());
    assert!(disabled["generatedBreakPhase"].is_null());
}

#[test]
fn command_request_rejects_stale_automatic_projection() {
    let input = |projected: Value| {
        json!({
            "kind": "commandRequest",
            "commandType": "finish",
            "requestedTimer": timer("timer-4", "focus", "running"),
            "projectedTimer": projected,
            "automatic": true,
            "generateAutoBreak": true,
            "autoStartBreaks": true,
            "localDeviceId": "owner",
            "ownership": ownership("owner"),
        })
    };
    let accepted = plan(input(timer("timer-4", "focus", "completed")));
    let stale = plan(input(timer("other", "focus", "completed")));
    assert_eq!(accepted["commandEligible"], true);
    assert_eq!(accepted["reserveGeneratedBreak"], true);
    assert_eq!(stale["commandEligible"], false);
    assert_eq!(stale["reserveGeneratedBreak"], false);
}

#[test]
fn finish_applied_advances_focus_and_break_without_host_policy() {
    let input = |phase: &str, history: Vec<Value>, auto_start: bool, owner: &str| {
        json!({
            "kind": "finishApplied",
            "source": {
                "commandId": "finish-4",
                "timerId": "timer-4",
                "phase": phase,
                "occurredAt": "2026-08-25T12:00:00.000Z",
            },
            "history": history,
            "autoStartBreaks": auto_start,
            "localDeviceId": "owner",
            "ownership": ownership(owner),
            "dayStart": START,
            "dayEnd": END,
        })
    };
    let history = vec![completion(4, "2026-08-25T12:00:00.000Z")];
    let focus = plan(input("focus", history.clone(), true, "owner"));
    let nonowner = plan(input("focus", history.clone(), true, "peer"));
    let disabled = plan(input("focus", history, false, "owner"));
    let rest = plan(input("short_break", vec![], true, "owner"));
    assert_eq!(focus["selectedPhase"], "short_break");
    assert_eq!(focus["queueAutoBreak"], true);
    assert_eq!(nonowner["queueAutoBreak"], false);
    assert_eq!(disabled["queueAutoBreak"], false);
    assert_eq!(rest["selectedPhase"], "focus");
    assert_eq!(rest["queueAutoBreak"], false);
}

fn generated_input(canonical: Value, optimistic: Value, pending: bool) -> Value {
    json!({
        "kind": "generatedBreak",
        "source": {"commandId": "finish-4", "timerId": "timer-4"},
        "canonical": canonical,
        "optimistic": optimistic,
        "sourceFinishPending": pending,
        "requireCanonical": false,
        "dayStart": START,
        "dayEnd": END,
    })
}

fn projection(present: bool) -> Value {
    let timer = present.then(|| timer("timer-4", "focus", "completed"));
    let history = present.then(|| vec![completion(4, "2026-08-25T12:00:00.000Z")]);
    json!({"canonicalTimer": timer, "history": history.unwrap_or_default()})
}

#[test]
fn generated_break_selects_exact_projection_and_rejects_stale_state() {
    let optimistic = plan(generated_input(projection(false), projection(true), true));
    assert_eq!(optimistic["generatedBreakEligible"], true);
    assert_eq!(optimistic["generatedBreakPhase"], "short_break");
    assert_eq!(optimistic["sourceAlreadyAccepted"], false);
    let accepted = plan(generated_input(projection(true), projection(false), false));
    assert_eq!(accepted["generatedBreakPhase"], "short_break");
    assert_eq!(accepted["sourceAlreadyAccepted"], true);
    let stale = plan(generated_input(projection(false), projection(false), false));
    assert_eq!(stale["generatedBreakEligible"], false);
    assert!(stale["generatedBreakPhase"].is_null());
}

#[test]
fn generated_break_requires_exact_source_history_evidence_for_eligibility() {
    let mut missing_history = projection(true);
    missing_history["history"] = json!([]);
    let mut mismatched_history = projection(true);
    mismatched_history["history"][0]["commandId"] = json!("different-finish");

    for optimistic in [missing_history, mismatched_history] {
        let result = plan(generated_input(projection(false), optimistic, true));
        assert_eq!(result["generatedBreakEligible"], false);
        assert!(result["generatedBreakPhase"].is_null());
        assert_eq!(result["sourceAlreadyAccepted"], false);
    }
}

#[test]
fn expiry_requires_the_same_running_timer_to_become_completed() {
    let mut cases = Vec::new();
    let mut missing_before = expiry_input(vec![], false, "owner");
    missing_before["beforeTimer"] = Value::Null;
    cases.push(missing_before);

    let mut already_completed = expiry_input(vec![], false, "owner");
    already_completed["beforeTimer"]["status"] = json!("completed");
    already_completed["beforeTimer"]["elapsedAtAnchorMs"] = json!(1_500_000);
    cases.push(already_completed);

    let mut replaced = expiry_input(vec![], false, "owner");
    replaced["projectedTimer"]["id"] = json!("replacement");
    cases.push(replaced);

    let mut still_running = expiry_input(vec![], false, "owner");
    still_running["projectedTimer"]["status"] = json!("running");
    still_running["projectedTimer"]["elapsedAtAnchorMs"] = json!(0);
    cases.push(still_running);

    for input in cases {
        let result = plan(input);
        assert_eq!(result["expired"], false);
        assert!(result["selectedPhase"].is_null());
    }
}

#[test]
fn expiry_only_updates_a_matching_selection_and_never_generates_after_a_break() {
    let mut unselected = expiry_input(vec![], true, "owner");
    unselected["selectedPhase"] = json!("short_break");
    let result = plan(unselected);
    assert_eq!(result["expired"], true);
    assert!(result["selectedPhase"].is_null());
    assert_eq!(result["generatedBreakPhase"], "short_break");

    let mut break_expiry = expiry_input(vec![], true, "owner");
    break_expiry["beforeTimer"] = timer("timer-4", "long_break", "running");
    break_expiry["projectedTimer"] = timer("timer-4", "long_break", "completed");
    break_expiry["selectedPhase"] = json!("long_break");
    let result = plan(break_expiry);
    assert_eq!(result["selectedPhase"], "focus");
    assert!(result["generatedBreakPhase"].is_null());
}

fn command_input() -> Value {
    json!({
        "kind": "commandRequest",
        "commandType": "finish",
        "requestedTimer": timer("timer-4", "focus", "running"),
        "projectedTimer": timer("timer-4", "focus", "completed"),
        "automatic": true,
        "generateAutoBreak": true,
        "autoStartBreaks": true,
        "localDeviceId": "owner",
        "ownership": ownership("owner"),
    })
}

#[test]
fn command_request_distinguishes_manual_and_automatic_boundaries() {
    let mut manual = command_input();
    manual["automatic"] = json!(false);
    manual["commandType"] = json!("pause");
    manual["requestedTimer"] = Value::Null;
    let result = plan(manual);
    assert_eq!(result["commandEligible"], true);
    assert_eq!(result["reserveGeneratedBreak"], false);

    let mut unsupported = command_input();
    unsupported["commandType"] = json!("pause");
    assert_eq!(
        plan_error(unsupported),
        "invalid shared-core input: only finish can be queued automatically"
    );

    let mut missing_requested = command_input();
    missing_requested["requestedTimer"] = Value::Null;
    assert_eq!(plan(missing_requested)["commandEligible"], false);

    let mut unfinished = command_input();
    unfinished["projectedTimer"]["status"] = json!("running");
    unfinished["projectedTimer"]["elapsedAtAnchorMs"] = json!(0);
    assert_eq!(plan(unfinished)["commandEligible"], false);
}

#[test]
fn automatic_finish_rejects_a_prior_finish_or_missing_ownership() {
    let mut prior_finish = command_input();
    prior_finish["projectedTimer"]["lastIntent"] = json!({
        "type": "finish",
        "commandId": "finish-before",
        "occurredAt": "2026-08-25T12:00:00.000Z",
    });
    assert_eq!(plan(prior_finish)["commandEligible"], false);

    let mut prior_pause = command_input();
    prior_pause["projectedTimer"]["lastIntent"] = json!({
        "type": "pause",
        "commandId": "pause-before",
        "occurredAt": "2026-08-25T12:00:00.000Z",
    });
    assert_eq!(plan(prior_pause)["commandEligible"], true);

    let mut unowned = command_input();
    unowned["ownership"] = Value::Null;
    let result = plan(unowned);
    assert_eq!(result["commandEligible"], false);
    assert_eq!(result["reserveGeneratedBreak"], false);
}

#[test]
fn generated_break_requires_exact_source_evidence() {
    let canonical = projection(true);
    for (field, value) in [
        ("timerId", json!("other")),
        ("commandId", json!("other")),
        ("phase", json!("short_break")),
        ("status", json!("cancelled")),
    ] {
        let mut evidence = canonical.clone();
        evidence["history"][0][field] = value;
        let result = plan(generated_input(evidence, projection(false), false));
        assert_eq!(result["sourceAlreadyAccepted"], false);
        assert!(result["generatedBreakPhase"].is_null());
    }

    for (field, value) in [
        ("id", json!("other")),
        ("phase", json!("short_break")),
        ("status", json!("cancelled")),
    ] {
        let mut evidence = canonical.clone();
        evidence["canonicalTimer"][field] = value;
        let result = plan(generated_input(evidence, projection(false), false));
        assert_eq!(result["generatedBreakEligible"], false);
    }

    let mut require_canonical = generated_input(projection(false), projection(true), true);
    require_canonical["requireCanonical"] = json!(true);
    assert_eq!(plan(require_canonical)["generatedBreakEligible"], false);
}

fn finish_input() -> Value {
    json!({
        "kind": "finishApplied",
        "source": {
            "commandId": "finish-4",
            "timerId": "timer-4",
            "phase": "focus",
            "occurredAt": "2026-08-25T12:00:00.000Z",
        },
        "history": [],
        "autoStartBreaks": true,
        "localDeviceId": "owner",
        "ownership": ownership("owner"),
        "dayStart": START,
        "dayEnd": END,
    })
}

#[test]
fn completion_plan_rejects_invalid_sources() {
    for field in ["commandId", "timerId", "phase"] {
        let mut input = finish_input();
        input["source"][field] = json!("");
        assert_eq!(
            plan_error(input),
            "invalid shared-core input: invalid completion source"
        );
    }
    let mut invalid_phase = finish_input();
    invalid_phase["source"]["phase"] = json!("nap");
    assert_eq!(
        plan_error(invalid_phase),
        "invalid shared-core input: invalid completion phase"
    );

    let mut invalid_time = finish_input();
    invalid_time["source"]["occurredAt"] = json!("not-a-time");
    assert_eq!(
        plan_error(invalid_time),
        "invalid RFC 3339 timestamp: not-a-time"
    );
}

#[test]
fn completion_plan_rejects_invalid_ownership_and_bounds() {
    let mut invalid_owner = command_input();
    invalid_owner["localDeviceId"] = json!("");
    assert_eq!(
        plan_error(invalid_owner),
        "invalid shared-core input: invalid completion ownership"
    );

    let mut invalid_bounds = finish_input();
    invalid_bounds["dayEnd"] = json!(START);
    assert_eq!(
        plan_error(invalid_bounds),
        "invalid shared-core input: invalid completion day bounds"
    );
}

#[test]
fn generated_break_rejects_empty_source_identity() {
    for field in ["commandId", "timerId"] {
        let mut input = generated_input(projection(false), projection(false), false);
        input["source"][field] = json!("");
        assert_eq!(
            plan_error(input),
            "invalid shared-core input: invalid completion source"
        );
    }
}
