use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/clock-observe-v1.json")).unwrap()
}

fn merge(target: &mut Value, patch: &Value) {
    if let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            merge(target.entry(key).or_insert(Value::Null), value);
        }
    } else {
        *target = patch.clone()
    }
}

fn request(f: &Value, case: &Value) -> Value {
    let profile = case["profile"].as_str().unwrap();
    let mut input = f["templates"][profile].clone();
    if let Some(saved) = case["saved"].as_str() {
        let field = match profile {
            "appleTrustedClock" => None,
            "desktopTrustedClock" => Some("sample"),
            _ => Some("clockOffset"),
        };
        if let Some(field) = field {
            input["state"][field] = f[saved].clone()
        } else {
            input["state"] = f[saved].clone()
        }
    }
    if case["requestSample"] == true {
        input["state"]["requestSample"] = f["androidSample"].clone()
    }
    if case["persistedAndroid"] == true {
        merge(
            &mut input["state"],
            &json!({"serverClockOffsetMs":100,"serverClockUncertaintyMs":1,
            "serverClockSamplePhysicalMs":1000000,"serverClockSampleElapsedRealtimeMs":20000,
            "serverClockBootId":"boot-a","retainedWallMs":1000100}),
        );
    }
    if case["runtimeDesktop"] == true {
        input["state"]["anchor"] = f["desktopSample"].clone()
    }
    if case["runtimePwa"] == true {
        input["state"]["runtime"] =
            json!({"identity":"100:1:1000000","monotonicMs":20.25,"wallMs":1000100});
    }
    if case["server"] == true {
        input["server"] = f["servers"][profile].clone()
    }
    if let Some(patch) = case.get("patch") {
        merge(&mut input, patch)
    }
    input
}

fn assert_fields(actual: &Value, expected: &Value, name: &str) {
    if let Some(fields) = expected.as_object() {
        for (key, value) in fields {
            assert_fields(&actual[key], value, name)
        }
    } else if actual.is_number() && expected.is_number() {
        assert_eq!(actual.as_f64(), expected.as_f64(), "{name}");
    } else {
        assert_eq!(actual, expected, "{name}")
    }
}

#[test]
fn shared_adversarial_raw_observations() {
    let f = fixture();
    for case in f["cases"].as_array().unwrap() {
        let input = request(&f, case);
        let result = dispatch_json("clock.observe.v1", &input.to_string());
        let name = case["name"].as_str().unwrap();
        if case["reject"] == true {
            assert!(result.is_err(), "{name}: {result:?}");
            continue;
        }
        let serialized = result.unwrap_or_else(|error| panic!("{name}: {error}"));
        let actual: Value = serde_json::from_str(&serialized).unwrap();
        assert_fields(&actual, &case["expected"], name);
        assert_eq!(actual["schemaVersion"], 1);
        assert_eq!(actual["compatibility"], input["compatibility"]);
        assert_eq!(
            dispatch_json("clock.observe.v1", &input.to_string()).unwrap(),
            serialized
        );
    }
}

#[test]
fn boundary_rejects_versions_duplicates_wrong_profiles_and_computed_time() {
    let f = fixture();
    for (field, value) in [
        ("schemaVersion", json!(2)),
        ("compatibility", json!("generic")),
        ("nowMs", json!(100)),
        ("elapsedMs", json!(100)),
        ("server", json!({})),
        ("action", json!("tick")),
        ("state", json!([])),
    ] {
        let mut input = f["templates"]["appleTrustedClock"].clone();
        input[field] = value;
        assert!(
            dispatch_json("clock.observe.v1", &input.to_string()).is_err(),
            "{field}"
        );
    }
    let input =
        f["templates"]["appleTrustedClock"]
            .to_string()
            .replacen("{", "{\"schemaVersion\":1,", 1);
    assert!(
        dispatch_json("clock.observe.v1", &input)
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn continued_states_roundtrip_without_touching_hlc_or_wire_records() {
    let f = fixture();
    for profile in [
        "appleTrustedClock",
        "androidTrustedClock",
        "desktopTrustedClock",
        "pwaTrustedClock",
    ] {
        let mut input = f["templates"][profile].clone();
        input["action"] = json!("sample");
        input["server"] = f["servers"][profile].clone();
        let result: Value =
            serde_json::from_str(&dispatch_json("clock.observe.v1", &input.to_string()).unwrap())
                .unwrap();
        input["state"] = result["state"].clone();
        if profile == "androidTrustedClock" {
            input["state"]["requestSample"] = result["sample"].clone()
        }
        input.as_object_mut().unwrap().remove("server");
        input["action"] = json!("current");
        if profile == "appleTrustedClock" {
            input["reading"]["uptimeSeconds"] = json!(21)
        } else {
            input["reading"]["monotonicMs"] = json!(20005)
        }
        let next: Value =
            serde_json::from_str(&dispatch_json("clock.observe.v1", &input.to_string()).unwrap())
                .unwrap();
        assert!(next["trustedNowMs"].as_i64().unwrap() > 1000000);
        assert!(next.get("hlc").is_none());
        assert!(next.get("canonicalTimer").is_none());
    }
}

#[test]
fn decimal_readings_and_envelope_preserve_exact_floating_point_state() {
    let decimal = "1300.1000000238419";
    let input = format!(
        r#"{{"schemaVersion":1,"compatibility":"appleTrustedClock","action":"current",
        "state":{{"offsetMs":100,"uncertaintyMs":1,"anchorMs":1000100,"anchorUptime":{decimal}}},
        "reading":{{"uptimeSeconds":{decimal}}}}}"#
    );
    let result = dispatch_json("clock.observe.v1", &input).unwrap();
    assert!(result.contains(&format!("\"anchorUptime\":{decimal}")));
    assert_eq!(
        pomodorough_core::dispatch_envelope_json("clock.observe.v1", &input),
        format!("{{\"ok\":true,\"value\":{result}}}")
    );
    // The same raw state can be returned through JSON without an intermediate
    // Value parser changing a binary float by one ULP.
    assert_eq!(dispatch_json("clock.observe.v1", &input).unwrap(), result);
}
