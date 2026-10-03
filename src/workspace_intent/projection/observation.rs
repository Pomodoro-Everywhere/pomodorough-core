use serde_json::{Value, json};

use super::super::model::{Compatibility, Observation};
use super::{CoreError, project};

pub(in crate::workspace_intent) enum ReplayDomains {
    Safe,
    Retained,
}

pub(in crate::workspace_intent) fn observed(
    workspace: &Value,
    observation: &Observation,
    profile: Compatibility,
    domains: ReplayDomains,
    now: &str,
) -> Result<Value, CoreError> {
    crate::reconciliation::workspace::display::validate_profile(
        workspace,
        profile == Compatibility::PwaStorage,
    )?;
    let wire = project(workspace, now)?;
    let pending = match domains {
        ReplayDomains::Safe => wire.get("displayContext").map_or(
            &wire["projectionPending"],
            |context| &context["projectionPending"],
        ),
        ReplayDomains::Retained => &workspace["local"],
    };
    let input = json!({"base": workspace["base"], "pending": pending, "now": now});
    let projected = if profile == Compatibility::PwaStorage {
        crate::projection::apply_workspace_json(&input.to_string())?
    } else {
        let observation = crate::timer::workspace::observation::Observation {
            canonical_anchor_at: observation.canonical_anchor_at.as_deref(),
            command_times: &observation.command_times,
        };
        crate::projection::apply_observed_workspace_json(&input.to_string(), &observation)?
    };
    Ok(
        json!({"workspace": serde_json::from_str::<Value>(&projected)?, "projectionPending": pending}),
    )
}

pub(super) fn apple_replay_time(
    workspace: &Value,
    pending: &Value,
    observation: &Observation,
) -> Result<String, CoreError> {
    let last = pending["commands"]
        .as_array()
        .unwrap()
        .iter()
        .max_by_key(|command| {
            (
                command["hlcWallMs"].as_i64(),
                command["hlcCounter"].as_i64(),
                command["deviceId"].as_str(),
                command["id"].as_str(),
            )
        });
    if let Some(command) = last {
        return Ok(observation
            .command_times
            .get(command["id"].as_str().unwrap())
            .map(String::as_str)
            .unwrap_or(command["occurredAt"].as_str().unwrap())
            .into());
    }
    let timer: Option<crate::timer::CanonicalTimer> =
        serde_json::from_value(workspace["base"]["canonicalTimer"].clone())?;
    let Some(timer) = timer else {
        return Ok("1970-01-01T00:00:00Z".into());
    };
    let observation = crate::timer::workspace::observation::Observation {
        canonical_anchor_at: observation.canonical_anchor_at.as_deref(),
        command_times: &observation.command_times,
    };
    Ok(crate::timer::format_time(
        &observation.canonical_anchor(&timer)?,
    ))
}
