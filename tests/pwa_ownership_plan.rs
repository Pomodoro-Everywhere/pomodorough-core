use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/pwa-ownership-plan-v1.json")).unwrap()
}

fn call(input: &Value) -> Result<Value, String> {
    pomodorough_core::dispatch_json("workspace.ownershipPlan.v1", &input.to_string())
        .map(|raw| serde_json::from_str(&raw).unwrap())
        .map_err(|error| error.to_string())
}

#[test]
fn missing_owner_uses_any_retained_start_without_delivery_permission() {
    let fixture = fixture();
    let input = &fixture["request"];
    let result = call(input).unwrap();
    assert_eq!(result["ownership"], fixture["owner"]);
    assert_eq!(result["renewed"], true);
    assert_eq!(result["workspace"], input["workspace"]);
    assert_eq!(result["effectsAfterCommit"], json!([]));
    let mut write = fixture["owner"].clone();
    write["kind"] = json!("recordTimerOwner");
    assert_eq!(result["ownershipWrites"], json!([write.clone(), write]));
}

#[test]
fn peer_tab_lease_boundary_uses_wall_clock_and_exact_equality() {
    let fixture = fixture();
    for case in fixture["leaseBoundary"].as_array().unwrap() {
        let mut input = fixture["request"].clone();
        input["ownership"] = fixture["owner"].clone();
        input["localTabId"] = json!("tab-renamed");
        input["clock"]["nowMs"] = case["nowMs"].clone();
        let result = call(&input).unwrap();
        assert_eq!(result["renewed"], case["renewed"]);
        assert_eq!(result["reason"], case["reason"]);
        if case["renewed"] == false {
            assert_eq!(result["ownership"], input["ownership"]);
            assert_eq!(result["ownershipWrites"], json!([]));
            assert_eq!(result["retryAtMs"], input["ownership"]["leaseExpiresAtMs"]);
        } else {
            assert_eq!(result["ownership"]["tabId"], input["localTabId"]);
        }
    }
}

fn canonical(input: &mut Value, device: Option<&str>) {
    let mut replay = input["workspace"].clone();
    replay["now"] = json!("1970-01-01T00:00:00Z");
    let raw = pomodorough_core::dispatch_json("workspace.project.v1", &replay.to_string()).unwrap();
    let projected: Value = serde_json::from_str(&raw).unwrap();
    let mut timer = projected["workspace"]["canonicalTimer"].clone();
    timer.as_object_mut().unwrap().remove("startedByDeviceId");
    if let Some(device) = device {
        timer["startedByDeviceId"] = json!(device);
    }
    input["workspace"]["base"]["canonicalTimer"] = timer;
}

#[test]
fn canonical_start_origin_overrides_retained_start_and_install_keeps_valid_owner() {
    for (device, claims) in [
        (Some("device-local"), true),
        (Some("device-foreign"), false),
        (None, true),
    ] {
        let mut input = fixture()["request"].clone();
        canonical(&mut input, device);
        input["action"] = json!({"kind": "install"});
        let result = call(&input).unwrap();
        assert_eq!(
            result["ownershipWrites"].as_array().unwrap().len(),
            usize::from(claims)
        );
        assert_eq!(result["renewed"], false);
    }
    let mut input = fixture()["request"].clone();
    canonical(&mut input, Some("device-foreign"));
    input["ownership"] = fixture()["owner"].clone();
    input["action"] = json!({"kind": "install"});
    let result = call(&input).unwrap();
    assert_eq!(result["ownership"], input["ownership"]);
    assert_eq!(result["ownershipWrites"], json!([]));
}

#[test]
fn foreign_device_never_takes_over_expired_existing_owner() {
    let mut input = fixture()["request"].clone();
    input["ownership"] = fixture()["owner"].clone();
    input["ownership"]["deviceId"] = json!("device-foreign");
    input["ownership"]["leaseExpiresAtMs"] = json!(0);
    let result = call(&input).unwrap();
    assert_eq!(result["reason"], "notOwner");
    assert_eq!(result["ownership"], input["ownership"]);
    assert_eq!(result["ownershipWrites"], json!([]));
    assert!(result.get("retryAtMs").is_none());
}

#[test]
fn legacy_missing_or_null_lease_is_expired_but_install_preserves_raw_omission() {
    for expiry in [None, Some(Value::Null), Some(json!(0))] {
        let mut input = fixture()["request"].clone();
        input["ownership"] = fixture()["owner"].clone();
        input["ownership"]
            .as_object_mut()
            .unwrap()
            .remove("leaseExpiresAtMs");
        if let Some(expiry) = expiry {
            input["ownership"]["leaseExpiresAtMs"] = expiry;
        }
        input["localTabId"] = json!("tab-reopened");
        assert_eq!(call(&input).unwrap()["renewed"], true);
        input["action"] = json!({"kind": "install"});
        assert_eq!(call(&input).unwrap()["ownership"], input["ownership"]);
    }
}

#[test]
fn terminal_and_removed_timers_prune_owner_without_reclaiming_replacement() {
    for kind in ["finish", "cancel", "clear", "replacement"] {
        let mut input = fixture()["request"].clone();
        input["ownership"] = fixture()["owner"].clone();
        let mut command = input["workspace"]["local"]["commands"][0].clone();
        command["id"] = json!(format!("terminal-{kind}"));
        command["deviceSequence"] = json!(2);
        command["hlcCounter"] = json!(1);
        command["type"] = json!(if kind == "replacement" { "start" } else { kind });
        if kind == "replacement" {
            command["timerId"] = json!("replacement-timer");
        }
        input["workspace"]["local"]["commands"]
            .as_array_mut()
            .unwrap()
            .push(command);
        let result = call(&input).unwrap();
        assert_eq!(result["ownership"], Value::Null, "{kind}");
        assert_eq!(
            result["ownershipWrites"],
            json!([{"kind": "removeTimerOwner"}]),
            "{kind}"
        );
        assert_eq!(result["renewed"], false);
    }
}

#[test]
fn natural_deadline_keeps_live_owner_but_does_not_claim_missing_owner() {
    let mut input = fixture()["request"].clone();
    input["clock"]["nowMs"] = json!(1784550400000_i64);
    assert_eq!(call(&input).unwrap()["reason"], "notClaimable");
    input["ownership"] = fixture()["owner"].clone();
    assert_eq!(call(&input).unwrap()["renewed"], true);
    input["ownership"]["leaseExpiresAtMs"] = json!(1784550460000_i64);
    input["localTabId"] = json!("peer");
    let denied = call(&input).unwrap();
    assert_eq!(denied["ownership"], input["ownership"]);
    assert_eq!(denied["ownershipWrites"], json!([]));
    assert_eq!(denied["retryAtMs"], input["ownership"]["leaseExpiresAtMs"]);
}

#[test]
fn stale_renewal_installs_actual_missing_owner_but_never_renews_presented_timer() {
    let mut input = fixture()["request"].clone();
    input["action"]["timerId"] = json!("stale-timer");
    let result = call(&input).unwrap();
    assert_eq!(result["reason"], "staleTimer");
    assert_eq!(result["renewed"], false);
    assert_eq!(result["ownership"], fixture()["owner"]);
    assert_eq!(result["ownershipWrites"][0]["timerId"], "ownership-timer");
}

#[test]
fn covered_retained_start_is_evidence_even_without_display_membership() {
    let mut input = fixture()["request"].clone();
    canonical(&mut input, None);
    input["workspace"]["canonicalHead"] = json!({"wallMs": 1784548800000_i64, "counter": 0});
    input["workspace"]["displayContext"]["projectionPending"] = json!({
        "commands": [], "taskOperations": [], "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []});
    let result = call(&input).unwrap();
    assert_eq!(result["renewed"], true);
    assert_eq!(result["workspace"], input["workspace"]);
}

#[test]
fn malformed_raw_owner_clock_and_caller_policy_controls_fail_closed() {
    let request = fixture()["request"].clone();
    for owner in [
        json!(false),
        json!([]),
        json!({}),
        json!({"timerId": "x", "deviceId": "y", "owns": true}),
        json!({"timerId": "x", "deviceId": "y", "leaseExpiresAtMs": "0"}),
        json!({"timerId": "x", "deviceId": "y", "leaseExpiresAtMs": -1}),
        json!({"timerId": "x", "deviceId": "y", "leaseExpiresAtMs": 9007199254740992_i64}),
    ] {
        let mut input = request.clone();
        input["ownership"] = owner;
        assert!(call(&input).is_err(), "{input}");
    }
    for field in [
        "owns",
        "claimable",
        "manual",
        "expectedUserId",
        "droppedTimerIds",
    ] {
        let mut input = request.clone();
        input[field] = json!(true);
        assert!(call(&input).is_err(), "{field}");
    }
    for (field, value) in [
        ("nowMs", -1),
        ("nowMs", 9007199254740992_i64),
        ("leaseDurationMs", 0),
    ] {
        let mut input = request.clone();
        input["clock"][field] = json!(value);
        assert!(call(&input).is_err());
    }
    let mut overflow = request;
    overflow["clock"]["leaseDurationMs"] = json!(9007199254740991_i64);
    assert!(
        call(&overflow)
            .unwrap_err()
            .contains("lease expiry overflow")
    );
}

#[test]
fn wrong_profiles_missing_observations_and_rewritten_display_fail_closed() {
    for field in ["ownership", "clock", "localTabId", "workspace"] {
        let mut input = fixture()["request"].clone();
        input.as_object_mut().unwrap().remove(field);
        assert!(call(&input).is_err(), "{field}");
    }
    for profile in [
        "appleWorkspace",
        "androidCoordinator",
        "desktopStorage",
        "desktopTerminal",
    ] {
        let mut input = fixture()["request"].clone();
        input["profile"] = json!(profile);
        assert!(call(&input).is_err());
    }
    let mut input = fixture()["request"].clone();
    input["workspace"]["displayContext"]["projectionPending"] = input["workspace"]["local"].clone();
    input["workspace"]["displayContext"]["projectionPending"]["commands"][0]["extension"] =
        Value::Null;
    assert!(call(&input).is_err());
    let duplicate = fixture()["request"].to_string().replace(
        "\"ownership\":null",
        "\"ownership\":null,\"ownership\":null",
    );
    assert!(pomodorough_core::dispatch_json("workspace.ownershipPlan.v1", &duplicate).is_err());
}
