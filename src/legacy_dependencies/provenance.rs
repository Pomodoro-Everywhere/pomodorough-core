use serde_json::Value;

use super::{CoreError, invalid};

pub(super) fn saved_finish(saved: &Value, completion: &Value) -> Result<(), CoreError> {
    if saved["type"] != "finish"
        || saved["phase"] != "focus"
        || completion["status"] != "completed"
        || completion["phase"] != saved["phase"]
        || completion["commandId"] != saved["id"]
        || completion["timerId"] != saved["timerId"]
        || completion["plannedDurationMs"] != saved["plannedDurationMs"]
    {
        return Err(invalid("conflicting saved legacy source"));
    }
    // timer::transition_session assigns an explicit Finish's occurredAt to
    // both terminal timestamps. Physical observations never change sent wire time.
    let occurred = crate::timer::parse_time(saved["occurredAt"].as_str().unwrap())?;
    let completed = super::evidence::completion_time(completion)?;
    if occurred != completed {
        return Err(invalid("conflicting saved legacy completion time"));
    }
    for field in ["completedAt", "endedAt"] {
        if let Some(stamp) = completion[field].as_str() {
            if crate::timer::parse_time(stamp)? != occurred {
                return Err(invalid("contradictory saved legacy terminal times"));
            }
        }
    }
    Ok(())
}
