//! One process-owned fleet service, bounded subscriptions and worker-owned reclamation.
//! Atomic disk snapshots share original receipts across local client processes.
use super::{
    model::{FeedState, Position},
    openseafeed, parse, rate,
}; // Existing validated adapters.
use crate::{
    resources::{AmbientResources, WorkerCost},
    source::{http_get_stoppable, Worker},
}; // Reuse bounded HTTP and admitted owned threads.
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::{file_lock::ExclusiveFileLock, secure_fs}; // Existing private file primitives.
use std::{
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
}; // No added dependencies.
const SUBSCRIBERS: usize = 8; // Maximum simultaneous subscribers per source, not queued work.
const RETAINED_FLEETS: usize = 6; // Current plus pinned older generations, across all three sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)] // Closed provider identity prevents cross-source reuse.
pub(super) enum FleetSource {
    OpenSeaFeed,
    Digitraffic,
    Aircraft,
} // Aircraft remains airborne-only.
impl FleetSource {
    // Source-specific admission and wire contracts.
    const ALL: [Self; 3] = [Self::OpenSeaFeed, Self::Digitraffic, Self::Aircraft]; // Finite service slots.
    fn index(self) -> usize {
        match self {
            Self::OpenSeaFeed => 0,
            Self::Digitraffic => 1,
            Self::Aircraft => 2,
        }
    } // Stable internal indexes.
    pub fn floor(self) -> u64 {
        match self {
            Self::OpenSeaFeed => openseafeed::MINIMUM_POLL_SECONDS,
            Self::Digitraffic => 30,
            Self::Aircraft => 900,
        }
    } // Minimum client intervals.
    fn name(self) -> &'static str {
        match self {
            Self::OpenSeaFeed => "openseafeed",
            Self::Digitraffic => "digitraffic",
            Self::Aircraft => "opensky",
        }
    } // Existing reservation names retained.
    fn limit(self) -> usize {
        if self == Self::OpenSeaFeed {
            openseafeed::MAX_RESPONSE_BYTES
        } else {
            parse::MAX_RESPONSE_BYTES
        }
    } // Do not widen other providers.
    fn retained_storage_bytes(self) -> usize {
        // Reserve before transport/decoding. The encoded-body allowance covers
        // retained string capacities; doubled row storage covers Vec growth,
        // Position values and per-allocation bookkeeping with bounded slack.
        let rows = if self == Self::OpenSeaFeed {
            openseafeed::MAX_RECORDS
        } else {
            parse::MAX_ITEMS
        };
        self.limit()
            .saturating_mul(2)
            .saturating_add(rows.saturating_mul(
                2 * std::mem::size_of::<Position>() + 4 * std::mem::size_of::<usize>(),
            ))
            .saturating_add(1024 * 1024)
    } // Conservative bounded charge follows the retained immutable batch.
    fn url(self) -> &'static str {
        match self {
            Self::OpenSeaFeed => openseafeed::ENDPOINT,
            Self::Digitraffic => "https://meri.digitraffic.fi/api/ais/v1/locations",
            Self::Aircraft => "https://opensky-network.org/api/states/all",
        }
    } // Public documented endpoints only.
} // End block.
#[derive(Debug, Default, Clone, Copy)] // Metadata never owns fleet-sized allocations.
pub(super) struct FleetMeta {
    // Source time, counts and omission reasons stay separate.
    pub generated_ms: Option<i64>, // Snapshot build, never a coordinate fix.
    pub latest_ais_ms: Option<i64>, // Latest ANY AIS message, never a coordinate fix.
    pub latest_fix_ms: Option<i64>, // Latest known fix among retained positions, not fleet freshness.
    pub known_fixes: usize,         // Quantify missing fix timestamps.
    pub counts: openseafeed::FleetCounts, // Disjoint source-row accounting.
    pub filtered: usize,            // Surface reports intentionally omitted by the aircraft parser.
} // End block.
#[derive(Debug)] // Immutable source result and its allocation charge travel together.
pub(super) struct FleetBatch {
    pub positions: Arc<Vec<Position>>,
    pub meta: FleetMeta,
    pub _storage: Option<Arc<StorageAdmission>>, // None is limited to unpublished test fixtures.
} // Geometry identity remains stable on cache hits.
#[derive(Debug, Default, Clone)] // Small publications can change without cloning position strings.
pub(super) struct FleetView {
    pub data: Option<Arc<FleetBatch>>,
    pub state: FeedState,
    pub generation: u64,
} // Receipt and data generations are independent.
struct SourceState {
    // One cache and cancellation flag per provider.
    users: [AtomicU64; SUBSCRIBERS], // Zero is vacant; positive values are requested intervals.
    cancel: AtomicBool, // Last subscriber cancels an in-flight request without a mutex.
    latest: Mutex<Arc<FleetView>>, // Readers only use try_lock.
} // End block.
impl Default for SourceState {
    // Allocation is constant-sized, with no network activity.
    fn default() -> Self {
        Self {
            users: std::array::from_fn(|_| AtomicU64::new(0)),
            cancel: AtomicBool::new(false),
            latest: Mutex::new(Arc::new(FleetView::default())),
        }
    } // Empty source cache.
} // End block.
impl SourceState {
    // All subscriber operations are atomic and bounded.
    fn interval(&self) -> Option<u64> {
        self.users
            .iter()
            .map(|slot| slot.load(Ordering::Acquire))
            .filter(|seconds| *seconds > 0)
            .min()
    } // Fastest active subscriber, subject to the source floor.
    fn publish(&self, next: FleetView) {
        // Only the service calls publication.
        let replaced = {
            let mut guard = self
                .latest
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            std::mem::replace(&mut *guard, Arc::new(next))
        }; // Recover poison without dropping under the lock.
        drop(replaced); // Raw data remains in worker-owned custody.
    } // End block.
    fn view(&self) -> Arc<FleetView> {
        Arc::clone(
            &self
                .latest
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    } // Worker-side blocking read only.
} // End block.
struct Shared {
    sources: [SourceState; 3],
    stopped: AtomicBool,
} // Fixed source count; no request FIFO.
impl Default for Shared {
    fn default() -> Self {
        Self {
            sources: std::array::from_fn(|_| SourceState::default()),
            stopped: AtomicBool::new(false),
        }
    }
} // Constant allocation.
pub(super) struct FleetFeed {
    shared: Arc<Shared>,
    source: FleetSource,
    slot: usize,
} // Scene owns a subscription, not an HTTP worker.
impl FleetFeed {
    // Scene operations never scan fleets or touch the filesystem.
    pub fn start(
        resources: AmbientResources,
        source: FleetSource,
        seconds: u64,
    ) -> Result<Self, String> {
        // One process owner is admitted once and shared by all scene subscribers.
        static SERVICE: OnceLock<Mutex<Option<Service>>> = OnceLock::new();
        let mut service = SERVICE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if service.is_none() {
            *service = Some(Service::start(resources, disk_fetch)?);
        } else {
            ensure_quota_root(service.as_ref().expect("checked above"), &resources)?;
        } // Failed admission/spawn leaves the slot empty so the scene can retry.
        let shared = Arc::clone(&service.as_ref().expect("initialized above").shared);
        drop(service);
        Self::attach(shared, source, seconds) // Reopening immediately reuses the existing publication.
    } // End block.
    fn attach(shared: Arc<Shared>, source: FleetSource, seconds: u64) -> Result<Self, String> {
        // Also the deterministic test seam.
        if shared.stopped.load(Ordering::Acquire) {
            return Err("fleet cache owner stopped".into());
        } // Never attach to abandoned custody.
        let interval = seconds.clamp(source.floor(), 3600); // No persisted request can defeat the source floor.
        let slot = shared.sources[source.index()]
            .users
            .iter()
            .position(|slot| {
                slot.compare_exchange(0, interval, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            })
            .ok_or("fleet subscriber limit reached")?; // Bounded admission, no waiting.
        Ok(Self {
            shared,
            source,
            slot,
        }) // This slot is released exactly once.
    } // End block.
    pub fn set_interval(&self, seconds: u64) {
        self.shared.sources[self.source.index()].users[self.slot]
            .store(seconds.clamp(self.source.floor(), 3600), Ordering::Release);
    } // No worker restart or geometry invalidation.
    pub fn try_snapshot(&self) -> Result<Option<Arc<FleetView>>, String> {
        // UI polling is nonblocking.
        if self.shared.stopped.load(Ordering::Acquire) {
            return Err("fleet cache owner stopped".into());
        } // Keep scene's last-good frame with visible error.
        match self.shared.sources[self.source.index()].latest.try_lock() {
            // Only a short publication mutex is shared.
            Ok(view) => Ok(Some(Arc::clone(&view))), // Original receipt and data Arc are unchanged on cache hits.
            Err(std::sync::TryLockError::WouldBlock) => Ok(None), // Retry on a later frame.
            Err(std::sync::TryLockError::Poisoned(_)) => {
                Err("fleet cache publication poisoned".into())
            } // Never silently freeze.
        } // End block.
    } // End block.
} // End block.
impl Drop for FleetFeed {
    // No fleet-sized destruction or joining belongs here.
    fn drop(&mut self) {
        // The process service retains custody after the scene releases its copies.
        let source = &self.shared.sources[self.source.index()]; // Fixed identity of this subscription.
        source.users[self.slot].store(0, Ordering::Release); // Release the bounded subscriber slot.
        if source.interval().is_none() {
            source.cancel.store(true, Ordering::Release);
        } // Cancellation is sticky until the owner starts its next attempt.
    } // End block.
} // End block.
struct Custody<T> {
    slots: [Option<Arc<T>>; RETAINED_FLEETS],
} // No unbounded retirement queue.
impl<T> Default for Custody<T> {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
        }
    }
} // Fixed capacity.
impl<T> Custody<T> {
    // Called exclusively by the service worker, including shutdown cleanup.
    fn available(&self) -> bool {
        self.slots.iter().any(Option::is_none)
    } // Reserve capacity before fetching another fleet.
    fn retain(&mut self, data: &Arc<T>) -> Result<(), String> {
        // Must run before any UI publication.
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or("fleet retirement admission busy")?; // Backpressure, not eviction.
        *slot = Some(Arc::clone(data)); // This guard prevents a UI-side final release.
        Ok(()) // Every published vector has one guard until exclusive worker reclamation.
    } // End block.
    fn collect(&mut self) {
        // try_unwrap is the atomic exclusive-ownership test, not a racy strong_count observation.
        for slot in &mut self.slots {
            // At most six guards, independent of fleet size.
            let Some(data) = slot.take() else { continue }; // Empty slots cost constant work.
            match Arc::try_unwrap(data) {
                Ok(data) => drop(data),
                Err(data) => *slot = Some(data),
            } // Actual vector/string destruction occurs only here on the worker.
        } // End block.
    } // End block.
    fn empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    } // Cleanup cannot abandon guards while UI copies survive.
} // End block.
struct Service {
    shared: Arc<Shared>,
    worker: Option<Worker>,
    quota_root: QuotaGroup, // A process singleton cannot silently charge a different host ledger.
} // One owned process-wide worker and no per-source worker queue.
struct Fetched {
    batch: FleetBatch,
    received_ms: i64,
    refresh_soon: bool,
} // Disk bootstrap may publish stale data before trying the network.
type FetchResult = Result<Option<Fetched>, String>; // None means an unchanged fresh disk receipt, not a new receive event.
impl Service {
    // Injection exercises real ownership without any provider calls.
    fn start(
        resources: AmbientResources,
        mut fetch: impl FnMut(FleetSource, Option<i64>, &AtomicBool) -> FetchResult + Send + 'static,
    ) -> Result<Self, String> {
        let quota_root = resources.finite().quota_group();
        // Admit the process-wide serial owner before allocating worker state.
        let admission = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: 16 * 1024 * 1024,
            })
            .map_err(|reason| format!("fleet cache worker admission refused: {reason:?}"))?;
        let shared = Arc::new(Shared::default()); // No raw data can exist if spawning fails.
        let state = Arc::clone(&shared); // Worker retains the shared state through final cleanup.
        let worker = Worker::start_admitted("fleet-cache", admission, move |stop| {
            // Admission stays bounded even during scene churn.
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::Lowest,
            ); // Match existing source workers.
            let mut custody = Custody::<FleetBatch>::default(); // Sole owner of retirement/destruction rights.
            let mut attempted = [None::<Instant>; 3]; // Monotonic attempt clocks survive subscriber restarts.
            let mut failures = [0_u32; 3]; // Error backoff is separate for each source.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Unwinding must still drain custody safely.
                while !stop.load(Ordering::Acquire) {
                    // Process service sleeps when there are no subscribers.
                    custody.collect(); // Reclaim only exclusively worker-owned raw vectors.
                    for source in FleetSource::ALL {
                        // At most one in-flight fetch across this service.
                        let index = source.index(); // Fixed source slot.
                        let entry = &state.sources[index]; // Source-scoped cache and stop flag.
                        let Some(interval) = entry.interval() else {
                            continue;
                        }; // No background polling without demand.
                        let delay = retry_seconds(interval.max(source.floor()), failures[index]); // Bound failures without defeating the request floor.
                        if attempted[index]
                            .is_some_and(|time| time.elapsed() < Duration::from_secs(delay))
                        {
                            continue;
                        } // Scene restart cannot bypass monotonic admission.
                        let previous = entry.view(); // Receipt identity is obtained off the UI.
                        if !custody.available() {
                            let mut next = (*previous).clone();
                            next.state.failed(
                                "fleet retirement admission busy; last good retained".into(),
                            );
                            entry.publish(next);
                            continue;
                        } // Never evict a guard still protecting a scene.
                        entry.cancel.store(false, Ordering::Release); // Only the owner clears cancellation for a new attempt.
                        if entry.interval().is_none() || stop.load(Ordering::Acquire) {
                            continue;
                        } // Recheck demand after clearing the flag.
                        let storage = match resources
                            .reserve_storage(source.retained_storage_bytes())
                        {
                            Ok(storage) => storage,
                            Err(error) => {
                                attempted[index] = Some(Instant::now());
                                failures[index] = failures[index].saturating_add(1);
                                let mut next = (*previous).clone();
                                next.state
                                    .failed(format!("fleet storage admission refused: {error:?}"));
                                entry.publish(next);
                                continue;
                            }
                        }; // Bound retained provider data before body capture or decode.
                        let result = fetch(source, previous.state.received_ms, &entry.cancel); // All filesystem, HTTP and decoding stay here.
                        attempted[index] = Some(Instant::now()); // Cooldown follows completed I/O, including failed/cancelled attempts; this is not a receipt timestamp.
                        let mut next = (*previous).clone(); // Failure retains both data and its original receipt.
                        match result {
                            // No cached hit is stamped with the current clock.
                            Ok(Some(fetched)) if !entry.cancel.load(Ordering::Acquire) => {
                                // Do not publish cancelled work.
                                let mut batch = fetched.batch;
                                batch._storage = Some(storage); // Charge follows every consumer retaining the batch.
                                let batch = Arc::new(batch);
                                custody
                                    .retain(&batch)
                                    .expect("capacity checked by sole owner"); // Guard before exposure; no intervening publisher exists.
                                next.state
                                    .received(fetched.received_ms, batch.meta.latest_fix_ms); // Preserve genuine network receipt and known fix summary.
                                next.data = Some(batch); // One immutable geometry identity per admitted result.
                                next.generation = next
                                    .generation
                                    .checked_add(1)
                                    .expect("fleet generation exhausted"); // Never reuse an old generation.
                                failures[index] = 0; // A successfully decoded result resets backoff.
                                if fetched.refresh_soon {
                                    attempted[index] = None;
                                } // Stale disk bootstrap schedules a network check on the next service pass.
                            } // End block.
                            Ok(None) if !entry.cancel.load(Ordering::Acquire) => {
                                next.state.error = None;
                                failures[index] = 0;
                            } // Fresh unchanged disk cache, no new time or generation.
                            Ok(_) => {
                                next.state
                                    .failed("fleet request cancelled; last good retained".into());
                                failures[index] = failures[index].saturating_add(1);
                            } // Preserve cancelled-owner admission.
                            Err(error) => {
                                next.state.failed(error);
                                failures[index] = failures[index].saturating_add(1);
                            } // Malformed/oversize/network failures retain last good.
                        } // End block.
                        entry.publish(next); // Replaced references are destroyed off-lock on this worker.
                    } // End block.
                    std::thread::sleep(Duration::from_millis(25)); // Fixed idle wake, no demand FIFO.
                } // End block.
            })); // The same cleanup runs after a panic or an explicit stop.
            state.stopped.store(true, Ordering::Release); // Close subscriptions before removing publications.
            for entry in &state.sources {
                entry.publish(FleetView::default());
            } // Release cache-owned references on the worker.
            while !custody.empty() {
                custody.collect();
                std::thread::sleep(Duration::from_millis(25));
            } // Keep the owner alive until scene and marker copies are gone.
        })
        .map_err(|error| format!("could not start fleet cache: {error}"))?; // Failed startup leaves the process registry retryable.
        Ok(Self {
            shared,
            worker: Some(worker),
            quota_root,
        }) // The static service owns the actual join handle.
    } // End block.
} // End block.
impl Drop for Service {
    fn drop(&mut self) {
        for entry in &self.shared.sources {
            entry.cancel.store(true, Ordering::Release);
        }
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
} // Tests/explicit owners also avoid UI joins.
fn retry_seconds(interval: u64, failures: u32) -> u64 {
    interval
        .saturating_mul(1_u64 << failures.min(8))
        .min(900_u64.max(interval))
} // Finite backoff, never below interval.
fn now_ms() -> Result<i64, String> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis(),
    )
    .map_err(|e| e.to_string())
} // Clock failure is not epoch zero.
fn decode(source: FleetSource, bytes: &[u8], stop: &AtomicBool) -> Result<FleetBatch, String> {
    // Worker-only transformation of a complete bounded body.
    if stop.load(Ordering::Acquire) {
        return Err("fleet decode cancelled".into());
    } // Early cancellation.
    if source == FleetSource::OpenSeaFeed {
        // Keep source-specific schema and timestamp semantics.
        let fleet = openseafeed::decode(bytes, stop)?; // Streaming rows, no full-fleet JSON tree.
        return Ok(FleetBatch {
            positions: fleet.positions,
            meta: FleetMeta {
                generated_ms: Some(fleet.generated_ms),
                latest_ais_ms: fleet.latest_ais_update_ms,
                counts: fleet.counts,
                ..Default::default()
            },
            _storage: None,
        }); // Every fix remains unknown.
    } // End block.
    let decoded = if source == FleetSource::Digitraffic {
        parse::digitraffic(bytes)?
    } else {
        parse::opensky(bytes)?
    }; // Aircraft parser keeps airborne-only scope.
    if stop.load(Ordering::Acquire) {
        return Err("fleet decode cancelled".into());
    } // Do not publish an obsolete legacy parse.
    let meta = FleetMeta {
        latest_fix_ms: decoded.items.iter().filter_map(|p| p.observed_ms).max(),
        known_fixes: decoded
            .items
            .iter()
            .filter(|p| p.observed_ms.is_some())
            .count(),
        filtered: decoded.filtered,
        counts: openseafeed::FleetCounts {
            records: decoded.items.len()
                + decoded.rejected
                + decoded.unpositioned
                + decoded.filtered,
            malformed: decoded.rejected,
            unpositioned: decoded.unpositioned,
            ..Default::default()
        },
        ..Default::default()
    }; // Preserve every omission category.
    Ok(FleetBatch {
        positions: Arc::new(decoded.items),
        meta,
        _storage: None,
    }) // The service guards this Arc before publication.
} // End block.
struct RawReceipt {
    received_ms: i64,
    bytes: Vec<u8>,
} // Disk bytes carry the original body-completion time.
fn read_cache(path: &Path, limit: usize) -> Result<Option<RawReceipt>, String> {
    // Called only on the cache worker.
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or("invalid fleet cache path")?;
    let directory =
        secure_fs::NoFollowDirectory::open_root(parent).map_err(|error| error.to_string())?;
    let mut file = match directory.open_regular(name) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }; // Admit only regular leaves without blocking on special files.
    secure_fs::restrict_open_file_to_owner(&file).map_err(|e| e.to_string())?; // Preserve private cache permissions.
    let mut header = [0_u8; 16]; // Eight magic bytes followed by signed receipt milliseconds.
    match file.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.to_string()),
    } // Truncated regular data is unusable.
    if &header[..8] != b"ILFLEET1" {
        return Ok(None);
    } // Invalid regular data falls through to quota-admitted refresh.
    let received_ms = i64::from_le_bytes(
        header[8..]
            .try_into()
            .map_err(|_| "invalid fleet receipt")?,
    ); // Exact original receipt.
    if received_ms < 0 {
        return Ok(None);
    } // Invalid stored receipt is unusable.
    let mut bytes = Vec::new(); // A file length hint is not trusted for allocation.
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?; // Bound decoded disk bytes too.
    if bytes.len() > limit {
        return Ok(None);
    } // Oversize regular content is unusable; never publish truncated bytes.
    Ok(Some(RawReceipt { received_ms, bytes })) // JSON validation happens before adoption.
} // End block.
fn write_cache(path: &Path, receipt: &RawReceipt) -> Result<(), String> {
    // Worker-only atomic publication while holding the source file lock.
    static SERIAL: AtomicU64 = AtomicU64::new(0); // Avoid collisions between this process's successive writes.
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    )); // Owned temporary pathname.
    let mut created = false; // Never unlink a pre-existing file after create_new fails.
    let result = (|| -> std::io::Result<()> {
        // Preserve the old snapshot until rename succeeds.
        let mut file = secure_fs::private_open_options()
            .write(true)
            .create_new(true)
            .open(&temporary)?; // Private, exclusive creation.
        created = true; // Cleanup now owns this temporary file.
        file.write_all(b"ILFLEET1")?;
        file.write_all(&receipt.received_ms.to_le_bytes())?;
        file.write_all(&receipt.bytes)?; // Receipt and body are committed together.
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path) // Atomic replacement; no half-written receipt/body pairing.
    })(); // No raw or decoded data is exposed to presentation here.
    if result.is_err() && created {
        let _ = std::fs::remove_file(&temporary);
    } // Remove only this attempt's incomplete file.
    result.map_err(|e| e.to_string()) // Caller reports a cache failure without replacing its last good state.
} // End block.
fn disk_fetch(source: FleetSource, previous: Option<i64>, stop: &AtomicBool) -> FetchResult {
    // Shared local-client cache, not a fabricated server observation.
    disk_fetch_with(
        &crate::source::default_cache_dir().join("live-data"),
        source,
        previous,
        stop,
        now_ms()?,
        || {
            // Same root and production transport.
            let bytes =
                http_get_stoppable(source.url(), source.limit(), Duration::from_secs(20), stop)
                    .map_err(|e| e.to_string())?; // Finite, source-specific HTTP bounds.
            Ok(RawReceipt {
                received_ms: now_ms()?,
                bytes,
            }) // Body completion before decode/cache work.
        },
    ) // Tests substitute only this transport call and the admission clock.
} // End block.
fn disk_fetch_with(
    root: &Path,
    source: FleetSource,
    previous: Option<i64>,
    stop: &AtomicBool,
    now: i64,
    download: impl FnOnce() -> Result<RawReceipt, String>,
) -> FetchResult {
    // Deterministic filesystem/admission test seam.
    if stop.load(Ordering::Acquire) {
        return Err("fleet request cancelled".into());
    } // No I/O for cancelled work.
    secure_fs::create_private_directory(root).map_err(|e| e.to_string())?; // Worker creates the cache directory.
    let path = root.join(format!("{}.snapshot", source.name())); // Source identity is part of the cache key.
    if let Some(raw) = read_cache(&path, source.limit())? {
        // Atomic reader may bootstrap while another process refreshes.
        let age = now.saturating_sub(raw.received_ms); // Receipt age is not coordinate age.
        let fresh = age >= 0 && age < source.floor() as i64 * 1000; // Future clocks do not extend a cache's TTL.
        if previous.is_none() || fresh {
            // Stale bootstrap is visibly old and schedules refresh next pass.
            // Reuse the in-process data Arc and receipt.
            match decode(source, &raw.bytes, stop) {
                Ok(batch) => {
                    if previous == Some(raw.received_ms) {
                        return Ok(None);
                    }
                    return Ok(Some(Fetched {
                        batch,
                        received_ms: raw.received_ms,
                        refresh_soon: !fresh,
                    }));
                }
                Err(_) if stop.load(Ordering::Acquire) => {
                    return Err("fleet request cancelled".into())
                }
                Err(_) => {} // Unusable regular cache content falls through to locked, quota-admitted refresh.
            } // No wall-clock restamping.
        } // End block.
    } // End block.
    let _lock =
        ExclusiveFileLock::try_acquire(&root.join(format!("{}.snapshot.lock", source.name())))
            .map_err(|e| e.to_string())?
            .ok_or("another local client is refreshing this fleet")?; // One cross-process fetch/write owner.
    if let Some(raw) = read_cache(&path, source.limit())? {
        // Recheck after acquisition; another process may have just committed.
        let age = now.saturating_sub(raw.received_ms); // Same original receipt semantics.
        if age >= 0 && age < source.floor() as i64 * 1000 {
            // Fresh shared result needs no HTTP request.
            // Preserve the exact in-process Arc.
            match decode(source, &raw.bytes, stop) {
                Ok(batch) => {
                    if previous == Some(raw.received_ms) {
                        return Ok(None);
                    }
                    return Ok(Some(Fetched {
                        batch,
                        received_ms: raw.received_ms,
                        refresh_soon: false,
                    }));
                }
                Err(_) if stop.load(Ordering::Acquire) => {
                    return Err("fleet request cancelled".into())
                }
                Err(_) => {} // Recheck under lock; corrupt content never defeats reservation admission.
            } // Reuse the other process's network receipt.
        } // End block.
    } // End block.
    rate::reserve(
        &root.join(format!("{}-request.time", source.name())),
        now,
        source.floor() as i64 * 1000,
    )?; // Failed/cancelled requests consume shared admission.
    let raw = download()?; // Production captures complete-body receipt; tests never call a provider.
    if raw.received_ms < 0 || raw.bytes.len() > source.limit() {
        return Err("invalid or oversized fleet receipt".into());
    } // Validate injected and production transport boundaries equally.
    let batch = decode(source, &raw.bytes, stop)?; // Malformed and oversize responses cannot overwrite disk or memory.
    if stop.load(Ordering::Acquire) {
        return Err("fleet request cancelled".into());
    } // Do not cache a cancelled result.
    write_cache(&path, &raw)?; // Atomic source-specific raw receipt, not a JSON replay timestamp.
    Ok(Some(Fetched {
        batch,
        received_ms: raw.received_ms,
        refresh_soon: false,
    })) // The service adds a custody guard before presentation can see the vector.
} // End block.
#[cfg(test)] // Synthetic fixtures only; no HTTP or private runtime data.
mod tests {
    // Exercise production cache/admission/custody methods directly.
    use super::*; // Private seams keep fixtures away from actual providers.
    use std::sync::mpsc; // Every inter-thread wait has a timeout.
    fn body() -> Vec<u8> {
        br#"{"generated_at":10,"count":1,"vessels":[{"mmsi":234567890,"ts":9,"lat":0,"lon":0,"hdg":90}]}"#.to_vec()
    } // Synthetic complete fleet.
    fn wait_view(feed: &FleetFeed) -> Arc<FleetView> {
        // Test-only bounded polling.
        let deadline = Instant::now() + Duration::from_secs(5); // No indefinite worker-read fixture.
        loop {
            // No lock is held while yielding to the real service worker.
            if let Some(view) = feed
                .try_snapshot()
                .unwrap()
                .filter(|view| view.data.is_some())
            {
                return view;
            } // Return actual publication.
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1)); // Broad liveness bound, not performance evidence.
        } // End block.
    } // End block.
    #[test]
    fn cache_service_admission_refusal_is_retryable() {
        let (_execution, resources) = crate::resources::isolated_test_resources();
        let held = resources
            .reserve_worker(WorkerCost {
                threads: 16,
                resident_bytes: 1,
            })
            .unwrap();
        let refused = Service::start(resources.clone(), |_source, _, _| Ok(None));
        assert!(refused.is_err());
        drop(held);

        let service = Service::start(resources, |_source, _, _| Ok(None))
            .expect("service starts after capacity returns");
        drop(service);
    }

    #[test]
    fn fleet_storage_refusal_is_reported_before_provider_fetch() {
        let (_execution, resources) = crate::resources::isolated_test_resources();
        let calls = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&calls);
        let service = Service::start(resources.clone(), move |_source, _, _| {
            counter.fetch_add(1, Ordering::AcqRel);
            Ok(None)
        })
        .unwrap();
        let quota = resources.finite().quota_group();
        let snapshot = quota.snapshot();
        let remaining = snapshot.limits.worker_bytes - snapshot.worker_bytes;
        let pressure = resources
            .reserve_storage(remaining.saturating_sub(1))
            .expect("leave less than one fleet reservation available");
        let feed =
            FleetFeed::attach(Arc::clone(&service.shared), FleetSource::OpenSeaFeed, 60).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let view = feed.try_snapshot().unwrap().unwrap();
            if let Some(error) = &view.state.error {
                assert!(error.contains("fleet storage admission refused"));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "storage refusal was not published"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(calls.load(Ordering::Acquire), 0);
        drop(pressure);
        drop(feed);
        drop(service);
    }

    #[test] // Reopening and a second consumer cannot fabricate another receive event.
    fn subscribers_share_arc_receipt_and_request_floor_across_restart() {
        // Real source worker, injected finite transport only.
        let calls = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&calls); // Count actual transport invocations.
        let service = Service::start(
            crate::resources::test_resources(),
            move |source, _, stop| {
                counter.fetch_add(1, Ordering::AcqRel);
                Ok(Some(Fetched {
                    batch: decode(source, &body(), stop)?,
                    received_ms: 77,
                    refresh_soon: false,
                }))
            },
        )
        .unwrap(); // No filesystem/network dependencies.
        let first =
            FleetFeed::attach(Arc::clone(&service.shared), FleetSource::OpenSeaFeed, 60).unwrap(); // First source demand.
        let initial = wait_view(&first); // Wait for an actual worker publication.
        let second =
            FleetFeed::attach(Arc::clone(&service.shared), FleetSource::OpenSeaFeed, 60).unwrap(); // Same source, separate consumer.
        let shared = wait_view(&second); // Must be the same cached data.
        assert!(Arc::ptr_eq(
            initial.data.as_ref().unwrap(),
            shared.data.as_ref().unwrap()
        )); // No fleet clone/redecode.
        drop(first);
        drop(second); // Simulate scene closure without ending the shared owner.
        let reopened =
            FleetFeed::attach(Arc::clone(&service.shared), FleetSource::OpenSeaFeed, 60).unwrap(); // Restart inside the floor.
        let again = wait_view(&reopened); // Immediate cached publication.
        assert_eq!(again.state.received_ms, Some(77));
        assert_eq!(calls.load(Ordering::Acquire), 1); // Original receipt, one request.
        assert!(Arc::ptr_eq(
            initial.data.as_ref().unwrap(),
            again.data.as_ref().unwrap()
        )); // Data generation is reused too.
        assert_eq!(again.data.as_ref().unwrap().positions[0].observed_ms, None); // No timestamp promotion.
        drop(reopened);
        drop(initial);
        drop(shared);
        drop(again);
        drop(service); // Actual worker owns final vector reclamation.
    } // End block.
    #[test] // Bounded subscriptions and contended reads never wait on a worker mutex.
    fn subscription_capacity_and_try_read_are_explicit() {
        // A worker is unnecessary to hold the publication lock.
        let shared = Arc::new(Shared::default()); // Fixed source slots.
        let mut feeds = (0..SUBSCRIBERS)
            .map(|_| FleetFeed::attach(Arc::clone(&shared), FleetSource::OpenSeaFeed, 1).unwrap())
            .collect::<Vec<_>>(); // Faster input clamps to the floor.
        assert!(FleetFeed::attach(Arc::clone(&shared), FleetSource::OpenSeaFeed, 60).is_err()); // No unbounded subscriber allocation.
        assert_eq!(shared.sources[0].interval(), Some(60)); // The caller cannot defeat cadence.
        let guard = shared.sources[0].latest.lock().unwrap(); // Force the actual WouldBlock path.
        assert!(feeds[0].try_snapshot().unwrap().is_none());
        drop(guard); // Read returns rather than waiting.
        drop(feeds.pop());
        assert!(FleetFeed::attach(Arc::clone(&shared), FleetSource::OpenSeaFeed, 60).is_ok());
        // Admission recovers after release.
    } // End block.
    #[test] // Separate-process disk readers preserve the first fetch's receipt and never cross sources.
    fn disk_cache_reuses_receipt_and_rejects_bad_refresh_without_overwrite() {
        // All downloads are explicit fixtures.
        let root = tempfile::tempdir().unwrap();
        let stop = AtomicBool::new(false); // Isolated public-data cache.
        let first = disk_fetch_with(
            root.path(),
            FleetSource::OpenSeaFeed,
            None,
            &stop,
            1_000_000,
            || {
                Ok(RawReceipt {
                    received_ms: 1_000_000,
                    bytes: body(),
                })
            },
        )
        .unwrap()
        .unwrap(); // First mock transport is persisted atomically.
        let other = disk_fetch_with(
            root.path(),
            FleetSource::OpenSeaFeed,
            None,
            &stop,
            1_000_001,
            || panic!("cached read attempted HTTP"),
        )
        .unwrap()
        .unwrap(); // Independent process-like reader.
        assert_eq!(
            (first.received_ms, other.received_ms),
            (1_000_000, 1_000_000)
        ); // Disk receipt is never restamped.
        assert!(disk_fetch_with(
            root.path(),
            FleetSource::OpenSeaFeed,
            Some(1_000_000),
            &stop,
            1_000_002,
            || panic!("same receipt attempted HTTP")
        )
        .unwrap()
        .is_none()); // In-process Arc may remain unchanged.
        assert!(disk_fetch_with(
            root.path(),
            FleetSource::Digitraffic,
            None,
            &stop,
            1_000_002,
            || Err("regional offline".into())
        )
        .is_err()); // No fallback to the broader source's cache.
        let invalid = disk_fetch_with(
            root.path(),
            FleetSource::OpenSeaFeed,
            Some(1_000_000),
            &stop,
            1_060_000,
            || {
                Ok(RawReceipt {
                    received_ms: 1_060_000,
                    bytes: b"{}".to_vec(),
                })
            },
        ); // Malformed next snapshot.
        assert!(invalid.is_err());
        let retained = read_cache(
            &root.path().join("openseafeed.snapshot"),
            openseafeed::MAX_RESPONSE_BYTES,
        )
        .unwrap()
        .unwrap(); // The prior complete disk record survives.
        assert_eq!((retained.received_ms, retained.bytes), (1_000_000, body())); // No false successful empty overwrite.
        assert!(disk_fetch_with(
            root.path(),
            FleetSource::OpenSeaFeed,
            Some(1_000_000),
            &stop,
            1_060_001,
            || panic!("failed attempt lost reservation")
        )
        .is_err()); // Failed attempts also hold the cross-process floor.
        assert_eq!(retry_seconds(60, 20), 900);
        assert!(retry_seconds(900, 1) >= 900); // Backoff stays bounded without lowering source floors.
    } // End block.
    struct DropProbe(mpsc::Sender<std::thread::ThreadId>); // Observe the actual destructor's thread identity.
    impl Drop for DropProbe {
        fn drop(&mut self) {
            let _ = self.0.send(std::thread::current().id());
        }
    } // Instrument the generic production custody algorithm.
    #[test] // Last UI release cannot destroy protected contents; exclusive unwrapping remains worker-owned.
    fn final_destruction_and_retirement_backpressure_are_worker_owned() {
        // No timing assumption about allocator speed.
        let (sent, leases) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let (dropped, drops) = mpsc::channel();
        let (checked, checks) = mpsc::channel(); // Bounded waits below.
        let worker = std::thread::Builder::new()
            .name("custody-proof".into())
            .spawn(move || {
                // Explicit disposal owner.
                let mut custody = Custody::default();
                let mut leased = Vec::new(); // Same fixed-capacity primitive used for Vec<Position>.
                for _ in 0..RETAINED_FLEETS {
                    let value = Arc::new(DropProbe(dropped.clone()));
                    custody.retain(&value).unwrap();
                    leased.push(value);
                } // Fill every admission slot.
                assert!(!custody.available());
                sent.send(leased).unwrap(); // Hand only guarded references to the simulated UI.
                custody.collect();
                assert!(!custody.empty());
                checked.send(()).unwrap(); // Prove the guard survives before the UI releases its copies.
                gate.recv_timeout(Duration::from_secs(5)).unwrap(); // UI explicitly signals after dropping all leases.
                custody.collect();
                assert!(custody.empty()); // Every actual destructor runs in this call on this worker.
            })
            .unwrap(); // No production thread/process changes.
        let owner_id = worker.thread().id();
        let leased = leases.recv_timeout(Duration::from_secs(5)).unwrap(); // Receive already-guarded data.
        checks.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(leased);
        assert!(drops.try_recv().is_err()); // Dropping the last UI copies cannot free the contents.
        release.send(()).unwrap(); // Allow worker-side exclusive reclamation.
        for _ in 0..RETAINED_FLEETS {
            assert_eq!(
                drops.recv_timeout(Duration::from_secs(5)).unwrap(),
                owner_id
            );
        } // Instrumented owner proof, not a strong-count guess.
        worker.join().unwrap(); // All destructor receipts arrived before joining.
    } // End block.
} // End block.

#[cfg(test)]
mod repair_regression {
    use super::*;
    fn body() -> Vec<u8> {
        br#"{"generated_at":10,"count":1,"vessels":[{"mmsi":234567890,"ts":9,"lat":0,"lon":0}]}"#
            .to_vec()
    }
    #[test]
    fn malformed_cache_refresh_is_locked_and_quota_admitted() {
        for corrupt in [
            b"partial".to_vec(),
            b"BADMAGIC00000000".to_vec(),
            b"ILFLEET1"
                .iter()
                .copied()
                .chain((-1_i64).to_le_bytes())
                .collect(),
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("openseafeed.snapshot");
            std::fs::write(&path, &corrupt).unwrap();
            let stop = AtomicBool::new(false);
            let result = disk_fetch_with(
                root.path(),
                FleetSource::OpenSeaFeed,
                None,
                &stop,
                1_000_000,
                || {
                    Ok(RawReceipt {
                        received_ms: 1_000_000,
                        bytes: body(),
                    })
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(result.received_ms, 1_000_000);
            assert_eq!(read_cache(&path, 1000).unwrap().unwrap().bytes, body());
            std::fs::write(&path, &corrupt).unwrap();
            assert!(disk_fetch_with(
                root.path(),
                FleetSource::OpenSeaFeed,
                Some(1_000_000),
                &stop,
                1_000_001,
                || panic!("floor bypass")
            )
            .is_err());
            let ledger = root.path().join("openseafeed-request.time");
            std::fs::write(&ledger, b"corrupt").unwrap();
            assert!(disk_fetch_with(
                root.path(),
                FleetSource::OpenSeaFeed,
                None,
                &stop,
                1_060_000,
                || panic!("corrupt ledger bypass")
            )
            .is_err());
            assert_eq!(std::fs::read(ledger).unwrap(), b"corrupt");
        }
    }
    #[test]
    fn malformed_json_cold_warm_recovery_and_lock_contention() {
        for previous in [None, Some(1_000_000)] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("openseafeed.snapshot");
            write_cache(
                &path,
                &RawReceipt {
                    received_ms: 1_000_000,
                    bytes: b"{}".to_vec(),
                },
            )
            .unwrap();
            let stop = AtomicBool::new(false);
            let held =
                ExclusiveFileLock::acquire(&root.path().join("openseafeed.snapshot.lock")).unwrap();
            assert!(disk_fetch_with(
                root.path(),
                FleetSource::OpenSeaFeed,
                previous,
                &stop,
                1_000_001,
                || panic!("lock bypass")
            )
            .is_err());
            drop(held);
            let result = disk_fetch_with(
                root.path(),
                FleetSource::OpenSeaFeed,
                previous,
                &stop,
                1_000_001,
                || {
                    Ok(RawReceipt {
                        received_ms: 1_000_001,
                        bytes: body(),
                    })
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(result.received_ms, 1_000_001);
            assert_eq!(result.batch.positions.len(), 1);
        }
    }
    #[test]
    fn cancelled_recovery_keeps_bad_cache_and_consumed_quota() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("openseafeed.snapshot");
        std::fs::write(&path, b"partial").unwrap();
        let stop = AtomicBool::new(false);
        assert!(disk_fetch_with(
            root.path(),
            FleetSource::OpenSeaFeed,
            None,
            &stop,
            1_000_000,
            || {
                stop.store(true, Ordering::Release);
                Ok(RawReceipt {
                    received_ms: 1_000_000,
                    bytes: body(),
                })
            }
        )
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"partial");
        assert_eq!(
            std::fs::read(root.path().join("openseafeed-request.time")).unwrap(),
            b"1000000"
        );
        stop.store(false, Ordering::Release);
        assert!(disk_fetch_with(
            root.path(),
            FleetSource::OpenSeaFeed,
            None,
            &stop,
            1_000_001,
            || panic!("cancelled quota reset")
        )
        .is_err());
        stop.store(true, Ordering::Release);
        assert!(disk_fetch_with(
            root.path(),
            FleetSource::OpenSeaFeed,
            None,
            &stop,
            1_060_000,
            || panic!("stopped recovery")
        )
        .is_err());
    }
    #[test]
    fn directory_and_oversize_admission() {
        let root = tempfile::tempdir().unwrap();
        let nonregular = root.path().join("directory.snapshot");
        std::fs::create_dir(&nonregular).unwrap();
        assert!(read_cache(&nonregular, 1000).is_err());
        let ledger = root.path().join("ledger.time");
        std::fs::create_dir(&ledger).unwrap();
        assert!(rate::reserve(&ledger, 1_000_000, 60_000).is_err());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("openseafeed.snapshot");
        write_cache(
            &path,
            &RawReceipt {
                received_ms: 1,
                bytes: vec![b'x'; 33],
            },
        )
        .unwrap();
        assert!(read_cache(&path, 32).unwrap().is_none());
        assert_eq!(std::fs::metadata(path).unwrap().len(), 49);
    }
}
fn ensure_quota_root(service: &Service, resources: &AmbientResources) -> Result<(), String> {
    let requested = resources.finite().quota_group();
    if !service.quota_root.shares_root(&requested) {
        return Err("fleet cache belongs to a different execution quota".into());
    }
    Ok(())
} // Multiple scenes must debit the same host-owned process ledger.
