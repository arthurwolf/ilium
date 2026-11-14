//! Host implementation of `ilium:host/audio`.
//!
//! `devices` and `play` go to the [`AudioBackend`] the process composed
//! (`wiring::wave_two::Backends::audio`): the client lists its sound devices and plays through the
//! system player. `play` encodes the samples as a WAV and runs the backend on a worker thread of
//! the instance, which queues `request-completed` when playback ends; dropping the instance
//! cancels a playback that has not started.
//!
//! `open-capture` and `open-loopback` are validated, checked against their grant and counted
//! against the stream quota, then started on the backend: it records the source and hands back
//! 20 ms chunks, which the import queues as `audio-frames` events of the capture id. Closing the
//! capture or dropping the instance drops the backend's stream, which stops the recording. A
//! process without an audio backend (the server) accepts and counts the stream but records
//! nothing.

use ilium_plugin_api::generated::host::audio::{AudioDevice, HostAudio};
use ilium_plugin_api::generated::types::events::{AudioFramesEvent, Event};
use ilium_plugin_api::generated::types::Outcome;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::env::{CancelFlag, CancelOnDrop, ImportEnv};

/// The sound devices and the player of a process.
pub trait AudioBackend: Send + Sync {
    /// The input and output devices of the computer.
    fn devices(&self) -> Vec<AudioDevice>;

    /// Plays one WAV file and returns when playback ended. A backend that can stop early checks
    /// `cancel`.
    fn play(&self, wav: &[u8], cancel: &CancelFlag) -> Result<(), PluginError>;

    /// Starts recording `source` as interleaved 16-bit samples at `sample_rate` with `channels`
    /// channels, handing `frames` one 20 ms chunk (`sample_rate / 50 * channels` samples) at a
    /// time from a thread of the backend. Dropping the returned stream stops the recording.
    /// A backend that cannot record answers `unsupported`.
    fn open_capture(
        &self,
        source: &CaptureSource,
        sample_rate: u32,
        channels: u8,
        frames: FrameSink,
    ) -> Result<CaptureStream, PluginError> {
        let _ = (source, sample_rate, channels, frames);
        Err(PluginError::new(
            ErrorCode::Unsupported,
            "this side cannot record audio",
        ))
    }
}

/// What a capture records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureSource {
    /// The default microphone.
    DefaultInput,
    /// The input device with this id from `devices()`.
    Device(String),
    /// What the computer plays (the monitor of the default output).
    Loopback,
}

/// Receives the chunks of one capture; called from a thread of the backend.
pub type FrameSink = Box<dyn FnMut(Vec<i16>) + Send>;

/// A running recording; dropping it stops the recording and releases the device.
pub type CaptureStream = Box<dyn Send>;

/// Grants that let a plugin see the device list (api-audio "Grants").
const AUDIO_GRANTS: [&str; 3] = ["audio.capture", "audio.loopback", "audio.playback"];

/// A 16-bit PCM WAV file of interleaved `samples`.
pub fn pcm_wav(samples: &[i16], sample_rate: u32, channels: u8) -> Vec<u8> {
    let data_bytes = u32::try_from(samples.len() * 2).unwrap_or(u32::MAX);
    let block_align = u16::from(channels) * 2;
    let byte_rate = sample_rate * u32::from(block_align);
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32.saturating_add(data_bytes)).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&u16::from(channels).to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::RequestId;

/// Lowest sample rate any audio call accepts, in hertz.
const MIN_SAMPLE_RATE: u32 = 8_000;
/// Highest sample rate any audio call accepts, in hertz.
const MAX_SAMPLE_RATE: u32 = 192_000;
/// Most capture streams a plugin may hold open (default quota).
const MAX_OPEN_STREAMS: usize = 2;
/// Longest playback one `play` call accepts, in seconds.
const MAX_PLAY_SECONDS: u64 = 30;

/// Open capture streams and the capture id counter of one instance.
struct AudioState {
    next_capture: u64,
    open: HashSet<u64>,
    /// The backend recordings of the open captures that record something.
    streams: HashMap<u64, CaptureStream>,
}

impl Default for AudioState {
    fn default() -> Self {
        Self {
            next_capture: 1,
            open: HashSet::new(),
            streams: HashMap::new(),
        }
    }
}

/// Builds a refusal with the given code and a plain-text message.
fn refuse(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// Checks sample rate and channel count shared by every audio call.
fn validate_format(sample_rate: u32, channels: u8) -> Result<(), PluginError> {
    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            "sample rate must be 8000 to 192000 Hz",
        ));
    }
    match channels {
        0 => Err(refuse(
            ErrorCode::InvalidArgument,
            "channels must be 1 or 2",
        )),
        1 | 2 => Ok(()),
        _ => Err(refuse(
            ErrorCode::Unsupported,
            "only mono and stereo are supported",
        )),
    }
}

/// Host side of the `audio` interface of one plugin instance: its open streams are its own and go
/// away with it. Cloning shares the streams (a test harness keeps one instance and hands out
/// clones).
#[derive(Clone)]
pub struct AudioImport {
    state: Arc<Mutex<AudioState>>,
    env: ImportEnv,
    backend: Option<Arc<dyn AudioBackend>>,
    life: Arc<CancelOnDrop>,
}

impl std::fmt::Debug for AudioImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AudioImport")
            .field("plugin", &self.env.plugin())
            .field("has_backend", &self.backend.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for AudioImport {
    fn default() -> Self {
        Self::new(ImportEnv::unrestricted(), None)
    }
}

impl AudioImport {
    /// The import of the instance `env` describes; `backend` lists devices and plays. Without
    /// one the device list is empty and an accepted playback completes with `unsupported`.
    pub fn new(env: ImportEnv, backend: Option<Arc<dyn AudioBackend>>) -> Self {
        Self {
            state: Arc::default(),
            env,
            backend,
            life: Arc::new(CancelOnDrop::default()),
        }
    }

    fn state(&self) -> MutexGuard<'_, AudioState> {
        // Every writer does one insert or remove, so the state is consistent after a panic.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers a new open stream, enforcing the stream quota.
    fn open_stream(&self) -> Result<u64, PluginError> {
        let mut state = self.state();
        if state.open.len() >= MAX_OPEN_STREAMS {
            return Err(refuse(
                ErrorCode::QuotaExceeded,
                "too many open capture streams",
            ));
        }
        let id = state.next_capture;
        state.next_capture += 1;
        state.open.insert(id);
        Ok(id)
    }

    /// Opens a capture of `source` after its grant and format were checked: counts it against
    /// the quota and starts the backend's recording, whose chunks become `audio-frames` events.
    fn start_capture(
        &self,
        source: CaptureSource,
        sample_rate: u32,
        channels: u8,
    ) -> Result<u64, PluginError> {
        let capture = self.open_stream()?;
        let Some(backend) = &self.backend else {
            return Ok(capture);
        };
        let sink = self.env.sink();
        // Keeps the runtime thread polling for frames while the recording runs.
        let worker = sink.worker();
        let frames: FrameSink = Box::new(move |samples| {
            let _ = &worker;
            sink.send(Event::AudioFrames(AudioFramesEvent { capture, samples }));
        });
        match backend.open_capture(&source, sample_rate, channels, frames) {
            Ok(stream) => {
                self.state().streams.insert(capture, stream);
                Ok(capture)
            }
            Err(failure) => {
                self.state().open.remove(&capture);
                Err(failure)
            }
        }
    }
}

impl HostAudio for AudioImport {
    fn devices(&mut self) -> Vec<AudioDevice> {
        let granted = AUDIO_GRANTS.iter().any(|grant| self.env.holds(grant));
        match &self.backend {
            Some(backend) if granted => backend.devices(),
            _ => Vec::new(),
        }
    }

    fn open_capture(
        &mut self,
        device: Option<String>,
        sample_rate: u32,
        channels: u8,
    ) -> Result<u64, PluginError> {
        validate_format(sample_rate, channels)?;
        if !self.env.holds("audio.capture") {
            return Err(refuse(
                ErrorCode::NotPermitted,
                "audio.capture is not granted",
            ));
        }
        if let Some(device) = &device {
            let listed = self
                .devices()
                .iter()
                .any(|known| known.is_input && &known.id == device);
            if !listed {
                return Err(refuse(ErrorCode::NotFound, "device is not in devices()"));
            }
        }
        let source = match device {
            Some(device) => CaptureSource::Device(device),
            None => CaptureSource::DefaultInput,
        };
        self.start_capture(source, sample_rate, channels)
    }

    fn open_loopback(&mut self, sample_rate: u32, channels: u8) -> Result<u64, PluginError> {
        validate_format(sample_rate, channels)?;
        if !self.env.holds("audio.loopback") {
            return Err(refuse(
                ErrorCode::NotPermitted,
                "audio.loopback is not granted",
            ));
        }
        self.start_capture(CaptureSource::Loopback, sample_rate, channels)
    }

    fn close_capture(&mut self, capture: u64) {
        let stream = {
            let mut state = self.state();
            state.open.remove(&capture);
            state.streams.remove(&capture)
        };
        // Stopping a recording can wait for the backend's reader; do it outside the lock.
        drop(stream);
    }

    fn play(
        &mut self,
        samples: Vec<i16>,
        sample_rate: u32,
        channels: u8,
    ) -> Result<RequestId, PluginError> {
        validate_format(sample_rate, channels)?;
        let sample_count = samples.len() as u64;
        if sample_count % u64::from(channels) != 0 {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                "sample count must be a multiple of the channel count",
            ));
        }
        let frames = sample_count / u64::from(channels);
        if frames > MAX_PLAY_SECONDS * u64::from(sample_rate) {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                "playback is longer than 30 s",
            ));
        }
        if !self.env.holds("audio.playback") {
            return Err(refuse(
                ErrorCode::NotPermitted,
                "audio.playback is not granted",
            ));
        }
        let id = self.env.next_request();
        let sink = self.env.sink();
        let guard = sink.worker();
        let cancel = self.life.flag();
        let backend = self.backend.clone();
        let spawned = std::thread::Builder::new()
            .name("plugin-audio-play".to_owned())
            .spawn(move || {
                let _guard = guard;
                let outcome = match backend {
                    None => Outcome::Failed(refuse(
                        ErrorCode::Unsupported,
                        "no audio output on this side",
                    )),
                    Some(backend) => {
                        let wav = pcm_wav(&samples, sample_rate, channels);
                        match backend.play(&wav, &cancel) {
                            Ok(()) => Outcome::Ok,
                            Err(failure) => Outcome::Failed(failure),
                        }
                    }
                };
                if !cancel.is_set() {
                    sink.send(super::env::completion_event(id, outcome));
                }
            });
        if spawned.is_err() {
            return Err(refuse(
                ErrorCode::Internal,
                "the playback worker did not start",
            ));
        }
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::{pcm_wav, AudioBackend, AudioImport};
    use crate::imports::env::{CancelFlag, ImportEnv};
    use crate::runtime::pending_events::PendingEvents;
    use ilium_plugin_api::generated::host::audio::AudioDevice;
    use ilium_plugin_api::generated::types::events::Event;
    use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// Lists one device and records the WAV files it was asked to play.
    #[derive(Default)]
    struct RecordingOutput {
        played: Mutex<Vec<Vec<u8>>>,
    }

    impl AudioBackend for RecordingOutput {
        fn devices(&self) -> Vec<AudioDevice> {
            vec![AudioDevice {
                id: "speakers".to_owned(),
                name: "Speakers".to_owned(),
                is_input: false,
                is_default: true,
            }]
        }

        fn play(&self, wav: &[u8], _cancel: &CancelFlag) -> Result<(), PluginError> {
            self.played.lock().expect("played").push(wav.to_vec());
            Ok(())
        }
    }

    #[test]
    fn devices_come_from_the_backend_only_while_an_audio_grant_is_held() {
        let backend = Arc::new(RecordingOutput::default());
        let mut granted = AudioImport::new(
            ImportEnv::unrestricted().with_grants(vec!["audio.playback".to_owned()]),
            Some(Arc::clone(&backend) as Arc<dyn AudioBackend>),
        );
        assert_eq!(granted.devices().len(), 1);
        let mut ungranted = AudioImport::new(
            ImportEnv::unrestricted().with_grants(Vec::new()),
            Some(backend as Arc<dyn AudioBackend>),
        );
        assert!(ungranted.devices().is_empty());
    }

    #[test]
    fn play_hands_a_wav_to_the_backend_and_completes() {
        let events = PendingEvents::new();
        let backend = Arc::new(RecordingOutput::default());
        let mut audio = AudioImport::new(
            ImportEnv::unrestricted().with_events(events.clone()),
            Some(Arc::clone(&backend) as Arc<dyn AudioBackend>),
        );
        let request = expect_ok(audio.play(vec![1i16, -1], 16_000, 1));
        let deadline = Instant::now() + Duration::from_secs(5);
        let outcome = loop {
            let found = events
                .drain()
                .into_iter()
                .find_map(|queued| match queued.event {
                    Event::RequestCompleted(result)
                        if result.request == RequestRef::Call(request) =>
                    {
                        Some(result.outcome)
                    }
                    _ => None,
                });
            if let Some(outcome) = found {
                break outcome;
            }
            assert!(Instant::now() < deadline, "playback never completed");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(outcome, Outcome::Ok);
        let played = backend.played.lock().expect("played");
        assert_eq!(played.as_slice(), &[pcm_wav(&[1, -1], 16_000, 1)]);
    }

    #[test]
    fn play_without_the_playback_grant_is_not_permitted() {
        let mut audio = AudioImport::new(
            ImportEnv::unrestricted().with_grants(vec!["audio.capture".to_owned()]),
            None,
        );
        assert_eq!(
            error_code_of(audio.play(vec![0i16; 2], 16_000, 1)),
            ErrorCode::NotPermitted
        );
    }

    #[test]
    fn a_pcm_wav_has_a_valid_header() {
        let wav = pcm_wav(&[0, 1, 2, 3], 48_000, 2);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]), 36 + 8);
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 2);
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            48_000
        );
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + 8);
    }

    /// Flags its drop, so a test sees when the import stopped a recording.
    struct StopFlag(Arc<std::sync::atomic::AtomicBool>);

    impl Drop for StopFlag {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// A recorder that keeps the frame sink of its last capture so the test can feed it.
    #[derive(Default)]
    struct FakeRecorder {
        sinks: Mutex<Vec<(CaptureSource, FrameSink)>>,
        stopped: Arc<std::sync::atomic::AtomicBool>,
        refuse: bool,
    }

    impl AudioBackend for FakeRecorder {
        fn devices(&self) -> Vec<AudioDevice> {
            Vec::new()
        }

        fn play(&self, _wav: &[u8], _cancel: &CancelFlag) -> Result<(), PluginError> {
            Ok(())
        }

        fn open_capture(
            &self,
            source: &CaptureSource,
            _sample_rate: u32,
            _channels: u8,
            frames: FrameSink,
        ) -> Result<CaptureStream, PluginError> {
            if self.refuse {
                return Err(PluginError::new(ErrorCode::Busy, "device held"));
            }
            self.sinks
                .lock()
                .expect("sinks")
                .push((source.clone(), frames));
            Ok(Box::new(StopFlag(Arc::clone(&self.stopped))))
        }
    }

    #[test]
    fn a_capture_delivers_backend_chunks_as_audio_frames_and_stops_on_close() {
        let events = PendingEvents::new();
        let backend = Arc::new(FakeRecorder::default());
        let mut audio = AudioImport::new(
            ImportEnv::unrestricted().with_events(events.clone()),
            Some(Arc::clone(&backend) as Arc<dyn AudioBackend>),
        );
        let capture = expect_ok(audio.open_loopback(16_000, 1));
        {
            let mut sinks = backend.sinks.lock().expect("sinks");
            let (source, sink) = sinks.last_mut().expect("the backend was asked to record");
            assert_eq!(*source, CaptureSource::Loopback);
            sink(vec![7i16; 320]);
        }
        let frames: Vec<AudioFramesEvent> = events
            .drain()
            .into_iter()
            .filter_map(|queued| match queued.event {
                Event::AudioFrames(frames) => Some(frames),
                _ => None,
            })
            .collect();
        assert_eq!(
            frames,
            vec![AudioFramesEvent {
                capture,
                samples: vec![7i16; 320],
            }]
        );
        assert!(!backend.stopped.load(std::sync::atomic::Ordering::SeqCst));
        audio.close_capture(capture);
        assert!(
            backend.stopped.load(std::sync::atomic::Ordering::SeqCst),
            "closing the capture stops the recording"
        );
    }

    #[test]
    fn a_refused_recording_frees_its_quota_slot() {
        let backend = Arc::new(FakeRecorder {
            refuse: true,
            ..FakeRecorder::default()
        });
        let mut audio = AudioImport::new(
            ImportEnv::unrestricted(),
            Some(backend as Arc<dyn AudioBackend>),
        );
        for _ in 0..3 {
            assert_eq!(
                error_code_of(audio.open_capture(None, 16_000, 1)),
                ErrorCode::Busy,
                "the backend's refusal is reported, never quota-exceeded"
            );
        }
    }

    #[test]
    fn capture_and_loopback_need_their_own_grants() {
        let mut audio = AudioImport::new(
            ImportEnv::unrestricted().with_grants(vec!["audio.playback".to_owned()]),
            None,
        );
        assert_eq!(
            error_code_of(audio.open_capture(None, 16_000, 1)),
            ErrorCode::NotPermitted
        );
        assert_eq!(
            error_code_of(audio.open_loopback(16_000, 1)),
            ErrorCode::NotPermitted
        );
    }
    use super::{CaptureSource, CaptureStream, FrameSink};
    use ilium_plugin_api::generated::host::audio::HostAudio;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::events::AudioFramesEvent;

    /// Returns the error code of a failed call; panics when the call succeeded.
    fn error_code_of<T>(result: Result<T, PluginError>) -> ErrorCode {
        match result {
            Ok(_) => panic!("expected an error result, got Ok"),
            Err(error) => error.code,
        }
    }

    /// Panics with the error code when the call failed; passes the success value through.
    fn expect_ok<T>(result: Result<T, PluginError>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected Ok, got error {:?}", error.code),
        }
    }

    /// Source: api-audio.md "Grants" and capability-audio.md "Devices" (no requirement ID).
    #[test]
    fn devices_without_audio_grant_returns_empty_list() {
        let mut audio = AudioImport::default();
        assert!(audio.devices().is_empty());
    }

    /// Source: api-audio.md "Surface" (open_capture returns a capture id) and "Edge cases" table.
    #[test]
    fn open_capture_default_device_mono_16khz_returns_capture_id() {
        let mut audio = AudioImport::default();
        let capture = expect_ok(audio.open_capture(None, 16_000, 1));
        assert_ne!(capture, 0, "a capture id must identify the opened stream");
    }

    /// Source: api-audio.md "Sample format" (stereo is two channels) and "Edge cases" table.
    #[test]
    fn open_capture_default_device_stereo_48khz_is_accepted() {
        let mut audio = AudioImport::default();
        expect_ok(audio.open_capture(None, 48_000, 2));
    }

    /// Source: api-audio.md "Sample format": any rate from 8,000 to 192,000 Hz is accepted (lower boundary).
    #[test]
    fn open_capture_minimum_rate_8000_hz_is_accepted() {
        let mut audio = AudioImport::default();
        expect_ok(audio.open_capture(None, 8_000, 1));
    }

    /// Source: api-audio.md "Sample format": any rate from 8,000 to 192,000 Hz is accepted (upper boundary).
    #[test]
    fn open_capture_maximum_rate_192000_hz_is_accepted() {
        let mut audio = AudioImport::default();
        expect_ok(audio.open_capture(None, 192_000, 2));
    }

    /// Source: api-audio.md "Edge cases" table: rate outside 8,000 to 192,000 Hz gives invalid-argument.
    #[test]
    fn open_capture_rate_one_below_minimum_is_invalid_argument() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.open_capture(None, 7_999, 1)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: rate outside 8,000 to 192,000 Hz gives invalid-argument.
    #[test]
    fn open_capture_rate_one_above_maximum_is_invalid_argument() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.open_capture(None, 192_001, 1)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: zero channels gives invalid-argument.
    #[test]
    fn open_capture_zero_channels_is_invalid_argument() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.open_capture(None, 16_000, 0)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: more than two channels gives unsupported.
    #[test]
    fn open_capture_three_channels_is_unsupported() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.open_capture(None, 16_000, 3)),
            ErrorCode::Unsupported
        );
    }

    /// Source: api-audio.md "Edge cases" table: a device id not in devices() gives not-found.
    #[test]
    fn open_capture_unknown_device_id_is_not_found() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.open_capture(Some("no-such-device".to_string()), 16_000, 1)),
            ErrorCode::NotFound
        );
    }

    /// Source: api-audio.md "Edge cases" table: too many open streams gives quota-exceeded (default 2).
    #[test]
    fn open_capture_third_concurrent_stream_is_quota_exceeded() {
        let mut audio = AudioImport::default();
        expect_ok(audio.open_capture(None, 16_000, 1));
        expect_ok(audio.open_capture(None, 16_000, 1));
        assert_eq!(
            error_code_of(audio.open_capture(None, 16_000, 1)),
            ErrorCode::QuotaExceeded
        );
    }

    /// Source: api-audio.md "Surface" (open_loopback returns a capture id).
    #[test]
    fn open_loopback_valid_rate_and_channels_returns_capture_id() {
        let mut audio = AudioImport::default();
        let capture = expect_ok(audio.open_loopback(48_000, 2));
        assert_ne!(
            capture, 0,
            "a loopback capture id must identify the opened stream"
        );
    }

    /// Source: api-audio.md "Edge cases" table: rate outside 8,000 to 192,000 Hz gives invalid-argument (lower boundary).
    #[test]
    fn open_loopback_rate_one_below_minimum_is_invalid_argument() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.open_loopback(7_999, 1)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: rate outside 8,000 to 192,000 Hz gives invalid-argument (upper boundary).
    #[test]
    fn open_loopback_rate_one_above_maximum_is_invalid_argument() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.open_loopback(192_001, 1)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: zero channels gives invalid-argument.
    #[test]
    fn open_loopback_zero_channels_is_invalid_argument() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.open_loopback(48_000, 0)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: more than two channels gives unsupported.
    #[test]
    fn open_loopback_three_channels_is_unsupported() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.open_loopback(48_000, 3)),
            ErrorCode::Unsupported
        );
    }

    /// Source: api-audio.md "Edge cases" table: close-capture of an unknown or closed id does nothing.
    #[test]
    fn close_capture_unknown_id_does_nothing() {
        let mut audio = AudioImport::default();
        audio.close_capture(0);
        audio.close_capture(u64::MAX);
    }

    /// Source: api-audio.md "Surface" (close-capture releases the device) and "Edge cases" table (stream quota).
    #[test]
    fn close_capture_releases_stream_slot_for_a_new_capture() {
        let mut audio = AudioImport::default();
        let first = expect_ok(audio.open_capture(None, 16_000, 1));
        expect_ok(audio.open_capture(None, 16_000, 1));
        audio.close_capture(first);
        expect_ok(audio.open_capture(None, 16_000, 1));
    }

    /// Source: api-audio.md "Surface" (play returns on request id) and "Sample format" (interleaved s16, stereo).
    #[test]
    fn play_stereo_interleaved_samples_returns_request_id() {
        let mut audio = AudioImport::default();
        expect_ok(audio.play(vec![0i16, 0, 1_000, -1_000], 16_000, 2));
    }

    /// Source: api-audio.md "Sample format": one event (20 ms) at 16 kHz mono holds 320 samples.
    #[test]
    fn play_one_chunk_mono_samples_returns_request_id() {
        let mut audio = AudioImport::default();
        expect_ok(audio.play(vec![0i16; 320], 16_000, 1));
    }

    /// Source: api-audio.md "Edge cases" table: empty input is a sample count of 0, a multiple of any channel count.
    #[test]
    fn play_empty_samples_is_accepted() {
        let mut audio = AudioImport::default();
        expect_ok(audio.play(Vec::new(), 16_000, 1));
    }

    /// Source: api-audio.md "Edge cases" table: sample count not a multiple of channels gives invalid-argument.
    #[test]
    fn play_sample_count_not_multiple_of_channels_is_invalid_argument() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.play(vec![0i16; 3], 16_000, 2)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: playback up to 30 s is accepted (boundary: 480,000 samples at 16 kHz mono).
    #[test]
    fn play_exactly_thirty_seconds_is_accepted() {
        let mut audio = AudioImport::default();
        expect_ok(audio.play(vec![0i16; 480_000], 16_000, 1));
    }

    /// Source: api-audio.md "Edge cases" table: longer than 30 s gives invalid-argument (480,001 samples at 16 kHz mono).
    #[test]
    fn play_just_over_thirty_seconds_is_invalid_argument() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.play(vec![0i16; 480_001], 16_000, 1)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Sample format": any rate from 8,000 to 192,000 Hz is accepted; out-of-range rate for play is inferred invalid-argument.
    #[test]
    fn play_rate_below_minimum_is_invalid_argument() {
        let mut audio = AudioImport::default();
        assert_eq!(
            error_code_of(audio.play(vec![0i16; 320], 7_999, 1)),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn the_stream_quota_belongs_to_the_instance() {
        let mut first = AudioImport::default();
        let mut second = AudioImport::default();
        first.open_loopback(48_000, 2).expect("first stream");
        first.open_loopback(48_000, 2).expect("second stream");
        assert_eq!(
            error_code_of(first.open_loopback(48_000, 2)),
            ErrorCode::QuotaExceeded
        );
        second
            .open_loopback(48_000, 2)
            .expect("another instance has its own quota");
    }

    #[test]
    fn closing_a_stream_through_a_clone_frees_the_slot() {
        let mut audio = AudioImport::default();
        let mut same_instance = audio.clone();
        let stream = audio.open_loopback(48_000, 2).expect("stream");
        audio.open_loopback(48_000, 2).expect("stream");
        same_instance.close_capture(stream);
        audio
            .open_loopback(48_000, 2)
            .expect("the slot is free again");
    }
}
