use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};

use crate::CoreError;
use crate::sync_projection::OperationClock;
use crate::timer::WireCommand;

use super::acknowledgements::PendingQueues;
use super::{CanonicalResponse, MAX_CLOCK_SKEW_MS, MAX_SAFE_INTEGER};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Hlc {
    wall_ms: i64,
    counter: i64,
}

pub(super) fn rebase(
    pending: &mut PendingQueues,
    response: &CanonicalResponse,
) -> Result<(), CoreError> {
    let (canonical_clock, minimum_ms, maximum_ms) = clock_bounds(response)?;
    rebase_command_clocks(
        &mut pending.commands,
        canonical_clock,
        minimum_ms,
        maximum_ms,
    )?;
    rebase_operation_clocks(
        pending
            .tasks
            .iter_mut()
            .map(|operation| &mut operation.clock),
        canonical_clock,
        minimum_ms,
        maximum_ms,
    )?;
    rebase_operation_clocks(
        pending
            .durations
            .iter_mut()
            .map(|operation| &mut operation.clock),
        canonical_clock,
        minimum_ms,
        maximum_ms,
    )?;
    rebase_operation_clocks(
        pending
            .auto_start
            .iter_mut()
            .map(|operation| &mut operation.clock),
        canonical_clock,
        minimum_ms,
        maximum_ms,
    )?;
    rebase_operation_clocks(
        pending
            .selected_task
            .iter_mut()
            .map(|operation| &mut operation.clock),
        canonical_clock,
        minimum_ms,
        maximum_ms,
    )
}

fn clock_bounds(response: &CanonicalResponse) -> Result<(Hlc, i64, i64), CoreError> {
    let server_time_ms = DateTime::parse_from_rfc3339(&response.server_time)
        .map_err(|_| CoreError::InvalidTimestamp(response.server_time.clone()))?
        .timestamp_millis();
    let minimum_ms = server_time_ms.saturating_sub(MAX_CLOCK_SKEW_MS).max(1);
    let maximum_ms = server_time_ms
        .saturating_add(MAX_CLOCK_SKEW_MS)
        .min(MAX_SAFE_INTEGER);
    let canonical_clock = Hlc {
        wall_ms: response.server_hlc_wall_ms,
        counter: response.server_hlc_counter,
    };
    Ok((canonical_clock, minimum_ms, maximum_ms))
}

fn rebase_command_clocks(
    commands: &mut [WireCommand],
    canonical_clock: Hlc,
    minimum_ms: i64,
    maximum_ms: i64,
) -> Result<(), CoreError> {
    let mut clocks = commands
        .iter()
        .map(command_clock)
        .collect::<Result<Vec<_>, CoreError>>()?;
    clocks.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let replacements = clock_replacements(
        clocks.into_iter().map(|(_, id, clock)| (id, clock)),
        canonical_clock,
        minimum_ms,
        maximum_ms,
    )?;
    for command in commands {
        if let Some(clock) = replacements.get(&command.id) {
            command.occurred_at =
                rebased_occurrence(&command.occurred_at, *clock, minimum_ms, maximum_ms)?;
            command.hlc_wall_ms = clock.wall_ms;
            command.hlc_counter = clock.counter;
        }
    }
    Ok(())
}

fn command_clock(command: &WireCommand) -> Result<(i64, String, Hlc), CoreError> {
    validate_queue_clock(
        &command.id,
        &command.device_id,
        command.hlc_wall_ms,
        command.hlc_counter,
    )?;
    Ok((
        command.device_sequence,
        command.id.clone(),
        Hlc {
            wall_ms: command.hlc_wall_ms,
            counter: command.hlc_counter,
        },
    ))
}

fn rebase_operation_clocks<'a>(
    clocks: impl IntoIterator<Item = &'a mut OperationClock>,
    canonical_clock: Hlc,
    minimum_ms: i64,
    maximum_ms: i64,
) -> Result<(), CoreError> {
    let mut clocks = clocks.into_iter().collect::<Vec<_>>();
    for clock in &clocks {
        validate_queue_clock(
            &clock.id,
            &clock.device_id,
            clock.hlc_wall_ms,
            clock.hlc_counter,
        )?;
    }
    let mut ordered = clocks
        .iter()
        .map(|clock| {
            (
                clock.id.clone(),
                Hlc {
                    wall_ms: clock.hlc_wall_ms,
                    counter: clock.hlc_counter,
                },
            )
        })
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    let replacements = clock_replacements(ordered, canonical_clock, minimum_ms, maximum_ms)?;
    for clock in &mut clocks {
        if let Some(replacement) = replacements.get(&clock.id) {
            clock.occurred_at =
                rebased_occurrence(&clock.occurred_at, *replacement, minimum_ms, maximum_ms)?;
            clock.hlc_wall_ms = replacement.wall_ms;
            clock.hlc_counter = replacement.counter;
        }
    }
    Ok(())
}

fn validate_queue_clock(
    id: &str,
    device_id: &str,
    wall_ms: i64,
    counter: i64,
) -> Result<(), CoreError> {
    if id.is_empty()
        || device_id.is_empty()
        || !(0..=MAX_SAFE_INTEGER).contains(&wall_ms)
        || !(0..=MAX_SAFE_INTEGER).contains(&counter)
    {
        return Err(CoreError::InvalidInput(
            "invalid retained operation clock".into(),
        ));
    }
    Ok(())
}

fn clock_replacements(
    clocks: impl IntoIterator<Item = (String, Hlc)>,
    canonical_clock: Hlc,
    minimum_ms: i64,
    maximum_ms: i64,
) -> Result<BTreeMap<String, Hlc>, CoreError> {
    let mut cursor = canonical_clock;
    let mut replacements = BTreeMap::new();
    for (id, clock) in clocks {
        if (minimum_ms..=maximum_ms).contains(&clock.wall_ms) && clock > cursor {
            cursor = clock;
            continue;
        }
        cursor = next_clock(cursor, maximum_ms)?;
        replacements.insert(id, cursor);
    }
    Ok(replacements)
}

fn next_clock(clock: Hlc, maximum_ms: i64) -> Result<Hlc, CoreError> {
    if clock.counter < MAX_SAFE_INTEGER {
        return Ok(Hlc {
            wall_ms: clock.wall_ms,
            counter: clock.counter + 1,
        });
    }
    if clock.wall_ms >= maximum_ms {
        return Err(CoreError::InvalidInput(
            "retained operation clock has no safe rebase headroom".into(),
        ));
    }
    Ok(Hlc {
        wall_ms: clock.wall_ms + 1,
        counter: 0,
    })
}

fn rebased_occurrence(
    original: &str,
    clock: Hlc,
    minimum_ms: i64,
    maximum_ms: i64,
) -> Result<String, CoreError> {
    let original_time = DateTime::parse_from_rfc3339(original)
        .map_err(|_| CoreError::InvalidTimestamp(original.to_owned()))?;
    let original_ms = original_time.timestamp_millis();
    if (minimum_ms..=maximum_ms).contains(&original_ms)
        && clock.wall_ms.abs_diff(original_ms) <= MAX_CLOCK_SKEW_MS as u64
    {
        return Ok(original.to_owned());
    }
    let replacement = DateTime::<Utc>::from_timestamp_millis(clock.wall_ms)
        .ok_or_else(|| CoreError::InvalidInput("invalid rebased operation time".into()))?;
    Ok(replacement.to_rfc3339_opts(SecondsFormat::Millis, true))
}
