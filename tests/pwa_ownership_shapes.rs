use serde_json::Value;

fn assert_rejection(name: &str) {
    let seed: Value =
        serde_json::from_str(include_str!("../fixtures/pwa-ownership-plan-v1.json")).unwrap();
    let shapes: Value =
        serde_json::from_str(include_str!("../fixtures/pwa-ownership-shapes-v1.json")).unwrap();
    let case = shapes["rejections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap();
    let mut input = seed["request"].clone();
    let keys: Vec<_> = case["path"].as_str().unwrap().split('.').collect();
    let parent = keys[..keys.len() - 1]
        .iter()
        .fold(&mut input, |value, key| &mut value[*key]);
    if let Some(value) = case.get("value") {
        parent[keys[keys.len() - 1]] = value.clone();
    } else {
        parent.as_object_mut().unwrap().remove(keys[keys.len() - 1]);
    }
    let raw =
        pomodorough_core::dispatch_envelope_json("workspace.ownershipPlan.v1", &input.to_string());
    let envelope: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(envelope["ok"], false, "{name}: {raw}");
    assert!(envelope.get("value").is_none(), "{name}: {raw}");
}

#[test]
fn raw_owner_full_tuple_is_not_a_record() {
    assert_rejection("owner-full-tuple");
}

#[test]
fn raw_owner_short_tuple_is_not_a_legacy_record() {
    assert_rejection("owner-short-tuple");
}

#[test]
fn raw_clock_tuple_is_not_a_record() {
    assert_rejection("clock-tuple");
}

#[test]
fn raw_profile_object_is_not_a_string_enum() {
    assert_rejection("profile-object");
}

#[test]
fn raw_install_rejects_unit_variant_policy_controls() {
    assert_rejection("install-controls");
}

#[test]
fn raw_display_profile_object_is_not_a_string_enum() {
    assert_rejection("display-profile-object");
}

#[test]
fn raw_workspace_never_sent_is_required() {
    assert_rejection("missing-never-sent");
}
