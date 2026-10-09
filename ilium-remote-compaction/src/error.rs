//! Every way a compaction can fail. Each `Display` text is a single line meant
//! to be shown to a person as-is. The original transcript is untouched by any
//! of them.

use std::path::PathBuf;

/// Failure reported by a [`crate::Summarizer`] implementation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SummarizerError {
    /// The request does not fit the model's input window.
    #[error("the summarizer input is too long: {0}")]
    ContextTooLong(String),
    /// The call failed for any other reason (network, quota, provider error).
    #[error("the summarizer failed: {0}")]
    Failed(String),
    /// The caller cancelled the call.
    #[error("the summarizer call was cancelled")]
    Cancelled,
}

#[derive(Debug, thiserror::Error)]
pub enum CompactionError {
    #[error("compaction cancelled")]
    Cancelled,
    #[error("cannot read the transcript {}: {error}", path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },
    #[error("the transcript {} is larger than the {max_bytes}-byte compaction limit", path.display())]
    TooLarge { path: PathBuf, max_bytes: u64 },
    #[error("parsing the transcript {} would exceed the {max_bytes}-byte memory budget", path.display())]
    ParseMemoryLimit { path: PathBuf, max_bytes: usize },
    #[error("cannot write {}: {error}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },
    #[error("the transcript {} has no conversation to compact beyond what is already summarized or kept as the recent tail", path.display())]
    NothingToCompact { path: PathBuf },
    #[error("the transcript {} changed while it was being compacted; the agent may still be running", path.display())]
    TranscriptChanged { path: PathBuf },
    #[error("the Codex rollout {} has an inconsistent `ordinal` on its last line", path.display())]
    MissingOrdinal { path: PathBuf },
    #[error("the transcript {} has no usable record to continue from: {reason}", path.display())]
    NoAnchorRecord { path: PathBuf, reason: String },
    #[error("could not render a compaction prompt: {0}")]
    Prompt(String),
    #[error("the rewritten transcript failed verification: {0}")]
    Verification(String),
}

impl From<ilium_prompts::PromptError> for CompactionError {
    fn from(error: ilium_prompts::PromptError) -> Self {
        Self::Prompt(error.to_string())
    }
}
