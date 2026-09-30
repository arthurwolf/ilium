//! Every way a conversion can fail. The `Display` text of each variant is a
//! single line meant to be shown to a person as-is.

use std::io;
use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum ConvertError {
    #[error("converting a {from} session to {to} is not supported")]
    Unsupported {
        from: &'static str,
        to: &'static str,
    },
    #[error("`{0}` is not a valid session id (expected a UUID)")]
    InvalidSessionId(String),
    #[error("no {agent} transcript for session {session_id} was found for project {}", project.display())]
    SourceNotFound {
        agent: &'static str,
        session_id: String,
        project: PathBuf,
    },
    #[error("cannot read the source transcript {}: {error}", path.display())]
    SourceUnreadable {
        path: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("the source transcript {} holds no conversation that can be converted", path.display())]
    EmptyConversation { path: PathBuf },
    #[error("cannot write the converted transcript {}: {error}", path.display())]
    TargetWrite {
        path: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("the converted transcript failed verification: {0}")]
    TargetVerification(String),
    #[error("cannot start `{executable}`: {error}")]
    CodexUnavailable {
        executable: String,
        #[source]
        error: io::Error,
    },
    #[error("codex app-server protocol error: {0}")]
    CodexProtocol(String),
    #[error("codex app-server exited before the import finished: {0}")]
    CodexExited(String),
    #[error("codex app-server did not finish `{stage}` within {seconds} s")]
    CodexTimeout { stage: &'static str, seconds: u64 },
    #[error("the Codex importer failed: {0}")]
    ImportFailed(String),
    #[error("the Codex importer skipped the session: {0}")]
    ImportSkipped(String),
    #[error("conversion cancelled")]
    Cancelled,
}

/// Collapses arbitrary text (process stderr, JSON-RPC messages) into one
/// bounded line so it can be embedded in an error `Display`.
pub(crate) fn single_line(text: &str, limit: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= limit {
        return collapsed;
    }
    let mut shortened: String = collapsed.chars().take(limit).collect();
    shortened.push('…');
    shortened
}
