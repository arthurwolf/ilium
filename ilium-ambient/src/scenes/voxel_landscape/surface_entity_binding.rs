//! Worker-only binding of authored surface anatomy to real selected-pack atlases.
//! A missing atlas or invalid UV omits that whole entity and records the gap.
//! Bedrock reference rectangles are used as an explicitly labelled Java-pack
//! compatibility approximation; a decoded PNG does not prove Java-native UVs.
use super::{
    assets::{
        animation::MissingAnimation,
        bank::{RequiredOrigin, TextureBank, TextureHandle, TextureRequirement},
        budget::{ByteBudget, Cancel, Reservation},
        error::{AssetError, Result},
        identity::{AssetPath, BlobOrigin, Digest256, Label, OriginKind, ResourceId},
        importer::{
            ImportResult, ScheduleSource, TextureCandidate, TextureLocation, TextureRequest,
        },
        pixels::ImageExpectations,
    },
    surface_entities::{self, AtlasEvidence, CompatibilityOutcome, Shape, UvStatus},
    surface_generation::SurfaceWorld,
    surface_mesh::{AlphaMode, FaceOwner},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
};

const MAX_ENTITIES: usize = 4096;
const MAX_PARTS_PER_ENTITY: usize = 1024;
const MAX_ENTITY_FACES: usize = 600_000;

#[derive(Clone, Debug)]
pub struct AtlasRecord {
    pub semantic: String,
    pub request: ResourceId,
    pub source: BlobOrigin,
    pub source_sha256: Digest256,
    pub dimensions: [u32; 2],
    pub used_fallback: bool,
    pub java_uv_verified: bool,
}

#[derive(Clone, Debug)]
pub struct EntityFace {
    pub anchor: [i32; 3],
    pub points: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    pub texture: TextureHandle,
    pub owner: FaceOwner,
    pub alpha: AlphaMode,
    pub normal: [f32; 3],
}

pub struct PreparedEntityMesh {
    pub bank: Digest256,
    pub faces: Vec<EntityFace>,
    pub atlases: Vec<AtlasRecord>,
    pub gaps: Vec<String>,
    pub rendered_entities: usize,
    _reservation: Reservation,
}
impl PreparedEntityMesh {
    pub fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self._reservation.belongs_to(budget)
    }
}

fn push_unique(paths: &mut Vec<String>, path: String) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

/// Explicit Java/legacy atlas filename vocabulary, never a claim about model UVs.
/// Importing a request still requires the exact member to exist in the selected
/// local pack and satisfy its review and color-encoding requirement.
fn atlas_paths(semantic: &str) -> Vec<String> {
    let path = semantic.strip_prefix("minecraft:").unwrap_or(semantic);
    let mut paths = Vec::new();
    match path {
        "cow/warm" | "cow/cold" | "pig/warm" | "pig/cold" | "chicken/warm" | "chicken/cold" => {
            let (species, climate) = path.split_once('/').unwrap_or((path, "temperate"));
            push_unique(&mut paths, format!("entity/{species}/{species}_{climate}"));
            push_unique(&mut paths, format!("entity/{species}/{climate}_{species}"));
            // An older selected pack may have one skin for every climate. This
            // is a visible, provenance-labelled variant fallback, not native art.
            push_unique(&mut paths, format!("entity/{species}/{species}"));
            push_unique(&mut paths, format!("entity/{species}"));
        }
        "cow" | "pig" | "chicken" => {
            push_unique(&mut paths, format!("entity/{path}/{path}_temperate"));
            push_unique(&mut paths, format!("entity/{path}/temperate_{path}"));
            push_unique(&mut paths, format!("entity/{path}/{path}"));
            push_unique(&mut paths, format!("entity/{path}"));
        }
        "mooshroom" => {
            for value in ["cow/mooshroom_red", "cow/red_mooshroom", "cow/mooshroom"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "bogged" | "parched" | "stray" => {
            push_unique(&mut paths, format!("entity/skeleton/{path}"));
        }
        "drowned" | "husk" => push_unique(&mut paths, format!("entity/zombie/{path}")),
        "evoker" | "pillager" | "ravager" | "vex" | "vindicator" => {
            push_unique(&mut paths, format!("entity/illager/{path}"));
        }
        "polar_bear" => {
            for value in ["polar_bear/polar_bear", "bear/polarbear", "bear/polar_bear"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "zombie_horse" => push_unique(&mut paths, "entity/horse/horse_zombie".into()),
        "skeleton_horse" => push_unique(&mut paths, "entity/horse/horse_skeleton".into()),
        "donkey" => push_unique(&mut paths, "entity/horse/donkey".into()),
        "horse" => {
            for value in ["horse/horse_brown", "horse/horse_white", "horse/horse"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "rabbit" => {
            for value in ["rabbit/rabbit_brown", "rabbit/brown"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "cat" => {
            for value in ["cat/cat_tabby", "cat/tabby"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "ocelot" => push_unique(&mut paths, "entity/cat/ocelot".into()),
        "llama" => {
            for value in ["llama/llama", "llama/creamy", "llama/llama_creamy"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "trader_llama" => {
            // The trader decoration is a separate equipment overlay, not a
            // complete skin atlas. Use an explicitly labelled base llama.
            for value in ["llama/llama_creamy", "llama/creamy"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "frog" => {
            for value in ["frog/frog_temperate", "frog/temperate_frog", "frog/frog"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "fox" => {
            for value in ["fox/fox", "fox/red_fox"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "parrot" => {
            for value in [
                "parrot/parrot_red_blue",
                "parrot/red_blue",
                "parrot/parrot_blue",
            ] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "villager" => {
            for value in ["villager/villager", "villager/villager_base"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "zombie_villager" => {
            for value in ["zombie_villager/zombie_villager", "zombie/zombie_villager"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "wandering_trader" => push_unique(&mut paths, "entity/wandering_trader".into()),
        "zombified_piglin" => {
            for value in ["piglin/zombified_piglin", "zombie_pigman"] {
                push_unique(&mut paths, format!("entity/{value}"));
            }
        }
        "creeper/charged" => push_unique(&mut paths, "entity/creeper/creeper_armor".into()),
        "panda" => push_unique(&mut paths, "entity/panda/panda".into()),
        "wolf" => push_unique(&mut paths, "entity/wolf/wolf".into()),
        "iron_golem" => push_unique(&mut paths, "entity/iron_golem/iron_golem".into()),
        "slime" => push_unique(&mut paths, "entity/slime/slime".into()),
        _ => {}
    }
    if !path.contains('/') {
        push_unique(&mut paths, format!("entity/{path}/{path}"));
        push_unique(&mut paths, format!("entity/{path}"));
    }
    paths.truncate(12);
    paths
}

fn request_id(semantic: &str) -> Result<ResourceId> {
    let path = semantic.strip_prefix("minecraft:").ok_or_else(|| {
        AssetError::InvalidMetadata(format!("unsupported entity texture semantic {semantic}"))
    })?;
    ResourceId::parse(&format!("minecraft:entity/render/{path}"))
}

/// Add these requests to the block/fluid request batch *before* its single import.
pub fn requests(
    world: &SurfaceWorld,
    pack: &ResourceId,
    fallback_pack: Option<&ResourceId>,
    goodvibes: bool,
    selected_is_bedrock: bool,
) -> Result<Vec<TextureRequest>> {
    if world.entities.len() > MAX_ENTITIES {
        return Err(AssetError::Limit {
            resource: "surface entities",
            requested: world.entities.len() as u64,
            limit: MAX_ENTITIES as u64,
        });
    }
    let mut semantics = BTreeSet::new();
    for entity in &world.entities {
        for part in &entity.model.parts {
            semantics.insert(part.texture_semantic);
        }
    }
    let mut requests = Vec::new();
    for semantic in semantics {
        let paths = atlas_paths(semantic);
        let id = request_id(semantic)?;
        let mut candidates = Vec::new();
        for (index, path) in paths.iter().enumerate() {
            let alias_reason = Label::new(if index == 0 {
                "Selected-pack entity image with authored geometry; Bedrock-reference UV not verified for Java"
            } else {
                "Selected-pack legacy or variant atlas path; explicit visual compatibility, not native climate/model proof"
            })?;
            if !selected_is_bedrock {
                candidates.push(TextureCandidate {
                    pack: pack.clone(),
                    location: TextureLocation::Resource {
                        id: ResourceId::parse(&format!("minecraft:{path}"))?,
                        alias_reason: Some(alias_reason.clone()),
                    },
                    schedule: ScheduleSource::AutomaticJava,
                    expected_source_sha256: None,
                    expected_image: ImageExpectations::default(),
                });
            }
            if goodvibes && !selected_is_bedrock {
                candidates.push(TextureCandidate {
                    pack: pack.clone(),
                    location: TextureLocation::Literal {
                        path: AssetPath::parse(&format!("textures/{path}.png"))?,
                        evidence: alias_reason,
                    },
                    schedule: ScheduleSource::NoMetadata,
                    expected_source_sha256: None,
                    expected_image: ImageExpectations::default(),
                });
            }
        }
        if let Some(fallback_pack) = fallback_pack.filter(|fallback| *fallback != pack) {
            for path in &paths {
                candidates.push(TextureCandidate {
                    pack: fallback_pack.clone(),
                    location: TextureLocation::Resource {
                        id: ResourceId::parse(&format!("minecraft:{path}"))?,
                        alias_reason: Some(Label::new("Reviewed installed Whimscape full-pack private fauna fallback; source remains explicit, not selected-native")?),
                    },
                    schedule: ScheduleSource::AutomaticJava,
                    expected_source_sha256: None,
                    expected_image: ImageExpectations::default(),
                });
            }
        }
        if candidates.len() > 16 {
            return Err(AssetError::Limit {
                resource: "entity atlas candidates",
                requested: candidates.len() as u64,
                limit: 16,
            });
        }
        let mut requirement = TextureRequirement::selected_color(id);
        requirement.origin = RequiredOrigin::SelectedOrExplicitFullPackFallback;
        requests.push(TextureRequest {
            requirement,
            candidates,
            missing_animation: MissingAnimation::StaticImage,
        });
    }
    Ok(requests)
}

fn face_corners(face: usize) -> [usize; 4] {
    match face {
        0 => [4, 5, 7, 6], // +Z
        1 => [2, 3, 1, 0], // -Z
        2 => [0, 1, 5, 4], // -Y
        3 => [3, 2, 6, 7], // +Y
        4 => [2, 0, 4, 6], // -X
        _ => [1, 3, 7, 5], // +X
    }
}
fn face_uv(rect: surface_entities::UvRect) -> [[f32; 2]; 4] {
    let (left, right) = if rect.flip_u {
        (rect.max[0], rect.min[0])
    } else {
        (rect.min[0], rect.max[0])
    };
    let (top, bottom) = if rect.flip_v {
        (rect.max[1], rect.min[1])
    } else {
        (rect.min[1], rect.max[1])
    };
    [[left, top], [right, top], [right, bottom], [left, bottom]]
}
fn normal(points: [[f32; 3]; 4]) -> Option<[f32; 3]> {
    let a = std::array::from_fn::<_, 3, _>(|i| points[1][i] - points[0][i]);
    let b = std::array::from_fn::<_, 3, _>(|i| points[3][i] - points[0][i]);
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let length = cross.iter().map(|n| n * n).sum::<f32>().sqrt();
    (length.is_finite() && length > 1e-7).then(|| cross.map(|n| n / length))
}

/// Resolve actual decoded atlas identity and create only fully textured entities.
/// All faces share the block/fluid bank epoch and scene byte account.
pub fn bind(
    world: &SurfaceWorld,
    imports: &ImportResult,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> Result<PreparedEntityMesh> {
    cancel.check()?;
    if world.entities.len() > MAX_ENTITIES {
        return Err(AssetError::Limit {
            resource: "surface entities",
            requested: world.entities.len() as u64,
            limit: MAX_ENTITIES as u64,
        });
    }
    let bank: &TextureBank = &imports.bank;
    let mut atlas_by_semantic = BTreeMap::<String, (ResourceId, TextureHandle, AtlasRecord)>::new();
    let mut gaps = Vec::new();
    for record in &imports.records {
        let Some(semantic) = record
            .resource
            .as_str()
            .strip_prefix("minecraft:entity/render/")
        else {
            continue;
        };
        let (Some(handle), Some(source), Some(hash), Some(dimensions)) = (
            bank.resolve(&record.resource),
            record.source.as_ref(),
            record.source_sha256,
            record.dimensions,
        ) else {
            gaps.push(format!(
                "{semantic}: entity atlas absent from permitted source(s) ({})",
                record.failure.as_deref().unwrap_or("unresolved")
            ));
            continue;
        };
        let texture = bank
            .texture(handle)
            .ok_or_else(|| AssetError::InvalidMetadata("entity atlas bank handle stale".into()))?;
        if texture.image().source_sha256() != hash
            || texture.image().dimensions() != dimensions
            || !texture.uses_budget(budget)
        {
            return Err(AssetError::InvalidMetadata(
                "entity atlas record/bank/account mismatch".into(),
            ));
        }
        let atlas = AtlasRecord {
            semantic: format!("minecraft:{semantic}"),
            request: record.resource.clone(),
            source: source.clone(),
            source_sha256: hash,
            dimensions,
            used_fallback: source.kind == OriginKind::ExplicitFullPackFallback,
            java_uv_verified: false,
        };
        atlas_by_semantic.insert(
            atlas.semantic.clone(),
            (record.resource.clone(), handle, atlas),
        );
    }
    let estimated_faces = world
        .entities
        .iter()
        .try_fold(0usize, |count, entity| {
            count.checked_add(entity.model.parts.len().checked_mul(6)?)
        })
        .ok_or(AssetError::Allocation)?;
    if estimated_faces > MAX_ENTITY_FACES {
        return Err(AssetError::Limit {
            resource: "entity faces",
            requested: estimated_faces as u64,
            limit: MAX_ENTITY_FACES as u64,
        });
    }
    let charge = estimated_faces
        .checked_mul(size_of::<EntityFace>())
        .and_then(|n| n.checked_add(MAX_ENTITIES * 256 + 65_536))
        .ok_or(AssetError::Allocation)?;
    let reservation = budget.reserve(charge as u64, cancel)?;
    let mut faces = Vec::new();
    faces
        .try_reserve_exact(estimated_faces)
        .map_err(|_| AssetError::Allocation)?;
    let mut rendered_entities = 0;
    let mut anchor_slots = BTreeMap::<[i32; 3], u16>::new();
    for entity in &world.entities {
        cancel.check()?;
        if entity.model.parts.len() > MAX_PARTS_PER_ENTITY {
            return Err(AssetError::Limit {
                resource: "entity model parts",
                requested: entity.model.parts.len() as u64,
                limit: MAX_PARTS_PER_ENTITY as u64,
            });
        }
        let transient = entity
            .model
            .parts
            .len()
            .checked_mul(size_of::<surface_entities::Part>())
            .and_then(|n| n.checked_add(4096))
            .ok_or(AssetError::Allocation)?;
        let _model_reservation = budget.reserve(transient as u64, cancel)?;
        let mut model = entity.model.clone();
        let semantics: BTreeSet<_> = model
            .parts
            .iter()
            .map(|part| part.texture_semantic)
            .collect();
        let mut all_bound = true;
        for semantic in semantics {
            let Some((request, _, atlas)) = atlas_by_semantic.get(semantic) else {
                gaps.push(format!(
                    "{} at {:?}: {semantic} absent; whole entity omitted",
                    entity.species.id(),
                    entity.anchor
                ));
                all_bound = false;
                break;
            };
            let evidence = AtlasEvidence {
                semantic: semantic.into(),
                resource_id: request.to_string(),
                png_dimensions: atlas.dimensions,
                encoded_sha256: atlas.source_sha256.to_string(),
            };
            match surface_entities::apply_authored_compatibility(&mut model, &evidence) {
                Ok(CompatibilityOutcome::Matched) => {}
                Ok(CompatibilityOutcome::Unmatched) => {
                    all_bound = false;
                    gaps.push(format!("{}: atlas semantic unmatched", entity.species.id()));
                    break;
                }
                Err(error) => {
                    all_bound = false;
                    gaps.push(format!(
                        "{}: authored UV compatibility failed: {error:?}",
                        entity.species.id()
                    ));
                    break;
                }
            }
        }
        if !all_bound || model.parts.iter().any(|part| part.uv.is_none()) {
            continue;
        }
        let slot = anchor_slots.entry(entity.anchor).or_insert(0);
        let owner_part = *slot;
        *slot = slot
            .checked_add(1)
            .ok_or_else(|| AssetError::InvalidMetadata("too many coincident entities".into()))?;
        let first_face = faces.len();
        for (part_index, part) in model.parts.iter().enumerate() {
            let uv = part
                .uv
                .ok_or_else(|| AssetError::InvalidMetadata("entity UV vanished".into()))?;
            if uv.status == UvStatus::UnsupportedPart {
                all_bound = false;
                break;
            }
            let Some((_, handle, _)) = atlas_by_semantic.get(part.texture_semantic) else {
                all_bound = false;
                break;
            };
            let texture = bank
                .texture(*handle)
                .ok_or_else(|| AssetError::InvalidMetadata("entity texture handle stale".into()))?;
            let alpha = if part.texture_semantic == "minecraft:creeper/charged"
                || entity.species == surface_entities::Species::Slime
            {
                AlphaMode::Blend
            } else if texture.image().info().alpha_min < 255 {
                AlphaMode::Cutout { threshold: 128 }
            } else {
                AlphaMode::Opaque
            };
            let corners = part.corners();
            let flat_axis = (0..3).find(|axis| (part.max[*axis] - part.min[*axis]).abs() < 1e-7);
            for face_index in 0..6 {
                // RasterFrame is two-sided. One quad represents both sides of a
                // zero-thickness plane without double-blending its same-depth pixels.
                if part.shape == Shape::Plane
                    && flat_axis
                        .is_some_and(|axis| face_index / 2 != 2 - axis || face_index % 2 == 1)
                {
                    continue;
                }
                let Some(rect) = uv.faces[face_index].normalized(uv.nominal) else {
                    all_bound = false;
                    break;
                };
                let order = face_corners(face_index);
                let points = order.map(|index| corners[index]);
                let Some(normal) = normal(points) else {
                    continue;
                };
                let face_id = part_index
                    .checked_mul(6)
                    .and_then(|n| n.checked_add(face_index))
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or_else(|| {
                        AssetError::InvalidMetadata("entity face owner overflows".into())
                    })?;
                faces.push(EntityFace {
                    anchor: entity.anchor,
                    points,
                    uv: face_uv(rect),
                    texture: *handle,
                    owner: FaceOwner {
                        position: entity.anchor,
                        part: owner_part,
                        face: face_id,
                        layer: 2,
                    },
                    alpha,
                    normal,
                });
                if faces.len() > MAX_ENTITY_FACES {
                    return Err(AssetError::Limit {
                        resource: "entity faces",
                        requested: faces.len() as u64,
                        limit: MAX_ENTITY_FACES as u64,
                    });
                }
            }
            if !all_bound {
                break;
            }
        }
        if all_bound {
            rendered_entities += 1;
        } else {
            faces.truncate(first_face);
            gaps.push(format!(
                "{} at {:?}: invalid complete atlas geometry; whole entity omitted",
                entity.species.id(),
                entity.anchor
            ));
        }
    }
    Ok(PreparedEntityMesh {
        bank: bank.identity(),
        faces,
        atlases: atlas_by_semantic
            .into_values()
            .map(|(_, _, record)| record)
            .collect(),
        gaps,
        rendered_entities,
        _reservation: reservation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn climate_and_legacy_paths_are_explicit() {
        assert_eq!(atlas_paths("minecraft:cow/warm")[0], "entity/cow/cow_warm");
        assert!(atlas_paths("minecraft:cow/warm").contains(&"entity/cow/cow".into()));
        assert_eq!(atlas_paths("minecraft:bogged")[0], "entity/skeleton/bogged");
        assert_eq!(
            atlas_paths("minecraft:creeper/charged")[0],
            "entity/creeper/creeper_armor"
        );
    }
    #[test]
    fn owner_face_and_uv_orientation_are_stable() {
        assert_eq!(face_corners(0), [4, 5, 7, 6]);
        let uv = surface_entities::UvRect {
            min: [0.25, 0.5],
            max: [0.75, 1.0],
            flip_u: true,
            flip_v: false,
        };
        assert_eq!(
            face_uv(uv),
            [[0.75, 0.5], [0.25, 0.5], [0.25, 1.0], [0.75, 1.0]]
        );
    }
    #[test]
    fn all_fifty_species_and_climates_have_bounded_exact_path_candidates() {
        let mut vocabulary = BTreeMap::new();
        for species in surface_entities::ALL_SPECIES {
            for climate in [
                surface_entities::ClimateSkin::Temperate,
                surface_entities::ClimateSkin::Warm,
                surface_entities::ClimateSkin::Cold,
            ] {
                let model = surface_entities::model(
                    *species,
                    surface_entities::AtlasLayout::Bedrock,
                    climate,
                );
                for part in &model.parts {
                    let paths = atlas_paths(part.texture_semantic);
                    assert!(
                        !paths.is_empty(),
                        "{}: {}",
                        species.id(),
                        part.texture_semantic
                    );
                    assert!(
                        paths.len() <= 5,
                        "{}: {}",
                        species.id(),
                        part.texture_semantic
                    );
                    vocabulary.insert(part.texture_semantic.to_owned(), paths);
                }
            }
        }
        println!(
            "{}",
            serde_json::json!({"type":"atlas_path_vocabulary","entries":vocabulary})
        );
    }
}
