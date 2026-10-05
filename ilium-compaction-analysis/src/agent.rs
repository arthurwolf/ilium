//! Agent identity and the per-agent constants of the research.
//!
//! Everything that differs between Claude Code and Codex but is not a log
//! format detail (cold-cache thresholds, default windows, grids, priors) lives
//! in one [`AgentProfile`] per agent, so adding an agent is one new entry.

use serde::{Deserialize, Serialize};

/// The coding agents whose transcripts this crate understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// Anthropic Claude Code (`~/.claude/projects/**/*.jsonl`).
    ClaudeCode,
    /// OpenAI Codex CLI (`~/.codex/sessions/**/rollout-*.jsonl`).
    Codex,
}

/// Measured constants of one agent (see the research reports).
#[derive(Debug, Clone, Copy)]
pub struct AgentProfile {
    /// Stable lowercase name.
    pub name: &'static str,
    /// Idle gap after which the prompt cache is assumed cold, main sessions.
    pub cold_gap_seconds_main: u32,
    /// Idle gap after which the prompt cache is assumed cold, subagents.
    pub cold_gap_seconds_subagent: u32,
    /// Context window assumed when the log does not state one.
    pub default_context_window_tokens: u32,
    /// Rework penalty prior, weighted tokens per compaction (one corpus).
    pub rework_prior_weighted_tokens: f64,
    /// Default candidate triggers in tokens.
    pub default_trigger_grid: &'static [u32],
    /// Sessions whose context never exceeded this are not replayed.
    pub replay_min_context_tokens: u32,
}

const CLAUDE_GRID: &[u32] = &[
    120_000, 150_000, 175_000, 200_000, 225_000, 250_000, 300_000, 350_000, 400_000, 450_000,
    500_000, 567_000, 600_000,
];
const CODEX_GRID: &[u32] = &[
    100_000, 110_000, 120_000, 130_000, 140_000, 150_000, 160_000, 170_000, 180_000, 190_000,
    200_000, 210_000, 220_000, 225_000, 232_000,
];

const CLAUDE_PROFILE: AgentProfile = AgentProfile {
    name: "claude_code",
    cold_gap_seconds_main: 3_600,
    cold_gap_seconds_subagent: 300,
    default_context_window_tokens: 1_000_000,
    rework_prior_weighted_tokens: 72_000.0,
    default_trigger_grid: CLAUDE_GRID,
    replay_min_context_tokens: 120_000,
};

const CODEX_PROFILE: AgentProfile = AgentProfile {
    name: "codex",
    cold_gap_seconds_main: 1_800,
    cold_gap_seconds_subagent: 1_800,
    default_context_window_tokens: 258_400,
    rework_prior_weighted_tokens: 40_000.0,
    default_trigger_grid: CODEX_GRID,
    replay_min_context_tokens: 100_000,
};

impl AgentKind {
    /// Every supported agent.
    pub const ALL: [AgentKind; 2] = [AgentKind::ClaudeCode, AgentKind::Codex];

    /// The agent's measured constants.
    pub fn profile(self) -> &'static AgentProfile {
        match self {
            AgentKind::ClaudeCode => &CLAUDE_PROFILE,
            AgentKind::Codex => &CODEX_PROFILE,
        }
    }

    /// Cold-cache idle gap in seconds for a main session or a subagent.
    pub fn cold_gap_seconds(self, is_subagent: bool) -> u32 {
        let profile = self.profile();
        if is_subagent {
            profile.cold_gap_seconds_subagent
        } else {
            profile.cold_gap_seconds_main
        }
    }

    /// Coarse model family used for per-model optima.
    ///
    /// Claude: `opus`, `sonnet`, `fable`, `haiku` or `other` by substring.
    /// Codex: the model name itself (families are not encoded in the names).
    pub fn model_family(self, model: &str) -> String {
        match self {
            AgentKind::ClaudeCode => {
                let lowered = model.to_ascii_lowercase();
                for family in ["opus", "sonnet", "fable", "haiku"] {
                    if lowered.contains(family) {
                        return family.to_string();
                    }
                }
                "other".to_string()
            }
            AgentKind::Codex => {
                if model.is_empty() {
                    "unknown".to_string()
                } else {
                    model.to_string()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_match_the_research() {
        assert_eq!(AgentKind::ClaudeCode.cold_gap_seconds(false), 3_600);
        assert_eq!(AgentKind::ClaudeCode.cold_gap_seconds(true), 300);
        assert_eq!(AgentKind::Codex.cold_gap_seconds(false), 1_800);
        assert_eq!(
            AgentKind::Codex.profile().default_context_window_tokens,
            258_400
        );
        assert_eq!(
            AgentKind::ClaudeCode.model_family("claude-opus-5-5"),
            "opus"
        );
        assert_eq!(AgentKind::ClaudeCode.model_family("mystery"), "other");
        assert_eq!(AgentKind::Codex.model_family("gpt-6.1-sol"), "gpt-6.1-sol");
    }

    #[test]
    fn grids_are_ascending() {
        for agent in AgentKind::ALL {
            let grid = agent.profile().default_trigger_grid;
            assert!(grid.windows(2).all(|pair| pair[0] < pair[1]));
        }
    }
}
