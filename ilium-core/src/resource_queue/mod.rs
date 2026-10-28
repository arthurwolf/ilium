//! Pure exclusive hardware queue authority. No clocks, randomness or I/O are read here.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod allocation;
mod estimate;
mod history;
mod transition;
pub use estimate::*;
pub use transition::*;
#[cfg(test)]
mod tests;
macro_rules! id { ($($name:ident),*) => {$ (
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)] pub struct $name(pub String);
impl From<&str> for $name { fn from(value: &str)->Self { Self(value.into()) } }
impl std::fmt::Display for $name { fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { self.0.fmt(f) } }
)*}; }
id!(
    ResourceId,
    ReservationId,
    ResourceProjectId,
    RequestId,
    InvocationId
);
pub const MAX_ESTIMATE_SECONDS: u64 = 604800;
pub const REQUEST_WINDOW_MILLIS: i64 = 604800000;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub project_id: ResourceProjectId,
    pub session_id: String,
    pub pane_id: crate::NodeId,
    pub runtime_generation: String,
    pub invocation_id: InvocationId,
    pub label: String,
}
impl AgentIdentity {
    pub fn same_invocation(&self, other: &Self) -> bool {
        self.project_id == other.project_id
            && self.session_id == other.session_id
            && self.pane_id == other.pane_id
            && self.runtime_generation == other.runtime_generation
            && self.invocation_id == other.invocation_id
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceActor {
    Agent(AgentIdentity),
    User,
    System,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceClock {
    pub unix_millis: i64,
    pub monotonic_millis: u64,
    pub boot_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvocationEligibility {
    Live,
    Dead,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationContext {
    pub request_id: RequestId,
    pub clock: ResourceClock,
    pub actor: ResourceActor,
    pub expected_revision: Option<u64>,
    pub eligibility: BTreeMap<InvocationId, InvocationEligibility>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceGate {
    Open,
    Paused { reason: String },
    Unavailable { reason: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservationState {
    Queued,
    Owned,
    NeedsAttention { reason: String },
    Released,
    Cancelled,
    Revoked { reason: String },
}
impl ReservationState {
    pub fn active(&self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Owned | Self::NeedsAttention { .. }
        )
    }
    pub fn occupied(&self) -> bool {
        matches!(self, Self::Owned | Self::NeedsAttention { .. })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourcePolicy {
    ExclusiveFifo,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceDefinition {
    pub policy: ResourcePolicy,
    pub created_at_unix_millis: i64,
    pub updated_at_unix_millis: i64,
    pub id: ResourceId,
    pub title: String,
    pub kind: String,
    pub physical_identity: Option<String>,
    pub description: String,
    pub rules: String,
    pub definition_revision: u64,
    pub archived: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectResourceLink {
    pub created_at_unix_millis: i64,
    pub project_id: ResourceProjectId,
    pub resource_id: ResourceId,
    pub slug: String,
    pub project_path: String,
    pub desired_instruction_revision: u64,
    pub sync_debt: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reservation {
    pub id: ReservationId,
    pub request_id: RequestId,
    pub resource_id: ResourceId,
    pub agent: AgentIdentity,
    pub ticket: u64,
    pub estimate_seconds: u64,
    pub requested_at: ResourceClock,
    pub acquired_at: Option<ResourceClock>,
    pub terminal_at: Option<i64>,
    pub state: ReservationState,
    pub ownership_generation: u64,
    pub acquisition_pending: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRecord {
    pub definition: ResourceDefinition,
    pub gate: ResourceGate,
    pub clearance_required: bool,
    pub next_ticket: u64,
    pub next_generation: u64,
    pub reservations: Vec<Reservation>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationReceipt {
    pub command: ResourceCommand,
    pub actor: ResourceActor,
    pub result: ResourceCommandResult,
    pub created_at: i64,
}
/// Bounded durable audit summary, ordered by (revision, ordinal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLifecycleEvent {
    pub resource_id: ResourceId,
    pub reservation_id: Option<ReservationId>,
    pub revision: u64,
    pub ordinal: u32,
    pub unix_millis: i64,
    pub actor: ResourceActor,
    pub kind: ResourceLifecycleKind,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceLifecycleKind {
    Created,
    Linked,
    Unlinked,
    Edited,
    Archived,
    Reserved,
    Acquired,
    Released,
    Cancelled,
    Revoked,
    NeedsAttention,
    GateChanged,
    RecoveryConfirmed,
    EstimateChanged,
    Acknowledged,
    ProjectRebound,
    BootFenced,
}
// 256 summaries per resource accommodate several lifecycle actions for each
// of the 16 terminal claims retained in the detail projection, plus active
// handoffs and estimate changes. The independent 4096-event global bound and
// repository byte admission prevent multiplying this allowance by all resources.
pub const MAX_RESOURCE_HISTORY_EVENTS: usize = 4096;
pub const MAX_HISTORY_EVENTS_PER_RESOURCE: usize = 256;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCatalog {
    pub history: Vec<ResourceLifecycleEvent>,
    pub schema_version: u32,
    pub revision: u64,
    pub boot_id: String,
    pub resources: BTreeMap<ResourceId, ResourceRecord>,
    pub links: Vec<ProjectResourceLink>,
    pub receipts: BTreeMap<RequestId, MutationReceipt>,
}
impl Default for ResourceCatalog {
    fn default() -> Self {
        Self {
            schema_version: 1,
            history: Vec::new(),
            revision: 0,
            boot_id: String::new(),
            resources: BTreeMap::new(),
            links: Vec::new(),
            receipts: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceCommand {
    RebindProject {
        project_id: ResourceProjectId,
        project_path: String,
    },
    Create {
        definition: ResourceDefinition,
        link: ProjectResourceLink,
    },
    Link {
        link: ProjectResourceLink,
    },
    Edit {
        definition: ResourceDefinition,
    },
    Unlink {
        project_id: ResourceProjectId,
        resource_id: ResourceId,
    },
    Archive {
        resource_id: ResourceId,
    },
    Reserve {
        resource_id: ResourceId,
        reservation_id: ReservationId,
        estimate_seconds: u64,
    },
    Release {
        reservation_id: ReservationId,
        ownership_generation: u64,
    },
    Cancel {
        reservation_id: ReservationId,
    },
    CancelByUser {
        reservation_id: ReservationId,
        reason: String,
    },
    Estimate {
        reservation_id: ReservationId,
        estimate_seconds: u64,
    },
    Acknowledge {
        reservation_id: ReservationId,
        ownership_generation: u64,
    },
    SetGate {
        resource_id: ResourceId,
        gate: ResourceGate,
    },
    Revoke {
        reservation_id: ReservationId,
        reason: String,
    },
    ConfirmClear {
        resource_id: ResourceId,
        reason: String,
    },
    ReconcileInvocation {
        agent: AgentIdentity,
        evidence: InvocationEligibility,
    },
    ReconcileBoot,
    ReconcileEligibility,
    SetSyncDebt {
        project_id: ResourceProjectId,
        resource_id: ResourceId,
        reason: Option<String>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCommandResult {
    pub resource_id: Option<ResourceId>,
    pub reservation_id: Option<ReservationId>,
    pub revision: u64,
    pub reused: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceEvent {
    pub resource_id: ResourceId,
    pub reservation_id: Option<ReservationId>,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTransition {
    pub catalog: ResourceCatalog,
    pub result: ResourceCommandResult,
    pub events: Vec<ResourceEvent>,
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[error("{code}: {message}")]
pub struct ResourceDomainError {
    pub code: String,
    pub message: String,
}
pub(crate) fn error(code: &str, message: impl Into<String>) -> ResourceDomainError {
    ResourceDomainError {
        code: code.into(),
        message: message.into(),
    }
}
