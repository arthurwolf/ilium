//! Compacts a Claude Code or Codex session transcript in place.
//!
//! The crate is pure: it reads the transcript file it is handed, asks an
//! injected [`Summarizer`] to condense the older part of the conversation,
//! and rewrites the file in the agent's own compaction format so that the
//! agent resumes the same session with a small context. It has no async
//! runtime, network, PTY or UI dependency.
//!
//! Safety properties, in order of importance:
//! - the original file is never modified unless the new transcript was fully
//!   written, flushed and re-read successfully; any error leaves it untouched;
//! - a backup of the original is kept next to it (`*.pre-compaction-*.bak`);
//! - a transcript that changed after it was read (the agent is still alive) is
//!   refused rather than overwritten.

mod compaction;
mod error;
mod input;
mod ledger;
mod neutral;
mod pause;
mod redact;
mod report;
mod summarize;
mod technique;
mod tokens;
mod transcript_io;
mod types;
mod write_claude;
mod write_codex;
mod writer_common;

pub use compaction::compact_session;
pub use error::{CompactionError, SummarizerError};
pub use pause::transcript_is_at_pause_point;
pub use technique::{
    clean_summary, effective_technique, render_merge_prompt, render_prompt, validate_summary,
    PromptInputs, RenderedPrompt,
};
pub use tokens::{estimate_tokens, latest_context_usage};
pub use types::{
    AgentKind, CompactionEvent, CompactionOptions, CompactionOutcome, CompactionRequest,
    Summarizer, SummaryRequest, SummaryResponse, Technique, TokenBreakdown,
};
