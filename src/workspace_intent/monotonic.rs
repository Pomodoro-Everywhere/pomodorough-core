use chrono::{SecondsFormat, Utc};
use serde_json::{Value, json};

use super::model::{Compatibility, Input, MonotonicAnchor, Observation};
use super::{CoreError, allocation, invalid, projection};

const MAX_SAFE_MS: f64 = 9_007_199_254_740_991.0;

fn valid_monotonic(value: f64) -> bool {
    value.is_finite() && (0.0..=MAX_SAFE_MS).contains(&value)
}

fn json_milliseconds(value: f64) -> Value {
    if value.fract() == 0.0 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

pub(super) fn validate(input: &Input) -> Result<(), CoreError> {
    let clock = &input.clock;
    if clock.monotonic_now_ms.is_some() && clock.continuity_id.is_none() {
        return Err(invalid("monotonic clock requires continuity identity"));
    }
    if clock
        .monotonic_now_ms
        .is_some_and(|ms| !valid_monotonic(ms))
        || clock.continuity_id.as_deref().is_some_and(str::is_empty)
    {
        return Err(invalid("invalid monotonic clock reading"));
    }
    if let Some(anchor) = &input.observation.monotonic_anchor {
        if anchor.timer_id.is_empty()
            || anchor.continuity_id.is_empty()
            || !valid_monotonic(anchor.sampled_monotonic_ms)
            || !(0..=MAX_SAFE_MS as i64).contains(&anchor.elapsed_at_anchor_ms)
            || !(1..=MAX_SAFE_MS as i64).contains(&anchor.sampled_trusted_now_ms)
            || chrono::DateTime::<Utc>::from_timestamp_millis(anchor.sampled_trusted_now_ms)
                .is_none()
        {
            return Err(invalid("invalid monotonic anchor"));
        }
        crate::timer::parse_time(&anchor.anchor_at)?;
    }
    if input.compatibility != Compatibility::PwaStorage
        && (clock.monotonic_now_ms.is_some()
            || clock.continuity_id.is_some()
            || input.observation.monotonic_anchor.is_some())
    {
        return Err(invalid("monotonic observation requires PWA profile"));
    }
    Ok(())
}

fn matches_timer(anchor: &MonotonicAnchor, timer: &Value) -> bool {
    timer["status"] == "running"
        && timer["id"] == anchor.timer_id
        && timer["anchorAt"] == anchor.anchor_at
        && timer["elapsedAtAnchorMs"] == anchor.elapsed_at_anchor_ms
}

fn sample(input: &Input, timer: &Value, observation: &mut Observation) -> Result<(), CoreError> {
    if timer["status"] != "running" {
        observation.monotonic_anchor = None;
        return Ok(());
    }
    let Some(now) = input.clock.monotonic_now_ms else {
        if input
            .clock
            .continuity_id
            .as_deref()
            .is_some_and(|identity| {
                observation
                    .monotonic_anchor
                    .as_ref()
                    .is_some_and(|anchor| anchor.continuity_id != identity)
            })
        {
            observation.monotonic_anchor = None;
        }
        return Ok(());
    };
    let identity = input.clock.continuity_id.as_ref().unwrap();
    if observation.monotonic_anchor.as_ref().is_some_and(|anchor| {
        matches_timer(anchor, timer)
            && anchor.continuity_id == *identity
            && now >= anchor.sampled_monotonic_ms
    }) {
        return Ok(());
    }
    let sampled_trusted_now_ms =
        crate::timer::parse_time(&input.clock.observed_at)?.timestamp_millis();
    if !(1..=MAX_SAFE_MS as i64).contains(&sampled_trusted_now_ms) {
        return Err(invalid("monotonic sample time out of range"));
    }
    observation.monotonic_anchor = Some(MonotonicAnchor {
        timer_id: timer["id"].as_str().unwrap().into(),
        anchor_at: timer["anchorAt"].as_str().unwrap().into(),
        elapsed_at_anchor_ms: timer["elapsedAtAnchorMs"].as_i64().unwrap(),
        sampled_trusted_now_ms,
        sampled_monotonic_ms: now,
        continuity_id: identity.clone(),
    });
    Ok(())
}

pub(super) fn elapsed(
    input: &Input,
    timer: &Value,
    observation: &Observation,
) -> Result<f64, CoreError> {
    let Some(now) = input.clock.monotonic_now_ms else {
        return Ok(allocation::elapsed(timer, &input.clock.observed_at)? as f64);
    };
    let Some(anchor) = observation.monotonic_anchor.as_ref().filter(|anchor| {
        matches_timer(anchor, timer)
            && input.clock.continuity_id.as_deref() == Some(&anchor.continuity_id)
    }) else {
        return Ok(allocation::elapsed(timer, &input.clock.observed_at)? as f64);
    };
    if now < anchor.sampled_monotonic_ms {
        return Ok(allocation::elapsed(timer, &input.clock.observed_at)? as f64);
    }
    let sampled = chrono::DateTime::<Utc>::from_timestamp_millis(anchor.sampled_trusted_now_ms)
        .ok_or_else(|| invalid("monotonic sample time out of range"))?;
    let base = allocation::elapsed(timer, &sampled.to_rfc3339())? as f64;
    let planned = timer["plannedDurationMs"].as_i64().unwrap() as f64;
    Ok((base + now - anchor.sampled_monotonic_ms).clamp(0.0, planned))
}

fn effective_now(timer: &Value, elapsed: f64) -> Result<String, CoreError> {
    let anchor = crate::timer::parse_time(timer["anchorAt"].as_str().unwrap())?.timestamp_millis();
    let elapsed_at_anchor = timer["elapsedAtAnchorMs"].as_i64().unwrap();
    let now = anchor
        .checked_add((elapsed - elapsed_at_anchor as f64).floor() as i64)
        .ok_or_else(|| invalid("monotonic timer observation out of range"))?;
    Ok(chrono::DateTime::<Utc>::from_timestamp_millis(now)
        .ok_or_else(|| invalid("monotonic timer observation out of range"))?
        .to_rfc3339_opts(SecondsFormat::Millis, true))
}

pub(super) fn before(input: &Input) -> Result<(Value, Observation), CoreError> {
    let mut observation = input.observation.clone();
    if input.clock.monotonic_now_ms.is_none() {
        let before = projection::before(input, None)?;
        sample(input, &before["canonicalTimer"], &mut observation)?;
        return Ok((before, observation));
    }
    // Replay before expiry so a wall jump cannot discard an anchored running timer.
    let unexpired = projection::project(&input.workspace, "1970-01-01T00:00:00Z")?;
    let decision = projection::entrypoint(
        input,
        &unexpired["workspace"],
        &observation,
        "1970-01-01T00:00:00Z",
    )?;
    let timer = &decision["canonicalTimer"];
    sample(input, timer, &mut observation)?;
    let now = if timer["status"] == "running" {
        Some(effective_now(timer, elapsed(input, timer, &observation)?)?)
    } else {
        None
    };
    Ok((projection::before(input, now.as_deref())?, observation))
}

pub(super) fn entrypoint_time(
    input: &Input,
    timer: &Value,
    observation: &Observation,
) -> Result<String, CoreError> {
    if timer["status"] == "running" && input.clock.monotonic_now_ms.is_some() {
        return effective_now(timer, elapsed(input, timer, observation)?);
    }
    Ok(input.clock.physical_now.clone())
}

pub(super) fn after(
    input: &Input,
    workspace: &Value,
    observation: &mut Observation,
) -> Result<(), CoreError> {
    if input.compatibility == Compatibility::PwaStorage {
        sample(input, &workspace["canonicalTimer"], observation)?;
    }
    Ok(())
}

pub(super) fn timer_observation(
    input: &Input,
    workspace: &Value,
    observation: &Observation,
) -> Result<Value, CoreError> {
    let timer = &workspace["canonicalTimer"];
    if timer.is_null() {
        return Ok(Value::Null);
    }
    let elapsed = elapsed(input, timer, observation)?;
    let remaining = timer["plannedDurationMs"].as_i64().unwrap() as f64 - elapsed;
    let deadline = if timer["status"] == "running" {
        json!(effective_now(
            timer,
            timer["plannedDurationMs"].as_i64().unwrap() as f64
        )?)
    } else {
        Value::Null
    };
    Ok(
        json!({"timerId": timer["id"], "elapsedMs": json_milliseconds(elapsed),
        "remainingMs": json_milliseconds(remaining), "deadlineAt": deadline}),
    )
}
