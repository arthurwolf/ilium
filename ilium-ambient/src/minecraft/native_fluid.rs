//! Exact saved liquid occupancy before model and fluid-face binding.
//!
//! The source palette remains authoritative. These samples never replace a
//! waterlogged solid or assert that a partial solid's face is occluding.
use super::{
    chunk::BlockState,
    fluids,
    native_shape::{self, Direction, Shape},
    render_cells::RenderCells,
    tours::PreparedMap,
};
use crate::voxel_landscape::{assets::identity::ResourceId, surface_mesh::AlphaMode};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub java_position: [i32; 3],
    pub kind: fluids::Kind,
    /// Native flowing-fluid amount, 1..=8 (8 for a source or falling state).
    pub amount: u8,
    pub falling: bool,
    /// The separate solid model must also be bound at this position.
    pub waterlogged: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("saved liquid at {position:?} has invalid native state: {source}")]
    InvalidState {
        position: [i32; 3],
        #[source]
        source: fluids::Error,
    },
    #[error("saved waterlogged state at {0:?} lacks captured native getFluidState behavior")]
    WaterloggedBehavior([i32; 3]),
    #[error("saved liquid at {0:?} has no exact palette state")]
    MissingState([i32; 3]),
    #[error("saved liquid coordinate conversion overflow")]
    Position,
    #[error("native liquid corner had no positive source height")]
    Corner,
    #[error("native liquid occlusion shape: {0}")]
    Shape(#[from] native_shape::Error),
    #[error("native stitched sprite bounds or frame size are invalid")]
    Atlas,
    #[error("native saved-world sky, block, or emission light is outside 0..=15")]
    Light,
}

#[derive(Clone, Copy)]
pub struct BlockOcclusion<'a> {
    /// Exact BlockStateBase.canOcclude, not model alpha or collision solidity.
    pub can_occlude: bool,
    /// Exact dynamic BlockState.getOcclusionShape at this position.
    pub shape: &'a Shape,
}

/// Pinned `LiquidBlockRenderer` source-order face test. Top has no self-shape
/// test; other faces first test the current block's opposite face at height1.
/// All faces reject a neighbor with the same native fluid type, then use
/// `Shapes.blockOccudes` against the neighbor's actual occlusion shape.
pub fn native_face_visible(
    kind: fluids::Kind,
    face: Direction,
    height: f32,
    current: BlockOcclusion<'_>,
    neighbor: BlockOcclusion<'_>,
    neighbor_fluid: Option<Cell>,
) -> Result<bool, Error> {
    if face != Direction::Up
        && native_shape::face_occluded(
            current.can_occlude,
            &Shape::block(),
            current.shape,
            face.opposite(),
        )?
    {
        return Ok(false);
    }
    if neighbor_fluid.is_some_and(|cell| cell.kind == kind) {
        return Ok(false);
    }
    let fluid_box = Shape::fluid_box(height)?;
    Ok(!native_shape::face_occluded(
        neighbor.can_occlude,
        &fluid_box,
        neighbor.shape,
        face,
    )?)
}

/// Index liquid blocks from exact admitted positions. The source solid remains
/// separately present for every waterlogged cell.
/// The renderer must then use native per-face culling, corner height, UV, flow
/// and kind-specific textures before accepting this as painted geometry.
pub fn classify(cells: &RenderCells) -> Result<BTreeMap<[i32; 3], Cell>, Error> {
    let mut output = BTreeMap::new();
    for &position in &cells.positions {
        if let Some(cell) = classify_at(&cells.map, position)? {
            let render = [position[0], position[2], position[1]];
            if output.insert(render, cell).is_some() {
                return Err(Error::Position);
            }
        }
    }
    Ok(output)
}

/// Read a real saved support-neighbor, including cells outside the render
/// core. A missing saved palette/chunk is an error, never guessed air.
pub fn classify_at(map: &PreparedMap, position: [i32; 3]) -> Result<Option<Cell>, Error> {
    let state = map.state(position).ok_or(Error::MissingState(position))?;
    classify_state(state, position)
}

pub fn classify_state(state: &BlockState, position: [i32; 3]) -> Result<Option<Cell>, Error> {
    match fluids::parse(state).map_err(|source| Error::InvalidState { position, source })? {
        fluids::State::Empty => Ok(None),
        fluids::State::Waterlogged => {
            if !native_waterlogged_source(&state.name) {
                return Err(Error::WaterloggedBehavior(position));
            }
            Ok(Some(Cell {
                java_position: position,
                kind: fluids::Kind::Water,
                amount: 8,
                falling: false,
                waterlogged: true,
            }))
        }
        fluids::State::Liquid(liquid) => Ok(Some(Cell {
            java_position: position,
            kind: liquid.kind(),
            amount: liquid.amount(),
            falling: liquid.falling(),
            waterlogged: false,
        })),
    }
}

/// Names in the actual four-map projected palettes with a captured 1.19.3 waterlogged
/// `getFluidState` returning source water when the property is true. Unknown
/// names stay typed prerequisites rather than inheriting an assumed behavior.
fn native_waterlogged_source(name: &str) -> bool {
    matches!(
        name,
        "minecraft:acacia_leaves"
            | "minecraft:small_dripleaf"
            | "minecraft:big_dripleaf"
            | "minecraft:big_dripleaf_stem"
            | "minecraft:dark_oak_fence"
            | "minecraft:dark_oak_slab"
            | "minecraft:dark_oak_stairs"
            | "minecraft:dark_oak_trapdoor"
            | "minecraft:jungle_fence"
            | "minecraft:mossy_stone_brick_slab"
            | "minecraft:mossy_stone_brick_stairs"
            | "minecraft:pointed_dripstone"
            | "minecraft:spruce_slab"
            | "minecraft:stone_brick_slab"
            | "minecraft:stone_brick_stairs"
            | "minecraft:birch_leaves"
            | "minecraft:oak_leaves"
            | "minecraft:campfire"
            | "minecraft:chain"
            | "minecraft:chest"
            | "minecraft:cobblestone_stairs"
            | "minecraft:spruce_stairs"
            | "minecraft:cobblestone_wall"
            | "minecraft:glass_pane"
            | "minecraft:glow_lichen"
            | "minecraft:spruce_fence"
            | "minecraft:spruce_trapdoor"
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeSprites {
    pub still: ResourceId,
    pub flowing: ResourceId,
    pub overlay: Option<ResourceId>,
    pub alpha: AlphaMode,
}

/// Sprite names are exact pinned JAR members. The overlay is selected by the
/// renderer only for a neighboring HalfTransparentBlock or LeavesBlock, never
/// by alpha inspection. The native render-layer registrations give translucent
/// water and solid lava.
pub fn native_sprites(
    kind: fluids::Kind,
) -> Result<NativeSprites, crate::voxel_landscape::assets::AssetError> {
    let (still, flowing, overlay, alpha) = match kind {
        fluids::Kind::Water => (
            "minecraft:block/water_still",
            "minecraft:block/water_flow",
            Some("minecraft:block/water_overlay"),
            AlphaMode::NativeBlend,
        ),
        fluids::Kind::Lava => (
            "minecraft:block/lava_still",
            "minecraft:block/lava_flow",
            None,
            AlphaMode::NativeSolid,
        ),
    };
    Ok(NativeSprites {
        still: ResourceId::parse(still)?,
        flowing: ResourceId::parse(flowing)?,
        overlay: overlay.map(ResourceId::parse).transpose()?,
        alpha,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalFace {
    North,
    South,
    West,
    East,
}

/// Corner order follows LiquidBlockRenderer's local variables 36, 38, 37, 35:
/// northwest, southwest, southeast, northeast in Java X/Z. The renderer's
/// own top vertices subtract 0.001f *after* corner-height calculation.
pub fn native_top_points(corners: [f32; 4]) -> [[f64; 3]; 4] {
    let lowered = corners.map(|height| f64::from(height - 0.001_f32));
    [
        [0.0, 0.0, lowered[0]],
        [0.0, 1.0, lowered[1]],
        [1.0, 1.0, lowered[2]],
        [1.0, 0.0, lowered[3]],
    ]
}

/// Source-order side vertices for renderer axes [Java X, Java Z, Java Y].
/// The top edge is lowered by 0.001f only when the top face was emitted; the
/// bottom edge is 0.001f only when the bottom face was emitted. In the native
/// renderer the top branch mutates its four corner registers before side faces
/// use them (renderer bytecode 688..750, then side bytecode 1675..1917).
/// UV, light, sprite choice and face occlusion remain separate inputs.
pub fn native_side_points(
    face: HorizontalFace,
    corners: [f32; 4],
    top_emitted: bool,
    bottom_visible: bool,
) -> [[f64; 3]; 4] {
    let inset = f64::from(0.001_f32);
    let low = if bottom_visible { inset } else { 0.0 };
    let [northwest, southwest, southeast, northeast] = corners.map(|height| {
        f64::from(if top_emitted {
            height - 0.001_f32
        } else {
            height
        })
    });
    match face {
        HorizontalFace::North => [
            [0.0, inset, northwest],
            [1.0, inset, northeast],
            [1.0, inset, low],
            [0.0, inset, low],
        ],
        HorizontalFace::South => [
            [1.0, 1.0 - inset, southeast],
            [0.0, 1.0 - inset, southwest],
            [0.0, 1.0 - inset, low],
            [1.0, 1.0 - inset, low],
        ],
        HorizontalFace::West => [
            [inset, 1.0, southwest],
            [inset, 0.0, northwest],
            [inset, 0.0, low],
            [inset, 1.0, low],
        ],
        HorizontalFace::East => [
            [1.0 - inset, 0.0, northeast],
            [1.0 - inset, 1.0, southeast],
            [1.0 - inset, 1.0, low],
            [1.0 - inset, 0.0, low],
        ],
    }
}

/// LiquidBlockRenderer.getHeight from the pinned Java 1.19.3 class body.
/// The caller must supply actual material/above-fluid facts; palette texture
/// alpha and model names do not establish them.
pub fn native_height(
    kind: fluids::Kind,
    cell: Option<Cell>,
    same_kind_above: bool,
    solid_material: bool,
) -> f32 {
    match cell.filter(|cell| cell.kind == kind) {
        Some(_) if same_kind_above => 1.0,
        Some(cell) => f32::from(cell.amount) / 9.0,
        None if solid_material => -1.0,
        None => 0.0,
    }
}

/// LiquidBlockRenderer.calculateAverageHeight. The diagonal getter is invoked
/// only when either adjacent height is positive, exactly as native bytecode.
/// Java f32 addition order is kept: diagonal, center, second, first.
pub fn native_corner_height(
    center: f32,
    first: f32,
    second: f32,
    diagonal: impl FnOnce() -> f32,
) -> Result<f32, Error> {
    if first >= 1.0 || second >= 1.0 {
        return Ok(1.0);
    }
    let mut weighted = [0.0_f32; 2];
    if first > 0.0 || second > 0.0 {
        let diagonal = diagonal();
        if diagonal >= 1.0 {
            return Ok(1.0);
        }
        add_native_height(&mut weighted, diagonal);
    }
    add_native_height(&mut weighted, center);
    add_native_height(&mut weighted, second);
    add_native_height(&mut weighted, first);
    if weighted[1] <= 0.0 {
        return Err(Error::Corner);
    }
    Ok(weighted[0] / weighted[1])
}

/// Calculates the four native top corners from a 3x3 X/Z neighborhood of
/// already source-qualified `native_height` values. Rows are north-to-south,
/// columns west-to-east. The caller must read the real halo and must not
/// replace missing saved chunks with zero-height air.
pub fn native_corners(heights: [[f32; 3]; 3]) -> Result<[f32; 4], Error> {
    let center = heights[1][1];
    if center >= 1.0 {
        return Ok([1.0; 4]);
    }
    let north = heights[0][1];
    let south = heights[2][1];
    let west = heights[1][0];
    let east = heights[1][2];
    Ok([
        native_corner_height(center, north, west, || heights[0][0])?,
        native_corner_height(center, south, west, || heights[2][0])?,
        native_corner_height(center, south, east, || heights[2][2])?,
        native_corner_height(center, north, east, || heights[0][2])?,
    ])
}

/// `TextureAtlasSprite` stores these normalized endpoints as Java `float`.
/// The dimensions and position are stitched-atlas inputs, not raw PNG size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtlasSprite {
    u0: f32,
    u1: f32,
    v0: f32,
    v1: f32,
    frame_size: [u32; 2],
}

impl AtlasSprite {
    /// Exact constructor arithmetic from `fol.<init>` in source010.
    pub fn new(
        atlas_size: [u32; 2],
        frame_size: [u32; 2],
        atlas_origin: [u32; 2],
    ) -> Result<Self, Error> {
        if atlas_size.contains(&0)
            || frame_size.contains(&0)
            || atlas_size
                .iter()
                .chain(frame_size.iter())
                .chain(atlas_origin.iter())
                .any(|&n| n > i32::MAX as u32)
            || atlas_origin[0]
                .checked_add(frame_size[0])
                .is_none_or(|end| end > atlas_size[0])
            || atlas_origin[1]
                .checked_add(frame_size[1])
                .is_none_or(|end| end > atlas_size[1])
        {
            return Err(Error::Atlas);
        }
        let u0 = atlas_origin[0] as f32 / atlas_size[0] as f32;
        let u1 = (atlas_origin[0] + frame_size[0]) as f32 / atlas_size[0] as f32;
        let v0 = atlas_origin[1] as f32 / atlas_size[1] as f32;
        let v1 = (atlas_origin[1] + frame_size[1]) as f32 / atlas_size[1] as f32;
        if !(u1 > u0 && v1 > v0) {
            return Err(Error::Atlas);
        }
        Ok(Self {
            u0,
            u1,
            v0,
            v1,
            frame_size,
        })
    }

    /// `fol.getU(double)`/`getV(double)` narrow to float before multiply.
    pub fn uv(self, pixel: [f64; 2]) -> [f32; 2] {
        let du = self.u1 - self.u0;
        let dv = self.v1 - self.v0;
        [
            self.u0 + du * pixel[0] as f32 / 16.0_f32,
            self.v0 + dv * pixel[1] as f32 / 16.0_f32,
        ]
    }

    /// `fol.uvShrinkRatio` divides four by the native inferred atlas size.
    pub fn shrink_ratio(self) -> f32 {
        let width = self.frame_size[0] as f32 / (self.u1 - self.u0);
        let height = self.frame_size[1] as f32 / (self.v1 - self.v0);
        4.0_f32 / width.max(height)
    }

    /// Top-face `fen` averages each axis in vertex order then calls `Mth.lerp`
    /// independently for each coordinate. No clamping is part of that method.
    pub fn shrink_top(self, uv: [[f32; 2]; 4]) -> [[f32; 2]; 4] {
        let mean_u = (((uv[0][0] + uv[1][0]) + uv[2][0]) + uv[3][0]) / 4.0_f32;
        let mean_v = (((uv[0][1] + uv[1][1]) + uv[2][1]) + uv[3][1]) / 4.0_f32;
        let ratio = self.shrink_ratio();
        uv.map(|[u, v]| [u + ratio * (mean_u - u), v + ratio * (mean_v - v)])
    }

    /// Still-water/lava top UV order from `fen` bytecode 780..838.
    pub fn still_top_uv(self) -> [[f32; 2]; 4] {
        let uv = [
            self.uv([0.0, 0.0]),
            self.uv([0.0, 16.0]),
            self.uv([16.0, 16.0]),
            self.uv([16.0, 0.0]),
        ];
        self.shrink_top(uv)
    }

    /// Flowing top UV order from `fen` bytecode 841..1065. These two terms
    /// are `Mth.sin(angle)*0.25` and `Mth.cos(angle)*0.25`, where the angle
    /// comes from the actual saved-fluid flow vector. Passing guessed values
    /// or using this path for a still top is a caller contract violation.
    pub fn flowing_top_uv(
        self,
        sine_quarter: f32,
        cosine_quarter: f32,
    ) -> Result<[[f32; 2]; 4], Error> {
        if !sine_quarter.is_finite() || !cosine_quarter.is_finite() {
            return Err(Error::Atlas);
        }
        let uv = [
            self.uv([
                f64::from(8.0 + (-cosine_quarter - sine_quarter) * 16.0),
                f64::from(8.0 + (-cosine_quarter + sine_quarter) * 16.0),
            ]),
            self.uv([
                f64::from(8.0 + (-cosine_quarter + sine_quarter) * 16.0),
                f64::from(8.0 + (cosine_quarter + sine_quarter) * 16.0),
            ]),
            self.uv([
                f64::from(8.0 + (cosine_quarter + sine_quarter) * 16.0),
                f64::from(8.0 + (cosine_quarter - sine_quarter) * 16.0),
            ]),
            self.uv([
                f64::from(8.0 + (cosine_quarter - sine_quarter) * 16.0),
                f64::from(8.0 + (-cosine_quarter - sine_quarter) * 16.0),
            ]),
        ];
        Ok(self.shrink_top(uv))
    }

    /// Horizontal side order matches `native_side_points`: top-left,
    /// top-right, bottom-right, bottom-left. Source UVs use half a sprite for
    /// each vertical unit; no guessed neighboring bank height is involved.
    pub fn side_uv(self, face: HorizontalFace, corners: [f32; 4]) -> [[f32; 2]; 4] {
        let [left, right] = match face {
            HorizontalFace::North => [corners[0], corners[3]],
            HorizontalFace::South => [corners[2], corners[1]],
            HorizontalFace::West => [corners[1], corners[0]],
            HorizontalFace::East => [corners[3], corners[2]],
        };
        let u0 = self.uv([0.0, 0.0])[0];
        let u8 = self.uv([8.0, 0.0])[0];
        let v_left = self.uv([0.0, f64::from((1.0_f32 - left) * 16.0_f32 * 0.5_f32)])[1];
        let v_right = self.uv([0.0, f64::from((1.0_f32 - right) * 16.0_f32 * 0.5_f32)])[1];
        let v8 = self.uv([0.0, 8.0])[1];
        [[u0, v_left], [u8, v_right], [u8, v8], [u0, v8]]
    }
}

/// `FluidState.shouldRenderBackwardUpFace` scans same-Y 3×3 neighbors, including
/// the center. The caller supplies exact saved fluid identity and world-aware
/// BlockState.isSolidRender for each position; canOcclude is a different flag.
pub fn native_backward_up_face(
    kind: fluids::Kind,
    same_y: [[(Option<fluids::Kind>, bool); 3]; 3],
) -> bool {
    same_y
        .into_iter()
        .flatten()
        .any(|(neighbor, solid_render)| neighbor != Some(kind) && !solid_render)
}

/// `LevelRenderer.getLightColor` requires world sky/block light and state
/// emission. Values must come from the saved-world lighting adapter.
pub fn native_world_packed_light(
    sky: u8,
    block: u8,
    emission: u8,
    emissive_rendering: bool,
) -> Result<u32, Error> {
    if sky > 15 || block > 15 || emission > 15 {
        return Err(Error::Light);
    }
    Ok(if emissive_rendering {
        15_728_880
    } else {
        (u32::from(sky) << 20) | (u32::from(block.max(emission)) << 4)
    })
}

/// `LiquidBlockRenderer.getLightColor` independently maximizes low-byte and
/// high-byte lanes for this position and the saved position immediately above.
pub fn native_fluid_packed_light(here: u32, above: u32) -> u32 {
    (here & 255).max(above & 255) | (((here >> 16) & 255).max((above >> 16) & 255) << 16)
}

fn add_native_height(weighted: &mut [f32; 2], height: f32) {
    if height >= 0.8 {
        weighted[0] += height * 10.0;
        weighted[1] += 10.0;
    } else if height >= 0.0 {
        weighted[0] += height;
        weighted[1] += 1.0;
    }
}

#[cfg(test)]
#[path = "native_fluid_tests.rs"]
mod tests;
