//! Raw sampled clock observations. No OS reads or HLC transitions occur here.
mod android;
mod apple;
mod desktop;
mod model;
mod observations;
mod pwa;

use crate::CoreError;
use model::*;
use serde_json::Value;

pub(crate) fn observe_json(input: &str) -> Result<String, CoreError> {
    crate::strict_json::parse(input)?;
    observations::validate(input)?;
    let input: Input = serde_json::from_str(input)?;
    if input.schema_version != 1 {
        return Err(invalid("unsupported clock schemaVersion"));
    }
    if (input.action == Action::Sample
        || input.action == Action::Advance && input.compatibility == Profile::Android)
        != input.server.is_some()
    {
        return Err(invalid("server sample does not match clock action"));
    }
    let mut output = match input.compatibility {
        Profile::Apple => apple::observe(&input)?,
        Profile::Android => android::observe(&input)?,
        Profile::Desktop => desktop::observe(&input)?,
        Profile::Pwa => pwa::observe(&input)?,
    };
    output["schemaVersion"] = Value::from(1);
    output["compatibility"] = serde_json::to_value(input.compatibility)?;
    Ok(serde_json::to_string(&output)?)
}

fn invalid(reason: &str) -> CoreError {
    CoreError::InvalidInput(format!("clock.observe.v1: {reason}"))
}

fn integer(value: i64, minimum: i64) -> Result<i64, CoreError> {
    if !(minimum..=SAFE).contains(&value) {
        return Err(invalid("integer outside supported range"));
    }
    Ok(value)
}

fn signed(value: i64) -> Result<i64, CoreError> {
    if !(-SAFE..=SAFE).contains(&value) {
        return Err(invalid("offset outside supported range"));
    }
    Ok(value)
}

fn floating_integer(value: f64, minimum: i64) -> Result<i64, CoreError> {
    if !value.is_finite() || value.fract() != 0.0 || value > SAFE as f64 {
        return Err(invalid("reading must be a safe integer"));
    }
    integer(value as i64, minimum)
}

fn uncertainty(value: i64) -> Result<i64, CoreError> {
    if !(0..=UNCERTAINTY).contains(&value) {
        return Err(invalid("server sample uncertainty exceeds 30000ms"));
    }
    Ok(value)
}

fn server(input: &Input) -> Result<&Server, CoreError> {
    let server = input
        .server
        .as_ref()
        .ok_or_else(|| invalid("missing server sample"))?;
    integer(server.server_time_ms, 1)?;
    if let Some(wall) = server.request_wall_ms {
        integer(wall, 1)?;
    }
    if let Some(wall) = server.server_hlc_wall_ms {
        integer(wall, 1)?;
        if (wall - server.server_time_ms).abs() > SKEW {
            return Err(invalid("server HLC disagrees with server time"));
        }
    }
    Ok(server)
}

fn monotonic(input: &Input) -> Result<i64, CoreError> {
    floating_integer(
        input
            .reading
            .monotonic_ms
            .ok_or_else(|| invalid("missing monotonic reading"))?,
        0,
    )
}

fn request_wall(server: &Server) -> Result<i64, CoreError> {
    integer(
        server
            .request_wall_ms
            .ok_or_else(|| invalid("missing request wall"))?,
        1,
    )
}

fn receipt(server: &Server) -> Result<(i64, i64, i64), CoreError> {
    let received = integer(
        server
            .response_wall_ms
            .ok_or_else(|| invalid("missing response wall"))?,
        1,
    )?;
    let sent = floating_integer(
        server
            .request_monotonic_ms
            .ok_or_else(|| invalid("missing request monotonic"))?,
        0,
    )?;
    let end = floating_integer(
        server
            .response_monotonic_ms
            .ok_or_else(|| invalid("missing response monotonic"))?,
        0,
    )?;
    if end < sent {
        return Err(invalid("response monotonic time moved backwards"));
    }
    Ok((received, sent, end))
}

fn physical_anchor(input: &Input, offset: Option<i64>) -> Result<Option<i64>, CoreError> {
    let Some(anchor) = input.trusted_anchor_ms else {
        return Ok(None);
    };
    integer(anchor, 1)?;
    // Android Instant translation accepts pre-epoch and non-wire-range dates.
    Ok(Some(anchor - offset.unwrap_or(0)))
}
