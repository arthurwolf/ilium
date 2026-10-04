//! `FrameSource` backed by the external ffmpeg and ffprobe programs.
//!
//! Every child is registered in the scene's `ChildSlot` so the scene's `Drop`
//! can kill it while the worker is blocked reading its pipe, and every child
//! is killed and reaped when its stream is dropped, so nothing lingers as a
//! zombie however a clip ends.

use super::command::{
    ffmpeg_spec, ffprobe_spec, parse_duration, ChildControl, ChildSlot, CommandRunner, CommandSpec,
    PlayRequest, SpawnedChild, StreamOptions, FFMPEG_PROGRAM,
};
use super::diagnostic::NativeRead;
use super::discover::MediaInput;
use super::player::{FrameSource, FrameStream, OpenError};
use super::ram::RamMedia;
use super::VideoSeries;
use crate::resources::AmbientResources;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A killed child is normally gone within milliseconds.
const REAP_TIMEOUT: Duration = Duration::from_secs(2);
/// Duration probes are cut off after this many bytes of output.
const PROBE_OUTPUT_LIMIT: u64 = 4096;

/// A successful separate probe already supplies the catalogue GIF duration.
/// FFmpeg's seekable GIF header parser otherwise scans the complete RAM body
/// again before decoding. Keep accurate `-ss` and every conversion option,
/// while telling only this decoder to stream the verified GIF representation.
fn catalogue_decoder_spec(
    series: VideoSeries,
    original: &MediaInput,
    request: &PlayRequest,
    options: &StreamOptions,
) -> CommandSpec {
    let mut spec = ffmpeg_spec(request, options);
    let MediaInput::Url(url) = original else {
        return spec;
    };
    if series != VideoSeries::Germination
        || !url
            .split('?')
            .next()
            .is_some_and(|path| path.to_ascii_lowercase().ends_with(".gif"))
        || super::series::entry(original).is_err()
    {
        return spec;
    }
    // All input options precede -i; ffmpeg_spec always supplies one input.
    if let Some(index) = spec.args.iter().position(|argument| argument == "-i") {
        spec.args.splice(
            index..index,
            [
                std::ffi::OsString::from("-seekable"),
                std::ffi::OsString::from("0"),
            ],
        );
    }
    spec
}

/// Input `-ss` may decode and discard an entire animated GIF before FFmpeg
/// writes its first raw frame. The player can instead consume completed raw
/// frames from zero, retaining its five-second watchdog per frame. This is
/// only valid for a resize of a catalogue GIF originally started at zero;
/// the player supplies its exact count of already published output frames.
fn sequential_gif_resume(
    series: VideoSeries,
    request: &PlayRequest,
    frames_published_before_resize: u64,
) -> Option<PlayRequest> {
    if series != VideoSeries::Germination
        || frames_published_before_resize == 0
        || !request.start_seconds.is_finite()
        || request.start_seconds <= 0.0
    {
        return None;
    }
    let super::discover::MediaInput::Url(url) = &request.input else {
        return None;
    };
    if !url
        .split('?')
        .next()
        .is_some_and(|path| path.to_ascii_lowercase().ends_with(".gif"))
        || super::series::entry(&request.input).is_err()
    {
        return None;
    }
    let total_limit = request.limit_seconds.map(|remaining| {
        // `-t` is before `-i`: it limits source input, so from-zero replay
        // needs the already-played prefix plus the remaining tail.
        request.start_seconds + remaining
    });
    if total_limit.is_some_and(|limit| !limit.is_finite() || limit <= 0.0) {
        return None;
    }
    Some(PlayRequest {
        start_seconds: 0.0,
        limit_seconds: total_limit,
        ..request.clone()
    })
}

pub struct FfmpegSource {
    runner: Arc<dyn CommandRunner>,
    slot: ChildSlot,
    options: StreamOptions,
    watchdog: Option<crate::source::Worker>,
    series: VideoSeries,
    prepared: Option<RamMedia>,
    resources: AmbientResources,
    // Only a verified successful probe, including N/A, is reusable.
    probed: Option<(MediaInput, Option<f64>)>,
}

impl FfmpegSource {
    pub fn new(
        runner: Arc<dyn CommandRunner>,
        slot: ChildSlot,
        options: StreamOptions,
        resources: AmbientResources,
    ) -> Self {
        Self {
            runner,
            slot,
            options,
            watchdog: None,
            series: VideoSeries::Custom,
            prepared: None,
            resources,
            probed: None,
        }
    }

    fn decoder_input(&self, input: &MediaInput) -> Result<MediaInput, OpenError> {
        if self.series == VideoSeries::Custom {
            return Ok(input.clone());
        }
        self.prepared
            .as_ref()
            .filter(|media| &media.source == input)
            .map(|media| media.endpoint.clone())
            .ok_or_else(|| OpenError::Failed("Remote video was not prepared in RAM".into()))
    }

    /// Physical leases survive native exit until the diagnostic worker joins.
    /// Wait briefly for that handoff instead of discarding the prepared clip.
    fn spawn_admitted_child(&self, spec: &CommandSpec) -> io::Result<SpawnedChild> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if self.slot.is_closed() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "Stopping"));
            }
            match self.runner.spawn(spec) {
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                result => return result,
            }
        }
    }

    fn start_watchdog(&mut self) -> io::Result<()> {
        if self.watchdog.is_none() {
            self.watchdog = Some(self.slot.watchdog(&self.resources)?);
        }
        Ok(())
    }

    fn open_with_resize_replay(
        &mut self,
        request: &PlayRequest,
        replayed_frames: Option<u64>,
    ) -> Result<Box<dyn FrameStream>, OpenError> {
        let frame_bytes = request
            .bounded_frame_bytes(self.options.pixel_format)
            .ok_or_else(|| {
                OpenError::Failed(
                    "Video output is too large (maximum 512 columns by 128 rows)".into(),
                )
            })?;
        self.start_watchdog().map_err(|error| {
            OpenError::Failed(format!("cannot start video timeout worker: {error}"))
        })?;
        let sequential = replayed_frames.and_then(|frames| {
            sequential_gif_resume(self.series, request, frames).map(|request| (request, frames))
        });
        let (request, preroll_frames) = sequential.unwrap_or_else(|| (request.clone(), 0));
        let original = request.input.clone();
        let request = PlayRequest {
            input: self.decoder_input(&request.input)?,
            ..request
        };
        let spec = catalogue_decoder_spec(self.series, &original, &request, &self.options);
        let child = self.spawn_admitted_child(&spec).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                OpenError::ToolMissing(FFMPEG_PROGRAM)
            } else {
                OpenError::Failed(format!("cannot start {FFMPEG_PROGRAM}: {error}"))
            }
        })?;
        if !self.slot.register(Arc::clone(&child.control)) {
            child.control.kill();
            child.control.reap(REAP_TIMEOUT);
            return Err(OpenError::Failed("stopping".to_owned()));
        }
        Ok(Box::new(FfmpegStream {
            stdout: child.stdout,
            control: child.control,
            slot: self.slot.clone(),
            frame_bytes,
            preroll_frames,
            last_read: NativeRead::default(),
        }))
    }
}

impl FrameSource for FfmpegSource {
    fn prepare(
        &mut self,
        series: VideoSeries,
        input: &MediaInput,
        stop: &AtomicBool,
    ) -> Result<(), OpenError> {
        self.series = series;
        if stop.load(Ordering::Acquire) {
            return Err(OpenError::Failed("Stopping".into()));
        }
        if series == VideoSeries::Custom {
            self.prepared = None;
            self.probed = None;
            return Ok(());
        }
        if self
            .prepared
            .as_ref()
            .is_some_and(|media| &media.source == input)
        {
            return Ok(());
        }
        self.prepared = None;
        self.probed = None;
        let prepared =
            RamMedia::acquire(input, stop, &self.resources).map_err(OpenError::Failed)?;
        if stop.load(Ordering::Acquire) {
            return Err(OpenError::Failed("Stopping".into()));
        }
        self.prepared = Some(prepared);
        Ok(())
    }

    fn probe_duration(&mut self, input: &MediaInput) -> Option<f64> {
        self.probe_duration_checked(input).ok().flatten()
    }

    fn probe_duration_checked(&mut self, source: &MediaInput) -> Result<Option<f64>, OpenError> {
        if self.series == VideoSeries::Germination {
            if let Some((input, duration)) = &self.probed {
                if input == source {
                    return Ok(*duration);
                }
            }
        }
        let input = self.decoder_input(source)?;
        self.start_watchdog().map_err(|error| {
            OpenError::Failed(format!("cannot start video timeout worker: {error}"))
        })?;
        let mut child = self.runner.spawn(&ffprobe_spec(&input)).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                OpenError::ToolMissing("ffprobe")
            } else {
                OpenError::Failed(format!("cannot start ffprobe: {error}"))
            }
        })?;
        if !self.slot.register(Arc::clone(&child.control)) {
            child.control.kill();
            child.control.reap(REAP_TIMEOUT);
            return Err(OpenError::Failed("Stopping".into()));
        }
        let mut output = String::new();
        let read = (&mut child.stdout)
            .take(PROBE_OUTPUT_LIMIT + 1)
            .read_to_string(&mut output);
        self.slot.arm(None);
        let timed_out = self.slot.timed_out();
        let exited = child.control.reap(Duration::from_millis(500));
        if !exited {
            child.control.kill();
            child.control.reap(REAP_TIMEOUT);
        }
        let successful = child.control.successful_exit();
        let diagnostic = child.control.diagnostic_tail();
        self.slot.clear();
        let detail = if diagnostic.is_empty() {
            String::new()
        } else {
            format!(": {diagnostic}")
        };
        if timed_out || !exited {
            return Err(OpenError::Failed(format!(
                "video duration probe timed out{detail}"
            )));
        }
        read.map_err(|error| {
            OpenError::Failed(format!("video duration probe read failed: {error}{detail}"))
        })?;
        if output.len() as u64 > PROBE_OUTPUT_LIMIT {
            return Err(OpenError::Failed(
                "video duration probe output exceeded limit".into(),
            ));
        }
        if successful != Some(true) {
            return Err(OpenError::Failed(format!(
                "video duration probe exited unsuccessfully{detail}"
            )));
        }
        // ffprobe's default=noprint_wrappers=1:nokey=1 prints N/A when the
        // format has no known duration. Every other successful output must be
        // exactly one positive finite numeric line before it can be cached.
        let value = output.trim();
        let duration = if value == "N/A" {
            None
        } else if value.is_empty() || value.contains(['\r', '\n']) {
            return Err(OpenError::Failed(
                "video duration probe returned malformed output".into(),
            ));
        } else {
            Some(parse_duration(value).ok_or_else(|| {
                OpenError::Failed("video duration probe returned malformed output".into())
            })?)
        };
        if self.series == VideoSeries::Germination {
            self.probed = Some((source.clone(), duration));
        }
        Ok(duration)
    }

    fn open(&mut self, request: &PlayRequest) -> Result<Box<dyn FrameStream>, OpenError> {
        self.open_with_resize_replay(request, None)
    }

    fn open_after_resize(
        &mut self,
        request: &PlayRequest,
        frames_published_before_resize: u64,
    ) -> Result<Box<dyn FrameStream>, OpenError> {
        self.open_with_resize_replay(request, Some(frames_published_before_resize))
    }
}

pub struct FfmpegStream {
    stdout: Box<dyn Read + Send>,
    control: Arc<dyn ChildControl>,
    slot: ChildSlot,
    frame_bytes: usize,
    preroll_frames: u64,
    last_read: NativeRead,
}

impl FrameStream for FfmpegStream {
    fn preroll_frames(&self) -> u64 {
        self.preroll_frames
    }

    fn native_read(&self) -> Option<NativeRead> {
        Some(self.last_read)
    }

    fn read_frame(&mut self, buffer: &mut Vec<u8>) -> io::Result<bool> {
        let began = Instant::now();
        self.last_read = NativeRead {
            expected: self.frame_bytes,
            ..NativeRead::default()
        };
        let result = self.read_frame_observed(buffer);
        self.last_read.elapsed_ms = began.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        self.last_read.slot_closed = self.slot.is_closed();
        result
    }
}

impl FfmpegStream {
    fn read_frame_observed(&mut self, buffer: &mut Vec<u8>) -> io::Result<bool> {
        if self.frame_bytes > buffer.len() {
            buffer
                .try_reserve_exact(self.frame_bytes - buffer.len())
                .map_err(|error| {
                    io::Error::other(format!("Cannot allocate video frame: {error}"))
                })?;
        }
        buffer.resize(self.frame_bytes, 0);
        self.slot.arm(Some(Duration::from_secs(5)));
        let mut received = 0;
        let read = loop {
            if received == self.frame_bytes {
                break Ok(());
            }
            match self.stdout.read(&mut buffer[received..]) {
                Ok(0) => break Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                Ok(count) => received += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => break Err(error),
            }
        };
        self.slot.arm(None);
        self.last_read.received = received;
        self.last_read.read_error = read.as_ref().err().map(io::Error::kind);
        self.last_read.watchdog_expired = self.slot.timed_out();
        if self.last_read.watchdog_expired {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "video decoder stalled for 5 seconds",
            ));
        }
        match read {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                if received != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        format!(
                            "Truncated video frame: received {received} of {} bytes",
                            self.frame_bytes
                        ),
                    ));
                }
                let reaped = self.control.reap(Duration::from_millis(500));
                self.last_read.eof_reaped = Some(reaped);
                if !reaped {
                    return Err(io::Error::other(
                        "Video decoder closed its output but did not exit",
                    ));
                }
                let exit = self.control.successful_exit();
                self.last_read.exit_success = exit;
                if exit != Some(true) {
                    let tail = self.control.diagnostic_tail();
                    let notice = if exit == Some(false) {
                        "Video decoder exited unsuccessfully"
                    } else {
                        "Video decoder exit was not verified successful"
                    };
                    return Err(io::Error::other(if tail.is_empty() {
                        notice.to_owned()
                    } else {
                        format!("{notice}: {tail}")
                    }));
                }
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
}

impl Drop for FfmpegStream {
    fn drop(&mut self) {
        self.control.kill();
        self.control.reap(REAP_TIMEOUT);
        self.slot.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_gif_resize_replays_prefix_and_retains_source_time_limit() {
        let request = PlayRequest {
            input: super::super::series::inputs()
                .unwrap()
                .into_iter()
                .find(|input| matches!(input, MediaInput::Url(url) if url.contains(".gif")))
                .unwrap(),
            start_seconds: 4.333_333,
            limit_seconds: Some(3.0),
            dots_width: 320,
            dots_height: 196,
        };
        let replay = sequential_gif_resume(VideoSeries::Germination, &request, 65)
            .expect("a catalogued GIF resize can replay from zero");
        assert_eq!(replay.input, request.input);
        assert_eq!(replay.start_seconds, 0.0);
        assert!((replay.limit_seconds.unwrap() - 7.333_333).abs() < 1e-9);
        let options = StreamOptions {
            frames_per_second: 24,
            slowed_percent: Some(5),
            fit: super::super::settings::FitMode::Fill,
            pixel_format: super::super::command::PixelFormat::Rgb24,
            detail: 75,
        };
        let args: Vec<String> = ffmpeg_spec(&replay, &options)
            .args
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        assert!(!args.iter().any(|argument| argument == "-ss"));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "-t" && pair[1] == "7.333"));
        let expected_filter = super::super::command::filter_chain(320, 196, &options);
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "-vf" && pair[1] == expected_filter));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "-pix_fmt" && pair[1] == "rgb24"));
        assert!(sequential_gif_resume(VideoSeries::Custom, &request, 65).is_none());
        assert!(sequential_gif_resume(VideoSeries::Germination, &request, 0).is_none());
        let webm = PlayRequest {
            input: MediaInput::Url("https://example.invalid/clip.webm".into()),
            ..request.clone()
        };
        assert!(sequential_gif_resume(VideoSeries::Germination, &webm, 65).is_none());
    }

    #[test]
    fn verified_gif_decoder_streams_while_preserving_accurate_seek_and_conversion() {
        let inputs = super::super::series::inputs().unwrap();
        let gif = inputs
            .iter()
            .find(|input| {
                matches!(input,
                    MediaInput::Url(url) if url.split('?').next().unwrap().ends_with(".gif")
                )
            })
            .unwrap();
        let options = StreamOptions {
            frames_per_second: 24,
            slowed_percent: Some(5),
            fit: super::super::settings::FitMode::Fill,
            pixel_format: super::super::command::PixelFormat::Rgb24,
            detail: 75,
        };
        let request = PlayRequest {
            input: MediaInput::Url("http://127.0.0.1:12345/owned-test-body".into()),
            start_seconds: 0.13,
            limit_seconds: Some(3.0),
            dots_width: 100,
            dots_height: 48,
        };
        let expected = ffmpeg_spec(&request, &options);
        let mut actual = catalogue_decoder_spec(VideoSeries::Germination, gif, &request, &options);
        let index = actual
            .args
            .iter()
            .position(|argument| argument == "-seekable")
            .unwrap();
        assert_eq!(actual.args[index + 1], "0");
        assert_eq!(actual.args[index + 2], "-i", "decoder-only input option");
        actual.args.drain(index..index + 2);
        assert_eq!(
            actual, expected,
            "seek, source limit, conversion and budgets are unchanged"
        );
        assert_eq!(
            catalogue_decoder_spec(VideoSeries::Custom, gif, &request, &options),
            expected
        );
        let unknown = MediaInput::Url("https://example.invalid/unknown.gif".into());
        assert_eq!(
            catalogue_decoder_spec(VideoSeries::Germination, &unknown, &request, &options),
            expected
        );
        let webm = inputs
            .iter()
            .find(|input| matches!(input, MediaInput::Url(url) if url.contains(".webm")))
            .unwrap();
        assert_eq!(
            catalogue_decoder_spec(VideoSeries::Germination, webm, &request, &options),
            expected
        );
        assert!(
            !ffprobe_spec(&request.input)
                .args
                .iter()
                .any(|argument| argument == "-seekable"),
            "duration probe keeps seekability"
        );
    }

    #[test]
    fn decoder_failure_at_eof_is_reported_instead_of_clean_completion() {
        struct FailedChild;
        impl ChildControl for FailedChild {
            fn kill(&self) {}
            fn reap(&self, _: Duration) -> bool {
                true
            }
            fn successful_exit(&self) -> Option<bool> {
                Some(false)
            }
        }
        let mut stream = FfmpegStream {
            stdout: Box::new(io::Cursor::new(Vec::<u8>::new())),
            control: Arc::new(FailedChild),
            slot: ChildSlot::default(),
            frame_bytes: 4,
            preroll_frames: 0,
            last_read: NativeRead::default(),
        };
        assert!(stream
            .read_frame(&mut Vec::new())
            .unwrap_err()
            .to_string()
            .contains("exited unsuccessfully"));
    }

    #[test]
    fn partial_frame_is_failure_even_when_native_exit_was_successful() {
        struct SuccessfulChild;
        impl ChildControl for SuccessfulChild {
            fn kill(&self) {}
            fn reap(&self, _: Duration) -> bool {
                true
            }
            fn successful_exit(&self) -> Option<bool> {
                Some(true)
            }
        }
        let mut stream = FfmpegStream {
            stdout: Box::new(io::Cursor::new(vec![1u8, 2])),
            control: Arc::new(SuccessfulChild),
            slot: ChildSlot::default(),
            frame_bytes: 4,
            preroll_frames: 0,
            last_read: NativeRead::default(),
        };
        let error = stream
            .read_frame(&mut Vec::new())
            .expect_err("a partial frame cannot be a clean EOF");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn unverified_native_exit_cannot_be_a_clean_eof() {
        struct UnknownChild;
        impl ChildControl for UnknownChild {
            fn kill(&self) {}
            fn reap(&self, _: Duration) -> bool {
                true
            }
        }
        let mut stream = FfmpegStream {
            stdout: Box::new(io::Cursor::new(Vec::<u8>::new())),
            control: Arc::new(UnknownChild),
            slot: ChildSlot::default(),
            frame_bytes: 4,
            preroll_frames: 0,
            last_read: NativeRead::default(),
        };
        assert!(stream
            .read_frame(&mut Vec::new())
            .unwrap_err()
            .to_string()
            .contains("not verified successful"));
    }

    #[test]
    fn verified_unknown_duration_differs_from_failed_probe_and_is_reusable() {
        use super::super::command::{CommandSpec, SpawnedChild};
        use super::super::settings::VideoSettings;
        use std::sync::atomic::AtomicUsize;
        struct ProbeControl(bool);
        impl ChildControl for ProbeControl {
            fn kill(&self) {}
            fn reap(&self, _: Duration) -> bool {
                true
            }
            fn successful_exit(&self) -> Option<bool> {
                Some(self.0)
            }
        }
        struct ProbeRunner {
            stdout: &'static [u8],
            success: bool,
            calls: Arc<AtomicUsize>,
        }
        impl CommandRunner for ProbeRunner {
            fn spawn(&self, _: &CommandSpec) -> io::Result<SpawnedChild> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(SpawnedChild {
                    stdout: Box::new(io::Cursor::new(self.stdout.to_vec())),
                    control: Arc::new(ProbeControl(self.success)),
                })
            }
        }
        let input = MediaInput::File("synthetic.mov".into());
        let options = super::super::render::stream_options(&VideoSettings::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = FfmpegSource::new(
            Arc::new(ProbeRunner {
                stdout: b"N/A\n",
                success: true,
                calls: Arc::clone(&calls),
            }),
            ChildSlot::default(),
            options.clone(),
            crate::resources::test_resources(),
        );
        assert!(matches!(source.probe_duration_checked(&input), Ok(None)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // A successful unknown duration is a valid cached outcome for the
        // prepared representation and does not run a second native probe.
        source.series = VideoSeries::Germination;
        source.probed = Some((input.clone(), None));
        assert!(matches!(source.probe_duration_checked(&input), Ok(None)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let failed_calls = Arc::new(AtomicUsize::new(0));
        let mut failed = FfmpegSource::new(
            Arc::new(ProbeRunner {
                stdout: b"12.0\n",
                success: false,
                calls: Arc::clone(&failed_calls),
            }),
            ChildSlot::default(),
            options.clone(),
            crate::resources::test_resources(),
        );
        assert!(matches!(
            failed.probe_duration_checked(&input),
            Err(OpenError::Failed(_))
        ));
        assert!(failed.probed.is_none());
        assert_eq!(failed_calls.load(Ordering::SeqCst), 1);

        for malformed_output in [
            b"".as_slice(),
            b"NaN\n",
            b"-1\n",
            b"N/A\n12\n",
            b"garbage\n",
        ] {
            let malformed_calls = Arc::new(AtomicUsize::new(0));
            let mut malformed = FfmpegSource::new(
                Arc::new(ProbeRunner {
                    stdout: malformed_output,
                    success: true,
                    calls: Arc::clone(&malformed_calls),
                }),
                ChildSlot::default(),
                options.clone(),
                crate::resources::test_resources(),
            );
            assert!(matches!(malformed.probe_duration_checked(&input),
                Err(OpenError::Failed(message)) if message.contains("malformed output")));
            assert!(malformed.probed.is_none());
            assert_eq!(malformed_calls.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    #[ignore = "downloads three real catalogue media sources; requires ffmpeg and network"]
    fn real_remote_germination_ram_probe_seek_resize_and_reuse() {
        use super::super::command::{PixelFormat, SystemRunner};
        use super::super::settings::{FitMode, VideoSettings};
        use super::super::{render, series};
        let inputs = series::inputs().unwrap();
        let selected = [
            inputs.iter().find(|input| matches!(input, MediaInput::Url(url) if url.contains("Growing_Plant_vimeo"))).unwrap(),
            inputs.iter().find(|input| matches!(input, MediaInput::Url(url) if url.contains(".gif"))).unwrap(),
            inputs.iter().find(|input| matches!(input, MediaInput::Url(url) if url.ends_with(".mov"))).unwrap(),
        ];
        for input in selected {
            let options = render::stream_options(&VideoSettings::default());
            let mut source = FfmpegSource::new(
                Arc::new(SystemRunner::new(crate::resources::test_resources())),
                ChildSlot::default(),
                options,
                crate::resources::test_resources(),
            );
            let stop = AtomicBool::new(false);
            source
                .prepare(VideoSeries::Germination, input, &stop)
                .unwrap();
            let endpoint = source.prepared.as_ref().unwrap().endpoint.clone();
            assert!(
                matches!(&endpoint, MediaInput::Url(url) if url.starts_with("http://127.0.0.1:"))
            );
            let duration = source
                .probe_duration_checked(input)
                .unwrap_or_else(|error| panic!("verified duration probe of {input:?}: {error:?}"));
            if !matches!(input, MediaInput::Url(url) if url.contains(".gif")) {
                assert!(duration.is_some(), "timed video duration of {input:?}");
            }
            assert_eq!(
                source.probe_duration_checked(input).unwrap(),
                duration,
                "the prepared representation reuses the verified probe outcome"
            );
            let mut cases = vec![
                (PixelFormat::Gray, FitMode::Fit, None, 80, 40, 0.0),
                (PixelFormat::Rgb24, FitMode::Fill, Some(50), 100, 48, 1.0),
                (PixelFormat::Gray, FitMode::Stretch, None, 120, 60, 2.0),
            ];
            if matches!(input, MediaInput::Url(url) if url.contains(".gif")) {
                // The smallest supported speed exposes source/filter phase
                // differences that a normal-speed first-frame check can miss.
                cases.push((PixelFormat::Rgb24, FitMode::Fill, Some(5), 100, 48, 4.3));
            }
            for (format, fit, speed, width, height, seek) in cases {
                source.options.pixel_format = format;
                source.options.fit = fit;
                source.options.slowed_percent = speed;
                source
                    .prepare(VideoSeries::Germination, input, &stop)
                    .unwrap();
                assert_eq!(
                    source.prepared.as_ref().unwrap().endpoint,
                    endpoint,
                    "reuse avoids a second remote download"
                );
                let request = PlayRequest {
                    input: input.clone(),
                    start_seconds: seek,
                    limit_seconds: Some(1.0),
                    dots_width: width,
                    dots_height: height,
                };
                let mut stream = source.open(&request).unwrap();
                let mut frame = Vec::new();
                for frame_index in 0..3 {
                    assert!(
                        stream.read_frame(&mut frame).unwrap_or_else(|error| {
                            panic!("remote RAM decode {input:?}, format={format:?}, fit={fit:?}, speed={speed:?}, dimensions={width}x{height}, seek={seek}, frame_index={frame_index}, native_read={:?}: {error}", stream.native_read())
                        }),
                        "decoded frame for {input:?}"
                    );
                    assert_eq!(frame.len(), request.bounded_frame_bytes(format).unwrap());
                    println!(
                        "{}",
                        serde_json::json!({
                            "type": "result",
                            "gate": "remote_ram_frame_read",
                            "input": format!("{input:?}"),
                            "format": format!("{format:?}"),
                            "fit": format!("{fit:?}"),
                            "slowed_percent": speed,
                            "seek_seconds": seek,
                            "frame_index": frame_index,
                            "native_read": format!("{:?}", stream.native_read()),
                        })
                    );
                }
                drop(stream);
                if matches!(input, MediaInput::Url(url) if url.contains(".gif")) {
                    // Compare the actual resize entry point with uninterrupted
                    // decoding at the new geometry, preserving the original
                    // filter phase for both normal and slowed playback.
                    let frames_before_resize = 65;
                    let uninterrupted_request = PlayRequest {
                        start_seconds: 0.0,
                        limit_seconds: None,
                        ..request.clone()
                    };
                    let mut uninterrupted = source.open(&uninterrupted_request).unwrap();
                    let mut expected = Vec::new();
                    for _ in 0..=frames_before_resize {
                        assert!(uninterrupted.read_frame(&mut expected).unwrap());
                    }
                    drop(uninterrupted);
                    let resumed_request = PlayRequest {
                        start_seconds: frames_before_resize as f64
                            / f64::from(source.options.frames_per_second)
                            * source.options.speed_ratio(),
                        ..request.clone()
                    };
                    let mut resumed = source
                        .open_after_resize(&resumed_request, frames_before_resize)
                        .unwrap();
                    assert_eq!(resumed.preroll_frames(), frames_before_resize);
                    let mut actual = Vec::new();
                    for _ in 0..=frames_before_resize {
                        assert!(resumed.read_frame(&mut actual).unwrap());
                    }
                    assert_eq!(actual, expected, "exact remote GIF resize frame");
                    assert_eq!(
                        source.prepared.as_ref().unwrap().endpoint,
                        endpoint,
                        "resize replay retains the same RAM body"
                    );
                    println!(
                        "{}",
                        serde_json::json!({
                            "type": "result",
                            "gate": "remote_gif_resize_frame_equivalence",
                            "format": format!("{format:?}"),
                            "fit": format!("{fit:?}"),
                            "slowed_percent": speed,
                            "dimensions": [width, height],
                            "preroll_frames": frames_before_resize,
                            "frame_bytes": actual.len(),
                            "exact_bytes_equal": true,
                        })
                    );
                    drop(resumed);
                }
            }
            let ticket = source.prepared.as_ref().unwrap().join_observer();
            drop(source);
            if let Some(ticket) = ticket {
                ticket
                    .join_until(std::time::Instant::now() + Duration::from_secs(3))
                    .unwrap();
            }
        }
    }
}

#[cfg(test)]
mod admission_retry_tests {
    use super::super::command::{CommandSpec, SpawnedChild};
    use super::super::settings::VideoSettings;
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{mpsc, Mutex};

    struct SuccessfulChild;
    impl ChildControl for SuccessfulChild {
        fn kill(&self) {}
        fn reap(&self, _: Duration) -> bool {
            true
        }
        fn successful_exit(&self) -> Option<bool> {
            Some(true)
        }
    }

    struct RetiringRunner {
        retired: Arc<AtomicBool>,
        denied: Mutex<Option<mpsc::Sender<()>>>,
        calls: Mutex<Vec<CommandSpec>>,
    }
    impl CommandRunner for RetiringRunner {
        fn spawn(&self, spec: &CommandSpec) -> io::Result<SpawnedChild> {
            self.calls.lock().unwrap().push(spec.clone());
            if !self.retired.load(Ordering::Acquire) {
                if let Some(sender) = self.denied.lock().unwrap().take() {
                    sender.send(()).unwrap();
                }
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "old diagnostic lease still held",
                ));
            }
            Ok(SpawnedChild {
                stdout: Box::new(io::Cursor::new(vec![42_u8; 4])),
                control: Arc::new(SuccessfulChild),
            })
        }
    }
    fn request() -> PlayRequest {
        PlayRequest {
            input: MediaInput::Url("http://127.0.0.1:12345/synthetic-prepared-endpoint".into()),
            start_seconds: 0.0,
            limit_seconds: None,
            dots_width: 2,
            dots_height: 2,
        }
    }
    fn source(runner: Arc<dyn CommandRunner>, slot: ChildSlot) -> FfmpegSource {
        FfmpegSource::new(
            runner,
            slot,
            super::super::render::stream_options(&VideoSettings::default()),
            crate::resources::test_resources(),
        )
    }

    #[test]
    fn decoder_open_retries_after_explicit_diagnostic_retirement_with_same_input() {
        let retired = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let runner = Arc::new(RetiringRunner {
            retired: Arc::clone(&retired),
            denied: Mutex::new(Some(sender)),
            calls: Mutex::new(Vec::new()),
        });
        let retirement = std::thread::spawn(move || {
            receiver.recv_timeout(Duration::from_secs(2)).unwrap();
            retired.store(true, Ordering::Release);
        });
        let mut source = source(runner.clone(), ChildSlot::default());
        let result = source.open(&request());
        retirement.join().unwrap();
        let mut stream = result.unwrap_or_else(|error| {
            panic!("valid clip lost during diagnostic retirement: {error:?}")
        });
        let calls = runner.calls.lock().unwrap();
        assert!(calls.len() >= 2, "first denial must be retried");
        assert!(
            calls.windows(2).all(|pair| pair[0] == pair[1]),
            "retry must preserve input, seek, dimensions and conversion"
        );
        let mut frame = Vec::new();
        assert!(stream.read_frame(&mut frame).unwrap());
        assert_eq!(frame, vec![42_u8; 4]);
    }

    struct DeniedRunner {
        slot: ChildSlot,
        close_on_denial: bool,
        kind: io::ErrorKind,
        calls: AtomicUsize,
    }
    impl CommandRunner for DeniedRunner {
        fn spawn(&self, _: &CommandSpec) -> io::Result<SpawnedChild> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.close_on_denial {
                self.slot.close();
            }
            Err(io::Error::new(
                self.kind,
                "host admission still unavailable",
            ))
        }
    }
    fn denied_runner(slot: ChildSlot, close: bool, kind: io::ErrorKind) -> Arc<DeniedRunner> {
        Arc::new(DeniedRunner {
            slot,
            close_on_denial: close,
            kind,
            calls: AtomicUsize::new(0),
        })
    }

    #[test]
    fn decoder_admission_retry_stops_when_scene_closes() {
        let slot = ChildSlot::default();
        let runner = denied_runner(slot.clone(), true, io::ErrorKind::WouldBlock);
        let mut source = source(runner.clone(), slot);
        assert!(
            matches!(source.open(&request()), Err(OpenError::Failed(message)) if message.contains("Stopping"))
        );
        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn closed_scene_does_not_attempt_native_admission() {
        let slot = ChildSlot::default();
        slot.close();
        let runner = denied_runner(slot.clone(), false, io::ErrorKind::WouldBlock);
        let mut source = source(runner.clone(), slot);
        assert!(
            matches!(source.open(&request()), Err(OpenError::Failed(message)) if message.contains("Stopping"))
        );
        assert_eq!(runner.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn permanent_decoder_admission_denial_is_bounded_and_reported() {
        let slot = ChildSlot::default();
        let runner = denied_runner(slot.clone(), false, io::ErrorKind::WouldBlock);
        let mut source = source(runner.clone(), slot);
        let start = std::time::Instant::now();
        assert!(
            matches!(source.open(&request()), Err(OpenError::Failed(message)) if message.contains("host admission still unavailable"))
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "permanent denial cannot hang playback"
        );
        assert!(
            runner.calls.load(Ordering::SeqCst) >= 2,
            "permanent denial must be retried before reporting exhaustion"
        );
    }

    #[test]
    fn missing_decoder_is_not_retried() {
        let slot = ChildSlot::default();
        let runner = denied_runner(slot.clone(), false, io::ErrorKind::NotFound);
        let mut source = source(runner.clone(), slot);
        assert!(matches!(
            source.open(&request()),
            Err(OpenError::ToolMissing("ffmpeg"))
        ));
        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn decoder_started_during_cancellation_is_killed_and_reaped() {
        struct CancelledChild {
            kills: AtomicUsize,
            reaps: AtomicUsize,
        }
        impl ChildControl for CancelledChild {
            fn kill(&self) {
                self.kills.fetch_add(1, Ordering::SeqCst);
            }
            fn reap(&self, _: Duration) -> bool {
                self.reaps.fetch_add(1, Ordering::SeqCst);
                true
            }
        }
        struct LateRunner {
            slot: ChildSlot,
            child: Arc<CancelledChild>,
            calls: AtomicUsize,
        }
        impl CommandRunner for LateRunner {
            fn spawn(&self, _: &CommandSpec) -> io::Result<SpawnedChild> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.slot.close();
                Ok(SpawnedChild {
                    stdout: Box::new(io::Cursor::new(Vec::<u8>::new())),
                    control: self.child.clone(),
                })
            }
        }
        let slot = ChildSlot::default();
        let child = Arc::new(CancelledChild {
            kills: AtomicUsize::new(0),
            reaps: AtomicUsize::new(0),
        });
        let runner = Arc::new(LateRunner {
            slot: slot.clone(),
            child: child.clone(),
            calls: AtomicUsize::new(0),
        });
        let mut source = source(runner.clone(), slot);
        assert!(
            matches!(source.open(&request()), Err(OpenError::Failed(message)) if message.eq_ignore_ascii_case("stopping"))
        );
        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(child.kills.load(Ordering::SeqCst), 1);
        assert_eq!(child.reaps.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct ObservedChild {
        exit: Option<bool>,
        reaped: bool,
        kills: AtomicUsize,
        reaps: AtomicUsize,
        exits: AtomicUsize,
    }
    impl ChildControl for ObservedChild {
        fn kill(&self) {
            self.kills.fetch_add(1, Ordering::SeqCst);
        }
        fn reap(&self, timeout: Duration) -> bool {
            assert!(timeout == Duration::from_millis(500) || timeout == REAP_TIMEOUT);
            self.reaps.fetch_add(1, Ordering::SeqCst);
            self.reaped
        }
        fn successful_exit(&self) -> Option<bool> {
            self.exits.fetch_add(1, Ordering::SeqCst);
            self.exit
        }
    }
    fn child(exit: Option<bool>, reaped: bool) -> Arc<ObservedChild> {
        Arc::new(ObservedChild {
            exit,
            reaped,
            kills: AtomicUsize::new(0),
            reaps: AtomicUsize::new(0),
            exits: AtomicUsize::new(0),
        })
    }
    fn stream(bytes: Vec<u8>, control: Arc<ObservedChild>) -> FfmpegStream {
        FfmpegStream {
            stdout: Box::new(io::Cursor::new(bytes)),
            control,
            slot: ChildSlot::default(),
            frame_bytes: 4,
            preroll_frames: 0,
            last_read: NativeRead::default(),
        }
    }

    #[test]
    fn diagnostic_eof_distinguishes_success_failure_unknown_and_unreaped() {
        for (exit, reaped, clean) in [
            (Some(true), true, true),
            (Some(false), true, false),
            (None, true, false),
            (Some(true), false, false),
        ] {
            let child = child(exit, reaped);
            let mut stream = stream(vec![1, 2, 3, 4], child.clone());
            let mut frame = Vec::new();
            assert!(stream.read_frame(&mut frame).unwrap());
            assert_eq!(frame, [1, 2, 3, 4]);
            let full = stream.native_read().unwrap();
            assert_eq!(
                (full.expected, full.received, full.read_error),
                (4, 4, None)
            );
            assert_eq!((full.eof_reaped, full.exit_success), (None, None));
            assert_eq!(child.reaps.load(Ordering::SeqCst), 0);
            let result = stream.read_frame(&mut frame);
            assert_eq!(matches!(result, Ok(false)), clean);
            if !clean {
                assert!(result.is_err());
            }
            let eof = stream.native_read().unwrap();
            assert_eq!((eof.expected, eof.received), (4, 0));
            assert_eq!(eof.read_error, Some(io::ErrorKind::UnexpectedEof));
            assert_eq!(eof.eof_reaped, Some(reaped));
            assert_eq!(eof.exit_success, if reaped { exit } else { None });
            assert!(!eof.watchdog_expired && !eof.slot_closed);
            // Observation neither retries reaping nor upgrades a missing status.
            assert_eq!(child.reaps.load(Ordering::SeqCst), 1);
            assert_eq!(child.exits.load(Ordering::SeqCst), usize::from(reaped));
            assert_eq!(child.kills.load(Ordering::SeqCst), 0);
            drop(stream);
            assert_eq!(child.kills.load(Ordering::SeqCst), 1);
            assert_eq!(child.reaps.load(Ordering::SeqCst), 2);
        }
    }

    #[test]
    fn diagnostic_partial_read_retains_error_and_observes_scene_close_without_extra_process_calls()
    {
        struct ClosingRead {
            slot: ChildSlot,
            close: bool,
            bytes: io::Cursor<Vec<u8>>,
        }
        impl Read for ClosingRead {
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                if self.close {
                    self.slot.close();
                    self.close = false;
                }
                self.bytes.read(output)
            }
        }
        for closing in [false, true] {
            let child = child(Some(true), true);
            let slot = ChildSlot::default();
            assert!(slot.register(child.clone()));
            let mut stream = FfmpegStream {
                stdout: Box::new(ClosingRead {
                    slot: slot.clone(),
                    close: closing,
                    bytes: io::Cursor::new(vec![9, 8]),
                }),
                control: child.clone(),
                slot,
                frame_bytes: 4,
                preroll_frames: 0,
                last_read: NativeRead::default(),
            };
            let error = stream.read_frame(&mut Vec::new()).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
            assert_eq!(
                error.to_string(),
                "Truncated video frame: received 2 of 4 bytes"
            );
            let observation = stream.native_read().unwrap();
            assert_eq!((observation.expected, observation.received), (4, 2));
            assert_eq!(observation.slot_closed, closing);
            assert!(!observation.watchdog_expired);
            assert_eq!(
                (observation.eof_reaped, observation.exit_success),
                (None, None)
            );
            assert_eq!(child.reaps.load(Ordering::SeqCst), 0);
            assert_eq!(child.exits.load(Ordering::SeqCst), 0);
            assert_eq!(child.kills.load(Ordering::SeqCst), usize::from(closing));
            drop(stream);
            assert_eq!(child.reaps.load(Ordering::SeqCst), 1);
            assert_eq!(child.kills.load(Ordering::SeqCst), usize::from(closing) + 1);
        }
    }
}
