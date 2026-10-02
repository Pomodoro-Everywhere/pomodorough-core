use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const DOMAINS: [&str; 5] = [
    "commands",
    "taskOperations",
    "durationOperations",
    "autoStartOperations",
    "selectedTaskOperations",
];

fn fixtures() -> Value {
    serde_json::from_str(include_str!("../fixtures/batch-plan-v1.json")).unwrap()
}

fn descriptor(index: usize, domain: usize) -> Value {
    let mut operation = json!({"id": format!("op-{index:05}"), "deviceId":"device-a",
        "hlcWallMs": index + 1, "hlcCounter": 0});
    if domain == 0 {
        operation["deviceSequence"] = json!(index + 1);
    }
    operation
}

fn request(counts: [usize; 5], mode: &str) -> Value {
    let (per_domain, total) = if mode == "sync" {
        (256, 512)
    } else {
        (4096, 8192)
    };
    let queues: serde_json::Map<_, _> = DOMAINS
        .iter()
        .enumerate()
        .map(|(domain, name)| {
            (
                (*name).into(),
                (0..counts[domain])
                    .rev()
                    .map(|i| descriptor(i, domain))
                    .collect(),
            )
        })
        .collect();
    json!({"kind":"new", "mode":mode, "nextDomain":"commands",
        "limits":{"perDomain":per_domain,"total":total}, "queues":queues,
        "timerDependencies":[]})
}

fn call(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("sync.batchPlan.v1", &input.to_string()).unwrap()).unwrap()
}

fn ids(queue: &Value) -> Value {
    queue
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].clone())
        .collect()
}

fn selected_count(output: &Value) -> usize {
    DOMAINS
        .iter()
        .map(|name| output["selected"][name].as_array().unwrap().len())
        .sum()
}

#[test]
fn shared_boundary_vectors_select_exact_existing_ids() {
    for case in fixtures()["boundaries"].as_array().unwrap() {
        let counts: [usize; 5] = serde_json::from_value(case["counts"].clone()).unwrap();
        let input = request(counts, case["mode"].as_str().unwrap());
        let output = call(&input);
        assert_eq!(output["status"], case["status"], "{}", case["name"]);
        assert_eq!(output["total"], counts.iter().sum::<usize>());
        for (domain, name) in DOMAINS.iter().enumerate() {
            let count = case["selectedCounts"][domain].as_u64().unwrap() as usize;
            let expected: Vec<_> = (0..count).map(|i| format!("op-{i:05}")).collect();
            assert_eq!(
                output["selected"][name],
                json!(expected),
                "{} {name}",
                case["name"]
            );
            assert_eq!(output["counts"][name], counts[domain]);
        }
        assert_eq!(
            call(&serde_json::from_str(&input.to_string()).unwrap()),
            output
        );
    }
}

fn drain(mut input: Value) {
    let mut observed = std::collections::BTreeSet::new();
    let expected: usize = DOMAINS
        .iter()
        .map(|name| input["queues"][name].as_array().unwrap().len())
        .sum();
    while observed.len() < expected {
        let output = call(&input);
        assert!(selected_count(&output) > 0);
        assert!(selected_count(&output) <= input["limits"]["total"].as_u64().unwrap() as usize);
        for name in DOMAINS {
            let selected = output["selected"][name].as_array().unwrap();
            assert!(selected.len() <= input["limits"]["perDomain"].as_u64().unwrap() as usize);
            for id in selected {
                assert!(observed.insert((name.to_owned(), id.as_str().unwrap().to_owned())));
            }
            input["queues"][name]
                .as_array_mut()
                .unwrap()
                .retain(|item| !selected.contains(&item["id"]));
        }
        input["nextDomain"] = output["nextDomain"].clone();
        input = serde_json::from_str(&input.to_string()).unwrap();
    }
    assert_eq!(selected_count(&call(&input)), 0);
}

#[test]
fn all_domain_mixes_drain_across_restarts_without_duplicates_or_starvation() {
    let fixture = fixtures();
    let present_count = fixture["mixes"]["countPerPresentDomain"].as_u64().unwrap() as usize;
    for mask in fixture["mixes"]["masks"].as_array().unwrap() {
        let mask = mask.as_u64().unwrap();
        let counts = std::array::from_fn(|i| {
            if mask & (1 << i) == 0 {
                0
            } else {
                present_count
            }
        });
        let mut input = request(counts, "sync");
        input["limits"] = json!({"perDomain":fixture["mixes"]["perDomain"],
            "total":fixture["mixes"]["total"]});
        drain(input);
    }
    let mut tiny = request([9; 5], "sync");
    tiny["limits"] = json!({"perDomain":1,"total":1});
    drain(tiny);
}

#[test]
fn rotating_cursor_gives_each_busy_domain_a_turn_even_with_one_slot() {
    let mut input = request([2; 5], "sync");
    input["limits"] = json!({"perDomain":1,"total":1});
    for name in DOMAINS {
        let output = call(&input);
        assert_eq!(output["selected"][name], json!(["op-00000"]));
        input["nextDomain"] = output["nextDomain"].clone();
        // Other domains remain busy, including commands. The persisted cursor
        // must prevent restarting at commands from starving later domains.
        input["queues"][name] = json!([descriptor(1, usize::from(name != "commands"))]);
    }
    assert_eq!(input["nextDomain"], "commands");
}

#[test]
fn shared_barriers_hold_children_and_later_commands_until_reconciliation() {
    for case in fixtures()["barriers"].as_array().unwrap() {
        let count = case["count"]
            .as_u64()
            .map(|n| n as usize)
            .unwrap_or_else(|| case["operations"].as_array().unwrap().len());
        let child = case["child"].as_u64().unwrap() as usize;
        let parent = case["parent"].as_u64().unwrap() as usize;
        let mut input = request([count, 1, 1, 1, 1], "sync");
        input["timerDependencies"] = json!([{"operationId":format!("op-{child:05}"),
            "dependsOnOperationId":format!("op-{parent:05}")}]);
        let output = call(&input);
        let expected = case.get("selected").cloned().unwrap_or_else(|| {
            (0..case["selectedCount"].as_u64().unwrap())
                .map(|i| json!(format!("op-{i:05}")))
                .collect()
        });
        assert_eq!(output["selected"]["commands"], expected);
        assert_eq!(output["heldTimerOperationId"], format!("op-{child:05}"));
        assert_eq!(
            output["selected"]["selectedTaskOperations"],
            json!(["op-00000"])
        );
        assert_eq!(call(&input), output, "restart must not release a barrier");
        for mode in ["merge", "replace_remote"] {
            input["mode"] = json!(mode);
            let atomic = call(&input);
            assert_eq!(
                atomic["status"],
                if count > 256 {
                    "oversized"
                } else {
                    "blocked_dependency"
                }
            );
            assert_eq!(selected_count(&atomic), 0);
        }
    }
}

#[test]
fn saved_claims_replay_exact_order_or_block_without_partial_ids() {
    for case in fixtures()["boundaries"].as_array().unwrap() {
        let counts = serde_json::from_value(case["counts"].clone()).unwrap();
        let mut input = request(counts, case["mode"].as_str().unwrap());
        let mut saved =
            json!({"kind":"saved", "mode":input["mode"], "limits":input["limits"], "queues":{}});
        for name in DOMAINS {
            saved["queues"][name] = ids(&input["queues"][name]);
        }
        let output = call(&saved);
        let fits = counts
            .iter()
            .all(|n| *n <= input["limits"]["perDomain"].as_u64().unwrap() as usize)
            && counts.iter().sum::<usize>() <= input["limits"]["total"].as_u64().unwrap() as usize;
        assert_eq!(
            output["status"],
            if fits {
                "replay_saved"
            } else {
                "oversized_saved"
            }
        );
        if fits {
            assert_eq!(output["selected"], saved["queues"]);
        } else {
            assert_eq!(selected_count(&output), 0);
        }
        assert!(output["nextDomain"].is_null());
        input = serde_json::from_str(&saved.to_string()).unwrap();
        assert_eq!(call(&input), output);
    }
}

#[test]
fn malformed_limits_identities_shapes_and_clocks_fail_closed() {
    let base = request([1; 5], "sync");
    let mutations = [
        ("/limits/total", json!(0)),
        ("/limits/total", json!(513)),
        ("/limits/perDomain", json!(257)),
        ("/limits/total", json!(1.5)),
        ("/limits/total", json!(true)),
        ("/limits/perDomain", json!(-1)),
        ("/queues/commands/0/deviceSequence", json!(0)),
        ("/queues/commands/0/hlcWallMs", json!(0)),
        ("/queues/taskOperations/0/id", json!("")),
        ("/queues/taskOperations/0/deviceId", json!("")),
        (
            "/queues/taskOperations/0/hlcCounter",
            json!(9_007_199_254_740_992_i64),
        ),
        ("/queues/autoStartOperations/0/hlcWallMs", json!(-1)),
        ("/nextDomain", json!("unknown")),
        ("/mode", json!("unknown")),
        ("/queues/commands", Value::Null),
        ("/timerDependencies", Value::Null),
    ];
    for (path, value) in mutations {
        let mut input = base.clone();
        *input.pointer_mut(path).unwrap() = value;
        assert!(
            dispatch_json("sync.batchPlan.v1", &input.to_string()).is_err(),
            "{path}"
        );
    }
    let duplicate = base
        .to_string()
        .replace("\"total\":512", "\"total\":512,\"total\":1");
    assert!(dispatch_json("sync.batchPlan.v1", &duplicate).is_err());
    for name in DOMAINS {
        let mut input = base.clone();
        input["queues"][name] = json!([input["queues"][name][0], input["queues"][name][0]]);
        assert!(dispatch_json("sync.batchPlan.v1", &input.to_string()).is_err());
        input["queues"].as_object_mut().unwrap().remove(name);
        assert!(dispatch_json("sync.batchPlan.v1", &input.to_string()).is_err());
    }
}

#[test]
fn dependencies_and_timer_sequence_cannot_reverse_replay_order() {
    for (parent, child) in [
        ("op-00000", "missing"),
        ("missing", "op-00001"),
        ("op-00000", "op-00000"),
        ("op-00001", "op-00000"),
    ] {
        let mut input = request([2, 0, 0, 0, 0], "sync");
        input["timerDependencies"] = json!([{"operationId":child,"dependsOnOperationId":parent}]);
        assert!(dispatch_json("sync.batchPlan.v1", &input.to_string()).is_err());
    }
    let mut input = request([2, 0, 0, 0, 0], "sync");
    input["timerDependencies"] = json!([
        {"operationId":"op-00001","dependsOnOperationId":"op-00000"},
        {"operationId":"op-00001","dependsOnOperationId":"op-00000"}]);
    assert!(dispatch_json("sync.batchPlan.v1", &input.to_string()).is_err());
    input["timerDependencies"] = json!([]);
    input["queues"]["commands"][0]["deviceSequence"] = json!(1);
    assert!(dispatch_json("sync.batchPlan.v1", &input.to_string()).is_err());
    input["queues"]["commands"][0]["deviceSequence"] = json!(2);
    input["queues"]["commands"][0]["hlcWallMs"] = json!(1);
    input["queues"]["commands"][1]["id"] = json!("zz-late-id");
    assert!(dispatch_json("sync.batchPlan.v1", &input.to_string()).is_err());
}

#[test]
fn shared_ordering_vector_is_permutation_invariant_in_every_domain() {
    let fixture = fixtures();
    let mut input = request([0; 5], "sync");
    for name in DOMAINS {
        let mut operations = fixture["ordering"]["commands"].clone();
        if name != "commands" {
            for operation in operations.as_array_mut().unwrap() {
                operation.as_object_mut().unwrap().remove("deviceSequence");
            }
        }
        input["queues"][name] = operations;
    }
    let expected = call(&input);
    for name in DOMAINS {
        assert_eq!(expected["selected"][name], fixture["ordering"]["selected"]);
    }
    for shift in 0..5 {
        for name in DOMAINS {
            let queue = input["queues"][name].as_array_mut().unwrap();
            queue.rotate_left(shift);
            queue.reverse();
        }
        assert_eq!(call(&input), expected);
    }
}

#[test]
fn strict_schema_and_bootstrap_bounds_reject_ambiguous_requests() {
    let mut inputs = Vec::new();
    let base = request([1; 5], "sync");
    for (path, field) in [
        ("", "extension"),
        ("/queues", "typo"),
        ("/queues/commands/0", "dependsOnCommandId"),
        ("/limits", "other"),
    ] {
        let mut input = base.clone();
        input
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(field.into(), json!(1));
        inputs.push(input);
    }
    for limits in [
        json!({"perDomain":4097,"total":8192}),
        json!({"perDomain":4096,"total":8193}),
    ] {
        let mut input = request([0; 5], "merge");
        input["limits"] = limits;
        inputs.push(input);
    }
    inputs.push(request([1; 5], "keep_remote"));
    inputs.push(request([0, 10001, 0, 0, 0], "sync"));
    for input in inputs {
        assert!(dispatch_json("sync.batchPlan.v1", &input.to_string()).is_err());
    }
    assert_eq!(selected_count(&call(&request([0; 5], "keep_remote"))), 0);
    let output = call(&request([0, 10000, 0, 0, 0], "sync"));
    assert_eq!(
        output["selected"]["taskOperations"]
            .as_array()
            .unwrap()
            .len(),
        256
    );
}

#[test]
fn saved_schema_rejects_replanning_fields_and_corrupt_claim_ids() {
    let base = json!({"kind":"saved","mode":"sync","limits":{"perDomain":256,"total":512},
        "queues":{"commands":[],"taskOperations":[],"durationOperations":[],
            "autoStartOperations":[],"selectedTaskOperations":[]}});
    for extra in ["nextDomain", "timerDependencies", "neverSent"] {
        let mut input = base.clone();
        input[extra] = json!([]);
        assert!(dispatch_json("sync.batchPlan.v1", &input.to_string()).is_err());
    }
    for name in DOMAINS {
        for bad in [
            json!([""]),
            json!(["id", "id"]),
            json!([{"id":"id"}]),
            Value::Null,
        ] {
            let mut input = base.clone();
            input["queues"][name] = bad;
            assert!(dispatch_json("sync.batchPlan.v1", &input.to_string()).is_err());
        }
    }
    assert_eq!(call(&base)["status"], "replay_saved");
}
