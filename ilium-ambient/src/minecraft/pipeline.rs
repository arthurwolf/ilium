//! Blocking catalog preparation for a scene-owned worker. Identity bindings and
//! recent windows belong to the caller; no path-derived identity is invented.
use super::{catalog, evidence, index, loader, preparation, region, windows};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Debug)]
pub struct MapContext {
    pub map: evidence::MapId,
    /// Chunk centers from this map, using the selector's current radius.
    pub recent: Vec<[i32; 2]>,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub maps: usize,
    pub map_attempts: usize,
    pub windows: windows::Limits,
    pub loading: loader::Limits,
    pub preparation: preparation::Limits,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maps: 4,
            map_attempts: 8,
            windows: windows::Limits::default(),
            loading: loader::Limits::default(),
            preparation: preparation::Limits::default(),
        }
    }
}

#[derive(Debug)]
pub struct PreparedSave {
    pub directory: PathBuf,
    pub last_played: i64,
    /// Retained for seeded saved-biome rendering; absence is never seed zero.
    pub world_seed: Option<i64>,
    pub window: preparation::PreparedWindow,
}

#[derive(Debug)]
pub struct MapReport {
    pub directory: PathBuf,
    pub allocated_chunks: usize,
    pub rejected_regions: usize,
    pub header_candidates: usize,
    pub scan_complete: bool,
    pub rejected_windows: usize,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct PreparedCatalog {
    pub maps: Vec<PreparedSave>,
    pub reports: Vec<MapReport>,
    pub unbound_maps: usize,
    /// False when the finite map-attempt or successful-map cap stopped the
    /// walk. Neither true nor false claims exhaustive terrain exploration.
    pub catalog_complete: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid saved catalog preparation limits")]
    Limits,
    #[error("saved catalog requires unique bound identities and nonzero generation")]
    Context,
    #[error("saved catalog preparation cancelled")]
    Cancelled,
}

/// Call off the rendering thread, after discover_metadata and the caller's
/// persistent identity registry have supplied bindings. Most recently played
/// maps are attempted first, including old timestamps: no absolute age cutoff.
/// At most four outputs retain loader storage, each admitted at <=32 MiB
/// logical charge plus <=2 MiB summary policy. This excludes transient decode,
/// model storage and snapshots retained elsewhere; it is not an RSS limit.
pub fn prepare_catalog(
    catalog: &catalog::Catalog,
    bindings: &BTreeMap<PathBuf, MapContext>,
    generation: u64,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<PreparedCatalog, Error> {
    prepare_catalog_with(
        catalog,
        bindings,
        generation,
        limits,
        cancelled,
        |save, context| {
            let mut report = MapReport {
                directory: save.directory.clone(),
                allocated_chunks: 0,
                rejected_regions: 0,
                header_candidates: 0,
                scan_complete: false,
                rejected_windows: 0,
                error: None,
            };
            let result = (|| {
                let region_directory = save.directory.join("region");
                let allocation =
                    index::allocated_chunks(&region_directory, cancelled).map_err(|error| {
                        (matches!(error, region::Error::Cancelled), error.to_string())
                    })?;
                report.allocated_chunks = allocation.chunks.len();
                report.rejected_regions = allocation.rejected_regions;
                let anchor = save
                    .metadata
                    .spawn_position
                    .map(|[x, _, z]| [x.div_euclid(16), z.div_euclid(16)])
                    .unwrap_or([0, 0]);
                let search = windows::search(
                    &allocation.chunks,
                    anchor,
                    &context.recent,
                    limits.windows,
                    cancelled,
                )
                .map_err(|error| (error == windows::Error::Cancelled, error.to_string()))?;
                report.header_candidates = search.header_complete;
                report.scan_complete = search.scan_complete;
                let output = preparation::load_candidates(
                    &region_directory,
                    evidence::Source {
                        map: context.map,
                        generation,
                    },
                    &search.candidates,
                    limits.loading,
                    limits.preparation,
                    cancelled,
                )
                .map_err(|error| {
                    (
                        matches!(error, preparation::Error::Loader(loader::Error::Cancelled)),
                        error.to_string(),
                    )
                })?;
                report.rejected_windows = output.rejected.len();
                Ok(output.window)
            })();
            match result {
                Ok(window) => Ok((window, report)),
                Err((true, _)) => Err(Error::Cancelled),
                Err((false, message)) => {
                    report.error = Some(message.chars().take(256).collect());
                    Ok((None, report))
                }
            }
        },
    )
}

fn prepare_catalog_with(
    catalog: &catalog::Catalog,
    bindings: &BTreeMap<PathBuf, MapContext>,
    generation: u64,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
    mut prepare: impl FnMut(
        &catalog::Save,
        &MapContext,
    ) -> Result<(Option<preparation::PreparedWindow>, MapReport), Error>,
) -> Result<PreparedCatalog, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    if limits.maps == 0
        || limits.maps > 4
        || limits.map_attempts == 0
        || limits.map_attempts > 8
        || limits.maps > limits.map_attempts
        || limits.loading.max_storage_charge == 0
        || limits.loading.max_storage_charge > 32 << 20
        || limits.preparation.summary_bytes > 2 << 20
        || limits.windows.candidates == 0
        || limits.windows.candidates > 16
    {
        return Err(Error::Limits);
    }
    if generation == 0 || catalog.maps.len() > 512 || bindings.len() > 512 {
        return Err(Error::Context);
    }
    let mut identities = BTreeSet::new();
    for context in bindings.values() {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if context.recent.len() > 64 || !identities.insert(context.map) {
            return Err(Error::Context);
        }
    }
    let mut paths = BTreeSet::new();
    for save in &catalog.maps {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if save.metadata.last_played < 0 || !paths.insert(&save.directory) {
            return Err(Error::Context);
        }
    }
    let mut order: Vec<_> = catalog.maps.iter().collect();
    order.sort_by(|left, right| {
        right
            .metadata
            .last_played
            .cmp(&left.metadata.last_played)
            .then_with(|| left.directory.cmp(&right.directory))
    });
    let mut result = PreparedCatalog::default();
    for save in order {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if result.maps.len() == limits.maps || result.reports.len() == limits.map_attempts {
            return Ok(result);
        }
        let Some(context) = bindings.get(&save.directory) else {
            result.unbound_maps += 1;
            continue;
        };
        let (window, report) = prepare(save, context)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if let Some(window) = window {
            if window.source
                != (evidence::Source {
                    map: context.map,
                    generation,
                })
            {
                return Err(Error::Context);
            }
            result.maps.push(PreparedSave {
                directory: save.directory.clone(),
                last_played: save.metadata.last_played,
                world_seed: save.metadata.world_seed,
                window,
            });
        }
        result.reports.push(report);
    }
    result.catalog_complete = true;
    Ok(result)
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
