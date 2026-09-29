//! Deterministic monochrome scenes. Decoration never enters PTY/source state.

use serde::{Deserialize, Serialize};
use std::time::Duration;

mod raster;
mod scenes;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationKind {
    #[default]
    Shoreline,
    MoonlitWater,
    SleepingRidge,
    WindyHillside,
    TeaSteam,
    Kelp,
    StoneCaustics,
    Cloudlets,
    TwoRipples,
    BreathingMountain,
}

impl AnimationKind {
    pub const ALL: [Self; 10] = [
        Self::Shoreline,
        Self::MoonlitWater,
        Self::SleepingRidge,
        Self::WindyHillside,
        Self::TeaSteam,
        Self::Kelp,
        Self::StoneCaustics,
        Self::Cloudlets,
        Self::TwoRipples,
        Self::BreathingMountain,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Shoreline => "Wave washing up sand",
            Self::MoonlitWater => "Moon over moving water",
            Self::SleepingRidge => "Clouds over a sleeping ridge",
            Self::WindyHillside => "Hillside brushed by wind",
            Self::TeaSteam => "Steam above a tea cup",
            Self::Kelp => "Kelp in a slow current",
            Self::StoneCaustics => "Water caustics on stone",
            Self::Cloudlets => "Metaballs becoming cloudlets",
            Self::TwoRipples => "Two gentle wave sources",
            Self::BreathingMountain => "Wireframe mountain breathing",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Shoreline => "A slow diagonal wash, delicate foam and lingering wet sand.",
            Self::MoonlitWater => "A still moon and a broken reflection moving with the water.",
            Self::SleepingRidge => {
                "Broad drifting clouds and valley mist over quiet mountain silhouettes."
            }
            Self::WindyHillside => "Coherent gusts bend rooted grass across a rounded hillside.",
            Self::TeaSteam => "Sparse steam curls rise above a motionless tea cup.",
            Self::Kelp => "Long ribbons follow a slow current above a dark seabed.",
            Self::StoneCaustics => "A stylized web of water light crawls over rounded stones.",
            Self::Cloudlets => "A few soft forms drift, join and separate in empty space.",
            Self::TwoRipples => "Two broad ripple fields meet and form changing curved bands.",
            Self::BreathingMountain => {
                "A sparse perspective mountain mesh rises and settles gently."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherMode {
    #[default]
    Ordered,
    Stippled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationSettings {
    pub enabled: bool,
    pub kind: AnimationKind,
    pub speed_percent: u16,
    pub density_percent: u16,
    pub dither: DitherMode,
}

impl Default for AnimationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: AnimationKind::default(),
            speed_percent: 100,
            density_percent: 60,
            dither: DitherMode::default(),
        }
    }
}

impl AnimationSettings {
    pub fn normalized(self) -> Self {
        Self {
            speed_percent: self.speed_percent.clamp(25, 200),
            density_percent: self.density_percent.clamp(25, 100),
            ..self
        }
    }
}

#[derive(Debug, Default)]
pub struct AnimationFrame {
    width: u16,
    height: u16,
    raster: raster::Raster,
    cells: Vec<u8>,
    last_render: Option<(AnimationSettings, u16, u16, Duration)>,
}

impl AnimationFrame {
    pub fn width(&self) -> u16 {
        self.width
    }
    pub fn height(&self) -> u16 {
        self.height
    }

    fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.raster
            .resize(usize::from(width) * 2, usize::from(height) * 4);
        self.cells
            .resize(usize::from(width) * usize::from(height), 0);
    }

    /// `enabled` is deliberately a compositor concern: Settings previews also
    /// render disabled scenes. The same pure renderer supplies both surfaces.
    pub fn render(
        &mut self,
        settings: &AnimationSettings,
        width: u16,
        height: u16,
        elapsed: Duration,
    ) {
        let settings = settings.normalized();
        let key = (settings, width, height, elapsed);
        if self.last_render == Some(key) {
            return;
        }
        self.resize(width, height);
        self.last_render = Some(key);
        if width == 0 || height == 0 {
            return;
        }
        let seconds = elapsed.as_secs_f64() * f64::from(settings.speed_percent) / 100.0;
        scenes::render(&mut self.raster, settings.kind, seconds);
        self.pack(settings.density_percent, settings.dither);
    }

    pub fn glyph(&self, x: u16, y: u16) -> char {
        if x >= self.width || y >= self.height {
            return ' ';
        }
        let bits = self.cells[usize::from(y) * usize::from(self.width) + usize::from(x)];
        if bits == 0 {
            ' '
        } else {
            char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' ')
        }
    }

    fn pack(&mut self, density_percent: u16, dither: DitherMode) {
        // Braille uses column-major 1,2,3,7 / 4,5,6,8 dot numbering.
        const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
        let density = f32::from(density_percent) / 100.0;
        for y in 0..usize::from(self.height) {
            for x in 0..usize::from(self.width) {
                let mut cell = 0;
                for (dy, row) in BITS.iter().enumerate() {
                    for (dx, bit) in row.iter().enumerate() {
                        let dot_x = x * 2 + dx;
                        let dot_y = y * 4 + dy;
                        let intensity = self.raster.dots[dot_y * self.raster.width + dot_x];
                        if intensity * density > raster::threshold(dot_x, dot_y, dither) {
                            cell |= bit;
                        }
                    }
                }
                self.cells[y * usize::from(self.width) + x] = cell;
            }
        }
    }
}
