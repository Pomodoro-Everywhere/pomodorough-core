use serde::Deserialize;
use serde_json::{Value, json};

use super::acknowledgements::PendingQueues;
use super::{
    LocalQueues, RequiredNullable, TimerDependency, delivery, timer_dependencies, validation,
};
use crate::CoreError;

pub(crate) mod bootstrap;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    base: Value,
    local: LocalQueues,
    canonical_head: RequiredNullable<Head>,
    #[serde(default = "empty_proof")]
    never_sent: Value,
    timer_dependencies: Vec<TimerDependency>,
    now: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Head {
    wall_ms: i64,
    counter: i64,
}

fn empty_proof() -> Value {
    json!({})
}

pub(crate) fn project_json(input: &str) -> Result<String, CoreError> {
    let raw = crate::strict_json::parse(input)?;
    validate_shape(&raw)?;
    let input: Input = serde_json::from_value(raw.clone())?;
    let head = input
        .canonical_head
        .0
        .map(|head| (head.wall_ms, head.counter));
    if let Some((wall, counter)) = head {
        crate::clock::validate_hlc_values(wall, counter)?;
    }
    validation::local_queue_ids(&input.local)?;
    super::clocks::validate_local(&input.local)?;
    timer_dependencies::validate_retained(&input.local.commands, &input.timer_dependencies)?;
    let policy = delivery::Policy::from_queues(&raw["local"], &json!({}), &input.never_sent)?;
    let pending = pending_queues(input.local);
    let projected = policy.project_queues(&pending, head)?;
    let safe = raw_projection_queues(&raw["local"], &projected)?;
    let mut projection = json!({"base": input.base, "pending": raw["local"], "now": input.now});
    // Validate every retained payload through the existing production reducers,
    // including domains excluded from display. Filtering must not hide corruption.
    let complete = crate::projection::apply_workspace_json(&projection.to_string())?;
    let workspace = if safe == raw["local"] {
        complete
    } else {
        projection["pending"] = safe.clone();
        crate::projection::apply_workspace_json(&projection.to_string())?
    };
    Ok(serde_json::to_string(&json!({
        "projectionPending": safe,
        "workspace": serde_json::from_str::<Value>(&workspace)?
    }))?)
}

fn pending_queues(local: LocalQueues) -> PendingQueues {
    PendingQueues {
        commands: local.commands,
        tasks: local.task_operations,
        durations: local.duration_operations,
        auto_start: local.auto_start_operations,
        selected_task: local.selected_task_operations,
    }
}

fn raw_projection_queues(local: &Value, projected: &PendingQueues) -> Result<Value, CoreError> {
    let mut safe = serde_json::to_value(projected)?;
    for name in delivery::QUEUES {
        if safe[name].as_array().is_some_and(|queue| !queue.is_empty()) {
            // Eligibility is whole-domain. Preserve extensions and omission/null
            // distinctions in the exact input objects, even for never-sent work.
            safe[name] = local[name].clone();
        }
    }
    Ok(safe)
}

pub(crate) fn validate_shape(raw: &Value) -> Result<(), CoreError> {
    let root = crate::strict_json::object(raw, "workspace")?;
    if !root.contains_key("canonicalHead") {
        return Err(CoreError::MissingProjection("canonicalHead"));
    }
    let base = crate::strict_json::object_field(root, "base", "base")?;
    for name in [
        "canonicalTimer",
        "history",
        "tasks",
        "durationsMs",
        "autoStartBreaks",
        "selectedTaskId",
    ] {
        if !base.contains_key(name) {
            return Err(CoreError::InvalidInput(format!("missing base.{name}")));
        }
    }
    let local = crate::strict_json::object_field(root, "local", "local")?;
    if local
        .keys()
        .any(|name| !delivery::QUEUES.contains(&name.as_str()))
    {
        return Err(CoreError::InvalidInput("unknown local queue".into()));
    }
    for name in delivery::QUEUES {
        crate::strict_json::object_array_field(local, name, &format!("local.{name}"), true)?;
    }
    crate::strict_json::object_array_field(root, "timerDependencies", "timerDependencies", true)
}
