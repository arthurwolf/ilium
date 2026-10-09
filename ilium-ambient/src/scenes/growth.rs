//! Deterministic fungal-colony growth for the terminal background.

use crate::control::{Control, ControlValue, SceneSettings};
use crate::scene::{Frame, OccupancyMask, Scene, SceneEnv};
use crate::style::ScenePalette;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrowthPattern {
    Branching,
    Rings,
    Veins,
    Carpet,
}
impl Default for GrowthPattern {
    fn default() -> Self {
        Self::Branching
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrowthColorMode {
    Monochrome,
    Palette,
}
impl Default for GrowthColorMode {
    fn default() -> Self {
        Self::Monochrome
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrowthSettings {
    pub pattern: GrowthPattern,
    pub color_mode: GrowthColorMode,
    pub density: u32,
    pub growth_rate: u32,
    pub branch_angle: u32,
    pub seeds: u32,
    pub reseed_seconds: u32,
    pub reserve_edge: bool,
    pub reserve_width: u32,
    pub mouse_erase: bool,
    pub mouse_radius: u32,
    pub appear_erase: u32,
    pub scroll_erase: u32,
    pub scroll_push: u32,
    pub seed: u32,
    pub frame_rate: u32,
}
impl Default for GrowthSettings {
    fn default() -> Self {
        Self {
            pattern: GrowthPattern::Branching,
            color_mode: GrowthColorMode::Monochrome,
            density: 55,
            growth_rate: 45,
            branch_angle: 35,
            seeds: 5,
            reseed_seconds: 60,
            reserve_edge: true,
            reserve_width: 1,
            mouse_erase: true,
            mouse_radius: 3,
            appear_erase: 100,
            scroll_erase: 50,
            scroll_push: 55,
            seed: 7,
            frame_rate: 20,
        }
    }
}
impl GrowthSettings {
    pub fn normalized(&self) -> Self {
        let mut s = self.clone();
        s.density = s.density.clamp(1, 100);
        s.growth_rate = s.growth_rate.clamp(1, 100);
        s.branch_angle = s.branch_angle.min(100);
        s.seeds = s.seeds.clamp(1, 32);
        s.reseed_seconds = s.reseed_seconds.clamp(1, 3600);
        s.reserve_width = s.reserve_width.min(8);
        s.mouse_radius = s.mouse_radius.min(12);
        s.appear_erase = s.appear_erase.min(100);
        s.scroll_erase = s.scroll_erase.min(100);
        s.scroll_push = s.scroll_push.min(100);
        s.seed = s.seed.min(9999);
        s.frame_rate = s.frame_rate.clamp(5, 30);
        s
    }
    fn slider(
        id: &'static str,
        label: &'static str,
        value: u32,
        range: (u32, u32),
        unit: &'static str,
    ) -> Control {
        Control::slider(
            id,
            label,
            value as i32,
            (range.0 as i32, range.1 as i32, 1),
            unit,
            "Growth control",
        )
    }
}
impl SceneSettings for GrowthSettings {
    fn normalized(&self) -> Self {
        GrowthSettings::normalized(self)
    }

    fn controls(&self) -> Vec<Control> {
        vec![
            Control::choice(
                "pattern",
                "Growth pattern",
                self.pattern as usize,
                &[
                    "Branching hyphae",
                    "Concentric rings",
                    "Veined mat",
                    "Fast carpet",
                ],
                "Morphology of the colony",
            ),
            Control::choice(
                "color_mode",
                "Color mode",
                self.color_mode as usize,
                &["Monochrome", "Palette hues"],
                "Use one tone or palette-derived cell colors",
            ),
            Self::slider("density", "Density", self.density, (1, 100), "%"),
            Self::slider(
                "growth_rate",
                "Growth rate",
                self.growth_rate,
                (1, 100),
                "%",
            ),
            Self::slider(
                "branch_angle",
                "Branch variation",
                self.branch_angle,
                (0, 100),
                "%",
            ),
            Self::slider("seeds", "Initial seeds", self.seeds, (1, 32), ""),
            Self::slider(
                "reseed_seconds",
                "Reseed interval",
                self.reseed_seconds,
                (1, 3600),
                " s",
            ),
            Control::toggle(
                "reserve_edge",
                "Reserve edge",
                self.reserve_edge,
                "Keep an outside rim as a permanent source",
            ),
            Self::slider(
                "reserve_width",
                "Reserve width",
                self.reserve_width,
                (0, 8),
                " cells",
            ),
            Control::toggle(
                "mouse_erase",
                "Mouse erases",
                self.mouse_erase,
                "The pointer clears nearby fungus",
            ),
            Self::slider(
                "mouse_radius",
                "Mouse radius",
                self.mouse_radius,
                (0, 12),
                " cells",
            ),
            Self::slider(
                "appear_erase",
                "Appear erase",
                self.appear_erase,
                (0, 100),
                "%",
            ),
            Self::slider(
                "scroll_erase",
                "Scroll erase",
                self.scroll_erase,
                (0, 100),
                "%",
            ),
            Self::slider(
                "scroll_push",
                "Scroll push",
                self.scroll_push,
                (0, 100),
                "%",
            ),
            Self::slider("seed", "Random seed", self.seed, (0, 9999), ""),
            Self::slider("frame_rate", "Frame rate", self.frame_rate, (5, 30), " fps"),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let old = serde_json::to_string(self).unwrap();
        match id {
            "pattern" => {
                self.pattern = match crate::control::index(&value).unwrap_or(0).min(3) {
                    0 => GrowthPattern::Branching,
                    1 => GrowthPattern::Rings,
                    2 => GrowthPattern::Veins,
                    _ => GrowthPattern::Carpet,
                }
            }
            "color_mode" => {
                self.color_mode = if crate::control::index(&value).unwrap_or(0) == 0 {
                    GrowthColorMode::Monochrome
                } else {
                    GrowthColorMode::Palette
                }
            }
            "reserve_edge" => {
                self.reserve_edge =
                    crate::control::boolean(&value).ok_or("Reserve edge expects toggle")?
            }
            "mouse_erase" => {
                self.mouse_erase =
                    crate::control::boolean(&value).ok_or("Mouse erase expects toggle")?
            }
            "density" => {
                self.density = crate::control::number(&value)
                    .unwrap_or(self.density as i32)
                    .clamp(1, 100) as u32
            }
            "growth_rate" => {
                self.growth_rate = crate::control::number(&value)
                    .unwrap_or(self.growth_rate as i32)
                    .clamp(1, 100) as u32
            }
            "branch_angle" => {
                self.branch_angle = crate::control::number(&value)
                    .unwrap_or(self.branch_angle as i32)
                    .clamp(0, 100) as u32
            }
            "seeds" => {
                self.seeds = crate::control::number(&value)
                    .unwrap_or(self.seeds as i32)
                    .clamp(1, 32) as u32
            }
            "reseed_seconds" => {
                self.reseed_seconds = crate::control::number(&value)
                    .unwrap_or(self.reseed_seconds as i32)
                    .clamp(1, 3600) as u32
            }
            "reserve_width" => {
                self.reserve_width = crate::control::number(&value)
                    .unwrap_or(self.reserve_width as i32)
                    .clamp(0, 8) as u32
            }
            "mouse_radius" => {
                self.mouse_radius = crate::control::number(&value)
                    .unwrap_or(self.mouse_radius as i32)
                    .clamp(0, 12) as u32
            }
            "appear_erase" => {
                self.appear_erase = crate::control::number(&value)
                    .unwrap_or(self.appear_erase as i32)
                    .clamp(0, 100) as u32
            }
            "scroll_erase" => {
                self.scroll_erase = crate::control::number(&value)
                    .unwrap_or(self.scroll_erase as i32)
                    .clamp(0, 100) as u32
            }
            "scroll_push" => {
                self.scroll_push = crate::control::number(&value)
                    .unwrap_or(self.scroll_push as i32)
                    .clamp(0, 100) as u32
            }
            "seed" => {
                self.seed = crate::control::number(&value)
                    .unwrap_or(self.seed as i32)
                    .clamp(0, 9999) as u32
            }
            "frame_rate" => {
                self.frame_rate = crate::control::number(&value)
                    .unwrap_or(self.frame_rate as i32)
                    .clamp(5, 30) as u32
            }
            _ => return Err(format!("Unknown growth control: {id}")),
        }
        Ok(old != serde_json::to_string(self).unwrap())
    }
}

pub struct GrowthScene {
    settings: GrowthSettings,
    // One simulation value per Braille dot: two columns by four rows per terminal cell.
    grid: Vec<f32>,
    previous: OccupancyMask,
    mask: OccupancyMask,
    pointer: Option<[f32; 2]>,
    width: u16,
    height: u16,
    last_time: f32,
    last_reseed: f32,
    palette: ScenePalette,
}
impl GrowthScene {
    pub fn new(settings: &GrowthSettings, env: &SceneEnv) -> Self {
        Self {
            settings: settings.normalized(),
            grid: Vec::new(),
            previous: OccupancyMask::default(),
            mask: OccupancyMask::default(),
            pointer: None,
            width: 0,
            height: 0,
            last_time: 0.0,
            last_reseed: 0.0,
            palette: env.palette.clone(),
        }
    }
    fn hash(mut x: u32) -> u32 {
        x ^= x >> 16;
        x = x.wrapping_mul(0x7feb352d);
        x ^= x >> 15;
        x = x.wrapping_mul(0x846ca68b);
        x ^ (x >> 16)
    }
    fn dot_width(&self) -> i32 {
        i32::from(self.width) * 2
    }
    fn dot_height(&self) -> i32 {
        i32::from(self.height) * 4
    }
    fn ensure(&mut self, w: u16, h: u16) {
        if self.width == w && self.height == h {
            return;
        }
        self.width = w;
        self.height = h;
        self.grid = vec![0.0; self.dot_width() as usize * self.dot_height() as usize];
        self.previous = OccupancyMask::empty(w, h);
        self.mask = OccupancyMask::empty(w, h);
        for i in 0..self.settings.seeds {
            let x = Self::hash(self.settings.seed + i * 31) % self.dot_width().max(1) as u32;
            let y = Self::hash(self.settings.seed + i * 97) % self.dot_height().max(1) as u32;
            let dot_width = self.dot_width() as usize;
            self.grid[y as usize * dot_width + x as usize] = 1.0;
        }
    }
    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.dot_width() && y < self.dot_height())
            .then_some(y as usize * self.dot_width() as usize + x as usize)
    }
    fn cell_occupied(&self, cell_x: i32, cell_y: i32) -> bool {
        self.mask.is_occupied(cell_x, cell_y)
    }
}
impl Scene for GrowthScene {
    fn wants_pointer(&self) -> bool {
        true
    }

    fn pointer(&mut self, pointer: Option<[f32; 2]>) {
        self.pointer = pointer
    }
    fn wants_occupancy(&self) -> bool {
        true
    }
    fn occupancy(&mut self, mask: &OccupancyMask) {
        self.previous = self.mask.clone();
        for cell_y in 0..i32::from(self.height) {
            for cell_x in 0..i32::from(self.width) {
                if !mask.is_occupied(cell_x, cell_y) || self.previous.is_occupied(cell_x, cell_y) {
                    continue;
                }
                for dot_y in cell_y * 4..cell_y * 4 + 4 {
                    for dot_x in cell_x * 2..cell_x * 2 + 2 {
                        let Some(target) = self.idx(dot_x, dot_y) else {
                            continue;
                        };
                        let source = self
                            .idx(dot_x - 1, dot_y)
                            .or_else(|| self.idx(dot_x, dot_y - 1));
                        let Some(source) = source else { continue };
                        let value = self.grid[source] * self.settings.scroll_push as f32 / 100.0;
                        self.grid[source] *= 1.0 - self.settings.scroll_erase as f32 / 100.0;
                        self.grid[target] = self.grid[target].max(value);
                    }
                }
            }
        }
        self.mask = mask.clone();
    }
    fn set_palette(&mut self, palette: &ScenePalette) {
        self.palette = palette.clone()
    }
    fn follows_palette(&self) -> bool {
        self.settings.color_mode == GrowthColorMode::Palette
    }
    fn uses_cell_colors(&self) -> bool {
        self.settings.color_mode == GrowthColorMode::Palette
    }
    fn frames_per_second(&self) -> u32 {
        self.settings.frame_rate
    }
    fn render(&mut self, frame: &mut Frame<'_>) {
        self.ensure(frame.width, frame.height);
        let now = frame.time.as_secs_f32();
        let dt = (now - self.last_time).max(0.0).min(0.25);
        self.last_time = now;
        if now - self.last_reseed >= self.settings.reseed_seconds as f32 {
            self.last_reseed = now;
            for i in 0..self.settings.seeds {
                let x = Self::hash(self.settings.seed.wrapping_add(i).wrapping_add(now as u32))
                    % self.dot_width().max(1) as u32;
                let y = Self::hash(
                    self.settings
                        .seed
                        .wrapping_add(i * 17)
                        .wrapping_add(now as u32),
                ) % self.dot_height().max(1) as u32;
                let dot_width = self.dot_width() as usize;
                self.grid[y as usize * dot_width + x as usize] = 1.0;
            }
        }
        let w = self.dot_width();
        let h = self.dot_height();
        for y in 0..h {
            for x in 0..w {
                let Some(i) = self.idx(x, y) else { continue };
                let cell_x = x / 2;
                let cell_y = y / 4;
                if self.cell_occupied(cell_x, cell_y) && !self.previous.is_occupied(cell_x, cell_y)
                {
                    self.grid[i] *= 1.0 - self.settings.appear_erase as f32 / 100.0;
                }
                if let Some([px, py]) = self.pointer {
                    let dx = x as f32 - px * w as f32;
                    let dy = y as f32 - py * h as f32;
                    let radius = (self.settings.mouse_radius as f32 * 3.0).max(1.0);
                    if self.settings.mouse_erase && dx * dx + dy * dy <= radius * radius {
                        self.grid[i] = 0.0;
                    }
                }
                let edge = self.settings.reserve_width as i32;
                if self.settings.reserve_edge
                    && (x < edge * 2 || y < edge * 4 || x >= w - edge * 2 || y >= h - edge * 4)
                {
                    self.grid[i] = 1.0;
                }
            }
        }
        let mut next = self.grid.clone();
        let step = dt * self.settings.growth_rate as f32 / 100.0 * 0.8;
        for y in 0..h {
            for x in 0..w {
                let Some(i) = self.idx(x, y) else { continue };
                if self.cell_occupied(x / 2, y / 4) {
                    next[i] = 0.0;
                    continue;
                }
                let mut sum = 0.0;
                let mut count: f32 = 0.0;
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    if let Some(j) = self.idx(x + dx, y + dy) {
                        sum += self.grid[j];
                        count += 1.0;
                    }
                }
                let avg = sum / count.max(1.0);
                let shape = match self.settings.pattern {
                    GrowthPattern::Branching => {
                        avg * (1.0
                            + (Self::hash(x as u32 * 31 ^ y as u32 * 17) % 100) as f32
                                / (260.0 - self.settings.branch_angle as f32))
                    }
                    GrowthPattern::Rings => {
                        avg * 0.8 + ((x + y) as f32 * 0.17 + now).sin().abs() * 0.02
                    }
                    GrowthPattern::Veins => avg * 1.15,
                    GrowthPattern::Carpet => avg * 1.35,
                };
                next[i] =
                    (self.grid[i] + step * shape * self.settings.density as f32 / 55.0).min(1.0);
            }
        }
        self.grid = next;
        for y in 0..h {
            for x in 0..w {
                let Some(i) = self.idx(x, y) else { continue };
                let a = self.grid[i];
                if a <= 0.06 {
                    continue;
                }
                frame.raster.owned_dot(x as usize, y as usize, a, 0);
                if self.settings.color_mode == GrowthColorMode::Palette {
                    let ci = (y as usize / 4) * usize::from(self.width) + (x as usize / 2);
                    frame.cell_colors[ci] = self
                        .palette
                        .at(((x + y) as usize % 8) as f32 / 7.0)
                        .unwrap_or([255, 255, 255]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ControlValue;

    #[test]
    fn normalization_keeps_growth_controls_safe() {
        let settings = GrowthSettings {
            density: 0,
            growth_rate: 1000,
            seeds: 0,
            reseed_seconds: 0,
            reserve_width: 99,
            mouse_radius: 99,
            frame_rate: 1,
            ..Default::default()
        }
        .normalized();
        assert_eq!(settings.density, 1);
        assert_eq!(settings.growth_rate, 100);
        assert_eq!(settings.seeds, 1);
        assert_eq!(settings.reseed_seconds, 1);
        assert_eq!(settings.reserve_width, 8);
        assert_eq!(settings.mouse_radius, 12);
        assert_eq!(settings.frame_rate, 5);
    }

    #[test]
    fn every_growth_control_is_exposed_and_mutable() {
        let mut settings = GrowthSettings::default();
        let controls = settings.controls();
        assert!(controls.len() >= 16);
        settings
            .set_control("pattern", ControlValue::Index(3))
            .unwrap();
        settings
            .set_control("color_mode", ControlValue::Index(1))
            .unwrap();
        settings
            .set_control("reseed_seconds", ControlValue::Number(120))
            .unwrap();
        settings
            .set_control("reserve_edge", ControlValue::Bool(false))
            .unwrap();
        assert_eq!(settings.pattern, GrowthPattern::Carpet);
        assert_eq!(settings.color_mode, GrowthColorMode::Palette);
        assert_eq!(settings.reseed_seconds, 120);
        assert!(!settings.reserve_edge);
    }
}
