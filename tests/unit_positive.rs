use pomodorough_core::{SelectedTaskField, classify_selected_task_field_json, dispatch_json};
use serde_json::{Value, json};

#[test]
fn selected_task_classification_preserves_escaped_unicode_identity() {
    let classification =
        classify_selected_task_field_json(r#"{"selectedTaskId":"task-\u00e9-\ud83d\ude80"}"#)
            .expect("selected task classification");

    assert_eq!(classification, "selected:task-é-🚀");
}

#[test]
fn selected_task_wire_state_distinguishes_omitted_deselected_and_selected() {
    assert_eq!(classify_selected_task_field_json("{}").unwrap(), "omitted");
    assert_eq!(
        classify_selected_task_field_json(r#"{"selectedTaskId":null}"#).unwrap(),
        "deselected"
    );

    let selected = SelectedTaskField::Selected("task-wire-0001".into());
    assert_eq!(
        serde_json::to_string(&selected).unwrap(),
        r#""task-wire-0001""#
    );
    assert_eq!(
        serde_json::to_string(&SelectedTaskField::Deselected).unwrap(),
        "null"
    );
    assert_eq!(
        serde_json::to_string(&SelectedTaskField::Omitted).unwrap(),
        "null"
    );
}

#[test]
fn bootstrap_dispatch_requires_choice_for_remote_history_plus_local_state() {
    let output: Value = serde_json::from_str(
        &dispatch_json(
            "bootstrap.plan.v1",
            &json!({
                "localHistory": [],
                "remoteHistory": [{
                    "id": "history-remote",
                    "timerId": "timer-remote",
                    "status": "completed"
                }],
                "hasLocalState": true,
                "hasRemoteState": true
            })
            .to_string(),
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(output["mode"], "choose");
    assert_eq!(output["localHistoryCount"], 0);
    assert_eq!(output["remoteHistoryCount"], 1);
}
