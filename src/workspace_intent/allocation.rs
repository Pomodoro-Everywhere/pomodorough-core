use chrono::{SecondsFormat, Utc};
use serde_json::{Value, json};

use super::model::{Allocation, CommandKind, Compatibility, Input, Observation};
use super::{CoreError, invalid};

pub(super) fn validate(input: &Input) -> Result<(), CoreError> {
    crate::clock::validate_hlc_values(input.allocation.hlc.wall_ms, input.allocation.hlc.counter)?;
    crate::clock::validate_hlc_values(input.allocation.device_sequence, 0)?;
    let limit = if input.intent.is_workspace_mutation() {
        3
    } else {
        2
    };
    if input.allocation.device_id.is_empty() || input.identities.command_uuids.len() > limit {
        return Err(invalid("invalid workspace allocation"));
    }
    let mut last = input.allocation.last_uuid.as_deref();
    if let Some(uuid) = last {
        validate_uuid(uuid, Some(b'7'))?;
    }
    for uuid in &input.identities.command_uuids {
        validate_uuid(uuid, Some(b'7'))?;
        if last.is_some_and(|last| uuid.as_str() <= last) {
            return Err(invalid("command UUID allocation is not increasing"));
        }
        last = Some(uuid);
    }
    if let Some(uuid) = &input.identities.timer_uuid {
        validate_uuid(uuid, Some(b'4'))?;
    }
    Ok(())
}

pub(crate) fn validate_uuid(uuid: &str, version: Option<u8>) -> Result<(), CoreError> {
    let bytes = uuid.as_bytes();
    if bytes.len() != 36 {
        return Err(invalid("invalid allocated UUID"));
    }
    for (index, byte) in bytes.iter().enumerate() {
        let valid = if matches!(index, 8 | 13 | 18 | 23) {
            *byte == b'-'
        } else {
            byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
        };
        if !valid {
            return Err(invalid("invalid allocated UUID"));
        }
    }
    if version.is_some_and(|version| bytes[14] != version) || !b"89ab".contains(&bytes[19]) {
        return Err(invalid("invalid allocated UUID version or variant"));
    }
    Ok(())
}

pub(super) fn advance(input: &Input, state: &mut Allocation) -> Result<(), CoreError> {
    tick(input, state)?;
    for command in input.workspace["local"]["commands"].as_array().unwrap() {
        if command["deviceId"] == state.device_id {
            state.device_sequence = state
                .device_sequence
                .max(command["deviceSequence"].as_i64().unwrap());
        }
    }
    state.device_sequence = state
        .device_sequence
        .checked_add(1)
        .ok_or_else(|| invalid("sequence overflow"))?;
    crate::clock::validate_hlc_values(state.device_sequence, 0)
}

pub(super) fn tick(input: &Input, state: &mut Allocation) -> Result<(), CoreError> {
    let now = crate::timer::parse_time(&input.clock.occurred_at)?.timestamp_millis();
    let tick =
        crate::clock::tick_json(&json!({"local": state.hlc, "physicalNowMs": now}).to_string())?;
    state.hlc = serde_json::from_str(&tick)?;
    if state.hlc.wall_ms.abs_diff(now) > 300_000 {
        return Err(invalid("HLC exceeds trusted occurrence skew"));
    }
    Ok(())
}

pub(super) fn reserved_uuid(
    input: &Input,
    state: &mut Allocation,
    index: usize,
) -> Result<String, CoreError> {
    let uuid = input
        .identities
        .command_uuids
        .get(index)
        .ok_or_else(|| invalid("insufficient operation identities"))?;
    let prior = state
        .last_uuid
        .as_deref()
        .map(uuid_timestamp)
        .transpose()?
        .unwrap_or(0);
    if uuid_timestamp(uuid)? != state.hlc.wall_ms.max(prior) {
        return Err(invalid(
            "operation UUID timestamp does not match reserved clock",
        ));
    }
    state.last_uuid = Some(uuid.clone());
    Ok(uuid.clone())
}

pub(super) fn occurrence(input: &Input, state: &Allocation) -> Result<String, CoreError> {
    command_occurrence(input, state)
}

pub(super) fn command(
    input: &Input,
    state: &Allocation,
    workspace: &Value,
    observation: &Observation,
    kind: CommandKind,
    index: usize,
) -> Result<Value, CoreError> {
    let uuid = input
        .identities
        .command_uuids
        .get(index)
        .ok_or_else(|| invalid("insufficient command identities"))?;
    let previous_ms = state
        .last_uuid
        .as_ref()
        .map(|last| uuid_timestamp(last))
        .transpose()?
        .unwrap_or(0);
    if uuid_timestamp(uuid)? != state.hlc.wall_ms.max(previous_ms) {
        return Err(invalid(
            "command UUID timestamp does not match reserved clock",
        ));
    }
    let apple = input.compatibility == Compatibility::AppleWorkspace;
    let starting = matches!(kind, CommandKind::Start);
    let timer = &workspace["canonicalTimer"];
    let phase = if starting {
        input.selection.phase.name()
    } else {
        timer["phase"].as_str().unwrap()
    };
    let timer_id = timer_identity(input, timer, starting)?;
    let occurred = command_occurrence(input, state)?;
    let mut command = json!({"id": if apple {format!("command-{uuid}")} else {uuid.clone()},
    "deviceId": state.device_id, "deviceSequence": state.device_sequence,
    "timerId": timer_id, "type": kind.name(), "phase": phase,
    "plannedDurationMs": if starting { &workspace["durationsMs"][phase] } else { &timer["plannedDurationMs"] },
    "occurredAt": occurred, "hlcWallMs": state.hlc.wall_ms, "hlcCounter": state.hlc.counter,
        "observedElapsedMs": if starting { 0 } else {
            super::monotonic::elapsed(input, timer, observation)?.round() as i64
        }});
    if starting && phase == "focus" && !workspace["selectedTaskId"].is_null() {
        command["taskId"] = workspace["selectedTaskId"].clone();
    }
    Ok(command)
}

fn command_occurrence(input: &Input, state: &Allocation) -> Result<String, CoreError> {
    if input.compatibility != Compatibility::PwaStorage {
        return Ok(input.clock.occurred_at.clone());
    }
    Ok(
        chrono::DateTime::<Utc>::from_timestamp_millis(state.hlc.wall_ms)
            .ok_or_else(|| invalid("HLC timestamp out of range"))?
            .to_rfc3339_opts(SecondsFormat::Millis, true),
    )
}

fn uuid_timestamp(uuid: &str) -> Result<i64, CoreError> {
    i64::from_str_radix(&format!("{}{}", &uuid[..8], &uuid[9..13]), 16)
        .map_err(|_| invalid("invalid command UUID timestamp"))
}

fn timer_identity(input: &Input, timer: &Value, starting: bool) -> Result<String, CoreError> {
    if !starting {
        return Ok(timer["id"].as_str().unwrap().into());
    }
    let uuid = input
        .identities
        .timer_uuid
        .as_ref()
        .ok_or_else(|| invalid("missing timer identity"))?;
    Ok(if input.compatibility == Compatibility::AppleWorkspace {
        format!("timer-{uuid}")
    } else {
        uuid.clone()
    })
}

pub(super) fn elapsed(timer: &Value, observed_at: &str) -> Result<i64, CoreError> {
    let elapsed = timer["elapsedAtAnchorMs"].as_i64().unwrap();
    let duration = timer["plannedDurationMs"].as_i64().unwrap();
    let delta = if timer["status"] == "running" {
        let anchor = crate::timer::parse_time(timer["anchorAt"].as_str().unwrap())?;
        (crate::timer::parse_time(observed_at)? - anchor)
            .num_milliseconds()
            .max(0)
    } else {
        0
    };
    Ok(elapsed.saturating_add(delta).clamp(0, duration))
}
