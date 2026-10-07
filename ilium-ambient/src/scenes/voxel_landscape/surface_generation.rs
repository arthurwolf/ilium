//! Bounded generated-surface world with globally owned semantic states.
//! Terrain/biome placement is an authored homage; source IDs and explicit
//! properties are retained. No underground rooms or underwater features.
use super::{
    assets::{
        block_state::BlockState,
        error::{AssetError, Result},
        identity::ResourceId,
    },
    noise::hash2,
    settings::VoxelLandscapeSettings,
    surface_biome_selector,
    surface_biomes::{OrganismSource, SurfaceBiome, SURFACE_AZALEA_INDICATOR_CONFIGURATION_ID},
    surface_camp_assembly,
    surface_context::{self, ContextEvent, SceneAtmosphere},
    surface_entities::{self, AtlasLayout, ClimateSkin, Model, Species},
    surface_events,
    surface_flora::{
        AdmissionError, FloraCell, FloraPlacement, FloraState, HabitatCell, PlantCandidate, Support,
    },
    surface_flora_vocabulary::{FloraEntry, ENTRIES},
    surface_fluid::FluidCell,
    surface_geology, surface_landmark_assembly, surface_ruin_assembly, surface_village_assembly,
    terrain_fields::{TerrainFields, TerrainSample},
    tree_decoration_profiles,
    tree_decorations::{DecorationOperation, DecorationPlanner, SurroundingCell},
    tree_forms::{self, Growth},
    tree_geometry::{LogAxis, TreeCell, TreeGeometry},
    tree_profiles::{self, TreeShape},
    village_kit,
};
use crate::control::SceneSettings;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub minimum: [i32; 2],
    pub maximum: [i32; 2],
}
impl Region {
    pub fn contains(self, position: [i32; 3]) -> bool {
        (0..2)
            .all(|axis| position[axis] >= self.minimum[axis] && position[axis] < self.maximum[axis])
    }
    pub fn validate(self) -> Result<()> {
        if (0..2).any(|axis| {
            self.minimum[axis] >= self.maximum[axis]
                || i64::from(self.maximum[axis]) - i64::from(self.minimum[axis]) > 128
                || self.minimum[axis] < i32::MIN + 256
                || self.maximum[axis] > i32::MAX - 256
        }) {
            return Err(AssetError::InvalidMetadata(
                "surface region outside bounded domain".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceOwner {
    /// Exact retained Java cell, before renderer-axis conversion. This carries
    /// no generated terrain, feature or player-authorship attribution.
    Saved {
        java_position: [i32; 3],
    },
    Terrain {
        biome: SurfaceBiome,
    },
    Geology {
        anchor: [i32; 3],
        source: &'static str,
    },
    Tree {
        anchor: [i32; 3],
        configuration: &'static str,
    },
    TreeDecoration {
        anchor: [i32; 3],
        configuration: &'static str,
    },
    Flora {
        anchor: [i32; 3],
        prescription: &'static str,
    },
    Structure {
        anchor: [i32; 3],
        source: String,
    },
}
#[derive(Clone, Debug)]
pub struct SurfaceBlock {
    pub state: BlockState,
    pub owner: SourceOwner,
}
#[derive(Clone, Debug)]
pub struct FeatureRecord {
    pub anchor: [i32; 3],
    pub source: &'static str,
    pub projected_cells: usize,
    /// This describes generation semantics, never claims native visual parity.
    pub authored_placement: bool,
}
pub struct SurfaceEntity {
    pub species: Species,
    pub anchor: [i32; 3],
    pub model: Model,
    pub atlas_status: &'static str,
}
/// A generated viewport may contain only selected tile cores. `region` is its
/// bounding rectangle for mesh culling and identity, never a density promise.
/// `columns`/`biomes` keys are the exact prepared XY coverage; outside them is
/// unknown, not authored air. Saved worlds intentionally have no generated
/// column map, and their separate adapter uses the supplied block positions.
pub struct SurfaceWorld {
    pub region: Region,
    pub seed: u64,
    pub columns: BTreeMap<[i32; 2], TerrainSample>,
    pub biomes: BTreeMap<[i32; 2], SurfaceBiome>,
    pub blocks: BTreeMap<[i32; 3], SurfaceBlock>,
    pub fluids: BTreeMap<[i32; 3], FluidCell>,
    pub trees: Vec<FeatureRecord>,
    pub flora: Vec<FeatureRecord>,
    pub structures: Vec<FeatureRecord>,
    pub entities: Vec<SurfaceEntity>,
    pub source_limitations: Vec<&'static str>,
}
impl SurfaceWorld {
    /// True only when generated terrain for this XY column was prepared.
    pub fn generated_column_covered(&self, xy: [i32; 2]) -> bool {
        self.columns.contains_key(&xy) && self.biomes.contains_key(&xy)
    }
}

/// Guard the projection invariant on actual emitted source. Future authored
/// variants fail visibly here instead of disappearing beyond the camera sweep.
fn validate_generated_height(world: &SurfaceWorld) -> Result<()> {
    let inside = |z: f64| {
        (super::surface_viewport::SOURCE_Z_MIN..super::surface_viewport::SOURCE_Z_MAX).contains(&z)
    };
    for position in world.blocks.keys().chain(world.fluids.keys()) {
        if !inside(f64::from(position[2])) {
            return Err(AssetError::InvalidMetadata(
                "generated source exceeds viewport height envelope".into(),
            ));
        }
    }
    for entity in &world.entities {
        let (minimum, maximum) = entity.model.bounds().ok_or_else(|| {
            AssetError::InvalidMetadata("generated entity has no finite silhouette bounds".into())
        })?;
        let low = f64::from(entity.anchor[2]) + f64::from(minimum[2]);
        let high = f64::from(entity.anchor[2]) + f64::from(maximum[2]);
        if !inside(low) || high > super::surface_viewport::SOURCE_Z_MAX {
            return Err(AssetError::InvalidMetadata(
                "generated entity exceeds viewport height envelope".into(),
            ));
        }
    }
    Ok(())
}
fn state(id: &str, properties: impl IntoIterator<Item = (String, String)>) -> Result<BlockState> {
    // Validate supplied properties before expanding compact generated-provider
    // defaults, preserving duplicate-property errors and explicit variants.
    let state = BlockState::new(ResourceId::parse(id)?, properties)?;
    if id != "minecraft:leaf_litter" {
        return Ok(state);
    }
    // The compact leaf-litter provider entry is the north/one-segment member
    // omitted from its otherwise explicit facing/amount inventory. This is
    // generated-world normalization; saved source states remain untouched.
    let mut properties = state.properties().clone();
    properties
        .entry("facing".to_owned())
        .or_insert_with(|| "north".to_owned());
    properties
        .entry("segment_amount".to_owned())
        .or_insert_with(|| "1".to_owned());
    BlockState::new(state.id().clone(), properties)
}
fn plain(id: &str) -> Result<BlockState> {
    state(id, Vec::<(String, String)>::new())
}
fn terrain_state(id: &str) -> Result<BlockState> {
    if matches!(id, "minecraft:grass_block" | "minecraft:podzol") {
        return state(id, [("snowy".to_owned(), "false".to_owned())]);
    }
    plain(id)
}
fn sample_ground(
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    x: i32,
    y: i32,
) -> (TerrainSample, SurfaceBiome) {
    let sample = fields.sample(x, y, settings.rivers);
    let biome = surface_biome_selector::select(u64::from(settings.seed), [x, y], sample);
    (sample, biome)
}
fn habitat(
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    position: [i32; 3],
) -> HabitatCell {
    let [x, y, z] = position;
    if !(0..=256).contains(&z)
        || x < i32::MIN + 256
        || y < i32::MIN + 256
        || x > i32::MAX - 256
        || y > i32::MAX - 256
    {
        return HabitatCell::Unknown;
    }
    let (sample, biome) = sample_ground(fields, settings, x, y);
    let ground = i32::from(sample.height) - 1;
    if z > ground {
        return if sample.water_level.is_some_and(|level| z < i32::from(level)) {
            HabitatCell::Water
        } else {
            HabitatCell::Air
        };
    }
    if z == ground {
        surface_geology::terrain_materials(u64::from(settings.seed), biome, [x, y], ground)
            .top_habitat()
    } else {
        HabitatCell::Solid
    }
}
fn add_global(anchor: [i32; 3], offset: [i16; 3]) -> Result<[i32; 3]> {
    Ok([
        anchor[0]
            .checked_add(i32::from(offset[0]))
            .ok_or_else(|| AssetError::InvalidMetadata("feature coordinate overflow".into()))?,
        anchor[1]
            .checked_add(i32::from(offset[1]))
            .ok_or_else(|| AssetError::InvalidMetadata("feature coordinate overflow".into()))?,
        anchor[2]
            .checked_add(i32::from(offset[2]))
            .ok_or_else(|| AssetError::InvalidMetadata("feature coordinate overflow".into()))?,
    ])
}

#[derive(Clone, Copy)]
struct TreeTerrainColumn {
    /// The first air cell above authored solid terrain.
    solid_top: i32,
    /// The first air cell above authored water, when present.
    water_top: Option<i32>,
}

/// Decide admission from the entire global candidate, before any region writes.
/// Sampling is cached by column: a wide crown does not resample climate for
/// every leaf. A rejected tree contributes neither cells nor decorators/halo.
fn tree_terrain_admits(
    anchor: [i32; 3],
    shape: TreeShape,
    geometry: &TreeGeometry,
    mut sample_column: impl FnMut([i32; 2]) -> TreeTerrainColumn,
    cancelled: impl Fn() -> bool,
) -> Result<bool> {
    let mut columns = BTreeMap::<[i32; 2], TreeTerrainColumn>::new();
    let mut fallen_logs = Vec::<(i32, i32)>::new(); // (along trunk, air gap below)
    let mut root_tips = 0;
    let mut grounded_root_tips = 0;
    for (offset, cell) in geometry.cells() {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        let position = add_global(anchor, offset)?;
        let xy = [position[0], position[1]];
        let column = *columns.entry(xy).or_insert_with(|| sample_column(xy));
        let clearance_top = column
            .water_top
            .unwrap_or(column.solid_top)
            .max(column.solid_top);
        match cell {
            TreeCell::Root if shape == TreeShape::Mangrove => {
                // Mangrove props may occupy shallow substrate or water. Their
                // endpoints must still reach a bank or the water surface.
                if position[2] < column.solid_top - 3 {
                    return Ok(false);
                }
                if offset[2] == 0 {
                    root_tips += 1;
                    if position[2] > clearance_top + 2 {
                        return Ok(false);
                    }
                    if position[2] <= clearance_top {
                        grounded_root_tips += 1;
                    }
                }
            }
            TreeCell::Root => return Ok(false),
            TreeCell::Log(axis) => {
                if position[2] < clearance_top {
                    return Ok(false);
                }
                // Broad upright stems may straddle an ordinary one-cell
                // downslope, but no basal column may hang above a deeper drop.
                // Mangroves instead use the explicit prop-root rule above.
                if shape != TreeShape::Mangrove
                    && axis == LogAxis::Vertical
                    && offset[2] == 0
                    && position[2] - column.solid_top > 1
                {
                    return Ok(false);
                }
                if shape == TreeShape::Fallen {
                    let along = match axis {
                        LogAxis::X => Some(position[0]),
                        LogAxis::GroundY => Some(position[1]),
                        LogAxis::Vertical => None, // the separate rooted stump
                    };
                    if let Some(along) = along {
                        fallen_logs.push((along, position[2] - column.solid_top));
                    }
                }
            }
            TreeCell::Leaf => {
                if position[2] < clearance_top {
                    return Ok(false);
                }
            }
        }
    }
    if shape == TreeShape::Mangrove {
        return Ok(root_tips > 0 && grounded_root_tips > 0);
    }
    if shape != TreeShape::Fallen {
        return Ok(true);
    }
    // An intact fallen beam may bridge two one-cell dips, or overhang a
    // one-cell dip at either end. Long, high or mostly unsupported beams are
    // refused as a whole; the stump alone is never published.
    fallen_logs.sort_unstable_by_key(|(along, _)| *along);
    let supported = fallen_logs.iter().filter(|(_, gap)| *gap == 0).count();
    if supported == 0 || supported * 2 < fallen_logs.len() {
        return Ok(false);
    }
    let front_gap = fallen_logs.iter().take_while(|(_, gap)| *gap > 0).count();
    let back_gap = fallen_logs
        .iter()
        .rev()
        .take_while(|(_, gap)| *gap > 0)
        .count();
    if front_gap > 1 || back_gap > 1 {
        return Ok(false);
    }
    let mut consecutive_gaps = 0;
    for (_, gap) in fallen_logs {
        if gap == 0 {
            consecutive_gaps = 0;
        } else {
            consecutive_gaps += 1;
            if gap > 1 || consecutive_gaps > 2 {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

// Authored bounded cover, informed by the differing pinned Java placement
// prescriptions. These percentages are not native chunk-attempt counts.
fn tree_cover_percent(biome: SurfaceBiome) -> u64 {
    use SurfaceBiome::*;
    match biome {
        BambooJungle | DarkForest | Jungle | PaleGarden => 85,
        BirchForest | DappledForest | Forest | SnowyTaiga | Taiga => 80,
        OldGrowthBirchForest | OldGrowthPineTaiga | OldGrowthSpruceTaiga => 75,
        MangroveSwamp | WoodedBadlands => 70,
        CherryGrove | FlowerForest => 60,
        Grove | Swamp => 55,
        WindsweptForest | MushroomFields => 45,
        SparseJungle => 30,
        Savanna | SavannaPlateau | WindsweptSavanna => 20,
        WindsweptGravellyHills | WindsweptHills | IceSpikes => 12,
        FrozenRiver | River => 8,
        Meadow => 5,
        Plains | SnowyPlains | SunflowerPlains => 2,
        Badlands | Beach | Desert | ErodedBadlands | FrozenPeaks | JaggedPeaks | SnowyBeach
        | SnowySlopes | StonyPeaks | StonyShore => 0,
    }
}

// These are authored ecological weights, not extracted native feature frequencies.
fn tree_configuration_weight(biome: SurfaceBiome, source: &str) -> u64 {
    let Some(profile) = tree_profiles::profile(source) else {
        return 0;
    };
    if profile.shape == TreeShape::Fallen {
        return 1;
    }
    if matches!(
        profile.shape,
        TreeShape::Bush | TreeShape::RedMushroom | TreeShape::BrownMushroom
    ) && biome != SurfaceBiome::MushroomFields
    {
        return 4;
    }
    match (biome, profile.shape, source) {
        (SurfaceBiome::DarkForest, TreeShape::Dense, _)
        | (SurfaceBiome::OldGrowthBirchForest, _, "minecraft:super_birch_bees_0002")
        | (SurfaceBiome::OldGrowthPineTaiga, TreeShape::GiantPine, _)
        | (SurfaceBiome::OldGrowthSpruceTaiga, TreeShape::GiantSpruce, _) => 48,
        (
            SurfaceBiome::OldGrowthPineTaiga | SurfaceBiome::OldGrowthSpruceTaiga,
            TreeShape::Pine | TreeShape::Spruce,
            _,
        ) => 8,
        _ => 16,
    }
}
// One interstitial candidate per existing tile supplies canopy between retained primary anchors.
fn tree_infill_enabled(biome: SurfaceBiome) -> bool {
    use SurfaceBiome::*;
    matches!(
        biome,
        BambooJungle
            | BirchForest
            | CherryGrove
            | DappledForest
            | DarkForest
            | FlowerForest
            | Forest
            | Grove
            | Jungle
            | MangroveSwamp
            | OldGrowthBirchForest
            | OldGrowthPineTaiga
            | OldGrowthSpruceTaiga
            | PaleGarden
            | SnowyTaiga
            | Swamp
            | Taiga
            | WindsweptForest
            | WoodedBadlands
    )
}

/// Keep foliage from swallowing authored landmarks. Structure admission is
/// global, so this check uses only the global XY footprint and remains stable
/// when the camera window is split or shifted. Trunks still use the exact-cell
/// collision check below; the wider halo applies only to crown cells because
/// those are what obscure roofs, paths, and village silhouettes.
fn tree_canopy_overlaps_structure_clearance(
    anchor: [i32; 3],
    states: &[tree_forms::TreeVoxelState],
    profile: &tree_profiles::TreeProfile,
    structure_xy: &BTreeSet<[i32; 2]>,
) -> bool {
    let radius = match profile.shape {
        TreeShape::Dense
        | TreeShape::GiantJungle
        | TreeShape::GiantPine
        | TreeShape::GiantSpruce => 3,
        _ => 2,
    };
    states
        .iter()
        .filter(|cell| {
            profile
                .crowns
                .iter()
                .any(|crown| crown.id == cell.resource_id)
        })
        .any(|cell| {
            let x = anchor[0] + i32::from(cell.position[0]);
            let y = anchor[1] + i32::from(cell.position[1]);
            (-radius..=radius)
                .any(|dx| (-radius..=radius).any(|dy| structure_xy.contains(&[x + dx, y + dy])))
        })
}

fn tree_configuration(biome: SurfaceBiome, entropy: u64) -> Option<&'static str> {
    // The supplemental surface tree is not an ordinary biome tree-table entry.
    // Rare woodland indicators are authored; no underground cave is inferred.
    let woodland = matches!(
        biome,
        SurfaceBiome::BambooJungle
            | SurfaceBiome::BirchForest
            | SurfaceBiome::DappledForest
            | SurfaceBiome::DarkForest
            | SurfaceBiome::FlowerForest
            | SurfaceBiome::Forest
            | SurfaceBiome::Jungle
            | SurfaceBiome::OldGrowthBirchForest
            | SurfaceBiome::SparseJungle
    );
    if woodland && entropy.rotate_left(7).is_multiple_of(128) {
        return Some(SURFACE_AZALEA_INDICATOR_CONFIGURATION_ID);
    }
    let configs = biome.descriptor().natural_tree_configuration_ids;
    if configs.is_empty() {
        return None;
    }
    let total_weight: u64 = configs
        .iter()
        .map(|source| tree_configuration_weight(biome, source))
        .sum();
    if total_weight == 0 {
        return None;
    }
    let mut choice = hash2(entropy ^ 0x7472_6565_5f63_6667, 0, 0) % total_weight;
    for &source in configs {
        let weight = tree_configuration_weight(biome, source);
        if choice < weight {
            return Some(source);
        }
        choice -= weight;
    }
    None
}

fn flora_selection_weight(biome: SurfaceBiome, id: &str) -> u64 {
    use SurfaceBiome::*;

    let arid = matches!(
        biome,
        Badlands
            | Desert
            | ErodedBadlands
            | Savanna
            | SavannaPlateau
            | WindsweptSavanna
            | WoodedBadlands
    );
    let flower_rich = matches!(biome, FlowerForest | Meadow | SunflowerPlains);
    let riparian = matches!(biome, River | FrozenRiver | Swamp | MangroveSwamp);
    match id {
        "minecraft:short_grass" => 12,
        "minecraft:tall_grass" | "minecraft:large_fern" => 4,
        "minecraft:fern" => {
            if matches!(
                biome,
                BambooJungle
                    | Jungle
                    | OldGrowthPineTaiga
                    | OldGrowthSpruceTaiga
                    | SnowyTaiga
                    | Taiga
            ) {
                8
            } else {
                2
            }
        }
        "minecraft:short_dry_grass" => u64::from(arid) * 10 + 1,
        "minecraft:tall_dry_grass" => u64::from(arid) * 4 + 1,
        "minecraft:dead_bush" => u64::from(arid) * 6 + 1,
        "minecraft:cactus" => u64::from(arid) * 4 + 1,
        "minecraft:cactus_flower" => u64::from(arid) * 2 + 1,
        "minecraft:bamboo" => 6,
        "minecraft:sugar_cane" | "minecraft:lily_pad" => {
            if riparian {
                8
            } else {
                4
            }
        }
        "minecraft:pumpkin" | "minecraft:melon" => 2,
        "minecraft:sweet_berry_bush" => 3,
        "minecraft:brown_mushroom" | "minecraft:red_mushroom" => {
            if biome == MushroomFields {
                5
            } else {
                1
            }
        }
        "minecraft:brown_mushroom_block"
        | "minecraft:red_mushroom_block"
        | "minecraft:mushroom_stem"
        | "minecraft:shelf_mushroom" => u64::from(biome == MushroomFields) * 4 + 1,
        "minecraft:leaf_litter" => {
            u64::from(matches!(biome, Forest | Taiga | OldGrowthBirchForest)) * 4 + 1
        }
        "minecraft:moss_carpet" => {
            u64::from(matches!(
                biome,
                Jungle | BambooJungle | Swamp | MangroveSwamp
            )) * 4
                + 1
        }
        "minecraft:pale_moss_block"
        | "minecraft:pale_moss_carpet"
        | "minecraft:pale_hanging_moss" => u64::from(biome == PaleGarden) * 5 + 1,
        "minecraft:allium"
        | "minecraft:azure_bluet"
        | "minecraft:blue_orchid"
        | "minecraft:cornflower"
        | "minecraft:dandelion"
        | "minecraft:closed_eyeblossom"
        | "minecraft:firefly_bush"
        | "minecraft:lilac"
        | "minecraft:lily_of_the_valley"
        | "minecraft:orange_tulip"
        | "minecraft:oxeye_daisy"
        | "minecraft:peony"
        | "minecraft:pink_petals"
        | "minecraft:pink_tulip"
        | "minecraft:poppy"
        | "minecraft:red_shrub"
        | "minecraft:red_tulip"
        | "minecraft:rose_bush"
        | "minecraft:sunflower"
        | "minecraft:white_tulip"
        | "minecraft:wildflowers" => {
            if flower_rich {
                7
            } else {
                2
            }
        }
        _ => 1,
    }
}

fn choose_flora_entry<'a>(
    candidates: &'a [&FloraEntry],
    biome: SurfaceBiome,
    entropy: u64,
) -> Option<&'a FloraEntry> {
    let total_weight: u64 = candidates
        .iter()
        .map(|entry| flora_selection_weight(biome, entry.id))
        .sum();
    if total_weight == 0 {
        return None;
    }
    let mut choice = entropy % total_weight;
    for entry in candidates {
        let weight = flora_selection_weight(biome, entry.id);
        if choice < weight {
            return Some(*entry);
        }
        choice -= weight;
    }
    None
}

fn pumpkin_patch_contains(seed: u64, x: i32, y: i32) -> bool {
    // Global ownership preserves patch membership across camera and region
    // boundaries, including negative coordinates. Most 64-cell tiles have none.
    let gx = x.div_euclid(64);
    let gy = y.div_euclid(64);
    let entropy = hash2(seed ^ 0x7075_6d70_6b69_6e73, i64::from(gx), i64::from(gy));
    if !entropy.is_multiple_of(24) {
        return false;
    }
    let center_x = 12 + (entropy.rotate_left(19) % 40) as i32;
    let center_y = 12 + (entropy.rotate_left(37) % 40) as i32;
    let dx = x.rem_euclid(64) - center_x;
    let dy = y.rem_euclid(64) - center_y;
    dx * dx + dy * dy <= 100
}

fn tree_growth(entropy: u64) -> Growth {
    // Stable authored age mixture, independent of camera and pack selection.
    match entropy.rotate_left(23) % 100 {
        0..=19 => Growth::Young,
        20..=83 => Growth::Mature,
        _ => Growth::Old,
    }
}
// Keep the existing authored bare-cactus height; a flowering candidate adds one
// crown cell without changing the stem's states or the natural selection table.
const CACTUS_HEIGHT: u8 = 2;

fn flower_candidate(anchor: [i32; 3], id: &'static str) -> PlantCandidate {
    let entropy = hash2(0x0063_6f76_6572, i64::from(anchor[0]), i64::from(anchor[1]));
    let facing = ["north", "east", "south", "west"][(entropy % 4) as usize];
    let amount = ["1", "2", "3", "4"][(entropy.rotate_left(17) % 4) as usize];
    let props = if matches!(id, "minecraft:cactus" | "minecraft:sugar_cane") {
        vec![("age", "0")]
    } else if id == "minecraft:sweet_berry_bush" {
        vec![("age", "3")]
    } else if id == "minecraft:closed_eyeblossom" {
        vec![("schedule_tick", "true")]
    } else if id == "minecraft:leaf_litter" {
        vec![("facing", facing), ("segment_amount", amount)]
    } else if matches!(id, "minecraft:pink_petals" | "minecraft:wildflowers") {
        vec![("facing", facing), ("flower_amount", amount)]
    } else if id == "minecraft:bamboo" {
        vec![("age", "1"), ("leaves", "none"), ("stage", "0")]
    } else if id == "minecraft:mangrove_propagule" {
        vec![
            ("age", "4"),
            ("hanging", "false"),
            ("stage", "0"),
            ("waterlogged", "false"),
        ]
    } else if id == "minecraft:pale_hanging_moss" {
        vec![("tip", "true")]
    } else if id == "minecraft:pale_moss_carpet" {
        vec![
            ("bottom", "true"),
            ("east", "none"),
            ("north", "none"),
            ("south", "none"),
            ("west", "none"),
        ]
    } else if matches!(
        id,
        "minecraft:brown_mushroom_block"
            | "minecraft:red_mushroom_block"
            | "minecraft:mushroom_stem"
    ) {
        vec![
            ("up", "true"),
            ("down", "true"),
            ("north", "true"),
            ("south", "true"),
            ("east", "true"),
            ("west", "true"),
        ]
    } else {
        Vec::new()
    };
    let state = FloraState {
        resource_id: id,
        properties: props,
    };
    let support = if id == "minecraft:cactus" || id == "minecraft:dead_bush" {
        Support::Sand
    } else if id == "minecraft:sugar_cane" {
        Support::WaterEdge
    } else if id == "minecraft:lily_pad" {
        Support::FloatingWater
    } else if matches!(id, "minecraft:short_dry_grass" | "minecraft:tall_dry_grass") {
        Support::SoilOrSand
    } else {
        Support::Soil
    };
    match id {
        "minecraft:tall_grass"
        | "minecraft:large_fern"
        | "minecraft:sunflower"
        | "minecraft:lilac"
        | "minecraft:rose_bush"
        | "minecraft:peony"
        | "minecraft:pitcher_plant" => PlantCandidate::tall(anchor, id, state, support),
        "minecraft:cactus" => {
            PlantCandidate::column(anchor, id, state, CACTUS_HEIGHT, support, true)
        }
        "minecraft:cactus_flower" => {
            // Retain this existing vocabulary slot and its entropy selection.
            // Its prescription owns both real age=0 cactus cells and the flower.
            // Rejection never falls back to a bare stem or a stand-alone flower.
            let mut candidate = flower_candidate(anchor, "minecraft:cactus");
            candidate.source_prescription = id;
            candidate.cells.push(FloraCell {
                position: [0, 0, i32::from(CACTUS_HEIGHT)],
                state,
            });
            candidate
        }
        // Both dry-grass resources occupy one cell. Neither uses a half property.
        "minecraft:short_dry_grass" | "minecraft:tall_dry_grass" => {
            PlantCandidate::single(anchor, id, state, support)
        }
        "minecraft:sugar_cane" => PlantCandidate::column(anchor, id, state, 3, support, false),
        "minecraft:bamboo" => {
            let mut candidate = PlantCandidate::column(anchor, id, state, 5, support, false);
            // Keep the woody lower stalk and the two leafy crown segments
            // distinct; one repeated state loses the characteristic silhouette.
            candidate.cells[3].state.properties =
                vec![("age", "1"), ("leaves", "small"), ("stage", "0")];
            candidate.cells[4].state.properties =
                vec![("age", "1"), ("leaves", "large"), ("stage", "1")];
            candidate
        }
        _ => PlantCandidate::single(anchor, id, state, support),
    }
}
/// Only lateral cactus probes outside the visible region need retained tree
/// occupancy. Every current flora candidate writes one XY column on the same
/// four-cell grid, so an outside anchor cannot write into, or reserve air in,
/// an inside anchor's column. Body cells and sand support remain local.
///
/// For a validated region there are at most 32 anchors along each of four edges
/// and three queried heights: at most 384 booleans, never a second tree mesh.
fn cactus_tree_halo(
    world: &SurfaceWorld,
    cancelled: impl Fn() -> bool,
) -> Result<BTreeMap<[i32; 3], bool>> {
    let mut probes = BTreeMap::new();
    for (&[x, y], sample) in &world.columns {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        if x.rem_euclid(4) != 2 || y.rem_euclid(4) != 2 || !world.region.contains([x, y, 0]) {
            continue;
        }
        let biome = world.biomes.get(&[x, y]).ok_or_else(|| {
            AssetError::InvalidMetadata("cactus clearance probe lost its biome".into())
        })?;
        if !ENTRIES.iter().any(|entry| {
            matches!(entry.id, "minecraft:cactus" | "minecraft:cactus_flower")
                && entry.generation_biomes.contains(&biome.id())
        }) {
            continue;
        }
        let anchor = [x, y, i32::from(sample.height)];
        for [dx, dy] in [[1_i16, 0], [-1, 0], [0, 1], [0, -1]] {
            let side = add_global(anchor, [dx, dy, 0])?;
            if world.region.contains(side) {
                continue;
            }
            for height in 0..=CACTUS_HEIGHT {
                probes.insert(add_global(side, [0, 0, i16::from(height)])?, false);
            }
        }
    }
    Ok(probes)
}

fn water_tint(biome: SurfaceBiome) -> [f32; 3] {
    let source = biome.descriptor().java_surface_water_rgb;
    source.map(super::assets::texture::srgb_byte_to_linear)
}

/// Authored visual skin grouping for surface biomes. Native variant-spawn
/// biome tags and weights are not supplied by this scene's biome catalogue.
fn fauna_climate(biome: SurfaceBiome) -> ClimateSkin {
    match biome {
        SurfaceBiome::Badlands
        | SurfaceBiome::BambooJungle
        | SurfaceBiome::Desert
        | SurfaceBiome::ErodedBadlands
        | SurfaceBiome::Jungle
        | SurfaceBiome::MangroveSwamp
        | SurfaceBiome::Savanna
        | SurfaceBiome::SavannaPlateau
        | SurfaceBiome::SparseJungle
        | SurfaceBiome::WindsweptSavanna
        | SurfaceBiome::WoodedBadlands => ClimateSkin::Warm,
        SurfaceBiome::FrozenPeaks
        | SurfaceBiome::FrozenRiver
        | SurfaceBiome::Grove
        | SurfaceBiome::IceSpikes
        | SurfaceBiome::JaggedPeaks
        | SurfaceBiome::SnowyBeach
        | SurfaceBiome::SnowyPlains
        | SurfaceBiome::SnowySlopes
        | SurfaceBiome::SnowyTaiga => ClimateSkin::Cold,
        _ => ClimateSkin::Temperate,
    }
}

fn first_global_fauna_grid(minimum: i32) -> i64 {
    let minimum = i64::from(minimum);
    (minimum + 15).div_euclid(16) * 16
}

/// Authored daylight and solid-ground admission, evaluated after structures,
/// trees and flora have written the final local surface snapshot. Exact native
/// spawn light/noise/group rules remain unresolved.
fn dry_surface_support(world: &SurfaceWorld, anchor: [i32; 3]) -> bool {
    let [x, y, feet] = anchor;
    let Some(biome) = world.biomes.get(&[x, y]).copied() else {
        return false;
    };
    let Some(sample) = world.columns.get(&[x, y]) else {
        return false;
    };
    if feet != i32::from(sample.height) {
        return false;
    }
    let Some(floor_z) = feet.checked_sub(1) else {
        return false;
    };
    let Some(support) = world.blocks.get(&[x, y, floor_z]) else {
        return false;
    };
    let expected = surface_geology::terrain_materials(world.seed, biome, [x, y], floor_z);
    if support.owner != (SourceOwner::Terrain { biome })
        || support.state.id().as_str() != expected.top
        || sample
            .water_level
            .is_some_and(|level| i32::from(level) > feet)
        || world.fluids.contains_key(&[x, y, floor_z])
        || world.fluids.contains_key(&anchor)
    {
        return false;
    }
    true
}

fn entity_clearance(world: &SurfaceWorld, anchor: [i32; 3], model: &Model) -> bool {
    let [x, y, feet] = anchor;
    let Some((minimum, maximum)) = model.bounds() else {
        return false;
    };
    if minimum
        .iter()
        .chain(maximum.iter())
        .any(|value| !value.is_finite())
        || minimum[2] < -0.01
    {
        return false;
    }
    let first_x = (f64::from(x) + 0.5 + f64::from(minimum[0])).floor() as i64;
    let last_x = (f64::from(x) + 0.5 + f64::from(maximum[0])).ceil() as i64;
    let first_y = (f64::from(y) + 0.5 + f64::from(minimum[1])).floor() as i64;
    let last_y = (f64::from(y) + 0.5 + f64::from(maximum[1])).ceil() as i64;
    let last_z = i64::from(feet) + (f64::from(maximum[2])).ceil() as i64;
    if first_x < i64::from(world.region.minimum[0])
        || last_x > i64::from(world.region.maximum[0])
        || first_y < i64::from(world.region.minimum[1])
        || last_y > i64::from(world.region.maximum[1])
        || last_z > i64::from(i32::MAX)
        || last_z <= i64::from(feet)
    {
        return false;
    }
    for cy in first_y..last_y {
        for cx in first_x..last_x {
            for cz in i64::from(feet)..last_z {
                let position = [cx as i32, cy as i32, cz as i32];
                if world.blocks.contains_key(&position) || world.fluids.contains_key(&position) {
                    return false;
                }
            }
        }
    }
    true
}

fn dry_surface_entity_site(world: &SurfaceWorld, anchor: [i32; 3], model: &Model) -> bool {
    dry_surface_support(world, anchor) && entity_clearance(world, anchor, model)
}
/// Generic biome monster tables also describe underground Slimes. The surface-only
/// scene admits their authored silhouettes only in the pinned surface habitat tag.
/// This is a biome filter, not native light, moon-phase or spawn-rate simulation.
fn surface_biome_allows_species(biome: SurfaceBiome, species: Species) -> bool {
    species != Species::Slime || matches!(biome, SurfaceBiome::Swamp | SurfaceBiome::MangroveSwamp)
}

fn natural_entity_site(
    world: &SurfaceWorld,
    anchor: [i32; 3],
    species: Species,
    model: &Model,
) -> bool {
    if !dry_surface_entity_site(world, anchor, model) {
        return false;
    }
    let [x, y, feet] = anchor;
    let Some((_, maximum)) = model.bounds() else {
        return false;
    };
    let last_z = i64::from(feet) + f64::from(maximum[2]).ceil() as i64;
    // Under foliage/roof is a bounded shade proxy; no clock or native light
    // engine exists in this scene. Daylight-exposed hostile candidates are
    // excluded instead of implying a vanilla daytime spawn.
    let covered = (last_z..last_z.saturating_add(12))
        .any(|z| z <= i64::from(i32::MAX) && world.blocks.contains_key(&[x, y, z as i32]));
    let hostile = matches!(
        species,
        Species::Bogged
            | Species::Creeper
            | Species::Drowned
            | Species::Enderman
            | Species::Husk
            | Species::Parched
            | Species::Phantom
            | Species::Skeleton
            | Species::Slime
            | Species::Spider
            | Species::Stray
            | Species::Witch
            | Species::Zombie
            | Species::ZombieVillager
            | Species::ZombifiedPiglin
    );
    !hostile || covered
}

/// Display one bee beside each accepted natural nest after every block writer
/// has finished. Nest occupancy, release timing and gameplay are not simulated.
fn populate_nest_bees(world: &mut SurfaceWorld, cancelled: impl Fn() -> bool) -> Result<()> {
    for (&position, block) in &world.blocks {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        if block.state.id().as_str() != "minecraft:bee_nest"
            || !matches!(block.owner, SourceOwner::TreeDecoration { .. })
        {
            continue;
        }
        let Some(biome) = world.biomes.get(&[position[0], position[1]]) else {
            continue;
        };
        if !biome
            .descriptor()
            .supplemental_organisms
            .iter()
            .any(|entry| {
                entry.organism_id == "minecraft:bee" && entry.source == OrganismSource::BeeNest
            })
        {
            continue;
        }
        let Some(facing) = block.state.properties().get("facing") else {
            continue;
        };
        let Some((anchor, model)) = surface_entities::bee_nest_occupant(position, facing) else {
            continue;
        };
        if world.entities.iter().any(|entity| entity.anchor == anchor)
            || !surface_entities::bee_nest_clearance(anchor, &model, |cell| {
                world.region.contains(cell)
                    && !world.blocks.contains_key(&cell)
                    && !world.fluids.contains_key(&cell)
            })
        {
            continue;
        }
        world.entities.push(SurfaceEntity {
            species: Species::Bee,
            anchor,
            model,
            atlas_status: "Original hovering bee at accepted natural nest; native occupancy/release timing is not simulated; selected/fallback atlas checked during binding",
        });
    }
    Ok(())
}

/// Admit a whole visitor party against the completed local surface. None of
/// its members are published when a companion lacks ground or clear space.
fn populate_trader_parties(world: &mut SurfaceWorld, cancelled: impl Fn() -> bool) -> Result<()> {
    let first = world
        .region
        .minimum
        .map(|value| value.div_euclid(surface_events::TRADER_GRID));
    let last = world
        .region
        .maximum
        .map(|value| (value - 1).div_euclid(surface_events::TRADER_GRID));
    for gy in first[1]..=last[1] {
        for gx in first[0]..=last[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            let Some(xy) = surface_events::trader_candidate(world.seed, [gx, gy]) else {
                continue;
            };
            let Some(sample) = world.columns.get(&xy) else {
                continue;
            };
            let Some(biome) = world.biomes.get(&xy).copied() else {
                continue;
            };
            let center = [xy[0], xy[1], i32::from(sample.height)];
            let Some(party) =
                surface_events::trader_party(center, fauna_climate(biome), |position| {
                    let ground = world.columns.get(&position)?;
                    ground
                        .water_level
                        .is_none_or(|water| water <= ground.height)
                        .then_some(i32::from(ground.height))
                })
            else {
                continue;
            };
            if !party.iter().all(|member| {
                natural_entity_site(world, member.anchor, member.species, &member.model)
                    && !world
                        .entities
                        .iter()
                        .any(|entity| entity.anchor == member.anchor)
            }) {
                continue;
            }
            for member in party {
                world.entities.push(SurfaceEntity {
                    species: member.species,
                    anchor: member.anchor,
                    model: member.model,
                    atlas_status: "Original static wandering trader and two companion llamas; no native trader timer/trading/lead gameplay; selected/fallback atlas checked during binding",
                });
            }
        }
    }
    Ok(())
}

/// A static raid party is owned by its real village anchor. Always use the
/// first global approach; choosing by visible-region collisions would change
/// the scene when the camera moves or the world is projected into fragments.
fn populate_village_raids(world: &mut SurfaceWorld, cancelled: impl Fn() -> bool) -> Result<()> {
    let villages: BTreeSet<_> = world
        .structures
        .iter()
        .filter(|feature| {
            matches!(
                feature.source,
                "minecraft:village_plains"
                    | "minecraft:village_desert"
                    | "minecraft:village_savanna"
                    | "minecraft:village_taiga"
                    | "minecraft:village_snowy"
            )
        })
        .map(|feature| feature.anchor)
        .collect();
    for village in villages {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        let Some(candidates) = surface_events::raid_candidates(world.seed, village) else {
            continue;
        };
        let xy = candidates[0];
        let Some(sample) = world.columns.get(&xy) else {
            continue;
        };
        let Some(biome) = world.biomes.get(&xy).copied() else {
            continue;
        };
        let center = [xy[0], xy[1], i32::from(sample.height)];
        let Some(party) = surface_events::raid_party(center, fauna_climate(biome), |position| {
            let ground = world.columns.get(&position)?;
            ground
                .water_level
                .is_none_or(|water| water <= ground.height)
                .then_some(i32::from(ground.height))
        }) else {
            continue;
        };
        if !party.iter().all(|member| {
            natural_entity_site(world, member.anchor, member.species, &member.model)
                && !world
                    .entities
                    .iter()
                    .any(|entity| entity.anchor == member.anchor)
        }) {
            continue;
        }
        for member in party {
            world.entities.push(SurfaceEntity {
                species:member.species,anchor:member.anchor,model:member.model,
                atlas_status:"Original static ravager and two pillagers near an accepted village; no raid waves/combat simulation; selected/fallback atlas checked during binding",
            });
        }
    }
    Ok(())
}

/// Apply final-world support and complete model clearance to global bank owners.
fn populate_riverbank_drowned(
    world: &mut SurfaceWorld,
    rivers: bool,
    atmosphere: SceneAtmosphere,
    cancelled: impl Fn() -> bool,
) -> Result<()> {
    if !atmosphere.is_night() || !rivers {
        return Ok(());
    }
    let first = world
        .region
        .minimum
        .map(|v| v.div_euclid(surface_context::RIVERBANK_GRID));
    let last = world
        .region
        .maximum
        .map(|v| (v - 1).div_euclid(surface_context::RIVERBANK_GRID));
    for gy in first[1]..=last[1] {
        for gx in first[0]..=last[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            let Some(xy) =
                surface_context::riverbank_candidate(world.seed, [gx, gy], atmosphere, rivers)
            else {
                continue;
            };
            let Some(sample) = world.columns.get(&xy) else {
                continue;
            };
            let Some(biome) = world.biomes.get(&xy).copied() else {
                continue;
            };
            let anchor = [xy[0], xy[1], i32::from(sample.height)];
            let model = surface_entities::model(
                Species::Drowned,
                AtlasLayout::Bedrock,
                fauna_climate(biome),
            );
            if !dry_surface_entity_site(world, anchor, &model)
                || world.entities.iter().any(|e| e.anchor == anchor)
            {
                continue;
            }
            world.entities.push(SurfaceEntity { species: Species::Drowned, anchor, model, atlas_status: "Static night riverbank walker on actual dry terrain; underwater spawning and gameplay not simulated; selected/fallback atlas checked during binding" });
        }
    }
    Ok(())
}

/// Phase-specific surface figures are generated only after every block writer.
/// All display episodes are static and deterministic, never native gameplay.
fn populate_contextual_fauna(
    world: &mut SurfaceWorld,
    atmosphere: SceneAtmosphere,
    cancelled: impl Fn() -> bool,
) -> Result<()> {
    if atmosphere == SceneAtmosphere::Day {
        return Ok(());
    }
    if atmosphere.is_night() {
        let hearts: Vec<_> = world
            .blocks
            .iter()
            .filter_map(|(&position, block)| {
                (block.state.id().as_str() == "minecraft:creaking_heart"
                    && matches!(block.owner, SourceOwner::TreeDecoration { .. })
                    && world.biomes.get(&[position[0], position[1]])
                        == Some(&SurfaceBiome::PaleGarden))
                .then_some(position)
            })
            .collect();
        for heart in hearts {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            if let Some(block) = world.blocks.get_mut(&heart) {
                let mut properties = block.state.properties().clone();
                properties.insert("creaking_heart_state".into(), "awake".into());
                block.state = BlockState::new(block.state.id().clone(), properties)?;
            }
            let Some(xy) = surface_context::creaking_candidate(world.seed, heart) else {
                continue;
            };
            if world.biomes.get(&xy) != Some(&SurfaceBiome::PaleGarden) {
                continue;
            }
            let Some(sample) = world.columns.get(&xy) else {
                continue;
            };
            let anchor = [xy[0], xy[1], i32::from(sample.height)];
            let model = surface_entities::model(
                Species::Creaking,
                AtlasLayout::Bedrock,
                ClimateSkin::Temperate,
            );
            if !dry_surface_entity_site(world, anchor, &model)
                || world.entities.iter().any(|e| e.anchor == anchor)
            {
                continue;
            }
            world.entities.push(SurfaceEntity {species:Species::Creaking,anchor,model,
                atlas_status:"Static night figure owned by accepted natural Pale Garden heart; native heart puppetry and combat are not simulated; selected/fallback atlas checked during binding"});
        }
    }
    let first = world
        .region
        .minimum
        .map(|v| v.div_euclid(surface_context::CONTEXT_GRID));
    let last = world
        .region
        .maximum
        .map(|v| (v - 1).div_euclid(surface_context::CONTEXT_GRID));
    for gy in first[1]..=last[1] {
        for gx in first[0]..=last[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            let Some((xy, event)) = surface_context::candidate(world.seed, [gx, gy], atmosphere)
            else {
                continue;
            };
            let Some(sample) = world.columns.get(&xy) else {
                continue;
            };
            let Some(biome) = world.biomes.get(&xy).copied() else {
                continue;
            };
            let ground = [xy[0], xy[1], i32::from(sample.height)];
            if !dry_surface_support(world, ground) {
                continue;
            }
            if event != ContextEvent::Phantom && !surface_context::storm_eligible(biome) {
                continue;
            }
            if event == ContextEvent::LightningPig
                && !biome
                    .descriptor()
                    .biome_table_fauna_ids
                    .contains(&"minecraft:pig")
            {
                continue;
            }
            let (anchor, models) = match event {
                ContextEvent::Phantom => {
                    let Some(height) = ground[2].checked_add(12) else {
                        continue;
                    };
                    let anchor = [xy[0], xy[1], height];
                    // Real open sky above the complete flight silhouette; no
                    // player/insomnia counter or native mob-spawn claim.
                    if (height..height.saturating_add(48)).any(|z| {
                        world.blocks.contains_key(&[xy[0], xy[1], z])
                            || world.fluids.contains_key(&[xy[0], xy[1], z])
                    }) {
                        continue;
                    }
                    (
                        anchor,
                        vec![surface_entities::model(
                            Species::Phantom,
                            AtlasLayout::Bedrock,
                            fauna_climate(biome),
                        )],
                    )
                }
                ContextEvent::HorseTrap => (
                    ground,
                    surface_entities::event_models(
                        surface_entities::SurfaceEvent::HorseJockey,
                        AtlasLayout::Bedrock,
                    ),
                ),
                ContextEvent::LightningPig => (
                    ground,
                    surface_entities::event_models(
                        surface_entities::SurfaceEvent::LightningPig,
                        AtlasLayout::Bedrock,
                    ),
                ),
            };
            if world.entities.iter().any(|e| e.anchor == anchor)
                || !models
                    .iter()
                    .all(|model| entity_clearance(world, anchor, model))
            {
                continue;
            }
            // Commit mount/rider together only after every model is clear.
            for model in models {
                world.entities.push(SurfaceEntity {species:model.species,anchor,model,
                    atlas_status:"Static globally owned night/precipitation episode; native insomnia/lightning/trap activation is not simulated; selected/fallback atlas checked during binding"});
            }
        }
    }
    Ok(())
}

// Denser crowns must not erase physical wood already supplied by an earlier global owner.
fn tree_block_is_wood(block: &SurfaceBlock) -> bool {
    let resource = block.state.id().as_str();
    let configuration = match &block.owner {
        SourceOwner::Tree { configuration, .. } => *configuration,
        SourceOwner::TreeDecoration { .. } => return resource == "minecraft:creaking_heart",
        _ => return false,
    };
    let Some(profile) = tree_profiles::profile(configuration) else {
        return false;
    };
    resource == profile.stem.id
        || (profile.shape == TreeShape::Mangrove && resource == "minecraft:mangrove_roots")
}

// Radius-64 village candidates keep all writes in their 256-cell owner tile.
// Load those owners for each complete lower-priority structure before testing
// overlap; a conflict outside the camera window must reject the whole piece.
fn reserve_village_footprint_owners(
    positions: impl Iterator<Item = [i32; 3]>,
    evaluated_grids: &mut BTreeSet<[i32; 2]>,
    structure_positions: &mut BTreeSet<[i32; 3]>,
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    let mut minimum = [i32::MAX; 2];
    let mut maximum = [i32::MIN; 2];
    let mut owner_grids = BTreeSet::new();
    let mut count = 0_usize;
    for position in positions {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        count += 1;
        if count > super::surface_structures::MAX_CELLS {
            return Err(AssetError::InvalidMetadata(
                "structure footprint exceeds prepared cell budget".into(),
            ));
        }
        for axis in 0..2 {
            minimum[axis] = minimum[axis].min(position[axis]);
            maximum[axis] = maximum[axis].max(position[axis]);
        }
        owner_grids.insert([position[0].div_euclid(256), position[1].div_euclid(256)]);
        // Prepared offsets are bounded to +/-256. The inclusive 513-cell span
        // touches at most three owner tiles per axis, including negative edges.
        if owner_grids.len() > 9
            || (0..2).any(|axis| {
                i64::from(maximum[axis]) - i64::from(minimum[axis])
                    > i64::from(super::surface_structures::MAX_OFFSET) * 2
            })
        {
            return Err(AssetError::InvalidMetadata(
                "structure footprint exceeds prepared horizontal bounds".into(),
            ));
        }
    }
    if count == 0 {
        return Err(AssetError::InvalidMetadata(
            "structure footprint is empty".into(),
        ));
    }
    for grid in owner_grids {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        if !evaluated_grids.insert(grid) {
            continue;
        }
        let Some(village) = surface_village_assembly::candidate(grid, fields, settings, cancelled)?
        else {
            continue;
        };
        // The initial village loop already visits every owner tile intersecting
        // the region. Newly loaded owners are off-window reservations only.
        for position in village.writes.keys() {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            structure_positions.insert(*position);
        }
    }
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    Ok(())
}

fn fossil_overlaps(
    fossil: &surface_geology::ExposedFossil,
    positions: impl Iterator<Item = [i32; 3]>,
    cancelled: &impl Fn() -> bool,
) -> Result<bool> {
    for position in positions {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        if fossil.covers(position) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Conservatively reserve every terrain-admitted structure footprint, even a
/// candidate later suppressed by structure priority. The decision uses the
/// fossil's entire footprint, never the requesting window's reservation set.
fn fossil_clear_of_structures(
    fossil: &surface_geology::ExposedFossil,
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: &impl Fn() -> bool,
) -> Result<bool> {
    let first = fossil.minimum.map(|value| (value - 224).div_euclid(256));
    let last = fossil.maximum.map(|value| (value + 224).div_euclid(256));
    for gy in first[1]..=last[1] {
        for gx in first[0]..=last[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            if let Some(village) =
                surface_village_assembly::candidate([gx, gy], fields, settings, cancelled)?
            {
                if fossil_overlaps(fossil, village.writes.keys().copied(), cancelled)? {
                    return Ok(false);
                }
            }
            if let Some(landmark) =
                surface_landmark_assembly::candidate([gx, gy], fields, settings, cancelled)?
            {
                if fossil_overlaps(fossil, landmark.writes.keys().copied(), cancelled)? {
                    return Ok(false);
                }
            }
        }
    }
    let first = fossil.minimum.map(|value| (value - 64).div_euclid(128));
    let last = fossil.maximum.map(|value| (value + 64).div_euclid(128));
    for gy in first[1]..=last[1] {
        for gx in first[0]..=last[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            if let Some(ruin) =
                surface_ruin_assembly::candidate([gx, gy], fields, settings, cancelled)?
            {
                if fossil_overlaps(fossil, ruin.prepared.cells().map(|(p, _)| p), cancelled)? {
                    return Ok(false);
                }
            }
            if let Some(camp) =
                surface_camp_assembly::candidate([gx, gy], fields, settings, cancelled)?
            {
                if fossil_overlaps(fossil, camp.prepared.cells().map(|(p, _)| p), cancelled)? {
                    return Ok(false);
                }
            }
        }
    }
    let first = fossil.minimum.map(|value| (value - 8).div_euclid(16));
    let last = fossil.maximum.map(|value| (value + 8).div_euclid(16));
    for gy in first[1]..=last[1] {
        for gx in first[0]..=last[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            if let Some(well) = surface_landmark_assembly::desert_well_candidate(
                [gx, gy],
                fields,
                settings,
                cancelled,
            )? {
                if fossil_overlaps(fossil, well.writes.keys().copied(), cancelled)? {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// Stage every state and check every reservation before either ledger changes.
/// The bounded commit has no fallible operation or cancellation checkpoint.
fn project_fossil(
    world: &mut SurfaceWorld,
    reserved: &mut BTreeSet<[i32; 3]>,
    fossil: surface_geology::ExposedFossil,
    cancelled: &impl Fn() -> bool,
) -> Result<bool> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    if fossil.cells.is_empty() || fossil.cells.len() > surface_geology::FOSSIL_MAX_CELLS {
        return Err(AssetError::InvalidMetadata(
            "invalid fossil cell count".into(),
        ));
    }
    if fossil_overlaps(&fossil, reserved.iter().copied(), cancelled)? {
        return Ok(false);
    }
    let mut prepared = Vec::with_capacity(fossil.cells.len());
    for &(offset, axis) in &fossil.cells {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        let position = add_global(fossil.anchor, offset)?;
        if world.blocks.contains_key(&position) || world.fluids.contains_key(&position) {
            return Ok(false);
        }
        prepared.push((
            position,
            SurfaceBlock {
                state: state(
                    "minecraft:bone_block",
                    [("axis".into(), axis.java_value().into())],
                )?,
                owner: SourceOwner::Geology {
                    anchor: fossil.anchor,
                    source: surface_geology::FOSSIL_SOURCE,
                },
            },
        ));
    }
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    for (position, block) in prepared {
        reserved.insert(position);
        if world.region.contains(position) {
            world.blocks.insert(position, block);
        }
    }
    Ok(true)
}

fn populate_fossils(
    world: &mut SurfaceWorld,
    reserved: &mut BTreeSet<[i32; 3]>,
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    // Cover complete existing tree queries too: anchor halo + one tree grid
    // step + geometry reach + attachment + fossil reach and anchor jitter.
    // Off-window bones reserve occupancy only; source columns are not expanded.
    let halo = 48
        + 13
        + i32::from(TreeGeometry::COORDINATE_LIMIT)
        + 1
        + surface_geology::FOSSIL_REACH
        + surface_geology::FOSSIL_JITTER;
    let first = world
        .region
        .minimum
        .map(|v| (v - halo).div_euclid(surface_geology::FOSSIL_GRID));
    let last = world
        .region
        .maximum
        .map(|v| (v + halo).div_euclid(surface_geology::FOSSIL_GRID));
    for gy in first[1]..=last[1] {
        for gx in first[0]..=last[0] {
            let Some(fossil) = surface_geology::desert_fossil(
                world.seed,
                [gx, gy],
                |xy| sample_ground(fields, settings, xy[0], xy[1]),
                cancelled,
            )?
            else {
                continue;
            };
            if !fossil_clear_of_structures(&fossil, fields, settings, cancelled)? {
                continue;
            }
            project_fossil(world, reserved, fossil, cancelled)?;
        }
    }
    Ok(())
}

pub fn prepare(
    region: Region,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<SurfaceWorld> {
    prepare_with_tree_configuration(region, settings, tree_configuration, cancelled)
}
// The private selector seam lets support regressions pin a source without fabricating terrain or geometry.
fn prepare_with_tree_configuration(
    region: Region,
    settings: &VoxelLandscapeSettings,
    choose_configuration: impl Fn(SurfaceBiome, u64) -> Option<&'static str>,
    cancelled: impl Fn() -> bool,
) -> Result<SurfaceWorld> {
    region.validate()?;
    let settings = settings.normalized();
    let seed = u64::from(settings.seed);
    let fields = TerrainFields::new(seed);
    let mut world = SurfaceWorld { region, seed, columns:BTreeMap::new(), biomes:BTreeMap::new(),
        blocks:BTreeMap::new(), fluids:BTreeMap::new(), trees:Vec::new(), flora:Vec::new(),
        structures:Vec::new(),entities:Vec::new(), source_limitations:vec![
            "Terrain/biome selector is authored Ilium homage, not native multi-noise bands",
            "Flora attempts use a denser authored surface distribution; exact Java count/noise algorithms remain pending",
            "Dry grass uses one-cell soil-or-sand admission; the unchanged cactus-flower selection slot constructs a whole two-cell age0 cactus plus crown with side clearance; native substrate tags, flower frequency and growth timing are not reproduced",
            "Surface azalea indicators use rare authored woodland selection; no underground cave/root network is generated",
            "Ice spire silhouettes and cold surface openings are authored; native feature processors are not reproduced",
            "Surface strata, calcite seams and soil mosaics use authored global material rules; native noise and surface-rule parity are not claimed",
            "Exposed desert rib/spine fossils are original Ilium geometry, fitted above dry sand with at most one cell of relief; no buried native fossil, excavation or ore processing is reproduced; all terrain-admitted structure footprints conservatively exclude fossils",
            "Village graph uses authored kit and terrain fit; native jigsaw/templates/processors and other structure families remain pending",
            "Seven surface landmark exteriors, nine ruin kits and eighteen camp presets use original geometry and sparse placement; native salts, templates and processors remain unverified",
            "Desert wells use independent authored16-block rare-feature admission and whole dry-sand footprint checks; native RNG, buried support and exact placement parity remain unverified",
            "Day/Night/Thunderstorm is fixed scene atmosphere; sparse phantom, skeleton-horse trap and transformed-pig episodes do not simulate sleep debt, lightning or combat",
            "Night Creaking figures belong to accepted natural Pale Garden hearts; native heart puppetry and atlas parity remain unverified",
            "Entity day shade uses authored clearance proxies; exact native spawn light and selected-pack atlas parity remain unverified",
            "Wandering visitors use sparse globally owned original trader/two-llama groups with complete final-ground clearance; native trader timers, trading and leads are not simulated",
            "Accepted natural bee nests display one original hovering bee when its complete silhouette fits; native nest occupancy and release timing are not simulated",
        ] };
    let mut terrain_states = BTreeMap::<&'static str, BlockState>::new();
    for y in region.minimum[1] - 1..=region.maximum[1] {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        for x in region.minimum[0] - 1..=region.maximum[0] {
            let (sample, biome) = sample_ground(&fields, &settings, x, y);
            world.columns.insert([x, y], sample);
            world.biomes.insert([x, y], biome);
        }
    }
    for y in region.minimum[1]..region.maximum[1] {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        for x in region.minimum[0]..region.maximum[0] {
            let sample = world.columns[&[x, y]];
            let biome = world.biomes[&[x, y]];
            let ground = i32::from(sample.height) - 1;
            let materials = surface_geology::terrain_materials(seed, biome, [x, y], ground);
            let min_neighbor = [[x - 1, y], [x + 1, y], [x, y - 1], [x, y + 1]]
                .into_iter()
                .filter_map(|p| world.columns.get(&p))
                .map(|s| i32::from(s.height) - 1)
                .min()
                .unwrap_or(ground);
            let bottom = (min_neighbor - 1).min(ground - 3).max(ground - 64).max(0);
            for z in bottom..=ground {
                let id = materials.at(z);
                let block = if let Some(state) = terrain_states.get(id) {
                    state.clone()
                } else {
                    let state = terrain_state(id)?;
                    terrain_states.insert(id, state.clone());
                    state
                };
                world.blocks.insert(
                    [x, y, z],
                    SurfaceBlock {
                        state: block,
                        owner: SourceOwner::Terrain { biome },
                    },
                );
            }
            if let Some(level) = sample.water_level {
                // River beds need a continuous column so banks connect to the
                // authored water plane. Wetland roots retain their original
                // shallow-surface semantics: filling every cell below the
                // surface would incorrectly waterlog dry mangrove roots.
                let continuous_river =
                    matches!(biome, SurfaceBiome::River | SurfaceBiome::FrozenRiver);
                if i32::from(level) > ground + 1 {
                    let top = i32::from(level) - 1;
                    let tint = water_tint(biome);
                    let freezes = surface_geology::freezes_surface(biome, seed, [x, y]);
                    let first = if continuous_river { ground + 1 } else { top };
                    for z in first..=top {
                        let position = [x, y, z];
                        if freezes && z == top {
                            world.blocks.insert(
                                position,
                                SurfaceBlock {
                                    state: plain("minecraft:ice")?,
                                    owner: SourceOwner::Terrain { biome },
                                },
                            );
                        } else {
                            world.fluids.insert(position, FluidCell::new(0, tint)?);
                        }
                    }
                }
            }
        }
    }
    // Evaluate a whole globally owned village candidate before region projection.
    // One stable TerrainFields snapshot serves every piece habitat callback.
    let mut structure_positions = BTreeSet::new();
    let mut evaluated_village_grids = BTreeSet::new();
    let village_min = region.minimum.map(|value| (value - 192).div_euclid(256));
    let village_max = region.maximum.map(|value| (value + 192).div_euclid(256));
    for gy in village_min[1]..=village_max[1] {
        for gx in village_min[0]..=village_max[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            evaluated_village_grids.insert([gx, gy]);
            let Some(village) =
                surface_village_assembly::candidate([gx, gy], &fields, &settings, &cancelled)?
            else {
                continue;
            };
            for position in village.writes.keys() {
                structure_positions.insert(*position);
            }
            let mut projected = 0;
            for (position, write) in village.writes {
                if !region.contains(position) {
                    continue;
                }
                projected += 1;
                world.blocks.remove(&position);
                world.fluids.remove(&position);
                if let Some(block) = write.state {
                    if block.id().as_str() == "minecraft:water" {
                        let biome = world.biomes[&[position[0], position[1]]];
                        world
                            .fluids
                            .insert(position, FluidCell::new(0, water_tint(biome))?);
                    } else {
                        world.blocks.insert(
                            position,
                            SurfaceBlock {
                                state: block,
                                owner: SourceOwner::Structure {
                                    anchor: village.center,
                                    source: write.piece_source,
                                },
                            },
                        );
                    }
                }
            }
            for marker in village.markers {
                if !region.contains(marker.position) {
                    continue;
                }
                let species = match marker.kind {
                    village_kit::MarkerKind::Resident(_) => Species::Villager,
                    village_kit::MarkerKind::Guardian => Species::IronGolem,
                    village_kit::MarkerKind::Animal(id) => match Species::from_id(id) {
                        Some(species) => species,
                        None => continue,
                    },
                };
                let climate = match village.style {
                    village_kit::VillageStyle::Desert | village_kit::VillageStyle::Savanna => {
                        ClimateSkin::Warm
                    }
                    village_kit::VillageStyle::Snowy => ClimateSkin::Cold,
                    _ => ClimateSkin::Temperate,
                };
                world.entities.push(SurfaceEntity {
                    species,
                    anchor: marker.position,
                    model: surface_entities::model(species, AtlasLayout::Bedrock, climate),
                    atlas_status:
                        "Village kit marker; selected-pack entity atlas compatibility unverified",
                });
            }
            if projected > 0 {
                world.structures.push(FeatureRecord {
                    anchor: village.center,
                    source: surface_village_assembly::source(village.style),
                    projected_cells: projected,
                    authored_placement: true,
                });
            }
        }
    }
    let landmark_min = region.minimum.map(|value| (value - 224).div_euclid(256));
    let landmark_max = region.maximum.map(|value| (value + 224).div_euclid(256));
    let mut landmark_candidates = Vec::new();
    for gy in landmark_min[1]..=landmark_max[1] {
        for gx in landmark_min[0]..=landmark_max[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            if let Some(landmark) =
                surface_landmark_assembly::candidate([gx, gy], &fields, &settings, &cancelled)?
            {
                landmark_candidates.push(landmark);
            }
        }
    }
    // Wells reserve their whole five-block exterior before region projection,
    // using an independent16-block grid with enough halo for adjacent cells.
    let well_min = region.minimum.map(|value| (value - 8).div_euclid(16));
    let well_max = region.maximum.map(|value| (value + 8).div_euclid(16));
    for gy in well_min[1]..=well_max[1] {
        for gx in well_min[0]..=well_max[0] {
            if let Some(well) = surface_landmark_assembly::desert_well_candidate(
                [gx, gy],
                &fields,
                &settings,
                &cancelled,
            )? {
                landmark_candidates.push(well);
            }
        }
    }
    for landmark in landmark_candidates {
        reserve_village_footprint_owners(
            landmark.writes.keys().copied(),
            &mut evaluated_village_grids,
            &mut structure_positions,
            &fields,
            &settings,
            &cancelled,
        )?;
        // A full preprojected overlap rejects the candidate. Rejected
        // fragments cannot reappear when a neighboring region is loaded.
        if landmark
            .writes
            .keys()
            .any(|position| structure_positions.contains(position))
        {
            continue;
        }
        for position in landmark.writes.keys() {
            structure_positions.insert(*position);
        }
        let mut projected = 0;
        for (position, block) in landmark.writes {
            if !region.contains(position) {
                continue;
            }
            projected += 1;
            world.blocks.remove(&position);
            world.fluids.remove(&position);
            if let Some(block) = block {
                if block.id().as_str() == "minecraft:water" {
                    let biome = world.biomes[&[position[0], position[1]]];
                    world
                        .fluids
                        .insert(position, FluidCell::new(0, water_tint(biome))?);
                } else {
                    world.blocks.insert(
                        position,
                        SurfaceBlock {
                            state: block,
                            owner: SourceOwner::Structure {
                                anchor: landmark.anchor,
                                source: landmark.source.clone(),
                            },
                        },
                    );
                }
            }
        }
        for (id, position) in landmark.occupants {
            if !region.contains(position) {
                continue;
            }
            let Some(species) = Species::from_id(id) else {
                continue;
            };
            let biome = world.biomes[&[position[0], position[1]]];
            let climate = fauna_climate(biome);
            world.entities.push(SurfaceEntity {
                species,
                anchor: position,
                model: surface_entities::model(species, AtlasLayout::Bedrock, climate),
                atlas_status:
                    "Original landmark marker; selected-pack entity atlas compatibility unverified",
            });
        }
        if projected > 0 {
            world.structures.push(FeatureRecord {
                anchor: landmark.anchor,
                source: surface_landmark_assembly::source(landmark.kind),
                projected_cells: projected,
                authored_placement: true,
            });
        }
    }
    // Ruins reserve their whole globally admitted footprint, including explicit
    // air, before projecting actual above-terrain/water portions into the view.
    let ruin_min = region.minimum.map(|value| (value - 64).div_euclid(128));
    let ruin_max = region.maximum.map(|value| (value + 64).div_euclid(128));
    for gy in ruin_min[1]..=ruin_max[1] {
        for gx in ruin_min[0]..=ruin_max[0] {
            let Some(ruin) =
                surface_ruin_assembly::candidate([gx, gy], &fields, &settings, &cancelled)?
            else {
                continue;
            };
            reserve_village_footprint_owners(
                ruin.prepared.cells().map(|(position, _)| position),
                &mut evaluated_village_grids,
                &mut structure_positions,
                &fields,
                &settings,
                &cancelled,
            )?;
            if ruin
                .prepared
                .cells()
                .any(|(position, _)| structure_positions.contains(&position))
            {
                continue;
            }
            structure_positions.extend(ruin.prepared.cells().map(|(position, _)| position));
            let anchor = ruin.prepared.anchor();
            let source = format!(
                "{}/rotation/{}",
                ruin.prepared.source(),
                ruin.prepared.rotation()
            );
            let mut projected = 0;
            for (position, write) in ruin.writes {
                if !region.contains(position) {
                    continue;
                }
                projected += 1;
                world.blocks.remove(&position);
                world.fluids.remove(&position);
                if let Some(state) = write {
                    world.blocks.insert(
                        position,
                        SurfaceBlock {
                            state,
                            owner: SourceOwner::Structure {
                                anchor,
                                source: source.clone(),
                            },
                        },
                    );
                }
            }
            if projected > 0 {
                world.structures.push(FeatureRecord {
                    anchor,
                    source: surface_ruin_assembly::source(ruin.kind),
                    projected_cells: projected,
                    authored_placement: true,
                });
            }
        }
    }
    // Source-profile camps reserve their complete clearing and foundation,
    // including air, before any camera-window projection.
    for gy in ruin_min[1]..=ruin_max[1] {
        for gx in ruin_min[0]..=ruin_max[0] {
            let Some(camp) =
                surface_camp_assembly::candidate([gx, gy], &fields, &settings, &cancelled)?
            else {
                continue;
            };
            reserve_village_footprint_owners(
                camp.prepared.cells().map(|(position, _)| position),
                &mut evaluated_village_grids,
                &mut structure_positions,
                &fields,
                &settings,
                &cancelled,
            )?;
            if camp
                .prepared
                .cells()
                .any(|(p, _)| structure_positions.contains(&p))
            {
                continue;
            }
            structure_positions.extend(camp.prepared.cells().map(|(p, _)| p));
            let anchor = camp.prepared.anchor();
            let source = format!(
                "{}/rotation/{}",
                camp.prepared.source(),
                camp.prepared.rotation()
            );
            let mut projected = 0;
            for (position, block) in camp
                .prepared
                .project(region.minimum.map(i64::from), region.maximum.map(i64::from))
                .map_err(|e| AssetError::InvalidMetadata(format!("camp projection: {e:?}")))?
            {
                projected += 1;
                world.blocks.remove(&position);
                world.fluids.remove(&position);
                if let Some(state) = block {
                    world.blocks.insert(
                        position,
                        SurfaceBlock {
                            state: state.clone(),
                            owner: SourceOwner::Structure {
                                anchor,
                                source: source.clone(),
                            },
                        },
                    );
                }
            }
            if projected > 0 {
                world.structures.push(FeatureRecord {
                    anchor,
                    source: camp.style.source_id(),
                    projected_cells: projected,
                    authored_placement: true,
                });
            }
        }
    }
    populate_fossils(
        &mut world,
        &mut structure_positions,
        &fields,
        &settings,
        &cancelled,
    )?;
    // Ice spires have globally owned whole candidates, independently of flora
    // density. Keep their exact feature/material provenance and aboveground scope.
    let geology_min = region.minimum.map(|value| (value - 32).div_euclid(21));
    let geology_max = region.maximum.map(|value| (value + 32).div_euclid(21));
    for gy in geology_min[1]..=geology_max[1] {
        for gx in geology_min[0]..=geology_max[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            let entropy = hash2(seed ^ 0x6963_655f_7370_696b, i64::from(gx), i64::from(gy));
            if entropy % 100 >= 72 {
                continue;
            }
            let x = gx * 21 + 10 + (entropy.rotate_left(17) % 9) as i32 - 4;
            let y = gy * 21 + 10 + (entropy.rotate_left(41) % 9) as i32 - 4;
            let (sample, biome) = sample_ground(&fields, &settings, x, y);
            if biome != SurfaceBiome::IceSpikes
                || sample
                    .water_level
                    .is_some_and(|level| level > sample.height)
            {
                continue;
            }
            let anchor = [x, y, i32::from(sample.height)];
            let positions = surface_geology::ice_spike(entropy)
                .into_iter()
                .map(|cell| Ok((add_global(anchor, cell.position)?, cell.resource_id)))
                .collect::<Result<Vec<_>>>()?;
            if positions.iter().any(|(position, _)| {
                if structure_positions.contains(position) {
                    return true;
                }
                let (ground, cell_biome) =
                    sample_ground(&fields, &settings, position[0], position[1]);
                ground.water_level.is_some_and(|level| {
                    level > ground.height && position[2] == i32::from(level) - 1
                }) && !surface_geology::freezes_surface(
                    cell_biome,
                    seed,
                    [position[0], position[1]],
                )
            }) {
                continue;
            }
            for (position, resource) in positions {
                let (ground, _) = sample_ground(&fields, &settings, position[0], position[1]);
                if position[2] < i32::from(ground.height) {
                    continue;
                }
                structure_positions.insert(position);
                if !region.contains(position) {
                    continue;
                }
                world.blocks.insert(
                    position,
                    SurfaceBlock {
                        state: plain(resource)?,
                        owner: SourceOwner::Geology {
                            anchor,
                            source: "minecraft:ice_spike",
                        },
                    },
                );
            }
        }
    }
    let structure_xy: BTreeSet<[i32; 2]> = structure_positions
        .iter()
        .map(|position| [position[0], position[1]])
        .collect();
    // Capture only off-window cactus side probes. The existing tree halo covers
    // them: current forms reach at most41 cells horizontally (fallen length40
    // plus the stump gap), and an attached block adds at most one more cell.
    let mut cactus_halo = cactus_tree_halo(&world, &cancelled)?;
    // Evaluate whole trees by global grid anchor, then project. This keeps the
    // same configuration and owner when a camera/region boundary moves.
    let anchor_min = region.minimum.map(|v| (v - 48).div_euclid(13));
    let anchor_max = region.maximum.map(|v| (v + 48).div_euclid(13));
    for (gy, infill) in (anchor_min[1]..=anchor_max[1]).flat_map(|gy| [(gy, false), (gy, true)]) {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        for gx in anchor_min[0]..=anchor_max[0] {
            let salt = if infill {
                0x7472_6565_5f66_696c
            } else {
                0x7472_6565
            };
            let hash = hash2(seed ^ salt, i64::from(gx), i64::from(gy));
            let (x, y) = if infill {
                (
                    gx * 13 + (hash.rotate_left(29) % 3) as i32 - 1,
                    gy * 13 + (hash.rotate_left(43) % 3) as i32 - 1,
                )
            } else {
                (
                    gx * 13 + 6 + (hash.rotate_left(29) % 9) as i32 - 4,
                    gy * 13 + 6 + (hash.rotate_left(43) % 9) as i32 - 4,
                )
            };
            let (sample, biome) = sample_ground(&fields, &settings, x, y);
            if infill && !tree_infill_enabled(biome) {
                continue;
            }
            let cover = tree_cover_percent(biome)
                * (settings.vegetation_percent.clamp(0, 100) as u64)
                / 100;
            if hash % 100 >= cover {
                continue;
            }
            let configs = biome.descriptor().natural_tree_configuration_ids;
            if configs.is_empty()
                || sample.water_level.is_some_and(|z| z > sample.height)
                    && biome != SurfaceBiome::MangroveSwamp
            {
                continue;
            }
            let Some(source) = choose_configuration(biome, hash) else {
                continue;
            };
            let Some(profile) = tree_profiles::profile(source) else {
                continue;
            };
            let anchor = [x, y, i32::from(sample.height)];
            let geometry = tree_forms::build(profile, tree_growth(hash), hash)
                .map_err(|_| AssetError::InvalidMetadata("tree geometry failed".into()))?;
            if !tree_terrain_admits(
                anchor,
                profile.shape,
                &geometry,
                |xy| {
                    let sample = fields.sample(xy[0], xy[1], settings.rivers);
                    TreeTerrainColumn {
                        solid_top: i32::from(sample.height),
                        water_top: sample.water_level.map(i32::from),
                    }
                },
                &cancelled,
            )? {
                continue;
            }
            let states = tree_forms::bind_states(profile, &geometry, hash)
                .map_err(|_| AssetError::InvalidMetadata("tree state binding failed".into()))?;
            if states.iter().any(|cell| {
                add_global(anchor, cell.position)
                    .is_ok_and(|position| structure_positions.contains(&position))
            }) {
                continue;
            }
            if tree_canopy_overlaps_structure_clearance(anchor, &states, profile, &structure_xy) {
                continue;
            }
            let mut projected = 0;
            for cell in states {
                let position = add_global(anchor, cell.position)?;
                if let Some(occupied) = cactus_halo.get_mut(&position) {
                    *occupied = true;
                }
                if !region.contains(position) {
                    continue;
                }
                if profile
                    .crowns
                    .iter()
                    .any(|resource| resource.id == cell.resource_id)
                    && world.blocks.get(&position).is_some_and(tree_block_is_wood)
                {
                    continue;
                }
                let block = state(
                    cell.resource_id,
                    cell.properties.into_iter().map(|(k, v)| {
                        let value = if cell.resource_id == "minecraft:mangrove_roots"
                            && k == "waterlogged"
                            && world.fluids.contains_key(&position)
                        {
                            "true"
                        } else {
                            v
                        };
                        (k.to_owned(), value.to_owned())
                    }),
                )?;
                world.blocks.insert(
                    position,
                    SurfaceBlock {
                        state: block,
                        owner: SourceOwner::Tree {
                            anchor,
                            configuration: source,
                        },
                    },
                );
                projected += 1;
            }
            if let Some(decorations) = tree_decoration_profiles::profile(source) {
                let context = |local: [i16; 3]| {
                    let Ok(position) = add_global(anchor, local) else {
                        return SurroundingCell::Unknown;
                    };
                    match habitat(&fields, &settings, position) {
                        // Air-only decorations such as litter and vines cannot occupy water.
                        HabitatCell::Air => SurroundingCell::Air,
                        HabitatCell::Water => SurroundingCell::Solid,
                        HabitatCell::Soil => SurroundingCell::ReplaceableSoil,
                        HabitatCell::Sand | HabitatCell::Solid => SurroundingCell::Solid,
                        HabitatCell::Unknown => SurroundingCell::Unknown,
                    }
                };
                let mut planner = DecorationPlanner::new(&geometry, &context);
                let ground_height = |offset: [i16; 2]| -> Option<i16> {
                    let xx = x.checked_add(i32::from(offset[0]))?;
                    let yy = y.checked_add(i32::from(offset[1]))?;
                    i16::try_from(
                        i32::from(fields.sample(xx, yy, settings.rivers).height) - 1 - anchor[2],
                    )
                    .ok()
                };
                let _applications = planner.apply_profile(decorations, hash, ground_height);
                for decoration in planner.finish() {
                    let position = add_global(anchor, decoration.state.position)?;
                    if let Some(occupied) = cactus_halo.get_mut(&position) {
                        // Planner output either writes this cell or is refused
                        // because terrain/structure/earlier wood already blocks
                        // it. No tree/decorator operation removes occupancy.
                        // This is an obstruction union, not a placement receipt.
                        *occupied = true;
                    }
                    if !region.contains(position) {
                        continue;
                    }
                    let block = state(
                        decoration.state.resource_id,
                        decoration
                            .state
                            .properties
                            .into_iter()
                            .map(|(k, v)| (k.to_owned(), v.to_owned())),
                    )?;
                    let can_place = match decoration.operation {
                        DecorationOperation::PlaceInAir => {
                            !world.blocks.contains_key(&position)
                                && !structure_positions.contains(&position)
                        }
                        DecorationOperation::ReplaceSoil => world
                            .blocks
                            .get(&position)
                            .is_some_and(|b| matches!(b.owner, SourceOwner::Terrain { .. })),
                        DecorationOperation::ReplaceLog => world
                            .blocks
                            .get(&position)
                            .is_some_and(|b| matches!(b.owner, SourceOwner::Tree { .. })),
                    };
                    if can_place {
                        world.blocks.insert(
                            position,
                            SurfaceBlock {
                                state: block,
                                owner: SourceOwner::TreeDecoration {
                                    anchor,
                                    configuration: source,
                                },
                            },
                        );
                        projected += 1;
                    }
                }
            }
            world.trees.push(FeatureRecord {
                anchor,
                source,
                projected_cells: projected,
                authored_placement: true,
            });
        }
    }
    let mut flora = FloraPlacement::new(65_536)
        .map_err(|_| AssetError::InvalidMetadata("flora placement limit".into()))?;
    // Retain candidate provenance because FloraPlacement::cells() contains only
    // states/positions, not its source prescription or global owner anchor.
    let mut flora_owners = BTreeMap::<[i32; 3], ([i32; 3], &'static str)>::new();
    let anchor_min = region.minimum.map(|v| (v - 1).div_euclid(4));
    let anchor_max = region.maximum.map(|v| (v + 1).div_euclid(4));
    for gy in anchor_min[1]..=anchor_max[1] {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        for gx in anchor_min[0]..=anchor_max[0] {
            let x = gx * 4 + 2;
            let y = gy * 4 + 2;
            let hash = hash2(seed ^ 0x0066_6c6f_7261, i64::from(gx), i64::from(gy));
            // Keep the four-cell ownership grid and its deterministic anchor,
            // but admit more candidates so full-scene views carry the visible
            // grass, flowers, reeds and biome-specific ground cover expected
            // from a Minecraft-like surface.
            if hash % 100 >= (settings.vegetation_percent.min(100) as u64) * 72 / 100 {
                continue;
            }
            let (sample, biome) = sample_ground(&fields, &settings, x, y);
            let candidates: Vec<_> = ENTRIES
                .iter()
                .filter(|entry| {
                    !entry.attachment
                        && entry.generation_biomes.contains(&biome.id())
                        && (entry.id != "minecraft:pumpkin" || pumpkin_patch_contains(seed, x, y))
                })
                .collect();
            if candidates.is_empty() {
                continue;
            }
            let Some(entry) = choose_flora_entry(&candidates, biome, hash.rotate_left(17)) else {
                continue;
            };
            let height = if entry.id == "minecraft:lily_pad" {
                let Some(water_level) = sample.water_level else {
                    continue;
                };
                water_level
            } else {
                sample.height
            };
            let anchor = [x, y, i32::from(height)];
            let candidate = flower_candidate(anchor, entry.id);
            let positions: Option<Vec<_>> = candidate
                .cells
                .iter()
                .map(|cell| {
                    Some([
                        anchor[0].checked_add(cell.position[0])?,
                        anchor[1].checked_add(cell.position[1])?,
                        anchor[2].checked_add(cell.position[2])?,
                    ])
                })
                .collect();
            let Some(positions) = positions else {
                continue;
            };
            let cactus_clearance = candidate.cactus_clearance;
            let admitted = flora.admit_cancellable(
                candidate,
                |position| {
                    if structure_positions.contains(&position)
                        || (cactus_clearance && cactus_halo.get(&position) == Some(&true))
                        || world.blocks.get(&position).is_some_and(|b| {
                            b.state.id().as_str() == "minecraft:ice"
                                || !matches!(b.owner, SourceOwner::Terrain { .. })
                        })
                    {
                        HabitatCell::Solid
                    } else {
                        habitat(&fields, &settings, position)
                    }
                },
                &cancelled,
            );
            if matches!(admitted, Err(AdmissionError::Cancelled)) {
                return Err(AssetError::Cancelled);
            }
            if admitted.is_ok() {
                let mut projected = 0;
                for position in positions {
                    if region.contains(position) {
                        flora_owners.insert(position, (anchor, entry.id));
                        projected += 1;
                    }
                }
                world.flora.push(FeatureRecord {
                    anchor,
                    source: entry.id,
                    projected_cells: projected,
                    authored_placement: true,
                });
            }
        }
    }
    for cell in flora.cells() {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        if !region.contains(cell.position) {
            continue;
        }
        let (anchor, prescription) =
            flora_owners.get(&cell.position).copied().ok_or_else(|| {
                AssetError::InvalidMetadata("accepted flora lost its owner ledger".into())
            })?;
        // Choose the counterpart after admission; preserve the closed source prescription.
        let resource_id = if cell.state.resource_id == "minecraft:closed_eyeblossom"
            && SceneAtmosphere::from_index(settings.atmosphere).is_night()
        {
            "minecraft:open_eyeblossom"
        } else {
            cell.state.resource_id
        };
        let block = state(
            resource_id,
            cell.state
                .properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string())),
        )?;
        world.blocks.insert(
            cell.position,
            SurfaceBlock {
                state: block,
                owner: SourceOwner::Flora {
                    anchor,
                    prescription,
                },
            },
        );
    }
    // Global owner grid and post-placement habitat admit the same candidate
    // in overlapping camera windows. This remains authored presentation fauna.
    for gy64 in
        (first_global_fauna_grid(region.minimum[1])..i64::from(region.maximum[1])).step_by(16)
    {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        for gx64 in
            (first_global_fauna_grid(region.minimum[0])..i64::from(region.maximum[0])).step_by(16)
        {
            let (gx, gy) = (gx64 as i32, gy64 as i32);
            let Some(sample) = world.columns.get(&[gx, gy]) else {
                continue;
            };
            let Some(biome) = world.biomes.get(&[gx, gy]) else {
                continue;
            };
            let fauna = biome.descriptor().biome_table_fauna_ids;
            if fauna.is_empty() {
                continue;
            }
            let hash = hash2(seed ^ 0x656e_7469_7479, gx64, gy64);
            if !hash.is_multiple_of(7) {
                continue;
            }
            let id = fauna[(hash.rotate_left(7) as usize) % fauna.len()];
            let Some(species) = Species::from_id(id) else {
                continue;
            };
            if !surface_biome_allows_species(*biome, species) {
                continue;
            }
            let model =
                surface_entities::model(species, AtlasLayout::Bedrock, fauna_climate(*biome));
            let anchor = [gx, gy, i32::from(sample.height)];
            let admitted = if SceneAtmosphere::from_index(settings.atmosphere).is_night() {
                dry_surface_entity_site(&world, anchor, &model)
            } else {
                natural_entity_site(&world, anchor, species, &model)
            };
            if !admitted {
                continue;
            }
            if world.entities.iter().any(|entity| entity.anchor == anchor) {
                continue;
            }
            world.entities.push(SurfaceEntity {species,anchor,model,
                atlas_status:"Authored habitat/daylight proxy; Bedrock-reference UV; selected/fallback atlas checked during binding"});
        }
    }
    populate_nest_bees(&mut world, &cancelled)?;
    populate_trader_parties(&mut world, &cancelled)?;
    populate_village_raids(&mut world, &cancelled)?;
    populate_contextual_fauna(
        &mut world,
        SceneAtmosphere::from_index(settings.atmosphere),
        &cancelled,
    )?;
    populate_riverbank_drowned(
        &mut world,
        settings.rivers,
        SceneAtmosphere::from_index(settings.atmosphere),
        &cancelled,
    )?;
    validate_generated_height(&world)?;
    Ok(world)
}

/// Assemble individually bounded source windows into one region before model
/// binding. The halo is evaluated by the existing whole-owner generator; only
/// each core contributes cells. This keeps tree/structure ownership tied to
/// global anchors and gives the mesher its adjacent-core neighbors.
pub fn prepare_viewport(
    region: Region,
    scale: f32,
    size: [usize; 2],
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<SurfaceWorld> {
    const MAX_BLOCKS: usize = 1_000_000;
    const MAX_FLUIDS: usize = 1_000_000;
    const MAX_COLUMNS: usize = 1_000_000;
    const MAX_ENTITIES: usize = 65_536;

    let cores = super::surface_viewport::visible_tiles(region, scale, size)?;
    let mut combined = SurfaceWorld {
        region,
        seed: u64::from(settings.seed),
        columns: BTreeMap::new(),
        biomes: BTreeMap::new(),
        blocks: BTreeMap::new(),
        fluids: BTreeMap::new(),
        trees: Vec::new(),
        flora: Vec::new(),
        structures: Vec::new(),
        entities: Vec::new(),
        source_limitations: Vec::new(),
    };
    let mut trees = BTreeMap::<([i32; 3], &'static str), FeatureRecord>::new();
    let mut flora = BTreeMap::<([i32; 3], &'static str), FeatureRecord>::new();
    let mut structures = BTreeMap::<([i32; 3], &'static str), FeatureRecord>::new();
    for (core, expanded) in cores {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        let tile = prepare(expanded, settings, &cancelled)?;
        if combined.source_limitations.is_empty() {
            combined.source_limitations = tile.source_limitations;
        }
        for (position, sample) in tile.columns {
            if core.contains([position[0], position[1], 0]) {
                combined.columns.insert(position, sample);
            }
        }
        for (position, biome) in tile.biomes {
            if core.contains([position[0], position[1], 0]) {
                combined.biomes.insert(position, biome);
            }
        }
        for (position, block) in tile.blocks {
            if core.contains(position) {
                combined.blocks.insert(position, block);
            }
        }
        for (position, fluid) in tile.fluids {
            if core.contains(position) {
                combined.fluids.insert(position, fluid);
            }
        }
        for entity in tile.entities {
            if core.contains(entity.anchor) {
                combined.entities.push(entity);
            }
        }
        for record in tile.trees {
            trees
                .entry((record.anchor, record.source))
                .or_insert(record);
        }
        for record in tile.flora {
            flora
                .entry((record.anchor, record.source))
                .or_insert(record);
        }
        for record in tile.structures {
            structures
                .entry((record.anchor, record.source))
                .or_insert(record);
        }
        for (resource, count, limit) in [
            (
                "generated viewport blocks",
                combined.blocks.len(),
                MAX_BLOCKS,
            ),
            (
                "generated viewport fluids",
                combined.fluids.len(),
                MAX_FLUIDS,
            ),
            (
                "generated viewport columns",
                combined.columns.len(),
                MAX_COLUMNS,
            ),
            (
                "generated viewport entities",
                combined.entities.len(),
                MAX_ENTITIES,
            ),
        ] {
            if count > limit {
                return Err(AssetError::Limit {
                    resource,
                    requested: count as u64,
                    limit: limit as u64,
                });
            }
        }
    }
    // The records are diagnostic metadata. Recount visible owner cells after
    // stitching rather than summing overlapping halo projections.
    let mut tree_counts = BTreeMap::<[i32; 3], usize>::new();
    let mut flora_counts = BTreeMap::<[i32; 3], usize>::new();
    let mut structure_counts = BTreeMap::<[i32; 3], usize>::new();
    for block in combined.blocks.values() {
        match &block.owner {
            SourceOwner::Tree { anchor, .. } | SourceOwner::TreeDecoration { anchor, .. } => {
                *tree_counts.entry(*anchor).or_default() += 1;
            }
            SourceOwner::Flora { anchor, .. } => {
                *flora_counts.entry(*anchor).or_default() += 1;
            }
            SourceOwner::Structure { anchor, .. } => {
                *structure_counts.entry(*anchor).or_default() += 1;
            }
            _ => {}
        }
    }
    combined.trees = trees
        .into_values()
        .filter_map(|mut record| {
            record.projected_cells = tree_counts.get(&record.anchor).copied().unwrap_or(0);
            (record.projected_cells > 0).then_some(record)
        })
        .collect();
    combined.flora = flora
        .into_values()
        .filter_map(|mut record| {
            record.projected_cells = flora_counts.get(&record.anchor).copied().unwrap_or(0);
            (record.projected_cells > 0).then_some(record)
        })
        .collect();
    combined.structures = structures
        .into_values()
        .filter_map(|mut record| {
            record.projected_cells = structure_counts.get(&record.anchor).copied().unwrap_or(0);
            (record.projected_cells > 0).then_some(record)
        })
        .collect();
    validate_generated_height(&combined)?;
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_canopy_clearance_rejects_foliage_near_structures_but_keeps_distant_trees() {
        let profile = tree_profiles::profile("minecraft:oak").unwrap();
        let states = [tree_forms::TreeVoxelState {
            position: [0, 0, 4],
            resource_id: "minecraft:oak_leaves",
            properties: Vec::new(),
        }];
        assert!(tree_canopy_overlaps_structure_clearance(
            [100, 200, 64],
            &states,
            profile,
            &BTreeSet::from([[102, 200]]),
        ));
        assert!(!tree_canopy_overlaps_structure_clearance(
            [100, 200, 64],
            &states,
            profile,
            &BTreeSet::from([[104, 200]]),
        ));
    }

    #[test]
    fn natural_riverbank_litter_stays_above_water_and_keeps_dry_patches() {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            vegetation_percent: 100,
            ..Default::default()
        };
        let world = prepare(
            Region {
                minimum: [-15216, -16504],
                maximum: [-15168, -16456],
            },
            &settings,
            || false,
        )
        .unwrap();
        assert!(
            world.fluids.contains_key(&[-15202, -16479, 62]),
            "original collision water must remain present"
        );
        for (position, _) in &world.fluids {
            let ground = i32::from(world.columns[&[position[0], position[1]]].height) - 1;
            for z in (ground + 1)..=position[2] {
                assert!(
                    world.fluids.contains_key(&[position[0], position[1], z])
                        || world.blocks.contains_key(&[position[0], position[1], z]),
                    "water column has a gap at [{}, {}, {}] below {:?}",
                    position[0],
                    position[1],
                    z,
                    position
                );
            }
        }
        let mut dry_litter = 0;
        for (position, block) in &world.blocks {
            if !matches!(block.owner, SourceOwner::TreeDecoration { .. }) {
                continue;
            }
            if block.state.id().as_str() == "minecraft:leaf_litter"
                && !world.fluids.contains_key(position)
            {
                dry_litter += 1;
            }
            assert!(
                !world.fluids.contains_key(position),
                "tree decoration {} overlaps retained water at {position:?}: {:?}",
                block.state.id().as_str(),
                block.owner
            );
        }
        assert!(dry_litter > 0, "dry leaf litter must remain nonvacuous");
    }

    #[test]
    fn natural_riverbank_litter_keeps_water_and_split_projection() {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            vegetation_percent: 100,
            ..Default::default()
        };
        let region = Region {
            minimum: [-15216, -16504],
            maximum: [-15168, -16456],
        };
        let whole = prepare(region, &settings, || false).unwrap();
        let projection = |world: &SurfaceWorld| -> BTreeMap<_, _> {
            world
                .blocks
                .iter()
                .filter(|(_, b)| matches!(b.owner, SourceOwner::TreeDecoration { .. }))
                .map(|(p, b)| (*p, (b.state.clone(), b.owner.clone())))
                .collect()
        };
        let expected = projection(&whole);
        assert!(expected
            .values()
            .any(|(s, _)| s.id().as_str() == "minecraft:leaf_litter"));
        assert!(whole.fluids.contains_key(&[-15202, -16479, 62]));
        let mut assembled = BTreeMap::new();
        let mut fluids = BTreeMap::new();
        for piece in [
            Region {
                minimum: region.minimum,
                maximum: [-15192, region.maximum[1]],
            },
            Region {
                minimum: [-15192, region.minimum[1]],
                maximum: region.maximum,
            },
        ] {
            let world = prepare(piece, &settings, || false).unwrap();
            for (p, b) in &world.blocks {
                if matches!(b.owner, SourceOwner::TreeDecoration { .. }) {
                    assert!(
                        !world.fluids.contains_key(p),
                        "tree decoration overlaps water at {p:?}"
                    );
                }
            }
            assembled.extend(projection(&world));
            fluids.extend(world.fluids);
        }
        assert_eq!(
            assembled, expected,
            "signed split changed dry decoration state/owner"
        );
        assert_eq!(
            fluids, whole.fluids,
            "water was removed or shifted by decoration admission"
        );
    }

    #[test]
    fn surface_slime_habitat_filters_underground_table_entries() {
        for biome in SurfaceBiome::all() {
            assert_eq!(
                biome
                    .descriptor()
                    .biome_table_fauna_ids
                    .contains(&"minecraft:slime"),
                *biome != SurfaceBiome::MushroomFields,
                "{}",
                biome.id()
            );
            let expected = matches!(biome, SurfaceBiome::Swamp | SurfaceBiome::MangroveSwamp);
            assert_eq!(
                surface_biome_allows_species(*biome, Species::Slime),
                expected,
                "{}",
                biome.id()
            );
            for species in surface_entities::ALL_SPECIES {
                if *species != Species::Slime {
                    assert!(
                        surface_biome_allows_species(*biome, *species),
                        "{} {}",
                        biome.id(),
                        species.id()
                    );
                }
            }
        }
    }

    #[test]
    fn night_does_not_admit_slime_at_natural_non_swamp_owner() {
        let center = [-13808, -16384];
        for atmosphere in 0..=2 {
            let settings = VoxelLandscapeSettings {
                seed: 71839,
                rivers: false,
                vegetation_percent: 0,
                atmosphere,
                ..Default::default()
            };
            let world = prepare(
                Region {
                    minimum: center.map(|v| v - 16),
                    maximum: center.map(|v| v + 16),
                },
                &settings,
                || false,
            )
            .unwrap();
            assert_eq!(world.biomes[&center], SurfaceBiome::OldGrowthBirchForest);
            assert_eq!(world.columns[&center].height, 112);
            assert!(
                !world
                    .entities
                    .iter()
                    .any(|entity| entity.species == Species::Slime && entity.anchor[..2] == center),
                "atmosphere {atmosphere}"
            );
        }
    }

    #[test]
    fn night_preserves_natural_slime_in_both_surface_swamp_habitats() {
        for (biome, anchor) in [
            (SurfaceBiome::Swamp, [9344, -16384, 63]),
            (SurfaceBiome::MangroveSwamp, [7728, -16384, 65]),
        ] {
            let center = [anchor[0], anchor[1]];
            let settings = VoxelLandscapeSettings {
                seed: 71839,
                rivers: false,
                vegetation_percent: 0,
                atmosphere: 1,
                ..Default::default()
            };
            let world = prepare(
                Region {
                    minimum: center.map(|v| v - 16),
                    maximum: center.map(|v| v + 16),
                },
                &settings,
                || false,
            )
            .unwrap();
            assert_eq!(world.biomes[&center], biome);
            assert_eq!(i32::from(world.columns[&center].height), anchor[2]);
            assert!(
                world
                    .entities
                    .iter()
                    .any(|entity| entity.species == Species::Slime && entity.anchor == anchor),
                "{} natural habitat lost",
                biome.id()
            );
        }
    }

    #[test]
    fn generated_height_guard_rejects_outside_and_accepts_edge_cell() {
        let mut world = SurfaceWorld {
            region: Region {
                minimum: [0, 0],
                maximum: [1, 1],
            },
            seed: 0,
            columns: BTreeMap::new(),
            biomes: BTreeMap::new(),
            blocks: BTreeMap::new(),
            fluids: BTreeMap::new(),
            trees: Vec::new(),
            flora: Vec::new(),
            structures: Vec::new(),
            entities: Vec::new(),
            source_limitations: Vec::new(),
        };
        let fluid = FluidCell::new(0, [0.0, 0.0, 1.0]).unwrap();
        world.fluids.insert([0, 0, 320], fluid);
        assert!(validate_generated_height(&world).is_err());
        let fluid = world.fluids.remove(&[0, 0, 320]).unwrap();
        world.fluids.insert([0, 0, 319], fluid);
        assert!(validate_generated_height(&world).is_ok());
        let fluid = world.fluids.remove(&[0, 0, 319]).unwrap();
        world.fluids.insert([0, 0, -1], fluid);
        assert!(validate_generated_height(&world).is_err());
    }
    #[test]
    fn tiled_viewport_reproduces_whole_window_states_and_global_owners() {
        let region = Region {
            minimum: [15584, -16416],
            maximum: [15680, -16320],
        };
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            vegetation_percent: 100,
            ..Default::default()
        };
        let whole = prepare(region, &settings, || false).unwrap();
        let stitched = prepare_viewport(region, 2.8, [4096, 4096], &settings, || false).unwrap();
        assert!(!whole.trees.is_empty());
        assert_eq!(stitched.blocks.len(), whole.blocks.len());
        for (position, expected) in &whole.blocks {
            let actual = stitched
                .blocks
                .get(position)
                .unwrap_or_else(|| panic!("stitched viewport lost block at {position:?}"));
            assert_eq!(actual.state, expected.state, "state at {position:?}");
            assert_eq!(actual.owner, expected.owner, "owner at {position:?}");
        }
        assert_eq!(stitched.fluids, whole.fluids);
        let identities = |world: &SurfaceWorld| {
            world
                .entities
                .iter()
                .map(|entity| (entity.anchor, entity.species.id()))
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(identities(&stitched), identities(&whole));
    }

    #[test]
    fn tiled_viewport_keeps_whole_well_and_water_across_both_core_seams() {
        let anchor = [-2374, -12665, 94];
        let region = Region {
            minimum: [anchor[0] - 95, anchor[1] - 95],
            maximum: [anchor[0] + 33, anchor[1] + 33],
        };
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            ..Default::default()
        };
        let whole = prepare(region, &settings, || false).unwrap();
        let stitched = prepare_viewport(region, 2.8, [4096, 4096], &settings, || false).unwrap();
        assert!(whole
            .structures
            .iter()
            .any(|record| { record.anchor == anchor && record.source == "minecraft:desert_well" }));
        assert_eq!(stitched.blocks.len(), whole.blocks.len());
        for (position, expected) in &whole.blocks {
            let actual = stitched
                .blocks
                .get(position)
                .unwrap_or_else(|| panic!("stitched viewport lost block at {position:?}"));
            assert_eq!(actual.state, expected.state, "state at {position:?}");
            assert_eq!(actual.owner, expected.owner, "owner at {position:?}");
        }
        assert_eq!(stitched.fluids, whole.fluids);
        assert!(stitched
            .structures
            .iter()
            .any(|record| { record.anchor == anchor && record.source == "minecraft:desert_well" }));
    }

    #[test]
    #[ignore = "explicit resource gate: generated native 360x240 source window"]
    fn witnessed_360_by_240_viewport_fits_source_limits() {
        let region =
            super::super::surface_viewport::region([-15011.0, -16246.0, 91.0], 2.8, [360, 240])
                .unwrap();
        let world = prepare_viewport(
            region,
            2.8,
            [360, 240],
            &VoxelLandscapeSettings::default(),
            || false,
        )
        .unwrap();
        println!(
            "{{\"type\":\"result\",\"blocks\":{},\"fluids\":{},\"columns\":{},\"trees\":{}}}",
            world.blocks.len(),
            world.fluids.len(),
            world.columns.len(),
            world.trees.len()
        );
        assert!(!world.trees.is_empty());
    }

    fn context_fixture(xy: [i32; 2], biome: SurfaceBiome) -> SurfaceWorld {
        let mut world = nest_fixture_world(SourceOwner::Terrain { biome });
        world.blocks.clear();
        world.biomes.clear();
        world.region = Region {
            minimum: [xy[0] - 16, xy[1] - 16],
            maximum: [xy[0] + 17, xy[1] + 17],
        };
        for y in world.region.minimum[1]..world.region.maximum[1] {
            for x in world.region.minimum[0]..world.region.maximum[0] {
                let mut sample = TerrainFields::new(world.seed).sample(x, y, false);
                sample.height = 80;
                sample.water_level = None;
                world.columns.insert([x, y], sample);
                world.biomes.insert([x, y], biome);
                world.blocks.insert(
                    [x, y, 79],
                    SurfaceBlock {
                        state: terrain_state(
                            surface_geology::terrain_materials(world.seed, biome, [x, y], 79).top,
                        )
                        .unwrap(),
                        owner: SourceOwner::Terrain { biome },
                    },
                );
            }
        }
        world
    }
    fn context_candidate(event: ContextEvent, atmosphere: SceneAtmosphere) -> [i32; 2] {
        (-64..64)
            .flat_map(|y| (-64..64).map(move |x| [x, y]))
            .find_map(|grid| {
                surface_context::candidate(71839, grid, atmosphere)
                    .filter(|(_, found)| *found == event)
                    .map(|(xy, _)| xy)
            })
            .unwrap()
    }
    #[test]
    fn contextual_phantom_requires_night_clear_air_and_is_not_duplicated() {
        let xy = context_candidate(ContextEvent::Phantom, SceneAtmosphere::Night);
        let mut world = context_fixture(xy, SurfaceBiome::Plains);
        populate_contextual_fauna(&mut world, SceneAtmosphere::Day, || false).unwrap();
        assert!(world.entities.is_empty());
        populate_contextual_fauna(&mut world, SceneAtmosphere::Night, || false).unwrap();
        assert_eq!(world.entities.len(), 1);
        assert_eq!(world.entities[0].species, Species::Phantom);
        assert_eq!(world.entities[0].anchor, [xy[0], xy[1], 92]);
        populate_contextual_fauna(&mut world, SceneAtmosphere::Night, || false).unwrap();
        assert_eq!(world.entities.len(), 1);
        let mut covered = context_fixture(xy, SurfaceBiome::Plains);
        covered.blocks.insert(
            [xy[0], xy[1], 100],
            SurfaceBlock {
                state: plain("minecraft:stone").unwrap(),
                owner: SourceOwner::Terrain {
                    biome: SurfaceBiome::Plains,
                },
            },
        );
        populate_contextual_fauna(&mut covered, SceneAtmosphere::Night, || false).unwrap();
        assert!(covered.entities.is_empty());
    }
    #[test]
    fn storm_mount_and_rider_commit_together_only_in_clear_rain_context() {
        let xy = context_candidate(ContextEvent::HorseTrap, SceneAtmosphere::Thunderstorm);
        let mut world = context_fixture(xy, SurfaceBiome::Plains);
        populate_contextual_fauna(&mut world, SceneAtmosphere::Thunderstorm, || false).unwrap();
        assert_eq!(world.entities.len(), 2);
        assert!(world
            .entities
            .iter()
            .any(|e| e.species == Species::SkeletonHorse));
        assert!(world
            .entities
            .iter()
            .any(|e| e.species == Species::Skeleton));
        assert!(world
            .entities
            .iter()
            .all(|e| e.anchor == [xy[0], xy[1], 80]));
        let mut blocked = context_fixture(xy, SurfaceBiome::Plains);
        blocked.blocks.insert(
            [xy[0], xy[1], 82],
            SurfaceBlock {
                state: plain("minecraft:stone").unwrap(),
                owner: SourceOwner::Terrain {
                    biome: SurfaceBiome::Plains,
                },
            },
        );
        populate_contextual_fauna(&mut blocked, SceneAtmosphere::Thunderstorm, || false).unwrap();
        assert!(blocked.entities.is_empty());
        let mut desert = context_fixture(xy, SurfaceBiome::Desert);
        populate_contextual_fauna(&mut desert, SceneAtmosphere::Thunderstorm, || false).unwrap();
        assert!(desert.entities.is_empty());
        assert!(matches!(
            populate_contextual_fauna(&mut desert, SceneAtmosphere::Thunderstorm, || true),
            Err(AssetError::Cancelled)
        ));
    }
    #[test]
    fn lightning_pig_requires_pig_habitat_dry_floor_and_storm() {
        let xy = context_candidate(ContextEvent::LightningPig, SceneAtmosphere::Thunderstorm);
        let mut world = context_fixture(xy, SurfaceBiome::Plains);
        populate_contextual_fauna(&mut world, SceneAtmosphere::Day, || false).unwrap();
        assert!(world.entities.is_empty());
        populate_contextual_fauna(&mut world, SceneAtmosphere::Thunderstorm, || false).unwrap();
        assert_eq!(world.entities.len(), 1);
        assert_eq!(world.entities[0].species, Species::ZombifiedPiglin);
        let mut wet = context_fixture(xy, SurfaceBiome::Plains);
        wet.columns.get_mut(&xy).unwrap().water_level = Some(81);
        populate_contextual_fauna(&mut wet, SceneAtmosphere::Thunderstorm, || false).unwrap();
        assert!(wet.entities.is_empty());
        let mut unsuitable = context_fixture(xy, SurfaceBiome::River);
        assert!(!SurfaceBiome::River
            .descriptor()
            .biome_table_fauna_ids
            .contains(&"minecraft:pig"));
        populate_contextual_fauna(&mut unsuitable, SceneAtmosphere::Thunderstorm, || false)
            .unwrap();
        assert!(unsuitable.entities.is_empty());
    }
    #[test]
    fn night_creaking_belongs_to_natural_pale_heart_and_preserves_axis() {
        let mut world = context_fixture([0, 0], SurfaceBiome::PaleGarden);
        let heart = [0, 0, 84];
        world.blocks.insert(
            heart,
            SurfaceBlock {
                state: state(
                    "minecraft:creaking_heart",
                    [
                        ("axis".into(), "y".into()),
                        ("creaking_heart_state".into(), "dormant".into()),
                    ],
                )
                .unwrap(),
                owner: SourceOwner::TreeDecoration {
                    anchor: [0, 0, 80],
                    configuration: "minecraft:pale_oak",
                },
            },
        );
        populate_contextual_fauna(&mut world, SceneAtmosphere::Day, || false).unwrap();
        assert!(world.entities.is_empty());
        assert_eq!(
            world.blocks[&heart].state.properties()["creaking_heart_state"],
            "dormant"
        );
        populate_contextual_fauna(&mut world, SceneAtmosphere::Night, || false).unwrap();
        assert_eq!(world.entities.len(), 1);
        assert_eq!(world.entities[0].species, Species::Creaking);
        assert_eq!(world.blocks[&heart].state.properties()["axis"], "y");
        assert_eq!(
            world.blocks[&heart].state.properties()["creaking_heart_state"],
            "awake"
        );
        populate_contextual_fauna(&mut world, SceneAtmosphere::Night, || false).unwrap();
        assert_eq!(world.entities.len(), 1);
        world.entities.clear();
        world.blocks.get_mut(&heart).unwrap().owner = SourceOwner::Saved {
            java_position: [0, 84, 0],
        };
        populate_contextual_fauna(&mut world, SceneAtmosphere::Night, || false).unwrap();
        assert!(world.entities.is_empty());
    }

    fn nest_fixture_world(owner: SourceOwner) -> SurfaceWorld {
        let position = [0, 0, 4];
        SurfaceWorld {
            region: Region {
                minimum: [-4, -4],
                maximum: [4, 4],
            },
            seed: 71839,
            columns: BTreeMap::new(),
            biomes: BTreeMap::from([([0, 0], SurfaceBiome::Forest)]),
            blocks: BTreeMap::from([(
                position,
                SurfaceBlock {
                    state: state(
                        "minecraft:bee_nest",
                        [
                            ("facing".into(), "north".into()),
                            ("honey_level".into(), "0".into()),
                        ],
                    )
                    .unwrap(),
                    owner,
                },
            )]),
            fluids: BTreeMap::new(),
            trees: vec![],
            flora: vec![],
            structures: vec![],
            entities: vec![],
            source_limitations: vec![],
        }
    }
    fn natural_nest_fixture() -> SurfaceWorld {
        nest_fixture_world(SourceOwner::TreeDecoration {
            anchor: [0, 1, 0],
            configuration: "minecraft:oak_bees_005",
        })
    }
    #[test]
    fn accepted_natural_nest_emits_one_bee_without_a_ground_requirement() {
        let mut world = natural_nest_fixture();
        populate_nest_bees(&mut world, || false).unwrap();
        assert_eq!(world.entities.len(), 1);
        assert_eq!(world.entities[0].species, Species::Bee);
        assert_eq!(world.entities[0].anchor, [0, -1, 4]);
        assert!(!world.blocks.contains_key(&[0, -1, 3]));
        populate_nest_bees(&mut world, || false).unwrap();
        assert_eq!(world.entities.len(), 1);
    }
    #[test]
    fn saved_nests_and_ineligible_biomes_never_gain_generated_bees() {
        let mut saved = nest_fixture_world(SourceOwner::Saved {
            java_position: [0, 4, 0],
        });
        populate_nest_bees(&mut saved, || false).unwrap();
        assert!(saved.entities.is_empty());
        let mut desert = natural_nest_fixture();
        desert.biomes.insert([0, 0], SurfaceBiome::Desert);
        populate_nest_bees(&mut desert, || false).unwrap();
        assert!(desert.entities.is_empty());
    }
    #[test]
    fn final_blocks_fluids_and_snapshot_edges_block_nest_flight() {
        let mut solid = natural_nest_fixture();
        solid.blocks.insert(
            [-1, -1, 4],
            SurfaceBlock {
                state: plain("minecraft:stone").unwrap(),
                owner: SourceOwner::Terrain {
                    biome: SurfaceBiome::Forest,
                },
            },
        );
        populate_nest_bees(&mut solid, || false).unwrap();
        assert!(solid.entities.is_empty());
        let mut wet = natural_nest_fixture();
        wet.fluids.insert(
            [1, -1, 4],
            FluidCell::new(0, water_tint(SurfaceBiome::Forest)).unwrap(),
        );
        populate_nest_bees(&mut wet, || false).unwrap();
        assert!(wet.entities.is_empty());
        let mut clipped = natural_nest_fixture();
        clipped.region.minimum[0] = 0;
        populate_nest_bees(&mut clipped, || false).unwrap();
        assert!(clipped.entities.is_empty());
        let mut cancelled = natural_nest_fixture();
        assert!(matches!(
            populate_nest_bees(&mut cancelled, || true),
            Err(AssetError::Cancelled)
        ));
        assert!(cancelled.entities.is_empty());
    }

    #[test]
    fn natural_surface_visitors_keep_the_complete_trader_and_two_llamas() {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            ..Default::default()
        };
        // Candidate/terrain positions found from actual default fields; final
        // world occupancy and the ordinary group consumer are exercised here.
        for center in [
            [631, -8176, 75],
            [651, -8142, 71],
            [4395, -7952, 99],
            [3858, -7701, 91],
            [1334, -7662, 75],
            [1384, -7640, 75],
            [-3864, -7477, 109],
            [1395, -7440, 66],
            [1390, -7401, 78],
        ] {
            let world = prepare(
                Region {
                    minimum: [center[0] - 8, center[1] - 8],
                    maximum: [center[0] + 8, center[1] + 8],
                },
                &settings,
                || false,
            )
            .unwrap();
            if !world
                .entities
                .iter()
                .any(|entity| entity.species == Species::WanderingTrader && entity.anchor == center)
            {
                continue;
            }
            for (species, offset) in [
                (Species::WanderingTrader, [0, 0]),
                (Species::TraderLlama, [-2, 1]),
                (Species::TraderLlama, [2, 1]),
            ] {
                let xy = [center[0] + offset[0], center[1] + offset[1]];
                let anchor = [xy[0], xy[1], i32::from(world.columns[&xy].height)];
                let members: Vec<_> = world
                    .entities
                    .iter()
                    .filter(|entity| entity.species == species && entity.anchor == anchor)
                    .collect();
                assert_eq!(members.len(), 1, "Visitor party is partial or duplicated");
                assert!(natural_entity_site(
                    &world,
                    anchor,
                    species,
                    &members[0].model
                ));
            }
            return;
        }
        panic!("Default natural candidate windows never admitted a whole visitor party");
    }

    #[test]
    fn fauna_climate_selects_real_model_skin_semantics() {
        for (biome, climate, expected_skin) in [
            (
                SurfaceBiome::Desert,
                ClimateSkin::Warm,
                "minecraft:cow/warm",
            ),
            (
                SurfaceBiome::SnowyTaiga,
                ClimateSkin::Cold,
                "minecraft:cow/cold",
            ),
            (
                SurfaceBiome::Plains,
                ClimateSkin::Temperate,
                "minecraft:cow",
            ),
        ] {
            assert_eq!(fauna_climate(biome), climate);
            let cow = surface_entities::model(Species::Cow, AtlasLayout::Bedrock, climate);
            assert!(cow
                .parts
                .iter()
                .any(|part| part.texture_semantic == expected_skin));
        }
    }
    #[test]
    fn fauna_grid_anchors_are_global_across_shifted_windows() {
        let anchors = |min: i32, max: i32| {
            (first_global_fauna_grid(min)..i64::from(max))
                .step_by(16)
                .collect::<Vec<_>>()
        };
        assert_eq!(anchors(8, 88), vec![16, 32, 48, 64, 80]);
        assert_eq!(
            anchors(0, 80)
                .into_iter()
                .filter(|x| *x >= 8)
                .collect::<Vec<_>>(),
            vec![16, 32, 48, 64]
        );
        assert_eq!(anchors(-17, 17), vec![-16, 0, 16]);
    }
    #[test]
    fn authored_flora_states_keep_ground_amounts_and_bamboo_segment_shapes() {
        for id in [
            "minecraft:leaf_litter",
            "minecraft:pink_petals",
            "minecraft:wildflowers",
        ] {
            let candidate = flower_candidate([4, 8, 64], id);
            let properties = &candidate.cells[0].state.properties;
            assert!(properties.iter().any(|(name, _)| *name == "facing"));
            let amount = if id == "minecraft:leaf_litter" {
                "segment_amount"
            } else {
                "flower_amount"
            };
            assert!(properties
                .iter()
                .any(|(name, value)| *name == amount && ["1", "2", "3", "4"].contains(value)));
        }
        let bamboo = flower_candidate([4, 8, 64], "minecraft:bamboo");
        assert_eq!(bamboo.cells.len(), 5);
        assert!(bamboo.cells[0]
            .state
            .properties
            .contains(&("leaves", "none")));
        assert!(bamboo.cells[3]
            .state
            .properties
            .contains(&("leaves", "small")));
        assert!(bamboo.cells[4]
            .state
            .properties
            .contains(&("leaves", "large")));
        let pitcher = flower_candidate([4, 8, 64], "minecraft:pitcher_plant");
        assert_eq!(pitcher.cells.len(), 2);
        assert!(pitcher.cells[1]
            .state
            .properties
            .contains(&("half", "upper")));
    }
    #[test]
    fn flora_selection_weights_favor_each_biomes_characteristic_cover() {
        assert!(
            flora_selection_weight(SurfaceBiome::Plains, "minecraft:short_grass")
                > flora_selection_weight(SurfaceBiome::Plains, "minecraft:poppy")
        );
        assert!(
            flora_selection_weight(SurfaceBiome::FlowerForest, "minecraft:poppy")
                > flora_selection_weight(SurfaceBiome::Plains, "minecraft:poppy")
        );
        assert!(
            flora_selection_weight(SurfaceBiome::Desert, "minecraft:short_dry_grass")
                > flora_selection_weight(SurfaceBiome::Plains, "minecraft:short_dry_grass")
        );
        assert!(
            flora_selection_weight(SurfaceBiome::Desert, "minecraft:cactus")
                > flora_selection_weight(SurfaceBiome::Plains, "minecraft:cactus")
        );

        let plains_candidates: Vec<_> = ENTRIES
            .iter()
            .filter(|entry| {
                !entry.attachment && entry.generation_biomes.contains(&SurfaceBiome::Plains.id())
            })
            .collect();
        let total_weight: u64 = plains_candidates
            .iter()
            .map(|entry| flora_selection_weight(SurfaceBiome::Plains, entry.id))
            .sum();
        let chosen: Vec<_> = (0..total_weight)
            .filter_map(|entropy| {
                choose_flora_entry(&plains_candidates, SurfaceBiome::Plains, entropy)
            })
            .collect();
        let grass_selections = chosen
            .iter()
            .filter(|entry| entry.id == "minecraft:short_grass")
            .count();
        let flower_selections = chosen
            .iter()
            .filter(|entry| entry.id == "minecraft:poppy")
            .count();
        assert!(grass_selections > flower_selections);
    }
    #[test]
    fn surface_azalea_indicators_are_reachable_without_replacing_forest_prescriptions() {
        let mut selected = BTreeSet::new();
        let mut azaleas = 0;
        for gx in -128..128 {
            for gy in -128..128 {
                let entropy = hash2(71839 ^ 0x7472_6565, gx, gy);
                let source = tree_configuration(SurfaceBiome::Forest, entropy).unwrap();
                selected.insert(source);
                azaleas += usize::from(source == "minecraft:azalea_tree");
            }
        }
        assert!(
            azaleas > 0 && azaleas < 1024,
            "Rare surface indicators selected {azaleas}/65536 times"
        );
        for source in SurfaceBiome::Forest
            .descriptor()
            .natural_tree_configuration_ids
        {
            assert!(
                selected.contains(source),
                "Ordinary prescription {source} lost"
            );
        }
        assert_eq!(tree_configuration(SurfaceBiome::Desert, 0), None);
    }

    #[test]
    fn natural_woodland_projects_a_surface_azalea_with_real_leaf_states() {
        let mut witnessed = false;
        for [x, y] in [[-3246, -17988], [-1758, -17949], [-3450, -17906]] {
            let world = prepare(
                Region {
                    minimum: [x - 16, y - 16],
                    maximum: [x + 16, y + 16],
                },
                &VoxelLandscapeSettings {
                    seed: 71839,
                    ..Default::default()
                },
                || false,
            )
            .unwrap();
            let anchor = [x, y, i32::from(world.columns[&[x, y]].height)];
            let record = world.trees.iter().any(|tree| {
                tree.anchor == anchor
                    && tree.source == "minecraft:azalea_tree"
                    && tree.projected_cells > 0
            });
            let leaf = world.blocks.values().any(|block| {
                matches!(&block.owner, SourceOwner::Tree {
                    anchor: tree_anchor, configuration: "minecraft:azalea_tree"
                } if *tree_anchor == anchor)
                    && matches!(
                        block.state.id().as_str(),
                        "minecraft:azalea_leaves" | "minecraft:flowering_azalea_leaves"
                    )
            });
            if record && leaf {
                witnessed = true;
                break;
            }
        }
        assert!(
            witnessed,
            "Natural surface indicators lost configuration or leaf projection"
        );
    }

    #[test]
    fn natural_desert_well_has_water_and_identical_split_projection() {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            ..Default::default()
        };
        // Actual chunk-admission/terrain witnesses; assembly and world projection
        // are exercised here, not substituted by a forced flat fixture.
        for anchor in [[-2374, -12665, 94], [-969, -12464, 77], [-13938, -8329, 96]] {
            let region = Region {
                minimum: [anchor[0] - 8, anchor[1] - 8],
                maximum: [anchor[0] + 12, anchor[1] + 12],
            };
            let whole = prepare(region, &settings, || false).unwrap();
            if !whole
                .structures
                .iter()
                .any(|f| f.source == "minecraft:desert_well" && f.anchor == anchor)
            {
                continue;
            }
            let placement = surface_landmark_assembly::desert_well_candidate(
                [anchor[0].div_euclid(16), anchor[1].div_euclid(16)],
                &TerrainFields::new(u64::from(settings.seed)),
                &settings,
                || false,
            )
            .unwrap()
            .unwrap();
            assert_eq!(placement.anchor, anchor);
            let expected_source = placement.source;
            let owned = |world: &SurfaceWorld| -> BTreeMap<_, _> {
                world.blocks.iter().filter(|(_, block)| matches!(&block.owner,
                    SourceOwner::Structure { anchor:a, source } if *a == anchor && source == &expected_source))
                    .map(|(position,block)| (*position,block.state.clone())).collect()
            };
            let expected = owned(&whole);
            assert!(expected.len() >= 25);
            for xy in [[2, 2], [1, 2], [3, 2], [2, 1], [2, 3]] {
                assert!(whole.fluids.contains_key(&[
                    anchor[0] + xy[0],
                    anchor[1] + xy[1],
                    anchor[2] + 1
                ]));
            }
            let left = prepare(
                Region {
                    minimum: region.minimum,
                    maximum: [anchor[0] + 2, region.maximum[1]],
                },
                &settings,
                || false,
            )
            .unwrap();
            let right = prepare(
                Region {
                    minimum: [anchor[0] + 2, region.minimum[1]],
                    maximum: region.maximum,
                },
                &settings,
                || false,
            )
            .unwrap();
            let mut joined = owned(&left);
            joined.extend(owned(&right));
            assert_eq!(joined, expected);
            return;
        }
        panic!("all natural desert-well witnesses rejected by actual assembly/projection");
    }

    #[test]
    fn natural_frozen_river_keeps_ice_and_fluid_cells_disjoint() {
        let world = prepare(
            Region {
                minimum: [8928, -15904],
                maximum: [8992, -15840],
            },
            &VoxelLandscapeSettings {
                seed: 71839,
                ..Default::default()
            },
            || false,
        )
        .unwrap();
        let ice = world
            .blocks
            .values()
            .filter(|block| block.state.id().as_str() == "minecraft:ice")
            .count();
        assert!(ice > 2500, "Frozenriver has only {ice} ice surfaces");
        assert!(world
            .fluids
            .keys()
            .all(|position| !world.blocks.contains_key(position)));
    }

    #[test]
    fn natural_ice_spikes_keep_packed_ice_feature_owners_above_ground() {
        let world = prepare(
            Region {
                minimum: [-11808, -15648],
                maximum: [-11744, -15584],
            },
            &VoxelLandscapeSettings {
                seed: 71839,
                ..Default::default()
            },
            || false,
        )
        .unwrap();
        let mut packed = 0;
        for (position, block) in &world.blocks {
            if !matches!(
                block.owner,
                SourceOwner::Geology {
                    source: "minecraft:ice_spike",
                    ..
                }
            ) {
                continue;
            }
            packed += 1;
            assert_eq!(block.state.id().as_str(), "minecraft:packed_ice");
            assert!(position[2] >= i32::from(world.columns[&[position[0], position[1]]].height));
            assert!(!world.fluids.contains_key(position));
        }
        assert!(
            packed > 100,
            "Naturalice-spike witness has only {packed} icefeaturecells"
        );
    }

    #[test]
    fn natural_forest_has_standing_cover_without_scattered_pumpkins() {
        let world = prepare(
            Region {
                minimum: [-2080, -16416],
                maximum: [-2016, -16352],
            },
            &VoxelLandscapeSettings {
                seed: 71839,
                ..Default::default()
            },
            || false,
        )
        .unwrap();
        let standing = world
            .trees
            .iter()
            .filter(|tree| tree.projected_cells > 0 && !tree.source.contains("fallen"))
            .count();
        let pumpkins = world
            .flora
            .iter()
            .filter(|flora| flora.projected_cells > 0 && flora.source == "minecraft:pumpkin")
            .count();
        assert!(
            standing >= 10,
            "Forest witness has only {standing} standing sources"
        );
        assert!(
            pumpkins <= 8,
            "Forest witness has {pumpkins} scattered pumpkins"
        );
    }

    #[test]
    fn natural_mangrove_roots_retain_water_and_mark_actual_wet_cells() {
        let world = prepare(
            Region {
                minimum: [-13088, -15648],
                maximum: [-13024, -15584],
            },
            &VoxelLandscapeSettings {
                seed: 71839,
                ..Default::default()
            },
            || false,
        )
        .unwrap();
        let mut wet_roots = 0;
        let mut dry_roots = 0;
        for (position, block) in &world.blocks {
            if block.state.id().as_str() != "minecraft:mangrove_roots" {
                continue;
            }
            if world.fluids.contains_key(position) {
                wet_roots += 1;
                assert_eq!(block.state.property("waterlogged"), Some("true"));
                assert!(matches!(block.owner, SourceOwner::Tree { .. }));
            } else {
                dry_roots += 1;
                assert_eq!(block.state.property("waterlogged"), Some("false"));
            }
        }
        assert!(
            wet_roots > 0 && dry_roots > 0,
            "Natural witness must exercise both root states"
        );
    }
    #[test]
    fn naturally_generated_lily_pads_float_on_water_not_on_the_bed() {
        let region = Region {
            minimum: [-13088, -15648],
            maximum: [-13024, -15584],
        };
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            vegetation_percent: 100,
            ..Default::default()
        };
        let world = prepare(region, &settings, || false).unwrap();
        let pads: Vec<_> = world
            .blocks
            .iter()
            .filter(|(_, block)| block.state.id().as_str() == "minecraft:lily_pad")
            .collect();
        assert!(
            !pads.is_empty(),
            "Natural mangrove witness must contain supported lily pads"
        );
        for (position, _) in pads {
            let below = [position[0], position[1], position[2] - 1];
            assert!(world.fluids.contains_key(&below));
            assert!(!world.blocks.contains_key(&below));
        }
    }
    #[test]
    fn world_tree_age_selection_contains_all_authored_growth_forms() {
        let ages: Vec<_> = (0_u64..100)
            .map(|value| tree_growth(value.rotate_right(23)))
            .collect();
        assert_eq!(ages.iter().filter(|&&age| age == Growth::Young).count(), 20);
        assert_eq!(
            ages.iter().filter(|&&age| age == Growth::Mature).count(),
            64
        );
        assert_eq!(ages.iter().filter(|&&age| age == Growth::Old).count(), 16);
        assert_eq!(tree_growth(97), tree_growth(97));
    }
    #[test]
    fn world_uses_current_biomes_and_global_feature_owners_without_an_old_ecology_tile() {
        let region = Region {
            minimum: [32, 32],
            maximum: [48, 48],
        };
        let settings = VoxelLandscapeSettings {
            vegetation_percent: 100,
            ..Default::default()
        };
        let world = prepare(region, &settings, || false).unwrap();
        assert_eq!(world.columns.len(), 18 * 18);
        assert!(world
            .blocks
            .values()
            .any(|b| matches!(b.owner, SourceOwner::Terrain { .. })));
        assert!(world
            .biomes
            .values()
            .all(|b| SurfaceBiome::from_id(b.id()) == Some(*b)));
        assert!(world
            .blocks
            .iter()
            .all(|(position, _)| region.contains(*position)));
    }
    #[test]
    fn cancelled_and_oversized_candidates_never_publish_worlds() {
        let region = Region {
            minimum: [0, 0],
            maximum: [16, 16],
        };
        assert!(prepare(region, &VoxelLandscapeSettings::default(), || true).is_err());
        assert!(prepare(
            Region {
                minimum: [0, 0],
                maximum: [129, 16]
            },
            &VoxelLandscapeSettings::default(),
            || false
        )
        .is_err());
    }
    #[test]
    fn dry_grass_and_cactus_crown_factories_keep_exact_semantic_states() {
        for id in ["minecraft:short_dry_grass", "minecraft:tall_dry_grass"] {
            let candidate = flower_candidate([-4, -8, 80], id);
            assert_eq!(candidate.source_prescription, id);
            assert_eq!(candidate.support, Support::SoilOrSand);
            assert!(!candidate.cactus_clearance);
            assert_eq!(candidate.cells.len(), 1);
            assert_eq!(candidate.cells[0].position, [0, 0, 0]);
            assert_eq!(candidate.cells[0].state.resource_id, id);
            assert!(candidate.cells[0].state.properties.is_empty());
        }
        let bare = flower_candidate([-4, -8, 80], "minecraft:cactus");
        let flowering = flower_candidate([-4, -8, 80], "minecraft:cactus_flower");
        assert_eq!(bare.cells.len(), 2);
        assert_eq!(bare.source_prescription, "minecraft:cactus");
        assert_eq!(flowering.source_prescription, "minecraft:cactus_flower");
        assert_eq!(flowering.support, Support::Sand);
        assert!(flowering.cactus_clearance);
        assert_eq!(flowering.cells.len(), 3);
        for height in 0..2 {
            assert_eq!(bare.cells[height], flowering.cells[height]);
            assert_eq!(flowering.cells[height].position, [0, 0, height as i32]);
            assert_eq!(
                flowering.cells[height].state.resource_id,
                "minecraft:cactus"
            );
            assert_eq!(flowering.cells[height].state.properties, [("age", "0")]);
        }
        assert_eq!(flowering.cells[2].position, [0, 0, 2]);
        assert_eq!(
            flowering.cells[2].state.resource_id,
            "minecraft:cactus_flower"
        );
        assert!(flowering.cells[2].state.properties.is_empty());
    }

    #[test]
    fn natural_arid_world_contains_both_dry_grasses_and_owned_supported_cactus_crowns() {
        // Original source007 RED fixture: this region already contains real cacti.
        // No feature is inserted, selected by hand, or given a new density.
        let region = Region {
            minimum: [3296, -16416],
            maximum: [3360, -16352],
        };
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            vegetation_percent: 100,
            ..Default::default()
        };
        let world = prepare(region, &settings, || false).unwrap();
        let mut dry_grass_counts = [0_usize; 2];
        let mut flowering_cacti = 0_usize;
        assert!(world.blocks.values().any(|block| {
            block.state.id().as_str() == "minecraft:cactus"
                && matches!(block.owner, SourceOwner::Flora { .. })
        }));
        for (&position, block) in &world.blocks {
            let id = block.state.id().as_str();
            if let Some(index) = ["minecraft:short_dry_grass", "minecraft:tall_dry_grass"]
                .iter()
                .position(|dry_id| *dry_id == id)
            {
                dry_grass_counts[index] += 1;
                assert!(block.state.property("half").is_none());
                assert!(matches!(
                    &block.owner,
                    SourceOwner::Flora { anchor, prescription }
                        if *anchor == position && *prescription == id
                ));
                let below = [position[0], position[1], position[2] - 1];
                let support = world
                    .blocks
                    .get(&below)
                    .expect("dry grass has terrain below");
                assert!(matches!(
                    support.state.id().as_str(),
                    "minecraft:sand" | "minecraft:red_sand"
                ));
                continue;
            }
            if id != "minecraft:cactus_flower" {
                continue;
            }
            flowering_cacti += 1;
            let SourceOwner::Flora {
                anchor,
                prescription,
            } = &block.owner
            else {
                panic!("natural cactus flower has no flora owner at {position:?}");
            };
            assert_eq!(*prescription, "minecraft:cactus_flower");
            assert_eq!([position[0], position[1]], [anchor[0], anchor[1]]);
            assert_eq!(position[2], anchor[2] + 2);
            assert!(block.state.property("age").is_none());
            for height in 0..2 {
                let stem_position = [anchor[0], anchor[1], anchor[2] + height];
                let stem = world
                    .blocks
                    .get(&stem_position)
                    .expect("whole owned cactus stem");
                assert_eq!(stem.state.id().as_str(), "minecraft:cactus");
                assert_eq!(stem.state.property("age"), Some("0"));
                assert_eq!(stem.owner, block.owner);
            }
            let sand_position = [anchor[0], anchor[1], anchor[2] - 1];
            assert!(matches!(
                world
                    .blocks
                    .get(&sand_position)
                    .map(|support| support.state.id().as_str()),
                Some("minecraft:sand" | "minecraft:red_sand")
            ));
            assert!(world.flora.iter().any(|record| {
                record.anchor == *anchor
                    && record.source == "minecraft:cactus_flower"
                    && record.projected_cells == 3
            }));
        }
        assert!(
            dry_grass_counts.into_iter().all(|count| count > 0),
            "natural sandy witness must contain both dry-grass resources"
        );
        assert!(
            flowering_cacti > 0,
            "natural cactus witness must contain an actual supported flower"
        );
    }

    fn relevant_flora_projection(
        world: &SurfaceWorld,
    ) -> BTreeMap<[i32; 3], (BlockState, SourceOwner)> {
        world
            .blocks
            .iter()
            .filter_map(|(&position, block)| {
                let id = block.state.id().as_str();
                if matches!(
                    id,
                    "minecraft:cactus"
                        | "minecraft:cactus_flower"
                        | "minecraft:short_dry_grass"
                        | "minecraft:tall_dry_grass"
                ) && matches!(block.owner, SourceOwner::Flora { .. })
                {
                    Some((position, (block.state.clone(), block.owner.clone())))
                } else {
                    None
                }
            })
            .collect()
    }

    fn relevant_tree_projection(
        world: &SurfaceWorld,
    ) -> BTreeMap<[i32; 3], (BlockState, SourceOwner)> {
        world
            .blocks
            .iter()
            .filter(|(_, block)| {
                matches!(
                    block.owner,
                    SourceOwner::Tree { .. } | SourceOwner::TreeDecoration { .. }
                )
            })
            .map(|(&position, block)| (position, (block.state.clone(), block.owner.clone())))
            .collect()
    }

    fn tree_anchors_crossing_tile_cores(
        projection: &BTreeMap<[i32; 3], (BlockState, SourceOwner)>,
        tiles: &[(Region, Region)],
    ) -> Vec<([i32; 3], BTreeSet<usize>)> {
        let mut owners_by_anchor = BTreeMap::<[i32; 3], BTreeSet<usize>>::new();
        for (&position, (_, owner)) in projection {
            let anchor = match owner {
                SourceOwner::Tree { anchor, .. } | SourceOwner::TreeDecoration { anchor, .. } => {
                    *anchor
                }
                _ => continue,
            };
            for (tile_index, (core, _)) in tiles.iter().enumerate() {
                if core.contains(position) {
                    owners_by_anchor
                        .entry(anchor)
                        .or_default()
                        .insert(tile_index);
                }
            }
        }
        owners_by_anchor
            .into_iter()
            .filter(|(_, tile_indices)| tile_indices.len() > 1)
            .collect()
    }

    #[derive(Debug, Default)]
    struct WoodedFloraCoverage {
        prescriptions: [usize; 4],
        dry_supports: [usize; 2],
    }

    fn assert_owned_arid_flora(
        world: &SurfaceWorld,
        settings: &VoxelLandscapeSettings,
        counted_region: Region,
    ) -> WoodedFloraCoverage {
        let fields = TerrainFields::new(u64::from(settings.seed));
        let mut groups =
            BTreeMap::<([i32; 3], &'static str), BTreeMap<[i32; 3], BlockState>>::new();
        let resources = [
            "minecraft:cactus",
            "minecraft:cactus_flower",
            "minecraft:short_dry_grass",
            "minecraft:tall_dry_grass",
        ];
        for (&position, block) in &world.blocks {
            let SourceOwner::Flora {
                anchor,
                prescription,
            } = &block.owner
            else {
                continue;
            };
            if !resources.contains(prescription) && !resources.contains(&block.state.id().as_str())
            {
                continue;
            }
            assert!(groups
                .entry((*anchor, *prescription))
                .or_default()
                .insert(position, block.state.clone())
                .is_none());
        }
        for record in &world.flora {
            if record.projected_cells > 0 && resources.contains(&record.source) {
                assert!(
                    groups.contains_key(&(record.anchor, record.source)),
                    "projected arid flora record lost its owned cells"
                );
            }
        }
        let mut coverage = WoodedFloraCoverage::default();
        for ((anchor, prescription), cells) in groups {
            let (index, height) = match prescription {
                "minecraft:cactus" => (0, 2),
                "minecraft:cactus_flower" => (1, 3),
                "minecraft:short_dry_grass" => (2, 1),
                "minecraft:tall_dry_grass" => (3, 1),
                other => panic!("arid flora has an unrelated prescription: {other}"),
            };
            let expected: BTreeMap<_, _> = (0..height)
                .map(|level| {
                    let block_state = if index < 2 && level < 2 {
                        state("minecraft:cactus", [("age".into(), "0".into())]).unwrap()
                    } else {
                        plain(prescription).unwrap()
                    };
                    ([anchor[0], anchor[1], anchor[2] + level], block_state)
                })
                .collect();
            assert_eq!(
                cells, expected,
                "incomplete or altered natural plant at {anchor:?}: {prescription}"
            );
            let records: Vec<_> = world
                .flora
                .iter()
                .filter(|record| record.anchor == anchor && record.source == prescription)
                .collect();
            assert_eq!(records.len(), 1, "plant must have one source record");
            assert_eq!(records[0].projected_cells, cells.len());
            assert!(records[0].authored_placement);

            let xy = [anchor[0], anchor[1]];
            let column = world.columns.get(&xy).expect("natural plant column");
            let biome = *world.biomes.get(&xy).expect("natural plant biome");
            assert_eq!(*column, fields.sample(xy[0], xy[1], settings.rivers));
            assert_eq!(anchor[2], i32::from(column.height));
            let below = [anchor[0], anchor[1], anchor[2] - 1];
            let support = world.blocks.get(&below).expect("emitted plant support");
            assert_eq!(support.owner, SourceOwner::Terrain { biome });
            let support_habitat = match support.state.id().as_str() {
                "minecraft:sand" | "minecraft:red_sand" => HabitatCell::Sand,
                "minecraft:grass_block" | "minecraft:coarse_dirt" => HabitatCell::Soil,
                id => panic!("unexpected arid plant support at {below:?}: {id}"),
            };
            assert_eq!(habitat(&fields, settings, below), support_habitat);
            assert!(dry_surface_support(world, anchor));
            assert!(!world.fluids.contains_key(&below));
            for &position in cells.keys() {
                assert!(world.region.contains(position));
                assert!(!world.fluids.contains_key(&position));
                if index < 2 {
                    for [dx, dy] in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
                        let side = [position[0] + dx, position[1] + dy, position[2]];
                        if world.region.contains(side) {
                            assert!(!world.blocks.contains_key(&side), "blocked cactus side");
                            assert!(!world.fluids.contains_key(&side), "wet cactus side");
                        }
                    }
                }
            }
            if index < 2 {
                assert_eq!(support_habitat, HabitatCell::Sand);
            }
            if biome == SurfaceBiome::WoodedBadlands
                && below[2] >= 100
                && counted_region.contains(anchor)
            {
                coverage.prescriptions[index] += 1;
                if index >= 2 {
                    let support_index = usize::from(support_habitat == HabitatCell::Soil);
                    coverage.dry_supports[support_index] += 1;
                }
            }
        }
        coverage
    }

    fn discover_wooded_flora_seam(settings: &VoxelLandscapeSettings) -> (SurfaceWorld, [i32; 3]) {
        // This is a bounded search of generated output, not a recorded witness.
        // Sampling chooses wooded plateaus; only prepare can supply the plants.
        const MAX_PREPARES: usize = 32;
        let fields = TerrainFields::new(u64::from(settings.seed));
        let mut prepared = 0;
        for gy in -128..=128 {
            for gx in -128..=128 {
                let center = [gx * 128, gy * 128];
                let (sample, biome) = sample_ground(&fields, settings, center[0], center[1]);
                if biome != SurfaceBiome::WoodedBadlands
                    || sample.height < 101
                    || sample.water_level.is_some()
                {
                    continue;
                }
                assert!(
                    prepared < MAX_PREPARES,
                    "wooded flora discovery exhausted {MAX_PREPARES} public prepares"
                );
                prepared += 1;
                let region = Region {
                    minimum: center.map(|v| v - 64),
                    maximum: center.map(|v| v + 64),
                };
                let interior = Region {
                    minimum: region.minimum.map(|v| v + 8),
                    maximum: region.maximum.map(|v| v - 8),
                };
                let world = prepare(region, settings, || false).unwrap();
                let coverage = assert_owned_arid_flora(&world, settings, interior);
                let mut materials = [0_usize; 3];
                for (&xy, column) in &world.columns {
                    if world.biomes.get(&xy) != Some(&SurfaceBiome::WoodedBadlands)
                        || column.height < 101
                    {
                        continue;
                    }
                    let position = [xy[0], xy[1], i32::from(column.height) - 1];
                    let Some(block) = world.blocks.get(&position) else {
                        continue;
                    };
                    if block.owner
                        != (SourceOwner::Terrain {
                            biome: SurfaceBiome::WoodedBadlands,
                        })
                    {
                        continue;
                    }
                    let index = match block.state.id().as_str() {
                        "minecraft:red_sand" => 0,
                        "minecraft:coarse_dirt" => 1,
                        "minecraft:grass_block" => 2,
                        id => panic!("wooded plateau lost its material mosaic: {id}"),
                    };
                    materials[index] += 1;
                }
                let interior_trees = world
                    .blocks
                    .iter()
                    .filter(|(position, block)| {
                        interior.contains(**position)
                            && matches!(block.owner, SourceOwner::Tree { .. })
                    })
                    .count();
                eprintln!(
                    "wooded_flora_discovery seed={} attempt={prepared} region={region:?} \
                     coverage={coverage:?} materials={materials:?} interior_trees={interior_trees}",
                    settings.seed
                );
                if coverage.prescriptions.contains(&0)
                    || coverage.dry_supports.contains(&0)
                    || materials.contains(&0)
                    || world.trees.is_empty()
                    || interior_trees == 0
                {
                    continue;
                }
                let anchor = world.blocks.iter().find_map(|(&position, block)| {
                    let SourceOwner::Flora {
                        anchor,
                        prescription: "minecraft:cactus_flower",
                    } = &block.owner
                    else {
                        return None;
                    };
                    (block.state.id().as_str() == "minecraft:cactus_flower"
                        && position == [anchor[0], anchor[1], anchor[2] + 2]
                        && interior.contains(*anchor)
                        && anchor[2] >= 101
                        && world.biomes.get(&[anchor[0], anchor[1]])
                            == Some(&SurfaceBiome::WoodedBadlands))
                    .then_some(*anchor)
                });
                let anchor = anchor.expect("positive flowering coverage needs a real crown");
                eprintln!(
                    "wooded_flora_witness seed={} region={region:?} anchor={anchor:?} split_x={}",
                    settings.seed,
                    anchor[0] + 1
                );
                return (world, anchor);
            }
        }
        panic!(
            "no complete natural wooded flora/tree/mosaic witness in 66049 coarse sites \
             and {prepared} public prepares"
        );
    }

    #[test]
    fn wooded_badlands_soil_changes_historical_cactus_support_without_changing_height() {
        let region = Region {
            minimum: [15584, -16416],
            maximum: [15648, -16352],
        };
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            vegetation_percent: 100,
            ..Default::default()
        };
        let world = prepare(region, &settings, || false).unwrap();
        let fields = TerrainFields::new(u64::from(settings.seed));
        assert!(
            !world.trees.is_empty(),
            "historical wooded region still needs trees"
        );
        assert!(
            !relevant_tree_projection(&world).is_empty(),
            "historical wooded region still needs projected trees"
        );
        for (&xy, column) in &world.columns {
            assert_eq!(*column, fields.sample(xy[0], xy[1], settings.rivers));
        }
        // Primary's original public readback had red sand and cactus at these
        // roots. The new soil mosaic intentionally changes their eligibility.
        for (anchor, expected_state) in [
            (
                [15634, -16374, 119],
                plain("minecraft:coarse_dirt").unwrap(),
            ),
            (
                [15634, -16358, 119],
                state("minecraft:grass_block", [("snowy".into(), "false".into())]).unwrap(),
            ),
        ] {
            let xy = [anchor[0], anchor[1]];
            assert_eq!(world.columns[&xy].height, 119);
            assert_eq!(world.biomes[&xy], SurfaceBiome::WoodedBadlands);
            let below = [anchor[0], anchor[1], 118];
            let support = world
                .blocks
                .get(&below)
                .expect("historical terrain surface");
            assert_eq!(support.state, expected_state);
            assert_eq!(
                support.owner,
                SourceOwner::Terrain {
                    biome: SurfaceBiome::WoodedBadlands,
                }
            );
            assert_eq!(habitat(&fields, &settings, below), HabitatCell::Soil);
            assert!(dry_surface_support(&world, anchor));
            assert!(!world.fluids.contains_key(&below));
            assert!(world.flora.iter().all(|record| {
                record.anchor != anchor
                    || !matches!(
                        record.source,
                        "minecraft:cactus" | "minecraft:cactus_flower"
                    )
            }));
            for level in 0..3 {
                let position = [anchor[0], anchor[1], anchor[2] + level];
                assert!(!world.fluids.contains_key(&position));
                assert!(!world.blocks.get(&position).is_some_and(|block| {
                    matches!(
                        block.state.id().as_str(),
                        "minecraft:cactus" | "minecraft:cactus_flower"
                    )
                }));
            }
        }
    }

    #[test]
    fn natural_wooded_badlands_flora_keep_global_owners_across_split_and_shifted_windows() {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            vegetation_percent: 100,
            ..Default::default()
        };
        let (whole, cactus_anchor) = discover_wooded_flora_seam(&settings);
        let region = whole.region;
        assert!(!whole.trees.is_empty(), "wooded seam needs actual trees");
        let expected_trees = relevant_tree_projection(&whole);
        assert!(
            !expected_trees.is_empty(),
            "wooded seam needs projected trees"
        );
        let viewport_tiles =
            super::super::surface_viewport::visible_tiles(region, 2.8, [4096, 4096]).unwrap();
        let crossing_trees = tree_anchors_crossing_tile_cores(&expected_trees, &viewport_tiles);
        assert!(
            !crossing_trees.is_empty(),
            "a naturally generated tree must cross selected viewport tile cores"
        );
        let (crossing_anchor, selected_tile_indices) = &crossing_trees[0];
        let expected_selected_tiles: BTreeMap<_, _> = expected_trees
            .iter()
            .filter(|(position, _)| {
                selected_tile_indices
                    .iter()
                    .any(|&index| viewport_tiles[index].0.contains(**position))
            })
            .map(|(&position, owned_block)| (position, owned_block.clone()))
            .collect();
        for reverse in [false, true] {
            let mut tile_indices: Vec<_> = selected_tile_indices.iter().copied().collect();
            if reverse {
                tile_indices.reverse();
            }
            let mut joined = BTreeMap::new();
            for tile_index in tile_indices {
                let (core, expanded) = viewport_tiles[tile_index];
                let tile = prepare(expanded, &settings, || false).unwrap();
                for (position, owned_block) in relevant_tree_projection(&tile) {
                    if core.contains(position) {
                        assert!(
                            joined.insert(position, owned_block).is_none(),
                            "tree cell {position:?} has duplicate viewport-core ownership"
                        );
                    }
                }
            }
            assert_eq!(
                joined, expected_selected_tiles,
                "selected tile order changed natural tree {crossing_anchor:?}; reverse={reverse}"
            );
        }
        let cactus = whole
            .blocks
            .get(&cactus_anchor)
            .expect("natural boundary cactus");
        assert_eq!(cactus.state.id().as_str(), "minecraft:cactus");
        assert_eq!(
            cactus.owner,
            SourceOwner::Flora {
                anchor: cactus_anchor,
                prescription: "minecraft:cactus_flower",
            }
        );
        let expected = relevant_flora_projection(&whole);
        // The first excluded left-column is the witnessed cactus's east side.
        let split_x = cactus_anchor[0] + 1;
        let left_region = Region {
            minimum: region.minimum,
            maximum: [split_x, region.maximum[1]],
        };
        let right_region = Region {
            minimum: [split_x, region.minimum[1]],
            maximum: region.maximum,
        };
        assert!(left_region.contains(cactus_anchor));
        for level in 0..3 {
            let side = [split_x, cactus_anchor[1], cactus_anchor[2] + level];
            assert!(!left_region.contains(side));
            assert!(right_region.contains(side));
            assert!(!whole.blocks.contains_key(&side));
            assert!(!whole.fluids.contains_key(&side));
        }
        for reverse in [false, true] {
            let parts = if reverse {
                [right_region, left_region]
            } else {
                [left_region, right_region]
            };
            let mut joined = BTreeMap::new();
            let mut joined_trees = BTreeMap::new();
            for part_region in parts {
                let part = prepare(part_region, &settings, || false).unwrap();
                assert_owned_arid_flora(&part, &settings, part_region);
                assert!(part.blocks.keys().all(|p| part_region.contains(*p)));
                for (position, owned_block) in relevant_flora_projection(&part) {
                    assert!(joined.insert(position, owned_block).is_none());
                }
                for (position, owned_block) in relevant_tree_projection(&part) {
                    assert!(joined_trees.insert(position, owned_block).is_none());
                }
            }
            assert_eq!(
                joined, expected,
                "split projection changed cactus/dry-grass states or global owners; reverse={reverse}"
            );
            assert_eq!(
                joined_trees, expected_trees,
                "split projection changed tree/decorator states or global owners; reverse={reverse}"
            );
        }

        for [dx, dy] in [[8, 8], [-8, -8]] {
            let shifted_region = Region {
                minimum: [region.minimum[0] + dx, region.minimum[1] + dy],
                maximum: [region.maximum[0] + dx, region.maximum[1] + dy],
            };
            let shifted = prepare(shifted_region, &settings, || false).unwrap();
            let overlap = Region {
                minimum: std::array::from_fn(|axis| {
                    region.minimum[axis].max(shifted_region.minimum[axis])
                }),
                maximum: std::array::from_fn(|axis| {
                    region.maximum[axis].min(shifted_region.maximum[axis])
                }),
            };
            assert!(overlap.contains(cactus_anchor));
            let coverage = assert_owned_arid_flora(&shifted, &settings, overlap);
            assert!(
                !coverage.prescriptions.contains(&0),
                "shifted flora must be nonvacuous"
            );
            assert!(
                !coverage.dry_supports.contains(&0),
                "shifted soil/sand flora must remain"
            );
            let expected_overlap: BTreeMap<_, _> = expected
                .iter()
                .filter(|(position, _)| shifted_region.contains(**position))
                .map(|(&position, owned_block)| (position, owned_block.clone()))
                .collect();
            let shifted_overlap: BTreeMap<_, _> = relevant_flora_projection(&shifted)
                .into_iter()
                .filter(|(position, _)| region.contains(*position))
                .collect();
            assert_eq!(
                shifted_overlap, expected_overlap,
                "shifted projection changed overlapping flora state or ownership"
            );
            let expected_tree_overlap: BTreeMap<_, _> = expected_trees
                .iter()
                .filter(|(position, _)| shifted_region.contains(**position))
                .map(|(&position, owned_block)| (position, owned_block.clone()))
                .collect();
            let shifted_tree_overlap: BTreeMap<_, _> = relevant_tree_projection(&shifted)
                .into_iter()
                .filter(|(position, _)| region.contains(*position))
                .collect();
            assert!(
                !expected_tree_overlap.is_empty(),
                "shifted tree overlap must be real"
            );
            assert_eq!(
                shifted_tree_overlap, expected_tree_overlap,
                "shifted projection changed overlapping tree/decorator states or owners"
            );
            // Whole then shifted, followed by shifted then whole, must agree.
            let after_shift = prepare(region, &settings, || false).unwrap();
            assert_eq!(relevant_flora_projection(&after_shift), expected);
            assert_eq!(relevant_tree_projection(&after_shift), expected_trees);
        }
        assert!(Region {
            minimum: [i32::MIN + 255, 0],
            maximum: [i32::MIN + 271, 16],
        }
        .validate()
        .is_err());
        assert!(Region {
            minimum: [i32::MAX - 271, 0],
            maximum: [i32::MAX - 255, 16],
        }
        .validate()
        .is_err());
    }

    #[test]
    fn off_window_tree_probe_at_crown_height_blocks_the_whole_flowering_cactus() {
        let mut world = context_fixture([6, 6], SurfaceBiome::Desert);
        world.region = Region {
            minimum: [2, 2],
            maximum: [7, 10],
        };
        let mut halo = cactus_tree_halo(&world, || false).unwrap();
        let off_window_tree_position = [7, 6, 82];
        assert_eq!(halo.get(&off_window_tree_position), Some(&false));
        // The tree projection loop marks a produced off-window tree cell in
        // this retained probe. Admission must see it at the flower's height.
        *halo.get_mut(&off_window_tree_position).unwrap() = true;
        let mut flora = FloraPlacement::new(16).unwrap();
        assert_eq!(
            flora.admit_cancellable(
                flower_candidate([6, 6, 80], "minecraft:cactus_flower"),
                |position| {
                    if halo.get(&position) == Some(&true) {
                        HabitatCell::Solid
                    } else if position[2] == 79 {
                        HabitatCell::Sand
                    } else {
                        HabitatCell::Air
                    }
                },
                || false,
            ),
            Err(AdmissionError::Obstructed(off_window_tree_position))
        );
        assert_eq!(flora.cells().count(), 0);
    }

    #[test]
    fn whole_fallen_beam_needs_ground_contact_but_can_bridge_two_shallow_cells() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([0, 0, 0], [0, 0, 1]).unwrap();
        geometry.branch([2, 0, 0], [6, 0, 0]).unwrap();
        let column = |xy: [i32; 2]| TreeTerrainColumn {
            solid_top: if (3..=4).contains(&xy[0]) { 79 } else { 80 },
            water_top: None,
        };
        assert!(
            tree_terrain_admits([0, 0, 80], TreeShape::Fallen, &geometry, column, || false)
                .unwrap()
        );
        assert!(!tree_terrain_admits(
            [0, 0, 80],
            TreeShape::Fallen,
            &geometry,
            |xy| TreeTerrainColumn {
                solid_top: if xy[0] >= 2 { 79 } else { 80 },
                water_top: None,
            },
            || false,
        )
        .unwrap());
        assert!(!tree_terrain_admits(
            [0, 0, 80],
            TreeShape::Fallen,
            &geometry,
            |xy| TreeTerrainColumn {
                solid_top: if xy[0] >= 5 { 79 } else { 80 },
                water_top: None,
            },
            || false,
        )
        .unwrap());
        let rotated = geometry.rotated(1);
        assert!(tree_terrain_admits(
            [0, 0, 80],
            TreeShape::Fallen,
            &rotated,
            |xy| TreeTerrainColumn {
                solid_top: if (3..=4).contains(&xy[1]) { 79 } else { 80 },
                water_top: None,
            },
            || false,
        )
        .unwrap());
    }

    #[test]
    fn standing_wood_and_leaves_require_clear_air_above_each_terrain_column() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([0, 0, 0], [0, 0, 3]).unwrap();
        geometry.canopy([1, 0, 1], [0, 0, 0]).unwrap();
        assert!(!tree_terrain_admits(
            [0, 0, 80],
            TreeShape::Rounded,
            &geometry,
            |xy| TreeTerrainColumn {
                solid_top: if xy[0] == 1 { 82 } else { 80 },
                water_top: None,
            },
            || false,
        )
        .unwrap());
        let mut wood = TreeGeometry::default();
        wood.branch([0, 0, 0], [0, 0, 3]).unwrap();
        wood.branch([1, 0, 0], [1, 0, 3]).unwrap();
        assert!(!tree_terrain_admits(
            [0, 0, 80],
            TreeShape::Dense,
            &wood,
            |xy| TreeTerrainColumn {
                solid_top: if xy[0] == 1 { 81 } else { 80 },
                water_top: None,
            },
            || false,
        )
        .unwrap());
        assert!(tree_terrain_admits(
            [0, 0, 80],
            TreeShape::Dense,
            &wood,
            |xy| TreeTerrainColumn {
                solid_top: if xy[0] == 1 { 79 } else { 80 },
                water_top: None,
            },
            || false,
        )
        .unwrap());
        for lowered_top in [78, 60] {
            assert!(!tree_terrain_admits(
                [0, 0, 80],
                TreeShape::Dense,
                &wood,
                |xy| TreeTerrainColumn {
                    solid_top: if xy[0] == 1 { lowered_top } else { 80 },
                    water_top: None,
                },
                || false,
            )
            .unwrap());
        }
        assert!(!tree_terrain_admits(
            [0, 0, 80],
            TreeShape::Rounded,
            &geometry,
            |_| TreeTerrainColumn {
                solid_top: 80,
                water_top: Some(82),
            },
            || false,
        )
        .unwrap());
        assert!(matches!(
            tree_terrain_admits(
                [0, 0, 80],
                TreeShape::Rounded,
                &geometry,
                |_| TreeTerrainColumn {
                    solid_top: 80,
                    water_top: None,
                },
                || true,
            ),
            Err(AssetError::Cancelled)
        ));
    }

    #[test]
    fn mangrove_props_can_enter_shallow_mud_and_water_but_must_reach_support() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([0, 0, 3], [0, 0, 6]).unwrap();
        geometry.root_branch([0, 0, 3], [4, 0, 0]).unwrap();
        assert!(tree_terrain_admits(
            [0, 0, 80],
            TreeShape::Mangrove,
            &geometry,
            |xy| TreeTerrainColumn {
                solid_top: if xy[0] == 4 { 82 } else { 80 },
                water_top: None,
            },
            || false,
        )
        .unwrap());
        assert!(tree_terrain_admits(
            [0, 0, 80],
            TreeShape::Mangrove,
            &geometry,
            |xy| TreeTerrainColumn {
                solid_top: if xy[0] == 0 { 80 } else { 76 },
                water_top: Some(83),
            },
            || false,
        )
        .unwrap());
        assert!(!tree_terrain_admits(
            [0, 0, 80],
            TreeShape::Mangrove,
            &geometry,
            |xy| TreeTerrainColumn {
                solid_top: if xy[0] == 4 { 84 } else { 80 },
                water_top: None,
            },
            || false,
        )
        .unwrap());
        assert!(!tree_terrain_admits(
            [0, 0, 80],
            TreeShape::Mangrove,
            &geometry,
            |xy| TreeTerrainColumn {
                solid_top: if xy[0] == 0 { 80 } else { 76 },
                water_top: None,
            },
            || false,
        )
        .unwrap());
    }

    #[test]
    fn terrain_rule_keeps_every_profile_and_growth_form_reachable_on_suitable_ground() {
        let growths = [Growth::Young, Growth::Mature, Growth::Old];
        for (index, profile) in tree_profiles::TREE_PROFILES.iter().enumerate() {
            for (age, growth) in growths.into_iter().enumerate() {
                let entropy = hash2(71839, index as i64, age as i64);
                let geometry = tree_forms::build(profile, growth, entropy).unwrap();
                assert!(
                    tree_terrain_admits(
                        [0, 0, 80],
                        profile.shape,
                        &geometry,
                        |_| TreeTerrainColumn {
                            solid_top: 80,
                            water_top: None,
                        },
                        || false,
                    )
                    .unwrap(),
                    "{} {growth:?} lost on flat suitable ground",
                    profile.id
                );
            }
        }
    }

    #[test]
    fn forced_unsupported_fallen_oak_is_absent_in_whole_partial_and_shifted_regions() {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            ..Default::default()
        };
        let unsupported: [i32; 3] = [-3963, -16372, 151];
        let supported: [i32; 3] = [2774, -16482, 86];
        let unsupported_entropy = hash2(
            u64::from(settings.seed) ^ 0x7472_6565,
            i64::from(unsupported[0].div_euclid(13)),
            i64::from(unsupported[1].div_euclid(13)),
        );
        for region in [
            Region {
                minimum: [unsupported[0] - 24, unsupported[1] - 24],
                maximum: [unsupported[0] + 24, unsupported[1] + 24],
            },
            Region {
                minimum: [unsupported[0], unsupported[1]],
                maximum: [unsupported[0] + 6, unsupported[1] + 6],
            },
            Region {
                minimum: [unsupported[0] - 4, unsupported[1] - 4],
                maximum: [unsupported[0] + 16, unsupported[1] + 16],
            },
        ] {
            let selected_unsupported = std::cell::Cell::new(false);
            let world = prepare_with_tree_configuration(
                region,
                &settings,
                |biome, entropy| {
                    if entropy != unsupported_entropy {
                        return tree_configuration(biome, entropy);
                    }
                    selected_unsupported.set(true);
                    Some("minecraft:fallen_oak_tree")
                },
                || false,
            )
            .unwrap();
            assert!(
                selected_unsupported.get(),
                "Unsupported candidate never reached selection"
            );
            assert!(!world.blocks.values().any(|block| {
                matches!(
                    &block.owner,
                    SourceOwner::Tree { anchor, .. } | SourceOwner::TreeDecoration { anchor, .. }
                        if *anchor == unsupported
                )
            }));
            assert!(!world.trees.iter().any(|tree| tree.anchor == unsupported));
        }
        let supported_entropy = hash2(
            u64::from(settings.seed) ^ 0x7472_6565,
            i64::from(supported[0].div_euclid(13)),
            i64::from(supported[1].div_euclid(13)),
        );
        let selected_supported = std::cell::Cell::new(false);
        let world = prepare_with_tree_configuration(
            Region {
                minimum: [supported[0] - 16, supported[1] - 16],
                maximum: [supported[0] + 16, supported[1] + 16],
            },
            &settings,
            |biome, entropy| {
                if entropy != supported_entropy {
                    return tree_configuration(biome, entropy);
                }
                selected_supported.set(true);
                Some("minecraft:fallen_oak_tree")
            },
            || false,
        )
        .unwrap();
        assert!(
            selected_supported.get(),
            "Supported candidate never reached selection"
        );
        assert!(world.trees.iter().any(|tree| tree.anchor == supported
            && tree.source == "minecraft:fallen_oak_tree"
            && tree.projected_cells > 0));
        assert!(world.blocks.values().any(|block| {
            matches!(
                &block.owner,
                SourceOwner::Tree { anchor, configuration: "minecraft:fallen_oak_tree" }
                    if *anchor == supported
            )
        }));
    }
}

#[cfg(test)]
#[path = "surface_ecology_preservation_tests.rs"]
mod ecology_preservation_tests;
#[cfg(test)]
#[path = "surface_ecology_tests.rs"]
mod ecology_regression_tests;

#[cfg(test)]
#[path = "surface_village_generation_tests.rs"]
mod village_layout_tests;

#[cfg(test)]
#[path = "surface_geology_public_tests.rs"]
mod geology_public_tests;

#[cfg(test)]
#[path = "surface_geology_tests.rs"]
mod geology_tests;

#[cfg(test)]
mod riverbank_drowned_regressions {
    use super::*;
    #[test]
    fn night_river_surface_has_drowned_on_clear_dry_bank() {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            atmosphere: 1,
            vegetation_percent: 0,
            ..Default::default()
        };
        let region = Region {
            minimum: [-15296, -16448],
            maximum: [-15168, -16320],
        };
        let world = prepare(region, &settings, || false).unwrap();
        let model = surface_entities::model(
            Species::Drowned,
            AtlasLayout::Bedrock,
            ClimateSkin::Temperate,
        );
        let (lo, hi) = model.bounds().unwrap();
        let mut bank_sites = Vec::new();
        let mut wet_river_columns = 0;
        for (&xy, sample) in &world.columns {
            if matches!(
                world.biomes.get(&xy),
                Some(SurfaceBiome::River | SurfaceBiome::FrozenRiver)
            ) && sample.water_level.is_some_and(|w| w > sample.height)
            {
                wet_river_columns += 1;
            }
            let feet = i32::from(sample.height);
            if sample.water_level.is_some_and(|w| i32::from(w) > feet) {
                continue;
            }
            let Some(floor) = world.blocks.get(&[xy[0], xy[1], feet - 1]) else {
                continue;
            };
            if !matches!(floor.owner, SourceOwner::Terrain { .. })
                || floor.state.id().as_str() != "minecraft:grass_block"
            {
                continue;
            }
            let nearby = (-8..=8).any(|dx| {
                (-8..=8).any(|dy| {
                    let river = [xy[0] + dx, xy[1] + dy];
                    world.columns.get(&river).is_some_and(|water| {
                        matches!(
                            world.biomes.get(&river),
                            Some(SurfaceBiome::River | SurfaceBiome::FrozenRiver)
                        ) && water.water_level.is_some_and(|level| {
                            level > water.height
                                && feet >= i32::from(level)
                                && feet <= i32::from(level) + 4
                        })
                    })
                })
            });
            if !nearby {
                continue;
            }
            let first = [
                (f64::from(xy[0]) + 0.5 + f64::from(lo[0])).floor() as i32,
                (f64::from(xy[1]) + 0.5 + f64::from(lo[1])).floor() as i32,
            ];
            let last = [
                (f64::from(xy[0]) + 0.5 + f64::from(hi[0])).ceil() as i32,
                (f64::from(xy[1]) + 0.5 + f64::from(hi[1])).ceil() as i32,
            ];
            if first[0] < region.minimum[0]
                || first[1] < region.minimum[1]
                || last[0] > region.maximum[0]
                || last[1] > region.maximum[1]
            {
                continue;
            }
            let mut clear = true;
            for y in first[1]..last[1] {
                for x in first[0]..last[0] {
                    for z in feet..feet + (hi[2].ceil() as i32) {
                        if world.blocks.contains_key(&[x, y, z])
                            || world.fluids.contains_key(&[x, y, z])
                        {
                            clear = false
                        }
                    }
                }
            }
            if clear {
                bank_sites.push([xy[0], xy[1], feet]);
            }
        }
        let drowned: Vec<_> = world
            .entities
            .iter()
            .filter(|e| e.species == Species::Drowned)
            .map(|e| e.anchor)
            .collect();
        assert!(
            wet_river_columns > 0,
            "fixture must contain actual river water"
        );
        assert!(
            !bank_sites.is_empty(),
            "fixture must contain collision-free original terrain bank"
        );
        assert!(
            drowned.iter().any(|a| bank_sites.contains(a)),
            "missing surface-only night riverbank drowned despite actual clear dry bank"
        );
    }

    #[test]
    fn riverbank_drowned_are_absent_in_day_storm_and_disabled_rivers() {
        for (atmosphere, rivers) in [(0, true), (2, true), (1, false)] {
            let world = prepare(
                Region {
                    minimum: [-15296, -16448],
                    maximum: [-15168, -16320],
                },
                &VoxelLandscapeSettings {
                    seed: 71839,
                    atmosphere,
                    rivers,
                    vegetation_percent: 0,
                    ..Default::default()
                },
                || false,
            )
            .unwrap();
            assert!(
                !world.entities.iter().any(|e| e.species == Species::Drowned),
                "phase={atmosphere} rivers={rivers}"
            );
        }
    }
    #[test]
    fn riverbank_drowned_reject_final_solid_obstructions() {
        let mut world = prepare(
            Region {
                minimum: [-15296, -16448],
                maximum: [-15168, -16320],
            },
            &VoxelLandscapeSettings {
                seed: 71839,
                atmosphere: 1,
                vegetation_percent: 0,
                ..Default::default()
            },
            || false,
        )
        .unwrap();
        let anchor = world
            .entities
            .iter()
            .find(|e| e.species == Species::Drowned)
            .expect("natural bank witness")
            .anchor;
        world.entities.retain(|e| e.anchor != anchor);
        // Synthetic obstruction in an owned test world; no production world data is changed.
        let obstruction = world.blocks[&[anchor[0], anchor[1], anchor[2] - 1]].clone();
        world.blocks.insert(anchor, obstruction);
        populate_riverbank_drowned(&mut world, true, SceneAtmosphere::Night, || false).unwrap();
        assert!(!world.entities.iter().any(|e| e.anchor == anchor));
    }
    #[test]
    fn riverbank_drowned_cancellation_keeps_existing_entities() {
        let mut world = prepare(
            Region {
                minimum: [-15296, -16448],
                maximum: [-15168, -16320],
            },
            &VoxelLandscapeSettings {
                seed: 71839,
                atmosphere: 1,
                vegetation_percent: 0,
                ..Default::default()
            },
            || false,
        )
        .unwrap();
        let before: Vec<_> = world
            .entities
            .iter()
            .map(|e| (e.species, e.anchor))
            .collect();
        assert!(matches!(
            populate_riverbank_drowned(&mut world, true, SceneAtmosphere::Night, || true),
            Err(AssetError::Cancelled)
        ));
        assert_eq!(
            before,
            world
                .entities
                .iter()
                .map(|e| (e.species, e.anchor))
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn riverbank_drowned_keep_signed_owners_across_split_and_shifted_windows() {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            atmosphere: 1,
            vegetation_percent: 0,
            ..Default::default()
        };
        let anchors = |region: Region| -> BTreeSet<[i32; 3]> {
            prepare(region, &settings, || false)
                .unwrap()
                .entities
                .iter()
                .filter(|e| e.species == Species::Drowned)
                .map(|e| e.anchor)
                .collect()
        };
        let whole = anchors(Region {
            minimum: [-15296, -16448],
            maximum: [-15168, -16320],
        });
        assert!(!whole.is_empty());
        let mut split = anchors(Region {
            minimum: [-15296, -16448],
            maximum: [-15232, -16320],
        });
        split.extend(anchors(Region {
            minimum: [-15232, -16448],
            maximum: [-15168, -16320],
        }));
        assert_eq!(whole, split);
        let shifted = anchors(Region {
            minimum: [-15295, -16447],
            maximum: [-15167, -16319],
        });
        let interior =
            |p: &&[i32; 3]| p[0] >= -15293 && p[0] < -15171 && p[1] >= -16445 && p[1] < -16323;
        assert_eq!(
            whole
                .iter()
                .filter(interior)
                .copied()
                .collect::<BTreeSet<_>>(),
            shifted.iter().filter(interior).copied().collect()
        );
    }
}

#[cfg(test)]
mod leaf_litter_default_regressions {
    use super::*;
    use crate::voxel_landscape::{assets::block_state::StateDefinition, surface_state_geometry};

    #[test]
    fn compact_generated_litter_selects_one_complete_variant() {
        let litter = plain("minecraft:leaf_litter").unwrap();
        assert_eq!(litter.property("facing"), Some("north"));
        assert_eq!(litter.property("segment_amount"), Some("1"));
        let geometry = surface_state_geometry::definitions(litter.id().as_str()).unwrap();
        assert_eq!(
            StateDefinition::parse(&geometry.blockstate)
                .unwrap()
                .select(&litter, [0; 3], 71839)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn generated_litter_preserves_explicit_variants_and_rejects_duplicates() {
        let supplied = [
            ("facing".to_owned(), "south".to_owned()),
            ("segment_amount".to_owned(), "3".to_owned()),
        ];
        let litter = state("minecraft:leaf_litter", supplied.clone()).unwrap();
        assert_eq!(litter.properties(), &supplied.into_iter().collect());
        assert!(state(
            "minecraft:leaf_litter",
            [
                ("facing".to_owned(), "north".to_owned()),
                ("facing".to_owned(), "south".to_owned())
            ]
        )
        .is_err());
    }

    #[test]
    fn both_natural_forest_witnesses_keep_renderable_litter() {
        let geometry = surface_state_geometry::definitions("minecraft:leaf_litter").unwrap();
        let definition = StateDefinition::parse(&geometry.blockstate).unwrap();
        for (xy, rivers) in [([6606, -2562], true), ([-15104, -16384], false)] {
            let settings = VoxelLandscapeSettings {
                seed: 71839,
                vegetation_percent: 100,
                rivers,
                ..Default::default()
            };
            let world = prepare(
                Region {
                    minimum: xy.map(|v| v - 16),
                    maximum: xy.map(|v| v + 16),
                },
                &settings,
                || false,
            )
            .unwrap();
            let litter: Vec<_> = world
                .blocks
                .iter()
                .filter(|(_, block)| block.state.id().as_str() == "minecraft:leaf_litter")
                .collect();
            assert!(
                !litter.is_empty(),
                "natural litter witness must not disappear"
            );
            for (position, block) in litter {
                assert_eq!(
                    definition
                        .select(&block.state, *position, world.seed)
                        .unwrap()
                        .len(),
                    1
                );
            }
        }
    }
}
