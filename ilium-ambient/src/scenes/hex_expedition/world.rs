//! Procedural expedition maps. The endless plane is tiled with islands; each
//! island is one expedition map in the manner of an adventure board game: a
//! single landmass ringed by beach and sea, its biomes arranged by moisture
//! and mountain ranges, with exactly one expedition ship waiting on the
//! shore, one distant goal (a temple or pyramid), and villages, camps, ruins,
//! caves, mines and shrines placed where they make sense and never on top of
//! each other.
//!
//! Everything is a pure function of (map kind, seed, island coordinates).
//! `Atlas` only memoises generated islands.

use std::collections::{HashMap, VecDeque};

pub const SQRT3: f32 = 1.732_050_8;

/// Axial neighbour offsets, paired with the unit direction from a tile centre
/// to that neighbour in pointy-top pixel space (y grows downwards):
/// east, south-east, south-west, west, north-west, north-east.
pub const NEIGHBORS: [(i32, i32); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];
pub const NEIGHBOR_DIRECTIONS: [(f32, f32); 6] = [
    (1.0, 0.0),
    (0.5, 0.866_025_4),
    (-0.5, 0.866_025_4),
    (-1.0, 0.0),
    (-0.5, -0.866_025_4),
    (0.5, -0.866_025_4),
];

/// Island cell size in tiles. Rows must stay even so that local and global
/// row parity agree.
pub const ISLAND_COLUMNS: i32 = 40;
pub const ISLAND_ROWS: i32 = 30;

/// Kinds of map, each with its own terrain mix, palette and weather.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapKind {
    Jungle,
    Savanna,
    Desert,
    Arctic,
    Volcanic,
}

impl MapKind {
    pub const ALL: [Self; 5] = [
        Self::Jungle,
        Self::Savanna,
        Self::Desert,
        Self::Arctic,
        Self::Volcanic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Jungle => "Jungle",
            Self::Savanna => "Savanna",
            Self::Desert => "Desert",
            Self::Arctic => "Arctic",
            Self::Volcanic => "Volcanic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Terrain {
    DeepWater,
    Water,
    Reef,
    Ice,
    Beach,
    Grass,
    Forest,
    Jungle,
    Mangrove,
    Swamp,
    Desert,
    Dunes,
    Mesa,
    Hills,
    Mountain,
    Glacier,
    Snow,
    Pines,
    Rock,
    DeadForest,
    Lava,
    Geyser,
    Volcano,
    Oasis,
}

impl Terrain {
    /// Terrain that ships sail on and that shows foam against land.
    pub fn is_water(self) -> bool {
        matches!(self, Self::DeepWater | Self::Water | Self::Reef)
    }

    /// Open ground on which people build and camp.
    pub fn is_flat_land(self) -> bool {
        matches!(
            self,
            Self::Grass
                | Self::Forest
                | Self::Jungle
                | Self::Desert
                | Self::Dunes
                | Self::Snow
                | Self::Pines
                | Self::Rock
                | Self::DeadForest
                | Self::Beach
        )
    }

    /// Rough ground where caves, mines and shrines belong.
    pub fn is_rough_land(self) -> bool {
        matches!(
            self,
            Self::Hills | Self::Mountain | Self::Rock | Self::Mesa | Self::Glacier
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    None,
    Village,
    /// The expedition goal on jungle, arctic and volcanic maps.
    Temple,
    /// The expedition goal on savanna and desert maps.
    Pyramid,
    Camp,
    Ruins,
    /// The expedition's own ship, moored on the shore. One per island.
    Ship,
    Cave,
    Shrine,
    Mine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    pub terrain: Terrain,
    pub feature: Feature,
    /// Per-tile hash for decoration choices and animation phases.
    pub variant: u32,
}

pub fn hash_cell(first: i32, second: i32, seed: u32) -> u32 {
    let mut value = (first as u32).wrapping_mul(0x8da6_b343)
        ^ (second as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0x9e37_79b1)
        ^ 0xcb1a_b31f;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

/// Uniform value in 0..1 from a hash.
pub fn unit(value: u32) -> f32 {
    (value >> 8) as f32 / (1u32 << 24) as f32
}

fn smooth(fraction: f64) -> f64 {
    fraction * fraction * (3.0 - 2.0 * fraction)
}

pub fn value_noise(x: f64, y: f64, seed: u32) -> f32 {
    let floor_x = x.floor();
    let floor_y = y.floor();
    let fx = smooth(x - floor_x);
    let fy = smooth(y - floor_y);
    let ix = floor_x as i64 as i32;
    let iy = floor_y as i64 as i32;
    let corner = |dx: i32, dy: i32| {
        f64::from(unit(hash_cell(
            ix.wrapping_add(dx),
            iy.wrapping_add(dy),
            seed,
        )))
    };
    let top = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * fx;
    let bottom = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * fx;
    (top + (bottom - top) * fy) as f32
}

/// Fractal value noise in 0..1, mean 0.5.
pub fn fbm(x: f64, y: f64, seed: u32, octaves: u32) -> f32 {
    let mut sum = 0.0;
    let mut norm = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    for octave in 0..octaves {
        sum += amplitude
            * value_noise(
                x * frequency,
                y * frequency,
                seed.wrapping_add(octave * 7919),
            );
        norm += amplitude;
        amplitude *= 0.5;
        frequency *= 2.03;
    }
    sum / norm
}

/// One world: a map kind and a seed.
#[derive(Debug, Clone, Copy)]
pub struct World {
    pub kind: MapKind,
    pub seed: u32,
    /// Landmark frequency, 0..2 (1 is the default density).
    pub landmark_scale: f32,
}

impl World {
    pub fn new(kind: MapKind, seed: u32, landmark_scale: f32) -> Self {
        Self {
            kind,
            seed: seed.wrapping_add(kind as u32 * 0x1f3d),
            landmark_scale,
        }
    }
}

/// Splits a global axial tile into (island x, island y, local column, local
/// row). Columns use the odd-row offset layout that matches pointy-top axial
/// coordinates.
pub fn locate(q: i32, r: i32) -> (i32, i32, i32, i32) {
    let column = q.wrapping_add(r.div_euclid(2));
    (
        column.div_euclid(ISLAND_COLUMNS),
        r.div_euclid(ISLAND_ROWS),
        column.rem_euclid(ISLAND_COLUMNS),
        r.rem_euclid(ISLAND_ROWS),
    )
}

/// Hex distance between two axial tiles.
pub fn hex_distance(first: (i32, i32), second: (i32, i32)) -> i32 {
    let dq = first.0 - second.0;
    let dr = first.1 - second.1;
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}

/// Local (column, row) neighbours in the odd-row layout.
fn offset_neighbors(column: i32, row: i32) -> [(i32, i32); 6] {
    if row & 1 == 0 {
        [
            (column + 1, row),
            (column - 1, row),
            (column, row - 1),
            (column - 1, row - 1),
            (column, row + 1),
            (column - 1, row + 1),
        ]
    } else {
        [
            (column + 1, row),
            (column - 1, row),
            (column, row - 1),
            (column + 1, row - 1),
            (column, row + 1),
            (column + 1, row + 1),
        ]
    }
}

/// Axial (q, r) of a local tile, for distance arithmetic inside one island.
fn local_axial(column: i32, row: i32) -> (i32, i32) {
    (column - row.div_euclid(2), row)
}

fn axial_of_index(index: usize) -> (i32, i32) {
    local_axial(index as i32 % ISLAND_COLUMNS, index as i32 / ISLAND_COLUMNS)
}

/// One generated island: a dense grid of tiles.
pub struct Island {
    tiles: Vec<Tile>,
}

impl Island {
    pub fn tile(&self, column: i32, row: i32) -> Tile {
        self.tiles[(row * ISLAND_COLUMNS + column) as usize]
    }
}

/// Per-tile scratch values used while shaping an island.
#[derive(Clone, Copy, Default)]
struct Cell {
    land: bool,
    /// Elevation 0..1.
    elevation: f32,
    moisture: f32,
    /// Mountain-range strength, near 1 along the crest.
    ridge: f32,
    /// Low-lying basins, where lakes collect.
    basin: f32,
    /// Hex steps to the nearest sea tile (land only; 1 = on the coast).
    coast: i32,
    /// Hex steps to the nearest land tile (sea only).
    offshore: i32,
    roll: f32,
}

fn index_of(column: i32, row: i32) -> usize {
    (row * ISLAND_COLUMNS + column) as usize
}

fn in_bounds(column: i32, row: i32) -> bool {
    (0..ISLAND_COLUMNS).contains(&column) && (0..ISLAND_ROWS).contains(&row)
}

/// Multi-source breadth-first distance over the island grid. Tiles beside the
/// grid edge count as one step from a source when `outside_is_source`.
fn distance_field(sources: &[bool], outside_is_source: bool) -> Vec<i32> {
    let mut distance = vec![i32::MAX; sources.len()];
    let mut queue = VecDeque::new();
    for row in 0..ISLAND_ROWS {
        for column in 0..ISLAND_COLUMNS {
            let index = index_of(column, row);
            let touches_outside = outside_is_source
                && offset_neighbors(column, row)
                    .iter()
                    .any(|(nc, nr)| !in_bounds(*nc, *nr));
            if sources[index] {
                distance[index] = 0;
                queue.push_back((column, row));
            } else if touches_outside {
                distance[index] = 1;
                queue.push_back((column, row));
            }
        }
    }
    while let Some((column, row)) = queue.pop_front() {
        let here = distance[index_of(column, row)];
        for (nc, nr) in offset_neighbors(column, row) {
            if !in_bounds(nc, nr) {
                continue;
            }
            let slot = index_of(nc, nr);
            if distance[slot] > here + 1 {
                distance[slot] = here + 1;
                queue.push_back((nc, nr));
            }
        }
    }
    distance
}

/// Keeps only the largest connected landmass; everything else becomes sea.
fn keep_largest_landmass(land: &mut [bool]) {
    let mut label = vec![0usize; land.len()];
    let mut sizes = vec![0usize];
    for start in 0..land.len() {
        if !land[start] || label[start] != 0 {
            continue;
        }
        let id = sizes.len();
        sizes.push(0);
        let mut queue = VecDeque::new();
        label[start] = id;
        queue.push_back(start);
        while let Some(index) = queue.pop_front() {
            sizes[id] += 1;
            let column = index as i32 % ISLAND_COLUMNS;
            let row = index as i32 / ISLAND_COLUMNS;
            for (nc, nr) in offset_neighbors(column, row) {
                if !in_bounds(nc, nr) {
                    continue;
                }
                let slot = index_of(nc, nr);
                if land[slot] && label[slot] == 0 {
                    label[slot] = id;
                    queue.push_back(slot);
                }
            }
        }
    }
    let Some((best, _)) = sizes
        .iter()
        .enumerate()
        .skip(1)
        .max_by_key(|(_, size)| **size)
    else {
        return;
    };
    for (index, slot) in land.iter_mut().enumerate() {
        *slot = *slot && label[index] == best;
    }
}

fn land_terrain(kind: MapKind, cell: &Cell) -> Terrain {
    let coast = cell.coast;
    let (e, m) = (cell.elevation, cell.moisture);
    let mountain = cell.ridge > 0.935 && e > 0.4 && coast >= 3;
    let hills = cell.ridge > 0.885 && e > 0.34 && coast >= 2;
    let lake = cell.basin > 0.70 && coast >= 4 && e < 0.62;
    match kind {
        MapKind::Jungle => {
            if coast == 1 {
                return if m > 0.52 {
                    Terrain::Mangrove
                } else {
                    Terrain::Beach
                };
            }
            if lake {
                Terrain::Water
            } else if mountain {
                Terrain::Mountain
            } else if hills {
                Terrain::Hills
            } else if m > 0.62 && e < 0.55 {
                Terrain::Swamp
            } else if m > 0.46 {
                Terrain::Jungle
            } else if m > 0.38 {
                Terrain::Forest
            } else {
                Terrain::Grass
            }
        }
        MapKind::Savanna => {
            if coast == 1 {
                return Terrain::Beach;
            }
            if lake {
                Terrain::Water
            } else if mountain {
                Terrain::Mountain
            } else if hills {
                Terrain::Hills
            } else if m > 0.6 {
                Terrain::Forest
            } else if m < 0.34 {
                Terrain::Desert
            } else {
                Terrain::Grass
            }
        }
        MapKind::Desert => {
            if coast == 1 || (coast == 2 && m < 0.36) {
                return Terrain::Beach;
            }
            if cell.basin > 0.74 && coast >= 3 {
                Terrain::Oasis
            } else if mountain {
                Terrain::Mountain
            } else if cell.ridge > 0.86 && e > 0.4 {
                Terrain::Mesa
            } else if hills {
                Terrain::Hills
            } else if m < 0.34 && e > 0.46 {
                Terrain::Rock
            } else if m < 0.5 {
                Terrain::Dunes
            } else {
                Terrain::Desert
            }
        }
        MapKind::Arctic => {
            if coast == 1 {
                return Terrain::Ice;
            }
            if lake {
                Terrain::Ice
            } else if mountain {
                Terrain::Mountain
            } else if e > 0.6 && cell.ridge > 0.85 {
                Terrain::Glacier
            } else if hills {
                Terrain::Hills
            } else if m > 0.5 {
                Terrain::Pines
            } else {
                Terrain::Snow
            }
        }
        MapKind::Volcanic => {
            if coast == 1 {
                return Terrain::Beach;
            }
            if cell.basin > 0.72 && coast >= 3 {
                Terrain::Lava
            } else if mountain {
                Terrain::Mountain
            } else if hills {
                Terrain::Hills
            } else if cell.roll < 0.035 && coast >= 3 {
                Terrain::Geyser
            } else if m > 0.56 {
                Terrain::DeadForest
            } else {
                Terrain::Rock
            }
        }
    }
}

/// Generates one island cell. Pure in its arguments.
pub fn generate_island(world: &World, island_x: i32, island_y: i32) -> Island {
    let kind = world.kind;
    let isl = hash_cell(island_x, island_y, world.seed ^ 0x15_1a_9d);
    let random = |salt: i32| unit(hash_cell(salt, 17, isl));
    let offset = |salt: i32| f64::from(hash_cell(salt, 29, isl) & 0xfff);
    let width = f64::from(ISLAND_COLUMNS);
    let height = f64::from(ISLAND_ROWS) * 0.866;
    let center = (
        width * 0.5 + f64::from(random(1) - 0.5) * 4.0,
        height * 0.5 + f64::from(random(2) - 0.5) * 3.0,
    );
    let radii = (
        width * f64::from(0.37 + 0.05 * random(3)),
        height * f64::from(0.38 + 0.05 * random(4)),
    );
    let count = (ISLAND_COLUMNS * ISLAND_ROWS) as usize;
    let mut cells = vec![Cell::default(); count];
    let position = |column: i32, row: i32| {
        (
            f64::from(column) + 0.5 * f64::from(row & 1),
            f64::from(row) * 0.866,
        )
    };
    let radial = |x: f64, y: f64| {
        (((x - center.0) / radii.0).powi(2) + ((y - center.1) / radii.1).powi(2)).sqrt()
    };

    let mut land = vec![false; count];
    for row in 0..ISLAND_ROWS {
        for column in 0..ISLAND_COLUMNS {
            let (x, y) = position(column, row);
            let d = radial(x, y);
            let shape = fbm(x * 0.07 + offset(1), y * 0.07 + offset(2), isl, 3);
            let fine = fbm(x * 0.21 + offset(3), y * 0.21 + offset(4), isl ^ 0x33, 2);
            let field = 1.0 - d.powf(1.7) as f32 + (shape - 0.5) * 1.7 + (fine - 0.5) * 0.45;
            let margin =
                column < 2 || row < 2 || column >= ISLAND_COLUMNS - 2 || row >= ISLAND_ROWS - 2;
            land[index_of(column, row)] = field > 0.12 && !margin;
        }
    }
    keep_largest_landmass(&mut land);

    let sea: Vec<bool> = land.iter().map(|is_land| !is_land).collect();
    let coast = distance_field(&sea, true);
    let offshore = distance_field(&land, false);
    for row in 0..ISLAND_ROWS {
        for column in 0..ISLAND_COLUMNS {
            let index = index_of(column, row);
            let (x, y) = position(column, row);
            let d = radial(x, y) as f32;
            let relief = fbm(x * 0.13 + offset(5), y * 0.13 + offset(6), isl ^ 0x4e, 3);
            let crest = fbm(
                x * 0.075 + offset(7),
                y * 0.075 + offset(8),
                isl ^ 0x7a31,
                2,
            );
            cells[index] = Cell {
                land: land[index],
                elevation: ((1.0 - d).clamp(0.0, 1.0) * 0.55 + relief * 0.6).clamp(0.0, 1.0),
                moisture: fbm(x * 0.27 + offset(9), y * 0.27 + offset(10), isl ^ 0x51ed, 3),
                ridge: 1.0 - (2.0 * crest - 1.0).abs(),
                basin: fbm(x * 0.25 + offset(11), y * 0.25 + offset(12), isl ^ 0x9b, 3),
                coast: coast[index],
                offshore: offshore[index],
                roll: unit(hash_cell(column, row, isl ^ 0x2b1d)),
            };
        }
    }

    let mut terrain = vec![Terrain::DeepWater; count];
    for index in 0..count {
        let cell = cells[index];
        terrain[index] = if cell.land {
            land_terrain(kind, &cell)
        } else if kind == MapKind::Arctic && cell.offshore == 1 {
            Terrain::Ice
        } else if cell.offshore >= 3 {
            Terrain::DeepWater
        } else if cell.offshore == 2 && cell.roll < 0.09 && kind != MapKind::Arctic {
            Terrain::Reef
        } else {
            Terrain::Water
        };
    }

    if kind == MapKind::Volcanic {
        raise_volcanoes(&cells, &mut terrain, isl);
    }

    let mut features = vec![Feature::None; count];
    place_features(world, &cells, &terrain, &mut features, isl);

    let tiles = (0..count)
        .map(|index| Tile {
            terrain: terrain[index],
            feature: features[index],
            variant: hash_cell(index as i32, 0x3c6e, isl),
        })
        .collect();
    Island { tiles }
}

/// One great volcano at the island's highest ground, with a ring of
/// mountains, and sometimes a lesser one far away.
fn raise_volcanoes(cells: &[Cell], terrain: &mut [Terrain], isl: u32) {
    let mut peaks: Vec<usize> = Vec::new();
    let mut order: Vec<usize> = (0..cells.len())
        .filter(|index| cells[*index].land && cells[*index].coast >= 4)
        .collect();
    order.sort_by(|a, b| cells[*b].elevation.total_cmp(&cells[*a].elevation));
    for index in order {
        let here = axial_of_index(index);
        let far_enough = peaks
            .iter()
            .all(|peak| hex_distance(here, axial_of_index(*peak)) >= 10);
        let wanted =
            peaks.is_empty() || (peaks.len() < 2 && unit(hash_cell(index as i32, 5, isl)) < 0.5);
        if far_enough && wanted {
            peaks.push(index);
        }
        if peaks.len() == 2 {
            break;
        }
    }
    for peak in peaks {
        terrain[peak] = Terrain::Volcano;
        let (column, row) = (peak as i32 % ISLAND_COLUMNS, peak as i32 / ISLAND_COLUMNS);
        for (nc, nr) in offset_neighbors(column, row) {
            if !in_bounds(nc, nr) {
                continue;
            }
            let slot = index_of(nc, nr);
            if cells[slot].land && !matches!(terrain[slot], Terrain::Lava | Terrain::Volcano) {
                terrain[slot] = if unit(hash_cell(nc, nr, isl ^ 0x6d)) < 0.55 {
                    Terrain::Mountain
                } else {
                    Terrain::Hills
                };
            }
        }
    }
}

/// Greedy placement with minimum spacing: candidates are ordered by score,
/// then accepted while they keep their distance from everything placed.
struct Placer<'a> {
    features: &'a mut [Feature],
    placed: Vec<(i32, i32)>,
}

impl Placer<'_> {
    fn try_place(&mut self, index: usize, feature: Feature, spacing: i32) -> bool {
        let here = axial_of_index(index);
        if self.features[index] != Feature::None
            || self
                .placed
                .iter()
                .any(|other| hex_distance(here, *other) < spacing)
        {
            return false;
        }
        self.features[index] = feature;
        self.placed.push(here);
        true
    }

    fn place_best(
        &mut self,
        candidates: &mut [(usize, f32)],
        feature: Feature,
        wanted: usize,
        spacing: i32,
    ) {
        candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut done = 0;
        for (index, _) in candidates.iter() {
            if done == wanted {
                break;
            }
            if self.try_place(*index, feature, spacing) {
                done += 1;
            }
        }
    }
}

fn place_features(
    world: &World,
    cells: &[Cell],
    terrain: &[Terrain],
    features: &mut [Feature],
    isl: u32,
) {
    let scale = world.landmark_scale;
    if scale <= 0.0 {
        return;
    }
    let count = cells.len();
    let score = |index: usize, salt: i32| unit(hash_cell(index as i32, salt, isl ^ 0xfea7));
    let amount = |base: f32| (base * scale).round().max(1.0) as usize;
    let mut placer = Placer {
        features,
        placed: Vec::new(),
    };

    // The ship: the sea tile on the shore furthest in a random direction.
    let ship_depth = if world.kind == MapKind::Arctic { 2 } else { 1 };
    let angle = f64::from(unit(hash_cell(1, 2, isl ^ 0x5417)) * std::f32::consts::TAU);
    let heading = (angle.cos(), angle.sin());
    let center = (
        f64::from(ISLAND_COLUMNS) * 0.5 - f64::from(ISLAND_ROWS) * 0.25,
        f64::from(ISLAND_ROWS) * 0.5,
    );
    let mut ship: Option<usize> = None;
    let mut best = f64::MIN;
    for index in 0..count {
        if cells[index].land
            || cells[index].offshore != ship_depth
            || terrain[index] != Terrain::Water
        {
            continue;
        }
        let (q, r) = axial_of_index(index);
        let (dx, dy) = (f64::from(q) - center.0, f64::from(r) - center.1);
        let along = dx * heading.0 + dy * heading.1 * 1.4;
        if along > best {
            best = along;
            ship = Some(index);
        }
    }
    let ship_at = ship.map(axial_of_index);
    if let Some(index) = ship {
        placer.try_place(index, Feature::Ship, 1);
    }
    let from_ship = |index: usize| ship_at.map_or(20, |at| hex_distance(axial_of_index(index), at));

    let flat: Vec<usize> = (0..count)
        .filter(|index| {
            cells[*index].land
                && terrain[*index].is_flat_land()
                && terrain[*index] != Terrain::Beach
        })
        .collect();

    // The goal: far from the ship, inland, on open ground.
    let goal = if matches!(world.kind, MapKind::Savanna | MapKind::Desert) {
        Feature::Pyramid
    } else {
        Feature::Temple
    };
    let mut goal_candidates: Vec<(usize, f32)> = flat
        .iter()
        .filter(|index| cells[**index].coast >= 3)
        .map(|index| {
            (
                *index,
                from_ship(*index) as f32
                    + 6.0 * score(*index, 1)
                    + cells[*index].coast as f32 * 0.8,
            )
        })
        .collect();
    placer.place_best(&mut goal_candidates, goal, 1, 1);

    // A base camp within sight of the ship.
    let mut near_ship: Vec<(usize, f32)> = (0..count)
        .filter(|index| {
            cells[*index].land
                && terrain[*index].is_flat_land()
                && (2..=4).contains(&from_ship(*index))
        })
        .map(|index| (index, score(index, 2)))
        .collect();
    placer.place_best(&mut near_ship, Feature::Camp, 1, 2);

    // Villages sit near water, a few tiles inland.
    let mut village_candidates: Vec<(usize, f32)> = flat
        .iter()
        .filter(|index| (2..=7).contains(&cells[**index].coast))
        .map(|index| {
            (
                *index,
                score(*index, 3) + if cells[*index].coast <= 4 { 0.35 } else { 0.0 },
            )
        })
        .collect();
    placer.place_best(&mut village_candidates, Feature::Village, amount(6.0), 4);

    let mut ruin_candidates: Vec<(usize, f32)> = (0..count)
        .filter(|index| {
            cells[*index].land
                && cells[*index].coast >= 3
                && (terrain[*index].is_flat_land() || terrain[*index] == Terrain::Hills)
        })
        .map(|index| (index, score(index, 4)))
        .collect();
    placer.place_best(&mut ruin_candidates, Feature::Ruins, amount(3.0), 4);

    let rough: Vec<usize> = (0..count)
        .filter(|index| {
            cells[*index].land && terrain[*index].is_rough_land() && cells[*index].coast >= 2
        })
        .collect();
    let mut cave_candidates: Vec<(usize, f32)> = rough
        .iter()
        .filter(|index| {
            matches!(
                terrain[**index],
                Terrain::Hills | Terrain::Mountain | Terrain::Mesa
            )
        })
        .map(|index| (*index, score(*index, 5)))
        .collect();
    placer.place_best(&mut cave_candidates, Feature::Cave, amount(3.0), 3);
    let mut mine_candidates: Vec<(usize, f32)> = rough
        .iter()
        .map(|index| (*index, score(*index, 6)))
        .collect();
    placer.place_best(&mut mine_candidates, Feature::Mine, amount(2.0), 4);

    let mut shrine_candidates: Vec<(usize, f32)> = (0..count)
        .filter(|index| {
            cells[*index].land
                && cells[*index].coast >= 3
                && matches!(
                    terrain[*index],
                    Terrain::Hills
                        | Terrain::Forest
                        | Terrain::Rock
                        | Terrain::Jungle
                        | Terrain::Pines
                )
        })
        .map(|index| (index, score(index, 7)))
        .collect();
    placer.place_best(&mut shrine_candidates, Feature::Shrine, amount(3.0), 3);

    let mut camp_candidates: Vec<(usize, f32)> = (0..count)
        .filter(|index| {
            cells[*index].land
                && cells[*index].coast >= 2
                && (terrain[*index].is_flat_land() || terrain[*index] == Terrain::Hills)
        })
        .map(|index| (index, score(index, 8)))
        .collect();
    placer.place_best(&mut camp_candidates, Feature::Camp, amount(4.0), 3);
}

/// Memoises generated islands.
#[derive(Default)]
pub struct Atlas {
    islands: HashMap<(MapKind, u32, u32, i32, i32), Island>,
}

/// Islands kept before the memo is flushed.
const ATLAS_LIMIT: usize = 48;

impl Atlas {
    pub fn tile(&mut self, world: &World, q: i32, r: i32) -> Tile {
        let (island_x, island_y, column, row) = locate(q, r);
        let key = (
            world.kind,
            world.seed,
            world.landmark_scale.to_bits(),
            island_x,
            island_y,
        );
        if !self.islands.contains_key(&key) {
            if self.islands.len() >= ATLAS_LIMIT {
                self.islands.clear();
            }
            self.islands
                .insert(key, generate_island(world, island_x, island_y));
        }
        self.islands[&key].tile(column, row)
    }
}
