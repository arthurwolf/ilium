//! Canonical non-ocean surface Overworld vocabulary, pinned to Java 26.3.
//!
//! Factual descriptive tuples come from shipped Java data and official Bedrock
//! 1.26.50.4 samples (the generation baseline for the 26.52 hotfix target).
//! Research provenance: `biome-research/sources.json` and `source-manifest.json`.
//! No runtime configuration, structure template, artwork or texture is embedded.
//!
//! Gameplay temperature/downfall describe weather and gameplay. They are NOT
//! multi-noise selector bands, coordinates, terrain heights or generation weights.
//! RGB values describe biome water surface color, not an opacity/blending policy.
//! Plant/block IDs are possible provider states or type-driven default blocks;
//! their presence does not remove placement, probability or substrate constraints.
//!
//! Bedrock has retained surface/hills/edge/mutated definitions beyond these43
//! canonical IDs. Their existence does not establish current normal generation.
//! The research appendix keeps those68 surface-or-legacy definitions separate.
//! Exact selector bands, Bedrock legacy placement, runtime ecology and final visual
//! acceptance remain separate gates. Surface azalea indicators above lush caves
//! are supplemental: excluding cave interiors must not erase their surface tree.

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum SurfaceBiome {
    Badlands,
    BambooJungle,
    Beach,
    BirchForest,
    CherryGrove,
    DappledForest,
    DarkForest,
    Desert,
    ErodedBadlands,
    FlowerForest,
    Forest,
    FrozenPeaks,
    FrozenRiver,
    Grove,
    IceSpikes,
    JaggedPeaks,
    Jungle,
    MangroveSwamp,
    Meadow,
    MushroomFields,
    OldGrowthBirchForest,
    OldGrowthPineTaiga,
    OldGrowthSpruceTaiga,
    PaleGarden,
    Plains,
    River,
    Savanna,
    SavannaPlateau,
    SnowyBeach,
    SnowyPlains,
    SnowySlopes,
    SnowyTaiga,
    SparseJungle,
    StonyPeaks,
    StonyShore,
    SunflowerPlains,
    Swamp,
    Taiga,
    WindsweptForest,
    WindsweptGravellyHills,
    WindsweptHills,
    WindsweptSavanna,
    WoodedBadlands,
}

/// Supplemental organisms are not ordinary biome creature/monster table entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OrganismSource {
    BeeNest,
    CreakingHeart,
    FixedStructureEntity,
    ConditionalStructureSpawn,
    SurfaceEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupplementalOrganismReference {
    pub organism_id: &'static str,
    pub source: OrganismSource,
    /// Eligibility still requires the named nest/heart/structure/event condition.
    pub condition: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceBiomeDescriptor {
    pub biome: SurfaceBiome,
    /// Canonical Java identifier, including `minecraft:`.
    pub id: &'static str,
    pub display_name: &'static str,
    /// Weather/gameplay value, not a climate selector coordinate.
    pub gameplay_temperature: f32,
    pub downfall: f32,
    pub has_precipitation: bool,
    pub java_surface_water_rgb: [u8; 3],
    pub bedrock_surface_water_rgb: [u8; 3],
    /// Official Bedrock identifier may use an older canonical spelling.
    pub bedrock_id: &'static str,
    /// Natural configuration IDs, not species. Includes fallen and giant-fungus forms.
    pub natural_tree_configuration_ids: &'static [&'static str],
    /// Possible surface feature block states, including type-driven default plants.
    /// Saplings/provider alternatives are descriptive vocabulary, not promised placements.
    pub surface_block_ids: &'static [&'static str],
    /// Exact creature/monster-table species; valid light/height/substrate rules still apply.
    /// A generic slime entry does not imply surface slimes outside their valid habitat.
    pub biome_table_fauna_ids: &'static [&'static str],
    /// Water-animal table species retained separately; underwater feature geometry is out of scope.
    pub water_biome_table_fauna_ids: &'static [&'static str],
    /// Surface variants only; igloo basements, buried portals and trail-ruin interiors excluded.
    pub surface_structure_variant_ids: &'static [&'static str],
    pub supplemental_organisms: &'static [SupplementalOrganismReference],
}

pub const NATURAL_WOOD_SPECIES: [&str; 10] = [
    "oak", "birch", "spruce", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "pale_oak",
    "poplar",
];

/// Pine forms use spruce wood; azalea uses oak wood and distinct azalea foliage.
pub const SURFACE_AZALEA_INDICATOR_CONFIGURATION_ID: &str = "minecraft:azalea_tree";

pub const ALL_SURFACE_BIOMES: [SurfaceBiome; 43] = [
    SurfaceBiome::Badlands,
    SurfaceBiome::BambooJungle,
    SurfaceBiome::Beach,
    SurfaceBiome::BirchForest,
    SurfaceBiome::CherryGrove,
    SurfaceBiome::DappledForest,
    SurfaceBiome::DarkForest,
    SurfaceBiome::Desert,
    SurfaceBiome::ErodedBadlands,
    SurfaceBiome::FlowerForest,
    SurfaceBiome::Forest,
    SurfaceBiome::FrozenPeaks,
    SurfaceBiome::FrozenRiver,
    SurfaceBiome::Grove,
    SurfaceBiome::IceSpikes,
    SurfaceBiome::JaggedPeaks,
    SurfaceBiome::Jungle,
    SurfaceBiome::MangroveSwamp,
    SurfaceBiome::Meadow,
    SurfaceBiome::MushroomFields,
    SurfaceBiome::OldGrowthBirchForest,
    SurfaceBiome::OldGrowthPineTaiga,
    SurfaceBiome::OldGrowthSpruceTaiga,
    SurfaceBiome::PaleGarden,
    SurfaceBiome::Plains,
    SurfaceBiome::River,
    SurfaceBiome::Savanna,
    SurfaceBiome::SavannaPlateau,
    SurfaceBiome::SnowyBeach,
    SurfaceBiome::SnowyPlains,
    SurfaceBiome::SnowySlopes,
    SurfaceBiome::SnowyTaiga,
    SurfaceBiome::SparseJungle,
    SurfaceBiome::StonyPeaks,
    SurfaceBiome::StonyShore,
    SurfaceBiome::SunflowerPlains,
    SurfaceBiome::Swamp,
    SurfaceBiome::Taiga,
    SurfaceBiome::WindsweptForest,
    SurfaceBiome::WindsweptGravellyHills,
    SurfaceBiome::WindsweptHills,
    SurfaceBiome::WindsweptSavanna,
    SurfaceBiome::WoodedBadlands,
];

impl SurfaceBiome {
    pub const fn all() -> &'static [Self] {
        &ALL_SURFACE_BIOMES
    }

    pub const fn descriptor(self) -> &'static SurfaceBiomeDescriptor {
        &SURFACE_BIOME_DESCRIPTORS[self as usize]
    }

    pub const fn id(self) -> &'static str {
        self.descriptor().id
    }

    /// Accepts a canonical bare path or the corresponding `minecraft:` identifier.
    /// Bedrock legacy aliases and other namespaces are deliberately not coerced.
    pub fn from_id(id: &str) -> Option<Self> {
        match id.strip_prefix("minecraft:").unwrap_or(id) {
            "badlands" => Some(Self::Badlands),
            "bamboo_jungle" => Some(Self::BambooJungle),
            "beach" => Some(Self::Beach),
            "birch_forest" => Some(Self::BirchForest),
            "cherry_grove" => Some(Self::CherryGrove),
            "dappled_forest" => Some(Self::DappledForest),
            "dark_forest" => Some(Self::DarkForest),
            "desert" => Some(Self::Desert),
            "eroded_badlands" => Some(Self::ErodedBadlands),
            "flower_forest" => Some(Self::FlowerForest),
            "forest" => Some(Self::Forest),
            "frozen_peaks" => Some(Self::FrozenPeaks),
            "frozen_river" => Some(Self::FrozenRiver),
            "grove" => Some(Self::Grove),
            "ice_spikes" => Some(Self::IceSpikes),
            "jagged_peaks" => Some(Self::JaggedPeaks),
            "jungle" => Some(Self::Jungle),
            "mangrove_swamp" => Some(Self::MangroveSwamp),
            "meadow" => Some(Self::Meadow),
            "mushroom_fields" => Some(Self::MushroomFields),
            "old_growth_birch_forest" => Some(Self::OldGrowthBirchForest),
            "old_growth_pine_taiga" => Some(Self::OldGrowthPineTaiga),
            "old_growth_spruce_taiga" => Some(Self::OldGrowthSpruceTaiga),
            "pale_garden" => Some(Self::PaleGarden),
            "plains" => Some(Self::Plains),
            "river" => Some(Self::River),
            "savanna" => Some(Self::Savanna),
            "savanna_plateau" => Some(Self::SavannaPlateau),
            "snowy_beach" => Some(Self::SnowyBeach),
            "snowy_plains" => Some(Self::SnowyPlains),
            "snowy_slopes" => Some(Self::SnowySlopes),
            "snowy_taiga" => Some(Self::SnowyTaiga),
            "sparse_jungle" => Some(Self::SparseJungle),
            "stony_peaks" => Some(Self::StonyPeaks),
            "stony_shore" => Some(Self::StonyShore),
            "sunflower_plains" => Some(Self::SunflowerPlains),
            "swamp" => Some(Self::Swamp),
            "taiga" => Some(Self::Taiga),
            "windswept_forest" => Some(Self::WindsweptForest),
            "windswept_gravelly_hills" => Some(Self::WindsweptGravellyHills),
            "windswept_hills" => Some(Self::WindsweptHills),
            "windswept_savanna" => Some(Self::WindsweptSavanna),
            "wooded_badlands" => Some(Self::WoodedBadlands),
            _ => None,
        }
    }
}

/// Canonical table order equals the enum discriminant order.
pub static SURFACE_BIOME_DESCRIPTORS: [SurfaceBiomeDescriptor; 43] = [
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::Badlands,
        id: "minecraft:badlands",
        display_name: "Badlands",
        gameplay_temperature: 2.0,
        downfall: 0.0,
        has_precipitation: false,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [78, 127, 129],
        bedrock_id: "minecraft:mesa",
        natural_tree_configuration_ids: &[],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:cactus", "minecraft:cactus_flower", "minecraft:dead_bush", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_dry_grass", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:tall_dry_grass", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:armadillo", "minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::BambooJungle,
        id: "minecraft:bamboo_jungle",
        display_name: "Bamboo Jungle",
        gameplay_temperature: 0.95,
        downfall: 0.9,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [20, 162, 197],
        bedrock_id: "minecraft:bamboo_jungle",
        natural_tree_configuration_ids: &["minecraft:fancy_oak", "minecraft:jungle_bush", "minecraft:mega_jungle_tree"],
        surface_block_ids: &["minecraft:bamboo", "minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:dirt", "minecraft:fern", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:jungle_leaves", "minecraft:jungle_log", "minecraft:jungle_sapling", "minecraft:lava", "minecraft:melon", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:ocelot", "minecraft:panda", "minecraft:parrot", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_bamboo_jungle", "minecraft:jungle_pyramid", "minecraft:ruined_portal_jungle"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_bamboo_jungle template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::Beach,
        id: "minecraft:beach",
        display_name: "Beach",
        gameplay_temperature: 0.8,
        downfall: 0.4,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [21, 124, 171],
        bedrock_id: "minecraft:beach",
        natural_tree_configuration_ids: &[],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:turtle", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal", "minecraft:shipwreck_beached"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::BirchForest,
        id: "minecraft:birch_forest",
        display_name: "Birch Forest",
        gameplay_temperature: 0.6,
        downfall: 0.6,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [6, 119, 206],
        bedrock_id: "minecraft:birch_forest",
        natural_tree_configuration_ids: &["minecraft:birch_bees_0002", "minecraft:fallen_birch_tree"],
        surface_block_ids: &["minecraft:bee_nest", "minecraft:birch_leaves", "minecraft:birch_log", "minecraft:birch_sapling", "minecraft:brown_mushroom", "minecraft:bush", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:lilac", "minecraft:lily_of_the_valley", "minecraft:peony", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:rose_bush", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:water", "minecraft:wildflowers"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_birch_forest", "minecraft:ruined_portal"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:bee", source: OrganismSource::BeeNest, condition: "Tree bee-nest decorator; generated nest and occupancy conditions." },
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_birch_forest template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::CherryGrove,
        id: "minecraft:cherry_grove",
        display_name: "Cherry Grove",
        gameplay_temperature: 0.5,
        downfall: 0.8,
        has_precipitation: true,
        java_surface_water_rgb: [93, 183, 239],
        bedrock_surface_water_rgb: [93, 183, 239],
        bedrock_id: "minecraft:cherry_grove",
        natural_tree_configuration_ids: &["minecraft:cherry_bees_005"],
        surface_block_ids: &["minecraft:bee_nest", "minecraft:cherry_leaves", "minecraft:cherry_log", "minecraft:cherry_sapling", "minecraft:dirt", "minecraft:glow_lichen", "minecraft:lava", "minecraft:pink_petals", "minecraft:short_grass", "minecraft:stone", "minecraft:tall_grass", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:pig", "minecraft:rabbit", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_cherry_grove", "minecraft:pillager_outpost", "minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:bee", source: OrganismSource::BeeNest, condition: "Tree bee-nest decorator; generated nest and occupancy conditions." },
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_cherry_grove template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::DappledForest,
        id: "minecraft:dappled_forest",
        display_name: "Dappled Forest",
        gameplay_temperature: 0.6,
        downfall: 0.6,
        has_precipitation: true,
        java_surface_water_rgb: [55, 81, 84],
        bedrock_surface_water_rgb: [55, 81, 84],
        bedrock_id: "minecraft:dappled_forest",
        natural_tree_configuration_ids: &["minecraft:fallen_poplar_tree", "minecraft:orange_poplar_leaf_litter", "minecraft:red_poplar_leaf_litter", "minecraft:spruce", "minecraft:yellow_poplar_leaf_litter"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:dirt", "minecraft:glow_lichen", "minecraft:lava", "minecraft:leaf_litter", "minecraft:orange_poplar_leaves", "minecraft:poplar_log", "minecraft:poplar_sapling", "minecraft:red_poplar_leaves", "minecraft:red_shrub", "minecraft:shelf_mushroom", "minecraft:short_grass", "minecraft:spruce_leaves", "minecraft:spruce_log", "minecraft:spruce_sapling", "minecraft:stone", "minecraft:water", "minecraft:yellow_poplar_leaves"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:fox", "minecraft:pig", "minecraft:rabbit", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_dappled_forest", "minecraft:ruined_portal"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_dappled_forest template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::DarkForest,
        id: "minecraft:dark_forest",
        display_name: "Dark Forest",
        gameplay_temperature: 0.7,
        downfall: 0.8,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [59, 108, 209],
        bedrock_id: "minecraft:roofed_forest",
        natural_tree_configuration_ids: &["minecraft:birch_leaf_litter", "minecraft:dark_oak_leaf_litter", "minecraft:fallen_birch_tree", "minecraft:fallen_oak_tree", "minecraft:fancy_oak_leaf_litter", "minecraft:huge_brown_mushroom", "minecraft:huge_red_mushroom", "minecraft:oak_leaf_litter"],
        surface_block_ids: &["minecraft:birch_leaves", "minecraft:birch_log", "minecraft:birch_sapling", "minecraft:brown_mushroom", "minecraft:brown_mushroom_block", "minecraft:dandelion", "minecraft:dark_oak_leaves", "minecraft:dark_oak_log", "minecraft:dark_oak_sapling", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:leaf_litter", "minecraft:lilac", "minecraft:lily_of_the_valley", "minecraft:mushroom_stem", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:peony", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:red_mushroom_block", "minecraft:rose_bush", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:mansion", "minecraft:ruined_portal"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::Desert,
        id: "minecraft:desert",
        display_name: "Desert",
        gameplay_temperature: 2.0,
        downfall: 0.0,
        has_precipitation: false,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [50, 165, 152],
        bedrock_id: "minecraft:desert",
        natural_tree_configuration_ids: &[],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:cactus", "minecraft:cactus_flower", "minecraft:dandelion", "minecraft:dead_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_dry_grass", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:suspicious_sand", "minecraft:tall_dry_grass", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:camel", "minecraft:creeper", "minecraft:enderman", "minecraft:husk", "minecraft:parched", "minecraft:rabbit", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:desert_pyramid", "minecraft:desert_well", "minecraft:pillager_outpost", "minecraft:ruined_portal_desert", "minecraft:village_desert"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:camel", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_desert template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cat", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_desert template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cow", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_desert template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:horse", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_desert template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_desert template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pig", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_desert template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
            SupplementalOrganismReference { organism_id: "minecraft:sheep", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_desert template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_desert template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:zombie_villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_desert template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::ErodedBadlands,
        id: "minecraft:eroded_badlands",
        display_name: "Eroded Badlands",
        gameplay_temperature: 2.0,
        downfall: 0.0,
        has_precipitation: false,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [73, 127, 153],
        bedrock_id: "minecraft:mesa_bryce",
        natural_tree_configuration_ids: &[],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:cactus", "minecraft:cactus_flower", "minecraft:dead_bush", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_dry_grass", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:tall_dry_grass", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:armadillo", "minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::FlowerForest,
        id: "minecraft:flower_forest",
        display_name: "Flower Forest",
        gameplay_temperature: 0.7,
        downfall: 0.8,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [32, 163, 204],
        bedrock_id: "minecraft:flower_forest",
        natural_tree_configuration_ids: &["minecraft:birch_bees_002", "minecraft:fallen_birch_tree", "minecraft:fancy_oak_bees_002", "minecraft:oak_bees_002"],
        surface_block_ids: &["minecraft:allium", "minecraft:azure_bluet", "minecraft:bee_nest", "minecraft:birch_leaves", "minecraft:birch_log", "minecraft:birch_sapling", "minecraft:brown_mushroom", "minecraft:cornflower", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:lilac", "minecraft:lily_of_the_valley", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:orange_tulip", "minecraft:oxeye_daisy", "minecraft:peony", "minecraft:pink_tulip", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:red_tulip", "minecraft:rose_bush", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:water", "minecraft:white_tulip"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:pig", "minecraft:rabbit", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_flower_forest", "minecraft:ruined_portal"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:bee", source: OrganismSource::BeeNest, condition: "Tree bee-nest decorator; generated nest and occupancy conditions." },
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_flower_forest template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::Forest,
        id: "minecraft:forest",
        display_name: "Forest",
        gameplay_temperature: 0.7,
        downfall: 0.8,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [30, 151, 242],
        bedrock_id: "minecraft:forest",
        natural_tree_configuration_ids: &["minecraft:birch_bees_0002_leaf_litter", "minecraft:fallen_birch_tree", "minecraft:fallen_oak_tree", "minecraft:fancy_oak_bees_0002_leaf_litter", "minecraft:oak_bees_0002_leaf_litter"],
        surface_block_ids: &["minecraft:bee_nest", "minecraft:birch_leaves", "minecraft:birch_log", "minecraft:birch_sapling", "minecraft:brown_mushroom", "minecraft:bush", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:leaf_litter", "minecraft:lilac", "minecraft:lily_of_the_valley", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:peony", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:rose_bush", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:wolf", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_forest", "minecraft:ruined_portal"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:bee", source: OrganismSource::BeeNest, condition: "Tree bee-nest decorator; generated nest and occupancy conditions." },
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_forest template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::FrozenPeaks,
        id: "minecraft:frozen_peaks",
        display_name: "Frozen Peaks",
        gameplay_temperature: -0.7,
        downfall: 0.9,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [68, 175, 245],
        bedrock_id: "minecraft:frozen_peaks",
        natural_tree_configuration_ids: &[],
        surface_block_ids: &["minecraft:glow_lichen", "minecraft:lava", "minecraft:stone", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:goat", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:pillager_outpost", "minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::FrozenRiver,
        id: "minecraft:frozen_river",
        display_name: "Frozen River",
        gameplay_temperature: 0.0,
        downfall: 0.5,
        has_precipitation: true,
        java_surface_water_rgb: [57, 56, 201],
        bedrock_surface_water_rgb: [24, 83, 144],
        bedrock_id: "minecraft:frozen_river",
        natural_tree_configuration_ids: &["minecraft:fancy_oak", "minecraft:oak"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:bush", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:drowned", "minecraft:enderman", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &["minecraft:salmon", "minecraft:squid"],
        surface_structure_variant_ids: &["minecraft:ruined_portal"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::Grove,
        id: "minecraft:grove",
        display_name: "Grove",
        gameplay_temperature: -0.2,
        downfall: 0.8,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [68, 175, 245],
        bedrock_id: "minecraft:grove",
        natural_tree_configuration_ids: &["minecraft:pine", "minecraft:spruce"],
        surface_block_ids: &["minecraft:dirt", "minecraft:glow_lichen", "minecraft:lava", "minecraft:pumpkin", "minecraft:spruce_leaves", "minecraft:spruce_log", "minecraft:spruce_sapling", "minecraft:stone", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:fox", "minecraft:rabbit", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:wolf", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:pillager_outpost", "minecraft:ruined_portal"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::IceSpikes,
        id: "minecraft:ice_spikes",
        display_name: "Ice Spikes",
        gameplay_temperature: 0.0,
        downfall: 0.5,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [20, 85, 155],
        bedrock_id: "minecraft:ice_plains_spikes",
        natural_tree_configuration_ids: &["minecraft:fallen_spruce_tree", "minecraft:spruce"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:packed_ice", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:spruce_leaves", "minecraft:spruce_log", "minecraft:spruce_sapling", "minecraft:stone", "minecraft:sugar_cane", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:polar_bear", "minecraft:rabbit", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:stray", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::JaggedPeaks,
        id: "minecraft:jagged_peaks",
        display_name: "Jagged Peaks",
        gameplay_temperature: -0.7,
        downfall: 0.9,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [68, 175, 245],
        bedrock_id: "minecraft:jagged_peaks",
        natural_tree_configuration_ids: &[],
        surface_block_ids: &["minecraft:glow_lichen", "minecraft:lava", "minecraft:stone", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:goat", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:pillager_outpost", "minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::Jungle,
        id: "minecraft:jungle",
        display_name: "Jungle",
        gameplay_temperature: 0.95,
        downfall: 0.9,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [20, 162, 197],
        bedrock_id: "minecraft:jungle",
        natural_tree_configuration_ids: &["minecraft:fallen_jungle_tree", "minecraft:fancy_oak", "minecraft:jungle_bush", "minecraft:jungle_tree", "minecraft:mega_jungle_tree"],
        surface_block_ids: &["minecraft:bamboo", "minecraft:brown_mushroom", "minecraft:cocoa", "minecraft:dandelion", "minecraft:dirt", "minecraft:fern", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:jungle_leaves", "minecraft:jungle_log", "minecraft:jungle_sapling", "minecraft:lava", "minecraft:melon", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:ocelot", "minecraft:panda", "minecraft:parrot", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:jungle_pyramid", "minecraft:ruined_portal_jungle", "minecraft:trail_ruins"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::MangroveSwamp,
        id: "minecraft:mangrove_swamp",
        display_name: "Mangrove Swamp",
        gameplay_temperature: 0.8,
        downfall: 0.9,
        has_precipitation: true,
        java_surface_water_rgb: [58, 122, 106],
        bedrock_surface_water_rgb: [58, 122, 106],
        bedrock_id: "minecraft:mangrove_swamp",
        natural_tree_configuration_ids: &["minecraft:mangrove", "minecraft:tall_mangrove"],
        surface_block_ids: &["minecraft:bee_nest", "minecraft:dead_bush", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:lily_pad", "minecraft:mangrove_leaves", "minecraft:mangrove_log", "minecraft:mangrove_propagule", "minecraft:mangrove_roots", "minecraft:moss_carpet", "minecraft:muddy_mangrove_roots", "minecraft:seagrass", "minecraft:short_grass", "minecraft:stone", "minecraft:tall_seagrass", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:bogged", "minecraft:creeper", "minecraft:enderman", "minecraft:frog", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &["minecraft:tropical_fish"],
        surface_structure_variant_ids: &["minecraft:ruined_portal_swamp"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:bee", source: OrganismSource::BeeNest, condition: "Tree bee-nest decorator; generated nest and occupancy conditions." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::Meadow,
        id: "minecraft:meadow",
        display_name: "Meadow",
        gameplay_temperature: 0.5,
        downfall: 0.8,
        has_precipitation: true,
        java_surface_water_rgb: [14, 78, 207],
        bedrock_surface_water_rgb: [68, 175, 245],
        bedrock_id: "minecraft:meadow",
        natural_tree_configuration_ids: &["minecraft:fancy_oak_bees", "minecraft:super_birch_bees"],
        surface_block_ids: &["minecraft:allium", "minecraft:azure_bluet", "minecraft:bee_nest", "minecraft:birch_leaves", "minecraft:birch_log", "minecraft:birch_sapling", "minecraft:cornflower", "minecraft:dandelion", "minecraft:dirt", "minecraft:glow_lichen", "minecraft:lava", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:oxeye_daisy", "minecraft:poppy", "minecraft:short_grass", "minecraft:stone", "minecraft:tall_grass", "minecraft:water", "minecraft:wildflowers"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:donkey", "minecraft:enderman", "minecraft:rabbit", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_meadow", "minecraft:pillager_outpost", "minecraft:ruined_portal_mountain", "minecraft:village_plains"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:bee", source: OrganismSource::BeeNest, condition: "Tree bee-nest decorator; generated nest and occupancy conditions." },
            SupplementalOrganismReference { organism_id: "minecraft:cat", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cow", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_meadow template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:horse", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pig", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
            SupplementalOrganismReference { organism_id: "minecraft:sheep", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:zombie_villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::MushroomFields,
        id: "minecraft:mushroom_fields",
        display_name: "Mushroom Fields",
        gameplay_temperature: 0.9,
        downfall: 1.0,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [138, 137, 151],
        bedrock_id: "minecraft:mushroom_island",
        natural_tree_configuration_ids: &["minecraft:huge_brown_mushroom", "minecraft:huge_red_mushroom"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:brown_mushroom_block", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:mushroom_stem", "minecraft:red_mushroom", "minecraft:red_mushroom_block", "minecraft:stone", "minecraft:sugar_cane", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:mooshroom"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::OldGrowthBirchForest,
        id: "minecraft:old_growth_birch_forest",
        display_name: "Old Growth Birch Forest",
        gameplay_temperature: 0.6,
        downfall: 0.6,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [68, 175, 245],
        bedrock_id: "minecraft:birch_forest_mutated",
        natural_tree_configuration_ids: &["minecraft:birch_bees_0002", "minecraft:fallen_birch_tree", "minecraft:fallen_super_birch_tree", "minecraft:super_birch_bees_0002"],
        surface_block_ids: &["minecraft:bee_nest", "minecraft:birch_leaves", "minecraft:birch_log", "minecraft:birch_sapling", "minecraft:brown_mushroom", "minecraft:bush", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:lilac", "minecraft:lily_of_the_valley", "minecraft:peony", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:rose_bush", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:water", "minecraft:wildflowers"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_old_growth_birch_forest", "minecraft:ruined_portal", "minecraft:trail_ruins"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:bee", source: OrganismSource::BeeNest, condition: "Tree bee-nest decorator; generated nest and occupancy conditions." },
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_old_growth_birch_forest template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::OldGrowthPineTaiga,
        id: "minecraft:old_growth_pine_taiga",
        display_name: "Old Growth Pine Taiga",
        gameplay_temperature: 0.3,
        downfall: 0.8,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [45, 109, 119],
        bedrock_id: "minecraft:mega_taiga",
        natural_tree_configuration_ids: &["minecraft:fallen_spruce_tree", "minecraft:mega_pine", "minecraft:mega_spruce", "minecraft:pine", "minecraft:spruce"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:dead_bush", "minecraft:dirt", "minecraft:fern", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:large_fern", "minecraft:lava", "minecraft:podzol", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:spruce_leaves", "minecraft:spruce_log", "minecraft:spruce_sapling", "minecraft:stone", "minecraft:sugar_cane", "minecraft:sweet_berry_bush", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:fox", "minecraft:pig", "minecraft:rabbit", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:wolf", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_old_growth_pine_taiga", "minecraft:ruined_portal", "minecraft:trail_ruins"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_old_growth_pine_taiga template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::OldGrowthSpruceTaiga,
        id: "minecraft:old_growth_spruce_taiga",
        display_name: "Old Growth Spruce Taiga",
        gameplay_temperature: 0.25,
        downfall: 0.8,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [68, 175, 245],
        bedrock_id: "minecraft:redwood_taiga_mutated",
        natural_tree_configuration_ids: &["minecraft:fallen_spruce_tree", "minecraft:mega_spruce", "minecraft:pine", "minecraft:spruce"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:dead_bush", "minecraft:dirt", "minecraft:fern", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:large_fern", "minecraft:lava", "minecraft:podzol", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:spruce_leaves", "minecraft:spruce_log", "minecraft:spruce_sapling", "minecraft:stone", "minecraft:sugar_cane", "minecraft:sweet_berry_bush", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:fox", "minecraft:pig", "minecraft:rabbit", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:wolf", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_old_growth_spruce_taiga", "minecraft:ruined_portal", "minecraft:trail_ruins"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_old_growth_spruce_taiga template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::PaleGarden,
        id: "minecraft:pale_garden",
        display_name: "Pale Garden",
        gameplay_temperature: 0.7,
        downfall: 0.8,
        has_precipitation: true,
        java_surface_water_rgb: [118, 136, 157],
        bedrock_surface_water_rgb: [118, 136, 157],
        bedrock_id: "minecraft:pale_garden",
        natural_tree_configuration_ids: &["minecraft:pale_oak", "minecraft:pale_oak_creaking"],
        surface_block_ids: &["minecraft:closed_eyeblossom", "minecraft:creaking_heart", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:pale_hanging_moss", "minecraft:pale_moss_block", "minecraft:pale_moss_carpet", "minecraft:pale_oak_leaves", "minecraft:pale_oak_log", "minecraft:pale_oak_sapling", "minecraft:pumpkin", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:tall_grass", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_pale_garden", "minecraft:mansion", "minecraft:ruined_portal"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:creaking", source: OrganismSource::CreakingHeart, condition: "Active naturally generated creaking heart; not a normal creature-table spawn." },
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_pale_garden template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::Plains,
        id: "minecraft:plains",
        display_name: "Plains",
        gameplay_temperature: 0.8,
        downfall: 0.4,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [68, 175, 245],
        bedrock_id: "minecraft:plains",
        natural_tree_configuration_ids: &["minecraft:fallen_oak_tree", "minecraft:fancy_oak_bees_005", "minecraft:oak_bees_005"],
        surface_block_ids: &["minecraft:bee_nest", "minecraft:brown_mushroom", "minecraft:bush", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:tall_grass", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:donkey", "minecraft:enderman", "minecraft:horse", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_horse", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:pillager_outpost", "minecraft:ruined_portal", "minecraft:village_plains"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:bee", source: OrganismSource::BeeNest, condition: "Tree bee-nest decorator; generated nest and occupancy conditions." },
            SupplementalOrganismReference { organism_id: "minecraft:cat", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cow", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:horse", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pig", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
            SupplementalOrganismReference { organism_id: "minecraft:sheep", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:zombie_villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_plains template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::River,
        id: "minecraft:river",
        display_name: "River",
        gameplay_temperature: 0.5,
        downfall: 0.5,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [0, 132, 255],
        bedrock_id: "minecraft:river",
        natural_tree_configuration_ids: &["minecraft:fancy_oak", "minecraft:oak"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:bush", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:seagrass", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:tall_seagrass", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:drowned", "minecraft:enderman", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &["minecraft:salmon", "minecraft:squid"],
        surface_structure_variant_ids: &["minecraft:ruined_portal"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::Savanna,
        id: "minecraft:savanna",
        display_name: "Savanna",
        gameplay_temperature: 2.0,
        downfall: 0.0,
        has_precipitation: false,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [44, 139, 156],
        bedrock_id: "minecraft:savanna",
        natural_tree_configuration_ids: &["minecraft:acacia", "minecraft:fallen_oak_tree", "minecraft:oak"],
        surface_block_ids: &["minecraft:acacia_leaves", "minecraft:acacia_log", "minecraft:acacia_sapling", "minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:tall_grass", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:armadillo", "minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:donkey", "minecraft:enderman", "minecraft:horse", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_horse", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_savanna", "minecraft:pillager_outpost", "minecraft:ruined_portal", "minecraft:village_savanna"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cat", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_savanna template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cow", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_savanna template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_savanna template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:horse", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_savanna template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_savanna template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pig", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_savanna template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
            SupplementalOrganismReference { organism_id: "minecraft:sheep", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_savanna template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_savanna template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:zombie_villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_savanna template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::SavannaPlateau,
        id: "minecraft:savanna_plateau",
        display_name: "Savanna Plateau",
        gameplay_temperature: 2.0,
        downfall: 0.0,
        has_precipitation: false,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [37, 144, 168],
        bedrock_id: "minecraft:savanna_plateau",
        natural_tree_configuration_ids: &["minecraft:acacia", "minecraft:fallen_oak_tree", "minecraft:oak"],
        surface_block_ids: &["minecraft:acacia_leaves", "minecraft:acacia_log", "minecraft:acacia_sapling", "minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:tall_grass", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:armadillo", "minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:donkey", "minecraft:enderman", "minecraft:horse", "minecraft:llama", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:wolf", "minecraft:zombie", "minecraft:zombie_horse", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::SnowyBeach,
        id: "minecraft:snowy_beach",
        display_name: "Snowy Beach",
        gameplay_temperature: 0.05,
        downfall: 0.3,
        has_precipitation: true,
        java_surface_water_rgb: [61, 87, 214],
        bedrock_surface_water_rgb: [20, 99, 165],
        bedrock_id: "minecraft:cold_beach",
        natural_tree_configuration_ids: &[],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal", "minecraft:shipwreck_beached"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::SnowyPlains,
        id: "minecraft:snowy_plains",
        display_name: "Snowy Plains",
        gameplay_temperature: 0.0,
        downfall: 0.5,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [20, 85, 155],
        bedrock_id: "minecraft:ice_plains",
        natural_tree_configuration_ids: &["minecraft:fallen_spruce_tree", "minecraft:spruce"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:spruce_leaves", "minecraft:spruce_log", "minecraft:spruce_sapling", "minecraft:stone", "minecraft:sugar_cane", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:polar_bear", "minecraft:rabbit", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:stray", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_horse", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:igloo", "minecraft:pillager_outpost", "minecraft:ruined_portal", "minecraft:village_snowy"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cat", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_snowy template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cow", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_snowy template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:horse", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_snowy template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_snowy template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pig", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_snowy template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
            SupplementalOrganismReference { organism_id: "minecraft:sheep", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_snowy template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_snowy template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:zombie_villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_snowy template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::SnowySlopes,
        id: "minecraft:snowy_slopes",
        display_name: "Snowy Slopes",
        gameplay_temperature: -0.3,
        downfall: 0.9,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [68, 175, 245],
        bedrock_id: "minecraft:snowy_slopes",
        natural_tree_configuration_ids: &[],
        surface_block_ids: &["minecraft:glow_lichen", "minecraft:lava", "minecraft:pumpkin", "minecraft:stone", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:goat", "minecraft:rabbit", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:igloo", "minecraft:pillager_outpost", "minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::SnowyTaiga,
        id: "minecraft:snowy_taiga",
        display_name: "Snowy Taiga",
        gameplay_temperature: -0.5,
        downfall: 0.4,
        has_precipitation: true,
        java_surface_water_rgb: [61, 87, 214],
        bedrock_surface_water_rgb: [32, 94, 131],
        bedrock_id: "minecraft:cold_taiga",
        natural_tree_configuration_ids: &["minecraft:fallen_spruce_tree", "minecraft:pine", "minecraft:spruce"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:dirt", "minecraft:fern", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:large_fern", "minecraft:lava", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:spruce_leaves", "minecraft:spruce_log", "minecraft:spruce_sapling", "minecraft:stone", "minecraft:sugar_cane", "minecraft:sweet_berry_bush", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:fox", "minecraft:pig", "minecraft:rabbit", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:wolf", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_snowy_taiga", "minecraft:igloo", "minecraft:ruined_portal", "minecraft:trail_ruins"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_snowy_taiga template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::SparseJungle,
        id: "minecraft:sparse_jungle",
        display_name: "Sparse Jungle",
        gameplay_temperature: 0.95,
        downfall: 0.8,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [13, 138, 227],
        bedrock_id: "minecraft:jungle_edge",
        natural_tree_configuration_ids: &["minecraft:fallen_jungle_tree", "minecraft:fancy_oak", "minecraft:jungle_bush", "minecraft:jungle_tree"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:cocoa", "minecraft:dandelion", "minecraft:dirt", "minecraft:fern", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:jungle_leaves", "minecraft:jungle_log", "minecraft:jungle_sapling", "minecraft:lava", "minecraft:melon", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:wolf", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_sparse_jungle", "minecraft:ruined_portal_jungle"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_sparse_jungle template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::StonyPeaks,
        id: "minecraft:stony_peaks",
        display_name: "Stony Peaks",
        gameplay_temperature: 1.0,
        downfall: 0.3,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [68, 175, 245],
        bedrock_id: "minecraft:stony_peaks",
        natural_tree_configuration_ids: &[],
        surface_block_ids: &["minecraft:glow_lichen", "minecraft:lava", "minecraft:stone", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:pillager_outpost", "minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::StonyShore,
        id: "minecraft:stony_shore",
        display_name: "Stony Shore",
        gameplay_temperature: 0.2,
        downfall: 0.3,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [13, 103, 187],
        bedrock_id: "minecraft:stone_beach",
        natural_tree_configuration_ids: &[],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:creeper", "minecraft:enderman", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::SunflowerPlains,
        id: "minecraft:sunflower_plains",
        display_name: "Sunflower Plains",
        gameplay_temperature: 0.8,
        downfall: 0.4,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [68, 175, 245],
        bedrock_id: "minecraft:sunflower_plains",
        natural_tree_configuration_ids: &["minecraft:fallen_oak_tree", "minecraft:fancy_oak_bees_005", "minecraft:oak_bees_005"],
        surface_block_ids: &["minecraft:bee_nest", "minecraft:brown_mushroom", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:sunflower", "minecraft:tall_grass", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:donkey", "minecraft:enderman", "minecraft:horse", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_horse", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:bee", source: OrganismSource::BeeNest, condition: "Tree bee-nest decorator; generated nest and occupancy conditions." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::Swamp,
        id: "minecraft:swamp",
        display_name: "Swamp",
        gameplay_temperature: 0.8,
        downfall: 0.9,
        has_precipitation: true,
        java_surface_water_rgb: [97, 123, 100],
        bedrock_surface_water_rgb: [97, 123, 100],
        bedrock_id: "minecraft:swampland",
        natural_tree_configuration_ids: &["minecraft:swamp_oak"],
        surface_block_ids: &["minecraft:blue_orchid", "minecraft:brown_mushroom", "minecraft:dead_bush", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:lily_pad", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:seagrass", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:tall_seagrass", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:bogged", "minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:frog", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_swamp", "minecraft:ruined_portal_swamp", "minecraft:swamp_hut"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:cat", source: OrganismSource::ConditionalStructureSpawn, condition: "swamp_hut bounding-box spawn override; runtime conditions required." },
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_swamp template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:witch", source: OrganismSource::ConditionalStructureSpawn, condition: "swamp_hut bounding-box spawn override; runtime conditions required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::Taiga,
        id: "minecraft:taiga",
        display_name: "Taiga",
        gameplay_temperature: 0.25,
        downfall: 0.8,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [40, 112, 130],
        bedrock_id: "minecraft:taiga",
        natural_tree_configuration_ids: &["minecraft:fallen_spruce_tree", "minecraft:pine", "minecraft:spruce"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:dirt", "minecraft:fern", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:large_fern", "minecraft:lava", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:spruce_leaves", "minecraft:spruce_log", "minecraft:spruce_sapling", "minecraft:stone", "minecraft:sugar_cane", "minecraft:sweet_berry_bush", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:fox", "minecraft:pig", "minecraft:rabbit", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:wolf", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_taiga", "minecraft:pillager_outpost", "minecraft:ruined_portal", "minecraft:trail_ruins", "minecraft:village_taiga"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:allay", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cat", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_taiga template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cow", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_taiga template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_taiga template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:horse", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_taiga template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible pillager_outpost template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:iron_golem", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_taiga template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pig", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_taiga template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::ConditionalStructureSpawn, condition: "pillager_outpost bounding-box spawn override; runtime conditions required." },
            SupplementalOrganismReference { organism_id: "minecraft:sheep", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_taiga template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_taiga template; actual template selection required." },
            SupplementalOrganismReference { organism_id: "minecraft:zombie_villager", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible village_taiga template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::WindsweptForest,
        id: "minecraft:windswept_forest",
        display_name: "Windswept Forest",
        gameplay_temperature: 0.2,
        downfall: 0.3,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [14, 99, 171],
        bedrock_id: "minecraft:extreme_hills_plus_trees",
        natural_tree_configuration_ids: &["minecraft:fallen_oak_tree", "minecraft:fallen_spruce_tree", "minecraft:fancy_oak", "minecraft:oak", "minecraft:spruce"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:bush", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:spruce_leaves", "minecraft:spruce_log", "minecraft:spruce_sapling", "minecraft:stone", "minecraft:sugar_cane", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:llama", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_windswept_forest", "minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_windswept_forest template; actual template selection required." },
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::WindsweptGravellyHills,
        id: "minecraft:windswept_gravelly_hills",
        display_name: "Windswept Gravelly Hills",
        gameplay_temperature: 0.2,
        downfall: 0.3,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [14, 99, 171],
        bedrock_id: "minecraft:extreme_hills_mutated",
        natural_tree_configuration_ids: &["minecraft:fallen_oak_tree", "minecraft:fallen_spruce_tree", "minecraft:fancy_oak", "minecraft:oak", "minecraft:spruce"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:bush", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:spruce_leaves", "minecraft:spruce_log", "minecraft:spruce_sapling", "minecraft:stone", "minecraft:sugar_cane", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:llama", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::WindsweptHills,
        id: "minecraft:windswept_hills",
        display_name: "Windswept Hills",
        gameplay_temperature: 0.2,
        downfall: 0.3,
        has_precipitation: true,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [0, 123, 247],
        bedrock_id: "minecraft:extreme_hills",
        natural_tree_configuration_ids: &["minecraft:fallen_oak_tree", "minecraft:fallen_spruce_tree", "minecraft:fancy_oak", "minecraft:oak", "minecraft:spruce"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:bush", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:spruce_leaves", "minecraft:spruce_log", "minecraft:spruce_sapling", "minecraft:stone", "minecraft:sugar_cane", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:llama", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::WindsweptSavanna,
        id: "minecraft:windswept_savanna",
        display_name: "Windswept Savanna",
        gameplay_temperature: 2.0,
        downfall: 0.0,
        has_precipitation: false,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [37, 144, 168],
        bedrock_id: "minecraft:savanna_mutated",
        natural_tree_configuration_ids: &["minecraft:acacia", "minecraft:fallen_oak_tree", "minecraft:oak"],
        surface_block_ids: &["minecraft:acacia_leaves", "minecraft:acacia_log", "minecraft:acacia_sapling", "minecraft:brown_mushroom", "minecraft:dandelion", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:poppy", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:armadillo", "minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:donkey", "minecraft:enderman", "minecraft:horse", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:zombie", "minecraft:zombie_horse", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
        ],
    },
    SurfaceBiomeDescriptor {
        biome: SurfaceBiome::WoodedBadlands,
        id: "minecraft:wooded_badlands",
        display_name: "Wooded Badlands",
        gameplay_temperature: 2.0,
        downfall: 0.0,
        has_precipitation: false,
        java_surface_water_rgb: [63, 118, 228],
        bedrock_surface_water_rgb: [85, 128, 158],
        bedrock_id: "minecraft:mesa_plateau_stone",
        natural_tree_configuration_ids: &["minecraft:fallen_oak_tree", "minecraft:oak_leaf_litter"],
        surface_block_ids: &["minecraft:brown_mushroom", "minecraft:cactus", "minecraft:cactus_flower", "minecraft:dead_bush", "minecraft:dirt", "minecraft:firefly_bush", "minecraft:glow_lichen", "minecraft:lava", "minecraft:leaf_litter", "minecraft:oak_leaves", "minecraft:oak_log", "minecraft:oak_sapling", "minecraft:pumpkin", "minecraft:red_mushroom", "minecraft:short_dry_grass", "minecraft:short_grass", "minecraft:stone", "minecraft:sugar_cane", "minecraft:tall_dry_grass", "minecraft:vine", "minecraft:water"],
        biome_table_fauna_ids: &["minecraft:armadillo", "minecraft:chicken", "minecraft:cow", "minecraft:creeper", "minecraft:enderman", "minecraft:pig", "minecraft:sheep", "minecraft:skeleton", "minecraft:slime", "minecraft:spider", "minecraft:witch", "minecraft:wolf", "minecraft:zombie", "minecraft:zombie_villager"],
        water_biome_table_fauna_ids: &[],
        surface_structure_variant_ids: &["minecraft:abandoned_camp_wooded_badlands", "minecraft:ruined_portal_mountain"],
        supplemental_organisms: &[
            SupplementalOrganismReference { organism_id: "minecraft:cushion", source: OrganismSource::FixedStructureEntity, condition: "Fixed entity in eligible abandoned_camp_wooded_badlands template; actual template selection required." },
        ],
    },
 ];

/// Conditional surface visitors/events are deliberately separate from biome rows.
/// These IDs are scope references, not a claim that every event occurs in every biome.
pub const SURFACE_EVENT_ORGANISMS: &[SupplementalOrganismReference] = &[
    SupplementalOrganismReference { organism_id: "minecraft:wandering_trader", source: OrganismSource::SurfaceEvent, condition: "Conditional trader, patrol, raid, trap or night event; exact eligibility requires the event adapter." },
    SupplementalOrganismReference { organism_id: "minecraft:trader_llama", source: OrganismSource::SurfaceEvent, condition: "Conditional trader, patrol, raid, trap or night event; exact eligibility requires the event adapter." },
    SupplementalOrganismReference { organism_id: "minecraft:phantom", source: OrganismSource::SurfaceEvent, condition: "Conditional trader, patrol, raid, trap or night event; exact eligibility requires the event adapter." },
    SupplementalOrganismReference { organism_id: "minecraft:skeleton_horse", source: OrganismSource::SurfaceEvent, condition: "Conditional trader, patrol, raid, trap or night event; exact eligibility requires the event adapter." },
    SupplementalOrganismReference { organism_id: "minecraft:pillager", source: OrganismSource::SurfaceEvent, condition: "Conditional trader, patrol, raid, trap or night event; exact eligibility requires the event adapter." },
    SupplementalOrganismReference { organism_id: "minecraft:ravager", source: OrganismSource::SurfaceEvent, condition: "Conditional trader, patrol, raid, trap or night event; exact eligibility requires the event adapter." },
    SupplementalOrganismReference { organism_id: "minecraft:vindicator", source: OrganismSource::SurfaceEvent, condition: "Conditional trader, patrol, raid, trap or night event; exact eligibility requires the event adapter." },
    SupplementalOrganismReference { organism_id: "minecraft:evoker", source: OrganismSource::SurfaceEvent, condition: "Conditional trader, patrol, raid, trap or night event; exact eligibility requires the event adapter." },
    SupplementalOrganismReference { organism_id: "minecraft:vex", source: OrganismSource::SurfaceEvent, condition: "Conditional trader, patrol, raid, trap or night event; exact eligibility requires the event adapter." },
 ];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn canonical_finite_set_round_trips_without_alias_padding() {
        let all = SurfaceBiome::all();
        assert_eq!(all.len(), 43);
        let ids: HashSet<_> = all.iter().map(|biome| biome.id()).collect();
        assert_eq!(ids.len(), 43);
        for (index, biome) in all.iter().copied().enumerate() {
            assert_eq!(biome as usize, index);
            assert_eq!(biome.descriptor().biome, biome);
            assert_eq!(SurfaceBiome::from_id(biome.id()), Some(biome));
            assert_eq!(
                SurfaceBiome::from_id(biome.id().trim_start_matches("minecraft:")),
                Some(biome)
            );
            assert!(!biome.id().contains("ocean"));
            assert!(!biome.id().ends_with("caves"));
            assert!(biome.descriptor().gameplay_temperature.is_finite());
            assert!((0.0..=1.0).contains(&biome.descriptor().downfall));
        }
        for invalid in [
            "",
            "minecraft:ocean",
            "minecraft:lush_caves",
            "minecraft:deep_dark",
            "swampland",
            "minecraft:desert_hills",
            "mod:forest",
            "Forest",
            " forest",
        ] {
            assert_eq!(SurfaceBiome::from_id(invalid), None);
        }
    }

    #[test]
    fn source_values_and_edition_palettes_remain_independent() {
        let plains = SurfaceBiome::Plains.descriptor();
        assert_eq!(plains.gameplay_temperature, 0.8);
        assert_eq!(plains.downfall, 0.4);
        assert_eq!(plains.java_surface_water_rgb, [63, 118, 228]);
        assert_eq!(plains.bedrock_surface_water_rgb, [68, 175, 245]);
        let dappled = SurfaceBiome::DappledForest.descriptor();
        assert_eq!(dappled.gameplay_temperature, 0.6);
        assert_eq!(dappled.downfall, 0.6);
        assert_eq!(dappled.java_surface_water_rgb, [55, 81, 84]);
        assert_eq!(dappled.bedrock_surface_water_rgb, [55, 81, 84]);
        let desert = SurfaceBiome::Desert.descriptor();
        assert_eq!(desert.gameplay_temperature, 2.0);
        assert_eq!(desert.downfall, 0.0);
        assert!(!desert.has_precipitation);
        assert_eq!(
            SurfaceBiome::Swamp.descriptor().bedrock_id,
            "minecraft:swampland"
        );
    }

    #[test]
    fn specialist_features_are_explicit_and_species_are_not_configuration_counts() {
        let dappled = SurfaceBiome::DappledForest.descriptor();
        for id in [
            "minecraft:red_poplar_leaf_litter",
            "minecraft:orange_poplar_leaf_litter",
            "minecraft:yellow_poplar_leaf_litter",
            "minecraft:fallen_poplar_tree",
        ] {
            assert!(dappled.natural_tree_configuration_ids.contains(&id));
        }
        assert!(dappled.surface_block_ids.contains(&"minecraft:red_shrub"));
        assert!(dappled
            .surface_block_ids
            .contains(&"minecraft:shelf_mushroom"));
        let mangrove = SurfaceBiome::MangroveSwamp.descriptor();
        assert!(mangrove
            .surface_block_ids
            .contains(&"minecraft:mangrove_roots"));
        assert!(mangrove
            .surface_block_ids
            .contains(&"minecraft:muddy_mangrove_roots"));
        assert!(SurfaceBiome::BambooJungle
            .descriptor()
            .surface_block_ids
            .contains(&"minecraft:bamboo"));
        assert!(SurfaceBiome::Jungle
            .descriptor()
            .surface_block_ids
            .contains(&"minecraft:vine"));
        let woods: HashSet<_> = NATURAL_WOOD_SPECIES.into_iter().collect();
        assert_eq!(woods.len(), 10);
        assert!(!woods.contains("pine"));
        assert_eq!(
            SURFACE_AZALEA_INDICATOR_CONFIGURATION_ID,
            "minecraft:azalea_tree"
        );
    }

    #[test]
    fn supplemental_organisms_and_surface_structures_are_scoped() {
        let meadow = SurfaceBiome::Meadow.descriptor();
        assert!(!meadow.biome_table_fauna_ids.contains(&"minecraft:bee"));
        assert!(meadow
            .supplemental_organisms
            .iter()
            .any(|entry| entry.organism_id == "minecraft:bee"
                && entry.source == OrganismSource::BeeNest));
        let pale = SurfaceBiome::PaleGarden.descriptor();
        assert!(!pale.biome_table_fauna_ids.contains(&"minecraft:creaking"));
        assert!(pale
            .supplemental_organisms
            .iter()
            .any(|entry| entry.organism_id == "minecraft:creaking"
                && entry.source == OrganismSource::CreakingHeart));
        assert!(SurfaceBiome::Desert
            .descriptor()
            .surface_structure_variant_ids
            .contains(&"minecraft:village_desert"));
        assert!(SurfaceBiome::Desert
            .descriptor()
            .surface_structure_variant_ids
            .contains(&"minecraft:desert_well"));
        assert!(SurfaceBiome::DappledForest
            .descriptor()
            .surface_structure_variant_ids
            .contains(&"minecraft:abandoned_camp_dappled_forest"));
        assert!(SURFACE_EVENT_ORGANISMS
            .iter()
            .any(|entry| entry.organism_id == "minecraft:phantom"));
    }
}
