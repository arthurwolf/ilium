//! Demand-driven fleet geometry. Presentation never waits for preparation.
use super::{fleet_cache::FleetBatch, map, maps::MapKind, model::Position};
use crate::{raster::Raster, source::Worker};
use std::{
    io,
    sync::{
        atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering}, // Preserve the typed operation.
        mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
        Arc,
        Mutex,
        TryLockError,
    },
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MarkerKey {
    pub request_generation: u64,
    pub data_generation: u64,
    pub kind: MapKind,
    pub dot_width: usize,
    pub dot_height: usize,
    pub cell_width: usize,
    pub cell_height: usize,
    pub marker_brightness: u8,
    pub show_heading: bool,
}

#[derive(Debug, Clone)]
pub(super) struct MarkerRequest {
    pub key: MarkerKey,
    pub positions: Arc<Vec<Position>>,
    pub _owner: Option<Arc<FleetBatch>>, // Keeps storage admission alive through replacement/retirement.
}

#[derive(Debug)]
pub(super) struct PreparedMarkers {
    pub key: MarkerKey,
    pub raster: Raster,
    pub occupied_centers: Vec<bool>,
    pub accepted_positions: usize,
    pub unique_center_cells: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SubmitState {
    Accepted,
    Busy,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MarkerError {
    Busy,
    Poisoned,
    Disconnected,
    InvalidViewport,
    PreparationFailed,
}
impl std::fmt::Display for MarkerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Busy => "marker worker state busy",
            Self::Poisoned => "marker worker state poisoned",
            Self::Disconnected => "marker worker disconnected",
            Self::InvalidViewport => "invalid marker viewport",
            Self::PreparationFailed => "marker preparation failed",
        })
    }
}
impl std::error::Error for MarkerError {}

#[derive(Default)]
struct Pending {
    request: Option<MarkerRequest>,
    // One disposal slot prevents final fleet deallocation on submission.
    // If occupied, admission returns Busy; caller retains the latest request.
    retired: Option<MarkerRequest>,
}
#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    latest: Mutex<Option<Arc<PreparedMarkers>>>,
    desired: AtomicU64,
    fault: AtomicU8,
}

const MAX_MARKER_WORKERS: usize = 4; // Bound live and stopping preparers across scene churn.
static MARKER_WORKERS: AtomicUsize = AtomicUsize::new(0); // Includes workers awaiting cancellation cleanup.
struct MarkerPermit; // One slot, owned by one actual worker closure.
impl MarkerPermit {
    // No UI-side mutex or sleep.
    fn acquire() -> io::Result<Self> {
        // Atomic bounded admission.
        MARKER_WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_MARKER_WORKERS).then_some(count + 1)
            })
            .map_err(|_| {
                io::Error::new(io::ErrorKind::WouldBlock, "marker worker admission busy")
            })?; // Busy is retryable.
        Ok(Self) // No thread exists unless this permit was acquired.
    } // End block.
} // End block.
impl Drop for MarkerPermit {
    fn drop(&mut self) {
        MARKER_WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
} // Actual exit or failed spawn releases the slot.

pub(super) struct MarkerWorker {
    worker: Option<Worker>,
    shared: Arc<Shared>,
    wake: SyncSender<()>,
}

impl MarkerWorker {
    pub fn try_start() -> io::Result<Self> {
        // Production admission never waits.
        Self::start_admitted(prepare_positions, MarkerPermit::acquire()?) // Hold the permit through actual worker exit.
    }
    fn start_admitted(
        // Preserve the typed operation.
        prepare: impl FnMut(&MarkerRequest, &dyn Fn() -> bool) -> Option<PreparedMarkers>
            + Send
            + 'static, // Existing test injection.
        permit: MarkerPermit, // Admission belongs to the closure, not the presentation owner.
    ) -> io::Result<Self> {
        // Spawn failure drops the captured permit.
        let shared = Arc::new(Shared::default());
        let state = Arc::clone(&shared);
        let (wake, receiver) = mpsc::sync_channel(1);
        let worker = Worker::try_spawn("map-markers", move |stop| {
            let _permit = permit; // Release admission only after all worker-side cleanup finishes.
                                  // Local drop order keeps admission through captured-state cleanup, including unwinding.
            let mut prepare = prepare;
            let state = state;
            let receiver = receiver;
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::Lowest,
            );
            while !stop.load(Ordering::Relaxed) {
                match receiver.recv_timeout(Duration::from_millis(25)) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                while receiver.try_recv().is_ok() {}
                let (request, retired) = match state.pending.lock() {
                    Ok(mut pending) => (pending.request.take(), pending.retired.take()),
                    Err(_) => {
                        state.fault.store(1, Ordering::Release);
                        break;
                    }
                };
                drop(retired);
                let Some(request) = request else { continue };
                let cancelled = || {
                    stop.load(Ordering::Relaxed)
                        || state.desired.load(Ordering::Acquire) != request.key.request_generation
                };
                if cancelled() {
                    continue;
                }
                let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    prepare(&request, &cancelled)
                }));
                let Ok(prepared) = prepared else {
                    state.fault.store(3, Ordering::Release);
                    break;
                };
                let Some(prepared) = prepared else {
                    if !cancelled() {
                        state.fault.store(3, Ordering::Release);
                    }
                    continue;
                };
                if cancelled() {
                    continue;
                }
                let prepared = Arc::new(prepared);
                let replaced = match state.latest.lock() {
                    Ok(mut latest) => {
                        if cancelled() {
                            None
                        } else {
                            latest.replace(Arc::clone(&prepared))
                        }
                    }
                    Err(_) => {
                        state.fault.store(1, Ordering::Release);
                        break;
                    }
                };
                drop(replaced);
            }
            // Close admission before final cleanup; submission rechecks under
            // the pending lock so no request can arrive after this drain.
            state
                .fault
                .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
                .ok();
            // Cleanup remains worker-owned even if the presentation owner drops.
            let cleanup = match state.pending.lock() {
                Ok(mut pending) => (pending.request.take(), pending.retired.take()),
                Err(poisoned) => {
                    let mut pending = poisoned.into_inner();
                    (pending.request.take(), pending.retired.take())
                }
            };
            drop(cleanup);
        })?;
        Ok(Self {
            worker: Some(worker),
            shared,
            wake,
        })
    }

    /// Monotonic generations are supplied by the scene, including A -> B -> A.
    pub fn invalidate(&self, request_generation: u64) {
        self.shared
            .desired
            .fetch_max(request_generation, Ordering::AcqRel);
    }

    fn fault(&self) -> Result<(), MarkerError> {
        match self.shared.fault.load(Ordering::Acquire) {
            0 => Ok(()),
            1 => Err(MarkerError::Poisoned),
            2 => Err(MarkerError::Disconnected),
            _ => Err(MarkerError::PreparationFailed),
        }
    }

    /// Borrowing keeps retry/failed admission input owned by the caller.
    pub fn try_submit_latest(&self, request: &MarkerRequest) -> Result<SubmitState, MarkerError> {
        self.invalidate(request.key.request_generation);
        self.fault()?;
        if !valid_key(request.key) {
            return Err(MarkerError::InvalidViewport);
        }
        if self.shared.desired.load(Ordering::Acquire) != request.key.request_generation {
            return Ok(SubmitState::Stale);
        }
        {
            let mut pending = match self.shared.pending.try_lock() {
                Ok(pending) => pending,
                Err(TryLockError::WouldBlock) => return Ok(SubmitState::Busy),
                Err(TryLockError::Poisoned(_)) => return Err(MarkerError::Poisoned),
            };
            self.fault()?;
            if self.shared.desired.load(Ordering::Acquire) != request.key.request_generation {
                return Ok(SubmitState::Stale);
            }
            if pending.retired.is_some() {
                return Ok(SubmitState::Busy);
            }
            pending.retired = pending.request.replace(request.clone());
        }
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => Ok(SubmitState::Accepted),
            Err(TrySendError::Disconnected(())) => Err(MarkerError::Disconnected),
        }
    }

    /// Full-key and desired-generation checks make late results harmless.
    pub fn try_latest(&self, key: MarkerKey) -> Result<Option<Arc<PreparedMarkers>>, MarkerError> {
        self.fault()?;
        let latest = match self.shared.latest.try_lock() {
            Ok(latest) => latest,
            Err(TryLockError::WouldBlock) => return Err(MarkerError::Busy),
            Err(TryLockError::Poisoned(_)) => return Err(MarkerError::Poisoned),
        };
        Ok(latest
            .as_ref()
            .filter(|prepared| {
                prepared.key == key
                    && self.shared.desired.load(Ordering::Acquire) == key.request_generation
            })
            .map(Arc::clone))
    }
}
impl Drop for MarkerWorker {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}

fn valid_key(key: MarkerKey) -> bool {
    key.marker_brightness <= 100
        && key.kind != MapKind::Earthquakes
        && u16::try_from(key.cell_width).is_ok()
        && u16::try_from(key.cell_height).is_ok()
        && key
            .cell_width
            .checked_mul(key.cell_height)
            .is_some_and(|n| n <= 4_194_304)
        && key
            .dot_width
            .checked_mul(key.dot_height)
            .is_some_and(|n| n <= 16_777_216)
}

pub(super) fn prepare_positions(
    request: &MarkerRequest,
    cancelled: &dyn Fn() -> bool,
) -> Option<PreparedMarkers> {
    if !valid_key(request.key) || cancelled() {
        return None;
    }
    let key = request.key;
    let mut raster = Raster::default();
    raster
        .dots
        .try_reserve_exact(key.dot_width * key.dot_height)
        .ok()?;
    raster.resize(key.dot_width, key.dot_height);
    let mut occupied_centers = Vec::new();
    occupied_centers
        .try_reserve_exact(key.cell_width * key.cell_height)
        .ok()?;
    occupied_centers.resize(key.cell_width * key.cell_height, false);
    let mut prepared = PreparedMarkers {
        key,
        raster,
        occupied_centers,
        accepted_positions: 0,
        unique_center_cells: 0,
    };
    if key.cell_width == 0 || key.cell_height == 0 || key.dot_width == 0 || key.dot_height == 0 {
        return (!cancelled()).then_some(prepared);
    }
    for (index, position) in request.positions.iter().enumerate() {
        if index % 128 == 0 && cancelled() {
            return None;
        }
        let Some((x, y, cell)) = marker_center(
            position,
            key.cell_width as u16,
            key.cell_height as u16,
            key.dot_width,
            key.dot_height,
        ) else {
            continue;
        };
        prepared.accepted_positions += 1;
        if !prepared.occupied_centers[cell] {
            prepared.occupied_centers[cell] = true;
            prepared.unique_center_cells += 1;
        }
        let intensity = f32::from(key.marker_brightness) / 100.0;
        if key.show_heading {
            draw_vehicle(
                &mut prepared.raster,
                (x, y),
                position.heading_degrees,
                intensity,
            );
        } else {
            // Exactly one raster dot per vehicle, never a soft multi-dot blob.
            let column = ((x * key.dot_width as f32) as usize).min(key.dot_width - 1);
            let row = ((y * key.dot_height as f32) as usize).min(key.dot_height - 1);
            prepared.raster.dots[row * key.dot_width + column] = intensity;
        }
    }
    (!cancelled()).then_some(prepared)
}

pub(super) fn marker_center(
    position: &Position,
    width: u16,
    height: u16,
    dot_width: usize,
    dot_height: usize,
) -> Option<(f32, f32, usize)> {
    let (x, y) = map::project(position.longitude, position.latitude)?;
    // Clamp only to visible dot centers: poles and the dateline remain visible,
    // while the geographic projection itself retains exact boundaries.
    let x = x.clamp(0.5 / dot_width as f32, 1.0 - 0.5 / dot_width as f32);
    let y = y.clamp(0.5 / dot_height as f32, 1.0 - 0.5 / dot_height as f32);
    let column = ((x * f32::from(width)) as usize).min(usize::from(width) - 1);
    let row = ((y * f32::from(height)) as usize).min(usize::from(height) - 1);
    Some((x, y, row * usize::from(width) + column))
}

pub(super) fn draw_vehicle(
    raster: &mut Raster,
    center: (f32, f32),
    heading: Option<f64>,
    intensity: f32,
) {
    raster.line(center, center, 0.8, intensity);
    let Some(heading) = heading.filter(|value| value.is_finite() && (0.0..360.0).contains(value))
    else {
        return;
    };
    let angle = (heading as f32).to_radians();
    let forward = (angle.sin(), -angle.cos());
    let side = (angle.cos(), angle.sin());
    let point = |along: f32, across: f32| {
        (
            center.0 + (forward.0 * along + side.0 * across) / raster.width as f32,
            center.1 + (forward.1 * along + side.1 * across) / raster.height as f32,
        )
    };
    let tip = point(2.5, 0.0);
    let left = point(-1.0, -1.4);
    let right = point(-1.0, 1.4);
    raster.line(tip, left, 0.45, intensity);
    raster.line(left, right, 0.45, intensity);
    raster.line(right, tip, 0.45, intensity);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn fixture_worker(
        prepare: impl FnMut(&MarkerRequest, &dyn Fn() -> bool) -> Option<PreparedMarkers>
            + Send
            + 'static,
    ) -> MarkerWorker {
        // Tests may wait for shared admission; UI code cannot.
        let deadline = Instant::now() + Duration::from_secs(10); // Bound parallel-test contention.
        let permit = loop {
            // Do not consume the closure before admission succeeds.
            match MarkerPermit::acquire() {
                Ok(permit) => break permit,
                Err(error) => {
                    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
            } // No unbounded fixture wait.
        }; // End block.
        MarkerWorker::start_admitted(prepare, permit).unwrap() // Exercise the same admitted worker constructor.
    } // End block.

    fn request(generation: u64) -> MarkerRequest {
        MarkerRequest {
            key: MarkerKey {
                request_generation: generation,
                data_generation: generation,
                kind: MapKind::Boats,
                dot_width: 160,
                dot_height: 96,
                cell_width: 80,
                cell_height: 24,
                marker_brightness: 90,
                show_heading: true,
            },
            positions: Arc::new(vec![
                Position::new("fixture".into(), 0.0, 0.0, Some(0)).unwrap()
            ]), // Preserve the typed operation.
            _owner: None,
        }
    }

    #[test]
    fn pending_b_is_replaced_by_c_while_a_is_in_flight() {
        let (started, starts) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let worker = fixture_worker(move |request, cancelled| {
            // Preserve the typed operation.
            started.send(request.key.request_generation).unwrap();
            if request.key.request_generation == 1 {
                gate.recv().unwrap();
            }
            prepare_positions(request, cancelled)
        }); // End block.
        assert_eq!(
            worker.try_submit_latest(&request(1)).unwrap(),
            SubmitState::Accepted
        );
        assert_eq!(starts.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
        assert_eq!(
            worker.try_submit_latest(&request(2)).unwrap(),
            SubmitState::Accepted
        );
        assert_eq!(
            worker.try_submit_latest(&request(3)).unwrap(),
            SubmitState::Accepted
        );
        release.send(()).unwrap();
        assert_eq!(starts.recv_timeout(Duration::from_secs(1)).unwrap(), 3);
        assert!(worker.try_latest(request(1).key).unwrap().is_none());
    }

    #[test]
    fn held_ui_locks_are_busy_and_do_not_wait() {
        let worker = fixture_worker(prepare_positions); // Preserve the typed operation.
        let pending = worker.shared.pending.lock().unwrap();
        let start = Instant::now();
        assert_eq!(
            worker.try_submit_latest(&request(1)).unwrap(),
            SubmitState::Busy
        );
        assert!(start.elapsed() < Duration::from_millis(100));
        drop(pending);
        let latest = worker.shared.latest.lock().unwrap();
        assert_eq!(
            worker.try_latest(request(1).key).unwrap_err(),
            MarkerError::Busy
        );
        drop(latest);
    }

    #[test]
    fn retirement_backpressure_invalidates_and_keeps_retry_input() {
        let (started, starts) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let worker = fixture_worker(move |request, cancelled| {
            // Preserve the typed operation.
            started.send(request.key.request_generation).unwrap();
            if request.key.request_generation == 1 {
                gate.recv().unwrap();
            }
            prepare_positions(request, cancelled)
        }); // End block.
        worker.try_submit_latest(&request(1)).unwrap();
        starts.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.try_submit_latest(&request(2)).unwrap();
        worker.try_submit_latest(&request(3)).unwrap();
        let newest = request(4);
        assert_eq!(
            worker.try_submit_latest(&newest).unwrap(),
            SubmitState::Busy
        );
        assert_eq!(worker.shared.desired.load(Ordering::Acquire), 4);
        assert_eq!(newest.positions.len(), 1);
        release.send(()).unwrap();
        // Worker will discard obsolete C without calling its preparer; retry
        // after the gate's real preparation completion clears pending cleanup.
        let deadline = Instant::now() + Duration::from_secs(5);
        while worker.try_submit_latest(&newest).unwrap() == SubmitState::Busy {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(starts.recv_timeout(Duration::from_secs(5)).unwrap(), 4);
    }

    #[test]
    fn full_key_and_monotonic_generation_reject_late_aba_results() {
        let worker = fixture_worker(prepare_positions); // Preserve the typed operation.
        let original = request(1);
        *worker.shared.latest.lock().unwrap() =
            Some(Arc::new(prepare_positions(&original, &|| false).unwrap()));
        worker.invalidate(1);
        assert!(worker.try_latest(original.key).unwrap().is_some());
        for changed in [
            MarkerKey {
                data_generation: 2,
                ..original.key
            },
            MarkerKey {
                cell_width: 81,
                ..original.key
            },
            MarkerKey {
                dot_height: 97,
                ..original.key
            },
            MarkerKey {
                marker_brightness: 20,
                show_heading: true,
                ..original.key
            },
            MarkerKey {
                kind: MapKind::Aircraft,
                ..original.key
            },
        ] {
            assert!(worker.try_latest(changed).unwrap().is_none());
        }
        worker.invalidate(2);
        worker.invalidate(3);
        worker.invalidate(1);
        assert!(worker.try_latest(original.key).unwrap().is_none());
        assert_eq!(
            worker.try_submit_latest(&original).unwrap(),
            SubmitState::Stale
        );
    }

    #[test]
    fn cancellation_happens_at_chunk_boundaries_and_before_return() {
        use std::cell::Cell;
        let mut input = request(1);
        input.positions = Arc::new(vec![input.positions[0].clone(); 400]);
        let checks = Cell::new(0);
        assert!(prepare_positions(&input, &|| {
            checks.set(checks.get() + 1);
            checks.get() == 3
        })
        .is_none());
        assert_eq!(checks.get(), 3);
        input.positions = Arc::new(vec![input.positions[0].clone(); 256]);
        let chunks = Cell::new(0);
        assert!(prepare_positions(&input, &|| {
            chunks.set(chunks.get() + 1);
            false
        })
        .is_some());
        // Entry, records 0 and 128, and the final publication guard.
        assert_eq!(chunks.get(), 4);
        let mut short = request(1);
        short.positions = Arc::new(Vec::new());
        let checks = Cell::new(0);
        assert!(prepare_positions(&short, &|| {
            checks.set(checks.get() + 1);
            checks.get() > 1
        })
        .is_none());
    }

    #[test]
    fn drop_does_not_join_a_gated_worker_and_worker_reclaims_input() {
        let (started, starts) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let (finished, finishes) = mpsc::channel();
        let worker = fixture_worker(move |request, cancelled| {
            // Preserve the typed operation.
            started.send(()).unwrap();
            gate.recv().unwrap();
            let result = prepare_positions(request, cancelled);
            finished.send(()).unwrap();
            result
        }); // End block.
        let input = request(1);
        let weak = Arc::downgrade(&input.positions);
        worker.try_submit_latest(&input).unwrap();
        starts.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(input);
        let start = Instant::now();
        drop(worker);
        assert!(start.elapsed() < Duration::from_millis(100));
        assert!(weak.upgrade().is_some());
        release.send(()).unwrap();
        finishes.recv_timeout(Duration::from_secs(5)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while weak.upgrade().is_some() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }

    #[test]
    fn coincident_cells_keep_both_heading_strokes() {
        let mut north = request(1);
        Arc::make_mut(&mut north.positions)[0].heading_degrees = Some(0.0);
        let mut east = north.positions[0].clone();
        east.heading_degrees = Some(90.0);
        let single = prepare_positions(&north, &|| false).unwrap();
        Arc::make_mut(&mut north.positions).push(east);
        let both = prepare_positions(&north, &|| false).unwrap();
        assert_eq!(both.accepted_positions, 2);
        assert_eq!(both.unique_center_cells, 1);
        assert!(both
            .raster
            .dots
            .iter()
            .zip(single.raster.dots)
            .any(|(a, b)| *a > b));
    }

    #[test]
    fn empty_zero_tiny_and_invalid_viewports_are_explicit() {
        let mut input = request(1);
        input.positions = Arc::new(Vec::new());
        let empty = prepare_positions(&input, &|| false).unwrap();
        assert_eq!(empty.accepted_positions, 0);
        assert_eq!(empty.occupied_centers.len(), 1920);
        input.key.cell_width = 0;
        input.key.dot_width = 0;
        assert!(prepare_positions(&input, &|| false)
            .unwrap()
            .raster
            .dots
            .is_empty());
        input = request(1);
        input.key.cell_width = 1;
        input.key.cell_height = 1;
        input.key.dot_width = 2;
        input.key.dot_height = 4;
        input.positions = Arc::new(vec![
            Position::new("pole".into(), 180.0, 90.0, Some(0)).unwrap(), // Preserve the typed operation.
            Position::new("south".into(), -180.0, -90.0, Some(0)).unwrap(), // Preserve the typed operation.
        ]);
        let tiny = prepare_positions(&input, &|| false).unwrap();
        assert_eq!(tiny.accepted_positions, 2);
        assert_eq!(tiny.occupied_centers, [true]);
        input.key.dot_width = usize::MAX;
        assert!(prepare_positions(&input, &|| false).is_none());
        assert_eq!(
            fixture_worker(prepare_positions) // Preserve the typed operation.
                .try_submit_latest(&input)
                .unwrap_err(),
            MarkerError::InvalidViewport
        );
    }

    #[test]
    fn poisoned_ui_state_and_disconnection_are_reported() {
        let worker = fixture_worker(prepare_positions); // Preserve the typed operation.
        let state = Arc::clone(&worker.shared);
        std::thread::spawn(move || {
            let _guard = state.latest.lock().unwrap();
            panic!("fixture poison");
        })
        .join()
        .unwrap_err();
        assert_eq!(
            worker.try_latest(request(1).key).unwrap_err(),
            MarkerError::Poisoned
        );
        let (dead_sender, dead_receiver) = mpsc::sync_channel(1);
        drop(dead_receiver);
        let mut disconnected = fixture_worker(prepare_positions); // Preserve the typed operation.
        disconnected.wake = dead_sender;
        assert_eq!(
            disconnected.try_submit_latest(&request(1)).unwrap_err(),
            MarkerError::Disconnected
        );
    }

    #[test]
    fn preparation_failure_is_visible_instead_of_silently_stalling() {
        let worker = fixture_worker(|_, _| None); // Preserve the typed operation.
        worker.try_submit_latest(&request(1)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if worker
                .try_latest(request(1).key)
                .is_err_and(|e| e == MarkerError::PreparationFailed)
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            worker.try_submit_latest(&request(2)).unwrap_err(),
            MarkerError::PreparationFailed
        );
    }

    #[test]
    fn unknown_invalid_heading_and_zero_brightness_do_not_invent_strokes() {
        let mut input = request(1);
        let unknown = prepare_positions(&input, &|| false).unwrap();
        for heading in [f64::NAN, -1.0, 360.0, 511.0] {
            Arc::make_mut(&mut input.positions)[0].heading_degrees = Some(heading);
            assert_eq!(
                prepare_positions(&input, &|| false).unwrap().raster.dots,
                unknown.raster.dots
            );
        }
        input.key.marker_brightness = 0;
        let dark = prepare_positions(&input, &|| false).unwrap();
        assert_eq!(dark.accepted_positions, 1);
        assert!(dark.raster.dots.iter().all(|value| *value == 0.0));
    }
}
