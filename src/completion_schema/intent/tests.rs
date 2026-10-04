use super::*;
use crate::strict_json::shape::Presence;
use serde_json::{Value, json};

#[test]
fn mandatory_nullable_and_enum_metadata_matches_hosted_negative_contract() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/pwa-selection-contract-v1.json"
    ))
    .unwrap();
    let actual = json!({"fields": fields(&REQUEST), "selectionFields": fields(&SELECTION),
        "actions": ACTIONS.iter().map(|variant| (variant.name.to_string(), describe(variant.fields))).collect::<serde_json::Map<_, _>>()});
    assert_eq!(
        actual, fixture,
        "Changing presence, nullable fields, or raw enums requires exact hosted cases."
    );
}

fn fields(shape: &Shape) -> Value {
    let Shape::Record(fields) = shape else {
        panic!("concrete record")
    };
    describe(fields)
}

fn describe(fields: &[Field]) -> Value {
    fields.iter().map(|field| (field.name.to_string(), json!({
        "required": matches!(field.presence, Presence::Required), "shape": name(&field.shape)
    }))).collect()
}

fn name(shape: &Shape) -> String {
    match shape {
        Shape::Object | Shape::Fields(_) | Shape::Record(_) => "object".into(),
        Shape::String => "string".into(),
        Shape::Scalar => "scalar".into(),
        Shape::Boolean => "boolean".into(),
        Shape::Integer => "integer".into(),
        Shape::StringEnum(variants) => format!("enum:{}", variants.join(",")),
        Shape::Nullable(inner) => format!("nullable:{}", name(inner)),
        Shape::Array(inner) => format!("array:{}", name(inner)),
        Shape::TaggedObject { tag, .. } => format!("tagged:{tag}"),
    }
}
