use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn fixture(path: &str) -> Value {
    let source = match path {
        "horizon" => include_str!("../fixtures/bootstrap-android-horizon-v1.json"),
        _ => include_str!("../fixtures/bootstrap-workspace-v1.json"),
    };
    serde_json::from_str(source).unwrap()
}

fn request(case: &Value) -> Value {
    let shared = fixture("workspace");
    let mut input = shared["request"].clone();
    input["profile"] = json!("androidRepository");
    let workspace = &mut input["local"]["workspace"];
    workspace["now"] = case["now"].clone();
    if case["noTimer"] != true {
        workspace["base"]["canonicalTimer"] = fixture("horizon")["timer"].clone();
    }
    if case["terminal"] == true {
        workspace["base"]["canonicalTimer"] = shared["timer"].clone();
        workspace["base"]["history"] = json!([shared["history"]]);
    }
    workspace["local"]["commands"] = commands(case, &shared);
    if case["durationQueue"] == true {
        let mut duration = shared["operationClock"].clone();
        duration.as_object_mut().unwrap().extend(
            json!({"phase":"focus", "durationMs":1500000})
                .as_object()
                .unwrap()
                .clone(),
        );
        duration["occurredAt"] = json!("2026-09-21T12:02:00Z");
        workspace["local"]["durationOperations"] = json!([duration]);
    }
    if case["remoteTask"] == true {
        input["remote"]["tasks"] = json!([shared["crossProduct"]["task"]]);
    }
    input
}

fn commands(case: &Value, shared: &Value) -> Value {
    let mut commands = Vec::new();
    for (index, clock) in case["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let mut command = shared["command"].clone();
        command["id"] = clock["id"].clone();
        command["type"] = json!("retarget");
        command["taskId"] = Value::Null;
        command["timerId"] = json!("missing-timer");
        command["occurredAt"] = clock["at"].clone();
        command["hlcWallMs"] = clock["wall"].clone();
        command["deviceSequence"] = json!(index + 1);
        if let Some(kind) = clock.get("type") {
            command["type"] = kind.clone();
        }
        if let Some(timer) = clock.get("timerId") {
            command["timerId"] = timer.clone();
        }
        if case["separateDevices"] == true {
            command["deviceId"] = json!(format!("device-{index}"));
        }
        commands.push(command);
    }
    if case["start"] == true {
        commands.push(shared["command"].clone());
    }
    json!(commands)
}

fn check(case: &Value) {
    let input = request(case);
    let output: Value = serde_json::from_str(
        &dispatch_json("bootstrap.workspacePlan.v1", &input.to_string()).unwrap(),
    )
    .unwrap();
    let present = case["noTimer"] != true || case["start"] == true;
    let count = case["count"].as_u64().unwrap();
    assert_eq!(
        output["classification"]["local"],
        json!({"hasState":present,
        "completedHistoryCount":count, "displayHistoryCount":count}),
        "{}",
        case["name"]
    );
    let plan = if count > 0 {
        json!({"mode":"auto", "strategy":"replace_remote", "reason":"local_only"})
    } else if present {
        json!({"mode":"auto", "strategy":"merge", "reason":"local_state_only"})
    } else {
        json!({"mode":"auto", "strategy":"keep_remote", "reason":"empty"})
    };
    assert_eq!(output["plan"], plan, "{}", case["name"]);
}

#[test]
fn android_complete_queue_horizon_includes_commands_excluded_from_delivery_safe_projection() {
    let mut case = fixture("horizon")["cases"][8].clone();
    case["now"] = json!("2026-09-21T12:00:30Z");
    let mut input = request(&case);
    input["local"]["workspace"]["canonicalHead"] = json!({"wallMs":1790000000002_i64,"counter":0});
    let output: Value = serde_json::from_str(
        &dispatch_json("bootstrap.workspacePlan.v1", &input.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        output["classification"]["local"]["completedHistoryCount"],
        1
    );
    assert_eq!(
        output["plan"],
        json!({"mode":"auto","strategy":"replace_remote","reason":"local_only"})
    );
    input["local"]["workspace"]["local"]["commands"][0]["occurredAt"] = json!("invalid");
    assert!(dispatch_json("bootstrap.workspacePlan.v1", &input.to_string()).is_err());
}

#[test]
fn android_anchor_horizon_does_not_expire_at_observation_deadline() {
    for case in fixture("horizon")["cases"]
        .as_array()
        .unwrap()
        .iter()
        .take(6)
    {
        check(case);
    }
}

#[test]
fn android_complete_raw_command_queue_and_null_terminal_inputs_derive_horizon() {
    for case in fixture("horizon")["cases"]
        .as_array()
        .unwrap()
        .iter()
        .skip(6)
    {
        check(case);
    }
}
