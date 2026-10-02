//! Blocking worker-only persistent identity and tour history repository.
//!
//! Load before constructing a tour controller. Never call filesystem methods
//! from render/presented callbacks; queue snapshots to an owned worker.
//! Limits bound retained records and serialized bytes, not RSS or I/O latency.
//! Generations fence current directories, not eternal identity: OS ID reuse
//! and check/open races remain. Moves are never automatically rebound.
use super::{
    evidence::MapId,
    tours::{Controller, History},
};
use ilium_platform::{
    file_lock::ExclusiveFileLock,
    minecraft, paths,
    secure_fs::{self, NoFollowDirectory},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
pub const MAX_ADMISSION: usize = 512;
pub const MAX_BINDINGS: usize = 4096;
pub const MAX_STATE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 16 * 1024;
const SCHEMA: u32 = 1;
const STATE_FILE: &str = "history.json";
const LOCK_FILE: &str = "history.lock";
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid Minecraft history: {0}")]
    Invalid(&'static str),
    #[error("Minecraft history limit exceeded: {0}")]
    Limit(&'static str),
    #[error("Minecraft history operation cancelled before publication")]
    Cancelled,
    #[error("Minecraft history repository is busy; retry on a worker")]
    Busy,
    #[error("Minecraft history revision conflict (current {revision})", revision = .current.revision)]
    Conflict { current: Box<Snapshot> },
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Rename succeeded; reload instead of retrying an old revision.
    #[error("Minecraft history revision {revision} published but confirmation failed: {source}")]
    Published {
        revision: u64,
        #[source]
        source: Box<Error>,
    },
    #[error(
        "Minecraft history operation failed ({cause}); owned temporary cleanup failed: {source}"
    )]
    Cleanup {
        cause: Box<Error>,
        #[source]
        source: io::Error,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    schema: u32,
    revision: u64,
    bindings: Vec<Binding>,
    history: History,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            revision: 0,
            bindings: Vec::new(),
            history: History::default(),
        }
    }
}
impl Snapshot {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn history(&self) -> History {
        self.history
    }
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }
    fn validate(&self) -> Result<(), Error> {
        if self.schema != SCHEMA {
            return Err(Error::Invalid("unrecognized schema"));
        }
        if self.bindings.len() > MAX_BINDINGS {
            return Err(Error::Limit("retained bindings"));
        }
        let mut identifiers = BTreeSet::new();
        let mut active = BTreeSet::new();
        for binding in &self.bindings {
            if binding.map.0 == [0; 16] || !identifiers.insert(binding.map) {
                return Err(Error::Invalid("zero or duplicate map identifier"));
            }
            for key in [&binding.root_key, &binding.path_key] {
                if key.is_empty() {
                    return Err(Error::Invalid("empty native path key"));
                }
                if key.len() > MAX_PATH_BYTES {
                    return Err(Error::Limit("native path key"));
                }
            }
            if binding.active && !active.insert((&binding.root_key, &binding.path_key)) {
                return Err(Error::Invalid("duplicate active binding"));
            }
        }
        validate_history(self.history)?;
        if self
            .history
            .appearances()
            .any(|seen| !identifiers.contains(&seen.key.map))
            || self
                .history
                .traversals()
                .any(|route| !identifiers.contains(&route.route.map))
        {
            return Err(Error::Invalid("history references an unbound map"));
        }
        if self.revision == 0 && (!self.bindings.is_empty() || self.history != History::default()) {
            return Err(Error::Invalid("nonempty revision zero"));
        }
        Ok(())
    }
}
/// Retired records retain their original ID, path and generation forever.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub map: MapId,
    pub root_key: Vec<u8>,
    pub path_key: Vec<u8>,
    pub root_generation: (u64, u64),
    pub directory_generation: (u64, u64),
    pub active: bool,
}
/// Admission receipt to recheck immediately before handing a save to loading.
#[derive(Debug, Clone)]
pub struct BoundMap {
    pub directory: PathBuf,
    pub map: MapId,
    root_key: Vec<u8>,
    root_generation: (u64, u64),
    directory_generation: (u64, u64),
}
impl BoundMap {
    pub fn verify(&self, canonical_root: &Path) -> Result<(), Error> {
        require_direct_child(canonical_root, &self.directory)?;
        if minecraft::native_path_key(canonical_root)? != self.root_key
            || secure_fs::directory_generation(canonical_root)? != self.root_generation
            || secure_fs::directory_generation(&self.directory)? != self.directory_generation
        {
            return Err(Error::Invalid("bound save directory changed"));
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct BoundCatalog {
    pub snapshot: Snapshot,
    pub maps: Vec<BoundMap>,
}
#[derive(Debug, Clone)]
pub struct Repository {
    directory: PathBuf,
}
impl Repository {
    pub fn new(directory: PathBuf) -> Result<Self, Error> {
        if !directory.is_absolute() {
            return Err(Error::Invalid("storage directory must be absolute"));
        }
        Ok(Self { directory })
    }
    fn transaction(&self, cancelled: &dyn Fn() -> bool) -> Result<Transaction, Error> {
        checkpoint(cancelled)?;
        secure_fs::create_private_directory(&self.directory)?;
        // Refuse the original final entry before canonicalization could hide
        // a Windows junction or other alias as its plain target directory.
        let root = NoFollowDirectory::open_root(&self.directory)?;
        let directory = paths::canonicalize(&self.directory)?;
        let generation = secure_fs::directory_generation(&directory)?;
        // Do not reopen by path: Windows' generic private OpenOptions follows
        // a reparse point. Hold the exact handle admitted through this root.
        let lock_file = match root.create_regular(LOCK_FILE.as_ref()) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                root.open_regular(LOCK_FILE.as_ref())?
            }
            Err(error) => return Err(error.into()),
        };
        let lock = ExclusiveFileLock::try_acquire_opened(lock_file)?.ok_or(Error::Busy)?;
        checkpoint(cancelled)?;
        let transaction = Transaction {
            _lock: lock,
            root,
            directory,
            generation,
        };
        transaction.verify_root()?;
        Ok(transaction)
    }
    /// Blocking worker API; missing state is default, malformed state is error.
    pub fn load(&self, cancelled: &dyn Fn() -> bool) -> Result<Snapshot, Error> {
        let transaction = self.transaction(cancelled)?;
        let snapshot = transaction.read()?;
        checkpoint(cancelled)?;
        Ok(snapshot)
    }
    /// Admit a COMPLETE catalog for one explicit canonical saves root. Missing
    /// rows are retired: passing a filtered/partial catalog is a caller bug.
    /// Every path must be a canonical direct child. No member is published if
    /// any member/cap/checkpoint fails; preserve input order for the caller.
    pub fn bind_catalog(
        &self,
        expected_revision: u64,
        canonical_root: &Path,
        directories: &[PathBuf],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<BoundCatalog, Error> {
        checkpoint(cancelled)?;
        if directories.len() > MAX_ADMISSION {
            return Err(Error::Limit("catalog admission batch"));
        }
        let transaction = self.transaction(cancelled)?;
        let current = transaction.compare(expected_revision)?;
        let root_generation = secure_fs::directory_generation(canonical_root)?;
        let root_key = minecraft::native_path_key(canonical_root)?;
        let mut snapshot = current.clone();
        let mut keys = BTreeSet::new();
        let mut maps = Vec::with_capacity(directories.len());
        for directory in directories {
            checkpoint(cancelled)?;
            require_direct_child(canonical_root, directory)?;
            let path_key = minecraft::native_path_key(directory)?;
            if !keys.insert(path_key.clone()) {
                return Err(Error::Invalid("duplicate catalog directory"));
            }
            let directory_generation = secure_fs::directory_generation(directory)?;
            let existing = snapshot.bindings.iter().find(|binding| {
                binding.active
                    && binding.root_key == root_key
                    && binding.path_key == path_key
                    && binding.root_generation == root_generation
                    && binding.directory_generation == directory_generation
            });
            let map = if let Some(binding) = existing {
                binding.map
            } else {
                if snapshot.bindings.len() >= MAX_BINDINGS {
                    return Err(Error::Limit("retained bindings"));
                }
                let map = allocate_identifier(&snapshot.bindings)?;
                for old in &mut snapshot.bindings {
                    if old.root_key == root_key && old.path_key == path_key {
                        old.active = false;
                    }
                }
                snapshot.bindings.push(Binding {
                    map,
                    root_key: root_key.clone(),
                    path_key,
                    root_generation,
                    directory_generation,
                    active: true,
                });
                map
            };
            maps.push(BoundMap {
                directory: directory.clone(),
                map,
                root_key: root_key.clone(),
                root_generation,
                directory_generation,
            });
        }
        for binding in &mut snapshot.bindings {
            if binding.root_key == root_key
                && (binding.root_generation != root_generation || !keys.contains(&binding.path_key))
            {
                binding.active = false;
            }
        }
        // A later loader must also verify these receipts; no path-held lease
        // is asserted by this whole-batch pre-publication check.
        for map in &maps {
            checkpoint(cancelled)?;
            map.verify(canonical_root)?;
        }
        if secure_fs::directory_generation(canonical_root)? != root_generation {
            return Err(Error::Invalid("saves root changed during admission"));
        }
        checkpoint(cancelled)?;
        if snapshot != current {
            increment_revision(&mut snapshot)?;
            transaction.publish(&snapshot, cancelled, &|_| Ok(()))?;
        }
        Ok(BoundCatalog { snapshot, maps })
    }
    /// Preserve every binding and reject a stale revision with current state.
    /// Never silently merge or discard appearances/routes on conflict.
    pub fn commit_history(
        &self,
        expected_revision: u64,
        history: History,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Snapshot, Error> {
        self.commit_history_with(expected_revision, history, cancelled, &|_| Ok(()))
    }
    fn commit_history_with(
        &self,
        expected_revision: u64,
        history: History,
        cancelled: &dyn Fn() -> bool,
        write_hook: &dyn Fn(&mut File) -> io::Result<()>,
    ) -> Result<Snapshot, Error> {
        let transaction = self.transaction(cancelled)?;
        let mut snapshot = transaction.compare(expected_revision)?;
        validate_history(history)?;
        snapshot.history = history;
        increment_revision(&mut snapshot)?;
        transaction.publish(&snapshot, cancelled, write_hook)?;
        Ok(snapshot)
    }
}

struct Transaction {
    _lock: ExclusiveFileLock,
    root: NoFollowDirectory,
    directory: PathBuf,
    generation: (u64, u64),
}
impl Transaction {
    fn verify_root(&self) -> Result<(), Error> {
        if secure_fs::directory_generation(&self.directory)? != self.generation {
            return Err(Error::Invalid("repository directory changed"));
        }
        Ok(())
    }
    fn read(&self) -> Result<Snapshot, Error> {
        self.verify_root()?;
        let file = match self.root.open_regular(STATE_FILE.as_ref()) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Snapshot::default()),
            Err(error) => return Err(error.into()),
        };
        let bytes = read_bounded(file)?;
        let snapshot: Snapshot = serde_json::from_slice(&bytes)?;
        // History belongs to the domain crate and its Serde derives accept
        // unknown fields. Refuse those instead of silently dropping authored
        // data when a later commit serializes the recognized schema.
        if serde_json::to_value(&snapshot)? != serde_json::from_slice::<serde_json::Value>(&bytes)?
        {
            return Err(Error::Invalid("unrecognized persisted fields"));
        }
        snapshot.validate()?;
        Ok(snapshot)
    }
    fn compare(&self, expected: u64) -> Result<Snapshot, Error> {
        let current = self.read()?;
        if current.revision != expected {
            return Err(Error::Conflict {
                current: Box::new(current),
            });
        }
        Ok(current)
    }
    fn publish(
        &self,
        snapshot: &Snapshot,
        cancelled: &dyn Fn() -> bool,
        write_hook: &dyn Fn(&mut File) -> io::Result<()>,
    ) -> Result<(), Error> {
        checkpoint(cancelled)?;
        snapshot.validate()?;
        let mut bounded = BoundedJson {
            bytes: Vec::new(),
            exceeded: false,
        };
        let serialized = serde_json::to_writer(&mut bounded, snapshot);
        if bounded.exceeded {
            return Err(Error::Limit("serialized state bytes"));
        }
        serialized?;
        let bytes = bounded.bytes;
        let identifier = minecraft::random_map_identifier()?;
        let token: String = identifier
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let name = format!(".history-{token}.tmp");
        checkpoint(cancelled)?;
        let mut file = self.root.create_regular(name.as_ref())?;
        let prepare = (|| {
            checkpoint(cancelled)?;
            secure_fs::restrict_open_file_to_owner(&file)?;
            write_hook(&mut file)?;
            checkpoint(cancelled)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            if read_bounded(self.root.open_regular(name.as_ref())?)? != bytes {
                return Err(Error::Invalid("temporary state readback differs"));
            }
            self.verify_root()?;
            checkpoint(cancelled)?;
            std::fs::rename(self.directory.join(&name), self.directory.join(STATE_FILE))?;
            Ok(())
        })();
        if let Err(cause) = prepare {
            return match self.root.remove_regular(name.as_ref(), &file) {
                Ok(()) => Err(cause),
                Err(source) => Err(Error::Cleanup {
                    cause: Box::new(cause),
                    source,
                }),
            };
        }
        // Once rename committed, cancellation cannot undo publication. Report
        // the commit accurately; no compensating write or backup is created.
        let confirm = (|| {
            self.root.sync_all()?;
            self.verify_root()?;
            if read_bounded(self.root.open_regular(STATE_FILE.as_ref())?)? != bytes {
                return Err(Error::Invalid("published state readback differs"));
            }
            Ok(())
        })();
        confirm.map_err(|source| Error::Published {
            revision: snapshot.revision,
            source: Box::new(source),
        })
    }
}
fn validate_history(history: History) -> Result<(), Error> {
    Controller::new(1, history)
        .map(|_| ())
        .map_err(|_| Error::Invalid("tour history contract"))
}
fn allocate_identifier(bindings: &[Binding]) -> Result<MapId, Error> {
    for _ in 0..4 {
        let map = MapId(minecraft::random_map_identifier()?);
        if map.0 != [0; 16] && bindings.iter().all(|binding| binding.map != map) {
            return Ok(map);
        }
    }
    Err(Error::Invalid("random map identifier collision"))
}
fn increment_revision(snapshot: &mut Snapshot) -> Result<(), Error> {
    snapshot.revision = snapshot
        .revision
        .checked_add(1)
        .ok_or(Error::Limit("store revision"))?;
    Ok(())
}
fn checkpoint(cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
fn require_direct_child(root: &Path, directory: &Path) -> Result<(), Error> {
    if !root.is_absolute() || !directory.is_absolute() || directory.parent() != Some(root) {
        return Err(Error::Invalid(
            "save must be a direct child of explicit saves root",
        ));
    }
    if paths::canonicalize(root)? != root || paths::canonicalize(directory)? != directory {
        return Err(Error::Invalid(
            "save and root must be canonical directories",
        ));
    }
    Ok(())
}
fn read_bounded(file: File) -> Result<Vec<u8>, Error> {
    if file.metadata()?.len() > MAX_STATE_BYTES as u64 {
        return Err(Error::Limit("state file bytes"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_STATE_BYTES {
        return Err(Error::Limit("state file bytes"));
    }
    Ok(bytes)
}

struct BoundedJson {
    bytes: Vec<u8>,
    exceeded: bool,
}
impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_STATE_BYTES.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(io::Error::other("serialized state limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
#[path = "history_store_tests.rs"]
mod tests;
