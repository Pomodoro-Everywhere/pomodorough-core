use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const WALL: i64 = 1_784_548_810_000;
const NOW: &str = "2026-07-20T12:00:10Z";

fn history_rows(total: usize) -> Value {
    let rows: Vec<_> = (0..total)
        .map(|i| {
            json!({
                "id": format!("history-{i:05}"),
                "timerId": format!("timer-{i:05}"),
                "phase": "focus",
                "status": "completed",
                "plannedDurationMs": 60000,
                "completedAt": "2026-07-20T11:00:00Z"
            })
        })
        .collect();
    Value::Array(rows)
}

fn base_with_history(history: Value) -> Value {
    json!({
        "canonicalTimer": null,
        "history": history,
        "tasks": [],
        "durationsMs": {"focus": 60000, "short_break": 120000, "long_break": 180000},
        "autoStartBreaks": false,
        "selectedTaskId": null
    })
}

fn project_request(history: Value) -> Value {
    json!({
        "base": base_with_history(history),
        "local": {"commands": [], "taskOperations": [], "durationOperations": [],
            "autoStartOperations": [], "selectedTaskOperations": []},
        "canonicalHead": {"wallMs": WALL, "counter": 0},
        "neverSent": {}, "timerDependencies": [], "now": NOW
    })
}

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn project_history_len(total: usize) -> Value {
    let output = call(
        "workspace.project.v1",
        &project_request(history_rows(total)),
    );
    let history = output["workspace"]["history"].as_array().unwrap();
    assert_eq!(history.len(), total, "project keeps {total} rows");
    assert!(output["workspace"]["canonicalTimer"].is_null());
    output
}

fn workspace_without_now(history: Value) -> Value {
    let mut request = project_request(history);
    request.as_object_mut().unwrap().remove("now");
    request
}

fn read_history_len(total: usize) {
    let input = json!({
        "profile": "pwaStorage",
        "source": {"kind": "workspace", "value": workspace_without_now(history_rows(total))},
        "selectedPhase": "focus",
        "observedAt": NOW,
        "calendarIntervals": [{"start": "2026-07-20T00:00:00Z", "end": "2026-07-21T00:00:00Z"}]
    });
    let output = call("workspace.readModel.v1", &input);
    assert_eq!(
        output["canonical"]["status"], "idle",
        "null timer stays idle"
    );
    assert_eq!(
        output["cadence"]["completedFocusTotal"].as_u64().unwrap(),
        total as u64,
        "read keeps {total} rows in total count"
    );
    assert_eq!(
        output["cadence"]["completedFocusToday"].as_u64().unwrap(),
        total as u64,
        "read keeps {total} rows in today count"
    );
}

fn rebase_history_len(total: usize) {
    let base = base_with_history(history_rows(total));
    let mut response = base.clone();
    response.as_object_mut().unwrap().extend(
        json!({"revision": 1, "serverTime": NOW,
            "serverHlcWallMs": WALL, "serverHlcCounter": 0,
            "acknowledgements": [], "taskAcknowledgements": [],
            "durationAcknowledgements": [], "autoStartAcknowledgements": [],
            "selectedTaskAcknowledgements": []})
        .as_object()
        .unwrap()
        .clone(),
    );
    let input = json!({"local": project_request(history_rows(0))["local"],
        "sent": project_request(history_rows(0))["local"],
        "response": response, "neverSent": {}, "timerDependencies": []});
    let output = call("reconcile.rebase.v3", &input);
    assert_eq!(output["history"].as_array().unwrap().len(), total);
}

fn bootstrap_history_len(total: usize) {
    let project = project_request(history_rows(total));
    let input = json!({
        "profile": "pwaStorage",
        "currentUserId": "account-a",
        "local": {"ownerId": "account-a", "workspace": project,
            "preferences": {"durationsMs": {"focus": 60000, "short_break": 120000, "long_break": 180000},
                "autoStartBreaks": false, "selectedTaskId": null},
            "knownTasks": [], "projectionPending": null},
        "remote": base_with_history(history_rows(0))
    });
    let output = call("bootstrap.workspacePlan.v1", &input);
    assert!(output.get("plan").is_some());
    assert_eq!(
        output["classification"]["local"]["completedHistoryCount"]
            .as_u64()
            .unwrap(),
        total as u64,
        "bootstrap keeps {total} local rows"
    );
    assert_eq!(
        output["classification"]["local"]["displayHistoryCount"]
            .as_u64()
            .unwrap(),
        total as u64,
        "bootstrap keeps {total} local display rows"
    );
    assert_eq!(
        output["classification"]["remote"]["completedHistoryCount"]
            .as_u64()
            .unwrap(),
        0,
        "remote stays empty"
    );
}

#[test]
fn full_history_counts_project_end_to_end() {
    for total in [9999, 10_000, 10_001] {
        project_history_len(total);
        read_history_len(total);
        rebase_history_len(total);
        bootstrap_history_len(total);
    }
}

fn live_base() -> Value {
    let mut base = base_with_history(history_rows(10_000));
    base["canonicalTimer"] = json!({
        "id": "live-timer", "phase": "focus", "status": "running",
        "plannedDurationMs": 60000, "elapsedAtAnchorMs": 5000,
        "anchorAt": "2026-07-20T12:00:00Z", "startedByDeviceId": "device-local"
    });
    base
}

#[test]
fn own_expiry_output_reopens_without_trimming() {
    let mut request = project_request(history_rows(0));
    request["base"] = live_base();
    request["now"] = json!("2026-07-20T12:01:00Z");
    let expired = call("workspace.project.v1", &request);
    let history = expired["workspace"]["history"].as_array().unwrap();
    assert_eq!(history.len(), 10_001, "expiry appends without trimming");
    assert!(history.iter().any(|row| row["timerId"] == "live-timer"));
    assert!(history.iter().any(|row| row["timerId"] == "timer-00000"));
    let mut reopen = project_request(history_rows(0));
    reopen["base"]["canonicalTimer"] = expired["workspace"]["canonicalTimer"].clone();
    reopen["base"]["history"] = expired["workspace"]["history"].clone();
    reopen["now"] = json!("2026-07-20T12:01:00Z");
    let again = call("workspace.project.v1", &reopen);
    assert_eq!(
        again["workspace"]["history"],
        expired["workspace"]["history"]
    );
    assert_eq!(
        again["workspace"]["canonicalTimer"],
        expired["workspace"]["canonicalTimer"]
    );
}

#[test]
fn command_batch_cap_still_rejects() {
    let commands: Vec<_> = (0..10_001)
        .map(|i| {
            json!({"id": format!("cmd-{i}"), "deviceId": "device-a",
                "deviceSequence": 1, "timerId": "timer-a", "type": "start",
                "phase": "focus", "plannedDurationMs": 60000,
                "occurredAt": "2026-07-20T12:00:00Z",
                "hlcWallMs": 1, "hlcCounter": 0, "observedElapsedMs": 0})
        })
        .collect();
    let input = json!({"commands": commands, "now": NOW}).to_string();
    let error = dispatch_json("timer.reduce.v1", &input).unwrap_err();
    assert!(error.to_string().contains("commands exceed"), "{error}");
}
