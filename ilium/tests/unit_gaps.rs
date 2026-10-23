//! Unit-gap tests for the public surface of the `ilium` library target:
//! `error::CliError`, `CliError::received_server_error`, and the project
//! session helpers in `session`.
//!
//! Everything else in this crate is `pub(crate)` inside the binary target and
//! cannot be named from an integration test, so those inventory items are
//! reported as skipped instead of being tested here.
//!
//! The session helpers resolve sockets and debug logs from `ILIUM_SOCKET_DIR`
//! and `ILIUM_DEBUG_LOG_DIR`. Those variables are process-wide, so every test
//! that touches them holds one shared lock and points both at its own
//! temporary root.
//!
//! The socket tests use Unix domain sockets, so the whole file is Unix-only.

#![cfg(unix)]

use std::error::Error as _;
use std::future::Future;
use std::io;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ilium::error::CliError;
use ilium::session::{self, ProjectSession, DEFAULT_SESSION_NAME};
use ilium_client::connection::{ConnectionError, Received};
use ilium_client::error::ClientError;
use ilium_logging::LoggingError;

static ENVIRONMENT_LOCK: Mutex<()> = Mutex::new(());

/// A temporary root with isolated socket and debug-log directories.
///
/// `root` is declared before `_guard` so the directories are removed while the
/// environment lock is still held.
struct Isolated {
    root: tempfile::TempDir,
    _guard: MutexGuard<'static, ()>,
}

impl Isolated {
    fn new() -> Self {
        let guard = ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let root = tempfile::tempdir().expect("create temporary test root");
        std::env::set_var("ILIUM_SOCKET_DIR", root.path().join("sockets"));
        std::env::set_var("ILIUM_DEBUG_LOG_DIR", root.path().join("logs"));
        Self {
            root,
            _guard: guard,
        }
    }

    fn socket_dir(&self) -> PathBuf {
        self.root.path().join("sockets")
    }

    /// The canonical path of a project directory created under the root.
    fn project(&self) -> PathBuf {
        let project = self.root.path().join("project");
        std::fs::create_dir_all(&project).expect("create project directory");
        std::fs::canonicalize(&project).expect("canonicalize project directory")
    }

    fn session(&self, project: &Path, name: &str) -> ProjectSession {
        session::resolve_project_session(project, name).expect("resolve project session")
    }
}

fn io_error(message: &str) -> io::Error {
    io::Error::other(message)
}

fn write_snapshot(project: &Path, name: &str) {
    let directory = project.join(".ilium").join("sessions");
    std::fs::create_dir_all(&directory).expect("create snapshot directory");
    std::fs::write(directory.join(format!("{name}.json")), "{}").expect("write snapshot");
}

/// Binds a listener at the session's socket path. Connections succeed while
/// it is alive, which is what the liveness probe treats as a running server.
fn listen(session: &ProjectSession) -> UnixListener {
    UnixListener::bind(&session.socket_path).expect("bind live session socket")
}

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Runtime::new()
        .expect("build tokio runtime")
        .block_on(future)
}

// ---------------------------------------------------------------------------
// error::CliError
// ---------------------------------------------------------------------------

#[test]
fn cli_error_invalid_cwd_names_the_rejected_path() {
    let error = CliError::InvalidCwd(PathBuf::from("/no/such dir"));
    assert_eq!(
        error.to_string(),
        "--cwd \"/no/such dir\" is not a valid directory"
    );
}

#[test]
fn cli_error_invalid_session_name_quotes_the_name_and_lists_allowed_characters() {
    let error = CliError::InvalidSessionName("bad name".to_string());
    assert_eq!(
        error.to_string(),
        "session name \"bad name\" must contain only letters, digits, hyphens, or underscores"
    );
}

#[test]
fn cli_error_session_storage_names_path_and_cause() {
    let error = CliError::SessionStorage {
        path: PathBuf::from("/srv/sessions"),
        source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
    };
    assert_eq!(
        error.to_string(),
        "failed to create or read project session storage at \"/srv/sessions\": denied"
    );
}

#[test]
fn cli_error_socket_path_too_long_names_the_path() {
    let error = CliError::SocketPathTooLong(PathBuf::from("/run/ilium/long.sock"));
    assert_eq!(
        error.to_string(),
        "project session socket path is too long: \"/run/ilium/long.sock\""
    );
}

#[test]
fn cli_error_server_binary_not_found_suggests_the_workspace_build() {
    let error = CliError::ServerBinaryNotFound(PathBuf::from("/opt/ilium"));
    assert_eq!(
        error.to_string(),
        "could not locate the ilium-server binary next to \"/opt/ilium\" or on PATH -- \
         run `cargo build --workspace` so both binaries land in the same directory"
    );
}

#[test]
fn cli_error_spawn_server_names_session_and_cause() {
    let error = CliError::SpawnServer {
        session: "default".to_string(),
        source: io_error("boom"),
    };
    assert_eq!(
        error.to_string(),
        "failed to spawn ilium-server for session \"default\": boom"
    );
}

#[test]
fn cli_error_server_start_timeout_formats_session_and_duration() {
    let error = CliError::ServerStartTimeout("default".to_string(), Duration::from_secs(5));
    assert_eq!(
        error.to_string(),
        "session \"default\"'s server did not become ready within 5s"
    );
}

#[test]
fn cli_error_session_log_metadata_names_marker_path_and_cause() {
    let error = CliError::SessionLogMetadata {
        path: PathBuf::from("/logs/.active-log-path"),
        source: io_error("boom"),
    };
    assert_eq!(
        error.to_string(),
        "failed to resolve session log metadata at \"/logs/.active-log-path\": boom"
    );
}

#[test]
fn cli_error_server_process_lookup_names_socket_and_cause() {
    let error = CliError::ServerProcessLookup {
        path: PathBuf::from("/run/ilium/s.sock"),
        source: io_error("boom"),
    };
    assert_eq!(
        error.to_string(),
        "could not determine the process that owns session socket \"/run/ilium/s.sock\": boom"
    );
}

#[test]
fn cli_error_server_termination_names_the_pid() {
    let error = CliError::ServerTermination {
        pid: 42,
        source: io_error("boom"),
    };
    assert_eq!(
        error.to_string(),
        "failed to terminate ilium-server process 42: boom"
    );
}

#[test]
fn cli_error_server_process_stop_timeout_formats_pid_and_duration() {
    let error = CliError::ServerProcessStopTimeout {
        pid: 42,
        timeout: Duration::from_millis(250),
    };
    assert_eq!(
        error.to_string(),
        "ilium-server process 42 did not stop within 250ms"
    );
}

#[test]
fn cli_error_session_not_running_quotes_the_name() {
    let error = CliError::SessionNotRunning("default".to_string());
    assert_eq!(error.to_string(), "session \"default\" is not running");
}

#[test]
fn cli_error_server_reported_error_prefixes_the_message() {
    let error = CliError::ServerReportedError("bad".to_string());
    assert_eq!(error.to_string(), "the server reported an error: bad");
}

#[test]
fn cli_error_received_server_error_variant_prefixes_the_message() {
    let error =
        CliError::ReceivedServerReportedError(Received::with_retention("bad".to_string(), None));
    assert_eq!(error.to_string(), "the server reported an error: bad");
}

#[test]
fn cli_error_resolve_client_executable_names_the_cause() {
    let error = CliError::ResolveClientExecutable(io_error("boom"));
    assert_eq!(
        error.to_string(),
        "could not resolve the running ilium client executable: boom"
    );
}

#[test]
fn cli_error_restart_client_names_path_and_cause() {
    let error = CliError::RestartClient {
        path: PathBuf::from("/bin/ilium"),
        source: io_error("boom"),
    };
    assert_eq!(
        error.to_string(),
        "failed to restart ilium client from \"/bin/ilium\": boom"
    );
}

#[test]
fn cli_error_chatroom_prefixes_the_wrapped_message() {
    let error = CliError::Chatroom(anyhow::anyhow!("no room"));
    assert_eq!(error.to_string(), "chatroom operation failed: no room");
}

#[test]
fn cli_error_not_inside_pane_names_the_missing_variable() {
    let error = CliError::NotInsideIliumPane("ILIUM_PANE_ID");
    assert_eq!(
        error.to_string(),
        "not running inside an ilium-managed pane: ILIUM_PANE_ID is not set -- \
         `ilium progress` only works when run from inside a pane ilium itself spawned"
    );
}

#[test]
fn cli_error_connection_variant_forwards_the_inner_display() {
    let error = CliError::Connection(ConnectionError::Connect {
        path: PathBuf::from("/run/ilium/s.sock"),
        source: io::Error::new(io::ErrorKind::ConnectionRefused, "refused"),
    });
    assert_eq!(
        error.to_string(),
        "failed to connect to ilium-server socket /run/ilium/s.sock: refused"
    );
}

#[test]
fn cli_error_client_variant_forwards_the_inner_display() {
    let error = CliError::Client(ClientError::NoProjectDirs);
    assert_eq!(
        error.to_string(),
        "could not resolve ilium's data directory (no valid home directory found)"
    );
}

#[test]
fn cli_error_logging_variant_forwards_the_inner_display() {
    let error = CliError::Logging(LoggingError::AlreadyInitialized);
    assert_eq!(
        error.to_string(),
        "file logging is already initialized in this process"
    );
}

#[test]
fn cli_error_exit_status_carries_the_status_byte() {
    let error = CliError::ExitStatus(6);
    assert!(
        matches!(error, CliError::ExitStatus(6)),
        "ExitStatus must carry the status byte it was built with"
    );
}

#[test]
fn cli_error_exit_status_displays_the_status_byte() {
    let error = CliError::ExitStatus(6);
    assert_eq!(error.to_string(), "exit status 6");
}

#[test]
fn cli_error_wrapped_io_errors_expose_their_source() {
    let cases = [
        (
            CliError::SessionStorage {
                path: PathBuf::from("/srv/sessions"),
                source: io_error("storage cause"),
            },
            "storage cause",
        ),
        (
            CliError::SpawnServer {
                session: "default".to_string(),
                source: io_error("spawn cause"),
            },
            "spawn cause",
        ),
        (
            CliError::SessionLogMetadata {
                path: PathBuf::from("/logs/.active-log-path"),
                source: io_error("metadata cause"),
            },
            "metadata cause",
        ),
        (
            CliError::ServerProcessLookup {
                path: PathBuf::from("/run/ilium/s.sock"),
                source: io_error("lookup cause"),
            },
            "lookup cause",
        ),
        (
            CliError::ServerTermination {
                pid: 42,
                source: io_error("termination cause"),
            },
            "termination cause",
        ),
        (
            CliError::ResolveClientExecutable(io_error("resolve cause")),
            "resolve cause",
        ),
        (
            CliError::RestartClient {
                path: PathBuf::from("/bin/ilium"),
                source: io_error("restart cause"),
            },
            "restart cause",
        ),
    ];
    for (error, expected_cause) in cases {
        let source = error
            .source()
            .unwrap_or_else(|| panic!("{error} must expose its io error as source"));
        assert_eq!(
            source.to_string(),
            expected_cause,
            "source of {error} must be the wrapped io error"
        );
        assert!(
            source.downcast_ref::<io::Error>().is_some(),
            "source of {error} must be an io::Error"
        );
    }
}

// ---------------------------------------------------------------------------
// CliError::received_server_error (no-retention branch)
// ---------------------------------------------------------------------------

#[test]
fn received_server_error_without_retention_wraps_the_message() {
    let error = CliError::received_server_error("boom".to_string(), None);
    assert!(
        matches!(&error, CliError::ServerReportedError(message) if message == "boom"),
        "a server error without retention must become ServerReportedError"
    );
    assert_eq!(error.to_string(), "the server reported an error: boom");
}

#[test]
fn received_server_error_empty_message_is_kept_verbatim() {
    let error = CliError::received_server_error(String::new(), None);
    assert!(
        matches!(&error, CliError::ServerReportedError(message) if message.is_empty()),
        "an empty server message must be kept as an empty string"
    );
    assert_eq!(error.to_string(), "the server reported an error: ");
}

#[test]
fn received_server_error_keeps_build_mismatch_hint_verbatim() {
    let message = "connection closed before the correlated progress response arrived; \
                   the running ilium-server may be from a different build"
        .to_string();
    let error = CliError::received_server_error(message.clone(), None);
    assert!(
        matches!(&error, CliError::ServerReportedError(kept) if *kept == message),
        "the build-mismatch hint text must reach the caller unchanged"
    );
}

// ---------------------------------------------------------------------------
// session::ProjectSession
// ---------------------------------------------------------------------------

#[test]
fn project_session_socket_path_matches_the_socket_path_helper() {
    let isolated = Isolated::new();
    let project = isolated.project();
    let session = isolated.session(&project, "alpha");
    let expected = session::socket_path_in(&isolated.socket_dir(), &project, "alpha");
    assert_eq!(
        session.socket_path, expected,
        "the socket path must be derived from the canonical project root and the name"
    );
}

#[test]
fn project_session_default_name_is_applied_to_the_snapshot_path() {
    let isolated = Isolated::new();
    let project = isolated.project();
    let session = isolated.session(&project, DEFAULT_SESSION_NAME);
    assert_eq!(session.name, "default");
    assert_eq!(
        session.snapshot_path,
        project.join(".ilium").join("sessions").join("default.json"),
        "the default session must snapshot to default.json in the project"
    );
}

#[test]
fn project_session_root_is_the_launch_directory_not_an_ancestor() {
    let isolated = Isolated::new();
    let project = isolated.project();
    let nested = project.join("sub");
    std::fs::create_dir_all(&nested).expect("create nested project directory");
    let session = isolated.session(&nested, "alpha");
    assert_eq!(
        session.project_root, nested,
        "the launch directory is the project boundary"
    );
    assert!(
        !session.snapshot_path.starts_with(project.join(".ilium")),
        "a nested launch must not write into the ancestor's .ilium directory"
    );
}

// ---------------------------------------------------------------------------
// session::SessionListing and session::list_sessions
// ---------------------------------------------------------------------------

#[test]
fn session_listing_fields_come_from_the_snapshot_file() {
    let isolated = Isolated::new();
    let project = isolated.project();
    write_snapshot(&project, "alpha");
    let listings = session::list_sessions(&project).expect("list sessions");
    assert_eq!(listings.len(), 1, "one snapshot must give one listing");
    assert_eq!(listings[0].name, "alpha");
    assert!(
        !listings[0].live,
        "a snapshot without a running server is not live"
    );
}

#[test]
fn session_listing_empty_session_directory_lists_nothing() {
    let isolated = Isolated::new();
    let project = isolated.project();
    std::fs::create_dir_all(project.join(".ilium").join("sessions"))
        .expect("create empty session directory");
    let listings = session::list_sessions(&project).expect("list sessions");
    assert!(
        listings.is_empty(),
        "an empty session directory lists nothing"
    );
}

#[test]
fn session_listing_live_socket_without_snapshot_is_not_listed() {
    // Listing is driven by snapshot files; a live server with no snapshot is
    // not reported. This is an interpretation of the inventory case.
    let isolated = Isolated::new();
    let project = isolated.project();
    let session = isolated.session(&project, "ghost");
    let _listener = listen(&session);
    let listings = session::list_sessions(&project).expect("list sessions");
    assert!(
        listings.iter().all(|listing| listing.name != "ghost"),
        "a session without a snapshot file must not appear in the listing"
    );
}

#[test]
fn list_sessions_no_session_directory_returns_empty() {
    let isolated = Isolated::new();
    let project = isolated.project();
    let listings = session::list_sessions(&project).expect("list sessions");
    assert!(
        listings.is_empty(),
        "a project without .ilium/sessions has no sessions"
    );
}

#[test]
fn list_sessions_multiple_sessions_are_sorted_by_name() {
    let isolated = Isolated::new();
    let project = isolated.project();
    for name in ["gamma", "alpha", "beta"] {
        write_snapshot(&project, name);
    }
    let names: Vec<String> = session::list_sessions(&project)
        .expect("list sessions")
        .into_iter()
        .map(|listing| listing.name)
        .collect();
    assert_eq!(names, ["alpha", "beta", "gamma"]);
}

#[test]
fn list_sessions_sockets_that_are_not_live_are_marked_not_live() {
    let isolated = Isolated::new();
    let project = isolated.project();
    write_snapshot(&project, "alpha");
    write_snapshot(&project, "beta");
    let beta = isolated.session(&project, "beta");
    let _listener = listen(&beta);
    let listings = session::list_sessions(&project).expect("list sessions");
    let alpha_live = listings
        .iter()
        .find(|listing| listing.name == "alpha")
        .map(|listing| listing.live);
    let beta_live = listings
        .iter()
        .find(|listing| listing.name == "beta")
        .map(|listing| listing.live);
    assert_eq!(alpha_live, Some(false), "alpha has no server socket");
    assert_eq!(beta_live, Some(true), "beta has a live server socket");
}

// ---------------------------------------------------------------------------
// session::LiveSessionSocket and session::list_live_session_sockets
// ---------------------------------------------------------------------------

#[test]
fn live_session_socket_fields_identify_session_and_socket_path() {
    let isolated = Isolated::new();
    let project = isolated.project();
    let session = isolated.session(&project, "delta");
    let _listener = listen(&session);
    let sockets = session::list_live_session_sockets().expect("list live sockets");
    let found = sockets
        .iter()
        .find(|socket| socket.socket_path == session.socket_path)
        .expect("the live session socket must be listed");
    assert_eq!(
        found.session_name_candidates.first().map(String::as_str),
        Some("delta"),
        "the most likely session name must be the one the socket was created for"
    );
}

#[test]
fn live_session_socket_key_round_trips_through_session_name_candidates() {
    let isolated = Isolated::new();
    let project = isolated.project();
    let key = session::socket_key_in(&isolated.socket_dir(), &project, "epsilon");
    let candidates = session::session_name_candidates(&key);
    assert!(
        candidates.iter().any(|candidate| candidate == "epsilon"),
        "candidates {candidates:?} must recover the session name from the key {key}"
    );
}

#[test]
fn list_live_session_sockets_empty_socket_directory_returns_empty() {
    let _isolated = Isolated::new();
    let sockets = session::list_live_session_sockets().expect("list live sockets");
    assert!(
        sockets.is_empty(),
        "a fresh socket directory has no live sockets"
    );
}

#[test]
fn list_live_session_sockets_stale_socket_is_excluded_and_removed() {
    let isolated = Isolated::new();
    let project = isolated.project();
    let session = isolated.session(&project, "stale");
    {
        let _listener = listen(&session);
    }
    assert!(
        session.socket_path.exists(),
        "dropping the listener leaves its socket file behind"
    );
    let sockets = session::list_live_session_sockets().expect("list live sockets");
    assert!(
        sockets
            .iter()
            .all(|socket| socket.socket_path != session.socket_path),
        "a socket with no listener must not be reported as live"
    );
    assert!(
        !session.socket_path.exists(),
        "the stale socket file must be removed by the liveness check"
    );
}

#[test]
fn list_live_session_sockets_live_socket_is_included() {
    let isolated = Isolated::new();
    let project = isolated.project();
    let session = isolated.session(&project, "live");
    let _listener = listen(&session);
    let sockets = session::list_live_session_sockets().expect("list live sockets");
    assert!(
        sockets
            .iter()
            .any(|socket| socket.socket_path == session.socket_path),
        "a socket with a listener must be reported as live"
    );
}

// ---------------------------------------------------------------------------
// session::ensure_server_running (live-server branch only)
// ---------------------------------------------------------------------------

#[test]
fn ensure_server_running_reuses_a_live_server_without_spawning() {
    let isolated = Isolated::new();
    let project = isolated.project();
    let session = isolated.session(&project, "alpha");
    // The listener lives in this process, so the socket peer is this process.
    // The marker names that PID, which the reuse check verifies.
    let _listener = listen(&session);
    let log_path = session.log_directory.join("log-live.txt");
    std::fs::write(
        &session.active_log_path_file,
        format!(
            "pid={}\nlog_path={}\n",
            std::process::id(),
            log_path.display()
        ),
    )
    .expect("write active log marker");
    let returned =
        block_on(session::ensure_server_running(&session)).expect("live server is reused");
    assert_eq!(
        returned, log_path,
        "a live server must return its published log path without spawning"
    );
}
