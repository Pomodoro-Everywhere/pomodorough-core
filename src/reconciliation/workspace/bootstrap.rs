use serde_json::{Value, json};

use super::{Input, pending_queues, validate_shape};
use crate::CoreError;
use crate::reconciliation::{clocks, delivery, timer_dependencies, validation};

pub(crate) enum Projection {
    DeliverySafe,
    Retained,
    Stored(Vec<crate::timer::WireCommand>),
}

pub(crate) fn timer(raw: &Value, projection: Projection) -> Result<Value, CoreError> {
    validate_shape(raw)?;
    let input: Input = serde_json::from_value(raw.clone())?;
    let head = input
        .canonical_head
        .0
        .map(|head| (head.wall_ms, head.counter));
    if let Some((wall, counter)) = head {
        crate::clock::validate_hlc_values(wall, counter)?;
    }
    validation::local_queue_ids(&input.local)?;
    clocks::validate_local(&input.local)?;
    timer_dependencies::validate_retained(&input.local.commands, &input.timer_dependencies)?;
    let policy = delivery::Policy::from_queues(&raw["local"], &json!({}), &input.never_sent)?;
    let pending = pending_queues(input.local);
    let projected = policy.project_queues(&pending, head)?;
    crate::timer::parse_time(&input.now)?;
    if input.base["canonicalTimer"].is_null() && pending.commands.is_empty() {
        return Ok(json!({"canonicalTimer": null, "history": input.base["history"]}));
    }
    let commands = match projection {
        Projection::DeliverySafe => projected.commands,
        Projection::Retained => pending.commands.clone(),
        Projection::Stored(commands) => commands,
    };
    replay(raw, pending.commands, commands, &input.now)
}

fn replay(
    raw: &Value,
    retained: Vec<crate::timer::WireCommand>,
    projected: Vec<crate::timer::WireCommand>,
    now: &str,
) -> Result<Value, CoreError> {
    crate::timer::workspace::validate_native_device(raw)?;
    let timer: Option<crate::timer::CanonicalTimer> =
        serde_json::from_value(raw["base"]["canonicalTimer"].clone())?;
    let history: Vec<crate::timer::HistoryItem> =
        serde_json::from_value(raw["base"]["history"].clone())?;
    // Bootstrap reads identities and presence, not task normalization. The timer
    // reducer validates every retained timer payload before delivery filtering.
    crate::timer::workspace::replay(timer.clone(), history.clone(), retained, now)?;
    let result = crate::timer::workspace::replay(timer, history, projected, now)?;
    Ok(json!({"canonicalTimer": result.canonical_timer, "history": result.history}))
}
