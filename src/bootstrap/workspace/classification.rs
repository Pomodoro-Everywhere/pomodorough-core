use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::{Value, json};

use super::{Input, Profile};
use crate::CoreError;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Classification {
    pub(super) has_state: bool,
    completed_history_count: usize,
    display_history_count: usize,
}

pub(super) fn local(input: &Input, timer: &Value) -> Result<Classification, CoreError> {
    let base = &input.local.workspace["base"];
    let preferences = &input.local.preferences;
    let settings = if input.profile == Profile::PwaStorage {
        base
    } else {
        preferences
    };
    let pending = input.local.workspace["local"].as_object().unwrap();
    let history = timer["history"].as_array().unwrap();
    let raw_history = base["history"].as_array().unwrap();
    let selected = match input.profile {
        Profile::AppleWorkspace => false,
        Profile::AndroidRepository => !preferences["selectedTaskId"].is_null(),
        Profile::DesktopStorage => {
            !preferences["selectedTaskId"].is_null() || !base["selectedTaskId"].is_null()
        }
        Profile::PwaStorage => nonempty(&base["selectedTaskId"]),
    };
    let retained = matches!(
        input.profile,
        Profile::AppleWorkspace | Profile::DesktopStorage
    ) && (!base["canonicalTimer"].is_null() || !raw_history.is_empty());
    let changed_durations = durations_differ(input, settings)?;
    let has_state = pending
        .values()
        .any(|queue| !queue.as_array().unwrap().is_empty())
        || !timer["canonicalTimer"].is_null()
        || !history.is_empty()
        || retained
        || !base["tasks"].as_array().unwrap().is_empty()
        || selected
        || settings["autoStartBreaks"] == true
        || changed_durations;
    Ok(classify(input.profile, history, has_state))
}

pub(super) fn remote(input: &Input) -> Result<Classification, CoreError> {
    let remote = &input.remote;
    let selected = match input.profile {
        Profile::AppleWorkspace => false,
        Profile::PwaStorage => nonempty(&remote["selectedTaskId"]),
        Profile::AndroidRepository | Profile::DesktopStorage => !remote["selectedTaskId"].is_null(),
    };
    let history = remote["history"].as_array().unwrap();
    let changed_durations = durations_differ(input, remote)?;
    let has_state = !remote["canonicalTimer"].is_null()
        || !history.is_empty()
        || !remote["tasks"].as_array().unwrap().is_empty()
        || selected
        || remote["autoStartBreaks"] == true
        || changed_durations;
    Ok(classify(input.profile, history, has_state))
}

fn nonempty(value: &Value) -> bool {
    value.as_str().is_some_and(|value| !value.is_empty())
}

fn classify(profile: Profile, history: &[Value], has_state: bool) -> Classification {
    let completed_history_count = super::super::completed_history_count(history);
    let display_history_count = match profile {
        Profile::AppleWorkspace | Profile::DesktopStorage => completed_history_count,
        Profile::AndroidRepository => history
            .iter()
            .filter(|row| row["status"] == "completed")
            .count(),
        Profile::PwaStorage => pwa_display_count(history),
    };
    Classification {
        has_state,
        completed_history_count,
        display_history_count,
    }
}

fn pwa_display_count(history: &[Value]) -> usize {
    let mut identities = BTreeSet::new();
    let mut count = 0;
    for row in history {
        if row["status"]
            .as_str()
            .is_some_and(|status| !status.is_empty() && status != "completed")
        {
            continue;
        }
        let identity = row["timerId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(|id| ("timer", id))
            .or_else(|| {
                row["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map(|id| ("id", id))
            });
        if identity.is_none_or(|identity| identities.insert(identity)) {
            count += 1;
        }
    }
    count
}

fn durations_differ(input: &Input, settings: &Value) -> Result<bool, CoreError> {
    let defaults = json!({"focus": 1_500_000, "short_break": 300_000, "long_break": 900_000});
    let defaults = if input.profile == Profile::PwaStorage {
        input
            .local
            .preferences
            .get("defaultDurationsMs")
            .unwrap_or(&defaults)
    } else {
        &defaults
    };
    let durations =
        if settings["durationsMs"].is_null() && input.profile == Profile::AndroidRepository {
            json!({"focus": minutes(settings, "focusMinutes", 25)?,
            "short_break": minutes(settings, "shortBreakMinutes", 5)?,
            "long_break": minutes(settings, "longBreakMinutes", 15)?})
        } else {
            settings["durationsMs"].clone()
        };
    Ok(durations != *defaults)
}

fn minutes(settings: &Value, field: &str, default: i64) -> Result<i64, CoreError> {
    let value = settings
        .get(field)
        .map(Value::as_i64)
        .unwrap_or(Some(default))
        .ok_or_else(|| CoreError::InvalidInput(format!("invalid {field}")))?;
    if !(1..=240).contains(&value) {
        return Err(CoreError::InvalidInput(format!("invalid {field}")));
    }
    Ok(value * 60_000)
}
