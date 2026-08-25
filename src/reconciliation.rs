use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::CoreError;
use crate::sync_projection::{
    AutoStartOperation, DurationOperation, SelectedTaskOperation, Task, TaskOperation,
};
use crate::timer::{CanonicalTimer, HistoryItem, WireCommand};

mod acknowledgements;
mod canonical_projection;
mod clocks;
mod timer_dependencies;
mod validation;

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
    selected_task_id: crate::SelectedTaskField,
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

fn is_false(value: &bool) -> bool {
    !value
}

pub(crate) fn rebase_v1_json(input: &str) -> Result<String, CoreError> {
    let value: Value = serde_json::from_str(input)?;
    validation::required_response_fields(&value)?;
    let input: RebaseInput = serde_json::from_value(value)?;
    Ok(serde_json::to_string(&rebase(input)?)?)
}

fn rebase(mut input: RebaseInput) -> Result<RebaseOutput, CoreError> {
    validation::canonical_response(&input.response)?;
    let acknowledged = acknowledgements::validate(&input.sent, &input.response)?;
    validation::local_queue_ids(&input.local)?;
    let timer_resolution = timer_dependencies::resolve(
        &mut input.local.commands,
        &input.timer_dependencies,
        &input.response,
    )?;
    let mut pending =
        acknowledgements::filter_pending(input.local, &acknowledged, &timer_resolution);
    clocks::rebase(&mut pending, &input.response)?;
    canonical_projection::assemble(input.response, pending, timer_resolution)
}
