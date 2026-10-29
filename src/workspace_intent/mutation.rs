use serde_json::{Value, json};

use super::model::{Allocation, CommandKind, Compatibility as C, Input, Intent};
use super::{CoreError, admission, allocation, append, invalid, monotonic, projection};

struct Plan<'a> {
    input: &'a mut Input,
    workspace: Value,
    allocation: Allocation,
    observation: super::model::Observation,
    operations: Value,
    index: usize,
    retired_duration_ids: Vec<String>,
}

fn intent_name(intent: &Intent) -> &'static str {
    match intent {
        Intent::UpsertTask { .. } => "upsertTask",
        Intent::AddAndSelectTask { .. } => "addAndSelectTask",
        Intent::DeleteTask { .. } => "deleteTask",
        Intent::RenameTask { .. } => "renameTask",
        Intent::SelectTask { .. } => "selectTask",
        Intent::SetDuration { .. } => "setDuration",
        Intent::ChangeDuration { .. } => "changeDuration",
        Intent::SetAutoStart { .. } => "setAutoStart",
        _ => "timerIntent",
    }
}

fn base_history_len(plan: &Plan<'_>) -> usize {
    plan.workspace["base"]["history"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0)
}

fn queued_total(plan: &Plan<'_>) -> usize {
    [
        "commands",
        "taskOperations",
        "durationOperations",
        "autoStartOperations",
        "selectedTaskOperations",
    ]
    .iter()
    .map(|name| plan.operations[name].as_array().map(Vec::len).unwrap_or(0))
    .sum()
}

pub(super) fn plan(input: &mut Input) -> Result<String, CoreError> {
    validate_boundaries(input)?;
    let (safe, observation) = monotonic::before(input)?;
    let before = projection::entrypoint(input, &safe, &observation, &input.clock.physical_now)?;
    let workspace = input.workspace.clone();
    let allocation = input.allocation.clone();
    let mut plan = Plan {
        input,
        workspace,
        allocation,
        observation,
        operations: empty_queues(),
        index: 0,
        retired_duration_ids: Vec::new(),
    };
    dispatch_mutation_intent(&mut plan, &before)?;
    drop(before);
    release_staged_base(&mut plan);
    output_with_prior_policy(plan, safe)
}

fn dispatch_mutation_intent(plan: &mut Plan<'_>, before: &Value) -> Result<(), CoreError> {
    match plan.input.intent.clone() {
        Intent::UpsertTask { title } => plan.upsert(&title, false, before)?,
        Intent::AddAndSelectTask { title } => plan.upsert(&title, true, before)?,
        Intent::DeleteTask { task_id } => plan.delete(&task_id, before)?,
        Intent::RenameTask { task_id, title } => plan.rename(&task_id, &title, before)?,
        Intent::SelectTask { task_id } => plan.select(task_id.0.as_deref(), before)?,
        Intent::SetDuration { phase, minutes } => {
            if plan.input.compatibility == C::AndroidCoordinator {
                return Err(invalid("absolute duration is not an Android entrypoint"));
            }
            let minutes = if plan.input.compatibility == C::AppleWorkspace {
                minutes.clamp(1, 180)
            } else {
                if !(1..=180).contains(&minutes) {
                    return Err(invalid("invalid duration range"));
                }
                minutes
            };
            plan.duration(phase.name(), minutes * 60_000, before)?;
        }
        Intent::ChangeDuration { phase, delta } => {
            if plan.input.compatibility != C::AndroidCoordinator {
                return Err(invalid("relative duration is Android-only"));
            }
            let current = plan.input.local_durations_ms.as_ref().unwrap()[phase.name()];
            plan.duration(
                phase.name(),
                ((current / 60_000) as i128 + delta as i128).clamp(1, 180) as i64 * 60_000,
                before,
            )?;
        }
        Intent::SetAutoStart { enabled } => plan.auto_start(enabled, before)?,
        _ => {
            let history = base_history_len(plan);
            let queued = queued_total(plan);
            return Err(CoreError::InvalidInput(format!(
                "unsupported workspace mutation intent op=workspace.intent.v1 intent={} history={history} queued={queued}",
                intent_name(&plan.input.intent),
            )));
        }
    }
    Ok(())
}

fn release_staged_base(plan: &mut Plan<'_>) {
    // Operations may re-project the live request workspace for selection
    // admission, so the staged base stays until they complete. The output
    // tail only consults retained queues, except a monotonic retarget check
    // that cannot run without a live anchor.
    if plan.observation.running_timer.is_none() {
        let staged: &mut Input = &mut *plan.input;
        drop(std::mem::replace(
            &mut staged.workspace["base"],
            serde_json::Value::Null,
        ));
    }
}

fn output_with_prior_policy(plan: Plan<'_>, safe: Value) -> Result<String, CoreError> {
    // The prior workspace is only read for unchanged display and for the
    // monotonic running-timer branch. Release it before the output tail
    // when neither can observe it.
    let keep_before = plan.index == 0
        || (plan.input.compatibility == C::PwaStorage
            && plan.input.clock.monotonic_now_ms.is_some()
            && safe["canonicalTimer"]["status"] == "running");
    if keep_before {
        plan.output(Some(&safe))
    } else {
        drop(safe);
        plan.output(None)
    }
}

fn validate_boundaries(input: &Input) -> Result<(), CoreError> {
    if input.compatibility == C::DesktopTerminal {
        return Err(invalid(
            "workspace task and preference entrypoints use desktopStorage",
        ));
    }
    let owner = input
        .ownership
        .as_ref()
        .ok_or_else(|| invalid("missing account ownership"))?;
    if owner.owner_id.0 != owner.expected_owner_id.0
        || owner.owner_id.0.as_deref().is_some_and(str::is_empty)
    {
        return Err(invalid("stale account ownership"));
    }
    let durability = input
        .durability
        .as_ref()
        .ok_or_else(|| invalid("missing outgoing durability"))?;
    if matches!(input.intent, Intent::SetDuration { .. })
        && input.compatibility == C::AppleWorkspace
        || matches!(input.intent, Intent::ChangeDuration { .. })
    {
        let local = input
            .local_durations_ms
            .as_ref()
            .ok_or_else(|| invalid("missing local duration settings"))?;
        if !crate::sync_projection::is_valid_duration_map(local) {
            return Err(invalid("invalid local duration settings"));
        }
    } else if input.local_durations_ms.is_some() {
        return Err(invalid("unexpected local duration settings"));
    }
    validate_durability(input, durability)
}

fn validate_durability(
    input: &Input,
    durability: &super::model::Durability,
) -> Result<(), CoreError> {
    let queued = input.workspace["local"]["durationOperations"]
        .as_array()
        .unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for id in &durability.outgoing_duration_operation_ids {
        if !seen.insert(id) || !queued.iter().any(|op| op["id"] == *id) {
            return Err(invalid("outgoing duration identity is not retained"));
        }
        if input.workspace["neverSent"]["durationOperations"]
            .as_array()
            .is_some_and(|proof| proof.iter().any(|item| item == id))
        {
            return Err(invalid(
                "possibly-sent duration cannot be superseded with never-sent proof",
            ));
        }
    }
    Ok(())
}

fn empty_queues() -> Value {
    json!({"commands": [], "taskOperations": [], "durationOperations": [],
        "autoStartOperations": [], "selectedTaskOperations": []})
}

impl Plan<'_> {
    fn reserve(&mut self, command: bool) -> Result<String, CoreError> {
        if command {
            allocation::advance(self.input, &mut self.allocation)?;
        } else {
            allocation::tick(self.input, &mut self.allocation)?;
        }
        let uuid = allocation::reserved_uuid(self.input, &mut self.allocation, self.index)?;
        self.index += 1;
        Ok(uuid)
    }

    fn operation(&mut self, domain: &str, mut operation: Value) -> Result<(), CoreError> {
        let uuid = self.reserve(false)?;
        operation["id"] = json!(match (self.input.compatibility, domain) {
            (C::AppleWorkspace | C::AndroidCoordinator, "taskOperations") =>
                format!("task-operation-{uuid}"),
            (C::AppleWorkspace | C::AndroidCoordinator, "durationOperations") =>
                format!("duration-operation-{uuid}"),
            _ => uuid,
        });
        operation["occurredAt"] = json!(allocation::occurrence(self.input, &self.allocation)?);
        operation["hlcWallMs"] = json!(self.allocation.hlc.wall_ms);
        operation["hlcCounter"] = json!(self.allocation.hlc.counter);
        operation["deviceId"] = json!(self.allocation.device_id);
        if domain == "durationOperations" && self.input.compatibility == C::PwaStorage {
            let tab = self
                .input
                .durability
                .as_ref()
                .unwrap()
                .local_tab_id
                .as_deref()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| invalid("missing PWA tab identity"))?;
            operation["ownerId"] = json!(tab);
        }
        self.workspace["local"][domain]
            .as_array_mut()
            .unwrap()
            .push(operation.clone());
        if self.workspace["neverSent"].get(domain).is_none() {
            self.workspace["neverSent"][domain] = json!([]);
        }
        self.workspace["neverSent"][domain]
            .as_array_mut()
            .unwrap()
            .push(operation["id"].clone());
        self.operations[domain]
            .as_array_mut()
            .unwrap()
            .push(operation);
        Ok(())
    }

    fn command(
        &mut self,
        before: &Value,
        kind: CommandKind,
        task_id: Option<&str>,
    ) -> Result<(), CoreError> {
        self.reserve(true)?;
        let mut command = allocation::command(
            self.input,
            &self.allocation,
            before,
            &self.observation,
            kind,
            self.index - 1,
        )?;
        if matches!(kind, CommandKind::Retarget) {
            command["taskId"] = json!(task_id);
        }
        append(&mut self.workspace, &command)?;
        self.observation.command_times.insert(
            command["id"].as_str().unwrap().into(),
            self.input.clock.physical_now.clone(),
        );
        self.operations["commands"]
            .as_array_mut()
            .unwrap()
            .push(command);
        Ok(())
    }

    fn select(&mut self, task_id: Option<&str>, before: &Value) -> Result<(), CoreError> {
        admission::validate_selection_task(self.input, before, task_id)?;
        let current = before["selectedTaskId"].as_str();
        let timer = &before["canonicalTimer"];
        let focus = timer["phase"] == "focus"
            && matches!(timer["status"].as_str(), Some("running" | "paused"));
        let repair = self.input.compatibility == C::AndroidCoordinator
            && focus
            && timer["taskId"].as_str() != task_id;
        let repeat = self.input.compatibility == C::DesktopStorage;
        if current == task_id && !repair && !repeat {
            return Ok(());
        }
        if current != task_id || repeat {
            self.operation("selectedTaskOperations", json!({"taskId": task_id}))?;
        }
        if focus && (current != task_id || repair || repeat) {
            self.command(before, CommandKind::Retarget, task_id)?;
        }
        Ok(())
    }

    fn upsert(&mut self, title: &str, select: bool, before: &Value) -> Result<(), CoreError> {
        let (id, title) = crate::task::identity(title)?;
        let exists = before["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == id);
        let active = matches!(
            before["canonicalTimer"]["status"].as_str(),
            Some("running" | "paused")
        );
        if select && self.input.compatibility == C::AppleWorkspace {
            return Err(invalid("Apple task upsert has no selection entrypoint"));
        }
        if select && active && !exists && self.input.compatibility == C::AndroidCoordinator {
            return Ok(());
        }
        if !exists || !select {
            self.operation(
                "taskOperations",
                json!({"taskId": id, "type": "upsert", "title": title}),
            )?;
        }
        if !select {
            return Ok(());
        }
        if exists {
            return self.select(Some(&id), before);
        }
        self.operation("selectedTaskOperations", json!({"taskId": id}))?;
        let timer = &before["canonicalTimer"];
        if timer["phase"] == "focus"
            && matches!(timer["status"].as_str(), Some("running" | "paused"))
        {
            self.command(before, CommandKind::Retarget, Some(&id))?;
        }
        Ok(())
    }

    fn delete(&mut self, task_id: &str, before: &Value) -> Result<(), CoreError> {
        if task_id.is_empty() {
            return Err(invalid("invalid task identity"));
        }
        if !before["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == task_id)
            && !super::known_tasks::contains(self.input, task_id)
        {
            if self.input.known_tasks.is_some() {
                return Ok(());
            }
            return Err(invalid("task delete requires an active task"));
        }
        self.operation(
            "taskOperations",
            json!({"taskId": task_id, "type": "delete"}),
        )?;
        if before["selectedTaskId"] == task_id && self.input.compatibility != C::PwaStorage {
            self.operation("selectedTaskOperations", json!({"taskId": null}))?;
            if self.input.compatibility == C::DesktopStorage {
                let timer = &before["canonicalTimer"];
                if timer["phase"] == "focus"
                    && matches!(timer["status"].as_str(), Some("running" | "paused"))
                {
                    self.command(before, CommandKind::Retarget, None)?;
                }
            }
        }
        Ok(())
    }

    fn rename(&mut self, task_id: &str, title: &str, before: &Value) -> Result<(), CoreError> {
        if task_id.is_empty() {
            return Err(invalid("invalid task identity"));
        }
        let tasks = before["tasks"].as_array().unwrap();
        if !tasks.iter().any(|task| task["id"] == task_id) {
            return Err(invalid("task rename requires an active task"));
        }
        let (id, title) = crate::task::identity(title)?;
        if id == task_id {
            return Ok(());
        }
        // Task identity hashes the title, so a rename replaces the record.
        // The new identity lands before the old one leaves, and the current
        // selection plus any active focus assignment follow it in the same
        // plan. A title that already exists merges into that task.
        if !tasks.iter().any(|task| task["id"] == id) {
            self.operation(
                "taskOperations",
                json!({"taskId": id, "type": "upsert", "title": title}),
            )?;
        }
        self.operation(
            "taskOperations",
            json!({"taskId": task_id, "type": "delete"}),
        )?;
        if before["selectedTaskId"] == task_id {
            self.operation("selectedTaskOperations", json!({"taskId": id}))?;
        }
        let timer = &before["canonicalTimer"];
        if timer["phase"] == "focus"
            && matches!(timer["status"].as_str(), Some("running" | "paused"))
            && timer["taskId"] == task_id
        {
            self.command(before, CommandKind::Retarget, Some(&id))?;
        }
        Ok(())
    }

    fn duration(&mut self, phase: &str, duration_ms: i64, before: &Value) -> Result<(), CoreError> {
        if self.input.compatibility == C::AndroidCoordinator
            && matches!(
                before["canonicalTimer"]["status"].as_str(),
                Some("running" | "paused")
            )
        {
            return Ok(());
        }
        let current = if matches!(
            self.input.compatibility,
            C::AppleWorkspace | C::AndroidCoordinator
        ) {
            self.input.local_durations_ms.as_ref().unwrap()[phase]
        } else {
            before["durationsMs"][phase].as_i64().unwrap()
        };
        if current == duration_ms && self.input.compatibility != C::DesktopStorage {
            return Ok(());
        }
        let retired = self.superseded_durations(phase)?;
        if self.input.compatibility == C::AppleWorkspace
            && matches!(
                before["canonicalTimer"]["status"].as_str(),
                Some("completed" | "cancelled" | "superseded")
            )
        {
            self.command(before, CommandKind::Clear, None)?;
        }
        for id in retired {
            self.retire_duration(&id);
            self.retired_duration_ids.push(id);
        }
        self.operation(
            "durationOperations",
            json!({"phase": phase, "durationMs": duration_ms}),
        )
    }

    fn superseded_durations(&self, phase: &str) -> Result<Vec<String>, CoreError> {
        let mut retired = Vec::new();
        let outgoing = &self
            .input
            .durability
            .as_ref()
            .unwrap()
            .outgoing_duration_operation_ids;
        for op in self.workspace["local"]["durationOperations"]
            .as_array()
            .unwrap()
        {
            if op["phase"] != phase {
                continue;
            }
            let id = op["id"].as_str().unwrap();
            let never = self.workspace["neverSent"]["durationOperations"]
                .as_array()
                .is_some_and(|proof| proof.iter().any(|item| item == id));
            let same_tab = self.input.compatibility != C::PwaStorage
                || op["ownerId"].as_str()
                    == self
                        .input
                        .durability
                        .as_ref()
                        .unwrap()
                        .local_tab_id
                        .as_deref();
            if !never || !same_tab || outgoing.iter().any(|sent| sent == id) {
                // Keep immutable retry payloads and other tabs' operations.
                // Only proven local, unclaimed work may be coalesced away.
                continue;
            }
            retired.push(id.to_owned());
        }
        Ok(retired)
    }

    fn retire_duration(&mut self, id: &str) {
        self.workspace["local"]["durationOperations"]
            .as_array_mut()
            .unwrap()
            .retain(|operation| operation["id"] != id);
        self.workspace["neverSent"]["durationOperations"]
            .as_array_mut()
            .unwrap()
            .retain(|proof| proof != id);
        crate::reconciliation::workspace::display::trim_workspace(&mut self.workspace)
            .expect("display context validated before mutation");
    }

    fn auto_start(&mut self, enabled: bool, before: &Value) -> Result<(), CoreError> {
        let latest = if self.input.compatibility == C::AppleWorkspace {
            self.workspace["local"]["autoStartOperations"]
                .as_array()
                .unwrap()
                .last()
                .map(|op| &op["enabled"])
                .unwrap_or(&before["autoStartBreaks"])
        } else {
            &before["autoStartBreaks"]
        };
        if *latest == enabled && self.input.compatibility != C::DesktopStorage {
            return Ok(());
        }
        self.operation("autoStartOperations", json!({"enabled": enabled}))
    }

    fn output(mut self, before: Option<&Value>) -> Result<String, CoreError> {
        let changed = self.index != 0;
        let after = self.display_after(before)?;
        let admission_now = if self.input.compatibility == C::PwaStorage
            && self.input.clock.monotonic_now_ms.is_some()
            && before.is_some_and(|before| before["canonicalTimer"]["status"] == "running")
        {
            self.display_now(before, self.operations["commands"].as_array().unwrap())?
        } else {
            self.input.clock.physical_now.clone()
        };
        admission::group(
            self.input,
            &self.workspace,
            &self.operations,
            &self.observation,
            &admission_now,
        )?;
        let outcomes = self.group_outcomes(&after);
        monotonic::after(self.input, &after, &mut self.observation)?;
        self.assemble_mutation_envelope(after, outcomes, changed)
    }

    fn assemble_mutation_envelope(
        mut self,
        after: Value,
        outcomes: Value,
        changed: bool,
    ) -> Result<String, CoreError> {
        let ids = self.operation_ids();
        let durable = self.durable_operations();
        let effects = self.effects_after_commit();
        let commands = self.operations["commands"].clone();
        let timer_observation =
            monotonic::timer_observation(self.input, &after, &self.observation)?;
        let atomic_commands = ids["commands"].clone();
        // Map inserts move large subtrees; the json! macro would clone them.
        let mut output = serde_json::Map::with_capacity(16);
        output.insert("schemaVersion".into(), json!(1));
        output.insert(
            "outcome".into(),
            json!(if changed { "planned" } else { "noop" }),
        );
        output.insert(
            "reason".into(),
            json!(if changed { "" } else { "unchangedOrIneligible" }),
        );
        output.insert(
            "workspace".into(),
            std::mem::replace(&mut self.workspace, json!(null)),
        );
        output.insert("selection".into(), json!(self.input.selection));
        output.insert("allocation".into(), json!(self.allocation));
        output.insert("observation".into(), json!(self.observation));
        output.insert("commands".into(), commands);
        output.insert("atomicCommandIds".into(), atomic_commands);
        output.insert(
            "operations".into(),
            std::mem::replace(&mut self.operations, json!(null)),
        );
        output.insert("durableOperations".into(), durable);
        output.insert("atomicOperationIds".into(), ids);
        output.insert(
            "retiredDurationOperationIds".into(),
            json!(self.retired_duration_ids),
        );
        output.insert("groupOutcomes".into(), outcomes);
        output.insert("ownershipWrites".into(), json!([]));
        output.insert("projection".into(), after);
        output.insert("timerObservation".into(), timer_observation);
        output.insert("effectsAfterCommit".into(), json!(effects));
        Ok(serde_json::Value::Object(output).to_string())
    }

    fn display_after(&self, before: Option<&Value>) -> Result<Value, CoreError> {
        if self.index == 0 {
            let Some(before) = before else {
                let bytes = self.operations.to_string().len();
                return Err(CoreError::InvalidInput(format!(
                    "unchanged workspace display requires the prior workspace \
                     op=workspace.intent.v1 index=0 history={} queued={} bytes={bytes}",
                    base_history_len(self),
                    queued_total(self),
                )));
            };
            return Ok(before.clone());
        }
        let commands = self.operations["commands"].as_array().unwrap();
        let now = self.display_now(before, commands)?;
        let mut observed = projection::observed(
            &self.workspace,
            &self.observation,
            self.input.compatibility,
            projection::ReplayDomains::Safe,
            &now,
        )?;
        crate::take_projection_workspace(
            &mut observed,
            "workspace.intent.v1",
            base_history_len(self),
            queued_total(self),
        )
    }

    fn display_now(&self, before: Option<&Value>, commands: &[Value]) -> Result<String, CoreError> {
        if self.input.compatibility == C::PwaStorage
            && self.input.clock.monotonic_now_ms.is_some()
            && before.is_some_and(|before| before["canonicalTimer"]["status"] == "running")
        {
            let timer = before
                .and_then(|before| before.get("canonicalTimer"))
                .filter(|timer| timer["status"] == "running")
                .ok_or_else(|| {
                    CoreError::InvalidInput(format!(
                        "missing running canonical timer op=workspace.intent.v1 \
                         history={} queued={}",
                        base_history_len(self),
                        queued_total(self),
                    ))
                })?;
            return monotonic::entrypoint_time(self.input, timer, &self.observation);
        }
        if !commands.is_empty() {
            return Ok(projection::after_time(self.input, commands));
        }
        if self.input.compatibility == C::PwaStorage {
            return Ok(self.input.clock.occurred_at.clone());
        }
        Ok(self.input.clock.physical_now.clone())
    }

    fn effects_after_commit(&self) -> Vec<Value> {
        if self.index == 0 {
            return Vec::new();
        }
        let mut effects = vec![json!({"kind": "launchSync"})];
        if matches!(self.input.intent, Intent::SetDuration { .. })
            && self.input.compatibility == C::AppleWorkspace
        {
            for command in self.operations["commands"].as_array().unwrap() {
                if command["type"] == "clear" {
                    effects.push(json!({"kind": "cancelAlarm", "timerId": command["timerId"]}));
                }
            }
        }
        effects
    }

    fn operation_ids(&self) -> Value {
        let mut ids = empty_queues();
        for name in [
            "commands",
            "taskOperations",
            "durationOperations",
            "autoStartOperations",
            "selectedTaskOperations",
        ] {
            ids[name] = json!(
                self.operations[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|op| &op["id"])
                    .collect::<Vec<_>>()
            );
        }
        ids
    }

    fn durable_operations(&self) -> Value {
        let mut durable = self.operations.clone();
        let no_device = match self.input.compatibility {
            C::PwaStorage => return durable,
            C::AndroidCoordinator => vec![
                "commands",
                "taskOperations",
                "durationOperations",
                "selectedTaskOperations",
            ],
            C::AppleWorkspace | C::DesktopStorage | C::DesktopTerminal => {
                vec!["commands", "taskOperations", "durationOperations"]
            }
        };
        for domain in no_device {
            for operation in durable[domain].as_array_mut().unwrap() {
                operation.as_object_mut().unwrap().remove("deviceId");
            }
        }
        durable
    }

    fn group_outcomes(&self, after: &Value) -> Value {
        let mut outcomes = empty_queues();
        for name in [
            "commands",
            "taskOperations",
            "durationOperations",
            "autoStartOperations",
            "selectedTaskOperations",
        ] {
            for op in self.operations[name].as_array().unwrap() {
                outcomes[name]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(admission::outcome(after, name, op)));
            }
        }
        outcomes
    }
}
