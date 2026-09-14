use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use super::acknowledgements::PendingQueues;
use super::{CanonicalResponse, LocalQueues};
use crate::CoreError;

const QUEUES: [&str; 5] = [
    "commands",
    "taskOperations",
    "durationOperations",
    "autoStartOperations",
    "selectedTaskOperations",
];

pub(super) struct Policy {
    frozen: BTreeMap<String, BTreeMap<String, Value>>,
}

impl Policy {
    pub(super) fn validate_drops(
        &self,
        dropped: &std::collections::BTreeSet<String>,
    ) -> Result<(), CoreError> {
        if dropped
            .iter()
            .any(|id| self.frozen["commands"].contains_key(id))
        {
            return Err(CoreError::InvalidInput(
                "reconciliation would discard a possibly delivered dependent".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn from_request(value: &Value) -> Result<Self, CoreError> {
        let local = value
            .get("local")
            .or_else(|| value.get("pending"))
            .ok_or(CoreError::MissingProjection("local"))?;
        let local: LocalQueues = serde_json::from_value(local.clone())?;
        let original = serde_json::to_value(local)?;
        let empty = serde_json::json!({});
        let never_sent = value.get("neverSent").unwrap_or(&empty);
        let object = crate::strict_json::object(never_sent, "neverSent")?;
        if object.keys().any(|key| !QUEUES.contains(&key.as_str())) {
            return Err(CoreError::InvalidInput("unknown neverSent queue".into()));
        }
        let mut frozen = BTreeMap::new();
        for name in QUEUES {
            frozen.insert(
                name.to_owned(),
                frozen_queue(&original[name], &value["sent"][name], never_sent.get(name))?,
            );
        }
        Ok(Self { frozen })
    }

    pub(super) fn projection(
        self,
        pending: &PendingQueues,
        response: &CanonicalResponse,
    ) -> Result<PendingQueues, CoreError> {
        // Never move retained clocks: acknowledged ordering barriers are absent
        // from later queues. The canonical head covers those barriers on restart.
        let head = (response.server_hlc_wall_ms, response.server_hlc_counter);
        let projected = PendingQueues {
            commands: self.projectable("commands", &pending.commands, head)?,
            tasks: self.projectable("taskOperations", &pending.tasks, head)?,
            durations: self.projectable("durationOperations", &pending.durations, head)?,
            auto_start: self.projectable("autoStartOperations", &pending.auto_start, head)?,
            selected_task: self.projectable(
                "selectedTaskOperations",
                &pending.selected_task,
                head,
            )?,
        };
        validate_timer_order(&pending.commands)?;
        Ok(projected)
    }

    fn projectable<T: Clone + Serialize>(
        &self,
        name: &str,
        queue: &[T],
        head: (i64, i64),
    ) -> Result<Vec<T>, CoreError> {
        let mut safe = true;
        for operation in queue {
            let value = serde_json::to_value(operation)?;
            let id = value["id"]
                .as_str()
                .ok_or(CoreError::MissingProjection("operation.id"))?;
            if let Some(original) = self.frozen[name].get(id) {
                if original != &value {
                    return Err(CoreError::InvalidInput(
                        "reconciliation would rewrite a possibly delivered operation".into(),
                    ));
                }
                safe = false;
            }
            let wall = value["hlcWallMs"]
                .as_i64()
                .ok_or(CoreError::MissingProjection("operation.hlcWallMs"))?;
            let counter = value["hlcCounter"]
                .as_i64()
                .ok_or(CoreError::MissingProjection("operation.hlcCounter"))?;
            safe &= (wall, counter) > head;
        }
        Ok(if safe { queue.to_vec() } else { Vec::new() })
    }
}

fn validate_timer_order(commands: &[crate::timer::WireCommand]) -> Result<(), CoreError> {
    let mut ordered: Vec<_> = commands.iter().collect();
    ordered.sort_by_key(|command| (command.device_id.as_str(), command.device_sequence));
    for pair in ordered.windows(2) {
        let [parent, child] = pair else { continue };
        if parent.device_id != child.device_id {
            continue;
        }
        let key = |command: &crate::timer::WireCommand| (command.hlc_wall_ms, command.hlc_counter);
        if parent.device_sequence == child.device_sequence
            || key(parent) > key(child)
            || (key(parent) == key(child) && parent.id >= child.id)
        {
            return Err(CoreError::InvalidInput(
                "immutable timer clocks do not preserve device sequence".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_dependencies(
    commands: &[crate::timer::WireCommand],
    dependencies: &[super::TimerDependency],
) -> Result<(), CoreError> {
    let by_id: BTreeMap<_, _> = commands
        .iter()
        .map(|command| (command.id.as_str(), command))
        .collect();
    for dependency in dependencies {
        let (Some(parent), Some(child)) = (
            by_id.get(dependency.depends_on_operation_id.as_str()),
            by_id.get(dependency.operation_id.as_str()),
        ) else {
            continue;
        };
        let parent_key = (
            parent.hlc_wall_ms,
            parent.hlc_counter,
            &parent.device_id,
            &parent.id,
        );
        let child_key = (
            child.hlc_wall_ms,
            child.hlc_counter,
            &child.device_id,
            &child.id,
        );
        if parent_key >= child_key {
            return Err(CoreError::InvalidInput(
                "immutable timer dependency is not causally ordered".into(),
            ));
        }
    }
    Ok(())
}

fn frozen_queue(
    local: &Value,
    sent: &Value,
    never_sent: Option<&Value>,
) -> Result<BTreeMap<String, Value>, CoreError> {
    let mut frozen = BTreeMap::new();
    for operation in local
        .as_array()
        .ok_or(CoreError::MissingProjection("local queue"))?
    {
        let id = operation["id"]
            .as_str()
            .ok_or(CoreError::MissingProjection("operation.id"))?;
        frozen.insert(id.to_owned(), operation.clone());
    }
    if let Some(never_sent) = never_sent {
        let entries = never_sent
            .as_array()
            .ok_or_else(|| CoreError::InvalidInput("invalid neverSent queue".into()))?;
        for entry in entries {
            let id = entry
                .as_str()
                .ok_or_else(|| CoreError::InvalidInput("neverSent requires string IDs".into()))?;
            let in_sent = sent
                .as_array()
                .is_some_and(|sent| sent.iter().any(|item| item["id"] == id));
            if in_sent || frozen.remove(id).is_none() {
                return Err(CoreError::InvalidInput(
                    "invalid neverSent identity or delivery claim".into(),
                ));
            }
        }
    }
    Ok(frozen)
}
