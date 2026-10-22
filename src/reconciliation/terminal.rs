use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{RebaseInput, delivery, rebase, timer_boundary::Boundary, validation};
use crate::CoreError;

const FIELDS: [(&str, &str); 5] = [
    ("commands", "pending"),
    ("taskOperations", "pendingTaskOperations"),
    ("durationOperations", "pendingDurationOperations"),
    ("autoStartOperations", "pendingAutoStartOperations"),
    ("selectedTaskOperations", "pendingSelectedTaskOperations"),
];

pub(crate) fn rebase_json(input: &str) -> Result<String, CoreError> {
    let raw = crate::strict_json::parse(input)?;
    validation::request_structure(&raw)?;
    validate_controls(&raw)?;
    let base = canonical_base(&raw["response"]);
    crate::timer::workspace::validate_native_device(&json!({"base": base}))?;
    let policy = delivery::Policy::from_request(&raw)?;
    let input: RebaseInput = serde_json::from_value(raw.clone())?;
    let display = display_before(&raw, &base)?;
    let typed_local = serde_json::to_value(&input.local)?;
    let output = rebase(input, Some(&policy), Boundary::Workspace)?;
    let mut output: Value = serde_json::from_str(&policy.serialize(output)?)?;
    preserve_pending(&mut output, &raw, &typed_local);
    let workspace = display_after(&raw, &base, display, &mut output)?;
    install_projection(&mut output, &raw["response"], workspace);
    Ok(serde_json::to_string(&output)?)
}

fn validate_controls(raw: &Value) -> Result<(), CoreError> {
    if raw.as_object().is_some_and(|root| {
        root.keys().any(|key| {
            !matches!(
                key.as_str(),
                "local"
                    | "pending"
                    | "sent"
                    | "response"
                    | "neverSent"
                    | "timerDependencies"
                    | "displayContext"
            )
        })
    }) {
        return Err(CoreError::InvalidInput(
            "unknown reconciliation v3 control".into(),
        ));
    }
    Ok(())
}

fn display_before(raw: &Value, base: &Value) -> Result<Option<Value>, CoreError> {
    if raw.get("displayContext").is_none() {
        return Ok(None);
    }
    let local = raw.get("local").unwrap_or(&raw["pending"]);
    let request = json!({"base": base, "local": local,
        "canonicalHead": {"wallMs": raw["response"]["serverHlcWallMs"], "counter": raw["response"]["serverHlcCounter"]},
        "neverSent": raw.get("neverSent").cloned().unwrap_or(json!({})),
        "timerDependencies": raw.get("timerDependencies").cloned().unwrap_or(json!([])),
        "now": raw["response"]["serverTime"], "displayContext": raw["displayContext"]});
    let projected: Value =
        serde_json::from_str(&super::workspace::project_json(&request.to_string())?)?;
    Ok(Some(
        projected["displayContext"]["projectionPending"].clone(),
    ))
}

fn display_after(
    raw: &Value,
    base: &Value,
    stored: Option<Value>,
    output: &mut Value,
) -> Result<Value, CoreError> {
    let Some(stored) = stored else {
        let mut request = serde_json::Map::with_capacity(3);
        request.insert("base".into(), base.clone());
        request.insert("pending".into(), output["projectionPending"].clone());
        request.insert("now".into(), raw["response"]["serverTime"].clone());
        return crate::projection::apply_workspace_value(serde_json::Value::Object(request));
    };
    let local: Value = FIELDS
        .iter()
        .map(|(queue, field)| ((*queue).to_owned(), output[*field].clone()))
        .collect();
    let proof = never_sent_proof(raw, &local);
    let request = terminal_projection_request(base, local, proof, raw, output, &stored);
    let mut projected = super::workspace::project_value(serde_json::Value::Object(request))?;
    output["displayContext"] = projected["displayContext"].take();
    Ok(projected["workspace"].take())
}

fn never_sent_proof(raw: &Value, local: &Value) -> Value {
    let mut proof = json!({});
    for name in delivery::QUEUES {
        proof[name] = json!(
            raw["neverSent"][name]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|id| local[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|op| op["id"] == **id))
                .collect::<Vec<_>>()
        );
    }
    proof
}

fn terminal_projection_request(
    base: &Value,
    local: Value,
    proof: Value,
    raw: &Value,
    output: &Value,
    stored: &Value,
) -> serde_json::Map<String, Value> {
    let mut request = serde_json::Map::with_capacity(8);
    request.insert("base".into(), base.clone());
    request.insert("local".into(), local);
    request.insert("neverSent".into(), proof);
    request.insert(
        "canonicalHead".into(),
        json!({"wallMs": raw["response"]["serverHlcWallMs"], "counter": raw["response"]["serverHlcCounter"]}),
    );
    request.insert(
        "timerDependencies".into(),
        output["pendingTimerDependencies"].clone(),
    );
    request.insert("now".into(), raw["response"]["serverTime"].clone());
    request.insert(
        "displayContext".into(),
        super::workspace::display::context(super::workspace::display::trim(
            stored,
            &request["local"],
        )),
    );
    request
}

fn canonical_base(response: &Value) -> Value {
    json!({"canonicalTimer":response["canonicalTimer"], "history":response["history"],
        "tasks":response["tasks"], "durationsMs":response["durationsMs"],
        "autoStartBreaks":response["autoStartBreaks"], "selectedTaskId":response["selectedTaskId"]})
}

fn preserve_pending(output: &mut Value, raw: &Value, typed: &Value) {
    let local = raw.get("local").unwrap_or(&raw["pending"]);
    for (queue, field) in FIELDS {
        let originals = operations_by_id(&local[queue]);
        let previous = operations_by_id(&typed[queue]);
        if let Some(pending) = output[field].as_array_mut() {
            for operation in pending {
                let id = operation["id"]
                    .as_str()
                    .expect("validated retained identity");
                if let (Some(original), Some(before)) = (originals.get(id), previous.get(id)) {
                    *operation = preserve_operation(original, before, operation);
                }
            }
        }
        if output["projectionPending"][queue]
            .as_array()
            .is_some_and(|items| !items.is_empty())
        {
            output["projectionPending"][queue] = output[field].clone();
        }
    }
}

fn operations_by_id(queue: &Value) -> BTreeMap<&str, &Value> {
    queue
        .as_array()
        .into_iter()
        .flatten()
        .map(|operation| {
            (
                operation["id"]
                    .as_str()
                    .expect("validated operation identity"),
                operation,
            )
        })
        .collect()
}

fn preserve_operation(original: &Value, before: &Value, after: &Value) -> Value {
    let mut retained = original.clone();
    if let Some(fields) = after.as_object() {
        for (key, value) in fields {
            // Only the existing dependency policy may change never-sent fields.
            // Unchanged typed defaults must not replace raw omission or null.
            if before.get(key) != Some(value) {
                retained[key] = value.clone();
            }
        }
    }
    retained
}

fn install_projection(output: &mut Value, response: &Value, workspace: Value) {
    for (field, source) in [
        ("baseTimer", "canonicalTimer"),
        ("baseHistory", "history"),
        ("baseTasks", "tasks"),
        ("baseDurationsMs", "durationsMs"),
        ("baseAutoStartBreaks", "autoStartBreaks"),
        ("baseSelectedTaskId", "selectedTaskId"),
    ] {
        output[field] = response[source].clone();
    }
    for (field, source) in [
        ("timer", "canonicalTimer"),
        ("history", "history"),
        ("tasks", "tasks"),
        ("durationsMs", "durationsMs"),
        ("autoStartBreaks", "autoStartBreaks"),
        ("selectedTaskId", "selectedTaskId"),
    ] {
        output[field] = workspace[source].clone();
    }
    output["schemaVersion"] = json!(3);
    output["canonicalResponse"] = response.clone();
    output["workspace"] = workspace;
}
