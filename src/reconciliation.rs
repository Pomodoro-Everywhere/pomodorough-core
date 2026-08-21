use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::DateTime;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::sync_projection::{
    AutoStartOperation, DurationOperation, SelectedTaskOperation, Task, TaskOperation,
    replay_auto_start, replay_durations, replay_selected_task, replay_tasks,
};
use crate::timer::{CanonicalTimer, HistoryItem, WireCommand};
use crate::{CoreError, SelectedTaskField, timer};

const MAX_CLOCK_SKEW_MS: i64 = 5 * 60_000;
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

struct RequiredNullable<T>(Option<T>);

impl<'de, T> Deserialize<'de> for RequiredNullable<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer).map(Self)
    }
}

#[derive(Clone, Deserialize)]
struct Identified {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SentQueues {
    #[serde(default)]
    commands: Vec<Identified>,
    #[serde(default)]
    task_operations: Vec<Identified>,
    #[serde(default)]
    duration_operations: Vec<Identified>,
    #[serde(default)]
    auto_start_operations: Vec<Identified>,
    #[serde(default)]
    selected_task_operations: Vec<Identified>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalQueues {
    #[serde(default)]
    commands: Vec<WireCommand>,
    #[serde(default)]
    task_operations: Vec<TaskOperation>,
    #[serde(default)]
    duration_operations: Vec<DurationOperation>,
    #[serde(default)]
    auto_start_operations: Vec<AutoStartOperation>,
    #[serde(default)]
    selected_task_operations: Vec<SelectedTaskOperation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalResponse {
    acknowledgements: Vec<Value>,
    task_acknowledgements: Vec<Value>,
    duration_acknowledgements: Vec<Value>,
    auto_start_acknowledgements: Vec<Value>,
    selected_task_acknowledgements: Vec<Value>,
    revision: i64,
    canonical_timer: RequiredNullable<CanonicalTimer>,
    history: Vec<HistoryItem>,
    tasks: Vec<Task>,
    durations_ms: BTreeMap<String, i64>,
    auto_start_breaks: bool,
    selected_task_id: SelectedTaskField,
    server_time: String,
    server_hlc_wall_ms: i64,
    server_hlc_counter: i64,
}

#[derive(Deserialize)]
struct RebaseInput {
    #[serde(alias = "pending")]
    local: LocalQueues,
    sent: SentQueues,
    response: CanonicalResponse,
    #[serde(default, rename = "timerDependencies")]
    timer_dependencies: Vec<TimerDependency>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimerDependency {
    operation_id: String,
    depends_on_operation_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RebaseOutput {
    revision: i64,
    pending: Vec<WireCommand>,
    pending_task_operations: Vec<TaskOperation>,
    pending_duration_operations: Vec<DurationOperation>,
    pending_auto_start_operations: Vec<AutoStartOperation>,
    pending_selected_task_operations: Vec<SelectedTaskOperation>,
    dropped_timer_operation_ids: Vec<String>,
    base_timer: Option<CanonicalTimer>,
    base_history: Vec<HistoryItem>,
    base_tasks: Vec<Task>,
    base_durations_ms: BTreeMap<String, i64>,
    base_auto_start_breaks: bool,
    base_selected_task_id: Option<String>,
    timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    tasks: Vec<Task>,
    durations_ms: BTreeMap<String, i64>,
    auto_start_breaks: bool,
    selected_task_id: Option<String>,
}

pub(crate) fn rebase_v1_json(input: &str) -> Result<String, CoreError> {
    let value: Value = serde_json::from_str(input)?;
    validate_required_response_fields(&value)?;
    let input: RebaseInput = serde_json::from_value(value)?;
    Ok(serde_json::to_string(&rebase(input)?)?)
}

fn validate_required_response_fields(input: &Value) -> Result<(), CoreError> {
    let response = input
        .get("response")
        .and_then(Value::as_object)
        .ok_or_else(|| CoreError::InvalidInput("missing canonical response".into()))?;
    for field in [
        "acknowledgements",
        "taskAcknowledgements",
        "durationAcknowledgements",
        "autoStartAcknowledgements",
        "selectedTaskAcknowledgements",
        "revision",
        "canonicalTimer",
        "history",
        "tasks",
        "durationsMs",
        "autoStartBreaks",
        "selectedTaskId",
        "serverTime",
        "serverHlcWallMs",
        "serverHlcCounter",
    ] {
        if !response.contains_key(field) {
            return Err(CoreError::MissingProjection(match field {
                "canonicalTimer" => "response.canonicalTimer",
                "selectedTaskId" => "response.selectedTaskId",
                "revision" => "response.revision",
                "history" => "response.history",
                "tasks" => "response.tasks",
                "durationsMs" => "response.durationsMs",
                "autoStartBreaks" => "response.autoStartBreaks",
                "serverTime" => "response.serverTime",
                "serverHlcWallMs" => "response.serverHlcWallMs",
                "serverHlcCounter" => "response.serverHlcCounter",
                "acknowledgements" => "response.acknowledgements",
                "taskAcknowledgements" => "response.taskAcknowledgements",
                "durationAcknowledgements" => "response.durationAcknowledgements",
                "autoStartAcknowledgements" => "response.autoStartAcknowledgements",
                "selectedTaskAcknowledgements" => "response.selectedTaskAcknowledgements",
                _ => unreachable!(),
            }));
        }
    }
    Ok(())
}

fn rebase(input: RebaseInput) -> Result<RebaseOutput, CoreError> {
    validate_canonical_response(&input.response)?;
    let command_ids = validate_acknowledgements(
        "acknowledgements",
        &input.sent.commands,
        &input.response.acknowledgements,
        "commandId",
    )?;
    let task_ids = validate_acknowledgements(
        "taskAcknowledgements",
        &input.sent.task_operations,
        &input.response.task_acknowledgements,
        "operationId",
    )?;
    let duration_ids = validate_acknowledgements(
        "durationAcknowledgements",
        &input.sent.duration_operations,
        &input.response.duration_acknowledgements,
        "operationId",
    )?;
    let auto_start_ids = validate_acknowledgements(
        "autoStartAcknowledgements",
        &input.sent.auto_start_operations,
        &input.response.auto_start_acknowledgements,
        "operationId",
    )?;
    let selected_task_ids = validate_acknowledgements(
        "selectedTaskAcknowledgements",
        &input.sent.selected_task_operations,
        &input.response.selected_task_acknowledgements,
        "operationId",
    )?;

    validate_unique_local_ids(
        "taskOperations",
        input
            .local
            .task_operations
            .iter()
            .map(|operation| operation.clock.id.as_str()),
    )?;
    validate_unique_local_ids(
        "durationOperations",
        input
            .local
            .duration_operations
            .iter()
            .map(|operation| operation.clock.id.as_str()),
    )?;
    validate_unique_local_ids(
        "autoStartOperations",
        input
            .local
            .auto_start_operations
            .iter()
            .map(|operation| operation.clock.id.as_str()),
    )?;
    validate_unique_local_ids(
        "selectedTaskOperations",
        input
            .local
            .selected_task_operations
            .iter()
            .map(|operation| operation.clock.id.as_str()),
    )?;

    let dropped_timer_operation_ids = timer_dependency_drops(
        &input.local.commands,
        &input.timer_dependencies,
        &input.response.acknowledgements,
    )?;
    let pending = input
        .local
        .commands
        .into_iter()
        .filter(|operation| {
            !command_ids.contains(&operation.id)
                && !dropped_timer_operation_ids.contains(&operation.id)
        })
        .collect::<Vec<_>>();
    let pending_task_operations = input
        .local
        .task_operations
        .into_iter()
        .filter(|operation| !task_ids.contains(&operation.clock.id))
        .collect::<Vec<_>>();
    let pending_duration_operations = input
        .local
        .duration_operations
        .into_iter()
        .filter(|operation| !duration_ids.contains(&operation.clock.id))
        .collect::<Vec<_>>();
    let pending_auto_start_operations = input
        .local
        .auto_start_operations
        .into_iter()
        .filter(|operation| !auto_start_ids.contains(&operation.clock.id))
        .collect::<Vec<_>>();
    let pending_selected_task_operations = input
        .local
        .selected_task_operations
        .into_iter()
        .filter(|operation| !selected_task_ids.contains(&operation.clock.id))
        .collect::<Vec<_>>();

    let response = input.response;
    let base_timer = response.canonical_timer.0;
    let base_history = response.history;
    let base_tasks = response.tasks;
    let base_durations_ms = response.durations_ms;
    let base_auto_start_breaks = response.auto_start_breaks;
    let base_selected_task_id = match response.selected_task_id {
        SelectedTaskField::Deselected => None,
        SelectedTaskField::Selected(task_id) => Some(task_id),
        SelectedTaskField::Omitted => {
            return Err(CoreError::MissingProjection("response.selectedTaskId"));
        }
    };

    let timer_projection = timer::replay(
        base_timer.clone(),
        base_history.clone(),
        pending.clone(),
        &response.server_time,
    )?;
    let task_projection = replay_tasks(base_tasks.clone(), pending_task_operations.clone())?;
    let duration_projection = replay_durations(
        Some(base_durations_ms.clone()),
        pending_duration_operations.clone(),
    )?;
    let auto_start_projection = replay_auto_start(
        base_auto_start_breaks,
        pending_auto_start_operations.clone(),
    )?;
    let active_task_ids = task_projection
        .tasks
        .iter()
        .map(|task| task.id.clone())
        .collect();
    let selected_task_projection = replay_selected_task(
        base_selected_task_id.clone(),
        pending_selected_task_operations.clone(),
        active_task_ids,
    )?;

    Ok(RebaseOutput {
        revision: response.revision,
        pending,
        pending_task_operations,
        pending_duration_operations,
        pending_auto_start_operations,
        pending_selected_task_operations,
        dropped_timer_operation_ids: dropped_timer_operation_ids.into_iter().collect(),
        base_timer,
        base_history,
        base_tasks,
        base_durations_ms,
        base_auto_start_breaks,
        base_selected_task_id,
        timer: timer_projection.canonical_timer,
        history: timer_projection.history,
        tasks: task_projection.tasks,
        durations_ms: duration_projection.durations_ms,
        auto_start_breaks: auto_start_projection.auto_start_breaks,
        selected_task_id: selected_task_projection.selected_task_id,
    })
}

fn validate_unique_local_ids<'a>(
    field: &str,
    identifiers: impl IntoIterator<Item = &'a str>,
) -> Result<(), CoreError> {
    let mut seen = BTreeSet::new();
    for identifier in identifiers {
        if identifier.is_empty() || !seen.insert(identifier) {
            return Err(CoreError::InvalidInput(format!(
                "invalid local {field} identities"
            )));
        }
    }
    Ok(())
}

fn timer_dependency_drops(
    commands: &[WireCommand],
    dependencies: &[TimerDependency],
    acknowledgements: &[Value],
) -> Result<BTreeSet<String>, CoreError> {
    let command_ids = commands
        .iter()
        .map(|command| command.id.as_str())
        .collect::<BTreeSet<_>>();
    if command_ids.len() != commands.len() {
        return Err(CoreError::InvalidInput(
            "duplicate local timer operation id".into(),
        ));
    }

    let mut dependency_by_child = BTreeMap::new();
    for dependency in dependencies {
        if dependency.operation_id.is_empty()
            || dependency.depends_on_operation_id.is_empty()
            || dependency.operation_id == dependency.depends_on_operation_id
            || !command_ids.contains(dependency.operation_id.as_str())
            || !command_ids.contains(dependency.depends_on_operation_id.as_str())
            || dependency_by_child
                .insert(
                    dependency.operation_id.as_str(),
                    dependency.depends_on_operation_id.as_str(),
                )
                .is_some()
        {
            return Err(CoreError::InvalidInput(
                "invalid timer dependency graph".into(),
            ));
        }
    }

    let mut visit_state = BTreeMap::new();
    for child in dependency_by_child.keys() {
        if visit_state.get(child) == Some(&2_u8) {
            continue;
        }
        let mut path = Vec::new();
        let mut current = *child;
        loop {
            match visit_state.get(current) {
                Some(1) => {
                    return Err(CoreError::InvalidInput(
                        "cyclic timer dependency graph".into(),
                    ));
                }
                Some(2) => break,
                _ => {
                    visit_state.insert(current, 1_u8);
                    path.push(current);
                }
            }
            let Some(parent) = dependency_by_child.get(current) else {
                break;
            };
            current = parent;
        }
        for identifier in path {
            visit_state.insert(identifier, 2_u8);
        }
    }

    let mut children_by_parent: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (child, parent) in &dependency_by_child {
        children_by_parent.entry(parent).or_default().push(child);
    }
    let mut dropped = BTreeSet::new();
    let mut applied = BTreeSet::new();
    let mut queue = VecDeque::new();
    for acknowledgement in acknowledgements {
        let Some(object) = acknowledgement.as_object() else {
            continue;
        };
        let Some(identifier) = object.get("commandId").and_then(Value::as_str) else {
            continue;
        };
        if object.get("outcome").and_then(Value::as_str) == Some("applied") {
            applied.insert(identifier.to_owned());
        } else if dropped.insert(identifier.to_owned()) {
            queue.push_back(identifier.to_owned());
        }
    }
    while let Some(parent) = queue.pop_front() {
        for child in children_by_parent
            .get(parent.as_str())
            .into_iter()
            .flatten()
        {
            if !applied.contains(*child) && dropped.insert((*child).to_owned()) {
                queue.push_back((*child).to_owned());
            }
        }
    }

    // Acknowledged operations are removed by normal acknowledgement handling.
    // The separate list tells persistence adapters which dependents to remove.
    for acknowledgement in acknowledgements {
        if let Some(identifier) = acknowledgement.get("commandId").and_then(Value::as_str) {
            dropped.remove(identifier);
        }
    }
    Ok(dropped)
}

fn validate_acknowledgements(
    field: &str,
    sent: &[Identified],
    acknowledgements: &[Value],
    id_field: &str,
) -> Result<BTreeSet<String>, CoreError> {
    let expected_ids = sent
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    if expected_ids.len() != sent.len()
        || expected_ids.iter().any(|identifier| identifier.is_empty())
        || acknowledgements.len() != expected_ids.len()
    {
        return invalid_acknowledgements(field);
    }

    let mut acknowledged_ids = BTreeSet::new();
    for acknowledgement in acknowledgements {
        let Some(object) = acknowledgement.as_object() else {
            return invalid_acknowledgements(field);
        };
        let Some(identifier) = object.get(id_field).and_then(Value::as_str) else {
            return invalid_acknowledgements(field);
        };
        let Some(outcome) = object.get("outcome").and_then(Value::as_str) else {
            return invalid_acknowledgements(field);
        };
        if object.get("reason").and_then(Value::as_str).is_none()
            || !matches!(outcome, "applied" | "ignored" | "rejected")
            || !expected_ids.contains(identifier)
            || !acknowledged_ids.insert(identifier.to_owned())
        {
            return invalid_acknowledgements(field);
        }
    }
    Ok(acknowledged_ids)
}

fn invalid_acknowledgements<T>(field: &str) -> Result<T, CoreError> {
    Err(CoreError::InvalidInput(format!("invalid {field} set")))
}

fn validate_canonical_response(response: &CanonicalResponse) -> Result<(), CoreError> {
    if !(0..=MAX_SAFE_INTEGER).contains(&response.revision) {
        return invalid_response("revision");
    }
    let server_time = DateTime::parse_from_rfc3339(&response.server_time)
        .map_err(|_| CoreError::InvalidTimestamp(response.server_time.clone()))?;
    let server_time_ms = server_time.timestamp_millis();
    let Some(clock_skew_ms) = response.server_hlc_wall_ms.checked_sub(server_time_ms) else {
        return invalid_response("server HLC");
    };
    if !(0..=MAX_CLOCK_SKEW_MS).contains(&clock_skew_ms)
        || !(0..=MAX_SAFE_INTEGER).contains(&response.server_hlc_wall_ms)
        || !(0..=MAX_SAFE_INTEGER).contains(&response.server_hlc_counter)
    {
        return invalid_response("server HLC");
    }
    timer::validate_replay_state(&response.canonical_timer.0, &response.history)?;
    validate_tasks(&response.tasks)?;
    validate_durations(&response.durations_ms)?;
    let task_ids = response
        .tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<BTreeSet<_>>();
    match &response.selected_task_id {
        SelectedTaskField::Selected(task_id)
            if task_id.is_empty() || !task_ids.contains(task_id.as_str()) =>
        {
            invalid_response("selectedTaskId")
        }
        SelectedTaskField::Omitted => Err(CoreError::MissingProjection("response.selectedTaskId")),
        SelectedTaskField::Deselected | SelectedTaskField::Selected(_) => Ok(()),
    }
}

fn validate_tasks(tasks: &[Task]) -> Result<(), CoreError> {
    let mut ids = BTreeSet::new();
    if tasks
        .iter()
        .any(|task| task.id.is_empty() || task.title.is_empty() || !ids.insert(task.id.as_str()))
    {
        return invalid_response("tasks");
    }
    Ok(())
}

fn validate_durations(durations: &BTreeMap<String, i64>) -> Result<(), CoreError> {
    for phase in ["focus", "short_break", "long_break"] {
        let Some(duration) = durations.get(phase) else {
            return invalid_response("durationsMs");
        };
        if !(60_000..=10_800_000).contains(duration) || duration % 60_000 != 0 {
            return invalid_response("durationsMs");
        }
    }
    Ok(())
}

fn invalid_response<T>(field: &str) -> Result<T, CoreError> {
    Err(CoreError::InvalidInput(format!(
        "invalid canonical response {field}"
    )))
}
