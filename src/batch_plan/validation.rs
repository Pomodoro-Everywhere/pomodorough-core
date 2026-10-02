use std::collections::{BTreeMap, BTreeSet};

use super::schema::{DOMAINS, Dependency, Domain, IdQueues, Limits, Mode, Operation, Queues};
use crate::CoreError;

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

pub(super) fn invalid(message: &str) -> CoreError {
    CoreError::InvalidInput(format!("batch plan: {message}"))
}

pub(super) fn limits(mode: Mode, limits: &Limits) -> Result<(), CoreError> {
    let (per_domain, total) = if mode == Mode::Sync {
        (256, 512)
    } else {
        (4096, 8192)
    };
    if !(1..=per_domain).contains(&limits.per_domain) || !(1..=total).contains(&limits.total) {
        return Err(invalid("limits must be positive and within endpoint caps"));
    }
    Ok(())
}

pub(super) fn queues(queues: &Queues) -> Result<(), CoreError> {
    if queues.len() != DOMAINS.len() {
        return Err(invalid("all five queues are required"));
    }
    for (domain, queue) in queues {
        identifiers(queue.iter().map(|operation| operation.id.as_str()))?;
        for operation in queue {
            operation_clock(operation, *domain)?;
        }
    }
    timer_sequence(&queues[&Domain::Commands])
}

pub(super) fn saved(queues: &IdQueues) -> Result<(), CoreError> {
    if queues.len() != DOMAINS.len() {
        return Err(invalid("all five queues are required"));
    }
    for queue in queues.values() {
        identifiers(queue.iter().map(String::as_str))?;
    }
    Ok(())
}

fn identifiers<'a>(ids: impl Iterator<Item = &'a str>) -> Result<(), CoreError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if id.is_empty() || !seen.insert(id) || seen.len() > crate::MAX_COMMANDS {
            return Err(invalid(
                "empty, duplicate or more than 10000 IDs in a domain",
            ));
        }
    }
    Ok(())
}

fn operation_clock(operation: &Operation, domain: Domain) -> Result<(), CoreError> {
    if operation.device_id.is_empty()
        || !(0..=MAX_SAFE_INTEGER).contains(&operation.hlc_wall_ms)
        || !(0..=MAX_SAFE_INTEGER).contains(&operation.hlc_counter)
    {
        return Err(invalid("invalid operation clock"));
    }
    match (domain, operation.device_sequence) {
        (Domain::Commands, Some(sequence))
            if (1..=MAX_SAFE_INTEGER).contains(&sequence) && operation.hlc_wall_ms > 0 =>
        {
            Ok(())
        }
        (Domain::Commands, _) => Err(invalid("timer requires positive clock and deviceSequence")),
        (_, None) => Ok(()),
        _ => Err(invalid("deviceSequence is only valid for commands")),
    }
}

fn timer_sequence(commands: &[Operation]) -> Result<(), CoreError> {
    let mut ordered: Vec<_> = commands.iter().collect();
    ordered.sort_by_key(|command| (&command.device_id, command.device_sequence));
    for pair in ordered.windows(2) {
        let [parent, child] = pair else { continue };
        if parent.device_id == child.device_id
            && (parent.device_sequence == child.device_sequence || parent.key() >= child.key())
        {
            return Err(invalid("timer clock order contradicts device sequence"));
        }
    }
    Ok(())
}

pub(super) fn dependencies(
    commands: &[Operation],
    dependencies: &[Dependency],
) -> Result<BTreeSet<String>, CoreError> {
    let by_id: BTreeMap<_, _> = commands
        .iter()
        .map(|command| (&command.id, command))
        .collect();
    let mut held = BTreeSet::new();
    for dependency in dependencies {
        let parent = by_id.get(&dependency.depends_on_operation_id);
        let child = by_id.get(&dependency.operation_id);
        let (Some(parent), Some(child)) = (parent, child) else {
            return Err(invalid("dependency endpoints must exist in commands"));
        };
        // Strictly increasing keys also prove the graph is acyclic.
        if parent.key() >= child.key() || !held.insert(child.id.clone()) {
            return Err(invalid("duplicate child or noncausal timer dependency"));
        }
    }
    Ok(held)
}

pub(super) fn fits(counts: &BTreeMap<Domain, usize>, limits: &Limits) -> bool {
    counts.values().all(|count| *count <= limits.per_domain)
        && counts.values().sum::<usize>() <= limits.total
}
