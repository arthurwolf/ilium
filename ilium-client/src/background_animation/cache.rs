//! Packed loop generation runs away from input/rendering. A receiver belongs
//! to one settings generation, so cancelled work can never publish stale frames.
use super::{AnimationFrame, AnimationSettings};
use ilium_ambient::resources::WorkerCost;
use ilium_ambient::source::Worker;
use ilium_execution::StorageAdmission;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub(crate) const CACHE_FPS: u32 = 30;
const MAX_CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_CACHE_CELLS: usize = 131_072;
// Two working AnimationFrames, scene scratch and the worker's bounded state.
// Packed output storage is charged separately for as long as frames are cached.
const CACHE_WORKER_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AnimationCacheStatus {
    pub completed_frames: usize,
    pub total_frames: usize,
    pub estimated_bytes: usize,
    /// Resident packed-frame allocations, including their vector storage.
    /// This is cache storage, not whole-process RSS or scene scratch space.
    pub resident_bytes: usize,
    pub elapsed: Duration,
    pub eta: Option<Duration>,
    pub is_ready: bool,
    pub is_limited: bool,
    pub has_error: bool,
}

#[derive(Default)]
struct BuildProgress {
    completed: AtomicUsize,
    resident: AtomicUsize,
}
struct Build {
    worker: Worker,
    receiver: mpsc::Receiver<CompletedCache>,
    progress: Arc<BuildProgress>,
    started: Instant,
}
struct CompletedCache {
    // Keep the payload before its storage lease so the allocation is freed
    // before its quota credit is released.
    frames: Vec<Vec<u8>>,
    storage: Arc<StorageAdmission>,
}

pub struct AnimationLoopCache {
    resources: ilium_ambient::resources::AmbientResources,
    width: u16,
    height: u16,
    settings: Option<AnimationSettings>,
    frames: Vec<Vec<u8>>,
    _frame_storage: Option<Arc<StorageAdmission>>,
    build: Option<Build>,
    status: AnimationCacheStatus,
}
impl std::fmt::Debug for AnimationLoopCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimationLoopCache")
            .field("status", &self.status())
            .finish()
    }
}
impl Drop for AnimationLoopCache {
    fn drop(&mut self) {
        self.cancel_build();
    }
}

impl AnimationLoopCache {
    /// Every nested scene uses the host's original execution/quota identity.
    pub fn new(resources: ilium_ambient::resources::AmbientResources) -> Self {
        Self {
            resources,
            width: 0,
            height: 0,
            settings: None,
            frames: Vec::new(),
            _frame_storage: None,
            build: None,
            status: AnimationCacheStatus::default(),
        }
    }
    fn cancel_build(&mut self) {
        if let Some(build) = self.build.take() {
            build.worker.stop_in_background();
        }
    }

    /// Stop unfinished work while no surface displays it, retaining a ready
    /// cache for the next visit. Admission remains held until cancellation ends.
    pub fn pause(&mut self) {
        if self.build.is_none() {
            return;
        }
        self.cancel_build();
        self.status.completed_frames = 0;
        self.status.resident_bytes = 0;
        self.status.elapsed = Duration::ZERO;
        self.status.eta = None;
        self.frames = Vec::new();
        self._frame_storage = None;
    }

    pub fn begin(&mut self, settings: &AnimationSettings, width: u16, height: u16) {
        let mut settings = settings.normalized();
        // Palette and visibility never change packed monochrome geometry.
        settings.enabled = true;
        settings.lightness_percent = 50;
        settings.hue_degrees = 0;
        settings.saturation_percent = 0;
        // The look (colors, brightness, panels, frame cap) is applied when a
        // frame is shown; only the pattern controls change packed geometry.
        settings.appearance = ilium_ambient::style::Appearance {
            pattern_contrast_percent: settings.appearance.pattern_contrast_percent,
            pattern_invert: settings.appearance.pattern_invert,
            ..Default::default()
        };
        settings.panels = super::PanelTarget::Both;
        settings.fps_limit = 0;
        settings.semantic_scope = super::SemanticScope::Project;
        if self.settings.as_ref() == Some(&settings) && self.width == width && self.height == height
        {
            return;
        }
        self.cancel_build();
        self.frames = Vec::new();
        self._frame_storage = None;
        self.width = width;
        self.height = height;
        let total_frames = usize::from(settings.loop_seconds) * CACHE_FPS as usize;
        let cells = usize::from(width) * usize::from(height);
        let estimated_bytes = cells
            .saturating_add(std::mem::size_of::<Vec<u8>>())
            .saturating_mul(total_frames);
        self.status = AnimationCacheStatus {
            total_frames,
            estimated_bytes,
            is_limited: !settings.uses_loop_cache()
                || estimated_bytes > MAX_CACHE_BYTES
                || cells > MAX_CACHE_CELLS,
            ..Default::default()
        };
        self.settings = Some(settings);
        self.start_build();
    }

    fn start_build(&mut self) {
        if self.build.is_some()
            || self.status.is_ready
            || self.status.is_limited
            || self.status.has_error
            || self.width == 0
            || self.height == 0
        {
            return;
        }
        let worker_cost = WorkerCost {
            threads: 1,
            resident_bytes: CACHE_WORKER_BYTES,
        };
        let limits = self.resources.finite().quota_group().snapshot().limits;
        let Some(required_worker_bytes) = self
            .status
            .estimated_bytes
            .checked_add(worker_cost.resident_bytes)
        else {
            self.status.is_limited = true;
            return;
        };
        // These ceilings are immutable for this execution identity. Refuse a
        // cache that can never fit, while keeping current occupancy/admission
        // contention retryable below.
        if worker_cost.threads > limits.worker_threads
            || required_worker_bytes > limits.worker_bytes
        {
            self.status.is_limited = true;
            return;
        }
        // Admission is shared with every client and retained before cloning
        // settings/resources or allocating the packed frame batch.
        let storage = match self.resources.reserve_storage(self.status.estimated_bytes) {
            Ok(storage) => storage,
            Err(_) => return,
        };
        let worker_reservation = match self.resources.reserve_worker(worker_cost) {
            Ok(reservation) => reservation,
            Err(_) => return,
        };
        let Some(settings) = self.settings.clone() else {
            return;
        };
        let (width, height, total) = (self.width, self.height, self.status.total_frames);
        let resources = self.resources.clone();
        let worker_storage = Arc::clone(&storage);
        let progress = Arc::new(BuildProgress::default());
        let worker_progress = Arc::clone(&progress);
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = Worker::start_admitted("loop-cache", worker_reservation, move |stop| {
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::Lowest,
            );
            let mut generator = AnimationFrame::default();
            generator.configure_resources(resources.clone());
            let mut head = AnimationFrame::default();
            head.configure_resources(resources);
            let mut frames = Vec::with_capacity(total);
            let mut resident = frames.capacity() * std::mem::size_of::<Vec<u8>>();
            worker_progress.resident.store(resident, Ordering::Release);
            for index in 0..total {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                render_loop_sample(&mut generator, &mut head, &settings, width, height, index);
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let cells = generator.packed_cells().to_vec();
                resident += cells.capacity();
                frames.push(cells);
                worker_progress.resident.store(resident, Ordering::Release);
                worker_progress
                    .completed
                    .store(index + 1, Ordering::Release);
            }
            let _ = sender.send(CompletedCache {
                frames,
                storage: worker_storage,
            });
        });
        match worker {
            Ok(worker) => {
                self.build = Some(Build {
                    worker,
                    receiver,
                    progress,
                    started: Instant::now(),
                })
            }
            Err(error) => {
                tracing::warn!(%error, "animation cache worker could not start");
                self.status.has_error = true;
            }
        }
    }

    /// Keep the existing compositor API, but never render a batch on its thread.
    pub fn step(
        &mut self,
        settings: &AnimationSettings,
        width: u16,
        height: u16,
        _frame_budget: usize,
    ) -> bool {
        self.begin(settings, width, height);
        self.start_build();
        let result = self.build.as_ref().map(|build| build.receiver.try_recv());
        if let Some(result) = result {
            match result {
                Ok(completed) => {
                    self.status = self.status();
                    self.frames = completed.frames;
                    self._frame_storage = Some(completed.storage);
                    self.status.is_ready = true;
                    self.status.eta = Some(Duration::ZERO);
                    self.cancel_build();
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.status.has_error = true;
                    self.status.resident_bytes = 0;
                    self.cancel_build();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        self.status.is_ready
    }

    pub fn status(&self) -> AnimationCacheStatus {
        let mut status = self.status;
        if let Some(build) = &self.build {
            status.completed_frames = build.progress.completed.load(Ordering::Acquire);
            status.resident_bytes = build.progress.resident.load(Ordering::Acquire);
            status.elapsed = build.started.elapsed();
            status.eta = (status.completed_frames > 0).then(|| {
                status.elapsed.mul_f64(
                    status.total_frames.saturating_sub(status.completed_frames) as f64
                        / status.completed_frames as f64,
                )
            });
        }
        status
    }

    pub fn frame_index(&self, elapsed: Duration) -> usize {
        if self.frames.is_empty() {
            return 0;
        }
        ((elapsed.as_nanos() * u128::from(CACHE_FPS) / 1_000_000_000) % self.frames.len() as u128)
            as usize
    }

    pub fn copy_frame_into(&self, elapsed: Duration, frame: &mut AnimationFrame) -> bool {
        let Some(cells) = self.frames.get(self.frame_index(elapsed)) else {
            return false;
        };
        frame.load_packed_cells(self.width, self.height, cells);
        true
    }
}

fn render_loop_sample(
    generator: &mut AnimationFrame,
    head: &mut AnimationFrame,
    settings: &AnimationSettings,
    width: u16,
    height: u16,
    index: usize,
) {
    let time = index as f64 / f64::from(CACHE_FPS);
    let duration = f64::from(settings.loop_seconds);
    let window = (duration / 4.0).min(2.0);
    generator.render(
        settings,
        width,
        height,
        Duration::from_secs_f64(time + window),
    );
    let tail_time = time - (duration - window);
    if tail_time < 0.0 {
        return;
    }
    head.render(settings, width, height, Duration::from_secs_f64(tail_time));
    let fraction = (tail_time / window) as f32;
    let weight = fraction * fraction * (3.0 - 2.0 * fraction);
    for (dot, target) in generator.raster.dots.iter_mut().zip(&head.raster.dots) {
        *dot = *dot * (1.0 - weight) + target * weight;
    }
    generator.pack(super::PackKey::of(settings));
}
