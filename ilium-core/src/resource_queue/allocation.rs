//! Exhaustive capacity accounting for queue protocol/admission values.
use super::*;
use crate::AllocationSize;
use std::mem::size_of;
macro_rules! fields { ($kind:ty {$($field:ident),* $(,)?}) => {impl AllocationSize for $kind {fn heap_bytes(&self)->usize {let Self{$($field),*}=self;0usize$(.saturating_add($field.heap_bytes()))*}}}; }
macro_rules! ids {($($kind:ty),*)=>{$(impl AllocationSize for $kind{fn heap_bytes(&self)->usize{self.0.heap_bytes()}})*};}
ids!(
    ResourceId,
    ReservationId,
    ResourceProjectId,
    RequestId,
    InvocationId
);
impl<K: AllocationSize, V: AllocationSize> AllocationSize for BTreeMap<K, V> {
    fn heap_bytes(&self) -> usize {
        // BTreeMap exposes no capacity; a minimum-occupancy tree can retain empty
        // slots in every node. Charge sixteen full entry slots and node pointers per
        // live entry, including the underfull root, then every owned nested capacity.
        self.iter().fold(
            self.len()
                .saturating_mul(size_of::<(K, V)>().saturating_mul(16).saturating_add(256)),
            |n, (k, v)| {
                n.saturating_add(k.heap_bytes())
                    .saturating_add(v.heap_bytes())
            },
        )
    }
}
fields!(AgentIdentity {
    project_id,
    session_id,
    pane_id,
    runtime_generation,
    invocation_id,
    label
});
impl AllocationSize for ResourceActor {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Agent(agent) => agent.heap_bytes(),
            Self::User | Self::System => 0,
        }
    }
}
fields!(ResourceClock {
    unix_millis,
    monotonic_millis,
    boot_id
});
impl AllocationSize for InvocationEligibility {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Live | Self::Dead | Self::Unknown => 0,
        }
    }
}
fields!(MutationContext {
    request_id,
    clock,
    actor,
    expected_revision,
    eligibility
});
impl AllocationSize for ResourceGate {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Open => 0,
            Self::Paused { reason } | Self::Unavailable { reason } => reason.heap_bytes(),
        }
    }
}
impl AllocationSize for ReservationState {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Queued | Self::Owned | Self::Released | Self::Cancelled => 0,
            Self::NeedsAttention { reason } | Self::Revoked { reason } => reason.heap_bytes(),
        }
    }
}
fields!(ResourceDefinition {
    policy,
    created_at_unix_millis,
    updated_at_unix_millis,
    id,
    title,
    kind,
    physical_identity,
    description,
    rules,
    definition_revision,
    archived
});
fields!(ProjectResourceLink {
    created_at_unix_millis,
    project_id,
    resource_id,
    slug,
    project_path,
    desired_instruction_revision,
    sync_debt
});
fields!(Reservation {
    id,
    request_id,
    resource_id,
    agent,
    ticket,
    estimate_seconds,
    requested_at,
    acquired_at,
    terminal_at,
    state,
    ownership_generation,
    acquisition_pending
});
fields!(ResourceRecord {
    definition,
    gate,
    clearance_required,
    next_ticket,
    next_generation,
    reservations
});
fields!(MutationReceipt {
    command,
    actor,
    result,
    created_at
});
fields!(ResourceCatalog {
    history,
    schema_version,
    revision,
    boot_id,
    resources,
    links,
    receipts
});
impl AllocationSize for ResourceCommand {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::RebindProject {
                project_id,
                project_path,
            } => project_id
                .heap_bytes()
                .saturating_add(project_path.heap_bytes()),
            Self::Create { definition, link } => {
                definition.heap_bytes().saturating_add(link.heap_bytes())
            }
            Self::Link { link } => link.heap_bytes(),
            Self::Edit { definition } => definition.heap_bytes(),
            Self::Unlink {
                project_id,
                resource_id,
            } => project_id
                .heap_bytes()
                .saturating_add(resource_id.heap_bytes()),
            Self::Archive { resource_id } => resource_id.heap_bytes(),
            Self::Reserve {
                resource_id,
                reservation_id,
                estimate_seconds,
            } => resource_id
                .heap_bytes()
                .saturating_add(reservation_id.heap_bytes())
                .saturating_add(estimate_seconds.heap_bytes()),
            Self::Release {
                reservation_id,
                ownership_generation,
            }
            | Self::Acknowledge {
                reservation_id,
                ownership_generation,
            } => reservation_id
                .heap_bytes()
                .saturating_add(ownership_generation.heap_bytes()),
            Self::Cancel { reservation_id } => reservation_id.heap_bytes(),
            Self::CancelByUser {
                reservation_id,
                reason,
            }
            | Self::Revoke {
                reservation_id,
                reason,
            } => reservation_id
                .heap_bytes()
                .saturating_add(reason.heap_bytes()),
            Self::Estimate {
                reservation_id,
                estimate_seconds,
            } => reservation_id
                .heap_bytes()
                .saturating_add(estimate_seconds.heap_bytes()),
            Self::SetGate { resource_id, gate } => {
                resource_id.heap_bytes().saturating_add(gate.heap_bytes())
            }
            Self::ConfirmClear {
                resource_id,
                reason,
            }
            | Self::RecoverStaleOwner {
                resource_id,
                reason,
            } => resource_id.heap_bytes().saturating_add(reason.heap_bytes()),
            Self::ReconcileInvocation { agent, evidence } => {
                agent.heap_bytes().saturating_add(evidence.heap_bytes())
            }
            Self::ReconcileBoot | Self::ReconcileEligibility => 0,
            Self::SetSyncDebt {
                project_id,
                resource_id,
                reason,
            } => project_id
                .heap_bytes()
                .saturating_add(resource_id.heap_bytes())
                .saturating_add(reason.heap_bytes()),
        }
    }
}
fields!(ResourceCommandResult {
    resource_id,
    reservation_id,
    revision,
    reused
});
fields!(ResourceEvent {
    resource_id,
    reservation_id,
    message
});
fields!(ResourceTransition {
    catalog,
    result,
    events
});
fields!(ResourceDomainError { code, message });
fields!(ResourceQueueEstimate {
    wait_seconds,
    estimated_start_unix_millis,
    reason,
    computed_at_unix_millis
});
fields!(ResourceQueueEntry {
    reservation,
    position,
    estimate
});
fields!(ResourceQueueSnapshot {
    resource_id,
    definition,
    gate,
    owner,
    queue,
    observation_reason
});
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unused_string_and_vector_capacity_are_charged() {
        let mut identity = ResourceId(String::with_capacity(4096));
        identity.0.push('a');
        assert!(identity.heap_bytes() >= 4096);
        let mut map = BTreeMap::new();
        map.insert(
            identity,
            ResourceGate::Paused {
                reason: String::with_capacity(8192),
            },
        );
        assert!(map.heap_bytes() >= 12288);
    }
}

fields!(ResourceLifecycleEvent {
    resource_id,
    reservation_id,
    revision,
    ordinal,
    unix_millis,
    actor,
    kind,
    message
});
impl AllocationSize for ResourceLifecycleKind {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Created
            | Self::Linked
            | Self::Unlinked
            | Self::Edited
            | Self::Archived
            | Self::Reserved
            | Self::Acquired
            | Self::Released
            | Self::Cancelled
            | Self::Revoked
            | Self::NeedsAttention
            | Self::GateChanged
            | Self::RecoveryConfirmed
            | Self::EstimateChanged
            | Self::Acknowledged
            | Self::ProjectRebound
            | Self::BootFenced => 0,
        }
    }
}

impl AllocationSize for ResourcePolicy {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::ExclusiveFifo => 0,
        }
    }
}
