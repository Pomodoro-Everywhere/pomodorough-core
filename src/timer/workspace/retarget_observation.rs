//! Private replay evidence supplied only after validating a continuous live clock.
use super::super::{Command, ReductionState, Session, sorted_commands};
use super::{
    CanonicalTimer, CoreError, HistoryItem, TimerReductionOutput, WireCommand, parse_time, prepare,
};
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub(crate) struct RunningTimer {
    timer_id: String,
    anchor_at: DateTime<Utc>,
    elapsed_at_anchor_ms: i64,
}

impl RunningTimer {
    pub(crate) fn new(
        timer_id: &str,
        anchor_at: &str,
        elapsed_at_anchor_ms: i64,
    ) -> Result<Self, CoreError> {
        Ok(Self {
            timer_id: timer_id.into(),
            anchor_at: parse_time(anchor_at)?,
            elapsed_at_anchor_ms,
        })
    }

    fn matches(&self, session: &Session) -> bool {
        session.timer_id == self.timer_id
            && session.status == "running"
            && session.phase == "focus"
            && session.anchor_at == self.anchor_at
            && session.elapsed_at_anchor_ms == self.elapsed_at_anchor_ms
    }

    fn from_session(session: &Session) -> Self {
        Self {
            timer_id: session.timer_id.clone(),
            anchor_at: session.anchor_at,
            elapsed_at_anchor_ms: session.elapsed_at_anchor_ms,
        }
    }
}

// A later resume sample also witnesses the earlier lifecycle segments of this
// retained timer. Retargets cannot change those segments or prove a new anchor.
fn observed_lineage(
    initial: &ReductionState,
    commands: &[Command],
    observed: &RunningTimer,
) -> Vec<RunningTimer> {
    let mut state = ReductionState {
        sessions: initial.sessions.clone(),
        current_id: initial.current_id.clone(),
        outcomes: Default::default(),
    };
    let mut candidates = Vec::new();
    let mut witnessed = 0;
    for command in commands.iter().map(Some).chain(std::iter::once(None)) {
        if let Some(session) = state
            .current_id
            .as_ref()
            .and_then(|id| state.sessions.get(id))
        {
            if session.timer_id == observed.timer_id
                && session.status == "running"
                && session.phase == "focus"
            {
                if candidates
                    .last()
                    .is_none_or(|anchor: &RunningTimer| !anchor.matches(session))
                {
                    candidates.push(RunningTimer::from_session(session));
                }
                if observed.matches(session) {
                    witnessed = candidates.len();
                }
            }
        }
        if let Some(command) = command.filter(|command| command.kind != "retarget") {
            state.apply(command.clone());
        }
    }
    candidates.truncate(witnessed);
    candidates
}

fn apply_observed(state: &mut ReductionState, command: Command, lineage: &[RunningTimer]) {
    let witnessed = command.kind == "retarget"
        && command.phase == "focus"
        && state
            .current_id
            .as_ref()
            .and_then(|id| state.sessions.get(id))
            .is_some_and(|session| {
                command.timer_id == session.timer_id
                    && lineage.iter().any(|anchor| anchor.matches(session))
            });
    if witnessed {
        // The live sample witnesses attribution before expiry. Rounded wire
        // elapsed cannot decide that boundary. Lifecycle replay and finish
        // still perform expiry at their unchanged clocks.
        state.apply_retarget(command);
    } else {
        state.apply(command);
    }
}

pub(crate) fn replay(
    timer: Option<CanonicalTimer>,
    history: Vec<HistoryItem>,
    commands: Vec<WireCommand>,
    now: &str,
    observed: &RunningTimer,
) -> Result<TimerReductionOutput, CoreError> {
    // Validate the original complete wire records before touching private replay time.
    let prepared = prepare(timer, history, commands)?;
    let now = parse_time(now)?;
    let mut state = ReductionState {
        sessions: prepared.sessions,
        current_id: prepared.current_id,
        outcomes: Default::default(),
    };
    let commands = sorted_commands(prepared.commands);
    let lineage = observed_lineage(&state, &commands, observed);
    for command in commands {
        apply_observed(&mut state, command, &lineage);
    }
    let mut result = state.finish(now)?;
    super::restore_native_device(&mut result, &prepared.devices);
    super::validate_pair(result.canonical_timer.as_ref(), &result.history)?;
    Ok(result)
}
