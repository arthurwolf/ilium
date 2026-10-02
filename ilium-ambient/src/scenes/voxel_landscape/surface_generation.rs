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
    surface_flora::{FloraPlacement, FloraState, HabitatCell, PlantCandidate, Support},
    surface_flora_vocabulary::ENTRIES,
    surface_fluid::FluidCell,
    surface_geology, surface_landmark_assembly, surface_ruin_assembly, surface_village_assembly,
    terrain_fields::{TerrainFields, TerrainSample},
    tree_decoration_profiles,
    tree_decorations::{DecorationOperation, DecorationPlanner, SurroundingCell},
    tree_forms::{self, Growth},
    tree_profiles, village_kit,
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
fn state(id: &str, properties: impl IntoIterator<Item = (String, String)>) -> Result<BlockState> {
    BlockState::new(ResourceId::parse(id)?, properties)
}
fn plain(id: &str) -> Result<BlockState> {
    state(id, Vec::<(String, String)>::new())
}
fn terrain_surface(biome: SurfaceBiome) -> (&'static str, &'static str) {
    use SurfaceBiome::*;
    match biome {
        Badlands | ErodedBadlands | WoodedBadlands => {
            ("minecraft:red_sand", "minecraft:terracotta")
        }
        Desert | Beach => ("minecraft:sand", "minecraft:sandstone"),
        SnowyBeach => ("minecraft:snow_block", "minecraft:sand"),
        StonyShore | StonyPeaks | WindsweptGravellyHills => ("minecraft:stone", "minecraft:stone"),
        FrozenPeaks => ("minecraft:snow_block", "minecraft:packed_ice"),
        JaggedPeaks => ("minecraft:snow_block", "minecraft:stone"),
        MushroomFields => ("minecraft:mycelium", "minecraft:dirt"),
        Swamp | MangroveSwamp => ("minecraft:mud", "minecraft:mud"),
        SnowyPlains | SnowySlopes | IceSpikes | SnowyTaiga | Grove => {
            ("minecraft:snow_block", "minecraft:dirt")
        }
        _ => ("minecraft:grass_block", "minecraft:dirt"),
    }
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
        let surface = terrain_surface(biome).0;
        if surface.contains("sand") {
            HabitatCell::Sand
        } else if matches!(
            surface,
            "minecraft:grass_block" | "minecraft:mycelium" | "minecraft:mud"
        ) {
            HabitatCell::Soil
        } else {
            HabitatCell::Solid
        }
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

// Authored bounded cover, informed by the differing pinned Java placement
// prescriptions. These percentages are not native chunk-attempt counts.
fn tree_cover_percent(biome: SurfaceBiome) -> u64 {
    use SurfaceBiome::*;
    match biome {
        BambooJungle | DarkForest | Jungle | PaleGarden => 85,
        BirchForest | DappledForest | Forest | SnowyTaiga | Taiga => 80,
        OldGrowthBirchForest | OldGrowthPineTaiga | OldGrowthSpruceTaiga => 75,
        MangroveSwamp => 70,
        CherryGrove | FlowerForest => 60,
        Grove => 55,
        WindsweptForest | MushroomFields => 45,
        Swamp | WoodedBadlands => 35,
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
    configs
        .get((entropy.rotate_left(11) as usize) % configs.len())
        .copied()
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
        "minecraft:cactus" => PlantCandidate::column(anchor, id, state, 2, support, true),
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
    let Some(floor_z) = feet.checked_sub(1) else {
        return false;
    };
    let Some(support) = world.blocks.get(&[x, y, floor_z]) else {
        return false;
    };
    if !matches!(support.owner, SourceOwner::Terrain { .. })
        || support.state.id().as_str() != terrain_surface(biome).0
        || world.columns.get(&[x, y]).is_some_and(|sample| {
            sample
                .water_level
                .is_some_and(|level| i32::from(level) > feet)
        })
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

pub fn prepare(
    region: Region,
    settings: &VoxelLandscapeSettings,
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
            "Flora attempts use authored sparse density; exact Java count/noise algorithms remain pending",
            "Surface azalea indicators use rare authored woodland selection; no underground cave/root network is generated",
            "Ice spire silhouettes and cold surface openings are authored; native feature processors are not reproduced",
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
            let (top, substrate) = terrain_surface(biome);
            let ground = i32::from(sample.height) - 1;
            let min_neighbor = [[x - 1, y], [x + 1, y], [x, y - 1], [x, y + 1]]
                .into_iter()
                .filter_map(|p| world.columns.get(&p))
                .map(|s| i32::from(s.height) - 1)
                .min()
                .unwrap_or(ground);
            let bottom = (min_neighbor - 1).min(ground - 3).max(ground - 64).max(0);
            for z in bottom..=ground {
                let id = if z == ground {
                    top
                } else if z >= ground - 3 {
                    substrate
                } else {
                    "minecraft:stone"
                };
                let block = if let Some(state) = terrain_states.get(id) {
                    state.clone()
                } else {
                    let state = if id == "minecraft:grass_block" {
                        state(id, [("snowy".to_owned(), "false".to_owned())])?
                    } else {
                        plain(id)?
                    };
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
                if i32::from(level) > ground + 1 {
                    let position = [x, y, i32::from(level) - 1];
                    if surface_geology::freezes_surface(biome, seed, [x, y]) {
                        world.blocks.insert(
                            position,
                            SurfaceBlock {
                                state: plain("minecraft:ice")?,
                                owner: SourceOwner::Terrain { biome },
                            },
                        );
                    } else {
                        world
                            .fluids
                            .insert(position, FluidCell::new(0, water_tint(biome))?);
                    }
                }
            }
        }
    }
    // Evaluate a whole globally owned village candidate before region projection.
    // One stable TerrainFields snapshot serves every piece habitat callback.
    let mut structure_positions = BTreeSet::new();
    let village_min = region.minimum.map(|value| (value - 192).div_euclid(256));
    let village_max = region.maximum.map(|value| (value + 192).div_euclid(256));
    for gy in village_min[1]..=village_max[1] {
        for gx in village_min[0]..=village_max[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
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
    // Evaluate whole trees by global grid anchor, then project. This keeps the
    // same configuration and owner when a camera/region boundary moves.
    let anchor_min = region.minimum.map(|v| (v - 48).div_euclid(13));
    let anchor_max = region.maximum.map(|v| (v + 48).div_euclid(13));
    for gy in anchor_min[1]..=anchor_max[1] {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        for gx in anchor_min[0]..=anchor_max[0] {
            let hash = hash2(seed ^ 0x7472_6565, i64::from(gx), i64::from(gy));
            let x = gx * 13 + 6 + (hash.rotate_left(29) % 9) as i32 - 4;
            let y = gy * 13 + 6 + (hash.rotate_left(43) % 9) as i32 - 4;
            let (sample, biome) = sample_ground(&fields, &settings, x, y);
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
            let Some(source) = tree_configuration(biome, hash) else {
                continue;
            };
            let Some(profile) = tree_profiles::profile(source) else {
                continue;
            };
            let anchor = [x, y, i32::from(sample.height)];
            let geometry = tree_forms::build(profile, tree_growth(hash), hash)
                .map_err(|_| AssetError::InvalidMetadata("tree geometry failed".into()))?;
            let states = tree_forms::bind_states(profile, &geometry, hash)
                .map_err(|_| AssetError::InvalidMetadata("tree state binding failed".into()))?;
            if states.iter().any(|cell| {
                add_global(anchor, cell.position)
                    .is_ok_and(|position| structure_positions.contains(&position))
            }) {
                continue;
            }
            let mut projected = 0;
            for cell in states {
                let position = add_global(anchor, cell.position)?;
                if !region.contains(position) {
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
                        HabitatCell::Air | HabitatCell::Water => SurroundingCell::Air,
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
                        i32::from(fields.sample(xx, yy, settings.rivers).height) - anchor[2],
                    )
                    .ok()
                };
                let _applications = planner.apply_profile(decorations, hash, ground_height);
                for decoration in planner.finish() {
                    let position = add_global(anchor, decoration.state.position)?;
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
            if hash % 100 >= (settings.vegetation_percent.min(100) as u64) * 55 / 100 {
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
            let entry = candidates[(hash.rotate_left(17) as usize) % candidates.len()];
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
            let admitted = flora.admit(candidate, |position| {
                if structure_positions.contains(&position)
                    || world.blocks.get(&position).is_some_and(|b| {
                        b.state.id().as_str() == "minecraft:ice"
                            || !matches!(b.owner, SourceOwner::Terrain { .. })
                    })
                {
                    HabitatCell::Solid
                } else {
                    habitat(&fields, &settings, position)
                }
            });
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
        if !region.contains(cell.position) {
            continue;
        }
        let (anchor, prescription) =
            flora_owners.get(&cell.position).copied().ok_or_else(|| {
                AssetError::InvalidMetadata("accepted flora lost its owner ledger".into())
            })?;
        let block = state(
            cell.state.resource_id,
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
    Ok(world)
}

#[cfg(test)]
mod tests {
    use super::*;
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
                        state: plain(terrain_surface(biome).0).unwrap(),
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
}
