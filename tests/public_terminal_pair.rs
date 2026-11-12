//! Public reducers accept a canonical timer that overlaps its own history row
//! only as a terminal pair that the workspace terminal validator accepts. The
//! row then takes the timer's elapsed time. Every other overlap keeps the
//! strict error.
use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const OVERLAP: &str = "invalid shared-core input: canonical timer overlaps timer history";
const INVALID_HISTORY: &str = "invalid shared-core input: invalid timer history";
const INVALID_DEVICE: &str = "invalid shared-core input: invalid native intent device identity";
const ELAPSED_MS: i64 = 10_000;

fn call(operation: &str, input: &Value) -> Result<Value, String> {
    dispatch_json(operation, &input.to_string())
        .map(|output| serde_json::from_str(&output).unwrap())
        .map_err(|error| error.to_string())
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-terminal-v1.json")).unwrap()
}

fn merged(mut value: Value, overrides: &Value) -> Value {
    for (name, field) in overrides.as_object().unwrap() {
        value[name] = field.clone();
    }
    value.as_object_mut().unwrap().retain(|_, v| !v.is_null());
    value
}

fn cancel_intent() -> Value {
    json!({"type": "cancel", "commandId": "cancel-existing", "occurredAt": "2026-07-20T12:00:00Z"})
}

/// Timer overrides whose cancel intent carries a native `deviceId`.
fn device_intent(device: Value) -> Value {
    let mut intent = cancel_intent();
    intent["deviceId"] = device;
    json!({"lastIntent": intent})
}

/// A genesis cancelled session: the timer recorded 10 s, the row predates
/// `elapsedMs`.
fn cancelled_timer(overrides: &Value) -> Value {
    let cancel = cancel_intent();
    let timer =
        json!({"status": "cancelled", "elapsedAtAnchorMs": ELAPSED_MS, "lastIntent": cancel});
    merged(merged(fixture()["timer"].clone(), &timer), overrides)
}

fn cancelled_row(overrides: &Value) -> Value {
    let row = json!({"status": "cancelled", "commandId": "cancel-existing", "completedAt": null});
    merged(merged(fixture()["history"].clone(), &row), overrides)
}

fn base(timer: &Value, history: &[Value]) -> Value {
    let mut base = fixture()["request"]["base"].clone();
    base["canonicalTimer"] = timer.clone();
    base["history"] = json!(history);
    base
}

/// The same timer/history input through `timer.reduce.v1` and
/// `projection.apply.v2`.
fn public_outputs(timer: &Value, history: &[Value]) -> [Result<Value, String>; 2] {
    let request = fixture()["request"].clone();
    let reduce = json!({"canonicalTimer": timer, "history": history, "commands": [],
        "now": request["now"]});
    let apply = json!({"base": base(timer, history), "pending": request["local"],
        "now": request["now"]});
    [
        call("timer.reduce.v1", &reduce),
        call("projection.apply.v2", &apply),
    ]
}

fn workspace_project(timer: &Value, history: &[Value], local: Value) -> Result<Value, String> {
    let mut request = fixture()["request"].clone();
    request["base"] = base(timer, history);
    request["local"] = local;
    call("workspace.project.v1", &request).map(|output| output["workspace"].clone())
}

#[test]
fn a_pre_field_cancelled_row_takes_the_elapsed_of_its_terminal_timer() {
    let timer = cancelled_timer(&json!({}));
    let row = cancelled_row(&json!({}));
    assert!(row.get("elapsedMs").is_none());
    for output in public_outputs(&timer, &[row]) {
        let output = output.unwrap();
        assert_eq!(output["history"][0]["elapsedMs"], ELAPSED_MS, "{output}");
        assert_eq!(output["canonicalTimer"]["elapsedAtAnchorMs"], ELAPSED_MS);
    }
}

/// `(name, timer overrides, rows, expected)`. `Ok(elapsed)` is the accepted
/// row's elapsed; `Err(message)` is the exact error.
type OverlapCase = (&'static str, Value, Vec<Value>, Result<i64, &'static str>);

fn overlap_cases() -> Vec<OverlapCase> {
    let none = json!({});
    vec![
        (
            "row without elapsed",
            none.clone(),
            vec![cancelled_row(&none)],
            Ok(ELAPSED_MS),
        ),
        (
            "row with equal elapsed",
            none.clone(),
            vec![cancelled_row(&json!({"elapsedMs": ELAPSED_MS}))],
            Ok(ELAPSED_MS),
        ),
        (
            "row with different elapsed",
            none.clone(),
            vec![cancelled_row(&json!({"elapsedMs": ELAPSED_MS - 1}))],
            Err(OVERLAP),
        ),
        (
            "running timer",
            json!({"status": "running"}),
            vec![cancelled_row(&none)],
            Err(OVERLAP),
        ),
        (
            "paused timer",
            json!({"status": "paused"}),
            vec![cancelled_row(&none)],
            Err(OVERLAP),
        ),
        (
            "different status",
            none.clone(),
            vec![cancelled_row(&json!({"status": "superseded"}))],
            Err(OVERLAP),
        ),
        (
            "different task",
            none.clone(),
            vec![cancelled_row(&json!({"taskId": "other-task"}))],
            Err(OVERLAP),
        ),
        (
            "different plan",
            none.clone(),
            vec![cancelled_row(&json!({"plannedDurationMs": 120_000}))],
            Err(OVERLAP),
        ),
        (
            "different end time",
            none.clone(),
            vec![cancelled_row(&json!({"endedAt": "2026-07-20T11:59:59Z"}))],
            Err(OVERLAP),
        ),
        (
            "different terminal command",
            none.clone(),
            vec![cancelled_row(&json!({"commandId": "cancel-other"}))],
            Err(OVERLAP),
        ),
        // History validation rejects two rows of one timer before any pairing.
        (
            "duplicate row for the timer",
            none.clone(),
            vec![
                cancelled_row(&none),
                cancelled_row(&json!({"id": "second-row"})),
            ],
            Err(INVALID_HISTORY),
        ),
        (
            "other row named after the timer",
            none.clone(),
            vec![cancelled_row(&json!({"timerId": "other-timer"}))],
            Err(OVERLAP),
        ),
        (
            "native device",
            device_intent(json!("device-origin")),
            vec![cancelled_row(&none)],
            Ok(ELAPSED_MS),
        ),
        (
            "empty native device",
            device_intent(json!("")),
            vec![cancelled_row(&none)],
            Err(INVALID_DEVICE),
        ),
        (
            "numeric native device",
            device_intent(json!(5)),
            vec![cancelled_row(&none)],
            Err(INVALID_DEVICE),
        ),
        // The device check runs before pairing, so a rejected pair with a bad
        // device reports the device error, as `workspace.project.v1` does.
        (
            "empty native device on a rejected pair",
            device_intent(json!("")),
            vec![cancelled_row(&json!({"commandId": "cancel-other"}))],
            Err(INVALID_DEVICE),
        ),
    ]
}

#[test]
fn public_reducers_accept_only_a_validated_terminal_pair() {
    for (name, timer_overrides, rows, expected) in overlap_cases() {
        let timer = cancelled_timer(&timer_overrides);
        for output in public_outputs(&timer, &rows) {
            match expected {
                Ok(elapsed) => {
                    let output = output.unwrap_or_else(|error| panic!("{name}: {error}"));
                    assert_eq!(output["history"][0]["elapsedMs"], elapsed, "{name}");
                }
                Err(message) => assert_eq!(output.unwrap_err(), message, "{name}"),
            }
        }
    }
}

/// The public reducers reject a bad native device exactly as
/// `workspace.project.v1` does, and keep a valid one on the accepted pair.
#[test]
fn public_reducers_validate_the_native_device_like_the_workspace_projection() {
    let rows = [cancelled_row(&json!({}))];
    let local = fixture()["request"]["local"].clone();
    for device in [json!(""), json!(5)] {
        let timer = cancelled_timer(&device_intent(device.clone()));
        let projected = workspace_project(&timer, &rows, local.clone());
        assert_eq!(projected.unwrap_err(), INVALID_DEVICE, "{device}");
    }
    let timer = cancelled_timer(&device_intent(json!("device-origin")));
    for output in public_outputs(&timer, &rows) {
        let device = &output.unwrap()["canonicalTimer"]["lastIntent"]["deviceId"];
        assert_eq!(device, "device-origin");
    }
}

#[test]
fn every_projection_gives_the_same_pair_and_it_passes_the_terminal_validator() {
    let timer = cancelled_timer(&json!({}));
    let rows = [cancelled_row(&json!({}))];
    let local = fixture()["request"]["local"].clone();
    let [reduced, applied] = public_outputs(&timer, &rows);
    let outputs = [
        reduced.unwrap(),
        applied.unwrap(),
        workspace_project(&timer, &rows, local.clone()).unwrap(),
    ];
    for output in &outputs {
        assert_eq!(output["history"], outputs[2]["history"]);
        assert_eq!(output["canonicalTimer"]["elapsedAtAnchorMs"], ELAPSED_MS);
        // The next zero-command read of the projected pair must accept it.
        let history = output["history"].as_array().unwrap();
        let reread = workspace_project(&output["canonicalTimer"], history, local.clone()).unwrap();
        assert_eq!(reread["history"], output["history"]);
        assert_eq!(reread["canonicalTimer"]["elapsedAtAnchorMs"], ELAPSED_MS);
    }
}

fn task_id(title: &str) -> Value {
    call("task.identity.v1", &json!({"title": title})).unwrap()["id"].clone()
}

#[test]
fn a_rename_moves_the_accepted_row_and_its_terminal_timer_together() {
    let task = json!({"taskId": task_id("A")});
    let timer = cancelled_timer(&task);
    let rows = [cancelled_row(&task)];
    let rename = json!({"id": "rename-a", "deviceId": "device-local",
        "occurredAt": "2026-07-20T12:00:30Z", "hlcWallMs": 1_784_548_830_000_i64, "hlcCounter": 0,
        "taskId": task_id("A"), "type": "rename", "title": "B"});
    let mut local = fixture()["request"]["local"].clone();
    local["taskOperations"] = json!([rename]);
    let apply = json!({"base": base(&timer, &rows), "pending": local,
        "now": fixture()["request"]["now"]});
    let applied = call("projection.apply.v2", &apply).unwrap();
    assert_eq!(applied["history"][0]["taskId"], task_id("B"));
    assert_eq!(applied["canonicalTimer"]["taskId"], task_id("B"));
    assert_eq!(applied["history"][0]["elapsedMs"], ELAPSED_MS);
    let history = applied["history"].as_array().unwrap();
    let reread = workspace_project(
        &applied["canonicalTimer"],
        history,
        fixture()["request"]["local"].clone(),
    );
    assert_eq!(reread.unwrap()["history"], applied["history"]);
}

/// Reconciliation keeps its strict canonical-response contract, even for a
/// pair that the public reducers now accept.
#[test]
fn old_reconciliation_still_rejects_an_accepted_pair() {
    let recorded: Value =
        serde_json::from_str(include_str!("../fixtures/reconciliation-terminal-v3.json")).unwrap();
    let mut response: Value =
        serde_json::from_str(recorded["http"]["responseRaw"].as_str().unwrap()).unwrap();
    response["acknowledgements"] = json!([]);
    response["canonicalTimer"] = cancelled_timer(&json!({}));
    response["history"] = json!([cancelled_row(&json!({}))]);
    let local = fixture()["request"]["local"].clone();
    let input = json!({"local": local, "sent": local, "response": response,
        "neverSent": {}, "timerDependencies": []});
    for operation in ["reconcile.rebase.v1", "reconcile.rebase.v2"] {
        assert_eq!(call(operation, &input).unwrap_err(), OVERLAP, "{operation}");
    }
}

/// A retained command that contradicts an accepted pair is a workspace
/// conflict, not an overlap: the pair itself passed the terminal validator.
#[test]
fn a_retained_command_that_contradicts_an_accepted_pair_is_a_workspace_conflict() {
    let fixture = fixture();
    let request = fixture["request"].clone();
    let mut finish = fixture["command"].clone();
    finish["id"] = json!("finish-existing");
    finish["type"] = json!("cancel");
    finish["occurredAt"] = fixture["timer"]["anchorAt"].clone();
    let mut pending = request["local"].clone();
    pending["commands"] = json!([finish]);
    let base = base(&fixture["timer"], &[fixture["history"].clone()]);
    let apply = json!({"base": base, "pending": pending, "now": request["now"]});
    assert_eq!(
        call("projection.apply.v2", &apply).unwrap_err(),
        fixture["conflictError"]
    );
}

/// `workspace.intent.v1` Start over a stored room pair, as Apple sends it
/// (`WorkspaceIntentRequestDetails.swift`).
fn start_over(timer: &Value, history: &[Value]) -> Result<Value, String> {
    let intent: Value =
        serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap();
    let mut request = intent["request"].clone();
    request["workspace"]["base"]["canonicalTimer"] = timer.clone();
    request["workspace"]["base"]["history"] = json!(history);
    call("workspace.intent.v1", &request)
}

/// R54 M1. An Iroh room projects from its genesis on every load. Apple restores
/// the raw genesis timer (`IrohRoomProjection.restoreTerminalTimer`) next to the
/// projected history and persists both. The reloaded pair must stay usable.
#[test]
fn a_reloaded_terminal_genesis_room_projects_and_plans() {
    let genesis_timer = cancelled_timer(&json!({}));
    let genesis_rows = [cancelled_row(&json!({}))];
    let [_, projected] = public_outputs(&genesis_timer, &genesis_rows);
    let stored_history = projected.unwrap()["history"].as_array().unwrap().clone();
    assert_eq!(stored_history[0]["elapsedMs"], ELAPSED_MS);
    let local = fixture()["request"]["local"].clone();
    let reread = workspace_project(&genesis_timer, &stored_history, local).unwrap();
    assert_eq!(reread["history"], json!(stored_history));
    for output in public_outputs(&genesis_timer, &stored_history) {
        assert_eq!(output.unwrap()["history"], json!(stored_history));
    }
    let planned = start_over(&genesis_timer, &stored_history).unwrap();
    assert_eq!(planned["outcome"], "planned", "{planned}");
}

/// The pair Core 0.55.0 let Apple persist: the pre-field row defaulted to 0 next
/// to the raw genesis timer at 10 s. It stays a conflict. No users exist, so no
/// stored copy needs a migration.
#[test]
fn the_persisted_zero_elapsed_genesis_pair_still_fails_closed() {
    let timer = cancelled_timer(&json!({}));
    let rows = [cancelled_row(&json!({"elapsedMs": 0}))];
    for output in public_outputs(&timer, &rows) {
        assert_eq!(output.unwrap_err(), OVERLAP);
    }
    let local = fixture()["request"]["local"].clone();
    let conflict = fixture()["conflictError"].clone();
    assert_eq!(
        json!(workspace_project(&timer, &rows, local).unwrap_err()),
        conflict
    );
    assert_eq!(json!(start_over(&timer, &rows).unwrap_err()), conflict);
}
