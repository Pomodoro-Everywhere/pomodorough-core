use super::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Sample {
    offset_ms: i64,
    uncertainty_ms: i64,
    sampled_at_wall_ms: i64,
    request_sequence: i64,
    received_at_wall_ms: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Runtime {
    identity: String,
    #[serde(deserialize_with = "exact_float")]
    monotonic_ms: f64,
    wall_ms: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    clock_offset: Value,
    #[serde(default)]
    minimum_wall_ms: i64,
    runtime: Option<Runtime>,
}

pub(super) fn observe(input: &Input) -> Result<Value, CoreError> {
    let mut state: State = serde_json::from_str(input.state.get())?;
    integer(state.minimum_wall_ms, 0)?;
    let now = match input.action {
        Action::Sample => {
            state.clock_offset = serde_json::to_value(sample(input)?)?;
            None
        }
        Action::Current => Some(current(input, &mut state)?),
        _ => return Err(invalid("PWA supports sample and current only")),
    };
    let anchor = input
        .trusted_anchor_ms
        .map(|value| integer(value, 1))
        .transpose()?;
    Ok(
        json!({"state":state, "trustedNowMs":now, "physicalDeltaMs":0,
        "physicalAnchorMs":anchor}),
    )
}

fn valid_sample(value: &Value) -> Option<Sample> {
    let sample: Sample = serde_json::from_value(value.clone()).ok()?;
    signed(sample.offset_ms).ok()?;
    uncertainty(sample.uncertainty_ms).ok()?;
    integer(sample.sampled_at_wall_ms, 1).ok()?;
    integer(sample.request_sequence, 1).ok()?;
    integer(sample.received_at_wall_ms, 1).ok()?;
    integer(sample.sampled_at_wall_ms + sample.offset_ms, 1).ok()?;
    (sample.received_at_wall_ms >= sample.sampled_at_wall_ms).then_some(sample)
}

fn sample(input: &Input) -> Result<Sample, CoreError> {
    let sample = server(input)?;
    let request = request_wall(sample)?;
    let end = integer(
        sample
            .response_wall_ms
            .ok_or_else(|| invalid("missing response wall"))?,
        1,
    )?;
    if end < request {
        return Err(invalid("PWA response wall moved backwards"));
    }
    let trip = end - request;
    let midpoint = request + trip / 2;
    Ok(Sample {
        offset_ms: signed(sample.server_time_ms - midpoint)?,
        uncertainty_ms: uncertainty((trip + 1) / 2)?,
        sampled_at_wall_ms: midpoint,
        request_sequence: integer(
            sample
                .request_sequence
                .ok_or_else(|| invalid("missing requestSequence"))?,
            1,
        )?,
        received_at_wall_ms: end,
    })
}

fn current(input: &Input, state: &mut State) -> Result<i64, CoreError> {
    let sample = valid_sample(&state.clock_offset);
    let identity = sample.as_ref().map_or_else(
        || "local".to_owned(),
        |s| {
            format!(
                "{}:{}:{}",
                s.offset_ms, s.uncertainty_ms, s.sampled_at_wall_ms
            )
        },
    );
    let Some(monotonic) = input.reading.monotonic_ms else {
        return wall_now(input, state, sample.as_ref());
    };
    if !monotonic.is_finite() {
        return Err(invalid("invalid PWA monotonic reading"));
    }
    if let Some(runtime) = &state.runtime {
        if !runtime.monotonic_ms.is_finite() {
            return Err(invalid("invalid PWA runtime reading"));
        }
        if runtime.identity == identity && monotonic >= runtime.monotonic_ms {
            let elapsed = monotonic - runtime.monotonic_ms;
            let delta = elapsed.floor() + f64::from(elapsed - elapsed.floor() >= 0.5);
            let now = floating_integer(runtime.wall_ms as f64 + delta, 1)?;
            return integer(now.max(state.minimum_wall_ms), 1);
        }
    }
    let wall = wall_now(input, state, sample.as_ref())?;
    state.runtime = Some(Runtime {
        identity,
        monotonic_ms: monotonic,
        wall_ms: wall,
    });
    Ok(wall)
}

fn wall_now(input: &Input, state: &State, sample: Option<&Sample>) -> Result<i64, CoreError> {
    let wall = integer(
        input
            .reading
            .wall_ms
            .ok_or_else(|| invalid("missing local wall"))?,
        1,
    )?;
    integer(
        (wall + sample.map_or(0, |s| s.offset_ms)).max(state.minimum_wall_ms),
        1,
    )
}
