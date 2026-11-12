//! `task.carryHistory.v1` over full logs: the server and peers that reduce a
//! whole operation log.
use super::*;

#[test]
fn rename_moves_finished_rows_and_the_terminal_timer_to_the_new_identity() {
    let mut commands = finished("timer-1", "Café", 0, 25).to_vec();
    commands.extend(finished("timer-2", "Café", 30, 55));
    let reduced = reduce(commands);
    let carried = carry(
        &reduced,
        vec![rename("r1", "device-a", "Café", "Espresso", 60)],
    )
    .unwrap();
    let tasks = tasks_by_timer(&carried["history"]);
    assert_eq!(tasks["timer-1"], id("Espresso"));
    assert_eq!(tasks["timer-2"], id("Espresso"));
    assert_eq!(carried["canonicalTimer"]["id"], "timer-2");
    assert_eq!(carried["canonicalTimer"]["taskId"], id("Espresso"));
    // Only `taskId` changes; every other field and the row order stay.
    let mut expected = reduced["history"].clone();
    for row in expected.as_array_mut().unwrap() {
        row["taskId"] = json!(id("Espresso"));
    }
    assert_eq!(carried["history"], expected);
    assert_eq!(
        carried.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["canonicalTimer", "history"]
    );
}

#[test]
fn offline_peer_row_that_started_before_the_rename_moves() {
    // Device B starts on Café at 0 and is offline. Device A renames Café at
    // 10 without seeing B's timer, so no retarget exists. B finishes at 25.
    let reduced = reduce(finished("timer-b", "Café", 0, 25).to_vec());
    let carried = carry(
        &reduced,
        vec![rename("r1", "device-a", "Café", "Espresso", 10)],
    )
    .unwrap();
    assert_eq!(
        tasks_by_timer(&carried["history"])["timer-b"],
        id("Espresso")
    );
    assert_eq!(carried["canonicalTimer"]["taskId"], id("Espresso"));
}

#[test]
fn bound_is_the_session_start_inclusive() {
    let mut commands = finished("starts-at-rename", "Café", 10, 35).to_vec();
    commands.extend(finished("starts-after", "Café", 40, 65));
    let reduced = reduce(commands);
    let carried = carry(
        &reduced,
        vec![rename("r1", "device-a", "Café", "Espresso", 10)],
    )
    .unwrap();
    let tasks = tasks_by_timer(&carried["history"]);
    assert_eq!(tasks["starts-at-rename"], id("Espresso"));
    assert_eq!(tasks["starts-after"], id("Café"));
    assert_eq!(carried["canonicalTimer"]["id"], "starts-after");
    assert_eq!(carried["canonicalTimer"]["taskId"], id("Café"));
}

#[test]
fn cancelled_and_superseded_rows_use_their_session_start_too() {
    // Each row starts at 5, before the rename at 10, and ends after it.
    let cancelled = vec![
        assigned(command("device-c", 1, "cancelled", "start", 5), "Café"),
        command("device-c", 2, "cancelled", "cancel", 15),
    ];
    let superseded = vec![
        assigned(command("device-c", 1, "superseded", "start", 5), "Café"),
        assigned(command("device-d", 1, "successor", "start", 12), "Other"),
    ];
    for (commands, timer, status) in [
        (cancelled, "cancelled", "cancelled"),
        (superseded, "superseded", "superseded"),
    ] {
        let reduced = reduce(commands);
        let row = reduced["history"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["timerId"] == timer)
            .unwrap_or_else(|| panic!("no {timer} row: {reduced}"));
        assert_eq!(row["status"], status, "{reduced}");
        let log = vec![rename("r1", "device-a", "Café", "Espresso", 10)];
        let carried = carry(&reduced, log).unwrap();
        assert_eq!(tasks_by_timer(&carried["history"])[timer], id("Espresso"));
    }
}

#[test]
fn row_without_a_session_falls_back_to_its_end_time() {
    let reduced = reduce(finished("timer-b", "Café", 0, 25).to_vec());
    let mut stripped = reduced.clone();
    stripped["sessions"] = json!([]);
    let before_end = carry(
        &stripped,
        vec![rename("r1", "device-a", "Café", "Espresso", 10)],
    );
    assert_eq!(
        tasks_by_timer(&before_end.unwrap()["history"])["timer-b"],
        id("Café")
    );
    let after_end = carry(
        &stripped,
        vec![rename("r1", "device-a", "Café", "Espresso", 25)],
    );
    assert_eq!(
        tasks_by_timer(&after_end.unwrap()["history"])["timer-b"],
        id("Espresso")
    );
}

#[test]
fn chains_cycles_and_recreated_titles_follow_reducer_order() {
    let mut commands = finished("old-a", "A", 0, 5).to_vec();
    commands.extend(finished("old-x", "X", 6, 8));
    commands.extend(finished("recreated-a", "A", 30, 35));
    let reduced = reduce(commands);
    let chain = vec![
        rename("r1", "device-a", "A", "B", 10),
        rename("r2", "device-a", "B", "C", 20),
    ];
    let tasks = tasks_by_timer(&carry(&reduced, chain).unwrap()["history"]);
    assert_eq!(tasks["old-a"], id("C"));
    assert_eq!(tasks["recreated-a"], id("A"));
    let cycle = vec![
        rename("r1", "device-a", "A", "B", 10),
        rename("r2", "device-a", "B", "A", 20),
    ];
    let tasks = tasks_by_timer(&carry(&reduced, cycle).unwrap()["history"]);
    assert_eq!(tasks["old-a"], id("A"));
    // Y -> Z first, then X -> Y: X rows stop at Y and never reach Z.
    let reverse = vec![
        rename("r1", "device-a", "Y", "Z", 10),
        rename("r2", "device-a", "X", "Y", 20),
    ];
    let tasks = tasks_by_timer(&carry(&reduced, reverse).unwrap()["history"]);
    assert_eq!(tasks["old-x"], id("Y"));
}

/// Two devices rename A without seeing each other: one to B at minute 10, one
/// to C at minute 20. The task reducer keeps the later rename as the winner
/// for A. Each planner upserts its own title, so B and C both exist. History
/// follows reducer order instead: the first rename moves every row of A that
/// started at or before it, so those rows land on B. Only rows that started
/// between the two renames move to C. Core cannot tell this case from a
/// sequential rename, so the carry never looks at the reducer winner.
#[test]
fn concurrent_renames_of_one_task_carry_to_the_first_rename_in_reducer_order() {
    let mut commands = finished("before-both", "A", 0, 5).to_vec();
    commands.extend(finished("between", "A", 12, 15));
    commands.extend(finished("after-both", "A", 30, 35));
    let log = vec![
        upsert("u-a", "A", 0),
        upsert("u-b", "B", 10),
        rename("r-b", "device-a", "A", "B", 10),
        upsert("u-c", "C", 20),
        rename("r-c", "device-c", "A", "C", 20),
    ];
    let reduced_tasks = call("task.reduce.v1", &json!({"operations": log})).unwrap();
    assert_eq!(reduced_tasks["winningOperationIds"][id("A")], "r-c");
    let mut titles: Vec<_> = reduced_tasks["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_owned())
        .collect();
    titles.sort();
    assert_eq!(titles, ["B", "C"]);
    let tasks = tasks_by_timer(&carry(&reduce(commands), log).unwrap()["history"]);
    assert_eq!(
        tasks["before-both"],
        id("B"),
        "first rename in reducer order"
    );
    assert_eq!(tasks["between"], id("C"));
    assert_eq!(tasks["after-both"], id("A"));
}

#[test]
fn merge_and_edge_space_renames_move_rows_once() {
    let mut commands = finished("cafe", "Café", 0, 5).to_vec();
    commands.extend(finished("espresso", "Espresso", 6, 8));
    commands.extend(finished("write", "Write", 9, 12));
    let reduced = reduce(commands);
    let log = vec![
        rename("r1", "device-a", "Café", "Espresso", 20),
        rename("r2", "device-a", "Write", "Write ", 20),
    ];
    let carried = carry(&reduced, log).unwrap();
    let tasks = tasks_by_timer(&carried["history"]);
    assert_eq!(tasks["cafe"], id("Espresso"));
    assert_eq!(tasks["espresso"], id("Espresso"));
    assert_ne!(id("Write"), id("Write "));
    assert_eq!(tasks["write"], id("Write "));
    assert_eq!(carried["history"].as_array().unwrap().len(), 3);
}

#[test]
fn running_and_paused_timers_keep_their_task_until_they_end() {
    for tail in [None, Some("pause")] {
        let mut commands = vec![assigned(command("device-b", 1, "live", "start", 0), "Café")];
        if let Some(kind) = tail {
            commands.push(command("device-b", 2, "live", kind, 5));
        }
        let reduced = call(
            "timer.reduce.v1",
            &json!({"commands": commands, "now": at(8)}),
        )
        .unwrap();
        let log = vec![rename("r1", "device-a", "Café", "Espresso", 6)];
        let carried = carry(&reduced, log).unwrap();
        assert_eq!(carried["canonicalTimer"], reduced["canonicalTimer"]);
        assert_eq!(carried["canonicalTimer"]["taskId"], id("Café"));
    }
}

#[test]
fn full_reduction_does_not_depend_on_arrival_or_input_order() {
    let mut commands = finished("before", "A", 0, 5).to_vec();
    commands.extend(finished("after", "A", 30, 35));
    let log = vec![
        upsert("u1", "B", 10),
        rename("r1", "device-a", "A", "B", 10),
        rename("r2", "device-c", "B", "C", 20),
    ];
    let expected =
        tasks_by_timer(&carry(&reduce(commands.clone()), log.clone()).unwrap()["history"]);
    assert_eq!(expected["before"], id("C"));
    assert_eq!(expected["after"], id("A"));
    for rotation in 0..log.len() {
        let mut shuffled_log = log.clone();
        shuffled_log.rotate_left(rotation);
        let mut shuffled_commands = commands.clone();
        shuffled_commands.reverse();
        let reduced = reduce(shuffled_commands);
        let mut history_reversed = reduced.clone();
        history_reversed["history"]
            .as_array_mut()
            .unwrap()
            .reverse();
        history_reversed["sessions"]
            .as_array_mut()
            .unwrap()
            .reverse();
        for input in [reduced, history_reversed] {
            let carried = carry(&input, shuffled_log.clone()).unwrap();
            assert_eq!(tasks_by_timer(&carried["history"]), expected, "{rotation}");
        }
    }
}

#[test]
fn partial_logs_converge_once_the_full_log_arrives() {
    let commands = finished("peer", "A", 0, 25).to_vec();
    let log = vec![rename("r1", "device-a", "A", "B", 10)];
    // Rename first: no history yet, then the peer's commands arrive.
    let empty = carry(&reduce(Vec::new()), log.clone()).unwrap();
    assert_eq!(empty["history"], json!([]));
    let rename_first = carry(&reduce(commands.clone()), log.clone()).unwrap();
    // Commands first: the row stays on A, then the rename arrives.
    let commands_first = carry(&reduce(commands.clone()), Vec::new()).unwrap();
    assert_eq!(tasks_by_timer(&commands_first["history"])["peer"], id("A"));
    let full = carry(&reduce(commands), log).unwrap();
    assert_eq!(full, rename_first);
    assert_eq!(tasks_by_timer(&full["history"])["peer"], id("B"));
}
