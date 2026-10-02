use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/read-model-v1.json")).unwrap()
}

fn output(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("workspace.readModel.v1", &input.to_string()).unwrap())
        .unwrap()
}

fn task_id() -> String {
    let identity =
        dispatch_json("task.identity.v1", &json!({"title": "Study"}).to_string()).unwrap();
    serde_json::from_str::<Value>(&identity).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn completion(index: usize, at: &str) -> Value {
    json!({"id": format!("history-{index}"), "timerId": format!("timer-{index}"),
        "phase": "focus", "status": "completed", "plannedDurationMs": 1500000,
        "taskId": task_id(), "completedAt": at})
}

#[test]
fn countdown_and_profile_display_parity() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    let idle = output(&request);
    assert_eq!(idle["schemaVersion"], 1);
    assert_eq!(idle["canonical"]["timerId"], Value::Null);
    assert_eq!(idle["display"]["remainingMs"], 1_500_000.0);
    assert_eq!(
        idle["availableIntents"],
        json!(["start", "selectPhase", "skip"])
    );

    request["source"]["value"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    let running = output(&request);
    assert_eq!(running["canonical"]["elapsedMs"], 180_000.0);
    assert_eq!(running["canonical"]["remainingMs"], 1_320_000.0);
    assert_eq!(running["canonical"]["deadlineAt"], "2026-03-08T07:52:00Z");
    assert_eq!(running["canonical"]["progress"], 0.12);
    assert_eq!(
        running["availableIntents"],
        json!(["pause", "finish", "cancel", "cancelAndClear", "selectPhase"])
    );
    request["source"]["value"]["base"]["canonicalTimer"]["status"] = json!("paused");
    let paused = output(&request);
    assert_eq!(paused["canonical"]["elapsedMs"], 120_000.0);
    assert_eq!(paused["canonical"]["deadlineAt"], Value::Null);
    assert_eq!(paused["availableIntents"][0], "resume");

    request["source"]["value"]["base"]["canonicalTimer"]["status"] = json!("completed");
    request["source"]["value"]["base"]["canonicalTimer"]["elapsedAtAnchorMs"] = json!(1_500_000);
    for (profile, reset) in [
        ("appleWorkspace", true),
        ("androidCoordinator", true),
        ("desktopTerminal", true),
        ("pwaStorage", true),
    ] {
        request["profile"] = json!(profile);
        let result = output(&request);
        assert_eq!(result["canonical"]["remainingMs"], 0.0);
        assert_eq!(
            result["display"]["remainingMs"],
            if reset { 1_500_000.0 } else { 0.0 }
        );
    }
}

#[test]
fn cadence_skip_and_task_counts_cover_zero_through_twelve_and_dst_edges() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    request["source"]["value"]["base"]["tasks"] = json!([{"id": task_id(), "title": "Study"}]);
    request["source"]["value"]["base"]["selectedTaskId"] = json!(task_id());
    for row in fixture["cadence"].as_array().unwrap() {
        let count = row["count"].as_u64().unwrap() as usize;
        let mut history = vec![completion(100, "2026-03-08T04:59:59.999Z")];
        history.extend((0..count).map(|index| completion(index, "2026-03-08T05:00:00Z")));
        history.push(completion(101, "2026-03-09T04:00:00Z"));
        request["source"]["value"]["base"]["history"] = json!(history);
        let result = output(&request);
        assert_eq!(result["cadence"]["completedFocusToday"], count);
        assert_eq!(
            result["cadence"]["completedFocusTodayPlannedDurationMs"],
            count as i64 * 1_500_000
        );
        assert_eq!(result["cadence"]["completedFocusTotal"], count + 2);
        assert_eq!(result["cadence"]["longBreakProgress"], row["progress"]);
        assert_eq!(result["cadence"]["skipDestination"], row["skip"]);
        assert_eq!(
            result["cadence"]["nextCompletedFocusBreakPhase"],
            row["next"]
        );
        assert_eq!(
            result["tasks"]["completedFocusTodayByTask"][task_id()],
            json!({"count": count, "plannedDurationMs": count as i64 * 1_500_000})
        );
        request["selectedPhase"] = json!("long_break");
        assert_eq!(output(&request)["cadence"]["skipDestination"], "focus");
        request["selectedPhase"] = json!("focus");
    }
}

#[test]
fn precise_pwa_interpolation_requires_raw_workspace_source() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    request["profile"] = json!("pwaStorage");
    request["source"]["value"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    request["monotonic"] = json!({"nowMs": 5000.375, "continuityId": "session",
        "anchor": {"timerId": "timer-one", "anchorAt": "2026-03-08T07:29:00Z",
            "elapsedAtAnchorMs": 120000, "sampledTrustedNowMs": 1772955000000_i64,
            "sampledMonotonicMs": 4999.875, "continuityId": "session"}});
    let result = output(&request);
    assert_eq!(result["canonical"]["elapsedMs"], 180_000.5);
    assert_eq!(result["canonical"]["observedElapsedMs"], 180_001);
    assert_eq!(result["canonical"]["remainingMs"], 1_319_999.5);
    let raw_source = request["source"].clone();
    let mut projection_request = request["source"]["value"].clone();
    projection_request["now"] = request["observedAt"].clone();
    let projection: Value = serde_json::from_str(
        &dispatch_json("workspace.project.v1", &projection_request.to_string()).unwrap(),
    )
    .unwrap();
    request["source"] = json!({"kind": "projectionResult", "value": projection});
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
    request["source"] = raw_source;
    assert_eq!(output(&request), result);
}

#[test]
fn malformed_boundaries_fail_closed() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    request["calendarIntervals"] =
        json!([{"start":"2026-03-08T05:00:00Z","end":"2026-03-08T05:00:00Z"}]);
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
    request = fixture["request"].clone();
    request["source"]["value"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    request["source"]["value"]["base"]["canonicalTimer"]["elapsedAtAnchorMs"] = json!(1_500_001);
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
    request["source"]["value"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    request["profile"] = json!("appleWorkspace");
    request["monotonic"] = json!({"nowMs": 1.5, "continuityId": "x", "anchor": {}});
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
    request["monotonic"] = Value::Null;
    request["source"] = json!({"kind":"projectionResult", "value":{"workspace":{}}});
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
    assert!(
        dispatch_json(
            "workspace.readModel.v1",
            r#"{"profile":"pwaStorage","profile":"appleWorkspace"}"#
        )
        .is_err()
    );
}

#[test]
fn expiry_and_wall_jump_keep_canonical_completion_authoritative() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    let mut timer = fixture["timer"].clone();
    timer["plannedDurationMs"] = json!(60_000);
    timer["elapsedAtAnchorMs"] = json!(59_999);
    timer["anchorAt"] = json!("2026-03-08T07:29:59Z");
    request["source"]["value"]["base"]["canonicalTimer"] = timer.clone();
    let completed = output(&request);
    assert_eq!(completed["canonical"]["status"], "completed");
    assert_eq!(completed["canonical"]["remainingMs"], 0.0);
    assert_eq!(completed["cadence"]["completedFocusToday"], 1);

    request["profile"] = json!("pwaStorage");
    request["monotonic"] = json!({"nowMs": 100.25, "continuityId": "session",
        "anchor": {"timerId": "timer-one", "anchorAt": "2026-03-08T07:29:59Z",
            "elapsedAtAnchorMs": 59999, "sampledTrustedNowMs": 1772954999000_i64,
            "sampledMonotonicMs": 100.0, "continuityId": "session"}});
    let live = output(&request);
    assert_eq!(live["canonical"]["status"], "running");
    assert_eq!(live["canonical"]["elapsedMs"], 59_999.25);
    assert_eq!(live["cadence"]["completedFocusToday"], 0);
    request["monotonic"]["nowMs"] = json!(101.0);
    assert_eq!(output(&request)["canonical"]["status"], "completed");
}

#[test]
fn readiness_profiles_and_invalid_projection_snapshot() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    request["source"]["value"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    for (profile, running, paused) in [
        (
            "appleWorkspace",
            json!(["pause", "finish", "cancel", "cancelAndClear", "selectPhase"]),
            json!([
                "resume",
                "finish",
                "cancel",
                "cancelAndClear",
                "selectPhase"
            ]),
        ),
        (
            "androidCoordinator",
            json!(["pause", "finish", "cancel", "cancelAndClear"]),
            json!(["resume", "finish", "cancel", "cancelAndClear"]),
        ),
        (
            "desktopTerminal",
            json!(["pause", "finish", "cancel", "cancelAndClear"]),
            json!(["resume", "finish", "cancel", "cancelAndClear"]),
        ),
        (
            "pwaStorage",
            json!(["pause", "finish", "cancel", "cancelAndClear"]),
            json!(["resume", "finish", "cancel", "cancelAndClear"]),
        ),
    ] {
        request["profile"] = json!(profile);
        request["source"]["value"]["base"]["canonicalTimer"]["status"] = json!("running");
        assert_eq!(output(&request)["availableIntents"], running);
        request["source"]["value"]["base"]["canonicalTimer"]["status"] = json!("paused");
        assert_eq!(output(&request)["availableIntents"], paused);
    }
    let mut projection = request["source"]["value"].clone();
    projection["now"] = request["observedAt"].clone();
    let mut projected: Value = serde_json::from_str(
        &dispatch_json("workspace.project.v1", &projection.to_string()).unwrap(),
    )
    .unwrap();
    projected["workspace"]["tasks"] = json!([{"id":"forged","title":"Study"}]);
    request["source"] = json!({"kind":"projectionResult","value":projected});
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
}

#[test]
fn twenty_five_hour_day_and_stale_monotonic_fallback() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    request["observedAt"] = json!("2026-11-02T04:59:59Z");
    request["calendarIntervals"] = json!([{"start": "2026-11-01T04:00:00Z",
        "end": "2026-11-02T05:00:00Z"}]);
    request["source"]["value"]["base"]["history"] = json!([
        completion(1, "2026-11-01T04:00:00Z"),
        completion(2, "2026-11-02T04:59:59Z"),
        completion(3, "2026-11-02T05:00:00Z")
    ]);
    assert_eq!(output(&request)["cadence"]["completedFocusToday"], 2);
    assert_eq!(
        output(&request)["cadence"]["completedFocusTodayPlannedDurationMs"],
        3_000_000
    );

    request = fixture["request"].clone();
    request["profile"] = json!("pwaStorage");
    request["source"]["value"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    request["monotonic"] = json!({"nowMs": 50.0, "continuityId": "new-session",
        "anchor": {"timerId": "timer-one", "anchorAt": "2026-03-08T07:29:00Z",
            "elapsedAtAnchorMs": 120000, "sampledTrustedNowMs": 1772955000000_i64,
            "sampledMonotonicMs": 100.0, "continuityId": "old-session"}});
    assert_eq!(output(&request)["canonical"]["elapsedMs"], 180_000.0);
    request["monotonic"]["nowMs"] = json!(-1.0);
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
    request["monotonic"] = Value::Null;
    request["calendarIntervals"] = json!([{"start":"2026-03-08T05:00:00Z",
        "end":"2026-03-09T04:00:00Z"}, {"start":"2026-03-08T06:00:00Z",
        "end":"2026-03-08T08:00:00Z"}]);
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
}

#[test]
fn projected_running_snapshot_is_not_an_unverified_read_source() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    let mut timer = fixture["timer"].clone();
    timer["plannedDurationMs"] = json!(60_000);
    timer["elapsedAtAnchorMs"] = json!(59_999);
    timer["anchorAt"] = json!("2026-03-08T07:29:59Z");
    request["source"]["value"]["base"]["canonicalTimer"] = timer.clone();
    let expected = output(&request);
    let mut projection_request = request["source"]["value"].clone();
    projection_request["now"] = json!("2026-03-08T07:29:59Z");
    let stale: Value = serde_json::from_str(
        &dispatch_json("workspace.project.v1", &projection_request.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(stale["workspace"]["canonicalTimer"]["status"], "running");
    request["source"] = json!({"kind":"projectionResult", "value": stale});
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
    request["source"] = fixture["request"]["source"].clone();
    request["source"]["value"]["base"]["canonicalTimer"] = timer;
    assert_eq!(output(&request), expected);
}

#[test]
fn checker_terminal_display_and_completed_short_anchor_rejection() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    request["selectedPhase"] = json!("short_break");
    request["source"]["value"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    request["source"]["value"]["base"]["canonicalTimer"]["status"] = json!("superseded");
    let apple = output(&request);
    assert_eq!(apple["display"]["status"], "idle");
    assert_eq!(apple["display"]["phase"], "short_break");
    assert_eq!(apple["display"]["elapsedMs"], 0.0);
    assert_eq!(apple["display"]["remainingMs"], 300_000.0);
    request["profile"] = json!("desktopStorage");
    request["source"]["value"]["base"]["canonicalTimer"]["status"] = json!("cancelled");
    let desktop = output(&request);
    assert_eq!(desktop["canonical"]["elapsedMs"], 120_000.0);
    assert_eq!(desktop["display"]["elapsedMs"], 0.0);
    assert_eq!(desktop["display"]["remainingMs"], 1_500_000.0);
    request["source"]["value"]["base"]["canonicalTimer"]["status"] = json!("completed");
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
    request["source"]["value"]["base"]["canonicalTimer"]["elapsedAtAnchorMs"] = json!(1_500_000);
    let completed = output(&request);
    assert_eq!(completed["canonical"]["status"], "completed");
    assert_eq!(completed["canonical"]["remainingMs"], 0.0);
    assert_eq!(completed["canonical"]["progress"], 1.0);
}

#[test]
fn checker_task_rows_use_reference_day_and_planned_durations() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    let id = task_id();
    request["source"]["value"]["base"]["tasks"] = json!([{"id": id, "title": "Study"}]);
    let history: Vec<Value> = fixture["checkerHistory"]
        .as_array()
        .unwrap()
        .iter()
        .cloned()
        .map(|mut item| {
            if item["taskId"] == "current" {
                item["taskId"] = json!(id);
            }
            item
        })
        .collect();
    request["source"]["value"]["base"]["history"] = json!(history);
    let result = output(&request);
    let totals = &fixture["checkerDailyTotals"];
    assert_eq!(
        result["cadence"]["completedFocusToday"],
        totals["focusCount"]
    );
    assert_eq!(
        result["cadence"]["completedFocusTodayPlannedDurationMs"],
        totals["focusPlannedDurationMs"]
    );
    assert_eq!(
        result["tasks"]["completedFocusTodayByTask"][&id],
        json!({"count": totals["currentTaskCount"], "plannedDurationMs": totals["currentTaskPlannedDurationMs"]})
    );
    assert!(
        result["tasks"]["completedFocusTodayByTask"]
            .get("removed")
            .is_none()
    );
    assert!(
        result["tasks"]["completedFocusTodayByTask"]
            .get("unassigned")
            .is_none()
    );
}

#[test]
fn desktop_gui_primary_uses_restart_for_terminal_and_start_only_for_idle() {
    let fixture = fixture();
    for profile in ["desktopStorage", "desktopTerminal"] {
        for row in fixture["desktopPrimary"].as_array().unwrap() {
            let mut request = fixture["request"].clone();
            request["profile"] = json!(profile);
            let status = row["status"].as_str().unwrap();
            if status != "idle" {
                request["source"]["value"]["base"]["canonicalTimer"] = fixture["timer"].clone();
                request["source"]["value"]["base"]["canonicalTimer"]["status"] = json!(status);
                if status == "completed" {
                    request["source"]["value"]["base"]["canonicalTimer"]["elapsedAtAnchorMs"] =
                        json!(1_500_000);
                }
            }
            let read = output(&request);
            assert_eq!(
                read["availableIntents"], row["intents"],
                "{profile}/{status}"
            );
        }
    }
}

#[test]
fn restart_intent_plans_atomic_pair_for_both_desktop_profiles() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    for profile in ["desktopStorage", "desktopTerminal"] {
        for status in ["completed", "cancelled", "superseded"] {
            let mut request = fixture["request"].clone();
            request["compatibility"] = json!(profile);
            request["intent"] = json!({"kind": "restart"});
            let mut timer = fixture["timer"].clone();
            timer["status"] = json!(status);
            if status == "completed" {
                timer["elapsedAtAnchorMs"] = json!(60_000);
            }
            request["workspace"]["base"]["canonicalTimer"] = timer.clone();
            request["requestedTimer"] = timer;
            let plan: Value = serde_json::from_str(
                &dispatch_json("workspace.intent.v1", &request.to_string()).unwrap(),
            )
            .unwrap();
            let commands = plan["commands"].as_array().unwrap();
            assert_eq!(
                commands
                    .iter()
                    .map(|command| command["type"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["clear", "start"],
                "{profile}/{status}"
            );
            assert_eq!(plan["atomicCommandIds"].as_array().unwrap().len(), 2);
        }
    }
}

#[test]
fn checker_civil_day_lengths_and_forged_projection_rejected() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    for end in [
        "2026-03-08T07:30:01Z",
        "2026-03-09T08:30:01Z",
        "2026-03-10T05:00:00Z",
    ] {
        request["calendarIntervals"] = json!([{"start": "2026-03-08T07:30:00Z", "end": end}]);
        assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
    }
    request = fixture["request"].clone();
    request["calendarIntervals"] =
        json!([{"start": "2026-03-08T05:00:00Z", "end": "2026-03-09T04:30:00Z"}]);
    assert_eq!(output(&request)["cadence"]["completedFocusToday"], 0);
    request = fixture["request"].clone();
    request["source"]["value"]["now"] = json!(request["observedAt"]);
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
    request = fixture["request"].clone();
    request["source"]["value"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    let mut projected_request = request["source"]["value"].clone();
    projected_request["now"] = request["observedAt"].clone();
    let projected: Value = serde_json::from_str(
        &dispatch_json("workspace.project.v1", &projected_request.to_string()).unwrap(),
    )
    .unwrap();
    for variant in ["valid", "timer", "history", "queue"] {
        let mut forged = projected.clone();
        match variant {
            "timer" => forged["workspace"]["canonicalTimer"]["id"] = json!("forged"),
            "history" => {
                forged["workspace"]["history"] = json!([completion(44, "2026-03-08T05:00:00Z")])
            }
            "queue" => forged["projectionPending"]["commands"] = json!([{"garbage":true}]),
            _ => {}
        }
        request["source"] = json!({"kind":"projectionResult", "value": forged});
        assert!(
            dispatch_json("workspace.readModel.v1", &request.to_string()).is_err(),
            "{variant}"
        );
    }
    request = fixture["request"].clone();
    request["source"]["value"]["local"]["commands"] = json!([{"garbage":true}]);
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
    request["source"]["value"]["local"]["commands"] = json!([]);
    request["source"]["value"]["base"]["history"] = json!([{"id":"bad"}]);
    assert!(dispatch_json("workspace.readModel.v1", &request.to_string()).is_err());
}

#[test]
fn checker_pwa_terminal_clear_not_visible() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    request["profile"] = json!("pwaStorage");
    request["source"]["value"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    request["source"]["value"]["base"]["canonicalTimer"]["status"] = json!("completed");
    request["source"]["value"]["base"]["canonicalTimer"]["elapsedAtAnchorMs"] = json!(1_500_000);
    let available = output(&request)["availableIntents"]
        .as_array()
        .unwrap()
        .clone();
    assert!(available.contains(&json!("start")));
    assert!(!available.contains(&json!("clear")));
}

fn retarget_command(index: i64, kind: &str, task: Option<&str>) -> Value {
    let at = [
        "2026-03-08T07:29:00Z",
        "2026-03-08T07:29:10Z",
        "2026-03-08T07:29:20Z",
    ];
    let mut command = json!({"id": format!("command-{index}"), "deviceId": "device-a",
        "deviceSequence": index + 1, "timerId": "retargeted", "type": kind, "phase": "focus",
        "plannedDurationMs": 60000, "occurredAt": at[index as usize],
        "hlcWallMs": 1772954940000_i64 + index, "hlcCounter": 0,
        "observedElapsedMs": if kind == "finish" {60000} else {0}});
    if kind == "retarget" || task.is_some() {
        command["taskId"] = json!(task);
    }
    command
}

#[test]
fn retargeted_completion_counts_only_selected_destination_task_today() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    let first: Value = serde_json::from_str(
        &dispatch_json("task.identity.v1", &json!({"title":"First"}).to_string()).unwrap(),
    )
    .unwrap();
    let second: Value = serde_json::from_str(
        &dispatch_json("task.identity.v1", &json!({"title":"Second"}).to_string()).unwrap(),
    )
    .unwrap();
    let a = first["id"].as_str().unwrap();
    let b = second["id"].as_str().unwrap();
    request["source"]["value"]["base"]["tasks"] = json!([
        {"id":a,"title":"First"}, {"id":b,"title":"Second"}]);
    request["source"]["value"]["canonicalHead"] = json!({"wallMs": 1, "counter": 0});
    request["source"]["value"]["local"]["commands"] = json!([
        retarget_command(0, "start", Some(a)),
        retarget_command(1, "retarget", Some(b)),
        retarget_command(2, "finish", None)
    ]);
    request["source"]["value"]["neverSent"] =
        json!({"commands": ["command-0", "command-1", "command-2"]});
    let result = output(&request);
    assert_eq!(
        result["tasks"]["completedFocusTodayByTask"][a],
        json!({"count":0,"plannedDurationMs":0})
    );
    assert_eq!(
        result["tasks"]["completedFocusTodayByTask"][b],
        json!({"count":1,"plannedDurationMs":60000})
    );
    assert_eq!(result["cadence"]["completedFocusToday"], 1);
    assert_eq!(
        result["cadence"]["completedFocusTodayPlannedDurationMs"],
        60_000
    );
    request["source"]["value"]["local"]["commands"][1]["taskId"] = Value::Null;
    let unassigned = output(&request);
    assert_eq!(
        unassigned["tasks"]["completedFocusTodayByTask"][b],
        json!({"count":0,"plannedDurationMs":0})
    );
    assert_eq!(unassigned["cadence"]["completedFocusToday"], 1);
    assert_eq!(
        unassigned["cadence"]["completedFocusTodayPlannedDurationMs"],
        60_000
    );
}
