use pomodorough_core::reduce_selected_task_json;
use serde_json::Value;

#[test]
fn selected_task_reducer_is_order_independent_and_scrubs_deleted_tasks() {
    let operations = vec![
        serde_json::json!({"id":"a","deviceId":"device-a","taskId":"task-a","wallMs":100,"counter":0}),
        serde_json::json!({"id":"b","deviceId":"device-a","taskId":null,"wallMs":200,"counter":0}),
        serde_json::json!({"id":"c","deviceId":"device-b","taskId":"task-b","wallMs":200,"counter":0}),
    ];

    for order in permutations(&operations) {
        let selected: Value = serde_json::from_str(
            &reduce_selected_task_json(
                &serde_json::json!({"operations":order,"activeTaskIds":["task-a","task-b"]})
                    .to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(selected, serde_json::json!({"selectedTaskId":"task-b"}));

        let scrubbed: Value = serde_json::from_str(
            &reduce_selected_task_json(
                &serde_json::json!({"operations":order,"activeTaskIds":["task-a"]}).to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(scrubbed, serde_json::json!({"selectedTaskId":null}));
    }
}

fn permutations(values: &[Value]) -> Vec<Vec<Value>> {
    if values.is_empty() {
        return vec![vec![]];
    }
    let mut result = Vec::new();
    for index in 0..values.len() {
        let mut rest = values.to_vec();
        let value = rest.remove(index);
        for mut suffix in permutations(&rest) {
            let mut order = vec![value.clone()];
            order.append(&mut suffix);
            result.push(order);
        }
    }
    result
}
