use serde::{Deserialize, Serialize};

use super::{Compatibility, Install, Output, generation, invalid, validate_ids, validate_phase};
use crate::CoreError;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Advance {
    command_id: String,
    timer_id: String,
    previous_phase: String,
    advanced_phase: String,
    generation: String,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) enum Outcome {
    Applied,
    Ignored,
    Rejected,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Acknowledgement {
    pub(super) command_id: String,
    pub(super) outcome: Outcome,
}

pub(super) fn validate(input: &Install) -> Result<(), CoreError> {
    validate_ids(
        &input
            .advances
            .iter()
            .map(|a| a.command_id.clone())
            .collect::<Vec<_>>(),
    )?;
    validate_ids(
        &input
            .acknowledgements
            .iter()
            .map(|a| a.command_id.clone())
            .collect::<Vec<_>>(),
    )?;
    for advance in &input.advances {
        if advance.timer_id.is_empty() {
            return Err(invalid("empty advance timer identity"));
        }
        validate_phase(&advance.previous_phase)?;
        validate_phase(&advance.advanced_phase)?;
        generation(&advance.generation)?;
    }
    Ok(())
}

fn exact(input: &Install, advance: &Advance) -> bool {
    let apple = input.compatibility == Compatibility::AppleAp04;
    input.after_history.iter().any(|item| {
        item.status == "completed"
            && item.timer_id == advance.timer_id
            && item.command_id.as_deref() == Some(&advance.command_id)
            && (apple || item.phase == advance.previous_phase)
    }) || input.canonical_timer.as_ref().is_some_and(|timer| {
        timer.id == advance.timer_id
            && timer.status == "completed"
            && (apple || timer.phase == advance.previous_phase)
            && timer.last_intent.as_ref().is_some_and(|intent| {
                intent.command_id == advance.command_id && (!apple || intent.kind == "finish")
            })
    })
}

fn resolution(input: &Install, advance: &Advance) -> Option<bool> {
    let acknowledgement = input
        .acknowledgements
        .iter()
        .find(|a| a.command_id == advance.command_id);
    // AP04 resolves only acknowledged records or the suffix of an invalid acknowledgement.
    if input.compatibility == Compatibility::AppleAp04 {
        return acknowledgement.map(|a| a.outcome == Outcome::Rejected || !exact(input, advance));
    }
    let discarded = input.discarded_command_ids.contains(&advance.command_id);
    if acknowledgement.is_none() && !discarded {
        return None;
    }
    Some(
        !exact(input, advance)
            && (discarded || acknowledgement.is_some_and(|a| a.outcome != Outcome::Applied)),
    )
}

pub(super) fn resolve(input: &Install) -> Output {
    let mut output = Output {
        selection: input.selection.clone(),
        source: None,
        reason: "noNewCompletion",
        advances: vec![],
        retired_advance_ids: vec![],
        rolled_back_advance_ids: vec![],
        lifecycle: None,
    };
    let invalid = input
        .advances
        .iter()
        .position(|a| resolution(input, a) == Some(true));
    let indices: Vec<_> = match input.compatibility {
        Compatibility::AppleAp04 => (0..input.advances.len()).rev().collect(),
        _ => (0..input.advances.len()).collect(),
    };
    for index in indices {
        let advance = &input.advances[index];
        let dependent = input.compatibility == Compatibility::AppleAp04
            && invalid.is_some_and(|first| index >= first);
        match resolution(input, advance) {
            None if !dependent => output.advances.push(advance.clone()),
            resolved => retire(
                &mut output,
                advance,
                dependent || resolved == Some(true),
                input.compatibility,
            ),
        }
    }
    if input.compatibility == Compatibility::AppleAp04 {
        output.advances.reverse();
    }
    output
}

fn retire(output: &mut Output, advance: &Advance, invalid: bool, compatibility: Compatibility) {
    output.retired_advance_ids.push(advance.command_id.clone());
    if !invalid
        || output.selection.generation != advance.generation
        || output.selection.phase != advance.advanced_phase
    {
        return;
    }
    output.selection.phase.clone_from(&advance.previous_phase);
    if compatibility == Compatibility::AppleAp04 {
        let current = generation(&advance.generation).expect("validated generation");
        output.selection.generation = if current == 0 { i64::MAX } else { current - 1 }.to_string();
    }
    output
        .rolled_back_advance_ids
        .push(advance.command_id.clone());
}
