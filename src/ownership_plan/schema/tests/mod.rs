use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::strict_json::shape::{Presence, Shape};

#[test]
fn required_negative_fixture_manifest_matches_every_concrete_schema_field() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/pwa-ownership-shapes-v1.json"
    ))
    .unwrap();
    let mut fields = BTreeMap::new();
    let mut closed = Vec::new();
    manifest(&super::REQUEST, "", true, &mut fields, &mut closed);
    let expected: BTreeMap<String, Value> = fixture["fieldShapes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| (field["path"].as_str().unwrap().into(), field.clone()))
        .collect();
    assert_eq!(
        expected.len(),
        fixture["fieldShapes"].as_array().unwrap().len(),
        "duplicate fixture field"
    );
    assert_eq!(
        fields, expected,
        "new or changed schema fields require negative fixture coverage"
    );
    closed.sort();
    let mut expected: Vec<_> = fixture["closedObjects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap().to_owned())
        .collect();
    expected.sort();
    assert_eq!(
        closed, expected,
        "closed records require unknown-null-control fixtures"
    );
}

#[test]
fn release_schema_keeps_every_raw_field_and_has_only_local_wall_clock() {
    let mut original = BTreeMap::new();
    let mut original_closed = Vec::new();
    manifest(
        &super::REQUEST,
        "",
        true,
        &mut original,
        &mut original_closed,
    );
    original.remove("clock.leaseDurationMs");
    let mut release = BTreeMap::new();
    let mut release_closed = Vec::new();
    manifest(
        &super::RELEASE_REQUEST,
        "",
        true,
        &mut release,
        &mut release_closed,
    );
    assert_eq!(release, original);
    assert_eq!(release_closed, original_closed);
    let Shape::TaggedObject { variants, .. } = super::request(Shape::Object)[1].shape else {
        panic!("ownership action must be tagged");
    };
    let empty: Vec<_> = variants
        .iter()
        .filter(|variant| variant.fields.is_empty())
        .map(|variant| variant.name)
        .collect();
    assert_eq!(empty, ["install", "release"]);
}

fn manifest(
    shape: &Shape,
    path: &str,
    required: bool,
    fields: &mut BTreeMap<String, Value>,
    closed: &mut Vec<String>,
) {
    fields.insert(
        path.into(),
        json!({"path": path, "shape": name(shape), "required": required}),
    );
    let concrete = match shape {
        Shape::Nullable(inner) => inner,
        shape => shape,
    };
    match concrete {
        Shape::Record(records) | Shape::Fields(records) => {
            closed.push(path.into());
            for field in *records {
                manifest(
                    &field.shape,
                    &child(path, field.name),
                    matches!(field.presence, Presence::Required),
                    fields,
                    closed,
                );
            }
        }
        Shape::TaggedObject { tag, variants } => {
            closed.push(path.into());
            fields.insert(
                child(path, tag),
                json!({"path":child(path, tag), "shape":"stringEnum", "required":true}),
            );
            for variant in *variants {
                for field in variant.fields {
                    manifest(
                        &field.shape,
                        &child(path, field.name),
                        matches!(field.presence, Presence::Required),
                        fields,
                        closed,
                    );
                }
            }
        }
        _ => {}
    }
}

fn name(shape: &Shape) -> String {
    match shape {
        Shape::Object | Shape::Record(_) | Shape::Fields(_) | Shape::TaggedObject { .. } => {
            "object".into()
        }
        Shape::Scalar => "scalar".into(),
        Shape::String => "string".into(),
        Shape::Integer => "integer".into(),
        Shape::Boolean => "boolean".into(),
        Shape::StringEnum(_) => "stringEnum".into(),
        Shape::Array(inner) => format!("{}Array", name(inner)),
        Shape::Nullable(inner) => {
            let name = name(inner);
            format!("nullable{}{}", name[..1].to_ascii_uppercase(), &name[1..])
        }
    }
}

fn child(path: &str, field: &str) -> String {
    if path.is_empty() {
        field.into()
    } else {
        format!("{path}.{field}")
    }
}
