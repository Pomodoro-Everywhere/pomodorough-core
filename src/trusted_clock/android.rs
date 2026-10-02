use super::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Sample {
    offset_ms: i64,
    uncertainty_ms: i64,
    server_time_ms: i64,
    midpoint_physical_ms: i64,
    midpoint_elapsed_realtime_ms: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Anchor {
    server_time_ms: i64,
    elapsed_realtime_ms: i64,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    server_clock_offset_ms: Option<i64>,
    server_clock_uncertainty_ms: Option<i64>,
    server_clock_sample_physical_ms: Option<i64>,
    server_clock_sample_elapsed_realtime_ms: Option<i64>,
    server_clock_boot_id: Option<String>,
    #[serde(default)]
    retained_wall_ms: i64,
    anchor: Option<Anchor>,
    request_sample: Option<Sample>,
}

pub(super) fn observe(input: &Input) -> Result<Value, CoreError> {
    let mut state: State = serde_json::from_str(input.state.get())?;
    validate_state(&state)?;
    let mut sampled = None;
    let now = match input.action {
        Action::Sample | Action::Advance => {
            sampled = Some(sample(input, &state)?);
            None
        }
        Action::Current => Some(current(input, &mut state)?),
        Action::Restore => {
            restore(input, &mut state)?;
            None
        }
    };
    let offset = sampled
        .as_ref()
        .map(|s| s.offset_ms)
        .or(state.server_clock_offset_ms);
    let stale = sample_stale(input, sampled.as_ref().or(state.request_sample.as_ref()))?;
    Ok(json!({"state":state, "sample":sampled, "trustedNowMs":now,
        "sampleStale":stale, "physicalDeltaMs":offset.map(|v| -v),
        "physicalAnchorMs":physical_anchor(input, offset)?}))
}

fn validate_state(state: &State) -> Result<(), CoreError> {
    if state.server_clock_offset_ms.is_none() != state.server_clock_uncertainty_ms.is_none()
        || state.server_clock_sample_physical_ms.is_none()
            != state.server_clock_sample_elapsed_realtime_ms.is_none()
        || state.server_clock_sample_physical_ms.is_some() && state.server_clock_offset_ms.is_none()
        || state.server_clock_boot_id.is_some()
            && state.server_clock_sample_elapsed_realtime_ms.is_none()
    {
        return Err(invalid("incomplete Android persisted sample"));
    }
    integer(state.retained_wall_ms, 0)?;
    if let Some(value) = state.server_clock_offset_ms {
        signed(value)?;
    }
    if let Some(value) = state.server_clock_uncertainty_ms {
        uncertainty(value)?;
    }
    if let Some(value) = state.server_clock_sample_physical_ms {
        integer(value, 1)?;
    }
    if let Some(value) = state.server_clock_sample_elapsed_realtime_ms {
        integer(value, 0)?;
    }
    if let Some(anchor) = &state.anchor {
        integer(anchor.server_time_ms, 0)?;
        integer(anchor.elapsed_realtime_ms, 0)?;
    }
    if let Some(sample) = &state.request_sample {
        signed(sample.offset_ms)?;
        uncertainty(sample.uncertainty_ms)?;
        integer(sample.server_time_ms, 1)?;
        integer(sample.midpoint_physical_ms, 0)?;
        integer(sample.midpoint_elapsed_realtime_ms, 0)?;
    }
    Ok(())
}

fn sample_stale(input: &Input, sample: Option<&Sample>) -> Result<Option<bool>, CoreError> {
    let Some(sample) = sample else {
        return Ok(None);
    };
    if input.reading.monotonic_ms.is_none() {
        return Ok(None);
    }
    let elapsed = monotonic(input)?;
    Ok(Some(
        elapsed < sample.midpoint_elapsed_realtime_ms
            || elapsed - sample.midpoint_elapsed_realtime_ms > SKEW - sample.uncertainty_ms,
    ))
}

fn sample(input: &Input, state: &State) -> Result<Sample, CoreError> {
    let server = server(input)?;
    if server.server_hlc_wall_ms.is_none() {
        return Err(invalid("Android sample requires server HLC wall"));
    }
    let (received, sent, end) = receipt(server)?;
    let trip = end - sent;
    let request = request_wall(server)?;
    let physical_delta = received - request;
    let uncertainty_ms = uncertainty((trip + 1) / 2 + (physical_delta - trip).abs())?;
    if input.action == Action::Advance {
        let previous = state
            .request_sample
            .as_ref()
            .ok_or_else(|| invalid("advance requires requestSample"))?;
        if end < previous.midpoint_elapsed_realtime_ms {
            return Err(invalid("advance moved backwards"));
        }
        let advanced = integer(
            previous.server_time_ms + end - previous.midpoint_elapsed_realtime_ms,
            0,
        )?;
        return Ok(Sample {
            offset_ms: signed(advanced - received)?,
            uncertainty_ms: previous.uncertainty_ms.max(uncertainty_ms),
            server_time_ms: advanced,
            midpoint_physical_ms: received,
            midpoint_elapsed_realtime_ms: end,
        });
    }
    let midpoint = integer(request + physical_delta / 2, 0)?;
    Ok(Sample {
        offset_ms: signed(server.server_time_ms - midpoint)?,
        uncertainty_ms,
        server_time_ms: server.server_time_ms,
        midpoint_physical_ms: midpoint,
        midpoint_elapsed_realtime_ms: integer(sent + trip / 2, 0)?,
    })
}

fn current(input: &Input, state: &mut State) -> Result<i64, CoreError> {
    let elapsed = monotonic(input)?;
    if let Some(sample) = &state.request_sample {
        if elapsed < sample.midpoint_elapsed_realtime_ms {
            return Err(invalid("elapsed moved backwards during request"));
        }
        return integer(
            sample.server_time_ms + elapsed - sample.midpoint_elapsed_realtime_ms,
            0,
        );
    }
    if let Some(anchor) = &state.anchor {
        if elapsed >= anchor.elapsed_realtime_ms {
            return integer(
                anchor.server_time_ms + elapsed - anchor.elapsed_realtime_ms,
                0,
            );
        }
    }
    let Some(offset) = state.server_clock_offset_ms else {
        return wall(input);
    };
    let continued = match (
        state.server_clock_sample_physical_ms,
        state.server_clock_sample_elapsed_realtime_ms,
    ) {
        (Some(physical), Some(saved)) if elapsed >= saved => {
            integer(physical + offset + elapsed - saved, 0)?
        }
        _ => recovered(input, state, offset)?,
    };
    state.anchor = Some(Anchor {
        server_time_ms: continued,
        elapsed_realtime_ms: elapsed,
    });
    Ok(continued)
}

fn recovered(input: &Input, state: &State, offset: i64) -> Result<i64, CoreError> {
    let candidate = integer(wall(input)? + offset, 1)?;
    let maximum =
        (state.retained_wall_ms + SKEW - state.server_clock_uncertainty_ms.unwrap_or(0)).min(SAFE);
    if candidate > maximum {
        return Err(invalid("Android reboot requires a fresh server sample"));
    }
    Ok(candidate.max(state.retained_wall_ms))
}

fn restore(input: &Input, state: &mut State) -> Result<(), CoreError> {
    if let Some(saved) = state.server_clock_sample_elapsed_realtime_ms {
        if state.server_clock_boot_id.is_some()
            && state.server_clock_boot_id == input.reading.boot_id
            && monotonic(input)? >= saved
        {
            return Ok(());
        }
        state.server_clock_sample_physical_ms = None;
        state.server_clock_sample_elapsed_realtime_ms = None;
        state.server_clock_boot_id = None;
    }
    Ok(())
}

fn wall(input: &Input) -> Result<i64, CoreError> {
    integer(
        input
            .reading
            .wall_ms
            .ok_or_else(|| invalid("missing physical reading"))?,
        0,
    )
}
