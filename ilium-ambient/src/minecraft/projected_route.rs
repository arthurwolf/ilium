//! Owned-worker preparation of one complete projected saved-world route.
//! Every possible viewport source chunk is decoded before any tile can be
//! published. Tile-local model/face halos are borrowed from that full source;
//! only tile cores emit. All tiles share one selected-pack texture bank.
use super::{
    history_store::BoundMap,
    loader,
    native_binding::{self, NativeSourceSession, NativeTile},
    projected_source, saved_binding, source_footprint, sparse_cells,
    tours::{self, Plan, PreparedMap, ProjectedDisplay},
};
use crate::voxel_landscape::{
    assets::{
        budget::{ByteBudget, Cancel, Reservation},
        error::AssetError,
        identity::{Digest256, ResourceId},
        models::ModelCompiler,
    },
    VoxelLandscapeSettings,
};
use std::{collections::BTreeSet, path::Path, sync::Arc};

const DISPLAY_WORK_BASE: u64 = 1_000_000;
const DISPLAY_WORK_PER_CHUNK: u64 = 100_000;

/// The display binding compares every retained planner chunk with its
/// projected counterpart.  Keep that finite work admission proportional to
/// the bounded retained map instead of rejecting otherwise valid large saves
/// once the old fixed sixteen-million ceiling is crossed.
fn display_work_limit(chunk_count: usize) -> u64 {
    DISPLAY_WORK_BASE.saturating_add((chunk_count as u64).saturating_mul(DISPLAY_WORK_PER_CHUNK))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("projected route/map/source identity mismatch")]
    Source,
    #[error("projected route contains no emitted saved geometry")]
    Empty,
    #[error("projected tile model or fluid vertex exceeded certified local bounds")]
    Geometry,
    #[error("projected tile worker panicked; route preparation stopped")]
    Worker,
    #[error("projected route exceeded tile/bank account")]
    Limit,
    #[error(transparent)]
    Footprint(#[from] source_footprint::Error),
    #[error(transparent)]
    Qualified(#[from] projected_source::Error),
    #[error(transparent)]
    Tile(#[from] sparse_cells::Error),
    #[error(transparent)]
    Binding(#[from] saved_binding::Error),
    #[error(transparent)]
    Native(#[from] native_binding::Error),
    #[error(transparent)]
    Tour(#[from] tours::Error),
    #[error(transparent)]
    Asset(#[from] AssetError),
}

pub struct Inputs<'a> {
    pub plan: &'a Plan,
    pub initial_map: &'a PreparedMap,
    pub bound: &'a BoundMap,
    pub saves_root: &'a Path,
    pub jar: &'a Path,
    pub selected: Option<&'a VoxelLandscapeSettings>,
    pub world_seed: i64,
    pub blend_radius: u8,
    pub fancy_leaves: bool,
    pub viewport: [usize; 2],
    pub scale: f64,
    pub account: &'a ByteBudget,
    /// Persistent cache for the viewport model-requirement scan; `None` disables it.
    pub cache: Option<&'a crate::prepared_cache::PreparedCache>,
    pub cancel: Cancel<'a>,
    // Sync: rendered tiles are built on scoped worker threads that share these.
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
    /// Reports concrete stages and bounded chunk counts to the preparation UI.
    pub progress: &'a (dyn Fn(&str) + Sync),
    /// Reports each completed projected source-chunk decode to the preparation UI.
    pub decode_progress: &'a (dyn Fn(usize, usize) + Sync),
    /// Reports bounded model, texture, and rendered-tile preparation work.
    pub work_progress: &'a (dyn Fn(&str, usize, usize, &str) + Sync),
}

pub struct PreparedRoute {
    pub map: Arc<PreparedMap>,
    pub tiles: Vec<NativeTile>,
    pub bank_epoch: Digest256,
    pub request: Arc<source_footprint::Request>,
    pub display: Arc<ProjectedDisplay>,
    pub viewport: [usize; 2],
    pub scale: f64,
    pub camera_height: f64,
    _tiles_charge: Reservation,
}

fn certified_points(points: &[[f64; 3]; 4]) -> bool {
    points.iter().flatten().all(|value| {
        value.is_finite()
            && (source_footprint::LOCAL_MIN..=source_footprint::LOCAL_MAX).contains(value)
    })
}

/// Upper bound on concurrent tile workers. Each worker keeps its own model
/// cache, all charged to the one scene account, so the count stays small.
const MAX_TILE_WORKERS: usize = 8;

/// Worker count for building `chunk_count` tiles: available parallelism,
/// capped, and never more workers than tiles.
fn tile_worker_count(chunk_count: usize) -> usize {
    std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(MAX_TILE_WORKERS)
        .min(chunk_count.max(1))
}

fn certify_tile(tile: &NativeTile) -> Result<(), Error> {
    if tile.mesh.bank != tile.bank_epoch || tile.bank().identity() != tile.bank_epoch {
        return Err(Error::Source);
    }
    for face in &tile.mesh.faces {
        let quad = face
            .model
            .quads
            .get(usize::from(face.quad_index))
            .ok_or(Error::Geometry)?;
        if !certified_points(&quad.points) {
            return Err(Error::Geometry);
        }
    }
    if let Some(fluid) = &tile.fluid {
        if fluid.bank != tile.bank_epoch
            || fluid
                .faces
                .iter()
                .any(|face| !certified_points(&face.quad.points))
        {
            return Err(Error::Geometry);
        }
    }
    Ok(())
}

/// Two bounded passes avoid retaining cloned raw tile states while the bank is
/// built. Empty exact-air tiles are still covered by the qualified source and
/// require no mesh. A failed/missing chunk or texture rejects the WHOLE route.
pub fn prepare(input: Inputs<'_>) -> Result<PreparedRoute, Error> {
    prepare_source(input, None)
}

/// Render preparation keeps selected world reads on the original descriptors.
/// The native jar remains the separately authorized asset in Inputs.
pub fn prepare_pinned(
    input: Inputs<'_>,
    source: projected_source::PinnedSource<'_>,
    native_jar: &ilium_platform::animation_files::PinnedFile,
) -> Result<PreparedRoute, Error> {
    if input.saves_root != source.root_label {
        return Err(Error::Source);
    }
    prepare_source(input, Some((source, native_jar)))
}

const REQUIREMENT_CACHE_KIND: &str = "route-requirements";

/// Everything that decides which model/texture IDs the viewport scan finds:
/// the world's region files, the viewport chunks, the seed, the native jar and
/// the selected pack.
fn requirement_key(input: &Inputs<'_>, render_chunks: &[[i32; 2]]) -> crate::prepared_cache::Key {
    let mut builder = crate::prepared_cache::KeyBuilder::new(REQUIREMENT_CACHE_KIND);
    builder
        .path_fingerprint(&input.bound.directory.join("region"))
        .path_fingerprint(&input.bound.directory.join("level.dat"))
        .path_fingerprint(input.jar)
        .number(input.world_seed as u64)
        .number(u64::from(input.fancy_leaves))
        .number(render_chunks.len() as u64);
    for chunk in render_chunks {
        builder.part(&chunk[0].to_le_bytes()).part(&chunk[1].to_le_bytes());
    }
    if let Some(selected) = input.selected {
        builder
            .text(&format!("{selected:?}"))
            .path_fingerprint(Path::new(&selected.pack_path))
            .path_fingerprint(Path::new(&selected.pack_addon_path));
    }
    builder.finish()
}

fn prepare_source(
    input: Inputs<'_>,
    source: Option<(
        projected_source::PinnedSource<'_>,
        &ilium_platform::animation_files::PinnedFile,
    )>,
) -> Result<PreparedRoute, Error> {
    input.cancel.check()?;
    (input.progress)("Checking projected viewport source coverage");
    if input.plan.source() != input.initial_map.source()
        || input.bound.map != input.plan.source().map
    {
        return Err(Error::Source);
    }
    let camera_height = input.plan.focus_y();
    let request = Arc::new(source_footprint::request(
        input.plan.line(),
        camera_height,
        input.viewport,
        input.scale,
        input.account,
        input.cancel,
    )?);
    let mut report_decode_progress =
        |update: loader::LoadProgress| (input.decode_progress)(update.completed, update.total);
    let qualified = match source {
        Some((source, _)) => projected_source::qualify_pinned_with_progress(
            input.initial_map,
            input.bound,
            source,
            &request,
            input.account,
            input.cancel,
            input.cancelled,
            &mut report_decode_progress,
        )?,
        None => projected_source::qualify_with_progress(
            input.initial_map,
            input.bound,
            input.saves_root,
            &request,
            input.account,
            input.cancel,
            input.cancelled,
            &mut report_decode_progress,
        )?,
    };
    let map = qualified.map;
    let mut source_budget = tours::Budget::new(
        display_work_limit(input.initial_map.loaded().chunks.len()),
        input.cancelled,
    );
    let display = Arc::new(ProjectedDisplay::bind(
        input.plan,
        Arc::clone(&map),
        Arc::clone(&request),
        input.viewport,
        input.scale,
        &mut source_budget,
    )?);
    (input.progress)("Viewport coverage is complete; binding the camera display");
    let session = match source {
        Some((_, native_jar)) => NativeSourceSession::open_pinned(
            native_jar,
            input.selected,
            input.fancy_leaves,
            input.account.clone(),
            input.cancel,
        )?,
        None => NativeSourceSession::open(
            input.jar,
            input.selected,
            input.fancy_leaves,
            input.account.clone(),
            input.cancel,
        )?,
    };
    let definitions = session.definitions()?;
    let immutable_definitions = session.immutable_definitions();
    // Ordered Vec so tile batches can slice it; the BTreeSet order is the chunk order.
    let render_chunks: Vec<[i32; 2]> = request.render_chunks().iter().copied().collect();
    let requirement_cache_key = input.cache.map(|_| requirement_key(&input, &render_chunks));
    let cached_ids: Option<BTreeSet<ResourceId>> = input
        .cache
        .zip(requirement_cache_key)
        .and_then(|(cache, key)| cache.load(REQUIREMENT_CACHE_KIND, &key));
    if cached_ids.is_some() {
        (input.progress)("Loaded viewport model requirements from the prepared-data cache");
    }
    let requirement_worker_count = if cached_ids.is_some() {
        0
    } else {
        tile_worker_count(render_chunks.len())
    };
    let mut requirement_compilers = Vec::with_capacity(requirement_worker_count);
    for _ in 0..requirement_worker_count {
        let mut compiler =
            ModelCompiler::new(&definitions, session.limits(), input.account.clone())?;
        if immutable_definitions {
            compiler.enable_immutable_source_cache();
        }
        requirement_compilers.push(compiler);
    }
    let scan_needed = cached_ids.is_none();
    let mut ids = cached_ids.unwrap_or_default();
    (input.work_progress)(
        "Collecting viewport model requirements",
        0,
        render_chunks.len(),
        "starting bounded viewport chunk scan",
    );
    // Batches hold one chunk per worker. Each worker has its own model cache and
    // requirement set; the sets are merged back in chunk order, so the union matches
    // a sequential scan (the requirement result does not depend on which cache served it).
    let mut next_index = 0;
    while scan_needed && next_index < render_chunks.len() {
        input.cancel.check()?;
        let batch_end = (next_index + requirement_compilers.len()).min(render_chunks.len());
        let batch = &render_chunks[next_index..batch_end];
        let outcomes = std::thread::scope(|scope| {
            let workers: Vec<_> = batch
                .iter()
                .zip(requirement_compilers.iter_mut())
                .map(|(&chunk, compiler)| {
                    let map = Arc::clone(&map);
                    let request = &request;
                    let session = &session;
                    let input = &input;
                    scope.spawn(move || -> Result<Option<BTreeSet<ResourceId>>, Error> {
                        if !immutable_definitions {
                            compiler.reset_for_next_tile();
                        }
                        let tile = match sparse_cells::prepare(
                            Arc::clone(&map),
                            request,
                            chunk,
                            input.account,
                            input.cancel,
                        ) {
                            Ok(tile) => tile,
                            Err(sparse_cells::Error::Empty) => return Ok(None),
                            Err(error) => return Err(error.into()),
                        };
                        let binding = saved_binding::prepare_accounted(
                            &tile.cells,
                            saved_binding::Limits::default(),
                            input.account,
                            input.cancel,
                            input.cancelled,
                        )?;
                        let mut chunk_ids = BTreeSet::<ResourceId>::new();
                        session.collect_required(
                            &binding,
                            input.world_seed,
                            &mut chunk_ids,
                            compiler,
                            input.cancel,
                        )?;
                        Ok(Some(chunk_ids))
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap_or(Err(Error::Worker)))
                .collect::<Vec<_>>()
        });
        for outcome in outcomes {
            if let Some(chunk_ids) = outcome? {
                ids.extend(chunk_ids);
            }
        }
        (input.work_progress)(
            "Collecting viewport model requirements",
            batch_end,
            render_chunks.len(),
            &format!("{} unique texture models", ids.len()),
        );
        next_index = batch_end;
    }
    drop(requirement_compilers);
    if let (true, Some(cache), Some(key)) = (scan_needed, input.cache, requirement_cache_key) {
        if let Err(error) = cache.store(REQUIREMENT_CACHE_KIND, &key, &ids) {
            tracing::warn!(%error, "could not save viewport model requirements to the cache");
        }
    }
    (input.progress)(&format!(
        "Loading selected texture-pack assets for {} required models",
        ids.len()
    ));
    let mut report_import_progress = |completed, total, resource: &ResourceId, loading| {
        (input.work_progress)(
            "Loading selected texture assets",
            completed,
            total,
            &format!(
                "{} {resource}",
                if loading { "currently" } else { "loaded" }
            ),
        );
    };
    let shared = session.import_with_progress(ids, input.cancel, &mut report_import_progress)?;
    let bank_epoch = shared.bank_epoch();
    let definitions = shared.definitions()?;
    if shared.immutable_definitions() != immutable_definitions {
        return Err(Error::Source);
    }
    // Each worker owns its compiler: the model caches are mutable, and a
    // tile's result does not depend on which worker's cache served it.
    let worker_count = tile_worker_count(render_chunks.len());
    let mut compilers = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let mut compiler =
            ModelCompiler::new(&definitions, shared.limits(), input.account.clone())?;
        if immutable_definitions {
            compiler.enable_immutable_source_cache();
        }
        compilers.push(compiler);
    }
    let tiles_charge = input.account.reserve(1 << 20, input.cancel)?;
    let mut tiles = Vec::<NativeTile>::new();
    tiles
        .try_reserve_exact(request.render_chunks().len())
        .map_err(|_| Error::Limit)?;
    if tiles
        .capacity()
        .saturating_mul(std::mem::size_of::<NativeTile>())
        > 1 << 20
    {
        return Err(Error::Limit);
    }
    (input.work_progress)(
        "Building projected viewport tiles",
        0,
        render_chunks.len(),
        "starting bounded tile raster preparation",
    );
    // Batches hold one chunk per worker and are merged back in chunk order, so
    // the route is identical to a sequential build.
    let mut next_index = 0;
    while next_index < render_chunks.len() {
        input.cancel.check()?;
        let batch_end = (next_index + compilers.len()).min(render_chunks.len());
        let batch = &render_chunks[next_index..batch_end];
        (input.progress)(&format!(
            "Building rendered viewport tiles {}-{}/{}, {} tiles ready",
            next_index + 1,
            batch_end,
            render_chunks.len(),
            tiles.len()
        ));
        let outcomes = std::thread::scope(|scope| {
            let workers: Vec<_> = batch
                .iter()
                .zip(compilers.iter_mut())
                .map(|(&chunk, compiler)| {
                    let map = Arc::clone(&map);
                    let request = &request;
                    let shared = &shared;
                    let input = &input;
                    scope.spawn(move || -> Result<Option<NativeTile>, Error> {
                        if !immutable_definitions {
                            compiler.reset_for_next_tile();
                        }
                        let tile = match sparse_cells::prepare(
                            Arc::clone(&map),
                            request,
                            chunk,
                            input.account,
                            input.cancel,
                        ) {
                            Ok(tile) => tile,
                            Err(sparse_cells::Error::Empty) => return Ok(None),
                            Err(error) => return Err(error.into()),
                        };
                        let binding = saved_binding::prepare_accounted(
                            &tile.cells,
                            saved_binding::Limits::default(),
                            input.account,
                            input.cancel,
                            input.cancelled,
                        )?;
                        let prepared = native_binding::prepare_native_shared(
                            binding,
                            shared,
                            compiler,
                            input.world_seed,
                            input.blend_radius,
                            input.cancel,
                        )?;
                        let native_tile = prepared.into_tile();
                        certify_tile(&native_tile)?;
                        Ok(Some(native_tile))
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap_or(Err(Error::Worker)))
                .collect::<Vec<_>>()
        });
        for outcome in outcomes {
            let Some(native_tile) = outcome? else {
                continue;
            };
            if native_tile.bank_epoch != bank_epoch || !Arc::ptr_eq(&native_tile.map, &map) {
                return Err(Error::Source);
            }
            if tiles
                .first()
                .is_some_and(|first| !Arc::ptr_eq(&first.imports, &native_tile.imports))
            {
                return Err(Error::Source);
            }
            tiles.push(native_tile);
        }
        (input.work_progress)(
            "Building projected viewport tiles",
            batch_end,
            render_chunks.len(),
            &format!("{} tiles ready", tiles.len()),
        );
        next_index = batch_end;
    }
    drop(compilers);
    if tiles.is_empty()
        || tiles.iter().all(|tile| {
            tile.mesh.faces.is_empty() && tile.fluid.as_ref().is_none_or(|m| m.faces.is_empty())
        })
    {
        return Err(Error::Empty);
    }
    Ok(PreparedRoute {
        map,
        tiles,
        bank_epoch,
        request,
        display,
        viewport: input.viewport,
        scale: input.scale,
        camera_height,
        _tiles_charge: tiles_charge,
    })
}

#[cfg(test)]
mod tests {
    use super::display_work_limit;

    #[test]
    fn display_work_admission_scales_with_retained_map_chunks() {
        assert_eq!(display_work_limit(0), 1_000_000);
        assert_eq!(display_work_limit(160), 17_000_000);
        assert_eq!(display_work_limit(384), 39_400_000);
        assert!(display_work_limit(384) > display_work_limit(160));
    }
}
