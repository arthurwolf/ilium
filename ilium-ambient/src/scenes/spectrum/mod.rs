//! Spectrum analyzer of the audio currently playing on the system, drawn in
//! Braille. See `capture` for how audio is obtained, `dsp` for the analysis
//! and smoothing math and `draw` for the eight visual styles.
//!
//! Threading: a `source::Worker` owned by the scene captures and analyses
//! audio and publishes immutable snapshots; `render` only reads the newest
//! one with `try_lock`, animates it (attack, gravity, peak hold) from the
//! frame clock and draws. Dropping the scene stops the worker and any helper
//! process.

mod capture;
mod draw;
mod dsp;

use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use crate::source::Worker;
use capture::{
    new_shared, plan_factory, spawn_capture, AnalysisConfig, CaptureTarget, Shared, Snapshot,
    SourceFactory, WorkerStatus, WAVEFORM_LEN,
};
use draw::{DrawInput, EnergyLevels, Ripple, SpectrogramHistory};
use dsp::{band_centers, band_edges, BandState, Dynamics, LevelScaler, SILENCE_RMS_DB};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// Declares a settings enum that is edited as a `Choice` control: the serde
/// name is the snake_case variant, the label is what the Settings panel shows.
macro_rules! choice_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub const LABELS: &'static [&'static str] = &[$($label),+];

            fn index(self) -> usize {
                Self::ALL.iter().position(|candidate| *candidate == self).unwrap_or(0)
            }

            fn from_index(index: usize) -> Option<Self> {
                Self::ALL.get(index).copied()
            }
        }
    };
}

choice_enum! {
    /// Where the audio comes from.
    InputKind {
        SystemOutput => "System output (loopback)",
        Microphone => "Default microphone / input",
        NamedDevice => "Named device",
    }
}

choice_enum! {
    SpectrumStyle {
        Bars => "Bars",
        MirroredBars => "Mirrored bars",
        Line => "Spectrum line",
        Area => "Filled area",
        Waveform => "Waveform (oscilloscope)",
        Radial => "Radial",
        Spectrogram => "Spectrogram",
        PulseRings => "Pulse rings",
    }
}

choice_enum! {
    /// Which edge the bars grow from. Ignored by radial and ring styles.
    Orientation {
        BottomUp => "Bottom up",
        TopDown => "Top down",
        LeftToRight => "Left to right",
        RightToLeft => "Right to left",
    }
}

choice_enum! {
    BandScale {
        Log => "Logarithmic",
        Mel => "Mel",
        Linear => "Linear",
    }
}

choice_enum! {
    ColorMode {
        Mono => "Mono ink",
        Rainbow => "Rainbow",
        Heat => "Heat",
        Ice => "Blue-white",
    }
}

/// Settings of the spectrum scene. Every field is clamped by `normalized`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpectrumSettings {
    /// Audio source: system output loopback, default input, or a named device.
    pub input: InputKind,
    /// Device to capture in `named_device` mode. Linux (PulseAudio/PipeWire):
    /// a source or sink-monitor name from `pactl list short sources`.
    /// Windows/macOS: part of a device name, for example `BlackHole`.
    pub device_name: String,
    pub style: SpectrumStyle,
    pub orientation: Orientation,
    pub color_mode: ColorMode,
    pub band_scale: BandScale,
    /// Number of analysis bands, 8..=128 (default 48).
    pub bands: u32,
    /// Lowest displayed frequency, 20..=1000 Hz (default 40).
    pub min_frequency: u32,
    /// Highest displayed frequency, 2000..=20000 Hz (default 16000).
    pub max_frequency: u32,
    /// FFT window: 2048 (faster) or 4096 (finer bass), default 2048.
    pub fft_size: u32,
    /// Auto-gain makes the loudest band reach the top whatever the volume.
    pub auto_gain: bool,
    /// Level shown as zero height, -100..=-30 dBFS (default -60).
    pub floor_db: i32,
    /// Level shown as full height, -30..=0 dBFS (default -10).
    pub ceiling_db: i32,
    /// Output gain, 25..=400 percent (default 100).
    pub sensitivity: u32,
    /// Bar fall-off and peak hold time, 0..=100 percent (default 60).
    pub smoothing: u32,
    /// Treble boost in dB per octave, 0..=6 (default 3): music falls off
    /// about 3 dB per octave, so this evens out the display.
    pub tilt: u32,
    pub peak_hold: bool,
    /// Mirror left/right with the bass in the middle.
    pub mirror: bool,
    /// Bar width in dots, 0..=8; 0 fits one bar per band (default 0).
    pub bar_width: u32,
    /// Gap between bars in dots, 0..=4 (default 1).
    pub bar_gap: u32,
    /// Spectrogram scroll speed, 10..=60 dots per second (default 30).
    pub spectrogram_speed: u32,
    /// Redraw rate, 10..=30 frames per second (default 24).
    pub refresh_rate: u32,
}

impl Default for SpectrumSettings {
    fn default() -> Self {
        Self {
            input: InputKind::SystemOutput,
            device_name: String::new(),
            style: SpectrumStyle::Bars,
            orientation: Orientation::BottomUp,
            color_mode: ColorMode::Mono,
            band_scale: BandScale::Log,
            bands: 48,
            min_frequency: 40,
            max_frequency: 16_000,
            fft_size: 2048,
            auto_gain: true,
            floor_db: -60,
            ceiling_db: -10,
            sensitivity: 100,
            smoothing: 60,
            tilt: 3,
            peak_hold: true,
            mirror: false,
            bar_width: 0,
            bar_gap: 1,
            spectrogram_speed: 30,
            refresh_rate: 24,
        }
    }
}

const MAX_DEVICE_NAME_LEN: usize = 200;

impl SceneSettings for SpectrumSettings {
    fn normalized(&self) -> Self {
        let floor_db = self.floor_db.clamp(-100, -30);
        Self {
            device_name: self
                .device_name
                .trim()
                .chars()
                .filter(|character| !character.is_control())
                .take(MAX_DEVICE_NAME_LEN)
                .collect(),
            bands: self.bands.clamp(8, 128),
            min_frequency: self.min_frequency.clamp(20, 1000),
            max_frequency: self.max_frequency.clamp(2000, 20_000),
            fft_size: if self.fft_size < 3072 { 2048 } else { 4096 },
            floor_db,
            ceiling_db: self.ceiling_db.clamp(-30, 0).max(floor_db + 10),
            sensitivity: self.sensitivity.clamp(25, 400),
            smoothing: self.smoothing.min(100),
            tilt: self.tilt.min(6),
            bar_width: self.bar_width.min(8),
            bar_gap: self.bar_gap.min(4),
            spectrogram_speed: self.spectrogram_speed.clamp(10, 60),
            refresh_rate: self.refresh_rate.clamp(10, 30),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        let style = self.style;
        let uses_bands = style != SpectrumStyle::Waveform;
        let has_bars = matches!(style, SpectrumStyle::Bars | SpectrumStyle::MirroredBars);
        let has_peaks = has_bars
            || matches!(
                style,
                SpectrumStyle::Line | SpectrumStyle::Area | SpectrumStyle::Radial
            );
        let is_oriented = !matches!(style, SpectrumStyle::Radial | SpectrumStyle::PulseRings);
        let mut rows = vec![Control::choice(
            "input",
            "Audio source",
            self.input.index(),
            InputKind::LABELS,
            "System output shows what is playing right now. On Windows and macOS it uses loopback capture; older macOS needs a virtual device such as BlackHole (use Named device).",
        )];
        if self.input == InputKind::NamedDevice {
            rows.push(Control::text(
                "device_name",
                "Device name",
                &self.device_name,
                "device name",
                "Linux: a PulseAudio/PipeWire source name (see `pactl list short sources`, add .monitor for an output). Windows/macOS: part of the device name.",
            ));
        }
        rows.push(Control::choice(
            "style",
            "Style",
            style.index(),
            SpectrumStyle::LABELS,
            "Bars, mirrored bars, a thin spectrum line, a filled area, the raw waveform, a radial spectrum, a scrolling spectrogram, or bass-pulse rings.",
        ));
        if is_oriented {
            rows.push(Control::choice(
                "orientation",
                "Orientation",
                self.orientation.index(),
                Orientation::LABELS,
                "The edge the spectrum grows from.",
            ));
        }
        rows.push(Control::choice(
            "color_mode",
            "Colors",
            self.color_mode.index(),
            ColorMode::LABELS,
            "Mono uses the global ink color. Rainbow colors by frequency, Heat and Blue-white by height or intensity.",
        ));
        if uses_bands {
            rows.push(Control::choice(
                "band_scale",
                "Frequency scale",
                self.band_scale.index(),
                BandScale::LABELS,
                "Spacing of the bands: logarithmic matches music, mel matches hearing, linear shows treble in detail.",
            ));
            rows.push(Control::slider(
                "bands",
                "Bands",
                self.bands as i32,
                (8, 128, 4),
                "",
                "Number of frequency bands analysed.",
            ));
            rows.push(Control::slider(
                "min_frequency",
                "Lowest frequency",
                self.min_frequency as i32,
                (20, 1000, 10),
                " Hz",
                "Left edge of the spectrum.",
            ));
            rows.push(Control::slider(
                "max_frequency",
                "Highest frequency",
                self.max_frequency as i32,
                (2000, 20_000, 500),
                " Hz",
                "Right edge of the spectrum.",
            ));
            rows.push(Control::choice(
                "fft_size",
                "FFT size",
                usize::from(self.fft_size >= 3072),
                &["2048 (quicker response)", "4096 (finer bass)"],
                "Longer windows resolve low notes better but react a little slower.",
            ));
            rows.push(Control::slider(
                "tilt",
                "Treble boost",
                self.tilt as i32,
                (0, 6, 1),
                " dB/oct",
                "Compensates the natural roll-off of music so treble is visible.",
            ));
        }
        rows.push(Control::toggle(
            "auto_gain",
            "Auto gain",
            self.auto_gain,
            "Follow the loudness so the display always fills the height. When on, only the span between floor and ceiling is used.",
        ));
        if uses_bands {
            rows.push(Control::slider(
                "floor_db",
                "Floor",
                self.floor_db,
                (-100, -30, 5),
                " dB",
                "Level shown as zero height.",
            ));
            rows.push(Control::slider(
                "ceiling_db",
                "Ceiling",
                self.ceiling_db,
                (-30, 0, 2),
                " dB",
                "Level shown as full height (fixed gain only).",
            ));
        }
        rows.push(Control::slider(
            "sensitivity",
            "Sensitivity",
            self.sensitivity as i32,
            (25, 400, 5),
            "%",
            "Multiplies the displayed height.",
        ));
        rows.push(Control::slider(
            "smoothing",
            "Smoothing",
            self.smoothing as i32,
            (0, 100, 5),
            "%",
            "How slowly bars fall back and how long peaks are held.",
        ));
        if has_peaks {
            rows.push(Control::toggle(
                "peak_hold",
                "Peak markers",
                self.peak_hold,
                "Show a marker that rests at recent peaks, then falls.",
            ));
        }
        if !matches!(style, SpectrumStyle::Waveform | SpectrumStyle::PulseRings) {
            rows.push(Control::toggle(
                "mirror",
                "Mirror",
                self.mirror,
                "Mirror the spectrum around the middle, with the bass at the centre.",
            ));
        }
        if has_bars {
            rows.push(Control::slider(
                "bar_width",
                "Bar width",
                self.bar_width as i32,
                (0, 8, 1),
                " dots",
                "Width of each bar in Braille dots; 0 fits one bar per band.",
            ));
            rows.push(Control::slider(
                "bar_gap",
                "Bar gap",
                self.bar_gap as i32,
                (0, 4, 1),
                " dots",
                "Space between bars.",
            ));
        }
        if style == SpectrumStyle::Spectrogram {
            rows.push(Control::slider(
                "spectrogram_speed",
                "Scroll speed",
                self.spectrogram_speed as i32,
                (10, 60, 5),
                " dots/s",
                "How fast the spectrogram scrolls.",
            ));
        }
        rows.push(Control::slider(
            "refresh_rate",
            "Refresh rate",
            self.refresh_rate as i32,
            (10, 30, 2),
            " fps",
            "Redraws per second. Higher is smoother and costs more CPU.",
        ));
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match id {
            "input" => set_choice(&mut self.input, &value, InputKind::from_index),
            "style" => set_choice(&mut self.style, &value, SpectrumStyle::from_index),
            "orientation" => set_choice(&mut self.orientation, &value, Orientation::from_index),
            "color_mode" => set_choice(&mut self.color_mode, &value, ColorMode::from_index),
            "band_scale" => set_choice(&mut self.band_scale, &value, BandScale::from_index),
            "fft_size" => {
                if let Some(index) = control::index(&value) {
                    self.fft_size = if index == 0 { 2048 } else { 4096 };
                }
            }
            "device_name" => {
                let Some(text) = control::text(&value) else {
                    return Ok(false);
                };
                if text.chars().count() > MAX_DEVICE_NAME_LEN {
                    return Err(format!(
                        "Device name is longer than {MAX_DEVICE_NAME_LEN} characters"
                    ));
                }
                if text.chars().any(char::is_control) {
                    return Err("Device name contains control characters".to_owned());
                }
                self.device_name = text.trim().to_owned();
            }
            "bands" => set_unsigned(&mut self.bands, &value),
            "min_frequency" => set_unsigned(&mut self.min_frequency, &value),
            "max_frequency" => set_unsigned(&mut self.max_frequency, &value),
            "sensitivity" => set_unsigned(&mut self.sensitivity, &value),
            "smoothing" => set_unsigned(&mut self.smoothing, &value),
            "tilt" => set_unsigned(&mut self.tilt, &value),
            "bar_width" => set_unsigned(&mut self.bar_width, &value),
            "bar_gap" => set_unsigned(&mut self.bar_gap, &value),
            "spectrogram_speed" => set_unsigned(&mut self.spectrogram_speed, &value),
            "refresh_rate" => set_unsigned(&mut self.refresh_rate, &value),
            "floor_db" => {
                if let Some(number) = control::number(&value) {
                    self.floor_db = number;
                }
            }
            "ceiling_db" => {
                if let Some(number) = control::number(&value) {
                    self.ceiling_db = number;
                }
            }
            "auto_gain" => {
                if let Some(on) = control::boolean(&value) {
                    self.auto_gain = on;
                }
            }
            "peak_hold" => {
                if let Some(on) = control::boolean(&value) {
                    self.peak_hold = on;
                }
            }
            "mirror" => {
                if let Some(on) = control::boolean(&value) {
                    self.mirror = on;
                }
            }
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}

fn set_choice<T>(target: &mut T, value: &ControlValue, from_index: fn(usize) -> Option<T>) {
    if let Some(choice) = control::index(value).and_then(from_index) {
        *target = choice;
    }
}

fn set_unsigned(target: &mut u32, value: &ControlValue) {
    if let Some(number) = control::number(value) {
        *target = number.max(0) as u32;
    }
}

/// What the input is doing, for `status()` and the idle animation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AudioState {
    Starting,
    Live,
    Silent,
    Failed(String),
}

/// Band index ranges of the bass (<200 Hz), mid (200 Hz-2 kHz) and treble
/// regions, used by the ring style and the radial swell.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BandGroups {
    bass: std::ops::Range<usize>,
    mid: std::ops::Range<usize>,
    treble: std::ops::Range<usize>,
}

impl BandGroups {
    fn from_centers(centers: &[f32]) -> Self {
        let count = centers.len();
        let bass_end = centers
            .iter()
            .take_while(|hz| **hz < 200.0)
            .count()
            .clamp(1, count.max(1));
        let mid_end = centers
            .iter()
            .take_while(|hz| **hz < 2000.0)
            .count()
            .clamp(bass_end, count.max(1));
        Self {
            bass: 0..bass_end.min(count),
            mid: bass_end.min(count)..mid_end.min(count),
            treble: mid_end.min(count)..count,
        }
    }

    fn mean(values: &[f32], range: &std::ops::Range<usize>) -> f32 {
        let slice = values.get(range.clone()).unwrap_or(&[]);
        if slice.is_empty() {
            0.0
        } else {
            slice.iter().sum::<f32>() / slice.len() as f32
        }
    }
}

const RIPPLE_SECONDS: f32 = 1.6;
const HISTORY_CAPACITY: usize = 1200;
/// A capture that stops delivering snapshots for this long counts as idle.
const STALE_AFTER: Duration = Duration::from_millis(1200);
/// Audio must stay below the silence threshold this long before the idle
/// animation takes over, so short pauses do not flicker.
const SILENCE_HOLD: Duration = Duration::from_millis(1500);

struct RippleState {
    born: Duration,
    strength: f32,
}

pub struct SpectrumScene {
    settings: SpectrumSettings,
    shared: Shared,
    // Dropped with the scene: stops the capture thread and its helper process.
    _worker: Worker,
    groups: BandGroups,
    dynamics: Dynamics,
    scaler: LevelScaler,
    bands: Vec<BandState>,
    snapshot: Arc<Snapshot>,
    worker_status: WorkerStatus,
    audio: AudioState,
    last_wall: Option<Duration>,
    snapshot_changed_at: Duration,
    last_loud_at: Option<Duration>,
    history: SpectrogramHistory,
    history_accumulator: f32,
    levels: EnergyLevels,
    previous_bass: f32,
    ripples: Vec<RippleState>,
    last_ripple_at: Option<Duration>,
    wave_peak: f32,
    targets: Vec<f32>,
    values: Vec<f32>,
    peaks: Vec<f32>,
    waveform: Vec<f32>,
}

impl SpectrumScene {
    pub fn new(settings: &SpectrumSettings, _env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        let target = CaptureTarget::from_settings(settings.input, &settings.device_name);
        Self::with_factory(&settings, plan_factory(target))
    }

    fn with_factory(settings: &SpectrumSettings, factory: SourceFactory) -> Self {
        let settings = settings.normalized();
        let shared = new_shared();
        let config = AnalysisConfig {
            fft_size: settings.fft_size as usize,
            bands: settings.bands as usize,
            scale: settings.band_scale,
            min_hz: settings.min_frequency as f32,
            max_hz: settings.max_frequency as f32,
            tilt_db_per_octave: settings.tilt as f32,
        };
        let worker = spawn_capture(Arc::clone(&shared), config, factory);
        let edges = band_edges(
            settings.band_scale,
            settings.bands as usize,
            settings.min_frequency as f32,
            settings.max_frequency as f32,
        );
        let bands = settings.bands as usize;
        Self {
            groups: BandGroups::from_centers(&band_centers(&edges)),
            dynamics: Dynamics::from_smoothing(settings.smoothing),
            scaler: LevelScaler::new(
                settings.auto_gain,
                settings.floor_db as f32,
                settings.ceiling_db as f32,
                settings.sensitivity as f32 / 100.0,
            ),
            bands: vec![BandState::default(); bands],
            settings,
            shared,
            _worker: worker,
            snapshot: Arc::new(Snapshot::empty()),
            worker_status: WorkerStatus::Starting,
            audio: AudioState::Starting,
            last_wall: None,
            snapshot_changed_at: Duration::ZERO,
            last_loud_at: None,
            history: SpectrogramHistory::new(HISTORY_CAPACITY),
            history_accumulator: 0.0,
            levels: EnergyLevels::default(),
            previous_bass: 0.0,
            ripples: Vec::new(),
            last_ripple_at: None,
            wave_peak: 0.3,
            targets: vec![0.0; bands],
            values: vec![0.0; bands],
            peaks: vec![0.0; bands],
            waveform: vec![0.0; WAVEFORM_LEN],
        }
    }

    /// Copy the newest snapshot and worker status without ever blocking.
    fn pull_shared(&mut self, wall: Duration) {
        if let Ok(state) = self.shared.try_lock() {
            if state.snapshot.seq != self.snapshot.seq {
                self.snapshot = Arc::clone(&state.snapshot);
                self.snapshot_changed_at = wall;
            }
            if state.status != self.worker_status {
                self.worker_status = state.status.clone();
            }
        }
    }

    fn update_audio_state(&mut self, wall: Duration) -> bool {
        let fresh =
            self.snapshot.seq > 0 && wall.saturating_sub(self.snapshot_changed_at) <= STALE_AFTER;
        if fresh && self.snapshot.level_db > SILENCE_RMS_DB {
            self.last_loud_at = Some(wall);
        }
        let is_live = fresh
            && self
                .last_loud_at
                .is_some_and(|loud| wall.saturating_sub(loud) <= SILENCE_HOLD);
        self.audio = match (&self.worker_status, is_live) {
            (WorkerStatus::Failed(reason), _) => AudioState::Failed(reason.clone()),
            (_, true) => AudioState::Live,
            (WorkerStatus::Starting, false) if self.snapshot.seq == 0 => AudioState::Starting,
            _ => AudioState::Silent,
        };
        is_live
    }

    /// A slow travelling swell along the baseline: the "nothing is playing" look.
    fn idle_targets(&mut self, seconds: f32) {
        let count = self.targets.len();
        let breath = 0.55 + 0.45 * (seconds * 0.5).sin();
        for (index, target) in self.targets.iter_mut().enumerate() {
            let position = index as f32 / count.max(2).saturating_sub(1).max(1) as f32;
            let wave = 0.5 + 0.5 * (seconds * 0.9 + position * 5.0).sin();
            let ripple = 0.5 + 0.5 * (seconds * 0.55 - position * 9.0).sin();
            let bass_hump = (-(position * 3.2).powi(2)).exp();
            *target =
                (0.035 + breath * (0.05 * wave + 0.03 * ripple + 0.05 * bass_hump)).clamp(0.0, 1.0);
        }
    }

    fn live_targets(&mut self, dt: f32) {
        let bands_db = &self.snapshot.bands_db;
        self.scaler.observe(bands_db, dt);
        for (index, target) in self.targets.iter_mut().enumerate() {
            *target = bands_db
                .get(index)
                .map_or(0.0, |db| self.scaler.normalize(*db));
        }
    }

    fn update_waveform(&mut self, is_live: bool, seconds: f32, dt: f32) {
        if !is_live || self.snapshot.waveform.is_empty() {
            for (index, sample) in self.waveform.iter_mut().enumerate() {
                let phase = index as f32;
                *sample = 0.08 * (phase * 0.045 + seconds * 1.1).sin()
                    + 0.04 * (phase * 0.11 - seconds * 0.7).sin();
            }
            return;
        }
        let peak = self
            .snapshot
            .waveform
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        if peak > self.wave_peak {
            self.wave_peak = peak;
        } else {
            self.wave_peak = (self.wave_peak - 0.5 * dt).max(peak).max(0.02);
        }
        let sensitivity = self.settings.sensitivity as f32 / 100.0;
        let gain = if self.settings.auto_gain {
            0.9 * sensitivity / self.wave_peak.max(0.05)
        } else {
            sensitivity
        };
        self.waveform.clear();
        self.waveform
            .extend(self.snapshot.waveform.iter().map(|sample| sample * gain));
    }

    fn update_rings(&mut self, wall: Duration, dt: f32) {
        let bass = BandGroups::mean(&self.targets, &self.groups.bass);
        let mid = BandGroups::mean(&self.targets, &self.groups.mid);
        let treble = BandGroups::mean(&self.targets, &self.groups.treble);
        // Punchy attack, quick exponential fall.
        let follow = |level: f32, target: f32| {
            if target > level {
                target
            } else {
                (level - dt * 2.2).max(target)
            }
        };
        self.levels = EnergyLevels {
            bass: follow(self.levels.bass, bass),
            mid: follow(self.levels.mid, mid),
            treble: follow(self.levels.treble, treble),
        };
        let cooled_down = self
            .last_ripple_at
            .is_none_or(|last| wall.saturating_sub(last).as_secs_f32() > 0.28);
        if bass > self.previous_bass + 0.16 && bass > 0.35 && cooled_down {
            self.ripples.push(RippleState {
                born: wall,
                strength: bass.min(1.0),
            });
            self.last_ripple_at = Some(wall);
        }
        self.previous_bass = bass;
        self.ripples
            .retain(|ripple| wall.saturating_sub(ripple.born).as_secs_f32() < RIPPLE_SECONDS);
    }
}

impl Scene for SpectrumScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let wall = frame.wall;
        let nominal_dt = 1.0 / self.settings.refresh_rate.max(1) as f32;
        let dt = self
            .last_wall
            .map_or(nominal_dt, |last| wall.saturating_sub(last).as_secs_f32())
            .clamp(0.0, 0.25);
        self.last_wall = Some(wall);
        self.pull_shared(wall);
        let is_live = self.update_audio_state(wall);
        let seconds = frame.time.as_secs_f32();
        if is_live {
            self.live_targets(dt);
        } else {
            self.idle_targets(seconds);
        }
        for ((band, target), (value, peak)) in self
            .bands
            .iter_mut()
            .zip(&self.targets)
            .zip(self.values.iter_mut().zip(self.peaks.iter_mut()))
        {
            band.step(*target, dt, &self.dynamics);
            *value = band.value;
            *peak = band.peak;
        }
        self.update_waveform(is_live, seconds, dt);
        self.update_rings(wall, dt);
        self.history_accumulator += dt * self.settings.spectrogram_speed as f32;
        let mut pushes = 0;
        while self.history_accumulator >= 1.0 && pushes < 64 {
            self.history.push(&self.targets);
            self.history_accumulator -= 1.0;
            pushes += 1;
        }
        self.history_accumulator = self.history_accumulator.min(1.0);
        let ripples: Vec<Ripple> = self
            .ripples
            .iter()
            .map(|ripple| Ripple {
                age: wall.saturating_sub(ripple.born).as_secs_f32() / RIPPLE_SECONDS,
                strength: ripple.strength,
            })
            .collect();
        let input = DrawInput {
            values: &self.values,
            peaks: &self.peaks,
            waveform: &self.waveform,
            history: &self.history,
            levels: self.levels,
            ripples: &ripples,
        };
        draw::draw(frame.raster, &self.settings, &input);
        if self.uses_cell_colors() {
            let cells = usize::from(frame.width) * usize::from(frame.height);
            frame.cell_colors.resize(cells, [255, 255, 255]);
            draw::fill_cell_colors(
                frame.cell_colors,
                frame.width,
                frame.height,
                frame.raster,
                &self.settings,
            );
        }
    }

    fn uses_cell_colors(&self) -> bool {
        self.settings.color_mode != ColorMode::Mono
    }

    fn frames_per_second(&self) -> u32 {
        self.settings.refresh_rate.clamp(10, 30)
    }

    fn status(&self) -> Option<String> {
        match &self.audio {
            AudioState::Live => None,
            AudioState::Starting => Some("Starting audio capture...".to_owned()),
            AudioState::Silent => Some("No audio playing".to_owned()),
            AudioState::Failed(reason) => Some(format!("Audio input unavailable: {reason}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::capture::testing::{MusicSource, SilentSource, SineSource};
    use super::capture::AudioSource;
    use super::*;
    use crate::debug::render_frame;
    use crate::raster::DitherMode;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    fn sine_factory(frequency: f32, amplitude: f32) -> (SourceFactory, Arc<AtomicBool>) {
        let (source, dropped) = SineSource::new(frequency, amplitude);
        let mut source = Some(source);
        let factory: SourceFactory = Box::new(move |_| {
            source
                .take()
                .map(|source| Box::new(source) as Box<dyn AudioSource>)
                .ok_or_else(|| "used twice".to_owned())
        });
        (factory, dropped)
    }

    fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// Wait for a few analyses, then render `frames` frames 40 ms apart.
    fn live_scene(
        settings: &SpectrumSettings,
        frequency: f32,
        amplitude: f32,
    ) -> (SpectrumScene, Arc<AtomicBool>) {
        let (factory, dropped) = sine_factory(frequency, amplitude);
        let scene = SpectrumScene::with_factory(settings, factory);
        let shared = Arc::clone(&scene.shared);
        assert!(wait_until(|| shared
            .lock()
            .map(|state| state.snapshot.seq >= 4)
            .unwrap_or(false)));
        (scene, dropped)
    }

    fn run_frames(
        scene: &mut SpectrumScene,
        width: u16,
        height: u16,
        frames: u32,
    ) -> crate::debug::Rendered {
        let mut last = render_frame(scene, width, height, Duration::ZERO);
        for step in 1..frames {
            last = render_frame(scene, width, height, Duration::from_millis(40) * step);
        }
        last
    }

    #[test]
    fn settings_round_trip_through_serde_with_defaults() {
        let defaults = SpectrumSettings::default();
        let json = serde_json::to_string(&defaults).unwrap();
        let parsed: SpectrumSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, defaults);
        let partial: SpectrumSettings =
            serde_json::from_str(r#"{"style":"radial","bands":64}"#).unwrap();
        assert_eq!(partial.style, SpectrumStyle::Radial);
        assert_eq!(partial.bands, 64);
        assert_eq!(partial.refresh_rate, 24);
        assert_eq!(defaults, defaults.normalized());
    }

    #[test]
    fn normalization_clamps_every_field() {
        let wild = SpectrumSettings {
            device_name: "  x\u{7}y ".to_owned(),
            bands: 1000,
            min_frequency: 5,
            max_frequency: 99_999,
            fft_size: 1234,
            floor_db: -500,
            ceiling_db: -500,
            sensitivity: 0,
            smoothing: 900,
            tilt: 50,
            bar_width: 99,
            bar_gap: 99,
            spectrogram_speed: 0,
            refresh_rate: 999,
            ..SpectrumSettings::default()
        }
        .normalized();
        assert_eq!(wild.device_name, "xy");
        assert_eq!(
            (wild.bands, wild.min_frequency, wild.max_frequency),
            (128, 20, 20_000)
        );
        assert_eq!(wild.fft_size, 2048);
        assert_eq!((wild.floor_db, wild.ceiling_db), (-100, -30));
        assert_eq!((wild.sensitivity, wild.smoothing, wild.tilt), (25, 100, 6));
        assert_eq!(
            (
                wild.bar_width,
                wild.bar_gap,
                wild.spectrogram_speed,
                wild.refresh_rate
            ),
            (8, 4, 10, 30)
        );
        let low = SpectrumSettings {
            bands: 1,
            fft_size: 9999,
            floor_db: -30,
            ceiling_db: -30,
            ..SpectrumSettings::default()
        }
        .normalized();
        assert_eq!((low.bands, low.fft_size, low.ceiling_db), (8, 4096, -20));
    }

    #[test]
    fn controls_have_unique_ids_labels_and_help_and_hide_irrelevant_rows() {
        let mut ids_seen = std::collections::HashSet::new();
        for style in SpectrumStyle::ALL {
            for input in InputKind::ALL {
                let settings = SpectrumSettings {
                    style: *style,
                    input: *input,
                    ..SpectrumSettings::default()
                };
                let rows = settings.controls();
                let ids: Vec<&str> = rows.iter().map(|row| row.id).collect();
                let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
                assert_eq!(ids.len(), unique.len(), "{style:?}: duplicate ids");
                for row in &rows {
                    assert!(
                        !row.label.is_empty() && row.label.len() <= 24,
                        "{}",
                        row.label
                    );
                    assert!(row.help.ends_with('.') && row.help.len() > 10, "{}", row.id);
                    ids_seen.insert(row.id);
                }
                assert_eq!(
                    ids.contains(&"device_name"),
                    *input == InputKind::NamedDevice
                );
                assert_eq!(
                    ids.contains(&"bar_width"),
                    matches!(style, SpectrumStyle::Bars | SpectrumStyle::MirroredBars)
                );
                assert_eq!(
                    ids.contains(&"spectrogram_speed"),
                    *style == SpectrumStyle::Spectrogram
                );
                assert_eq!(
                    ids.contains(&"orientation"),
                    !matches!(style, SpectrumStyle::Radial | SpectrumStyle::PulseRings)
                );
                assert_eq!(ids.contains(&"bands"), *style != SpectrumStyle::Waveform);
            }
        }
        for id in [
            "input",
            "style",
            "color_mode",
            "auto_gain",
            "sensitivity",
            "smoothing",
            "refresh_rate",
            "peak_hold",
            "mirror",
            "tilt",
            "fft_size",
        ] {
            assert!(ids_seen.contains(id), "control {id} missing");
        }
    }

    #[test]
    fn every_control_round_trips_through_set_control() {
        let mut settings = SpectrumSettings {
            input: InputKind::NamedDevice,
            style: SpectrumStyle::Bars,
            ..SpectrumSettings::default()
        };
        for row in settings.controls() {
            // Step the row once each way and confirm the displayed value follows.
            let Some(next) = row.stepped(1) else {
                assert_eq!(row.id, "device_name");
                assert_eq!(
                    settings.set_control(
                        "device_name",
                        ControlValue::Text("BlackHole 2ch".to_owned())
                    ),
                    Ok(true)
                );
                assert_eq!(
                    settings.set_control(
                        "device_name",
                        ControlValue::Text("BlackHole 2ch".to_owned())
                    ),
                    Ok(false)
                );
                continue;
            };
            let changed = settings.set_control(row.id, next.clone()).unwrap();
            let after = settings
                .controls()
                .into_iter()
                .find(|candidate| candidate.id == row.id)
                .unwrap();
            assert!(
                changed || after.value == row.value,
                "{}: step had no effect but value moved",
                row.id
            );
            if changed {
                assert_ne!(
                    after.value, row.value,
                    "{} value unchanged after change",
                    row.id
                );
            }
        }
        assert_eq!(settings, settings.normalized());
    }

    #[test]
    fn set_control_rejects_unknown_ids_wrong_types_and_bad_names() {
        let mut settings = SpectrumSettings::default();
        assert_eq!(
            settings.set_control("nope", ControlValue::Number(1)),
            Ok(false)
        );
        assert_eq!(
            settings.set_control("bands", ControlValue::Bool(true)),
            Ok(false)
        );
        assert_eq!(
            settings.set_control("style", ControlValue::Index(999)),
            Ok(false)
        );
        assert!(settings
            .set_control("device_name", ControlValue::Text("a".repeat(500)))
            .is_err());
        assert!(settings
            .set_control("device_name", ControlValue::Text("bad\nname".to_owned()))
            .is_err());
        assert_eq!(settings, SpectrumSettings::default());
        // Out-of-range numbers are clamped, and report a change only when it happened.
        assert_eq!(
            settings.set_control("bands", ControlValue::Number(10_000)),
            Ok(true)
        );
        assert_eq!(settings.bands, 128);
        assert_eq!(
            settings.set_control("bands", ControlValue::Number(128)),
            Ok(false)
        );
        assert_eq!(
            settings.set_control("ceiling_db", ControlValue::Number(-60)),
            Ok(true)
        );
        assert!(settings.ceiling_db >= settings.floor_db + 10);
    }

    #[test]
    fn scene_key_changes_when_any_visual_setting_changes() {
        use crate::registry::{AmbientKind, AmbientSettings};
        let base = AmbientSettings::default();
        let mut changed = base.clone();
        changed.spectrum.style = SpectrumStyle::Radial;
        assert_ne!(
            base.scene_key(AmbientKind::Spectrum),
            changed.scene_key(AmbientKind::Spectrum)
        );
    }

    #[test]
    fn groups_split_bands_by_frequency() {
        let edges = band_edges(BandScale::Log, 48, 40.0, 16_000.0);
        let groups = BandGroups::from_centers(&band_centers(&edges));
        assert!(!groups.bass.is_empty() && !groups.mid.is_empty() && !groups.treble.is_empty());
        assert_eq!(groups.bass.end, groups.mid.start);
        assert_eq!(groups.mid.end, groups.treble.start);
        assert_eq!(groups.treble.end, 48);
        let tiny = BandGroups::from_centers(&[1000.0]);
        assert_eq!(tiny.bass, 0..1);
    }

    #[test]
    fn live_sine_draws_a_bar_at_the_right_place() {
        let settings = SpectrumSettings {
            style: SpectrumStyle::Bars,
            bands: 32,
            bar_gap: 0,
            peak_hold: false,
            smoothing: 20,
            ..SpectrumSettings::default()
        };
        let (mut scene, _dropped) = live_scene(&settings, 1000.0, 0.5);
        let rendered = run_frames(&mut scene, 64, 16, 12);
        assert!(
            scene.status().is_none(),
            "live audio has no status: {:?}",
            scene.status()
        );
        // 1 kHz on 32 log bands from 40 Hz to 16 kHz sits around 60% of the width.
        let edges = band_edges(BandScale::Log, 32, 40.0, 16_000.0);
        let expected = edges.iter().position(|edge| *edge > 1000.0).unwrap() - 1;
        let dots_wide = 64 * 2;
        let bar_width = dots_wide / 32;
        let centre_x = expected * bar_width + bar_width / 2;
        let height_at = |x: usize| {
            (0..64)
                .filter(|y| rendered.raster.dots[y * dots_wide + x] > 0.3)
                .count()
        };
        assert!(
            height_at(centre_x) >= 50,
            "tallest at the 1 kHz band: {}",
            height_at(centre_x)
        );
        assert!(
            height_at(centre_x) > 3 * height_at(3 * bar_width),
            "far bass bar is much shorter"
        );
        assert!(rendered.lit_dots() > 100);
    }

    #[test]
    fn frames_differ_over_time_and_are_deterministic_for_equal_inputs() {
        let settings = SpectrumSettings {
            style: SpectrumStyle::Waveform,
            ..SpectrumSettings::default()
        };
        let (factory, _dropped) = sine_factory(440.0, 0.4);
        // A failing source keeps the scene in its deterministic idle animation.
        drop(factory);
        let failing = |settings: &SpectrumSettings| {
            let factory: SourceFactory = Box::new(|_| Err("no device".to_owned()));
            SpectrumScene::with_factory(settings, factory)
        };
        let mut first = failing(&settings);
        let mut second = failing(&settings);
        let at_zero = render_frame(&mut first, 40, 10, Duration::from_secs(1));
        let repeat = render_frame(&mut second, 40, 10, Duration::from_secs(1));
        assert_eq!(
            at_zero.raster.dots, repeat.raster.dots,
            "same time, same idle picture"
        );
        let later = render_frame(&mut first, 40, 10, Duration::from_secs(4));
        assert_ne!(
            at_zero.raster.dots, later.raster.dots,
            "idle animation moves"
        );
    }

    #[test]
    fn idle_animation_is_gentle_but_visible_in_every_style() {
        for style in SpectrumStyle::ALL {
            let settings = SpectrumSettings {
                style: *style,
                ..SpectrumSettings::default()
            };
            let factory: SourceFactory = Box::new(|_| Err("no device".to_owned()));
            let mut scene = SpectrumScene::with_factory(&settings, factory);
            let rendered = run_frames(&mut scene, 60, 14, 60);
            let dots = rendered.raster.dots.len();
            let visible = rendered
                .raster
                .dots
                .iter()
                .filter(|dot| **dot > 0.08)
                .count();
            assert!(visible > 15, "{style:?} idle shows something ({visible})");
            assert!(
                rendered.lit_dots() < dots / 3,
                "{style:?} idle stays calm ({} of {dots})",
                rendered.lit_dots()
            );
        }
    }

    #[test]
    fn status_reports_failure_silence_and_starting() {
        let failing: SourceFactory =
            Box::new(|_| Err("cannot start pw-record: not found".to_owned()));
        let mut scene = SpectrumScene::with_factory(&SpectrumSettings::default(), failing);
        assert!(wait_until(|| {
            render_frame(&mut scene, 20, 6, Duration::ZERO);
            scene
                .status()
                .is_some_and(|status| status.starts_with("Audio input unavailable:"))
        }));
        assert_eq!(
            scene.status().as_deref(),
            Some("Audio input unavailable: cannot start pw-record: not found")
        );
        // A running source that delivers nothing is "No audio playing".
        let dropped = Arc::new(AtomicBool::new(false));
        let mut source = Some(SilentSource { dropped });
        let silent: SourceFactory = Box::new(move |_| {
            source
                .take()
                .map(|source| Box::new(source) as Box<dyn AudioSource>)
                .ok_or_else(|| "again".to_owned())
        });
        let mut scene = SpectrumScene::with_factory(&SpectrumSettings::default(), silent);
        render_frame(&mut scene, 20, 6, Duration::ZERO);
        assert_eq!(scene.status().as_deref(), Some("Starting audio capture..."));
        // With a working (but mute) source and time passing, it becomes "No audio playing".
        scene.shared.lock().unwrap().status = WorkerStatus::Running;
        render_frame(&mut scene, 20, 6, Duration::from_secs(3));
        assert_eq!(scene.status().as_deref(), Some("No audio playing"));
    }

    #[test]
    fn quiet_input_counts_as_silence_and_loud_input_wakes_the_scene() {
        let (mut scene, _dropped) = live_scene(&SpectrumSettings::default(), 1000.0, 0.0002);
        run_frames(&mut scene, 30, 8, 5);
        assert_eq!(
            scene.status().as_deref(),
            Some("No audio playing"),
            "-74 dBFS is silence"
        );
        let (mut scene, _dropped) = live_scene(&SpectrumSettings::default(), 1000.0, 0.3);
        run_frames(&mut scene, 30, 8, 5);
        assert_eq!(scene.status(), None);
    }

    #[test]
    fn stalled_capture_falls_back_to_idle_after_the_stale_period() {
        let (mut scene, _dropped) = live_scene(&SpectrumSettings::default(), 1000.0, 0.4);
        run_frames(&mut scene, 30, 8, 3);
        assert_eq!(scene.status(), None);
        // Freeze the published data by making the scene ignore new snapshots:
        // render far into the future with the same snapshot sequence.
        let frozen = Arc::clone(&scene.snapshot);
        scene.shared = new_shared();
        scene.shared.lock().unwrap().snapshot = Arc::clone(&frozen);
        scene.shared.lock().unwrap().status = WorkerStatus::Running;
        scene.snapshot = frozen;
        render_frame(&mut scene, 30, 8, Duration::from_secs(10));
        render_frame(&mut scene, 30, 8, Duration::from_secs(12));
        assert_eq!(scene.status().as_deref(), Some("No audio playing"));
    }

    #[test]
    fn every_style_renders_a_live_signal_with_distinct_output() {
        let mut outputs = Vec::new();
        for style in SpectrumStyle::ALL {
            let settings = SpectrumSettings {
                style: *style,
                ..SpectrumSettings::default()
            };
            let (mut scene, _dropped) = live_scene(&settings, 700.0, 0.4);
            let rendered = run_frames(&mut scene, 60, 16, 20);
            assert!(
                rendered.lit_dots() > 60,
                "{style:?}: {} lit",
                rendered.lit_dots()
            );
            outputs.push((
                *style,
                rendered.braille_lines(70, DitherMode::Ordered).join("\n"),
            ));
        }
        for (index, (first, a)) in outputs.iter().enumerate() {
            for (second, b) in &outputs[index + 1..] {
                assert_ne!(a, b, "{first:?} and {second:?} render identically");
            }
        }
    }

    #[test]
    fn colour_modes_fill_cell_colours_for_every_cell() {
        for mode in [ColorMode::Rainbow, ColorMode::Heat, ColorMode::Ice] {
            let settings = SpectrumSettings {
                color_mode: mode,
                ..SpectrumSettings::default()
            };
            let (mut scene, _dropped) = live_scene(&settings, 500.0, 0.4);
            assert!(scene.uses_cell_colors());
            let rendered = run_frames(&mut scene, 40, 10, 6);
            assert_eq!(rendered.cell_colors.len(), 400);
            let distinct: std::collections::HashSet<[u8; 3]> =
                rendered.cell_colors.iter().copied().collect();
            assert!(
                distinct.len() > 5,
                "{mode:?} produces a gradient, got {}",
                distinct.len()
            );
        }
        assert!(!SpectrumScene::with_factory(
            &SpectrumSettings::default(),
            Box::new(|_| Err("x".to_owned()))
        )
        .uses_cell_colors());
    }

    #[test]
    fn frames_per_second_follows_the_setting() {
        for rate in [10, 24, 30] {
            let settings = SpectrumSettings {
                refresh_rate: rate,
                ..SpectrumSettings::default()
            };
            let scene = SpectrumScene::with_factory(&settings, Box::new(|_| Err("x".to_owned())));
            assert_eq!(scene.frames_per_second(), rate);
        }
    }

    #[test]
    fn dropping_the_scene_stops_capture_promptly() {
        let (scene, dropped) = live_scene(&SpectrumSettings::default(), 1000.0, 0.4);
        let started = Instant::now();
        drop(scene);
        assert!(
            dropped.load(Ordering::SeqCst),
            "source released by the worker"
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn render_never_blocks_while_the_worker_holds_the_lock() {
        let (mut scene, _dropped) = live_scene(&SpectrumSettings::default(), 1000.0, 0.4);
        let shared = Arc::clone(&scene.shared);
        let guard = shared.lock().unwrap();
        let started = Instant::now();
        let rendered = render_frame(&mut scene, 30, 8, Duration::from_secs(1));
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(rendered.raster.dots.len() == 60 * 32);
        drop(guard);
    }

    #[test]
    fn scenes_render_at_odd_and_tiny_sizes() {
        for style in SpectrumStyle::ALL {
            for (width, height) in [(1, 1), (2, 1), (7, 3), (200, 3), (3, 60)] {
                let settings = SpectrumSettings {
                    style: *style,
                    color_mode: ColorMode::Rainbow,
                    ..SpectrumSettings::default()
                };
                let mut scene =
                    SpectrumScene::with_factory(&settings, Box::new(|_| Err("x".to_owned())));
                let rendered = render_frame(&mut scene, width, height, Duration::from_secs(2));
                assert_eq!(
                    rendered.cell_colors.len(),
                    usize::from(width) * usize::from(height)
                );
            }
        }
    }

    fn music_scene(settings: &SpectrumSettings) -> SpectrumScene {
        let mut source = Some(MusicSource::new());
        let factory: SourceFactory = Box::new(move |_| {
            source
                .take()
                .map(|source| Box::new(source) as Box<dyn AudioSource>)
                .ok_or_else(|| "again".to_owned())
        });
        let scene = SpectrumScene::with_factory(settings, factory);
        let shared = Arc::clone(&scene.shared);
        assert!(wait_until(|| shared
            .lock()
            .map(|state| state.snapshot.seq >= 4)
            .unwrap_or(false)));
        scene
    }

    /// Prints a live-signal picture per style for visual review:
    /// `cargo test -p ilium-ambient show_scene_pictures -- --ignored --nocapture`.
    #[test]
    #[ignore = "prints Braille for visual inspection"]
    fn show_scene_pictures() {
        for style in SpectrumStyle::ALL {
            let settings = SpectrumSettings {
                style: *style,
                ..SpectrumSettings::default()
            };
            let mut scene = music_scene(&settings);
            let rendered = run_frames(&mut scene, 60, 14, 80);
            println!("--- {style:?} (synthetic music)");
            for line in rendered.braille_lines(60, DitherMode::Ordered) {
                println!("{line}");
            }
        }
    }
}
