use serde_json::{Value, json};

use super::{CoreError, Request, lifecycle};
use crate::workspace_intent::model::{Compatibility, Input};

pub(super) fn plan(
    request: &Request,
    input: &Input,
    before: &Value,
    dependency: Option<&Value>,
) -> Result<String, CoreError> {
    let timer = &before["canonicalTimer"];
    if crate::timer::workspace::missing_natural_history(&input.workspace, timer) {
        return super::noop(request, "staleTimer");
    }
    let canonical = serde_json::from_value(timer.clone())?;
    let history =
        serde_json::from_value::<Vec<crate::timer::HistoryItem>>(before["history"].clone())?;
    let Some(source) = crate::timer::workspace::natural_completion(Some(&canonical), &history)?
    else {
        return super::noop(request, "staleTimer");
    };
    if presented_stale(request, input, timer)? || superseded_obligation(input, timer)? {
        return super::noop(request, "staleTimer");
    }
    if request.lifecycle.finished(&source.timer_id, &source.phase) {
        return super::noop(request, "alreadyConsumed");
    }
    if crate::timer::parse_time(&input.clock.observed_at)?
        < crate::timer::parse_time(&canonical.anchor_at)?
    {
        return super::noop(request, "notExpired");
    }
    let (owned, retry) = super::automatic_owner(request, timer)?;
    if !owned {
        return super::noop_with_retry(request, "not_owner", retry);
    }
    super::validate_requested_dependency(dependency, input, timer)?;
    let reserve =
        auto_start(request, super::effective_auto_start(input)?) && source.phase == "focus";
    let mut result: Value = serde_json::from_str(&super::commit(
        request,
        input,
        before,
        super::observed(input)?,
        reserve,
    )?)?;
    record(request, input, before, source, &mut result)?;
    Ok(result.to_string())
}

fn record(
    request: &Request,
    input: &Input,
    before: &Value,
    source: &crate::timer::HistoryItem,
    result: &mut Value,
) -> Result<(), CoreError> {
    let mut state = request.lifecycle.clone();
    state.remember(source);
    let admitted = super::admission::commands(
        input,
        &result["workspace"],
        result["commands"].as_array().unwrap(),
        &serde_json::from_value(result["observation"].clone())?,
        &super::projection::after_time(input, result["commands"].as_array().unwrap()),
    )?;
    let finished = super::source(&admitted["workspace"], &result["commands"][0])?;
    state.remember(&serde_json::from_value(finished.clone())?);
    lifecycle::evidence::remember(
        &mut state,
        &result["commands"][0],
        &before["canonicalTimer"],
        &serde_json::to_value(source)?,
    )?;
    lifecycle::validate_state(&state)?;
    result["lifecycle"] = serde_json::to_value(state)?;
    result["source"] = serde_json::to_value(source)?;
    result["sourceStatus"] = json!("naturalExpiry");
    Ok(())
}

fn queued_finish(input: &Input, timer: &Value) -> bool {
    // Delivery-safe display can suppress a claimed Finish. Its retained bytes
    // still discharge the local command obligation and must never be replaced.
    input.workspace["local"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["timerId"] == timer["id"] && command["type"] == "finish")
}

fn presented_stale(request: &Request, input: &Input, timer: &Value) -> Result<bool, CoreError> {
    if request.lifecycle.has_finish_evidence() && request.requested_timer.status == "running" {
        return super::stale(request, &super::unexpired(input)?["canonicalTimer"]);
    }
    super::stale(request, timer)
}

fn superseded_obligation(input: &Input, timer: &Value) -> Result<bool, CoreError> {
    if queued_finish(input, timer) {
        return Ok(true);
    }
    let retained = super::admission::group(
        input,
        &input.workspace,
        &json!({}),
        &input.observation,
        &input.clock.physical_now,
    )?;
    let current = &retained["workspace"]["canonicalTimer"];
    Ok(current["id"] != timer["id"]
        || current["status"] != "completed"
        || !crate::timer::workspace::same_intent(&current["lastIntent"], &timer["lastIntent"]))
}

pub(super) fn auto_start(request: &Request, enabled: bool) -> bool {
    enabled && !(is_natural(request) && request.selection.explicit)
}

pub(super) fn advance_selection(request: &Request) -> bool {
    !is_natural(request)
        || (!request.selection.explicit
            && request.selection.phase.name() == request.requested_timer.phase
            && !request
                .lifecycle
                .consumed(&request.requested_timer.id, &request.requested_timer.phase))
}

fn is_natural(request: &Request) -> bool {
    request.compatibility == Compatibility::PwaStorage
        && (request.requested_timer.status == "completed"
            || request.lifecycle.has_finish_evidence())
        && request
            .requested_timer
            .last_intent
            .as_ref()
            .is_some_and(|intent| matches!(intent.kind.as_str(), "start" | "resume"))
}
