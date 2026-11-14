//! One persistent scene owner. Time requests replace pending time requests;
//! emitted-frame receipts are ordered and are never silently discarded.
use super::{AnimationCacheStatus, AnimationFrame, AnimationLoopCache, AnimationSettings};
use ilium_ambient::raster::PaintedOwner;
use ilium_ambient::scene::{FrameReceiptId, MAX_SCENE_RECEIPT_SLOTS};
use ilium_execution::{QuotaGroup, StorageAdmission, WorkerAdmission};
use ilium_platform::owned_worker::{
    reserve_owned_worker, OwnedWorker, StopToken, WorkerKind, WorkerReservation, WorkerTicket,
};
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
const ANIMATION_STACK_BYTES: usize = 2 * 1024 * 1024;
fn service_count() -> Arc<AtomicUsize> {
    #[cfg(test)]
    {
        thread_local! { static SERVICES: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0)); }
        SERVICES.with(Arc::clone)
    }
    #[cfg(not(test))]
    {
        static SERVICES: OnceLock<Arc<AtomicUsize>> = OnceLock::new();
        Arc::clone(SERVICES.get_or_init(|| Arc::new(AtomicUsize::new(0))))
    }
}
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
    pub image_background: Option<(u8, u8, u8)>,
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
    scene_generation: Option<u64>,
    scene_receipt_id: Option<FrameReceiptId>,
    owner_ids: Vec<u32>,
    service: Weak<Shared>,
    _permit: FramePermit,
}

impl FrameSnapshot {
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
}

enum SceneCommand {
    Configure(Box<RenderRequest>),
    Emitted(EmissionReceipt),
    Pause,
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
    frames: Arc<AtomicUsize>,
    slots: Arc<AtomicU8>,
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
    // Retain the admitting scope through physical retirement on another thread.
    services: Arc<AtomicUsize>,
    // The platform wake owner retains this through actual OS join/TLS exit.
    _physical: WorkerAdmission,
}
impl Drop for ServicePermit {
    fn drop(&mut self) {
        self.services.fetch_sub(1, Ordering::AcqRel);
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
    worker: WorkerReservation<ServicePermit>,
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
        let physical = quota
            .reserve_external_worker(1, ANIMATION_STACK_BYTES)
            .map_err(|reason| {
                let kind = match reason {
                    ilium_execution::RejectReason::Busy
                    | ilium_execution::RejectReason::WorkerLimit
                    | ilium_execution::RejectReason::WorkerBytes => io::ErrorKind::WouldBlock,
                    _ => io::ErrorKind::Other,
                };
                io::Error::new(kind, format!("animation worker admission: {reason:?}"))
            })?;
        let services = service_count();
        services
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_SERVICES).then_some(count + 1)
            })
            .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "animation services full"))?;
        let worker = reserve_owned_worker(
            Some(ANIMATION_STACK_BYTES),
            ServicePermit {
                services,
                _physical: physical,
            },
        )
        .map_err(io::Error::other)?;
        Ok(AnimationAdmission { quota, worker })
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
            frames: Arc::new(AtomicUsize::new(0)),
            slots: Arc::new(AtomicU8::new(0)),
        });
        let wake = Arc::clone(&shared);
        let engine = Arc::clone(&shared);
        let worker = admission.worker.spawn(
            "ilium-animation",
            WorkerKind::Cooperative,
            shared.stop.clone(),
            move || {
                wake.changed.notify_all();
            },
            move |stop| {
                // Unit tests run many debug-build workers in parallel on small
                // runners; a lowest-priority worker is starved there and the
                // bounded settle waits time out. Production keeps the demotion.
                if !cfg!(test) {
                    ilium_platform::thread_priority::lower_current_thread(
                        ilium_platform::thread_priority::WorkerPriority::Lowest,
                    );
                }
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
    pub fn ticket(&self) -> WorkerTicket {
        self.worker.ticket()
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

    /// One mailbox lock for the UI's per-tick collection: the status and the
    /// completed frame together, so the worker cannot slip in between the two
    /// reads and leave the UI with a status but no frame this tick.
    pub fn try_collect(&self) -> Option<(ServiceStatus, Option<Arc<FrameSnapshot>>)> {
        let mut mailbox = self.shared.mailbox.try_lock().ok()?;
        let mut status = mailbox.status.clone();
        status.retained_frames = self.shared.frames.load(Ordering::Acquire);
        status.is_accepting = self.shared.is_accepting();
        let frame = mailbox.output.take();
        Some((status, frame))
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
    let strings = [
        &ambient.location.label,
        &ambient.video.source,
        &ambient.spectrum.device_name,
        &ambient.images.folders,
        &ambient.images.urls,
        &ambient.openstreetmap.local_path,
        &ambient.openstreetmap.endpoint,
        &ambient.openstreetmap.coordinates,
    ];
    // std's B-tree nodes contain at most eleven key/value entries. Charging a
    // whole 4KiB node per entry (plus an empty root) conservatively covers the
    // current String/PackSourceSettings node layout and child links.
    let mut retained = std::mem::size_of::<AnimationSettings>();
    for string in strings {
        let Some(next) = retained.checked_add(string.capacity()) else {
            return false;
        };
        retained = next;
        if retained > MAX_SETTINGS_RETAINED_BYTES {
            return false;
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
// explicit admission errors. The operation closure permits deterministic forcing
// of contention without exposing the quota ledger's private lock.
fn reserve_worker_storage(
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

// The caller already owns one bounded frame slot. Failure or cancellation
// must retire only that slot before returning; successful packing transfers
// the slot to FramePermit with the storage lease.
fn reserve_frame_storage(
    stop: &StopToken,
    frames: &AtomicUsize,
    slots: &AtomicU8,
    slot: u8,
    reserve: impl FnMut() -> Result<StorageAdmission, ilium_execution::RejectReason>,
) -> Result<Option<StorageAdmission>, ilium_execution::RejectReason> {
    let result = reserve_worker_storage(stop, reserve);
    if !result.as_ref().is_ok_and(|storage| storage.is_some()) {
        slots.fetch_and(!(1 << slot), Ordering::AcqRel);
        frames.fetch_sub(1, Ordering::AcqRel);
    }
    result
}

fn run(
    shared: &Arc<Shared>,
    mut frame: AnimationFrame,
    stop: StopToken,
    resources: ilium_ambient::resources::AmbientResources,
) {
    // Close command admission on every exit.
    let startup_admission = AdmissionGuard(shared);
    let mut cache = AnimationLoopCache::new(resources);
    // Move the SAME guard after cache construction so admission closes
    // before its destructor work, including on panic or orderly exit.
    let _admission = startup_admission;
    let mut sequence = 0;
    loop {
        let (request, receipts, stopping) = {
            let mut mailbox = shared.mailbox.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if stop.is_stopped() || mailbox.latest.is_some() || !mailbox.receipts.is_empty() {
                    break;
                }
                // Preserve the existing stop wait. A condvar notification can
                // race sleep; the fixed stop observation bounds the delay.
                mailbox = shared
                    .changed
                    .wait_timeout(mailbox, Duration::from_millis(50))
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            let stopping = stop.is_stopped();
            if stopping {
                shared.accepting.store(false, Ordering::Release);
            }
            let request = mailbox.latest.take().filter(|_| !stopping);
            (request, std::mem::take(&mut mailbox.receipts), stopping)
        };
        for command in receipts {
            match command {
                SceneCommand::Configure(request) => {
                    let _configuration = ConfigurationGuard(shared);
                    render_request(
                        shared,
                        &mut frame,
                        &mut cache,
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
                }
            }
        }
        shared.ready.notify_one();
        // Successfully admitted semantic receipts drain even on cancellation.
        if stopping {
            return;
        }
        let Some(request) = request else {
            continue;
        };
        render_request(
            shared,
            &mut frame,
            &mut cache,
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
    sequence: &mut u64,
    stop: &StopToken,
    request: RenderRequest,
) {
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
    let storage = match reserve_frame_storage(stop, &shared.frames, &shared.slots, slot, || {
        shared.quota.reserve_external_storage(MAX_FRAME_BYTES)
    }) {
        Ok(Some(storage)) => storage,
        Ok(None) => return,
        Err(reason) => {
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
                    image_background: image_background(
                        color,
                        scene_color,
                        self.raster
                            .image_coverage
                            .get(usize::from(y) * usize::from(self.width) + usize::from(x))
                            .copied(),
                        &request.settings.appearance,
                    ),
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
            geometry_render_count: 0,
            completed_at: Instant::now(),
            resident_bytes: bytes,
            status,
            frames_per_second: self.host.frames_per_second(),
            cache,
            is_wikipedia: self.is_wikipedia,
            has_cell_colors: self.has_cell_colors,
            cells,
            scene_generation: self.last_ambient.map(|key| key.generation),
            scene_receipt_id,
            owner_ids: self.raster.owner_ids.clone(),
            service,
            _permit: permit,
        })
    }
}

fn image_background(
    styled: Option<(u8, u8, u8)>,
    scene: Option<(u8, u8, u8)>,
    coverage: Option<u8>,
    appearance: &ilium_ambient::style::Appearance,
) -> Option<(u8, u8, u8)> {
    let coverage = coverage.filter(|coverage| *coverage > 0)?.min(8);
    let (red, green, blue) = styled?;
    // Monotone uses one ink color, so preserve image detail as ink intensity.
    let tone = if appearance.mode == ilium_ambient::style::ColorMode::Monotone {
        let (red, green, blue) = scene.unwrap_or((255, 255, 255));
        let luma = (54 * u32::from(red) + 183 * u32::from(green) + 19 * u32::from(blue)) / 256;
        if appearance.invert {
            255 - luma
        } else {
            luma
        }
    } else {
        255
    };
    let dim = |channel: u8| (u32::from(channel) * u32::from(coverage) * tone / (16 * 255)) as u8;
    Some((dim(red), dim(green), dim(blue)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::background_animation::{AmbientHost, AnimationKind};
    use ilium_ambient::{Frame, Scene};

    // Serialize this module's service admissions, without making production
    // limits depend on the Rust test harness's chosen parallelism.
    static TEST_OWNER: Mutex<()> = Mutex::new(());

    #[test]
    fn image_background_monotone_preserves_image_tone_and_inversion() {
        let mut look = ilium_ambient::style::Appearance::default();
        look.mode = ilium_ambient::style::ColorMode::Monotone;
        let ink = Some((240, 120, 60));
        let dark = image_background(ink, Some((32, 32, 32)), Some(8), &look).unwrap();
        let light = image_background(ink, Some((224, 224, 224)), Some(8), &look).unwrap();
        assert!(dark.0 < light.0 && dark.1 < light.1 && dark.2 < light.2);
        assert_eq!(image_background(ink, None, Some(0), &look), None);
        look.invert = true;
        assert_eq!(
            image_background(ink, Some((223, 223, 223)), Some(8), &look),
            Some(dark)
        );
        look.mode = ilium_ambient::style::ColorMode::Color;
        assert_eq!(
            image_background(ink, Some((32, 32, 32)), Some(8), &look),
            Some((120, 60, 30))
        );
    }

    #[test]
    fn parallel_fixture_services_keep_the_original_four_owner_limit() {
        let admissions: Vec<_> = (0..MAX_SERVICES)
            .map(|_| AnimationService::reserve().unwrap())
            .collect();
        assert!(AnimationService::reserve().is_err());
        std::thread::spawn(|| {
            let admissions: Vec<_> = (0..MAX_SERVICES)
                .map(|_| AnimationService::reserve().unwrap())
                .collect();
            assert!(AnimationService::reserve().is_err());
            drop(admissions);
            assert_eq!(service_count().load(Ordering::Acquire), 0);
        })
        .join()
        .unwrap();
        assert_eq!(service_count().load(Ordering::Acquire), MAX_SERVICES);
        drop(admissions);
        assert_eq!(service_count().load(Ordering::Acquire), 0);
        assert!(AnimationService::reserve().is_ok());
    }

    #[test]
    fn wake_admission_recovers_from_transient_contention() {
        let quota = isolated_quota();
        let stop = StopToken::default();
        let mut calls = 0;
        let storage = reserve_worker_storage(&stop, || {
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
    fn snapshot_admission_retries_busy_without_losing_accepted_frame_storage() {
        let quota = isolated_quota();
        let stop = StopToken::default();
        let frames = AtomicUsize::new(1);
        let slots = AtomicU8::new(0b10);
        let mut calls = 0;
        let storage = reserve_frame_storage(&stop, &frames, &slots, 1, || {
            calls += 1;
            if calls <= 2 {
                Err(ilium_execution::RejectReason::Busy)
            } else {
                quota.reserve_external_storage(MAX_FRAME_BYTES)
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(frames.load(Ordering::Acquire), 1);
        assert_eq!(slots.load(Ordering::Acquire), 0b10);
        assert_eq!(quota.snapshot().worker_bytes, MAX_FRAME_BYTES);
        drop(storage);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn snapshot_admission_cancellation_releases_only_its_reserved_slot_and_count() {
        let stop = StopToken::default();
        let frames = AtomicUsize::new(2);
        let slots = AtomicU8::new(0b1010);
        let mut calls = 0;
        let result = reserve_frame_storage(&stop, &frames, &slots, 1, || {
            calls += 1;
            assert_eq!(frames.load(Ordering::Acquire), 2);
            assert_eq!(slots.load(Ordering::Acquire), 0b1010);
            stop.stop();
            Err(ilium_execution::RejectReason::Busy)
        })
        .unwrap();
        assert!(result.is_none());
        assert_eq!(calls, 1);
        assert_eq!(frames.load(Ordering::Acquire), 1);
        assert_eq!(slots.load(Ordering::Acquire), 0b1000);
    }

    #[test]
    fn snapshot_admission_capacity_failure_releases_only_its_reserved_slot_and_count() {
        let stop = StopToken::default();
        let frames = AtomicUsize::new(2);
        let slots = AtomicU8::new(0b1010);
        let mut calls = 0;
        let result = reserve_frame_storage(&stop, &frames, &slots, 1, || {
            calls += 1;
            Err(ilium_execution::RejectReason::WorkerBytes)
        });
        assert!(matches!(
            result,
            Err(ilium_execution::RejectReason::WorkerBytes)
        ));
        assert_eq!(calls, 1);
        assert_eq!(frames.load(Ordering::Acquire), 1);
        assert_eq!(slots.load(Ordering::Acquire), 0b1000);
    }

    #[test]
    fn wake_admission_cancels_after_contention_without_retrying() {
        let stop = StopToken::default();
        let mut calls = 0;
        let storage = reserve_worker_storage(&stop, || {
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
        let result = reserve_worker_storage(&stop, || {
            calls += 1;
            Err(ilium_execution::RejectReason::WorkerBytes)
        });
        assert!(matches!(
            result,
            Err(ilium_execution::RejectReason::WorkerBytes)
        ));
        assert_eq!(calls, 1);
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
                kind: AnimationKind::NightLights,
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

    fn presentation(frame: &Arc<FrameSnapshot>) -> PresentationLease {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match frame.begin_presentation() {
                Ok(lease) => return lease,
                Err(AdmissionError::Busy) => {
                    assert!(
                        Instant::now() < deadline,
                        "presentation mailbox remained busy"
                    );
                    std::thread::yield_now();
                }
                Err(reason) => panic!("presentation refused: {reason:?}"),
            }
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
    fn animation_worker_stack_is_charged_before_native_start() {
        let _owner = TEST_OWNER.lock().unwrap();
        const STACK_BYTES: usize = 2 * 1024 * 1024;
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            worker_threads: 1,
            worker_bytes: STACK_BYTES - 1,
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
        });

        match AnimationService::reserve_in(quota) {
            Err(error) => assert!(
                error.to_string().contains("WorkerBytes"),
                "expected byte-ledger refusal for animation stack, got: {error}"
            ),
            Ok(_) => panic!("animation owner admission omitted its declared native stack"),
        }

        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            worker_threads: 1,
            worker_bytes: STACK_BYTES,
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
        });
        let admission = AnimationService::reserve_in(quota.clone()).unwrap();
        assert_eq!(quota.snapshot().worker_bytes, STACK_BYTES);
        drop(admission);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn shared_thread_credit_survives_blocked_retirement_until_actual_join() {
        let _owner = TEST_OWNER.lock().unwrap();
        let quota = isolated_quota();
        let (service, entered, release, _) = controlled_in(Some(quota.clone()));
        assert_eq!(
            service.ticket().metadata().requested_stack_bytes,
            Some(2 * 1024 * 1024),
            "animation worker must request the stack charged during admission"
        );
        submit(&service, request(1, 0));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        let admitted_bytes = quota.snapshot().worker_bytes;
        assert!(admitted_bytes >= 2 * 1024 * 1024);
        let ticket = service.ticket();
        drop(service);
        assert_eq!(quota.snapshot().worker_threads, 1);
        assert_eq!(
            quota.snapshot().worker_bytes,
            admitted_bytes,
            "the stack debit remains while the OS thread is still blocked"
        );
        assert!(AnimationService::reserve_in(quota.clone()).is_err());
        release.send(()).unwrap();
        ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .unwrap();
        // A successful join observes physical retirement and releases thread
        // admission even while this lightweight ticket remains alive.
        assert_eq!(quota.snapshot().worker_threads, 0);
        drop(ticket);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
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
        // Frame storage outlives engine retirement; thread admission does not.
        assert_eq!(quota.snapshot().worker_threads, 0);
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
        let mut receipt = presentation(&frame).receipt(surviving).unwrap();
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
    fn same_size_scene_revision_fences_an_in_flight_old_frame() {
        let _owner = TEST_OWNER.lock().unwrap();
        let (service, entered, release, _receipts) = controlled();
        submit(&service, request(1, 0));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();

        submit(&service, request(2, 1));
        release.send(()).unwrap();
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(
            service.try_snapshot().is_none(),
            "same-size scene revision must fence the old frame"
        );

        release.send(()).unwrap();
        let frame = snapshot(&service);
        assert_eq!(frame.revision, 2);
        assert_eq!((frame.width, frame.height), (2, 1));
        assert_eq!(frame.elapsed, Duration::from_secs(1));

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
            let mut receipt = presentation(snapshot).receipt(surviving).unwrap();
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
        let stale = service.try_request(request(1, 1)).unwrap_err();
        assert_eq!(stale.reason, AdmissionError::Stale);
        assert_eq!(stale.value.revision, 1);
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            // Status collection is deliberately nonblocking. The actor may
            // hold its mailbox while consuming the newly released pauses.
            if service.try_status().is_some_and(|status| {
                status.stale > 0 && service.shared.configurations.load(Ordering::Acquire) == 0
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "pause receipts did not drain");
            std::thread::yield_now();
        }
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
        let lease = presentation(&old);
        let mut next = request(2, 1);
        next.settings.kind = AnimationKind::Clouds;
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
        let stale_deadline = Instant::now() + Duration::from_secs(3);
        while !matches!(old.begin_presentation(), Err(AdmissionError::Stale)) {
            assert!(
                Instant::now() < stale_deadline,
                "old frame never went stale"
            );
            std::thread::yield_now();
        }
        assert_eq!(snapshot(&service).revision, 2);
        let ticket = service.ticket();
        drop(service);
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .is_ok());
    }
}
