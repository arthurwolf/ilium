//! Resolves this client's view of a session's UDS socket path -- the same
//! formula `illium_server::paths::resolve` computes independently
//! server-side when it actually binds the socket (see the workspace
//! `CLAUDE.md`'s "Config & data locations"). Small and stable enough that
//! both sides deriving it from `directories::BaseDirs`/`ProjectDirs`
//! themselves is simpler than introducing a shared crate just for one path
//! formula, and keeps illium-client from depending on illium-server's
//! internal types per the workspace's layering rules (`illium-client`
//! never reaches into server-internal types directly). This formula MUST
//! stay in sync with `illium_server::paths::resolve_socket_path` -- update
//! both together.

use std::path::PathBuf;

use directories::{BaseDirs, ProjectDirs};

use crate::error::ClientError;

/// This session's Unix domain socket path: `$XDG_RUNTIME_DIR/illium/
/// <session_name>.sock` when a runtime dir is available (the common case
/// on any systemd/pam-managed Linux session -- `illium_server::paths`
/// prefers it for the same reason: `sockaddr_un.sun_path` is capped at
/// roughly 108 bytes, and the runtime dir is guaranteed short where
/// `data_dir` routinely isn't), falling back to `<data_dir>/
/// <session_name>.sock` only when the platform exposes no runtime dir at
/// all.
pub fn socket_path(session_name: &str) -> Result<PathBuf, ClientError> {
    match BaseDirs::new().as_ref().and_then(BaseDirs::runtime_dir) {
        Some(runtime_dir) => Ok(runtime_dir
            .join("illium")
            .join(format!("{session_name}.sock"))),
        None => {
            let project_dirs =
                ProjectDirs::from("", "", "illium").ok_or(ClientError::NoProjectDirs)?;
            Ok(project_dirs.data_dir().join(format!("{session_name}.sock")))
        }
    }
}

/// This client's config directory, `~/.config/illium/` on Linux -- the
/// same `directories::ProjectDirs` app-id `illium-server` resolves
/// independently for its own `config.toml` (see the workspace
/// `CLAUDE.md`'s "Config & data locations"). Holds `config.toml`'s
/// client-side tables (`[keybindings]`, `[theme]`) -- see
/// `crate::config::load`.
pub fn config_dir() -> Result<PathBuf, ClientError> {
    let project_dirs = ProjectDirs::from("", "", "illium").ok_or(ClientError::NoProjectDirs)?;
    Ok(project_dirs.config_dir().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_ends_with_the_session_name_and_sock_extension() {
        let path = socket_path("my-session").expect("project dirs should resolve on this OS");
        assert_eq!(path.file_name().unwrap(), "my-session.sock");
    }

    /// Mirrors `illium_server::paths`'s own `prefers_runtime_dir_when_available`
    /// test: when a runtime dir is set, this must land at the exact same
    /// path the server actually binds, or the client can never connect to
    /// a server it (or the CLI) just spawned.
    #[test]
    fn prefers_runtime_dir_when_available() {
        let temp_dir = std::env::temp_dir().join(format!(
            "illium-client-paths-test-runtime-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp_dir).expect("failed to create temp runtime dir");

        // SAFETY: this test process does not read `XDG_RUNTIME_DIR` from
        // any other thread concurrently; `directories::BaseDirs::new` is
        // called synchronously right after this within the same test.
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", &temp_dir);
        }

        let path = socket_path("my-session").expect("resolving under a temp runtime dir");

        unsafe {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        let _ = std::fs::remove_dir_all(&temp_dir);

        assert_eq!(path, temp_dir.join("illium").join("my-session.sock"));
    }
}
