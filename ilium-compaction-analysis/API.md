# ilium-compaction-analysis: public API

Pure analysis of agent session transcripts (Claude Code and Codex): parsers, compact
per-session traces, corpus statistics, a trace-driven compaction replay, an optimizer and the
trigger/setting semantics. Zero I/O, zero async, no ratatui, no dependency on `ilium-client`.
Dependencies: `serde`, `serde_json` (with `raw_value`), `thiserror`.

Basis: `~/.claude/docs/research/compaction-threshold-v2.md` and
`~/.claude/docs/research/session-analysis/compaction/{claude,codex}/`. Design:
`docs/compaction-optimizer-design.md` (sections 2, 4, 5, 6, 8, 9).

## Module map

| module | role |
|---|---|
| `parse` | `LogFormat` registry (`LOG_FORMATS`: one entry per agent), `TraceBuilder`, `line_may_matter` |
| `trace` | `SessionTrace`, `TurnSample`, `CompactionEvent`, `TraceCounters`, `TRACE_FORMAT_VERSION` |
| `dedupe` | `dedupe_across_traces`: removes what resumed/forked transcripts repeat across files |
| `tool_features` | `ToolFeatures`, `TurnTools`, `ToolAccumulator` (hashed tool-call features per request; re-exported by `trace`) |
| `rework` | `measure`, `measure_with`, `ReworkConfig`, `MeasuredRework`, `EstimatorKind` (measured rework per compaction) |
| `stats` | `CorpusStats::compute`, `Quantiles`, `RegimeSet`, `cluster_regimes` |
| `price` | `PriceWeights`, `PriceLookup`, `TokenCounts`, `turn_cost` |
| `replay` | `simulate`, `SimConfig`, `SimTable`, `ReworkModel`, `PostDraw` |
| `optimize` | `optimize`, `OptimizeConfig`, `Recommendation`, `ObservedCompactions`, `three_way_comparison` |
| `semantics` | `AgentSemantics` (trigger <-> setting), `CliDefaults`, offset/ratio measurement |
| `rng` | `SeededRng` (SplitMix64; deterministic draws and bootstraps) |

Crate root re-exports: `AgentKind`, `AgentProfile`, `AnalysisError`, `AnalysisResult`,
`SessionTrace`, `TRACE_FORMAT_VERSION`.

## 1. Streaming a transcript file (caller owns the I/O)

```rust
use ilium_compaction_analysis::parse::{line_may_matter, TraceBuilder};
use ilium_compaction_analysis::AgentKind;

let agent = AgentKind::ClaudeCode;                    // or AgentKind::Codex
let mut builder = TraceBuilder::new(agent)
    .with_subagent(path_is_under_subagents_dir);      // Claude: caller knows; Codex: auto from session_meta
// optional: .with_max_line_bytes(n) / .with_options(ParseOptions { .. })
let mut buffer = Vec::new();
while reader.read_until(b'\n', &mut buffer)? > 0 {    // bytes: no UTF-8 requirement
    if line_may_matter(agent, &buffer) {              // cheap byte-substring prefilter, no allocation
        builder.feed_line(&buffer);                   // never fails; bad lines are counted
    }
    buffer.clear();
}
let trace: SessionTrace = builder.finish();           // cache this (see section 6)
```

Signatures:

```rust
pub fn line_may_matter(agent: AgentKind, line: &[u8]) -> bool;
impl TraceBuilder {
    pub fn new(agent: AgentKind) -> Self;
    pub fn with_subagent(self, is_subagent: bool) -> Self;
    pub fn with_options(self, options: ParseOptions) -> Self;
    pub fn with_max_line_bytes(self, max_line_bytes: usize) -> Self;   // default 32 MiB
    pub fn feed_line(&mut self, line: &[u8]);       // trims CR/LF; tolerant of any bytes
    pub fn finish(self) -> SessionTrace;
}
pub static LOG_FORMATS: [LogFormat; 2];             // registry; adding an agent = one entry
pub fn format_for(agent: AgentKind) -> &'static LogFormat;
```

`feed_line` re-applies the prefilter internally, so skipping the caller-side prefilter is correct
but slower. Tolerated and counted (`SessionTrace::counters`): empty lines (ignored), lines above
the byte limit (`lines_skipped_oversize`, skipped unparsed), lines not starting with `{`
(`lines_not_json`), truncated or undecodable relevant lines (`lines_unparseable`; a live
session's cut-off last line lands here), irrelevant lines (`lines_irrelevant`).

Prefilter needles (any one present means "may matter"):

* Claude: `"usage"`, `compact_boundary`, `isCompactSummary`. (`isSidechain` appears on *every*
  Claude line and would defeat the filter; sidechain handling is done on the already-matched
  assistant lines instead.)
* Codex: `token_usage_record`, `"compacted"`, `turn_context`, `session_meta`,
  `thread_settings_applied`, `model_context_window`, `"function_call"`, `"custom_tool_call"` (the
  last two, with their closing quotes, match tool-call lines but not the much larger
  `*_output` lines; they carry the rework features).

### Parsing rules implemented

Claude Code (`parse/claude.rs`):

* Requests are deduplicated by `message.id` (one assistant line per content block); a later line
  with the same id replaces the usage (streaming ends with the final counts).
* With `usage.iterations`, the `type == "message"` iterations are the main requests (summed for
  cost; the **first** one gives the context size); `advisor_message` iterations are excluded, so
  advisor tool turns are not double counted.
* Context = `input_tokens + cache_read_input_tokens + cache_creation_input_tokens`; write tier
  from `usage.cache_creation.ephemeral_{5m,1h}_input_tokens`; untiered writes are treated as
  1-hour writes.
* `system` / `compact_boundary` lines yield a `CompactionEvent` (`trigger`, `preTokens`, logged
  `postTokens`, `durationMs`). The logged `postTokens` is **not** the next prompt size: the first
  following assistant request is stored as the measured post-compaction request
  (`post_context_tokens`, cache split). The `isCompactSummary` user line gives the summary size
  (`chars / 3.5` tokens, `summary_estimated = true`; the ratio is `ParseOptions`).
* `precomputed` = metadata flag truthy or duration under 1 s (the research's rule).
* Main sessions drop `<synthetic>` requests, sidechain requests, and models matching
  `ParseOptions::skip_models_in_main_sessions` (default `haiku`); subagent traces keep all but
  `<synthetic>`.

Codex (`parse/codex.rs`):

* `token_usage_record` deduplicated by `response_id` (first wins). `input_tokens` includes the
  cached part; `output_tokens` includes reasoning; context = `input_tokens`;
  `cache_write_input_tokens` (always 0) goes to the short write tier.
* `compacted` lines: the summarisation request is the record whose id equals
  `compaction_response_id` (or `latest_token_usage_record.response_id`) among the last 4 records;
  it is flagged `FLAG_COMPACTION_REQUEST`, its output is the (measured) summary size, the pre
  size is the **previous work response's `input + output`** (the quantity the trigger compares),
  the post measurement is the next work response. `compacted` lines without a matchable id (older
  formats, resumed-history replays) are counted in `compactions_unmatched` and produce no event.
  `replacement_history_tokens` is `chars / 3.5` of the raw history (informational).
* Model from `turn_context.model` / `thread_settings_applied.thread_settings.model`; context
  window from `"model_context_window":N` (byte scan of `token_count` lines), `turn_context` or
  `session_meta` when present, else the caller uses `AgentProfile::default_context_window_tokens`
  (258,400). Subagent from `session_meta.source.subagent`. Thread files are keyed by file, never by
  `thread_id`.

## 2. The trace

```rust
pub struct SessionTrace {
    pub format_version: u32,              // == TRACE_FORMAT_VERSION
    pub agent: AgentKind,
    pub is_subagent: bool,
    pub models: Vec<String>,              // TurnSample::model indexes this
    pub turns: Vec<TurnSample>,           // 40 bytes each in memory
    pub turn_id_hashes: Vec<u32>,         // parallel to turns; for dedupe_across_traces
    pub tools: ToolFeatures,              // per-request tool-call features (v2); hashes only
    pub compactions: Vec<CompactionEvent>,
    pub context_window_tokens: Option<u32>,
    pub counters: TraceCounters,
}
pub struct TurnSample {                    // serialized as a flat array, see section 6
    pub timestamp_ms: i64, pub context_tokens: u32, pub input_tokens: u32,
    pub cache_read_tokens: u32, pub cache_write_5m_tokens: u32, pub cache_write_1h_tokens: u32,
    pub output_tokens: u32, pub model: u16,
    pub flags: u8,                         // FLAG_CACHE_COLD | FLAG_GAP_COLD | FLAG_COMPACTION_REQUEST | FLAG_FIRST_AFTER_COMPACTION
    pub gap_decaseconds: u16,              // idle gap since the previous request, 10 s units, saturating
}
pub struct CompactionEvent {
    pub timestamp_ms: i64, pub turn_index: u32,            // first post-compaction turn (== turns.len() if none)
    pub trigger: CompactionTrigger,                        // Auto | Manual | Unknown (Codex: Unknown)
    pub pre_tokens: u32,                                   // size the trigger fired at
    pub logged_post_tokens: u32,                           // Claude postTokens, NOT the next prompt size
    pub last_pre_context_tokens: u32,
    pub post_measured: bool, pub post_context_tokens: u32, pub post_input_tokens: u32,
    pub post_cache_read_tokens: u32, pub post_cache_write_5m_tokens: u32, pub post_cache_write_1h_tokens: u32,
    pub summary_tokens: u32, pub summary_estimated: bool,
    pub duration_ms: u32, pub precomputed: bool, pub replacement_history_tokens: u32,
    pub model: u16, pub request_turn_index: Option<u32>,   // Codex summarisation request
}
```

* Cold-cache flags: `FLAG_CACHE_COLD` = cache read below half of the previous context (previous
  context above 20k; never set on the first request after a compaction); `FLAG_GAP_COLD` = idle
  gap above `AgentKind::cold_gap_seconds(is_subagent)` (Claude main 1 h, subagent 5 min; Codex
  30 min).
* `CompactionEvent::is_measured()` = post-compaction request observed and a pre-turn exists.
* Helpers: `max_context_tokens`, `first_context_tokens` (the fixed prefix C0), `dominant_model`,
  `cache_write_5m_share`, `measured_compactions`, `model_name`.

`dedupe_across_traces(&mut [SessionTrace]) -> DedupeReport` removes requests (matched on
`(hash, timestamp)`) and compactions (timestamp + sizes) that an *earlier* trace of the slice
already holds and recomputes derived fields. Sort main sessions before subagents, then by file
name, as the research did.

### Tool-call features (trace format v2, the input of the measured rework)

Per request, parsers record compact features, **never raw paths, search arguments or commands**
(only 32-bit hashes; the privacy test greps a serialized trace for the fixture's secret strings):

```rust
pub struct ToolFeatures { pub turns: Vec<TurnTools>, pub read_hashes: Vec<u32>, pub command_hashes: Vec<u32> }
pub struct TurnTools {            // serialized as [tool_calls, read_calls, command_calls, flags, stored_reads, stored_commands]
    pub tool_calls: u16,          // every tool call of the request
    pub read_calls: u16,          // file-read-like calls (exact count, not capped)
    pub command_calls: u16,       // Bash / exec commands
    pub flags: u8,                // TOOL_FLAG_EDIT (Edit/Write/MultiEdit/NotebookEdit, Codex apply_patch)
                                  // | TOOL_FLAG_WRITE_HEURISTIC (shell heredoc, sed -i, tee, > redirect, git apply/commit, patch)
    pub stored_reads: u8,         // how many of read_hashes belong to this request (cap 16)
    pub stored_commands: u8,      // how many of command_hashes belong to this request (cap 8)
}
```

* `ToolFeatures::turns` has exactly one row per request (`is_consistent_with(turns.len())`);
  `offsets()` gives each request's slice of the hash vectors.
* A read hash carries its kind in the two low bits: `HASH_KIND_PATH` (0: Read, `cat`/`head`/`tail`/
  `nl`/`bat`/`less`/`sed -n`, resolved against the logged cwd) or `HASH_KIND_SEARCH` (1: Grep, Glob,
  `rg`, `grep`, `ls`, `find`, `tree`, `fd`, `wc`, `git log|show|diff|status|blame`, hashed whole).
  Codex also hashes the path-like arguments of `rg`/`grep`/`ls`/`find` as path reads (the Codex
  research did). A shell command counts as ONE read-like call however many segments read.
* Command hashes cover whitespace-normalised commands of at least 20 characters that are not
  trivial (`ls`, `pwd`, `git status`, `echo`, ...).
* Claude: one assistant line is written per content block, so a request's tool-use blocks arrive
  over several lines; they are merged into the request (tool-use ids already counted are
  skipped). Codex: a response's tool calls precede its `token_usage_record` and attach to it
  (`exec` code blocks are scanned for `cmd:"..."` literals; `exec_command`/`shell`/`local_shell`
  arguments are decoded).
* `TraceCounters::tool_calls_dropped` counts tool-call lines whose features were not recorded
  (Codex lines above 1 MiB, or a block for a request that is no longer the latest).

## 3. Corpus statistics

```rust
let stats = CorpusStats::compute(agent, &traces, &StatsConfig::default(), Some(&price_lookup));
```

`CorpusStats { counts, models, compactions, cycles, cold_cache, cost_mix, growth, reach,
regimes, first_request_tokens }`:

* `counts`: main/subagent sessions and requests, compactions (auto/manual/measured), sessions with
  compaction, time span, skipped lines, bytes.
* `models`: per model usage and weighted cost / USD.
* `compactions`: p10/p50/p90 of `pre_tokens` (all / auto), first post-compaction request size,
  its cache-read share and fresh tokens, summary tokens, duration.
* `cycles`: requests / weighted cost / minutes per post-compaction cycle, counts of cycles
  `<= 3 / 10 / 30` requests, requests to the first compaction.
* `cold_cache`: cold-request and cold-gap shares and the weighted cost share of avoidable
  prefix rewrites.
* `cost_mix`: input / output / cache-read / cache-write shares of the weighted cost.
* `growth`: growth per request by previous-context bin (default 0-50k ... 500k+).
* `reach`: share of cost at contexts above 150/200/250/350k (and of the sessions reaching them).
* `regimes`: compactions clustered by `pre_tokens` (neighbours more than 1.2x apart split); the
  regime of the newest compaction has `is_most_recent`. `stats::most_recent_regime_events` returns
  that regime's measured events.
* `first_request_tokens`: distribution of C0 over main sessions.

## 4. Prices

```rust
pub type PriceLookup<'a> = &'a dyn Fn(&str) -> Option<PriceWeights>;   // model name -> $/MTok
pub struct PriceWeights { input, output, cache_read, cache_write_5m, cache_write_1h: f64 }
impl PriceWeights {
    pub const fn anthropic_research() -> Self;   // 1, 5, 0.1, 1.25, 2.0
    pub const fn codex_research() -> Self;       // 1, 5, 0.1, 1.0, 1.0 (no write premium)
    pub fn research_for(agent: AgentKind, model: &str) -> Self;   // Opus 5.5 reads at 0.05
    pub fn relative_weights(&self) -> PriceWeights;               // divide by the input price
}
```

Without a lookup (or for models it does not know) the research weights are used, `usd` fields are
`None` and `usd_complete` is false. For Codex the write prices are forced to the input price.

## 5. Replay and optimization

```rust
let mut config = SimConfig::for_agent(agent)            // grid: Claude 120..600k, Codex 100..232k
    .with_extra_triggers(&[current_trigger, default_trigger]);
// min 5 requests between compactions, 40 draw lists x 60 draws, seed 12345,
// min replay context (Claude 120k, Codex 100k), include_subagents = false
let table: SimTable = simulate(agent, &traces, &config, Some(&price_lookup))?;
let observed = ObservedCompactions::from_traces(&traces, agent);
let semantics = AgentSemantics::default_for(agent);     // optionally .with_measured_offset(..) / .with_measured_ratio(..)
let recommendation: Recommendation = optimize(&table, &observed, &semantics, &OptimizeConfig::for_agent(agent))?;
let three_way = three_way_comparison(&table, &recommendation.rework, default_trigger, current_trigger, recommendation.trigger_tokens);
let rows = table.summary(&recommendation.rework);        // per trigger: cost, USD, compactions, per-model, median session
```

Replay model (per session, unit = input-token equivalents; USD accumulated in parallel):

* Ordinary request: read `(1 - miss) * context` at the read rate, write
  `growth + miss * context` at the write rate (the session's measured 5m/1h mix), plus output and
  the small uncached input. `miss` is binary for Claude, a measured fraction for Codex.
* When the trigger fires (Claude: context after the request's growth `>=` trigger; Codex: previous
  response `input + output >=` trigger) and `>= 5` requests passed since the last compaction:
  the compaction request over the context (cache read while warm, else full input rate; no write),
  the summary output, and the rebuild of a post-compaction prefix drawn **jointly** (size and
  cache split together) from the measured compactions of the most recent regime
  (`DrawSource::RecentRegime`; falls back to all measured non-manual compactions, then to the
  research medians, each with a warning). Draw lists are shared across triggers and sessions
  (common random numbers).
* Observed compactions are removed from the replayed stream: Claude mirrors the 30 requests after
  each boundary with the work before it (rework is then charged **absolutely**); Codex drops the
  summarisation requests and credits the pre-compaction growth (rework is charged
  **differentially**, only for compactions beyond the observed count).
* Rework (`ReworkModel`): priors Claude 72,000 / Codex 40,000 weighted tokens per compaction,
  `with_multiplier(0.5 | 1 | 2)`; `ReworkModel::measured(tokens)` /
  `prefer_measured(agent, Some((tokens, n)))` switch to a caller-measured value once `n >= 30`.
  Rework is added after the simulation (`SimTable::session_cost / total_costs`), so sensitivity
  needs no re-run.

`SimTable` accessors: `total_costs`, `total_usd`, `total_compactions` (all take a session filter,
e.g. per model family), `median_session_costs`, `summary`, `trigger_index`, `measured_total`.

`optimize` returns `Recommendation { trigger_tokens, setting_name, setting_value, setting_clamped,
basis, confidence, extrapolated, observed_support_at_pick, bands { argmin_tokens,
within_2_percent, within_5_percent }, bootstrap { resamples (>= 200), argmin_p2_5/median/p97_5,
argmin_frequency, pick_within_band_share }, pick_rule, unsupported_argmin_tokens,
unsupported_relative_saving, grid_floor_limited, per_model, rework_sensitivity (0.5x/1x/2x),
candidates (cost, usd, excess, compactions, observed_support), sessions, rework, warnings }`.

Pick rule. Let S be the candidates whose `observed_support` (fraction of observed compactions at or
below the candidate) is at least `min_support_fraction` (default 1%):

* (a) `PickRule::PlainArgmin`: S is empty (Claude today: every compaction near 567k): the plain
  argmin, `extrapolated = true`, basis text "extrapolated: 0% of observed compactions at or below
  this level".
* (b) `PickRule::BandWithSupport`: the 2% band intersects S: the lowest candidate in band and S.
* (c) `PickRule::SupportedOptimum`: S is non-empty but the 2% band has no support: the cheapest
  candidate of S, `extrapolated = false`. The cheaper unsupported argmin is reported in
  `unsupported_argmin_tokens` and `unsupported_relative_saving` (`1 - cost(argmin) / cost(pick)`),
  with the warning "cost keeps falling below X tokens ... but no observed compactions there". The
  bootstrap's `pick_within_band_share` is judged against the best *supported* candidate, and
  confidence never reaches `High` in this case.

`grid_floor_limited` is true when the plain argmin is the lowest or highest grid point (the true
optimum may lie outside the grid; a warning says "widen the grid and re-run"); the caller can
widen the grid with `SimConfig::with_extra_triggers` / a custom `triggers` list.

`ObservedCompactions::from_traces(traces, agent)` includes **subagent** compactions;
`ObservedCompactions::main_sessions_only(traces, agent)` counts main sessions only, which is the
population the replayed optimum covers: use it for the support rule.

Confidence: `Low` (fewer than 10 sessions or 5 compactions, or the pick is within 2% in under 60%
of resamples), `High` only with observed support (rules a/c never), a measured rework and a stable
bootstrap, else `Medium`.

`semantics` (`SEMANTICS_VERSION = 1`):

* Claude `autoCompactWindow = trigger + offset` (default 33,000; accepted 100,000..=1,000,000;
  `measure_claude_offset` = `median(setting - preTokens)` from `claude_offset_observations(traces,
  regime, setting_in_force)`).
* Codex `model_auto_compact_token_limit`: setting never above `floor(0.9 * window)` (default window
  258,400, cap 232,560); realized trigger = `min(limit, cap) * realized_ratio` (default 1.0;
  `measure_codex_ratio`).
* `AgentSemantics::{trigger_to_setting, setting_to_trigger, cli_defaults(window)}`; defaults:
  Claude 967,000 for 1M windows else the window boundary; Codex `floor(0.9 * window)`.

### Measured rework (`rework`)

```rust
pub fn measure(traces: &[SessionTrace], agent: AgentKind) -> Option<MeasuredRework>;     // defaults, research weights
pub fn measure_with(traces: &[SessionTrace], agent: AgentKind, lookup: Option<PriceLookup<'_>>, config: &ReworkConfig) -> Option<MeasuredRework>;
// ReworkConfig { window_turns: 30, min_compactions: 30, min_control_cycle_turns: 100, bootstrap_resamples: 1000, seed: 11 }
```

`None` unless at least `min_compactions` (default 30) measured compactions of **main** sessions back
an estimator. For each such compaction (K = 30 work requests; Codex summarisation requests
excluded) three windows are compared: **A** (K requests after), **B** (K before, within the previous
cycle) and **M** (a window in the middle of the following cycle, only for cycles of at least 100
requests). Per window: read-like calls, requests that read, requests re-reading a path read before
the compaction ("lost" paths), repeated identical calls, commands identical to pre-compaction ones,
requests until the first edit/write. Excess is converted to weighted tokens as
`excess requests x mean weighted cost of an A request` (relative weights, each request's measured
cache split; the cold first request after the compaction is excluded because the compaction cost
already prices it). Estimators (`EstimatorKind`):

| estimator | value per compaction |
|---|---|
| `ExcessCallsVsPre` | `(calls(A) - calls(B)) x (non-read cost per call + read price x mean context)`, unclamped mean |
| `LostRereadTurnsVsControl` | `max(0, lost-re-read requests(A) - (M)) x cost per request` |
| `ReadingTurnsVsControl` | `max(0, reading requests(A) - (M)) x cost per request` |
| `LostRereadTurnsVsPre` | `max(0, lost-re-read requests(A) - (B)) x cost per request`, needs a previous compaction |

**Central policy (both agents): the median of the estimator means** over every estimator backed by
at least `min_compactions` compactions (the middle of the spread; for four estimators, the mean of
the middle two). No single estimator is trusted: in the research they disagreed, even in sign, and
on real Claude logs the excess-calls estimator alone was noisy (95% CI spanning zero) while the
others sat two to four times higher. The 95% interval bootstraps the median over whole compactions
(all estimators recomputed on each resample, seeded). `MeasuredRework` carries
`tokens_per_compaction` (central, clamped at 0), `mean_tokens` (central before clamping),
`median_tokens` (median of the estimators' per-compaction medians), `ci_low_tokens` /
`ci_high_tokens`, `compactions` (the smallest count among the estimators used),
`central_estimator: CentralEstimator` (`MedianOfEstimators`, or `Single(kind)` when only one
estimator reached the minimum), `central_estimators` (the estimators feeding the median), every
`EstimatorSummary` (`estimators`, also those below the minimum), the spread `spread_low_tokens` /
`spread_high_tokens` (means of the estimators used), `examined_compactions`, the mean window
metrics `after` / `before` / `control` (`WindowMeans`) and **`noisy: bool`**. `describe()` gives
"rework measured from N compactions (X weighted tokens, 95% CI a to b; median of estimators;
estimators span lo to hi[; noisy])".

**Noise guard**: `noisy` is true when the central CI lower bound is <= 0 AND the estimator spread
(high / low) exceeds `NOISY_SPREAD_RATIO` (3.0; a non-positive low end counts as unbounded;
`rework::is_noisy(ci_low, spread_low, spread_high)`). `ReworkModel::from_measurement` then blends
50/50 with the research prior (`0.5 x measured + 0.5 x prior`), sets
`ReworkModel::blended_with_prior` and says so in `describe()` ("... but noisy ..., blended 50/50
with the research prior"); otherwise the measured value is used as is.

Wiring:

```rust
let sim_config = SimConfig::for_corpus(agent, &traces, Some(&lookup));       // measured rework when available, else the prior
let opt_config = OptimizeConfig::for_corpus(agent, &traces, Some(&lookup));  // same measurement
// or explicitly: ReworkModel::from_measurement(agent, measure(&traces, agent).as_ref()) + .with_rework(model)
```

`ReworkModel` gained `measured_from_compactions: Option<u32>`, `measured_ci_tokens: Option<(f64, f64)>`
and `blended_with_prior: bool` (all `#[serde(default)]`) and `describe()`. When the model is measured, `Recommendation.basis` ends
with "rework measured from N compactions (X weighted tokens, 95% CI ...)", a matching warning
replaces the "research prior" warning, and the sensitivity rows stay at 0.5x / 1x / 2x of the
measured (or blended) value; `High` confidence becomes reachable. `for_agent` keeps returning the prior.

## 6. Cache format

Callers cache one `SessionTrace` per transcript file (`{path,size,mtime} -> trace`):

```rust
pub const TRACE_FORMAT_VERSION: u32 = 2;      // bump when the layout OR the parsing rules change (2: added `tools`)
trace.to_json_bytes() -> AnalysisResult<Vec<u8>>             // compact JSON
SessionTrace::from_json_bytes(&[u8]) -> AnalysisResult<SessionTrace>   // AnalysisError::TraceVersionMismatch on other versions
```

Exact JSON (real output on the synthetic fixture, abbreviated):

```json
{"format_version":2,"agent":"claude_code","is_subagent":false,"models":["claude-sonnet-5-5"],
 "turns":[[1791187200500,30003,3,0,0,30000,200,0,0,0],[1791187230000,31502,2,30000,0,1500,300,0,0,2]],
 "turn_id_hashes":[2490198141,2490197604],
 "tools":{"turns":[[1,1,0,0,1,0],[1,1,0,0,1,0],[1,1,1,0,2,1],[2,1,1,0,1,1],[1,0,0,1,0,0],[1,0,1,2,0,1]],
   "read_hashes":[4243326469,2125915092,2125915092,3694838033,2125915092],
   "command_hashes":[2076546020,3591943648,318341412]},
 "compactions":[{"timestamp_ms":1791196320000,"turn_index":7,"trigger":"auto","pre_tokens":100000,
   "logged_post_tokens":17000,"last_pre_context_tokens":100000,"post_measured":true,
   "post_context_tokens":40000,"post_input_tokens":3,"post_cache_read_tokens":12000,
   "post_cache_write_5m_tokens":0,"post_cache_write_1h_tokens":27997,"summary_tokens":2000,
   "summary_estimated":true,"duration_ms":5000,"precomputed":false,"replacement_history_tokens":0,
   "model":0,"request_turn_index":null}],
 "context_window_tokens":null,
 "counters":{"lines_fed":19,"bytes_fed":16790,"lines_irrelevant":1,"lines_skipped_oversize":0,
   "lines_not_json":1,"lines_unparseable":1,"duplicate_requests":1,"skipped_model_requests":1,
   "sidechain_requests_skipped":1,"compactions_unmatched":0,"requests_without_usage":0,
   "tool_calls_dropped":0}}
```

* `turns[i]` = `[timestamp_ms, context, input, cache_read, write_5m, write_1h, output, model,
  flags, gap_decaseconds]` (flags: 1 cache-cold, 2 gap-cold, 4 compaction request, 8 first after
  compaction).
* `agent`: `"claude_code"` | `"codex"`; `trigger`: `"auto"` | `"manual"` | `"unknown"`.
* Version 2 added `tools`; a version 1 cache is refused with
  `AnalysisError::TraceVersionMismatch { found: 1, expected: 2 }` (the client's cache already
  invalidates on `TRACE_FORMAT_VERSION`, so the path is: mismatch -> rescan). The version also
  changed the parsing rules slightly: a streamed Claude message keeps the time of its first line.
* Real-corpus sizes with tool features: 3,373 requests / 12 compactions from a 669 MB Claude
  transcript serialize to 296 KB; 610 requests from a 42 MB Codex rollout to 59 KB; 56,353 Claude
  requests (73 files) to 5.1 MB; 77,848 Codex requests (20 files) to 7.6 MB (about 90 to 100 bytes
  per request).
* `SimTable`, `Recommendation`, `CorpusStats` and the other result types also derive
  `Serialize/Deserialize` (default serde layout) for UI hand-off; they are not versioned caches.

## 7. Errors

`AnalysisError`: `TraceVersionMismatch { found, expected }`, `TraceSerialization`,
`InvalidConfig(String)` (empty grid, zero draws), `NoReplayableSession`,
`TriggerNotSimulated(u32)`. Parsing never returns an error.

## 8. Not covered (v1)

* The rework estimators follow the research but are not bit-identical to its Python (they use the
  compact hashed features, one unified window geometry and exclude the cold first post-compaction
  request from the per-request cost); the "turns until first edit" delay is reported as a metric but
  is not an estimator (the research found it too noisy). Measured rework is per agent and needs
  at least 30 main-session compactions with a full 30-request window on both sides.
* Quality loss from more frequent summaries is not priced; extrapolation below the observed
  range is labelled, not corrected.
* Subagents are replayable (`include_subagents`) but excluded by default and never mixed into the
  main-session optimum.
