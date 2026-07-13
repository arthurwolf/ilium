//! Project-scoped session lifecycle for the CLI wrapper.
//!
//! A session is identified by the canonical directory in which `ilium` was
//! launched plus its logical name. Persistent state therefore belongs to the
//! project (`.ilium/sessions/<name>.json`), while only the short-lived Unix
//! socket is global. This preserves detached client/server operation without
//! making every project attach to a machine-wide `default` session.

use std::fmt::Write as _;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use directories::BaseDirs;
use ilium_ipc::{write_frame, ClientRequest};
use sha2::{Digest, Sha256};

use crate::error::CliError;

/// The logical session selected by a bare `ilium` invocation.
pub const DEFAULT_SESSION_NAME: &str = "default";
const SERVER_START_TIMEOUT: Duration = Duration::from_secs(5);
const SERVER_START_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SERVER_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const GRACEFUL_RESTART_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_SOCKET_PATH_BYTES: usize = 100;
const MAX_SOCKET_SLUG_BYTES: usize = 48;

/// Every path needed to own exactly one project-local session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSession {
    pub name: String,
    pub project_root: PathBuf,
    pub socket_path: PathBuf,
    pub snapshot_path: PathBuf,
}

/// One project-local session visible to `ilium ls`.
pub struct SessionListing {
    pub name: String,
    pub live: bool,
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
    let socket_key = socket_key(&project_root, session_name);
    let socket_path = socket_dir.join(format!("{socket_key}.sock"));
    if socket_path.as_os_str().len() >= MAX_SOCKET_PATH_BYTES {
        return Err(CliError::SocketPathTooLong(socket_path));
    }

    Ok(ProjectSession {
        name: session_name.to_string(),
        project_root,
        socket_path,
        snapshot_path: snapshot_dir.join(format!("{session_name}.json")),
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
    let directory = BaseDirs::new()
        .as_ref()
        .and_then(BaseDirs::runtime_dir)
        .map(|runtime_dir| runtime_dir.join("ilium"))
        .unwrap_or_else(|| std::env::temp_dir().join("ilium"));
    std::fs::create_dir_all(&directory).map_err(|source| CliError::SessionStorage {
        path: directory.clone(),
        source,
    })?;
    Ok(directory)
}

fn socket_key(project_root: &Path, session_name: &str) -> String {
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
    if readable_slug.len() > MAX_SOCKET_SLUG_BYTES {
        let mut truncate_at = MAX_SOCKET_SLUG_BYTES;
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

/// True if the socket accepts a connection. A dead filesystem entry is
/// removed so a replacement server can bind it.
pub fn is_session_live(socket_path: &Path) -> bool {
    if !socket_path.exists() {
        return false;
    }
    match StdUnixStream::connect(socket_path) {
        Ok(_) => true,
        Err(_) => {
            let _ = std::fs::remove_file(socket_path);
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
        }
    }
    first_existing_server_binary(candidate_paths).unwrap_or_else(|| PathBuf::from(binary_name))
}

fn spawn_server_detached(session: &ProjectSession) -> Result<(), CliError> {
    let server_binary = locate_server_binary();
    Command::new(&server_binary)
        .args([
            "--session-name",
            &session.name,
            "--socket-path",
            session.socket_path.to_string_lossy().as_ref(),
            "--snapshot-path",
            session.snapshot_path.to_string_lossy().as_ref(),
            "--session-cwd",
            session.project_root.to_string_lossy().as_ref(),
        ])
        .current_dir(&session.project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                CliError::ServerBinaryNotFound(server_binary)
            } else {
                CliError::SpawnServer {
                    session: session.name.clone(),
                    source,
                }
            }
        })?;
    Ok(())
}

/// Returns only after this project session owns a live server socket.
pub async fn ensure_server_running(session: &ProjectSession) -> Result<(), CliError> {
    if is_session_live(&session.socket_path) {
        return Ok(());
    }
    spawn_server_detached(session)?;
    let deadline = Instant::now() + SERVER_START_TIMEOUT;
    while Instant::now() < deadline {
        if is_session_live(&session.socket_path) {
            return Ok(());
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
/// `--fresh` invocation can never terminate another project's session.
///
/// `SIGTERM` deliberately leaves the project snapshot in place. The newly
/// spawned server restores that snapshot, retaining the session layout while
/// replacing the server executable during development.
pub async fn replace_server(session: &ProjectSession) -> Result<(), CliError> {
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
    ensure_server_running(session).await
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
/// connection failure means the filesystem entry is stale; normal session
/// discovery removes it and reports that no server needs replacing.
async fn server_process_id(socket_path: &Path) -> Result<Option<u32>, CliError> {
    let stream = match tokio::net::UnixStream::connect(socket_path).await {
        Ok(stream) => stream,
        Err(_) => {
            let _ = std::fs::remove_file(socket_path);
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

/// Sends the normal Unix termination signal to one server process identified
/// by its live socket peer credentials.
fn terminate_server(server_pid: u32) -> Result<(), CliError> {
    let result = unsafe { libc::kill(server_pid as libc::pid_t, libc::SIGTERM) };
    if result == 0 {
        return Ok(());
    }
    Err(CliError::ServerTermination {
        pid: server_pid,
        source: std::io::Error::last_os_error(),
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
    let result = unsafe { libc::kill(process_id as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
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
        assert!(first_session.snapshot_path.starts_with(&first));
        assert!(second_session.snapshot_path.starts_with(&second));
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
    fn stale_socket_is_not_live_and_is_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stale_path = dir.path().join("stale.sock");
        std::fs::write(&stale_path, b"not a socket").expect("write stale file");
        assert!(!is_session_live(&stale_path));
        assert!(!stale_path.exists());
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
}
