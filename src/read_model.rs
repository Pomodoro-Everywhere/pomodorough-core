use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{CoreError, timer::CanonicalTimer};

mod clock;
mod presentation;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Profile {
    AppleWorkspace,
    AndroidCoordinator,
    DesktopStorage,
    DesktopTerminal,
    PwaStorage,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum Source {
    Workspace(Value),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Interval {
    start: String,
    end: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    profile: Profile,
    source: Source,
    selected_phase: String,
    observed_at: String,
    calendar_intervals: Vec<Interval>,
    #[serde(default)]
    monotonic: Option<clock::Monotonic>,
    #[serde(skip)]
    lifecycle: crate::workspace_intent::completion_mutation::CompletionLifecycle,
    #[serde(skip)]
    selection: Option<ReadSelection>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadSelection {
    phase: String,
    generation: String,
    explicit: bool,
}

fn invalid(reason: &str) -> CoreError {
    CoreError::InvalidInput(reason.into())
}

fn parse_input(raw: &str) -> Result<Input, CoreError> {
    // Decode extensions separately to retain the shipped schema's exact error text.
    let mut value = crate::strict_json::parse(raw)?;
    crate::strict_json::shape::validate(&value, &crate::completion_schema::READ, "")?;
    let lifecycle = value
        .as_object_mut()
        .and_then(|object| object.remove("lifecycle"));
    let selection = value
        .as_object_mut()
        .and_then(|object| object.remove("selection"));
    let mut input: Input = serde_json::from_value(value)?;
    if input.profile != Profile::PwaStorage && (lifecycle.is_some() || selection.is_some()) {
        return Err(invalid(
            "completion presentation context requires PWA profile",
        ));
    }
    input.lifecycle = lifecycle
        .as_ref()
        .map(crate::workspace_intent::completion_mutation::parse_lifecycle)
        .transpose()?
        .unwrap_or_default();
    if let Some(selection) = &selection {
        crate::strict_json::object(selection, "read model selection")?;
    }
    input.selection = selection.map(serde_json::from_value).transpose()?;
    Ok(input)
}

pub(crate) fn read_json(raw: &str) -> Result<String, CoreError> {
    let input = parse_input(raw)?;
    let observed = crate::timer::parse_time(&input.observed_at)?;
    let day = day_bounds(&input.calendar_intervals, observed)?;
    if !matches!(
        input.selected_phase.as_str(),
        "focus" | "short_break" | "long_break"
    ) {
        return Err(invalid("invalid read model selected phase"));
    }
    clock::validate(input.monotonic.as_ref(), input.profile)?;
    validate_selection(&input)?;
    let workspace = workspace(&input)?;
    let Source::Workspace(raw) = &input.source;
    crate::workspace_intent::completion_mutation::lifecycle_evidence(&input.lifecycle, raw)?;
    let timer: Option<CanonicalTimer> =
        serde_json::from_value(workspace["canonicalTimer"].clone())?;
    let history: Vec<crate::timer::HistoryItem> =
        serde_json::from_value(workspace["history"].clone())?;
    if let Some(timer) = &timer {
        crate::timer::validate_canonical_timer(timer)?;
        if matches!(timer.status.as_str(), "running" | "paused")
            && history
                .iter()
                .any(|item| item.timer_id == timer.id || item.id == timer.id)
        {
            return Err(invalid("active read model timer overlaps history"));
        }
    }
    let cadence = presentation::cadence(&history, day)?;
    let canonical = clock::timer_view(timer.as_ref(), observed, input.monotonic.as_ref())?;
    let output = presentation::render(&input, &workspace, &history, &canonical, cadence)?;
    Ok(serde_json::to_string(&output)?)
}

fn validate_selection(input: &Input) -> Result<(), CoreError> {
    crate::workspace_intent::completion_mutation::validate_lifecycle(&input.lifecycle)?;
    if let Some(selection) = &input.selection {
        let generation = selection.generation.parse::<i64>();
        if input.profile != Profile::PwaStorage
            || selection.phase != input.selected_phase
            || !generation
                .is_ok_and(|value| value >= 0 && value.to_string() == selection.generation)
        {
            return Err(invalid("invalid read model selection"));
        }
    }
    Ok(())
}

fn day_bounds(
    intervals: &[Interval],
    observed: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), CoreError> {
    let mut bounds = intervals
        .iter()
        .map(|interval| {
            let start = crate::timer::parse_time(&interval.start)?;
            let end = crate::timer::parse_time(&interval.end)?;
            if start >= end
                || end - start < chrono::Duration::hours(23)
                || end - start > chrono::Duration::hours(25)
            {
                return Err(invalid("invalid read model calendar interval"));
            }
            Ok((start, end))
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    bounds.sort();
    if bounds.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(invalid("overlapping read model calendar intervals"));
    }
    bounds
        .into_iter()
        .find(|(start, end)| observed >= *start && observed < *end)
        .ok_or_else(|| invalid("missing read model calendar interval"))
}

fn workspace(input: &Input) -> Result<Value, CoreError> {
    match &input.source {
        Source::Workspace(value) => {
            crate::reconciliation::workspace::display::validate_profile(
                value,
                input.profile == Profile::PwaStorage,
            )?;
            let workspace = crate::strict_json::object(value, "read model workspace")?;
            if workspace.contains_key("now") {
                return Err(invalid("read model workspace must not supply now"));
            }
            let mut request = value.clone();
            request["now"] =
                json!(projection_time(value, input)?.unwrap_or_else(|| input.observed_at.clone()));
            let projected = crate::reconciliation::workspace::project_json(&request.to_string())?;
            let projected: Value = serde_json::from_str(&projected)?;
            Ok(projected["workspace"].clone())
        }
    }
}

fn projection_time(value: &Value, input: &Input) -> Result<Option<String>, CoreError> {
    let Some(monotonic) = &input.monotonic else {
        return Ok(None);
    };
    let mut request = value.clone();
    request["now"] = json!("1970-01-01T00:00:00Z");
    let unexpired = crate::reconciliation::workspace::project_json(&request.to_string())?;
    let unexpired: Value = serde_json::from_str(&unexpired)?;
    let timer = &unexpired["workspace"]["canonicalTimer"];
    if timer.is_null() {
        return Ok(None);
    }
    clock::replay_time(&serde_json::from_value(timer.clone())?, monotonic)
}

#[cfg(test)]
mod shape_guards {
    use crate::completion_schema::{self as schema, tests::assert_fields};

    #[test]
    fn read_decoder_fields_require_shared_shape_guards() {
        assert_fields::<super::Input>(&schema::READ, &["selection", "lifecycle"]);
        assert_fields::<super::ReadSelection>(&schema::SELECTION, &[]);
        assert_fields::<super::Interval>(&schema::INTERVAL, &[]);
        assert_fields::<super::clock::Monotonic>(&schema::MONOTONIC, &[]);
        assert_fields::<super::clock::Anchor>(&schema::ANCHOR, &[]);
    }
}
