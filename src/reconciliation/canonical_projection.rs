use std::collections::BTreeMap;

use crate::sync_projection::{
    Task, replay_auto_start, replay_durations, replay_selected_task, replay_tasks,
};
use crate::timer::{CanonicalTimer, HistoryItem};
use crate::{CoreError, SelectedTaskField, timer};

use super::acknowledgements::PendingQueues;
use super::timer_dependencies::TimerDependencyResolution;
use super::{CanonicalResponse, RebaseOutput};

struct CanonicalBase {
    revision: i64,
    timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    tasks: Vec<Task>,
    durations_ms: BTreeMap<String, i64>,
    auto_start_breaks: bool,
    selected_task_id: Option<String>,
    server_time: String,
}

struct PendingProjection {
    timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    tasks: Vec<Task>,
    durations_ms: BTreeMap<String, i64>,
    auto_start_breaks: bool,
    selected_task_id: Option<String>,
}

pub(super) fn assemble(
    response: CanonicalResponse,
    pending: PendingQueues,
    timer_resolution: TimerDependencyResolution,
) -> Result<RebaseOutput, CoreError> {
    let base = canonical_base(response)?;
    let projection = project_pending(&base, &pending)?;
    Ok(rebase_output(base, pending, timer_resolution, projection))
}

fn canonical_base(response: CanonicalResponse) -> Result<CanonicalBase, CoreError> {
    let selected_task_id = match response.selected_task_id {
        SelectedTaskField::Deselected => None,
        SelectedTaskField::Selected(task_id) => Some(task_id),
        SelectedTaskField::Omitted => {
            return Err(CoreError::MissingProjection("response.selectedTaskId"));
        }
    };
    Ok(CanonicalBase {
        revision: response.revision,
        timer: response.canonical_timer.0,
        history: response.history,
        tasks: response.tasks,
        durations_ms: response.durations_ms,
        auto_start_breaks: response.auto_start_breaks,
        selected_task_id,
        server_time: response.server_time,
    })
}

fn project_pending(
    base: &CanonicalBase,
    pending: &PendingQueues,
) -> Result<PendingProjection, CoreError> {
    let timer = timer::replay(
        base.timer.clone(),
        base.history.clone(),
        pending.commands.clone(),
        &base.server_time,
    )?;
    let tasks = replay_tasks(base.tasks.clone(), pending.tasks.clone())?;
    let durations = replay_durations(Some(base.durations_ms.clone()), pending.durations.clone())?;
    let auto_start = replay_auto_start(base.auto_start_breaks, pending.auto_start.clone())?;
    let active_task_ids = tasks.tasks.iter().map(|task| task.id.clone()).collect();
    let selected_task = replay_selected_task(
        base.selected_task_id.clone(),
        pending.selected_task.clone(),
        active_task_ids,
    )?;
    Ok(PendingProjection {
        timer: timer.canonical_timer,
        history: timer.history,
        tasks: tasks.tasks,
        durations_ms: durations.durations_ms,
        auto_start_breaks: auto_start.auto_start_breaks,
        selected_task_id: selected_task.selected_task_id,
    })
}

fn rebase_output(
    base: CanonicalBase,
    pending: PendingQueues,
    timer_resolution: TimerDependencyResolution,
    projection: PendingProjection,
) -> RebaseOutput {
    RebaseOutput {
        revision: base.revision,
        pending: pending.commands,
        pending_task_operations: pending.tasks,
        pending_duration_operations: pending.durations,
        pending_auto_start_operations: pending.auto_start,
        pending_selected_task_operations: pending.selected_task,
        pending_timer_dependencies: timer_resolution.pending_dependencies,
        promoted_timer_operation_ids: timer_resolution
            .promoted_operation_ids
            .into_iter()
            .collect(),
        dropped_timer_operation_ids: timer_resolution.dropped_operation_ids.into_iter().collect(),
        dropped_timer_ids: timer_resolution.dropped_timer_ids.into_iter().collect(),
        base_timer: base.timer,
        base_history: base.history,
        base_tasks: base.tasks,
        base_durations_ms: base.durations_ms,
        base_auto_start_breaks: base.auto_start_breaks,
        base_selected_task_id: base.selected_task_id,
        timer: projection.timer,
        history: projection.history,
        tasks: projection.tasks,
        durations_ms: projection.durations_ms,
        auto_start_breaks: projection.auto_start_breaks,
        selected_task_id: projection.selected_task_id,
    }
}
