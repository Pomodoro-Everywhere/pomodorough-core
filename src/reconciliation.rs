use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::{DateTime, SecondsFormat, Utc};
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

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimerDependency {
    operation_id: String,
    depends_on_operation_id: String,
    #[serde(default, skip_serializing_if = "is_false")]
    generated_break: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_day_start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_day_end: Option<String>,
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
    pending_timer_dependencies: Vec<TimerDependency>,
    promoted_timer_operation_ids: Vec<String>,
    dropped_timer_operation_ids: Vec<String>,
    dropped_timer_ids: Vec<String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Hlc {
    wall_ms: i64,
    counter: i64,
}

fn is_false(value: &bool) -> bool {
    !value
}

struct TimerDependencyResolution {
    dropped_operation_ids: BTreeSet<String>,
    dropped_timer_ids: BTreeSet<String>,
    promoted_operation_ids: BTreeSet<String>,
    pending_dependencies: Vec<TimerDependency>,
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

fn rebase(mut input: RebaseInput) -> Result<RebaseOutput, CoreError> {
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

    let timer_dependency_resolution = resolve_timer_dependencies(
        &mut input.local.commands,
        &input.timer_dependencies,
        &input.response,
    )?;
    let mut pending = input
        .local
        .commands
        .into_iter()
        .filter(|operation| {
            !command_ids.contains(&operation.id)
                && !timer_dependency_resolution
                    .dropped_operation_ids
                    .contains(&operation.id)
        })
        .collect::<Vec<_>>();
    let mut pending_task_operations = input
        .local
        .task_operations
        .into_iter()
        .filter(|operation| !task_ids.contains(&operation.clock.id))
        .collect::<Vec<_>>();
    let mut pending_duration_operations = input
        .local
        .duration_operations
        .into_iter()
        .filter(|operation| !duration_ids.contains(&operation.clock.id))
        .collect::<Vec<_>>();
    let mut pending_auto_start_operations = input
        .local
        .auto_start_operations
        .into_iter()
        .filter(|operation| !auto_start_ids.contains(&operation.clock.id))
        .collect::<Vec<_>>();
    let mut pending_selected_task_operations = input
        .local
        .selected_task_operations
        .into_iter()
        .filter(|operation| !selected_task_ids.contains(&operation.clock.id))
        .collect::<Vec<_>>();

    rebase_pending_clocks(
        &mut pending,
        &mut pending_task_operations,
        &mut pending_duration_operations,
        &mut pending_auto_start_operations,
        &mut pending_selected_task_operations,
        &input.response,
    )?;

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
        pending_timer_dependencies: timer_dependency_resolution.pending_dependencies,
        promoted_timer_operation_ids: timer_dependency_resolution
            .promoted_operation_ids
            .into_iter()
            .collect(),
        dropped_timer_operation_ids: timer_dependency_resolution
            .dropped_operation_ids
            .into_iter()
            .collect(),
        dropped_timer_ids: timer_dependency_resolution
            .dropped_timer_ids
            .into_iter()
            .collect(),
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

fn rebase_pending_clocks(
    commands: &mut [WireCommand],
    task_operations: &mut [TaskOperation],
    duration_operations: &mut [DurationOperation],
    auto_start_operations: &mut [AutoStartOperation],
    selected_task_operations: &mut [SelectedTaskOperation],
    response: &CanonicalResponse,
) -> Result<(), CoreError> {
    let server_time_ms = DateTime::parse_from_rfc3339(&response.server_time)
        .map_err(|_| CoreError::InvalidTimestamp(response.server_time.clone()))?
        .timestamp_millis();
    let minimum_ms = server_time_ms.saturating_sub(MAX_CLOCK_SKEW_MS).max(1);
    let maximum_ms = server_time_ms
        .saturating_add(MAX_CLOCK_SKEW_MS)
        .min(MAX_SAFE_INTEGER);
    let canonical_clock = Hlc {
        wall_ms: response.server_hlc_wall_ms,
        counter: response.server_hlc_counter,
    };

    let mut command_clocks = commands
        .iter()
        .map(|command| {
            validate_queue_clock(
                &command.id,
                &command.device_id,
                command.hlc_wall_ms,
                command.hlc_counter,
            )?;
            Ok((
                command.device_sequence,
                command.id.clone(),
                Hlc {
                    wall_ms: command.hlc_wall_ms,
                    counter: command.hlc_counter,
                },
            ))
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    command_clocks.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let command_replacements = clock_replacements(
        command_clocks.into_iter().map(|(_, id, clock)| (id, clock)),
        canonical_clock,
        minimum_ms,
        maximum_ms,
    )?;
    for command in commands {
        if let Some(clock) = command_replacements.get(&command.id) {
            command.occurred_at =
                rebased_occurrence(&command.occurred_at, *clock, minimum_ms, maximum_ms)?;
            command.hlc_wall_ms = clock.wall_ms;
            command.hlc_counter = clock.counter;
        }
    }

    rebase_operation_clocks(
        task_operations
            .iter_mut()
            .map(|operation| &mut operation.clock),
        canonical_clock,
        minimum_ms,
        maximum_ms,
    )?;
    rebase_operation_clocks(
        duration_operations
            .iter_mut()
            .map(|operation| &mut operation.clock),
        canonical_clock,
        minimum_ms,
        maximum_ms,
    )?;
    rebase_operation_clocks(
        auto_start_operations
            .iter_mut()
            .map(|operation| &mut operation.clock),
        canonical_clock,
        minimum_ms,
        maximum_ms,
    )?;
    rebase_operation_clocks(
        selected_task_operations
            .iter_mut()
            .map(|operation| &mut operation.clock),
        canonical_clock,
        minimum_ms,
        maximum_ms,
    )?;
    Ok(())
}

fn rebase_operation_clocks<'a>(
    clocks: impl IntoIterator<Item = &'a mut crate::sync_projection::OperationClock>,
    canonical_clock: Hlc,
    minimum_ms: i64,
    maximum_ms: i64,
) -> Result<(), CoreError> {
    let mut clocks = clocks.into_iter().collect::<Vec<_>>();
    for clock in &clocks {
        validate_queue_clock(
            &clock.id,
            &clock.device_id,
            clock.hlc_wall_ms,
            clock.hlc_counter,
        )?;
    }
    let mut ordered = clocks
        .iter()
        .map(|clock| {
            (
                clock.id.clone(),
                Hlc {
                    wall_ms: clock.hlc_wall_ms,
                    counter: clock.hlc_counter,
                },
            )
        })
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    let replacements = clock_replacements(ordered, canonical_clock, minimum_ms, maximum_ms)?;
    for clock in &mut clocks {
        if let Some(replacement) = replacements.get(&clock.id) {
            clock.occurred_at =
                rebased_occurrence(&clock.occurred_at, *replacement, minimum_ms, maximum_ms)?;
            clock.hlc_wall_ms = replacement.wall_ms;
            clock.hlc_counter = replacement.counter;
        }
    }
    Ok(())
}

fn validate_queue_clock(
    id: &str,
    device_id: &str,
    wall_ms: i64,
    counter: i64,
) -> Result<(), CoreError> {
    if id.is_empty()
        || device_id.is_empty()
        || !(0..=MAX_SAFE_INTEGER).contains(&wall_ms)
        || !(0..=MAX_SAFE_INTEGER).contains(&counter)
    {
        return Err(CoreError::InvalidInput(
            "invalid retained operation clock".into(),
        ));
    }
    Ok(())
}

fn clock_replacements(
    clocks: impl IntoIterator<Item = (String, Hlc)>,
    canonical_clock: Hlc,
    minimum_ms: i64,
    maximum_ms: i64,
) -> Result<BTreeMap<String, Hlc>, CoreError> {
    let mut cursor = canonical_clock;
    let mut replacements = BTreeMap::new();
    for (id, clock) in clocks {
        if (minimum_ms..=maximum_ms).contains(&clock.wall_ms) && clock > cursor {
            cursor = clock;
            continue;
        }
        cursor = next_clock(cursor, maximum_ms)?;
        replacements.insert(id, cursor);
    }
    Ok(replacements)
}

fn next_clock(clock: Hlc, maximum_ms: i64) -> Result<Hlc, CoreError> {
    if clock.counter < MAX_SAFE_INTEGER {
        return Ok(Hlc {
            wall_ms: clock.wall_ms,
            counter: clock.counter + 1,
        });
    }
    if clock.wall_ms >= maximum_ms {
        return Err(CoreError::InvalidInput(
            "retained operation clock has no safe rebase headroom".into(),
        ));
    }
    Ok(Hlc {
        wall_ms: clock.wall_ms + 1,
        counter: 0,
    })
}

fn rebased_occurrence(
    original: &str,
    clock: Hlc,
    minimum_ms: i64,
    maximum_ms: i64,
) -> Result<String, CoreError> {
    let original_time = DateTime::parse_from_rfc3339(original)
        .map_err(|_| CoreError::InvalidTimestamp(original.to_owned()))?;
    let original_ms = original_time.timestamp_millis();
    if (minimum_ms..=maximum_ms).contains(&original_ms)
        && clock.wall_ms.abs_diff(original_ms) <= MAX_CLOCK_SKEW_MS as u64
    {
        return Ok(original.to_owned());
    }
    let replacement = DateTime::<Utc>::from_timestamp_millis(clock.wall_ms)
        .ok_or_else(|| CoreError::InvalidInput("invalid rebased operation time".into()))?;
    Ok(replacement.to_rfc3339_opts(SecondsFormat::Millis, true))
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

fn resolve_timer_dependencies(
    commands: &mut [WireCommand],
    dependencies: &[TimerDependency],
    response: &CanonicalResponse,
) -> Result<TimerDependencyResolution, CoreError> {
    let command_positions = commands
        .iter()
        .enumerate()
        .map(|(index, command)| (command.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let command_ids = command_positions.keys().cloned().collect::<BTreeSet<_>>();
    if command_ids.len() != commands.len() {
        return Err(CoreError::InvalidInput(
            "duplicate local timer operation id".into(),
        ));
    }

    let mut dependency_by_child = BTreeMap::new();
    let mut generated_by_source = BTreeMap::new();
    let mut generated_ranges = BTreeMap::new();
    for dependency in dependencies {
        if dependency.operation_id.is_empty()
            || dependency.depends_on_operation_id.is_empty()
            || dependency.operation_id == dependency.depends_on_operation_id
            || !command_ids.contains(&dependency.operation_id)
            || !command_ids.contains(&dependency.depends_on_operation_id)
            || dependency_by_child
                .insert(
                    dependency.operation_id.clone(),
                    dependency.depends_on_operation_id.clone(),
                )
                .is_some()
        {
            return Err(CoreError::InvalidInput(
                "invalid timer dependency graph".into(),
            ));
        }
        match (
            dependency.generated_break,
            dependency.source_day_start.as_deref(),
            dependency.source_day_end.as_deref(),
        ) {
            (false, None, None) => {}
            (true, Some(start), Some(end)) => {
                let child = &commands[command_positions[&dependency.operation_id]];
                let parent = &commands[command_positions[&dependency.depends_on_operation_id]];
                let start = parse_dependency_time(start)?;
                let end = parse_dependency_time(end)?;
                let range_ms = end.timestamp_millis() - start.timestamp_millis();
                if child.kind != "start"
                    || !matches!(child.phase.as_str(), "short_break" | "long_break")
                    || parent.kind != "finish"
                    || parent.phase != "focus"
                    || !(1..=26 * 60 * 60 * 1_000).contains(&range_ms)
                    || generated_by_source
                        .insert(
                            dependency.depends_on_operation_id.clone(),
                            dependency.operation_id.clone(),
                        )
                        .is_some()
                {
                    return Err(CoreError::InvalidInput(
                        "invalid generated break dependency".into(),
                    ));
                }
                generated_ranges.insert(dependency.operation_id.clone(), (start, end));
            }
            _ => {
                return Err(CoreError::InvalidInput(
                    "invalid generated break dependency".into(),
                ));
            }
        }
    }

    let mut visit_state = BTreeMap::new();
    for child in dependency_by_child.keys() {
        if visit_state.get(child) == Some(&2_u8) {
            continue;
        }
        let mut path = Vec::new();
        let mut current = child.as_str();
        loop {
            match visit_state.get(current) {
                Some(1) => {
                    return Err(CoreError::InvalidInput(
                        "cyclic timer dependency graph".into(),
                    ));
                }
                Some(2) => break,
                _ => {
                    visit_state.insert(current.to_owned(), 1_u8);
                    path.push(current.to_owned());
                }
            }
            let Some(parent) = dependency_by_child.get(current) else {
                break;
            };
            current = parent.as_str();
        }
        for identifier in path {
            visit_state.insert(identifier, 2_u8);
        }
    }

    let mut children_by_parent: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (child, parent) in &dependency_by_child {
        children_by_parent
            .entry(parent.clone())
            .or_default()
            .push(child.clone());
    }
    let acknowledgement_outcomes = response
        .acknowledgements
        .iter()
        .filter_map(|acknowledgement| {
            Some((
                acknowledgement.get("commandId")?.as_str()?.to_owned(),
                acknowledgement.get("outcome")?.as_str()?.to_owned(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let acknowledged_ids = acknowledgement_outcomes
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut accepted_generated_sources = BTreeSet::new();
    let mut invalid_generated_sources = BTreeSet::new();
    let mut promoted = BTreeSet::new();
    let mut dropped_timer_ids = BTreeSet::new();

    for (source_id, generated_start_id) in &generated_by_source {
        let Some(outcome) = acknowledgement_outcomes.get(source_id) else {
            continue;
        };
        let source = &commands[command_positions[source_id]];
        let generated_start = &commands[command_positions[generated_start_id]];
        let (day_start, day_end) = generated_ranges[generated_start_id];
        let exact_completion = exact_focus_completion_time(response, source)?;
        if exact_completion
            .is_some_and(|completed_at| completed_at < day_start || completed_at >= day_end)
        {
            return Err(CoreError::InvalidInput(
                "generated break source day excludes canonical completion".into(),
            ));
        }
        let canonical_supersedes_break = response.canonical_timer.0.as_ref().is_some_and(|timer| {
            matches!(timer.status.as_str(), "running" | "paused")
                && timer.id != source.timer_id
                && timer.id != generated_start.timer_id
        });
        let newer_manual_start = commands.iter().any(|command| {
            command.kind == "start"
                && command.id != generated_start.id
                && command.device_sequence > generated_start.device_sequence
                && !dependency_by_child.contains_key(&command.id)
        });
        let accepted = matches!(outcome.as_str(), "applied" | "ignored")
            && exact_completion.is_some()
            && !canonical_supersedes_break
            && !newer_manual_start;
        if !accepted {
            invalid_generated_sources.insert(source_id.clone());
            dropped_timer_ids.insert(generated_start.timer_id.clone());
            continue;
        }

        let batch_ids = descendants(source_id, &children_by_parent)
            .into_iter()
            .filter(|identifier| {
                commands[command_positions[identifier]].timer_id == generated_start.timer_id
            })
            .collect::<BTreeSet<_>>();
        validate_generated_break_batch(
            commands,
            &command_positions,
            generated_start_id,
            &batch_ids,
        )?;
        let generated_break_completed = batch_ids
            .iter()
            .any(|identifier| commands[command_positions[identifier]].kind == "finish");
        let (phase, duration_ms) = if generated_break_completed {
            (
                generated_start.phase.clone(),
                generated_start.planned_duration_ms,
            )
        } else {
            let completed_count = completed_focus_count(response, source, day_start, day_end)?;
            let phase = if completed_count > 0 && completed_count % 4 == 0 {
                "long_break"
            } else {
                "short_break"
            };
            (phase.to_owned(), response.durations_ms[phase])
        };
        for identifier in &batch_ids {
            let command = &mut commands[command_positions[identifier]];
            command.phase.clone_from(&phase);
            command.planned_duration_ms = duration_ms;
            command.observed_elapsed_ms = command.observed_elapsed_ms.clamp(0, duration_ms);
            if !acknowledged_ids.contains(identifier) {
                promoted.insert(identifier.clone());
            }
        }
        accepted_generated_sources.insert(source_id.clone());
    }

    let mut applied_barriers = acknowledgement_outcomes
        .iter()
        .filter(|(identifier, outcome)| {
            outcome.as_str() == "applied" && !invalid_generated_sources.contains(*identifier)
        })
        .map(|(identifier, _)| identifier.clone())
        .collect::<BTreeSet<_>>();
    applied_barriers.extend(accepted_generated_sources.iter().cloned());

    let mut dropped = BTreeSet::new();
    let mut queue = VecDeque::new();
    for (identifier, outcome) in &acknowledgement_outcomes {
        if outcome != "applied"
            && !accepted_generated_sources.contains(identifier)
            && dropped.insert(identifier.clone())
        {
            queue.push_back(identifier.clone());
        }
    }
    for identifier in &invalid_generated_sources {
        if dropped.insert(identifier.clone()) {
            queue.push_back(identifier.clone());
        }
    }
    while let Some(parent) = queue.pop_front() {
        for child in children_by_parent
            .get(parent.as_str())
            .into_iter()
            .flatten()
        {
            if !applied_barriers.contains(child) && dropped.insert(child.clone()) {
                queue.push_back(child.clone());
            }
        }
    }

    // Acknowledged operations are removed by normal acknowledgement handling.
    // The separate list tells persistence adapters which dependents to remove.
    for identifier in &acknowledged_ids {
        dropped.remove(identifier);
    }

    for parent in &applied_barriers {
        for child in children_by_parent.get(parent).into_iter().flatten() {
            if !acknowledged_ids.contains(child) && !dropped.contains(child) {
                promoted.insert(child.clone());
            }
        }
    }
    promoted.retain(|identifier| !dropped.contains(identifier));

    let pending_ids = command_ids
        .difference(&acknowledged_ids)
        .filter(|identifier| !dropped.contains(*identifier))
        .cloned()
        .collect::<BTreeSet<_>>();
    let pending_dependencies = dependencies
        .iter()
        .filter(|dependency| {
            !promoted.contains(&dependency.operation_id)
                && pending_ids.contains(&dependency.operation_id)
                && pending_ids.contains(&dependency.depends_on_operation_id)
        })
        .cloned()
        .collect();

    Ok(TimerDependencyResolution {
        dropped_operation_ids: dropped,
        dropped_timer_ids,
        promoted_operation_ids: promoted,
        pending_dependencies,
    })
}

fn parse_dependency_time(value: &str) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| CoreError::InvalidTimestamp(value.to_owned()))
}

fn descendants(root: &str, children_by_parent: &BTreeMap<String, Vec<String>>) -> BTreeSet<String> {
    let mut descendants = BTreeSet::new();
    let mut queue = VecDeque::from([root.to_owned()]);
    while let Some(parent) = queue.pop_front() {
        for child in children_by_parent.get(&parent).into_iter().flatten() {
            if descendants.insert(child.clone()) {
                queue.push_back(child.clone());
            }
        }
    }
    descendants
}

fn validate_generated_break_batch(
    commands: &[WireCommand],
    command_positions: &BTreeMap<String, usize>,
    generated_start_id: &str,
    batch_ids: &BTreeSet<String>,
) -> Result<(), CoreError> {
    let generated_start = &commands[command_positions[generated_start_id]];
    if batch_ids.is_empty()
        || batch_ids.iter().any(|identifier| {
            let command = &commands[command_positions[identifier]];
            command.timer_id != generated_start.timer_id
                || !matches!(
                    command.kind.as_str(),
                    "start" | "pause" | "resume" | "finish" | "cancel" | "clear"
                )
                || (command.kind == "start" && command.id != generated_start_id)
        })
    {
        return Err(CoreError::InvalidInput(
            "invalid generated break command batch".into(),
        ));
    }
    Ok(())
}

fn exact_focus_completion_time(
    response: &CanonicalResponse,
    source: &WireCommand,
) -> Result<Option<DateTime<Utc>>, CoreError> {
    if let Some(item) = response.history.iter().find(|item| {
        item.timer_id == source.timer_id
            && item.command_id.as_deref() == Some(source.id.as_str())
            && item.phase == "focus"
            && item.status == "completed"
    }) {
        return item
            .completed_at
            .as_deref()
            .or(item.ended_at.as_deref())
            .map(parse_dependency_time)
            .transpose();
    }
    let exact_timer = response.canonical_timer.0.as_ref().filter(|timer| {
        timer.id == source.timer_id
            && timer.phase == "focus"
            && timer.status == "completed"
            && timer
                .last_intent
                .as_ref()
                .is_some_and(|intent| intent.kind == "finish" && intent.command_id == source.id)
    });
    exact_timer
        .map(|timer| parse_dependency_time(&timer.anchor_at))
        .transpose()
}

fn completed_focus_count(
    response: &CanonicalResponse,
    source: &WireCommand,
    day_start: DateTime<Utc>,
    day_end: DateTime<Utc>,
) -> Result<usize, CoreError> {
    let mut focuses = response
        .history
        .iter()
        .filter(|item| item.phase == "focus" && item.status == "completed")
        .map(|item| {
            let completed_at = item
                .completed_at
                .as_deref()
                .or(item.ended_at.as_deref())
                .ok_or_else(|| CoreError::InvalidInput("invalid timer history".into()))?;
            Ok((
                parse_dependency_time(completed_at)?,
                item.command_id
                    .clone()
                    .unwrap_or_else(|| item.timer_id.clone()),
                item,
            ))
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    focuses.retain(|(completed_at, _, _)| *completed_at >= day_start && *completed_at < day_end);
    focuses.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    Ok(focuses
        .iter()
        .position(|(_, _, item)| {
            item.command_id.as_deref() == Some(source.id.as_str())
                || item.timer_id == source.timer_id
        })
        .map_or(focuses.len() + 1, |index| index + 1))
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
