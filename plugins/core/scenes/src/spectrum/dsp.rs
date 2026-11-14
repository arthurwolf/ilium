//! Signal processing for the spectrum scene: radix-2 FFT, Hann window,
//! log/mel/linear band layout, dB scaling with auto-gain, and the
//! attack/gravity/peak-hold state machines. Pure functions and small state
//! structs, no I/O and no clock (callers pass `dt`).

use super::BandScale;
use std::f64::consts::PI;

/// Lowest dB value ever produced (digital silence).
pub const DB_FLOOR: f32 = -120.0;

/// Below this RMS level (dBFS) the input counts as silence.
pub const SILENCE_RMS_DB: f32 = -68.0;

/// Amplitude (1.0 = full scale) to decibels, clamped at `DB_FLOOR`.
pub fn amplitude_to_db(amplitude: f32) -> f32 {
    (20.0 * amplitude.max(1e-9).log10()).max(DB_FLOOR)
}

/// In-place iterative radix-2 Cooley-Tukey FFT with precomputed twiddles and
/// bit-reversal table.
pub struct Fft {
    size: usize,
    bit_reverse: Vec<u32>,
    twiddle_cos: Vec<f32>,
    twiddle_sin: Vec<f32>,
}

impl Fft {
    /// `size` must be a power of two and at least 2.
    pub fn new(size: usize) -> Result<Self, String> {
        if size < 2 || !size.is_power_of_two() {
            return Err(format!("FFT size must be a power of two, got {size}"));
        }
        let bits = size.trailing_zeros();
        let bit_reverse = (0..size as u32)
            .map(|index| index.reverse_bits() >> (32 - bits))
            .collect();
        let half = size / 2;
        // Twiddles are computed in f64 so 4096-point transforms keep ~1e-6 accuracy.
        let (twiddle_cos, twiddle_sin) = (0..half)
            .map(|k| {
                let angle = -2.0 * PI * k as f64 / size as f64;
                (angle.cos() as f32, angle.sin() as f32)
            })
            .unzip();
        Ok(Self {
            size,
            bit_reverse,
            twiddle_cos,
            twiddle_sin,
        })
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// Forward transform of `real + i*imag` in place. A slice whose length is
    /// not `size()` is left untouched (callers own the sizing).
    pub fn forward(&self, real: &mut [f32], imag: &mut [f32]) {
        if real.len() != self.size || imag.len() != self.size {
            return;
        }
        for (index, &target) in self.bit_reverse.iter().enumerate() {
            let target = target as usize;
            if target > index {
                real.swap(index, target);
                imag.swap(index, target);
            }
        }
        let mut length = 2;
        while length <= self.size {
            let half = length / 2;
            let step = self.size / length;
            for start in (0..self.size).step_by(length) {
                for offset in 0..half {
                    let cos = self.twiddle_cos[offset * step];
                    let sin = self.twiddle_sin[offset * step];
                    let (low, high) = (start + offset, start + offset + half);
                    let temp_real = real[high] * cos - imag[high] * sin;
                    let temp_imag = real[high] * sin + imag[high] * cos;
                    real[high] = real[low] - temp_real;
                    imag[high] = imag[low] - temp_imag;
                    real[low] += temp_real;
                    imag[low] += temp_imag;
                }
            }
            length *= 2;
        }
    }
}

/// Periodic Hann window of `size` samples (correct for spectral analysis).
pub fn hann_window(size: usize) -> Vec<f32> {
    (0..size)
        .map(|index| (0.5 - 0.5 * (2.0 * PI * index as f64 / size as f64).cos()) as f32)
        .collect()
}

fn hz_to_mel(hz: f64) -> f64 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

fn mel_to_hz(mel: f64) -> f64 {
    700.0 * (10f64.powf(mel / 2595.0) - 1.0)
}

/// `bands + 1` ascending edge frequencies between `min_hz` and `max_hz`.
pub fn band_edges(scale: BandScale, bands: usize, min_hz: f32, max_hz: f32) -> Vec<f32> {
    let bands = bands.max(1);
    let low = f64::from(min_hz.max(1.0));
    let high = f64::from(max_hz).max(low * 1.01);
    (0..=bands)
        .map(|index| {
            let fraction = index as f64 / bands as f64;
            let hz = match scale {
                BandScale::Log => low * (high / low).powf(fraction),
                BandScale::Mel => {
                    let (mel_low, mel_high) = (hz_to_mel(low), hz_to_mel(high));
                    mel_to_hz(mel_low + (mel_high - mel_low) * fraction)
                }
                BandScale::Linear => low + (high - low) * fraction,
            };
            hz as f32
        })
        .collect()
}

/// Geometric centre of each band, in Hz.
pub fn band_centers(edges: &[f32]) -> Vec<f32> {
    edges
        .windows(2)
        .map(|pair| (pair[0] * pair[1]).max(0.0).sqrt())
        .collect()
}

/// How one band reads the FFT magnitude spectrum: the loudest bin inside the
/// band or, for bands narrower than a bin, a linear interpolation at the
/// band centre.
#[derive(Debug, Clone, Copy, PartialEq)]
enum BandSpan {
    Bins { low: usize, high: usize },
    Between { bin: usize, fraction: f32 },
}

/// Result of analysing one window of audio.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// Per-band level in dBFS (0 dB = a full-scale sine) after tilt.
    pub bands_db: Vec<f32>,
    /// RMS level of the window in dBFS.
    pub level_db: f32,
}

/// Windowed FFT front end producing one dB value per band.
pub struct Analyzer {
    fft: Fft,
    window: Vec<f32>,
    real: Vec<f32>,
    imag: Vec<f32>,
    magnitude: Vec<f32>,
    spans: Vec<BandSpan>,
    tilt_db: Vec<f32>,
}

impl Analyzer {
    pub fn new(
        fft_size: usize,
        sample_rate: u32,
        edges: &[f32],
        tilt_db_per_octave: f32,
    ) -> Result<Self, String> {
        let fft = Fft::new(fft_size)?;
        let bin_hz = sample_rate.max(1) as f32 / fft_size as f32;
        let nyquist = sample_rate as f32 / 2.0;
        let highest_bin = fft_size / 2;
        let spans = edges
            .windows(2)
            .map(|pair| {
                let low_hz = pair[0].min(nyquist * 0.98);
                let high_hz = pair[1].min(nyquist * 0.99).max(low_hz);
                let low = (low_hz / bin_hz).ceil() as usize;
                let high = ((high_hz / bin_hz).floor() as usize).min(highest_bin);
                if low <= high {
                    BandSpan::Bins {
                        low: low.max(1),
                        high: high.max(1),
                    }
                } else {
                    let centre = (low_hz * high_hz).max(0.0).sqrt() / bin_hz;
                    let bin = (centre.floor() as usize).clamp(1, highest_bin - 1);
                    BandSpan::Between {
                        bin,
                        fraction: (centre - bin as f32).clamp(0.0, 1.0),
                    }
                }
            })
            .collect();
        let tilt_db = band_centers(edges)
            .into_iter()
            .map(|centre| tilt_db_per_octave * (centre.max(1.0) / 1000.0).log2())
            .collect();
        Ok(Self {
            window: hann_window(fft_size),
            real: vec![0.0; fft_size],
            imag: vec![0.0; fft_size],
            magnitude: vec![0.0; fft_size / 2 + 1],
            fft,
            spans,
            tilt_db,
        })
    }

    /// Analyse the newest `fft_size` samples of `samples` (zero-padded at the
    /// front when shorter).
    pub fn analyze(&mut self, samples: &[f32]) -> Analysis {
        let size = self.fft.size();
        let tail = &samples[samples.len().saturating_sub(size)..];
        let padding = size - tail.len();
        self.real[..padding].fill(0.0);
        self.imag.fill(0.0);
        let mut energy = 0.0f64;
        for (index, &sample) in tail.iter().enumerate() {
            energy += f64::from(sample) * f64::from(sample);
            self.real[padding + index] = sample * self.window[padding + index];
        }
        self.fft.forward(&mut self.real, &mut self.imag);
        // Hann coherent gain is 0.5, so a full-scale sine reads 4 / N * (N / 4) = 1.
        let scale = 4.0 / size as f32;
        for (bin, magnitude) in self.magnitude.iter_mut().enumerate() {
            *magnitude = self.real[bin].hypot(self.imag[bin]) * scale;
        }
        let bands_db = self
            .spans
            .iter()
            .zip(&self.tilt_db)
            .map(|(span, tilt)| {
                let amplitude = match *span {
                    BandSpan::Bins { low, high } => self.magnitude[low..=high]
                        .iter()
                        .copied()
                        .fold(0.0f32, f32::max),
                    BandSpan::Between { bin, fraction } => {
                        self.magnitude[bin] * (1.0 - fraction) + self.magnitude[bin + 1] * fraction
                    }
                };
                (amplitude_to_db(amplitude) + tilt).max(DB_FLOOR)
            })
            .collect();
        let level_db = amplitude_to_db((energy / tail.len().max(1) as f64).sqrt() as f32);
        Analysis { bands_db, level_db }
    }
}

/// Maps band dB values onto 0..1 heights. Either a fixed floor/ceiling window
/// or one whose top follows the loudest recent band (auto-gain).
#[derive(Debug, Clone)]
pub struct LevelScaler {
    pub auto_gain: bool,
    pub floor_db: f32,
    pub ceiling_db: f32,
    /// Output multiplier (1.0 = neutral).
    pub sensitivity: f32,
    tracked_peak_db: f32,
}

impl LevelScaler {
    /// Auto-gain never lifts the window top above this level of pure noise.
    const AUTO_TOP_MINIMUM_DB: f32 = -52.0;
    /// How fast the auto-gain window top falls after the music got quieter.
    const AUTO_RELEASE_DB_PER_SECOND: f32 = 3.0;

    pub fn new(auto_gain: bool, floor_db: f32, ceiling_db: f32, sensitivity: f32) -> Self {
        Self {
            auto_gain,
            floor_db,
            ceiling_db,
            sensitivity,
            tracked_peak_db: Self::AUTO_TOP_MINIMUM_DB,
        }
    }

    /// Feed the newest band levels. Only used by auto-gain; `dt` in seconds.
    pub fn observe(&mut self, bands_db: &[f32], dt: f32) {
        if !self.auto_gain {
            return;
        }
        let loudest = bands_db.iter().copied().fold(DB_FLOOR, f32::max);
        if loudest > self.tracked_peak_db {
            // Fast attack so a sudden loud passage never clips for long.
            let blend = 1.0 - (-dt / 0.05).exp();
            self.tracked_peak_db += (loudest - self.tracked_peak_db) * blend;
        } else {
            self.tracked_peak_db = (self.tracked_peak_db - Self::AUTO_RELEASE_DB_PER_SECOND * dt)
                .max(Self::AUTO_TOP_MINIMUM_DB);
        }
    }

    /// Top of the visible window in dB.
    pub fn window_top_db(&self) -> f32 {
        if self.auto_gain {
            (self.tracked_peak_db + 1.0).clamp(Self::AUTO_TOP_MINIMUM_DB, 6.0)
        } else {
            self.ceiling_db
        }
    }

    pub fn span_db(&self) -> f32 {
        (self.ceiling_db - self.floor_db).max(10.0)
    }

    /// 0..1 height for one dB value.
    pub fn normalize(&self, db: f32) -> f32 {
        let bottom = self.window_top_db() - self.span_db();
        (((db - bottom) / self.span_db()) * self.sensitivity).clamp(0.0, 1.0)
    }
}

/// Timing constants derived from the user's "smoothing" percentage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dynamics {
    /// Time constant of the exponential rise, seconds.
    pub attack_tau: f32,
    /// Gravity in heights per second squared for the bar fall-off.
    pub gravity: f32,
    /// Seconds a peak marker rests before falling.
    pub peak_hold: f32,
    /// Gravity of a falling peak marker.
    pub peak_gravity: f32,
}

impl Dynamics {
    pub fn from_smoothing(percent: u32) -> Self {
        let amount = percent.min(100) as f32 / 100.0;
        // Time for a full-height bar to fall to zero under constant gravity.
        let fall_seconds = 0.10 + 0.90 * amount;
        Self {
            attack_tau: 0.004 + 0.05 * amount,
            gravity: 2.0 / (fall_seconds * fall_seconds),
            peak_hold: 0.35 + 0.35 * amount,
            peak_gravity: 2.5,
        }
    }
}

/// One band's displayed value and peak marker.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BandState {
    pub value: f32,
    pub peak: f32,
    fall_speed: f32,
    peak_rest: f32,
    peak_speed: f32,
}

impl BandState {
    /// Advance by `dt` seconds toward `target` (0..1).
    pub fn step(&mut self, target: f32, dt: f32, dynamics: &Dynamics) {
        let dt = dt.clamp(0.0, 0.25);
        if target >= self.value {
            let blend = 1.0 - (-dt / dynamics.attack_tau.max(1e-4)).exp();
            self.value += (target - self.value) * blend;
            self.fall_speed = 0.0;
        } else {
            self.fall_speed += dynamics.gravity * dt;
            self.value = (self.value - self.fall_speed * dt).max(target);
        }
        self.value = self.value.clamp(0.0, 1.0);
        if self.value >= self.peak {
            self.peak = self.value;
            self.peak_rest = dynamics.peak_hold;
            self.peak_speed = 0.0;
        } else if self.peak_rest > 0.0 {
            self.peak_rest -= dt;
        } else {
            self.peak_speed += dynamics.peak_gravity * dt;
            self.peak = (self.peak - self.peak_speed * dt).max(self.value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive_dft(input: &[f32]) -> Vec<(f64, f64)> {
        let size = input.len();
        (0..size)
            .map(|k| {
                let mut real = 0.0;
                let mut imag = 0.0;
                for (n, &sample) in input.iter().enumerate() {
                    let angle = -2.0 * PI * (k * n) as f64 / size as f64;
                    real += f64::from(sample) * angle.cos();
                    imag += f64::from(sample) * angle.sin();
                }
                (real, imag)
            })
            .collect()
    }

    fn pseudo_random(count: usize) -> Vec<f32> {
        let mut state = 0x1234_5678u32;
        (0..count)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn fft_matches_naive_dft_for_several_sizes() {
        for size in [2usize, 8, 64, 256] {
            let input = pseudo_random(size);
            let expected = naive_dft(&input);
            let fft = Fft::new(size).unwrap();
            let mut real = input.clone();
            let mut imag = vec![0.0; size];
            fft.forward(&mut real, &mut imag);
            for k in 0..size {
                let tolerance = 1e-3 * (size as f64).sqrt();
                assert!(
                    (f64::from(real[k]) - expected[k].0).abs() < tolerance
                        && (f64::from(imag[k]) - expected[k].1).abs() < tolerance,
                    "size {size} bin {k}: fft ({}, {}) vs dft ({}, {})",
                    real[k],
                    imag[k],
                    expected[k].0,
                    expected[k].1
                );
            }
        }
    }

    #[test]
    fn fft_rejects_non_power_of_two_and_ignores_wrong_slices() {
        assert!(Fft::new(0).is_err());
        assert!(Fft::new(1000).is_err());
        let fft = Fft::new(8).unwrap();
        let mut real = vec![1.0; 4];
        let mut imag = vec![0.0; 4];
        fft.forward(&mut real, &mut imag);
        assert_eq!(real, vec![1.0; 4]);
    }

    #[test]
    fn hann_window_is_periodic_and_peaks_in_the_middle() {
        let window = hann_window(8);
        assert!(window[0].abs() < 1e-7);
        assert!((window[4] - 1.0).abs() < 1e-6);
        assert!((window[2] - window[6]).abs() < 1e-6);
        let mean: f32 = window.iter().sum::<f32>() / 8.0;
        assert!((mean - 0.5).abs() < 1e-6, "coherent gain 0.5, got {mean}");
    }

    #[test]
    fn band_edges_are_monotonic_and_span_the_requested_range() {
        for scale in [BandScale::Log, BandScale::Mel, BandScale::Linear] {
            let edges = band_edges(scale, 32, 40.0, 16_000.0);
            assert_eq!(edges.len(), 33);
            assert!((edges[0] - 40.0).abs() < 0.01);
            assert!((edges[32] - 16_000.0).abs() < 1.0);
            assert!(edges.windows(2).all(|pair| pair[1] > pair[0]), "{scale:?}");
        }
        let log = band_edges(BandScale::Log, 4, 100.0, 1600.0);
        assert!((log[1] - 200.0).abs() < 0.5 && (log[2] - 400.0).abs() < 1.0);
        let linear = band_edges(BandScale::Linear, 4, 0.0, 400.0);
        assert!((linear[2] - 200.5).abs() < 1.0);
        // Mel sits between linear and log: its first band is wider than log's.
        let mel = band_edges(BandScale::Mel, 32, 40.0, 16_000.0);
        let log = band_edges(BandScale::Log, 32, 40.0, 16_000.0);
        assert!(mel[1] - mel[0] > log[1] - log[0]);
    }

    fn sine(frequency: f32, rate: u32, count: usize, amplitude: f32) -> Vec<f32> {
        (0..count)
            .map(|n| {
                amplitude
                    * (2.0 * PI * f64::from(frequency) * n as f64 / f64::from(rate)).sin() as f32
            })
            .collect()
    }

    #[test]
    fn sine_peaks_in_the_band_that_contains_its_frequency() {
        let rate = 44_100;
        for scale in [BandScale::Log, BandScale::Mel, BandScale::Linear] {
            let edges = band_edges(scale, 48, 40.0, 16_000.0);
            for size in [2048usize, 4096] {
                let mut analyzer = Analyzer::new(size, rate, &edges, 0.0).unwrap();
                for frequency in [110.0f32, 440.0, 1000.0, 3520.0, 9000.0, 14_000.0] {
                    let analysis = analyzer.analyze(&sine(frequency, rate, size, 1.0));
                    let loudest = analysis
                        .bands_db
                        .iter()
                        .enumerate()
                        .max_by(|a, b| a.1.total_cmp(b.1))
                        .map(|(index, _)| index)
                        .unwrap();
                    // Low bands can be narrower than an FFT bin: allow one bin of slack.
                    let slack = 1.0 + f64::from(rate) as f32 / size as f32;
                    assert!(
                        edges[loudest] - slack <= frequency
                            && frequency <= edges[loudest + 1] + slack,
                        "{scale:?} {size}: {frequency} Hz landed in band {loudest} \
                         [{}, {}]",
                        edges[loudest],
                        edges[loudest + 1]
                    );
                    // Full-scale sine reads within scalloping loss of 0 dBFS.
                    assert!(
                        analysis.bands_db[loudest] > -3.5 && analysis.bands_db[loudest] < 0.5,
                        "{scale:?} {size} {frequency}: {} dB",
                        analysis.bands_db[loudest]
                    );
                }
            }
        }
    }

    #[test]
    fn amplitude_scales_in_decibels() {
        let rate = 44_100;
        let edges = band_edges(BandScale::Log, 32, 40.0, 16_000.0);
        let mut analyzer = Analyzer::new(2048, rate, &edges, 0.0).unwrap();
        let loud = analyzer.analyze(&sine(1000.0, rate, 2048, 0.5));
        let quiet = analyzer.analyze(&sine(1000.0, rate, 2048, 0.05));
        let peak = |analysis: &Analysis| analysis.bands_db.iter().copied().fold(-200.0, f32::max);
        assert!((peak(&loud) - peak(&quiet) - 20.0).abs() < 0.5);
        assert!(
            (loud.level_db - (-9.03)).abs() < 0.3,
            "rms {}",
            loud.level_db
        );
    }

    #[test]
    fn silence_reads_as_the_floor_and_short_input_is_padded() {
        let edges = band_edges(BandScale::Log, 16, 40.0, 16_000.0);
        let mut analyzer = Analyzer::new(2048, 44_100, &edges, 0.0).unwrap();
        let analysis = analyzer.analyze(&vec![0.0; 2048]);
        assert!(analysis.bands_db.iter().all(|db| *db <= DB_FLOOR + 0.01));
        assert!(analysis.level_db < SILENCE_RMS_DB);
        let short = analyzer.analyze(&sine(1000.0, 44_100, 500, 1.0));
        assert_eq!(short.bands_db.len(), 16);
    }

    #[test]
    fn tilt_lifts_treble_relative_to_bass() {
        let edges = band_edges(BandScale::Log, 32, 40.0, 16_000.0);
        let mut flat = Analyzer::new(2048, 44_100, &edges, 0.0).unwrap();
        let mut tilted = Analyzer::new(2048, 44_100, &edges, 3.0).unwrap();
        let bass = sine(100.0, 44_100, 2048, 0.5);
        let treble = sine(8000.0, 44_100, 2048, 0.5);
        let peak = |analysis: Analysis| analysis.bands_db.into_iter().fold(-200.0, f32::max);
        let flat_gap = peak(flat.analyze(&treble)) - peak(flat.analyze(&bass));
        let tilted_gap = peak(tilted.analyze(&treble)) - peak(tilted.analyze(&bass));
        // 100 Hz to 8 kHz is about 6.3 octaves: 3 dB/octave is ~19 dB.
        assert!(
            (tilted_gap - flat_gap - 19.0).abs() < 2.5,
            "{flat_gap} {tilted_gap}"
        );
    }

    #[test]
    fn fixed_scaler_maps_floor_and_ceiling_to_zero_and_one() {
        let scaler = LevelScaler::new(false, -70.0, -10.0, 1.0);
        assert_eq!(scaler.normalize(-70.0), 0.0);
        assert!((scaler.normalize(-40.0) - 0.5).abs() < 1e-6);
        assert_eq!(scaler.normalize(0.0), 1.0);
        let sensitive = LevelScaler::new(false, -70.0, -10.0, 2.0);
        assert!((sensitive.normalize(-55.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn auto_gain_follows_loudness_and_releases_slowly() {
        let mut scaler = LevelScaler::new(true, -70.0, -10.0, 1.0);
        let quiet = [-45.0f32, -60.0];
        for _ in 0..40 {
            scaler.observe(&quiet, 0.05);
        }
        // The loudest band now reaches almost the top even though it is quiet.
        assert!(scaler.normalize(-45.0) > 0.9);
        let before = scaler.window_top_db();
        scaler.observe(&[-90.0], 1.0);
        let after = scaler.window_top_db();
        assert!((before - after - 3.0).abs() < 0.2, "{before} -> {after}");
        for _ in 0..200 {
            scaler.observe(&[-120.0], 1.0);
        }
        assert!(
            scaler.window_top_db() >= -52.0 + 1.0 - 1e-3,
            "noise floor guard"
        );
    }

    #[test]
    fn dynamics_scale_with_the_smoothing_percentage() {
        let snappy = Dynamics::from_smoothing(0);
        let smooth = Dynamics::from_smoothing(100);
        assert!(snappy.attack_tau < smooth.attack_tau);
        assert!(snappy.gravity > smooth.gravity * 10.0);
        assert!(snappy.peak_hold < smooth.peak_hold);
        assert_eq!(Dynamics::from_smoothing(500), smooth);
    }

    #[test]
    fn band_rises_fast_falls_with_gravity_and_holds_its_peak() {
        let dynamics = Dynamics::from_smoothing(60);
        let mut band = BandState::default();
        for _ in 0..10 {
            band.step(1.0, 0.02, &dynamics);
        }
        assert!(band.value > 0.9, "attack reached {}", band.value);
        let top = band.value;
        // Falling: monotonically decreasing, accelerating (gravity), never below target.
        let mut previous = top;
        let mut previous_drop = 0.0;
        let mut accelerated = false;
        for _ in 0..30 {
            band.step(0.2, 0.02, &dynamics);
            let drop = previous - band.value;
            assert!(drop >= -1e-6 && band.value >= 0.2 - 1e-6);
            if drop > previous_drop + 1e-6 {
                accelerated = true;
            }
            previous = band.value;
            previous_drop = drop;
        }
        assert!(accelerated, "gravity should speed up the fall");
        // Peak marker stayed at the top while the bar fell (hold time 0.56 s > 0.6 s of falling?).
        assert!(band.peak >= band.value);
        assert!(
            band.peak > 0.5,
            "peak {} should still be above the bar",
            band.peak
        );
        // After a long rest the peak comes down to the bar.
        for _ in 0..200 {
            band.step(0.2, 0.02, &dynamics);
        }
        assert!((band.peak - 0.2).abs() < 1e-3 && (band.value - 0.2).abs() < 1e-3);
    }

    #[test]
    fn peak_holds_before_falling_and_large_dt_is_capped() {
        let dynamics = Dynamics::from_smoothing(50);
        let mut band = BandState::default();
        band.step(1.0, 1.0, &dynamics);
        let peak = band.peak;
        // dt is capped at 0.25 s, so one huge step cannot teleport the state.
        band.step(0.0, 100.0, &dynamics);
        assert!(band.value > 0.0);
        assert!((band.peak - peak).abs() < 1e-6, "peak held during rest");
    }
}
