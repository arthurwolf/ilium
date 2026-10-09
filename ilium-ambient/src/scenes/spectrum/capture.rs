//! Audio capture for the spectrum scene.
//!
//! Design decision (researched 2026-09):
//! * Linux "system output" = the monitor of the default sink. Reading it
//!   through a helper process (`pw-record` or `parec`) is the most portable
//!   option: it works on PipeWire and PulseAudio alike, needs no link-time
//!   dependency, and the `cpal` ALSA backend cannot open monitors at all.
//!   `pw-record -P stream.capture.sink=true` follows the default sink when
//!   the user switches output devices, which `parec -d @DEFAULT_MONITOR@`
//!   (bound at connect time) does not, so PipeWire's tool is tried first.
//!   pw-record docs: <https://docs.pipewire.org/page_man_pw-record_1.html>;
//!   parec: <https://www.freedesktop.org/wiki/Software/PulseAudio/>.
//! * Windows (WASAPI loopback) and macOS (CoreAudio process tap aggregate,
//!   macOS 14.6+) use `cpal` 0.18: opening the default *output* device as an
//!   input transparently enables loopback (see cpal's `host::wasapi` and
//!   `host::coreaudio::macos::loopback`). Older macOS needs a virtual device
//!   such as BlackHole, selected by name.
//! * Microphone / named devices use the helpers on Linux (Pulse/PipeWire
//!   source names) with a `cpal` fallback, and `cpal` elsewhere.
//!
//! Blocking capture and analysis run on admitted `source::Worker` threads
//! owned by the scene. The CPAL callback has a separate retained worker
//! reservation, bounded callback size and nonblocking publication. Analysis
//! publishes immutable snapshots through a slot that `render` only reads with
//! `try_lock`.

use super::dsp::{band_edges, Analyzer};
use super::{BandScale, InputKind};
use crate::resources::{AmbientResources, WorkerCost, WorkerReservation};
use crate::source::{sleep_unless_stopped, Worker};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::Read;
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
#[cfg(any(target_os = "macos", test))]
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CAPTURE_WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;
const CAPTURE_WORKER_RESIDENT_BYTES: usize = 2 * 1024 * 1024;
const HELPER_READER_RESIDENT_BYTES: usize = 4 * 1024 * 1024;
const CPAL_CALLBACK_RESIDENT_BYTES: usize = 4 * 1024 * 1024;
#[cfg(any(target_os = "linux", target_os = "windows"))]
const CPAL_STREAM_THREADS: usize = 2;
#[cfg(target_os = "macos")]
const CPAL_STREAM_THREADS: usize = 4;
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
const CPAL_STREAM_THREADS: usize = 0;

/// Sample rate requested from the capture helpers.
pub const HELPER_SAMPLE_RATE: u32 = 44_100;
/// Number of newest mono samples published for the oscilloscope style.
pub const WAVEFORM_LEN: usize = 1024;
/// Analysis period in samples per second of audio: 60 analyses per second.
const ANALYSES_PER_SECOND: u32 = 60;

/// Interleaved sample encodings produced by capture helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmFormat {
    F32Le,
    // Decoded and tested, but no shipped helper command asks for it (both
    // helpers are configured for float32); kept for helpers that only do s16.
    #[allow(dead_code)]
    S16Le,
}

impl PcmFormat {
    pub fn bytes_per_sample(self) -> usize {
        match self {
            Self::F32Le => 4,
            Self::S16Le => 2,
        }
    }
}

/// Turns raw interleaved PCM bytes (arriving in arbitrary chunks) into mono
/// `f32` samples in -1..1. Bytes that do not complete a frame are kept for
/// the next call; non-finite samples become silence.
#[derive(Debug)]
pub struct PcmDecoder {
    format: PcmFormat,
    channels: usize,
    pending: Vec<u8>,
}

impl PcmDecoder {
    pub fn new(format: PcmFormat, channels: u16) -> Self {
        Self {
            format,
            channels: usize::from(channels.max(1)),
            pending: Vec::new(),
        }
    }

    fn sample(&self, bytes: &[u8]) -> f32 {
        let value = match self.format {
            PcmFormat::F32Le => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            PcmFormat::S16Le => f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0,
        };
        if value.is_finite() {
            value.clamp(-4.0, 4.0)
        } else {
            0.0
        }
    }

    pub fn push(&mut self, bytes: &[u8], mono: &mut Vec<f32>) {
        self.pending.extend_from_slice(bytes);
        let sample_bytes = self.format.bytes_per_sample();
        let frame_bytes = sample_bytes * self.channels;
        let complete = self.pending.len() / frame_bytes * frame_bytes;
        for frame in self.pending[..complete].chunks_exact(frame_bytes) {
            let sum: f32 = frame
                .chunks_exact(sample_bytes)
                .map(|sample| self.sample(sample))
                .sum();
            mono.push(sum / self.channels as f32);
        }
        self.pending.drain(..complete);
    }
}

/// Outcome of one `AudioSource::read`.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    /// Samples were appended to the output vector.
    Data,
    /// Nothing arrived within the wait; poll the stop flag and try again.
    Idle,
    /// The source ended or failed; the reason is user-facing.
    Ended(String),
}

/// A live mono sample stream. Lives entirely on the worker thread, so it need
/// not be `Send` (cpal streams are not on every platform).
pub trait AudioSource {
    fn sample_rate(&self) -> u32;
    /// Wait at most `wait` for samples and append mono samples to `out`.
    fn read(&mut self, out: &mut Vec<f32>, wait: Duration) -> ReadOutcome;
}

/// A helper command that writes raw PCM to stdout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureCommand {
    pub program: &'static str,
    pub args: Vec<String>,
    pub format: PcmFormat,
    pub channels: u16,
    pub sample_rate: u32,
}

/// What to capture, independent of the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTarget {
    SystemOutput,
    DefaultInput,
    Named(String),
}

impl CaptureTarget {
    pub fn from_settings(input: InputKind, device_name: &str) -> Self {
        match input {
            InputKind::SystemOutput => Self::SystemOutput,
            InputKind::Microphone => Self::DefaultInput,
            InputKind::NamedDevice => Self::Named(device_name.trim().to_owned()),
        }
    }
}

/// `pw-record` capture: raw float32 stereo. Without `--target`, `pw-record`
/// links to the default source, or, with `stream.capture.sink=true`, to the
/// monitor of the default sink (following changes of the default).
pub fn pipewire_command(target: &CaptureTarget) -> CaptureCommand {
    let mut args: Vec<String> = [
        "--raw",
        "--rate",
        "44100",
        "--channels",
        "2",
        "--format",
        "f32",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    args.extend(["--latency".to_owned(), "30ms".to_owned()]);
    match target {
        CaptureTarget::SystemOutput => {
            args.extend(["-P".to_owned(), "stream.capture.sink=true".to_owned()]);
        }
        CaptureTarget::DefaultInput => {}
        CaptureTarget::Named(name) => {
            // PulseAudio spells "monitor of output X" as `X.monitor`; PipeWire
            // targets the sink node `X` and asks for its capture side.
            match name
                .strip_suffix(".monitor")
                .filter(|sink| !sink.is_empty())
            {
                Some(sink) => args.extend([
                    "--target".to_owned(),
                    sink.to_owned(),
                    "-P".to_owned(),
                    "stream.capture.sink=true".to_owned(),
                ]),
                None => args.extend(["--target".to_owned(), name.clone()]),
            }
        }
    }
    args.push("-".to_owned());
    CaptureCommand {
        program: "pw-record",
        args,
        format: PcmFormat::F32Le,
        channels: 2,
        sample_rate: HELPER_SAMPLE_RATE,
    }
}

/// `parec` capture: raw float32 stereo on stdout.
pub fn pulse_command(target: &CaptureTarget) -> CaptureCommand {
    let device = match target {
        CaptureTarget::SystemOutput => "@DEFAULT_MONITOR@".to_owned(),
        CaptureTarget::DefaultInput => "@DEFAULT_SOURCE@".to_owned(),
        CaptureTarget::Named(name) => name.clone(),
    };
    let args = vec![
        "-d".to_owned(),
        device,
        "--format=float32le".to_owned(),
        format!("--rate={HELPER_SAMPLE_RATE}"),
        "--channels=2".to_owned(),
        "--latency-msec=30".to_owned(),
        "--client-name=ilium-ambient".to_owned(),
        "--stream-name=spectrum".to_owned(),
    ];
    CaptureCommand {
        program: "parec",
        args,
        format: PcmFormat::F32Le,
        channels: 2,
        sample_rate: HELPER_SAMPLE_RATE,
    }
}

/// One way of obtaining audio; the worker tries plans in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourcePlan {
    Process(CaptureCommand),
    Cpal(CaptureTarget),
}

impl SourcePlan {
    #[cfg(test)]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Process(command) => command.program,
            Self::Cpal(_) => "cpal",
        }
    }
}

/// Ordered capture plans for a target on this platform. `has_program`
/// reports whether an executable is on `PATH`. `Err` is a user-facing reason
/// nothing can be tried.
pub fn plan_sources(
    target: &CaptureTarget,
    is_linux: bool,
    has_program: &dyn Fn(&str) -> bool,
) -> Result<Vec<SourcePlan>, String> {
    if let CaptureTarget::Named(name) = target {
        if name.is_empty() {
            return Err("no device name set".to_owned());
        }
    }
    if !is_linux {
        return Ok(vec![SourcePlan::Cpal(target.clone())]);
    }
    let mut plans = Vec::new();
    let pipewire = pipewire_command(target);
    let pulse = pulse_command(target);
    // System output prefers pw-record (follows default-sink changes); the
    // other targets prefer parec (its device names are Pulse source names).
    let ordered = if *target == CaptureTarget::SystemOutput {
        [pipewire, pulse]
    } else {
        [pulse, pipewire]
    };
    for command in ordered {
        if has_program(command.program) {
            plans.push(SourcePlan::Process(command));
        }
    }
    if *target != CaptureTarget::SystemOutput {
        plans.push(SourcePlan::Cpal(target.clone()));
    }
    if plans.is_empty() {
        return Err(
            "neither pw-record nor parec found (install pipewire-bin or pulseaudio-utils)"
                .to_owned(),
        );
    }
    Ok(plans)
}

/// True when an executable called `program` exists in a `PATH` directory.
pub fn program_on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(program);
        candidate.is_file() || candidate.with_extension("exe").is_file()
    })
}

enum Chunk {
    Samples(Vec<f32>),
    Ended(String),
}

/// Shared receive logic of the channel-backed sources. Samples that arrive
/// together with an end marker are delivered first; the end is reported on
/// the next call so it is never lost.
fn receive_chunks(
    receiver: &Receiver<Chunk>,
    pending_end: &mut Option<String>,
    out: &mut Vec<f32>,
    wait: Duration,
    disconnected: &str,
) -> ReadOutcome {
    if let Some(reason) = pending_end.take() {
        return ReadOutcome::Ended(reason);
    }
    let first = match receiver.recv_timeout(wait) {
        Ok(chunk) => chunk,
        Err(RecvTimeoutError::Timeout) => return ReadOutcome::Idle,
        Err(RecvTimeoutError::Disconnected) => return ReadOutcome::Ended(disconnected.to_owned()),
    };
    let mut next = Some(first);
    while let Some(chunk) = next.take() {
        match chunk {
            Chunk::Samples(samples) => out.extend(samples),
            Chunk::Ended(reason) => {
                if out.is_empty() {
                    return ReadOutcome::Ended(reason);
                }
                *pending_end = Some(reason);
                break;
            }
        }
        next = receiver.try_recv().ok();
    }
    ReadOutcome::Data
}

/// A capture helper child process. A reader thread decodes stdout and feeds a
/// bounded channel; dropping the source kills the child and joins the reader,
/// so nothing outlives the scene.
pub struct ProcessSource {
    child: Child,
    receiver: Option<Receiver<Chunk>>,
    reader: Option<Worker>,
    _helper_reservation: WorkerReservation,
    pending_end: Option<String>,
    sample_rate: u32,
}

impl ProcessSource {
    pub fn spawn(command: &CaptureCommand, resources: &AmbientResources) -> Result<Self, String> {
        // Reserve both the helper process role and its blocking pipe reader
        // before creating the child or any channels.
        let helper_reservation = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: HELPER_READER_RESIDENT_BYTES,
            })
            .map_err(|error| format!("audio helper admission refused: {error:?}"))?;
        let reader_reservation = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: HELPER_READER_RESIDENT_BYTES,
            })
            .map_err(|error| format!("audio pipe reader admission refused: {error:?}"))?;
        let mut child = Command::new(command.program)
            .args(&command.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("cannot start {}: {error}", command.program))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (Some(stdout), Some(stderr)) = (stdout, stderr) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{} gave no output pipe", command.program));
        };
        let (sender, receiver) = sync_channel(64);
        let decoder = PcmDecoder::new(command.format, command.channels);
        let program = command.program;
        let reader = Worker::start_admitted_with_stack(
            "spectrum-reader",
            reader_reservation,
            Some(CAPTURE_WORKER_STACK_BYTES),
            move |_| read_pipe(stdout, stderr, decoder, sender, program),
        )
        .map_err(|error| {
            let _ = child.kill();
            let _ = child.wait();
            format!("cannot start reader thread: {error}")
        })?;
        Ok(Self {
            child,
            receiver: Some(receiver),
            reader: Some(reader),
            _helper_reservation: helper_reservation,
            pending_end: None,
            sample_rate: command.sample_rate,
        })
    }
}

/// Reader thread body. Stderr is only read after stdout closed (helpers are
/// silent while running, so the pipe cannot fill up in practice).
fn read_pipe(
    mut stdout: ChildStdout,
    mut stderr: ChildStderr,
    mut decoder: PcmDecoder,
    sender: SyncSender<Chunk>,
    program: &'static str,
) {
    let mut bytes = [0u8; 8192];
    loop {
        match stdout.read(&mut bytes) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                let mut mono = Vec::with_capacity(count / 4);
                decoder.push(&bytes[..count], &mut mono);
                match sender.try_send(Chunk::Samples(mono)) {
                    // A full queue means the analyser is behind: drop audio, keep going.
                    Ok(()) | Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => return,
                }
            }
        }
    }
    let mut message = String::new();
    let _ = stderr.by_ref().take(600).read_to_string(&mut message);
    let message = message.trim();
    let reason = if message.is_empty() {
        format!("{program} stopped")
    } else {
        format!("{program}: {}", message.lines().next().unwrap_or(message))
    };
    let _ = sender.send(Chunk::Ended(reason));
}

impl AudioSource for ProcessSource {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn read(&mut self, out: &mut Vec<f32>, wait: Duration) -> ReadOutcome {
        let Some(receiver) = self.receiver.as_ref() else {
            return ReadOutcome::Ended("capture closed".to_owned());
        };
        receive_chunks(
            receiver,
            &mut self.pending_end,
            out,
            wait,
            "capture helper stopped",
        )
    }
}

impl Drop for ProcessSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Dropping the receiver unblocks a reader stuck on a full channel.
        self.receiver = None;
        if let Some(reader) = self.reader.take().and_then(Worker::request_stop) {
            // The killed child closed stdout. Observe the original reader for a
            // bounded interval; any survivor remains in platform retirement.
            if let Err(error) = reader.join_until(Instant::now() + Duration::from_secs(2)) {
                tracing::warn!(%error, "spectrum helper reader remains under retirement supervision");
            }
        }
    }
}

/// A `cpal` input stream (microphone, WASAPI loopback, macOS loopback).
pub struct CpalSource {
    _stream: cpal::Stream,
    _callback_reservation: Option<WorkerReservation>,
    receiver: Receiver<Chunk>,
    pending_end: Option<String>,
    sample_rate: u32,
}

fn find_device(host: &cpal::Host, target: &CaptureTarget) -> Result<(cpal::Device, bool), String> {
    match target {
        // Opening the default OUTPUT device as an input enables loopback on
        // Windows and macOS; the flag tells the caller to use output config.
        CaptureTarget::SystemOutput => host
            .default_output_device()
            .map(|device| (device, true))
            .ok_or_else(|| "no default output device".to_owned()),
        CaptureTarget::DefaultInput => host
            .default_input_device()
            .map(|device| (device, false))
            .ok_or_else(|| "no default input device".to_owned()),
        CaptureTarget::Named(name) => {
            let wanted = name.to_lowercase();
            let devices = host
                .devices()
                .map_err(|error| format!("cannot list audio devices: {error}"))?;
            let mut loopback_match = None;
            for device in devices {
                let Ok(description) = device.description() else {
                    continue;
                };
                if !description.name().to_lowercase().contains(&wanted) {
                    continue;
                }
                if device.supports_input() {
                    return Ok((device, false));
                }
                loopback_match.get_or_insert(device);
            }
            loopback_match
                .map(|device| (device, true))
                .ok_or_else(|| format!("no audio device matching \"{name}\""))
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sender: SyncSender<Chunk>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let channels = usize::from(config.channels.max(1));
    let error_sender = sender.clone();
    device
        .build_input_stream(
            *config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mono = downmix_callback(data, channels, |sample| {
                    let value: f32 = cpal::Sample::to_sample(*sample);
                    value
                });
                let Some(mono) = mono else { return };
                // Never block the audio thread: a full queue drops this buffer.
                let _ = sender.try_send(Chunk::Samples(mono));
            },
            move |error| {
                let _ = error_sender.try_send(Chunk::Ended(format!("audio stream error: {error}")));
            },
            None,
        )
        .map_err(|error| format!("cannot open audio stream: {error}"))
}

const MAX_CALLBACK_FRAMES: usize = 8192;

fn downmix_callback<T>(
    data: &[T],
    channels: usize,
    convert: impl Fn(&T) -> f32,
) -> Option<Vec<f32>> {
    let channels = channels.max(1);
    if data.len().div_ceil(channels) > MAX_CALLBACK_FRAMES {
        return None;
    }
    Some(
        data.chunks(channels)
            .map(|frame| {
                let sum: f32 = frame
                    .iter()
                    .map(|sample| {
                        let value = convert(sample);
                        if value.is_finite() {
                            value.clamp(-4.0, 4.0)
                        } else {
                            0.0
                        }
                    })
                    .sum();
                sum / channels as f32
            })
            .collect(),
    )
}

impl CpalSource {
    pub fn open(target: &CaptureTarget, resources: &AmbientResources) -> Result<Self, String> {
        if CPAL_STREAM_THREADS == 0 {
            return Err(
                "audio callback thread accounting is unavailable on this platform".to_owned(),
            );
        }
        let callback_reservation = resources
            .reserve_worker(WorkerCost {
                threads: CPAL_STREAM_THREADS,
                resident_bytes: CPAL_CALLBACK_RESIDENT_BYTES,
            })
            .map_err(|error| format!("audio callback admission refused: {error:?}"))?;
        let host = cpal::default_host();
        let (device, loopback) = find_device(&host, target)?;
        let supported = if loopback {
            device.default_output_config()
        } else {
            device.default_input_config()
        }
        .map_err(|error| format!("no usable audio format: {error}"))?;
        let sample_format = supported.sample_format();
        let sample_rate = supported.sample_rate();
        let config: cpal::StreamConfig = supported.into();
        let (sender, receiver) = sync_channel(64);
        let stream = match sample_format {
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config, sender),
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config, sender),
            cpal::SampleFormat::I32 => build_stream::<i32>(&device, &config, sender),
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config, sender),
            other => Err(format!("unsupported sample format {other}")),
        }?;
        stream
            .play()
            .map_err(|error| format!("cannot start audio stream: {error}"))?;
        Ok(Self {
            _stream: stream,
            _callback_reservation: Some(callback_reservation),
            receiver,
            pending_end: None,
            sample_rate,
        })
    }
}

#[cfg(any(target_os = "macos", test))]
fn retain_cpal_reservation_until_process_exit(reservation: WorkerReservation) {
    // CoreAudio's disconnect monitors are not joinable through cpal. Keep the
    // admission charged after stream destruction instead of claiming those
    // opaque workers exited. The shared process quota bounds retained entries.
    static RETIRED: OnceLock<Mutex<Vec<WorkerReservation>>> = OnceLock::new();
    RETIRED
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(reservation);
}

#[cfg(target_os = "macos")]
impl Drop for CpalSource {
    fn drop(&mut self) {
        if let Some(reservation) = self._callback_reservation.take() {
            // This runs before `_stream` is dropped, so the charge remains held
            // through backend teardown and for the rest of the process lifetime.
            retain_cpal_reservation_until_process_exit(reservation);
        }
    }
}

impl AudioSource for CpalSource {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn read(&mut self, out: &mut Vec<f32>, wait: Duration) -> ReadOutcome {
        receive_chunks(
            &self.receiver,
            &mut self.pending_end,
            out,
            wait,
            "audio stream stopped",
        )
    }
}

/// One published analysis result. Immutable once shared.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Increments with every analysis; 0 means "nothing yet".
    pub seq: u64,
    pub bands_db: Vec<f32>,
    /// Newest mono samples, oldest first, `WAVEFORM_LEN` long once available.
    pub waveform: Vec<f32>,
    pub level_db: f32,
}

impl Snapshot {
    pub fn empty() -> Self {
        Self {
            seq: 0,
            bands_db: Vec::new(),
            waveform: Vec::new(),
            level_db: super::dsp::DB_FLOOR,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerStatus {
    Starting,
    Running,
    Failed(String),
}

pub struct SharedState {
    pub snapshot: Arc<Snapshot>,
    pub status: WorkerStatus,
}

pub type Shared = Arc<Mutex<SharedState>>;

pub fn new_shared() -> Shared {
    Arc::new(Mutex::new(SharedState {
        snapshot: Arc::new(Snapshot::empty()),
        status: WorkerStatus::Starting,
    }))
}

fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, SharedState> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Analysis parameters the worker needs; a subset of the user settings.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisConfig {
    pub fft_size: usize,
    pub bands: usize,
    pub scale: BandScale,
    pub min_hz: f32,
    pub max_hz: f32,
    pub tilt_db_per_octave: f32,
}

/// Creates the source for attempt number `attempt` on the worker thread.
pub type SourceFactory = Box<dyn FnMut(usize) -> Result<Box<dyn AudioSource>, String> + Send>;

/// Factory that walks the plans for `target`, resolved lazily on the worker
/// (PATH lookups stay off the render thread).
pub fn plan_factory(target: CaptureTarget, resources: AmbientResources) -> SourceFactory {
    let mut plans: Vec<SourcePlan> = Vec::new();
    Box::new(move |attempt| {
        if attempt == 0 || plans.is_empty() {
            plans = plan_sources(&target, cfg!(target_os = "linux"), &program_on_path)?;
        }
        match &plans[attempt % plans.len()] {
            SourcePlan::Process(command) => {
                Ok(Box::new(ProcessSource::spawn(command, &resources)?) as Box<dyn AudioSource>)
            }
            SourcePlan::Cpal(target) => {
                Ok(Box::new(CpalSource::open(target, &resources)?) as Box<dyn AudioSource>)
            }
        }
    })
}

/// Spawn the capture worker. `factory` runs on the worker thread.
pub fn spawn_capture(
    resources: &AmbientResources,
    shared: Shared,
    config: AnalysisConfig,
    mut factory: SourceFactory,
) -> Result<Worker, String> {
    let reservation = resources
        .reserve_worker(WorkerCost {
            threads: 1,
            resident_bytes: CAPTURE_WORKER_RESIDENT_BYTES,
        })
        .map_err(|error| format!("spectrum analysis admission refused: {error:?}"))?;
    Worker::start_admitted_with_stack(
        "spectrum",
        reservation,
        Some(CAPTURE_WORKER_STACK_BYTES),
        move |stop| capture_loop(&stop, &shared, &config, &mut factory),
    )
    .map_err(|error| format!("spectrum analysis worker could not start: {error}"))
}

/// Pause between capture attempts, honouring stop within ~100 ms.
const RETRY_SECONDS: u64 = 4;

fn capture_loop(
    stop: &AtomicBool,
    shared: &Shared,
    config: &AnalysisConfig,
    factory: &mut SourceFactory,
) {
    let mut attempt = 0usize;
    while !stop.load(Ordering::Relaxed) {
        let mut produced_data = false;
        let failure = match factory(attempt) {
            Ok(mut source) => pump(stop, shared, config, source.as_mut(), &mut produced_data),
            Err(reason) => reason,
        };
        if stop.load(Ordering::Relaxed) {
            return;
        }
        lock(shared).status = WorkerStatus::Failed(failure);
        // A source that worked for a while is retried; a failing one hands
        // over to the next plan.
        if !produced_data {
            attempt += 1;
        }
        if !sleep_unless_stopped(stop, Duration::from_secs(RETRY_SECONDS)) {
            return;
        }
    }
}

/// Read, analyse and publish until the source ends or a stop is requested.
/// Returns the reason the source ended.
fn pump(
    stop: &AtomicBool,
    shared: &Shared,
    config: &AnalysisConfig,
    source: &mut dyn AudioSource,
    produced_data: &mut bool,
) -> String {
    let edges = band_edges(config.scale, config.bands, config.min_hz, config.max_hz);
    let rate = source.sample_rate().max(8_000);
    let mut analyzer = match Analyzer::new(config.fft_size, rate, &edges, config.tilt_db_per_octave)
    {
        Ok(analyzer) => analyzer,
        Err(reason) => return reason,
    };
    let hop = (rate / ANALYSES_PER_SECOND).max(64) as usize;
    let keep = config.fft_size.max(WAVEFORM_LEN);
    let mut ring: Vec<f32> = Vec::with_capacity(keep * 2);
    let mut fresh = 0usize;
    let mut sequence = 0u64;
    let mut chunk = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        chunk.clear();
        match source.read(&mut chunk, Duration::from_millis(50)) {
            ReadOutcome::Idle => continue,
            ReadOutcome::Ended(reason) => return reason,
            ReadOutcome::Data => {}
        }
        if !*produced_data {
            *produced_data = true;
            lock(shared).status = WorkerStatus::Running;
        }
        fresh += chunk.len();
        ring.extend_from_slice(&chunk);
        if ring.len() > keep * 2 {
            let excess = ring.len() - keep;
            ring.drain(..excess);
        }
        if ring.len() < config.fft_size || fresh < hop {
            continue;
        }
        fresh = 0;
        let analysis = analyzer.analyze(&ring);
        sequence += 1;
        let waveform = ring[ring.len().saturating_sub(WAVEFORM_LEN)..].to_vec();
        let snapshot = Arc::new(Snapshot {
            seq: sequence,
            bands_db: analysis.bands_db,
            waveform,
            level_db: analysis.level_db,
        });
        lock(shared).snapshot = snapshot;
    }
    "stopped".to_owned()
}

#[cfg(test)]
pub mod testing {
    //! Fake sources for tests: no audio hardware, no child processes.
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// Endless sine wave delivered in small chunks with a short pause.
    pub struct SineSource {
        pub frequency: f32,
        pub amplitude: f32,
        pub rate: u32,
        pub produced: usize,
        pub dropped: Arc<AtomicBool>,
        pub reads: Arc<AtomicUsize>,
    }

    impl SineSource {
        pub fn new(frequency: f32, amplitude: f32) -> (Self, Arc<AtomicBool>) {
            let dropped = Arc::new(AtomicBool::new(false));
            (
                Self {
                    frequency,
                    amplitude,
                    rate: 44_100,
                    produced: 0,
                    dropped: Arc::clone(&dropped),
                    reads: Arc::new(AtomicUsize::new(0)),
                },
                dropped,
            )
        }
    }

    impl AudioSource for SineSource {
        fn sample_rate(&self) -> u32 {
            self.rate
        }

        fn read(&mut self, out: &mut Vec<f32>, wait: Duration) -> ReadOutcome {
            self.reads.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(2).min(wait));
            for _ in 0..1024 {
                let phase =
                    2.0 * std::f64::consts::PI * f64::from(self.frequency) * self.produced as f64
                        / f64::from(self.rate);
                out.push(self.amplitude * phase.sin() as f32);
                self.produced += 1;
            }
            ReadOutcome::Data
        }
    }

    impl Drop for SineSource {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    /// Music-like signal: a harmonic stack with a slow tremolo, a beat-like
    /// bass pulse and a little noise, for visual review of the styles.
    pub struct MusicSource {
        pub produced: usize,
        noise_state: u32,
    }

    impl MusicSource {
        pub fn new() -> Self {
            Self {
                produced: 0,
                noise_state: 0x2545_f491,
            }
        }
    }

    impl AudioSource for MusicSource {
        fn sample_rate(&self) -> u32 {
            44_100
        }

        fn read(&mut self, out: &mut Vec<f32>, wait: Duration) -> ReadOutcome {
            std::thread::sleep(Duration::from_millis(2).min(wait));
            let tau = 2.0 * std::f64::consts::PI;
            for _ in 0..1024 {
                let t = self.produced as f64 / 44_100.0;
                let beat = (0.5 + 0.5 * (tau * 2.0 * t).cos()).powi(4);
                let mut value =
                    0.5 * beat * (tau * 55.0 * t).sin() + 0.25 * beat * (tau * 110.0 * t).sin();
                for (harmonic, frequency) in [
                    220.0, 330.0, 440.0, 660.0, 880.0, 1320.0, 1760.0, 3520.0, 7040.0,
                ]
                .iter()
                .enumerate()
                {
                    let tremolo = 0.6 + 0.4 * (tau * (0.3 + harmonic as f64 * 0.17) * t).sin();
                    value += 0.16 / (1.0 + harmonic as f64 * 0.7)
                        * tremolo
                        * (tau * frequency * t).sin();
                }
                self.noise_state = self
                    .noise_state
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
                value += 0.03 * ((self.noise_state >> 8) as f64 / (1u64 << 24) as f64 - 0.5);
                out.push(value as f32 * 0.6);
                self.produced += 1;
            }
            ReadOutcome::Data
        }
    }

    /// Feeds raw PCM byte chunks (as a helper process would write them,
    /// deliberately split at odd boundaries) through `PcmDecoder`.
    pub struct PcmBytesSource {
        chunks: std::collections::VecDeque<Vec<u8>>,
        decoder: PcmDecoder,
        rate: u32,
    }

    impl PcmBytesSource {
        pub fn new(bytes: &[u8], format: PcmFormat, channels: u16, rate: u32) -> Self {
            Self {
                chunks: bytes.chunks(4093).map(<[u8]>::to_vec).collect(),
                decoder: PcmDecoder::new(format, channels),
                rate,
            }
        }
    }

    impl AudioSource for PcmBytesSource {
        fn sample_rate(&self) -> u32 {
            self.rate
        }

        fn read(&mut self, out: &mut Vec<f32>, wait: Duration) -> ReadOutcome {
            match self.chunks.pop_front() {
                Some(chunk) => {
                    self.decoder.push(&chunk, out);
                    std::thread::sleep(Duration::from_millis(1));
                    ReadOutcome::Data
                }
                None => {
                    std::thread::sleep(wait.min(Duration::from_millis(10)));
                    ReadOutcome::Ended("byte stream finished".to_owned())
                }
            }
        }
    }

    /// Source that never produces samples but records that it was dropped.
    pub struct SilentSource {
        pub dropped: Arc<AtomicBool>,
    }

    impl AudioSource for SilentSource {
        fn sample_rate(&self) -> u32 {
            44_100
        }

        fn read(&mut self, _out: &mut Vec<f32>, wait: Duration) -> ReadOutcome {
            std::thread::sleep(wait.min(Duration::from_millis(20)));
            ReadOutcome::Idle
        }
    }

    impl Drop for SilentSource {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use std::time::Instant;

    #[test]
    fn cpal_callback_rejects_more_than_the_bounded_frame_count() {
        let samples = vec![0.25f32; (MAX_CALLBACK_FRAMES + 1) * 2];
        let converted = std::cell::Cell::new(0);
        assert!(downmix_callback(&samples, 2, |sample| {
            converted.set(converted.get() + 1);
            *sample
        })
        .is_none());
        assert_eq!(converted.get(), 0, "reject before converting or allocating");
    }

    #[test]
    fn retained_backend_reservation_stays_charged_until_process_exit() {
        let (_execution, resources) = crate::resources::isolated_test_resources();
        let quota = resources.finite().quota_group();
        let before = quota.snapshot().worker_threads;
        let reservation = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: 1024,
            })
            .unwrap();

        retain_cpal_reservation_until_process_exit(reservation);

        assert_eq!(
            quota.snapshot().worker_threads,
            before + 1,
            "unobservable backend workers remain charged until process exit"
        );
        assert_eq!(quota.snapshot().worker_bytes, 1024);
    }

    #[test]
    fn decoder_handles_f32_stereo_split_at_arbitrary_byte_boundaries() {
        let frames = [(0.5f32, -0.5f32), (1.0, 0.0), (0.25, 0.75), (-1.0, -1.0)];
        let mut bytes = Vec::new();
        for (left, right) in frames {
            bytes.extend_from_slice(&left.to_le_bytes());
            bytes.extend_from_slice(&right.to_le_bytes());
        }
        // Feed 3, 7 and 5 byte pieces, then the rest, to cross frame boundaries.
        let mut decoder = PcmDecoder::new(PcmFormat::F32Le, 2);
        let mut mono = Vec::new();
        let (a, rest) = bytes.split_at(3);
        let (b, rest) = rest.split_at(7);
        let (c, d) = rest.split_at(5);
        for piece in [a, b, c, d] {
            decoder.push(piece, &mut mono);
        }
        assert_eq!(mono, vec![0.0, 0.5, 0.5, -1.0]);
    }

    #[test]
    fn decoder_handles_s16_mono_and_sanitizes_bad_floats() {
        let mut decoder = PcmDecoder::new(PcmFormat::S16Le, 1);
        let mut mono = Vec::new();
        let mut bytes = Vec::new();
        for value in [0i16, 16_384, -32_768, 32_767] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        decoder.push(&bytes, &mut mono);
        assert_eq!(mono.len(), 4);
        assert!((mono[1] - 0.5).abs() < 1e-6 && (mono[2] + 1.0).abs() < 1e-6);
        let mut decoder = PcmDecoder::new(PcmFormat::F32Le, 1);
        let mut mono = Vec::new();
        let mut bytes = Vec::new();
        for value in [f32::NAN, f32::INFINITY, 1e9, 0.25] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        decoder.push(&bytes, &mut mono);
        assert_eq!(mono, vec![0.0, 0.0, 4.0, 0.25]);
    }

    #[test]
    fn pulse_and_pipewire_commands_target_the_right_devices() {
        let pulse = pulse_command(&CaptureTarget::SystemOutput);
        assert_eq!(pulse.program, "parec");
        assert!(pulse
            .args
            .windows(2)
            .any(|pair| pair == ["-d", "@DEFAULT_MONITOR@"]));
        assert!(pulse.args.contains(&"--format=float32le".to_owned()));
        assert!(pulse.args.contains(&"--rate=44100".to_owned()));
        assert!(pulse.args.contains(&"--latency-msec=30".to_owned()));
        let mic = pulse_command(&CaptureTarget::DefaultInput);
        assert!(mic
            .args
            .windows(2)
            .any(|pair| pair == ["-d", "@DEFAULT_SOURCE@"]));
        let named = pulse_command(&CaptureTarget::Named("my.monitor".to_owned()));
        assert!(named
            .args
            .windows(2)
            .any(|pair| pair == ["-d", "my.monitor"]));

        let system = pipewire_command(&CaptureTarget::SystemOutput);
        assert_eq!(system.program, "pw-record");
        assert!(system.args.contains(&"--raw".to_owned()));
        assert!(system
            .args
            .windows(2)
            .any(|pair| pair == ["-P", "stream.capture.sink=true"]));
        assert_eq!(system.args.last().map(String::as_str), Some("-"));
        assert!(!system.args.contains(&"--target".to_owned()));
        let mic = pipewire_command(&CaptureTarget::DefaultInput);
        assert!(!mic.args.iter().any(|arg| arg.contains("capture.sink")));
        let named = pipewire_command(&CaptureTarget::Named("alsa_input.x".to_owned()));
        assert!(named
            .args
            .windows(2)
            .any(|pair| pair == ["--target", "alsa_input.x"]));
        let monitor = pipewire_command(&CaptureTarget::Named("alsa_output.y.monitor".to_owned()));
        assert!(monitor
            .args
            .windows(2)
            .any(|pair| pair == ["--target", "alsa_output.y"]));
        assert!(monitor
            .args
            .windows(2)
            .any(|pair| pair == ["-P", "stream.capture.sink=true"]));
    }

    #[test]
    fn plan_order_prefers_pipewire_for_system_output_and_falls_back() {
        let both = |_: &str| true;
        let plans = plan_sources(&CaptureTarget::SystemOutput, true, &both).unwrap();
        assert_eq!(
            plans.iter().map(SourcePlan::label).collect::<Vec<_>>(),
            ["pw-record", "parec"]
        );
        let only_parec = |name: &str| name == "parec";
        let plans = plan_sources(&CaptureTarget::SystemOutput, true, &only_parec).unwrap();
        assert_eq!(
            plans.iter().map(SourcePlan::label).collect::<Vec<_>>(),
            ["parec"]
        );
        let error = plan_sources(&CaptureTarget::SystemOutput, true, &|_: &str| false).unwrap_err();
        assert!(error.contains("pw-record") && error.contains("parec"));
        // Microphone: parec first, cpal as the last resort even with no helpers.
        let plans = plan_sources(&CaptureTarget::DefaultInput, true, &|_: &str| false).unwrap();
        assert_eq!(
            plans.iter().map(SourcePlan::label).collect::<Vec<_>>(),
            ["cpal"]
        );
        let plans = plan_sources(&CaptureTarget::DefaultInput, true, &both).unwrap();
        assert_eq!(
            plans.iter().map(SourcePlan::label).collect::<Vec<_>>(),
            ["parec", "pw-record", "cpal"]
        );
        // Other platforms: cpal only, whatever the target.
        let plans = plan_sources(&CaptureTarget::SystemOutput, false, &both).unwrap();
        assert_eq!(plans, vec![SourcePlan::Cpal(CaptureTarget::SystemOutput)]);
        let error = plan_sources(&CaptureTarget::Named(String::new()), true, &both).unwrap_err();
        assert!(error.contains("device name"));
    }

    #[test]
    fn capture_target_follows_the_input_setting() {
        assert_eq!(
            CaptureTarget::from_settings(InputKind::SystemOutput, "x"),
            CaptureTarget::SystemOutput
        );
        assert_eq!(
            CaptureTarget::from_settings(InputKind::Microphone, "x"),
            CaptureTarget::DefaultInput
        );
        assert_eq!(
            CaptureTarget::from_settings(InputKind::NamedDevice, "  BlackHole 2ch "),
            CaptureTarget::Named("BlackHole 2ch".to_owned())
        );
    }

    fn analysis_config() -> AnalysisConfig {
        AnalysisConfig {
            fft_size: 2048,
            bands: 32,
            scale: BandScale::Log,
            min_hz: 40.0,
            max_hz: 16_000.0,
            tilt_db_per_octave: 0.0,
        }
    }

    fn wait_for(shared: &Shared, condition: impl Fn(&SharedState) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if condition(&lock(shared)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn worker_publishes_analysis_of_an_injected_sine_and_stops_on_drop() {
        let (source, dropped) = SineSource::new(1000.0, 0.5);
        let mut source = Some(source);
        let shared = new_shared();
        let (thread_name_tx, thread_name_rx) = std::sync::mpsc::sync_channel(1);
        let factory: SourceFactory = Box::new(move |_| {
            let _ = thread_name_tx.try_send(
                std::thread::current()
                    .name()
                    .unwrap_or("unnamed")
                    .to_owned(),
            );
            source
                .take()
                .map(|source| Box::new(source) as Box<dyn AudioSource>)
                .ok_or_else(|| "used twice".to_owned())
        });
        let worker = spawn_capture(
            &crate::resources::test_resources(),
            Arc::clone(&shared),
            analysis_config(),
            factory,
        )
        .unwrap();
        assert_eq!(
            thread_name_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "ilium-ambient-spectrum"
        );
        assert!(wait_for(&shared, |state| state.snapshot.seq >= 3));
        {
            let state = lock(&shared);
            assert_eq!(state.status, WorkerStatus::Running);
            assert_eq!(state.snapshot.bands_db.len(), 32);
            assert_eq!(state.snapshot.waveform.len(), WAVEFORM_LEN);
            let edges = band_edges(BandScale::Log, 32, 40.0, 16_000.0);
            let loudest = state
                .snapshot
                .bands_db
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(index, _)| index)
                .unwrap();
            assert!(edges[loudest] - 1.0 <= 1000.0 && 1000.0 <= edges[loudest + 1] + 1.0);
            assert!(
                (state.snapshot.level_db + 9.0).abs() < 0.5,
                "{}",
                state.snapshot.level_db
            );
        }
        let started = Instant::now();
        let ticket = worker.join_observer().unwrap();
        drop(worker);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "nonblocking drop"
        );
        assert_eq!(
            ticket
                .join_until(Instant::now() + Duration::from_secs(1))
                .unwrap(),
            ilium_platform::owned_worker::WorkerExit::Joined
        );
        assert!(
            dropped.load(Ordering::SeqCst),
            "source dropped by the supervised worker before join returned"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "prompt shutdown"
        );
    }

    #[test]
    fn capture_worker_refuses_when_shared_physical_quota_is_full() {
        let (_execution, resources) = crate::resources::isolated_test_resources();
        let quota = resources.finite().quota_group();
        let mut reservations = Vec::new();
        loop {
            let snapshot = quota.snapshot();
            if snapshot.worker_threads == snapshot.limits.worker_threads
                || snapshot.worker_bytes == snapshot.limits.worker_bytes
            {
                break;
            }
            let Ok(reservation) = resources.reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: 1,
            }) else {
                break;
            };
            reservations.push(reservation);
        }
        let shared = new_shared();
        let result = spawn_capture(
            &resources,
            shared,
            analysis_config(),
            Box::new(|_| panic!("refused capture must not run")),
        );
        assert!(matches!(result, Err(error) if error.contains("admission refused")));
        drop(reservations);
    }

    fn stereo_sine_bytes(format: PcmFormat, frequency: f64, rate: u32, frames: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for n in 0..frames {
            let sample =
                0.5 * (2.0 * std::f64::consts::PI * frequency * n as f64 / f64::from(rate)).sin();
            // Left carries the tone, right is its inverse-free copy at half level.
            for value in [sample, sample * 0.5] {
                match format {
                    PcmFormat::F32Le => bytes.extend_from_slice(&(value as f32).to_le_bytes()),
                    PcmFormat::S16Le => {
                        bytes.extend_from_slice(&((value * 32_767.0) as i16).to_le_bytes())
                    }
                }
            }
        }
        bytes
    }

    #[test]
    fn injected_helper_bytes_reach_the_analysis_as_float32_and_int16() {
        for format in [PcmFormat::F32Le, PcmFormat::S16Le] {
            let bytes = stereo_sine_bytes(format, 2000.0, 44_100, 44_100 / 2);
            let source = PcmBytesSource::new(&bytes, format, 2, 44_100);
            let mut source = Some(source);
            let shared = new_shared();
            let factory: SourceFactory = Box::new(move |_| {
                source
                    .take()
                    .map(|source| Box::new(source) as Box<dyn AudioSource>)
                    .ok_or_else(|| "byte stream finished".to_owned())
            });
            let worker = spawn_capture(
                &crate::resources::test_resources(),
                Arc::clone(&shared),
                analysis_config(),
                factory,
            )
            .unwrap();
            // The stream ends after ~0.5 s: wait for that, then inspect the last snapshot.
            assert!(
                wait_for(&shared, |state| matches!(
                    state.status,
                    WorkerStatus::Failed(_)
                )),
                "{format:?}"
            );
            let state = lock(&shared);
            assert_eq!(
                state.status,
                WorkerStatus::Failed("byte stream finished".to_owned())
            );
            assert!(
                state.snapshot.seq > 10,
                "{format:?}: {} analyses",
                state.snapshot.seq
            );
            let edges = band_edges(BandScale::Log, 32, 40.0, 16_000.0);
            let loudest = state
                .snapshot
                .bands_db
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(index, _)| index)
                .unwrap();
            assert!(
                edges[loudest] - 5.0 <= 2000.0 && 2000.0 <= edges[loudest + 1] + 5.0,
                "{format:?}: band {loudest}"
            );
            // Mono mix of 0.5 + 0.25 amplitude channels is 0.375: about -8.5 dBFS peak, -11.5 dBFS RMS.
            assert!(
                (state.snapshot.level_db + 11.5).abs() < 1.0,
                "{format:?}: {} dB",
                state.snapshot.level_db
            );
            drop(state);
            drop(worker);
        }
    }

    #[test]
    fn worker_reports_failure_and_stops_promptly_during_retry_sleep() {
        let shared = new_shared();
        let factory: SourceFactory = Box::new(|_| Err("no such device".to_owned()));
        let worker = spawn_capture(
            &crate::resources::test_resources(),
            Arc::clone(&shared),
            analysis_config(),
            factory,
        )
        .unwrap();
        assert!(wait_for(&shared, |state| {
            state.status == WorkerStatus::Failed("no such device".to_owned())
        }));
        let started = Instant::now();
        drop(worker);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn worker_stops_even_when_the_source_never_delivers_data() {
        let dropped = Arc::new(AtomicBool::new(false));
        let mut source = Some(SilentSource {
            dropped: Arc::clone(&dropped),
        });
        let shared = new_shared();
        let factory: SourceFactory = Box::new(move |_| {
            source
                .take()
                .map(|source| Box::new(source) as Box<dyn AudioSource>)
                .ok_or_else(|| "used twice".to_owned())
        });
        let worker = spawn_capture(
            &crate::resources::test_resources(),
            Arc::clone(&shared),
            analysis_config(),
            factory,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(lock(&shared).snapshot.seq, 0);
        let ticket = worker.join_observer().unwrap();
        drop(worker);
        assert_eq!(
            ticket
                .join_until(Instant::now() + Duration::from_secs(1))
                .unwrap(),
            ilium_platform::owned_worker::WorkerExit::Joined
        );
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn plan_attempts_hand_over_to_the_next_plan_after_a_failure() {
        // A source that ends immediately without data advances the attempt
        // counter, so the factory is asked for attempt 1 next.
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&attempts);
        let shared = new_shared();
        let factory: SourceFactory = Box::new(move |attempt| {
            recorded.lock().unwrap().push(attempt);
            Err("first plan broken".to_owned())
        });
        let worker = spawn_capture(
            &crate::resources::test_resources(),
            Arc::clone(&shared),
            analysis_config(),
            factory,
        )
        .unwrap();
        assert!(wait_for(&shared, |state| matches!(
            state.status,
            WorkerStatus::Failed(_)
        )));
        drop(worker);
        assert_eq!(attempts.lock().unwrap().first(), Some(&0));
    }

    #[cfg(unix)]
    #[test]
    fn process_source_decodes_stdout_and_reports_exit() {
        let command = CaptureCommand {
            program: "sh",
            // 16 bytes = four mono f32 frames of 0.0 followed by EOF.
            args: vec!["-c".to_owned(), "head -c 16 /dev/zero".to_owned()],
            format: PcmFormat::F32Le,
            channels: 1,
            sample_rate: 44_100,
        };
        let mut source =
            ProcessSource::spawn(&command, &crate::resources::test_resources()).unwrap();
        let mut samples = Vec::new();
        let mut ended = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        while ended.is_none() && Instant::now() < deadline {
            match source.read(&mut samples, Duration::from_millis(50)) {
                ReadOutcome::Ended(reason) => ended = Some(reason),
                ReadOutcome::Data | ReadOutcome::Idle => {}
            }
        }
        assert_eq!(samples, vec![0.0; 4]);
        assert!(ended.unwrap().contains("sh"));
    }

    #[cfg(unix)]
    #[test]
    fn process_source_drop_kills_a_silent_child_promptly() {
        let command = CaptureCommand {
            program: "sleep",
            args: vec!["60".to_owned()],
            format: PcmFormat::F32Le,
            channels: 2,
            sample_rate: 44_100,
        };
        let source = ProcessSource::spawn(&command, &crate::resources::test_resources()).unwrap();
        let child_id = source.child.id();
        let started = Instant::now();
        drop(source);
        assert!(started.elapsed() < Duration::from_secs(3));
        // The child was reaped: its /proc entry is gone.
        assert!(!std::path::Path::new(&format!("/proc/{child_id}")).exists());
    }

    #[test]
    fn process_source_reports_a_missing_program() {
        let command = CaptureCommand {
            program: "ilium-definitely-not-installed",
            args: Vec::new(),
            format: PcmFormat::F32Le,
            channels: 2,
            sample_rate: 44_100,
        };
        let error = ProcessSource::spawn(&command, &crate::resources::test_resources())
            .err()
            .unwrap();
        assert!(error.contains("cannot start"));
    }

    /// Real capture of the default output monitor. Needs a running sound
    /// server and something playing (for example
    /// `ffmpeg -f lavfi -i sine=frequency=1000 -t 5 -f pulse default`).
    /// Run with `cargo test -p ilium-ambient real_system_capture -- --ignored --nocapture`.
    /// Optional environment: `ILIUM_SPECTRUM_TEST_DEVICE` captures that named
    /// device instead (for example a null sink's `.monitor`), and
    /// `ILIUM_SPECTRUM_TEST_HZ` asserts the peak band contains that frequency.
    #[test]
    #[ignore = "captures from the real system audio monitor"]
    fn real_system_capture_prints_the_peak_band() {
        let target = match std::env::var("ILIUM_SPECTRUM_TEST_DEVICE") {
            Ok(name) if !name.is_empty() => CaptureTarget::Named(name),
            _ => CaptureTarget::SystemOutput,
        };
        let plans = plan_sources(&target, cfg!(target_os = "linux"), &program_on_path)
            .expect("a capture helper");
        let mut last_error = String::new();
        for plan in &plans {
            let source: Result<Box<dyn AudioSource>, String> = match plan {
                SourcePlan::Process(command) => {
                    ProcessSource::spawn(command, &crate::resources::test_resources())
                        .map(|s| Box::new(s) as Box<dyn AudioSource>)
                }
                SourcePlan::Cpal(target) => {
                    CpalSource::open(target, &crate::resources::test_resources())
                        .map(|s| Box::new(s) as Box<dyn AudioSource>)
                }
            };
            let mut source = match source {
                Ok(source) => source,
                Err(error) => {
                    last_error = error;
                    continue;
                }
            };
            let rate = source.sample_rate();
            let mut samples = Vec::new();
            let deadline = Instant::now() + Duration::from_millis(1500);
            while Instant::now() < deadline && samples.len() < rate as usize {
                if let ReadOutcome::Ended(reason) =
                    source.read(&mut samples, Duration::from_millis(100))
                {
                    last_error = reason;
                    break;
                }
            }
            if samples.len() < 4096 {
                println!(
                    "{}: only {} samples ({last_error})",
                    plan.label(),
                    samples.len()
                );
                continue;
            }
            let edges = band_edges(BandScale::Log, 48, 40.0, 16_000.0);
            let mut analyzer = Analyzer::new(4096, rate, &edges, 0.0).unwrap();
            let analysis = analyzer.analyze(&samples);
            let (peak_band, peak_db) = analysis
                .bands_db
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(index, db)| (index, *db))
                .unwrap();
            println!(
                "plan {}: {} samples at {rate} Hz, rms {:.1} dBFS, peak band {peak_band} \
                 [{:.0}-{:.0} Hz] at {peak_db:.1} dB",
                plan.label(),
                samples.len(),
                analysis.level_db,
                edges[peak_band],
                edges[peak_band + 1]
            );
            assert!(
                samples.len() >= rate as usize / 2,
                "at least half a second captured"
            );
            if let Some(expected) = std::env::var("ILIUM_SPECTRUM_TEST_HZ")
                .ok()
                .and_then(|hz| hz.parse::<f32>().ok())
            {
                let slack = rate as f32 / 4096.0;
                assert!(
                    edges[peak_band] - slack <= expected
                        && expected <= edges[peak_band + 1] + slack,
                    "peak band {peak_band} does not contain {expected} Hz"
                );
            }
            return;
        }
        panic!("no capture plan produced audio: {last_error}");
    }
}
