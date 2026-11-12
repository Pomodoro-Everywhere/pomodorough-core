//! The base/replay boundary: a client projects the server's carried base plus
//! its never-sent local renames. That must equal one full-order pass over the
//! raw log, because base edges apply exactly once (on the server) and every
//! local edge sorts after them.
use super::*;

/// Raw rows. Each title pair crosses the boundary in a different way.
pub(super) fn raw_commands() -> Vec<Value> {
    let mut commands = finished("chain", "A", 0, 5).to_vec();
    commands.extend(finished("reverse", "X", 1, 4));
    commands.extend(finished("cycle", "P", 2, 6));
    commands.extend(finished("recreated", "A", 30, 35));
    // Device C starts on A before the base rename at 10, is offline, and
    // finishes at 33. Its commands reach the server after the rename.
    commands.push(assigned(command("device-c", 1, "peer", "start", 8), "A"));
    commands.push(command("device-c", 2, "peer", "finish", 33));
    commands
}

/// Acknowledged renames: the server already carried them into the base.
pub(super) fn base_log() -> Vec<Value> {
    vec![
        rename("b1", "device-a", "A", "B", 10),
        rename("b2", "device-a", "Y", "Z", 10),
        rename("b3", "device-a", "P", "Q", 10),
    ]
}

/// Never-sent renames above the canonical head.
pub(super) fn local_log() -> Vec<Value> {
    vec![
        rename("l1", "device-a", "B", "C", 70),
        rename("l2", "device-a", "X", "Y", 70),
        rename("l3", "device-a", "Q", "P", 70),
        rename("l4", "device-a", "A", "D", 70),
    ]
}

fn project(server: &Value, local: Vec<Value>) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/workspace-projection-v1.json")).unwrap();
    let mut request = fixture["request"].clone();
    request["base"]["tasks"] = json!([]);
    request["base"]["selectedTaskId"] = Value::Null;
    request["base"]["history"] = server["history"].clone();
    request["base"]["canonicalTimer"] = server["canonicalTimer"].clone();
    request["canonicalHead"] = json!({"wallMs": WALL + 60 * 60_000, "counter": 0});
    let ids: Vec<_> = local.iter().map(|op| op["id"].clone()).collect();
    request["local"] = json!({
        "commands": [], "taskOperations": local, "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []
    });
    request["neverSent"] = json!({"taskOperations": ids});
    request["timerDependencies"] = json!([]);
    request["now"] = json!(at(120));
    call("workspace.project.v1", &request).unwrap()["workspace"].clone()
}

#[test]
fn projection_over_a_carried_base_equals_one_full_pass() {
    let raw = reduce(raw_commands());
    let server = carry(&raw, base_log()).unwrap();
    let full = carry(&raw, [base_log(), local_log()].concat()).unwrap();
    let expected = tasks_by_timer(&full["history"]);
    assert_eq!(expected["chain"], id("C"), "chain across the boundary");
    assert_eq!(expected["reverse"], id("Y"), "Y -> Z in base, X -> Y local");
    assert_eq!(expected["cycle"], id("P"), "cycle across the boundary");
    assert_eq!(
        expected["peer"],
        id("C"),
        "offline peer spans the base rename"
    );
    assert_eq!(
        expected["recreated"],
        id("D"),
        "recreated A after the base rename"
    );
    for rotation in 0..local_log().len() {
        let mut local = local_log();
        local.rotate_left(rotation);
        let projected = project(&server, local);
        assert_eq!(
            tasks_by_timer(&projected["history"]),
            expected,
            "{rotation}"
        );
        assert_eq!(
            projected["canonicalTimer"], full["canonicalTimer"],
            "{rotation}"
        );
    }
}

/// Every ordering of `items`, by Heap's algorithm.
fn permutations(items: Vec<Value>) -> Vec<Vec<Value>> {
    fn visit(k: usize, items: &mut Vec<Value>, out: &mut Vec<Vec<Value>>) {
        if k <= 1 {
            out.push(items.clone());
            return;
        }
        for index in 0..k {
            visit(k - 1, items, out);
            let swap = if k % 2 == 0 { index } else { 0 };
            items.swap(swap, k - 1);
        }
    }
    let mut out = Vec::new();
    let mut items = items;
    visit(items.len(), &mut items, &mut out);
    out
}

#[test]
fn full_pass_does_not_depend_on_arrival_order() {
    let log = [base_log(), local_log()].concat();
    let expected = carry(&reduce(raw_commands()), log.clone()).unwrap();
    let orders = permutations(log);
    assert_eq!(orders.len(), 5040);
    // Each command rotation reaches the reducer in a different order.
    let reductions: Vec<_> = (0..raw_commands().len())
        .map(|rotation| {
            let mut commands = raw_commands();
            commands.rotate_left(rotation);
            reduce(commands)
        })
        .collect();
    for (index, order) in orders.into_iter().enumerate() {
        let reduced = &reductions[index % reductions.len()];
        assert_eq!(carry(reduced, order).unwrap(), expected, "order {index}");
    }
}
