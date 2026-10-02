use super::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    offset_ms: Option<i64>,
    uncertainty_ms: Option<i64>,
    anchor_ms: Option<i64>,
    #[serde(default, deserialize_with = "exact_optional_float")]
    anchor_uptime: Option<f64>,
    last_emitted_ms: Option<i64>,
}

pub(super) fn observe(input: &Input) -> Result<Value, CoreError> {
    let mut state: State = serde_json::from_str(input.state.get())?;
    if let Some(last) = state.last_emitted_ms {
        integer(last, 1)?;
    }
    let now = match input.action {
        Action::Sample => {
            state = sample(input, state.last_emitted_ms)?;
            None
        }
        Action::Current | Action::Advance => Some(current(input, &state)?),
        Action::Restore => return Err(invalid("Apple restore is not supported; use current")),
    };
    // This optional bound check matches WireBounds.physicalMilliseconds(Date).
    let occurrence_wall =
        now.and_then(|value| floating_integer((date_seconds(value) * 1000.0).trunc(), 1).ok());
    if input.action == Action::Advance && state.offset_ms.is_some() {
        let recorded = occurrence_wall.ok_or_else(|| invalid("invalid Apple occurrence Date"))?;
        state.last_emitted_ms = Some(state.last_emitted_ms.unwrap_or(0).max(recorded));
    }
    let offset = physical_offset(&state)?;
    let physical = physical(input, offset)?;
    Ok(
        json!({"state":state, "trustedNowMs":now, "occurrenceWallMs":occurrence_wall,
        "trustedDateSeconds":now.map(date_seconds),
        "physicalDeltaMs":offset.map(|v| -v), "physicalAnchorMs":physical.and_then(anchor_milliseconds),
        "physicalAnchorSeconds":physical}),
    )
}

fn sample(input: &Input, last_emitted_ms: Option<i64>) -> Result<State, CoreError> {
    let sample = server(input)?;
    let sent = sample
        .request_uptime_seconds
        .ok_or_else(|| invalid("missing request uptime"))?;
    let end = sample
        .response_uptime_seconds
        .ok_or_else(|| invalid("missing response uptime"))?;
    if !sent.is_finite() || !end.is_finite() || sent < 0.0 || end < sent {
        return Err(invalid("invalid Apple response uptime"));
    }
    let half = (end - sent) * 500.0;
    let delta = floating_integer(half.trunc(), 0)?;
    let uncertainty_ms = uncertainty(floating_integer(half.ceil(), 0)?)?;
    // The native method bounds final offset and anchor, not its midpoint sum.
    let midpoint = request_wall(sample)?
        .checked_add(delta)
        .ok_or_else(|| invalid("Apple midpoint overflow"))?;
    Ok(State {
        offset_ms: Some(signed(sample.server_time_ms - midpoint)?),
        uncertainty_ms: Some(uncertainty_ms),
        anchor_ms: Some(integer(sample.server_time_ms + delta, 1)?),
        anchor_uptime: Some(end),
        last_emitted_ms,
    })
}

fn current(input: &Input, state: &State) -> Result<i64, CoreError> {
    let candidate = if state.offset_ms.is_none()
        && state.uncertainty_ms.is_none()
        && state.anchor_ms.is_none()
        && state.anchor_uptime.is_none()
        && state.last_emitted_ms.is_none()
    {
        wall(input)?
    } else {
        continued(input, state)?
    };
    match state.last_emitted_ms {
        Some(last) if candidate <= last => integer(
            last.checked_add(1)
                .ok_or_else(|| invalid("last emitted overflow"))?,
            1,
        ),
        _ => Ok(candidate),
    }
}

fn continued(input: &Input, state: &State) -> Result<i64, CoreError> {
    let offset = signed(
        state
            .offset_ms
            .ok_or_else(|| invalid("incomplete Apple sample"))?,
    )?;
    uncertainty(
        state
            .uncertainty_ms
            .ok_or_else(|| invalid("incomplete Apple sample"))?,
    )?;
    let anchor = integer(
        state
            .anchor_ms
            .ok_or_else(|| invalid("incomplete Apple sample"))?,
        1,
    )?;
    let saved = state
        .anchor_uptime
        .ok_or_else(|| invalid("incomplete Apple sample"))?;
    let uptime = input
        .reading
        .uptime_seconds
        .ok_or_else(|| invalid("missing uptime"))?;
    if !saved.is_finite() || saved < 0.0 || !uptime.is_finite() {
        return Err(invalid("invalid uptime"));
    }
    if uptime >= saved {
        return integer(
            anchor + floating_integer(((uptime - saved) * 1000.0).trunc(), 0)?,
            1,
        );
    }
    let recovered = integer(wall(input)? + offset, 1)?;
    if recovered > anchor.max(state.last_emitted_ms.unwrap_or(0)) + SKEW {
        return Err(invalid("Apple reboot requires a fresh server sample"));
    }
    Ok(recovered)
}

fn wall(input: &Input) -> Result<i64, CoreError> {
    let seconds = input
        .reading
        .wall_seconds
        .ok_or_else(|| invalid("missing Apple wall seconds"))?;
    floating_integer(
        (((seconds - 978_307_200.0) + 978_307_200.0) * 1000.0).trunc(),
        1,
    )
}

fn date_seconds(milliseconds: i64) -> f64 {
    // Foundation Date stores seconds relative to 2001, then adds the Unix epoch.
    (milliseconds as f64 / 1000.0 - 978_307_200.0) + 978_307_200.0
}

fn physical(input: &Input, offset: Option<i64>) -> Result<Option<f64>, CoreError> {
    let seconds = if let Some(seconds) = input.trusted_anchor_seconds {
        if !seconds.is_finite() {
            return Err(invalid("invalid Apple anchor Date"));
        }
        (seconds - 978_307_200.0) + 978_307_200.0
    } else if let Some(anchor) = input.trusted_anchor_ms {
        date_seconds(integer(anchor, 1)?)
    } else {
        return Ok(None);
    };
    let Some(offset) = offset else {
        return Ok(Some(seconds));
    };
    let observed = floating_integer((seconds * 1000.0).trunc(), 1)?;
    Ok(Some(date_seconds(integer(observed - offset, 1)?)))
}

fn anchor_milliseconds(seconds: f64) -> Option<i64> {
    let milliseconds = (seconds * 1000.0).round();
    (milliseconds.is_finite()
        && milliseconds >= i64::MIN as f64
        && milliseconds < -(i64::MIN as f64))
        .then_some(milliseconds as i64)
}

fn physical_offset(state: &State) -> Result<Option<i64>, CoreError> {
    if state.offset_ms.is_none() && state.uncertainty_ms.is_none() {
        return Ok(None);
    }
    uncertainty(
        state
            .uncertainty_ms
            .ok_or_else(|| invalid("incomplete physical clock sample"))?,
    )?;
    Ok(Some(signed(state.offset_ms.ok_or_else(|| {
        invalid("incomplete physical clock sample")
    })?)?))
}
