//! A carried base reused as projection input. Carrying is not idempotent, so
//! a rename edge that the base already contains must not be applied again,
//! and Core can only keep the order of edges that it sees.
use super::*;

/// Workspace parts shared by both projection operations.
fn base(server: &Value) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/workspace-projection-v1.json")).unwrap();
    let mut base = fixture["request"]["base"].clone();
    base["tasks"] = json!([]);
    base["selectedTaskId"] = Value::Null;
    base["history"] = server["history"].clone();
    base["canonicalTimer"] = server["canonicalTimer"].clone();
    base
}

fn queues(task_operations: Vec<Value>) -> Value {
    json!({
        "commands": [], "taskOperations": task_operations, "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []
    })
}

/// `projection.apply.v2` the way clients call it: the terminal timer is
/// cleared because its row is in history.
fn apply_v2(server: &Value, pending: Vec<Value>) -> Value {
    let mut base = base(server);
    base["canonicalTimer"] = Value::Null;
    let input = json!({"base": base, "pending": queues(pending), "now": at(120)});
    call("projection.apply.v2", &input).unwrap()
}

fn workspace_project(server: &Value, local: Vec<Value>, proof: bool, head_minutes: i64) -> Value {
    let ids: Vec<_> = local.iter().map(|op| op["id"].clone()).collect();
    let never_sent = if proof {
        json!({"taskOperations": ids})
    } else {
        json!({})
    };
    let request = json!({
        "base": base(server), "local": queues(local), "neverSent": never_sent,
        "canonicalHead": {"wallMs": WALL + head_minutes * 60_000, "counter": 0},
        "timerDependencies": [], "now": at(120)
    });
    call("workspace.project.v1", &request).unwrap()["workspace"].clone()
}

/// Lost acknowledgement: this device sent both renames, the server stored
/// and carried them, and the response never arrived. Both stay queued.
#[test]
fn a_rename_whose_ack_was_lost_is_not_carried_twice() {
    let raw = reduce(finished("old-a", "A", 0, 5).to_vec());
    let sent = vec![
        rename("r1", "device-a", "B", "C", 10),
        rename("r2", "device-a", "A", "B", 20),
    ];
    let server = carry(&raw, sent.clone()).unwrap();
    assert_eq!(tasks_by_timer(&server["history"])["old-a"], id("B"));

    // A second pass is not harmless: the reverse chain moves the row on to C.
    let replayed_all = apply_v2(&server, sent.clone());
    assert_eq!(tasks_by_timer(&replayed_all["history"])["old-a"], id("C"));

    // The delivery gate keeps possibly delivered work out of the projection.
    // Sending retired the never-sent proof, and the server head covers both.
    for (proof, head) in [(false, 0), (false, 60), (true, 60)] {
        let projected = workspace_project(&server, sent.clone(), proof, head);
        assert_eq!(
            tasks_by_timer(&projected["history"])["old-a"],
            id("B"),
            "proof {proof}, head {head}"
        );
        assert_eq!(projected["canonicalTimer"]["taskId"], json!(id("B")));
    }
}

/// Iroh reuse: the room output carries the captured records. A local rename
/// that is not captured yet is applied after them, whatever its clock.
#[test]
fn reused_room_output_matches_the_full_order_once_the_rename_is_captured() {
    let raw = reduce(finished("old-a", "A", 0, 5).to_vec());
    let captured = rename("p1", "device-b", "B", "C", 30);
    let uncaptured = rename("l1", "device-a", "A", "B", 20);
    let room = carry(&raw, vec![captured.clone()]).unwrap();
    assert_eq!(tasks_by_timer(&room["history"])["old-a"], id("A"));

    // The uncaptured edge is not in the room output, so it applies once.
    let local = apply_v2(&room, vec![uncaptured.clone()]);
    assert_eq!(tasks_by_timer(&local["history"])["old-a"], id("B"));

    // In full order the uncaptured edge sorts before the captured one, so the
    // row belongs on C. The reused base shows B until capture.
    let captured_log = vec![captured, uncaptured];
    let after_capture = carry(&raw, captured_log).unwrap();
    assert_eq!(tasks_by_timer(&after_capture["history"])["old-a"], id("C"));
}
