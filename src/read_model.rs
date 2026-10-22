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
    let mut workspace = workspace;
    // The canonical timer is one small object shared by completion evidence;
    // only the full history moves out by value.
    let timer: Option<CanonicalTimer> =
        serde_json::from_value(workspace["canonicalTimer"].clone())?;
    let history: Vec<crate::timer::HistoryItem> =
        serde_json::from_value(workspace["history"].take())?;
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

fn parse_civil_interval(
    interval: &Interval,
    index: usize,
    observed: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>, usize), CoreError> {
    let start = crate::timer::parse_time(&interval.start)?;
    let end = crate::timer::parse_time(&interval.end)?;
    let duration = end - start;
    if start >= end
        || duration < chrono::Duration::hours(22)
        || duration > chrono::Duration::hours(26)
    {
        return Err(invalid(&format!(
            "invalid read model calendar interval index={index} start={} end={} duration_ms={} observed_at={}",
            interval.start,
            interval.end,
            duration.num_milliseconds(),
            observed.to_rfc3339(),
        )));
    }
    Ok((start, end, index))
}

fn day_bounds(
    intervals: &[Interval],
    observed: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), CoreError> {
    // Platform owns local-midnight boundaries; Core never derives days with
    // UTC arithmetic. Accept 22h..=26h (24h +/- max 2h DST shift; Troll is
    // the current 2h case, Lord Howe 30min fits inside). Date-line: skipped
    // dates emit no interval, repeated dates emit two distinct intervals, so
    // empty and overlapping stay rejected and over-26h stays a non-day.
    let mut bounds = Vec::with_capacity(intervals.len());
    for (index, interval) in intervals.iter().enumerate() {
        bounds.push(parse_civil_interval(interval, index, observed)?);
    }
    bounds.sort();
    if let Some(pair) = bounds.windows(2).find(|pair| pair[0].1 > pair[1].0) {
        let (first, second) = (pair[0].2, pair[1].2);
        return Err(invalid(&format!(
            "overlapping read model calendar intervals first_start={} first_end={} second_start={} second_end={} observed_at={}",
            intervals[first].start,
            intervals[first].end,
            intervals[second].start,
            intervals[second].end,
            observed.to_rfc3339(),
        )));
    }
    bounds
        .into_iter()
        .find(|(start, end, _)| observed >= *start && observed < *end)
        .map(|(start, end, _)| (start, end))
        .ok_or_else(|| {
            let summary = intervals
                .iter()
                .map(|interval| format!("{}->{}", interval.start, interval.end))
                .collect::<Vec<_>>()
                .join(",");
            invalid(&format!(
                "missing read model calendar interval observed_at={} intervals=[{summary}]",
                observed.to_rfc3339(),
            ))
        })
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
            let observed = clock::running_timer(input.monotonic.as_ref())?;
            let history = value["base"]["history"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0);
            let queued = value
                .get("local")
                .and_then(Value::as_object)
                .map(|queues| {
                    queues
                        .values()
                        .map(|queue| queue.as_array().map(Vec::len).unwrap_or(0))
                        .sum()
                })
                .unwrap_or(0);
            let mut output = crate::reconciliation::workspace::project_observed_value(
                request,
                observed.as_ref(),
            )?;
            crate::take_projection_workspace(&mut output, "workspace.readModel.v1", history, queued)
        }
    }
}

fn projection_time(value: &Value, input: &Input) -> Result<Option<String>, CoreError> {
    let Some(monotonic) = &input.monotonic else {
        return Ok(None);
    };
    let mut request = value.clone();
    request["now"] = json!("1970-01-01T00:00:00Z");
    let observed = clock::running_timer(input.monotonic.as_ref())?;
    let unexpired =
        crate::reconciliation::workspace::project_observed_value(request, observed.as_ref())?;
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
