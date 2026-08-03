//! Project-scoped session lifecycle for the CLI wrapper.
//!
//! A session is identified by the canonical directory in which `ilium` was
//! launched plus its logical name. Persistent state therefore belongs to the
//! project (`.ilium/sessions/<name>.json`), while only the short-lived Unix
//! socket is global. This preserves detached client/server operation without
//! making every project attach to a machine-wide `default` session.

use std::fmt::Write as _;
// The liveness probe below is still Unix-only; the Windows arm arrives with
// the named-pipe transport (see `ilium_ipc`'s transport boundary).
#[cfg(unix)]
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chrono::Local;
use ilium_ipc::{write_frame, ClientRequest};
use ilium_platform::file_lock::ExclusiveFileLock;
use ilium_platform::runtime_dir::{self, MAX_SOCKET_PATH_BYTES};
use ilium_platform::{detached, process_control, secure_fs};
use sha2::{Digest, Sha256};

use crate::error::CliError;

/// The logical session selected by a bare `ilium` invocation.
pub const DEFAULT_SESSION_NAME: &str = "default";
const SERVER_START_TIMEOUT: Duration = Duration::from_secs(5);
const SERVER_START_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SERVER_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const GRACEFUL_RESTART_TIMEOUT: Duration = Duration::from_secs(1);
/// Upper bound on the readable slug regardless of available budget: past this
/// the extra characters stop aiding recognition and only crowd out the parts
/// of the name that carry identity.
const MAX_SOCKET_SLUG_BYTES: usize = 48;
const ACTIVE_LOG_PATH_FILE: &str = ".active-log-path";
const SERVER_START_LOCK_FILE: &str = ".server-start.lock";
/// How many timestamped debug logs `prune_stale_logs` leaves behind per
/// session on every server start. `/tmp` is commonly tmpfs, so an unrotated
/// log directory consumes RAM, not just disk, across the lifetime of the
/// machine -- this bounds it instead of leaving retention unbounded.
const RETAINED_LOG_FILE_COUNT: usize = 5;

/// Ready metadata format emitted by current `ilium-server` processes. A
/// legacy marker containing only the log path is still understood so an
/// already-running server from a prior local build remains controllable.
const READY_METADATA_PID_PREFIX: &str = "pid=";
const READY_METADATA_LOG_PATH_PREFIX: &str = "log_path=";

/// Every path needed to own exactly one project-local session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSession {
    pub name: String,
    pub project_root: PathBuf,
    pub socket_path: PathBuf,
    pub snapshot_path: PathBuf,
    pub log_directory: PathBuf,
    pub active_log_path_file: PathBuf,
    pub server_start_lock_file: PathBuf,
}

/// One project-local session visible to `ilium ls`.
pub struct SessionListing {
    pub name: String,
    pub live: bool,
}

/// The server's atomically published identity and diagnostic destination.
/// `server_pid: None` denotes the path-only format from a pre-ready-marker
/// local build; current servers always publish the PID as well.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveLogMetadata {
    server_pid: Option<u32>,
    log_path: PathBuf,
}

/// Resolves the canonical project root and all paths for one named session.
///
/// The readable socket prefix follows Claude Code's absolute-path slug
/// convention. A digest of the unmodified canonical path prevents collisions
/// such as `/work/a.b` and `/work/a-b`, and bounds the socket length.
pub fn resolve_project_session(cwd: &Path, session_name: &str) -> Result<ProjectSession, CliError> {
    let project_root = cwd
        .canonicalize()
        .map_err(|_| CliError::InvalidCwd(cwd.to_path_buf()))?;
    if !project_root.is_dir() {
        return Err(CliError::InvalidCwd(cwd.to_path_buf()));
    }
    validate_session_name(session_name)?;

    let snapshot_dir = project_root.join(".ilium").join("sessions");
    std::fs::create_dir_all(&snapshot_dir).map_err(|source| CliError::SessionStorage {
        path: snapshot_dir.clone(),
        source,
    })?;
    let socket_dir = runtime_socket_dir()?;
    // The readable slug is decoration; the digest and the session name carry
    // identity. So the slug takes whatever the *actual* directory leaves over
    // rather than a fixed cap, which is what made macOS fail: its per-session
    // temporary directory alone consumed most of `sun_path`.
    let socket_key = socket_key(
        &project_root,
        session_name,
        slug_budget(&socket_dir, session_name),
    );
    let socket_path = socket_dir.join(format!("{socket_key}.sock"));
    if socket_path.as_os_str().len() >= MAX_SOCKET_PATH_BYTES {
        // Only reachable when the digest, the session name, and `.sock` alone
        // overflow the budget -- an empty slug cannot shrink any further.
        return Err(CliError::SocketPathTooLong(socket_path));
    }

    let log_root = runtime_dir::debug_log_root().map_err(|source| CliError::SessionStorage {
        path: PathBuf::from("debug log root"),
        source,
    })?;
    let log_directory = log_root.join(&socket_key);
    ensure_private_directory(&log_directory)?;

    Ok(ProjectSession {
        name: session_name.to_string(),
        project_root,
        socket_path,
        snapshot_path: snapshot_dir.join(format!("{session_name}.json")),
        active_log_path_file: log_directory.join(ACTIVE_LOG_PATH_FILE),
        server_start_lock_file: log_directory.join(SERVER_START_LOCK_FILE),
        log_directory,
    })
}

/// Serializes the socket recheck, log-path publication, and detached spawn so
/// simultaneous clients cannot start competing servers with different logs.
///
/// The returned guard holds the lock until it drops; `ilium_platform` owns
/// both the lock mechanism and the lock file's owner-only permissions.
fn acquire_server_start_lock(session: &ProjectSession) -> Result<ExclusiveFileLock, CliError> {
    ExclusiveFileLock::acquire(&session.server_start_lock_file).map_err(|source| {
        CliError::SessionStorage {
            path: session.server_start_lock_file.clone(),
            source,
        }
    })
}

fn ensure_private_directory(path: &Path) -> Result<(), CliError> {
    std::fs::create_dir_all(path).map_err(|source| CliError::SessionStorage {
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = std::fs::symlink_metadata(path).map_err(|source| CliError::SessionStorage {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CliError::SessionStorage {
            path: path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "debug log directory must be a real directory, not a symlink",
            ),
        });
    }
    secure_fs::restrict_directory_to_owner(path).map_err(|source| CliError::SessionStorage {
        path: path.to_path_buf(),
        source,
    })
}

fn validate_session_name(session_name: &str) -> Result<(), CliError> {
    let is_valid = !session_name.is_empty()
        && session_name.len() <= 48
        && session_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if is_valid {
        Ok(())
    } else {
        Err(CliError::InvalidSessionName(session_name.to_string()))
    }
}

fn runtime_socket_dir() -> Result<PathBuf, CliError> {
    runtime_dir::session_socket_directory().map_err(|source| CliError::SessionStorage {
        path: PathBuf::from("session socket directory"),
        source,
    })
}

/// How many bytes the readable slug may occupy so the finished socket path
/// still fits `sockaddr_un`.
///
/// Everything else in the path is fixed-size or identity-critical: the
/// directory, one separator, the twelve-character digest, its two separators,
/// the session name, and `.sock`. Whatever remains is the slug's, which may be
/// nothing at all.
fn slug_budget(socket_dir: &Path, session_name: &str) -> usize {
    const DIGEST_PREFIX_BYTES: usize = 12;
    const SEPARATOR_BYTES: usize = 3; // one after the directory, two in the key
    const EXTENSION_BYTES: usize = ".sock".len();

    MAX_SOCKET_PATH_BYTES
        .saturating_sub(socket_dir.as_os_str().len())
        .saturating_sub(DIGEST_PREFIX_BYTES)
        .saturating_sub(SEPARATOR_BYTES)
        .saturating_sub(session_name.len())
        .saturating_sub(EXTENSION_BYTES)
        // One byte of headroom keeps the finished path strictly below the
        // limit, which is what `resolve_project_session` checks.
        .saturating_sub(1)
        .min(MAX_SOCKET_SLUG_BYTES)
}

fn socket_key(project_root: &Path, session_name: &str, slug_budget: usize) -> String {
    let mut readable_slug: String = project_root
        .to_string_lossy()
        .chars()
        .map(|character| {
            if character == '/' || character == '.' {
                '-'
            } else {
                character
            }
        })
        .collect();
    // `String::truncate` panics unless the byte offset falls on a char
    // boundary. Path components routinely contain multi-byte UTF-8
    // characters (accents, CJK, emoji), so a fixed byte offset can land
    // mid-character; walk back to the nearest valid boundary first.
    if readable_slug.len() > slug_budget {
        let mut truncate_at = slug_budget;
        while truncate_at > 0 && !readable_slug.is_char_boundary(truncate_at) {
            truncate_at -= 1;
        }
        readable_slug.truncate(truncate_at);
    }

    let digest = Sha256::digest(project_root.as_os_str().as_encoded_bytes());
    let mut digest_prefix = String::with_capacity(12);
    for byte in &digest[..6] {
        let _ = write!(&mut digest_prefix, "{byte:02x}");
    }
    format!("{readable_slug}-{digest_prefix}-{session_name}")
}

/// True only for a connect error that proves the filesystem entry is a dead
/// listener rather than a live one this process merely failed to reach.
/// `ConnectionRefused` is the kernel's answer when nothing is `accept()`ing
/// on the socket; `NotFound` covers a path removed by a concurrent racer
/// between the caller's existence check and this connect attempt. Anything
/// else -- `EMFILE`/`ENFILE` (fd exhaustion, not exotic in a process that
/// spawns many PTYs), `EACCES`, a transient `EINTR`, and similar -- says
/// nothing about the listener's liveness, so the socket must be left alone:
/// deleting it out from under a healthy server would let a caller spawn a
/// second server for the same project.
fn connect_error_proves_dead_listener(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
    )
}

/// True if the socket accepts a connection. A filesystem entry proven dead
/// (see `connect_error_proves_dead_listener`) is removed so a replacement
/// server can bind it; any other connect failure leaves it in place.
pub fn is_session_live(socket_path: &Path) -> bool {
    if !socket_path.exists() {
        return false;
    }
    match StdUnixStream::connect(socket_path) {
        Ok(_) => true,
        Err(error) => {
            if connect_error_proves_dead_listener(&error) {
                let _ = std::fs::remove_file(socket_path);
            }
            false
        }
    }
}

/// Lists this project's native sessions, never sessions belonging to another
/// directory. A snapshot remains listed after its server exits so it can be
/// deliberately reopened.
pub fn list_sessions(cwd: &Path) -> Result<Vec<SessionListing>, CliError> {
    let project_root = cwd
        .canonicalize()
        .map_err(|_| CliError::InvalidCwd(cwd.to_path_buf()))?;
    let snapshot_dir = project_root.join(".ilium").join("sessions");
    if !snapshot_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in std::fs::read_dir(&snapshot_dir).map_err(|source| CliError::SessionStorage {
        path: snapshot_dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| CliError::SessionStorage {
            path: snapshot_dir.clone(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        // A stray or legacy snapshot file whose name no longer satisfies
        // `validate_session_name` must not hide every other valid session
        // from the listing -- skip just that entry.
        let Ok(session) = resolve_project_session(&project_root, name) else {
            continue;
        };
        sessions.push(SessionListing {
            name: name.to_string(),
            live: is_session_live(&session.socket_path),
        });
    }
    sessions.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(sessions)
}

fn first_existing_server_binary(
    candidate_paths: impl IntoIterator<Item = PathBuf>,
) -> Option<PathBuf> {
    candidate_paths
        .into_iter()
        .find(|candidate_path| candidate_path.is_file())
}

fn locate_server_binary() -> PathBuf {
    let binary_name = format!("ilium-server{}", std::env::consts::EXE_SUFFIX);
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut candidate_paths = vec![
        workspace_root
            .join("target")
            .join("debug")
            .join(&binary_name),
        workspace_root
            .join("target")
            .join("release")
            .join(&binary_name),
    ];
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(directory) = current_exe.parent() {
            candidate_paths.insert(0, directory.join(&binary_name));
            // Integration-test executables live under `debug/deps`, while
            // Cargo places sibling binaries one level above in `debug`.
            // Prefer that matching target directory before falling back to
            // the workspace's potentially stale default target output.
            if let Some(build_output_directory) = directory.parent() {
                candidate_paths.insert(1, build_output_directory.join(&binary_name));
            }
        }
    }
    first_existing_server_binary(candidate_paths).unwrap_or_else(|| PathBuf::from(binary_name))
}

/// Spawns `ilium-server` as a true Unix daemon: double-forked so the
/// running server is reparented to init (PID 1) rather than staying a
/// direct child of this CLI process.
///
/// A single fork (the previous approach, via `.process_group(0)`) only
/// isolates the server's signal/terminal group -- it remains this
/// process's child in the kernel's process table. `attach_or_create`'s
/// caller can run the TUI for days, and `restart_client_process` later
/// `exec()`s a replacement image in the *same* PID, which preserves the
/// parent/child relationship (children are never reparented by `exec`)
/// but destroys every bit of Rust-level state -- including any reaper
/// thread this process could spawn. Either way, a server that later
/// crashes or is killed (`kill-session`, `--restart-server` from another
/// terminal) would sit as an unreaped zombie for the rest of this
/// process's lifetime, because nothing is left that could call `wait()`
/// on it.
///
/// Reparenting to init sidesteps the problem instead of chasing it: init
/// reaps its orphans unconditionally, so no reaper needs to survive
/// attach, restart, or exec for the server's exit status to be collected.
/// The short-lived middle process this fork creates is reaped
/// immediately below, so this still leaves nothing unwaited.
fn spawn_server_detached(session: &ProjectSession) -> Result<PathBuf, CliError> {
    let server_binary = locate_server_binary();
    // Runs while this function's only caller (`ensure_server_running`) still
    // holds the server-start lock, so a concurrent attach cannot race a
    // delete against a server that just started writing its own fresh log.
    prune_stale_logs(session);
    let log_path = log_path_for_new_server(session);
    let mut command = Command::new(&server_binary);
    command
        .args([
            "--session-name",
            &session.name,
            "--socket-path",
            session.socket_path.to_string_lossy().as_ref(),
            "--snapshot-path",
            session.snapshot_path.to_string_lossy().as_ref(),
            "--session-cwd",
            session.project_root.to_string_lossy().as_ref(),
            "--log-path",
            log_path.to_string_lossy().as_ref(),
            "--active-log-path-file",
            session.active_log_path_file.to_string_lossy().as_ref(),
        ])
        .current_dir(&session.project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // `ilium_platform::detached` owns how each platform reaches a process that
    // outlives this one with no controlling terminal, including reaping the
    // intermediate process on Unix so nothing is left unwaited.
    match detached::spawn_detached(&mut command) {
        Ok(()) => Ok(log_path),
        Err(source) => Err(if source.kind() == std::io::ErrorKind::NotFound {
            CliError::ServerBinaryNotFound(server_binary)
        } else {
            CliError::SpawnServer {
                session: session.name.clone(),
                source,
            }
        }),
    }
}

/// Returns only after this project session owns a live server socket, together
/// with the timestamped file shared by that server and its attached clients.
pub async fn ensure_server_running(session: &ProjectSession) -> Result<PathBuf, CliError> {
    if is_session_live(&session.socket_path) {
        return read_active_log_path_for_live_server(session).await;
    }
    let _server_start_lock = acquire_server_start_lock(session)?;
    if is_session_live(&session.socket_path) {
        return read_active_log_path_for_live_server(session).await;
    }
    let expected_log_path = spawn_server_detached(session)?;
    let deadline = Instant::now() + SERVER_START_TIMEOUT;
    while Instant::now() < deadline {
        if is_session_live(&session.socket_path)
            && matches!(
                read_active_log_path_for_live_server(session).await,
                Ok(path) if path == expected_log_path
            )
        {
            return Ok(expected_log_path);
        }
        tokio::time::sleep(SERVER_START_POLL_INTERVAL).await;
    }
    Err(CliError::ServerStartTimeout(
        session.name.clone(),
        SERVER_START_TIMEOUT,
    ))
}

/// Replaces this session's running server, if any, with a server spawned by
/// the current CLI executable. The old server's PID comes from the Unix
/// socket peer credentials rather than a name-based process search, so a
/// restart can never terminate another project's session.
///
/// `SIGTERM` deliberately leaves the project snapshot in place. The newly
/// spawned server restores that snapshot, retaining the session layout while
/// replacing the server executable during development.
pub async fn replace_server(session: &ProjectSession) -> Result<PathBuf, CliError> {
    stop_server(session).await?;
    ensure_server_running(session).await
}

/// Stops only this project session's live server. The caller chooses whether
/// to preserve or remove its durable snapshot before starting a replacement.
async fn stop_server(session: &ProjectSession) -> Result<(), CliError> {
    if let Some(server_pid) = server_process_id(&session.socket_path).await? {
        // A current server saves its live tree before it exits. Older dev
        // servers do not recognize this appended IPC variant, so a bounded
        // wait then falls back to terminating the exact socket peer.
        let _ = request_graceful_restart(session).await;
        if wait_for_server_process_stop(server_pid, GRACEFUL_RESTART_TIMEOUT)
            .await
            .is_err()
        {
            terminate_server(server_pid)?;
            wait_for_server_process_stop(server_pid, SERVER_STOP_TIMEOUT).await?;
        }
        // The server's graceful shutdown normally removes this path, while
        // an older server terminated by the compatibility fallback leaves a
        // stale entry. The peer PID is already confirmed dead above, so this
        // cannot unlink a listener owned by another live server.
        let _ = std::fs::remove_file(&session.socket_path);
    }
    Ok(())
}

/// Deletes this one project's session snapshot and starts an empty server.
/// Unlike `replace_server`, this is intentionally destructive and only runs
/// when the CLI caller explicitly requested a reset.
pub async fn reset_session(session: &ProjectSession) -> Result<PathBuf, CliError> {
    stop_server(session).await?;
    for path in reset_storage_paths(session) {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(CliError::SessionStorage { path, source }),
        }
    }
    ensure_server_running(session).await
}

/// Builds a collision-resistant, human-readable process-start filename using
/// the machine's local timezone. Milliseconds keep rapid development restarts
/// from appending to the same file while preserving the requested date/time.
fn log_path_for_new_server(session: &ProjectSession) -> PathBuf {
    let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S%.3f");
    session.log_directory.join(format!("log-{timestamp}.txt"))
}

/// True for filenames produced by `log_path_for_new_server`, so pruning
/// never touches `.active-log-path`, `.server-start.lock`, or anything else
/// an operator might have dropped into the session's log directory.
fn is_debug_log_file_name(file_name: &str) -> bool {
    file_name.starts_with("log-") && file_name.ends_with(".txt")
}

/// Decides which debug-log filenames are stale given the full set present in
/// a session's log directory. Pure and independent of the filesystem so the
/// retention policy is unit-testable without a real directory: everything
/// beyond the `RETAINED_LOG_FILE_COUNT` most recent files is stale, except
/// `active_log_file_name` (when present) is always kept even if it falls
/// outside that window -- a slow-starting server's own log must survive its
/// own start.
///
/// Filenames use the fixed-width, zero-padded `log-<YYYY-MM-DD_HH-MM-SS.mmm>.txt`
/// scheme from `log_path_for_new_server`, so lexical order is chronological
/// order and no timestamp parsing is needed.
fn select_stale_log_file_names(
    existing_log_file_names: &[String],
    active_log_file_name: Option<&str>,
) -> Vec<String> {
    let mut sorted_names = existing_log_file_names.to_vec();
    sorted_names.sort();

    let stale_count = sorted_names.len().saturating_sub(RETAINED_LOG_FILE_COUNT);
    sorted_names
        .into_iter()
        .take(stale_count)
        .filter(|file_name| Some(file_name.as_str()) != active_log_file_name)
        .collect()
}

/// Deletes debug logs beyond the retained window in `session.log_directory`,
/// so every server start (each `ilium` launch, `--restart-server`,
/// `--reset-session`) doesn't leave behind one full debug log for the
/// lifetime of the machine's `/tmp`. Nothing else in this crate or in
/// `ilium-server` ever prunes this directory: `stop_server` removes only the
/// socket, and `reset_storage_paths` removes only the snapshot -- this is the
/// one place log retention is enforced.
///
/// This is best-effort maintenance, not part of the server-start contract:
/// a listing or delete failure is logged to stderr and otherwise ignored
/// rather than propagated, so a permissions hiccup in an old log file can
/// never block starting the actual server.
fn prune_stale_logs(session: &ProjectSession) {
    let entries = match std::fs::read_dir(&session.log_directory) {
        Ok(entries) => entries,
        // A brand-new session has no log directory contents to prune yet.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            eprintln!(
                "ilium: warning: could not list {:?} to prune stale debug logs: {error}",
                session.log_directory
            );
            return;
        }
    };

    let mut existing_log_file_names = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let Ok(file_name) = entry.file_name().into_string() else {
            // Not valid UTF-8, so it cannot be a filename this CLI ever
            // wrote -- leave it alone rather than guessing.
            continue;
        };
        if is_debug_log_file_name(&file_name) {
            existing_log_file_names.push(file_name);
        }
    }

    // A dead marker (server process no longer running) must not pin its log
    // forever; only a marker whose PID is still alive is honored.
    let active_log_file_name = read_active_log_metadata(session)
        .ok()
        .filter(|metadata| metadata.server_pid.is_some_and(is_process_running))
        .and_then(|metadata| {
            metadata
                .log_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        });

    let stale_file_names =
        select_stale_log_file_names(&existing_log_file_names, active_log_file_name.as_deref());
    for file_name in stale_file_names {
        let path = session.log_directory.join(&file_name);
        if let Err(error) = std::fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("ilium: warning: failed to prune stale debug log {path:?}: {error}");
            }
        }
    }
}

/// Resolves the timestamped path selected when an already-running server
/// successfully bound. The detached server publishes this atomically after
/// binding its listener, rather than the launcher publishing a speculative
/// path before the daemon has proved it can start.
pub fn read_active_log_path(session: &ProjectSession) -> Result<PathBuf, CliError> {
    Ok(read_active_log_metadata(session)?.log_path)
}

/// Reads a server's diagnostic path only when its PID agrees with the live
/// Unix-socket peer. This prevents a stale marker from attaching a fresh
/// client to the wrong file after a failed or replaced daemon launch.
async fn read_active_log_path_for_live_server(
    session: &ProjectSession,
) -> Result<PathBuf, CliError> {
    let metadata = read_active_log_metadata(session)?;
    if let Some(expected_pid) = metadata.server_pid {
        let actual_pid = server_process_id(&session.socket_path).await?;
        if actual_pid != Some(expected_pid) {
            return Err(invalid_active_log_metadata(
                session,
                format!(
                    "active log metadata names server PID {expected_pid}, but the live socket peer is {actual_pid:?}"
                ),
            ));
        }
    }
    Ok(metadata.log_path)
}

/// Parses and validates the session-local marker before any caller trusts it
/// for logging. New markers bind the path to a PID; legacy path-only markers
/// remain readable during local development upgrades.
fn read_active_log_metadata(session: &ProjectSession) -> Result<ActiveLogMetadata, CliError> {
    let contents = std::fs::read_to_string(&session.active_log_path_file).map_err(|source| {
        CliError::SessionLogMetadata {
            path: session.active_log_path_file.clone(),
            source,
        }
    })?;
    let metadata = if contents.starts_with(READY_METADATA_PID_PREFIX) {
        let mut lines = contents.lines();
        let pid_line = lines.next().unwrap_or_default();
        let log_path_line = lines.next().unwrap_or_default();
        if lines.next().is_some() {
            return Err(invalid_active_log_metadata(
                session,
                "active log metadata has unexpected trailing content",
            ));
        }
        let server_pid = pid_line
            .strip_prefix(READY_METADATA_PID_PREFIX)
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| {
                invalid_active_log_metadata(session, "active log metadata has an invalid PID")
            })?;
        let log_path = log_path_line
            .strip_prefix(READY_METADATA_LOG_PATH_PREFIX)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| {
                invalid_active_log_metadata(session, "active log metadata has no log path")
            })?;
        ActiveLogMetadata {
            server_pid: Some(server_pid),
            log_path,
        }
    } else {
        ActiveLogMetadata {
            server_pid: None,
            log_path: PathBuf::from(contents),
        }
    };
    let path = metadata.log_path;
    if path.parent() != Some(session.log_directory.as_path()) {
        return Err(invalid_active_log_metadata(
            session,
            "active log path points outside this session's log directory",
        ));
    }
    Ok(ActiveLogMetadata {
        server_pid: metadata.server_pid,
        log_path: path,
    })
}

/// Produces the uniform user-facing error for a malformed or mismatched
/// ready marker without leaking a half-parsed value to lifecycle callers.
fn invalid_active_log_metadata(session: &ProjectSession, message: impl Into<String>) -> CliError {
    CliError::SessionLogMetadata {
        path: session.active_log_path_file.clone(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()),
    }
}

/// Returns the durable artifacts an explicit reset must remove. Kept pure so
/// default-session compatibility cleanup cannot silently regress.
fn reset_storage_paths(session: &ProjectSession) -> Vec<PathBuf> {
    let mut reset_paths = vec![session.snapshot_path.clone()];
    // The pre-client/server workspace had one project-wide YAML file and
    // maps only to the default native session. Leaving it behind would make
    // an explicit default reset silently recreate the very state it removed.
    if session.name == DEFAULT_SESSION_NAME {
        reset_paths.push(session.project_root.join(".ilium").join("sessions.yml"));
    }
    reset_paths
}

/// Asks a compatible server to flush its snapshot and terminate. The caller
/// handles a failed send or timeout by falling back to the peer-PID signal,
/// preserving compatibility with a server from an earlier local build.
async fn request_graceful_restart(session: &ProjectSession) -> Result<(), std::io::Error> {
    let mut stream = tokio::net::UnixStream::connect(&session.socket_path).await?;
    write_frame(&mut stream, &ClientRequest::RestartServer)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))
}

/// Returns the process ID at the other end of a live session socket. A
/// connection failure that proves the listener is dead (see
/// `connect_error_proves_dead_listener`) removes the stale filesystem entry
/// and reports that no server needs replacing; any other connect failure
/// (fd exhaustion, permissions, a transient interrupt) leaves the socket in
/// place, since it says nothing about whether a server is still listening.
async fn server_process_id(socket_path: &Path) -> Result<Option<u32>, CliError> {
    let stream = match tokio::net::UnixStream::connect(socket_path).await {
        Ok(stream) => stream,
        Err(error) => {
            if connect_error_proves_dead_listener(&error) {
                let _ = std::fs::remove_file(socket_path);
            }
            return Ok(None);
        }
    };
    let credentials = stream
        .peer_cred()
        .map_err(|source| CliError::ServerProcessLookup {
            path: socket_path.to_path_buf(),
            source,
        })?;
    let pid = credentials
        .pid()
        .ok_or_else(|| CliError::ServerProcessLookup {
            path: socket_path.to_path_buf(),
            source: std::io::Error::other("session socket did not report a peer process ID"),
        })?;
    let pid = u32::try_from(pid).map_err(|_| CliError::ServerProcessLookup {
        path: socket_path.to_path_buf(),
        source: std::io::Error::other("session socket reported an invalid peer process ID"),
    })?;
    Ok(Some(pid))
}

/// Asks one server process, identified by its live socket peer credentials,
/// to terminate. An already-exited process is success: the graceful shutdown
/// can finish between the preceding liveness probe and this fallback.
fn terminate_server(server_pid: u32) -> Result<(), CliError> {
    process_control::terminate(server_pid).map_err(|source| CliError::ServerTermination {
        pid: server_pid,
        source,
    })
}

/// Waits for the exact process that owned the session socket to exit. A
/// socket-connect probe alone is insufficient: an old listener can accept a
/// queued probe during shutdown and make a just-stopped server look live.
async fn wait_for_server_process_stop(server_pid: u32, timeout: Duration) -> Result<(), CliError> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !is_process_running(server_pid) {
            return Ok(());
        }
        tokio::time::sleep(SERVER_START_POLL_INTERVAL).await;
    }
    Err(CliError::ServerProcessStopTimeout {
        pid: server_pid,
        timeout,
    })
}

/// Checks the signal table without delivering a signal. `EPERM` still means
/// the process exists; the current user merely lacks permission to signal it.
fn is_process_running(process_id: u32) -> bool {
    process_control::is_running(process_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_with_the_same_logical_name_never_share_state() {
        let root = tempfile::tempdir().expect("tempdir");
        let first = root.path().join("a.b");
        let second = root.path().join("a-b");
        std::fs::create_dir_all(&first).expect("first project");
        std::fs::create_dir_all(&second).expect("second project");

        let first_session = resolve_project_session(&first, DEFAULT_SESSION_NAME).expect("first");
        let second_session =
            resolve_project_session(&second, DEFAULT_SESSION_NAME).expect("second");

        assert_ne!(first_session.socket_path, second_session.socket_path);
        assert_ne!(first_session.snapshot_path, second_session.snapshot_path);
        assert_ne!(first_session.log_directory, second_session.log_directory);
        assert!(first_session.snapshot_path.starts_with(&first));
        assert!(second_session.snapshot_path.starts_with(&second));
        let log_root = runtime_dir::debug_log_root().expect("debug log root");
        assert!(first_session.log_directory.starts_with(&log_root));
        assert!(second_session.log_directory.starts_with(&log_root));
        assert_private_directory(&log_root);
        assert_private_directory(&first_session.log_directory);
        assert_eq!(
            first_session
                .active_log_path_file
                .file_name()
                .and_then(|name| name.to_str()),
            Some(ACTIVE_LOG_PATH_FILE)
        );
        assert_eq!(
            first_session
                .server_start_lock_file
                .file_name()
                .and_then(|name| name.to_str()),
            Some(SERVER_START_LOCK_FILE)
        );
    }

    #[test]
    fn session_name_rejects_path_characters() {
        let root = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            resolve_project_session(root.path(), "../other"),
            Err(CliError::InvalidSessionName(_))
        ));
    }

    #[test]
    fn resetting_default_removes_its_legacy_migration_source() {
        let root = tempfile::tempdir().expect("tempdir");
        let default_session =
            resolve_project_session(root.path(), DEFAULT_SESSION_NAME).expect("default session");
        let paths = reset_storage_paths(&default_session);

        assert!(paths.contains(&default_session.snapshot_path));
        assert!(paths.contains(&root.path().join(".ilium").join("sessions.yml")));
    }

    #[test]
    fn resetting_a_named_session_keeps_the_default_legacy_migration_source() {
        let root = tempfile::tempdir().expect("tempdir");
        let named_session = resolve_project_session(root.path(), "review").expect("named session");
        let paths = reset_storage_paths(&named_session);

        assert_eq!(paths, vec![named_session.snapshot_path]);
    }

    #[test]
    fn stale_socket_is_not_live_and_is_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stale_path = dir.path().join("stale.sock");
        std::fs::write(&stale_path, b"not a socket").expect("write stale file");
        assert!(!is_session_live(&stale_path));
        assert!(!stale_path.exists());
    }

    #[test]
    fn terminating_an_already_exited_server_is_successful() {
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn short-lived process");
        let process_id = child.id();
        child.wait().expect("wait for short-lived process");

        terminate_server(process_id).expect("an already-exited server needs no signal");
    }

    #[test]
    fn socket_key_does_not_panic_on_multibyte_path_boundary() {
        let root = tempfile::tempdir().expect("tempdir");
        // Repeated multi-byte characters guarantee byte offset 48 (the slug
        // truncation point) lands mid-character rather than on a boundary.
        let unicode_dir = root.path().join("café-日本語-😀-projet-de-test-longue");
        std::fs::create_dir_all(&unicode_dir).expect("unicode project dir");

        let session = resolve_project_session(&unicode_dir, DEFAULT_SESSION_NAME)
            .expect("should not panic on multi-byte path truncation");
        assert!(session.socket_path.to_string_lossy().ends_with(".sock"));
    }

    #[test]
    fn new_server_log_path_has_timestamped_text_filename() {
        let root = tempfile::tempdir().expect("tempdir");
        let session = resolve_project_session(root.path(), "review").expect("session");

        let log_path = log_path_for_new_server(&session);
        let filename = log_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("filename");

        assert!(filename.starts_with("log-20"));
        assert!(filename.ends_with(".txt"));
        assert_eq!(log_path.parent(), Some(session.log_directory.as_path()));
    }

    #[test]
    fn active_log_metadata_round_trips_only_inside_session_directory() {
        let root = tempfile::tempdir().expect("tempdir");
        let session = resolve_project_session(root.path(), "review").expect("session");
        let log_path = session
            .log_directory
            .join("log-2026-07-19_12-00-00.000.txt");

        std::fs::write(
            &session.active_log_path_file,
            log_path.as_os_str().as_encoded_bytes(),
        )
        .expect("write ready-server metadata fixture");

        assert_eq!(
            read_active_log_path(&session).expect("read metadata"),
            log_path
        );
    }

    #[test]
    fn select_stale_log_file_names_keeps_only_the_most_recent_files() {
        let names: Vec<String> = (0..8)
            .map(|index| format!("log-2026-07-19_00-00-0{index}.000.txt"))
            .collect();

        let stale = select_stale_log_file_names(&names, None);

        assert_eq!(stale, names[..3].to_vec());
    }

    #[test]
    fn select_stale_log_file_names_never_marks_the_active_log_stale() {
        let names: Vec<String> = (0..8)
            .map(|index| format!("log-2026-07-19_00-00-0{index}.000.txt"))
            .collect();

        let stale = select_stale_log_file_names(&names, Some(&names[0]));

        assert!(!stale.contains(&names[0]));
        assert_eq!(stale, names[1..3].to_vec());
    }

    #[test]
    fn prune_stale_logs_deletes_only_files_beyond_the_retention_window() {
        let root = tempfile::tempdir().expect("tempdir");
        let session = resolve_project_session(root.path(), "review").expect("session");

        let mut created_file_names = Vec::new();
        for index in 0..8 {
            let file_name = format!("log-2026-07-19_00-00-0{index}.000.txt");
            std::fs::write(session.log_directory.join(&file_name), b"").expect("write log fixture");
            created_file_names.push(file_name);
        }

        prune_stale_logs(&session);

        let mut remaining: Vec<String> = std::fs::read_dir(&session.log_directory)
            .expect("read log directory")
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_debug_log_file_name(name))
            .collect();
        remaining.sort();

        assert_eq!(remaining, created_file_names[3..].to_vec());
    }

    #[test]
    fn prune_stale_logs_never_deletes_the_active_log_of_a_live_process() {
        let root = tempfile::tempdir().expect("tempdir");
        let session = resolve_project_session(root.path(), "review").expect("session");

        // Older than the retention window on its own -- pruning must still
        // leave it in place because it is published as the active log for
        // this test process, which is guaranteed live for the test's
        // duration.
        let active_file_name = "log-2020-01-01_00-00-00.000.txt";
        let active_log_path = session.log_directory.join(active_file_name);
        std::fs::write(&active_log_path, b"").expect("write active log fixture");
        for index in 0..8 {
            let file_name = format!("log-2026-07-19_00-00-0{index}.000.txt");
            std::fs::write(session.log_directory.join(&file_name), b"").expect("write log fixture");
        }
        std::fs::write(
            &session.active_log_path_file,
            format!(
                "pid={}\nlog_path={}",
                std::process::id(),
                active_log_path.display()
            ),
        )
        .expect("write active-log-path marker");

        prune_stale_logs(&session);

        assert!(active_log_path.exists());
    }

    #[test]
    fn server_start_lock_serializes_competing_first_attach_processes() {
        let root = tempfile::tempdir().expect("tempdir");
        let session = resolve_project_session(root.path(), "review").expect("session");
        let first_lock = acquire_server_start_lock(&session).expect("first lock");
        let (acquired_sender, acquired_receiver) = std::sync::mpsc::channel();

        let competing_session = session.clone();
        let competing_thread = std::thread::spawn(move || {
            let competing_lock =
                acquire_server_start_lock(&competing_session).expect("competing lock");
            acquired_sender.send(()).expect("report acquisition");
            drop(competing_lock);
        });

        assert!(acquired_receiver
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        drop(first_lock);
        acquired_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("competing lock should acquire after release");
        competing_thread.join().expect("competing thread");
        assert_private_file(&session.server_start_lock_file);
    }

    /// Owner-only modes are a Unix concept; on Windows the same protection
    /// comes from the parent directory's inherited ACL, which exposes no mode
    /// bits to compare. See `ilium_platform::secure_fs`.
    #[cfg(unix)]
    fn assert_private_directory(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let mode = std::fs::metadata(path)
            .expect("directory metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "{path:?} should be owner-only");
    }

    #[cfg(not(unix))]
    fn assert_private_directory(path: &Path) {
        assert!(path.is_dir(), "{path:?} should exist");
    }

    #[cfg(unix)]
    fn assert_private_file(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let mode = std::fs::metadata(path)
            .expect("file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "{path:?} should be owner-only");
    }

    #[cfg(not(unix))]
    fn assert_private_file(path: &Path) {
        assert!(path.is_file(), "{path:?} should exist");
    }
}
