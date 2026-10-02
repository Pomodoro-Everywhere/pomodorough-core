use serde_json::Value;

use crate::{CoreError, strict_json};

const DERIVED: [&str; 12] = [
    "hasLocalState",
    "hasRemoteState",
    "localHistoryCount",
    "remoteHistoryCount",
    "completedHistoryCount",
    "projectionResult",
    "hasState",
    "displayHistoryCount",
    "classification",
    "plan",
    "horizon",
    "projectionHorizon",
];

pub(super) fn request(value: &Value) -> Result<(), CoreError> {
    raw_object(value, "bootstrap")?;
    let local = &value["local"];
    raw_object(local, "local")?;
    nullable_string(local, "ownerId")?;
    nullable_string(value, "currentUserId")?;
    if !local["projectionPending"].is_null() && value["profile"] != "pwaStorage" {
        return Err(invalid(
            "persisted projectionPending requires pwaStorage profile",
        ));
    }
    let workspace = &local["workspace"];
    raw_object(workspace, "workspace")?;
    snapshot(&workspace["base"], "base")?;
    snapshot(&value["remote"], "remote")?;
    remote_timer(&value["remote"]["canonicalTimer"])?;
    let preferences = &local["preferences"];
    raw_object(preferences, "preferences")?;
    nullable_string(preferences, "selectedTaskId")?;
    boolean(preferences, "autoStartBreaks")?;
    if !preferences.as_object().unwrap().contains_key("durationsMs") {
        return Err(invalid("missing preferences.durationsMs"));
    }
    if !(value["profile"] == "androidRepository" && preferences["durationsMs"].is_null()) {
        durations(&preferences["durationsMs"])?;
    }
    if let Some(defaults) = preferences.get("defaultDurationsMs") {
        durations(defaults)?;
    }
    strict_json::object_array_field(
        strict_json::object(local, "local")?,
        "knownTasks",
        "knownTasks",
        true,
    )?;
    if local["knownTasks"].as_array().unwrap().len() > crate::MAX_BOOTSTRAP_HISTORY {
        return Err(invalid("knownTasks exceeds bootstrap limit"));
    }
    let queues = strict_json::object(&workspace["local"], "local queues")?;
    for queue in queues.values() {
        if queue
            .as_array()
            .is_some_and(|queue| queue.len() > crate::MAX_COMMANDS)
        {
            return Err(invalid("queue exceeds bootstrap limit"));
        }
    }
    Ok(())
}

fn snapshot(value: &Value, path: &str) -> Result<(), CoreError> {
    raw_object(value, path)?;
    let object = strict_json::object(value, path)?;
    for field in [
        "canonicalTimer",
        "history",
        "tasks",
        "durationsMs",
        "autoStartBreaks",
        "selectedTaskId",
    ] {
        if !object.contains_key(field) {
            return Err(invalid(&format!("missing {path}.{field}")));
        }
    }
    strict_json::nullable_object_field(object, "canonicalTimer", "canonicalTimer")?;
    for field in ["history", "tasks"] {
        strict_json::object_array_field(object, field, field, true)?;
        if value[field].as_array().unwrap().len() > crate::MAX_BOOTSTRAP_HISTORY {
            return Err(invalid(&format!("{field} exceeds bootstrap limit")));
        }
    }
    for row in value["history"].as_array().unwrap() {
        for field in ["id", "timerId", "status"] {
            if row
                .get(field)
                .is_some_and(|value| !value.is_null() && !value.is_string())
            {
                return Err(invalid(&format!("invalid history.{field}")));
            }
        }
    }
    durations(&value["durationsMs"])?;
    nullable_string(value, "selectedTaskId")?;
    boolean(value, "autoStartBreaks")
}

fn raw_object(value: &Value, path: &str) -> Result<(), CoreError> {
    let object = strict_json::object(value, path)?;
    if object.keys().any(|key| DERIVED.contains(&key.as_str())) {
        return Err(invalid(
            "bootstrap requires raw state, not derived classification",
        ));
    }
    Ok(())
}

fn remote_timer(value: &Value) -> Result<(), CoreError> {
    if value.is_null() {
        return Ok(());
    }
    let validate = || {
        let timer: crate::timer::CanonicalTimer = serde_json::from_value(value.clone())?;
        crate::timer::validate_canonical_timer(&timer)
    };
    validate().map_err(|error| {
        invalid(&format!(
            "bootstrap remote timer requires recovery: {error}"
        ))
    })
}

fn durations(value: &Value) -> Result<(), CoreError> {
    let map = strict_json::object(value, "durationsMs")?;
    if map.len() != 3
        || ["focus", "short_break", "long_break"].iter().any(|phase| {
            !value[phase]
                .as_i64()
                .is_some_and(|duration| (60_000..=14_400_000).contains(&duration))
        })
    {
        return Err(invalid("invalid bootstrap durationsMs"));
    }
    Ok(())
}

fn nullable_string(value: &Value, field: &str) -> Result<(), CoreError> {
    match value.get(field) {
        Some(Value::Null | Value::String(_)) => Ok(()),
        _ => Err(invalid(&format!("{field} must be a string or null"))),
    }
}

fn boolean(value: &Value, field: &str) -> Result<(), CoreError> {
    if value[field].is_boolean() {
        Ok(())
    } else {
        Err(invalid(&format!("invalid {field}")))
    }
}

fn invalid(reason: &str) -> CoreError {
    CoreError::InvalidInput(reason.into())
}
