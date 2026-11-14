//! The video decoder of client plugins (`ilium:host/media`).
//!
//! The import (`ilium_plugin_host::imports::media`) owns the contract: grants, scopes, the quota,
//! positions and ranges. This backend runs the programs the manual names: `ffprobe` describes a
//! file and `ffmpeg` decodes one run of it to packed `rgb8` frames on its standard output. Each
//! run is its own child process (SEC-139): it reads only the one local file (`file:` input,
//! `-protocol_whitelist file`, no standard input), its address space is bounded before it
//! starts, and a crash fails the plugin's call with `failed(decoder)` instead of taking the
//! client down. A reader thread decodes ahead into a bounded channel, so at most
//! `DecodeRequest::buffer_bytes` of frames wait for the plugin.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::host::media::VideoInfo;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_host::imports::media::{DecodeRequest, FrameSource, MediaBackend};

const FFMPEG_PROGRAM: &str = "ffmpeg";
const FFPROBE_PROGRAM: &str = "ffprobe";
/// Largest single allocation the decoder may make (`-max_alloc`), as the native video scene.
const DECODER_MAX_ALLOC: &str = "134217728";
/// Address space of one decoder process.
const DECODER_ADDRESS_SPACE_BYTES: usize = 1024 * 1024 * 1024;
/// How long `ffprobe` may take to describe a file.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long `next-frame` waits for the decoder before answering `timed-out`.
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a decoder that closed its output may take to exit.
const EXIT_TIMEOUT: Duration = Duration::from_secs(2);
/// Frames decoded ahead at most, whatever their size.
const MAX_FRAMES_AHEAD: usize = 256;
/// Step of the bounded waits on a child process.
const CHILD_POLL: Duration = Duration::from_millis(10);

/// Decodes plugin videos with the `ffmpeg` and `ffprobe` programs on `PATH`.
#[derive(Debug, Clone, Default)]
pub struct FfmpegMedia;

impl FfmpegMedia {
    /// The decoder of the client's plugin host.
    pub fn new() -> Self {
        Self
    }
}

/// The program `name` in a folder of `PATH`, or `unsupported` naming it.
fn program_on_path(name: &str) -> Result<PathBuf, PluginError> {
    std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .map(|folder| folder.join(name))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            PluginError::new(
                ErrorCode::Unsupported,
                format!("the program `{name}` is not on PATH"),
            )
        })
}

/// `file:<path>`: ffmpeg reads the argument as a local file, never as another protocol, even
/// when the name contains a colon.
fn file_input(file: &Path) -> OsString {
    let mut input = OsString::from("file:");
    input.push(file.as_os_str());
    input
}

/// Seconds as ffmpeg reads them.
fn seconds_argument(millis: u64) -> String {
    format!("{}.{:03}", millis / 1_000, millis % 1_000)
}

/// Arguments of the `ffprobe` call that describes the first video stream of `file`.
pub(crate) fn probe_arguments(file: &Path) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = [
        "-v",
        "error",
        "-hide_banner",
        "-max_alloc",
        DECODER_MAX_ALLOC,
        "-threads",
        "1",
        "-protocol_whitelist",
        "file",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,avg_frame_rate,r_frame_rate:format=duration",
        "-of",
        "default=noprint_wrappers=1",
        "-i",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    arguments.push(file_input(file));
    arguments
}

/// Arguments of the `ffmpeg` call that decodes `request` to raw `rgb24` frames on its standard
/// output.
pub(crate) fn decode_arguments(request: &DecodeRequest) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = Vec::new();
    let mut push = |items: &[&str]| arguments.extend(items.iter().map(OsString::from));
    push(&[
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-nostats",
        "-max_alloc",
        DECODER_MAX_ALLOC,
        "-protocol_whitelist",
        "file",
        "-threads",
        "2",
    ]);
    if request.start_millis > 0 {
        push(&["-ss", &seconds_argument(request.start_millis)]);
    }
    if let Some(end) = request.end_millis {
        let length = end.saturating_sub(request.start_millis);
        push(&["-t", &seconds_argument(length)]);
    }
    push(&["-i"]);
    arguments.push(file_input(&request.file));
    let scale = format!("scale={}:{}:flags=bilinear", request.width, request.height);
    let mut push = |items: &[&str]| arguments.extend(items.iter().map(OsString::from));
    push(&["-map", "0:v:0", "-an", "-sn", "-dn"]);
    push(&["-vf", &scale]);
    push(&["-pix_fmt", "rgb24", "-f", "rawvideo", "pipe:1"]);
    arguments
}

/// A frame rate as ffprobe prints it (`30000/1001`, `25/1`, `0/0`), in frames per second.
fn parse_rate(text: &str) -> Option<f32> {
    let (numerator, denominator) = text.trim().split_once('/')?;
    let numerator: f64 = numerator.parse().ok()?;
    let denominator: f64 = denominator.parse().ok()?;
    let rate = numerator / denominator;
    (rate.is_finite() && rate > 0.0).then_some(rate as f32)
}

/// The description in ffprobe's `key=value` output; a file without a video stream (no width or
/// height) is `unsupported`.
pub(crate) fn parse_probe(output: &str) -> Result<VideoInfo, PluginError> {
    let mut width = 0;
    let mut height = 0;
    let mut average_rate = None;
    let mut stated_rate = None;
    let mut duration_millis = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "width" => width = value.parse().unwrap_or(0),
            "height" => height = value.parse().unwrap_or(0),
            "avg_frame_rate" => average_rate = parse_rate(value),
            "r_frame_rate" => stated_rate = parse_rate(value),
            "duration" => {
                duration_millis = value
                    .parse::<f64>()
                    .ok()
                    .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
                    .map(|seconds| (seconds * 1_000.0).round() as u64);
            }
            _ => {}
        }
    }
    if width == 0 || height == 0 {
        return Err(PluginError::new(
            ErrorCode::Unsupported,
            "the file has no video stream ffmpeg decodes",
        ));
    }
    Ok(VideoInfo {
        width,
        height,
        frames_per_second: average_rate.or(stated_rate).unwrap_or(0.0),
        duration_millis,
    })
}

/// How many frames of `frame_bytes` fit in `buffer_bytes`: at least one, at most
/// [`MAX_FRAMES_AHEAD`].
pub(crate) fn frames_ahead(buffer_bytes: usize, frame_bytes: usize) -> usize {
    (buffer_bytes / frame_bytes.max(1)).clamp(1, MAX_FRAMES_AHEAD)
}

/// The decoder failure the manual names (`failed(decoder)`).
fn decoder_failed(detail: impl std::fmt::Display) -> PluginError {
    PluginError::new(ErrorCode::Io, format!("failed(decoder): {detail}"))
}

/// A child process of the decoder, started confined.
fn spawn_confined(program: &Path, arguments: &[OsString]) -> Result<Child, PluginError> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    match ilium_platform::child_limits::configure_child_address_space_limit(
        &mut command,
        DECODER_ADDRESS_SPACE_BYTES,
    ) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => {
            tracing::debug!(%error, "plugin video decoder address-space limit unavailable");
        }
        Err(error) => return Err(decoder_failed(error)),
    }
    command.spawn().map_err(decoder_failed)
}

/// Waits up to `limit` for `child` to exit; kills and reaps it when it does not.
fn wait_bounded(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if started.elapsed() < limit => std::thread::sleep(CHILD_POLL),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

impl MediaBackend for FfmpegMedia {
    fn probe(&self, file: &Path) -> Result<VideoInfo, PluginError> {
        // The manual's requirement is `ffmpeg`; `ffprobe` ships with it.
        program_on_path(FFMPEG_PROGRAM)?;
        let ffprobe = program_on_path(FFPROBE_PROGRAM)?;
        let mut child = spawn_confined(&ffprobe, &probe_arguments(file))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| decoder_failed("ffprobe has no output pipe"))?;
        // The description is a few lines, far below a pipe buffer, so reading after the exit
        // cannot stall the child.
        let status = wait_bounded(&mut child, PROBE_TIMEOUT)
            .ok_or_else(|| PluginError::new(ErrorCode::TimedOut, "ffprobe did not finish"))?;
        let mut output = String::new();
        stdout.read_to_string(&mut output).map_err(decoder_failed)?;
        if !status.success() {
            return Err(PluginError::new(
                ErrorCode::Unsupported,
                format!("ffprobe cannot read the file ({status})"),
            ));
        }
        parse_probe(&output)
    }

    fn decode(&self, request: &DecodeRequest) -> Result<Box<dyn FrameSource>, PluginError> {
        let ffmpeg = program_on_path(FFMPEG_PROGRAM)?;
        let frame_bytes = request.width as usize * request.height as usize * 3;
        let mut child = spawn_confined(&ffmpeg, &decode_arguments(request))?;
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(decoder_failed("ffmpeg has no output pipe"));
        };
        let (sender, frames) = mpsc::sync_channel(frames_ahead(request.buffer_bytes, frame_bytes));
        let reader = std::thread::Builder::new()
            .name("plugin-video-decode".to_owned())
            .spawn(move || read_frames(stdout, frame_bytes, &sender));
        let reader = match reader {
            Ok(reader) => reader,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(decoder_failed(error));
            }
        };
        Ok(Box::new(FfmpegRun {
            child,
            frames: Some(frames),
            reader: Some(reader),
            finished: false,
        }))
    }
}

/// Reads whole frames from the decoder until its output ends or the run is dropped.
fn read_frames(mut stdout: impl Read, frame_bytes: usize, frames: &SyncSender<Vec<u8>>) {
    loop {
        let mut frame = vec![0; frame_bytes];
        if stdout.read_exact(&mut frame).is_err() {
            // The end of the output (a partial last frame is dropped) or a closed pipe.
            return;
        }
        if frames.send(frame).is_err() {
            return;
        }
    }
}

/// One running decoder.
struct FfmpegRun {
    child: Child,
    frames: Option<Receiver<Vec<u8>>>,
    reader: Option<JoinHandle<()>>,
    finished: bool,
}

impl FfmpegRun {
    /// The decoder closed its output: the end of the run, or a crash.
    fn finish(&mut self) -> Result<Option<Vec<u8>>, PluginError> {
        if self.finished {
            return Ok(None);
        }
        self.finished = true;
        match wait_bounded(&mut self.child, EXIT_TIMEOUT) {
            Some(status) if status.success() => Ok(None),
            Some(status) => Err(decoder_failed(format!("ffmpeg exited with {status}"))),
            None => Err(decoder_failed("ffmpeg stopped writing but did not exit")),
        }
    }
}

impl FrameSource for FfmpegRun {
    fn next_frame(&mut self) -> Result<Option<Vec<u8>>, PluginError> {
        let Some(frames) = &self.frames else {
            return Ok(None);
        };
        match frames.recv_timeout(FRAME_TIMEOUT) {
            Ok(frame) => Ok(Some(frame)),
            Err(RecvTimeoutError::Disconnected) => self.finish(),
            Err(RecvTimeoutError::Timeout) => Err(PluginError::new(
                ErrorCode::TimedOut,
                "the decoder has no frame ready yet",
            )),
        }
    }
}

impl Drop for FfmpegRun {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        // Dropping the receiver unblocks a reader waiting for room; the killed child closed
        // the pipe a reader waits on.
        self.frames = None;
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decode_arguments, frames_ahead, parse_probe, probe_arguments, program_on_path, read_frames,
        FfmpegMedia, MAX_FRAMES_AHEAD,
    };
    use ilium_plugin_api::generated::types::errors::ErrorCode;
    use ilium_plugin_host::imports::media::{DecodeRequest, MediaBackend};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    fn strings(arguments: &[OsString]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    fn request(start: u64, end: Option<u64>) -> DecodeRequest {
        DecodeRequest {
            file: PathBuf::from("/videos/a:b.mp4"),
            start_millis: start,
            end_millis: end,
            width: 32,
            height: 18,
            buffer_bytes: 64 * 1024 * 1024,
        }
    }

    #[test]
    fn the_decoder_reads_only_the_one_local_file() {
        for arguments in [
            strings(&decode_arguments(&request(0, None))),
            strings(&probe_arguments(Path::new("/videos/a:b.mp4"))),
        ] {
            let whitelist = arguments
                .iter()
                .position(|argument| argument == "-protocol_whitelist")
                .expect("protocol whitelist");
            assert_eq!(arguments[whitelist + 1], "file");
            assert!(arguments.contains(&"file:/videos/a:b.mp4".to_owned()));
            assert!(arguments.contains(&"-max_alloc".to_owned()));
        }
        assert!(strings(&decode_arguments(&request(0, None))).contains(&"-nostdin".to_owned()));
    }

    #[test]
    fn a_run_starts_at_its_position_and_stops_at_the_range_end() {
        let arguments = strings(&decode_arguments(&request(1_500, Some(4_000))));
        let seek = arguments.iter().position(|argument| argument == "-ss");
        let length = arguments.iter().position(|argument| argument == "-t");
        let input = arguments.iter().position(|argument| argument == "-i");
        assert_eq!(arguments[seek.expect("seek") + 1], "1.500");
        assert_eq!(arguments[length.expect("length") + 1], "2.500");
        assert!(seek < input && length < input, "input options precede -i");
        assert!(arguments.contains(&"scale=32:18:flags=bilinear".to_owned()));
        let from_start = strings(&decode_arguments(&request(0, None)));
        assert!(!from_start.contains(&"-ss".to_owned()));
        assert!(!from_start.contains(&"-t".to_owned()));
        assert!(from_start.ends_with(&[
            "-pix_fmt".to_owned(),
            "rgb24".to_owned(),
            "-f".to_owned(),
            "rawvideo".to_owned(),
            "pipe:1".to_owned()
        ]));
    }

    #[test]
    fn a_probe_reads_size_rate_and_duration() {
        let info = parse_probe(
            "width=1920\nheight=1080\nr_frame_rate=30000/1001\navg_frame_rate=0/0\nduration=12.345678\n",
        )
        .expect("video");
        assert_eq!((info.width, info.height), (1920, 1080));
        assert!((info.frames_per_second - 29.97).abs() < 0.01);
        assert_eq!(info.duration_millis, Some(12_346));
        let open_ended =
            parse_probe("width=64\nheight=36\navg_frame_rate=25/1\nduration=N/A\n").expect("video");
        assert_eq!(open_ended.frames_per_second, 25.0);
        assert_eq!(open_ended.duration_millis, None);
    }

    #[test]
    fn a_file_without_a_video_stream_is_unsupported() {
        assert_eq!(
            parse_probe("duration=3.0\n").expect_err("audio only").code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn the_decode_ahead_buffer_holds_between_one_and_the_cap_of_frames() {
        assert_eq!(frames_ahead(64 * 1024 * 1024, 3_840 * 2_160 * 3), 2);
        assert_eq!(frames_ahead(1024, 3_840 * 2_160 * 3), 1);
        assert_eq!(frames_ahead(64 * 1024 * 1024, 3), MAX_FRAMES_AHEAD);
    }

    #[test]
    fn the_reader_sends_whole_frames_and_drops_a_partial_last_one() {
        let (sender, frames) = std::sync::mpsc::sync_channel(8);
        read_frames(&[1u8, 2, 3, 4, 5, 6, 7][..], 3, &sender);
        drop(sender);
        let read: Vec<Vec<u8>> = frames.iter().collect();
        assert_eq!(read, vec![vec![1, 2, 3], vec![4, 5, 6]]);
    }

    /// Decodes a generated clip when the machine has ffmpeg; skipped otherwise.
    #[test]
    fn a_generated_clip_decodes_to_frames_of_the_requested_size_when_ffmpeg_is_installed() {
        let (Ok(ffmpeg), Ok(_)) = (program_on_path("ffmpeg"), program_on_path("ffprobe")) else {
            return;
        };
        let folder = tempfile::tempdir().expect("scratch");
        let clip = folder.path().join("clip.mp4");
        let made = std::process::Command::new(ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=64x36:rate=10",
                "-t",
                "1",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&clip)
            .status()
            .expect("ffmpeg runs");
        if !made.success() {
            return;
        }
        let media = FfmpegMedia::new();
        let info = media.probe(&clip).expect("probed");
        assert_eq!((info.width, info.height), (64, 36));
        let mut run = media
            .decode(&DecodeRequest {
                file: clip,
                start_millis: 500,
                end_millis: None,
                width: 8,
                height: 4,
                buffer_bytes: 1024,
            })
            .expect("decoding");
        let mut frames = 0;
        while let Some(frame) = run.next_frame().expect("frame") {
            assert_eq!(frame.len(), 8 * 4 * 3);
            frames += 1;
        }
        assert!(
            (3..=7).contains(&frames),
            "half a second at 10 fps, got {frames}"
        );
    }
}
