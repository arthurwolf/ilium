//! ffmpeg/ffprobe command lines and the process abstraction behind them.
//!
//! Nothing here goes through a shell: every argument is an `OsString`, paths
//! are passed after `-i` with a `file:` prefix, and network protocols are
//! restricted with `-protocol_whitelist`. Spawning goes through
//! `CommandRunner` so tests can inject fake processes.

use super::discover::MediaInput;
use super::settings::FitMode;
use std::ffi::OsString;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const FFMPEG_PROGRAM: &str = "ffmpeg";
pub const FFPROBE_PROGRAM: &str = "ffprobe";

const LOCAL_PROTOCOLS: &str = "file,crypto";
const NETWORK_PROTOCOLS: &str = "http,https,tcp,tls,crypto";
/// Give up on a stalled network read after 20 s (microseconds for ffmpeg).
const NETWORK_TIMEOUT_MICROSECONDS: &str = "20000000";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Gray,
    Rgb24,
}

impl PixelFormat {
    pub fn bytes_per_dot(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::Rgb24 => 3,
        }
    }

    fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::Gray => "gray",
            Self::Rgb24 => "rgb24",
        }
    }
}

/// Settings-derived options shared by every clip of one scene.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamOptions {
    pub frames_per_second: u32,
    /// `Some(percent)` slows the clip down to that share of real time.
    pub slowed_percent: Option<u32>,
    pub fit: FitMode,
    pub pixel_format: PixelFormat,
    /// Sharpen strength 0..=100, 0 disables the filter.
    pub detail: u32,
}

impl StreamOptions {
    /// Source seconds that pass per second of output.
    pub fn speed_ratio(&self) -> f64 {
        self.slowed_percent
            .map_or(1.0, |percent| f64::from(percent) / 100.0)
    }
}

/// One ffmpeg run: where to start, how long, and the exact output size.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayRequest {
    pub input: MediaInput,
    pub start_seconds: f64,
    pub limit_seconds: Option<f64>,
    /// Output size in Braille dots (2 x columns, 4 x rows).
    pub dots_width: u32,
    pub dots_height: u32,
}

impl PlayRequest {
    pub fn bounded_frame_bytes(&self, format: PixelFormat) -> Option<usize> {
        if self.dots_width == 0
            || self.dots_height == 0
            || self.dots_width > 1024
            || self.dots_height > 512
        {
            return None;
        }
        (self.dots_width as usize)
            .checked_mul(self.dots_height as usize)?
            .checked_mul(format.bytes_per_dot())
    }
    #[cfg(test)]
    pub fn frame_bytes(&self, format: PixelFormat) -> usize {
        self.dots_width as usize * self.dots_height as usize * format.bytes_per_dot()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
}

fn seconds_argument(seconds: f64) -> String {
    format!("{:.3}", seconds.max(0.0))
}

fn protocols_for(input: &MediaInput) -> &'static str {
    if input.is_url() {
        NETWORK_PROTOCOLS
    } else {
        LOCAL_PROTOCOLS
    }
}

/// The `-vf` chain: square pixels, optional slow motion, frame-rate
/// conversion, fit to exactly `width x height` dots, optional sharpening.
pub fn filter_chain(width: u32, height: u32, options: &StreamOptions) -> String {
    let mut filters = vec!["scale=trunc(iw*sar):ih".to_owned(), "setsar=1".to_owned()];
    if let Some(percent) = options.slowed_percent.filter(|percent| *percent < 100) {
        filters.push(format!("setpts=PTS/{}", f64::from(percent) / 100.0));
    }
    filters.push(format!("fps={}", options.frames_per_second));
    match options.fit {
        FitMode::Fit => {
            filters.push(format!(
                "scale={width}:{height}:force_original_aspect_ratio=decrease:flags=area"
            ));
            filters.push(format!("pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black"));
        }
        FitMode::Fill => {
            filters.push(format!(
                "scale={width}:{height}:force_original_aspect_ratio=increase:flags=area"
            ));
            filters.push(format!("crop={width}:{height}"));
        }
        FitMode::Stretch => filters.push(format!("scale={width}:{height}:flags=area")),
    }
    if options.detail > 0 {
        let amount = f64::from(options.detail) / 100.0 * 1.5;
        filters.push(format!("unsharp=5:5:{amount:.2}:5:5:0"));
    }
    filters.join(",")
}

pub fn ffmpeg_spec(request: &PlayRequest, options: &StreamOptions) -> CommandSpec {
    let mut args: Vec<OsString> = Vec::new();
    let mut push = |items: &[&str]| args.extend(items.iter().map(OsString::from));
    push(&["-hide_banner", "-nostdin", "-loglevel", "error", "-nostats"]);
    if request.input.is_url() {
        // Identify ourselves politely; option documented in the http section
        // of https://ffmpeg.org/ffmpeg-protocols.html.
        push(&["-rw_timeout", NETWORK_TIMEOUT_MICROSECONDS]);
        push(&["-user_agent", crate::source::USER_AGENT]);
    }
    push(&["-protocol_whitelist", protocols_for(&request.input)]);
    // Two decoder threads keep the machine responsive; 24 fps of a tiny
    // output needs no more.
    push(&["-threads", "2"]);
    if request.start_seconds > 0.0 {
        push(&["-ss", &seconds_argument(request.start_seconds)]);
    }
    if let Some(limit) = request.limit_seconds {
        push(&["-t", &seconds_argument(limit)]);
    }
    push(&["-i"]);
    args.push(request.input.ffmpeg_argument());
    let chain = filter_chain(request.dots_width, request.dots_height, options);
    let mut push = |items: &[&str]| args.extend(items.iter().map(OsString::from));
    push(&[
        "-map",
        "0:v:0?",
        "-an",
        "-sn",
        "-dn",
        "-filter_threads",
        "1",
    ]);
    push(&["-vf", &chain]);
    push(&["-pix_fmt", options.pixel_format.ffmpeg_name()]);
    push(&["-f", "rawvideo", "pipe:1"]);
    CommandSpec {
        program: OsString::from(FFMPEG_PROGRAM),
        args,
    }
}

pub fn ffprobe_spec(input: &MediaInput) -> CommandSpec {
    let mut args: Vec<OsString> = ["-v", "error", "-hide_banner", "-protocol_whitelist"]
        .iter()
        .map(OsString::from)
        .collect();
    args.push(OsString::from(protocols_for(input)));
    if input.is_url() {
        args.push(OsString::from("-rw_timeout"));
        args.push(OsString::from(NETWORK_TIMEOUT_MICROSECONDS));
        args.push(OsString::from("-user_agent"));
        args.push(OsString::from(crate::source::USER_AGENT));
    }
    for item in [
        "-show_entries",
        "format=duration",
        "-of",
        "default=noprint_wrappers=1:nokey=1",
        "-i",
    ] {
        args.push(OsString::from(item));
    }
    args.push(input.ffmpeg_argument());
    CommandSpec {
        program: OsString::from(FFPROBE_PROGRAM),
        args,
    }
}

/// Parse ffprobe's plain duration output ("93.500000", "N/A").
pub fn parse_duration(output: &str) -> Option<f64> {
    output
        .lines()
        .next()?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
}

/// Control half of a running child. Both calls take `&self` and never block
/// for long, so another thread may kill a child a worker is reading from.
pub trait ChildControl: Send + Sync {
    fn kill(&self);
    /// Wait up to `timeout` for the child to be reaped. `true` when gone.
    fn reap(&self, timeout: Duration) -> bool;
}

pub struct SpawnedChild {
    pub stdout: Box<dyn Read + Send>,
    pub control: Arc<dyn ChildControl>,
}

pub trait CommandRunner: Send + Sync {
    fn spawn(&self, spec: &CommandSpec) -> std::io::Result<SpawnedChild>;
}

/// Runs real processes: stdin and stderr closed, stdout piped.
pub struct SystemRunner;

const MAX_VIDEO_CHILDREN: usize = 4;
static ACTIVE_VIDEO_CHILDREN: AtomicUsize = AtomicUsize::new(0);
struct ChildAdmission;
impl ChildAdmission {
    fn acquire() -> std::io::Result<Self> {
        ACTIVE_VIDEO_CHILDREN
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_VIDEO_CHILDREN).then_some(count + 1)
            })
            .map(|_| Self)
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "previous video helpers are still stopping; retry shortly",
                )
            })
    }
}
impl Drop for ChildAdmission {
    fn drop(&mut self) {
        ACTIVE_VIDEO_CHILDREN.fetch_sub(1, Ordering::AcqRel);
    }
}
struct RetiredChild {
    child: Child,
    _admission: ChildAdmission,
}
struct SystemChild {
    child: Mutex<Option<Child>>,
    admission: Option<ChildAdmission>,
}

impl Drop for SystemChild {
    fn drop(&mut self) {
        let Some(mut child) = self
            .child
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        else {
            return;
        };
        let _ = child.kill();
        if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
            return;
        }
        // Keep ownership when an OS-delayed child outlasts the ordinary reap
        // deadline. One portable reaper handles every such child.
        let Some(admission) = self.admission.take() else {
            return;
        };
        let child = RetiredChild {
            child,
            _admission: admission,
        };
        static REAPER: OnceLock<std::sync::mpsc::Sender<RetiredChild>> = OnceLock::new();
        let sender = REAPER.get_or_init(|| {
            let (sender, receiver) = std::sync::mpsc::channel::<RetiredChild>();
            let result = std::thread::Builder::new()
                .name("ilium-video-child-reaper".into())
                .spawn(move || {
                    let mut pending: Vec<RetiredChild> = Vec::new();
                    loop {
                        match receiver.recv_timeout(Duration::from_millis(25)) {
                            Ok(child) => pending.push(child),
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        }
                        let mut index = 0;
                        while index < pending.len() {
                            if matches!(pending[index].child.try_wait(), Ok(Some(_)) | Err(_)) {
                                let mut finished = pending.swap_remove(index);
                                let _ = finished.child.wait();
                            } else {
                                index += 1;
                            }
                        }
                    }
                });
            if let Err(error) = result {
                tracing::warn!(%error, "video child reaper could not start");
            }
            sender
        });
        if sender.send(child).is_err() {
            tracing::warn!("video child cleanup owner unavailable");
        }
    }
}

impl ChildControl for SystemChild {
    fn kill(&self) {
        let mut child = self
            .child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Error means "already exited", which is what we want.
        if let Some(child) = child.as_mut() {
            let _ = child.kill();
        }
    }

    fn reap(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let mut child = self
                    .child
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let Some(child) = child.as_mut() else {
                    return true;
                };
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => return true,
                    Ok(None) => {}
                }
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl CommandRunner for SystemRunner {
    fn spawn(&self, spec: &CommandSpec) -> std::io::Result<SpawnedChild> {
        let admission = ChildAdmission::acquire()?;
        let mut child = Command::new(&spec.program)
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::other("child has no stdout pipe"));
        };
        Ok(SpawnedChild {
            stdout: Box::new(stdout),
            control: Arc::new(SystemChild {
                child: Mutex::new(Some(child)),
                admission: Some(admission),
            }),
        })
    }
}

/// Where the scene remembers the live child so `Drop` can kill it from
/// outside the worker thread while that thread is blocked reading it.
#[derive(Clone, Default)]
pub struct ChildSlot {
    inner: Arc<Mutex<SlotState>>,
}

#[derive(Default)]
struct SlotState {
    closed: bool,
    current: Option<Arc<dyn ChildControl>>,
    deadline: Option<Instant>,
    timed_out: bool,
}

impl ChildSlot {
    fn lock(&self) -> std::sync::MutexGuard<'_, SlotState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Register a fresh child. Returns `false` when the slot is already
    /// closed: the caller must then kill the child itself.
    pub fn register(&self, control: Arc<dyn ChildControl>) -> bool {
        let mut state = self.lock();
        if state.closed {
            return false;
        }
        state.current = Some(control);
        state.timed_out = false;
        state.deadline = Some(Instant::now() + Duration::from_secs(3));
        true
    }

    pub fn clear(&self) {
        let mut state = self.lock();
        state.current = None;
        state.deadline = None;
    }

    pub fn arm(&self, duration: Option<Duration>) {
        self.lock().deadline = duration.map(|duration| Instant::now() + duration);
    }

    pub fn timed_out(&self) -> bool {
        self.lock().timed_out
    }

    pub fn watchdog(&self) -> std::io::Result<crate::source::Worker> {
        let slot = self.clone();
        crate::source::Worker::try_spawn("video-watchdog", move |stop| {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let expired = {
                    let mut state = slot.lock();
                    if state.closed {
                        return;
                    }
                    if state.deadline.is_some_and(|at| Instant::now() >= at) {
                        state.deadline = None;
                        state.timed_out = true;
                        state.current.clone()
                    } else {
                        None
                    }
                };
                if let Some(child) = expired {
                    child.kill();
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        })
    }

    /// Refuse further children and kill the current one.
    pub fn close(&self) {
        let control = {
            let mut state = self.lock();
            state.closed = true;
            state.current.take()
        };
        if let Some(control) = control {
            control.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn options(fit: FitMode) -> StreamOptions {
        StreamOptions {
            frames_per_second: 12,
            slowed_percent: None,
            fit,
            pixel_format: PixelFormat::Gray,
            detail: 0,
        }
    }

    fn request(input: MediaInput) -> PlayRequest {
        PlayRequest {
            input,
            start_seconds: 0.0,
            limit_seconds: None,
            dots_width: 120,
            dots_height: 80,
        }
    }

    fn strings(spec: &CommandSpec) -> Vec<String> {
        spec.args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn live_file_command_line_is_exact() {
        let spec = ffmpeg_spec(
            &request(MediaInput::File(PathBuf::from("/v/-my clip.mkv"))),
            &options(FitMode::Fit),
        );
        assert_eq!(spec.program, OsString::from("ffmpeg"));
        assert_eq!(
            strings(&spec),
            [
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-nostats",
                "-protocol_whitelist",
                "file,crypto",
                "-threads",
                "2",
                "-i",
                "file:/v/-my clip.mkv",
                "-map",
                "0:v:0?",
                "-an",
                "-sn",
                "-dn",
                "-filter_threads",
                "1",
                "-vf",
                "scale=trunc(iw*sar):ih,setsar=1,fps=12,scale=120:80:force_original_aspect_ratio=decrease:flags=area,pad=120:80:(ow-iw)/2:(oh-ih)/2:black",
                "-pix_fmt",
                "gray",
                "-f",
                "rawvideo",
                "pipe:1",
            ]
        );
    }

    #[test]
    fn slowed_mode_uses_setpts_before_the_frame_rate_filter() {
        let mut slowed = options(FitMode::Stretch);
        slowed.slowed_percent = Some(25);
        assert_eq!(
            filter_chain(40, 16, &slowed),
            "scale=trunc(iw*sar):ih,setsar=1,setpts=PTS/0.25,fps=12,scale=40:16:flags=area"
        );
        assert!((slowed.speed_ratio() - 0.25).abs() < 1e-12);
        slowed.slowed_percent = Some(100);
        assert!(!filter_chain(40, 16, &slowed).contains("setpts"));
    }

    #[test]
    fn random_scene_seeks_and_limits_before_the_input() {
        let mut random = request(MediaInput::File(PathBuf::from("a.mp4")));
        random.start_seconds = 754.3216;
        random.limit_seconds = Some(20.0);
        let args = strings(&ffmpeg_spec(&random, &options(FitMode::Fit)));
        let seek = args.iter().position(|arg| arg == "-ss").unwrap();
        let limit = args.iter().position(|arg| arg == "-t").unwrap();
        let input = args.iter().position(|arg| arg == "-i").unwrap();
        assert_eq!(args[seek + 1], "754.322");
        assert_eq!(args[limit + 1], "20.000");
        assert!(seek < input && limit < input);
        assert_eq!(args[input + 1], "file:a.mp4");
    }

    #[test]
    fn fill_and_stretch_and_detail_filters() {
        let mut fill = options(FitMode::Fill);
        fill.detail = 40;
        fill.pixel_format = PixelFormat::Rgb24;
        let chain = filter_chain(200, 100, &fill);
        assert!(chain.contains(
            "scale=200:100:force_original_aspect_ratio=increase:flags=area,crop=200:100"
        ));
        assert!(chain.ends_with("unsharp=5:5:0.60:5:5:0"));
        let spec = ffmpeg_spec(&request(MediaInput::File("x.mp4".into())), &fill);
        assert!(strings(&spec)
            .windows(2)
            .any(|pair| pair == ["-pix_fmt", "rgb24"]));
    }

    #[test]
    fn urls_get_network_protocols_and_a_read_timeout() {
        let url = MediaInput::Url("https://example.com/live.m3u8?a=1&b=2".to_owned());
        let args = strings(&ffmpeg_spec(&request(url.clone()), &options(FitMode::Fit)));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-rw_timeout", "20000000"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-protocol_whitelist", "http,https,tcp,tls,crypto"]));
        let input = args.iter().position(|arg| arg == "-i").unwrap();
        assert_eq!(args[input + 1], "https://example.com/live.m3u8?a=1&b=2");
    }

    #[test]
    fn ffprobe_command_line_is_exact() {
        let spec = ffprobe_spec(&MediaInput::File(PathBuf::from("/v/a b.mp4")));
        assert_eq!(spec.program, OsString::from("ffprobe"));
        assert_eq!(
            strings(&spec),
            [
                "-v",
                "error",
                "-hide_banner",
                "-protocol_whitelist",
                "file,crypto",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                "-i",
                "file:/v/a b.mp4",
            ]
        );
    }

    #[test]
    fn duration_parsing_rejects_junk() {
        assert_eq!(parse_duration("93.500000\n"), Some(93.5));
        assert_eq!(parse_duration("N/A\n"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("0.000000"), None);
        assert_eq!(parse_duration("inf"), None);
    }

    #[test]
    fn frame_size_depends_on_the_pixel_format() {
        let request = request(MediaInput::File("x".into()));
        assert_eq!(request.frame_bytes(PixelFormat::Gray), 9600);
        assert_eq!(request.frame_bytes(PixelFormat::Rgb24), 28800);
    }

    #[test]
    fn closed_slot_refuses_new_children_and_kills_the_current_one() {
        struct Probe(Mutex<u32>);
        impl ChildControl for Probe {
            fn kill(&self) {
                *self.0.lock().unwrap() += 1;
            }
            fn reap(&self, _: Duration) -> bool {
                true
            }
        }
        let slot = ChildSlot::default();
        let first = Arc::new(Probe(Mutex::new(0)));
        assert!(slot.register(first.clone()));
        slot.close();
        assert_eq!(*first.0.lock().unwrap(), 1);
        let late = Arc::new(Probe(Mutex::new(0)));
        assert!(!slot.register(late.clone()));
        assert_eq!(
            *late.0.lock().unwrap(),
            0,
            "caller kills refused children itself"
        );
    }
}
