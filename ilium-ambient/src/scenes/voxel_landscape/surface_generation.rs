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
    surface_biomes::SurfaceBiome,
    surface_camp_assembly,
    surface_entities::{self, AtlasLayout, ClimateSkin, Model, Species},
    surface_flora::{FloraPlacement, FloraState, HabitatCell, PlantCandidate, Support},
    surface_flora_vocabulary::ENTRIES,
    surface_fluid::FluidCell,
    surface_landmark_assembly, surface_ruin_assembly, surface_village_assembly,
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
        StonyShore | StonyPeaks | JaggedPeaks | FrozenPeaks | WindsweptGravellyHills => {
            ("minecraft:stone", "minecraft:stone")
        }
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
fn natural_entity_site(
    world: &SurfaceWorld,
    anchor: [i32; 3],
    species: Species,
    model: &Model,
) -> bool {
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
            "Village graph uses authored kit and terrain fit; native jigsaw/templates/processors and other structure families remain pending",
            "Seven surface landmark exteriors, nine ruin kits and eighteen camp presets use original geometry and sparse placement; native salts, templates and processors remain unverified",
            "Entity spawn light/habitat and selected-pack atlas compatibility remain pending",
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
                    world.fluids.insert(
                        [x, y, i32::from(level) - 1],
                        FluidCell::new(0, water_tint(biome))?,
                    );
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
    for gy in landmark_min[1]..=landmark_max[1] {
        for gx in landmark_min[0]..=landmark_max[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            let Some(landmark) =
                surface_landmark_assembly::candidate([gx, gy], &fields, &settings, &cancelled)?
            else {
                continue;
            };
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
                world.entities.push(SurfaceEntity {species,anchor:position,
                    model:surface_entities::model(species,AtlasLayout::Bedrock,climate),
                    atlas_status:"Original landmark marker; selected-pack entity atlas compatibility unverified"});
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
    // Evaluate whole trees by global grid anchor, then project. This keeps the
    // same configuration and owner when a camera/region boundary moves.
    let anchor_min = region.minimum.map(|v| (v - 48).div_euclid(13));
    let anchor_max = region.maximum.map(|v| (v + 48).div_euclid(13));
    for gy in anchor_min[1]..=anchor_max[1] {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        for gx in anchor_min[0]..=anchor_max[0] {
            let x = gx * 13 + 6;
            let y = gy * 13 + 6;
            let hash = hash2(seed ^ 0x7472_6565, i64::from(gx), i64::from(gy));
            if hash % 100 >= (settings.vegetation_percent.min(100) as u64).saturating_mul(18) / 100
            {
                continue;
            }
            let (sample, biome) = sample_ground(&fields, &settings, x, y);
            let configs = biome.descriptor().natural_tree_configuration_ids;
            if configs.is_empty()
                || sample.water_level.is_some_and(|z| z > sample.height)
                    && biome != SurfaceBiome::MangroveSwamp
            {
                continue;
            }
            let source = configs[(hash.rotate_left(11) as usize) % configs.len()];
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
                    cell.properties
                        .into_iter()
                        .map(|(k, v)| (k.to_owned(), v.to_owned())),
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
                .filter(|entry| !entry.attachment && entry.generation_biomes.contains(&biome.id()))
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
                    || world
                        .blocks
                        .get(&position)
                        .is_some_and(|b| !matches!(b.owner, SourceOwner::Terrain { .. }))
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
            if !natural_entity_site(&world, anchor, species, &model) {
                continue;
            }
            if world.entities.iter().any(|entity| entity.anchor == anchor) {
                continue;
            }
            world.entities.push(SurfaceEntity {species,anchor,model,
                atlas_status:"Authored habitat/daylight proxy; Bedrock-reference UV; selected/fallback atlas checked during binding"});
        }
    }
    Ok(world)
}

#[cfg(test)]
mod tests {
    use super::*;
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
