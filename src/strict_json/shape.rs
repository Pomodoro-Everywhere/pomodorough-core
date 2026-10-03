//! Concrete JSON representations checked before Serde can accept other encodings.
use serde_json::{Map, Value};

use crate::CoreError;

#[derive(Clone, Copy)]
pub(crate) enum Shape {
    Object,
    String,
    Integer,
    Boolean,
    StringEnum(&'static [&'static str]),
    Record(&'static [Field]),
    TaggedObject {
        tag: &'static str,
        variants: &'static [Variant],
    },
    Array(&'static Shape),
    Nullable(&'static Shape),
}

#[derive(Clone, Copy)]
pub(crate) enum Presence {
    Required,
    Optional,
}

pub(crate) struct Field {
    pub(crate) name: &'static str,
    pub(crate) shape: Shape,
    pub(crate) presence: Presence,
}

pub(crate) struct Variant {
    pub(crate) name: &'static str,
    pub(crate) fields: &'static [Field],
}

impl Field {
    pub(crate) const fn required(name: &'static str, shape: Shape) -> Self {
        Self {
            name,
            shape,
            presence: Presence::Required,
        }
    }

    pub(crate) const fn optional(name: &'static str, shape: Shape) -> Self {
        Self {
            name,
            shape,
            presence: Presence::Optional,
        }
    }
}

pub(crate) fn validate(value: &Value, shape: &Shape, path: &str) -> Result<(), CoreError> {
    let (valid, expected) = match shape {
        Shape::Record(fields) => return record(super::object(value, path)?, fields, path, None),
        Shape::TaggedObject { tag, variants } => return tagged(value, tag, variants, path),
        Shape::Array(element) => return array(value, element, path),
        Shape::Nullable(inner) => {
            return if value.is_null() {
                Ok(())
            } else {
                validate(value, inner, path)
            };
        }
        Shape::Object => (value.is_object(), "object"),
        Shape::String => (value.is_string(), "string"),
        Shape::Integer => (value.as_i64().is_some(), "integer"),
        Shape::Boolean => (value.is_boolean(), "boolean"),
        Shape::StringEnum(variants) => (
            value
                .as_str()
                .is_some_and(|value| variants.contains(&value)),
            "string enum",
        ),
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(path, expected))
    }
}

fn array(value: &Value, element: &Shape, path: &str) -> Result<(), CoreError> {
    let values = value.as_array().ok_or_else(|| invalid(path, "array"))?;
    for value in values {
        validate(value, element, &format!("{path}[]"))?;
    }
    Ok(())
}

fn tagged(value: &Value, tag: &str, variants: &[Variant], path: &str) -> Result<(), CoreError> {
    let object = super::object(value, path)?;
    let name = object
        .get(tag)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&child(path, tag), "string enum"))?;
    let variant = variants
        .iter()
        .find(|variant| variant.name == name)
        .ok_or_else(|| invalid(&child(path, tag), "string enum"))?;
    record(object, variant.fields, path, Some(tag))
}

fn record(
    object: &Map<String, Value>,
    fields: &[Field],
    path: &str,
    tag: Option<&str>,
) -> Result<(), CoreError> {
    for key in object.keys() {
        if Some(key.as_str()) != tag && !fields.iter().any(|field| field.name == key) {
            return Err(CoreError::InvalidInput(format!(
                "unknown JSON field {}",
                child(path, key)
            )));
        }
    }
    for field in fields {
        let next = child(path, field.name);
        match object.get(field.name) {
            Some(value) => validate(value, &field.shape, &next)?,
            None if matches!(field.presence, Presence::Required) => {
                return Err(CoreError::InvalidInput(format!(
                    "missing required JSON field {next}"
                )));
            }
            None => {}
        }
    }
    Ok(())
}

fn child(path: &str, field: &str) -> String {
    if path.is_empty() {
        field.into()
    } else {
        format!("{path}.{field}")
    }
}

fn invalid(path: &str, expected: &str) -> CoreError {
    CoreError::InvalidInput(format!("{path} must be a JSON {expected}"))
}
