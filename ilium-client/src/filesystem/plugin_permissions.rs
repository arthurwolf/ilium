//! Native, host-only remembered-decision storage. This adapter grants nothing.
//! Original IO receipts and immutable snapshots retain their original quota.
//! Linux atomic publication is provided by ilium-platform; other OSes fail closed.
use super::ordered::{OrderedWriter, WriteCompletion, WriteId};
use ilium_animation_js::{
    manifest::Capability as WireCapability,
    permission_projection,
    permissions::{PackageIdentity, Right, Scope},
};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, RejectReason, Retained,
    StorageAdmission,
};
use ilium_platform::animation_files::{PinnedDirectory, WriteMode};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, TryLockError,
    },
};
use tokio::sync::Notify;
const STATE_BYTES: usize = 256 * 1024;
const METADATA_BYTES: usize = 64 * 1024;
const FENCE_BYTES: usize = 16 * 1024;
const SNAPSHOT_BYTES: usize = STATE_BYTES + 16 * 1024;
const COST: JobCost = JobCost {
    input_bytes: 4 * 1024 * 1024,
    result_bytes: 16 * 1024 * 1024,
};
const LEAF: &str = "decisions.json";
type Result<T> = std::result::Result<T, PersistenceError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum PersistenceError {
    #[error("permission persistence admission: {0:?}")]
    Admission(RejectReason),
    #[error("permission persistence is busy")]
    Busy,
    #[error("stale or foreign permission persistence fence")]
    Stale,
    #[error("permission persistence canceled before publication")]
    Canceled,
    #[error("invalid remembered state: {0}")]
    Invalid(&'static str),
    #[error("remembered state belongs to a different native principal")]
    WrongPrincipal,
    #[error("permission ledger base changed; reload and review")]
    Conflict,
    #[error("permission ledger {stage}: {kind:?}")]
    Io {
        stage: &'static str,
        kind: io::ErrorKind,
    },
    #[error("permission ledger publication is unconfirmed at {0}")]
    PublicationUnconfirmed(&'static str),
    #[error("permission persistence completion is unavailable")]
    Lost,
}
fn io_failure(stage: &'static str, error: io::Error) -> PersistenceError {
    PersistenceError::Io {
        stage,
        kind: error.kind(),
    }
}
fn sha(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn hexadecimal(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Supplied by the native owning worker, never deserialized from a helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PersistenceStamp {
    pub selection_revision: u64,
    pub instance_id: u64,
    pub plan_revision: u64,
    pub authorization_epoch: u64,
}
struct OwnerIssuer {
    alive: AtomicBool,
}
/// The issuer, native principal and mutation gate cannot be constructed by callers.
pub(crate) struct PermissionFence {
    issuer: Arc<OwnerIssuer>,
    identity: PackageIdentity,
    principal_key: String,
    namespace: String,
    stamp: PersistenceStamp,
    current: Mutex<bool>,
    // Same charged native actor capture follows jobs and escaped snapshots/receipts.
    _actor_wake: Arc<dyn Fn() + Send + Sync>,
    _storage: StorageAdmission,
}
impl PermissionFence {
    pub(crate) fn stamp(&self) -> PersistenceStamp {
        self.stamp
    }
    fn check(&self) -> Result<()> {
        if !self.issuer.alive.load(Ordering::Acquire) {
            return Err(PersistenceError::Stale);
        }
        match self.current.try_lock() {
            Ok(current) if *current => Ok(()),
            Ok(_) => Err(PersistenceError::Canceled),
            Err(TryLockError::WouldBlock) => Err(PersistenceError::Busy),
            Err(TryLockError::Poisoned(_)) => Err(PersistenceError::Stale),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    Absent,
    Present { digest: [u8; 32], epoch: u64 },
}
/// Bytes remain opaque and immutable; native broker restoration remains required.
/// This contains no grant, live handle or deserializable native authority token.
pub(crate) struct LedgerSnapshot {
    fence: Arc<PermissionFence>,
    bytes: Option<Vec<u8>>,
    base: Base,
    directory: Option<Arc<PinnedDirectory>>,
    _storage: StorageAdmission,
}
impl LedgerSnapshot {
    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        self.bytes.as_deref()
    }
    pub(crate) fn stamp(&self) -> PersistenceStamp {
        self.fence.stamp
    }
    pub(crate) fn is_current(&self) -> bool {
        self.fence.check().is_ok()
    }
}
/// A completed file+directory flush, bound to one native pending mutation.
/// It proves durability only, not permission, current epoch, or terminal emission.
pub(crate) struct DurableLedgerReceipt {
    write_id: WriteId,
    fence: Arc<PermissionFence>,
    digest: [u8; 32],
    epoch: u64,
}
impl DurableLedgerReceipt {
    pub(crate) fn matches_fence(&self, fence: &Arc<PermissionFence>) -> bool {
        Arc::ptr_eq(&self.fence, fence)
    }

    pub(crate) fn write_id(&self) -> WriteId {
        self.write_id
    }
    pub(crate) fn stamp(&self) -> PersistenceStamp {
        self.fence.stamp
    }
    pub(crate) fn digest(&self) -> [u8; 32] {
        self.digest
    }
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }
    pub(crate) fn is_current(&self) -> bool {
        self.fence.check().is_ok()
    }
}
/// Retained outcomes keep original job costs alive through caller inspection.
pub(crate) enum PermissionCompletion {
    Loaded(Retained<Result<LedgerSnapshot>>),
    Written(Retained<Result<DurableLedgerReceipt>>),
    Failed(PersistenceError),
}

// Strict structural preflight of the actual broker v1 envelope. We never
// reconstruct/rewrite this schema or use it to authorize anything. Full native
// consuming broker restore is required again at the runtime activation boundary.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    principal_key: String,
    epoch: u64,
    block_all: bool,
    records: Vec<Record>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    right: Right,
    allowed: bool,
    remembered: bool,
    binding_hash: Option<[u8; 32]>,
    approved_hash: [u8; 32],
}
fn validate(bytes: &[u8], fence: &PermissionFence) -> Result<u64> {
    if bytes.is_empty() || bytes.len() > STATE_BYTES {
        return Err(PersistenceError::Invalid("byte bound"));
    }
    let saved: Envelope =
        serde_json::from_slice(bytes).map_err(|_| PersistenceError::Invalid("v1 envelope JSON"))?;
    if saved.version != 1
        || saved.epoch == 0
        || saved.epoch == u64::MAX
        || saved.records.len() > 256
    {
        return Err(PersistenceError::Invalid("version, epoch or record count"));
    }
    if saved.principal_key != fence.principal_key {
        return Err(PersistenceError::WrongPrincipal);
    }
    let mut normalized = Vec::with_capacity(saved.records.len());
    for record in saved.records {
        let id = serde_json::to_value(record.right.id)
            .map_err(|_| PersistenceError::Invalid("capability"))?;
        let id = id
            .as_str()
            .ok_or(PersistenceError::Invalid("capability spelling"))?
            .to_owned();
        let right = permission_projection::right(&WireCapability {
            id,
            scope: serde_json::to_value(&record.right.scope)
                .map_err(|_| PersistenceError::Invalid("scope"))?,
        })
        .map_err(|_| PersistenceError::Invalid("normalized scope"))?;
        let needs_binding = matches!(&right.scope, Scope::Disk { .. } | Scope::Audio { .. });
        if !record.remembered
            || (!record.allowed && record.binding_hash.is_some())
            || (record.allowed && needs_binding != record.binding_hash.is_some())
        {
            return Err(PersistenceError::Invalid("remembered binding"));
        }
        if fence.principal_key.starts_with("unsigned:")
            && record.approved_hash != fence.identity.content_hash()
        {
            return Err(PersistenceError::WrongPrincipal);
        }
        if normalized.contains(&right) {
            return Err(PersistenceError::Invalid("duplicate normalized scope"));
        }
        normalized.push(right);
    }
    std::hint::black_box(saved.block_all); // Preserve the exact opaque bytes, including this denial latch.
    Ok(saved.epoch)
}
fn namespace(
    root: &Arc<PinnedDirectory>,
    fence: &PermissionFence,
    create: bool,
) -> Result<Option<Arc<PinnedDirectory>>> {
    match root.child(&fence.namespace, create) {
        Ok(directory) => {
            if create {
                root.sync()
                    .map_err(|error| io_failure("namespace parent flush", error))?;
            }
            Ok(Some(Arc::new(directory)))
        }
        Err(error) if !create && error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_failure("namespace", error)),
    }
}
/// Caller owns a job reservation covering bounded read buffers/parsing first.
fn read(directory: &PinnedDirectory, fence: &PermissionFence) -> Result<(Option<Vec<u8>>, Base)> {
    let file = match directory.open_file(LEAF) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((None, Base::Absent)),
        Err(error) => return Err(io_failure("open regular ledger", error)),
    };
    let length = usize::try_from(
        file.len()
            .map_err(|error| io_failure("ledger length", error))?,
    )
    .map_err(|_| PersistenceError::Invalid("ledger length"))?;
    if length == 0 || length > STATE_BYTES {
        return Err(PersistenceError::Invalid("ledger length"));
    }
    let mut bytes = vec![0; length];
    let mut offset = 0;
    while offset < length {
        fence.check()?;
        let count = file
            .read_at(&mut bytes[offset..], offset as u64)
            .map_err(|error| io_failure("read ledger", error))?;
        if count == 0 {
            return Err(PersistenceError::Invalid("ledger truncated"));
        }
        offset += count;
    }
    let mut extra = [0];
    if file
        .read_at(&mut extra, length as u64)
        .map_err(|error| io_failure("ledger tail", error))?
        != 0
        || file
            .len()
            .map_err(|error| io_failure("final ledger length", error))? as usize
            != length
    {
        return Err(PersistenceError::Invalid("ledger changed length"));
    }
    let epoch = validate(&bytes, fence)?;
    let digest = sha(&bytes);
    Ok((Some(bytes), Base::Present { digest, epoch }))
}
struct LedgerLoad {
    root: Arc<PinnedDirectory>,
    fence: Arc<PermissionFence>,
    quota: ilium_execution::QuotaGroup,
}
impl Job for LedgerLoad {
    type Output = LedgerSnapshot;
    type Error = PersistenceError;
    fn run(self, context: JobContext) -> Result<LedgerSnapshot> {
        if context.stop_requested() {
            return Err(PersistenceError::Canceled);
        }
        self.fence.check()?;
        let storage = self
            .quota
            .reserve_external_storage(SNAPSHOT_BYTES)
            .map_err(PersistenceError::Admission)?;
        let (bytes, base, retained_directory) =
            if let Some(directory) = namespace(&self.root, &self.fence, false)? {
                let _lease = directory
                    .try_exclusive_lease()
                    .map_err(|error| io_failure("read namespace lease", error))?;
                let (bytes, base) = read(&directory, &self.fence)?;
                (bytes, base, Some(Arc::clone(&directory)))
            } else {
                (None, Base::Absent, None)
            };
        if context.stop_requested() {
            return Err(PersistenceError::Canceled);
        }
        self.fence.check()?;
        Ok(LedgerSnapshot {
            fence: self.fence,
            bytes,
            base,
            directory: retained_directory,
            _storage: storage,
        })
    }
}
struct PublishedLedger {
    fence: Arc<PermissionFence>,
    digest: [u8; 32],
    epoch: u64,
}
struct PermissionLedgerWrite {
    root: Arc<PinnedDirectory>,
    fence: Arc<PermissionFence>,
    base: Base,
    directory: Option<Arc<PinnedDirectory>>,
    bytes: Vec<u8>,
    #[cfg(test)]
    fault: Option<TestFault>,
}
impl Job for PermissionLedgerWrite {
    type Output = PublishedLedger;
    type Error = PersistenceError;
    fn run(self, context: JobContext) -> Result<PublishedLedger> {
        if context.stop_requested() {
            return Err(PersistenceError::Canceled);
        }
        self.fence.check()?;
        let epoch = validate(&self.bytes, &self.fence)?;
        if let Base::Present {
            epoch: previous, ..
        } = self.base
        {
            if epoch < previous {
                return Err(PersistenceError::Invalid("ledger epoch rollback"));
            }
        }
        let directory = namespace(&self.root, &self.fence, true)?
            .ok_or(PersistenceError::Invalid("missing created namespace"))?;
        if self
            .directory
            .as_ref()
            .is_some_and(|previous| previous.identity() != directory.identity())
        {
            return Err(PersistenceError::Conflict);
        }
        let _lease = directory
            .try_exclusive_lease()
            .map_err(|error| io_failure("write namespace lease", error))?;
        if read(&directory, &self.fence)?.1 != self.base {
            return Err(PersistenceError::Conflict);
        }
        let mut write = directory
            .begin_atomic(
                LEAF,
                if self.base == Base::Absent {
                    WriteMode::CreateNew
                } else {
                    WriteMode::ReplaceEntry
                },
            )
            .map_err(|error| io_failure("create stage", error))?;
        write
            .write(&self.bytes, STATE_BYTES)
            .map_err(|error| io_failure("stage write", error))?;
        write
            .prepare_durable()
            .map_err(|error| io_failure("stage flush", error))?;
        #[cfg(test)]
        if let Some(TestFault::InvalidateBeforePublish) = &self.fault {
            *self
                .fence
                .current
                .lock()
                .map_err(|_| PersistenceError::Stale)? = false;
        }
        #[cfg(test)]
        if let Some(TestFault::PauseAfterFlush { entered, release }) = &self.fault {
            entered.send(()).map_err(|_| PersistenceError::Canceled)?;
            release
                .recv_timeout(std::time::Duration::from_secs(3))
                .map_err(|_| PersistenceError::Canceled)?;
        }
        // The cancellation/effect issue point is serialized with host invalidation.
        // A host cannot claim its invalidation succeeded while this gate is held.
        {
            let current = self
                .fence
                .current
                .try_lock()
                .map_err(|_| PersistenceError::Busy)?;
            if !*current
                || !self.fence.issuer.alive.load(Ordering::Acquire)
                || context.stop_requested()
            {
                return Err(PersistenceError::Canceled);
            }
            write
                .publish_entry()
                .map_err(|_| PersistenceError::PublicationUnconfirmed("entry publish"))?;
        }
        #[cfg(test)]
        if let Some(TestFault::FailAfterPublish) = &self.fault {
            return Err(PersistenceError::PublicationUnconfirmed(
                "injected directory flush failure",
            ));
        }
        // Once publication crossed, cancellation cannot prove rollback. Flush it
        // and report real durability, then fence delivery against current selection.
        write
            .durable_ack()
            .map_err(|_| PersistenceError::PublicationUnconfirmed("directory flush"))?;
        Ok(PublishedLedger {
            fence: self.fence,
            digest: sha(&self.bytes),
            epoch,
        })
    }
}
#[cfg(test)]
enum TestFault {
    InvalidateBeforePublish,
    FailAfterPublish,
    PauseAfterFlush {
        entered: std::sync::mpsc::SyncSender<()>,
        release: std::sync::mpsc::Receiver<()>,
    },
}

enum Pending {
    Load(Receipt<LedgerLoad>),
    Write(WriteId),
}
pub(crate) struct PluginPermissionFiles {
    client: Client,
    root: Arc<PinnedDirectory>,
    issuer: Arc<OwnerIssuer>,
    current: Option<Arc<PermissionFence>>,
    writer: OrderedWriter<PermissionLedgerWrite>,
    pending: Option<Pending>,
    closed: bool,
    settlement_unconfirmed: bool,
    actor_wake: Arc<dyn Fn() + Send + Sync>,
    _storage: StorageAdmission,
}
impl Drop for PluginPermissionFiles {
    fn drop(&mut self) {
        // In-flight captures and their original bank admission remain alive until
        // actual exit. Dropping the owner never leaves a deliverable current grant.
        self.issuer.alive.store(false, Ordering::Release);
    }
}
impl PluginPermissionFiles {
    pub(crate) fn shares_root(&self, client: &Client) -> bool {
        self.client.quota_group().shares_root(&client.quota_group())
    }

    /// Root pin/creation is the composition root's admitted native task, not JS.
    /// Supply the actual actor's charged, nonblocking wake callback, not an
    /// empty substitute. Read and ordered-write terminal events notify both it
    /// and the UI. Its original ownership remains with fences and IO results.
    pub(crate) fn new(
        client: &Client,
        root: Arc<PinnedDirectory>,
        notification: Arc<Notify>,
        actor_wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let storage = client
            .quota_group()
            .reserve_external_storage(METADATA_BYTES)
            .map_err(PersistenceError::Admission)?;
        let wake = Arc::clone(&notification);
        let read_actor_wake = Arc::clone(&actor_wake);
        let client = client.clone().with_completion_wake(move || {
            wake.notify_one();
            read_actor_wake();
        });
        Ok(Self {
            writer: OrderedWriter::new_with_actor_wake(
                client.clone(),
                notification,
                Arc::clone(&actor_wake),
            ),
            client,
            root,
            issuer: Arc::new(OwnerIssuer {
                alive: AtomicBool::new(true),
            }),
            current: None,
            pending: None,
            closed: false,
            settlement_unconfirmed: false,
            actor_wake,
            _storage: storage,
        })
    }
    pub(crate) fn bind(
        &mut self,
        identity: &PackageIdentity,
        stamp: PersistenceStamp,
    ) -> Result<Arc<PermissionFence>> {
        if self.closed {
            return Err(PersistenceError::Admission(RejectReason::Closed));
        }
        if self.pending.is_some() {
            return Err(PersistenceError::Busy);
        }
        if stamp.selection_revision == 0
            || stamp.instance_id == 0
            || stamp.plan_revision == 0
            || stamp.authorization_epoch == 0
        {
            return Err(PersistenceError::Invalid("zero native stamp"));
        }
        let storage = self
            .client
            .quota_group()
            .reserve_external_storage(FENCE_BYTES)
            .map_err(PersistenceError::Admission)?;
        if let Some(current) = &self.current {
            let mut active = current
                .current
                .try_lock()
                .map_err(|_| PersistenceError::Busy)?;
            *active = false;
        }
        let principal_key = identity.principal_key();
        let fence = Arc::new(PermissionFence {
            issuer: Arc::clone(&self.issuer),
            namespace: hexadecimal(&sha(principal_key.as_bytes())),
            principal_key,
            identity: identity.clone(),
            stamp,
            current: Mutex::new(true),
            _actor_wake: Arc::clone(&self.actor_wake),
            _storage: storage,
        });
        self.current = Some(Arc::clone(&fence));
        Ok(fence)
    }
    fn matches(&self, fence: &Arc<PermissionFence>) -> Result<()> {
        if !Arc::ptr_eq(&self.issuer, &fence.issuer)
            || !self
                .current
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, fence))
        {
            return Err(PersistenceError::Stale);
        }
        fence.check()
    }
    /// Nonblocking invalidation precedes any host acknowledgement; Busy means no
    /// fence transition occurred. Broker revocation is a separate native step.
    pub(crate) fn invalidate(&mut self, fence: &Arc<PermissionFence>) -> Result<()> {
        self.matches(fence)?;
        let mut current = fence
            .current
            .try_lock()
            .map_err(|_| PersistenceError::Busy)?;
        *current = false;
        Ok(())
    }
    pub(crate) fn request_load(&mut self, fence: &Arc<PermissionFence>) -> Result<()> {
        if self.closed {
            return Err(PersistenceError::Admission(RejectReason::Closed));
        }
        self.matches(fence)?;
        if self.pending.is_some() {
            return Err(PersistenceError::Busy);
        }
        let reservation = self
            .client
            .try_reserve(Lane::Io, COST)
            .map_err(PersistenceError::Admission)?;
        let job = LedgerLoad {
            root: Arc::clone(&self.root),
            fence: Arc::clone(fence),
            quota: self.client.quota_group(),
        };
        let receipt = reservation
            .submit(job)
            .map_err(|rejected| PersistenceError::Admission(rejected.reason))?;
        self.pending = Some(Pending::Load(receipt));
        Ok(())
    }
    /// Immutable base came from this owner's real load, not a caller-supplied
    /// digest. Candidate bytes must be native broker export, never script data.
    pub(crate) fn request_commit(
        &mut self,
        base: &LedgerSnapshot,
        bytes: &[u8],
    ) -> Result<WriteId> {
        #[cfg(test)]
        {
            self.enqueue_commit(base, bytes, None)
        }
        #[cfg(not(test))]
        {
            self.enqueue_commit(base, bytes)
        }
    }
    fn enqueue_commit(
        &mut self,
        base: &LedgerSnapshot,
        bytes: &[u8],
        #[cfg(test)] fault: Option<TestFault>,
    ) -> Result<WriteId> {
        if self.closed {
            return Err(PersistenceError::Admission(RejectReason::Closed));
        }
        self.matches(&base.fence)?;
        if self.pending.is_some() {
            return Err(PersistenceError::Busy);
        }
        if bytes.is_empty() || bytes.len() > STATE_BYTES {
            return Err(PersistenceError::Invalid("candidate byte bound"));
        }
        let id = self
            .writer
            .enqueue_with(COST, || PermissionLedgerWrite {
                root: Arc::clone(&self.root),
                fence: Arc::clone(&base.fence),
                base: base.base,
                directory: base.directory.as_ref().map(Arc::clone),
                bytes: bytes.to_vec(),
                #[cfg(test)]
                fault,
            })
            .map_err(PersistenceError::Admission)?;
        self.pending = Some(Pending::Write(id));
        Ok(id)
    }
    /// A current native fence must also be checked by runtime before activation;
    /// merely receiving an outcome never publishes a grant.
    pub(crate) fn collect(&mut self) -> Option<PermissionCompletion> {
        let completion = self.collect_inner()?;
        let error = match &completion {
            PermissionCompletion::Loaded(outcome) => outcome.view().as_ref().err(),
            PermissionCompletion::Written(outcome) => outcome.view().as_ref().err(),
            PermissionCompletion::Failed(error) => Some(error),
        };
        if matches!(
            error,
            Some(PersistenceError::Lost | PersistenceError::PublicationUnconfirmed(_))
        ) {
            self.settlement_unconfirmed = true;
        }
        Some(completion)
    }
    /// Admission closed plus observed original receipts; uncertainty survives
    /// disappearance of pending flags or a mismatched/lost completion record.
    pub(crate) fn is_physically_settled(&self) -> bool {
        self.closed
            && self.pending.is_none()
            && self.writer.pending() == 0
            && !self.settlement_unconfirmed
    }
    fn collect_inner(&mut self) -> Option<PermissionCompletion> {
        let mut pending = self.pending.take()?;
        match &mut pending {
            Pending::Load(receipt) => match receipt.try_take() {
                JobPoll::Pending => {
                    self.pending = Some(pending);
                    None
                }
                JobPoll::Ready(outcome) => Some(PermissionCompletion::Loaded(outcome.map(
                    |outcome| match outcome {
                        JobOutcome::Finished(result) => result,
                        JobOutcome::NotStarted { .. } => Err(PersistenceError::Canceled),
                        JobOutcome::Panicked => Err(PersistenceError::Lost),
                    },
                ))),
                JobPoll::Lost | JobPoll::Taken => {
                    Some(PermissionCompletion::Failed(PersistenceError::Lost))
                }
            },
            Pending::Write(expected) => match self.writer.poll() {
                None => {
                    self.pending = Some(pending);
                    None
                }
                Some(WriteCompletion::Outcome { id, outcome }) if id == *expected => Some(
                    PermissionCompletion::Written(outcome.map(|outcome| match outcome {
                        JobOutcome::Finished(result) => {
                            result.map(|published| DurableLedgerReceipt {
                                write_id: id,
                                fence: published.fence,
                                digest: published.digest,
                                epoch: published.epoch,
                            })
                        }
                        JobOutcome::NotStarted { .. } => Err(PersistenceError::Canceled),
                        JobOutcome::Panicked => {
                            Err(PersistenceError::PublicationUnconfirmed("worker panic"))
                        }
                    })),
                ),
                Some(WriteCompletion::Rejected { id, rejection }) if id == *expected => Some(
                    PermissionCompletion::Failed(PersistenceError::Admission(rejection.reason)),
                ),
                Some(_) => Some(PermissionCompletion::Failed(
                    PersistenceError::PublicationUnconfirmed(
                        "lost or mismatched original write receipt",
                    ),
                )),
            },
        }
    }
    /// Ordered accepted writes drain; retain this owner and collect every result
    /// before shutting down the shared bank. Closing is not cancellation/retirement.
    pub(crate) fn close_admission(&mut self) {
        self.closed = true;
        self.writer.close_admission();
    }
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub(crate) fn notification(&self) -> Arc<Notify> {
        self.writer.notification()
    }
}
#[cfg(test)]
#[path = "plugin_permissions_tests.rs"]
mod tests;
