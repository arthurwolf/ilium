//! Pure analysis of agent session transcripts: compaction events, trace-driven
//! cost replay and compaction-trigger optimisation. No I/O, no async, no UI.
//!
//! The crate turns the measured research on when an agent should compact its
//! context (Claude Code and Codex transcripts) into code. A caller (the
//! client's scan job) owns the files; this crate owns the meaning.
//!
//! # Pipeline
//!
//! 1. [`parse`]: per-agent log formats ([`parse::LOG_FORMATS`]) turn lines into
//!    a compact [`trace::SessionTrace`] through [`parse::TraceBuilder`]. The
//!    caller streams a file: prefilter with [`parse::line_may_matter`], feed
//!    the surviving lines, `finish`, and caches the trace (it is serde
//!    serializable, see [`trace::TRACE_FORMAT_VERSION`]).
//! 2. [`dedupe`]: removes requests and compactions that resumed or forked
//!    transcripts repeat across files.
//! 3. [`stats`]: [`stats::CorpusStats`] summarises many traces (compaction
//!    sizes, cycles, cold-cache cost, growth, regimes, fixed prefix).
//! 4. [`replay`]: [`replay::simulate`] replays every large session under a grid
//!    of effective triggers and returns a [`replay::SimTable`].
//! 5. [`optimize`]: [`optimize::optimize`] derives argmin, flat bands,
//!    bootstrap, per-model optima, rework sensitivity and the pick
//!    ([`optimize::Recommendation`]); [`optimize::three_way_comparison`] prices
//!    default | current | recommended.
//! 6. [`rework`]: measured rework per compaction from the hashed tool-call
//!    features (replaces the research prior once 30 compactions back it).
//! 7. [`semantics`]: maps the effective trigger to the configuration value
//!    each agent understands (and back).
//!
//! Prices are supplied by the caller ([`price::PriceLookup`]); without them the
//! unitless research weights are used and dollar figures are absent.
//!
//! # Example
//!
//! ```
//! use ilium_compaction_analysis::parse::{line_may_matter, TraceBuilder};
//! use ilium_compaction_analysis::AgentKind;
//!
//! let transcript = [
//!     r#"{"type":"user","message":{"content":"hello"}}"#,
//!     r#"{"type":"assistant","timestamp":"2026-10-05T08:00:00.000Z","message":{"id":"msg_1","model":"claude-sonnet-5-5","usage":{"input_tokens":3,"cache_read_input_tokens":0,"cache_creation_input_tokens":30000,"output_tokens":200,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":30000}}}}"#,
//! ];
//! let mut builder = TraceBuilder::new(AgentKind::ClaudeCode);
//! for line in transcript {
//!     if line_may_matter(AgentKind::ClaudeCode, line.as_bytes()) {
//!         builder.feed_line(line.as_bytes());
//!     }
//! }
//! let trace = builder.finish();
//! assert_eq!(trace.turns.len(), 1);
//! assert_eq!(trace.turns[0].context_tokens, 30_003);
//! ```

mod agent;
pub mod dedupe;
mod error;
pub mod optimize;
pub mod parse;
pub mod price;
pub mod replay;
pub mod rework;
pub mod rng;
pub mod semantics;
pub mod stats;
pub mod tool_features;
pub mod trace;
mod util;

pub use agent::{AgentKind, AgentProfile};
pub use error::{AnalysisError, AnalysisResult};
pub use trace::{SessionTrace, TRACE_FORMAT_VERSION};
