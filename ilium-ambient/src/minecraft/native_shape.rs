//! Pinned Java 1.19.3 `Shapes.blockOccudes` for exact cuboid decompositions.
//!
//! `LiquidBlockRenderer.isFaceOccludedByState` first checks the *block state's*
//! `canOcclude`. Its `getOcclusionShape` is a voxel shape, not the rendered
//! block model or a texture-alpha test. The caller must supply that exact
//! state-dependent shape. Discrete native voxel shapes may be decomposed into
//! cuboids without changing occupancy; this module checks their directional
//! boundary slices. Coordinates closer than native's 1e-7 comparison are
//! rejected until the full IndexMerger representation is available.
use super::chunk::BlockState;

const NATIVE_EPSILON: f64 = 1.0e-7;
const MAX_CUBOIDS: usize = 64;

// Exact surface names in source indexes009/020 plus individually sourced support
// states. This is not the full saved palette of all four windows. The material
// `isSolid`, `canOcclude`, and shape method are independent native properties.
const NO_OCCLUSION: &[&str] = &[
    "minecraft:acacia_leaves",
    "minecraft:air",
    "minecraft:allium",
    "minecraft:amethyst_cluster",
    "minecraft:azalea",
    "minecraft:azalea_leaves",
    "minecraft:azure_bluet",
    "minecraft:big_dripleaf_stem",
    "minecraft:birch_leaves",
    "minecraft:blue_bed",
    "minecraft:blue_orchid",
    "minecraft:brown_mushroom",
    "minecraft:bubble_column",
    "minecraft:campfire",
    "minecraft:cave_air",
    "minecraft:cave_vines",
    "minecraft:cave_vines_plant",
    "minecraft:chain",
    "minecraft:cobweb",
    "minecraft:cornflower",
    "minecraft:dandelion",
    "minecraft:dark_oak_trapdoor",
    "minecraft:dead_bush",
    "minecraft:fern",
    "minecraft:flowering_azalea",
    "minecraft:flowering_azalea_leaves",
    "minecraft:glass_pane",
    "minecraft:glow_lichen",
    "minecraft:grass",
    "minecraft:hanging_roots",
    "minecraft:ice",
    "minecraft:kelp",
    "minecraft:kelp_plant",
    "minecraft:large_amethyst_bud",
    "minecraft:large_fern",
    "minecraft:lava",
    "minecraft:lilac",
    "minecraft:lily_of_the_valley",
    "minecraft:lily_pad",
    "minecraft:medium_amethyst_bud",
    "minecraft:oak_leaves",
    "minecraft:orange_tulip",
    "minecraft:oxeye_daisy",
    "minecraft:peony",
    "minecraft:pink_tulip",
    "minecraft:pointed_dripstone",
    "minecraft:poppy",
    "minecraft:purple_bed",
    "minecraft:rail",
    "minecraft:red_mushroom",
    "minecraft:red_tulip",
    "minecraft:rose_bush",
    "minecraft:seagrass",
    "minecraft:small_amethyst_bud",
    "minecraft:small_dripleaf",
    "minecraft:spawner",
    "minecraft:spore_blossom",
    "minecraft:spruce_door",
    "minecraft:spruce_leaves",
    "minecraft:spruce_trapdoor",
    "minecraft:sugar_cane",
    "minecraft:sunflower",
    "minecraft:sweet_berry_bush",
    "minecraft:tall_grass",
    "minecraft:tall_seagrass",
    "minecraft:torch",
    "minecraft:vine",
    "minecraft:void_air",
    "minecraft:wall_torch",
    "minecraft:water",
    "minecraft:white_tulip",
];

const FULL_CUBE_OCCLUSION: &[&str] = &[
    "minecraft:acacia_log",
    "minecraft:amethyst_block",
    "minecraft:andesite",
    "minecraft:bedrock",
    "minecraft:bee_nest",
    "minecraft:birch_log",
    "minecraft:blue_ice",
    "minecraft:bone_block",
    "minecraft:budding_amethyst",
    "minecraft:calcite",
    "minecraft:chiseled_stone_bricks",
    "minecraft:clay",
    "minecraft:coal_ore",
    "minecraft:coarse_dirt",
    "minecraft:cobblestone",
    "minecraft:copper_ore",
    "minecraft:cracked_stone_bricks",
    "minecraft:crafting_table",
    "minecraft:crying_obsidian",
    "minecraft:dark_oak_log",
    "minecraft:dark_oak_planks",
    "minecraft:deepslate",
    "minecraft:deepslate_coal_ore",
    "minecraft:deepslate_copper_ore",
    "minecraft:deepslate_diamond_ore",
    "minecraft:deepslate_emerald_ore",
    "minecraft:deepslate_gold_ore",
    "minecraft:deepslate_iron_ore",
    "minecraft:deepslate_lapis_ore",
    "minecraft:deepslate_redstone_ore",
    "minecraft:diamond_ore",
    "minecraft:diorite",
    "minecraft:dirt",
    "minecraft:dripstone_block",
    "minecraft:emerald_ore",
    "minecraft:furnace",
    "minecraft:gold_ore",
    "minecraft:granite",
    "minecraft:grass_block",
    "minecraft:gravel",
    "minecraft:infested_deepslate",
    "minecraft:infested_stone",
    "minecraft:iron_ore",
    "minecraft:jack_o_lantern",
    "minecraft:jungle_planks",
    "minecraft:lapis_ore",
    "minecraft:magma_block",
    "minecraft:moss_block",
    "minecraft:mossy_cobblestone",
    "minecraft:mossy_stone_bricks",
    "minecraft:netherrack",
    "minecraft:oak_log",
    "minecraft:oak_planks",
    "minecraft:obsidian",
    "minecraft:packed_ice",
    "minecraft:podzol",
    "minecraft:polished_granite",
    "minecraft:prismarine",
    "minecraft:pumpkin",
    "minecraft:raw_copper_block",
    "minecraft:raw_iron_block",
    "minecraft:redstone_ore",
    "minecraft:rooted_dirt",
    "minecraft:sand",
    "minecraft:sandstone",
    "minecraft:sea_lantern",
    "minecraft:smooth_basalt",
    "minecraft:snow_block",
    "minecraft:spruce_log",
    "minecraft:spruce_planks",
    "minecraft:stone",
    "minecraft:stone_bricks",
    "minecraft:tuff",
];

const DYNAMIC_OCCLUSION: &[&str] = &[
    "minecraft:bell",
    "minecraft:big_dripleaf",
    "minecraft:chest",
    "minecraft:cobblestone_stairs",
    "minecraft:cobblestone_wall",
    "minecraft:dark_oak_fence",
    "minecraft:dark_oak_slab",
    "minecraft:dark_oak_stairs",
    "minecraft:dirt_path",
    "minecraft:jungle_fence",
    "minecraft:jungle_stairs",
    "minecraft:moss_carpet",
    "minecraft:mossy_stone_brick_slab",
    "minecraft:mossy_stone_brick_stairs",
    "minecraft:oak_fence",
    "minecraft:snow",
    "minecraft:spruce_fence",
    "minecraft:spruce_slab",
    "minecraft:spruce_stairs",
    "minecraft:stone_brick_slab",
    "minecraft:stone_brick_stairs",
    "minecraft:stone_slab",
];

// Exact `Material.isSolid` branch for the sourced names. This is
// deliberately separate from canOcclude and the state's solidRender predicate.
const SOLID_MATERIAL: &[&str] = &[
    "minecraft:acacia_leaves",
    "minecraft:acacia_log",
    "minecraft:amethyst_block",
    "minecraft:amethyst_cluster",
    "minecraft:andesite",
    "minecraft:azalea_leaves",
    "minecraft:bedrock",
    "minecraft:bee_nest",
    "minecraft:bell",
    "minecraft:birch_leaves",
    "minecraft:birch_log",
    "minecraft:blue_bed",
    "minecraft:blue_ice",
    "minecraft:bone_block",
    "minecraft:budding_amethyst",
    "minecraft:calcite",
    "minecraft:campfire",
    "minecraft:chain",
    "minecraft:chest",
    "minecraft:chiseled_stone_bricks",
    "minecraft:clay",
    "minecraft:coal_ore",
    "minecraft:coarse_dirt",
    "minecraft:cobblestone",
    "minecraft:cobblestone_stairs",
    "minecraft:cobblestone_wall",
    "minecraft:cobweb",
    "minecraft:copper_ore",
    "minecraft:cracked_stone_bricks",
    "minecraft:crafting_table",
    "minecraft:crying_obsidian",
    "minecraft:dark_oak_fence",
    "minecraft:dark_oak_log",
    "minecraft:dark_oak_planks",
    "minecraft:dark_oak_slab",
    "minecraft:dark_oak_stairs",
    "minecraft:dark_oak_trapdoor",
    "minecraft:deepslate",
    "minecraft:deepslate_coal_ore",
    "minecraft:deepslate_copper_ore",
    "minecraft:deepslate_diamond_ore",
    "minecraft:deepslate_emerald_ore",
    "minecraft:deepslate_gold_ore",
    "minecraft:deepslate_iron_ore",
    "minecraft:deepslate_lapis_ore",
    "minecraft:deepslate_redstone_ore",
    "minecraft:diamond_ore",
    "minecraft:diorite",
    "minecraft:dirt",
    "minecraft:dirt_path",
    "minecraft:dripstone_block",
    "minecraft:emerald_ore",
    "minecraft:flowering_azalea_leaves",
    "minecraft:furnace",
    "minecraft:glass_pane",
    "minecraft:gold_ore",
    "minecraft:granite",
    "minecraft:grass_block",
    "minecraft:gravel",
    "minecraft:ice",
    "minecraft:infested_deepslate",
    "minecraft:infested_stone",
    "minecraft:iron_ore",
    "minecraft:jack_o_lantern",
    "minecraft:jungle_fence",
    "minecraft:jungle_planks",
    "minecraft:jungle_stairs",
    "minecraft:lapis_ore",
    "minecraft:large_amethyst_bud",
    "minecraft:magma_block",
    "minecraft:medium_amethyst_bud",
    "minecraft:moss_block",
    "minecraft:mossy_cobblestone",
    "minecraft:mossy_stone_brick_slab",
    "minecraft:mossy_stone_brick_stairs",
    "minecraft:mossy_stone_bricks",
    "minecraft:netherrack",
    "minecraft:oak_fence",
    "minecraft:oak_leaves",
    "minecraft:oak_log",
    "minecraft:oak_planks",
    "minecraft:obsidian",
    "minecraft:packed_ice",
    "minecraft:podzol",
    "minecraft:pointed_dripstone",
    "minecraft:polished_granite",
    "minecraft:prismarine",
    "minecraft:pumpkin",
    "minecraft:purple_bed",
    "minecraft:raw_copper_block",
    "minecraft:raw_iron_block",
    "minecraft:redstone_ore",
    "minecraft:rooted_dirt",
    "minecraft:sand",
    "minecraft:sandstone",
    "minecraft:sea_lantern",
    "minecraft:small_amethyst_bud",
    "minecraft:smooth_basalt",
    "minecraft:snow_block",
    "minecraft:spawner",
    "minecraft:spruce_door",
    "minecraft:spruce_fence",
    "minecraft:spruce_leaves",
    "minecraft:spruce_log",
    "minecraft:spruce_planks",
    "minecraft:spruce_slab",
    "minecraft:spruce_stairs",
    "minecraft:spruce_trapdoor",
    "minecraft:stone",
    "minecraft:stone_brick_slab",
    "minecraft:stone_brick_stairs",
    "minecraft:stone_bricks",
    "minecraft:stone_slab",
    "minecraft:tuff",
];

// Independently derived from Material.blocksMotion (dtn.c, field Z). It
// happens to equal isSolid within this bounded named union;
// WEB and BAMBOO_SAPLING outside it prove these predicates cannot be aliased.
const MOTION_BLOCKING_MATERIAL: &[&str] = &[
    "minecraft:acacia_leaves",
    "minecraft:acacia_log",
    "minecraft:amethyst_block",
    "minecraft:amethyst_cluster",
    "minecraft:andesite",
    "minecraft:azalea_leaves",
    "minecraft:bedrock",
    "minecraft:bee_nest",
    "minecraft:bell",
    "minecraft:birch_leaves",
    "minecraft:birch_log",
    "minecraft:blue_bed",
    "minecraft:blue_ice",
    "minecraft:bone_block",
    "minecraft:budding_amethyst",
    "minecraft:calcite",
    "minecraft:campfire",
    "minecraft:chain",
    "minecraft:chest",
    "minecraft:chiseled_stone_bricks",
    "minecraft:clay",
    "minecraft:coal_ore",
    "minecraft:coarse_dirt",
    "minecraft:cobblestone",
    "minecraft:cobblestone_stairs",
    "minecraft:cobblestone_wall",
    "minecraft:copper_ore",
    "minecraft:cracked_stone_bricks",
    "minecraft:crafting_table",
    "minecraft:crying_obsidian",
    "minecraft:dark_oak_fence",
    "minecraft:dark_oak_log",
    "minecraft:dark_oak_planks",
    "minecraft:dark_oak_slab",
    "minecraft:dark_oak_stairs",
    "minecraft:dark_oak_trapdoor",
    "minecraft:deepslate",
    "minecraft:deepslate_coal_ore",
    "minecraft:deepslate_copper_ore",
    "minecraft:deepslate_diamond_ore",
    "minecraft:deepslate_emerald_ore",
    "minecraft:deepslate_gold_ore",
    "minecraft:deepslate_iron_ore",
    "minecraft:deepslate_lapis_ore",
    "minecraft:deepslate_redstone_ore",
    "minecraft:diamond_ore",
    "minecraft:diorite",
    "minecraft:dirt",
    "minecraft:dirt_path",
    "minecraft:dripstone_block",
    "minecraft:emerald_ore",
    "minecraft:flowering_azalea_leaves",
    "minecraft:furnace",
    "minecraft:glass_pane",
    "minecraft:gold_ore",
    "minecraft:granite",
    "minecraft:grass_block",
    "minecraft:gravel",
    "minecraft:ice",
    "minecraft:infested_deepslate",
    "minecraft:infested_stone",
    "minecraft:iron_ore",
    "minecraft:jack_o_lantern",
    "minecraft:jungle_fence",
    "minecraft:jungle_planks",
    "minecraft:jungle_stairs",
    "minecraft:lapis_ore",
    "minecraft:large_amethyst_bud",
    "minecraft:magma_block",
    "minecraft:medium_amethyst_bud",
    "minecraft:moss_block",
    "minecraft:mossy_cobblestone",
    "minecraft:mossy_stone_brick_slab",
    "minecraft:mossy_stone_brick_stairs",
    "minecraft:mossy_stone_bricks",
    "minecraft:netherrack",
    "minecraft:oak_fence",
    "minecraft:oak_leaves",
    "minecraft:oak_log",
    "minecraft:oak_planks",
    "minecraft:obsidian",
    "minecraft:packed_ice",
    "minecraft:podzol",
    "minecraft:pointed_dripstone",
    "minecraft:polished_granite",
    "minecraft:prismarine",
    "minecraft:pumpkin",
    "minecraft:purple_bed",
    "minecraft:raw_copper_block",
    "minecraft:raw_iron_block",
    "minecraft:redstone_ore",
    "minecraft:rooted_dirt",
    "minecraft:sand",
    "minecraft:sandstone",
    "minecraft:sea_lantern",
    "minecraft:small_amethyst_bud",
    "minecraft:smooth_basalt",
    "minecraft:snow_block",
    "minecraft:spawner",
    "minecraft:spruce_door",
    "minecraft:spruce_fence",
    "minecraft:spruce_leaves",
    "minecraft:spruce_log",
    "minecraft:spruce_planks",
    "minecraft:spruce_slab",
    "minecraft:spruce_stairs",
    "minecraft:spruce_trapdoor",
    "minecraft:stone",
    "minecraft:stone_brick_slab",
    "minecraft:stone_brick_stairs",
    "minecraft:stone_bricks",
    "minecraft:stone_slab",
    "minecraft:tuff",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cuboid {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    cuboids: Vec<Cuboid>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("native shape has a nonfinite or zero-volume cuboid")]
    InvalidCuboid,
    #[error("native shape exceeds the admitted 64-cuboid decomposition")]
    Limit,
    #[error("native shape has distinct boundaries inside Java's 1e-7 tolerance")]
    Precision,
    #[error("native canOcclude flag is not captured for saved state {0}")]
    UnknownFlag(String),
    #[error("native state-dependent getOcclusionShape is not resolved for {0}")]
    DynamicShape(String),
    #[error("native shape has missing or invalid saved properties for {0}")]
    InvalidState(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateOcclusion {
    pub can_occlude: bool,
    pub shape: Shape,
}

/// Surface names came from source indexes009/020; cave/void air and these two
/// buried support states have separate pinned registration proofs. Other names
/// fail explicitly until their actual registration chain is captured.
pub fn can_occlude(state: &BlockState) -> Result<bool, Error> {
    let name = state.name.as_str();
    if NO_OCCLUSION.contains(&name) {
        Ok(false)
    } else if FULL_CUBE_OCCLUSION.contains(&name) || DYNAMIC_OCCLUSION.contains(&name) {
        Ok(true)
    } else {
        Err(Error::UnknownFlag(state.name.clone()))
    }
}

/// Native material solidness for exact captured names, used by fluid height
/// and flow. An unknown support neighbor is a typed error, never assumed air.
pub fn material_is_solid(state: &BlockState) -> Result<bool, Error> {
    let name = state.name.as_str();
    if SOLID_MATERIAL.contains(&name) {
        Ok(true)
    } else if NO_OCCLUSION.contains(&name)
        || DYNAMIC_OCCLUSION.contains(&name)
        || FULL_CUBE_OCCLUSION.contains(&name)
    {
        Ok(false)
    } else {
        Err(Error::UnknownFlag(state.name.clone()))
    }
}

/// FlowingFluid's zero-neighbor-height branch uses this independent native
/// predicate. An unindexed saved support block is an error, not assumed air.
pub fn material_blocks_motion(state: &BlockState) -> Result<bool, Error> {
    let name = state.name.as_str();
    if MOTION_BLOCKING_MATERIAL.contains(&name) {
        Ok(true)
    } else if NO_OCCLUSION.contains(&name)
        || DYNAMIC_OCCLUSION.contains(&name)
        || FULL_CUBE_OCCLUSION.contains(&name)
    {
        Ok(false)
    } else {
        Err(Error::UnknownFlag(state.name.clone()))
    }
}

/// Caller-ready occlusion for the captured named union. The seven former
/// dynamic cases use source022's exact state-selected cuboids. Their native
/// getOcclusionShape endpoints take a world/position but read no neighbor in
/// these seven paths. Unknown names still require their own source evidence.
pub fn state_occlusion(state: &BlockState) -> Result<StateOcclusion, Error> {
    let flag = can_occlude(state)?;
    if !flag {
        return Ok(StateOcclusion {
            can_occlude: false,
            shape: Shape::empty(),
        });
    }
    if FULL_CUBE_OCCLUSION.contains(&state.name.as_str()) {
        return Ok(StateOcclusion {
            can_occlude: true,
            shape: Shape::block(),
        });
    }
    let shape = match state.name.as_str() {
        "minecraft:bell" => bell_shape(state)?,
        "minecraft:chest" => chest_shape(state)?,
        "minecraft:cobblestone_stairs"
        | "minecraft:spruce_stairs"
        | "minecraft:dark_oak_stairs"
        | "minecraft:jungle_stairs"
        | "minecraft:mossy_stone_brick_stairs"
        | "minecraft:stone_brick_stairs" => stairs_shape(state)?,
        "minecraft:cobblestone_wall" => wall_shape(state)?,
        "minecraft:dirt_path" => shape_from_units(&[[0, 0, 0, 16, 15, 16]])?,
        "minecraft:spruce_fence"
        | "minecraft:dark_oak_fence"
        | "minecraft:jungle_fence"
        | "minecraft:oak_fence" => fence_shape(state)?,
        "minecraft:dark_oak_slab"
        | "minecraft:mossy_stone_brick_slab"
        | "minecraft:spruce_slab"
        | "minecraft:stone_brick_slab"
        | "minecraft:stone_slab" => slab_shape(state)?,
        "minecraft:moss_carpet" => shape_from_units(&[[0, 0, 0, 16, 1, 16]])?,
        "minecraft:snow" => snow_shape(state)?,
        "minecraft:big_dripleaf" => big_dripleaf_shape(state)?,
        _ => return Err(Error::DynamicShape(state.name.clone())),
    };
    Ok(StateOcclusion {
        can_occlude: true,
        shape,
    })
}

fn property<'a>(state: &'a BlockState, name: &str) -> Result<&'a str, Error> {
    state
        .properties
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| Error::InvalidState(state.name.clone()))
}

fn shape_from_units(boxes: &[[u8; 6]]) -> Result<Shape, Error> {
    Shape::new(
        boxes
            .iter()
            .map(|box_| Cuboid {
                min: [box_[0], box_[1], box_[2]].map(|v| f64::from(v) / 16.0),
                max: [box_[3], box_[4], box_[5]].map(|v| f64::from(v) / 16.0),
            })
            .collect(),
    )
}

fn bell_shape(state: &BlockState) -> Result<Shape, Error> {
    // cmp.h's field selection and cmp.<clinit> literal cuboids. The model's
    // separate moving bell body is NOT part of this block occlusion shape.
    let facing = property(state, "facing")?;
    let attachment = property(state, "attachment")?;
    let mut boxes = Vec::with_capacity(3);
    match attachment {
        "floor" => boxes.push(match facing {
            "north" | "south" => [0, 0, 4, 16, 16, 12],
            "west" | "east" => [4, 0, 0, 12, 16, 16],
            _ => return Err(Error::InvalidState(state.name.clone())),
        }),
        "ceiling" | "single_wall" | "double_wall" => {
            boxes.extend([[4, 4, 4, 12, 6, 12], [5, 6, 5, 11, 13, 11]]);
            boxes.push(match attachment {
                "ceiling" => [7, 13, 7, 9, 16, 9],
                "double_wall" => match facing {
                    "north" | "south" => [7, 13, 0, 9, 15, 16],
                    "west" | "east" => [0, 13, 7, 16, 15, 9],
                    _ => return Err(Error::InvalidState(state.name.clone())),
                },
                "single_wall" => match facing {
                    "north" => [7, 13, 0, 9, 15, 13],
                    "south" => [7, 13, 3, 9, 15, 16],
                    "west" => [0, 13, 7, 13, 15, 9],
                    "east" => [3, 13, 7, 16, 15, 9],
                    _ => return Err(Error::InvalidState(state.name.clone())),
                },
                _ => return Err(Error::InvalidState(state.name.clone())),
            });
        }
        _ => return Err(Error::InvalidState(state.name.clone())),
    }
    if !matches!(property(state, "powered")?, "true" | "false") {
        return Err(Error::InvalidState(state.name.clone()));
    }
    shape_from_units(&boxes)
}

fn clockwise(facing: &str) -> Option<&'static str> {
    Some(match facing {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        "west" => "north",
        _ => return None,
    })
}

fn counter_clockwise(facing: &str) -> Option<&'static str> {
    Some(match facing {
        "north" => "west",
        "west" => "south",
        "south" => "east",
        "east" => "north",
        _ => return None,
    })
}

fn chest_shape(state: &BlockState) -> Result<Shape, Error> {
    // cns.getShape: single field l or connected direction h/i/j/k. cns.h
    // rotates facing clockwise for LEFT and counterclockwise for RIGHT.
    let facing = property(state, "facing")?;
    let box_ = match property(state, "type")? {
        "single" => [1, 0, 1, 15, 14, 15],
        "left" | "right" => {
            let connected = if property(state, "type")? == "left" {
                clockwise(facing)
            } else {
                counter_clockwise(facing)
            }
            .ok_or_else(|| Error::InvalidState(state.name.clone()))?;
            match connected {
                "north" => [1, 0, 0, 15, 14, 15],
                "south" => [1, 0, 1, 15, 14, 16],
                "west" => [0, 0, 1, 15, 14, 15],
                "east" => [1, 0, 1, 16, 14, 15],
                _ => return Err(Error::InvalidState(state.name.clone())),
            }
        }
        _ => return Err(Error::InvalidState(state.name.clone())),
    };
    if clockwise(facing).is_none() || !matches!(property(state, "waterlogged")?, "true" | "false") {
        return Err(Error::InvalidState(state.name.clone()));
    }
    shape_from_units(&[box_])
}

fn stairs_shape(state: &BlockState) -> Result<Shape, Error> {
    // cug.G[shape.ordinal()*4+facing.get2DDataValue], with SOUTH=0,
    // WEST=1, NORTH=2, EAST=3. Bit order is g/k/h/l for TOP and i/m/j/n
    // for BOTTOM after ctq top/bottom slab union.
    const MASKS: [u8; 20] = [
        12, 5, 3, 10, 14, 13, 7, 11, 13, 7, 11, 14, 8, 4, 1, 2, 4, 1, 2, 8,
    ];
    let shape = match property(state, "shape")? {
        "straight" => 0,
        "inner_left" => 1,
        "inner_right" => 2,
        "outer_left" => 3,
        "outer_right" => 4,
        _ => return Err(Error::InvalidState(state.name.clone())),
    };
    let direction = match property(state, "facing")? {
        "south" => 0,
        "west" => 1,
        "north" => 2,
        "east" => 3,
        _ => return Err(Error::InvalidState(state.name.clone())),
    };
    let top = match property(state, "half")? {
        "top" => true,
        "bottom" => false,
        _ => return Err(Error::InvalidState(state.name.clone())),
    };
    if !matches!(property(state, "waterlogged")?, "true" | "false") {
        return Err(Error::InvalidState(state.name.clone()));
    }
    let mask = MASKS[shape * 4 + direction];
    let mut boxes = vec![if top {
        [0, 8, 0, 16, 16, 16]
    } else {
        [0, 0, 0, 16, 8, 16]
    }];
    let y = if top { [0, 8] } else { [8, 16] };
    for (bit, x, z) in [
        (1, [0, 8], [0, 8]),
        (2, [8, 16], [0, 8]),
        (4, [0, 8], [8, 16]),
        (8, [8, 16], [8, 16]),
    ] {
        if mask & bit != 0 {
            boxes.push([x[0], y[0], z[0], x[1], y[1], z[1]]);
        }
    }
    shape_from_units(&boxes)
}

fn slab_shape(state: &BlockState) -> Result<Shape, Error> {
    // ctq.getShape switches the mapped SlabType: DOUBLE -> Shapes.block,
    // TOP -> [0,8,0,16,16,16], BOTTOM -> [0,0,0,16,8,16].
    if !matches!(property(state, "waterlogged")?, "true" | "false") {
        return Err(Error::InvalidState(state.name.clone()));
    }
    match property(state, "type")? {
        "double" => Ok(Shape::block()),
        "top" => shape_from_units(&[[0, 8, 0, 16, 16, 16]]),
        "bottom" => shape_from_units(&[[0, 0, 0, 16, 8, 16]]),
        _ => Err(Error::InvalidState(state.name.clone())),
    }
}

fn snow_shape(state: &BlockState) -> Result<Shape, Error> {
    // ctv.c[LAYERS] contains the eight exact 2/16-height steps.
    let layers = property(state, "layers")?
        .parse::<u8>()
        .map_err(|_| Error::InvalidState(state.name.clone()))?;
    if !(1..=8).contains(&layers) {
        return Err(Error::InvalidState(state.name.clone()));
    }
    shape_from_units(&[[0, 0, 0, 16, layers * 2, 16]])
}

fn big_dripleaf_shape(state: &BlockState) -> Result<Shape, Error> {
    // cmq.h unions the tilt-selected leaf with cmq.k[facing]. The latter is
    // cmr's direction-selected stem minus the top 3/16 slab, so it ends at 13.
    // cmq's bootstrap binds h to the saved-state getShape lookup map.
    let leaf = match property(state, "tilt")? {
        "none" | "unstable" => Some([0, 11, 0, 16, 15, 16]),
        "partial" => Some([0, 11, 0, 16, 13, 16]),
        "full" => None,
        _ => return Err(Error::InvalidState(state.name.clone())),
    };
    let stem = match property(state, "facing")? {
        "north" => [5, 0, 9, 11, 13, 15],
        "south" => [5, 0, 1, 11, 13, 7],
        "east" => [1, 0, 5, 7, 13, 11],
        "west" => [9, 0, 5, 15, 13, 11],
        _ => return Err(Error::InvalidState(state.name.clone())),
    };
    if !matches!(property(state, "waterlogged")?, "true" | "false") {
        return Err(Error::InvalidState(state.name.clone()));
    }
    let mut boxes = vec![stem];
    if let Some(leaf) = leaf {
        boxes.push(leaf);
    }
    shape_from_units(&boxes)
}

fn wall_shape(state: &BlockState) -> Result<Shape, Error> {
    // cvh outline map a(4,3,16,0,14,16); this is the delegated occlusion
    // shape, not its 24-high collision map. Each saved WallSide is explicit.
    let mut boxes = Vec::with_capacity(5);
    for (side, low) in [
        ("north", [5, 0, 0, 11, 14, 11]),
        ("south", [5, 0, 5, 11, 14, 16]),
        ("west", [0, 0, 5, 11, 14, 11]),
        ("east", [5, 0, 5, 16, 14, 11]),
    ] {
        match property(state, side)? {
            "none" => {}
            "low" => boxes.push(low),
            "tall" => {
                let mut high = low;
                high[4] = 16;
                boxes.push(high);
            }
            _ => return Err(Error::InvalidState(state.name.clone())),
        }
    }
    if property(state, "up")? == "true" {
        boxes.push([4, 0, 4, 12, 16, 12]);
    } else if property(state, "up")? != "false" {
        return Err(Error::InvalidState(state.name.clone()));
    }
    if !matches!(property(state, "waterlogged")?, "true" | "false") {
        return Err(Error::InvalidState(state.name.clone()));
    }
    shape_from_units(&boxes)
}

fn fence_shape(state: &BlockState) -> Result<Shape, Error> {
    // cpj.getOcclusionShape selects coi.a(2,1,16,6,15), distinct from the
    // outline and collision arrays. SOUTH/WEST/NORTH/EAST arms are bit1/2/4/8.
    let mut boxes = vec![[6, 0, 6, 10, 16, 10]];
    for (side, arm) in [
        ("south", [7, 6, 7, 9, 15, 16]),
        ("west", [0, 6, 7, 9, 15, 9]),
        ("north", [7, 6, 0, 9, 15, 9]),
        ("east", [7, 6, 7, 16, 15, 9]),
    ] {
        match property(state, side)? {
            "false" => {}
            "true" => boxes.push(arm),
            _ => return Err(Error::InvalidState(state.name.clone())),
        }
    }
    if !matches!(property(state, "waterlogged")?, "true" | "false") {
        return Err(Error::InvalidState(state.name.clone()));
    }
    shape_from_units(&boxes)
}

impl Shape {
    pub fn new(cuboids: Vec<Cuboid>) -> Result<Self, Error> {
        if cuboids.len() > MAX_CUBOIDS {
            return Err(Error::Limit);
        }
        if cuboids.iter().any(|cube| {
            (0..3).any(|axis| {
                !cube.min[axis].is_finite()
                    || !cube.max[axis].is_finite()
                    || cube.min[axis] >= cube.max[axis]
            })
        }) {
            return Err(Error::InvalidCuboid);
        }
        Ok(Self { cuboids })
    }

    pub fn empty() -> Self {
        Self {
            cuboids: Vec::new(),
        }
    }

    pub fn block() -> Self {
        Self {
            cuboids: vec![Cuboid {
                min: [0.0; 3],
                max: [1.0; 3],
            }],
        }
    }

    pub fn fluid_box(height: f32) -> Result<Self, Error> {
        if !height.is_finite() || !(0.0..=1.0).contains(&height) {
            return Err(Error::InvalidCuboid);
        }
        if height == 0.0 {
            return Ok(Self::empty());
        }
        Self::new(vec![Cuboid {
            min: [0.0; 3],
            max: [1.0, f64::from(height), 1.0],
        }])
    }

    fn extreme(&self, axis: usize, maximum: bool) -> Option<f64> {
        self.cuboids
            .iter()
            .map(|cube| {
                if maximum {
                    cube.max[axis]
                } else {
                    cube.min[axis]
                }
            })
            .reduce(|left, right| {
                if maximum {
                    left.max(right)
                } else {
                    left.min(right)
                }
            })
    }
}

impl Direction {
    fn axis(self) -> usize {
        match self {
            Self::West | Self::East => 0,
            Self::Down | Self::Up => 1,
            Self::North | Self::South => 2,
        }
    }

    fn positive(self) -> bool {
        matches!(self, Self::Up | Self::South | Self::East)
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }
}

#[derive(Clone, Copy)]
struct Rectangle {
    min: [f64; 2],
    max: [f64; 2],
}

fn fuzzy_equal(left: f64, right: f64) -> bool {
    left == right || (left - right).abs() <= NATIVE_EPSILON
}

fn slice(shape: &Shape, axis: usize, high: bool) -> Vec<Rectangle> {
    let Some(edge) = shape.extreme(axis, high) else {
        return Vec::new();
    };
    let other: Vec<_> = (0..3).filter(|candidate| *candidate != axis).collect();
    shape
        .cuboids
        .iter()
        .filter(|cube| fuzzy_equal(if high { cube.max[axis] } else { cube.min[axis] }, edge))
        .map(|cube| Rectangle {
            min: [cube.min[other[0]], cube.min[other[1]]],
            max: [cube.max[other[0]], cube.max[other[1]]],
        })
        .collect()
}

fn sorted_edges(rectangles: &[Rectangle], dimension: usize) -> Result<Vec<f64>, Error> {
    let mut edges = Vec::with_capacity(rectangles.len() * 2);
    for rectangle in rectangles {
        edges.push(rectangle.min[dimension]);
        edges.push(rectangle.max[dimension]);
    }
    edges.sort_by(f64::total_cmp);
    edges.dedup();
    if edges
        .windows(2)
        .any(|window| window[1] - window[0] <= NATIVE_EPSILON)
    {
        return Err(Error::Precision);
    }
    Ok(edges)
}

/// The native index merger compares coordinates on every axis, including the
/// sliced one. Our cuboid slice shortcut is valid only when distinct source
/// coordinates cannot merge under its 1e-7 fuzzy equality.
fn validate_source_edges(first: &Shape, second: &Shape) -> Result<(), Error> {
    for axis in 0..3 {
        let mut edges = Vec::with_capacity((first.cuboids.len() + second.cuboids.len()) * 2);
        for cuboid in first.cuboids.iter().chain(&second.cuboids) {
            edges.extend([cuboid.min[axis], cuboid.max[axis]]);
        }
        edges.sort_by(f64::total_cmp);
        edges.dedup();
        if edges
            .windows(2)
            .any(|pair| pair[1] - pair[0] <= NATIVE_EPSILON)
        {
            return Err(Error::Precision);
        }
    }
    Ok(())
}

fn contains(rectangles: &[Rectangle], point: [f64; 2]) -> bool {
    rectangles.iter().any(|rectangle| {
        (0..2).all(|axis| rectangle.min[axis] < point[axis] && point[axis] < rectangle.max[axis])
    })
}

fn difference_is_empty(target: &[Rectangle], cover: &[Rectangle]) -> Result<bool, Error> {
    if target.is_empty() {
        return Ok(true);
    }
    let all: Vec<_> = target.iter().chain(cover).copied().collect();
    let x = sorted_edges(&all, 0)?;
    let y = sorted_edges(&all, 1)?;
    for horizontal in x.windows(2) {
        for vertical in y.windows(2) {
            let point = [
                horizontal[0] + (horizontal[1] - horizontal[0]) * 0.5,
                vertical[0] + (vertical[1] - vertical[0]) * 0.5,
            ];
            if contains(target, point) && !contains(cover, point) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Implements the native method's positive/negative slice order and BooleanOp.
/// The source method tests whether its first slice (positive) or second slice
/// (negative) has any area outside the other. Java BooleanOp `eai.e` is
/// ONLY_FIRST and `eai.c` is ONLY_SECOND. Do not call this without the exact saved block
/// state's native `getOcclusionShape` and `canOcclude` result.
pub fn block_occudes(fluid: &Shape, block: &Shape, direction: Direction) -> Result<bool, Error> {
    if block.cuboids.is_empty() {
        return Ok(false);
    }
    validate_source_edges(fluid, block)?;
    let axis = direction.axis();
    let positive = direction.positive();
    let (first, second) = if positive {
        (fluid, block)
    } else {
        (block, fluid)
    };
    if !first
        .extreme(axis, true)
        .is_some_and(|value| fuzzy_equal(value, 1.0))
        || !second
            .extreme(axis, false)
            .is_some_and(|value| fuzzy_equal(value, 0.0))
    {
        return Ok(false);
    }
    let first_slice = slice(first, axis, true);
    let second_slice = slice(second, axis, false);
    if positive {
        // Native BooleanOp.ONLY_FIRST.
        difference_is_empty(&first_slice, &second_slice)
    } else {
        // Native BooleanOp.ONLY_SECOND.
        difference_is_empty(&second_slice, &first_slice)
    }
}

/// `LiquidBlockRenderer.isFaceOccludedByState`: the state's `canOcclude` gate
/// precedes `Shapes.blockOccudes`.
pub fn face_occluded(
    can_occlude: bool,
    fluid: &Shape,
    block: &Shape,
    direction: Direction,
) -> Result<bool, Error> {
    if !can_occlude {
        return Ok(false);
    }
    block_occudes(fluid, block, direction)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn state(name: &str) -> BlockState {
        BlockState {
            name: name.into(),
            properties: BTreeMap::new(),
        }
    }

    fn state_props(name: &str, properties: &[(&str, &str)]) -> BlockState {
        BlockState {
            name: name.into(),
            properties: properties
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect(),
        }
    }

    #[test]
    fn captured_factory_flags_keep_shape_and_material_predicates_separate() {
        for name in [
            "minecraft:air",
            "minecraft:cave_air",
            "minecraft:void_air",
            "minecraft:water",
            "minecraft:lava",
            "minecraft:bubble_column",
            "minecraft:acacia_leaves",
            "minecraft:birch_leaves",
            "minecraft:oak_leaves",
            "minecraft:campfire",
            "minecraft:chain",
            "minecraft:glass_pane",
            "minecraft:glow_lichen",
            "minecraft:spruce_trapdoor",
        ] {
            let occlusion = state_occlusion(&state(name)).unwrap();
            assert!(!occlusion.can_occlude, "{name}");
            assert_eq!(occlusion.shape, Shape::empty());
        }
        assert_eq!(
            state_occlusion(&state("minecraft:stone")).unwrap().shape,
            Shape::block()
        );
        assert_eq!(
            state_occlusion(&state("minecraft:dirt")).unwrap().shape,
            Shape::block()
        );
        assert_eq!(
            state_occlusion(&state("minecraft:oak_log")).unwrap().shape,
            Shape::block()
        );
        for name in DYNAMIC_OCCLUSION
            .iter()
            .copied()
            .filter(|name| !matches!(*name, "minecraft:dirt_path" | "minecraft:moss_carpet"))
        {
            assert!(can_occlude(&state(name)).unwrap());
            assert_eq!(
                state_occlusion(&state(name)),
                Err(Error::InvalidState(name.into()))
            );
        }
        assert_eq!(NO_OCCLUSION.len(), 71); // Exact source009/020/045 plus source-mapped 085 names.
        assert_eq!(FULL_CUBE_OCCLUSION.len(), 73);
        assert_eq!(DYNAMIC_OCCLUSION.len(), 22);
        assert_eq!(SOLID_MATERIAL.len(), 114);
        assert_eq!(MOTION_BLOCKING_MATERIAL.len(), 113);
        assert!(material_blocks_motion(&state("minecraft:oak_leaves")).unwrap());
        assert!(!material_blocks_motion(&state("minecraft:water")).unwrap());
        assert_eq!(
            material_blocks_motion(&state("minecraft:web")),
            Err(Error::UnknownFlag("minecraft:web".into()))
        );
        assert!(material_is_solid(&state("minecraft:oak_leaves")).unwrap());
        assert!(!can_occlude(&state("minecraft:oak_leaves")).unwrap());
        assert!(!material_is_solid(&state("minecraft:glow_lichen")).unwrap());
        assert!(!material_is_solid(&state("minecraft:air")).unwrap());
        assert_eq!(
            can_occlude(&state("minecraft:oak_pressure_plate")),
            Err(Error::UnknownFlag("minecraft:oak_pressure_plate".into()))
        );
    }

    fn assert_full_solid_occluder(saved: BlockState) {
        assert!(can_occlude(&saved).unwrap());
        assert!(material_is_solid(&saved).unwrap());
        assert!(material_blocks_motion(&saved).unwrap());
        assert_eq!(
            state_occlusion(&saved).unwrap(),
            StateOcclusion {
                can_occlude: true,
                shape: Shape::block(),
            }
        );
    }

    #[test]
    fn pinned_bedrock_support_state_is_a_full_solid_occluder() {
        assert_full_solid_occluder(state("minecraft:bedrock"));
    }

    #[test]
    fn pinned_deepslate_axes_are_full_solid_occluders() {
        for axis in ["x", "y", "z"] {
            assert_full_solid_occluder(state_props("minecraft:deepslate", &[("axis", axis)]));
        }
    }

    #[test]
    fn new_support_rows_do_not_alias_other_deepslate_names() {
        assert_eq!(
            can_occlude(&state("minecraft:reinforced_deepslate")),
            Err(Error::UnknownFlag("minecraft:reinforced_deepslate".into()))
        );
    }

    #[test]
    fn source085_material_and_occlusion_flags_are_independent() {
        let web = state("minecraft:cobweb");
        assert!(!can_occlude(&web).unwrap());
        assert!(material_is_solid(&web).unwrap());
        assert!(!material_blocks_motion(&web).unwrap());

        let ice = state("minecraft:ice");
        assert!(!can_occlude(&ice).unwrap());
        assert!(material_is_solid(&ice).unwrap());
        assert!(material_blocks_motion(&ice).unwrap());

        let carpet = state("minecraft:moss_carpet");
        assert!(can_occlude(&carpet).unwrap());
        assert!(!material_is_solid(&carpet).unwrap());
        assert!(!material_blocks_motion(&carpet).unwrap());
        assert_eq!(
            state_occlusion(&carpet).unwrap().shape,
            shape_from_units(&[[0, 0, 0, 16, 1, 16]]).unwrap()
        );

        let dripleaf = state_props(
            "minecraft:big_dripleaf",
            &[
                ("tilt", "none"),
                ("facing", "north"),
                ("waterlogged", "true"),
            ],
        );
        assert!(can_occlude(&dripleaf).unwrap());
        assert!(!material_is_solid(&dripleaf).unwrap());
        assert!(!material_blocks_motion(&dripleaf).unwrap());
    }

    #[test]
    fn source085_slab_and_snow_shapes_follow_saved_properties() {
        for name in [
            "minecraft:dark_oak_slab",
            "minecraft:mossy_stone_brick_slab",
            "minecraft:spruce_slab",
            "minecraft:stone_brick_slab",
            "minecraft:stone_slab",
        ] {
            for (kind, min_y, max_y) in [
                ("bottom", 0.0, 0.5),
                ("top", 0.5, 1.0),
                ("double", 0.0, 1.0),
            ] {
                let slab = state_props(name, &[("type", kind), ("waterlogged", "true")]);
                let shape = state_occlusion(&slab).unwrap();
                assert!(shape.can_occlude);
                assert_eq!(shape.shape.cuboids[0].min[1], min_y, "{name} {kind}");
                assert_eq!(shape.shape.cuboids[0].max[1], max_y, "{name} {kind}");
            }
        }
        for layers in 1..=8 {
            let snow = state_props("minecraft:snow", &[("layers", &layers.to_string())]);
            let shape = state_occlusion(&snow).unwrap();
            assert_eq!(shape.shape.cuboids[0].max[1], f64::from(layers) / 8.0);
        }
        assert_eq!(
            state_occlusion(&state_props("minecraft:snow", &[("layers", "9")])),
            Err(Error::InvalidState("minecraft:snow".into()))
        );
    }

    #[test]
    fn source085_big_dripleaf_union_tracks_tilt_and_facing() {
        for (facing, expected_stem) in [
            ("north", [5, 0, 9, 11, 13, 15]),
            ("south", [5, 0, 1, 11, 13, 7]),
            ("east", [1, 0, 5, 7, 13, 11]),
            ("west", [9, 0, 5, 15, 13, 11]),
        ] {
            for (tilt, leaf_height) in [
                ("none", Some(15.0 / 16.0)),
                ("unstable", Some(15.0 / 16.0)),
                ("partial", Some(13.0 / 16.0)),
                ("full", None),
            ] {
                let saved = state_props(
                    "minecraft:big_dripleaf",
                    &[("facing", facing), ("tilt", tilt), ("waterlogged", "false")],
                );
                let shape = state_occlusion(&saved).unwrap().shape;
                assert_eq!(
                    shape.cuboids[0],
                    shape_from_units(&[expected_stem]).unwrap().cuboids[0]
                );
                assert_eq!(
                    shape.cuboids.len(),
                    if leaf_height.is_some() { 2 } else { 1 }
                );
                if let Some(height) = leaf_height {
                    assert_eq!(shape.cuboids[1].max[1], height);
                }
            }
        }
    }

    #[test]
    fn source085_new_fences_and_stairs_reuse_pinned_native_shape_classes() {
        for name in [
            "minecraft:dark_oak_fence",
            "minecraft:jungle_fence",
            "minecraft:oak_fence",
        ] {
            let saved = state_props(
                name,
                &[
                    ("north", "true"),
                    ("south", "false"),
                    ("west", "false"),
                    ("east", "false"),
                    ("waterlogged", "false"),
                ],
            );
            assert_eq!(state_occlusion(&saved).unwrap().shape.cuboids.len(), 2);
        }
        for name in [
            "minecraft:dark_oak_stairs",
            "minecraft:mossy_stone_brick_stairs",
            "minecraft:stone_brick_stairs",
        ] {
            let saved = state_props(
                name,
                &[
                    ("facing", "south"),
                    ("half", "top"),
                    ("shape", "straight"),
                    ("waterlogged", "true"),
                ],
            );
            assert_eq!(state_occlusion(&saved).unwrap().shape.cuboids.len(), 3);
        }
    }

    #[test]
    fn exact098_jungle_stairs_variants_use_the_native_stair_shape() {
        // Three actual projected-source palette witnesses in two chunks;
        // registration fw copies jungle_planks and instantiates cug.
        for (facing, half, first_octant) in [
            ("west", "top", [0.0, 0.0, 0.0]),
            ("east", "bottom", [0.5, 0.5, 0.0]),
            ("east", "top", [0.5, 0.0, 0.0]),
        ] {
            let saved = state_props(
                "minecraft:jungle_stairs",
                &[
                    ("facing", facing),
                    ("half", half),
                    ("shape", "straight"),
                    ("waterlogged", "true"),
                ],
            );
            assert!(can_occlude(&saved).unwrap());
            assert!(material_is_solid(&saved).unwrap());
            assert!(material_blocks_motion(&saved).unwrap());
            let shape = state_occlusion(&saved).unwrap().shape;
            assert_eq!(shape.cuboids.len(), 3);
            assert!(shape.cuboids[1..]
                .iter()
                .any(|cube| cube.min == first_octant));
        }
    }

    #[test]
    fn source022_state_shapes_use_occlusion_not_collision_boxes() {
        let path = state_occlusion(&state("minecraft:dirt_path")).unwrap();
        assert_eq!(path.shape.cuboids[0].max[1], 15.0 / 16.0);
        let fence = state_props(
            "minecraft:spruce_fence",
            &[
                ("north", "true"),
                ("south", "false"),
                ("west", "false"),
                ("east", "false"),
                ("waterlogged", "true"),
            ],
        );
        let fence = state_occlusion(&fence).unwrap();
        assert_eq!(fence.shape.cuboids.len(), 2);
        assert_eq!(fence.shape.cuboids[1].min, [7.0 / 16.0, 6.0 / 16.0, 0.0]);
        assert_eq!(fence.shape.cuboids[1].max[1], 15.0 / 16.0);
        let wall = state_props(
            "minecraft:cobblestone_wall",
            &[
                ("north", "none"),
                ("south", "tall"),
                ("west", "none"),
                ("east", "none"),
                ("up", "false"),
                ("waterlogged", "true"),
            ],
        );
        let wall = state_occlusion(&wall).unwrap();
        assert_eq!(wall.shape.cuboids.len(), 1);
        assert_eq!(wall.shape.cuboids[0].max[1], 1.0);
        let stair = state_props(
            "minecraft:spruce_stairs",
            &[
                ("facing", "south"),
                ("half", "top"),
                ("shape", "straight"),
                ("waterlogged", "true"),
            ],
        );
        let stair = state_occlusion(&stair).unwrap();
        assert_eq!(stair.shape.cuboids.len(), 3); // Source mask12: upper slab + two lower octants.
        assert!(stair
            .shape
            .cuboids
            .iter()
            .any(|cube| cube.min == [0.0, 0.0, 0.5]));
        let chest = state_props(
            "minecraft:chest",
            &[
                ("facing", "north"),
                ("type", "left"),
                ("waterlogged", "true"),
            ],
        );
        let chest = state_occlusion(&chest).unwrap();
        assert_eq!(chest.shape.cuboids[0].max[0], 1.0); // LEFT north connects east.
        let bell = state_props(
            "minecraft:bell",
            &[
                ("facing", "north"),
                ("attachment", "ceiling"),
                ("powered", "false"),
            ],
        );
        let bell = state_occlusion(&bell).unwrap();
        assert_eq!(bell.shape.cuboids.len(), 3);
        assert_eq!(bell.shape.cuboids[2].max[1], 1.0);
    }

    #[test]
    fn every_source022_state_selector_has_a_bounded_shape() {
        let mut checked = 0;
        for facing in ["north", "south", "west", "east"] {
            for attachment in ["floor", "ceiling", "single_wall", "double_wall"] {
                for powered in ["false", "true"] {
                    let bell = state_props(
                        "minecraft:bell",
                        &[
                            ("facing", facing),
                            ("attachment", attachment),
                            ("powered", powered),
                        ],
                    );
                    assert!(state_occlusion(&bell).unwrap().shape.cuboids.len() <= 3);
                    checked += 1;
                }
            }
            for kind in ["single", "left", "right"] {
                for waterlogged in ["false", "true"] {
                    let chest = state_props(
                        "minecraft:chest",
                        &[
                            ("facing", facing),
                            ("type", kind),
                            ("waterlogged", waterlogged),
                        ],
                    );
                    assert_eq!(state_occlusion(&chest).unwrap().shape.cuboids.len(), 1);
                    checked += 1;
                }
            }
            for half in ["top", "bottom"] {
                for shape in [
                    "straight",
                    "inner_left",
                    "inner_right",
                    "outer_left",
                    "outer_right",
                ] {
                    for waterlogged in ["false", "true"] {
                        for name in ["minecraft:cobblestone_stairs", "minecraft:spruce_stairs"] {
                            let stairs = state_props(
                                name,
                                &[
                                    ("facing", facing),
                                    ("half", half),
                                    ("shape", shape),
                                    ("waterlogged", waterlogged),
                                ],
                            );
                            assert!(state_occlusion(&stairs).unwrap().shape.cuboids.len() <= 5);
                            checked += 1;
                        }
                    }
                }
            }
        }
        for north in ["none", "low", "tall"] {
            for south in ["none", "low", "tall"] {
                for west in ["none", "low", "tall"] {
                    for east in ["none", "low", "tall"] {
                        for up in ["false", "true"] {
                            for waterlogged in ["false", "true"] {
                                let wall = state_props(
                                    "minecraft:cobblestone_wall",
                                    &[
                                        ("north", north),
                                        ("south", south),
                                        ("west", west),
                                        ("east", east),
                                        ("up", up),
                                        ("waterlogged", waterlogged),
                                    ],
                                );
                                assert!(state_occlusion(&wall).unwrap().shape.cuboids.len() <= 5);
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        for mask in 0..16 {
            for waterlogged in ["false", "true"] {
                let flags = ["south", "west", "north", "east"];
                let mut properties = flags
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        (
                            *name,
                            if mask & (1 << index) != 0 {
                                "true"
                            } else {
                                "false"
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                properties.push(("waterlogged", waterlogged));
                let fence = state_props("minecraft:spruce_fence", &properties);
                assert!(state_occlusion(&fence).unwrap().shape.cuboids.len() <= 5);
                checked += 1;
            }
        }
        assert_eq!(checked, 32 + 24 + 160 + 324 + 32);
        assert_eq!(
            state_occlusion(&state("minecraft:dirt_path"))
                .unwrap()
                .shape
                .cuboids
                .len(),
            1
        );
    }

    fn cube(min: [f64; 3], max: [f64; 3]) -> Shape {
        Shape::new(vec![Cuboid { min, max }]).unwrap()
    }

    #[test]
    fn native_directional_slice_and_can_occlude_gate_are_distinct() {
        let full = Shape::block();
        for direction in [
            Direction::Down,
            Direction::Up,
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ] {
            assert!(block_occudes(&full, &full, direction).unwrap());
            assert!(!face_occluded(false, &full, &full, direction).unwrap());
            assert!(!block_occudes(&full, &Shape::empty(), direction).unwrap());
        }
    }

    #[test]
    fn native_boolean_op_checks_the_fluid_slice_not_the_full_block_slice() {
        let fluid = cube([0.0; 3], [1.0, 8.0 / 9.0, 1.0]);
        assert!(block_occudes(&fluid, &Shape::block(), Direction::East).unwrap());
        let lower_block = cube([0.0; 3], [1.0, 0.5, 1.0]);
        assert!(!block_occudes(&fluid, &lower_block, Direction::East).unwrap());
        assert!(block_occudes(&fluid, &Shape::block(), Direction::Down).unwrap());
        assert!(!block_occudes(&fluid, &Shape::block(), Direction::Up).unwrap());
    }

    #[test]
    fn separated_boxes_keep_a_real_hole_in_the_directional_face() {
        let frame = Shape::new(vec![
            Cuboid {
                min: [0.0, 0.0, 0.0],
                max: [0.4, 1.0, 1.0],
            },
            Cuboid {
                min: [0.6, 0.0, 0.0],
                max: [1.0, 1.0, 1.0],
            },
        ])
        .unwrap();
        assert!(block_occudes(&frame, &Shape::block(), Direction::Up).unwrap());
    }

    #[test]
    fn near_coincident_source_coordinates_are_explicitly_rejected() {
        let shape = Shape::new(vec![
            Cuboid {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 1.0],
            },
            Cuboid {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 1.0 - 0.5e-7],
            },
        ])
        .unwrap();
        assert_eq!(
            block_occudes(&shape, &Shape::block(), Direction::Up),
            Err(Error::Precision)
        );
    }

    #[test]
    fn near_coincident_sliced_axis_coordinates_are_rejected() {
        let almost_full = cube([0.0, 0.0, 0.0], [1.0, 1.0 - 0.5e-7, 1.0]);
        assert_eq!(
            block_occudes(&almost_full, &Shape::block(), Direction::Up),
            Err(Error::Precision)
        );
    }
}
