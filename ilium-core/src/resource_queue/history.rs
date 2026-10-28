//! Durable bounded lifecycle summaries; receipts remain the retry authority.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
pub(super) fn validate_history(catalog: &ResourceCatalog) -> Result<(), ResourceDomainError> {
    if catalog.history.len() > MAX_RESOURCE_HISTORY_EVENTS {
        return Err(error("quota", "resource event history capacity exceeded"));
    }
    let mut previous = None;
    let mut counts = BTreeMap::new();
    for event in &catalog.history {
        let position = (event.revision, event.ordinal);
        if event.revision == 0
            || event.revision > catalog.revision
            || previous.is_some_and(|prior| prior >= position)
            || !catalog.resources.contains_key(&event.resource_id)
            || event.unix_millis < 0
            || event.message.len() > 1024
            || event.message.chars().any(char::is_control)
        {
            return Err(error("corrupt_history", "invalid resource lifecycle event"));
        }
        if event.reservation_id.as_ref().is_some_and(|id| {
            id.0.is_empty() || id.0.len() > 256 || id.0.chars().any(char::is_control)
        }) {
            return Err(error(
                "corrupt_history",
                "invalid lifecycle reservation identity",
            ));
        }
        if let ResourceActor::Agent(agent) = &event.actor {
            if agent.pane_id.0 == 0
                || [
                    &agent.project_id.0,
                    &agent.session_id,
                    &agent.runtime_generation,
                    &agent.invocation_id.0,
                ]
                .iter()
                .any(|value| {
                    value.is_empty() || value.len() > 256 || value.chars().any(char::is_control)
                })
                || agent.label.len() > 480
                || agent.label.contains('\0')
            {
                return Err(error("corrupt_history", "invalid lifecycle actor"));
            }
        }
        let count = counts.entry(&event.resource_id).or_insert(0usize);
        *count += 1;
        if *count > MAX_HISTORY_EVENTS_PER_RESOURCE {
            return Err(error(
                "quota",
                "per-resource lifecycle history capacity exceeded",
            ));
        }
        previous = Some(position);
    }
    Ok(())
}
pub(super) fn record_lifecycle(
    before: &ResourceCatalog,
    after: &mut ResourceCatalog,
    command: &ResourceCommand,
    context: &MutationContext,
) {
    let revision = after.revision;
    let mut events = Vec::new();
    let mut add = |resource: &ResourceId,
                   reservation: Option<&ReservationId>,
                   kind: ResourceLifecycleKind,
                   message: &str| {
        events.push(ResourceLifecycleEvent {
            resource_id: resource.clone(),
            reservation_id: reservation.cloned(),
            revision,
            ordinal: 0,
            unix_millis: context.clock.unix_millis,
            actor: context.actor.clone(),
            kind,
            message: message
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect(),
        })
    };
    for (id, record) in &after.resources {
        let old: BTreeMap<_, _> = before
            .resources
            .get(id)
            .into_iter()
            .flat_map(|record| record.reservations.iter())
            .map(|claim| (&claim.id, claim))
            .collect();
        for claim in &record.reservations {
            let prior = old.get(&claim.id).copied();
            if prior.is_none() {
                add(
                    id,
                    Some(&claim.id),
                    ResourceLifecycleKind::Reserved,
                    "Reservation entered the exclusive FIFO queue",
                );
            }
            if prior.is_none_or(|prior| {
                prior.state != claim.state
                    || prior.ownership_generation != claim.ownership_generation
            }) {
                let transition = match &claim.state {
                    ReservationState::Owned => Some((
                        ResourceLifecycleKind::Acquired,
                        "Exclusive ownership granted",
                    )),
                    ReservationState::NeedsAttention { reason } => {
                        Some((ResourceLifecycleKind::NeedsAttention, reason.as_str()))
                    }
                    ReservationState::Released => Some((
                        ResourceLifecycleKind::Released,
                        "Owner explicitly released the resource",
                    )),
                    ReservationState::Cancelled => Some((
                        ResourceLifecycleKind::Cancelled,
                        "Queued reservation cancelled",
                    )),
                    ReservationState::Revoked { reason } => {
                        Some((ResourceLifecycleKind::Revoked, reason.as_str()))
                    }
                    ReservationState::Queued => None,
                };
                if let Some((kind, message)) = transition {
                    add(id, Some(&claim.id), kind, message)
                }
            }
            if let Some(prior) = prior {
                if prior.estimate_seconds != claim.estimate_seconds {
                    add(
                        id,
                        Some(&claim.id),
                        ResourceLifecycleKind::EstimateChanged,
                        "Reservation estimate changed; ownership is unchanged",
                    );
                }
                if prior.acquisition_pending
                    && !claim.acquisition_pending
                    && claim.state == ReservationState::Owned
                {
                    add(
                        id,
                        Some(&claim.id),
                        ResourceLifecycleKind::Acknowledged,
                        "Owning invocation acknowledged acquisition",
                    );
                }
            }
        }
    }
    let administrative = match command {
        ResourceCommand::Create { definition, .. } => Some((
            definition.id.clone(),
            ResourceLifecycleKind::Created,
            "Resource created and linked",
        )),
        ResourceCommand::Link { link } => Some((
            link.resource_id.clone(),
            ResourceLifecycleKind::Linked,
            "Project linked to resource",
        )),
        ResourceCommand::Unlink { resource_id, .. } => Some((
            resource_id.clone(),
            ResourceLifecycleKind::Unlinked,
            "Project unlinked from resource",
        )),
        ResourceCommand::Edit { definition } => Some((
            definition.id.clone(),
            ResourceLifecycleKind::Edited,
            "Resource definition updated",
        )),
        ResourceCommand::Archive { resource_id } => Some((
            resource_id.clone(),
            ResourceLifecycleKind::Archived,
            "Resource archived",
        )),
        ResourceCommand::SetGate { resource_id, .. } => Some((
            resource_id.clone(),
            ResourceLifecycleKind::GateChanged,
            "Resource acquisition gate changed",
        )),
        ResourceCommand::ConfirmClear { resource_id, .. } => Some((
            resource_id.clone(),
            ResourceLifecycleKind::RecoveryConfirmed,
            "User confirmed physical clearance and reopened the resource",
        )),
        _ => None,
    };
    if let Some((resource, kind, message)) = administrative {
        add(&resource, None, kind, message)
    }
    if let ResourceCommand::RebindProject { project_id, .. } = command {
        let resources: BTreeSet<_> = after
            .links
            .iter()
            .filter(|link| link.project_id == *project_id)
            .map(|link| link.resource_id.clone())
            .collect();
        for resource in resources {
            add(
                &resource,
                None,
                ResourceLifecycleKind::ProjectRebound,
                "Project path explicitly rebound without changing claim identities",
            );
        }
    }
    if matches!(command, ResourceCommand::ReconcileBoot) && before.boot_id != after.boot_id {
        for resource in after.resources.keys() {
            add(
                resource,
                None,
                ResourceLifecycleKind::BootFenced,
                "Machine boot changed; existing occupancy requires guarded recovery",
            );
        }
    }
    for (ordinal, event) in events.iter_mut().enumerate() {
        event.ordinal = ordinal as u32;
    }
    after.history.extend(events);
    let mut counts = BTreeMap::new();
    let mut keep = Vec::with_capacity(after.history.len().min(MAX_RESOURCE_HISTORY_EVENTS));
    for event in after.history.drain(..).rev() {
        let count = counts.entry(event.resource_id.clone()).or_insert(0usize);
        if *count < MAX_HISTORY_EVENTS_PER_RESOURCE && keep.len() < MAX_RESOURCE_HISTORY_EVENTS {
            *count += 1;
            keep.push(event)
        }
    }
    keep.reverse();
    after.history = keep;
}
