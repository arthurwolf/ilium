//! Native client-only permission activation: opaque load, genuine review, durable
//! commit, then create. No script/helper endpoint exposes this controller or token.
use super::{
    ordered::WriteId,
    plugin_permissions::{
        DurableLedgerReceipt, LedgerSnapshot, PermissionCompletion, PermissionFence,
        PersistenceError, PersistenceStamp, PluginPermissionFiles,
    },
};
use ilium_animation_js::{
    native_audio_capture::QualifiedCaptureBinding,
    native_storage::SelectedStorage,
    permissions::{Invalidation, PlanReview, UserChoice},
    runtime::{
        InstanceResolution, PackageInstance, PendingResolution, RetainedReplayRevoker,
        StoppedInstance, VerifiedPreparation,
    },
};
use ilium_execution::{Client, QuotaGroup, Retained, StorageAdmission};
use ilium_platform::owned_worker::StopToken;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

type Result<T> = std::result::Result<T, String>;
type LoadedOutcome = Retained<std::result::Result<LedgerSnapshot, PersistenceError>>;
type WrittenOutcome = Retained<std::result::Result<DurableLedgerReceipt, PersistenceError>>;

#[derive(Clone, Copy)]
pub(super) struct WriteExpectation {
    pub(super) write_id: WriteId,
    pub(super) storage_stamp: PersistenceStamp,
    pub(super) candidate_digest: [u8; 32],
    pub(super) candidate_epoch: u64,
    pub(super) native_plan_revision: u64,
}
/// These numbers only correlate a REAL native durable receipt. This function
/// issues no authority/token and cannot turn JSON/readback into durability.
pub(super) fn receipt_matches(
    receipt: &DurableLedgerReceipt,
    expected: WriteExpectation,
    fence: &Arc<PermissionFence>,
) -> bool {
    receipt.is_current()
        && receipt.matches_fence(fence)
        && receipt.write_id() == expected.write_id
        && receipt.stamp() == expected.storage_stamp
        && receipt.digest() == expected.candidate_digest
        && receipt.epoch() == expected.candidate_epoch
}

pub(crate) enum ActivationUpdate {
    Review(PlanReview),
    Finished(InstanceResolution),
    /// Retained outcomes stay in the controller through actual collection.
    /// Caller must process stop.invalidation and pending original invalidation.
    Failed {
        message: String,
        stop: Option<StoppedInstance>,
    },
}

/// One admitted native workflow owner. Pending token and real IO owners are
/// PRIVATE; callers receive genuine review/effect inventories, never grant IDs.
pub(crate) struct PermissionCancellation {
    pub(crate) stop: Option<StoppedInstance>,
    pub(crate) delegated_invalidation: Option<Invalidation>,
    pub(crate) delegated_authority_error: Option<String>,
    pub(crate) persistence_error: Option<String>,
}
struct DelegatedInstance {
    instance: Arc<Mutex<PackageInstance>>,
    revoker: RetainedReplayRevoker,
    stop: StopToken,
}

pub(crate) struct PluginPermissionController {
    quota: QuotaGroup,
    files: PluginPermissionFiles,
    verified: Option<VerifiedPreparation>,
    instance: Option<PackageInstance>,
    delegated: Option<DelegatedInstance>,
    fence: Option<Arc<PermissionFence>>,
    loaded: Option<LoadedOutcome>,
    written: Option<WrittenOutcome>,
    retirement_loaded: Option<LoadedOutcome>,
    retirement_written: Option<WrittenOutcome>,
    pending: Option<PendingResolution>,
    expected_write: Option<WriteExpectation>,
    selection_revision: Option<u64>,
    selected_storage: BTreeMap<String, Arc<SelectedStorage>>,
    selected_audio: BTreeMap<String, QualifiedCaptureBinding>,
    blocked: Option<String>,
    finished: bool,
    failure_reported: bool,
    _storage: StorageAdmission,
}
impl PluginPermissionController {
    pub(crate) fn new(files: PluginPermissionFiles, client: &Client) -> Result<Self> {
        if !files.shares_root(client) {
            return Err("Permission controller has a foreign original quota root".into());
        }
        let storage = client
            .quota_group()
            .reserve_external_storage(64 * 1024)
            .map_err(|error| format!("Permission controller admission: {error:?}"))?;
        Ok(Self {
            quota: client.quota_group(),
            files,
            verified: None,
            instance: None,
            delegated: None,
            fence: None,
            loaded: None,
            written: None,
            retirement_loaded: None,
            retirement_written: None,
            pending: None,
            expected_write: None,
            selection_revision: None,
            selected_storage: BTreeMap::new(),
            selected_audio: BTreeMap::new(),
            blocked: None,
            finished: false,
            failure_reported: false,
            _storage: storage,
        })
    }
    /// Selection revision is native owning-worker correlation, never script JSON.
    pub(crate) fn start(
        &mut self,
        verified: VerifiedPreparation,
        selection_revision: u64,
    ) -> Result<()> {
        if !verified.shares_root(&self.quota) {
            return Err("Verified package has a foreign original quota root".into());
        }
        if self.selection_revision.is_some() || selection_revision == 0 {
            return Err("Permission workflow requires a fresh current owner".into());
        }
        let stamp = PersistenceStamp {
            selection_revision,
            instance_id: verified.instance_id(),
            plan_revision: 1,
            authorization_epoch: verified.authorization_epoch(),
        };
        let fence = self
            .files
            .bind(verified.principal(), stamp)
            .map_err(|error| error.to_string())?;
        self.files
            .request_load(&fence)
            .map_err(|error| error.to_string())?;
        self.verified = Some(verified);
        self.fence = Some(fence);
        self.selection_revision = Some(selection_revision);
        Ok(())
    }
    fn check_selection(&self, current_selection: u64) -> Result<()> {
        if self.selection_revision != Some(current_selection) || self.blocked.is_some() {
            return Err("Permission workflow selection is no longer current".into());
        }
        Ok(())
    }
    pub(crate) fn pending_invalidation(&self) -> Option<&Invalidation> {
        self.pending.as_ref().map(PendingResolution::invalidation)
    }
    pub(crate) fn notification(&self) -> Arc<Notify> {
        self.files.notification()
    }
    pub(crate) fn is_pending_io(&self) -> bool {
        self.files.is_pending()
    }
    /// Only a genuine native completion wake after failed/cancelled activation.
    /// Keep original retired outcomes alongside the previous durable receipts.
    pub(crate) fn collect_retirement_on_wake(&mut self) -> Result<()> {
        if self.blocked.is_none() {
            return Err("Permission retirement before cancellation/failure".into());
        }
        match self.files.collect() {
            Some(PermissionCompletion::Loaded(outcome)) => self.retirement_loaded = Some(outcome),
            Some(PermissionCompletion::Written(outcome)) => self.retirement_written = Some(outcome),
            Some(PermissionCompletion::Failed(error)) => return Err(error.to_string()),
            None => {}
        }
        Ok(())
    }
    /// Collect original HTTP receipts only on the finite client's real wake.
    /// No cancelled instance escapes this collector or reopens rendering.
    pub(crate) fn collect_http_retirement_on_wake(
        &mut self,
        host: &mut ilium_animation_js::native_http_host::NativeHttpHost,
    ) -> Result<Vec<ilium_animation_js::native_http_host::HttpEvent>> {
        if self.blocked.is_none() {
            return Err("HTTP retirement before cancellation/failure".into());
        }
        // Close admission before collecting. The host retains Lost/Unconfirmed
        // original receipts; its is_drained is a separate required proof.
        host.cancel_all();
        let instance = self
            .instance
            .as_mut()
            .ok_or("Original HTTP retirement instance missing")?;
        host.on_completion_wake(instance)
            .map_err(|error| error.to_string())
    }
    /// The source actor retains its original CPU/IO receipts after logical
    /// cancellation. Allow only its bounded collector to see this retired
    /// instance on a genuine finite-client completion wake.
    pub(crate) fn collect_source_retirement_on_wake<T>(
        &mut self,
        collect: impl FnOnce(&mut PackageInstance) -> Result<T>,
    ) -> Result<T> {
        if self.blocked.is_none() {
            return Err("Source retirement before cancellation/failure".into());
        }
        let instance = self
            .instance
            .as_mut()
            .ok_or("Original source retirement instance missing")?;
        collect(instance)
    }
    pub(crate) fn is_physically_settled(&self) -> bool {
        self.blocked.is_some()
            && self.files.is_physically_settled()
            && self
                .instance
                .as_ref()
                .is_none_or(PackageInstance::is_physically_retired)
            && self.delegated.as_ref().is_none_or(|owner| {
                owner
                    .instance
                    .try_lock()
                    .is_ok_and(|instance| instance.is_physically_retired())
            })
    }
    /// Read-only preactivation inspection; no mutable instance escapes the durable gate.
    pub(crate) fn with_review_state<T>(
        &self,
        current_selection: u64,
        inspect: impl FnOnce(
            &ilium_animation_js::package::Package,
            &ilium_animation_js::permissions::PackageIdentity,
            u64,
            u64,
            u64,
        ) -> T,
    ) -> Result<T> {
        self.check_selection(current_selection)?;
        if self.finished || self.pending.is_some() {
            return Err("Permission review is no longer pending".into());
        }
        let instance_id = self
            .fence
            .as_ref()
            .ok_or("Native storage fence missing")?
            .stamp()
            .instance_id;
        self.instance
            .as_ref()
            .ok_or("Native permission load/plan is not ready")?
            .with_permission_review_state(instance_id, inspect)
            .map_err(|error| error.to_string())
    }
    pub(crate) fn package_instance(&self) -> Option<&PackageInstance> {
        if self.finished && self.blocked.is_none() {
            self.instance.as_ref()
        } else {
            None
        }
    }
    /// No mutable accepted instance escapes before the durable gate finishes.
    pub(crate) fn package_instance_mut(&mut self) -> Option<&mut PackageInstance> {
        if self.finished && self.blocked.is_none() {
            self.instance.as_mut()
        } else {
            None
        }
    }
    /// Transfer only an accepted pre-rendered instance to an admitted finite
    /// preparation owner. The controller retains original revocation, quota,
    /// persistence and physical-exit custody throughout the delegation.
    pub(crate) fn delegate_accepted_replay(
        &mut self,
        stop: StopToken,
    ) -> Result<Arc<Mutex<PackageInstance>>> {
        if !self.finished || self.blocked.is_some() || self.delegated.is_some() {
            return Err("Replay delegation requires one accepted current instance".into());
        }
        let instance = self
            .instance
            .as_ref()
            .ok_or("Accepted replay instance missing")?;
        let revoker = instance
            .retain_replay_revoker()
            .map_err(|error| error.to_string())?;
        let owned = Arc::new(Mutex::new(
            self.instance.take().ok_or("Replay instance moved")?,
        ));
        self.delegated = Some(DelegatedInstance {
            instance: Arc::clone(&owned),
            revoker,
            stop,
        });
        Ok(owned)
    }
    pub(crate) fn delegated_instance(&self) -> Option<&Arc<Mutex<PackageInstance>>> {
        self.delegated.as_ref().map(|owner| &owner.instance)
    }
    pub(crate) fn review_selected(
        &mut self,
        current_selection: u64,
        revision: u64,
        picked: Option<(String, Arc<SelectedStorage>)>,
        picked_audio: Option<(String, Option<QualifiedCaptureBinding>)>,
    ) -> Result<PlanReview> {
        self.check_selection(current_selection)?;
        if self.pending.is_some() || self.finished {
            return Err("Native resolution is already pending".into());
        }
        let instance = self
            .instance
            .as_mut()
            .ok_or("Native permission load/plan is not ready")?;
        let instance_id = self
            .fence
            .as_ref()
            .ok_or("Native storage fence missing")?
            .stamp()
            .instance_id;
        let mut proposed = self.selected_storage.clone();
        if let Some((request_id, resource)) = picked {
            if proposed.len() >= 64 && !proposed.contains_key(&request_id) {
                return Err("Selected native resource inventory is full".into());
            }
            proposed.insert(request_id, resource);
        }
        let mut audio = self.selected_audio.clone();
        if let Some((request_id, selected)) = picked_audio {
            match selected {
                Some(selected) => {
                    if audio.len() + proposed.len() >= 64 && !audio.contains_key(&request_id) {
                        return Err("Selected native resource inventory is full".into());
                    }
                    audio.insert(request_id, selected);
                }
                None => {
                    audio.remove(&request_id);
                }
            }
        }
        let review = instance
            .review_selected_resources(instance_id, revision, proposed.clone(), audio.clone())
            .map_err(|error| error.to_string())?;
        self.selected_storage = proposed;
        self.selected_audio = audio;
        Ok(review)
    }
    /// Returns the ORIGINAL native invalidation inventory. Owning worker must
    /// apply it to real acquisition/publication owners BEFORE the next poll.
    /// No new create/services/publication can occur until that subsequent poll.
    pub(crate) fn resolve(
        &mut self,
        current_selection: u64,
        review: PlanReview,
        answers: BTreeMap<String, UserChoice>,
    ) -> Result<&Invalidation> {
        self.check_selection(current_selection)?;
        if self.pending.is_some() || self.finished {
            return Err("Native resolution already consumed".into());
        }
        let instance = self.instance.as_mut().ok_or("Native plan missing")?;
        let pending = instance
            .begin_resolution(review, answers)
            .map_err(|error| error.to_string())?;
        self.pending = Some(pending); // Keep original authority/invalidation even on IO refusal.
        let pending = self
            .pending
            .as_ref()
            .ok_or("Native pending resolution missing")?;
        if let Some(error) = pending.persistence_error() {
            self.blocked = Some(error.to_string());
        } else if let Some(bytes) = pending.remembered_bytes() {
            let snapshot = self
                .loaded
                .as_ref()
                .ok_or("Native ledger outcome missing")?
                .view()
                .as_ref()
                .map_err(|error| error.to_string())?;
            let fence = self.fence.as_ref().ok_or("Native ledger fence missing")?;
            // NEVER rebind the old snapshot to a post-resolution epoch. The
            // storage fence is still exact, while runtime checks new broker epoch.
            if !snapshot.is_current() || snapshot.stamp() != fence.stamp() {
                self.blocked = Some("Native loaded ledger is stale".into());
            } else {
                match self.files.request_commit(snapshot, bytes) {
                    Ok(write_id) => {
                        self.expected_write = Some(WriteExpectation {
                            write_id,
                            storage_stamp: fence.stamp(),
                            candidate_digest: pending
                                .remembered_digest()
                                .ok_or("Native candidate digest missing")?,
                            candidate_epoch: pending.authorization_epoch(),
                            native_plan_revision: pending.plan_revision(),
                        });
                    }
                    Err(error) => self.blocked = Some(error.to_string()),
                }
            }
        }
        Ok(self
            .pending
            .as_ref()
            .ok_or("Native pending resolution missing")?
            .invalidation())
    }
    fn failed(&mut self, message: String) -> ActivationUpdate {
        self.files.close_admission();
        self.blocked = Some(message.clone());
        let stop = self.instance.as_mut().map(PackageInstance::stop);
        self.finished = false;
        self.failure_reported = true;
        ActivationUpdate::Failed { message, stop }
    }
    /// Driven by the owning worker's real completion notification. This never
    /// waits inside a finite-bank callback and introduces no polling thread.
    pub(crate) fn poll(&mut self, current_selection: u64) -> Option<ActivationUpdate> {
        if self.selection_revision != Some(current_selection) && self.selection_revision.is_some() {
            if let Some(fence) = &self.fence {
                // Busy cannot acknowledge cancellation; keep retry dependency
                // explicit and do not publish/create in either case.
                if let Err(error) = self.files.invalidate(fence) {
                    self.blocked = Some(error.to_string());
                    if !self.failure_reported {
                        return Some(self.failed(error.to_string()));
                    }
                }
            }
            self.blocked = Some("Native permission selection changed".into());
        }
        if self.blocked.is_some() {
            if !self.failure_reported {
                return Some(self.failed(self.blocked.clone().unwrap_or_default()));
            }
            // A terminal workflow still collects the ORIGINAL accepted IO job.
            // No prepare/create or grant can resume from a late durable outcome.
            match self.files.collect() {
                Some(PermissionCompletion::Loaded(outcome)) => self.loaded = Some(outcome),
                Some(PermissionCompletion::Written(outcome)) => self.written = Some(outcome),
                Some(PermissionCompletion::Failed(_)) | None => {}
            }
            return None;
        }
        match self.files.collect() {
            Some(PermissionCompletion::Loaded(outcome)) => {
                self.loaded = Some(outcome); // Hold the original IO outcome and snapshot.
                let result = (|| -> Result<PlanReview> {
                    let loaded = self
                        .loaded
                        .as_ref()
                        .ok_or("Native load outcome missing")?
                        .view()
                        .as_ref()
                        .map_err(|error| error.to_string())?;
                    if !loaded.is_current() {
                        return Err("Native ledger load is stale".into());
                    }
                    let fence = self.fence.as_ref().ok_or("Native load fence missing")?;
                    if loaded.stamp() != fence.stamp() {
                        return Err("Native load fence mismatch".into());
                    }
                    let verified = self
                        .verified
                        .take()
                        .ok_or("Native verified preparation missing")?;
                    let (instance, review) = verified
                        .prepare(loaded.bytes())
                        .map_err(|error| error.to_string())?;
                    self.instance = Some(instance);
                    Ok(review)
                })();
                return Some(match result {
                    Ok(review) => ActivationUpdate::Review(review),
                    Err(message) => self.failed(message),
                });
            }
            Some(PermissionCompletion::Written(outcome)) => {
                self.written = Some(outcome); // Durable receipt stays genuinely retained.
                let result = (|| -> Result<()> {
                    let receipt = self
                        .written
                        .as_ref()
                        .ok_or("Native write outcome missing")?
                        .view()
                        .as_ref()
                        .map_err(|error| error.to_string())?;
                    let expected = self
                        .expected_write
                        .ok_or("Native pending WriteId missing")?;
                    if !receipt_matches(
                        receipt,
                        expected,
                        self.fence
                            .as_ref()
                            .ok_or("Native issued receipt fence missing")?,
                    ) {
                        return Err("Native durable receipt is stale or mismatched".into());
                    }
                    let snapshot = self
                        .loaded
                        .as_ref()
                        .ok_or("Native loaded base missing")?
                        .view()
                        .as_ref()
                        .map_err(|error| error.to_string())?;
                    if !snapshot.is_current() || snapshot.stamp() != expected.storage_stamp {
                        return Err("Native snapshot owner changed during commit".into());
                    }
                    let pending = self
                        .pending
                        .as_ref()
                        .ok_or("Native pending authority missing")?;
                    if pending.remembered_digest() != Some(expected.candidate_digest)
                        || pending.authorization_epoch() != expected.candidate_epoch
                        || pending.instance_id() != expected.storage_stamp.instance_id
                        || pending.plan_revision() != expected.native_plan_revision
                    {
                        return Err("Native candidate differs from the durable mutation".into());
                    }
                    Ok(())
                })();
                if let Err(message) = result {
                    return Some(self.failed(message));
                }
                self.expected_write = None;
            }
            Some(PermissionCompletion::Failed(error)) => {
                return Some(self.failed(error.to_string()));
            }
            None => {}
        }
        if self.pending.is_none() || self.files.is_pending() || self.expected_write.is_some() {
            return None;
        }
        // A remembered candidate MUST have the checked real receipt retained;
        // a missing outcome is not equivalent to a successful write.
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.remembered_bytes().is_some())
            && self.written.is_none()
        {
            return Some(self.failed("Native remembered decision has no durable receipt".into()));
        }
        let pending = self.pending.take()?;
        let result = self
            .instance
            .as_mut()
            .ok_or_else(|| "Native instance missing".to_owned())
            .and_then(|instance| {
                instance
                    .finish_resolution(pending)
                    .map_err(|error| error.to_string())
            });
        Some(match result {
            Ok(resolution) => {
                self.finished = resolution.accepted_creation().is_some();
                ActivationUpdate::Finished(resolution)
            }
            Err(message) => self.failed(message),
        })
    }
    /// Block native authority first. Adapter IO custody stays retained/drained;
    /// a canceled waiter is never treated as actual worker exit or rollback.
    pub(crate) fn cancel(&mut self) -> PermissionCancellation {
        self.blocked = Some("Native permission workflow cancelled".into());
        self.finished = false;
        self.failure_reported = true;
        let stop = self.instance.as_mut().map(PackageInstance::stop);
        // The retained native broker is revoked before signalling a running
        // preparation. The helper's physical exit remains a separate predicate.
        let (delegated_invalidation, delegated_authority_error) = match self.delegated.as_mut() {
            Some(owner) => {
                let result = owner.revoker.revoke();
                owner.stop.stop();
                match result {
                    Ok(invalidation) => (invalidation, None),
                    Err(error) => (None, Some(error.to_string())),
                }
            }
            None => (None, None),
        };
        let persistence_error = self
            .fence
            .as_ref()
            .and_then(|fence| self.files.invalidate(fence).err())
            .map(|error| error.to_string());
        self.files.close_admission();
        PermissionCancellation {
            stop,
            delegated_invalidation,
            delegated_authority_error,
            persistence_error,
        }
    }
}
