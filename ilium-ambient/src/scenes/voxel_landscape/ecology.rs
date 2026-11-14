//! Pure surface ecology for the seeded terrain kernel. Ground coordinates are
//! x/z in the kernel; `sample` calls its second ground coordinate `y` to match
//! the parent world API. No occupancy, cave carving or hydrology is modified.
//! Prescriptions describe the actual skin, substrate and feature morphology;
//! the world consumer must apply them rather than render the name alone.

use super::catalog::{BiomeAffinity, FeatureId, Material};
use super::noise::value2;
use super::terrain::{Biome as TerrainBiome, Column, SEA_LEVEL};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Biome {
    Plains,
    SunflowerPlains,
    Forest,
    BirchForest,
    OldGrowthBirchForest,
    DappledForest,
    DarkForest,
    FlowerForest,
    Taiga,
    SnowyTaiga,
    OldGrowthPineTaiga,
    OldGrowthSpruceTaiga,
    Desert,
    Savanna,
    SavannaPlateau,
    WindsweptSavanna,
    Jungle,
    SparseJungle,
    BambooJungle,
    Swamp,
    MangroveSwamp,
    Badlands,
    ErodedBadlands,
    WoodedBadlands,
    Meadow,
    CherryGrove,
    PaleGarden,
    SnowyPlains,
    IceSpikes,
    Grove,
    SnowySlopes,
    JaggedPeaks,
    FrozenPeaks,
    StonyPeaks,
    WindsweptHills,
    WindsweptForest,
    WindsweptGravellyHills,
    Beach,
    SnowyBeach,
    StonyShore,
    River,
    FrozenRiver,
    Ocean,
    DeepOcean,
    ColdOcean,
    DeepColdOcean,
    LukewarmOcean,
    DeepLukewarmOcean,
    WarmOcean,
    FrozenOcean,
    DeepFrozenOcean,
    MushroomFields,
}
pub const ALL: &[Biome] = &[
    Biome::Plains,
    Biome::SunflowerPlains,
    Biome::Forest,
    Biome::BirchForest,
    Biome::OldGrowthBirchForest,
    Biome::DappledForest,
    Biome::DarkForest,
    Biome::FlowerForest,
    Biome::Taiga,
    Biome::SnowyTaiga,
    Biome::OldGrowthPineTaiga,
    Biome::OldGrowthSpruceTaiga,
    Biome::Desert,
    Biome::Savanna,
    Biome::SavannaPlateau,
    Biome::WindsweptSavanna,
    Biome::Jungle,
    Biome::SparseJungle,
    Biome::BambooJungle,
    Biome::Swamp,
    Biome::MangroveSwamp,
    Biome::Badlands,
    Biome::ErodedBadlands,
    Biome::WoodedBadlands,
    Biome::Meadow,
    Biome::CherryGrove,
    Biome::PaleGarden,
    Biome::SnowyPlains,
    Biome::IceSpikes,
    Biome::Grove,
    Biome::SnowySlopes,
    Biome::JaggedPeaks,
    Biome::FrozenPeaks,
    Biome::StonyPeaks,
    Biome::WindsweptHills,
    Biome::WindsweptForest,
    Biome::WindsweptGravellyHills,
    Biome::Beach,
    Biome::SnowyBeach,
    Biome::StonyShore,
    Biome::River,
    Biome::FrozenRiver,
    Biome::Ocean,
    Biome::DeepOcean,
    Biome::ColdOcean,
    Biome::DeepColdOcean,
    Biome::LukewarmOcean,
    Biome::DeepLukewarmOcean,
    Biome::WarmOcean,
    Biome::FrozenOcean,
    Biome::DeepFrozenOcean,
    Biome::MushroomFields,
];
impl Biome {
    pub const ALL: &'static [Self] = ALL;
    pub const fn name(self) -> &'static str {
        match self {
            Self::Plains => "Plains",
            Self::SunflowerPlains => "Sunflower plains",
            Self::Forest => "Forest",
            Self::BirchForest => "Birch forest",
            Self::OldGrowthBirchForest => "Old-growth birch forest",
            Self::DappledForest => "Dappled forest",
            Self::DarkForest => "Dark forest",
            Self::FlowerForest => "Flower forest",
            Self::Taiga => "Taiga",
            Self::SnowyTaiga => "Snowy taiga",
            Self::OldGrowthPineTaiga => "Old-growth pine taiga",
            Self::OldGrowthSpruceTaiga => "Old-growth spruce taiga",
            Self::Desert => "Desert",
            Self::Savanna => "Savanna",
            Self::SavannaPlateau => "Savanna plateau",
            Self::WindsweptSavanna => "Windswept savanna",
            Self::Jungle => "Jungle",
            Self::SparseJungle => "Sparse jungle",
            Self::BambooJungle => "Bamboo jungle",
            Self::Swamp => "Swamp",
            Self::MangroveSwamp => "Mangrove swamp",
            Self::Badlands => "Badlands",
            Self::ErodedBadlands => "Eroded badlands",
            Self::WoodedBadlands => "Wooded badlands",
            Self::Meadow => "Meadow",
            Self::CherryGrove => "Cherry grove",
            Self::PaleGarden => "Pale garden",
            Self::SnowyPlains => "Snowy plains",
            Self::IceSpikes => "Ice spikes",
            Self::Grove => "Grove",
            Self::SnowySlopes => "Snowy slopes",
            Self::JaggedPeaks => "Jagged peaks",
            Self::FrozenPeaks => "Frozen peaks",
            Self::StonyPeaks => "Stony peaks",
            Self::WindsweptHills => "Windswept hills",
            Self::WindsweptForest => "Windswept forest",
            Self::WindsweptGravellyHills => "Windswept gravelly hills",
            Self::Beach => "Beach",
            Self::SnowyBeach => "Snowy beach",
            Self::StonyShore => "Stony shore",
            Self::River => "River",
            Self::FrozenRiver => "Frozen river",
            Self::Ocean => "Ocean",
            Self::DeepOcean => "Deep ocean",
            Self::ColdOcean => "Cold ocean",
            Self::DeepColdOcean => "Deep cold ocean",
            Self::LukewarmOcean => "Lukewarm ocean",
            Self::DeepLukewarmOcean => "Deep lukewarm ocean",
            Self::WarmOcean => "Warm ocean",
            Self::FrozenOcean => "Frozen ocean",
            Self::DeepFrozenOcean => "Deep frozen ocean",
            Self::MushroomFields => "Mushroom fields",
        }
    }
}

/// Dominant vegetation architecture, separate from the broad placement affinity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Vegetation {
    Grassland,
    Sunflowers,
    OakWoodland,
    BirchWoodland,
    OldBirch,
    MixedDappled,
    DenseOak,
    FloralWoodland,
    SpruceTaiga,
    SnowSpruce,
    OldPine,
    OldSpruce,
    Succulents,
    AcaciaGrassland,
    PlateauAcacia,
    WindAcacia,
    JungleEmergents,
    JungleEdge,
    Bamboo,
    MarshReeds,
    Mangroves,
    DryScrub,
    HoodooScrub,
    MesaWoodland,
    AlpineFlowers,
    Cherry,
    PaleWoodland,
    SnowGrassland,
    IceNeedles,
    MountainSpruce,
    SparseSnow,
    BareRock,
    Glacier,
    RockyAlpine,
    WindGrass,
    WindPine,
    GravelScrub,
    DuneGrass,
    SnowDune,
    CoastPine,
    Riparian,
    FrozenRiparian,
    Seagrass,
    DeepSeagrass,
    ColdKelp,
    DeepColdKelp,
    WarmSeagrass,
    DeepWarmSeagrass,
    CoralReef,
    SeaIce,
    DeepSeaIce,
    Fungi,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Landform {
    Lowland,
    Rolling,
    ForestFloor,
    Plateau,
    WindExposed,
    WetBasin,
    Mesa,
    Hoodoos,
    WoodedMesa,
    MeadowSlope,
    GroveSlope,
    SnowPlain,
    IceSpikes,
    SnowSlope,
    JaggedRock,
    GlacialPeak,
    BarePeak,
    GravelHill,
    SandyCoast,
    FrozenCoast,
    RockyCoast,
    RiverChannel,
    FrozenChannel,
    ShallowSea,
    DeepSea,
    ColdShallowSea,
    ColdDeepSea,
    WarmShallowSea,
    WarmDeepSea,
    ReefShelf,
    FrozenShallowSea,
    FrozenDeepSea,
    FungalCoast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ecology {
    pub biome: Biome,
    pub affinity: BiomeAffinity,
    /// Solid skin / seabed. Water occupancy remains the terrain kernel's job.
    pub surface: Material,
    pub soil: Material,
    pub rock: Material,
    /// 0..100; world placement can use this as a deterministic coverage percent.
    pub vegetation_density: u8,
    pub vegetation: Vegetation,
    pub landform: Landform,
    /// Thin frozen water skin, never a replacement for the underlying column.
    pub frozen_surface: bool,
    pub preferred_features: &'static [FeatureId],
}

impl Biome {
    /// Distinct material and morphology prescriptions, not random RGB variants.
    pub const fn prescription(self) -> Ecology {
        use BiomeAffinity as A;
        use Landform as L;
        use Material as M;
        use Vegetation as V;
        let (affinity, surface, soil, rock, density, vegetation, landform, frozen, preferred): (
            A,
            M,
            M,
            M,
            u8,
            V,
            L,
            bool,
            &'static [FeatureId],
        ) = match self {
            Self::Plains => (
                A::Temperate,
                M::Grass,
                M::Dirt,
                M::Stone,
                20,
                V::Grassland,
                L::Lowland,
                false,
                &[FeatureId(47), FeatureId(18)],
            ),
            Self::SunflowerPlains => (
                A::Temperate,
                M::Grass,
                M::Dirt,
                M::Stone,
                35,
                V::Sunflowers,
                L::Rolling,
                false,
                &[FeatureId(46), FeatureId(47)],
            ),
            Self::Forest => (
                A::Temperate,
                M::Grass,
                M::Dirt,
                M::Stone,
                66,
                V::OakWoodland,
                L::ForestFloor,
                false,
                &[FeatureId(0), FeatureId(1), FeatureId(36)],
            ),
            Self::BirchForest => (
                A::Temperate,
                M::Grass,
                M::Dirt,
                M::Stone,
                57,
                V::BirchWoodland,
                L::ForestFloor,
                false,
                &[FeatureId(4), FeatureId(5), FeatureId(36)],
            ),
            Self::OldGrowthBirchForest => (
                A::Temperate,
                M::Podzol,
                M::Dirt,
                M::Stone,
                70,
                V::OldBirch,
                L::ForestFloor,
                false,
                &[FeatureId(4), FeatureId(5), FeatureId(44)],
            ),
            Self::DappledForest => (
                A::Temperate,
                M::Grass,
                M::Dirt,
                M::Limestone,
                64,
                V::MixedDappled,
                L::ForestFloor,
                false,
                &[FeatureId(0), FeatureId(5), FeatureId(37)],
            ),
            Self::DarkForest => (
                A::Temperate,
                M::Podzol,
                M::Dirt,
                M::Stone,
                88,
                V::DenseOak,
                L::ForestFloor,
                false,
                &[FeatureId(2), FeatureId(1), FeatureId(43)],
            ),
            Self::FlowerForest => (
                A::Temperate,
                M::Grass,
                M::Dirt,
                M::Stone,
                62,
                V::FloralWoodland,
                L::ForestFloor,
                false,
                &[FeatureId(0), FeatureId(4), FeatureId(47), FeatureId(45)],
            ),
            Self::Taiga => (
                A::Boreal,
                M::Podzol,
                M::Dirt,
                M::Stone,
                65,
                V::SpruceTaiga,
                L::ForestFloor,
                false,
                &[FeatureId(6), FeatureId(36), FeatureId(39)],
            ),
            Self::SnowyTaiga => (
                A::Frozen,
                M::Snow,
                M::Podzol,
                M::Stone,
                53,
                V::SnowSpruce,
                L::ForestFloor,
                false,
                &[FeatureId(7), FeatureId(22), FeatureId(41)],
            ),
            Self::OldGrowthPineTaiga => (
                A::Boreal,
                M::Podzol,
                M::Dirt,
                M::Granite,
                76,
                V::OldPine,
                L::ForestFloor,
                false,
                &[FeatureId(8), FeatureId(22), FeatureId(44)],
            ),
            Self::OldGrowthSpruceTaiga => (
                A::Boreal,
                M::Moss,
                M::Podzol,
                M::Stone,
                81,
                V::OldSpruce,
                L::ForestFloor,
                false,
                &[FeatureId(6), FeatureId(22), FeatureId(40)],
            ),
            Self::Desert => (
                A::Arid,
                M::Sand,
                M::Sand,
                M::Sandstone,
                13,
                V::Succulents,
                L::Lowland,
                false,
                &[FeatureId(26), FeatureId(27), FeatureId(28), FeatureId(72)],
            ),
            Self::Savanna => (
                A::Arid,
                M::Grass,
                M::Dirt,
                M::Sandstone,
                23,
                V::AcaciaGrassland,
                L::Rolling,
                false,
                &[FeatureId(9), FeatureId(29), FeatureId(30)],
            ),
            Self::SavannaPlateau => (
                A::Arid,
                M::Grass,
                M::Dirt,
                M::Stone,
                31,
                V::PlateauAcacia,
                L::Plateau,
                false,
                &[FeatureId(9), FeatureId(31), FeatureId(58)],
            ),
            Self::WindsweptSavanna => (
                A::Arid,
                M::Gravel,
                M::Dirt,
                M::Stone,
                17,
                V::WindAcacia,
                L::WindExposed,
                false,
                &[FeatureId(9), FeatureId(21), FeatureId(52)],
            ),
            Self::Jungle => (
                A::Tropical,
                M::Grass,
                M::Dirt,
                M::Stone,
                92,
                V::JungleEmergents,
                L::ForestFloor,
                false,
                &[FeatureId(10), FeatureId(11), FeatureId(12), FeatureId(51)],
            ),
            Self::SparseJungle => (
                A::Tropical,
                M::Grass,
                M::Dirt,
                M::Limestone,
                48,
                V::JungleEdge,
                L::Rolling,
                false,
                &[FeatureId(10), FeatureId(15), FeatureId(37)],
            ),
            Self::BambooJungle => (
                A::Tropical,
                M::Podzol,
                M::Dirt,
                M::Stone,
                85,
                V::Bamboo,
                L::ForestFloor,
                false,
                &[FeatureId(25), FeatureId(12), FeatureId(36)],
            ),
            Self::Swamp => (
                A::Wetland,
                M::Mud,
                M::Clay,
                M::Stone,
                69,
                V::MarshReeds,
                L::WetBasin,
                false,
                &[
                    FeatureId(3),
                    FeatureId(32),
                    FeatureId(33),
                    FeatureId(34),
                    FeatureId(35),
                ],
            ),
            Self::MangroveSwamp => (
                A::Wetland,
                M::Mud,
                M::Mud,
                M::Clay,
                80,
                V::Mangroves,
                L::WetBasin,
                false,
                &[FeatureId(13), FeatureId(14), FeatureId(32), FeatureId(86)],
            ),
            Self::Badlands => (
                A::Badlands,
                M::RedSand,
                M::Terracotta,
                M::RedSandstone,
                8,
                V::DryScrub,
                L::Mesa,
                false,
                &[FeatureId(30), FeatureId(57), FeatureId(64)],
            ),
            Self::ErodedBadlands => (
                A::Badlands,
                M::Terracotta,
                M::RedSand,
                M::RedSandstone,
                5,
                V::HoodooScrub,
                L::Hoodoos,
                false,
                &[FeatureId(56), FeatureId(55), FeatureId(30)],
            ),
            Self::WoodedBadlands => (
                A::Badlands,
                M::RedSand,
                M::Podzol,
                M::RedSandstone,
                28,
                V::MesaWoodland,
                L::WoodedMesa,
                false,
                &[FeatureId(21), FeatureId(9), FeatureId(57)],
            ),
            Self::Meadow => (
                A::Alpine,
                M::Grass,
                M::Dirt,
                M::Stone,
                43,
                V::AlpineFlowers,
                L::MeadowSlope,
                false,
                &[FeatureId(47), FeatureId(48), FeatureId(38)],
            ),
            Self::CherryGrove => (
                A::Temperate,
                M::Grass,
                M::Dirt,
                M::Stone,
                65,
                V::Cherry,
                L::GroveSlope,
                false,
                &[FeatureId(17), FeatureId(47), FeatureId(45)],
            ),
            Self::PaleGarden => (
                A::Temperate,
                M::Moss,
                M::Podzol,
                M::Limestone,
                72,
                V::PaleWoodland,
                L::ForestFloor,
                false,
                &[FeatureId(4), FeatureId(21), FeatureId(40), FeatureId(45)],
            ),
            Self::SnowyPlains => (
                A::Frozen,
                M::Snow,
                M::Dirt,
                M::Stone,
                9,
                V::SnowGrassland,
                L::SnowPlain,
                false,
                &[FeatureId(41), FeatureId(59)],
            ),
            Self::IceSpikes => (
                A::Frozen,
                M::Ice,
                M::Snow,
                M::Stone,
                2,
                V::IceNeedles,
                L::IceSpikes,
                true,
                &[FeatureId(76), FeatureId(100)],
            ),
            Self::Grove => (
                A::Alpine,
                M::Snow,
                M::Podzol,
                M::Stone,
                59,
                V::MountainSpruce,
                L::GroveSlope,
                false,
                &[FeatureId(7), FeatureId(24), FeatureId(40)],
            ),
            Self::SnowySlopes => (
                A::Alpine,
                M::Snow,
                M::Snow,
                M::Slate,
                5,
                V::SparseSnow,
                L::SnowSlope,
                false,
                &[FeatureId(77), FeatureId(61), FeatureId(101)],
            ),
            Self::JaggedPeaks => (
                A::Alpine,
                M::Slate,
                M::Gravel,
                M::Stone,
                1,
                V::BareRock,
                L::JaggedRock,
                false,
                &[FeatureId(53), FeatureId(58), FeatureId(67)],
            ),
            Self::FrozenPeaks => (
                A::Frozen,
                M::Snow,
                M::Ice,
                M::Stone,
                0,
                V::Glacier,
                L::GlacialPeak,
                false,
                &[FeatureId(77), FeatureId(59), FeatureId(60)],
            ),
            Self::StonyPeaks => (
                A::Alpine,
                M::Stone,
                M::Gravel,
                M::Granite,
                2,
                V::RockyAlpine,
                L::BarePeak,
                false,
                &[FeatureId(53), FeatureId(52), FeatureId(63)],
            ),
            Self::WindsweptHills => (
                A::Alpine,
                M::Grass,
                M::Dirt,
                M::Stone,
                18,
                V::WindGrass,
                L::WindExposed,
                false,
                &[FeatureId(48), FeatureId(38), FeatureId(58)],
            ),
            Self::WindsweptForest => (
                A::Alpine,
                M::Podzol,
                M::Dirt,
                M::Stone,
                39,
                V::WindPine,
                L::WindExposed,
                false,
                &[FeatureId(24), FeatureId(8), FeatureId(52)],
            ),
            Self::WindsweptGravellyHills => (
                A::Alpine,
                M::Gravel,
                M::Gravel,
                M::Stone,
                7,
                V::GravelScrub,
                L::GravelHill,
                false,
                &[FeatureId(52), FeatureId(61), FeatureId(48)],
            ),
            Self::Beach => (
                A::Coastal,
                M::Sand,
                M::Sand,
                M::Sandstone,
                8,
                V::DuneGrass,
                L::SandyCoast,
                false,
                &[FeatureId(16), FeatureId(89)],
            ),
            Self::SnowyBeach => (
                A::Frozen,
                M::Snow,
                M::Sand,
                M::Stone,
                3,
                V::SnowDune,
                L::FrozenCoast,
                false,
                &[FeatureId(99), FeatureId(100)],
            ),
            Self::StonyShore => (
                A::Coastal,
                M::Stone,
                M::Gravel,
                M::Basalt,
                12,
                V::CoastPine,
                L::RockyCoast,
                false,
                &[FeatureId(20), FeatureId(70), FeatureId(88)],
            ),
            Self::River => (
                A::Wetland,
                M::Gravel,
                M::Clay,
                M::Stone,
                25,
                V::Riparian,
                L::RiverChannel,
                false,
                &[FeatureId(32), FeatureId(82), FeatureId(84)],
            ),
            Self::FrozenRiver => (
                A::Frozen,
                M::Ice,
                M::Clay,
                M::Stone,
                5,
                V::FrozenRiparian,
                L::FrozenChannel,
                true,
                &[FeatureId(99), FeatureId(101)],
            ),
            Self::Ocean => (
                A::Coastal,
                M::Sand,
                M::Sand,
                M::Stone,
                11,
                V::Seagrass,
                L::ShallowSea,
                false,
                &[FeatureId(49), FeatureId(89)],
            ),
            Self::DeepOcean => (
                A::Coastal,
                M::Clay,
                M::Gravel,
                M::Stone,
                3,
                V::DeepSeagrass,
                L::DeepSea,
                false,
                &[FeatureId(49)],
            ),
            Self::ColdOcean => (
                A::Coastal,
                M::Gravel,
                M::Clay,
                M::Stone,
                9,
                V::ColdKelp,
                L::ColdShallowSea,
                false,
                &[FeatureId(49), FeatureId(88)],
            ),
            Self::DeepColdOcean => (
                A::Coastal,
                M::Clay,
                M::Gravel,
                M::Basalt,
                2,
                V::DeepColdKelp,
                L::ColdDeepSea,
                false,
                &[FeatureId(49)],
            ),
            Self::LukewarmOcean => (
                A::Coastal,
                M::Sand,
                M::Clay,
                M::Limestone,
                17,
                V::WarmSeagrass,
                L::WarmShallowSea,
                false,
                &[FeatureId(49), FeatureId(86)],
            ),
            Self::DeepLukewarmOcean => (
                A::Coastal,
                M::Clay,
                M::Sand,
                M::Limestone,
                5,
                V::DeepWarmSeagrass,
                L::WarmDeepSea,
                false,
                &[FeatureId(49), FeatureId(50)],
            ),
            Self::WarmOcean => (
                A::Coastal,
                M::Sand,
                M::Sand,
                M::Limestone,
                33,
                V::CoralReef,
                L::ReefShelf,
                false,
                &[FeatureId(50), FeatureId(49), FeatureId(88)],
            ),
            Self::FrozenOcean => (
                A::Frozen,
                M::Gravel,
                M::Clay,
                M::Stone,
                1,
                V::SeaIce,
                L::FrozenShallowSea,
                true,
                &[FeatureId(99), FeatureId(100)],
            ),
            Self::DeepFrozenOcean => (
                A::Frozen,
                M::Clay,
                M::Gravel,
                M::Stone,
                0,
                V::DeepSeaIce,
                L::FrozenDeepSea,
                true,
                &[FeatureId(100), FeatureId(76)],
            ),
            Self::MushroomFields => (
                A::Temperate,
                M::Podzol,
                M::Mud,
                M::Stone,
                63,
                V::Fungi,
                L::FungalCoast,
                false,
                &[FeatureId(42), FeatureId(43), FeatureId(45), FeatureId(40)],
            ),
        };
        Ecology {
            biome: self,
            affinity,
            surface,
            soil,
            rock,
            vegetation_density: density,
            vegetation,
            landform,
            frozen_surface: frozen,
            preferred_features: preferred,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Habitat {
    temperature: f64,
    moisture: f64,
    height: i16,
    depth: i16,
    continentalness: f64,
    river: bool,
    ocean: bool,
    shore: bool,
    woodland: f64,
    flowers: f64,
    wind: f64,
    geology: f64,
}

fn classify(h: Habitat) -> Biome {
    use Biome::*;
    let t = h.temperature;
    let m = h.moisture;
    let altitude = h.height;
    if h.river {
        return if t < 0.26 { FrozenRiver } else { River };
    }
    if h.ocean {
        let deep = h.depth >= 8;
        return if t < 0.20 {
            if deep {
                DeepFrozenOcean
            } else {
                FrozenOcean
            }
        } else if t < 0.38 {
            if deep {
                DeepColdOcean
            } else {
                ColdOcean
            }
        } else if t > 0.70 {
            WarmOcean
        } else if t > 0.56 {
            if deep {
                DeepLukewarmOcean
            } else {
                LukewarmOcean
            }
        } else if deep {
            DeepOcean
        } else {
            Ocean
        };
    }
    if h.shore {
        return if t < 0.26 {
            SnowyBeach
        } else if h.geology > 0.25 {
            StonyShore
        } else {
            Beach
        };
    }
    // A rare low coastal land habitat, never an underwater column relabelled
    // as an island. It receives fungal soil and giant/small mushroom recipes.
    if altitude < SEA_LEVEL + 9
        && h.continentalness < 0.10
        && t > 0.30
        && m > 0.45
        && h.geology > 0.58
    {
        return MushroomFields;
    }
    if altitude >= 54 {
        return if t < 0.23 {
            FrozenPeaks
        } else if t < 0.45 {
            JaggedPeaks
        } else {
            StonyPeaks
        };
    }
    if altitude >= 35 {
        if t < 0.27 {
            return if m > 0.53 && h.woodland > 0.0 && altitude < 48 {
                Grove
            } else {
                SnowySlopes
            };
        }
        if t < 0.43 && altitude >= 46 {
            return Grove;
        }
        if t < 0.61 && h.wind > 0.28 {
            return if m < 0.45 && h.geology > 0.10 {
                WindsweptGravellyHills
            } else if m > 0.60 {
                WindsweptForest
            } else {
                WindsweptHills
            };
        }
        if (0.39..0.64).contains(&t) && m > 0.48 {
            return if m > 0.55 && h.flowers > 0.20 {
                CherryGrove
            } else {
                Meadow
            };
        }
    }
    if altitude <= SEA_LEVEL + 7 && m > 0.60 {
        return if t > 0.62 { MangroveSwamp } else { Swamp };
    }
    if t > 0.61 && m < 0.34 {
        if h.geology > 0.10 {
            return if h.geology > 0.40 {
                ErodedBadlands
            } else if m > 0.23 && h.woodland > 0.10 {
                WoodedBadlands
            } else {
                Badlands
            };
        }
        return Desert;
    }
    if t > 0.61 && m < 0.62 {
        return if h.wind > 0.28 {
            WindsweptSavanna
        } else if altitude >= 32 {
            SavannaPlateau
        } else {
            Savanna
        };
    }
    if t > 0.64 && m >= 0.62 {
        return if h.woodland > 0.30 {
            BambooJungle
        } else if m < 0.72 || h.woodland < -0.20 {
            SparseJungle
        } else {
            Jungle
        };
    }
    if t < 0.26 {
        return if m > 0.53 && h.woodland > 0.0 {
            SnowyTaiga
        } else if h.geology > 0.28 && m < 0.53 {
            IceSpikes
        } else {
            SnowyPlains
        };
    }
    if t < 0.39 {
        return if h.woodland > 0.25 {
            OldGrowthPineTaiga
        } else if h.woodland < -0.25 {
            OldGrowthSpruceTaiga
        } else {
            Taiga
        };
    }
    if m > 0.55 {
        return if m > 0.72 && h.woodland < -0.40 {
            PaleGarden
        } else if m > 0.70 && h.woodland > 0.0 {
            DarkForest
        } else if h.flowers > 0.30 {
            FlowerForest
        } else if m < 0.69 && h.woodland > 0.10 {
            if h.woodland > 0.42 {
                OldGrowthBirchForest
            } else {
                BirchForest
            }
        } else if m < 0.65 && h.woodland < -0.10 {
            DappledForest
        } else {
            Forest
        };
    }
    if h.flowers > 0.25 {
        SunflowerPlains
    } else {
        Plains
    }
}

/// Deterministic ecology for an authoritative existing terrain column. Smooth
/// absolute-coordinate fields give coherent patches across chunk/negative seams.
/// No per-chunk random state or hash-based biome lottery is used.
pub fn sample(seed: u64, x: i32, y: i32, column: Column) -> Ecology {
    let (x, z) = (i64::from(x), i64::from(y));
    let biome = classify(Habitat {
        temperature: column.climate.temperature,
        moisture: column.climate.moisture,
        height: column.surface_height,
        depth: column
            .water_level
            .map_or(0, |level| (level - column.height).max(0)),
        continentalness: column.climate.continentalness,
        river: column.river,
        ocean: column.biome == TerrainBiome::Ocean,
        shore: column.biome == TerrainBiome::Beach,
        woodland: value2(seed ^ 0x776f_6f64_6c61_6e64, x, z, 256),
        flowers: value2(seed ^ 0x0066_6c6f_7765_7273, x, z, 192),
        wind: value2(seed ^ 0x7769_6e64, x, z, 320),
        geology: value2(seed ^ 0x0067_656f_6c6f_6779, x, z, 384),
    });
    biome.prescription()
}

/// Frozen world-coordinate fixtures discovered from real kernel samples, seed 7.
/// Parent acceptance can use these locations for every named environment.
pub const WITNESSES: &[(Biome, u64, i32, i32)] = &[
    (Biome::Plains, 7, 89489, -177042),
    (Biome::SunflowerPlains, 7, 129418, -245508),
    (Biome::Forest, 7, -211987, -103079),
    (Biome::BirchForest, 7, 496014, 427386),
    (Biome::OldGrowthBirchForest, 7, -219639, 491149),
    (Biome::DappledForest, 7, -293991, -486597),
    (Biome::DarkForest, 7, -470183, 210104),
    (Biome::FlowerForest, 7, -143435, 98498),
    (Biome::Taiga, 7, -355326, 80605),
    (Biome::SnowyTaiga, 7, -158166, 195368),
    (Biome::OldGrowthPineTaiga, 7, -211352, -112764),
    (Biome::OldGrowthSpruceTaiga, 7, -303131, -504351),
    (Biome::Desert, 7, -295172, 336445),
    (Biome::Savanna, 7, 433032, -510133),
    (Biome::SavannaPlateau, 7, -220793, 268133),
    (Biome::WindsweptSavanna, 7, -89880, -29489),
    (Biome::Jungle, 7, 483033, -231554),
    (Biome::SparseJungle, 7, -104517, 44594),
    (Biome::BambooJungle, 7, -515816, -499111),
    (Biome::Swamp, 7, -513697, 385027),
    (Biome::MangroveSwamp, 7, -439632, -174669),
    (Biome::Badlands, 7, -35364, -368570),
    (Biome::ErodedBadlands, 7, -495571, 476289),
    (Biome::WoodedBadlands, 7, 296511, 15097),
    (Biome::Meadow, 7, 142756, 8332),
    (Biome::CherryGrove, 7, -461256, 174167),
    (Biome::PaleGarden, 7, -321821, -494402),
    (Biome::SnowyPlains, 7, 237452, 353696),
    (Biome::IceSpikes, 7, 120296, -385340),
    (Biome::Grove, 7, 199597, -344177),
    (Biome::SnowySlopes, 7, -503093, -83566),
    (Biome::JaggedPeaks, 7, -195466, -352161),
    (Biome::FrozenPeaks, 7, -516493, -41424),
    (Biome::StonyPeaks, 7, 163287, 490484),
    (Biome::WindsweptHills, 7, -310466, -264706),
    (Biome::WindsweptForest, 7, -115009, -29940),
    (Biome::WindsweptGravellyHills, 7, 105066, -119755),
    (Biome::Beach, 7, 122929, 431834),
    (Biome::SnowyBeach, 7, 243924, 370536),
    (Biome::StonyShore, 7, -220541, 174247),
    (Biome::River, 7, -18734, 232801),
    (Biome::FrozenRiver, 7, -22619, -390032),
    (Biome::Ocean, 7, -325757, 503622),
    (Biome::DeepOcean, 7, 235017, -135172),
    (Biome::ColdOcean, 7, -100481, 181679),
    (Biome::DeepColdOcean, 7, -256626, 203507),
    (Biome::LukewarmOcean, 7, 320371, -162809),
    (Biome::DeepLukewarmOcean, 7, -45175, 346083),
    (Biome::WarmOcean, 7, 359868, -366127),
    (Biome::FrozenOcean, 7, -82405, -214974),
    (Biome::DeepFrozenOcean, 7, 238858, 346128),
    (Biome::MushroomFields, 7, 146131, 178553),
];

#[cfg(test)]
mod tests {
    use super::super::catalog::FEATURE_RECIPES;
    use super::super::terrain::Terrain;
    use super::*;
    use std::collections::HashSet;

    fn inland() -> Habitat {
        Habitat {
            temperature: 0.5,
            moisture: 0.5,
            height: 28,
            depth: 0,
            continentalness: 0.2,
            river: false,
            ocean: false,
            shore: false,
            woodland: 0.0,
            flowers: 0.0,
            wind: 0.0,
            geology: 0.0,
        }
    }

    #[test]
    fn all_52_have_exact_generated_world_witnesses() {
        assert_eq!(ALL.len(), 52);
        assert_eq!(Biome::ALL, ALL);
        assert_eq!(WITNESSES.len(), 52);
        let mut witnessed = HashSet::new();
        for &(biome, seed, x, z) in WITNESSES {
            let column = Terrain::new(seed).sample(x, z);
            let sampled = sample(seed, x, z, column);
            assert_eq!(
                sampled.biome,
                biome,
                "{} witness ({seed},{x},{z})",
                biome.name()
            );
            assert!(witnessed.insert(biome));
            assert_eq!(sampled, biome.prescription());
            assert_eq!(sample(seed, x, z, column), sampled);
        }
        assert!(ALL.iter().all(|biome| witnessed.contains(biome)));
    }

    #[test]
    fn inventory_has_distinct_actionable_skin_and_feature_prescriptions() {
        let mut names = HashSet::new();
        let mut prescriptions = HashSet::new();
        for &biome in ALL {
            let p = biome.prescription();
            assert!(names.insert(biome.name()));
            assert!(!p.preferred_features.is_empty());
            assert!(p.vegetation_density <= 100);
            for id in p.preferred_features {
                assert_eq!(FEATURE_RECIPES[usize::from(id.0)].id, *id);
            }
            // Excludes biome name, vegetation/landform labels: actual renderer /
            // placement inputs must distinguish every profile independently.
            assert!(
                prescriptions.insert((
                    p.affinity,
                    p.surface,
                    p.soil,
                    p.rock,
                    p.vegetation_density,
                    p.frozen_surface,
                    p.preferred_features
                )),
                "name-only alias: {}",
                biome.name()
            );
        }
        assert_eq!(
            Biome::SunflowerPlains.prescription().preferred_features[0],
            FeatureId(46)
        );
        assert_eq!(
            Biome::BambooJungle.prescription().preferred_features[0],
            FeatureId(25)
        );
        assert_eq!(
            Biome::MangroveSwamp.prescription().preferred_features[0],
            FeatureId(13)
        );
        assert_eq!(
            Biome::CherryGrove.prescription().preferred_features[0],
            FeatureId(17)
        );
        assert_eq!(
            Biome::WarmOcean.prescription().preferred_features[0],
            FeatureId(50)
        );
        assert!(
            Biome::DarkForest.prescription().vegetation_density
                > Biome::Forest.prescription().vegetation_density
        );
        assert!(
            Biome::SparseJungle.prescription().vegetation_density
                < Biome::Jungle.prescription().vegetation_density
        );
    }

    #[test]
    fn river_ocean_and_shore_rules_obey_occupancy_and_freezing_boundaries() {
        let mut h = inland();
        h.river = true;
        h.ocean = true;
        h.shore = true;
        h.temperature = 0.259;
        assert_eq!(classify(h), Biome::FrozenRiver);
        h.temperature = 0.26;
        assert_eq!(classify(h), Biome::River);
        h.river = false;
        h.shore = false;
        h.depth = 7;
        h.temperature = 0.199;
        assert_eq!(classify(h), Biome::FrozenOcean);
        h.depth = 8;
        assert_eq!(classify(h), Biome::DeepFrozenOcean);
        h.temperature = 0.20;
        assert_eq!(classify(h), Biome::DeepColdOcean);
        h.temperature = 0.38;
        assert_eq!(classify(h), Biome::DeepOcean);
        h.temperature = 0.561;
        assert_eq!(classify(h), Biome::DeepLukewarmOcean);
        h.temperature = 0.701;
        assert_eq!(classify(h), Biome::WarmOcean);
        h.ocean = false;
        h.shore = true;
        h.temperature = 0.25;
        assert_eq!(classify(h), Biome::SnowyBeach);
        h.temperature = 0.30;
        h.geology = 0.251;
        assert_eq!(classify(h), Biome::StonyShore);
        h.geology = 0.25;
        assert_eq!(classify(h), Biome::Beach);
        assert!(Biome::FrozenRiver.prescription().frozen_surface);
        assert!(!Biome::River.prescription().frozen_surface);
    }

    #[test]
    fn mountain_jungle_desert_and_fungal_rules_require_real_habitat() {
        let mut h = inland();
        h.height = 54;
        h.temperature = 0.22;
        assert_eq!(classify(h), Biome::FrozenPeaks);
        h.temperature = 0.23;
        assert_eq!(classify(h), Biome::JaggedPeaks);
        h.temperature = 0.45;
        assert_eq!(classify(h), Biome::StonyPeaks);
        h.height = 28;
        h.temperature = 0.8;
        h.moisture = 0.2;
        assert_eq!(classify(h), Biome::Desert);
        h.geology = 0.41;
        assert_eq!(classify(h), Biome::ErodedBadlands);
        h.geology = 0.0;
        h.moisture = 0.8;
        h.woodland = 0.31;
        assert_eq!(classify(h), Biome::BambooJungle);
        h.woodland = 0.0;
        assert_eq!(classify(h), Biome::Jungle);
        h.moisture = 0.65;
        assert_eq!(classify(h), Biome::SparseJungle);
        h.height = 22;
        h.temperature = 0.5;
        h.moisture = 0.5;
        h.continentalness = 0.0;
        h.geology = 0.59;
        assert_eq!(classify(h), Biome::MushroomFields);
        h.ocean = true;
        h.depth = 4;
        assert_eq!(
            classify(h),
            Biome::Ocean,
            "fungal fields never relabel underwater terrain"
        );
    }

    #[test]
    fn sample_is_order_independent_and_absolute_coordinate_safe() {
        let terrain = Terrain::new(91);
        let mut points = [
            (i32::MIN, i32::MAX),
            (-257, -256),
            (-17, -16),
            (-1, 0),
            (0, -1),
            (15, 16),
            (255, 256),
            (i32::MAX, i32::MIN),
        ];
        let forward: Vec<_> = points
            .iter()
            .map(|&(x, z)| sample(91, x, z, terrain.sample(x, z)))
            .collect();
        points.reverse();
        let mut backward: Vec<_> = points
            .iter()
            .map(|&(x, z)| sample(91, x, z, terrain.sample(x, z)))
            .collect();
        backward.reverse();
        assert_eq!(forward, backward);
        for &(biome, seed, x, z) in WITNESSES {
            let column = Terrain::new(seed).sample(x, z);
            // The site must still have its original kernel occupancy after sampling.
            let before: Vec<_> = (0..80).map(|height| column.block(height)).collect();
            assert_eq!(sample(seed, x, z, column).biome, biome);
            assert_eq!(
                (0..80)
                    .map(|height| column.block(height))
                    .collect::<Vec<_>>(),
                before
            );
        }
    }
}
