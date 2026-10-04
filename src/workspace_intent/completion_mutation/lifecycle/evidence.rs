//! Durable original Finish inputs prove discharge after current provenance changes.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{State, invalid, model::Completion};
use crate::{
    CoreError,
    timer::{CanonicalTimer, HistoryItem, WireCommand},
};

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FinishEvidence {
    command: Value,
    source_timer: Value,
    source_history: Value,
}

impl FinishEvidence {
    fn validate(&self) -> Result<WireCommand, CoreError> {
        crate::strict_json::shape::validate(
            &serde_json::to_value(self)?,
            &crate::completion_schema::evidence::FINISH,
            "finish evidence",
        )?;
        crate::timer::workspace::validate_native_device(&serde_json::json!({
            "base": {"canonicalTimer": self.source_timer}}))?;
        let timer: CanonicalTimer = serde_json::from_value(self.source_timer.clone())?;
        let history: HistoryItem = serde_json::from_value(self.source_history.clone())?;
        let command: WireCommand = serde_json::from_value(self.command.clone())?;
        if command.kind != "finish"
            || command.timer_id != timer.id
            || command.phase != timer.phase
            || command.planned_duration_ms != timer.planned_duration_ms
            || crate::timer::parse_time(&command.occurred_at)?
                < crate::timer::parse_time(&timer.anchor_at)?
            || crate::timer::workspace::natural_completion(
                Some(&timer),
                std::slice::from_ref(&history),
            )?
            .is_none()
        {
            return Err(invalid("invalid original natural Finish evidence"));
        }
        replay_finish(timer, history, &command)?;
        Ok(command)
    }
}

fn replay_finish(
    timer: CanonicalTimer,
    history: HistoryItem,
    command: &WireCommand,
) -> Result<(), CoreError> {
    let replay = crate::timer::workspace::replay(
        Some(timer),
        vec![history],
        vec![command.clone()],
        &command.occurred_at,
    )?;
    if !replay.history.iter().any(|row| {
        row.timer_id == command.timer_id
            && row.phase == command.phase
            && row.command_id.as_deref() == Some(&command.id)
    }) {
        return Err(invalid(
            "original Finish evidence does not discharge completion",
        ));
    }
    Ok(())
}

pub(crate) fn validate(state: &State) -> Result<(), CoreError> {
    let mut ids = std::collections::BTreeSet::new();
    for item in state.finish_evidence.iter().flatten() {
        let command = item.validate()?;
        if !ids.insert(command.id.clone())
            || !state.consumed_completions.iter().any(|row| {
                row.command_id.as_deref() == Some(&command.id)
                    && row.timer_id == command.timer_id
                    && row.phase == command.phase
            })
        {
            return Err(invalid("Finish evidence lacks matching consumed identity"));
        }
    }
    Ok(())
}

pub(crate) fn workspace(state: &State, raw: &Value) -> Result<(), CoreError> {
    for item in state
        .consumed_completions
        .iter()
        .filter(|row| row.command_id.is_some())
    {
        consumption(state, raw, item)?;
    }
    Ok(())
}

fn consumption(state: &State, raw: &Value, item: &Completion) -> Result<(), CoreError> {
    let id = item.command_id.as_deref().unwrap();
    let retained = raw["local"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["id"] == id);
    if retained.is_some_and(|command| {
        command["type"] != "finish"
            || command["timerId"] != item.timer_id
            || command["phase"] != item.phase
    }) {
        return Err(invalid("consumed Finish disagrees with retained command"));
    }
    let durable = state
        .finish_evidence
        .iter()
        .flatten()
        .find(|evidence| evidence.command["id"] == id);
    if let (Some(retained), Some(durable)) = (retained, durable) {
        if retained != &durable.command {
            return Err(invalid(
                "retained Finish differs from durable original evidence",
            ));
        }
    }
    if let Some(durable) = durable {
        validate_current_session(durable, &raw["base"])?;
    }
    if retained.is_none() && !base_finish(&raw["base"], item, id) && durable.is_none() {
        return Err(invalid(
            "consumed Finish lacks raw or durable original evidence",
        ));
    }
    Ok(())
}

fn base_finish(base: &Value, item: &Completion, id: &str) -> bool {
    let history = base["history"].as_array().unwrap().iter().any(|row| {
        row["commandId"] == id
            && row["timerId"] == item.timer_id
            && row["phase"] == item.phase
            && row["status"] == "completed"
    });
    let timer = &base["canonicalTimer"];
    history
        || (timer["id"] == item.timer_id
            && timer["phase"] == item.phase
            && timer["status"] == "completed"
            && timer["lastIntent"]["type"] == "finish"
            && timer["lastIntent"]["commandId"] == id)
}

fn validate_current_session(evidence: &FinishEvidence, base: &Value) -> Result<(), CoreError> {
    let original = &evidence.source_history;
    let row = base["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["timerId"] == original["timerId"]);
    if row.is_some_and(|row| {
        ["phase", "plannedDurationMs", "taskId"]
            .iter()
            .any(|key| row[*key] != original[*key])
    }) {
        return Err(invalid(
            "current session differs from original Finish evidence",
        ));
    }
    Ok(())
}

pub(crate) fn remember(
    state: &mut State,
    command: &Value,
    timer: &Value,
    history: &Value,
) -> Result<(), CoreError> {
    let Some(records) = state.finish_evidence.as_mut() else {
        return Ok(());
    };
    if !records
        .iter()
        .any(|record| record.command["id"] == command["id"])
    {
        records.push(FinishEvidence {
            command: command.clone(),
            source_timer: timer.clone(),
            source_history: history.clone(),
        });
    }
    validate(state)
}

pub(crate) fn installation(state: &State, raw: &Value) -> Result<(), CoreError> {
    if !state.has_finish_evidence() {
        return Ok(());
    }
    let history: Vec<Value> = ["beforeHistory", "afterHistory"]
        .iter()
        .flat_map(|key| raw[*key].as_array().unwrap().iter().cloned())
        .collect();
    let records = serde_json::json!({"base": {"canonicalTimer": raw["canonicalTimer"], "history": history},
        "local": {"commands": raw["sentContext"]["commands"].as_array().cloned().unwrap_or_default()}});
    workspace(state, &records)
}

#[cfg(test)]
mod tests {
    #[test]
    fn durable_record_fields_keep_mandatory_shared_guards() {
        crate::completion_schema::tests::assert_fields::<super::FinishEvidence>(
            &crate::completion_schema::evidence::FINISH,
            &[],
        );
        let contract: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/pwa-finish-evidence-schema-v1.json"
        ))
        .unwrap();
        let crate::strict_json::shape::Shape::Record(fields) =
            &crate::completion_schema::evidence::FINISH
        else {
            panic!("evidence record")
        };
        assert_eq!(fields.len(), contract["fields"].as_object().unwrap().len());
        for field in *fields {
            assert!(matches!(
                field.presence,
                crate::strict_json::shape::Presence::Required
            ));
            assert!(!matches!(
                field.shape,
                crate::strict_json::shape::Shape::Nullable(_)
            ));
            assert_eq!(contract["fields"][field.name]["required"], true);
            assert_eq!(contract["fields"][field.name]["nullable"], false);
        }
    }
}
