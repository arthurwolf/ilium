//! Resolves this client's view of a session's UDS socket path -- the same
//! `<data_dir>/<session>.sock` convention `illium_server::paths` computes
//! independently server-side (see the workspace `CLAUDE.md`'s "Config &
//! data locations"). Small and stable enough that both sides deriving it
//! from `directories::ProjectDirs` themselves is simpler than introducing
//! a shared crate just for one path formula, and keeps illium-client from
//! depending on illium-server's internal types per the workspace's
//! layering rules (`illium-client` never reaches into server-internal
//! types directly).

use std::path::PathBuf;

use directories::ProjectDirs;

use crate::error::ClientError;

/// This session's Unix domain socket path, `<data_dir>/<session_name>.sock`.
pub fn socket_path(session_name: &str) -> Result<PathBuf, ClientError> {
    let project_dirs = ProjectDirs::from("", "", "illium").ok_or(ClientError::NoProjectDirs)?;
    Ok(project_dirs.data_dir().join(format!("{session_name}.sock")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_ends_with_the_session_name_and_sock_extension() {
        let path = socket_path("my-session").expect("project dirs should resolve on this OS");
        assert_eq!(path.file_name().unwrap(), "my-session.sock");
    }
}
