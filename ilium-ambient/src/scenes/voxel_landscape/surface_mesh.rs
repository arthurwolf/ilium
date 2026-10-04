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
    Cutout {
        threshold: u8,
    },
    Blend,
    /// Pinned 1.19.3 solid shader: source alpha never discards the RGB/depth.
    NativeSolid,
    /// Pinned rendertype_cutout.fsh discards only final alpha below 0.1.
    NativeCutout,
    /// Pinned rendertype_cutout_mipped.fsh discards below 0.5.
    NativeCutoutMipped,
    /// Pinned translucent layer keeps source alpha for ordered composition.
    NativeBlend,
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
    /// Bind source-native block-entity cuboid faces into the same face/owner,
    /// bank and accounting path as ordinary native block-model quads. The
    /// caller has already baked and sourced the 1.19.3 atlas geometry. Model
    /// definition origins are empty because these faces do not come from JSON;
    /// the native archive digest belongs in the caller's model epoch.
    pub fn from_source_native_builtin(
        state: BlockState,
        quads: Vec<BoundQuad>,
        bank: &TextureBank,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        if quads.is_empty() || quads.len() > 8192 {
            return Err(metadata::invalid("native builtin quad count"));
        }
        for quad in &quads {
            cancel.check()?;
            if quad.cull_face.is_some()
                || quad.points.iter().flatten().any(|value| !value.is_finite())
                || quad.uv.iter().flatten().any(|value| !value.is_finite())
                || quad.normal.iter().any(|value| !value.is_finite())
                || quad
                    .material
                    .tint
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            {
                return Err(metadata::invalid("invalid native builtin face"));
            }
            let texture = bank
                .texture(quad.material.texture)
                .ok_or_else(|| metadata::invalid("stale native builtin atlas handle"))?;
            if !texture.uses_budget(budget) || texture.encoding() != Encoding::SrgbColor {
                return Err(metadata::invalid(
                    "native builtin atlas differs from scene account",
                ));
            }
        }
        let reservation = budget.reserve(32768 + quads.len() as u64 * 512, cancel)?;
        Ok(Self {
            state,
            quads,
            opaque_boundaries: [false; 6],
            origins: Vec::new(),
            bank: bank.identity(),
            medium: None,
            medium_boundaries: [false; 6],
            _reservation: reservation,
        })
    }

    /// BellRenderer contributes an entity-textured body in addition to the
    /// ordinary bell JSON stand. Keep that stand's provenance, boundary flags,
    /// selected bank, and owner namespace while charging the copied quads.
    pub fn with_source_native_builtin(
        &self,
        extra: Vec<BoundQuad>,
        bank: &TextureBank,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        let count = self
            .quads
            .len()
            .checked_add(extra.len())
            .ok_or_else(|| metadata::invalid("combined native quad count overflow"))?;
        if self.bank != bank.identity()
            || !self._reservation.belongs_to(budget)
            || extra.is_empty()
            || count > 8192
        {
            return Err(metadata::invalid("combined native bell bank/account/quads"));
        }
        for quad in &extra {
            cancel.check()?;
            if quad.cull_face.is_some()
                || quad.part != u16::MAX
                || quad.points.iter().flatten().any(|value| !value.is_finite())
                || quad.uv.iter().flatten().any(|value| !value.is_finite())
                || quad.normal.iter().any(|value| !value.is_finite())
                || quad
                    .material
                    .tint
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            {
                return Err(metadata::invalid("invalid additive native bell face"));
            }
            let texture = bank
                .texture(quad.material.texture)
                .ok_or_else(|| metadata::invalid("stale native bell atlas handle"))?;
            if !texture.uses_budget(budget) || texture.encoding() != Encoding::SrgbColor {
                return Err(metadata::invalid(
                    "native bell atlas differs from scene account",
                ));
            }
        }
        if self.quads.iter().any(|quad| quad.part == u16::MAX) {
            return Err(metadata::invalid(
                "native bell owner part conflicts with JSON model",
            ));
        }
        let charge = 32768_u64
            .checked_add(count as u64 * 512)
            .and_then(|value| value.checked_add(self.origins.len() as u64 * 4096))
            .ok_or_else(|| metadata::invalid("combined native bell charge overflow"))?;
        let reservation = budget.reserve(charge, cancel)?;
        let mut quads = Vec::new();
        quads
            .try_reserve_exact(count)
            .map_err(|_| AssetError::Allocation)?;
        quads.extend(self.quads.iter().cloned());
        quads.extend(extra);
        Ok(Self {
            state: self.state.clone(),
            quads,
            opaque_boundaries: self.opaque_boundaries,
            origins: self.origins.clone(),
            bank: self.bank,
            medium: self.medium.clone(),
            medium_boundaries: self.medium_boundaries,
            _reservation: reservation,
        })
    }

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
                if matches!(material.alpha, AlphaMode::Opaque | AlphaMode::NativeSolid) {
                    if let Some(boundary) = quad.complete_boundary {
                        opaque_boundaries[boundary.index()] = true;
                    }
                }
                if medium.is_some()
                    && matches!(material.alpha, AlphaMode::Blend | AlphaMode::NativeBlend)
                {
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
        Self::build_with_admission(instances, region, bank, max_faces, budget, cancel, None)
    }

    // Restrict owners; retain their culling witnesses.
    pub(crate) fn build_with_admission(
        instances: &BTreeMap<[i32; 3], Arc<BoundModel>>,
        region: MeshRegion,
        bank: Digest256,
        max_faces: usize,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
        published: Option<&[[i32; 3]]>,
    ) -> Result<Self> {
        cancel.check()?;
        if let Some(positions) = published {
            if positions.len() > 1_000_000 {
                return Err(AssetError::Limit {
                    resource: "mesh publication positions",
                    requested: positions.len() as u64,
                    limit: 1_000_000,
                });
            }
            for pair in positions.windows(2) {
                cancel.check()?;
                if pair[0] >= pair[1] {
                    return Err(metadata::invalid(
                        "mesh publication positions are not strictly sorted",
                    ));
                }
            }
        }
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
            if published.is_some_and(|positions| positions.binary_search(&position).is_err()) {
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
            if matches!(
                quad.material.alpha,
                AlphaMode::Blend | AlphaMode::NativeBlend
            ) && shared_medium
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

#[cfg(test)]
mod native_builtin_combination_tests {
    use super::super::assets::{
        animation::MissingAnimation,
        bank::TextureBankBuilder,
        budget::Limits,
        compatibility::DefinitionSet,
        identity::{Label, OriginKind},
        models::ModelCompiler,
        review::{fixture_origin, fixture_review},
        texture::fixture_texture,
    };
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn id(text: &str) -> ResourceId {
        ResourceId::parse(text).unwrap()
    }

    fn fixture_bank(budget: &ByteBudget, bell_color: [u8; 4]) -> TextureBank {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let mut builder = TextureBankBuilder::new(
            fixture_review(),
            Vec::new(),
            Limits::default(),
            budget.clone(),
            cancel,
        )
        .unwrap();
        for (name, color) in [
            ("test:stand", [80, 50, 20, 255]),
            ("test:bell_body", bell_color),
        ] {
            builder
                .insert(
                    id(name),
                    fixture_texture(
                        [1, 1],
                        &color,
                        None,
                        &MissingAnimation::StaticImage,
                        Encoding::SrgbColor,
                        fixture_origin(OriginKind::SelectedPack),
                        budget,
                    ),
                    cancel,
                )
                .unwrap();
        }
        builder.finish(Vec::new(), cancel).unwrap()
    }

    fn stand(budget: &ByteBudget, bank: &TextureBank) -> BoundModel {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let mut definitions = DefinitionSet::new(
            fixture_origin(OriginKind::DiagnosticFixture),
            Limits::default(),
            budget.clone(),
        )
        .unwrap();
        definitions.install_geometry_templates(cancel).unwrap();
        definitions
            .bind_single(
                id("test:bell"),
                id("minecraft:block/cube_all"),
                "all",
                id("test:stand"),
                Label::new("synthetic bell stand combination test").unwrap(),
                cancel,
            )
            .unwrap();
        let mut compiler =
            ModelCompiler::new(&definitions, Limits::default(), budget.clone()).unwrap();
        let normalized = compiler
            .compile_state(
                &BlockState::new(id("test:bell"), []).unwrap(),
                [0; 3],
                0,
                cancel,
            )
            .unwrap();
        BoundModel::bind(
            &normalized,
            bank,
            &MaterialTable {
                medium: None,
                rules: BTreeMap::from([(
                    id("test:stand"),
                    TextureRenderRule {
                        alpha: AlphaMode::NativeSolid,
                        layer: 0,
                        normal_map: None,
                        specular_map: None,
                    },
                )]),
                tints: BTreeMap::new(),
            },
            budget,
            cancel,
        )
        .unwrap()
    }

    fn bell_face(bank: &TextureBank) -> BoundQuad {
        BoundQuad {
            points: [
                [0.25, 0.25, 0.75],
                [0.75, 0.25, 0.75],
                [0.75, 0.75, 0.75],
                [0.25, 0.75, 0.75],
            ],
            uv: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            normal: [0.0, 0.0, 1.0],
            material: FaceMaterial {
                texture: bank.resolve(&id("test:bell_body")).unwrap(),
                alpha: AlphaMode::NativeSolid,
                tint: [1.0; 3],
                layer: 0,
                normal_map: None,
                specular_map: None,
            },
            cull_face: None,
            shade: true,
            part: u16::MAX,
            face: 0,
        }
    }

    #[test]
    fn additive_bell_keeps_json_faces_origins_bank_and_budget_custody() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(256 << 20).unwrap();
        let bank = fixture_bank(&budget, [220, 170, 40, 255]);
        let stand = stand(&budget, &bank);
        assert_eq!(stand.quads.len(), 6);
        assert!(!stand.origins.is_empty());
        assert!(stand.opaque_boundaries.iter().all(|value| *value));
        let before = budget.used();
        let combined = stand
            .with_source_native_builtin(vec![bell_face(&bank)], &bank, &budget, cancel)
            .unwrap();
        assert_eq!(combined.quads.len(), 7);
        assert_eq!(
            serde_json::to_vec(&combined.origins).unwrap(),
            serde_json::to_vec(&stand.origins).unwrap()
        );
        assert_eq!(combined.opaque_boundaries, stand.opaque_boundaries);
        assert_eq!(combined.quads[6].part, u16::MAX);
        assert!(combined.quads[..6].iter().all(|quad| quad.part != u16::MAX));
        assert_eq!(combined.bank, bank.identity());
        assert!(budget.used() > before);
        drop(combined);
        assert_eq!(budget.used(), before);

        let wrong_bank = fixture_bank(&budget, [40, 170, 220, 255]);
        assert_ne!(bank.identity(), wrong_bank.identity());
        assert!(stand
            .with_source_native_builtin(vec![bell_face(&bank)], &wrong_bank, &budget, cancel)
            .is_err());
        let other_budget = ByteBudget::new(256 << 20).unwrap();
        assert!(stand
            .with_source_native_builtin(vec![bell_face(&bank)], &bank, &other_budget, cancel)
            .is_err());
        assert!(stand
            .with_source_native_builtin(vec![bell_face(&wrong_bank)], &bank, &budget, cancel)
            .is_err());
    }

    #[test]
    fn additive_bell_retains_json_opaque_neighbor_boundary_and_unique_owner() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(256 << 20).unwrap();
        let bank = fixture_bank(&budget, [220, 170, 40, 255]);
        let stand = Arc::new(stand(&budget, &bank));
        let combined = Arc::new(
            stand
                .with_source_native_builtin(vec![bell_face(&bank)], &bank, &budget, cancel)
                .unwrap(),
        );
        let instances = BTreeMap::from([([0, 0, 0], combined), ([1, 0, 0], stand)]);
        let mesh = PreparedMesh::build(
            &instances,
            MeshRegion {
                minimum: [0, 0],
                maximum: [2, 1],
            },
            bank.identity(),
            32,
            &budget,
            cancel,
        )
        .unwrap();
        // The facing JSON cube faces cull each other. The source bell body
        // remains a distinct, uncullable face with its own part/face key.
        assert_eq!(mesh.faces.len(), 11);
        assert_eq!(
            mesh.faces
                .iter()
                .filter(|face| face.owner.part == u16::MAX)
                .count(),
            1
        );
        assert_eq!(
            mesh.faces
                .iter()
                .filter(|face| face.owner.position == [0, 0, 0])
                .count(),
            6
        );
    }
}
