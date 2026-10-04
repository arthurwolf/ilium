//! Bounded block-pattern evidence, not vanilla biome IDs or structure provenance.
//! Only complete, globally aligned 16x16 core tiles are classified. Edge strips
//! and content below the 24-cell cover band are explicitly outside this survey.
use super::{
    chunk::BlockState,
    surface::{self, Observation, Sample, SurfaceWindow, Work, MIN_Y},
};
use serde::{Deserialize, Serialize};
use std::mem::size_of;

pub const RULE_REVISION: u16 = 2;
const BAND: i32 = 24;
const TILE_CELLS: usize = 256;
const HARD_TARGETS: usize = 2304;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Kind {
    Biome,
    Structure,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Category {
    OpenGrassland,
    RootedWoodland,
    SnowySurface,
    DrySandySurface,
    VegetatedShore,
    RockyRelief,
    WeatheredMasonry,
    OrnamentalSandstone,
    PrismarineMasonry,
    /// Paired door or bed within connected observed construction; no provenance claim.
    DwellingLikeConstruction,
}
const CATEGORIES: [Category; 9] = [
    Category::OpenGrassland,
    Category::RootedWoodland,
    Category::SnowySurface,
    Category::DrySandySurface,
    Category::VegetatedShore,
    Category::RockyRelief,
    Category::WeatheredMasonry,
    Category::OrnamentalSandstone,
    Category::PrismarineMasonry,
];
impl Category {
    pub fn kind(self) -> Kind {
        match self {
            Self::WeatheredMasonry
            | Self::OrnamentalSandstone
            | Self::PrismarineMasonry
            | Self::DwellingLikeConstruction => Kind::Structure,
            _ => Kind::Biome,
        }
    }
    fn minima(self) -> (u16, u16) {
        match self {
            Self::OpenGrassland => (128, 8),
            Self::RootedWoodland => (32, 3),
            Self::SnowySurface => (64, 64),
            Self::DrySandySurface => (128, 4),
            Self::VegetatedShore => (32, 4),
            Self::RockyRelief => (64, 16),
            Self::WeatheredMasonry => (16, 8),
            Self::OrnamentalSandstone => (8, 4),
            Self::PrismarineMasonry => (8, 2),
            Self::DwellingLikeConstruction => (8, 1),
        }
    }
}
/// Ordered heuristic support levels, NEVER probabilities or naturalness claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Confidence {
    Supported,
    Corroborated,
}
/// Caller-assigned stable map identity; do not reuse a transient catalog index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MapId(pub [u8; 16]);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub map: MapId,
    pub generation: u64,
}
/// Generation is intentionally separate: reloading is not new target identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TargetKey {
    pub map: MapId,
    pub revision: u16,
    pub category: Category,
    pub tile: [i32; 2],
    pub anchor: [i32; 3],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Support {
    pub columns: u16,
    pub primary_columns: u16,
    pub secondary_columns: u16,
    pub secondary_sectors: u8,
    /// Woodland: up to four separated roots; masonry: up to four corners;
    /// shore: number of water columns cardinally adjacent to a dry bank.
    pub links: u16,
    pub minimum: [i32; 3],
    pub maximum: [i32; 3],
    /// Origin of the 16x16 evidence window. An 8-block shifted window may
    /// cross saved chunk seams; key.tile still names the anchor's chunk.
    pub origin: [i32; 2],
    /// Exact connected component, x fastest, then z, relative to origin.
    pub footprint: [u64; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Witness<'a> {
    pub block: Observation<'a>,
    /// A decoded water cell above this witness; not an alpha/visibility claim.
    pub water_above: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target<'a> {
    pub source: Source,
    pub key: TargetKey,
    pub confidence: Confidence,
    pub support: Support,
    pub anchor: Witness<'a>,
    pub corroboration: Witness<'a>,
    pub landmarks: [Option<Witness<'a>>; 4],
}
/// Owned, small planner handoff. Exact states remain borrowed in Target and in
/// the loader's retained chunks. Neither representation means evidence was shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetSummary {
    pub source: Source,
    pub key: TargetKey,
    pub confidence: Confidence,
    pub support: Support,
    pub corroboration: [i32; 3],
    pub landmarks: [Option<[i32; 3]>; 4],
    pub anchor_only_air_above: bool,
    pub anchor_water_above: bool,
}
impl Target<'_> {
    pub fn summary(&self) -> TargetSummary {
        TargetSummary {
            source: self.source,
            key: self.key,
            confidence: self.confidence,
            support: self.support,
            corroboration: self.corroboration.block.position,
            landmarks: self.landmarks.map(|value| value.map(|w| w.block.position)),
            anchor_only_air_above: self.anchor.block.only_air_above,
            anchor_water_above: self.anchor.water_above,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_tiles: usize,
    /// Shifted 16x16 construction windows, separately bounded from core tiles.
    pub max_construction_windows: usize,
    pub max_targets: usize,
    /// Additional probes, graph operations and geometry checks; surface::Work
    /// independently charges qualification/top_at and supplies cancellation.
    pub max_operations: usize,
    /// Report and target-vector capacity only; no state strings are copied.
    pub max_owned_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_tiles: 128,
            max_construction_windows: 512,
            max_targets: 512,
            max_operations: 8_000_000,
            max_owned_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub tiles: usize,
    pub construction_windows: usize,
    pub columns: usize,
    pub edge_columns_not_surveyed: usize,
    pub empty_columns: usize,
    /// No eligible base in the 24-cell, domain-clipped cover band.
    pub band_limited_columns: usize,
    /// Columns stopped by unknown names or unsupported property schemas.
    pub unrecognized_columns: usize,
    pub qualifying_components: usize,
    pub operations: usize,
}
#[derive(Debug)]
pub struct Report<'a> {
    pub source: Source,
    pub targets: Vec<Target<'a>>,
    pub stats: Stats,
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error(transparent)]
    Surface(#[from] surface::Error),
    #[error("evidence limit: {0}")]
    Limit(&'static str),
}
struct Budget {
    used: usize,
    limit: usize,
}
impl Budget {
    fn tick(&mut self, work: &Work<'_>) -> Result<(), Error> {
        work.checkpoint()?;
        if self.used >= self.limit {
            return Err(Error::Limit("operations"));
        }
        self.used += 1;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Role {
    #[default]
    Unknown,
    Air,
    Soil,
    Grass,
    Sand,
    Rock,
    Gravel,
    Ice,
    Snow,
    Herb,
    Dry,
    Lily,
    Water,
    Log(u8),
    Leaves(u8),
    Gray(bool),
    Sandstone(u8),
    Prismarine(u8),
}
const BOOL: &[&str] = &["false", "true"];
const AGE: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15",
];
fn schema(state: &BlockState, fields: &[(&str, &[&str])]) -> bool {
    state.properties.len() == fields.len()
        && fields.iter().all(|(key, values)| {
            state
                .properties
                .get(*key)
                .is_some_and(|value| values.contains(&value.as_str()))
        })
}
fn role(state: &BlockState) -> Role {
    use Role::*;
    if state.is_air() {
        return Air;
    }
    let Some(name) = state.name.strip_prefix("minecraft:") else {
        return Unknown;
    };
    let woods = [
        "oak", "birch", "spruce", "jungle", "acacia", "dark_oak", "mangrove",
    ];
    for (index, wood) in woods.iter().enumerate() {
        if name.strip_suffix("_log") == Some(*wood) && schema(state, &[("axis", &["y"])]) {
            return Log(index as u8 + 1);
        }
        if name.strip_suffix("_leaves") != Some(*wood) {
            continue;
        }
        let basic = [
            ("distance", &["1", "2", "3", "4", "5", "6", "7"][..]),
            ("persistent", BOOL),
        ];
        let wet = [basic[0], basic[1], ("waterlogged", BOOL)];
        if schema(state, &basic) || schema(state, &wet) {
            return Leaves(index as u8 + 1);
        }
        return Unknown;
    }
    match name {
        "grass_block" if schema(state, &[("snowy", BOOL)]) => return Grass,
        "podzol" | "mycelium" if schema(state, &[("snowy", BOOL)]) => return Soil,
        "snow"
            if schema(
                state,
                &[("layers", &["1", "2", "3", "4", "5", "6", "7", "8"])],
            ) =>
        {
            return Snow
        }
        "cactus" if schema(state, &[("age", AGE)]) => return Dry,
        "water" if schema(state, &[("level", AGE)]) => return Water,
        "deepslate" if schema(state, &[("axis", &["x", "y", "z"])]) => return Rock,
        _ => {}
    }
    if !state.properties.is_empty() {
        return Unknown;
    }
    match name {
        "dirt" | "coarse_dirt" | "rooted_dirt" | "clay" | "mud" => Soil,
        "sand" | "red_sand" => Sand,
        "stone" | "andesite" | "diorite" | "granite" => Rock,
        "gravel" => Gravel,
        "ice" | "packed_ice" | "blue_ice" => Ice,
        "snow_block" => Snow,
        "grass" | "fern" | "dandelion" | "poppy" => Herb,
        "dead_bush" => Dry,
        "lily_pad" => Lily,
        "cobblestone" | "stone_bricks" | "bricks" => Gray(false),
        "mossy_cobblestone" | "mossy_stone_bricks" | "cracked_stone_bricks" => Gray(true),
        "sandstone" => Sandstone(0),
        "cut_sandstone" | "smooth_sandstone" => Sandstone(1),
        "chiseled_sandstone" => Sandstone(2),
        "prismarine" => Prismarine(0),
        "prismarine_bricks" | "dark_prismarine" => Prismarine(1),
        "sea_lantern" => Prismarine(2),
        _ => Unknown,
    }
}
fn family(role: Role) -> u8 {
    match role {
        Role::Gray(_) => 1,
        Role::Sandstone(_) => 2,
        Role::Prismarine(_) => 3,
        _ => 0,
    }
}
fn soil(role: Role) -> bool {
    matches!(role, Role::Soil | Role::Grass)
}
#[derive(Clone, Copy, Debug, Default)]
struct Column<'a> {
    top_y: Option<i16>,
    ground: Option<Observation<'a>>,
    role: Role,
    plant: Option<Observation<'a>>,
    plant_role: Role,
    canopy: Option<Observation<'a>>,
    canopy_species: u8,
    trunk: Option<Observation<'a>>,
    trunk_species: u8,
    rooted: bool,
    water: Option<Observation<'a>>,
    snow: Option<Observation<'a>>,
    snow_cover: bool,
    wall_depth: u8,
}
impl<'a> Column<'a> {
    fn witness(self, block: Observation<'a>) -> Witness<'a> {
        Witness {
            block,
            water_above: self
                .water
                .is_some_and(|water| water.position[1] > block.position[1]),
        }
    }
    fn height(self, category: Category) -> i32 {
        let observation = match category {
            Category::VegetatedShore => self.water.or(self.ground),
            Category::SnowySurface => self.snow.or(self.ground),
            _ => self.ground,
        };
        observation.map_or(MIN_Y, |block| block.position[1])
    }
}
fn probe<'a>(
    window: &SurfaceWindow<'a>,
    position: [i32; 3],
    top: i32,
    budget: &mut Budget,
    work: &Work<'_>,
) -> Result<Observation<'a>, Error> {
    budget.tick(work)?;
    let Sample::State(state) = window.sample(position) else {
        return Err(surface::Error::InconsistentSample(position).into());
    };
    Ok(Observation {
        position,
        state,
        only_air_above: position[1] == top,
    })
}
fn column<'a>(
    window: &SurfaceWindow<'a>,
    ground: [i32; 2],
    budget: &mut Budget,
    work: &mut Work<'_>,
    stats: &mut Stats,
) -> Result<Column<'a>, Error> {
    let mut value = Column::default();
    let Some(top) = window.top_at(ground, work)? else {
        stats.empty_columns += 1;
        return Ok(value);
    };
    value.top_y = Some(top.position[1] as i16);
    let (mut log_count, mut last_log, mut last_snow) = (0, None, None);
    for depth in 0..BAND {
        let y = top.position[1] - depth;
        if y < MIN_Y {
            break;
        }
        let block = probe(
            window,
            [ground[0], y, ground[1]],
            top.position[1],
            budget,
            work,
        )?;
        let found = role(block.state);
        if let Role::Log(species) = found {
            if last_log != Some(y + 1) || value.trunk_species != species {
                log_count = 0;
                value.trunk = Some(block);
                value.trunk_species = species;
            }
            log_count += 1;
            last_log = Some(y);
            continue;
        }
        match found {
            Role::Air => continue,
            Role::Water => {
                value.water.get_or_insert(block);
                continue;
            }
            Role::Leaves(species) => {
                if value.canopy.is_none() {
                    value.canopy = Some(block);
                    value.canopy_species = species;
                }
                continue;
            }
            Role::Herb | Role::Dry | Role::Lily => {
                value.plant = Some(block);
                value.plant_role = found;
                continue;
            }
            Role::Snow => {
                value.snow_cover = true;
                value.snow.get_or_insert(block);
                last_snow = Some(y);
                continue;
            }
            _ => {}
        }
        value.ground = Some(block);
        value.role = found;
        value.rooted = soil(found) && log_count >= 3 && last_log == Some(y + 1);
        let planted = value.plant.is_some_and(|plant| match value.plant_role {
            Role::Herb => soil(found) && plant.position[1] == y + 1,
            Role::Dry => found == Role::Sand && plant.position[1] == y + 1,
            Role::Lily => value
                .water
                .is_some_and(|water| plant.position[1] == water.position[1] + 1),
            _ => false,
        });
        if !planted {
            value.plant = None;
        }
        if last_snow != Some(y + 1)
            || !matches!(
                found,
                Role::Soil | Role::Grass | Role::Rock | Role::Gravel | Role::Ice
            )
        {
            value.snow = None;
        }
        if found == Role::Unknown {
            stats.unrecognized_columns += 1;
        }
        let group = family(found);
        if group == 0 {
            return Ok(value);
        }
        value.wall_depth = 1;
        for distance in 1..=2 {
            if y - distance < MIN_Y {
                break;
            }
            let below = probe(
                window,
                [ground[0], y - distance, ground[1]],
                top.position[1],
                budget,
                work,
            )?;
            if family(role(below.state)) != group {
                break;
            }
            value.wall_depth += 1;
        }
        return Ok(value);
    }
    // No base reached: this is a bounded survey miss, not missing/air data.
    stats.band_limited_columns += 1;
    Ok(Column {
        top_y: value.top_y,
        ..Column::default()
    })
}
fn neighbors(index: usize) -> [Option<usize>; 4] {
    let (x, z) = (index % 16, index / 16);
    [
        (x > 0).then(|| index - 1),
        (x < 15).then(|| index + 1),
        (z > 0).then(|| index - 16),
        (z < 15).then(|| index + 16),
    ]
}
fn included(mask: &[u64; 4], index: usize) -> bool {
    mask[index / 64] & (1_u64 << (index % 64)) != 0
}
fn insert(mask: &mut [u64; 4], index: usize) {
    mask[index / 64] |= 1_u64 << (index % 64);
}
fn member(category: Category, cell: Column<'_>) -> bool {
    if cell.ground.is_none() || cell.role == Role::Unknown {
        return false;
    }
    let dry = cell.water.is_none() && !cell.snow_cover;
    match category {
        Category::OpenGrassland => dry && cell.role == Role::Grass && cell.canopy.is_none(),
        Category::RootedWoodland => dry && soil(cell.role),
        Category::SnowySurface => cell.snow.is_some() || cell.role == Role::Ice,
        Category::DrySandySurface => dry && cell.role == Role::Sand,
        Category::VegetatedShore => !cell.snow_cover && (cell.water.is_some() || soil(cell.role)),
        Category::RockyRelief => dry && matches!(cell.role, Role::Rock | Role::Gravel),
        Category::WeatheredMasonry => family(cell.role) == 1,
        Category::OrnamentalSandstone => family(cell.role) == 2,
        Category::PrismarineMasonry => family(cell.role) == 3,
        Category::DwellingLikeConstruction => false,
    }
}
fn signals<'a>(
    category: Category,
    cell: Column<'a>,
) -> (Option<Observation<'a>>, Option<Observation<'a>>) {
    let select = |condition| if condition { cell.ground } else { None };
    match category {
        Category::OpenGrassland => (
            cell.ground,
            cell.plant.filter(|_| cell.plant_role == Role::Herb),
        ),
        Category::RootedWoodland => (cell.canopy, cell.trunk.filter(|_| cell.rooted)),
        Category::SnowySurface => (cell.snow, cell.ground.filter(|_| cell.snow.is_some())),
        Category::DrySandySurface => (
            cell.ground,
            cell.plant.filter(|_| cell.plant_role == Role::Dry),
        ),
        Category::VegetatedShore => (
            cell.water,
            cell.plant.filter(|_| cell.plant_role == Role::Lily),
        ),
        Category::RockyRelief => (
            select(cell.role == Role::Rock),
            select(cell.role == Role::Gravel),
        ),
        Category::WeatheredMasonry => (
            select(cell.role == Role::Gray(false)),
            select(cell.role == Role::Gray(true)),
        ),
        Category::OrnamentalSandstone => (
            select(cell.role == Role::Sandstone(1)),
            select(cell.role == Role::Sandstone(2)),
        ),
        Category::PrismarineMasonry => (
            select(cell.role == Role::Prismarine(1)),
            select(cell.role == Role::Prismarine(2)),
        ),
        Category::DwellingLikeConstruction => (None, None),
    }
}
fn rooted_canopy(
    cells: &[Column<'_>; TILE_CELLS],
    index: usize,
    budget: &mut Budget,
    work: &Work<'_>,
) -> Result<bool, Error> {
    let cell = cells[index];
    let Some(trunk) = cell.trunk.filter(|_| cell.rooted) else {
        return Ok(false);
    };
    let (x, z) = ((index % 16) as i32, (index / 16) as i32);
    let mut count = 0;
    for dz in -2..=2 {
        for dx in -2..=2 {
            budget.tick(work)?;
            if !(0..16).contains(&(x + dx)) || !(0..16).contains(&(z + dz)) {
                continue;
            }
            let other = cells[((z + dz) * 16 + x + dx) as usize];
            if !soil(other.role) || other.canopy_species != cell.trunk_species {
                continue;
            }
            if other.canopy.is_some_and(|leaf| {
                (trunk.position[1] - 1..=trunk.position[1] + 4).contains(&leaf.position[1])
            }) {
                count += 1;
            }
        }
    }
    Ok(count >= 8)
}
/// A common corner with two perpendicular length-four runs, all at one top Y,
/// each column verified to contain three consecutive same-family masonry cells.
fn corner(
    cells: &[Column<'_>; TILE_CELLS],
    mask: &[u64; 4],
    index: usize,
    budget: &mut Budget,
    work: &Work<'_>,
) -> Result<bool, Error> {
    let y = cells[index].height(Category::WeatheredMasonry);
    let (x, z) = ((index % 16) as i32, (index / 16) as i32);
    for sx in [-1, 1] {
        for sz in [-1, 1] {
            let mut valid = true;
            for distance in 0..4 {
                for (px, pz) in [(x + sx * distance, z), (x, z + sz * distance)] {
                    budget.tick(work)?;
                    if !(0..16).contains(&px) || !(0..16).contains(&pz) {
                        valid = false;
                        continue;
                    }
                    let other = (pz * 16 + px) as usize;
                    if !included(mask, other)
                        || cells[other].wall_depth < 3
                        || cells[other].height(Category::WeatheredMasonry) != y
                    {
                        valid = false;
                    }
                }
            }
            if valid {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn landmark<'a>(items: &mut [Option<Witness<'a>>; 4], value: Witness<'a>) {
    let p = value.block.position;
    if items.iter().flatten().any(|old| {
        let q = old.block.position;
        (i64::from(p[0]) - i64::from(q[0]))
            .abs()
            .max((i64::from(p[2]) - i64::from(q[2])).abs())
            < 4
    }) {
        return;
    }
    if let Some(slot) = items.iter_mut().find(|slot| slot.is_none()) {
        *slot = Some(value);
    }
}
fn target<'a>(
    category: Category,
    (tile, source): ([i32; 2], Source),
    cells: &[Column<'a>; TILE_CELLS],
    indices: &[usize],
    mask: [u64; 4],
    budget: &mut Budget,
    work: &Work<'_>,
) -> Result<Option<Target<'a>>, Error> {
    let structure = category.kind() == Kind::Structure;
    if indices.len() < if structure { 32 } else { 96 } {
        return Ok(None);
    }
    let mut support = Support {
        columns: indices.len() as u16,
        primary_columns: 0,
        secondary_columns: 0,
        secondary_sectors: 0,
        links: 0,
        minimum: [i32::MAX; 3],
        maximum: [i32::MIN; 3],
        origin: tile.map(|coordinate| coordinate * 16),
        footprint: mask,
    };
    let (mut anchor, mut corroboration) = (None, None);
    let mut landmarks = [None; 4];
    let (mut sectors, mut banks) = (0_u16, 0_u16);
    for &index in indices {
        budget.tick(work)?;
        let cell = cells[index];
        let (primary, secondary) = signals(category, cell);
        for block in cell.ground.into_iter().chain(primary).chain(secondary) {
            for (axis, coordinate) in block.position.into_iter().enumerate() {
                support.minimum[axis] = support.minimum[axis].min(coordinate);
                support.maximum[axis] = support.maximum[axis].max(coordinate);
            }
        }
        if let Some(block) = primary {
            support.primary_columns += 1;
            anchor.get_or_insert(cell.witness(block));
        }
        if let Some(block) = secondary {
            support.secondary_columns += 1;
            corroboration.get_or_insert(cell.witness(block));
            sectors |= 1 << ((index % 16) / 4 + (index / 16) / 4 * 4);
            if category == Category::RootedWoodland {
                landmark(&mut landmarks, cell.witness(block));
            }
        }
        if structure && corner(cells, &mask, index, budget, work)? {
            if let Some(block) = cell.ground {
                landmark(&mut landmarks, cell.witness(block));
            }
        }
        if category != Category::VegetatedShore {
            continue;
        }
        if cell.water.is_none() && soil(cell.role) {
            banks += 1;
        }
        if cell.water.is_some()
            && neighbors(index).into_iter().flatten().any(|other| {
                included(&mask, other) && cells[other].water.is_none() && soil(cells[other].role)
            })
        {
            support.links += 1;
        }
    }
    if structure || category == Category::RootedWoodland {
        support.links = landmarks.iter().flatten().count() as u16;
    }
    support.secondary_sectors = sectors.count_ones() as u8;
    let (minimum_primary, minimum_secondary) = category.minima();
    let span = if structure { 6 } else { 8 };
    if support.primary_columns < minimum_primary
        || support.secondary_columns < minimum_secondary
        || support.secondary_sectors < 2
        || [0, 2]
            .into_iter()
            .any(|axis| support.maximum[axis] - support.minimum[axis] + 1 < span)
        || (structure && support.links < 2)
        || (category == Category::RootedWoodland && support.links < 3)
        || (category == Category::VegetatedShore && (banks < 32 || support.links < 8))
        || (category == Category::RockyRelief && support.maximum[1] - support.minimum[1] < 4)
    {
        return Ok(None);
    }
    let (Some(anchor), Some(corroboration)) = (anchor, corroboration) else {
        return Ok(None);
    };
    let strong = support.primary_columns >= minimum_primary * 2
        && support.secondary_columns >= minimum_secondary * 2
        && support.secondary_sectors >= 3
        && (!(structure || category == Category::RootedWoodland) || support.links == 4);
    Ok(Some(Target {
        source,
        key: TargetKey {
            map: source.map,
            revision: RULE_REVISION,
            category,
            tile,
            anchor: anchor.block.position,
        },
        confidence: if strong {
            Confidence::Corroborated
        } else {
            Confidence::Supported
        },
        support,
        anchor,
        corroboration,
        landmarks,
    }))
}
/// Transactional: no successful partial report on cancellation, missing data,
/// allocation failure or budget exhaustion. Largest qualifying component per
/// tile/category wins, with stable anchor ties. No history is mutated here.
pub fn analyze<'a>(
    window: &SurfaceWindow<'a>,
    source: Source,
    limits: Limits,
    work: &mut Work<'_>,
) -> Result<Report<'a>, Error> {
    work.checkpoint()?;
    let core = window.core();
    let lower = core
        .minimum
        .map(|value| (i64::from(value) + 15).div_euclid(16));
    let upper = core
        .maximum
        .map(|value| (i64::from(value) - 15).div_euclid(16));
    let count =
        (upper[0] - lower[0] + 1).max(0) as usize * (upper[1] - lower[1] + 1).max(0) as usize;
    if count > limits.max_tiles.min(256) {
        return Err(Error::Limit("tiles"));
    }
    if limits.max_targets > HARD_TARGETS {
        return Err(Error::Limit("target configuration"));
    }
    let top_bytes = count
        .checked_mul(TILE_CELLS)
        .and_then(|cells| cells.checked_mul(size_of::<i16>()))
        .ok_or(Error::Limit("top grid bytes"))?;
    let storage = |capacity: usize| {
        capacity
            .checked_mul(size_of::<Target<'a>>())
            .and_then(|bytes| bytes.checked_add(size_of::<Report<'a>>()))
            .and_then(|bytes| bytes.checked_add(top_bytes))
    };
    if storage(limits.max_targets).is_none_or(|bytes| bytes > limits.max_owned_bytes) {
        return Err(Error::Limit("owned bytes"));
    }
    window.require_complete(work)?;
    let mut targets = Vec::new();
    targets
        .try_reserve_exact(limits.max_targets)
        .map_err(|_| Error::Limit("allocation"))?;
    if storage(targets.capacity()).is_none_or(|bytes| bytes > limits.max_owned_bytes) {
        return Err(Error::Limit("capacity bytes"));
    }
    let mut tops = Vec::new();
    tops.try_reserve_exact(count * TILE_CELLS)
        .map_err(|_| Error::Limit("top grid allocation"))?;
    if tops
        .capacity()
        .checked_mul(size_of::<i16>())
        .and_then(|bytes| {
            storage(targets.capacity())
                .and_then(|total| total.checked_sub(top_bytes)?.checked_add(bytes))
        })
        .is_none_or(|bytes| bytes > limits.max_owned_bytes)
    {
        return Err(Error::Limit("top grid capacity"));
    }
    tops.resize(count * TILE_CELLS, i16::MIN);
    let top_width = (upper[0] - lower[0] + 1).max(0) as usize * 16;
    let columns = (i64::from(core.maximum[0]) - i64::from(core.minimum[0]) + 1) as usize
        * (i64::from(core.maximum[1]) - i64::from(core.minimum[1]) + 1) as usize;
    let mut stats = Stats {
        edge_columns_not_surveyed: columns - count * TILE_CELLS,
        ..Stats::default()
    };
    let mut budget = Budget {
        used: 0,
        limit: limits.max_operations.min(64_000_000),
    };
    for tz in lower[1]..=upper[1] {
        for tx in lower[0]..=upper[0] {
            budget.tick(work)?;
            let tile = [tx as i32, tz as i32];
            let mut cells = [Column::default(); TILE_CELLS];
            for (index, cell) in cells.iter_mut().enumerate() {
                *cell = column(
                    window,
                    [
                        tile[0] * 16 + (index % 16) as i32,
                        tile[1] * 16 + (index / 16) as i32,
                    ],
                    &mut budget,
                    work,
                    &mut stats,
                )?;
                let grid_x = (tx - lower[0]) as usize * 16 + index % 16;
                let grid_z = (tz - lower[1]) as usize * 16 + index / 16;
                tops[grid_z * top_width + grid_x] = cell.top_y.unwrap_or(i16::MIN);
            }
            for index in 0..TILE_CELLS {
                budget.tick(work)?;
                cells[index].rooted = rooted_canopy(&cells, index, &mut budget, work)?;
            }
            stats.tiles += 1;
            stats.columns += TILE_CELLS;
            for category in CATEGORIES {
                let mut visited = [0_u64; 4];
                let mut best: Option<Target<'a>> = None;
                for start in 0..TILE_CELLS {
                    budget.tick(work)?;
                    if included(&visited, start) || !member(category, cells[start]) {
                        continue;
                    }
                    let mut queue = [0_usize; TILE_CELLS];
                    let (mut head, mut length) = (0, 1);
                    let mut mask = [0_u64; 4];
                    queue[0] = start;
                    insert(&mut visited, start);
                    insert(&mut mask, start);
                    while head < length {
                        budget.tick(work)?;
                        let index = queue[head];
                        head += 1;
                        for other in neighbors(index).into_iter().flatten() {
                            if included(&visited, other)
                                || !member(category, cells[other])
                                || (cells[index].height(category) - cells[other].height(category))
                                    .abs()
                                    > 4
                            {
                                continue;
                            }
                            // Mark before enqueue: at most 256 distinct slots.
                            insert(&mut visited, other);
                            insert(&mut mask, other);
                            queue[length] = other;
                            length += 1;
                        }
                    }
                    let candidate = target(
                        category,
                        (tile, source),
                        &cells,
                        &queue[..length],
                        mask,
                        &mut budget,
                        work,
                    )?;
                    let Some(candidate) = candidate else { continue };
                    stats.qualifying_components += 1;
                    if best.as_ref().is_none_or(|old| {
                        candidate.support.columns > old.support.columns
                            || (candidate.support.columns == old.support.columns
                                && candidate.key < old.key)
                    }) {
                        best = Some(candidate);
                    }
                }
                let Some(best) = best else { continue };
                if targets.len() >= limits.max_targets {
                    return Err(Error::Limit("targets"));
                }
                targets.push(best);
            }
        }
    }
    construction::append_targets(
        construction::Input {
            window,
            source,
            lower,
            upper,
            tops: &tops,
            width: top_width,
            limits,
        },
        &mut targets,
        &mut stats,
        &mut budget,
        work,
    )?;
    stats.operations = budget.used;
    work.checkpoint()?;
    Ok(Report {
        source,
        targets,
        stats,
    })
}

/// Distinct authentic construction appearances for a classified dwelling's
/// displayed exterior. Zero excludes natural ground and fluid-only wet solids.
pub(super) fn construction_appearance_material(state: &BlockState) -> u8 {
    construction::appearance_material(state)
}

#[path = "construction.rs"]
mod construction;

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
