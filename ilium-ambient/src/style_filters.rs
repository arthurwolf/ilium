//! Colour filters: true colour transforms applied to every shaded cell after
//! the palette and tone adjustments, on top of (and independent from) the
//! style presets. A filter works on linear-ish 0..=1 channels and is blended
//! with the unfiltered colour by a strength percentage.

use serde::{Deserialize, Serialize};

type Channels = [f32; 3];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    #[default]
    None,
    Invert,
    Red,
    Green,
    Blue,
    Cyan,
    Magenta,
    Yellow,
    Amber,
    Sepia,
    Cyanotype,
    NightVision,
    Infrared,
    Thermal,
    Cool,
    Warm,
    Moonlight,
    Candlelight,
    Dusk,
    Dawn,
    Pastel,
    Faded,
    Matte,
    Vivid,
    Noir,
    Silver,
    Solarize,
    Posterize,
    GameBoy,
    CrossProcess,
    BleachBypass,
    TealOrange,
    DuotoneBlueOrange,
    DuotonePinkTeal,
    DuotonePurpleGold,
    Hologram,
    NightShift,
    SwapRedBlue,
    RotateChannels,
    Ice,
    Fire,
    Mint,
    Rose,
    Lavender,
    Vintage,
    Polaroid,
}

impl Filter {
    pub const ALL: [Self; 46] = [
        Self::None,
        Self::Invert,
        Self::Red,
        Self::Green,
        Self::Blue,
        Self::Cyan,
        Self::Magenta,
        Self::Yellow,
        Self::Amber,
        Self::Sepia,
        Self::Cyanotype,
        Self::NightVision,
        Self::Infrared,
        Self::Thermal,
        Self::Cool,
        Self::Warm,
        Self::Moonlight,
        Self::Candlelight,
        Self::Dusk,
        Self::Dawn,
        Self::Pastel,
        Self::Faded,
        Self::Matte,
        Self::Vivid,
        Self::Noir,
        Self::Silver,
        Self::Solarize,
        Self::Posterize,
        Self::GameBoy,
        Self::CrossProcess,
        Self::BleachBypass,
        Self::TealOrange,
        Self::DuotoneBlueOrange,
        Self::DuotonePinkTeal,
        Self::DuotonePurpleGold,
        Self::Hologram,
        Self::NightShift,
        Self::SwapRedBlue,
        Self::RotateChannels,
        Self::Ice,
        Self::Fire,
        Self::Mint,
        Self::Rose,
        Self::Lavender,
        Self::Vintage,
        Self::Polaroid,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Invert => "Invert",
            Self::Red => "Red filter",
            Self::Green => "Green filter",
            Self::Blue => "Blue filter",
            Self::Cyan => "Cyan filter",
            Self::Magenta => "Magenta filter",
            Self::Yellow => "Yellow filter",
            Self::Amber => "Amber filter",
            Self::Sepia => "Sepia",
            Self::Cyanotype => "Cyanotype",
            Self::NightVision => "Night vision",
            Self::Infrared => "Infrared",
            Self::Thermal => "Thermal camera",
            Self::Cool => "Cool",
            Self::Warm => "Warm",
            Self::Moonlight => "Moonlight",
            Self::Candlelight => "Candlelight",
            Self::Dusk => "Dusk",
            Self::Dawn => "Dawn",
            Self::Pastel => "Pastel",
            Self::Faded => "Faded film",
            Self::Matte => "Matte",
            Self::Vivid => "Vivid pop",
            Self::Noir => "Noir",
            Self::Silver => "Silver",
            Self::Solarize => "Solarize",
            Self::Posterize => "Posterize",
            Self::GameBoy => "Game Boy",
            Self::CrossProcess => "Cross process",
            Self::BleachBypass => "Bleach bypass",
            Self::TealOrange => "Teal and orange",
            Self::DuotoneBlueOrange => "Duotone blue-orange",
            Self::DuotonePinkTeal => "Duotone pink-teal",
            Self::DuotonePurpleGold => "Duotone purple-gold",
            Self::Hologram => "Hologram",
            Self::NightShift => "Night shift (no blue)",
            Self::SwapRedBlue => "Swap red and blue",
            Self::RotateChannels => "Rotate channels",
            Self::Ice => "Ice",
            Self::Fire => "Fire",
            Self::Mint => "Mint",
            Self::Rose => "Rose",
            Self::Lavender => "Lavender",
            Self::Vintage => "Vintage",
            Self::Polaroid => "Polaroid",
        }
    }

    pub fn labels() -> Vec<&'static str> {
        Self::ALL.iter().map(|filter| filter.label()).collect()
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }

    /// The filtered colour at full strength.
    fn transform(self, c: Channels) -> Channels {
        let l = luma(c);
        match self {
            Self::None => c,
            Self::Invert => c.map(|v| 1.0 - v),
            Self::Red => tint(c, [1.0, 0.1, 0.1]),
            Self::Green => tint(c, [0.1, 1.0, 0.2]),
            Self::Blue => tint(c, [0.15, 0.3, 1.0]),
            Self::Cyan => tint(c, [0.1, 0.95, 1.0]),
            Self::Magenta => tint(c, [1.0, 0.15, 0.9]),
            Self::Yellow => tint(c, [1.0, 0.95, 0.1]),
            Self::Amber => tint(c, [1.0, 0.62, 0.05]),
            Self::Sepia => matrix(
                c,
                [
                    [0.393, 0.769, 0.189],
                    [0.349, 0.686, 0.168],
                    [0.272, 0.534, 0.131],
                ],
            ),
            Self::Cyanotype => ramp3(l, [0.02, 0.07, 0.18], [0.35, 0.62, 0.78], [0.93, 0.97, 1.0]),
            Self::NightVision => {
                let boosted = (l * 1.35).min(1.0);
                [boosted * 0.2, boosted, boosted * 0.25]
            }
            Self::Infrared => [l.powf(0.7), l * 0.25, (1.0 - l) * 0.7 + c[2] * 0.3],
            Self::Thermal => thermal(l),
            Self::Cool => [c[0] * 0.82, c[1] * 0.96, (c[2] * 1.12 + 0.03).min(1.0)],
            Self::Warm => [(c[0] * 1.12 + 0.03).min(1.0), c[1] * 1.0, c[2] * 0.8],
            Self::Moonlight => ramp3(l, [0.02, 0.04, 0.10], [0.45, 0.55, 0.78], [0.9, 0.94, 1.0]),
            Self::Candlelight => ramp3(l, [0.08, 0.02, 0.0], [0.85, 0.45, 0.12], [1.0, 0.9, 0.6]),
            Self::Dusk => ramp3(l, [0.06, 0.03, 0.16], [0.62, 0.32, 0.5], [0.98, 0.72, 0.5]),
            Self::Dawn => ramp3(l, [0.12, 0.1, 0.25], [0.9, 0.55, 0.55], [1.0, 0.92, 0.7]),
            Self::Pastel => c.map(|v| 0.55 + v * 0.45),
            Self::Faded => c.map(|v| 0.12 + v * 0.74),
            Self::Matte => c.map(|v| 0.08 + v * 0.8),
            Self::Vivid => {
                let boosted = c.map(|v| l + (v - l) * 1.7);
                boosted.map(|v| ((v - 0.5) * 1.15 + 0.5).clamp(0.0, 1.0))
            }
            Self::Noir => {
                let hard = ((l - 0.5) * 1.6 + 0.5).clamp(0.0, 1.0);
                [hard; 3]
            }
            Self::Silver => ramp3(l, [0.05, 0.06, 0.08], [0.55, 0.58, 0.64], [0.97, 0.98, 1.0]),
            Self::Solarize => c
                .map(|v| if v > 0.5 { 1.0 - v } else { v })
                .map(|v| v * 2.0),
            Self::Posterize => c.map(|v| (v * 3.0).round() / 3.0),
            Self::GameBoy => {
                const SHADES: [Channels; 4] = [
                    [0.06, 0.22, 0.06],
                    [0.19, 0.38, 0.19],
                    [0.55, 0.67, 0.06],
                    [0.61, 0.74, 0.06],
                ];
                SHADES[((l * 4.0) as usize).min(3)]
            }
            Self::CrossProcess => [
                (c[0] * c[0] * 1.2 + c[0] * 0.1).min(1.0),
                c[1].powf(0.85),
                (c[2] * 0.7 + 0.15).min(1.0),
            ],
            Self::BleachBypass => {
                let overlay = c.map(|v| {
                    if l < 0.5 {
                        2.0 * v * l
                    } else {
                        1.0 - 2.0 * (1.0 - v) * (1.0 - l)
                    }
                });
                [
                    (overlay[0] + l) * 0.5,
                    (overlay[1] + l) * 0.5,
                    (overlay[2] + l) * 0.5,
                ]
            }
            Self::TealOrange => ramp3(l, [0.02, 0.18, 0.22], [0.5, 0.5, 0.48], [1.0, 0.62, 0.25]),
            Self::DuotoneBlueOrange => duotone(l, [0.03, 0.08, 0.35], [1.0, 0.6, 0.2]),
            Self::DuotonePinkTeal => duotone(l, [0.0, 0.3, 0.35], [1.0, 0.45, 0.7]),
            Self::DuotonePurpleGold => duotone(l, [0.17, 0.05, 0.35], [1.0, 0.82, 0.3]),
            Self::Hologram => {
                let shimmer = 0.5 + 0.5 * (c[0] * 6.0 + c[2] * 4.0).sin();
                [
                    l * 0.35,
                    (l * 0.85 + shimmer * 0.15).min(1.0),
                    (l + 0.2).min(1.0),
                ]
            }
            Self::NightShift => [c[0], c[1] * 0.78, c[2] * 0.25],
            Self::SwapRedBlue => [c[2], c[1], c[0]],
            Self::RotateChannels => [c[1], c[2], c[0]],
            Self::Ice => ramp3(l, [0.02, 0.1, 0.22], [0.5, 0.82, 0.95], [0.97, 1.0, 1.0]),
            Self::Fire => ramp3(l, [0.1, 0.0, 0.0], [0.95, 0.3, 0.02], [1.0, 0.92, 0.4]),
            Self::Mint => ramp3(l, [0.05, 0.15, 0.12], [0.5, 0.88, 0.75], [0.92, 1.0, 0.95]),
            Self::Rose => ramp3(l, [0.18, 0.05, 0.1], [0.9, 0.5, 0.6], [1.0, 0.9, 0.9]),
            Self::Lavender => ramp3(l, [0.12, 0.08, 0.22], [0.65, 0.55, 0.88], [0.95, 0.92, 1.0]),
            Self::Vintage => {
                let toned = matrix(c, [[0.5, 0.35, 0.15], [0.3, 0.55, 0.15], [0.25, 0.3, 0.45]]);
                toned.map(|v| 0.1 + v * 0.8)
            }
            Self::Polaroid => [
                (c[0] * 1.1 + 0.05).min(1.0),
                (c[1] * 1.02 + 0.02).min(1.0),
                (c[2] * 0.9 + 0.1).min(1.0),
            ]
            .map(|v| 0.06 + v * 0.88),
        }
    }

    /// Filter `color` and blend with the original by `strength_percent` (0..=100).
    pub fn apply(self, color: [u8; 3], strength_percent: u16) -> [u8; 3] {
        if self == Self::None || strength_percent == 0 {
            return color;
        }
        let source = color.map(|channel| f32::from(channel) / 255.0);
        let filtered = self.transform(source);
        let fraction = f32::from(strength_percent.min(100)) / 100.0;
        let mut output = [0u8; 3];
        for index in 0..3 {
            let value = source[index] + (filtered[index] - source[index]) * fraction;
            output[index] = (value * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        output
    }
}

fn luma(c: Channels) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn matrix(c: Channels, rows: [[f32; 3]; 3]) -> Channels {
    rows.map(|row| (row[0] * c[0] + row[1] * c[1] + row[2] * c[2]).clamp(0.0, 1.0))
}

/// A colour gel: multiplies each channel by the gel colour, keeping the
/// brightness of the original.
fn tint(c: Channels, gel: Channels) -> Channels {
    let l = luma(c);
    let gel_luma = luma(gel).max(0.05);
    gel.map(|g| (l * g / gel_luma).clamp(0.0, 1.0))
}

fn mix(a: Channels, b: Channels, fraction: f32) -> Channels {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * fraction)
}

fn duotone(l: f32, dark: Channels, light: Channels) -> Channels {
    mix(dark, light, l.clamp(0.0, 1.0))
}

fn ramp3(l: f32, dark: Channels, middle: Channels, light: Channels) -> Channels {
    let l = l.clamp(0.0, 1.0);
    if l < 0.5 {
        mix(dark, middle, l * 2.0)
    } else {
        mix(middle, light, (l - 0.5) * 2.0)
    }
}

fn thermal(l: f32) -> Channels {
    const STOPS: [Channels; 5] = [
        [0.0, 0.0, 0.2],
        [0.4, 0.0, 0.55],
        [0.9, 0.15, 0.2],
        [1.0, 0.75, 0.1],
        [1.0, 1.0, 0.85],
    ];
    let scaled = l.clamp(0.0, 1.0) * 4.0;
    let index = (scaled.floor() as usize).min(3);
    mix(STOPS[index], STOPS[index + 1], scaled - index as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_and_zero_strength_are_identity() {
        assert_eq!(Filter::None.apply([10, 20, 30], 100), [10, 20, 30]);
        assert_eq!(Filter::Invert.apply([10, 20, 30], 0), [10, 20, 30]);
    }

    #[test]
    fn invert_flips_and_red_filter_is_red() {
        assert_eq!(Filter::Invert.apply([0, 100, 255], 100), [255, 155, 0]);
        let red = Filter::Red.apply([128, 128, 128], 100);
        assert!(red[0] > red[1] && red[0] > red[2], "{red:?}");
    }

    #[test]
    fn strength_blends_between_original_and_filtered() {
        let half = Filter::Invert.apply([0, 0, 0], 50);
        assert!((120..=135).contains(&half[0]), "{half:?}");
    }

    #[test]
    fn labels_are_unique_and_every_filter_runs_on_extremes() {
        let mut labels = Filter::labels();
        let count = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), count);
        for filter in Filter::ALL {
            assert_eq!(Filter::from_index(filter.index()), Some(filter));
            for color in [[0, 0, 0], [255, 255, 255], [200, 30, 90]] {
                filter.apply(color, 100);
            }
        }
    }
}
