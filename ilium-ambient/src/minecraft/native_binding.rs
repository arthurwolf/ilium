//! Strict native-source binding for exact saved states with pinned render layers.
//!
//! Native fluid faces and waterlogged solid models share one bank and account.
//! This path never enters generated `prepare_world`, its authored
//! `DefinitionSet`, alias catalog, or skipped-state recovery path.
use super::{
    native_assets::{self, NativeSources},
    native_block_colors::{self, BlockColor},
    native_builtin::{self, BellBody, BuiltinKind, Recipe},
    native_fluid::{self},
    native_fluid_assembly,
    native_render_layer::{self, Layer, RenderLayers},
    native_tint::{self, ClimatePolicy, NativeRequest, NativeTint},
    saved_binding::SavedBinding,
    tours::PreparedMap,
};
use crate::voxel_landscape::{
    assets::{
        bank::TextureBank,
        block_state::BlockState,
        budget::{ByteBudget, Cancel, Reservation},
        error::AssetError,
        identity::{Digest256, OriginKind, ResourceId},
        importer::{DefinitionSources, ImportResult, TextureImporter},
        models::{ModelCompiler, NormalizedState},
    },
    surface_binding::PreparedSurface,
    surface_entity_binding,
    surface_fluid::FluidMesh,
    surface_mesh::{
        AlphaMode, BoundModel, BoundQuad, FaceMaterial, MaterialTable, MeshRegion, PreparedMesh,
        TextureRenderRule,
    },
    VoxelLandscapeSettings,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex, Weak},
};

const MAX_NORMALIZED_STATES: usize = 4096;
const MAX_BOUND_MODELS: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("native asset source: {0}")]
    NativeSource(#[from] native_assets::Error),
    #[error("native block color: {0}")]
    BlockColor(#[from] native_block_colors::Error),
    #[error("native biome color: {0}")]
    Tint(#[from] native_tint::Error),
    #[error("native builtin model: {0}")]
    Builtin(#[from] native_builtin::Error),
    #[error("native render-layer evidence: {0}")]
    RenderLayerEvidence(#[from] native_render_layer::Error),
    #[error(transparent)]
    Asset(#[from] AssetError),
    #[error("saved native binding requires an exact source map and world seed")]
    Source,
    #[error("saved native fluid surface: {0}")]
    Fluid(#[from] native_fluid_assembly::Error),
    #[error("saved state {0} requires an unavailable pinned native layer")]
    RenderLayer(String),
    #[error(
        "saved state {state} at {java_position:?} compiled to no block quads; a native built-in renderer is required"
    )]
    EmptyModel {
        state: String,
        java_position: [i32; 3],
    },
    #[error("native binding source/model limit: {0}")]
    Limit(&'static str),
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
fn normalized_key(state: &NormalizedState) -> Result<ModelStateKey, AssetError> {
    fn json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, AssetError> {
        serde_json::to_vec(value).map_err(|error| {
            AssetError::InvalidMetadata(format!("native model provenance: {error}"))
        })
    }
    let choices = state
        .applications
        .iter()
        .map(|(application, model)| {
            Ok(ModelChoiceKey {
                application: json(application)?,
                origins: json(&model.origins)?,
            })
        })
        .collect::<Result<Vec<_>, AssetError>>()?;
    Ok(ModelStateKey {
        state: state.state.clone(),
        origin: json(&state.state_origin)?,
        choices,
    })
}

type BoundKey = (ModelStateKey, Vec<(u16, [u8; 3])>);
#[derive(Default)]
struct ModelReuse {
    entries: BTreeMap<BoundKey, (Weak<BoundModel>, Reservation)>,
}
impl ModelReuse {
    fn prune(&mut self) {
        // Once per tile, never once per block. Dead Weak allocations and
        // their key/node charges are released together.
        self.entries
            .retain(|_, (model, _)| model.strong_count() != 0);
    }
    fn get(&self, key: &BoundKey) -> Option<Arc<BoundModel>> {
        self.entries.get(key).and_then(|(model, _)| model.upgrade())
    }
    fn remember(
        &mut self,
        key: &BoundKey,
        model: &Arc<BoundModel>,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<(), AssetError> {
        cancel.check()?;
        if self.entries.len() >= MAX_BOUND_MODELS || self.entries.contains_key(key) {
            return Ok(());
        }
        // The node/Weak/inline Arc allocation remains live after the model's
        // payload and reservation drop. Charge it plus every copied key
        // capacity BEFORE cloning; 4096 covers node and allocator overhead.
        let mut bytes = 4096usize
            .checked_add(std::mem::size_of::<BoundModel>() + 2 * std::mem::size_of::<usize>())
            .ok_or(AssetError::Allocation)?;
        let mut add = |amount: usize| -> Result<(), AssetError> {
            bytes = bytes.checked_add(amount).ok_or(AssetError::Allocation)?;
            Ok(())
        };
        add(key
            .0
            .state
            .id()
            .as_str()
            .len()
            .checked_mul(2)
            .ok_or(AssetError::Allocation)?)?;
        for (name, value) in key.0.state.properties() {
            add(512)?; // conservative BTree property node and String headers
            add(name.capacity())?;
            add(value.capacity())?;
        }
        add(key.0.origin.capacity())?;
        add(key
            .0
            .choices
            .capacity()
            .checked_mul(std::mem::size_of::<ModelChoiceKey>())
            .ok_or(AssetError::Allocation)?)?;
        for choice in &key.0.choices {
            add(choice.application.capacity())?;
            add(choice.origins.capacity())?;
        }
        add(key
            .1
            .capacity()
            .checked_mul(std::mem::size_of::<(u16, [u8; 3])>())
            .ok_or(AssetError::Allocation)?)?;
        let charge = match budget.reserve(
            u64::try_from(bytes).map_err(|_| AssetError::Allocation)?,
            cancel,
        ) {
            Ok(charge) => charge,
            Err(AssetError::Cancelled) => return Err(AssetError::Cancelled),
            Err(_) => return Ok(()), // optional reuse never rejects a source
        };
        self.entries
            .insert(key.clone(), (Arc::downgrade(model), charge));
        Ok(())
    }
}

/// Keeps the raw map and its logical copy reservation alive with the prepared
/// surface. The caller must reserve its decoded catalog/window and RenderCells
/// charges in the SAME `ByteBudget` before invoking this preparation.
pub struct PreparedNative {
    pub surface: PreparedSurface,
    pub map: Arc<PreparedMap>,
    pub source_profile: &'static str,
    pub native_archive_sha256: Digest256,
    pub selected_archive_sha256: Option<Digest256>,
    _state_copy: Reservation,
}

/// Retains only geometry, one shared immutable bank Arc, and the charged
/// projected source map after a worker has finished a tile. Raw SurfaceWorld
/// copies, normalization maps and their temporary reservation drop here.
pub struct NativeTile {
    pub mesh: PreparedMesh,
    pub fluid: Option<FluidMesh>,
    pub imports: Arc<ImportResult>,
    pub map: Arc<PreparedMap>,
    pub source_profile: &'static str,
    pub native_archive_sha256: Digest256,
    pub selected_archive_sha256: Option<Digest256>,
    pub bank_epoch: Digest256,
    pub model_epoch: Digest256,
}
impl NativeTile {
    pub fn bank(&self) -> &TextureBank {
        &self.imports.bank
    }
}
impl PreparedNative {
    pub fn into_tile(self) -> NativeTile {
        let PreparedNative {
            surface,
            map,
            source_profile,
            native_archive_sha256,
            selected_archive_sha256,
            ..
        } = self;
        let PreparedSurface {
            mesh,
            fluid,
            imports,
            bank_epoch,
            model_epoch,
            ..
        } = surface;
        NativeTile {
            mesh,
            fluid,
            imports,
            map,
            source_profile,
            native_archive_sha256,
            selected_archive_sha256,
            bank_epoch,
            model_epoch,
        }
    }
}

/// Worker-only source and selected-bank transaction. One imported bank is
/// shared by every sparse tile; its Arc and all BoundModel/mesh reservations
/// belong to this same account. The caller first visits every projected tile
/// to collect exact selected model/fluids/builtin texture IDs, then imports
/// once, then binds the same tile set. No missing ID can become a fallback.
pub struct NativeSourceSession {
    sources: NativeSources,
    layers: RenderLayers,
    budget: ByteBudget,
    _ids_charge: Reservation,
}

pub struct SharedNative {
    sources: NativeSources,
    layers: RenderLayers,
    imports: Arc<ImportResult>,
    budget: ByteBudget,
    model_reuse: Mutex<ModelReuse>,
}
impl SharedNative {
    pub fn bank_epoch(&self) -> Digest256 {
        self.imports.bank.identity()
    }

    pub fn definitions(&self) -> Result<DefinitionSources<'_>, Error> {
        Ok(self.sources.definitions()?)
    }

    pub fn limits(&self) -> crate::voxel_landscape::assets::budget::Limits {
        self.sources.limits()
    }

    pub fn immutable_definitions(&self) -> bool {
        self.sources.immutable_definition_sources()
    }
}

impl NativeSourceSession {
    pub fn open(
        jar: &Path,
        selected: Option<&VoxelLandscapeSettings>,
        fancy_leaves: bool,
        budget: ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self, Error> {
        Self::open_archive(
            native_assets::NativeArchive::Path(jar),
            selected,
            fancy_leaves,
            budget,
            cancel,
        )
    }

    pub fn open_pinned(
        jar: &ilium_platform::animation_files::PinnedFile,
        selected: Option<&VoxelLandscapeSettings>,
        fancy_leaves: bool,
        budget: ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self, Error> {
        Self::open_archive(
            native_assets::NativeArchive::Pinned(jar),
            selected,
            fancy_leaves,
            budget,
            cancel,
        )
    }

    fn open_archive(
        jar: native_assets::NativeArchive<'_>,
        selected: Option<&VoxelLandscapeSettings>,
        fancy_leaves: bool,
        budget: ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self, Error> {
        cancel.check()?;
        let ids_charge = budget.reserve(8 << 20, cancel)?;
        let sources =
            NativeSources::open_archive(jar, selected, Default::default(), budget.clone(), cancel)?;
        Ok(Self {
            sources,
            layers: RenderLayers::load(fancy_leaves)?,
            budget,
            _ids_charge: ids_charge,
        })
    }

    /// The caller keeps this borrowed source and one model compiler alive for
    /// the complete selected-ID pass, then drops both before `import(self)`.
    pub fn definitions(&self) -> Result<DefinitionSources<'_>, Error> {
        Ok(self.sources.definitions()?)
    }

    pub fn limits(&self) -> crate::voxel_landscape::assets::budget::Limits {
        self.sources.limits()
    }

    pub fn immutable_definitions(&self) -> bool {
        self.sources.immutable_definition_sources()
    }

    pub fn collect_required(
        &self,
        binding: &SavedBinding,
        world_seed: i64,
        ids: &mut BTreeSet<ResourceId>,
        compiler: &mut ModelCompiler<'_, DefinitionSources<'_>>,
        cancel: Cancel<'_>,
    ) -> Result<(), Error> {
        if binding
            .state_copy_charge
            .as_ref()
            .is_some_and(|charge| !charge.belongs_to(&self.budget))
        {
            return Err(Error::Source);
        }
        for (&position, block) in &binding.world.blocks {
            cancel.check()?;
            let java_position = [position[0], position[2], position[1]];
            let raw = binding.map.state(java_position).ok_or(Error::Source)?;
            if binding
                .liquid_cells
                .get(&java_position)
                .is_some_and(|cell| !cell.waterlogged)
            {
                continue;
            }
            if let Some(recipe) = native_builtin::recipe(raw)? {
                ids.insert(recipe.atlas);
                continue;
            }
            if let Some(body) = native_builtin::bell_body(raw)? {
                ids.insert(body.atlas);
            }
            let state =
                compiler.compile_state(&block.state, position, world_seed as u64, cancel)?;
            for (_, model) in &state.applications {
                for quad in &model.quads {
                    ids.insert(quad.texture.clone());
                }
            }
            if ids.len() > 8192 {
                return Err(Error::Limit("selected texture identities"));
            }
        }
        for kind in [super::fluids::Kind::Water, super::fluids::Kind::Lava] {
            if binding.liquid_cells.values().any(|cell| cell.kind == kind) {
                let sprites = native_fluid::native_sprites(kind)?;
                ids.insert(sprites.still);
                ids.insert(sprites.flowing);
                if let Some(overlay) = sprites.overlay {
                    ids.insert(overlay);
                }
            }
        }
        if ids.len() > 8192 {
            return Err(Error::Limit("selected texture identities"));
        }
        Ok(())
    }

    pub fn import(
        self,
        ids: BTreeSet<ResourceId>,
        cancel: Cancel<'_>,
    ) -> Result<SharedNative, Error> {
        self.import_with_progress(ids, cancel, &mut |_, _, _, _| {})
    }

    pub fn import_with_progress(
        self,
        ids: BTreeSet<ResourceId>,
        cancel: Cancel<'_>,
        progress: &mut dyn FnMut(usize, usize, &ResourceId, bool),
    ) -> Result<SharedNative, Error> {
        cancel.check()?;
        if ids.is_empty() || ids.len() > 8192 {
            return Err(Error::Limit("selected texture identities"));
        }
        let ids = ids.into_iter().collect::<Vec<_>>();
        let requests = self.sources.texture_requests(&ids, cancel)?;
        let imports = TextureImporter::new(
            self.sources.packs(),
            self.sources.limits(),
            self.budget.clone(),
        )?
        .import_with_progress(requests.as_slice(), cancel, progress)?;
        if imports.bank.coverage().required_satisfied != ids.len()
            || ids.iter().any(|id| imports.bank.resolve(id).is_none())
            || imports.records.iter().any(|record| record.source.is_none())
        {
            return Err(Error::Limit("required native model textures missing"));
        }
        Ok(SharedNative {
            sources: self.sources,
            layers: self.layers,
            imports: Arc::new(imports),
            budget: self.budget,
            model_reuse: Mutex::new(ModelReuse::default()),
        })
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "source, account, and cancellation have distinct custody"
)]
pub fn prepare_native(
    binding: SavedBinding,
    jar: &Path,
    selected: Option<&VoxelLandscapeSettings>,
    world_seed: i64,
    blend_radius: u8,
    fancy_leaves: bool,
    budget: ByteBudget,
    cancel: Cancel<'_>,
) -> Result<PreparedNative, Error> {
    let session = NativeSourceSession::open(jar, selected, fancy_leaves, budget, cancel)?;
    let definitions = session.definitions()?;
    let mut compiler = ModelCompiler::new(&definitions, session.limits(), session.budget.clone())?;
    if session.immutable_definitions() {
        compiler.enable_immutable_source_cache();
    }
    let mut ids = BTreeSet::new();
    session.collect_required(&binding, world_seed, &mut ids, &mut compiler, cancel)?;
    drop(compiler);
    let shared = session.import(ids, cancel)?;
    let definitions = shared.definitions()?;
    let mut compiler = ModelCompiler::new(&definitions, shared.limits(), shared.budget.clone())?;
    if shared.immutable_definitions() {
        compiler.enable_immutable_source_cache();
    }
    prepare_native_shared(
        binding,
        &shared,
        &mut compiler,
        world_seed,
        blend_radius,
        cancel,
    )
}

/// Bind one sparse tile against a bank imported once for the entire projected
/// source. The caller owns `SharedNative` until every tile has been bound; each
/// returned PreparedSurface owns an Arc of that same immutable bank.
pub fn prepare_native_shared(
    mut binding: SavedBinding,
    shared: &SharedNative,
    compiler: &mut ModelCompiler<'_, DefinitionSources<'_>>,
    world_seed: i64,
    blend_radius: u8,
    cancel: Cancel<'_>,
) -> Result<PreparedNative, Error> {
    cancel.check()?;
    if let Ok(mut reuse) = shared.model_reuse.lock() {
        reuse.prune();
    }
    if binding.map.source().generation == 0 || blend_radius > 7 {
        return Err(Error::Source);
    }
    // Signed Java seed bits are retained for native blockstate variant choice.
    binding.world.seed = world_seed as u64;
    let budget = shared.budget.clone();
    let state_copy = match binding.state_copy_charge.take() {
        Some(charge) if charge.belongs_to(&budget) => charge,
        Some(_) => return Err(Error::Source),
        None => budget.reserve(binding.storage_charge as u64, cancel)?,
    };
    let sources = &shared.sources;
    let layers = &shared.layers;
    let imports = Arc::clone(&shared.imports);
    // The per-position normalized/instance maps and distinct cache node
    // overhead exist concurrently with the bound models. Charge them before
    // constructing either tree; the charge drops after the tile is sealed.
    let scratch_bytes = binding
        .world
        .blocks
        .len()
        .checked_mul(256)
        .and_then(|bytes| bytes.checked_add(16 << 20))
        .ok_or(Error::Limit("native binding scratch"))?;
    let _scratch = budget.reserve(scratch_bytes as u64, cancel)?;
    let provenance = sources.provenance().clone();
    let mut normalized = BTreeMap::<[i32; 3], Arc<NormalizedState>>::new();
    let mut normalized_cache = BTreeMap::<ModelStateKey, Arc<NormalizedState>>::new();
    let mut builtin_recipes = BTreeMap::<[i32; 3], (Recipe, Reservation)>::new();
    let mut bell_bodies = BTreeMap::<[i32; 3], (BellBody, Reservation)>::new();
    let mut textures = BTreeSet::<ResourceId>::new();
    for (&position, block) in &binding.world.blocks {
        cancel.check()?;
        let java_position = [position[0], position[2], position[1]];
        let raw = binding.map.state(java_position).ok_or(Error::Source)?;
        if binding
            .liquid_cells
            .get(&java_position)
            .is_some_and(|cell| !cell.waterlogged)
        {
            // Pure water/lava/bubble_column has no JSON model quads. Its raw
            // state and owner remain in binding.world; fluid faces are added
            // from the separate exact liquid map after bank import.
            continue;
        }
        if let Some(recipe) = native_builtin::recipe(raw)? {
            let charge = budget.reserve(4096 + std::mem::size_of::<Recipe>() as u64, cancel)?;
            textures.insert(recipe.atlas.clone());
            builtin_recipes.insert(position, (recipe, charge));
            continue;
        }
        if let Some(body) = native_builtin::bell_body(raw)? {
            let charge = budget.reserve(4096 + std::mem::size_of::<BellBody>() as u64, cancel)?;
            textures.insert(body.atlas.clone());
            bell_bodies.insert(position, (body, charge));
        }
        let state = compiler.compile_state(&block.state, position, binding.world.seed, cancel)?;
        if !state.uses_budget(&budget)
            || state
                .applications
                .iter()
                .any(|(_, model)| !model.uses_budget(&budget))
        {
            return Err(Error::Source);
        }
        for (_, model) in &state.applications {
            for quad in &model.quads {
                textures.insert(quad.texture.clone());
            }
        }
        let key = normalized_key(&state)?;
        let state = if let Some(existing) = normalized_cache.get(&key) {
            Arc::clone(existing)
        } else {
            if normalized_cache.len() == MAX_NORMALIZED_STATES {
                return Err(Error::Limit("distinct normalized states"));
            }
            let state = Arc::new(state);
            normalized_cache.insert(key, Arc::clone(&state));
            state
        };
        normalized.insert(position, state);
    }
    for kind in [super::fluids::Kind::Water, super::fluids::Kind::Lava] {
        if binding.liquid_cells.values().any(|cell| cell.kind == kind) {
            let sprites = native_fluid::native_sprites(kind)?;
            textures.insert(sprites.still);
            textures.insert(sprites.flowing);
            if let Some(overlay) = sprites.overlay {
                textures.insert(overlay);
            }
        }
    }
    let ids = textures.into_iter().collect::<Vec<_>>();
    if ids.iter().any(|id| imports.bank.resolve(id).is_none()) {
        return Err(Error::Limit("required native model textures missing"));
    }
    let mut tint = None::<NativeTint>;
    let mut instances = BTreeMap::<[i32; 3], Arc<BoundModel>>::new();
    let mut bound_cache = BTreeMap::<(ModelStateKey, Vec<(u16, [u8; 3])>), Arc<BoundModel>>::new();
    for (&position, state) in &normalized {
        cancel.check()?;
        let java_position = [position[0], position[2], position[1]];
        let raw = binding.map.state(java_position).ok_or(Error::Source)?;
        if raw.name != state.state.id().as_str()
            || raw.properties.len() != state.state.properties().len()
            || raw
                .properties
                .iter()
                .any(|(name, value)| state.state.property(name) != Some(value.as_str()))
        {
            return Err(Error::Source);
        }
        if state
            .applications
            .iter()
            .all(|(_, model)| model.quads.is_empty())
            && !bell_bodies.contains_key(&position)
        {
            return Err(Error::EmptyModel {
                state: raw.name.clone(),
                java_position,
            });
        }
        let mut indices = BTreeSet::<u16>::new();
        let mut rules = BTreeMap::<ResourceId, TextureRenderRule>::new();
        for (_, model) in &state.applications {
            for quad in &model.quads {
                if let Some(index) = quad.tint_index {
                    indices.insert(index);
                }
                let id = state.state.id().as_str();
                let layer = layers.block(id)?;
                let (alpha, order) = match layer {
                    Layer::Solid => (AlphaMode::NativeSolid, 0),
                    Layer::CutoutMipped => (AlphaMode::NativeCutoutMipped, 1),
                    Layer::Cutout => (AlphaMode::NativeCutout, 2),
                    Layer::Translucent => (AlphaMode::NativeBlend, 3),
                };
                rules.insert(
                    quad.texture.clone(),
                    TextureRenderRule {
                        alpha,
                        layer: order,
                        normal_map: None,
                        specular_map: None,
                    },
                );
            }
        }
        let mut tints = BTreeMap::new();
        let mut tint_key = Vec::new();
        for index in indices {
            cancel.check()?;
            let color = match native_block_colors::dispatch(raw, java_position, index)? {
                BlockColor::NoHandler => [255; 3], // Native render overload returns -1.
                BlockColor::Fixed(color) => color,
                BlockColor::Biome {
                    kind,
                    java_position,
                } => {
                    if tint.is_none() {
                        tint = Some(NativeTint::load(sources, cancel)?);
                    }
                    tint.as_ref()
                        .ok_or(Error::Source)?
                        .sample_native(
                            &binding.map,
                            NativeRequest {
                                source: binding.map.source(),
                                java_position,
                                kind,
                                climate_policy: ClimatePolicy::RenderEarlierWith1193,
                                world_seed: Some(world_seed),
                                blend_radius,
                            },
                            cancel,
                        )?
                        .rgb
                }
            };
            tints.insert(index, color.map(|channel| f32::from(channel) / 255.0));
            tint_key.push((index, color));
        }
        let key = (normalized_key(state)?, tint_key);
        if let Some(existing) = bound_cache.get(&key) {
            instances.insert(position, Arc::clone(existing));
            continue;
        }
        if bound_cache.len() == MAX_BOUND_MODELS {
            return Err(Error::Limit("distinct bound models"));
        }
        let reused = shared
            .model_reuse
            .lock()
            .ok()
            .and_then(|reuse| reuse.get(&key));
        if let Some(model) = reused {
            // A route-wide hit still enters this tile's local admission map.
            bound_cache.insert(key, Arc::clone(&model));
            instances.insert(position, model);
            continue;
        }
        let policy = MaterialTable {
            medium: None,
            rules,
            tints,
        };
        let model = Arc::new(BoundModel::bind(
            state,
            &imports.bank,
            &policy,
            &budget,
            cancel,
        )?);
        if let Ok(mut reuse) = shared.model_reuse.lock() {
            reuse.remember(&key, &model, &budget, cancel)?;
        }
        bound_cache.insert(key, Arc::clone(&model));
        instances.insert(position, model);
    }
    // BedRenderer and ChestRenderer use model-part cubes and stitched entity
    // textures, not JSON blockstate quads. The complete atlas is imported into
    // this bank; all six faces of each of the three parts receive stable
    // part/face owner IDs through PreparedMesh::build.
    for (&position, (recipe, _charge)) in &builtin_recipes {
        cancel.check()?;
        let handle = imports.bank.resolve(&recipe.atlas).ok_or(Error::Source)?;
        // Model-part U/V divide by their logical atlas axes independently;
        // selected HD, rectangular and authored animated frames stay valid.
        imports.bank.texture(handle).ok_or(Error::Source)?;
        let block = binding.world.blocks.get(&position).ok_or(Error::Source)?;
        let (alpha, layer) = match recipe.kind {
            // Sheets BED_SHEET_TYPE = RenderType.entitySolid.
            BuiltinKind::Bed(_) => (AlphaMode::NativeSolid, 0),
            // Sheets CHEST_SHEET_TYPE = RenderType.entityCutout. Captured
            // entity_cutout.fsh discards texture alpha below 0.1.
            BuiltinKind::Chest(_) => (AlphaMode::NativeCutout, 2),
        };
        let mut quads = Vec::with_capacity(18);
        for face in native_builtin::bake_faces(recipe) {
            quads.push(BoundQuad {
                points: face.points,
                uv: face.uv,
                normal: face.normal,
                material: FaceMaterial {
                    texture: handle,
                    alpha,
                    tint: [1.0; 3],
                    layer,
                    normal_map: None,
                    specular_map: None,
                },
                cull_face: None,
                shade: true,
                part: face.part,
                face: face.face,
            });
        }
        let model = BoundModel::from_source_native_builtin(
            block.state.clone(),
            quads,
            &imports.bank,
            &budget,
            cancel,
        )?;
        instances.insert(position, Arc::new(model));
    }
    // BellRenderer supplies a moving entity-model body *in addition* to the
    // stand/support quads from its ordinary nonempty JSON block model.
    // Native ffq calls RenderType.entitySolid through fed.b; this snapshot
    // uses the exact zero-ringing pose, without fabricated live tick state.
    for (&position, (body, _charge)) in &bell_bodies {
        cancel.check()?;
        let stand = instances.get(&position).ok_or(Error::Source)?;
        let handle = imports.bank.resolve(&body.atlas).ok_or(Error::Source)?;
        imports.bank.texture(handle).ok_or(Error::Source)?;
        let mut extra = Vec::with_capacity(12);
        for face in native_builtin::bake_bell_faces(body) {
            extra.push(BoundQuad {
                points: face.points,
                uv: face.uv,
                normal: face.normal,
                material: FaceMaterial {
                    texture: handle,
                    alpha: AlphaMode::NativeSolid,
                    tint: [1.0; 3],
                    layer: 0,
                    normal_map: None,
                    specular_map: None,
                },
                cull_face: None,
                shade: true,
                part: u16::MAX,
                face: face.part * 6 + face.face,
            });
        }
        let combined = stand.with_source_native_builtin(extra, &imports.bank, &budget, cancel)?;
        instances.insert(position, Arc::new(combined));
    }
    let region = MeshRegion {
        minimum: binding.world.region.minimum,
        maximum: binding.world.region.maximum,
    };
    let mesh = PreparedMesh::build(
        &instances,
        region,
        imports.bank.identity(),
        1_000_000,
        &budget,
        cancel,
    )?;
    if binding
        .liquid_cells
        .values()
        .any(|cell| cell.kind == super::fluids::Kind::Water)
        && tint.is_none()
    {
        tint = Some(NativeTint::load(sources, cancel)?);
    }
    let fluid = native_fluid_assembly::build(
        &binding,
        &imports.bank,
        tint.as_ref(),
        world_seed,
        blend_radius,
        &budget,
        cancel,
    )?;
    let entities = surface_entity_binding::bind(&binding.world, &imports, &budget, cancel)?;
    if !entities.faces.is_empty() || entities.rendered_entities != 0 {
        return Err(Error::Source);
    }
    let mut world_hash = Sha256::new();
    world_hash.update(b"native-saved-1193-render-layer");
    world_hash.update(world_seed.to_le_bytes());
    world_hash.update(binding.map.source().map.0);
    world_hash.update(binding.map.source().generation.to_le_bytes());
    world_hash.update([blend_radius]);
    for (position, block) in &binding.world.blocks {
        for coordinate in position {
            world_hash.update(coordinate.to_le_bytes());
        }
        world_hash.update(block.state.fingerprint().bytes());
    }
    let world_epoch = Digest256::try_from(format!("{:x}", world_hash.finalize()))?;
    let mut model_hash = Sha256::new();
    model_hash.update(imports.bank.identity().bytes());
    model_hash.update(provenance.native_archive_sha256.bytes());
    model_hash.update(world_epoch.bytes());
    for (position, model) in &instances {
        for coordinate in position {
            model_hash.update(coordinate.to_le_bytes());
        }
        model_hash.update(model.state.fingerprint().bytes());
        for origin in &model.origins {
            model_hash.update(origin.sha256.bytes());
        }
        for quad in &model.quads {
            for channel in quad.material.tint {
                model_hash.update(channel.to_le_bytes());
            }
        }
    }
    if let Some(fluid) = &fluid {
        for face in &fluid.faces {
            for coordinate in face.position {
                model_hash.update(coordinate.to_le_bytes());
            }
            model_hash.update(face.owner.face.to_le_bytes());
            for point in face.quad.points {
                for coordinate in point {
                    model_hash.update(coordinate.to_le_bytes());
                }
            }
            for uv in face.quad.uv {
                for coordinate in uv {
                    model_hash.update(coordinate.to_le_bytes());
                }
            }
            for channel in face.quad.material.tint {
                model_hash.update(channel.to_le_bytes());
            }
        }
    }
    let model_epoch = Digest256::try_from(format!("{:x}", model_hash.finalize()))?;
    let material_fallbacks = imports
        .records
        .iter()
        .filter_map(|record| {
            let source = record.source.as_ref()?;
            (source.kind == OriginKind::ExplicitFullPackFallback)
                .then(|| format!("{}: {} {}", record.resource, source.pack, source.path))
        })
        .collect();
    let bank_epoch = imports.bank.identity();
    let surface = PreparedSurface {
        world: binding.world,
        imports,
        mesh,
        fluid,
        entities,
        bank_epoch,
        world_epoch,
        model_epoch,
        skipped_states: Vec::new(),
        compatibility_aliases: Vec::new(),
        material_fallbacks,
        model_substitutions: Vec::new(),
        source_sha256: provenance.selected_archive_sha256,
        budget,
    };
    Ok(PreparedNative {
        surface,
        map: binding.map,
        source_profile: provenance.profile,
        native_archive_sha256: provenance.native_archive_sha256,
        selected_archive_sha256: provenance.selected_archive_sha256,
        _state_copy: state_copy,
    })
}

#[cfg(test)]
#[path = "native_binding_tests.rs"]
mod tests;
