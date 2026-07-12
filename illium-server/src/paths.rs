//! Resolves the real, platform-correct config/data/runtime paths this
//! server uses when run as an actual daemon (as opposed to a test, which
//! constructs [`crate::ServerOptions`] directly with tempdir paths and
//! never calls into this module -- see `CLAUDE.md`'s "Config & data
//! locations", which this module exists solely to satisfy: never hardcode
//! `~`, always go through `directories::ProjectDirs`/`BaseDirs`.

use std::path::{Path, PathBuf};

use directories::{BaseDirs, ProjectDirs};

use crate::error::ServerError;

/// Linux's `sockaddr_un.sun_path` buffer is 108 bytes total, including the
/// trailing NUL terminator the OS requires. This is set well below that
/// hard cap rather than right up against it, so the check stays meaningful
/// even for a moderately long session name on top of an already-deep
/// runtime/data directory, not just a pathological worst case.
const MAX_SOCKET_PATH_BYTES: usize = 100;

/// The three real filesystem locations one running session needs:
/// - `config_dir`: shared across all sessions, holds `config.toml`.
/// - `socket_path`: this session's own UDS. Prefers
///   `$XDG_RUNTIME_DIR/illium/<session>.sock` (short by construction --
///   see [`resolve_socket_path`]) and falls back to
///   `<data_dir>/<session>.sock` only when the platform has no runtime
///   dir.
/// - `snapshot_path`: this session's own crash-recovery JSON snapshot,
///   `<data_dir>/<session>.snapshot.json` -- always under `data_dir`; JSON
///   files on disk have no `sun_path`-style length cap, so this one never
///   needs the runtime-dir treatment.
pub struct SessionPaths {
    pub config_dir: PathBuf,
    pub socket_path: PathBuf,
    pub snapshot_path: PathBuf,
}

/// Resolves [`SessionPaths`] for `session_name` and ensures the data
/// directory exists (the snapshot lives there, and so does the socket when
/// falling back -- see [`resolve_socket_path`]). Does not create
/// `config_dir` -- an absent config directory means "no config.toml",
/// which [`crate::config::load`] already treats as "use defaults", not an
/// error.
pub fn resolve(session_name: &str) -> Result<SessionPaths, ServerError> {
    let project_dirs = ProjectDirs::from("", "", "illium").ok_or(ServerError::NoProjectDirs)?;
    let data_dir = project_dirs.data_dir();
    std::fs::create_dir_all(data_dir)?;

    let socket_path = resolve_socket_path(session_name, data_dir)?;

    Ok(SessionPaths {
        config_dir: project_dirs.config_dir().to_path_buf(),
        socket_path,
        snapshot_path: data_dir.join(format!("{session_name}.snapshot.json")),
    })
}

/// Picks the shortest sane home for this session's UDS.
///
/// Unix domain socket paths are capped at roughly 108 bytes on Linux (the
/// `sockaddr_un.sun_path` buffer, exposed by `std` as "path must be
/// shorter than `SUN_LEN`"). `<data_dir>/<session>.sock` -- the previous,
/// only, socket location -- lives under `XDG_DATA_HOME`, which has no such
/// constraint and routinely nests several directories deep; combined with
/// a longish session name that silently overflows the buffer and `bind()`
/// fails with a cryptic OS error.
///
/// Prefer `$XDG_RUNTIME_DIR/illium/<session>.sock` instead:
/// `BaseDirs::runtime_dir` (typically `/run/user/<uid>` on Linux,
/// systemd/pam-managed and guaranteed short) is exactly the OS-standard
/// place for exactly this kind of ephemeral, session-scoped socket. Fall
/// back to `<data_dir>/<session>.sock` only when the platform exposes no
/// runtime dir at all (non-Linux, or no session manager set one).
///
/// Either way, validate the final path's byte length against
/// [`MAX_SOCKET_PATH_BYTES`] before handing it back, so a too-deep
/// `XDG_RUNTIME_DIR`/`XDG_DATA_HOME` or an unreasonably long session name
/// fails here with [`ServerError::SocketPathTooLong`] -- a clear,
/// actionable error -- rather than as a raw OS `bind()` failure once
/// `illium_server::run` tries to use the path.
fn resolve_socket_path(session_name: &str, data_dir: &Path) -> Result<PathBuf, ServerError> {
    let candidate = match BaseDirs::new().as_ref().and_then(BaseDirs::runtime_dir) {
        Some(runtime_dir) => {
            let socket_dir = runtime_dir.join("illium");
            std::fs::create_dir_all(&socket_dir)?;
            socket_dir.join(format!("{session_name}.sock"))
        }
        None => data_dir.join(format!("{session_name}.sock")),
    };

    validate_socket_path_length(candidate)
}

/// Rejects a socket path at or over [`MAX_SOCKET_PATH_BYTES`] with a
/// [`ServerError::SocketPathTooLong`] naming the actual byte length and
/// the limit, instead of letting a too-long path reach `bind()` and
/// surface as an unguided OS error.
fn validate_socket_path_length(path: PathBuf) -> Result<PathBuf, ServerError> {
    let byte_length = path.as_os_str().len();
    if byte_length >= MAX_SOCKET_PATH_BYTES {
        return Err(ServerError::SocketPathTooLong {
            path,
            byte_length,
            max_byte_length: MAX_SOCKET_PATH_BYTES,
        });
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// When a runtime dir is available, `resolve_socket_path` must use it
    /// (`<runtime_dir>/illium/<session>.sock`), never the data dir --
    /// that's the whole point of preferring the short, OS-standard
    /// location.
    #[test]
    fn prefers_runtime_dir_when_available() {
        let temp_dir =
            std::env::temp_dir().join(format!("illium-paths-test-runtime-{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).expect("failed to create temp runtime dir");
        // SAFETY: this test process does not read `XDG_RUNTIME_DIR` from
        // any other thread concurrently; `directories::BaseDirs::new` is
        // called synchronously right after this within the same test.
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", &temp_dir);
        }

        let socket_path = resolve_socket_path("my-session", Path::new("/nonexistent/data-dir"))
            .expect("resolving a short session name under a temp runtime dir must succeed");

        unsafe {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        let _ = std::fs::remove_dir_all(&temp_dir);

        assert!(
            socket_path.starts_with(&temp_dir),
            "expected {socket_path:?} to live under the runtime dir {temp_dir:?}"
        );
        assert_eq!(socket_path, temp_dir.join("illium").join("my-session.sock"));
    }

    /// With no runtime dir available at all, `resolve` (not just the
    /// socket helper, since only `resolve` has a `data_dir` to fall back
    /// to) must still land the socket under `data_dir`, matching the
    /// pre-fix behavior for platforms/environments with no runtime dir.
    #[test]
    fn falls_back_to_data_dir_without_runtime_dir() {
        // A short, deterministic stand-in for `<data_dir>` -- resolve()
        // itself is not exercised here since it always goes through the
        // real `ProjectDirs`, which this test cannot control; the fallback
        // branch inside `resolve_socket_path` is what's under test.
        let data_dir = std::env::temp_dir().join("illium-paths-test-data-dir");

        // SAFETY: same single-threaded-within-this-test justification as
        // the runtime-dir test above. `BaseDirs::runtime_dir` on Linux only
        // ever comes from `XDG_RUNTIME_DIR` (there is no non-XDG
        // fallback), so clearing just that is enough to force the
        // fallback branch.
        let previous_runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
        unsafe {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }

        let socket_path = resolve_socket_path("my-session", &data_dir)
            .expect("resolving a short session name under a short data dir must succeed");

        if let Some(runtime_dir) = previous_runtime_dir {
            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", runtime_dir);
            }
        }

        assert_eq!(socket_path, data_dir.join("my-session.sock"));
    }

    /// A deliberately too-long candidate path must be rejected with the
    /// typed, actionable error -- never panic, and never let a raw OS
    /// error string with no guidance be the only signal.
    #[test]
    fn rejects_a_path_at_or_over_the_length_limit() {
        let long_component = "a".repeat(MAX_SOCKET_PATH_BYTES);
        let too_long_path = PathBuf::from("/tmp").join(long_component).join("s.sock");

        let result = validate_socket_path_length(too_long_path.clone());

        match result {
            Err(ServerError::SocketPathTooLong {
                path,
                byte_length,
                max_byte_length,
            }) => {
                assert_eq!(path, too_long_path);
                assert_eq!(byte_length, too_long_path.as_os_str().len());
                assert_eq!(max_byte_length, MAX_SOCKET_PATH_BYTES);
                assert!(byte_length >= max_byte_length);
            }
            other => panic!("expected ServerError::SocketPathTooLong, got {other:?}"),
        }
    }

    /// A path right at the boundary (exactly `MAX_SOCKET_PATH_BYTES`
    /// bytes) is rejected too -- the check is `>=`, matching the "leave
    /// headroom" intent documented on the constant, not an off-by-one
    /// `>`.
    #[test]
    fn rejects_a_path_exactly_at_the_limit() {
        let path = PathBuf::from("a".repeat(MAX_SOCKET_PATH_BYTES));
        assert_eq!(path.as_os_str().len(), MAX_SOCKET_PATH_BYTES);

        let result = validate_socket_path_length(path);

        assert!(matches!(result, Err(ServerError::SocketPathTooLong { .. })));
    }

    /// A comfortably short path must pass through unchanged.
    #[test]
    fn accepts_a_short_path() {
        let path = PathBuf::from("/run/user/1000/illium/main.sock");
        let result =
            validate_socket_path_length(path.clone()).expect("a short path must be accepted");
        assert_eq!(result, path);
    }
}
