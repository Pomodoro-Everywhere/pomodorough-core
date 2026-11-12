//! Reconciliation rebase over a server-carried base plus never-sent local
//! renames. Every rebase version must give the same history as one full-order
//! pass over the raw log, as the workspace projection does.
use super::boundary::{base_log, local_log, raw_commands};
use super::*;

const REBASE_OPERATIONS: [&str; 3] = [
    "reconcile.rebase.v1",
    "reconcile.rebase.v2",
    "reconcile.rebase.v3",
];

fn queues(task_operations: Vec<Value>) -> Value {
    json!({
        "commands": [], "taskOperations": task_operations, "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []
    })
}

/// The recorded canonical response with the carried history and a head at
/// minute 60, below every local rename. The terminal timer is left out,
/// because rebase v1 and v2 reject any timer that overlaps its history row.
fn response(server: &Value) -> Value {
    let recorded: Value = serde_json::from_str(include_str!(
        "../../fixtures/reconciliation-terminal-v3.json"
    ))
    .unwrap();
    let mut response: Value =
        serde_json::from_str(recorded["http"]["responseRaw"].as_str().unwrap()).unwrap();
    response["acknowledgements"] = json!([]);
    response["canonicalTimer"] = Value::Null;
    response["history"] = server["history"].clone();
    response["serverTime"] = json!(at(60));
    response["serverHlcWallMs"] = json!(WALL + 60 * 60_000);
    response["serverHlcCounter"] = json!(0);
    response
}

fn rebase(operation: &str, server: &Value, local: Vec<Value>) -> Value {
    let ids: Vec<_> = local.iter().map(|op| op["id"].clone()).collect();
    let input = json!({
        "local": queues(local), "sent": queues(vec![]), "response": response(server),
        "neverSent": {"taskOperations": ids}, "timerDependencies": []
    });
    call(operation, &input).unwrap_or_else(|error| panic!("{operation}: {error}"))
}

#[test]
fn every_rebase_over_a_carried_base_equals_one_full_pass() {
    let raw = reduce(raw_commands());
    let server = carry(&raw, base_log()).unwrap();
    let full = carry(&raw, [base_log(), local_log()].concat()).unwrap();
    let expected = tasks_by_timer(&full["history"]);
    assert_eq!(expected["chain"], id("C"), "chain across the boundary");
    assert_eq!(expected["reverse"], id("Y"), "Y -> Z in base, X -> Y local");
    assert_eq!(expected["cycle"], id("P"), "cycle across the boundary");
    for operation in REBASE_OPERATIONS {
        for rotation in 0..local_log().len() {
            let mut local = local_log();
            local.rotate_left(rotation);
            let rebased = rebase(operation, &server, local);
            let context = format!("{operation} rotation {rotation}");
            assert_eq!(tasks_by_timer(&rebased["history"]), expected, "{context}");
            assert_eq!(rebased["timer"], Value::Null, "{context}");
            // The base stays the server's carried base; only the projection moves.
            assert_eq!(rebased["baseHistory"], server["history"], "{context}");
        }
    }
}
