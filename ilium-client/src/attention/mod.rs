//! Attention queue: which agents need the user's attention, and why.
//!
//! Pure domain logic with no I/O and no async, so every rule is unit-testable:
//! - [`item`]: the item, its kind and the stable evidence fingerprint.
//! - [`queue`]: the in-memory queue (ordering, merge, staleness, persistence).
//! - [`instant`]: the deterministic layer that needs no LLM.
//! - [`refresh`]: coalescing of press-triggered refreshes.
//! - [`runtime`]: the one place with threads, for the Turbo worker.
//!
//! Two layers feed the queue. The instant layer derives items from live agent
//! detection. The LLM layer (shared with the restructure call, or per-agent in
//! turbo mode) only adds or annotates items; it never removes an item the live
//! state still supports, and a reply is merged only while the evidence
//! fingerprint it was computed from still matches the pane.

pub mod instant;
pub mod item;
pub mod panel;
pub mod parse;
pub mod provider;
pub mod queue;
pub mod refresh;
pub mod runtime;
pub mod settings;
pub mod state;
pub mod store;
pub mod turbo;
pub mod worker;

pub use instant::{derive_instant_item, LivePane};
pub use item::{fingerprint_parts, AttentionItem, AttentionKind, AttentionSource};
pub use queue::{AttentionQueue, LlmReply, PersistedQueue};
pub use parse::{parse_attention, take_attention_value, KnownPane, ParsedAttention};
pub use turbo::{CallOutcome, TurboJob, TurboLimiter, TurboScheduler};
pub use refresh::{RefreshDecision, RefreshGate};
pub use settings::{AttentionMode, AttentionSettings};
pub use worker::{run_turbo_batch, AttentionCallError, AttentionCompletionClient, TurboEnvironment, TurboRequest};
