//! CPAL audio ownership and deterministic streaming sample conversion.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use tokio::sync::mpsc;

use crate::{VoiceError, VoiceInputMode};

pub(crate) const REALTIME_SAMPLE_RATE: u32 = 24_000;
const CAPTURE_CHANNEL_CAPACITY: usize = 32;
const MAX_BUFFERED_PLAYBACK_SECONDS: usize = 30;

/// One mono, signed 16-bit, 24 kHz frame ready for the Realtime API.
#[derive(Debug)]
pub(crate) struct CapturedAudio {
    pub pcm16_le: Vec<u8>,
}

/// Owns both device streams. Dropping this object stops capture and playback.
pub(crate) struct AudioEngine {
    _input_stream: cpal::Stream,
    _output_stream: cpal::Stream,
    capture_receiver: mpsc::Receiver<CapturedAudio>,
    capture_enabled: Arc<AtomicBool>,
    playback_samples: Arc<Mutex<VecDeque<f32>>>,
    played_output_frames: Arc<AtomicU64>,
    output_sample_rate: u32,
    output_resampler: StreamingLinearResampler,
    output_volume: f32,
}

impl AudioEngine {
    pub(crate) fn start(
        input_device_name: Option<&str>,
        output_device_name: Option<&str>,
        input_mode: VoiceInputMode,
        output_volume_percent: u8,
    ) -> Result<Self, VoiceError> {
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

        let capture_enabled = Arc::new(AtomicBool::new(matches!(
            input_mode,
            VoiceInputMode::SemanticVad
        )));
        let (capture_sender, capture_receiver) = mpsc::channel(CAPTURE_CHANNEL_CAPACITY);
        let input_resampler = Arc::new(Mutex::new(StreamingLinearResampler::new(
            input_supported_config.sample_rate(),
            REALTIME_SAMPLE_RATE,
        )));
        let input_stream = build_input_stream(
            &input_device,
            &input_supported_config,
            capture_sender,
            capture_enabled.clone(),
            input_resampler,
        )?;

        let playback_samples = Arc::new(Mutex::new(VecDeque::new()));
        let played_output_frames = Arc::new(AtomicU64::new(0));
        let output_stream = build_output_stream(
            &output_device,
            &output_supported_config,
            playback_samples.clone(),
            played_output_frames.clone(),
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
        Ok(Self {
            _input_stream: input_stream,
            _output_stream: output_stream,
            capture_receiver,
            capture_enabled,
            playback_samples,
            played_output_frames,
            output_sample_rate,
            output_resampler: StreamingLinearResampler::new(
                REALTIME_SAMPLE_RATE,
                output_sample_rate,
            ),
            output_volume: f32::from(output_volume_percent) / 100.0,
        })
    }

    pub(crate) async fn next_capture(&mut self) -> Option<CapturedAudio> {
        self.capture_receiver.recv().await
    }

    pub(crate) fn set_capture_enabled(&self, is_enabled: bool) {
        self.capture_enabled.store(is_enabled, Ordering::Release);
    }

    /// Marks the start of a new response item's audio. Also drops any
    /// samples still queued from a previous item: `response.created`/new
    /// `item_id` deltas can arrive before the device has finished draining
    /// the prior item's tail, and without clearing here those leftover
    /// samples would keep incrementing `played_output_frames` after the
    /// reset below, misattributing the old item's playback time to the new
    /// item's `audio_end_ms` on a later `conversation.item.truncate`.
    pub(crate) fn begin_response_audio(&self) {
        lock_recovering_poison(&self.playback_samples).clear();
        self.played_output_frames.store(0, Ordering::Release);
    }

    pub(crate) fn enqueue_realtime_pcm16(&mut self, bytes: &[u8]) {
        let input_samples = pcm16_le_to_f32(bytes);
        let mut output_samples = self.output_resampler.process(&input_samples);
        for sample in &mut output_samples {
            *sample *= self.output_volume;
        }

        let maximum_samples = self.output_sample_rate as usize * MAX_BUFFERED_PLAYBACK_SECONDS;
        let mut queue = lock_recovering_poison(&self.playback_samples);
        let excess = queue
            .len()
            .saturating_add(output_samples.len())
            .saturating_sub(maximum_samples);
        let drain_count = excess.min(queue.len());
        queue.drain(..drain_count);
        queue.extend(output_samples);
    }

    /// Stops pending playback and returns how many milliseconds were actually
    /// emitted to the device for the current response item.
    pub(crate) fn interrupt_playback(&self) -> u64 {
        lock_recovering_poison(&self.playback_samples).clear();
        let played_frames = self.played_output_frames.swap(0, Ordering::AcqRel);
        played_frames.saturating_mul(1_000) / u64::from(self.output_sample_rate)
    }
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
    capture_sender: mpsc::Sender<CapturedAudio>,
    capture_enabled: Arc<AtomicBool>,
    resampler: Arc<Mutex<StreamingLinearResampler>>,
) -> Result<cpal::Stream, VoiceError> {
    let channels = usize::from(supported_config.channels());
    let config = supported_config.config();

    macro_rules! build {
        ($sample_type:ty) => {
            build_typed_input_stream::<$sample_type>(
                device,
                &config,
                channels,
                capture_sender,
                capture_enabled,
                resampler,
            )
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
    capture_sender: mpsc::Sender<CapturedAudio>,
    capture_enabled: Arc<AtomicBool>,
    resampler: Arc<Mutex<StreamingLinearResampler>>,
) -> Result<cpal::Stream, VoiceError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            *config,
            move |data: &[T], _| {
                if !capture_enabled.load(Ordering::Acquire) || channels == 0 {
                    return;
                }

                let mono = data
                    .chunks(channels)
                    .map(|frame| {
                        frame
                            .iter()
                            .map(|sample| sample.to_sample::<f32>())
                            .sum::<f32>()
                            / channels as f32
                    })
                    .collect::<Vec<_>>();
                let converted = lock_recovering_poison(&resampler).process(&mono);
                if converted.is_empty() {
                    return;
                }

                let _ = capture_sender.try_send(CapturedAudio {
                    pcm16_le: f32_to_pcm16_le(&converted),
                });
            },
            |error| tracing::warn!(%error, "voice input stream error"),
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
    playback_samples: Arc<Mutex<VecDeque<f32>>>,
    played_output_frames: Arc<AtomicU64>,
) -> Result<cpal::Stream, VoiceError> {
    let channels = usize::from(supported_config.channels());
    let config = supported_config.config();

    macro_rules! build {
        ($sample_type:ty) => {
            build_typed_output_stream::<$sample_type>(
                device,
                &config,
                channels,
                playback_samples,
                played_output_frames,
            )
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

fn build_typed_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    playback_samples: Arc<Mutex<VecDeque<f32>>>,
    played_output_frames: Arc<AtomicU64>,
) -> Result<cpal::Stream, VoiceError>
where
    T: SizedSample + FromSample<f32>,
{
    device
        .build_output_stream(
            *config,
            move |output: &mut [T], _| {
                let mut queue = lock_recovering_poison(&playback_samples);
                let mut emitted_frames = 0_u64;
                for frame in output.chunks_mut(channels.max(1)) {
                    // Distinguish "real (possibly silent) queued sample" from
                    // "underrun padding" using `pop_front`'s `Option` itself,
                    // not the sample's value -- a value-based check (e.g.
                    // `sample != 0.0`) misclassifies a genuine trailing
                    // silent sample or a fully muted (volume 0) response as
                    // underrun, undercounting `played_output_frames` and
                    // skewing the elapsed-ms calculation in
                    // `interrupt_playback`.
                    let sample = match queue.pop_front() {
                        Some(sample) => {
                            emitted_frames = emitted_frames.saturating_add(1);
                            sample
                        }
                        None => 0.0,
                    };
                    for channel in frame {
                        *channel = T::from_sample(sample);
                    }
                }
                played_output_frames.fetch_add(emitted_frames, Ordering::Relaxed);
            },
            |error| tracing::warn!(%error, "voice output stream error"),
            None,
        )
        .map_err(|source| VoiceError::BuildAudioStream {
            direction: "output",
            source,
        })
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

/// Chunk-boundary-independent linear resampler. It deliberately keeps a
/// source sample between calls, preventing clicks and drift between CPAL
/// callbacks without coupling the API to a particular DSP library.
#[derive(Debug)]
struct StreamingLinearResampler {
    step: f64,
    source_position: f64,
    buffered_samples: Vec<f32>,
}

impl StreamingLinearResampler {
    fn new(input_sample_rate: u32, output_sample_rate: u32) -> Self {
        Self {
            step: f64::from(input_sample_rate) / f64::from(output_sample_rate),
            source_position: 0.0,
            buffered_samples: Vec::new(),
        }
    }

    fn process(&mut self, samples: &[f32]) -> Vec<f32> {
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
