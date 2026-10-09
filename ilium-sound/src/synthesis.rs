//! Deterministic, bounded PCM synthesis for the custom notification sound.
//!
//! Persisted controls use integers so hand-edited TOML cannot introduce NaN or
//! infinity and `SoundSettings` can retain its `Eq` contract. Rendering always
//! normalizes controls before doing floating-point signal math. Preview and WAV
//! are projections of the same PCM renderer.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

pub const SAMPLE_RATE: u32 = 44_100;
pub const MAX_DURATION_MS: u16 = 3_000;
pub const MAX_PCM_SAMPLES: usize = SAMPLE_RATE as usize * MAX_DURATION_MS as usize / 1_000;
pub const MAX_PREVIEW_COLUMNS: usize = 240;
const CANCELLATION_POLL_SAMPLES: usize = 1_024;
const MAX_AMPLITUDE: f64 = 0.8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Waveform {
    Sine,
    #[default]
    Triangle,
    Saw,
    Square,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundDesign {
    /// Fundamental wave shape; brightness blends in its stronger overtones.
    pub waveform: Waveform,
    /// Fundamental frequency, clamped to 60..=1,800 Hz.
    pub pitch_hz: u16,
    /// Linear pitch movement from beginning to end, in cents, -1,800..=1,800.
    pub pitch_slide_cents: i16,
    /// Attack, decay, sustain, and release envelope controls.
    pub attack_ms: u16,
    pub decay_ms: u16,
    pub sustain_percent: u8,
    pub release_ms: u16,
    /// Pulse rate in tenths of Hz and its amplitude modulation depth.
    pub pulse_rate_tenths_hz: u16,
    pub pulse_depth_percent: u8,
    /// A second sine voice at this interval, mixed with the fundamental.
    pub harmony_semitones: i8,
    pub harmony_mix_percent: u8,
    /// Wave shape, second harmonic, and deterministic noise texture.
    pub brightness_percent: u8,
    pub noise_percent: u8,
    /// 80..=3,000 ms and 0..=100 percent; rendered peak stays below 0.8.
    pub duration_ms: u16,
    pub volume_percent: u8,
}

impl Default for SoundDesign {
    fn default() -> Self {
        Self {
            waveform: Waveform::Triangle,
            pitch_hz: 720,
            pitch_slide_cents: -500,
            attack_ms: 6,
            decay_ms: 100,
            sustain_percent: 45,
            release_ms: 160,
            pulse_rate_tenths_hz: 80,
            pulse_depth_percent: 30,
            harmony_semitones: 7,
            harmony_mix_percent: 20,
            brightness_percent: 55,
            noise_percent: 5,
            duration_ms: 550,
            volume_percent: 60,
        }
    }
}

impl SoundDesign {
    /// Caps every hand-editable control. The original design can be preserved
    /// as authored data; call this at both rendering and save boundaries.
    pub fn normalized(&self) -> Self {
        Self {
            waveform: self.waveform,
            pitch_hz: self.pitch_hz.clamp(60, 1_800),
            pitch_slide_cents: self.pitch_slide_cents.clamp(-1_800, 1_800),
            attack_ms: self.attack_ms.clamp(1, 1_000),
            decay_ms: self.decay_ms.min(1_000),
            sustain_percent: self.sustain_percent.min(100),
            release_ms: self.release_ms.clamp(1, 1_500),
            pulse_rate_tenths_hz: self.pulse_rate_tenths_hz.min(160),
            pulse_depth_percent: self.pulse_depth_percent.min(95),
            harmony_semitones: self.harmony_semitones.clamp(-12, 12),
            harmony_mix_percent: self.harmony_mix_percent.min(60),
            brightness_percent: self.brightness_percent.min(100),
            noise_percent: self.noise_percent.min(40),
            duration_ms: self.duration_ms.clamp(80, MAX_DURATION_MS),
            volume_percent: self.volume_percent.min(100),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveformColumn {
    pub min: i16,
    pub max: i16,
}

/// Returns mono signed 16-bit PCM. At 44.1 kHz the normalized duration is at
/// most 132,300 samples (264,600 bytes), regardless of untrusted config input.
pub fn render_pcm(design: &SoundDesign) -> Vec<i16> {
    render_pcm_cancellable(design, || false).expect("unconditionally accepted synthesis")
}

fn render_pcm_cancellable(
    design: &SoundDesign,
    mut should_cancel: impl FnMut() -> bool,
) -> Option<Vec<i16>> {
    let design = design.normalized();
    let frame_count = SAMPLE_RATE as usize * usize::from(design.duration_ms) / 1_000;
    let mut samples = Vec::with_capacity(frame_count);
    let mut phase = 0.0_f64;
    let mut harmony_phase = 0.0_f64;
    let brightness = f64::from(design.brightness_percent) / 100.0;
    let harmony_mix = f64::from(design.harmony_mix_percent) / 100.0;
    let noise_mix = f64::from(design.noise_percent) / 100.0;
    let pulse_depth = f64::from(design.pulse_depth_percent) / 100.0;
    let pulse_rate_hz = f64::from(design.pulse_rate_tenths_hz) / 10.0;
    let harmony_ratio = 2.0_f64.powf(f64::from(design.harmony_semitones) / 12.0);
    let volume = MAX_AMPLITUDE * f64::from(design.volume_percent) / 100.0;

    for index in 0..frame_count {
        if index & (CANCELLATION_POLL_SAMPLES - 1) == 0 && should_cancel() {
            return None;
        }
        let progress = index as f64 / (frame_count - 1) as f64;
        let time_seconds = index as f64 / f64::from(SAMPLE_RATE);
        let pitch_hz = f64::from(design.pitch_hz)
            * 2.0_f64.powf(f64::from(design.pitch_slide_cents) * progress / 1_200.0);
        let sine = (TAU * phase).sin();
        let selected = wave_at(design.waveform, phase);
        // Convex combinations keep every stage within [-1, 1]. The second
        // harmonic also makes brightness audible when Sine is selected.
        let overtone = 0.8 * selected + 0.2 * (2.0 * TAU * phase).sin();
        let timbre = (1.0 - brightness) * sine + brightness * overtone;
        let harmony = (TAU * harmony_phase).sin();
        let tone = (1.0 - harmony_mix) * timbre + harmony_mix * harmony;
        let texture = (1.0 - noise_mix) * tone + noise_mix * indexed_noise(index);
        let pulse = if design.pulse_rate_tenths_hz == 0 {
            1.0
        } else {
            let gate = 0.5 + 0.5 * (TAU * pulse_rate_hz * time_seconds).cos();
            1.0 - pulse_depth + pulse_depth * gate
        };
        let envelope = envelope_at(&design, progress * f64::from(design.duration_ms));
        let value = texture * pulse * envelope * volume;
        // The signal has a mathematical 0.8 bound, so conversion cannot clip.
        samples.push((value * f64::from(i16::MAX)).round() as i16);

        phase = (phase + pitch_hz / f64::from(SAMPLE_RATE)).fract();
        harmony_phase = (harmony_phase + pitch_hz * harmony_ratio / f64::from(SAMPLE_RATE)).fract();
    }
    Some(samples)
}

/// Encodes the same PCM as a standard 44-byte mono PCM RIFF/WAVE stream.
pub fn render_wav(design: &SoundDesign) -> Vec<u8> {
    let pcm = render_pcm(design);
    let data_bytes = (pcm.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in pcm {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

/// Computes exact min/max columns from the rendered audio for a terminal
/// waveform. A zero-width viewport returns no columns; hostile widths cap at
/// 240 so resizing cannot allocate an unbounded preview.
pub fn waveform_preview(design: &SoundDesign, columns: usize) -> Vec<WaveformColumn> {
    try_waveform_preview(design, columns, || false)
        .expect("unconditionally accepted waveform preview")
}

/// Computes a preview while checking cancellation every 1,024 PCM samples.
/// Cancellation drops partial synthesis and never publishes partial columns.
pub fn try_waveform_preview(
    design: &SoundDesign,
    columns: usize,
    should_cancel: impl FnMut() -> bool,
) -> Option<Vec<WaveformColumn>> {
    let columns = columns.min(MAX_PREVIEW_COLUMNS);
    if columns == 0 {
        return Some(Vec::new());
    }
    let pcm = render_pcm_cancellable(design, should_cancel)?;
    let columns = columns.min(pcm.len());
    Some(
        (0..columns)
            .map(|index| {
                let start = index * pcm.len() / columns;
                let end = (index + 1) * pcm.len() / columns;
                let segment = &pcm[start..end];
                WaveformColumn {
                    min: *segment.iter().min().unwrap_or(&0),
                    max: *segment.iter().max().unwrap_or(&0),
                }
            })
            .collect(),
    )
}

fn wave_at(waveform: Waveform, phase: f64) -> f64 {
    match waveform {
        Waveform::Sine => (TAU * phase).sin(),
        Waveform::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
        Waveform::Saw => 2.0 * phase - 1.0,
        Waveform::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
    }
}

fn indexed_noise(index: usize) -> f64 {
    let mut value = (index as u32) ^ 0x9e37_79b9;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    f64::from(value) / f64::from(u32::MAX) * 2.0 - 1.0
}

fn envelope_at(design: &SoundDesign, time_ms: f64) -> f64 {
    let duration = f64::from(design.duration_ms);
    let attack = f64::from(design.attack_ms);
    let decay = f64::from(design.decay_ms);
    let release = f64::from(design.release_ms);
    let scale = (duration / (attack + decay + release)).min(1.0);
    let attack_end = attack * scale;
    let decay_end = attack_end + decay * scale;
    let release_start = duration - release * scale;
    let sustain = f64::from(design.sustain_percent) / 100.0;

    if time_ms < attack_end {
        time_ms / attack_end
    } else if time_ms < decay_end {
        1.0 - (1.0 - sustain) * (time_ms - attack_end) / (decay * scale)
    } else if time_ms < release_start {
        sustain
    } else {
        sustain * (duration - time_ms) / (release * scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_and_pcm_payload_are_consistent_and_repeatable() {
        let design = SoundDesign::default();
        let first = render_wav(&design);
        let second = render_wav(&design);
        let pcm = render_pcm(&design);
        assert_eq!(first, second);
        assert_eq!(&first[0..4], b"RIFF");
        assert_eq!(&first[8..12], b"WAVE");
        assert_eq!(&first[12..16], b"fmt ");
        assert_eq!(
            u32::from_le_bytes(first[24..28].try_into().unwrap()),
            SAMPLE_RATE
        );
        assert_eq!(u16::from_le_bytes(first[34..36].try_into().unwrap()), 16);
        assert_eq!(&first[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes(first[4..8].try_into().unwrap()) as usize + 8,
            first.len()
        );
        assert_eq!(
            u32::from_le_bytes(first[40..44].try_into().unwrap()) as usize,
            pcm.len() * 2
        );
        for (sample, encoded) in pcm.iter().zip(first[44..].chunks_exact(2)) {
            assert_eq!(sample.to_le_bytes(), encoded);
        }
    }

    #[test]
    fn hostile_controls_are_capped_and_audio_has_headroom_and_quiet_endpoints() {
        let design = SoundDesign {
            pitch_hz: u16::MAX,
            pitch_slide_cents: i16::MAX,
            attack_ms: u16::MAX,
            decay_ms: u16::MAX,
            sustain_percent: u8::MAX,
            release_ms: u16::MAX,
            pulse_rate_tenths_hz: u16::MAX,
            pulse_depth_percent: u8::MAX,
            harmony_semitones: i8::MAX,
            harmony_mix_percent: u8::MAX,
            brightness_percent: u8::MAX,
            noise_percent: u8::MAX,
            duration_ms: u16::MAX,
            volume_percent: u8::MAX,
            ..SoundDesign::default()
        };
        let normalized = design.normalized();
        assert_eq!(normalized.duration_ms, MAX_DURATION_MS);
        assert!(normalized.pitch_hz <= 1_800);
        assert!(normalized.harmony_mix_percent <= 60);
        let pcm = render_pcm(&design);
        assert!(pcm.len() <= MAX_PCM_SAMPLES);
        assert_eq!(pcm.first(), Some(&0));
        assert_eq!(pcm.last(), Some(&0));
        assert!(pcm.iter().any(|sample| *sample != 0));
        assert!(pcm.iter().all(|sample| i32::from(sample.abs()) <= 26_215));
    }

    #[test]
    fn each_active_control_changes_the_audible_pcm() {
        let base = SoundDesign::default();
        let baseline = render_pcm(&base);
        let variants = [
            SoundDesign {
                waveform: Waveform::Saw,
                ..base.clone()
            },
            SoundDesign {
                pitch_hz: 900,
                ..base.clone()
            },
            SoundDesign {
                pitch_slide_cents: 300,
                ..base.clone()
            },
            SoundDesign {
                attack_ms: 100,
                ..base.clone()
            },
            SoundDesign {
                decay_ms: 300,
                ..base.clone()
            },
            SoundDesign {
                sustain_percent: 70,
                ..base.clone()
            },
            SoundDesign {
                release_ms: 400,
                ..base.clone()
            },
            SoundDesign {
                pulse_rate_tenths_hz: 120,
                ..base.clone()
            },
            SoundDesign {
                pulse_depth_percent: 60,
                ..base.clone()
            },
            SoundDesign {
                harmony_semitones: 12,
                ..base.clone()
            },
            SoundDesign {
                harmony_mix_percent: 40,
                ..base.clone()
            },
            SoundDesign {
                brightness_percent: 80,
                ..base.clone()
            },
            SoundDesign {
                noise_percent: 20,
                ..base.clone()
            },
            SoundDesign {
                volume_percent: 80,
                ..base.clone()
            },
        ];
        for (index, variant) in variants.into_iter().enumerate() {
            assert_ne!(
                render_pcm(&variant),
                baseline,
                "control {index} had no effect"
            );
        }
        assert_ne!(
            render_pcm(&SoundDesign {
                duration_ms: 700,
                ..base
            }),
            baseline
        );
    }

    #[test]
    fn preview_columns_are_exact_extrema_of_the_rendered_pcm() {
        let design = SoundDesign::default();
        let pcm = render_pcm(&design);
        let preview = waveform_preview(&design, 17);
        assert_eq!(preview.len(), 17);
        for (index, column) in preview.iter().enumerate() {
            let start = index * pcm.len() / preview.len();
            let end = (index + 1) * pcm.len() / preview.len();
            assert_eq!(column.min, *pcm[start..end].iter().min().unwrap());
            assert_eq!(column.max, *pcm[start..end].iter().max().unwrap());
        }
        assert!(waveform_preview(&design, 0).is_empty());
        assert_eq!(
            waveform_preview(&design, usize::MAX).len(),
            MAX_PREVIEW_COLUMNS
        );
    }

    #[test]
    fn preview_render_stops_when_cancellation_arrives_during_synthesis() {
        let mut checks = 0;
        let result = try_waveform_preview(&SoundDesign::default(), 17, || {
            checks += 1;
            checks == 3
        });

        assert!(
            result.is_none(),
            "cancelled synthesis must not publish columns"
        );
        assert_eq!(checks, 3, "cancellation should stop further sample work");
    }

    #[test]
    fn cancellable_preview_matches_the_existing_waveform_renderer() {
        let design = SoundDesign::default();
        let pcm = render_pcm(&design);
        let expected = (0..17)
            .map(|index| {
                let start = index * pcm.len() / 17;
                let end = (index + 1) * pcm.len() / 17;
                let samples = &pcm[start..end];
                WaveformColumn {
                    min: *samples.iter().min().expect("nonempty preview interval"),
                    max: *samples.iter().max().expect("nonempty preview interval"),
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(try_waveform_preview(&design, 17, || false), Some(expected));
    }

    #[test]
    fn design_serialization_round_trips_without_floating_point_values() {
        let design = SoundDesign::default();
        let encoded = serde_json::to_string(&design).unwrap();
        let decoded: SoundDesign = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, design);
    }
}
