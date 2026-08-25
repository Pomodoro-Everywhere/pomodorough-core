use pomodorough_core::dispatch_json;
use serde_json::{Map, Value, json};

const SERVER_WALL_MS: i64 = 1_784_548_800_000;

fn timestamp(offset_ms: i64) -> String {
    let hours = 12 + offset_ms / 3_600_000;
    let minutes = offset_ms % 3_600_000 / 60_000;
    let seconds = offset_ms % 60_000 / 1_000;
    let milliseconds = offset_ms % 1_000;
    if milliseconds == 0 {
        format!("2026-07-20T{hours:02}:{minutes:02}:{seconds:02}Z")
    } else {
        let fraction = format!("{milliseconds:03}")
            .trim_end_matches('0')
            .to_owned();
        format!("2026-07-20T{hours:02}:{minutes:02}:{seconds:02}.{fraction}Z")
    }
}

fn command(id: &str, timer_id: &str, sequence: i64, wall_offset_ms: i64) -> Value {
    json!({
        "id": id,
        "deviceId": "device-a",
        "deviceSequence": sequence,
        "timerId": timer_id,
        "type": "start",
        "phase": "short_break",
        "plannedDurationMs": 300_000,
        "occurredAt": timestamp(wall_offset_ms),
        "hlcWallMs": SERVER_WALL_MS + wall_offset_ms,
        "hlcCounter": 0,
        "observedElapsedMs": 0
    })
}

fn completed_focus(id: &str, timer_id: &str, command_id: &str, wall_offset_ms: i64) -> Value {
    json!({
        "id": id,
        "timerId": timer_id,
        "commandId": command_id,
        "phase": "focus",
        "status": "completed",
        "plannedDurationMs": 1_500_000,
        "completedAt": timestamp(wall_offset_ms)
    })
}

fn generated_break_dependencies() -> Value {
    json!([
        {
            "operationId": "generated-start",
            "dependsOnOperationId": "finish-sent",
            "generatedBreak": true,
            "sourceDayStart": "2026-07-20T00:00:00Z",
            "sourceDayEnd": "2026-07-21T00:00:00Z"
        },
        {
            "operationId": "generated-pause",
            "dependsOnOperationId": "generated-start"
        }
    ])
}

fn task_operation(id: &str, task_id: &str, title: &str, wall_offset_ms: i64) -> Value {
    json!({
        "id": id,
        "deviceId": "device-a",
        "taskId": task_id,
        "type": "upsert",
        "title": title,
        "occurredAt": timestamp(wall_offset_ms),
        "hlcWallMs": SERVER_WALL_MS + wall_offset_ms,
        "hlcCounter": 0
    })
}

fn duration_operation(id: &str, phase: &str, duration_ms: i64, wall_offset_ms: i64) -> Value {
    json!({
        "id": id,
        "deviceId": "device-a",
        "phase": phase,
        "durationMs": duration_ms,
        "occurredAt": timestamp(wall_offset_ms),
        "hlcWallMs": SERVER_WALL_MS + wall_offset_ms,
        "hlcCounter": 0
    })
}

fn auto_start_operation(id: &str, enabled: bool, wall_offset_ms: i64) -> Value {
    json!({
        "id": id,
        "deviceId": "device-a",
        "enabled": enabled,
        "occurredAt": timestamp(wall_offset_ms),
        "hlcWallMs": SERVER_WALL_MS + wall_offset_ms,
        "hlcCounter": 0
    })
}

fn selected_task_operation(id: &str, task_id: Option<&str>, wall_offset_ms: i64) -> Value {
    json!({
        "id": id,
        "deviceId": "device-a",
        "taskId": task_id,
        "occurredAt": timestamp(wall_offset_ms),
        "hlcWallMs": SERVER_WALL_MS + wall_offset_ms,
        "hlcCounter": 0
    })
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

fn acknowledgement(items: &[Value], id_key: &str, outcome: &str) -> Vec<Value> {
    items
        .iter()
        .map(|item| {
            json!({
                (id_key): item["id"],
                "outcome": outcome,
                "reason": ""
            })
        })
        .collect()
}

fn canonical_response(sent: &Value) -> Value {
    json!({
        "acknowledgements": acknowledgement(sent["commands"].as_array().unwrap(), "commandId", "applied"),
        "taskAcknowledgements": acknowledgement(sent["taskOperations"].as_array().unwrap(), "operationId", "applied"),
        "durationAcknowledgements": acknowledgement(sent["durationOperations"].as_array().unwrap(), "operationId", "applied"),
        "autoStartAcknowledgements": acknowledgement(sent["autoStartOperations"].as_array().unwrap(), "operationId", "applied"),
        "selectedTaskAcknowledgements": acknowledgement(sent["selectedTaskOperations"].as_array().unwrap(), "operationId", "applied"),
        "revision": 9,
        "canonicalTimer": null,
        "history": [],
        "tasks": [{"id": "task-remote", "title": "Remote"}],
        "durationsMs": {
            "focus": 1_500_000,
            "short_break": 300_000,
            "long_break": 900_000
        },
        "autoStartBreaks": false,
        "selectedTaskId": null,
        "serverTime": timestamp(10_000),
        "serverHlcWallMs": SERVER_WALL_MS + 10_000,
        "serverHlcCounter": 0
    })
}

fn reconcile_with_dependencies(
    local: Value,
    sent: Value,
    response: Value,
    timer_dependencies: Value,
) -> Result<Value, String> {
    let input = json!({
        "local": local,
        "sent": sent,
        "response": response,
        "timerDependencies": timer_dependencies,
    });
    dispatch_json("reconcile.rebase.v1", &input.to_string())
        .map_err(|error| error.to_string())
        .and_then(|encoded| serde_json::from_str(&encoded).map_err(|error| error.to_string()))
}

fn reconcile(local: Value, sent: Value, response: Value) -> Result<Value, String> {
    reconcile_with_dependencies(local, sent, response, json!([]))
}

fn one_of_each() -> Value {
    json!({
        "commands": [command("command-sent", "timer-sent", 1, 1_000)],
        "taskOperations": [task_operation("task-sent", "task-sent", "Sent", 1_000)],
        "durationOperations": [duration_operation("duration-sent", "focus", 1_800_000, 1_000)],
        "autoStartOperations": [auto_start_operation("auto-sent", true, 1_000)],
        "selectedTaskOperations": [selected_task_operation("selected-sent", None, 1_000)]
    })
}

#[test]
fn reconcile_rebase_v1_rejects_missing_extra_duplicate_and_malformed_acknowledgements() {
    let sent = one_of_each();
    let local = sent.clone();
    let domains = [
        ("commands", "acknowledgements", "commandId"),
        ("taskOperations", "taskAcknowledgements", "operationId"),
        (
            "durationOperations",
            "durationAcknowledgements",
            "operationId",
        ),
        (
            "autoStartOperations",
            "autoStartAcknowledgements",
            "operationId",
        ),
        (
            "selectedTaskOperations",
            "selectedTaskAcknowledgements",
            "operationId",
        ),
    ];

    for (queue_field, response_field, id_field) in domains {
        let valid = canonical_response(&sent);
        let expected_id = sent[queue_field][0]["id"].clone();
        let invalid_sets = [
            None,
            Some(json!([])),
            Some(json!([
                {(id_field): expected_id.clone(), "outcome": "applied", "reason": ""},
                {(id_field): "extra-id", "outcome": "applied", "reason": ""}
            ])),
            Some(json!([
                {(id_field): expected_id.clone(), "outcome": "applied", "reason": ""},
                {(id_field): expected_id.clone(), "outcome": "applied", "reason": ""}
            ])),
            Some(json!([{(id_field): expected_id.clone(), "outcome": "unknown", "reason": ""}])),
            Some(json!([{(id_field): 7, "outcome": "applied", "reason": ""}])),
            Some(json!([{(id_field): expected_id.clone(), "outcome": "applied"}])),
            Some(json!([{(id_field): expected_id, "outcome": "applied", "reason": null}])),
        ];

        for invalid_set in invalid_sets {
            let mut invalid = valid.clone();
            match invalid_set {
                Some(value) => invalid[response_field] = value,
                None => {
                    invalid.as_object_mut().unwrap().remove(response_field);
                }
            }
            let error = reconcile(local.clone(), sent.clone(), invalid).unwrap_err();
            assert!(
                error.contains(response_field),
                "{response_field} error was {error}"
            );
        }
    }
}

#[test]
fn reconcile_rebase_v1_removes_every_acknowledged_outcome_from_every_queue() {
    let outcomes = ["applied", "ignored", "rejected"];
    for outcome in outcomes {
        let sent = one_of_each();
        let retained = json!({
            "commands": [command("command-retained", "timer-retained", 2, 11_000)],
            "taskOperations": [task_operation("task-retained-op", "task-retained", "Retained", 11_000)],
            "durationOperations": [duration_operation("duration-retained", "long_break", 1_200_000, 11_000)],
            "autoStartOperations": [auto_start_operation("auto-retained", false, 11_000)],
            "selectedTaskOperations": [selected_task_operation("selected-retained", Some("task-retained"), 11_000)]
        });
        let mut local = Map::new();
        for field in [
            "commands",
            "taskOperations",
            "durationOperations",
            "autoStartOperations",
            "selectedTaskOperations",
        ] {
            let mut operations = sent[field].as_array().unwrap().clone();
            operations.extend(retained[field].as_array().unwrap().clone());
            local.insert(field.to_owned(), Value::Array(operations));
        }
        let mut response = canonical_response(&sent);
        for field in [
            "acknowledgements",
            "taskAcknowledgements",
            "durationAcknowledgements",
            "autoStartAcknowledgements",
            "selectedTaskAcknowledgements",
        ] {
            response[field][0]["outcome"] = json!(outcome);
            response[field][0]["reason"] =
                json!(if outcome == "applied" { "" } else { "resolved" });
        }

        let output = reconcile(Value::Object(local), sent, response).unwrap();
        assert_eq!(output["pending"][0]["id"], "command-retained");
        assert_eq!(output["pendingTaskOperations"][0]["id"], "task-retained-op");
        assert_eq!(
            output["pendingDurationOperations"][0]["id"],
            "duration-retained"
        );
        assert_eq!(
            output["pendingAutoStartOperations"][0]["id"],
            "auto-retained"
        );
        assert_eq!(
            output["pendingSelectedTaskOperations"][0]["id"],
            "selected-retained"
        );
    }
}

#[test]
fn reconcile_rebase_v1_requires_complete_canonical_response_and_selected_task_presence() {
    let local = empty_queues();
    let sent = empty_queues();
    let valid = canonical_response(&sent);
    for field in [
        "revision",
        "canonicalTimer",
        "history",
        "tasks",
        "durationsMs",
        "autoStartBreaks",
        "selectedTaskId",
        "serverTime",
        "serverHlcWallMs",
        "serverHlcCounter",
    ] {
        let mut invalid = valid.clone();
        invalid.as_object_mut().unwrap().remove(field);
        assert!(
            reconcile(local.clone(), sent.clone(), invalid).is_err(),
            "missing {field} was accepted"
        );
    }

    let deselected = reconcile(local.clone(), sent.clone(), valid.clone()).unwrap();
    assert!(deselected.get("baseSelectedTaskId").is_some());
    assert_eq!(deselected["baseSelectedTaskId"], Value::Null);
    assert_eq!(deselected["selectedTaskId"], Value::Null);

    let mut selected = valid;
    selected["selectedTaskId"] = json!("task-remote");
    let selected = reconcile(local, sent, selected).unwrap();
    assert_eq!(selected["baseSelectedTaskId"], "task-remote");
    assert_eq!(selected["selectedTaskId"], "task-remote");
}

#[test]
fn reconcile_rebase_v1_drops_transitive_timer_dependencies_after_non_applied_parent() {
    let mut finish = command("finish-sent", "focus-timer", 1, 1_000);
    finish["type"] = json!("finish");
    let generated_start = command("generated-start", "break-timer", 2, 2_000);
    let mut generated_pause = command("generated-pause", "break-timer", 3, 3_000);
    generated_pause["type"] = json!("pause");

    let mut sent = empty_queues();
    sent["commands"] = json!([finish.clone()]);
    let mut local = empty_queues();
    local["commands"] = json!([finish, generated_start.clone(), generated_pause.clone()]);
    let dependencies = json!([
        {"operationId": "generated-start", "dependsOnOperationId": "finish-sent"},
        {"operationId": "generated-pause", "dependsOnOperationId": "generated-start"}
    ]);

    let mut ignored = canonical_response(&sent);
    ignored["acknowledgements"][0]["outcome"] = json!("ignored");
    ignored["acknowledgements"][0]["reason"] = json!("already completed");
    let ignored =
        reconcile_with_dependencies(local.clone(), sent.clone(), ignored, dependencies.clone())
            .unwrap();
    assert_eq!(ignored["pending"], json!([]));
    assert_eq!(
        ignored["droppedTimerOperationIds"],
        json!(["generated-pause", "generated-start"])
    );

    let mut barrier_sent = empty_queues();
    barrier_sent["commands"] = json!([local["commands"][0].clone(), local["commands"][1].clone()]);
    let mut barrier_response = canonical_response(&barrier_sent);
    barrier_response["acknowledgements"][0]["outcome"] = json!("ignored");
    barrier_response["acknowledgements"][0]["reason"] = json!("already completed");
    let barrier = reconcile_with_dependencies(
        local.clone(),
        barrier_sent,
        barrier_response,
        dependencies.clone(),
    )
    .unwrap();
    assert_eq!(
        barrier["pending"]
            .as_array()
            .unwrap()
            .iter()
            .map(|command| &command["id"])
            .collect::<Vec<_>>(),
        vec![&generated_pause["id"]]
    );
    assert_eq!(barrier["droppedTimerOperationIds"], json!([]));

    let applied =
        reconcile_with_dependencies(local, sent.clone(), canonical_response(&sent), dependencies)
            .unwrap();
    assert_eq!(
        applied["pending"]
            .as_array()
            .unwrap()
            .iter()
            .map(|command| &command["id"])
            .collect::<Vec<_>>(),
        vec![&generated_start["id"], &generated_pause["id"]]
    );
    assert_eq!(applied["droppedTimerOperationIds"], json!([]));
}

#[test]
fn reconcile_rebase_v1_promotes_generated_long_break_after_fourth_canonical_focus() {
    let mut finish = command("finish-sent", "focus-four", 1, 4_000);
    finish["type"] = json!("finish");
    finish["phase"] = json!("focus");
    finish["plannedDurationMs"] = json!(1_500_000);
    let generated_start = command("generated-start", "break-generated", 2, 5_000);
    let mut generated_pause = command("generated-pause", "break-generated", 3, 6_000);
    generated_pause["type"] = json!("pause");
    generated_pause["observedElapsedMs"] = json!(1_200_000);

    let mut sent = empty_queues();
    sent["commands"] = json!([finish.clone()]);
    let mut local = empty_queues();
    local["commands"] = json!([finish, generated_start, generated_pause]);
    let mut response = canonical_response(&sent);
    response["acknowledgements"][0]["outcome"] = json!("ignored");
    response["acknowledgements"][0]["reason"] = json!("already completed");
    response["history"] = json!([
        completed_focus("history-one", "focus-one", "finish-one", 1_000),
        completed_focus("history-two", "focus-two", "finish-two", 2_000),
        completed_focus("history-three", "focus-three", "finish-three", 3_000),
        completed_focus("history-four", "focus-four", "finish-sent", 4_000)
    ]);

    let output =
        reconcile_with_dependencies(local, sent, response, generated_break_dependencies()).unwrap();
    assert_eq!(
        output["promotedTimerOperationIds"],
        json!(["generated-pause", "generated-start"])
    );
    assert_eq!(output["pendingTimerDependencies"], json!([]));
    assert_eq!(output["droppedTimerOperationIds"], json!([]));
    assert_eq!(output["droppedTimerIds"], json!([]));
    for command in output["pending"].as_array().unwrap() {
        assert_eq!(command["phase"], "long_break");
        assert_eq!(command["plannedDurationMs"], 900_000);
        assert!(command["observedElapsedMs"].as_i64().unwrap() <= 900_000);
    }
}

#[test]
fn reconcile_rebase_v1_preserves_a_completed_generated_break_phase_and_duration() {
    let mut finish = command("finish-sent", "focus-four", 1, 4_000);
    finish["type"] = json!("finish");
    finish["phase"] = json!("focus");
    finish["plannedDurationMs"] = json!(1_500_000);
    let generated_start = command("generated-start", "break-generated", 2, 5_000);
    let mut generated_finish = command("generated-pause", "break-generated", 3, 6_000);
    generated_finish["type"] = json!("finish");
    generated_finish["observedElapsedMs"] = json!(300_000);

    let mut sent = empty_queues();
    sent["commands"] = json!([finish.clone()]);
    let mut local = empty_queues();
    local["commands"] = json!([finish, generated_start, generated_finish]);
    let mut response = canonical_response(&sent);
    response["history"] = json!([
        completed_focus("history-one", "focus-one", "finish-one", 1_000),
        completed_focus("history-two", "focus-two", "finish-two", 2_000),
        completed_focus("history-three", "focus-three", "finish-three", 3_000),
        completed_focus("history-four", "focus-four", "finish-sent", 4_000)
    ]);

    let output =
        reconcile_with_dependencies(local, sent, response, generated_break_dependencies()).unwrap();
    for command in output["pending"].as_array().unwrap() {
        assert_eq!(command["phase"], "short_break");
        assert_eq!(command["plannedDurationMs"], 300_000);
    }
}

#[test]
fn reconcile_rebase_v1_drops_generated_break_without_exact_evidence_or_after_manual_start() {
    for include_exact_evidence in [false, true] {
        let mut finish = command("finish-sent", "focus-source", 1, 1_000);
        finish["type"] = json!("finish");
        finish["phase"] = json!("focus");
        finish["plannedDurationMs"] = json!(1_500_000);
        let generated_start = command("generated-start", "break-generated", 2, 2_000);
        let mut generated_pause = command("generated-pause", "break-generated", 3, 3_000);
        generated_pause["type"] = json!("pause");
        let manual_start = command("manual-start", "manual-timer", 4, 4_000);

        let mut sent = empty_queues();
        sent["commands"] = json!([finish.clone()]);
        let mut local = empty_queues();
        local["commands"] = if include_exact_evidence {
            json!([finish, generated_start, generated_pause, manual_start])
        } else {
            json!([finish, generated_start, generated_pause])
        };
        let mut response = canonical_response(&sent);
        if include_exact_evidence {
            response["history"] = json!([completed_focus(
                "history-source",
                "focus-source",
                "finish-sent",
                1_000
            )]);
        }

        let output =
            reconcile_with_dependencies(local, sent, response, generated_break_dependencies())
                .unwrap();
        assert_eq!(
            output["droppedTimerOperationIds"],
            json!(["generated-pause", "generated-start"])
        );
        assert_eq!(output["droppedTimerIds"], json!(["break-generated"]));
        assert_eq!(output["promotedTimerOperationIds"], json!([]));
        assert_eq!(output["pendingTimerDependencies"], json!([]));
        if include_exact_evidence {
            assert_eq!(output["pending"][0]["id"], "manual-start");
        } else {
            assert_eq!(output["pending"], json!([]));
        }
    }
}

#[test]
fn reconcile_rebase_v1_retains_unresolved_generated_break_context() {
    let mut finish = command("finish-sent", "focus-source", 1, 1_000);
    finish["type"] = json!("finish");
    finish["phase"] = json!("focus");
    finish["plannedDurationMs"] = json!(1_500_000);
    let generated_start = command("generated-start", "break-generated", 2, 2_000);
    let mut generated_pause = command("generated-pause", "break-generated", 3, 3_000);
    generated_pause["type"] = json!("pause");
    let mut local = empty_queues();
    local["commands"] = json!([finish, generated_start, generated_pause]);
    let sent = empty_queues();

    let dependencies = generated_break_dependencies();
    let output = reconcile_with_dependencies(
        local,
        sent.clone(),
        canonical_response(&sent),
        dependencies.clone(),
    )
    .unwrap();
    assert_eq!(output["pendingTimerDependencies"], dependencies);
    assert_eq!(output["promotedTimerOperationIds"], json!([]));
    assert_eq!(output["droppedTimerOperationIds"], json!([]));
}

#[test]
fn reconcile_rebase_v1_rebases_each_retained_queue_after_the_server_clock() {
    let sent = empty_queues();
    let mut local = json!({
        "commands": [command("command-stale", "timer-stale", 1, 1_000)],
        "taskOperations": [task_operation("task-stale", "task-stale", "Task", 1_000)],
        "durationOperations": [duration_operation("duration-stale", "focus", 1_800_000, 1_000)],
        "autoStartOperations": [auto_start_operation("auto-stale", true, 1_000)],
        "selectedTaskOperations": [selected_task_operation("selected-stale", None, 1_000)]
    });
    local["commands"][0]["occurredAt"] = json!("2026-07-20T11:53:20Z");
    local["commands"][0]["hlcWallMs"] = json!(SERVER_WALL_MS - 400_000);
    let mut response = canonical_response(&sent);
    response["serverHlcCounter"] = json!(7);

    let output = reconcile(local, sent, response).unwrap();
    for field in [
        "pending",
        "pendingTaskOperations",
        "pendingDurationOperations",
        "pendingAutoStartOperations",
        "pendingSelectedTaskOperations",
    ] {
        assert_eq!(output[field][0]["hlcWallMs"], SERVER_WALL_MS + 10_000);
        assert_eq!(output[field][0]["hlcCounter"], 8);
    }
    assert_eq!(
        output["pending"][0]["occurredAt"],
        "2026-07-20T12:00:10.000Z"
    );
    assert_eq!(
        output["pendingTaskOperations"][0]["occurredAt"],
        timestamp(1_000)
    );
}

#[test]
fn reconcile_rebase_v1_rejects_a_queue_when_the_server_clock_has_no_headroom() {
    let sent = empty_queues();
    let mut local = empty_queues();
    local["commands"] = json!([command("command-stale", "timer-stale", 1, 1_000)]);
    let mut response = canonical_response(&sent);
    response["serverHlcWallMs"] = json!(SERVER_WALL_MS + 310_000);
    response["serverHlcCounter"] = json!(9_007_199_254_740_991_i64);

    let error = reconcile(local, sent, response).unwrap_err();
    assert!(error.contains("headroom"), "unexpected error: {error}");
}

#[test]
fn reconcile_rebase_v1_rejects_invalid_timer_dependency_graphs() {
    let sent = empty_queues();
    let mut local = empty_queues();
    local["commands"] = json!([
        command("first", "timer", 1, 1_000),
        command("second", "timer", 2, 2_000)
    ]);
    for dependencies in [
        json!([{"operationId": "missing", "dependsOnOperationId": "first"}]),
        json!([
            {"operationId": "first", "dependsOnOperationId": "second"},
            {"operationId": "second", "dependsOnOperationId": "first"}
        ]),
    ] {
        assert!(
            reconcile_with_dependencies(
                local.clone(),
                sent.clone(),
                canonical_response(&sent),
                dependencies,
            )
            .is_err()
        );
    }
}

#[test]
fn reconcile_rebase_v1_rejects_duplicate_timer_ids_and_invalid_generated_breaks() {
    let sent = empty_queues();
    let response = canonical_response(&sent);

    let duplicate = command("duplicate", "timer", 1, 1_000);
    let mut duplicate_local = empty_queues();
    duplicate_local["commands"] = json!([duplicate.clone(), duplicate]);
    assert!(reconcile(duplicate_local, sent.clone(), response.clone()).is_err());

    let mut finish = command("finish", "focus-timer", 1, 1_000);
    finish["type"] = json!("finish");
    finish["phase"] = json!("focus");
    let generated = command("generated", "break-timer", 2, 2_000);
    let mut local = empty_queues();
    local["commands"] = json!([finish.clone(), generated.clone()]);

    for dependency in [
        json!({
            "operationId": "generated",
            "dependsOnOperationId": "finish",
            "generatedBreak": false,
            "sourceDayStart": "2026-07-20T00:00:00Z",
            "sourceDayEnd": "2026-07-21T00:00:00Z"
        }),
        json!({
            "operationId": "generated",
            "dependsOnOperationId": "finish",
            "generatedBreak": true
        }),
        json!({
            "operationId": "generated",
            "dependsOnOperationId": "finish",
            "generatedBreak": true,
            "sourceDayStart": "not-a-time",
            "sourceDayEnd": "2026-07-21T00:00:00Z"
        }),
        json!({
            "operationId": "generated",
            "dependsOnOperationId": "finish",
            "generatedBreak": true,
            "sourceDayStart": "2026-07-21T00:00:00Z",
            "sourceDayEnd": "2026-07-20T00:00:00Z"
        }),
    ] {
        assert!(
            reconcile_with_dependencies(
                local.clone(),
                sent.clone(),
                response.clone(),
                json!([dependency]),
            )
            .is_err()
        );
    }

    let mut wrong_parent = finish.clone();
    wrong_parent["type"] = json!("start");
    let mut wrong_parent_local = empty_queues();
    wrong_parent_local["commands"] = json!([wrong_parent, generated.clone()]);
    assert!(
        reconcile_with_dependencies(
            wrong_parent_local,
            sent.clone(),
            response.clone(),
            json!([{
                "operationId": "generated",
                "dependsOnOperationId": "finish",
                "generatedBreak": true,
                "sourceDayStart": "2026-07-20T00:00:00Z",
                "sourceDayEnd": "2026-07-21T00:00:00Z"
            }]),
        )
        .is_err()
    );

    let second_generated = command("generated-second", "break-timer-2", 3, 3_000);
    let mut duplicate_source_local = empty_queues();
    duplicate_source_local["commands"] = json!([finish, generated, second_generated]);
    assert!(
        reconcile_with_dependencies(
            duplicate_source_local,
            sent,
            response,
            json!([
                {
                    "operationId": "generated",
                    "dependsOnOperationId": "finish",
                    "generatedBreak": true,
                    "sourceDayStart": "2026-07-20T00:00:00Z",
                    "sourceDayEnd": "2026-07-21T00:00:00Z"
                },
                {
                    "operationId": "generated-second",
                    "dependsOnOperationId": "finish",
                    "generatedBreak": true,
                    "sourceDayStart": "2026-07-20T00:00:00Z",
                    "sourceDayEnd": "2026-07-21T00:00:00Z"
                }
            ]),
        )
        .is_err()
    );
}

#[test]
fn reconcile_rebase_v1_rejects_unsafe_retained_timer_values() {
    let sent = empty_queues();
    let mut local = empty_queues();
    let mut unsafe_command = command("unsafe", "timer", 1, 1_000);
    unsafe_command["plannedDurationMs"] = json!(i64::MAX);
    unsafe_command["observedElapsedMs"] = json!(i64::MAX);
    local["commands"] = json!([unsafe_command]);
    assert!(reconcile(local, sent.clone(), canonical_response(&sent)).is_err());
}

#[test]
fn reconcile_rebase_v1_rejects_canonical_timer_history_identity_overlap() {
    let sent = empty_queues();
    let mut response = canonical_response(&sent);
    response["canonicalTimer"] = json!({
        "id": "timer-overlap",
        "phase": "focus",
        "status": "running",
        "plannedDurationMs": 60_000,
        "elapsedAtAnchorMs": 0,
        "anchorAt": timestamp(0)
    });
    response["history"] = json!([{
        "id": "history-overlap",
        "timerId": "timer-overlap",
        "phase": "focus",
        "status": "completed",
        "plannedDurationMs": 60_000,
        "completedAt": timestamp(0)
    }]);
    assert!(reconcile(empty_queues(), sent, response).is_err());
}

#[test]
fn reconcile_rebase_v1_rejects_duplicate_local_ids_in_every_operation_domain() {
    let sent = empty_queues();
    for (field, operation) in [
        (
            "taskOperations",
            task_operation("duplicate", "task", "Task", 1_000),
        ),
        (
            "durationOperations",
            duration_operation("duplicate", "focus", 1_800_000, 1_000),
        ),
        (
            "autoStartOperations",
            auto_start_operation("duplicate", true, 1_000),
        ),
        (
            "selectedTaskOperations",
            selected_task_operation("duplicate", None, 1_000),
        ),
    ] {
        let mut local = empty_queues();
        local[field] = json!([operation.clone(), operation]);
        assert!(
            reconcile(local, sent.clone(), canonical_response(&sent)).is_err(),
            "duplicate {field} was accepted"
        );
    }
}

#[test]
fn reconcile_rebase_v1_rejects_unsafe_revision_and_hlc_values_without_overflow() {
    let local = empty_queues();
    let sent = empty_queues();
    for (field, value) in [
        ("revision", json!(9_007_199_254_740_992_i64)),
        ("serverHlcWallMs", json!(i64::MAX)),
        ("serverHlcCounter", json!(9_007_199_254_740_992_i64)),
    ] {
        let mut invalid = canonical_response(&sent);
        invalid[field] = value;
        assert!(
            reconcile(local.clone(), sent.clone(), invalid).is_err(),
            "unsafe {field} was accepted"
        );
    }
}

#[test]
fn reconcile_rebase_v1_installs_canonical_base_and_replays_all_retained_domains() {
    let sent = one_of_each();
    let retained = json!({
        "commands": [command("command-retained", "timer-retained", 2, 5_000)],
        "taskOperations": [task_operation("task-retained-op", "task-retained", "Beta", 6_000)],
        "durationOperations": [duration_operation("duration-retained", "long_break", 1_200_000, 6_000)],
        "autoStartOperations": [auto_start_operation("auto-retained", false, 6_000)],
        "selectedTaskOperations": [selected_task_operation("selected-retained", Some("task-retained"), 6_000)]
    });
    let mut local = Map::new();
    for field in [
        "commands",
        "taskOperations",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ] {
        let mut operations = sent[field].as_array().unwrap().clone();
        operations.extend(retained[field].as_array().unwrap().clone());
        local.insert(field.to_owned(), Value::Array(operations));
    }
    let mut response = canonical_response(&sent);
    response["canonicalTimer"] = json!({
        "id": "timer-remote",
        "taskId": "task-remote",
        "phase": "focus",
        "status": "running",
        "plannedDurationMs": 60_000,
        "elapsedAtAnchorMs": 0,
        "anchorAt": timestamp(0),
        "startedByDeviceId": "device-remote",
        "lastIntent": {
            "type": "start",
            "commandId": "remote-start",
            "occurredAt": timestamp(0)
        }
    });
    response["tasks"] = json!([{"id": "task-remote", "title": "Gamma"}]);
    response["durationsMs"] = json!({
        "focus": 1_800_000,
        "short_break": 300_000,
        "long_break": 900_000
    });
    response["autoStartBreaks"] = json!(true);
    response["selectedTaskId"] = json!("task-remote");

    let output = reconcile(Value::Object(local), sent, response).unwrap();
    assert_eq!(output["revision"], 9);
    assert_eq!(output["baseTimer"]["id"], "timer-remote");
    assert_eq!(
        output["baseTasks"],
        json!([{"id": "task-remote", "title": "Gamma"}])
    );
    assert_eq!(output["baseDurationsMs"]["long_break"], 900_000);
    assert_eq!(output["baseAutoStartBreaks"], true);
    assert_eq!(output["baseSelectedTaskId"], "task-remote");

    assert_eq!(output["timer"]["id"], "timer-retained");
    assert_eq!(output["history"][0]["timerId"], "timer-remote");
    assert_eq!(output["history"][0]["status"], "superseded");
    assert_eq!(
        output["tasks"],
        json!([
            {"id": "task-retained", "title": "Beta"},
            {"id": "task-remote", "title": "Gamma"}
        ])
    );
    assert_eq!(output["durationsMs"]["focus"], 1_800_000);
    assert_eq!(output["durationsMs"]["long_break"], 1_200_000);
    assert_eq!(output["autoStartBreaks"], false);
    assert_eq!(output["selectedTaskId"], "task-retained");
}

#[test]
fn reconcile_rebase_v1_matches_convergence_response_case() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/convergence-v1.json")).unwrap();
    let fixture_case = &fixture["responseCases"][0];

    let commands = fixture_case["local"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            json!({
                "id": item["id"],
                "deviceId": item["deviceId"],
                "deviceSequence": item["sequence"],
                "timerId": item["timerId"],
                "taskId": item.get("taskId").cloned().unwrap_or(Value::Null),
                "type": item["type"],
                "phase": item["phase"],
                "plannedDurationMs": item["durationMs"],
                "occurredAt": timestamp(item["atMs"].as_i64().unwrap()),
                "hlcWallMs": item["wallMs"],
                "hlcCounter": item["counter"],
                "observedElapsedMs": item["elapsedMs"]
            })
        })
        .collect::<Vec<_>>();
    let production_operation = |item: &Value| {
        let mut output = item.as_object().unwrap().clone();
        let at_ms = output.remove("atMs").unwrap().as_i64().unwrap();
        let wall_ms = output.remove("wallMs").unwrap();
        let counter = output.remove("counter").unwrap();
        output.insert("occurredAt".to_owned(), json!(timestamp(at_ms)));
        output.insert("hlcWallMs".to_owned(), wall_ms);
        output.insert("hlcCounter".to_owned(), counter);
        Value::Object(output)
    };
    let operations = |field: &str| {
        fixture_case["local"][field]
            .as_array()
            .unwrap()
            .iter()
            .map(production_operation)
            .collect::<Vec<_>>()
    };
    let local = json!({
        "commands": commands,
        "taskOperations": operations("taskOperations"),
        "durationOperations": operations("durationOperations"),
        "autoStartOperations": operations("autoStartOperations"),
        "selectedTaskOperations": []
    });
    let sent = json!({
        "commands": local["commands"].as_array().unwrap().iter().filter(|item|
            fixture_case["sentIds"]["commands"].as_array().unwrap().contains(&item["id"])
        ).cloned().collect::<Vec<_>>(),
        "taskOperations": local["taskOperations"].as_array().unwrap().iter().filter(|item|
            fixture_case["sentIds"]["taskOperations"].as_array().unwrap().contains(&item["id"])
        ).cloned().collect::<Vec<_>>(),
        "durationOperations": local["durationOperations"].as_array().unwrap().iter().filter(|item|
            fixture_case["sentIds"]["durationOperations"].as_array().unwrap().contains(&item["id"])
        ).cloned().collect::<Vec<_>>(),
        "autoStartOperations": local["autoStartOperations"].as_array().unwrap().iter().filter(|item|
            fixture_case["sentIds"]["autoStartOperations"].as_array().unwrap().contains(&item["id"])
        ).cloned().collect::<Vec<_>>(),
        "selectedTaskOperations": []
    });
    let response_acknowledgements = |field: &str, id_field: &str| {
        fixture_case["acknowledgements"][field]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| {
                json!({
                    (id_field): item["id"],
                    "outcome": item["outcome"],
                    "reason": item["reason"]
                })
            })
            .collect::<Vec<_>>()
    };
    let canonical = &fixture_case["canonical"];
    let timer = &canonical["timer"];
    let response = json!({
        "acknowledgements": response_acknowledgements("commands", "commandId"),
        "taskAcknowledgements": response_acknowledgements("taskOperations", "operationId"),
        "durationAcknowledgements": response_acknowledgements("durationOperations", "operationId"),
        "autoStartAcknowledgements": response_acknowledgements("autoStartOperations", "operationId"),
        "selectedTaskAcknowledgements": [],
        "revision": 1,
        "canonicalTimer": {
            "id": timer["id"],
            "taskId": timer["taskId"],
            "phase": timer["phase"],
            "status": timer["status"],
            "plannedDurationMs": timer["durationMs"],
            "elapsedAtAnchorMs": timer["elapsedMs"],
            "anchorAt": timestamp(timer["anchorMs"].as_i64().unwrap()),
            "startedByDeviceId": "device-a",
            "lastIntent": {
                "type": "start",
                "commandId": timer["lastCommandId"],
                "occurredAt": timestamp(timer["anchorMs"].as_i64().unwrap())
            }
        },
        "history": canonical["history"],
        "tasks": canonical["tasks"],
        "durationsMs": canonical["durationsMs"],
        "autoStartBreaks": canonical["autoStartBreaks"],
        "selectedTaskId": null,
        "serverTime": timestamp(5_000),
        "serverHlcWallMs": SERVER_WALL_MS + 5_000,
        "serverHlcCounter": 0
    });

    let output = reconcile(local, sent, response).unwrap();
    let expected = &fixture_case["expected"];
    assert_eq!(
        output["pending"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| &item["id"])
            .collect::<Vec<_>>(),
        expected["commandIds"]
            .as_array()
            .unwrap()
            .iter()
            .collect::<Vec<_>>()
    );
    assert_eq!(output["timer"]["id"], expected["timer"]["id"]);
    assert_eq!(output["timer"]["status"], expected["timer"]["status"]);
    assert_eq!(
        output["history"][0]["timerId"],
        expected["history"][0]["timerId"]
    );
    assert_eq!(output["tasks"], expected["tasks"]);
    assert_eq!(output["durationsMs"], expected["durationsMs"]);
    assert_eq!(output["autoStartBreaks"], expected["autoStartBreaks"]);
}

#[test]
fn bootstrap_plan_v1_covers_keep_remote_replace_remote_merge_and_choice() {
    let completed = json!({
        "id": "history-local",
        "timerId": "timer-local",
        "status": "completed"
    });
    let remote = json!({
        "id": "history-remote",
        "timerId": "timer-remote",
        "status": "completed"
    });
    let cases = [
        (
            json!({"localHistory": [], "remoteHistory": [], "hasLocalState": false, "hasRemoteState": false}),
            json!({"mode": "auto", "strategy": "keep_remote", "reason": "empty"}),
        ),
        (
            json!({"localHistory": [], "remoteHistory": [], "hasLocalState": true, "hasRemoteState": false}),
            json!({"mode": "auto", "strategy": "merge", "reason": "local_state_only"}),
        ),
        (
            json!({"localHistory": [completed.clone()], "remoteHistory": [], "hasLocalState": true, "hasRemoteState": false}),
            json!({"mode": "auto", "strategy": "replace_remote", "reason": "local_only"}),
        ),
        (
            json!({"localHistory": [], "remoteHistory": [remote.clone()], "hasLocalState": false, "hasRemoteState": true}),
            json!({"mode": "auto", "strategy": "keep_remote", "reason": "remote_only"}),
        ),
        (
            json!({"localHistory": [completed], "remoteHistory": [remote], "hasLocalState": true, "hasRemoteState": true}),
            json!({"mode": "choose", "localHistoryCount": 1, "remoteHistoryCount": 1}),
        ),
        (
            json!({"localOwnerId": "user-a", "currentUserId": "user-a"}),
            json!({"mode": "normal_sync", "reason": "same_owner"}),
        ),
        (
            json!({"localOwnerId": "user-a", "currentUserId": "user-b"}),
            json!({"mode": "auto", "strategy": "keep_remote", "reason": "different_owner"}),
        ),
    ];

    for (input, expected) in cases {
        let encoded = dispatch_json("bootstrap.plan.v1", &input.to_string()).unwrap();
        let output: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(output, expected);
        assert_eq!(
            dispatch_json("bootstrap.plan.v1", &input.to_string()).unwrap(),
            encoded
        );
    }
}

#[test]
fn bootstrap_plan_v1_counts_only_well_formed_completed_history() {
    let output: Value = serde_json::from_str(
        &dispatch_json(
            "bootstrap.plan.v1",
            &json!({
                "localHistory": [
                    {"id": "missing-status"},
                    {"id": "null-status", "status": null},
                    {"id": "wrong-status-type", "status": 1},
                    {"status": "completed"},
                    "not-an-object"
                ],
                "hasLocalState": false,
                "hasRemoteState": false
            })
            .to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(output["strategy"], "keep_remote");
    assert_eq!(output["reason"], "empty");
}

#[test]
fn bootstrap_plan_v1_rejects_an_owned_local_state_without_current_user_identity() {
    let error = dispatch_json(
        "bootstrap.plan.v1",
        &json!({"localOwnerId": "user-a"}).to_string(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("currentUserId"));
}
