use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::{Profile, invalid};
use crate::{CoreError, timer::CanonicalTimer};

const MAX_SAFE_MS: f64 = 9_007_199_254_740_991.0;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Monotonic {
    pub now_ms: f64,
    pub continuity_id: String,
    pub anchor: Anchor,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Anchor {
    timer_id: String,
    anchor_at: String,
    elapsed_at_anchor_ms: i64,
    sampled_trusted_now_ms: i64,
    sampled_monotonic_ms: f64,
    continuity_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TimerView {
    pub timer_id: Option<String>,
    pub phase: Option<String>,
    pub status: String,
    pub planned_duration_ms: i64,
    pub elapsed_ms: f64,
    pub remaining_ms: f64,
    pub deadline_at: Option<String>,
    pub progress: f64,
    pub observed_elapsed_ms: i64,
}

pub(super) fn validate(monotonic: Option<&Monotonic>, profile: Profile) -> Result<(), CoreError> {
    let Some(m) = monotonic else {
        return Ok(());
    };
    let a = &m.anchor;
    if profile != Profile::PwaStorage
        || a.timer_id.is_empty()
        || a.continuity_id.is_empty()
        || m.continuity_id.is_empty()
        || !valid(m.now_ms)
        || !valid(a.sampled_monotonic_ms)
        || !(1..=MAX_SAFE_MS as i64).contains(&a.sampled_trusted_now_ms)
        || !(0..=MAX_SAFE_MS as i64).contains(&a.elapsed_at_anchor_ms)
        || DateTime::<Utc>::from_timestamp_millis(a.sampled_trusted_now_ms).is_none()
    {
        return Err(invalid("invalid read model monotonic observation"));
    }
    crate::timer::parse_time(&a.anchor_at)?;
    Ok(())
}

pub(super) fn replay_time(
    timer: &CanonicalTimer,
    monotonic: &Monotonic,
) -> Result<Option<String>, CoreError> {
    let a = &monotonic.anchor;
    if timer.status != "running"
        || a.timer_id != timer.id
        || a.anchor_at != timer.anchor_at
        || a.elapsed_at_anchor_ms != timer.elapsed_at_anchor_ms
        || a.continuity_id != monotonic.continuity_id
        || monotonic.now_ms < a.sampled_monotonic_ms
    {
        return Ok(None);
    }
    let sampled = DateTime::<Utc>::from_timestamp_millis(a.sampled_trusted_now_ms)
        .ok_or_else(|| invalid("invalid read model monotonic sample"))?;
    let anchor = crate::timer::parse_time(&timer.anchor_at)?;
    let elapsed = elapsed_running(timer, sampled, Some(monotonic), anchor)?;
    let delta = (elapsed - timer.elapsed_at_anchor_ms as f64).floor() as i64;
    let effective = anchor
        .checked_add_signed(chrono::Duration::milliseconds(delta))
        .ok_or_else(|| invalid("read model replay time out of range"))?;
    Ok(Some(effective.to_rfc3339_opts(SecondsFormat::Millis, true)))
}

fn valid(value: f64) -> bool {
    value.is_finite() && (0.0..=MAX_SAFE_MS).contains(&value)
}

pub(super) fn timer_view(
    timer: Option<&CanonicalTimer>,
    now: DateTime<Utc>,
    monotonic: Option<&Monotonic>,
) -> Result<TimerView, CoreError> {
    let Some(timer) = timer else {
        return Ok(TimerView {
            timer_id: None,
            phase: None,
            status: "idle".into(),
            planned_duration_ms: 0,
            elapsed_ms: 0.0,
            remaining_ms: 0.0,
            deadline_at: None,
            progress: 0.0,
            observed_elapsed_ms: 0,
        });
    };
    let planned = timer.planned_duration_ms;
    let anchor = crate::timer::parse_time(&timer.anchor_at)?;
    let elapsed = match timer.status.as_str() {
        "running" => elapsed_running(timer, now, monotonic, anchor)?,
        "completed" => planned as f64,
        _ => timer.elapsed_at_anchor_ms as f64,
    };
    let elapsed = elapsed.clamp(0.0, planned as f64);
    let deadline = if timer.status == "running" {
        let remaining = planned - timer.elapsed_at_anchor_ms;
        Some(
            anchor
                .checked_add_signed(chrono::Duration::milliseconds(remaining))
                .ok_or_else(|| invalid("read model deadline out of range"))?
                .to_rfc3339_opts(SecondsFormat::AutoSi, true),
        )
    } else {
        None
    };
    Ok(TimerView {
        timer_id: Some(timer.id.clone()),
        phase: Some(timer.phase.clone()),
        status: timer.status.clone(),
        planned_duration_ms: planned,
        elapsed_ms: elapsed,
        remaining_ms: planned as f64 - elapsed,
        deadline_at: deadline,
        progress: elapsed / planned as f64,
        observed_elapsed_ms: elapsed.round() as i64,
    })
}

fn elapsed_running(
    timer: &CanonicalTimer,
    now: DateTime<Utc>,
    monotonic: Option<&Monotonic>,
    anchor: DateTime<Utc>,
) -> Result<f64, CoreError> {
    let wall = |at: DateTime<Utc>| {
        let delta = at
            .signed_duration_since(anchor)
            .to_std()
            .map_or(0.0, |d| d.as_secs_f64() * 1_000.0);
        (timer.elapsed_at_anchor_ms as f64 + delta).clamp(0.0, timer.planned_duration_ms as f64)
    };
    let Some(m) = monotonic else {
        return Ok(wall(now));
    };
    let a = &m.anchor;
    if a.timer_id != timer.id
        || a.anchor_at != timer.anchor_at
        || a.elapsed_at_anchor_ms != timer.elapsed_at_anchor_ms
        || a.continuity_id != m.continuity_id
        || m.now_ms < a.sampled_monotonic_ms
    {
        return Ok(wall(now));
    }
    let sampled = DateTime::<Utc>::from_timestamp_millis(a.sampled_trusted_now_ms)
        .ok_or_else(|| invalid("invalid read model monotonic sample"))?;
    Ok((wall(sampled) + m.now_ms - a.sampled_monotonic_ms)
        .clamp(0.0, timer.planned_duration_ms as f64))
}
