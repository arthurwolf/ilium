//! One persistent scene owner. Time requests replace pending time requests;
//! emitted-frame receipts are ordered and are never silently discarded.
use super::plugin_backend::{PluginBackend, PluginFrameIdentity};
use super::{AnimationCacheStatus, AnimationFrame, AnimationLoopCache, AnimationSettings};
use crate::animation_plugins::AnimationSourceTab;
use ilium_ambient::raster::PaintedOwner;
use ilium_ambient::scene::{FrameReceiptId, MAX_SCENE_RECEIPT_SLOTS};
use ilium_animation_js::helper::HelperAuthority;
use ilium_animation_js::replay::{PendingEmission, PlaybackLease};
use ilium_animation_js::runtime::{CommittedFrameEmission, RetainedFrameAuthority};
#[cfg(test)]
use ilium_execution::{Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt};
use ilium_execution::{QuotaGroup, StorageAdmission, WorkerAdmission};
use ilium_platform::owned_worker::{spawn_owned, OwnedWorker, StopToken, WorkerKind, WorkerTicket};
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
#[cfg(test)]
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

const MAX_CELLS: usize = 131_072;
const MAX_FRAMES: usize = MAX_SCENE_RECEIPT_SLOTS as usize;
const MAX_FRAME_BYTES: usize = 24 * 1024 * 1024;
const MAX_RECEIPTS: usize = 16;
const MAX_SERVICES: usize = 4;
const MAX_SETTINGS_BYTES: usize = 64 * 1024;
const MAX_SETTINGS_RETAINED_BYTES: usize = 256 * 1024;
static SERVICES: AtomicUsize = AtomicUsize::new(0);
static ADMISSION_READY: OnceLock<Arc<tokio::sync::Notify>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct RenderRequest {
    /// Changes for scene, dimensions or settings, never just for time.
    pub revision: u64,
    pub settings: AnimationSettings,
    pub width: u16,
    pub height: u16,
    pub elapsed: Duration,
    pub requested_at: Instant,
    pub pointer: Option<[f32; 2]>,
    /// Screen occupancy for mask-aware scenes and its change counter; the
    /// counter is part of the request identity so a changed screen re-renders.
    pub occupancy: Option<Arc<ilium_ambient::OccupancyMask>>,
    pub occupancy_revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    Busy,
    Full,
    Stale,
    Invalid,
    Stopped,
    RevisionNeedsBarrier,
}

#[derive(Debug)]
pub struct Rejected<T> {
    pub reason: AdmissionError,
    pub value: T,
}

#[derive(Debug)]
pub struct SnapshotCell {
    pub glyph: char,
    pub native_glyph: Option<char>,
    pub packed_bits: u8,
    pub color: Option<(u8, u8, u8)>,
    pub article_symbol: Option<String>,
    pub article_is_continuation: bool,
    pub article_style: (bool, bool),
    pub article_background: Option<(u8, u8, u8)>,
    pub article_underline: bool,
}

/// Allocation admission is retained until the final Arc (including receipts)
/// drops. Consumers cannot accumulate an unbounded history by cloning frames.
#[derive(Debug)]
struct FramePermit {
    count: Arc<AtomicUsize>,
    slots: Arc<AtomicU8>,
    slot: u8,
    // Payload dies before this guard; snapshots retain storage after engine exit.
    _storage: StorageAdmission,
}
impl Drop for FramePermit {
    fn drop(&mut self) {
        self.slots.fetch_and(!(1 << self.slot), Ordering::AcqRel);
        self.count.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A released UI snapshot clears only a cheap atomic bit. The worker drops
/// displaced scene-owned owner tables when it next seals this slot.
fn reserve_slot(slots: &AtomicU8) -> Option<u8> {
    let mut observed = slots.load(Ordering::Acquire);
    loop {
        let slot = (0..MAX_SCENE_RECEIPT_SLOTS).find(|slot| observed & (1 << slot) == 0)?;
        match slots.compare_exchange_weak(
            observed,
            observed | (1 << slot),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Some(slot),
            Err(actual) => observed = actual,
        }
    }
}

#[derive(Debug)]
pub struct FrameSnapshot {
    pub revision: u64,
    pub sequence: u64,
    pub width: u16,
    pub height: u16,
    pub elapsed: Duration,
    #[cfg(test)]
    pub(crate) geometry_render_count: usize,
    pub requested_at: Instant,
    pub completed_at: Instant,
    /// Owned snapshot allocations plus value/Arc bookkeeping; excludes engine,
    /// cache, allocator overhead and process RSS.
    pub resident_bytes: usize,
    pub status: Option<String>,
    pub frames_per_second: Option<u32>,
    pub cache: AnimationCacheStatus,
    pub is_wikipedia: bool,
    pub has_cell_colors: bool,
    cells: Vec<SnapshotCell>,
    plugin_identity: Option<PluginFrameIdentity>,
    plugin_authority: Option<RetainedFrameAuthority>,
    replay: Option<PlaybackLease>,
    scene_generation: Option<u64>,
    scene_receipt_id: Option<FrameReceiptId>,
    owner_ids: Vec<u32>,
    service: Weak<Shared>,
    _permit: FramePermit,
}

impl FrameSnapshot {
    /// Loaded archive identity, qualified by the same revision as these cells.
    /// Official-release trust is independent of archive integrity qualification.
    pub fn plugin_identity(&self) -> Option<&PluginFrameIdentity> {
        self.plugin_identity
            .as_ref()
            .filter(|identity| identity.revision == self.revision)
    }

    pub fn plugin_package_digest(&self) -> Option<&str> {
        self.plugin_identity()
            .map(|identity| identity.package_digest.as_str())
    }

    pub fn cell(&self, x: u16, y: u16) -> Option<&SnapshotCell> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.cells
            .get(usize::from(y) * usize::from(self.width) + usize::from(x))
    }

    /// Acquire before handing this frame to a presenter. Retained display
    /// snapshots alone do not prevent retirement; explicit in-flight leases do.
    pub fn begin_presentation(self: &Arc<Self>) -> Result<PresentationLease, AdmissionError> {
        let shared = self.service.upgrade().ok_or(AdmissionError::Stopped)?;
        let mailbox = shared
            .mailbox
            .try_lock()
            .map_err(|_| AdmissionError::Busy)?;
        if !shared.is_accepting() {
            return Err(AdmissionError::Stopped);
        }
        if mailbox.revision != Some(self.revision) {
            return Err(AdmissionError::Stale);
        }
        shared
            .presentations
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_FRAMES).then_some(count + 1)
            })
            .map_err(|_| AdmissionError::Full)?;
        drop(mailbox);
        Ok(PresentationLease {
            frame: Arc::clone(self),
            shared,
        })
    }
}

pub struct PresentationLease {
    frame: Arc<FrameSnapshot>,
    shared: Arc<Shared>,
}
impl std::fmt::Debug for PresentationLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PresentationLease")
            .field("sequence", &self.frame.sequence)
            .finish()
    }
}
impl Drop for PresentationLease {
    fn drop(&mut self) {
        self.shared.presentations.fetch_sub(1, Ordering::AcqRel);
        self.shared.ready.notify_one();
    }
}
impl PresentationLease {
    pub fn snapshot(&self) -> &FrameSnapshot {
        &self.frame
    }
    /// The exact queued frame retains the original native channel. The
    /// presenter holds the returned non-cloneable permit through backend flush.
    pub fn begin_output(&self) -> Result<Option<CommittedFrameEmission>, String> {
        match (&self.frame.plugin_identity, &self.frame.plugin_authority) {
            (None, None) => Ok(None),
            (Some(identity), Some(authority)) => {
                if identity.revision != self.frame.revision {
                    return Err("Plugin frame revision changed before output".into());
                }
                authority
                    .begin_output(&HelperAuthority {
                        package_digest: identity.package_digest.clone(),
                        instance_id: identity.instance_id,
                        plan_generation: identity.plan_generation,
                        authorization_epoch: identity.authorization_epoch,
                    })
                    .map(Some)
                    .map_err(|error| error.to_string())
            }
            _ => Err("Plugin frame native authority missing".into()),
        }
    }
    /// The playback lease belongs to this exact worker snapshot. A second
    /// presentation of the same replay receipt cannot silently duplicate
    /// source history; the worker must sample and publish a fresh frame.
    pub fn has_replay(&self) -> bool {
        self.frame.replay.is_some()
    }
    pub fn prepare_replay(&self, surviving: &[u8]) -> Result<Option<PendingEmission>, String> {
        let Some(replay) = &self.frame.replay else {
            return Ok(None);
        };
        replay
            .fork_for_presentation()
            .and_then(|lease| lease.prepare_emission(surviving))
            .map(Some)
            .map_err(|error| error.to_string())
    }
    /// Convert only after successful actual emission. Invalid masks return
    /// lease ownership too, so semantic acknowledgements cannot disappear.
    pub fn receipt(self, surviving: Vec<u8>) -> Result<EmissionReceipt, Rejected<(Self, Vec<u8>)>> {
        if surviving.capacity() > MAX_CELLS
            || surviving.len() != self.frame.cells.len()
            || surviving
                .iter()
                .zip(&self.frame.cells)
                .any(|(bits, cell)| bits & !cell.packed_bits != 0)
        {
            return Err(Rejected {
                reason: AdmissionError::Invalid,
                value: (self, surviving),
            });
        }
        Ok(EmissionReceipt {
            lease: self,
            surviving,
        })
    }
}

#[derive(Debug)]
pub struct EmissionReceipt {
    lease: PresentationLease,
    surviving: Vec<u8>,
}

impl EmissionReceipt {
    fn owners(&self) -> Option<Vec<PaintedOwner>> {
        const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
        let mut counts = BTreeMap::<u32, u32>::new();
        let width = usize::from(self.lease.frame.width);
        if width == 0 {
            return Some(Vec::new());
        }
        for (index, bits) in self.surviving.iter().enumerate() {
            for (dy, row) in BITS.iter().enumerate() {
                for (dx, bit) in row.iter().enumerate() {
                    if bits & bit == 0 {
                        continue;
                    }
                    let dot = (index / width * 4 + dy) * width * 2 + index % width * 2 + dx;
                    let Some(&owner) = self.lease.frame.owner_ids.get(dot) else {
                        continue;
                    };
                    if owner != 0 {
                        *counts.entry(owner).or_default() += 1;
                        if counts.len() > 8192 {
                            return None;
                        }
                    }
                }
            }
        }
        Some(
            counts
                .into_iter()
                .map(|(id, dots)| PaintedOwner { id, dots })
                .collect(),
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct ServiceStatus {
    pub completed: u64,
    pub coalesced: u64,
    pub stale: u64,
    pub retained_frames: usize,
    pub limited_frames: u64,
    pub is_accepting: bool,
    pub error: Option<String>,
    #[cfg(test)]
    pub native_task_timer_armed: bool,
    #[cfg(test)]
    pub finite_probe_acks: u64,
    #[cfg(test)]
    pub same_wake_task_and_finite: u64,
    #[cfg(test)]
    pub native_helper_physically_settled: bool,
}

enum SceneCommand {
    Configure(Box<RenderRequest>),
    Emitted(EmissionReceipt),
    Pause,
    #[cfg(test)]
    HoldRealFiniteWake {
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
        finished: mpsc::SyncSender<()>,
        wake_seen: mpsc::SyncSender<()>,
        revoke_original_activation: bool,
    },
}

#[cfg(test)]
struct RealFiniteWakeProbe {
    finished: mpsc::SyncSender<()>,
}
#[cfg(test)]
impl Job for RealFiniteWakeProbe {
    type Output = ();
    type Error = String;
    fn run(self, _: JobContext) -> Result<(), String> {
        self.finished.send(()).map_err(|error| error.to_string())
    }
}

#[derive(Default)]
struct Mailbox {
    revision: Option<u64>,
    settings: Option<AnimationSettings>,
    dimensions: (u16, u16),
    latest: Option<RenderRequest>,
    receipts: VecDeque<SceneCommand>,
    output: Option<Arc<FrameSnapshot>>,
    status: ServiceStatus,
}
struct Shared {
    quota: QuotaGroup,
    configurations: AtomicUsize,
    ready: Arc<tokio::sync::Notify>,
    presentations: AtomicUsize,
    stop: StopToken,
    accepting: AtomicBool,
    mailbox: Mutex<Mailbox>,
    changed: Condvar,
    permission_review: OnceLock<Arc<crate::animation_plugins::review_bridge::ReviewBridge>>,
    frames: Arc<AtomicUsize>,
    slots: Arc<AtomicU8>,
    #[cfg(test)]
    test_wake_observer: Mutex<Option<mpsc::SyncSender<()>>>,
}
impl Shared {
    fn is_accepting(&self) -> bool {
        self.accepting.load(Ordering::Acquire) && !self.stop.is_stopped()
    }
}
struct ConfigurationGuard<'a>(&'a Shared);
impl Drop for ConfigurationGuard<'_> {
    fn drop(&mut self) {
        self.0.configurations.fetch_sub(1, Ordering::AcqRel);
        self.0.ready.notify_one();
    }
}
struct AdmissionGuard<'a>(&'a Shared);
impl Drop for AdmissionGuard<'_> {
    fn drop(&mut self) {
        self.0.accepting.store(false, Ordering::Release);
    }
}
struct ServicePermit {
    // The platform wake owner retains this through actual OS join/TLS exit.
    _physical: WorkerAdmission,
}
impl Drop for ServicePermit {
    fn drop(&mut self) {
        SERVICES.fetch_sub(1, Ordering::AcqRel);
        let notification = admission_notification();
        notification.notify_waiters();
        notification.notify_one();
    }
}

pub(super) fn admission_notification() -> Arc<tokio::sync::Notify> {
    Arc::clone(ADMISSION_READY.get_or_init(|| Arc::new(tokio::sync::Notify::new())))
}

pub(super) struct AnimationAdmission {
    quota: QuotaGroup,
    _permit: ServicePermit,
}

pub struct AnimationService {
    shared: Arc<Shared>,
    worker: OwnedWorker,
}

impl AnimationService {
    pub fn start(resources: ilium_ambient::resources::AmbientResources) -> io::Result<Self> {
        Self::start_admitted(
            Self::reserve()?,
            None,
            Arc::new(tokio::sync::Notify::new()),
            resources,
        )
    }

    #[cfg(test)]
    fn with_frame(frame: AnimationFrame) -> io::Result<Self> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let admission = loop {
            match Self::reserve() {
                Ok(admission) => break admission,
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error),
            }
        };
        Self::start_admitted(
            admission,
            Some(frame),
            Arc::new(tokio::sync::Notify::new()),
            ilium_ambient::resources::AmbientResources::new(crate::execution::test_client()),
        )
    }

    pub(super) fn reserve() -> io::Result<AnimationAdmission> {
        Self::reserve_in(crate::execution::process_quota())
    }

    fn reserve_in(quota: QuotaGroup) -> io::Result<AnimationAdmission> {
        // Engine/library heaps keep their existing domain bounds; this adapter
        // charges the actual owner thread and independently owned frame storage.
        let physical = quota.reserve_external_worker(1, 0).map_err(|reason| {
            let kind = match reason {
                ilium_execution::RejectReason::Busy
                | ilium_execution::RejectReason::WorkerLimit
                | ilium_execution::RejectReason::WorkerBytes => io::ErrorKind::WouldBlock,
                _ => io::ErrorKind::Other,
            };
            io::Error::new(kind, format!("animation worker admission: {reason:?}"))
        })?;
        SERVICES
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_SERVICES).then_some(count + 1)
            })
            .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "animation services full"))?;
        Ok(AnimationAdmission {
            quota,
            _permit: ServicePermit {
                _physical: physical,
            },
        })
    }

    pub(super) fn start_admitted(
        admission: AnimationAdmission,
        initial_frame: Option<AnimationFrame>,
        ready: Arc<tokio::sync::Notify>,
        resources: ilium_ambient::resources::AmbientResources,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            quota: admission.quota.clone(),
            ready,
            configurations: AtomicUsize::new(0),
            presentations: AtomicUsize::new(0),
            stop: StopToken::default(),
            accepting: AtomicBool::new(true),
            mailbox: Mutex::new(Mailbox::default()),
            changed: Condvar::new(),
            permission_review: OnceLock::new(),
            frames: Arc::new(AtomicUsize::new(0)),
            slots: Arc::new(AtomicU8::new(0)),
            #[cfg(test)]
            test_wake_observer: Mutex::new(None),
        });
        let weak_permission_owner = Arc::downgrade(&shared);
        let permission_review = crate::animation_plugins::review_bridge::ReviewBridge::new(
            shared.quota.clone(),
            shared.ready.clone(),
            Box::new(move || {
                if let Some(shared) = weak_permission_owner.upgrade() {
                    shared.changed.notify_one();
                }
            }),
        )
        .map_err(io::Error::other)?;
        if !permission_review.shares_root(&shared.quota) {
            return Err(io::Error::other("Foreign native review quota root"));
        }
        shared
            .permission_review
            .set(permission_review)
            .map_err(|_| io::Error::other("Native permission bridge already initialized"))?;
        let wake = Arc::clone(&shared);
        let engine = Arc::clone(&shared);
        let worker = spawn_owned(
            "ilium-animation",
            WorkerKind::Cooperative,
            shared.stop.clone(),
            move || {
                // The platform supervisor retains wake state through actual
                // join/TLS exit; a retiring engine keeps its admission debit.
                let _retained_admission = &admission;
                wake.changed.notify_all();
            },
            move |stop| {
                ilium_platform::thread_priority::lower_current_thread(
                    ilium_platform::thread_priority::WorkerPriority::Lowest,
                );
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // Persistent engine construction belongs to this thread,
                    // including any future host-owned source/runtime setup.
                    let mut frame = initial_frame.unwrap_or_default();
                    frame.configure_resources(resources.clone());
                    run(&engine, frame, stop, resources)
                }));
                engine.accepting.store(false, Ordering::Release);
                engine.ready.notify_one();
                if result.is_err() {
                    engine
                        .mailbox
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .status
                        .error = Some("Animation worker panicked".into());
                }
            },
        )?;
        Ok(Self { shared, worker })
    }

    pub fn configuration_count(&self) -> usize {
        self.shared.configurations.load(Ordering::Acquire)
    }
    pub fn presentation_count(&self) -> usize {
        self.shared.presentations.load(Ordering::Acquire)
    }
    pub fn is_accepting(&self) -> bool {
        self.shared.is_accepting()
    }
    pub(crate) fn permission_bridge(
        &self,
    ) -> Option<Arc<crate::animation_plugins::review_bridge::ReviewBridge>> {
        if !self.shared.is_accepting() {
            return None;
        }
        self.shared.permission_review.get().cloned()
    }

    pub fn ticket(&self) -> WorkerTicket {
        self.worker.ticket()
    }

    #[cfg(test)]
    fn test_hold_real_finite_wake(
        &self,
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
        finished: mpsc::SyncSender<()>,
        wake_seen: mpsc::SyncSender<()>,
        revoke_original_activation: bool,
    ) -> Result<(), AdmissionError> {
        let Ok(mut mailbox) = self.shared.mailbox.try_lock() else {
            return Err(AdmissionError::Busy);
        };
        if !self.shared.is_accepting() {
            return Err(AdmissionError::Stopped);
        }
        if mailbox.receipts.len() >= MAX_RECEIPTS {
            return Err(AdmissionError::Full);
        }
        mailbox
            .receipts
            .push_back(SceneCommand::HoldRealFiniteWake {
                entered,
                release,
                finished,
                wake_seen,
                revoke_original_activation,
            });
        self.shared.changed.notify_one();
        Ok(())
    }

    /// Nonblocking. Rejection returns ownership so an input caller can retry.
    pub fn try_request(&self, request: RenderRequest) -> Result<(), Box<Rejected<RenderRequest>>> {
        if !settings_fit(&request.settings) {
            return Err(Box::new(Rejected {
                reason: AdmissionError::Invalid,
                value: request,
            }));
        };
        let reason = if !self.shared.is_accepting() {
            Some(AdmissionError::Stopped)
        } else if !dimensions_fit(request.width, request.height) {
            Some(AdmissionError::Invalid)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(Box::new(Rejected {
                reason,
                value: request,
            }));
        }
        let Ok(mut mailbox) = self.shared.mailbox.try_lock() else {
            return Err(Box::new(Rejected {
                reason: AdmissionError::Busy,
                value: request,
            }));
        };
        if !self.shared.is_accepting() {
            return Err(Box::new(Rejected {
                reason: AdmissionError::Stopped,
                value: request,
            }));
        }
        if mailbox
            .revision
            .is_some_and(|revision| request.revision < revision)
        {
            return Err(Box::new(Rejected {
                reason: AdmissionError::Stale,
                value: request,
            }));
        }
        if mailbox.revision == Some(request.revision)
            && (mailbox.settings.as_ref() != Some(&request.settings)
                || mailbox.dimensions != (request.width, request.height))
        {
            return Err(Box::new(Rejected {
                reason: AdmissionError::Invalid,
                value: request,
            }));
        }
        if mailbox
            .revision
            .is_some_and(|revision| revision != request.revision)
            && self.shared.presentations.load(Ordering::Acquire) != 0
        {
            return Err(Box::new(Rejected {
                reason: AdmissionError::RevisionNeedsBarrier,
                value: request,
            }));
        }
        let changed = mailbox.revision != Some(request.revision);
        if changed
            && (mailbox.receipts.len() >= MAX_RECEIPTS
                || self.shared.configurations.load(Ordering::Acquire) >= MAX_RECEIPTS)
        {
            return Err(Box::new(Rejected {
                reason: AdmissionError::Full,
                value: request,
            }));
        }
        if changed {
            if self.shared.permission_review.get().is_none_or(|bridge| {
                bridge
                    .select(
                        request.revision,
                        request.settings.source
                            == crate::animation_plugins::AnimationSourceTab::Plugin,
                    )
                    .is_err()
            }) {
                return Err(Box::new(Rejected {
                    reason: AdmissionError::Invalid,
                    value: request,
                }));
            }
            mailbox.output = None;
            mailbox.settings = Some(request.settings.clone());
            mailbox.dimensions = (request.width, request.height);
            mailbox.revision = Some(request.revision);
            mailbox.latest = None;
            self.shared.configurations.fetch_add(1, Ordering::AcqRel);
            mailbox
                .receipts
                .push_back(SceneCommand::Configure(Box::new(request)));
        } else {
            if mailbox.latest.is_some() {
                mailbox.status.coalesced += 1;
            }
            mailbox.latest = Some(request);
        }
        self.shared.changed.notify_one();
        Ok(())
    }

    pub fn try_receipt(&self, receipt: EmissionReceipt) -> Result<(), Rejected<EmissionReceipt>> {
        if !self.shared.is_accepting() {
            return Err(Rejected {
                reason: AdmissionError::Stopped,
                value: receipt,
            });
        }
        let Ok(mut mailbox) = self.shared.mailbox.try_lock() else {
            return Err(Rejected {
                reason: AdmissionError::Busy,
                value: receipt,
            });
        };
        if !self.shared.is_accepting() {
            return Err(Rejected {
                reason: AdmissionError::Stopped,
                value: receipt,
            });
        }
        if mailbox.receipts.len() >= MAX_RECEIPTS {
            return Err(Rejected {
                reason: AdmissionError::Full,
                value: receipt,
            });
        }
        mailbox.receipts.push_back(SceneCommand::Emitted(receipt));
        self.shared.changed.notify_one();
        Ok(())
    }

    /// Ordered visibility transition. Bumps the same revision fence as a scene
    /// switch; pending generation cannot publish after a successful pause.
    pub fn try_pause(&self, revision: u64) -> Result<(), AdmissionError> {
        if !self.shared.is_accepting() {
            return Err(AdmissionError::Stopped);
        }
        let Ok(mut mailbox) = self.shared.mailbox.try_lock() else {
            return Err(AdmissionError::Busy);
        };
        if !self.shared.is_accepting() {
            return Err(AdmissionError::Stopped);
        }
        if mailbox.revision.is_some_and(|current| revision <= current) {
            return Err(AdmissionError::Stale);
        }
        if self.shared.presentations.load(Ordering::Acquire) != 0 {
            return Err(AdmissionError::RevisionNeedsBarrier);
        }
        if mailbox.receipts.len() >= MAX_RECEIPTS
            || self.shared.configurations.load(Ordering::Acquire) >= MAX_RECEIPTS
        {
            return Err(AdmissionError::Full);
        }
        if self
            .shared
            .permission_review
            .get()
            .is_none_or(|bridge| bridge.select(revision, false).is_err())
        {
            return Err(AdmissionError::Invalid);
        }
        mailbox.revision = Some(revision);
        mailbox.settings = None;
        mailbox.latest = None;
        mailbox.output = None;
        self.shared.configurations.fetch_add(1, Ordering::AcqRel);
        mailbox.receipts.push_back(SceneCommand::Pause);
        self.shared.changed.notify_one();
        Ok(())
    }

    pub fn try_snapshot(&self) -> Option<Arc<FrameSnapshot>> {
        self.shared.mailbox.try_lock().ok()?.output.take()
    }

    pub fn try_status(&self) -> Option<ServiceStatus> {
        let mut status = self.shared.mailbox.try_lock().ok()?.status.clone();
        status.retained_frames = self.shared.frames.load(Ordering::Acquire);
        status.is_accepting = self.shared.is_accepting();
        Some(status)
    }
}

impl Drop for AnimationService {
    fn drop(&mut self) {
        self.shared.accepting.store(false, Ordering::Release);
        // OwnedWorker's drop requests cancellation and wakes the condition.
        // It transfers no joins or scene destruction onto this caller.
    }
}

// Bounded serialization counts settings bytes without allocating a serialized
// copy. This limits retained request strings before mailbox admission.
pub(super) fn dimensions_fit(width: u16, height: u16) -> bool {
    usize::from(width) * usize::from(height) <= MAX_CELLS
}

pub(super) fn settings_fit(settings: &AnimationSettings) -> bool {
    // All heap-owning settings fields reachable from AnimationSettings. Update
    // this inventory when introducing another dynamic settings field.
    let ambient = &settings.ambient;
    let voxel = &ambient.voxel_landscape;
    let strings = [
        &ambient.graph.source_id,
        &ambient.location.label,
        &ambient.video.source,
        &ambient.stars.start_datetime,
        &ambient.spectrum.device_name,
        &ambient.images.folders,
        &ambient.images.urls,
        &ambient.openstreetmap.local_path,
        &ambient.openstreetmap.endpoint,
        &ambient.openstreetmap.coordinates,
        &voxel.saved_maps.saves_folder,
        &voxel.pack_path,
        &voxel.pack_root,
        &voxel.pack_addon_path,
    ];
    // std's B-tree nodes contain at most eleven key/value entries. Charging a
    // whole 4KiB node per entry (plus an empty root) conservatively covers the
    // current String/PackSourceSettings node layout and child links.
    let Some(mut retained) = voxel
        .pack_custom_sources
        .len()
        .checked_add(1)
        .and_then(|entries| entries.checked_mul(4096))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<AnimationSettings>()))
    else {
        return false;
    };
    for string in strings {
        let Some(next) = retained.checked_add(string.capacity()) else {
            return false;
        };
        retained = next;
        if retained > MAX_SETTINGS_RETAINED_BYTES {
            return false;
        }
    }
    for (key, source) in &voxel.pack_custom_sources {
        for string in [key, &source.path, &source.root, &source.addon_path] {
            let Some(next) = retained.checked_add(string.capacity()) else {
                return false;
            };
            retained = next;
            if retained > MAX_SETTINGS_RETAINED_BYTES {
                return false;
            }
        }
    }
    if retained > MAX_SETTINGS_RETAINED_BYTES {
        return false;
    }
    struct Counter {
        bytes: usize,
    }
    impl io::Write for Counter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.bytes = self.bytes.saturating_add(buffer.len());
            if self.bytes > MAX_SETTINGS_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "animation settings too large",
                ));
            }
            Ok(buffer.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { bytes: 0 };
    serde_json::to_writer(&mut counter, settings).is_ok()
}

// Quota Busy means a short shared-ledger collision, not capacity exhaustion.
// Retry only on the scene's existing native thread; cancellation bounds its
// lifetime and the sleep avoids a hot spin. Genuine admission failures remain
// explicit startup errors. The operation closure permits deterministic forcing
// of contention without exposing the quota ledger's private lock.
fn reserve_wake_storage(
    stop: &StopToken,
    mut reserve: impl FnMut() -> Result<StorageAdmission, ilium_execution::RejectReason>,
) -> Result<Option<StorageAdmission>, ilium_execution::RejectReason> {
    loop {
        if stop.is_stopped() {
            return Ok(None);
        }
        match reserve() {
            Ok(storage) => return Ok(Some(storage)),
            Err(ilium_execution::RejectReason::Busy) => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
}

fn run(
    shared: &Arc<Shared>,
    mut frame: AnimationFrame,
    stop: StopToken,
    resources: ilium_ambient::resources::AmbientResources,
) {
    // Close command admission on every exit, including wake setup refusal.
    let startup_admission = AdmissionGuard(shared);
    // One bounded wake slot shares the original root. The callback retains
    // its original charge if the finite job outlives this scene worker.
    let native_wake_storage =
        match reserve_wake_storage(&stop, || shared.quota.reserve_external_storage(4096)) {
            Ok(Some(storage)) => Arc::new(storage),
            Ok(None) => return,
            Err(error) => {
                shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .status
                    .error = Some(format!("Native plugin wake admission: {error:?}"));
                shared.ready.notify_one();
                return;
            }
        };
    let (native_wake_sender, native_wake_receiver) = std::sync::mpsc::sync_channel(1);
    let weak_shared = Arc::downgrade(shared);
    let wake_charge = Arc::clone(&native_wake_storage);
    let actor_wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _retained_charge = &wake_charge;
        // Full preserves the existing obligation to inspect native receipts.
        let _ = native_wake_sender.try_send(());
        if let Some(shared) = weak_shared.upgrade() {
            #[cfg(test)]
            if let Some(observer) = shared
                .test_wake_observer
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_ref()
            {
                let _ = observer.try_send(());
            }
            shared.changed.notify_one();
        }
    });
    let completion_wake = Arc::clone(&actor_wake);
    let plugin_resources = ilium_ambient::resources::AmbientResources::new(
        resources
            .finite()
            .clone()
            .with_completion_wake(move || completion_wake()),
    );
    let Some(review_bridge) = shared.permission_review.get().cloned() else {
        shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .status
            .error = Some("Native permission review owner missing".into());
        shared.ready.notify_one();
        return;
    };
    let mut plugin = PluginBackend::new(
        shared.quota.clone(),
        plugin_resources.clone(),
        Arc::clone(&review_bridge),
        actor_wake,
        Arc::clone(&shared.ready),
    );
    let mut cache = AnimationLoopCache::new(resources);
    #[cfg(test)]
    let mut finite_probe_receipt: Option<Receipt<RealFiniteWakeProbe>> = None;
    // Move the SAME guard after native/cache construction so admission closes
    // before their destructor work, including on panic or orderly exit.
    let _admission = startup_admission;
    let mut sequence = 0;
    let mut shutdown_started = false;
    loop {
        let task_deadline = plugin.next_task_deadline();
        #[cfg(test)]
        {
            let mut mailbox = shared
                .mailbox
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            mailbox.status.native_task_timer_armed = task_deadline.is_some();
            mailbox.status.native_helper_physically_settled = plugin.is_physically_settled();
        }
        let (request, receipts, stopping, native_wake, task_due, review_intent) = {
            let mut mailbox = shared.mailbox.lock().unwrap_or_else(|e| e.into_inner());
            let mut native_wake = false;
            let mut task_due = false;
            loop {
                native_wake |= native_wake_receiver.try_recv().is_ok();
                task_due |= task_deadline.is_some_and(|deadline| Instant::now() >= deadline);
                if (stop.is_stopped() && !shutdown_started)
                    || (!shutdown_started && mailbox.latest.is_some())
                    || !mailbox.receipts.is_empty()
                    || native_wake
                    || task_due
                    || (!shutdown_started && review_bridge.has_intent())
                {
                    break;
                }
                // Preserve the existing stop wait. A condvar notification can
                // race sleep; the charged one-slot channel retains the wake.
                // The fixed stop observation and optional task deadline never
                // poll finite receipts; only their original wake does that.
                let wait = task_deadline.map_or(Duration::from_millis(50), |deadline| {
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(50))
                });
                mailbox = shared
                    .changed
                    .wait_timeout(mailbox, wait)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            let stopping = stop.is_stopped();
            if stopping {
                shared.accepting.store(false, Ordering::Release);
            }
            let request = mailbox.latest.take().filter(|_| !stopping);
            (
                request,
                std::mem::take(&mut mailbox.receipts),
                stopping,
                native_wake,
                task_due,
                !shutdown_started && review_bridge.has_intent(),
            )
        };
        for command in receipts {
            match command {
                SceneCommand::Configure(request) => {
                    let _configuration = ConfigurationGuard(shared);
                    render_request(
                        shared,
                        &mut frame,
                        &mut cache,
                        &mut plugin,
                        &mut sequence,
                        &stop,
                        *request,
                    );
                }
                SceneCommand::Emitted(receipt) => {
                    if let (Some(generation), Some(id)) = (
                        receipt.lease.frame.scene_generation,
                        receipt.lease.frame.scene_receipt_id,
                    ) {
                        if let Some(owners) = receipt.owners() {
                            frame.host_mut().presented_frame(generation, id, &owners);
                        } else {
                            shared
                                .mailbox
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .status
                                .error = Some("Animation receipt exceeds owner limit".into());
                        }
                    }
                }
                SceneCommand::Pause => {
                    let _configuration = ConfigurationGuard(shared);
                    frame.release_hosts();
                    cache.pause();
                    plugin.stop();
                }
                #[cfg(test)]
                SceneCommand::HoldRealFiniteWake {
                    entered,
                    release,
                    finished,
                    wake_seen,
                    revoke_original_activation,
                } => {
                    // The original finite client and its completion-wake
                    // callback remain intact. Only the scene actor is held so
                    // the genuine receipt and monotonic task due coalesce.
                    *shared
                        .test_wake_observer
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Some(wake_seen);
                    let reservation = plugin_resources
                        .finite()
                        .try_reserve(
                            Lane::Io,
                            JobCost {
                                input_bytes: 1,
                                result_bytes: 1,
                            },
                        )
                        .expect("real finite probe admission");
                    finite_probe_receipt = Some(
                        reservation
                            .submit(RealFiniteWakeProbe { finished })
                            .expect("real finite probe submission"),
                    );
                    if revoke_original_activation {
                        plugin
                            .test_revoke_current_activation()
                            .expect("revoke actual accepted activation");
                    }
                    entered.send(()).expect("scene hold observation");
                    release
                        .recv_timeout(Duration::from_secs(10))
                        .expect("release held scene actor");
                }
            }
        }
        // The original mailbox guard has exited BEFORE broker/controller work.
        if review_intent && !stopping && review_bridge.has_intent() {
            if let Err(error) = plugin.on_review_intent() {
                plugin.fail_current(&error);
                shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .status
                    .error = Some(error);
            }
        }
        if stopping {
            plugin.stop();
        }
        if native_wake {
            if let Err(error) = plugin.on_native_completion() {
                plugin.fail_current(&error);
                shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .status
                    .error = Some(error);
            }
            #[cfg(test)]
            if let Some(receipt) = finite_probe_receipt.as_mut() {
                match receipt.try_take() {
                    JobPoll::Ready(outcome) => {
                        assert!(matches!(outcome.view(), JobOutcome::Finished(Ok(()))));
                        finite_probe_receipt = None;
                        *shared
                            .test_wake_observer
                            .lock()
                            .unwrap_or_else(|error| error.into_inner()) = None;
                        let mut mailbox = shared
                            .mailbox
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        mailbox.status.finite_probe_acks += 1;
                        if task_due {
                            mailbox.status.same_wake_task_and_finite += 1;
                        }
                    }
                    JobPoll::Pending => {}
                    JobPoll::Lost | JobPoll::Taken => {
                        panic!("real finite wake lost its original receipt")
                    }
                }
            }
        }
        if task_due && !stopping {
            if let Err(error) = plugin.on_task_deadline() {
                plugin.fail_current(&error);
                shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .status
                    .error = Some(error);
            }
            // A task-time error can arrive after the original finite wake
            // was already consumed on this actor turn. Inspect logical
            // retirement now, without collecting another finite receipt;
            // any still-running original owner retains its later real wake.
            if let Err(error) = plugin.settle_retirement(false) {
                shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .status
                    .error = Some(error);
            }
        }
        shared.ready.notify_one();
        // Successfully admitted semantic receipts drain even on cancellation.
        // Scene teardown can block only this worker, never its logical owner.
        if stopping {
            shutdown_started = true;
            plugin.stop();
            if let Err(error) = plugin.settle_retirement(false) {
                shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .status
                    .error = Some(error);
            }
            if plugin.is_physically_settled() {
                return;
            }
            // Original finite callbacks wake this SAME worker. Never release
            // native controller/helper/service custody on a canceled waiter.
            continue;
        }
        let Some(request) = request else {
            continue;
        };
        render_request(
            shared,
            &mut frame,
            &mut cache,
            &mut plugin,
            &mut sequence,
            &stop,
            request,
        );
        shared.ready.notify_one();
    }
}

fn render_request(
    shared: &Arc<Shared>,
    frame: &mut AnimationFrame,
    cache: &mut AnimationLoopCache,
    plugin: &mut PluginBackend,
    sequence: &mut u64,
    stop: &StopToken,
    request: RenderRequest,
) {
    if request.settings.source == AnimationSourceTab::Plugin {
        cache.pause();
        frame.release_hosts();
        render_plugin_request(shared, plugin, sequence, stop, request);
        return;
    }
    plugin.stop();
    frame.pointer(request.pointer);
    frame.set_occupancy(request.occupancy.clone(), request.occupancy_revision);
    let ready = if request.settings.uses_loop_cache() {
        cache.step(&request.settings, request.width, request.height, 8)
            && cache.copy_frame_into(request.elapsed, frame)
    } else {
        cache.pause();
        false
    };
    if !ready {
        frame.render(
            &request.settings,
            request.width,
            request.height,
            request.elapsed,
        );
    }
    if stop.is_stopped() {
        return;
    }
    // Same revision + a newer clock request must NOT invalidate this result.
    // Only a settings/scene/dimension revision fences presentation.
    let mut mailbox = shared.mailbox.lock().unwrap_or_else(|e| e.into_inner());
    if mailbox.revision != Some(request.revision) {
        mailbox.status.stale += 1;
        return;
    }
    // Release an unconsumed replaceable output before reserving its successor.
    mailbox.output = None;
    drop(mailbox);
    let Ok(_) = shared
        .frames
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_FRAMES).then_some(count + 1)
        })
    else {
        shared
            .mailbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .limited_frames += 1;
        return;
    };
    let Some(slot) = reserve_slot(&shared.slots) else {
        shared.frames.fetch_sub(1, Ordering::AcqRel);
        shared
            .mailbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .limited_frames += 1;
        return;
    };
    // Reserve the bounded packing allocation before constructing any cells.
    // A refused replaceable frame keeps the UI's last presented snapshot.
    let storage = match shared.quota.reserve_external_storage(MAX_FRAME_BYTES) {
        Ok(storage) => storage,
        Err(reason) => {
            shared.slots.fetch_and(!(1 << slot), Ordering::AcqRel);
            shared.frames.fetch_sub(1, Ordering::AcqRel);
            let mut mailbox = shared.mailbox.lock().unwrap_or_else(|e| e.into_inner());
            mailbox.status.limited_frames += 1;
            // Busy is momentary lock contention. Every other refusal, WorkerBytes
            // included, is sticky: say so, or the field silently never appears.
            if !matches!(reason, ilium_execution::RejectReason::Busy) {
                let quota = shared.quota.snapshot();
                tracing::warn!(?reason, ?quota, "animation frame admission refused");
                mailbox.status.error = Some(format!(
                    "Animation frame admission failed: {reason:?} (worker bytes {} of {} MiB in use)",
                    quota.worker_bytes >> 20,
                    quota.limits.worker_bytes >> 20,
                ));
            }
            drop(mailbox);
            shared.ready.notify_one();
            return;
        }
    };
    let permit = FramePermit {
        _storage: storage,
        count: Arc::clone(&shared.frames),
        slots: Arc::clone(&shared.slots),
        slot,
    };
    *sequence += 1;
    let snapshot = frame.snapshot(
        &request,
        *sequence,
        if request.settings.uses_loop_cache() {
            cache.status()
        } else {
            Default::default()
        },
        permit,
        Arc::downgrade(shared),
    );
    // Snapshot admission, not render, is when a receipt occupies one of the
    // three retained worker slots. The UI never owns its heavy FrameOwners.
    let sealed = snapshot.as_ref().is_none_or(|snapshot| {
        match (snapshot.scene_generation, snapshot.scene_receipt_id) {
            (Some(generation), Some(id)) => frame.host_mut().seal_frame(generation, id),
            (None, None) => true,
            _ => false,
        }
    });
    let mut mailbox = shared.mailbox.lock().unwrap_or_else(|e| e.into_inner());
    match snapshot {
        Some(snapshot) if sealed && mailbox.revision == Some(request.revision) => {
            mailbox.status.completed += 1;
            mailbox.output = Some(Arc::new(snapshot));
        }
        Some(_) if !sealed => {
            mailbox.status.error = Some("Animation frame receipt sealing failed".into())
        }
        Some(_) => mailbox.status.stale += 1,
        None => mailbox.status.error = Some("Animation snapshot exceeds byte limit".into()),
    }
    drop(mailbox);
    shared.ready.notify_one();
}

/// Reserve immutable packing before asking the canonical plugin owner to commit.
/// Obsolete requests may finish, but only the current revision can be published.
fn render_plugin_request(
    shared: &Arc<Shared>,
    plugin: &mut PluginBackend,
    sequence: &mut u64,
    stop: &StopToken,
    request: RenderRequest,
) {
    if stop.is_stopped() {
        return;
    }
    {
        let mut mailbox = shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if mailbox.revision != Some(request.revision) {
            mailbox.status.stale += 1;
            return;
        }
        // Release an unconsumed replaceable frame before successor admission.
        mailbox.output = None;
    }
    if shared
        .frames
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_FRAMES).then_some(count + 1)
        })
        .is_err()
    {
        shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .status
            .limited_frames += 1;
        return;
    }
    let Some(slot) = reserve_slot(&shared.slots) else {
        shared.frames.fetch_sub(1, Ordering::AcqRel);
        shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .status
            .limited_frames += 1;
        return;
    };
    let storage = match shared.quota.reserve_external_storage(MAX_FRAME_BYTES) {
        Ok(storage) => storage,
        Err(reason) => {
            shared.slots.fetch_and(!(1 << slot), Ordering::AcqRel);
            shared.frames.fetch_sub(1, Ordering::AcqRel);
            let mut mailbox = shared
                .mailbox
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            mailbox.status.limited_frames += 1;
            // Busy is momentary lock contention. Every other refusal, WorkerBytes
            // included, is sticky: say so, or the field silently never appears.
            if !matches!(reason, ilium_execution::RejectReason::Busy) {
                let quota = shared.quota.snapshot();
                tracing::warn!(?reason, ?quota, "plugin snapshot admission refused");
                mailbox.status.error = Some(format!(
                    "Plugin snapshot admission failed: {reason:?} (worker bytes {} of {} MiB in use)",
                    quota.worker_bytes >> 20,
                    quota.limits.worker_bytes >> 20,
                ));
            }
            return;
        }
    };
    let permit = FramePermit {
        _storage: storage,
        count: Arc::clone(&shared.frames),
        slots: Arc::clone(&shared.slots),
        slot,
    };
    let Some(next_sequence) = sequence.checked_add(1) else {
        shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .status
            .error = Some("Animation sequence exhausted".into());
        return;
    };
    *sequence = next_sequence;
    let result = plugin.render(&request, next_sequence, stop);
    if let Err(error) = &result {
        plugin.fail_current(error);
    }
    if stop.is_stopped() {
        plugin.stop();
        return;
    }
    let mut mailbox = shared
        .mailbox
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if mailbox.revision != Some(request.revision) {
        mailbox.status.stale += 1;
        return;
    }
    match result {
        Ok(Some(frame)) => {
            let bytes = frame.resident_bytes.checked_add(
                std::mem::size_of::<FrameSnapshot>() + 2 * std::mem::size_of::<usize>(),
            );
            if frame.identity.revision != request.revision
                || bytes.is_none_or(|bytes| bytes > MAX_FRAME_BYTES)
                || frame.cells.len() != usize::from(request.width) * usize::from(request.height)
            {
                mailbox.status.error = Some("Plugin snapshot identity or size is invalid".into());
                return;
            }
            let snapshot = FrameSnapshot {
                revision: request.revision,
                sequence: next_sequence,
                width: request.width,
                height: request.height,
                elapsed: request.elapsed,
                #[cfg(test)]
                geometry_render_count: 0,
                requested_at: request.requested_at,
                completed_at: Instant::now(),
                resident_bytes: bytes.unwrap_or(MAX_FRAME_BYTES),
                status: None,
                frames_per_second: Some(frame.frames_per_second),
                cache: Default::default(),
                is_wikipedia: false,
                has_cell_colors: true,
                cells: frame.cells,
                plugin_identity: Some(frame.identity),
                plugin_authority: Some(frame.authority),
                replay: frame.replay,
                scene_generation: None,
                scene_receipt_id: None,
                owner_ids: Vec::new(),
                service: Arc::downgrade(shared),
                _permit: permit,
            };
            mailbox.status.completed += 1;
            mailbox.status.error = None;
            mailbox.output = Some(Arc::new(snapshot));
        }
        Ok(None) => {}
        Err(error) => mailbox.status.error = Some(error.chars().take(240).collect()),
    }
}

impl AnimationFrame {
    fn snapshot(
        &self,
        request: &RenderRequest,
        sequence: u64,
        cache: AnimationCacheStatus,
        permit: FramePermit,
        service: Weak<Shared>,
    ) -> Option<FrameSnapshot> {
        let count = usize::from(self.width) * usize::from(self.height);
        let mut cells = Vec::with_capacity(count);
        let mut bytes = cells
            .capacity()
            .checked_mul(std::mem::size_of::<SnapshotCell>())?
            .checked_add(
                self.raster
                    .owner_ids
                    .len()
                    .checked_mul(std::mem::size_of::<u32>())?,
            )?
            .checked_add(std::mem::size_of::<FrameSnapshot>() + 2 * std::mem::size_of::<usize>())?;
        bytes = bytes.checked_add(self.host.receipt_bytes())?;
        for y in 0..self.height {
            for x in 0..self.width {
                let symbol = self.article_symbol(x, y);
                if bytes.checked_add(symbol.map_or(0, str::len))? > MAX_FRAME_BYTES {
                    return None;
                }
                let article_symbol = symbol.map(str::to_owned);
                bytes = bytes.checked_add(article_symbol.as_ref().map_or(0, String::capacity))?;
                if bytes > MAX_FRAME_BYTES {
                    return None;
                }
                let native_glyph = self.native_glyph(x, y);
                let scene_color = self.cell_color(x, y);
                let color = if request.settings.appearance.is_neutral() {
                    scene_color
                } else {
                    let coverage = if self.is_wikipedia || native_glyph.is_some() {
                        1.0
                    } else {
                        f32::from(
                            self.cells[usize::from(y) * usize::from(self.width) + usize::from(x)]
                                .count_ones() as u8,
                        ) / 8.0
                    };
                    let fraction = |coordinate: u16, extent: u16| {
                        if extent <= 1 {
                            0.5
                        } else {
                            f32::from(coordinate) / f32::from(extent - 1)
                        }
                    };
                    let (red, green, blue) = request.settings.foreground_rgb();
                    let [red, green, blue] = request.settings.appearance.shade(
                        [red, green, blue],
                        scene_color.map(|(r, g, b)| [r, g, b]),
                        &ilium_ambient::style::CellContext {
                            coverage,
                            x: fraction(x, self.width),
                            y: fraction(y, self.height),
                            seconds: request.elapsed.as_secs_f32(),
                        },
                    );
                    Some((red, green, blue))
                };
                cells.push(SnapshotCell {
                    glyph: self.glyph(x, y),
                    native_glyph,
                    packed_bits: self.cells
                        [usize::from(y) * usize::from(self.width) + usize::from(x)],
                    color,
                    article_symbol,
                    article_is_continuation: self.article_is_continuation(x, y),
                    article_style: self.article_style(x, y),
                    article_background: None,
                    article_underline: false,
                });
            }
        }
        let status = self.status();
        bytes = bytes.checked_add(status.as_ref().map_or(0, String::capacity))?;
        if bytes > MAX_FRAME_BYTES {
            return None;
        }
        let scene_receipt_id = if self.last_ambient.is_some() {
            Some(FrameReceiptId::new(permit.slot, sequence)?)
        } else {
            None
        };
        Some(FrameSnapshot {
            revision: request.revision,
            sequence,
            width: self.width,
            height: self.height,
            elapsed: request.elapsed,
            requested_at: request.requested_at,
            #[cfg(test)]
            geometry_render_count: self.geometry_render_count,
            completed_at: Instant::now(),
            resident_bytes: bytes,
            status,
            frames_per_second: self.host.frames_per_second(),
            cache,
            is_wikipedia: self.is_wikipedia,
            has_cell_colors: self.has_cell_colors,
            cells,
            plugin_identity: None,
            plugin_authority: None,
            replay: None,
            scene_generation: self.last_ambient.map(|key| key.generation),
            scene_receipt_id,
            owner_ids: self.raster.owner_ids.clone(),
            service,
            _permit: permit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation_plugins::review_bridge::ReviewPhase;
    use crate::animation_plugins::{AnimationSourceTab, PluginSelection};
    use crate::background_animation::{AmbientHost, AnimationKind};
    use crossterm::event::KeyCode;
    use ilium_ambient::{Frame, Scene};
    use ilium_animation_js::manifest::AnimationMode;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Write};
    use std::path::Path;
    use std::process::Command;

    // Serialize this module's service admissions, without making production
    // limits depend on the Rust test harness's chosen parallelism.
    static TEST_OWNER: Mutex<()> = Mutex::new(());

    #[test]
    fn wake_admission_recovers_from_transient_contention() {
        let quota = isolated_quota();
        let stop = StopToken::default();
        let mut calls = 0;
        let storage = reserve_wake_storage(&stop, || {
            calls += 1;
            if calls == 1 {
                Err(ilium_execution::RejectReason::Busy)
            } else {
                quota.reserve_external_storage(4096)
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(quota.snapshot().worker_bytes, 4096);
        drop(storage);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn wake_admission_cancels_after_contention_without_retrying() {
        let stop = StopToken::default();
        let mut calls = 0;
        let storage = reserve_wake_storage(&stop, || {
            calls += 1;
            stop.stop();
            Err(ilium_execution::RejectReason::Busy)
        })
        .unwrap();
        assert!(storage.is_none());
        assert_eq!(calls, 1);
    }

    #[test]
    fn wake_admission_preserves_genuine_capacity_failure() {
        let stop = StopToken::default();
        let mut calls = 0;
        let result = reserve_wake_storage(&stop, || {
            calls += 1;
            Err(ilium_execution::RejectReason::WorkerBytes)
        });
        assert!(matches!(
            result,
            Err(ilium_execution::RejectReason::WorkerBytes)
        ));
        assert_eq!(calls, 1);
    }

    const NATIVE_TASK_SCENE_SOURCE: &str = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{}}}
        export async function create(host){
          const opened=await host.tasks.poll({interval_ms:200,deadline_ms:1000},()=>{});
          if(!opened.ok)throw Error(opened.error.code);
          return {render(context,frame){frame.cells.set_cell(0,0,{mask:1});frame.present()},dispose(){}};
        }"#;

    fn native_task_scene_archive() -> Vec<u8> {
        let manifest = json!({"api_version":1,"id":"native-scene-tasks",
            "name":"Native scene tasks","version":"1.0.0","entry":"entry.mjs",
            "modes":["live"],"settings":{"type":"object","properties":{}},
            "files":[{"path":"entry.mjs","bytes":NATIVE_TASK_SCENE_SOURCE.len(),
                "sha256":format!("{:x}",Sha256::digest(NATIVE_TASK_SCENE_SOURCE.as_bytes()))}]});
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        archive.start_file("entry.mjs", options).unwrap();
        archive
            .write_all(NATIVE_TASK_SCENE_SOURCE.as_bytes())
            .unwrap();
        archive.start_file("manifest.json", options).unwrap();
        archive
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        archive.finish().unwrap().into_inner()
    }

    fn wait_for_task_scene(service: &AnimationService) {
        let bridge = service.permission_bridge().expect("original review bridge");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut session = loop {
            if let Some(session) = bridge.session().unwrap() {
                break session;
            }
            let status = bridge.status().unwrap();
            assert_ne!(status.phase(), ReviewPhase::Failed, "{}", status.message());
            assert!(Instant::now() < deadline, "actual no-rights review absent");
            std::thread::sleep(Duration::from_millis(5));
        };
        session.handle_key(&bridge, KeyCode::Enter).unwrap();
        drop(session);
        loop {
            let phase = bridge.status().unwrap();
            assert_ne!(phase.phase(), ReviewPhase::Failed, "{}", phase.message());
            let status = service.try_status();
            if phase.phase() == ReviewPhase::Ready
                && status
                    .as_ref()
                    .is_some_and(|status| status.native_task_timer_armed)
            {
                return;
            }
            assert!(Instant::now() < deadline, "accepted task timer not armed");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn run_native_task_scene_child(revoke_original_activation: bool) {
        let quota = crate::execution::process_quota();
        let resources =
            ilium_ambient::resources::AmbientResources::new(crate::execution::test_client());
        let worker_threads_before = quota.snapshot().worker_threads;
        let service = AnimationService::start(resources).expect("admitted actual scene worker");
        let mut selected = request(1, 0);
        selected.settings.source = AnimationSourceTab::Plugin;
        selected.settings.enabled = true;
        selected.settings.plugin.selected = Some(PluginSelection {
            package_id: "native-scene-tasks".into(),
            mode: AnimationMode::Live,
            settings: json!({}),
        });
        submit(&service, selected);
        wait_for_task_scene(&service);

        let (entered_sender, entered_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let (finished_sender, finished_receiver) = mpsc::sync_channel(1);
        let (wake_sender, wake_receiver) = mpsc::sync_channel(2);
        service
            .test_hold_real_finite_wake(
                entered_sender,
                release_receiver,
                finished_sender,
                wake_sender,
                revoke_original_activation,
            )
            .expect("real finite-wake command admission");
        entered_receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        finished_receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        wake_receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        // The accepted poll has at most a 1000 ms total lifetime. Holding the
        // scene actor past it makes the real finite wake and task due visible
        // together on the next actor turn, independent of helper startup time.
        std::thread::sleep(Duration::from_millis(1100));
        release_sender.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let status = service.try_status();
            if let Some(status) = status {
                if status.same_wake_task_and_finite > 0 && status.finite_probe_acks > 0 {
                    if revoke_original_activation {
                        if status.error.is_some() {
                            break;
                        }
                    } else {
                        assert!(status.error.is_none(), "{:?}", status.error);
                        break;
                    }
                }
            }
            assert!(
                Instant::now() < deadline,
                "real finite wake/task retirement not observed"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        if !revoke_original_activation {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match service.try_pause(2) {
                    Ok(()) => break,
                    Err(AdmissionError::Busy) if Instant::now() < deadline => {
                        std::thread::yield_now()
                    }
                    Err(error) => panic!("native reconfigure/pause refused: {error:?}"),
                }
            }
            loop {
                if service
                    .try_status()
                    .is_some_and(|status| !status.native_task_timer_armed)
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "reconfigured task timer remained armed"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            submit(&service, request(3, 0));
            loop {
                if service
                    .try_snapshot()
                    .is_some_and(|frame| frame.revision == 3)
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "replacement scene did not render after retiring task workflow"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(service
                .try_status()
                .is_some_and(|status| status.error.is_none() && !status.native_task_timer_armed));
        }
        let helper_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if service
                .try_status()
                .is_some_and(|status| status.native_helper_physically_settled)
            {
                break;
            }
            assert!(
                Instant::now() < helper_deadline,
                "original native helper remained physically owned"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let ticket = service.ticket();
        drop(service);
        ticket
            .join_until(Instant::now() + Duration::from_secs(10))
            .unwrap();
        drop(ticket);
        assert_eq!(
            quota.snapshot().worker_threads,
            worker_threads_before,
            "actual scene worker/helper debit remains after physical join"
        );
    }

    fn isolated_native_task_scene(revoke_original_activation: bool, exact_test: &str) {
        if std::env::var_os("ILIUM_NATIVE_TASK_SCENE_CHILD").is_some() {
            run_native_task_scene_child(revoke_original_activation);
            return;
        }
        let helper = std::env::var_os("ILIUM_ANIMATION_HELPER")
            .expect("explicit matching built helper path required");
        assert!(Path::new(&helper).is_absolute());
        let temporary = tempfile::tempdir().unwrap();
        let bin = temporary.path().join("bin");
        let data = temporary.path().join("data");
        let config = temporary.path().join("config");
        let cache = temporary.path().join("cache");
        let packages = data.join("ilium/animation-plugins");
        for path in [&bin, &packages, &config, &cache] {
            std::fs::create_dir_all(path).unwrap();
        }
        // SetupJob derives the real helper as a sibling of current_exe.
        // Copy both built artifacts into this isolated process; keep all
        // release::verifier(), archive, and helper sandbox checks unchanged.
        let copied_test = bin.join("ilium-client-native-task-test");
        std::fs::copy(std::env::current_exe().unwrap(), &copied_test).unwrap();
        let copied_helper = bin.join(format!(
            "ilium-animation-helper{}",
            std::env::consts::EXE_SUFFIX
        ));
        std::fs::copy(Path::new(&helper), &copied_helper).unwrap();
        std::fs::write(
            packages.join("native-scene-tasks-1.0.0.iliumanim"),
            native_task_scene_archive(),
        )
        .unwrap();
        let output = Command::new(copied_test)
            .arg("--ignored")
            .arg("--exact")
            .arg(exact_test)
            .arg("--nocapture")
            .env("ILIUM_NATIVE_TASK_SCENE_CHILD", "1")
            .env("XDG_DATA_HOME", &data)
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_CACHE_HOME", &cache)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child scene test: status {:?}, stdout {}, stderr {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
    fn real_scene_actor_combines_finite_wake_task_due_and_reconfigures() {
        isolated_native_task_scene(false,
            "background_animation::worker::tests::real_scene_actor_combines_finite_wake_task_due_and_reconfigures");
    }

    #[test]
    #[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
    fn real_scene_actor_error_retires_revoked_task_on_same_finite_wake() {
        isolated_native_task_scene(true,
            "background_animation::worker::tests::real_scene_actor_error_retires_revoked_task_on_same_finite_wake");
    }

    struct ControlledScene {
        entered: mpsc::SyncSender<std::thread::ThreadId>,
        release: mpsc::Receiver<()>,
        painted: mpsc::SyncSender<Vec<PaintedOwner>>,
    }
    impl Scene for ControlledScene {
        fn render(&mut self, frame: &mut Frame<'_>) {
            self.entered.send(std::thread::current().id()).unwrap();
            self.release.recv().unwrap();
            frame.raster.dots.fill(1.0);
            frame.raster.owner_ids.fill(7);
        }
        fn presented(&mut self, owners: &[PaintedOwner]) {
            self.painted.send(owners.to_vec()).unwrap();
        }
    }

    struct TrackedReceipt {
        semantic_block: u32,
        released: mpsc::SyncSender<std::thread::ThreadId>,
    }
    impl Drop for TrackedReceipt {
        fn drop(&mut self) {
            let _ = self.released.send(std::thread::current().id());
        }
    }
    struct SameGenerationReceipts {
        render_number: u32,
        slots: [Option<(FrameReceiptId, TrackedReceipt)>; MAX_FRAMES],
        painted: mpsc::SyncSender<u32>,
        released: mpsc::SyncSender<std::thread::ThreadId>,
    }
    impl Scene for SameGenerationReceipts {
        fn render(&mut self, frame: &mut Frame<'_>) {
            self.render_number += 1;
            frame.raster.dots.fill(1.0);
            // The raster's owner id restarts at one every render, just as
            // FrameOwners::register does for different saved positions.
            frame.raster.owner_ids.fill(1);
        }
        fn receipt_bytes(&self) -> usize {
            128
        }
        fn seal_frame(&mut self, id: FrameReceiptId) {
            self.slots[id.slot()] = Some((
                id,
                TrackedReceipt {
                    semantic_block: self.render_number,
                    released: self.released.clone(),
                },
            ));
        }
        fn presented_frame(&mut self, id: FrameReceiptId, owners: &[PaintedOwner]) {
            let Some((sealed, receipt)) = self.slots[id.slot()].as_ref() else {
                return;
            };
            if *sealed == id && owners.iter().any(|owner| owner.id == 1 && owner.dots > 0) {
                self.painted.send(receipt.semantic_block).unwrap();
            }
        }
    }

    fn request(revision: u64, seconds: u64) -> RenderRequest {
        RenderRequest {
            revision,
            settings: AnimationSettings {
                kind: AnimationKind::Stars,
                ..Default::default()
            },
            width: 2,
            height: 1,
            elapsed: Duration::from_secs(seconds),
            requested_at: Instant::now(),
            pointer: None,
            occupancy: None,
            occupancy_revision: 0,
        }
    }

    type Harness = (
        AnimationService,
        mpsc::Receiver<std::thread::ThreadId>,
        mpsc::SyncSender<()>,
        mpsc::Receiver<Vec<PaintedOwner>>,
    );
    fn controlled() -> Harness {
        controlled_in(None)
    }
    fn controlled_in(quota: Option<QuotaGroup>) -> Harness {
        let (entered, observed) = mpsc::sync_channel(8);
        let (release, advance) = mpsc::sync_channel(8);
        let (painted, receipts) = mpsc::sync_channel(8);
        let scene = Mutex::new(Some(ControlledScene {
            entered,
            release: advance,
            painted,
        }));
        let frame = AnimationFrame {
            host: AmbientHost::with_factory(Box::new(move |_, _, _| {
                Box::new(scene.lock().unwrap().take().unwrap())
            })),
            ..Default::default()
        };
        let service = match quota {
            Some(quota) => AnimationService::start_admitted(
                AnimationService::reserve_in(quota).unwrap(),
                Some(frame),
                Arc::new(tokio::sync::Notify::new()),
                ilium_ambient::resources::AmbientResources::new(crate::execution::test_client()),
            )
            .unwrap(),
            None => AnimationService::with_frame(frame).unwrap(),
        };
        (service, observed, release, receipts)
    }

    fn submit(service: &AnimationService, mut request: RenderRequest) {
        loop {
            match service.try_request(request) {
                Ok(()) => return,
                Err(rejected) if rejected.reason == AdmissionError::Busy => {
                    request = rejected.value;
                    std::thread::yield_now();
                }
                Err(rejected) => panic!("Unexpected admission: {:?}", rejected.reason),
            }
        }
    }

    fn snapshot(service: &AnimationService) -> Arc<FrameSnapshot> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(frame) = service.try_snapshot() {
                return frame;
            }
            assert!(Instant::now() < deadline, "No animation snapshot");
            std::thread::yield_now();
        }
    }

    fn isolated_quota() -> QuotaGroup {
        QuotaGroup::new(ilium_execution::QuotaLimits {
            worker_threads: 1,
            worker_bytes: MAX_FRAME_BYTES * MAX_FRAMES,
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
        })
    }

    #[test]
    fn plugin_identity_qualifies_user_archive_and_rejects_mismatched_revision() {
        let _owner = TEST_OWNER.lock().unwrap();
        let (service, entered, release, _) = controlled();
        submit(&service, request(1, 0));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        release.send(()).unwrap();
        let mut retained = snapshot(&service);
        assert!(retained.plugin_identity().is_none());
        let frame = Arc::get_mut(&mut retained).unwrap();
        frame.plugin_identity = Some(PluginFrameIdentity {
            package_id: "user_animation".into(),
            package_digest: "a".repeat(64),
            verified_ilium: false,
            instance_id: 7,
            revision: frame.revision,
            plan_generation: 9,
            authorization_epoch: 2,
        });
        assert_eq!(frame.plugin_package_digest(), Some("a".repeat(64).as_str()));
        assert!(!frame.plugin_identity().unwrap().verified_ilium);
        frame.revision += 1;
        assert!(frame.plugin_identity().is_none());
        assert!(frame.plugin_package_digest().is_none());
    }

    #[test]
    fn shared_thread_credit_survives_blocked_retirement_until_actual_join() {
        let _owner = TEST_OWNER.lock().unwrap();
        let quota = isolated_quota();
        let (service, entered, release, _) = controlled_in(Some(quota.clone()));
        submit(&service, request(1, 0));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        let ticket = service.ticket();
        drop(service);
        assert_eq!(quota.snapshot().worker_threads, 1);
        assert!(AnimationService::reserve_in(quota.clone()).is_err());
        release.send(()).unwrap();
        ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .unwrap();
        // The join observer owns platform wake state conservatively. Release
        // that completed observer before expecting its physical debit back.
        assert_eq!(quota.snapshot().worker_threads, 1);
        drop(ticket);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert!(AnimationService::reserve_in(quota).is_ok());
    }

    #[test]
    fn frame_storage_survives_engine_join_until_the_last_snapshot_clone() {
        let _owner = TEST_OWNER.lock().unwrap();
        let quota = isolated_quota();
        let (service, entered, release, _) = controlled_in(Some(quota.clone()));
        submit(&service, request(1, 0));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        // The controlled render is parked before frame packing. Capture the
        // already admitted review/wake storage independently of frame custody.
        let engine_storage = quota.snapshot().worker_bytes;
        release.send(()).unwrap();
        let frame = snapshot(&service);
        let retained = Arc::clone(&frame);
        assert_eq!(
            quota.snapshot().worker_bytes,
            engine_storage + MAX_FRAME_BYTES
        );
        let ticket = service.ticket();
        drop(service);
        ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .unwrap();
        // The join observer owns platform wake state conservatively. Release
        // that completed observer before expecting its physical debit back.
        assert_eq!(quota.snapshot().worker_threads, 1);
        drop(ticket);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, MAX_FRAME_BYTES);
        drop(frame);
        assert_eq!(quota.snapshot().worker_bytes, MAX_FRAME_BYTES);
        drop(retained);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn real_thread_publishes_finished_time_despite_newer_pending_time() {
        let _owner = TEST_OWNER.lock().unwrap();
        let (service, entered, release, _receipts) = controlled();
        submit(&service, request(1, 0));
        assert_ne!(
            entered.recv_timeout(Duration::from_secs(3)).unwrap(),
            std::thread::current().id()
        );
        for seconds in 1..100 {
            submit(&service, request(1, seconds));
        }
        release.send(()).unwrap();
        // The next render blocks. Its newer clock must not suppress frame zero.
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        let first = snapshot(&service);
        assert_eq!(first.revision, 1);
        assert_eq!(first.elapsed, Duration::ZERO);
        release.send(()).unwrap();
        assert_eq!(snapshot(&service).elapsed, Duration::from_secs(99));
        let ticket = service.ticket();
        drop(service);
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .is_ok());
    }

    #[test]
    fn resize_revision_fences_old_frame_and_receipts_use_original_owners() {
        let _owner = TEST_OWNER.lock().unwrap();
        let (service, entered, release, receipts) = controlled();
        submit(&service, request(1, 0));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        let mut resized = request(2, 1);
        resized.width = 3;
        submit(&service, resized);
        release.send(()).unwrap();
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(service.try_snapshot().is_none());
        release.send(()).unwrap();
        let frame = snapshot(&service);
        assert_eq!(frame.width, 3);
        assert!(
            receipts.try_recv().is_err(),
            "Generation alone must not credit painted history"
        );
        let surviving = vec![frame.cell(0, 0).unwrap().packed_bits, 0, 0];
        let mut receipt = frame
            .begin_presentation()
            .unwrap()
            .receipt(surviving)
            .unwrap();
        loop {
            match service.try_receipt(receipt) {
                Ok(()) => break,
                Err(rejected) if rejected.reason == AdmissionError::Busy => {
                    receipt = rejected.value;
                }
                Err(rejected) => panic!("Receipt rejected: {:?}", rejected.reason),
            }
        }
        let owners = receipts.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(owners[0].id, 7);
        assert!(owners[0].dots > 0 && owners[0].dots <= 8);
        let ticket = service.ticket();
        drop(service);
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .is_ok());
    }

    #[test]
    fn two_same_generation_frames_keep_exact_owner_tables_and_retire_on_worker() {
        let _owner = TEST_OWNER.lock().unwrap();
        let (painted, observed) = mpsc::sync_channel(8);
        let (released, drops) = mpsc::sync_channel(8);
        let scene = Mutex::new(Some(SameGenerationReceipts {
            render_number: 0,
            slots: std::array::from_fn(|_| None),
            painted,
            released,
        }));
        let frame = AnimationFrame {
            host: AmbientHost::with_factory(Box::new(move |_, _, _| {
                Box::new(scene.lock().unwrap().take().unwrap())
            })),
            ..Default::default()
        };
        let service = AnimationService::with_frame(frame).unwrap();
        submit(&service, request(1, 0));
        let first = snapshot(&service);
        submit(&service, request(1, 1));
        let second = snapshot(&service);
        assert_eq!(first.scene_generation, second.scene_generation);
        assert_ne!(first.scene_receipt_id, second.scene_receipt_id);
        assert_eq!(first.owner_ids[0], 1);
        assert_eq!(second.owner_ids[0], 1);

        for snapshot in [&first, &second] {
            let surviving = vec![snapshot.cell(0, 0).unwrap().packed_bits, 0];
            let mut receipt = snapshot
                .begin_presentation()
                .unwrap()
                .receipt(surviving)
                .unwrap();
            loop {
                match service.try_receipt(receipt) {
                    Ok(()) => break,
                    Err(rejected) if rejected.reason == AdmissionError::Busy => {
                        receipt = rejected.value;
                    }
                    Err(rejected) => panic!("Receipt rejected: {:?}", rejected.reason),
                }
            }
        }
        assert_eq!(observed.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
        assert_eq!(observed.recv_timeout(Duration::from_secs(3)).unwrap(), 2);

        // Once the first snapshot/lease is gone, its slot can be reused. The
        // replaced heavy semantic context drops on this worker, not the UI.
        drop(first);
        submit(&service, request(1, 2));
        let third = snapshot(&service);
        assert_eq!(third.scene_receipt_id.unwrap().slot(), 0);
        let worker_drop = drops.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_ne!(worker_drop, std::thread::current().id());
        let ticket = service.ticket();
        drop(service);
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .is_ok());
    }

    #[test]
    fn admission_returns_oversized_request_and_drop_does_not_wait_for_render() {
        let _owner = TEST_OWNER.lock().unwrap();
        let (service, entered, release, _receipts) = controlled();
        let mut huge = request(1, 0);
        huge.width = u16::MAX;
        huge.height = u16::MAX;
        let rejected = service.try_request(huge).unwrap_err();
        assert_eq!(rejected.reason, AdmissionError::Invalid);
        assert_eq!(rejected.value.width, u16::MAX);
        submit(&service, request(1, 0));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        let ticket = service.ticket();
        let started = Instant::now();
        drop(service);
        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(ticket.exit().is_none());
        release.send(()).unwrap();
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .is_ok());
    }
    #[test]
    fn pause_is_ordered_bounded_and_fences_a_blocked_render() {
        let _owner = TEST_OWNER.lock().unwrap();
        let (service, entered, release, _receipts) = controlled();
        submit(&service, request(1, 0));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        for revision in 2..=16 {
            loop {
                match service.try_pause(revision) {
                    Ok(()) => break,
                    Err(AdmissionError::Busy) => std::thread::yield_now(),
                    Err(error) => panic!("Unexpected pause admission: {error:?}"),
                }
            }
        }
        assert_eq!(service.try_pause(17), Err(AdmissionError::Full));
        release.send(()).unwrap();
        let ticket = service.ticket();
        drop(service);
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .is_ok());
    }

    #[test]
    fn retained_frames_bound_memory_without_discarding_the_callers_frames() {
        let _owner = TEST_OWNER.lock().unwrap();
        let (service, entered, release, _receipts) = controlled();
        let mut retained = Vec::new();
        for seconds in 0..3 {
            submit(&service, request(1, seconds));
            entered.recv_timeout(Duration::from_secs(3)).unwrap();
            release.send(()).unwrap();
            retained.push(snapshot(&service));
        }
        submit(&service, request(1, 3));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if service
                .try_status()
                .is_some_and(|status| status.limited_frames > 0)
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(service.try_snapshot().is_none());
        assert_eq!(retained[0].elapsed, Duration::ZERO);
        drop(retained.remove(0));
        submit(&service, request(1, 4));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        release.send(()).unwrap();
        assert_eq!(snapshot(&service).elapsed, Duration::from_secs(4));
        let ticket = service.ticket();
        drop(service);
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .is_ok());
    }
    #[test]
    fn spare_settings_capacity_is_rejected_before_mailbox_retention() {
        let _owner = TEST_OWNER.lock().unwrap();
        // Other UI fixtures share the real four-service bank. Wait for actual
        // retirement just as the other worker fixtures do; this test exercises
        // settings capacity after admission, rather than transient saturation.
        let service = AnimationService::with_frame(AnimationFrame::default()).unwrap();
        let mut oversized = request(1, 0);
        oversized.settings.ambient.video.source = String::with_capacity(1024 * 1024);
        assert!(oversized.settings.ambient.video.source.is_empty());
        let rejected = service.try_request(oversized).unwrap_err();
        assert_eq!(rejected.reason, AdmissionError::Invalid);
        assert!(rejected.value.settings.ambient.video.source.capacity() >= 1024 * 1024);
        let ticket = service.ticket();
        drop(service);
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .is_ok());
    }

    #[test]
    fn scene_retirement_waits_for_emitted_old_frame_ack_but_not_display_arc() {
        let _owner = TEST_OWNER.lock().unwrap();
        let (service, entered, release, receipts) = controlled();
        submit(&service, request(1, 0));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        release.send(()).unwrap();
        let old = snapshot(&service);
        let lease = old.begin_presentation().unwrap();
        let mut next = request(2, 1);
        next.settings.kind = AnimationKind::Shoreline;
        assert_eq!(
            service.try_request(next.clone()).unwrap_err().reason,
            AdmissionError::RevisionNeedsBarrier
        );
        // Time continues while an old presentation is in flight.
        submit(&service, request(1, 1));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        let surviving = old.cell(0, 0).unwrap().packed_bits;
        assert_ne!(surviving, 0);
        let mut receipt = lease.receipt(vec![surviving, 0]).unwrap();
        loop {
            match service.try_receipt(receipt) {
                Ok(()) => break,
                Err(rejected) if rejected.reason == AdmissionError::Busy => {
                    receipt = rejected.value
                }
                Err(rejected) => panic!("Receipt rejected: {:?}", rejected.reason),
            }
        }
        assert_eq!(
            service.try_request(next.clone()).unwrap_err().reason,
            AdmissionError::RevisionNeedsBarrier
        );
        release.send(()).unwrap();
        let owners = receipts.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(
            owners,
            vec![PaintedOwner {
                id: 7,
                dots: surviving.count_ones()
            }]
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match service.try_request(next) {
                Ok(()) => break,
                Err(rejected)
                    if matches!(
                        rejected.reason,
                        AdmissionError::Busy | AdmissionError::RevisionNeedsBarrier
                    ) =>
                {
                    assert!(Instant::now() < deadline);
                    next = rejected.value;
                    std::thread::yield_now();
                }
                Err(rejected) => panic!("Revision rejected: {:?}", rejected.reason),
            }
        }
        // Retaining the old immutable frame for display is harmless; attempting
        // another presentation after the retirement boundary is stale.
        assert!(matches!(
            old.begin_presentation(),
            Err(AdmissionError::Stale)
        ));
        assert_eq!(snapshot(&service).revision, 2);
        let ticket = service.ticket();
        drop(service);
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .is_ok());
    }
}
