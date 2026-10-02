use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const AT: &str = "2026-08-25T12:00:00.000Z";

fn history(id: &str, phase: &str) -> Value {
    json!({"id":format!("history-{id}"),"timerId":id,"commandId":format!("finish-{id}"),
        "phase":phase,"status":"completed","plannedDurationMs":60000,"completedAt":AT})
}

fn input(profile: &str) -> Value {
    json!({"kind":"install","compatibility":profile,"beforeHistory":[],"afterHistory":[],
        "canonicalTimer":null,"selection":{"phase":"focus","generation":"0","explicit":false},
        "pending":{"commandIds":[],"sendableCommandIds":[],"otherOperationIds":[]},
        "advances":[],"acknowledgements":[],"discardedCommandIds":[],"referenceTime":AT,
        "calendarIntervals":[{"start":"2026-08-25T00:00:00Z","end":"2026-08-26T00:00:00Z"}]})
}

fn plan(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("timer.completionState.v1", &input.to_string()).unwrap())
        .unwrap()
}

fn fixture_history(ids: &Value) -> Vec<Value> {
    ids.as_array()
        .unwrap()
        .iter()
        .map(|id| {
            let id = id.as_str().unwrap();
            history(id, if id == "z" { "long_break" } else { "focus" })
        })
        .collect()
}

#[test]
fn shared_ap04_d03_parity_fixtures_are_order_independent() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../fixtures/completion-state-v1.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let mut request = input(case["compatibility"].as_str().unwrap());
        request["beforeHistory"] = json!(fixture_history(&case["before"]));
        request["afterHistory"] = json!(fixture_history(&case["after"]));
        request["selection"]["phase"] = case["selected"].clone();
        request["selection"]["explicit"] = case["explicit"].clone();
        let result = plan(&request);
        assert_eq!(
            result["source"]["timerId"], case["expectedSource"],
            "{}",
            case["name"]
        );
        assert_eq!(
            result["selection"]["phase"], case["expectedPhase"],
            "{}",
            case["name"]
        );
        request["afterHistory"].as_array_mut().unwrap().reverse();
        request["beforeHistory"].as_array_mut().unwrap().reverse();
        assert_eq!(plan(&request), result);
    }
}

#[test]
fn all_four_destinations_and_counts_zero_through_twelve() {
    for count in 0..=12 {
        for phase in ["focus", "short_break", "long_break"] {
            let rows: Vec<_> = (0..count)
                .map(|n| history(&n.to_string(), "focus"))
                .collect();
            let request = json!({"kind":"skip","selection":{"phase":phase,"generation":"9","explicit":true},
                "sourcePhase":phase,"history":rows,"referenceTime":AT,
                "calendarIntervals":input("desktopD03")["calendarIntervals"]});
            let expected = if phase != "focus" {
                "focus"
            } else if count % 4 == 3 {
                "long_break"
            } else {
                "short_break"
            };
            let result = plan(&request);
            assert_eq!(result["selection"]["phase"], expected);
            assert_eq!(result["selection"]["generation"], "9");
            assert!(result["source"].is_null());
        }
    }
}

#[test]
fn durable_install_consumes_even_when_blocked_and_does_not_retry_after_restart() {
    let mut request = input("desktopD03");
    request["afterHistory"] = json!([history("a", "focus")]);
    request["pending"]["otherOperationIds"] = json!(["task-pending"]);
    assert_eq!(plan(&request)["reason"], "pendingOperations");
    request["beforeHistory"] = request["afterHistory"].clone();
    request["pending"]["otherOperationIds"] = json!([]);
    let durable: Value = serde_json::from_str(&request.to_string()).unwrap();
    assert_eq!(plan(&durable)["reason"], "noNewCompletion");
    assert_eq!(plan(&durable)["selection"]["phase"], "focus");
}

#[test]
fn older_backfill_and_timestamp_corrections_do_not_reconsume_identity() {
    let mut request = input("desktopD03");
    let latest = history("latest", "focus");
    let mut older = history("older", "long_break");
    older["completedAt"] = json!("2026-08-25T11:00:00Z");
    request["beforeHistory"] = json!([latest.clone()]);
    request["afterHistory"] = json!([older, latest]);
    assert_eq!(plan(&request)["reason"], "noNewCompletion");
    request["afterHistory"][1]["completedAt"] = json!("2026-08-25T13:00:00Z");
    request["afterHistory"][1]["id"] = json!("corrected-row");
    assert_eq!(plan(&request)["reason"], "noNewCompletion");
}

fn advance(id: &str, previous: &str, next: &str, generation: &str) -> Value {
    json!({"commandId":format!("finish-{id}"),"timerId":id,"previousPhase":previous,
        "advancedPhase":next,"generation":generation})
}

#[test]
fn apple_rejected_generated_chain_unwinds_in_reverse_and_survives_restart() {
    let mut request = input("appleAp04");
    request["selection"]["generation"] = json!("2");
    request["advances"] = json!([
        advance("focus", "focus", "short_break", "1"),
        advance("generated", "short_break", "focus", "2")
    ]);
    request["acknowledgements"] = json!([{"commandId":"finish-focus","outcome":"rejected"}]);
    let result = plan(&request);
    assert_eq!(
        result["rolledBackAdvanceIds"],
        json!(["finish-generated", "finish-focus"])
    );
    assert_eq!(result["selection"]["generation"], "0");
    assert_eq!(result["selection"]["phase"], "focus");
    assert_eq!(result["advances"], json!([]));
    request["selection"] = result["selection"].clone();
    request["advances"] = result["advances"].clone();
    assert_eq!(plan(&request)["rolledBackAdvanceIds"], json!([]));
}

#[test]
fn explicit_generation_change_prevents_rollback_even_for_same_phase() {
    for profile in ["appleAp04", "desktopD03"] {
        let mut request = input(profile);
        request["selection"] = json!({"phase":"short_break","generation":"2","explicit":true});
        request["advances"] = json!([advance("a", "focus", "short_break", "1")]);
        request["acknowledgements"] = json!([{"commandId":"finish-a","outcome":"rejected"}]);
        let result = plan(&request);
        assert_eq!(result["selection"], request["selection"]);
        assert_eq!(result["rolledBackAdvanceIds"], json!([]));
        assert_eq!(result["retiredAdvanceIds"], json!(["finish-a"]));
    }
}

#[test]
fn acknowledgement_divergences_are_explicit_not_silently_unified() {
    for (outcome, evidence, apple_rollback, desktop_rollback) in [
        ("applied", false, true, false),
        ("ignored", false, true, true),
        ("rejected", true, true, false),
        ("ignored", true, false, false),
    ] {
        for (profile, rollback) in [
            ("appleAp04", apple_rollback),
            ("desktopD03", desktop_rollback),
        ] {
            let mut request = input(profile);
            request["selection"] = json!({"phase":"short_break","generation":"1","explicit":true});
            request["advances"] = json!([advance("a", "focus", "short_break", "1")]);
            request["acknowledgements"] = json!([{"commandId":"finish-a","outcome":outcome}]);
            request["pending"]["commandIds"] = json!(["pending"]);
            request["pending"]["sendableCommandIds"] = json!(["pending"]);
            if evidence {
                request["afterHistory"] = json!([history("a", "focus")]);
            }
            assert_eq!(
                !plan(&request)["rolledBackAdvanceIds"]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                rollback
            );
        }
    }
}

#[test]
fn missing_ack_retains_advance_and_discard_resolves_it() {
    let mut request = input("desktopD03");
    request["selection"]["phase"] = json!("short_break");
    request["advances"] = json!([advance("a", "focus", "short_break", "0")]);
    assert_eq!(plan(&request)["advances"], request["advances"]);
    request["discardedCommandIds"] = json!(["finish-a"]);
    let result = plan(&request);
    assert_eq!(result["selection"]["phase"], "focus");
    assert_eq!(result["selection"]["generation"], "0");
}

#[test]
fn apple_discard_only_retains_unacknowledged_advance() {
    let mut request = input("appleAp04");
    request["selection"] = json!({"phase":"short_break","generation":"1","explicit":false});
    request["advances"] = json!([advance("f", "focus", "short_break", "1")]);
    request["discardedCommandIds"] = json!(["finish-f"]);
    let result = plan(&request);
    assert_eq!(result["selection"], request["selection"]);
    assert_eq!(result["advances"], request["advances"]);
    assert_eq!(result["retiredAdvanceIds"], json!([]));
    assert_eq!(result["rolledBackAdvanceIds"], json!([]));
}

#[test]
fn generation_wrap_is_lossless_through_json() {
    let mut request = input("appleAp04");
    request["selection"]["phase"] = json!("short_break");
    request["advances"] = json!([advance("a", "focus", "short_break", "0")]);
    request["acknowledgements"] = json!([{"commandId":"finish-a","outcome":"rejected"}]);
    assert_eq!(
        plan(&request)["selection"]["generation"],
        i64::MAX.to_string()
    );
}

#[test]
fn raw_identity_and_timestamp_are_preserved_with_ended_at_fallback() {
    let mut request = input("desktopD03");
    let mut row = history("TIMER-Ä", "focus");
    row.as_object_mut().unwrap().remove("commandId");
    row.as_object_mut().unwrap().remove("completedAt");
    row["endedAt"] = json!("2026-08-25T14:00:00+02:00");
    row["unknownWireField"] = json!({"neverRewrite":true});
    request["afterHistory"] = json!([row]);
    let before = request.to_string();
    let result = plan(&request);
    assert_eq!(result["source"]["timerId"], "TIMER-Ä");
    assert_eq!(result["source"]["occurredAt"], "2026-08-25T14:00:00+02:00");
    assert!(result["source"]["commandId"].is_null());
    assert_eq!(request.to_string(), before);
}

#[test]
fn reference_day_divergence_and_half_open_calendar_bounds() {
    let mut request = input("desktopD03");
    request["referenceTime"] = json!("2026-08-26T00:00:00Z");
    request["calendarIntervals"]
        .as_array_mut()
        .unwrap()
        .push(json!({
        "start":"2026-08-26T00:00:00Z","end":"2026-08-27T00:00:00Z"}));
    request["afterHistory"] = json!([history("a", "focus")]);
    assert_eq!(plan(&request)["selection"]["phase"], "short_break");
    request["compatibility"] = json!("appleAp04");
    assert_eq!(plan(&request)["reason"], "outsideReferenceDay");
}

#[test]
fn malformed_inputs_fail_closed_even_when_selection_is_blocked() {
    let base = input("appleAp04");
    let changes = [
        ("/selection/generation", json!("9223372036854775808")),
        ("/selection/generation", json!("01")),
        ("/selection/generation", json!(-1)),
        ("/selection/phase", json!("invalid")),
        ("/referenceTime", json!("bad")),
        ("/pending/commandIds", json!(["duplicate", "duplicate"])),
        ("/pending/sendableCommandIds", json!(["absent"])),
        (
            "/acknowledgements",
            json!([{"commandId":"a","outcome":"unknown"}]),
        ),
        ("/calendarIntervals/0/end", json!("2026-08-24T00:00:00Z")),
    ];
    for (pointer, value) in changes {
        let mut request = base.clone();
        request["selection"]["explicit"] = json!(true);
        *request.pointer_mut(pointer).unwrap() = value;
        assert!(
            dispatch_json("timer.completionState.v1", &request.to_string()).is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn installation_completion_cadence_is_not_skip_cadence() {
    for profile in ["appleAp04", "desktopD03"] {
        for count in 1..=12 {
            let mut request = input(profile);
            request["afterHistory"] = json!(
                (0..count)
                    .map(|n| history(&n.to_string(), "focus"))
                    .collect::<Vec<_>>()
            );
            let expected = if count % 4 == 0 {
                "long_break"
            } else {
                "short_break"
            };
            assert_eq!(plan(&request)["selection"]["phase"], expected);
        }
    }
}

#[test]
fn blocked_only_commands_diverge_and_active_timer_always_blocks() {
    let mut request = input("desktopD03");
    request["afterHistory"] = json!([history("a", "focus")]);
    request["pending"]["commandIds"] = json!(["blocked-generated"]);
    assert_eq!(plan(&request)["reason"], "completionSelected");
    request["compatibility"] = json!("appleAp04");
    assert_eq!(plan(&request)["reason"], "pendingCommands");
    request["pending"]["commandIds"] = json!([]);
    request["canonicalTimer"] = json!({"id":"active","phase":"focus","status":"running",
        "plannedDurationMs":60000,"elapsedAtAnchorMs":0,"anchorAt":AT});
    for profile in ["appleAp04", "desktopD03"] {
        request["compatibility"] = json!(profile);
        assert_eq!(plan(&request)["reason"], "activeTimer");
    }
}

#[test]
fn exact_evidence_checks_identity_and_preserves_documented_phase_difference() {
    for profile in ["appleAp04", "desktopD03"] {
        let mut request = input(profile);
        request["selection"] = json!({"phase":"short_break","generation":"1","explicit":true});
        request["advances"] = json!([advance("a", "focus", "short_break", "1")]);
        request["acknowledgements"] = json!([{"commandId":"finish-a","outcome":"ignored"}]);
        request["pending"] =
            json!({"commandIds":["p"],"sendableCommandIds":["p"],"otherOperationIds":[]});
        let mut row = history("a", "focus");
        row["commandId"] = json!("FINISH-a");
        request["afterHistory"] = json!([row]);
        assert_eq!(plan(&request)["rolledBackAdvanceIds"], json!(["finish-a"]));
        request["afterHistory"][0] = history("a", "long_break");
        let expected = if profile == "appleAp04" {
            json!([])
        } else {
            json!(["finish-a"])
        };
        assert_eq!(plan(&request)["rolledBackAdvanceIds"], expected);
    }
}

#[test]
fn interval_identity_and_record_validation_reject_ambiguous_state() {
    let mut cases = vec![];
    let mut request = input("desktopD03");
    request["afterHistory"] = json!([history("a", "focus")]);
    request["calendarIntervals"] = json!([]);
    cases.push(request);
    let mut request = input("desktopD03");
    let duplicate = request["calendarIntervals"][0].clone();
    request["calendarIntervals"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    cases.push(request);
    for field in ["beforeHistory", "afterHistory"] {
        let mut request = input("desktopD03");
        request[field] = json!([history("a", "focus"), history("a", "focus")]);
        cases.push(request);
    }
    for field in [
        "commandId",
        "timerId",
        "previousPhase",
        "advancedPhase",
        "generation",
    ] {
        let mut request = input("desktopD03");
        request["advances"] = json!([advance("a", "focus", "short_break", "1")]);
        request["advances"][0][field] = json!("");
        cases.push(request);
    }
    let mut request = input("desktopD03");
    request["acknowledgements"] = json!([
        {"commandId":"a","outcome":"applied"},{"commandId":"a","outcome":"rejected"}]);
    cases.push(request);
    for request in cases {
        assert!(dispatch_json("timer.completionState.v1", &request.to_string()).is_err());
    }
}

#[test]
fn duplicate_raw_json_is_rejected_including_unknown_extensions() {
    let mut request = input("desktopD03");
    request["afterHistory"] = json!([history("a", "focus")]);
    let raw = request.to_string();
    for invalid in [
        raw.replace("\"timerId\":\"a\"", "\"timerId\":\"a\",\"timerId\":\"b\""),
        raw.replace(
            "\"timerId\":\"a\"",
            "\"timerId\":\"a\",\"extension\":{\"x\":1,\"x\":2}",
        ),
        raw.replace(
            "\"kind\":\"install\"",
            "\"kind\":\"install\",\"unexpected\":true",
        ),
    ] {
        assert_ne!(invalid, raw);
        assert!(dispatch_json("timer.completionState.v1", &invalid).is_err());
    }
}

#[test]
fn desktop_millisecond_ties_do_not_silently_adopt_apple_fractional_order() {
    let mut request = input("desktopD03");
    let mut first = history("a", "focus");
    first["completedAt"] = json!("2026-08-25T12:00:00.0001Z");
    let mut last = history("z", "long_break");
    last["completedAt"] = json!("2026-08-25T12:00:00.0009Z");
    request["afterHistory"] = json!([last, first]);
    assert_eq!(plan(&request)["source"]["timerId"], "a");
    request["compatibility"] = json!("appleAp04");
    assert_eq!(plan(&request)["source"]["timerId"], "z");
}

fn rollback_request(profile: &str) -> Value {
    let mut request = input(profile);
    request["selection"] = json!({"phase":"short_break","generation":"1","explicit":false});
    request["advances"] = json!([advance("f", "focus", "short_break", "1")]);
    request["pending"] =
        json!({"commandIds":["pending"],"sendableCommandIds":["pending"],"otherOperationIds":[]});
    request
}

fn completion_timer() -> Value {
    json!({"id":"f","phase":"focus","status":"completed","plannedDurationMs":60000,
        "elapsedAtAnchorMs":60000,"anchorAt":AT,
        "lastIntent":{"type":"finish","commandId":"finish-f","occurredAt":AT}})
}

#[test]
fn apple_acknowledged_discard_uses_ack_and_evidence_only() {
    for outcome in ["applied", "ignored", "rejected"] {
        for evidence in [false, true] {
            let mut request = rollback_request("appleAp04");
            request["acknowledgements"] = json!([{"commandId":"finish-f","outcome":outcome}]);
            if evidence {
                request["canonicalTimer"] = completion_timer();
            }
            let without_discard = plan(&request);
            request["discardedCommandIds"] = json!(["finish-f"]);
            let result = plan(&request);
            assert_eq!(result, without_discard);
            let invalid = outcome == "rejected" || !evidence;
            assert_eq!(
                result["rolledBackAdvanceIds"],
                if invalid {
                    json!(["finish-f"])
                } else {
                    json!([])
                }
            );
            assert_eq!(result["retiredAdvanceIds"], json!(["finish-f"]));
            assert_eq!(result["advances"], json!([]));
        }
    }
}

#[test]
fn discard_only_parent_does_not_invalidate_suffix_but_rejected_ack_does() {
    let mut request = rollback_request("appleAp04");
    request["selection"] = json!({"phase":"focus","generation":"2","explicit":false});
    request["advances"] = json!([
        advance("f", "focus", "short_break", "1"),
        advance("child", "short_break", "focus", "2")
    ]);
    request["discardedCommandIds"] = json!(["finish-f", "finish-child"]);
    let retained = plan(&request);
    assert_eq!(retained["advances"], request["advances"]);
    assert_eq!(retained["selection"], request["selection"]);
    assert_eq!(retained["retiredAdvanceIds"], json!([]));
    request["acknowledgements"] = json!([{"commandId":"finish-f","outcome":"rejected"}]);
    let rejected = plan(&request);
    assert_eq!(
        rejected["retiredAdvanceIds"],
        json!(["finish-child", "finish-f"])
    );
    assert_eq!(
        rejected["rolledBackAdvanceIds"],
        rejected["retiredAdvanceIds"]
    );
    assert_eq!(rejected["selection"]["phase"], "focus");
    assert_eq!(rejected["selection"]["generation"], "0");
    assert_eq!(rejected["advances"], json!([]));
}

#[test]
fn canonical_timer_only_evidence_preserves_each_profiles_exact_predicate() {
    for profile in ["appleAp04", "desktopD03"] {
        for (pointer, value, apple_exact, desktop_exact) in [
            ("/id", json!("f"), true, true),
            ("/id", json!("other"), false, false),
            ("/status", json!("cancelled"), false, false),
            ("/phase", json!("long_break"), true, false),
            ("/lastIntent", Value::Null, false, false),
            ("/lastIntent/type", json!("pause"), false, true),
            ("/lastIntent/commandId", json!("other"), false, false),
        ] {
            let mut request = rollback_request(profile);
            let mut timer = completion_timer();
            *timer.pointer_mut(pointer).unwrap() = value;
            request["canonicalTimer"] = timer;
            request["acknowledgements"] = json!([{"commandId":"finish-f","outcome":"ignored"}]);
            let exact = if profile == "appleAp04" {
                apple_exact
            } else {
                desktop_exact
            };
            let result = plan(&request);
            assert_eq!(
                result["rolledBackAdvanceIds"],
                if exact {
                    json!([])
                } else {
                    json!(["finish-f"])
                },
                "{profile} {pointer}"
            );
            assert_eq!(result["advances"], json!([]));
        }
    }
}

#[test]
fn desktop_discard_ack_matrix_remains_independent_of_apple_fix() {
    for outcome in [None, Some("applied"), Some("ignored"), Some("rejected")] {
        for discarded in [false, true] {
            for evidence in [false, true] {
                let mut request = rollback_request("desktopD03");
                if let Some(outcome) = outcome {
                    request["acknowledgements"] =
                        json!([{"commandId":"finish-f","outcome":outcome}]);
                }
                if discarded {
                    request["discardedCommandIds"] = json!(["finish-f"]);
                }
                if evidence {
                    request["canonicalTimer"] = completion_timer();
                }
                let result = plan(&request);
                let resolved = discarded || outcome.is_some();
                let restored = resolved && !evidence && (discarded || outcome != Some("applied"));
                assert_eq!(
                    result["selection"]["phase"],
                    if restored { "focus" } else { "short_break" }
                );
                assert_eq!(result["selection"]["generation"], "1");
                assert_eq!(
                    result["advances"],
                    if resolved {
                        json!([])
                    } else {
                        request["advances"].clone()
                    }
                );
                assert_eq!(
                    result["retiredAdvanceIds"],
                    if resolved {
                        json!(["finish-f"])
                    } else {
                        json!([])
                    }
                );
            }
        }
    }
}

#[test]
fn apple_middle_rejection_retains_prefix_and_unwinds_acknowledged_child() {
    let mut request = rollback_request("appleAp04");
    request["selection"] = json!({"phase":"short_break","generation":"3","explicit":false});
    let prefix = advance("prefix", "focus", "short_break", "1");
    request["advances"] = json!([
        prefix.clone(),
        advance("middle", "short_break", "focus", "2"),
        advance("f", "focus", "short_break", "3")
    ]);
    request["discardedCommandIds"] = json!(["finish-prefix"]);
    request["canonicalTimer"] = completion_timer();
    request["acknowledgements"] = json!([{"commandId":"finish-middle","outcome":"rejected"},
        {"commandId":"finish-f","outcome":"applied"}]);
    let result = plan(&request);
    assert_eq!(result["advances"], json!([prefix]));
    assert_eq!(
        result["retiredAdvanceIds"],
        json!(["finish-f", "finish-middle"])
    );
    assert_eq!(result["rolledBackAdvanceIds"], result["retiredAdvanceIds"]);
    assert_eq!(result["selection"]["phase"], "short_break");
    assert_eq!(result["selection"]["generation"], "1");
}

#[test]
fn desktop_multi_record_rollback_keeps_generation_and_input_order() {
    let mut request = rollback_request("desktopD03");
    request["selection"]["phase"] = json!("focus");
    let child = advance("child", "short_break", "focus", "1");
    let parent = advance("f", "focus", "short_break", "1");
    request["advances"] = json!([child.clone(), parent.clone()]);
    request["discardedCommandIds"] = json!(["finish-child", "finish-f"]);
    let result = plan(&request);
    assert_eq!(
        result["rolledBackAdvanceIds"],
        json!(["finish-child", "finish-f"])
    );
    assert_eq!(result["selection"], request["selection"]);
    request["advances"] = json!([parent, child]);
    let reversed = plan(&request);
    assert_eq!(
        reversed["retiredAdvanceIds"],
        json!(["finish-f", "finish-child"])
    );
    assert_eq!(reversed["rolledBackAdvanceIds"], json!(["finish-child"]));
    assert_eq!(reversed["selection"]["phase"], "short_break");
    assert_eq!(reversed["selection"]["generation"], "1");
}
