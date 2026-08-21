use pomodorough_core::reduce_projection_fixture_case_json;
use serde_json::Value;

const FIXTURE: &[u8] = include_bytes!("../fixtures/convergence-v1.json");

#[test]
fn operation_projection_matches_fixture_for_every_arrival_order() {
    let fixture: Value = serde_json::from_slice(FIXTURE).unwrap();
    for case in fixture["projectionCases"].as_array().unwrap() {
        let tasks = case["taskOperations"].as_array().unwrap();
        let durations = case["durationOperations"].as_array().unwrap();
        let auto_start = case["autoStartOperations"].as_array().unwrap();

        for task_order in permutations(tasks) {
            for duration_order in permutations(durations) {
                for auto_start_order in permutations(auto_start) {
                    let input = serde_json::json!({
                        "taskOperations": task_order,
                        "durationOperations": duration_order,
                        "autoStartOperations": auto_start_order,
                    });
                    let actual: Value = serde_json::from_str(
                        &reduce_projection_fixture_case_json(&input.to_string()).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(actual, case["expected"], "case {}", case["name"]);
                }
            }
        }
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
