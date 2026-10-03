use crate::CoreError;
use crate::timer::{self, CanonicalTimer, HistoryItem, TimerReductionOutput, WireCommand};

#[derive(Clone, Copy)]
pub(super) enum Boundary {
    Strict,
    Workspace,
}

impl Boundary {
    pub(super) fn validate(
        self,
        timer: &Option<CanonicalTimer>,
        history: &[HistoryItem],
    ) -> Result<(), CoreError> {
        match self {
            Self::Strict => timer::validate_replay_state(timer, history),
            Self::Workspace => timer::workspace::validate_pair(timer.as_ref(), history).map(|_| ()),
        }
    }

    pub(super) fn replay(
        self,
        timer: Option<CanonicalTimer>,
        history: Vec<HistoryItem>,
        commands: Vec<WireCommand>,
        now: &str,
    ) -> Result<TimerReductionOutput, CoreError> {
        match self {
            Self::Strict => timer::replay(timer, history, commands, now),
            Self::Workspace => timer::workspace::replay(timer, history, commands, now),
        }
    }
}
