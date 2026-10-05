//! The public data types of the crate.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SummarizerError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Claude,
    Codex,
}

/// Which prompt and summary format is used to condense the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Technique {
    ClaudeCode,
    Codex,
    Opencode,
    GeminiCli,
    BestOfAllWorlds,
    Custom,
}

impl Technique {
    pub const ALL: [Technique; 6] = [
        Technique::ClaudeCode,
        Technique::Codex,
        Technique::Opencode,
        Technique::GeminiCli,
        Technique::BestOfAllWorlds,
        Technique::Custom,
    ];

    /// Human-readable name for settings lists.
    pub fn label(self) -> &'static str {
        match self {
            Technique::ClaudeCode => "Claude Code",
            Technique::Codex => "Codex",
            Technique::Opencode => "opencode",
            Technique::GeminiCli => "Gemini CLI",
            Technique::BestOfAllWorlds => "Best of all worlds",
            Technique::Custom => "Custom prompt",
        }
    }

    /// Stable identifier, identical to the serde name.
    pub fn id(self) -> &'static str {
        match self {
            Technique::ClaudeCode => "claude_code",
            Technique::Codex => "codex",
            Technique::Opencode => "opencode",
            Technique::GeminiCli => "gemini_cli",
            Technique::BestOfAllWorlds => "best_of_all_worlds",
            Technique::Custom => "custom",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|technique| technique.id() == id)
    }

    /// The agent's own technique: Claude Code for Claude, Codex for Codex.
    pub fn default_for(agent: AgentKind) -> Self {
        match agent {
            AgentKind::Claude => Technique::ClaudeCode,
            AgentKind::Codex => Technique::Codex,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionOptions {
    pub technique: Technique,
    /// Instruction text of the `Custom` technique (plain text; a literal
    /// `{{conversation}}` is replaced by a pointer to the conversation block).
    pub custom_prompt: Option<String>,
    /// Newest history kept verbatim, in estimated tokens.
    pub tail_tokens: u64,
    /// Newest tool output left intact when preparing the summarizer input.
    pub protected_recent_tool_tokens: u64,
    /// Length of the stub that replaces older tool output.
    pub tool_result_chars: usize,
    pub redact_secrets: bool,
    /// Input window of the summarizer model, used to size chunks.
    pub summarizer_context_tokens: u64,
    pub summarizer_max_output_tokens: u64,
    /// Backups of the transcript kept next to it (at least one is always kept).
    pub keep_backups: usize,
}

impl Default for CompactionOptions {
    fn default() -> Self {
        Self {
            technique: Technique::ClaudeCode,
            custom_prompt: None,
            tail_tokens: 20_000,
            protected_recent_tool_tokens: 40_000,
            tool_result_chars: 2_000,
            redact_secrets: true,
            summarizer_context_tokens: 120_000,
            summarizer_max_output_tokens: 16_000,
            keep_backups: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryRequest {
    pub system: String,
    pub user: String,
    pub max_output_tokens: u64,
    /// Short description for logs and progress, such as `chunk 2/3` or `merge`.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryResponse {
    pub text: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// The model call, injected by the caller. Implementations may block.
pub trait Summarizer {
    fn summarize(&self, request: &SummaryRequest) -> Result<SummaryResponse, SummarizerError>;
}

#[derive(Debug, Clone)]
pub struct CompactionRequest {
    pub agent: AgentKind,
    /// Verified path of the transcript to rewrite.
    pub transcript_path: PathBuf,
    pub session_id: String,
    pub project_cwd: PathBuf,
    pub options: CompactionOptions,
    /// Context window of the agent's model, for the "after" figures.
    pub context_window_tokens: Option<u64>,
}

/// Token figures of one compaction. Everything is an estimate (bytes / 4)
/// unless the field says otherwise.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenBreakdown {
    /// Exact usage stated by the transcript when available, else an estimate.
    pub before_context: u64,
    /// Estimate of the whole active conversation before masking.
    pub conversation_total: u64,
    pub masked_savings: u64,
    /// Summed over all summarizer calls (provider figures when reported).
    pub summarizer_input: u64,
    pub summarizer_output: u64,
    pub tail_kept: u64,
    /// Summary plus tail; the agent's fixed harness overhead is not included.
    pub after_context: u64,
    pub window: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompactionEvent {
    Step {
        index: usize,
        total: usize,
        title: String,
    },
    /// Overall progress from 0 to 1.
    Progress(f32),
    Log(String),
    /// Emitted whenever the breakdown changes.
    Tokens(TokenBreakdown),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionOutcome {
    /// Unchanged: the edit happens in place and the session resumes by id.
    pub session_id: String,
    pub transcript_path: PathBuf,
    pub backup_path: PathBuf,
    pub tokens: TokenBreakdown,
    pub summary_chars: usize,
    pub chunks: usize,
    /// True when the deterministic fallback summary was used.
    pub used_fallback: bool,
    pub redactions: usize,
}
