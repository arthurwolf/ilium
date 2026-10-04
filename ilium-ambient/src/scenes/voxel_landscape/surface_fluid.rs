//! Camera-independent fluid surfaces. Input contains a one-cell XY/Z halo and a
//! separate solid occupancy set, so a translucent surface never removes its bed.
use super::{
    assets::{
        bank::{TextureBank, TextureHandle},
        budget::{ByteBudget, Cancel, Reservation},
        error::{AssetError, Result},
        identity::Digest256,
        metadata,
        texture::Encoding,
    },
    surface_mesh::{AlphaMode, BoundQuad, FaceMaterial, FaceOwner, MeshRegion},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidCell {
    /// 0 = source, 1..7 = decreasing height, 8 = falling/full-height.
    pub level: u8,
    /// Linear RGB tint supplied by the selected biome source.
    pub tint: [f32; 3],
}
impl FluidCell {
    pub fn new(level: u8, tint: [f32; 3]) -> Result<Self> {
        if level > 8
            || tint
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(metadata::invalid(
                "invalid fluid level or linear biome tint",
            ));
        }
        Ok(Self { level, tint })
    }
    fn height(self) -> f64 {
        if self.level == 0 || self.level == 8 {
            1.0
        } else {
            f64::from(8 - self.level) / 8.0
        }
    }
}

#[derive(Clone, Debug)]
pub struct FluidFace {
    pub position: [i32; 3],
    pub quad: BoundQuad,
    pub owner: FaceOwner,
}

#[derive(Debug)]
pub struct FluidMesh {
    pub faces: Vec<FluidFace>,
    pub bank: Digest256,
    pub region: MeshRegion,
    _reservation: Reservation,
}
impl FluidMesh {
    pub(crate) fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self._reservation.belongs_to(budget)
    }
    /// Publish source-native water/lava faces built from exact saved states.
    /// This path deliberately permits a waterlogged solid at the same anchor.
    /// It is separate from `build` because generated `FluidCell` levels and
    /// corner averaging do not implement the pinned 1.19.3 renderer.
    pub fn from_native_faces(
        faces: Vec<FluidFace>,
        region: MeshRegion,
        bank: &TextureBank,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        if faces.is_empty()
            || faces.len() > 1_000_000
            || (0..2).any(|axis| {
                region.minimum[axis] >= region.maximum[axis]
                    || i64::from(region.maximum[axis]) - i64::from(region.minimum[axis]) > 2048
            })
        {
            return Err(metadata::invalid("native fluid face count or region"));
        }
        for face in &faces {
            cancel.check()?;
            if face.position != face.owner.position
                || face.owner.part != face.quad.part
                || face.owner.face != face.quad.face
                || face.owner.layer != face.quad.material.layer
                || face.position[0] < region.minimum[0]
                || face.position[0] >= region.maximum[0]
                || face.position[1] < region.minimum[1]
                || face.position[1] >= region.maximum[1]
                || !matches!(
                    face.quad.material.alpha,
                    AlphaMode::NativeBlend | AlphaMode::NativeSolid
                )
                || face
                    .quad
                    .points
                    .iter()
                    .flatten()
                    .any(|value| !value.is_finite())
                || face
                    .quad
                    .uv
                    .iter()
                    .flatten()
                    .any(|value| !value.is_finite())
                || face.quad.normal.iter().any(|value| !value.is_finite())
                || face
                    .quad
                    .material
                    .tint
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            {
                return Err(metadata::invalid("invalid native fluid face"));
            }
            let texture = bank
                .texture(face.quad.material.texture)
                .ok_or_else(|| metadata::invalid("stale native fluid texture"))?;
            if !texture.uses_budget(budget) || texture.encoding() != Encoding::SrgbColor {
                return Err(metadata::invalid(
                    "native fluid texture differs from scene account",
                ));
            }
        }
        // The assembler reserves up to six slots per liquid cell, then culls
        // most of them in dense water. Vec keeps that allocation when moved
        // here, so charge its retained capacity rather than its visible len.
        let face_storage = (faces.capacity() as u64)
            .checked_mul(std::mem::size_of::<FluidFace>() as u64)
            .and_then(|bytes| bytes.checked_add(4096))
            .ok_or(AssetError::Allocation)?;
        let reservation = budget.reserve(face_storage, cancel)?;
        Ok(Self {
            faces,
            bank: bank.identity(),
            region,
            _reservation: reservation,
        })
    }
    /// Water and solid samples outside the region form a halo; unknown is never
    /// silently declared solid. The caller must supply all visible bed and bank
    /// models separately to the opaque model mesh.
    #[expect(
        clippy::too_many_arguments,
        reason = "fluid material, geometry, bank and cancellation are independently validated"
    )]
    pub fn build(
        cells: &BTreeMap<[i32; 3], FluidCell>,
        solids: &BTreeSet<[i32; 3]>,
        region: MeshRegion,
        bank: &TextureBank,
        diffuse: TextureHandle,
        max_faces: usize,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        if max_faces == 0 || max_faces > 1_000_000 || cells.len() > 1_000_000 {
            return Err(metadata::invalid("fluid mesh count limit"));
        }
        if (0..2).any(|axis| {
            region.minimum[axis] >= region.maximum[axis]
                || i64::from(region.maximum[axis]) - i64::from(region.minimum[axis]) > 2048
        }) {
            return Err(metadata::invalid("fluid mesh region bounds"));
        }
        let texture = bank
            .texture(diffuse)
            .ok_or_else(|| metadata::invalid("stale fluid handle"))?;
        if !texture.uses_budget(budget) || texture.encoding() != Encoding::SrgbColor {
            return Err(metadata::invalid(
                "fluid diffuse must be selected color in scene account",
            ));
        }
        let mut faces = Vec::new();
        faces
            .try_reserve_exact(max_faces.min(cells.len().saturating_mul(5)))
            .map_err(|_| AssetError::Allocation)?;
        for (&position, &cell) in cells {
            cancel.check()?;
            FluidCell::new(cell.level, cell.tint)?;
            if position[0] < region.minimum[0]
                || position[0] >= region.maximum[0]
                || position[1] < region.minimum[1]
                || position[1] >= region.maximum[1]
            {
                continue;
            }
            if solids.contains(&position) {
                return Err(metadata::invalid("fluid and solid occupy the same cell"));
            }
            let material = FaceMaterial {
                texture: diffuse,
                alpha: AlphaMode::Blend,
                tint: cell.tint,
                layer: 0,
                normal_map: None,
                specular_map: None,
            };
            // Vertex order: southwest, southeast, northeast, northwest.
            let corners = [
                corner_height(cells, position, 0, 0),
                corner_height(cells, position, 1, 0),
                corner_height(cells, position, 1, 1),
                corner_height(cells, position, 0, 1),
            ];
            if neighbor(cells, position, [0, 0, 1]).is_none() {
                push(
                    &mut faces,
                    max_faces,
                    position,
                    0,
                    material,
                    [
                        [0.0, 0.0, corners[0]],
                        [1.0, 0.0, corners[1]],
                        [1.0, 1.0, corners[2]],
                        [0.0, 1.0, corners[3]],
                    ],
                    [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                    [0.0, 0.0, 1.0],
                )?;
            }
            for (face, step, edge, normal) in [
                (1, [0, -1, 0], [0, 1], [0.0, -1.0, 0.0]),
                (2, [1, 0, 0], [1, 2], [1.0, 0.0, 0.0]),
                (3, [0, 1, 0], [2, 3], [0.0, 1.0, 0.0]),
                (4, [-1, 0, 0], [3, 0], [-1.0, 0.0, 0.0]),
            ] {
                let Some(adjacent_position) = position.checked_add(step) else {
                    continue;
                };
                if solids.contains(&adjacent_position) {
                    continue;
                }
                let neighbor_cell = cells.get(&adjacent_position).copied();
                let [a, b] = edge;
                let lower_a = neighbor_cell.map_or(0.0, |_| {
                    corner_height(
                        cells,
                        adjacent_position,
                        if step[0] < 0 {
                            1
                        } else if step[0] > 0 {
                            0
                        } else {
                            a as i32 % 2
                        },
                        if step[1] < 0 {
                            1
                        } else if step[1] > 0 {
                            0
                        } else {
                            a as i32 / 2
                        },
                    )
                });
                let lower_b = neighbor_cell.map_or(0.0, |_| {
                    corner_height(
                        cells,
                        adjacent_position,
                        if step[0] < 0 {
                            1
                        } else if step[0] > 0 {
                            0
                        } else {
                            b as i32 % 2
                        },
                        if step[1] < 0 {
                            1
                        } else if step[1] > 0 {
                            0
                        } else {
                            b as i32 / 2
                        },
                    )
                });
                if lower_a + 1e-9 >= corners[a] && lower_b + 1e-9 >= corners[b] {
                    continue;
                }
                let xy = |corner: usize| match corner {
                    0 => [0.0, 0.0],
                    1 => [1.0, 0.0],
                    2 => [1.0, 1.0],
                    _ => [0.0, 1.0],
                };
                let pa = xy(a);
                let pb = xy(b);
                push(
                    &mut faces,
                    max_faces,
                    position,
                    face,
                    material,
                    [
                        [pa[0], pa[1], lower_a],
                        [pb[0], pb[1], lower_b],
                        [pb[0], pb[1], corners[b]],
                        [pa[0], pa[1], corners[a]],
                    ],
                    [
                        [0.0, lower_a as f32],
                        [1.0, lower_b as f32],
                        [1.0, corners[b] as f32],
                        [0.0, corners[a] as f32],
                    ],
                    normal,
                )?;
            }
        }
        let reservation = budget.reserve(
            4096 + faces.len() as u64 * std::mem::size_of::<FluidFace>() as u64,
            cancel,
        )?;
        Ok(Self {
            faces,
            bank: bank.identity(),
            region,
            _reservation: reservation,
        })
    }
}
#[expect(
    clippy::too_many_arguments,
    reason = "one face carries distinct geometry, UV, normal and material"
)]
fn push(
    faces: &mut Vec<FluidFace>,
    limit: usize,
    position: [i32; 3],
    face: u16,
    material: FaceMaterial,
    points: [[f64; 3]; 4],
    uv: [[f32; 2]; 4],
    normal: [f32; 3],
) -> Result<()> {
    if faces.len() >= limit {
        return Err(AssetError::Limit {
            resource: "fluid mesh faces",
            requested: faces.len() as u64 + 1,
            limit: limit as u64,
        });
    }
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
            face,
        },
        owner: FaceOwner {
            position,
            part: 0,
            face,
            layer: material.layer,
        },
    });
    Ok(())
}
fn neighbor(
    cells: &BTreeMap<[i32; 3], FluidCell>,
    position: [i32; 3],
    step: [i32; 3],
) -> Option<FluidCell> {
    cells.get(&position.checked_add(step)?).copied()
}
trait CheckedAdd3 {
    fn checked_add(self, step: [i32; 3]) -> Option<Self>
    where
        Self: Sized;
}
impl CheckedAdd3 for [i32; 3] {
    fn checked_add(self, step: [i32; 3]) -> Option<Self> {
        Some([
            self[0].checked_add(step[0])?,
            self[1].checked_add(step[1])?,
            self[2].checked_add(step[2])?,
        ])
    }
}
fn corner_height(
    cells: &BTreeMap<[i32; 3], FluidCell>,
    position: [i32; 3],
    dx: i32,
    dy: i32,
) -> f64 {
    let mut total = 0.0;
    let mut count = 0.0;
    for offset_x in [dx - 1, dx] {
        for offset_y in [dy - 1, dy] {
            let Some(sample) = position.checked_add([offset_x, offset_y, 0]) else {
                continue;
            };
            let Some(cell) = cells.get(&sample) else {
                continue;
            };
            if neighbor(cells, sample, [0, 0, 1]).is_some() {
                return 1.0;
            }
            total += cell.height();
            count += 1.0;
        }
    }
    if count == 0.0 {
        0.0
    } else {
        total / count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel_landscape::assets::{
        animation::MissingAnimation,
        bank::{TextureBankBuilder, TextureRequirement},
        budget::Limits,
        identity::{OriginKind, ResourceId},
        review::{fixture_origin, fixture_review},
        texture::fixture_texture,
    };
    use std::sync::atomic::AtomicBool;

    #[test]
    fn mostly_culled_native_faces_keep_their_full_capacity_charged() {
        let budget = ByteBudget::new(256 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let id = ResourceId::parse("test:block/water").unwrap();
        let mut builder = TextureBankBuilder::new(
            fixture_review(),
            vec![],
            Limits::default(),
            budget.clone(),
            cancel,
        )
        .unwrap();
        let texture = fixture_texture(
            [1, 1],
            &[30, 80, 200, 153],
            None,
            &MissingAnimation::StaticImage,
            Encoding::SrgbColor,
            fixture_origin(OriginKind::DiagnosticFixture),
            &budget,
        );
        builder.insert(id.clone(), texture, cancel).unwrap();
        let bank = builder
            .finish(vec![TextureRequirement::selected_color(id.clone())], cancel)
            .unwrap();
        let handle = bank.resolve(&id).unwrap();
        let position = [0, 0, 0];
        let mut faces = Vec::with_capacity(64);
        faces.push(FluidFace {
            position,
            owner: FaceOwner {
                position,
                part: 0,
                face: 0,
                layer: 3,
            },
            quad: BoundQuad {
                points: [
                    [0.0, 0.0, 0.5],
                    [1.0, 0.0, 0.5],
                    [1.0, 1.0, 0.5],
                    [0.0, 1.0, 0.5],
                ],
                uv: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                normal: [0.0, 0.0, 1.0],
                material: FaceMaterial {
                    texture: handle,
                    alpha: AlphaMode::NativeBlend,
                    tint: [1.0; 3],
                    layer: 3,
                    normal_map: None,
                    specular_map: None,
                },
                cull_face: None,
                shade: true,
                part: 0,
                face: 0,
            },
        });
        let before = budget.used();
        let mesh = FluidMesh::from_native_faces(
            faces,
            MeshRegion {
                minimum: [-1, -1],
                maximum: [2, 2],
            },
            &bank,
            &budget,
            cancel,
        )
        .unwrap();
        assert_eq!(mesh.faces.len(), 1);
        assert!(mesh.faces.capacity() >= 64);
        assert_eq!(
            budget.used() - before,
            4096 + mesh.faces.capacity() as u64 * std::mem::size_of::<FluidFace>() as u64
        );
        drop(mesh);
        assert_eq!(budget.used(), before);
    }

    #[test]
    fn shared_corners_are_global_and_partial_levels_keep_exposed_lips() {
        let cells = BTreeMap::from([
            ([0, 0, 1], FluidCell::new(0, [0.2, 0.4, 0.7]).unwrap()),
            ([1, 0, 1], FluidCell::new(4, [0.2, 0.4, 0.7]).unwrap()),
        ]);
        assert_eq!(
            corner_height(&cells, [0, 0, 1], 1, 0),
            corner_height(&cells, [1, 0, 1], 0, 0)
        );
        assert!(corner_height(&cells, [0, 0, 1], 0, 0) > corner_height(&cells, [1, 0, 1], 1, 0));
        assert!(FluidCell::new(9, [1.0; 3]).is_err());
    }
}
