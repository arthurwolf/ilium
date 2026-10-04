//! Worker-only saved fluid face assembly for the background CPU renderer.
//!
//! Texture handles reference independently imported selected-pack sprites.
//! Their normalized local UVs preserve native logical 0..16 face coordinates,
//! but do not claim an unobserved live stitched-atlas size or atlas-edge shrink.
//! The shared surface raster supplies its existing directional-light policy;
//! saved SkyLight/BlockLight were not retained by the current chunk decoder.
//! The CPU raster normalizes triangle winding and is double-sided. Each
//! source front top/side face is therefore emitted once: adding the native
//! reverse-side vertices would double-blend translucent pixels. This fixed
//! isometric camera looks from above, so native's conditional backward-up
//! face has no visible front-camera role here. Underside/overlay backface and
//! inverse-normal light parity with the live GPU path remain unverified.
use super::{
    fluids,
    native_fluid::{self, Cell, HorizontalFace},
    native_shape::{self, Direction},
    native_tint::{ClimatePolicy, ColorKind, NativeRequest, NativeTint},
    saved_binding::SavedBinding,
    tours::PreparedMap,
};
use crate::voxel_landscape::{
    assets::{
        bank::{TextureBank, TextureHandle},
        budget::{ByteBudget, Cancel},
        error::AssetError,
    },
    surface_fluid::{FluidFace, FluidMesh},
    surface_mesh::{BoundQuad, FaceMaterial, FaceOwner, MeshRegion},
};

const MAX_FACES: usize = 1_000_000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("saved fluid source state or corner: {0}")]
    Fluid(#[from] native_fluid::Error),
    #[error("saved fluid native material or shape: {0}")]
    Shape(#[from] native_shape::Error),
    #[error("saved fluid biome tint: {0}")]
    Tint(#[from] super::native_tint::Error),
    #[error("saved fluid texture or shared account: {0}")]
    Asset(#[from] AssetError),
    #[error("saved fluid renderer lacks an exact selected-pack sprite or admitted face")]
    MissingSprite,
    #[error("saved water face lacks the required native biome tint provider")]
    MissingTint,
    #[error("saved fluid position overflow or incomplete support halo")]
    Position,
}

fn offset(position: [i32; 3], step: [i32; 3]) -> Result<[i32; 3], Error> {
    Ok([
        position[0].checked_add(step[0]).ok_or(Error::Position)?,
        position[1].checked_add(step[1]).ok_or(Error::Position)?,
        position[2].checked_add(step[2]).ok_or(Error::Position)?,
    ])
}

fn sample(
    map: &PreparedMap,
    position: [i32; 3],
) -> Result<(Option<Cell>, native_shape::StateOcclusion, bool), Error> {
    let state = map.state(position).ok_or(Error::Position)?;
    let liquid = native_fluid::classify_state(state, position)?;
    let occlusion = native_shape::state_occlusion(state)?;
    let solid_material = native_shape::material_is_solid(state)?;
    Ok((liquid, occlusion, solid_material))
}

fn sample_height(map: &PreparedMap, kind: fluids::Kind, position: [i32; 3]) -> Result<f32, Error> {
    let state = map.state(position).ok_or(Error::Position)?;
    let liquid = native_fluid::classify_state(state, position)?;
    let solid_material = native_shape::material_is_solid(state)?;
    let above = native_fluid::classify_at(map, offset(position, [0, 1, 0])?)?;
    Ok(native_fluid::native_height(
        kind,
        liquid,
        above.is_some_and(|cell| cell.kind == kind),
        solid_material,
    ))
}

/// Horizontal components of pinned FlowingFluid.getFlow. The native falling
/// Y=-6 branch changes vector magnitude, not the top UV angle. This CPU path
/// uses standard atan2/sin/cos for the angle; native Mth lookup-bit parity is
/// separately unverified. All neighbor amounts/materials remain source-derived.
fn flow_xz(map: &PreparedMap, cell: Cell) -> Result<[f64; 2], Error> {
    let own = f32::from(cell.amount) / 9.0_f32;
    let mut flow = [0.0_f64; 2];
    // Direction.Plane.HORIZONTAL iterates NORTH, EAST, SOUTH, WEST in the
    // pinned gv$c initializer. Keep that order for Java's f64 additions.
    for step in [[0, 0, -1], [1, 0, 0], [0, 0, 1], [-1, 0, 0]] {
        let neighbor = offset(cell.java_position, step)?;
        let state = map.state(neighbor).ok_or(Error::Position)?;
        let fluid = native_fluid::classify_state(state, neighbor)?;
        // FlowingFluid checks Material.blocksMotion here, which is distinct
        // from LiquidBlockRenderer.getHeight's Material.isSolid query.
        let blocks_motion = native_shape::material_blocks_motion(state)?;
        if fluid.is_some_and(|value| value.kind != cell.kind) {
            continue;
        }
        let adjacent = fluid.map_or(0.0_f32, |value| f32::from(value.amount) / 9.0_f32);
        let slope = if adjacent == 0.0_f32 {
            if blocks_motion {
                0.0_f32
            } else {
                let below = native_fluid::classify_at(map, offset(neighbor, [0, -1, 0])?)?;
                below
                    .filter(|value| value.kind == cell.kind)
                    .map_or(0.0_f32, |value| {
                        own - (f32::from(value.amount) / 9.0_f32 - 8.0_f32 / 9.0_f32)
                    })
            }
        } else {
            own - adjacent
        };
        flow[0] += f64::from(step[0] as f32 * slope);
        flow[1] += f64::from(step[2] as f32 * slope);
    }
    Ok(flow)
}

fn local_top_uv(flow: [f64; 2]) -> [[f32; 2]; 4] {
    if flow == [0.0, 0.0] {
        return [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
    }
    let angle = flow[1].atan2(flow[0]) as f32 - 1.570_796_4_f32;
    let sine = angle.sin() * 0.25_f32;
    let cosine = angle.cos() * 0.25_f32;
    let local = [
        [8.0 + (-cosine - sine) * 16.0, 8.0 + (-cosine + sine) * 16.0],
        [8.0 + (-cosine + sine) * 16.0, 8.0 + (cosine + sine) * 16.0],
        [8.0 + (cosine + sine) * 16.0, 8.0 + (cosine - sine) * 16.0],
        [8.0 + (cosine - sine) * 16.0, 8.0 + (-cosine - sine) * 16.0],
    ];
    local.map(|[u, v]| [u / 16.0_f32, v / 16.0_f32])
}

fn local_side_uv(face: HorizontalFace, corners: [f32; 4], top_emitted: bool) -> [[f32; 2]; 4] {
    let [left, right] = match face {
        HorizontalFace::North => [corners[0], corners[3]],
        HorizontalFace::South => [corners[2], corners[1]],
        HorizontalFace::West => [corners[1], corners[0]],
        HorizontalFace::East => [corners[3], corners[2]],
    };
    // fen mutates each corner register by -0.001f when it emits the top;
    // both subsequent side geometry and side V values read those registers.
    let [left, right] = if top_emitted {
        [left - 0.001_f32, right - 0.001_f32]
    } else {
        [left, right]
    };
    [
        [0.0, (1.0 - left) * 0.5],
        [0.5, (1.0 - right) * 0.5],
        [0.5, 0.5],
        [0.0, 0.5],
    ]
}

fn side_height(face: HorizontalFace, corners: [f32; 4], top_emitted: bool) -> f32 {
    let height = match face {
        HorizontalFace::North => corners[0].max(corners[3]),
        HorizontalFace::South => corners[1].max(corners[2]),
        HorizontalFace::West => corners[0].max(corners[1]),
        HorizontalFace::East => corners[2].max(corners[3]),
    };
    if top_emitted {
        height - 0.001_f32
    } else {
        height
    }
}

fn push_face(
    faces: &mut Vec<FluidFace>,
    java_position: [i32; 3],
    number: u16,
    points: [[f64; 3]; 4],
    uv: [[f32; 2]; 4],
    normal: [f32; 3],
    material: FaceMaterial,
) -> Result<(), Error> {
    if faces.len() == MAX_FACES {
        return Err(Error::Asset(AssetError::Limit {
            resource: "native fluid faces",
            requested: faces.len() as u64 + 1,
            limit: MAX_FACES as u64,
        }));
    }
    let position = [java_position[0], java_position[2], java_position[1]];
    faces.push(FluidFace {
        position,
        quad: BoundQuad {
            points,
            uv,
            normal,
            material,
            cull_face: None,
            shade: true,
            part: 0,
            face: number,
        },
        owner: FaceOwner {
            position,
            part: 0,
            face: number,
            layer: material.layer,
        },
    });
    Ok(())
}

fn handle(
    bank: &TextureBank,
    id: &crate::voxel_landscape::assets::identity::ResourceId,
) -> Result<TextureHandle, Error> {
    bank.resolve(id).ok_or(Error::MissingSprite)
}

fn water_overlay_neighbor(name: &str) -> bool {
    // Exact observed LeavesBlock registrations (cqw) in source index009.
    matches!(
        name,
        "minecraft:acacia_leaves"
            | "minecraft:azalea_leaves"
            | "minecraft:birch_leaves"
            | "minecraft:flowering_azalea_leaves"
            | "minecraft:oak_leaves"
            | "minecraft:spruce_leaves"
    )
}

pub fn build(
    binding: &SavedBinding,
    bank: &TextureBank,
    tint: Option<&NativeTint>,
    world_seed: i64,
    blend_radius: u8,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> Result<Option<FluidMesh>, Error> {
    cancel.check()?;
    if binding.liquid_cells.is_empty() {
        return Ok(None);
    }
    let region = MeshRegion {
        minimum: binding.world.region.minimum,
        maximum: binding.world.region.maximum,
    };
    // Reserve the maximum temporary face vector before allocation. The final
    // FluidMesh reserves its exact face count; both charges coexist briefly.
    let upper = binding
        .liquid_cells
        .len()
        .checked_mul(6)
        .ok_or(Error::Position)?
        .min(MAX_FACES);
    let temporary = budget.reserve(
        4096 + upper as u64 * std::mem::size_of::<FluidFace>() as u64,
        cancel,
    )?;
    let mut faces = Vec::new();
    faces
        .try_reserve_exact(upper)
        .map_err(|_| AssetError::Allocation)?;
    for &cell in binding.liquid_cells.values() {
        // Sparse source tiles include a one-cell culling halo. Its liquid
        // states must remain queryable, but only this tile's core emits faces.
        if cell.java_position[0] < region.minimum[0]
            || cell.java_position[0] >= region.maximum[0]
            || cell.java_position[2] < region.minimum[1]
            || cell.java_position[2] >= region.maximum[1]
        {
            continue;
        }
        cancel.check()?;
        let current = sample(&binding.map, cell.java_position)?.1;
        let mut heights = [[0.0_f32; 3]; 3];
        for (row, row_heights) in heights.iter_mut().enumerate() {
            for (column, height) in row_heights.iter_mut().enumerate() {
                let neighbor = offset(cell.java_position, [column as i32 - 1, 0, row as i32 - 1])?;
                *height = sample_height(&binding.map, cell.kind, neighbor)?;
            }
        }
        let corners = native_fluid::native_corners(heights)?;
        let sprites = native_fluid::native_sprites(cell.kind)?;
        let still = handle(bank, &sprites.still)?;
        let flowing = handle(bank, &sprites.flowing)?;
        let overlay = sprites
            .overlay
            .as_ref()
            .map(|id| handle(bank, id))
            .transpose()?;
        let tint_rgb = if cell.kind == fluids::Kind::Water {
            tint.ok_or(Error::MissingTint)?
                .sample_native(
                    &binding.map,
                    NativeRequest {
                        source: binding.map.source(),
                        java_position: cell.java_position,
                        kind: ColorKind::Water,
                        climate_policy: ClimatePolicy::RenderEarlierWith1193,
                        world_seed: Some(world_seed),
                        blend_radius,
                    },
                    cancel,
                )?
                .rgb
                .map(|channel| f32::from(channel) / 255.0_f32)
        } else {
            [1.0; 3]
        };
        let material = |texture| FaceMaterial {
            texture,
            alpha: sprites.alpha,
            tint: tint_rgb,
            layer: if cell.kind == fluids::Kind::Water {
                3
            } else {
                0
            },
            normal_map: None,
            specular_map: None,
        };
        let face_visible = |direction, step, height| -> Result<bool, Error> {
            let neighbor_position = offset(cell.java_position, step)?;
            let (neighbor_fluid, neighbor, _) = sample(&binding.map, neighbor_position)?;
            Ok(native_fluid::native_face_visible(
                cell.kind,
                direction,
                height,
                native_fluid::BlockOcclusion {
                    can_occlude: current.can_occlude,
                    shape: &current.shape,
                },
                native_fluid::BlockOcclusion {
                    can_occlude: neighbor.can_occlude,
                    shape: &neighbor.shape,
                },
                neighbor_fluid,
            )?)
        };
        let mut visible = [false; 6];
        visible[0] = face_visible(
            Direction::Up,
            [0, 1, 0],
            corners
                .iter()
                .copied()
                .reduce(f32::min)
                .ok_or(Error::Position)?,
        )?;
        visible[1] = face_visible(Direction::Down, [0, -1, 0], 8.0_f32 / 9.0_f32)?;
        // The source mutates all four corner registers after the top branch,
        // before its final side-neighbor shape checks. Self/same-fluid gates
        // are independent of this height; face_visible retains their order.
        for (index, face, direction, step) in [
            (2, HorizontalFace::North, Direction::North, [0, 0, -1]),
            (3, HorizontalFace::South, Direction::South, [0, 0, 1]),
            (4, HorizontalFace::West, Direction::West, [-1, 0, 0]),
            (5, HorizontalFace::East, Direction::East, [1, 0, 0]),
        ] {
            visible[index] = face_visible(direction, step, side_height(face, corners, visible[0]))?;
        }
        if visible[0] {
            let flow = flow_xz(&binding.map, cell)?;
            push_face(
                &mut faces,
                cell.java_position,
                0,
                native_fluid::native_top_points(corners),
                local_top_uv(flow),
                [0.0, 0.0, 1.0],
                material(if flow == [0.0, 0.0] { still } else { flowing }),
            )?;
        }
        if visible[1] {
            let low = f64::from(0.001_f32);
            push_face(
                &mut faces,
                cell.java_position,
                1,
                [
                    [0.0, 1.0, low],
                    [0.0, 0.0, low],
                    [1.0, 0.0, low],
                    [1.0, 1.0, low],
                ],
                [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
                [0.0, 0.0, -1.0],
                material(still),
            )?;
        }
        for (index, face, step, normal) in [
            (2, HorizontalFace::North, [0, 0, -1], [0.0, -1.0, 0.0]),
            (3, HorizontalFace::South, [0, 0, 1], [0.0, 1.0, 0.0]),
            (4, HorizontalFace::West, [-1, 0, 0], [-1.0, 0.0, 0.0]),
            (5, HorizontalFace::East, [1, 0, 0], [1.0, 0.0, 0.0]),
        ] {
            if !visible[index] {
                continue;
            }
            let neighbor = binding
                .map
                .state(offset(cell.java_position, step)?)
                .ok_or(Error::Position)?;
            let texture =
                if cell.kind == fluids::Kind::Water && water_overlay_neighbor(&neighbor.name) {
                    overlay.ok_or(Error::MissingSprite)?
                } else {
                    flowing
                };
            push_face(
                &mut faces,
                cell.java_position,
                index as u16,
                native_fluid::native_side_points(face, corners, visible[0], visible[1]),
                local_side_uv(face, corners, visible[0]),
                normal,
                material(texture),
            )?;
        }
    }
    if faces.is_empty() {
        return Ok(None);
    }
    let mesh = FluidMesh::from_native_faces(faces, region, bank, budget, cancel)?;
    drop(temporary);
    Ok(Some(mesh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_local_uv_preserves_native_logical_face_layout_without_fake_atlas_size() {
        assert_eq!(
            local_top_uv([0.0, 0.0]),
            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]
        );
        assert_eq!(
            local_side_uv(HorizontalFace::North, [0.5, 0.2, 0.3, 1.0], false),
            [[0.0, 0.25], [0.5, 0.0], [0.5, 0.5], [0.0, 0.5]]
        );
        let lowered = local_side_uv(HorizontalFace::North, [0.5, 0.2, 0.3, 1.0], true);
        assert!((lowered[0][1] - 0.2505).abs() < 1.0e-6);
        assert!((lowered[1][1] - 0.0005).abs() < 1.0e-6);
        assert_eq!(
            side_height(HorizontalFace::North, [0.5, 0.2, 0.3, 1.0], false),
            1.0
        );
        assert!(
            (side_height(HorizontalFace::North, [0.5, 0.2, 0.3, 1.0], true) - 0.999).abs() < 1.0e-6
        );
    }

    #[test]
    fn overlay_selection_is_exact_captured_leaves_class_not_texture_alpha() {
        assert!(water_overlay_neighbor("minecraft:oak_leaves"));
        assert!(water_overlay_neighbor("minecraft:flowering_azalea_leaves"));
        assert!(!water_overlay_neighbor("minecraft:glass_pane"));
        assert!(!water_overlay_neighbor("minecraft:stone"));
    }
}
