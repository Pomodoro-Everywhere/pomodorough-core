use super::Local;
use crate::{CoreError, timer::WireCommand};

pub(super) fn commands(local: &Local) -> Result<Vec<WireCommand>, CoreError> {
    let queues = crate::reconciliation::workspace::display::queues(
        &local.workspace,
        local.projection_pending.as_ref(),
    )
    .map_err(|error| match error {
        CoreError::InvalidInput(reason)
            if matches!(
                reason.as_str(),
                "incomplete persisted projection queues"
                    | "invalid persisted projection identity"
                    | "persisted projection does not match retained payloads"
            ) =>
        {
            CoreError::InvalidInput(format!("bootstrap workspace requires recovery: {reason}"))
        }
        error => error,
    })?;
    Ok(serde_json::from_value(queues["commands"].clone())?)
}
