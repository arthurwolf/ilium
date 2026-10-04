//! Selective host audio service. Capture selection is authenticated by the Rust
//! broker; scripts never choose a fallback device or manufacture a grant.
//! No worker is created here. Backend adapters must transfer their native worker
//! admissions to actual join custody, including on failed/timed-out retirement.
use crate::error::{AnimationError, Result};
use ilium_ambient::{resources::AmbientResources, AudioFft};
use ilium_execution::{QuotaGroup, StorageAdmission};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc, time::Instant};
fn invalid(message: &str) -> AnimationError {
    AnimationError::Runtime(message.into())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioProduct {
    Level,
    Waveform,
    Envelope,
    Bands,
    History,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioWindow {
    Hann,
    Blackman,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioDemand {
    pub products: BTreeSet<AudioProduct>,
    pub sample_rate: u32,
    pub max_hz: u32,
    pub fft_samples: usize,
    pub waveform_samples: usize,
    pub envelope_samples: usize,
    pub band_count: usize,
    pub history_frames: usize,
    pub window: AudioWindow,
    pub smoothing_ms: u32,
}
impl Default for AudioDemand {
    fn default() -> Self {
        Self {
            products: BTreeSet::new(),
            sample_rate: 44100,
            max_hz: 30,
            fft_samples: 1024,
            waveform_samples: 256,
            envelope_samples: 64,
            band_count: 32,
            history_frames: 32,
            window: AudioWindow::Hann,
            smoothing_ms: 100,
        }
    }
}
impl AudioDemand {
    pub fn validate(&self) -> Result<()> {
        if !(8000..=96000).contains(&self.sample_rate)
            || !(1..=120).contains(&self.max_hz)
            || !(128..=8192).contains(&self.fft_samples)
            || !self.fft_samples.is_power_of_two()
            || !(1..=4096).contains(&self.waveform_samples)
            || !(1..=1024).contains(&self.envelope_samples)
            || !(1..=256).contains(&self.band_count)
            || !(1..=128).contains(&self.history_frames)
            || self.smoothing_ms > 2000
        {
            return Err(invalid("audio demand outside bounded limits"));
        }
        Ok(())
    }
    pub fn is_empty(&self) -> bool {
        self.products.is_empty()
    }
    fn wants(&self, product: AudioProduct) -> bool {
        self.products.contains(&product)
    }
    fn spectral(&self) -> bool {
        self.wants(AudioProduct::Bands) || self.wants(AudioProduct::History)
    }
}
#[derive(Debug, Serialize)]
pub struct AudioSnapshot {
    pub captured_at_ms: u64,
    pub sample_rate: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rms: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waveform: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bands: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub band_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_frames: Option<usize>,
}
/// Immutable snapshot admission survives its producer. No uncharged payload Clone.
pub struct RetainedAudioSnapshot {
    value: AudioSnapshot,
    quota: QuotaGroup,
    _admission: StorageAdmission,
}
impl std::fmt::Debug for RetainedAudioSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RetainedAudioSnapshot")
            .field("value", &self.value)
            .field("_admission", &self._admission)
            .finish_non_exhaustive()
    }
}
impl RetainedAudioSnapshot {
    /// Compare actual original admission roots; equal ceilings do not suffice.
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.quota.shares_root(quota)
    }
    pub fn view(&self) -> &AudioSnapshot {
        &self.value
    }
}
pub struct AudioProcessor {
    demand: AudioDemand,
    quota: QuotaGroup,
    _storage: StorageAdmission,
    samples: Vec<f32>,
    write: usize,
    filled: usize,
    fft: Option<AudioFft>,
    window: Vec<f32>,
    real: Vec<f32>,
    imag: Vec<f32>,
    smoothed: Vec<f32>,
    history: Vec<f32>,
    history_write: usize,
    history_len: usize,
    previous_ms: Option<u64>,
    fft_count: u64,
    cancelled: bool,
}
impl AudioProcessor {
    pub fn new(demand: AudioDemand, quota: QuotaGroup) -> Result<Self> {
        demand.validate()?;
        // Charge conservative complete Vec capacities, FFT tables and snapshot
        // transient workspace BEFORE constructing any owned sample buffers.
        let sample_count = demand
            .fft_samples
            .max(demand.waveform_samples)
            .max(demand.envelope_samples);
        let bytes = 4096
            + sample_count * 4
            + demand.fft_samples * 40
            + demand.band_count * 8
            + demand.band_count * demand.history_frames * 4;
        let storage = quota
            .reserve_external_storage(bytes)
            .map_err(|error| AnimationError::Budget(format!("audio DSP admission: {error:?}")))?;
        let spectral = demand.spectral();
        let fft = if spectral {
            Some(AudioFft::new(demand.fft_samples).map_err(|message| invalid(&message))?)
        } else {
            None
        };
        let window = if spectral {
            (0..demand.fft_samples)
                .map(|index| {
                    let angle = std::f64::consts::TAU * index as f64 / demand.fft_samples as f64;
                    match demand.window {
                        AudioWindow::Hann => (0.5 - 0.5 * angle.cos()) as f32,
                        AudioWindow::Blackman => {
                            (0.42 - 0.5 * angle.cos() + 0.08 * (2. * angle).cos()) as f32
                        }
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let real = if spectral {
            vec![0.; demand.fft_samples]
        } else {
            Vec::new()
        };
        let imag = if spectral {
            vec![0.; demand.fft_samples]
        } else {
            Vec::new()
        };
        let smoothed = if spectral {
            vec![0.; demand.band_count]
        } else {
            Vec::new()
        };
        let history = if demand.wants(AudioProduct::History) {
            vec![0.; demand.band_count * demand.history_frames]
        } else {
            Vec::new()
        };
        Ok(Self {
            demand,
            quota,
            _storage: storage,
            samples: vec![0.; sample_count],
            write: 0,
            filled: 0,
            fft,
            window,
            real,
            imag,
            smoothed,
            history,
            history_write: 0,
            history_len: 0,
            previous_ms: None,
            fft_count: 0,
            cancelled: false,
        })
    }
    pub fn has_fft(&self) -> bool {
        self.fft.is_some()
    }
    pub fn fft_count(&self) -> u64 {
        self.fft_count
    }
    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.samples.fill(0.);
        self.history.fill(0.);
    }
    pub fn push(&mut self, samples: &[f32]) -> Result<()> {
        if self.cancelled {
            return Err(invalid("audio processor cancelled"));
        }
        if samples.len() > 8192 {
            return Err(invalid("audio sample batch exceeds bound"));
        }
        for &sample in samples {
            self.samples[self.write] = if sample.is_finite() {
                sample.clamp(-1., 1.)
            } else {
                0.
            };
            self.write = (self.write + 1) % self.samples.len();
            self.filled = (self.filled + 1).min(self.samples.len());
        }
        Ok(())
    }
    fn recent(&self, offset: usize) -> f32 {
        if offset >= self.filled {
            return 0.;
        }
        self.samples[(self.write + self.samples.len() - 1 - offset) % self.samples.len()]
    }
    pub fn snapshot(&mut self, now_ms: u64) -> Result<Arc<RetainedAudioSnapshot>> {
        if self.cancelled || self.demand.is_empty() {
            return Err(invalid("audio products unavailable"));
        }
        if self.previous_ms.is_some_and(|previous| {
            now_ms < previous
                || now_ms - previous < (1000_u64.div_ceil(u64::from(self.demand.max_hz)))
        }) {
            return Err(invalid("audio snapshot rate exceeded"));
        }
        let history_count = (self.history_len + 1).min(self.demand.history_frames);
        let output_samples = usize::from(self.demand.wants(AudioProduct::Waveform))
            * self.demand.waveform_samples
            + usize::from(self.demand.wants(AudioProduct::Envelope)) * self.demand.envelope_samples
            + usize::from(self.demand.wants(AudioProduct::Bands)) * self.demand.band_count
            + usize::from(self.demand.wants(AudioProduct::History))
                * self.demand.band_count
                * history_count;
        let admission = self
            .quota
            .reserve_external_storage(1024 + output_samples * 4)
            .map_err(|error| {
                AnimationError::Budget(format!("audio snapshot admission: {error:?}"))
            })?;
        let rms = if self.demand.wants(AudioProduct::Level) {
            Some(
                ((0..self.filled)
                    .map(|offset| f64::from(self.recent(offset)).powi(2))
                    .sum::<f64>()
                    / self.filled.max(1) as f64)
                    .sqrt() as f32,
            )
        } else {
            None
        };
        if let Some(fft) = &self.fft {
            let size = self.demand.fft_samples;
            for index in 0..size {
                self.real[index] = self.samples[(self.write + self.samples.len()
                    - size % self.samples.len()
                    + index)
                    % self.samples.len()]
                    * self.window[index];
            }
            self.imag.fill(0.);
            fft.forward(&mut self.real, &mut self.imag);
            self.fft_count += 1;
            let gain = self.window.iter().sum::<f32>().max(1.);
            let bins = size / 2;
            let dt = self
                .previous_ms
                .map_or(1., |previous| (now_ms - previous) as f32 / 1000.);
            let blend = if self.demand.smoothing_ms == 0 {
                1.
            } else {
                1. - (-dt / (self.demand.smoothing_ms as f32 / 1000.)).exp()
            };
            for band in 0..self.demand.band_count {
                let low = (1 + band * bins / self.demand.band_count).min(bins);
                let high = (1 + (band + 1) * bins / self.demand.band_count)
                    .min(bins + 1)
                    .max(low + 1);
                let peak = (low..high)
                    .map(|bin| self.real[bin].hypot(self.imag[bin]) * 2. / gain)
                    .fold(0., f32::max)
                    .clamp(0., 1.);
                self.smoothed[band] += (peak - self.smoothed[band]) * blend;
            }
        }
        if self.demand.wants(AudioProduct::History) {
            let start = self.history_write * self.demand.band_count;
            self.history[start..start + self.demand.band_count].copy_from_slice(&self.smoothed);
            self.history_write = (self.history_write + 1) % self.demand.history_frames;
            self.history_len = history_count;
        }
        let waveform = self.demand.wants(AudioProduct::Waveform).then(|| {
            (0..self.demand.waveform_samples)
                .rev()
                .map(|offset| self.recent(offset))
                .collect()
        });
        let envelope = self.demand.wants(AudioProduct::Envelope).then(|| {
            let span = self.filled.max(self.demand.envelope_samples);
            (0..self.demand.envelope_samples)
                .map(|bucket| {
                    let low = bucket * span / self.demand.envelope_samples;
                    let high = ((bucket + 1) * span / self.demand.envelope_samples).max(low + 1);
                    (low..high)
                        .map(|offset| self.recent(span - 1 - offset).abs())
                        .fold(0., f32::max)
                })
                .collect()
        });
        let bands = self
            .demand
            .wants(AudioProduct::Bands)
            .then(|| self.smoothed.clone());
        let history = self.demand.wants(AudioProduct::History).then(|| {
            let oldest = (self.history_write + self.demand.history_frames - self.history_len)
                % self.demand.history_frames;
            let mut values = Vec::with_capacity(self.history_len * self.demand.band_count);
            for row in 0..self.history_len {
                let start = ((oldest + row) % self.demand.history_frames) * self.demand.band_count;
                values.extend_from_slice(&self.history[start..start + self.demand.band_count]);
            }
            values
        });
        self.previous_ms = Some(now_ms);
        Ok(Arc::new(RetainedAudioSnapshot {
            value: AudioSnapshot {
                captured_at_ms: now_ms,
                sample_rate: self.demand.sample_rate,
                level: rms,
                rms,
                waveform,
                envelope,
                bands,
                history,
                band_count: self.demand.spectral().then_some(self.demand.band_count),
                history_frames: self
                    .demand
                    .wants(AudioProduct::History)
                    .then_some(self.history_len),
            },
            quota: self.quota.clone(),
            _admission: admission,
        }))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioSourceSelection {
    Loopback,
    Microphone,
    Device(String),
}
/// Constructed by the trusted Rust permissions broker after matching the exact
/// selected device/source and accepted product set to a current grant epoch.
pub struct AuthenticatedAudioGrant {
    source: AudioSourceSelection,
    products: BTreeSet<AudioProduct>,
    epoch: u64,
}
impl AuthenticatedAudioGrant {
    pub fn from_host(
        source: AudioSourceSelection,
        products: BTreeSet<AudioProduct>,
        epoch: u64,
    ) -> Result<Self> {
        if epoch == 0
            || matches!(&source,AudioSourceSelection::Device(name) if name.is_empty()||name.len()>256||name.chars().any(char::is_control))
        {
            return Err(invalid("invalid authenticated audio binding"));
        }
        Ok(Self {
            source,
            products,
            epoch,
        })
    }
    pub fn source(&self) -> &AudioSourceSelection {
        &self.source
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}
/// Implementations own their complete capture graph. read_mono writes only into
/// the supplied bounded slice and never blocks longer than the admitted source
/// timeout. Stop wakes reads. A timed-out join MUST transfer native resources and
/// their admissions to the platform custody supervisor; dropping cannot detach.
pub trait OwnedAudioCapture {
    fn sample_rate(&self) -> u32;
    fn read_mono(&mut self, out: &mut [f32]) -> Result<usize>;
    fn request_stop(&mut self);
    fn join_until(&mut self, deadline: Instant) -> Result<bool>;
}
pub trait AuthenticatedCaptureFactory {
    fn open(
        &mut self,
        grant: &AuthenticatedAudioGrant,
        demand: &AudioDemand,
        resources: &AmbientResources,
    ) -> Result<Box<dyn OwnedAudioCapture>>;
}
pub struct NativeAudioService {
    capture: Option<Box<dyn OwnedAudioCapture>>,
    processor: AudioProcessor,
    scratch: Vec<f32>,
    _scratch_admission: StorageAdmission,
    epoch: u64,
    cancelled: bool,
}
impl NativeAudioService {
    pub fn open(
        demand: AudioDemand,
        grant: AuthenticatedAudioGrant,
        resources: &AmbientResources,
        quota: QuotaGroup,
        factory: &mut dyn AuthenticatedCaptureFactory,
    ) -> Result<Self> {
        demand.validate()?;
        if !quota.shares_root(&resources.finite().quota_group()) {
            return Err(invalid("audio resources use a different quota"));
        }
        if !demand.products.is_subset(&grant.products) {
            return Err(AnimationError::PermissionDenied(
                "audio product set exceeds grant".into(),
            ));
        }
        let scratch_admission = quota.reserve_external_storage(8192 * 4).map_err(|error| {
            AnimationError::Budget(format!("capture scratch admission: {error:?}"))
        })?;
        let processor = AudioProcessor::new(demand.clone(), quota)?;
        // Empty demand never opens a microphone, device or native helper.
        let capture = if demand.is_empty() {
            None
        } else {
            Some(factory.open(&grant, &demand, resources)?)
        };
        let mut service = Self {
            capture,
            processor,
            scratch: vec![0.; 8192],
            _scratch_admission: scratch_admission,
            epoch: grant.epoch,
            cancelled: false,
        };
        if service
            .capture
            .as_ref()
            .is_some_and(|capture| capture.sample_rate() != demand.sample_rate)
        {
            service.cancel();
            return Err(invalid(
                "capture sample rate does not match accepted demand",
            ));
        }
        Ok(service)
    }
    pub fn poll(
        &mut self,
        now_ms: u64,
        current_epoch: u64,
    ) -> Result<Option<Arc<RetainedAudioSnapshot>>> {
        if current_epoch != self.epoch {
            self.cancel();
            return Err(AnimationError::PermissionDenied(
                "audio grant epoch expired".into(),
            ));
        }
        if self.cancelled {
            return Err(invalid("audio service cancelled"));
        }
        let Some(capture) = self.capture.as_mut() else {
            return Ok(None);
        };
        let count = match capture.read_mono(&mut self.scratch) {
            Ok(count) => count,
            Err(error) => {
                self.cancel();
                return Err(error);
            }
        };
        if count > self.scratch.len() {
            self.cancel();
            return Err(invalid("capture returned oversized batch"));
        }
        self.processor.push(&self.scratch[..count])?;
        if self.processor.previous_ms.is_some_and(|previous| {
            now_ms >= previous
                && now_ms - previous < 1000_u64.div_ceil(u64::from(self.processor.demand.max_hz))
        }) {
            return Ok(None);
        }
        self.processor.snapshot(now_ms).map(Some)
    }
    pub fn cancel(&mut self) {
        if self.cancelled {
            return;
        }
        self.cancelled = true;
        self.processor.cancel();
        if let Some(capture) = self.capture.as_mut() {
            capture.request_stop();
        }
    }
    pub fn retire(&mut self, deadline: Instant) -> Result<bool> {
        self.cancel();
        match self.capture.as_mut() {
            Some(capture) => capture.join_until(deadline),
            None => Ok(true),
        }
    }
}
impl Drop for NativeAudioService {
    fn drop(&mut self) {
        self.cancel();
        if let Some(capture) = self.capture.as_mut() {
            let _ = capture.join_until(Instant::now() + std::time::Duration::from_millis(250));
        }
    }
}
