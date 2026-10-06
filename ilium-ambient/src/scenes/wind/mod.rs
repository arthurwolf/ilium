//! Wind: dots blown across the empty parts of the screen.
//!
//! A small physics simulation (`sim`) moves dots under a fixed or rotating
//! wind with gusts, optional gravity and drag. The host tells the scene which
//! terminal cells show text (`Scene::occupancy`); dots live only in the empty
//! cells and bounce off everything else. When a cell becomes occupied, `flow`
//! works out where its content came from: scrolling text pushes nearby dots
//! the way it moves, text that appears from nowhere pushes them away more
//! gently. Optionally dots piled into one cell merge into a larger dot
//! character, drawn as native text.
//!
//! Determinism: start positions depend on the seed; motion depends on the
//! animation clock (`Frame::time`) and the masks received. If the clock runs
//! backwards the physics simply pauses for that frame. Nothing here blocks or
//! spawns threads.
//!
//! No external data source is used.

mod flow;
mod settings;
mod sim;

pub use settings::WindSettings;

use crate::control::SceneSettings;
use crate::registry::AmbientSettings;
use crate::scene::{Frame, OccupancyMask, Scene, SceneEnv};
use sim::Sim;

/// Glyph for dots piled to the merge threshold, and to twice and thrice it.
const MERGED_GLYPHS: [char; 3] = ['\u{2022}', '\u{25cf}', '\u{25c9}'];

pub struct WindScene {
    settings: WindSettings,
    sim: Sim,
    /// Merged-dot glyph per cell of the last frame, row-major.
    glyphs: Vec<Option<char>>,
    glyph_width: u16,
    glyph_height: u16,
}

impl WindScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current
    // palette. When animations become plugins, the plugin constructor receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. Monochrome scenes may ignore it. Wind draws plain dots in the shared
    // dot colour, so the host's global look already applies to it.
    pub fn new(settings: &WindSettings, _env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        Self {
            sim: Sim::new(&settings),
            settings,
            glyphs: Vec::new(),
            glyph_width: 0,
            glyph_height: 0,
        }
    }

    /// Dots per cell of the current population.
    fn counts(&self, width: u16, height: u16) -> Vec<u16> {
        let mut counts = vec![0_u16; usize::from(width) * usize::from(height)];
        for dot in self.sim.dots() {
            let (column, row) = (dot.x.floor() as i32, dot.y.floor() as i32);
            if column >= 0 && row >= 0 && column < i32::from(width) && row < i32::from(height) {
                let index = row as usize * usize::from(width) + column as usize;
                counts[index] = counts[index].saturating_add(1);
            }
        }
        counts
    }

    fn merged_glyph(&self, count: u16) -> Option<char> {
        if !self.settings.merge_dots {
            return None;
        }
        let threshold = self.settings.merge_threshold as u16;
        let tier = usize::from(count / threshold);
        (tier > 0).then(|| MERGED_GLYPHS[(tier - 1).min(MERGED_GLYPHS.len() - 1)])
    }
}

impl Scene for WindScene {
    fn pointer(&mut self, position: Option<[f32; 2]>) {
        self.sim.set_pointer(position);
    }

    fn wants_occupancy(&self) -> bool {
        true
    }

    fn occupancy(&mut self, mask: &OccupancyMask) {
        self.sim.set_mask(mask);
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        // The scene may be asked for a frame before the first mask arrives
        // (a Settings preview without a workspace): treat the screen as empty.
        if self.sim.mask().width() != frame.width || self.sim.mask().height() != frame.height {
            self.sim
                .set_mask(&OccupancyMask::empty(frame.width, frame.height));
        }
        self.sim.advance(frame.time.as_secs_f32());
        let counts = self.counts(frame.width, frame.height);
        self.glyph_width = frame.width;
        self.glyph_height = frame.height;
        self.glyphs = counts
            .iter()
            .map(|count| self.merged_glyph(*count))
            .collect();
        for dot in self.sim.dots() {
            let (column, row) = (dot.x.floor() as i32, dot.y.floor() as i32);
            if column < 0
                || row < 0
                || column >= i32::from(frame.width)
                || row >= i32::from(frame.height)
            {
                continue;
            }
            let index = row as usize * usize::from(frame.width) + column as usize;
            if self.glyphs[index].is_some() {
                continue;
            }
            let sub_x = (column as usize) * 2 + (((dot.x - dot.x.floor()) * 2.0) as usize).min(1);
            let sub_y = (row as usize) * 4 + (((dot.y - dot.y.floor()) * 4.0) as usize).min(3);
            frame.raster.owned_dot(sub_x, sub_y, 1.0, 0);
        }
    }

    fn native_glyph(&self, x: u16, y: u16) -> Option<char> {
        if x >= self.glyph_width || y >= self.glyph_height {
            return None;
        }
        self.glyphs
            .get(usize::from(y) * usize::from(self.glyph_width) + usize::from(x))
            .copied()
            .flatten()
    }

    fn frames_per_second(&self) -> u32 {
        self.settings.frame_rate
    }

    fn reconfigure(&mut self, settings: &AmbientSettings) -> bool {
        self.settings = settings.wind.normalized();
        self.sim.reconfigure(&self.settings);
        true
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod mouse_tests;
