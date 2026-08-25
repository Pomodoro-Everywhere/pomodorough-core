use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::{DateTime, Utc};

use crate::CoreError;
use crate::timer::WireCommand;

use super::{CanonicalResponse, TimerDependency};

pub(super) struct TimerDependencyResolution {
    pub(super) dropped_operation_ids: BTreeSet<String>,
    pub(super) dropped_timer_ids: BTreeSet<String>,
    pub(super) promoted_operation_ids: BTreeSet<String>,
    pub(super) pending_dependencies: Vec<TimerDependency>,
}

struct TimerCommandIndex {
    positions: BTreeMap<String, usize>,
    ids: BTreeSet<String>,
}

struct TimerDependencyGraph {
    parent_by_child: BTreeMap<String, String>,
    children_by_parent: BTreeMap<String, Vec<String>>,
    generated_by_source: BTreeMap<String, String>,
    generated_ranges: BTreeMap<String, (DateTime<Utc>, DateTime<Utc>)>,
}

struct TimerAcknowledgements {
    outcomes: BTreeMap<String, String>,
    ids: BTreeSet<String>,
}

struct GeneratedBreakResolution {
    accepted_sources: BTreeSet<String>,
    invalid_sources: BTreeSet<String>,
    promoted: BTreeSet<String>,
    dropped_timer_ids: BTreeSet<String>,
}

enum GeneratedBreakOutcome {
    Accepted(BTreeSet<String>),
    Rejected(String),
}

struct GeneratedBreakContext<'a> {
    commands: &'a mut [WireCommand],
    index: &'a TimerCommandIndex,
    graph: &'a TimerDependencyGraph,
    response: &'a CanonicalResponse,
    acknowledged_ids: &'a BTreeSet<String>,
    source_id: &'a str,
    generated_start_id: &'a str,
}

pub(super) fn resolve(
    commands: &mut [WireCommand],
    dependencies: &[TimerDependency],
    response: &CanonicalResponse,
) -> Result<TimerDependencyResolution, CoreError> {
    let index = index_timer_commands(commands)?;
    let graph = validate_timer_dependency_graph(commands, dependencies, &index)?;
    let acknowledgements = timer_acknowledgements(response);
    let generated =
        reconcile_generated_breaks(commands, &index, &graph, response, &acknowledgements)?;
    let barriers = applied_timer_barriers(&acknowledgements, &generated);
    let dropped = dropped_timer_operations(&acknowledgements, &graph, &generated, &barriers);
    let mut promoted = generated.promoted;
    promote_timer_dependents(
        &mut promoted,
        &barriers,
        &graph,
        &acknowledgements.ids,
        &dropped,
    );
    let pending_dependencies = pending_timer_dependencies(
        dependencies,
        &index.ids,
        &acknowledgements.ids,
        &dropped,
        &promoted,
    );
    Ok(TimerDependencyResolution {
        dropped_operation_ids: dropped,
        dropped_timer_ids: generated.dropped_timer_ids,
        promoted_operation_ids: promoted,
        pending_dependencies,
    })
}

fn index_timer_commands(commands: &[WireCommand]) -> Result<TimerCommandIndex, CoreError> {
    let positions = commands
        .iter()
        .enumerate()
        .map(|(index, command)| (command.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let ids = positions.keys().cloned().collect::<BTreeSet<_>>();
    if ids.len() != commands.len() {
        return Err(CoreError::InvalidInput(
            "duplicate local timer operation id".into(),
        ));
    }
    Ok(TimerCommandIndex { positions, ids })
}

fn validate_timer_dependency_graph(
    commands: &[WireCommand],
    dependencies: &[TimerDependency],
    index: &TimerCommandIndex,
) -> Result<TimerDependencyGraph, CoreError> {
    let mut graph = TimerDependencyGraph {
        parent_by_child: BTreeMap::new(),
        children_by_parent: BTreeMap::new(),
        generated_by_source: BTreeMap::new(),
        generated_ranges: BTreeMap::new(),
    };
    for dependency in dependencies {
        insert_timer_dependency(dependency, &index.ids, &mut graph.parent_by_child)?;
        register_generated_dependency(commands, dependency, index, &mut graph)?;
    }
    validate_acyclic_timer_dependencies(&graph.parent_by_child)?;
    graph.children_by_parent = timer_children_by_parent(&graph.parent_by_child);
    Ok(graph)
}

fn insert_timer_dependency(
    dependency: &TimerDependency,
    command_ids: &BTreeSet<String>,
    parent_by_child: &mut BTreeMap<String, String>,
) -> Result<(), CoreError> {
    if dependency.operation_id.is_empty()
        || dependency.depends_on_operation_id.is_empty()
        || dependency.operation_id == dependency.depends_on_operation_id
        || !command_ids.contains(&dependency.operation_id)
        || !command_ids.contains(&dependency.depends_on_operation_id)
        || parent_by_child
            .insert(
                dependency.operation_id.clone(),
                dependency.depends_on_operation_id.clone(),
            )
            .is_some()
    {
        return Err(CoreError::InvalidInput(
            "invalid timer dependency graph".into(),
        ));
    }
    Ok(())
}

fn register_generated_dependency(
    commands: &[WireCommand],
    dependency: &TimerDependency,
    index: &TimerCommandIndex,
    graph: &mut TimerDependencyGraph,
) -> Result<(), CoreError> {
    let (start, end) = match (
        dependency.generated_break,
        dependency.source_day_start.as_deref(),
        dependency.source_day_end.as_deref(),
    ) {
        (false, None, None) => return Ok(()),
        (true, Some(start), Some(end)) => (start, end),
        _ => {
            return Err(CoreError::InvalidInput(
                "invalid generated break dependency".into(),
            ));
        }
    };
    let child = &commands[index.positions[&dependency.operation_id]];
    let parent = &commands[index.positions[&dependency.depends_on_operation_id]];
    let start = parse_dependency_time(start)?;
    let end = parse_dependency_time(end)?;
    let range_ms = end.timestamp_millis() - start.timestamp_millis();
    if child.kind != "start"
        || !matches!(child.phase.as_str(), "short_break" | "long_break")
        || parent.kind != "finish"
        || parent.phase != "focus"
        || !(1..=26 * 60 * 60 * 1_000).contains(&range_ms)
        || graph
            .generated_by_source
            .insert(
                dependency.depends_on_operation_id.clone(),
                dependency.operation_id.clone(),
            )
            .is_some()
    {
        return Err(CoreError::InvalidInput(
            "invalid generated break dependency".into(),
        ));
    }
    graph
        .generated_ranges
        .insert(dependency.operation_id.clone(), (start, end));
    Ok(())
}

fn validate_acyclic_timer_dependencies(
    parent_by_child: &BTreeMap<String, String>,
) -> Result<(), CoreError> {
    let mut visit_state = BTreeMap::new();
    for child in parent_by_child.keys() {
        if visit_state.get(child) == Some(&2_u8) {
            continue;
        }
        let mut path = Vec::new();
        let mut current = child.as_str();
        loop {
            match visit_state.get(current) {
                Some(1) => {
                    return Err(CoreError::InvalidInput(
                        "cyclic timer dependency graph".into(),
                    ));
                }
                Some(2) => break,
                _ => {
                    visit_state.insert(current.to_owned(), 1_u8);
                    path.push(current.to_owned());
                }
            }
            let Some(parent) = parent_by_child.get(current) else {
                break;
            };
            current = parent.as_str();
        }
        for identifier in path {
            visit_state.insert(identifier, 2_u8);
        }
    }
    Ok(())
}

fn timer_children_by_parent(
    parent_by_child: &BTreeMap<String, String>,
) -> BTreeMap<String, Vec<String>> {
    let mut children = BTreeMap::new();
    for (child, parent) in parent_by_child {
        children
            .entry(parent.clone())
            .or_insert_with(Vec::new)
            .push(child.clone());
    }
    children
}

fn timer_acknowledgements(response: &CanonicalResponse) -> TimerAcknowledgements {
    let outcomes = response
        .acknowledgements
        .iter()
        .filter_map(|acknowledgement| {
            Some((
                acknowledgement.get("commandId")?.as_str()?.to_owned(),
                acknowledgement.get("outcome")?.as_str()?.to_owned(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let ids = outcomes.keys().cloned().collect();
    TimerAcknowledgements { outcomes, ids }
}

fn reconcile_generated_breaks(
    commands: &mut [WireCommand],
    index: &TimerCommandIndex,
    graph: &TimerDependencyGraph,
    response: &CanonicalResponse,
    acknowledgements: &TimerAcknowledgements,
) -> Result<GeneratedBreakResolution, CoreError> {
    let mut resolution = GeneratedBreakResolution {
        accepted_sources: BTreeSet::new(),
        invalid_sources: BTreeSet::new(),
        promoted: BTreeSet::new(),
        dropped_timer_ids: BTreeSet::new(),
    };
    for (source_id, generated_start_id) in &graph.generated_by_source {
        let Some(outcome) = acknowledgements.outcomes.get(source_id) else {
            continue;
        };
        let context = GeneratedBreakContext {
            commands,
            index,
            graph,
            response,
            acknowledged_ids: &acknowledgements.ids,
            source_id,
            generated_start_id,
        };
        match context.reconcile(outcome)? {
            GeneratedBreakOutcome::Accepted(promoted) => {
                resolution.accepted_sources.insert(source_id.clone());
                resolution.promoted.extend(promoted);
            }
            GeneratedBreakOutcome::Rejected(timer_id) => {
                resolution.invalid_sources.insert(source_id.clone());
                resolution.dropped_timer_ids.insert(timer_id);
            }
        }
    }
    Ok(resolution)
}

impl GeneratedBreakContext<'_> {
    fn reconcile(mut self, outcome: &str) -> Result<GeneratedBreakOutcome, CoreError> {
        let (day_start, day_end) = self.generated_range();
        let exact_completion = exact_focus_completion_time(self.response, self.source())?;
        if exact_completion
            .is_some_and(|completed_at| completed_at < day_start || completed_at >= day_end)
        {
            return Err(CoreError::InvalidInput(
                "generated break source day excludes canonical completion".into(),
            ));
        }
        if !self.is_accepted(outcome, exact_completion.is_some()) {
            return Ok(GeneratedBreakOutcome::Rejected(
                self.generated_start().timer_id.clone(),
            ));
        }
        Ok(GeneratedBreakOutcome::Accepted(self.apply_accepted()?))
    }

    fn apply_accepted(&mut self) -> Result<BTreeSet<String>, CoreError> {
        let batch_ids = self.batch_ids();
        validate_generated_break_batch(
            self.commands,
            &self.index.positions,
            self.generated_start_id,
            &batch_ids,
        )?;
        let (phase, duration_ms) = self.phase_and_duration(&batch_ids)?;
        Ok(self.normalize(&batch_ids, &phase, duration_ms))
    }

    fn is_accepted(&self, outcome: &str, exact_completion: bool) -> bool {
        let source = self.source();
        let generated_start = self.generated_start();
        let canonical_supersedes = self
            .response
            .canonical_timer
            .0
            .as_ref()
            .is_some_and(|timer| {
                matches!(timer.status.as_str(), "running" | "paused")
                    && timer.id != source.timer_id
                    && timer.id != generated_start.timer_id
            });
        let newer_manual_start = self.commands.iter().any(|command| {
            command.kind == "start"
                && command.id != generated_start.id
                && command.device_sequence > generated_start.device_sequence
                && !self.graph.parent_by_child.contains_key(&command.id)
        });
        matches!(outcome, "applied" | "ignored")
            && exact_completion
            && !canonical_supersedes
            && !newer_manual_start
    }

    fn batch_ids(&self) -> BTreeSet<String> {
        let generated_timer_id = self.generated_start().timer_id.as_str();
        descendants(self.source_id, &self.graph.children_by_parent)
            .into_iter()
            .filter(|identifier| self.command(identifier).timer_id == generated_timer_id)
            .collect()
    }

    fn phase_and_duration(&self, batch_ids: &BTreeSet<String>) -> Result<(String, i64), CoreError> {
        let generated_start = self.generated_start();
        if batch_ids
            .iter()
            .any(|identifier| self.command(identifier).kind == "finish")
        {
            return Ok((
                generated_start.phase.clone(),
                generated_start.planned_duration_ms,
            ));
        }
        let (day_start, day_end) = self.generated_range();
        let completed = completed_focus_count(self.response, self.source(), day_start, day_end)?;
        let phase = if completed > 0 && completed % 4 == 0 {
            "long_break"
        } else {
            "short_break"
        };
        Ok((phase.to_owned(), self.response.durations_ms[phase]))
    }

    fn normalize(
        &mut self,
        batch_ids: &BTreeSet<String>,
        phase: &String,
        duration_ms: i64,
    ) -> BTreeSet<String> {
        let mut promoted = BTreeSet::new();
        for identifier in batch_ids {
            let command = &mut self.commands[self.index.positions[identifier]];
            command.phase.clone_from(phase);
            command.planned_duration_ms = duration_ms;
            command.observed_elapsed_ms = command.observed_elapsed_ms.clamp(0, duration_ms);
            if !self.acknowledged_ids.contains(identifier) {
                promoted.insert(identifier.clone());
            }
        }
        promoted
    }

    fn source(&self) -> &WireCommand {
        self.command(self.source_id)
    }

    fn generated_start(&self) -> &WireCommand {
        self.command(self.generated_start_id)
    }

    fn command(&self, identifier: &str) -> &WireCommand {
        &self.commands[self.index.positions[identifier]]
    }

    fn generated_range(&self) -> (DateTime<Utc>, DateTime<Utc>) {
        self.graph.generated_ranges[self.generated_start_id]
    }
}

fn applied_timer_barriers(
    acknowledgements: &TimerAcknowledgements,
    generated: &GeneratedBreakResolution,
) -> BTreeSet<String> {
    let mut barriers = acknowledgements
        .outcomes
        .iter()
        .filter(|(identifier, outcome)| {
            outcome.as_str() == "applied" && !generated.invalid_sources.contains(*identifier)
        })
        .map(|(identifier, _)| identifier.clone())
        .collect::<BTreeSet<_>>();
    barriers.extend(generated.accepted_sources.iter().cloned());
    barriers
}

fn dropped_timer_operations(
    acknowledgements: &TimerAcknowledgements,
    graph: &TimerDependencyGraph,
    generated: &GeneratedBreakResolution,
    barriers: &BTreeSet<String>,
) -> BTreeSet<String> {
    let (mut dropped, queue) = seeded_timer_drops(acknowledgements, generated);
    cascade_timer_drops(&mut dropped, queue, graph, barriers);
    // Acknowledged operations are removed by normal acknowledgement handling.
    // The separate list tells persistence adapters which dependents to remove.
    for identifier in &acknowledgements.ids {
        dropped.remove(identifier);
    }
    dropped
}

fn seeded_timer_drops(
    acknowledgements: &TimerAcknowledgements,
    generated: &GeneratedBreakResolution,
) -> (BTreeSet<String>, VecDeque<String>) {
    let mut dropped = BTreeSet::new();
    let mut queue = VecDeque::new();
    for (identifier, outcome) in &acknowledgements.outcomes {
        if outcome != "applied"
            && !generated.accepted_sources.contains(identifier)
            && dropped.insert(identifier.clone())
        {
            queue.push_back(identifier.clone());
        }
    }
    for identifier in &generated.invalid_sources {
        if dropped.insert(identifier.clone()) {
            queue.push_back(identifier.clone());
        }
    }
    (dropped, queue)
}

fn cascade_timer_drops(
    dropped: &mut BTreeSet<String>,
    mut queue: VecDeque<String>,
    graph: &TimerDependencyGraph,
    barriers: &BTreeSet<String>,
) {
    while let Some(parent) = queue.pop_front() {
        for child in graph
            .children_by_parent
            .get(parent.as_str())
            .into_iter()
            .flatten()
        {
            if !barriers.contains(child) && dropped.insert(child.clone()) {
                queue.push_back(child.clone());
            }
        }
    }
}

fn promote_timer_dependents(
    promoted: &mut BTreeSet<String>,
    barriers: &BTreeSet<String>,
    graph: &TimerDependencyGraph,
    acknowledged_ids: &BTreeSet<String>,
    dropped: &BTreeSet<String>,
) {
    for parent in barriers {
        for child in graph.children_by_parent.get(parent).into_iter().flatten() {
            if !acknowledged_ids.contains(child) && !dropped.contains(child) {
                promoted.insert(child.clone());
            }
        }
    }
    promoted.retain(|identifier| !dropped.contains(identifier));
}

fn pending_timer_dependencies(
    dependencies: &[TimerDependency],
    command_ids: &BTreeSet<String>,
    acknowledged_ids: &BTreeSet<String>,
    dropped: &BTreeSet<String>,
    promoted: &BTreeSet<String>,
) -> Vec<TimerDependency> {
    let pending_ids = command_ids
        .difference(acknowledged_ids)
        .filter(|identifier| !dropped.contains(*identifier))
        .cloned()
        .collect::<BTreeSet<_>>();
    dependencies
        .iter()
        .filter(|dependency| {
            !promoted.contains(&dependency.operation_id)
                && pending_ids.contains(&dependency.operation_id)
                && pending_ids.contains(&dependency.depends_on_operation_id)
        })
        .cloned()
        .collect()
}

fn parse_dependency_time(value: &str) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| CoreError::InvalidTimestamp(value.to_owned()))
}

fn descendants(root: &str, children_by_parent: &BTreeMap<String, Vec<String>>) -> BTreeSet<String> {
    let mut descendants = BTreeSet::new();
    let mut queue = VecDeque::from([root.to_owned()]);
    while let Some(parent) = queue.pop_front() {
        for child in children_by_parent.get(&parent).into_iter().flatten() {
            if descendants.insert(child.clone()) {
                queue.push_back(child.clone());
            }
        }
    }
    descendants
}

fn validate_generated_break_batch(
    commands: &[WireCommand],
    command_positions: &BTreeMap<String, usize>,
    generated_start_id: &str,
    batch_ids: &BTreeSet<String>,
) -> Result<(), CoreError> {
    let generated_start = &commands[command_positions[generated_start_id]];
    if batch_ids.is_empty()
        || batch_ids.iter().any(|identifier| {
            let command = &commands[command_positions[identifier]];
            command.timer_id != generated_start.timer_id
                || !matches!(
                    command.kind.as_str(),
                    "start" | "pause" | "resume" | "finish" | "cancel" | "clear"
                )
                || (command.kind == "start" && command.id != generated_start_id)
        })
    {
        return Err(CoreError::InvalidInput(
            "invalid generated break command batch".into(),
        ));
    }
    Ok(())
}

fn exact_focus_completion_time(
    response: &CanonicalResponse,
    source: &WireCommand,
) -> Result<Option<DateTime<Utc>>, CoreError> {
    if let Some(item) = response.history.iter().find(|item| {
        item.timer_id == source.timer_id
            && item.command_id.as_deref() == Some(source.id.as_str())
            && item.phase == "focus"
            && item.status == "completed"
    }) {
        return item
            .completed_at
            .as_deref()
            .or(item.ended_at.as_deref())
            .map(parse_dependency_time)
            .transpose();
    }
    let exact_timer = response.canonical_timer.0.as_ref().filter(|timer| {
        timer.id == source.timer_id
            && timer.phase == "focus"
            && timer.status == "completed"
            && timer
                .last_intent
                .as_ref()
                .is_some_and(|intent| intent.kind == "finish" && intent.command_id == source.id)
    });
    exact_timer
        .map(|timer| parse_dependency_time(&timer.anchor_at))
        .transpose()
}

fn completed_focus_count(
    response: &CanonicalResponse,
    source: &WireCommand,
    day_start: DateTime<Utc>,
    day_end: DateTime<Utc>,
) -> Result<usize, CoreError> {
    let mut focuses = response
        .history
        .iter()
        .filter(|item| item.phase == "focus" && item.status == "completed")
        .map(|item| {
            let completed_at = item
                .completed_at
                .as_deref()
                .or(item.ended_at.as_deref())
                .ok_or_else(|| CoreError::InvalidInput("invalid timer history".into()))?;
            Ok((
                parse_dependency_time(completed_at)?,
                item.command_id
                    .clone()
                    .unwrap_or_else(|| item.timer_id.clone()),
                item,
            ))
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    focuses.retain(|(completed_at, _, _)| *completed_at >= day_start && *completed_at < day_end);
    focuses.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    Ok(focuses
        .iter()
        .position(|(_, _, item)| {
            item.command_id.as_deref() == Some(source.id.as_str())
                || item.timer_id == source.timer_id
        })
        .map_or(focuses.len() + 1, |index| index + 1))
}
