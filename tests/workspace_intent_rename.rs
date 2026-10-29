use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

const PROFILES: [&str; 4] = [
    "appleWorkspace",
    "androidCoordinator",
    "desktopStorage",
    "pwaStorage",
];

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/workspace-intent-v1.json")).unwrap()
}

fn request(profile: &str, intent: Value) -> Value {
    let mut input = fixture()["request"].take();
    input["compatibility"] = json!(profile);
    input["intent"] = intent;
    input["ownership"] = json!({"expectedOwnerId": "account-a", "ownerId": "account-a"});
    input["durability"] = json!({"outgoingDurationOperationIds": [], "localTabId": "tab-a"});
    input["identities"]["commandUuids"] = json!([
        "019f7f65-dd10-7000-8000-000000000001",
        "019f7f65-dd10-7000-8000-000000000002",
        "019f7f65-dd10-7000-8000-000000000003",
        "019f7f65-dd10-7000-8000-000000000004"
    ]);
    input
}

fn plan(input: &Value) -> Result<Value, String> {
    dispatch_json("workspace.intent.v1", &input.to_string())
        .map(|output| serde_json::from_str(&output).unwrap())
        .map_err(|error| error.to_string())
}

fn task(title: &str) -> Value {
    serde_json::from_str(
        &dispatch_json("task.identity.v1", &json!({"title": title}).to_string()).unwrap(),
    )
    .unwrap()
}

fn rename_request(profile: &str, old: &Value, title: &str) -> Value {
    let mut input = request(
        profile,
        json!({"kind": "renameTask", "taskId": old["id"], "title": title}),
    );
    input["workspace"]["base"]["tasks"] = json!([{"id": old["id"], "title": old["title"]}]);
    input
}

fn task_operations(output: &Value) -> Vec<(String, String)> {
    output["operations"]["taskOperations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|op| {
            (
                op["type"].as_str().unwrap().to_owned(),
                op["taskId"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn rename_replaces_identity_and_moves_selection_and_active_focus_on_every_profile() {
    let old = task("Café");
    let new = task("Café au lait");
    for profile in PROFILES {
        let mut input = rename_request(profile, &old, "Cafe\u{301} au lait");
        input["workspace"]["base"]["selectedTaskId"] = old["id"].clone();
        input["workspace"]["base"]["canonicalTimer"] = fixture()["timer"].clone();
        input["workspace"]["base"]["canonicalTimer"]["taskId"] = old["id"].clone();
        let output = plan(&input).unwrap();
        assert_eq!(output["outcome"], "planned", "{profile}");
        assert_eq!(
            task_operations(&output),
            vec![
                ("upsert".to_owned(), new["id"].as_str().unwrap().to_owned()),
                ("delete".to_owned(), old["id"].as_str().unwrap().to_owned()),
            ],
            "{profile}"
        );
        assert_eq!(
            output["operations"]["taskOperations"][0]["title"], new["title"],
            "{profile}"
        );
        let selected = output["operations"]["selectedTaskOperations"]
            .as_array()
            .unwrap();
        assert_eq!(selected.len(), 1, "{profile}");
        assert_eq!(selected[0]["taskId"], new["id"], "{profile}");
        let commands = output["operations"]["commands"].as_array().unwrap();
        assert_eq!(commands.len(), 1, "{profile}");
        assert_eq!(commands[0]["type"], "retarget", "{profile}");
        assert_eq!(commands[0]["taskId"], new["id"], "{profile}");
        assert_eq!(
            output["projection"]["tasks"],
            json!([{"id": new["id"], "title": new["title"]}]),
            "{profile}"
        );
        assert_eq!(
            output["projection"]["selectedTaskId"], new["id"],
            "{profile}"
        );
        assert_eq!(
            output["projection"]["canonicalTimer"]["taskId"], new["id"],
            "{profile}"
        );
    }
}

#[test]
fn rename_of_unselected_task_during_break_only_replaces_the_record() {
    let old = task("Café");
    let other = task("Other");
    let new = task("Espresso");
    for profile in PROFILES {
        let mut input = rename_request(profile, &old, "Espresso");
        assert_ne!(new["id"], old["id"]);
        input["workspace"]["base"]["tasks"] = json!([
            {"id": old["id"], "title": old["title"]},
            {"id": other["id"], "title": other["title"]}
        ]);
        input["workspace"]["base"]["selectedTaskId"] = other["id"].clone();
        input["workspace"]["base"]["canonicalTimer"] = json!({
            "id": "timer-break", "phase": "short_break", "status": "running",
            "plannedDurationMs": 120000, "elapsedAtAnchorMs": 5000,
            "anchorAt": "2026-07-20T12:00:00Z"
        });
        let output = plan(&input).unwrap();
        let ops = task_operations(&output);
        assert_eq!(ops.len(), 2, "{profile}");
        assert!(ops.contains(&("upsert".to_owned(), new["id"].as_str().unwrap().to_owned())));
        assert_eq!(output["operations"]["selectedTaskOperations"], json!([]));
        assert_eq!(output["operations"]["commands"], json!([]));
        assert_eq!(output["projection"]["selectedTaskId"], other["id"]);
        let mut titles: Vec<_> = output["projection"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["title"].as_str().unwrap().to_owned())
            .collect();
        titles.sort();
        assert_eq!(titles, vec!["Espresso".to_owned(), "Other".to_owned()]);
    }
}

#[test]
fn rename_to_existing_title_merges_without_a_second_upsert() {
    let old = task("Café");
    let existing = task("Espresso");
    let mut input = rename_request("desktopStorage", &old, "Espresso");
    input["workspace"]["base"]["tasks"] = json!([
        {"id": old["id"], "title": old["title"]},
        {"id": existing["id"], "title": existing["title"]}
    ]);
    input["workspace"]["base"]["selectedTaskId"] = old["id"].clone();
    let output = plan(&input).unwrap();
    assert_eq!(
        task_operations(&output),
        vec![("delete".to_owned(), old["id"].as_str().unwrap().to_owned())]
    );
    assert_eq!(
        output["operations"]["selectedTaskOperations"][0]["taskId"],
        existing["id"]
    );
    assert_eq!(
        output["projection"]["tasks"],
        json!([{"id": existing["id"], "title": existing["title"]}])
    );
    assert_eq!(output["projection"]["selectedTaskId"], existing["id"]);
}

#[test]
fn rename_to_an_equivalent_title_is_a_noop() {
    let old = task("Café");
    let input = rename_request("androidCoordinator", &old, "Cafe\u{301}\t");
    let output = plan(&input).unwrap();
    assert_eq!(output["outcome"], "noop");
    assert_eq!(output["reason"], "unchangedOrIneligible");
    assert_eq!(output["operations"]["taskOperations"], json!([]));
}

#[test]
fn rename_rejects_unknown_empty_and_invalid_inputs() {
    let old = task("Café");
    let missing = task("Missing");
    let cases = [
        (
            json!({"kind": "renameTask", "taskId": missing["id"], "title": "New"}),
            "task rename requires an active task",
        ),
        (
            json!({"kind": "renameTask", "taskId": "", "title": "New"}),
            "invalid task identity",
        ),
        (
            json!({"kind": "renameTask", "taskId": old["id"], "title": "\n\t"}),
            "task title is empty after normalization",
        ),
        (
            json!({"kind": "renameTask", "taskId": old["id"]}),
            "missing field `title`",
        ),
        (
            json!({"kind": "renameTask", "taskId": old["id"], "title": "New", "extra": 1}),
            "unknown field `extra`",
        ),
    ];
    for (intent, expected) in cases {
        let mut input = request("pwaStorage", intent.clone());
        input["workspace"]["base"]["tasks"] = json!([{"id": old["id"], "title": old["title"]}]);
        let error = plan(&input).unwrap_err();
        assert!(error.contains(expected), "{intent}: {error}");
    }
}

#[test]
fn rename_is_rejected_for_desktop_terminal_like_other_task_entrypoints() {
    let old = task("Café");
    let input = rename_request("desktopTerminal", &old, "New");
    let error = plan(&input).unwrap_err();
    assert!(error.contains("desktopStorage"), "{error}");
}
