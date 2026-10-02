use pomodorough_core::dispatch_json;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    input: Value,
    expected: Value,
}

fn cases() -> Vec<Case> {
    serde_json::from_str::<Fixture>(include_str!("../fixtures/bootstrap-plan-v1-desktop.json"))
        .unwrap()
        .cases
}

fn assert_native_contract(cases: &[Case]) {
    for case in cases {
        let input = case.input.to_string();
        let raw = dispatch_json("bootstrap.plan.v1", &input).unwrap();
        let native: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            native, case.expected,
            "{} complete native output",
            case.name
        );
        assert_eq!(dispatch_json("bootstrap.plan.v1", &input).unwrap(), raw);
    }
}

fn assert_named_case(name: &str) {
    let cases = cases()
        .into_iter()
        .filter(|case| case.name == name)
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 1, "missing or duplicate fixture case {name}");
    assert_native_contract(&cases);
}

#[test]
fn bootstrap_plan_v1_all_branches_preserve_shipped_output_contract() {
    let cases = cases();
    assert_eq!(cases.len(), 22);
    assert_native_contract(&cases);
}

#[test]
fn bootstrap_plan_v1_both_state_without_completed_history_keeps_merge_reason() {
    assert_named_case("both_state_only");
}

#[test]
fn bootstrap_plan_v1_remote_state_without_completed_history_keeps_empty_reason() {
    assert_named_case("remote_state_only");
}
