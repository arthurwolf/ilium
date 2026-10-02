//! State/model/texture-bound exposed geometry. Opaque complete neighbor boundaries
//! may occlude; cutout leaves, partial cuboids and water cannot erase the riverbed.
use super::assets::{
    bank::{TextureBank, TextureHandle},
    block_state::BlockState,
    budget::{ByteBudget, Cancel, Reservation},
    error::{AssetError, Result},
    identity::{Digest256, ResourceId},
    metadata,
    models::{oriented_quad, DefinitionOrigin, Direction, ModelQuad, NormalizedState},
    texture::Encoding,
};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaMode {
    Opaque,
    Cutout { threshold: u8 },
    Blend,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceMaterial {
    pub texture: TextureHandle,
    pub alpha: AlphaMode,
    pub tint: [f32; 3],
    pub layer: u16,
    pub normal_map: Option<TextureHandle>,
    pub specular_map: Option<TextureHandle>,
}
pub trait BindingPolicy {
    fn medium(&self, _state: &BlockState) -> Option<ResourceId> {
        None
    }
    fn material(
        &self,
        state: &BlockState,
        quad: &ModelQuad,
        bank: &TextureBank,
    ) -> Result<FaceMaterial>;
}
#[derive(Clone, Debug)]
pub struct TextureRenderRule {
    pub alpha: AlphaMode,
    pub layer: u16,
    pub normal_map: Option<ResourceId>,
    pub specular_map: Option<ResourceId>,
}
/// Caller-supplied state/biome tint values; no grass/water filename heuristic.
pub struct MaterialTable {
    pub medium: Option<ResourceId>,
    pub rules: BTreeMap<ResourceId, TextureRenderRule>,
    pub tints: BTreeMap<u16, [f32; 3]>,
}
impl BindingPolicy for MaterialTable {
    fn medium(&self, _state: &BlockState) -> Option<ResourceId> {
        self.medium.clone()
    }
    fn material(
        &self,
        _state: &BlockState,
        quad: &ModelQuad,
        bank: &TextureBank,
    ) -> Result<FaceMaterial> {
        let rule = self
            .rules
            .get(&quad.texture)
            .ok_or_else(|| metadata::invalid("texture lacks explicit alpha/layer rule"))?;
        let texture = bank.resolve(&quad.texture).ok_or_else(|| {
            AssetError::InvalidMetadata(format!("missing material texture {}", quad.texture))
        })?;
        let tint = match quad.tint_index {
            Some(index) => *self.tints.get(&index).ok_or_else(|| {
                metadata::invalid("model tint index lacks a supplied biome/state tint")
            })?,
            None => [1.0; 3],
        };
        let data_handle = |id: &Option<ResourceId>| -> Result<Option<TextureHandle>> {
            id.as_ref()
                .map(|id| {
                    bank.resolve(id).ok_or_else(|| {
                        AssetError::InvalidMetadata(format!("missing explicit material map {id}"))
                    })
                })
                .transpose()
        };
        Ok(FaceMaterial {
            texture,
            alpha: rule.alpha,
            tint,
            layer: rule.layer,
            normal_map: data_handle(&rule.normal_map)?,
            specular_map: data_handle(&rule.specular_map)?,
        })
    }
}
#[derive(Clone, Debug)]
pub struct BoundQuad {
    pub points: [[f64; 3]; 4],
    pub uv: [[f32; 2]; 4],
    pub normal: [f32; 3],
    pub material: FaceMaterial,
    pub cull_face: Option<Direction>,
    pub shade: bool,
    pub part: u16,
    pub face: u16,
}
#[derive(Debug)]
pub struct BoundModel {
    pub state: BlockState,
    pub quads: Vec<BoundQuad>,
    pub opaque_boundaries: [bool; 6],
    pub origins: Vec<DefinitionOrigin>,
    pub bank: Digest256,
    pub medium: Option<ResourceId>,
    pub medium_boundaries: [bool; 6],
    _reservation: Reservation,
}
impl BoundModel {
    pub fn bind(
        state: &NormalizedState,
        bank: &TextureBank,
        policy: &impl BindingPolicy,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        if !state.uses_budget(budget) {
            return Err(metadata::invalid(
                "state and bound geometry accounts differ",
            ));
        }
        let count: usize = state
            .applications
            .iter()
            .map(|(_, model)| model.quads.len())
            .sum();
        if count > 8192 {
            return Err(metadata::invalid("state geometry exceeds 8192 quads"));
        }
        let origin_count: usize = state
            .applications
            .iter()
            .map(|(_, model)| model.origins.len())
            .sum();
        let reservation = budget.reserve(
            32768 + count as u64 * 512 + origin_count as u64 * 4096,
            cancel,
        )?;
        let mut quads = Vec::new();
        let mut opaque_boundaries = [false; 6];
        let mut medium_boundaries = [false; 6];
        let medium = policy.medium(&state.state);
        let mut origins = vec![state.state_origin.clone()];
        for (part, (application, model)) in state.applications.iter().enumerate() {
            if !model.uses_budget(budget) {
                return Err(metadata::invalid(
                    "model and bound geometry accounts differ",
                ));
            }
            origins.extend(model.origins.iter().cloned());
            for (face, base) in model.quads.iter().enumerate() {
                cancel.check()?;
                let quad = oriented_quad(base, application)?;
                let material = policy.material(&state.state, &quad, bank)?;
                let texture = bank
                    .texture(material.texture)
                    .ok_or_else(|| metadata::invalid("material uses a stale bank handle"))?;
                if !texture.uses_budget(budget) {
                    return Err(metadata::invalid("geometry and texture accounts differ"));
                }
                if texture.encoding() != Encoding::SrgbColor {
                    return Err(metadata::invalid(
                        "material data texture used as diffuse color",
                    ));
                }
                for handle in [material.normal_map, material.specular_map]
                    .into_iter()
                    .flatten()
                {
                    let map = bank.texture(handle).ok_or_else(|| {
                        metadata::invalid("material map uses a stale bank handle")
                    })?;
                    if !map.uses_budget(budget) || map.encoding() != Encoding::LinearData {
                        return Err(metadata::invalid(
                            "normal/specular map must be linear data in the scene account",
                        ));
                    }
                }
                if material
                    .tint
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
                {
                    return Err(metadata::invalid(
                        "material tint outside finite linear 0..1",
                    ));
                }
                if material.alpha == AlphaMode::Opaque && texture.image().info().alpha_min != 255 {
                    return Err(metadata::invalid(
                        "opaque rule contradicts source alpha; use explicit cutout/blend policy",
                    ));
                }
                if matches!(material.alpha, AlphaMode::Cutout { threshold: 0 }) {
                    return Err(metadata::invalid(
                        "zero cutout threshold would make transparent holes solid",
                    ));
                }
                if material.alpha == AlphaMode::Opaque {
                    if let Some(boundary) = quad.complete_boundary {
                        opaque_boundaries[boundary.index()] = true;
                    }
                }
                if medium.is_some() && material.alpha == AlphaMode::Blend {
                    if let Some(boundary) = quad.complete_boundary {
                        medium_boundaries[boundary.index()] = true;
                    }
                }
                quads.push(BoundQuad {
                    points: quad.points,
                    uv: quad.uv,
                    normal: quad.normal,
                    material,
                    cull_face: quad.cull_face,
                    shade: quad.shade,
                    part: part as u16,
                    face: face as u16,
                });
            }
        }
        Ok(Self {
            state: state.state.clone(),
            quads,
            opaque_boundaries,
            origins,
            bank: bank.identity(),
            medium,
            medium_boundaries,
            _reservation: reservation,
        })
    }
}
/// Complete collision-free owner key: camera/window origins are deliberately absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FaceOwner {
    pub position: [i32; 3],
    pub part: u16,
    pub face: u16,
    pub layer: u16,
}
#[derive(Clone, Debug)]
pub struct MeshFace {
    pub position: [i32; 3],
    pub model: Arc<BoundModel>,
    pub quad_index: u16,
    pub owner: FaceOwner,
}
#[derive(Clone, Copy, Debug)]
pub struct MeshRegion {
    pub minimum: [i32; 2],
    pub maximum: [i32; 2],
}
#[derive(Debug)]
pub struct PreparedMesh {
    pub faces: Vec<MeshFace>,
    pub bank: Digest256,
    pub region: MeshRegion,
    _reservation: Reservation,
}
impl PreparedMesh {
    // Keep the shared budget identity explicit.
    pub(crate) fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self._reservation.belongs_to(budget)
    }
    /// Input includes any needed one-cell halo. Missing samples are visible, never assumed opaque.
    pub fn build(
        instances: &BTreeMap<[i32; 3], Arc<BoundModel>>,
        region: MeshRegion,
        bank: Digest256,
        max_faces: usize,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        if max_faces == 0 || max_faces > 1_000_000 || instances.len() > 1_000_000 {
            return Err(metadata::invalid("mesh count limit"));
        }
        if (0..2).any(|axis| {
            region.minimum[axis] >= region.maximum[axis]
                || i64::from(region.maximum[axis]) - i64::from(region.minimum[axis]) > 2048
        }) {
            return Err(metadata::invalid("mesh region bounds"));
        }
        // Two finite passes permit exact allocation without retaining a growable over-capacity vector.
        let admitted = |position: [i32; 3], model: &BoundModel, index: usize| -> Result<bool> {
            if model.bank != bank || !model._reservation.belongs_to(budget) {
                return Err(metadata::invalid("mesh models use another bank/account"));
            }
            if (0..2).any(|axis| {
                position[axis] < region.minimum[axis] || position[axis] >= region.maximum[axis]
            }) {
                return Ok(false);
            }
            let quad = &model.quads[index];
            let Some(face) = quad.cull_face else {
                return Ok(true);
            };
            let step = face.step();
            let Some(neighbor) = position[0].checked_add(step[0]).and_then(|x| {
                position[1]
                    .checked_add(step[1])
                    .and_then(|y| position[2].checked_add(step[2]).map(|z| [x, y, z]))
            }) else {
                return Ok(true);
            };
            let Some(other) = instances.get(&neighbor) else {
                return Ok(true);
            };
            if other.bank != bank || !other._reservation.belongs_to(budget) {
                return Err(metadata::invalid("neighbor uses another bank/account"));
            }
            if other.opaque_boundaries[face.opposite().index()] {
                return Ok(false);
            }
            let shared_medium = model.medium.is_some() && model.medium == other.medium;
            if quad.material.alpha == AlphaMode::Blend
                && shared_medium
                && model.medium_boundaries[face.index()]
                && other.medium_boundaries[face.opposite().index()]
            {
                return Ok(false);
            }
            Ok(true)
        };
        let mut count = 0usize;
        for (position, model) in instances {
            cancel.check()?;
            for index in 0..model.quads.len() {
                if !admitted(*position, model, index)? {
                    continue;
                }
                count += 1;
                if count > max_faces {
                    return Err(AssetError::Limit {
                        resource: "prepared mesh faces",
                        requested: count as u64,
                        limit: max_faces as u64,
                    });
                }
            }
        }
        let reservation = budget.reserve(
            4096 + count as u64 * std::mem::size_of::<MeshFace>() as u64,
            cancel,
        )?;
        let mut faces = Vec::new();
        faces
            .try_reserve_exact(count)
            .map_err(|_| AssetError::Allocation)?;
        for (position, model) in instances {
            cancel.check()?;
            for (index, quad) in model.quads.iter().enumerate() {
                if !admitted(*position, model, index)? {
                    continue;
                }
                faces.push(MeshFace {
                    position: *position,
                    model: Arc::clone(model),
                    quad_index: index as u16,
                    owner: FaceOwner {
                        position: *position,
                        part: quad.part,
                        face: quad.face,
                        layer: quad.material.layer,
                    },
                });
            }
        }
        Ok(Self {
            faces,
            bank,
            region,
            _reservation: reservation,
        })
    }
}
