//! Converts one agent session between Claude Code and Codex transcript
//! formats so `claude --resume <id>` or `codex resume <id>` continues the
//! conversation.
//!
//! * Claude Code -> Codex runs Codex's own importer over a private
//!   `codex app-server` child ([`claude_to_codex`]).
//! * Codex -> Claude Code is converted here ([`codex_to_claude`]): user
//!   prompts, assistant text and tool calls with their results are kept;
//!   reasoning, token events and harness context are dropped.
//!
//! [`convert_session`] blocks, emits [`ConversionEvent`]s as it goes and
//! polls its cancel flag between steps and while waiting on the subprocess.

mod app_server;
mod claude_to_codex;
mod claude_writer;
mod codex_rollout;
mod codex_to_claude;
mod error;
mod paths;
mod report;

#[cfg(all(test, unix))]
mod fake_server_tests;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use ilium_core::{AgentProvider, BuiltinAgentProvider};

pub use error::ConvertError;

use report::Reporter;

/// Hard ceiling for source transcripts, parser buffers, and generated output.
/// Reads use this limit before allocating complete transcript strings.
pub(crate) const MAX_TRANSCRIPT_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) const MAX_IMPORT_LEDGER_BYTES: u64 = 16 * 1024 * 1024;

/// What to convert and where the agents keep their data.
#[derive(Debug, Clone)]
pub struct ConversionRequest {
    /// User home (contains `.claude/` and `.codex/`).
    pub home_dir: PathBuf,
    /// Canonical cwd of the pane: scopes transcript lookup and becomes the cwd
    /// of the new session.
    pub project_cwd: PathBuf,
    /// Claude or Codex.
    pub source: BuiltinAgentProvider,
    /// The other one; any other combination is [`ConvertError::Unsupported`].
    pub target: BuiltinAgentProvider,
    pub source_session_id: String,
    /// `None` => `$CODEX_HOME` or `<home_dir>/.codex`; `Some` => exported as
    /// `CODEX_HOME` to the spawned codex (used by tests).
    pub codex_home: Option<PathBuf>,
    /// `None` => resolve `codex` on `PATH`.
    pub codex_executable: Option<PathBuf>,
}

/// Progress a UI can render as a step list, a log and a progress bar.
#[derive(Debug, Clone, PartialEq)]
pub enum ConversionEvent {
    /// A step begins; `index` is 1-based.
    Step {
        index: usize,
        total: usize,
        title: String,
    },
    /// One human-readable line, no trailing newline.
    Log(String),
    /// Overall progress, `0.0..=1.0`, monotonic non-decreasing.
    Progress(f32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionOutcome {
    /// UUID usable with the target's resume command.
    pub new_session_id: String,
    pub target_transcript_path: PathBuf,
    /// Source transcript items that were carried over.
    pub converted_items: usize,
    /// Source transcript items that were intentionally left out.
    pub dropped_items: usize,
}

/// Converts one session. See the crate documentation.
///
/// `cancel` is polled between steps and while waiting on the codex
/// subprocess; once set the subprocess is killed and
/// [`ConvertError::Cancelled`] is returned. A Codex -> Claude cancellation
/// leaves no target transcript. A Claude -> Codex cancellation cannot undo a
/// thread Codex itself had already created: that file belongs to Codex.
pub fn convert_session(
    request: &ConversionRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(ConversionEvent),
) -> Result<ConversionOutcome, ConvertError> {
    convert_session_with_cancel(
        request,
        &|| cancel.load(std::sync::atomic::Ordering::Relaxed),
        sink,
    )
}

/// Converts while consulting an owner-provided cancellation predicate at each
/// domain checkpoint. This lets execution-bank shutdown reach the same cleanup
/// paths as explicit user cancellation.
pub fn convert_session_with_cancel(
    request: &ConversionRequest,
    is_cancelled: &dyn Fn() -> bool,
    sink: &mut dyn FnMut(ConversionEvent),
) -> Result<ConversionOutcome, ConvertError> {
    let direction = match (request.source, request.target) {
        (BuiltinAgentProvider::Claude, BuiltinAgentProvider::Codex) => Direction::ClaudeToCodex,
        (BuiltinAgentProvider::Codex, BuiltinAgentProvider::Claude) => Direction::CodexToClaude,
        (from, to) => {
            return Err(ConvertError::Unsupported {
                from: from.label(),
                to: to.label(),
            })
        }
    };
    if !looks_like_uuid(&request.source_session_id) {
        return Err(ConvertError::InvalidSessionId(
            request.source_session_id.clone(),
        ));
    }
    let total_steps = match direction {
        Direction::ClaudeToCodex => claude_to_codex::TOTAL_STEPS,
        Direction::CodexToClaude => codex_to_claude::TOTAL_STEPS,
    };
    let mut reporter = Reporter::new(sink, is_cancelled, total_steps);
    reporter.log(format!(
        "Converting {} session {} to {} in {}",
        request.source.label(),
        request.source_session_id,
        request.target.label(),
        request.project_cwd.display()
    ));
    reporter.check_cancel()?;
    match direction {
        Direction::ClaudeToCodex => claude_to_codex::convert(
            request,
            &mut reporter,
            &claude_to_codex::Timeouts::default(),
        ),
        Direction::CodexToClaude => codex_to_claude::convert(request, &mut reporter),
    }
}

enum Direction {
    ClaudeToCodex,
    CodexToClaude,
}

/// Session ids are UUID-shaped (Codex uses v7, Claude v4); the check keeps
/// arbitrary text out of filesystem lookups.
fn looks_like_uuid(value: &str) -> bool {
    const HYPHEN_INDICES: [usize; 4] = [8, 13, 18, 23];
    value.len() == 36
        && value.chars().enumerate().all(|(index, character)| {
            if HYPHEN_INDICES.contains(&index) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        })
}
