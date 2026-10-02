use serde::Deserialize;

use super::rollback::Outcome;
use super::*;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum Context {
    Android {
        commands: Vec<SentCommand>,
        #[serde(rename = "selectionAtSend")]
        selection_at_send: Option<CapturedSelection>,
        #[serde(rename = "acknowledgementHistory")]
        acknowledgement_history: Vec<HistoryItem>,
        #[serde(rename = "acknowledgementTimer", deserialize_with = "required_timer")]
        acknowledgement_timer: Box<Option<CanonicalTimer>>,
        #[serde(rename = "nextProjectionTimer", deserialize_with = "required_timer")]
        next_projection_timer: Box<Option<CanonicalTimer>>,
    },
    Pwa {
        commands: Vec<SentCommand>,
        #[serde(rename = "rollbackHistory")]
        rollback_history: Vec<HistoryItem>,
    },
}

// A custom deserializer prevents Serde's missing-field Option fallback.
fn required_timer<'de, D>(deserializer: D) -> Result<Box<Option<CanonicalTimer>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<CanonicalTimer>::deserialize(deserializer).map(Box::new)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CapturedSelection {
    phase: String,
    generation: String,
}

// These are raw wire command fields used by selection, not a caller-computed plan.
// Other command fields retain the existing wire schema's extensibility.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SentCommand {
    id: String,
    timer_id: String,
    #[serde(rename = "type")]
    kind: String,
    phase: String,
    device_sequence: u64,
    occurred_at: String,
    physical_occurred_at: Option<String>,
}

pub(super) fn validate(input: &Install) -> Result<(), CoreError> {
    let commands = match (&input.compatibility, &input.sent_context) {
        (
            Compatibility::AndroidCapturedSend,
            Some(Context::Android {
                commands,
                selection_at_send,
                acknowledgement_history,
                acknowledgement_timer,
                next_projection_timer,
            }),
        ) => {
            if let Some(selection) = selection_at_send {
                validate_phase(&selection.phase)?;
                generation(&selection.generation)?;
            }
            validate_completion_history(acknowledgement_history)?;
            validate_optional_timer(acknowledgement_timer)?;
            validate_optional_timer(next_projection_timer)?;
            commands
        }
        (
            Compatibility::PwaRejectedFinish,
            Some(Context::Pwa {
                commands,
                rollback_history,
            }),
        ) => {
            validate_completion_history(rollback_history)?;
            commands
        }
        (Compatibility::AppleAp04 | Compatibility::DesktopD03, None) => return Ok(()),
        _ => return Err(invalid("completion profile requires matching sent context")),
    };
    if !input.advances.is_empty() {
        return Err(invalid(
            "sent completion profiles do not consume durable advances",
        ));
    }
    validate_ids(&commands.iter().map(|c| c.id.clone()).collect::<Vec<_>>())?;
    for command in commands {
        validate_phase(&command.phase)?;
        parse_time(&command.occurred_at)?;
        if command.timer_id.is_empty() || command.device_sequence > 9_007_199_254_740_991 {
            return Err(invalid("invalid sent completion command"));
        }
    }
    Ok(())
}

pub(super) fn install(
    input: &Install,
    context: &Context,
    bounds: &[Bounds],
) -> Result<Output, CoreError> {
    let mut output = Output {
        selection: input.selection.clone(),
        source: None,
        reason: "noAcknowledgedFinish",
        advances: vec![],
        retired_advance_ids: vec![],
        rolled_back_advance_ids: vec![],
    };
    if selection_changed(context, &input.selection) {
        output.reason = "selectionChangedSinceSend";
        return Ok(output);
    }
    for finish in ordered_finishes(context) {
        let Some(ack) = input
            .acknowledgements
            .iter()
            .find(|a| a.command_id == finish.id)
        else {
            continue;
        };
        let phase = match context {
            Context::Android { .. } => android_phase(
                input,
                context,
                finish,
                ack.outcome,
                &output.selection.phase,
                bounds,
            )?,
            Context::Pwa {
                rollback_history, ..
            } => pwa_phase(
                rollback_history,
                finish,
                ack.outcome,
                &output.selection.phase,
                bounds,
            )?,
        };
        output.selection.phase = phase;
        output.reason = "sentFinishesReconciled";
    }
    Ok(output)
}

fn selection_changed(context: &Context, selection: &Selection) -> bool {
    match context {
        Context::Android {
            selection_at_send, ..
        } => selection_at_send.as_ref().is_some_and(|sent| {
            sent.phase != selection.phase || sent.generation != selection.generation
        }),
        Context::Pwa { .. } => false,
    }
}

fn ordered_finishes(context: &Context) -> Vec<&SentCommand> {
    let (Context::Android { commands, .. } | Context::Pwa { commands, .. }) = context;
    let mut finishes: Vec<_> = commands.iter().filter(|c| c.kind == "finish").collect();
    match context {
        Context::Android { .. } => finishes.sort_by_key(|c| (c.device_sequence, &c.id)),
        Context::Pwa { .. } => finishes.sort_by_key(|c| std::cmp::Reverse(c.device_sequence)),
    }
    finishes
}

fn completed(history: &[HistoryItem], timer: &Option<CanonicalTimer>, id: &str) -> bool {
    history
        .iter()
        .any(|h| h.timer_id == id && h.status == "completed")
        || timer
            .as_ref()
            .is_some_and(|t| t.id == id && t.status == "completed")
}

fn android_phase(
    input: &Install,
    context: &Context,
    finish: &SentCommand,
    outcome: Outcome,
    current: &str,
    bounds: &[Bounds],
) -> Result<String, CoreError> {
    let Context::Android {
        acknowledgement_history,
        acknowledgement_timer,
        next_projection_timer,
        ..
    } = context
    else {
        unreachable!("Android branch requires Android context")
    };
    let evidence = completed(
        acknowledgement_history,
        acknowledgement_timer,
        &finish.timer_id,
    );
    if outcome != Outcome::Applied && !evidence {
        return Ok(input
            .canonical_timer
            .as_ref()
            .filter(|t| t.id == finish.timer_id)
            .map_or(&finish.phase, |t| &t.phase)
            .clone());
    }
    if evidence {
        let history = android_history(input, finish);
        validate_history(&history)?;
        let reference = android_reference(input, finish)?;
        return phase_after(&finish.phase, &history, day_at(bounds, reference)?);
    }
    Ok(next_projection_timer
        .as_ref()
        .as_ref()
        .filter(|t| {
            t.last_intent
                .as_ref()
                .is_some_and(|i| i.command_id == finish.id)
        })
        .map_or(current, |t| &t.phase)
        .into())
}

fn android_history(input: &Install, finish: &SentCommand) -> Vec<HistoryItem> {
    let mut history = input.after_history.clone();
    if history
        .iter()
        .any(|h| h.timer_id == finish.timer_id && h.status == "completed")
    {
        return history;
    }
    if let Some(timer) = input
        .canonical_timer
        .as_ref()
        .filter(|t| t.id == finish.timer_id && t.status == "completed")
    {
        history.push(HistoryItem {
            id: format!("canonical-completion:{}", timer.id),
            timer_id: timer.id.clone(),
            command_id: Some(finish.id.clone()),
            phase: timer.phase.clone(),
            status: timer.status.clone(),
            planned_duration_ms: timer.planned_duration_ms,
            completed_at: Some(timer.anchor_at.clone()),
            ended_at: Some(timer.anchor_at.clone()),
            task_id: timer.task_id.clone(),
        });
    }
    history
}

fn android_reference(input: &Install, finish: &SentCommand) -> Result<DateTime<Utc>, CoreError> {
    let timestamp = input
        .after_history
        .iter()
        .find(|h| {
            h.timer_id == finish.timer_id
                && h.status == "completed"
                && h.command_id.as_ref().is_none_or(|id| id == &finish.id)
        })
        .and_then(|h| h.completed_at.as_deref().or(h.ended_at.as_deref()))
        .or_else(|| {
            input
                .canonical_timer
                .as_ref()
                .filter(|t| t.id == finish.timer_id && t.status == "completed")
                .map(|t| t.anchor_at.as_str())
        })
        .or(finish.physical_occurred_at.as_deref())
        .unwrap_or(&finish.occurred_at);
    parse_time(timestamp).or_else(|_| parse_time(&finish.occurred_at))
}

fn pwa_phase(
    history: &[HistoryItem],
    finish: &SentCommand,
    outcome: Outcome,
    current: &str,
    bounds: &[Bounds],
) -> Result<String, CoreError> {
    if outcome != Outcome::Rejected {
        return Ok(current.into());
    }
    validate_history(history)?;
    let day = day_at(bounds, parse_time(&finish.occurred_at)?)?;
    let destination = phase_after(&finish.phase, history, day)?;
    Ok(if current == destination {
        &finish.phase
    } else {
        current
    }
    .into())
}
