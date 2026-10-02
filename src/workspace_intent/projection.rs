use serde_json::{Value, json};

use super::model::{Compatibility, Input, Observation, ReplicationMode};
use super::{CoreError, admission};

mod observation;
pub(super) use observation::{ReplayDomains, observed};

pub(super) fn project(workspace: &Value, now: &str) -> Result<Value, CoreError> {
    let mut request = workspace.clone();
    request["now"] = json!(now);
    let output = crate::reconciliation::workspace::project_json(&request.to_string())?;
    Ok(serde_json::from_str(&output)?)
}

pub(super) fn before(input: &Input, replay_at: Option<&str>) -> Result<Value, CoreError> {
    let wire = project(&input.workspace, &input.clock.occurred_at)?;
    // Manual Apple mutations use the centralized non-expiring presentation path.
    // Other adapters refresh their workspace at the supplied observation time.
    let replay = if input.compatibility == Compatibility::AppleWorkspace
        && input.replication_mode == ReplicationMode::Centralized
    {
        observation::apple_replay_time(
            &input.workspace,
            &wire["projectionPending"],
            &input.observation,
        )?
    } else {
        replay_at.unwrap_or(&input.clock.physical_now).to_owned()
    };
    Ok(observed(
        &input.workspace,
        &input.observation,
        input.compatibility,
        ReplayDomains::Safe,
        &replay,
    )?["workspace"]
        .clone())
}

pub(super) fn entrypoint(
    input: &Input,
    safe: &Value,
    observation: &Observation,
    now: &str,
) -> Result<Value, CoreError> {
    let retained = match input.compatibility {
        // Android's coordinator receives all Room queues. Desktop and PWA
        // legacy readers replay local queues when no canonical head is installed.
        Compatibility::AndroidCoordinator => true,
        Compatibility::DesktopStorage
        | Compatibility::DesktopTerminal
        | Compatibility::PwaStorage => input.workspace["canonicalHead"].is_null(),
        Compatibility::AppleWorkspace => false,
    };
    if !retained {
        return Ok(safe.clone());
    }
    let replay_at = if input.compatibility == Compatibility::PwaStorage
        && input.clock.monotonic_now_ms.is_some()
        && now == input.clock.physical_now
    {
        let unexpired = observed(
            &input.workspace,
            observation,
            input.compatibility,
            ReplayDomains::Retained,
            "1970-01-01T00:00:00Z",
        )?;
        super::monotonic::entrypoint_time(
            input,
            &unexpired["workspace"]["canonicalTimer"],
            observation,
        )?
    } else {
        now.to_owned()
    };
    Ok(observed(
        &input.workspace,
        observation,
        input.compatibility,
        ReplayDomains::Retained,
        &replay_at,
    )?["workspace"]
        .clone())
}

pub(super) fn after_time(input: &Input, commands: &[Value]) -> String {
    if input.compatibility == Compatibility::PwaStorage {
        return commands.last().unwrap()["occurredAt"]
            .as_str()
            .unwrap()
            .into();
    }
    input.clock.physical_now.clone()
}

pub(super) fn after(
    input: &Input,
    before: &Value,
    workspace: &Value,
    observation: &Observation,
    commands: &[Value],
) -> Result<Value, CoreError> {
    if commands.is_empty() {
        return Ok(json!({"workspace": before}));
    }
    let mut result = observed(
        workspace,
        observation,
        input.compatibility,
        ReplayDomains::Safe,
        &after_time(input, commands),
    )?;
    result["admission"] = admission::commands(
        input,
        workspace,
        commands,
        observation,
        &after_time(input, commands),
    )?;
    Ok(result)
}
