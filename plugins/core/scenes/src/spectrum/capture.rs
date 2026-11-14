//! Audio input of the spectrum scene.
//!
//! The host records: `open-loopback` captures what the system is playing (the monitor of the
//! default output), `open-capture` the default or a named input device. The host converts to
//! the rate and channel count asked for and delivers 20 ms chunks of 16-bit samples as
//! `audio-frames` events to the component's `handler` export (`ScenesScene::on_events`), which
//! files them in the mailbox of their capture here. A scene owns no thread: at the start of each
//! frame it drains its mailbox ([`AudioInput::take`]) and runs the analysis of the new samples
//! in place ([`Pipeline::push`]).
//!
//! The mailbox keeps a bounded number of chunks per capture and only for captures that are
//! open, so a scene that stops drawing (or a capture that was closed) never piles up audio.

use super::dsp::{band_edges, Analyzer, DB_FLOOR};
use super::{BandScale, InputKind};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

/// Rate the scene asks the host to record at.
pub const SAMPLE_RATE: u32 = 44_100;
/// Samples of the oscilloscope trace.
pub const WAVEFORM_LEN: usize = 1024;
/// At most this many analyses per second of audio.
const ANALYSES_PER_SECOND: u32 = 60;
/// Chunks a mailbox keeps (20 ms each, so about 2.5 seconds); older ones are dropped.
const MAILBOX_CHUNKS: usize = 128;

/// Where the audio comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTarget {
    /// What the system is playing (loopback of the default output).
    SystemOutput,
    /// The default microphone or input.
    DefaultInput,
    /// A device by name (a Pulse/PipeWire source name, or part of a device name).
    Named(String),
}

impl CaptureTarget {
    /// The target the settings ask for; a named device without a name is the default input.
    pub fn from_settings(input: InputKind, device_name: &str) -> Self {
        match input {
            InputKind::SystemOutput => Self::SystemOutput,
            InputKind::Microphone => Self::DefaultInput,
            InputKind::NamedDevice if device_name.trim().is_empty() => Self::DefaultInput,
            InputKind::NamedDevice => Self::Named(device_name.trim().to_owned()),
        }
    }
}

/// The sound input under the scene: opens one recording and hands over what arrived.
pub trait AudioInput: Send {
    /// Starts recording `target` in mono at `sample_rate`; the capture id for `take`.
    fn open(&mut self, target: &CaptureTarget, sample_rate: u32) -> Result<u64, String>;
    /// Appends the mono samples of `capture` that arrived since the last call, oldest first,
    /// scaled to -1.0..=1.0.
    fn take(&mut self, capture: u64, out: &mut Vec<f32>);
    /// Stops recording `capture`.
    fn close(&mut self, capture: u64);
}

/// Chunks of open captures, filled by the handler export and drained by the scenes.
static MAILBOX: Mutex<BTreeMap<u64, VecDeque<Vec<i16>>>> = Mutex::new(BTreeMap::new());

fn mailbox() -> std::sync::MutexGuard<'static, BTreeMap<u64, VecDeque<Vec<i16>>>> {
    MAILBOX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Opens the mailbox of `capture`.
fn register(capture: u64) {
    mailbox().entry(capture).or_default();
}

/// Closes the mailbox of `capture` and drops what it still held.
fn unregister(capture: u64) {
    mailbox().remove(&capture);
}

/// Files one chunk of `capture` (an `audio-frames` event). A chunk of a capture that is not
/// open is dropped; a full mailbox drops its oldest chunk.
pub fn deliver(capture: u64, samples: Vec<i16>) {
    let mut boxes = mailbox();
    let Some(queue) = boxes.get_mut(&capture) else {
        return;
    };
    if queue.len() >= MAILBOX_CHUNKS {
        queue.pop_front();
    }
    queue.push_back(samples);
}

/// Moves every chunk of `capture` into `out` as samples scaled to -1.0..=1.0.
fn drain(capture: u64, out: &mut Vec<f32>) {
    let chunks: Vec<Vec<i16>> = mailbox()
        .get_mut(&capture)
        .map(|queue| queue.drain(..).collect())
        .unwrap_or_default();
    for chunk in chunks {
        out.extend(chunk.into_iter().map(|sample| f32::from(sample) / 32_768.0));
    }
}

/// The host's `audio` import; the chunks come through the mailbox.
#[derive(Debug, Default)]
pub struct HostAudioInput;

impl AudioInput for HostAudioInput {
    fn open(&mut self, target: &CaptureTarget, sample_rate: u32) -> Result<u64, String> {
        use ilium_plugin_sdk::generated::host::audio::HostAudio;
        let mut host = ilium_plugin_sdk::host();
        let opened = match target {
            CaptureTarget::SystemOutput => HostAudio::open_loopback(&mut *host, sample_rate, 1),
            CaptureTarget::DefaultInput => {
                HostAudio::open_capture(&mut *host, None, sample_rate, 1)
            }
            CaptureTarget::Named(name) => {
                HostAudio::open_capture(&mut *host, Some(name.clone()), sample_rate, 1)
            }
        };
        let capture = opened.map_err(|error| error.message)?;
        register(capture);
        Ok(capture)
    }

    fn take(&mut self, capture: u64, out: &mut Vec<f32>) {
        drain(capture, out);
    }

    fn close(&mut self, capture: u64) {
        use ilium_plugin_sdk::generated::host::audio::HostAudio;
        unregister(capture);
        HostAudio::close_capture(&mut *ilium_plugin_sdk::host(), capture);
    }
}

/// One analysis result.
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
            level_db: DB_FLOOR,
        }
    }
}

/// What the input is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerStatus {
    Starting,
    Running,
    Failed(String),
}

/// Analysis parameters; a subset of the user settings.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisConfig {
    pub fft_size: usize,
    pub bands: usize,
    pub scale: BandScale,
    pub min_hz: f32,
    pub max_hz: f32,
    pub tilt_db_per_octave: f32,
}

/// The analysis of a sample stream: a ring of the newest samples, analysed once at least one
/// hop of new samples arrived.
pub struct Pipeline {
    analyzer: Analyzer,
    ring: Vec<f32>,
    fft_size: usize,
    hop: usize,
    keep: usize,
    fresh: usize,
    sequence: u64,
}

impl Pipeline {
    /// The analysis of `config` for samples at `sample_rate`.
    pub fn new(config: &AnalysisConfig, sample_rate: u32) -> Result<Self, String> {
        let rate = sample_rate.max(8_000);
        let edges = band_edges(config.scale, config.bands, config.min_hz, config.max_hz);
        let analyzer = Analyzer::new(config.fft_size, rate, &edges, config.tilt_db_per_octave)?;
        let keep = config.fft_size.max(WAVEFORM_LEN);
        Ok(Self {
            analyzer,
            ring: Vec::with_capacity(keep * 2),
            fft_size: config.fft_size,
            hop: (rate / ANALYSES_PER_SECOND).max(64) as usize,
            keep,
            fresh: 0,
            sequence: 0,
        })
    }

    /// Adds `samples`; the newest analysis when enough new samples arrived for one.
    pub fn push(&mut self, samples: &[f32]) -> Option<Snapshot> {
        if samples.is_empty() {
            return None;
        }
        self.fresh += samples.len();
        self.ring.extend_from_slice(samples);
        if self.ring.len() > self.keep * 2 {
            let excess = self.ring.len() - self.keep;
            self.ring.drain(..excess);
        }
        if self.ring.len() < self.fft_size || self.fresh < self.hop {
            return None;
        }
        self.fresh = 0;
        let analysis = self.analyzer.analyze(&self.ring);
        self.sequence += 1;
        Some(Snapshot {
            seq: self.sequence,
            bands_db: analysis.bands_db,
            waveform: self.ring[self.ring.len().saturating_sub(WAVEFORM_LEN)..].to_vec(),
            level_db: analysis.level_db,
        })
    }
}

#[cfg(test)]
pub mod testing {
    //! Fake inputs for tests: no host, no audio hardware.
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// A signal generator: `next` returns the sample at index `n`.
    pub type Signal = Box<dyn FnMut(usize) -> f32 + Send>;

    /// An input that delivers `per_take` samples of a signal at every `take`, or fails to open.
    pub struct FakeInput {
        signal: Option<Signal>,
        produced: usize,
        per_take: usize,
        failure: Option<String>,
        pub closed: Arc<AtomicBool>,
    }

    impl FakeInput {
        /// An input whose `open` fails with `reason`.
        pub fn failing(reason: &str) -> Self {
            Self {
                signal: None,
                produced: 0,
                per_take: 0,
                failure: Some(reason.to_owned()),
                closed: Arc::new(AtomicBool::new(false)),
            }
        }

        /// An input that opens and never delivers a sample.
        pub fn silent() -> Self {
            Self {
                signal: None,
                produced: 0,
                per_take: 0,
                failure: None,
                closed: Arc::new(AtomicBool::new(false)),
            }
        }

        /// An endless sine of `frequency` Hz at `amplitude` (1.0 = full scale).
        pub fn sine(frequency: f32, amplitude: f32) -> Self {
            let rate = f64::from(SAMPLE_RATE);
            Self::signal(Box::new(move |index| {
                let phase = 2.0 * std::f64::consts::PI * f64::from(frequency) * index as f64 / rate;
                amplitude * phase.sin() as f32
            }))
        }

        /// Music-like signal: a harmonic stack with a slow tremolo, a beat-like bass pulse and
        /// a little noise, for visual review of the styles.
        pub fn music() -> Self {
            let mut noise_state = 0x2545_f491u32;
            Self::signal(Box::new(move |index| {
                let tau = 2.0 * std::f64::consts::PI;
                let t = index as f64 / f64::from(SAMPLE_RATE);
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
                noise_state = noise_state
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
                value += 0.03 * ((noise_state >> 8) as f64 / (1u64 << 24) as f64 - 0.5);
                value as f32 * 0.6
            }))
        }

        fn signal(signal: Signal) -> Self {
            Self {
                signal: Some(signal),
                produced: 0,
                per_take: 4096,
                failure: None,
                closed: Arc::new(AtomicBool::new(false)),
            }
        }

        /// Stops delivering samples from now on (a stalled capture).
        pub fn stall(&mut self) {
            self.per_take = 0;
        }
    }

    impl AudioInput for FakeInput {
        fn open(&mut self, _target: &CaptureTarget, _sample_rate: u32) -> Result<u64, String> {
            match &self.failure {
                Some(reason) => Err(reason.clone()),
                None => Ok(7),
            }
        }

        fn take(&mut self, _capture: u64, out: &mut Vec<f32>) {
            if let Some(signal) = self.signal.as_mut() {
                for _ in 0..self.per_take {
                    out.push(signal(self.produced));
                    self.produced += 1;
                }
            }
        }

        fn close(&mut self, _capture: u64) {
            self.closed.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AnalysisConfig {
        AnalysisConfig {
            fft_size: 2048,
            bands: 32,
            scale: BandScale::Log,
            min_hz: 40.0,
            max_hz: 16_000.0,
            tilt_db_per_octave: 0.0,
        }
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
        assert_eq!(
            CaptureTarget::from_settings(InputKind::NamedDevice, "  "),
            CaptureTarget::DefaultInput
        );
    }

    #[test]
    fn the_mailbox_keeps_only_open_captures_and_bounds_each_one() {
        // Ids far from the ones other tests use.
        let open = 9_001;
        let closed = 9_002;
        deliver(closed, vec![1, 2, 3]);
        let mut out = Vec::new();
        drain(closed, &mut out);
        assert!(out.is_empty(), "a capture that is not open keeps nothing");

        register(open);
        for index in 0..(MAILBOX_CHUNKS + 10) {
            deliver(open, vec![index as i16]);
        }
        drain(open, &mut out);
        assert_eq!(out.len(), MAILBOX_CHUNKS, "the oldest chunks are dropped");
        assert_eq!(out[0], 10.0 / 32_768.0);
        out.clear();
        drain(open, &mut out);
        assert!(out.is_empty(), "draining empties the mailbox");

        deliver(open, vec![i16::MIN, i16::MAX]);
        unregister(open);
        drain(open, &mut out);
        assert!(out.is_empty(), "closing drops what was left");
        deliver(open, vec![5]);
        drain(open, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn the_pipeline_analyses_a_sine_into_its_band() {
        let mut pipeline = Pipeline::new(&config(), SAMPLE_RATE).unwrap();
        assert!(pipeline.push(&[]).is_none());
        // Not a full window yet.
        assert!(pipeline.push(&vec![0.0; 1000]).is_none());
        let rate = SAMPLE_RATE as f32;
        let sine: Vec<f32> = (0..4096)
            .map(|index| 0.5 * (2.0 * std::f32::consts::PI * 1000.0 * index as f32 / rate).sin())
            .collect();
        let snapshot = pipeline.push(&sine).expect("a full window is analysed");
        assert_eq!(snapshot.seq, 1);
        assert_eq!(snapshot.waveform.len(), WAVEFORM_LEN);
        assert!(snapshot.level_db > -20.0, "{}", snapshot.level_db);
        let edges = band_edges(BandScale::Log, 32, 40.0, 16_000.0);
        let expected = edges.iter().position(|edge| *edge > 1000.0).unwrap() - 1;
        let loudest = snapshot
            .bands_db
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .unwrap();
        assert_eq!(loudest, expected);
        // Fewer new samples than one hop: no new analysis.
        assert!(pipeline.push(&sine[..10]).is_none());
        assert_eq!(pipeline.push(&sine).map(|next| next.seq), Some(2));
    }

    #[test]
    fn the_ring_stays_bounded() {
        let mut pipeline = Pipeline::new(&config(), SAMPLE_RATE).unwrap();
        for _ in 0..50 {
            pipeline.push(&vec![0.1; 10_000]);
        }
        assert!(pipeline.ring.len() <= pipeline.keep * 2);
    }

    #[test]
    fn an_invalid_fft_size_is_refused() {
        let config = AnalysisConfig {
            fft_size: 1000,
            ..config()
        };
        assert!(Pipeline::new(&config, SAMPLE_RATE).is_err());
    }
}
