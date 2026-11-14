//! Host implementation of `ilium:host/fs`.
//!
//! Every scope (`package`, `data`, `cache`) is a folder on disk. Paths are checked lexically (no
//! absolute paths, no `..`) and again against the canonical scope root so a symlink cannot lead
//! out of the scope. All state (scope folders, answers waiting for a `take-*` call, advisory
//! locks, watches) belongs to the import value of one plugin instance and ends with it. Asynchronous
//! operations finish inside the call; `request-completed` is queued for the plugin and the answer
//! waits for the matching `take-*` call. Failures are returned by the call itself. A watch polls
//! its folder on a worker thread owned by the import and queues `fs-changed` events.
//!
//! The `project` scope opens on its grants. Read-only named scopes (`agent-sessions:<agent>`,
//! `agent-config:<agent>`) open on `fs.read:<scope>` once the composition root points them at a
//! folder (`configure_read_only_scope`). `pick` asks the [`FilePicker`] of the process on a worker
//! thread; each confirmed path becomes a `picked/<n>` scope of this instance, opened by
//! `fs.read:picked` or `fs.write:picked`. `agent-config.propose` still answers `not-permitted`.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ilium_plugin_api::generated::host::fs::{
    ConfigChange, DirEntry, EntryKind, FsMetadata, HostFs, PickKind, PickSpec, ProposalResult,
    RevisionToken, Revisioned, ScannedEntry, ScopeRoot, TailChunk, WriteMode,
};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::events::{Event, FsChangedEvent};
use ilium_plugin_api::generated::types::ids::{LockId, RequestId, WatchId};
use ilium_plugin_api::generated::types::paths::FsPath;
use ilium_plugin_api::generated::types::Outcome;
use ilium_plugin_api::grants::Grant;
use regex::Regex;
use sha2::{Digest, Sha256};

use super::env::{completion_event, CancelFlag, CancelOnDrop, ImportEnv, RequestIds};
use crate::runtime::pending_events::EventSink;

/// Asks the user for a path. The client composes one that shows the kernel's picker; a side
/// without one leaves `pick` unsupported.
pub trait FilePicker: Send + Sync {
    /// Blocks until the user confirms a path (`Some`) or dismisses the picker (`None`). Returns
    /// early with `None` once `cancel` is set (the instance ended).
    fn pick(&self, spec: &PickSpec, cancel: &CancelFlag) -> Result<Option<PathBuf>, PluginError>;
}

/// The paths the user chose for one instance and the answers waiting for `take-picked`.
#[derive(Debug, Default)]
struct PickBook {
    /// `picked/<n>` is `chosen[n]`.
    chosen: Vec<PathBuf>,
    answers: HashMap<u64, Option<FsPath>>,
}

/// Prefix of the scopes the user chose with `pick`.
const SCOPE_PICKED_PREFIX: &str = "picked/";

/// Largest `max_bytes` of a synchronous `read` or `tail` (1 MiB).
const SYNC_READ_MAX_BYTES: u32 = 1024 * 1024;
/// Largest `max_entries` of `list-dir`.
const LIST_DIR_MAX_ENTRIES: u32 = 4_096;
/// Largest `max_entries` of `scan`.
const SCAN_MAX_ENTRIES: u32 = 65_536;
/// Largest payload of one write (`fs_write_mib`, 16 MiB by default).
const WRITE_MAX_BYTES: usize = 16 * 1024 * 1024;
/// Largest file `read-all` and `read-with-revision` return (64 MiB by default).
const READ_ALL_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Largest lock wait in milliseconds.
const LOCK_MAX_TIMEOUT_MILLIS: u32 = 60_000;
/// Longest error message in characters (api-errors).
const MESSAGE_MAX_CHARS: usize = 256;

const SCOPE_PACKAGE: &str = "package";
const SCOPE_DATA: &str = "data";
const SCOPE_CACHE: &str = "cache";
const SCOPE_PROJECT: &str = "project";
/// Folder of the project that no plugin may reach through the `project` scope.
const PROJECT_HIDDEN_FOLDER: &str = ".ilium";

/// Watches one plugin may hold (api-fs: `watches`, default 64).
const WATCHES_PER_PLUGIN: usize = 64;
/// How often a watch looks at its folder.
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// After the first change a watch waits this long to batch more.
const WATCH_WINDOW: Duration = Duration::from_millis(200);
/// A watch sends at most five events per second.
const WATCH_MIN_EVENT_GAP: Duration = Duration::from_millis(200);
/// One `fs-changed` event lists at most this many paths.
const WATCH_MAX_PATHS: usize = 1_024;

/// Host side of the `fs` interface of one plugin instance.
#[derive(Debug)]
pub struct FsImport {
    env: ImportEnv,
    state: FsState,
    picker: Option<Arc<dyn FilePicker>>,
    /// Sets the cancel flag of running picks when the instance ends.
    life: CancelOnDrop,
}

impl std::fmt::Debug for dyn FilePicker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FilePicker")
    }
}

impl Default for FsImport {
    fn default() -> Self {
        Self::new(ImportEnv::unrestricted())
    }
}

/// Folders behind the three always granted scopes.
#[derive(Debug, Clone)]
struct ScopeDirs {
    package: PathBuf,
    data: PathBuf,
    cache: PathBuf,
}

/// The operation a call performs, which decides the grant it needs on a granted scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
    Watch,
    /// A lock needs both the read and the write grant.
    Lock,
}

/// The `project` scope: the session's project folder and the grants that open it.
#[derive(Debug, Clone)]
struct ProjectScope {
    root: PathBuf,
    grants: Arc<Mutex<Vec<Grant>>>,
}

impl ProjectScope {
    fn granted(&self, family: &str) -> bool {
        let wanted = format!("{family}:{SCOPE_PROJECT}");
        self.grants
            .lock()
            .map(|grants| grants.iter().any(|grant| grant.as_str() == wanted))
            .unwrap_or(false)
    }

    fn allows(&self, access: Access) -> bool {
        match access {
            Access::Read => self.granted("fs.read"),
            Access::Write => self.granted("fs.write"),
            Access::Watch => self.granted("fs.watch"),
            Access::Lock => self.granted("fs.read") && self.granted("fs.write"),
        }
    }
}

/// One running watch: its worker polls the folder until `stop` is set.
#[derive(Debug)]
struct WatchEntry {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for WatchEntry {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// State of the import of one plugin instance.
#[derive(Debug)]
struct FsState {
    env: ImportEnv,
    project: Option<ProjectScope>,
    /// Read-only scopes by name (`agent-sessions:claude`), each opened by `fs.read:<name>`.
    read_only: BTreeMap<String, PathBuf>,
    /// What `pick` returned; shared with the pick workers.
    picks: Arc<Mutex<PickBook>>,
    dirs: Option<ScopeDirs>,
    /// Scratch folder created by this state; removed when the thread ends.
    owned_dir: Option<PathBuf>,
    next_id: u64,
    bytes: HashMap<u64, Vec<u8>>,
    entries: HashMap<u64, Vec<ScannedEntry>>,
    lock_answers: HashMap<u64, LockId>,
    revisioned: HashMap<u64, Revisioned>,
    written: HashMap<u64, RevisionToken>,
    watches: HashMap<u64, WatchEntry>,
    /// Held advisory locks: lock id to (scope-qualified path, exclusive).
    held_locks: HashMap<u64, (String, bool)>,
    /// Outcomes of finished asynchronous requests, until the test harness reads them.
    completed: HashMap<u64, Outcome>,
    /// When set, new requests stay pending (the harness holds completions).
    hold: bool,
    /// Requests accepted while completions were held.
    held: Vec<(u64, Outcome)>,
}

impl FsImport {
    /// The import of the instance `env` describes. Its scopes use a private scratch folder until
    /// `configure_scope_roots` points them at real folders.
    pub fn new(env: ImportEnv) -> Self {
        Self {
            state: FsState::new(env.clone()),
            env,
            picker: None,
            life: CancelOnDrop::default(),
        }
    }

    /// Lets `pick` ask `picker` for a path.
    pub fn set_file_picker(&mut self, picker: Arc<dyn FilePicker>) {
        self.picker = Some(picker);
    }

    /// Adds the read-only scope `scope` rooted at `root`; it opens only while the instance holds
    /// `fs.read:<scope>` (and `fs.watch:<scope>` for a watch). A root that does not exist (the
    /// agent is not installed) is `not-found` and leaves the scope out.
    pub fn configure_read_only_scope(
        &mut self,
        scope: &str,
        root: &Path,
    ) -> Result<(), PluginError> {
        let canonical = fs::canonicalize(root).map_err(io_error)?;
        self.state.read_only.insert(scope.to_owned(), canonical);
        Ok(())
    }

    /// Points the three scopes at real folders (created when missing).
    pub fn configure_scope_roots(
        &mut self,
        package: PathBuf,
        data: PathBuf,
        cache: PathBuf,
    ) -> Result<(), PluginError> {
        for folder in [&package, &data, &cache] {
            fs::create_dir_all(folder).map_err(io_error)?;
        }
        let canonical = |folder: &Path| fs::canonicalize(folder).map_err(io_error);
        self.state.dirs = Some(ScopeDirs {
            package: canonical(&package)?,
            data: canonical(&data)?,
            cache: canonical(&cache)?,
        });
        Ok(())
    }

    /// Points the `project` scope at the session's project folder. The scope opens only while
    /// `fs.read:project`, `fs.write:project` or `fs.watch:project` is in `grants`; the list is
    /// shared, so a later revocation takes effect on the next call.
    pub fn configure_project_scope(
        &mut self,
        root: PathBuf,
        grants: Arc<Mutex<Vec<Grant>>>,
    ) -> Result<(), PluginError> {
        let canonical = fs::canonicalize(&root).map_err(io_error)?;
        self.state.project = Some(ProjectScope {
            root: canonical,
            grants,
        });
        Ok(())
    }

    /// The outcome of a finished asynchronous request, if it has finished.
    pub fn completed_outcome(&self, request: RequestId) -> Option<Outcome> {
        self.state.completed.get(&request.0).cloned()
    }

    /// Holds (or releases) the completion of new asynchronous requests. Releasing finishes every
    /// held request with the outcome it had.
    pub fn hold_completions(&mut self, on: bool) {
        self.state.hold = on;
        if !on {
            for (id, outcome) in std::mem::take(&mut self.state.held) {
                self.state.complete(id, outcome);
            }
        }
    }

    /// How many watches run.
    pub fn watch_count(&self) -> usize {
        self.state.watches.len()
    }

    fn with_state<R>(&mut self, action: impl FnOnce(&mut FsState) -> R) -> R {
        action(&mut self.state)
    }

    /// Resolves a scope-relative path to a real location, enforcing the scope rules.
    fn resolve(&mut self, path: &FsPath, access: Access) -> Result<Resolved, PluginError> {
        resolve_in(&mut self.state, path, access)
    }
}

fn plugin_error(code: ErrorCode, message: impl Into<String>) -> PluginError {
    let message: String = message.into();
    PluginError {
        code,
        message: message.chars().take(MESSAGE_MAX_CHARS).collect(),
        retry_after_ms: None,
    }
}

fn io_error(error: std::io::Error) -> PluginError {
    use std::io::ErrorKind;
    let code = match error.kind() {
        ErrorKind::NotFound => ErrorCode::NotFound,
        ErrorKind::PermissionDenied => ErrorCode::NotPermitted,
        ErrorKind::AlreadyExists => ErrorCode::Conflict,
        _ => ErrorCode::Io,
    };
    plugin_error(code, error.to_string())
}

/// Largest number of asynchronous fs requests pending at once.
const PENDING_REQUEST_CAP: usize = 64;

impl FsState {
    fn new(env: ImportEnv) -> Self {
        Self {
            env,
            project: None,
            read_only: BTreeMap::new(),
            picks: Arc::default(),
            dirs: None,
            owned_dir: None,
            next_id: 0,
            bytes: HashMap::new(),
            entries: HashMap::new(),
            lock_answers: HashMap::new(),
            revisioned: HashMap::new(),
            written: HashMap::new(),
            watches: HashMap::new(),
            held_locks: HashMap::new(),
            completed: HashMap::new(),
            hold: false,
            held: Vec::new(),
        }
    }

    /// Records the outcome of a request and queues `request-completed` for the plugin.
    fn complete(&mut self, id: u64, outcome: Outcome) {
        self.completed.insert(id, outcome.clone());
        self.env.complete(RequestId(id), outcome);
    }

    /// Refuses a new asynchronous request while `PENDING_REQUEST_CAP` are pending.
    fn require_capacity(&self) -> Result<(), PluginError> {
        if self.held.len() >= PENDING_REQUEST_CAP {
            return Err(plugin_error(
                ErrorCode::Busy,
                "too many fs requests are pending",
            ));
        }
        Ok(())
    }

    /// Records a request as finished, or pending while completions are held.
    fn finish_with(&mut self, id: u64, outcome: Outcome) {
        if self.hold {
            self.held.push((id, outcome));
        } else {
            self.complete(id, outcome);
        }
    }

    fn finish(&mut self, id: u64) {
        self.finish_with(id, Outcome::Ok);
    }

    /// Allocates a request id for an operation that has no result to take.
    fn complete_new(&mut self) -> u64 {
        let id = self.allocate_request();
        self.finish(id);
        id
    }

    /// A request id of the runtime; it names exactly one `request-completed` event.
    fn allocate_request(&mut self) -> u64 {
        self.env.next_request().0
    }

    /// A lock or watch handle, unique inside this import.
    fn allocate_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Returns the scope folders, creating the private scratch folders on first use.
    fn scope_dirs(&mut self) -> Result<ScopeDirs, PluginError> {
        if let Some(dirs) = &self.dirs {
            return Ok(dirs.clone());
        }
        let base = std::env::temp_dir().join(format!(
            "ilium-plugin-host-fs-{}-{}",
            std::process::id(),
            unique_suffix(self.env.ids())
        ));
        let dirs = ScopeDirs {
            package: base.join(SCOPE_PACKAGE),
            data: base.join(SCOPE_DATA),
            cache: base.join(SCOPE_CACHE),
        };
        for folder in [&dirs.package, &dirs.data, &dirs.cache] {
            fs::create_dir_all(folder).map_err(io_error)?;
        }
        let canonical_dirs = ScopeDirs {
            package: fs::canonicalize(&dirs.package).map_err(io_error)?,
            data: fs::canonicalize(&dirs.data).map_err(io_error)?,
            cache: fs::canonicalize(&dirs.cache).map_err(io_error)?,
        };
        self.owned_dir = Some(base);
        self.dirs = Some(canonical_dirs.clone());
        Ok(canonical_dirs)
    }
}

/// A scope-relative path resolved to a real location.
#[derive(Debug)]
struct Resolved {
    target: PathBuf,
    scope: String,
    writable: bool,
    is_scope_root: bool,
    /// Scope-qualified, normalised path used as the advisory lock key.
    key: String,
}

impl Resolved {
    fn require_writable(&self) -> Result<(), PluginError> {
        if self.writable {
            Ok(())
        } else {
            Err(plugin_error(
                ErrorCode::NotPermitted,
                format!("scope {} is read only", self.scope),
            ))
        }
    }
}

/// A name that differs between two calls even across runtimes of one process.
fn unique_suffix(ids: &RequestIds) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    format!("{nanos:x}-{}", ids.next_call())
}

fn resolve_in(state: &mut FsState, path: &FsPath, access: Access) -> Result<Resolved, PluginError> {
    let (root, writable) = {
        let dirs = state.scope_dirs()?;
        match path.scope.as_str() {
            SCOPE_PACKAGE => Ok((dirs.package, false)),
            SCOPE_DATA => Ok((dirs.data, true)),
            SCOPE_CACHE => Ok((dirs.cache, true)),
            SCOPE_PROJECT => match &state.project {
                Some(project) if project.allows(access) => {
                    Ok((project.root.clone(), project.granted("fs.write")))
                }
                _ => Err(plugin_error(
                    ErrorCode::NotPermitted,
                    "scope project is not granted",
                )),
            },
            other => extra_scope_root(state, other, access),
        }
    }?;
    let picked_target = path.scope.starts_with(SCOPE_PICKED_PREFIX);
    if path.path.contains('\0') {
        return Err(plugin_error(
            ErrorCode::InvalidArgument,
            "path contains a NUL character",
        ));
    }
    if path.path.starts_with('/') || path.path.contains('\\') {
        return Err(plugin_error(
            ErrorCode::NotPermitted,
            "path must be relative to its scope",
        ));
    }
    let mut target = root.clone();
    let mut normalised: Vec<&str> = Vec::new();
    for component in path.path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                return Err(plugin_error(
                    ErrorCode::NotPermitted,
                    "path leaves its scope",
                ))
            }
            name => {
                target.push(name);
                normalised.push(name);
            }
        }
    }
    // A symlink inside the scope must not lead out of it: check the deepest existing ancestor.
    if path.scope == SCOPE_PROJECT && normalised.first() == Some(&PROJECT_HIDDEN_FOLDER) {
        return Err(plugin_error(
            ErrorCode::NotPermitted,
            "the project's .ilium folder is hidden",
        ));
    }
    // A picked save path may not exist yet: the user chose that very path, so it is its own
    // scope and needs no ancestor check.
    let chosen_path_itself = picked_target && normalised.is_empty();
    if !chosen_path_itself {
        let mut probe = target.clone();
        while fs::symlink_metadata(&probe).is_err() {
            if !probe.pop() {
                break;
            }
        }
        let canonical_probe = fs::canonicalize(&probe).map_err(io_error)?;
        if !canonical_probe.starts_with(&root) {
            return Err(plugin_error(
                ErrorCode::NotPermitted,
                "path leaves its scope through a link",
            ));
        }
    }
    Ok(Resolved {
        key: format!("{}:{}", path.scope, normalised.join("/")),
        is_scope_root: normalised.is_empty(),
        target,
        scope: path.scope.clone(),
        writable,
    })
}

/// The grant family an access needs on a granted scope.
fn access_family(access: Access) -> &'static [&'static str] {
    match access {
        Access::Read => &["fs.read"],
        Access::Write => &["fs.write"],
        Access::Watch => &["fs.watch"],
        Access::Lock => &["fs.read", "fs.write"],
    }
}

/// The root and writability of a `picked/<n>` or read-only named scope.
fn extra_scope_root(
    state: &FsState,
    scope: &str,
    access: Access,
) -> Result<(PathBuf, bool), PluginError> {
    let refused = || {
        plugin_error(
            ErrorCode::NotPermitted,
            format!("scope {scope} is not granted"),
        )
    };
    if let Some(index) = scope.strip_prefix(SCOPE_PICKED_PREFIX) {
        let granted = access_family(access)
            .iter()
            .all(|family| state.env.holds(&format!("{family}:picked")));
        let chosen = index.parse::<usize>().ok().and_then(|index| {
            state
                .picks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .chosen
                .get(index)
                .cloned()
        });
        return match chosen {
            Some(root) if granted => Ok((root, state.env.holds("fs.write:picked"))),
            _ => Err(refused()),
        };
    }
    let Some(root) = state.read_only.get(scope) else {
        return Err(refused());
    };
    let granted = match access {
        Access::Read => state.env.holds(&format!("fs.read:{scope}")),
        // The agent configuration folders are never watched (api-fs "Scopes").
        Access::Watch => {
            !scope.starts_with("agent-config:") && state.env.holds(&format!("fs.watch:{scope}"))
        }
        Access::Write | Access::Lock => false,
    };
    if granted {
        Ok((root.clone(), false))
    } else {
        Err(refused())
    }
}

fn kind_of(file_type: fs::FileType) -> EntryKind {
    if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

fn metadata_of(location: &Path, scope_writable: bool) -> Result<FsMetadata, PluginError> {
    let raw = fs::symlink_metadata(location).map_err(io_error)?;
    let modified_unix_millis = raw
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok());
    Ok(FsMetadata {
        kind: kind_of(raw.file_type()),
        length: raw.len(),
        modified_unix_millis,
        read_only: !scope_writable || raw.permissions().readonly(),
    })
}

fn revision_of(bytes: &[u8]) -> RevisionToken {
    let digest = Sha256::digest(bytes);
    let mut token = String::from("sha256-");
    for byte in digest {
        token.push_str(&format!("{byte:02x}"));
    }
    token
}

fn check_write_size(length: usize) -> Result<(), PluginError> {
    if length > WRITE_MAX_BYTES {
        return Err(plugin_error(
            ErrorCode::InvalidArgument,
            format!("payload of {length} bytes is over the {WRITE_MAX_BYTES} byte write cap"),
        ));
    }
    Ok(())
}

fn require_parent_exists(target: &Path) -> Result<(), PluginError> {
    match target.parent() {
        Some(parent) if !parent.is_dir() => Err(plugin_error(
            ErrorCode::NotFound,
            "parent folder does not exist",
        )),
        _ => Ok(()),
    }
}

/// Writes `data` to a sibling temporary file and renames it over `target` (atomic replace).
fn replace_atomically(target: &Path, data: &[u8], ids: &RequestIds) -> Result<(), PluginError> {
    require_parent_exists(target)?;
    let unique = unique_suffix(ids);
    let file_name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = target.with_file_name(format!(".{file_name}.tmp-{unique}"));
    let outcome = fs::write(&temporary, data).and_then(|()| fs::rename(&temporary, target));
    if outcome.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    outcome.map_err(io_error)
}

fn read_bounded(location: &Path, offset: u64, max_bytes: u32) -> Result<Vec<u8>, PluginError> {
    let mut file = fs::File::open(location).map_err(io_error)?;
    if file.metadata().map_err(io_error)?.is_dir() {
        return Err(plugin_error(
            ErrorCode::InvalidArgument,
            "path is a folder, not a file",
        ));
    }
    file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
    let mut buffer = Vec::new();
    file.take(u64::from(max_bytes))
        .read_to_end(&mut buffer)
        .map_err(io_error)?;
    Ok(buffer)
}

fn read_whole_file(location: &Path) -> Result<Vec<u8>, PluginError> {
    let length = fs::metadata(location).map_err(io_error)?.len();
    if length > READ_ALL_MAX_BYTES {
        return Err(plugin_error(
            ErrorCode::QuotaExceeded,
            format!("file of {length} bytes is over the {READ_ALL_MAX_BYTES} byte read cap"),
        ));
    }
    fs::read(location).map_err(io_error)
}

/// Translates a scope-root glob into a regular expression. `*` stops at `/`, `**` does not.
fn glob_to_regex(glob: &str) -> Result<Regex, PluginError> {
    let invalid =
        |reason: &str| plugin_error(ErrorCode::InvalidArgument, format!("glob: {reason}"));
    if glob.is_empty() {
        return Err(invalid("pattern is empty"));
    }
    let characters: Vec<char> = glob.chars().collect();
    let mut pattern = String::from("^(?:");
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        match character {
            '*' if characters.get(index + 1) == Some(&'*') => {
                index += 1;
                if characters.get(index + 1) == Some(&'/') {
                    index += 1;
                    pattern.push_str("(?:.*/)?");
                } else {
                    pattern.push_str(".*");
                }
            }
            '*' => pattern.push_str("[^/]*"),
            '?' => pattern.push_str("[^/]"),
            '[' => {
                let mut class = String::from("[");
                index += 1;
                if matches!(characters.get(index), Some('!') | Some('^')) {
                    class.push('^');
                    index += 1;
                }
                let class_start = class.len();
                let mut closed = false;
                while index < characters.len() {
                    let inner = characters[index];
                    if inner == ']' && class.len() > class_start {
                        closed = true;
                        break;
                    }
                    if matches!(inner, '\\' | '[' | '&' | '~' | '^') {
                        class.push('\\');
                    }
                    class.push(inner);
                    index += 1;
                }
                if !closed {
                    return Err(invalid("character class is not closed"));
                }
                class.push(']');
                pattern.push_str(&class);
            }
            other => pattern.push_str(&regex::escape(&other.to_string())),
        }
        index += 1;
    }
    pattern.push_str(")$");
    Regex::new(&pattern).map_err(|error| invalid(&error.to_string()))
}

/// Collects entries below `folder` depth first in name order, stopping at `max_entries`.
/// How one `scan` walk matches and reports entries.
#[derive(Debug, Clone, Copy)]
struct WalkOptions {
    match_name_only: bool,
    scope_writable: bool,
    max_entries: usize,
    /// Skip the project's hidden `.ilium` folder at the start folder.
    hide_project_folder: bool,
}

fn walk_matching(
    folder: &Path,
    relative_prefix: &str,
    matcher: &Regex,
    options: WalkOptions,
    found: &mut Vec<ScannedEntry>,
) -> Result<(), PluginError> {
    let WalkOptions {
        match_name_only,
        scope_writable,
        max_entries,
        hide_project_folder,
    } = options;
    let mut children: Vec<fs::DirEntry> = fs::read_dir(folder)
        .map_err(io_error)?
        .filter_map(Result::ok)
        .collect();
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        if found.len() >= max_entries {
            return Ok(());
        }
        let name = child.file_name().to_string_lossy().into_owned();
        if hide_project_folder && relative_prefix.is_empty() && name == PROJECT_HIDDEN_FOLDER {
            continue;
        }
        let relative_path = if relative_prefix.is_empty() {
            name.clone()
        } else {
            format!("{relative_prefix}/{name}")
        };
        let metadata = match metadata_of(&child.path(), scope_writable) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let subject = if match_name_only {
            &name
        } else {
            &relative_path
        };
        if matcher.is_match(subject) {
            found.push(ScannedEntry {
                relative_path: relative_path.clone(),
                entry: DirEntry {
                    name: name.clone(),
                    metadata: metadata.clone(),
                },
            });
        }
        if metadata.kind == EntryKind::Directory {
            walk_matching(&child.path(), &relative_path, matcher, options, found)?;
        }
    }
    Ok(())
}

/// The canonical root folder of a scope, for the relative paths of `fs-changed`.
fn scope_root_of(state: &mut FsState, scope: &str) -> Result<PathBuf, PluginError> {
    let dirs = state.scope_dirs()?;
    match scope {
        SCOPE_PACKAGE => Ok(dirs.package),
        SCOPE_DATA => Ok(dirs.data),
        SCOPE_CACHE => Ok(dirs.cache),
        SCOPE_PROJECT => state
            .project
            .as_ref()
            .map(|project| project.root.clone())
            .ok_or_else(|| plugin_error(ErrorCode::NotPermitted, "scope project is not granted")),
        other => extra_scope_root(state, other, Access::Watch)
            .map(|(root, _)| root)
            .and_then(|root| fs::canonicalize(&root).map_err(io_error)),
    }
}

/// What a watch remembers of one entry to notice a change.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryStamp {
    kind: u8,
    length: u64,
    modified: Option<SystemTime>,
}

/// The entries a watch looks at, by path relative to the scope root. Links are stamped by
/// themselves and never followed; the project's hidden folder never appears.
fn snapshot_of(
    target: &Path,
    scope_root: &Path,
    recursive: bool,
) -> Option<BTreeMap<String, EntryStamp>> {
    fs::symlink_metadata(target).ok()?;
    let mut found = BTreeMap::new();
    let mut pending = vec![target.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(children) = fs::read_dir(&folder) else {
            continue;
        };
        for child in children.filter_map(Result::ok) {
            let location = child.path();
            let Ok(relative) = location.strip_prefix(scope_root) else {
                continue;
            };
            let relative_text = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if relative_text == PROJECT_HIDDEN_FOLDER
                || relative_text.starts_with(&format!("{PROJECT_HIDDEN_FOLDER}/"))
            {
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(&location) else {
                continue;
            };
            found.insert(
                relative_text,
                EntryStamp {
                    kind: kind_of(metadata.file_type()) as u8,
                    length: metadata.len(),
                    modified: metadata.modified().ok(),
                },
            );
            if recursive && metadata.is_dir() {
                pending.push(location);
            }
        }
    }
    Some(found)
}

/// The paths whose stamp differs between two snapshots (added, removed or modified).
fn changed_paths(
    before: &BTreeMap<String, EntryStamp>,
    after: &BTreeMap<String, EntryStamp>,
) -> Vec<String> {
    let mut changed: Vec<String> = after
        .iter()
        .filter(|(path, stamp)| before.get(*path) != Some(*stamp))
        .map(|(path, _)| path.clone())
        .collect();
    changed.extend(
        before
            .keys()
            .filter(|path| !after.contains_key(*path))
            .cloned(),
    );
    changed.sort();
    changed.dedup();
    changed
}

/// Polls one watch until `stop` is set or the watched folder disappears (api-fs watch rules:
/// 200 ms window, at most five events per second, 1,024 paths per event).
fn run_watch(
    target: &Path,
    scope_root: &Path,
    recursive: bool,
    watch: WatchId,
    sink: &EventSink,
    stop: &AtomicBool,
) {
    let watched_path = target
        .strip_prefix(scope_root)
        .map(|relative| {
            relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let Some(mut baseline) = snapshot_of(target, scope_root, recursive) else {
        return;
    };
    let mut last_event: Option<Instant> = None;
    let mut window_opened: Option<Instant> = None;
    let mut accumulated: Vec<String> = Vec::new();
    while !stop.load(Ordering::Acquire) {
        std::thread::sleep(WATCH_POLL_INTERVAL);
        let Some(current) = snapshot_of(target, scope_root, recursive) else {
            sink.send(Event::FsChanged(FsChangedEvent {
                watch,
                paths: vec![watched_path],
            }));
            return;
        };
        let changed = changed_paths(&baseline, &current);
        if !changed.is_empty() {
            window_opened.get_or_insert_with(Instant::now);
            accumulated.extend(changed);
            baseline = current;
        }
        let due = window_opened.is_some_and(|opened| {
            opened.elapsed() >= WATCH_WINDOW
                && last_event.is_none_or(|sent| sent.elapsed() >= WATCH_MIN_EVENT_GAP)
        });
        if due {
            accumulated.sort();
            accumulated.dedup();
            let paths = if accumulated.len() > WATCH_MAX_PATHS {
                vec![watched_path.clone()]
            } else {
                std::mem::take(&mut accumulated)
            };
            accumulated.clear();
            sink.send(Event::FsChanged(FsChangedEvent { watch, paths }));
            last_event = Some(Instant::now());
            window_opened = None;
        }
    }
}

impl Drop for FsState {
    fn drop(&mut self) {
        // Stop the watch workers before the scratch folder they poll disappears.
        self.watches.clear();
        if let Some(owned_dir) = self.owned_dir.take() {
            let _ = fs::remove_dir_all(owned_dir);
        }
    }
}

impl HostFs for FsImport {
    fn roots(&mut self) -> Vec<ScopeRoot> {
        let project = self.with_state(|state| {
            state.project.as_ref().and_then(|project| {
                let readable = project.allows(Access::Read);
                let writable = project.allows(Access::Write);
                (readable || writable).then(|| ScopeRoot {
                    scope: SCOPE_PROJECT.to_owned(),
                    writable,
                })
            })
        });
        let mut roots = vec![
            ScopeRoot {
                scope: SCOPE_PACKAGE.to_owned(),
                writable: false,
            },
            ScopeRoot {
                scope: SCOPE_DATA.to_owned(),
                writable: true,
            },
            ScopeRoot {
                scope: SCOPE_CACHE.to_owned(),
                writable: true,
            },
        ];
        roots.extend(project);
        let read_only: Vec<ScopeRoot> = self.with_state(|state| {
            state
                .read_only
                .keys()
                .filter(|scope| state.env.holds(&format!("fs.read:{scope}")))
                .map(|scope| ScopeRoot {
                    scope: scope.clone(),
                    writable: false,
                })
                .collect()
        });
        roots.extend(read_only);
        roots
    }

    fn metadata(&mut self, path: FsPath) -> Result<FsMetadata, PluginError> {
        let resolved = self.resolve(&path, Access::Read)?;
        metadata_of(&resolved.target, resolved.writable)
    }

    fn list_dir(&mut self, path: FsPath, max_entries: u32) -> Result<Vec<DirEntry>, PluginError> {
        if max_entries > LIST_DIR_MAX_ENTRIES {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                format!("max-entries is at most {LIST_DIR_MAX_ENTRIES}"),
            ));
        }
        let resolved = self.resolve(&path, Access::Read)?;
        let mut children: Vec<fs::DirEntry> = fs::read_dir(&resolved.target)
            .map_err(|error| {
                if resolved.target.is_file() {
                    plugin_error(ErrorCode::InvalidArgument, "path is a file, not a folder")
                } else {
                    io_error(error)
                }
            })?
            .filter_map(Result::ok)
            .collect();
        children.sort_by_key(fs::DirEntry::file_name);
        let mut listing = Vec::new();
        let hide_project_folder = resolved.scope == SCOPE_PROJECT && resolved.is_scope_root;
        children
            .retain(|child| !(hide_project_folder && child.file_name() == PROJECT_HIDDEN_FOLDER));
        for child in children.into_iter().take(max_entries as usize) {
            if let Ok(metadata) = metadata_of(&child.path(), resolved.writable) {
                listing.push(DirEntry {
                    name: child.file_name().to_string_lossy().into_owned(),
                    metadata,
                });
            }
        }
        Ok(listing)
    }

    fn read(&mut self, path: FsPath, offset: u64, max_bytes: u32) -> Result<Vec<u8>, PluginError> {
        if max_bytes > SYNC_READ_MAX_BYTES {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                format!("max-bytes is at most {SYNC_READ_MAX_BYTES}"),
            ));
        }
        let resolved = self.resolve(&path, Access::Read)?;
        read_bounded(&resolved.target, offset, max_bytes)
    }

    fn read_all(&mut self, path: FsPath) -> Result<RequestId, PluginError> {
        self.with_state(|state| state.require_capacity())?;
        let resolved = self.resolve(&path, Access::Read)?;
        let content = read_whole_file(&resolved.target)?;
        Ok(self.with_state(|state| {
            let id = state.allocate_request();
            state.bytes.insert(id, content);
            state.finish(id);
            RequestId(id)
        }))
    }

    fn scan(
        &mut self,
        path: FsPath,
        glob: String,
        max_entries: u32,
    ) -> Result<RequestId, PluginError> {
        self.with_state(|state| state.require_capacity())?;
        if max_entries > SCAN_MAX_ENTRIES {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                format!("max-entries is at most {SCAN_MAX_ENTRIES}"),
            ));
        }
        let matcher = glob_to_regex(&glob)?;
        let resolved = self.resolve(&path, Access::Read)?;
        if !resolved.target.is_dir() {
            return Err(plugin_error(
                if resolved.target.exists() {
                    ErrorCode::InvalidArgument
                } else {
                    ErrorCode::NotFound
                },
                "scan needs an existing folder",
            ));
        }
        let mut found = Vec::new();
        walk_matching(
            &resolved.target,
            "",
            &matcher,
            WalkOptions {
                match_name_only: !glob.contains('/'),
                scope_writable: resolved.writable,
                max_entries: max_entries as usize,
                hide_project_folder: resolved.scope == SCOPE_PROJECT && resolved.is_scope_root,
            },
            &mut found,
        )?;
        Ok(self.with_state(|state| {
            let id = state.allocate_request();
            state.entries.insert(id, found);
            state.finish(id);
            RequestId(id)
        }))
    }

    fn write(
        &mut self,
        path: FsPath,
        data: Vec<u8>,
        mode: WriteMode,
    ) -> Result<RequestId, PluginError> {
        self.with_state(|state| state.require_capacity())?;
        let resolved = self.resolve(&path, Access::Write)?;
        check_write_size(data.len())?;
        resolved.require_writable()?;
        if resolved.is_scope_root {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                "a scope root is a folder, not a file",
            ));
        }
        require_parent_exists(&resolved.target)?;
        match mode {
            WriteMode::CreateNew => {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&resolved.target)
                    .map_err(io_error)?;
                file.write_all(&data).map_err(io_error)?;
            }
            WriteMode::Replace => replace_atomically(&resolved.target, &data, self.env.ids())?,
            WriteMode::Append => {
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&resolved.target)
                    .map_err(io_error)?;
                file.write_all(&data).map_err(io_error)?;
            }
        }
        Ok(self.with_state(|state| RequestId(state.complete_new())))
    }

    fn create_dir(&mut self, path: FsPath) -> Result<(), PluginError> {
        let resolved = self.resolve(&path, Access::Write)?;
        resolved.require_writable()?;
        if resolved.target.is_dir() {
            return Ok(());
        }
        require_parent_exists(&resolved.target)?;
        fs::create_dir(&resolved.target).map_err(io_error)
    }

    fn remove(&mut self, path: FsPath) -> Result<RequestId, PluginError> {
        self.with_state(|state| state.require_capacity())?;
        let resolved = self.resolve(&path, Access::Write)?;
        if resolved.is_scope_root {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                "a scope root cannot be removed",
            ));
        }
        resolved.require_writable()?;
        let metadata = fs::symlink_metadata(&resolved.target).map_err(io_error)?;
        if metadata.is_dir() {
            fs::remove_dir_all(&resolved.target).map_err(io_error)?;
        } else {
            fs::remove_file(&resolved.target).map_err(io_error)?;
        }
        Ok(self.with_state(|state| RequestId(state.complete_new())))
    }

    fn rename(&mut self, source: FsPath, target: FsPath) -> Result<RequestId, PluginError> {
        self.with_state(|state| state.require_capacity())?;
        if source.scope != target.scope {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                "rename stays inside one scope",
            ));
        }
        let resolved_source = self.resolve(&source, Access::Write)?;
        let resolved_target = self.resolve(&target, Access::Write)?;
        if resolved_source.is_scope_root || resolved_target.is_scope_root {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                "a scope root cannot be renamed or replaced",
            ));
        }
        resolved_source.require_writable()?;
        fs::symlink_metadata(&resolved_source.target).map_err(io_error)?;
        require_parent_exists(&resolved_target.target)?;
        if fs::symlink_metadata(&resolved_target.target).is_ok() {
            return Err(plugin_error(ErrorCode::Conflict, "target already exists"));
        }
        fs::rename(&resolved_source.target, &resolved_target.target).map_err(io_error)?;
        Ok(self.with_state(|state| RequestId(state.complete_new())))
    }

    fn watch(&mut self, path: FsPath, recursive: bool) -> Result<WatchId, PluginError> {
        if path.scope == SCOPE_PACKAGE {
            return Err(plugin_error(
                ErrorCode::NotPermitted,
                "the package scope cannot be watched",
            ));
        }
        let resolved = self.resolve(&path, Access::Watch)?;
        fs::symlink_metadata(&resolved.target).map_err(io_error)?;
        if self.state.watches.len() >= WATCHES_PER_PLUGIN {
            return Err(plugin_error(
                ErrorCode::QuotaExceeded,
                "a plugin may hold 64 watches",
            ));
        }
        let scope_root = scope_root_of(&mut self.state, &path.scope)?;
        let id = self.state.allocate_id();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let sink = self.env.sink();
        let guard = sink.worker();
        let target = resolved.target.clone();
        let worker = std::thread::spawn(move || {
            let _guard = guard;
            run_watch(
                &target,
                &scope_root,
                recursive,
                WatchId(id),
                &sink,
                &worker_stop,
            );
        });
        self.state.watches.insert(
            id,
            WatchEntry {
                stop,
                worker: Some(worker),
            },
        );
        Ok(WatchId(id))
    }

    fn unwatch(&mut self, watch: WatchId) {
        // Dropping the entry stops and joins its worker; an unknown id is ignored.
        self.state.watches.remove(&watch.0);
    }

    fn lock(
        &mut self,
        path: FsPath,
        exclusive: bool,
        timeout_millis: u32,
    ) -> Result<RequestId, PluginError> {
        self.with_state(|state| state.require_capacity())?;
        if timeout_millis > LOCK_MAX_TIMEOUT_MILLIS {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                format!("timeout-millis is at most {LOCK_MAX_TIMEOUT_MILLIS}"),
            ));
        }
        let resolved = self.resolve(&path, Access::Lock)?;
        resolved.require_writable()?;
        self.with_state(|state| {
            let conflicts = state.held_locks.values().any(|(held_key, held_exclusive)| {
                *held_key == resolved.key && (exclusive || *held_exclusive)
            });
            if conflicts {
                // No other holder can release while this call runs, so waiting would only expire.
                if timeout_millis == 0 {
                    return Err(plugin_error(
                        ErrorCode::Conflict,
                        "path is locked by another holder",
                    ));
                }
                let request_id = state.allocate_request();
                let error = plugin_error(
                    ErrorCode::TimedOut,
                    "the lock was still held at the timeout",
                );
                state.finish_with(request_id, Outcome::Failed(error));
                return Ok(RequestId(request_id));
            }
            let lock_id = state.allocate_id();
            state.held_locks.insert(lock_id, (resolved.key, exclusive));
            let request_id = state.allocate_request();
            state.lock_answers.insert(request_id, LockId(lock_id));
            state.finish(request_id);
            Ok(RequestId(request_id))
        })
    }

    fn unlock(&mut self, lock: LockId) {
        self.with_state(|state| {
            state.held_locks.remove(&lock.0);
        });
    }

    fn pick(&mut self, spec: PickSpec) -> Result<RequestId, PluginError> {
        if !self.env.holds("fs.pick") {
            return Err(plugin_error(
                ErrorCode::NotPermitted,
                "the fs.pick grant is not held",
            ));
        }
        if !self.env.holds("fs.read:picked") && !self.env.holds("fs.write:picked") {
            return Err(plugin_error(
                ErrorCode::NotPermitted,
                "fs.pick needs fs.read:picked or fs.write:picked",
            ));
        }
        if spec.kind == PickKind::SaveFile && !self.env.holds("fs.write:picked") {
            return Err(plugin_error(
                ErrorCode::NotPermitted,
                "a save-file pick needs fs.write:picked",
            ));
        }
        let Some(picker) = self.picker.clone() else {
            return Err(plugin_error(
                ErrorCode::Unsupported,
                "no file picker is available on this side",
            ));
        };
        let id = self.env.next_request();
        let picks = Arc::clone(&self.state.picks);
        let sink = self.env.sink();
        let guard = sink.worker();
        let cancel = self.life.flag();
        let spawned = std::thread::Builder::new()
            .name("plugin-fs-pick".to_owned())
            .spawn(move || {
                let _guard = guard;
                let answer = picker.pick(&spec, &cancel);
                if cancel.is_set() {
                    return;
                }
                let outcome = {
                    let mut book = picks
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    match answer {
                        Ok(Some(chosen)) => {
                            let scope = format!("{SCOPE_PICKED_PREFIX}{}", book.chosen.len());
                            book.chosen.push(chosen);
                            book.answers.insert(
                                id.0,
                                Some(FsPath {
                                    scope,
                                    path: String::new(),
                                }),
                            );
                            Outcome::Ok
                        }
                        Ok(None) => {
                            book.answers.insert(id.0, None);
                            Outcome::Ok
                        }
                        Err(failure) => Outcome::Failed(failure),
                    }
                };
                sink.send(completion_event(id, outcome));
            });
        if spawned.is_err() {
            return Err(plugin_error(
                ErrorCode::Internal,
                "the picker worker did not start",
            ));
        }
        Ok(id)
    }

    fn read_with_revision(&mut self, path: FsPath) -> Result<RequestId, PluginError> {
        self.with_state(|state| state.require_capacity())?;
        let resolved = self.resolve(&path, Access::Read)?;
        let bytes = read_whole_file(&resolved.target)?;
        let revision = revision_of(&bytes);
        Ok(self.with_state(|state| {
            let id = state.allocate_request();
            state.revisioned.insert(id, Revisioned { bytes, revision });
            state.finish(id);
            RequestId(id)
        }))
    }

    fn write_atomic(
        &mut self,
        path: FsPath,
        data: Vec<u8>,
        expected: Option<RevisionToken>,
    ) -> Result<RequestId, PluginError> {
        self.with_state(|state| state.require_capacity())?;
        let resolved = self.resolve(&path, Access::Write)?;
        check_write_size(data.len())?;
        resolved.require_writable()?;
        if resolved.is_scope_root {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                "a scope root is a folder, not a file",
            ));
        }
        match expected {
            None => {
                require_parent_exists(&resolved.target)?;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&resolved.target)
                    .map_err(io_error)?;
                file.write_all(&data).map_err(io_error)?;
            }
            Some(token) => {
                let current = match fs::read(&resolved.target) {
                    Ok(current) => current,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Err(plugin_error(ErrorCode::Conflict, "file no longer exists"))
                    }
                    Err(error) => return Err(io_error(error)),
                };
                if revision_of(&current) != token {
                    return Err(plugin_error(
                        ErrorCode::Conflict,
                        "file changed since it was read",
                    ));
                }
                replace_atomically(&resolved.target, &data, self.env.ids())?;
            }
        }
        let revision = revision_of(&data);
        Ok(self.with_state(|state| {
            let id = state.allocate_request();
            state.written.insert(id, revision);
            state.finish(id);
            RequestId(id)
        }))
    }

    fn tail(
        &mut self,
        path: FsPath,
        cursor: u64,
        max_bytes: u32,
    ) -> Result<TailChunk, PluginError> {
        if max_bytes > SYNC_READ_MAX_BYTES {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                format!("max-bytes is at most {SYNC_READ_MAX_BYTES}"),
            ));
        }
        let resolved = self.resolve(&path, Access::Read)?;
        let length = fs::metadata(&resolved.target).map_err(io_error)?.len();
        // A cursor past the end means the file was truncated or rotated: restart from the top.
        let truncated = cursor > length;
        let start = if truncated { 0 } else { cursor };
        let bytes = read_bounded(&resolved.target, start, max_bytes)?;
        Ok(TailChunk {
            cursor: start + bytes.len() as u64,
            bytes,
            truncated,
        })
    }

    fn propose_agent_config(
        &mut self,
        _agent: String,
        _title: String,
        _changes: Vec<ConfigChange>,
    ) -> Result<RequestId, PluginError> {
        Err(plugin_error(
            ErrorCode::NotPermitted,
            "the agent-config.propose grant is not held",
        ))
    }

    fn take_proposal(&mut self, _request: RequestId) -> Option<ProposalResult> {
        None
    }

    fn take_bytes(&mut self, request: RequestId) -> Option<Vec<u8>> {
        self.with_state(|state| state.bytes.remove(&request.0))
    }

    fn take_entries(&mut self, request: RequestId) -> Option<Vec<ScannedEntry>> {
        self.with_state(|state| state.entries.remove(&request.0))
    }

    fn take_lock(&mut self, request: RequestId) -> Option<LockId> {
        self.with_state(|state| state.lock_answers.remove(&request.0))
    }

    fn take_picked(&mut self, request: RequestId) -> Option<FsPath> {
        self.state
            .picks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .answers
            .remove(&request.0)
            .flatten()
    }

    fn take_revisioned(&mut self, request: RequestId) -> Option<Revisioned> {
        self.with_state(|state| state.revisioned.remove(&request.0))
    }

    fn take_written(&mut self, request: RequestId) -> Option<RevisionToken> {
        self.with_state(|state| state.written.remove(&request.0))
    }
}

#[cfg(test)]
mod tests {
    use super::{FilePicker, FsImport};
    use crate::imports::env::{CancelFlag, ImportEnv};
    use crate::runtime::pending_events::PendingEvents;
    use ilium_plugin_api::generated::host::fs::{HostFs, PickKind, PickSpec, WriteMode};
    use ilium_plugin_api::generated::types::events::Event;
    use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// Confirms a fixed path, or dismisses the picker when it has none.
    struct FixedPicker(Option<PathBuf>);

    impl FilePicker for FixedPicker {
        fn pick(
            &self,
            _spec: &PickSpec,
            _cancel: &CancelFlag,
        ) -> Result<Option<PathBuf>, PluginError> {
            Ok(self.0.clone())
        }
    }

    fn picking_import(granted: &[&str], picked: Option<PathBuf>) -> (FsImport, PendingEvents) {
        let events = PendingEvents::new();
        let mut fs_import = FsImport::new(
            ImportEnv::unrestricted()
                .with_events(events.clone())
                .with_grants(granted.iter().map(|grant| (*grant).to_owned()).collect()),
        );
        fs_import.set_file_picker(Arc::new(FixedPicker(picked)));
        (fs_import, events)
    }

    fn wait_for_completion(
        events: &PendingEvents,
        request: ilium_plugin_api::generated::types::ids::RequestId,
    ) -> Outcome {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            for queued in events.drain() {
                if let Event::RequestCompleted(result) = queued.event {
                    if result.request == RequestRef::Call(request) {
                        return result.outcome;
                    }
                }
            }
            assert!(Instant::now() < deadline, "no completion for {request:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn open_file_spec() -> PickSpec {
        PickSpec {
            kind: PickKind::OpenFile,
            title: "Choose a file".to_owned(),
            extensions: vec!["md".to_owned()],
        }
    }

    #[test]
    fn a_confirmed_pick_becomes_a_readable_picked_scope() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let chosen = scratch.path().join("notes.md");
        std::fs::write(&chosen, b"picked bytes").expect("chosen file");
        let (mut fs_import, events) = picking_import(&["fs.pick", "fs.read:picked"], Some(chosen));
        let request = fs_import.pick(open_file_spec()).expect("pick accepted");
        assert_eq!(wait_for_completion(&events, request), Outcome::Ok);
        let picked = fs_import.take_picked(request).expect("chosen path");
        assert_eq!(picked.scope, "picked/0");
        assert!(picked.path.is_empty());
        assert_eq!(fs_import.take_picked(request), None);
        let read = fs_import.read_all(picked.clone()).expect("read accepted");
        assert_eq!(fs_import.take_bytes(read), Some(b"picked bytes".to_vec()));
        assert_eq!(
            error_code_of(fs_import.write(picked, b"x".to_vec(), WriteMode::Replace)),
            Some(ErrorCode::NotPermitted),
            "fs.read:picked alone does not allow writing"
        );
    }

    #[test]
    fn a_dismissed_pick_completes_with_no_path() {
        let (mut fs_import, events) = picking_import(&["fs.pick", "fs.read:picked"], None);
        let request = fs_import.pick(open_file_spec()).expect("pick accepted");
        assert_eq!(wait_for_completion(&events, request), Outcome::Ok);
        assert_eq!(fs_import.take_picked(request), None);
    }

    #[test]
    fn pick_needs_a_picked_scope_grant_and_a_picker() {
        let (mut no_scope, _) = picking_import(&["fs.pick"], None);
        assert_eq!(
            error_code_of(no_scope.pick(open_file_spec())),
            Some(ErrorCode::NotPermitted)
        );
        let mut no_picker = FsImport::new(
            ImportEnv::unrestricted()
                .with_grants(vec!["fs.pick".to_owned(), "fs.read:picked".to_owned()]),
        );
        assert_eq!(
            error_code_of(no_picker.pick(open_file_spec())),
            Some(ErrorCode::Unsupported)
        );
    }

    #[test]
    fn a_read_only_agent_scope_opens_on_its_read_grant_only() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        std::fs::write(scratch.path().join("session.jsonl"), b"{}\n").expect("transcript");
        let granted = |grants: &[&str]| {
            let mut fs_import = FsImport::new(
                ImportEnv::unrestricted()
                    .with_grants(grants.iter().map(|grant| (*grant).to_owned()).collect()),
            );
            fs_import
                .configure_read_only_scope("agent-sessions:claude", scratch.path())
                .expect("scope root");
            fs_import
        };
        let mut reader = granted(&["fs.read:agent-sessions:claude"]);
        assert!(reader
            .roots()
            .iter()
            .any(|root| root.scope == "agent-sessions:claude" && !root.writable));
        let read = reader
            .read_all(fs_path("agent-sessions:claude", "session.jsonl"))
            .expect("read accepted");
        assert_eq!(reader.take_bytes(read), Some(b"{}\n".to_vec()));
        assert_eq!(
            error_code_of(reader.write(
                fs_path("agent-sessions:claude", "session.jsonl"),
                b"x".to_vec(),
                WriteMode::Replace
            )),
            Some(ErrorCode::NotPermitted)
        );
        let mut stranger = granted(&["fs.read:agent-sessions:codex"]);
        assert_eq!(
            error_code_of(stranger.read_all(fs_path("agent-sessions:claude", "session.jsonl"))),
            Some(ErrorCode::NotPermitted)
        );
        assert!(stranger
            .roots()
            .iter()
            .all(|root| root.scope != "agent-sessions:claude"));
    }
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::paths::FsPath;

    /// Largest `max_bytes` a synchronous `read` or `tail` accepts (1 MiB, fixed cap).
    const ONE_MIB: u32 = 1024 * 1024;
    /// Largest `max_entries` for `list_dir` (fixed cap).
    const LIST_DIR_MAX_ENTRIES: u32 = 4_096;
    /// Largest `max_entries` for `scan` (fixed cap).
    const SCAN_MAX_ENTRIES: u32 = 65_536;
    /// Largest payload of one `write` (`fs_write_mib`, 16 MiB by default).
    const WRITE_MAX_BYTES: usize = 16 * 1024 * 1024;

    fn fs_path(scope: &str, path: &str) -> FsPath {
        FsPath {
            scope: scope.to_owned(),
            path: path.to_owned(),
        }
    }

    fn error_code_of<T>(result: Result<T, PluginError>) -> Option<ErrorCode> {
        result.err().map(|error| error.code)
    }

    /// REQ: CAP-011
    #[test]
    fn roots_lists_the_always_granted_scopes() {
        let mut fs_import = FsImport::default();
        let roots = fs_import.roots();
        let scope_names: Vec<&str> = roots.iter().map(|root| root.scope.as_str()).collect();
        assert!(
            scope_names.contains(&"package"),
            "package scope missing: {scope_names:?}"
        );
        assert!(
            scope_names.contains(&"data"),
            "data scope missing: {scope_names:?}"
        );
        assert!(
            scope_names.contains(&"cache"),
            "cache scope missing: {scope_names:?}"
        );
    }

    /// REQ: CAP-011
    #[test]
    fn roots_marks_package_read_only_and_data_writable() {
        let mut fs_import = FsImport::default();
        let roots = fs_import.roots();
        let package = roots.iter().find(|root| root.scope == "package");
        let data = roots.iter().find(|root| root.scope == "data");
        assert!(
            matches!(package, Some(root) if !root.writable),
            "package must be read only"
        );
        assert!(
            matches!(data, Some(root) if root.writable),
            "data must be writable"
        );
    }

    /// REQ: API-002
    #[test]
    fn metadata_of_absent_entry_is_not_found() {
        let mut fs_import = FsImport::default();
        let result = fs_import.metadata(fs_path("data", "no-such-entry.txt"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// REQ: API-001
    #[test]
    fn metadata_error_carries_a_message_within_the_limit() {
        let mut fs_import = FsImport::default();
        let result = fs_import.metadata(fs_path("data", "no-such-entry.txt"));
        match result {
            Ok(_) => panic!("metadata of an absent entry must fail"),
            Err(error) => {
                assert!(!error.message.is_empty(), "error message must not be empty");
                assert!(
                    error.message.chars().count() <= 256,
                    "message over 256 characters"
                );
            }
        }
    }

    /// REQ: SEC-058
    #[test]
    fn metadata_path_escaping_the_scope_root_is_not_permitted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.metadata(fs_path("data", "../escape.txt"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: CAP-018
    #[test]
    fn metadata_in_an_ungranted_scope_is_not_permitted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.metadata(fs_path("project", "README.md"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-062
    #[test]
    fn list_dir_above_the_entry_cap_is_invalid_argument() {
        let mut fs_import = FsImport::default();
        let result = fs_import.list_dir(fs_path("data", ""), LIST_DIR_MAX_ENTRIES + 1);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-062
    #[test]
    fn list_dir_at_the_entry_cap_is_accepted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.list_dir(fs_path("data", ""), LIST_DIR_MAX_ENTRIES);
        assert_ne!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-002
    #[test]
    fn list_dir_of_absent_folder_is_not_found() {
        let mut fs_import = FsImport::default();
        let result = fs_import.list_dir(fs_path("data", "no-such-folder"), 10);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// REQ: API-062
    #[test]
    fn read_above_one_mib_is_invalid_argument() {
        let mut fs_import = FsImport::default();
        let result = fs_import.read(fs_path("data", "blob.bin"), 0, ONE_MIB + 1);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-062
    #[test]
    fn read_at_exactly_one_mib_is_accepted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.read(fs_path("data", "blob.bin"), 0, ONE_MIB);
        assert_ne!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn read_outside_the_scope_fails_at_once() {
        let mut fs_import = FsImport::default();
        let result = fs_import.read(fs_path("data", "../../etc/passwd"), 0, 16);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-002
    #[test]
    fn read_of_absent_file_is_not_found() {
        let mut fs_import = FsImport::default();
        let result = fs_import.read(fs_path("data", "no-such-file.bin"), 0, 16);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// REQ: API-005
    #[test]
    fn read_all_outside_the_scope_fails_in_the_call() {
        let mut fs_import = FsImport::default();
        let result = fs_import.read_all(fs_path("data", "../secret.txt"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: CAP-018
    #[test]
    fn read_all_in_an_ungranted_scope_is_not_permitted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.read_all(fs_path("picked", "0"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-061
    #[test]
    fn scan_above_the_entry_cap_is_invalid_argument() {
        let mut fs_import = FsImport::default();
        let result = fs_import.scan(fs_path("data", ""), "*.md".to_owned(), SCAN_MAX_ENTRIES + 1);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-061
    #[test]
    fn scan_at_the_entry_cap_is_accepted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.scan(fs_path("data", ""), "*.md".to_owned(), SCAN_MAX_ENTRIES);
        assert_ne!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn scan_with_a_malformed_glob_is_invalid_argument() {
        let mut fs_import = FsImport::default();
        let result = fs_import.scan(fs_path("data", ""), "[unclosed".to_owned(), 10);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn write_above_the_payload_cap_is_invalid_argument() {
        let mut fs_import = FsImport::default();
        let data = vec![0_u8; WRITE_MAX_BYTES + 1];
        let result = fs_import.write(fs_path("data", "large.bin"), data, WriteMode::Replace);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn write_at_the_payload_cap_is_accepted() {
        let mut fs_import = FsImport::default();
        let data = vec![0_u8; WRITE_MAX_BYTES];
        let result = fs_import.write(fs_path("data", "large.bin"), data, WriteMode::Replace);
        assert_ne!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn empty_write_is_not_refused_by_the_call_checks() {
        let mut fs_import = FsImport::default();
        let result = fs_import.write(fs_path("data", "empty.txt"), Vec::new(), WriteMode::Append);
        assert_ne!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn write_to_the_read_only_package_scope_is_not_permitted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.write(
            fs_path("package", "assets/file.txt"),
            b"new".to_vec(),
            WriteMode::Replace,
        );
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-002
    #[test]
    fn create_dir_with_a_missing_parent_is_not_found() {
        let mut fs_import = FsImport::default();
        let result = fs_import.create_dir(fs_path("data", "missing-parent/child"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// REQ: API-005
    #[test]
    fn create_dir_outside_the_scope_is_not_permitted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.create_dir(fs_path("data", "../outside"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-005
    #[test]
    fn remove_of_a_scope_root_is_invalid_argument() {
        let mut fs_import = FsImport::default();
        let result = fs_import.remove(fs_path("data", ""));
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn rename_across_two_scopes_is_invalid_argument() {
        let mut fs_import = FsImport::default();
        let result = fs_import.rename(fs_path("data", "a.txt"), fs_path("cache", "a.txt"));
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-002
    #[test]
    fn watch_of_absent_folder_is_not_found() {
        let mut fs_import = FsImport::default();
        let result = fs_import.watch(fs_path("data", "no-such-folder"), true);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// REQ: SEC-058
    #[test]
    fn watch_outside_the_scope_is_not_permitted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.watch(fs_path("data", "../elsewhere"), false);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-005
    #[test]
    fn lock_of_a_file_that_does_not_exist_is_accepted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.lock(fs_path("data", "not-yet-created.lock"), true, 2_000);
        assert!(result.is_ok(), "a lock need not name an existing file");
    }

    /// REQ: API-005
    #[test]
    fn lock_needs_write_grant_so_read_only_package_is_not_permitted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.lock(fs_path("package", "assets/file.txt"), false, 0);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: CAP-018
    #[test]
    fn pick_without_the_fs_pick_grant_is_not_permitted() {
        let mut fs_import =
            FsImport::new(ImportEnv::unrestricted().with_grants(vec!["fs.read:picked".to_owned()]));
        let spec = PickSpec {
            kind: PickKind::OpenFile,
            title: "Choose a file".to_owned(),
            extensions: vec!["md".to_owned()],
        };
        let result = fs_import.pick(spec);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-005
    #[test]
    fn read_with_revision_outside_the_scope_fails_in_the_call() {
        let mut fs_import = FsImport::default();
        let result = fs_import.read_with_revision(fs_path("data", "../secret.txt"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-005
    #[test]
    fn write_atomic_to_the_read_only_package_scope_is_not_permitted() {
        let mut fs_import = FsImport::default();
        let result =
            fs_import.write_atomic(fs_path("package", "assets/file.txt"), b"x".to_vec(), None);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-062
    #[test]
    fn tail_above_one_mib_is_invalid_argument() {
        let mut fs_import = FsImport::default();
        let result = fs_import.tail(fs_path("data", "log.txt"), 0, ONE_MIB + 1);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn tail_outside_the_scope_fails_at_once() {
        let mut fs_import = FsImport::default();
        let result = fs_import.tail(fs_path("data", "../log.txt"), 0, 1024);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: CAP-018
    #[test]
    fn propose_agent_config_without_its_grant_is_not_permitted() {
        let mut fs_import = FsImport::default();
        let result = fs_import.propose_agent_config(
            "claude".to_owned(),
            "Update settings".to_owned(),
            Vec::new(),
        );
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }
}
