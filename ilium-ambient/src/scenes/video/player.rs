//! The worker-side playback loop: pick a clip, run a frame stream, hand
//! timestamped frames to the scene through a bounded queue, and recover from
//! every failure with a growing back-off instead of spinning.

use super::command::{PixelFormat, PlayRequest, StreamOptions};
use super::diagnostic::{Diagnostics, NativeRead};
use super::discover::{self, MediaInput};
use super::schedule::{frame_time, source_position, PlayItem, Scheduler};
use super::settings::VideoSettings;
use super::{series, VideoSeries};
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
    /// Acquire seekable remote media before any duration probe or decoder open.
    fn prepare(
        &mut self,
        series: VideoSeries,
        _input: &MediaInput,
        _stop: &AtomicBool,
    ) -> Result<(), OpenError> {
        if series == VideoSeries::Custom {
            Ok(())
        } else {
            Err(OpenError::Failed(
                "Remote RAM video preparation is unavailable".into(),
            ))
        }
    }
    /// Length of `input` in seconds when it can be determined.
    fn probe_duration(&mut self, input: &MediaInput) -> Option<f64>;
    /// Germination distinguishes a verified unknown duration from a failed
    /// native probe. Existing custom-source implementations retain the option.
    fn probe_duration_checked(&mut self, input: &MediaInput) -> Result<Option<f64>, OpenError> {
        Ok(self.probe_duration(input))
    }
    fn open(&mut self, request: &PlayRequest) -> Result<Box<dyn FrameStream>, OpenError>;
    /// A resize of a clip whose original start was zero may be able to replay
    /// native output from the beginning, discarding exactly the frames already
    /// published. Sources without that capability retain normal accurate seek.
    fn open_after_resize(
        &mut self,
        request: &PlayRequest,
        _frames_published_before_resize: u64,
    ) -> Result<Box<dyn FrameStream>, OpenError> {
        self.open(request)
    }
}

/// One clip in progress. Dropping it must release every resource, killing
/// and reaping a child process if there is one.
pub trait FrameStream: Send {
    /// Fill `buffer` with the next frame. `Ok(false)` at the end of the clip.
    fn read_frame(&mut self, buffer: &mut Vec<u8>) -> io::Result<bool>;
    /// Output frames to consume one at a time before publishing this stream.
    /// Each call to `read_frame` keeps its own native five-second watchdog.
    fn preroll_frames(&self) -> u64 {
        0
    }
    /// Passive facts from the last native read; fixtures need not have a child.
    fn native_read(&self) -> Option<NativeRead> {
        None
    }
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
    pub(super) diagnostics: Diagnostics,
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
    },
    /// Output size changed: restart at the same position.
    Resized {
        resume: PlayItem,
        frames: u64,
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

// Expected catalogue identity only, not a substitute for acquisition validation.
fn expected_digest(input: &MediaInput) -> Option<&'static str> {
    if !super::diagnostic::enabled() {
        return None;
    }
    series::entry(input)
        .ok()
        .map(|entry| entry.download_sha256.as_str())
}

impl Player {
    fn diagnose(&self, phase: &'static str, frames: u64, facts: impl std::fmt::Debug) {
        self.shared.diagnostics.emit(
            phase,
            (
                self.segment,
                self.stopping(),
                self.shared.size(),
                self.config.settings.repeat_one,
                frames,
                facts,
            ),
        );
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    fn run(mut self) {
        let mut resume: Option<PlayItem> = None;
        let mut frames_before_resize: u64 = 0;
        let mut original_start_seconds: f64 = 0.0;
        while !self.stopping() {
            let Some(size) = self.wait_for_size() else {
                return;
            };
            let resuming = resume.is_some();
            let item = match resume.take() {
                Some(item) => item,
                None => {
                    frames_before_resize = 0;
                    match self.next_item() {
                        Some(item) => {
                            original_start_seconds = item.start_seconds;
                            item
                        }
                        None => continue,
                    }
                }
            };
            self.diagnose(
                "play_begin",
                frames_before_resize,
                (
                    expected_digest(&item.input),
                    size,
                    item.start_seconds,
                    item.limit_seconds,
                    item.duration_seconds,
                    &self.config.options,
                ),
            );
            // A from-zero native stream has the same output-frame clock only
            // when this clip itself began at zero. Random excerpts starting
            // later retain FFmpeg's normal accurate seek.
            let replayed_frames =
                (resuming && original_start_seconds == 0.0 && frames_before_resize > 0)
                    .then_some(frames_before_resize);
            let played = self.play(&item, size, replayed_frames);
            self.diagnose(
                "play_returned_after_stream_drop",
                frames_before_resize,
                match &played {
                    Outcome::Finished { frames } => ("finished", *frames),
                    Outcome::Resized { frames, .. } => ("resized", *frames),
                    Outcome::Stopped => ("stopped", 0),
                    Outcome::Failed(_) => ("failed", 0),
                },
            );
            match played {
                Outcome::Stopped => return,
                Outcome::Resized {
                    resume: next,
                    frames,
                } => {
                    frames_before_resize = frames_before_resize.saturating_add(frames);
                    resume = Some(next);
                }
                Outcome::Finished { frames } => {
                    // A successful short tail after resize is not a short clip.
                    // Keep repeat-one and the prepared RAM body for that clip.
                    let frames = frames_before_resize.saturating_add(frames);
                    self.diagnose("finished_total", frames, expected_digest(&item.input));
                    let fast = frames < u64::from(self.config.options.frames_per_second);
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
        self.diagnose("mark_failed", 0, (self.failures, message.is_some()));
        tracing::warn!(
            reason = message
                .as_deref()
                .unwrap_or("clip delivered fewer than one second of frames"),
            diagnostic_scene = self.shared.diagnostics.id(),
            stopping = self.stopping(),
            repeat_one = self.config.settings.repeat_one,
            failures = self.failures,
            "Video playback backing off before advancing playlist"
        );
        self.scheduler.mark_failed();
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
        let series = self.config.settings.series;
        let stop = &self.stop;
        let shared = &self.shared;
        let item = self.scheduler.next_prepared(
            &mut |input, should_probe| {
                if series == VideoSeries::Germination {
                    shared.set_notice(Some(format!(
                        "Loading {}",
                        series::display_name(input).unwrap_or_else(|| input.display_name())
                    )));
                }
                shared.diagnostics.emit(
                    "prepare_begin",
                    (expected_digest(input), stop.load(Ordering::Acquire)),
                );
                let began = Instant::now();
                let prepared = source.prepare(series, input, stop);
                shared.diagnostics.emit(
                    "prepare_returned",
                    (
                        expected_digest(input),
                        began.elapsed(),
                        prepared.is_ok(),
                        stop.load(Ordering::Acquire),
                    ),
                );
                prepared.map_err(|error| match error {
                    OpenError::Failed(message) => message,
                    OpenError::ToolMissing(program) => {
                        format!("{program} not found: install ffmpeg")
                    }
                })?;
                if !should_probe {
                    return Ok(None);
                }
                if series == VideoSeries::Germination {
                    shared
                        .diagnostics
                        .emit("checked_probe_begin", expected_digest(input));
                    let began = Instant::now();
                    let probed = source.probe_duration_checked(input);
                    shared.diagnostics.emit(
                        "checked_probe_returned",
                        (
                            expected_digest(input),
                            began.elapsed(),
                            probed.as_ref().ok(),
                            stop.load(Ordering::Acquire),
                        ),
                    );
                    probed.map_err(|error| match error {
                        OpenError::Failed(message) => message,
                        OpenError::ToolMissing(program) => {
                            format!("{program} not found: install ffmpeg")
                        }
                    })
                } else {
                    Ok(source.probe_duration(input))
                }
            },
            series == VideoSeries::Germination,
        );
        let item = match item {
            Ok(item) => item,
            Err(message) => {
                self.fail(message);
                return None;
            }
        };
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
        if settings.series == VideoSeries::Germination {
            match series::inputs() {
                Ok(inputs) => self.scheduler.set_entries(inputs),
                Err(error) => {
                    self.scheduler.set_entries(Vec::new());
                    self.shared.set_notice(Some(error));
                }
            }
            return;
        }
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

    fn play(
        &mut self,
        item: &PlayItem,
        (columns, rows): (u16, u16),
        replayed_frames: Option<u64>,
    ) -> Outcome {
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
        let began = Instant::now();
        let opened = match replayed_frames {
            Some(frames) => self.source.open_after_resize(&request, frames),
            None => self.source.open(&request),
        };
        self.diagnose("open_returned", 0, (began.elapsed(), opened.is_ok()));
        let mut stream = match opened {
            Ok(stream) => stream,
            Err(OpenError::ToolMissing(program)) => {
                return Outcome::Failed(format!("{program} not found: install ffmpeg"));
            }
            Err(OpenError::Failed(message)) => return Outcome::Failed(message),
        };
        self.segment += 1;
        let playing = Arc::new(NowPlaying {
            name: if self.config.settings.series == VideoSeries::Germination {
                series::display_name(&item.input).unwrap_or_else(|| item.input.display_name())
            } else {
                item.input.display_name()
            },
            duration_seconds: item.duration_seconds,
        });
        let options = self.config.options.clone();
        let ratio = options.speed_ratio();
        let mut buffer: Vec<u8> = Vec::new();
        let mut frames: u64 = 0;
        let mut preroll_remaining = stream.preroll_frames();
        loop {
            if self.stopping() {
                return Outcome::Stopped;
            }
            if self.shared.size() != Some((columns, rows)) {
                return self.resize_outcome(item, frames, ratio);
            }
            let read = stream.read_frame(&mut buffer);
            match &read {
                Ok(false) => self.diagnose("read_eof", frames, stream.native_read()),
                Err(error) => {
                    self.diagnose("read_failed", frames, (error.kind(), stream.native_read()));
                }
                _ => {}
            }
            // A blocking read may finish after geometry or lifetime changes.
            // Retire its obsolete result before classifying the current clip.
            if self.stopping() {
                return Outcome::Stopped;
            }
            if self.shared.size() != Some((columns, rows)) {
                return self.resize_outcome(item, frames, ratio);
            }
            match read {
                Ok(true) => {}
                Ok(false) => return Outcome::Finished { frames },
                Err(error) => return Outcome::Failed(format!("read error: {error}")),
            }
            if preroll_remaining > 0 {
                preroll_remaining -= 1;
                continue;
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
            }
            frames += 1;
            if frames.is_multiple_of(u64::from(options.frames_per_second.max(1))) {
                self.diagnose("published_second", frames, stream.native_read());
            }
            if frames >= u64::from(options.frames_per_second) {
                self.failures = 0;
            }
        }
    }

    fn resize_outcome(&self, item: &PlayItem, frames: u64, ratio: f64) -> Outcome {
        let played = frame_time(frames, self.config.options.frames_per_second);
        let position = source_position(item.start_seconds, played, ratio);
        let remaining = item
            .limit_seconds
            .map(|limit| (limit - (position - item.start_seconds)).max(1.0));
        self.diagnose(
            "resize_seek",
            frames,
            (
                item.start_seconds,
                position,
                item.duration_seconds,
                remaining,
            ),
        );
        Outcome::Resized {
            frames,
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
        let began = Instant::now();
        let mut reported_wait = false;
        loop {
            if self.stopping() {
                return Push::Stop;
            }
            if self.shared.size() != Some(size) {
                return Push::Resized;
            }
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
            if !reported_wait && began.elapsed() >= Duration::from_millis(500) {
                self.diagnose("queue_wait", 0, (size, frame.pts, frame.source_seconds));
                reported_wait = true;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;

    struct UnusedSource;
    impl FrameSource for UnusedSource {
        fn probe_duration(&mut self, _: &MediaInput) -> Option<f64> {
            None
        }
        fn open(&mut self, _: &PlayRequest) -> Result<Box<dyn FrameStream>, OpenError> {
            Err(OpenError::Failed("publication-only fixture".into()))
        }
    }

    #[test]
    fn cancellation_or_resize_prevents_publication_even_when_queue_has_room() {
        for cancelled in [false, true] {
            let config = PlayerConfig::new(
                VideoSettings::default(),
                StreamOptions {
                    frames_per_second: 12,
                    slowed_percent: None,
                    fit: super::super::settings::FitMode::Fit,
                    pixel_format: PixelFormat::Gray,
                    detail: 0,
                },
                1,
            );
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let shared = Arc::new(SharedState::default());
            shared.set_size(if cancelled { 1 } else { 2 }, 1);
            let player = Player {
                scheduler: Scheduler::new(&config.settings, 1),
                config,
                source: Box::new(UnusedSource),
                sender,
                shared,
                stop: Arc::new(AtomicBool::new(cancelled)),
                scanned_at: None,
                segment: 0,
                failures: 0,
            };
            let frame = VideoFrame {
                segment: 1,
                playing: Arc::new(NowPlaying {
                    name: "synthetic".into(),
                    duration_seconds: None,
                }),
                pts: Duration::ZERO,
                source_seconds: 0.0,
                width: 2,
                height: 4,
                format: PixelFormat::Gray,
                data: vec![0; 8],
            };
            let result = player.push(frame, (1, 1));
            if cancelled {
                assert!(matches!(result, Push::Stop));
            } else {
                assert!(matches!(result, Push::Resized));
            }
            assert!(receiver.try_recv().is_err());
        }
    }

    #[derive(Clone)]
    struct TerminalReadSource {
        shared: Arc<SharedState>,
        stop: Arc<AtomicBool>,
        resize: bool,
        cancel: bool,
        eof: bool,
    }

    impl FrameSource for TerminalReadSource {
        fn probe_duration(&mut self, _: &MediaInput) -> Option<f64> {
            Some(30.0)
        }

        fn open(&mut self, request: &PlayRequest) -> Result<Box<dyn FrameStream>, OpenError> {
            assert_eq!((request.dots_width, request.dots_height), (40, 20));
            assert_eq!(request.start_seconds, 7.0);
            Ok(Box::new(self.clone()))
        }
    }

    impl FrameStream for TerminalReadSource {
        fn read_frame(&mut self, _: &mut Vec<u8>) -> io::Result<bool> {
            if self.resize {
                self.shared.set_size(30, 8);
            }
            if self.cancel {
                self.stop.store(true, Ordering::Release);
            }
            if self.eof {
                Ok(false)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "synthetic terminal read",
                ))
            }
        }
    }

    fn terminal_read_player(resize: bool, cancel: bool, eof: bool) -> (Player, PlayItem) {
        let config = PlayerConfig::new(
            VideoSettings::default(),
            StreamOptions {
                frames_per_second: 12,
                slowed_percent: None,
                fit: super::super::settings::FitMode::Fit,
                pixel_format: PixelFormat::Gray,
                detail: 0,
            },
            1,
        );
        let (sender, _) = std::sync::mpsc::sync_channel(1);
        let shared = Arc::new(SharedState::default());
        shared.set_size(20, 5);
        let stop = Arc::new(AtomicBool::new(false));
        let source = TerminalReadSource {
            shared: Arc::clone(&shared),
            stop: Arc::clone(&stop),
            resize,
            cancel,
            eof,
        };
        let player = Player {
            scheduler: Scheduler::new(&config.settings, 1),
            config,
            source: Box::new(source),
            sender,
            shared,
            stop,
            scanned_at: None,
            segment: 0,
            failures: 0,
        };
        let item = PlayItem {
            input: MediaInput::File(std::path::PathBuf::from("synthetic.mp4")),
            start_seconds: 7.0,
            limit_seconds: None,
            duration_seconds: Some(30.0),
        };
        (player, item)
    }

    #[test]
    fn resize_during_terminal_read_resumes_the_same_clip() {
        for eof in [false, true] {
            let (mut player, item) = terminal_read_player(true, false, eof);
            let Outcome::Resized { resume, frames } = player.play(&item, (20, 5), None) else {
                panic!("obsolete terminal read must resume the resized clip (eof={eof})");
            };
            assert_eq!(resume, item);
            assert_eq!(frames, 0);
        }
    }

    #[test]
    fn cancellation_during_terminal_read_stops_without_clip_failure() {
        for eof in [false, true] {
            let (mut player, item) = terminal_read_player(false, true, eof);
            assert!(
                matches!(player.play(&item, (20, 5), None), Outcome::Stopped),
                "obsolete terminal read must stop playback (eof={eof})"
            );
        }
    }

    #[test]
    fn unchanged_terminal_reads_keep_failure_and_completion() {
        let (mut failed_player, item) = terminal_read_player(false, false, false);
        assert!(matches!(
            failed_player.play(&item, (20, 5), None),
            Outcome::Failed(_)
        ));
        let (mut finished_player, item) = terminal_read_player(false, false, true);
        assert!(matches!(
            finished_player.play(&item, (20, 5), None),
            Outcome::Finished { frames: 0 }
        ));
    }

    #[derive(Clone, Copy)]
    enum PrerollInterruption {
        None,
        Resize,
        Stop,
    }

    struct PrerollSource {
        shared: Arc<SharedState>,
        stop: Arc<AtomicBool>,
        interruption: PrerollInterruption,
    }

    struct PrerollStream {
        shared: Arc<SharedState>,
        stop: Arc<AtomicBool>,
        interruption: PrerollInterruption,
        index: u8,
    }

    impl FrameSource for PrerollSource {
        fn probe_duration(&mut self, _: &MediaInput) -> Option<f64> {
            Some(10.0)
        }

        fn open(&mut self, _: &PlayRequest) -> Result<Box<dyn FrameStream>, OpenError> {
            Err(OpenError::Failed("resize replay was not selected".into()))
        }

        fn open_after_resize(
            &mut self,
            request: &PlayRequest,
            frames_published_before_resize: u64,
        ) -> Result<Box<dyn FrameStream>, OpenError> {
            assert_eq!(request.start_seconds, 0.3);
            assert_eq!(frames_published_before_resize, 3);
            Ok(Box::new(PrerollStream {
                shared: Arc::clone(&self.shared),
                stop: Arc::clone(&self.stop),
                interruption: self.interruption,
                index: 0,
            }))
        }
    }

    impl FrameStream for PrerollStream {
        fn preroll_frames(&self) -> u64 {
            3
        }

        fn read_frame(&mut self, buffer: &mut Vec<u8>) -> io::Result<bool> {
            if self.index == 4 {
                return Ok(false);
            }
            buffer.clear();
            buffer.resize(40 * 20, self.index);
            self.index += 1;
            if self.index == 1 {
                match self.interruption {
                    PrerollInterruption::None => {}
                    PrerollInterruption::Resize => self.shared.set_size(30, 8),
                    PrerollInterruption::Stop => self.stop.store(true, Ordering::Release),
                }
            }
            Ok(true)
        }
    }

    fn preroll_player(
        interruption: PrerollInterruption,
    ) -> (Player, PlayItem, std::sync::mpsc::Receiver<VideoFrame>) {
        let settings = VideoSettings::default();
        let config = PlayerConfig::new(
            settings.clone(),
            StreamOptions {
                frames_per_second: 10,
                slowed_percent: None,
                fit: super::super::settings::FitMode::Fit,
                pixel_format: PixelFormat::Gray,
                detail: 0,
            },
            1,
        );
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let shared = Arc::new(SharedState::default());
        shared.set_size(20, 5);
        let stop = Arc::new(AtomicBool::new(false));
        let source = PrerollSource {
            shared: Arc::clone(&shared),
            stop: Arc::clone(&stop),
            interruption,
        };
        let player = Player {
            config,
            source: Box::new(source),
            sender,
            shared,
            stop,
            scheduler: Scheduler::new(&settings, 1),
            scanned_at: None,
            segment: 0,
            failures: 0,
        };
        let item = PlayItem {
            input: MediaInput::File("synthetic.mp4".into()),
            start_seconds: 0.3,
            limit_seconds: None,
            duration_seconds: Some(10.0),
        };
        (player, item, receiver)
    }

    #[test]
    fn resize_preroll_only_publishes_the_frame_at_the_preserved_position() {
        let (mut player, item, receiver) = preroll_player(PrerollInterruption::None);
        assert!(matches!(
            player.play(&item, (20, 5), Some(3)),
            Outcome::Finished { frames: 1 }
        ));
        let frame = receiver
            .try_recv()
            .expect("the requested frame was published");
        assert_eq!(frame.data[0], 3);
        assert_eq!(frame.pts, Duration::ZERO);
        assert_eq!(frame.source_seconds, 0.3);
        assert!(receiver.try_recv().is_err(), "preroll was never presented");
    }

    #[test]
    fn resize_or_stop_during_preroll_retires_obsolete_frames() {
        let (mut resized, item, receiver) = preroll_player(PrerollInterruption::Resize);
        assert!(matches!(
            resized.play(&item, (20, 5), Some(3)),
            Outcome::Resized { frames: 0, .. }
        ));
        assert!(receiver.try_recv().is_err());

        let (mut stopped, item, receiver) = preroll_player(PrerollInterruption::Stop);
        assert!(matches!(
            stopped.play(&item, (20, 5), Some(3)),
            Outcome::Stopped
        ));
        assert!(receiver.try_recv().is_err());
    }
}
