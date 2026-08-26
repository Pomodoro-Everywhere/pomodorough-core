use pomodorough_core::dispatch_json;
use serde_json::{Value, json};

fn head(physical_now_ms: i64, observed: Vec<Value>) -> Result<Value, String> {
    let input = json!({
        "physicalNowMs": physical_now_ms,
        "observed": observed,
    });
    dispatch_json("hlc.head.v1", &input.to_string())
        .map_err(|error| error.to_string())
        .and_then(|output| serde_json::from_str(&output).map_err(|error| error.to_string()))
}

#[test]
fn hlc_head_returns_non_incrementing_lexicographic_maximum() {
    let cases = [
        (
            100,
            vec![
                json!({"wallMs": 100, "counter": 4}),
                json!({"wallMs": 100, "counter": 7}),
            ],
            json!({"wallMs": 100, "counter": 7}),
        ),
        (
            100,
            vec![
                json!({"wallMs": 99, "counter": 900}),
                json!({"wallMs": 101, "counter": 3}),
                json!({"wallMs": 100, "counter": 999}),
            ],
            json!({"wallMs": 101, "counter": 3}),
        ),
        (
            100,
            vec![json!({"wallMs": 100, "counter": 0})],
            json!({"wallMs": 100, "counter": 0}),
        ),
    ];

    for (physical_now_ms, observed, expected) in cases {
        assert_eq!(head(physical_now_ms, observed).unwrap(), expected);
    }
}

#[test]
fn hlc_head_uses_physical_now_for_empty_or_older_observations() {
    assert_eq!(
        head(321, vec![]).unwrap(),
        json!({"wallMs": 321, "counter": 0})
    );
    assert_eq!(
        head(321, vec![json!({"wallMs": 320, "counter": 99})]).unwrap(),
        json!({"wallMs": 321, "counter": 0})
    );
}

#[test]
fn hlc_head_rejects_invalid_values_and_shapes() {
    let invalid = [
        json!({"physicalNowMs": -1, "observed": []}),
        json!({"physicalNowMs": 9_007_199_254_740_992_i64, "observed": []}),
        json!({"physicalNowMs": 0, "observed": [{"wallMs": -1, "counter": 0}]}),
        json!({"physicalNowMs": 0, "observed": [{"wallMs": 0, "counter": -1}]}),
        json!({"physicalNowMs": 0, "observed": [{"wallMs": 0, "counter": 9_007_199_254_740_992_i64}]}),
        json!({"physicalNowMs": 0}),
        json!({"physicalNowMs": 0, "observed": [], "extra": true}),
        json!({"physicalNowMs": 0, "observed": [{"wallMs": 0, "counter": 0, "extra": true}]}),
    ];

    for input in invalid {
        assert!(
            dispatch_json("hlc.head.v1", &input.to_string()).is_err(),
            "accepted {input}"
        );
    }
}

#[test]
fn hlc_head_is_permutation_invariant() {
    let clocks = [
        json!({"wallMs": 100, "counter": 8}),
        json!({"wallMs": 101, "counter": 1}),
        json!({"wallMs": 101, "counter": 3}),
    ];
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];

    for order in orders {
        let observed = order.map(|index| clocks[index].clone()).to_vec();
        assert_eq!(
            head(99, observed).unwrap(),
            json!({"wallMs": 101, "counter": 3})
        );
    }
}
