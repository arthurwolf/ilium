//! Compacts a Claude Code or Codex session transcript (ported from the kernel `ilium-remote-compaction` crate).
//!
//! The module is pure: it takes the transcript bytes the host read (`agent.transcript`, `fs`), asks an injected
//! [`Summarizer`] to condense the older part of the conversation and returns the rewritten transcript bytes in the
//! agent's own compaction format, so the agent resumes the same session with a small context. It never touches a
//! file, a clock or a random source: the host passes the wall clock (`now_millis`) and a record-id seed, takes the
//! backup and writes the returned bytes back.
//!
//! Safety properties, in order of importance:
//! - the plugin never writes anything; any error leaves the original untouched;
//! - the outcome names the snapshot (`source_len`, `source_last_line_hash`) the rewrite was computed from, so the
//!   host can refuse to overwrite a transcript that changed after it was read (the agent is still alive);
//! - the appended records are re-read and structurally verified before the bytes are returned.

mod compaction;
mod error;
mod input;
mod ledger;
mod neutral;
mod pause;
pub mod prompts;
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

pub use writer_common::{backup_file_name, backups_to_prune, BackupEntry};

/// Identity of a transcript at the moment it was read: its byte length and a hash of its final line. The host
/// compares it with the file's current bytes (through this same function) right before it writes the rewrite
/// back, and refuses when they differ, because the agent is still appending to it.
pub fn transcript_snapshot(bytes: &[u8]) -> (u64, u64) {
    let snapshot = transcript_io::FileSnapshot::of(bytes);
    (snapshot.len, snapshot.last_line_hash)
}

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
