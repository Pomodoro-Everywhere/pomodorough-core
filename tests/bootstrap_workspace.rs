use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/bootstrap-workspace-v1.json")).unwrap()
}

fn call(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("bootstrap.workspacePlan.v1", &input.to_string()).unwrap())
        .unwrap()
}

fn request(profile: &str, case: &Value) -> Value {
    let mut input = fixture()["request"].clone();
    input["profile"] = json!(profile);
    if !case["path"].as_array().unwrap().is_empty() {
        let mut field = &mut input;
        for key in case["path"].as_array().unwrap() {
            field = &mut field[key.as_str().unwrap()];
        }
        *field = case["value"].clone();
    }
    input
}

fn expected(profile: &str, case: &Value, side: &str) -> Value {
    let prefix = match profile {
        "appleWorkspace" => "apple",
        "androidRepository" => "android",
        "desktopStorage" => "desktop",
        "pwaStorage" => "pwa",
        _ => unreachable!(),
    };
    let capital = if side == "local" { "Local" } else { "Remote" };
    let has_state = case
        .get(format!("{prefix}{capital}"))
        .unwrap_or(&case[side]);
    let count = case
        .get(format!("{side}Count"))
        .cloned()
        .unwrap_or(json!(0));
    let display = if side == "local" {
        case.get(format!("{prefix}Display")).unwrap_or(&count)
    } else {
        case.get(format!("{prefix}RemoteDisplay")).unwrap_or(&count)
    };
    json!({"hasState": has_state, "completedHistoryCount": count, "displayHistoryCount": display})
}

#[test]
fn raw_shared_profile_vectors_return_complete_classification_and_exact_v1_plan() {
    for profile in [
        "appleWorkspace",
        "androidRepository",
        "desktopStorage",
        "pwaStorage",
    ] {
        for case in fixture()["cases"].as_array().unwrap() {
            let input = request(profile, case);
            let output = call(&input);
            let local = expected(profile, case, "local");
            let remote = expected(profile, case, "remote");
            let history = &input["local"]["workspace"]["base"]["history"];
            let plan = dispatch_json("bootstrap.plan.v1", &json!({
                "localOwnerId": input["local"]["ownerId"], "currentUserId": input["currentUserId"],
                "localHistory": history, "remoteHistory": input["remote"]["history"],
                "hasLocalState": local["hasState"], "hasRemoteState": remote["hasState"]
            }).to_string()).unwrap();
            assert_eq!(
                output,
                json!({"plan": serde_json::from_str::<Value>(&plan).unwrap(),
                "classification": {"profile": profile, "local": local, "remote": remote}}),
                "{profile}: {}",
                case["name"]
            );
            if let Some(plan) = case.get("plan") {
                assert_eq!(&output["plan"], plan);
            }
            assert_eq!(
                dispatch_json("bootstrap.workspacePlan.v1", &input.to_string()).unwrap(),
                dispatch_json("bootstrap.workspacePlan.v1", &input.to_string()).unwrap()
            );
        }
    }
}

#[test]
fn raw_history_and_state_cross_product_keeps_all_shipped_plan_branches() {
    let fixture = fixture();
    for mask in 0..16 {
        let mut input = fixture["request"].clone();
        let (lh, rh, ls, rs) = (mask & 1 != 0, mask & 2 != 0, mask & 4 != 0, mask & 8 != 0);
        input["local"]["workspace"]["base"]["history"] = if lh {
            json!([fixture["crossProduct"]["history"]])
        } else {
            json!([])
        };
        input["remote"]["history"] = if rh {
            json!([fixture["crossProduct"]["history"]])
        } else {
            json!([])
        };
        input["local"]["workspace"]["base"]["tasks"] = if ls {
            json!([fixture["crossProduct"]["task"]])
        } else {
            json!([])
        };
        input["remote"]["tasks"] = if rs {
            json!([fixture["crossProduct"]["task"]])
        } else {
            json!([])
        };
        let old = dispatch_json("bootstrap.plan.v1", &json!({"localHistory": input["local"]["workspace"]["base"]["history"],
            "remoteHistory": input["remote"]["history"], "hasLocalState": ls || lh, "hasRemoteState": rs || rh}).to_string()).unwrap();
        let old: Value = serde_json::from_str(&old).unwrap();
        for profile in [
            "appleWorkspace",
            "androidRepository",
            "desktopStorage",
            "pwaStorage",
        ] {
            input["profile"] = json!(profile);
            let output = call(&input);
            assert_eq!(output["plan"], old, "mask={mask}/{profile}");
            assert_eq!(
                output["classification"]["local"],
                json!({"hasState": ls || lh,
                "completedHistoryCount": usize::from(lh), "displayHistoryCount": usize::from(lh)})
            );
            assert_eq!(
                output["classification"]["remote"],
                json!({"hasState": rs || rh,
                "completedHistoryCount": usize::from(rh), "displayHistoryCount": usize::from(rh)})
            );
        }
    }
}
