use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/pwa-ownership-release-v1.json")).unwrap()
}

fn request(case: &Value) -> Value {
    let seed: Value =
        serde_json::from_str(include_str!("../fixtures/pwa-ownership-plan-v1.json")).unwrap();
    let mut raw = seed["request"].clone();
    raw["action"] = json!({"kind": "release"});
    raw["clock"]
        .as_object_mut()
        .unwrap()
        .remove("leaseDurationMs");
    raw["ownership"] = case.get("owner").unwrap_or(&seed["owner"]).clone();
    if let Some(fields) = case["set"].as_object() {
        for (path, value) in fields {
            *field(&mut raw, path) = value.clone();
        }
    }
    if let Some(paths) = case["omit"].as_array() {
        for path in paths {
            let (parent, key) = path
                .as_str()
                .unwrap()
                .rsplit_once('.')
                .unwrap_or(("", path.as_str().unwrap()));
            field(&mut raw, parent).as_object_mut().unwrap().remove(key);
        }
    }
    raw
}

fn field<'a>(value: &'a mut Value, path: &str) -> &'a mut Value {
    if path.is_empty() {
        return value;
    }
    path.split('.')
        .fold(value, |value, key| match key.parse::<usize>() {
            Ok(index) => &mut value[index],
            Err(_) => &mut value[key],
        })
}

#[test]
fn shared_release_successes_compare_complete_returns() {
    let fixture = fixture();
    assert_eq!(fixture["successes"].as_array().unwrap().len(), 21);
    for case in fixture["successes"].as_array().unwrap() {
        let input = request(case);
        let mut owner = input["ownership"].clone();
        let writes = if case["writes"] == 1 {
            owner["leaseExpiresAtMs"] = input["clock"]["nowMs"].clone();
            let mut write = owner.clone();
            write["kind"] = json!("recordTimerOwner");
            json!([write])
        } else {
            json!([])
        };
        let expected = json!({"schemaVersion": 1, "workspace": input["workspace"],
            "ownership": owner, "renewed": false, "reason": case["reason"],
            "ownershipWrites": writes, "effectsAfterCommit": []});
        let envelope: Value = serde_json::from_str(&pomodorough_core::dispatch_envelope_json(
            "workspace.ownershipPlan.v1",
            &input.to_string(),
        ))
        .unwrap();
        assert_eq!(
            envelope,
            json!({"ok": true, "value": expected}),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn release_keeps_raw_numeric_tokens_through_the_complete_public_envelope() {
    let mut input = request(&json!({}));
    input["workspace"]["local"]["commands"][0]["extension"] = json!({"number": 90.5});
    let raw = input.to_string().replace("90.5", "90.49999999999999");
    let envelope = pomodorough_core::dispatch_envelope_json("workspace.ownershipPlan.v1", &raw);
    assert!(
        envelope.contains("\"number\":90.49999999999999"),
        "{envelope}"
    );
    assert!(!envelope.contains("\"number\":90.5"));
}

#[test]
fn shared_release_rejections_have_error_only_envelopes() {
    let fixture = fixture();
    assert_eq!(fixture["rejections"].as_array().unwrap().len(), 31);
    for case in fixture["rejections"].as_array().unwrap() {
        let input = request(case);
        let mut raw = input.to_string();
        if let Some(key) = case["duplicate"].as_str() {
            raw = format!("{{\"{key}\":{},{}", input[key], &raw[1..]);
        }
        let envelope: Value = serde_json::from_str(&pomodorough_core::dispatch_envelope_json(
            "workspace.ownershipPlan.v1",
            &raw,
        ))
        .unwrap();
        assert_eq!(envelope["ok"], false, "{}: {envelope}", case["name"]);
        assert!(envelope["error"].is_string());
        assert_eq!(envelope.as_object().unwrap().len(), 2);
    }
}
