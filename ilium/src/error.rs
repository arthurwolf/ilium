//! Top-level typed errors for the ilium CLI wrapper.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("--cwd {0:?} is not a valid directory")]
    InvalidCwd(PathBuf),
    #[error("session name {0:?} must contain only letters, digits, hyphens, or underscores")]
    InvalidSessionName(String),
    #[error("failed to create or read project session storage at {path:?}: {source}")]
    SessionStorage {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("project session socket path is too long: {0:?}")]
    SocketPathTooLong(PathBuf),
    #[error(
        "could not locate the ilium-server binary next to {0:?} or on PATH -- \
         run `cargo build --workspace` so both binaries land in the same directory"
    )]
    ServerBinaryNotFound(PathBuf),
    #[error("failed to spawn ilium-server for session {session:?}: {source}")]
    SpawnServer {
        session: String,
        source: std::io::Error,
    },
    #[error("session {0:?}'s server did not become ready within {1:?}")]
    ServerStartTimeout(String, Duration),
    #[error("could not determine the process that owns session socket {path:?}: {source}")]
    ServerProcessLookup {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to terminate ilium-server process {pid}: {source}")]
    ServerTermination {
        pid: u32,
        #[source]
        source: std::io::Error,
    },
    #[error("ilium-server process {pid} did not stop within {timeout:?}")]
    ServerProcessStopTimeout { pid: u32, timeout: Duration },
    #[error("session {0:?} is not running")]
    SessionNotRunning(String),
    #[error("the server reported an error: {0}")]
    ServerReportedError(String),
    #[error(transparent)]
    Connection(#[from] ilium_client::connection::ConnectionError),
    #[error(transparent)]
    Client(#[from] ilium_client::error::ClientError),
}
