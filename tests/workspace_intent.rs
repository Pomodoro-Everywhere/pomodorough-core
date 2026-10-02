use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap()
}

fn request(profile: &str, action: &str, status: &str) -> Value {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    request["compatibility"] = json!(profile);
    request["intent"] = json!({"kind": action});
    if status != "idle" {
        let mut timer = fixture["timer"].clone();
        timer["status"] = json!(status);
        if status == "completed" {
            timer["elapsedAtAnchorMs"] = json!(60000);
        }
        request["workspace"]["base"]["canonicalTimer"] = timer;
        if matches!(profile, "desktopStorage" | "desktopTerminal" | "pwaStorage")
            && matches!(action, "restart" | "cancelAndClear")
            || profile == "desktopTerminal" && action == "cancel"
        {
            request["requestedTimer"] = request["workspace"]["base"]["canonicalTimer"].clone();
        }
    }
    request
}

fn plan(request: &Value) -> Value {
    let output = dispatch_json("workspace.intent.v1", &request.to_string())
        .unwrap_or_else(|error| panic!("{error}: {request}"));
    serde_json::from_str(&output).unwrap()
}

fn checker_case(name: &str) -> Value {
    fixture()["checkerCases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap()
        .clone()
}

fn checker_request(case: &Value) -> Value {
    let mut input = request(
        case["profile"].as_str().unwrap(),
        case["intent"].as_str().unwrap(),
        case["status"].as_str().unwrap(),
    );
    if let Some(mode) = case["replicationMode"].as_str() {
        input["replicationMode"] = json!(mode);
    }
    if case["retainedHistory"] == true {
        input["workspace"]["base"]["canonicalTimer"] = Value::Null;
        input["workspace"]["base"]["history"] = json!([fixture()["retainedCompletion"]]);
    }
    if let Some(anchor) = case["presentedAnchorAt"].as_str() {
        input["requestedTimer"]["anchorAt"] = json!(anchor);
    }
    if let Some(id) = case["presentedTimerId"].as_str() {
        input["requestedTimer"]["id"] = json!(id);
    }
    input
}

#[test]
fn shared_action_status_matrix() {
    let fixture = fixture();
    for (profile, rows) in fixture["profiles"].as_object().unwrap() {
        let mut actions = fixture["common"].as_object().unwrap().clone();
        actions.extend(rows.as_object().unwrap().clone());
        for (action, expected) in actions {
            for (index, status) in fixture["statuses"].as_array().unwrap().iter().enumerate() {
                let input = request(profile, &action, status.as_str().unwrap());
                let output = plan(&input);
                let commands = output["commands"].as_array().unwrap();
                let types: Vec<_> = commands.iter().map(|value| value["type"].clone()).collect();
                assert_eq!(json!(types), expected[index], "{profile}/{action}/{status}");
                assert_eq!(
                    output["atomicCommandIds"].as_array().unwrap().len(),
                    commands.len()
                );
                assert_eq!(output["allocation"]["deviceSequence"], 7 + commands.len());
                for (index, command) in commands.iter().enumerate() {
                    assert_eq!(command["deviceSequence"], 8 + index);
                    assert_eq!(command["hlcCounter"], index);
                    assert_eq!(command["hlcWallMs"], 1784548810000_i64);
                    assert_eq!(command["deviceId"], "device-local");
                    assert_eq!(
                        output["projection"]["timerOutcomes"][command["id"].as_str().unwrap()]["outcome"],
                        "applied"
                    );
                }
                if commands.is_empty() {
                    assert_eq!(output["outcome"], "noop");
                    assert_eq!(output["workspace"], input["workspace"]);
                    assert_eq!(output["allocation"], input["allocation"]);
                    assert_eq!(output["effectsAfterCommit"], json!([]));
                }
            }
        }
    }
}

#[test]
fn explicit_selection_preserves_entrypoint_differences() {
    for profile in [
        "appleWorkspace",
        "androidCoordinator",
        "desktopStorage",
        "desktopTerminal",
        "pwaStorage",
    ] {
        for status in [
            "idle",
            "running",
            "paused",
            "completed",
            "cancelled",
            "superseded",
        ] {
            for phase in ["focus", "short_break", "long_break"] {
                let mut input = request(profile, "selectPhase", status);
                input["intent"]["phase"] = json!(phase);
                let output = plan(&input);
                let blocked = matches!(profile, "androidCoordinator" | "desktopTerminal")
                    && matches!(status, "running" | "paused");
                let same = profile == "androidCoordinator" && phase == "focus";
                let generation = if blocked || same || profile == "pwaStorage" {
                    "5"
                } else {
                    "6"
                };
                assert_eq!(
                    output["selection"]["generation"], generation,
                    "{profile}/{status}/{phase}"
                );
                assert_eq!(
                    output["selection"]["phase"],
                    if blocked { "focus" } else { phase }
                );
                let clears = profile == "appleWorkspace"
                    && matches!(status, "completed" | "cancelled" | "superseded");
                assert_eq!(
                    output["commands"].as_array().unwrap().len(),
                    usize::from(clears)
                );
            }
        }
    }
}

fn history(count: usize) -> Value {
    json!(
        (0..count)
            .map(|index| json!({"id": format!("h-{index}"),
        "timerId": format!("t-{index}"), "phase": "focus", "status": "completed",
        "plannedDurationMs": 60000, "completedAt": "2026-07-20T10:00:00Z"}))
            .collect::<Vec<_>>()
    )
}

#[test]
fn skip_uses_shared_history_policy_for_zero_through_twelve() {
    for count in 0..=12 {
        for phase in ["focus", "short_break", "long_break"] {
            let mut input = request("appleWorkspace", "skip", "idle");
            input["workspace"]["base"]["history"] = history(count);
            input["selection"]["phase"] = json!(phase);
            let output = plan(&input);
            let expected = if phase != "focus" {
                "focus"
            } else if count % 4 == 3 {
                "long_break"
            } else {
                "short_break"
            };
            assert_eq!(
                output["selection"],
                json!({"phase": expected, "generation": "6", "explicit": true})
            );
        }
    }
}

#[test]
fn selection_generation_overflow_is_profile_specific() {
    for (profile, before, after) in [
        ("appleWorkspace", "9223372036854775807", "0"),
        (
            "androidCoordinator",
            "9223372036854775807",
            "-9223372036854775808",
        ),
        (
            "androidCoordinator",
            "-9223372036854775808",
            "-9223372036854775807",
        ),
        (
            "desktopStorage",
            "9223372036854775807",
            "9223372036854775808",
        ),
        (
            "desktopTerminal",
            "99999999999999999999999999",
            "100000000000000000000000000",
        ),
        ("pwaStorage", "9223372036854775807", "9223372036854775807"),
    ] {
        let mut input = request(profile, "selectPhase", "idle");
        input["intent"]["phase"] = json!("short_break");
        input["selection"]["generation"] = json!(before);
        assert_eq!(plan(&input)["selection"]["generation"], after);
    }
}

#[test]
fn physical_observation_never_rewrites_wire_timestamps() {
    let mut input = request("appleWorkspace", "pause", "running");
    input["workspace"]["base"]["canonicalTimer"]["anchorAt"] = json!("2026-07-20T12:05:00Z");
    input["observation"]["canonicalAnchorAt"] = json!("2026-07-20T12:00:00Z");
    input["clock"]["occurredAt"] = json!("2026-07-20T12:05:10Z");
    input["identities"]["commandUuids"] = json!(["019f7f6a-70f0-7000-8000-000000000001"]);
    input["clock"]["observedAt"] = json!("2026-07-20T12:00:08Z");
    let output = plan(&input);
    assert_eq!(output["commands"][0]["observedElapsedMs"], 13000);
    assert_eq!(output["commands"][0]["occurredAt"], "2026-07-20T12:05:10Z");
    assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
    assert_eq!(
        output["projection"]["canonicalTimer"]["elapsedAtAnchorMs"],
        13000
    );
}

#[test]
fn pwa_monotonic_anchor_survives_forward_wall_jump_and_pause() {
    let mut input = request("pwaStorage", "selectPhase", "running");
    input["intent"]["phase"] = json!("focus");
    input["clock"]["monotonicNowMs"] = json!(100.0);
    input["clock"]["continuityId"] = json!("browser-session-1");
    let sampled = plan(&input);
    assert_eq!(sampled["outcome"], "noop");
    assert_eq!(sampled["allocation"], input["allocation"]);
    assert_eq!(sampled["workspace"], input["workspace"]);
    assert_eq!(
        sampled["observation"]["monotonicAnchor"],
        json!({
            "timerId": "existing-timer", "anchorAt": "2026-07-20T12:00:00Z",
            "elapsedAtAnchorMs": 5000, "sampledTrustedNowMs": 1784548810000_i64,
            "sampledMonotonicMs": 100.0, "continuityId": "browser-session-1"
        })
    );
    assert_eq!(
        sampled["timerObservation"],
        json!({
            "timerId": "existing-timer", "elapsedMs": 15000, "remainingMs": 45000,
            "deadlineAt": "2026-07-20T12:00:55.000Z"
        })
    );

    input["observation"] = sampled["observation"].clone();
    input["intent"] = json!({"kind": "pause"});
    input["clock"]["physicalNow"] = json!("2026-07-20T12:01:20Z");
    input["clock"]["observedAt"] = input["clock"]["physicalNow"].clone();
    input["clock"]["occurredAt"] = input["clock"]["physicalNow"].clone();
    input["clock"]["monotonicNowMs"] = json!(1100.0);
    input["identities"]["commandUuids"] = json!(["019f7f66-ee80-7000-8000-000000000001"]);
    let paused = plan(&input);
    assert_eq!(paused["commands"][0]["observedElapsedMs"], 16000);
    assert_eq!(
        paused["commands"][0]["occurredAt"],
        "2026-07-20T12:01:20.000Z"
    );
    assert_eq!(
        paused["timerObservation"],
        json!({
            "timerId": "existing-timer", "elapsedMs": 16000, "remainingMs": 44000,
            "deadlineAt": null
        })
    );
    assert_eq!(paused["projection"]["canonicalTimer"]["status"], "paused");
    assert_eq!(paused["observation"]["monotonicAnchor"], Value::Null);
    assert_eq!(paused["workspace"]["base"], input["workspace"]["base"]);
}

#[test]
fn pwa_missing_anchor_uses_wall_time_then_seeds_raw_sample() {
    let mut input = request("pwaStorage", "pause", "running");
    input["clock"]["monotonicNowMs"] = json!(1100.0);
    input["clock"]["continuityId"] = json!("browser-session-2");
    let paused = plan(&input);
    assert_eq!(paused["commands"][0]["observedElapsedMs"], 15000);
    assert_eq!(paused["observation"]["monotonicAnchor"], Value::Null);
}

#[test]
fn pwa_resume_resamples_timer_identity_and_ignores_old_anchor() {
    let mut input = request("pwaStorage", "pause", "running");
    input["clock"]["monotonicNowMs"] = json!(100.0);
    input["clock"]["continuityId"] = json!("browser-session-1");
    let paused = plan(&input);
    input["workspace"] = paused["workspace"].clone();
    input["allocation"] = paused["allocation"].clone();
    input["observation"] = paused["observation"].clone();
    input["intent"] = json!({"kind": "resume"});
    input["clock"]["physicalNow"] = json!("2026-07-20T12:00:11Z");
    input["clock"]["observedAt"] = input["clock"]["physicalNow"].clone();
    input["clock"]["occurredAt"] = input["clock"]["physicalNow"].clone();
    input["clock"]["monotonicNowMs"] = json!(1100.0);
    input["identities"]["commandUuids"] = json!(["019f7f65-e0f8-7000-8000-000000000001"]);
    let resumed = plan(&input);
    assert_eq!(resumed["commands"][0]["observedElapsedMs"], 15000);
    assert_eq!(
        resumed["workspace"]["local"]["commands"][0],
        paused["commands"][0]
    );
    assert_eq!(
        resumed["observation"]["monotonicAnchor"]["anchorAt"],
        resumed["projection"]["canonicalTimer"]["anchorAt"]
    );

    input["workspace"] = resumed["workspace"].clone();
    input["allocation"] = resumed["allocation"].clone();
    input["observation"] = resumed["observation"].clone();
    input["intent"] = json!({"kind": "pause"});
    input["clock"]["monotonicNowMs"] = json!(2100.0);
    input["identities"]["commandUuids"] = json!(["019f7f65-e0f8-7000-8000-000000000002"]);
    assert_eq!(plan(&input)["commands"][0]["observedElapsedMs"], 16000);
}

#[test]
fn pwa_discontinuous_or_backwards_monotonic_reverts_to_wall_observation() {
    let mut input = request("pwaStorage", "selectPhase", "running");
    input["intent"]["phase"] = json!("focus");
    input["clock"]["monotonicNowMs"] = json!(100.0);
    input["clock"]["continuityId"] = json!("browser-session-1");
    let anchor = plan(&input)["observation"].clone();
    for (monotonic, identity) in [(50.0, "browser-session-1"), (1100.0, "browser-session-2")] {
        input["observation"] = anchor.clone();
        input["intent"] = json!({"kind": "pause"});
        input["clock"]["monotonicNowMs"] = json!(monotonic);
        input["clock"]["continuityId"] = json!(identity);
        input["clock"]["physicalNow"] = json!("2026-07-20T12:00:12Z");
        input["clock"]["observedAt"] = input["clock"]["physicalNow"].clone();
        input["clock"]["occurredAt"] = input["clock"]["physicalNow"].clone();
        input["identities"]["commandUuids"] = json!(["019f7f65-e4e0-7000-8000-000000000001"]);
        assert_eq!(plan(&input)["commands"][0]["observedElapsedMs"], 17000);
    }
    input["clock"]
        .as_object_mut()
        .unwrap()
        .remove("monotonicNowMs");
    input["clock"]
        .as_object_mut()
        .unwrap()
        .remove("continuityId");
    let fallback = plan(&input);
    assert_eq!(fallback["commands"][0]["observedElapsedMs"], 17000);
    assert_eq!(fallback["observation"]["monotonicAnchor"], Value::Null);
}

#[test]
fn pwa_fractional_monotonic_progress_rounds_only_generated_wire_elapsed() {
    let mut input = request("pwaStorage", "selectPhase", "running");
    input["intent"]["phase"] = json!("focus");
    input["clock"]["monotonicNowMs"] = json!(100.25);
    input["clock"]["continuityId"] = json!("browser-session-1");
    let anchor = plan(&input)["observation"]["monotonicAnchor"].clone();
    input["observation"]["monotonicAnchor"] = anchor;
    input["intent"] = json!({"kind": "pause"});
    input["clock"]["monotonicNowMs"] = json!(1100.75);
    assert_eq!(plan(&input)["commands"][0]["observedElapsedMs"], 16001);
    input["workspace"]["base"]["canonicalTimer"]["id"] = json!("replacement-timer");
    let replaced = plan(&input);
    assert_eq!(replaced["commands"][0]["observedElapsedMs"], 15000);
    assert_eq!(replaced["workspace"]["base"], input["workspace"]["base"]);
}

#[test]
fn pwa_fractional_live_elapsed_keeps_countdown_boundary_precise() {
    let mut input = request("pwaStorage", "selectPhase", "running");
    input["intent"]["phase"] = json!("focus");
    input["clock"]["monotonicNowMs"] = json!(100.25);
    input["clock"]["continuityId"] = json!("browser-session-1");
    let sampled = plan(&input);
    input["observation"] = sampled["observation"].clone();
    input["clock"]["monotonicNowMs"] = json!(1099.75);
    let live = plan(&input);
    assert_eq!(live["timerObservation"]["elapsedMs"], json!(15999.5));
    assert_eq!(live["timerObservation"]["remainingMs"], json!(44000.5));
    assert_eq!(
        (live["timerObservation"]["remainingMs"].as_f64().unwrap() / 1000.0).ceil(),
        45.0
    );
    input["intent"] = json!({"kind": "pause"});
    let paused = plan(&input);
    assert_eq!(paused["commands"][0]["observedElapsedMs"], 16000);
}

#[test]
fn pwa_absent_monotonic_sample_preserves_anchor_for_later_progress() {
    let mut input = request("pwaStorage", "selectPhase", "running");
    input["intent"]["phase"] = json!("focus");
    input["clock"]["monotonicNowMs"] = json!(100.0);
    input["clock"]["continuityId"] = json!("browser-session-1");
    let sampled = plan(&input);
    input["observation"] = sampled["observation"].clone();
    input["clock"]
        .as_object_mut()
        .unwrap()
        .remove("monotonicNowMs");
    input["clock"]
        .as_object_mut()
        .unwrap()
        .remove("continuityId");
    for field in ["observedAt", "occurredAt", "physicalNow"] {
        input["clock"][field] = json!("2026-07-20T12:00:20Z");
    }
    let absent = plan(&input);
    assert_eq!(absent["timerObservation"]["elapsedMs"], json!(25000));
    assert_eq!(
        absent["observation"]["monotonicAnchor"],
        sampled["observation"]["monotonicAnchor"]
    );
    input["observation"] = absent["observation"].clone();
    input["clock"]["monotonicNowMs"] = json!(2100.0);
    input["clock"]["continuityId"] = json!("browser-session-1");
    for field in ["observedAt", "occurredAt", "physicalNow"] {
        input["clock"][field] = json!("2026-07-20T12:00:40Z");
    }
    let live = plan(&input);
    assert_eq!(live["timerObservation"]["elapsedMs"], json!(17000));
    assert_eq!(
        live["observation"]["monotonicAnchor"],
        sampled["observation"]["monotonicAnchor"]
    );
}

#[test]
fn pwa_absent_sample_resets_only_on_explicit_continuity_change() {
    let mut input = request("pwaStorage", "selectPhase", "running");
    input["intent"]["phase"] = json!("focus");
    input["clock"]["monotonicNowMs"] = json!(100.0);
    input["clock"]["continuityId"] = json!("browser-session-1");
    let sampled = plan(&input);
    input["observation"] = sampled["observation"].clone();
    input["clock"]
        .as_object_mut()
        .unwrap()
        .remove("monotonicNowMs");
    input["clock"]["observedAt"] = json!("2026-07-20T12:00:20Z");
    input["clock"]["occurredAt"] = input["clock"]["observedAt"].clone();
    input["clock"]["physicalNow"] = input["clock"]["observedAt"].clone();
    assert_eq!(
        plan(&input)["observation"]["monotonicAnchor"],
        sampled["observation"]["monotonicAnchor"]
    );
    input["clock"]["continuityId"] = json!("browser-session-2");
    let restarted = plan(&input);
    assert_eq!(restarted["timerObservation"]["elapsedMs"], json!(25000));
    assert_eq!(restarted["observation"]["monotonicAnchor"], Value::Null);
}

#[test]
fn pwa_fractional_live_observation_does_not_complete_before_deadline() {
    let mut input = request("pwaStorage", "selectPhase", "running");
    input["intent"]["phase"] = json!("focus");
    input["clock"]["monotonicNowMs"] = json!(100.25);
    input["clock"]["continuityId"] = json!("browser-session-1");
    input["observation"] = plan(&input)["observation"].clone();
    input["clock"]["monotonicNowMs"] = json!(45099.75);
    let live = plan(&input);
    assert_eq!(live["projection"]["canonicalTimer"]["status"], "running");
    assert_eq!(live["timerObservation"]["elapsedMs"], json!(59999.5));
    assert_eq!(live["timerObservation"]["remainingMs"], json!(0.5));
    input["intent"] = json!({"kind": "pause"});
    assert_eq!(plan(&input)["commands"][0]["observedElapsedMs"], 60000);
}

#[test]
fn malformed_monotonic_observations_fail_closed_without_reservation() {
    let mut input = request("pwaStorage", "pause", "running");
    input["clock"]["monotonicNowMs"] = json!(100.0);
    input["clock"]["continuityId"] = json!("browser-session-1");
    let mut sample = input.clone();
    sample["intent"] = json!({"kind": "selectPhase", "phase": "focus"});
    let anchor = plan(&sample)["observation"]["monotonicAnchor"].clone();
    for (path, value) in [
        ("/clock/monotonicNowMs", json!(-1)),
        ("/clock/monotonicNowMs", json!(9007199254740992_i64)),
        ("/clock/continuityId", json!("")),
        ("/observation/monotonicAnchor/sampledMonotonicMs", json!(-1)),
        ("/observation/monotonicAnchor/sampledTrustedNowMs", json!(0)),
        ("/observation/monotonicAnchor/elapsedAtAnchorMs", json!(-1)),
    ] {
        let mut invalid = input.clone();
        invalid["observation"]["monotonicAnchor"] = anchor.clone();
        *invalid.pointer_mut(path).unwrap() = value;
        assert!(
            dispatch_json("workspace.intent.v1", &invalid.to_string()).is_err(),
            "{path}"
        );
    }
    let mut missing_identity = input.clone();
    missing_identity["clock"]
        .as_object_mut()
        .unwrap()
        .remove("continuityId");
    assert!(dispatch_json("workspace.intent.v1", &missing_identity.to_string()).is_err());
    input["compatibility"] = json!("appleWorkspace");
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    sample["clock"]["observedAt"] = json!("1969-12-31T23:59:59Z");
    assert!(dispatch_json("workspace.intent.v1", &sample.to_string()).is_err());
}

#[test]
fn malformed_saved_anchor_fails_even_when_stale_and_monotonic_missing() {
    let mut input = request("pwaStorage", "selectPhase", "running");
    input["intent"]["phase"] = json!("focus");
    input["clock"]["monotonicNowMs"] = json!(100.0);
    input["clock"]["continuityId"] = json!("browser-session-1");
    let anchor = plan(&input)["observation"]["monotonicAnchor"].clone();
    input["workspace"]["base"]["canonicalTimer"]["id"] = json!("replacement-timer");
    input["clock"]
        .as_object_mut()
        .unwrap()
        .remove("monotonicNowMs");
    input["clock"]
        .as_object_mut()
        .unwrap()
        .remove("continuityId");
    for (field, value) in [
        ("sampledMonotonicMs", json!(-1)),
        ("sampledTrustedNowMs", json!(0)),
        ("sampledTrustedNowMs", json!(9007199254740991_i64)),
        ("anchorAt", json!("invalid timestamp")),
        ("continuityId", json!("")),
        ("unexpectedField", json!(true)),
    ] {
        input["observation"]["monotonicAnchor"] = anchor.clone();
        input["observation"]["monotonicAnchor"][field] = value;
        assert!(
            dispatch_json("workspace.intent.v1", &input.to_string()).is_err(),
            "{field}"
        );
    }
}

#[test]
fn apple_trusted_start_uses_physical_projection_anchor() {
    let mut input = request("appleWorkspace", "start", "idle");
    input["clock"]["occurredAt"] = json!("2026-07-20T12:05:00Z");
    input["identities"]["commandUuids"] = json!(["019f7f6a-49e0-7000-8000-000000000001"]);
    let output = plan(&input);
    assert_eq!(output["commands"][0]["occurredAt"], "2026-07-20T12:05:00Z");
    assert_eq!(
        output["projection"]["canonicalTimer"]["anchorAt"],
        "2026-07-20T12:00:10Z"
    );
    assert_eq!(output["projection"]["canonicalTimer"]["status"], "running");
}

#[test]
fn android_trusted_start_keeps_wire_time_and_projects_physical_anchor() {
    let mut input = request("androidCoordinator", "start", "idle");
    input["clock"]["occurredAt"] = json!("2026-07-20T12:05:00Z");
    input["identities"]["commandUuids"] = json!(["019f7f6a-49e0-7000-8000-000000000001"]);
    let output = plan(&input);
    assert_eq!(output["commands"][0]["occurredAt"], "2026-07-20T12:05:00Z");
    assert_eq!(
        output["projection"]["canonicalTimer"]["anchorAt"],
        "2026-07-20T12:00:10Z"
    );
    assert_eq!(
        output["workspace"]["local"]["commands"][0]["occurredAt"],
        "2026-07-20T12:05:00Z"
    );
}

#[test]
fn hlc_ahead_does_not_change_apple_occurrence_but_pwa_uses_wall() {
    for profile in ["appleWorkspace", "pwaStorage"] {
        let mut input = request(profile, "start", "idle");
        input["allocation"]["hlc"] = json!({"wallMs": 1784548830000_i64, "counter": 9});
        input["identities"]["commandUuids"] = json!(["019f7f66-2b30-7000-8000-000000000001"]);
        let output = plan(&input);
        let command = &output["commands"][0];
        assert_eq!(command["hlcWallMs"], 1784548830000_i64);
        assert_eq!(command["hlcCounter"], 10);
        assert_eq!(
            command["occurredAt"],
            if profile == "pwaStorage" {
                "2026-07-20T12:00:30.000Z"
            } else {
                "2026-07-20T12:00:10Z"
            }
        );
    }
}

#[test]
fn atomic_failure_leaves_inputs_unchanged() {
    let mut input = request("appleWorkspace", "cancel", "running");
    input["identities"]["commandUuids"]
        .as_array_mut()
        .unwrap()
        .pop();
    let saved = input.clone();
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    assert_eq!(input, saved);
}

#[test]
fn malformed_boundary_inputs_fail_closed() {
    let mutations = [
        ("/intent/kind", json!("finish")),
        ("/compatibility", json!("default")),
        ("/selection/generation", json!("05")),
        ("/selection/generation", json!("-1")),
        ("/allocation/deviceSequence", json!(9007199254740991_i64)),
        ("/allocation/deviceId", json!("")),
        ("/identities/timerUuid", json!("not-a-uuid")),
        ("/identities/commandUuids", json!([])),
        ("/clock/observedAt", json!("2026-07-20T12:00:11Z")),
        (
            "/observation/canonicalAnchorAt",
            json!("2026-07-20T12:00:00Z"),
        ),
        (
            "/observation/commandTimes",
            json!({"foreign": "2026-07-20T12:00:00Z"}),
        ),
    ];
    for (path, value) in mutations {
        let mut input = request("appleWorkspace", "start", "idle");
        *input.pointer_mut(path).unwrap() = value;
        assert!(
            dispatch_json("workspace.intent.v1", &input.to_string()).is_err(),
            "{path}"
        );
    }
    assert!(dispatch_json("workspace.intent.v1", "{\"intent\":{},\"intent\":{}}").is_err());
}

#[test]
fn retained_payloads_remain_exact_and_frozen_domains_block_display_not_admission() {
    let mut input = request("appleWorkspace", "start", "idle");
    let retained = json!({"id": "duration-old", "deviceId": "other-device", "phase": "focus",
        "durationMs": 120000, "occurredAt": "2026-07-20T12:00:01Z", "hlcWallMs": 1784548801000_i64,
        "hlcCounter": 0, "extension": {"omitted": null}});
    input["workspace"]["local"]["durationOperations"] = json!([retained]);
    let output = plan(&input);
    assert_eq!(
        output["workspace"]["local"]["durationOperations"],
        input["workspace"]["local"]["durationOperations"]
    );
    assert_eq!(output["commands"][0]["plannedDurationMs"], 60000);
    let mut next = input.clone();
    next["workspace"] = output["workspace"].clone();
    next["allocation"] = output["allocation"].clone();
    next["observation"] = output["observation"].clone();
    next["identities"]["commandUuids"] = json!(["019f7f65-dd10-7000-8000-000000000002"]);
    next["intent"]["kind"] = json!("pause");
    let pause = plan(&next);
    assert_eq!(
        pause["workspace"]["local"]["commands"][0],
        output["commands"][0]
    );
    assert_eq!(pause["projection"]["canonicalTimer"]["status"], "paused");
    next["workspace"]["neverSent"]["commands"] = json!([]);
    next["intent"]["kind"] = json!("start");
    next["identities"]["timerUuid"] = json!("12345678-1234-4234-8234-123456789013");
    let queued = plan(&next);
    assert_eq!(queued["commandOutcomes"][0]["outcome"], "queued");
    assert_eq!(
        queued["workspace"]["local"]["commands"][0],
        output["commands"][0]
    );
    assert!(queued["projection"]["canonicalTimer"].is_null());
}

#[test]
fn start_task_and_prefix_contracts() {
    let task: Value =
        serde_json::from_str(&dispatch_json("task.identity.v1", r#"{"title":"Work"}"#).unwrap())
            .unwrap();
    for profile in [
        "appleWorkspace",
        "androidCoordinator",
        "desktopStorage",
        "pwaStorage",
    ] {
        for phase in ["focus", "short_break", "long_break"] {
            let mut input = request(profile, "start", "idle");
            input["workspace"]["base"]["tasks"] = json!([{"id": task["id"], "title": "Work"}]);
            input["workspace"]["base"]["selectedTaskId"] = task["id"].clone();
            input["selection"]["phase"] = json!(phase);
            let output = plan(&input);
            let command = &output["commands"][0];
            assert_eq!(
                command.get("taskId"),
                (phase == "focus").then_some(&task["id"])
            );
            assert_eq!(command["observedElapsedMs"], 0);
            assert_eq!(
                command["plannedDurationMs"],
                input["workspace"]["base"]["durationsMs"][phase]
            );
            let prefix = if profile == "appleWorkspace" {
                "command-"
            } else {
                ""
            };
            assert_eq!(
                command["id"],
                format!("{prefix}019f7f65-dd10-7000-8000-000000000001")
            );
            assert_eq!(output["ownershipWrites"][0]["kind"], "recordStart");
        }
    }
}

#[test]
fn expired_cancel_group_and_replication_mode_keep_existing_distinctions() {
    let mut input = request("androidCoordinator", "cancelAndClear", "running");
    input["clock"]["physicalNow"] = json!("2026-07-20T12:01:10Z");
    input["clock"]["observedAt"] = input["clock"]["physicalNow"].clone();
    input["clock"]["occurredAt"] = input["clock"]["physicalNow"].clone();
    input["identities"]["commandUuids"] = json!(["019f7f66-c770-7000-8000-000000000001"]);
    let android = plan(&input);
    assert_eq!(android["commands"].as_array().unwrap().len(), 1);
    assert_eq!(android["commands"][0]["type"], "clear");
    assert_eq!(android["selection"]["phase"], "short_break");
    assert_eq!(android["selection"]["generation"], "6");
    input["compatibility"] = json!("appleWorkspace");
    input["intent"]["kind"] = json!("pause");
    assert_eq!(plan(&input)["commands"][0]["type"], "pause");
    input["replicationMode"] = json!("iroh");
    assert_eq!(plan(&input)["outcome"], "noop");
}

#[test]
fn stale_group_fingerprints_do_not_reserve_or_clear_new_timers() {
    for profile in ["desktopStorage", "desktopTerminal", "pwaStorage"] {
        let mut input = request(profile, "cancelAndClear", "running");
        input["requestedTimer"] = input["workspace"]["base"]["canonicalTimer"].clone();
        input["requestedTimer"]["id"] = json!("stale-timer");
        let output = plan(&input);
        assert_eq!(output["reason"], "staleTimer");
        assert_eq!(output["commands"], json!([]));
        assert_eq!(output["allocation"], input["allocation"]);
    }
}

#[test]
fn grouped_timer_requires_requested_identity_snapshot() {
    for profile in ["desktopStorage", "desktopTerminal", "pwaStorage"] {
        let mut input = request(profile, "cancelAndClear", "running");
        input.as_object_mut().unwrap().remove("requestedTimer");
        assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    }
}

#[test]
fn skip_requires_apple_profile_and_complete_calendar() {
    let mut input = request("androidCoordinator", "skip", "idle");
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["compatibility"] = json!("appleWorkspace");
    input["calendarIntervals"] = json!([]);
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
}

#[test]
fn clock_skew_and_reused_allocations_fail_without_any_plan() {
    let mut input = request("appleWorkspace", "start", "idle");
    input["allocation"]["hlc"]["wallMs"] = json!(1784549110001_i64);
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["allocation"]["hlc"]["wallMs"] = json!(1784548800000_i64);
    input["allocation"]["lastUuid"] = input["identities"]["commandUuids"][0].clone();
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["allocation"]["lastUuid"] = Value::Null;
    input["identities"]["commandUuids"][1] = input["identities"]["commandUuids"][0].clone();
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["identities"]["commandUuids"] = json!(["019f7f66-2b30-7000-8000-000000000002"]);
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["allocation"]["lastUuid"] = json!("019f7f66-2b30-7000-8000-000000000001");
    assert_eq!(plan(&input)["commands"][0]["hlcWallMs"], 1784548810000_i64);
}

#[test]
fn selection_only_commits_without_clock_or_identity_consumption() {
    let mut input = request("appleWorkspace", "selectPhase", "running");
    input["intent"]["phase"] = json!("short_break");
    let output = plan(&input);
    assert_eq!(output["outcome"], "planned");
    assert_eq!(output["commands"], json!([]));
    assert_eq!(
        output["selection"],
        json!({"phase":"short_break", "generation":"6", "explicit":true})
    );
    assert_eq!(output["allocation"], input["allocation"]);
    assert_eq!(output["workspace"], input["workspace"]);
    assert_eq!(output["effectsAfterCommit"], json!([]));
}

#[test]
fn elapsed_backwards_and_hlc_counter_overflow() {
    let mut input = request("appleWorkspace", "pause", "running");
    input["clock"]["observedAt"] = json!("2026-07-20T11:59:00Z");
    assert_eq!(plan(&input)["commands"][0]["observedElapsedMs"], 5000);
    input["allocation"]["hlc"] =
        json!({"wallMs": 1784548810000_i64, "counter": 9007199254740991_i64});
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
    input["intent"]["kind"] = json!("resume");
    assert_eq!(plan(&input)["allocation"], input["allocation"]);
}

#[test]
fn desktop_terminal_cancel_rechecks_presented_timer_fingerprint() {
    let mut input = checker_request(&checker_case("terminalCancelStaleAnchor"));
    let stale = plan(&input);
    assert_eq!(stale["outcome"], "noop");
    assert_eq!(stale["reason"], "staleTimer");
    assert_eq!(stale["commands"], json!([]));
    assert_eq!(stale["allocation"], input["allocation"]);
    input = checker_request(&checker_case("terminalCancelSameTimer"));
    assert_eq!(
        plan(&input)["atomicCommandIds"].as_array().unwrap().len(),
        2
    );
    input.as_object_mut().unwrap().remove("requestedTimer");
    assert!(dispatch_json("workspace.intent.v1", &input.to_string()).is_err());
}

#[test]
fn desktop_restart_recovers_only_matching_retained_terminal_history() {
    let mut input = checker_request(&checker_case("storageRestartEvicted"));
    let presented = input["requestedTimer"].clone();
    let restarted = plan(&input);
    assert_eq!(
        restarted["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|command| command["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["clear", "start"]
    );
    assert_eq!(restarted["commands"][0]["timerId"], presented["id"]);
    assert_eq!(restarted["atomicCommandIds"].as_array().unwrap().len(), 2);
    assert_eq!(
        restarted["projection"]["canonicalTimer"]["status"],
        "running"
    );
    assert_eq!(
        restarted["workspace"]["base"]["canonicalTimer"],
        Value::Null
    );
    input = checker_request(&checker_case("storageRestartUnrelatedId"));
    assert_eq!(plan(&input)["commands"], json!([]));
    input = checker_request(&checker_case("storageRestartEvicted"));
    input["workspace"]["local"]["commands"] = restarted["commands"].clone();
    input["workspace"]["neverSent"]["commands"] = restarted["atomicCommandIds"].clone();
    assert_eq!(plan(&input)["commands"], json!([]));
}

#[test]
fn iroh_start_never_claims_centralized_ownership() {
    for profile in ["appleWorkspace", "desktopStorage", "androidCoordinator"] {
        let mut input = checker_request(&checker_case("centralizedStartOwnership"));
        input["compatibility"] = json!(profile);
        assert_eq!(plan(&input)["ownershipWrites"][0]["kind"], "recordStart");
        input["replicationMode"] = checker_case("irohStartOwnership")["replicationMode"].clone();
        assert!(
            plan(&input)["ownershipWrites"]
                .as_array()
                .unwrap()
                .iter()
                .all(|write| write["kind"] != "recordStart")
        );
    }
}
