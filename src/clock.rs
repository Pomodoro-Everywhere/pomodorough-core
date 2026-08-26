use serde::{Deserialize, Serialize};

use crate::CoreError;

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const MAX_UUID_TIMESTAMP: i64 = (1_i64 << 48) - 1;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Hlc {
    wall_ms: i64,
    counter: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HlcTickInput {
    local: Hlc,
    #[serde(default)]
    remote: Option<Hlc>,
    physical_now_ms: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Ord, PartialOrd, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HlcHeadClock {
    wall_ms: i64,
    counter: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HlcHeadInput {
    physical_now_ms: i64,
    observed: Vec<HlcHeadClock>,
}

pub(crate) fn head_json(input: &str) -> Result<String, CoreError> {
    let input: HlcHeadInput = serde_json::from_str(input)?;
    validate_safe("physicalNowMs", input.physical_now_ms)?;
    let mut head = HlcHeadClock {
        wall_ms: input.physical_now_ms,
        counter: 0,
    };
    for clock in input.observed {
        validate_safe("wallMs", clock.wall_ms)?;
        validate_safe("counter", clock.counter)?;
        head = head.max(clock);
    }
    Ok(serde_json::to_string(&head)?)
}

pub(crate) fn tick_json(input: &str) -> Result<String, CoreError> {
    let input: HlcTickInput = serde_json::from_str(input)?;
    validate_hlc(input.local)?;
    if let Some(remote) = input.remote {
        validate_hlc(remote)?;
    }
    validate_safe("physicalNowMs", input.physical_now_ms)?;

    let remote_wall = input.remote.map_or(0, |clock| clock.wall_ms);
    let wall_ms = input
        .local
        .wall_ms
        .max(remote_wall)
        .max(input.physical_now_ms);
    let counter = match input.remote {
        Some(remote) if wall_ms == input.local.wall_ms && wall_ms == remote.wall_ms => {
            increment(input.local.counter.max(remote.counter))?
        }
        _ if wall_ms == input.local.wall_ms => increment(input.local.counter)?,
        Some(remote) if wall_ms == remote.wall_ms => increment(remote.counter)?,
        _ => 0,
    };
    Ok(serde_json::to_string(&Hlc { wall_ms, counter })?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UuidV7Input {
    timestamp_ms: i64,
    random_value_hex: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UuidV7Output {
    uuid: String,
    timestamp_ms: i64,
}

pub(crate) fn uuid_v7_from_parts_json(input: &str) -> Result<String, CoreError> {
    let input: UuidV7Input = serde_json::from_str(input)?;
    if !(0..=MAX_UUID_TIMESTAMP).contains(&input.timestamp_ms) {
        return Err(CoreError::InvalidInput(
            "timestampMs is outside UUIDv7's 48-bit range".to_owned(),
        ));
    }
    if input.random_value_hex.is_empty() || input.random_value_hex.len() > 19 {
        return Err(CoreError::InvalidInput(
            "randomValueHex must encode exactly 74 bits".to_owned(),
        ));
    }
    let random = u128::from_str_radix(&input.random_value_hex, 16)
        .map_err(|_| CoreError::InvalidInput("randomValueHex is not hexadecimal".to_owned()))?;
    if random >= (1_u128 << 74) {
        return Err(CoreError::InvalidInput(
            "randomValueHex exceeds 74 bits".to_owned(),
        ));
    }

    let rand_a = (random >> 62) & 0x0fff;
    let rand_b = random & ((1_u128 << 62) - 1);
    let bits = ((input.timestamp_ms as u128) << 80)
        | (7_u128 << 76)
        | (rand_a << 64)
        | (2_u128 << 62)
        | rand_b;
    let hex = format!("{bits:032x}");
    let uuid = format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    );
    Ok(serde_json::to_string(&UuidV7Output {
        uuid,
        timestamp_ms: input.timestamp_ms,
    })?)
}

fn validate_hlc(clock: Hlc) -> Result<(), CoreError> {
    validate_safe("wallMs", clock.wall_ms)?;
    validate_safe("counter", clock.counter)
}

fn validate_safe(name: &str, value: i64) -> Result<(), CoreError> {
    if !(0..=MAX_SAFE_INTEGER).contains(&value) {
        return Err(CoreError::InvalidInput(format!(
            "{name} must be a non-negative JavaScript-safe integer"
        )));
    }
    Ok(())
}

fn increment(value: i64) -> Result<i64, CoreError> {
    let incremented = value
        .checked_add(1)
        .ok_or_else(|| CoreError::InvalidInput("HLC counter overflow".to_owned()))?;
    validate_safe("counter", incremented)?;
    Ok(incremented)
}
