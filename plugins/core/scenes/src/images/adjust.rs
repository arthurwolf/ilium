//! The tone the Images scene gives a picture before the shared look is applied: a fixed,
//! quiet "dimmed" treatment (brightness, contrast, saturation, tone level). The user's colour
//! controls (brightness, contrast, hue, saturation, palette, intensity) are the shared look's, so
//! this scene has no rows for them.

/// A dot is dark enough to stay unlit at or below this raw luminance.
pub const LUMINANCE_FLOOR: f32 = 0.035;
/// The old renderer lit a dot when its `level` exceeded this value.
pub const LEGACY_LIT_LEVEL: f32 = 0.08;
/// The raster intensity reaches 1.0 at this `level`. Twice the legacy threshold, so the old hard
/// cut-off sits at intensity 0.5 and the host's dither reproduces the tones around it instead of
/// a flat on/off mask.
pub const LEVEL_FULL_SCALE: f32 = LEGACY_LIT_LEVEL * 2.0;

/// The result of adjusting one raw pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adjusted {
    /// Adjusted colour, not clamped (may be outside 0..1).
    pub color: [f32; 3],
    /// Mean tone (contrast applied, brightness not) times opacity and intensity, clamped 0..1.
    /// Brightness only darkens the dot colours, so a dim look keeps the full tonal range of
    /// dots instead of starving the dither.
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

    /// The colour as bytes: clamp, scale, truncate.
    pub fn color_bytes(&self) -> [u32; 3] {
        [
            (self.color[0].clamp(0.0, 1.0) * 255.0) as u32,
            (self.color[1].clamp(0.0, 1.0) * 255.0) as u32,
            (self.color[2].clamp(0.0, 1.0) * 255.0) as u32,
        ]
    }
}

/// The multipliers of one treatment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adjustment {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    intensity: f32,
    opacity: f32,
}

impl Adjustment {
    /// The scene's treatment: the numbers of the former default look (brightness 72 % times the
    /// dimmed preset's 0.72, contrast 82 % times 0.82, saturation 82 %, intensity 48 % times
    /// 0.58, opacity 58 %).
    pub fn standard() -> Self {
        Self {
            brightness: 0.72 * 0.72,
            contrast: 0.82 * 0.82,
            saturation: 0.82,
            intensity: 0.48 * 0.58,
            opacity: 0.58,
        }
    }

    /// A treatment with known round numbers (contrast 1.28, intensity 0.82, the rest neutral),
    /// so render tests can compute their expectations by hand.
    #[cfg(test)]
    pub fn neutral_for_test() -> Self {
        Self {
            brightness: 1.0,
            contrast: 1.28,
            saturation: 1.0,
            intensity: 0.82,
            opacity: 1.0,
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
        // Tone: the same pipeline without the brightness factor. Saturation keeps the channel
        // mean, so only contrast matters.
        let contrasted_mean = ((raw[0] + raw[1] + raw[2]) / 3.0 - 0.5) * self.contrast + 0.5;
        let level = (contrasted_mean * self.opacity * self.intensity).clamp(0.0, 1.0);
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
    fn the_standard_treatment_keeps_the_numbers_of_the_former_default() {
        let adjustment = Adjustment::standard();
        assert!((adjustment.brightness - 0.72 * 0.72).abs() < 1e-6);
        assert!((adjustment.contrast - 0.82 * 0.82).abs() < 1e-6);
        assert!((adjustment.saturation - 0.82).abs() < 1e-6);
        assert!((adjustment.intensity - 0.48 * 0.58).abs() < 1e-6);
        assert!((adjustment.opacity - 0.58).abs() < 1e-6);
    }

    #[test]
    fn a_hand_computed_pixel_matches() {
        let adjustment = Adjustment {
            brightness: 1.0,
            contrast: 1.28,
            saturation: 1.0,
            intensity: 0.82,
            opacity: 1.0,
        };
        let adjusted = adjustment.apply([0.8, 0.4, 0.2]);
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
    fn a_neutral_grey_stays_neutral() {
        let adjusted = Adjustment::standard().apply([0.5, 0.5, 0.5]);
        assert!((adjusted.color[0] - adjusted.color[1]).abs() < 1e-6);
        assert!((adjusted.color[1] - adjusted.color[2]).abs() < 1e-6);
        assert!((adjusted.color[0] - 0.5 * 0.72 * 0.72).abs() < 1e-6);
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
    fn white_uses_a_wider_part_of_the_range_than_near_black() {
        let adjustment = Adjustment::standard();
        let white = adjustment.apply([1.0; 3]);
        let dark = adjustment.apply([0.05; 3]);
        assert!(white.dot_intensity() > dark.dot_intensity());
    }
}
