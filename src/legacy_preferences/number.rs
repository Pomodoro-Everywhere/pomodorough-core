//! The frozen PWA importer uses Number, nonfinite-to-one, clamp, then Math.round.
use serde_json::Value;

use super::{CoreError, invalid};

pub(super) fn minutes(value: &Value) -> Result<i64, CoreError> {
    let number = match value {
        Value::Number(number) => number.as_f64().unwrap_or(f64::NAN),
        Value::Bool(enabled) => {
            if *enabled {
                1.0
            } else {
                0.0
            }
        }
        Value::Null => 0.0,
        Value::String(text) => numeric_string(text),
        Value::Array(_) => numeric_string(&array_string(value)?),
        Value::Object(object) => {
            // A JSON property named toString shadows Object.prototype.toString.
            if object.contains_key("toString") {
                return Err(invalid("legacy duration cannot convert object to number"));
            }
            f64::NAN
        }
    };
    let clamped = if number.is_finite() {
        number.clamp(1.0, 180.0)
    } else {
        1.0
    };
    Ok((clamped + 0.5).floor() as i64)
}

fn array_string(value: &Value) -> Result<String, CoreError> {
    Ok(match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(values) => values
            .iter()
            .map(array_string)
            .collect::<Result<Vec<_>, _>>()?
            .join(","),
        Value::Object(object) => {
            if object.contains_key("toString") {
                return Err(invalid("legacy duration cannot convert object to number"));
            }
            "[object Object]".into()
        }
        _ => value.to_string(),
    })
}

fn numeric_string(text: &str) -> f64 {
    let text = text.trim_matches(|ch: char| {
        matches!(ch,
        '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' |
        '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
    });
    if text.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0b", 2),
        ("0B", 2),
        ("0o", 8),
        ("0O", 8),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            return radix_number(digits, radix);
        }
    }
    if !decimal(text.as_bytes()) {
        return f64::NAN;
    }
    text.parse().unwrap_or(f64::NAN)
}

fn radix_number(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut value = 0.0;
    for digit in digits.chars() {
        let Some(digit) = digit.to_digit(radix) else {
            return f64::NAN;
        };
        value = value * f64::from(radix) + f64::from(digit);
    }
    value
}

fn decimal(bytes: &[u8]) -> bool {
    let mut index = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let whole = digits(bytes, &mut index);
    let fraction = if bytes.get(index) == Some(&b'.') {
        index += 1;
        digits(bytes, &mut index)
    } else {
        0
    };
    if whole + fraction == 0 {
        return false;
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        index += usize::from(matches!(bytes.get(index), Some(b'+' | b'-')));
        if digits(bytes, &mut index) == 0 {
            return false;
        }
    }
    index == bytes.len()
}

fn digits(bytes: &[u8], index: &mut usize) -> usize {
    let start = *index;
    while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
        *index += 1;
    }
    *index - start
}
