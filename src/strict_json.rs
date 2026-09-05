use std::fmt;

use serde::de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

use crate::CoreError;

pub(crate) fn parse(input: &str) -> Result<Value, CoreError> {
    crate::check_input_len(input)?;
    let mut deserializer = serde_json::Deserializer::from_str(input);
    let value = StrictValue.deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

pub(crate) fn object<'a>(
    value: &'a Value,
    path: &str,
) -> Result<&'a Map<String, Value>, CoreError> {
    value
        .as_object()
        .ok_or_else(|| invalid_container(path, "object"))
}

pub(crate) fn object_field<'a>(
    parent: &'a Map<String, Value>,
    field: &str,
    path: &str,
) -> Result<&'a Map<String, Value>, CoreError> {
    let value = parent
        .get(field)
        .ok_or_else(|| invalid_container(path, "object"))?;
    object(value, path)
}

pub(crate) fn nullable_object_field<'a>(
    parent: &'a Map<String, Value>,
    field: &str,
    path: &str,
) -> Result<Option<&'a Map<String, Value>>, CoreError> {
    match parent.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => object(value, path).map(Some),
    }
}

pub(crate) fn object_array_field(
    parent: &Map<String, Value>,
    field: &str,
    path: &str,
    required: bool,
) -> Result<(), CoreError> {
    let Some(value) = parent.get(field) else {
        return if required {
            Err(invalid_container(path, "array"))
        } else {
            Ok(())
        };
    };
    let values = value
        .as_array()
        .ok_or_else(|| invalid_container(path, "array"))?;
    if values.iter().any(|value| !value.is_object()) {
        return Err(invalid_container(&format!("{path}[]"), "object"));
    }
    Ok(())
}

pub(crate) fn array_field(
    parent: &Map<String, Value>,
    field: &str,
    path: &str,
    required: bool,
) -> Result<(), CoreError> {
    match parent.get(field) {
        Some(Value::Array(_)) => Ok(()),
        Some(_) => Err(invalid_container(path, "array")),
        None if required => Err(invalid_container(path, "array")),
        None => Ok(()),
    }
}

fn invalid_container(path: &str, expected: &str) -> CoreError {
    CoreError::InvalidInput(format!("{path} must be a JSON {expected}"))
}

struct StrictValue;

impl<'de> DeserializeSeed<'de> for StrictValue {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("invalid JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(StrictValue)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(field) = map.next_key::<String>()? {
            if values.contains_key(&field) {
                return Err(A::Error::custom(format!("duplicate field `{field}`")));
            }
            values.insert(field, map.next_value_seed(StrictValue)?);
        }
        Ok(Value::Object(values))
    }
}
