use serde_json::{Value, json};

use super::{FinishBatch, Request, allocation, invalid};
use crate::CoreError;
use crate::workspace_intent::model::{CommandKind, Compatibility, Input, Interval};

pub(super) fn append(
    request: &Request,
    input: &Input,
    day: &Interval,
    batch: &mut FinishBatch,
) -> Result<(), CoreError> {
    validate_candidates(request)?;
    let duration = batch.after["workspace"]["durationsMs"][&batch.next_phase]
        .as_i64()
        .ok_or_else(|| invalid("missing generated break duration"))?;
    allocation::advance(input, &mut batch.allocation)?;
    let mut start_input = request.context();
    start_input.selection.phase = serde_json::from_value(json!(batch.next_phase))?;
    let mut start = allocation::command(
        &start_input,
        &batch.allocation,
        &batch.after["workspace"],
        &batch.observation,
        CommandKind::Start,
        1,
    )?;
    start["plannedDurationMs"] = json!(duration);
    if request.compatibility == Compatibility::PwaStorage {
        start["dependsOnCommandId"] = batch.commands[0]["id"].clone();
        start["generatedBreak"] = json!(true);
    }
    record_start(request, input, day, start, batch)
}

fn record_start(
    request: &Request,
    input: &Input,
    day: &Interval,
    start: Value,
    batch: &mut FinishBatch,
) -> Result<(), CoreError> {
    let id = start["id"].as_str().unwrap().to_owned();
    let timer_id = start["timerId"].clone();
    super::super::append(&mut batch.workspace, &start)?;
    batch.workspace["timerDependencies"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "operationId": id, "dependsOnOperationId": batch.commands[0]["id"], "generatedBreak": true,
            "sourceDayStart": day.start, "sourceDayEnd": day.end
        }));
    batch.allocation.last_uuid = Some(request.identities.command_uuids[1].clone());
    batch
        .observation
        .command_times
        .insert(id.clone(), input.clock.physical_now.clone());
    if request.compatibility == Compatibility::AppleWorkspace {
        batch.records["provisionalBreak"] = json!({"focusTimerId": batch.commands[0]["timerId"],
            "finishCommandId": batch.commands[0]["id"], "breakTimerId": timer_id, "startCommandId": id});
    }
    batch.commands.push(start);
    Ok(())
}

fn validate_candidates(request: &Request) -> Result<(), CoreError> {
    if request.identities.command_uuids.len() != 2 || request.identities.timer_uuid.is_none() {
        return Err(invalid(
            "generated break requires two command identities and a timer identity",
        ));
    }
    if request.compatibility == Compatibility::PwaStorage
        && (request.local_tab_id.is_none()
            || request.lease_now_ms.is_none()
            || request.lease_duration_ms.is_none())
    {
        return Err(invalid("generated break requires PWA lease context"));
    }
    Ok(())
}

pub(super) fn owner_write(request: &Request, start: &Value) -> Result<Value, CoreError> {
    let now = request
        .lease_now_ms
        .ok_or_else(|| invalid("missing lease clock"))?;
    let duration = request
        .lease_duration_ms
        .ok_or_else(|| invalid("missing lease duration"))?;
    let expiry = now
        .checked_add(duration)
        .filter(|value| *value <= 9_007_199_254_740_991)
        .ok_or_else(|| invalid("lease expiry overflow"))?;
    Ok(
        json!({"kind": "recordTimerOwner", "timerId": start["timerId"],
        "deviceId": request.allocation.device_id, "tabId": request.local_tab_id,
        "leaseExpiresAtMs": expiry}),
    )
}
