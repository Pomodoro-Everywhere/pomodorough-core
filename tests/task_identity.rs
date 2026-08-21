use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

#[test]
fn task_identity_normalizes_edges_and_matches_server_uuidv8() {
    let input = json!({"title": "\u{0000}Cafe\u{0301}\u{001f}"});
    let output: Value = serde_json::from_str(
        &dispatch_json("task.identity.v1", &input.to_string()).expect("identity"),
    )
    .unwrap();
    assert_eq!(output["title"], "Café");
    assert_eq!(output["id"], "aaf83054-24b2-8c0e-901f-a974147bfe82");
    assert_eq!(output["utf8Bytes"], 5);
}

#[test]
fn task_identity_preserves_printable_edge_spaces_and_internal_format_scalars() {
    let output: Value = serde_json::from_str(
        &dispatch_json(
            "task.identity.v1",
            &json!({"title": "\u{00a0} task \u{00a0}"}).to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(output["title"], " task ");

    let output: Value = serde_json::from_str(
        &dispatch_json(
            "task.identity.v1",
            &json!({"title": "\u{00a0}task\u{200b}name\u{00a0}"}).to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(output["title"], "task\u{200b}name");
}

#[test]
fn task_identity_rejects_empty_and_oversized_normalized_titles() {
    for title in ["\u{0000}\u{00a0}", &"é".repeat(257)] {
        let envelope: Value = serde_json::from_str(&dispatch_envelope_json(
            "task.identity.v1",
            &json!({"title": title}).to_string(),
        ))
        .unwrap();
        assert_eq!(envelope["ok"], false);
        assert!(envelope["error"].as_str().unwrap().contains("task title"));
    }
}
