use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

// R51-C02: adapter-generated civil-day intervals. Platform owns
// local-midnight boundaries; Core admits 22h..=26h and keeps nonempty,
// ordered, nonoverlapping, observation-contained checks.

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/read-model-v1.json")).unwrap()
}

fn output(input: &Value) -> Value {
    serde_json::from_str(&dispatch_json("workspace.readModel.v1", &input.to_string()).unwrap())
        .unwrap()
}

fn task_id() -> String {
    let identity =
        dispatch_json("task.identity.v1", &json!({"title": "Study"}).to_string()).unwrap();
    serde_json::from_str::<Value>(&identity).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn completion(index: usize, at: &str) -> Value {
    json!({"id": format!("history-{index}"), "timerId": format!("timer-{index}"),
        "phase": "focus", "status": "completed", "plannedDurationMs": 1500000,
        "taskId": task_id(), "completedAt": at})
}

fn request_for(profile: &str, observed: &str, start: &str, end: &str) -> Value {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    request["profile"] = json!(profile);
    request["observedAt"] = json!(observed);
    request["calendarIntervals"] = json!([{"start": start, "end": end}]);
    request
}

fn assert_renders_all_profiles(cases: &[(&str, &str, &str)]) {
    for profile in [
        "appleWorkspace",
        "androidCoordinator",
        "desktopStorage",
        "desktopTerminal",
        "pwaStorage",
    ] {
        for (observed, start, end) in cases {
            let result = output(&request_for(profile, observed, start, end));
            assert_eq!(
                result["cadence"]["completedFocusToday"], 0,
                "{profile}/{observed}"
            );
            assert_eq!(
                result["cadence"]["completedFocusTotal"], 0,
                "{profile}/{observed}"
            );
        }
    }
}

#[test]
fn troll_transition_days_render_in_all_profiles() {
    // Production JS Date adapter output for Antarctica/Troll.
    assert_renders_all_profiles(&[
        (
            "2026-03-29T12:00:00Z",
            "2026-03-29T00:00:00.000Z",
            "2026-03-29T22:00:00.000Z",
        ),
        (
            "2026-10-25T12:00:00Z",
            "2026-10-24T22:00:00.000Z",
            "2026-10-26T00:00:00.000Z",
        ),
        (
            "2026-03-28T12:00:00Z",
            "2026-03-28T00:00:00.000Z",
            "2026-03-29T00:00:00.000Z",
        ),
        (
            "2026-10-26T12:00:00Z",
            "2026-10-26T00:00:00.000Z",
            "2026-10-27T00:00:00.000Z",
        ),
    ]);
}

#[test]
fn dst_control_days_render_in_all_profiles() {
    // Production JS Date adapter output for New York and Lord Howe.
    assert_renders_all_profiles(&[
        (
            "2026-03-08T12:00:00Z",
            "2026-03-08T05:00:00.000Z",
            "2026-03-09T04:00:00.000Z",
        ),
        (
            "2026-11-01T12:00:00Z",
            "2026-11-01T04:00:00.000Z",
            "2026-11-02T05:00:00.000Z",
        ),
        (
            "2026-10-04T12:00:00Z",
            "2026-10-03T13:30:00.000Z",
            "2026-10-04T13:00:00.000Z",
        ),
        (
            "2026-04-05T12:00:00Z",
            "2026-04-04T13:00:00.000Z",
            "2026-04-05T13:30:00.000Z",
        ),
    ]);
}

#[test]
fn troll_day_edges_count_start_inclusive_end_exclusive() {
    for (observed, start, end, before) in [
        (
            "2026-03-29T12:00:00Z",
            "2026-03-29T00:00:00.000Z",
            "2026-03-29T22:00:00.000Z",
            "2026-03-28T23:59:59.999Z",
        ),
        (
            "2026-10-25T12:00:00Z",
            "2026-10-24T22:00:00.000Z",
            "2026-10-26T00:00:00.000Z",
            "2026-10-24T21:59:59.999Z",
        ),
    ] {
        let id = task_id();
        let mut request = request_for("pwaStorage", observed, start, end);
        request["source"]["value"]["base"]["tasks"] = json!([{"id": id, "title": "Study"}]);
        let just_before_end = if end.starts_with("2026-03") {
            "2026-03-29T21:59:59.999Z"
        } else {
            "2026-10-25T23:59:59.999Z"
        };
        request["source"]["value"]["base"]["history"] = json!([
            completion(1, start),
            completion(2, just_before_end),
            completion(3, end),
            completion(4, before),
        ]);
        let mut history = request["source"]["value"]["base"]["history"].clone();
        for row in history.as_array_mut().unwrap() {
            row["taskId"] = json!(id);
        }
        request["source"]["value"]["base"]["history"] = history;
        let result = output(&request);
        assert_eq!(result["cadence"]["completedFocusToday"], 2, "{observed}");
        assert_eq!(result["cadence"]["completedFocusTotal"], 4, "{observed}");
        assert_eq!(
            result["cadence"]["completedFocusTodayPlannedDurationMs"], 3_000_000,
            "{observed}"
        );
        assert_eq!(
            result["tasks"]["completedFocusTodayByTask"][&id],
            json!({"count": 2, "plannedDurationMs": 3_000_000}),
            "{observed}"
        );
    }
}

#[test]
fn civil_day_failures_carry_interval_and_observation_context() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    request["observedAt"] = json!("2026-03-29T12:00:00Z");
    request["calendarIntervals"] =
        json!([{"start": "2026-03-29T00:00:00.000Z", "end": "2026-03-29T21:59:59.000Z"}]);
    let error = dispatch_json("workspace.readModel.v1", &request.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains("index=0"), "{error}");
    assert!(error.contains("2026-03-29T00:00:00.000Z"), "{error}");
    assert!(error.contains("2026-03-29T21:59:59.000Z"), "{error}");
    assert!(error.contains("duration_ms=79199000"), "{error}");
    assert!(error.contains("2026-03-29T12:00:00"), "{error}");

    request["calendarIntervals"] =
        json!([{"start": "2026-03-29T00:00:00.000Z", "end": "2026-03-29T00:00:00.000Z"}]);
    let error = dispatch_json("workspace.readModel.v1", &request.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains("index=0"), "{error}");
    assert!(error.contains("duration_ms=0"), "{error}");

    request["calendarIntervals"] = json!([
        {"start": "2026-03-29T00:00:00.000Z", "end": "2026-03-29T22:00:00.000Z"},
        {"start": "2026-03-29T12:00:00.000Z", "end": "2026-03-30T12:00:00.000Z"},
    ]);
    let error = dispatch_json("workspace.readModel.v1", &request.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains("overlapping"), "{error}");
    assert!(
        error.contains("first_start=2026-03-29T00:00:00.000Z"),
        "{error}"
    );
    assert!(
        error.contains("first_end=2026-03-29T22:00:00.000Z"),
        "{error}"
    );
    assert!(
        error.contains("second_start=2026-03-29T12:00:00.000Z"),
        "{error}"
    );
    assert!(
        error.contains("second_end=2026-03-30T12:00:00.000Z"),
        "{error}"
    );
    assert!(error.contains("2026-03-29T12:00:00"), "{error}");

    request["calendarIntervals"] =
        json!([{"start": "2026-03-28T00:00:00.000Z", "end": "2026-03-29T00:00:00.000Z"}]);
    let error = dispatch_json("workspace.readModel.v1", &request.to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains("missing"), "{error}");
    assert!(error.contains("2026-03-29T12:00:00"), "{error}");
    assert!(
        error.contains("intervals=[2026-03-28T00:00:00.000Z->2026-03-29T00:00:00.000Z]"),
        "{error}"
    );
}

#[test]
fn civil_day_single_interval_rejects_cover_edges() {
    let fixture = fixture();
    let mut request = fixture["request"].clone();
    request["observedAt"] = json!("2026-03-29T12:00:00Z");
    for (start, end, duration_ms) in [
        (
            "2026-03-29T00:00:00.000Z",
            "2026-03-30T21:30:00.000Z",
            "duration_ms=163800000",
        ),
        (
            "2026-03-29T12:00:00.000Z",
            "2026-03-29T00:00:00.000Z",
            "duration_ms=-43200000",
        ),
        (
            "2026-03-29T00:00:00.000Z",
            "2026-03-29T21:59:59.999Z",
            "duration_ms=79199999",
        ),
        (
            "2026-03-29T00:00:00.000Z",
            "2026-03-30T02:00:00.001Z",
            "duration_ms=93600001",
        ),
    ] {
        request["calendarIntervals"] = json!([{"start": start, "end": end}]);
        let error = dispatch_json("workspace.readModel.v1", &request.to_string())
            .unwrap_err()
            .to_string();
        assert!(error.contains("index=0"), "{start}/{end} {error}");
        assert!(error.contains(duration_ms), "{start}/{end} {error}");
        assert!(error.contains(start), "{error}");
        assert!(error.contains(end), "{error}");
        assert!(error.contains("2026-03-29T12:00:00"), "{error}");
    }
    request["calendarIntervals"] = json!([]);
    let error = dispatch_json("workspace.readModel.v1", &request.to_string())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("missing read model calendar interval"),
        "{error}"
    );
    assert!(error.contains("intervals=[]"), "{error}");
}
