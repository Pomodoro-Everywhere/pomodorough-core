use super::*;
use crate::workspace_intent::model::Input;
use serde_json::json;

fn group() -> (Input, Value, Value) {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/workspace-intent-v1.json"
    ))
    .unwrap();
    let known: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/workspace-intent-desktop-known-tasks-v1.json"
    ))
    .unwrap();
    let mut raw = fixture["request"].clone();
    raw["compatibility"] = json!("desktopStorage");
    raw["intent"] = json!({"kind": "deleteTask", "taskId": known["knownTasks"][0]["id"]});
    raw["workspace"]["base"]["tasks"] = known["knownTasks"].clone();
    raw["workspace"]["base"]["selectedTaskId"] = known["knownTasks"][0]["id"].clone();
    raw["workspace"]["base"]["canonicalTimer"] = fixture["timer"].clone();
    raw["ownership"] = json!({"ownerId": null, "expectedOwnerId": null});
    raw["durability"] = json!({"outgoingDurationOperationIds": []});
    raw["identities"]["commandUuids"]
        .as_array_mut()
        .unwrap()
        .push(json!("019f7f65-dd10-7000-8000-000000000003"));
    let output: Value = serde_json::from_str(
        &crate::dispatch_json("workspace.intent.v1", &raw.to_string()).unwrap(),
    )
    .unwrap();
    let input: Input = serde_json::from_value(raw).unwrap();
    (
        input,
        output["workspace"].clone(),
        output["operations"].clone(),
    )
}

#[test]
fn queued_delete_rejects_ignored_or_malformed_group_members() {
    let (input, workspace, operations) = group();
    for corrupt in [
        "ignored-retarget",
        "invalid-retarget",
        "unknown-selection",
        "invalid-delete",
    ] {
        let mut invalid = operations.clone();
        match corrupt {
            "ignored-retarget" => invalid["commands"][0]["timerId"] = json!("unrelated-timer"),
            "invalid-retarget" => invalid["commands"][0]["phase"] = json!("short_break"),
            "unknown-selection" => {
                invalid["selectedTaskOperations"][0]["taskId"] = json!("unknown-task")
            }
            _ => invalid["taskOperations"][0]["type"] = json!("unknown-operation"),
        }
        let mut candidate = workspace.clone();
        candidate["local"] = invalid.clone();
        assert!(
            super::group(
                &input,
                &candidate,
                &invalid,
                &input.observation,
                &input.clock.physical_now
            )
            .is_err(),
            "{corrupt}"
        );
    }
}
