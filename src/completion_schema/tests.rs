use crate::strict_json::shape::Shape;
use serde::de::{DeserializeOwned, Deserializer, Visitor, value::Error};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

struct DecoderFields<'a>(&'a mut BTreeSet<&'static str>);

impl<'de> Deserializer<'de> for DecoderFields<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Error> {
        Err(serde::de::Error::custom("metadata only"))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, Error> {
        self.0.extend(fields.iter().copied());
        Err(serde::de::Error::custom("metadata only"))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        variants: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, Error> {
        self.0.extend(variants.iter().copied());
        Err(serde::de::Error::custom("metadata only"))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map identifier ignored_any
    }
}

pub(crate) fn assert_enum<T: DeserializeOwned>(shape: &Shape) {
    let mut variants = BTreeSet::new();
    let _ = T::deserialize(DecoderFields(&mut variants));
    let Shape::StringEnum(expected) = shape else {
        panic!("raw string enum")
    };
    assert!(!variants.is_empty(), "decoder must expose enum metadata");
    assert_eq!(
        variants,
        expected.iter().copied().collect(),
        "raw enum guards differ from typed variants"
    );
}

pub(crate) fn assert_fields<T: DeserializeOwned>(shape: &Shape, extensions: &[&'static str]) {
    let mut fields = BTreeSet::new();
    let _ = T::deserialize(DecoderFields(&mut fields));
    assert!(
        !fields.is_empty(),
        "decoder must expose concrete struct metadata"
    );
    fields.extend(extensions.iter().copied());
    let (Shape::Fields(schema) | Shape::Record(schema)) = shape else {
        panic!("expected representation fields")
    };
    let names: BTreeSet<_> = schema.iter().map(|field| field.name).collect();
    assert_eq!(names.len(), schema.len(), "duplicate representation field");
    assert_eq!(
        fields, names,
        "typed fields changed without representation guards"
    );
}

#[test]
fn structural_fixture_guards_cover_every_shared_schema_path() {
    let mut fields = BTreeMap::new();
    for (operation, shape) in [
        ("workspace.completionMutation.v1", &super::FINISH),
        ("workspace.readModel.v1", &super::READ),
        ("timer.completionState.v1", &super::INSTALL),
    ] {
        manifest(shape, operation, &mut fields);
    }
    let actual = json!({"fields": fields});
    if std::env::var_os("PWA12_PRINT_SHAPES").is_some() {
        println!("PWA12_SHAPES {actual}");
    }
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/pwa-completion-shapes-v1.json")).unwrap();
    let old: BTreeMap<_, _> = fields
        .iter()
        .filter(|(path, _)| !path.contains("/finishEvidence"))
        .map(|(path, shape)| (path.clone(), shape.clone()))
        .collect();
    assert_evidence_paths(&fields, &fixture);
    assert_eq!(
        json!({"fields": old}),
        fixture,
        "schema changes require permanent raw-path fixtures"
    );
    for (operation, shape) in [
        ("workspace.completionMutation.v1", &super::FINISH),
        ("workspace.readModel.v1", &super::READ),
        ("timer.completionState.v1", &super::INSTALL),
    ] {
        assert!(fields.keys().any(|path| path.starts_with(operation)));
        assert!(crate::strict_json::shape::validate(&json!([]), shape, "").is_err());
    }
}

fn assert_evidence_paths(fields: &BTreeMap<String, String>, old: &Value) {
    let contract: Value = serde_json::from_str(include_str!(
        "../../fixtures/pwa-finish-evidence-schema-v1.json"
    ))
    .unwrap();
    let mut expected = BTreeMap::new();
    for operation in [
        "workspace.completionMutation.v1",
        "workspace.readModel.v1",
        "timer.completionState.v1",
    ] {
        let base = format!("{operation}/lifecycle/finishEvidence");
        expected.insert(
            base.clone(),
            contract["collection"].as_str().unwrap().to_owned(),
        );
        expected.insert(
            format!("{base}/0"),
            contract["record"].as_str().unwrap().to_owned(),
        );
        for (name, record) in contract["fields"].as_object().unwrap() {
            let prefix = record["schemaPath"].as_str().unwrap();
            for (path, shape) in old["fields"]
                .as_object()
                .unwrap()
                .iter()
                .filter(|(path, _)| {
                    path.as_str() == prefix || path.starts_with(&format!("{prefix}/"))
                })
            {
                expected.insert(
                    format!("{base}/0/{name}{}", &path[prefix.len()..]),
                    shape.as_str().unwrap().to_owned(),
                );
            }
        }
    }
    let actual = fields
        .iter()
        .filter(|(path, _)| path.contains("/finishEvidence"))
        .map(|(path, shape)| (path.clone(), shape.clone()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        actual, expected,
        "new evidence paths need exact hosted guards"
    );
}

fn manifest(shape: &Shape, path: &str, fields: &mut BTreeMap<String, String>) {
    match shape {
        Shape::Fields(records) | Shape::Record(records) => {
            fields.insert(path.into(), "object".into());
            for field in *records {
                manifest(&field.shape, &format!("{path}/{}", field.name), fields);
            }
        }
        Shape::Nullable(inner) => {
            manifest(inner, path, fields);
            fields.get_mut(path).unwrap().insert_str(0, "nullable:");
        }
        Shape::Array(inner) => {
            fields.insert(path.into(), "array".into());
            manifest(inner, &format!("{path}/0"), fields);
        }
        Shape::Scalar => {
            fields.insert(path.into(), "scalar".into());
        }
        Shape::String => {
            fields.insert(path.into(), "string".into());
        }
        Shape::Object => {
            fields.insert(path.into(), "object".into());
        }
        _ => panic!("new representation needs negative fixtures"),
    }
}
