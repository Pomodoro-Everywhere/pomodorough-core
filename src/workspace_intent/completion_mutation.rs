//! Completion decisions at each client's original local commit boundary.
mod generated_break;
mod lifecycle;

use chrono::SecondsFormat;
use serde::Deserialize;
use serde_json::{Value, json};

use super::model::{
    Allocation, Clock, CommandKind, Compatibility, Identities, Input, Intent, Interval,
    Observation, ReplicationMode, Selection,
};
use super::{CoreError, admission, allocation, invalid, monotonic, policy, projection};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Stage {
    FinishCommit,
    AutomaticFinishCommit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ownership {
    timer_id: String,
    device_id: String,
    #[serde(default)]
    tab_id: Option<String>,
    #[serde(default)]
    lease_expires_at_ms: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    stage: Stage,
    compatibility: Compatibility,
    replication_mode: ReplicationMode,
    workspace: Value,
    requested_timer: crate::timer::CanonicalTimer,
    selection: Selection,
    allocation: Allocation,
    observation: Observation,
    clock: Clock,
    identities: Identities,
    calendar_intervals: Vec<Interval>,
    ownership: Option<Ownership>,
    #[serde(default)]
    local_tab_id: Option<String>,
    #[serde(default)]
    lease_now_ms: Option<i64>,
    #[serde(default)]
    lease_duration_ms: Option<i64>,
    #[serde(default)]
    boundary_retry: Option<BoundaryRetry>,
    #[serde(default)]
    lifecycle: lifecycle::State,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BoundaryRetry {
    original_observed_at: String,
    measured_elapsed_ms: i64,
}

struct FinishBatch {
    workspace: Value,
    allocation: Allocation,
    observation: Observation,
    after: Value,
    commands: Vec<Value>,
    selection: Selection,
    records: Value,
    next_phase: String,
}

impl Request {
    fn context(&self) -> Input {
        Input {
            compatibility: self.compatibility,
            replication_mode: if self.replication_mode == ReplicationMode::Iroh {
                ReplicationMode::Iroh
            } else {
                ReplicationMode::Centralized
            },
            intent: Intent::Pause,
            workspace: self.workspace.clone(),
            requested_timer: Some(self.requested_timer.clone()),
            selection: self.selection.clone(),
            allocation: self.allocation.clone(),
            observation: self.observation.clone(),
            clock: Clock {
                occurred_at: self.clock.occurred_at.clone(),
                physical_now: self.clock.physical_now.clone(),
                observed_at: self.clock.observed_at.clone(),
                monotonic_now_ms: self.clock.monotonic_now_ms,
                continuity_id: self.clock.continuity_id.clone(),
            },
            identities: Identities {
                command_uuids: self.identities.command_uuids.clone(),
                timer_uuid: self.identities.timer_uuid.clone(),
            },
            calendar_intervals: self
                .calendar_intervals
                .iter()
                .map(|day| Interval {
                    start: day.start.clone(),
                    end: day.end.clone(),
                })
                .collect(),
            ownership: None,
            durability: None,
            local_durations_ms: None,
            known_tasks: None,
        }
    }

    fn supports_generated_break(&self) -> bool {
        self.replication_mode == ReplicationMode::Centralized
            && matches!(
                self.compatibility,
                Compatibility::AppleWorkspace
                    | Compatibility::AndroidCoordinator
                    | Compatibility::PwaStorage
            )
    }
}

pub(crate) fn plan_json(raw: &str) -> Result<String, CoreError> {
    crate::reconciliation::workspace::display::persist_result(plan_raw(raw)?)
}

fn plan_raw(raw: &str) -> Result<String, CoreError> {
    let value = crate::strict_json::parse(raw)?;
    if matches!(
        value["stage"].as_str(),
        Some("expiryObservation" | "deferredBreakOpportunity")
    ) {
        return lifecycle::plan_json(value);
    }
    if value.get("ownership").is_none() {
        return Err(invalid("missing completion ownership"));
    }
    if value["workspace"].get("now").is_some() {
        return Err(invalid("workspace.now is owned by completion planner"));
    }
    let requested_dependency = value["requestedTimer"].get("dependsOnCommandId").cloned();
    let request: Request = serde_json::from_value(value)?;
    let input = request.context();
    validate(&request, &input)?;
    plan_finish(&request, &input, requested_dependency.as_ref())
}

fn plan_finish(
    request: &Request,
    input: &Input,
    requested_dependency: Option<&Value>,
) -> Result<String, CoreError> {
    let before = unexpired(input)?;
    let timer = &before["canonicalTimer"];
    if !matches!(timer["status"].as_str(), Some("running" | "paused"))
        || presented_stale(request, input, timer)?
    {
        return noop(request, "staleTimer");
    }
    validate_requested_dependency(requested_dependency, input, timer)?;
    let (projected, observation) = match request.stage {
        Stage::FinishCommit => (timer.clone(), observed(input)?),
        Stage::AutomaticFinishCommit => {
            let (projected, observation) = automatic_projection(input)?;
            if let Some(noop) = automatic_denial(request, timer, &projected)? {
                return Ok(noop);
            }
            (projected, observation)
        }
    };
    let auto_start = effective_auto_start(input)?;
    let plan = completion_request(request, &projected, auto_start)?;
    if plan["commandEligible"] != true {
        return Err(invalid("finish eligibility is inconsistent"));
    }
    commit(
        request,
        input,
        &before,
        observation,
        plan["reserveGeneratedBreak"] == true,
    )
}

fn presented_stale(request: &Request, input: &Input, timer: &Value) -> Result<bool, CoreError> {
    if request.requested_timer.status == "completed"
        && matches!(
            request.compatibility,
            Compatibility::DesktopStorage | Compatibility::DesktopTerminal
        )
    {
        let (expired, _) = automatic_projection(input)?;
        return Ok(expired["status"] != "completed" || stale(request, &expired)?);
    }
    Ok(!matches!(
        request.requested_timer.status.as_str(),
        "running" | "paused"
    ) || stale(request, timer)?)
}

fn automatic_denial(
    request: &Request,
    timer: &Value,
    projected: &Value,
) -> Result<Option<String>, CoreError> {
    if timer["status"] != "running"
        || projected["id"] != timer["id"]
        || projected["status"] != "completed"
        || projected["lastIntent"]["type"] == "finish"
    {
        return noop(request, "notExpired").map(Some);
    }
    let (owned, retry_at_ms) = automatic_owner(request, timer)?;
    if !owned {
        let reason = if request.compatibility == Compatibility::PwaStorage {
            "not_owner"
        } else {
            "notOwner"
        };
        return noop_with_retry(request, reason, retry_at_ms).map(Some);
    }
    Ok(None)
}

fn validate(request: &Request, input: &Input) -> Result<(), CoreError> {
    validate_supported_context(request)?;
    policy::validate_generation(input)?;
    allocation::validate(input)?;
    monotonic::validate(input)?;
    crate::timer::validate_canonical_timer(&request.requested_timer)?;
    validate_completion_observation(request, input)?;
    validate_completion_workspace(input)?;
    lifecycle::validate_state(&request.lifecycle)?;
    Ok(())
}

fn validate_supported_context(request: &Request) -> Result<(), CoreError> {
    crate::reconciliation::workspace::display::validate_profile(
        &request.workspace,
        request.compatibility == Compatibility::PwaStorage,
    )?;
    if !matches!(request.identities.command_uuids.len(), 1 | 2)
        || (request.replication_mode == ReplicationMode::Iroh
            && (request.compatibility != Compatibility::AppleWorkspace
                || !matches!(request.stage, Stage::FinishCommit)))
    {
        return Err(invalid("unsupported completion context"));
    }
    if request.local_tab_id.is_some() != request.lease_now_ms.is_some()
        || (request.lease_duration_ms.is_some()
            && request.compatibility != Compatibility::PwaStorage)
        || (request.compatibility == Compatibility::PwaStorage
            && matches!(request.stage, Stage::AutomaticFinishCommit)
            && request.local_tab_id.is_none())
        || (request.compatibility != Compatibility::PwaStorage && request.local_tab_id.is_some())
    {
        return Err(invalid("invalid completion lease context"));
    }
    Ok(())
}

fn validate_completion_observation(request: &Request, input: &Input) -> Result<(), CoreError> {
    for stamp in [
        &input.clock.occurred_at,
        &input.clock.physical_now,
        &input.clock.observed_at,
    ] {
        crate::timer::parse_time(stamp)?;
    }
    calendar_bounds(&request.calendar_intervals)?;
    if crate::timer::parse_time(&input.clock.observed_at)?
        > crate::timer::parse_time(&input.clock.physical_now)?
        && request.boundary_retry.is_none()
    {
        return Err(invalid("observation is later than physical now"));
    }
    validate_boundary_retry(request)?;
    validate_lease(request)?;
    super::validate_observation(input)
}

fn validate_boundary_retry(request: &Request) -> Result<(), CoreError> {
    let observed = crate::timer::parse_time(&request.clock.observed_at)?;
    let physical = crate::timer::parse_time(&request.clock.physical_now)?;
    let Some(retry) = &request.boundary_retry else {
        return Ok(());
    };
    let original = crate::timer::parse_time(&retry.original_observed_at)?;
    if request.compatibility != Compatibility::AppleWorkspace
        || !matches!(request.stage, Stage::AutomaticFinishCommit)
        || original > physical
        || observed.signed_duration_since(original).num_milliseconds() != 10
        || observed.signed_duration_since(original) != chrono::Duration::milliseconds(10)
        || retry.measured_elapsed_ms < request.requested_timer.planned_duration_ms
    {
        return Err(invalid("invalid Apple millisecond boundary retry"));
    }
    Ok(())
}

fn validate_lease(request: &Request) -> Result<(), CoreError> {
    if request.ownership.as_ref().is_some_and(|owner| {
        owner.timer_id.is_empty()
            || owner.device_id.is_empty()
            || owner.tab_id.as_deref().is_some_and(str::is_empty)
            || owner.lease_expires_at_ms.is_some_and(|ms| ms < 0)
    }) {
        return Err(invalid("invalid completion ownership"));
    }
    if request.local_tab_id.as_deref().is_some_and(str::is_empty)
        || request
            .lease_now_ms
            .is_some_and(|ms| !(0..=9_007_199_254_740_991).contains(&ms))
        || request
            .lease_duration_ms
            .is_some_and(|ms| !(1..=9_007_199_254_740_991).contains(&ms))
        || (request.compatibility == Compatibility::PwaStorage
            && matches!(request.stage, Stage::AutomaticFinishCommit)
            && request.ownership.as_ref().is_some_and(|owner| {
                owner
                    .lease_expires_at_ms
                    .is_some_and(|ms| ms > 9_007_199_254_740_991)
            }))
    {
        return Err(invalid("invalid completion lease context"));
    }
    Ok(())
}

fn validate_completion_workspace(input: &Input) -> Result<(), CoreError> {
    if input.workspace["base"]["canonicalTimer"]
        .get("dependsOnCommandId")
        .is_some()
    {
        return Err(invalid("dependent canonical timer requires another stage"));
    }
    // Validation checks the raw aggregate without consuming an expiry before
    // the planner's physical observation and concurrent-Finish admission.
    projection::project(&input.workspace, "1970-01-01T00:00:00Z")?;
    Ok(())
}

fn validate_requested_dependency(
    presented: Option<&Value>,
    input: &Input,
    timer: &Value,
) -> Result<(), CoreError> {
    let parent = super::inherited_dependency(&input.workspace, &json!({"timerId": timer["id"]}));
    if presented.is_some_and(|value| Some(value) != parent.as_ref()) {
        return Err(invalid("dependent timer lacks matching parent"));
    }
    Ok(())
}

fn effective_auto_start(input: &Input) -> Result<bool, CoreError> {
    Ok(
        lifecycle::project(input, &input.workspace, &input.clock.occurred_at)?["workspace"]["autoStartBreaks"]
            == true,
    )
}

fn unexpired(input: &Input) -> Result<Value, CoreError> {
    let safe = lifecycle::observe(
        input,
        &input.workspace,
        &input.observation,
        "1970-01-01T00:00:00Z",
    )?["workspace"]
        .clone();
    projection::entrypoint(input, &safe, &input.observation, "1970-01-01T00:00:00Z")
}

fn stale(request: &Request, timer: &Value) -> Result<bool, CoreError> {
    let presented = serde_json::to_value(&request.requested_timer)?;
    if timer["lastIntent"]["type"] == "finish" || timer["lastIntent"]["type"] == "cancel" {
        return Ok(true);
    }
    Ok([
        "id",
        "status",
        "phase",
        "plannedDurationMs",
        "anchorAt",
        "elapsedAtAnchorMs",
        "taskId",
    ]
    .iter()
    .any(|key| presented[*key] != timer[*key])
        || !crate::timer::workspace::same_intent(&presented["lastIntent"], &timer["lastIntent"]))
}

fn observed(input: &Input) -> Result<Observation, CoreError> {
    if input.compatibility == Compatibility::PwaStorage {
        return monotonic::before(input).map(|(_, observation)| observation);
    }
    Ok(input.observation.clone())
}

fn automatic_projection(input: &Input) -> Result<(Value, Observation), CoreError> {
    if input.compatibility == Compatibility::PwaStorage {
        let (workspace, observation) = monotonic::before(input)?;
        return Ok((workspace["canonicalTimer"].clone(), observation));
    }
    let projected = lifecycle::observe(
        input,
        &input.workspace,
        &input.observation,
        &input.clock.observed_at,
    )?;
    let decision = projection::entrypoint(
        input,
        &projected["workspace"],
        &input.observation,
        &input.clock.observed_at,
    )?;
    Ok((
        decision["canonicalTimer"].clone(),
        input.observation.clone(),
    ))
}

fn automatic_owner(request: &Request, timer: &Value) -> Result<(bool, Option<i64>), CoreError> {
    let local = request.allocation.device_id.as_str();
    if request.compatibility != Compatibility::PwaStorage {
        return Ok((local_owner(request, timer, local), None));
    }
    let Some(owner) = &request.ownership else {
        return Ok((can_claim_missing_owner(request, timer, local), None));
    };
    if owner.timer_id != timer["id"] || owner.device_id != local {
        return Ok((false, None));
    }
    let lease_now = request
        .lease_now_ms
        .ok_or_else(|| invalid("missing lease clock"))?;
    if owner.tab_id.as_deref() == request.local_tab_id.as_deref()
        || owner
            .lease_expires_at_ms
            .is_none_or(|expiry| expiry <= lease_now)
    {
        Ok((true, None))
    } else {
        Ok((false, owner.lease_expires_at_ms))
    }
}

fn local_owner(request: &Request, timer: &Value, local: &str) -> bool {
    if request
        .ownership
        .as_ref()
        .is_some_and(|owner| owner.timer_id != timer["id"])
    {
        return false;
    }
    match request.compatibility {
        Compatibility::AppleWorkspace => {
            request
                .ownership
                .as_ref()
                .map(|owner| owner.device_id.as_str())
                .or(timer["startedByDeviceId"].as_str())
                == Some(local)
        }
        Compatibility::AndroidCoordinator => request
            .ownership
            .as_ref()
            .is_some_and(|owner| owner.device_id == local),
        Compatibility::DesktopStorage | Compatibility::DesktopTerminal => request
            .ownership
            .as_ref()
            .is_some_and(|owner| owner.device_id == local),
        Compatibility::PwaStorage => false,
    }
}

fn can_claim_missing_owner(request: &Request, timer: &Value, local: &str) -> bool {
    let canonical = &request.workspace["base"]["canonicalTimer"];
    if canonical["id"] == timer["id"] && canonical.get("startedByDeviceId").is_some() {
        return canonical["startedByDeviceId"] == local;
    }
    request.workspace["local"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["type"] == "start" && command["timerId"] == timer["id"])
}

fn completion_request(
    request: &Request,
    timer: &Value,
    auto_start: bool,
) -> Result<Value, CoreError> {
    let owner = if request.compatibility == Compatibility::PwaStorage {
        json!({"timerId": timer["id"], "ownerDeviceId": request.allocation.device_id})
    } else {
        completion_owner(request, timer)
    };
    let context = json!({"kind": "commandRequest", "commandType": "finish",
        "requestedTimer": request.requested_timer, "projectedTimer": timer,
        "automatic": matches!(request.stage, Stage::AutomaticFinishCommit),
        "generateAutoBreak": true, "autoStartBreaks": auto_start,
        "localDeviceId": request.allocation.device_id, "ownership": owner});
    Ok(serde_json::from_str(
        &crate::completion_plan::plan_v1_json(&context.to_string())?,
    )?)
}

fn commit(
    request: &Request,
    input: &Input,
    before: &Value,
    mut observation: Observation,
    reserve_break: bool,
) -> Result<String, CoreError> {
    let (workspace, allocation, command) =
        prepare_finish(request, input, before, &mut observation)?;
    let after = finish_projection(request, input, &workspace, &observation, &command)?;
    let source = source(&after["workspace"], &command)?;
    let (next_phase, queue_break, day) =
        completion_phase(request, &after["workspace"], source, &command)?;
    if queue_break != reserve_break {
        return Err(invalid("inconsistent generated break plans"));
    }
    let (selection, records) = selection_records(request, &next_phase, &command)?;
    let mut batch = FinishBatch {
        workspace,
        allocation,
        observation,
        after,
        commands: vec![command],
        selection,
        records,
        next_phase,
    };
    if reserve_break && request.supports_generated_break() {
        generated_break::append(request, input, day, &mut batch)?;
    }
    let mut result: Value = serde_json::from_str(&planned_result(request, input, batch)?)?;
    lifecycle::finish_opportunity(request, reserve_break, &mut result)?;
    Ok(result.to_string())
}

fn planned_result(
    request: &Request,
    input: &Input,
    batch: FinishBatch,
) -> Result<String, CoreError> {
    let FinishBatch {
        workspace,
        allocation,
        observation,
        after: _,
        commands,
        selection,
        records,
        next_phase,
    } = batch;
    let ownership_writes = completion_ownership_writes(request, &commands)?;
    let durable = commands
        .iter()
        .map(|item| durable_command(request.compatibility, item))
        .collect::<Vec<_>>();
    let now = projection::after_time(input, &commands);
    admission::commands(input, &workspace, &commands, &observation, &now)?;
    let final_projection = lifecycle::observe(input, &workspace, &observation, &now)?;
    let mut result = json!({"schemaVersion": 1, "outcome": "planned", "reason": "",
    "retryAtMs": null,
    "commands": commands, "durableCommands": durable,
    "commandOutcomes": admission::command_outcomes(&final_projection["workspace"], &commands),
    "atomicCommandIds": commands.iter().map(|item| &item["id"]).collect::<Vec<_>>(),
    "workspace": workspace, "projection": final_projection["workspace"], "selection": selection,
    "allocation": allocation, "observation": observation, "completionRecords": records,
    "ownershipWrites": ownership_writes, "effectsAfterCommit": [
        {"kind": "launchSync"}, {"kind": "cancelAlarm", "timerId": commands[0]["timerId"]}
    ]});
    if commands.len() == 2 {
        result["effectsAfterCommit"]
            .as_array_mut()
            .unwrap()
            .push(json!({
            "kind": "scheduleAlarm", "timerId": commands[1]["timerId"],
            "phase": next_phase, "durationMs": commands[1]["plannedDurationMs"]}));
    }
    if request.compatibility == Compatibility::PwaStorage {
        result.as_object_mut().unwrap().remove("retryAtMs");
    }
    Ok(result.to_string())
}

fn completion_ownership_writes(
    request: &Request,
    commands: &[Value],
) -> Result<Vec<Value>, CoreError> {
    if commands.len() == 1 {
        return Ok(if request.compatibility == Compatibility::PwaStorage {
            vec![json!({"kind": "removeTimerOwner"})]
        } else {
            vec![]
        });
    }
    let start = &commands[1];
    Ok(match request.compatibility {
        Compatibility::PwaStorage => vec![generated_break::owner_write(request, start)?],
        Compatibility::AppleWorkspace => {
            vec![json!({"kind": "recordStart", "timerId": start["timerId"],
            "deviceId": request.allocation.device_id, "startCommandId": start["id"]})]
        }
        Compatibility::AndroidCoordinator => {
            vec![json!({"kind": "setOwnedTimerId", "timerId": start["timerId"]})]
        }
        Compatibility::DesktopStorage | Compatibility::DesktopTerminal => {
            return Err(invalid("desktop generated finish requires deferred stage"));
        }
    })
}

fn prepare_finish(
    request: &Request,
    input: &Input,
    before: &Value,
    observation: &mut Observation,
) -> Result<(Value, Allocation, Value), CoreError> {
    let mut allocation = request.allocation.clone();
    allocation::advance(input, &mut allocation)?;
    let mut command = allocation::command(
        input,
        &allocation,
        before,
        observation,
        CommandKind::Finish,
        0,
    )?;
    if request.compatibility == Compatibility::PwaStorage {
        if let Some(parent) = super::inherited_dependency(&request.workspace, &command) {
            command["dependsOnCommandId"] = parent;
        }
    }
    if matches!(
        request.compatibility,
        Compatibility::DesktopStorage | Compatibility::DesktopTerminal
    ) {
        command["occurredAt"] = json!(
            crate::timer::parse_time(&input.clock.occurred_at)?
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        );
    }
    let mut workspace = request.workspace.clone();
    super::append(&mut workspace, &command)?;
    allocation.last_uuid = Some(request.identities.command_uuids[0].clone());
    observation.command_times.insert(
        command["id"].as_str().unwrap().into(),
        input.clock.physical_now.clone(),
    );
    Ok((workspace, allocation, command))
}

fn finish_projection(
    request: &Request,
    input: &Input,
    workspace: &Value,
    observation: &Observation,
    command: &Value,
) -> Result<Value, CoreError> {
    let now = projection::after_time(input, std::slice::from_ref(command));
    let admission_observation = if matches!(
        request.compatibility,
        Compatibility::DesktopStorage | Compatibility::DesktopTerminal
    ) {
        &Observation::default()
    } else {
        observation
    };
    let mut admitted = admission::commands(
        input,
        workspace,
        std::slice::from_ref(command),
        admission_observation,
        &now,
    )?;
    let display = lifecycle::observe(input, workspace, observation, &now)?;
    // Completion eligibility and preferences keep the entrypoint's safe view;
    // exact Finish provenance comes from the separately admitted command ledger.
    for key in ["autoStartBreaks", "durationsMs"] {
        admitted["workspace"][key] = display["workspace"][key].clone();
    }
    Ok(admitted)
}

fn source<'a>(after: &'a Value, command: &Value) -> Result<&'a Value, CoreError> {
    let rows = after["history"]
        .as_array()
        .ok_or_else(|| invalid("missing completion history"))?;
    rows.iter()
        .find(|row| {
            row["timerId"] == command["timerId"]
                && row["commandId"] == command["id"]
                && row["phase"] == command["phase"]
                && row["status"] == "completed"
                && row["completedAt"].as_str().is_some()
        })
        .ok_or_else(|| invalid("finish lacks exact completion provenance"))
}

fn completion_phase<'a>(
    request: &'a Request,
    after: &Value,
    source: &Value,
    command: &Value,
) -> Result<(String, bool, &'a Interval), CoreError> {
    let stamp = crate::timer::parse_time(source["completedAt"].as_str().unwrap())?;
    let day = request
        .calendar_intervals
        .iter()
        .find(|day| {
            let start = crate::timer::parse_time(&day.start).expect("calendar validated");
            let end = crate::timer::parse_time(&day.end).expect("calendar validated");
            start <= stamp && stamp < end
        })
        .ok_or_else(|| invalid("missing completion calendar interval"))?;
    let owner = completion_owner(request, command);
    let context = json!({"kind": "finishApplied", "source": {
        "commandId": command["id"], "timerId": command["timerId"],
        "phase": command["phase"], "occurredAt": source["completedAt"]},
        "history": after["history"], "autoStartBreaks": after["autoStartBreaks"],
        "localDeviceId": request.allocation.device_id, "ownership": owner,
        "dayStart": day.start, "dayEnd": day.end});
    let plan: Value =
        serde_json::from_str(&crate::completion_plan::plan_v1_json(&context.to_string())?)?;
    let phase = plan["selectedPhase"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("missing completion phase"))?;
    Ok((phase, plan["queueAutoBreak"] == true, day))
}

fn completion_owner(request: &Request, command: &Value) -> Value {
    let timer_id = command.get("timerId").unwrap_or(&command["id"]);
    if request.compatibility == Compatibility::PwaStorage {
        return json!({"timerId": timer_id, "ownerDeviceId": request.allocation.device_id});
    }
    let owner = request
        .ownership
        .as_ref()
        .filter(|owner| owner.timer_id == *timer_id)
        .map(|owner| owner.device_id.as_str())
        .or_else(|| {
            (request.compatibility == Compatibility::AppleWorkspace
                && !request.requested_timer.started_by_device_id.is_empty())
            .then_some(request.requested_timer.started_by_device_id.as_str())
        });
    owner.map_or(Value::Null, |device| {
        json!({
            "timerId": timer_id, "ownerDeviceId": device
        })
    })
}

fn calendar_bounds(intervals: &[Interval]) -> Result<(), CoreError> {
    let mut bounds = Vec::with_capacity(intervals.len());
    for day in intervals {
        let start = crate::timer::parse_time(&day.start)?;
        let end = crate::timer::parse_time(&day.end)?;
        if start >= end {
            return Err(invalid("invalid completion calendar interval"));
        }
        bounds.push((start, end));
    }
    bounds.sort_unstable();
    if bounds.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(invalid("overlapping completion calendar intervals"));
    }
    Ok(())
}

fn selection_records(
    request: &Request,
    phase: &str,
    command: &Value,
) -> Result<(Selection, Value), CoreError> {
    let mut selection = request.selection.clone();
    let previous = selection.phase.name();
    let mut record = Value::Null;
    if request.compatibility != Compatibility::AppleWorkspace || !selection.explicit {
        selection.phase = serde_json::from_value(json!(phase))?;
    }
    if request.compatibility == Compatibility::AppleWorkspace && !selection.explicit {
        let current = selection.generation.parse::<i64>().unwrap();
        selection.generation = current.checked_add(1).unwrap_or(0).to_string();
        if request.replication_mode == ReplicationMode::Centralized {
            record = json!({"kind": "phaseAdvance", "finishCommandId": command["id"],
                "timerId": command["timerId"], "previousPhase": previous,
                "advancedPhase": phase, "generation": selection.generation});
        }
    } else if request.compatibility == Compatibility::AndroidCoordinator && previous != phase {
        selection.generation = selection
            .generation
            .parse::<i64>()
            .unwrap()
            .wrapping_add(1)
            .to_string();
    } else if matches!(
        request.compatibility,
        Compatibility::DesktopStorage | Compatibility::DesktopTerminal
    ) {
        record = json!({"kind": "pendingPhaseAdvance", "finishCommandId": command["id"],
            "timerId": command["timerId"], "sourcePhase": command["phase"],
            "advancedPhase": phase, "selectedPhaseVersion": selection.generation});
    }
    Ok((
        selection,
        json!({"phaseAdvance": record, "pendingAutoBreak": null, "provisionalBreak": null}),
    ))
}

fn durable_command(profile: Compatibility, command: &Value) -> Value {
    let mut durable = command.clone();
    if profile != Compatibility::PwaStorage {
        durable.as_object_mut().unwrap().remove("deviceId");
    }
    durable
}

fn noop(request: &Request, reason: &str) -> Result<String, CoreError> {
    noop_with_retry(request, reason, None)
}

fn noop_with_retry(
    request: &Request,
    reason: &str,
    retry_at_ms: Option<i64>,
) -> Result<String, CoreError> {
    let timer = &request.workspace["base"]["canonicalTimer"];
    let replay_at = if matches!(timer["status"].as_str(), Some("running" | "paused"))
        && matches!(
            timer["lastIntent"]["type"].as_str(),
            Some("finish" | "cancel")
        ) {
        // Preserve concurrent-Finish no-op behavior without manufacturing a
        // completed history row from an inconsistent active presentation marker.
        "1970-01-01T00:00:00Z"
    } else {
        &request.clock.occurred_at
    };
    let mut result = json!({"schemaVersion": 1, "outcome": "noop", "reason": reason,
        "retryAtMs": retry_at_ms,
         "commands": [], "durableCommands": [], "atomicCommandIds": [],
         "commandOutcomes": [],
        "workspace": request.workspace, "selection": request.selection,
        "allocation": request.allocation, "observation": request.observation,
        "projection": lifecycle::project(&request.context(), &request.workspace, replay_at)?["workspace"],
        "completionRecords": {"phaseAdvance": null, "pendingAutoBreak": null, "provisionalBreak": null},
        "ownershipWrites": [], "effectsAfterCommit": [], "lifecycle": request.lifecycle});
    if request.compatibility == Compatibility::PwaStorage && retry_at_ms.is_none() {
        result.as_object_mut().unwrap().remove("retryAtMs");
    }
    Ok(result.to_string())
}
