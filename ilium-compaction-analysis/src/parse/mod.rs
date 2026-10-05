//! Streaming parsers: raw transcript lines in, a [`SessionTrace`] out.
//!
//! Each supported agent is one [`LogFormat`] entry in [`LOG_FORMATS`]: its
//! cheap relevance needles and a constructor for its [`FormatParser`]. Adding
//! an agent means one new format module and one new registry entry; nothing
//! else branches on the agent.
//!
//! The caller owns all I/O. A typical file scan is:
//!
//! 1. read the file line by line (bytes, no UTF-8 requirement),
//! 2. skip the line when [`line_may_matter`] says it cannot matter,
//! 3. hand the rest to [`TraceBuilder::feed_line`],
//! 4. call [`TraceBuilder::finish`] and cache the resulting trace.
//!
//! The parsers never fail as a whole. Truncated tail lines (live sessions are
//! being appended), non-JSON lines and over-long lines are counted in
//! [`TraceCounters`](crate::trace::TraceCounters) and skipped.

mod claude;
mod codex;
mod sink;
mod tools;

pub use sink::{RawCompaction, RawTurn, TraceSink};

use crate::agent::AgentKind;
use crate::trace::SessionTrace;
use crate::util::contains_bytes;

/// Default per-line byte limit: lines above it are skipped and counted.
pub const DEFAULT_MAX_LINE_BYTES: usize = 32 * 1024 * 1024;

/// Knobs of the parsers.
#[derive(Debug, Clone)]
pub struct ParseOptions {
    /// Lines longer than this many bytes are skipped unparsed and counted in
    /// `lines_skipped_oversize`.
    pub max_line_bytes: usize,
    /// Claude: requests whose model name contains one of these substrings are
    /// dropped from **main** sessions (Haiku housekeeping calls run in their
    /// own small context and would corrupt the context sequence). Subagent
    /// traces keep every model.
    pub skip_models_in_main_sessions: Vec<String>,
    /// Characters per token used to estimate a Claude compaction summary from
    /// its text length (the research used 3.5).
    pub summary_characters_per_token: f64,
    /// Characters per token used to estimate a Codex replacement history.
    pub history_characters_per_token: f64,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            max_line_bytes: DEFAULT_MAX_LINE_BYTES,
            skip_models_in_main_sessions: vec!["haiku".to_string()],
            summary_characters_per_token: 3.5,
            history_characters_per_token: 3.5,
        }
    }
}

/// A parser for one agent's line format.
///
/// Implementations are stateful (identifier dedupe, pending summaries) and
/// push what they understand into the [`TraceSink`].
pub trait FormatParser {
    /// Interprets one trimmed, non-empty JSON-object line that passed the
    /// relevance needles. A deserialization failure is reported as `Err` and
    /// counted by the builder; it never aborts parsing.
    fn parse_line(
        &mut self,
        line: &[u8],
        sink: &mut TraceSink,
        options: &ParseOptions,
    ) -> Result<(), serde_json::Error>;
}

/// One registry entry: everything the crate knows about an agent's log format.
pub struct LogFormat {
    /// The agent that writes this format.
    pub agent: AgentKind,
    /// Human-readable format name.
    pub name: &'static str,
    /// A line can only matter when it contains at least one of these byte
    /// substrings. Used both by [`line_may_matter`] and inside
    /// [`TraceBuilder::feed_line`].
    pub relevance_needles: &'static [&'static [u8]],
    /// Creates a fresh parser.
    pub new_parser: fn() -> Box<dyn FormatParser>,
}

/// The format registry.
pub static LOG_FORMATS: [LogFormat; 2] = [claude::FORMAT, codex::FORMAT];

/// The registry entry of an agent.
pub fn format_for(agent: AgentKind) -> &'static LogFormat {
    LOG_FORMATS
        .iter()
        .find(|format| format.agent == agent)
        // Every `AgentKind` variant has an entry (checked by a unit test);
        // fall back to the first entry rather than panicking.
        .unwrap_or(&LOG_FORMATS[0])
}

/// Cheap byte-substring prefilter: `false` means the line cannot carry any
/// information the agent's parser uses, so the caller may skip it without
/// allocating or parsing.
///
/// Claude: `"usage"`, `compact_boundary`, `isCompactSummary`.
/// Codex: `token_usage_record`, `"compacted"`, `turn_context`, `session_meta`,
/// `thread_settings_applied`, `model_context_window`.
pub fn line_may_matter(agent: AgentKind, line: &[u8]) -> bool {
    format_for(agent)
        .relevance_needles
        .iter()
        .any(|needle| contains_bytes(line, needle))
}

/// Incremental builder of a [`SessionTrace`] from transcript lines.
pub struct TraceBuilder {
    format: &'static LogFormat,
    options: ParseOptions,
    parser: Box<dyn FormatParser>,
    sink: TraceSink,
}

impl TraceBuilder {
    /// A builder for a main (non-subagent) transcript of `agent`.
    pub fn new(agent: AgentKind) -> Self {
        let format = format_for(agent);
        Self {
            format,
            options: ParseOptions::default(),
            parser: (format.new_parser)(),
            sink: TraceSink::new(agent, false),
        }
    }

    /// Declares whether the transcript is a subagent/sidechain file. For Codex
    /// the flag is also set automatically from `session_meta`.
    pub fn with_subagent(mut self, is_subagent: bool) -> Self {
        self.sink.trace.is_subagent = is_subagent;
        self
    }

    /// Replaces the parser options.
    pub fn with_options(mut self, options: ParseOptions) -> Self {
        self.options = options;
        self
    }

    /// Sets only the per-line byte limit.
    pub fn with_max_line_bytes(mut self, max_line_bytes: usize) -> Self {
        self.options.max_line_bytes = max_line_bytes;
        self
    }

    /// Feeds one transcript line (with or without its line terminator).
    ///
    /// Never fails: oversize, non-JSON, irrelevant and undecodable lines are
    /// counted and skipped.
    pub fn feed_line(&mut self, line: &[u8]) {
        let line = trim_line(line);
        if line.is_empty() {
            return;
        }
        let counters = self.sink.counters_mut();
        counters.lines_fed += 1;
        counters.bytes_fed += line.len() as u64;
        if line.len() > self.options.max_line_bytes {
            self.sink.counters_mut().lines_skipped_oversize += 1;
            return;
        }
        if line[0] != b'{' {
            self.sink.counters_mut().lines_not_json += 1;
            return;
        }
        let relevant = self
            .format
            .relevance_needles
            .iter()
            .any(|needle| contains_bytes(line, needle));
        if !relevant {
            self.sink.counters_mut().lines_irrelevant += 1;
            return;
        }
        if self
            .parser
            .parse_line(line, &mut self.sink, &self.options)
            .is_err()
        {
            self.sink.counters_mut().lines_unparseable += 1;
        }
    }

    /// Finishes and returns the trace.
    pub fn finish(self) -> SessionTrace {
        self.sink.into_trace()
    }
}

/// Strips ASCII whitespace (including `\r\n`) from both ends.
fn trim_line(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(line.len());
    let end = line
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |position| position + 1);
    &line[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_agent_has_a_registry_entry() {
        for agent in AgentKind::ALL {
            assert_eq!(format_for(agent).agent, agent);
        }
        assert_eq!(LOG_FORMATS.len(), AgentKind::ALL.len());
    }

    #[test]
    fn prefilter_matches_the_documented_needles() {
        assert!(line_may_matter(
            AgentKind::ClaudeCode,
            br#"{"message":{"usage":{}}}"#
        ));
        assert!(line_may_matter(
            AgentKind::ClaudeCode,
            br#"{"subtype":"compact_boundary"}"#
        ));
        assert!(!line_may_matter(
            AgentKind::ClaudeCode,
            br#"{"type":"user","message":{"content":"hi"}}"#
        ));
        assert!(line_may_matter(
            AgentKind::Codex,
            br#"{"type":"token_usage_record","payload":{}}"#
        ));
        assert!(line_may_matter(
            AgentKind::Codex,
            br#"{"type":"compacted"}"#
        ));
        assert!(!line_may_matter(
            AgentKind::Codex,
            br#"{"type":"response_item","payload":{}}"#
        ));
    }

    #[test]
    fn builder_counts_and_skips_bad_lines() {
        let mut builder = TraceBuilder::new(AgentKind::ClaudeCode).with_max_line_bytes(100);
        builder.feed_line(b"");
        builder.feed_line(b"   \r\n");
        builder.feed_line(b"plain text, not json");
        builder.feed_line(br#"{"type":"user"}"#);
        builder.feed_line(br#"{"type":"assistant","message":{"usage":{"input_tokens":1"#);
        let long = format!("{{\"usage\":\"{}\"}}", "x".repeat(200));
        builder.feed_line(long.as_bytes());
        let trace = builder.finish();
        assert_eq!(trace.counters.lines_fed, 4);
        assert_eq!(trace.counters.lines_not_json, 1);
        assert_eq!(trace.counters.lines_irrelevant, 1);
        assert_eq!(trace.counters.lines_unparseable, 1);
        assert_eq!(trace.counters.lines_skipped_oversize, 1);
        assert!(trace.turns.is_empty());
    }
}
