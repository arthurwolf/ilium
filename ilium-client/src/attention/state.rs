//! Client-side holder of the attention queue and everything around it that is
//! plain data: the refresh gate, persistence bookkeeping, the arrival block
//! and the panel selection. The behaviour lives in `crate::app_attention`,
//! which needs `App`; keeping the data here keeps that file about behaviour.

use std::collections::HashMap;
use std::path::PathBuf;

use ilium_core::NodeId;

use super::runtime::{SharedAttentionCall, TurboWorkers};
use super::{AttentionKind, AttentionQueue, PersistedQueue, RefreshGate};

/// A press refresh is dropped when it comes sooner than this after the last.
pub const REFRESH_MINIMUM_INTERVAL_MS: u64 = 2_000;
/// The live panes are re-read at most this often.
pub const RECONCILE_INTERVAL_MS: u64 = 500;
/// A changed queue is written to disk after it has been stable this long.
pub const SAVE_DEBOUNCE_MS: u64 = 2_000;
/// The arrival block disappears by itself after this long.
pub const ARRIVAL_VISIBLE_MS: u64 = 60_000;

/// A newly arrived item, shown beside the last-prompt banner until the user
/// reacts, looks at the agent, or the item goes stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arrival {
    /// The pane whose item arrived.
    pub pane_id: NodeId,
    pub arrived_ms: u64,
}

#[derive(Debug)]
pub struct AttentionState {
    pub queue: AttentionQueue,
    /// Coalesces press-triggered refreshes of the LLM layer.
    pub gate: RefreshGate,
    /// How many refreshes the gate let through (`StartNow` or a queued rerun).
    pub refresh_requests: u64,
    /// Where the queue is stored; `None` disables persistence.
    pub store_path: Option<PathBuf>,
    /// Stored queue waiting for live panes before it can be re-validated.
    pub pending_restore: Option<PersistedQueue>,
    /// When the stored queue is given up on if no live pane has vouched for it.
    pub restore_deadline_ms: Option<u64>,
    pub last_reconcile_ms: Option<u64>,
    /// What was last written (or read), so an unchanged queue is not rewritten.
    pub last_saved: Option<PersistedQueue>,
    /// When the queue last differed from `last_saved`.
    pub dirty_since_ms: Option<u64>,
    pub arrival: Option<Arrival>,
    /// Kind each pane was queued as at the previous reconcile; an item
    /// "arrives" when its pane gains a kind it did not have.
    pub known_kinds: HashMap<NodeId, AttentionKind>,
    /// Highlighted row of the right-hand panel.
    pub panel_selected: usize,
    /// Shared-mode calls in flight, by project: restructure calls that also
    /// ask for the attention list. Removed when their reply (or error) lands.
    pub shared_calls: HashMap<NodeId, SharedAttentionCall>,
    /// The refresh the gate let through is still waiting for its calls.
    pub refresh_open: bool,
    /// Unix milliseconds when the open refresh started.
    pub refresh_started_ms: u64,
    /// Turbo worker threads, current and winding down.
    pub turbo: TurboWorkers,
    /// Where Turbo workers send their replies: the client's naming-event
    /// channel, attached by the event loop. `None` (tests, start-up) means a
    /// Turbo refresh has nowhere to report and is not started.
    pub events: Option<tokio::sync::mpsc::Sender<crate::naming_workers::NamingWorkerEvent>>,
}

impl Default for AttentionState {
    fn default() -> Self {
        Self {
            queue: AttentionQueue::new(),
            gate: RefreshGate::new(REFRESH_MINIMUM_INTERVAL_MS),
            refresh_requests: 0,
            store_path: None,
            pending_restore: None,
            restore_deadline_ms: None,
            last_reconcile_ms: None,
            last_saved: None,
            dirty_since_ms: None,
            arrival: None,
            known_kinds: HashMap::new(),
            panel_selected: 0,
            shared_calls: HashMap::new(),
            refresh_open: false,
            refresh_started_ms: 0,
            turbo: TurboWorkers::default(),
            events: None,
        }
    }
}
