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
    #[error("file {} is {bytes} bytes; the maximum supported size is {maximum} bytes", path.display())]
    FileTooLarge {
        path: PathBuf,
        bytes: u64,
        maximum: u64,
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
    let capacity = limit.saturating_mul(4).min(text.len()).saturating_add(3);
    let mut output = String::with_capacity(capacity);
    let mut characters = 0usize;
    let mut pending_space = false;
    let mut truncated = false;
    for character in text.chars() {
        if character.is_whitespace() {
            pending_space |= !output.is_empty();
            continue;
        }
        if pending_space {
            if characters == limit {
                truncated = true;
                break;
            }
            output.push(' ');
            characters += 1;
            pending_space = false;
        }
        if characters == limit {
            truncated = true;
            break;
        }
        output.push(character);
        characters += 1;
    }
    if truncated {
        output.push('…');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::single_line;

    #[test]
    fn single_line_bounds_unicode_without_copying_the_complete_input() {
        assert_eq!(single_line("a\n  b", 3), "a b");
        assert_eq!(single_line("é 🦀 x", 3), "é 🦀…");
        assert_eq!(single_line("only", 0), "…");
    }
}
