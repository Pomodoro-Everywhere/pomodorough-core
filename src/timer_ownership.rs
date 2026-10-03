//! Shared PWA missing-owner evidence, lease admission, and checked owner writes.
use serde::Deserialize;
use serde_json::{Value, json};

use crate::CoreError;

pub(crate) const MAX_SAFE_MS: i64 = 9_007_199_254_740_991;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Ownership {
    pub(crate) timer_id: String,
    pub(crate) device_id: String,
    #[serde(default)]
    pub(crate) tab_id: Option<String>,
    #[serde(default)]
    pub(crate) lease_expires_at_ms: Option<i64>,
}

pub(crate) fn can_claim_missing(workspace: &Value, timer: &Value, local: &str) -> bool {
    let canonical = &workspace["base"]["canonicalTimer"];
    if canonical["id"] == timer["id"] && canonical.get("startedByDeviceId").is_some() {
        return canonical["startedByDeviceId"] == local;
    }
    workspace["local"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["type"] == "start" && command["timerId"] == timer["id"])
}

pub(crate) fn owns_lease(
    owner: &Ownership,
    timer_id: &Value,
    local_device: &str,
    local_tab: Option<&str>,
    now_ms: i64,
) -> (bool, Option<i64>) {
    if owner.timer_id != *timer_id || owner.device_id != local_device {
        return (false, None);
    }
    if owner.tab_id.as_deref() == local_tab
        || owner
            .lease_expires_at_ms
            .is_none_or(|expiry| expiry <= now_ms)
    {
        (true, None)
    } else {
        (false, owner.lease_expires_at_ms)
    }
}

pub(crate) fn record_write(
    timer_id: &Value,
    device: &str,
    tab: Option<&str>,
    now_ms: i64,
    duration_ms: i64,
) -> Result<Value, CoreError> {
    let expiry = now_ms
        .checked_add(duration_ms)
        .filter(|value| *value <= MAX_SAFE_MS)
        .ok_or_else(|| CoreError::InvalidInput("lease expiry overflow".into()))?;
    Ok(json!({"kind": "recordTimerOwner", "timerId": timer_id,
        "deviceId": device, "tabId": tab, "leaseExpiresAtMs": expiry}))
}
