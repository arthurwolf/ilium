//! Turning a collapsed tile grid into what is drawn: a cached rail layer,
//! closed box loops and screen tiles.

use super::library::{Library, Point, TileKind, EMPTY_TILE, MAX_ROUTES_PER_TILE, SCREEN_TILE};
use super::settings::BoxMachineSettings;
use super::wfc::{collapse_grid, mix, SplitMix64};
use std::collections::HashMap;

/// Loops shorter than this many dots are too small to carry a box.
const MIN_LOOP_LENGTH: i32 = 12;
const MAX_CUBES_PER_LOOP: i32 = 8;
const MARQUEE_LENGTH: usize = 64;

/// A closed rail loop. `points` are its corners in order; the last one
/// connects back to the first.
pub(super) struct CubeLoop {
    pub points: Vec<Point>,
    /// `cumulative[i]` is the path length before segment `i`; the last
    /// entry is the loop length.
    pub cumulative: Vec<i32>,
    pub cube_count: usize,
    /// Distance of the first box from `points[0]`, in `[0, length / cubes)`.
    pub phase: f64,
    /// +1 or -1.
    pub direction: f64,
}

impl CubeLoop {
    pub fn length(&self) -> i32 {
        self.cumulative.last().copied().unwrap_or(0)
    }

    /// Point at path distance `distance` (any real number, wraps around).
    pub fn position(&self, distance: f64) -> (f64, f64) {
        let length = f64::from(self.length());
        let along = distance.rem_euclid(length);
        let segment_count = self.points.len();
        let index = self
            .cumulative
            .partition_point(|start| f64::from(*start) <= along)
            .saturating_sub(1)
            .min(segment_count - 1);
        let from = self.points[index];
        let to = self.points[(index + 1) % segment_count];
        let span = f64::from(self.cumulative[index + 1] - self.cumulative[index]).max(1.0);
        let fraction = (along - f64::from(self.cumulative[index])) / span;
        (
            f64::from(from.0) + f64::from(to.0 - from.0) * fraction,
            f64::from(from.1) + f64::from(to.1 - from.1) * fraction,
        )
    }
}

pub(super) struct Screen {
    /// Global dot position of the tile's top-left corner.
    pub origin: Point,
    pub id: u64,
    pub marquee: Vec<bool>,
}

pub(super) struct Layout {
    pub width: usize,
    pub height: usize,
    /// Static rail layer, `width * height` intensities.
    pub rails: Vec<f32>,
    pub loops: Vec<CubeLoop>,
    pub screens: Vec<Screen>,
    /// Tile grid, kept for the adjacency tests.
    #[cfg(test)]
    pub grid: (usize, usize),
    #[cfg(test)]
    pub tiles: Vec<usize>,
}

fn tile_weights(library: &Library, settings: &BoxMachineSettings) -> Vec<f32> {
    let track_scale = settings.density as f32 / 50.0;
    library
        .tiles
        .iter()
        .map(|tile| match tile.kind {
            TileKind::Empty => 1.5,
            TileKind::Screen => {
                if settings.screens > 0 {
                    1.0
                } else {
                    0.0
                }
            }
            TileKind::Tracks => {
                let class_weight = [0.0, 3.0, 1.5, 0.5][tile.routes.len().min(MAX_ROUTES_PER_TILE)];
                let count = library.tracks_by_route_count[tile.routes.len()].max(1) as f32;
                class_weight * track_scale / count
            }
        })
        .collect()
}

struct RouteInstance {
    points: Vec<Point>,
}

fn place_routes(
    library: &Library,
    tiles: &[usize],
    grid_width: usize,
    origin: Point,
) -> Vec<RouteInstance> {
    let mut instances = Vec::new();
    for (cell, &tile_index) in tiles.iter().enumerate() {
        let tile_x = origin.0 + (cell % grid_width) as i32 * library.tile_size;
        let tile_y = origin.1 + (cell / grid_width) as i32 * library.tile_size;
        for &route_index in &library.tiles[tile_index].routes {
            let points = library.routes[route_index]
                .points
                .iter()
                .map(|&(x, y)| (tile_x + x, tile_y + y))
                .collect();
            instances.push(RouteInstance { points });
        }
    }
    instances
}

fn trace_loops(instances: &[RouteInstance]) -> Vec<Vec<Point>> {
    let mut at_point: HashMap<Point, Vec<usize>> = HashMap::new();
    for (index, instance) in instances.iter().enumerate() {
        for end in [instance.points.first(), instance.points.last()]
            .into_iter()
            .flatten()
        {
            at_point.entry(*end).or_default().push(index);
        }
    }
    let mut visited = vec![false; instances.len()];
    let mut loops = Vec::new();
    for start in 0..instances.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut points = instances[start].points.clone();
        let origin = points[0];
        let mut closed = false;
        while let Some(&end) = points.last() {
            if end == origin && points.len() > 1 {
                closed = true;
                break;
            }
            let next = at_point
                .get(&end)
                .and_then(|users| users.iter().copied().find(|user| !visited[*user]));
            let Some(next) = next else { break };
            visited[next] = true;
            let mut next_points = instances[next].points.clone();
            if next_points.first() != Some(&end) {
                next_points.reverse();
            }
            points.extend(next_points.into_iter().skip(1));
        }
        if !closed {
            continue;
        }
        points.pop();
        loops.push(simplify(points));
    }
    loops
}

/// Drop points that lie on a straight run.
fn simplify(points: Vec<Point>) -> Vec<Point> {
    let count = points.len();
    (0..count)
        .filter(|&i| {
            let previous = points[(i + count - 1) % count];
            let next = points[(i + 1) % count];
            let here = points[i];
            !((previous.0 == here.0 && here.0 == next.0)
                || (previous.1 == here.1 && here.1 == next.1))
        })
        .map(|i| points[i])
        .collect()
}

pub(super) fn segment_pairs(points: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    let count = points.len();
    (0..count).map(move |i| (points[i], points[(i + 1) % count]))
}

fn build_loop(points: Vec<Point>, index: usize, settings: &BoxMachineSettings) -> Option<CubeLoop> {
    if points.len() < 2 {
        return None;
    }
    let mut cumulative = vec![0];
    for (from, to) in segment_pairs(&points) {
        let length = (to.0 - from.0).abs() + (to.1 - from.1).abs();
        cumulative.push(cumulative.last().copied().unwrap_or(0) + length);
    }
    let length = cumulative.last().copied().unwrap_or(0);
    if length < MIN_LOOP_LENGTH {
        return None;
    }
    let cube_count = (length / settings.spacing as i32).clamp(1, MAX_CUBES_PER_LOOP);
    let hash = mix(u64::from(settings.seed) ^ (index as u64 + 1).wrapping_mul(0x9e37_79b9));
    let fraction = (hash >> 40) as f64 / 16_777_216.0;
    let direction = if settings.reverse_alt {
        if index.is_multiple_of(2) {
            1.0
        } else {
            -1.0
        }
    } else {
        1.0
    };
    Some(CubeLoop {
        points,
        cumulative,
        cube_count: cube_count as usize,
        phase: fraction * f64::from(length) / f64::from(cube_count),
        direction,
    })
}

fn build_marquee(seed: u64) -> Vec<bool> {
    let mut rng = SplitMix64(seed);
    let mut bits = Vec::with_capacity(MARQUEE_LENGTH);
    let mut lit = true;
    while bits.len() < MARQUEE_LENGTH {
        let run = 1 + (rng.next_u64() % 3) as usize;
        bits.extend(std::iter::repeat_n(lit, run));
        lit = !lit;
    }
    bits.truncate(MARQUEE_LENGTH);
    bits
}

fn draw_rails(
    rails: &mut [f32],
    width: usize,
    height: usize,
    loops: &[CubeLoop],
    settings: &BoxMachineSettings,
) {
    let level = settings.rail_level as f32 / 100.0;
    if level <= 0.0 {
        return;
    }
    for cube_loop in loops {
        for (from, to) in segment_pairs(&cube_loop.points) {
            let steps = (to.0 - from.0).abs().max((to.1 - from.1).abs());
            for step in 0..=steps {
                let x = from.0 + (to.0 - from.0).signum() * step;
                let y = from.1 + (to.1 - from.1).signum() * step;
                if settings.rails_dashed && (x + y) % 2 != 0 {
                    continue;
                }
                if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                    rails[y as usize * width + x as usize] = level;
                }
            }
        }
    }
}

impl Layout {
    pub fn build(
        library: &Library,
        settings: &BoxMachineSettings,
        width: usize,
        height: usize,
    ) -> Self {
        let tile_size = library.tile_size as usize;
        let grid = (width / tile_size, height / tile_size);
        let tile_origin = (
            ((width - grid.0 * tile_size) / 2) as i32,
            ((height - grid.1 * tile_size) / 2) as i32,
        );
        let weights = tile_weights(library, settings);
        let seed =
            mix(u64::from(settings.seed) ^ ((grid.0 as u64) << 32) ^ ((grid.1 as u64) << 48));
        let mut tiles = if grid.0 == 0 || grid.1 == 0 {
            Vec::new()
        } else {
            collapse_grid(library, &weights, grid, seed)
        };
        let mut allowed_screens = settings.screens as usize;
        for tile in &mut tiles {
            if *tile == SCREEN_TILE {
                if allowed_screens == 0 {
                    *tile = EMPTY_TILE;
                } else {
                    allowed_screens -= 1;
                }
            }
        }

        let instances = place_routes(library, &tiles, grid.0.max(1), tile_origin);
        let loops: Vec<CubeLoop> = trace_loops(&instances)
            .into_iter()
            .enumerate()
            .filter_map(|(index, points)| build_loop(points, index, settings))
            .collect();
        let mut rails = vec![0.0; width * height];
        draw_rails(&mut rails, width, height, &loops, settings);

        let screens = tiles
            .iter()
            .enumerate()
            .filter(|(_, tile)| **tile == SCREEN_TILE)
            .map(|(cell, _)| {
                let id =
                    mix(u64::from(settings.seed) ^ (cell as u64 + 1).wrapping_mul(0xa24b_aed4));
                Screen {
                    origin: (
                        tile_origin.0 + (cell % grid.0) as i32 * library.tile_size,
                        tile_origin.1 + (cell / grid.0) as i32 * library.tile_size,
                    ),
                    id,
                    marquee: build_marquee(id),
                }
            })
            .collect();
        Self {
            width,
            height,
            rails,
            loops,
            screens,
            #[cfg(test)]
            grid,
            #[cfg(test)]
            tiles,
        }
    }
}
