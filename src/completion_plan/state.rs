use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{parse_bounds, phase_after, validate_optional_timer, validate_phase};
use crate::CoreError;
use crate::timer::{CanonicalTimer, HistoryItem, parse_time, validate_history};

mod pwa;
mod rollback;
mod sent;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Compatibility {
    AppleAp04,
    DesktopD03,
    AndroidCapturedSend,
    PwaRejectedFinish,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    phase: String,
    // Decimal text preserves Apple's Int64 generation across JavaScript bridges.
    generation: String,
    explicit: bool,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Input {
    Install(Box<Install>),
    Skip(Skip),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Install {
    compatibility: Compatibility,
    before_history: Vec<HistoryItem>,
    after_history: Vec<HistoryItem>,
    canonical_timer: Option<CanonicalTimer>,
    selection: Selection,
    pending: Pending,
    advances: Vec<rollback::Advance>,
    acknowledgements: Vec<rollback::Acknowledgement>,
    discarded_command_ids: Vec<String>,
    reference_time: String,
    calendar_intervals: Vec<Interval>,
    #[serde(default)]
    sent_context: Option<sent::Context>,
    #[serde(skip)]
    lifecycle: Option<crate::workspace_intent::completion_mutation::CompletionLifecycle>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pending {
    command_ids: Vec<String>,
    sendable_command_ids: Vec<String>,
    other_operation_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Interval {
    start: String,
    end: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Skip {
    selection: Selection,
    source_phase: String,
    history: Vec<HistoryItem>,
    reference_time: String,
    calendar_intervals: Vec<Interval>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    selection: Selection,
    source: Option<Source>,
    reason: &'static str,
    advances: Vec<rollback::Advance>,
    retired_advance_ids: Vec<String>,
    rolled_back_advance_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lifecycle: Option<crate::workspace_intent::completion_mutation::CompletionLifecycle>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Source {
    history_id: String,
    timer_id: String,
    command_id: Option<String>,
    phase: String,
    occurred_at: String,
}

pub(crate) fn plan_json(input: &str) -> Result<String, CoreError> {
    crate::check_input_len(input)?;
    // The extension must not change the shipped unknown-field error envelope.
    let mut value = crate::strict_json::parse(input)?;
    if value["kind"] == "install" {
        crate::strict_json::shape::validate(&value, &crate::completion_schema::INSTALL, "")?;
    }
    let lifecycle = if value["kind"] == "install" {
        value
            .as_object_mut()
            .and_then(|object| object.remove("lifecycle"))
    } else {
        None
    };
    let original = value.clone();
    let output = match serde_json::from_value(value)? {
        Input::Install(mut input) => {
            input.lifecycle = lifecycle
                .as_ref()
                .map(crate::workspace_intent::completion_mutation::parse_lifecycle)
                .transpose()?;
            if let Some(state) = &input.lifecycle {
                crate::workspace_intent::completion_mutation::lifecycle_install_evidence(
                    state, &original,
                )?;
            }
            install(*input)?
        }
        Input::Skip(input) => skip(input)?,
    };
    Ok(serde_json::to_string(&output)?)
}

fn invalid(message: &str) -> CoreError {
    CoreError::InvalidInput(message.into())
}

fn generation(value: &str) -> Result<i64, CoreError> {
    let parsed = value
        .parse::<i64>()
        .map_err(|_| invalid("invalid selection generation"))?;
    if parsed < 0 || parsed.to_string() != value {
        return Err(invalid("invalid selection generation"));
    }
    Ok(parsed)
}

fn validate_selection(selection: &Selection) -> Result<(), CoreError> {
    validate_phase(&selection.phase)?;
    generation(&selection.generation)?;
    Ok(())
}

fn validate_completion_history(history: &[HistoryItem]) -> Result<(), CoreError> {
    let mut validation_copy = history.to_vec();
    for item in &mut validation_copy {
        if item.status == "completed" && item.completed_at.is_none() {
            item.completed_at.clone_from(&item.ended_at);
        }
        if item.command_id.as_ref().is_some_and(String::is_empty) {
            return Err(invalid("empty completion command identity"));
        }
    }
    validate_history(&validation_copy)?;
    Ok(())
}

fn validate_install(input: &Install) -> Result<(), CoreError> {
    validate_selection(&input.selection)?;
    validate_completion_history(&input.before_history)?;
    validate_completion_history(&input.after_history)?;
    validate_optional_timer(&input.canonical_timer)?;
    validate_ids(&input.pending.command_ids)?;
    validate_ids(&input.pending.sendable_command_ids)?;
    validate_ids(&input.pending.other_operation_ids)?;
    validate_ids(&input.discarded_command_ids)?;
    if input
        .pending
        .sendable_command_ids
        .iter()
        .any(|id| !input.pending.command_ids.contains(id))
    {
        return Err(invalid("sendable completion command is not pending"));
    }
    rollback::validate(input)?;
    if let Some(state) = &input.lifecycle {
        if input.compatibility != Compatibility::PwaRejectedFinish {
            return Err(invalid("completion lifecycle requires PWA install"));
        }
        crate::workspace_intent::completion_mutation::validate_lifecycle(state)?;
    }
    sent::validate(input)
}

fn validate_ids(ids: &[String]) -> Result<(), CoreError> {
    let mut seen = std::collections::BTreeSet::new();
    if ids.iter().any(|id| id.is_empty() || !seen.insert(id)) {
        return Err(invalid("invalid completion operation identities"));
    }
    Ok(())
}

type Bounds = (DateTime<Utc>, DateTime<Utc>);

fn intervals(values: &[Interval]) -> Result<Vec<Bounds>, CoreError> {
    let mut bounds = values
        .iter()
        .map(|v| parse_bounds(&v.start, &v.end))
        .collect::<Result<Vec<_>, _>>()?;
    bounds.sort();
    if bounds.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(invalid("overlapping completion calendar intervals"));
    }
    Ok(bounds)
}

fn day_at(bounds: &[Bounds], at: DateTime<Utc>) -> Result<Bounds, CoreError> {
    bounds
        .iter()
        .copied()
        .find(|(start, end)| at >= *start && at < *end)
        .ok_or_else(|| invalid("missing completion calendar interval"))
}

fn stamp(item: &HistoryItem) -> &str {
    item.completed_at
        .as_deref()
        .or(item.ended_at.as_deref())
        .unwrap_or_default()
}

fn latest(input: &Install) -> Result<Option<&HistoryItem>, CoreError> {
    let mut completed = input
        .after_history
        .iter()
        .filter(|item| item.status == "completed")
        .map(|item| {
            let at = parse_time(stamp(item))?;
            let order = match input.compatibility {
                Compatibility::AppleAp04 => (at.timestamp(), at.timestamp_subsec_nanos()),
                _ => (at.timestamp_millis(), 0),
            };
            Ok((order, item))
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    completed.sort_by(|(left_time, left), (right_time, right)| {
        right_time
            .cmp(left_time)
            .then_with(|| match input.compatibility {
                Compatibility::AppleAp04 => right.timer_id.cmp(&left.timer_id),
                _ => left.timer_id.cmp(&right.timer_id),
            })
    });
    let Some((newest, _)) = completed.first() else {
        return Ok(None);
    };
    Ok(completed
        .iter()
        .take_while(|(at, _)| at == newest)
        .find_map(|(_, item)| {
            let consumed = input
                .before_history
                .iter()
                .any(|before| same_completion(before, item));
            (input.compatibility == Compatibility::AppleAp04 || !consumed).then_some(*item)
        }))
}

fn same_completion(before: &HistoryItem, after: &HistoryItem) -> bool {
    // D03 consumes timer/command/phase identity, not corrected timestamp or history row ID.
    before.status == "completed"
        && before.timer_id == after.timer_id
        && before.phase == after.phase
        && before.command_id.as_deref().unwrap_or(&before.timer_id)
            == after.command_id.as_deref().unwrap_or(&after.timer_id)
}

fn source(item: &HistoryItem) -> Source {
    Source {
        history_id: item.id.clone(),
        timer_id: item.timer_id.clone(),
        command_id: item.command_id.clone(),
        phase: item.phase.clone(),
        occurred_at: stamp(item).into(),
    }
}

fn blocker(input: &Install, output: &Output) -> Option<&'static str> {
    if input
        .canonical_timer
        .as_ref()
        .is_some_and(|t| matches!(t.status.as_str(), "running" | "paused"))
    {
        return Some("activeTimer");
    }
    match input.compatibility {
        Compatibility::AppleAp04 if !input.pending.command_ids.is_empty() => {
            Some("pendingCommands")
        }
        Compatibility::AppleAp04 if output.selection.explicit => Some("explicitSelection"),
        Compatibility::DesktopD03 if !output.advances.is_empty() => Some("provisionalAdvance"),
        Compatibility::DesktopD03
            if !input.pending.sendable_command_ids.is_empty()
                || !input.pending.other_operation_ids.is_empty() =>
        {
            Some("pendingOperations")
        }
        _ => None,
    }
}

fn install(input: Install) -> Result<Output, CoreError> {
    validate_install(&input)?;
    let bounds = intervals(&input.calendar_intervals)?;
    let reference = parse_time(&input.reference_time)?;
    if let Some(context) = &input.sent_context {
        return sent::install(&input, context, &bounds);
    }
    let mut output = rollback::resolve(&input);
    let candidate = latest(&input)?;
    output.source = candidate.map(source);
    if let Some(reason) = blocker(&input, &output) {
        output.reason = reason;
        return Ok(output);
    }
    let Some(item) = candidate else {
        return Ok(output);
    };
    let at = parse_time(stamp(item))?;
    let day = day_at(&bounds, at)?;
    if input.compatibility == Compatibility::AppleAp04 && day_at(&bounds, reference)? != day {
        output.reason = "outsideReferenceDay";
    } else if input.compatibility == Compatibility::DesktopD03
        && output.selection.phase != item.phase
    {
        output.reason = "selectionDiffersFromSource";
    } else {
        output.selection.phase = phase_after(&item.phase, &input.after_history, day)?;
        output.reason = "completionSelected";
    }
    Ok(output)
}

fn skip(input: Skip) -> Result<Output, CoreError> {
    validate_selection(&input.selection)?;
    validate_phase(&input.source_phase)?;
    validate_completion_history(&input.history)?;
    let day = day_at(
        &intervals(&input.calendar_intervals)?,
        parse_time(&input.reference_time)?,
    )?;
    let mut selection = input.selection;
    selection.phase = if input.source_phase != "focus" {
        "focus"
    } else if super::completed_focus_count(&input.history, day)? % 4 == 3 {
        "long_break"
    } else {
        "short_break"
    }
    .into();
    Ok(Output {
        selection,
        source: None,
        reason: "skipSelected",
        advances: vec![],
        retired_advance_ids: vec![],
        rolled_back_advance_ids: vec![],
        lifecycle: None,
    })
}

#[cfg(test)]
mod shape_guards {
    use crate::completion_schema::{self as schema, tests::assert_fields};

    #[test]
    fn install_decoder_fields_require_shared_shape_guards() {
        assert_fields::<super::Install>(&schema::INSTALL, &["lifecycle"]);
        assert_fields::<super::Selection>(&schema::SELECTION, &[]);
        assert_fields::<super::Pending>(&schema::PENDING, &[]);
        assert_fields::<super::Interval>(&schema::INTERVAL, &[]);
        assert_fields::<super::rollback::Advance>(&schema::ADVANCE, &[]);
        assert_fields::<super::rollback::Acknowledgement>(&schema::ACK, &[]);
        assert_fields::<super::sent::SentCommand>(&schema::SENT_COMMAND, &[]);
    }
}
