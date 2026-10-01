//! The worker-side playback loop: pick a clip, run a frame stream, hand
//! timestamped frames to the scene through a bounded queue, and recover from
//! every failure with a growing back-off instead of spinning.

use super::command::{PixelFormat, PlayRequest, StreamOptions};
use super::discover::{self, MediaInput};
use super::schedule::{frame_time, source_position, PlayItem, Scheduler};
use super::settings::VideoSettings;
use crate::source::sleep_unless_stopped;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Frames the worker may run ahead of the display.
pub const QUEUE_FRAMES: usize = 12;

#[derive(Debug)]
pub enum OpenError {
    /// The named external program is not installed.
    ToolMissing(&'static str),
    Failed(String),
}

/// Where frames come from. The production implementation runs ffmpeg; tests
/// supply synthetic frames.
pub trait FrameSource: Send {
    /// Length of `input` in seconds when it can be determined.
    fn probe_duration(&mut self, input: &MediaInput) -> Option<f64>;
    fn open(&mut self, request: &PlayRequest) -> Result<Box<dyn FrameStream>, OpenError>;
}

/// One clip in progress. Dropping it must release every resource, killing
/// and reaping a child process if there is one.
pub trait FrameStream: Send {
    /// Fill `buffer` with the next frame. `Ok(false)` at the end of the clip.
    fn read_frame(&mut self, buffer: &mut Vec<u8>) -> io::Result<bool>;
}

/// Facts about the clip a frame belongs to.
#[derive(Debug, PartialEq)]
pub struct NowPlaying {
    pub name: String,
    pub duration_seconds: Option<f64>,
}

pub struct VideoFrame {
    /// Increases with every clip (and every restart after a resize).
    pub segment: u64,
    pub playing: Arc<NowPlaying>,
    /// Presentation time from the start of this segment.
    pub pts: Duration,
    /// Position inside the source file, for the status line.
    pub source_seconds: f64,
    /// Size in dots.
    pub width: usize,
    pub height: usize,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

/// State shared between the scene and its worker.
#[derive(Default)]
pub struct SharedState {
    /// Wanted output size in terminal cells: `(width << 16) | height`; 0 = unknown.
    size: AtomicU32,
    notice: Mutex<Option<String>>,
}

impl SharedState {
    pub fn set_size(&self, width: u16, height: u16) {
        self.size.store(
            (u32::from(width) << 16) | u32::from(height),
            Ordering::Relaxed,
        );
    }

    pub fn size(&self) -> Option<(u16, u16)> {
        let packed = self.size.load(Ordering::Relaxed);
        let (width, height) = ((packed >> 16) as u16, (packed & 0xffff) as u16);
        (width > 0 && height > 0).then_some((width, height))
    }

    pub fn set_notice(&self, notice: Option<String>) {
        *self
            .notice
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = notice;
    }

    pub fn notice(&self) -> Option<String> {
        self.notice
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

pub struct PlayerConfig {
    pub settings: VideoSettings,
    pub options: StreamOptions,
    pub seed: u64,
    /// First retry delay after a failure; doubles up to `backoff_max`.
    pub backoff_base: Duration,
    pub backoff_max: Duration,
    /// How long to wait before looking again when there is nothing to play.
    pub idle_wait: Duration,
    /// How often the source is scanned again for added or removed files.
    pub rescan_interval: Duration,
}

impl PlayerConfig {
    pub fn new(settings: VideoSettings, options: StreamOptions, seed: u64) -> Self {
        Self {
            settings,
            options,
            seed,
            backoff_base: Duration::from_secs(1),
            backoff_max: Duration::from_secs(20),
            idle_wait: Duration::from_secs(2),
            rescan_interval: Duration::from_secs(60),
        }
    }
}

enum Outcome {
    Finished {
        frames: u64,
        elapsed: Duration,
    },
    /// Output size changed: restart at the same position.
    Resized {
        resume: PlayItem,
    },
    Stopped,
    Failed(String),
}

enum Push {
    Sent,
    Stop,
    Resized,
}

pub fn run_player(
    config: PlayerConfig,
    source: Box<dyn FrameSource>,
    sender: SyncSender<VideoFrame>,
    shared: Arc<SharedState>,
    stop: Arc<AtomicBool>,
) {
    Player {
        scheduler: Scheduler::new(&config.settings, config.seed),
        config,
        source,
        sender,
        shared,
        stop,
        scanned_at: None,
        segment: 0,
        failures: 0,
    }
    .run();
}

struct Player {
    config: PlayerConfig,
    source: Box<dyn FrameSource>,
    sender: SyncSender<VideoFrame>,
    shared: Arc<SharedState>,
    stop: Arc<AtomicBool>,
    scheduler: Scheduler,
    scanned_at: Option<Instant>,
    segment: u64,
    failures: u32,
}

impl Player {
    fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn run(mut self) {
        let mut resume: Option<PlayItem> = None;
        while !self.stopping() {
            let Some(size) = self.wait_for_size() else {
                return;
            };
            let item = match resume.take() {
                Some(item) => item,
                None => match self.next_item() {
                    Some(item) => item,
                    None => continue,
                },
            };
            match self.play(&item, size) {
                Outcome::Stopped => return,
                Outcome::Resized { resume: next } => resume = Some(next),
                Outcome::Finished { frames, elapsed } => {
                    let fast = elapsed < Duration::from_secs(1)
                        && frames < u64::from(self.config.options.frames_per_second);
                    if frames == 0 {
                        let name = item.input.display_name();
                        self.fail(format!("cannot play {name}"));
                    } else if fast {
                        // A clip that flashes by (or a loop of one frame)
                        // must not restart ffmpeg in a tight loop.
                        self.back_off(None);
                    } else {
                        self.failures = 0;
                    }
                }
                Outcome::Failed(message) => self.fail(message),
            }
        }
    }

    fn wait_for_size(&self) -> Option<(u16, u16)> {
        loop {
            if self.stopping() {
                return None;
            }
            if let Some(size) = self.shared.size() {
                return Some(size);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn fail(&mut self, message: String) {
        self.back_off(Some(message));
    }

    /// Wait `base * 2^(failures)` (capped), publishing `message` meanwhile.
    fn back_off(&mut self, message: Option<String>) {
        self.failures = self.failures.saturating_add(1);
        let exponent = self.failures.saturating_sub(1).min(16);
        let delay = self
            .config
            .backoff_base
            .saturating_mul(1u32 << exponent)
            .min(self.config.backoff_max);
        if let Some(message) = message {
            self.shared.set_notice(Some(format!(
                "{message}; next try in {} s",
                delay.as_secs().max(1)
            )));
        }
        sleep_unless_stopped(&self.stop, delay);
    }

    /// Choose the next clip, rescanning the source when it is due.
    fn next_item(&mut self) -> Option<PlayItem> {
        let due = self.scanned_at.is_none_or(|at| {
            self.scheduler.is_empty() || at.elapsed() >= self.config.rescan_interval
        });
        if due {
            self.rescan();
        }
        if self.scheduler.is_empty() {
            sleep_unless_stopped(&self.stop, self.config.idle_wait);
            return None;
        }
        let source = &mut self.source;
        let item = self
            .scheduler
            .next(&mut |input| source.probe_duration(input));
        if item.is_none() {
            sleep_unless_stopped(&self.stop, self.config.idle_wait);
        }
        item
    }

    fn rescan(&mut self) {
        self.scanned_at = Some(Instant::now());
        let settings = &self.config.settings;
        self.shared
            .set_notice(Some("Scanning video sources...".into()));
        let discovery =
            discover::discover_cancellable(&settings.source, settings.recursive, &self.stop);
        if discovery.inputs.is_empty() {
            let notice = if discovery.limited {
                "Video scan limit reached: choose a narrower folder or disable recursion".to_owned()
            } else if settings.source.trim().is_empty() {
                "No video source set: choose a file, folder or URL".to_owned()
            } else {
                format!("No videos found: {}", settings.source)
            };
            self.shared.set_notice(Some(notice));
        }
        self.scheduler.set_entries(discovery.inputs);
    }

    fn play(&mut self, item: &PlayItem, (columns, rows): (u16, u16)) -> Outcome {
        if self.stopping() {
            return Outcome::Stopped;
        }
        let request = PlayRequest {
            input: item.input.clone(),
            start_seconds: item.start_seconds,
            limit_seconds: item.limit_seconds,
            dots_width: u32::from(columns) * 2,
            dots_height: u32::from(rows) * 4,
        };
        let mut stream = match self.source.open(&request) {
            Ok(stream) => stream,
            Err(OpenError::ToolMissing(program)) => {
                return Outcome::Failed(format!("{program} not found: install ffmpeg"));
            }
            Err(OpenError::Failed(message)) => return Outcome::Failed(message),
        };
        self.segment += 1;
        let playing = Arc::new(NowPlaying {
            name: item.input.display_name(),
            duration_seconds: item.duration_seconds,
        });
        let options = self.config.options.clone();
        let ratio = options.speed_ratio();
        let started = Instant::now();
        let mut buffer: Vec<u8> = Vec::new();
        let mut frames: u64 = 0;
        loop {
            if self.stopping() {
                return Outcome::Stopped;
            }
            if self.shared.size() != Some((columns, rows)) {
                return self.resize_outcome(item, frames, ratio);
            }
            match stream.read_frame(&mut buffer) {
                Ok(true) => {}
                Ok(false) => {
                    return Outcome::Finished {
                        frames,
                        elapsed: started.elapsed(),
                    }
                }
                Err(error) => return Outcome::Failed(format!("read error: {error}")),
            }
            let pts = frame_time(frames, options.frames_per_second);
            let frame = VideoFrame {
                segment: self.segment,
                playing: Arc::clone(&playing),
                pts,
                source_seconds: source_position(item.start_seconds, pts, ratio),
                width: request.dots_width as usize,
                height: request.dots_height as usize,
                format: options.pixel_format,
                data: std::mem::take(&mut buffer),
            };
            match self.push(frame, (columns, rows)) {
                Push::Sent => {}
                Push::Stop => return Outcome::Stopped,
                Push::Resized => return self.resize_outcome(item, frames, ratio),
            }
            if frames == 0 {
                self.shared.set_notice(None);
                self.failures = 0;
            }
            frames += 1;
        }
    }

    fn resize_outcome(&self, item: &PlayItem, frames: u64, ratio: f64) -> Outcome {
        let played = frame_time(frames, self.config.options.frames_per_second);
        let position = source_position(item.start_seconds, played, ratio);
        let remaining = item
            .limit_seconds
            .map(|limit| (limit - (position - item.start_seconds)).max(1.0));
        Outcome::Resized {
            resume: PlayItem {
                input: item.input.clone(),
                start_seconds: position,
                limit_seconds: remaining,
                duration_seconds: item.duration_seconds,
            },
        }
    }

    /// Queue a frame, waiting for room but staying responsive to stop and resize.
    fn push(&self, mut frame: VideoFrame, size: (u16, u16)) -> Push {
        loop {
            match self.sender.try_send(frame) {
                Ok(()) => return Push::Sent,
                Err(TrySendError::Disconnected(_)) => return Push::Stop,
                Err(TrySendError::Full(returned)) => frame = returned,
            }
            if self.stopping() {
                return Push::Stop;
            }
            if self.shared.size() != Some(size) {
                return Push::Resized;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
    }
}
