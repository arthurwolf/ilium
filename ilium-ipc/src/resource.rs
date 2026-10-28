//! Typed native resource authority requests. Identity proofs are server-owned.
use ilium_core::{
    resource_queue::{
        Reservation, ReservationId, ResourceCatalog, ResourceClock, ResourceCommand,
        ResourceCommandResult, ResourceId,
    },
    NodeId,
};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceOperation {
    Snapshot,
    /// Server-to-server fresh fencing proof; never mutates the catalog.
    ValidateInvocation {
        agent: ilium_core::resource_queue::AgentIdentity,
    },
    Mutate(ResourceCommand),
    Status {
        reservation_id: ReservationId,
    },
    Wait {
        reservation_id: ReservationId,
    },
    RepairInstructions {
        resource_id: ResourceId,
    },
    PrepareRecovery {
        resource_id: ResourceId,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRequest {
    pub project_id: Option<NodeId>,
    pub pane_id: Option<NodeId>,
    pub request_key: Option<String>,
    pub expected_revision: Option<u64>,
    pub recovery_token: Option<String>,
    pub operation: ResourceOperation,
}
impl ResourceRequest {
    pub fn new(project_id: Option<NodeId>, operation: ResourceOperation) -> Self {
        Self {
            project_id,
            pane_id: None,
            request_key: None,
            expected_revision: None,
            recovery_token: None,
            operation,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceReply {
    pub active_waits: Vec<(NodeId, ReservationId)>,
    pub wait_revision: u64,
    pub local_invocations: Vec<ilium_core::resource_queue::AgentIdentity>,
    pub catalog: ResourceCatalog,
    pub clock: ResourceClock,
    pub result: Option<ResourceCommandResult>,
    pub reservation: Option<Reservation>,
    pub recovery_token: Option<String>,
    pub message: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ResourceError {
    pub code: String,
    pub message: String,
}
