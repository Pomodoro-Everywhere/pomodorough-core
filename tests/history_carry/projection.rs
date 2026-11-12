//! Local projection: a renaming device sees its history move before the
//! server acknowledges the rename.
use super::*;

fn row(timer: &str, title: &str, status: &str, end: i64, elapsed: i64) -> Value {
    let end_field = if status == "completed" {
        "completedAt"
    } else {
        "endedAt"
    };
    json!({
        "id": timer, "timerId": timer, "taskId": id(title), "phase": "focus",
        "status": status, "plannedDurationMs": PLANNED_MS, end_field: at(end),
        "elapsedMs": elapsed
    })
}

/// A read-model request whose local task queue holds never-sent operations
/// above the canonical head, so Core replays it.
fn read_model(tasks: &[&str], history: Value, local: Vec<Value>) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/read-model-v1.json")).unwrap();
    let mut request = fixture["request"].clone();
    let workspace = &mut request["source"]["value"];
    workspace["base"]["tasks"] = tasks
        .iter()
        .map(|title| json!({"id": id(title), "title": title}))
        .collect();
    workspace["base"]["history"] = history;
    workspace["canonicalHead"] = json!({"wallMs": WALL, "counter": 0});
    let ids: Vec<_> = local.iter().map(|op| op["id"].clone()).collect();
    workspace["local"]["taskOperations"] = json!(local);
    workspace["neverSent"] = json!({"taskOperations": ids});
    call("workspace.readModel.v1", &request).unwrap()
}

fn today(model: &Value, title: &str) -> (i64, i64) {
    let row = &model["tasks"]["completedFocusTodayByTask"][id(title)];
    (
        row["count"].as_i64().unwrap(),
        row["elapsedMs"].as_i64().unwrap(),
    )
}

#[test]
fn rename_keeps_todays_runs_and_minutes_in_the_read_model() {
    let history = json!([
        row("t1", "Café", "completed", 25, 1_500_000),
        row("t2", "Café", "completed", 50, 600_000),
    ]);
    let local = vec![
        upsert("u1", "Espresso", 60),
        rename("r1", "device-a", "Café", "Espresso", 60),
    ];
    let model = read_model(&["Café"], history, local);
    assert_eq!(today(&model, "Espresso"), (2, 2_100_000));
    assert!(
        model["tasks"]["completedFocusTodayByTask"]
            .get(id("Café"))
            .is_none(),
        "{model}"
    );
    assert_eq!(model["cadence"]["completedFocusToday"], 2);
}

#[test]
fn rename_onto_an_existing_title_sums_both_tasks_once() {
    let history = json!([
        row("t1", "Café", "completed", 25, 1_500_000),
        row("t2", "Espresso", "completed", 50, 600_000),
        row("t3", "Espresso", "cancelled", 55, 60_000),
    ]);
    let local = vec![rename("r1", "device-a", "Café", "Espresso", 60)];
    let model = read_model(&["Café", "Espresso"], history, local);
    assert_eq!(today(&model, "Espresso"), (2, 2_100_000));
    assert_eq!(model["tasks"]["total"], 1);
}

#[test]
fn edge_space_rename_moves_the_history() {
    let history = json!([row("t1", "Write", "completed", 25, 1_500_000)]);
    let local = vec![
        upsert("u1", "Write ", 60),
        rename("r1", "device-a", "Write", "Write ", 60),
    ];
    let model = read_model(&["Write"], history, local);
    assert_eq!(today(&model, "Write "), (1, 1_500_000));
}

fn intent_request(timer_status: &str) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/workspace-intent-v1.json")).unwrap();
    let mut input = fixture["request"].clone();
    input["compatibility"] = json!("appleWorkspace");
    input["intent"] = json!({"kind": "renameTask", "taskId": id("Café"), "title": "Espresso"});
    input["ownership"] = json!({"expectedOwnerId": "account-a", "ownerId": "account-a"});
    input["durability"] = json!({"outgoingDurationOperationIds": [], "localTabId": "tab-a"});
    input["identities"]["commandUuids"] = json!([
        "019f7f65-dd10-7000-8000-000000000001",
        "019f7f65-dd10-7000-8000-000000000002",
        "019f7f65-dd10-7000-8000-000000000003",
        "019f7f65-dd10-7000-8000-000000000004"
    ]);
    let base = &mut input["workspace"]["base"];
    base["tasks"] = json!([{"id": id("Café"), "title": "Café"}]);
    base["selectedTaskId"] = json!(id("Café"));
    base["canonicalTimer"] = fixture["timer"].clone();
    base["canonicalTimer"]["taskId"] = json!(id("Café"));
    base["canonicalTimer"]["status"] = json!(timer_status);
    let finished_at = fixture["timer"]["anchorAt"]
        .as_str()
        .unwrap()
        .replace("12:00", "11:00");
    base["history"] = json!([{
        "id": "earlier", "timerId": "earlier", "taskId": id("Café"), "phase": "focus",
        "status": "completed", "plannedDurationMs": PLANNED_MS,
        "completedAt": finished_at, "elapsedMs": PLANNED_MS
    }]);
    input
}

#[test]
fn rename_during_running_or_paused_focus_retargets_once_and_moves_old_rows() {
    for status in ["running", "paused"] {
        let output = call("workspace.intent.v1", &intent_request(status)).unwrap();
        assert_eq!(output["outcome"], "planned", "{status}: {output}");
        let commands = output["operations"]["commands"].as_array().unwrap();
        assert_eq!(commands.len(), 1, "{status}");
        assert_eq!(commands[0]["type"], "retarget", "{status}");
        let projection = &output["projection"];
        assert_eq!(projection["canonicalTimer"]["taskId"], id("Espresso"));
        assert_eq!(projection["canonicalTimer"]["status"], status);
        let tasks = tasks_by_timer(&projection["history"]);
        assert_eq!(tasks["earlier"], id("Espresso"), "{status}: {projection}");
        assert_eq!(tasks.len(), 1, "{status}: the live session has no row yet");
    }
}

#[test]
fn local_replay_does_not_depend_on_queue_order() {
    let history = json!([
        row("t1", "A", "completed", 5, 1_500_000),
        row("t2", "A", "completed", 40, 600_000),
    ]);
    // A -> B at 10, B -> C at 20, then a recreated A row ends at 40 and a
    // second A -> D at 45 moves only that row.
    let local = vec![
        upsert("u1", "B", 10),
        rename("r1", "device-a", "A", "B", 10),
        upsert("u2", "C", 20),
        rename("r2", "device-c", "B", "C", 20),
        upsert("u3", "D", 45),
        rename("r3", "device-a", "A", "D", 45),
    ];
    for rotation in 0..local.len() {
        let mut queue = local.clone();
        queue.rotate_left(rotation);
        queue.reverse();
        let model = read_model(&["A"], history.clone(), queue);
        assert_eq!(today(&model, "C"), (1, 1_500_000), "{rotation}");
        assert_eq!(today(&model, "D"), (1, 600_000), "{rotation}");
    }
}
