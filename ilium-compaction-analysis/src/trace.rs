//! The compact per-session trace: what gets cached and fed to every analysis.
//!
//! A [`SessionTrace`] keeps one fixed-size [`TurnSample`] per deduplicated
//! model request and one [`CompactionEvent`] per compaction. No message
//! content, path or identifier survives parsing (only a 32-bit hash of the
//! message or response identifier, used to dedupe turns that two transcript
//! files both contain). A typical turn costs 40 bytes in memory and one short
//! JSON array on disk.

use serde::{Deserialize, Serialize};

use crate::agent::AgentKind;
use crate::error::{AnalysisError, AnalysisResult};
pub use crate::tool_features::{
    ToolAccumulator, ToolFeatures, ToolOffsets, TurnTools, HASH_KIND_MASK, HASH_KIND_PATH,
    HASH_KIND_SEARCH, MAX_COMMAND_HASHES_PER_TURN, MAX_READ_HASHES_PER_TURN, TOOL_FLAG_EDIT,
    TOOL_FLAG_WRITE_HEURISTIC,
};

/// Version of the serialized [`SessionTrace`] layout and of the parsing rules
/// that produce it. Bump it whenever either changes so callers invalidate
/// their caches. Version 2 added [`ToolFeatures`] (per-request tool-call
/// features for the rework measurement); version 1 caches are refused with
/// [`AnalysisError::TraceVersionMismatch`] and must be rescanned.
pub const TRACE_FORMAT_VERSION: u32 = 2;

/// The request was a cache miss: its cache read was below half of the previous
/// request's context (and that context was at least 20k tokens).
pub const FLAG_CACHE_COLD: u8 = 1;
/// The idle gap before this request exceeded the agent's cold-cache threshold.
pub const FLAG_GAP_COLD: u8 = 1 << 1;
/// Codex only: this request is the summarisation call of a compaction.
pub const FLAG_COMPACTION_REQUEST: u8 = 1 << 2;
/// This is the first request after a compaction (the measured post size).
pub const FLAG_FIRST_AFTER_COMPACTION: u8 = 1 << 3;

/// Context size below which a request is never judged a cache miss.
pub(crate) const COLD_MIN_PREVIOUS_CONTEXT: u32 = 20_000;
/// Gap quantum: 10 seconds.
pub(crate) const GAP_QUANTUM_SECONDS: u32 = 10;

/// One deduplicated model request.
///
/// Token fields describe what the provider billed for the request. `context`
/// is the prompt size of the *main* request: for Claude turns that include an
/// advisor tool call, the billed totals sum the main iterations while the
/// context is that of the first main iteration, so context is never double
/// counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "TurnRow", into = "TurnRow")]
pub struct TurnSample {
    /// Request time, Unix milliseconds (0 when the log carried none).
    pub timestamp_ms: i64,
    /// Prompt size: uncached input + cache read + cache write.
    pub context_tokens: u32,
    /// Uncached input tokens.
    pub input_tokens: u32,
    /// Tokens read from the prompt cache.
    pub cache_read_tokens: u32,
    /// Tokens written to the 5-minute cache tier.
    pub cache_write_5m_tokens: u32,
    /// Tokens written to the 1-hour cache tier (untiered writes count here).
    pub cache_write_1h_tokens: u32,
    /// Output tokens (Codex: includes reasoning tokens).
    pub output_tokens: u32,
    /// Index into [`SessionTrace::models`].
    pub model: u16,
    /// `FLAG_*` bits.
    pub flags: u8,
    /// Idle gap since the previous request, in 10-second units, saturating.
    pub gap_decaseconds: u16,
}

/// Serialized shape of a [`TurnSample`]:
/// `[timestamp_ms, context, input, cache_read, write_5m, write_1h, output,
/// model, flags, gap_decaseconds]`.
type TurnRow = (i64, u32, u32, u32, u32, u32, u32, u16, u8, u16);

impl From<TurnRow> for TurnSample {
    fn from(row: TurnRow) -> Self {
        Self {
            timestamp_ms: row.0,
            context_tokens: row.1,
            input_tokens: row.2,
            cache_read_tokens: row.3,
            cache_write_5m_tokens: row.4,
            cache_write_1h_tokens: row.5,
            output_tokens: row.6,
            model: row.7,
            flags: row.8,
            gap_decaseconds: row.9,
        }
    }
}

impl From<TurnSample> for TurnRow {
    fn from(turn: TurnSample) -> Self {
        (
            turn.timestamp_ms,
            turn.context_tokens,
            turn.input_tokens,
            turn.cache_read_tokens,
            turn.cache_write_5m_tokens,
            turn.cache_write_1h_tokens,
            turn.output_tokens,
            turn.model,
            turn.flags,
            turn.gap_decaseconds,
        )
    }
}

impl TurnSample {
    /// Total tokens written to the cache by this request.
    pub fn cache_write_tokens(&self) -> u32 {
        self.cache_write_5m_tokens
            .saturating_add(self.cache_write_1h_tokens)
    }

    /// Whether every bit of `flag` is set.
    pub fn has_flag(&self, flag: u8) -> bool {
        self.flags & flag == flag
    }

    /// Idle gap since the previous request, in seconds (10 s resolution).
    pub fn gap_seconds(&self) -> u32 {
        u32::from(self.gap_decaseconds) * GAP_QUANTUM_SECONDS
    }

    /// Whether this request is a compaction summarisation call.
    pub fn is_compaction_request(&self) -> bool {
        self.has_flag(FLAG_COMPACTION_REQUEST)
    }
}

/// Why a compaction ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionTrigger {
    /// The agent compacted by itself at its threshold.
    Auto,
    /// The user ran `/compact` (or equivalent).
    Manual,
    /// The log did not say (Codex does not record it).
    Unknown,
}

/// One compaction and its measured before/after sizes.
///
/// `pre_tokens` is the quantity the trigger compares against: Claude's logged
/// `preTokens`; Codex's last work response `input + output`. The logged
/// Claude `postTokens` is kept for reference only: it is **not** the next
/// prompt size. The measured post-compaction request is the first following
/// turn (`post_*` fields).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionEvent {
    /// Event time, Unix milliseconds (0 when unknown).
    pub timestamp_ms: i64,
    /// Index in [`SessionTrace::turns`] of the first request after the
    /// compaction; equals `turns.len()` while none followed.
    pub turn_index: u32,
    /// Why it ran.
    pub trigger: CompactionTrigger,
    /// Size the trigger fired at (see the type docs).
    pub pre_tokens: u32,
    /// Claude's logged `postTokens`; 0 when absent. Not the next prompt size.
    pub logged_post_tokens: u32,
    /// Context of the last request before the compaction.
    pub last_pre_context_tokens: u32,
    /// Whether a request after the compaction was observed (`post_*` valid).
    pub post_measured: bool,
    /// Context of the first request after the compaction.
    pub post_context_tokens: u32,
    /// Uncached input of that request.
    pub post_input_tokens: u32,
    /// Cache read of that request.
    pub post_cache_read_tokens: u32,
    /// 5-minute-tier cache write of that request.
    pub post_cache_write_5m_tokens: u32,
    /// 1-hour-tier cache write of that request.
    pub post_cache_write_1h_tokens: u32,
    /// Summary size in tokens (see `summary_estimated`).
    pub summary_tokens: u32,
    /// `true` when `summary_tokens` is a characters/3.5 estimate (Claude);
    /// `false` when it is the measured output of the summarisation request
    /// (Codex).
    pub summary_estimated: bool,
    /// Time the compaction took, milliseconds (0 when unknown).
    pub duration_ms: u32,
    /// Claude: the summary had been computed in advance in the background.
    pub precomputed: bool,
    /// Codex: estimated tokens of the replacement history (characters / 3.5);
    /// informational only.
    pub replacement_history_tokens: u32,
    /// Model of the last request before the compaction.
    pub model: u16,
    /// Codex: index of the summarisation request in `turns`.
    pub request_turn_index: Option<u32>,
}

impl CompactionEvent {
    /// Whether both a pre-compaction turn and a post-compaction turn exist, so
    /// the event can feed replay draws and post-size statistics.
    pub fn is_measured(&self) -> bool {
        self.post_measured && self.turn_index > 0
    }

    /// Tokens of the first post-compaction request that missed the cache
    /// (uncached input plus cache writes).
    pub fn post_fresh_tokens(&self) -> u32 {
        self.post_input_tokens
            .saturating_add(self.post_cache_write_5m_tokens)
            .saturating_add(self.post_cache_write_1h_tokens)
    }
}

/// Parse-time counters: how much of the input was understood.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceCounters {
    /// Lines handed to the builder (empty lines excluded).
    pub lines_fed: u64,
    /// Bytes handed to the builder.
    pub bytes_fed: u64,
    /// Lines rejected by the relevance prefilter without parsing.
    pub lines_irrelevant: u64,
    /// Lines longer than the byte limit, skipped unparsed.
    pub lines_skipped_oversize: u64,
    /// Lines that did not start with `{` (stray text, truncated tail).
    pub lines_not_json: u64,
    /// Lines that looked relevant but failed to deserialize.
    pub lines_unparseable: u64,
    /// Lines whose message or response identifier was already seen (Claude
    /// writes one line per content block; the last one wins, Codex keeps the
    /// first).
    pub duplicate_requests: u64,
    /// Requests dropped because their model is excluded (for example Haiku
    /// housekeeping calls inside a main Claude session).
    pub skipped_model_requests: u64,
    /// Claude sidechain requests dropped from a main-session trace.
    pub sidechain_requests_skipped: u64,
    /// Codex compactions whose summarisation request could not be located.
    pub compactions_unmatched: u64,
    /// Requests with a missing or non-positive usage block.
    pub requests_without_usage: u64,
    /// Tool-call lines whose features were not recorded (too large, or
    /// arriving for a request that is no longer the latest).
    #[serde(default)]
    pub tool_calls_dropped: u64,
}

/// A parsed session: the unit that callers cache per transcript file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionTrace {
    /// Layout version; must equal [`TRACE_FORMAT_VERSION`].
    pub format_version: u32,
    /// Which agent wrote the transcript.
    pub agent: AgentKind,
    /// Subagent / sidechain transcript (supplied by the caller, or detected
    /// from the Codex `session_meta` source).
    pub is_subagent: bool,
    /// Distinct model names; [`TurnSample::model`] indexes this table.
    pub models: Vec<String>,
    /// Deduplicated requests in log order.
    pub turns: Vec<TurnSample>,
    /// 32-bit hash of each turn's message/response identifier, parallel to
    /// `turns`; empty after deserialising a trace written without hashes.
    #[serde(default)]
    pub turn_id_hashes: Vec<u32>,
    /// Tool-call features, one row per request (hashes only, never text).
    #[serde(default)]
    pub tools: ToolFeatures,
    /// Compactions in log order.
    pub compactions: Vec<CompactionEvent>,
    /// Context window the log stated (Codex `model_context_window`).
    pub context_window_tokens: Option<u32>,
    /// What the parser skipped.
    pub counters: TraceCounters,
}

impl SessionTrace {
    /// An empty trace for an agent.
    pub fn empty(agent: AgentKind, is_subagent: bool) -> Self {
        Self {
            format_version: TRACE_FORMAT_VERSION,
            agent,
            is_subagent,
            models: Vec::new(),
            turns: Vec::new(),
            turn_id_hashes: Vec::new(),
            tools: ToolFeatures::default(),
            compactions: Vec::new(),
            context_window_tokens: None,
            counters: TraceCounters::default(),
        }
    }

    /// Serialises the trace as compact JSON.
    pub fn to_json_bytes(&self) -> AnalysisResult<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }

    /// Reads a trace written by [`SessionTrace::to_json_bytes`], refusing any
    /// other format version.
    pub fn from_json_bytes(bytes: &[u8]) -> AnalysisResult<Self> {
        #[derive(Deserialize)]
        struct VersionProbe {
            format_version: u32,
        }
        let probe: VersionProbe = serde_json::from_slice(bytes)?;
        if probe.format_version != TRACE_FORMAT_VERSION {
            return Err(AnalysisError::TraceVersionMismatch {
                found: probe.format_version,
                expected: TRACE_FORMAT_VERSION,
            });
        }
        Ok(serde_json::from_slice(bytes)?)
    }

    /// Name of a model-table entry, or the empty string when out of range.
    pub fn model_name(&self, model: u16) -> &str {
        self.models
            .get(usize::from(model))
            .map_or("", String::as_str)
    }

    /// Largest context of any request.
    pub fn max_context_tokens(&self) -> u32 {
        self.turns
            .iter()
            .map(|turn| turn.context_tokens)
            .max()
            .unwrap_or(0)
    }

    /// Context of the first request (the fixed prefix, C0).
    pub fn first_context_tokens(&self) -> Option<u32> {
        self.turns.first().map(|turn| turn.context_tokens)
    }

    /// Model index with the most requests, ties broken by the lowest index.
    pub fn dominant_model(&self) -> Option<u16> {
        let mut counts = vec![0_u32; self.models.len().max(1)];
        for turn in &self.turns {
            if let Some(slot) = counts.get_mut(usize::from(turn.model)) {
                *slot += 1;
            }
        }
        let (best_index, best_count) =
            counts
                .iter()
                .enumerate()
                .fold(
                    (0, 0),
                    |best, (index, &count)| {
                        if count > best.1 {
                            (index, count)
                        } else {
                            best
                        }
                    },
                );
        (best_count > 0).then_some(best_index as u16)
    }

    /// Share of cache-write tokens that went to the 5-minute tier (0 when the
    /// session wrote nothing).
    pub fn cache_write_5m_share(&self) -> f64 {
        let (short, long) = self
            .turns
            .iter()
            .fold((0_u64, 0_u64), |(short, long), turn| {
                (
                    short + u64::from(turn.cache_write_5m_tokens),
                    long + u64::from(turn.cache_write_1h_tokens),
                )
            });
        let total = short + long;
        if total == 0 {
            0.0
        } else {
            short as f64 / total as f64
        }
    }

    /// Indices of the compaction events that have both sides measured.
    pub fn measured_compactions(&self) -> impl Iterator<Item = &CompactionEvent> {
        self.compactions.iter().filter(|event| event.is_measured())
    }

    /// Recomputes the derived per-turn fields (`FLAG_CACHE_COLD`,
    /// `FLAG_GAP_COLD`, gap) after turns were removed, and re-links the
    /// compaction events to the shifted turn indices.
    ///
    /// `kept_before` maps each original turn index (plus one trailing entry
    /// for `turns.len()`) to its new index.
    pub(crate) fn rebuild_after_removal(&mut self, kept_before: &[u32]) {
        for event in &mut self.compactions {
            let original = event.turn_index as usize;
            event.turn_index = kept_before
                .get(original)
                .copied()
                .unwrap_or(event.turn_index);
            if let Some(request) = event.request_turn_index {
                event.request_turn_index = kept_before.get(request as usize).copied();
            }
        }
        let cold_gap = self.agent.cold_gap_seconds(self.is_subagent);
        let mut previous_work: Option<(u32, i64)> = None;
        let mut previous_any_ms: Option<i64> = None;
        for turn in &mut self.turns {
            derive_turn_fields(turn, previous_work, previous_any_ms, cold_gap);
            previous_any_ms = Some(turn.timestamp_ms);
            if !turn.is_compaction_request() {
                previous_work = Some((turn.context_tokens, turn.timestamp_ms));
            }
        }
    }
}

/// Sets the derived flags and the gap of `turn` from its predecessors.
///
/// `previous_work` is the context and time of the previous non-compaction
/// request; `previous_any_ms` the time of the immediately preceding request.
pub(crate) fn derive_turn_fields(
    turn: &mut TurnSample,
    previous_work: Option<(u32, i64)>,
    previous_any_ms: Option<i64>,
    cold_gap_seconds: u32,
) {
    turn.flags &= !(FLAG_CACHE_COLD | FLAG_GAP_COLD);
    turn.gap_decaseconds = 0;
    if let Some(previous_ms) = previous_any_ms {
        if turn.timestamp_ms > 0 && previous_ms > 0 && turn.timestamp_ms >= previous_ms {
            let gap_seconds = ((turn.timestamp_ms - previous_ms) / 1_000) as u64;
            let quantised = gap_seconds / u64::from(GAP_QUANTUM_SECONDS);
            turn.gap_decaseconds = u16::try_from(quantised).unwrap_or(u16::MAX);
            if gap_seconds > u64::from(cold_gap_seconds) {
                turn.flags |= FLAG_GAP_COLD;
            }
        }
    }
    // The first request after a compaction always misses (the prompt shrank);
    // that is the compaction's cost, not an idle-gap miss.
    if turn.has_flag(FLAG_FIRST_AFTER_COMPACTION) {
        return;
    }
    if let Some((previous_context, _)) = previous_work {
        let cache_missed = f64::from(turn.cache_read_tokens) < 0.5 * f64::from(previous_context);
        if previous_context > COLD_MIN_PREVIOUS_CONTEXT && cache_missed {
            turn.flags |= FLAG_CACHE_COLD;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(context: u32, read: u32, timestamp_ms: i64) -> TurnSample {
        TurnSample {
            timestamp_ms,
            context_tokens: context,
            input_tokens: 2,
            cache_read_tokens: read,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: context.saturating_sub(read + 2),
            output_tokens: 100,
            model: 0,
            flags: 0,
            gap_decaseconds: 0,
        }
    }

    #[test]
    fn turn_serializes_as_a_flat_array() {
        let turn = sample(50_000, 40_000, 1_000);
        let json = serde_json::to_string(&turn).unwrap();
        assert_eq!(json, "[1000,50000,2,40000,0,9998,100,0,0,0]");
        let back: TurnSample = serde_json::from_str(&json).unwrap();
        assert_eq!(back, turn);
    }

    #[test]
    fn trace_round_trips_and_rejects_other_versions() {
        let mut trace = SessionTrace::empty(AgentKind::Codex, true);
        trace.models.push("gpt-6.1-sol".to_string());
        trace.turns.push(sample(10, 5, 1));
        let bytes = trace.to_json_bytes().unwrap();
        assert_eq!(SessionTrace::from_json_bytes(&bytes).unwrap(), trace);

        let mut tampered = trace.clone();
        tampered.format_version = TRACE_FORMAT_VERSION + 1;
        let bytes = serde_json::to_vec(&tampered).unwrap();
        assert!(matches!(
            SessionTrace::from_json_bytes(&bytes),
            Err(AnalysisError::TraceVersionMismatch { .. })
        ));
    }

    #[test]
    fn derived_fields_flag_cold_cache_and_gap() {
        let mut turn = sample(100_000, 10_000, 4_000_000);
        derive_turn_fields(&mut turn, Some((90_000, 1_000)), Some(1_000), 3_600);
        assert!(turn.has_flag(FLAG_CACHE_COLD));
        assert!(turn.has_flag(FLAG_GAP_COLD));
        assert_eq!(turn.gap_seconds(), 3_990);

        let mut warm = sample(100_000, 89_000, 10_000);
        derive_turn_fields(&mut warm, Some((90_000, 1_000)), Some(1_000), 3_600);
        assert_eq!(warm.flags, 0);
        assert_eq!(warm.gap_seconds(), 0);

        let mut small = sample(15_000, 0, 10_000);
        derive_turn_fields(&mut small, Some((15_000, 1_000)), Some(1_000), 3_600);
        assert!(!small.has_flag(FLAG_CACHE_COLD));
    }

    #[test]
    fn a_turn_stays_compact() {
        // The memory budget quoted in the docs: at most 40 bytes per request.
        assert!(std::mem::size_of::<TurnSample>() <= 40);
    }

    #[test]
    fn dominant_model_and_share() {
        let mut trace = SessionTrace::empty(AgentKind::ClaudeCode, false);
        trace.models = vec!["a".into(), "b".into()];
        let mut first = sample(10, 5, 1);
        first.model = 1;
        let mut second = sample(10, 5, 2);
        second.model = 1;
        trace.turns = vec![first, second, sample(10, 5, 3)];
        assert_eq!(trace.dominant_model(), Some(1));
        assert_eq!(trace.cache_write_5m_share(), 0.0);
    }
}
