use pomodorough_core::{dispatch_envelope_json, dispatch_json};
use serde_json::{Value, json};

const DOMAINS: [&str; 5] = [
    "commands",
    "taskOperations",
    "durationOperations",
    "autoStartOperations",
    "selectedTaskOperations",
];

fn call(operation: &str, input: &Value) -> Value {
    serde_json::from_str(&dispatch_json(operation, &input.to_string()).unwrap()).unwrap()
}

fn captured() -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/pwa-display-admission-v1.json"))
        .unwrap()["request"]
        .clone()
}

fn workspace() -> Value {
    let mut input: Value =
        serde_json::from_str::<Value>(include_str!("../fixtures/workspace-projection-v1.json"))
            .unwrap()["request"]
            .clone();
    input["canonicalHead"] = Value::Null;
    input["displayContext"] = json!({"profile":"pwaStorage", "projectionPending":empty()});
    input
}

fn empty() -> Value {
    DOMAINS
        .into_iter()
        .map(|name| (name.to_owned(), json!([])))
        .collect()
}

#[test]
fn captured_null_head_duration_is_applied_and_persisted_without_delivery_permission() {
    for (phase, minutes) in [("short_break", 10), ("long_break", 30)] {
        let mut input = captured();
        input["intent"] = json!({"kind":"setDuration", "phase":phase, "minutes":minutes});
        let output = call("workspace.intent.v1", &input);
        assert_eq!(output["projection"]["durationsMs"][phase], minutes * 60000);
        assert_eq!(
            output["groupOutcomes"]["durationOperations"][0]["outcome"],
            "applied"
        );
        assert_eq!(
            output["workspace"]["displayContext"]["projectionPending"]["durationOperations"],
            output["workspace"]["local"]["durationOperations"]
        );
        assert_eq!(output["workspace"]["canonicalHead"], Value::Null);
        assert_eq!(output["workspace"]["base"], input["workspace"]["base"]);
        let mut reload = output["workspace"].clone();
        reload["now"] = input["clock"]["physicalNow"].clone();
        let reopened = call("workspace.project.v1", &reload);
        assert_eq!(reopened["projectionPending"], empty());
        assert_eq!(reopened["workspace"], output["projection"]);
    }
}

#[test]
fn all_five_domains_admit_proven_null_head_rows_and_preserve_raw_extensions() {
    let mut input = workspace();
    for domain in DOMAINS {
        input["local"][domain][0]["extension"] = json!({"null":null, "nested":[false, {}]});
    }
    let output = call("workspace.project.v1", &input);
    assert_eq!(
        output["displayContext"]["projectionPending"],
        input["local"]
    );
    assert_eq!(output["projectionPending"], empty());
    assert_eq!(output["workspace"]["durationsMs"]["focus"], 1800000);
    assert_eq!(output["workspace"]["autoStartBreaks"], true);
    assert_eq!(output["workspace"]["tasks"], json!([]));
}

#[test]
fn preference_admission_requires_every_unstored_row_to_be_fresh_and_proven() {
    for domain in DOMAINS.into_iter().filter(|domain| *domain != "commands") {
        for stored_old in [false, true] {
            for blocked_by in ["none", "claim", "stale"] {
                let input = domain_barrier(domain, stored_old, blocked_by);
                let output = call("workspace.project.v1", &input);
                let eligible = stored_old || blocked_by == "none";
                assert_eq!(
                    output["displayContext"]["projectionPending"][domain],
                    if eligible {
                        input["local"][domain].clone()
                    } else {
                        input["displayContext"]["projectionPending"][domain].clone()
                    }
                );
                assert_eq!(output["projectionPending"][domain], json!([]));
            }
        }
    }
}

fn domain_barrier(domain: &str, stored_old: bool, blocked_by: &str) -> Value {
    let mut input = workspace();
    let old = input["local"][domain][0].clone();
    let mut fresh = old.clone();
    fresh["id"] = json!("fresh");
    fresh["hlcCounter"] = json!(12);
    input["local"][domain] = json!([old, fresh]);
    input["neverSent"][domain] = json!(["fresh"]);
    if stored_old {
        input["displayContext"]["projectionPending"][domain] = json!([old]);
    }
    if blocked_by != "claim" {
        input["neverSent"][domain]
            .as_array_mut()
            .unwrap()
            .push(old["id"].clone());
    }
    if blocked_by == "stale" {
        input["canonicalHead"] = json!({"wallMs":1784548800000_i64,"counter":11});
    }
    input
}

#[test]
fn stored_claimed_preference_can_be_superseded_but_hidden_claim_blocks_whole_domain() {
    for stored in [true, false] {
        let mut input = captured();
        input["workspace"]["neverSent"]["autoStartOperations"] = json!([]);
        if !stored {
            input["workspace"]["displayContext"]["projectionPending"]["autoStartOperations"] =
                json!([]);
        }
        input["workspace"]["base"]["autoStartBreaks"] = json!(true);
        input["intent"] = json!({"kind":"setAutoStart", "enabled":false});
        let output = call("workspace.intent.v1", &input);
        assert_eq!(output["projection"]["autoStartBreaks"], !stored);
        assert_eq!(
            output["groupOutcomes"]["autoStartOperations"][0]["outcome"],
            if stored { "applied" } else { "queued" }
        );
        assert_eq!(
            output["workspace"]["neverSent"]["autoStartOperations"],
            json!([output["operations"]["autoStartOperations"][0]["id"]])
        );
        assert_eq!(
            output["workspace"]["local"]["autoStartOperations"][0],
            input["workspace"]["local"]["autoStartOperations"][0]
        );
    }
}

#[test]
fn head_gate_and_raw_membership_do_not_fabricate_proof() {
    for counter in [10, 11, 12] {
        let mut input = workspace();
        input["canonicalHead"] = json!({"wallMs":1784548800000_i64,"counter":counter});
        let output = call("workspace.project.v1", &input);
        assert_eq!(
            output["displayContext"]["projectionPending"],
            if counter < 11 {
                input["local"].clone()
            } else {
                empty()
            }
        );
    }
    let mut input = workspace();
    input["displayContext"]["projectionPending"] = input["local"].clone();
    input["neverSent"] = json!({});
    input["canonicalHead"] = json!({"wallMs":1784548800000_i64,"counter":12});
    let output = call("workspace.project.v1", &input);
    assert_eq!(output["displayContext"], input["displayContext"]);
    assert_eq!(output["projectionPending"], empty());
    input["displayContext"]["projectionPending"]["durationOperations"][0]["extension"] =
        json!(true);
    assert_eq!(
        serde_json::from_str::<Value>(&dispatch_envelope_json(
            "workspace.project.v1",
            &input.to_string()
        ))
        .unwrap()["ok"],
        false
    );
}

#[test]
fn absent_context_and_null_legacy_context_keep_existing_contracts() {
    let mut input = workspace();
    input.as_object_mut().unwrap().remove("displayContext");
    let output = call("workspace.project.v1", &input);
    assert_eq!(output["projectionPending"], empty());
    assert_eq!(output["workspace"]["durationsMs"]["focus"], 1500000);
    input["displayContext"] = json!({"profile":"pwaStorage", "projectionPending":null});
    input["neverSent"] = json!({});
    let output = call("workspace.project.v1", &input);
    assert_eq!(
        output["displayContext"]["projectionPending"],
        input["local"]
    );
    assert_eq!(output["projectionPending"], empty());
}
