//! Costs-and-stats extraction from a coding agent's own JSONL transcript.
//!
//! The transcript is the only complete record of a session: which models
//! answered, how many tokens each call used, how long turns took, which tools
//! ran, and (for Claude Code) the cost snapshots the CLI writes itself. This
//! module turns that stream into a [`SessionStats`] value the popover can draw.
//!
//! Real transcripts reach several gigabytes with single lines above 30 MB
//! (embedded screenshots, tool output), so parsing is built around two rules:
//!
//! * **Incremental.** [`StatsAccumulator`] remembers how many bytes it has
//!   consumed and resumes from there, so a refresh of a live session only pays
//!   for the newly appended lines.
//! * **Filtered before parsed.** A byte-level pre-filter decides which lines
//!   are worth a full `serde_json` parse; the multi-megabyte records that carry
//!   no statistics are skipped after being read, never deserialised.
//!
//! No figure here is invented. Token counts come from the provider's own usage
//! objects, and cost is shown only where the CLI recorded it (Claude Code's
//! `cost-state` snapshot); otherwise the field stays `None`.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::sync::OnceLock;

use ilium_core::AgentClass;
use regex::bytes::{Regex, RegexSet};
use serde_json::Value;

/// Most recent user prompts kept for the Prompts tab.
pub const RECENT_PROMPT_LIMIT: usize = 24;
/// Characters kept per stored prompt; the popover clips further to its width.
const PROMPT_TEXT_LIMIT: usize = 1_500;
/// Upper bound on stored per-call samples; older ones are merged pairwise.
const SAMPLE_CAP: usize = 4_000;
/// Upper bound on stored activity timestamps; halved when exceeded.
const ACTIVITY_CAP: usize = 20_000;
/// Upper bound on stored turn durations (most recent kept).
const TURN_DURATION_CAP: usize = 400;
/// Bytes of a Codex line inspected by the pre-filter. Every record's kind
/// keys sit at the start of the line, ahead of any bulky payload body.
const CODEX_PREFIX_BYTES: usize = 640;

/// Token counts normalised across providers so the two never disagree on what
/// a column means. `input` is uncached prompt input, `cache_read` and
/// `cache_write` are prompt-cache traffic, `output` includes `reasoning`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenTotals {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
    /// Subset of `output` spent on hidden reasoning, where the provider says.
    pub reasoning: u64,
}

impl TokenTotals {
    pub fn total(&self) -> u64 {
        self.input + self.cache_read + self.cache_write + self.output
    }

    /// Everything sent to the model, cached or not.
    pub fn prompt_side(&self) -> u64 {
        self.input + self.cache_read + self.cache_write
    }

    /// Share of prompt-side tokens served from the cache, when any were sent.
    pub fn cache_hit_ratio(&self) -> Option<f64> {
        let prompt = self.prompt_side();
        (prompt > 0).then(|| self.cache_read as f64 / prompt as f64)
    }

    fn add(&mut self, other: &Self) {
        self.input += other.input;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.output += other.output;
        self.reasoning += other.reasoning;
    }

    fn saturating_sub(&self, other: &Self) -> Self {
        Self {
            input: self.input.saturating_sub(other.input),
            cache_read: self.cache_read.saturating_sub(other.cache_read),
            cache_write: self.cache_write.saturating_sub(other.cache_write),
            output: self.output.saturating_sub(other.output),
            reasoning: self.reasoning.saturating_sub(other.reasoning),
        }
    }

    fn max_merge(&mut self, other: &Self) {
        self.input = self.input.max(other.input);
        self.cache_read = self.cache_read.max(other.cache_read);
        self.cache_write = self.cache_write.max(other.cache_write);
        self.output = self.output.max(other.output);
        self.reasoning = self.reasoning.max(other.reasoning);
    }
}

/// Usage attributed to one model over the whole session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelUsage {
    pub model: String,
    pub calls: u32,
    pub tokens: TokenTotals,
}

/// One user prompt as recorded in the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptRecord {
    pub at_ms: Option<i64>,
    pub text: String,
    /// Length of the original text in characters, before clipping.
    pub characters: usize,
    /// How many times in a row this exact text was sent (goal continuations
    /// re-send one objective every turn); `1` for an ordinary prompt.
    pub repeats: u32,
}

/// One model call's token delta, for charts. `context` is the size of the
/// prompt sent on that call: how full the model's window was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSample {
    pub at_ms: i64,
    pub tokens: TokenTotals,
    pub context: Option<u64>,
}

/// A rate-limit window as reported by the provider (Codex).
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitWindow {
    pub used_percent: f64,
    pub window_minutes: Option<u64>,
    pub resets_at_unix: Option<i64>,
}

/// Cost figures Claude Code recorded itself. The CLI writes these snapshots
/// periodically rather than per call, so for a live pane the figure can trail
/// the token counters; the popover labels it accordingly.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportedCost {
    pub total_usd: f64,
    pub lines_added: u64,
    pub lines_removed: u64,
    pub api_duration_ms: u64,
    pub tool_duration_ms: u64,
    /// True when at least one model had no known price at snapshot time.
    pub has_unknown_model_cost: bool,
    /// Per-model dollars from the same snapshot.
    pub model_costs: Vec<(String, f64)>,
}

/// Everything the popover shows about one session.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionStats {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub advisor_model: Option<String>,
    pub cli_version: Option<String>,
    pub git_branch: Option<String>,
    pub cwd: Option<String>,
    pub plan_type: Option<String>,

    pub first_at_ms: Option<i64>,
    pub last_at_ms: Option<i64>,

    pub prompt_count: u32,
    pub model_calls: u32,
    pub turns_completed: u32,
    pub turns_aborted: u32,
    pub compactions: u32,
    pub api_errors: u32,
    pub tool_calls: u32,
    /// `None` when the provider's transcript does not flag failed tools.
    pub tool_errors: Option<u32>,

    pub tokens: TokenTotals,
    pub models: Vec<ModelUsage>,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub peak_context_tokens: u64,

    pub tools: Vec<(String, u32)>,
    pub recent_prompts: Vec<PromptRecord>,

    pub active_ms: u64,
    pub longest_turn_ms: u64,
    pub turn_durations_ms: Vec<u64>,
    pub average_first_token_ms: Option<u64>,

    pub cost: Option<ReportedCost>,
    pub rate_limits: Vec<(String, RateLimitWindow)>,

    pub samples: Vec<TokenSample>,
    pub activity_ms: Vec<i64>,

    pub bytes_read: u64,
    pub records_parsed: u64,
}

/// One slice of the session timeline, for the Activity charts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimelineBucket {
    pub start_ms: i64,
    pub tokens: TokenTotals,
    pub calls: u32,
    pub peak_context: u64,
    pub events: u32,
}

impl SessionStats {
    /// Wall-clock span from the first to the last recorded event.
    pub fn span_ms(&self) -> Option<u64> {
        let (first, last) = (self.first_at_ms?, self.last_at_ms?);
        Some(last.saturating_sub(first).max(0) as u64)
    }

    /// Fraction of the model window the latest prompt filled, where known.
    pub fn context_fill(&self) -> Option<f64> {
        let (used, window) = (self.context_tokens?, self.context_window?);
        (window > 0).then(|| (used as f64 / window as f64).min(1.0))
    }

    /// Splits the session into `count` equal time slices covering the first
    /// through last recorded event, folding samples and activity into them.
    pub fn timeline(&self, count: usize) -> Vec<TimelineBucket> {
        let (Some(first), Some(last)) = (self.first_at_ms, self.last_at_ms) else {
            return Vec::new();
        };
        if count == 0 {
            return Vec::new();
        }
        let span = (last - first).max(1);
        let width = (span as f64 / count as f64).max(1.0);
        let index_for = |at_ms: i64| -> usize {
            (((at_ms - first).max(0) as f64 / width) as usize).min(count - 1)
        };
        let mut buckets: Vec<TimelineBucket> = (0..count)
            .map(|index| TimelineBucket {
                start_ms: first + (index as f64 * width) as i64,
                ..TimelineBucket::default()
            })
            .collect();
        for sample in &self.samples {
            let bucket = &mut buckets[index_for(sample.at_ms)];
            bucket.tokens.add(&sample.tokens);
            bucket.calls += 1;
            bucket.peak_context = bucket.peak_context.max(sample.context.unwrap_or(0));
        }
        for at_ms in &self.activity_ms {
            buckets[index_for(*at_ms)].events += 1;
        }
        buckets
    }
}

/// A Claude model call, merged across the several transcript records the CLI
/// writes for one response (one per content block, all carrying full usage).
#[derive(Debug, Clone)]
struct ClaudeCall {
    at_ms: Option<i64>,
    model: String,
    tokens: TokenTotals,
    sidechain: bool,
}

/// Incremental parser. Feed it a transcript file with [`Self::ingest_file`];
/// call [`Self::snapshot`] whenever a value for display is needed.
#[derive(Debug, Clone)]
pub struct StatsAccumulator {
    class: AgentClass,
    offset: u64,
    stats: SessionStats,

    tool_counts: HashMap<String, u32>,
    seen_tool_ids: HashSet<String>,
    prompts: Vec<PromptRecord>,
    // Codex has recorded user turns in two different shapes; only one is used
    // in the snapshot (the response-item stream when present).
    event_prompts: Vec<PromptRecord>,
    event_prompt_count: u32,
    response_prompt_count: u32,
    turn_durations: Vec<u64>,
    first_token_total_ms: u64,
    first_token_count: u64,
    cost_snapshot: Option<ReportedCost>,

    claude_calls: Vec<ClaudeCall>,
    claude_call_index: HashMap<String, usize>,
    claude_tool_errors: u32,

    codex_last_total: TokenTotals,
    codex_model_usage: HashMap<String, (u32, TokenTotals)>,
    codex_totals: TokenTotals,
    codex_current_model: Option<String>,
    codex_rate_limits: Vec<(String, RateLimitWindow)>,
    codex_samples: Vec<TokenSample>,
}

impl StatsAccumulator {
    pub fn new(class: AgentClass) -> Self {
        Self {
            class,
            offset: 0,
            stats: SessionStats::default(),
            tool_counts: HashMap::new(),
            seen_tool_ids: HashSet::new(),
            prompts: Vec::new(),
            event_prompts: Vec::new(),
            event_prompt_count: 0,
            response_prompt_count: 0,
            turn_durations: Vec::new(),
            first_token_total_ms: 0,
            first_token_count: 0,
            cost_snapshot: None,
            claude_calls: Vec::new(),
            claude_call_index: HashMap::new(),
            claude_tool_errors: 0,
            codex_last_total: TokenTotals::default(),
            codex_model_usage: HashMap::new(),
            codex_totals: TokenTotals::default(),
            codex_current_model: None,
            codex_rate_limits: Vec::new(),
            codex_samples: Vec::new(),
        }
    }

    /// Bytes consumed so far (always at a line boundary).
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Reads any bytes appended since the last call. Progress is reported at
    /// most every `PROGRESS_INTERVAL_BYTES` so the caller can publish partial
    /// snapshots while a multi-gigabyte transcript is still being scanned.
    ///
    /// A file shorter than the remembered offset was replaced or truncated, so
    /// the accumulator restarts from zero rather than reporting stale totals.
    pub fn ingest_file(
        &mut self,
        path: &Path,
        mut on_progress: impl FnMut(&Self, u64, u64),
    ) -> std::io::Result<()> {
        const PROGRESS_INTERVAL_BYTES: u64 = 24 * 1024 * 1024;

        let file = std::fs::File::open(path)?;
        let length = file.metadata()?.len();
        if length < self.offset {
            *self = Self::new(self.class.clone());
        }
        let mut reader = BufReader::with_capacity(1 << 20, file);
        reader.seek(SeekFrom::Start(self.offset))?;
        let mut line = Vec::new();
        let mut since_progress = 0_u64;
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            // A last line without its newline may still be mid-write. Leaving
            // it unconsumed makes the next refresh re-read it whole.
            if line.last() != Some(&b'\n') {
                break;
            }
            self.offset += read as u64;
            since_progress += read as u64;
            self.feed_line(&line);
            if since_progress >= PROGRESS_INTERVAL_BYTES {
                since_progress = 0;
                on_progress(self, self.offset, length);
            }
        }
        on_progress(self, self.offset, length);
        Ok(())
    }

    /// Consumes one transcript line. Unknown, malformed, or irrelevant lines
    /// are ignored: one bad record must never hide the rest of the session.
    pub fn feed_line(&mut self, line: &[u8]) {
        match self.class {
            AgentClass::Claude => self.feed_claude(line),
            AgentClass::Codex => self.feed_codex(line),
            AgentClass::Antigravity | AgentClass::Other(_) => {}
        }
    }

    // ----------------------------------------------------------- Claude Code

    fn feed_claude(&mut self, line: &[u8]) {
        let filters = claude_filters();
        // A tool-result line can carry megabytes of output. Failures are
        // counted straight from the bytes, and the line is never parsed.
        if filters.tool_result.is_match(line) {
            self.claude_tool_errors += filters.tool_error.find_iter(line).count() as u32;
            return;
        }
        if !filters.relevant.is_match(line) {
            return;
        }
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        self.stats.records_parsed += 1;
        let at_ms = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp_ms);
        let sidechain = record
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match record.get("type").and_then(Value::as_str) {
            Some("assistant") => self.claude_assistant(&record, at_ms, sidechain),
            Some("user") => self.claude_user(&record, at_ms, sidechain),
            Some("system") => self.claude_system(&record, at_ms),
            Some("cost-state") => self.claude_cost(&record),
            _ => {}
        }
    }

    fn claude_assistant(&mut self, record: &Value, at_ms: Option<i64>, sidechain: bool) {
        self.touch(at_ms);
        set_if_some(&mut self.stats.cli_version, string_at(record, "version"));
        set_if_some(&mut self.stats.git_branch, string_at(record, "gitBranch"));
        set_if_some(&mut self.stats.cwd, string_at(record, "cwd"));
        if !sidechain {
            set_if_some(&mut self.stats.effort, string_at(record, "effort"));
            set_if_some(
                &mut self.stats.advisor_model,
                string_at(record, "advisorModel"),
            );
        }
        let Some(message) = record.get("message") else {
            return;
        };
        let model = message.get("model").and_then(Value::as_str).unwrap_or("");
        if model == "<synthetic>" {
            // Client-generated placeholder (API error, interruption): no
            // tokens were spent, but the event itself is worth counting.
            self.stats.api_errors += 1;
            return;
        }
        if let Some(Value::Array(blocks)) = message.get("content") {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                    continue;
                }
                let id = block.get("id").and_then(Value::as_str).unwrap_or("");
                if !id.is_empty() && !self.seen_tool_ids.insert(id.to_string()) {
                    continue;
                }
                let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                self.count_tool(name, at_ms);
            }
        }
        let Some(usage) = message.get("usage") else {
            return;
        };
        let tokens = claude_tokens(usage);
        let id = message.get("id").and_then(Value::as_str).unwrap_or("");
        // Every content block of one response repeats the same usage object,
        // so merging by message id (never summing) avoids multi-counting.
        if let Some(&index) = self.claude_call_index.get(id).filter(|_| !id.is_empty()) {
            self.claude_calls[index].tokens.max_merge(&tokens);
            return;
        }
        if !id.is_empty() {
            self.claude_call_index
                .insert(id.to_string(), self.claude_calls.len());
        }
        self.claude_calls.push(ClaudeCall {
            at_ms,
            model: model.to_string(),
            tokens,
            sidechain,
        });
    }

    fn claude_user(&mut self, record: &Value, at_ms: Option<i64>, sidechain: bool) {
        if sidechain || record.get("isMeta").and_then(Value::as_bool) == Some(true) {
            return;
        }
        if record.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
            self.stats.compactions += 1;
            return;
        }
        let origin_kind = record
            .get("origin")
            .and_then(|origin| origin.get("kind"))
            .and_then(Value::as_str);
        if !matches!(origin_kind, None | Some("human")) {
            return;
        }
        if record.get("promptSource").and_then(Value::as_str) == Some("system") {
            return;
        }
        let Some(text) = record
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(claude_prompt_text)
        else {
            return;
        };
        if is_claude_synthetic_prompt(&text) {
            return;
        }
        self.touch(at_ms);
        self.stats.prompt_count += 1;
        self.push_prompt(at_ms, &text);
        if let Some(at_ms) = at_ms {
            self.push_activity(at_ms);
        }
    }

    fn claude_system(&mut self, record: &Value, at_ms: Option<i64>) {
        match record.get("subtype").and_then(Value::as_str) {
            Some("turn_duration") => {
                self.touch(at_ms);
                if let Some(duration) = record.get("durationMs").and_then(Value::as_u64) {
                    self.stats.turns_completed += 1;
                    self.push_turn_duration(duration);
                }
            }
            Some("compact_boundary") => self.stats.compactions += 1,
            _ => {}
        }
    }

    fn claude_cost(&mut self, record: &Value) {
        let mut model_costs: Vec<(String, f64)> = record
            .get("modelUsage")
            .and_then(Value::as_object)
            .map(|models| {
                models
                    .iter()
                    .filter_map(|(model, usage)| {
                        Some((model.clone(), usage.get("costUSD")?.as_f64()?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        model_costs.sort_by(|a, b| b.1.total_cmp(&a.1));
        self.cost_snapshot = Some(ReportedCost {
            total_usd: record
                .get("totalCostUSD")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            lines_added: record
                .get("totalLinesAdded")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            lines_removed: record
                .get("totalLinesRemoved")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            api_duration_ms: record
                .get("totalAPIDuration")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as u64,
            tool_duration_ms: record
                .get("totalToolDuration")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as u64,
            has_unknown_model_cost: record
                .get("hasUnknownModelCost")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            model_costs,
        });
    }

    // ----------------------------------------------------------------- Codex

    fn feed_codex(&mut self, line: &[u8]) {
        let prefix = &line[..line.len().min(CODEX_PREFIX_BYTES)];
        let Some(kind) = codex_filters().classify(prefix) else {
            return;
        };
        if kind == CodexLine::Compacted {
            self.stats.compactions += 1;
            return;
        }
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        self.stats.records_parsed += 1;
        let at_ms = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp_ms);
        let Some(payload) = record.get("payload") else {
            return;
        };
        match kind {
            CodexLine::SessionMeta => {
                self.touch(at_ms);
                set_if_some(
                    &mut self.stats.cli_version,
                    string_at(payload, "cli_version"),
                );
                set_if_some(&mut self.stats.cwd, string_at(payload, "cwd"));
                set_if_some(
                    &mut self.stats.provider,
                    string_at(payload, "model_provider"),
                );
                // The session's own start precedes its first logged event.
                if let Some(started) = payload
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_timestamp_ms)
                {
                    self.stats.first_at_ms =
                        Some(self.stats.first_at_ms.map_or(started, |v| v.min(started)));
                }
            }
            CodexLine::TurnContext => {
                self.touch(at_ms);
                if let Some(model) = string_at(payload, "model") {
                    self.codex_current_model = Some(model.clone());
                    self.stats.model = Some(model);
                }
                set_if_some(&mut self.stats.effort, string_at(payload, "effort"));
            }
            CodexLine::TokenCount => self.codex_token_count(payload, at_ms),
            CodexLine::TaskStarted => {
                self.touch(at_ms);
                if let Some(window) = payload.get("model_context_window").and_then(Value::as_u64) {
                    self.stats.context_window = Some(window);
                }
            }
            CodexLine::TaskComplete => {
                self.touch(at_ms);
                if let Some(duration) = payload.get("duration_ms").and_then(Value::as_u64) {
                    self.stats.turns_completed += 1;
                    self.push_turn_duration(duration);
                }
                if let Some(first_token) = payload
                    .get("time_to_first_token_ms")
                    .and_then(Value::as_u64)
                {
                    self.first_token_total_ms += first_token;
                    self.first_token_count += 1;
                }
            }
            CodexLine::TurnAborted => {
                self.touch(at_ms);
                self.stats.turns_aborted += 1;
            }
            CodexLine::EventUser | CodexLine::GoalUpdated => {
                let text = if kind == CodexLine::EventUser {
                    payload.get("message").and_then(Value::as_str)
                } else {
                    payload
                        .get("goal")
                        .and_then(|goal| goal.get("objective"))
                        .and_then(Value::as_str)
                };
                if let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) {
                    self.touch(at_ms);
                    self.event_prompt_count += 1;
                    push_bounded_prompt(&mut self.event_prompts, at_ms, text);
                }
            }
            CodexLine::ResponseUser => {
                let Some(text) = crate::transcript_context::textual_value(payload.get("content"))
                else {
                    return;
                };
                if crate::transcript_context::is_codex_context_envelope(&text) {
                    return;
                }
                self.touch(at_ms);
                self.response_prompt_count += 1;
                push_bounded_prompt(&mut self.prompts, at_ms, &text);
            }
            CodexLine::ToolCall => {
                self.touch(at_ms);
                let name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                self.count_tool(name, at_ms);
            }
            CodexLine::Compacted => {}
        }
    }

    fn codex_token_count(&mut self, payload: &Value, at_ms: Option<i64>) {
        self.touch(at_ms);
        if let Some(limits) = payload.get("rate_limits") {
            set_if_some(&mut self.stats.plan_type, string_at(limits, "plan_type"));
            let mut parsed = Vec::new();
            for key in ["primary", "secondary"] {
                let Some(window) = limits.get(key).filter(|window| window.is_object()) else {
                    continue;
                };
                let Some(used_percent) = window.get("used_percent").and_then(Value::as_f64) else {
                    continue;
                };
                parsed.push((
                    key.to_string(),
                    RateLimitWindow {
                        used_percent,
                        window_minutes: window.get("window_minutes").and_then(Value::as_u64),
                        resets_at_unix: window.get("resets_at").and_then(Value::as_i64),
                    },
                ));
            }
            if !parsed.is_empty() {
                self.codex_rate_limits = parsed;
            }
        }
        let Some(info) = payload.get("info").filter(|info| info.is_object()) else {
            return;
        };
        if let Some(window) = info.get("model_context_window").and_then(Value::as_u64) {
            self.stats.context_window = Some(window);
        }
        let Some(total) = info.get("total_token_usage").map(codex_tokens) else {
            return;
        };
        // A cumulative figure that went backwards means the CLI reset its
        // counter; the new value is then a fresh baseline, not a negative delta.
        let delta = if total.total() < self.codex_last_total.total() {
            total
        } else {
            total.saturating_sub(&self.codex_last_total)
        };
        if delta.total() == 0 && delta.reasoning == 0 {
            // The CLI repeats the same cumulative figure on several events.
            return;
        }
        self.codex_last_total = total;
        self.codex_totals.add(&delta);
        let context = info
            .get("last_token_usage")
            .and_then(|last| last.get("input_tokens"))
            .and_then(Value::as_u64);
        if let Some(context) = context {
            self.stats.context_tokens = Some(context);
            self.stats.peak_context_tokens = self.stats.peak_context_tokens.max(context);
        }
        let model = self
            .codex_current_model
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let usage = self.codex_model_usage.entry(model).or_default();
        usage.0 += 1;
        usage.1.add(&delta);
        self.stats.model_calls += 1;
        if let Some(at_ms) = at_ms {
            self.push_activity(at_ms);
            push_sample(
                &mut self.codex_samples,
                TokenSample {
                    at_ms,
                    tokens: delta,
                    context,
                },
            );
        }
    }

    // ---------------------------------------------------------------- shared

    fn touch(&mut self, at_ms: Option<i64>) {
        let Some(at_ms) = at_ms else {
            return;
        };
        self.stats.first_at_ms = Some(self.stats.first_at_ms.map_or(at_ms, |v| v.min(at_ms)));
        self.stats.last_at_ms = Some(self.stats.last_at_ms.map_or(at_ms, |v| v.max(at_ms)));
    }

    fn count_tool(&mut self, name: &str, at_ms: Option<i64>) {
        self.stats.tool_calls += 1;
        *self.tool_counts.entry(name.to_string()).or_insert(0) += 1;
        if let Some(at_ms) = at_ms {
            self.push_activity(at_ms);
        }
    }

    fn push_activity(&mut self, at_ms: i64) {
        self.stats.activity_ms.push(at_ms);
        if self.stats.activity_ms.len() > ACTIVITY_CAP {
            // Thinning keeps the density profile while bounding memory.
            let thinned: Vec<i64> = self.stats.activity_ms.iter().copied().step_by(2).collect();
            self.stats.activity_ms = thinned;
        }
    }

    fn push_prompt(&mut self, at_ms: Option<i64>, text: &str) {
        push_bounded_prompt(&mut self.prompts, at_ms, text);
    }

    fn push_turn_duration(&mut self, duration_ms: u64) {
        self.stats.active_ms += duration_ms;
        self.stats.longest_turn_ms = self.stats.longest_turn_ms.max(duration_ms);
        self.turn_durations.push(duration_ms);
        if self.turn_durations.len() > TURN_DURATION_CAP {
            self.turn_durations.remove(0);
        }
    }

    /// Builds the display value from everything consumed so far.
    pub fn snapshot(&self) -> SessionStats {
        let mut stats = self.stats.clone();
        stats.bytes_read = self.offset;
        stats.tools = {
            let mut tools: Vec<(String, u32)> = self
                .tool_counts
                .iter()
                .map(|(name, count)| (name.clone(), *count))
                .collect();
            tools.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            tools
        };
        stats.turn_durations_ms = self.turn_durations.clone();
        stats.average_first_token_ms = (self.first_token_count > 0)
            .then(|| self.first_token_total_ms / self.first_token_count);

        match self.class {
            AgentClass::Claude => self.finish_claude(&mut stats),
            AgentClass::Codex => self.finish_codex(&mut stats),
            AgentClass::Antigravity | AgentClass::Other(_) => {}
        }
        stats
    }

    fn finish_claude(&self, stats: &mut SessionStats) {
        stats.provider = Some("Anthropic".to_string());
        stats.tool_errors = Some(self.claude_tool_errors);
        stats.recent_prompts = recent(&self.prompts);
        let mut by_model: HashMap<&str, (u32, TokenTotals)> = HashMap::new();
        let mut samples = Vec::with_capacity(self.claude_calls.len());
        for call in &self.claude_calls {
            stats.tokens.add(&call.tokens);
            let usage = by_model.entry(call.model.as_str()).or_default();
            usage.0 += 1;
            usage.1.add(&call.tokens);
            if call.sidechain {
                continue;
            }
            stats.model_calls += 1;
            // The prompt of a call is everything sent, cached or not.
            let context = call.tokens.prompt_side();
            stats.context_tokens = Some(context);
            stats.peak_context_tokens = stats.peak_context_tokens.max(context);
            stats.model = Some(call.model.clone());
            if let Some(at_ms) = call.at_ms {
                samples.push(TokenSample {
                    at_ms,
                    tokens: call.tokens,
                    context: Some(context),
                });
            }
        }
        // Sub-agent calls are real spend, so they stay in the totals; only
        // the main conversation defines the "current" model and context.
        stats.models = model_list(by_model.into_iter().map(|(m, v)| (m.to_string(), v)));
        stats.samples = decimate(samples);
        stats.cost = self.cost_snapshot.clone();
    }

    fn finish_codex(&self, stats: &mut SessionStats) {
        stats.tokens = self.codex_totals;
        stats.tool_errors = None;
        stats.provider = stats
            .provider
            .clone()
            .map(|provider| match provider.as_str() {
                "openai" => "OpenAI".to_string(),
                other => capitalise(other),
            })
            .or_else(|| Some("OpenAI".to_string()));
        stats.models = model_list(
            self.codex_model_usage
                .iter()
                .map(|(model, value)| (model.clone(), *value)),
        );
        stats.rate_limits = self.codex_rate_limits.clone();
        stats.samples = decimate(self.codex_samples.clone());
        if self.response_prompt_count > 0 {
            stats.recent_prompts = recent(&self.prompts);
            stats.prompt_count = self.response_prompt_count;
        } else {
            stats.recent_prompts = recent(&self.event_prompts);
            stats.prompt_count = self.event_prompt_count;
        }
    }
}

// --------------------------------------------------------------------- helpers

fn recent(prompts: &[PromptRecord]) -> Vec<PromptRecord> {
    let start = prompts.len().saturating_sub(RECENT_PROMPT_LIMIT);
    prompts[start..].to_vec()
}

fn push_bounded_prompt(target: &mut Vec<PromptRecord>, at_ms: Option<i64>, text: &str) {
    let text = text.trim();
    let clipped: String = text.chars().take(PROMPT_TEXT_LIMIT).collect();
    if let Some(last) = target.last_mut().filter(|last| last.text == clipped) {
        last.repeats += 1;
        last.at_ms = at_ms.or(last.at_ms);
        return;
    }
    target.push(PromptRecord {
        at_ms,
        characters: text.chars().count(),
        text: clipped,
        repeats: 1,
    });
    if target.len() > RECENT_PROMPT_LIMIT * 2 {
        target.drain(..target.len() - RECENT_PROMPT_LIMIT);
    }
}

fn push_sample(samples: &mut Vec<TokenSample>, sample: TokenSample) {
    samples.push(sample);
    if samples.len() > SAMPLE_CAP * 2 {
        *samples = decimate(std::mem::take(samples));
    }
}

/// Merges neighbouring samples pairwise until at most `SAMPLE_CAP` remain.
/// Token deltas add up and context takes the larger value, so charts built
/// from the merged series keep the same totals and peaks.
fn decimate(mut samples: Vec<TokenSample>) -> Vec<TokenSample> {
    while samples.len() > SAMPLE_CAP {
        samples = samples
            .chunks(2)
            .map(|pair| {
                let mut merged = pair[0];
                if let Some(second) = pair.get(1) {
                    merged.tokens.add(&second.tokens);
                    merged.at_ms = second.at_ms;
                    merged.context = merged.context.max(second.context);
                }
                merged
            })
            .collect();
    }
    samples
}

fn model_list(entries: impl Iterator<Item = (String, (u32, TokenTotals))>) -> Vec<ModelUsage> {
    let mut models: Vec<ModelUsage> = entries
        .map(|(model, (calls, tokens))| ModelUsage {
            model,
            calls,
            tokens,
        })
        .collect();
    models.sort_by(|a, b| {
        b.tokens
            .total()
            .cmp(&a.tokens.total())
            .then_with(|| a.model.cmp(&b.model))
    });
    models
}

fn capitalise(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

fn set_if_some(target: &mut Option<String>, value: Option<String>) {
    if value.is_some() {
        *target = value;
    }
}

fn string_at(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn parse_timestamp_ms(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.timestamp_millis())
}

fn claude_tokens(usage: &Value) -> TokenTotals {
    let field = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    TokenTotals {
        input: field("input_tokens"),
        cache_read: field("cache_read_input_tokens"),
        cache_write: field("cache_creation_input_tokens"),
        output: field("output_tokens"),
        reasoning: usage
            .get("output_tokens_details")
            .and_then(|details| details.get("thinking_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
    }
}

/// Codex counts `input_tokens` *including* cached ones; the shared shape keeps
/// them apart so the two providers add up the same way.
fn codex_tokens(usage: &Value) -> TokenTotals {
    let field = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let cached = field("cached_input_tokens");
    TokenTotals {
        input: field("input_tokens").saturating_sub(cached),
        cache_read: cached,
        cache_write: field("cache_write_input_tokens"),
        output: field("output_tokens"),
        reasoning: field("reasoning_output_tokens"),
    }
}

/// Text of a human-typed Claude prompt: a plain string, or the text blocks of
/// a multimodal message (images are dropped).
fn claude_prompt_text(content: &Value) -> Option<String> {
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => {
            if blocks
                .iter()
                .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
            {
                return None;
            }
            blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| unwrap_claude_command(text))
}

/// A slash command is stored as `<command-name>/x</command-name>` plus
/// message and argument tags; show it the way the user typed it.
fn unwrap_claude_command(text: &str) -> String {
    let tag = |name: &str| -> Option<&str> {
        let open = format!("<{name}>");
        let close = format!("</{name}>");
        let start = text.find(&open)? + open.len();
        let end = text[start..].find(&close)? + start;
        Some(text[start..end].trim())
    };
    let Some(command) = tag("command-name") else {
        return text.to_string();
    };
    match tag("command-args").filter(|args| !args.is_empty()) {
        Some(args) => format!("{command} {args}"),
        None => command.to_string(),
    }
}

/// Harness-injected text that arrives as a user record without being typed:
/// command-output echoes and system reminders.
fn is_claude_synthetic_prompt(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with("<local-command-")
        || text.starts_with("<system-reminder>")
        || text.starts_with("<task-notification>")
        || text.starts_with(
            "Caveat: The messages below were generated by the user while running local commands",
        )
}

struct ClaudeFilters {
    tool_result: Regex,
    tool_error: Regex,
    relevant: RegexSet,
}

fn claude_filters() -> &'static ClaudeFilters {
    static FILTERS: OnceLock<ClaudeFilters> = OnceLock::new();
    FILTERS.get_or_init(|| ClaudeFilters {
        tool_result: Regex::new(r#""type":\s*"tool_result""#).expect("static regex"),
        tool_error: Regex::new(r#""is_error":\s*true"#).expect("static regex"),
        relevant: RegexSet::new([
            r#""type":\s*"assistant""#,
            r#""type":\s*"user""#,
            r#""type":\s*"system""#,
            r#""type":\s*"cost-state""#,
        ])
        .expect("static regex set"),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexLine {
    SessionMeta,
    TurnContext,
    TokenCount,
    TaskStarted,
    TaskComplete,
    TurnAborted,
    EventUser,
    GoalUpdated,
    ResponseUser,
    ToolCall,
    Compacted,
}

struct CodexFilters {
    set: RegexSet,
}

const CODEX_KINDS: [(&str, CodexLine); 12] = [
    (r#""type":\s*"session_meta""#, CodexLine::SessionMeta),
    (r#""type":\s*"turn_context""#, CodexLine::TurnContext),
    (r#""type":\s*"compacted""#, CodexLine::Compacted),
    (
        r#""type":\s*"event_msg",\s*"payload":\s*\{\s*"type":\s*"token_count""#,
        CodexLine::TokenCount,
    ),
    (
        r#""type":\s*"event_msg",\s*"payload":\s*\{\s*"type":\s*"task_started""#,
        CodexLine::TaskStarted,
    ),
    (
        r#""type":\s*"event_msg",\s*"payload":\s*\{\s*"type":\s*"task_complete""#,
        CodexLine::TaskComplete,
    ),
    (
        r#""type":\s*"event_msg",\s*"payload":\s*\{\s*"type":\s*"turn_aborted""#,
        CodexLine::TurnAborted,
    ),
    (
        r#""type":\s*"event_msg",\s*"payload":\s*\{\s*"type":\s*"user_message""#,
        CodexLine::EventUser,
    ),
    (
        r#""type":\s*"event_msg",\s*"payload":\s*\{\s*"type":\s*"thread_goal_updated""#,
        CodexLine::GoalUpdated,
    ),
    (
        r#""type":\s*"response_item",\s*"payload":\s*\{\s*"type":\s*"message",\s*"role":\s*"user""#,
        CodexLine::ResponseUser,
    ),
    (
        r#""type":\s*"response_item",\s*"payload":\s*\{\s*"type":\s*"function_call""#,
        CodexLine::ToolCall,
    ),
    (
        r#""type":\s*"response_item",\s*"payload":\s*\{\s*"type":\s*"custom_tool_call""#,
        CodexLine::ToolCall,
    ),
];

fn codex_filters() -> &'static CodexFilters {
    static FILTERS: OnceLock<CodexFilters> = OnceLock::new();
    FILTERS.get_or_init(|| CodexFilters {
        set: RegexSet::new(CODEX_KINDS.iter().map(|(pattern, _)| *pattern))
            .expect("static regex set"),
    })
}

impl CodexFilters {
    /// Identifies a Codex record from the leading bytes of its line, so the
    /// bulky body of an irrelevant record (reasoning, tool output) is never
    /// parsed. `session_meta` and `turn_context` are matched by their own
    /// top-level type, which appears before the payload.
    fn classify(&self, prefix: &[u8]) -> Option<CodexLine> {
        self.set
            .matches(prefix)
            .iter()
            .next()
            .map(|index| CODEX_KINDS[index].1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(class: AgentClass, lines: &[&str]) -> SessionStats {
        let mut accumulator = StatsAccumulator::new(class);
        for line in lines {
            accumulator.feed_line(format!("{line}\n").as_bytes());
        }
        accumulator.snapshot()
    }

    fn claude_assistant(id: &str, output: u64, timestamp: &str, tool: Option<&str>) -> String {
        let content = tool.map_or_else(
            || r#"[{"type":"text","text":"ok"}]"#.to_string(),
            |name| format!(r#"[{{"type":"tool_use","id":"toolu_{id}","name":"{name}"}}]"#),
        );
        format!(
            r#"{{"isSidechain":false,"message":{{"model":"claude-sonnet-5","id":"{id}","type":"message","role":"assistant","content":{content},"usage":{{"input_tokens":3,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":{output},"output_tokens_details":{{"thinking_tokens":5}}}}}},"type":"assistant","timestamp":"{timestamp}","effort":"high","version":"2.1.0","gitBranch":"main"}}"#
        )
    }

    #[test]
    fn claude_merges_repeated_blocks_of_one_response_instead_of_summing() {
        let first = claude_assistant("msg_1", 40, "2026-09-25T10:00:00.000Z", Some("Bash"));
        let repeat = claude_assistant("msg_1", 40, "2026-09-25T10:00:01.000Z", Some("Bash"));
        let second = claude_assistant("msg_2", 60, "2026-09-25T10:00:30.000Z", None);
        let stats = feed(AgentClass::Claude, &[&first, &repeat, &second]);

        assert_eq!(stats.tokens.output, 100);
        assert_eq!(stats.tokens.input, 6);
        assert_eq!(stats.tokens.cache_read, 2000);
        assert_eq!(stats.tokens.cache_write, 200);
        assert_eq!(stats.tokens.reasoning, 10);
        assert_eq!(stats.model_calls, 2);
        assert_eq!(stats.tool_calls, 1, "the repeated tool_use id counts once");
        assert_eq!(stats.tools, vec![("Bash".to_string(), 1)]);
        assert_eq!(stats.model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(stats.effort.as_deref(), Some("high"));
        assert_eq!(stats.git_branch.as_deref(), Some("main"));
        assert_eq!(stats.context_tokens, Some(1103));
        assert_eq!(stats.span_ms(), Some(30_000));
    }

    #[test]
    fn claude_counts_only_human_prompts_and_skips_harness_records() {
        let human = r#"{"type":"user","timestamp":"2026-09-25T10:00:00.000Z","origin":{"kind":"human"},"message":{"content":"fix the login bug"}}"#;
        let notification = r#"{"type":"user","timestamp":"2026-09-25T10:01:00.000Z","origin":{"kind":"task-notification"},"message":{"content":"job done"}}"#;
        let meta = r#"{"type":"user","isMeta":true,"message":{"content":"injected"}}"#;
        let sidechain = r#"{"type":"user","isSidechain":true,"message":{"content":"subagent"}}"#;
        let echo = r#"{"type":"user","message":{"content":"<local-command-stdout>hi</local-command-stdout>"}}"#;
        let result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":"x","is_error":true}]}}"#;
        let stats = feed(
            AgentClass::Claude,
            &[human, notification, meta, sidechain, echo, result],
        );

        assert_eq!(stats.prompt_count, 1);
        assert_eq!(stats.recent_prompts.len(), 1);
        assert_eq!(stats.recent_prompts[0].text, "fix the login bug");
        assert_eq!(stats.tool_errors, Some(1));
    }

    #[test]
    fn claude_records_turn_durations_and_cost_snapshot() {
        let turn = r#"{"type":"system","subtype":"turn_duration","durationMs":32500,"timestamp":"2026-09-25T10:00:32.000Z"}"#;
        let turn_two = r#"{"type":"system","subtype":"turn_duration","durationMs":1500,"timestamp":"2026-09-25T10:01:32.000Z"}"#;
        let cost = r#"{"type":"cost-state","totalCostUSD":1.25,"totalAPIDuration":9000,"totalToolDuration":3000,"totalLinesAdded":10,"totalLinesRemoved":2,"hasUnknownModelCost":false,"modelUsage":{"claude-sonnet-5":{"costUSD":1.0},"claude-haiku-4-5":{"costUSD":0.25}}}"#;
        let stats = feed(AgentClass::Claude, &[turn, turn_two, cost]);

        assert_eq!(stats.turns_completed, 2);
        assert_eq!(stats.active_ms, 34_000);
        assert_eq!(stats.longest_turn_ms, 32_500);
        let cost = stats.cost.expect("cost snapshot is retained");
        assert!((cost.total_usd - 1.25).abs() < 1e-9);
        assert_eq!(cost.model_costs[0].0, "claude-sonnet-5");
        assert_eq!(cost.lines_added, 10);
    }

    #[test]
    fn claude_synthetic_model_counts_an_api_error_without_tokens() {
        let synthetic = r#"{"type":"assistant","message":{"model":"<synthetic>","id":"m","content":[{"type":"text","text":"API Error"}],"usage":{"input_tokens":0,"output_tokens":0}}}"#;
        let stats = feed(AgentClass::Claude, &[synthetic]);
        assert_eq!(stats.api_errors, 1);
        assert_eq!(stats.tokens.total(), 0);
    }

    #[test]
    fn claude_subagent_tokens_count_toward_spend_but_not_context() {
        let main = claude_assistant("msg_a", 10, "2026-09-25T10:00:00.000Z", None);
        let side = claude_assistant("msg_b", 500, "2026-09-25T10:00:05.000Z", None)
            .replace(r#""isSidechain":false"#, r#""isSidechain":true"#);
        let stats = feed(AgentClass::Claude, &[&main, &side]);
        assert_eq!(stats.tokens.output, 510);
        assert_eq!(stats.model_calls, 1);
        assert_eq!(stats.samples.len(), 1);
    }

    #[test]
    fn tool_result_lines_are_never_parsed_but_failures_are_counted() {
        let mut accumulator = StatsAccumulator::new(AgentClass::Claude);
        let huge = format!(
            r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","content":"{}","is_error":true}}]}}}}"#,
            "x".repeat(100_000)
        );
        accumulator.feed_line(format!("{huge}\n").as_bytes());
        let stats = accumulator.snapshot();
        assert_eq!(stats.tool_errors, Some(1));
        assert_eq!(stats.records_parsed, 0);
    }

    fn codex_token_count(total: (u64, u64, u64, u64), last_input: u64, at: &str) -> String {
        format!(
            r#"{{"timestamp":"{at}","ordinal":1,"type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{},"cached_input_tokens":{},"cache_write_input_tokens":0,"output_tokens":{},"reasoning_output_tokens":{}}},"last_token_usage":{{"input_tokens":{last_input},"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0}},"model_context_window":258400}},"rate_limits":{{"plan_type":"pro","primary":{{"used_percent":74.0,"window_minutes":10080,"resets_at":1790054687}},"secondary":null}}}}}}"#,
            total.0, total.1, total.2, total.3
        )
    }

    #[test]
    fn codex_uses_cumulative_totals_and_ignores_repeated_events() {
        let meta = r#"{"timestamp":"2026-09-20T02:57:13.775Z","ordinal":0,"type":"session_meta","payload":{"id":"x","timestamp":"2026-09-20T02:41:21.248Z","cwd":"/work","cli_version":"0.155.1","model_provider":"openai"}}"#;
        let context = r#"{"timestamp":"2026-09-20T02:57:14.413Z","ordinal":9,"type":"turn_context","payload":{"model":"gpt-5.6-terra","effort":"medium"}}"#;
        let first = codex_token_count((1000, 400, 50, 20), 1000, "2026-09-20T02:57:20.209Z");
        let repeat = codex_token_count((1000, 400, 50, 20), 1000, "2026-09-20T02:57:21.209Z");
        let second = codex_token_count((3000, 1400, 150, 60), 2000, "2026-09-20T02:58:20.209Z");
        let stats = feed(
            AgentClass::Codex,
            &[meta, context, &first, &repeat, &second],
        );

        assert_eq!(stats.tokens.input, 1600, "uncached input = input - cached");
        assert_eq!(stats.tokens.cache_read, 1400);
        assert_eq!(stats.tokens.output, 150);
        assert_eq!(stats.tokens.reasoning, 60);
        assert_eq!(stats.model_calls, 2);
        assert_eq!(stats.context_tokens, Some(2000));
        assert_eq!(stats.context_window, Some(258_400));
        assert_eq!(stats.model.as_deref(), Some("gpt-5.6-terra"));
        assert_eq!(stats.effort.as_deref(), Some("medium"));
        assert_eq!(stats.plan_type.as_deref(), Some("pro"));
        assert_eq!(stats.provider.as_deref(), Some("OpenAI"));
        assert_eq!(stats.rate_limits.len(), 1);
        assert!((stats.rate_limits[0].1.used_percent - 74.0).abs() < 1e-9);
        assert_eq!(
            stats.first_at_ms,
            parse_timestamp_ms("2026-09-20T02:41:21.248Z")
        );
        assert_eq!(stats.models[0].calls, 2);
    }

    #[test]
    fn codex_tracks_tools_turns_and_prefers_response_item_prompts() {
        let started = r#"{"timestamp":"2026-09-20T03:00:00.000Z","type":"event_msg","payload":{"type":"task_started","model_context_window":100}}"#;
        let event_user = r#"{"timestamp":"2026-09-20T03:00:01.000Z","type":"event_msg","payload":{"type":"user_message","message":"event copy"}}"#;
        let response_user = r#"{"timestamp":"2026-09-20T03:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"real prompt"}]}}"#;
        let envelope = r#"{"timestamp":"2026-09-20T03:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"#;
        let call = r#"{"timestamp":"2026-09-20T03:00:02.000Z","type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{}"}}"#;
        let custom = r#"{"timestamp":"2026-09-20T03:00:03.000Z","type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":"x"}}"#;
        let output = r#"{"timestamp":"2026-09-20T03:00:04.000Z","type":"response_item","payload":{"type":"custom_tool_call_output","output":"x"}}"#;
        let complete = r#"{"timestamp":"2026-09-20T03:03:00.000Z","type":"event_msg","payload":{"type":"task_complete","duration_ms":192373,"time_to_first_token_ms":5008}}"#;
        let compacted =
            r#"{"timestamp":"2026-09-20T03:04:00.000Z","type":"compacted","payload":{}}"#;
        let stats = feed(
            AgentClass::Codex,
            &[
                started,
                event_user,
                response_user,
                envelope,
                call,
                custom,
                output,
                complete,
                compacted,
            ],
        );

        assert_eq!(stats.prompt_count, 1);
        assert_eq!(stats.recent_prompts[0].text, "real prompt");
        assert_eq!(stats.tool_calls, 2, "outputs are not calls");
        assert_eq!(stats.turns_completed, 1);
        assert_eq!(stats.active_ms, 192_373);
        assert_eq!(stats.average_first_token_ms, Some(5008));
        assert_eq!(stats.compactions, 1);
        assert_eq!(stats.tool_errors, None);
    }

    #[test]
    fn codex_falls_back_to_event_prompts_when_no_response_prompts_exist() {
        let event_user = r#"{"timestamp":"2026-09-20T03:00:01.000Z","type":"event_msg","payload":{"type":"user_message","message":"only event"}}"#;
        let stats = feed(AgentClass::Codex, &[event_user]);
        assert_eq!(stats.recent_prompts[0].text, "only event");
    }

    #[test]
    fn identical_consecutive_prompts_collapse_into_one_record_with_a_repeat_count() {
        let user = |at: &str, text: &str| {
            format!(
                r#"{{"timestamp":"{at}","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"{text}"}}]}}}}"#
            )
        };
        let first = user("2026-09-20T03:00:01.000Z", "keep going");
        let again = user("2026-09-20T03:05:01.000Z", "keep going");
        let other = user("2026-09-20T03:06:01.000Z", "stop");
        let stats = feed(AgentClass::Codex, &[&first, &again, &other]);
        assert_eq!(stats.recent_prompts.len(), 2);
        assert_eq!(stats.recent_prompts[0].repeats, 2);
        assert_eq!(stats.recent_prompts[1].repeats, 1);
        assert_eq!(stats.prompt_count, 3, "every send still counts as a prompt");
    }

    #[test]
    fn claude_slash_commands_read_as_typed() {
        let command = r#"{"type":"user","message":{"content":"<command-name>/goal</command-name>\n <command-message>goal</command-message>\n <command-args>ship it</command-args>"}}"#;
        let bare =
            r#"{"type":"user","message":{"content":"<command-name>/compact</command-name>"}}"#;
        let stats = feed(AgentClass::Claude, &[command, bare]);
        assert_eq!(stats.recent_prompts[0].text, "/goal ship it");
        assert_eq!(stats.recent_prompts[1].text, "/compact");
    }

    #[test]
    fn malformed_and_irrelevant_lines_are_ignored() {
        let stats = feed(
            AgentClass::Claude,
            &[
                "{not json",
                r#"{"type":"user","message":"#,
                "",
                r#"{"type":"queue-operation"}"#,
            ],
        );
        assert_eq!(stats.prompt_count, 0);
    }

    #[test]
    fn timeline_buckets_fold_samples_and_activity() {
        let stats = SessionStats {
            first_at_ms: Some(0),
            last_at_ms: Some(1000),
            samples: vec![
                TokenSample {
                    at_ms: 0,
                    tokens: TokenTotals {
                        output: 10,
                        ..TokenTotals::default()
                    },
                    context: Some(5),
                },
                TokenSample {
                    at_ms: 1000,
                    tokens: TokenTotals {
                        output: 30,
                        ..TokenTotals::default()
                    },
                    context: Some(9),
                },
            ],
            activity_ms: vec![0, 500, 1000],
            ..SessionStats::default()
        };
        let buckets = stats.timeline(4);
        assert_eq!(buckets.len(), 4);
        assert_eq!(buckets[0].tokens.output, 10);
        assert_eq!(buckets[3].tokens.output, 30);
        assert_eq!(buckets[3].peak_context, 9);
        assert_eq!(buckets.iter().map(|b| b.events).sum::<u32>(), 3);
    }

    #[test]
    fn ingest_file_resumes_from_its_offset_and_skips_a_partial_last_line() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.jsonl");
        let first = claude_assistant("msg_1", 40, "2026-09-25T10:00:00.000Z", None);
        let second = claude_assistant("msg_2", 60, "2026-09-25T10:00:30.000Z", None);
        std::fs::write(&path, format!("{first}\n{}", &second[..20])).unwrap();

        let mut accumulator = StatsAccumulator::new(AgentClass::Claude);
        accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
        assert_eq!(accumulator.snapshot().tokens.output, 40);

        std::fs::write(&path, format!("{first}\n{second}\n")).unwrap();
        accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
        assert_eq!(accumulator.snapshot().tokens.output, 100);
        assert_eq!(accumulator.snapshot().model_calls, 2);
    }

    #[test]
    fn a_truncated_file_restarts_the_accumulator() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.jsonl");
        let first = claude_assistant("msg_1", 40, "2026-09-25T10:00:00.000Z", None);
        std::fs::write(&path, format!("{first}\n{first}x\n")).unwrap();
        let mut accumulator = StatsAccumulator::new(AgentClass::Claude);
        accumulator.ingest_file(&path, |_, _, _| {}).unwrap();

        let replacement = claude_assistant("msg_9", 7, "2026-09-25T11:00:00.000Z", None);
        std::fs::write(&path, format!("{replacement}\n")).unwrap();
        accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
        assert_eq!(accumulator.snapshot().tokens.output, 7);
    }

    #[test]
    fn decimation_preserves_totals_and_peaks() {
        let samples: Vec<TokenSample> = (0..(SAMPLE_CAP as i64 * 3 + 1))
            .map(|index| TokenSample {
                at_ms: index,
                tokens: TokenTotals {
                    output: 2,
                    ..TokenTotals::default()
                },
                context: Some(index as u64),
            })
            .collect();
        let expected_total: u64 = samples.iter().map(|s| s.tokens.output).sum();
        let peak = samples.last().unwrap().context;
        let merged = decimate(samples);
        assert!(merged.len() <= SAMPLE_CAP);
        assert_eq!(
            merged.iter().map(|s| s.tokens.output).sum::<u64>(),
            expected_total
        );
        assert_eq!(merged.iter().filter_map(|s| s.context).max(), peak);
    }

    /// Manual check against a real transcript:
    /// `ILIUM_STATS_SAMPLE=/path/to/file.jsonl ILIUM_STATS_CLASS=codex cargo test
    /// -p ilium-client --lib real_transcript_report -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads a real transcript named by ILIUM_STATS_SAMPLE"]
    fn real_transcript_report() {
        let path = std::env::var("ILIUM_STATS_SAMPLE").expect("ILIUM_STATS_SAMPLE");
        let class = match std::env::var("ILIUM_STATS_CLASS").as_deref() {
            Ok("codex") => AgentClass::Codex,
            _ => AgentClass::Claude,
        };
        let mut accumulator = StatsAccumulator::new(class);
        let started = std::time::Instant::now();
        accumulator
            .ingest_file(std::path::Path::new(&path), |_, _, _| {})
            .unwrap();
        let mut stats = accumulator.snapshot();
        let elapsed = started.elapsed();
        stats.samples.truncate(2);
        stats.activity_ms.truncate(2);
        stats.turn_durations_ms.truncate(3);
        println!("elapsed {elapsed:?}\n{stats:#?}");
    }

    #[test]
    fn cache_hit_ratio_is_none_without_prompt_tokens() {
        assert_eq!(TokenTotals::default().cache_hit_ratio(), None);
        let totals = TokenTotals {
            input: 10,
            cache_read: 90,
            ..TokenTotals::default()
        };
        assert_eq!(totals.cache_hit_ratio(), Some(0.9));
    }
}
