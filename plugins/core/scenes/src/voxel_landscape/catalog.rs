//! Original surface-world vocabulary and executable, block-space feature recipes.
//! Coordinates are local integer blocks; +Y is up. Later operations overwrite earlier
//! ones. `None` removes a block. Callers own terrain anchoring, orientation, collision
//! checks and biome selection. No external texture-pack artwork is embedded.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FeatureId(pub u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FeatureCategory {
    Trees,
    Vegetation,
    Geology,
    Water,
    Settlement,
    Agriculture,
    Ruins,
    Infrastructure,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BiomeAffinity {
    Any,
    Temperate,
    Boreal,
    Arid,
    Tropical,
    Wetland,
    Coastal,
    Alpine,
    Frozen,
    Badlands,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Placement {
    DryGround,
    WaterEdge,
    ShallowWater,
    Cliff,
    Slope,
    FlatGround,
    SnowGround,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Top,
    Left,
    Right,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motif {
    Turf,
    Granular,
    Rock,
    Layered,
    Bark,
    Leaves,
    Boards,
    Masonry,
    Glazed,
    Crop,
    Liquid,
    Crystal,
    Fabric,
    Metal,
    Books,
    Ore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Material {
    Grass,
    Dirt,
    Stone,
    Cobblestone,
    MossStone,
    Sand,
    RedSand,
    Sandstone,
    RedSandstone,
    Gravel,
    Clay,
    Terracotta,
    Granite,
    Basalt,
    Limestone,
    Slate,
    CoalOre,
    IronOre,
    CopperOre,
    OakLog,
    BirchLog,
    SpruceLog,
    JungleLog,
    AcaciaLog,
    MangroveLog,
    OakLeaves,
    BirchLeaves,
    SpruceLeaves,
    JungleLeaves,
    CherryLeaves,
    DryLeaves,
    Planks,
    DarkPlanks,
    Bricks,
    Glass,
    Water,
    Snow,
    Ice,
    Cactus,
    Wheat,
    Carrot,
    Potato,
    Beetroot,
    Pumpkin,
    Melon,
    Flower,
    Reed,
    Mushroom,
    WhiteMushroom,
    Moss,
    Podzol,
    Farmland,
    Path,
    Obsidian,
    Lava,
    Copper,
    Iron,
    Lantern,
    Wool,
    Hay,
    Bookshelf,
    Coral,
    Seagrass,
    Mud,
}
pub const ALL_MATERIALS: &[Material] = &[
    Material::Grass,
    Material::Dirt,
    Material::Stone,
    Material::Cobblestone,
    Material::MossStone,
    Material::Sand,
    Material::RedSand,
    Material::Sandstone,
    Material::RedSandstone,
    Material::Gravel,
    Material::Clay,
    Material::Terracotta,
    Material::Granite,
    Material::Basalt,
    Material::Limestone,
    Material::Slate,
    Material::CoalOre,
    Material::IronOre,
    Material::CopperOre,
    Material::OakLog,
    Material::BirchLog,
    Material::SpruceLog,
    Material::JungleLog,
    Material::AcaciaLog,
    Material::MangroveLog,
    Material::OakLeaves,
    Material::BirchLeaves,
    Material::SpruceLeaves,
    Material::JungleLeaves,
    Material::CherryLeaves,
    Material::DryLeaves,
    Material::Planks,
    Material::DarkPlanks,
    Material::Bricks,
    Material::Glass,
    Material::Water,
    Material::Snow,
    Material::Ice,
    Material::Cactus,
    Material::Wheat,
    Material::Carrot,
    Material::Potato,
    Material::Beetroot,
    Material::Pumpkin,
    Material::Melon,
    Material::Flower,
    Material::Reed,
    Material::Mushroom,
    Material::WhiteMushroom,
    Material::Moss,
    Material::Podzol,
    Material::Farmland,
    Material::Path,
    Material::Obsidian,
    Material::Lava,
    Material::Copper,
    Material::Iron,
    Material::Lantern,
    Material::Wool,
    Material::Hay,
    Material::Bookshelf,
    Material::Coral,
    Material::Seagrass,
    Material::Mud,
];

#[derive(Clone, Copy, Debug)]
pub struct MaterialStyle {
    pub color: [u8; 3],
    pub motif: Motif,
}
impl Material {
    pub const fn style(self) -> MaterialStyle {
        match self {
            Self::Grass => MaterialStyle {
                color: [109, 153, 96],
                motif: Motif::Turf,
            },
            Self::Dirt => MaterialStyle {
                color: [139, 106, 80],
                motif: Motif::Granular,
            },
            Self::Stone => MaterialStyle {
                color: [136, 143, 147],
                motif: Motif::Rock,
            },
            Self::Cobblestone => MaterialStyle {
                color: [129, 137, 138],
                motif: Motif::Masonry,
            },
            Self::MossStone => MaterialStyle {
                color: [113, 141, 112],
                motif: Motif::Masonry,
            },
            Self::Sand => MaterialStyle {
                color: [222, 202, 146],
                motif: Motif::Granular,
            },
            Self::RedSand => MaterialStyle {
                color: [197, 132, 91],
                motif: Motif::Granular,
            },
            Self::Sandstone => MaterialStyle {
                color: [208, 186, 140],
                motif: Motif::Layered,
            },
            Self::RedSandstone => MaterialStyle {
                color: [187, 112, 80],
                motif: Motif::Layered,
            },
            Self::Gravel => MaterialStyle {
                color: [159, 155, 144],
                motif: Motif::Granular,
            },
            Self::Clay => MaterialStyle {
                color: [158, 171, 176],
                motif: Motif::Layered,
            },
            Self::Terracotta => MaterialStyle {
                color: [180, 131, 112],
                motif: Motif::Layered,
            },
            Self::Granite => MaterialStyle {
                color: [173, 134, 125],
                motif: Motif::Rock,
            },
            Self::Basalt => MaterialStyle {
                color: [98, 108, 119],
                motif: Motif::Rock,
            },
            Self::Limestone => MaterialStyle {
                color: [195, 196, 176],
                motif: Motif::Rock,
            },
            Self::Slate => MaterialStyle {
                color: [110, 126, 141],
                motif: Motif::Layered,
            },
            Self::CoalOre => MaterialStyle {
                color: [120, 128, 132],
                motif: Motif::Ore,
            },
            Self::IronOre => MaterialStyle {
                color: [159, 143, 133],
                motif: Motif::Ore,
            },
            Self::CopperOre => MaterialStyle {
                color: [134, 162, 145],
                motif: Motif::Ore,
            },
            Self::OakLog => MaterialStyle {
                color: [135, 106, 78],
                motif: Motif::Bark,
            },
            Self::BirchLog => MaterialStyle {
                color: [209, 201, 179],
                motif: Motif::Bark,
            },
            Self::SpruceLog => MaterialStyle {
                color: [103, 86, 74],
                motif: Motif::Bark,
            },
            Self::JungleLog => MaterialStyle {
                color: [151, 114, 91],
                motif: Motif::Bark,
            },
            Self::AcaciaLog => MaterialStyle {
                color: [151, 133, 113],
                motif: Motif::Bark,
            },
            Self::MangroveLog => MaterialStyle {
                color: [139, 106, 104],
                motif: Motif::Bark,
            },
            Self::OakLeaves => MaterialStyle {
                color: [119, 160, 105],
                motif: Motif::Leaves,
            },
            Self::BirchLeaves => MaterialStyle {
                color: [146, 177, 112],
                motif: Motif::Leaves,
            },
            Self::SpruceLeaves => MaterialStyle {
                color: [91, 139, 128],
                motif: Motif::Leaves,
            },
            Self::JungleLeaves => MaterialStyle {
                color: [94, 155, 111],
                motif: Motif::Leaves,
            },
            Self::CherryLeaves => MaterialStyle {
                color: [225, 163, 186],
                motif: Motif::Leaves,
            },
            Self::DryLeaves => MaterialStyle {
                color: [166, 150, 94],
                motif: Motif::Leaves,
            },
            Self::Planks => MaterialStyle {
                color: [185, 153, 110],
                motif: Motif::Boards,
            },
            Self::DarkPlanks => MaterialStyle {
                color: [124, 104, 91],
                motif: Motif::Boards,
            },
            Self::Bricks => MaterialStyle {
                color: [177, 126, 117],
                motif: Motif::Masonry,
            },
            Self::Glass => MaterialStyle {
                color: [178, 214, 218],
                motif: Motif::Glazed,
            },
            Self::Water => MaterialStyle {
                color: [122, 176, 205],
                motif: Motif::Liquid,
            },
            Self::Snow => MaterialStyle {
                color: [231, 236, 240],
                motif: Motif::Granular,
            },
            Self::Ice => MaterialStyle {
                color: [171, 209, 231],
                motif: Motif::Crystal,
            },
            Self::Cactus => MaterialStyle {
                color: [135, 169, 114],
                motif: Motif::Crop,
            },
            Self::Wheat => MaterialStyle {
                color: [216, 191, 111],
                motif: Motif::Crop,
            },
            Self::Carrot => MaterialStyle {
                color: [198, 156, 100],
                motif: Motif::Crop,
            },
            Self::Potato => MaterialStyle {
                color: [170, 169, 109],
                motif: Motif::Crop,
            },
            Self::Beetroot => MaterialStyle {
                color: [174, 107, 131],
                motif: Motif::Crop,
            },
            Self::Pumpkin => MaterialStyle {
                color: [214, 164, 107],
                motif: Motif::Layered,
            },
            Self::Melon => MaterialStyle {
                color: [155, 183, 109],
                motif: Motif::Layered,
            },
            Self::Flower => MaterialStyle {
                color: [218, 164, 185],
                motif: Motif::Crop,
            },
            Self::Reed => MaterialStyle {
                color: [162, 185, 128],
                motif: Motif::Crop,
            },
            Self::Mushroom => MaterialStyle {
                color: [199, 133, 120],
                motif: Motif::Glazed,
            },
            Self::WhiteMushroom => MaterialStyle {
                color: [219, 205, 176],
                motif: Motif::Glazed,
            },
            Self::Moss => MaterialStyle {
                color: [112, 153, 119],
                motif: Motif::Turf,
            },
            Self::Podzol => MaterialStyle {
                color: [132, 119, 88],
                motif: Motif::Turf,
            },
            Self::Farmland => MaterialStyle {
                color: [121, 100, 76],
                motif: Motif::Layered,
            },
            Self::Path => MaterialStyle {
                color: [182, 166, 128],
                motif: Motif::Granular,
            },
            Self::Obsidian => MaterialStyle {
                color: [99, 91, 120],
                motif: Motif::Crystal,
            },
            Self::Lava => MaterialStyle {
                color: [228, 153, 115],
                motif: Motif::Liquid,
            },
            Self::Copper => MaterialStyle {
                color: [175, 156, 130],
                motif: Motif::Metal,
            },
            Self::Iron => MaterialStyle {
                color: [184, 195, 197],
                motif: Motif::Metal,
            },
            Self::Lantern => MaterialStyle {
                color: [241, 214, 151],
                motif: Motif::Glazed,
            },
            Self::Wool => MaterialStyle {
                color: [214, 198, 185],
                motif: Motif::Fabric,
            },
            Self::Hay => MaterialStyle {
                color: [201, 181, 118],
                motif: Motif::Fabric,
            },
            Self::Bookshelf => MaterialStyle {
                color: [159, 127, 112],
                motif: Motif::Books,
            },
            Self::Coral => MaterialStyle {
                color: [196, 153, 176],
                motif: Motif::Crystal,
            },
            Self::Seagrass => MaterialStyle {
                color: [130, 174, 149],
                motif: Motif::Crop,
            },
            Self::Mud => MaterialStyle {
                color: [116, 111, 97],
                motif: Motif::Granular,
            },
        }
    }
    /// A seekable original 8×8 motif; callers provide orientation through `face`.
    pub fn texture(self, u: u8, v: u8, face: Face, seed: u64) -> [u8; 3] {
        let u = i32::from(u % 8);
        let v = i32::from(v % 8);
        let style = self.style();
        let hash = seed
            .wrapping_add((u as u64).wrapping_mul(0x9e3779b97f4a7c15))
            .wrapping_add((v as u64).wrapping_mul(0xbf58476d1ce4e5b9))
            .wrapping_add((self as u64).wrapping_mul(0x94d049bb133111eb));
        let grain = ((hash ^ (hash >> 29)) & 15) as i32 - 7;
        if self == Self::Bookshelf && face != Face::Top {
            // Two shelves of independently colored original book spines inside a
            // wooden frame. Horizontal shelf lines are not generic board seams.
            let is_frame = u == 0 || u == 7 || v == 0 || v == 3 || v == 7;
            let colors = [
                [195, 125, 133],
                [162, 176, 121],
                [122, 155, 198],
                [203, 177, 120],
                [161, 130, 180],
                [112, 176, 172],
            ];
            let base = if is_frame {
                [181, 149, 108]
            } else {
                colors[(u - 1) as usize]
            };
            let highlight = if !is_frame && v == 2 && u % 2 == 1 {
                14
            } else {
                grain / 3
            };
            return base.map(|channel| (channel + highlight).clamp(0, 255) as u8);
        }
        if matches!(self, Self::CoalOre | Self::IronOre | Self::CopperOre) {
            // Compact mineral inclusions embedded in a grey stone matrix. The
            // inclusion mask and two-tone copper oxidation remain seekable.
            let shifted_u = (u + (seed & 3) as i32) % 8;
            let shifted_v = (v + ((seed >> 2) & 3) as i32) % 8;
            let ore_patch = [(1, 1), (5, 2), (3, 5)].into_iter().any(|(cx, cy)| {
                (shifted_u - cx).abs() <= 1
                    && (shifted_v - cy).abs() <= 1
                    && ((shifted_u - cx).abs() + (shifted_v - cy).abs() <= 1
                        || self == Self::IronOre)
            });
            let base = if ore_patch {
                match self {
                    Self::CoalOre => [65, 69, 75],
                    Self::IronOre => [195, 156, 128],
                    Self::CopperOre if (shifted_u + shifted_v) % 3 == 0 => [100, 172, 149],
                    Self::CopperOre => [204, 145, 102],
                    _ => Self::Stone.style().color,
                }
            } else {
                Self::Stone.style().color
            };
            return base.map(|channel| (i32::from(channel) + grain / 2).clamp(0, 255) as u8);
        }
        let relief = match style.motif {
            Motif::Turf => {
                if face != Face::Top && v > 2 {
                    -27 + grain
                } else {
                    grain
                }
            }
            Motif::Granular => grain * 2,
            Motif::Rock | Motif::Ore => {
                if (u + 2 * v) % 7 == 0 {
                    -21
                } else {
                    grain
                }
            }
            Motif::Layered => {
                if v % 3 == 0 {
                    -18
                } else {
                    grain / 2
                }
            }
            Motif::Bark => {
                if face == Face::Top {
                    if (u - 3).abs().max((v - 3).abs()) % 2 == 0 {
                        9
                    } else {
                        -15
                    }
                } else if u % 3 == 0 {
                    -22
                } else {
                    grain
                }
            }
            Motif::Leaves => {
                if (u * 3 + v * 5) % 7 < 2 {
                    -19
                } else {
                    grain + 5
                }
            }
            Motif::Boards | Motif::Books => {
                if v % 4 == 0 || (u + v / 4 * 3) % 8 == 0 {
                    -22
                } else {
                    grain / 2
                }
            }
            Motif::Masonry => {
                if v % 4 == 0 || (u + v / 4 * 4) % 8 == 0 {
                    22
                } else {
                    grain
                }
            }
            Motif::Glazed => {
                if u == 0 || v == 0 {
                    -22
                } else if u == v {
                    24
                } else {
                    3
                }
            }
            Motif::Crop => {
                if u % 3 == 0 {
                    -23
                } else if v < 2 {
                    17
                } else {
                    grain
                }
            }
            Motif::Liquid => {
                if (u + v * 2) % 8 < 2 {
                    16
                } else {
                    grain / 3
                }
            }
            Motif::Crystal => {
                if (u - v).abs() < 2 {
                    25
                } else {
                    -6 + grain / 2
                }
            }
            Motif::Fabric => {
                if (u + v) % 2 == 0 {
                    6
                } else {
                    -7
                }
            }
            Motif::Metal => {
                if u == 0 || v == 7 {
                    -26
                } else {
                    grain / 3 + 7
                }
            }
        };
        let base = if matches!(self, Self::Grass | Self::Podzol | Self::Moss)
            && face != Face::Top
            && v > 2
        {
            Self::Dirt.style().color
        } else {
            style.color
        };
        base.map(|channel| (i32::from(channel) + relief).clamp(0, 255) as u8)
    }
}

/// Primitive geometry is deliberately data, not names which need a hidden switch.
/// All maxima are inclusive. Every primitive has finite, bounded local coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildOp {
    Box {
        min: [i16; 3],
        max: [i16; 3],
        material: Material,
    },
    HollowBox {
        min: [i16; 3],
        max: [i16; 3],
        material: Material,
    },
    /// Four vertical perimeter walls, without a floor or ceiling.
    WallBox {
        min: [i16; 3],
        max: [i16; 3],
        material: Material,
    },
    /// Perimeter in the two axes other than `normal_axis`; useful for a wheel.
    Ring {
        min: [i16; 3],
        max: [i16; 3],
        normal_axis: u8,
        material: Material,
    },
    RemoveBox {
        min: [i16; 3],
        max: [i16; 3],
    },
    Ellipsoid {
        center: [i16; 3],
        radii: [i16; 3],
        material: Material,
    },
    Cone {
        base: [i16; 3],
        radius: i16,
        height: i16,
        material: Material,
    },
    Roof {
        origin: [i16; 3],
        width: i16,
        depth: i16,
        height: i16,
        material: Material,
    },
    Line {
        start: [i16; 3],
        end: [i16; 3],
        material: Material,
    },
}
impl BuildOp {
    pub fn bounds(self) -> ([i16; 3], [i16; 3]) {
        match self {
            Self::Box { min, max, .. }
            | Self::WallBox { min, max, .. }
            | Self::Ring { min, max, .. }
            | Self::HollowBox { min, max, .. }
            | Self::RemoveBox { min, max } => (min, max),
            Self::Ellipsoid { center, radii, .. } => (
                std::array::from_fn(|i| center[i] - radii[i]),
                std::array::from_fn(|i| center[i] + radii[i]),
            ),
            Self::Cone {
                base,
                radius,
                height,
                ..
            } => (
                [base[0] - radius, base[1], base[2] - radius],
                [base[0] + radius, base[1] + height - 1, base[2] + radius],
            ),
            Self::Roof {
                origin,
                width,
                depth,
                height,
                ..
            } => (
                origin,
                [
                    origin[0] + width - 1,
                    origin[1] + height - 1,
                    origin[2] + depth - 1,
                ],
            ),
            Self::Line { start, end, .. } => (
                std::array::from_fn(|i| start[i].min(end[i])),
                std::array::from_fn(|i| start[i].max(end[i])),
            ),
        }
    }
    pub fn visit_blocks(self, mut put: impl FnMut(i16, i16, i16, Option<Material>)) {
        if let Self::Line {
            start,
            end,
            material,
        } = self
        {
            let steps = (0..3)
                .map(|i| (i32::from(end[i]) - i32::from(start[i])).abs())
                .max()
                .unwrap_or(0)
                .max(1);
            for step in 0..=steps {
                let point: [i16; 3] = std::array::from_fn(|i| {
                    (i32::from(start[i]) + (i32::from(end[i]) - i32::from(start[i])) * step / steps)
                        as i16
                });
                put(point[0], point[1], point[2], Some(material));
            }
            return;
        }
        let (min, max) = self.bounds();
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                for x in min[0]..=max[0] {
                    let point = [x, y, z];
                    let material = match self {
                        Self::Box { material, .. } => Some(Some(material)),
                        Self::RemoveBox { .. } => Some(None),
                        Self::WallBox { material, .. } => [0, 2]
                            .into_iter()
                            .any(|i| point[i] == min[i] || point[i] == max[i])
                            .then_some(Some(material)),
                        Self::Ring {
                            normal_axis,
                            material,
                            ..
                        } => (0..3)
                            .filter(|&i| i != usize::from(normal_axis))
                            .any(|i| point[i] == min[i] || point[i] == max[i])
                            .then_some(Some(material)),
                        Self::HollowBox { material, .. } => (0..3)
                            .any(|i| point[i] == min[i] || point[i] == max[i])
                            .then_some(Some(material)),
                        Self::Ellipsoid {
                            center,
                            radii,
                            material,
                        } => {
                            // Fixed-point normalized distance avoids platform float differences.
                            let distance: i64 = (0..3)
                                .map(|i| {
                                    let delta = i64::from(point[i] - center[i]);
                                    let radius = i64::from(radii[i].max(1));
                                    delta * delta * 65536 / (radius * radius)
                                })
                                .sum();
                            (distance <= 65536).then_some(Some(material))
                        }
                        Self::Cone {
                            base,
                            radius,
                            height,
                            material,
                        } => {
                            let remaining = i32::from(height - (y - base[1]));
                            let local_radius =
                                (i32::from(radius) * remaining / i32::from(height.max(1))).max(0);
                            let dx = i32::from(x - base[0]);
                            let dz = i32::from(z - base[2]);
                            (dx * dx + dz * dz <= local_radius * local_radius)
                                .then_some(Some(material))
                        }
                        Self::Roof {
                            origin,
                            width,
                            height,
                            material,
                            ..
                        } => {
                            let edge_distance = (x - origin[0]).min(width - 1 - (x - origin[0]));
                            let roof_y = origin[1]
                                + (edge_distance * height / (width / 2).max(1)).min(height - 1);
                            (y == roof_y).then_some(Some(material))
                        }
                        Self::Line { .. } => None,
                    };
                    if let Some(material) = material {
                        put(x, y, z, material);
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FeatureRecipe {
    pub id: FeatureId,
    pub name: &'static str,
    pub category: FeatureCategory,
    pub biomes: &'static [BiomeAffinity],
    pub placement: Placement,
    /// The authored ecological or architectural distinction, not a random variation.
    pub semantic_difference: &'static str,
    pub geometry: &'static [BuildOp],
}
impl FeatureRecipe {
    pub fn visit_blocks(&self, mut put: impl FnMut(i16, i16, i16, Option<Material>)) {
        for operation in self.geometry {
            operation.visit_blocks(&mut put);
        }
    }
    pub fn bounds(&self) -> ([i16; 3], [i16; 3]) {
        let mut lower = [i16::MAX; 3];
        let mut upper = [i16::MIN; 3];
        for operation in self.geometry {
            let (min, max) = operation.bounds();
            for axis in 0..3 {
                lower[axis] = lower[axis].min(min[axis]);
                upper[axis] = upper[axis].max(max[axis]);
            }
        }
        (lower, upper)
    }
    pub fn accepts_biome(&self, biome: BiomeAffinity) -> bool {
        self.biomes.contains(&BiomeAffinity::Any) || self.biomes.contains(&biome)
    }
}

/// 208 authored recipes; IDs follow this stable catalogue order.
pub static FEATURE_RECIPES: &[FeatureRecipe] = &[
    FeatureRecipe {
        id: FeatureId(0),
        name: "broad oak",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Single bole supports a broad irregular crown",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 5, 0],
                radii: [3, 2, 3],
                material: Material::OakLeaves,
            },
            BuildOp::Ellipsoid {
                center: [2, 5, 0],
                radii: [2, 1, 2],
                material: Material::OakLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(1),
        name: "forked oak",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Bole divides into two separated scaffold limbs",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [0, 3, 0],
                end: [-2, 6, 0],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [0, 3, 0],
                end: [2, 7, 0],
                material: Material::OakLog,
            },
            BuildOp::Ellipsoid {
                center: [-2, 6, 0],
                radii: [2, 2, 2],
                material: Material::OakLeaves,
            },
            BuildOp::Ellipsoid {
                center: [2, 7, 0],
                radii: [2, 2, 2],
                material: Material::OakLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(2),
        name: "hollow ancient oak",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Walk-through hollow trunk under an old crown",
        geometry: &[
            BuildOp::WallBox {
                min: [-1, 0, -1],
                max: [1, 6, 1],
                material: Material::OakLog,
            },
            BuildOp::RemoveBox {
                min: [0, 0, -1],
                max: [0, 2, 1],
            },
            BuildOp::Ellipsoid {
                center: [0, 7, 0],
                radii: [4, 2, 4],
                material: Material::OakLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(3),
        name: "leaning riverside willow",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Wetland],
        placement: Placement::WaterEdge,
        semantic_difference: "Leaning stem with hanging curtains of leaves",
        geometry: &[
            BuildOp::Line {
                start: [0, 0, 0],
                end: [2, 5, 0],
                material: Material::OakLog,
            },
            BuildOp::Ellipsoid {
                center: [2, 5, 0],
                radii: [3, 1, 3],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [-1, 2, 0],
                max: [-1, 4, 0],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [5, 2, 0],
                max: [5, 4, 0],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [2, 2, -3],
                max: [2, 4, -3],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [2, 2, 3],
                max: [2, 4, 3],
                material: Material::OakLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(4),
        name: "column birch",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Temperate, BiomeAffinity::Boreal],
        placement: Placement::DryGround,
        semantic_difference: "Tall white stem capped with a narrow vertical crown",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 6, 0],
                material: Material::BirchLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 7, 0],
                radii: [2, 3, 2],
                material: Material::BirchLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(5),
        name: "multi-stem birch",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Three basal stems form a low joined grove",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, 0],
                max: [-2, 4, 0],
                material: Material::BirchLog,
            },
            BuildOp::Box {
                min: [0, 0, 1],
                max: [0, 5, 1],
                material: Material::BirchLog,
            },
            BuildOp::Box {
                min: [2, 0, 0],
                max: [2, 4, 0],
                material: Material::BirchLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 6, 0],
                radii: [4, 2, 2],
                material: Material::BirchLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(6),
        name: "tiered spruce",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Boreal],
        placement: Placement::DryGround,
        semantic_difference: "Three discrete conical branch whorls",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 9, 0],
                material: Material::SpruceLog,
            },
            BuildOp::Cone {
                base: [0, 3, 0],
                radius: 3,
                height: 4,
                material: Material::SpruceLeaves,
            },
            BuildOp::Cone {
                base: [0, 6, 0],
                radius: 2,
                height: 3,
                material: Material::SpruceLeaves,
            },
            BuildOp::Cone {
                base: [0, 8, 0],
                radius: 1,
                height: 3,
                material: Material::SpruceLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(7),
        name: "snow-loaded spruce",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Frozen, BiomeAffinity::Alpine],
        placement: Placement::SnowGround,
        semantic_difference: "Conical evergreen carries projecting snow shelves",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 8, 0],
                material: Material::SpruceLog,
            },
            BuildOp::Cone {
                base: [0, 2, 0],
                radius: 3,
                height: 7,
                material: Material::SpruceLeaves,
            },
            BuildOp::Box {
                min: [-2, 4, -2],
                max: [2, 4, 2],
                material: Material::Snow,
            },
            BuildOp::Cone {
                base: [0, 5, 0],
                radius: 2,
                height: 4,
                material: Material::SpruceLeaves,
            },
            BuildOp::Box {
                min: [-1, 7, -1],
                max: [1, 7, 1],
                material: Material::Snow,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(8),
        name: "sparse larch",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Boreal, BiomeAffinity::Alpine],
        placement: Placement::DryGround,
        semantic_difference: "Open trunk with separated horizontal branch tiers",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 8, 0],
                material: Material::SpruceLog,
            },
            BuildOp::Line {
                start: [-3, 3, 0],
                end: [3, 3, 0],
                material: Material::SpruceLog,
            },
            BuildOp::Line {
                start: [0, 5, -2],
                end: [0, 5, 2],
                material: Material::SpruceLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 3, 0],
                radii: [3, 1, 1],
                material: Material::DryLeaves,
            },
            BuildOp::Ellipsoid {
                center: [0, 6, 0],
                radii: [2, 1, 2],
                material: Material::DryLeaves,
            },
            BuildOp::Ellipsoid {
                center: [0, 9, 0],
                radii: [1, 1, 1],
                material: Material::DryLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(9),
        name: "umbrella acacia",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::DryGround,
        semantic_difference: "Angled limbs support two flat horizontal canopies",
        geometry: &[
            BuildOp::Line {
                start: [0, 0, 0],
                end: [2, 5, 0],
                material: Material::AcaciaLog,
            },
            BuildOp::Line {
                start: [1, 3, 0],
                end: [-2, 4, 0],
                material: Material::AcaciaLog,
            },
            BuildOp::Ellipsoid {
                center: [2, 6, 0],
                radii: [3, 1, 3],
                material: Material::DryLeaves,
            },
            BuildOp::Ellipsoid {
                center: [-2, 5, 0],
                radii: [2, 1, 2],
                material: Material::DryLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(10),
        name: "buttressed jungle tree",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Tropical],
        placement: Placement::DryGround,
        semantic_difference: "Tall jungle tree has four basal buttress roots",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [1, 11, 1],
                material: Material::JungleLog,
            },
            BuildOp::Line {
                start: [-3, 0, 0],
                end: [0, 4, 0],
                material: Material::JungleLog,
            },
            BuildOp::Line {
                start: [4, 0, 0],
                end: [1, 4, 0],
                material: Material::JungleLog,
            },
            BuildOp::Line {
                start: [0, 0, -3],
                end: [0, 4, 0],
                material: Material::JungleLog,
            },
            BuildOp::Line {
                start: [0, 0, 4],
                end: [0, 4, 1],
                material: Material::JungleLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 12, 0],
                radii: [4, 3, 4],
                material: Material::JungleLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(11),
        name: "emergent jungle tree",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Tropical],
        placement: Placement::DryGround,
        semantic_difference: "High emergent canopy above a second mid-level crown",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [1, 14, 1],
                material: Material::JungleLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 9, 0],
                radii: [3, 2, 3],
                material: Material::JungleLeaves,
            },
            BuildOp::Ellipsoid {
                center: [0, 15, 0],
                radii: [5, 2, 5],
                material: Material::JungleLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(12),
        name: "vine-draped jungle tree",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Tropical],
        placement: Placement::DryGround,
        semantic_difference: "Canopy sends four vines down around a clear trunk",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 8, 0],
                material: Material::JungleLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 9, 0],
                radii: [3, 2, 3],
                material: Material::JungleLeaves,
            },
            BuildOp::Box {
                min: [-3, 1, 0],
                max: [-3, 8, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [3, 1, 0],
                max: [3, 8, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [0, 1, -3],
                max: [0, 8, -3],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [0, 1, 3],
                max: [0, 8, 3],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(13),
        name: "stilt-root mangrove",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Wetland],
        placement: Placement::ShallowWater,
        semantic_difference: "Canopy raised above four diagonal exposed roots",
        geometry: &[
            BuildOp::Box {
                min: [0, 3, 0],
                max: [1, 7, 1],
                material: Material::MangroveLog,
            },
            BuildOp::Line {
                start: [-3, 0, 0],
                end: [0, 4, 0],
                material: Material::MangroveLog,
            },
            BuildOp::Line {
                start: [3, 0, 0],
                end: [0, 4, 0],
                material: Material::MangroveLog,
            },
            BuildOp::Line {
                start: [0, 0, -3],
                end: [0, 4, 0],
                material: Material::MangroveLog,
            },
            BuildOp::Line {
                start: [0, 0, 3],
                end: [0, 4, 0],
                material: Material::MangroveLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 8, 0],
                radii: [3, 2, 3],
                material: Material::JungleLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(14),
        name: "propagule mangrove",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Wetland],
        placement: Placement::WaterEdge,
        semantic_difference: "Hanging propagules under an overhanging wetland canopy",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 5, 0],
                material: Material::MangroveLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 7, 0],
                radii: [4, 1, 3],
                material: Material::JungleLeaves,
            },
            BuildOp::Box {
                min: [-2, 3, 1],
                max: [-2, 5, 1],
                material: Material::MangroveLog,
            },
            BuildOp::Box {
                min: [2, 4, -1],
                max: [2, 5, -1],
                material: Material::MangroveLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(15),
        name: "fan palm",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Coastal, BiomeAffinity::Tropical],
        placement: Placement::DryGround,
        semantic_difference: "Slender palm with four straight fan fronds",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 6, 0],
                material: Material::JungleLog,
            },
            BuildOp::Line {
                start: [0, 7, 0],
                end: [-4, 7, 0],
                material: Material::JungleLeaves,
            },
            BuildOp::Line {
                start: [0, 7, 0],
                end: [4, 7, 0],
                material: Material::JungleLeaves,
            },
            BuildOp::Line {
                start: [0, 7, 0],
                end: [0, 7, -4],
                material: Material::JungleLeaves,
            },
            BuildOp::Line {
                start: [0, 7, 0],
                end: [0, 7, 4],
                material: Material::JungleLeaves,
            },
            BuildOp::Box {
                min: [1, 6, 0],
                max: [1, 6, 0],
                material: Material::Pumpkin,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(16),
        name: "bent coconut palm",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::DryGround,
        semantic_difference: "Bent palm trunk with drooping fronds and fruit",
        geometry: &[
            BuildOp::Line {
                start: [0, 0, 0],
                end: [2, 5, 0],
                material: Material::JungleLog,
            },
            BuildOp::Line {
                start: [2, 5, 0],
                end: [3, 8, 0],
                material: Material::JungleLog,
            },
            BuildOp::Line {
                start: [3, 8, 0],
                end: [-1, 6, 0],
                material: Material::JungleLeaves,
            },
            BuildOp::Line {
                start: [3, 8, 0],
                end: [7, 6, 0],
                material: Material::JungleLeaves,
            },
            BuildOp::Line {
                start: [3, 8, 0],
                end: [3, 6, -4],
                material: Material::JungleLeaves,
            },
            BuildOp::Line {
                start: [3, 8, 0],
                end: [3, 6, 4],
                material: Material::JungleLeaves,
            },
            BuildOp::Box {
                min: [3, 7, 1],
                max: [3, 7, 1],
                material: Material::Pumpkin,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(17),
        name: "cherry blossom tree",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Low fork with pink blossom crown and fallen petal carpet",
        geometry: &[
            BuildOp::Line {
                start: [0, 0, 0],
                end: [-2, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [0, 0, 0],
                end: [2, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 5, 0],
                radii: [4, 2, 3],
                material: Material::CherryLeaves,
            },
            BuildOp::Box {
                min: [-2, 0, -2],
                max: [2, 0, 2],
                material: Material::Flower,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(18),
        name: "apple orchard tree",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Rounded fruiting canopy carries visible fruit blocks",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 5, 0],
                radii: [2, 2, 2],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [-2, 4, 0],
                max: [-2, 4, 0],
                material: Material::Beetroot,
            },
            BuildOp::Box {
                min: [1, 5, 1],
                max: [1, 5, 1],
                material: Material::Beetroot,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(19),
        name: "pollarded roadside tree",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Short cut bole sprouts separate upright shoots",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [1, 2, 1],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 3, 0],
                max: [0, 5, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 3, 1],
                max: [1, 5, 1],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-1, 3, 0],
                max: [-1, 5, 0],
                material: Material::OakLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 6, 0],
                radii: [3, 1, 2],
                material: Material::OakLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(20),
        name: "wind-shaped coastal pine",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::DryGround,
        semantic_difference: "Crown and exposed limbs extend downwind",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 4, 0],
                material: Material::SpruceLog,
            },
            BuildOp::Line {
                start: [0, 4, 0],
                end: [4, 6, 0],
                material: Material::SpruceLog,
            },
            BuildOp::Ellipsoid {
                center: [3, 6, 0],
                radii: [4, 1, 2],
                material: Material::SpruceLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(21),
        name: "dead standing snag",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Badlands, BiomeAffinity::Boreal],
        placement: Placement::DryGround,
        semantic_difference: "Bare trunk with snapped branch stubs and no canopy",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 6, 0],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [0, 3, 0],
                end: [2, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [0, 5, 0],
                end: [-2, 6, 0],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(22),
        name: "fallen nurse log",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Boreal, BiomeAffinity::Wetland],
        placement: Placement::DryGround,
        semantic_difference: "Fallen log supports two young seedlings",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, 0],
                max: [3, 0, 1],
                material: Material::SpruceLog,
            },
            BuildOp::Box {
                min: [-1, 1, 0],
                max: [-1, 2, 0],
                material: Material::SpruceLog,
            },
            BuildOp::Cone {
                base: [-1, 3, 0],
                radius: 1,
                height: 2,
                material: Material::SpruceLeaves,
            },
            BuildOp::Box {
                min: [2, 1, 0],
                max: [2, 1, 0],
                material: Material::SpruceLog,
            },
            BuildOp::Cone {
                base: [2, 2, 0],
                radius: 1,
                height: 2,
                material: Material::SpruceLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(23),
        name: "burned forest trunk",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Badlands],
        placement: Placement::DryGround,
        semantic_difference: "Charred snag with ash collar and hollow burned top",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, -2],
                max: [2, 0, 2],
                material: Material::Basalt,
            },
            BuildOp::WallBox {
                min: [0, 1, 0],
                max: [2, 6, 2],
                material: Material::CoalOre,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 1],
                max: [1, 7, 1],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(24),
        name: "rock-rooted alpine pine",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::Slope,
        semantic_difference: "Roots wrap an exposed rock under a narrow high crown",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [0, 1, 0],
                radii: [2, 1, 2],
                material: Material::Stone,
            },
            BuildOp::Line {
                start: [-2, 0, 0],
                end: [0, 3, 0],
                material: Material::SpruceLog,
            },
            BuildOp::Box {
                min: [0, 3, 0],
                max: [0, 7, 0],
                material: Material::SpruceLog,
            },
            BuildOp::Cone {
                base: [0, 5, 0],
                radius: 2,
                height: 5,
                material: Material::SpruceLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(25),
        name: "bamboo grove",
        category: FeatureCategory::Trees,
        biomes: &[BiomeAffinity::Tropical],
        placement: Placement::DryGround,
        semantic_difference: "Cluster of segmented straight hollow-looking culms",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, 0],
                max: [-2, 7, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 9, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [2, 0, 1],
                max: [2, 6, 1],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [0, 0, 2],
                max: [0, 8, 2],
                material: Material::Reed,
            },
            BuildOp::Ellipsoid {
                center: [0, 8, 0],
                radii: [3, 1, 2],
                material: Material::JungleLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(26),
        name: "single saguaro",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::DryGround,
        semantic_difference: "Central ribbed cactus with raised opposite arms",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 5, 0],
                material: Material::Cactus,
            },
            BuildOp::Box {
                min: [-2, 2, 0],
                max: [-1, 2, 0],
                material: Material::Cactus,
            },
            BuildOp::Box {
                min: [-2, 2, 0],
                max: [-2, 4, 0],
                material: Material::Cactus,
            },
            BuildOp::Box {
                min: [1, 3, 0],
                max: [2, 3, 0],
                material: Material::Cactus,
            },
            BuildOp::Box {
                min: [2, 3, 0],
                max: [2, 4, 0],
                material: Material::Cactus,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(27),
        name: "barrel cactus cluster",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::DryGround,
        semantic_difference: "Low round cacti surround a bare sand gap",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [-2, 1, 0],
                radii: [1, 1, 1],
                material: Material::Cactus,
            },
            BuildOp::Ellipsoid {
                center: [2, 1, 0],
                radii: [1, 1, 1],
                material: Material::Cactus,
            },
            BuildOp::Ellipsoid {
                center: [0, 1, 2],
                radii: [1, 1, 1],
                material: Material::Cactus,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(28),
        name: "prickly pear",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::DryGround,
        semantic_difference: "Flattened paddle branches climb asymmetrically",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 1, 0],
                material: Material::Cactus,
            },
            BuildOp::Box {
                min: [-2, 1, 0],
                max: [-1, 2, 0],
                material: Material::Cactus,
            },
            BuildOp::Box {
                min: [1, 2, 0],
                max: [2, 3, 0],
                material: Material::Cactus,
            },
            BuildOp::Box {
                min: [2, 4, 0],
                max: [2, 4, 0],
                material: Material::Flower,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(29),
        name: "agave rosette",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::DryGround,
        semantic_difference: "Radial succulent leaves grow out from basal center",
        geometry: &[
            BuildOp::Line {
                start: [0, 0, 0],
                end: [-2, 1, 0],
                material: Material::DryLeaves,
            },
            BuildOp::Line {
                start: [0, 0, 0],
                end: [2, 1, 0],
                material: Material::DryLeaves,
            },
            BuildOp::Line {
                start: [0, 0, 0],
                end: [0, 1, -2],
                material: Material::DryLeaves,
            },
            BuildOp::Line {
                start: [0, 0, 0],
                end: [0, 1, 2],
                material: Material::DryLeaves,
            },
            BuildOp::Line {
                start: [0, 0, 0],
                end: [-1, 2, -1],
                material: Material::DryLeaves,
            },
            BuildOp::Line {
                start: [0, 0, 0],
                end: [1, 2, 1],
                material: Material::DryLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(30),
        name: "dry tumble shrub",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Arid, BiomeAffinity::Badlands],
        placement: Placement::DryGround,
        semantic_difference: "Open woody brush has bare branching tips",
        geometry: &[
            BuildOp::Line {
                start: [0, 0, 0],
                end: [-2, 2, 0],
                material: Material::AcaciaLog,
            },
            BuildOp::Line {
                start: [0, 0, 0],
                end: [2, 2, 0],
                material: Material::AcaciaLog,
            },
            BuildOp::Line {
                start: [0, 0, 0],
                end: [0, 3, 0],
                material: Material::AcaciaLog,
            },
            BuildOp::Line {
                start: [0, 0, 0],
                end: [0, 2, 2],
                material: Material::AcaciaLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(31),
        name: "desert flowering shrub",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::DryGround,
        semantic_difference: "Compact drought shrub with flowers only on branch ends",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [0, 1, 0],
                radii: [2, 1, 2],
                material: Material::DryLeaves,
            },
            BuildOp::Box {
                min: [-2, 1, 0],
                max: [-2, 1, 0],
                material: Material::Flower,
            },
            BuildOp::Box {
                min: [2, 1, 0],
                max: [2, 1, 0],
                material: Material::Flower,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(32),
        name: "reed bank",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Wetland],
        placement: Placement::WaterEdge,
        semantic_difference: "Parallel reeds on a mud shoreline strip",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, 0],
                max: [3, 0, 1],
                material: Material::Mud,
            },
            BuildOp::Box {
                min: [-3, 1, 0],
                max: [-3, 4, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [-1, 1, 0],
                max: [-1, 4, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [1, 1, 0],
                max: [1, 4, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [3, 1, 0],
                max: [3, 4, 0],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(33),
        name: "cattail marsh",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Wetland],
        placement: Placement::ShallowWater,
        semantic_difference: "Tall marsh stalks have brown seed heads",
        geometry: &[
            BuildOp::Box {
                min: [-1, 0, 0],
                max: [-1, 2, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [1, 0, 0],
                max: [1, 2, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [0, 0, 2],
                max: [0, 2, 2],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [-1, 3, 0],
                max: [-1, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 3, 0],
                max: [1, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 3, 2],
                max: [0, 3, 2],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(34),
        name: "lily pad pool",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Wetland],
        placement: Placement::ShallowWater,
        semantic_difference: "Floating pads surround a central bloom",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, -2],
                max: [2, 0, 2],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-1, 1, -1],
                max: [-1, 1, 0],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [1, 1, 1],
                max: [1, 1, 1],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 1, 0],
                material: Material::Flower,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(35),
        name: "duckweed mat",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Wetland],
        placement: Placement::ShallowWater,
        semantic_difference: "Broken floating green mat with open water holes",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [3, 0, 3],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-2, 1, -2],
                max: [2, 1, 2],
                material: Material::Moss,
            },
            BuildOp::RemoveBox {
                min: [0, 1, 0],
                max: [1, 1, 1],
            },
            BuildOp::RemoveBox {
                min: [-2, 1, 1],
                max: [-2, 1, 2],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(36),
        name: "fern fan",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Boreal, BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Opposite fern fronds emerge along a central rachis",
        geometry: &[
            BuildOp::Line {
                start: [0, 0, -2],
                end: [0, 1, 2],
                material: Material::Reed,
            },
            BuildOp::Line {
                start: [0, 1, -1],
                end: [-2, 0, 0],
                material: Material::OakLeaves,
            },
            BuildOp::Line {
                start: [0, 1, -1],
                end: [2, 0, 0],
                material: Material::OakLeaves,
            },
            BuildOp::Line {
                start: [0, 1, 0],
                end: [-2, 0, 1],
                material: Material::OakLeaves,
            },
            BuildOp::Line {
                start: [0, 1, 0],
                end: [2, 0, 1],
                material: Material::OakLeaves,
            },
            BuildOp::Line {
                start: [0, 1, 1],
                end: [-2, 0, 2],
                material: Material::OakLeaves,
            },
            BuildOp::Line {
                start: [0, 1, 1],
                end: [2, 0, 2],
                material: Material::OakLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(37),
        name: "bracken thicket",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Boreal],
        placement: Placement::DryGround,
        semantic_difference: "Layered fern crowns fill a low sheltered patch",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [-2, 1, 0],
                radii: [2, 1, 1],
                material: Material::OakLeaves,
            },
            BuildOp::Ellipsoid {
                center: [2, 1, 1],
                radii: [2, 1, 1],
                material: Material::OakLeaves,
            },
            BuildOp::Ellipsoid {
                center: [0, 2, -1],
                radii: [2, 1, 2],
                material: Material::OakLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(38),
        name: "heather heath",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Alpine, BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Low shrub ridges carry a row of flower tips",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, 0],
                max: [3, 0, 1],
                material: Material::DryLeaves,
            },
            BuildOp::Box {
                min: [-3, 1, 0],
                max: [-3, 1, 0],
                material: Material::Flower,
            },
            BuildOp::Box {
                min: [-1, 1, 0],
                max: [-1, 1, 0],
                material: Material::Flower,
            },
            BuildOp::Box {
                min: [1, 1, 0],
                max: [1, 1, 0],
                material: Material::Flower,
            },
            BuildOp::Box {
                min: [3, 1, 0],
                max: [3, 1, 0],
                material: Material::Flower,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(39),
        name: "berry bramble arch",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Thorny bramble arches above a small passable gap",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, 0],
                max: [-2, 2, 1],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [2, 0, 0],
                max: [2, 2, 1],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [-2, 3, 0],
                max: [2, 3, 1],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [-1, 3, 0],
                max: [-1, 3, 0],
                material: Material::Beetroot,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(40),
        name: "moss-covered boulder",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Boreal, BiomeAffinity::Wetland],
        placement: Placement::DryGround,
        semantic_difference: "Moss carpet follows the top of a rounded rock",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [0, 1, 0],
                radii: [2, 1, 2],
                material: Material::Stone,
            },
            BuildOp::Ellipsoid {
                center: [0, 2, 0],
                radii: [2, 1, 2],
                material: Material::Moss,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(41),
        name: "lichen rock crust",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Alpine, BiomeAffinity::Frozen],
        placement: Placement::DryGround,
        semantic_difference: "Sparse pale crust dots an exposed slab",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, -2],
                max: [2, 0, 2],
                material: Material::Slate,
            },
            BuildOp::Box {
                min: [-1, 1, 0],
                max: [-1, 1, 1],
                material: Material::Moss,
            },
            BuildOp::Box {
                min: [1, 1, -1],
                max: [1, 1, -1],
                material: Material::Snow,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(42),
        name: "red cap mushroom",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Temperate, BiomeAffinity::Boreal],
        placement: Placement::DryGround,
        semantic_difference: "Broad red canopy on a thin stem",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 2, 0],
                material: Material::BirchLog,
            },
            BuildOp::Ellipsoid {
                center: [0, 3, 0],
                radii: [2, 1, 2],
                material: Material::Mushroom,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(43),
        name: "giant brown mushroom",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Wetland],
        placement: Placement::DryGround,
        semantic_difference: "Flat square mushroom umbrella with exposed gills",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 4, 0],
                material: Material::BirchLog,
            },
            BuildOp::Box {
                min: [-3, 5, -3],
                max: [3, 5, 3],
                material: Material::WhiteMushroom,
            },
            BuildOp::Box {
                min: [-2, 4, -2],
                max: [2, 4, 2],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(44),
        name: "shelf fungus log",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Boreal],
        placement: Placement::DryGround,
        semantic_difference: "Stacked bracket fungi climb one face of a fallen log",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [4, 1, 1],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 1, 2],
                max: [2, 1, 2],
                material: Material::WhiteMushroom,
            },
            BuildOp::Box {
                min: [3, 2, 1],
                max: [4, 2, 2],
                material: Material::Mushroom,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(45),
        name: "fairy mushroom ring",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Mushrooms form a closed ring around grass",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, -2],
                max: [-2, 0, -2],
                material: Material::WhiteMushroom,
            },
            BuildOp::Box {
                min: [0, 0, -3],
                max: [0, 0, -3],
                material: Material::WhiteMushroom,
            },
            BuildOp::Box {
                min: [2, 0, -2],
                max: [2, 0, -2],
                material: Material::WhiteMushroom,
            },
            BuildOp::Box {
                min: [3, 0, 0],
                max: [3, 0, 0],
                material: Material::WhiteMushroom,
            },
            BuildOp::Box {
                min: [2, 0, 2],
                max: [2, 0, 2],
                material: Material::WhiteMushroom,
            },
            BuildOp::Box {
                min: [0, 0, 3],
                max: [0, 0, 3],
                material: Material::WhiteMushroom,
            },
            BuildOp::Box {
                min: [-2, 0, 2],
                max: [-2, 0, 2],
                material: Material::WhiteMushroom,
            },
            BuildOp::Box {
                min: [-3, 0, 0],
                max: [-3, 0, 0],
                material: Material::WhiteMushroom,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(46),
        name: "sunflower stand",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Tall stems terminate in directional golden disks",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, 0],
                max: [-2, 2, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 2, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [2, 0, 0],
                max: [2, 2, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [-2, 3, 0],
                max: [-2, 4, 0],
                material: Material::Wheat,
            },
            BuildOp::Box {
                min: [0, 3, 0],
                max: [0, 4, 0],
                material: Material::Wheat,
            },
            BuildOp::Box {
                min: [2, 3, 0],
                max: [2, 4, 0],
                material: Material::Wheat,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(47),
        name: "wildflower meadow",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::DryGround,
        semantic_difference: "Mixed low flowers separated by grass paths",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -1],
                max: [-3, 1, -1],
                material: Material::Flower,
            },
            BuildOp::Box {
                min: [-1, 0, 1],
                max: [-1, 1, 1],
                material: Material::Flower,
            },
            BuildOp::Box {
                min: [1, 0, -2],
                max: [1, 1, -2],
                material: Material::Flower,
            },
            BuildOp::Box {
                min: [3, 0, 0],
                max: [3, 1, 0],
                material: Material::Flower,
            },
            BuildOp::Box {
                min: [0, 0, 2],
                max: [0, 0, 2],
                material: Material::Flower,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(48),
        name: "alpine cushion plant",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::DryGround,
        semantic_difference: "Wind-flattened cushion grows inside a gravel ring",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, -2],
                max: [2, 0, 2],
                material: Material::Gravel,
            },
            BuildOp::Ellipsoid {
                center: [0, 1, 0],
                radii: [2, 1, 2],
                material: Material::Moss,
            },
            BuildOp::Box {
                min: [0, 2, 0],
                max: [0, 2, 0],
                material: Material::Flower,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(49),
        name: "kelp surface raft",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::ShallowWater,
        semantic_difference: "Long floating fronds meet a submerged anchored stem",
        geometry: &[
            BuildOp::Box {
                min: [0, -2, 0],
                max: [0, 0, 0],
                material: Material::Seagrass,
            },
            BuildOp::Line {
                start: [-3, 0, 0],
                end: [3, 0, 0],
                material: Material::Seagrass,
            },
            BuildOp::Line {
                start: [0, 0, -2],
                end: [0, 0, 2],
                material: Material::Seagrass,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(50),
        name: "coral branch garden",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::ShallowWater,
        semantic_difference: "Forked shallow-water coral among seagrass",
        geometry: &[
            BuildOp::Line {
                start: [0, 0, 0],
                end: [-2, 3, 0],
                material: Material::Coral,
            },
            BuildOp::Line {
                start: [0, 0, 0],
                end: [2, 4, 0],
                material: Material::Coral,
            },
            BuildOp::Line {
                start: [0, 1, 0],
                end: [0, 3, 2],
                material: Material::Coral,
            },
            BuildOp::Box {
                min: [-2, 0, 2],
                max: [-2, 1, 2],
                material: Material::Seagrass,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(51),
        name: "hanging cliff vines",
        category: FeatureCategory::Vegetation,
        biomes: &[BiomeAffinity::Tropical],
        placement: Placement::Cliff,
        semantic_difference: "Exposed cliff lip supports four hanging vine lengths",
        geometry: &[
            BuildOp::Box {
                min: [-3, 5, 0],
                max: [3, 5, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-3, 1, 1],
                max: [-3, 5, 1],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [-1, 3, 1],
                max: [-1, 5, 1],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [1, 2, 1],
                max: [1, 5, 1],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [3, 1, 1],
                max: [3, 5, 1],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(52),
        name: "talus fan",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::Slope,
        semantic_difference: "Loose stones fan outward below a cliff",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [0, 1, 0],
                radii: [4, 1, 3],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-1, 2, 0],
                max: [0, 2, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [2, 1, 2],
                max: [2, 1, 2],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(53),
        name: "granite tor",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Alpine, BiomeAffinity::Temperate],
        placement: Placement::Slope,
        semantic_difference: "Stacked weathered granite slabs form a tor",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, -2],
                max: [2, 1, 2],
                material: Material::Granite,
            },
            BuildOp::Box {
                min: [-1, 2, -1],
                max: [1, 3, 1],
                material: Material::Granite,
            },
            BuildOp::Box {
                min: [-2, 4, -1],
                max: [1, 4, 1],
                material: Material::Granite,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(54),
        name: "basalt columns",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Badlands, BiomeAffinity::Coastal],
        placement: Placement::Slope,
        semantic_difference: "Adjacent narrow basalt pillars form a jointed outcrop",
        geometry: &[
            BuildOp::Box {
                min: [-1, 0, 0],
                max: [-1, 4, 0],
                material: Material::Basalt,
            },
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 6, 0],
                material: Material::Basalt,
            },
            BuildOp::Box {
                min: [1, 0, 0],
                max: [1, 5, 0],
                material: Material::Basalt,
            },
            BuildOp::Box {
                min: [0, 0, 1],
                max: [0, 3, 1],
                material: Material::Basalt,
            },
            BuildOp::Box {
                min: [1, 0, 1],
                max: [1, 4, 1],
                material: Material::Basalt,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(55),
        name: "limestone arch",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Arid, BiomeAffinity::Coastal],
        placement: Placement::Slope,
        semantic_difference: "Natural stone arch leaves a walk-through opening",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -1],
                max: [4, 5, 1],
                material: Material::Limestone,
            },
            BuildOp::RemoveBox {
                min: [-2, 0, -1],
                max: [2, 3, 1],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(56),
        name: "sandstone hoodoo",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Arid, BiomeAffinity::Badlands],
        placement: Placement::Slope,
        semantic_difference: "Narrow weathered column carries an overhanging cap",
        geometry: &[
            BuildOp::Box {
                min: [-1, 0, -1],
                max: [1, 5, 1],
                material: Material::Sandstone,
            },
            BuildOp::Box {
                min: [-2, 6, -2],
                max: [2, 6, 2],
                material: Material::RedSandstone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(57),
        name: "mesa shelf",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Badlands],
        placement: Placement::Slope,
        semantic_difference: "Layered red cliff has an overhanging upper terrace",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [3, 1, 3],
                material: Material::RedSandstone,
            },
            BuildOp::Box {
                min: [-2, 2, -2],
                max: [2, 3, 2],
                material: Material::Terracotta,
            },
            BuildOp::Box {
                min: [-3, 4, -3],
                max: [3, 4, 3],
                material: Material::RedSand,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(58),
        name: "split boulder",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::Slope,
        semantic_difference: "Large rock is divided by a straight open fracture",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [0, 2, 0],
                radii: [3, 2, 3],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [0, 0, -3],
                max: [0, 4, 3],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(59),
        name: "glacial erratic",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Frozen, BiomeAffinity::Alpine],
        placement: Placement::Slope,
        semantic_difference: "Foreign granite boulder rests on scraped slate bed",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [3, 0, 3],
                material: Material::Slate,
            },
            BuildOp::Ellipsoid {
                center: [0, 2, 0],
                radii: [3, 2, 2],
                material: Material::Granite,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(60),
        name: "moraine ridge",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Frozen, BiomeAffinity::Alpine],
        placement: Placement::Slope,
        semantic_difference: "Gravel ridge incorporates uneven embedded rocks",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -1],
                max: [4, 1, 1],
                material: Material::Gravel,
            },
            BuildOp::Ellipsoid {
                center: [-2, 2, 0],
                radii: [1, 1, 1],
                material: Material::Stone,
            },
            BuildOp::Ellipsoid {
                center: [2, 2, 0],
                radii: [2, 1, 1],
                material: Material::Granite,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(61),
        name: "scree chute",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::Slope,
        semantic_difference: "Stepped gravel chute descends between stone shoulders",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [3, 0, 3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-1, 1, -3],
                max: [1, 1, -3],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-1, 2, -2],
                max: [1, 2, -2],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-1, 3, -1],
                max: [1, 3, -1],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-1, 4, 0],
                max: [1, 4, 0],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-1, 5, 1],
                max: [1, 5, 1],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-1, 6, 2],
                max: [1, 6, 2],
                material: Material::Gravel,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(62),
        name: "coal seam exposure",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Alpine, BiomeAffinity::Boreal],
        placement: Placement::Cliff,
        semantic_difference: "Dark coal seam cuts through a vertical stone wall",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, 0],
                max: [3, 4, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-3, 2, 0],
                max: [3, 2, 0],
                material: Material::CoalOre,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(63),
        name: "iron vein exposure",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::Cliff,
        semantic_difference: "Rusty vein rises diagonally across a rock face",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, 0],
                max: [3, 5, 1],
                material: Material::Stone,
            },
            BuildOp::Line {
                start: [-3, 1, 0],
                end: [3, 5, 0],
                material: Material::IronOre,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(64),
        name: "copper weathered outcrop",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Badlands],
        placement: Placement::Slope,
        semantic_difference: "Green copper vein surrounds a weathered protruding rock",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [0, 2, 0],
                radii: [3, 2, 2],
                material: Material::Stone,
            },
            BuildOp::Line {
                start: [-2, 1, -1],
                end: [2, 3, -1],
                material: Material::CopperOre,
            },
            BuildOp::Box {
                min: [0, 3, -2],
                max: [1, 3, -2],
                material: Material::CopperOre,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(65),
        name: "cave mouth",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Alpine, BiomeAffinity::Temperate],
        placement: Placement::Cliff,
        semantic_difference: "Deep dark entrance under a stone overburden",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -3],
                max: [4, 6, 3],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [-2, 0, -3],
                max: [2, 3, 2],
            },
            BuildOp::Box {
                min: [-2, 0, 3],
                max: [2, 3, 3],
                material: Material::Basalt,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(66),
        name: "double cave portal",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::Cliff,
        semantic_difference: "Two entrances separated by a natural stone pier",
        geometry: &[
            BuildOp::Box {
                min: [-5, 0, -2],
                max: [5, 5, 2],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [-4, 0, -2],
                max: [-2, 3, 1],
            },
            BuildOp::RemoveBox {
                min: [2, 0, -2],
                max: [4, 3, 1],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(67),
        name: "ravine lip",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Alpine, BiomeAffinity::Badlands],
        placement: Placement::Cliff,
        semantic_difference: "Parallel fractured cliffs surround an open surface trench",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 4, 4],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [-1, 0, -4],
                max: [1, 5, 4],
            },
            BuildOp::Box {
                min: [-4, 5, -4],
                max: [-2, 5, 4],
                material: Material::Grass,
            },
            BuildOp::Box {
                min: [2, 5, -4],
                max: [4, 5, 4],
                material: Material::Grass,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(68),
        name: "sinkhole rim",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::Slope,
        semantic_difference: "Circular depression breaks through a grassy rock shell",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [0, 1, 0],
                radii: [4, 2, 4],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-3, 3, -3],
                max: [3, 3, 3],
                material: Material::Grass,
            },
            BuildOp::RemoveBox {
                min: [-1, -1, -1],
                max: [1, 4, 1],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(69),
        name: "karst pinnacle",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Tropical],
        placement: Placement::Slope,
        semantic_difference: "Steep narrow limestone spire rises from mossy base",
        geometry: &[
            BuildOp::Cone {
                base: [0, 0, 0],
                radius: 3,
                height: 9,
                material: Material::Limestone,
            },
            BuildOp::Box {
                min: [-2, 0, -2],
                max: [2, 0, 2],
                material: Material::Moss,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(70),
        name: "sea stack",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::Slope,
        semantic_difference: "Detached wave-eroded pillar has an undercut base",
        geometry: &[
            BuildOp::Box {
                min: [-1, 0, -1],
                max: [1, 5, 1],
                material: Material::Stone,
            },
            BuildOp::Ellipsoid {
                center: [0, 6, 0],
                radii: [3, 1, 2],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(71),
        name: "sea cave",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::Cliff,
        semantic_difference: "Water reaches a carved opening at the cliff foot",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -2],
                max: [4, 5, 2],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [-2, 0, -2],
                max: [2, 2, 1],
            },
            BuildOp::Box {
                min: [-3, -1, -3],
                max: [3, -1, 2],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(72),
        name: "dune crest",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::Slope,
        semantic_difference: "Asymmetric sand ridge with a steep lee face",
        geometry: &[
            BuildOp::Cone {
                base: [0, 0, 0],
                radius: 5,
                height: 4,
                material: Material::Sand,
            },
            BuildOp::RemoveBox {
                min: [1, 0, -5],
                max: [5, 2, 5],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(73),
        name: "dune blowout",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::Slope,
        semantic_difference: "Wind-cut hollow interrupts a low sand mound",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [0, 1, 0],
                radii: [5, 2, 4],
                material: Material::Sand,
            },
            BuildOp::RemoveBox {
                min: [-1, 1, -4],
                max: [1, 3, 1],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(74),
        name: "salt pan crust",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::Slope,
        semantic_difference: "Pale salt polygon encloses a lower clay depression",
        geometry: &[
            BuildOp::WallBox {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Snow,
            },
            BuildOp::Box {
                min: [-3, -1, -3],
                max: [3, -1, 3],
                material: Material::Clay,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(75),
        name: "obsidian lava collar",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Badlands],
        placement: Placement::Slope,
        semantic_difference: "Black cooled crust rims an exposed hot center",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [3, 0, 3],
                material: Material::Obsidian,
            },
            BuildOp::Box {
                min: [-1, 1, -1],
                max: [1, 1, 1],
                material: Material::Lava,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(76),
        name: "ice pressure ridge",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Frozen],
        placement: Placement::Slope,
        semantic_difference: "Broken blue slabs project above a frozen sheet",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -2],
                max: [4, 0, 2],
                material: Material::Ice,
            },
            BuildOp::Line {
                start: [-3, 1, 0],
                end: [0, 4, 0],
                material: Material::Ice,
            },
            BuildOp::Line {
                start: [0, 4, 0],
                end: [3, 1, 0],
                material: Material::Ice,
            },
            BuildOp::Box {
                min: [-1, 4, 0],
                max: [1, 4, 0],
                material: Material::Snow,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(77),
        name: "snow cornice",
        category: FeatureCategory::Geology,
        biomes: &[BiomeAffinity::Alpine, BiomeAffinity::Frozen],
        placement: Placement::Cliff,
        semantic_difference: "Wind-carved snow shelf overhangs a dark cliff",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, 0],
                max: [3, 4, 2],
                material: Material::Slate,
            },
            BuildOp::Box {
                min: [-4, 5, -2],
                max: [4, 5, 3],
                material: Material::Snow,
            },
            BuildOp::Box {
                min: [-3, 6, -1],
                max: [3, 6, 2],
                material: Material::Snow,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(78),
        name: "spring pool",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::WaterEdge,
        semantic_difference: "Spring emerges through a stone backwall into a small pool",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [3, 0, 3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-2, 1, -2],
                max: [2, 1, 2],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-3, 1, 2],
                max: [3, 3, 2],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 2, 1],
                max: [0, 3, 1],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(79),
        name: "waterfall plunge",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::Slope,
        semantic_difference: "High falling column reaches a deep plunge basin",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -3],
                max: [4, 0, 3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-3, 1, -2],
                max: [3, 1, 2],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-2, 2, 2],
                max: [2, 7, 2],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 2, 1],
                max: [0, 7, 1],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(80),
        name: "stepped cascade",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::Slope,
        semantic_difference: "Three descending rock steps carry continuous water",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, -3],
                max: [2, 0, 3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-1, 1, -2],
                max: [1, 1, -1],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-1, 2, -1],
                max: [1, 2, 0],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-1, 3, 0],
                max: [1, 3, 1],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(81),
        name: "braided stream",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::Slope,
        semantic_difference: "Twin shallow channels split around a gravel island",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-3, 1, -4],
                max: [-2, 1, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [2, 1, -4],
                max: [3, 1, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-3, 1, -4],
                max: [3, 1, -3],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-3, 1, 3],
                max: [3, 1, 4],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(82),
        name: "meander bend",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::WaterEdge,
        semantic_difference: "L-shaped stream bends around a grassy inside bank",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Grass,
            },
            BuildOp::Box {
                min: [-4, 1, -4],
                max: [-2, 1, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-4, 1, 2],
                max: [4, 1, 4],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(83),
        name: "oxbow lake",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::WaterEdge,
        semantic_difference: "U-shaped abandoned channel encloses a grass peninsula",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Grass,
            },
            BuildOp::Box {
                min: [-4, 1, -4],
                max: [-3, 1, 3],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [3, 1, -4],
                max: [4, 1, 3],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-4, 1, 2],
                max: [4, 1, 3],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(84),
        name: "gravel ford",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::WaterEdge,
        semantic_difference: "Raised gravel crossing interrupts a shallow stream",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -4],
                max: [3, 0, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-4, 1, 0],
                max: [4, 1, 1],
                material: Material::Gravel,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(85),
        name: "river island",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::WaterEdge,
        semantic_difference: "Grassy oval island rises inside surrounding water",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Water,
            },
            BuildOp::Ellipsoid {
                center: [0, 1, 0],
                radii: [2, 1, 3],
                material: Material::Dirt,
            },
            BuildOp::Box {
                min: [-1, 2, -2],
                max: [1, 2, 2],
                material: Material::Grass,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(86),
        name: "reed lagoon",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Wetland, BiomeAffinity::Coastal],
        placement: Placement::WaterEdge,
        semantic_difference: "Sheltered lagoon edged on three sides by reed banks",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-4, 1, -4],
                max: [-4, 2, 4],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [4, 1, -4],
                max: [4, 2, 4],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [-4, 1, 4],
                max: [4, 2, 4],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(87),
        name: "mudflat channels",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Wetland, BiomeAffinity::Coastal],
        placement: Placement::WaterEdge,
        semantic_difference: "Three narrow tidal channels divide exposed mud flats",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Mud,
            },
            BuildOp::Box {
                min: [-3, 1, -4],
                max: [-3, 1, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [0, 1, -4],
                max: [0, 1, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [3, 1, -4],
                max: [3, 1, 4],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(88),
        name: "tidal rockpool",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::WaterEdge,
        semantic_difference: "Water rests inside a complete irregular rock rim",
        geometry: &[
            BuildOp::WallBox {
                min: [-3, 0, -3],
                max: [3, 1, 3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-2, 1, -2],
                max: [2, 1, 2],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [1, 2, -2],
                max: [1, 2, -2],
                material: Material::Coral,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(89),
        name: "sandbar lagoon",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::WaterEdge,
        semantic_difference: "Sand spit separates lagoon from open water",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-3, 1, 0],
                max: [3, 1, 1],
                material: Material::Sand,
            },
            BuildOp::Box {
                min: [2, 1, -3],
                max: [3, 1, 1],
                material: Material::Sand,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(90),
        name: "delta mouth",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Wetland, BiomeAffinity::Coastal],
        placement: Placement::WaterEdge,
        semantic_difference: "Stream forks into three channels at the shore",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Mud,
            },
            BuildOp::Box {
                min: [-1, 1, -4],
                max: [1, 1, -1],
                material: Material::Water,
            },
            BuildOp::Line {
                start: [0, 1, 0],
                end: [-4, 1, 4],
                material: Material::Water,
            },
            BuildOp::Line {
                start: [0, 1, 0],
                end: [0, 1, 4],
                material: Material::Water,
            },
            BuildOp::Line {
                start: [0, 1, 0],
                end: [4, 1, 4],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(91),
        name: "beaver dam pond",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Boreal],
        placement: Placement::WaterEdge,
        semantic_difference: "Log barrier holds a raised woodland pond",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -4],
                max: [3, 0, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-4, 1, 1],
                max: [4, 2, 2],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-3, 2, -3],
                max: [3, 2, 0],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(92),
        name: "marsh hummocks",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Wetland],
        placement: Placement::WaterEdge,
        semantic_difference: "Raised moss islands punctuate shallow muddy water",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Water,
            },
            BuildOp::Ellipsoid {
                center: [-2, 1, -2],
                radii: [1, 1, 1],
                material: Material::Moss,
            },
            BuildOp::Ellipsoid {
                center: [2, 1, 0],
                radii: [1, 1, 1],
                material: Material::Moss,
            },
            BuildOp::Ellipsoid {
                center: [0, 1, 3],
                radii: [1, 1, 1],
                material: Material::Moss,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(93),
        name: "peat bog pools",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Wetland, BiomeAffinity::Boreal],
        placement: Placement::WaterEdge,
        semantic_difference: "Dark peat contains four isolated pool windows",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Podzol,
            },
            BuildOp::Box {
                min: [-3, 1, -3],
                max: [-2, 1, -2],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [1, 1, -2],
                max: [2, 1, -1],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-2, 1, 1],
                max: [-1, 1, 2],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [2, 1, 2],
                max: [3, 1, 3],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(94),
        name: "hot mineral spring",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::FlatGround,
        semantic_difference: "Warm pool has terraced mineral deposition",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [3, 0, 3],
                material: Material::Limestone,
            },
            BuildOp::Box {
                min: [-2, 1, -2],
                max: [2, 1, 2],
                material: Material::Clay,
            },
            BuildOp::Box {
                min: [-1, 2, -1],
                max: [1, 2, 1],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(95),
        name: "travertine terraces",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::Slope,
        semantic_difference: "Stacked shallow mineral basins spill into one another",
        geometry: &[
            BuildOp::WallBox {
                min: [-4, 0, -3],
                max: [4, 1, 3],
                material: Material::Limestone,
            },
            BuildOp::Box {
                min: [-3, 1, -2],
                max: [3, 1, 2],
                material: Material::Water,
            },
            BuildOp::WallBox {
                min: [-2, 2, 0],
                max: [2, 3, 2],
                material: Material::Limestone,
            },
            BuildOp::Box {
                min: [-1, 3, 1],
                max: [1, 3, 1],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(96),
        name: "desert oasis",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::WaterEdge,
        semantic_difference: "Small reed-fringed pool breaks a broad sand basin",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Sand,
            },
            BuildOp::Ellipsoid {
                center: [0, 1, 0],
                radii: [2, 1, 2],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-3, 1, 0],
                max: [-3, 2, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [2, 1, 2],
                max: [2, 2, 2],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(97),
        name: "dry wadi",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::DryGround,
        semantic_difference: "Dry branching gravel channel cuts through eroded sand shoulders",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Sand,
            },
            BuildOp::Box {
                min: [-1, 1, -4],
                max: [1, 1, 4],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-4, 1, -4],
                max: [-2, 1, 4],
                material: Material::Sand,
            },
            BuildOp::Box {
                min: [2, 1, -4],
                max: [4, 1, 4],
                material: Material::Sand,
            },
            BuildOp::RemoveBox {
                min: [-1, 1, -4],
                max: [1, 1, 4],
            },
            BuildOp::RemoveBox {
                min: [-4, 1, -1],
                max: [-2, 1, 0],
            },
            BuildOp::Box {
                min: [-4, 0, -1],
                max: [-2, 0, 0],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-4, 2, 2],
                max: [-3, 2, 4],
                material: Material::Sandstone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(98),
        name: "seasonal clay puddle",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Badlands, BiomeAffinity::Arid],
        placement: Placement::WaterEdge,
        semantic_difference: "Small retained water pocket inside cracked clay",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [3, 0, 3],
                material: Material::Clay,
            },
            BuildOp::Box {
                min: [-1, 1, -1],
                max: [1, 1, 1],
                material: Material::Water,
            },
            BuildOp::Line {
                start: [-3, 1, -3],
                end: [-1, 1, -1],
                material: Material::Dirt,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(99),
        name: "frozen lake lead",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Frozen],
        placement: Placement::WaterEdge,
        semantic_difference: "Broken ice shelves border a recessed open-water crack",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 0, 4],
                material: Material::Ice,
            },
            BuildOp::Box {
                min: [-4, 1, -4],
                max: [4, 1, 4],
                material: Material::Snow,
            },
            BuildOp::Box {
                min: [-1, 1, -4],
                max: [0, 1, 4],
                material: Material::Water,
            },
            BuildOp::RemoveBox {
                min: [-1, 1, -4],
                max: [0, 1, 4],
            },
            BuildOp::Box {
                min: [-1, 0, -4],
                max: [0, 0, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-2, 2, -2],
                max: [-2, 2, 0],
                material: Material::Ice,
            },
            BuildOp::Box {
                min: [1, 2, 1],
                max: [2, 2, 2],
                material: Material::Ice,
            },
            BuildOp::RemoveBox {
                min: [1, 1, -4],
                max: [2, 1, -3],
            },
            BuildOp::Box {
                min: [1, 0, -4],
                max: [2, 0, -3],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(100),
        name: "iceberg floe",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Frozen, BiomeAffinity::Coastal],
        placement: Placement::WaterEdge,
        semantic_difference: "Stepped ice crown rises from a floating slab",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -3],
                max: [4, 0, 3],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-3, 1, -2],
                max: [3, 1, 2],
                material: Material::Ice,
            },
            BuildOp::Box {
                min: [-2, 2, -1],
                max: [2, 3, 1],
                material: Material::Ice,
            },
            BuildOp::Box {
                min: [-1, 4, 0],
                max: [1, 4, 0],
                material: Material::Snow,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(101),
        name: "glacial melt channel",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Frozen, BiomeAffinity::Alpine],
        placement: Placement::WaterEdge,
        semantic_difference: "Descending meltwater exits an undercut snow-bank portal",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -4],
                max: [3, 1, 4],
                material: Material::Snow,
            },
            BuildOp::Box {
                min: [-1, 1, -4],
                max: [1, 1, 4],
                material: Material::Water,
            },
            BuildOp::RemoveBox {
                min: [-1, 1, -4],
                max: [1, 1, 4],
            },
            BuildOp::Box {
                min: [-1, 0, -4],
                max: [1, 0, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-2, 2, 1],
                max: [2, 3, 4],
                material: Material::Snow,
            },
            BuildOp::RemoveBox {
                min: [-1, 1, 1],
                max: [1, 2, 4],
            },
            BuildOp::Box {
                min: [-1, -1, -5],
                max: [1, -1, -5],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-2, -2, -6],
                max: [2, -2, -6],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(102),
        name: "snowmelt tarn",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::FlatGround,
        semantic_difference: "Small high lake has a boulder and snow rim",
        geometry: &[
            BuildOp::WallBox {
                min: [-3, 0, -3],
                max: [3, 1, 3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-2, 1, -2],
                max: [2, 1, 2],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-3, 2, -3],
                max: [3, 2, -3],
                material: Material::Snow,
            },
            BuildOp::Ellipsoid {
                center: [3, 2, 1],
                radii: [1, 1, 1],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(103),
        name: "lavafall basin",
        category: FeatureCategory::Water,
        biomes: &[BiomeAffinity::Badlands],
        placement: Placement::WaterEdge,
        semantic_difference: "Hot fall feeds a black crusted surface basin",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [3, 0, 3],
                material: Material::Obsidian,
            },
            BuildOp::Box {
                min: [-2, 1, -2],
                max: [2, 1, 2],
                material: Material::Lava,
            },
            BuildOp::Box {
                min: [-2, 1, 2],
                max: [2, 5, 2],
                material: Material::Basalt,
            },
            BuildOp::Box {
                min: [0, 2, 1],
                max: [0, 5, 1],
                material: Material::Lava,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(104),
        name: "village cottage",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Dwelling has a covered entry porch",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [6, 4, 6],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [6, 2, 2],
                max: [6, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 9,
                depth: 9,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [0, 1, -2],
                max: [3, 1, -1],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, 2, -2],
                max: [0, 3, -2],
                material: Material::OakLog,
            },
            BuildOp::Roof {
                origin: [-1, 4, -3],
                width: 6,
                depth: 3,
                height: 2,
                material: Material::DarkPlanks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(105),
        name: "village longhouse",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Boreal],
        placement: Placement::FlatGround,
        semantic_difference: "Shared hall has paired interior benches",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 10],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [6, 4, 10],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [6, 2, 2],
                max: [6, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 9,
                depth: 13,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [1, 1, 2],
                max: [1, 1, 8],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [5, 1, 2],
                max: [5, 1, 8],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(106),
        name: "village watchtower",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Tall lookout has an open fenced observation deck",
        geometry: &[
            BuildOp::HollowBox {
                min: [0, 0, 0],
                max: [4, 8, 4],
                material: Material::Cobblestone,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [-1, 9, -1],
                max: [5, 9, 5],
                material: Material::Planks,
            },
            BuildOp::WallBox {
                min: [-1, 10, -1],
                max: [5, 10, 5],
                material: Material::OakLog,
            },
            BuildOp::Roof {
                origin: [-1, 12, -1],
                width: 7,
                depth: 7,
                height: 3,
                material: Material::DarkPlanks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(107),
        name: "village bell square",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Freestanding bell frame stands on paved plaza",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [3, 0, 3],
                material: Material::Cobblestone,
            },
            BuildOp::Box {
                min: [-2, 1, 0],
                max: [-2, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [2, 1, 0],
                max: [2, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-2, 5, 0],
                max: [2, 5, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 3, 0],
                max: [0, 4, 0],
                material: Material::Copper,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(108),
        name: "village well",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::FlatGround,
        semantic_difference: "Covered water shaft has corner supports",
        geometry: &[
            BuildOp::Box {
                min: [-2, -1, -2],
                max: [2, -1, 2],
                material: Material::Cobblestone,
            },
            BuildOp::WallBox {
                min: [-2, 0, -2],
                max: [2, 1, 2],
                material: Material::Cobblestone,
            },
            BuildOp::Box {
                min: [-1, 0, -1],
                max: [1, 0, 1],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-2, 2, 0],
                max: [-2, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [2, 2, 0],
                max: [2, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Roof {
                origin: [-3, 5, -3],
                width: 7,
                depth: 7,
                height: 2,
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(109),
        name: "village bakery",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Baker dwelling has an exterior masonry oven",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [6, 4, 6],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [6, 2, 2],
                max: [6, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 9,
                depth: 9,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::HollowBox {
                min: [7, 0, 2],
                max: [9, 2, 4],
                material: Material::Bricks,
            },
            BuildOp::RemoveBox {
                min: [7, 1, 3],
                max: [7, 1, 3],
            },
            BuildOp::Box {
                min: [8, 3, 3],
                max: [8, 5, 3],
                material: Material::Bricks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(110),
        name: "village smithy",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Open smithy porch shelters a forge and anvil",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [6, 4, 6],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [6, 2, 2],
                max: [6, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 9,
                depth: 9,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [7, 0, 0],
                max: [10, 0, 4],
                material: Material::Stone,
            },
            BuildOp::HollowBox {
                min: [8, 1, 2],
                max: [10, 3, 4],
                material: Material::Bricks,
            },
            BuildOp::Box {
                min: [8, 2, 1],
                max: [9, 2, 1],
                material: Material::Iron,
            },
            BuildOp::Box {
                min: [7, 4, 0],
                max: [10, 4, 4],
                material: Material::DarkPlanks,
            },
            BuildOp::RemoveBox {
                min: [9, 2, 2],
                max: [9, 2, 2],
            },
            BuildOp::Box {
                min: [9, 2, 3],
                max: [9, 2, 3],
                material: Material::CoalOre,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(111),
        name: "village library",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Two-story reading room contains wall bookshelves",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 7],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [6, 7, 7],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [6, 2, 2],
                max: [6, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 8, -1],
                width: 9,
                depth: 10,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [1, 1, 6],
                max: [5, 2, 6],
                material: Material::Bookshelf,
            },
            BuildOp::Box {
                min: [1, 4, 6],
                max: [5, 5, 6],
                material: Material::Bookshelf,
            },
            BuildOp::Box {
                min: [1, 4, 1],
                max: [5, 4, 5],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(112),
        name: "village apothecary",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Herb shop joins a glazed growing bay",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [6, 4, 6],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [6, 2, 2],
                max: [6, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 9,
                depth: 9,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::HollowBox {
                min: [7, 0, 1],
                max: [10, 3, 5],
                material: Material::Glass,
            },
            BuildOp::Box {
                min: [8, 1, 2],
                max: [9, 1, 4],
                material: Material::Flower,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(113),
        name: "village fisher hut",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Coastal, BiomeAffinity::Wetland],
        placement: Placement::WaterEdge,
        semantic_difference: "Raised fisher hut connects to an over-water dock",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [4, 0, 4],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [4, 4, 4],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [4, 2, 2],
                max: [4, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 7,
                depth: 7,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [1, 0, -6],
                max: [3, 0, -1],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, -2, -6],
                max: [1, -1, -6],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [3, -2, -6],
                max: [3, -1, -6],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(114),
        name: "village shepherd pen",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Shepherd dwelling adjoins an enclosed wool pen",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [4, 0, 4],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [4, 4, 4],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [4, 2, 2],
                max: [4, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 7,
                depth: 7,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::WallBox {
                min: [5, 0, 0],
                max: [11, 1, 6],
                material: Material::OakLog,
            },
            BuildOp::RemoveBox {
                min: [5, 1, 2],
                max: [5, 1, 3],
            },
            BuildOp::Box {
                min: [8, 1, 2],
                max: [9, 1, 3],
                material: Material::Wool,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(115),
        name: "village butcher stall",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Open market stall has work counter and hanging canopy",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [5, 0, 4],
                material: Material::Path,
            },
            BuildOp::Box {
                min: [0, 1, 3],
                max: [5, 1, 3],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [5, 1, 0],
                max: [5, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 5, 0],
                max: [5, 5, 4],
                material: Material::Wool,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(116),
        name: "village cartographer",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Survey office has elevated roof observation platform",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [6, 4, 6],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [6, 2, 2],
                max: [6, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 9,
                depth: 9,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [1, 7, 2],
                max: [5, 7, 4],
                material: Material::Planks,
            },
            BuildOp::WallBox {
                min: [1, 8, 2],
                max: [5, 8, 4],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [3, 8, 3],
                max: [3, 9, 3],
                material: Material::Copper,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(117),
        name: "village carpenter",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Boreal, BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Workshop includes timber stacks and cutting bench",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [7, 0, 6],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [7, 4, 6],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [7, 2, 2],
                max: [7, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 10,
                depth: 9,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [8, 0, 1],
                max: [10, 1, 2],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [8, 1, 4],
                max: [10, 1, 4],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [9, 2, 4],
                max: [9, 2, 4],
                material: Material::Iron,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(118),
        name: "village potter",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Arid, BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Clay workshop has detached firing kiln",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [6, 4, 6],
                material: Material::Terracotta,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [6, 2, 2],
                max: [6, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 9,
                depth: 9,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::HollowBox {
                min: [8, 0, 1],
                max: [10, 2, 3],
                material: Material::Bricks,
            },
            BuildOp::Box {
                min: [8, 3, 1],
                max: [10, 3, 3],
                material: Material::Bricks,
            },
            BuildOp::RemoveBox {
                min: [8, 1, 2],
                max: [8, 1, 2],
            },
            BuildOp::Box {
                min: [1, 1, 5],
                max: [4, 1, 5],
                material: Material::Clay,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(119),
        name: "village mason",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::FlatGround,
        semantic_difference: "Stoneworker shelter adjoins dressed stone stacks",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [6, 4, 6],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [6, 2, 2],
                max: [6, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 9,
                depth: 9,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [7, 0, 0],
                max: [8, 1, 1],
                material: Material::Limestone,
            },
            BuildOp::Box {
                min: [7, 0, 3],
                max: [9, 0, 4],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(120),
        name: "village inn",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Inn has two floors and an attached stable",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [8, 0, 8],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [8, 7, 8],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [8, 2, 2],
                max: [8, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 8, -1],
                width: 11,
                depth: 11,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [1, 4, 1],
                max: [7, 4, 7],
                material: Material::Planks,
            },
            BuildOp::WallBox {
                min: [9, 0, 1],
                max: [13, 2, 7],
                material: Material::OakLog,
            },
            BuildOp::Roof {
                origin: [8, 4, 0],
                width: 7,
                depth: 9,
                height: 2,
                material: Material::DarkPlanks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(121),
        name: "village chapel",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Narrow nave ends in a taller stone bell tower",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [4, 0, 9],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [4, 5, 9],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [4, 2, 2],
                max: [4, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 6, -1],
                width: 7,
                depth: 12,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::HollowBox {
                min: [1, 0, 8],
                max: [3, 9, 10],
                material: Material::Cobblestone,
            },
            BuildOp::Cone {
                base: [2, 10, 9],
                radius: 2,
                height: 3,
                material: Material::DarkPlanks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(122),
        name: "desert courtyard house",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::FlatGround,
        semantic_difference: "Flat-roofed dwelling encloses a roofless courtyard",
        geometry: &[
            BuildOp::HollowBox {
                min: [0, 0, 0],
                max: [8, 4, 8],
                material: Material::Sandstone,
            },
            BuildOp::RemoveBox {
                min: [3, 1, 0],
                max: [4, 2, 0],
            },
            BuildOp::WallBox {
                min: [2, 1, 2],
                max: [6, 4, 6],
                material: Material::Sandstone,
            },
            BuildOp::RemoveBox {
                min: [3, 4, 3],
                max: [5, 5, 5],
            },
            BuildOp::Box {
                min: [3, 0, 3],
                max: [5, 0, 5],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(123),
        name: "desert caravanserai",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::FlatGround,
        semantic_difference: "Walled travel yard has corner towers and gate",
        geometry: &[
            BuildOp::WallBox {
                min: [0, 0, 0],
                max: [12, 3, 10],
                material: Material::Sandstone,
            },
            BuildOp::RemoveBox {
                min: [5, 0, 0],
                max: [7, 2, 0],
            },
            BuildOp::Box {
                min: [0, 0, 0],
                max: [1, 5, 1],
                material: Material::Sandstone,
            },
            BuildOp::Box {
                min: [11, 0, 0],
                max: [12, 5, 1],
                material: Material::Sandstone,
            },
            BuildOp::Box {
                min: [0, 0, 9],
                max: [1, 5, 10],
                material: Material::Sandstone,
            },
            BuildOp::Box {
                min: [11, 0, 9],
                max: [12, 5, 10],
                material: Material::Sandstone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(124),
        name: "snow village igloo",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Frozen],
        placement: Placement::SnowGround,
        semantic_difference: "Snow dome has a projecting entrance tunnel",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [0, 2, 0],
                radii: [4, 3, 4],
                material: Material::Snow,
            },
            BuildOp::RemoveBox {
                min: [-2, 0, -2],
                max: [2, 3, 2],
            },
            BuildOp::HollowBox {
                min: [-1, 0, -6],
                max: [1, 2, -3],
                material: Material::Snow,
            },
            BuildOp::RemoveBox {
                min: [0, 0, -6],
                max: [0, 1, -2],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(125),
        name: "snow log cabin",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Frozen, BiomeAffinity::Boreal],
        placement: Placement::SnowGround,
        semantic_difference: "Log walls and steep roof shed snow beside chimney",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [6, 4, 6],
                material: Material::SpruceLog,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [6, 2, 2],
                max: [6, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 9,
                depth: 9,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Roof {
                origin: [-1, 7, -1],
                width: 9,
                depth: 9,
                height: 2,
                material: Material::Snow,
            },
            BuildOp::Box {
                min: [5, 1, 5],
                max: [5, 9, 5],
                material: Material::Bricks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(126),
        name: "savanna stilt home",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::FlatGround,
        semantic_difference: "Elevated acacia home leaves shaded space beneath",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 2, 0],
                material: Material::AcaciaLog,
            },
            BuildOp::Box {
                min: [4, 0, 0],
                max: [4, 2, 0],
                material: Material::AcaciaLog,
            },
            BuildOp::Box {
                min: [0, 0, 4],
                max: [0, 2, 4],
                material: Material::AcaciaLog,
            },
            BuildOp::Box {
                min: [4, 0, 4],
                max: [4, 2, 4],
                material: Material::AcaciaLog,
            },
            BuildOp::Box {
                min: [0, 3, 0],
                max: [4, 3, 4],
                material: Material::Planks,
            },
            BuildOp::HollowBox {
                min: [0, 4, 0],
                max: [4, 6, 4],
                material: Material::AcaciaLog,
            },
            BuildOp::RemoveBox {
                min: [1, 4, 0],
                max: [1, 5, 0],
            },
            BuildOp::Roof {
                origin: [-1, 7, -1],
                width: 7,
                depth: 7,
                height: 2,
                material: Material::Hay,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(127),
        name: "swamp boardwalk home",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Wetland],
        placement: Placement::WaterEdge,
        semantic_difference: "Raised mangrove home has a projecting plank walkway",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [4, 0, 4],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [4, 3, 4],
                material: Material::MangroveLog,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [4, 2, 2],
                max: [4, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 4, -1],
                width: 7,
                depth: 7,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [-1, -3, -1],
                max: [-1, -1, -1],
                material: Material::MangroveLog,
            },
            BuildOp::Box {
                min: [5, -3, 5],
                max: [5, -1, 5],
                material: Material::MangroveLog,
            },
            BuildOp::Box {
                min: [1, 0, -5],
                max: [3, 0, -1],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(128),
        name: "village meeting hall",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Broad public hall has central stage and column porch",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [10, 0, 8],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [10, 5, 8],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [10, 2, 2],
                max: [10, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 6, -1],
                width: 13,
                depth: 11,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [2, 1, 6],
                max: [8, 1, 7],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, 1, -2],
                max: [1, 4, -2],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [9, 1, -2],
                max: [9, 4, -2],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 5, -2],
                max: [10, 5, -1],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(129),
        name: "village market arcade",
        category: FeatureCategory::Settlement,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Three sheltered vendor counters flank a paved aisle",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [10, 0, 6],
                material: Material::Path,
            },
            BuildOp::Box {
                min: [1, 1, 0],
                max: [2, 1, 1],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [4, 1, 0],
                max: [5, 1, 1],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [7, 1, 0],
                max: [8, 1, 1],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [10, 1, 0],
                max: [10, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 5, 0],
                max: [10, 5, 2],
                material: Material::Wool,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(130),
        name: "irrigated wheat field",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Four grain rows flank a central irrigation channel",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [8, 0, 8],
                material: Material::Farmland,
            },
            BuildOp::Box {
                min: [4, 1, 0],
                max: [4, 1, 8],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [0, 1, 1],
                max: [0, 1, 7],
                material: Material::Wheat,
            },
            BuildOp::Box {
                min: [2, 1, 1],
                max: [2, 1, 7],
                material: Material::Wheat,
            },
            BuildOp::Box {
                min: [6, 1, 1],
                max: [6, 1, 7],
                material: Material::Wheat,
            },
            BuildOp::Box {
                min: [8, 1, 1],
                max: [8, 1, 7],
                material: Material::Wheat,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(131),
        name: "carrot beds",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Raised carrot beds have a perimeter access path",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [8, 0, 8],
                material: Material::Farmland,
            },
            BuildOp::Box {
                min: [4, 1, 0],
                max: [4, 1, 8],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [0, 1, 1],
                max: [0, 1, 7],
                material: Material::Carrot,
            },
            BuildOp::Box {
                min: [2, 1, 1],
                max: [2, 1, 7],
                material: Material::Carrot,
            },
            BuildOp::Box {
                min: [6, 1, 1],
                max: [6, 1, 7],
                material: Material::Carrot,
            },
            BuildOp::Box {
                min: [8, 1, 1],
                max: [8, 1, 7],
                material: Material::Carrot,
            },
            BuildOp::Box {
                min: [-1, 0, -1],
                max: [9, 0, -1],
                material: Material::Path,
            },
            BuildOp::Box {
                min: [-1, 0, 9],
                max: [9, 0, 9],
                material: Material::Path,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(132),
        name: "potato ridges",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Potato crops grow on elevated soil ridges",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 1, 7],
                material: Material::Farmland,
            },
            BuildOp::Box {
                min: [3, 0, 0],
                max: [3, 1, 7],
                material: Material::Farmland,
            },
            BuildOp::Box {
                min: [6, 0, 0],
                max: [6, 1, 7],
                material: Material::Farmland,
            },
            BuildOp::Box {
                min: [0, 2, 0],
                max: [0, 2, 7],
                material: Material::Potato,
            },
            BuildOp::Box {
                min: [3, 2, 0],
                max: [3, 2, 7],
                material: Material::Potato,
            },
            BuildOp::Box {
                min: [6, 2, 0],
                max: [6, 2, 7],
                material: Material::Potato,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(133),
        name: "beetroot kitchen garden",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Small beet beds surround a working compost bin",
        geometry: &[
            BuildOp::Box {
                min: [2, 0, 2],
                max: [4, 0, 4],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Farmland,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [1, 1, 6],
                material: Material::Beetroot,
            },
            BuildOp::Box {
                min: [5, 1, 0],
                max: [6, 1, 6],
                material: Material::Beetroot,
            },
            BuildOp::WallBox {
                min: [2, 1, 2],
                max: [4, 2, 4],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [3, 1, 3],
                max: [3, 1, 3],
                material: Material::Podzol,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(134),
        name: "pumpkin patch",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Separate pumpkins rest beside trailing foliage rows",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [7, 0, 7],
                material: Material::Farmland,
            },
            BuildOp::Box {
                min: [0, 1, 1],
                max: [7, 1, 1],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [0, 1, 5],
                max: [7, 1, 5],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [1, 1, 2],
                max: [1, 1, 2],
                material: Material::Pumpkin,
            },
            BuildOp::Box {
                min: [4, 1, 2],
                max: [4, 1, 2],
                material: Material::Pumpkin,
            },
            BuildOp::Box {
                min: [6, 1, 6],
                max: [6, 1, 6],
                material: Material::Pumpkin,
            },
            BuildOp::Box {
                min: [2, 1, 6],
                max: [2, 1, 6],
                material: Material::Pumpkin,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(135),
        name: "melon trellis",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Tropical],
        placement: Placement::FlatGround,
        semantic_difference: "Fruit rows climb paired trellis frames",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Farmland,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 3, 6],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [6, 1, 0],
                max: [6, 3, 6],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 4, 0],
                max: [6, 4, 6],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [1, 1, 1],
                max: [5, 1, 2],
                material: Material::Melon,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(136),
        name: "sugar cane plantation",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Tropical, BiomeAffinity::Wetland],
        placement: Placement::FlatGround,
        semantic_difference: "Tall cane rows border three water trenches",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [8, 0, 7],
                material: Material::Mud,
            },
            BuildOp::Box {
                min: [1, 1, 0],
                max: [1, 1, 7],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [4, 1, 0],
                max: [4, 1, 7],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [7, 1, 0],
                max: [7, 1, 7],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 3, 7],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [3, 1, 0],
                max: [3, 3, 7],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [6, 1, 0],
                max: [6, 3, 7],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(137),
        name: "terraced rice paddies",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Wetland, BiomeAffinity::Tropical],
        placement: Placement::FlatGround,
        semantic_difference: "Three flooded terraces hold rows of rice seedlings",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [7, 1, 2],
                material: Material::Mud,
            },
            BuildOp::Box {
                min: [0, 2, 3],
                max: [7, 3, 5],
                material: Material::Mud,
            },
            BuildOp::Box {
                min: [0, 4, 6],
                max: [7, 5, 8],
                material: Material::Mud,
            },
            BuildOp::Box {
                min: [1, 2, 1],
                max: [6, 2, 1],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [1, 4, 4],
                max: [6, 4, 4],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [1, 6, 7],
                max: [6, 6, 7],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [1, 3, 1],
                max: [1, 3, 1],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [3, 3, 1],
                max: [3, 3, 1],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [5, 3, 1],
                max: [5, 3, 1],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [1, 5, 4],
                max: [1, 5, 4],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [3, 5, 4],
                max: [3, 5, 4],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [5, 5, 4],
                max: [5, 5, 4],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [1, 7, 7],
                max: [1, 7, 7],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [3, 7, 7],
                max: [3, 7, 7],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [5, 7, 7],
                max: [5, 7, 7],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(138),
        name: "vineyard rows",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Vines climb three linear post-supported wires",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [8, 0, 8],
                material: Material::Farmland,
            },
            BuildOp::Box {
                min: [0, 1, 1],
                max: [0, 3, 1],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 1, 4],
                max: [0, 3, 4],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 1, 7],
                max: [0, 3, 7],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [8, 1, 1],
                max: [8, 3, 1],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [8, 1, 4],
                max: [8, 3, 4],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [8, 1, 7],
                max: [8, 3, 7],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 3, 1],
                max: [8, 3, 1],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [0, 3, 4],
                max: [8, 3, 4],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [0, 3, 7],
                max: [8, 3, 7],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [1, 2, 1],
                max: [7, 2, 1],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [1, 2, 4],
                max: [7, 2, 4],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [1, 2, 7],
                max: [7, 2, 7],
                material: Material::OakLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(139),
        name: "hop poles",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Climbing plants spiral around tall isolated poles",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 5, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [3, 0, 3],
                max: [3, 5, 3],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [6, 0, 0],
                max: [6, 5, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 1, 0],
                max: [1, 4, 0],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [4, 1, 3],
                max: [4, 4, 3],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [7, 1, 0],
                max: [7, 4, 0],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(140),
        name: "herb spiral",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Stone spiral raises a central herb bed",
        geometry: &[
            BuildOp::WallBox {
                min: [-3, 0, -3],
                max: [3, 0, 3],
                material: Material::Cobblestone,
            },
            BuildOp::WallBox {
                min: [-2, 1, -2],
                max: [2, 1, 2],
                material: Material::Cobblestone,
            },
            BuildOp::WallBox {
                min: [-1, 2, -1],
                max: [1, 2, 1],
                material: Material::Cobblestone,
            },
            BuildOp::Box {
                min: [0, 3, 0],
                max: [0, 3, 0],
                material: Material::Flower,
            },
            BuildOp::Box {
                min: [-2, 1, 0],
                max: [-2, 1, 1],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(141),
        name: "orchard pruning yard",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Low orchard trees stand in an ordered harvest yard",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [8, 0, 8],
                material: Material::Grass,
            },
            BuildOp::Box {
                min: [1, 1, 1],
                max: [1, 3, 1],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [6, 1, 1],
                max: [6, 3, 1],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 1, 6],
                max: [1, 3, 6],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [6, 1, 6],
                max: [6, 3, 6],
                material: Material::OakLog,
            },
            BuildOp::Ellipsoid {
                center: [1, 4, 1],
                radii: [2, 1, 2],
                material: Material::OakLeaves,
            },
            BuildOp::Ellipsoid {
                center: [6, 4, 1],
                radii: [2, 1, 2],
                material: Material::OakLeaves,
            },
            BuildOp::Ellipsoid {
                center: [1, 4, 6],
                radii: [2, 1, 2],
                material: Material::OakLeaves,
            },
            BuildOp::Ellipsoid {
                center: [6, 4, 6],
                radii: [2, 1, 2],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [4, 1, 4],
                max: [5, 1, 5],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(142),
        name: "apiary garden",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Bee houses sit beside a planted flower strip",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 1, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [3, 0, 0],
                max: [3, 1, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [6, 0, 0],
                max: [6, 1, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 2, 0],
                max: [1, 3, 1],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [3, 2, 0],
                max: [4, 3, 1],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [6, 2, 0],
                max: [7, 3, 1],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, 0, 3],
                max: [7, 0, 4],
                material: Material::Flower,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(143),
        name: "compost heaps",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::FlatGround,
        semantic_difference: "Three-stage compost bays progress from leaves to soil",
        geometry: &[
            BuildOp::Box {
                min: [0, -1, 0],
                max: [2, -1, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [3, -1, 0],
                max: [5, -1, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [6, -1, 0],
                max: [8, -1, 2],
                material: Material::Planks,
            },
            BuildOp::WallBox {
                min: [0, 0, 0],
                max: [2, 1, 2],
                material: Material::Planks,
            },
            BuildOp::WallBox {
                min: [3, 0, 0],
                max: [5, 1, 2],
                material: Material::Planks,
            },
            BuildOp::WallBox {
                min: [6, 0, 0],
                max: [8, 1, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, 1, 1],
                max: [1, 1, 1],
                material: Material::OakLeaves,
            },
            BuildOp::Box {
                min: [4, 1, 1],
                max: [4, 1, 1],
                material: Material::Podzol,
            },
            BuildOp::Box {
                min: [7, 1, 1],
                max: [7, 1, 1],
                material: Material::Dirt,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(144),
        name: "hay drying racks",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Hay is lifted on paired A-frame drying supports",
        geometry: &[
            BuildOp::Line {
                start: [0, 0, 0],
                end: [2, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [4, 0, 0],
                end: [2, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [0, 0, 5],
                end: [2, 4, 5],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [4, 0, 5],
                end: [2, 4, 5],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 2, 0],
                max: [3, 2, 5],
                material: Material::Hay,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(145),
        name: "grain silo",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Tall grain store has a pointed weatherproof cap",
        geometry: &[
            BuildOp::HollowBox {
                min: [0, 0, 0],
                max: [4, 7, 4],
                material: Material::Planks,
            },
            BuildOp::Cone {
                base: [2, 8, 2],
                radius: 3,
                height: 4,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [1, 1, -1],
                max: [2, 2, -1],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(146),
        name: "waterwheel mill",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Mill house has a square paddle wheel above a channel",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [4, 0, 6],
                material: Material::Cobblestone,
            },
            BuildOp::HollowBox {
                min: [0, 1, 0],
                max: [4, 4, 6],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [1, 1, 0],
                max: [1, 2, 0],
            },
            BuildOp::Box {
                min: [4, 2, 2],
                max: [4, 2, 3],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 7,
                depth: 9,
                height: 3,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [5, -1, 0],
                max: [7, -1, 6],
                material: Material::Water,
            },
            BuildOp::Ring {
                min: [5, 1, 1],
                max: [5, 5, 5],
                normal_axis: 0,
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [5, 3, 2],
                max: [5, 3, 4],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(147),
        name: "windmill granary",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Tall mill supports crossed diagonal wind sails",
        geometry: &[
            BuildOp::HollowBox {
                min: [0, 0, 0],
                max: [4, 8, 4],
                material: Material::Stone,
            },
            BuildOp::Cone {
                base: [2, 9, 2],
                radius: 3,
                height: 4,
                material: Material::DarkPlanks,
            },
            BuildOp::Line {
                start: [-2, 4, -1],
                end: [6, 12, -1],
                material: Material::Wool,
            },
            BuildOp::Line {
                start: [-2, 12, -1],
                end: [6, 4, -1],
                material: Material::Wool,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(148),
        name: "greenhouse nursery",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Glass enclosure shelters propagation benches",
        geometry: &[
            BuildOp::HollowBox {
                min: [0, 0, 0],
                max: [6, 3, 8],
                material: Material::Glass,
            },
            BuildOp::Roof {
                origin: [0, 4, 0],
                width: 7,
                depth: 9,
                height: 3,
                material: Material::Glass,
            },
            BuildOp::RemoveBox {
                min: [3, 1, 0],
                max: [3, 2, 0],
            },
            BuildOp::Box {
                min: [1, 1, 1],
                max: [1, 1, 7],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [5, 1, 1],
                max: [5, 1, 7],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, 2, 1],
                max: [1, 2, 7],
                material: Material::Flower,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(149),
        name: "mushroom cellar entrance",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Boreal],
        placement: Placement::FlatGround,
        semantic_difference: "Sunken growing room exposes a covered surface stair",
        geometry: &[
            BuildOp::HollowBox {
                min: [0, -3, 0],
                max: [6, 0, 6],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [1, -2, 1],
                max: [5, -2, 5],
                material: Material::WhiteMushroom,
            },
            BuildOp::Roof {
                origin: [-1, 1, -1],
                width: 9,
                depth: 9,
                height: 2,
                material: Material::DarkPlanks,
            },
            BuildOp::RemoveBox {
                min: [2, -2, 0],
                max: [3, 0, 0],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(150),
        name: "animal watering trough",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::FlatGround,
        semantic_difference: "Long timber trough contains water beside a fence",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 2],
                material: Material::Planks,
            },
            BuildOp::WallBox {
                min: [0, 0, 0],
                max: [6, 1, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, 1, 1],
                max: [5, 1, 1],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [0, 0, 4],
                max: [6, 1, 4],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(151),
        name: "chicken nesting coop",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Raised nesting house has a fenced run and ramp",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 1, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [3, 0, 0],
                max: [3, 1, 0],
                material: Material::OakLog,
            },
            BuildOp::HollowBox {
                min: [0, 2, 0],
                max: [3, 4, 3],
                material: Material::Planks,
            },
            BuildOp::Roof {
                origin: [-1, 5, -1],
                width: 6,
                depth: 6,
                height: 2,
                material: Material::DarkPlanks,
            },
            BuildOp::WallBox {
                min: [4, 0, 0],
                max: [8, 1, 4],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [3, 3, 1],
                end: [6, 0, 1],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(152),
        name: "pig mud pen",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Open gated pig enclosure has a recessed wallow and feeding trough",
        geometry: &[
            BuildOp::WallBox {
                min: [0, 0, 0],
                max: [8, 1, 6],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 0, 1],
                max: [7, 0, 5],
                material: Material::Mud,
            },
            BuildOp::WallBox {
                min: [1, 1, 3],
                max: [4, 1, 5],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [3, 0, 0],
                max: [4, 1, 0],
            },
            BuildOp::Box {
                min: [2, 0, 2],
                max: [5, 0, 3],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [1, 1, 3],
                max: [4, 1, 3],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, 1, 5],
                max: [4, 1, 5],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, 1, 4],
                max: [1, 1, 4],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [4, 1, 4],
                max: [4, 1, 4],
                material: Material::Planks,
            },
            BuildOp::RemoveBox {
                min: [2, 1, 4],
                max: [3, 1, 4],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(153),
        name: "sheep fold",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Alpine, BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Dry stone fold incorporates a hay feeding shelter",
        geometry: &[
            BuildOp::WallBox {
                min: [0, 0, 0],
                max: [8, 1, 8],
                material: Material::Cobblestone,
            },
            BuildOp::RemoveBox {
                min: [3, 0, 0],
                max: [4, 1, 0],
            },
            BuildOp::Box {
                min: [1, 1, 6],
                max: [4, 1, 7],
                material: Material::Hay,
            },
            BuildOp::Roof {
                origin: [0, 3, 5],
                width: 6,
                depth: 4,
                height: 2,
                material: Material::DarkPlanks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(154),
        name: "irrigation sluice",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::FlatGround,
        semantic_difference: "Wood gate controls a channel between stone banks",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 8],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 3, 8],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [6, 1, 0],
                max: [6, 3, 8],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [1, 1, 4],
                max: [5, 3, 4],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [3, 4, 4],
                max: [3, 5, 4],
                material: Material::Iron,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(155),
        name: "scarecrow plot",
        category: FeatureCategory::Agriculture,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Cross-armed scarecrow guards a grain planting",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Farmland,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [1, 1, 6],
                material: Material::Wheat,
            },
            BuildOp::Box {
                min: [5, 1, 0],
                max: [6, 1, 6],
                material: Material::Wheat,
            },
            BuildOp::Box {
                min: [3, 1, 3],
                max: [3, 3, 3],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 3, 3],
                max: [5, 3, 3],
                material: Material::Wool,
            },
            BuildOp::Box {
                min: [3, 4, 3],
                max: [3, 4, 3],
                material: Material::Pumpkin,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(156),
        name: "collapsed cottage",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Roofless wall shell with a rubble breach",
        geometry: &[
            BuildOp::WallBox {
                min: [0, 0, 0],
                max: [6, 3, 6],
                material: Material::MossStone,
            },
            BuildOp::RemoveBox {
                min: [0, 0, 2],
                max: [2, 3, 4],
            },
            BuildOp::Box {
                min: [-2, 0, 2],
                max: [0, 0, 4],
                material: Material::Cobblestone,
            },
            BuildOp::Box {
                min: [2, 4, 4],
                max: [4, 4, 5],
                material: Material::DarkPlanks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(157),
        name: "roofless watchtower",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::FlatGround,
        semantic_difference: "Broken tall tower has a missing upper corner",
        geometry: &[
            BuildOp::WallBox {
                min: [0, 0, 0],
                max: [4, 8, 4],
                material: Material::MossStone,
            },
            BuildOp::RemoveBox {
                min: [0, 5, 0],
                max: [2, 9, 2],
            },
            BuildOp::Box {
                min: [-1, 0, -1],
                max: [1, 0, 1],
                material: Material::Cobblestone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(158),
        name: "sunken temple stair",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Tropical],
        placement: Placement::FlatGround,
        semantic_difference: "Stairway descends into a partially buried stone portal",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [8, 2, 8],
                material: Material::MossStone,
            },
            BuildOp::RemoveBox {
                min: [3, 0, 0],
                max: [5, 3, 5],
            },
            BuildOp::Box {
                min: [3, 0, 0],
                max: [5, 0, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [3, -1, 2],
                max: [5, -1, 3],
                material: Material::Stone,
            },
            BuildOp::HollowBox {
                min: [2, 0, 5],
                max: [6, 4, 6],
                material: Material::MossStone,
            },
            BuildOp::RemoveBox {
                min: [3, 0, 5],
                max: [5, 3, 6],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(159),
        name: "desert pyramid ruin",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::FlatGround,
        semantic_difference: "Stepped pyramid contains an exposed entrance corridor",
        geometry: &[
            BuildOp::Cone {
                base: [0, 0, 0],
                radius: 6,
                height: 7,
                material: Material::Sandstone,
            },
            BuildOp::RemoveBox {
                min: [-1, 0, -6],
                max: [1, 2, 1],
            },
            BuildOp::Box {
                min: [-1, 0, 2],
                max: [1, 2, 2],
                material: Material::Basalt,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(160),
        name: "jungle altar",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Tropical],
        placement: Placement::FlatGround,
        semantic_difference: "Overgrown stepped altar carries an offering platform",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [3, 0, 3],
                material: Material::MossStone,
            },
            BuildOp::Box {
                min: [-2, 1, -2],
                max: [2, 1, 2],
                material: Material::MossStone,
            },
            BuildOp::Box {
                min: [-1, 2, -1],
                max: [1, 2, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 3, 0],
                max: [0, 3, 0],
                material: Material::Copper,
            },
            BuildOp::Box {
                min: [2, 1, -2],
                max: [2, 2, 0],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(161),
        name: "broken aqueduct",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Arid, BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Elevated channel ends at a collapsed span",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, 0],
                max: [-3, 4, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [3, 0, 0],
                max: [4, 4, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-4, 5, 0],
                max: [-1, 5, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [2, 5, 0],
                max: [4, 5, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 0, 0],
                max: [1, 0, 1],
                material: Material::Gravel,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(162),
        name: "ruined bridge pier",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::WaterEdge,
        semantic_difference: "Two worn piers survive without their central deck",
        geometry: &[
            BuildOp::Box {
                min: [-3, -2, 0],
                max: [-2, 3, 2],
                material: Material::MossStone,
            },
            BuildOp::Box {
                min: [2, -2, 0],
                max: [3, 1, 2],
                material: Material::MossStone,
            },
            BuildOp::Box {
                min: [-3, 4, 0],
                max: [-1, 4, 2],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(163),
        name: "ancient stone circle",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Upright megaliths ring an open ritual center",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -3],
                max: [-3, 2, -3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 0, -4],
                max: [0, 2, -4],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [3, 0, -3],
                max: [3, 2, -3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [4, 0, 0],
                max: [4, 2, 0],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [3, 0, 3],
                max: [3, 2, 3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 0, 4],
                max: [0, 2, 4],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-3, 0, 3],
                max: [-3, 2, 3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-4, 0, 0],
                max: [-4, 2, 0],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(164),
        name: "fallen dolmen",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Slanted fallen capstone rests on surviving supports",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, 0],
                max: [-2, 2, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [2, 0, 0],
                max: [2, 1, 1],
                material: Material::Stone,
            },
            BuildOp::Line {
                start: [-3, 3, 0],
                end: [3, 1, 0],
                material: Material::Stone,
            },
            BuildOp::Line {
                start: [-3, 3, 1],
                end: [3, 1, 1],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(165),
        name: "burial cairn entrance",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::FlatGround,
        semantic_difference: "Stone mound has a low open passage",
        geometry: &[
            BuildOp::Ellipsoid {
                center: [0, 1, 0],
                radii: [4, 2, 4],
                material: Material::Cobblestone,
            },
            BuildOp::RemoveBox {
                min: [-1, 0, -4],
                max: [1, 1, 0],
            },
            BuildOp::Box {
                min: [-1, 0, 1],
                max: [1, 1, 1],
                material: Material::Basalt,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(166),
        name: "graveyard wall",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Broken cemetery enclosure contains ordered headstones",
        geometry: &[
            BuildOp::WallBox {
                min: [0, 0, 0],
                max: [8, 1, 8],
                material: Material::MossStone,
            },
            BuildOp::RemoveBox {
                min: [3, 0, 0],
                max: [5, 1, 0],
            },
            BuildOp::Box {
                min: [2, 0, 3],
                max: [2, 1, 3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [2, 0, 6],
                max: [2, 1, 6],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [5, 0, 3],
                max: [5, 1, 3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [5, 0, 6],
                max: [5, 1, 6],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(167),
        name: "abandoned mineshaft portal",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Alpine, BiomeAffinity::Badlands],
        placement: Placement::FlatGround,
        semantic_difference: "Timber-supported dark portal has rusted approach rails",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, 0],
                max: [4, 5, 3],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [-2, 0, 0],
                max: [2, 3, 2],
            },
            BuildOp::Box {
                min: [-2, 0, 0],
                max: [-2, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [2, 0, 0],
                max: [2, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-2, 4, 0],
                max: [2, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-1, 0, -5],
                max: [-1, 0, -1],
                material: Material::Iron,
            },
            BuildOp::Box {
                min: [1, 0, -5],
                max: [1, 0, -1],
                material: Material::Iron,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(168),
        name: "mine spoil heaps",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Badlands],
        placement: Placement::FlatGround,
        semantic_difference: "Excavated waste surrounds a disused ore cart",
        geometry: &[
            BuildOp::Box {
                min: [-1, 0, -2],
                max: [1, 0, 0],
                material: Material::Iron,
            },
            BuildOp::Cone {
                base: [-3, 0, 0],
                radius: 3,
                height: 3,
                material: Material::Gravel,
            },
            BuildOp::Cone {
                base: [3, 0, 1],
                radius: 2,
                height: 4,
                material: Material::Stone,
            },
            BuildOp::WallBox {
                min: [-1, 0, -2],
                max: [1, 2, 0],
                material: Material::Iron,
            },
            BuildOp::Box {
                min: [0, 1, -1],
                max: [0, 1, -1],
                material: Material::IronOre,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(169),
        name: "abandoned quarry",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::FlatGround,
        semantic_difference: "Cut stone benches surround a deep square excavation",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [4, 3, 4],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [-2, 0, -2],
                max: [2, 4, 2],
            },
            BuildOp::RemoveBox {
                min: [-3, 2, -3],
                max: [3, 4, 3],
            },
            BuildOp::Box {
                min: [-4, 0, -4],
                max: [-3, 0, -3],
                material: Material::Limestone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(170),
        name: "shipwreck bow",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::WaterEdge,
        semantic_difference: "Curved broken wooden prow rests on beach sand",
        geometry: &[
            BuildOp::Box {
                min: [-3, 1, -2],
                max: [3, 1, 2],
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [-4, 0, -3],
                max: [4, 0, 3],
                material: Material::Sand,
            },
            BuildOp::WallBox {
                min: [-3, 1, -2],
                max: [3, 3, 2],
                material: Material::DarkPlanks,
            },
            BuildOp::RemoveBox {
                min: [-3, 2, -2],
                max: [-1, 4, 2],
            },
            BuildOp::Box {
                min: [3, 1, -1],
                max: [4, 3, 1],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 6, 0],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(171),
        name: "shipwreck mast",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::WaterEdge,
        semantic_difference: "Snapped mast and hanging sail rise from deck debris",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -2],
                max: [3, 0, 2],
                material: Material::DarkPlanks,
            },
            BuildOp::Line {
                start: [0, 1, 0],
                end: [2, 6, 0],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [-2, 5, 0],
                end: [4, 5, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 2, 0],
                max: [2, 4, 0],
                material: Material::Wool,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(172),
        name: "ruined lighthouse",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::WaterEdge,
        semantic_difference: "Broken coastal tower retains a lantern ring",
        geometry: &[
            BuildOp::WallBox {
                min: [-2, 0, -2],
                max: [2, 8, 2],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [-2, 4, -2],
                max: [-1, 9, 0],
            },
            BuildOp::Box {
                min: [-2, 9, -2],
                max: [2, 9, 2],
                material: Material::Copper,
            },
            BuildOp::Box {
                min: [0, 10, 0],
                max: [0, 10, 0],
                material: Material::Lantern,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(173),
        name: "abandoned farmstead",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Roofless farmhouse adjoins overgrown fenced beds",
        geometry: &[
            BuildOp::WallBox {
                min: [0, 0, 0],
                max: [4, 2, 4],
                material: Material::MossStone,
            },
            BuildOp::RemoveBox {
                min: [1, 0, 0],
                max: [2, 2, 0],
            },
            BuildOp::WallBox {
                min: [5, 0, 0],
                max: [9, 0, 6],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [6, 1, 1],
                max: [8, 1, 5],
                material: Material::OakLeaves,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(174),
        name: "burned barn",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Charred posts frame the remaining stone floor",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 8],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 4, 0],
                material: Material::CoalOre,
            },
            BuildOp::Box {
                min: [6, 1, 0],
                max: [6, 4, 0],
                material: Material::CoalOre,
            },
            BuildOp::Box {
                min: [0, 1, 8],
                max: [0, 4, 8],
                material: Material::CoalOre,
            },
            BuildOp::Box {
                min: [6, 1, 8],
                max: [6, 4, 8],
                material: Material::CoalOre,
            },
            BuildOp::Box {
                min: [2, 1, 3],
                max: [4, 1, 5],
                material: Material::Basalt,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(175),
        name: "ruined windmill",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Roofless mill retains only two broken sail arms",
        geometry: &[
            BuildOp::WallBox {
                min: [0, 0, 0],
                max: [4, 6, 4],
                material: Material::MossStone,
            },
            BuildOp::RemoveBox {
                min: [0, 4, 0],
                max: [1, 7, 1],
            },
            BuildOp::Line {
                start: [2, 5, -1],
                end: [5, 8, -1],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [2, 5, -1],
                end: [0, 3, -1],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(176),
        name: "snow-buried cabin",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Frozen],
        placement: Placement::SnowGround,
        semantic_difference: "Snow covers a cabin whose doorway remains visible",
        geometry: &[
            BuildOp::HollowBox {
                min: [0, 0, 0],
                max: [4, 2, 4],
                material: Material::SpruceLog,
            },
            BuildOp::Roof {
                origin: [-1, 3, -1],
                width: 7,
                depth: 7,
                height: 3,
                material: Material::Snow,
            },
            BuildOp::Box {
                min: [-2, 0, -2],
                max: [0, 1, 6],
                material: Material::Snow,
            },
            BuildOp::RemoveBox {
                min: [2, 0, 0],
                max: [2, 1, 0],
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(177),
        name: "ruined hot spring bath",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::FlatGround,
        semantic_difference: "Broken tiled walls expose an ancient bath basin",
        geometry: &[
            BuildOp::Box {
                min: [2, 0, 2],
                max: [6, 0, 4],
                material: Material::Limestone,
            },
            BuildOp::WallBox {
                min: [0, 0, 0],
                max: [8, 2, 6],
                material: Material::Bricks,
            },
            BuildOp::RemoveBox {
                min: [0, 1, 0],
                max: [3, 3, 2],
            },
            BuildOp::WallBox {
                min: [2, 0, 2],
                max: [6, 1, 4],
                material: Material::Limestone,
            },
            BuildOp::Box {
                min: [3, 1, 3],
                max: [5, 1, 3],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(178),
        name: "broken monument",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::FlatGround,
        semantic_difference: "Fallen statue blocks flank an inscribed pedestal",
        geometry: &[
            BuildOp::Box {
                min: [-2, 0, -2],
                max: [2, 0, 2],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-1, 1, -1],
                max: [1, 2, 1],
                material: Material::Limestone,
            },
            BuildOp::Box {
                min: [0, 3, 0],
                max: [0, 4, 0],
                material: Material::Limestone,
            },
            BuildOp::Box {
                min: [3, 0, 0],
                max: [5, 0, 1],
                material: Material::Limestone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(179),
        name: "abandoned rail halt",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Roofless passenger platform retains benches and rails",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [8, 0, 2],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [1, 1, 1],
                max: [3, 1, 1],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, 0, 4],
                max: [8, 0, 4],
                material: Material::Iron,
            },
            BuildOp::Box {
                min: [0, 0, 6],
                max: [8, 0, 6],
                material: Material::Iron,
            },
            BuildOp::Box {
                min: [7, 1, 0],
                max: [7, 4, 0],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(180),
        name: "collapsed fort gate",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Badlands],
        placement: Placement::FlatGround,
        semantic_difference: "Breached gatehouse has surviving flanking bastions",
        geometry: &[
            BuildOp::WallBox {
                min: [-4, 0, 0],
                max: [-2, 5, 3],
                material: Material::Stone,
            },
            BuildOp::WallBox {
                min: [2, 0, 0],
                max: [4, 4, 3],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-1, 0, 0],
                max: [1, 0, 2],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-1, 5, 0],
                max: [0, 5, 1],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(181),
        name: "overgrown courtyard fountain",
        category: FeatureCategory::Ruins,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Broken central fountain is occupied by vines",
        geometry: &[
            BuildOp::WallBox {
                min: [-3, 0, -3],
                max: [3, 1, 3],
                material: Material::MossStone,
            },
            BuildOp::RemoveBox {
                min: [-3, 1, 0],
                max: [-3, 1, 2],
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 4, 0],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-1, 3, -1],
                max: [1, 3, 1],
                material: Material::MossStone,
            },
            BuildOp::Box {
                min: [-2, 1, 1],
                max: [-2, 3, 1],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(182),
        name: "stone arch bridge",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::WaterEdge,
        semantic_difference: "Masonry bridge spans an open arch beneath its deck",
        geometry: &[
            BuildOp::Box {
                min: [-5, 0, 0],
                max: [5, 4, 2],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [-3, 0, 0],
                max: [3, 2, 2],
            },
            BuildOp::Box {
                min: [-5, 5, 0],
                max: [5, 5, 2],
                material: Material::Path,
            },
            BuildOp::Box {
                min: [-5, 6, 0],
                max: [5, 6, 0],
                material: Material::Cobblestone,
            },
            BuildOp::Box {
                min: [-5, 6, 2],
                max: [5, 6, 2],
                material: Material::Cobblestone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(183),
        name: "timber trestle bridge",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Boreal],
        placement: Placement::WaterEdge,
        semantic_difference: "Straight deck rests on three timber bents",
        geometry: &[
            BuildOp::Box {
                min: [-5, 4, 0],
                max: [5, 4, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [-4, 0, 0],
                max: [-4, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-4, 0, 2],
                max: [-4, 3, 2],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 0, 2],
                max: [0, 3, 2],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [4, 0, 0],
                max: [4, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [4, 0, 2],
                max: [4, 3, 2],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [-4, 0, 0],
                end: [0, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [0, 0, 2],
                end: [4, 4, 2],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(184),
        name: "rope suspension bridge",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Tropical],
        placement: Placement::Cliff,
        semantic_difference: "Sagging plank walkway hangs between tall anchor posts",
        geometry: &[
            BuildOp::Box {
                min: [-5, 0, 0],
                max: [-5, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-5, 0, 2],
                max: [-5, 4, 2],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [5, 0, 0],
                max: [5, 4, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [5, 0, 2],
                max: [5, 4, 2],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-5, 3, 0],
                max: [-5, 3, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [-4, 3, 0],
                max: [-4, 3, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [-3, 3, 0],
                max: [-3, 3, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [-2, 2, 0],
                max: [-2, 2, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [-1, 2, 0],
                max: [-1, 2, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, 2, 0],
                max: [0, 2, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, 2, 0],
                max: [1, 2, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [2, 2, 0],
                max: [2, 2, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [3, 3, 0],
                max: [3, 3, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [4, 3, 0],
                max: [4, 3, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [5, 3, 0],
                max: [5, 3, 2],
                material: Material::Planks,
            },
            BuildOp::Line {
                start: [-5, 5, 0],
                end: [0, 3, 0],
                material: Material::Reed,
            },
            BuildOp::Line {
                start: [-5, 5, 2],
                end: [0, 3, 2],
                material: Material::Reed,
            },
            BuildOp::Line {
                start: [0, 3, 0],
                end: [5, 5, 0],
                material: Material::Reed,
            },
            BuildOp::Line {
                start: [0, 3, 2],
                end: [5, 5, 2],
                material: Material::Reed,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(185),
        name: "marsh boardwalk",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Wetland],
        placement: Placement::FlatGround,
        semantic_difference: "Raised plank causeway crosses mud on regularly spaced legs",
        geometry: &[
            BuildOp::Box {
                min: [-5, 2, 0],
                max: [5, 2, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [-4, 0, 0],
                max: [-4, 1, 0],
                material: Material::MangroveLog,
            },
            BuildOp::Box {
                min: [-4, 0, 2],
                max: [-4, 1, 2],
                material: Material::MangroveLog,
            },
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 1, 0],
                material: Material::MangroveLog,
            },
            BuildOp::Box {
                min: [0, 0, 2],
                max: [0, 1, 2],
                material: Material::MangroveLog,
            },
            BuildOp::Box {
                min: [4, 0, 0],
                max: [4, 1, 0],
                material: Material::MangroveLog,
            },
            BuildOp::Box {
                min: [4, 0, 2],
                max: [4, 1, 2],
                material: Material::MangroveLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(186),
        name: "stone stepping crossing",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Separated stepping stones cross visible water",
        geometry: &[
            BuildOp::Box {
                min: [-5, 0, -2],
                max: [5, 0, 2],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [-4, 1, 0],
                max: [-4, 1, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [-2, 1, 0],
                max: [-2, 1, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 1, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [2, 1, 0],
                max: [2, 1, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [4, 1, 0],
                max: [4, 1, 1],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(187),
        name: "harbor pier",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::WaterEdge,
        semantic_difference: "Wide dock includes mooring posts and cargo crate",
        geometry: &[
            BuildOp::Box {
                min: [0, 2, 0],
                max: [4, 2, 10],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, -1, 0],
                max: [0, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, -1, 5],
                max: [0, 3, 5],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, -1, 10],
                max: [0, 3, 10],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [4, -1, 0],
                max: [4, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [4, -1, 5],
                max: [4, 3, 5],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [4, -1, 10],
                max: [4, 3, 10],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 3, 8],
                max: [2, 4, 9],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(188),
        name: "fishing jetty",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Coastal, BiomeAffinity::Wetland],
        placement: Placement::WaterEdge,
        semantic_difference: "Narrow pier ends in an L-shaped fishing platform",
        geometry: &[
            BuildOp::Box {
                min: [0, 1, 0],
                max: [1, 1, 8],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, 1, 7],
                max: [5, 1, 8],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, -2, 7],
                max: [0, 1, 7],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [5, -2, 7],
                max: [5, 1, 7],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(189),
        name: "river lock",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::WaterEdge,
        semantic_difference: "Parallel masonry walls contain paired timber gates",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [8, 0, 12],
                material: Material::Water,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 4, 12],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [8, 1, 0],
                max: [8, 4, 12],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [1, 1, 1],
                max: [7, 3, 1],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, 1, 11],
                max: [7, 3, 11],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(190),
        name: "canal aqueduct",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::WaterEdge,
        semantic_difference: "Water channel runs over two open stone arches",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [12, 4, 2],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [1, 0, 0],
                max: [4, 2, 2],
            },
            BuildOp::RemoveBox {
                min: [8, 0, 0],
                max: [11, 2, 2],
            },
            BuildOp::Box {
                min: [0, 5, 0],
                max: [12, 5, 2],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 6, 1],
                max: [12, 6, 1],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(191),
        name: "roadside milestone",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::FlatGround,
        semantic_difference: "Inscribed stone marker stands beside a paved strip",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, 0],
                max: [3, 0, 2],
                material: Material::Path,
            },
            BuildOp::Box {
                min: [0, 1, 3],
                max: [0, 3, 3],
                material: Material::Limestone,
            },
            BuildOp::Box {
                min: [0, 2, 2],
                max: [0, 2, 2],
                material: Material::Slate,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(192),
        name: "signpost junction",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::FlatGround,
        semantic_difference: "Two perpendicular direction signs mark crossing paths",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -1],
                max: [4, 0, 1],
                material: Material::Path,
            },
            BuildOp::Box {
                min: [-1, 0, -4],
                max: [1, 0, 4],
                material: Material::Path,
            },
            BuildOp::Box {
                min: [2, 1, 2],
                max: [2, 4, 2],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 4, 2],
                max: [4, 4, 2],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [2, 3, 1],
                max: [2, 3, 4],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(193),
        name: "lantern road arch",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Overhead crossbeam suspends light across road",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -1],
                max: [3, 0, 1],
                material: Material::Path,
            },
            BuildOp::Box {
                min: [-3, 1, 0],
                max: [-3, 5, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [3, 1, 0],
                max: [3, 5, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-3, 6, 0],
                max: [3, 6, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 5, 0],
                max: [0, 5, 0],
                material: Material::Lantern,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(194),
        name: "rail straight",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::FlatGround,
        semantic_difference: "Paired rails sit on visible transverse wooden sleepers",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -1],
                max: [-4, 0, 3],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [-2, 0, -1],
                max: [-2, 0, 3],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [0, 0, -1],
                max: [0, 0, 3],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [2, 0, -1],
                max: [2, 0, 3],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [4, 0, -1],
                max: [4, 0, 3],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [-5, 1, 0],
                max: [5, 1, 0],
                material: Material::Iron,
            },
            BuildOp::Box {
                min: [-5, 1, 2],
                max: [5, 1, 2],
                material: Material::Iron,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(195),
        name: "rail switch",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::FlatGround,
        semantic_difference: "Rail branch forks away from a straight track",
        geometry: &[
            BuildOp::Box {
                min: [-5, 0, -1],
                max: [5, 0, 3],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-5, 1, 0],
                max: [5, 1, 0],
                material: Material::Iron,
            },
            BuildOp::Box {
                min: [-5, 1, 2],
                max: [5, 1, 2],
                material: Material::Iron,
            },
            BuildOp::Line {
                start: [0, 1, 0],
                end: [5, 1, -4],
                material: Material::Iron,
            },
            BuildOp::Line {
                start: [0, 1, 2],
                end: [5, 1, -2],
                material: Material::Iron,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(196),
        name: "rail buffer stop",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::FlatGround,
        semantic_difference: "Track terminates at a braced timber buffer",
        geometry: &[
            BuildOp::Box {
                min: [-4, 0, -1],
                max: [2, 0, 3],
                material: Material::Gravel,
            },
            BuildOp::Box {
                min: [-4, 1, 0],
                max: [2, 1, 0],
                material: Material::Iron,
            },
            BuildOp::Box {
                min: [-4, 1, 2],
                max: [2, 1, 2],
                material: Material::Iron,
            },
            BuildOp::Box {
                min: [2, 2, -1],
                max: [2, 3, 3],
                material: Material::OakLog,
            },
            BuildOp::Line {
                start: [0, 1, 0],
                end: [2, 3, 0],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(197),
        name: "mountain switchback",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::Slope,
        semantic_difference: "Stone stairs turn around a retaining wall",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [1, 0, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [1, 0, 0],
                max: [1, 1, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [2, 0, 0],
                max: [2, 2, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [3, 0, 0],
                max: [3, 3, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [4, 3, 0],
                max: [6, 3, 1],
                material: Material::Path,
            },
            BuildOp::Box {
                min: [5, 3, 2],
                max: [6, 4, 3],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(198),
        name: "cliff ladder",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::Cliff,
        semantic_difference: "Runged timber access climbs a sheer rock face",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 1],
                max: [4, 7, 2],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [1, 0, 0],
                max: [1, 7, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [3, 0, 0],
                max: [3, 7, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [1, 1, 0],
                max: [3, 1, 0],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, 3, 0],
                max: [3, 3, 0],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, 5, 0],
                max: [3, 5, 0],
                material: Material::Planks,
            },
            BuildOp::Box {
                min: [1, 7, 0],
                max: [3, 7, 0],
                material: Material::Planks,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(199),
        name: "retaining terrace",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Alpine],
        placement: Placement::FlatGround,
        semantic_difference: "Stone retaining wall supports a level grass platform",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [8, 3, 1],
                material: Material::Cobblestone,
            },
            BuildOp::Box {
                min: [0, 3, 2],
                max: [8, 3, 6],
                material: Material::Dirt,
            },
            BuildOp::Box {
                min: [0, 4, 1],
                max: [8, 4, 6],
                material: Material::Grass,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(200),
        name: "palisade gate",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Boreal],
        placement: Placement::FlatGround,
        semantic_difference: "Pointed timber enclosure opens at a tall lintel gate",
        geometry: &[
            BuildOp::Box {
                min: [-5, 0, 0],
                max: [-3, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [3, 0, 0],
                max: [5, 3, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-2, 0, 0],
                max: [-2, 5, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [2, 0, 0],
                max: [2, 5, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-2, 6, 0],
                max: [2, 6, 0],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(201),
        name: "dry stone field wall",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Temperate],
        placement: Placement::FlatGround,
        semantic_difference: "Low field boundary has a stepped pedestrian stile",
        geometry: &[
            BuildOp::Box {
                min: [-5, 0, 0],
                max: [5, 1, 1],
                material: Material::Cobblestone,
            },
            BuildOp::Box {
                min: [-1, 0, -1],
                max: [-1, 0, -1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 1, -1],
                max: [0, 1, -1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [1, 0, 2],
                max: [1, 0, 2],
                material: Material::Stone,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(202),
        name: "outpost lookout",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Badlands],
        placement: Placement::FlatGround,
        semantic_difference: "Raised open platform has corner posts and a signal lamp",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 6, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [4, 0, 0],
                max: [4, 6, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 0, 4],
                max: [0, 6, 4],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [4, 0, 4],
                max: [4, 6, 4],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 5, 0],
                max: [4, 5, 4],
                material: Material::Planks,
            },
            BuildOp::Roof {
                origin: [-1, 8, -1],
                width: 7,
                depth: 7,
                height: 2,
                material: Material::DarkPlanks,
            },
            BuildOp::Box {
                min: [2, 7, 2],
                max: [2, 7, 2],
                material: Material::Lantern,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(203),
        name: "lighthouse beacon",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Coastal],
        placement: Placement::FlatGround,
        semantic_difference: "Intact coastal tower has a glazed lantern chamber",
        geometry: &[
            BuildOp::HollowBox {
                min: [0, 0, 0],
                max: [4, 9, 4],
                material: Material::Limestone,
            },
            BuildOp::RemoveBox {
                min: [2, 1, 0],
                max: [2, 2, 0],
            },
            BuildOp::HollowBox {
                min: [0, 10, 0],
                max: [4, 12, 4],
                material: Material::Glass,
            },
            BuildOp::Box {
                min: [2, 11, 2],
                max: [2, 11, 2],
                material: Material::Lantern,
            },
            BuildOp::Cone {
                base: [2, 13, 2],
                radius: 3,
                height: 3,
                material: Material::Copper,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(204),
        name: "covered caravan shelter",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Arid],
        placement: Placement::FlatGround,
        semantic_difference: "Open-sided shaded shelter surrounds a water trough",
        geometry: &[
            BuildOp::Box {
                min: [2, 0, 2],
                max: [6, 0, 4],
                material: Material::Sandstone,
            },
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 3, 0],
                material: Material::AcaciaLog,
            },
            BuildOp::Box {
                min: [8, 0, 0],
                max: [8, 3, 0],
                material: Material::AcaciaLog,
            },
            BuildOp::Box {
                min: [0, 0, 6],
                max: [0, 3, 6],
                material: Material::AcaciaLog,
            },
            BuildOp::Box {
                min: [8, 0, 6],
                max: [8, 3, 6],
                material: Material::AcaciaLog,
            },
            BuildOp::Box {
                min: [0, 4, 0],
                max: [8, 4, 6],
                material: Material::Wool,
            },
            BuildOp::WallBox {
                min: [2, 0, 2],
                max: [6, 1, 4],
                material: Material::Sandstone,
            },
            BuildOp::Box {
                min: [3, 1, 3],
                max: [5, 1, 3],
                material: Material::Water,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(205),
        name: "surface mine crane",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Badlands],
        placement: Placement::FlatGround,
        semantic_difference: "Timber gantry suspends a hook above a stone loading pad",
        geometry: &[
            BuildOp::Box {
                min: [0, 0, 0],
                max: [6, 0, 6],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 1, 0],
                max: [0, 7, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [0, 8, 0],
                max: [6, 8, 0],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [6, 4, 0],
                max: [6, 7, 0],
                material: Material::Iron,
            },
            BuildOp::Box {
                min: [5, 3, 0],
                max: [6, 3, 0],
                material: Material::Iron,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(206),
        name: "timber log raft",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Wetland, BiomeAffinity::Boreal],
        placement: Placement::WaterEdge,
        semantic_difference: "Lashed floating logs carry a raised steering pole",
        geometry: &[
            BuildOp::Box {
                min: [-3, 0, -2],
                max: [3, 0, 2],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [-2, 1, -2],
                max: [-2, 1, 2],
                material: Material::Reed,
            },
            BuildOp::Box {
                min: [2, 1, -2],
                max: [2, 1, 2],
                material: Material::Reed,
            },
            BuildOp::Line {
                start: [0, 1, 0],
                end: [2, 4, 0],
                material: Material::OakLog,
            },
        ],
    },
    FeatureRecipe {
        id: FeatureId(207),
        name: "campsite fire ring",
        category: FeatureCategory::Infrastructure,
        biomes: &[BiomeAffinity::Any],
        placement: Placement::FlatGround,
        semantic_difference: "Circular stone hearth has benches and canvas shelter",
        geometry: &[
            BuildOp::WallBox {
                min: [-1, 0, -1],
                max: [1, 0, 1],
                material: Material::Stone,
            },
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 0, 0],
                material: Material::CoalOre,
            },
            BuildOp::Box {
                min: [-3, 0, -2],
                max: [-3, 0, 2],
                material: Material::OakLog,
            },
            BuildOp::Box {
                min: [3, 0, -2],
                max: [3, 0, 2],
                material: Material::OakLog,
            },
            BuildOp::Roof {
                origin: [-2, 2, 3],
                width: 5,
                depth: 4,
                height: 3,
                material: Material::Wool,
            },
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn blocks(recipe: &FeatureRecipe) -> HashMap<[i16; 3], Material> {
        let mut result = HashMap::new();
        recipe.visit_blocks(|x, y, z, material| {
            if let Some(material) = material {
                result.insert([x, y, z], material);
            } else {
                result.remove(&[x, y, z]);
            }
        });
        result
    }

    fn normalized_shape(recipe: &FeatureRecipe) -> Vec<[i16; 3]> {
        let source: Vec<_> = blocks(recipe).into_keys().collect();
        (0..4)
            .map(|turns| {
                let mut points: Vec<_> = source
                    .iter()
                    .map(|&[x, y, z]| match turns {
                        0 => [x, y, z],
                        1 => [-z, y, x],
                        2 => [-x, y, -z],
                        _ => [z, y, -x],
                    })
                    .collect();
                let min: [i16; 3] =
                    std::array::from_fn(|axis| points.iter().map(|p| p[axis]).min().unwrap());
                for point in &mut points {
                    for axis in 0..3 {
                        point[axis] -= min[axis];
                    }
                }
                points.sort_unstable();
                points
            })
            .min()
            .unwrap()
    }

    #[test]
    fn catalogue_has_distinct_authored_geometry_and_stable_ids() {
        assert_eq!(FEATURE_RECIPES.len(), 208);
        let mut names = HashSet::new();
        let mut signatures = HashSet::new();
        let mut shapes = HashMap::new();
        let mut counts = HashMap::new();
        let mut total = 0;
        for (index, recipe) in FEATURE_RECIPES.iter().enumerate() {
            assert_eq!(recipe.id, FeatureId(index as u16));
            assert!(names.insert(recipe.name));
            let normalized = normalized_shape(recipe);
            if let Some(previous) = shapes.insert(normalized, recipe.name) {
                panic!(
                    "duplicate uncolored rotated geometry: {previous} and {}",
                    recipe.name
                );
            }
            assert!(recipe.semantic_difference.len() >= 30);
            assert!(!recipe.biomes.is_empty());
            assert!(!recipe.geometry.is_empty());
            let mut final_blocks: Vec<_> = blocks(recipe).into_iter().collect();
            final_blocks.sort_by_key(|(position, _)| *position);
            assert!(
                signatures.insert(final_blocks.clone()),
                "duplicate geometry: {}",
                recipe.name
            );
            assert!(
                final_blocks.len() >= 8,
                "empty or token geometry: {}",
                recipe.name
            );
            total += final_blocks.len();
            *counts.entry(recipe.category).or_insert(0) += 1;
        }
        assert_eq!(counts.len(), 8);
        assert!(counts.values().all(|&count| count == 26));
        assert!(total > 20_000);
    }

    #[test]
    fn all_recipe_geometry_stays_inside_declared_bounds_and_is_repeatable() {
        for recipe in FEATURE_RECIPES {
            for operation in recipe.geometry {
                if let BuildOp::WallBox { min, max, .. } = operation {
                    assert!(
                        max[0] - min[0] >= 2 && max[2] - min[2] >= 2,
                        "wall has no open interior: {}",
                        recipe.name
                    );
                }
            }
            let (min, max) = recipe.bounds();
            assert!((0..3).all(|axis| max[axis] - min[axis] <= 32));
            let mut first = Vec::new();
            recipe.visit_blocks(|x, y, z, material| {
                let position = [x, y, z];
                assert!(
                    (0..3).all(|axis| position[axis] >= min[axis] && position[axis] <= max[axis])
                );
                first.push((position, material));
            });
            let mut second = Vec::new();
            recipe.visit_blocks(|x, y, z, material| second.push(([x, y, z], material)));
            assert_eq!(first, second);
        }
    }

    #[test]
    fn carving_order_preserves_cave_passage_and_can_rebuild_a_block() {
        let cave = FEATURE_RECIPES
            .iter()
            .find(|recipe| recipe.name == "cave mouth")
            .unwrap();
        let final_blocks = blocks(cave);
        assert!(!final_blocks.contains_key(&[0, 1, -2]));
        assert_eq!(final_blocks.get(&[0, 5, 0]), Some(&Material::Stone));
        assert_eq!(final_blocks.get(&[0, 1, 3]), Some(&Material::Basalt));
        let mut rebuilt = HashMap::new();
        for operation in [
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 0, 0],
                material: Material::Stone,
            },
            BuildOp::RemoveBox {
                min: [0, 0, 0],
                max: [0, 0, 0],
            },
            BuildOp::Box {
                min: [0, 0, 0],
                max: [0, 0, 0],
                material: Material::Moss,
            },
        ] {
            operation.visit_blocks(|x, y, z, material| {
                if let Some(material) = material {
                    rebuilt.insert([x, y, z], material);
                } else {
                    rebuilt.remove(&[x, y, z]);
                }
            });
        }
        assert_eq!(rebuilt.get(&[0, 0, 0]), Some(&Material::Moss));
    }

    #[test]
    fn biome_affinity_rejects_inappropriate_surface_growth() {
        let cactus = FEATURE_RECIPES
            .iter()
            .find(|recipe| recipe.name == "single saguaro")
            .unwrap();
        assert!(cactus.accepts_biome(BiomeAffinity::Arid));
        assert!(!cactus.accepts_biome(BiomeAffinity::Frozen));
        let camp = FEATURE_RECIPES
            .iter()
            .find(|recipe| recipe.name == "campsite fire ring")
            .unwrap();
        assert!(camp.accepts_biome(BiomeAffinity::Frozen));
    }

    #[test]
    fn original_tiles_are_periodic_seekable_and_visibly_textured() {
        assert_eq!(ALL_MATERIALS.len(), 64);
        let mut material_ids = HashSet::new();
        for &material in ALL_MATERIALS {
            assert!(material_ids.insert(material));
            let mut colors = HashSet::new();
            for face in [Face::Top, Face::Left, Face::Right] {
                for u in 0..8 {
                    for v in 0..8 {
                        let pixel = material.texture(u, v, face, 42);
                        assert_eq!(pixel, material.texture(u + 8, v + 8, face, 42));
                        assert_eq!(pixel, material.texture(u, v, face, 42));
                        colors.insert(pixel);
                    }
                }
            }
            assert!(colors.len() >= 2, "flat material: {material:?}");
        }
        let grass_top = Material::Grass.texture(1, 1, Face::Top, 0);
        let grass_soil = Material::Grass.texture(1, 6, Face::Left, 0);
        assert!(grass_top[1] > grass_top[0], "turf must be green");
        assert!(
            grass_soil[0] > grass_soil[1] && grass_soil[1] > grass_soil[2],
            "lower sides must be soil brown"
        );
        let red_book = Material::Bookshelf.texture(1, 1, Face::Left, 0);
        let blue_book = Material::Bookshelf.texture(3, 1, Face::Left, 0);
        assert!(red_book[0] > red_book[2]);
        assert!(blue_book[2] > blue_book[0]);
        let shelf = Material::Bookshelf.texture(3, 3, Face::Left, 0);
        assert!(shelf[0] > shelf[1] && shelf[1] > shelf[2]);
        let coal = Material::CoalOre.texture(1, 1, Face::Top, 0);
        let matrix = Material::CoalOre.texture(7, 7, Face::Top, 0);
        assert!(
            coal.into_iter().sum::<u8>()
                < matrix.into_iter().map(u16::from).sum::<u16>().min(255) as u8
        );
        let iron = Material::IronOre.texture(1, 1, Face::Top, 0);
        assert!(iron[0] > iron[1] && iron[1] > iron[2]);
        let copper = Material::CopperOre.texture(1, 1, Face::Top, 0);
        let oxidation = Material::CopperOre.texture(1, 2, Face::Top, 0);
        assert!(copper[0] > copper[1]);
        assert!(oxidation[1] > oxidation[0]);
        assert_ne!(
            Material::Grass.texture(1, 6, Face::Top, 7),
            Material::Grass.texture(1, 6, Face::Left, 7)
        );
    }

    #[test]
    fn roof_line_and_ellipsoid_have_expected_block_space_topology() {
        let tower = FEATURE_RECIPES
            .iter()
            .find(|recipe| recipe.id == FeatureId(106))
            .unwrap();
        let tower_blocks = blocks(tower);
        assert!(
            !tower_blocks.contains_key(&[2, 10, 2]),
            "watchtower deck fence must not have a lid"
        );
        assert_eq!(tower_blocks.get(&[-1, 10, 2]), Some(&Material::OakLog));
        let pig = FEATURE_RECIPES
            .iter()
            .find(|recipe| recipe.id == FeatureId(152))
            .unwrap();
        let pig_blocks = blocks(pig);
        assert!(
            !pig_blocks.contains_key(&[3, 1, 2]),
            "pig pen must be open above its ground"
        );
        assert!(
            !pig_blocks.contains_key(&[3, 1, 0]),
            "pig pen gate must be open"
        );
        let mut fence = HashSet::new();
        BuildOp::WallBox {
            min: [0, 0, 0],
            max: [6, 1, 6],
            material: Material::OakLog,
        }
        .visit_blocks(|x, y, z, _| {
            fence.insert([x, y, z]);
        });
        assert!(!fence.contains(&[3, 0, 3]));
        assert!(!fence.contains(&[3, 1, 3]));
        assert!(fence.contains(&[0, 1, 3]));
        let mut wheel = HashSet::new();
        BuildOp::Ring {
            min: [0, 0, 0],
            max: [0, 4, 4],
            normal_axis: 0,
            material: Material::OakLog,
        }
        .visit_blocks(|x, y, z, _| {
            wheel.insert([x, y, z]);
        });
        assert_eq!(wheel.len(), 16);
        assert!(!wheel.contains(&[0, 2, 2]));
        let mut line_blocks = HashSet::new();
        BuildOp::Line {
            start: [-3, 0, 0],
            end: [3, 6, 0],
            material: Material::OakLog,
        }
        .visit_blocks(|x, y, z, _| {
            line_blocks.insert([x, y, z]);
        });
        assert_eq!(line_blocks.len(), 7);
        assert!(line_blocks.contains(&[-3, 0, 0]));
        assert!(line_blocks.contains(&[3, 6, 0]));
        let mut roof_blocks = HashSet::new();
        BuildOp::Roof {
            origin: [0, 0, 0],
            width: 7,
            depth: 5,
            height: 3,
            material: Material::Planks,
        }
        .visit_blocks(|x, y, z, _| {
            roof_blocks.insert([x, y, z]);
        });
        assert_eq!(roof_blocks.len(), 35);
        assert!(roof_blocks.contains(&[3, 2, 2]));
        assert!(roof_blocks.contains(&[0, 0, 2]));
        let mut round_blocks = HashSet::new();
        BuildOp::Ellipsoid {
            center: [0, 0, 0],
            radii: [2, 2, 2],
            material: Material::Stone,
        }
        .visit_blocks(|x, y, z, _| {
            round_blocks.insert([x, y, z]);
        });
        assert!(round_blocks.contains(&[0, 0, 0]));
        assert!(round_blocks.contains(&[2, 0, 0]));
        assert!(!round_blocks.contains(&[2, 2, 2]));
    }
}

impl Material {
    /// Stable identities for original procedural materials. These names denote
    /// the generator's vocabulary, not vanilla Minecraft block-state aliases.
    pub const fn generated_state_name(self) -> &'static str {
        match self {
            Self::Grass => "ilium:generated/grass",
            Self::Dirt => "ilium:generated/dirt",
            Self::Stone => "ilium:generated/stone",
            Self::Cobblestone => "ilium:generated/cobblestone",
            Self::MossStone => "ilium:generated/moss_stone",
            Self::Sand => "ilium:generated/sand",
            Self::RedSand => "ilium:generated/red_sand",
            Self::Sandstone => "ilium:generated/sandstone",
            Self::RedSandstone => "ilium:generated/red_sandstone",
            Self::Gravel => "ilium:generated/gravel",
            Self::Clay => "ilium:generated/clay",
            Self::Terracotta => "ilium:generated/terracotta",
            Self::Granite => "ilium:generated/granite",
            Self::Basalt => "ilium:generated/basalt",
            Self::Limestone => "ilium:generated/limestone",
            Self::Slate => "ilium:generated/slate",
            Self::CoalOre => "ilium:generated/coal_ore",
            Self::IronOre => "ilium:generated/iron_ore",
            Self::CopperOre => "ilium:generated/copper_ore",
            Self::OakLog => "ilium:generated/oak_log",
            Self::BirchLog => "ilium:generated/birch_log",
            Self::SpruceLog => "ilium:generated/spruce_log",
            Self::JungleLog => "ilium:generated/jungle_log",
            Self::AcaciaLog => "ilium:generated/acacia_log",
            Self::MangroveLog => "ilium:generated/mangrove_log",
            Self::OakLeaves => "ilium:generated/oak_leaves",
            Self::BirchLeaves => "ilium:generated/birch_leaves",
            Self::SpruceLeaves => "ilium:generated/spruce_leaves",
            Self::JungleLeaves => "ilium:generated/jungle_leaves",
            Self::CherryLeaves => "ilium:generated/cherry_leaves",
            Self::DryLeaves => "ilium:generated/dry_leaves",
            Self::Planks => "ilium:generated/planks",
            Self::DarkPlanks => "ilium:generated/dark_planks",
            Self::Bricks => "ilium:generated/bricks",
            Self::Glass => "ilium:generated/glass",
            Self::Water => "ilium:generated/water",
            Self::Snow => "ilium:generated/snow",
            Self::Ice => "ilium:generated/ice",
            Self::Cactus => "ilium:generated/cactus",
            Self::Wheat => "ilium:generated/wheat",
            Self::Carrot => "ilium:generated/carrot",
            Self::Potato => "ilium:generated/potato",
            Self::Beetroot => "ilium:generated/beetroot",
            Self::Pumpkin => "ilium:generated/pumpkin",
            Self::Melon => "ilium:generated/melon",
            Self::Flower => "ilium:generated/flower",
            Self::Reed => "ilium:generated/reed",
            Self::Mushroom => "ilium:generated/mushroom",
            Self::WhiteMushroom => "ilium:generated/white_mushroom",
            Self::Moss => "ilium:generated/moss",
            Self::Podzol => "ilium:generated/podzol",
            Self::Farmland => "ilium:generated/farmland",
            Self::Path => "ilium:generated/path",
            Self::Obsidian => "ilium:generated/obsidian",
            Self::Lava => "ilium:generated/lava",
            Self::Copper => "ilium:generated/copper",
            Self::Iron => "ilium:generated/iron",
            Self::Lantern => "ilium:generated/lantern",
            Self::Wool => "ilium:generated/wool",
            Self::Hay => "ilium:generated/hay",
            Self::Bookshelf => "ilium:generated/bookshelf",
            Self::Coral => "ilium:generated/coral",
            Self::Seagrass => "ilium:generated/seagrass",
            Self::Mud => "ilium:generated/mud",
        }
    }
}

#[cfg(test)]
mod generated_material_identity_contract {
    use super::{Material, ALL_MATERIALS};
    use std::collections::{BTreeMap, BTreeSet};
    #[test]
    fn all_64_original_generated_materials_have_distinct_host_names() {
        let mut names = BTreeSet::new();
        let mut identities = BTreeMap::new();
        assert_eq!(ALL_MATERIALS.len(), 64);
        for &original in ALL_MATERIALS {
            let name = original.generated_state_name();
            assert!(name.starts_with("ilium:generated/"));
            assert!(name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_:/".contains(&b)));
            assert!(names.insert(name), "duplicate original identity {name}");
            assert!(identities.insert(format!("{original:?}"), name).is_none());
        }
        assert_eq!(identities["Grass"], "ilium:generated/grass");
        assert_eq!(identities["MossStone"], "ilium:generated/moss_stone");
        assert_eq!(
            identities["WhiteMushroom"],
            "ilium:generated/white_mushroom"
        );
        assert_eq!(identities["Basalt"], "ilium:generated/basalt");
        assert!(ALL_MATERIALS.contains(&Material::Basalt));
    }
}
