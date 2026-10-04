use serde_json::{Value, json};

use super::model::{CommandKind as K, Compatibility as C, Input, Intent, Phase, Selection};
use super::{CoreError, invalid};

pub(super) fn commands(input: &Input, timer: &Value) -> Vec<K> {
    if stale_target(input, timer) {
        return vec![];
    }
    let status = timer["status"].as_str().unwrap_or("idle");
    if matches!(input.intent, Intent::SelectPhase { .. } | Intent::Skip) {
        return if matches!(status, "completed" | "cancelled" | "superseded")
            && input.compatibility == C::AppleWorkspace
        {
            vec![K::Clear]
        } else {
            vec![]
        };
    }
    timer_commands(input, status)
}

fn timer_commands(input: &Input, status: &str) -> Vec<K> {
    let active = matches!(status, "running" | "paused");
    let terminal = matches!(status, "completed" | "cancelled" | "superseded");
    match input.intent {
        Intent::Start if start_allowed(input.compatibility, status) => vec![K::Start],
        Intent::Pause if status == "running" => vec![K::Pause],
        Intent::Resume if resume_allowed(input.compatibility, status) => vec![K::Resume],
        Intent::Cancel if active => cancel(input.compatibility),
        Intent::CancelAndClear if active => vec![K::Cancel, K::Clear],
        Intent::CancelAndClear if terminal && terminal_cancel(input.compatibility, status) => {
            vec![K::Clear]
        }
        Intent::Clear if clear_allowed(input.compatibility, status) => vec![K::Clear],
        Intent::Restart
            if terminal
                && matches!(input.compatibility, C::DesktopStorage | C::DesktopTerminal) =>
        {
            restart()
        }
        _ => vec![],
    }
}

pub(super) fn stale_target(input: &Input, timer: &Value) -> bool {
    let Some(requested) = &input.requested_timer else {
        return false;
    };
    if !matches!(input.intent, Intent::Restart | Intent::CancelAndClear)
        && !(input.compatibility == C::DesktopTerminal && matches!(input.intent, Intent::Cancel))
    {
        return false;
    }
    let requested = serde_json::to_value(requested).expect("canonical timer serializes");
    match input.compatibility {
        C::DesktopStorage | C::DesktopTerminal => {
            [
                "id",
                "status",
                "phase",
                "plannedDurationMs",
                "anchorAt",
                "elapsedAtAnchorMs",
                "taskId",
            ]
            .iter()
            .any(|key| requested[*key] != timer[*key])
                || requested["lastIntent"]["commandId"] != timer["lastIntent"]["commandId"]
        }
        C::PwaStorage => requested["id"] != timer["id"] || requested["phase"] != timer["phase"],
        C::AppleWorkspace | C::AndroidCoordinator => false,
    }
}

pub(super) fn target(input: &Input, before: &Value) -> Value {
    if !before["canonicalTimer"].is_null()
        || !matches!(input.intent, Intent::Restart)
        || !matches!(input.compatibility, C::DesktopStorage | C::DesktopTerminal)
        || !input.workspace["local"]["commands"]
            .as_array()
            .is_some_and(Vec::is_empty)
    {
        return before.clone();
    }
    let Some(timer) = input.requested_timer.as_ref() else {
        return before.clone();
    };
    if !matches!(
        timer.status.as_str(),
        "completed" | "cancelled" | "superseded"
    ) || !input.workspace["base"]["history"]
        .as_array()
        .is_some_and(|history| history.iter().any(|item| item["timerId"] == timer.id))
    {
        return before.clone();
    }
    let mut selected = before.clone();
    selected["canonicalTimer"] = serde_json::to_value(timer).expect("validated timer serializes");
    selected
}

pub(super) fn validate_target(input: &Input, timer: &Value) -> Result<(), CoreError> {
    if timer.is_null()
        || !matches!(input.intent, Intent::Restart | Intent::CancelAndClear)
            && !(input.compatibility == C::DesktopTerminal
                && matches!(input.intent, Intent::Cancel))
    {
        return Ok(());
    }
    if (matches!(input.compatibility, C::DesktopStorage | C::DesktopTerminal)
        || input.compatibility == C::PwaStorage && matches!(input.intent, Intent::CancelAndClear))
        && input.requested_timer.is_none()
    {
        return Err(invalid("grouped timer intent requires requestedTimer"));
    }
    Ok(())
}

fn start_allowed(compatibility: C, status: &str) -> bool {
    match compatibility {
        C::AppleWorkspace => !matches!(status, "running" | "paused"),
        C::DesktopTerminal => status == "idle",
        C::AndroidCoordinator | C::DesktopStorage | C::PwaStorage => true,
    }
}

fn resume_allowed(compatibility: C, status: &str) -> bool {
    status == "paused" || status == "superseded" && compatibility == C::AndroidCoordinator
}

fn clear_allowed(compatibility: C, status: &str) -> bool {
    match compatibility {
        C::AndroidCoordinator => matches!(status, "completed" | "cancelled"),
        _ => matches!(status, "completed" | "cancelled" | "superseded"),
    }
}

fn terminal_cancel(compatibility: C, status: &str) -> bool {
    matches!(compatibility, C::AndroidCoordinator | C::PwaStorage)
        && matches!(status, "completed" | "cancelled")
}

fn cancel(compatibility: C) -> Vec<K> {
    match compatibility {
        C::AppleWorkspace | C::DesktopTerminal => vec![K::Cancel, K::Clear],
        _ => vec![K::Cancel],
    }
}

fn restart() -> Vec<K> {
    vec![K::Clear, K::Start]
}

pub(super) fn selection(input: &Input, workspace: &Value) -> Result<Selection, CoreError> {
    if let Some(selection) = super::pwa_selection::choice(input, workspace)? {
        return Ok(selection);
    }
    let phase = match input.intent {
        Intent::SelectPhase { phase } => phase,
        Intent::Skip => skip(input, workspace)?,
        _ => return Ok(input.selection.clone()),
    };
    let active = matches!(
        workspace["canonicalTimer"]["status"].as_str(),
        Some("running" | "paused")
    );
    let blocked = active
        && matches!(
            input.compatibility,
            C::AndroidCoordinator | C::DesktopTerminal
        );
    if blocked || input.compatibility == C::AndroidCoordinator && phase == input.selection.phase {
        return Ok(input.selection.clone());
    }
    let mut selection = input.selection.clone();
    selection.phase = phase;
    selection.generation = next_generation(input)?;
    if input.compatibility == C::AppleWorkspace {
        selection.explicit = true;
    }
    Ok(selection)
}

pub(super) fn skip(input: &Input, workspace: &Value) -> Result<Phase, CoreError> {
    if input.compatibility != C::AppleWorkspace && input.lifecycle.is_none() {
        return Err(invalid(
            "skip is not available for this compatibility profile",
        ));
    }
    let request = json!({"kind": "skip", "selection": {"phase": input.selection.phase, "generation": "0", "explicit": false},
        "sourcePhase": input.selection.phase, "history": workspace["history"],
        "referenceTime": input.clock.occurred_at, "calendarIntervals": input.calendar_intervals});
    let result = crate::completion_plan::state::plan_json(&request.to_string())?;
    let result: Value = serde_json::from_str(&result)?;
    Ok(serde_json::from_value(
        result["selection"]["phase"].clone(),
    )?)
}

pub(super) fn validate_generation(input: &Input) -> Result<(), CoreError> {
    let value = &input.selection.generation;
    match input.compatibility {
        C::DesktopStorage | C::DesktopTerminal => {
            if value.is_empty()
                || !value.bytes().all(|v| v.is_ascii_digit())
                || value.len() > 1 && value.starts_with('0')
            {
                return Err(invalid("invalid decimal generation"));
            }
        }
        _ => {
            let parsed = value
                .parse::<i64>()
                .map_err(|_| invalid("invalid generation"))?;
            if parsed.to_string() != *value
                || parsed < 0 && input.compatibility != C::AndroidCoordinator
            {
                return Err(invalid("invalid generation"));
            }
        }
    }
    Ok(())
}

pub(super) fn no_change_reason(input: &Input, before: &Value) -> &'static str {
    if stale_target(input, &before["canonicalTimer"]) {
        return "staleTimer";
    }
    match input.intent {
        Intent::SelectPhase { .. } | Intent::Skip => "selectionUnchangedOrActive",
        _ => match input.compatibility {
            C::DesktopTerminal => "invalidAction",
            C::DesktopStorage | C::PwaStorage => "invalidTransition",
            C::AppleWorkspace | C::AndroidCoordinator => "ineligible",
        },
    }
}

pub(super) fn after_commands(
    input: &Input,
    before: &Value,
    after: &Value,
    commands: &[Value],
    selection: &mut Selection,
) -> Result<(), CoreError> {
    super::pwa_selection::begin_cycle(input, commands, selection);
    if input.compatibility != C::AndroidCoordinator
        || commands.is_empty()
        || !matches!(input.intent, Intent::CancelAndClear)
    {
        return Ok(());
    }
    let source = after["history"].as_array().unwrap().iter().find(|item| {
        item["timerId"] == before["canonicalTimer"]["id"] && item["status"] == "completed"
    });
    let Some(source) = source else {
        return Ok(());
    };
    selection.phase = cancelled_completion_phase(input, source, after)?;
    if selection.phase != input.selection.phase {
        selection.generation = next_generation(input)?;
    }
    Ok(())
}

fn cancelled_completion_phase(
    input: &Input,
    source: &Value,
    after: &Value,
) -> Result<Phase, CoreError> {
    let at = source["completedAt"]
        .as_str()
        .or(source["endedAt"].as_str())
        .unwrap();
    let stamp = crate::timer::parse_time(at)?;
    let mut day = None;
    for interval in &input.calendar_intervals {
        if stamp >= crate::timer::parse_time(&interval.start)?
            && stamp < crate::timer::parse_time(&interval.end)?
        {
            day = Some(interval);
        }
    }
    let day = day.ok_or_else(|| invalid("missing cancellation completion calendar interval"))?;
    let request = json!({"kind": "finishApplied", "source": {
        "commandId": source["commandId"].as_str().unwrap_or("cancelled-completion"),
        "timerId": source["timerId"], "phase": source["phase"], "occurredAt": at},
        "history": after["history"], "autoStartBreaks": false, "localDeviceId": input.allocation.device_id,
        "ownership": null, "dayStart": day.start, "dayEnd": day.end});
    let result: Value =
        serde_json::from_str(&crate::completion_plan::plan_v1_json(&request.to_string())?)?;
    Ok(serde_json::from_value(result["selectedPhase"].clone())?)
}

fn next_generation(input: &Input) -> Result<String, CoreError> {
    let value = &input.selection.generation;
    match input.compatibility {
        C::DesktopStorage | C::DesktopTerminal => {
            let mut digits = value.as_bytes().to_vec();
            for digit in digits.iter_mut().rev() {
                if *digit != b'9' {
                    *digit += 1;
                    return String::from_utf8(digits).map_err(|_| invalid("invalid generation"));
                }
                *digit = b'0';
            }
            Ok(format!("1{}", "0".repeat(digits.len())))
        }
        C::PwaStorage => Ok(value.clone()),
        C::AppleWorkspace => Ok(value
            .parse::<i64>()
            .unwrap()
            .checked_add(1)
            .unwrap_or(0)
            .to_string()),
        C::AndroidCoordinator => Ok(value.parse::<i64>().unwrap().wrapping_add(1).to_string()),
    }
}
