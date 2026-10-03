use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const CONFLICT: &str = "invalid shared-core input: conflicting workspace terminal timer/history";

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/reconciliation-terminal-v3.json")).unwrap()
}

fn request(case: &Value) -> Value {
    let fixture = fixture();
    let mut local = fixture["local"].clone();
    if let Some(fields) = case["commandOverrides"].as_object() {
        local["commands"][0]
            .as_object_mut()
            .unwrap()
            .extend(fields.clone());
    }
    let mut response: Value =
        serde_json::from_str(fixture["http"]["responseRaw"].as_str().unwrap()).unwrap();
    response["history"] = json!([]);
    let mut sent = local.clone();
    if case["acknowledged"] != true {
        sent["commands"] = json!([]);
        response["acknowledgements"] = json!([]);
    }
    if case["extensions"] == true {
        local["commands"][0]["extension"] = json!({"empty":"","null":null,"nested":[false,{}]});
        response["canonicalTimer"]["extension"] = json!({"kept":true});
        response["canonicalTimer"]["lastIntent"]["deviceId"] = json!("origin-device");
        response["canonicalTimer"]["lastIntent"]["extension"] = json!([null, "", {}]);
    }
    if case["commandOverrides"].get("taskId").is_some() {
        response["canonicalTimer"]["taskId"] = json!("12345678-1234-4234-8234-123456789010");
    }
    json!({"local":local,"sent":sent,"response":response,"neverSent":{},"timerDependencies":[]})
}

fn workspace(input: &Value) -> Value {
    let response = &input["response"];
    json!({"base":{"canonicalTimer":response["canonicalTimer"],"history":response["history"],
        "tasks":response["tasks"],"durationsMs":response["durationsMs"],"autoStartBreaks":response["autoStartBreaks"],
        "selectedTaskId":response["selectedTaskId"]},"local":input["local"],"neverSent":input["neverSent"],
        "canonicalHead":{"wallMs":response["serverHlcWallMs"],"counter":response["serverHlcCounter"]},
        "timerDependencies":input["timerDependencies"],"now":response["serverTime"]})
}

fn rejection(name: &str) {
    let fixture = fixture();
    let case = fixture["retainedIntentRejections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap();
    let input = request(case);
    let before = input.clone();
    for (operation, request) in [
        ("reconcile.rebase.v3", input.clone()),
        ("workspace.project.v1", workspace(&input)),
    ] {
        assert_eq!(
            dispatch_json(operation, &request.to_string())
                .unwrap_err()
                .to_string(),
            CONFLICT,
            "{name}/{operation}"
        );
    }
    assert_eq!(input, before);
}

#[test]
fn missing_history_ack_time() {
    rejection("missing-history-ack-time");
}

#[test]
fn missing_history_ack_type() {
    rejection("missing-history-ack-type");
}

#[test]
fn missing_history_ack_timer() {
    rejection("missing-history-ack-timer");
}

#[test]
fn missing_history_frozen_time() {
    rejection("missing-history-frozen-time");
}

#[test]
fn missing_history_frozen_type() {
    rejection("missing-history-frozen-type");
}

#[test]
fn missing_history_frozen_timer() {
    rejection("missing-history-frozen-timer");
}

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

#[test]
fn valid_missing_history_synthesizes_exact_terminal_and_preserves_frozen_extensions() {
    for case in fixture()["retainedIntentControls"].as_array().unwrap() {
        let input = request(case);
        let before = input.clone();
        let output = call("reconcile.rebase.v3", &input);
        assert_eq!(output["canonicalResponse"], input["response"]);
        assert_eq!(output["baseTimer"], input["response"]["canonicalTimer"]);
        assert_eq!(output["baseHistory"], json!([]));
        assert_eq!(output["projectionPending"]["commands"], json!([]));
        let expected = if case["acknowledged"] == true {
            json!([])
        } else {
            input["local"]["commands"].clone()
        };
        assert_eq!(output["pending"], expected, "{}", case["name"]);
        let projected = call("workspace.project.v1", &workspace(&input));
        assert_eq!(
            output["workspace"], projected["workspace"],
            "{}",
            case["name"]
        );
        assert_synthesized(&output["workspace"], &input["response"]["canonicalTimer"]);
        assert_eq!(input, before);
    }
}

fn assert_synthesized(workspace: &Value, canonical: &Value) {
    assert_eq!(workspace["canonicalTimer"]["id"], canonical["id"]);
    assert_eq!(workspace["history"].as_array().unwrap().len(), 1);
    let row = &workspace["history"][0];
    assert_eq!(row["commandId"], canonical["lastIntent"]["commandId"]);
    assert_eq!(row["completedAt"], canonical["anchorAt"]);
    assert_eq!(row["endedAt"], canonical["anchorAt"]);
    for field in ["taskId", "phase", "plannedDurationMs", "status"] {
        assert_eq!(row[field], canonical[field]);
        assert_eq!(workspace["canonicalTimer"][field], canonical[field]);
    }
}
