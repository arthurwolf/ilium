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
use crate::scene::{Frame, Scene, SceneEnv};
use crate::source::Worker;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

/// A changed terminal size must hold this long before ffmpeg is restarted.
const RESIZE_SETTLE: Duration = Duration::from_millis(500);

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
    /// Declared last so it is dropped (stopped and joined) after `Drop::drop`
    /// has already killed the child the worker may be blocked on.
    worker: Option<Worker>,
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
    pub fn new(settings: &VideoSettings, _env: &SceneEnv) -> Self {
        Self::with_runner(settings, Arc::new(SystemRunner))
    }

    /// Like `new`, with the process spawner replaced (tests).
    pub(super) fn with_runner(settings: &VideoSettings, runner: Arc<dyn CommandRunner>) -> Self {
        let slot = ChildSlot::default();
        let options = stream_options(settings);
        let source = FfmpegSource::new(runner, slot.clone(), options.clone());
        let config = PlayerConfig::new(settings.clone(), options, resolve_seed(settings.seed));
        Self::with_source(settings, config, Box::new(source), slot)
    }

    pub(super) fn with_source(
        settings: &VideoSettings,
        config: PlayerConfig,
        source: Box<dyn FrameSource>,
        slot: ChildSlot,
    ) -> Self {
        let (sender, receiver) = sync_channel(QUEUE_FRAMES);
        let shared = Arc::new(SharedState::default());
        let worker_shared = Arc::clone(&shared);
        let worker = Worker::spawn("video", move |stop| {
            run_player(config, source, sender, worker_shared, stop);
        });
        Self {
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
            worker: Some(worker),
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
        loop {
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
    }
}

impl Scene for VideoScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        self.track_size((frame.width, frame.height), frame.wall);
        self.pump(frame.wall);
        let colored = self.uses_cell_colors();
        match &self.current {
            Some(current) => {
                let dots = current
                    .decoded
                    .resampled_dots(frame.raster.width, frame.raster.height);
                if dots.len() == frame.raster.dots.len() {
                    frame.raster.dots.copy_from_slice(&dots);
                }
                if colored {
                    let columns = usize::from(frame.width);
                    let rows = usize::from(frame.height);
                    let mut colors = current.decoded.resampled_colors(columns, rows);
                    colors.resize(columns * rows, [90, 90, 90]);
                    *frame.cell_colors = colors;
                }
            }
            None => {
                draw_placeholder(frame.raster, frame.wall);
                if colored {
                    let cells = usize::from(frame.width) * usize::from(frame.height);
                    frame.cell_colors.clear();
                    frame.cell_colors.resize(cells, [90, 90, 90]);
                }
            }
        }
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
