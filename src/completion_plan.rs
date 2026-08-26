use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::CoreError;
use crate::timer::{
    CanonicalTimer, HistoryItem, parse_time, validate_canonical_timer, validate_history,
};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum PlanInput {
    Expiry(ExpiryInput),
    CommandRequest(CommandRequestContext),
    FinishApplied(FinishAppliedInput),
    GeneratedBreak(GeneratedBreakInput),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ownership {
    timer_id: String,
    owner_device_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompletionSource {
    command_id: String,
    timer_id: String,
    phase: String,
    occurred_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompletionIdentity {
    command_id: String,
    timer_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Projection {
    canonical_timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompletionPlan {
    expired: bool,
    command_eligible: bool,
    reserve_generated_break: bool,
    selected_phase: Option<String>,
    queue_auto_break: bool,
    generated_break_eligible: bool,
    generated_break_phase: Option<String>,
    source_already_accepted: bool,
}

pub(crate) fn plan_v1_json(input: &str) -> Result<String, CoreError> {
    let input: PlanInput = serde_json::from_str(input)?;
    Ok(serde_json::to_string(&plan(input)?)?)
}

fn plan(input: PlanInput) -> Result<CompletionPlan, CoreError> {
    match input {
        PlanInput::Expiry(input) => expiry_plan(input),
        PlanInput::CommandRequest(input) => command_request_plan(input),
        PlanInput::FinishApplied(input) => finish_applied_plan(input),
        PlanInput::GeneratedBreak(input) => generated_break_plan(input),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExpiryInput {
    before_timer: Option<CanonicalTimer>,
    projected_timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    selected_phase: String,
    auto_start_breaks: bool,
    local_device_id: String,
    ownership: Option<Ownership>,
    day_start: String,
    day_end: String,
}

fn expiry_plan(context: ExpiryInput) -> Result<CompletionPlan, CoreError> {
    validate_optional_timer(&context.before_timer)?;
    validate_projection(&context.projected_timer, &context.history)?;
    validate_phase(&context.selected_phase)?;
    validate_identity(&context.local_device_id, context.ownership.as_ref())?;
    let expired = exact_expiry(&context.before_timer, &context.projected_timer);
    let Some(timer) = context.projected_timer.as_ref().filter(|_| expired) else {
        return Ok(CompletionPlan::default());
    };
    let bounds = parse_bounds(&context.day_start, &context.day_end)?;
    let phase = phase_after(&timer.phase, &context.history, bounds)?;
    let selected_phase = (context.selected_phase == timer.phase).then(|| phase.clone());
    let generated_break_phase = (timer.phase == "focus"
        && context.auto_start_breaks
        && owns(
            timer.id.as_str(),
            &context.local_device_id,
            context.ownership.as_ref(),
        ))
    .then_some(phase);
    Ok(CompletionPlan {
        expired: true,
        selected_phase,
        generated_break_phase,
        ..CompletionPlan::default()
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CommandRequestContext {
    command_type: String,
    requested_timer: Option<CanonicalTimer>,
    projected_timer: Option<CanonicalTimer>,
    automatic: bool,
    generate_auto_break: bool,
    auto_start_breaks: bool,
    local_device_id: String,
    ownership: Option<Ownership>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FinishAppliedInput {
    source: CompletionSource,
    history: Vec<HistoryItem>,
    auto_start_breaks: bool,
    local_device_id: String,
    ownership: Option<Ownership>,
    day_start: String,
    day_end: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GeneratedBreakInput {
    source: CompletionIdentity,
    canonical: Projection,
    optimistic: Projection,
    source_finish_pending: bool,
    require_canonical: bool,
    day_start: String,
    day_end: String,
}

fn command_request_plan(context: CommandRequestContext) -> Result<CompletionPlan, CoreError> {
    validate_optional_timer(&context.requested_timer)?;
    validate_optional_timer(&context.projected_timer)?;
    validate_identity(&context.local_device_id, context.ownership.as_ref())?;
    if context.automatic && context.command_type != "finish" {
        return Err(CoreError::InvalidInput(
            "only finish can be queued automatically".into(),
        ));
    }
    let timer = if context.automatic {
        context.projected_timer.as_ref()
    } else {
        context.requested_timer.as_ref()
    };
    let eligible = command_eligible(&context, timer);
    let reserve = eligible
        && context.generate_auto_break
        && context.command_type == "finish"
        && timer.is_some_and(|value| {
            value.phase == "focus"
                && context.auto_start_breaks
                && owns(
                    &value.id,
                    &context.local_device_id,
                    context.ownership.as_ref(),
                )
        });
    Ok(CompletionPlan {
        command_eligible: eligible,
        reserve_generated_break: reserve,
        ..CompletionPlan::default()
    })
}

fn command_eligible(context: &CommandRequestContext, timer: Option<&CanonicalTimer>) -> bool {
    if !context.automatic {
        return true;
    }
    if context.command_type != "finish" {
        return false;
    }
    let Some((requested, projected)) = context
        .requested_timer
        .as_ref()
        .zip(context.projected_timer.as_ref())
    else {
        return false;
    };
    requested.id == projected.id
        && projected.status == "completed"
        && projected
            .last_intent
            .as_ref()
            .is_none_or(|intent| intent.kind != "finish")
        && timer.is_some_and(|value| {
            owns(
                &value.id,
                &context.local_device_id,
                context.ownership.as_ref(),
            )
        })
}

fn finish_applied_plan(input: FinishAppliedInput) -> Result<CompletionPlan, CoreError> {
    validate_source(&input.source)?;
    validate_history(&input.history)?;
    validate_identity(&input.local_device_id, input.ownership.as_ref())?;
    let bounds = parse_bounds(&input.day_start, &input.day_end)?;
    let selected_phase = phase_after(&input.source.phase, &input.history, bounds)?;
    let queue_auto_break = input.source.phase == "focus"
        && input.auto_start_breaks
        && owns(
            &input.source.timer_id,
            &input.local_device_id,
            input.ownership.as_ref(),
        );
    Ok(CompletionPlan {
        selected_phase: Some(selected_phase),
        queue_auto_break,
        ..CompletionPlan::default()
    })
}

fn generated_break_plan(input: GeneratedBreakInput) -> Result<CompletionPlan, CoreError> {
    validate_source_identity(&input.source)?;
    validate_projection(&input.canonical.canonical_timer, &input.canonical.history)?;
    validate_projection(&input.optimistic.canonical_timer, &input.optimistic.history)?;
    let accepted =
        !input.source_finish_pending && projection_has_source(&input.canonical, &input.source);
    let projection = if input.require_canonical || accepted {
        &input.canonical
    } else {
        &input.optimistic
    };
    let eligible = projection_timer_is_source(projection, &input.source);
    let bounds = parse_bounds(&input.day_start, &input.day_end)?;
    let phase = projection_has_source(projection, &input.source)
        .then(|| break_phase(&projection.history, bounds))
        .transpose()?;
    Ok(CompletionPlan {
        generated_break_eligible: eligible,
        generated_break_phase: phase,
        source_already_accepted: accepted,
        ..CompletionPlan::default()
    })
}

fn projection_has_source(projection: &Projection, source: &CompletionIdentity) -> bool {
    projection_timer_is_source(projection, source)
        && projection.history.iter().any(|item| {
            item.timer_id == source.timer_id
                && item.command_id.as_deref() == Some(source.command_id.as_str())
                && item.phase == "focus"
                && item.status == "completed"
        })
}

fn projection_timer_is_source(projection: &Projection, source: &CompletionIdentity) -> bool {
    projection.canonical_timer.as_ref().is_some_and(|timer| {
        timer.id == source.timer_id && timer.phase == "focus" && timer.status == "completed"
    })
}

fn exact_expiry(before: &Option<CanonicalTimer>, projected: &Option<CanonicalTimer>) -> bool {
    before
        .as_ref()
        .zip(projected.as_ref())
        .is_some_and(|(before, after)| {
            before.status == "running" && before.id == after.id && after.status == "completed"
        })
}

fn owns(timer_id: &str, local_device_id: &str, ownership: Option<&Ownership>) -> bool {
    ownership
        .is_some_and(|value| value.timer_id == timer_id && value.owner_device_id == local_device_id)
}

fn phase_after(
    phase: &str,
    history: &[HistoryItem],
    bounds: (DateTime<Utc>, DateTime<Utc>),
) -> Result<String, CoreError> {
    validate_phase(phase)?;
    if phase == "focus" {
        break_phase(history, bounds)
    } else {
        Ok("focus".into())
    }
}

fn break_phase(
    history: &[HistoryItem],
    (day_start, day_end): (DateTime<Utc>, DateTime<Utc>),
) -> Result<String, CoreError> {
    let mut completed = 0;
    for item in history
        .iter()
        .filter(|item| item.phase == "focus" && item.status == "completed")
    {
        let timestamp = item.completed_at.as_deref().or(item.ended_at.as_deref());
        let Some(timestamp) = timestamp else { continue };
        let completed_at = parse_time(timestamp)?;
        completed += usize::from(completed_at >= day_start && completed_at < day_end);
    }
    Ok(if completed > 0 && completed % 4 == 0 {
        "long_break"
    } else {
        "short_break"
    }
    .into())
}

fn parse_bounds(start: &str, end: &str) -> Result<(DateTime<Utc>, DateTime<Utc>), CoreError> {
    let bounds = (parse_time(start)?, parse_time(end)?);
    if bounds.0 >= bounds.1 {
        return Err(CoreError::InvalidInput(
            "invalid completion day bounds".into(),
        ));
    }
    Ok(bounds)
}

fn validate_projection(
    timer: &Option<CanonicalTimer>,
    history: &[HistoryItem],
) -> Result<(), CoreError> {
    validate_optional_timer(timer)?;
    validate_history(history)?;
    Ok(())
}

fn validate_optional_timer(timer: &Option<CanonicalTimer>) -> Result<(), CoreError> {
    if let Some(timer) = timer {
        validate_canonical_timer(timer)?;
    }
    Ok(())
}

fn validate_source(source: &CompletionSource) -> Result<(), CoreError> {
    if source.command_id.is_empty() || source.timer_id.is_empty() || source.phase.is_empty() {
        return Err(CoreError::InvalidInput("invalid completion source".into()));
    }
    validate_phase(&source.phase)?;
    parse_time(&source.occurred_at)?;
    Ok(())
}

fn validate_source_identity(source: &CompletionIdentity) -> Result<(), CoreError> {
    if source.command_id.is_empty() || source.timer_id.is_empty() {
        return Err(CoreError::InvalidInput("invalid completion source".into()));
    }
    Ok(())
}

fn validate_identity(
    local_device_id: &str,
    ownership: Option<&Ownership>,
) -> Result<(), CoreError> {
    if local_device_id.is_empty()
        || ownership
            .is_some_and(|value| value.timer_id.is_empty() || value.owner_device_id.is_empty())
    {
        return Err(CoreError::InvalidInput(
            "invalid completion ownership".into(),
        ));
    }
    Ok(())
}

fn validate_phase(phase: &str) -> Result<(), CoreError> {
    if !matches!(phase, "focus" | "short_break" | "long_break") {
        return Err(CoreError::InvalidInput("invalid completion phase".into()));
    }
    Ok(())
}
