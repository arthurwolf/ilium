//! Sound-studio controls project onto the same persisted design used by the
//! detached sound adapter. Preview is cached per edit, never per TUI frame.

use ilium_sound::{SoundDesign, SoundSettings, Waveform, WaveformColumn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundControl {
    Waveform,
    Pitch,
    Sweep,
    Attack,
    Decay,
    Sustain,
    Release,
    PulseRate,
    PulseDepth,
    Harmony,
    HarmonyMix,
    Brightness,
    Noise,
    Duration,
    Volume,
}

impl SoundControl {
    pub const ALL: [Self; 15] = [
        Self::Waveform,
        Self::Pitch,
        Self::Sweep,
        Self::Attack,
        Self::Decay,
        Self::Sustain,
        Self::Release,
        Self::PulseRate,
        Self::PulseDepth,
        Self::Harmony,
        Self::HarmonyMix,
        Self::Brightness,
        Self::Noise,
        Self::Duration,
        Self::Volume,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Waveform => "Wave shape",
            Self::Pitch => "Pitch",
            Self::Sweep => "Pitch slide",
            Self::Attack => "Attack",
            Self::Decay => "Decay",
            Self::Sustain => "Sustain",
            Self::Release => "Release",
            Self::PulseRate => "Pulse rate",
            Self::PulseDepth => "Pulse depth",
            Self::Harmony => "Harmony interval",
            Self::HarmonyMix => "Harmony blend",
            Self::Brightness => "Brightness",
            Self::Noise => "Texture",
            Self::Duration => "Duration",
            Self::Volume => "Volume",
        }
    }

    pub const fn range(self) -> (i32, i32) {
        match self {
            Self::Waveform => (0, 3),
            Self::Pitch => (60, 1800),
            Self::Sweep => (-1800, 1800),
            Self::Attack => (1, 1000),
            Self::Decay => (0, 1000),
            Self::Sustain => (0, 100),
            Self::Release => (1, 1500),
            Self::PulseRate => (0, 160),
            Self::PulseDepth => (0, 95),
            Self::Harmony => (-12, 12),
            Self::HarmonyMix => (0, 60),
            Self::Brightness => (0, 100),
            Self::Noise => (0, 40),
            Self::Duration => (80, 3000),
            Self::Volume => (0, 100),
        }
    }

    pub fn value(self, design: &SoundDesign) -> i32 {
        match self {
            Self::Waveform => match design.waveform {
                Waveform::Sine => 0,
                Waveform::Triangle => 1,
                Waveform::Saw => 2,
                Waveform::Square => 3,
            },
            Self::Pitch => i32::from(design.pitch_hz),
            Self::Sweep => i32::from(design.pitch_slide_cents),
            Self::Attack => i32::from(design.attack_ms),
            Self::Decay => i32::from(design.decay_ms),
            Self::Sustain => i32::from(design.sustain_percent),
            Self::Release => i32::from(design.release_ms),
            Self::PulseRate => i32::from(design.pulse_rate_tenths_hz),
            Self::PulseDepth => i32::from(design.pulse_depth_percent),
            Self::Harmony => i32::from(design.harmony_semitones),
            Self::HarmonyMix => i32::from(design.harmony_mix_percent),
            Self::Brightness => i32::from(design.brightness_percent),
            Self::Noise => i32::from(design.noise_percent),
            Self::Duration => i32::from(design.duration_ms),
            Self::Volume => i32::from(design.volume_percent),
        }
    }

    pub fn set(self, design: &mut SoundDesign, value: i32) {
        let (minimum, maximum) = self.range();
        let value = value.clamp(minimum, maximum);
        match self {
            Self::Waveform => {
                design.waveform = match value {
                    0 => Waveform::Sine,
                    1 => Waveform::Triangle,
                    2 => Waveform::Saw,
                    _ => Waveform::Square,
                }
            }
            Self::Pitch => design.pitch_hz = value as u16,
            Self::Sweep => design.pitch_slide_cents = value as i16,
            Self::Attack => design.attack_ms = value as u16,
            Self::Decay => design.decay_ms = value as u16,
            Self::Sustain => design.sustain_percent = value as u8,
            Self::Release => design.release_ms = value as u16,
            Self::PulseRate => design.pulse_rate_tenths_hz = value as u16,
            Self::PulseDepth => design.pulse_depth_percent = value as u8,
            Self::Harmony => design.harmony_semitones = value as i8,
            Self::HarmonyMix => design.harmony_mix_percent = value as u8,
            Self::Brightness => design.brightness_percent = value as u8,
            Self::Noise => design.noise_percent = value as u8,
            Self::Duration => design.duration_ms = value as u16,
            Self::Volume => design.volume_percent = value as u8,
        }
    }

    pub const fn number_unit(self) -> &'static str {
        match self {
            Self::Pitch | Self::PulseRate => "Hz",
            Self::Sweep => "cents",
            Self::Attack | Self::Decay | Self::Release | Self::Duration => "ms",
            Self::Harmony => "semitones",
            Self::Waveform => "",
            _ => "%",
        }
    }

    /// Direct entry uses display units and never applies slider-step rounding.
    pub fn number_text(self, design: &SoundDesign) -> String {
        if self == Self::PulseRate {
            format!("{:.1}", f64::from(self.value(design)) / 10.0)
        } else {
            self.value(design).to_string()
        }
    }

    pub fn parse_number(self, text: &str) -> Result<i32, String> {
        use crate::value_number::{NumberSpec, NumberValue};
        if self == Self::Waveform {
            return Err("Choose a wave shape from its catalog".into());
        }
        let (minimum, maximum) = self.range();
        if self == Self::PulseRate {
            let NumberValue::Decimal(value) = (NumberSpec::Decimal {
                minimum: f64::from(minimum) / 10.0,
                maximum: f64::from(maximum) / 10.0,
            })
            .parse(text)?
            else {
                return Err("Enter a pulse rate in Hz".into());
            };
            let tenths = value * 10.0;
            if tenths != tenths.round() {
                return Err("Pulse rate supports increments of 0.1 Hz".into());
            }
            return Ok(tenths as i32);
        }
        let NumberValue::Integer(value) = (NumberSpec::Integer {
            minimum: i128::from(minimum),
            maximum: i128::from(maximum),
        })
        .parse(text)?
        else {
            return Err("Enter a whole number".into());
        };
        i32::try_from(value).map_err(|_| "The value exceeds the control's storage range".into())
    }

    pub fn adjust(self, design: &mut SoundDesign, direction: i32) {
        let step = match self {
            Self::Pitch => 20,
            Self::Sweep => 50,
            Self::Attack | Self::Decay | Self::Release | Self::Duration => 10,
            _ => 1,
        };
        self.set(design, self.value(design) + direction.signum() * step);
    }

    pub fn position(self, design: &SoundDesign) -> f64 {
        let (minimum, maximum) = self.range();
        let value = self.value(design).clamp(minimum, maximum);
        if matches!(self, Self::Pitch | Self::Duration) {
            (f64::from(value) / f64::from(minimum)).ln()
                / (f64::from(maximum) / f64::from(minimum)).ln()
        } else {
            f64::from(value - minimum) / f64::from(maximum - minimum)
        }
    }

    pub fn set_position(self, design: &mut SoundDesign, numerator: u16, denominator: u16) {
        let ratio = if denominator == 0 {
            0.0
        } else {
            f64::from(numerator.min(denominator)) / f64::from(denominator)
        };
        let (minimum, maximum) = self.range();
        let value = if matches!(self, Self::Pitch | Self::Duration) {
            f64::from(minimum) * (f64::from(maximum) / f64::from(minimum)).powf(ratio)
        } else {
            f64::from(minimum) + ratio * f64::from(maximum - minimum)
        };
        self.set(design, value.round() as i32);
    }

    pub fn display(self, design: &SoundDesign) -> String {
        let value = self.value(design);
        match self {
            Self::Waveform => match design.waveform {
                Waveform::Sine => "Sine",
                Waveform::Triangle => "Triangle",
                Waveform::Saw => "Saw",
                Waveform::Square => "Square",
            }
            .into(),
            Self::Pitch => format!("{value} Hz"),
            Self::Sweep => format!("{value:+} cents"),
            Self::Attack | Self::Decay | Self::Release | Self::Duration => format!("{value} ms"),
            Self::PulseRate => format!("{:.1} Hz", f64::from(value) / 10.0),
            Self::Harmony => format!("{value:+} semitones"),
            _ => format!("{value}%"),
        }
    }
}

#[derive(Debug)]
pub struct SoundStudio {
    pub(crate) identity: std::sync::Arc<()>,
    pub draft: SoundSettings,
    pub preview: Vec<WaveformColumn>,
}

impl SoundStudio {
    pub fn new(mut draft: SoundSettings) -> Self {
        draft.design = draft.design.normalized();
        let preview = ilium_sound::waveform_preview(&draft.design, 120);
        Self {
            identity: std::sync::Arc::new(()),
            draft,
            preview,
        }
    }

    pub fn changed(&mut self) {
        self.draft.design = self.draft.design.normalized();
        self.preview = ilium_sound::waveform_preview(&self.draft.design, 120);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_studio_entries_preserve_non_step_values_and_reject_invalid_input() {
        assert_eq!(SoundControl::Pitch.parse_number("0731"), Ok(731));
        assert_eq!(SoundControl::Sweep.parse_number("-173"), Ok(-173));
        assert_eq!(SoundControl::PulseRate.parse_number("1.7"), Ok(17));
        assert_eq!(SoundControl::PulseRate.parse_number("16.0"), Ok(160));
        for text in ["1.75", "NaN", "inf", "16.1", "-0.1"] {
            assert!(
                SoundControl::PulseRate.parse_number(text).is_err(),
                "{text}"
            );
        }
        assert!(SoundControl::Waveform.parse_number("1").is_err());
        for control in SoundControl::ALL
            .into_iter()
            .filter(|value| *value != SoundControl::Waveform)
        {
            let (minimum, maximum) = control.range();
            let mut design = SoundDesign::default();
            for value in [minimum, maximum] {
                control.set(&mut design, value);
                assert_eq!(
                    control.parse_number(&control.number_text(&design)),
                    Ok(value)
                );
            }
            assert!(control
                .parse_number("99999999999999999999999999999999999999999")
                .is_err());
        }
    }

    #[test]
    fn every_control_can_reach_its_entire_safe_range() {
        for control in SoundControl::ALL {
            let mut design = SoundDesign::default();
            let (minimum, maximum) = control.range();
            control.set_position(&mut design, 0, 100);
            assert_eq!(control.value(&design), minimum);
            control.set_position(&mut design, 100, 100);
            assert_eq!(control.value(&design), maximum);
            control.set(&mut design, i32::MIN);
            assert_eq!(control.value(&design), minimum);
            control.set(&mut design, i32::MAX);
            assert_eq!(control.value(&design), maximum);
        }
    }

    #[test]
    fn editing_a_slider_updates_the_audio_and_cached_preview() {
        let mut studio = SoundStudio::new(SoundSettings::default());
        let before = ilium_sound::render_pcm(&studio.draft.design);
        let preview = studio.preview.clone();
        SoundControl::Pitch.adjust(&mut studio.draft.design, 1);
        studio.changed();
        assert_ne!(ilium_sound::render_pcm(&studio.draft.design), before);
        assert_ne!(studio.preview, preview);
    }
}
