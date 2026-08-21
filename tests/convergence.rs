use pomodorough_core::reduce_timer_fixture_case_json;
use serde_json::Value;
use sha2::{Digest, Sha256};

const FIXTURE: &[u8] = include_bytes!("../fixtures/convergence-v1.json");
const EXPECTED_SHA256: &str = "a293a679179f7f441a89b04f0260ee77fc0d810abc61e99501f9260a6ea9012e";

#[test]
fn canonical_timer_reducer_matches_every_fixture_arrival_order() {
    let digest = format!("{:x}", Sha256::digest(FIXTURE));
    assert_eq!(digest, EXPECTED_SHA256);

    let fixture: Value = serde_json::from_slice(FIXTURE).unwrap();
    let epoch = fixture["epoch"].as_str().unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let commands = case["commands"].as_array().unwrap();
        for order in permutations(commands) {
            let input = serde_json::json!({
                "epoch": epoch,
                "nowMs": case["nowMs"],
                "commands": order,
            });
            let actual: Value =
                serde_json::from_str(&reduce_timer_fixture_case_json(&input.to_string()).unwrap())
                    .unwrap();
            assert_eq!(actual, case["expected"], "case {}", case["name"]);
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
