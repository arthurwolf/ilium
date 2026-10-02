//! Maps: normalized axis-aligned paths, snapped onto a cell grid that matches
//! the screen's aspect ratio, with the blocked cells and the path samples the
//! AI and the simulation need.

use super::model::TowerKind;

/// Grid height in cells, HUD strip included.
pub const GRID_HEIGHT: i32 = 20;
/// Rows reserved for the HUD above the playfield.
pub const HUD_ROWS: i32 = 3;
/// Spacing of path samples in cells.
pub const SAMPLE_STEP: f32 = 0.5;
/// Cells closer than this to a path centre line cannot be built on.
const PATH_CLEARANCE: f32 = 0.95;

pub struct MapDef {
    pub name: &'static str,
    /// Each path as normalized points; consecutive points share an x or a y
    /// so every segment is axis aligned. Values outside 0..1 lie off screen.
    pub paths: &'static [&'static [(f32, f32)]],
}

pub const MAPS: [MapDef; 6] = [
    MapDef {
        name: "SWITCHBACK",
        paths: &[&[
            (-0.05, 0.1),
            (0.93, 0.1),
            (0.93, 0.37),
            (0.07, 0.37),
            (0.07, 0.64),
            (0.93, 0.64),
            (0.93, 0.92),
            (1.05, 0.92),
        ]],
    },
    MapDef {
        name: "SPIRAL",
        paths: &[&[
            (-0.05, 0.1),
            (0.92, 0.1),
            (0.92, 0.9),
            (0.08, 0.9),
            (0.08, 0.3),
            (0.75, 0.3),
            (0.75, 0.7),
            (0.3, 0.7),
            (0.3, 0.5),
            (0.52, 0.5),
        ]],
    },
    MapDef {
        name: "COMB",
        paths: &[&[
            (0.1, -0.05),
            (0.1, 0.85),
            (0.3, 0.85),
            (0.3, 0.15),
            (0.5, 0.15),
            (0.5, 0.85),
            (0.7, 0.85),
            (0.7, 0.15),
            (0.9, 0.15),
            (0.9, 1.05),
        ]],
    },
    MapDef {
        name: "TWIN GATES",
        paths: &[
            &[(-0.05, 0.15), (0.55, 0.15), (0.55, 0.5), (1.05, 0.5)],
            &[
                (-0.05, 0.85),
                (0.25, 0.85),
                (0.25, 0.5),
                (0.55, 0.5),
                (1.05, 0.5),
            ],
        ],
    },
    MapDef {
        name: "STAIRCASE",
        paths: &[&[
            (-0.05, 0.9),
            (0.15, 0.9),
            (0.15, 0.7),
            (0.35, 0.7),
            (0.35, 0.5),
            (0.55, 0.5),
            (0.55, 0.3),
            (0.75, 0.3),
            (0.75, 0.1),
            (1.05, 0.1),
        ]],
    },
    MapDef {
        name: "SERPENT",
        paths: &[&[
            (-0.05, 0.5),
            (0.2, 0.5),
            (0.2, 0.2),
            (0.4, 0.2),
            (0.4, 0.8),
            (0.6, 0.8),
            (0.6, 0.2),
            (0.8, 0.2),
            (0.8, 0.5),
            (1.05, 0.5),
        ]],
    },
];

/// One point sampled along a path.
#[derive(Debug, Clone, Copy)]
pub struct PathSample {
    pub pos: (f32, f32),
    pub path: usize,
}

pub struct Level {
    pub width: i32,
    pub height: i32,
    pub name: &'static str,
    /// Cell-centre polylines (cell coordinates, `0.0` is the centre of cell 0).
    pub paths: Vec<Vec<(f32, f32)>>,
    /// Cumulative lengths of every polyline vertex.
    pub cumulative: Vec<Vec<f32>>,
    pub samples: Vec<PathSample>,
    pub blocked: Vec<bool>,
    /// Buildable cells in row-major order.
    pub buildable: Vec<(i32, i32)>,
    /// `coverage[kind][i]`: path length (cells) within the base range of
    /// `kind` for buildable cell `i`.
    pub coverage: Vec<Vec<f32>>,
    /// `masks[kind][i]`: which samples that coverage consists of, one bit
    /// per sample, so overlaps between towers are a popcount.
    pub masks: Vec<Vec<Vec<u64>>>,
    /// For each buildable cell, the longest straight run (cells) of path an
    /// axis-aligned ray from it follows within 1.3 cells of its line.
    pub straight: Vec<f32>,
}

impl Level {
    pub fn cell_index(&self, cell: (i32, i32)) -> Option<usize> {
        if cell.0 < 0 || cell.1 < 0 || cell.0 >= self.width || cell.1 >= self.height {
            return None;
        }
        Some(cell.1 as usize * self.width as usize + cell.0 as usize)
    }

    pub fn is_buildable(&self, cell: (i32, i32)) -> bool {
        self.cell_index(cell)
            .is_some_and(|index| !self.blocked[index])
    }

    /// Position of `cell` in `buildable`, if it can be built on.
    pub fn buildable_index(&self, cell: (i32, i32)) -> Option<usize> {
        self.buildable
            .binary_search_by_key(&(cell.1, cell.0), |&(x, y)| (y, x))
            .ok()
    }

    pub fn path_length(&self, path: usize) -> f32 {
        self.cumulative[path].last().copied().unwrap_or(0.0)
    }

    /// Position and heading (radians) at `distance` cells along `path`.
    pub fn position(&self, path: usize, distance: f32) -> ((f32, f32), f32) {
        let points = &self.paths[path];
        let cumulative = &self.cumulative[path];
        let distance = distance.clamp(0.0, self.path_length(path));
        let mut segment = 0;
        while segment + 2 < points.len() && cumulative[segment + 1] < distance {
            segment += 1;
        }
        let from = points[segment];
        let to = points[segment + 1];
        let length = (cumulative[segment + 1] - cumulative[segment]).max(1e-4);
        let fraction = ((distance - cumulative[segment]) / length).clamp(0.0, 1.0);
        (
            (
                from.0 + (to.0 - from.0) * fraction,
                from.1 + (to.1 - from.1) * fraction,
            ),
            (to.1 - from.1).atan2(to.0 - from.0),
        )
    }

    /// Where the playfield cell `(x, y)` is drawn is the scene's business;
    /// here only the geometry.
    pub fn build(map_index: usize, width: i32) -> Self {
        let definition = &MAPS[map_index % MAPS.len()];
        let height = GRID_HEIGHT;
        let play_rows = (height - HUD_ROWS - 2) as f32;
        let play_columns = (width - 3) as f32;
        let to_cell = |(nx, ny): (f32, f32)| -> (f32, f32) {
            let x = if (0.0..=1.0).contains(&nx) {
                (1.0 + nx * play_columns).round()
            } else {
                1.0 + nx * play_columns
            };
            let y = if (0.0..=1.0).contains(&ny) {
                (HUD_ROWS as f32 + ny * play_rows).round()
            } else {
                HUD_ROWS as f32 + ny * play_rows
            };
            (x, y)
        };
        let paths: Vec<Vec<(f32, f32)>> = definition
            .paths
            .iter()
            .map(|path| path.iter().copied().map(to_cell).collect())
            .collect();
        let cumulative: Vec<Vec<f32>> = paths
            .iter()
            .map(|path| {
                let mut total = 0.0;
                let mut lengths = vec![0.0];
                for pair in path.windows(2) {
                    total += (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
                    lengths.push(total);
                }
                lengths
            })
            .collect();
        let mut level = Self {
            width,
            height,
            name: definition.name,
            paths,
            cumulative,
            samples: Vec::new(),
            blocked: vec![false; (width * height) as usize],
            buildable: Vec::new(),
            coverage: Vec::new(),
            masks: Vec::new(),
            straight: Vec::new(),
        };
        level.sample_paths();
        level.block_cells();
        level.compute_coverage();
        level
    }

    fn sample_paths(&mut self) {
        for path in 0..self.paths.len() {
            let length = self.path_length(path);
            let count = (length / SAMPLE_STEP).floor() as usize;
            for step in 0..=count {
                let distance = step as f32 * SAMPLE_STEP;
                let (pos, _) = self.position(path, distance);
                self.samples.push(PathSample { pos, path });
            }
        }
    }

    fn block_cells(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                let index = (y * self.width + x) as usize;
                let hud = y < HUD_ROWS - 1 || y >= self.height - 1 || x < 1 || x >= self.width - 1;
                let near_path = self.paths.iter().any(|path| {
                    path.windows(2).any(|pair| {
                        segment_distance((x as f32, y as f32), pair[0], pair[1]) < PATH_CLEARANCE
                    })
                });
                self.blocked[index] = hud || near_path;
                if !self.blocked[index] {
                    self.buildable.push((x, y));
                }
            }
        }
    }

    fn compute_coverage(&mut self) {
        let cells = self.buildable.clone();
        let words = self.samples.len().div_ceil(64);
        self.masks = TowerKind::ALL
            .iter()
            .map(|kind| {
                // A Chill slows for a moment after a hit, so its effect
                // reaches a little beyond its range.
                let reach = kind.stats().range + if *kind == TowerKind::Chill { 1.4 } else { 0.0 };
                let squared = reach * reach;
                cells
                    .iter()
                    .map(|&(x, y)| {
                        let mut mask = vec![0u64; words];
                        for (index, sample) in self.samples.iter().enumerate() {
                            let dx = sample.pos.0 - x as f32;
                            let dy = sample.pos.1 - y as f32;
                            if dx * dx + dy * dy <= squared {
                                mask[index / 64] |= 1 << (index % 64);
                            }
                        }
                        mask
                    })
                    .collect()
            })
            .collect();
        self.coverage = self
            .masks
            .iter()
            .map(|per_cell| {
                per_cell
                    .iter()
                    .map(|mask| {
                        mask.iter().map(|word| word.count_ones()).sum::<u32>() as f32 * SAMPLE_STEP
                    })
                    .collect()
            })
            .collect();
        self.straight = cells
            .iter()
            .map(|&(x, y)| {
                let range = TowerKind::Lancer.stats().range;
                [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)]
                    .iter()
                    .map(|&(dx, dy)| {
                        let count = self
                            .samples
                            .iter()
                            .filter(|sample| {
                                let rx = sample.pos.0 - x as f32;
                                let ry = sample.pos.1 - y as f32;
                                let along = rx * dx + ry * dy;
                                let across = (rx * dy - ry * dx).abs();
                                along > 0.5 && along <= range && across < 1.3
                            })
                            .count();
                        count as f32 * SAMPLE_STEP
                    })
                    .fold(0.0, f32::max)
            })
            .collect();
    }
}

/// Distance from point `p` to segment `a`-`b`.
pub fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let squared = dx * dx + dy * dy;
    let fraction = if squared > 1e-6 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let nearest = (a.0 + dx * fraction, a.1 + dy * fraction);
    (p.0 - nearest.0).hypot(p.1 - nearest.1)
}

/// Grid width in cells for a raster of `raster_width` x `raster_height` dots.
pub fn grid_width_for(raster_width: usize, raster_height: usize) -> i32 {
    let aspect = raster_width as f32 / raster_height.max(1) as f32;
    ((GRID_HEIGHT as f32 * aspect).round() as i32).clamp(34, 96)
}
