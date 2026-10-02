use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const WIRE: &str = "2026-07-20T12:00:10Z";
const PHYSICAL: &str = "2026-07-20T12:00:15Z";
const TASK: &str = "12345678-1234-4234-8234-123456789010";

fn call(operation: &str, input: &Value) -> Result<Value, String> {
    dispatch_json(operation, &input.to_string())
        .map(|output| serde_json::from_str(&output).unwrap())
        .map_err(|error| error.to_string())
}

fn fixture(name: &str) -> Value {
    let content = match name {
        "intent" => include_str!("../fixtures/workspace-intent-v1.json"),
        _ => include_str!("../fixtures/workspace-terminal-v1.json"),
    };
    serde_json::from_str(content).unwrap()
}

fn originating_finish(profile: &str) -> (Value, Value) {
    let mut input = fixture("intent")["request"].clone();
    input.as_object_mut().unwrap().remove("intent");
    input["stage"] = json!("finishCommit");
    input["compatibility"] = json!(profile);
    input["ownership"] = Value::Null;
    input["identities"]["commandUuids"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    let mut timer = fixture("intent")["timer"].clone();
    timer["taskId"] = json!(TASK);
    input["workspace"]["base"]["canonicalTimer"] = timer.clone();
    input["requestedTimer"] = timer;
    input["clock"]["physicalNow"] = json!(PHYSICAL);
    input["clock"]["observedAt"] = json!(PHYSICAL);
    let output = call("workspace.completionMutation.v1", &input).unwrap();
    assert!(output["commands"][0].get("taskId").is_none());
    (input, output)
}

fn install_wire_result(output: &Value) -> Value {
    let original = &output["workspace"];
    let wire = call(
        "projection.apply.v2",
        &json!({"base": original["base"],
        "pending": original["local"], "now": WIRE}),
    )
    .unwrap();
    let mut workspace = original.clone();
    workspace["base"]["canonicalTimer"] = wire["canonicalTimer"].clone();
    workspace["base"]["history"] = wire["history"].clone();
    workspace["neverSent"] = json!({});
    workspace
}

fn projection(workspace: &Value) -> Result<Value, String> {
    let mut request = workspace.clone();
    request["now"] = json!(PHYSICAL);
    call("workspace.project.v1", &request).map(|output| output["workspace"].clone())
}

fn intent(workspace: &Value, observation: &Value, profile: &str) -> Value {
    let mut input = fixture("intent")["request"].clone();
    input["workspace"] = workspace.clone();
    input["observation"] = observation.clone();
    input["compatibility"] = json!(profile);
    input["intent"] = json!({"kind": "pause"});
    input["clock"]["physicalNow"] = json!(PHYSICAL);
    input["clock"]["observedAt"] = json!(PHYSICAL);
    input
}

fn lifecycle(workspace: &Value, observation: &Value, profile: &str) -> Value {
    let mut input = intent(workspace, observation, profile);
    input.as_object_mut().unwrap().remove("intent");
    input["stage"] = json!("deferredBreakOpportunity");
    input["ownership"] = Value::Null;
    input["lifecycle"] = json!({"consumedCompletions": [], "pendingBreaks": []});
    input["centralizedSession"] = json!({"userId": null, "authenticated": false});
    input["event"] = json!({"kind": "opportunity"});
    if profile == "appleWorkspace" {
        input["replicationMode"] = json!("iroh");
        input["previousWorkspace"] = workspace.clone();
        input["previousObservation"] = observation.clone();
    }
    input
}

#[test]
fn checker_originating_task_attributed_finish_composes_with_full_uncertain_queue() {
    let (_, output) = originating_finish("desktopStorage");
    let workspace = install_wire_result(&output);
    let read = json!({"profile": "desktopStorage", "source": {"kind": "workspace", "value": workspace},
        "selectedPhase": "focus", "observedAt": PHYSICAL,
        "calendarIntervals": fixture("intent")["request"]["calendarIntervals"]});
    let observed = json!({"canonicalAnchorAt": null, "commandTimes": {}});
    let results = [
        projection(&workspace),
        call("workspace.readModel.v1", &read),
        call(
            "workspace.intent.v1",
            &intent(&workspace, &observed, "desktopStorage"),
        ),
        call(
            "workspace.completionMutation.v1",
            &lifecycle(&workspace, &observed, "desktopStorage"),
        ),
    ];
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    assert_eq!(
        projection(&workspace).unwrap()["history"],
        workspace["base"]["history"]
    );
    assert_eq!(
        workspace["local"]["commands"],
        output["workspace"]["local"]["commands"]
    );
}

fn command(id: &str, kind: &str, sequence: i64) -> Value {
    let mut command = fixture("terminal")["command"].clone();
    command["id"] = json!(id);
    command["type"] = json!(kind);
    command["deviceSequence"] = json!(sequence);
    command["hlcCounter"] = json!(sequence);
    command["occurredAt"] = json!(WIRE);
    command
}

fn ignored_field_cases() -> Vec<Value> {
    fixture("terminal")["ignoredCommandFields"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn checker_ten_ignored_field_variants_follow_real_finish_and_cancel_reducers() {
    let mut failures = Vec::new();
    for kind in ["finish", "cancel"] {
        for (index, fields) in ignored_field_cases().iter().enumerate() {
            let mut start = command("origin-start", "start", 1);
            start["occurredAt"] = json!("2026-07-20T12:00:00Z");
            let mut terminal = command("origin-terminal", kind, 2);
            for (field, value) in fields.as_object().unwrap() {
                if field == "omitTask" {
                    terminal.as_object_mut().unwrap().remove("taskId");
                } else {
                    terminal[field] = value.clone();
                }
            }
            let commands = json!([start, terminal]);
            let reduced = call(
                "timer.reduce.v1",
                &json!({"commands": commands, "now": WIRE}),
            )
            .unwrap();
            let mut workspace = fixture("terminal")["request"].clone();
            workspace.as_object_mut().unwrap().remove("now");
            workspace["base"]["canonicalTimer"] = reduced["canonicalTimer"].clone();
            workspace["base"]["history"] = reduced["history"].clone();
            workspace["local"]["commands"] = commands;
            match projection(&workspace) {
                Ok(result) => {
                    assert_eq!(result["canonicalTimer"], reduced["canonicalTimer"]);
                    assert_eq!(result["history"], reduced["history"]);
                }
                Err(error) => failures.push(format!("{kind}/{index}: {error}")),
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn checker_physical_observation_preserves_raw_wire_pair_and_originating_queue() {
    let mut failures = Vec::new();
    for profile in ["appleWorkspace", "desktopStorage"] {
        let (_, output) = originating_finish(profile);
        let retained = install_wire_result(&output);
        for empty in [false, true] {
            let mut workspace = retained.clone();
            let observation = if empty {
                workspace["local"]["commands"] = json!([]);
                json!({"canonicalAnchorAt": PHYSICAL, "commandTimes": {}})
            } else {
                output["observation"].clone()
            };
            for operation in ["workspace.intent.v1", "workspace.completionMutation.v1"] {
                let request = if operation == "workspace.intent.v1" {
                    intent(&workspace, &observation, profile)
                } else {
                    lifecycle(&workspace, &observation, profile)
                };
                match call(operation, &request) {
                    Ok(result) => {
                        assert_eq!(result["workspace"], workspace);
                        assert_eq!(result["observation"], observation);
                        assert_eq!(result["projection"]["canonicalTimer"]["anchorAt"], PHYSICAL);
                        assert_eq!(
                            result["projection"]["history"],
                            workspace["base"]["history"]
                        );
                    }
                    Err(error) => failures.push(format!("{profile}/{empty}/{operation}: {error}")),
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn checker_invalid_standalone_terminal_fails_before_history_synthesis() {
    let mut accepted = Vec::new();
    for variant in ["underelapsed", "clear-intent"] {
        let mut workspace = fixture("terminal")["request"].clone();
        workspace["base"]["canonicalTimer"] = fixture("terminal")["timer"].clone();
        if variant == "underelapsed" {
            workspace["base"]["canonicalTimer"]["elapsedAtAnchorMs"] = json!(59999);
        } else {
            workspace["base"]["canonicalTimer"]["lastIntent"]["type"] = json!("clear");
        }
        if projection(&workspace).is_ok() {
            accepted.push(variant);
        }
    }
    assert!(accepted.is_empty(), "accepted standalone {accepted:?}");
}

#[test]
fn checker_supersession_follows_hlc_order_despite_occurrence_rollback() {
    let mut first = command("first-start", "start", 1);
    first["occurredAt"] = json!("2026-07-20T12:00:00Z");
    let mut second = command("second-start", "start", 2);
    second["timerId"] = json!("replacement-timer");
    second["occurredAt"] = json!("2026-07-20T11:59:59Z");
    let commands = json!([first, second]);
    let reduced = call(
        "timer.reduce.v1",
        &json!({"commands": commands, "now": WIRE}),
    )
    .unwrap();
    let restored = call(
        "timer.replay.page.v1",
        &json!({"sessions": reduced["sessions"], "currentTimerId": "existing-timer",
        "commands": [], "after": null, "now": WIRE}),
    )
    .unwrap();
    let mut workspace = fixture("terminal")["request"].clone();
    workspace["base"]["canonicalTimer"] = restored["canonicalTimer"].clone();
    workspace["base"]["history"] = restored["history"].clone();
    workspace["local"]["commands"] = commands;
    let projected = projection(&workspace).unwrap();
    assert_eq!(projected["canonicalTimer"], restored["canonicalTimer"]);
    assert_eq!(projected["history"], restored["history"]);
}

#[test]
fn checker_workspace_preserves_native_intent_device_metadata() {
    let mut workspace = fixture("terminal")["request"].clone();
    workspace["base"]["canonicalTimer"] = fixture("terminal")["timer"].clone();
    workspace["base"]["canonicalTimer"]["lastIntent"]["deviceId"] = json!("device-origin");
    workspace["base"]["history"] = json!([fixture("terminal")["history"]]);
    let projected = projection(&workspace).unwrap();
    assert_eq!(
        projected["canonicalTimer"],
        workspace["base"]["canonicalTimer"]
    );
    let mut retained = command("finish-existing", "finish", 1);
    retained["occurredAt"] = workspace["base"]["canonicalTimer"]["anchorAt"].clone();
    workspace["local"]["commands"] = json!([retained]);
    workspace["neverSent"]["commands"] = json!(["finish-existing"]);
    assert_eq!(
        projection(&workspace).unwrap()["canonicalTimer"],
        projected["canonicalTimer"]
    );
}

fn apply_ignored_fields(command: &mut Value, fields: &Value) {
    for (field, value) in fields.as_object().unwrap() {
        if field == "omitTask" {
            command.as_object_mut().unwrap().remove("taskId");
        } else {
            command[field] = value.clone();
        }
    }
}

#[test]
fn checker_pause_resume_provenance_ignores_unused_task_phase_and_duration() {
    for kind in ["pause", "resume"] {
        for fields in ignored_field_cases() {
            let mut start = command("origin-start", "start", 1);
            start["occurredAt"] = json!("2026-07-20T12:00:00Z");
            let mut prior = command("origin-prior", kind, 2);
            apply_ignored_fields(&mut prior, &fields);
            let mut replacement = command("replacement-start", "start", 3);
            replacement["timerId"] = json!("replacement-timer");
            let commands = json!([start, prior, replacement]);
            let reduced = call(
                "timer.reduce.v1",
                &json!({"commands": commands, "now": WIRE}),
            )
            .unwrap();
            let restored = call(
                "timer.replay.page.v1",
                &json!({"sessions": reduced["sessions"], "currentTimerId": "existing-timer",
                "commands": [], "after": null, "now": WIRE}),
            )
            .unwrap();
            let mut workspace = fixture("terminal")["request"].clone();
            workspace["base"]["canonicalTimer"] = restored["canonicalTimer"].clone();
            workspace["base"]["history"] = restored["history"].clone();
            workspace["local"]["commands"] = commands;
            let projected = projection(&workspace).unwrap();
            assert_eq!(projected["canonicalTimer"], restored["canonicalTimer"]);
            assert_eq!(projected["history"], restored["history"]);
        }
    }
}

#[test]
fn checker_every_accepted_zero_command_terminal_aggregate_reopens_exactly() {
    let mut accepted = 0;
    let mut rejected = 0;
    for status in ["running", "paused", "completed", "cancelled", "superseded"] {
        for elapsed in [0, 59999, 60000] {
            for kind in [
                "none", "start", "pause", "resume", "finish", "cancel", "clear", "unknown",
            ] {
                let mut workspace = fixture("terminal")["request"].clone();
                let mut timer = fixture("terminal")["timer"].clone();
                timer["status"] = json!(status);
                timer["elapsedAtAnchorMs"] = json!(elapsed);
                timer["lastIntent"] = if kind == "none" {
                    Value::Null
                } else {
                    json!({"type": kind, "commandId": "source-command", "occurredAt": timer["anchorAt"], "deviceId": "native-device"})
                };
                workspace["base"]["canonicalTimer"] = timer;
                match projection(&workspace) {
                    Ok(first) => {
                        workspace["base"]["canonicalTimer"] = first["canonicalTimer"].clone();
                        workspace["base"]["history"] = first["history"].clone();
                        assert_eq!(
                            projection(&workspace).unwrap(),
                            first,
                            "{status}/{elapsed}/{kind}"
                        );
                        accepted += 1;
                    }
                    Err(_) => rejected += 1,
                }
            }
        }
    }
    assert_eq!((accepted, rejected), (60, 60));
    assert_eq!(accepted + rejected, 120);
}

#[test]
fn checker_proof_retirement_keeps_complete_originating_queue_and_native_metadata() {
    let (_, output) = originating_finish("desktopStorage");
    let mut workspace = install_wire_result(&output);
    let finish = workspace["local"]["commands"][0].clone();
    workspace["base"]["canonicalTimer"]["lastIntent"]["deviceId"] = finish["deviceId"].clone();
    workspace["canonicalHead"] =
        json!({"wallMs": finish["hlcWallMs"], "counter": finish["hlcCounter"]});
    workspace["neverSent"] = output["workspace"]["neverSent"].clone();
    let proven = projection(&workspace).unwrap();
    workspace["neverSent"] = json!({});
    assert_eq!(projection(&workspace).unwrap(), proven);
    assert_eq!(workspace["local"], output["workspace"]["local"]);
    assert_eq!(
        proven["canonicalTimer"],
        workspace["base"]["canonicalTimer"]
    );
}

#[test]
fn checker_physical_observation_cannot_mask_corrupt_wire_provenance() {
    let (_, output) = originating_finish("desktopStorage");
    let mut workspace = install_wire_result(&output);
    workspace["base"]["canonicalTimer"]["anchorAt"] = json!("2026-07-20T12:00:14Z");
    let observation = json!({"canonicalAnchorAt": WIRE, "commandTimes": {}});
    for operation in ["workspace.intent.v1", "workspace.completionMutation.v1"] {
        let input = if operation == "workspace.intent.v1" {
            intent(&workspace, &observation, "desktopStorage")
        } else {
            lifecycle(&workspace, &observation, "desktopStorage")
        };
        assert!(
            call(operation, &input)
                .unwrap_err()
                .contains("conflicting workspace terminal")
        );
    }
}

#[test]
fn checker_native_device_contract_is_workspace_only_and_generated_intents_need_no_mapper() {
    let mut workspace = fixture("terminal")["request"].clone();
    workspace["base"]["canonicalTimer"] = fixture("terminal")["timer"].clone();
    for device in [json!(""), json!(42), json!({"ignoredExtension": true})] {
        workspace["base"]["canonicalTimer"]["lastIntent"]["deviceId"] = device;
        assert!(
            projection(&workspace)
                .unwrap_err()
                .contains("invalid native intent device identity")
        );
        let legacy = call("timer.reduce.v1", &json!({"canonicalTimer": workspace["base"]["canonicalTimer"], "commands": [], "now": WIRE})).unwrap();
        assert!(
            legacy["canonicalTimer"]["lastIntent"]
                .get("deviceId")
                .is_none()
        );
    }
    workspace["base"]["canonicalTimer"] = Value::Null;
    workspace["local"]["commands"] = json!([command("generated-start", "start", 1)]);
    workspace["neverSent"]["commands"] = json!(["generated-start"]);
    let result = projection(&workspace).unwrap();
    assert_eq!(
        result["canonicalTimer"]["lastIntent"]["deviceId"],
        "device-local"
    );
}

#[test]
fn checker_legacy_reducer_keeps_ignoring_duplicate_native_extension_fields() {
    let raw = r#"{"commands":[],"canonicalTimer":{"id":"legacy","phase":"focus","status":"paused","plannedDurationMs":60000,"elapsedAtAnchorMs":1000,"anchorAt":"2026-07-20T12:00:00Z","lastIntent":{"type":"pause","commandId":"pause","occurredAt":"2026-07-20T12:00:00Z","deviceId":"old","deviceId":{"ignored":true}}},"now":"2026-07-20T12:00:10Z"}"#;
    let output: Value =
        serde_json::from_str(&dispatch_json("timer.reduce.v1", raw).unwrap()).unwrap();
    assert!(
        output["canonicalTimer"]["lastIntent"]
            .get("deviceId")
            .is_none()
    );
}

#[test]
fn checker_concurrent_finish_marker_keeps_noop_without_invalid_completion_history() {
    let (mut input, _) = originating_finish("androidCoordinator");
    input["stage"] = json!("automaticFinishCommit");
    for field in ["occurredAt", "observedAt", "physicalNow"] {
        input["clock"][field] = json!("2026-07-20T12:01:00Z");
    }
    input["identities"]["commandUuids"] = json!(["019f7f66-a060-7000-8000-000000000001"]);
    input["workspace"]["base"]["canonicalTimer"]["lastIntent"] = json!({
        "type": "finish", "commandId": "concurrent-finish", "occurredAt": "2026-07-20T12:00:20Z"});
    let result = call("workspace.completionMutation.v1", &input).unwrap();
    assert_eq!(result["reason"], "staleTimer");
    assert_eq!(result["workspace"], input["workspace"]);
    assert_eq!(result["allocation"], input["allocation"]);
    assert_eq!(result["commands"], json!([]));
    assert_eq!(result["projection"]["history"], json!([]));
    assert_eq!(result["projection"]["canonicalTimer"]["status"], "running");
}
