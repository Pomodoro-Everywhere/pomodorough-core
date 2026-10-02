use super::*;
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Sample {
    offset_ms: i64,
    uncertainty_ms: i64,
    acquired_physical_ms: i64,
    acquired_monotonic_ms: i64,
    acquired_trusted_ms: i64,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum Mode {
    Local,
    Wall,
    #[default]
    Monotonic,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    sample: Value,
    anchor: Option<Sample>,
    #[serde(default)]
    mode: Mode,
}

pub(super) fn observe(input: &Input) -> Result<Value, CoreError> {
    let mut state: State = serde_json::from_str(input.state.get())?;
    let mut response_sample = None;
    let mut trusted_response = None;
    let now = match input.action {
        Action::Sample => {
            let (sample, response_time) = sample(input)?;
            if let Some(sample) = &sample {
                state.sample = serde_json::to_value(sample)?;
                state.anchor = Some(sample.clone());
            }
            response_sample = sample;
            trusted_response = Some(response_time);
            None
        }
        Action::Restore => {
            restore(input, &mut state)?;
            None
        }
        Action::Current => Some(current(input, &mut state)?),
        Action::Advance => return Err(invalid("Desktop advance is not supported")),
    };
    let parsed = if matches!(state.mode, Mode::Local) && input.trusted_anchor_ms.is_none() {
        // A local-only read deliberately bypasses malformed trusted persistence.
        parsed(&state.sample).unwrap_or(None)
    } else {
        parsed(&state.sample)?
    };
    let offset = parsed.map(|s| s.offset_ms);
    Ok(
        json!({"state":state, "trustedNowMs":now, "sample":response_sample,
        "trustedResponseMs":trusted_response, "physicalDeltaMs":offset.map(|v| -v),
        "physicalAnchorMs":physical(input, offset)?}),
    )
}

fn parsed(value: &Value) -> Result<Option<Sample>, CoreError> {
    if value.is_null() {
        return Ok(None);
    }
    let sample: Sample = serde_json::from_value(value.clone())?;
    signed(sample.offset_ms)?;
    uncertainty(sample.uncertainty_ms)?;
    integer(sample.acquired_physical_ms, 1)?;
    integer(sample.acquired_monotonic_ms, 0)?;
    integer(sample.acquired_trusted_ms, 1)?;
    Ok(Some(sample))
}

fn sample(input: &Input) -> Result<(Option<Sample>, i64), CoreError> {
    let sample = server(input)?;
    let missing = [
        sample.request_wall_ms.is_none(),
        sample.response_wall_ms.is_none(),
        sample.request_monotonic_ms.is_none(),
        sample.response_monotonic_ms.is_none(),
    ];
    if missing.iter().all(|missing| *missing) {
        return Ok((None, sample.server_time_ms));
    }
    if missing.iter().any(|missing| *missing) {
        return Err(invalid("incomplete Desktop response timing"));
    }
    let request = request_wall(sample)?;
    let (received, sent, end) = receipt(sample)?;
    let trip = end - sent;
    let physical_trip = received - request;
    let disagreement = (physical_trip - trip).abs();
    if physical_trip < 0 || disagreement > DRIFT {
        return Err(invalid("Desktop response clocks disagree"));
    }
    let half = trip / 2;
    let midpoint = integer(request + half, 1)?;
    let trusted_response = integer(sample.server_time_ms + trip - half, 1)?;
    Ok((
        Some(Sample {
            offset_ms: signed(sample.server_time_ms - midpoint)?,
            uncertainty_ms: uncertainty((trip + 1) / 2 + disagreement)?,
            acquired_physical_ms: received,
            acquired_monotonic_ms: end,
            acquired_trusted_ms: trusted_response,
        }),
        trusted_response,
    ))
}

fn physical(input: &Input, offset: Option<i64>) -> Result<Option<i64>, CoreError> {
    let Some(anchor) = input.trusted_anchor_ms else {
        return Ok(None);
    };
    integer(anchor, 1)?;
    let Some(offset) = offset else {
        return Ok(Some(anchor));
    };
    let translated = anchor - offset;
    let physical = integer(translated, 1).ok().and_then(formatted_milliseconds);
    // The Python mapper keeps the original string if its formatter cannot
    // represent the translated instant, including dates beyond year 9999.
    Ok(Some(physical.unwrap_or(anchor)))
}

fn formatted_milliseconds(milliseconds: i64) -> Option<i64> {
    let epoch = milliseconds as f64 / 1000.0;
    let whole = epoch.floor() as i64;
    let micros = ((epoch - whole as f64) * 1_000_000.0).round_ties_even() as i64;
    let whole = whole + micros / 1_000_000;
    let micros = micros % 1_000_000;
    let date = chrono::DateTime::from_timestamp(whole, micros as u32 * 1000)?;
    if !(1..=9999).contains(&date.year()) {
        return None;
    }
    // utc_timestamp truncates to milliseconds, then parse_timestamp_ms uses
    // Python's floating Unix seconds before truncating back to milliseconds.
    let fraction = (micros / 1000 * 1000) as f64 / 1_000_000.0;
    floating_integer(((whole as f64 + fraction) * 1000.0).trunc(), 1).ok()
}

fn projected(sample: &Sample, wall: i64, elapsed: i64) -> Result<Option<i64>, CoreError> {
    if elapsed < sample.acquired_monotonic_ms {
        return Ok(None);
    }
    let delta = elapsed - sample.acquired_monotonic_ms;
    let expected = sample.acquired_physical_ms + delta;
    if (wall - expected).abs() > DRIFT {
        return Ok(None);
    }
    Ok(Some(integer(sample.acquired_trusted_ms + delta, 1)?))
}

fn restore(input: &Input, state: &mut State) -> Result<(), CoreError> {
    // Startup discards invalid persistence, but a live read rejects it.
    let sample = match parsed(&state.sample) {
        Ok(Some(sample)) => sample,
        Ok(None) => return Ok(()),
        Err(_) => {
            state.sample = Value::Null;
            return Ok(());
        }
    };
    if projected(&sample, wall(input)?, monotonic(input)?)?.is_none() {
        state.sample = Value::Null;
    } else {
        state.anchor = Some(sample)
    }
    Ok(())
}

fn current(input: &Input, state: &mut State) -> Result<i64, CoreError> {
    let wall = wall(input)?;
    if matches!(state.mode, Mode::Local) {
        return Ok(wall);
    }
    let Some(sample) = parsed(&state.sample)? else {
        return Ok(wall);
    };
    if matches!(state.mode, Mode::Wall) {
        return integer(wall + sample.offset_ms, 1);
    }
    let elapsed = monotonic(input)?;
    if state.anchor.as_ref() != Some(&sample) {
        let Some(now) = projected(&sample, wall, elapsed)? else {
            return Ok(wall);
        };
        state.anchor = Some(sample);
        return Ok(now);
    }
    if elapsed < sample.acquired_monotonic_ms {
        return Ok(wall);
    }
    integer(
        sample.acquired_trusted_ms + elapsed - sample.acquired_monotonic_ms,
        1,
    )
}

fn wall(input: &Input) -> Result<i64, CoreError> {
    integer(
        input
            .reading
            .wall_ms
            .ok_or_else(|| invalid("missing physical reading"))?,
        1,
    )
}
