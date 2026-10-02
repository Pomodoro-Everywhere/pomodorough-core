use serde_json::Value;

use crate::{CoreError, timer::WireCommand};

pub(super) fn android(workspace: &Value) -> Result<String, CoreError> {
    let commands: Vec<WireCommand> =
        serde_json::from_value(workspace["local"]["commands"].clone())?;
    let latest = commands.iter().max_by_key(|command| {
        (
            command.hlc_wall_ms,
            command.hlc_counter,
            command.device_id.as_str(),
            command.id.as_str(),
        )
    });
    // Match SynchronizedProjectionRequestFactory: last command in HLC/UTF-8
    // order, not the latest occurrence, observation, or filtered command.
    let timestamp = latest
        .map(|command| command.occurred_at.as_str())
        .or_else(|| workspace["base"]["canonicalTimer"]["anchorAt"].as_str())
        .unwrap_or("1970-01-01T00:00:00Z");
    crate::timer::parse_time(timestamp)?;
    Ok(timestamp.to_owned())
}
