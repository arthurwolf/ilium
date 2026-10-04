//! Seeded luminous curtains over a genuinely ink-free, irregular horizon.
//! The compositor may put a black background behind safe cells of this scene;
//! this renderer only owns Braille intensity, never terminal text or styles.

use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuroraSettings {
    /// Percentage of the field occupied by ink-free ground, 10..=65.
    pub horizon_height: i32,
    /// Amplitude of the seeded skyline, 0..=100.
    pub horizon_roughness: i32,
    /// Number of overlapping curtains, 2..=10.
    pub curtain_count: i32,
    /// Curtain phase speed; zero freezes their geometry, 0..=100.
    pub curtain_motion: i32,
    pub trees: bool,
    /// Number and width of the seeded foreground silhouettes, 0..=100.
    pub tree_density: i32,
    pub seed: i32,
}

impl Default for AuroraSettings {
    fn default() -> Self {
        Self {
            horizon_height: 28,
            horizon_roughness: 36,
            curtain_count: 6,
            curtain_motion: 38,
            trees: true,
            tree_density: 55,
            seed: 1,
        }
    }
}

impl SceneSettings for AuroraSettings {
    fn normalized(&self) -> Self {
        Self {
            horizon_height: self.horizon_height.clamp(10, 65),
            horizon_roughness: self.horizon_roughness.clamp(0, 100),
            curtain_count: self.curtain_count.clamp(2, 10),
            curtain_motion: self.curtain_motion.clamp(0, 100),
            trees: self.trees,
            tree_density: self.tree_density.clamp(0, 100),
            seed: self.seed.clamp(0, 9999),
        }
    }

    fn controls(&self) -> Vec<Control> {
        let settings = self.normalized();
        vec![
            Control::slider(
                "horizon_height",
                "Ground height",
                settings.horizon_height,
                (10, 65, 1),
                "%",
                "Ink-free ground measured upward from the lower edge.",
            ),
            Control::slider(
                "horizon_roughness",
                "Horizon roughness",
                settings.horizon_roughness,
                (0, 100, 1),
                "",
                "Seeded low hills; zero gives a level horizon.",
            ),
            Control::slider(
                "curtain_count",
                "Curtains",
                settings.curtain_count,
                (2, 10, 1),
                "",
                "Overlapping bands with separate folds and crests.",
            ),
            Control::slider(
                "curtain_motion",
                "Curtain motion",
                settings.curtain_motion,
                (0, 100, 1),
                "",
                "Slow folding; zero holds the same geometry at every time.",
            ),
            Control::toggle(
                "trees",
                "Tree silhouettes",
                settings.trees,
                "Dark seeded trees rise just above the horizon without sky ink on them.",
            ),
            Control::slider(
                "tree_density",
                "Tree density",
                settings.tree_density,
                (0, 100, 1),
                "",
                "Zero leaves the horizon clear, even when trees are enabled.",
            ),
            Control::slider(
                "seed",
                "Seed",
                settings.seed,
                (0, 9999, 1),
                "",
                "Repeatable skyline and tree placement.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match id {
            "trees" => {
                self.trees =
                    control::boolean(&value).ok_or_else(|| "trees expects a toggle".to_owned())?;
            }
            "horizon_height" | "horizon_roughness" | "curtain_count" | "curtain_motion"
            | "tree_density" | "seed" => {
                let number =
                    control::number(&value).ok_or_else(|| format!("{id} expects a number"))?;
                match id {
                    "horizon_height" => self.horizon_height = number,
                    "horizon_roughness" => self.horizon_roughness = number,
                    "curtain_count" => self.curtain_count = number,
                    "curtain_motion" => self.curtain_motion = number,
                    "tree_density" => self.tree_density = number,
                    "seed" => self.seed = number,
                    _ => unreachable!("matched numeric control ids above"),
                }
            }
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}

fn random(seed: i32, index: u32) -> f32 {
    let mut bits = (seed as u32).wrapping_add(index.wrapping_mul(0x9e37_79b9));
    bits = (bits ^ (bits >> 16)).wrapping_mul(0x7feb_352d);
    bits = (bits ^ (bits >> 15)).wrapping_mul(0x846c_a68b);
    (bits ^ (bits >> 16)) as f32 / u32::MAX as f32
}

impl AuroraSettings {
    /// Fractional row from the top; the raster is strictly empty at/below it.
    fn horizon_at(&self, column: f32) -> f32 {
        let level = 1.0 - self.horizon_height as f32 / 100.0;
        let roughness = self.horizon_roughness as f32 / 100.0;
        let first = (column * TAU * 2.1 + random(self.seed, 10) * TAU).sin();
        let second = (column * TAU * 5.3 + random(self.seed, 11) * TAU).sin();
        (level + roughness * (first * 0.034 + second * 0.014)).clamp(0.30, 0.92)
    }

    /// A small set of triangular pine silhouettes, evaluated once per column.
    fn tree_top_at(&self, column: f32, horizon: f32) -> f32 {
        if !self.trees || self.tree_density == 0 {
            return horizon;
        }
        let count = (self.tree_density + 9) / 10;
        let mut top = horizon;
        for index in 0..count {
            let index = index as u32;
            let center = (index as f32 + 0.5) / count as f32
                + (random(self.seed, index * 3 + 40) - 0.5) * 0.055;
            let half_width = 0.017 + random(self.seed, index * 3 + 41) * 0.025;
            let offset = (column - center).abs();
            if offset < half_width {
                let height = 0.065 + random(self.seed, index * 3 + 42) * 0.15;
                top = top.min(horizon - height * (1.0 - offset / half_width));
            }
        }
        top.max(0.0)
    }
}

pub struct AuroraScene {
    settings: AuroraSettings,
}

impl AuroraScene {
    pub fn new(settings: &AuroraSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }

    fn from_settings(settings: &AuroraSettings) -> Self {
        Self {
            settings: settings.normalized(),
        }
    }
}

impl Scene for AuroraScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let raster = &mut frame.raster;
        raster.dots.fill(0.0);
        raster.owner_ids.fill(0);
        if raster.width == 0 || raster.height == 0 {
            return;
        }
        let width = raster.width;
        let height = raster.height;
        // Reduce the large absolute clock before f32 geometry. This is seekable
        // and remains finite even near Duration::MAX.
        let phase = (frame.time.as_secs_f64() * f64::from(self.settings.curtain_motion) * 0.0027)
            .rem_euclid(f64::from(TAU)) as f32;
        let count = self.settings.curtain_count as usize;
        for x in 0..width {
            let column = (x as f32 + 0.5) / width as f32;
            let horizon = self.settings.horizon_at(column);
            let foreground_top = self.settings.tree_top_at(column, horizon);
            let limit = (foreground_top * height as f32).ceil() as usize;
            // The lower curtain tails can finish well above a low horizon.
            // A bounded twilight glow gives default trees a lit sky to cut
            // against after ordered dithering, while the ground stays ink-free.
            let twilight_shimmer = 0.82 + 0.18 * (column * TAU * 1.7 + phase).sin();
            for y in 0..limit.min(height) {
                let row = (y as f32 + 0.5) / height as f32;
                if row < foreground_top && row < horizon {
                    let nearness = ((row - horizon + 0.24) / 0.24).clamp(0.0, 1.0);
                    let twilight = nearness * nearness * (3.0 - 2.0 * nearness);
                    raster.dots[y * width + x] =
                        0.035 * (1.0 - row / horizon).powi(2) + 0.46 * twilight * twilight_shimmer;
                }
            }
            for ribbon in 0..count {
                let fraction = ribbon as f32 / count as f32;
                let shift = random(self.settings.seed, ribbon as u32 + 100) * TAU;
                let crest = 0.10
                    + fraction * 0.36
                    + 0.032 * (column * TAU * 1.9 + shift + phase * (0.7 + fraction)).sin();
                let fold = 0.5
                    + 0.5
                        * (column * TAU * (3.8 + fraction * 2.1) + shift * 1.3
                            - phase * (1.0 + fraction * 0.3))
                            .sin();
                let strength = 0.21 + 0.72 * fold.powi(3);
                let tail = 0.20 + 0.08 * (1.0 - fraction);
                let start = ((crest - 0.025).max(0.0) * height as f32).floor() as usize;
                let end = ((crest + tail).min(foreground_top) * height as f32).ceil() as usize;
                for y in start..end.min(height) {
                    let row = (y as f32 + 0.5) / height as f32;
                    if row >= foreground_top || row >= horizon {
                        continue;
                    }
                    let distance = (row - crest) / tail;
                    let fade = if distance < 0.0 {
                        (1.0 + distance * 6.0).max(0.0)
                    } else {
                        (1.0 - distance).max(0.0).powi(2)
                    };
                    let intensity = (strength * fade).clamp(0.0, 1.0);
                    let slot = &mut raster.dots[y * width + x];
                    *slot = slot.max(intensity);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Raster;
    use std::time::{Duration, SystemTime};

    fn render(settings: &AuroraSettings, width: u16, height: u16, seconds: f64) -> Vec<f32> {
        let mut scene = AuroraScene::from_settings(settings);
        let mut raster = Raster::default();
        raster.resize(usize::from(width) * 2, usize::from(height) * 4);
        let mut colors = Vec::new();
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width,
            height,
            time: Duration::from_secs_f64(seconds),
            wall: Duration::ZERO,
            now: SystemTime::UNIX_EPOCH,
        });
        raster.dots
    }

    #[test]
    fn empty_tiny_and_large_seeks_are_bounded_and_reproducible() {
        let settings = AuroraSettings::default();
        for (width, height) in [(0, 0), (1, 1), (8, 3), (120, 40), (240, 80)] {
            let first = render(&settings, width, height, 79.0);
            assert_eq!(first, render(&settings, width, height, 79.0));
            assert!(first
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
        }
        assert_ne!(
            render(&settings, 120, 40, 0.0),
            render(&settings, 120, 40, 12.0)
        );
        let frozen = AuroraSettings {
            curtain_motion: 0,
            ..settings
        };
        assert_eq!(
            render(&frozen, 120, 40, 0.0),
            render(&frozen, 120, 40, 1200.0)
        );
    }

    #[test]
    fn dark_horizon_and_tree_silhouettes_contain_no_ink() {
        let with_trees = AuroraSettings::default();
        let without_trees = AuroraSettings {
            trees: false,
            ..with_trees.clone()
        };
        let tree = render(&with_trees, 120, 40, 11.0);
        let clear = render(&without_trees, 120, 40, 11.0);
        let width = 240;
        let height = 160;
        let mut removed_ink = false;
        for x in 0..width {
            let column = (x as f32 + 0.5) / width as f32;
            let horizon = with_trees.horizon_at(column);
            let tree_top = with_trees.tree_top_at(column, horizon);
            for y in 0..height {
                let row = (y as f32 + 0.5) / height as f32;
                let index = y * width + x;
                if row >= horizon || row >= tree_top {
                    assert_eq!(tree[index], 0.0);
                }
                assert!(tree[index] <= clear[index]);
                removed_ink |= clear[index] > tree[index];
            }
        }
        assert!(removed_ink);
        let flat = AuroraSettings {
            horizon_roughness: 0,
            ..without_trees
        };
        assert_eq!(flat.horizon_at(0.1), flat.horizon_at(0.9));
    }

    #[test]
    fn default_trees_cut_visible_ordered_ink_at_both_viewport_sizes() {
        let with_trees = AuroraSettings::default();
        let without_trees = AuroraSettings {
            trees: false,
            ..with_trees.clone()
        };
        // AnimationFrame::pack uses this exact Ordered threshold and the
        // default 60% density before assembling Braille cell bits.
        for (columns, rows, minimum_removed, minimum_horizon) in
            [(120, 40, 70, 500), (48, 16, 12, 80)]
        {
            let width = columns * 2;
            let height = rows * 4;
            let tree = render(&with_trees, columns, rows, 9.0);
            let clear = render(&without_trees, columns, rows, 9.0);
            let mut removed = 0;
            let mut near_horizon = 0;
            for y in 0..usize::from(height) {
                let row = (y as f32 + 0.5) / height as f32;
                for x in 0..usize::from(width) {
                    let horizon = with_trees.horizon_at((x as f32 + 0.5) / width as f32);
                    let threshold =
                        crate::raster::threshold(x, y, crate::raster::DitherMode::Ordered);
                    let index = y * usize::from(width) + x;
                    let clear_lit = clear[index] * 0.60 > threshold;
                    let tree_lit = tree[index] * 0.60 > threshold;
                    if row >= horizon {
                        assert!(!clear_lit && !tree_lit);
                    }
                    if clear_lit && !tree_lit {
                        removed += 1;
                    }
                    if clear_lit && row >= horizon - 0.18 && row < horizon {
                        near_horizon += 1;
                    }
                }
            }
            assert!(
                removed >= minimum_removed,
                "{columns}x{rows}: only {removed} tree-cut dots"
            );
            assert!(
                near_horizon >= minimum_horizon,
                "{columns}x{rows}: only {near_horizon} lit dots near the horizon"
            );
        }
    }

    #[test]
    fn typed_controls_normalize_and_persist_independently() {
        let mut settings = AuroraSettings::default();
        assert_eq!(
            serde_json::from_str::<AuroraSettings>("{}").unwrap(),
            settings
        );
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<AuroraSettings>(&encoded).unwrap(),
            settings
        );
        for row in settings.controls() {
            assert!(!row.help.is_empty());
            assert_eq!(settings.set_control(row.id, row.value), Ok(false));
        }
        assert!(settings
            .set_control("trees", ControlValue::Number(1))
            .is_err());
        assert_eq!(
            settings.set_control("unknown", ControlValue::Bool(true)),
            Ok(false)
        );
        assert_eq!(
            settings.set_control("curtain_count", ControlValue::Number(999)),
            Ok(true)
        );
        assert_eq!(settings.curtain_count, 10);
        assert_eq!(
            settings.set_control("trees", ControlValue::Bool(false)),
            Ok(true)
        );
        assert!(!settings.trees);
    }
}
