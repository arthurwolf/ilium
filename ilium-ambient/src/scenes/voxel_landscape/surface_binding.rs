//! Worker-only bridge from semantic surface states to selected-pack pixels.
//! Every source lookup, image decode, model normalization and mesh build shares
//! one bounded account. Missing selected art stays explicit, including any
//! separately reviewed full-pack image used for the same semantic resource.
use super::{
    assets::{
        animation::MissingAnimation,
        bank::{RequiredOrigin, TextureBank, TextureRequirement},
        block_state::BlockState,
        budget::{ByteBudget, Cancel, Limits},
        compatibility::DefinitionSet,
        error::{AssetError, Result},
        identity::{AssetPath, BlobOrigin, Digest256, Label, OriginKind, ResourceId},
        importer::{
            DefinitionSources, ImportResult, ScheduleSource, TextureCandidate, TextureImporter,
            TextureLocation, TextureRequest,
        },
        layers::{ResourceKey, ResourceKind},
        models::{ModelCompiler, NormalizedState},
        pixels::ImageExpectations,
    },
    pack_profiles, pack_sources,
    settings::VoxelLandscapeSettings,
    surface_entity_binding::{self, PreparedEntityMesh},
    surface_flora_vocabulary::ENTRIES,
    surface_fluid::FluidMesh,
    surface_generation::{self, Region, SurfaceWorld},
    surface_mesh::{
        AlphaMode, BoundModel, MaterialTable, MeshRegion, PreparedMesh, TextureRenderRule,
    },
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const SCENE_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

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
    if !saved {
        return Ok(Arc::new(state));
    }
    let key = normalized_key(&state)?;
    if let Some(existing) = cache.get(&key) {
        return Ok(Arc::clone(existing));
    }
    if cache.len() >= 4096 {
        return Err(AssetError::InvalidMetadata(
            "saved model-state cache exceeds4096".into(),
        ));
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
    pub imports: ImportResult,
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

fn material_id(block: &ResourceId) -> Result<ResourceId> {
    let (namespace, path) = block.parts();
    ResourceId::parse(&format!("{namespace}:block/{path}"))
}
fn compatibility_reason(id: &ResourceId) -> Result<Label> {
    Label::new(&format!("Original Ilium geometry for {id}; selected-pack image required; source-native block model unverified"))
}
fn add_compatibility(
    definitions: &mut DefinitionSet,
    id: &ResourceId,
    goodvibes: bool,
    cancel: Cancel<'_>,
) -> Result<()> {
    let (_, path) = id.parts();
    let reason = compatibility_reason(id)?;
    if let Some(geometry) = super::surface_state_geometry::definitions(id.as_str()) {
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
    } else if ENTRIES.iter().any(|entry| entry.id == id.as_str()) {
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
        for path in paths.iter().take(if goodvibes { 12 } else { 14 }) {
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
                id,
                alias_reason: Some(Label::new("Reviewed full-pack texture fallback: exact missing semantic image; not selected-pack art")?),
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
fn tint(world: &SurfaceWorld, position: [i32; 3], id: &ResourceId) -> [f32; 3] {
    let biome = world.biomes.get(&[position[0], position[1]]);
    let (temperature, downfall) = biome
        .map(|biome| {
            let descriptor = biome.descriptor();
            (
                descriptor.gameplay_temperature.clamp(0., 1.),
                descriptor.downfall.clamp(0., 1.),
            )
        })
        .unwrap_or((0.6, 0.5));
    // Explicit authored approximation of climate tint; no claim of native colormap pixels.
    let dry = 1.0 - downfall * temperature;
    let rgb = if id.parts().1.ends_with("_leaves") {
        [
            0.32 + 0.22 * dry,
            0.62 + 0.22 * downfall,
            0.24 + 0.12 * (1. - temperature),
        ]
    } else {
        [
            0.40 + 0.25 * dry,
            0.67 + 0.18 * downfall,
            0.30 + 0.12 * (1. - temperature),
        ]
    };
    rgb.map(|v| v.clamp(0., 1.))
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

fn prepare_world(
    settings: &VoxelLandscapeSettings,
    reviewed_fallback: Option<&VoxelLandscapeSettings>,
    supplied_tints: Option<BTreeMap<[i32; 3], [f32; 3]>>,
    cancel: Cancel<'_>,
    make_world: impl FnOnce() -> Result<SurfaceWorld>,
) -> Result<PreparedSurface> {
    cancel.check()?;
    let limits = Limits::default();
    let budget = ByteBudget::new(SCENE_BUDGET_BYTES)?;
    let mounted = pack_sources::mount_selected(settings, budget.clone(), cancel)?;
    let source_sha256 = mounted.source_sha256;
    let profile = pack_profiles::profile(settings.pack_profile)?;
    let goodvibes = profile.id == "goodvibes";
    let aliases = explicit_aliases(profile.id)?;
    let world = make_world()?;
    let region = world.region;
    let review = mounted.pack.review().clone();
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
        .values()
        .map(|block| block.state.id().clone())
        .collect();
    for id in &ids {
        add_compatibility(&mut compatibility, id, goodvibes, cancel)?;
    }
    let (fallback, fallback_unavailable) = match reviewed_fallback {
        Some(settings) => {
            match pack_sources::mount_reviewed_fallback(settings, budget.clone(), cancel) {
                Ok(mounted) => (Some(mounted), false),
                Err(AssetError::Cancelled) => return Err(AssetError::Cancelled),
                Err(error) => {
                    tracing::warn!("reviewed image fallback mount unavailable: {error}");
                    (None, true)
                }
            }
        }
        None => (None, false),
    };
    let fallback_pack = fallback
        .as_ref()
        .map(|mounted| mounted.pack.review().pack.clone());
    let selected_is_bedrock = review.edition == super::assets::review::SourceEdition::Bedrock;
    let mut packs = vec![mounted.pack];
    if let Some(fallback) = fallback {
        if fallback.pack.review().pack == review.pack {
            return Err(AssetError::InvalidReview(
                "image fallback duplicates selected pack".into(),
            ));
        }
        packs.push(fallback.pack);
    }
    // The additional full pack supplies only explicitly requested images. Its block
    // definitions must never change the selected pack's terrain geometry.
    let sources = DefinitionSources::new(&packs[..1], &[], Some(&compatibility))?;
    let mut compiler = ModelCompiler::new(&sources, limits, budget.clone())?;
    let mut original_compiler = ModelCompiler::new(&compatibility, limits, budget.clone())?;
    let mut normalized = Vec::<([i32; 3], Arc<NormalizedState>)>::new();
    let mut normalized_cache = BTreeMap::new();
    let mut textures = BTreeSet::<ResourceId>::new();
    let mut skipped_counts = BTreeMap::<String, u32>::new();
    let mut substitution_counts = BTreeMap::<String, u32>::new();
    for (&position, block) in &world.blocks {
        cancel.check()?;
        // Saved transparent/partial neighbor states must reach the existing
        // model-aware boundary culler instead of occupancy-only rejection.
        if supplied_tints.is_none() && !exposed(&world, position) {
            continue;
        }
        match compiler.compile_state(&block.state, position, world.seed, cancel) {
            Ok(state) => {
                for (_, model) in &state.applications {
                    for quad in &model.quads {
                        textures.insert(quad.texture.clone());
                    }
                }
                normalized.push((
                    position,
                    retain_normalized(state, supplied_tints.is_some(), &mut normalized_cache)?,
                ));
            }
            Err(AssetError::Cancelled) => return Err(AssetError::Cancelled),
            Err(error)
                if geometry_recovery_allowed(&error, supplied_tints.is_none(), profile.id) =>
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
                normalized.push((
                    position,
                    retain_normalized(state, supplied_tints.is_some(), &mut normalized_cache)?,
                ));
                *substitution_counts.entry(format!("{}: selected geometry {error}; original compatibility geometry with requested images",block.state.id())).or_default()+=1;
            }
            Err(error) => {
                *skipped_counts
                    .entry(format!("{}: {error}", block.state.id()))
                    .or_default() += 1;
            }
        }
    }
    if !world.fluids.is_empty() {
        textures.insert(ResourceId::parse("minecraft:block/water_still")?);
    }
    let mut requests: Vec<_> = textures
        .into_iter()
        .map(|id| {
            request(
                id,
                &review.pack,
                &aliases,
                goodvibes,
                fallback_pack.as_ref(),
            )
        })
        .collect::<Result<_>>()?;
    requests.extend(surface_entity_binding::requests(
        &world,
        &review.pack,
        fallback_pack.as_ref(),
        goodvibes,
        selected_is_bedrock,
    )?);
    let importer = TextureImporter::new(&packs, limits, budget.clone())?;
    let imports = importer.import(&requests, cancel)?;
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
                    || record.resource.parts().1.starts_with("entity/bed/")))
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
    for (position, state) in normalized {
        cancel.check()?;
        let color = match &supplied_tints {
            Some(tints) => saved_tint(&state, tints.get(&position).copied())?,
            None => tint(&world, position, state.state.id()),
        };
        let cache_key = supplied_tints
            .as_ref()
            .map(|_| normalized_key(&state).map(|key| (key, color.map(f32::to_bits))))
            .transpose()?;
        if let Some(model) = cache_key.as_ref().and_then(|key| bound_cache.get(key)) {
            instances.insert(position, Arc::clone(model));
            continue;
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
            medium: None,
            rules,
            tints,
        };
        match BoundModel::bind(&state, &imports.bank, &table, &budget, cancel) {
            Ok(model) => {
                let model = Arc::new(model);
                if let Some(key) = cache_key {
                    if bound_cache.len() >= 4096 {
                        return Err(AssetError::InvalidMetadata(
                            "saved bound-model cache exceeds4096".into(),
                        ));
                    }
                    bound_cache.insert(key, Arc::clone(&model));
                }
                instances.insert(position, model);
            }
            Err(AssetError::Cancelled) => return Err(AssetError::Cancelled),
            Err(error) => {
                *skipped_counts
                    .entry(format!("{}: {error}", state.state.id()))
                    .or_default() += 1;
            }
        }
    }
    let region = MeshRegion {
        minimum: region.minimum,
        maximum: region.maximum,
    };
    let mesh = PreparedMesh::build(
        &instances,
        region,
        imports.bank.identity(),
        1_000_000,
        &budget,
        cancel,
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
    for (position, model) in &instances {
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
    Ok(PreparedSurface {
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
    })
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
        let selected_only = request(id, &selected, &BTreeMap::new(), false, None).unwrap();
        assert_eq!(
            selected_only.requirement.origin,
            RequiredOrigin::SelectedPack
        );
        assert_eq!(selected_only.candidates.len(), 1);
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
    fn saved_cache_shares_equivalent_geometry_without_charging_each_position() {
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
        assert!(!Arc::ptr_eq(&first, &generated));
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
