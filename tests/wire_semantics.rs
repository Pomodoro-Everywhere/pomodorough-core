use pomodorough_core::classify_selected_task_field_json;

#[test]
fn selected_task_field_distinguishes_omission_null_and_value() {
    assert_eq!(
        classify_selected_task_field_json(r#"{}"#).unwrap(),
        "omitted"
    );
    assert_eq!(
        classify_selected_task_field_json(r#"{"selectedTaskId":null}"#).unwrap(),
        "deselected"
    );
    assert_eq!(
        classify_selected_task_field_json(
            r#"{"selectedTaskId":"33f9d32c-a7ee-8aa9-897a-13e19bc4e5d4"}"#
        )
        .unwrap(),
        "selected:33f9d32c-a7ee-8aa9-897a-13e19bc4e5d4"
    );
}
