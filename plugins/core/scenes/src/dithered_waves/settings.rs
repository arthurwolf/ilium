//! Settings of the `dithered-waves` scene, read from the `settings` object of the scene spec.
//!
//! The keys, ranges and defaults are the ones the manifest's `settings_schema` declares (a
//! unit test compares the two), so the host shows exactly the rows this module understands.
//! Fractional quantities are percentages (`wave_frequency: 300` is 3.0 crests per screen
//! height). Colour, palette, brightness shaping and the shared dither stay the host's.

use serde_json::Value;

/// Threshold pattern used to turn the smooth wave field into dots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WavePattern {
    /// Coarsest regular lattice, 4 gray steps.
    Bayer2,
    /// Classic crosshatch lattice, 16 steps.
    #[default]
    Bayer4,
    /// Finest regular lattice, 64 steps.
    Bayer8,
    /// A fixed irregular stipple tile (still identical on every frame).
    Noise,
}

impl WavePattern {
    /// Every matrix with its `enum` string of the schema.
    pub const ALL: [(Self, &'static str); 4] = [
        (Self::Bayer2, "bayer2"),
        (Self::Bayer4, "bayer4"),
        (Self::Bayer8, "bayer8"),
        (Self::Noise, "noise"),
    ];

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(_, candidate)| *candidate == name)
            .map(|(matrix, _)| *matrix)
    }
}

/// The settings of the `dithered-waves` scene, as the manifest schema declares them.
#[derive(Debug, Clone, PartialEq)]
pub struct DitheredWavesSettings {
    /// Wave crests per screen height in percent, 100..=800. Default 300.
    pub wave_frequency: u32,
    /// How strongly noise warps the bands, percent, 0..=100. Default 30.
    pub wave_amplitude: u32,
    /// Drift speed in percent of the base rate, 0..=200. Default 50.
    pub wave_speed: u32,
    /// The dot lattice the waves are cut with.
    pub pattern: WavePattern,
    /// Gray levels of the dither, 2..=6. 2 is pure black and white dots.
    pub levels: u32,
    /// Edge length of one dither pixel in Braille dots, 1..=4. Default 1.
    pub pixel_size: u32,
    /// Peak dot intensity in percent, 10..=100. Default 35.
    pub intensity: u32,
    /// Band sharpness in percent, 50..=300. Default 150.
    pub band_sharpness: u32,
    /// Threshold shift in percent, -30..=30. Positive adds dots. Default 0.
    pub bias: i32,
    /// Adds a faint ripple around a slowly wandering point. Default off.
    pub ripple: bool,
    /// Selects one of many different wave layouts, 0..=9999. Default 1.
    pub seed: u32,
}

impl Default for DitheredWavesSettings {
    fn default() -> Self {
        Self {
            wave_frequency: 300,
            wave_amplitude: 30,
            wave_speed: 50,
            pattern: WavePattern::Bayer4,
            levels: 2,
            pixel_size: 1,
            intensity: 35,
            band_sharpness: 150,
            bias: 0,
            ripple: false,
            seed: 1,
        }
    }
}

pub const WAVE_FREQUENCY: (u32, u32) = (100, 800);
pub const WAVE_AMPLITUDE: (u32, u32) = (0, 100);
pub const WAVE_SPEED: (u32, u32) = (0, 200);
pub const LEVELS: (u32, u32) = (2, 6);
pub const PIXEL_SIZE: (u32, u32) = (1, 4);
pub const INTENSITY: (u32, u32) = (10, 100);
pub const BAND_SHARPNESS: (u32, u32) = (50, 300);
pub const BIAS: (i32, i32) = (-30, 30);
pub const SEED: (u32, u32) = (0, 9999);

fn clamp((min, max): (u32, u32), value: u32) -> u32 {
    value.clamp(min, max)
}

/// The settings object of a scene spec: the `settings` member when the spec has one (the
/// host's spec), else the spec itself (a bare settings object).
fn settings_object(spec: &Value) -> &Value {
    match spec.get("settings") {
        Some(settings) if settings.is_object() => settings,
        _ => spec,
    }
}

fn number(object: &Value, key: &str) -> Option<i64> {
    let value = object.get(key)?;
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|float| float.round() as i64))
}

fn unsigned(object: &Value, key: &str, default: u32) -> u32 {
    number(object, key).map_or(default, |value| value.clamp(0, i64::from(u32::MAX)) as u32)
}

impl DitheredWavesSettings {
    /// The settings of a spec or settings object: a missing, mistyped or unknown key keeps
    /// the default, and every value is clamped into its range.
    pub fn from_spec(spec: &Value) -> Self {
        let object = settings_object(spec);
        let defaults = Self::default();
        Self {
            wave_frequency: unsigned(object, "wave_frequency", defaults.wave_frequency),
            wave_amplitude: unsigned(object, "wave_amplitude", defaults.wave_amplitude),
            wave_speed: unsigned(object, "wave_speed", defaults.wave_speed),
            pattern: object
                .get("pattern")
                .and_then(Value::as_str)
                .and_then(WavePattern::from_name)
                .unwrap_or(defaults.pattern),
            levels: unsigned(object, "levels", defaults.levels),
            pixel_size: unsigned(object, "pixel_size", defaults.pixel_size),
            intensity: unsigned(object, "intensity", defaults.intensity),
            band_sharpness: unsigned(object, "band_sharpness", defaults.band_sharpness),
            bias: number(object, "bias").map_or(defaults.bias, |value| {
                value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
            }),
            ripple: object
                .get("ripple")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.ripple),
            seed: unsigned(object, "seed", defaults.seed),
        }
        .normalized()
    }

    /// Every value clamped into its range.
    pub fn normalized(&self) -> Self {
        Self {
            wave_frequency: clamp(WAVE_FREQUENCY, self.wave_frequency),
            wave_amplitude: clamp(WAVE_AMPLITUDE, self.wave_amplitude),
            wave_speed: clamp(WAVE_SPEED, self.wave_speed),
            levels: clamp(LEVELS, self.levels),
            pixel_size: clamp(PIXEL_SIZE, self.pixel_size),
            intensity: clamp(INTENSITY, self.intensity),
            band_sharpness: clamp(BAND_SHARPNESS, self.band_sharpness),
            bias: self.bias.clamp(BIAS.0, BIAS.1),
            seed: clamp(SEED, self.seed),
            ..self.clone()
        }
    }
}
