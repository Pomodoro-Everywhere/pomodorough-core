//! Legacy fixture projection wire adapters.
//!
//! `sync_projection` owns replay and clock/winner policy. This module owns only exact legacy JSON
//! translation and output shaping.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::sync_projection::{
    AutoStartOperation, DurationOperation, OperationClock, SelectedTaskOperation, Task,
    TaskOperation, replay_auto_start, replay_durations, replay_selected_task, replay_tasks,
};
use crate::{CoreError, SelectedTaskField};

const FIXTURE_OCCURRED_AT: &str = "1970-01-01T00:00:00Z";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureProjectionInput {
    task_operations: Vec<FixtureTaskOperation>,
    duration_operations: Vec<FixtureDurationOperation>,
    auto_start_operations: Vec<FixtureAutoStartOperation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureOperationClock {
    id: String,
    device_id: String,
    wall_ms: i64,
    counter: i64,
}

impl From<FixtureOperationClock> for OperationClock {
    fn from(clock: FixtureOperationClock) -> Self {
        Self {
            id: clock.id,
            device_id: clock.device_id,
            occurred_at: FIXTURE_OCCURRED_AT.to_owned(),
            hlc_wall_ms: clock.wall_ms,
            hlc_counter: clock.counter,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureTaskOperation {
    #[serde(flatten)]
    clock: FixtureOperationClock,
    task_id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    title: String,
}

impl From<FixtureTaskOperation> for TaskOperation {
    fn from(operation: FixtureTaskOperation) -> Self {
        Self {
            clock: operation.clock.into(),
            task_id: operation.task_id,
            kind: operation.kind,
            title: operation.title,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureDurationOperation {
    #[serde(flatten)]
    clock: FixtureOperationClock,
    phase: String,
    duration_ms: i64,
}

impl From<FixtureDurationOperation> for DurationOperation {
    fn from(operation: FixtureDurationOperation) -> Self {
        Self {
            clock: operation.clock.into(),
            phase: operation.phase,
            duration_ms: operation.duration_ms,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureAutoStartOperation {
    #[serde(flatten)]
    clock: FixtureOperationClock,
    enabled: bool,
}

impl From<FixtureAutoStartOperation> for AutoStartOperation {
    fn from(operation: FixtureAutoStartOperation) -> Self {
        Self {
            clock: operation.clock.into(),
            enabled: operation.enabled,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FixtureProjectionOutput {
    tasks: Vec<Task>,
    durations_ms: BTreeMap<String, i64>,
    auto_start_breaks: bool,
}

pub fn reduce_projection_fixture_case_json(input: &str) -> Result<String, CoreError> {
    let value = strict_fixture_input(input, false)?;
    let input: FixtureProjectionInput = serde_json::from_value(value)?;
    let output = FixtureProjectionOutput {
        tasks: project_tasks(input.task_operations)?,
        durations_ms: project_durations(input.duration_operations)?,
        auto_start_breaks: project_auto_start(input.auto_start_operations)?,
    };
    Ok(serde_json::to_string(&output)?)
}

fn project_tasks(operations: Vec<FixtureTaskOperation>) -> Result<Vec<Task>, CoreError> {
    let operations = operations.into_iter().map(TaskOperation::from).collect();
    let mut tasks = replay_tasks(Vec::new(), operations)?.tasks;

    // Legacy fixture output is ordered by task ID; production output is ordered by title then ID.
    tasks.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(tasks)
}

fn project_durations(
    operations: Vec<FixtureDurationOperation>,
) -> Result<BTreeMap<String, i64>, CoreError> {
    // Legacy output contains exactly submitted phases, unlike production server defaults.
    let base_durations_ms = operations
        .iter()
        .map(|operation| (operation.phase.clone(), operation.duration_ms))
        .collect();
    let operations = operations
        .into_iter()
        .map(DurationOperation::from)
        .collect();
    Ok(replay_durations(Some(base_durations_ms), operations)?.durations_ms)
}

fn project_auto_start(operations: Vec<FixtureAutoStartOperation>) -> Result<bool, CoreError> {
    let operations = operations
        .into_iter()
        .map(AutoStartOperation::from)
        .collect();
    let output = replay_auto_start(false, operations)?;
    output
        .winning_operation_id
        .map(|_| output.auto_start_breaks)
        .ok_or(CoreError::MissingProjection("autoStartOperations"))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureSelectedTaskInput {
    operations: Vec<FixtureSelectedTaskOperation>,
    active_task_ids: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureSelectedTaskOperation {
    #[serde(flatten)]
    clock: FixtureOperationClock,
    // Legacy wire treats an omitted taskId and explicit null identically.
    task_id: Option<String>,
}

impl From<FixtureSelectedTaskOperation> for SelectedTaskOperation {
    fn from(operation: FixtureSelectedTaskOperation) -> Self {
        Self {
            clock: operation.clock.into(),
            task_id: operation
                .task_id
                .map_or(SelectedTaskField::Deselected, SelectedTaskField::Selected),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FixtureSelectedTaskOutput {
    selected_task_id: Option<String>,
}

pub fn reduce_selected_task_json(input: &str) -> Result<String, CoreError> {
    let value = strict_fixture_input(input, true)?;
    let input: FixtureSelectedTaskInput = serde_json::from_value(value)?;
    let operations = input
        .operations
        .into_iter()
        .map(SelectedTaskOperation::from)
        .collect();
    let output = replay_selected_task(None, operations, input.active_task_ids)?;
    Ok(serde_json::to_string(&FixtureSelectedTaskOutput {
        selected_task_id: output.selected_task_id,
    })?)
}

fn strict_fixture_input(input: &str, selected_task: bool) -> Result<serde_json::Value, CoreError> {
    let value = crate::strict_json::parse(input)?;
    let root = crate::strict_json::object(&value, "fixture projection")?;
    if selected_task {
        crate::strict_json::object_array_field(root, "operations", "operations", true)?;
        crate::strict_json::array_field(root, "activeTaskIds", "activeTaskIds", true)?;
    } else {
        for field in [
            "taskOperations",
            "durationOperations",
            "autoStartOperations",
        ] {
            crate::strict_json::object_array_field(root, field, field, true)?;
        }
    }
    Ok(value)
}
