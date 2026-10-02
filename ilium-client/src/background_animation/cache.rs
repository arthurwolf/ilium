//! Packed loop generation runs away from input/rendering. A receiver belongs
//! to one settings generation, so cancelled work can never publish stale frames.
use super::{AnimationFrame, AnimationSettings};
use ilium_ambient::source::Worker;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub(crate) const CACHE_FPS: u32 = 30;
const MAX_CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_CACHE_CELLS: usize = 131_072;
static BUILDERS: AtomicUsize = AtomicUsize::new(0);

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
    receiver: mpsc::Receiver<Vec<Vec<u8>>>,
    progress: Arc<BuildProgress>,
    started: Instant,
}
struct BuilderPermit;
impl BuilderPermit {
    fn acquire() -> Option<Self> {
        BUILDERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 2).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
impl Drop for BuilderPermit {
    fn drop(&mut self) {
        BUILDERS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Default)]
pub struct AnimationLoopCache {
    width: u16,
    height: u16,
    settings: Option<AnimationSettings>,
    frames: Vec<Vec<u8>>,
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
    }

    pub fn begin(&mut self, settings: &AnimationSettings, width: u16, height: u16) {
        let mut settings = settings.normalized();
        // Palette and visibility never change packed monochrome geometry.
        settings.enabled = true;
        settings.lightness_percent = 50;
        settings.hue_degrees = 0;
        settings.saturation_percent = 0;
        if self.settings.as_ref() == Some(&settings) && self.width == width && self.height == height
        {
            return;
        }
        self.cancel_build();
        self.frames = Vec::new();
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
        let Some(permit) = BuilderPermit::acquire() else {
            return;
        };
        let Some(settings) = self.settings.clone() else {
            return;
        };
        let (width, height, total) = (self.width, self.height, self.status.total_frames);
        let progress = Arc::new(BuildProgress::default());
        let worker_progress = Arc::clone(&progress);
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = Worker::try_spawn("loop-cache", move |stop| {
            let _permit = permit;
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::Lowest,
            );
            let mut generator = AnimationFrame::default();
            let mut head = AnimationFrame::default();
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
            let _ = sender.send(frames);
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
                Ok(frames) => {
                    self.status = self.status();
                    self.frames = frames;
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
    generator.pack(settings.density_percent, settings.dither);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::background_animation::AnimationKind;

    fn ready_cache(settings: &AnimationSettings) -> AnimationLoopCache {
        let mut cache = AnimationLoopCache::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cache.step(settings, 24, 12, 8) {
            assert!(Instant::now() < deadline, "background cache must complete");
            std::thread::sleep(Duration::from_millis(1));
        }
        cache
    }

    #[test]
    fn ready_playback_copies_without_rendering_and_accounts_allocations() {
        let settings = AnimationSettings {
            loop_seconds: 1,
            ..Default::default()
        };
        let cache = ready_cache(&settings);
        let actual = cache.frames.capacity() * std::mem::size_of::<Vec<u8>>()
            + cache.frames.iter().map(Vec::capacity).sum::<usize>();
        assert_eq!(cache.status().resident_bytes, actual);
        let mut frame = AnimationFrame::default();
        for index in 0..300 {
            assert!(cache.copy_frame_into(Duration::from_millis(index * 17), &mut frame));
        }
        assert_eq!(frame.geometry_render_count, 0);
        assert_eq!(frame.scene_cache.preparations, 0);
    }

    #[test]
    fn oversized_cache_is_rejected_without_allocating_or_starting_a_worker() {
        let mut cache = AnimationLoopCache::default();
        cache.begin(&AnimationSettings::default(), u16::MAX, u16::MAX);
        assert!(cache.status().is_limited);
        assert_eq!(cache.status().resident_bytes, 0);
        assert!(cache.build.is_none());
        assert!(cache.frames.is_empty());
    }

    #[test]
    fn starting_and_cancelling_long_caches_does_not_render_or_join_on_the_caller() {
        let mut cache = AnimationLoopCache::default();
        let mut settings = AnimationSettings {
            kind: AnimationKind::Kelp,
            loop_seconds: 120,
            ..Default::default()
        };
        for size in [(120, 40), (160, 60), (100, 30)] {
            settings.kelp.current_strength_percent += 1;
            let started = Instant::now();
            assert!(!cache.step(&settings, size.0, size.1, usize::MAX));
            cache.pause();
            assert!(
                started.elapsed() < Duration::from_millis(100),
                "caller must not calculate a requested batch or join cancelled work"
            );
            assert_eq!(cache.status().resident_bytes, 0);
        }
    }

    #[test]
    fn replacement_generation_never_uses_stale_dimensions_or_settings() {
        let mut cache = AnimationLoopCache::default();
        cache.begin(&AnimationSettings::default(), 80, 24);
        let settings = AnimationSettings {
            kind: AnimationKind::Kelp,
            loop_seconds: 1,
            ..Default::default()
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cache.step(&settings, 8, 4, 8) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!((cache.width, cache.height), (8, 4));
        assert_eq!(cache.frames.len(), 30);
        assert!(cache.frames.iter().all(|frame| frame.len() == 32));
        assert_eq!(cache.settings.as_ref().unwrap().kind, AnimationKind::Kelp);
    }

    #[test]
    fn hillside_and_kelp_wrap_with_an_ordinary_forward_frame_step() {
        for kind in [AnimationKind::WindyHillside, AnimationKind::Kelp] {
            let settings = AnimationSettings {
                kind,
                loop_seconds: 2,
                ..Default::default()
            };
            let mut generator = AnimationFrame::default();
            let mut head = AnimationFrame::default();
            let mut samples = Vec::new();
            for index in 0..60 {
                render_loop_sample(&mut generator, &mut head, &settings, 24, 12, index);
                samples.push(generator.raster.dots.clone());
            }
            let difference = |left: &[f32], right: &[f32]| {
                left.iter()
                    .zip(right)
                    .map(|(a, b)| (a - b).abs())
                    .sum::<f32>()
                    / left.len() as f32
            };
            let ordinary = samples
                .windows(2)
                .map(|pair| difference(&pair[0], &pair[1]))
                .fold(0.0_f32, f32::max);
            // Also check the transition itself: a large tail spike must not
            // make the broad adjacent-frame bound pass a discontinuous seam.
            let pre_tail = samples[..45]
                .windows(2)
                .map(|pair| difference(&pair[0], &pair[1]))
                .fold(0.0_f32, f32::max);
            let tail = samples[44..]
                .windows(2)
                .map(|pair| difference(&pair[0], &pair[1]))
                .fold(0.0_f32, f32::max);
            assert!(
                tail <= pre_tail * 3.0 + 0.001,
                "{kind:?}: tail {tail} pre-tail {pre_tail}"
            );
            let seam = difference(&samples[59], &samples[0]);
            assert!(
                seam <= ordinary * 1.2 + 0.001,
                "{kind:?}: seam {seam} ordinary {ordinary}"
            );
            assert!(ordinary > 0.0001, "scene must keep moving");
        }
    }
}

#[cfg(test)]
mod replacement_lifecycle_tests {
    include!("replacement_lifecycle_tests.rs");
}
