use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::{CoreError, Input, Plan, Profile, evidence, invalid};

struct Context<'a> {
    input: &'a Input,
    commands: &'a BTreeMap<&'a str, &'a Value>,
    edges: &'a [Value],
    projection: &'a Value,
}

pub(super) fn plan(
    input: &Input,
    commands: &BTreeMap<&str, &Value>,
    projection: &Value,
) -> Result<Plan, CoreError> {
    let edges = edges(input, commands)?;
    validate_graph(&edges, commands)?;
    validate_metadata(input, &edges, commands)?;
    let mut plan = Plan {
        dependencies: vec![],
        classifications: vec![],
        unresolved: vec![],
    };
    let context = Context {
        input,
        commands,
        edges: &edges,
        projection,
    };
    for original in &edges {
        let mut edge = original.clone();
        let child = commands[edge["operationId"].as_str().unwrap()];
        if rejected_dependency(input, child, original, &mut plan)? {
            continue;
        }
        if let Some(reason) = upgrade(input, commands, &edges, child, &mut edge)? {
            unresolved(&mut plan, child, original, reason, Value::Null);
            continue;
        }
        let parent_id = edge["dependsOnOperationId"].as_str().unwrap();
        if let Some(parent) = commands.get(parent_id) {
            if generated(input, child, parent, &edge) {
                pending_generated(&context, child, parent, &mut edge, original, &mut plan)?;
            } else {
                if edge["generatedBreak"] == true {
                    return Err(invalid("invalid legacy generated break source"));
                }
                plan.dependencies.push(edge.clone());
                let kind = "retainedDirectDependency";
                classify(&mut plan, child, original, &edge, kind);
            }
        } else {
            missing(input, child, &edge, original, &mut plan)?;
        }
    }
    block_rejected_acknowledgements(input, &mut plan)?;
    Ok(plan)
}

fn rejected_dependency(
    input: &Input,
    child: &Value,
    edge: &Value,
    plan: &mut Plan,
) -> Result<bool, CoreError> {
    let id = edge["dependsOnOperationId"].as_str().unwrap();
    if !input
        .source_acknowledgements
        .iter()
        .any(|ack| ack.command_id == id && ack.outcome == "rejected")
    {
        return Ok(false);
    }
    unresolved(
        plan,
        child,
        edge,
        "sourceRejectedRequiresDecision",
        json!({
        "canonicalCompletion": evidence::canonical_completion(input, id),
        "savedCommand": evidence::saved_command(input, id)? }),
    );
    Ok(true)
}

fn block_rejected_acknowledgements(input: &Input, plan: &mut Plan) -> Result<(), CoreError> {
    for ack in input
        .source_acknowledgements
        .iter()
        .filter(|ack| ack.outcome == "rejected")
    {
        if plan.unresolved.iter().any(|row| {
            row["dependsOnOperationId"] == ack.command_id
                && row["reason"] == "sourceRejectedRequiresDecision"
        }) {
            continue;
        }
        let saved = evidence::saved_command(input, &ack.command_id)?;
        let local = input.workspace["local"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == ack.command_id);
        plan.unresolved.push(json!({"operationId": ack.command_id, "reason": "sourceRejectedRequiresDecision",
            "command": local.or(saved.as_ref()), "dependencyRecord": null,
            "sourceEvidence": {"canonicalCompletion": evidence::canonical_completion(input, &ack.command_id),
                "savedCommand": saved}}));
    }
    Ok(())
}

fn edges(input: &Input, commands: &BTreeMap<&str, &Value>) -> Result<Vec<Value>, CoreError> {
    if let Some(edges) = input.workspace["timerDependencies"].as_array() {
        return Ok(edges.clone());
    }
    let field = if input.profile == Profile::PwaStorage {
        "dependsOnCommandId"
    } else {
        "generatedByFinishCommandId"
    };
    let mut result = vec![];
    for command in input.workspace["local"]["commands"].as_array().unwrap() {
        let Some(parent) = command[field].as_str().filter(|id| !id.is_empty()) else {
            if command["generatedBreak"] == true {
                return Err(invalid("generated break has no source identity"));
            }
            continue;
        };
        let mut edge = json!({"operationId": command["id"], "dependsOnOperationId": parent});
        let saved =
            if input.profile == Profile::AndroidCentralized && !commands.contains_key(parent) {
                evidence::saved_command(input, parent)?
            } else {
                None
            };
        if command["generatedBreak"] == true
            || (input.profile == Profile::AndroidCentralized
                && commands
                    .get(parent)
                    .copied()
                    .or(saved.as_ref())
                    .is_some_and(|source| generated(input, command, source, &edge)))
        {
            edge["generatedBreak"] = json!(true);
        }
        for field in ["sourceDayStart", "sourceDayEnd"] {
            if let Some(value) = command.get(field) {
                edge[field] = value.clone();
            }
        }
        result.push(edge);
    }
    Ok(result)
}

fn validate_graph(edges: &[Value], commands: &BTreeMap<&str, &Value>) -> Result<(), CoreError> {
    let mut parents = BTreeMap::new();
    for edge in edges {
        let child = edge["operationId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| invalid("invalid legacy dependency child"))?;
        let parent = edge["dependsOnOperationId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| invalid("invalid legacy dependency parent"))?;
        if child == parent
            || !commands.contains_key(child)
            || parents.insert(child, parent).is_some()
        {
            return Err(invalid("invalid legacy dependency graph"));
        }
        if edge["generatedBreak"] != true
            && (edge.get("sourceDayStart").is_some() || edge.get("sourceDayEnd").is_some())
        {
            return Err(invalid("source day requires generated break"));
        }
    }
    for child in parents.keys() {
        let mut seen = BTreeSet::new();
        let mut current = child;
        while let Some(parent) = parents.get(current) {
            if !seen.insert(current) {
                return Err(invalid("cyclic legacy dependency graph"));
            }
            current = parent;
        }
    }
    Ok(())
}

fn generated(input: &Input, child: &Value, parent: &Value, edge: &Value) -> bool {
    child["type"] == "start"
        && matches!(child["phase"].as_str(), Some("short_break" | "long_break"))
        && parent["type"] == "finish"
        && parent["phase"] == "focus"
        && (edge["generatedBreak"] == true || input.profile == Profile::AndroidCentralized)
}

fn validate_metadata(
    input: &Input,
    edges: &[Value],
    commands: &BTreeMap<&str, &Value>,
) -> Result<(), CoreError> {
    let key = |row: &Value| {
        (
            row["hlcWallMs"].as_i64().unwrap(),
            row["hlcCounter"].as_i64().unwrap(),
            row["deviceId"]
                .as_str()
                .unwrap_or(&input.device_id)
                .to_owned(),
            row["id"].as_str().unwrap().to_owned(),
        )
    };
    for edge in edges {
        if let Some(parent) = commands.get(edge["dependsOnOperationId"].as_str().unwrap()) {
            if key(parent) >= key(commands[edge["operationId"].as_str().unwrap()]) {
                return Err(invalid(
                    "immutable legacy dependency is not causally ordered",
                ));
            }
        }
        match edge.get("sourceDayStart").zip(edge.get("sourceDayEnd")) {
            Some((start, end)) => {
                let start = crate::timer::parse_time(start.as_str().unwrap())?;
                let end = crate::timer::parse_time(end.as_str().unwrap())?;
                if !(1..=26 * 60 * 60 * 1000).contains(&(end - start).num_milliseconds()) {
                    return Err(invalid("invalid legacy source day range"));
                }
            }
            None if edge.get("sourceDayStart").is_some() || edge.get("sourceDayEnd").is_some() => {
                return Err(invalid("incomplete legacy source day"));
            }
            None => {}
        }
    }
    Ok(())
}

fn upgrade(
    input: &Input,
    commands: &BTreeMap<&str, &Value>,
    edges: &[Value],
    child: &Value,
    edge: &mut Value,
) -> Result<Option<&'static str>, CoreError> {
    if child["type"] == "start" {
        return Ok(None);
    }
    let parent = &edge["dependsOnOperationId"];
    let starts: Vec<_> = edges
        .iter()
        .filter_map(|candidate| {
            let start = commands[candidate["operationId"].as_str().unwrap()];
            (start["type"] == "start"
                && (candidate["generatedBreak"] == true
                    || commands
                        .get(parent.as_str().unwrap())
                        .is_some_and(|source| generated(input, start, source, candidate)))
                && candidate["dependsOnOperationId"] == *parent
                && start["timerId"] == child["timerId"]
                && start["deviceId"] == child["deviceId"])
                .then_some(start)
        })
        .collect();
    if starts.is_empty() {
        return Ok(None);
    }
    if starts.len() != 1 || starts[0]["deviceSequence"].as_i64() >= child["deviceSequence"].as_i64()
    {
        return Err(invalid("ambiguous legacy sibling dependency"));
    }
    if !evidence::never_sent(input, "commands", child["id"].as_str().unwrap()) {
        return Ok(Some("possiblyDeliveredSibling"));
    }
    let start = starts[0];
    let previous = commands
        .values()
        .filter(|row| {
            row["timerId"] == child["timerId"]
                && row["deviceId"] == child["deviceId"]
                && row["deviceSequence"].as_i64() < child["deviceSequence"].as_i64()
                && row["deviceSequence"].as_i64() >= start["deviceSequence"].as_i64()
        })
        .max_by_key(|row| row["deviceSequence"].as_i64())
        .unwrap();
    edge["dependsOnOperationId"] = previous["id"].clone();
    Ok(None)
}

fn pending_generated(
    context: &Context<'_>,
    child: &Value,
    parent: &Value,
    edge: &mut Value,
    original: &Value,
    plan: &mut Plan,
) -> Result<(), CoreError> {
    let (input, projection) = (context.input, context.projection);
    let Some(source) = evidence::completion(projection, parent["id"].as_str().unwrap()) else {
        unresolved(
            plan,
            child,
            original,
            "missingExactCompletion",
            parent.clone(),
        );
        return Ok(());
    };
    validate_source(parent, child, source)?;
    let Some(bounds) = evidence::bounds(input, edge, source)? else {
        unresolved(
            plan,
            child,
            original,
            "missingCalendarEvidence",
            source.clone(),
        );
        return Ok(());
    };
    edge["generatedBreak"] = json!(true);
    edge["sourceDayStart"] = bounds["sourceDayStart"].clone();
    edge["sourceDayEnd"] = bounds["sourceDayEnd"].clone();
    let phase = evidence::expected_phase(projection, source, &bounds)?;
    if frozen_payload_requires_decision(context, parent, child, &phase) {
        unresolved(
            plan,
            child,
            original,
            "possiblyDeliveredPayloadDecisionRequired",
            source.clone(),
        );
        return Ok(());
    }
    record_generated(plan, child, original, edge, source, phase);
    Ok(())
}

fn frozen_payload_requires_decision(
    context: &Context<'_>,
    source: &Value,
    start: &Value,
    phase: &str,
) -> bool {
    let batch = generated_batch(context, source, start);
    // A Finish preserves the Start's target payload, not every batch member.
    // reconcile::GeneratedBreakContext::normalize still visits all members.
    let (phase, duration) = if batch.iter().any(|row| row["type"] == "finish") {
        (
            start["phase"].as_str().unwrap(),
            start["plannedDurationMs"].as_i64().unwrap(),
        )
    } else {
        (
            phase,
            context.input.workspace["base"]["durationsMs"][phase]
                .as_i64()
                .unwrap(),
        )
    };
    batch.iter().any(|row| {
        !evidence::never_sent(context.input, "commands", row["id"].as_str().unwrap())
            && (row["phase"] != phase
                || row["plannedDurationMs"] != duration
                || row["observedElapsedMs"]
                    .as_i64()
                    .unwrap()
                    .clamp(0, duration)
                    != row["observedElapsedMs"])
    })
}

fn generated_batch<'a>(context: &Context<'a>, source: &Value, start: &Value) -> Vec<&'a Value> {
    let parents: BTreeMap<_, _> = context
        .edges
        .iter()
        .map(|edge| {
            (
                edge["operationId"].as_str().unwrap(),
                edge["dependsOnOperationId"].as_str().unwrap(),
            )
        })
        .collect();
    context
        .commands
        .values()
        .filter(|row| row["timerId"] == start["timerId"])
        .filter(|row| {
            let mut current = row["id"].as_str().unwrap();
            while let Some(parent) = parents.get(current) {
                if *parent == source["id"].as_str().unwrap() {
                    return true;
                }
                current = parent;
            }
            false
        })
        .copied()
        .collect()
}

fn record_generated(
    plan: &mut Plan,
    child: &Value,
    original: &Value,
    edge: &Value,
    source: &Value,
    phase: String,
) {
    plan.dependencies.push(edge.clone());
    classify(plan, child, original, edge, "pendingGeneratedBreak");
    let record = plan.classifications.last_mut().unwrap();
    record["sourceCompletedAt"] = source["completedAt"].clone();
    record["sourcePhaseAfter"] = json!(phase);
}

fn validate_source(parent: &Value, child: &Value, source: &Value) -> Result<(), CoreError> {
    if source["timerId"] != parent["timerId"]
        || source["plannedDurationMs"] != parent["plannedDurationMs"]
    {
        return Err(invalid("conflicting legacy completion source"));
    }
    if evidence::completion_time(source)? > evidence::time(child)? {
        return Err(invalid("legacy source completion is after dependent"));
    }
    Ok(())
}

fn missing(
    input: &Input,
    child: &Value,
    edge: &Value,
    original: &Value,
    plan: &mut Plan,
) -> Result<(), CoreError> {
    let id = edge["dependsOnOperationId"].as_str().unwrap();
    let source = evidence::canonical_completion(input, id);
    let saved = evidence::saved_command(input, id)?;
    let ack = input
        .source_acknowledgements
        .iter()
        .find(|ack| ack.command_id == id);
    let reason = match (source.as_ref(), saved.as_ref(), ack) {
        (_, _, Some(ack)) if ack.outcome == "rejected" => "sourceRejectedRequiresDecision",
        (None, _, _) => "missingSourceProvenance",
        (_, None, _) => "missingSavedSourceCommand",
        (_, _, None) => "sourceAcknowledgementRequired",
        (Some(source), Some(saved), Some(_)) => {
            super::provenance::saved_finish(saved, source)?;
            saved_causality(input, saved, child)?;
            if child["type"] != "start"
                || !matches!(child["phase"].as_str(), Some("short_break" | "long_break"))
            {
                "missingDirectParentRequiresDecision"
            } else {
                return discharge(input, child, edge, original, source, plan);
            }
        }
    };
    unresolved(
        plan,
        child,
        original,
        reason,
        json!({"canonicalCompletion": source, "savedCommand": saved}),
    );
    Ok(())
}

fn saved_causality(input: &Input, saved: &Value, child: &Value) -> Result<(), CoreError> {
    let device = |row: &Value| {
        row["deviceId"]
            .as_str()
            .unwrap_or(&input.device_id)
            .to_owned()
    };
    let key = |row: &Value| {
        (
            row["hlcWallMs"].as_i64().unwrap(),
            row["hlcCounter"].as_i64().unwrap(),
            device(row),
            row["id"].as_str().unwrap().to_owned(),
        )
    };
    if key(saved) >= key(child)
        || (device(saved) == device(child)
            && saved["deviceSequence"].as_i64() >= child["deviceSequence"].as_i64())
    {
        return Err(invalid(
            "saved legacy source is not causally before dependent",
        ));
    }
    Ok(())
}

fn discharge(
    input: &Input,
    child: &Value,
    edge: &Value,
    original: &Value,
    source: &Value,
    plan: &mut Plan,
) -> Result<(), CoreError> {
    if evidence::completion_time(source)? > evidence::time(child)? {
        return Err(invalid("legacy source completion is after dependent"));
    }
    let Some(bounds) = evidence::bounds(input, edge, source)? else {
        unresolved(
            plan,
            child,
            original,
            "missingCalendarEvidence",
            source.clone(),
        );
        return Ok(());
    };
    let phase = evidence::expected_phase(&input.workspace["base"], source, &bounds)?;
    if child["phase"] != phase
        || child["plannedDurationMs"] != input.workspace["base"]["durationsMs"][&phase]
    {
        unresolved(
            plan,
            child,
            original,
            "canonicalPayloadDecisionRequired",
            source.clone(),
        );
        return Ok(());
    }
    classify(
        plan,
        child,
        original,
        &Value::Null,
        "acknowledgedCanonicalSource",
    );
    Ok(())
}

fn classify(plan: &mut Plan, child: &Value, original: &Value, edge: &Value, kind: &str) {
    plan.classifications
        .push(json!({"operationId": child["id"], "kind": kind,
        "originalDependency": original, "dependency": edge, "wireAction": "preserve"}));
}

fn unresolved(plan: &mut Plan, child: &Value, edge: &Value, reason: &str, source: Value) {
    plan.unresolved.push(
        json!({"operationId": child["id"], "dependsOnOperationId": edge["dependsOnOperationId"],
        "reason": reason, "command": child, "dependencyRecord": edge, "sourceEvidence": source}),
    );
}
