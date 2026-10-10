//! Immutable ownership of one already admitted native live-source snapshot.
//!
//! This module performs no acquisition, provider I/O, script dispatch or replay
//! authorization. A capture retains the exact admitted source snapshot Arc and
//! a non-dispatchable broker fence. Replay may transfer that owner only while
//! the original activation fence is current.

use crate::{
    error::{AnimationError, Result},
    permissions::{PermissionBroker, SourceCaptureFence},
    replay::InputFamily,
    runtime::PackageInstance,
    sources::{AdmittedSourceSnapshot, SourceSnapshotFingerprint},
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use std::sync::{Arc, Mutex};

const CAPTURE_METADATA_BYTES: usize = 64 * 1024;

fn charge(quota: &QuotaGroup) -> Result<StorageAdmission> {
    quota
        .reserve_external_storage(CAPTURE_METADATA_BYTES)
        .map_err(|_| AnimationError::Budget("native source capture metadata".into()))
}

/// Fixed-copy information about a captured feed.
///
/// This is evidence and accounting only. It contains neither the source payload,
/// an image handle, a Channel, an operation ticket nor any replay authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeSourceCaptureSummary {
    pub family: InputFamily,
    pub source_revision: u64,
    pub instance_id: u64,
    pub plan_generation: u64,
    pub authorization_epoch: u64,
    pub payload_digest: [u8; 32],
    pub image_digest: [u8; 32],
    pub content_digest: [u8; 32],
    pub payload_bytes: usize,
    pub image_bytes: usize,
    pub resident_bytes: usize,
    pub metadata_bytes: usize,
    pub total_accounted_bytes: usize,
}

/// Native first-stage source recording.
///
/// The retained snapshot owns the original source result admission and the
/// original NativeSourceImage pixel Arcs. This owner deliberately retains no
/// feed StopToken, worker receipt, HTTP operation or dispatch-capable Channel.
pub struct NativeCapturedFeed {
    owner: Arc<Mutex<PermissionBroker>>,
    fence: SourceCaptureFence,
    quota: QuotaGroup,
    snapshot: Arc<AdmittedSourceSnapshot>,
    summary: NativeSourceCaptureSummary,
    replay_lineage: Vec<crate::replay::GrantLineage>,
    _metadata: StorageAdmission,
}

impl NativeCapturedFeed {
    /// Trusted native producer only.
    ///
    /// `snapshot` must be the exact Arc returned by the live SourceDispatcher.
    /// The resident limit is checked against that actual admitted payload and
    /// image inventory before capture metadata is admitted.
    #[allow(clippy::too_many_arguments)] // Each capture fence, lease and identity remains explicit at this security boundary.
    pub(crate) fn from_native_source(
        instance: &PackageInstance,
        owner: Arc<Mutex<PermissionBroker>>,
        fence: SourceCaptureFence,
        quota: QuotaGroup,
        family: InputFamily,
        snapshot: Arc<AdmittedSourceSnapshot>,
        replay_lineage: Vec<crate::replay::GrantLineage>,
        max_resident_bytes: usize,
    ) -> Result<Arc<Self>> {
        if max_resident_bytes == 0 {
            return Err(AnimationError::Budget(
                "native source capture resident limit".into(),
            ));
        }

        let fingerprint: SourceSnapshotFingerprint =
            instance.with_source_capture_authority(&owner, &fence, &quota, || {
                snapshot.capture_fingerprint()
            })?;

        if fingerprint.resident_bytes > max_resident_bytes {
            return Err(AnimationError::Budget(
                "native source capture exceeds resident limit".into(),
            ));
        }

        let (instance_id, plan_generation, authorization_epoch) = fence.coordinates();

        let total_accounted_bytes = fingerprint
            .resident_bytes
            .checked_add(CAPTURE_METADATA_BYTES)
            .ok_or_else(|| {
                AnimationError::Budget("native source capture accounting overflow".into())
            })?;

        let summary = NativeSourceCaptureSummary {
            family,
            source_revision: snapshot.view().revision(),
            instance_id,
            plan_generation,
            authorization_epoch,
            payload_digest: fingerprint.payload_digest,
            image_digest: fingerprint.image_digest,
            content_digest: fingerprint.content_digest,
            payload_bytes: fingerprint.payload_bytes,
            image_bytes: fingerprint.image_bytes,
            resident_bytes: fingerprint.resident_bytes,
            metadata_bytes: CAPTURE_METADATA_BYTES,
            total_accounted_bytes,
        };

        // Admit the capture object's bounded metadata before Arc::new allocates
        // its native owner. The payload itself remains on its original debit.
        let metadata = charge(&quota)?;

        // Authorization may have changed while quota admission ran. Recheck
        // before publishing a usable captured owner.
        instance.with_source_capture_authority(&owner, &fence, &quota, || ())?;

        Ok(Arc::new(Self {
            owner,
            fence,
            quota,
            snapshot,
            summary,
            replay_lineage,
            _metadata: metadata,
        }))
    }

    /// Read only fixed capture evidence while this exact original activation is
    /// still current. Revocation/replan/retire, a foreign instance, foreign
    /// broker/root, helper retirement or a poisoned broker all refuse.
    pub fn summary(&self, instance: &PackageInstance) -> Result<NativeSourceCaptureSummary> {
        instance
            .with_source_capture_authority(&self.owner, &self.fence, &self.quota, || self.summary)
    }

    pub fn replay_lineage(&self) -> &[crate::replay::GrantLineage] {
        &self.replay_lineage
    }

    /// Copy an admitted source value into replay-owned storage, then recheck
    /// the original activation before publishing the copy. Image pixels remain
    /// retained as exact admitted allocations, never flattened or re-decoded.
    pub fn to_frozen_service_value(
        &self,
        instance: &PackageInstance,
    ) -> Result<crate::engine::ServiceValue> {
        let metadata = serde_json::to_value(self.snapshot.view())?;
        let value = crate::engine::ServiceValue::copy_from_host(
            &metadata,
            &[],
            &std::collections::BTreeMap::new(),
            instance.engine_limits(),
            self.quota.clone(),
        )?;
        instance.with_source_capture_authority(&self.owner, &self.fence, &self.quota, || ())?;
        Ok(value)
    }

    /// Retain the exact image allocations alongside their slot-based source
    /// metadata. The caller still must pass `verify_frozen_snapshot` before
    /// those values can enter a replay clip.
    pub fn replay_images(
        &self,
        instance: &PackageInstance,
    ) -> Result<Vec<crate::sources::NativeSourceImage>> {
        instance.with_source_capture_authority(&self.owner, &self.fence, &self.quota, || {
            self.snapshot.native_images().to_vec()
        })
    }

    /// Recheck that a proposed frozen value is byte-for-byte the metadata of
    /// this exact admitted source snapshot while its original activation fence
    /// remains current. Native image slots must also retain the exact original
    /// admitted allocations in matching order.
    pub fn verify_frozen_snapshot(
        &self,
        instance: &PackageInstance,
        frozen: &crate::replay::FrozenInputSnapshot,
    ) -> Result<()> {
        // Serialization is bounded by the already-admitted source payload and
        // stays outside the broker lock; the closure below only compares fixed
        // identities and rechecks the activation fence.
        let source_value = serde_json::to_value(self.snapshot.view())?;
        let payload_matches = &source_value == frozen.value().metadata();
        let source_images = self.snapshot.native_images();
        let image_owners_match = source_images.len() == frozen.native_images().len()
            && source_images
                .iter()
                .zip(frozen.native_images())
                .all(|(source, retained)| source.same_allocation(retained));
        instance.with_source_capture_authority(&self.owner, &self.fence, &self.quota, || {
            if frozen.family() != self.summary.family
                || frozen.revision() != self.summary.source_revision
                || !payload_matches
                || !image_owners_match
            {
                return Err(AnimationError::PermissionDenied(
                    "frozen source identity or image custody mismatch".into(),
                ));
            }
            Ok(())
        })?
    }
}

impl std::fmt::Debug for NativeCapturedFeed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeCapturedFeed")
            .field("summary", &self.summary)
            .finish_non_exhaustive()
    }
}
