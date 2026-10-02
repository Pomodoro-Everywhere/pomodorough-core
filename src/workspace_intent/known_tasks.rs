use std::collections::BTreeSet;

use serde_json::Value;

use super::model::{Compatibility, Input};
use super::{CoreError, invalid};

pub(super) fn validate(input: &Input, value: &Value) -> Result<(), CoreError> {
    if value.get("knownTasks").is_none() {
        return Ok(());
    }
    if !matches!(
        input.compatibility,
        Compatibility::DesktopStorage | Compatibility::DesktopTerminal
    ) {
        return Err(invalid(
            "unknown field `knownTasks` for compatibility profile",
        ));
    }
    let tasks = input
        .known_tasks
        .as_ref()
        .ok_or_else(|| invalid("knownTasks must be an array"))?;
    let mut seen = BTreeSet::new();
    for task in tasks {
        let (id, _) = crate::task::identity(&task.title)?;
        if task.id != id || !seen.insert(&task.id) {
            return Err(invalid("invalid or duplicate known task identity"));
        }
    }
    Ok(())
}

pub(super) fn contains(input: &Input, task_id: &str) -> bool {
    input
        .known_tasks
        .as_ref()
        .is_some_and(|tasks| tasks.iter().any(|task| task.id == task_id))
}
