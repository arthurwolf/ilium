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
mod simd;

pub use settings::WindSettings;

use crate::control::SceneSettings;
use crate::registry::AmbientSettings;
use crate::scene::{Frame, OccupancyMask, Scene, SceneEnv};
use sim::Sim;

/// Glyph for dots piled to the merge threshold, and to twice and thrice it.
const MERGED_GLYPHS: [char; 3] = ['\u{2022}', '\u{25cf}', '\u{25c9}'];
/// Measured bitset crossover for the 160x50 Wind raster: about 16k dots.
const RASTER_DEDUP_MIN_DENSITY_DIVISOR: usize = 4;

#[inline]
fn should_deduplicate_raster(dot_count: usize, raster_pixel_count: usize) -> bool {
    dot_count >= raster_pixel_count / RASTER_DEDUP_MIN_DENSITY_DIVISOR
}

pub struct WindScene {
    settings: WindSettings,
    sim: Sim,
    /// Reused per-cell occupancy counts for merged-dot rendering.
    dot_counts: Vec<u16>,
    /// Eight Braille subpixels per terminal cell for the merged render pass.
    cell_subpixels: Vec<u8>,
    /// Merged-dot glyph per cell of the last frame, row-major.
    glyphs: Vec<Option<char>>,
    /// Reused raster-pixel occupancy for deduplicating opaque dots before writes.
    raster_coverage: Vec<u64>,
    glyph_width: u16,
    glyph_height: u16,
}

impl WindScene {
    // PALETTE (future user-authored JavaScript animation extension contract):
    // `env.palette` is the shared look's current palette. If that extension API
    // exposes this scene interface, its constructor receives the current palette
    // and must follow it: scenes with natural colours shift them onto it
    // (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. Monochrome scenes may ignore it. Wind draws plain dots in the shared
    // dot colour, so the host's global look already applies to it.
    pub fn new(settings: &WindSettings, _env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        Self {
            sim: Sim::new(&settings),
            settings,
            dot_counts: Vec::new(),
            cell_subpixels: Vec::new(),
            glyphs: Vec::new(),
            raster_coverage: Vec::new(),
            glyph_width: 0,
            glyph_height: 0,
        }
    }

    fn merged_glyph(merge_threshold: u32, count: u16) -> Option<char> {
        let threshold = merge_threshold as u16;
        if count < threshold {
            None
        } else if count < threshold * 2 {
            Some(MERGED_GLYPHS[0])
        } else if count < threshold * 3 {
            Some(MERGED_GLYPHS[1])
        } else {
            Some(MERGED_GLYPHS[2])
        }
    }

    #[inline]
    fn in_bounds_cell(x: f32, y: f32, width: u16, height: u16) -> Option<(usize, usize)> {
        if !(0.0..f32::from(width)).contains(&x) || !(0.0..f32::from(height)).contains(&y) {
            return None;
        }
        Some((x as usize, y as usize))
    }

    fn draw_positions_merged(
        frame: &mut Frame<'_>,
        positions: impl IntoIterator<Item = (f32, f32)>,
        merge_threshold: u32,
        counts: &mut Vec<u16>,
        cell_subpixels: &mut Vec<u8>,
        glyphs: &mut Vec<Option<char>>,
    ) {
        let cell_width = usize::from(frame.width);
        let cell_height = usize::from(frame.height);
        let cell_count = cell_width * cell_height;
        counts.resize(cell_count, 0);
        counts.fill(0);
        cell_subpixels.resize(cell_count, 0);
        cell_subpixels.fill(0);
        let raster_width = frame.raster.width;
        let merge_threshold = merge_threshold as u16;

        for (x, y) in positions {
            let Some((column, row)) = Self::in_bounds_cell(x, y, frame.width, frame.height) else {
                continue;
            };
            let cell_index = row * cell_width + column;
            counts[cell_index] = counts[cell_index].saturating_add(1);
            let count = counts[cell_index];
            if count < merge_threshold {
                let sub_x = ((x - column as f32) * 2.0) as usize;
                let sub_y = ((y - row as f32) * 4.0) as usize;
                let subpixel = sub_y * 2 + sub_x;
                cell_subpixels[cell_index] |= 1 << subpixel;
            } else if count == merge_threshold {
                cell_subpixels[cell_index] = 0;
            }
        }

        glyphs.resize(cell_count, None);
        for row in 0..cell_height {
            let row_start = row * cell_width;
            let raster_row_start = row * 4 * raster_width;
            for column in 0..cell_width {
                let index = row_start + column;
                let glyph = Self::merged_glyph(merge_threshold as u32, counts[index]);
                glyphs[index] = glyph;
                if glyph.is_some() {
                    continue;
                }

                let mut pixels = cell_subpixels[index];
                let raster_cell_start = raster_row_start + column * 2;
                while pixels != 0 {
                    let subpixel = pixels.trailing_zeros() as usize;
                    let sub_x = subpixel % 2;
                    let sub_y = subpixel / 2;
                    let raster_index = raster_cell_start + sub_y * raster_width + sub_x;
                    frame.raster.opaque_dot_index_in_bounds(raster_index);
                    pixels &= pixels - 1;
                }
            }
        }
    }

    #[cfg(test)]
    fn draw_dot(frame: &mut Frame<'_>, dot: &sim::Dot) {
        let (column, row) = (dot.x.floor() as i32, dot.y.floor() as i32);
        Self::draw_dot_at_cell(frame, dot, column, row);
    }

    #[cfg(test)]
    fn draw_dot_at_cell(frame: &mut Frame<'_>, dot: &sim::Dot, column: i32, row: i32) {
        Self::draw_position_at_cell(frame, dot.x, dot.y, column, row);
    }

    fn draw_position_at_cell(frame: &mut Frame<'_>, x: f32, y: f32, column: i32, row: i32) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        if column < 0
            || row < 0
            || column >= i32::from(frame.width)
            || row >= i32::from(frame.height)
        {
            return;
        }
        // In-bounds coordinates are nonnegative, and the raster scales are
        // powers of two, so direct scaling preserves the legacy subpixel bins
        // while avoiding per-dot floor/subtraction work.
        let sub_x = ((x * 2.0) as usize).min(usize::from(frame.raster.width) - 1);
        let sub_y = ((y * 4.0) as usize).min(usize::from(frame.raster.height) - 1);
        frame.raster.opaque_dot_in_bounds(sub_x, sub_y);
    }

    #[cfg(test)]
    fn draw_dots_deduplicated(frame: &mut Frame<'_>, dots: &[sim::Dot], coverage: &mut Vec<u64>) {
        Self::draw_positions_deduplicated(frame, dots.iter().map(|dot| (dot.x, dot.y)), coverage);
    }

    fn draw_positions_deduplicated(
        frame: &mut Frame<'_>,
        positions: impl IntoIterator<Item = (f32, f32)>,
        coverage: &mut Vec<u64>,
    ) {
        let pixel_count = frame.raster.dots.len();
        coverage.resize(pixel_count.div_ceil(u64::BITS as usize), 0);
        coverage.fill(0);
        let raster_width = frame.raster.width;

        for (x, y) in positions {
            if !x.is_finite()
                || !y.is_finite()
                || x < 0.0
                || y < 0.0
                || x >= f32::from(frame.width)
                || y >= f32::from(frame.height)
            {
                continue;
            }
            // Finite in-range checks replace cell flooring here; power-of-two
            // scaling then selects the same subpixel bins directly.
            let sub_x = (x * 2.0) as usize;
            let sub_y = (y * 4.0) as usize;
            let index = sub_y * raster_width + sub_x;
            debug_assert!(index < pixel_count);
            coverage[index / u64::BITS as usize] |= 1 << (index % u64::BITS as usize);
        }

        for (word_index, &word) in coverage.iter().enumerate() {
            let mut remaining = word;
            while remaining != 0 {
                let bit = remaining.trailing_zeros() as usize;
                let index = word_index * u64::BITS as usize + bit;
                // The bitset already yields a valid flat raster index; avoid
                // dividing into coordinates only to multiply them back here.
                frame.raster.opaque_dot_index_in_bounds(index);
                remaining &= remaining - 1;
            }
        }
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
        self.sim.advance_for_render(frame.time.as_secs_f32());
        self.glyph_width = frame.width;
        self.glyph_height = frame.height;
        if self.settings.merge_dots {
            Self::draw_positions_merged(
                frame,
                self.sim.render_positions(),
                self.settings.merge_threshold,
                &mut self.dot_counts,
                &mut self.cell_subpixels,
                &mut self.glyphs,
            );
        } else {
            self.glyphs.clear();
            if should_deduplicate_raster(
                self.sim.render_positions().size_hint().0,
                frame.raster.dots.len(),
            ) {
                Self::draw_positions_deduplicated(
                    frame,
                    self.sim.render_positions(),
                    &mut self.raster_coverage,
                );
            } else {
                for (x, y) in self.sim.render_positions() {
                    Self::draw_position_at_cell(frame, x, y, x.floor() as i32, y.floor() as i32);
                }
            }
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

#[cfg(test)]
mod raster_dedup_tests {
    use super::{sim, WindScene};
    use crate::raster::Raster;
    use crate::scene::Frame;
    use std::time::{Duration, SystemTime};

    fn frame<'a>(raster: &'a mut Raster, cell_colors: &'a mut Vec<[u8; 3]>) -> Frame<'a> {
        Frame {
            raster,
            cell_colors,
            width: 4,
            height: 3,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn deduplication_starts_near_the_measured_density_crossover() {
        let raster_pixels = 160 * 50 * 8;
        let crossover = raster_pixels / 4;
        assert!(!super::should_deduplicate_raster(
            crossover - 1,
            raster_pixels
        ));
        assert!(super::should_deduplicate_raster(crossover, raster_pixels));
        assert!(!super::should_deduplicate_raster(10_000, raster_pixels));
        assert!(super::should_deduplicate_raster(20_000, raster_pixels));
        assert!(super::should_deduplicate_raster(50_000, raster_pixels));
    }

    #[test]
    fn deduplicated_raster_writes_match_per_dot_painting() {
        let dots = [
            sim::Dot {
                x: 0.1,
                y: 0.1,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
            sim::Dot {
                x: 0.2,
                y: 0.2,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
            sim::Dot {
                x: 2.99,
                y: 1.99,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
            sim::Dot {
                x: 3.0,
                y: 2.0,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
            sim::Dot {
                x: -0.1,
                y: 1.0,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
            sim::Dot {
                x: 4.0,
                y: 1.0,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
            sim::Dot {
                x: 1.0,
                y: 3.0,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
            sim::Dot {
                x: f32::NAN,
                y: 1.0,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
            sim::Dot {
                x: 1.0,
                y: f32::INFINITY,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
        ];
        let mut expected = Raster::default();
        expected.resize(8, 12);
        expected.dots[0] = 0.25;
        expected.owner_ids[0] = 17;
        let mut actual = Raster::default();
        actual.resize(8, 12);
        actual.dots[0] = 0.25;
        actual.owner_ids[0] = 17;
        let mut expected_colors = Vec::new();
        let mut actual_colors = Vec::new();
        {
            let mut frame = frame(&mut expected, &mut expected_colors);
            for dot in &dots {
                WindScene::draw_dot(&mut frame, dot);
            }
        }
        {
            let mut coverage = Vec::new();
            WindScene::draw_dots_deduplicated(
                &mut frame(&mut actual, &mut actual_colors),
                &dots,
                &mut coverage,
            );
        }
        assert_eq!(actual.dots, expected.dots);
        assert_eq!(actual.owner_ids, expected.owner_ids);
    }
}
