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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

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
/// Upper bound on stored work-state marks; halved when exceeded.
const WORK_MARK_CAP: usize = 20_000;
/// Upper bound on stored progress-bar runs (most recent kept).
const PROGRESS_SPAN_CAP: usize = 256;
/// Upper bound on stored turn durations (most recent kept).
const TURN_DURATION_CAP: usize = 400;
/// Bytes of a Codex line inspected by the pre-filter. Every record's kind
/// keys sit at the start of the line, ahead of any bulky payload body.
const CODEX_PREFIX_BYTES: usize = 640;

/// Token counts normalised across providers so the two never disagree on what
/// a column means. `input` is uncached prompt input, `cache_read` and
/// `cache_write` are prompt-cache traffic, `output` includes `reasoning`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub calls: u32,
    pub tokens: TokenTotals,
}

/// One user prompt as recorded in the transcript.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TokenSample {
    pub at_ms: i64,
    pub tokens: TokenTotals,
    pub context: Option<u64>,
}

/// What the agent was doing between one transcript event and the next.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum WorkKind {
    /// No turn running: waiting for the user.
    Idle,
    /// The model is generating (thinking or writing text).
    Model,
    /// A shell command is running.
    Shell,
    /// A file is being edited or written.
    Edit,
    /// Files are being read or searched.
    Read,
    /// A sub-agent is working.
    Agent,
    /// A web fetch or search is running.
    Web,
    /// Any other tool (MCP, planning, ...).
    Other,
}

impl WorkKind {
    pub const ALL: [WorkKind; 8] = [
        Self::Idle,
        Self::Model,
        Self::Shell,
        Self::Edit,
        Self::Read,
        Self::Agent,
        Self::Web,
        Self::Other,
    ];

    /// Maps a tool name from either provider's transcript to what it does.
    pub fn for_tool(name: &str) -> Self {
        let name = name.to_ascii_lowercase();
        match name.as_str() {
            "bash" | "shell" | "exec_command" | "local_shell" | "shell_command" | "write_stdin"
            | "bashoutput" | "killshell" => Self::Shell,
            "edit" | "write" | "multiedit" | "notebookedit" | "apply_patch" => Self::Edit,
            "read" | "grep" | "glob" | "ls" | "view_image" => Self::Read,
            "task" | "agent" | "spawn_agent" | "wait_agent" | "send_input" => Self::Agent,
            "webfetch" | "websearch" | "web_search" => Self::Web,
            _ => Self::Other,
        }
    }
}

/// The agent switched to `kind` at `at_ms` and stayed there until the next mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkMark {
    pub at_ms: i64,
    pub kind: WorkKind,
}

/// How an Ilium progress-bar (progress monitor) run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProgressOutcome {
    /// Still being watched.
    Running,
    Success,
    Failure,
    /// The monitor stopped before the task reported a terminal status.
    Unknown,
    /// The agent cleared the monitor itself.
    Cleared,
}

/// One progress-bar run: from the `ilium progress set` call to its outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProgressSpan {
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub outcome: ProgressOutcome,
}

/// Reads the delivery text Ilium types into an agent when a progress monitor
/// ends; returns the monitor number and how it ended.
pub(crate) fn parse_progress_outcome(text: &str) -> Option<(u64, ProgressOutcome)> {
    let rest = text.trim_start().strip_prefix("Ilium progress monitor ")?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let monitor_id: u64 = rest[..digits].parse().ok()?;
    let first_line = rest[digits..].lines().next().unwrap_or("");
    let outcome = if first_line.contains(" completed successfully") {
        ProgressOutcome::Success
    } else if first_line.contains(" failed") {
        ProgressOutcome::Failure
    } else if first_line.contains("stopped before")
        || first_line.contains("could no longer observe")
    {
        ProgressOutcome::Unknown
    } else {
        return None;
    };
    Some((monitor_id, outcome))
}

/// A rate-limit window as reported by the provider (Codex).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RateLimitWindow {
    pub used_percent: f64,
    pub window_minutes: Option<u64>,
    pub resets_at_unix: Option<i64>,
}

/// Cost figures Claude Code recorded itself. The CLI writes these snapshots
/// periodically rather than per call, so for a live pane the figure can trail
/// the token counters; the popover labels it accordingly.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

/// Tokens one model consumed during one minute. The dollar cost is derived
/// later from a price table, so a changed price override reprices history
/// without re-reading the transcript.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpendBucket {
    /// Start of the minute, in Unix milliseconds.
    pub minute_ms: i64,
    pub model: Arc<str>,
    pub tokens: TokenTotals,
}

/// Percentage points of one plan-quota window that were used up during one
/// minute while this session ran. Codex reports the account-wide `used_percent`
/// of each window; the rise between two readings is what this session saw
/// being consumed, so concurrent agents on the same account count together.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuotaBucket {
    /// Start of the minute, in Unix milliseconds.
    pub minute_ms: i64,
    /// Window name as Codex reports it (`primary` or `secondary`).
    pub window: Arc<str>,
    pub percent_points: f64,
}

const MINUTE_MS: i64 = 60_000;

/// Allocation custody travels with immutable statistics, including exported
/// Arc snapshots. Equality compares domain data rather than quota identity.
#[derive(Clone, Debug, Default)]
pub(crate) struct StatsRetention {
    _hold: Option<std::sync::Arc<ilium_execution::StorageAdmission>>,
}
impl StatsRetention {
    pub(crate) fn new(hold: std::sync::Arc<ilium_execution::StorageAdmission>) -> Self {
        Self { _hold: Some(hold) }
    }
}
impl PartialEq for StatsRetention {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// Everything the popover shows about one session.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
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
    /// Timestamp of the transcript event that carried these account readings.
    #[serde(default)]
    pub rate_limits_at_ms: Option<i64>,

    pub samples: Vec<TokenSample>,
    /// Per-minute, per-model token spend including sub-agent calls; the
    /// source of the cost sparkline and burn rate.
    pub spend: Vec<SpendBucket>,
    /// Per-minute rise of each plan-quota window (Codex only).
    pub quota_spend: Vec<QuotaBucket>,
    pub activity_ms: Vec<i64>,
    /// Chronological state changes of the agent, for the work ribbon.
    pub work_marks: Vec<WorkMark>,
    /// Progress-bar runs, oldest first.
    pub progress_spans: Vec<ProgressSpan>,

    pub bytes_read: u64,
    pub records_parsed: u64,
    /// Records over the line limit that were read in bounded form; their
    /// middle was skipped (usage fields are salvaged when present).
    #[serde(default)]
    pub oversized_records: u32,
    #[serde(default)]
    pub oversized_bytes: u64,
    #[serde(skip)]
    pub(crate) retention: StatsRetention,
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
    pub(crate) fn retained_bytes(&self) -> usize {
        let metadata = [
            &self.provider,
            &self.model,
            &self.effort,
            &self.advisor_model,
            &self.cli_version,
            &self.git_branch,
            &self.cwd,
            &self.plan_type,
        ];
        metadata
            .iter()
            .filter_map(|value| value.as_ref())
            .map(|value| value.capacity())
            .sum::<usize>()
            + self.models.capacity() * std::mem::size_of::<ModelUsage>()
            + self
                .models
                .iter()
                .map(|value| value.model.capacity())
                .sum::<usize>()
            + self.tools.capacity() * std::mem::size_of::<(String, u32)>()
            + self
                .tools
                .iter()
                .map(|(name, _)| name.capacity())
                .sum::<usize>()
            + self.recent_prompts.capacity() * std::mem::size_of::<PromptRecord>()
            + self
                .recent_prompts
                .iter()
                .map(|prompt| prompt.text.capacity())
                .sum::<usize>()
            + self.turn_durations_ms.capacity() * 8
            + self.samples.capacity() * std::mem::size_of::<TokenSample>()
            + self.activity_ms.capacity() * 8
            + self.work_marks.capacity() * std::mem::size_of::<WorkMark>()
            + self.progress_spans.capacity() * std::mem::size_of::<ProgressSpan>()
            + self.spend.capacity() * std::mem::size_of::<SpendBucket>()
            + self
                .spend
                .iter()
                .map(|bucket| bucket.model.len())
                .sum::<usize>()
            + self.quota_spend.capacity() * std::mem::size_of::<QuotaBucket>()
            + self
                .quota_spend
                .iter()
                .map(|bucket| bucket.window.len())
                .sum::<usize>()
            + self.rate_limits.capacity() * std::mem::size_of::<(String, RateLimitWindow)>()
            + self
                .rate_limits
                .iter()
                .map(|(name, _)| name.capacity())
                .sum::<usize>()
            + self.cost.as_ref().map_or(0, |cost| {
                cost.model_costs.capacity() * std::mem::size_of::<(String, f64)>()
                    + cost
                        .model_costs
                        .iter()
                        .map(|(name, _)| name.capacity())
                        .sum::<usize>()
            })
    }
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
        self.timeline_between(count, first, last)
    }

    /// Like [`Self::timeline`] but over `first..=last`, ignoring events outside.
    pub fn timeline_between(&self, count: usize, first: i64, last: i64) -> Vec<TimelineBucket> {
        if count == 0 || last < first {
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
        for sample in self
            .samples
            .iter()
            .filter(|sample| (first..=last).contains(&sample.at_ms))
        {
            let bucket = &mut buckets[index_for(sample.at_ms)];
            bucket.tokens.add(&sample.tokens);
            bucket.calls += 1;
            bucket.peak_context = bucket.peak_context.max(sample.context.unwrap_or(0));
        }
        for at_ms in self
            .activity_ms
            .iter()
            .filter(|at_ms| (first..=last).contains(at_ms))
        {
            buckets[index_for(*at_ms)].events += 1;
        }
        buckets
    }
}

/// A Claude model call, merged across the several transcript records the CLI
/// writes for one response (one per content block, all carrying full usage).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ClaudeCall {
    at_ms: Option<i64>,
    model: String,
    tokens: TokenTotals,
    sidechain: bool,
    /// Calls the advisor tool made to a stronger model during this response.
    /// The CLI reports them only inside `usage.iterations` (type
    /// `advisor_message`), never in the top-level token counts, yet they are
    /// billed at the advisor model's own price.
    #[serde(default)]
    advisors: Vec<(String, TokenTotals)>,
}

/// Most recent sub-agent calls kept unfolded, enough to merge the adjacent
/// records of one response without retaining a call per transcript record.
const SIDECHAIN_WINDOW_CALLS: usize = 1_024;
const SIDECHAIN_WINDOW_TOOL_IDS: usize = 4_096;

/// Bounded memory of recent sub-agent calls and tool ids (see
/// [`StatsAccumulator::sidechain_window`]).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct SidechainWindow {
    calls: std::collections::VecDeque<ClaudeCall>,
    /// Message id -> sequence number of the call; `evicted` calls precede the
    /// front of `calls`, so the position is `sequence - evicted`.
    call_sequence: HashMap<String, u64>,
    call_ids: std::collections::VecDeque<String>,
    evicted: u64,
    tool_ids: std::collections::VecDeque<String>,
    tool_id_set: HashSet<String>,
}

impl SidechainWindow {
    fn calls(&self) -> impl Iterator<Item = &ClaudeCall> {
        self.calls.iter()
    }

    /// Merges a repeated record of a windowed response, or stores a new call
    /// and returns the oldest one when the window overflowed.
    fn merge_or_push(&mut self, id: &str, call: ClaudeCall) -> Option<ClaudeCall> {
        if let Some(&sequence) = self.call_sequence.get(id).filter(|_| !id.is_empty()) {
            let index = (sequence - self.evicted) as usize;
            let existing = &mut self.calls[index];
            existing.tokens.max_merge(&call.tokens);
            for (position, (advisor_model, advisor_tokens)) in call.advisors.into_iter().enumerate()
            {
                match existing.advisors.get_mut(position) {
                    Some((_, held)) => held.max_merge(&advisor_tokens),
                    None => existing.advisors.push((advisor_model, advisor_tokens)),
                }
            }
            return None;
        }
        let sequence = self.evicted + self.calls.len() as u64;
        self.calls.push_back(call);
        self.call_ids.push_back(id.to_string());
        if !id.is_empty() {
            self.call_sequence.insert(id.to_string(), sequence);
        }
        if self.calls.len() <= SIDECHAIN_WINDOW_CALLS {
            return None;
        }
        let evicted = self.calls.pop_front();
        if let Some(old_id) = self.call_ids.pop_front() {
            self.call_sequence.remove(&old_id);
        }
        self.evicted += 1;
        evicted
    }

    /// True the first time a tool id is seen inside the recent window.
    fn note_tool_id(&mut self, id: &str) -> bool {
        if id.is_empty() {
            return true;
        }
        if !self.tool_id_set.insert(id.to_string()) {
            return false;
        }
        self.tool_ids.push_back(id.to_string());
        if self.tool_ids.len() > SIDECHAIN_WINDOW_TOOL_IDS {
            if let Some(old) = self.tool_ids.pop_front() {
                self.tool_id_set.remove(&old);
            }
        }
        true
    }

    fn retained_bytes(&self) -> usize {
        self.calls.len() * (std::mem::size_of::<ClaudeCall>() + 256)
            + (self.call_ids.len() + self.tool_ids.len()) * 160
    }
}

/// Sub-agent calls that left the window, kept only as totals.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct FoldedSpend {
    tokens: TokenTotals,
    models: HashMap<String, (u32, TokenTotals)>,
    #[serde(with = "ordered_pairs")]
    spend: BTreeMap<(i64, String), TokenTotals>,
}

impl FoldedSpend {
    fn fold(&mut self, call: &ClaudeCall) {
        let mut record = |model: &str, tokens: &TokenTotals| {
            self.tokens.add(tokens);
            let usage = self.models.entry(model.to_string()).or_default();
            usage.0 += 1;
            usage.1.add(tokens);
            if let Some(at_ms) = call.at_ms {
                let minute_ms = at_ms - at_ms.rem_euclid(MINUTE_MS);
                self.spend
                    .entry((minute_ms, model.to_string()))
                    .or_default()
                    .add(tokens);
            }
        };
        record(&call.model, &call.tokens);
        for (advisor_model, advisor_tokens) in &call.advisors {
            record(advisor_model, advisor_tokens);
        }
    }

    fn retained_bytes(&self) -> usize {
        self.models
            .keys()
            .map(|name| name.capacity() + 128)
            .sum::<usize>()
            + self
                .spend
                .keys()
                .map(|(_, name)| name.capacity() + 256)
                .sum::<usize>()
    }
}

// JSON object keys cannot represent the timestamp/model pairs used here.
mod ordered_pairs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S, V>(
        values: &BTreeMap<(i64, String), V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        V: Serialize,
    {
        values.iter().collect::<Vec<_>>().serialize(serializer)
    }

    pub fn deserialize<'de, D, V>(deserializer: D) -> Result<BTreeMap<(i64, String), V>, D::Error>
    where
        D: Deserializer<'de>,
        V: Deserialize<'de>,
    {
        Ok(Vec::<((i64, String), V)>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}

/// Incremental parser. Feed it a transcript file with [`Self::ingest_file`];
/// call [`Self::snapshot`] whenever a value for display is needed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Monitor numbers whose outcome was already recorded (Codex logs the
    /// same delivered message in two record shapes).
    seen_progress_outcomes: HashSet<u64>,

    claude_calls: Vec<ClaudeCall>,
    claude_call_index: HashMap<String, usize>,
    /// Sub-agent calls. A sub-agent transcript can hold tens of thousands of
    /// calls, so they are never retained one by one: the most recent few stay
    /// here (a response is written as several adjacent records that must merge
    /// by message id) and older ones fold into the aggregates below.
    #[serde(default)]
    sidechain_window: SidechainWindow,
    /// Totals of every sub-agent call already folded out of the window.
    #[serde(default)]
    folded_sidechain: FoldedSpend,
    claude_tool_errors: u32,
    /// Bytes consumed so far from each sub-agent transcript that belongs to
    /// this session (see [`Self::ingest_extra_file`]).
    extra_offsets: HashMap<PathBuf, u64>,
    /// Per child rollout of a Codex session (spawned sub-agent threads): its
    /// own cumulative counter and current model, so deltas never mix threads.
    #[serde(default)]
    codex_children: HashMap<PathBuf, (TokenTotals, Option<String>)>,
    /// While a child rollout is fed, only token spend is recorded: its
    /// prompts, context and account readings are not this thread's.
    #[serde(skip)]
    codex_child_mode: bool,

    codex_last_total: TokenTotals,
    codex_model_usage: HashMap<String, (u32, TokenTotals)>,
    codex_totals: TokenTotals,
    codex_current_model: Option<String>,
    codex_rate_limits: Vec<(String, RateLimitWindow)>,
    #[serde(default)]
    codex_rate_limits_at_ms: Option<i64>,
    codex_samples: Vec<TokenSample>,
    #[serde(with = "ordered_pairs")]
    codex_spend: BTreeMap<(i64, String), TokenTotals>,
    /// Last reading per quota window: used percent and the reset instant that
    /// identifies which window period it belongs to.
    codex_quota_last: HashMap<String, (f64, Option<i64>)>,
    #[serde(with = "ordered_pairs")]
    codex_quota_spend: BTreeMap<(i64, String), f64>,
    // Last field: the state debit outlives destruction of every owned map/vector.
    #[serde(skip)]
    retention: StatsRetention,
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
            seen_progress_outcomes: HashSet::new(),
            claude_calls: Vec::new(),
            claude_call_index: HashMap::new(),
            sidechain_window: SidechainWindow::default(),
            folded_sidechain: FoldedSpend::default(),
            claude_tool_errors: 0,
            extra_offsets: HashMap::new(),
            codex_children: HashMap::new(),
            codex_child_mode: false,
            codex_last_total: TokenTotals::default(),
            codex_model_usage: HashMap::new(),
            codex_totals: TokenTotals::default(),
            codex_current_model: None,
            codex_rate_limits: Vec::new(),
            codex_rate_limits_at_ms: None,
            codex_samples: Vec::new(),
            codex_spend: BTreeMap::new(),
            codex_quota_last: HashMap::new(),
            codex_quota_spend: BTreeMap::new(),
            retention: StatsRetention::default(),
        }
    }

    pub(crate) fn attach_retention(&mut self, retention: StatsRetention) {
        self.retention = retention.clone();
        self.stats.retention = retention;
    }
    pub(crate) fn retained_bytes(&self) -> usize {
        let strings = self
            .tool_counts
            .keys()
            .chain(self.seen_tool_ids.iter())
            .chain(self.claude_call_index.keys())
            .chain(self.codex_model_usage.keys())
            .chain(self.codex_quota_last.keys());
        let string_bytes = strings.map(String::capacity).sum::<usize>();
        self.stats.retained_bytes()
            + self.prompts.capacity() * std::mem::size_of::<PromptRecord>()
            + self
                .prompts
                .iter()
                .map(|prompt| prompt.text.capacity())
                .sum::<usize>()
            + self.event_prompts.capacity() * std::mem::size_of::<PromptRecord>()
            + self
                .event_prompts
                .iter()
                .map(|prompt| prompt.text.capacity())
                .sum::<usize>()
            + self.turn_durations.capacity() * 8
            + self.codex_samples.capacity() * std::mem::size_of::<TokenSample>()
            + self
                .codex_current_model
                .as_ref()
                .map_or(0, String::capacity)
            + self.codex_rate_limits.capacity() * std::mem::size_of::<(String, RateLimitWindow)>()
            + self
                .codex_rate_limits
                .iter()
                .map(|(name, _)| name.capacity())
                .sum::<usize>()
            + self.cost_snapshot.as_ref().map_or(0, |cost| {
                cost.model_costs.capacity() * std::mem::size_of::<(String, f64)>()
                    + cost
                        .model_costs
                        .iter()
                        .map(|(name, _)| name.capacity())
                        .sum::<usize>()
            })
            + string_bytes
            + self.claude_calls.capacity() * std::mem::size_of::<ClaudeCall>()
            + self
                .claude_calls
                .iter()
                .map(|call| {
                    call.model.capacity()
                        + call.advisors.capacity() * std::mem::size_of::<(String, TokenTotals)>()
                        + call
                            .advisors
                            .iter()
                            .map(|(name, _)| name.capacity())
                            .sum::<usize>()
                })
                .sum::<usize>()
            + self.sidechain_window.retained_bytes()
            + self.folded_sidechain.retained_bytes()
            + self
                .codex_spend
                .keys()
                .map(|(_, name)| name.capacity() + 256)
                .sum::<usize>()
            + self
                .codex_quota_spend
                .keys()
                .map(|(_, name)| name.capacity() + 256)
                .sum::<usize>()
            + self
                .extra_offsets
                .keys()
                .chain(self.codex_children.keys())
                .map(|path| path.capacity() + 128)
                .sum::<usize>()
            + (self.tool_counts.capacity()
                + self.seen_tool_ids.capacity()
                + self.claude_call_index.capacity()
                + self.codex_model_usage.capacity()
                + self.codex_quota_last.capacity())
                * 128
    }
    pub(crate) fn check_bounds(&self) -> std::io::Result<()> {
        let retained = self.retained_bytes();
        const RETAINED_BYTE_LIMIT: usize = 8 * 1024 * 1024;
        if retained > RETAINED_BYTE_LIMIT {
            return Err(std::io::Error::other(format!(
                "Statistics retained byte limit reached: observed {retained}, limit {RETAINED_BYTE_LIMIT}, {} over; totals incomplete",
                retained.saturating_sub(RETAINED_BYTE_LIMIT)
            )));
        }
        Ok(())
    }
    /// Reads one line. A line over `MAX_LINE_BYTES` is never buffered whole:
    /// only its head and tail (`EDGE_BYTES` each) are kept, the middle is
    /// discarded as it streams past, and `line` receives a small salvage
    /// record (see `salvage_oversized`) so valid statistics survive. The
    /// returned length is the true number of bytes consumed from the file;
    /// the flag says whether the line was oversized.
    fn read_bounded_line(
        reader: &mut impl BufRead,
        line: &mut Vec<u8>,
        class: &AgentClass,
    ) -> std::io::Result<(usize, bool)> {
        const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
        const EDGE_BYTES: usize = 64 * 1024;
        line.clear();
        let mut total = 0_usize;
        let mut tail: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
        let mut oversized = false;
        let mut ended = false;
        loop {
            let buffer = reader.fill_buf()?;
            if buffer.is_empty() {
                break;
            }
            let count = buffer
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(buffer.len(), |index| index + 1);
            let chunk = &buffer[..count];
            if !oversized && total + count > MAX_LINE_BYTES {
                oversized = true;
                line.truncate(EDGE_BYTES);
            }
            if oversized {
                tail.extend(chunk.iter().copied());
                let excess = tail.len().saturating_sub(EDGE_BYTES);
                tail.drain(..excess);
            } else {
                line.extend_from_slice(chunk);
            }
            total += count;
            ended = chunk.last() == Some(&b'\n');
            reader.consume(count);
            if ended {
                break;
            }
        }
        if oversized {
            let tail: Vec<u8> = tail.into_iter().collect();
            let mut salvage = salvage_oversized(class, line, &tail);
            if ended {
                salvage.push(b'\n');
            }
            *line = salvage;
        }
        Ok((total, oversized))
    }

    /// Bytes consumed so far (always at a line boundary).
    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub(crate) fn class(&self) -> &AgentClass {
        &self.class
    }

    pub(crate) fn extra_offsets(&self) -> &HashMap<PathBuf, u64> {
        &self.extra_offsets
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
        on_progress: impl FnMut(&Self, u64, u64),
    ) -> std::io::Result<()> {
        self.ingest_file_cancellable(path, || false, on_progress)
    }
    pub(crate) fn ingest_file_cancellable(
        &mut self,
        path: &Path,
        should_stop: impl Fn() -> bool,
        mut on_progress: impl FnMut(&Self, u64, u64),
    ) -> std::io::Result<()> {
        const PROGRESS_INTERVAL_BYTES: u64 = 24 * 1024 * 1024;
        const PASS_BYTES: u64 = 16 * 1024 * 1024;

        let file = ilium_platform::secure_fs::open_regular_file(path)?;
        let length = file.metadata()?.len();
        if length < self.offset {
            let retention = self.retention.clone();
            *self = Self::new(self.class.clone());
            self.attach_retention(retention);
        }
        let pass_start = self.offset;
        let mut reader = BufReader::with_capacity(1 << 20, file);
        reader.seek(SeekFrom::Start(self.offset))?;
        let mut line = Vec::new();
        let mut since_progress = 0_u64;
        loop {
            if should_stop() {
                return Err(std::io::Error::other(
                    "Statistics cancelled; totals incomplete",
                ));
            }
            let (read, oversized) = Self::read_bounded_line(&mut reader, &mut line, &self.class)?;
            if read == 0 {
                break;
            }
            // A last line without its newline may still be mid-write. Leaving
            // it unconsumed makes the next refresh re-read it whole.
            if line.last() != Some(&b'\n') {
                break;
            }
            // Resume from the last complete record, never a partial line.
            if self.offset > pass_start
                && self.offset.saturating_sub(pass_start) + read as u64 > PASS_BYTES
            {
                break;
            }
            self.offset += read as u64;
            since_progress += read as u64;
            self.note_oversized(oversized, read);
            self.feed_line(&line);
            self.check_bounds()?;
            if since_progress >= PROGRESS_INTERVAL_BYTES {
                since_progress = 0;
                on_progress(self, self.offset, length);
            }
        }
        on_progress(self, self.offset, length);
        Ok(())
    }

    /// Feeds another transcript that belongs to the same session, namely a
    /// Claude Code sub-agent file under `<session>/subagents/`. Claude keeps
    /// those calls out of the main file, yet they are real spend: a session's
    /// main transcript can hold a fiftieth of what the CLI billed. Every line
    /// is sidechain-flagged by the CLI, so the calls count toward totals and
    /// spend but never redefine the current model or context. Like the main
    /// file it resumes at its remembered offset and leaves a partial last line
    /// for the next pass. A file that shrank is skipped rather than re-read.
    pub fn ingest_extra_file(&mut self, path: &Path) -> std::io::Result<()> {
        let file = ilium_platform::secure_fs::open_regular_file(path)?;
        let length = file.metadata()?.len();
        let mut offset = self.extra_offsets.get(path).copied().unwrap_or(0);
        if length <= offset {
            return Ok(());
        }
        let mut reader = BufReader::with_capacity(1 << 20, file);
        reader.seek(SeekFrom::Start(offset))?;
        let mut line = Vec::new();
        let pass_start = offset;
        const PASS_BYTES: u64 = 16 * 1024 * 1024;
        loop {
            let (read, oversized) = Self::read_bounded_line(&mut reader, &mut line, &self.class)?;
            if read == 0 || line.last() != Some(&b'\n') {
                break;
            }
            if offset > pass_start && offset.saturating_sub(pass_start) + read as u64 > PASS_BYTES {
                break;
            }
            offset += read as u64;
            self.note_oversized(oversized, read);
            self.feed_line(&line);
            self.check_bounds()?;
        }
        self.extra_offsets.insert(path.to_path_buf(), offset);
        Ok(())
    }

    /// Reads a sub-agent transcript of any size to its end. One
    /// [`Self::ingest_extra_file`] pass reads a bounded slice so a caller can
    /// report progress; a session's spend needs every pass, so this repeats
    /// them until the file is consumed or `cancelled` asks to stop.
    pub fn ingest_extra_file_to_end(
        &mut self,
        path: &Path,
        cancelled: &impl Fn() -> bool,
    ) -> std::io::Result<()> {
        self.ingest_to_end(path, cancelled, Self::ingest_extra_file)
    }

    /// [`Self::ingest_extra_file_to_end`] for a Codex sub-agent rollout.
    pub fn ingest_codex_child_to_end(
        &mut self,
        path: &Path,
        cancelled: &impl Fn() -> bool,
    ) -> std::io::Result<()> {
        self.ingest_to_end(path, cancelled, Self::ingest_codex_child)
    }

    fn ingest_to_end(
        &mut self,
        path: &Path,
        cancelled: &impl Fn() -> bool,
        pass: fn(&mut Self, &Path) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        loop {
            if cancelled() {
                return Err(std::io::Error::other("cancelled"));
            }
            let before = self.extra_offsets.get(path).copied().unwrap_or(0);
            pass(self, path)?;
            if self.extra_offsets.get(path).copied().unwrap_or(0) == before {
                return Ok(());
            }
        }
    }

    /// Feeds a Codex sub-agent rollout (a thread spawned by this session).
    /// Codex logs those in their own files with their own cumulative token
    /// counters, so each file keeps a separate baseline and model, and only
    /// its token spend joins this session's totals.
    pub fn ingest_codex_child(&mut self, path: &Path) -> std::io::Result<()> {
        let file = ilium_platform::secure_fs::open_regular_file(path)?;
        let length = file.metadata()?.len();
        let mut offset = self.extra_offsets.get(path).copied().unwrap_or(0);
        if length <= offset {
            return Ok(());
        }
        let mut reader = BufReader::with_capacity(1 << 20, file);
        reader.seek(SeekFrom::Start(offset))?;
        let (mut last_total, mut model) = self.codex_children.remove(path).unwrap_or_default();
        std::mem::swap(&mut self.codex_last_total, &mut last_total);
        std::mem::swap(&mut self.codex_current_model, &mut model);
        self.codex_child_mode = true;
        let result = self.read_codex_child_lines(&mut reader, &mut offset);
        self.codex_child_mode = false;
        std::mem::swap(&mut self.codex_last_total, &mut last_total);
        std::mem::swap(&mut self.codex_current_model, &mut model);
        self.codex_children
            .insert(path.to_path_buf(), (last_total, model));
        self.extra_offsets.insert(path.to_path_buf(), offset);
        result?;
        self.check_bounds()
    }

    fn read_codex_child_lines(
        &mut self,
        reader: &mut BufReader<std::fs::File>,
        offset: &mut u64,
    ) -> std::io::Result<()> {
        let mut line = Vec::new();
        let pass_start = *offset;
        const PASS_BYTES: u64 = 16 * 1024 * 1024;
        loop {
            let (read, oversized) = Self::read_bounded_line(reader, &mut line, &self.class)?;
            if read == 0 || line.last() != Some(&b'\n') {
                break;
            }
            if *offset > pass_start && offset.saturating_sub(pass_start) + read as u64 > PASS_BYTES
            {
                break;
            }
            *offset += read as u64;
            self.note_oversized(oversized, read);
            let prefix = &line[..line.len().min(CODEX_PREFIX_BYTES)];
            if matches!(
                codex_filters().classify(prefix),
                Some(CodexLine::TurnContext | CodexLine::TokenCount)
            ) {
                self.feed_codex(&line);
            }
        }
        Ok(())
    }

    fn note_oversized(&mut self, oversized: bool, bytes: usize) {
        if oversized {
            self.stats.oversized_records += 1;
            self.stats.oversized_bytes = self.stats.oversized_bytes.saturating_add(bytes as u64);
        }
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
        let mut work = WorkKind::Model;
        if let Some(Value::Array(blocks)) = message.get("content") {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                    continue;
                }
                let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                work = WorkKind::for_tool(name);
                let id = block.get("id").and_then(Value::as_str).unwrap_or("");
                let first_sight = if sidechain {
                    self.sidechain_window.note_tool_id(id)
                } else {
                    self.seen_tool_ids.insert(id.to_string())
                };
                if !id.is_empty() && !first_sight {
                    continue;
                }
                if !sidechain && work == WorkKind::Shell {
                    let command = block
                        .get("input")
                        .and_then(|input| input.get("command"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    self.note_shell_command(at_ms, command);
                }
                self.count_tool(name, at_ms);
            }
        }
        if !sidechain {
            self.push_work(at_ms, work);
        }
        let Some(usage) = message.get("usage") else {
            return;
        };
        let tokens = claude_tokens(usage);
        let advisors = claude_advisor_calls(usage);
        let id = message.get("id").and_then(Value::as_str).unwrap_or("");
        if sidechain {
            let call = ClaudeCall {
                at_ms,
                model: model.to_string(),
                tokens,
                sidechain,
                advisors,
            };
            if let Some(evicted) = self.sidechain_window.merge_or_push(id, call) {
                self.folded_sidechain.fold(&evicted);
            }
            return;
        }
        // Every content block of one response repeats the same usage object,
        // so merging by message id (never summing) avoids multi-counting.
        if let Some(&index) = self.claude_call_index.get(id).filter(|_| !id.is_empty()) {
            let call = &mut self.claude_calls[index];
            call.tokens.max_merge(&tokens);
            for (position, (advisor_model, advisor_tokens)) in advisors.into_iter().enumerate() {
                match call.advisors.get_mut(position) {
                    Some((_, existing)) => existing.max_merge(&advisor_tokens),
                    None => call.advisors.push((advisor_model, advisor_tokens)),
                }
            }
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
            advisors,
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
        self.note_progress_text(at_ms, &text);
        self.push_work(at_ms, WorkKind::Model);
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
                self.push_work(at_ms, WorkKind::Idle);
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
                    if !self.codex_child_mode {
                        self.stats.model = Some(model);
                    }
                }
                if !self.codex_child_mode {
                    set_if_some(&mut self.stats.effort, string_at(payload, "effort"));
                }
            }
            CodexLine::TokenCount => self.codex_token_count(payload, at_ms),
            CodexLine::TaskStarted => {
                self.touch(at_ms);
                self.push_work(at_ms, WorkKind::Model);
                if let Some(window) = payload.get("model_context_window").and_then(Value::as_u64) {
                    self.stats.context_window = Some(window);
                }
            }
            CodexLine::TaskComplete => {
                self.touch(at_ms);
                self.push_work(at_ms, WorkKind::Idle);
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
                self.push_work(at_ms, WorkKind::Idle);
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
                    if kind == CodexLine::EventUser {
                        self.note_progress_text(at_ms, text);
                    }
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
                self.note_progress_text(at_ms, &text);
                self.response_prompt_count += 1;
                push_bounded_prompt(&mut self.prompts, at_ms, &text);
            }
            CodexLine::ToolCall => {
                self.touch(at_ms);
                let name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let work = WorkKind::for_tool(name);
                if work == WorkKind::Shell {
                    let arguments = payload
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    self.note_shell_command(at_ms, arguments);
                }
                self.push_work(at_ms, work);
                self.count_tool(name, at_ms);
            }
            CodexLine::Compacted => {}
        }
    }

    fn codex_token_count(&mut self, payload: &Value, at_ms: Option<i64>) {
        self.touch(at_ms);
        if let Some(limits) = payload
            .get("rate_limits")
            .filter(|_| !self.codex_child_mode)
        {
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
            if let Some(at_ms) = at_ms {
                for (name, window) in &parsed {
                    self.record_quota_reading(name, window, at_ms);
                }
            }
            if !parsed.is_empty() {
                self.codex_rate_limits = parsed;
                self.codex_rate_limits_at_ms = at_ms;
            }
        }
        let Some(info) = payload.get("info").filter(|info| info.is_object()) else {
            return;
        };
        if let Some(window) = info
            .get("model_context_window")
            .and_then(Value::as_u64)
            .filter(|_| !self.codex_child_mode)
        {
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
        let context = context.filter(|_| !self.codex_child_mode);
        if let Some(context) = context {
            self.stats.context_tokens = Some(context);
            self.stats.peak_context_tokens = self.stats.peak_context_tokens.max(context);
        }
        let model = self
            .codex_current_model
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        if let Some(at_ms) = at_ms {
            self.codex_spend
                .entry((at_ms - at_ms.rem_euclid(MINUTE_MS), model.clone()))
                .or_default()
                .add(&delta);
        }
        let usage = self.codex_model_usage.entry(model).or_default();
        usage.0 += 1;
        usage.1.add(&delta);
        self.stats.model_calls += 1;
        if let Some(at_ms) = at_ms.filter(|_| !self.codex_child_mode) {
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

    /// Turns consecutive readings of one window into consumed percentage
    /// points. A reading from a different window period (the reset instant
    /// changed) or a lower one only re-baselines: how much of the new period
    /// this session used is unknown, so nothing is invented.
    fn record_quota_reading(&mut self, name: &str, window: &RateLimitWindow, at_ms: i64) {
        let previous = self.codex_quota_last.insert(
            name.to_owned(),
            (window.used_percent, window.resets_at_unix),
        );
        let Some((last_percent, last_reset)) = previous else {
            return;
        };
        let rise = window.used_percent - last_percent;
        if last_reset != window.resets_at_unix || rise <= 0.0 {
            return;
        }
        *self
            .codex_quota_spend
            .entry((at_ms - at_ms.rem_euclid(MINUTE_MS), name.to_owned()))
            .or_default() += rise;
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

    /// Records that the agent switched to `kind`; repeats of the current
    /// state carry no information and are dropped.
    fn push_work(&mut self, at_ms: Option<i64>, kind: WorkKind) {
        let Some(at_ms) = at_ms else {
            return;
        };
        if self
            .stats
            .work_marks
            .last()
            .is_some_and(|last| last.kind == kind)
        {
            return;
        }
        self.stats.work_marks.push(WorkMark { at_ms, kind });
        if self.stats.work_marks.len() > WORK_MARK_CAP {
            let thinned: Vec<WorkMark> = self.stats.work_marks.iter().copied().step_by(2).collect();
            self.stats.work_marks = thinned;
        }
    }

    /// Notes a shell command: `ilium progress set` starts a progress-bar run,
    /// `ilium progress clear` ends the oldest open one without an outcome.
    fn note_shell_command(&mut self, at_ms: Option<i64>, command: &str) {
        let Some(at_ms) = at_ms else {
            return;
        };
        if command.contains("ilium progress set") {
            self.stats.progress_spans.push(ProgressSpan {
                start_ms: at_ms,
                end_ms: None,
                outcome: ProgressOutcome::Running,
            });
            if self.stats.progress_spans.len() > PROGRESS_SPAN_CAP {
                self.stats.progress_spans.remove(0);
            }
        } else if command.contains("ilium progress clear") {
            self.end_progress(at_ms, ProgressOutcome::Cleared);
        }
    }

    /// Reads a delivered progress-monitor result out of user-visible text.
    fn note_progress_text(&mut self, at_ms: Option<i64>, text: &str) {
        let (Some(at_ms), Some((monitor_id, outcome))) = (at_ms, parse_progress_outcome(text))
        else {
            return;
        };
        if self.seen_progress_outcomes.insert(monitor_id) {
            self.end_progress(at_ms, outcome);
        }
    }

    fn end_progress(&mut self, at_ms: i64, outcome: ProgressOutcome) {
        if let Some(span) = self
            .stats
            .progress_spans
            .iter_mut()
            .find(|span| span.end_ms.is_none())
        {
            span.end_ms = Some(at_ms.max(span.start_ms));
            span.outcome = outcome;
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
        stats.work_marks.sort_by_key(|mark| mark.at_ms);
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
        let mut spend: BTreeMap<(i64, &str), TokenTotals> = BTreeMap::new();
        let mut samples = Vec::with_capacity(self.claude_calls.len());
        // Sub-agent calls are real spend, so they stay in the totals; only
        // the main conversation defines the "current" model and context.
        let retained = self
            .claude_calls
            .iter()
            .chain(self.sidechain_window.calls());
        for call in retained {
            stats.tokens.add(&call.tokens);
            let usage = by_model.entry(call.model.as_str()).or_default();
            usage.0 += 1;
            usage.1.add(&call.tokens);
            for (advisor_model, advisor_tokens) in &call.advisors {
                stats.tokens.add(advisor_tokens);
                let usage = by_model.entry(advisor_model.as_str()).or_default();
                usage.0 += 1;
                usage.1.add(advisor_tokens);
            }
            if let Some(at_ms) = call.at_ms {
                let minute_ms = at_ms - at_ms.rem_euclid(MINUTE_MS);
                spend
                    .entry((minute_ms, call.model.as_str()))
                    .or_default()
                    .add(&call.tokens);
                for (advisor_model, advisor_tokens) in &call.advisors {
                    spend
                        .entry((minute_ms, advisor_model.as_str()))
                        .or_default()
                        .add(advisor_tokens);
                }
            }
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
        stats.tokens.add(&self.folded_sidechain.tokens);
        for (model, (calls, tokens)) in &self.folded_sidechain.models {
            let usage = by_model.entry(model.as_str()).or_default();
            usage.0 += calls;
            usage.1.add(tokens);
        }
        for ((minute_ms, model), tokens) in &self.folded_sidechain.spend {
            spend
                .entry((*minute_ms, model.as_str()))
                .or_default()
                .add(tokens);
        }
        stats.models = model_list(by_model.into_iter().map(|(m, v)| (m.to_string(), v)));
        stats.samples = decimate(samples);
        stats.cost = self.cost_snapshot.clone();
        stats.spend = spend_buckets(spend.into_iter());
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
        stats.rate_limits_at_ms = self.codex_rate_limits_at_ms;
        stats.quota_spend = self.quota_buckets();
        stats.samples = decimate(self.codex_samples.clone());
        stats.spend = spend_buckets(
            self.codex_spend
                .iter()
                .map(|((minute_ms, model), tokens)| ((*minute_ms, model.as_str()), *tokens)),
        );
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

impl StatsAccumulator {
    fn quota_buckets(&self) -> Vec<QuotaBucket> {
        let mut names: HashMap<&str, Arc<str>> = HashMap::new();
        self.codex_quota_spend
            .iter()
            .map(|((minute_ms, window), percent_points)| QuotaBucket {
                minute_ms: *minute_ms,
                window: Arc::clone(
                    names
                        .entry(window.as_str())
                        .or_insert_with(|| Arc::from(window.as_str())),
                ),
                percent_points: *percent_points,
            })
            .collect()
    }
}

/// Collects per-minute spend, sharing one allocation per distinct model name.
fn spend_buckets<'a>(
    entries: impl Iterator<Item = ((i64, &'a str), TokenTotals)>,
) -> Vec<SpendBucket> {
    let mut names: HashMap<&'a str, Arc<str>> = HashMap::new();
    entries
        .map(|((minute_ms, model), tokens)| SpendBucket {
            minute_ms,
            model: Arc::clone(names.entry(model).or_insert_with(|| Arc::from(model))),
            tokens,
        })
        .collect()
}

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

/// Advisor-tool calls of one response: each is a separate billed model call
/// with its own token counts, listed as `advisor_message` iterations.
fn claude_advisor_calls(usage: &Value) -> Vec<(String, TokenTotals)> {
    usage
        .get("iterations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|iteration| {
            iteration.get("type").and_then(Value::as_str) == Some("advisor_message")
        })
        .map(|iteration| {
            let model = iteration
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("advisor")
                .to_string();
            (model, claude_tokens(iteration))
        })
        .collect()
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
pub(crate) fn codex_tokens(usage: &Value) -> TokenTotals {
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
    (!text.is_empty()).then(|| ilium_agent_session::unwrap_claude_command(text))
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

/// Builds a small stand-in record for an oversized line from its head and
/// tail. Claude assistant records keep `model`, `usage` and the timestamp
/// near an edge, so the token counts survive; every other oversized record
/// (tool output, images, API blobs) becomes an empty line and is skipped.
fn salvage_oversized(class: &AgentClass, head: &[u8], tail: &[u8]) -> Vec<u8> {
    if !matches!(class, AgentClass::Claude) {
        return Vec::new();
    }
    static FIELDS: OnceLock<[Regex; 6]> = OnceLock::new();
    let fields = FIELDS.get_or_init(|| {
        [
            Regex::new(r#""timestamp":\s*"([^"\\]{1,40})""#).expect("static regex"),
            Regex::new(r#""model":\s*"([^"\\]{1,80})""#).expect("static regex"),
            Regex::new(r#""id":\s*"(msg_[A-Za-z0-9_]{1,80})""#).expect("static regex"),
            Regex::new(r#""usage":\s*(\{(?:[^{}]|\{[^{}]*\})*\})"#).expect("static regex"),
            Regex::new(r#""isSidechain":\s*(true|false)"#).expect("static regex"),
            Regex::new(r#""type":\s*"assistant""#).expect("static regex"),
        ]
    });
    if !fields[5].is_match(head) {
        return Vec::new();
    }
    let find = |index: usize| -> Option<Vec<u8>> {
        [tail, head].iter().find_map(|part| {
            fields[index]
                .captures_iter(part)
                .last()
                .and_then(|captures| captures.get(1))
                .map(|found| found.as_bytes().to_vec())
        })
    };
    let (Some(usage), Some(model)) = (find(3), find(1)) else {
        return Vec::new();
    };
    let mut record = Vec::with_capacity(usage.len() + 256);
    record.extend_from_slice(br#"{"type":"assistant","isSidechain":"#);
    record.extend_from_slice(&find(4).unwrap_or_else(|| b"false".to_vec()));
    if let Some(timestamp) = find(0) {
        record.extend_from_slice(br#","timestamp":""#);
        record.extend_from_slice(&timestamp);
        record.push(b'"');
    }
    record.extend_from_slice(br#","message":{"model":""#);
    record.extend_from_slice(&model);
    record.push(b'"');
    if let Some(id) = find(2) {
        record.extend_from_slice(br#","id":""#);
        record.extend_from_slice(&id);
        record.push(b'"');
    }
    record.extend_from_slice(br#","usage":"#);
    record.extend_from_slice(&usage);
    record.extend_from_slice(b"}}");
    record
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
    #[test]
    fn progress_outcomes_are_read_from_delivered_monitor_text() {
        use super::{parse_progress_outcome as parse, ProgressOutcome::*};
        assert_eq!(
            parse(
                "Ilium progress monitor 7 reports that job completed successfully.\nFinal progress: 100%."
            ),
            Some((7, Success))
        );
        assert_eq!(
            parse("Ilium progress monitor 12 reports that job failed.\nError: boom."),
            Some((12, Failure))
        );
        assert_eq!(
            parse("Ilium progress monitor 3 stopped before job reached a terminal task status."),
            Some((3, Unknown))
        );
        assert_eq!(parse("fix the progress monitor"), None);
    }

    #[test]
    fn claude_transcript_yields_work_marks_and_progress_runs() {
        let mut acc = StatsAccumulator::new(AgentClass::Claude);
        let lines = [
            r#"{"type":"user","timestamp":"2026-10-03T10:00:00.000Z","message":{"role":"user","content":"build it"}}"#,
            r#"{"type":"assistant","timestamp":"2026-10-03T10:00:10.000Z","message":{"id":"m1","model":"claude-sonnet-5","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ilium progress set --command x"}}],"usage":{"input_tokens":1,"output_tokens":2}}}"#,
            r#"{"type":"assistant","timestamp":"2026-10-03T10:05:00.000Z","message":{"id":"m2","model":"claude-sonnet-5","content":[{"type":"text","text":"waiting"}],"usage":{"input_tokens":1,"output_tokens":2}}}"#,
            r#"{"type":"user","timestamp":"2026-10-03T10:06:00.000Z","message":{"role":"user","content":"Ilium progress monitor 4 reports that job failed.\nFinal progress: 40%."}}"#,
            r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-10-03T10:07:00.000Z","durationMs":420000}"#,
        ];
        for line in lines {
            acc.feed_line(line.as_bytes());
        }
        let stats = acc.snapshot();
        let kinds: Vec<_> = stats.work_marks.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            [
                WorkKind::Model,
                WorkKind::Shell,
                WorkKind::Model,
                WorkKind::Idle
            ]
        );
        assert_eq!(stats.progress_spans.len(), 1);
        let span = stats.progress_spans[0];
        assert_eq!(span.outcome, ProgressOutcome::Failure);
        assert!(span.end_ms.unwrap() > span.start_ms);
    }

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

    fn codex_quota_event(used: f64, resets_at: i64, at: &str) -> String {
        format!(
            r#"{{"timestamp":"{at}","ordinal":1,"type":"event_msg","payload":{{"type":"token_count","info":null,"rate_limits":{{"plan_type":"pro","primary":{{"used_percent":{used},"window_minutes":300,"resets_at":{resets_at}}},"secondary":{{"used_percent":10.0,"window_minutes":10080,"resets_at":999}}}}}}}}"#
        )
    }

    #[test]
    fn codex_quota_rises_become_per_minute_consumption_and_resets_only_rebaseline() {
        let lines = [
            codex_quota_event(20.0, 1000, "2026-09-20T02:57:20.000Z"),
            codex_quota_event(23.5, 1000, "2026-09-20T02:57:50.000Z"),
            codex_quota_event(23.5, 1000, "2026-09-20T02:58:10.000Z"),
            codex_quota_event(25.0, 1000, "2026-09-20T02:58:40.000Z"),
            // Window reset: a lower reading in a new period is a new baseline.
            codex_quota_event(2.0, 2000, "2026-09-20T03:30:00.000Z"),
            codex_quota_event(3.0, 2000, "2026-09-20T03:31:00.000Z"),
        ];
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let stats = feed(AgentClass::Codex, &refs);
        let primary: Vec<f64> = stats
            .quota_spend
            .iter()
            .filter(|bucket| &*bucket.window == "primary")
            .map(|bucket| bucket.percent_points)
            .collect();
        assert_eq!(primary, vec![3.5, 1.5, 1.0]);
        assert!(
            stats
                .quota_spend
                .iter()
                .all(|bucket| &*bucket.window != "secondary"),
            "a flat window consumed nothing"
        );
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
        assert_eq!(
            stats.rate_limits_at_ms,
            parse_timestamp_ms("2026-09-20T02:58:20.209Z")
        );
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

    /// Manual real-data check: `ILIUM_REAL_TRANSCRIPT=<file> ILIUM_REAL_CLASS=claude|codex`.
    #[test]
    #[ignore = "reads a real transcript named by ILIUM_REAL_TRANSCRIPT"]
    fn real_transcript_totals_for_manual_comparison() {
        let Ok(path) = std::env::var("ILIUM_REAL_TRANSCRIPT") else {
            return;
        };
        let class = if std::env::var("ILIUM_REAL_CLASS").as_deref() == Ok("codex") {
            AgentClass::Codex
        } else {
            AgentClass::Claude
        };
        let mut accumulator = StatsAccumulator::new(class);
        let path = std::path::PathBuf::from(path);
        while {
            let before = accumulator.offset();
            accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
            accumulator.offset() != before
        } {}
        let stats = accumulator.snapshot();
        let mut per_model: std::collections::BTreeMap<String, [u64; 4]> = Default::default();
        for bucket in &stats.spend {
            let entry = per_model.entry(bucket.model.to_string()).or_default();
            entry[0] += bucket.tokens.input;
            entry[1] += bucket.tokens.cache_read;
            entry[2] += bucket.tokens.cache_write;
            entry[3] += bucket.tokens.output;
        }
        println!(
            "REAL oversized={} models={per_model:?}",
            stats.oversized_records
        );
    }

    #[test]
    fn oversized_irrelevant_line_is_skipped_and_later_records_still_count() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-oversized.jsonl");
        let mut bytes = vec![b'x'; 4 * 1024 * 1024 + 1];
        bytes.push(b'\n');
        bytes.extend_from_slice(
            claude_assistant("msg_9", 7, "2026-11-10T10:00:00.000Z", None).as_bytes(),
        );
        bytes.push(b'\n');
        std::fs::write(&path, &bytes).unwrap();
        let mut accumulator = StatsAccumulator::new(AgentClass::Claude);
        accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
        assert_eq!(accumulator.offset(), bytes.len() as u64);
        let stats = accumulator.snapshot();
        assert_eq!(stats.oversized_records, 1);
        assert_eq!(stats.tokens.output, 7);
    }

    /// Manual check against a real rollout:
    /// `ILIUM_REAL_CODEX_ROLLOUT=<parent.jsonl> <test binary> real_codex_family_report --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_codex_family_report() {
        let Ok(path) = std::env::var("ILIUM_REAL_CODEX_ROLLOUT") else {
            println!("set ILIUM_REAL_CODEX_ROLLOUT to a parent rollout file");
            return;
        };
        let main = std::path::PathBuf::from(path);
        let started = std::time::Instant::now();
        let mut accumulator = StatsAccumulator::new(AgentClass::Codex);
        // The store reads a bounded slice per pass and repeats until caught up.
        let mut main_total = TokenTotals::default();
        let mut children = Vec::new();
        for pass in 0..10_000 {
            let before = (
                accumulator.offset(),
                accumulator.extra_offsets.values().sum::<u64>(),
            );
            accumulator.ingest_file(&main, |_, _, _| {}).unwrap();
            if pass == 0 || children.is_empty() {
                children = crate::session_stats_codex_children::children_of(&main);
            }
            for child in &children {
                accumulator.ingest_codex_child(child).unwrap();
            }
            let after = (
                accumulator.offset(),
                accumulator.extra_offsets.values().sum::<u64>(),
            );
            if before == after {
                break;
            }
        }
        // The main file alone, for comparison with the family total.
        let mut main_only = StatsAccumulator::new(AgentClass::Codex);
        loop {
            let before = main_only.offset();
            main_only.ingest_file(&main, |_, _, _| {}).unwrap();
            if main_only.offset() == before {
                break;
            }
        }
        main_total.add(&main_only.snapshot().tokens);
        let stats = accumulator.snapshot();
        println!(
            "children={} main_input={} main_cache_read={} main_output={}",
            children.len(),
            main_total.input,
            main_total.cache_read,
            main_total.output
        );
        println!(
            "family_input={} family_cache_read={} family_cache_write={} family_output={} models={} spend_buckets={}",
            stats.tokens.input,
            stats.tokens.cache_read,
            stats.tokens.cache_write,
            stats.tokens.output,
            stats.models.len(),
            stats.spend.len()
        );
        println!(
            "retained_bytes={} elapsed_ms={}",
            accumulator.retained_bytes() + stats.retained_bytes(),
            started.elapsed().as_millis()
        );
    }

    #[test]
    fn advisor_tool_calls_are_billed_to_the_advisor_model_once_per_response() {
        let record = |block: &str| {
            format!(
                r#"{{"type":"assistant","timestamp":"2026-11-10T10:00:10.000Z","message":{{"id":"m1","model":"claude-opus-5-5","content":[{block}],"usage":{{"input_tokens":4,"output_tokens":10,"iterations":[{{"type":"message","input_tokens":4,"output_tokens":10}},{{"type":"advisor_message","model":"claude-fable-5-2","input_tokens":1000,"output_tokens":200}}]}}}}}}"#
            )
        };
        // Two content blocks of the same response repeat the usage object.
        let stats = feed(
            AgentClass::Claude,
            &[
                &record(r#"{"type":"text","text":"a"}"#),
                &record(r#"{"type":"text","text":"b"}"#),
            ],
        );
        let fable = stats
            .models
            .iter()
            .find(|usage| usage.model == "claude-fable-5-2")
            .expect("advisor model is listed");
        assert_eq!((fable.tokens.input, fable.tokens.output), (1000, 200));
        let opus = stats
            .models
            .iter()
            .find(|usage| usage.model == "claude-opus-5-5")
            .unwrap();
        assert_eq!((opus.tokens.input, opus.tokens.output), (4, 10));
        assert!(stats
            .spend
            .iter()
            .any(|bucket| &*bucket.model == "claude-fable-5-2"));
    }

    /// Regression: a session whose sub-agents made more than 65,536 records
    /// used to fail the retained-record limit and lose all of its spend, and
    /// the pane path read only the first 16 MiB of each sub-agent file.
    #[test]
    fn huge_sub_agent_transcripts_are_counted_in_full_without_a_record_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("agent-big.jsonl");
        let calls = 80_000_u64;
        let mut text = String::new();
        for number in 0..calls {
            let minute = number % 600;
            let (hour, minute) = (10 + minute / 60, minute % 60);
            // Each response is written twice (two content blocks); a long
            // padding field pushes the file well past one 16 MiB pass.
            for _ in 0..2 {
                text.push_str(&format!(
                    r#"{{"isSidechain":true,"type":"assistant","timestamp":"2026-11-10T{hour:02}:{minute:02}:00.000Z","message":{{"id":"m{number}","model":"claude-opus-5-5","content":[{{"type":"tool_use","id":"t{number}","name":"Bash","input":{{"command":"{}"}}}}],"usage":{{"input_tokens":1,"output_tokens":2,"cache_read_input_tokens":10}}}}}}"#,
                    "x".repeat(120)
                ));
                text.push('\n');
            }
        }
        std::fs::write(&path, &text).unwrap();
        assert!(text.len() as u64 > 16 * 1024 * 1024);
        let mut accumulator = StatsAccumulator::new(AgentClass::Claude);
        accumulator
            .ingest_extra_file_to_end(&path, &|| false)
            .unwrap();
        accumulator.check_bounds().unwrap();
        let stats = accumulator.snapshot();
        assert_eq!(stats.tokens.output, calls * 2);
        assert_eq!(stats.tokens.cache_read, calls * 10);
        assert_eq!(stats.tokens.input, calls);
        assert_eq!(stats.tool_calls as u64, calls);
        let bucket_total: u64 = stats.spend.iter().map(|bucket| bucket.tokens.output).sum();
        assert_eq!(bucket_total, calls * 2);
        let opus = stats
            .models
            .iter()
            .find(|usage| usage.model == "claude-opus-5-5")
            .unwrap();
        assert_eq!(opus.calls as u64, calls);
    }

    #[test]
    fn codex_sub_agent_threads_add_their_own_spend_without_mixing_counters() {
        let directory = tempfile::tempdir().unwrap();
        let context = |model: &str, at: &str| {
            format!(
                r#"{{"timestamp":"{at}","type":"turn_context","payload":{{"model":"{model}","effort":"high"}}}}"#
            )
        };
        let main = directory.path().join("main.jsonl");
        std::fs::write(
            &main,
            format!(
                "{}\n{}\n",
                context("gpt-6.1-sol", "2026-11-10T10:00:00.000Z"),
                codex_token_count((100, 0, 10, 0), 100, "2026-11-10T10:01:00.000Z")
            ),
        )
        .unwrap();
        // The child's cumulative counter restarts at zero and uses another model.
        let child = directory.path().join("child.jsonl");
        std::fs::write(
            &child,
            format!(
                "{}\n{}\n{}\n",
                context("gpt-6.1-luna", "2026-11-10T10:02:00.000Z"),
                codex_token_count((50, 0, 5, 0), 50, "2026-11-10T10:03:00.000Z"),
                codex_token_count((80, 0, 9, 0), 30, "2026-11-10T10:04:00.000Z")
            ),
        )
        .unwrap();
        let mut accumulator = StatsAccumulator::new(AgentClass::Codex);
        accumulator.ingest_file(&main, |_, _, _| {}).unwrap();
        accumulator.ingest_codex_child(&child).unwrap();
        // A second pass must not count the child again.
        accumulator.ingest_codex_child(&child).unwrap();
        let stats = accumulator.snapshot();
        assert_eq!(stats.tokens.input, 180);
        assert_eq!(stats.tokens.output, 19);
        let mut models: Vec<_> = stats
            .models
            .iter()
            .map(|usage| (usage.model.clone(), usage.tokens.input))
            .collect();
        models.sort();
        assert_eq!(
            models,
            vec![
                ("gpt-6.1-luna".to_owned(), 80),
                ("gpt-6.1-sol".to_owned(), 100)
            ]
        );
        // The child never redefines the parent's displayed model or context.
        assert_eq!(stats.model.as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(stats.context_tokens, Some(100));
        assert!(stats
            .spend
            .iter()
            .any(|bucket| &*bucket.model == "gpt-6.1-luna"));
    }

    #[test]
    fn oversized_claude_assistant_record_keeps_its_usage() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-oversized-assistant.jsonl");
        let filler = "y".repeat(5 * 1024 * 1024);
        let line = format!(
            r#"{{"isSidechain":true,"type":"assistant","timestamp":"2026-11-10T10:00:00.000Z","message":{{"model":"claude-sonnet-5","id":"msg_big","role":"assistant","content":[{{"type":"text","text":"{filler}"}}],"usage":{{"input_tokens":3,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":42}}}}}}"#
        );
        let mut bytes = line.into_bytes();
        bytes.push(b'\n');
        std::fs::write(&path, &bytes).unwrap();
        let mut accumulator = StatsAccumulator::new(AgentClass::Claude);
        accumulator.ingest_extra_file(&path).unwrap();
        let stats = accumulator.snapshot();
        assert_eq!(stats.oversized_records, 1);
        assert_eq!(stats.tokens.output, 42);
        assert_eq!(stats.tokens.cache_read, 1000);
    }

    #[test]
    fn a_line_larger_than_the_pass_budget_still_makes_progress() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-huge.jsonl");
        let mut bytes = br#"{"type":"response_item","payload":""#.to_vec();
        bytes.extend(vec![b'z'; 17 * 1024 * 1024]);
        bytes.extend_from_slice(b"\"}\n{}\n");
        std::fs::write(&path, &bytes).unwrap();
        let mut accumulator = StatsAccumulator::new(AgentClass::Codex);
        accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
        assert!(
            accumulator.offset() > 17 * 1024 * 1024,
            "huge line consumed"
        );
        accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
        assert_eq!(accumulator.offset(), bytes.len() as u64);
    }

    #[test]
    fn oversized_codex_record_is_skipped_without_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-oversized-codex.jsonl");
        let mut bytes = br#"{"type":"response_item","payload":""#.to_vec();
        bytes.extend(vec![b'z'; 6 * 1024 * 1024]);
        bytes.extend_from_slice(b"\"}\n");
        std::fs::write(&path, &bytes).unwrap();
        let mut accumulator = StatsAccumulator::new(AgentClass::Codex);
        accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
        assert_eq!(accumulator.offset(), bytes.len() as u64);
        assert_eq!(accumulator.snapshot().oversized_records, 1);
    }

    #[test]
    fn cancellation_preserves_the_last_complete_record_offset() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-cancelled.jsonl");
        std::fs::write(&path, b"{}\n").unwrap();
        let mut accumulator = StatsAccumulator::new(AgentClass::Codex);
        assert!(accumulator
            .ingest_file_cancellable(&path, || true, |_, _, _| {})
            .unwrap_err()
            .to_string()
            .contains("cancelled"));
        assert_eq!(accumulator.offset(), 0);
        accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
        assert_eq!(accumulator.offset(), 3);
    }

    #[test]
    fn main_transcript_reads_are_bounded_per_pass_and_resume_without_losing_lines() {
        use super::StatsAccumulator;
        use ilium_core::AgentClass;
        use std::io::Write;

        const PASS_BYTES: u64 = 16 * 1024 * 1024;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large-session.jsonl");
        let chunk = b"{}\n".repeat(32 * 1024);
        let mut file = std::fs::File::create(&path).unwrap();
        for _ in 0..(2 * PASS_BYTES as usize / chunk.len()) {
            file.write_all(&chunk).unwrap();
        }
        drop(file);

        let mut accumulator = StatsAccumulator::new(AgentClass::Codex);
        accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
        assert_eq!(accumulator.offset(), PASS_BYTES / 3 * 3);

        accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
        let expected_offset = std::fs::metadata(&path).unwrap().len();
        assert_eq!(accumulator.offset(), expected_offset);
    }

    #[test]
    fn sidecar_transcript_reads_are_bounded_per_pass_and_resume_without_losing_lines() {
        use super::StatsAccumulator;
        use ilium_core::AgentClass;
        use std::io::Write;

        const PASS_BYTES: u64 = 16 * 1024 * 1024;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large-sidecar.jsonl");
        let chunk = b"{}\n".repeat(32 * 1024);
        let mut file = std::fs::File::create(&path).unwrap();
        for _ in 0..(2 * PASS_BYTES as usize / chunk.len()) {
            file.write_all(&chunk).unwrap();
        }
        drop(file);

        let mut accumulator = StatsAccumulator::new(AgentClass::Claude);
        accumulator.ingest_extra_file(&path).unwrap();
        assert_eq!(
            accumulator.extra_offsets.get(&path),
            Some(&(PASS_BYTES / 3 * 3))
        );

        accumulator.ingest_extra_file(&path).unwrap();
        let expected_offset = std::fs::metadata(&path).unwrap().len();
        assert_eq!(accumulator.extra_offsets.get(&path), Some(&expected_offset));
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
