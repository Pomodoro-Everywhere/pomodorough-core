use chrono::{DateTime, Utc};

use crate::CoreError;
use crate::timer::{HistoryItem, parse_time};

pub(crate) struct FocusCount {
    pub(crate) total: usize,
    pub(crate) through_source: Option<usize>,
}

pub(crate) fn focus_count(
    history: &[HistoryItem],
    command_id: &str,
    timer_id: &str,
    (day_start, day_end): (DateTime<Utc>, DateTime<Utc>),
) -> Result<FocusCount, CoreError> {
    let mut focuses = Vec::new();
    for item in history
        .iter()
        .filter(|item| item.phase == "focus" && item.status == "completed")
    {
        let timestamp = item
            .completed_at
            .as_deref()
            .or(item.ended_at.as_deref())
            .ok_or_else(|| CoreError::InvalidInput("invalid timer history".into()))?;
        let completed_at = parse_time(timestamp)?;
        if completed_at >= day_start && completed_at < day_end {
            let identity = item.command_id.as_deref().unwrap_or(&item.timer_id);
            focuses.push((completed_at, identity, item));
        }
    }
    // Stable timestamp/identity ordering preserves input order for identical keys.
    // The source requires both identities; another timer's command is not evidence.
    focuses.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)));
    let through_source = focuses
        .iter()
        .position(|(_, _, item)| {
            item.command_id.as_deref() == Some(command_id) && item.timer_id == timer_id
        })
        .map(|index| index + 1);
    Ok(FocusCount {
        total: focuses.len(),
        through_source,
    })
}
