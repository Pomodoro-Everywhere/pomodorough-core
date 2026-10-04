//! Migration-only decoding keeps JSON numbers correctly rounded before coercion.
use std::collections::BTreeMap;

use serde_json::{Number, Value, value::RawValue};

use super::{CoreError, invalid};

pub(crate) fn parse(input: &str) -> Result<Value, CoreError> {
    // Preserve the established syntax, duplicate-key, depth, and range errors.
    // Do not use this validator's approximate floating-point values as input.
    crate::strict_json::parse(input)?;
    decode(serde_json::from_str::<&RawValue>(input)?.get())
}

fn decode(raw: &str) -> Result<Value, CoreError> {
    match raw.as_bytes()[0] {
        b'{' => {
            let fields: BTreeMap<String, &RawValue> = serde_json::from_str(raw)?;
            let mut object = serde_json::Map::new();
            for (name, value) in fields {
                object.insert(name, decode(value.get())?);
            }
            Ok(Value::Object(object))
        }
        b'[' => {
            let elements: Vec<&RawValue> = serde_json::from_str(raw)?;
            elements
                .into_iter()
                .map(|value| decode(value.get()))
                .collect()
        }
        b'-' | b'0'..=b'9' => number(raw).map(Value::Number),
        _ => Ok(serde_json::from_str(raw)?),
    }
}

fn number(raw: &str) -> Result<Number, CoreError> {
    if !raw.contains(['.', 'e', 'E']) && raw != "-0" {
        if let Ok(value) = raw.parse::<i64>() {
            return Ok(value.into());
        }
        if let Ok(value) = raw.parse::<u64>() {
            return Ok(value.into());
        }
    }
    let value = raw
        .parse::<f64>()
        .map_err(|_| invalid("invalid legacy JSON number"))?;
    Number::from_f64(value).ok_or_else(|| invalid("legacy JSON number is not finite"))
}
