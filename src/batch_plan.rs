use crate::CoreError;

mod schema;
mod validation;

use schema::{DOMAINS, Dependency, Domain, IdQueues, Limits, Mode, Plan, Queues, Request, Status};

pub(crate) fn plan_json(input: &str) -> Result<String, CoreError> {
    let request: Request = serde_json::from_value(crate::strict_json::parse(input)?)?;
    let plan = match request {
        Request::New {
            mode,
            limits,
            next_domain,
            queues,
            timer_dependencies,
        } => plan_new(mode, &limits, next_domain, queues, &timer_dependencies)?,
        Request::Saved {
            mode,
            limits,
            queues,
        } => plan_saved(mode, &limits, queues)?,
    };
    Ok(serde_json::to_string(&plan)?)
}

fn plan_saved(mode: Mode, limits: &Limits, queues: IdQueues) -> Result<Plan, CoreError> {
    validation::limits(mode, limits)?;
    validation::saved(&queues)?;
    let mut plan = Plan::empty(
        queues
            .iter()
            .map(|(domain, ids)| (*domain, ids.len()))
            .collect(),
        None,
    );
    require_empty_keep_remote(mode, plan.total)?;
    if validation::fits(&plan.counts, limits) {
        plan.status = Status::ReplaySaved;
        plan.selected = queues;
    } else {
        plan.status = Status::OversizedSaved;
    }
    Ok(plan)
}

fn plan_new(
    mode: Mode,
    limits: &Limits,
    next_domain: Domain,
    mut queues: Queues,
    dependencies: &[Dependency],
) -> Result<Plan, CoreError> {
    validation::limits(mode, limits)?;
    validation::queues(&queues)?;
    let held = validation::dependencies(&queues[&Domain::Commands], dependencies)?;
    for queue in queues.values_mut() {
        queue.sort_by(|left, right| left.key().cmp(&right.key()));
    }
    let mut plan = Plan::empty(
        queues
            .iter()
            .map(|(domain, queue)| (*domain, queue.len()))
            .collect(),
        Some(next_domain),
    );
    require_empty_keep_remote(mode, plan.total)?;
    plan.held_timer_operation_id = queues[&Domain::Commands]
        .iter()
        .find(|command| held.contains(&command.id))
        .map(|command| command.id.clone());
    if mode != Mode::Sync && !validation::fits(&plan.counts, limits) {
        plan.status = Status::Oversized;
    } else if mode != Mode::Sync && plan.held_timer_operation_id.is_some() {
        plan.status = Status::BlockedDependency;
    } else {
        select(&queues, limits, next_domain, &mut plan);
    }
    Ok(plan)
}

fn require_empty_keep_remote(mode: Mode, total: usize) -> Result<(), CoreError> {
    if mode == Mode::KeepRemote && total != 0 {
        return Err(validation::invalid("keep_remote requires empty queues"));
    }
    Ok(())
}

fn select(queues: &Queues, limits: &Limits, mut next: Domain, plan: &mut Plan) {
    let mut total = 0;
    let mut misses = 0;
    while total < limits.total && misses < DOMAINS.len() {
        let domain = next;
        next = next.next();
        let selected = plan
            .selected
            .get_mut(&domain)
            .expect("five validated domains");
        let candidate = queues[&domain].get(selected.len());
        let candidate = candidate.filter(|operation| {
            selected.len() < limits.per_domain
                && (domain != Domain::Commands
                    || Some(&operation.id) != plan.held_timer_operation_id.as_ref())
        });
        if let Some(operation) = candidate {
            selected.push(operation.id.clone());
            total += 1;
            misses = 0;
            plan.next_domain = Some(next);
        } else {
            misses += 1;
        }
    }
    if total == 0 && plan.held_timer_operation_id.is_some() {
        plan.status = Status::BlockedDependency;
    }
}
