//! Opt-in PWA choice state shares the completion lifecycle's persistence contract.
use serde_json::Value;

use super::model::{Compatibility, Input, Intent, ReplicationMode, Selection};
use super::{CoreError, completion_mutation, invalid, policy};

pub(super) fn parse(value: &Value) -> Result<Input, CoreError> {
    let mut legacy = value.clone();
    let lifecycle = legacy
        .as_object_mut()
        .and_then(|object| object.remove("lifecycle"));
    if lifecycle.is_some() {
        crate::strict_json::shape::validate(value, &crate::completion_schema::intent::REQUEST, "")?;
    }
    let mut input: Input = serde_json::from_value(legacy)?;
    if let Some(lifecycle) = lifecycle {
        if input.compatibility != Compatibility::PwaStorage
            || input.replication_mode != ReplicationMode::Centralized
        {
            return Err(invalid(
                "selection lifecycle requires centralized PWA intent",
            ));
        }
        input.lifecycle = Some(completion_mutation::parse_lifecycle(&lifecycle)?);
    }
    Ok(input)
}

pub(super) fn choice(input: &Input, workspace: &Value) -> Result<Option<Selection>, CoreError> {
    if input.lifecycle.is_none() {
        return Ok(None);
    }
    let phase = match input.intent {
        Intent::SelectPhase { phase } => phase,
        Intent::Skip => policy::skip(input, workspace)?,
        _ => return Ok(None),
    };
    let generation = input
        .selection
        .generation
        .parse::<i64>()
        .map_err(|_| invalid("invalid generation"))?
        .checked_add(1)
        .ok_or_else(|| invalid("selection generation exhausted"))?;
    Ok(Some(Selection {
        phase,
        generation: generation.to_string(),
        explicit: true,
    }))
}

pub(super) fn with_lifecycle(input: &Input, raw: String) -> Result<String, CoreError> {
    let Some(lifecycle) = &input.lifecycle else {
        return Ok(raw);
    };
    let mut output: Value = serde_json::from_str(&raw)?;
    let mut lifecycle = lifecycle.clone();
    if matches!(input.intent, Intent::SelectPhase { .. } | Intent::Skip) {
        let timer: Option<crate::timer::CanonicalTimer> =
            serde_json::from_value(output["projection"]["canonicalTimer"].clone())?;
        let history: Vec<crate::timer::HistoryItem> =
            serde_json::from_value(output["projection"]["history"].clone())?;
        if let Some(row) = crate::timer::workspace::natural_completion(timer.as_ref(), &history)? {
            lifecycle.remember(row);
        }
    }
    output["lifecycle"] = serde_json::to_value(lifecycle)?;
    Ok(output.to_string())
}

pub(super) fn begin_cycle(input: &Input, commands: &[Value], selection: &mut Selection) {
    if input.lifecycle.is_some() && commands.iter().any(|command| command["type"] == "start") {
        // The admitted new session consumes the previous choice. Its generation
        // remains the user's choice version, not a timer-operation counter.
        selection.explicit = false;
    }
}

#[cfg(test)]
mod tests {
    use crate::strict_json::shape::{Presence, Shape};

    #[test]
    fn intent_decoder_fields_require_shared_schema_guards() {
        crate::completion_schema::tests::assert_fields::<super::Input>(
            &crate::completion_schema::intent::REQUEST,
            &["lifecycle"],
        );
        crate::completion_schema::tests::assert_fields::<super::Selection>(
            &crate::completion_schema::intent::SELECTION,
            &[],
        );
        let Shape::Record(fields) = &crate::completion_schema::intent::SELECTION else {
            panic!("selection record")
        };
        crate::completion_schema::tests::assert_enum::<super::super::model::Phase>(
            &fields[0].shape,
        );
    }

    #[test]
    fn schema_presence_and_nullable_metadata_matches_actual_intent_decoder() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/pwa-selection-public-v1.json"))
                .unwrap();
        let mut raw = fixture["receipts"][0]["selected"]["input"].clone();
        raw.as_object_mut().unwrap().remove("lifecycle");
        let Shape::Record(fields) = &crate::completion_schema::intent::REQUEST else {
            panic!("intent record")
        };
        for field in fields.iter().filter(|field| field.name != "lifecycle") {
            let mut omitted = raw.clone();
            omitted.as_object_mut().unwrap().remove(field.name);
            let result = serde_json::from_value::<super::Input>(omitted);
            assert_eq!(
                result.is_err(),
                matches!(field.presence, Presence::Required),
                "{} required metadata",
                field.name
            );
            if matches!(field.shape, Shape::Nullable(_)) {
                let mut nullable = raw.clone();
                nullable[field.name] = serde_json::Value::Null;
                assert!(
                    serde_json::from_value::<super::Input>(nullable).is_ok(),
                    "{} nullable metadata",
                    field.name
                );
            }
        }
    }
}
