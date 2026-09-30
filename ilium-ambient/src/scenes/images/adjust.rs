//! Colour adjustment, moved unchanged from the client's former static image
//! renderer: brightness, contrast, saturation, preset tints and the hue tint.

use super::settings::{ImagePreset, ImagesSettings};

/// A dot is dark enough to stay unlit at or below this raw luminance.
pub const LUMINANCE_FLOOR: f32 = 0.035;
/// The old renderer lit a dot when its `level` exceeded this value.
pub const LEGACY_LIT_LEVEL: f32 = 0.08;
/// The raster intensity reaches 1.0 at this `level`. Twice the legacy
/// threshold, so the old hard cut-off sits at intensity 0.5 and the host's
/// dither reproduces the tones around it instead of a flat on/off mask. It is
/// also the level of a mid-grey pixel at the default opacity and intensity.
pub const LEVEL_FULL_SCALE: f32 = LEGACY_LIT_LEVEL * 2.0;

/// The result of adjusting one raw pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adjusted {
    /// Adjusted colour, not clamped (may be outside 0..1).
    pub color: [f32; 3],
    /// Mean tone (contrast, saturation and tints applied, brightness not)
    /// times opacity and intensity, clamped 0..1. The old renderer also
    /// multiplied by brightness here; brightness now only darkens the dot
    /// colours, so a dim setting keeps the full tonal range of dots instead of
    /// starving the dither.
    pub level: f32,
    /// Luminance of the raw pixel.
    pub luminance: f32,
}

impl Adjusted {
    #[cfg(test)]
    /// Old rule: lit when the level exceeds 0.08 and the pixel is not black.
    pub fn is_lit_legacy(&self) -> bool {
        self.level > LEGACY_LIT_LEVEL && self.luminance > LUMINANCE_FLOOR
    }

    /// Dot intensity for the host's threshold/dither, 0..=1.
    pub fn dot_intensity(&self) -> f32 {
        if self.luminance > LUMINANCE_FLOOR {
            (self.level / LEVEL_FULL_SCALE).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The colour as the old renderer accumulated it: clamp, scale, truncate.
    pub fn color_bytes(&self) -> [u32; 3] {
        [
            (self.color[0].clamp(0.0, 1.0) * 255.0) as u32,
            (self.color[1].clamp(0.0, 1.0) * 255.0) as u32,
            (self.color[2].clamp(0.0, 1.0) * 255.0) as u32,
        ]
    }
}

/// Preset multipliers: (brightness, contrast, saturation, intensity).
pub fn preset_factors(preset: ImagePreset) -> (f32, f32, f32, f32) {
    match preset {
        ImagePreset::Dimmed => (0.72, 0.82, 1.0, 0.58),
        ImagePreset::Vivid => (1.0, 1.28, 1.0, 0.82),
        ImagePreset::Monochrome => (0.72, 0.92, 0.0, 0.62),
        ImagePreset::Cool => (0.72, 0.86, 1.0, 0.62),
        ImagePreset::Warm => (0.72, 0.86, 1.0, 0.62),
    }
}

/// Precomputed adjustment of one settings snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adjustment {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    intensity: f32,
    opacity: f32,
    hue_tint: f32,
    preset: ImagePreset,
}

impl Adjustment {
    pub fn from_settings(settings: &ImagesSettings) -> Self {
        let factors = preset_factors(settings.preset);
        Self {
            brightness: f32::from(settings.brightness_percent) / 100.0 * factors.0,
            contrast: f32::from(settings.contrast_percent) / 100.0 * factors.1,
            saturation: f32::from(settings.saturation_percent) / 100.0 * factors.2,
            intensity: f32::from(settings.intensity_percent.min(100)) / 100.0 * factors.3,
            opacity: f32::from(settings.opacity_percent.min(100)) / 100.0,
            hue_tint: (f32::from(settings.hue_degrees) - 180.0) / 180.0,
            preset: settings.preset,
        }
    }

    /// Adjust a raw pixel (channels 0..=1).
    pub fn apply(&self, raw: [f32; 3]) -> Adjusted {
        let [mut red, mut green, mut blue] = raw;
        let luminance = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
        red = ((red - 0.5) * self.contrast + 0.5) * self.brightness;
        green = ((green - 0.5) * self.contrast + 0.5) * self.brightness;
        blue = ((blue - 0.5) * self.contrast + 0.5) * self.brightness;
        let gray = (red + green + blue) / 3.0;
        red = gray + (red - gray) * self.saturation;
        green = gray + (green - gray) * self.saturation;
        blue = gray + (blue - gray) * self.saturation;
        let (mut red_tint, mut blue_tint) = match self.preset {
            ImagePreset::Cool => (-0.03, 0.08),
            ImagePreset::Warm => (0.08, -0.05),
            _ => (0.0, 0.0),
        };
        red_tint += self.hue_tint * 0.08;
        blue_tint -= self.hue_tint * 0.08;
        red += red_tint;
        blue += blue_tint;
        // Tone: the same pipeline without the brightness factor. Saturation
        // keeps the channel mean, so only contrast and the tints matter.
        let contrasted_mean = ((raw[0] + raw[1] + raw[2]) / 3.0 - 0.5) * self.contrast + 0.5;
        let tone = contrasted_mean + (red_tint + blue_tint) / 3.0;
        let level = (tone * self.opacity * self.intensity).clamp(0.0, 1.0);
        Adjusted {
            color: [red, green, blue],
            level,
            luminance,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_reproduce_the_old_adjustment_numbers() {
        let adjustment = Adjustment::from_settings(&ImagesSettings::default());
        assert!((adjustment.brightness - 0.72 * 0.72).abs() < 1e-6);
        assert!((adjustment.contrast - 0.82 * 0.82).abs() < 1e-6);
        assert!((adjustment.saturation - 0.82).abs() < 1e-6);
        assert!((adjustment.intensity - 0.48 * 0.58).abs() < 1e-6);
        assert!((adjustment.opacity - 0.58).abs() < 1e-6);
        assert!(adjustment.hue_tint.abs() < 1e-6);
    }

    #[test]
    fn vivid_preset_numbers_match_the_old_table() {
        let settings = ImagesSettings {
            preset: ImagePreset::Vivid,
            ..ImagesSettings::default()
        };
        let adjustment = Adjustment::from_settings(&settings);
        assert!((adjustment.brightness - 0.72).abs() < 1e-6);
        assert!((adjustment.contrast - 0.82 * 1.28).abs() < 1e-6);
        assert!((adjustment.intensity - 0.48 * 0.82).abs() < 1e-6);
    }

    #[test]
    fn hand_computed_pixel_matches() {
        // Vivid, all sliders at 100 %, neutral hue: brightness 1.0,
        // contrast 1.28, saturation 1.0, intensity 0.82, opacity 1.0.
        let settings = ImagesSettings {
            preset: ImagePreset::Vivid,
            brightness_percent: 100,
            contrast_percent: 100,
            saturation_percent: 100,
            intensity_percent: 100,
            opacity_percent: 100,
            ..ImagesSettings::default()
        };
        let adjusted = Adjustment::from_settings(&settings).apply([0.8, 0.4, 0.2]);
        // channel = ((c - 0.5) * 1.28 + 0.5) * 1.0
        let red = (0.3 * 1.28 + 0.5) as f32;
        let green = (-0.1 * 1.28 + 0.5) as f32;
        let blue = (-0.3 * 1.28 + 0.5) as f32;
        assert!((adjusted.color[0] - red).abs() < 1e-5);
        assert!((adjusted.color[1] - green).abs() < 1e-5);
        assert!((adjusted.color[2] - blue).abs() < 1e-5);
        let level = ((red + green + blue) / 3.0 * 0.82).clamp(0.0, 1.0);
        assert!((adjusted.level - level).abs() < 1e-5);
        let luminance = 0.2126 * 0.8 + 0.7152 * 0.4 + 0.0722 * 0.2;
        assert!((adjusted.luminance - luminance).abs() < 1e-5);
        assert_eq!(
            adjusted.color_bytes(),
            [
                (red * 255.0) as u32,
                (green * 255.0) as u32,
                (blue * 255.0) as u32
            ]
        );
    }

    #[test]
    fn hue_tint_and_cool_warm_offsets_apply() {
        let base = ImagesSettings {
            brightness_percent: 100,
            contrast_percent: 100,
            saturation_percent: 100,
            preset: ImagePreset::Vivid,
            ..ImagesSettings::default()
        };
        let neutral = Adjustment::from_settings(&base).apply([0.5, 0.5, 0.5]);
        // preset brightness 1.0, contrast irrelevant at 0.5 -> 0.5
        assert!((neutral.color[0] - 0.5).abs() < 1e-6);
        let warm_hue = ImagesSettings {
            hue_degrees: 360,
            ..base.clone()
        };
        let tinted = Adjustment::from_settings(&warm_hue).apply([0.5, 0.5, 0.5]);
        assert!((tinted.color[0] - 0.58).abs() < 1e-6);
        assert!((tinted.color[2] - 0.42).abs() < 1e-6);
        let cool = ImagesSettings {
            preset: ImagePreset::Cool,
            brightness_percent: 100,
            contrast_percent: 100,
            ..base
        };
        let cooled = Adjustment::from_settings(&cool).apply([0.5, 0.5, 0.5]);
        // brightness 0.72: base 0.36 then +0.08 blue / -0.03 red
        assert!((cooled.color[2] - 0.44).abs() < 1e-5);
        assert!((cooled.color[0] - 0.33).abs() < 1e-5);
    }

    #[test]
    fn dot_intensity_is_half_at_the_legacy_threshold() {
        let lit = Adjusted {
            color: [0.5; 3],
            level: LEGACY_LIT_LEVEL,
            luminance: 0.5,
        };
        assert!((lit.dot_intensity() - 0.5).abs() < 1e-6);
        assert!(!lit.is_lit_legacy());
        let brighter = Adjusted {
            level: 0.081,
            ..lit
        };
        assert!(brighter.is_lit_legacy() && brighter.dot_intensity() > 0.5);
        let black = Adjusted {
            luminance: 0.01,
            ..lit
        };
        assert_eq!(black.dot_intensity(), 0.0);
    }

    #[test]
    fn white_uses_a_wide_part_of_the_range_in_every_preset() {
        for preset in ImagePreset::ALL {
            let settings = ImagesSettings {
                preset: *preset,
                ..ImagesSettings::default()
            };
            let white = Adjustment::from_settings(&settings).apply([1.0; 3]);
            let dark = Adjustment::from_settings(&settings).apply([0.05; 3]);
            assert!(white.dot_intensity() > dark.dot_intensity());
        }
    }
}
