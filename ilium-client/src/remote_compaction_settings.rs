//! User settings for remote compaction (`[remote_compaction]` in `config.toml`).
//!
//! Like [`crate::cost_settings`], the model is data-only. The settings tab
//! walks [`RemoteCompactionRow::rows`] so keyboard, mouse and rendering can
//! never disagree about which rows exist, and every value change goes through
//! the pure [`RemoteCompactionSettings::adjust`] that the tests drive.

use ilium_core::AgentClass;
use ilium_remote_compaction::{AgentKind, CompactionOptions, Technique};
use serde::{Deserialize, Serialize};

pub const THRESHOLD_PERCENT_RANGE: (u8, u8) = (30, 88);
pub const CUSTOM_PROMPT_MAX_CHARS: usize = 20_000;
/// The configuration transport rejects a command whose JSON exceeds 64 KiB
/// (`filesystem::configuration::serialized_bound`), which 20 000 four-byte
/// characters or 20 000 control characters would, so the prompt is also capped
/// by its JSON-escaped size. The other fields add well under 2 KiB.
pub const CUSTOM_PROMPT_MAX_SERIALIZED_BYTES: usize = 56_000;
pub const TAIL_TOKENS_RANGE: (u64, u64) = (2_000, 100_000);
pub const PROTECTED_TOOL_TOKENS_RANGE: (u64, u64) = (0, 200_000);
pub const TOOL_RESULT_CHARS_RANGE: (usize, usize) = (200, 20_000);
pub const SUMMARIZER_CONTEXT_RANGE: (u64, u64) = (8_000, 900_000);
pub const PAUSE_TIMEOUT_RANGE: (u64, u64) = (10, 1_800);
pub const COOLDOWN_MINUTES_RANGE: (u64, u64) = (1, 240);
pub const KEEP_BACKUPS_RANGE: (usize, usize) = (1, 20);
/// The summarizer output limit is not a setting: the caller replaces this
/// with the inference model's own limit when it knows it.
pub const DEFAULT_SUMMARIZER_MAX_OUTPUT_TOKENS: u64 = 16_000;

const THRESHOLD_STEP: u8 = 1;
const PAUSE_TIMEOUT_STEPS: [u64; 12] = [10, 20, 30, 60, 90, 120, 180, 300, 600, 900, 1_200, 1_800];
const COOLDOWN_MINUTE_STEPS: [u64; 13] = [1, 2, 5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240];
const TAIL_TOKEN_STEPS: [u64; 10] = [
    2_000, 5_000, 10_000, 15_000, 20_000, 30_000, 40_000, 60_000, 80_000, 100_000,
];
const PROTECTED_TOOL_TOKEN_STEPS: [u64; 10] = [
    0, 10_000, 20_000, 30_000, 40_000, 60_000, 80_000, 100_000, 150_000, 200_000,
];
const TOOL_RESULT_CHAR_STEPS: [usize; 8] = [200, 500, 1_000, 2_000, 3_000, 5_000, 10_000, 20_000];
const SUMMARIZER_CONTEXT_STEPS: [u64; 10] = [
    8_000, 16_000, 32_000, 64_000, 100_000, 120_000, 200_000, 400_000, 700_000, 900_000,
];

/// Everything `[remote_compaction]` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteCompactionSettings {
    /// Master switch. While off the Compact button keeps sending `/compact`.
    pub enabled: bool,
    /// Let the context monitor start a compaction by itself.
    pub automatic: bool,
    /// Context fill, in percent of the window, that starts an automatic
    /// compaction. Kept below every agent's native trigger (Codex: 90).
    pub threshold_percent: u8,
    pub claude_technique: Technique,
    pub codex_technique: Technique,
    /// Technique for agents without a row of their own.
    pub other_technique: Technique,
    /// Instructions of the `Custom` technique; empty means built-in.
    pub custom_prompt: String,
    pub tail_tokens: u64,
    pub protected_recent_tool_tokens: u64,
    pub tool_result_chars: usize,
    pub redact_secrets: bool,
    pub summarizer_context_tokens: u64,
    pub pause_timeout_seconds: u64,
    pub cooldown_minutes: u64,
    pub keep_backups: usize,
    /// Set once the user closes the privacy banner; it is never drawn again.
    pub privacy_banner_dismissed: bool,
}

impl Default for RemoteCompactionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            automatic: false,
            threshold_percent: 65,
            claude_technique: Technique::default_for(AgentKind::Claude),
            codex_technique: Technique::default_for(AgentKind::Codex),
            other_technique: Technique::default_for(AgentKind::Claude),
            custom_prompt: String::new(),
            tail_tokens: 20_000,
            protected_recent_tool_tokens: 40_000,
            tool_result_chars: 2_000,
            redact_secrets: true,
            summarizer_context_tokens: 120_000,
            pause_timeout_seconds: 120,
            cooldown_minutes: 10,
            keep_backups: 3,
            privacy_banner_dismissed: false,
        }
    }
}

impl RemoteCompactionSettings {
    /// The technique configured for `agent`: one row each for Claude and
    /// Codex, the shared "other agents" row for everything else.
    pub fn technique_for(&self, agent: &AgentClass) -> Technique {
        match agent {
            AgentClass::Claude => self.claude_technique,
            AgentClass::Codex => self.codex_technique,
            AgentClass::Antigravity | AgentClass::Other(_) => self.other_technique,
        }
    }

    /// Maps every setting onto the crate's options for one compaction run.
    pub fn options_for(&self, agent: AgentKind) -> CompactionOptions {
        let technique = match agent {
            AgentKind::Claude => self.claude_technique,
            AgentKind::Codex => self.codex_technique,
        };
        CompactionOptions {
            technique,
            custom_prompt: (!self.custom_prompt.trim().is_empty())
                .then(|| self.custom_prompt.clone()),
            tail_tokens: self.tail_tokens,
            protected_recent_tool_tokens: self.protected_recent_tool_tokens,
            tool_result_chars: self.tool_result_chars,
            redact_secrets: self.redact_secrets,
            summarizer_context_tokens: self.summarizer_context_tokens,
            summarizer_max_output_tokens: DEFAULT_SUMMARIZER_MAX_OUTPUT_TOKENS,
            keep_backups: self.keep_backups,
        }
    }

    /// Whether the privacy banner is still to be drawn, in the settings tab
    /// and in the compaction dialog alike.
    pub const fn should_show_privacy_banner(&self) -> bool {
        !self.privacy_banner_dismissed
    }

    /// Closes the banner for good. Returns whether anything changed.
    pub fn dismiss_privacy_banner(&mut self) -> bool {
        let changed = !self.privacy_banner_dismissed;
        self.privacy_banner_dismissed = true;
        changed
    }

    /// Repairs out-of-range values from a hand-edited file.
    pub fn sanitized(mut self) -> Self {
        self.threshold_percent = self
            .threshold_percent
            .clamp(THRESHOLD_PERCENT_RANGE.0, THRESHOLD_PERCENT_RANGE.1);
        self.custom_prompt = truncate_prompt(self.custom_prompt);
        self.tail_tokens = self
            .tail_tokens
            .clamp(TAIL_TOKENS_RANGE.0, TAIL_TOKENS_RANGE.1);
        self.protected_recent_tool_tokens = self
            .protected_recent_tool_tokens
            .clamp(PROTECTED_TOOL_TOKENS_RANGE.0, PROTECTED_TOOL_TOKENS_RANGE.1);
        self.tool_result_chars = self
            .tool_result_chars
            .clamp(TOOL_RESULT_CHARS_RANGE.0, TOOL_RESULT_CHARS_RANGE.1);
        self.summarizer_context_tokens = self
            .summarizer_context_tokens
            .clamp(SUMMARIZER_CONTEXT_RANGE.0, SUMMARIZER_CONTEXT_RANGE.1);
        self.pause_timeout_seconds = self
            .pause_timeout_seconds
            .clamp(PAUSE_TIMEOUT_RANGE.0, PAUSE_TIMEOUT_RANGE.1);
        self.cooldown_minutes = self
            .cooldown_minutes
            .clamp(COOLDOWN_MINUTES_RANGE.0, COOLDOWN_MINUTES_RANGE.1);
        self.keep_backups = self
            .keep_backups
            .clamp(KEEP_BACKUPS_RANGE.0, KEEP_BACKUPS_RANGE.1);
        self
    }

    /// The technique stored for `target`.
    pub fn technique(&self, target: TechniqueTarget) -> Technique {
        match target {
            TechniqueTarget::Claude => self.claude_technique,
            TechniqueTarget::Codex => self.codex_technique,
            TechniqueTarget::Other => self.other_technique,
        }
    }

    fn technique_mut(&mut self, target: TechniqueTarget) -> &mut Technique {
        match target {
            TechniqueTarget::Claude => &mut self.claude_technique,
            TechniqueTarget::Codex => &mut self.codex_technique,
            TechniqueTarget::Other => &mut self.other_technique,
        }
    }

    /// Applies one interaction to `row`. `direction` is `-1`, `0` (activate)
    /// or `1`. Returns whether anything changed.
    pub fn adjust(&mut self, row: RemoteCompactionRow, direction: i32) -> bool {
        let before = self.clone();
        match row {
            RemoteCompactionRow::PrivacyBanner => {
                self.dismiss_privacy_banner();
            }
            RemoteCompactionRow::Enabled => self.enabled = !self.enabled,
            RemoteCompactionRow::Automatic => self.automatic = !self.automatic,
            RemoteCompactionRow::Threshold => {
                let step = if direction < 0 {
                    -i16::from(THRESHOLD_STEP)
                } else {
                    i16::from(THRESHOLD_STEP)
                };
                self.threshold_percent = (i16::from(self.threshold_percent) + step).clamp(
                    i16::from(THRESHOLD_PERCENT_RANGE.0),
                    i16::from(THRESHOLD_PERCENT_RANGE.1),
                ) as u8;
            }
            RemoteCompactionRow::PauseTimeout => {
                self.pause_timeout_seconds =
                    step_ladder(&PAUSE_TIMEOUT_STEPS, self.pause_timeout_seconds, direction)
            }
            RemoteCompactionRow::Cooldown => {
                self.cooldown_minutes =
                    step_ladder(&COOLDOWN_MINUTE_STEPS, self.cooldown_minutes, direction)
            }
            RemoteCompactionRow::Technique(target) => {
                let current = self.technique(target);
                *self.technique_mut(target) = cycle_technique(current, direction);
            }
            RemoteCompactionRow::TailTokens => {
                self.tail_tokens = step_ladder(&TAIL_TOKEN_STEPS, self.tail_tokens, direction)
            }
            RemoteCompactionRow::ProtectedToolTokens => {
                self.protected_recent_tool_tokens = step_ladder(
                    &PROTECTED_TOOL_TOKEN_STEPS,
                    self.protected_recent_tool_tokens,
                    direction,
                )
            }
            RemoteCompactionRow::ToolResultChars => {
                self.tool_result_chars =
                    step_ladder(&TOOL_RESULT_CHAR_STEPS, self.tool_result_chars, direction)
            }
            RemoteCompactionRow::RedactSecrets => self.redact_secrets = !self.redact_secrets,
            RemoteCompactionRow::SummarizerContextTokens => {
                self.summarizer_context_tokens = step_ladder(
                    &SUMMARIZER_CONTEXT_STEPS,
                    self.summarizer_context_tokens,
                    direction,
                )
            }
            RemoteCompactionRow::KeepBackups => {
                let step: i64 = if direction < 0 { -1 } else { 1 };
                self.keep_backups = (self.keep_backups as i64 + step)
                    .clamp(KEEP_BACKUPS_RANGE.0 as i64, KEEP_BACKUPS_RANGE.1 as i64)
                    as usize;
            }
            // Edited in the multi-line editor; the app owns that dialog.
            RemoteCompactionRow::CustomPrompt | RemoteCompactionRow::Model => {}
        }
        *self != before
    }

    /// Replaces the custom prompt after the multi-line editor, bounded like
    /// a hand-edited file. Returns whether anything changed.
    pub fn set_custom_prompt(&mut self, text: String) -> bool {
        let text = truncate_prompt(text);
        let changed = self.custom_prompt != text;
        self.custom_prompt = text;
        changed
    }
}

fn truncate_prompt(text: String) -> String {
    let mut end = text.len();
    let mut serialized_bytes = 0;
    for (chars, (index, character)) in text.char_indices().enumerate() {
        serialized_bytes += json_escaped_len(character);
        if chars == CUSTOM_PROMPT_MAX_CHARS || serialized_bytes > CUSTOM_PROMPT_MAX_SERIALIZED_BYTES
        {
            end = index;
            break;
        }
    }
    let mut text = text;
    text.truncate(end);
    text
}

/// Bytes `character` takes inside a JSON string literal.
fn json_escaped_len(character: char) -> usize {
    match character {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        control if control < ' ' => 6,
        other => other.len_utf8(),
    }
}

/// Next/previous technique; activation steps forward. Wraps around.
fn cycle_technique(current: Technique, direction: i32) -> Technique {
    let all = Technique::ALL;
    let index = all.iter().position(|item| *item == current).unwrap_or(0);
    let next = if direction < 0 {
        (index + all.len() - 1) % all.len()
    } else {
        (index + 1) % all.len()
    };
    all[next]
}

/// Moves to the next/previous ladder value; a value off the ladder (a
/// hand-edited file) snaps to the nearest one in the requested direction.
/// Activating (`direction == 0`) steps forward.
fn step_ladder<T: Copy + PartialOrd>(ladder: &[T], current: T, direction: i32) -> T {
    if direction >= 0 {
        ladder
            .iter()
            .copied()
            .find(|value| *value > current)
            .unwrap_or(current)
    } else {
        ladder
            .iter()
            .rev()
            .copied()
            .find(|value| *value < current)
            .unwrap_or(current)
    }
}

/// The three per-agent technique selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TechniqueTarget {
    Claude,
    Codex,
    Other,
}

impl TechniqueTarget {
    pub const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::Other];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude technique",
            Self::Codex => "Codex technique",
            Self::Other => "Other agents technique",
        }
    }
}

/// What a row does when activated, which decides how the tab draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteCompactionRowKind {
    /// The closable privacy box.
    Banner,
    /// `[x]` checkbox.
    Toggle,
    /// `‹ value ›` stepper over numbers.
    Stepper,
    /// `‹ value ›` selector over named choices.
    Select,
    /// Opens the multi-line editor.
    Editor,
    /// Shown for information only.
    ReadOnly,
}

/// One selectable row of the Remote compaction settings tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteCompactionRow {
    PrivacyBanner,
    Enabled,
    Automatic,
    Threshold,
    PauseTimeout,
    Cooldown,
    Technique(TechniqueTarget),
    CustomPrompt,
    TailTokens,
    ProtectedToolTokens,
    ToolResultChars,
    RedactSecrets,
    SummarizerContextTokens,
    KeepBackups,
    Model,
}

impl RemoteCompactionRow {
    /// Every row in display order. The banner row exists only until it has
    /// been dismissed.
    pub fn rows(settings: &RemoteCompactionSettings) -> Vec<Self> {
        let mut rows = Vec::with_capacity(16);
        if settings.should_show_privacy_banner() {
            rows.push(Self::PrivacyBanner);
        }
        rows.extend([
            Self::Enabled,
            Self::Automatic,
            Self::Threshold,
            Self::PauseTimeout,
            Self::Cooldown,
        ]);
        rows.extend(TechniqueTarget::ALL.map(Self::Technique));
        rows.extend([
            Self::CustomPrompt,
            Self::TailTokens,
            Self::ProtectedToolTokens,
            Self::ToolResultChars,
            Self::RedactSecrets,
            Self::SummarizerContextTokens,
            Self::KeepBackups,
            Self::Model,
        ]);
        rows
    }

    pub const fn kind(self) -> RemoteCompactionRowKind {
        match self {
            Self::PrivacyBanner => RemoteCompactionRowKind::Banner,
            Self::Enabled | Self::Automatic | Self::RedactSecrets => {
                RemoteCompactionRowKind::Toggle
            }
            Self::Threshold
            | Self::PauseTimeout
            | Self::Cooldown
            | Self::TailTokens
            | Self::ProtectedToolTokens
            | Self::ToolResultChars
            | Self::SummarizerContextTokens
            | Self::KeepBackups => RemoteCompactionRowKind::Stepper,
            Self::Technique(_) => RemoteCompactionRowKind::Select,
            Self::CustomPrompt => RemoteCompactionRowKind::Editor,
            Self::Model => RemoteCompactionRowKind::ReadOnly,
        }
    }

    /// Whether the value is a number whose changes save after the debounce
    /// instead of immediately.
    pub const fn is_debounced(self) -> bool {
        matches!(self.kind(), RemoteCompactionRowKind::Stepper)
    }

    /// Every row that can have a help topic, regardless of dismissal.
    pub fn all_rows() -> Vec<Self> {
        Self::rows(&RemoteCompactionSettings {
            privacy_banner_dismissed: false,
            ..RemoteCompactionSettings::default()
        })
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::PrivacyBanner => "Privacy",
            Self::Enabled => "Remote compaction",
            Self::Automatic => "Compact automatically",
            Self::Threshold => "Automatic threshold",
            Self::PauseTimeout => "Wait for a pause",
            Self::Cooldown => "Cooldown after a run",
            Self::Technique(target) => target.label(),
            Self::CustomPrompt => "Custom prompt",
            Self::TailTokens => "Verbatim tail",
            Self::ProtectedToolTokens => "Protected tool output",
            Self::ToolResultChars => "Older tool result size",
            Self::RedactSecrets => "Redact secrets",
            Self::SummarizerContextTokens => "Summarizer input window",
            Self::KeepBackups => "Backups to keep",
            Self::Model => "Summarizer model",
        }
    }

    /// Stable ID of this row's help topic (see `settings_help`).
    pub const fn help_id(self) -> &'static str {
        match self {
            Self::PrivacyBanner => "RCOMP-01",
            Self::Enabled => "RCOMP-02",
            Self::Automatic => "RCOMP-03",
            Self::Threshold => "RCOMP-04",
            Self::PauseTimeout => "RCOMP-05",
            Self::Cooldown => "RCOMP-06",
            Self::Technique(TechniqueTarget::Claude) => "RCOMP-07",
            Self::Technique(TechniqueTarget::Codex) => "RCOMP-08",
            Self::Technique(TechniqueTarget::Other) => "RCOMP-09",
            Self::CustomPrompt => "RCOMP-10",
            Self::TailTokens => "RCOMP-11",
            Self::ProtectedToolTokens => "RCOMP-12",
            Self::ToolResultChars => "RCOMP-13",
            Self::RedactSecrets => "RCOMP-14",
            Self::SummarizerContextTokens => "RCOMP-15",
            Self::KeepBackups => "RCOMP-16",
            Self::Model => "RCOMP-17",
        }
    }
}

/// Number of help topics the tab owns (`RCOMP-01` through `RCOMP-17`).
pub const HELP_TOPIC_COUNT: usize = 17;

/// What a technique does, in one sentence.
pub const fn technique_description(technique: Technique) -> &'static str {
    match technique {
        Technique::ClaudeCode => {
            "Claude Code's nine-section summary: intent, concepts, files, errors, every user \
             message, pending tasks, current work and the next step."
        }
        Technique::Codex => {
            "Codex's context-checkpoint handoff: a note to the next model with progress, \
             decisions, constraints and what remains."
        }
        Technique::Opencode => {
            "opencode's six-section Markdown summary, updating the earlier summary in place \
             instead of rewriting it."
        }
        Technique::GeminiCli => {
            "Gemini CLI's private scratchpad followed by an XML state snapshot of goal, \
             knowledge and plan."
        }
        Technique::BestOfAllWorlds => {
            "Ilium's blend: a ledger of files and commands built in code, nine sections, the \
             objective first, the next step last, and every user directive kept verbatim."
        }
        Technique::Custom => {
            "Your own instructions (edit them under Custom prompt). Falls back to the Claude \
             Code prompt while that is empty."
        }
    }
}

/// Whether the technique's prompt is the upstream project's real prompt.
pub const fn technique_fidelity(technique: Technique) -> &'static str {
    match technique {
        Technique::ClaudeCode | Technique::Codex | Technique::Opencode | Technique::GeminiCli => {
            "Prompt: the real upstream prompt. Sections it lacks are filled from the Claude \
             nine-section template."
        }
        Technique::BestOfAllWorlds => "Prompt: Ilium's own design, not an upstream prompt.",
        Technique::Custom => {
            "Prompt: yours; Ilium adds only the no-tools and untrusted-history guards."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_specification() {
        let settings = RemoteCompactionSettings::default();
        assert!(!settings.enabled);
        assert!(!settings.automatic);
        assert_eq!(settings.threshold_percent, 65);
        assert_eq!(settings.claude_technique, Technique::ClaudeCode);
        assert_eq!(settings.codex_technique, Technique::Codex);
        assert_eq!(settings.other_technique, Technique::ClaudeCode);
        assert!(settings.custom_prompt.is_empty());
        assert_eq!(settings.tail_tokens, 20_000);
        assert_eq!(settings.protected_recent_tool_tokens, 40_000);
        assert_eq!(settings.tool_result_chars, 2_000);
        assert!(settings.redact_secrets);
        assert_eq!(settings.summarizer_context_tokens, 120_000);
        assert_eq!(settings.pause_timeout_seconds, 120);
        assert_eq!(settings.cooldown_minutes, 10);
        assert_eq!(settings.keep_backups, 3);
        assert!(!settings.privacy_banner_dismissed);
        assert!(settings.should_show_privacy_banner());
        assert_eq!(settings.clone().sanitized(), settings);
    }

    #[test]
    fn sanitize_clamps_every_range_and_bounds_the_prompt() {
        let high = RemoteCompactionSettings {
            threshold_percent: 99,
            tail_tokens: u64::MAX,
            protected_recent_tool_tokens: u64::MAX,
            tool_result_chars: usize::MAX,
            summarizer_context_tokens: u64::MAX,
            pause_timeout_seconds: u64::MAX,
            cooldown_minutes: u64::MAX,
            keep_backups: usize::MAX,
            custom_prompt: "x".repeat(CUSTOM_PROMPT_MAX_CHARS + 50),
            ..Default::default()
        }
        .sanitized();
        assert_eq!(high.threshold_percent, 88);
        assert_eq!(high.tail_tokens, 100_000);
        assert_eq!(high.protected_recent_tool_tokens, 200_000);
        assert_eq!(high.tool_result_chars, 20_000);
        assert_eq!(high.summarizer_context_tokens, 900_000);
        assert_eq!(high.pause_timeout_seconds, 1_800);
        assert_eq!(high.cooldown_minutes, 240);
        assert_eq!(high.keep_backups, 20);
        assert_eq!(high.custom_prompt.chars().count(), CUSTOM_PROMPT_MAX_CHARS);

        let low = RemoteCompactionSettings {
            threshold_percent: 0,
            tail_tokens: 0,
            protected_recent_tool_tokens: 0,
            tool_result_chars: 0,
            summarizer_context_tokens: 0,
            pause_timeout_seconds: 0,
            cooldown_minutes: 0,
            keep_backups: 0,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(low.threshold_percent, 30);
        assert_eq!(low.tail_tokens, 2_000);
        assert_eq!(low.protected_recent_tool_tokens, 0);
        assert_eq!(low.tool_result_chars, 200);
        assert_eq!(low.summarizer_context_tokens, 8_000);
        assert_eq!(low.pause_timeout_seconds, 10);
        assert_eq!(low.cooldown_minutes, 1);
        assert_eq!(low.keep_backups, 1);
    }

    #[test]
    fn a_multibyte_prompt_is_cut_on_a_character_boundary_inside_the_transport_bound() {
        let prompt = "🦀".repeat(CUSTOM_PROMPT_MAX_CHARS);
        let settings = RemoteCompactionSettings {
            custom_prompt: prompt,
            ..Default::default()
        }
        .sanitized();
        assert!(settings.custom_prompt.len() <= CUSTOM_PROMPT_MAX_SERIALIZED_BYTES);
        assert!(settings.custom_prompt.chars().all(|c| c == '🦀'));
        assert!(!settings.custom_prompt.is_empty());
    }

    #[test]
    fn an_escape_heavy_prompt_still_fits_the_configuration_transport() {
        let settings = RemoteCompactionSettings {
            custom_prompt: "\u{1}".repeat(CUSTOM_PROMPT_MAX_CHARS),
            ..Default::default()
        }
        .sanitized();
        let bound = crate::filesystem::configuration::serialized_bound(&settings);
        assert!(bound.is_ok(), "{bound:?}");
        assert!(settings.custom_prompt.len() < CUSTOM_PROMPT_MAX_CHARS);
    }

    #[test]
    fn toggles_flip_and_report_the_change() {
        let mut settings = RemoteCompactionSettings::default();
        for row in [
            RemoteCompactionRow::Enabled,
            RemoteCompactionRow::Automatic,
            RemoteCompactionRow::RedactSecrets,
        ] {
            let before = settings.clone();
            assert!(settings.adjust(row, 0), "{row:?}");
            assert_ne!(settings, before, "{row:?}");
            assert!(settings.adjust(row, -1), "{row:?} flips back");
            assert_eq!(settings, before, "{row:?}");
        }
    }

    #[test]
    fn threshold_steps_by_one_and_stops_at_its_limits() {
        let mut settings = RemoteCompactionSettings::default();
        assert!(settings.adjust(RemoteCompactionRow::Threshold, 1));
        assert_eq!(settings.threshold_percent, 66);
        assert!(settings.adjust(RemoteCompactionRow::Threshold, -1));
        assert!(settings.adjust(RemoteCompactionRow::Threshold, -1));
        assert_eq!(settings.threshold_percent, 64);
        settings.threshold_percent = 88;
        assert!(!settings.adjust(RemoteCompactionRow::Threshold, 1));
        settings.threshold_percent = 30;
        assert!(!settings.adjust(RemoteCompactionRow::Threshold, -1));
        assert_eq!(settings.threshold_percent, 30);
    }

    #[test]
    fn numeric_rows_walk_their_ladders_and_stop_at_the_ends() {
        use RemoteCompactionRow as Row;
        let mut settings = RemoteCompactionSettings::default();
        let read = |settings: &RemoteCompactionSettings, row: Row| -> u64 {
            match row {
                Row::PauseTimeout => settings.pause_timeout_seconds,
                Row::Cooldown => settings.cooldown_minutes,
                Row::TailTokens => settings.tail_tokens,
                Row::ProtectedToolTokens => settings.protected_recent_tool_tokens,
                Row::ToolResultChars => settings.tool_result_chars as u64,
                Row::SummarizerContextTokens => settings.summarizer_context_tokens,
                Row::KeepBackups => settings.keep_backups as u64,
                _ => unreachable!(),
            }
        };
        for row in [
            Row::PauseTimeout,
            Row::Cooldown,
            Row::TailTokens,
            Row::ProtectedToolTokens,
            Row::ToolResultChars,
            Row::SummarizerContextTokens,
            Row::KeepBackups,
        ] {
            let start = read(&settings, row);
            assert!(settings.adjust(row, 1), "{row:?} grows");
            let up = read(&settings, row);
            assert!(up > start, "{row:?}");
            assert!(settings.adjust(row, -1), "{row:?} shrinks");
            assert_eq!(read(&settings, row), start, "{row:?} returns");
            // Walk to both ends; the ends are inert.
            while settings.adjust(row, 1) {}
            let top = read(&settings, row);
            assert!(!settings.adjust(row, 1), "{row:?} stops at the top");
            while settings.adjust(row, -1) {}
            let bottom = read(&settings, row);
            assert!(bottom < top, "{row:?}");
            assert!(!settings.adjust(row, -1), "{row:?} stops at the bottom");
            settings = RemoteCompactionSettings::default();
        }
        // The documented range ends are exactly reachable.
        let mut ends = RemoteCompactionSettings::default();
        while ends.adjust(Row::TailTokens, 1) {}
        assert_eq!(ends.tail_tokens, TAIL_TOKENS_RANGE.1);
        while ends.adjust(Row::TailTokens, -1) {}
        assert_eq!(ends.tail_tokens, TAIL_TOKENS_RANGE.0);
        while ends.adjust(Row::ProtectedToolTokens, -1) {}
        assert_eq!(ends.protected_recent_tool_tokens, 0);
        while ends.adjust(Row::SummarizerContextTokens, 1) {}
        assert_eq!(ends.summarizer_context_tokens, SUMMARIZER_CONTEXT_RANGE.1);
        while ends.adjust(Row::KeepBackups, 1) {}
        assert_eq!(ends.keep_backups, KEEP_BACKUPS_RANGE.1);
    }

    #[test]
    fn an_off_ladder_value_snaps_in_the_requested_direction() {
        let mut settings = RemoteCompactionSettings {
            tail_tokens: 23_456,
            ..Default::default()
        };
        assert!(settings.adjust(RemoteCompactionRow::TailTokens, 1));
        assert_eq!(settings.tail_tokens, 30_000);
        settings.tail_tokens = 23_456;
        assert!(settings.adjust(RemoteCompactionRow::TailTokens, -1));
        assert_eq!(settings.tail_tokens, 20_000);
    }

    #[test]
    fn technique_rows_cycle_through_every_technique_independently() {
        for target in TechniqueTarget::ALL {
            let mut settings = RemoteCompactionSettings::default();
            let others: Vec<_> = TechniqueTarget::ALL
                .into_iter()
                .filter(|other| *other != target)
                .map(|other| settings.technique(other))
                .collect();
            let start = settings.technique(target);
            let mut seen = vec![start];
            for _ in 1..Technique::ALL.len() {
                assert!(settings.adjust(RemoteCompactionRow::Technique(target), 1));
                seen.push(settings.technique(target));
            }
            let mut unique = seen.clone();
            unique.dedup();
            assert_eq!(unique.len(), Technique::ALL.len(), "{target:?}: {seen:?}");
            for technique in Technique::ALL {
                assert!(
                    seen.contains(&technique),
                    "{target:?} reaches {technique:?}"
                );
            }
            // One more step wraps to the start; backwards undoes forwards.
            assert!(settings.adjust(RemoteCompactionRow::Technique(target), 1));
            assert_eq!(settings.technique(target), start);
            assert!(settings.adjust(RemoteCompactionRow::Technique(target), -1));
            assert_ne!(settings.technique(target), start);
            assert!(settings.adjust(RemoteCompactionRow::Technique(target), 1));
            assert_eq!(settings.technique(target), start);
            let after: Vec<_> = TechniqueTarget::ALL
                .into_iter()
                .filter(|other| *other != target)
                .map(|other| settings.technique(other))
                .collect();
            assert_eq!(others, after, "{target:?} leaves the other rows alone");
        }
    }

    #[test]
    fn technique_for_routes_each_agent_class_to_its_row() {
        let settings = RemoteCompactionSettings {
            claude_technique: Technique::Opencode,
            codex_technique: Technique::GeminiCli,
            other_technique: Technique::BestOfAllWorlds,
            ..Default::default()
        };
        assert_eq!(
            settings.technique_for(&AgentClass::Claude),
            Technique::Opencode
        );
        assert_eq!(
            settings.technique_for(&AgentClass::Codex),
            Technique::GeminiCli
        );
        assert_eq!(
            settings.technique_for(&AgentClass::Antigravity),
            Technique::BestOfAllWorlds
        );
        assert_eq!(
            settings.technique_for(&AgentClass::Other("aider".into())),
            Technique::BestOfAllWorlds
        );
    }

    #[test]
    fn the_banner_row_dismisses_once_and_never_returns() {
        let mut settings = RemoteCompactionSettings::default();
        assert_eq!(
            RemoteCompactionRow::rows(&settings).first(),
            Some(&RemoteCompactionRow::PrivacyBanner)
        );
        assert!(settings.adjust(RemoteCompactionRow::PrivacyBanner, 0));
        assert!(settings.privacy_banner_dismissed);
        assert!(!settings.should_show_privacy_banner());
        assert!(!RemoteCompactionRow::rows(&settings).contains(&RemoteCompactionRow::PrivacyBanner));
        assert!(!settings.adjust(RemoteCompactionRow::PrivacyBanner, 0));
        assert!(!settings.dismiss_privacy_banner());
    }

    #[test]
    fn rows_cover_every_help_topic_exactly_once() {
        let rows = RemoteCompactionRow::all_rows();
        assert_eq!(rows.len(), HELP_TOPIC_COUNT);
        let mut ids: Vec<_> = rows.iter().map(|row| row.help_id()).collect();
        ids.sort_unstable();
        let expected: Vec<String> = (1..=HELP_TOPIC_COUNT)
            .map(|index| format!("RCOMP-{index:02}"))
            .collect();
        assert_eq!(ids, expected);
        let dismissed = RemoteCompactionSettings {
            privacy_banner_dismissed: true,
            ..Default::default()
        };
        assert_eq!(
            RemoteCompactionRow::rows(&dismissed).len(),
            HELP_TOPIC_COUNT - 1
        );
    }

    #[test]
    fn display_only_rows_and_the_prompt_row_do_not_change_settings() {
        let mut settings = RemoteCompactionSettings::default();
        assert!(!settings.adjust(RemoteCompactionRow::Model, 0));
        assert!(!settings.adjust(RemoteCompactionRow::CustomPrompt, 0));
        assert_eq!(settings, RemoteCompactionSettings::default());
        assert!(settings.set_custom_prompt("Keep the schema.".into()));
        assert!(!settings.set_custom_prompt("Keep the schema.".into()));
    }

    #[test]
    fn options_for_maps_every_field_per_agent() {
        let settings = RemoteCompactionSettings {
            claude_technique: Technique::Opencode,
            codex_technique: Technique::Custom,
            custom_prompt: "Summarise tersely.".into(),
            tail_tokens: 30_000,
            protected_recent_tool_tokens: 10_000,
            tool_result_chars: 500,
            redact_secrets: false,
            summarizer_context_tokens: 64_000,
            keep_backups: 7,
            ..Default::default()
        };
        let claude = settings.options_for(AgentKind::Claude);
        assert_eq!(claude.technique, Technique::Opencode);
        assert_eq!(claude.custom_prompt.as_deref(), Some("Summarise tersely."));
        assert_eq!(claude.tail_tokens, 30_000);
        assert_eq!(claude.protected_recent_tool_tokens, 10_000);
        assert_eq!(claude.tool_result_chars, 500);
        assert!(!claude.redact_secrets);
        assert_eq!(claude.summarizer_context_tokens, 64_000);
        assert_eq!(
            claude.summarizer_max_output_tokens,
            DEFAULT_SUMMARIZER_MAX_OUTPUT_TOKENS
        );
        assert_eq!(claude.keep_backups, 7);
        assert_eq!(
            settings.options_for(AgentKind::Codex).technique,
            Technique::Custom
        );
        assert_eq!(
            RemoteCompactionSettings::default()
                .options_for(AgentKind::Claude)
                .custom_prompt,
            None
        );
    }

    #[test]
    fn every_technique_has_a_description_and_provenance() {
        for technique in Technique::ALL {
            assert!(!technique_description(technique).is_empty());
            assert!(technique_fidelity(technique).starts_with("Prompt:"));
        }
    }
}
