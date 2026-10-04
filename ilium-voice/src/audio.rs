//! CPAL audio ownership and deterministic streaming sample conversion.

use ilium_execution::{QuotaGroup, RejectReason, StorageAdmission, WorkerAdmission};
use ilium_platform::owned_worker::{
    spawn_owned, OwnedWorker, StopToken, WorkerExit, WorkerKind, WorkerTicket,
};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use tokio::sync::mpsc;

use crate::audio_preparation::{CapturePreparation, OutputPreparation};
use crate::{VoiceError, VoiceInputMode};

pub(crate) const REALTIME_SAMPLE_RATE: u32 = 24_000;
/// Test and demo seam: `none` runs the session without opening any audio
/// device, so typed sentences and a scripted provider exercise the whole
/// pipeline on a machine with no microphone or speaker (CI, containers).
const AUDIO_OVERRIDE_ENV: &str = "ILIUM_VOICE_AUDIO";
const CAPTURE_CHANNEL_CAPACITY: usize = 32;
const MAX_CAPTURE_TAIL_FRAMES: usize = 2 * CAPTURE_CHANNEL_CAPACITY + 1;
pub(super) const MAX_BUFFERED_PLAYBACK_SECONDS: usize = 30;
pub(super) const MAX_CAPTURE_DEVICE_SAMPLES: usize = 262_144;
const MAX_CAPTURE_PCM_SAMPLES: usize = 32_768;
pub(crate) const MAX_PROVIDER_PCM_BYTES: usize = 256 * 1024;
const MAX_DEVICE_SAMPLE_RATE: u32 = 384_000;
const MAX_DEVICE_CHANNELS: u16 = 32;

const DEVICE_OWNER_BYTES: usize = 2 * 1024 * 1024;
const NATIVE_AUDIO_BYTES: usize = 64 * 1024 * 1024;
const HEADLESS_AUDIO_BYTES: usize = 128 * 1024;

// Callback cells are preallocated on the admitted device owner. Safe atomics
// avoid a callback lock and ensure a reset/reuse race cannot create a Rust data
// race. These are audio-specific SPSC channels, not a second execution pool.
const RAW_SAMPLE_CAPACITY: usize = 1024 * 1024;
const PLAYBACK_SAMPLE_CAPACITY: usize = 6 * 1024 * 1024;
const CALLBACK_CLOSED: usize = 1usize << (usize::BITS - 1);

#[derive(Default)]
pub(super) struct CallbackGate {
    state: AtomicUsize,
    changed: tokio::sync::Notify,
}
struct ActiveCallback<'a>(&'a CallbackGate);
#[derive(Debug, PartialEq, Eq)]
enum CallbackRefusal {
    Closed,
    Contended,
}
impl CallbackGate {
    fn new(open: bool) -> Self {
        Self {
            state: AtomicUsize::new(if open { 0 } else { CALLBACK_CLOSED }),
            changed: tokio::sync::Notify::new(),
        }
    }
    fn enter(&self) -> Result<ActiveCallback<'_>, CallbackRefusal> {
        let mut state = self.state.load(Ordering::Acquire);
        // Native streams have one FnMut callback owner. Bound control races;
        // refusal is before sample admission, never a wait in the callback.
        for _ in 0..8 {
            if state & CALLBACK_CLOSED != 0 {
                return Err(CallbackRefusal::Closed);
            }
            match self
                .state
                .compare_exchange(state, state + 1, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return Ok(ActiveCallback(self)),
                Err(actual) => state = actual,
            }
        }
        Err(CallbackRefusal::Contended)
    }
    pub(super) async fn close(&self) {
        self.state.fetch_or(CALLBACK_CLOSED, Ordering::AcqRel);
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.state.load(Ordering::Acquire) & !CALLBACK_CLOSED == 0 {
                return;
            }
            changed.await;
        }
    }
    fn open(&self) {
        self.state.fetch_and(!CALLBACK_CLOSED, Ordering::Release);
    }
}
impl Drop for ActiveCallback<'_> {
    fn drop(&mut self) {
        let previous = self.0.state.fetch_sub(1, Ordering::AcqRel);
        if previous == CALLBACK_CLOSED + 1 {
            self.0.changed.notify_one();
        }
    }
}
struct RawHeader {
    start: AtomicU64,
    length: AtomicUsize,
}
pub(super) struct RawCapture {
    cells: Box<[AtomicU32]>,
    headers: Box<[RawHeader]>,
    written: AtomicU64,
    read: AtomicU64,
    samples_written: AtomicU64,
    samples_read: AtomicU64,
    pub(super) processed: AtomicU64,
    pub(super) changed: tokio::sync::Notify,
    pub(super) wake: Arc<OnceLock<std::thread::Thread>>,
}
impl RawCapture {
    pub(super) fn new() -> Self {
        Self {
            cells: (0..RAW_SAMPLE_CAPACITY)
                .map(|_| AtomicU32::new(0))
                .collect(),
            headers: (0..CAPTURE_CHANNEL_CAPACITY)
                .map(|_| RawHeader {
                    start: AtomicU64::new(0),
                    length: AtomicUsize::new(0),
                })
                .collect(),
            written: AtomicU64::new(0),
            read: AtomicU64::new(0),
            samples_written: AtomicU64::new(0),
            samples_read: AtomicU64::new(0),
            processed: AtomicU64::new(0),
            changed: tokio::sync::Notify::new(),
            wake: Arc::new(OnceLock::new()),
        }
    }
    pub(super) fn push<T>(&self, data: &[T], channels: usize) -> bool
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let written = self.written.load(Ordering::Relaxed);
        let sample_start = self.samples_written.load(Ordering::Relaxed);
        let count = data.len().div_ceil(channels);
        if written == u64::MAX
            || sample_start.checked_add(count as u64).is_none()
            || written - self.read.load(Ordering::Acquire) >= CAPTURE_CHANNEL_CAPACITY as u64
            || sample_start - self.samples_read.load(Ordering::Acquire) + count as u64
                > RAW_SAMPLE_CAPACITY as u64
        {
            return false;
        }
        for (index, frame) in data.chunks(channels).enumerate() {
            let sample = frame
                .iter()
                .map(|sample| sample.to_sample::<f32>())
                .sum::<f32>()
                / channels as f32;
            self.cells[(sample_start as usize + index) % RAW_SAMPLE_CAPACITY]
                .store(sample.to_bits(), Ordering::Relaxed);
        }
        let header = &self.headers[written as usize % CAPTURE_CHANNEL_CAPACITY];
        header.start.store(sample_start, Ordering::Relaxed);
        header.length.store(count, Ordering::Relaxed);
        self.samples_written
            .store(sample_start + count as u64, Ordering::Relaxed);
        self.written.store(written + 1, Ordering::Release);
        if let Some(thread) = self.wake.get() {
            thread.unpark();
        }
        true
    }
    pub(super) fn pop_into(&self, target: &mut Vec<f32>) -> Option<u64> {
        let read = self.read.load(Ordering::Relaxed);
        if read == self.written.load(Ordering::Acquire) {
            return None;
        }
        let header = &self.headers[read as usize % CAPTURE_CHANNEL_CAPACITY];
        let start = header.start.load(Ordering::Relaxed);
        let count = header.length.load(Ordering::Relaxed);
        target.clear();
        for index in 0..count {
            target.push(f32::from_bits(
                self.cells[(start as usize + index) % RAW_SAMPLE_CAPACITY].load(Ordering::Relaxed),
            ));
        }
        self.samples_read
            .store(start + count as u64, Ordering::Release);
        self.read.store(read + 1, Ordering::Release);
        Some(read + 1)
    }
    fn admitted_sequence(&self) -> u64 {
        self.written.load(Ordering::Acquire)
    }
}

pub(super) struct PlaybackRing {
    cells: Box<[AtomicU64]>,
    write: AtomicU64,
    read: AtomicU64,
    discard_before: AtomicU64,
    pub(super) generation: Arc<AtomicU64>,
    pub(super) played: AtomicU64,
    pub(super) gate: CallbackGate,
    pub(super) wake: Arc<OnceLock<std::thread::Thread>>,
}
impl PlaybackRing {
    pub(super) fn new(
        sample_rate: u32,
        generation: Arc<AtomicU64>,
        wake: Arc<OnceLock<std::thread::Thread>>,
    ) -> Self {
        let capacity = (sample_rate as usize * MAX_BUFFERED_PLAYBACK_SECONDS)
            .clamp(1, PLAYBACK_SAMPLE_CAPACITY);
        Self {
            cells: (0..capacity).map(|_| AtomicU64::new(0)).collect(),
            write: AtomicU64::new(0),
            read: AtomicU64::new(0),
            discard_before: AtomicU64::new(0),
            generation,
            played: AtomicU64::new(0),
            gate: CallbackGate::new(true),
            wake,
        }
    }
    pub(super) fn push(&self, generation: u64, values: &[f32]) -> usize {
        let Ok(epoch) = u32::try_from(generation) else {
            return 0;
        };
        if self.generation.load(Ordering::Acquire) != generation {
            return 0;
        }
        let write = self.write.load(Ordering::Relaxed);
        let read = self
            .read
            .load(Ordering::Acquire)
            .max(self.discard_before.load(Ordering::Acquire));
        let free = self.cells.len().saturating_sub((write - read) as usize);
        let count = values.len().min(free).min(512);
        for (index, value) in values[..count].iter().enumerate() {
            self.cells[(write as usize + index) % self.cells.len()].store(
                (u64::from(epoch) << 32) | u64::from(value.to_bits()),
                Ordering::Relaxed,
            );
        }
        if self.generation.load(Ordering::Acquire) != generation {
            return 0;
        }
        self.write.store(write + count as u64, Ordering::Release);
        count
    }
    fn pop(&self, generation: u64) -> Option<f32> {
        for _ in 0..513 {
            let read = self
                .read
                .load(Ordering::Relaxed)
                .max(self.discard_before.load(Ordering::Acquire));
            if read == self.write.load(Ordering::Acquire) {
                self.read.store(read, Ordering::Release);
                return None;
            }
            let value = self.cells[read as usize % self.cells.len()].load(Ordering::Relaxed);
            self.read.store(read + 1, Ordering::Release);
            if value >> 32 == generation {
                return Some(f32::from_bits(value as u32));
            }
        }
        None
    }
    pub(super) async fn invalidate(&self) -> Result<u64, VoiceError> {
        self.gate.close().await;
        let generation = self.generation.load(Ordering::Acquire);
        if generation >= u64::from(u32::MAX) {
            return Err(VoiceError::AudioPreparation(
                "audio response generation limit reached".into(),
            ));
        }
        self.generation.store(generation + 1, Ordering::Release);
        self.discard_before
            .store(self.write.load(Ordering::Acquire), Ordering::Release);
        let played = self.played.swap(0, Ordering::AcqRel);
        self.gate.open();
        if let Some(thread) = self.wake.get() {
            thread.unpark();
        }
        Ok(played)
    }
}

/// Actual audio OS-owner receipts, separate from provider/actor completion.
/// `join_until` is blocking: use a dedicated lifecycle observer, never the UI
/// or a Tokio coordination thread. A timeout keeps every unjoined receipt.
#[derive(Clone)]
pub struct AudioCustody {
    inner: Arc<AudioCustodyInner>,
}
struct AudioCustodyInner {
    tickets: Mutex<Vec<WorkerTicket>>,
    // Actor/channel/receipt metadata survives its last logical consumer.
    startup_allocation: Mutex<Option<Arc<dyn crate::VoiceTextAllocation>>>,
    _metadata: StorageAdmission,
}
impl AudioCustody {
    #[cfg(test)]
    pub(crate) fn new(quota: &QuotaGroup) -> Result<Self, VoiceError> {
        Self::try_new(quota).map_err(|reason| {
            VoiceError::AudioPreparation(format!("audio actor metadata admission: {reason:?}"))
        })
    }
    pub(crate) fn try_new(quota: &QuotaGroup) -> Result<Self, RejectReason> {
        // Reserve metadata before constructing the actor/owner receipt storage.
        let metadata = quota.reserve_external_storage(128 * 1024)?;
        Ok(Self {
            inner: Arc::new(AudioCustodyInner {
                tickets: Mutex::new(Vec::with_capacity(2)),
                _metadata: metadata,
                startup_allocation: Mutex::new(None),
            }),
        })
    }
    pub(crate) fn retain_startup(&self, allocation: Arc<dyn crate::VoiceTextAllocation>) {
        // Set exactly once, before spawning the actor or either native owner.
        *lock_recovering_poison(&self.inner.startup_allocation) = Some(allocation);
    }
    pub(crate) fn startup_allocation(&self) -> Option<Arc<dyn crate::VoiceTextAllocation>> {
        lock_recovering_poison(&self.inner.startup_allocation).clone()
    }
    pub(crate) fn register(&self, ticket: WorkerTicket) -> Result<(), VoiceError> {
        let mut tickets = lock_recovering_poison(&self.inner.tickets);
        if tickets.len() >= 2 {
            return Err(VoiceError::AudioPreparation(
                "audio owner receipt capacity exceeded".into(),
            ));
        }
        tickets.push(ticket);
        Ok(())
    }
    pub fn pending_owners(&self) -> usize {
        lock_recovering_poison(&self.inner.tickets)
            .iter()
            .filter(|ticket| ticket.exit().is_none())
            .count()
    }
    pub fn join_until(&self, deadline: std::time::Instant) -> std::io::Result<()> {
        let mut panicked = false;
        loop {
            let tickets = lock_recovering_poison(&self.inner.tickets).clone();
            if tickets.is_empty() {
                break;
            }
            let mut joined = Vec::with_capacity(2);
            let mut expired = None;
            for ticket in &tickets {
                match ticket.join_until(deadline) {
                    Ok(exit) => {
                        panicked |= exit == WorkerExit::Panicked;
                        joined.push(ticket.id());
                    }
                    Err(error) => {
                        expired = Some(error.worker_id);
                        break;
                    }
                }
            }
            lock_recovering_poison(&self.inner.tickets)
                .retain(|ticket| !joined.contains(&ticket.id()));
            // The device builder can register DSP while shutdown is pending.
            // Re-read after actual device join rather than treating an earlier
            // one-ticket snapshot as a complete retirement receipt.
            if let Some(worker_id) = expired {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("audio OS owner {worker_id} has not joined"),
                ));
            }
        }
        if panicked {
            return Err(std::io::Error::other(
                "audio OS owner panicked during retirement",
            ));
        }
        Ok(())
    }
}

enum NativeDebit {
    Worker(WorkerAdmission),
    Storage(StorageAdmission),
}
/// Source-qualified native declaration. Opaque backend credit is never
/// released on an unproved join: after native creation is attempted it stays
/// charged until process exit. No callback or provider change is hidden here.
pub(super) struct NativeAudioAdmission {
    debit: Option<NativeDebit>,
    retain_until_process_exit: bool,
    attempted: AtomicBool,
}
impl NativeAudioAdmission {
    fn reserve(quota: &QuotaGroup) -> Result<Arc<Self>, VoiceError> {
        let policy = ilium_platform::audio_backend::native_stream_custody();
        let debit = if policy.declared_threads == 0 {
            NativeDebit::Storage(quota.reserve_external_storage(NATIVE_AUDIO_BYTES).map_err(
                |reason| {
                    VoiceError::AudioPreparation(format!(
                        "opaque native audio storage admission: {reason:?}"
                    ))
                },
            )?)
        } else {
            NativeDebit::Worker(
                quota
                    .reserve_external_worker(policy.declared_threads, NATIVE_AUDIO_BYTES)
                    .map_err(|reason| {
                        VoiceError::AudioPreparation(format!("native audio admission: {reason:?}"))
                    })?,
            )
        };
        if !policy.stream_drop_joins {
            tracing::warn!(backend=policy.backend, declared_threads=policy.declared_threads,
                "native audio join/TLS custody is opaque; attempted native ownership remains process-charged");
        }
        Ok(Arc::new(Self {
            debit: Some(debit),
            retain_until_process_exit: !policy.stream_drop_joins,
            attempted: AtomicBool::new(false),
        }))
    }
}
impl Drop for NativeAudioAdmission {
    fn drop(&mut self) {
        if self.retain_until_process_exit && self.attempted.load(Ordering::Acquire) {
            // CPAL exposes no native join receipt on this backend. Deliberately
            // retain the bounded declaration; releasing it would fabricate
            // exit proof. Future starts see truthful shared resource pressure.
            if let Some(debit) = self.debit.take() {
                match debit {
                    NativeDebit::Worker(lease) => std::mem::forget(lease),
                    NativeDebit::Storage(lease) => std::mem::forget(lease),
                }
            }
        }
    }
}

async fn start_device_owner<R: 'static, T: Send + 'static>(
    quota: &QuotaGroup,
    custody: &AudioCustody,
    native: Arc<NativeAudioAdmission>,
    create: impl FnOnce() -> Result<(R, T), VoiceError> + Send + 'static,
) -> Result<(OwnedWorker, T), VoiceError> {
    let owner_admission = Arc::new(
        quota
            .reserve_external_worker(1, DEVICE_OWNER_BYTES)
            .map_err(|reason| {
                VoiceError::AudioPreparation(format!("audio device owner admission: {reason:?}"))
            })?,
    );
    let startup_allocation = custody.startup_allocation();
    let thread = Arc::new(OnceLock::<std::thread::Thread>::new());
    let wake_thread = thread.clone();
    let (ready, receiver) = tokio::sync::oneshot::channel();
    let owner = spawn_owned(
        "ilium-audio-devices",
        WorkerKind::Cooperative,
        StopToken::default(),
        move || {
            // Wake is nonblocking; these leases outlive actual stream destruction
            // AND the platform supervisor's OS join/TLS observation.
            let _owner_admission = &owner_admission;
            let _native = &native;
            let _startup = &startup_allocation;
            if let Some(thread) = wake_thread.get() {
                thread.unpark();
            }
        },
        move |stop| {
            let _ = thread.set(std::thread::current());
            if stop.is_stopped() {
                return;
            }
            match create() {
                Ok((resource, value)) => {
                    let delivered = ready.send(Ok(value)).is_ok();
                    while delivered && !stop.is_stopped() {
                        std::thread::park();
                    }
                    // R need not be Send: native streams stay on their creator OS
                    // owner, including blocking Stream::drop and its native joins.
                    drop(resource);
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
            }
        },
    )
    .map_err(|error| VoiceError::AudioPreparation(format!("audio device owner spawn: {error}")))?;
    custody.register(owner.ticket())?;
    let value = receiver.await.map_err(|_| {
        VoiceError::AudioPreparation(
            "audio device initialization owner ended without a result".into(),
        )
    })??;
    Ok((owner, value))
}

/// One mono, signed 16-bit, 24 kHz frame ready for the Realtime API.
pub(crate) struct CapturedAudio {
    pub pcm16_le: Vec<u8>,
    pub(super) _allocation: Option<Arc<NativeAudioAdmission>>,
}
impl std::fmt::Debug for CapturedAudio {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedAudio")
            .field("pcm_bytes", &self.pcm16_le.len())
            .finish()
    }
}

#[derive(Default)]
struct CaptureHealth {
    oversized: AtomicU64,
    consumer_stalled: AtomicU64,
    device_failed: AtomicU64,
    changed: tokio::sync::Notify,
}

impl CaptureHealth {
    fn failure(&self) -> Option<VoiceError> {
        if self.oversized.load(Ordering::Acquire) != 0
            || self.consumer_stalled.load(Ordering::Acquire) != 0
        {
            Some(VoiceError::AudioPreparation(
                "native capture overrun; utterance is incomplete".into(),
            ))
        } else if self.device_failed.load(Ordering::Acquire) != 0 {
            Some(VoiceError::AudioPreparation(
                "native audio stream reported a failure".into(),
            ))
        } else {
            None
        }
    }
    fn report_drops(&self) {
        let oversized = self.oversized.swap(0, Ordering::Relaxed);
        let consumer_stalled = self.consumer_stalled.swap(0, Ordering::Relaxed);
        if oversized != 0 || consumer_stalled != 0 {
            tracing::warn!(
                oversized,
                consumer_stalled,
                "voice capture frames dropped by realtime admission"
            );
        }
    }
}

/// Owns both device streams. Dropping this object stops capture and playback.
pub(crate) struct AudioEngine {
    /// `None` only for the headless test seam (see `AUDIO_OVERRIDE_ENV`).
    _device_owner: Option<OwnedWorker>,
    /// Keeps a headless engine's capture channel open: a dropped sender would
    /// make `next_capture` report the session as ended instead of idle.
    _headless_capture_sender: Option<mpsc::Sender<CapturedAudio>>,
    capture_receiver: mpsc::Receiver<CapturedAudio>,
    capture_gate: Arc<CallbackGate>,
    raw_capture: Option<Arc<RawCapture>>,
    capture_health: Arc<CaptureHealth>,
    playback_samples: Arc<PlaybackRing>,
    output_sample_rate: u32,
    output_preparation: Option<OutputPreparation>,
    output_generation: Arc<AtomicU64>,
    // Shared playback/capture allocations can outlive either OS owner.
    _native_admission: Option<Arc<NativeAudioAdmission>>,
    _headless_admission: Option<StorageAdmission>,
}

impl AudioEngine {
    pub(crate) async fn start(
        input_device_name: Option<&str>,
        output_device_name: Option<&str>,
        input_mode: VoiceInputMode,
        output_volume_percent: u8,
        quota: &QuotaGroup,
        custody: &AudioCustody,
    ) -> Result<Self, VoiceError> {
        if is_headless_requested() {
            return Self::headless(output_volume_percent, quota);
        }
        let native = NativeAudioAdmission::reserve(quota)?;
        let input_device_name = input_device_name.map(str::to_owned);
        let output_device_name = output_device_name.map(str::to_owned);
        let builder_native = native.clone();
        let builder_quota = quota.clone();
        let builder_custody = custody.clone();
        let (owner, mut engine) = start_device_owner(quota, custody, native, move || {
            Self::build(
                input_device_name.as_deref(),
                output_device_name.as_deref(),
                input_mode,
                output_volume_percent,
                &builder_quota,
                &builder_custody,
                builder_native,
            )
        })
        .await?;
        engine._device_owner = Some(owner);
        Ok(engine)
    }

    fn build(
        input_device_name: Option<&str>,
        output_device_name: Option<&str>,
        input_mode: VoiceInputMode,
        output_volume_percent: u8,
        quota: &QuotaGroup,
        custody: &AudioCustody,
        native: Arc<NativeAudioAdmission>,
    ) -> Result<((cpal::Stream, cpal::Stream), Self), VoiceError> {
        let host = cpal::default_host();
        let input_device = find_device(&host, input_device_name, true)?;
        let output_device = find_device(&host, output_device_name, false)?;
        let input_supported_config = input_device.default_input_config().map_err(|source| {
            VoiceError::AudioConfiguration {
                direction: "input",
                source,
            }
        })?;
        let output_supported_config = output_device.default_output_config().map_err(|source| {
            VoiceError::AudioConfiguration {
                direction: "output",
                source,
            }
        })?;

        let capture_gate = Arc::new(CallbackGate::new(matches!(
            input_mode,
            VoiceInputMode::SemanticVad
        )));
        let raw_capture = Arc::new(RawCapture::new());
        let (capture_sender, capture_receiver) = mpsc::channel(CAPTURE_CHANNEL_CAPACITY);
        let capture_health = Arc::new(CaptureHealth::default());
        for config in [&input_supported_config, &output_supported_config] {
            if !(1..=MAX_DEVICE_SAMPLE_RATE).contains(&config.sample_rate())
                || !(1..=MAX_DEVICE_CHANNELS).contains(&config.channels())
            {
                return Err(VoiceError::InvalidConfiguration(
                    "audio devices require 1–384000 Hz and 1–32 channels".into(),
                ));
            }
        }
        native.attempted.store(true, Ordering::Release);
        let input_stream = build_input_stream(
            &input_device,
            &input_supported_config,
            raw_capture.clone(),
            capture_gate.clone(),
            capture_health.clone(),
        )?;

        let output_generation = Arc::new(AtomicU64::new(0));
        let playback_samples = Arc::new(PlaybackRing::new(
            output_supported_config.sample_rate(),
            output_generation.clone(),
            raw_capture.wake.clone(),
        ));
        let output_stream = build_output_stream(
            &output_device,
            &output_supported_config,
            playback_samples.clone(),
            capture_health.clone(),
        )?;

        input_stream
            .play()
            .map_err(|source| VoiceError::StartAudioStream {
                direction: "input",
                source,
            })?;
        output_stream
            .play()
            .map_err(|source| VoiceError::StartAudioStream {
                direction: "output",
                source,
            })?;

        let output_sample_rate = output_supported_config.sample_rate();
        let output_preparation = OutputPreparation::start(
            output_sample_rate,
            f32::from(output_volume_percent) / 100.0,
            playback_samples.clone(),
            CapturePreparation {
                raw: raw_capture.clone(),
                sender: capture_sender,
                sample_rate: input_supported_config.sample_rate(),
            },
            quota,
            custody,
            Some(native.clone()),
        )?;
        Ok((
            (input_stream, output_stream),
            Self {
                _device_owner: None,
                _headless_capture_sender: None,
                capture_receiver,
                capture_gate,
                raw_capture: Some(raw_capture),
                capture_health,
                playback_samples,
                output_sample_rate,
                output_preparation: Some(output_preparation),
                output_generation,
                _native_admission: Some(native),
                _headless_admission: None,
            },
        ))
    }

    /// An engine with no devices: capture never yields and playback is
    /// discarded. Used only through `AUDIO_OVERRIDE_ENV`.
    fn headless(_output_volume_percent: u8, quota: &QuotaGroup) -> Result<Self, VoiceError> {
        let admission = quota
            .reserve_external_storage(HEADLESS_AUDIO_BYTES)
            .map_err(|reason| {
                VoiceError::AudioPreparation(format!(
                    "headless audio storage admission: {reason:?}"
                ))
            })?;
        let (capture_sender, capture_receiver) = mpsc::channel(1);
        let generation = Arc::new(AtomicU64::new(0));
        let playback = Arc::new(PlaybackRing::new(
            1,
            generation.clone(),
            Arc::new(OnceLock::new()),
        ));
        Ok(Self {
            _device_owner: None,
            _headless_capture_sender: Some(capture_sender),
            capture_receiver,
            capture_gate: Arc::new(CallbackGate::new(false)),
            raw_capture: None,
            capture_health: Arc::new(CaptureHealth::default()),
            playback_samples: playback,
            output_sample_rate: REALTIME_SAMPLE_RATE,
            output_preparation: None,
            output_generation: generation,
            _native_admission: None,
            _headless_admission: Some(admission),
        })
    }

    #[cfg(test)]
    pub(super) fn backpressure_fixture(
        quota: &QuotaGroup,
        custody: &AudioCustody,
    ) -> Result<(Self, impl Fn() -> bool + 'static), VoiceError> {
        // Test-only native-equivalent storage: the startup holder and DSP wake
        // retain it through actual join. No CPAL device/backend is created.
        #[derive(Debug)]
        struct FixtureAllocation {
            _storage: StorageAdmission,
        }
        impl crate::VoiceTextAllocation for FixtureAllocation {}
        let storage = quota
            .reserve_external_storage(NATIVE_AUDIO_BYTES)
            .map_err(|reason| {
                VoiceError::AudioPreparation(format!("fixture ring storage admission: {reason:?}"))
            })?;
        custody.retain_startup(Arc::new(FixtureAllocation { _storage: storage }));
        let mut audio = Self::headless(100, quota)?;
        let raw = Arc::new(RawCapture::new());
        let playback = Arc::new(PlaybackRing::new(
            1,
            audio.output_generation.clone(),
            raw.wake.clone(),
        ));
        let (sender, receiver) = mpsc::channel(CAPTURE_CHANNEL_CAPACITY);
        let preparation = OutputPreparation::start(
            REALTIME_SAMPLE_RATE,
            1.,
            playback.clone(),
            CapturePreparation {
                raw: raw.clone(),
                sender,
                sample_rate: REALTIME_SAMPLE_RATE,
            },
            quota,
            custody,
            None,
        )?;
        let capacity = preparation.mailbox_capacity_probe();
        let probe_ring = playback.clone();
        let probe = move || {
            capacity() == 0
                && probe_ring
                    .write
                    .load(Ordering::Acquire)
                    .saturating_sub(probe_ring.read.load(Ordering::Acquire))
                    >= probe_ring.cells.len() as u64
        };
        audio.capture_receiver = receiver;
        audio.raw_capture = Some(raw);
        audio.playback_samples = playback;
        audio.output_preparation = Some(preparation);
        Ok((audio, probe))
    }

    pub(crate) async fn next_capture(&mut self) -> Result<Option<CapturedAudio>, VoiceError> {
        loop {
            if let Some(error) = self.capture_health.failure() {
                self.capture_health.report_drops();
                return Err(error);
            }
            tokio::select! {
                capture=self.capture_receiver.recv()=> {
                    if let Some(raw)=&self.raw_capture {if let Some(thread)=raw.wake.get() {thread.unpark();}}
                    return Ok(capture);
                }
                _=self.capture_health.changed.notified()=>{}
            }
        }
    }
    /// Close admission, settle all already-entered native callbacks, then
    /// consume PCM through the highest raw ordinal before provider commit.
    pub(crate) async fn pause_capture_and_drain(
        &mut self,
    ) -> Result<Vec<CapturedAudio>, VoiceError> {
        self.capture_gate.close().await;
        let Some(raw) = &self.raw_capture else {
            return Ok(Vec::new());
        };
        let target = raw.admitted_sequence();
        let mut pending = Vec::with_capacity(MAX_CAPTURE_TAIL_FRAMES);
        loop {
            let changed = raw.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            while pending.len() < MAX_CAPTURE_TAIL_FRAMES {
                match self.capture_receiver.try_recv() {
                    Ok(capture) => pending.push(capture),
                    Err(_) => break,
                }
            }
            if pending.len() == MAX_CAPTURE_TAIL_FRAMES && !self.capture_receiver.is_empty() {
                return Err(VoiceError::AudioPreparation(
                    "capture tail accounting limit exceeded; utterance cancelled".into(),
                ));
            }
            if let Some(thread) = raw.wake.get() {
                thread.unpark();
            }
            if raw.processed.load(Ordering::Acquire) >= target {
                break;
            }
            if let Some(error) = self.capture_health.failure() {
                return Err(error);
            }
            tokio::select! {
                capture=self.capture_receiver.recv(),if pending.len()<MAX_CAPTURE_TAIL_FRAMES=> {let Some(capture)=capture else {return Err(VoiceError::SessionEnded);};pending.push(capture);}
                _=&mut changed=>{}
            }
        }
        while pending.len() < MAX_CAPTURE_TAIL_FRAMES {
            match self.capture_receiver.try_recv() {
                Ok(capture) => pending.push(capture),
                Err(_) => break,
            }
        }
        if !self.capture_receiver.is_empty() {
            return Err(VoiceError::AudioPreparation(
                "capture tail accounting limit exceeded; utterance cancelled".into(),
            ));
        }
        if let Some(error) = self.capture_health.failure() {
            return Err(error);
        }
        Ok(pending)
    }
    pub(crate) fn set_capture_enabled(&self, is_enabled: bool) {
        if is_enabled {
            self.capture_gate.open();
        } else {
            self.capture_gate
                .state
                .fetch_or(CALLBACK_CLOSED, Ordering::AcqRel);
        }
    }
    pub(crate) async fn begin_response_audio(&mut self) -> Result<(), VoiceError> {
        self.playback_samples.invalidate().await.map(|_| ())
    }

    /// Admission is ordered and lossless. Only bounded chunks cross into the
    /// CPU owner; a stalled consumer applies asynchronous backpressure.
    pub(crate) async fn enqueue_realtime_pcm16(&mut self, bytes: &[u8]) -> Result<(), VoiceError> {
        if bytes.len() > MAX_PROVIDER_PCM_BYTES {
            return Err(VoiceError::AudioPreparation(
                "provider PCM delta exceeds 256 KiB admission limit".into(),
            ));
        }
        if let Some(preparation) = &self.output_preparation {
            preparation
                .enqueue(self.output_generation.load(Ordering::Acquire), bytes)
                .await?;
        }
        Ok(())
    }

    /// Stops pending playback and returns how many milliseconds were actually
    /// emitted to the device for the current response item.
    pub(crate) async fn interrupt_playback(&mut self) -> Result<u64, VoiceError> {
        let played = self.playback_samples.invalidate().await?;
        Ok(played.saturating_mul(1000) / u64::from(self.output_sample_rate))
    }
}

fn is_headless_requested() -> bool {
    std::env::var(AUDIO_OVERRIDE_ENV).is_ok_and(|value| value.trim().eq_ignore_ascii_case("none"))
}

/// Returns stable display names without keeping devices alive.
pub fn available_input_devices() -> Result<Vec<String>, VoiceError> {
    available_devices(true)
}

/// Returns stable display names without keeping devices alive.
pub fn available_output_devices() -> Result<Vec<String>, VoiceError> {
    available_devices(false)
}

fn available_devices(is_input: bool) -> Result<Vec<String>, VoiceError> {
    let host = cpal::default_host();
    let direction = if is_input { "input" } else { "output" };
    let devices = if is_input {
        host.input_devices()
    } else {
        host.output_devices()
    }
    .map_err(|source| VoiceError::EnumerateAudioDevices { direction, source })?;
    let mut names = devices.map(|device| device.to_string()).collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    Ok(names)
}

fn find_device(
    host: &cpal::Host,
    requested_name: Option<&str>,
    is_input: bool,
) -> Result<cpal::Device, VoiceError> {
    let direction = if is_input { "input" } else { "output" };
    let Some(requested_name) = requested_name.filter(|name| !name.trim().is_empty()) else {
        return if is_input {
            host.default_input_device()
        } else {
            host.default_output_device()
        }
        .ok_or(VoiceError::MissingAudioDevice { direction });
    };

    let devices = if is_input {
        host.input_devices()
    } else {
        host.output_devices()
    }
    .map_err(|source| VoiceError::EnumerateAudioDevices { direction, source })?;

    devices
        .map(|device| {
            let name = device.to_string();
            (device, name)
        })
        .find_map(|(device, name)| (name == requested_name).then_some(device))
        .ok_or_else(|| VoiceError::NamedAudioDeviceNotFound {
            direction,
            name: requested_name.to_owned(),
        })
}

fn build_input_stream(
    device: &cpal::Device,
    supported_config: &cpal::SupportedStreamConfig,
    raw: Arc<RawCapture>,
    gate: Arc<CallbackGate>,
    health: Arc<CaptureHealth>,
) -> Result<cpal::Stream, VoiceError> {
    let channels = usize::from(supported_config.channels());
    let config = supported_config.config();
    macro_rules! build {
        ($sample_type:ty) => {
            build_typed_input_stream::<$sample_type>(device, &config, channels, raw, gate, health)
        };
    }
    match supported_config.sample_format() {
        cpal::SampleFormat::I8 => build!(i8),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::I64 => build!(i64),
        cpal::SampleFormat::U8 => build!(u8),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::U32 => build!(u32),
        cpal::SampleFormat::U64 => build!(u64),
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::F64 => build!(f64),
        format => Err(VoiceError::UnsupportedSampleFormat {
            direction: "input",
            format,
        }),
    }
}
fn build_typed_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    raw: Arc<RawCapture>,
    gate: Arc<CallbackGate>,
    health: Arc<CaptureHealth>,
) -> Result<cpal::Stream, VoiceError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let input_sample_rate = config.sample_rate;
    let error_health = health.clone();
    device
        .build_input_stream(
            *config,
            move |data: &[T], _| {
                let _active = match gate.enter() {
                    Ok(active) => active,
                    Err(CallbackRefusal::Closed) => return,
                    Err(CallbackRefusal::Contended) => {
                        // Explicit incomplete-utterance failure: bounded gate
                        // contention never silently removes microphone samples.
                        health.consumer_stalled.fetch_add(1, Ordering::Release);
                        health.changed.notify_one();
                        return;
                    }
                };
                if !capture_fits_budget(data.len(), channels, input_sample_rate) {
                    health.oversized.fetch_add(1, Ordering::Release);
                    health.changed.notify_one();
                    return;
                }
                if !raw.push(data, channels) {
                    health.consumer_stalled.fetch_add(1, Ordering::Release);
                    health.changed.notify_one();
                }
            },
            move |_error| {
                error_health.device_failed.fetch_add(1, Ordering::Release);
                error_health.changed.notify_one();
            },
            None,
        )
        .map_err(|source| VoiceError::BuildAudioStream {
            direction: "input",
            source,
        })
}
fn build_output_stream(
    device: &cpal::Device,
    supported_config: &cpal::SupportedStreamConfig,
    playback: Arc<PlaybackRing>,
    health: Arc<CaptureHealth>,
) -> Result<cpal::Stream, VoiceError> {
    let channels = usize::from(supported_config.channels());
    let config = supported_config.config();
    macro_rules! build {
        ($sample_type:ty) => {
            build_typed_output_stream::<$sample_type>(device, &config, channels, playback, health)
        };
    }
    match supported_config.sample_format() {
        cpal::SampleFormat::I8 => build!(i8),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::I64 => build!(i64),
        cpal::SampleFormat::U8 => build!(u8),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::U32 => build!(u32),
        cpal::SampleFormat::U64 => build!(u64),
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::F64 => build!(f64),
        format => Err(VoiceError::UnsupportedSampleFormat {
            direction: "output",
            format,
        }),
    }
}
pub(super) fn render_output<T>(output: &mut [T], channels: usize, playback: &PlaybackRing)
where
    T: SizedSample + FromSample<f32>,
{
    let active = playback.gate.enter();
    let generation = playback.generation.load(Ordering::Acquire);
    let mut emitted = 0u64;
    for frame in output.chunks_mut(channels.max(1)) {
        let sample = if active.is_ok() {
            match playback.pop(generation) {
                Some(sample) => {
                    emitted = emitted.saturating_add(1);
                    sample
                }
                None => 0.0,
            }
        } else {
            0.0
        };
        for channel in frame {
            *channel = T::from_sample(sample);
        }
    }
    if active.is_ok() {
        playback.played.fetch_add(emitted, Ordering::Relaxed);
    }
    drop(active);
    if let Some(thread) = playback.wake.get() {
        thread.unpark();
    }
}
fn build_typed_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    playback: Arc<PlaybackRing>,
    health: Arc<CaptureHealth>,
) -> Result<cpal::Stream, VoiceError>
where
    T: SizedSample + FromSample<f32>,
{
    device
        .build_output_stream(
            *config,
            move |output: &mut [T], _| render_output(output, channels, &playback),
            move |_error| {
                health.device_failed.fetch_add(1, Ordering::Release);
                health.changed.notify_one();
            },
            None,
        )
        .map_err(|source| VoiceError::BuildAudioStream {
            direction: "output",
            source,
        })
}

fn capture_fits_budget(device_samples: usize, channels: usize, input_sample_rate: u32) -> bool {
    if channels == 0 || input_sample_rate == 0 || device_samples > MAX_CAPTURE_DEVICE_SAMPLES {
        return false;
    }
    // Include the interpolation tail. Every retained callback and all thirty-
    // two PCM slots have a finite sample and byte bound, even for odd buffers.
    let frame_count = device_samples.div_ceil(channels);
    let projected_samples = (frame_count as u64 + 2)
        .saturating_mul(u64::from(REALTIME_SAMPLE_RATE))
        / u64::from(input_sample_rate);
    projected_samples <= MAX_CAPTURE_PCM_SAMPLES as u64
}

fn lock_recovering_poison<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn f32_to_pcm16_le(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        let integer = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        bytes.extend_from_slice(&integer.to_le_bytes());
    }
    bytes
}

pub(crate) fn pcm16_le_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / f32::from(i16::MAX))
        .collect()
}

/// Re-aligns a streamed PCM16-LE byte sequence to sample boundaries across
/// arbitrary chunk splits. Network audio deltas are not guaranteed to end on
/// a 16-bit sample boundary; without carrying the dangling byte to the next
/// chunk, `chunks_exact(2)` would silently drop it and decode the entire
/// following chunk one byte out of phase, which plays as loud noise.
#[derive(Debug, Default)]
pub(super) struct Pcm16SampleAligner {
    pending_byte: Option<u8>,
}

impl Pcm16SampleAligner {
    /// Decodes every complete sample available once `bytes` is appended to
    /// the carried remainder, keeping any new dangling byte for the next call.
    pub(super) fn process(&mut self, bytes: &[u8]) -> Vec<f32> {
        // Only allocate a joined buffer when a byte was actually carried.
        let joined_storage;
        let aligned_bytes = match self.pending_byte.take() {
            Some(carried_byte) => {
                let mut joined = Vec::with_capacity(bytes.len() + 1);
                joined.push(carried_byte);
                joined.extend_from_slice(bytes);
                joined_storage = joined;
                joined_storage.as_slice()
            }
            None => bytes,
        };

        if aligned_bytes.len() % 2 == 1 {
            self.pending_byte = aligned_bytes.last().copied();
        }

        // `pcm16_le_to_f32` uses `chunks_exact(2)`, which ignores exactly the
        // dangling byte stored above.
        pcm16_le_to_f32(aligned_bytes)
    }

    /// Drops any carried byte; the stream it belonged to has ended.
    pub(super) fn reset(&mut self) {
        self.pending_byte = None;
    }
}

/// Chunk-boundary-independent linear resampler. It deliberately keeps a
/// source sample between calls, preventing clicks and drift between CPAL
/// callbacks without coupling the API to a particular DSP library.
#[derive(Debug)]
pub(super) struct StreamingLinearResampler {
    step: f64,
    source_position: f64,
    buffered_samples: Vec<f32>,
}

impl StreamingLinearResampler {
    pub(super) fn new(input_sample_rate: u32, output_sample_rate: u32) -> Self {
        Self {
            step: f64::from(input_sample_rate) / f64::from(output_sample_rate),
            source_position: 0.0,
            buffered_samples: Vec::new(),
        }
    }

    pub(super) fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        self.buffered_samples.extend_from_slice(samples);
        let mut output = Vec::new();

        while self.source_position + 1.0 < self.buffered_samples.len() as f64 {
            let lower_index = self.source_position.floor() as usize;
            let upper_index = lower_index + 1;
            let fraction = (self.source_position - lower_index as f64) as f32;
            let sample = self.buffered_samples[lower_index]
                + (self.buffered_samples[upper_index] - self.buffered_samples[lower_index])
                    * fraction;
            output.push(sample);
            self.source_position += self.step;
        }

        // The loop's final iteration can advance `source_position` by up to
        // `step` past the last index it actually consumed (it adds `step`
        // after passing the `+ 1.0 < len` check), so for step > 2 -- i.e.
        // input sample rates above 48 kHz being downsampled to
        // REALTIME_SAMPLE_RATE -- the floor of `source_position` can exceed
        // `buffered_samples.len()`. Clamp the drain to what actually exists;
        // `source_position` keeps the (now-negative-after-subtraction)
        // leftover offset so the next call's `extend_from_slice` lines the
        // fractional position back up correctly.
        let consumed = (self.source_position.floor() as usize).min(self.buffered_samples.len());
        if consumed > 0 {
            self.buffered_samples.drain(..consumed);
            self.source_position -= consumed as f64;
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_metadata_admission_refuses_typed_before_constructing_custody() {
        let mut limits = crate::test_quota().snapshot().limits;
        limits.worker_bytes = 128 * 1024 - 1;
        let quota = QuotaGroup::new(limits);
        assert!(matches!(
            AudioCustody::try_new(&quota),
            Err(RejectReason::WorkerBytes)
        ));
        assert_eq!(quota.snapshot().worker_bytes, 0);
        assert_eq!(quota.snapshot().worker_threads, 0);
    }

    #[test]
    fn capture_admission_bounds_samples_before_allocation() {
        assert!(capture_fits_budget(960, 2, 48_000));
        assert!(!capture_fits_budget(
            MAX_CAPTURE_DEVICE_SAMPLES + 1,
            2,
            48_000
        ));
        assert!(!capture_fits_budget(0, 0, 48_000));
        assert!(!capture_fits_budget(0, 2, 0));
        assert!(!capture_fits_budget(100_000, 1, 8_000));
        assert!(capture_fits_budget(10_000, 1, 8_000));
    }

    #[tokio::test]
    async fn oversized_provider_delta_reports_overload_without_admitting_a_prefix() {
        let mut audio = AudioEngine::headless(100, &crate::test_quota()).unwrap();
        assert!(matches!(
            audio
                .enqueue_realtime_pcm16(&vec![0; MAX_PROVIDER_PCM_BYTES + 1])
                .await,
            Err(VoiceError::AudioPreparation(_))
        ));
        assert!(audio
            .playback_samples
            .pop(audio.output_generation.load(Ordering::Acquire))
            .is_none());
    }

    #[tokio::test]
    async fn interruption_fences_generation_and_preserves_emitted_silent_frames() {
        let mut audio = AudioEngine::headless(0, &crate::test_quota()).unwrap();
        assert_eq!(audio.playback_samples.push(0, &[0., 0., 0.]), 3);
        audio
            .playback_samples
            .played
            .store(24_000, Ordering::Relaxed);
        assert_eq!(audio.interrupt_playback().await.unwrap(), 1_000);
        assert_eq!(audio.output_generation.load(Ordering::Acquire), 1);
        assert!(audio
            .playback_samples
            .pop(audio.output_generation.load(Ordering::Acquire))
            .is_none());
        audio.playback_samples.played.store(48, Ordering::Relaxed);
        audio.begin_response_audio().await.unwrap();
        assert_eq!(audio.playback_samples.played.load(Ordering::Acquire), 0);
        assert_eq!(audio.output_generation.load(Ordering::Acquire), 2);
    }

    #[test]
    fn pcm16_conversion_clamps_and_round_trips() {
        let encoded = f32_to_pcm16_le(&[-2.0, -0.5, 0.0, 0.5, 2.0]);
        let decoded = pcm16_le_to_f32(&encoded);

        assert_eq!(decoded.len(), 5);
        assert!((decoded[0] + 1.0).abs() < 0.0001);
        assert!((decoded[1] + 0.5).abs() < 0.0001);
        assert_eq!(decoded[2], 0.0);
        assert!((decoded[3] - 0.5).abs() < 0.0001);
        assert!((decoded[4] - 1.0).abs() < 0.0001);
    }

    #[test]
    fn pcm16_sample_aligner_is_independent_of_chunk_boundaries() {
        let samples = (0..500)
            .map(|index| ((index as f32) / 15.0).sin())
            .collect::<Vec<_>>();
        let encoded = f32_to_pcm16_le(&samples);
        let expected = pcm16_le_to_f32(&encoded);

        // Odd chunk sizes force a carried byte on every call; the prime 7
        // additionally walks the split point through every byte offset.
        for chunk_size in [1_usize, 3, 7, 64, 129] {
            let mut aligner = Pcm16SampleAligner::default();
            let actual = encoded
                .chunks(chunk_size)
                .flat_map(|chunk| aligner.process(chunk))
                .collect::<Vec<_>>();

            assert_eq!(actual, expected, "mismatch for chunk_size={chunk_size}");
        }
    }

    #[test]
    fn pcm16_sample_aligner_reset_drops_the_carried_byte() {
        let mut aligner = Pcm16SampleAligner::default();

        // One dangling byte: no complete sample yet, byte is carried.
        assert!(aligner.process(&[0x12]).is_empty());
        aligner.reset();

        // After reset the next two bytes must decode as one whole sample
        // instead of pairing with the stale carried byte.
        let decoded = aligner.process(&[0x00, 0x40]);
        assert_eq!(decoded.len(), 1);
        assert!((decoded[0] - f32::from(0x4000_i16) / f32::from(i16::MAX)).abs() < 0.0001);
    }

    #[test]
    fn streaming_resampler_is_independent_of_callback_boundaries() {
        let input = (0..4_800)
            .map(|index| ((index as f32) / 20.0).sin())
            .collect::<Vec<_>>();
        let mut whole = StreamingLinearResampler::new(48_000, 24_000);
        let expected = whole.process(&input);
        let mut chunked = StreamingLinearResampler::new(48_000, 24_000);
        let actual = input
            .chunks(137)
            .flat_map(|chunk| chunked.process(chunk))
            .collect::<Vec<_>>();

        assert_eq!(actual, expected);
        assert!(actual.len().abs_diff(2_400) <= 1);
    }

    #[test]
    fn streaming_resampler_does_not_panic_downsampling_high_rate_inputs_in_small_chunks() {
        // step > 2 (96 kHz and 192 kHz down to 24 kHz) is the regime where the
        // final loop iteration can push `source_position` past
        // `buffered_samples.len()`; varied small chunk sizes exercise every
        // possible remainder against `step` so a fixed chunk size can't hide
        // the out-of-bounds drain.
        for input_sample_rate in [96_000_u32, 192_000_u32] {
            let step = f64::from(input_sample_rate) / f64::from(24_000_u32);
            let input = (0..20_000)
                .map(|index| ((index as f32) / 40.0).sin())
                .collect::<Vec<_>>();

            let mut whole = StreamingLinearResampler::new(input_sample_rate, 24_000);
            let expected = whole.process(&input);

            for chunk_size in [5_usize, 6, 7, 137] {
                let mut chunked = StreamingLinearResampler::new(input_sample_rate, 24_000);
                let actual = input
                    .chunks(chunk_size)
                    .flat_map(|chunk| chunked.process(chunk))
                    .collect::<Vec<_>>();

                assert_eq!(
                    actual, expected,
                    "mismatch for input_sample_rate={input_sample_rate}, chunk_size={chunk_size}, step={step}"
                );
            }
        }
    }
}

#[cfg(test)]
mod custody_tests {
    use super::*;
    use std::time::{Duration, Instant};

    struct CreatorOnlyResource {
        // Constructed inside the owner: native handles need not be Send.
        _not_send: std::rc::Rc<()>,
        dropped: std::sync::mpsc::Sender<std::thread::ThreadId>,
    }
    impl Drop for CreatorOnlyResource {
        fn drop(&mut self) {
            let _ = self.dropped.send(std::thread::current().id());
        }
    }

    #[tokio::test]
    async fn device_creation_and_non_send_destruction_stay_on_actual_os_owner() {
        let quota = crate::test_quota();
        let custody = AudioCustody::new(&quota).unwrap();
        let native = NativeAudioAdmission::reserve(&quota).unwrap();
        let (dropped, destruction) = std::sync::mpsc::channel();
        let (owner, creator) = start_device_owner(&quota, &custody, native, move || {
            Ok((
                CreatorOnlyResource {
                    _not_send: std::rc::Rc::new(()),
                    dropped,
                },
                std::thread::current().id(),
            ))
        })
        .await
        .unwrap();
        assert_ne!(creator, std::thread::current().id());
        assert!(quota.snapshot().worker_threads >= 1);
        drop(owner);
        custody
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            destruction.recv_timeout(Duration::from_secs(1)).unwrap(),
            creator
        );
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 128 * 1024);
        drop(custody);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[tokio::test]
    async fn canceled_initialization_keeps_blocked_owner_charged_until_actual_join() {
        let quota = crate::test_quota();
        let custody = AudioCustody::new(&quota).unwrap();
        let native = NativeAudioAdmission::reserve(&quota).unwrap();
        let (release, blocked) = std::sync::mpsc::channel();
        let (entered, admission) = tokio::sync::oneshot::channel();
        let mut initialization =
            Box::pin(start_device_owner(&quota, &custody, native, move || {
                let _ = entered.send(());
                let _ = blocked.recv();
                Ok(((), ()))
            }));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(std::future::Future::poll(initialization.as_mut(), &mut context).is_pending());
        admission.await.unwrap();
        drop(initialization);
        assert_eq!(custody.pending_owners(), 1);
        assert!(custody.join_until(Instant::now()).is_err());
        assert!(quota.snapshot().worker_bytes >= NATIVE_AUDIO_BYTES + DEVICE_OWNER_BYTES);
        release.send(()).unwrap();
        custody
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 128 * 1024);
    }

    #[tokio::test]
    async fn initialization_failure_has_a_real_retirement_receipt() {
        let quota = crate::test_quota();
        let custody = AudioCustody::new(&quota).unwrap();
        let native = NativeAudioAdmission::reserve(&quota).unwrap();
        let result = start_device_owner::<(), ()>(&quota, &custody, native, || {
            Err(VoiceError::AudioPreparation(
                "forced device initialization failure".into(),
            ))
        })
        .await;
        assert!(result.is_err());
        custody
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 128 * 1024);
    }

    #[test]
    fn native_admission_refuses_before_any_creation_attempt() {
        let mut limits = crate::test_quota().snapshot().limits;
        limits.worker_bytes = NATIVE_AUDIO_BYTES - 1;
        let quota = QuotaGroup::new(limits);
        assert!(NativeAudioAdmission::reserve(&quota).is_err());
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn headless_storage_follows_engine_lifetime() {
        let quota = crate::test_quota();
        let engine = AudioEngine::headless(100, &quota).unwrap();
        assert_eq!(quota.snapshot().worker_bytes, HEADLESS_AUDIO_BYTES);
        assert_eq!(quota.snapshot().worker_threads, 0);
        drop(engine);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn opaque_backend_releases_unattempted_credit_but_never_fabricates_native_exit() {
        let quota = crate::test_quota();
        let lease = NativeAudioAdmission {
            debit: Some(NativeDebit::Worker(
                quota.reserve_external_worker(1, 1024).unwrap(),
            )),
            retain_until_process_exit: true,
            attempted: AtomicBool::new(false),
        };
        drop(lease);
        assert_eq!(quota.snapshot().worker_bytes, 0);
        let lease = NativeAudioAdmission {
            debit: Some(NativeDebit::Worker(
                quota.reserve_external_worker(1, 1024).unwrap(),
            )),
            retain_until_process_exit: true,
            attempted: AtomicBool::new(true),
        };
        drop(lease);
        assert_eq!(quota.snapshot().worker_threads, 1);
        assert_eq!(quota.snapshot().worker_bytes, 1024);
        // Isolated test quota only: intentional process-held declaration,
        // never a root/global worker or a fabricated native exit receipt.
    }
    #[test]
    fn actual_panicked_owner_join_is_terminal_and_metadata_survives_last_custody_clone() {
        use std::time::{Duration, Instant};
        let quota = crate::test_quota();
        let custody = AudioCustody::new(&quota).unwrap();
        let last = custody.clone();
        let admission = quota.reserve_external_worker(1, 4096).unwrap();
        let owner = spawn_owned(
            "ilium-audio-forced-native-panic",
            WorkerKind::Cooperative,
            StopToken::default(),
            move || {
                let _lease = &admission;
            },
            |_| {
                panic!("synthetic no-device OS audio owner failure");
            },
        )
        .unwrap();
        custody.register(owner.ticket()).unwrap();
        drop(owner);
        let error = custody
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap_err();
        assert_ne!(error.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(custody.pending_owners(), 0);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 128 * 1024);
        drop(custody);
        assert_eq!(quota.snapshot().worker_bytes, 128 * 1024);
        drop(last);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn callback_raw_admission_preserves_downmix_order_without_reallocation_and_refuses_original_tail(
    ) {
        let raw = RawCapture::new();
        assert!(raw.push(&[1f32, 0., 0.5, -0.5, 0.25, 0.75], 2));
        let mut scratch = Vec::with_capacity(MAX_CAPTURE_DEVICE_SAMPLES);
        let capacity = scratch.capacity();
        assert_eq!(raw.pop_into(&mut scratch), Some(1));
        assert_eq!(scratch, vec![0.5, 0., 0.5]);
        assert_eq!(scratch.capacity(), capacity);
        for _ in 0..CAPTURE_CHANNEL_CAPACITY {
            assert!(raw.push(&[0.25f32], 1));
        }
        let admitted = raw.admitted_sequence();
        assert!(!raw.push(&[0.75f32], 1));
        assert_eq!(raw.admitted_sequence(), admitted);
        for _ in 0..CAPTURE_CHANNEL_CAPACITY {
            assert!(raw.pop_into(&mut scratch).is_some());
            assert_eq!(scratch, [0.25]);
        }
        assert!(raw.pop_into(&mut scratch).is_none());
    }
    #[tokio::test]
    async fn output_barrier_waits_inflight_callback_and_counts_silent_samples() {
        let ring = PlaybackRing::new(1, Arc::new(AtomicU64::new(0)), Arc::new(OnceLock::new()));
        assert_eq!(ring.push(0, &[0., 0.5]), 2);
        let active = ring.gate.enter().unwrap();
        let mut barrier = Box::pin(ring.invalidate());
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(std::future::Future::poll(barrier.as_mut(), &mut context).is_pending());
        ring.played.fetch_add(2, Ordering::Relaxed);
        drop(active);
        assert_eq!(barrier.await.unwrap(), 2);
        assert_eq!(ring.generation.load(Ordering::Acquire), 1);
        let mut silence = [1f32; 2];
        render_output(&mut silence, 1, &ring);
        assert_eq!(silence, [0., 0.]);
        assert_eq!(ring.played.load(Ordering::Acquire), 0);
        assert_eq!(ring.push(1, &[0., 0.25]), 2);
        render_output(&mut silence, 1, &ring);
        assert_eq!(silence, [0., 0.25]);
        assert_eq!(ring.played.load(Ordering::Acquire), 2);
    }
    #[tokio::test]
    async fn capture_close_barrier_waits_every_entered_callback_before_highest_ordinal() {
        let gate = CallbackGate::new(true);
        let raw = RawCapture::new();
        let active = gate.enter().unwrap();
        gate.open(); // idempotent start must not reset active custody
        let mut barrier = Box::pin(gate.close());
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(std::future::Future::poll(barrier.as_mut(), &mut context).is_pending());
        assert!(gate.enter().is_err());
        assert!(raw.push(&[0.125f32, 0.25], 1));
        drop(active);
        barrier.await;
        assert_eq!(raw.admitted_sequence(), 1);
        assert!(gate.enter().is_err());
    }
    #[test]
    fn packed_playback_byte_cap_is_explicit_and_oldest_samples_never_evict_on_full() {
        let ring = PlaybackRing::new(1, Arc::new(AtomicU64::new(0)), Arc::new(OnceLock::new()));
        assert_eq!(ring.cells.len(), 30);
        assert_eq!(ring.push(0, &[0.25; 30]), 30);
        assert_eq!(ring.push(0, &[0.5]), 0);
        let mut output = [0f32; 30];
        render_output(&mut output, 1, &ring);
        assert_eq!(output, [0.25; 30]);
    }
    #[tokio::test]
    async fn capture_tail_barrier_drains_both_full_pcm_and_raw_queues_at_upsample_ratio() {
        let quota = crate::test_quota();
        let _storage = quota.reserve_external_storage(64 * 1024 * 1024).unwrap();
        let custody = AudioCustody::new(&quota).unwrap();
        let mut audio = AudioEngine::headless(100, &quota).unwrap();
        let raw = Arc::new(RawCapture::new());
        let (sender, receiver) = mpsc::channel(CAPTURE_CHANNEL_CAPACITY);
        let playback = Arc::new(PlaybackRing::new(
            1,
            audio.output_generation.clone(),
            raw.wake.clone(),
        ));
        audio.output_preparation = Some(
            OutputPreparation::start(
                1,
                1.,
                playback.clone(),
                CapturePreparation {
                    raw: raw.clone(),
                    sender,
                    sample_rate: 8000,
                },
                &quota,
                &custody,
                None,
            )
            .unwrap(),
        );
        audio.raw_capture = Some(raw.clone());
        audio.capture_receiver = receiver;
        audio.playback_samples = playback;
        audio.set_capture_enabled(true);
        let packet = vec![0.25f32; 10000];
        assert!(capture_fits_budget(packet.len(), 1, 8000));
        for _ in 0..CAPTURE_CHANNEL_CAPACITY {
            assert!(raw.push(&packet, 1));
        }
        loop {
            let changed = raw.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if raw.processed.load(Ordering::Acquire) == CAPTURE_CHANNEL_CAPACITY as u64 {
                break;
            }
            tokio::time::timeout(Duration::from_secs(2), changed)
                .await
                .unwrap();
        }
        assert_eq!(audio.capture_receiver.len(), CAPTURE_CHANNEL_CAPACITY);
        for _ in 0..CAPTURE_CHANNEL_CAPACITY {
            assert!(raw.push(&packet, 1));
        }
        let pending = tokio::time::timeout(Duration::from_secs(2), audio.pause_capture_and_drain())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pending.len(), 2 * CAPTURE_CHANNEL_CAPACITY);
        assert_eq!(pending.capacity(), MAX_CAPTURE_TAIL_FRAMES);
        let bytes = pending
            .iter()
            .map(|capture| capture.pcm16_le.len())
            .sum::<usize>();
        assert_eq!(
            bytes,
            (2 * CAPTURE_CHANNEL_CAPACITY * packet.len() - 1) * 3 * 2
        );
        assert!(audio.capture_receiver.is_empty());
        assert_eq!(
            raw.processed.load(Ordering::Acquire),
            raw.admitted_sequence()
        );
        assert!(audio.capture_gate.enter().is_err());
        drop(audio);
        custody
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }
}
