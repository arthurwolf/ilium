//! Worker-owned package activation. Informational script plans never become
//! authority: only the native broker's current channel can activate an instance.
use crate::{
    engine::{
        ArraySpec, CompletionState, CreateState, HostRequest, ServiceAuthority, ServicePhase,
        ServiceValue,
    }, // Use the admitted binary service boundary.
    error::{AnimationError, Result},
    helper::{
        HelperAuthority, HelperLimits, HelperPlayback, HelperSession, PreparedSeed,
        RetainedHelperFrame,
    },
    manifest::AnimationMode,
    native_video::{VideoAuthority, VideoAuthorization, VideoOperation},
    package::{Package, PackageLimits},
    permissions::{
        Activation,
        CallPhase,
        Ceiling,
        Channel,
        FrameEmissionOutcome,
        FrameEmissionTicket,
        HostBinding,
        Invalidation,
        OperationNeed,
        OperationTicket,
        PermissionBroker, // Retain genuine native operation authority.
        PlanReview,
        UserChoice, // Keep host-reviewed consent independent of helper transport.
    },
    plan::{AnimationPlan, PlanBudget},
    plan_authorization::AuthorizationProjection,
    replay::{
        GrantLineage, ReplayAccess, ReplayAuthority, ReplayAuthorization, ReplayFlushedProof,
        TerminalFrameStamp,
    },
    trust::{PackageIdentity as VerifiedIdentity, TrustVerifier},
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::owned_worker::StopToken;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::Instant,
};

pub struct PackageInstance {
    helper: HelperSession,
    quota: QuotaGroup,
    pending_resolution: Option<Arc<()>>,
    helper_retired: bool, // Logical retirement intent never proves that the child or its pipe workers exited.
    replay_retired_proof: Arc<AtomicBool>, // Published only after actual child and pipe-worker exit.
    broker: Arc<Mutex<PermissionBroker>>,
    identity: VerifiedIdentity,
    package: Arc<Package>,
    mode: AnimationMode, // Retain the native-selected mode so every emission path enforces pre-rendered retirement.
    ambient_seed: u32, // Derived from verified package and normalized settings before module evaluation.
    ambient_bootstrap_digest: [u8; 32],
    environment_digest: [u8; 32],
    helper_build_digest: [u8; 32],
    replay_request_count: u64,
    replay_pure_yield_count: u64,
    replay_completed_yield_count: u64,
    replay_video_open_count: u64,
    replay_source_open_count: u64,
    replay_source_freeze_count: u64,
    replay_source_sequence_count: u64,
    plan: AnimationPlan,
    projection: AuthorizationProjection,
    selected_storage: BTreeMap<String, Arc<crate::native_storage::SelectedStorage>>,
    _selected_storage_metadata: StorageAdmission,
    activation: Option<Activation>,
    creation: Option<CreateState>,
    settings: Value,
    // Original storage is owned by the shared broker through its FINAL native owner.
}

/// Read-only retirement evidence for one exact helper session. The Arc identity
/// is minted with PackageInstance and only retire_helper publishes true after
/// the original child and every original pipe worker have physically exited.
#[derive(Clone)]
pub(crate) struct HelperRetirementEvidence {
    retired: Arc<AtomicBool>,
}
impl HelperRetirementEvidence {
    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.retired, &other.retired)
    }
    pub(crate) fn is_physically_retired(&self) -> bool {
        self.retired.load(Ordering::Acquire)
    }
}

/// Native-only settlement evidence bound to the exact request allocation and
/// original ticket Arcs. Only actual broker delivery/settlement updates the flag;
/// helper transport or deferred-retirement errors cannot erase that evidence.
pub(crate) struct ServiceGroupSettlement {
    request: HostRequest,
    tickets: Vec<Arc<OperationTicket>>,
    settled: AtomicBool,
}
impl ServiceGroupSettlement {
    pub(crate) fn was_settled(&self) -> bool {
        self.settled.load(Ordering::Acquire)
    }
    fn check_original(&self, operations: &[&ServiceOperation]) -> Result<()> {
        let request = original_service_group_request(operations)?;
        if !std::ptr::eq(&*self.request, &**request)
            || self.tickets.len() != operations.len()
            || self
                .tickets
                .iter()
                .zip(operations)
                .any(|(ticket, operation)| !Arc::ptr_eq(ticket, &operation.ticket))
        {
            return Err(AnimationError::PermissionDenied(
                "foreign service group settlement receipt".into(),
            ));
        }
        Ok(())
    }
}

/// Original broker channel retained with a published frame. The coordinates in
/// a frame's identity are checked against this channel; they never construct it.
/// Authorization linearizes when the presenter commits an output operation.
/// A later revocation cannot undo an already committed write; the terminal
/// result is settled independently of source-history credit.
/// An original activation revocation owner that remains callable while an
/// admitted preparation job temporarily owns the PackageInstance. Numeric
/// coordinates do not create it; the broker's private channel is retained.
pub struct RetainedReplayRevoker {
    broker: Arc<Mutex<PermissionBroker>>,
    channel: Channel,
    instance_id: u64,
    retired: bool,
    _storage: StorageAdmission,
}
impl RetainedReplayRevoker {
    pub fn revoke(&mut self) -> Result<Option<Invalidation>> {
        if self.retired {
            return Ok(None);
        }
        let mut broker = lock_broker(&self.broker)?;
        broker
            .grant(&self.channel, "_host_channel_check")
            .map_err(permission_error)?;
        let invalidation = broker.retire(self.instance_id);
        self.retired = true;
        Ok(Some(invalidation))
    }
}

/// The only VideoAuthorization exposed to a decoder. It retains the native
/// channel, original package identity and quota debit; the guest supplies none
/// of these. Revocation invalidates the broker channel even after finite bytes
/// have been acquired or the V8 helper has physically retired.
struct InstanceVideoAuthorization {
    broker: Arc<Mutex<PermissionBroker>>,
    channel: Channel,
    expected: VideoAuthority,
    resource_id: u64,
    _storage: StorageAdmission,
}
impl VideoAuthorization for InstanceVideoAuthorization {
    fn check(
        &self,
        authority: &VideoAuthority,
        resource_id: u64,
        _operation: VideoOperation,
    ) -> Result<()> {
        if authority != &self.expected || resource_id != self.resource_id {
            return Err(AnimationError::PermissionDenied(
                "video authorization has no original resource owner".into(),
            ));
        }
        let broker = lock_broker(&self.broker)?;
        let (instance_id, plan_revision, authorization_epoch) = broker
            .channel_coordinates(&self.channel)
            .map_err(permission_error)?;
        if instance_id != authority.instance_id
            || plan_revision != authority.plan_revision
            || authorization_epoch != authority.authorization_epoch
        {
            return Err(AnimationError::PermissionDenied(
                "video authorization is stale or foreign".into(),
            ));
        }
        Ok(())
    }
}

pub struct RetainedFrameAuthority {
    broker: Arc<Mutex<PermissionBroker>>,
    channel: Channel,
    expected: HelperAuthority,
    quota: QuotaGroup,
    _storage: StorageAdmission,
}

impl std::fmt::Debug for RetainedFrameAuthority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RetainedFrameAuthority")
            .field("instance_id", &self.expected.instance_id)
            .field("plan_generation", &self.expected.plan_generation)
            .finish_non_exhaustive()
    }
}

/// A non-cloneable permit held by the actual terminal writer through backend
/// flush. A successful flush only proves bytes were accepted by the backend;
/// it does not certify which source dots survived composition.
#[must_use = "settle the committed terminal operation"]
pub struct CommittedFrameEmission {
    broker: Arc<Mutex<PermissionBroker>>,
    expected: HelperAuthority,
    ticket: Option<FrameEmissionTicket>,
    frame_stamp: Option<Arc<TerminalFrameStamp>>,
    storage: Option<StorageAdmission>,
}

impl CommittedFrameEmission {
    fn settle(mut self, outcome: FrameEmissionOutcome) -> Result<()> {
        let ticket = self.ticket.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("terminal emission already settled".into())
        })?;
        lock_broker(&self.broker)?
            .settle_frame_emission(ticket, outcome)
            .map_err(permission_error)?;
        self.ticket.take();
        Ok(())
    }

    /// The backend accepted the whole frame. Source history is still the
    /// responsibility of the exact composed-frame receipt after this event.
    pub fn backend_flushed(mut self) -> Result<ReplayFlushedProof> {
        let ticket = self.ticket.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("terminal emission already settled".into())
        })?;
        lock_broker(&self.broker)?
            .settle_frame_emission(ticket, FrameEmissionOutcome::BackendFlushed)
            .map_err(permission_error)?;
        self.ticket.take();
        let storage = self.storage.take().ok_or_else(|| {
            AnimationError::PermissionDenied("terminal emission admission missing".into())
        })?;
        Ok(ReplayFlushedProof::from_native(
            Arc::clone(&self.broker),
            ReplayAuthority {
                package_digest: self.expected.package_digest.clone(),
                instance_id: self.expected.instance_id,
                revision: self.expected.plan_generation,
                authorization_epoch: self.expected.authorization_epoch,
            },
            self.frame_stamp.take(),
            storage,
        ))
    }

    /// The backend may have accepted a prefix. Do not credit source history or
    /// reuse the previous diff base; the presenter must stop/resynchronize.
    pub fn backend_failed_uncertain(self) -> Result<()> {
        self.settle(FrameEmissionOutcome::BackendFailureUncertain)
    }
}
impl Drop for CommittedFrameEmission {
    fn drop(&mut self) {
        // Unwind or early exit after the native effect boundary is uncertain.
        // A poisoned broker cannot prove settlement: quarantine the original
        // admission rather than silently release an unsettled physical effect.
        if let Some(ticket) = self.ticket.take() {
            let settled = self.broker.lock().is_ok_and(|mut broker| {
                broker
                    .settle_frame_emission(&ticket, FrameEmissionOutcome::BackendFailureUncertain)
                    .is_ok()
            });
            if !settled {
                if let Some(storage) = self.storage.take() {
                    std::mem::forget(storage);
                }
            }
        }
    }
}

impl RetainedFrameAuthority {
    #[cfg(test)]
    pub(crate) fn from_active_test_channel(
        broker: Arc<Mutex<PermissionBroker>>,
        channel: Channel,
        expected: HelperAuthority,
        quota: QuotaGroup,
    ) -> Result<Self> {
        let owner = lock_broker(&broker)?;
        let coordinates = owner
            .channel_coordinates(&channel)
            .map_err(permission_error)?;
        if coordinates
            != (
                expected.instance_id,
                expected.plan_generation,
                expected.authorization_epoch,
            )
            || owner
                .identity()
                .content_hash()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                != expected.package_digest
        {
            return Err(AnimationError::PermissionDenied(
                "test frame owner must retain the original active broker channel".into(),
            ));
        }
        drop(owner);
        let storage = quota.reserve_external_storage(4096).map_err(|error| {
            AnimationError::Budget(format!("test frame owner admission: {error:?}"))
        })?;
        Ok(Self {
            broker,
            channel,
            expected,
            quota,
            _storage: storage,
        })
    }

    /// Charge the original root before the scene worker allocates terminal
    /// source-index sets and raw owner counters. The caller retains this lease
    /// through the complete bounded mapping pass.
    pub fn reserve_world_receipt_scratch(&self, bytes: usize) -> Result<StorageAdmission> {
        self.quota
            .reserve_external_storage(bytes.max(1))
            .map_err(|error| {
                AnimationError::Budget(format!("world terminal mapping scratch: {error:?}"))
            })
    }
    /// This exact non-cloneable proof was minted by the same original broker
    /// after a complete terminal flush. Revocation after begin_output does not
    /// invalidate already emitted bytes; foreign channels cannot use the proof.
    pub fn validate_flushed_proof(
        &self,
        frame: &HelperAuthority,
        stamp: &Arc<TerminalFrameStamp>,
        proof: &ReplayFlushedProof,
    ) -> Result<()> {
        let authority = ReplayAuthority {
            package_digest: frame.package_digest.clone(),
            instance_id: frame.instance_id,
            revision: frame.plan_generation,
            authorization_epoch: frame.authorization_epoch,
        };
        if frame != &self.expected || !proof.belongs_to_frame(&self.broker, &authority, stamp) {
            return Err(AnimationError::PermissionDenied(
                "world terminal proof belongs to another original broker or plan".into(),
            ));
        }
        Ok(())
    }

    /// World history proof is created only for a source retained by this
    /// original quota root and the exact queued frame's terminal flush.
    #[allow(clippy::too_many_arguments)] // Keep explicit bounded inputs and original authority at this existing boundary.
    pub fn world_emission_after_proof(
        &self,
        frame: &HelperAuthority,
        stamp: &Arc<TerminalFrameStamp>,
        proof: &ReplayFlushedProof,
        binding: &Arc<crate::native_worlds::WorldDotBinding>,
        source_indices: &[usize],
        emitted_owner_dots: &std::collections::BTreeMap<u32, u32>,
        composition_revision: u64,
    ) -> Result<crate::native_worlds::WorldEmission> {
        self.validate_flushed_proof(frame, stamp, proof)?;
        if !binding.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "world source belongs to a foreign native quota root".into(),
            ));
        }
        crate::native_worlds::WorldEmission::after_proven_host_emission(
            binding,
            source_indices,
            emitted_owner_dots,
            composition_revision,
        )
    }

    pub fn begin_output(&self, frame: &HelperAuthority) -> Result<CommittedFrameEmission> {
        self.begin_output_inner(frame, None)
    }
    pub fn begin_output_for_presentation(
        &self,
        frame: &HelperAuthority,
        stamp: Arc<TerminalFrameStamp>,
    ) -> Result<CommittedFrameEmission> {
        self.begin_output_inner(frame, Some(stamp))
    }
    fn begin_output_inner(
        &self,
        frame: &HelperAuthority,
        frame_stamp: Option<Arc<TerminalFrameStamp>>,
    ) -> Result<CommittedFrameEmission> {
        if frame != &self.expected {
            return Err(AnimationError::PermissionDenied(
                "published frame identity differs from original native owner".into(),
            ));
        }
        // Admit the in-flight native effect before broker commitment. The
        // token retains this same original quota root until physical settlement.
        let storage = self.quota.reserve_external_storage(2048).map_err(|error| {
            AnimationError::Budget(format!("terminal emission admission: {error:?}"))
        })?;
        let mut broker = lock_broker(&self.broker)?;
        let (instance_id, plan_generation, authorization_epoch) = broker
            .channel_coordinates(&self.channel)
            .map_err(permission_error)?;
        if frame.instance_id != instance_id
            || frame.plan_generation != plan_generation
            || frame.authorization_epoch != authorization_epoch
            || frame.package_digest != self.expected.package_digest
        {
            return Err(AnimationError::PermissionDenied(
                "published frame native activation is stale".into(),
            ));
        }
        // The original broker records the effect under this same mutex. A
        // revocation before this call denies without terminal I/O; a later
        // revocation fences future calls but preserves this committed outcome.
        let ticket = broker
            .begin_frame_emission(&self.channel)
            .map_err(permission_error)?;
        Ok(CommittedFrameEmission {
            broker: Arc::clone(&self.broker),
            expected: frame.clone(),
            ticket: Some(ticket),
            frame_stamp,
            storage: Some(storage),
        })
    }
}

/// Replay retains the original private channel. Captured grant lineage is
/// rechecked against this broker at every protected access; serialized values
/// never become an independent authority.
struct BrokerReplayAuthorization {
    broker: Arc<Mutex<PermissionBroker>>,
    channel: Channel,
    expected: ReplayAuthority,
    retired: Arc<AtomicBool>,
    _storage: StorageAdmission,
}
impl ReplayAuthorization for BrokerReplayAuthorization {
    fn check(
        &self,
        authority: &ReplayAuthority,
        lineage: &[GrantLineage],
        access: ReplayAccess,
    ) -> Result<()> {
        if authority != &self.expected {
            return Err(AnimationError::PermissionDenied(
                "stale replay requires its original native owner".into(),
            ));
        }
        if matches!(
            access,
            ReplayAccess::CachedDelivery | ReplayAccess::Playback | ReplayAccess::Emission
        ) && !self.retired.load(Ordering::Acquire)
        {
            return Err(AnimationError::PermissionDenied(
                "replay producer has not physically retired".into(),
            ));
        }
        let broker = lock_broker(&self.broker)?;
        let (instance_id, revision, epoch) = broker
            .channel_coordinates(&self.channel)
            .map_err(permission_error)?;
        if authority.instance_id != instance_id
            || authority.revision != revision
            || authority.authorization_epoch != epoch
        {
            return Err(AnimationError::PermissionDenied(
                "replay original activation changed".into(),
            ));
        }
        for item in lineage {
            let grant = broker
                .current_grant(&self.channel, &item.request_id)
                .map_err(permission_error)?;
            if !item.matches_grant(&grant)? {
                return Err(AnimationError::PermissionDenied(
                    "recorded replay grant scope changed".into(),
                ));
            }
        }
        Ok(())
    }
    fn validate_flushed(
        &self,
        authority: &ReplayAuthority,
        proof: &ReplayFlushedProof,
    ) -> Result<()> {
        if authority != &self.expected || !proof.belongs_to(&self.broker, authority) {
            return Err(AnimationError::PermissionDenied(
                "flushed replay proof belongs to another original broker or plan".into(),
            ));
        }
        Ok(())
    }
}

pub struct InstancePreparation<'a> {
    pub archive: &'a [u8],
    pub verifier: &'a TrustVerifier,
    pub helper_executable: &'a Path,
    pub trusted_bootstrap: &'a str,
    pub settings: &'a Value,
    pub mode: AnimationMode,
    pub environment: &'a Value,
    pub host_policy: Ceiling,
    pub instance_id: u64,
    pub limits: HelperLimits,
    pub quota: QuotaGroup,
}

/// Admitted immutable native preparation, before helper launch and ledger restore.
/// This owner retains the SAME parsed Package and a bounded copy of its archive.
pub struct VerifiedPreparation {
    archive: Vec<u8>,
    helper_executable: PathBuf,
    trusted_bootstrap: String,
    environment: Value,
    package: Arc<Package>,
    identity: VerifiedIdentity,
    broker: PermissionBroker,
    settings: Value,
    mode: AnimationMode,
    instance_id: u64,
    limits: HelperLimits,
    quota: QuotaGroup,
    _storage: StorageAdmission,
}

/// Genuine native pending activation; not Clone, Deserialize or constructible.
/// The trusted host controller retains exclusive custody through real durable IO.
/// No copied hash, boolean, JSON or operation ID can construct this token.
pub struct PendingResolution {
    issuer: Arc<()>,
    instance_id: u64,
    plan_revision: u64,
    epoch: u64,
    activation: Option<Activation>,
    invalidation: Invalidation,
    denied_required: Vec<String>,
    candidate: Option<Vec<u8>>,
    persistence_error: Option<AnimationError>,
    digest: Option<[u8; 32]>,
    _storage: StorageAdmission,
}
impl PendingResolution {
    pub fn invalidation(&self) -> &Invalidation {
        &self.invalidation
    }
    pub fn denied_required(&self) -> &[String] {
        &self.denied_required
    }
    pub fn remembered_bytes(&self) -> Option<&[u8]> {
        self.candidate.as_deref()
    }
    pub fn persistence_error(&self) -> Option<&AnimationError> {
        self.persistence_error.as_ref()
    }
    pub fn remembered_digest(&self) -> Option<[u8; 32]> {
        self.digest
    }
    pub fn authorization_epoch(&self) -> u64 {
        self.epoch
    }
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }
    pub fn plan_revision(&self) -> u64 {
        self.plan_revision
    }
}
impl VerifiedPreparation {
    pub fn principal(&self) -> &crate::permissions::PackageIdentity {
        self.broker.identity()
    }
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }
    pub fn authorization_epoch(&self) -> u64 {
        self.broker.authorization_epoch()
    }
    pub fn package(&self) -> &Package {
        &self.package
    }
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.quota.shares_root(quota)
    }

    /// Only for packages declaring ZERO rights: no remembered right can grant
    /// anything. Rights-bearing packages must load a native namespace first.
    pub fn prepare_without_rights(self) -> Result<(PackageInstance, PlanReview)> {
        if !self.package.manifest().capabilities.is_empty() {
            return Err(AnimationError::PermissionDenied(
                "native permission ledger load required".into(),
            ));
        }
        self.prepare(None)
    }
    /// Some is a current admitted native ledger; None only follows a genuine
    /// current missing-state load (apart from the restricted zero-right wrapper).
    pub fn prepare(self, remembered: Option<&[u8]>) -> Result<(PackageInstance, PlanReview)> {
        let Self {
            archive,
            helper_executable,
            trusted_bootstrap,
            environment,
            package,
            identity,
            mut broker,
            settings,
            mode,
            instance_id,
            limits,
            quota,
            _storage: storage,
        } = self;
        // Consume restore BEFORE launching a helper or creating a review. Failure
        // drops this unused broker; there is no empty-ledger retry or live rollback.
        if let Some(bytes) = remembered {
            broker = broker.restore_remembered(bytes).map_err(permission_error)?;
        }
        // Transfer the ORIGINAL admission before first sharing: audio/source
        // factory clones may outlive PackageInstance and retain broker metadata.
        broker
            .retain_authority_storage(storage)
            .map_err(permission_error)?;
        let mut ambient_digest = Sha256::new();
        ambient_digest.update(b"ilium-v8-replay-ambient-v1");
        ambient_digest.update(package.digest().as_bytes());
        ambient_digest.update(serde_json::to_vec(&settings)?);
        let ambient_digest: [u8; 32] = ambient_digest.finalize().into();
        let ambient_seed = u32::from_le_bytes([
            ambient_digest[0],
            ambient_digest[1],
            ambient_digest[2],
            ambient_digest[3],
        ]);
        let ambient_bootstrap_digest: [u8; 32] =
            Sha256::digest(trusted_bootstrap.as_bytes()).into();
        let environment_digest: [u8; 32] = Sha256::digest(serde_json::to_vec(&environment)?).into();
        let mut helper = HelperSession::launch(
            &helper_executable,
            &archive,
            &trusted_bootstrap,
            HelperAuthority {
                package_digest: identity.digest().into(),
                instance_id,
                plan_generation: 1,
                authorization_epoch: broker.authorization_epoch(),
            },
            limits,
            quota.clone(),
            HelperPlayback {
                mode: mode.clone(),
                ambient_seed,
            },
        )?;
        let helper_build_digest = helper.build_digest();
        let raw = helper.plan(&settings, mode.clone(), &environment)?;
        let mut plan_budget = PlanBudget::default();
        plan_budget.max_clip_seconds = plan_budget
            .max_clip_seconds
            .min(package.manifest().limits.clip_seconds as f64);
        let plan = AnimationPlan::parse(&raw, mode.clone(), plan_budget)?;
        let projection = AuthorizationProjection::build(package.manifest(), &plan)?;
        let review = broker
            .prepare(
                instance_id,
                1,
                projection.permission_plan.clone(),
                BTreeMap::new(),
            )
            .map_err(permission_error)?;
        let selected_storage_metadata =
            quota.reserve_external_storage(64 * 512).map_err(|error| {
                AnimationError::Budget(format!("selected storage registry: {error:?}"))
            })?;
        Ok((
            PackageInstance {
                helper,
                quota,
                pending_resolution: None,
                helper_retired: false,
                replay_retired_proof: Arc::new(AtomicBool::new(false)),
                broker: Arc::new(Mutex::new(broker)),
                identity,
                package,
                mode,
                ambient_seed,
                ambient_bootstrap_digest,
                environment_digest,
                helper_build_digest,
                replay_request_count: 0,
                replay_pure_yield_count: 0,
                replay_completed_yield_count: 0,
                replay_video_open_count: 0,
                replay_source_open_count: 0,
                replay_source_freeze_count: 0,
                replay_source_sequence_count: 0,
                plan,
                projection,
                selected_storage: BTreeMap::new(),
                _selected_storage_metadata: selected_storage_metadata,
                activation: None,
                creation: None,
                settings,
            },
            review,
        ))
    }
}

// Allocation-free preflight before cloning host environment into a suspended owner.
fn environment_bytes(value: &Value, depth: usize) -> Result<usize> {
    if depth > 32 {
        return Err(AnimationError::Budget(
            "preparation environment depth".into(),
        ));
    }
    let mut bytes = 64usize;
    match value {
        Value::String(text) => bytes = bytes.saturating_add(text.len()),
        Value::Array(values) => {
            for value in values {
                bytes = bytes.saturating_add(environment_bytes(value, depth + 1)?);
                if bytes > 256 * 1024 {
                    return Err(AnimationError::Budget(
                        "preparation environment size".into(),
                    ));
                }
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                bytes = bytes
                    .saturating_add(1024 + key.len())
                    .saturating_add(environment_bytes(value, depth + 1)?);
                if bytes > 256 * 1024 {
                    return Err(AnimationError::Budget(
                        "preparation environment size".into(),
                    ));
                }
            }
        }
        _ => {}
    }
    if bytes > 256 * 1024 {
        return Err(AnimationError::Budget(
            "preparation environment size".into(),
        ));
    }
    Ok(bytes)
}

pub struct StoppedInstance {
    pub invalidation: Option<Invalidation>,
    pub cancellation: Result<()>,
    pub authority_error: Option<AnimationError>, // Poisoning never fabricates retirement or clears the ledger.
}

pub struct InstanceResolution {
    pub invalidation: Invalidation,
    pub denied_required: Vec<String>,
    pub creation: Option<CreateState>,
    pub teardown_error: Option<AnimationError>,
    pub activation_invalidation: Option<Invalidation>, // Return additional cleanup if accepted activation fails during native startup.
    pub authority_error: Option<AnimationError>, // Independent inability to retire poisoned native authority.
    pub creation_error: Option<AnimationError>, // Keep the original startup failure distinct from physical teardown failure.
} // Preserve consent, activation cleanup, startup failure, and physical retirement as independent results.
impl InstanceResolution {
    /// A native creation may still be awaiting real asynchronous services. Only
    /// an accepted resolution with no startup, authority or teardown failure
    /// permits exposing the instance; Pending never implies animation readiness.
    pub fn accepted_creation(&self) -> Option<CreateState> {
        if !self.denied_required.is_empty()
            || self.creation_error.is_some()
            || self.authority_error.is_some()
            || self.teardown_error.is_some()
            || self.activation_invalidation.is_some()
        {
            None
        } else {
            self.creation
        }
    }
}

pub(crate) struct ServiceOperation {
    // The native dispatcher retains this noncloneable request and ticket association until actual work terminates.
    request: HostRequest, // Keep the exact admitted input and native correlation stamp alive with the operation.
    ticket: Arc<OperationTicket>, // SAME original ticket shared with native IO, never reconstruct native authority from an exposed operation identifier.
} // This wrapper does not implement any service or mint a service handle.
impl ServiceOperation {
    // Expose immutable input to concrete native service adapters in this crate.
    pub(crate) fn request(&self) -> &HostRequest {
        &self.request
    } // Borrow immutable binary planes without separating their original-root admission.
} // Cancellation may signal request().stop_token(), but actual job ownership still determines terminal settlement.

fn original_service_group_request<'a>(
    operations: &[&'a ServiceOperation],
) -> Result<&'a HostRequest> {
    let first = operations
        .first()
        .copied()
        .filter(|_| operations.len() <= 8)
        .ok_or_else(|| AnimationError::Runtime("service dependency count must be 1..8".into()))?;
    if operations
        .iter()
        .any(|operation| !std::ptr::eq(&*operation.request, &*first.request))
    {
        return Err(AnimationError::PermissionDenied(
            "service dependencies do not retain the original request".into(),
        ));
    }
    Ok(&first.request)
}

// Observations describe the broker's current actual grant, never the requested
// ceiling. A native binding identity is not a registered guest AssetHandle and
// must not be serialized as one; the asset owner separately publishes that handle.
fn permission_observation(grant: &crate::permissions::Grant, epoch: u64) -> Result<Value> {
    if epoch == 0 || epoch > 9_007_199_254_740_991 {
        return Err(AnimationError::PermissionDenied(
            "native permission observation epoch is outside the SDK range".into(),
        ));
    }
    Ok(serde_json::json!({
        "request_id": grant.request_id,
        "id": grant.right.id,
        "scope": grant.right.scope,
        "epoch": epoch,
    }))
}
#[cfg(all(feature = "native-host", feature = "native-network"))]
#[derive(Clone)]
pub(crate) struct SourceFeedOperation {
    ticket: Arc<OperationTicket>,
    broker: Arc<Mutex<PermissionBroker>>,
    channel: Channel,
    quota: QuotaGroup,
    stop: StopToken,
    deadline: Instant,
}
#[cfg(all(feature = "native-host", feature = "native-network"))]
impl SourceFeedOperation {
    fn is_stopped_or_expired(&self) -> bool {
        self.stop.is_stopped() || Instant::now() >= self.deadline
    }
}

fn permission_error(error: crate::permissions::PermissionError) -> AnimationError {
    AnimationError::PermissionDenied(error.to_string())
}

fn lock_broker(owner: &Mutex<PermissionBroker>) -> Result<MutexGuard<'_, PermissionBroker>> {
    owner.lock().map_err(|_| {
        AnimationError::PermissionDenied(
            "native permission authority poisoned; activation refused".into(),
        )
    })
}
fn authority_from(broker: &PermissionBroker, active: &Activation) -> Result<ServiceAuthority> {
    broker
        .grant(&active.channel, "_host_channel_check")
        .map_err(permission_error)?;
    Ok(ServiceAuthority {
        instance_id: active.plan.instance_id,
        plan_generation: active.plan.plan_revision,
        authorization_epoch: active.plan.authorization_epoch,
    })
}

fn validate_baseline_request(request: &HostRequest) -> Result<()> {
    if request.is_cancelled()
        || !matches!(
            request.method.as_str(),
            "compute.submit" | "compute.result" | "compute.status" | "compute.close"
        )
    {
        return Err(AnimationError::PermissionDenied(
            "invalid baseline math operation".into(),
        ));
    }
    Ok(())
}
fn validate_media_request(request: &HostRequest) -> Result<()> {
    if request.is_cancelled()
        || !matches!(
            request.method.as_str(),
            "media.images.decode"
                | "media.images.resize"
                | "media.images.sample"
                | "media.images.close"
                | "media.video.open"
                | "media.video.pause"
                | "media.video.seek"
                | "media.video.close"
                | "gpu.render"
        )
    {
        return Err(AnimationError::PermissionDenied(
            "invalid baseline media operation".into(),
        ));
    }
    Ok(())
}
fn validate_request(
    digest: &str,
    request: &HostRequest,
    authority: ServiceAuthority,
) -> Result<ServiceAuthority> {
    if request.package_digest != digest || request.authority != authority {
        return Err(AnimationError::PermissionDenied(
            "service request does not match current native activation".into(),
        ));
    }
    Ok(authority)
}

fn constrained_limits(
    manifest: &crate::manifest::RuntimeLimits,
    mut host: HelperLimits,
) -> Result<HelperLimits> {
    let bytes = |value| {
        usize::try_from(value).map_err(|_| AnimationError::Budget("manifest resource size".into()))
    };
    host.engine.heap_bytes = host.engine.heap_bytes.min(bytes(manifest.heap_bytes)?);
    host.engine.backing_bytes = host.engine.backing_bytes.min(bytes(manifest.heap_bytes)?);
    host.engine.frame_bytes = host.engine.frame_bytes.min(bytes(manifest.frame_bytes)?);
    host.engine.render_ms = host.engine.render_ms.min(manifest.render_ms);
    host.engine.preparation_ms = host.engine.preparation_ms.min(manifest.preparation_ms);
    Ok(host)
}

impl PackageInstance {
    /// Original quota identity; copied settings or metadata cannot create it.
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.quota.shares_root(quota)
    }
    /// Native caller scratch follows the same original instance root.
    pub fn reserve_input_seed_storage(&self, bytes: usize) -> Result<StorageAdmission> {
        self.live_helper_authority()?;
        let maximum = self
            .engine_limits()
            .frame_bytes
            .checked_add(128 * 1024)
            .ok_or_else(|| AnimationError::Budget("input seed capacity overflow".into()))?;
        if bytes > maximum {
            return Err(AnimationError::Budget(
                "input seed copy exceeds original capacity".into(),
            ));
        }
        self.quota
            .reserve_external_storage(bytes)
            .map_err(|error| AnimationError::Budget(format!("input seed copy: {error:?}")))
    }
    pub fn settings(&self) -> &Value {
        &self.settings
    }

    /// Fresh live helper authority for input preparation in either animation mode.
    /// This check does not authorize acquisition or a later copy/publication.
    pub fn check_live_input_authority(&self, expected: &HelperAuthority) -> Result<()> {
        let active = self.live_helper_authority()?;
        let current = HelperAuthority {
            package_digest: self.package.digest().into(),
            instance_id: active.instance_id,
            plan_generation: active.plan_generation,
            authorization_epoch: active.authorization_epoch,
        };
        if &current != expected {
            return Err(AnimationError::PermissionDenied(
                "input owner activation changed".into(),
            ));
        }
        Ok(())
    }

    /// Inspect native pending review coordinates while the original broker is held.
    /// The bounded native closure must not execute JS, perform IO, wait for jobs,
    /// acquire services, or call back into this broker.
    pub fn with_permission_review_state<T>(
        &self,
        instance_id: u64,
        inspect: impl FnOnce(&Package, &crate::permissions::PackageIdentity, u64, u64, u64) -> T,
    ) -> Result<T> {
        if instance_id != self.helper.authority().instance_id
            || self.activation.is_some()
            || self.helper_retired
            || self.pending_resolution.is_some()
        {
            return Err(AnimationError::PermissionDenied(
                "review owner unavailable".into(),
            ));
        }
        let broker = lock_broker(&self.broker)?;
        let (revision, epoch) = broker
            .pending_review_coordinates(instance_id)
            .map_err(permission_error)?;
        Ok(inspect(
            &self.package,
            broker.identity(),
            instance_id,
            revision,
            epoch,
        ))
    }
    /// Retain exact verified native identity before any helper or permission review.
    pub fn verify(preparation: InstancePreparation<'_>) -> Result<VerifiedPreparation> {
        let InstancePreparation {
            archive,
            verifier,
            helper_executable,
            trusted_bootstrap,
            settings,
            mode,
            environment,
            host_policy,
            instance_id,
            limits,
            quota,
        } = preparation;
        if instance_id == 0
            || trusted_bootstrap.len() > 1024 * 1024
            || helper_executable.as_os_str().len() > 16 * 1024
        {
            return Err(AnimationError::Budget(
                "native preparation identity or metadata".into(),
            ));
        }
        environment_bytes(environment, 0)?;
        let package_limits = PackageLimits::default();
        if archive.len() as u64 > package_limits.archive_bytes {
            return Err(AnimationError::Budget("archive size".into()));
        }
        let expanded = usize::try_from(package_limits.expanded_bytes)
            .map_err(|_| AnimationError::Budget("package expansion size".into()))?;
        // Archive/path/bootstrap/environment copies and retained package have an
        // original-root debit BEFORE allocation, including suspended ledger load.
        let bytes = expanded
            .checked_add(archive.len())
            .and_then(|value| value.checked_add(2 * 1024 * 1024))
            .ok_or_else(|| AnimationError::Budget("native preparation storage overflow".into()))?;
        let storage = quota
            .reserve_external_storage(bytes)
            .map_err(|error| AnimationError::Budget(format!("package admission: {error:?}")))?;
        let package = Arc::new(Package::from_bytes(archive, package_limits)?);
        let limits = constrained_limits(&package.manifest().limits, limits)?;
        if !package.manifest().modes.contains(&mode) {
            return Err(AnimationError::Runtime(
                "package does not support this mode".into(),
            ));
        }
        let settings = crate::settings::validate_settings(&package.manifest().settings, settings)?;
        let identity = verifier.verify(&package);
        let principal = verifier.permission_identity(&package)?;
        let manifest = Ceiling {
            permissions: package
                .manifest()
                .capabilities
                .iter()
                .map(crate::permission_projection::right)
                .collect::<Result<Vec<_>>>()?,
        };
        let broker =
            PermissionBroker::new(principal, manifest, host_policy).map_err(permission_error)?;
        Ok(VerifiedPreparation {
            archive: archive.to_vec(),
            helper_executable: helper_executable.to_owned(),
            trusted_bootstrap: trusted_bootstrap.to_owned(),
            environment: environment.clone(),
            package,
            identity,
            broker,
            settings,
            mode,
            instance_id,
            limits,
            quota,
            _storage: storage,
        })
    }

    /// A picker changes the reviewed binding, so it always consumes a fresh
    /// review revision rather than modifying a grant returned to the script.
    pub fn review_selected(
        &mut self,
        instance_id: u64,
        revision: u64,
        bindings: BTreeMap<String, HostBinding>,
    ) -> Result<PlanReview> {
        if instance_id != self.helper.authority().instance_id
            || self.activation.is_some()
            || self.helper_retired
            || self.pending_resolution.is_some()
        {
            // A logically retired helper cannot consume a new native review.
            return Err(AnimationError::PermissionDenied(
                "selection review does not match the inactive package instance".into(),
            ));
        }
        lock_broker(&self.broker)?
            .prepare(
                instance_id,
                revision,
                self.projection.permission_plan.clone(),
                bindings,
            )
            .map_err(permission_error)
    }
    /// Re-review only actual picker-pinned resources. Every binding is derived
    /// from an original native file/directory handle and its exact requested
    /// right; guest-supplied IDs or paths never enter the broker binding table.
    pub fn review_selected_resources(
        &mut self,
        instance_id: u64,
        revision: u64,
        resources: BTreeMap<String, Arc<crate::native_storage::SelectedStorage>>,
        audio: BTreeMap<String, crate::native_audio_capture::QualifiedCaptureBinding>,
    ) -> Result<PlanReview> {
        if resources.len() + audio.len() > 64 {
            return Err(AnimationError::Budget("selected resource count".into()));
        }
        let mut bindings = BTreeMap::new();
        for (request_id, resource) in &resources {
            let request = self
                .projection
                .permission_plan
                .permissions
                .iter()
                .find(|request| request.request_id.as_deref() == Some(request_id))
                .ok_or_else(|| {
                    AnimationError::PermissionDenied("selected request identity missing".into())
                })?;
            if !resource.matches_right(&crate::permissions::Right {
                id: request.id,
                scope: request.scope.clone(),
            }) {
                return Err(AnimationError::PermissionDenied(
                    "selected resource differs from reviewed right".into(),
                ));
            }
            bindings.insert(request_id.clone(), resource.binding().clone());
        }
        for (request_id, selected) in &audio {
            let request = self
                .projection
                .permission_plan
                .permissions
                .iter()
                .find(|request| request.request_id.as_deref() == Some(request_id))
                .ok_or_else(|| {
                    AnimationError::PermissionDenied(
                        "selected audio request identity missing".into(),
                    )
                })?;
            if !selected.matches_right(&crate::permissions::Right {
                id: request.id,
                scope: request.scope.clone(),
            }) || bindings.contains_key(request_id)
            {
                return Err(AnimationError::PermissionDenied(
                    "selected audio differs from reviewed right".into(),
                ));
            }
            bindings.insert(request_id.clone(), selected.binding().clone());
        }
        let review = self.review_selected(instance_id, revision, bindings)?;
        self.selected_storage = resources;
        Ok(review)
    }
    pub(crate) fn selected_storage(
        &self,
    ) -> &BTreeMap<String, Arc<crate::native_storage::SelectedStorage>> {
        &self.selected_storage
    }
    pub(crate) fn selected_asset_id(request_id: &str) -> String {
        format!("selected-{:x}", Sha256::digest(request_id.as_bytes()))
    }

    /// Resolve genuine review into privately held authority, without create,
    /// host seeding, dispatch, or frame publication. Caller handles invalidation.
    pub fn begin_resolution(
        &mut self,
        review: PlanReview,
        answers: BTreeMap<String, UserChoice>,
    ) -> Result<PendingResolution> {
        if self.activation.is_some() || self.helper_retired || self.pending_resolution.is_some() {
            return Err(AnimationError::PermissionDenied(
                "resolution requires an inactive native owner".into(),
            ));
        }
        let instance_id = review.instance_id();
        let plan_revision = review.plan_revision();
        let remembered = answers.values().any(|choice| {
            matches!(
                choice,
                UserChoice::AllowRemembered | UserChoice::DenyRemembered
            )
        });
        let storage = self
            .quota
            .reserve_external_storage(384 * 1024)
            .map_err(|error| {
                AnimationError::Budget(format!("pending permission admission: {error:?}"))
            })?;
        let mut broker = lock_broker(&self.broker)?;
        let resolution = broker.resolve(review, answers).map_err(permission_error)?;
        let issuer = Arc::new(());
        self.pending_resolution = Some(Arc::clone(&issuer));
        let (candidate, persistence_error) = if remembered {
            match broker.export_remembered() {
                Ok(bytes) => (Some(bytes), None),
                Err(error) => (None, Some(permission_error(error))),
            }
        } else {
            (None, None)
        };
        let epoch = broker.authorization_epoch();
        drop(broker);
        // Even failed export retains the original consent invalidation in this
        // genuine token; caller receives cleanup instead of a lost Error path.
        let digest = candidate.as_ref().map(|bytes| Sha256::digest(bytes).into());
        Ok(PendingResolution {
            issuer,
            instance_id,
            plan_revision,
            epoch,
            activation: resolution.activation,
            invalidation: resolution.invalidation,
            denied_required: resolution.denied_required,
            candidate,
            persistence_error,
            digest,
            _storage: storage,
        })
    }

    /// Trusted host-only continuation. The controller keeps this noncloneable
    /// token private and consumes it ONLY after checking its actual native durable
    /// receipt (if bytes exist). No helper/JS callback may expose this method.
    pub fn finish_resolution(&mut self, pending: PendingResolution) -> Result<InstanceResolution> {
        if pending.persistence_error.is_some()
            || self.helper_retired
            || self.activation.is_some()
            || lock_broker(&self.broker)?.authorization_epoch() != pending.epoch
            || !self
                .pending_resolution
                .as_ref()
                .is_some_and(|issuer| Arc::ptr_eq(issuer, &pending.issuer))
        {
            return Err(AnimationError::PermissionDenied(
                "stale pending native resolution".into(),
            ));
        }
        self.pending_resolution = None;
        // Retain candidate/guard through the complete startup/denial result.
        let resolution = crate::permissions::Resolution {
            activation: pending.activation,
            invalidation: pending.invalidation,
            denied_required: pending.denied_required,
        };
        let Some(activation) = resolution.activation else {
            // Required denial must never leave the helper able to create an instance.
            self.creation = None; // There is no accepted script instance.
            self.activation = None; // The broker already invalidated any previous native activation.
            let teardown_error = self.retire_helper().err(); // Report physical retirement failure without claiming that the helper exited.
            return Ok(InstanceResolution {
                invalidation: resolution.invalidation,
                denied_required: resolution.denied_required,
                creation: None,
                teardown_error,
                activation_invalidation: None,
                authority_error: None,
                creation_error: None,
            }); // Return every consent and teardown obligation together.
        }; // An accepted native channel is now owned by this activation.
        self.activation = Some(activation); // Store native authority before every fallible projection, seed, or create operation.
        let creation = (|| {
            let owner = Arc::clone(&self.broker);
            let broker = lock_broker(&owner)?;
            let active = self.activation.as_ref().ok_or_else(|| {
                AnimationError::PermissionDenied("native activation missing".into())
            })?;
            let authority = authority_from(&broker, active)?;
            self.plan = self
                .projection
                .prune_inputs(&broker, &active.channel, &self.plan)?;
            let mut accepted = serde_json::to_value(&self.plan)?;
            let mut accepted_requests = Vec::new();
            let mut observed_grants = Vec::new();
            for request in &self.projection.permission_plan.permissions {
                let id = request.request_id.as_deref().ok_or_else(|| {
                    AnimationError::PermissionDenied("native request identity missing".into())
                })?;
                if let Some(grant) = broker
                    .grant(&active.channel, id)
                    .map_err(permission_error)?
                {
                    accepted_requests.push(request);
                    let mut observation =
                        permission_observation(grant, active.plan.authorization_epoch)?;
                    if let Some(selected) = self.selected_storage.get(id) {
                        if grant.binding.as_ref() != Some(selected.binding()) {
                            return Err(AnimationError::PermissionDenied(
                                "selected grant lost original pinned binding".into(),
                            ));
                        }
                        observation["handle"] = serde_json::json!({
                            "id": Self::selected_asset_id(id),
                            "kind": "asset",
                        });
                    }
                    observed_grants.push(observation);
                }
            }
            accepted["permissions"] = serde_json::to_value(&accepted_requests)?;
            accepted["generation"] = Value::from(active.plan.plan_revision);
            accepted["authorization_epoch"] = Value::from(active.plan.authorization_epoch);
            drop(broker);
            // Recheck SAME owner through inert binding ACK. Deferred retirement
            // occurs only after callback-local broker guard has dropped.
            self.helper.with_native_publication(|helper| {
                let broker = lock_broker(&owner)?;
                authority_from(&broker, active)?;
                helper.bind_service_authority(authority)
            })?;
            self.live_helper_authority()?;
            let bundle_id = self.bundle_asset_id();
            let seed = self.helper.with_native_publication(|helper| {
                let broker = lock_broker(&owner)?;
                authority_from(&broker, active)?;
                helper.prepare_frame_seed(&serde_json::json!({"frame": null, "host": {"permissions": observed_grants, "cancelled": false, "package": {"id": self.identity.id(), "version": self.package.manifest().version, "digest": self.identity.digest(), "verified_ilium": self.identity.is_ilium()}, "bundle": {"id": bundle_id, "kind": "asset"}, "selection": {"generation": active.plan.plan_revision, "authorization_epoch": active.plan.authorization_epoch}}}), &[], &BTreeMap::new())
            })?;
            self.activate_frame_seed(seed)?;
            // JS execution is outside the serialized native authority. All resulting
            // effects/requests/publication still independently require current gates.
            self.live_helper_authority()?;
            self.helper.start_create(&self.settings, &accepted)
        })();
        let creation = match creation {
            // Preserve both consent and cleanup inventories on partial startup failure.
            Ok(creation) => creation, // A successfully started instance can now be pumped under its private channel.
            Err(error) => {
                // Native acceptance may have succeeded even when the child could not finish startup.
                let (activation_invalidation, authority_error) = match self.revoke_activation() {
                    Ok(invalidation) => (invalidation, None),
                    Err(error) => (None, Some(error)),
                }; // Retire the exact stored native activation before returning startup failure.
                let teardown_error = self.retire_helper().err(); // Independently require physical helper and pipe-worker retirement.
                return Ok(InstanceResolution {
                    invalidation: resolution.invalidation,
                    denied_required: resolution.denied_required,
                    creation: None,
                    teardown_error,
                    activation_invalidation,
                    authority_error,
                    creation_error: Some(error),
                }); // Caller must process both invalidations and report both independent failures.
            } // Duplicate operation IDs across invalidations denote the same native cleanup obligation.
        }; // Native activation remains current only after startup succeeded.
        self.creation = Some(creation); // Retain the actual child creation state.
        Ok(InstanceResolution {
            invalidation: resolution.invalidation,
            denied_required: resolution.denied_required,
            creation: Some(creation),
            teardown_error: None,
            activation_invalidation: None,
            authority_error: None,
            creation_error: None,
        }) // Startup success does not claim a service dispatcher exists.
    } // End the native activation transaction.
    fn current_service_authority(&self) -> Result<ServiceAuthority> {
        let broker = lock_broker(&self.broker)?;
        self.current_service_authority_locked(&broker)
    }
    fn current_service_authority_locked(
        &self,
        broker: &PermissionBroker,
    ) -> Result<ServiceAuthority> {
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("instance is not accepted".into()))?;
        authority_from(broker, active)
    }
    /// Existing root math operation gate: bounded issue under SAME authority,
    /// no callback may recursively lock owner/run JS/pump/wait for worker exit.
    pub(crate) fn with_baseline_math<T>(
        &mut self,
        request: &HostRequest,
        issue: impl FnOnce(ServiceAuthority) -> Result<T>,
    ) -> Result<T> {
        if self.helper_retired {
            return Err(AnimationError::PermissionDenied("helper retired".into()));
        }
        let owner = Arc::clone(&self.broker);
        let broker = lock_broker(&owner)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)?;
        validate_baseline_request(request)?;
        issue(authority)
    }
    /// Baseline image and video controls use the already supervised animation worker. This
    /// brief check cannot authorize selected disk, network, source imagery, or a new
    /// worker; the native registry and final copy ACK are fenced separately.
    pub(crate) fn check_baseline_media(&self, request: &HostRequest) -> Result<()> {
        if self.helper_retired || !request.payload.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "image owner retired or foreign".into(),
            ));
        }
        let broker = lock_broker(&self.broker)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)?;
        validate_media_request(request)
    }

    /// GPU rendering is a privileged bounded operation even though this
    /// backend currently rasterizes on the CPU. Require an accepted exact
    /// `device.gpu` grant for the mesh kernel before allocating any pixels.
    pub(crate) fn check_gpu_permission(&self) -> Result<()> {
        let owner = Arc::clone(&self.broker);
        let broker = lock_broker(&owner)?;
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        for permission in &self.projection.permission_plan.permissions {
            if permission.id != crate::permissions::Capability::DeviceGpu {
                continue;
            }
            let crate::permissions::Scope::Gpu { kernels } = &permission.scope else {
                continue;
            };
            if !kernels.iter().any(|kernel| kernel == "mesh") {
                continue;
            }
            let Some(request_id) = permission.request_id.as_deref() else {
                continue;
            };
            if !active
                .plan
                .demands
                .contains_key(&crate::plan_authorization::operation_demand(request_id))
            {
                continue;
            }
            if broker
                .grant(&active.channel, request_id)
                .map_err(permission_error)?
                .is_some()
            {
                return Ok(());
            }
        }
        Err(AnimationError::PermissionDenied(
            "no accepted device.gpu mesh grant".into(),
        ))
    }
    /// Bounded registry insertion only: no decode, image destruction, I/O,
    /// JavaScript or worker wait is permitted inside the broker mutex.
    pub(crate) fn with_baseline_media_registry<T>(
        &self,
        request: &HostRequest,
        insert: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.check_baseline_media(request)?;
        let owner = Arc::clone(&self.broker);
        let broker = lock_broker(&owner)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)?;
        insert()
    }
    /// The same retained request and exact immutable result pass the helper's
    /// inert native copy/ACK before any later package continuation executes.
    pub(crate) fn complete_baseline_media(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        self.check_baseline_media(request)?;
        if !value.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "image result belongs to a different quota root".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let digest = self.package.digest();
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            helper.complete_service_request(request.id, authority, value)
        })
    }
    /// The immutable package bundle and instance-local cache are baseline
    /// services. A selected disk or persistent-state operation must instead
    /// carry its exact original broker ticket and pinned native resource.
    pub(crate) fn check_baseline_storage(&self, request: &HostRequest) -> Result<()> {
        if self.helper_retired || !request.payload.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "storage owner retired or foreign".into(),
            ));
        }
        let broker = lock_broker(&self.broker)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)?;
        if request.is_cancelled()
            || !matches!(
                request.method.as_str(),
                "assets.read"
                    | "assets.list"
                    | "assets.write"
                    | "cache.get"
                    | "cache.put"
                    | "cache.remove"
            )
        {
            return Err(AnimationError::PermissionDenied(
                "invalid baseline storage operation".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn complete_baseline_storage(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        self.check_baseline_storage(request)?;
        if !value.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "storage result belongs to a different quota root".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let digest = self.package.digest();
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            helper.complete_service_request(request.id, authority, value)
        })
    }
    /// The package and broker are both original-instance owners; this
    /// identity may initialize a cache namespace but cannot authorize I/O.
    pub(crate) fn storage_principal(&self) -> Result<crate::permissions::PackageIdentity> {
        self.live_helper_authority()?;
        Ok(lock_broker(&self.broker)?.identity().clone())
    }
    pub(crate) fn state_persist_request_id(&self) -> Result<Option<String>> {
        self.live_helper_authority()?;
        let active = self.activation.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("persistent state activation missing".into())
        })?;
        let request_id = self.projection.permission_plan.permissions.iter()
            .find(|request| request.id == crate::permissions::Capability::StatePersist
                && matches!(&request.scope, crate::permissions::Scope::Namespace { name } if name == "session"))
            .and_then(|request| request.request_id.as_ref());
        let Some(request_id) = request_id else {
            return Ok(None);
        };
        if lock_broker(&self.broker)?
            .grant(&active.channel, request_id)
            .map_err(permission_error)?
            .is_some()
        {
            Ok(Some(request_id.clone()))
        } else {
            Ok(None)
        }
    }
    pub(crate) fn bundle_asset_id(&self) -> String {
        format!("bundle-{}", self.package.digest())
    }
    /// Native service owners may install an already admitted prepared resource
    /// while creation is active in either mode. This is not a frame emission
    /// and may perform only a bounded registry insertion under the broker guard.
    pub(crate) fn with_resource_registry_authority<T>(
        &self,
        insert: impl FnOnce() -> T,
    ) -> Result<T> {
        self.live_helper_authority()?;
        let broker = lock_broker(&self.broker)?;
        self.current_service_authority_locked(&broker)?;
        Ok(insert())
    }
    /// Original native receipt polling only, including after helper retirement.
    /// No request/effect/service acquisition is permitted in this callback.
    pub(crate) fn with_native_math_authority<T>(
        &mut self,
        poll: impl FnOnce(ServiceAuthority) -> Result<T>,
    ) -> Result<T> {
        let owner = Arc::clone(&self.broker);
        let broker = lock_broker(&owner)?;
        let authority = self.current_service_authority_locked(&broker)?;
        poll(authority)
    }
    pub(crate) fn complete_baseline_math(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        if self.helper_retired {
            return Err(AnimationError::PermissionDenied("helper retired".into()));
        }
        validate_baseline_request(request)?;
        let owner = Arc::clone(&self.broker);
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let digest = self.package.digest();
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            helper.complete_service_request(request.id, authority, value)
        })
    }
    /// Tasks are nonprivileged execution control. Authenticate the original
    /// activation channel without manufacturing an empty-needs operation ticket.
    /// In replay, only an empty yield issued while create is pending is pure.
    pub(crate) fn check_native_task_request(
        &self,
        request: &HostRequest,
    ) -> Result<ServiceAuthority> {
        if !matches!(
            request.method.as_str(),
            "tasks.yield" | "tasks.poll.open" | "tasks.poll.next" | "tasks.poll.close"
        ) || !request.payload.shares_root(&self.quota)
            || request.is_cancelled()
        {
            return Err(AnimationError::PermissionDenied(
                "invalid native task request".into(),
            ));
        }
        if self.mode == AnimationMode::PreRendered
            && (request.method != "tasks.yield"
                || self.creation != Some(CreateState::Pending)
                || request.phase != ServicePhase::Create
                || request.payload.metadata() != &serde_json::json!({})
                || !request.payload.arrays().is_empty()
                || !request.payload.planes().is_empty())
        {
            return Err(AnimationError::PermissionDenied(
                "periodic or post-create tasks require live mode".into(),
            ));
        }
        self.live_helper_authority()?;
        let broker = lock_broker(&self.broker)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)
    }
    /// A bounded registry mutation is serialized with revoke/reconfigure.
    /// The closure cannot run JS, IPC, an effect, or wait for another owner.
    pub(crate) fn with_native_task_registry<T>(
        &self,
        request: &HostRequest,
        mutate: impl FnOnce(ServiceAuthority) -> Result<T>,
    ) -> Result<T> {
        self.check_native_task_request(request)?;
        let broker = lock_broker(&self.broker)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)?;
        mutate(authority)
    }
    pub(crate) fn native_task_authority(&self) -> Result<ServiceAuthority> {
        self.live_helper_authority()
    }
    /// Bounded inert child copy/ACK; the original channel is checked again at
    /// publication. The existing helper guard defers physical teardown until
    /// this copy transaction has finished; no package checkpoint runs here.
    pub(crate) fn complete_native_task(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        self.check_native_task_request(request)?;
        if !value.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "foreign task result root".into(),
            ));
        }
        let replay_yield =
            self.mode == AnimationMode::PreRendered && request.method == "tasks.yield";
        if replay_yield
            && (value.metadata() != &serde_json::json!({"ok":true,"value":null})
                || !value.arrays().is_empty()
                || !value.planes().is_empty()
                || self.replay_completed_yield_count >= self.replay_pure_yield_count)
        {
            return Err(AnimationError::PermissionDenied(
                "impure replay yield result".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("task activation missing".into()))?;
        let digest = self.package.digest();
        let outcome = self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            helper.complete_service_request(request.id, authority, value)
        })?;
        if replay_yield && outcome == CompletionState::Delivered {
            self.replay_completed_yield_count += 1;
        }
        Ok(outcome)
    }
    /// World requests are carried by the original accepted helper, with the
    /// current native activation and one shared quota root. A copied world id
    /// never substitutes for this request authority.
    pub(crate) fn check_native_world_request(
        &self,
        request: &HostRequest,
    ) -> Result<ServiceAuthority> {
        if self.mode != AnimationMode::Live
            || self.helper_retired
            || !matches!(
                request.method.as_str(),
                "worlds.open"
                    | "worlds.frame"
                    | "worlds.close"
                    | "worlds.frame.close"
                    | "worlds.list"
                    | "worlds.region"
                    | "worlds.model"
            )
            || !request.payload.shares_root(&self.quota)
            || request.is_cancelled()
        {
            return Err(AnimationError::PermissionDenied(
                "invalid live world request".into(),
            ));
        }
        self.live_helper_authority()?;
        let broker = lock_broker(&self.broker)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)
    }
    pub(crate) fn native_world_authority(&self) -> Result<ServiceAuthority> {
        if self.mode != AnimationMode::Live {
            return Err(AnimationError::PermissionDenied(
                "world requests require live mode".into(),
            ));
        }
        self.live_helper_authority()
    }
    pub(crate) fn complete_native_world(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        self.check_native_world_request(request)?;
        if !value.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "foreign world result root".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("world activation missing".into()))?;
        let digest = self.package.digest();
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            helper.complete_service_request(request.id, authority, value)
        })
    }
    /// Presentation subscriptions are explicit live observers of native
    /// terminal ACKs, not an implicit render-side service acquisition.
    pub(crate) fn check_native_presentation_request(
        &self,
        request: &HostRequest,
    ) -> Result<ServiceAuthority> {
        if self.mode != AnimationMode::Live
            || self.helper_retired
            || !matches!(
                request.method.as_str(),
                "presentation.subscribe" | "presentation.next" | "presentation.close"
            )
            || !request.payload.shares_root(&self.quota)
            || request.is_cancelled()
        {
            return Err(AnimationError::PermissionDenied(
                "invalid live presentation request".into(),
            ));
        }
        self.live_helper_authority()?;
        let broker = lock_broker(&self.broker)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)
    }
    pub(crate) fn with_native_presentation_registry<T>(
        &self,
        request: &HostRequest,
        mutate: impl FnOnce(ServiceAuthority) -> Result<T>,
    ) -> Result<T> {
        self.check_native_presentation_request(request)?;
        let broker = lock_broker(&self.broker)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)?;
        mutate(authority)
    }
    pub(crate) fn native_presentation_authority(&self) -> Result<ServiceAuthority> {
        if self.mode != AnimationMode::Live {
            return Err(AnimationError::PermissionDenied(
                "presentation requires live mode".into(),
            ));
        }
        self.live_helper_authority()
    }
    pub(crate) fn complete_native_presentation(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        self.check_native_presentation_request(request)?;
        if !value.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "foreign presentation result root".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let active = self.activation.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("presentation activation missing".into())
        })?;
        let digest = self.package.digest();
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            helper.complete_service_request(request.id, authority, value)
        })
    }
    /// Select an exact original accepted audio request after durable resolution.
    /// A script source string, copied accepted-plan JSON, or same-root quota
    /// cannot choose the host device, demand ID, broker channel, or grant epoch.
    #[cfg(feature = "native-host")]
    pub fn selected_audio_capture(
        &self,
        selected: crate::native_audio_capture::QualifiedCaptureBinding,
    ) -> Result<
        Option<(
            crate::native_audio::AudioDemand,
            crate::native_audio::AuthenticatedAudioGrant,
            crate::native_audio_capture::NativeAudioCaptureFactory,
        )>,
    > {
        if self.helper_retired {
            return Err(AnimationError::PermissionDenied("helper retired".into()));
        }
        let Some(plan) = self.plan.inputs.audio.as_ref() else {
            return Ok(None); // Denied/absent demand never opens a device.
        };
        let demand = crate::native_audio::AudioDemand::from_accepted_plan(plan)?;
        let source = crate::native_audio::AudioSourceSelection::from_accepted_plan(plan)?;
        if selected.selector() != &source {
            return Err(AnimationError::PermissionDenied(
                "selected audio source differs from accepted plan".into(),
            ));
        }
        let needed_products = demand
            .products
            .iter()
            .map(|product| match product {
                crate::native_audio::AudioProduct::Level => crate::permissions::AudioProduct::Level,
                crate::native_audio::AudioProduct::Waveform => {
                    crate::permissions::AudioProduct::Waveform
                }
                crate::native_audio::AudioProduct::Envelope => {
                    crate::permissions::AudioProduct::Envelope
                }
                crate::native_audio::AudioProduct::Bands => crate::permissions::AudioProduct::Bands,
                crate::native_audio::AudioProduct::History => {
                    crate::permissions::AudioProduct::History
                }
            })
            .collect::<std::collections::BTreeSet<_>>();
        let broker = lock_broker(&self.broker)?;
        self.current_service_authority_locked(&broker)?;
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let mut selected_request_id = None;
        for request in &self.projection.permission_plan.permissions {
            if request.id != selected.capability() {
                continue;
            }
            let id = request.request_id.as_deref().ok_or_else(|| {
                AnimationError::PermissionDenied("native audio request identity missing".into())
            })?;
            let Some(grant) = broker
                .grant(&active.channel, id)
                .map_err(permission_error)?
            else {
                continue;
            };
            let crate::permissions::Scope::Audio { device, products } = &grant.right.scope else {
                continue;
            };
            if grant.binding.as_ref() == Some(selected.binding())
                && device == selected.scope_device()
                && needed_products.is_subset(products)
                && active
                    .plan
                    .demands
                    .contains_key(&crate::plan_authorization::operation_demand(id))
            {
                selected_request_id = Some(id.to_owned());
                break;
            }
        }
        let request_id = selected_request_id.ok_or_else(|| {
            AnimationError::PermissionDenied("no accepted selected audio demand".into())
        })?;
        let epoch = active.plan.authorization_epoch;
        let channel = active.channel.clone();
        drop(broker);
        let grant = crate::native_audio::AuthenticatedAudioGrant::from_host(
            source,
            demand.products.clone(),
            epoch,
        )?;
        let factory = crate::native_audio_capture::NativeAudioCaptureFactory::new(
            selected,
            Arc::clone(&self.broker),
            channel,
            crate::plan_authorization::operation_demand(&request_id),
            self.quota.clone(),
        )?;
        Ok(Some((demand, grant, factory)))
    }
    fn live_helper_authority(&self) -> Result<ServiceAuthority> {
        // Guard every runtime path that could execute package code or issue new work.
        if self.helper_retired {
            return Err(AnimationError::Runtime(
                "helper retirement has begun".into(),
            ));
        } // Failed physical teardown cannot make the helper logically usable again.
        self.current_service_authority() // Recheck the native channel even when no privileged grants were requested.
    } // Retained native playback uses current_service_authority directly after independent physical proof.
    fn validate_service_request(&self, request: &HostRequest) -> Result<ServiceAuthority> {
        // Bind a retained request to its original native package activation.
        let authority = self.current_service_authority()?; // A previous epoch or retired broker channel fails before native effects or publication.
        self.validate_service_request_locked(request, authority)?;
        Ok(authority)
    }
    fn validate_service_request_locked(
        &self,
        request: &HostRequest,
        authority: ServiceAuthority,
    ) -> Result<ServiceAuthority> {
        validate_request(self.package.digest(), request, authority)
    }
    pub fn pump(&mut self) -> Result<CreateState> {
        // Only a fresh native-authorized pump may execute completion continuations.
        self.live_helper_authority()?; // Preserve the native channel check after consent changes and helper retirement.
        let state = self.helper.pump()?; // The helper command performs the only service-reaction checkpoint.
        self.creation = Some(state); // Record the actual resulting creation state.
        Ok(state) // This operation can expose queued requests but never authorizes their native effects itself.
    } // Complete and diagnostic paths remain separate from this pump.
    pub fn requests(&mut self) -> Result<Vec<HostRequest>> {
        // Return only retained requests belonging to the currently accepted native activation.
        let authority = self.live_helper_authority()?; // Do not drain requests for a stale or logically retired helper.
        let requests = self.helper.take_requests(); // Move admitted request ownership without deep-copying binary planes.
        if requests.iter().any(|request| {
            request.package_digest != self.package.digest() || request.authority != authority
        }) {
            // Validate the complete batch before publishing any request to the dispatcher.
            for request in &requests {
                request.stop_token().stop();
            } // Signal all affected native request owners without claiming that any body exited.
            let retirement = self.retire_helper(); // A mixed or stale authenticated response retires its helper only.
            return Err(AnimationError::PermissionDenied(format!(
                "helper request activation mismatch; helper retirement: {retirement:?}"
            ))); // Native activation and unrelated native owners remain independently owned.
        } // No request reaches a native service before the whole batch's authority has been checked.
        if self.mode == AnimationMode::PreRendered {
            self.replay_video_open_count = self
                .replay_video_open_count
                .checked_add(
                    requests
                        .iter()
                        .filter(|request| {
                            request.method == "media.video.open"
                                && matches!(
                                    request.phase,
                                    ServicePhase::Create | ServicePhase::Async
                                )
                        })
                        .count() as u64,
                )
                .ok_or_else(|| AnimationError::Budget("replay Video open counter".into()))?;
            #[cfg(all(feature = "native-host", feature = "native-network"))]
            {
                self.replay_source_open_count = self
                    .replay_source_open_count
                    .checked_add(
                        requests
                            .iter()
                            .filter(|request| {
                                crate::native_source_host::SourceCall::recognized(&request.method)
                                    && request.method.ends_with(".open")
                                    && matches!(
                                        request.phase,
                                        ServicePhase::Create | ServicePhase::Async
                                    )
                            })
                            .count() as u64,
                    )
                    .ok_or_else(|| AnimationError::Budget("replay source open counter".into()))?;
            }
            self.replay_source_freeze_count = self
                .replay_source_freeze_count
                .checked_add(
                    requests
                        .iter()
                        .filter(|request| request.method == "replay.freeze")
                        .count() as u64,
                )
                .ok_or_else(|| AnimationError::Budget("replay source freeze counter".into()))?;
            self.replay_source_sequence_count = self
                .replay_source_sequence_count
                .checked_add(
                    requests
                        .iter()
                        .filter(|request| request.method == "replay.capture_sequence")
                        .count() as u64,
                )
                .ok_or_else(|| AnimationError::Budget("replay source sequence counter".into()))?;
            self.replay_pure_yield_count = self
                .replay_pure_yield_count
                .checked_add(
                    requests
                        .iter()
                        .filter(|request| {
                            request.method == "tasks.yield"
                                && request.phase == ServicePhase::Create
                                && request.payload.metadata() == &serde_json::json!({})
                                && request.payload.arrays().is_empty()
                                && request.payload.planes().is_empty()
                        })
                        .count() as u64,
                )
                .ok_or_else(|| AnimationError::Budget("replay pure yield counter".into()))?;
            self.replay_request_count = self
                .replay_request_count
                .checked_add(requests.len() as u64)
                .ok_or_else(|| AnimationError::Budget("replay host request counter".into()))?;
        }
        Ok(requests
            .into_iter()
            .filter(|request| !request.is_cancelled())
            .collect()) // Suppress expired or cancelled unissued work while surviving aliases keep their leases.
    } // Actual dispatch still derives endpoint requirements and obtains a private operation ticket.
    pub fn take_cancelled_requests(&mut self) -> Vec<HostRequest> {
        // Drain terminal notifications even after logical retirement or native revocation.
        self.helper.take_cancelled_requests() // These immutable records signal cancellation and do not establish actual worker termination.
    } // Native operation owners retain their own receipts until real completion.
    pub(crate) fn dispatch_service(
        &mut self,
        request: HostRequest,
        demand_id: &str,
        needs: Vec<OperationNeed>,
    ) -> Result<ServiceOperation> {
        self.dispatch_service_selected(request, |_, broker, channel, phase| {
            broker
                .dispatch(channel, phase, demand_id, needs)
                .map_err(permission_error)
        })
    }
    fn dispatch_service_selected(
        &mut self,
        request: HostRequest,
        select: impl FnOnce(
            &AuthorizationProjection,
            &mut PermissionBroker,
            &crate::permissions::Channel,
            CallPhase,
        ) -> Result<OperationTicket>,
    ) -> Result<ServiceOperation> {
        self.live_helper_authority()?;
        self.validate_service_request(&request)?;
        if !request.payload.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "foreign native service request root".into(),
            ));
        }
        if request.is_cancelled() {
            return Err(AnimationError::Runtime(
                "service request is already terminal".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let phase = match request.phase {
            ServicePhase::Create => CallPhase::Create,
            ServicePhase::Async => CallPhase::Async,
        };
        let mut broker = lock_broker(&self.broker)?;
        let ticket = select(&self.projection, &mut broker, &active.channel, phase)?;
        drop(broker);
        Ok(ServiceOperation {
            request,
            ticket: Arc::new(ticket),
        })
    }
    /// Original operation factory prepared BEFORE actual finite IO submission.
    /// Only original commit_service(... actual client.try_submit ...) marks issued.
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn http_authority_factory(
        &self,
        operation: &ServiceOperation,
        credentials: Option<Box<dyn crate::native_http_authority::HostCredentialAdapter>>,
    ) -> Result<crate::native_http_authority::NativeHttpAuthorityFactory> {
        self.live_helper_authority()?;
        self.validate_service_request(&operation.request)?;
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        crate::native_http_authority::NativeHttpAuthorityFactory::from_native(
            Arc::clone(&self.broker),
            active.channel.clone(),
            Arc::clone(&operation.ticket),
            operation.request.clone(),
            self.quota.clone(),
            credentials,
        )
    }
    /// Capture the exact grants which covered the original committed Video
    /// selected/HTTP operation before its helper result settles that ticket.
    /// The returned lineage has no effect authority without this broker.
    pub(crate) fn video_replay_lineage(
        &self,
        operation: &ServiceOperation,
    ) -> Result<Vec<GrantLineage>> {
        if operation.request.method != "media.video.open" {
            return Err(AnimationError::PermissionDenied(
                "recorded Video source requires its original open operation".into(),
            ));
        }
        let active = self.activation.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("recorded Video activation missing".into())
        })?;
        let broker = lock_broker(&self.broker)?;
        let grants = broker
            .committed_operation_grants(&operation.ticket, &active.channel)
            .map_err(permission_error)?;
        grants.iter().map(GrantLineage::from_grant).collect()
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn source_replay_lineage(
        &self,
        operation: &ServiceOperation,
    ) -> Result<Vec<GrantLineage>> {
        if !crate::native_source_host::SourceCall::recognized(&operation.request.method)
            || !operation.request.method.ends_with(".open")
        {
            return Err(AnimationError::PermissionDenied(
                "recorded source requires its original committed feed-open operation".into(),
            ));
        }
        let active = self.activation.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("recorded source activation missing".into())
        })?;
        let broker = lock_broker(&self.broker)?;
        let grants = broker
            .committed_operation_grants(&operation.ticket, &active.channel)
            .map_err(permission_error)?;
        grants.iter().map(GrantLineage::from_grant).collect()
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn source_planning_authority(
        &self,
        request: &HostRequest,
        quota: &QuotaGroup,
    ) -> Result<crate::native_source_host::SourcePlanningAuthority> {
        self.live_helper_authority()?;
        self.validate_service_request(request)?;
        if !self.quota.shares_root(quota)
            || !request.payload.shares_root(quota)
            || !crate::native_source_host::SourceCall::recognized(&request.method)
        {
            return Err(AnimationError::PermissionDenied(
                "source planning original owner/call mismatch".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("source activation missing".into()))?;
        crate::native_source_host::SourcePlanningAuthority::from_runtime(
            Arc::clone(&self.broker),
            active.channel.clone(),
            request.clone(),
            self.quota.clone(),
        )
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_baseline_source<T>(
        &mut self,
        request: &HostRequest,
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.live_helper_authority()?;
        if !request.payload.shares_root(&self.quota)
            || !crate::native_source_host::SourceCall::recognized(&request.method)
        {
            return Err(AnimationError::PermissionDenied(
                "closed native source planning call".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let broker = lock_broker(&owner)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)?;
        if request.is_cancelled() {
            return Err(AnimationError::PermissionDenied(
                "source planning cancelled".into(),
            ));
        }
        Ok(issue()) // Actual bounded original CPU queue issue only; no transport, credential, JS, wait or baseline grant.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn dispatch_source_http_service(
        &mut self,
        request: HostRequest,
        need: OperationNeed,
    ) -> Result<ServiceOperation> {
        if !crate::native_source_host::SourceCall::recognized(&request.method) {
            return Err(AnimationError::PermissionDenied(
                "closed native source transport call".into(),
            ));
        }
        self.dispatch_service_selected(request, |projection, broker, channel, phase| {
            projection.dispatch_http(broker, channel, phase, need)
        })
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    fn with_current_source_operation<T>(
        &self,
        operation: &ServiceOperation,
        issue: impl FnOnce(&PermissionBroker, &crate::permissions::Channel) -> Result<T>,
    ) -> Result<T> {
        self.live_helper_authority()?;
        self.validate_service_request(&operation.request)?;
        if operation.request.is_cancelled()
            || !crate::native_source_host::SourceCall::recognized(&operation.request.method)
        {
            return Err(AnimationError::PermissionDenied(
                "source operation lifetime/call".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("source activation missing".into()))?;
        let broker = lock_broker(&self.broker)?;
        broker
            .check_operation_lineage(&operation.ticket, &active.channel)
            .map_err(permission_error)?;
        issue(&broker, &active.channel) // SAME owner through bounded native issue; no JS/wait/network under guard.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_unissued_authority<T>(
        &self,
        operation: &ServiceOperation,
        issue: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.with_current_source_operation(operation, |_, _| issue()) // Pure bounded cadence only, NEVER commit or issue IO.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_cpu_authority<T>(
        &self,
        operation: &ServiceOperation,
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.with_current_source_operation(operation, |broker, channel| {
            broker
                .check_committed_operation(&operation.ticket, channel)
                .map_err(permission_error)?;
            Ok(issue()) // Actual original CPU submit, no nested wait.
        })
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_capture_authority<T>(
        &self,
        owner: &Arc<Mutex<PermissionBroker>>,
        fence: &crate::permissions::SourceCaptureFence,
        quota: &QuotaGroup,
        read: impl FnOnce() -> T,
    ) -> Result<T> {
        if self.helper_retired
            || !Arc::ptr_eq(owner, &self.broker)
            || !self.quota.shares_root(quota)
        {
            return Err(AnimationError::PermissionDenied(
                "source capture original owner retired or mismatched".into(),
            ));
        }
        let broker = lock_broker(owner)?;
        let fenced = broker
            .check_source_capture_fence(fence)
            .map_err(permission_error)?;
        let current = self.current_service_authority_locked(&broker)?;
        if fenced
            != (
                current.instance_id,
                current.plan_generation,
                current.authorization_epoch,
            )
        {
            return Err(AnimationError::PermissionDenied(
                "source capture activation changed".into(),
            ));
        }
        Ok(read())
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_http_authority<T>(
        &self,
        operation: &ServiceOperation,
        needs: &[OperationNeed],
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.with_current_source_operation(operation, |broker, channel| {
            broker
                .check_committed_needs(&operation.ticket, channel, needs)
                .map_err(permission_error)?;
            Ok(issue()) // Additional exact native provider page under SAME original committed operation.
        })
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn dispatch_source_feed_http_service(
        &mut self,
        source: &crate::native_source_host::SourcePlanningAuthority,
        need: OperationNeed,
    ) -> Result<SourceFeedOperation> {
        self.live_helper_authority()?;
        let (owner, channel, quota, stop, deadline) = source.feed_context()?;
        if !Arc::ptr_eq(&owner, &self.broker) || !quota.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "source feed original broker/quota mismatch".into(),
            ));
        }
        let active = self.activation.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("source feed activation missing".into())
        })?;
        let mut broker = lock_broker(&self.broker)?;
        let coordinates = broker
            .channel_coordinates(&channel)
            .map_err(permission_error)?;
        let current = self.current_service_authority_locked(&broker)?;
        if coordinates
            != (
                current.instance_id,
                current.plan_generation,
                current.authorization_epoch,
            )
            || broker
                .channel_coordinates(&active.channel)
                .map_err(permission_error)?
                != coordinates
        {
            return Err(AnimationError::PermissionDenied(
                "source feed activation changed".into(),
            ));
        }
        let ticket =
            self.projection
                .dispatch_http(&mut broker, &channel, CallPhase::Async, need)?;
        Ok(SourceFeedOperation {
            ticket: Arc::new(ticket),
            broker: owner,
            channel,
            quota,
            stop,
            deadline,
        })
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_feed_planning<T>(
        &self,
        context: (
            Arc<Mutex<PermissionBroker>>,
            Channel,
            QuotaGroup,
            StopToken,
            Instant,
        ),
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.live_helper_authority()?;
        let (owner, channel, quota, stop, deadline) = context;
        if stop.is_stopped()
            || Instant::now() >= deadline
            || !Arc::ptr_eq(&owner, &self.broker)
            || !quota.shares_root(&self.quota)
        {
            return Err(AnimationError::PermissionDenied(
                "source feed planning owner/lifetime".into(),
            ));
        }
        let broker = lock_broker(&self.broker)?;
        let current = self.current_service_authority_locked(&broker)?;
        if broker
            .channel_coordinates(&channel)
            .map_err(permission_error)?
            != (
                current.instance_id,
                current.plan_generation,
                current.authorization_epoch,
            )
        {
            return Err(AnimationError::PermissionDenied(
                "source feed planning channel changed".into(),
            ));
        }
        Ok(issue())
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    fn check_source_feed_operation(&self, operation: &SourceFeedOperation) -> Result<()> {
        self.live_helper_authority()?;
        if operation.is_stopped_or_expired()
            || !Arc::ptr_eq(&operation.broker, &self.broker)
            || !operation.quota.shares_root(&self.quota)
        {
            return Err(AnimationError::PermissionDenied(
                "source feed operation lifetime/root mismatch".into(),
            ));
        }
        let broker = lock_broker(&self.broker)?;
        let current = self.current_service_authority_locked(&broker)?;
        if broker
            .channel_coordinates(&operation.channel)
            .map_err(permission_error)?
            != (
                current.instance_id,
                current.plan_generation,
                current.authorization_epoch,
            )
        {
            return Err(AnimationError::PermissionDenied(
                "source feed operation channel changed".into(),
            ));
        }
        broker
            .check_operation_lineage(&operation.ticket, &operation.channel)
            .map_err(permission_error)
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_feed_unissued<T>(
        &self,
        operation: &SourceFeedOperation,
        issue: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.check_source_feed_operation(operation)?;
        issue()
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn commit_source_feed<T>(
        &mut self,
        operation: &SourceFeedOperation,
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.check_source_feed_operation(operation)?;
        lock_broker(&self.broker)?
            .commit(&operation.ticket, issue)
            .map_err(permission_error)
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_feed_cpu_authority<T>(
        &self,
        operation: &SourceFeedOperation,
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.check_source_feed_operation(operation)?;
        let broker = lock_broker(&self.broker)?;
        broker
            .check_committed_operation(&operation.ticket, &operation.channel)
            .map_err(permission_error)?;
        Ok(issue())
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_feed_http_authority<T>(
        &self,
        operation: &SourceFeedOperation,
        needs: &[OperationNeed],
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.check_source_feed_operation(operation)?;
        let broker = lock_broker(&self.broker)?;
        broker
            .check_committed_needs(&operation.ticket, &operation.channel, needs)
            .map_err(permission_error)?;
        Ok(issue())
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn source_feed_http_authority_factory(
        &self,
        operation: &SourceFeedOperation,
        credentials: Option<Box<dyn crate::native_http_authority::HostCredentialAdapter>>,
    ) -> Result<crate::native_http_authority::NativeHttpAuthorityFactory> {
        self.check_source_feed_operation(operation)?;
        crate::native_http_authority::NativeHttpAuthorityFactory::from_feed(
            Arc::clone(&operation.broker),
            operation.channel.clone(),
            Arc::clone(&operation.ticket),
            operation.quota.clone(),
            operation.stop.clone(),
            operation.deadline,
            credentials,
        )
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn deliver_source_feed<T>(
        &mut self,
        operation: &SourceFeedOperation,
        publish: impl FnOnce() -> T,
    ) -> Result<T> {
        self.check_source_feed_operation(operation)?;
        lock_broker(&self.broker)?
            .deliver(&operation.ticket, publish)
            .map_err(permission_error)
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn settle_source_feed_terminal(
        &mut self,
        operation: &SourceFeedOperation,
    ) -> Result<()> {
        if !Arc::ptr_eq(&operation.broker, &self.broker)
            || !operation.quota.shares_root(&self.quota)
        {
            return Err(AnimationError::PermissionDenied(
                "source feed terminal foreign owner".into(),
            ));
        }
        let mut broker = lock_broker(&self.broker)?;
        match broker.settle_without_delivery(&operation.ticket) {
            Ok(()) | Err(crate::permissions::PermissionError::UnknownOperation) => Ok(()),
            Err(error) => Err(permission_error(error)),
        }
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn complete_baseline_source(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        self.live_helper_authority()?;
        if request.is_cancelled()
            || !request.payload.shares_root(&self.quota)
            || !value.shares_root(&self.quota)
            || !crate::native_source_host::SourceCall::recognized(&request.method)
        {
            return Err(AnimationError::PermissionDenied(
                "source baseline publication root/call".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let digest = self.package.digest();
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("source activation missing".into()))?;
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            if request.is_cancelled() {
                return Err(AnimationError::PermissionDenied(
                    "source publication cancelled".into(),
                ));
            }
            helper.complete_service_request(request.id, authority, value)
        }) // Actual inert copy ACK; no helper JS/checkpoint under original native owner.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub fn complete_native_replay_freeze(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        self.live_helper_authority()?;
        if request.method != "replay.freeze"
            || request.is_cancelled()
            || !request.payload.shares_root(&self.quota)
            || !value.shares_root(&self.quota)
        {
            return Err(AnimationError::PermissionDenied(
                "replay freeze publication owner/call mismatch".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let digest = self.package.digest();
        let active = self.activation.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("replay freeze activation missing".into())
        })?;
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            if request.is_cancelled() {
                return Err(AnimationError::PermissionDenied(
                    "replay freeze publication cancelled".into(),
                ));
            }
            helper.complete_service_request(request.id, authority, value)
        })
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub fn complete_native_replay_sequence(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        self.live_helper_authority()?;
        if self.mode != AnimationMode::PreRendered
            || !matches!(request.phase, ServicePhase::Create | ServicePhase::Async)
            || request.method != "replay.capture_sequence"
            || request.is_cancelled()
            || !request.payload.shares_root(&self.quota)
            || !value.shares_root(&self.quota)
        {
            return Err(AnimationError::PermissionDenied(
                "replay sequence publication mode/owner/call mismatch".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let digest = self.package.digest();
        let active = self.activation.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("replay sequence activation missing".into())
        })?;
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            if request.is_cancelled() {
                return Err(AnimationError::PermissionDenied(
                    "replay sequence publication cancelled".into(),
                ));
            }
            helper.complete_service_request(request.id, authority, value)
        })
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub fn copy_native_source_feed_snapshots(
        &mut self,
        snapshots: &[Value],
    ) -> Result<ServiceValue> {
        self.live_helper_authority()?;
        if snapshots.len() > 32 {
            return Err(AnimationError::Budget(
                "source feed snapshot inventory".into(),
            ));
        }
        let broker = lock_broker(&self.broker)?;
        self.current_service_authority_locked(&broker)?;
        ServiceValue::copy_from_host(
            &Value::Array(snapshots.to_vec()),
            &[],
            &BTreeMap::new(),
            self.engine_limits(),
            self.quota.clone(),
        )
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub fn complete_native_source_feed_close(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        self.authorize_native_source_feed_close(request)?;
        let method_kind = request.method.strip_suffix(".close");
        let fields = request.payload.metadata().as_object();
        let descriptor = &value.metadata()["value"];
        if !matches!(
            method_kind,
            Some(
                "sources.series"
                    | "sources.earthquakes"
                    | "sources.aircraft"
                    | "sources.boats"
                    | "sources.chess"
                    | "sources.weather"
            )
        ) || fields.is_none_or(|fields| {
            fields.len() != 2
                || fields.get("kind").and_then(Value::as_str) != method_kind
                || fields
                    .get("id")
                    .and_then(Value::as_str)
                    .is_none_or(|id| !id.starts_with("source-feed-") || id.len() > 128)
        }) || !value.arrays().is_empty()
            || value.metadata()["ok"] != true
            || descriptor["id"] != request.payload.metadata()["id"]
            || descriptor["kind"] != request.payload.metadata()["kind"]
            || descriptor["revision"].as_u64().is_none()
            || descriptor["status"]["state"] != "closed"
            || descriptor.get("latest").is_some()
        {
            return Err(AnimationError::PermissionDenied(
                "native source feed close mismatch".into(),
            ));
        }
        self.complete_baseline_source(request, value)
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub fn authorize_native_source_feed_close(&self, request: &HostRequest) -> Result<()> {
        self.live_helper_authority()?;
        if request.is_cancelled()
            || !request.payload.shares_root(&self.quota)
            || !matches!(
                request.method.as_str(),
                "sources.series.close"
                    | "sources.earthquakes.close"
                    | "sources.aircraft.close"
                    | "sources.boats.close"
                    | "sources.chess.close"
                    | "sources.weather.close"
            )
        {
            return Err(AnimationError::PermissionDenied(
                "source feed control lifetime/root".into(),
            ));
        }
        self.validate_service_request(request).map(|_| ())
    }

    /// Release a registered source image only after the helper has accepted
    /// its original control request. All non-Delivered results keep custody.
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub fn complete_native_source_image_close(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        self.live_helper_authority()?;
        let fields = request.payload.metadata().as_object();
        if request.method != "media.images.close"
            || request.is_cancelled()
            || !request.payload.shares_root(&self.quota)
            || !value.shares_root(&self.quota)
            || !value.arrays().is_empty()
            || value.metadata() != &serde_json::json!({"ok":true,"value":null})
            || fields.is_none_or(|fields| {
                fields.len() != 2
                    || fields.get("kind").and_then(Value::as_str) != Some("image")
                    || fields
                        .get("id")
                        .and_then(Value::as_str)
                        .is_none_or(|id| !id.starts_with("source-image-") || id.len() > 64)
            })
        {
            return Err(AnimationError::PermissionDenied(
                "native source image close request or result mismatch".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let digest = self.package.digest();
        let active = self.activation.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("source image activation missing".into())
        })?;
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            helper.complete_service_request(request.id, authority, value)
        })
    }

    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn native_http_owner(
        &self,
        quota: &QuotaGroup,
    ) -> Result<Arc<Mutex<PermissionBroker>>> {
        self.live_helper_authority()?;
        if !self.quota.shares_root(quota) {
            return Err(AnimationError::PermissionDenied(
                "HTTP original-root mismatch".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("HTTP activation missing".into()))?;
        {
            let broker = lock_broker(&self.broker)?;
            authority_from(&broker, active)?;
        }
        Ok(Arc::clone(&self.broker))
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn check_http_owner(
        &self,
        owner: &Arc<Mutex<PermissionBroker>>,
        quota: &QuotaGroup,
    ) -> Result<()> {
        if !Arc::ptr_eq(owner, &self.broker) || !quota.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "foreign HTTP native owner".into(),
            ));
        }
        Ok(()) // Owner identity only; every issue/publication separately authenticates current authority.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn dispatch_http_service(
        &mut self,
        request: HostRequest,
        need: OperationNeed,
    ) -> Result<ServiceOperation> {
        if request.method != "http.request" {
            return Err(AnimationError::PermissionDenied(
                "invalid native HTTP request".into(),
            ));
        }
        self.dispatch_service_selected(request, |projection, broker, channel, phase| {
            projection.dispatch_http(broker, channel, phase, need)
        }) // SAME dispatch_service owner path; only the native dependency selector differs.
    }
    /// A Video URL open uses the identical reviewed HTTP dependency selector,
    /// but retains its original Video service request and decoder owner. The
    /// browser-supplied URL never becomes an FFmpeg argument.
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn dispatch_video_http_service(
        &mut self,
        request: HostRequest,
        need: OperationNeed,
    ) -> Result<ServiceOperation> {
        if request.method != "media.video.open" {
            return Err(AnimationError::PermissionDenied(
                "invalid native Video HTTP request".into(),
            ));
        }
        self.dispatch_service_selected(request, |projection, broker, channel, phase| {
            projection.dispatch_http(broker, channel, phase, need)
        })
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_http_operation_authority<T>(
        &self,
        operation: &ServiceOperation,
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.live_helper_authority()?;
        self.validate_service_request(&operation.request)?;
        if operation.request.method != "http.request" || operation.request.is_cancelled() {
            return Err(AnimationError::PermissionDenied(
                "HTTP decode issue cancelled".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("HTTP activation missing".into()))?;
        let broker = lock_broker(&self.broker)?;
        broker
            .check_committed_operation(&operation.ticket, &active.channel)
            .map_err(permission_error)?;
        Ok(issue()) // SAME original ticket; bounded pure CPU queue issue only, no second commit or nested wait.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn complete_http_refusal(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        if request.method != "http.request"
            || !request.payload.shares_root(&self.quota)
            || !value.shares_root(&self.quota)
            || value.metadata().get("ok").and_then(Value::as_bool) != Some(false)
            || !value.arrays().is_empty()
        {
            return Err(AnimationError::PermissionDenied(
                "HTTP refusal schema".into(),
            ));
        }
        self.live_helper_authority()?;
        let owner = Arc::clone(&self.broker);
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("HTTP activation missing".into()))?;
        let digest = self.package.digest();
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            helper.complete_service_request(request.id, authority, value)
        }) // Nonacquiring fixed native error only; SAME guard through inert copy/ACK, no minted effect ticket.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn settle_http_terminal(&mut self, operation: &ServiceOperation) -> Result<()> {
        // Caller MUST own real terminal outcome/rejected input. Lost/cancel signal is not terminal proof.
        let mut broker = lock_broker(&self.broker)?;
        match broker.settle_without_delivery(&operation.ticket) {
            Ok(()) | Err(crate::permissions::PermissionError::UnknownOperation) => Ok(()), // Genuine issuer already settled by final publication.
            Err(error) => Err(permission_error(error)), // Foreign/poisoned authority cannot be reset.
        }
    }

    pub(crate) fn commit_service<T>(
        &mut self,
        operation: &ServiceOperation,
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        // Commit only a concrete bounded native effect prepared by the service owner.
        self.live_helper_authority()?; // Logical helper retirement prevents new effects while retained playback remains authorized separately.
        self.validate_service_request(&operation.request)?; // Recheck the original package and activation after queue residence.
        if operation.request.is_cancelled() {
            return Err(AnimationError::Runtime(
                "service request is already terminal".into(),
            ));
        } // A cancellation signal prevents uncommitted work from issuing.
        lock_broker(&self.broker)?
            .commit(&operation.ticket, issue)
            .map_err(permission_error) // Native issue must be bounded and must not wait for I/O or run JavaScript.
    } // Actual body lifetime remains with the native job/receipt, not the committed ticket alone.
    /// One native effect can depend on multiple individually accepted scoped
    /// rights. Every operation must retain the exact same original request Arc;
    /// a copied correlation ID cannot join another body's authorization.
    pub(crate) fn commit_service_group<T>(
        &mut self,
        operations: &[&ServiceOperation],
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.live_helper_authority()?;
        let request = original_service_group_request(operations)?;
        self.validate_service_request(request)?;
        if request.is_cancelled() {
            return Err(AnimationError::Runtime(
                "service group request is already terminal".into(),
            ));
        }
        let tickets: Vec<_> = operations
            .iter()
            .map(|operation| operation.ticket.as_ref())
            .collect();
        lock_broker(&self.broker)?
            .commit_group(&tickets, issue)
            .map_err(permission_error)
    }
    pub(crate) fn prepare_service_group_settlement(
        &self,
        operations: &[&ServiceOperation],
    ) -> Result<Arc<ServiceGroupSettlement>> {
        self.live_helper_authority()?;
        let request = original_service_group_request(operations)?;
        self.validate_service_request(request)?;
        Ok(Arc::new(ServiceGroupSettlement {
            request: request.clone(),
            tickets: operations
                .iter()
                .map(|operation| Arc::clone(&operation.ticket))
                .collect(),
            settled: AtomicBool::new(false),
        }))
    }
    /// Use only after genuine native completion or known unissued work. The
    /// opaque receipt proves prior settlement if delivery returned an error.
    pub(crate) fn settle_service_group(
        &mut self,
        operations: &[&ServiceOperation],
        settlement: &ServiceGroupSettlement,
    ) -> Result<()> {
        settlement.check_original(operations)?;
        if settlement.was_settled() {
            return Ok(());
        }
        let tickets: Vec<_> = operations
            .iter()
            .map(|operation| operation.ticket.as_ref())
            .collect();
        lock_broker(&self.broker)?
            .settle_group_without_delivery(&tickets)
            .map_err(permission_error)?;
        settlement.settled.store(true, Ordering::Release);
        Ok(())
    }
    /// Deliver one helper completion only after all original dependencies pass
    /// current authorization. Refused delivery retains all unsettled tickets.
    pub(crate) fn complete_authorized_group(
        &mut self,
        operations: &[&ServiceOperation],
        value: ServiceValue,
        settlement: &ServiceGroupSettlement,
    ) -> Result<CompletionState> {
        settlement.check_original(operations)?;
        if settlement.was_settled() {
            return Err(AnimationError::PermissionDenied(
                "service group already settled".into(),
            ));
        }
        let request = original_service_group_request(operations)?;
        if !value.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "foreign service group result root".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        if self.helper_retired {
            self.settle_service_group(operations, settlement)?;
            return Ok(CompletionState::Cancelled);
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let digest = self.package.digest();
        let tickets: Vec<_> = operations
            .iter()
            .map(|operation| operation.ticket.as_ref())
            .collect();
        self.helper.with_native_publication(|helper| {
            let mut broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            broker
                .deliver_group(&tickets, || {
                    // Broker removed all original tickets before this callback.
                    // Publish evidence before helper I/O/deferred retirement.
                    settlement.settled.store(true, Ordering::Release);
                    helper.complete_service_request(request.id, authority, value)
                })
                .map_err(permission_error)?
        })
    }
    pub(crate) fn complete_authorized(
        &mut self,
        operation: &ServiceOperation,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        // Call only after the actual native body has finished producing its admitted immutable result.
        let owner = Arc::clone(&self.broker);
        if self.helper_retired {
            lock_broker(&owner)?
                .settle_without_delivery(&operation.ticket)
                .map_err(permission_error)?;
            return Ok(CompletionState::Cancelled);
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let digest = self.package.digest();
        self.helper.with_native_publication(|helper| {
            let mut broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, &operation.request, authority)?;
            broker
                .deliver(&operation.ticket, || {
                    helper.complete_service_request(operation.request.id, authority, value)
                })
                .map_err(permission_error)?
        })
    }
    pub(crate) fn settle_service(&mut self, operation: &ServiceOperation) -> Result<()> {
        // Call only after native failure/cancellation has actually completed or a committed result is intentionally discarded.
        lock_broker(&self.broker)?
            .settle_without_delivery(&operation.ticket)
            .map_err(permission_error) // Stale authorization can be settled without releasing the external job's remaining guards.
    } // Cancellation notification, helper EOF, and a dropped waiter are not evidence that this precondition holds.
    pub fn render(&mut self, context: &Value, arrays: &[ArraySpec]) -> Result<RetainedHelperFrame> {
        self.live_helper_authority()?; // Rendering requires both a current native channel and a logically live helper.
        if self.creation != Some(CreateState::Ready) {
            return Err(AnimationError::Runtime(
                "animation preparation is pending".into(),
            ));
        }
        self.helper.render_retained(context, arrays)
    }

    /// Drain the bounded diagnostics emitted by the most recent render. The
    /// helper copied these records into the parent before returning the frame;
    /// reading them here performs no guest call and grants no capability.
    pub fn take_status(&mut self) -> Result<Option<Value>> {
        self.live_helper_authority()?;
        Ok(self
            .helper
            .take_status()
            .map(|status| status.into_parts().0))
    }

    /// Native typed copy/ACK under original broker guard, never a JS seed hook.
    pub fn prepare_frame_seed(
        &mut self,
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, Vec<u8>>,
    ) -> Result<PreparedSeed> {
        self.live_helper_authority()?;
        if self.creation != Some(CreateState::Ready) {
            return Err(AnimationError::Runtime(
                "animation preparation is pending".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            authority_from(&broker, active)?;
            helper.prepare_frame_seed(metadata, arrays, planes)
        })
    }
    /// Actual native receipt only; recheck current original activation then
    /// release authority guard BEFORE trusted JS hook (never pump here).
    pub fn activate_frame_seed(&mut self, seed: PreparedSeed) -> Result<()> {
        let check = self.live_helper_authority().and_then(|authority| {
            if authority != seed.authority() {
                return Err(AnimationError::PermissionDenied(
                    "stale native seed activation".into(),
                ));
            }
            Ok(())
        });
        if let Err(error) = check {
            let discard = self.helper.discard_frame_seed(seed);
            return Err(match discard {
                Ok(()) => error,
                Err(discard) => AnimationError::Runtime(format!(
                    "seed activation refused: {error}; discard failed: {discard}"
                )),
            });
        }
        self.helper.activate_frame_seed(seed)
    }
    pub fn discard_frame_seed(&mut self, seed: PreparedSeed) -> Result<()> {
        self.helper.discard_frame_seed(seed) // Only native cleanup; no grant or hook.
    }
    pub fn seed_frame(
        &mut self,
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, Vec<u8>>,
    ) -> Result<()> {
        let seed = self.prepare_frame_seed(metadata, arrays, planes)?;
        self.activate_frame_seed(seed)
    }

    /// Construct a decoder authorization from this original accepted broker
    /// channel. The caller must assign a fresh nonzero native resource ID and
    /// separately authenticate its bundle, selected ticket or HTTP receipt.
    /// This callback stays usable across physical helper retirement for finite
    /// clip preparation, but revocation still fences every codec effect.
    pub(crate) fn native_video_authorization(
        &self,
        resource_id: u64,
    ) -> Result<(VideoAuthority, Arc<dyn VideoAuthorization>)> {
        if resource_id == 0 {
            return Err(AnimationError::PermissionDenied(
                "zero native video resource identity".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("video activation missing".into()))?;
        let broker = lock_broker(&self.broker)?;
        let service = self.current_service_authority_locked(&broker)?;
        let expected = VideoAuthority {
            package_digest: self.package.digest().into(),
            instance_id: service.instance_id,
            plan_revision: service.plan_generation,
            authorization_epoch: service.authorization_epoch,
        };
        let storage = self.quota.reserve_external_storage(512).map_err(|error| {
            AnimationError::Budget(format!("video authorization admission: {error:?}"))
        })?;
        let owner: Arc<dyn VideoAuthorization> = Arc::new(InstanceVideoAuthorization {
            broker: Arc::clone(&self.broker),
            channel: active.channel.clone(),
            expected: expected.clone(),
            resource_id,
            _storage: storage,
        });
        Ok((expected, owner))
    }

    /// Current native coordinates are advisory until with_frame_authority or with_playback_authority protects actual emission.
    pub fn frame_authority(&self) -> Option<HelperAuthority> {
        // Keep publication coordinates independent from the immutable IPC session stamp.
        let active = self.current_service_authority().ok()?; // Authenticate the broker's private channel even after physical helper retirement.
        Some(HelperAuthority {
            package_digest: self.package.digest().into(),
            instance_id: active.instance_id,
            plan_generation: active.plan_generation,
            authorization_epoch: active.authorization_epoch,
        }) // These copied fields cannot authorize a later handoff by themselves.
    } // Retained frame ownership and source provenance remain the native presentation owner's responsibility.
    /// Publish only the native-selected activation's PRIVATE channel with this
    /// frame. No guest result, copied ID, or catalogue record can call this.
    /// The returned owner survives retirement until queued terminal output is
    /// either written or explicitly rejected.
    pub fn retain_frame_authority(&self) -> Result<RetainedFrameAuthority> {
        if self.mode == AnimationMode::PreRendered && !self.is_physically_retired() {
            return Err(AnimationError::Runtime(
                "pre-rendered output requires physical helper retirement".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let broker = lock_broker(&self.broker)?;
        let authority = authority_from(&broker, active)?;
        let expected = HelperAuthority {
            package_digest: self.package.digest().into(),
            instance_id: authority.instance_id,
            plan_generation: authority.plan_generation,
            authorization_epoch: authority.authorization_epoch,
        };
        drop(broker);
        let storage = self.quota.reserve_external_storage(4096).map_err(|error| {
            AnimationError::Budget(format!("retained frame authority: {error:?}"))
        })?;
        Ok(RetainedFrameAuthority {
            broker: Arc::clone(&self.broker),
            channel: active.channel.clone(),
            expected,
            quota: self.quota.clone(),
            _storage: storage,
        })
    }
    /// Cancellation can revoke this original activation while its helper is
    /// owned by a finite preparation job. The job still must prove physical
    /// retirement before the controller releases original custody.
    pub fn retain_replay_revoker(&self) -> Result<RetainedReplayRevoker> {
        if self.mode != AnimationMode::PreRendered {
            return Err(AnimationError::PermissionDenied(
                "live instance cannot delegate replay".into(),
            ));
        }
        let active = self.activation.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("native replay activation missing".into())
        })?;
        let broker = lock_broker(&self.broker)?;
        let current = authority_from(&broker, active)?;
        drop(broker);
        let storage = self.quota.reserve_external_storage(4096).map_err(|error| {
            AnimationError::Budget(format!("replay revoker storage: {error:?}"))
        })?;
        Ok(RetainedReplayRevoker {
            broker: Arc::clone(&self.broker),
            channel: active.channel.clone(),
            instance_id: current.instance_id,
            retired: false,
            _storage: storage,
        })
    }
    /// The producer obtains this while active; the same retained native broker
    /// permits playback only after retire_helper confirms physical exit.
    pub fn retain_procedural_replay_authorization(
        &self,
    ) -> Result<(ReplayAuthority, Arc<dyn ReplayAuthorization>)> {
        if self.mode != AnimationMode::PreRendered {
            return Err(AnimationError::PermissionDenied(
                "live instance cannot issue replay".into(),
            ));
        }
        let active = self.activation.as_ref().ok_or_else(|| {
            AnimationError::PermissionDenied("native replay activation missing".into())
        })?;
        let broker = lock_broker(&self.broker)?;
        let current = authority_from(&broker, active)?;
        let authority = ReplayAuthority {
            package_digest: self.package.digest().into(),
            instance_id: current.instance_id,
            revision: current.plan_generation,
            authorization_epoch: current.authorization_epoch,
        };
        drop(broker);
        let storage = self.quota.reserve_external_storage(4096).map_err(|error| {
            AnimationError::Budget(format!("replay authorization storage: {error:?}"))
        })?;
        Ok((
            authority.clone(),
            Arc::new(BrokerReplayAuthorization {
                broker: Arc::clone(&self.broker),
                channel: active.channel.clone(),
                expected: authority,
                retired: Arc::clone(&self.replay_retired_proof),
                _storage: storage,
            }),
        ))
    }
    /// Register a source result that has already passed its original source
    /// completion guard. Registration is not frame emission: it may occur
    /// during create, including before a pre-rendered helper is retired.
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_registration_authority<T>(
        &mut self,
        expected: &HelperAuthority,
        register: impl FnOnce() -> T,
    ) -> Result<T> {
        self.live_helper_authority()?;
        let owner = Arc::clone(&self.broker);
        let broker = lock_broker(&owner)?;
        let active = self.current_service_authority_locked(&broker)?;
        if expected.package_digest != self.package.digest()
            || expected.instance_id != active.instance_id
            || expected.plan_generation != active.plan_generation
            || expected.authorization_epoch != active.authorization_epoch
        {
            return Err(AnimationError::PermissionDenied(
                "native source registration activation changed".into(),
            ));
        }
        Ok(register())
    }
    pub fn with_frame_authority<T>(
        &mut self,
        expected: &HelperAuthority,
        emit: impl FnOnce() -> T,
    ) -> Result<T> {
        // Run the actual bounded native frame handoff while the authority owner is exclusively borrowed.
        if self.mode == AnimationMode::PreRendered
            && (!self.helper_retired || !self.helper.is_physically_retired())
        {
            return Err(AnimationError::Runtime(
                "pre-rendered emission requires confirmed physical helper retirement".into(),
            ));
        } // Every public emission gate enforces the native-selected mode, including callers that choose this common path.
        let owner = Arc::clone(&self.broker);
        let broker = lock_broker(&owner)?;
        let authority = self.current_service_authority_locked(&broker)?;
        let current = HelperAuthority {
            package_digest: self.package.digest().into(),
            instance_id: authority.instance_id,
            plan_generation: authority.plan_generation,
            authorization_epoch: authority.authorization_epoch,
        }; // Recheck issuer, native nonce, revision, and epoch at the emission boundary.
        if &current != expected {
            return Err(AnimationError::PermissionDenied(
                "retained frame belongs to a different native activation".into(),
            ));
        } // Match the native retained frame's original package and activation coordinates.
        Ok(emit()) // The closure must perform the final native handoff; queued later consumption requires another current-authority check.
    } // This native closure may not execute helper code, acquire services, or invent protected replay provenance.
    pub fn with_playback_authority<T>(
        &mut self,
        expected: &HelperAuthority,
        emit: impl FnOnce() -> T,
    ) -> Result<T> {
        // Permit pre-rendered playback only after physical V8 helper retirement is proved.
        if !self.helper_retired || !self.helper.is_physically_retired() {
            return Err(AnimationError::Runtime(
                "pre-rendered playback requires confirmed physical helper retirement".into(),
            ));
        } // Pipe closure, cancellation notification, and a failed prior join never satisfy this gate.
        self.with_frame_authority(expected, emit) // Retain and recheck the independently owned native activation through every actual playback emission.
    } // Native clip eligibility, protected source ownership, and retained bytes remain separate mandatory checks at the caller.
    pub fn accept_frame(&mut self, accepted: bool) -> Result<()> {
        // Acknowledgement can execute package hooks and therefore needs a current native lifecycle check.
        self.live_helper_authority()?; // Never run acknowledgement JavaScript after logical helper retirement or native revocation.
        self.helper.accept_frame(accepted) // Engine checkpoints still enforce the synchronous acknowledgement acquisition prohibition.
    } // Completion result publication never calls this hook.
    pub fn active_identity(&self) -> Option<&VerifiedIdentity> {
        // Report identity only while the native activation remains current.
        self.current_service_authority().ok()?; // An occupied activation Option alone is not authorization after broker revocation.
        Some(&self.identity) // Identity inspection does not keep the V8 helper alive.
    } // Immutable package metadata remains available separately through package().
    /// Read-only original instance limits, never a newly configured ceiling.
    pub fn engine_limits(&self) -> &crate::engine::EngineLimits {
        self.helper.engine_limits()
    }
    pub fn plan(&self) -> &AnimationPlan {
        &self.plan
    }
    /// The exact reviewed package bytes and accepted no-input plan are bound
    /// to the pre-evaluation ambient seed. Any other package/mode fails closed.
    pub fn certify_procedural_replay(&self) -> Result<crate::replay::ReplayCertification> {
        if self.mode != AnimationMode::PreRendered
            || self.creation != Some(CreateState::Ready)
            || self.helper_retired
            || self.current_service_authority().is_err()
        {
            return Err(AnimationError::PermissionDenied(
                "replay preparation owner is not active".into(),
            ));
        }
        if self.replay_request_count != self.replay_pure_yield_count
            || self.replay_pure_yield_count != self.replay_completed_yield_count
        {
            return Err(AnimationError::PermissionDenied(
                "unqualified or unsettled native host requests occurred during replay creation"
                    .into(),
            ));
        }
        crate::replay::ReplayCertification::sealed_procedural(
            &self.package,
            &self.plan,
            &self.settings,
            crate::replay::ReplayExecutionIdentity {
                ambient_seed: self.ambient_seed,
                bootstrap_digest: self.ambient_bootstrap_digest,
                environment_digest: self.environment_digest,
                helper_build_digest: self.helper_build_digest,
            },
            self.replay_completed_yield_count,
        )
    }
    /// Certified finite Video recordings are distinct from the procedural
    /// source-free profile. The native Video owner supplies the exact count of
    /// successful originally acquired sources, not guest IDs or a plan claim.
    pub fn certify_recorded_video_replay(
        &self,
        acquired_video_count: usize,
    ) -> Result<crate::replay::ReplayCertification> {
        if self.mode != AnimationMode::PreRendered
            || self.creation != Some(CreateState::Ready)
            || self.helper_retired
            || self.current_service_authority().is_err()
            || acquired_video_count == 0
            || acquired_video_count > 8
            || self.replay_video_open_count != acquired_video_count as u64
            || self
                .replay_pure_yield_count
                .checked_add(self.replay_video_open_count)
                != Some(self.replay_request_count)
            || self.replay_pure_yield_count != self.replay_completed_yield_count
        {
            return Err(AnimationError::PermissionDenied(
                "recorded replay lacks exact settled Video source inventory".into(),
            ));
        }
        crate::replay::ReplayCertification::sealed_recorded_video(
            &self.package,
            &self.plan,
            &self.settings,
            crate::replay::ReplayExecutionIdentity {
                ambient_seed: self.ambient_seed,
                bootstrap_digest: self.ambient_bootstrap_digest,
                environment_digest: self.environment_digest,
                helper_build_digest: self.helper_build_digest,
            },
            self.replay_completed_yield_count,
            self.replay_video_open_count,
        )
    }
    /// Certify a source-backed clip only when every create-time host request
    /// is accounted for by the exact captured feeds, one native freeze, or a
    /// settled pure yield. The capture digest and recording identity are then
    /// bound again by ClipSpec against the retained FrozenInputs.
    #[cfg(all(
        feature = "native-host",
        feature = "native-network",
        feature = "v8-runtime"
    ))]
    pub fn certify_source_capture_replay(
        &self,
        frozen: &crate::replay::FrozenInputs,
        captures: &[Arc<crate::native_source_capture::NativeCapturedFeed>],
    ) -> Result<crate::replay::ReplayCertification> {
        let source_count = self.replay_source_open_count;
        if self.mode != AnimationMode::PreRendered
            || self.creation != Some(CreateState::Ready)
            || self.helper_retired
            || self.current_service_authority().is_err()
            || source_count == 0
            || self.replay_video_open_count != 0
            || self.replay_source_freeze_count != 1
            || frozen.snapshots().len() as u64 != source_count
            || captures.len() as u64 != source_count
            || self
                .replay_pure_yield_count
                .checked_add(source_count)
                .and_then(|count| count.checked_add(self.replay_source_freeze_count))
                != Some(self.replay_request_count)
            || self.replay_pure_yield_count != self.replay_completed_yield_count
        {
            return Err(AnimationError::PermissionDenied(
                "source replay lacks an exact settled capture request inventory".into(),
            ));
        }
        let mut lineage_by_id = BTreeMap::new();
        for (capture, snapshot) in captures.iter().zip(frozen.snapshots()) {
            capture.verify_frozen_snapshot(self, snapshot)?;
            for lineage in capture.replay_lineage() {
                match lineage_by_id.get(&lineage.request_id) {
                    Some(existing) if existing != lineage => {
                        return Err(AnimationError::PermissionDenied(
                            "source replay grant lineage changed".into(),
                        ));
                    }
                    Some(_) => {}
                    None => {
                        lineage_by_id.insert(lineage.request_id.clone(), lineage.clone());
                    }
                }
            }
        }
        if lineage_by_id
            .into_values()
            .ne(frozen.lineage().iter().cloned())
        {
            return Err(AnimationError::PermissionDenied(
                "source replay frozen grant lineage differs from native captures".into(),
            ));
        }
        crate::replay::ReplayCertification::sealed_source_capture(
            &self.package,
            &self.plan,
            &self.settings,
            crate::replay::ReplayExecutionIdentity {
                ambient_seed: self.ambient_seed,
                bootstrap_digest: self.ambient_bootstrap_digest,
                environment_digest: self.environment_digest,
                helper_build_digest: self.helper_build_digest,
            },
            self.replay_completed_yield_count,
            source_count,
            frozen,
        )
    }
    #[cfg(all(
        feature = "native-host",
        feature = "native-network",
        feature = "v8-runtime"
    ))]
    pub fn certify_source_sequence_replay(
        &self,
        frozen: &crate::replay::FrozenInputs,
        sequence: &crate::replay::FrozenSourceSequence,
        captures: &[(
            String,
            Arc<crate::native_source_capture::NativeCapturedFeed>,
        )],
    ) -> Result<crate::replay::ReplayCertification> {
        let source_count = self.replay_source_open_count;
        if self.mode != AnimationMode::PreRendered
            || self.creation != Some(CreateState::Ready)
            || self.helper_retired
            || self.current_service_authority().is_err()
            || source_count == 0
            || frozen.snapshots().len() as u64 != source_count
            || sequence.frames().is_empty()
            || captures.is_empty()
            || self.replay_video_open_count != 0
            || self.replay_source_freeze_count != 0
            || self.replay_source_sequence_count != 1
            || self
                .replay_pure_yield_count
                .checked_add(source_count)
                .and_then(|count| count.checked_add(self.replay_source_sequence_count))
                != Some(self.replay_request_count)
            || self.replay_pure_yield_count != self.replay_completed_yield_count
        {
            return Err(AnimationError::PermissionDenied(
                "source sequence lacks an exact settled capture request inventory".into(),
            ));
        }
        let mut observed = BTreeSet::new();
        let mut lineage_by_id = BTreeMap::new();
        for (handle_id, capture) in captures {
            let summary = capture.summary(self)?;
            let identity = (handle_id.clone(), summary.source_revision);
            if !observed.insert(identity.clone()) {
                return Err(AnimationError::PermissionDenied(
                    "source sequence repeated a native capture identity".into(),
                ));
            }
            let snapshot = sequence
                .frames()
                .iter()
                .flat_map(|frame| frame.snapshots())
                .find(|snapshot| {
                    snapshot.handle_id() == identity.0 && snapshot.revision() == identity.1
                })
                .ok_or_else(|| {
                    AnimationError::PermissionDenied(
                        "source sequence contains a revision without its original native capture"
                            .into(),
                    )
                })?;
            capture.verify_frozen_snapshot(self, snapshot)?;
            for lineage in capture.replay_lineage() {
                match lineage_by_id.get(&lineage.request_id) {
                    Some(existing) if existing != lineage => {
                        return Err(AnimationError::PermissionDenied(
                            "source sequence grant lineage changed".into(),
                        ));
                    }
                    Some(_) => {}
                    None => {
                        lineage_by_id.insert(lineage.request_id.clone(), lineage.clone());
                    }
                }
            }
        }
        let expected: BTreeSet<_> = sequence
            .frames()
            .iter()
            .flat_map(|frame| frame.snapshots())
            .map(|snapshot| (snapshot.handle_id().to_owned(), snapshot.revision()))
            .collect();
        let initial_frame = sequence.frames().first().ok_or_else(|| {
            AnimationError::PermissionDenied("source sequence initial frame is absent".into())
        })?;
        if initial_frame.snapshots().len() != frozen.snapshots().len() {
            return Err(AnimationError::PermissionDenied(
                "source sequence initial frame differs from frozen input inventory".into(),
            ));
        }
        for snapshot in frozen.snapshots() {
            let (handle_id, capture) = captures
                .iter()
                .find(|(handle_id, capture)| {
                    handle_id == snapshot.handle_id()
                        && capture
                            .summary(self)
                            .is_ok_and(|summary| summary.source_revision == snapshot.revision())
                })
                .ok_or_else(|| {
                    AnimationError::PermissionDenied(
                        "frozen initial source lacks its original native capture".into(),
                    )
                })?;
            if !initial_frame.snapshots().iter().any(|initial| {
                initial.handle_id() == handle_id && initial.revision() == snapshot.revision()
            }) {
                return Err(AnimationError::PermissionDenied(
                    "frozen initial source is absent from sequence start".into(),
                ));
            }
            capture.verify_frozen_snapshot(self, snapshot)?;
        }
        if observed != expected
            || lineage_by_id
                .into_values()
                .ne(frozen.lineage().iter().cloned())
        {
            return Err(AnimationError::PermissionDenied(
                "source sequence capture inventory or grant lineage differs from native owners"
                    .into(),
            ));
        }
        crate::replay::ReplayCertification::sealed_source_sequence(
            &self.package,
            &self.plan,
            &self.settings,
            crate::replay::ReplayExecutionIdentity {
                ambient_seed: self.ambient_seed,
                bootstrap_digest: self.ambient_bootstrap_digest,
                environment_digest: self.environment_digest,
                helper_build_digest: self.helper_build_digest,
            },
            self.replay_completed_yield_count,
            source_count,
            frozen,
            sequence,
        )
    }
    pub fn check_recorded_video_replay_requests(&self, acquired_video_count: usize) -> Result<()> {
        if self.mode != AnimationMode::PreRendered
            || self.replay_video_open_count != acquired_video_count as u64
            || self
                .replay_pure_yield_count
                .checked_add(self.replay_video_open_count)
                != Some(self.replay_request_count)
            || self.replay_pure_yield_count != self.replay_completed_yield_count
        {
            return Err(AnimationError::PermissionDenied(
                "recorded replay issued a new external request".into(),
            ));
        }
        Ok(())
    }
    #[cfg(all(
        feature = "native-host",
        feature = "native-network",
        feature = "v8-runtime"
    ))]
    pub fn check_source_capture_replay_requests(&self, captured_source_count: usize) -> Result<()> {
        if self.mode != AnimationMode::PreRendered
            || captured_source_count == 0
            || self.replay_video_open_count != 0
            || self.replay_source_open_count != captured_source_count as u64
            || self.replay_source_freeze_count != 1
            || self
                .replay_pure_yield_count
                .checked_add(self.replay_source_open_count)
                .and_then(|count| count.checked_add(self.replay_source_freeze_count))
                != Some(self.replay_request_count)
            || self.replay_pure_yield_count != self.replay_completed_yield_count
        {
            return Err(AnimationError::PermissionDenied(
                "source replay observed an unrecorded native request".into(),
            ));
        }
        Ok(())
    }
    #[cfg(all(
        feature = "native-host",
        feature = "native-network",
        feature = "v8-runtime"
    ))]
    pub fn check_source_sequence_replay_requests(&self, opened_source_count: usize) -> Result<()> {
        if self.mode != AnimationMode::PreRendered
            || opened_source_count == 0
            || self.replay_video_open_count != 0
            || self.replay_source_open_count != opened_source_count as u64
            || self.replay_source_freeze_count != 0
            || self.replay_source_sequence_count != 1
            || self
                .replay_pure_yield_count
                .checked_add(self.replay_source_open_count)
                .and_then(|count| count.checked_add(self.replay_source_sequence_count))
                != Some(self.replay_request_count)
            || self.replay_pure_yield_count != self.replay_completed_yield_count
        {
            return Err(AnimationError::PermissionDenied(
                "source sequence replay observed an unrecorded native request".into(),
            ));
        }
        Ok(())
    }
    /// Sample-to-sample native work must remain empty for this source-free
    /// profile. A newly issued request invalidates the incomplete clip.
    pub fn check_procedural_replay_requests(&self) -> Result<()> {
        if self.mode != AnimationMode::PreRendered
            || self.replay_request_count != self.replay_pure_yield_count
            || self.replay_pure_yield_count != self.replay_completed_yield_count
        {
            return Err(AnimationError::PermissionDenied(
                "procedural replay issued an external host request".into(),
            ));
        }
        Ok(())
    }
    pub fn ambient_seed(&self) -> u32 {
        self.ambient_seed
    }
    pub fn package(&self) -> &Package {
        &self.package
    }

    /// Read original child/pipe-worker exit evidence; logical retirement alone
    /// never satisfies this predicate.
    pub fn is_physically_retired(&self) -> bool {
        self.helper_retired && self.helper.is_physically_retired()
    }

    /// Native adapters retain this opaque evidence before helper retirement.
    /// It exposes neither script data nor a writable retirement bit.
    pub(crate) fn helper_retirement_evidence(&self) -> HelperRetirementEvidence {
        HelperRetirementEvidence {
            retired: Arc::clone(&self.replay_retired_proof),
        }
    }

    #[cfg(test)]
    pub(crate) fn arm_complete_service_ack_failure_for_test(&self) -> Result<()> {
        self.helper.arm_complete_service_ack_failure_for_test()
    }

    #[cfg(test)]
    pub(crate) fn helper_transport_test_snapshot(
        &self,
    ) -> crate::helper::HelperTransportTestSnapshot {
        self.helper.transport_test_snapshot()
    }

    #[cfg(test)]
    pub(crate) fn helper_sequence_for_test(&self) -> u64 {
        self.helper.sequence_for_test()
    }

    #[cfg(test)]
    pub(crate) fn helper_has_pending_service_request_for_test(&self, id: u64) -> bool {
        self.helper.has_pending_service_request_for_test(id)
    }

    pub fn retire_helper(&mut self) -> Result<()> {
        // End package execution without revoking the independently owned native activation.
        self.helper_retired = true; // Logical retirement becomes permanent before signalling or waiting for the child.
        self.creation = None; // The retired V8 instance may never be pumped, rendered, seeded, or acknowledged again.
        self.helper.cancel()?; // Failed shutdown or worker join remains a failed physical retirement on every later call.
        if !self.helper.is_physically_retired() {
            return Err(AnimationError::Runtime(
                "helper cancellation did not prove physical retirement".into(),
            ));
        } // Require explicit child and worker exit evidence even after a nominal cancellation return.
        self.replay_retired_proof.store(true, Ordering::Release);
        Ok(()) // Native activation, broker decisions, retained clips, and actual native jobs remain separately owned.
    } // Pre-rendered playback must still use with_playback_authority at actual emission.
    pub fn revoke_activation(&mut self) -> Result<Option<Invalidation>> {
        self.creation = None;
        let had_pending = self.pending_resolution.take().is_some();
        let active = self.activation.take();
        if !had_pending && active.is_none() {
            return Ok(None);
        }
        let instance_id = active
            .as_ref()
            .map_or(self.helper.authority().instance_id, |active| {
                active.plan.instance_id
            });
        // Removing local authority precedes locking; poisoning never restores it.
        let mut broker = lock_broker(&self.broker)?;
        Ok(Some(broker.retire(instance_id)))
    }
    pub fn stop(&mut self) -> StoppedInstance {
        let (invalidation, authority_error) = match self.revoke_activation() {
            Ok(invalidation) => (invalidation, None),
            Err(error) => (None, Some(error)),
        };
        // All mutex guards have exited BEFORE signalling/joining helper workers.
        let cancellation = self.retire_helper();
        StoppedInstance {
            invalidation,
            cancellation,
            authority_error,
        }
    }
}

#[cfg(test)]
mod shared_authority_tests {
    use super::*;
    fn broker() -> PermissionBroker {
        PermissionBroker::new(
            crate::permissions::PackageIdentity::unverified("test".into(), b"shared-owner-fixture")
                .unwrap(),
            Ceiling {
                permissions: vec![],
            },
            Ceiling {
                permissions: vec![],
            },
        )
        .unwrap()
    }
    #[test]
    fn genuine_shared_channel_rechecks_same_owner_and_retirement() {
        let owner = Arc::new(Mutex::new(broker()));
        let other = Arc::clone(&owner);
        let active = {
            let mut broker = lock_broker(&owner).unwrap();
            let review = broker
                .prepare(
                    1,
                    1,
                    crate::permissions::PermissionPlan {
                        permissions: vec![],
                        demands: vec![],
                    },
                    BTreeMap::new(),
                )
                .unwrap();
            broker
                .resolve(review, BTreeMap::new())
                .unwrap()
                .activation
                .unwrap()
        };
        let guard = lock_broker(&owner).unwrap();
        assert_eq!(authority_from(&guard, &active).unwrap().instance_id, 1);
        assert!(matches!(
            other.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        drop(guard);
        let invalidation = lock_broker(&other).unwrap().retire(1);
        assert_eq!(invalidation.instance_ids, vec![1]);
        assert!(invalidation.operations.is_empty()); // This broker-only fixture has no native work to cancel.
        assert!(authority_from(&lock_broker(&owner).unwrap(), &active).is_err());
        assert!(authority_from(&broker(), &active).is_err());
    }
    #[test]
    fn retained_frame_owner_uses_original_channel_at_output_boundary() {
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
            worker_threads: 1,
            worker_bytes: 8192,
        });
        let owner = Arc::new(Mutex::new(broker()));
        let active = {
            let mut broker = lock_broker(&owner).unwrap();
            let review = broker
                .prepare(
                    1,
                    1,
                    crate::permissions::PermissionPlan {
                        permissions: vec![],
                        demands: vec![],
                    },
                    BTreeMap::new(),
                )
                .unwrap();
            broker
                .resolve(review, BTreeMap::new())
                .unwrap()
                .activation
                .unwrap()
        };
        let expected = HelperAuthority {
            package_digest: "genuine-test-package".into(),
            instance_id: active.plan.instance_id,
            plan_generation: active.plan.plan_revision,
            authorization_epoch: active.plan.authorization_epoch,
        };
        let foreign = RetainedFrameAuthority {
            broker: Arc::new(Mutex::new(broker())),
            channel: active.channel.clone(),
            expected: expected.clone(),
            quota: quota.clone(),
            _storage: quota.reserve_external_storage(4096).unwrap(),
        };
        assert!(matches!(
            foreign.begin_output(&expected),
            Err(AnimationError::PermissionDenied(_))
        ));
        drop(foreign);
        let retained = RetainedFrameAuthority {
            broker: Arc::clone(&owner),
            channel: active.channel,
            expected: expected.clone(),
            quota: quota.clone(),
            _storage: quota.reserve_external_storage(4096).unwrap(),
        };
        let mut copied = expected.clone();
        copied.plan_generation += 1;
        assert!(matches!(
            retained.begin_output(&copied),
            Err(AnimationError::PermissionDenied(_))
        ));
        copied.plan_generation = expected.plan_generation;
        copied.authorization_epoch += 1;
        assert!(matches!(
            retained.begin_output(&copied),
            Err(AnimationError::PermissionDenied(_))
        ));
        let stamp = TerminalFrameStamp::for_native_presentation();
        let other_stamp = TerminalFrameStamp::for_native_presentation();
        let committed_success = retained
            .begin_output_for_presentation(&expected, Arc::clone(&stamp))
            .unwrap();
        let committed_uncertain = retained.begin_output(&expected).unwrap();
        assert_eq!(lock_broker(&owner).unwrap().pending_frame_emissions(), 2);
        let invalidation = lock_broker(&owner).unwrap().retire(expected.instance_id);
        assert_eq!(invalidation.instance_ids, vec![expected.instance_id]);
        assert!(invalidation.operations.is_empty()); // Frame commits have separate retained custody below.
        assert_eq!(lock_broker(&owner).unwrap().pending_frame_emissions(), 2);
        committed_uncertain.backend_failed_uncertain().unwrap();
        // The uncertain debit has retired, so a denial here cannot be
        // mistaken for quota exhaustion while the valid token remains live.
        assert!(matches!(
            retained.begin_output(&expected),
            Err(AnimationError::PermissionDenied(_))
        ));
        // Genuine effects committed before retirement settle afterward.
        drop(retained);
        assert_eq!(quota.snapshot().worker_bytes, 2048);
        let proof = committed_success.backend_flushed().unwrap();
        let replay_authority = ReplayAuthority {
            package_digest: expected.package_digest.clone(),
            instance_id: expected.instance_id,
            revision: expected.plan_generation,
            authorization_epoch: expected.authorization_epoch,
        };
        assert!(proof.belongs_to_frame(&owner, &replay_authority, &stamp));
        assert!(!proof.belongs_to_frame(&owner, &replay_authority, &other_stamp));
        drop(proof);
        assert_eq!(lock_broker(&owner).unwrap().pending_frame_emissions(), 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn procedural_replay_adapter_rechecks_private_broker_and_retirement_gate() {
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
            worker_threads: 1,
            worker_bytes: 8192,
        });
        let owner = Arc::new(Mutex::new(broker()));
        let active = {
            let mut broker = lock_broker(&owner).unwrap();
            let review = broker
                .prepare(
                    1,
                    1,
                    crate::permissions::PermissionPlan {
                        permissions: vec![],
                        demands: vec![],
                    },
                    BTreeMap::new(),
                )
                .unwrap();
            broker
                .resolve(review, BTreeMap::new())
                .unwrap()
                .activation
                .unwrap()
        };
        let authority = ReplayAuthority {
            package_digest: "genuine-test-package".into(),
            instance_id: active.plan.instance_id,
            revision: active.plan.plan_revision,
            authorization_epoch: active.plan.authorization_epoch,
        };
        let adapter = BrokerReplayAuthorization {
            broker: Arc::clone(&owner),
            channel: active.channel,
            expected: authority.clone(),
            retired: Arc::new(AtomicBool::new(false)),
            _storage: quota.reserve_external_storage(4096).unwrap(),
        };
        assert!(adapter
            .check(&authority, &[], ReplayAccess::Prepare)
            .is_ok());
        assert!(matches!(
            adapter.check(&authority, &[], ReplayAccess::Playback),
            Err(AnimationError::PermissionDenied(_))
        ));
        let mut stale = authority.clone();
        stale.revision += 1;
        assert!(matches!(
            adapter.check(&stale, &[], ReplayAccess::Prepare),
            Err(AnimationError::PermissionDenied(_))
        ));
        let invalidation = lock_broker(&owner).unwrap().retire(authority.instance_id);
        assert_eq!(invalidation.instance_ids, vec![authority.instance_id]);
        assert!(invalidation.operations.is_empty()); // No native operation was issued by this fixture.
        assert!(matches!(
            adapter.check(&authority, &[], ReplayAccess::Prepare),
            Err(AnimationError::PermissionDenied(_))
        ));
    }
    #[test]
    fn poisoned_shared_authority_is_never_recovered() {
        let owner = Arc::new(Mutex::new(broker()));
        let held = Arc::clone(&owner);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = owner.lock().unwrap();
            panic!("synthetic task-local poisoned authority");
        }));
        assert!(panic.is_err());
        assert!(lock_broker(&owner).is_err());
        assert!(lock_broker(&held).is_err());
    }
    #[test]
    fn original_admission_survives_last_shared_native_owner() {
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
            worker_threads: 1,
            worker_bytes: 8192,
        });
        let admission = quota.reserve_external_storage(4096).unwrap();
        let mut broker = broker();
        broker.retain_authority_storage(admission).unwrap();
        let owner = Arc::new(Mutex::new(broker));
        let source_owner = Arc::clone(&owner);
        drop(owner);
        assert_eq!(quota.snapshot().worker_bytes, 4096);
        drop(source_owner);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}

#[cfg(test)]
mod permission_observation_tests {
    use super::*;
    use crate::permissions::{Capability, Grant, Right, Scope};
    #[test]
    fn informational_snapshot_has_actual_scope_epoch_and_no_fabricated_handle() {
        let grant = Grant {
            request_id: "pointer".into(),
            right: Right {
                id: Capability::InputPointer,
                scope: Scope::AnimationViewport,
            },
            binding: None,
        };
        assert_eq!(
            permission_observation(&grant, 7).unwrap(),
            serde_json::json!({
                "request_id":"pointer", "id":"input.pointer",
                "scope":{"kind":"animation_viewport"}, "epoch":7,
            })
        );
        assert!(permission_observation(&grant, 0).is_err());
        assert!(permission_observation(&grant, 9_007_199_254_740_992).is_err());
    }
}

#[cfg(test)]
mod activation_observation_tests {
    use super::*;
    use crate::permissions::{Capability, PackageIdentity, Right, Scope};
    fn resolution(creation: Option<CreateState>) -> InstanceResolution {
        let identity =
            PackageIdentity::unverified("activation-observation".into(), b"fixture").unwrap();
        let mut broker = PermissionBroker::new(
            identity,
            Ceiling {
                permissions: vec![],
            },
            Ceiling {
                permissions: vec![],
            },
        )
        .unwrap();
        let invalidation = broker
            .revoke(Right {
                id: Capability::InputPointer,
                scope: Scope::AnimationViewport,
            })
            .unwrap();
        InstanceResolution {
            invalidation,
            denied_required: vec![],
            creation,
            teardown_error: None,
            activation_invalidation: None,
            authority_error: None,
            creation_error: None,
        }
    }
    #[test]
    fn accepted_creation_preserves_actual_pending_and_ready_states() {
        assert_eq!(resolution(None).accepted_creation(), None);
        assert_eq!(
            resolution(Some(CreateState::Pending)).accepted_creation(),
            Some(CreateState::Pending)
        );
        assert_eq!(
            resolution(Some(CreateState::Ready)).accepted_creation(),
            Some(CreateState::Ready)
        );
    }
    #[test]
    fn every_independent_failure_blocks_even_a_ready_creation_observation() {
        for failure in 0..5 {
            let mut value = resolution(Some(CreateState::Ready));
            match failure {
                0 => value.denied_required.push("pointer".into()),
                1 => {
                    value.creation_error =
                        Some(AnimationError::Runtime("fixture startup failure".into()))
                }
                2 => {
                    value.authority_error =
                        Some(AnimationError::Runtime("fixture authority failure".into()))
                }
                3 => {
                    value.teardown_error =
                        Some(AnimationError::Runtime("fixture teardown failure".into()))
                }
                _ => value.activation_invalidation = Some(resolution(None).invalidation),
            }
            assert_eq!(value.accepted_creation(), None, "failure {failure}");
        }
    }
}

// Test-only forcing of the exact logical-before-physical retirement interval.
// No mutator or additional publication gate exists in a production build.
#[cfg(test)]
mod retirement_boundary_tests {
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux sandbox"]
    fn logical_retirement_without_physical_exit_refuses_actual_playback_sink() {
        use crate::{
            engine::CreateState,
            helper::{isolation_qualification as fixture, HelperLimits},
            manifest::AnimationMode,
            permissions::Ceiling,
            runtime::{InstancePreparation, PackageInstance},
            trust::TrustVerifier,
        };
        use serde_json::json;
        use std::{collections::BTreeMap, path::Path};
        let bytes = fixture::archive(
            "export function plan(){return {format:'gray32',fps:30,inputs:{}}} export async function create(){if(fixture_seed_calls!==1)throw Error('missing_fixture_seed_activation');return {render(){},dispose(){}}}",
        );
        let trusted_bootstrap = format!(
            "{}\n{}",
            fixture::BOOTSTRAP,
            r#"
            globalThis.fixture_seed_calls = 0;
            globalThis.__ilium_seed_frame = (metadata, planes) => {
                if (metadata.frame !== null || !metadata.host ||
                    !Array.isArray(metadata.host.permissions) || metadata.host.permissions.length !== 0 ||
                    metadata.host.cancelled !== false || metadata.host.package.id !== 'helper-qualification' ||
                    !Number.isSafeInteger(metadata.host.selection.generation) || metadata.host.selection.generation <= 0 ||
                    !Number.isSafeInteger(metadata.host.selection.authorization_epoch) || metadata.host.selection.authorization_epoch <= 0 ||
                    Reflect.ownKeys(planes).length !== 0) throw Error('unexpected_fixture_host_seed');
                if (++fixture_seed_calls !== 1) throw Error('repeated_fixture_seed_activation');
            };
        "#
        );
        let quota = fixture::quota();
        let executable =
            std::env::var("ILIUM_ANIMATION_HELPER").expect("actual helper absolute path required");
        let verifier = TrustVerifier::from_release_inventory(vec![]).unwrap();
        let settings = json!({});
        let environment = json!({});
        let verified = PackageInstance::verify(InstancePreparation {
            archive: &bytes,
            verifier: &verifier,
            helper_executable: Path::new(&executable),
            trusted_bootstrap: &trusted_bootstrap,
            settings: &settings,
            mode: AnimationMode::Live,
            environment: &environment,
            host_policy: Ceiling {
                permissions: vec![],
            },
            instance_id: 73,
            limits: HelperLimits::default(),
            quota: quota.clone(),
        })
        .unwrap();
        let (mut instance, review) = verified.prepare_without_rights().unwrap();
        let pending = instance.begin_resolution(review, BTreeMap::new()).unwrap();
        let resolution = instance.finish_resolution(pending).unwrap();
        assert_eq!(resolution.accepted_creation(), Some(CreateState::Ready));
        let expected = instance.frame_authority().unwrap();
        assert!(!instance.helper.is_physically_retired());
        assert!(!instance
            .replay_retired_proof
            .load(std::sync::atomic::Ordering::Acquire));
        // Force only the real native logical state assigned by retire_helper BEFORE
        // cancellation/join. Do not fabricate a child, activation, physical proof,
        // grant, successful join or configured producer limit.
        instance.helper_retired = true;
        assert!(!instance.is_physically_retired());
        let mut emitted = 0;
        assert!(instance
            .with_playback_authority(&expected, || {
                emitted += 1;
            })
            .is_err());
        assert_eq!(emitted, 0);
        assert_eq!(instance.frame_authority(), Some(expected));
        // Retire the actual test-owned helper through its ordinary physical path.
        instance.retire_helper().unwrap();
        assert!(instance.is_physically_retired());
        assert!(instance
            .replay_retired_proof
            .load(std::sync::atomic::Ordering::Acquire));
        drop(instance);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
