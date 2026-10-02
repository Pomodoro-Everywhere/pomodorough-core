use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub(super) enum Domain {
    Commands,
    TaskOperations,
    DurationOperations,
    AutoStartOperations,
    SelectedTaskOperations,
}

pub(super) const DOMAINS: [Domain; 5] = [
    Domain::Commands,
    Domain::TaskOperations,
    Domain::DurationOperations,
    Domain::AutoStartOperations,
    Domain::SelectedTaskOperations,
];

impl Domain {
    pub(super) fn next(self) -> Self {
        match self {
            Self::Commands => Self::TaskOperations,
            Self::TaskOperations => Self::DurationOperations,
            Self::DurationOperations => Self::AutoStartOperations,
            Self::AutoStartOperations => Self::SelectedTaskOperations,
            Self::SelectedTaskOperations => Self::Commands,
        }
    }
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    Sync,
    Merge,
    ReplaceRemote,
    KeepRemote,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Limits {
    pub(super) per_domain: usize,
    pub(super) total: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Operation {
    pub(super) id: String,
    pub(super) device_id: String,
    pub(super) hlc_wall_ms: i64,
    pub(super) hlc_counter: i64,
    pub(super) device_sequence: Option<i64>,
}

impl Operation {
    pub(super) fn key(&self) -> (i64, i64, &str, &str) {
        (
            self.hlc_wall_ms,
            self.hlc_counter,
            &self.device_id,
            &self.id,
        )
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Dependency {
    pub(super) operation_id: String,
    pub(super) depends_on_operation_id: String,
}

pub(super) type Queues = BTreeMap<Domain, Vec<Operation>>;
pub(super) type IdQueues = BTreeMap<Domain, Vec<String>>;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Request {
    New {
        mode: Mode,
        limits: Limits,
        #[serde(rename = "nextDomain")]
        next_domain: Domain,
        queues: Queues,
        #[serde(rename = "timerDependencies")]
        timer_dependencies: Vec<Dependency>,
    },
    Saved {
        mode: Mode,
        limits: Limits,
        queues: IdQueues,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    Planned,
    BlockedDependency,
    Oversized,
    ReplaySaved,
    OversizedSaved,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Plan {
    pub(super) status: Status,
    pub(super) selected: IdQueues,
    pub(super) next_domain: Option<Domain>,
    pub(super) held_timer_operation_id: Option<String>,
    pub(super) counts: BTreeMap<Domain, usize>,
    pub(super) total: usize,
}

impl Plan {
    pub(super) fn empty(counts: BTreeMap<Domain, usize>, next_domain: Option<Domain>) -> Self {
        Self {
            status: Status::Planned,
            selected: DOMAINS
                .into_iter()
                .map(|domain| (domain, Vec::new()))
                .collect(),
            next_domain,
            held_timer_operation_id: None,
            total: counts.values().sum(),
            counts,
        }
    }
}
