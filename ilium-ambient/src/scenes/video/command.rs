//! ffmpeg/ffprobe command lines and the process abstraction behind them.
//!
//! Nothing here goes through a shell: every argument is an `OsString`, paths
//! are passed after `-i` with a `file:` prefix, and network protocols are
//! restricted with `-protocol_whitelist`. Spawning goes through
//! `CommandRunner` so tests can inject fake processes.

use super::discover::MediaInput;
use super::settings::FitMode;
use crate::resources::{AmbientResources, WorkerCost, WorkerReservation};
use crate::source::Worker;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
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
/// Crop before scaling Fill; Fit scales directly to bounded square-pixel
/// dimensions. Neither creates a source-sized SAR correction intermediate.
pub fn filter_chain(width: u32, height: u32, options: &StreamOptions) -> String {
    let mut filters = Vec::new();
    if let Some(percent) = options.slowed_percent.filter(|percent| *percent < 100) {
        filters.push(format!("setpts=PTS/{}", f64::from(percent) / 100.0));
    }
    filters.push(format!("fps={}", options.frames_per_second));
    match options.fit {
        FitMode::Fit => {
            filters.push(format!(
                "scale=w='max(1,min({width},trunc({height}*dar)))':h='max(1,min({height},trunc({width}/dar)))':flags=area"
            ));
            filters.push("setsar=1".to_owned());
            filters.push(format!("pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black"));
        }
        FitMode::Fill => {
            filters.push(format!(
                "crop=w='max(1,min(iw,ih*{width}/{height}/sar))':h='max(1,min(ih,iw*sar*{height}/{width}))':exact=1"
            ));
            filters.push(format!("scale={width}:{height}:flags=area"));
            filters.push("setsar=1".to_owned());
        }
        FitMode::Stretch => {
            filters.push(format!("scale={width}:{height}:flags=area"));
            filters.push("setsar=1".to_owned());
        }
    }
    if options.detail > 0 && width >= 5 && height >= 5 {
        let amount = f64::from(options.detail) / 100.0 * 1.5;
        filters.push(format!("unsharp=5:5:{amount:.2}:5:5:0"));
    }
    filters.join(",")
}

pub fn ffmpeg_spec(request: &PlayRequest, options: &StreamOptions) -> CommandSpec {
    let mut args: Vec<OsString> = Vec::new();
    let mut push = |items: &[&str]| args.extend(items.iter().map(OsString::from));
    push(&[
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-nostats",
        "-max_alloc",
        "134217728",
    ]);
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
    push(&["-threads", "1"]);
    push(&["-f", "rawvideo", "pipe:1"]);
    CommandSpec {
        program: OsString::from(FFMPEG_PROGRAM),
        args,
    }
}

pub fn ffprobe_spec(input: &MediaInput) -> CommandSpec {
    let mut args: Vec<OsString> = [
        "-v",
        "error",
        "-hide_banner",
        "-max_alloc",
        "134217728",
        "-threads",
        "2",
        "-protocol_whitelist",
    ]
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
    /// Available native exit status. Fixtures may have no OS process status.
    fn successful_exit(&self) -> Option<bool> {
        None
    }
    fn diagnostic_tail(&self) -> String {
        String::new()
    }
}

pub struct SpawnedChild {
    pub stdout: Box<dyn Read + Send>,
    pub control: Arc<dyn ChildControl>,
}

pub trait CommandRunner: Send + Sync {
    fn spawn(&self, spec: &CommandSpec) -> std::io::Result<SpawnedChild>;
}

/// Native children are admitted against the host before spawn. A prestarted
/// custodian retains delayed children and their physical credit until OS exit.
pub struct SystemRunner {
    resources: AmbientResources,
    custodian: Mutex<Option<Arc<ChildCustodian>>>,
}

const MAX_VIDEO_CHILDREN: usize = 4;
const HELPER_BYTES: usize = 8 * 1024 * 1024;
const NATIVE_BYTES: usize = 1024 * 1024 * 1024;
// These are declared known FFmpeg roles, not a proven aggregate OS-thread cap.
const DECLARED_NATIVE_THREADS: usize = 5;
const STDERR_TAIL_BYTES: usize = 2048;
static ACTIVE_VIDEO_CHILDREN: AtomicUsize = AtomicUsize::new(0);

fn admission_error(error: impl std::fmt::Debug) -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        format!("Video host admission rejected: {error:?}"),
    )
}

struct ChildAdmission;
impl ChildAdmission {
    fn acquire() -> io::Result<Self> {
        ACTIVE_VIDEO_CHILDREN
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_VIDEO_CHILDREN).then_some(count + 1)
            })
            .map(|_| Self)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
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
    _physical: WorkerReservation,
    exit: Option<ExitStatus>,
    logged_wait_error: bool,
}
impl RetiredChild {
    fn check_exit(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(exit) = self.exit {
            return Ok(Some(exit));
        }
        let status = self.child.try_wait()?;
        self.exit = status;
        Ok(status)
    }
}

struct ChildCustodian {
    sender: mpsc::Sender<RetiredChild>,
    _worker: Worker,
}
impl ChildCustodian {
    fn start(resources: &AmbientResources) -> io::Result<Arc<Self>> {
        let reservation = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: HELPER_BYTES,
            })
            .map_err(admission_error)?;
        let (sender, receiver) = mpsc::channel::<RetiredChild>();
        let worker = Worker::start_admitted("video-child-custodian", reservation, move |_| {
            let mut pending = Vec::with_capacity(MAX_VIDEO_CHILDREN);
            let mut disconnected = false;
            loop {
                if disconnected {
                    std::thread::sleep(Duration::from_millis(25));
                } else {
                    match receiver.recv_timeout(Duration::from_millis(25)) {
                        Ok(child) => pending.push(child),
                        Err(mpsc::RecvTimeoutError::Disconnected) => disconnected = true,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
                let mut index = 0;
                while index < pending.len() {
                    match pending[index].check_exit() {
                        Ok(Some(_)) => {
                            pending.swap_remove(index);
                        }
                        Ok(None) => index += 1,
                        Err(error) => {
                            if !pending[index].logged_wait_error {
                                tracing::warn!(%error, "cannot verify retired video child exit");
                                pending[index].logged_wait_error = true;
                            }
                            index += 1;
                        }
                    }
                }
                if disconnected && pending.is_empty() {
                    return;
                }
            }
        })?;
        Ok(Arc::new(Self {
            sender,
            _worker: worker,
        }))
    }

    fn retire(&self, child: RetiredChild) {
        if let Err(error) = self.sender.send(child) {
            // The worker unexpectedly exited. Preserve both the OS handle and
            // native debit; dropping either would report false retirement.
            static EMERGENCY: OnceLock<Mutex<Vec<RetiredChild>>> = OnceLock::new();
            EMERGENCY
                .get_or_init(|| Mutex::new(Vec::new()))
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(error.0);
            tracing::error!(
                "video child custodian unavailable; child retained for process lifetime"
            );
        }
    }
}

impl SystemRunner {
    pub fn new(resources: AmbientResources) -> Self {
        Self {
            resources,
            custodian: Mutex::new(None),
        }
    }

    fn custodian(&self) -> io::Result<Arc<ChildCustodian>> {
        let mut owned = self
            .custodian
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(custodian) = owned.as_ref() {
            return Ok(Arc::clone(custodian));
        }
        let custodian = ChildCustodian::start(&self.resources)?;
        *owned = Some(Arc::clone(&custodian));
        Ok(custodian)
    }
}

struct SystemChild {
    child: Mutex<Option<RetiredChild>>,
    custodian: Arc<ChildCustodian>,
    stderr_tail: Arc<Mutex<VecDeque<u8>>>,
    _stderr_worker: Worker,
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
        if !matches!(child.check_exit(), Ok(Some(_))) {
            let _ = child.child.kill();
        }
        if !matches!(child.check_exit(), Ok(Some(_))) {
            self.custodian.retire(child);
        }
    }
}
impl ChildControl for SystemChild {
    fn successful_exit(&self) -> Option<bool> {
        self.child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_mut()?
            .check_exit()
            .ok()
            .flatten()
            .map(|status| status.success())
    }

    fn diagnostic_tail(&self) -> String {
        let tail = self
            .stderr_tail
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let bytes: Vec<u8> = tail.iter().copied().collect();
        String::from_utf8_lossy(&bytes)
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect::<String>()
            .trim()
            .to_owned()
    }

    fn kill(&self) {
        if let Some(child) = self
            .child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_mut()
        {
            if !matches!(child.check_exit(), Ok(Some(_))) {
                let _ = child.child.kill();
            }
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
                if matches!(child.check_exit(), Ok(Some(_))) {
                    return true;
                }
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn retire_failed_setup(mut child: RetiredChild, custodian: &ChildCustodian) {
    let _ = child.child.kill();
    if !matches!(child.check_exit(), Ok(Some(_))) {
        custodian.retire(child);
    }
}

impl CommandRunner for SystemRunner {
    fn spawn(&self, spec: &CommandSpec) -> io::Result<SpawnedChild> {
        let admission = ChildAdmission::acquire()?;
        // The cleanup owner and every host debit exist before the native spawn.
        let custodian = self.custodian()?;
        let native = self
            .resources
            .reserve_worker(WorkerCost {
                threads: DECLARED_NATIVE_THREADS,
                resident_bytes: NATIVE_BYTES,
            })
            .map_err(admission_error)?;
        let diagnostics = self
            .resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: HELPER_BYTES,
            })
            .map_err(admission_error)?;
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("OPENBLAS_NUM_THREADS", "1")
            .env("OMP_NUM_THREADS", "1");
        match ilium_platform::child_limits::configure_child_address_space_limit(
            &mut command,
            NATIVE_BYTES,
        ) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                tracing::debug!(%error, "native video address-space limit unavailable on this platform");
            }
            Err(error) => return Err(error),
        }
        let child = command.spawn()?;
        let mut owned = RetiredChild {
            child,
            _admission: admission,
            _physical: native,
            exit: None,
            logged_wait_error: false,
        };
        let Some(stdout) = owned.child.stdout.take() else {
            retire_failed_setup(owned, &custodian);
            return Err(io::Error::other("child has no stdout pipe"));
        };
        let Some(mut stderr) = owned.child.stderr.take() else {
            retire_failed_setup(owned, &custodian);
            return Err(io::Error::other("child has no stderr pipe"));
        };
        let tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_BYTES)));
        let writer = Arc::clone(&tail);
        let stderr_worker = match Worker::start_admitted("video-stderr", diagnostics, move |_| {
            let mut chunk = [0u8; 512];
            loop {
                match stderr.read(&mut chunk) {
                    Ok(0) => return,
                    Ok(count) => {
                        let mut tail = writer
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        for byte in &chunk[..count] {
                            if tail.len() == STDERR_TAIL_BYTES {
                                tail.pop_front();
                            }
                            tail.push_back(*byte);
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => return,
                }
            }
        }) {
            Ok(worker) => worker,
            Err(error) => {
                retire_failed_setup(owned, &custodian);
                return Err(error);
            }
        };
        Ok(SpawnedChild {
            stdout: Box::new(stdout),
            control: Arc::new(SystemChild {
                child: Mutex::new(Some(owned)),
                custodian,
                stderr_tail: tail,
                _stderr_worker: stderr_worker,
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

    pub fn is_closed(&self) -> bool {
        self.lock().closed
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

    pub fn watchdog(&self, resources: &AmbientResources) -> io::Result<Worker> {
        let reservation = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: HELPER_BYTES,
            })
            .map_err(admission_error)?;
        let slot = self.clone();
        Worker::start_admitted("video-watchdog", reservation, move |stop| {
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
                "-max_alloc",
                "134217728",
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
                "fps=12,scale=w='max(1,min(120,trunc(80*dar)))':h='max(1,min(80,trunc(120/dar)))':flags=area,setsar=1,pad=120:80:(ow-iw)/2:(oh-ih)/2:black",
                "-pix_fmt",
                "gray",
                "-threads",
                "1",
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
            "setpts=PTS/0.25,fps=12,scale=40:16:flags=area,setsar=1"
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
            "crop=w='max(1,min(iw,ih*200/100/sar))':h='max(1,min(ih,iw*sar*100/200))':exact=1,scale=200:100:flags=area,setsar=1"
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
                "-max_alloc",
                "134217728",
                "-threads",
                "2",
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
    fn isolated_resources(
        threads: usize,
        bytes: usize,
    ) -> (
        ilium_execution::Execution,
        AmbientResources,
        ilium_execution::QuotaGroup,
    ) {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
        };
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 2,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
            worker_threads: threads,
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
    fn host_rejection_happens_before_native_spawn() {
        let (mut execution, resources, _) = isolated_resources(2, 32 * 1024 * 1024);
        let runner = SystemRunner::new(resources);
        let error = match runner.spawn(&CommandSpec {
            program: OsString::from("this-program-must-not-run"),
            args: Vec::new(),
        }) {
            Ok(_) => panic!("native child unexpectedly spawned"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(error.to_string().contains("host admission rejected"));
        drop(runner);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn failed_native_exit_keeps_credit_until_control_drop_and_exposes_bounded_stderr() {
        let (mut execution, resources, quota) = isolated_resources(10, 2 * 1024 * 1024 * 1024);
        let before = quota.snapshot().worker_bytes;
        let runner = SystemRunner::new(resources);
        let mut child = runner
            .spawn(&CommandSpec {
                program: OsString::from("/bin/sh"),
                args: ["-c", "printf 'invalid media' >&2; exit 7"]
                    .map(OsString::from)
                    .to_vec(),
            })
            .unwrap();
        let mut stdout = Vec::new();
        child.stdout.read_to_end(&mut stdout).unwrap();
        assert!(stdout.is_empty());
        assert!(child.control.reap(Duration::from_secs(2)));
        assert_eq!(child.control.successful_exit(), Some(false));
        assert!(quota.snapshot().worker_bytes >= before + NATIVE_BYTES);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !child.control.diagnostic_tail().contains("invalid media")
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(child.control.diagnostic_tail().contains("invalid media"));
        drop(child);
        assert!(quota.snapshot().worker_bytes < before + NATIVE_BYTES);
        drop(runner);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }

    /// Reject source-sized intermediate allocation while retaining a valid
    /// small picture. The native allocation ceiling keeps the RED run safe.
    #[test]
    #[ignore = "runs the real ffmpeg"]
    fn real_ffmpeg_extreme_sar_uses_bounded_intermediate_frames() {
        let runner = SystemRunner::new(crate::resources::test_resources());
        for fit in [FitMode::Fit, FitMode::Fill, FitMode::Stretch] {
            let mut args: Vec<OsString> = [
                "-hide_banner",
                "-nostdin",
                "-v",
                "error",
                "-max_alloc",
                "1048576",
                "-threads",
                "2",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=16x16:rate=6:duration=0.5,setsar=10000:max=10000",
                "-filter_threads",
                "1",
                "-vf",
            ]
            .map(OsString::from)
            .to_vec();
            args.push(filter_chain(120, 80, &options(fit)).into());
            args.extend(
                [
                    "-pix_fmt",
                    "gray",
                    "-threads",
                    "1",
                    "-frames:v",
                    "1",
                    "-f",
                    "rawvideo",
                    "pipe:1",
                ]
                .map(OsString::from),
            );
            let mut child = runner
                .spawn(&CommandSpec {
                    program: OsString::from(FFMPEG_PROGRAM),
                    args,
                })
                .expect("ffmpeg is installed and admitted");
            let mut output = Vec::new();
            child.stdout.read_to_end(&mut output).unwrap();
            assert!(child.control.reap(Duration::from_secs(2)));
            assert_eq!(
                child.control.successful_exit(),
                Some(true),
                "{fit:?}: {}",
                child.control.diagnostic_tail()
            );
            assert_eq!(output.len(), 9600, "{fit:?}");
        }
    }
}
