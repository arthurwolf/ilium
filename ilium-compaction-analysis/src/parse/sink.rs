//! The shared assembly point every log format feeds.
//!
//! A format parser decides *what* a line means; the sink owns the invariants
//! that must not differ between agents: the model table, derived cold-cache and
//! gap fields, and linking each compaction to the first request that follows
//! it.

use std::collections::HashMap;

use crate::agent::AgentKind;
use crate::trace::{
    derive_turn_fields, CompactionEvent, CompactionTrigger, SessionTrace, ToolAccumulator,
    TraceCounters, TurnSample, FLAG_COMPACTION_REQUEST, FLAG_FIRST_AFTER_COMPACTION,
};

/// A request as a format parser extracted it, before derived fields exist.
#[derive(Debug, Clone, Copy)]
pub struct RawTurn {
    /// Request time, Unix milliseconds (0 when unknown).
    pub timestamp_ms: i64,
    /// Prompt size of the main request.
    pub context_tokens: u32,
    /// Uncached input tokens.
    pub input_tokens: u32,
    /// Cache-read tokens.
    pub cache_read_tokens: u32,
    /// 5-minute-tier cache-write tokens.
    pub cache_write_5m_tokens: u32,
    /// 1-hour-tier cache-write tokens.
    pub cache_write_1h_tokens: u32,
    /// Output tokens.
    pub output_tokens: u32,
    /// Index into the model table (see [`TraceSink::model_index`]).
    pub model: u16,
    /// Hash of the message or response identifier.
    pub id_hash: u32,
}

impl RawTurn {
    fn into_sample(self, flags: u8) -> TurnSample {
        TurnSample {
            timestamp_ms: self.timestamp_ms,
            context_tokens: self.context_tokens,
            input_tokens: self.input_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_5m_tokens: self.cache_write_5m_tokens,
            cache_write_1h_tokens: self.cache_write_1h_tokens,
            output_tokens: self.output_tokens,
            model: self.model,
            flags,
            gap_decaseconds: 0,
        }
    }
}

/// Description of a compaction as a format parser extracted it.
#[derive(Debug, Clone, Default)]
pub struct RawCompaction {
    /// Event time, Unix milliseconds (0 when unknown).
    pub timestamp_ms: i64,
    /// Why it ran.
    pub trigger: Option<CompactionTrigger>,
    /// Size the trigger fired at.
    pub pre_tokens: u32,
    /// Logged post size (Claude `postTokens`); not the next prompt size.
    pub logged_post_tokens: u32,
    /// Summary size in tokens.
    pub summary_tokens: u32,
    /// Whether `summary_tokens` is an estimate.
    pub summary_estimated: bool,
    /// Duration in milliseconds.
    pub duration_ms: u32,
    /// Summary computed in advance.
    pub precomputed: bool,
    /// Estimated replacement-history tokens (Codex).
    pub replacement_history_tokens: u32,
}

/// Accumulates a trace on behalf of a format parser.
#[derive(Debug)]
pub struct TraceSink {
    pub(crate) trace: SessionTrace,
    model_indices: HashMap<String, u16>,
    previous_work: Option<(u32, i64)>,
    previous_any_ms: Option<i64>,
    pending_post: Option<usize>,
}

impl TraceSink {
    pub(crate) fn new(agent: AgentKind, is_subagent: bool) -> Self {
        Self {
            trace: SessionTrace::empty(agent, is_subagent),
            model_indices: HashMap::new(),
            previous_work: None,
            previous_any_ms: None,
            pending_post: None,
        }
    }

    pub(crate) fn into_trace(self) -> SessionTrace {
        self.trace
    }

    /// Mutable parse counters.
    pub fn counters_mut(&mut self) -> &mut TraceCounters {
        &mut self.trace.counters
    }

    /// Whether the trace is (so far) a subagent transcript.
    pub fn is_subagent(&self) -> bool {
        self.trace.is_subagent
    }

    /// Marks the trace as a subagent transcript (Codex learns this from
    /// `session_meta`).
    pub fn mark_subagent(&mut self) {
        self.trace.is_subagent = true;
    }

    /// Records the context window the log stated.
    pub fn set_context_window(&mut self, tokens: u32) {
        if tokens > 0 {
            self.trace.context_window_tokens = Some(tokens);
        }
    }

    /// Interns a model name into the table.
    pub fn model_index(&mut self, name: &str) -> u16 {
        if let Some(&index) = self.model_indices.get(name) {
            return index;
        }
        if self.trace.models.len() >= usize::from(u16::MAX) {
            return u16::MAX;
        }
        let index = self.trace.models.len() as u16;
        self.trace.models.push(name.to_string());
        self.model_indices.insert(name.to_string(), index);
        index
    }

    /// Number of turns pushed so far.
    pub fn turn_count(&self) -> usize {
        self.trace.turns.len()
    }

    /// The turn at `index`, if any.
    pub fn turn(&self, index: usize) -> Option<&TurnSample> {
        self.trace.turns.get(index)
    }

    /// Appends a request, computing its gap and cache-miss flags and, when a
    /// compaction is waiting for its first post-compaction request, recording
    /// that request's size and cache split on the event.
    pub fn push_turn(&mut self, raw: RawTurn) -> usize {
        self.push_turn_with_tools(raw, &ToolAccumulator::default())
    }

    /// Like [`TraceSink::push_turn`], recording the request's tool features.
    pub fn push_turn_with_tools(&mut self, raw: RawTurn, tools: &ToolAccumulator) -> usize {
        let index = self.trace.turns.len();
        let flags = if self.pending_post.is_some() {
            FLAG_FIRST_AFTER_COMPACTION
        } else {
            0
        };
        let mut sample = raw.into_sample(flags);
        let cold_gap = self.trace.agent.cold_gap_seconds(self.trace.is_subagent);
        derive_turn_fields(
            &mut sample,
            self.previous_work,
            self.previous_any_ms,
            cold_gap,
        );
        self.previous_any_ms = Some(sample.timestamp_ms);
        self.previous_work = Some((sample.context_tokens, sample.timestamp_ms));
        if let Some(event_index) = self.pending_post.take() {
            self.fill_post_measurement(event_index, &sample);
        }
        self.trace.turns.push(sample);
        self.trace.turn_id_hashes.push(raw.id_hash);
        self.trace.tools.push(tools);
        index
    }

    /// Adds tool features to the request at `index`. Only the latest request
    /// can grow (hashes are stored contiguously); returns `false` and counts
    /// the drop otherwise.
    pub fn add_tools_to_turn(&mut self, index: usize, tools: &ToolAccumulator) -> bool {
        if tools.is_empty() {
            return true;
        }
        if index + 1 == self.trace.turns.len() && self.trace.tools.merge_into_last(tools) {
            return true;
        }
        self.trace.counters.tool_calls_dropped += 1;
        false
    }

    /// Replaces the usage of an already-pushed turn (Claude writes the same
    /// message several times while streaming; the last line is final).
    pub fn replace_turn(&mut self, index: usize, raw: RawTurn) {
        let Some(existing) = self.trace.turns.get(index).copied() else {
            return;
        };
        let keep_flags = existing.flags & (FLAG_COMPACTION_REQUEST | FLAG_FIRST_AFTER_COMPACTION);
        let mut sample = raw.into_sample(keep_flags);
        // The request time is the first line of the message (as in the
        // research); later content-block lines only refine the usage.
        if existing.timestamp_ms != 0 {
            sample.timestamp_ms = existing.timestamp_ms;
        }
        let previous_work = index
            .checked_sub(1)
            .and_then(|previous| self.trace.turns.get(previous))
            .map(|turn| (turn.context_tokens, turn.timestamp_ms));
        let previous_any_ms = previous_work.map(|(_, timestamp)| timestamp);
        let cold_gap = self.trace.agent.cold_gap_seconds(self.trace.is_subagent);
        derive_turn_fields(&mut sample, previous_work, previous_any_ms, cold_gap);
        let is_last = index + 1 == self.trace.turns.len();
        self.trace.turns[index] = sample;
        if is_last {
            self.previous_work = Some((sample.context_tokens, sample.timestamp_ms));
            self.previous_any_ms = Some(sample.timestamp_ms);
        }
        if sample.has_flag(FLAG_FIRST_AFTER_COMPACTION) {
            let owning_event = self
                .trace
                .compactions
                .iter()
                .rposition(|event| event.turn_index as usize == index && event.post_measured);
            if let Some(event_index) = owning_event {
                self.fill_post_measurement(event_index, &sample);
            }
        }
    }

    fn fill_post_measurement(&mut self, event_index: usize, sample: &TurnSample) {
        let Some(event) = self.trace.compactions.get_mut(event_index) else {
            return;
        };
        event.post_measured = true;
        event.post_context_tokens = sample.context_tokens;
        event.post_input_tokens = sample.input_tokens;
        event.post_cache_read_tokens = sample.cache_read_tokens;
        event.post_cache_write_5m_tokens = sample.cache_write_5m_tokens;
        event.post_cache_write_1h_tokens = sample.cache_write_1h_tokens;
    }

    /// Opens a compaction that happened after the last pushed turn. Its
    /// post-compaction measurement is filled by the next pushed turn.
    pub fn open_compaction(&mut self, raw: RawCompaction) -> usize {
        let last_turn = self.trace.turns.last().copied();
        let event = CompactionEvent {
            timestamp_ms: raw.timestamp_ms,
            turn_index: u32::try_from(self.trace.turns.len()).unwrap_or(u32::MAX),
            trigger: raw.trigger.unwrap_or(CompactionTrigger::Unknown),
            pre_tokens: raw.pre_tokens,
            logged_post_tokens: raw.logged_post_tokens,
            last_pre_context_tokens: last_turn.map_or(0, |turn| turn.context_tokens),
            post_measured: false,
            post_context_tokens: 0,
            post_input_tokens: 0,
            post_cache_read_tokens: 0,
            post_cache_write_5m_tokens: 0,
            post_cache_write_1h_tokens: 0,
            summary_tokens: raw.summary_tokens,
            summary_estimated: raw.summary_estimated,
            duration_ms: raw.duration_ms,
            precomputed: raw.precomputed,
            replacement_history_tokens: raw.replacement_history_tokens,
            model: last_turn.map_or(0, |turn| turn.model),
            request_turn_index: None,
        };
        self.trace.compactions.push(event);
        let event_index = self.trace.compactions.len() - 1;
        self.pending_post = Some(event_index);
        event_index
    }

    /// Codex: declares that the already-pushed turn `request_index` is the
    /// summarisation call of a compaction. The compaction's pre size is the
    /// work turn before it; the request's output is the summary.
    ///
    /// Returns `false` (and changes nothing) when there is no work turn before
    /// the request.
    pub fn open_compaction_from_request(
        &mut self,
        request_index: usize,
        mut raw: RawCompaction,
    ) -> bool {
        let Some(request) = self.trace.turns.get(request_index).copied() else {
            return false;
        };
        let Some(previous_index) = request_index.checked_sub(1) else {
            return false;
        };
        let previous = self.trace.turns[previous_index];
        if request.has_flag(FLAG_FIRST_AFTER_COMPACTION) {
            // Back-to-back compactions: the request is not a work request, so
            // the earlier compaction never saw its post-compaction request.
            self.trace.turns[request_index].flags &= !FLAG_FIRST_AFTER_COMPACTION;
            if let Some(earlier) = self
                .trace
                .compactions
                .iter_mut()
                .rfind(|event| event.turn_index as usize == request_index)
            {
                earlier.post_measured = false;
                earlier.post_context_tokens = 0;
                earlier.post_input_tokens = 0;
                earlier.post_cache_read_tokens = 0;
                earlier.post_cache_write_5m_tokens = 0;
                earlier.post_cache_write_1h_tokens = 0;
            }
        }
        self.trace.turns[request_index].flags |= FLAG_COMPACTION_REQUEST;
        self.previous_work = Some((previous.context_tokens, previous.timestamp_ms));
        raw.pre_tokens = previous
            .context_tokens
            .saturating_add(previous.output_tokens);
        raw.summary_tokens = request.output_tokens;
        raw.summary_estimated = false;
        let event_index = self.open_compaction(raw);
        let event = &mut self.trace.compactions[event_index];
        event.last_pre_context_tokens = previous.context_tokens;
        event.model = request.model;
        event.request_turn_index = u32::try_from(request_index).ok();
        true
    }

    /// Sets the summary size of the most recent compaction if no request has
    /// followed it and it has none yet. Returns whether it was attached.
    pub fn attach_summary_to_open_compaction(&mut self, tokens: u32) -> bool {
        let turn_count = self.trace.turns.len();
        match self.trace.compactions.last_mut() {
            Some(event) if event.turn_index as usize == turn_count && event.summary_tokens == 0 => {
                event.summary_tokens = tokens;
                event.summary_estimated = true;
                true
            }
            _ => false,
        }
    }
}
