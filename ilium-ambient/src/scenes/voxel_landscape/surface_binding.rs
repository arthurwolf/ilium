//! Worker-only bridge from semantic surface states to selected-pack pixels.
//! Every source lookup, image decode, model normalization and mesh build shares
//! one bounded account. Missing selected art stays explicit, including any
//! separately reviewed full-pack image used for the same semantic resource.
use super::{
    assets::{
        animation::{ExplicitFrame, MissingAnimation, PixelRect},
        bank::{RequiredOrigin, TextureBank, TextureRequirement},
        block_state::BlockState,
        budget::{ByteBudget, Cancel, Limits, Reservation},
        compatibility::DefinitionSet,
        error::{AssetError, Result},
        identity::{AssetPath, BlobOrigin, Digest256, Label, OriginKind, ResourceId},
        importer::{
            DefinitionSources, ImportResult, ScheduleSource, TextureCandidate, TextureImporter,
            TextureLocation, TextureRequest,
        },
        layers::{LayeredPack, ResourceKey, ResourceKind},
        models::{Direction, ModelCompiler, NormalizedState},
        pixels::ImageExpectations,
    },
    pack_profiles, pack_sources,
    settings::VoxelLandscapeSettings,
    surface_context::SceneAtmosphere,
    surface_entity_binding::{self, PreparedEntityMesh},
    surface_entity_raster,
    surface_flora_vocabulary::ENTRIES,
    surface_fluid::FluidMesh,
    surface_generation::{self, Region, SurfaceWorld},
    surface_mesh::{
        AlphaMode, BoundModel, FaceOwner, MaterialTable, MeshRegion, PreparedMesh,
        TextureRenderRule,
    },
    surface_raster::{self, RasterFrame, RasterLimits},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

const SCENE_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

#[cfg(test)]
#[path = "ground_boundary_regressions.rs"]
mod ground_boundary_regressions;

fn texture_alpha(path: &str, alpha_min: u8) -> AlphaMode {
    // Dense packed and blue ice must cull internal faces; only ordinary ice blends.
    if path.contains("glass") || path == "block/ice" {
        AlphaMode::Blend
    } else if alpha_min < 255 {
        AlphaMode::Cutout { threshold: 128 }
    } else {
        AlphaMode::Opaque
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ModelChoiceKey {
    application: Vec<u8>,
    origins: Vec<u8>,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ModelStateKey {
    state: BlockState,
    origin: Vec<u8>,
    choices: Vec<ModelChoiceKey>,
}
fn normalized_key(state: &NormalizedState) -> Result<ModelStateKey> {
    let bytes = |value| {
        serde_json::to_vec(value).map_err(|error| {
            AssetError::InvalidMetadata(format!("model cache provenance: {error}"))
        })
    };
    let origin = bytes(&state.state_origin)?;
    let mut choices = Vec::new();
    for (application, model) in &state.applications {
        choices.push(ModelChoiceKey {
            application: serde_json::to_vec(application).map_err(|error| {
                AssetError::InvalidMetadata(format!("model cache application: {error}"))
            })?,
            origins: serde_json::to_vec(&model.origins).map_err(|error| {
                AssetError::InvalidMetadata(format!("model cache origins: {error}"))
            })?,
        });
    }
    Ok(ModelStateKey {
        state: state.state.clone(),
        origin,
        choices,
    })
}
fn retain_normalized(
    state: NormalizedState,
    saved: bool,
    cache: &mut BTreeMap<ModelStateKey, Arc<NormalizedState>>,
) -> Result<Arc<NormalizedState>> {
    let key = normalized_key(&state)?;
    if let Some(existing) = cache.get(&key) {
        return Ok(Arc::clone(existing));
    }
    if cache.len() >= 4096 {
        if saved {
            return Err(AssetError::InvalidMetadata(
                "saved model-state cache exceeds4096".into(),
            ));
        }
        return Ok(Arc::new(state));
    }
    let state = Arc::new(state);
    cache.insert(key, Arc::clone(&state));
    Ok(state)
}

fn saved_tint(state: &NormalizedState, supplied: Option<[f32; 3]>) -> Result<[f32; 3]> {
    if let Some(tint) = supplied {
        return Ok(tint);
    }
    if state
        .applications
        .iter()
        .any(|(_, model)| model.quads.iter().any(|quad| quad.tint_index.is_some()))
    {
        return Err(AssetError::Unsupported(
            "saved tinted model requires exact source-native tint input".into(),
        ));
    }
    // Untinted model quads ignore this multiplier; no biome colour is invented.
    Ok([1.0; 3])
}

pub struct PreparedSurface {
    pub world: SurfaceWorld,
    /// Immutable selected-pack bank shared by bounded saved-source tiles.
    pub imports: Arc<ImportResult>,
    pub mesh: PreparedMesh,
    pub fluid: Option<FluidMesh>,
    pub entities: PreparedEntityMesh,
    pub bank_epoch: Digest256,
    pub world_epoch: Digest256,
    pub model_epoch: Digest256,
    pub skipped_states: Vec<String>,
    pub compatibility_aliases: Vec<String>,
    pub material_fallbacks: Vec<String>,
    pub model_substitutions: Vec<String>,
    pub source_sha256: Option<Digest256>,
    pub budget: ByteBudget,
}
impl PreparedSurface {
    pub fn bank(&self) -> &TextureBank {
        &self.imports.bank
    }
    pub fn material_coverage(&self) -> (usize, usize) {
        let report = self.bank().coverage();
        (report.required_satisfied, report.required_count)
    }
}

/// A complete worker-produced frame. Its owner array is kept for seam and
/// revision qualification; the scene composites only colors and coverage.
pub struct StreamedViewport {
    pub size: [usize; 2],
    pub camera: [f64; 3],
    pub time: Duration,
    pub colors: Vec<[u8; 3]>,
    pub covered: Vec<bool>,
    pub owners: Vec<Option<FaceOwner>>,
    pub bank_epoch: Digest256,
    pub world_epoch: Digest256,
    pub model_epoch: Digest256,
    pub source_sha256: Option<Digest256>,
    pub tile_count: usize,
    pub material_coverage: (usize, usize),
    pub compatibility_aliases: usize,
    pub material_fallbacks: usize,
    pub fallback_atlases: usize,
    pub status: Option<String>,
    pub account_peak: u64,
    pub submitted_quads: usize,
    pub projected_attempts: usize,
    pub visible_triangles: usize,
    pub sample_tests: u64,
    pub retained_tiles: usize,
    _receipt_charge: Reservation,
}

fn material_id(block: &ResourceId) -> Result<ResourceId> {
    let (namespace, path) = block.parts();
    // Minecraft's magma block uses magma.png, including the retained full packs.
    let path = if namespace == "minecraft" && path == "magma_block" {
        "magma"
    } else {
        path
    };
    ResourceId::parse(&format!("{namespace}:block/{path}"))
}
fn transparent_block_medium(id: &ResourceId) -> Option<ResourceId> {
    let (namespace, path) = id.parts();
    if namespace != "minecraft" {
        return None;
    }
    // Only full glass blocks share a medium. Normalized geometry still decides
    // which quads cover an entire cell boundary; panes and partial models do not.
    let full_glass = path == "glass"
        || path.strip_suffix("_stained_glass").is_some_and(|color| {
            matches!(
                color,
                "white"
                    | "orange"
                    | "magenta"
                    | "light_blue"
                    | "yellow"
                    | "lime"
                    | "pink"
                    | "gray"
                    | "light_gray"
                    | "cyan"
                    | "purple"
                    | "blue"
                    | "brown"
                    | "green"
                    | "red"
                    | "black"
            )
        });
    full_glass.then(|| id.clone())
}
fn compatibility_reason(id: &ResourceId) -> Result<Label> {
    Label::new(&format!("Original Ilium geometry for {id}; selected-pack image required; source-native block model unverified"))
}
fn add_compatibility(
    definitions: &mut DefinitionSet,
    id: &ResourceId,
    goodvibes: bool,
    plasticator: bool,
    generated: bool,
    cancel: Cancel<'_>,
) -> Result<()> {
    let (_, path) = id.parts();
    let reason = compatibility_reason(id)?;
    if generated || !super::surface_state_geometry::is_remaining_generated_material(id.as_str()) {
        if let Some(geometry) =
            super::surface_state_geometry::definitions_for_profile(id.as_str(), plasticator)
        {
            for (name, model) in geometry.models {
                definitions.insert_model(&name, &model, reason.clone(), cancel)?;
            }
            return definitions.insert(
                ResourceKey {
                    kind: ResourceKind::Blockstate,
                    id: id.clone(),
                },
                &geometry.blockstate,
                reason,
                cancel,
            );
        }
    }
    if path == "grass_block" {
        let texture = |name| ResourceId::parse(&format!("minecraft:block/{name}"));
        return definitions.bind_grass(
            id.clone(),
            texture("grass_block_top")?,
            texture("dirt")?,
            texture("grass_block_side")?,
            goodvibes
                .then(|| texture("grass_block_side_overlay"))
                .transpose()?,
            reason,
            cancel,
        );
    }
    if path.ends_with("_log")
        || path.ends_with("_wood")
        || path.ends_with("_stem")
        || path.ends_with("_hyphae")
        || path.ends_with("_pillar")
    {
        let side = material_id(id)?;
        let end = if path.ends_with("_hyphae") || path.ends_with("_wood") {
            side.clone()
        } else {
            ResourceId::parse(&format!("minecraft:block/{path}_top"))?
        };
        return definitions.bind_column(id.clone(), side, end, reason, cancel);
    }
    let texture = material_id(id)?;
    let (parent, variable) = if path.ends_with("_leaves") {
        ("minecraft:block/leaves", "all")
    } else if ENTRIES.iter().any(|entry| entry.id == id.as_str())
        || (generated && id.as_str() == "minecraft:open_eyeblossom")
    {
        let tinted = matches!(
            path,
            "grass" | "fern" | "tall_grass" | "large_fern" | "vine" | "short_grass"
        );
        (
            if tinted {
                "minecraft:block/tinted_cross"
            } else {
                "minecraft:block/cross"
            },
            "cross",
        )
    } else {
        ("minecraft:block/cube_all", "all")
    };
    definitions.bind_single(
        id.clone(),
        ResourceId::parse(parent)?,
        variable,
        texture,
        reason,
        cancel,
    )
}

fn explicit_aliases(profile: &str) -> Result<BTreeMap<ResourceId, Vec<AssetPath>>> {
    let value: Value = serde_json::from_str(include_str!("pack_aliases.json"))
        .map_err(|error| AssetError::InvalidMetadata(format!("pack alias catalog: {error}")))?;
    let mut output = BTreeMap::new();
    if let Some(entries) = value
        .get("aliases")
        .and_then(|all| all.get(profile))
        .and_then(Value::as_object)
    {
        for (key, paths) in entries {
            let id = ResourceId::parse(key)?;
            let mut converted = Vec::new();
            if let Some(paths) = paths.as_array() {
                for path in paths.iter().filter_map(Value::as_str) {
                    converted.push(AssetPath::parse(path)?);
                }
            }
            output.insert(id, converted);
        }
    }
    Ok(output)
}
fn request(
    id: ResourceId,
    pack: &ResourceId,
    aliases: &BTreeMap<ResourceId, Vec<AssetPath>>,
    goodvibes: bool,
    fallback_pack: Option<&ResourceId>,
    fallback_goodvibes: bool,
) -> Result<TextureRequest> {
    let mut candidates = vec![TextureCandidate {
        pack: pack.clone(),
        location: TextureLocation::Resource {
            id: id.clone(),
            alias_reason: None,
        },
        schedule: ScheduleSource::AutomaticJava,
        expected_source_sha256: None,
        expected_image: ImageExpectations::default(),
    }];
    if goodvibes {
        candidates.push(TextureCandidate {pack:pack.clone(),
            location:TextureLocation::Literal{path:AssetPath::parse(&format!("textures/{}.png",id.parts().1))?,
                evidence:Label::new("GoodVibes extracted minecraft root: exact semantic textures/ path; admitted only if real member resolves")?},
            schedule:ScheduleSource::NoMetadata,expected_source_sha256:None,
            expected_image:ImageExpectations::default()});
    }
    if let Some(paths) = aliases.get(&id) {
        let cap = match (goodvibes, fallback_goodvibes) {
            (true, true) => 10,
            (false, true) => 12,
            (true, false) => 12,
            (false, false) => 14,
        };
        for path in paths.iter().take(cap) {
            candidates.push(TextureCandidate {
                pack: pack.clone(),
                location: TextureLocation::Literal {
                    path: path.clone(),
                    evidence: Label::new("Exact member in retained private full-pack catalog")?,
                },
                schedule: if goodvibes {
                    ScheduleSource::NoMetadata
                } else {
                    ScheduleSource::AutomaticJava
                },
                expected_source_sha256: None,
                expected_image: ImageExpectations::default(),
            });
        }
    }

    // Reviewed semantic compatibility for absent modern source members. The
    // candidate record keeps the selected image's real origin and this reason;
    // it must not be reported as a native texture for the requested state.
    let alternate = match id.parts().1 {
        "block/mud" => Some("minecraft:block/dirt"),
        "block/short_grass" | "block/tall_grass" | "block/bush" | "block/firefly_bush" => {
            Some("minecraft:block/grass")
        }
        _ => None,
    };
    if let Some(alternate) = alternate {
        let reason=Label::new(&format!("Authored selected-pack compatibility: {id} absent; visibly use {alternate}; not native {id} art"))?;
        let alternate = ResourceId::parse(alternate)?;
        candidates.push(TextureCandidate {
            pack: pack.clone(),
            location: TextureLocation::Resource {
                id: alternate.clone(),
                alias_reason: Some(reason.clone()),
            },
            schedule: ScheduleSource::AutomaticJava,
            expected_source_sha256: None,
            expected_image: ImageExpectations::default(),
        });
        if goodvibes {
            candidates.push(TextureCandidate {
                pack: pack.clone(),
                location: TextureLocation::Literal {
                    path: AssetPath::parse(&format!("textures/{}.png", alternate.parts().1))?,
                    evidence: reason,
                },
                schedule: ScheduleSource::NoMetadata,
                expected_source_sha256: None,
                expected_image: ImageExpectations::default(),
            });
        }
    }
    let mut requirement = TextureRequirement::selected_color(id.clone());
    if let Some(fallback) = fallback_pack {
        requirement.origin = RequiredOrigin::SelectedOrExplicitFullPackFallback;
        candidates.push(TextureCandidate {
            pack: fallback.clone(),
            location: TextureLocation::Resource {
                id: id.clone(),
                alias_reason: Some(Label::new("Reviewed full-pack texture fallback: exact missing semantic image; not selected-pack art")?),
            },
            schedule: ScheduleSource::AutomaticJava,
            expected_source_sha256: None,
            expected_image: ImageExpectations::default(),
        });
        if fallback_goodvibes {
            candidates.push(TextureCandidate {
                pack: fallback.clone(),
                location: TextureLocation::Literal {
                    path: AssetPath::parse(&format!("textures/{}.png", id.parts().1))?,
                    evidence: Label::new("Reviewed GoodVibes full-pack fallback: exact extracted-root image; not selected-pack art")?,
                },
                schedule: ScheduleSource::NoMetadata,
                expected_source_sha256: None,
                expected_image: ImageExpectations::default(),
            });
        }
    }
    if id.parts().1 == "block/composter_bottom" {
        // Textureless lacks its bottom image. Prefer an exact, reviewed full
        // pack bottom when provided; otherwise preserve the selected style
        // with its side image and an explicit semantic-compatibility label.
        candidates.push(TextureCandidate {
            pack: pack.clone(),
            location: TextureLocation::Resource {
                id: ResourceId::parse("minecraft:block/composter_side")?,
                alias_reason: Some(Label::new("Authored selected-pack compatibility: composter bottom absent; use selected composter side; not native bottom art")?),
            },
            schedule: ScheduleSource::AutomaticJava,
            expected_source_sha256: None,
            expected_image: ImageExpectations::default(),
        });
    }
    Ok(TextureRequest {
        requirement,
        candidates,
        missing_animation: MissingAnimation::StaticImage,
    })
}
// Plasticator's selected Java release supplies sprite strips for both lit
// campfire materials but no .mcmeta. Apply source-pinned, explicitly original
// compatibility to the direct selected candidate only. Saved cells, other
// packs, aliases and reviewed image fallbacks keep their ordinary policy.
fn apply_generated_plasticator_campfire_animation(
    request: &mut TextureRequest,
    selected_pack: &ResourceId,
    generated_plasticator_lit_campfire: bool,
) -> Result<()> {
    if !generated_plasticator_lit_campfire {
        return Ok(());
    }
    let (source_sha256, dimensions, missing) = match request.requirement.id.as_str() {
        "minecraft:block/campfire_fire" => {
            // Deliberate Ilium playback: 4 ticks (200 ms) per distinct source
            // cell, not a claim of pack-authored timing or interpolation.
            let frames = (0..8)
                .map(|index| ExplicitFrame {
                    rect: PixelRect {
                        x: 0,
                        y: index * 16,
                        width: 16,
                        height: 16,
                    },
                    ticks: 4,
                })
                .collect();
            (
                "1ba0cbe8ef1408a3cb0ba87d445cafcc9f57096c22f2012e21450c10bf2336ea",
                [16, 128],
                MissingAnimation::ExplicitFrames {
                    frames,
                    interpolate: false,
                    reason: Label::new("Original generated Plasticator campfire fire: eight distinct 16x16 source cells; Ilium compatibility playback at 4 ticks per frame, not pack-authored timing")?,
                },
            )
        }
        "minecraft:block/campfire_log_lit" => (
            "15490b1343de15a99151d706e3078e7c602ac9c98a526e88a15749f0b0168b8b",
            [16, 64],
            // All four source cells are pixel-identical. A static square crop
            // avoids invented timing while retaining exact selected pixels.
            MissingAnimation::StaticCrop {
                rect: PixelRect::whole(16, 16),
                reason: Label::new("Original generated Plasticator lit logs: four pixel-identical 16x16 source cells; Ilium static first-cell crop, not authored timing")?,
            },
        ),
        _ => return Ok(()),
    };
    let semantic = request.requirement.id.clone();
    let Some(candidate) = request.candidates.first_mut() else {
        return Err(AssetError::InvalidMetadata(
            "generated Plasticator campfire lacks a selected texture candidate".into(),
        ));
    };
    if &candidate.pack != selected_pack
        || !matches!(&candidate.location, TextureLocation::Resource { id, alias_reason: None } if id == &semantic)
        || !matches!(&candidate.schedule, ScheduleSource::AutomaticJava)
    {
        return Err(AssetError::InvalidMetadata(
            "generated Plasticator campfire direct source candidate changed".into(),
        ));
    }
    candidate.expected_source_sha256 = Some(Digest256::try_from(source_sha256.to_owned())?);
    candidate.expected_image.dimensions = Some(dimensions);
    candidate.schedule = ScheduleSource::AutomaticJavaWithMissing(missing);
    Ok(())
}

fn exposed(world: &SurfaceWorld, position: [i32; 3]) -> bool {
    [[0, 0, 1], [-1, 0, 0], [1, 0, 0], [0, -1, 0], [0, 1, 0]]
        .iter()
        .any(|step| {
            let neighbor = [
                position[0].saturating_add(step[0]),
                position[1].saturating_add(step[1]),
                position[2].saturating_add(step[2]),
            ];
            !world.blocks.contains_key(&neighbor)
        })
}

fn geometry_recovery_allowed(error: &AssetError, generated: bool, profile: &str) -> bool {
    // Preserve the existing reviewed Jicklus exception for supplied worlds.
    // Broader authored recovery is only for generated homage geometry.
    (generated
        && matches!(
            error,
            AssetError::Unsupported(_) | AssetError::InvalidMetadata(_)
        ))
        || matches!(error, AssetError::Unsupported(detail) if profile == "jicklus" && detail == "metadata field z")
}
fn uses_generated_material_geometry(id: &ResourceId, supplied: bool) -> bool {
    !supplied && super::surface_state_geometry::is_remaining_generated_material(id.as_str())
}
fn tint(world: &SurfaceWorld, position: [i32; 3], id: &ResourceId) -> [f32; 3] {
    super::surface_tint::color(
        world.biomes.get(&[position[0], position[1]]).copied(),
        id.parts().1,
    )
}

pub fn prepare(
    region: Region,
    settings: &VoxelLandscapeSettings,
    cancel: Cancel<'_>,
) -> Result<PreparedSurface> {
    prepare_with_fallback(region, settings, None, cancel)
}

/// The worker may supply a separately reviewed full pack for missing images.
/// Existing component callers keep the selected-only entry point above.
pub fn prepare_with_fallback(
    region: Region,
    settings: &VoxelLandscapeSettings,
    reviewed_fallback: Option<&VoxelLandscapeSettings>,
    cancel: Cancel<'_>,
) -> Result<PreparedSurface> {
    prepare_world(settings, reviewed_fallback, None, cancel, || {
        surface_generation::prepare(region, settings, || cancel.is_cancelled())
    })
}

/// Production generated viewport path. Component callers retain the bounded
/// single-window entry point; source tiles are stitched before one bank/model
/// binding so a frame has one budget, bank epoch, and global face-owner set.
pub fn prepare_viewport_with_fallback(
    region: Region,
    scale: f32,
    size: [usize; 2],
    settings: &VoxelLandscapeSettings,
    reviewed_fallback: Option<&VoxelLandscapeSettings>,
    cancel: Cancel<'_>,
) -> Result<PreparedSurface> {
    prepare_world(settings, reviewed_fallback, None, cancel, || {
        surface_generation::prepare_viewport(region, scale, size, settings, || {
            cancel.is_cancelled()
        })
    })
}

/// The scene worker shares one account across old snapshots, new banks and
/// full-frame receipts, including transitions between retained/streamed paths.
pub fn prepare_viewport_with_fallback_in_budget(
    region: Region,
    scale: f32,
    size: [usize; 2],
    settings: &VoxelLandscapeSettings,
    reviewed_fallback: Option<&VoxelLandscapeSettings>,
    budget: ByteBudget,
    cancel: Cancel<'_>,
) -> Result<PreparedSurface> {
    prepare_world_in_budget(settings, reviewed_fallback, None, budget, cancel, || {
        surface_generation::prepare_viewport(region, scale, size, settings, || {
            cancel.is_cancelled()
        })
    })
}

/// Bind exact supplied saved cells using the selected pack. This entry point
/// never calls terrain/feature/entity generation. Stored biome tint lookup and
/// original decoded palette ownership remain with the saved-world adapter.
/// Coordinates use ground-x, ground-y, height throughout this boundary.
pub fn prepare_supplied(
    world: SurfaceWorld,
    tints: BTreeMap<[i32; 3], [f32; 3]>,
    settings: &VoxelLandscapeSettings,
    cancel: Cancel<'_>,
) -> Result<PreparedSurface> {
    validate_supplied(&world, &tints, cancel)?;
    prepare_world(settings, None, Some(tints), cancel, || Ok(world))
}

fn validate_supplied(
    world: &SurfaceWorld,
    tints: &BTreeMap<[i32; 3], [f32; 3]>,
    cancel: Cancel<'_>,
) -> Result<()> {
    cancel.check()?;
    if (0..2).any(|axis| {
        world.region.minimum[axis] >= world.region.maximum[axis]
            || i64::from(world.region.maximum[axis]) - i64::from(world.region.minimum[axis]) > 256
    }) || world.blocks.len() > 1_000_000
        || world.fluids.len() > 1_000_000
        || !world.columns.is_empty()
        || !world.biomes.is_empty()
        || !world.trees.is_empty()
        || !world.flora.is_empty()
        || !world.structures.is_empty()
        || !world.entities.is_empty()
    {
        return Err(AssetError::InvalidMetadata(
            "invalid supplied saved-world bounds, count or generated metadata".into(),
        ));
    }
    for (&position, block) in &world.blocks {
        cancel.check()?;
        if !matches!(block.owner, surface_generation::SourceOwner::Saved { java_position }
            if [java_position[0], java_position[2], java_position[1]] == position)
            || !(-64..=319).contains(&position[2])
            || (0..2).any(|axis| {
                i64::from(position[axis]) < i64::from(world.region.minimum[axis]) - 1
                    || i64::from(position[axis]) > i64::from(world.region.maximum[axis])
            })
        {
            return Err(AssetError::InvalidMetadata(
                "supplied block lacks exact saved position or exceeds halo".into(),
            ));
        }
    }
    for (&position, color) in tints {
        cancel.check()?;
        if !world.blocks.contains_key(&position)
            || color
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        {
            return Err(AssetError::InvalidMetadata(
                "invalid supplied block tint".into(),
            ));
        }
    }
    for (&position, cell) in &world.fluids {
        cancel.check()?;
        if world.blocks.contains_key(&position)
            || !(-64..=319).contains(&position[2])
            || (0..2).any(|axis| {
                i64::from(position[axis]) < i64::from(world.region.minimum[axis]) - 1
                    || i64::from(position[axis]) > i64::from(world.region.maximum[axis])
            })
        {
            return Err(AssetError::InvalidMetadata(
                "supplied fluid overlaps a block or exceeds saved bounds".into(),
            ));
        }
        super::surface_fluid::FluidCell::new(cell.level, cell.tint)?;
    }
    cancel.check()
}

fn blocks_fluid_cell(block: &surface_generation::SurfaceBlock, generated: bool) -> bool {
    // Generated open root lattices contain water; full solid cells still
    // reject a fluid overlap. Supplied cells retain their adapter contract.
    !(generated
        && matches!(block.owner, surface_generation::SourceOwner::Tree { .. })
        && block.state.id().as_str() == "minecraft:mangrove_roots"
        && block.state.property("waterlogged") == Some("true"))
}

/// All generated viewport tiles retain this one mounted source and byte account.
/// The bank is imported only after the collection pass has seen every tile.
struct BindingSources {
    packs: Vec<LayeredPack>,
    limits: Limits,
    budget: ByteBudget,
    source_sha256: Option<Digest256>,
    profile_id: &'static str,
    goodvibes: bool,
    plasticator: bool,
    exact_plasticator_campfire_source: bool,
    aliases: BTreeMap<ResourceId, Vec<AssetPath>>,
    fallback_goodvibes: bool,
    fallback_unavailable: bool,
}

impl BindingSources {
    fn open_with_budget(
        settings: &VoxelLandscapeSettings,
        reviewed_fallback: Option<&VoxelLandscapeSettings>,
        budget: ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        let limits = Limits::default();
        let mounted = pack_sources::mount_selected(settings, budget.clone(), cancel)?;
        let source_sha256 = mounted.source_sha256;
        let profile = pack_profiles::profile(settings.pack_profile)?;
        let goodvibes = profile.id == "goodvibes";
        let plasticator = profile.id == "plasticator";
        // Only this reviewed Java source has the generated campfire override.
        let exact_plasticator_campfire_source = plasticator
            && source_sha256
                == Some(Digest256::try_from(
                    "afd418ccb268b209ae63a7f32722de06659c44454da713c7de5f516f62504048".to_owned(),
                )?);
        let aliases = explicit_aliases(profile.id)?;
        let review = mounted.pack.review();
        let (fallback, fallback_unavailable) = match reviewed_fallback {
            Some(settings) => {
                match pack_sources::mount_reviewed_fallback(settings, budget.clone(), cancel) {
                    Ok(mounted) => (Some(mounted), false),
                    Err(
                        error @ (AssetError::Cancelled
                        | AssetError::Allocation
                        | AssetError::Limit { .. }),
                    ) => return Err(error),
                    Err(error) => {
                        tracing::warn!("reviewed image fallback mount unavailable: {error}");
                        (None, true)
                    }
                }
            }
            None => (None, false),
        };
        let fallback_goodvibes = reviewed_fallback
            .map(|settings| {
                pack_profiles::profile(settings.pack_profile)
                    .map(|profile| profile.id == "goodvibes")
            })
            .transpose()?
            .unwrap_or(false);
        if fallback
            .as_ref()
            .is_some_and(|fallback| fallback.pack.review().pack == review.pack)
        {
            return Err(AssetError::InvalidReview(
                "image fallback duplicates selected pack".into(),
            ));
        }
        let mut packs = vec![mounted.pack];
        if let Some(fallback) = fallback {
            packs.push(fallback.pack);
        }
        Ok(Self {
            packs,
            limits,
            budget,
            source_sha256,
            profile_id: profile.id,
            goodvibes,
            plasticator,
            exact_plasticator_campfire_source,
            aliases,
            fallback_goodvibes,
            fallback_unavailable,
        })
    }
}

pub fn scene_budget() -> Result<ByteBudget> {
    ByteBudget::new(SCENE_BUDGET_BYTES)
}

fn prepare_world(
    settings: &VoxelLandscapeSettings,
    reviewed_fallback: Option<&VoxelLandscapeSettings>,
    supplied_tints: Option<BTreeMap<[i32; 3], [f32; 3]>>,
    cancel: Cancel<'_>,
    make_world: impl FnOnce() -> Result<SurfaceWorld>,
) -> Result<PreparedSurface> {
    prepare_world_in_budget(
        settings,
        reviewed_fallback,
        supplied_tints,
        scene_budget()?,
        cancel,
        make_world,
    )
}

fn prepare_world_in_budget(
    settings: &VoxelLandscapeSettings,
    reviewed_fallback: Option<&VoxelLandscapeSettings>,
    supplied_tints: Option<BTreeMap<[i32; 3], [f32; 3]>>,
    budget: ByteBudget,
    cancel: Cancel<'_>,
    make_world: impl FnOnce() -> Result<SurfaceWorld>,
) -> Result<PreparedSurface> {
    let sources = BindingSources::open_with_budget(settings, reviewed_fallback, budget, cancel)?;
    let world = make_world()?;
    prepare_world_from_sources(
        world,
        supplied_tints,
        &sources,
        None,
        None,
        None,
        settings,
        cancel,
    )?
    .ok_or_else(|| AssetError::InvalidMetadata("binding pass produced no surface".into()))
}

fn merge_collected_request(
    collected: &mut BTreeMap<ResourceId, TextureRequest>,
    request: TextureRequest,
    max_textures: usize,
) -> Result<()> {
    let id = request.requirement.id.clone();
    let pinned = request.candidates.iter().any(|candidate| {
        matches!(
            candidate.schedule,
            ScheduleSource::AutomaticJavaWithMissing(_)
        )
    });
    match collected.get_mut(&id) {
        Some(previous) if pinned => *previous = request,
        Some(_) => {}
        None => {
            collected.insert(id, request);
        }
    }
    if collected.len() > max_textures {
        return Err(AssetError::Limit {
            resource: "generated viewport texture identities",
            requested: collected.len() as u64,
            limit: max_textures as u64,
        });
    }
    Ok(())
}

fn needed_for_core_binding(core: Option<Region>, position: [i32; 3]) -> bool {
    core.is_none_or(|core| {
        (0..2).all(|axis| {
            i64::from(position[axis]) >= i64::from(core.minimum[axis]) - 1
                && position[axis] <= core.maximum[axis]
        })
    })
}

// Count actual candidates without allocating a dense volume or exposing the shell underside.
fn binding_slot_count(
    world: &SurfaceWorld,
    core: Option<Region>,
    cancel: Cancel<'_>,
) -> Result<usize> {
    let mut count = 0usize;
    for position in world.blocks.keys() {
        cancel.check()?;
        if !needed_for_core_binding(core, *position) {
            continue;
        }
        count += 1;
        if count > 1_000_000 {
            return Err(AssetError::Limit {
                resource: "generated binding candidates",
                requested: count as u64,
                limit: 1_000_000,
            });
        }
    }
    Ok(count)
}

// The normalized element transforms already precede blockstate quarter turns.
fn model_leaves_cell(state: &NormalizedState, cancel: Cancel<'_>) -> Result<bool> {
    for (_, model) in &state.applications {
        for quad in &model.quads {
            cancel.check()?;
            if quad
                .points
                .iter()
                .flatten()
                .any(|value| !(0.0..=1.0).contains(value))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Bound neighbors and published owners share a reservation until mesh preparation ends.
struct GeneratedInstances {
    instances: BTreeMap<[i32; 3], Arc<BoundModel>>,
    published: Vec<[i32; 3]>,
    _retained_charge: Reservation,
}

// Publication and attempted binding are separate: a culling witness may later become reachable.
fn generated_instances(
    world: &SurfaceWorld,
    core: Option<Region>,
    normalized: &[([i32; 3], Arc<NormalizedState>)],
    budget: &ByteBudget,
    cancel: Cancel<'_>,
    mut bind: impl FnMut([i32; 3]) -> Result<Option<Arc<BoundModel>>>,
) -> Result<GeneratedInstances> {
    cancel.check()?;
    let count = binding_slot_count(world, core, cancel)?;
    let stride = std::mem::size_of::<[i32; 3]>()
        + std::mem::size_of::<u8>()
        + std::mem::size_of::<usize>()
        + std::mem::size_of::<Option<Arc<BoundModel>>>();
    let bytes = count
        .checked_mul(stride)
        .and_then(|value| value.checked_add(4096))
        .ok_or(AssetError::Allocation)?;
    let _work_charge = budget.reserve(bytes as u64, cancel)?;
    let mut positions = Vec::new();
    positions
        .try_reserve_exact(count)
        .map_err(|_| AssetError::Allocation)?;
    let mut flags = Vec::<u8>::new();
    flags
        .try_reserve_exact(count)
        .map_err(|_| AssetError::Allocation)?;
    flags.resize(count, 0);
    let mut models = Vec::<Option<Arc<BoundModel>>>::new();
    models
        .try_reserve_exact(count)
        .map_err(|_| AssetError::Allocation)?;
    models.resize_with(count, || None);
    let mut queue = Vec::<usize>::new();
    queue
        .try_reserve_exact(count)
        .map_err(|_| AssetError::Allocation)?;
    for &position in world.blocks.keys() {
        cancel.check()?;
        if !needed_for_core_binding(core, position) {
            continue;
        }
        let index = positions.len();
        positions.push(position);
        let outside_core = core.is_some_and(|region| !region.contains(position));
        let escaped = match normalized.binary_search_by_key(&position, |(key, _)| *key) {
            Ok(index) => model_leaves_cell(&normalized[index].1, cancel)?,
            Err(_) => false,
        };
        if !outside_core && !escaped && !exposed(world, position) {
            continue;
        }
        flags[index] |= 1;
        queue.push(index);
    }
    let mut cursor = 0usize;
    while cursor < queue.len() {
        cancel.check()?;
        let index = queue[cursor];
        cursor += 1;
        let position = positions[index];
        if flags[index] & 2 == 0 {
            models[index] = bind(position)?;
            flags[index] |= 2;
        }
        let sealed = models[index]
            .as_ref()
            .is_some_and(|model| model.opaque_boundaries.iter().all(|opaque| *opaque));
        let publishes = core.is_none_or(|region| region.contains(position));
        for direction in Direction::ALL {
            cancel.check()?;
            let step = direction.step();
            let Some(neighbor) = position[0].checked_add(step[0]).and_then(|x| {
                position[1]
                    .checked_add(step[1])
                    .and_then(|y| position[2].checked_add(step[2]).map(|z| [x, y, z]))
            }) else {
                continue;
            };
            let Ok(other) = positions.binary_search(&neighbor) else {
                continue;
            };
            if publishes && flags[other] & 2 == 0 {
                models[other] = bind(neighbor)?;
                flags[other] |= 2;
            }
            if sealed || flags[other] & 1 != 0 {
                continue;
            }
            flags[other] |= 1;
            queue.push(other);
        }
    }
    let mut bound_count = 0usize;
    let mut published_count = 0usize;
    for index in 0..count {
        cancel.check()?;
        bound_count += usize::from(models[index].is_some());
        published_count += usize::from(
            flags[index] & 1 != 0 && core.is_none_or(|region| region.contains(positions[index])),
        );
    }
    let map_stride = std::mem::size_of::<([i32; 3], Arc<BoundModel>)>() + 64;
    let retained_bytes = bound_count
        .checked_mul(map_stride)
        .and_then(|value| {
            published_count
                .checked_mul(std::mem::size_of::<[i32; 3]>())
                .and_then(|keys| value.checked_add(keys))
        })
        .and_then(|value| value.checked_add(4096))
        .ok_or(AssetError::Allocation)?;
    let retained_charge = budget.reserve(retained_bytes as u64, cancel)?;
    let mut instances = BTreeMap::new();
    let mut published = Vec::new();
    published
        .try_reserve_exact(published_count)
        .map_err(|_| AssetError::Allocation)?;
    for (index, (position, model)) in positions.into_iter().zip(models).enumerate() {
        cancel.check()?;
        if flags[index] & 1 != 0 && core.is_none_or(|region| region.contains(position)) {
            published.push(position);
        }
        if let Some(model) = model {
            instances.insert(position, model);
        }
    }
    cancel.check()?;
    Ok(GeneratedInstances {
        instances,
        published,
        _retained_charge: retained_charge,
    })
}

/// A collection pass returns only exact requests. A binding pass consumes the
/// complete imported bank and emits faces owned by `core`, with the untrimmed
/// halo still available for exposed-face and fluid-neighbor decisions.
#[expect(
    clippy::too_many_arguments,
    reason = "source, owner core, import and cancellation have separate custody"
)]
fn prepare_world_from_sources(
    mut world: SurfaceWorld,
    supplied_tints: Option<BTreeMap<[i32; 3], [f32; 3]>>,
    sources: &BindingSources,
    core: Option<Region>,
    imports_override: Option<Arc<ImportResult>>,
    collect_requests: Option<&mut BTreeMap<ResourceId, TextureRequest>>,
    settings: &VoxelLandscapeSettings,
    cancel: Cancel<'_>,
) -> Result<Option<PreparedSurface>> {
    cancel.check()?;
    if let Some(core) = core {
        world.entities.retain(|entity| core.contains(entity.anchor));
    }
    let limits = sources.limits;
    let budget = sources.budget.clone();
    let source_sha256 = sources.source_sha256;
    let goodvibes = sources.goodvibes;
    let plasticator = sources.plasticator;
    let profile_id = sources.profile_id;
    let exact_plasticator_campfire_source = sources.exact_plasticator_campfire_source;
    let aliases = &sources.aliases;
    let fallback_goodvibes = sources.fallback_goodvibes;
    let fallback_unavailable = sources.fallback_unavailable;
    let packs = &sources.packs;
    let review = packs[0].review();
    let fallback_pack = packs.get(1).map(|pack| pack.review().pack.clone());
    let selected_is_bedrock = review.edition == super::assets::review::SourceEdition::Bedrock;
    let region = core.unwrap_or(world.region);
    let origin = BlobOrigin {
        pack: review.pack.clone(),
        release: review.release.clone(),
        layer: Label::new("Original Ilium geometry compatibility; not pack-authored model")?,
        path: AssetPath::parse("compatibility/root.json")?,
        review_digest: review.digest()?,
        kind: OriginKind::OriginalCompatibilityGeometry,
    };
    let mut compatibility = DefinitionSet::new(origin, limits, budget.clone())?;
    compatibility.install_geometry_templates(cancel)?;
    let ids: BTreeSet<_> = world
        .blocks
        .iter()
        .filter(|(position, _)| needed_for_core_binding(core, **position))
        .map(|(_, block)| block.state.id().clone())
        .collect();
    for id in &ids {
        add_compatibility(
            &mut compatibility,
            id,
            goodvibes,
            plasticator,
            supplied_tints.is_none(),
            cancel,
        )?;
    }
    // The additional full pack supplies only explicitly requested images. Its block
    // definitions must never change the selected pack's terrain geometry.
    let sources = DefinitionSources::new(&packs[..1], &[], Some(&compatibility))?;
    let mut compiler = ModelCompiler::new(&sources, limits, budget.clone())?;
    let mut original_compiler = ModelCompiler::new(&compatibility, limits, budget.clone())?;
    let collecting = collect_requests.is_some();
    let candidate_count = if supplied_tints.is_none() {
        binding_slot_count(&world, core, cancel)?
    } else {
        0
    };
    let _normalized_charge = if supplied_tints.is_none() && !collecting {
        let bytes = candidate_count
            .checked_mul(std::mem::size_of::<([i32; 3], Arc<NormalizedState>)>())
            .ok_or(AssetError::Allocation)?;
        Some(budget.reserve(bytes as u64, cancel)?)
    } else {
        None
    };
    let mut normalized = Vec::<([i32; 3], Arc<NormalizedState>)>::new();
    if supplied_tints.is_none() && !collecting {
        normalized
            .try_reserve_exact(candidate_count)
            .map_err(|_| AssetError::Allocation)?;
    }
    let mut normalized_cache = BTreeMap::new();
    let mut textures = BTreeSet::<ResourceId>::new();
    let mut generated_lit_campfire = false;
    let mut skipped_counts = BTreeMap::<String, u32>::new();
    let mut substitution_counts = BTreeMap::<String, u32>::new();
    for (&position, block) in &world.blocks {
        cancel.check()?;
        // The source world keeps the full generation halo for exposure, fluids,
        // climate and authored features. Only core faces are published, and
        // PreparedMesh reads bound neighbor geometry at most one cell away.
        // Binding the rest of the 16-cell halo wastes most of a tile's finite
        // account at low zoom without changing any published face.
        if !needed_for_core_binding(core, position) {
            continue;
        }
        // Discover exact selected states before opacity admission.
        // Collection drops each state after reading its textures.
        let generated_material =
            uses_generated_material_geometry(block.state.id(), supplied_tints.is_some());
        // These nine generated props deliberately use Ilium's state geometry.
        // Source-pack JSON can supply materials, but cannot replace their shape.
        // Saved cells retain their exact source-model selection policy.
        let selected_state = if generated_material {
            original_compiler.compile_state(&block.state, position, world.seed, cancel)
        } else {
            compiler.compile_state(&block.state, position, world.seed, cancel)
        };
        match selected_state {
            Ok(state) => {
                if generated_material
                    && block.state.id().as_str() == "minecraft:campfire"
                    && block.state.property("lit") == Some("true")
                {
                    generated_lit_campfire = true;
                }
                if generated_material {
                    *substitution_counts
                        .entry(format!(
                            "{}: original generated material geometry; requested pack images",
                            block.state.id(),
                        ))
                        .or_default() += 1;
                }
                for (_, model) in &state.applications {
                    for quad in &model.quads {
                        textures.insert(quad.texture.clone());
                    }
                }
                if !collecting {
                    normalized.push((
                        position,
                        retain_normalized(state, supplied_tints.is_some(), &mut normalized_cache)?,
                    ));
                }
            }
            Err(
                error @ (AssetError::Cancelled | AssetError::Allocation | AssetError::Limit { .. }),
            ) => return Err(error),
            Err(error)
                if !generated_material
                    && geometry_recovery_allowed(&error, supplied_tints.is_none(), profile_id) =>
            {
                // Retain the selected definition failure; images still follow
                // the explicit selected-first request and origin contract.
                let state =
                    original_compiler.compile_state(&block.state, position, world.seed, cancel)?;
                for (_, model) in &state.applications {
                    for quad in &model.quads {
                        textures.insert(quad.texture.clone());
                    }
                }
                if !collecting {
                    normalized.push((
                        position,
                        retain_normalized(state, supplied_tints.is_some(), &mut normalized_cache)?,
                    ));
                }
                *substitution_counts.entry(format!("{}: selected geometry {error}; original compatibility geometry with requested images",block.state.id())).or_default()+=1;
            }
            Err(error) => {
                *skipped_counts
                    .entry(format!("{}: {error}", block.state.id()))
                    .or_default() += 1;
            }
        }
    }
    drop(normalized_cache);
    drop(compiler);
    drop(original_compiler);
    drop(compatibility);
    if !world.fluids.is_empty() {
        textures.insert(ResourceId::parse("minecraft:block/water_still")?);
    }
    let mut requests: Vec<_> = textures
        .into_iter()
        .map(|id| {
            request(
                id,
                &review.pack,
                aliases,
                goodvibes,
                fallback_pack.as_ref(),
                fallback_goodvibes,
            )
        })
        .collect::<Result<_>>()?;
    for request in &mut requests {
        apply_generated_plasticator_campfire_animation(
            request,
            &review.pack,
            exact_plasticator_campfire_source && generated_lit_campfire,
        )?;
    }
    requests.extend(surface_entity_binding::requests(
        &world,
        &review.pack,
        fallback_pack.as_ref(),
        goodvibes,
        selected_is_bedrock,
    )?);
    if let Some(collected) = collect_requests {
        for request in requests {
            merge_collected_request(collected, request, limits.textures)?;
        }
        return Ok(None);
    }
    let imports = match imports_override {
        Some(imports) => {
            let recorded: BTreeSet<_> = imports
                .records
                .iter()
                .map(|record| &record.resource)
                .collect();
            for request in &requests {
                if !recorded.contains(&request.requirement.id) {
                    return Err(AssetError::InvalidMetadata(
                        "generated tile requests resource absent from shared bank".into(),
                    ));
                }
            }
            imports
        }
        None => {
            let importer = TextureImporter::new(packs, limits, budget.clone())?;
            Arc::new(importer.import(&requests, cancel)?)
        }
    };
    let mut entities = surface_entity_binding::bind(&world, &imports, &budget, cancel)?;
    if fallback_unavailable {
        entities
            .gaps
            .push("reviewed full-pack fauna fallback unavailable; selected-only art used".into());
    }
    let compatibility_aliases = imports
        .records
        .iter()
        .filter_map(|record| {
            record.candidates.iter().find_map(|candidate| {
                let label = candidate.alias_evidence.as_ref()?;
                (candidate.found_origin.as_ref() == record.source.as_ref()
                    && label
                        .as_str()
                        .starts_with("Authored selected-pack compatibility"))
                .then(|| format!("{}: {}", record.resource, label.as_str()))
            })
        })
        .collect();
    let material_fallbacks = imports
        .records
        .iter()
        .filter_map(|record| {
            let origin = record.source.as_ref()?;
            (origin.kind == OriginKind::ExplicitFullPackFallback
                && (record.resource.parts().1.starts_with("block/")
                    || record.resource.parts().1.starts_with("entity/bed/")
                    || record.resource.parts().1.starts_with("entity/chest/")
                    || record.resource.parts().1.starts_with("entity/bell/")))
            .then(|| {
                format!(
                    "{}: {} {} {}",
                    record.resource,
                    origin.pack,
                    origin.path,
                    record
                        .source_sha256
                        .map(|digest| digest.to_string())
                        .unwrap_or_default()
                )
            })
        })
        .collect();
    let mut instances = BTreeMap::new();
    // BoundModel geometry is block-local; PreparedMesh supplies a distinct
    // world position/FaceOwner for every instance. Keep this cache local to one
    // selected bank/account, and include exact choices, provenance and tint.
    let mut bound_cache = BTreeMap::<(ModelStateKey, [u32; 3]), Arc<BoundModel>>::new();
    let mut bind_one = |position: [i32; 3],
                        state: &NormalizedState|
     -> Result<Option<Arc<BoundModel>>> {
        cancel.check()?;
        let color = match &supplied_tints {
            Some(tints) => saved_tint(state, tints.get(&position).copied())?,
            None => tint(&world, position, state.state.id()),
        };
        let cache_key = (normalized_key(state)?, color.map(f32::to_bits));
        if let Some(model) = bound_cache.get(&cache_key) {
            return Ok(Some(Arc::clone(model)));
        }
        let mut rules = BTreeMap::new();
        for (_, model) in &state.applications {
            for quad in &model.quads {
                if let Some(handle) = imports.bank.resolve(&quad.texture) {
                    if let Some(texture) = imports.bank.texture(handle) {
                        let path = quad.texture.parts().1;
                        let alpha = texture_alpha(path, texture.image().info().alpha_min);
                        let layer = if path == "block/grass_block_side_overlay" {
                            1
                        } else {
                            0
                        };
                        rules.insert(
                            quad.texture.clone(),
                            TextureRenderRule {
                                alpha,
                                layer,
                                normal_map: None,
                                specular_map: None,
                            },
                        );
                    }
                }
            }
        }
        let mut tints = BTreeMap::new();
        tints.insert(0, color);
        let table = MaterialTable {
            medium: transparent_block_medium(state.state.id()),
            rules,
            tints,
        };
        match BoundModel::bind(state, &imports.bank, &table, &budget, cancel) {
            Ok(model) => {
                let model = Arc::new(model);
                if bound_cache.len() >= 4096 {
                    if supplied_tints.is_some() {
                        return Err(AssetError::InvalidMetadata(
                            "saved bound-model cache exceeds4096".into(),
                        ));
                    }
                } else {
                    bound_cache.insert(cache_key, Arc::clone(&model));
                }
                Ok(Some(model))
            }
            Err(
                error @ (AssetError::Cancelled | AssetError::Allocation | AssetError::Limit { .. }),
            ) => Err(error),
            Err(error) => {
                *skipped_counts
                    .entry(format!("{}: {error}", state.state.id()))
                    .or_default() += 1;
                Ok(None)
            }
        }
    };
    let generated = if supplied_tints.is_none() {
        Some(generated_instances(
            &world,
            core,
            &normalized,
            &budget,
            cancel,
            |position| {
                let Ok(index) = normalized.binary_search_by_key(&position, |(key, _)| *key) else {
                    return Ok(None);
                };
                bind_one(position, &normalized[index].1)
            },
        )?)
    } else {
        for (position, state) in &normalized {
            if let Some(model) = bind_one(*position, state)? {
                instances.insert(*position, model);
            }
        }
        None
    };
    drop(bound_cache);
    drop(normalized);
    drop(_normalized_charge);
    let instances = generated
        .as_ref()
        .map_or(&instances, |value| &value.instances);
    let published = generated.as_ref().map(|value| value.published.as_slice());
    let region = MeshRegion {
        minimum: region.minimum,
        maximum: region.maximum,
    };
    let mesh = PreparedMesh::build_with_admission(
        instances,
        region,
        imports.bank.identity(),
        1_000_000,
        &budget,
        cancel,
        published,
    )?;
    let fluid = if world.fluids.is_empty() {
        None
    } else if let Some(handle) = imports
        .bank
        .resolve(&ResourceId::parse("minecraft:block/water_still")?)
    {
        let solids = world
            .blocks
            .iter()
            .filter_map(|(position, block)| {
                blocks_fluid_cell(block, supplied_tints.is_none()).then_some(*position)
            })
            .collect();
        Some(FluidMesh::build(
            &world.fluids,
            &solids,
            region,
            &imports.bank,
            handle,
            1_000_000,
            &budget,
            cancel,
        )?)
    } else {
        *skipped_counts
            .entry("minecraft:block/water_still missing; retained bed and bank only".into())
            .or_default() += 1;
        None
    };
    let bank_epoch = imports.bank.identity();
    let mut world_hash = Sha256::new();
    world_hash.update(world.seed.to_le_bytes());
    world_hash.update([u8::from(settings.rivers)]);
    world_hash.update(settings.vegetation_percent.to_le_bytes());
    world_hash.update(settings.structures_percent.to_le_bytes());
    // A stored biome-only change also changes material identity. Generated
    // callers retain their existing epoch bytes exactly.
    if let Some(tints) = &supplied_tints {
        world_hash.update(b"saved-cell-tints");
        for (position, color) in tints {
            for coordinate in position {
                world_hash.update(coordinate.to_le_bytes());
            }
            for channel in color {
                world_hash.update(channel.to_le_bytes());
            }
        }
    }
    for edge in world.region.minimum.into_iter().chain(world.region.maximum) {
        world_hash.update(edge.to_le_bytes());
    }
    for (position, block) in &world.blocks {
        for coordinate in position {
            world_hash.update(coordinate.to_le_bytes());
        }
        world_hash.update(block.state.fingerprint().bytes());
        world_hash.update(format!("{:?}", block.owner).as_bytes());
    }
    for (position, cell) in &world.fluids {
        for coordinate in position {
            world_hash.update(coordinate.to_le_bytes());
        }
        world_hash.update([cell.level]);
        for color in cell.tint {
            world_hash.update(color.to_le_bytes());
        }
    }
    for entity in &world.entities {
        for coordinate in entity.anchor {
            world_hash.update(coordinate.to_le_bytes());
        }
        world_hash.update(entity.species.id().as_bytes());
    }
    let world_epoch = Digest256::try_from(format!("{:x}", world_hash.finalize()))?;
    let mut model_hash = Sha256::new();
    model_hash.update(bank_epoch.bytes());
    model_hash.update(world_epoch.bytes());
    for (position, model) in instances {
        for coordinate in position {
            model_hash.update(coordinate.to_le_bytes());
        }
        model_hash.update(model.state.fingerprint().bytes());
        for origin in &model.origins {
            model_hash.update(origin.sha256.bytes());
        }
    }
    for atlas in &entities.atlases {
        model_hash.update(atlas.semantic.as_bytes());
        model_hash.update(atlas.source_sha256.bytes());
        model_hash.update(atlas.source.pack.as_str().as_bytes());
    }
    for face in &entities.faces {
        for coordinate in face.anchor {
            model_hash.update(coordinate.to_le_bytes());
        }
        model_hash.update(face.owner.part.to_le_bytes());
        model_hash.update(face.owner.face.to_le_bytes());
        for point in face.points {
            for coordinate in point {
                model_hash.update(coordinate.to_le_bytes());
            }
        }
        for uv in face.uv {
            for coordinate in uv {
                model_hash.update(coordinate.to_le_bytes());
            }
        }
    }
    let model_epoch = Digest256::try_from(format!("{:x}", model_hash.finalize()))?;
    let skipped_states = skipped_counts
        .into_iter()
        .map(|(problem, count)| format!("{problem} ({count} blocks)"))
        .collect();
    let model_substitutions = substitution_counts
        .into_iter()
        .map(|(reason, count)| format!("{reason} ({count} blocks)"))
        .collect();
    cancel.check()?;
    Ok(Some(PreparedSurface {
        world,
        imports,
        mesh,
        fluid,
        entities,
        bank_epoch,
        world_epoch,
        model_epoch,
        skipped_states,
        compatibility_aliases,
        material_fallbacks,
        model_substitutions,
        source_sha256,
        budget,
    }))
}

/// Meshes retain only the geometry and identity needed to repaint a later
/// animation instant; the much larger source world and model compiler die
/// after each tile binds. A complete cache is published only after every tile
/// and the final pixel receipt fit in the one scene account.
struct BoundTile {
    mesh: PreparedMesh,
    fluid: Option<FluidMesh>,
    entities: PreparedEntityMesh,
    world_epoch: Digest256,
    model_epoch: Digest256,
    state_gaps: usize,
    model_substitutions: usize,
    fauna_gaps: usize,
    rendered_entities: usize,
    compatibility_aliases: usize,
    material_fallbacks: usize,
    fallback_atlas_semantics: Vec<String>,
}

fn bind_generated_tile(
    core: Region,
    halo: Region,
    settings: &VoxelLandscapeSettings,
    sources: &BindingSources,
    imports: &Arc<ImportResult>,
    cancel: Cancel<'_>,
) -> Result<BoundTile> {
    let world = surface_generation::prepare(halo, settings, || cancel.is_cancelled())?;
    let prepared = prepare_world_from_sources(
        world,
        None,
        sources,
        Some(core),
        Some(Arc::clone(imports)),
        None,
        settings,
        cancel,
    )?
    .ok_or_else(|| AssetError::InvalidMetadata("generated tile did not bind".into()))?;
    if prepared.bank_epoch != imports.bank.identity() {
        return Err(AssetError::InvalidMetadata(
            "generated tile bank/account mismatch".into(),
        ));
    }
    Ok(BoundTile {
        state_gaps: prepared.skipped_states.len(),
        model_substitutions: prepared.model_substitutions.len(),
        fauna_gaps: prepared.entities.gaps.len(),
        rendered_entities: prepared.entities.rendered_entities,
        compatibility_aliases: prepared.compatibility_aliases.len(),
        material_fallbacks: prepared.material_fallbacks.len(),
        fallback_atlas_semantics: prepared
            .entities
            .atlases
            .iter()
            .filter(|atlas| atlas.used_fallback)
            .map(|atlas| atlas.semantic.clone())
            .collect(),
        mesh: prepared.mesh,
        fluid: prepared.fluid,
        entities: prepared.entities,
        world_epoch: prepared.world_epoch,
        model_epoch: prepared.model_epoch,
    })
}

/// One worker-owned mounted source, imported bank and optional complete mesh
/// cache for a viewport key. Cache admission is opportunistic but never drops
/// a feature or emits a partial frame when the finite account is tight.
pub struct GeneratedViewportSession {
    sources: BindingSources,
    tiles: Vec<(Region, Region)>,
    imports: Arc<ImportResult>,
    cached_tiles: Option<Vec<BoundTile>>,
    region: Region,
    scale: f32,
    size: [usize; 2],
    settings: VoxelLandscapeSettings,
    reviewed_fallback: Option<VoxelLandscapeSettings>,
}
impl GeneratedViewportSession {
    pub fn open(
        region: Region,
        scale: f32,
        size: [usize; 2],
        settings: &VoxelLandscapeSettings,
        reviewed_fallback: Option<&VoxelLandscapeSettings>,
        budget: ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        let tiles = super::surface_viewport::visible_tiles(region, scale, size)?;
        let sources =
            BindingSources::open_with_budget(settings, reviewed_fallback, budget, cancel)?;
        let mut requests = BTreeMap::<ResourceId, TextureRequest>::new();
        for &(core, halo) in &tiles {
            cancel.check()?;
            let world = surface_generation::prepare(halo, settings, || cancel.is_cancelled())?;
            prepare_world_from_sources(
                world,
                None,
                &sources,
                Some(core),
                None,
                Some(&mut requests),
                settings,
                cancel,
            )?;
        }
        let requests: Vec<_> = requests.into_values().collect();
        let imports = Arc::new(
            TextureImporter::new(&sources.packs, sources.limits, sources.budget.clone())?
                .import(&requests, cancel)?,
        );
        Ok(Self {
            sources,
            tiles,
            imports,
            cached_tiles: None,
            region,
            scale,
            size,
            settings: settings.clone(),
            reviewed_fallback: reviewed_fallback.cloned(),
        })
    }

    pub fn matches(
        &self,
        region: Region,
        scale: f32,
        size: [usize; 2],
        settings: &VoxelLandscapeSettings,
        reviewed_fallback: Option<&VoxelLandscapeSettings>,
    ) -> bool {
        self.region == region
            && self.scale == scale
            && self.size == size
            && &self.settings == settings
            && self.reviewed_fallback.as_ref() == reviewed_fallback
    }

    pub fn render(
        &mut self,
        camera: [f64; 3],
        time: Duration,
        cancel: Cancel<'_>,
    ) -> Result<StreamedViewport> {
        cancel.check()?;
        let sources = &self.sources;
        let imports = &self.imports;
        let tiles = &self.tiles;
        let settings = &self.settings;
        let scale = self.scale;
        let size = self.size;
        let bank_epoch = imports.bank.identity();
        let coverage = imports.bank.coverage();
        let material_coverage = (coverage.required_satisfied, coverage.required_count);
        let light = SceneAtmosphere::from_index(settings.atmosphere).light();
        let mut pixels =
            match RasterFrame::new(size, RasterLimits::default(), &sources.budget, cancel) {
                Ok(pixels) => pixels,
                Err(AssetError::Limit {
                    resource: "working bytes",
                    ..
                }) if self.cached_tiles.is_some() => {
                    self.cached_tiles = None;
                    RasterFrame::new(size, RasterLimits::default(), &sources.budget, cancel)?
                }
                Err(error) => return Err(error),
            };
        let cached = self.cached_tiles.as_ref();
        let mut pending_cache = cached.is_none().then(|| Vec::with_capacity(tiles.len()));
        let mut state_gaps = 0usize;
        let mut model_substitutions = 0usize;
        let mut fauna_gaps = 0usize;
        let mut rendered_entities = 0usize;
        let mut compatibility_aliases = 0usize;
        let mut material_fallbacks = 0usize;
        let mut fallback_atlas_semantics = BTreeSet::new();
        let mut world_hash = Sha256::new();
        let mut model_hash = Sha256::new();
        world_hash.update(bank_epoch.bytes());
        model_hash.update(bank_epoch.bytes());
        for (_tile_index, &(core, halo)) in tiles.iter().enumerate() {
            cancel.check()?;
            #[cfg(test)]
            if std::env::var_os("ILIUM_VIEWPORT_TRACE_TILES").is_some() {
                eprintln!(
                    "{{\"type\":\"progress\",\"phase\":\"render_tile\",\"tile\":{_tile_index},\"count\":{},\"account_used\":{},\"account_peak\":{},\"submitted_quads\":{},\"projected_attempts\":{},\"visible_triangles\":{},\"sample_tests\":{}}}",
                    tiles.len(),
                    sources.budget.used(),
                    sources.budget.peak(),
                    pixels.submitted_quads(),
                    pixels.projected_attempts(),
                    pixels.visible_triangles(),
                    pixels.sample_tests()
                );
            }
            let generated = if cached.is_none() {
                let tile = match bind_generated_tile(core, halo, settings, sources, imports, cancel)
                {
                    Err(AssetError::Limit {
                        resource: "working bytes",
                        ..
                    }) if pending_cache
                        .as_ref()
                        .is_some_and(|cache| !cache.is_empty()) =>
                    {
                        // Retained old tiles are an optimization, never a
                        // reason to drop new world features or a frame.
                        pending_cache = None;
                        bind_generated_tile(core, halo, settings, sources, imports, cancel)?
                    }
                    other => other?,
                };
                Some(tile)
            } else {
                None
            };
            let tile = match cached {
                Some(retained) => retained.get(_tile_index).ok_or_else(|| {
                    AssetError::InvalidMetadata("incomplete generated tile cache".into())
                })?,
                None => generated
                    .as_ref()
                    .ok_or_else(|| AssetError::InvalidMetadata("generated tile absent".into()))?,
            };
            for edge in core.minimum.into_iter().chain(core.maximum) {
                world_hash.update(edge.to_le_bytes());
                model_hash.update(edge.to_le_bytes());
            }
            world_hash.update(tile.world_epoch.bytes());
            model_hash.update(tile.model_epoch.bytes());
            surface_raster::draw_mesh_layer(
                &tile.mesh,
                &imports.bank,
                camera,
                f64::from(scale),
                time,
                light,
                &mut pixels,
                cancel,
            )?;
            surface_entity_raster::draw_entity_mesh(
                &tile.entities,
                &imports.bank,
                &sources.budget,
                camera,
                f64::from(scale),
                time,
                light,
                &mut pixels,
                cancel,
            )?;
            // RasterFrame retains independent depth-ranked opaque and blend
            // fragments; submitting a tile's fluids before the next tile's solids
            // does not change the globally sorted final pixel result.
            if let Some(fluid) = &tile.fluid {
                surface_raster::draw_fluid_mesh(
                    fluid,
                    &imports.bank,
                    camera,
                    f64::from(scale),
                    time,
                    light,
                    &mut pixels,
                    cancel,
                )?;
            }
            state_gaps += tile.state_gaps;
            model_substitutions += tile.model_substitutions;
            fauna_gaps += tile.fauna_gaps;
            rendered_entities += tile.rendered_entities;
            // Alias and material records belong to the shared bank, so their
            // counts are identical on each tile rather than additive.
            compatibility_aliases = compatibility_aliases.max(tile.compatibility_aliases);
            material_fallbacks = material_fallbacks.max(tile.material_fallbacks);
            fallback_atlas_semantics.extend(tile.fallback_atlas_semantics.iter().cloned());
            if let (Some(cache), Some(generated)) = (pending_cache.as_mut(), generated) {
                cache.push(generated);
            }
        }
        cancel.check()?;
        let count = size[0].checked_mul(size[1]).ok_or(AssetError::Allocation)?;
        let receipt_bytes = count
            .checked_mul(
                std::mem::size_of::<[u8; 3]>()
                    + std::mem::size_of::<bool>()
                    + std::mem::size_of::<Option<FaceOwner>>(),
            )
            .ok_or(AssetError::Allocation)?;
        let receipt_charge = match sources.budget.reserve(receipt_bytes as u64, cancel) {
            Ok(charge) => charge,
            Err(AssetError::Limit {
                resource: "working bytes",
                ..
            }) if pending_cache.is_some() || self.cached_tiles.is_some() => {
                pending_cache = None;
                self.cached_tiles = None;
                sources.budget.reserve(receipt_bytes as u64, cancel)?
            }
            Err(error) => return Err(error),
        };
        let mut colors = Vec::new();
        let mut covered = Vec::new();
        let mut owners = Vec::new();
        colors
            .try_reserve_exact(count)
            .map_err(|_| AssetError::Allocation)?;
        covered
            .try_reserve_exact(count)
            .map_err(|_| AssetError::Allocation)?;
        owners
            .try_reserve_exact(count)
            .map_err(|_| AssetError::Allocation)?;
        for y in 0..size[1] {
            cancel.check()?;
            for x in 0..size[0] {
                let pixel = pixels.pixel(x, y)?;
                colors.push(if pixel.color.alpha() == 0. {
                    [0; 3]
                } else {
                    pixel
                        .color
                        .straight()
                        .map(surface_raster::linear_to_srgb_byte)
                });
                covered.push(pixel.color.alpha() > 0.);
                owners.push(pixel.front_owner);
            }
        }
        cancel.check()?;
        let fallback_atlases = fallback_atlas_semantics.len();
        let status = (material_coverage.0 < material_coverage.1
        || state_gaps > 0
        || model_substitutions > 0
        || fauna_gaps > 0
        || compatibility_aliases > 0
        || material_fallbacks > 0
        || fallback_atlases > 0)
        .then(|| format!(
            "Surface art {}/{}; {compatibility_aliases} texture aliases; {model_substitutions} model substitutions; {state_gaps} state gaps; {rendered_entities} textured fauna candidates; {fauna_gaps} fauna gaps; {fallback_atlases} fauna and {material_fallbacks} material images from reviewed full-pack fallback (not selected-native)",
            material_coverage.0, material_coverage.1,
        ));
        if let Some(cache) = pending_cache {
            if cache.len() != tiles.len() {
                return Err(AssetError::InvalidMetadata(
                    "incomplete generated tile cache".into(),
                ));
            }
            self.cached_tiles = Some(cache);
        }
        let retained_tiles = self.cached_tiles.as_ref().map_or(0, Vec::len);
        Ok(StreamedViewport {
            size,
            camera,
            time,
            colors,
            covered,
            owners,
            bank_epoch,
            world_epoch: Digest256::try_from(format!("{:x}", world_hash.finalize()))?,
            model_epoch: Digest256::try_from(format!("{:x}", model_hash.finalize()))?,
            source_sha256: sources.source_sha256,
            tile_count: tiles.len(),
            material_coverage,
            compatibility_aliases,
            material_fallbacks,
            fallback_atlases,
            status,
            account_peak: sources.budget.peak(),
            submitted_quads: pixels.submitted_quads(),
            projected_attempts: pixels.projected_attempts(),
            visible_triangles: pixels.visible_triangles(),
            sample_tests: pixels.sample_tests(),
            retained_tiles,
            _receipt_charge: receipt_charge,
        })
    }
}

/// Standalone component entry point. The production worker retains the
/// session and one account across successive complete frame receipts.
#[expect(
    clippy::too_many_arguments,
    reason = "viewport, source policy, camera and cancellation are distinct contracts"
)]
pub fn render_viewport_with_fallback(
    region: Region,
    scale: f32,
    size: [usize; 2],
    camera: [f64; 3],
    time: Duration,
    settings: &VoxelLandscapeSettings,
    reviewed_fallback: Option<&VoxelLandscapeSettings>,
    cancel: Cancel<'_>,
) -> Result<StreamedViewport> {
    let mut session = GeneratedViewportSession::open(
        region,
        scale,
        size,
        settings,
        reviewed_fallback,
        scene_budget()?,
        cancel,
    )?;
    session.render(camera, time, cancel)
}

#[cfg(test)]
mod streamed_viewport_witness {
    use super::*;
    use std::{path::Path, sync::atomic::AtomicBool};

    #[test]
    fn core_binding_keeps_every_immediate_neighbor_and_unfiltered_saved_worlds() {
        let core = Region {
            minimum: [0, 0],
            maximum: [96, 96],
        };
        for position in [[-1, 0, 10], [96, 0, 10], [0, -1, 10], [0, 96, 10]] {
            assert!(needed_for_core_binding(Some(core), position));
        }
        for position in [[-2, 0, 10], [97, 0, 10], [0, -2, 10], [0, 97, 10]] {
            assert!(!needed_for_core_binding(Some(core), position));
            assert!(needed_for_core_binding(None, position));
        }
    }

    /// Private pack bytes are intentionally outside the checkout. Invoke with
    /// ILIUM_VIEWPORT_CACHE_ROOT pointing at a reviewed sources.json registry.
    #[test]
    #[ignore = "requires the separately installed private full-pack registry"]
    fn selected_pack_zoom_25_100_400_pixel_receipts() {
        let root = std::env::var("ILIUM_VIEWPORT_CACHE_ROOT").unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let fallback = super::super::pack_registry::resolve_registered(
            &VoxelLandscapeSettings {
                pack_profile: 2,
                ..Default::default()
            },
            Path::new(&root),
            cancel,
        )
        .unwrap();
        for (zoom, size) in [(25, [720, 480]), (100, [360, 240]), (400, [160, 96])] {
            let settings = super::super::pack_registry::resolve_registered(
                &VoxelLandscapeSettings {
                    seed: 71839,
                    zoom_percent: zoom,
                    ..Default::default()
                },
                Path::new(&root),
                cancel,
            )
            .unwrap();
            let scale = 2.8 * zoom as f32 / 100.0;
            let camera = [64.0, 64.0, 80.0];
            let region = super::super::surface_viewport::region(camera, scale, size).unwrap();
            let budget = scene_budget().unwrap();
            let mut session = GeneratedViewportSession::open(
                region,
                scale,
                size,
                &settings,
                Some(&fallback),
                budget.clone(),
                cancel,
            )
            .unwrap();
            let receipt = session.render(camera, Duration::ZERO, cancel).unwrap();
            assert_eq!(receipt.colors.len(), size[0] * size[1]);
            assert_eq!(receipt.covered.len(), receipt.colors.len());
            assert!(receipt.account_peak <= budget.limit());
            assert!(receipt.submitted_quads <= 16_000_000);
            assert!(receipt.projected_attempts <= 16_000_000);
            assert!(receipt.visible_triangles <= RasterLimits::default().triangles);
            assert!(receipt.sample_tests <= RasterLimits::default().sample_tests);
            assert!(receipt.covered.iter().any(|value| *value));
            assert!(receipt.material_coverage.0 > 0);
            assert!(receipt
                .covered
                .iter()
                .zip(&receipt.owners)
                .all(|(covered, owner)| !covered || owner.is_some()));
            if zoom == 25 {
                assert_eq!(receipt.tile_count, 169);
            }
            let corners = [
                0,
                size[0] - 1,
                (size[1] - 1) * size[0],
                size[0] * size[1] - 1,
            ]
            .map(|index| receipt.covered[index]);
            if let Ok(capture_dir) = std::env::var("ILIUM_VIEWPORT_CAPTURE_DIR") {
                let mut rgba = Vec::with_capacity(receipt.colors.len() * 4);
                for (rgb, covered) in receipt.colors.iter().zip(&receipt.covered) {
                    rgba.extend_from_slice(&[
                        rgb[0],
                        rgb[1],
                        rgb[2],
                        if *covered { 255 } else { 0 },
                    ]);
                }
                let image =
                    image::RgbaImage::from_raw(size[0] as u32, size[1] as u32, rgba).unwrap();
                image
                    .save(
                        Path::new(&capture_dir)
                            .join(format!("zoom-{zoom}-{}x{}.png", size[0], size[1])),
                    )
                    .unwrap();
            }
            println!(
                "{}",
                serde_json::json!({
                    "type": "result", "zoom_percent": zoom, "size": size,
                    "tiles": receipt.tile_count,
                    "covered_pixels": receipt.covered.iter().filter(|value| **value).count(),
                    "corner_coverage": corners,
                    "account_peak": receipt.account_peak,
                    "submitted_quads": receipt.submitted_quads,
                    "projected_attempts": receipt.projected_attempts,
                    "visible_triangles": receipt.visible_triangles,
                    "sample_tests": receipt.sample_tests,
                    "retained_tiles": receipt.retained_tiles,
                    "bank_epoch": receipt.bank_epoch.to_string(),
                    "world_epoch": receipt.world_epoch.to_string(),
                    "model_epoch": receipt.model_epoch.to_string(),
                    "material_coverage": receipt.material_coverage,
                    "compatibility_aliases": receipt.compatibility_aliases,
                    "material_fallbacks": receipt.material_fallbacks,
                    "fallback_atlases": receipt.fallback_atlases,
                    "status": receipt.status.as_deref(),
                    "source_sha256": receipt.source_sha256.map(|value| value.to_string()),
                })
            );
            if zoom == 400 || (zoom == 25 && receipt.retained_tiles == receipt.tile_count) {
                // The previous receipt is still charged while a new animation
                // instant uses the same bank and finite account.
                let later = session
                    .render(camera, Duration::from_millis(400), cancel)
                    .unwrap();
                assert_eq!(later.bank_epoch, receipt.bank_epoch);
                assert_eq!(later.world_epoch, receipt.world_epoch);
                assert_eq!(later.model_epoch, receipt.model_epoch);
                assert!(later.account_peak <= budget.limit());
                assert_eq!(later.retained_tiles, later.tile_count);
                println!(
                    "{}",
                    serde_json::json!({
                        "type": "result", "zoom_percent": zoom,
                        "time_ms": 400,
                        "tiles": later.tile_count,
                        "retained_tiles": later.retained_tiles,
                        "account_peak_with_previous_receipt": later.account_peak,
                        "covered_pixels": later.covered.iter().filter(|value| **value).count(),
                        "bank_epoch": later.bank_epoch.to_string(),
                        "world_epoch": later.world_epoch.to_string(),
                        "model_epoch": later.model_epoch.to_string(),
                    })
                );
            }
        }
    }
}

#[cfg(test)]
mod supplied_tests {
    use super::super::{
        assets::block_state::BlockState,
        surface_generation::{SourceOwner, SurfaceBlock},
    };
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn streaming_identity_union_keeps_source_pinned_campfire_in_either_tile_order() {
        let selected = ResourceId::parse("test:selected").unwrap();
        let id = ResourceId::parse("minecraft:block/campfire_fire").unwrap();
        let plain = request(id.clone(), &selected, &BTreeMap::new(), false, None, false).unwrap();
        let mut pinned = plain.clone();
        apply_generated_plasticator_campfire_animation(&mut pinned, &selected, true).unwrap();
        for incoming in [
            [plain.clone(), pinned.clone()],
            [pinned.clone(), plain.clone()],
        ] {
            let mut collected = BTreeMap::new();
            for candidate in incoming {
                merge_collected_request(&mut collected, candidate, 8192).unwrap();
            }
            assert!(matches!(
                collected.get(&id).unwrap().candidates[0].schedule,
                ScheduleSource::AutomaticJavaWithMissing(_)
            ));
        }
    }

    #[test]
    fn glass_medium_preserves_color_namespace_and_partial_geometry_boundaries() {
        for block in [
            "minecraft:glass",
            "minecraft:red_stained_glass",
            "minecraft:light_blue_stained_glass",
        ] {
            let id = ResourceId::parse(block).unwrap();
            assert_eq!(transparent_block_medium(&id), Some(id));
        }
        for block in [
            "minecraft:glass_pane",
            "minecraft:red_stained_glass_pane",
            "minecraft:ice",
            "minecraft:unknown_stained_glass",
            "custom:glass",
        ] {
            let id = ResourceId::parse(block).unwrap();
            assert_eq!(transparent_block_medium(&id), None);
        }
        assert_ne!(
            transparent_block_medium(&ResourceId::parse("minecraft:red_stained_glass").unwrap()),
            transparent_block_medium(&ResourceId::parse("minecraft:blue_stained_glass").unwrap())
        );
    }

    #[test]
    fn magma_material_uses_its_real_texture_without_remapping_other_namespaces() {
        for (block, texture) in [
            ("minecraft:magma_block", "minecraft:block/magma"),
            ("minecraft:stone", "minecraft:block/stone"),
            ("custom:magma_block", "custom:block/magma_block"),
        ] {
            assert_eq!(
                material_id(&ResourceId::parse(block).unwrap()).unwrap(),
                ResourceId::parse(texture).unwrap()
            );
        }
    }

    #[test]
    fn only_generated_explicit_wet_root_lattices_admit_cooccupied_water() {
        let mut block = SurfaceBlock {
            state: BlockState::new(
                ResourceId::parse("minecraft:mangrove_roots").unwrap(),
                [("waterlogged".into(), "true".into())],
            )
            .unwrap(),
            owner: SourceOwner::Tree {
                anchor: [0, 0, 62],
                configuration: "minecraft:mangrove",
            },
        };
        assert!(!blocks_fluid_cell(&block, true));
        assert!(blocks_fluid_cell(&block, false));
        block.owner = SourceOwner::Saved {
            java_position: [0, 62, 0],
        };
        assert!(blocks_fluid_cell(&block, true));
        block.owner = SourceOwner::Tree {
            anchor: [0, 0, 62],
            configuration: "minecraft:mangrove",
        };
        block.state = BlockState::new(
            ResourceId::parse("minecraft:mangrove_roots").unwrap(),
            [("waterlogged".into(), "false".into())],
        )
        .unwrap();
        assert!(blocks_fluid_cell(&block, true));
        block.state = BlockState::new(
            ResourceId::parse("minecraft:oak_log").unwrap(),
            [("waterlogged".into(), "true".into())],
        )
        .unwrap();
        assert!(blocks_fluid_cell(&block, true));
    }

    fn world() -> SurfaceWorld {
        let mut blocks = BTreeMap::new();
        blocks.insert(
            [-17, 1041, -64],
            SurfaceBlock {
                state: BlockState::new(
                    ResourceId::parse("minecraft:oak_slab").unwrap(),
                    [
                        ("type".into(), "bottom".into()),
                        ("waterlogged".into(), "false".into()),
                    ],
                )
                .unwrap(),
                owner: SourceOwner::Saved {
                    java_position: [-17, -64, 1041],
                },
            },
        );
        SurfaceWorld {
            region: Region {
                minimum: [-32, 1024],
                maximum: [144, 1200],
            },
            seed: 0,
            columns: BTreeMap::new(),
            biomes: BTreeMap::new(),
            blocks,
            fluids: BTreeMap::new(),
            trees: Vec::new(),
            flora: Vec::new(),
            structures: Vec::new(),
            entities: Vec::new(),
            source_limitations: Vec::new(),
        }
    }
    #[test]
    fn image_fallback_is_exact_semantic_last_candidate_and_explicitly_admitted() {
        let id = ResourceId::parse("minecraft:block/mangrove_log").unwrap();
        let selected = ResourceId::parse("ilium:goodvibes").unwrap();
        let fallback = ResourceId::parse("ilium:whimscape").unwrap();
        let with_fallback = request(
            id.clone(),
            &selected,
            &BTreeMap::new(),
            true,
            Some(&fallback),
            false,
        )
        .unwrap();
        assert_eq!(
            with_fallback.requirement.origin,
            RequiredOrigin::SelectedOrExplicitFullPackFallback
        );
        assert!(
            with_fallback.candidates[..with_fallback.candidates.len() - 1]
                .iter()
                .all(|candidate| candidate.pack == selected)
        );
        let last = with_fallback.candidates.last().unwrap();
        assert_eq!(last.pack, fallback);
        assert!(
            matches!(&last.location, TextureLocation::Resource { id: candidate, alias_reason: Some(_) } if candidate == &id)
        );
        let selected_only = request(id, &selected, &BTreeMap::new(), false, None, false).unwrap();
        assert_eq!(
            selected_only.requirement.origin,
            RequiredOrigin::SelectedPack
        );
        assert_eq!(selected_only.candidates.len(), 1);
    }

    #[test]
    fn generated_plasticator_campfire_uses_square_source_pinned_candidate_plans() {
        use super::super::assets::animation::{AnimationEvidence, AnimationPlan};
        use std::time::Duration;

        let selected = ResourceId::parse("ilium-pack:plasticator").unwrap();
        let fallback = ResourceId::parse("ilium-pack:jicklus").unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(4 << 20).unwrap();
        for (resource, digest, dimensions, frame_count, next_y) in [
            (
                "minecraft:block/campfire_fire",
                "1ba0cbe8ef1408a3cb0ba87d445cafcc9f57096c22f2012e21450c10bf2336ea",
                [16, 128],
                8,
                32,
            ),
            (
                "minecraft:block/campfire_log_lit",
                "15490b1343de15a99151d706e3078e7c602ac9c98a526e88a15749f0b0168b8b",
                [16, 64],
                1,
                0,
            ),
        ] {
            let id = ResourceId::parse(resource).unwrap();
            let mut generated = request(
                id,
                &selected,
                &BTreeMap::new(),
                false,
                Some(&fallback),
                false,
            )
            .unwrap();
            apply_generated_plasticator_campfire_animation(&mut generated, &selected, true)
                .unwrap();
            assert!(matches!(
                &generated.missing_animation,
                MissingAnimation::StaticImage
            ));
            assert_eq!(generated.candidates.len(), 2);
            let direct = &generated.candidates[0];
            assert_eq!(direct.pack, selected);
            assert_eq!(direct.expected_image.dimensions, Some(dimensions));
            assert_eq!(
                direct.expected_source_sha256,
                Some(Digest256::try_from(digest.to_owned()).unwrap())
            );
            let ScheduleSource::AutomaticJavaWithMissing(missing) = &direct.schedule else {
                panic!("generated selected candidate lacks the local missing-metadata policy");
            };
            let plan = AnimationPlan::build(
                dimensions,
                None,
                missing,
                &Limits::default(),
                &budget,
                cancel,
            )
            .unwrap();
            assert_eq!(plan.frame_count(), frame_count);
            assert_eq!(plan.at(Duration::ZERO).current, PixelRect::whole(16, 16));
            assert_eq!(plan.at(Duration::from_millis(400)).current.y, next_y);
            assert!(!plan.authored_schedule());
            assert!(matches!(
                plan.evidence(),
                AnimationEvidence::Compatibility { reason, static_crop }
                    if reason.as_str().contains("not pack-authored timing")
                        || *static_crop && reason.as_str().contains("not authored timing")
            ));
            assert!(matches!(
                &generated.candidates[1].schedule,
                ScheduleSource::AutomaticJava
            ));
            assert_eq!(generated.candidates[1].expected_source_sha256, None);
            assert_eq!(generated.candidates[1].expected_image.dimensions, None);
        }
    }

    #[test]
    fn plasticator_campfire_compatibility_does_not_enter_saved_or_other_material_requests() {
        let selected = ResourceId::parse("ilium-pack:plasticator").unwrap();
        for resource in [
            "minecraft:block/campfire_fire",
            "minecraft:block/campfire_log_lit",
            "minecraft:block/campfire_log",
            "minecraft:block/composter_top",
        ] {
            let id = ResourceId::parse(resource).unwrap();
            let mut saved =
                request(id.clone(), &selected, &BTreeMap::new(), false, None, false).unwrap();
            apply_generated_plasticator_campfire_animation(&mut saved, &selected, false).unwrap();
            assert!(matches!(
                &saved.candidates[0].schedule,
                ScheduleSource::AutomaticJava
            ));
            assert_eq!(saved.candidates[0].expected_source_sha256, None);
            let mut generated =
                request(id, &selected, &BTreeMap::new(), false, None, false).unwrap();
            apply_generated_plasticator_campfire_animation(&mut generated, &selected, true)
                .unwrap();
            if !matches!(
                resource,
                "minecraft:block/campfire_fire" | "minecraft:block/campfire_log_lit"
            ) {
                assert!(matches!(
                    &generated.candidates[0].schedule,
                    ScheduleSource::AutomaticJava
                ));
                assert_eq!(generated.candidates[0].expected_source_sha256, None);
            }
        }
    }

    #[test]
    fn plasticator_campfire_fire_plan_respects_limits_and_cancellation() {
        use super::super::assets::animation::AnimationPlan;
        let selected = ResourceId::parse("ilium-pack:plasticator").unwrap();
        let id = ResourceId::parse("minecraft:block/campfire_fire").unwrap();
        let mut request = request(id, &selected, &BTreeMap::new(), false, None, false).unwrap();
        apply_generated_plasticator_campfire_animation(&mut request, &selected, true).unwrap();
        let ScheduleSource::AutomaticJavaWithMissing(missing) = &request.candidates[0].schedule
        else {
            panic!("expected generated compatibility plan");
        };
        let budget = ByteBudget::new(4 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let limits = Limits {
            animation_frames: 7,
            ..Limits::default()
        };
        assert!(matches!(
            AnimationPlan::build(
                [16, 128],
                None,
                missing,
                &limits,
                &budget,
                Cancel::new(&stop)
            ),
            Err(AssetError::Limit {
                resource: "animation frames",
                ..
            })
        ));
        let limits = Limits {
            frame_ticks: 3,
            ..Limits::default()
        };
        assert!(AnimationPlan::build(
            [16, 128],
            None,
            missing,
            &limits,
            &budget,
            Cancel::new(&stop)
        )
        .is_err());
        stop.store(true, std::sync::atomic::Ordering::Release);
        assert!(matches!(
            AnimationPlan::build(
                [16, 128],
                None,
                missing,
                &Limits::default(),
                &budget,
                Cancel::new(&stop)
            ),
            Err(AssetError::Cancelled)
        ));
    }
    #[test]
    fn missing_composter_bottom_prefers_exact_full_pack_then_labelled_selected_side() {
        let id = ResourceId::parse("minecraft:block/composter_bottom").unwrap();
        let selected = ResourceId::parse("ilium:textureless").unwrap();
        let fallback = ResourceId::parse("ilium:jicklus").unwrap();
        let request = request(
            id.clone(),
            &selected,
            &BTreeMap::new(),
            false,
            Some(&fallback),
            false,
        )
        .unwrap();
        assert_eq!(request.candidates.len(), 3);
        assert!(matches!(&request.candidates[2].location,
            TextureLocation::Resource { id: source, alias_reason: Some(reason) }
                if source.as_str() == "minecraft:block/composter_side"
                    && reason.as_str().contains("Authored selected-pack compatibility")));
        assert!(matches!(&request.candidates[1].location,
            TextureLocation::Resource { id: source, alias_reason: Some(_) }
                if source == &id && request.candidates[1].pack == fallback));
    }

    #[test]
    fn reviewed_goodvibes_fallback_reaches_its_extracted_root_without_changing_origin() {
        let id = ResourceId::parse("minecraft:entity/chest/normal").unwrap();
        let selected = ResourceId::parse("ilium:jicklus").unwrap();
        let fallback = ResourceId::parse("ilium:goodvibes").unwrap();
        let request = request(
            id,
            &selected,
            &BTreeMap::new(),
            false,
            Some(&fallback),
            true,
        )
        .unwrap();
        assert_eq!(request.candidates.len(), 3);
        assert_eq!(
            request.requirement.origin,
            RequiredOrigin::SelectedOrExplicitFullPackFallback
        );
        assert!(matches!(&request.candidates[2].location,
            TextureLocation::Literal { path, evidence }
                if path.as_str() == "textures/entity/chest/normal.png"
                    && evidence.as_str().contains("Reviewed GoodVibes full-pack fallback")));
        assert_eq!(request.candidates[2].pack, fallback);
    }

    #[test]
    fn generated_geometry_recovery_preserves_resource_failures_and_saved_policy() {
        let unsupported = AssetError::Unsupported("metadata field z".into());
        let malformed = AssetError::InvalidMetadata("duplicate source JSON key".into());
        assert!(geometry_recovery_allowed(&unsupported, true, "textureless"));
        assert!(geometry_recovery_allowed(&malformed, true, "jicklus"));
        assert!(!geometry_recovery_allowed(&malformed, false, "jicklus"));
        assert!(!geometry_recovery_allowed(
            &unsupported,
            false,
            "textureless"
        ));
        assert!(geometry_recovery_allowed(&unsupported, false, "jicklus"));
        for error in [
            AssetError::Cancelled,
            AssetError::Allocation,
            AssetError::Limit {
                resource: "bytes",
                requested: 2,
                limit: 1,
            },
        ] {
            assert!(!geometry_recovery_allowed(&error, true, "jicklus"));
        }
    }

    #[test]
    fn all_sixteen_observed_material_states_compile_with_original_resource_paths() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(64 << 20).unwrap();
        let mut definitions = DefinitionSet::new(
            super::super::assets::review::fixture_origin(OriginKind::DiagnosticFixture),
            Limits::default(),
            budget.clone(),
        )
        .unwrap();
        definitions.install_geometry_templates(cancel).unwrap();
        let cases: [(&str, &[(&str, &str)]); 16] = [
            (
                "minecraft:campfire",
                &[
                    ("facing", "north"),
                    ("lit", "false"),
                    ("signal_fire", "false"),
                    ("waterlogged", "false"),
                ],
            ),
            (
                "minecraft:campfire",
                &[
                    ("facing", "east"),
                    ("lit", "false"),
                    ("signal_fire", "false"),
                    ("waterlogged", "false"),
                ],
            ),
            (
                "minecraft:campfire",
                &[
                    ("facing", "south"),
                    ("lit", "false"),
                    ("signal_fire", "false"),
                    ("waterlogged", "false"),
                ],
            ),
            (
                "minecraft:campfire",
                &[
                    ("facing", "west"),
                    ("lit", "false"),
                    ("signal_fire", "false"),
                    ("waterlogged", "false"),
                ],
            ),
            (
                "minecraft:chest",
                &[
                    ("facing", "north"),
                    ("type", "single"),
                    ("waterlogged", "false"),
                ],
            ),
            (
                "minecraft:chest",
                &[
                    ("facing", "east"),
                    ("type", "single"),
                    ("waterlogged", "false"),
                ],
            ),
            (
                "minecraft:chest",
                &[
                    ("facing", "south"),
                    ("type", "single"),
                    ("waterlogged", "false"),
                ],
            ),
            (
                "minecraft:chest",
                &[
                    ("facing", "west"),
                    ("type", "single"),
                    ("waterlogged", "false"),
                ],
            ),
            ("minecraft:cauldron", &[]),
            (
                "minecraft:bell",
                &[
                    ("attachment", "ceiling"),
                    ("facing", "north"),
                    ("powered", "false"),
                ],
            ),
            ("minecraft:composter", &[("level", "0")]),
            (
                "minecraft:oak_fence_gate",
                &[
                    ("facing", "north"),
                    ("in_wall", "false"),
                    ("open", "true"),
                    ("powered", "false"),
                ],
            ),
            (
                "minecraft:jungle_fence_gate",
                &[
                    ("facing", "north"),
                    ("in_wall", "false"),
                    ("open", "true"),
                    ("powered", "false"),
                ],
            ),
            (
                "minecraft:acacia_fence_gate",
                &[
                    ("facing", "north"),
                    ("in_wall", "false"),
                    ("open", "true"),
                    ("powered", "false"),
                ],
            ),
            (
                "minecraft:spruce_fence_gate",
                &[
                    ("facing", "north"),
                    ("in_wall", "false"),
                    ("open", "true"),
                    ("powered", "false"),
                ],
            ),
            (
                "minecraft:spruce_fence_gate",
                &[
                    ("facing", "south"),
                    ("in_wall", "false"),
                    ("open", "true"),
                    ("powered", "false"),
                ],
            ),
        ];
        let mut admitted = BTreeSet::new();
        for (id, _) in cases {
            let id = ResourceId::parse(id).unwrap();
            if admitted.insert(id.clone()) {
                add_compatibility(&mut definitions, &id, false, false, true, cancel).unwrap();
            }
        }
        let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget).unwrap();
        for (id, properties) in cases {
            let state = BlockState::new(
                ResourceId::parse(id).unwrap(),
                properties
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            assert!(uses_generated_material_geometry(state.id(), false));
            assert!(!uses_generated_material_geometry(state.id(), true));
            let compiled = compiler
                .compile_state(&state, [0, 0, 64], 0, cancel)
                .unwrap();
            assert!(compiled.state_origin.compatibility.is_some(), "{id}");
            assert!(!compiled.applications.is_empty(), "{id}");
            assert!(
                compiled
                    .applications
                    .iter()
                    .flat_map(|(_, model)| &model.quads)
                    .all(|quad| quad.texture.parts().0 == "minecraft"),
                "{id}"
            );
            let expected = match id {
                "minecraft:campfire" => "minecraft:block/campfire_log",
                "minecraft:chest" => "minecraft:entity/chest/normal",
                "minecraft:cauldron" => "minecraft:block/cauldron_inner",
                "minecraft:bell" => "minecraft:entity/bell/bell_body",
                "minecraft:composter" => "minecraft:block/composter_top",
                "minecraft:oak_fence_gate" => "minecraft:block/oak_planks",
                "minecraft:jungle_fence_gate" => "minecraft:block/jungle_planks",
                "minecraft:acacia_fence_gate" => "minecraft:block/acacia_planks",
                "minecraft:spruce_fence_gate" => "minecraft:block/spruce_planks",
                _ => unreachable!(),
            };
            assert!(
                compiled
                    .applications
                    .iter()
                    .flat_map(|(_, model)| &model.quads)
                    .any(|quad| quad.texture.as_str() == expected),
                "{id}: {expected}"
            );
            if id == "minecraft:chest" {
                use super::super::assets::models::{oriented_quad, Direction};
                let (application, model) = &compiled.applications[0];
                let front = model
                    .quads
                    .iter()
                    .find(|quad| quad.face == Direction::North)
                    .unwrap();
                let normal = oriented_quad(front, application)
                    .unwrap()
                    .normal
                    .map(|v| v.round() as i32);
                let expected = match state.property("facing").unwrap() {
                    "north" => [0, -1, 0],
                    "east" => [1, 0, 0],
                    "south" => [0, 1, 0],
                    "west" => [-1, 0, 0],
                    _ => unreachable!(),
                };
                assert_eq!(normal, expected);
            }
        }
    }

    #[test]
    fn plasticator_bell_compiles_three_selected_face_resources_without_fake_entity_atlas() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(16 << 20).unwrap();
        let mut definitions = DefinitionSet::new(
            super::super::assets::review::fixture_origin(OriginKind::DiagnosticFixture),
            Limits::default(),
            budget.clone(),
        )
        .unwrap();
        definitions.install_geometry_templates(cancel).unwrap();
        let id = ResourceId::parse("minecraft:bell").unwrap();
        add_compatibility(&mut definitions, &id, false, true, true, cancel).unwrap();
        let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget).unwrap();
        let state = BlockState::new(
            id,
            [
                ("attachment".into(), "ceiling".into()),
                ("facing".into(), "north".into()),
                ("powered".into(), "false".into()),
            ],
        )
        .unwrap();
        let compiled = compiler
            .compile_state(&state, [0, 0, 64], 0, cancel)
            .unwrap();
        let textures: BTreeSet<_> = compiled
            .applications
            .iter()
            .flat_map(|(_, model)| &model.quads)
            .map(|quad| quad.texture.as_str())
            .collect();
        assert_eq!(
            textures,
            BTreeSet::from([
                "minecraft:block/bell_bottom",
                "minecraft:block/bell_side",
                "minecraft:block/bell_top",
            ])
        );
    }

    #[test]
    fn supplied_cells_keep_prior_compatibility_definition_if_jicklus_recovery_is_needed() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(16 << 20).unwrap();
        let mut definitions = DefinitionSet::new(
            super::super::assets::review::fixture_origin(OriginKind::DiagnosticFixture),
            Limits::default(),
            budget.clone(),
        )
        .unwrap();
        definitions.install_geometry_templates(cancel).unwrap();
        let id = ResourceId::parse("minecraft:chest").unwrap();
        add_compatibility(&mut definitions, &id, false, false, false, cancel).unwrap();
        let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget).unwrap();
        let state = BlockState::new(
            id,
            [
                ("facing".into(), "north".into()),
                ("type".into(), "single".into()),
                ("waterlogged".into(), "false".into()),
            ],
        )
        .unwrap();
        let compiled = compiler
            .compile_state(&state, [0, 0, 64], 0, cancel)
            .unwrap();
        assert!(compiled
            .applications
            .iter()
            .flat_map(|(_, model)| &model.quads)
            .all(|quad| quad.texture.as_str() == "minecraft:block/chest"));
    }

    #[test]
    fn saved_contract_accepts_retained_long_window_and_exact_signed_cell() {
        let stop = AtomicBool::new(false);
        let world = world();
        assert!(
            world.region.validate().is_err(),
            "generated region ceiling remains128"
        );
        validate_supplied(&world, &BTreeMap::new(), Cancel::new(&stop)).unwrap();
        assert_eq!(
            world.blocks[&[-17, 1041, -64]].state.property("type"),
            Some("bottom")
        );
        assert_eq!(
            world.blocks[&[-17, 1041, -64]]
                .state
                .property("waterlogged"),
            Some("false")
        );
    }

    #[test]
    fn saved_contract_refuses_generated_owners_wrong_axes_and_invalid_tint() {
        let stop = AtomicBool::new(false);
        let mut input = world();
        input.blocks.get_mut(&[-17, 1041, -64]).unwrap().owner = SourceOwner::Saved {
            java_position: [-17, 1041, -64],
        };
        assert!(validate_supplied(&input, &BTreeMap::new(), Cancel::new(&stop)).is_err());
        input.blocks.get_mut(&[-17, 1041, -64]).unwrap().owner = SourceOwner::Terrain {
            biome: super::super::surface_biomes::SurfaceBiome::Plains,
        };
        assert!(validate_supplied(&input, &BTreeMap::new(), Cancel::new(&stop)).is_err());
        let input = world();
        for tints in [
            BTreeMap::from([([-17, 1041, -64], [f32::NAN, 0.5, 0.5])]),
            BTreeMap::from([([0, 0, 0], [0.5; 3])]),
        ] {
            assert!(validate_supplied(&input, &tints, Cancel::new(&stop)).is_err());
        }
    }

    #[test]
    fn saved_contract_rejects_generated_metadata_and_cancels_before_pack_io() {
        let stop = AtomicBool::new(true);
        assert!(matches!(
            prepare_supplied(
                world(),
                BTreeMap::new(),
                &VoxelLandscapeSettings::default(),
                Cancel::new(&stop)
            ),
            Err(AssetError::Cancelled)
        ));
        let stop = AtomicBool::new(false);
        let mut input = world();
        input.biomes.insert(
            [-17, 1041],
            super::super::surface_biomes::SurfaceBiome::Plains,
        );
        assert!(validate_supplied(&input, &BTreeMap::new(), Cancel::new(&stop)).is_err());
    }

    fn cache_fixture(budget: &ByteBudget, cancel: Cancel<'_>) -> DefinitionSet {
        let mut definitions = DefinitionSet::new(
            super::super::assets::review::fixture_origin(OriginKind::DiagnosticFixture),
            Limits::default(),
            budget.clone(),
        )
        .unwrap();
        definitions.install_geometry_templates(cancel).unwrap();
        definitions
            .bind_column(
                ResourceId::parse("test:log").unwrap(),
                ResourceId::parse("test:block/bark").unwrap(),
                ResourceId::parse("test:block/end").unwrap(),
                Label::new("Synthetic saved cache regression").unwrap(),
                cancel,
            )
            .unwrap();
        definitions
    }

    #[test]
    fn saved_tint_requires_exact_input_only_for_actual_tinted_model_quads() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(8 << 20).unwrap();
        let mut definitions = cache_fixture(&budget, cancel);
        definitions
            .bind_grass(
                ResourceId::parse("test:grass").unwrap(),
                ResourceId::parse("test:block/top").unwrap(),
                ResourceId::parse("test:block/dirt").unwrap(),
                ResourceId::parse("test:block/side").unwrap(),
                None,
                Label::new("Synthetic saved tint requirement").unwrap(),
                cancel,
            )
            .unwrap();
        let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget).unwrap();
        let state = BlockState::new(
            ResourceId::parse("test:log").unwrap(),
            [("axis".into(), "y".into())],
        )
        .unwrap();
        let model = compiler
            .compile_state(&state, [0, 0, 64], 0, cancel)
            .unwrap();
        assert!(model
            .applications
            .iter()
            .all(|(_, model)| model.quads.iter().all(|quad| quad.tint_index.is_none())));
        assert_eq!(saved_tint(&model, None).unwrap(), [1.0; 3]);
        // Compile an actual tinted definition; the requirement comes from
        // its normalized quad, never a guessed saved block-name list.
        let grass = BlockState::new(
            ResourceId::parse("test:grass").unwrap(),
            Vec::<(String, String)>::new(),
        )
        .unwrap();
        let model = compiler
            .compile_state(&grass, [0, 0, 64], 0, cancel)
            .unwrap();
        assert!(model
            .applications
            .iter()
            .any(|(_, model)| model.quads.iter().any(|quad| quad.tint_index == Some(0))));
        assert!(saved_tint(&model, None).is_err());
        let exact = [0.1, 0.7, 0.3];
        assert_eq!(saved_tint(&model, Some(exact)).unwrap(), exact);
    }

    #[test]
    fn normalized_cache_shares_equivalent_geometry_without_charging_each_position() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(8 << 20).unwrap();
        let definitions = cache_fixture(&budget, cancel);
        let mut compiler =
            ModelCompiler::new(&definitions, Limits::default(), budget.clone()).unwrap();
        let state = BlockState::new(
            ResourceId::parse("test:log").unwrap(),
            [("axis".into(), "y".into())],
        )
        .unwrap();
        let mut cache = BTreeMap::new();
        let first = retain_normalized(
            compiler
                .compile_state(&state, [0, 0, 64], 0, cancel)
                .unwrap(),
            true,
            &mut cache,
        )
        .unwrap();
        let retained_charge = budget.used();
        for x in 1..256 {
            let next = retain_normalized(
                compiler
                    .compile_state(&state, [x, -x, 64], 0, cancel)
                    .unwrap(),
                true,
                &mut cache,
            )
            .unwrap();
            assert!(Arc::ptr_eq(&first, &next));
            assert_eq!(budget.used(), retained_charge);
        }
        assert_eq!(cache.len(), 1);
        let generated = retain_normalized(
            compiler
                .compile_state(&state, [0, 0, 64], 0, cancel)
                .unwrap(),
            false,
            &mut cache,
        )
        .unwrap();
        assert!(Arc::ptr_eq(&first, &generated));
        assert_eq!(budget.used(), retained_charge);
    }

    #[test]
    fn saved_cache_key_keeps_state_application_and_source_provenance() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(8 << 20).unwrap();
        let definitions = cache_fixture(&budget, cancel);
        let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget).unwrap();
        let state = BlockState::new(
            ResourceId::parse("test:log").unwrap(),
            [("axis".into(), "y".into())],
        )
        .unwrap();
        let mut normalized = compiler
            .compile_state(&state, [0, 0, 64], 0, cancel)
            .unwrap();
        let original = normalized_key(&normalized).unwrap();
        normalized.applications[0].0.uvlock = !normalized.applications[0].0.uvlock;
        assert!(normalized_key(&normalized).unwrap() != original);
        normalized.applications[0].0.uvlock = !normalized.applications[0].0.uvlock;
        normalized.applications[0].0.y_turns = 1;
        assert!(normalized_key(&normalized).unwrap() != original);
        normalized.applications[0].0.y_turns = 0;
        let original_compatibility = normalized.state_origin.compatibility.clone();
        normalized.state_origin.compatibility =
            Some(Label::new("Different source provenance").unwrap());
        assert!(normalized_key(&normalized).unwrap() != original);
        normalized.state_origin.compatibility = original_compatibility;
        assert!(normalized_key(&normalized).unwrap() == original);
        normalized.state = BlockState::new(
            ResourceId::parse("test:log").unwrap(),
            [("axis".into(), "x".into())],
        )
        .unwrap();
        assert!(normalized_key(&normalized).unwrap() != original);
    }
}

#[cfg(test)]
mod dense_ice_alpha_tests {
    use super::*;
    #[test]
    fn dense_ice_is_opaque_without_changing_ordinary_ice_and_glass() {
        for path in ["block/packed_ice", "block/blue_ice"] {
            assert_eq!(
                texture_alpha(path, 255),
                AlphaMode::Opaque,
                "{path} retains internal translucent faces"
            );
        }
        for path in [
            "block/ice",
            "block/glass",
            "block/tinted_glass",
            "block/red_stained_glass",
            "block/glass_pane_top",
        ] {
            assert_eq!(texture_alpha(path, 255), AlphaMode::Blend);
        }
        assert_eq!(
            texture_alpha("block/packed_ice", 0),
            AlphaMode::Cutout { threshold: 128 }
        );
        assert_eq!(
            texture_alpha("block/oak_leaves", 0),
            AlphaMode::Cutout { threshold: 128 }
        );
        assert_eq!(texture_alpha("block/stone", 255), AlphaMode::Opaque);
    }
}

#[cfg(test)]
#[path = "workstation_regressions.rs"]
mod workstation_regressions;

#[cfg(test)]
mod night_flora_binding_fallback_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    #[test]
    fn generated_eyeblossom_fallback_keeps_exact_cross_geometry() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(32 << 20).unwrap();
        let origin = super::super::assets::review::fixture_origin(OriginKind::DiagnosticFixture);
        let mut definitions =
            DefinitionSet::new(origin, Limits::default(), budget.clone()).unwrap();
        definitions.install_geometry_templates(cancel).unwrap();
        for name in ["closed", "open"] {
            let id = ResourceId::parse(&format!("minecraft:{name}_eyeblossom")).unwrap();
            add_compatibility(&mut definitions, &id, false, false, true, cancel).unwrap();
        }
        let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget).unwrap();
        let lo = (1.0 - std::f64::consts::FRAC_1_SQRT_2) / 2.0;
        let hi = 1.0 - lo;
        let expected = [
            [[hi, hi, 1.0], [lo, lo, 1.0], [lo, lo, 0.0], [hi, hi, 0.0]],
            [[lo, lo, 1.0], [hi, hi, 1.0], [hi, hi, 0.0], [lo, lo, 0.0]],
            [[hi, lo, 1.0], [lo, hi, 1.0], [lo, hi, 0.0], [hi, lo, 0.0]],
            [[lo, hi, 1.0], [hi, lo, 1.0], [hi, lo, 0.0], [lo, hi, 0.0]],
        ];
        for name in ["closed", "open"] {
            let id = ResourceId::parse(&format!("minecraft:{name}_eyeblossom")).unwrap();
            let state = BlockState::new(id, [("schedule_tick".into(), "true".into())]).unwrap();
            let normalized = compiler
                .compile_state(&state, [-33, -17, 65], 71839, cancel)
                .unwrap();
            assert_eq!(normalized.state, state);
            assert!(normalized.state_origin.compatibility.is_some());
            assert_eq!(normalized.applications.len(), 1);
            let model = &normalized.applications[0].1;
            let material = format!("minecraft:block/{name}_eyeblossom");
            assert_eq!(model.id.as_str(), material);
            assert_eq!(model.quads.len(), expected.len());
            for (quad, expected_points) in model.quads.iter().zip(expected) {
                assert_eq!(quad.texture.as_str(), material);
                assert!(
                    quad.tint_index.is_none()
                        && quad.cull_face.is_none()
                        && quad.complete_boundary.is_none()
                        && !quad.shade
                );
                assert_eq!(quad.uv, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
                for (actual, expected_point) in quad.points.iter().zip(expected_points) {
                    assert!(actual
                        .iter()
                        .zip(expected_point)
                        .all(|(a, e)| (*a - e).abs() < 1e-12));
                }
            }
        }
    }
}

#[cfg(test)]
mod night_flora_binding_tests {
    use super::super::assets::models::DefinitionProvider;
    use super::*;
    use std::sync::atomic::AtomicBool;
    #[test]
    fn eyeblossom_requests_keep_exact_selected_first() {
        let selected = ResourceId::parse("fixture:selected").unwrap();
        let fallback = ResourceId::parse("fixture:fallback").unwrap();
        for name in ["closed", "open"] {
            let id = ResourceId::parse(&format!("minecraft:block/{name}_eyeblossom")).unwrap();
            let requested = request(
                id.clone(),
                &selected,
                &BTreeMap::new(),
                false,
                Some(&fallback),
                false,
            )
            .unwrap();
            assert_eq!(requested.requirement.id, id);
            assert_eq!(
                requested.requirement.origin,
                RequiredOrigin::SelectedOrExplicitFullPackFallback
            );
            assert_eq!(requested.candidates.len(), 2);
            for (index, candidate) in requested.candidates.iter().enumerate() {
                assert_eq!(
                    &candidate.pack,
                    if index == 0 { &selected } else { &fallback }
                );
                assert!(
                    matches!(&candidate.location, TextureLocation::Resource { id: actual, alias_reason } if actual == &id && alias_reason.is_some() == (index == 1))
                );
            }
        }
    }
    #[test]
    fn eyeblossom_cache_keeps_counterpart_and_source() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(32 << 20).unwrap();
        let origin = super::super::assets::review::fixture_origin(OriginKind::DiagnosticFixture);
        let mut definitions =
            DefinitionSet::new(origin, Limits::default(), budget.clone()).unwrap();
        definitions.install_geometry_templates(cancel).unwrap();
        for name in ["closed", "open"] {
            let id = ResourceId::parse(&format!("minecraft:{name}_eyeblossom")).unwrap();
            add_compatibility(&mut definitions, &id, false, false, true, cancel).unwrap();
        }
        let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget).unwrap();
        let mut cache = BTreeMap::new();
        let mut retained = Vec::new();
        for name in ["closed", "open"] {
            let id = ResourceId::parse(&format!("minecraft:{name}_eyeblossom")).unwrap();
            let state = BlockState::new(id, [("schedule_tick".into(), "true".into())]).unwrap();
            let normalized = compiler
                .compile_state(&state, [-33, -17, 65], 71839, cancel)
                .unwrap();
            assert_eq!(normalized.state, state);
            assert!(normalized.state_origin.compatibility.is_some());
            assert_eq!(&normalized.state_origin.resource.id, state.id());
            assert_eq!(normalized.applications.len(), 1);
            let model = &normalized.applications[0].1;
            assert_eq!(
                model.id.as_str(),
                format!("minecraft:block/{name}_eyeblossom")
            );
            assert!(model
                .origins
                .iter()
                .any(|origin| origin.resource.id == model.id));
            for origin in std::iter::once(&normalized.state_origin).chain(&model.origins) {
                let input = definitions
                    .definition(&origin.resource, cancel)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    serde_json::to_vec(&origin.origin).unwrap(),
                    serde_json::to_vec(&input.document.origin).unwrap()
                );
                assert_eq!(origin.sha256, input.document.sha256);
            }
            let first = retain_normalized(normalized, false, &mut cache).unwrap();
            let repeated = retain_normalized(
                compiler
                    .compile_state(&state, [-33, -17, 65], 71839, cancel)
                    .unwrap(),
                false,
                &mut cache,
            )
            .unwrap();
            assert!(Arc::ptr_eq(&first, &repeated));
            retained.push(first);
        }
        assert!(!Arc::ptr_eq(&retained[0], &retained[1]));
        for change_path in [false, true] {
            let mut changed = compiler
                .compile_state(&retained[1].state, [-33, -17, 65], 71839, cancel)
                .unwrap();
            if change_path {
                changed.state_origin.origin.path =
                    AssetPath::parse("fixture/changed.json").unwrap();
            } else {
                changed.state_origin.sha256 = Digest256::of(b"different diagnostic bytes");
            }
            let changed = retain_normalized(changed, false, &mut cache).unwrap();
            assert!(!Arc::ptr_eq(&retained[1], &changed));
        }
    }
}

#[cfg(test)]
mod night_flora_mounted_tests {
    use super::super::assets::{
        bank::CoverageProblem,
        layers::MountLayout,
        models::Direction,
        pixels::fixture_png,
        review::{fixture_review, SourceEdition},
        source::{AssetSource, MemberInfo, SourceBytes},
        texture::LinearRgba,
    };
    use super::super::surface_raster::{draw_mesh, DirectionalLight};
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    struct MountedNightSource {
        members: BTreeMap<AssetPath, MemberInfo>,
        files: BTreeMap<AssetPath, SourceBytes>,
        _index_charge: Reservation,
        stop: Arc<AtomicBool>,
        cancel_open: AtomicBool,
    }
    impl MountedNightSource {
        fn new(
            entries: &[(&str, Option<&[u8]>)],
            budget: &ByteBudget,
            stop: Arc<AtomicBool>,
            cancel: Cancel<'_>,
        ) -> Result<Self> {
            cancel.check()?;
            let index_charge = budget.reserve(4096 + entries.len() as u64 * 4096, cancel)?;
            let mut source = Self {
                members: BTreeMap::new(),
                files: BTreeMap::new(),
                _index_charge: index_charge,
                stop,
                cancel_open: AtomicBool::new(false),
            };
            for &(path, bytes) in entries {
                cancel.check()?;
                let Some(bytes) = bytes else { continue };
                let path = AssetPath::parse(path)?;
                let retained = SourceBytes::from_slice(
                    bytes,
                    Limits::default().encoded_bytes,
                    budget,
                    cancel,
                )?;
                let info = MemberInfo {
                    path: path.clone(),
                    bytes: bytes.len() as u64,
                };
                if source.members.insert(path.clone(), info).is_some() {
                    return Err(AssetError::Duplicate(path.to_string()));
                }
                source.files.insert(path, retained);
            }
            Ok(source)
        }
    }
    impl AssetSource for MountedNightSource {
        fn members(&self) -> &BTreeMap<AssetPath, MemberInfo> {
            &self.members
        }
        fn source_digest(&self) -> Option<Digest256> {
            None
        }
        fn read(
            &self,
            path: &AssetPath,
            cap: u64,
            budget: &ByteBudget,
            cancel: Cancel<'_>,
        ) -> Result<Option<SourceBytes>> {
            cancel.check()?;
            if self
                .files
                .values()
                .next()
                .is_some_and(|stored| !stored.uses_budget(budget))
            {
                return Err(AssetError::InvalidMetadata(
                    "fixture source account differs".into(),
                ));
            }
            let Some(stored) = self.files.get(path) else {
                return Ok(None);
            };
            let bytes = SourceBytes::from_slice(stored.bytes(), cap, budget, cancel)?;
            if path.as_str().ends_with("/open_eyeblossom.png")
                && self.cancel_open.swap(false, Ordering::AcqRel)
            {
                self.stop.store(true, Ordering::Release);
            }
            cancel.check()?;
            Ok(Some(bytes))
        }
    }
    fn mounted_pack(
        role: OriginKind,
        images: [Option<&[u8]>; 2],
        metadata_case: u8,
        budget: &ByteBudget,
        stop: &Arc<AtomicBool>,
        cancel: Cancel<'_>,
    ) -> (LayeredPack, Arc<MountedNightSource>) {
        let closed_state: &[u8] =
            br#"{"variants":{"schedule_tick=true":{"model":"minecraft:block/closed_eyeblossom"}}}"#;
        let open_state: &[u8] = br#"{"variants":{"schedule_tick=true":{"model":"minecraft:block/open_eyeblossom","y":90}}}"#;
        let closed_model: &[u8] = br#"{"elements":[{"from":[0,0,0],"to":[16,0,16],"shade":false,"faces":{"up":{"texture":"minecraft:block/closed_eyeblossom","uv":[0,0,16,16]}}}]}"#;
        let open_model: &[u8] = br#"{"elements":[{"from":[0,8,0],"to":[8,8,16],"shade":false,"faces":{"up":{"texture":"minecraft:block/open_eyeblossom","uv":[0,0,16,16]}}}]}"#;
        let model_bytes = |value: &'static [u8]| -> &'static [u8] {
            if role == OriginKind::SelectedPack {
                value
            } else {
                &b"{\"elements\":[]}"[..]
            }
        };
        let entries = [
            (
                "assets/minecraft/blockstates/closed_eyeblossom.json",
                (metadata_case != 1).then_some(closed_state),
            ),
            (
                "assets/minecraft/blockstates/open_eyeblossom.json",
                (metadata_case != 1).then_some(open_state),
            ),
            (
                "assets/minecraft/models/block/closed_eyeblossom.json",
                (metadata_case != 2).then_some(model_bytes(closed_model)),
            ),
            (
                "assets/minecraft/models/block/open_eyeblossom.json",
                (metadata_case != 2).then_some(model_bytes(open_model)),
            ),
            (
                "assets/minecraft/textures/block/closed_eyeblossom.png",
                images[0],
            ),
            (
                "assets/minecraft/textures/block/open_eyeblossom.png",
                images[1],
            ),
        ];
        let source =
            Arc::new(MountedNightSource::new(&entries, budget, Arc::clone(stop), cancel).unwrap());
        let mut review = fixture_review();
        review.pack = ResourceId::parse(if role == OriginKind::SelectedPack {
            "fixture:night_selected"
        } else {
            "fixture:night_fallback"
        })
        .unwrap();
        review.edition = SourceEdition::Java;
        let capability: Arc<dyn AssetSource> = source.clone();
        let pack = LayeredPack::mount(
            review,
            capability,
            None,
            MountLayout::Java,
            role,
            Limits::default(),
            budget.clone(),
            cancel,
        )
        .unwrap();
        (pack, source)
    }
    fn mounted_origin(pack: &LayeredPack, path: &str, role: OriginKind) -> BlobOrigin {
        BlobOrigin {
            pack: pack.review().pack.clone(),
            release: pack.review().release.clone(),
            layer: Label::new("root").unwrap(),
            path: AssetPath::parse(path).unwrap(),
            review_digest: pack.review().digest().unwrap(),
            kind: role,
        }
    }
    fn mounted_compatibility(budget: &ByteBudget, cancel: Cancel<'_>) -> DefinitionSet {
        let origin =
            super::super::assets::review::fixture_origin(OriginKind::OriginalCompatibilityGeometry);
        let mut compatibility =
            DefinitionSet::new(origin, Limits::default(), budget.clone()).unwrap();
        compatibility.install_geometry_templates(cancel).unwrap();
        for name in ["closed", "open"] {
            add_compatibility(
                &mut compatibility,
                &ResourceId::parse(&format!("minecraft:{name}_eyeblossom")).unwrap(),
                false,
                false,
                true,
                cancel,
            )
            .unwrap();
        }
        compatibility
    }
    fn mounted_requests(packs: &[LayeredPack], fallback: bool) -> Vec<TextureRequest> {
        ["closed", "open"]
            .into_iter()
            .map(|name| {
                request(
                    ResourceId::parse(&format!("minecraft:block/{name}_eyeblossom")).unwrap(),
                    &packs[0].review().pack,
                    &BTreeMap::new(),
                    false,
                    if fallback {
                        Some(&packs[1].review().pack)
                    } else {
                        None
                    },
                    false,
                )
                .unwrap()
            })
            .collect()
    }
    fn assert_mounted_image(
        imports: &ImportResult,
        id: &ResourceId,
        origin: BlobOrigin,
        source_sha256: Digest256,
        rgba: [u8; 4],
    ) {
        let record = imports
            .records
            .iter()
            .find(|record| &record.resource == id)
            .unwrap();
        assert!(record.failure.is_none());
        assert_eq!(record.source.as_ref(), Some(&origin));
        assert_eq!(record.source_sha256, Some(source_sha256));
        assert_eq!(record.rgba_sha256, Some(Digest256::of(&rgba)));
        assert_eq!(record.dimensions, Some([1, 1]));
        let successful = record.candidates.last().unwrap();
        assert_eq!(successful.pack, origin.pack);
        assert!(successful.target_format.is_none());
        let evidence = (origin.kind == OriginKind::ExplicitFullPackFallback).then(|| Label::new("Reviewed full-pack texture fallback: exact missing semantic image; not selected-pack art").unwrap());
        assert_eq!(successful.alias_evidence, evidence);
        assert_eq!(successful.attempts.len(), 1);
        assert_eq!(
            (
                &successful.attempts[0].layer,
                &successful.attempts[0].path,
                successful.attempts[0].found,
                successful.attempts[0].source_sha256
            ),
            (&origin.layer, &origin.path, true, None)
        );
        assert_eq!(successful.found_origin.as_ref(), Some(&origin));
        assert_eq!(successful.source_sha256, Some(source_sha256));
        let texture = imports
            .bank
            .texture(imports.bank.resolve(id).unwrap())
            .unwrap();
        assert_eq!(texture.image().origin(), &origin);
        assert_eq!(texture.image().source_sha256(), source_sha256);
        assert_eq!(texture.image().rgba_sha256(), Digest256::of(&rgba));
        assert_eq!(texture.image().bytes(), rgba.as_slice());
        assert_eq!(
            texture
                .sample_color([0.5; 2], Duration::ZERO)
                .unwrap()
                .premultiplied(),
            rgba.map(|value| f32::from(value) / 255.0)
        );
    }
    fn assert_mounted_pixels(
        state: &NormalizedState,
        bank: &TextureBank,
        rgba: [u8; 4],
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) {
        let texture = state.applications[0].1.quads[0].texture.clone();
        let rules = BTreeMap::from([(
            texture,
            TextureRenderRule {
                alpha: AlphaMode::Opaque,
                layer: 0,
                normal_map: None,
                specular_map: None,
            },
        )]);
        let model = Arc::new(
            BoundModel::bind(
                state,
                bank,
                &MaterialTable {
                    medium: None,
                    rules,
                    tints: BTreeMap::new(),
                },
                budget,
                cancel,
            )
            .unwrap(),
        );
        let position = [-33, -17, 65];
        let mesh = PreparedMesh::build(
            &BTreeMap::from([(position, model)]),
            MeshRegion {
                minimum: [-34, -18],
                maximum: [-31, -15],
            },
            bank.identity(),
            100,
            budget,
            cancel,
        )
        .unwrap();
        let mut frame =
            RasterFrame::new([64, 64], RasterLimits::default(), budget, cancel).unwrap();
        draw_mesh(
            &mesh,
            bank,
            [-32.5, -16.5, 65.5],
            16.0,
            Duration::ZERO,
            DirectionalLight {
                ambient: 1.0,
                diffuse: 0.0,
                ..Default::default()
            },
            &mut frame,
            cancel,
        )
        .unwrap();
        let mut covered = 0;
        for index in 0..4096 {
            let pixel = frame.pixel(index % 64, index / 64).unwrap();
            let Some(owner) = pixel.front_owner else {
                assert_eq!(pixel.color, LinearRgba::CLEAR);
                continue;
            };
            assert_eq!((owner.position, owner.part, owner.layer), (position, 0, 0));
            assert_eq!(
                pixel.color.premultiplied(),
                rgba.map(|value| f32::from(value) / 255.0)
            );
            covered += 1;
        }
        assert!(covered > 0);
    }
    #[test]
    fn mounted_eyeblossoms_preserve_selected_metadata_images_and_pixels() {
        let limits = Limits::default();
        let budget = ByteBudget::new(limits.decoder_scratch_bytes + (32 << 20)).unwrap();
        {
            let stop = Arc::new(AtomicBool::new(false));
            let cancel = Cancel::new(&stop);
            let fixture_capacity = 4_u64 << 20;
            let _fixture_charge = budget.reserve(fixture_capacity, cancel).unwrap();
            let rgba = [
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [255, 255, 255, 255],
            ];
            let png = rgba.map(|pixel| fixture_png(1, 1, &pixel));
            assert!(
                png.iter().map(|bytes| bytes.capacity() as u64).sum::<u64>() <= fixture_capacity
            );
            let (selected, source) = mounted_pack(
                OriginKind::SelectedPack,
                [Some(&png[0]), Some(&png[1])],
                0,
                &budget,
                &stop,
                cancel,
            );
            let (fallback, _) = mounted_pack(
                OriginKind::ExplicitFullPackFallback,
                [Some(&png[2]), Some(&png[3])],
                0,
                &budget,
                &stop,
                cancel,
            );
            let packs = [selected, fallback];
            let compatibility = mounted_compatibility(&budget, cancel);
            let definitions =
                DefinitionSources::new(&packs[..1], &[], Some(&compatibility)).unwrap();
            let mut compiler = ModelCompiler::new(&definitions, limits, budget.clone()).unwrap();
            let mut original = ModelCompiler::new(&compatibility, limits, budget.clone()).unwrap();
            let requests = mounted_requests(&packs, true);
            let importer = TextureImporter::new(&packs, limits, budget.clone()).unwrap();
            let imports = importer.import(&requests, cancel).unwrap();
            assert_eq!(
                (
                    imports.bank.coverage().selected_texture_count,
                    imports.bank.coverage().explicit_fallback_texture_count,
                    imports.bank.coverage().required_satisfied
                ),
                (2, 0, 2)
            );
            for (index, name) in ["closed", "open"].into_iter().enumerate() {
                let state = BlockState::new(
                    ResourceId::parse(&format!("minecraft:{name}_eyeblossom")).unwrap(),
                    [("schedule_tick".into(), "true".into())],
                )
                .unwrap();
                let normalized = compiler
                    .compile_state(&state, [-33, -17, 65], 71839, cancel)
                    .unwrap();
                let other = original
                    .compile_state(&state, [-33, -17, 65], 71839, cancel)
                    .unwrap();
                assert_eq!(other.applications[0].1.quads.len(), 4);
                assert_eq!(normalized.state, state);
                assert_eq!(normalized.applications.len(), 1);
                let (application, model) = &normalized.applications[0];
                assert_eq!(
                    (
                        &application.model,
                        application.x_turns,
                        application.y_turns,
                        application.uvlock,
                        application.weight
                    ),
                    (&model.id, 0, index as u8, false, 1)
                );
                assert_eq!(model.quads.len(), 1);
                let quad = &model.quads[0];
                let points = if index == 0 {
                    [
                        [0.0, 0.0, 0.0],
                        [1.0, 0.0, 0.0],
                        [1.0, 1.0, 0.0],
                        [0.0, 1.0, 0.0],
                    ]
                } else {
                    [
                        [0.0, 0.0, 0.5],
                        [0.5, 0.0, 0.5],
                        [0.5, 1.0, 0.5],
                        [0.0, 1.0, 0.5],
                    ]
                };
                assert_eq!(quad.points, points);
                assert_eq!(quad.uv, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
                assert_eq!(
                    (quad.normal, quad.face, quad.element, quad.shade),
                    ([0.0, 0.0, 1.0], Direction::Up, 0, false)
                );
                assert_eq!(
                    (quad.tint_index, quad.cull_face, quad.complete_boundary),
                    (None, None, None)
                );
                assert_eq!(model.origins.len(), 1);
                assert_eq!(
                    model.id.as_str(),
                    format!("minecraft:block/{name}_eyeblossom")
                );
                assert_eq!(
                    normalized.state_origin.resource,
                    ResourceKey {
                        kind: ResourceKind::Blockstate,
                        id: state.id().clone()
                    }
                );
                assert_eq!(
                    model.origins[0].resource,
                    ResourceKey {
                        kind: ResourceKind::Model,
                        id: model.id.clone()
                    }
                );
                let state_path = format!("assets/minecraft/blockstates/{name}_eyeblossom.json");
                let model_path = format!("assets/minecraft/models/block/{name}_eyeblossom.json");
                for (origin, path) in [
                    (&normalized.state_origin, state_path.as_str()),
                    (&model.origins[0], model_path.as_str()),
                ] {
                    assert!(origin.compatibility.is_none());
                    assert_eq!(
                        origin.origin,
                        mounted_origin(&packs[0], path, OriginKind::SelectedPack)
                    );
                    assert_eq!(
                        origin.sha256,
                        source.files[&AssetPath::parse(path).unwrap()].digest()
                    );
                }
                let id = ResourceId::parse(&format!("minecraft:block/{name}_eyeblossom")).unwrap();
                assert!(model.quads.iter().all(|quad| quad.texture == id));
                let path = format!("assets/minecraft/textures/block/{name}_eyeblossom.png");
                assert_mounted_image(
                    &imports,
                    &id,
                    mounted_origin(&packs[0], &path, OriginKind::SelectedPack),
                    Digest256::of(&png[index]),
                    rgba[index],
                );
                assert_eq!(imports.records[index].candidates.len(), 1);
                assert_mounted_pixels(&normalized, &imports.bank, rgba[index], &budget, cancel);
            }
            let repeated = importer.import(&requests, cancel).unwrap();
            assert_eq!(repeated.bank.identity(), imports.bank.identity());
            drop(repeated);
            let before = budget.used();
            source.cancel_open.store(true, Ordering::Release);
            assert!(matches!(
                importer.import(&requests, cancel),
                Err(AssetError::Cancelled)
            ));
            assert!(!source.cancel_open.load(Ordering::Acquire) && stop.load(Ordering::Acquire));
            assert_eq!(budget.used(), before);
            stop.store(false, Ordering::Release);
            let recovered = importer.import(&requests, cancel).unwrap();
            assert_eq!(recovered.bank.identity(), imports.bank.identity());
            let foreign = ByteBudget::new(limits.decoder_scratch_bytes + (32 << 20)).unwrap();
            assert!(matches!(
                TextureImporter::new(&packs, limits, foreign.clone()),
                Err(AssetError::InvalidMetadata(_))
            ));
            let image_path =
                AssetPath::parse("assets/minecraft/textures/block/open_eyeblossom.png").unwrap();
            assert!(matches!(
                source.read(&image_path, u64::MAX, &foreign, cancel),
                Err(AssetError::InvalidMetadata(_))
            ));
            assert_eq!(foreign.used(), 0);
            assert!(matches!(
                source.read(&image_path, 0, &budget, cancel),
                Err(AssetError::Limit { .. })
            ));
        }
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn mounted_open_image_absence_and_failure_never_borrow_closed_art() {
        let limits = Limits::default();
        let budget = ByteBudget::new(limits.decoder_scratch_bytes + (32 << 20)).unwrap();
        {
            let stop = Arc::new(AtomicBool::new(false));
            let cancel = Cancel::new(&stop);
            let fixture_capacity = 4_u64 << 20;
            let _fixture_charge = budget.reserve(fixture_capacity, cancel).unwrap();
            let rgba = [
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [255, 255, 255, 255],
            ];
            let png = rgba.map(|pixel| fixture_png(1, 1, &pixel));
            assert!(
                png.iter().map(|bytes| bytes.capacity() as u64).sum::<u64>() <= fixture_capacity
            );
            for case in 0..5 {
                let selected_open: Option<&[u8]> = match case {
                    3 => Some(b"not a PNG"),
                    4 => Some(&png[1]),
                    _ => None,
                };
                let fallback_open = (case != 1).then_some(png[3].as_slice());
                let (selected, _) = mounted_pack(
                    OriginKind::SelectedPack,
                    [Some(&png[0]), selected_open],
                    0,
                    &budget,
                    &stop,
                    cancel,
                );
                let (fallback, fallback_source) = mounted_pack(
                    OriginKind::ExplicitFullPackFallback,
                    [Some(&png[2]), fallback_open],
                    0,
                    &budget,
                    &stop,
                    cancel,
                );
                let packs = [selected, fallback];
                let compatibility = mounted_compatibility(&budget, cancel);
                let definitions =
                    DefinitionSources::new(&packs[..1], &[], Some(&compatibility)).unwrap();
                let mut compiler =
                    ModelCompiler::new(&definitions, limits, budget.clone()).unwrap();
                let state = BlockState::new(
                    ResourceId::parse("minecraft:open_eyeblossom").unwrap(),
                    [("schedule_tick".into(), "true".into())],
                )
                .unwrap();
                let normalized = compiler
                    .compile_state(&state, [-33, -17, 65], 71839, cancel)
                    .unwrap();
                assert_eq!(normalized.applications[0].1.quads.len(), 1);
                assert_eq!(
                    normalized.applications[0].1.origins[0].origin.kind,
                    OriginKind::SelectedPack
                );
                let mut requests = mounted_requests(&packs, case != 2);
                if case == 4 {
                    requests[1].candidates[0].expected_source_sha256 =
                        Some(Digest256::of(b"wrong image pin"));
                }
                let imports = TextureImporter::new(&packs, limits, budget.clone())
                    .unwrap()
                    .import(&requests, cancel)
                    .unwrap();
                let open = ResourceId::parse("minecraft:block/open_eyeblossom").unwrap();
                let closed = ResourceId::parse("minecraft:block/closed_eyeblossom").unwrap();
                assert_mounted_image(
                    &imports,
                    &closed,
                    mounted_origin(
                        &packs[0],
                        "assets/minecraft/textures/block/closed_eyeblossom.png",
                        OriginKind::SelectedPack,
                    ),
                    Digest256::of(&png[0]),
                    rgba[0],
                );
                let record = imports
                    .records
                    .iter()
                    .find(|record| record.resource == open)
                    .unwrap();
                for (index, candidate) in record.candidates.iter().enumerate() {
                    assert_eq!(candidate.pack, packs[index].review().pack);
                    assert!(candidate.target_format.is_none());
                    let evidence = (index == 1).then(|| Label::new("Reviewed full-pack texture fallback: exact missing semantic image; not selected-pack art").unwrap());
                    assert_eq!(candidate.alias_evidence, evidence);
                    assert_eq!(candidate.attempts.len(), 1);
                    let attempt = &candidate.attempts[0];
                    assert_eq!(
                        (
                            &attempt.layer,
                            &attempt.path,
                            attempt.found,
                            attempt.source_sha256
                        ),
                        (
                            &Label::new("root").unwrap(),
                            &AssetPath::parse(
                                "assets/minecraft/textures/block/open_eyeblossom.png"
                            )
                            .unwrap(),
                            (case == 0 && index == 1) || case >= 3,
                            None
                        )
                    );
                }
                if case == 0 {
                    let path = "assets/minecraft/textures/block/open_eyeblossom.png";
                    assert_mounted_image(
                        &imports,
                        &open,
                        mounted_origin(&packs[1], path, OriginKind::ExplicitFullPackFallback),
                        fallback_source.files[&AssetPath::parse(path).unwrap()].digest(),
                        rgba[3],
                    );
                    assert_eq!(record.candidates.len(), 2);
                    assert!(record.candidates[0].found_origin.is_none());
                    assert!(record.candidates[0].source_sha256.is_none());
                    assert_eq!(
                        (
                            imports.bank.coverage().selected_texture_count,
                            imports.bank.coverage().explicit_fallback_texture_count,
                            imports.bank.coverage().required_satisfied
                        ),
                        (1, 1, 2)
                    );
                    assert_mounted_pixels(&normalized, &imports.bank, rgba[3], &budget, cancel);
                    continue;
                }
                assert!(
                    record.failure.is_some()
                        && record.source.is_none()
                        && record.source_sha256.is_none()
                        && record.rgba_sha256.is_none()
                        && record.dimensions.is_none()
                );
                assert!(imports.bank.resolve(&open).is_none());
                let row = imports
                    .bank
                    .coverage()
                    .rows
                    .iter()
                    .find(|row| row.requirement.id == open)
                    .unwrap();
                assert_eq!(row.problems, vec![CoverageProblem::Missing]);
                assert!(row.handle.is_none());
                assert_eq!(
                    (
                        imports.bank.coverage().selected_texture_count,
                        imports.bank.coverage().explicit_fallback_texture_count,
                        imports.bank.coverage().required_satisfied
                    ),
                    (1, 0, 1)
                );
                assert_eq!(record.candidates.len(), if case == 1 { 2 } else { 1 });
                if case >= 3 {
                    assert_eq!(
                        record.candidates[0].found_origin,
                        Some(mounted_origin(
                            &packs[0],
                            "assets/minecraft/textures/block/open_eyeblossom.png",
                            OriginKind::SelectedPack
                        ))
                    );
                    assert_eq!(
                        record.candidates[0].source_sha256,
                        Some(Digest256::of(selected_open.unwrap()))
                    );
                } else {
                    assert!(record
                        .candidates
                        .iter()
                        .all(|candidate| candidate.found_origin.is_none()
                            && candidate.source_sha256.is_none()));
                }
            }
        }
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn mounted_eyeblossom_model_precedence_remains_independent_of_blockstates() {
        let limits = Limits::default();
        let budget = ByteBudget::new(limits.decoder_scratch_bytes + (32 << 20)).unwrap();
        {
            let stop = Arc::new(AtomicBool::new(false));
            let cancel = Cancel::new(&stop);
            for metadata_case in [1, 2] {
                let (selected, source) = mounted_pack(
                    OriginKind::SelectedPack,
                    [None, None],
                    metadata_case,
                    &budget,
                    &stop,
                    cancel,
                );
                let (fallback, _) = mounted_pack(
                    OriginKind::ExplicitFullPackFallback,
                    [None, None],
                    0,
                    &budget,
                    &stop,
                    cancel,
                );
                let packs = [selected, fallback];
                let compatibility = mounted_compatibility(&budget, cancel);
                let definitions =
                    DefinitionSources::new(&packs[..1], &[], Some(&compatibility)).unwrap();
                let mut compiler =
                    ModelCompiler::new(&definitions, limits, budget.clone()).unwrap();
                for name in ["closed", "open"] {
                    let state = BlockState::new(
                        ResourceId::parse(&format!("minecraft:{name}_eyeblossom")).unwrap(),
                        [("schedule_tick".into(), "true".into())],
                    )
                    .unwrap();
                    let normalized = compiler
                        .compile_state(&state, [-33, -17, 65], 71839, cancel)
                        .unwrap();
                    assert_eq!(
                        normalized.state_origin.compatibility.is_some(),
                        metadata_case == 1
                    );
                    let model = &normalized.applications[0].1;
                    assert_eq!(model.quads.len(), if metadata_case == 1 { 1 } else { 4 });
                    if metadata_case != 1 {
                        continue;
                    }
                    let path = format!("assets/minecraft/models/block/{name}_eyeblossom.json");
                    assert!(model.origins[0].compatibility.is_none());
                    assert_eq!(
                        model.origins[0].origin,
                        mounted_origin(&packs[0], &path, OriginKind::SelectedPack)
                    );
                    assert_eq!(
                        model.origins[0].sha256,
                        source.files[&AssetPath::parse(&path).unwrap()].digest()
                    );
                }
            }
        }
        assert_eq!(budget.used(), 0);
    }
}

#[cfg(test)]
#[path = "surface_geology_asset_tests.rs"]
mod geology_asset_tests;
