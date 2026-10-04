use serde_json::{Value, json};

use super::{CoreError, invalid, number};

pub(super) struct Migration {
    pub(super) settings: Value,
    pub(super) operations: Vec<(&'static str, Value)>,
}

pub(super) fn migrate(raw: &Value) -> Result<Migration, CoreError> {
    let mut settings = raw.clone();
    let mut operations = Vec::new();
    if raw["durationSyncBootstrapped"] != true {
        operations.extend(durations(raw)?);
        retire(&mut settings, &["durations"], "durationSyncBootstrapped");
    }
    if raw["autoStartSyncBootstrapped"] != true {
        if raw["autoStartBreaks"] == true || raw["autoStartBreaksExplicit"] == true {
            operations.push((
                "autoStartOperations",
                json!({"enabled": raw["autoStartBreaks"] == true}),
            ));
        }
        retire(
            &mut settings,
            &["autoStartBreaks", "autoStartBreaksExplicit"],
            "autoStartSyncBootstrapped",
        );
    }
    if raw["selectedTaskSyncBootstrapped"] != true {
        if let Some(task) = selected_task(raw)? {
            operations.push(("selectedTaskOperations", json!({"taskId": task})));
        }
        retire(
            &mut settings,
            &["selectedTaskId", "selectedTaskIdExplicit"],
            "selectedTaskSyncBootstrapped",
        );
    }
    Ok(Migration {
        settings,
        operations,
    })
}

fn durations(raw: &Value) -> Result<Vec<(&'static str, Value)>, CoreError> {
    let mut operations = Vec::new();
    for (phase, default) in [("focus", 25), ("short_break", 5), ("long_break", 15)] {
        let minutes = &raw["durations"][phase];
        if minutes.is_null() {
            continue;
        }
        let duration_ms = number::minutes(minutes)? * 60_000;
        if duration_ms != default * 60_000 {
            operations.push((
                "durationOperations",
                json!({"ownerId": "bootstrap",
                "phase": phase, "durationMs": duration_ms}),
            ));
        }
    }
    Ok(operations)
}

fn selected_task(raw: &Value) -> Result<Option<Value>, CoreError> {
    if let Some(task) = raw["selectedTaskId"]
        .as_str()
        .filter(|task| !task.is_empty())
    {
        return Ok(Some(json!(task)));
    }
    if raw["selectedTaskIdExplicit"] != true {
        return Ok(None);
    }
    match raw.get("selectedTaskId") {
        Some(Value::Null) => Ok(Some(Value::Null)),
        _ => Err(invalid(
            "explicit legacy selection requires nonempty taskId or null",
        )),
    }
}

fn retire(settings: &mut Value, fields: &[&str], marker: &str) {
    let settings = settings.as_object_mut().unwrap();
    for field in fields {
        settings.remove(*field);
    }
    settings.insert(marker.into(), json!(true));
}
