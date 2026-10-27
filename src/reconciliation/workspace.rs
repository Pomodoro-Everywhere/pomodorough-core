use serde::Deserialize;
use serde_json::{Value, json};

use super::acknowledgements::PendingQueues;
use super::{
    LocalQueues, RequiredNullable, TimerDependency, delivery, timer_dependencies, validation,
};
use crate::CoreError;

pub(crate) mod bootstrap;
pub(crate) mod display;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    // Moved out before typed decode so the full base history is never
    // duplicated; shape checks still run on the complete request first.
    // The placeholder decodes here but planning uses the moved value.
    base: Value,
    local: LocalQueues,
    canonical_head: RequiredNullable<Head>,
    #[serde(default = "empty_proof")]
    never_sent: Value,
    timer_dependencies: Vec<TimerDependency>,
    now: String,
    #[serde(default, rename = "displayContext")]
    _display_context: Option<Value>,
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
    Ok(project_value(crate::strict_json::parse(input)?)?.to_string())
}

pub(crate) fn project_value(input: Value) -> Result<Value, CoreError> {
    project_observed_value(input, None)
}

pub(crate) fn project_observed_value(
    raw: Value,
    observed: Option<&crate::timer::workspace::retarget_observation::RunningTimer>,
) -> Result<Value, CoreError> {
    let mut raw = raw;
    validate_shape(&raw)?;
    let (base, context) = take_base_and_context(&mut raw)?;
    let local_value = raw["local"].clone();
    let input: Input = serde_json::from_value(raw)?;
    let _ = &input.base;
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
    let policy = delivery::Policy::from_queues(&local_value, &json!({}), &input.never_sent)?;
    let pending = pending_queues(input.local);
    let projected = policy.project_queues(&pending, head)?;
    let safe = raw_projection_queues(&local_value, &projected)?;
    project_display_value(
        DisplayProjection {
            base,
            local: local_value,
            context,
            never_sent: input.never_sent,
            head,
            safe,
        },
        &input.now,
        observed,
    )
}

struct DisplayProjection {
    base: Value,
    local: Value,
    context: Option<Value>,
    never_sent: Value,
    head: Option<(i64, i64)>,
    safe: Value,
}

fn take_base_and_context(raw: &mut Value) -> Result<(Value, Option<Value>), CoreError> {
    // Move exact subtrees out before typed decode. The base history must
    // never duplicate; local queues keep their missing/null/value form for
    // the whole pipeline. Placeholders preserve field checks in decode.
    // Counts and byte length stay numeric in the error; contents never log.
    let history = raw["base"]["history"].as_array().map(Vec::len).unwrap_or(0);
    let queued = raw
        .get("local")
        .and_then(Value::as_object)
        .map(|queues| {
            queues
                .values()
                .map(|queue| queue.as_array().map(Vec::len).unwrap_or(0))
                .sum()
        })
        .unwrap_or(0);
    let base = crate::take_required_field(raw, "base", "workspace.projection", history, queued)?;
    // displayContext presence is semantic: absent stays absent so the
    // no-context contract path keeps working.
    let context = if raw.get("displayContext").is_some() {
        Some(crate::take_required_field(
            raw,
            "displayContext",
            "workspace.projection",
            history,
            queued,
        )?)
    } else {
        None
    };
    Ok((base, context))
}

fn project_display_value(
    request: DisplayProjection,
    now: &str,
    observed: Option<&crate::timer::workspace::retarget_observation::RunningTimer>,
) -> Result<Value, CoreError> {
    let (base, root, safe) = display_workspace_root(request);
    let (stored, selected) = select_display_queues(&root, &safe)?;
    let workspace = project_selected_queues(base, &root, selected.clone(), now, observed)?;
    let mut result_map = serde_json::Map::with_capacity(3);
    result_map.insert("projectionPending".into(), safe);
    result_map.insert("workspace".into(), workspace);
    if stored.is_some() {
        result_map.insert("displayContext".into(), display::context(selected));
    }
    Ok(Value::Object(result_map))
}

fn display_workspace_root(request: DisplayProjection) -> (Value, Value, Value) {
    // Small root for delivery/display lookups; the large base moves by
    // value into each projection request and is never cloned. Map inserts
    // move ownership; the json! macro would clone through serialization.
    // Delivery admission reads neverSent and canonicalHead from this root,
    // so both travel alongside the exact local queues.
    let DisplayProjection {
        base,
        local,
        context,
        never_sent,
        head,
        safe,
    } = request;
    let mut root_map = serde_json::Map::with_capacity(4);
    root_map.insert("local".into(), local);
    if let Some(context) = context {
        root_map.insert("displayContext".into(), context);
    }
    root_map.insert("neverSent".into(), never_sent);
    root_map.insert(
        "canonicalHead".into(),
        head.map_or(
            Value::Null,
            |(wall, counter)| json!({"wallMs": wall, "counter": counter}),
        ),
    );
    (base, Value::Object(root_map), safe)
}

fn select_display_queues(root: &Value, safe: &Value) -> Result<(Option<Value>, Value), CoreError> {
    let stored = display::stored(root)?;
    let selected = if stored.is_some() {
        display::queues(root, stored.as_ref())?
    } else {
        safe.clone()
    };
    Ok((stored, selected))
}

fn project_selected_queues(
    base: Value,
    root: &Value,
    selected: Value,
    now: &str,
    observed: Option<&crate::timer::workspace::retarget_observation::RunningTimer>,
) -> Result<Value, CoreError> {
    // Validate every retained payload through the existing production reducers,
    // including domains excluded from display. Filtering must not hide
    // corruption. The retained check runs first so its errors keep precedence.
    if selected == root["local"] {
        let projection = projection_request(base, selected, now);
        apply_projection_value(projection, observed)
    } else {
        let retained = projection_request(base, root["local"].clone(), now);
        crate::projection::apply_selected_workspace_value(retained, selected, observed)
    }
}

fn projection_request(base: Value, pending: Value, now: &str) -> Value {
    let mut map = serde_json::Map::with_capacity(3);
    map.insert("base".into(), base);
    map.insert("pending".into(), pending);
    map.insert("now".into(), Value::String(now.to_owned()));
    Value::Object(map)
}

fn apply_projection_value(
    projection: Value,
    observed: Option<&crate::timer::workspace::retarget_observation::RunningTimer>,
) -> Result<Value, CoreError> {
    match observed {
        Some(observed) => crate::projection::apply_monotonic_value(projection, observed),
        None => crate::projection::apply_workspace_value(projection),
    }
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
    crate::strict_json::object_array_field(root, "timerDependencies", "timerDependencies", true)?;
    // Serde decodes structs positionally, so a populated array would authorize
    // optimistic projection with malformed ordering evidence. Require object/null first.
    // Path keeps the established `workspace.` prefix so the error envelope stays
    // byte-identical for previously rejected inputs.
    crate::strict_json::nullable_object_field(root, "canonicalHead", "workspace.canonicalHead")?;
    Ok(())
}
