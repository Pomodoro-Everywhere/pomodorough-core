use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::CoreError;

mod classification;
mod horizon;
mod pwa;
mod validation;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Profile {
    AppleWorkspace,
    AndroidRepository,
    DesktopStorage,
    PwaStorage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    profile: Profile,
    current_user_id: Option<String>,
    local: Local,
    remote: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Local {
    owner_id: Option<String>,
    workspace: Value,
    preferences: Value,
    #[serde(default)]
    projection_pending: Option<Value>,
    #[serde(rename = "knownTasks")]
    _known_tasks: Vec<Value>,
}

pub(crate) fn plan_json(raw: &str) -> Result<String, CoreError> {
    let value = crate::strict_json::parse(raw)?;
    validation::request(&value)?;
    let input: Input = serde_json::from_value(value)?;
    let local_timer = project_timer(&input).map_err(|error| {
        CoreError::InvalidInput(format!("bootstrap workspace requires recovery: {error}"))
    })?;
    let local_history = local_timer["history"].as_array().unwrap();
    let remote_history = input.remote["history"].as_array().unwrap();
    let local = classification::local(&input, &local_timer)?;
    let remote = classification::remote(&input)?;
    let plan = super::plan(super::BootstrapPlanInput {
        local_owner_id: input.local.owner_id,
        current_user_id: input.current_user_id,
        local_history: local_history.clone(),
        remote_history: remote_history.clone(),
        has_local_state: local.has_state,
        has_remote_state: remote.has_state,
    })?;
    Ok(serde_json::to_string(
        &json!({"plan": plan, "classification": {
            "profile": input.profile, "local": local, "remote": remote
        }}),
    )?)
}

fn project_timer(input: &Input) -> Result<Value, CoreError> {
    use crate::reconciliation::workspace::bootstrap::{Projection, timer};
    let mut workspace = input.local.workspace.clone();
    crate::reconciliation::workspace::validate_shape(&workspace)?;
    let observed = workspace["now"]
        .as_str()
        .ok_or_else(|| CoreError::InvalidInput("missing workspace.now".into()))?;
    crate::timer::parse_time(observed)?;
    if input.profile == Profile::AndroidRepository {
        workspace["now"] = json!(horizon::android(&workspace)?);
    }
    let projection = match input.profile {
        Profile::AppleWorkspace => Projection::DeliverySafe,
        Profile::AndroidRepository | Profile::DesktopStorage => Projection::Retained,
        Profile::PwaStorage => Projection::Stored(pwa::commands(&input.local)?),
    };
    timer(&workspace, projection)
}
