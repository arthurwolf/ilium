//! Worker-only saved-directory binding, metadata and preparation orchestration.
//!
//! No rendering, settings decisions, world writes or appearance credit occur
//! here. An admission transaction may already have committed complete bindings
//! when later preparation fails/cancels; reload before retrying its revision.
//! Repeated directory observations detect change, not an atomic filesystem
//! snapshot. Save data should be quiescent; generation reuse and path/open
//! races remain the platform and read-only adapters' documented limitations.
use super::{
    catalog,
    evidence::MapId,
    history_store::{self, BoundMap, Repository, Snapshot},
    pipeline, region,
    tours::{self, History, PreparedMap},
};
use ilium_platform::{
    paths,
    secure_fs::{self, NoFollowDirectory},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_DIRECTORY_ENTRIES: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    MissingRoot,
    EmptyRoot,
    NoMetadata,
    NoQualifiedWindows,
    Ready,
}
#[derive(Debug)]
pub struct SessionCatalog {
    pub availability: Availability,
    pub snapshot: Snapshot,
    pub bindings: Vec<BoundMap>,
    pub maps: Vec<Arc<PreparedMap>>,
    pub metadata: catalog::Catalog,
    pub reports: Vec<pipeline::MapReport>,
    /// False only for a root initially absent. This is not terrain coverage.
    pub inventory_complete: bool,
    /// Pipeline's finite map-attempt/output walk, not exhaustive world search.
    pub catalog_complete: bool,
    pub unbound_maps: usize,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid saved session catalog: {0}")]
    Invalid(&'static str),
    #[error("saved session catalog limit exceeded: {0}")]
    Limit(&'static str),
    #[error("saved session catalog cancelled")]
    Cancelled,
    #[error("saved directory changed or became unavailable: {directory:?}")]
    Changed {
        directory: PathBuf,
        #[source]
        source: Option<Box<Error>>,
    },
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Persistence(history_store::Error),
    #[error(transparent)]
    Metadata(catalog::Error),
    #[error(transparent)]
    Pipeline(pipeline::Error),
    #[error(transparent)]
    Tour(tours::Error),
}
#[derive(Debug, PartialEq, Eq)]
struct Inventory {
    root: PathBuf,
    generation: (u64, u64),
    directories: BTreeMap<PathBuf, (u64, u64)>,
}
/// Blocking service for an owned worker. Root and private repository storage
/// are supplied explicitly and must be absolute; storage must be outside the
/// saves root. Returns complete source admissions and finite qualified maps.
/// The caller keeps returned History/revision outside its disposable Scene.
pub fn prepare(
    saves_root: &Path,
    storage_directory: &Path,
    generation: u64,
    limits: pipeline::Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<SessionCatalog, Error> {
    checkpoint(cancelled)?;
    if !saves_root.is_absolute() || !storage_directory.is_absolute() || generation == 0 {
        return Err(Error::Invalid(
            "absolute root/storage and nonzero generation required",
        ));
    }
    // Reuse the public pipeline's actual validation without doing filesystem
    // work or duplicating its evolving top-level limits/contract rules.
    pipeline::prepare_catalog(
        &catalog::Catalog::default(),
        &BTreeMap::new(),
        generation,
        limits,
        cancelled,
    )?;
    let observed = match std::fs::symlink_metadata(saves_root) {
        Ok(_) => Some(inventory(saves_root, cancelled)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let root_path = match &observed {
        Some(inventory) => inventory.root.clone(),
        None => projected_path(saves_root)?,
    };
    if projected_path(storage_directory)?.starts_with(&root_path) {
        return Err(Error::Invalid(
            "repository storage must be outside saves root",
        ));
    }
    let repository = Repository::new(storage_directory.to_owned())?;
    let snapshot = repository.load(cancelled)?;
    let Some(observed) = observed else {
        // A missing official install may return later. Do not bind an empty
        // catalog against it or retire its existing history/bindings.
        return Ok(SessionCatalog {
            availability: Availability::MissingRoot,
            snapshot,
            bindings: Vec::new(),
            maps: Vec::new(),
            metadata: catalog::Catalog::default(),
            reports: Vec::new(),
            inventory_complete: false,
            catalog_complete: false,
            unbound_maps: 0,
        });
    };
    verify_inventory(&observed, cancelled)?;
    let directories: Vec<_> = observed.directories.keys().cloned().collect();
    let bound =
        repository.bind_catalog(snapshot.revision(), &observed.root, &directories, cancelled)?;
    checkpoint(cancelled)?;
    // Bind every safe direct directory first, including folders with missing
    // or corrupt metadata; LastPlayed/LevelName never determine their IDs.
    let mut metadata = catalog::discover_metadata(&observed.root, cancelled)?;
    supplement_unexplored_metadata(&directories, &mut metadata, cancelled)?;
    verify_inventory(&observed, cancelled)?;
    if metadata
        .maps
        .iter()
        .any(|save| !observed.directories.contains_key(&save.directory))
    {
        return Err(changed(&observed.root, None));
    }
    let mut contexts = BTreeMap::new();
    for binding in &bound.maps {
        checkpoint(cancelled)?;
        contexts.insert(
            binding.directory.clone(),
            pipeline::MapContext {
                map: binding.map,
                recent: recent_centers(bound.snapshot.history(), binding.map)?,
            },
        );
    }
    // Immediately before the pipeline handoff, reject stale save receipts and
    // known region-directory aliases; no recursive walk or world mutation.
    for binding in &bound.maps {
        checkpoint(cancelled)?;
        binding
            .verify(&observed.root)
            .map_err(|error| changed(&binding.directory, Some(Error::Persistence(error))))?;
    }
    for save in &metadata.maps {
        checkpoint(cancelled)?;
        let directory = NoFollowDirectory::open_root(&save.directory)
            .map_err(|error| changed(&save.directory, Some(Error::Io(error))))?;
        match directory.open_directory("region".as_ref()) {
            Ok(_) => (),
            // Ordinary unexplored worlds (or removed terrain) are finite
            // per-map pipeline failures, not a failure of a qualified peer.
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(changed(&save.directory, Some(Error::Io(error)))),
        }
    }
    let prepared = pipeline::prepare_catalog(&metadata, &contexts, generation, limits, cancelled)?;
    verify_inventory(&observed, cancelled)?;
    for binding in &bound.maps {
        checkpoint(cancelled)?;
        binding
            .verify(&observed.root)
            .map_err(|error| changed(&binding.directory, Some(Error::Persistence(error))))?;
    }
    if prepared.unbound_maps != 0 {
        return Err(Error::Invalid(
            "prepared catalog contains unbound metadata maps",
        ));
    }
    let mut maps = Vec::with_capacity(prepared.maps.len());
    for prepared_save in prepared.maps {
        checkpoint(cancelled)?;
        let window = prepared_save.window;
        let mut budget = tours::Budget::new(limits.preparation.work_units as u64, cancelled);
        let map = PreparedMap::new(
            window.source,
            prepared_save.last_played,
            Arc::new(window.loaded),
            window.targets,
            &mut budget,
        )?;
        maps.push(Arc::new(map));
    }
    // Do not start a new controller with an obsolete History if a different
    // worker committed it while this potentially expensive preparation ran.
    let current = repository.load(cancelled)?;
    if current.revision() != bound.snapshot.revision() {
        return Err(Error::Persistence(history_store::Error::Conflict {
            current: Box::new(current),
        }));
    }
    // PreparedMap validation can itself do bounded CPU work. Recheck source
    // observations after it and revision readback, just before delivery.
    verify_inventory(&observed, cancelled)?;
    checkpoint(cancelled)?;
    let availability = if !maps.is_empty() {
        Availability::Ready
    } else if bound.maps.is_empty() {
        Availability::EmptyRoot
    } else if metadata.maps.is_empty() {
        Availability::NoMetadata
    } else {
        Availability::NoQualifiedWindows
    };
    Ok(SessionCatalog {
        availability,
        snapshot: bound.snapshot,
        bindings: bound.maps,
        maps,
        metadata,
        reports: prepared.reports,
        inventory_complete: true,
        catalog_complete: prepared.catalog_complete,
        unbound_maps: prepared.unbound_maps,
    })
}

fn inventory(root: &Path, cancelled: &dyn Fn() -> bool) -> Result<Inventory, Error> {
    checkpoint(cancelled)?;
    // Open the authored final entry before canonicalization can hide a link.
    let handle = NoFollowDirectory::open_root(root)?;
    let root = paths::canonicalize(root)?;
    let generation = secure_fs::directory_generation(&root)?;
    collect_inventory(
        &root,
        &handle,
        generation,
        std::fs::read_dir(&root)?,
        cancelled,
    )
}
fn collect_inventory(
    root: &Path,
    handle: &NoFollowDirectory,
    generation: (u64, u64),
    entries: impl Iterator<Item = io::Result<std::fs::DirEntry>>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Inventory, Error> {
    let mut directories = BTreeMap::new();
    for (number, entry) in entries.enumerate() {
        checkpoint(cancelled)?;
        if number >= MAX_DIRECTORY_ENTRIES {
            return Err(Error::Limit("saves root directory entries"));
        }
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(Error::Invalid("saves root contains a direct symbolic link"));
        }
        if !kind.is_dir() {
            continue;
        }
        handle.open_directory(&entry.file_name())?;
        let directory = root.join(entry.file_name());
        if directory.parent() != Some(root) || paths::canonicalize(&directory)? != directory {
            return Err(Error::Invalid(
                "save directory must be an exact canonical direct child",
            ));
        }
        let object = secure_fs::directory_generation(&directory)?;
        if directories.insert(directory, object).is_some() {
            return Err(Error::Invalid("duplicate directory observation"));
        }
        if directories.len() > history_store::MAX_ADMISSION {
            return Err(Error::Limit("complete directory admission batch"));
        }
    }
    checkpoint(cancelled)?;
    if secure_fs::directory_generation(root)? != generation {
        return Err(changed(root, None));
    }
    Ok(Inventory {
        root: root.to_owned(),
        generation,
        directories,
    })
}
fn verify_inventory(observed: &Inventory, cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
    match inventory(&observed.root, cancelled) {
        Ok(current) if current == *observed => Ok(()),
        Ok(_) => Err(changed(&observed.root, None)),
        Err(Error::Cancelled) => Err(Error::Cancelled),
        Err(error) => Err(changed(&observed.root, Some(error))),
    }
}
fn changed(directory: &Path, source: Option<Error>) -> Error {
    Error::Changed {
        directory: directory.to_owned(),
        source: source.map(Box::new),
    }
}
fn recent_centers(history: History, map: MapId) -> Result<Vec<[i32; 2]>, Error> {
    let mut centers = Vec::new();
    let mut distinct = BTreeSet::new();
    for traversal in history
        .traversals()
        .filter(|traversal| traversal.route.map == map)
    {
        // Route endpoints are quarter-block integers. Their midpoint divided
        // by sixteen blocks is (left+right)/128, with floor for negatives.
        let midpoint = [0, 1].map(|axis| {
            (i128::from(traversal.route.endpoints[0][axis])
                + i128::from(traversal.route.endpoints[1][axis]))
            .div_euclid(128)
        });
        let center = [i32::try_from(midpoint[0]), i32::try_from(midpoint[1])];
        let [Ok(x), Ok(z)] = center else {
            return Err(Error::Invalid("retained route center outside chunk domain"));
        };
        if !distinct.insert([x, z]) {
            continue;
        }
        if centers.len() == 64 {
            return Err(Error::Limit("retained route centers"));
        }
        centers.push([x, z]);
    }
    Ok(centers)
}
/// The metadata discovery adapter intentionally requires a region folder.
/// Retain valid level.dat-only worlds as candidates too, so absent terrain
/// produces a bounded per-map pipeline report rather than "missing metadata".
fn supplement_unexplored_metadata(
    directories: &[PathBuf],
    metadata: &mut catalog::Catalog,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Error> {
    let known: BTreeSet<_> = metadata
        .maps
        .iter()
        .map(|save| save.directory.clone())
        .chain(metadata.issues.iter().map(|issue| issue.directory.clone()))
        .collect();
    for directory in directories {
        checkpoint(cancelled)?;
        if known.contains(directory) {
            continue;
        }
        // Exact original discovery predicate: those with a region directory
        // were already attempted, even if its bounded issue list is full.
        let has_region = match std::fs::metadata(directory.join("region")) {
            Ok(metadata) => metadata.is_dir(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(changed(directory, Some(Error::Io(error)))),
        };
        if has_region {
            continue;
        }
        let handle = NoFollowDirectory::open_root(directory)?;
        match handle.open_regular("level.dat".as_ref()) {
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                record_metadata_issue(metadata, directory, &error);
                continue;
            }
        }
        match catalog::read_metadata(directory, cancelled) {
            Ok(value) => metadata.maps.push(catalog::Save {
                directory: directory.clone(),
                metadata: value,
            }),
            Err(catalog::Error::Region(region::Error::Cancelled)) => return Err(Error::Cancelled),
            Err(error) => record_metadata_issue(metadata, directory, &error),
        }
    }
    metadata.maps.sort_by(|left, right| {
        right
            .metadata
            .last_played
            .cmp(&left.metadata.last_played)
            .then_with(|| left.directory.cmp(&right.directory))
    });
    Ok(())
}
fn record_metadata_issue(
    metadata: &mut catalog::Catalog,
    directory: &Path,
    error: &dyn std::fmt::Display,
) {
    metadata.rejected_maps += 1;
    if metadata.issues.len() < 64 {
        metadata.issues.push(catalog::MapIssue {
            directory: directory.to_owned(),
            message: error.to_string().chars().take(256).collect(),
        });
    }
}
/// Resolve an existing ancestor without creating anything, then append missing
/// components. This protects the world-write exclusion even for a storage
/// alias below an existing symlink parent; ordinary nonexisting data dirs work.
fn projected_path(path: &Path) -> Result<PathBuf, Error> {
    let mut ancestor = path.to_owned();
    let mut missing = Vec::new();
    loop {
        match paths::canonicalize(&ancestor) {
            Ok(mut canonical) => {
                for component in missing.into_iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = ancestor
                    .file_name()
                    .ok_or(Error::Invalid("unresolvable storage/root path"))?
                    .to_owned();
                missing.push(name);
                if !ancestor.pop() {
                    return Err(Error::Invalid("unresolvable absolute path"));
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
}
fn checkpoint(cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
impl From<history_store::Error> for Error {
    fn from(error: history_store::Error) -> Self {
        match error {
            history_store::Error::Cancelled => Self::Cancelled,
            other => Self::Persistence(other),
        }
    }
}
impl From<catalog::Error> for Error {
    fn from(error: catalog::Error) -> Self {
        match error {
            catalog::Error::Region(region::Error::Cancelled) => Self::Cancelled,
            other => Self::Metadata(other),
        }
    }
}
impl From<pipeline::Error> for Error {
    fn from(error: pipeline::Error) -> Self {
        match error {
            pipeline::Error::Cancelled => Self::Cancelled,
            other => Self::Pipeline(other),
        }
    }
}
impl From<tours::Error> for Error {
    fn from(error: tours::Error) -> Self {
        match error {
            tours::Error::Cancelled => Self::Cancelled,
            other => Self::Tour(other),
        }
    }
}
#[cfg(test)]
#[path = "session_catalog_tests.rs"]
mod tests;
