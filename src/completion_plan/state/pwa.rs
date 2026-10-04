use super::{Bounds, HistoryItem, Install, Output, day_at, parse_time, phase_after, sent, source};
use crate::CoreError;

pub(super) fn install(
    input: &Install,
    commands: &[sent::SentCommand],
    bounds: &[Bounds],
    output: &mut Output,
) -> Result<(), CoreError> {
    let Some(timer) = input
        .canonical_timer
        .as_ref()
        .filter(|timer| timer.status == "completed")
    else {
        return Ok(());
    };
    if sent::has_finish(commands, &timer.id)
        || !input.pending.command_ids.is_empty()
        || !input.pending.other_operation_ids.is_empty()
    {
        return Ok(());
    }
    let Some(row) = crate::timer::workspace::validate_pair(Some(timer), &input.after_history)?
    else {
        return Ok(());
    };
    if !exact_completion(timer, row) {
        return Ok(());
    }
    let mut lifecycle = input.lifecycle.clone().unwrap_or_default();
    let consumed = lifecycle.consumed(&row.timer_id, &row.phase)
        || input.before_history.iter().any(|before| {
            before.status == "completed"
                && before.timer_id == row.timer_id
                && before.phase == row.phase
        });
    if consumed {
        return Ok(());
    }
    lifecycle.remember(row);
    output.lifecycle = Some(lifecycle);
    output.source = Some(source(row));
    if output.selection.explicit {
        output.reason = "explicitSelection";
    } else if output.selection.phase != row.phase {
        output.reason = "selectionDiffersFromSource";
    } else {
        let day = day_at(bounds, parse_time(&timer.anchor_at)?)?;
        output.selection.phase = phase_after(&row.phase, &input.after_history, day)?;
        output.reason = "completionSelected";
    }
    Ok(())
}

fn exact_completion(timer: &crate::timer::CanonicalTimer, row: &HistoryItem) -> bool {
    timer
        .last_intent
        .as_ref()
        .is_some_and(|intent| match intent.kind.as_str() {
            "start" | "resume" => row.command_id.is_none(),
            "finish" => row.command_id.as_deref() == Some(intent.command_id.as_str()),
            _ => false,
        })
}

pub(super) fn natural_finish(
    input: &Install,
    timer_id: &str,
    phase: &str,
    bounds: &[Bounds],
) -> Result<Option<String>, CoreError> {
    if protected_selection(input, timer_id, phase)? {
        return Ok(Some(input.selection.phase.clone()));
    }
    let Some(timer) = input.canonical_timer.as_ref().filter(|timer| {
        timer.status == "completed" && timer.id == timer_id && timer.phase == phase
    }) else {
        return Ok(None);
    };
    let Some(row) = crate::timer::workspace::validate_pair(Some(timer), &input.after_history)?
    else {
        return Ok(None);
    };
    // A later terminal command changes provenance, not the already consumed
    // natural session or a phase choice made after its Finish was queued.
    if consumed_natural(input, timer_id, phase) {
        return Ok(Some(input.selection.phase.clone()));
    }
    if row.command_id.is_some()
        || !timer
            .last_intent
            .as_ref()
            .is_some_and(|intent| matches!(intent.kind.as_str(), "start" | "resume"))
    {
        return Ok(None);
    }
    let selected = if input.selection.explicit || input.selection.phase != row.phase {
        input.selection.phase.clone()
    } else {
        phase_after(
            &row.phase,
            &input.after_history,
            day_at(bounds, parse_time(row.completed_at.as_deref().unwrap())?)?,
        )?
    };
    Ok(Some(selected))
}

fn protected_selection(input: &Install, timer_id: &str, phase: &str) -> Result<bool, CoreError> {
    let Some(state) = &input.lifecycle else {
        return Ok(false);
    };
    let durable = state.has_finish_evidence()
        && state.natural_consumed(timer_id, phase)
        && state.finished(timer_id, phase);
    if !input.selection.explicit && !durable {
        return Ok(false);
    }
    // A late ACK cannot replace a durable user choice after its timer was cleared
    // or replaced. The current canonical pair still has to be valid.
    crate::timer::workspace::validate_pair(input.canonical_timer.as_ref(), &input.after_history)?;
    Ok(true)
}

fn consumed_natural(input: &Install, timer_id: &str, phase: &str) -> bool {
    input
        .lifecycle
        .as_ref()
        .is_some_and(|state| state.natural_consumed(timer_id, phase))
        || input.before_history.iter().any(|row| {
            row.status == "completed"
                && row.timer_id == timer_id
                && row.phase == phase
                && row.command_id.is_none()
        })
}
