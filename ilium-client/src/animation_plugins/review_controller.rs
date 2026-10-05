//! Native bridge → existing controller; this implements no ledger or grant mint.
use super::review_bridge::{ReviewAction, ReviewBridge, ReviewPhase};
use crate::filesystem::plugin_permission_controller::{
    ActivationUpdate, PermissionCancellation, PluginPermissionController,
};
use ilium_animation_js::permissions::Selection;
use ilium_animation_js::permissions::{Capability, Invalidation, PlanReview};
type Result<T> = std::result::Result<T, String>;
/// Coordinates supplied by the actual current controller/broker, never UI.
struct CurrentReviewAuthority {
    selection_revision: u64,
    instance_id: u64,
    plan_revision: u64,
    authorization_epoch: u64,
}
pub(crate) enum IntentOutcome {
    None,
    Resolved,
    Picking(PickSelection),
    PickingAudio(PickAudioSelection),
    /// Preserve ORIGINAL stop/invalidation and durable refusal through actual
    /// teardown. Parent must consume this before another controller poll.
    Cancelled(PermissionCancellation),
}
pub(crate) struct PickSelection {
    pub(crate) selection_revision: u64,
    pub(crate) request_id: String,
    pub(crate) path: String,
    pub(crate) slot: String,
    pub(crate) disk_selection: Selection,
    pub(crate) writable: bool,
    pub(crate) review_revision: u64,
    pub(crate) authorization_epoch: u64,
}
pub(crate) struct PickAudioSelection {
    pub(crate) selection_revision: u64,
    pub(crate) request_id: String,
    pub(crate) endpoint: String,
    pub(crate) scope_device: String,
    pub(crate) capability: Capability,
    pub(crate) review_revision: u64,
    pub(crate) authorization_epoch: u64,
}
/// Call on the owning worker after observing a real intent wake. The closure
/// must apply invalidation to ORIGINAL services/acquisition/publication owners;
/// queueing a later cleanup does not satisfy this synchronous effect boundary.
pub(crate) fn consume_intent(
    controller: &mut PluginPermissionController,
    bridge: &ReviewBridge,
    selection_revision: u64,
    apply_invalidation: &mut impl FnMut(&Invalidation) -> Result<()>,
) -> Result<IntentOutcome> {
    let authority = controller.with_review_state(
        selection_revision,
        |_, _, instance_id, plan_revision, authorization_epoch| CurrentReviewAuthority {
            selection_revision,
            instance_id,
            plan_revision,
            authorization_epoch,
        },
    )?;
    let Some(action) = bridge.take_action(
        authority.selection_revision,
        authority.instance_id,
        authority.plan_revision,
        authority.authorization_epoch,
    )?
    else {
        return Ok(IntentOutcome::None);
    };
    match action {
        ReviewAction::Resolve {
            selection_revision,
            review,
            answers,
        } => {
            let invalidation = controller.resolve(selection_revision, review, answers)?;
            apply_invalidation(invalidation)?; // Must precede controller.poll/create.
            if controller.is_pending_io() {
                bridge.set_phase(selection_revision, ReviewPhase::DurableWrite, None)?;
            } else {
                bridge.set_phase(selection_revision, ReviewPhase::Creating, None)?;
            }
        }
        ReviewAction::Cancel { .. } => {
            return Ok(IntentOutcome::Cancelled(controller.cancel()));
        }
        ReviewAction::Pick {
            selection_revision,
            request_id,
            path,
            slot,
            disk_selection,
            writable,
            review_revision,
            authorization_epoch,
        } => {
            return Ok(IntentOutcome::Picking(PickSelection {
                selection_revision,
                request_id,
                path,
                slot,
                disk_selection,
                writable,
                review_revision,
                authorization_epoch,
            }));
        }
        ReviewAction::PickAudio {
            selection_revision,
            request_id,
            endpoint,
            scope_device,
            capability,
            review_revision,
            authorization_epoch,
        } => {
            return Ok(IntentOutcome::PickingAudio(PickAudioSelection {
                selection_revision,
                request_id,
                endpoint,
                scope_device,
                capability,
                review_revision,
                authorization_epoch,
            }));
        }
    }
    Ok(IntentOutcome::Resolved)
}
/// Preserve real Finished/Failed inventories for the worker. Ready is published
/// only from an actual controller creation result, after native invalidation.
pub(crate) fn collect_controller(
    controller: &mut PluginPermissionController,
    selection_revision: u64,
    apply_invalidation: &mut impl FnMut(&Invalidation) -> Result<()>,
) -> Result<Option<ActivationUpdate>> {
    if let Some(invalidation) = controller.pending_invalidation() {
        apply_invalidation(invalidation)?;
    }
    let Some(update) = controller.poll(selection_revision) else {
        return Ok(None);
    };
    Ok(Some(update))
}
/// Requires the root-owned read-only preactivation getter specified in this
/// proposal's caller contract. No principal/package clone or mutable escape.
pub(crate) fn publish_native_review(
    controller: &PluginPermissionController,
    bridge: &ReviewBridge,
    selection_revision: u64,
    review: PlanReview,
) -> Result<()> {
    controller.with_review_state(
        selection_revision,
        |package, principal, instance, revision, epoch| {
            if review.instance_id() != instance
                || review.plan_revision() != revision
                || review.authorization_epoch() != epoch
            {
                return Err("Native review superseded before UI publication".into());
            }
            bridge.publish(selection_revision, package, principal, review)
        },
    )?
}
/// Borrow the caller-retained effect inventory: refusal never drops stop or
/// activation cleanup ownership. Caller retries/settles it before another poll.
pub(crate) fn publish_controller_outcome(
    bridge: &ReviewBridge,
    selection_revision: u64,
    update: &ActivationUpdate,
    apply_invalidation: &mut impl FnMut(&Invalidation) -> Result<()>,
) -> Result<()> {
    match update {
        ActivationUpdate::Finished(resolution) => {
            apply_invalidation(&resolution.invalidation)?;
            if let Some(invalidation) = &resolution.activation_invalidation {
                apply_invalidation(invalidation)?;
            }
            let phase = match resolution.accepted_creation() {
                Some(ilium_animation_js::engine::CreateState::Ready) => ReviewPhase::Ready,
                Some(ilium_animation_js::engine::CreateState::Pending) => ReviewPhase::Creating,
                None => ReviewPhase::Failed,
            };
            bridge.set_phase(
                selection_revision,
                phase,
                if phase == ReviewPhase::Failed {
                    Some("Required rights denied or native startup/authority/teardown failed")
                } else {
                    None
                },
            )
        }
        ActivationUpdate::Failed { message, stop } => {
            if let Some(invalidation) = stop.as_ref().and_then(|stop| stop.invalidation.as_ref()) {
                apply_invalidation(invalidation)?;
            }
            bridge.deny_pending(selection_revision, message)
        }
        ActivationUpdate::Review(_) => {
            Err("Native review must be published with its original token".into())
        }
    }
}
/// Cancellation remains caller-owned until real stop and native persistence
/// invalidation are settled. An error keeps this inventory available to retry.
pub(crate) fn publish_cancellation(
    bridge: &ReviewBridge,
    selection_revision: u64,
    cancellation: &PermissionCancellation,
    apply_invalidation: &mut impl FnMut(&Invalidation) -> Result<()>,
) -> Result<()> {
    if let Some(stop) = &cancellation.stop {
        if let Some(invalidation) = &stop.invalidation {
            apply_invalidation(invalidation)?;
        }
        if let Some(error) = &stop.authority_error {
            let message = error.to_string();
            bridge.set_phase(selection_revision, ReviewPhase::Failed, Some(&message))?;
            return Err(message);
        }
        if let Err(error) = &stop.cancellation {
            let message = error.to_string();
            bridge.set_phase(selection_revision, ReviewPhase::Failed, Some(&message))?;
            return Err(message);
        }
    }
    if let Some(invalidation) = &cancellation.delegated_invalidation {
        apply_invalidation(invalidation)?;
    }
    if let Some(error) = &cancellation.delegated_authority_error {
        bridge.set_phase(selection_revision, ReviewPhase::Failed, Some(error))?;
        return Err(error.clone());
    }
    if let Some(error) = &cancellation.persistence_error {
        bridge.set_phase(selection_revision, ReviewPhase::Failed, Some(error))?;
        return Err(error.clone());
    }
    bridge.set_phase(selection_revision, ReviewPhase::Cancelled, None)
}
