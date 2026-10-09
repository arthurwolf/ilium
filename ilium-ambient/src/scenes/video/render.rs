//! `VideoScene`: owns the worker, shows the newest due frame, reports status.

use super::command::{ChildSlot, CommandRunner, PixelFormat, StreamOptions, SystemRunner};
use super::convert::{decode_frame, Decoded, Tone};
use super::ffmpeg::FfmpegSource;
use super::player::{
    run_player, FrameSource, NowPlaying, PlayerConfig, SharedState, VideoFrame, QUEUE_FRAMES,
};
use super::schedule::{format_clock, resolve_seed};
use super::settings::{PlaybackMode, RenderStyle, VideoSettings};
use crate::raster::Raster;
use crate::resources::{AmbientResources, WorkerCost};
use crate::scene::{Frame, Scene, SceneEnv};
use crate::source::Worker;
use crate::style::ScenePalette;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A changed terminal size must hold this long before ffmpeg is restarted.
const RESIZE_SETTLE: Duration = Duration::from_millis(500);
// Unit tests exercise independently owned scenes concurrently; the real
// client has one backdrop and at most one settings preview.
#[cfg(not(test))]
const MAX_VIDEO_WORKERS: usize = 2;
#[cfg(test)]
const MAX_VIDEO_WORKERS: usize = 64;
static ACTIVE_VIDEO_WORKERS: AtomicUsize = AtomicUsize::new(0);

struct VideoAdmission;
impl Drop for VideoAdmission {
    fn drop(&mut self) {
        ACTIVE_VIDEO_WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Current {
    decoded: Decoded,
    playing: Arc<NowPlaying>,
    source_seconds: f64,
}

pub struct VideoScene {
    settings: VideoSettings,
    slot: ChildSlot,
    shared: Arc<SharedState>,
    receiver: Receiver<VideoFrame>,
    tone: Tone,
    pending: Option<VideoFrame>,
    current: Option<Current>,
    segment: Option<u64>,
    origin: Duration,
    /// Size (cells) the worker was last asked to decode at.
    requested: Option<(u16, u16)>,
    /// A different size seen since this wall time, not yet acted upon.
    settling: Option<((u16, u16), Duration)>,
    start: Option<Box<dyn FnOnce(Arc<std::sync::atomic::AtomicBool>) + Send>>,
    worker: Option<Worker>,
    palette: ScenePalette,
    resources: AmbientResources,
    // Covers the simultaneous bounded queue, producer, pending, current and
    // conversion/resample allocation inventory through their last owner.
    _pipeline_storage: Option<Arc<ilium_execution::StorageAdmission>>,
    retry_after: Option<Instant>,
}

pub(super) fn stream_options(settings: &VideoSettings) -> StreamOptions {
    StreamOptions {
        frames_per_second: settings.frame_rate,
        slowed_percent: (settings.mode == PlaybackMode::Slowed).then_some(settings.slowed_percent),
        fit: settings.fit,
        pixel_format: if settings.style == RenderStyle::Colored {
            PixelFormat::Rgb24
        } else {
            PixelFormat::Gray
        },
        detail: settings.detail,
    }
}

impl VideoScene {
    // PALETTE (native Scene contract): `env.palette` is the shared look's current
    // palette. This scene follows it natively: its cell colours are mapped onto the
    // palette by brightness (`ScenePalette::recolor`) as each frame is produced, and
    // `Scene::set_palette` delivers later changes (applied from the next render).
    // With no palette provided the colours are untouched.
    pub fn new(settings: &VideoSettings, env: &SceneEnv) -> Self {
        let mut scene = Self::with_runner_resources(
            settings,
            Arc::new(SystemRunner::new(env.resources.clone())),
            env.resources.clone(),
        );
        scene.palette = env.palette.clone();
        scene
    }

    #[cfg(test)]
    pub(super) fn with_runner(settings: &VideoSettings, runner: Arc<dyn CommandRunner>) -> Self {
        Self::with_runner_resources(settings, runner, crate::resources::test_resources())
    }

    pub(super) fn with_runner_resources(
        settings: &VideoSettings,
        runner: Arc<dyn CommandRunner>,
        resources: AmbientResources,
    ) -> Self {
        let slot = ChildSlot::default();
        let options = stream_options(settings);
        let source = FfmpegSource::new(runner, slot.clone(), options.clone(), resources.clone());
        let config = PlayerConfig::new(settings.clone(), options, resolve_seed(settings.seed));
        Self::with_source_resources(settings, config, Box::new(source), slot, resources)
    }

    #[cfg(test)]
    pub(super) fn with_source(
        settings: &VideoSettings,
        config: PlayerConfig,
        source: Box<dyn FrameSource>,
        slot: ChildSlot,
    ) -> Self {
        Self::with_source_resources(
            settings,
            config,
            source,
            slot,
            crate::resources::test_resources(),
        )
    }

    fn with_source_resources(
        settings: &VideoSettings,
        config: PlayerConfig,
        source: Box<dyn FrameSource>,
        slot: ChildSlot,
        resources: AmbientResources,
    ) -> Self {
        let (sender, receiver) = sync_channel(QUEUE_FRAMES);
        let shared = Arc::new(SharedState::default());
        let worker_shared = Arc::clone(&shared);
        let start = Box::new(move |stop| run_player(config, source, sender, worker_shared, stop));
        let mut scene = Self {
            settings: settings.clone(),
            slot,
            shared,
            receiver,
            tone: Tone::new(settings),
            pending: None,
            current: None,
            segment: None,
            origin: Duration::ZERO,
            requested: None,
            settling: None,
            start: Some(start),
            worker: None,
            palette: ScenePalette::default(),
            resources,
            _pipeline_storage: None,
            retry_after: None,
        };
        scene.try_start();
        scene
    }

    fn try_start(&mut self) {
        if self.start.is_none() || self.retry_after.is_some_and(|at| Instant::now() < at) {
            return;
        }
        let admitted = ACTIVE_VIDEO_WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_VIDEO_WORKERS).then_some(count + 1)
            })
            .is_ok();
        if !admitted {
            self.retry_after = Some(Instant::now() + Duration::from_secs(1));
            self.shared.set_notice(Some(
                "Waiting for previous video cleanup; other scenes remain available".into(),
            ));
            return;
        }
        let admission = VideoAdmission;
        // 12 queued RGB frames, producer/pending frames, two conversion planes,
        // retained current and resize transients fit this conservative envelope.
        let storage = match self.resources.reserve_storage(40 * 1024 * 1024) {
            Ok(storage) => storage,
            Err(error) => {
                self.retry_after = Some(Instant::now() + Duration::from_secs(1));
                self.shared.set_notice(Some(format!(
                    "Video frame storage admission rejected: {error:?}; retry shortly"
                )));
                return;
            }
        };
        let physical = match self.resources.reserve_worker(WorkerCost {
            threads: 1,
            resident_bytes: 8 * 1024 * 1024,
        }) {
            Ok(physical) => physical,
            Err(error) => {
                self.retry_after = Some(Instant::now() + Duration::from_secs(1));
                self.shared.set_notice(Some(format!(
                    "Video worker host admission rejected: {error:?}; retry shortly"
                )));
                return;
            }
        };
        if let Some(start) = self.start.take() {
            let display_storage = Arc::clone(&storage);
            match Worker::start_admitted("video", physical, move |stop| {
                let _storage = storage;
                let _admission = admission;
                start(stop);
            }) {
                Ok(worker) => {
                    self._pipeline_storage = Some(display_storage);
                    self.worker = Some(worker);
                }
                Err(error) => self.shared.set_notice(Some(format!(
                    "Could not start video worker: {error}; choose another scene and retry"
                ))),
            }
        }
    }

    /// Debounced size tracking: the first size is requested at once, later
    /// changes only after they have held for `RESIZE_SETTLE`.
    fn track_size(&mut self, size: (u16, u16), wall: Duration) {
        match self.requested {
            None => self.request_size(size),
            Some(requested) if requested == size => self.settling = None,
            Some(_) => match self.settling {
                Some((candidate, since)) if candidate == size => {
                    if wall.saturating_sub(since) >= RESIZE_SETTLE {
                        self.request_size(size);
                    }
                }
                _ => self.settling = Some((size, wall)),
            },
        }
    }

    fn request_size(&mut self, size: (u16, u16)) {
        self.requested = Some(size);
        self.settling = None;
        self.shared.set_size(size.0, size.1);
    }

    /// Take every queued frame that is due at `wall`; the newest one wins.
    fn pump(&mut self, wall: Duration) {
        let Some((columns, rows)) = self.requested else {
            return;
        };
        let expected = (usize::from(columns) * 2, usize::from(rows) * 4);
        // Only the newest due frame is worth decoding after a stall.
        let mut newest: Option<VideoFrame> = None;
        for _ in 0..=QUEUE_FRAMES {
            let next = match self.pending.take() {
                Some(frame) => frame,
                None => match self.receiver.try_recv() {
                    Ok(frame) => frame,
                    Err(_) => break,
                },
            };
            // Frames decoded for an earlier size are stale.
            if (next.width, next.height) != expected {
                continue;
            }
            if self.segment != Some(next.segment) {
                self.segment = Some(next.segment);
                self.origin = wall.saturating_sub(next.pts);
            }
            if next.pts <= wall.saturating_sub(self.origin) {
                newest = Some(next);
            } else {
                self.pending = Some(next);
                break;
            }
        }
        if let Some(frame) = newest {
            self.show(frame);
        }
    }

    fn show(&mut self, frame: VideoFrame) {
        let decoded = decode_frame(
            &frame.data,
            frame.width,
            frame.height,
            frame.format,
            self.settings.style,
            &self.tone,
        );
        if let Some(decoded) = decoded {
            self.shared.diagnostics.decoded(frame.source_seconds);
            self.current = Some(Current {
                decoded,
                playing: frame.playing,
                source_seconds: frame.source_seconds,
            });
        }
    }

    /// Source position of the frame on screen (tests).
    #[cfg(test)]
    pub(super) fn displayed_seconds(&self) -> Option<f64> {
        self.current.as_ref().map(|current| current.source_seconds)
    }
}

impl Drop for VideoScene {
    fn drop(&mut self) {
        // Order matters: raise the stop flag, then kill the child so a worker
        // blocked in `read_exact` wakes up; the field drop joins it afterwards.
        if let Some(worker) = &self.worker {
            worker
                .stop_flag()
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.slot.close();
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}

impl Scene for VideoScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        self.shared.diagnostics.rendered(frame.wall);
        self.try_start();
        self.track_size((frame.width.min(512), frame.height.min(128)), frame.wall);
        self.pump(frame.wall);
        let colored = self.uses_cell_colors();
        match &self.current {
            Some(current) => {
                if (current.decoded.width, current.decoded.height)
                    == (frame.raster.width, frame.raster.height)
                {
                    frame.raster.dots.copy_from_slice(&current.decoded.dots);
                } else {
                    let dots = current
                        .decoded
                        .resampled_dots(frame.raster.width, frame.raster.height);
                    if dots.len() == frame.raster.dots.len() {
                        frame.raster.dots.copy_from_slice(&dots);
                    }
                }
                if colored {
                    let columns = usize::from(frame.width);
                    let rows = usize::from(frame.height);
                    let mut colors = current.decoded.resampled_colors(columns, rows);
                    colors.resize(columns * rows, [90, 90, 90]);
                    *frame.cell_colors = colors;
                    self.palette.recolor_cells(frame.cell_colors);
                }
            }
            None => {
                draw_placeholder(frame.raster, frame.wall);
                if colored {
                    let cells = usize::from(frame.width) * usize::from(frame.height);
                    frame.cell_colors.clear();
                    frame.cell_colors.resize(cells, [90, 90, 90]);
                    self.palette.recolor_cells(frame.cell_colors);
                }
            }
        }
    }

    fn set_palette(&mut self, palette: &ScenePalette) {
        self.palette = palette.clone();
    }

    fn follows_palette(&self) -> bool {
        true
    }

    fn uses_cell_colors(&self) -> bool {
        self.settings.style == RenderStyle::Colored
    }

    fn frames_per_second(&self) -> u32 {
        self.settings.frame_rate.clamp(6, 24)
    }

    fn status(&self) -> Option<String> {
        if let Some(notice) = self.shared.notice() {
            return Some(notice);
        }
        let Some(current) = &self.current else {
            return Some("Starting video...".to_owned());
        };
        if self.requested.is_some_and(|(columns, rows)| {
            current.decoded.width != usize::from(columns) * 2
                || current.decoded.height != usize::from(rows) * 4
        }) {
            // The retained picture is resampled while the new decoder starts
            // or replays its prefix. Playing must describe a current-sized
            // decoded frame, rather than the retained picture's old clock.
            return Some(format!("Resizing: {}", current.playing.name));
        }
        let mut text = format!(
            "Playing: {} {}",
            current.playing.name,
            format_clock(current.source_seconds)
        );
        if let Some(total) = current.playing.duration_seconds {
            text.push_str(&format!(" / {}", format_clock(total)));
        }
        if self.settings.mode == PlaybackMode::Slowed {
            text.push_str(&format!(" ({}% speed)", self.settings.slowed_percent));
        }
        if self.settings.uses_plain_http() {
            text.push_str(" - warning: http:// is not encrypted");
        }
        Some(text)
    }
}

/// Dim "no picture yet" mark: a 16:9 screen outline with a play triangle that
/// breathes slowly. Kept faint so it reads as a background, not content.
fn draw_placeholder(raster: &mut Raster, wall: Duration) {
    let (width, height) = (raster.width as f32, raster.height as f32);
    let side = (width * 0.5).min(height * 0.5 * 16.0 / 9.0);
    if side < 8.0 {
        return;
    }
    let (screen_w, screen_h) = (side, side * 9.0 / 16.0);
    let breath = 0.4 + 0.12 * (wall.as_secs_f32() * 1.4).sin();
    let point = |dx: f32, dy: f32| ((width / 2.0 + dx) / width, (height / 2.0 + dy) / height);
    let (half_w, half_h) = (screen_w / 2.0, screen_h / 2.0);
    let corners = [
        point(-half_w, -half_h),
        point(half_w, -half_h),
        point(half_w, half_h),
        point(-half_w, half_h),
    ];
    for index in 0..4 {
        raster.line(corners[index], corners[(index + 1) % 4], 0.5, breath);
    }
    let triangle = [
        point(-screen_w * 0.1, -screen_h * 0.24),
        point(-screen_w * 0.1, screen_h * 0.24),
        point(screen_w * 0.14, 0.0),
    ];
    for index in 0..3 {
        raster.line(
            triangle[index],
            triangle[(index + 1) % 3],
            0.5,
            breath * 0.9,
        );
    }
}

#[cfg(test)]
mod admission_tests {
    use super::super::discover::MediaInput;
    use super::super::player::{FrameStream, OpenError};
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
    };
    use std::sync::mpsc;
    use std::time::Instant;

    struct BlockedSource {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl FrameSource for BlockedSource {
        fn probe_duration(&mut self, _: &MediaInput) -> Option<f64> {
            None
        }
        fn open(
            &mut self,
            _: &super::super::command::PlayRequest,
        ) -> Result<Box<dyn FrameStream>, OpenError> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            Err(OpenError::Failed("synthetic release".into()))
        }
    }

    fn isolated(bytes: usize) -> (Execution, AmbientResources, QuotaGroup) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 2,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
            worker_threads: 3,
            worker_bytes: bytes,
        });
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: None,
                    resident_bytes_per_thread: 1024 * 1024,
                },
                io: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
                service: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 2,
                service_jobs: 0,
                input_bytes: 4096,
                result_bytes: 4096,
            })
            .unwrap();
        (execution, AmbientResources::new(client), quota)
    }

    #[test]
    fn host_rejection_is_recoverable_before_video_worker_start() {
        let (mut execution, resources, _) = isolated(16 * 1024 * 1024);
        let settings = VideoSettings {
            source: "https://example.invalid/clip.mp4".into(),
            ..VideoSettings::default()
        };
        let config = PlayerConfig::new(settings.clone(), stream_options(&settings), 1);
        let (entered, _) = mpsc::channel();
        let (_, release) = mpsc::channel();
        let scene = VideoScene::with_source_resources(
            &settings,
            config,
            Box::new(BlockedSource { entered, release }),
            ChildSlot::default(),
            resources.clone(),
        );
        assert!(scene.worker.is_none());
        assert!(scene
            .status()
            .unwrap()
            .contains("storage admission rejected"));
        drop(scene);
        drop(resources);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }

    #[test]
    fn blocked_video_owner_keeps_pipeline_and_thread_credit_after_scene_drop() {
        let (mut execution, resources, quota) = isolated(128 * 1024 * 1024);
        let baseline = quota.snapshot().worker_bytes;
        let settings = VideoSettings {
            source: "https://example.invalid/clip.mp4".into(),
            ..VideoSettings::default()
        };
        let config = PlayerConfig::new(settings.clone(), stream_options(&settings), 1);
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut scene = VideoScene::with_source_resources(
            &settings,
            config,
            Box::new(BlockedSource {
                entered,
                release: gate,
            }),
            ChildSlot::default(),
            resources.clone(),
        );
        crate::debug::render_frame(&mut scene, 20, 5, Duration::ZERO);
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        let ticket = scene.worker.as_ref().unwrap().join_observer().unwrap();
        let dropped_at = Instant::now();
        drop(scene);
        assert!(dropped_at.elapsed() < Duration::from_millis(100));
        assert!(quota.snapshot().worker_bytes >= baseline + 48 * 1024 * 1024);
        release.send(()).unwrap();
        ticket
            .join_until(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(quota.snapshot().worker_bytes >= baseline + 8 * 1024 * 1024);
        drop(ticket);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
        drop(resources);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }
}
