//! One persistent OS owner for ordered terminal streams and immutable results.
#[cfg(test)]
#[path = "terminal_parsing_app_regressions.rs"]
pub(crate) mod app_regressions;
#[cfg(test)]
#[path = "terminal_parsing_regressions.rs"]
mod regression_tests;
use crate::{
    terminal_activity::VisibleTextEvidence,
    terminal_view::{OrderedOutputCursor, TerminalSnapshot, TerminalState, TerminalView},
};
use ilium_core::NodeId;
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, QuotaGroup, QuotaLimits, Receipt,
    RejectReason, StorageAdmission,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Condvar, Mutex, Weak,
    },
    time::Duration,
};
use tokio::sync::Notify;
const MIB: usize = 1024 * 1024;
const MAX_COMMAND_BYTES: usize = 64 * MIB;
const MAX_COMMANDS: usize = 256;
const MAX_RESULT_METADATA_BYTES: usize = 2 * MIB;
pub(crate) const MAX_STATE_BYTES: usize = 128 * MIB;
#[cfg(test)]
const MAX_SNAPSHOT_BYTES: usize = 256 * MIB;
const MAX_PIN_BYTES: usize = 128 * MIB;
const MAX_CAPTURE_BYTES: usize = 128 * MIB;
const OUTPUT_QUANTUM_BYTES: usize = 16 * 1024;
const REPLAY_PREVIEW_INTERVAL_BYTES: usize = 256 * 1024;

/// App-owned parser accounting with charged headroom for current and replacement
/// frames in each of four visible panes. Ordinary allocations cannot consume it.
pub(crate) struct ParserMemoryGovernor {
    quota: QuotaGroup,
    normal_bytes: Arc<AtomicUsize>,
    normal_limit: usize,
}

impl ParserMemoryGovernor {
    pub(crate) fn new(total_limit: usize, replacement_headroom: usize) -> Arc<Self> {
        assert!(total_limit > replacement_headroom && replacement_headroom > 0);
        Arc::new(Self {
            quota: QuotaGroup::new(QuotaLimits {
                clients: 0,
                jobs: 0,
                service_jobs: 0,
                input_bytes: 0,
                result_bytes: 0,
                worker_threads: 0,
                worker_bytes: total_limit,
            }),
            normal_bytes: Arc::new(AtomicUsize::new(0)),
            normal_limit: total_limit - replacement_headroom,
        })
    }

    fn normal_limit(&self) -> usize {
        self.normal_limit
    }

    pub(crate) fn reserve_external_storage(
        self: &Arc<Self>,
        bytes: usize,
    ) -> Result<ParserStorageLease, RejectReason> {
        self.reserve(bytes, false)
    }

    fn reserve_frame(
        self: &Arc<Self>,
        bytes: usize,
        allow_replacement_headroom: bool,
    ) -> Result<ParserStorageLease, RejectReason> {
        self.reserve(bytes, allow_replacement_headroom)
    }

    fn reserve(
        self: &Arc<Self>,
        bytes: usize,
        may_use_replacement_headroom: bool,
    ) -> Result<ParserStorageLease, RejectReason> {
        if bytes == 0 {
            return Err(RejectReason::InvalidCost);
        }
        let normal = reserve_counter(&self.normal_bytes, bytes, self.normal_limit).is_some();
        if !normal && !may_use_replacement_headroom {
            return Err(RejectReason::WorkerBytes);
        }
        let root = match self.quota.reserve_external_storage(bytes) {
            Ok(root) => root,
            Err(error) => {
                if normal {
                    self.normal_bytes.fetch_sub(bytes, Ordering::AcqRel);
                }
                return Err(error);
            }
        };
        Ok(ParserStorageLease {
            root,
            bytes,
            normal_bytes: normal.then(|| self.normal_bytes.clone()),
        })
    }

    pub(crate) fn snapshot(&self) -> ilium_execution::QuotaSnapshot {
        self.quota.snapshot()
    }

    pub(crate) fn shares_root(&self, other: &Self) -> bool {
        self.quota.shares_root(&other.quota)
    }
}

#[derive(Debug)]
pub(crate) struct ParserStorageLease {
    root: StorageAdmission,
    bytes: usize,
    normal_bytes: Option<Arc<AtomicUsize>>,
}

impl ParserStorageLease {
    fn shares_root(&self, governor: &ParserMemoryGovernor) -> bool {
        self.root.shares_root(&governor.quota)
    }

    pub(crate) fn resident_bytes(&self) -> usize {
        self.root.resident_bytes()
    }
}

impl Drop for ParserStorageLease {
    fn drop(&mut self) {
        if let Some(normal_bytes) = &self.normal_bytes {
            normal_bytes.fetch_sub(self.bytes, Ordering::AcqRel);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub(crate) enum ParserPressure {
    StatePool = 1,
    SnapshotPool = 2, // Retained immutable generations occupy the configured snapshot budget.
    PaneLimit = 3,
    StorageBusy = 4,    // Allocation bookkeeping is temporarily contended.
    ProcessStorage = 5, // The app-wide parser allocation ceiling needs hidden-owner reclamation.
}
impl ParserPressure {
    pub(crate) fn can_reclaim_hidden_engine(self) -> bool {
        matches!(
            self,
            Self::StatePool | Self::SnapshotPool | Self::ProcessStorage
        )
    }
    fn from_code(code: usize) -> Option<Self> {
        match code {
            1 => Some(Self::StatePool),
            2 => Some(Self::SnapshotPool), // Immutable generation retention is waiting.
            3 => Some(Self::PaneLimit),
            4 => Some(Self::StorageBusy), // The allocation ledger should be retried.
            5 => Some(Self::ProcessStorage),
            _ => None,
        }
    }
}
fn snapshot_budget_bytes(state_budget_bytes: usize) -> usize {
    // Reserve explicit bounded replacement and pin headroom only when pooling is enabled.
    if state_budget_bytes == 0 {
        // Off disables the configurable generation pool; shared storage still has a hard ceiling.
        return 0;
    }
    state_budget_bytes
        .saturating_add(MAX_PIN_BYTES)
        .saturating_add(MAX_STATE_BYTES) // Cover current generations, bounded old pins, and one replacement generation.
}
fn geometry_peak(rows: u16, cols: u16) -> usize {
    usize::from(rows)
        .saturating_mul(usize::from(cols))
        .saturating_mul(128)
        .saturating_add(usize::from(rows).saturating_mul(256))
        .saturating_add(128 * 1024)
}
pub(crate) fn validate_geometry(rows: u16, cols: u16) -> Result<(), String> {
    if rows == 0
        || cols == 0
        || rows > 4096
        || cols > 4096
        || geometry_peak(rows, cols) > MAX_STATE_BYTES
    {
        return Err("Terminal geometry cannot fit parser admission (4096 per dimension plus 128MiB checked cell/metadata budget); reduce pane size".into());
    }
    Ok(())
}
#[derive(Clone)]
pub(crate) struct PaneTarget {
    pub pane_id: NodeId,
    pub identity: Arc<()>,
    alive: Arc<AtomicBool>,
    focused: Arc<AtomicBool>,
    displayed: Arc<AtomicBool>,
    suspended_output: Arc<AtomicBool>,
    confirmed_removed: Arc<AtomicBool>,
    pressure: Arc<AtomicUsize>,
    progress_preview: Arc<ProgressPreview>,
    _claim: Arc<EngineClaim>,
}
struct ProgressPreview(Mutex<Option<Arc<TerminalSnapshot>>>);
impl ProgressPreview {
    fn latest(&self) -> Option<Arc<TerminalSnapshot>> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
    fn replace(&self, snapshot: Arc<TerminalSnapshot>) {
        let mut current = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(previous) = current.replace(snapshot) {
            if let Some(charge) = &previous.allocation_charge {
                charge.retire_live();
            }
        }
    }
    fn clear(&self) {
        let mut current = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(previous) = current.take() {
            if let Some(charge) = &previous.allocation_charge {
                charge.retire_live();
            }
        }
    }
}
impl Drop for ProgressPreview {
    fn drop(&mut self) {
        let current = self.0.get_mut().unwrap_or_else(|error| error.into_inner());
        if let Some(previous) = current.take() {
            if let Some(charge) = &previous.allocation_charge {
                charge.retire_live();
            }
        }
    }
}
struct EngineClaim(Arc<AtomicUsize>, ParserStorageLease); // The engine envelope remains owned through retirement and retained targets.
impl Drop for EngineClaim {
    fn drop(&mut self) {
        let _storage_bytes = self.1.resident_bytes(); // Keep the declared envelope visible until this claim actually retires.
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
pub(crate) enum PaneCommand {
    Register {
        rows: u16,
        cols: u16,
        budget: u16,
    },
    Output {
        first: u64,
        sequence: u64,
        bytes: Vec<u8>,
        track: bool,
    },
    Replay {
        sequence: u64,
        bytes: Vec<u8>,
        complete: bool,
    },
    Resize(u16, u16),
    Budget(u16),
    ScrollUp(u16),
    ScrollDown(u16),
    Bottom,
    History(usize),
    HistoryFenced {
        byte: usize,
        origin: Arc<()>,
    },
    Fingerprint,
    // Production captures use the exact acknowledged immutable source.
    // Keep the parser-copy path only for its allocation/ordering regressions.
    #[cfg(test)]
    Capture {
        generation: u64,
    },
    InputBarrier {
        generation: u64,
    },
}
impl PaneCommand {
    fn bytes(&self) -> usize {
        match self {
            Self::Output { bytes, .. } | Self::Replay { bytes, .. } => bytes.capacity(),
            _ => std::mem::size_of::<Self>(),
        }
    }
}
struct Command {
    target: PaneTarget,
    ordinal: u64,
    body: PaneCommand,
    bytes: usize,
    cursor: OrderedOutputCursor,
    blocked: Option<StateDeferral>,
    reported_blocked: Option<ParserPressure>,
    publication_pending: bool,
    previewed_bytes: usize,
    evidence: Option<VisibleTextEvidence>,
}
#[derive(Clone, Copy)]
struct StateDeferral {
    pressure: ParserPressure,
    required_bytes: usize,
    limit_bytes: usize,
}
struct Queue {
    commands: VecDeque<Command>,
    bytes: usize,
    active_bytes: usize,
    active_commands: usize,
    closed: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    changed: Arc<Condvar>,
    notification: Arc<Notify>,
    results: Mutex<VecDeque<ParseResult>>,
    snapshot_bytes: Arc<AtomicUsize>,
    pin_bytes: Arc<AtomicUsize>,
    capture_bytes: Arc<AtomicUsize>,
    storage: Arc<ParserMemoryGovernor>, // One app-owned finite ledger covers allocations and their retiring owners.
    engine_claims: Arc<AtomicUsize>,
    /// Aggregate bytes all engines may retain; follows `terminal.engine_memory_budget_mib`.
    state_limit: AtomicUsize,
    /// Aggregate snapshot bytes include explicit pin and replacement headroom; zero disables pooling.
    snapshot_limit: AtomicUsize,
}
pub(crate) enum ParseResult {
    InputBarrier {
        target: PaneTarget,
        generation: u64,
        ordinal: u64,
        sequence: u64,
        mouse: bool,
        paste: bool,
    },
    Acknowledged {
        target: PaneTarget,
        ordinal: u64,
        evidence: Option<VisibleTextEvidence>,
    },
    Published {
        target: PaneTarget,
        ordinal: u64,
        snapshot: Arc<TerminalSnapshot>,
        evidence: Option<VisibleTextEvidence>,
    },
    #[cfg(test)]
    Capture {
        target: PaneTarget,
        generation: u64,
        snapshot: crate::smart_copy::SmartCopySnapshot,
    },
    #[cfg(test)]
    CaptureFailed {
        target: PaneTarget,
        generation: u64,
        message: String,
    },
    Error {
        target: PaneTarget,
        message: String,
    },
}
impl ParseResult {
    fn metadata_bytes(&self) -> usize {
        let extra = match self {
            Self::Published {
                evidence: Some(evidence),
                ..
            }
            | Self::Acknowledged {
                evidence: Some(evidence),
                ..
            } => evidence
                .rows
                .capacity()
                .saturating_mul(std::mem::size_of::<
                    crate::terminal_activity::VisibleRowEvidence,
                >())
                .saturating_add(
                    evidence
                        .rows
                        .iter()
                        .map(|row| row.text.capacity())
                        .sum::<usize>(),
                ),
            Self::Error { message, .. } => message.capacity(),
            #[cfg(test)]
            Self::CaptureFailed { message, .. } => message.capacity(),
            _ => 0,
        };
        // Fixed typed queue storage and Arc/control metadata are charged even
        // when their payloads are separately admitted immutable allocations.
        std::mem::size_of::<Self>()
            .saturating_add(128)
            .saturating_add(extra)
    }
}
pub(crate) struct SnapshotCharge {
    bytes: usize,
    used: Arc<AtomicUsize>,
    charged: AtomicBool,
    live: bool,
    ui_live_owner: AtomicBool,
    live_owners: AtomicUsize,
    released: Arc<Condvar>,
    pins: Arc<AtomicUsize>,
    captures: Arc<AtomicUsize>,
    cached_pin: Mutex<Weak<SnapshotPin>>,
    _storage: Arc<ParserStorageLease>,
    storage_governor: Arc<ParserMemoryGovernor>,
}
impl std::fmt::Debug for SnapshotCharge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Report allocation facts without cloning or changing admission.
        formatter
            .debug_struct("SnapshotCharge")
            .field("bytes", &self.bytes)
            .field("storage", &self._storage)
            .finish() // Show the owned byte envelope and lease.
    }
}
#[derive(Debug)]
pub(crate) struct SnapshotLiveLease {
    charge: Arc<SnapshotCharge>,
}
impl Drop for SnapshotLiveLease {
    fn drop(&mut self) {
        self.charge.release_live_owner();
    }
}
#[derive(Debug)]
pub(crate) struct SnapshotPin {
    bytes: usize,
    used: Arc<AtomicUsize>,
    _charge: Arc<SnapshotCharge>,
}
impl Drop for SnapshotPin {
    fn drop(&mut self) {
        self.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
impl Drop for SnapshotCharge {
    fn drop(&mut self) {
        if self.charged.swap(false, Ordering::AcqRel) {
            self.used.fetch_sub(self.bytes, Ordering::AcqRel);
        }
        self.released.notify_all(); // Only final allocation release can return snapshot or capture credit.
    }
}
impl SnapshotCharge {
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
    /// Every queued/actually emitted frame keeps this allocation inside the
    /// existing live category. UI replacement alone cannot free that debit.
    pub(crate) fn lease_live(self: &Arc<Self>) -> Arc<SnapshotLiveLease> {
        // Invariant: called while the current facade still owns its live
        // category. Existing painted leases clone their guard instead.
        self.live_owners.fetch_add(1, Ordering::AcqRel);
        Arc::new(SnapshotLiveLease {
            charge: self.clone(),
        })
    }
    fn release_live_owner(&self) {
        if self.live_owners.fetch_sub(1, Ordering::AcqRel) == 1 {
            // UI retirement can leave pins, histories, or other snapshot owners alive.
            self.released.notify_all(); // Wake bookkeeping without releasing the allocation's final-owner debit.
        }
    }
    pub(crate) fn retire_live(&self) {
        if self.live && self.ui_live_owner.swap(false, Ordering::AcqRel) {
            self.release_live_owner();
        }
    }
    pub(crate) fn pin(self: &Arc<Self>) -> Result<Arc<SnapshotPin>, String> {
        let mut cached = self
            .cached_pin
            .try_lock()
            .map_err(|_| "Terminal history pin admission busy; retry".to_string())?;
        if let Some(pin) = cached.upgrade() {
            return Ok(pin);
        }
        reserve_counter(&self.pins,self.bytes,MAX_PIN_BYTES).ok_or_else(||format!("Terminal retained-generation pin budget exhausted ({} bytes required;128MiB aggregate limit); release older search/context captures",self.bytes))?;
        let pin = Arc::new(SnapshotPin {
            bytes: self.bytes,
            used: self.pins.clone(),
            _charge: self.clone(),
        });
        *cached = Arc::downgrade(&pin);
        Ok(pin)
    }
    pub(crate) fn reserve_capture(&self, bytes: usize) -> Result<Arc<Self>, String> {
        reserve_counter(&self.captures, bytes, MAX_CAPTURE_BYTES).ok_or_else(|| format!(
            "Smart Copy capture admission exhausted ({bytes} bytes required;128MiB aggregate limit); release an older capture or reduce pane geometry"))?;
        let storage = match self.storage_governor.reserve_external_storage(bytes) {
            // Every capture owns its independent allocation lease.
            Ok(storage) => storage, // Keep the lease through the capture's final allocation owner.
            Err(error) => {
                self.captures.fetch_sub(bytes, Ordering::AcqRel); // No allocation or category credit may leak on rejection.
                self.released.notify_all();
                return Err(format!("Smart Copy capture storage admission: {error:?}"));
            }
        };
        Ok(Arc::new(Self {
            bytes,
            used: self.captures.clone(),
            charged: AtomicBool::new(true),
            live: false,
            ui_live_owner: AtomicBool::new(false),
            live_owners: AtomicUsize::new(0),
            released: self.released.clone(),
            pins: self.pins.clone(),
            captures: self.captures.clone(),
            cached_pin: Mutex::new(Weak::new()),
            _storage: Arc::new(storage), // A capture must not borrow the original snapshot's byte envelope.
            storage_governor: self.storage_governor.clone(),
        }))
    }
    fn reserve_checked(
        shared: &Shared,
        bytes: usize,
        capture: bool,
        allow_replacement_headroom: bool,
    ) -> Result<Option<Self>, RejectReason> {
        let (used, limit) = if capture {
            (&shared.capture_bytes, MAX_CAPTURE_BYTES)
        } else {
            (
                &shared.snapshot_bytes,
                shared.snapshot_limit.load(Ordering::Acquire),
            )
        };
        let limit = if !capture && limit == 0 {
            usize::MAX
        } else {
            limit
        };
        if reserve_counter(used, bytes, limit).is_none() {
            // Refuse before allocating a new immutable generation.
            return Ok(None);
        }
        let storage = match if capture {
            shared.storage.reserve_external_storage(bytes)
        } else {
            shared
                .storage
                .reserve_frame(bytes, allow_replacement_headroom)
        } {
            // Lease the exact conservative allocation size independently.
            Ok(storage) => storage, // Keep this root admission until the final allocation owner drops.
            Err(error) => {
                // Category credit is not allocation custody until both admissions succeed.
                used.fetch_sub(bytes, Ordering::AcqRel);
                shared.changed.notify_all();
                return Err(error);
            }
        };
        Ok(Some(Self {
            bytes,
            used: used.clone(),
            charged: AtomicBool::new(true),
            live: !capture,
            ui_live_owner: AtomicBool::new(!capture),
            live_owners: AtomicUsize::new(usize::from(!capture)),
            released: shared.changed.clone(),
            pins: shared.pin_bytes.clone(),
            captures: shared.capture_bytes.clone(),
            cached_pin: Mutex::new(Weak::new()),
            _storage: Arc::new(storage), // The lease belongs to this immutable allocation, not to the producing parser lifetime.
            storage_governor: shared.storage.clone(),
        })) // Return a fully admitted allocation guard.
    }
    #[cfg(test)]
    fn reserve(shared: &Shared, bytes: usize, capture: bool) -> Option<Self> {
        Self::reserve_checked(shared, bytes, capture, true)
            .ok()
            .flatten()
    }
}
fn reserve_counter(used: &Arc<AtomicUsize>, bytes: usize, limit: usize) -> Option<()> {
    let mut current = used.load(Ordering::Acquire);
    loop {
        let next = current.checked_add(bytes)?;
        if next > limit {
            return None;
        }
        match used.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Some(()),
            Err(actual) => current = actual,
        }
    }
}
pub(crate) struct PaneFrontend {
    target: PaneTarget,
    shared: Arc<Shared>,
    ordinal: u64,
    pending: VecDeque<PaneCommand>,
    output_backpressure: bool,
    older_intents: usize,
}
impl PaneFrontend {
    pub(crate) fn set_focused(&self, focused: bool) {
        if self.target.focused.swap(focused, Ordering::AcqRel) != focused {
            self.shared.changed.notify_one();
        }
    }

    pub(crate) fn set_displayed(&self, displayed: bool) {
        if self.target.displayed.swap(displayed, Ordering::AcqRel) != displayed {
            self.shared.changed.notify_one();
        }
    }
    pub(crate) fn parser_pressure(&self) -> Option<ParserPressure> {
        ParserPressure::from_code(self.target.pressure.load(Ordering::Acquire))
    }
    pub(crate) fn accepts_target(&self, target: &PaneTarget) -> bool {
        // Fence old parser generations while preserving domain pane identity.
        Arc::ptr_eq(&self.target.identity, &target.identity)
            && Arc::ptr_eq(&self.target.alive, &target.alive)
            && self.target.alive.load(Ordering::Acquire) // Require this exact still-live frontend generation.
    }
    pub(crate) fn can_evict(&self, applied_ordinal: u64) -> bool {
        // Never discard admitted or locally pending commands to manufacture pool credit.
        self.ordinal == applied_ordinal
            && self.pending.is_empty()
            && !self.output_backpressure
            && !self.has_suspended_output()
            && !self.is_confirmed_removed()
            && self.target.alive.load(Ordering::Acquire) // Every admitted ordinal must already be consumed before eviction.
    }
    pub(crate) fn progress_preview(&self) -> Option<Arc<TerminalSnapshot>> {
        self.target.progress_preview.latest()
    }
    pub(crate) fn clear_progress_preview(&self) {
        self.target.progress_preview.clear();
    }
    pub(crate) fn is_confirmed_removed(&self) -> bool {
        self.target.confirmed_removed.load(Ordering::Acquire)
    }
    pub(crate) fn has_suspended_output(&self) -> bool {
        self.target.suspended_output.load(Ordering::Acquire)
    }
    pub(crate) fn confirm_removed(&mut self, identity: &Arc<()>) -> bool {
        if !Arc::ptr_eq(identity, &self.target.identity) || !self.has_suspended_output() {
            return false;
        }
        if self
            .target
            .confirmed_removed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        self.target.alive.store(false, Ordering::Release);
        // This is confirmed domain removal of this exact instance, not a
        // speculative user ClosePane request. Pending intents are canceled.
        self.pending.clear();
        self.shared.changed.notify_all();
        self.shared.notification.notify_one();
        true
    }
    pub(crate) fn submit(&mut self, body: PaneCommand) -> Result<u64, PaneCommand> {
        let output = matches!(
            body,
            PaneCommand::Output { .. } | PaneCommand::Replay { .. }
        );
        if !(self.pending.is_empty()
            || output && self.output_backpressure && self.older_intents == 0)
        {
            if output && !self.output_backpressure {
                self.older_intents = self.pending.len();
                self.output_backpressure = true;
            }
            return Err(body);
        }
        let result = self.submit_direct(body);
        if output {
            if result.is_err() && !self.output_backpressure {
                self.older_intents = self.pending.len();
            }
            self.output_backpressure = result.is_err();
        }
        result
    }
    fn submit_direct(&mut self, body: PaneCommand) -> Result<u64, PaneCommand> {
        let bytes = body.bytes();
        let Ok(mut queue) = self.shared.queue.try_lock() else {
            return Err(body);
        };
        if queue.closed
            || queue.commands.len().saturating_add(queue.active_commands) >= MAX_COMMANDS
            || queue
                .bytes
                .saturating_add(queue.active_bytes)
                .saturating_add(bytes)
                > MAX_COMMAND_BYTES
        {
            return Err(body);
        }
        let Some(ordinal) = self.ordinal.checked_add(1) else {
            return Err(body);
        };
        queue.bytes += bytes;
        queue.commands.push_back(Command {
            target: self.target.clone(),
            ordinal,
            body,
            bytes,
            cursor: OrderedOutputCursor::default(),
            blocked: None,
            reported_blocked: None,
            publication_pending: false,
            previewed_bytes: 0,
            evidence: None,
        });
        self.ordinal = ordinal;
        drop(queue);
        self.shared.changed.notify_one();
        Ok(ordinal)
    }
    pub(crate) fn submit_intent(&mut self, body: PaneCommand) -> Result<(), String> {
        let limit = if matches!(body, PaneCommand::InputBarrier { .. }) {
            32
        } else {
            31
        };
        self.submit_bounded_intent(body, limit)
    }
    /// One slot belongs to the single already-admitted input barrier's effect.
    pub(crate) fn submit_input_effect(&mut self, body: PaneCommand) -> Result<(), String> {
        self.submit_bounded_intent(body, 32)
    }
    fn submit_bounded_intent(&mut self, body: PaneCommand, limit: usize) -> Result<(), String> {
        if self.pending.len() >= limit {
            return Err(
                "terminal action rejected: pending command limit; retry after parser progresses"
                    .into(),
            );
        }
        if !self.pending.is_empty() || self.output_backpressure {
            self.pending.push_back(body);
            return Ok(());
        }
        match self.submit_direct(body) {
            Ok(_) => Ok(()),
            Err(body) => {
                self.pending.push_back(body);
                Ok(())
            }
        }
    }
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }
    pub(crate) fn retry(&mut self) {
        if self.output_backpressure && self.older_intents == 0 {
            return;
        }
        while let Some(body) = self.pending.pop_front() {
            match self.submit_direct(body) {
                Ok(_) => {
                    if self.output_backpressure {
                        self.older_intents = self.older_intents.saturating_sub(1);
                        if self.older_intents == 0 {
                            break;
                        }
                    }
                }
                Err(body) => {
                    self.pending.push_front(body);
                    break;
                }
            }
        }
    }
}
impl Drop for PaneFrontend {
    fn drop(&mut self) {
        self.target.alive.store(false, Ordering::Release);
        self.shared.changed.notify_one();
    }
}
pub struct TerminalParsing {
    shared: Arc<Shared>,
    receipt: Receipt<ParserJob>,
}
impl TerminalParsing {
    /// Zero disables optional pooling; every allocation still owns a lease under a finite storage root.
    pub fn start(client: Client, budget_mib: u32) -> Result<Self, String> {
        // Fixture callers get an explicit finite root; production injects its app-owned shared root.
        let budget_bytes = (budget_mib as usize).saturating_mul(MIB);
        Self::start_with_storage(
            client,
            crate::execution::terminal_storage_quota(),
            budget_bytes,
        )
    }
    pub(crate) fn pool_enabled(&self) -> bool {
        self.shared.state_limit.load(Ordering::Acquire) != 0
    }
    /// Apply off, on, increases, and decreases live without replacing existing allocation custody.
    pub fn set_budget_mib(&self, budget_mib: u32) {
        let bytes = (budget_mib as usize).saturating_mul(MIB);
        self.shared
            .snapshot_limit
            .store(snapshot_budget_bytes(bytes), Ordering::Release);
        self.shared.state_limit.store(bytes, Ordering::Release);
        self.shared.changed.notify_all(); // Retry retained commands against the new policy promptly.
        self.shared.notification.notify_one();
    }
    pub(crate) fn start_with_storage(
        client: Client,
        storage: Arc<ParserMemoryGovernor>, // Fixtures and production supply an isolated or shared app root.
        budget_bytes: usize,
    ) -> Result<Self, String> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                commands: VecDeque::with_capacity(MAX_COMMANDS),
                bytes: 0,
                active_bytes: 0,
                active_commands: 0,
                closed: false,
            }),
            changed: Arc::new(Condvar::new()),
            notification: Arc::new(Notify::new()),
            results: Mutex::new(VecDeque::with_capacity(MAX_COMMANDS)),
            snapshot_bytes: Arc::new(AtomicUsize::new(0)),
            pin_bytes: Arc::new(AtomicUsize::new(0)),
            capture_bytes: Arc::new(AtomicUsize::new(0)),
            storage,
            engine_claims: Arc::new(AtomicUsize::new(0)),
            state_limit: AtomicUsize::new(budget_bytes),
            snapshot_limit: AtomicUsize::new(snapshot_budget_bytes(budget_bytes)), // Pool snapshots include bounded pin and replacement headroom; zero remains unpooled.
        });
        let receipt = client
            .try_submit(
                Lane::Service,
                JobCost {
                    input_bytes: 128 * MIB,
                    result_bytes: 2 * MIB,
                },
                ParserJob(shared.clone()),
            )
            .map_err(|rejected| format!("terminal parser startup: {:?}", rejected.reason))?;
        Ok(Self { shared, receipt })
    }
    pub fn notification(&self) -> Arc<Notify> {
        self.shared.notification.clone()
    }
    /// Engines currently claimed, including those still retiring.
    pub(crate) fn engine_claims(&self) -> usize {
        self.shared.engine_claims.load(Ordering::Acquire)
    }
    pub(crate) fn attach(&self, pane_id: NodeId, view: &mut TerminalView) -> Result<(), String> {
        if view.frontend.is_some() {
            return Ok(());
        }
        let (rows, cols) = view.desired_size;
        if let Err(error) = validate_geometry(rows, cols) {
            view.admission_pressure = Some(ParserPressure::PaneLimit);
            return Err(error);
        }
        let storage = match self
            .shared
            .storage
            .reserve_external_storage(MAX_STATE_BYTES)
        {
            Ok(storage) => storage,
            Err(error) => {
                view.admission_pressure = Some(match error {
                    RejectReason::WorkerBytes => ParserPressure::ProcessStorage,
                    RejectReason::Busy | RejectReason::AccountingPoisoned => {
                        ParserPressure::StorageBusy
                    }
                    _ => ParserPressure::ProcessStorage,
                });
                return Err(format!("terminal engine storage admission: {error:?}"));
            }
        }; // Admission precedes target construction; the claim retains this debit through retirement.
        view.admission_pressure = None;
        view.admission_error = None;
        self.shared.engine_claims.fetch_add(1, Ordering::AcqRel);
        let claim = Arc::new(EngineClaim(self.shared.engine_claims.clone(), storage)); // Keep its envelope through the last retiring target owner.
        let target = PaneTarget {
            pane_id,
            identity: view.identity.clone(),
            alive: Arc::new(AtomicBool::new(true)),
            focused: Arc::new(AtomicBool::new(false)),
            displayed: Arc::new(AtomicBool::new(false)),
            suspended_output: Arc::new(AtomicBool::new(false)),
            confirmed_removed: Arc::new(AtomicBool::new(false)),
            pressure: Arc::new(AtomicUsize::new(0)), // A newly attached generation starts with no owner-reported pressure.
            progress_preview: Arc::new(ProgressPreview(Mutex::new(None))),
            _claim: claim,
        };
        let mut frontend = PaneFrontend {
            target,
            shared: self.shared.clone(),
            ordinal: 0,
            pending: VecDeque::with_capacity(32),
            output_backpressure: false,
            older_intents: 0,
        };
        frontend
            .submit(PaneCommand::Register {
                rows: view.desired_size.0,
                cols: view.desired_size.1,
                budget: view.budget_mib,
            })
            .map_err(|_| "terminal parser registration backpressure".to_string())?;
        view.attach_frontend(frontend);
        Ok(())
    }
    pub(crate) fn collect(&self) -> Vec<ParseResult> {
        let Ok(mut results) = self.shared.results.try_lock() else {
            return Vec::new();
        };
        let drained: Vec<ParseResult> = results.drain(..).collect();
        drop(results);
        if !drained.is_empty() {
            self.shared.changed.notify_one();
        }
        drained
    }
    pub(crate) fn pending_work(&self) -> Option<(usize, usize)> {
        let queue = self.shared.queue.try_lock().ok()?;
        Some((
            queue.commands.len() + queue.active_commands,
            queue.bytes.saturating_add(queue.active_bytes),
        ))
    }
    pub(crate) fn failure(&mut self) -> Option<String> {
        match self.receipt.try_take() {
            JobPoll::Pending | JobPoll::Taken => None,
            JobPoll::Lost => Some(
                "Terminal parser owner lost its completion receipt; admitted state is uncertain"
                    .into(),
            ),
            JobPoll::Ready(outcome) => Some(match outcome.view() {
                JobOutcome::Finished(Ok(())) => "Terminal parser owner exited".into(),
                JobOutcome::Finished(Err(error)) => error.clone(),
                JobOutcome::NotStarted { .. } => {
                    "Terminal parser was cancelled before startup".into()
                }
                JobOutcome::Panicked => {
                    "Terminal parser owner panicked; admitted state is uncertain".into()
                }
            }),
        }
    }
    pub fn cancel(&mut self) {
        self.receipt.cancel();
        if let Ok(mut queue) = self.shared.queue.try_lock() {
            queue.closed = true;
        }
        self.shared.changed.notify_all();
    }
}
impl Drop for TerminalParsing {
    fn drop(&mut self) {
        self.cancel();
    }
}
struct Engine {
    state: TerminalState, // Drop actual mutable parser allocations before the target can release its envelope.
    target: PaneTarget,
}
/// Drops the engines of panes whose frontend is gone. Releasing an engine
/// returns its claim and memory, so the client is woken: a displayed pane
/// that was refused admission while this one retired retries only when the
/// event loop runs again.
fn retire_dropped_engines(engines: &mut HashMap<NodeId, Engine>, shared: &Shared) {
    let before = engines.len();
    engines.retain(|_, engine| engine.target.alive.load(Ordering::Acquire));
    if engines.len() != before {
        shared.notification.notify_one();
    }
}
fn command_display_priority(command: &Command) -> usize {
    command_priority(
        command.target.focused.load(Ordering::Acquire),
        command.target.displayed.load(Ordering::Acquire),
    )
}
fn command_priority(focused: bool, displayed: bool) -> usize {
    if focused && displayed {
        0
    } else if displayed {
        1
    } else {
        2
    }
}
fn best_priority_index(priorities: impl Iterator<Item = (usize, usize)>) -> Option<usize> {
    priorities
        .min_by_key(|(index, priority)| (*priority, *index))
        .map(|(index, _)| index)
}
fn should_run_deferred(
    deferred_priority: Option<usize>,
    queued_priority: Option<usize>,
    retry_deferred: bool,
) -> bool {
    let Some(deferred_priority) = deferred_priority else {
        return false;
    };
    match queued_priority {
        None => true,
        Some(queued_priority) => {
            deferred_priority < queued_priority
                || deferred_priority == queued_priority && retry_deferred
        }
    }
}
struct ParserJob(Arc<Shared>);
impl Job for ParserJob {
    type Output = ();
    type Error = String;
    fn run(self, context: JobContext) -> Result<(), String> {
        let shared = self.0;
        let _exit_wake = ExitWake(shared.notification.clone());
        let mut engines: HashMap<NodeId, Engine> = HashMap::new();
        // Suspended original commands remain inside the shared job/byte bounds.
        let mut deferred: VecDeque<Command> = VecDeque::new();
        let mut retry_deferred = false;
        let mut retry_remaining = 0_usize;
        loop {
            if context.stop_requested() {
                return cancellation_report(&shared);
            }
            retire_dropped_engines(&mut engines, &shared);
            let mut queue = shared
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let eligible = |queue: &Queue, deferred: &VecDeque<Command>| {
                best_priority_index(
                    queue
                        .commands
                        .iter()
                        .enumerate()
                        .filter(|(_, command)| {
                            !deferred.iter().any(|pending| {
                                command.target.pane_id == pending.target.pane_id
                                    && Arc::ptr_eq(
                                        &command.target.identity,
                                        &pending.target.identity,
                                    )
                            })
                        })
                        .map(|(index, command)| (index, command_display_priority(command))),
                )
            };
            while eligible(&queue, &deferred).is_none()
                && retry_remaining == 0
                && !context.stop_requested()
                && !queue.closed
            {
                queue = shared
                    .changed
                    .wait_timeout(queue, Duration::from_millis(100))
                    .unwrap_or_else(|error| error.into_inner())
                    .0;
                // An idle parser must still retire engines of dropped panes,
                // or an evicted pane would hold its claim until some other
                // pane happened to send a command.
                retire_dropped_engines(&mut engines, &shared);
                if !deferred.is_empty() {
                    retry_remaining = deferred.len(); // Retry every retained target before another all-blocked wait.
                    break;
                }
            }
            if context.stop_requested() || queue.closed {
                drop(queue);
                return cancellation_report(&shared);
            }
            let position = eligible(&queue, &deferred);
            let deferred_position = best_priority_index(
                deferred
                    .iter()
                    .enumerate()
                    .map(|(index, command)| (index, command_display_priority(command))),
            );
            let deferred_priority =
                deferred_position.map(|index| command_display_priority(&deferred[index]));
            let queued_priority =
                position.map(|index| command_display_priority(&queue.commands[index]));
            let run_deferred = retry_remaining != 0
                && should_run_deferred(deferred_priority, queued_priority, retry_deferred);
            let mut command = if run_deferred {
                // Alternate fresh work with bounded deferred sweeps.
                retry_deferred = false;
                retry_remaining -= 1; // A failed admission must consume retry budget instead of causing a busy loop.
                                      // Invariant: guarded by the selected priority entry; exclusively owned.
                deferred
                    .remove(deferred_position.unwrap())
                    .expect("selected deferred command exists")
            } else if let Some(position) = position {
                retry_deferred = !deferred.is_empty();
                retry_remaining = retry_remaining.max(deferred.len()); // Fresh work gives each older deferred target another bounded opportunity.
                                                                       // Invariant: position was obtained under this same queue lock.
                let command = queue
                    .commands
                    .remove(position)
                    .expect("eligible command exists");
                queue.bytes -= command.bytes;
                queue.active_bytes += command.bytes;
                queue.active_commands += 1;
                command
            } else {
                continue;
            };
            command.blocked = None;
            let consumed_before = command.cursor.consumed; // A retry can distinguish real byte progress from repeated admission failure.
            let mut slice_bytes = 0_usize;
            let mut voluntarily_yielded = false;
            drop(queue);
            if !command.target.alive.load(Ordering::Acquire) {
                command.target.pressure.store(0, Ordering::Release); // Definitive target invalidation cancels this generation's pressure.
                if command.target.suspended_output.load(Ordering::Acquire) {
                    push_result(
                        &shared,
                        ParseResult::Error {
                            target: command.target.clone(),
                            message: format!(
                                "Confirmed pane invalidation canceled admitted terminal ordinal{} after{} bytes; remaining original command explicitly released",
                                command.ordinal, command.cursor.consumed
                            ),
                        },
                        &context,
                    )?;
                }
                let mut queue = shared
                    .queue
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                queue.active_bytes -= command.bytes;
                queue.active_commands -= 1;
                continue;
            }
            if command.reported_blocked == Some(ParserPressure::PaneLimit)
                && !matches!(
                    &command.body,
                    PaneCommand::Output { .. } | PaneCommand::Replay { .. }
                )
            {
                // Output copy-on-write peaks can improve when retained history owners release.
                deferred.push_back(command); // Other fenced operations have unchanged geometry and allocation peaks until cancellation.
                continue;
            }
            let mut evidence = None;
            let result: Result<(), String> = (|| {
                if command.publication_pending {
                    // A completed mutation must only retry immutable publication.
                    return publish_command(&shared, &engines, &mut command, &context);
                    // Preserve its exact ordinal, cursor, and saved semantic evidence.
                }
                if let PaneCommand::Register { rows, cols, budget } = &command.body {
                    validate_geometry(*rows, *cols)?;
                    let initial = geometry_peak(*rows, *cols);
                    try_state_budget(
                        &mut engines,
                        initial,
                        &shared,
                        &context,
                        &command.target,
                        &mut command.blocked,
                    )?;
                    engines.insert(
                        command.target.pane_id,
                        Engine {
                            target: command.target.clone(),
                            state: TerminalState::with_scrollback_budget_mib(*rows, *cols, *budget),
                        },
                    );
                } else {
                    let mut engine = engines
                        .remove(&command.target.pane_id)
                        .ok_or("terminal parser pane is awaiting admission")?;
                    if !Arc::ptr_eq(&engine.target.identity, &command.target.identity) {
                        engines.insert(engine.target.pane_id, engine);
                        return Err("stale terminal pane instance".into());
                    }
                    let operation: Result<(), String> = (|| {
                        match &command.body {
                            PaneCommand::Output {
                                first,
                                sequence,
                                bytes,
                                track,
                            } => {
                                evidence = engine.state.apply_ordered_output(
                                    *first,
                                    *sequence,
                                    bytes,
                                    *track,
                                    &mut command.cursor,
                                    |state, chunk| {
                                        if slice_bytes >= OUTPUT_QUANTUM_BYTES {
                                            voluntarily_yielded = true;
                                            return Err(
                                                "terminal output yielded its parsing quantum"
                                                    .into(),
                                            );
                                        }
                                        try_state_budget(
                                            &mut engines,
                                            state.input_peak_bytes(chunk),
                                            &shared,
                                            &context,
                                            &command.target,
                                            &mut command.blocked,
                                        )?; // Refuse the next exact slice before mutation if its allocation cannot be admitted.
                                        slice_bytes = slice_bytes.saturating_add(chunk.len());
                                        Ok(())
                                    },
                                )?;
                            }
                            PaneCommand::Replay {
                                sequence, bytes, ..
                            } => {
                                if !command.cursor.replay_started
                                    && *sequence > engine.state.output_sequence()
                                {
                                    let (rows, columns) = engine.state.live_size();
                                    try_state_budget(
                                        &mut engines,
                                        engine
                                            .state
                                            .retained_allocation_bytes()
                                            .saturating_add(geometry_peak(rows, columns)),
                                        &shared,
                                        &context,
                                        &command.target,
                                        &mut command.blocked,
                                    )?;
                                }
                                engine.state.apply_ordered_replay(
                                    bytes,
                                    *sequence,
                                    &mut command.cursor,
                                    |state, chunk| {
                                        if slice_bytes >= OUTPUT_QUANTUM_BYTES {
                                            voluntarily_yielded = true;
                                            return Err(
                                                "terminal output yielded its parsing quantum"
                                                    .into(),
                                            );
                                        }
                                        try_state_budget(
                                            &mut engines,
                                            state.input_peak_bytes(chunk),
                                            &shared,
                                            &context,
                                            &command.target,
                                            &mut command.blocked,
                                        )?; // Refuse the next exact slice before mutation if its allocation cannot be admitted.
                                        slice_bytes = slice_bytes.saturating_add(chunk.len());
                                        Ok(())
                                    },
                                )?;
                            }
                            PaneCommand::Resize(rows, cols) => {
                                validate_geometry(*rows, *cols)?;
                                let peak = engine.state.resize_peak_bytes(*rows, *cols);
                                try_state_budget(
                                    &mut engines,
                                    peak,
                                    &shared,
                                    &context,
                                    &command.target,
                                    &mut command.blocked,
                                )?;
                                engine.state.resize(*rows, *cols);
                            }
                            PaneCommand::Budget(budget) => {
                                engine.state.set_scrollback_budget_mib(*budget)
                            }
                            PaneCommand::ScrollUp(lines) => {
                                try_state_budget(
                                    &mut engines,
                                    engine.state.retained_allocation_bytes().saturating_mul(2),
                                    &shared,
                                    &context,
                                    &command.target,
                                    &mut command.blocked,
                                )?;
                                engine.state.scroll_up(*lines);
                            }
                            PaneCommand::ScrollDown(lines) => engine.state.scroll_down(*lines),
                            PaneCommand::Bottom => engine.state.scroll_to_bottom(),
                            PaneCommand::History(byte)
                            | PaneCommand::HistoryFenced { byte, .. } => {
                                if let PaneCommand::HistoryFenced { origin, .. } = &command.body {
                                    if !engine.state.history_origin_matches(origin) {
                                        return Err("Search history origin changed before navigation; refresh the search".into());
                                    }
                                }
                                try_state_budget(
                                    &mut engines,
                                    engine.state.history_rebuild_peak_bytes(*byte),
                                    &shared,
                                    &context,
                                    &command.target,
                                    &mut command.blocked,
                                )?;
                                engine.state.jump_to_history_byte(*byte);
                            }
                            PaneCommand::Fingerprint => {
                                engine.state.synchronize_visible_text_fingerprint()
                            }
                            #[cfg(test)]
                            PaneCommand::Capture { generation } => {
                                let (rows, cols) = engine.state.with_screen(|screen| screen.size());
                                let capture_bytes = engine
                                    .state
                                    .retained_allocation_bytes()
                                    .saturating_add(
                                        usize::from(rows)
                                            .saturating_mul(usize::from(cols))
                                            .saturating_mul(256),
                                    )
                                    .saturating_add(128 * 1024);
                                let charge=SnapshotCharge::reserve(&shared,capture_bytes,true).ok_or_else(||format!("Smart Copy capture cannot fit its128MiB aggregate admission budget ({} bytes required); release an older capture or reduce pane geometry",capture_bytes))?;
                                let mut snapshot = engine
                                    .state
                                    .with_screen(crate::smart_copy::SmartCopySnapshot::capture);
                                snapshot.retain_allocation(Arc::new(charge));
                                push_result(
                                    &shared,
                                    ParseResult::Capture {
                                        target: command.target.clone(),
                                        generation: *generation,
                                        snapshot,
                                    },
                                    &context,
                                )?;
                            }
                            PaneCommand::InputBarrier { generation } => {
                                push_result(
                                    &shared,
                                    ParseResult::InputBarrier {
                                        target: command.target.clone(),
                                        generation: *generation,
                                        ordinal: command.ordinal,
                                        sequence: engine.state.output_sequence(),
                                        mouse: engine.state.wants_mouse_protocol(),
                                        paste: engine.state.wants_bracketed_paste(),
                                    },
                                    &context,
                                )?;
                            }
                            PaneCommand::Register { .. } => {}
                        }
                        Ok(())
                    })();
                    engines.insert(engine.target.pane_id, engine);
                    operation?;
                }
                let has_separate_result = match command.body {
                    PaneCommand::InputBarrier { .. } => true,
                    #[cfg(test)]
                    PaneCommand::Capture { .. } => true,
                    _ => false,
                };
                if has_separate_result {
                    // Input barriers and fixture captures also complete their admitted ordinal.
                    return push_result(
                        &shared,
                        ParseResult::Acknowledged {
                            target: command.target.clone(),
                            ordinal: command.ordinal,
                            evidence: command.evidence.take(),
                        },
                        &context,
                    );
                }
                command.evidence = evidence; // Keep the completed operation's bounded semantic samples while its snapshot waits.
                command.publication_pending = true; // Every retry after this point must skip all mutable terminal operations.
                publish_command(&shared, &engines, &mut command, &context) // Attempt publication once and return control to the owner on pressure.
            })();
            if voluntarily_yielded {
                // A consumed byte quantum retains its original command without reporting memory pressure.
                command.target.pressure.store(0, Ordering::Release);
                command.reported_blocked = None;
                command
                    .target
                    .suspended_output
                    .store(true, Ordering::Release);
                if matches!(
                    &command.body,
                    PaneCommand::Replay { .. } | PaneCommand::Output { .. }
                ) && (command.previewed_bytes == 0
                    || command
                        .cursor
                        .consumed
                        .saturating_sub(command.previewed_bytes)
                        >= REPLAY_PREVIEW_INTERVAL_BYTES)
                {
                    if let Some(engine) = engines.get(&command.target.pane_id) {
                        if publish_progress_preview(&shared, engine, &command.target) {
                            command.previewed_bytes = command.cursor.consumed;
                        }
                    }
                }
                let made_progress = command.cursor.consumed > consumed_before; // Only real byte progress can replenish runnable retry credit.
                deferred.push_back(command); // Keep the same byte/count debit and fence later commands for this target.
                if made_progress {
                    // A runnable output must not wait one hundred milliseconds per quantum.
                    retry_remaining = retry_remaining.max(deferred.len());
                }
                continue; // Return to retirement and cross-pane scheduling without publishing partial output.
            }
            if let Some(blocked) = command.blocked {
                command
                    .target
                    .pressure
                    .store(blocked.pressure as usize, Ordering::Release);
                if matches!(
                    command.body,
                    PaneCommand::Output { .. } | PaneCommand::Replay { .. }
                ) {
                    command
                        .target
                        .suspended_output
                        .store(true, Ordering::Release);
                }
                if command.reported_blocked != Some(blocked.pressure) {
                    // Report each changed admission class once while preserving the original command.
                    let boundary = match blocked.pressure {
                        ParserPressure::StatePool => "engine memory backpressure", // Hidden-engine retirement can release aggregate state credit.
                        ParserPressure::SnapshotPool => "immutable snapshot admission blocked", // Only final generation release returns immutable allocation credit.
                        ParserPressure::PaneLimit => "single-pane allocation limit", // An individual operation remains bounded even when pooling is off.
                        ParserPressure::StorageBusy => "allocation bookkeeping busy", // Retry contended accounting without evicting unrelated panes.
                        ParserPressure::ProcessStorage => "app-wide parser storage pressure", // Hidden idle engines may release the hard ceiling.
                    };
                    let message = format!(
                        "Terminal {boundary} ({} bytes required; {} bytes limit); original command/cursor retained while other panes continue",
                        blocked.required_bytes, blocked.limit_bytes
                    );
                    push_result(
                        &shared,
                        ParseResult::Error {
                            target: command.target.clone(),
                            message,
                        },
                        &context,
                    )?;
                    command.reported_blocked = Some(blocked.pressure);
                }
                deferred.push_back(command);
                continue;
            }
            if let Err(message) = result {
                push_result(
                    &shared,
                    match &command.body {
                        #[cfg(test)]
                        PaneCommand::Capture { generation } => ParseResult::CaptureFailed {
                            target: command.target.clone(),
                            generation: *generation,
                            message,
                        },
                        _ => ParseResult::Error {
                            target: command.target.clone(),
                            message,
                        },
                    },
                    &context,
                )?;
                push_result(
                    &shared,
                    ParseResult::Acknowledged {
                        target: command.target.clone(),
                        ordinal: command.ordinal,
                        evidence: command.evidence.take(),
                    },
                    &context,
                )?; // Explicit command errors complete their ordinal without acknowledging any memory or quantum deferral.
            }
            command.target.pressure.store(0, Ordering::Release);
            command
                .target
                .suspended_output
                .store(false, Ordering::Release);
            let mut queue = shared
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            queue.active_bytes -= command.bytes;
            queue.active_commands -= 1;
            drop(queue);
            shared.notification.notify_one();
        }
    }
}
fn publish_progress_preview(shared: &Shared, engine: &Engine, target: &PaneTarget) -> bool {
    if !target.alive.load(Ordering::Acquire) {
        return false;
    }
    let bytes = engine.state.retained_allocation_bytes();
    if bytes > MAX_STATE_BYTES {
        return false; // Leave the final publication path to report the normal per-pane limit.
    }
    let Ok(Some(charge)) = SnapshotCharge::reserve_checked(
        shared,
        bytes,
        false,
        target.displayed.load(Ordering::Acquire),
    ) else {
        return false; // A preview is optional; the immutable final publication retains exact pressure reporting.
    };
    let charge = Arc::new(charge);
    let mut snapshot = engine.state.publish();
    snapshot.history.retain_charge(charge.clone());
    snapshot.allocation_charge = Some(charge);
    target.progress_preview.replace(Arc::new(snapshot));
    shared.notification.notify_one();
    true
}
fn publish_command(
    shared: &Shared,
    engines: &HashMap<NodeId, Engine>,
    command: &mut Command,
    context: &JobContext,
) -> Result<(), String> {
    // Publication gets one nonblocking admission attempt for this exact completed command.
    if context.stop_requested() || !command.target.alive.load(Ordering::Acquire) {
        return Err("terminal publication cancelled before acknowledgement".into());
    }
    let engine = engines
        .get(&command.target.pane_id)
        .ok_or("terminal state disappeared")?; // The deferred command fences later mutations of this engine.
    let bytes = engine.state.retained_allocation_bytes(); // Charge the conservative immutable allocation before cloning it.
    if bytes > MAX_STATE_BYTES {
        // Preserve the independent per-generation bound in both parser modes.
        command.blocked = Some(StateDeferral {
            pressure: ParserPressure::PaneLimit,
            required_bytes: bytes,
            limit_bytes: MAX_STATE_BYTES,
        }); // Retain the already-completed command until explicit recovery or cancellation.
        return Err("terminal publication exceeds its single-generation admission".into());
    }
    let charge = match SnapshotCharge::reserve_checked(
        shared,
        bytes,
        false,
        command.target.displayed.load(Ordering::Acquire),
    ) {
        // Pool policy and independent storage custody are separate admissions.
        Ok(Some(charge)) => Arc::new(charge),
        Ok(None) => {
            // Only this pane waits while the owner remains available to retire and process others.
            command.blocked = Some(StateDeferral {
                pressure: ParserPressure::SnapshotPool,
                required_bytes: bytes,
                limit_bytes: shared.snapshot_limit.load(Ordering::Acquire),
            });
            return Err("terminal immutable generation allocation deferred".into());
        }
        Err(RejectReason::Busy) => {
            // Retry contended bookkeeping without changing semantic ownership.
            command.blocked = Some(StateDeferral {
                pressure: ParserPressure::StorageBusy,
                required_bytes: bytes,
                limit_bytes: usize::MAX,
            });
            return Err("terminal immutable storage bookkeeping deferred".into());
            // Retry without reexecuting the completed terminal operation.
        }
        Err(RejectReason::WorkerBytes) => {
            command.blocked = Some(StateDeferral {
                pressure: ParserPressure::ProcessStorage,
                required_bytes: bytes,
                limit_bytes: crate::execution::TERMINAL_STORAGE_BYTES,
            });
            return Err("terminal immutable allocation reached the app storage ceiling".into());
        }
        Err(error) => return Err(format!("terminal immutable storage admission: {error:?}")),
    };
    let mut snapshot = engine.state.publish(); // Clone only after every byte has an admitted allocation owner.
    snapshot.history.retain_charge(charge.clone()); // A retained history segment keeps the whole conservative generation debit alive.
    snapshot.allocation_charge = Some(charge); // Painted sources and pins share this exact allocation lease.
    push_result(
        shared,
        ParseResult::Published {
            target: command.target.clone(),
            ordinal: command.ordinal,
            snapshot: Arc::new(snapshot),
            evidence: command.evidence.take(),
        },
        context,
    ) // Publish the original command's evidence exactly once after successful admission.
}

fn coalesce_published(results: &mut VecDeque<ParseResult>, replacement: &ParseResult) {
    let ParseResult::Published {
        target: new_target, ..
    } = replacement
    else {
        return; // Keep errors, barriers, and acknowledgements independent of screen coalescing.
    };
    for result in results.iter_mut() {
        // Preserve every existing ordinal and semantic sample at its original position.
        let same = matches!(result, ParseResult::Published { target, .. } if target.pane_id == new_target.pane_id && Arc::ptr_eq(&target.identity, &new_target.identity) && Arc::ptr_eq(&target.alive, &new_target.alive)); // Replace only the exact frontend generation's older pictures.
        if !same {
            // Unrelated pictures retain their allocation owners unchanged.
            continue;
        }
        let placeholder = ParseResult::Error {
            target: new_target.clone(),
            message: String::new(),
        };
        if let ParseResult::Published {
            target,
            ordinal,
            evidence,
            ..
        } = std::mem::replace(result, placeholder)
        {
            // Final-owner drops alone release old snapshot credit.
            *result = ParseResult::Acknowledged {
                target,
                ordinal,
                evidence,
            };
        }
    }
}

fn push_result(shared: &Shared, result: ParseResult, context: &JobContext) -> Result<(), String> {
    let bytes = result.metadata_bytes();
    let queue_slack = MAX_COMMANDS * std::mem::size_of::<ParseResult>();
    if bytes.saturating_add(queue_slack) > MAX_RESULT_METADATA_BYTES {
        return Err("Terminal result metadata cannot fit its2MiB publication budget".into());
    }
    // The one not-yet-published result is scratch against the128MiB input
    // declaration; mailbox+active commands together are at most64MiB.
    let mut result = Some(result);
    loop {
        if context.stop_requested() {
            return cancellation_report(shared);
        }
        let mut results = shared
            .results
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let used = results
            .iter()
            .map(ParseResult::metadata_bytes)
            .sum::<usize>();
        if results.len() < MAX_COMMANDS
            && used.saturating_add(bytes).saturating_add(queue_slack) <= MAX_RESULT_METADATA_BYTES
        {
            if let Some(result) = result.take() {
                coalesce_published(&mut results, &result);
                results.push_back(result);
            }
            drop(results);
            shared.notification.notify_one();
            return Ok(());
        }
        drop(results);
        let queue = shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        drop(
            shared
                .changed
                .wait_timeout(queue, Duration::from_millis(100))
                .unwrap_or_else(|error| error.into_inner()),
        );
    }
}

fn try_state_budget(
    engines: &mut HashMap<NodeId, Engine>,
    peak: usize,
    shared: &Shared,
    context: &JobContext,
    target: &PaneTarget,
    deferred: &mut Option<StateDeferral>,
) -> Result<(), String> {
    if context.stop_requested() || !target.alive.load(Ordering::Acquire) {
        return Err("terminal command cancelled by pane closure/shutdown".into());
    }
    if peak > MAX_STATE_BYTES {
        // One engine's conservative operation peak remains bounded even without a pool.
        *deferred = Some(StateDeferral {
            pressure: ParserPressure::PaneLimit,
            required_bytes: peak,
            limit_bytes: MAX_STATE_BYTES,
        });
        return Err("terminal operation exceeds its single-engine allocation limit".into());
        // Preserve the original command and exact cursor for explicit recovery.
    }
    let limit = shared.state_limit.load(Ordering::Acquire);
    if limit == 0 {
        return Ok(());
    }
    retire_dropped_engines(engines, shared); // Pooled admission retires stale engines; unpooled retirement already runs once per owner quantum.
    let used = engines
        .values()
        .map(|engine| engine.state.retained_allocation_bytes())
        .sum::<usize>();
    if used.saturating_add(peak) <= limit {
        return Ok(());
    }
    *deferred = Some(StateDeferral {
        pressure: ParserPressure::StatePool,
        required_bytes: peak,
        limit_bytes: limit,
    });
    Err("terminal state allocation deferred before processing the next byte slice".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    use std::time::Instant;

    #[test]
    fn displayed_panes_preempt_hidden_parser_backlog_without_losing_fifo_ties() {
        let queued = [(0, 1), (1, 0), (2, 0), (3, 1)];
        assert_eq!(best_priority_index(queued.into_iter()), Some(1));
        assert!(should_run_deferred(Some(0), Some(1), false));
        assert!(!should_run_deferred(Some(1), Some(0), true));
        assert!(should_run_deferred(Some(0), Some(0), true));
    }

    #[test]
    fn newly_focused_pane_preempts_older_visible_sibling_backlog() {
        let queued = [
            (0, command_priority(false, true)),
            (1, command_priority(true, true)),
            (2, command_priority(false, false)),
        ];
        assert_eq!(best_priority_index(queued.into_iter()), Some(1));
        assert_eq!(command_priority(true, false), 2);
        let focused_priority = command_priority(true, true);
        let sibling_priority = command_priority(false, true);
        assert!(should_run_deferred(
            Some(focused_priority),
            Some(sibling_priority),
            false
        ));
        assert!(!should_run_deferred(
            Some(sibling_priority),
            Some(focused_priority),
            true
        ));
    }

    struct Bank(Execution);
    impl Bank {
        fn start() -> (Self, TerminalParsing) {
            Self::start_with_budget(256)
        }
        fn start_with_budget(budget_mib: u32) -> (Self, TerminalParsing) {
            Self::start_with_storage(budget_mib, crate::execution::terminal_storage_quota())
        }
        fn start_with_storage(
            budget_mib: u32,
            storage: Arc<ParserMemoryGovernor>,
        ) -> (Self, TerminalParsing) {
            let quota = QuotaGroup::new(QuotaLimits {
                clients: 1,
                jobs: 1,
                service_jobs: 1,
                input_bytes: 128 * MIB,
                result_bytes: 2 * MIB,
                worker_threads: 1,
                // The finite service bank is independent of parser allocation custody.
                worker_bytes: 641 * MIB,
            });
            let empty = LaneConfig {
                threads: 0,
                queue_slots: 0,
                priority: None,
                resident_bytes_per_thread: 0,
            };
            let execution = Execution::start(
                quota,
                ExecutionConfig {
                    cpu: empty,
                    io: empty,
                    service: LaneConfig {
                        threads: 1,
                        queue_slots: 1,
                        priority: None,
                        resident_bytes_per_thread: 128 * MIB,
                    },
                },
            )
            .unwrap();
            let client = execution
                .client(ClientLimits {
                    jobs: 1,
                    service_jobs: 1,
                    input_bytes: 128 * MIB,
                    result_bytes: 2 * MIB,
                })
                .unwrap();
            let parsing = TerminalParsing::start_with_storage(
                client,
                storage,
                (budget_mib as usize).saturating_mul(MIB),
            )
            .unwrap();
            (Self(execution), parsing)
        }
    }
    impl Drop for Bank {
        fn drop(&mut self) {
            self.0.request_shutdown(ShutdownMode::Cancel);
            assert_eq!(
                self.0
                    .join_until_background(Instant::now() + Duration::from_secs(5))
                    .unwrap()
                    .remaining_workers,
                0
            );
        }
    }

    #[test]
    fn parser_governors_are_isolated_or_shared_by_injection() {
        let first = crate::execution::terminal_storage_quota();
        let second = crate::execution::terminal_storage_quota();
        let first_clone = first.clone();
        assert!(first.shares_root(&first_clone));
        assert!(!first.shares_root(&second));
        let normal_limit = first.normal_limit();
        assert_eq!(
            crate::execution::TERMINAL_STORAGE_BYTES - normal_limit,
            crate::execution::TERMINAL_REPLACEMENT_HEADROOM
        );
        let lease = first.reserve_external_storage(normal_limit).unwrap();
        assert!(second.reserve_external_storage(normal_limit).is_ok());
        assert!(first.reserve_external_storage(1).is_err());
        let replacement = first
            .reserve_frame(crate::execution::TERMINAL_REPLACEMENT_HEADROOM, true)
            .unwrap();
        assert_eq!(
            first.snapshot().worker_bytes,
            crate::execution::TERMINAL_STORAGE_BYTES
        );
        assert!(first.reserve_frame(1, true).is_err());
        drop(replacement);
        drop(lease);
        assert!(first.reserve_external_storage(normal_limit).is_ok());

        let shared = crate::execution::terminal_storage_quota();
        let (_first_bank, first_parser) = Bank::start_with_storage(0, shared.clone());
        let (_second_bank, second_parser) = Bank::start_with_storage(0, shared);
        assert!(first_parser
            .shared
            .storage
            .shares_root(&second_parser.shared.storage));
        let full = first_parser
            .shared
            .storage
            .reserve_external_storage(first_parser.shared.storage.normal_limit())
            .unwrap();
        assert!(second_parser
            .shared
            .storage
            .reserve_external_storage(1)
            .is_err());
        drop(full);
    }

    #[test]
    fn pooling_off_still_refuses_over_cap_engine_and_recovers_after_owner_drop() {
        let storage = crate::execution::terminal_storage_quota_with_limit(2 * MAX_STATE_BYTES);
        let (_bank, parsing) = Bank::start_with_storage(0, storage);
        assert!(!parsing.pool_enabled());
        let mut first = TerminalView::new(24, 80);
        parsing.attach(NodeId(1), &mut first).unwrap();
        let mut waiting = TerminalView::new(24, 80);
        assert!(parsing.attach(NodeId(2), &mut waiting).is_err());
        assert_eq!(
            waiting.parser_pressure(),
            Some(ParserPressure::ProcessStorage)
        );

        drop(first);
        let deadline = Instant::now() + Duration::from_secs(5);
        while parsing.engine_claims() != 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            parsing.engine_claims(),
            0,
            "retired engine must release its storage lease"
        );
        parsing.attach(NodeId(2), &mut waiting).unwrap();
        assert_eq!(waiting.parser_pressure(), None);
    }

    fn attach(parsing: &TerminalParsing, id: NodeId, view: &mut TerminalView) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if parsing.attach(id, view).is_ok() {
                return;
            }
            assert!(Instant::now() < deadline, "registration did not progress");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn offer(view: &mut TerminalView, mut command: PaneCommand) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match view.frontend.as_mut().unwrap().submit(command) {
                Ok(_) => return,
                Err(original) => command = original,
            }
            assert!(Instant::now() < deadline, "command did not progress");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn result(
        parsing: &TerminalParsing,
        mut wanted: impl FnMut(&ParseResult) -> bool,
    ) -> ParseResult {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            for result in parsing.collect() {
                if wanted(&result) {
                    return result;
                }
                if let ParseResult::Error { message, .. } = result {
                    panic!("parser error: {message}");
                }
            }
            assert!(Instant::now() < deadline, "result did not arrive");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn drain_and_install(
        parsing: &TerminalParsing,
        panes: &mut [(NodeId, TerminalView)],
    ) -> Vec<Arc<TerminalSnapshot>> {
        let mut installed = Vec::new();
        for result in parsing.collect() {
            let (target, ordinal, snapshot) = match result {
                ParseResult::Published {
                    target,
                    ordinal,
                    snapshot,
                    ..
                } => (target, ordinal, Some(snapshot)),
                ParseResult::Acknowledged {
                    target, ordinal, ..
                } => (target, ordinal, None),
                ParseResult::Error { target, message } => {
                    panic!(
                        "unexpected parser error for {:?}: {message}",
                        target.pane_id
                    )
                }
                _ => panic!("unexpected separate payload in publication fixture"),
            };
            let (_, view) = panes
                .iter_mut()
                .find(|(id, _)| *id == target.pane_id)
                .expect("every collected pane must be installed");
            assert!(view.accepts_parser_target(&target));
            assert!(
                ordinal > view.applied_ordinal,
                "result ordinal moved backward"
            );
            if let Some(snapshot) = snapshot {
                view.install(snapshot.clone(), ordinal);
                installed.push(snapshot);
            } else {
                view.applied_ordinal = ordinal;
            }
        }
        installed
    }
    fn wait_installed(parsing: &TerminalParsing, panes: &mut [(NodeId, TerminalView)]) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            drop(drain_and_install(parsing, panes));
            if panes.iter().all(|(_, view)| view.can_evict_parser())
                && parsing.pending_work() == Some((0, 0))
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "not every admitted ordinal was installed"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn wait_for_pressure(parsing: &TerminalParsing, view: &TerminalView, expected: ParserPressure) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut reported = false;
        loop {
            for result in parsing.collect() {
                match result {
                    ParseResult::Error { target, message } => {
                        assert!(view.accepts_parser_target(&target));
                        assert!(!message.is_empty());
                        assert_eq!(view.parser_pressure(), Some(expected));
                        reported = true;
                    }
                    _ => panic!("blocked command produced an unexpected result"),
                }
            }
            if reported && view.parser_pressure() == Some(expected) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "typed pressure was never reported"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn wait_for_storage(parsing: &TerminalParsing, claims: usize, bytes: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                parsing.collect().is_empty(),
                "unconsumed result during retirement"
            );
            if parsing.engine_claims() == claims
                && parsing.shared.storage.snapshot().worker_bytes == bytes
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "final storage owners did not retire"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn osc_pressure_retains_original_cursor_and_other_panes_continue() {
        let (_bank, parsing) = Bank::start();
        let mut first = TerminalView::new(4, 40);
        let mut second = TerminalView::new(4, 40);
        attach(&parsing, NodeId(101), &mut first);
        attach(&parsing, NodeId(102), &mut second);
        // Require both engines constructed before forcing a smaller synthetic
        // admission limit; real resident/storage declarations stay unchanged.
        offer(
            &mut first,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"ready".to_vec(),
                track: false,
            },
        );
        result(
            &parsing,
            |result| matches!(result, ParseResult::Published { target, snapshot, .. } if target.pane_id == NodeId(101) && snapshot.sequence == 1),
        );
        offer(
            &mut second,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"ready".to_vec(),
                track: false,
            },
        );
        result(
            &parsing,
            |result| matches!(result, ParseResult::Published { target, snapshot, .. } if target.pane_id == NodeId(102) && snapshot.sequence == 1),
        );
        parsing.shared.state_limit.store(MIB, Ordering::Release);
        let mut bytes = b"\x1b]0;".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', 512 * 1024));
        bytes.push(7);
        let expected_bytes = bytes.clone();
        offer(
            &mut first,
            PaneCommand::Output {
                first: 2,
                sequence: 2,
                bytes,
                track: false,
            },
        );
        result(
            &parsing,
            |result| matches!(result, ParseResult::Error { target, message } if target.pane_id == NodeId(101) && message.contains("cursor")),
        );
        offer(
            &mut first,
            PaneCommand::Output {
                first: 3,
                sequence: 3,
                bytes: b"after".to_vec(),
                track: false,
            },
        );
        offer(
            &mut second,
            PaneCommand::Output {
                first: 2,
                sequence: 2,
                bytes: b"independent pane".to_vec(),
                track: false,
            },
        );
        let independent = result(
            &parsing,
            |result| matches!(result, ParseResult::Published { target, snapshot, .. } if target.pane_id == NodeId(102) && snapshot.sequence == 2),
        );
        let ParseResult::Published { snapshot, .. } = independent else {
            unreachable!()
        };
        assert!(snapshot.visible.contents().contains("independent pane"));
        let (pending, bytes) = parsing.pending_work().unwrap();
        assert!(pending >= 2);
        assert!(bytes >= expected_bytes.len());
        parsing
            .shared
            .state_limit
            .store(MAX_STATE_BYTES, Ordering::Release);
        parsing.shared.changed.notify_one();
        let resumed = result(
            &parsing,
            |result| matches!(result, ParseResult::Published { target, snapshot, .. } if target.pane_id == NodeId(101) && snapshot.sequence == 3),
        );
        let ParseResult::Published { snapshot, .. } = resumed else {
            unreachable!()
        };
        let mut expected = b"ready".to_vec();
        expected.extend_from_slice(&expected_bytes);
        expected.extend_from_slice(b"after");
        assert_eq!(snapshot.history.to_vec(), expected);
        assert!(snapshot.visible.contents().contains("after"));
    }
    #[test]
    fn confirmed_tree_absence_releases_only_suspended_instance_without_reordering_payloads() {
        let (_bank, parsing) = Bank::start();
        let mut app =
            crate::app::App::new("fixture".into(), std::path::PathBuf::from("/synthetic"));
        let group_id = app
            .tree
            .add_group(ilium_core::ROOT_ID, "terminals")
            .unwrap();
        let first_id = app
            .tree
            .add_pane(group_id, "first", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let second_id = app
            .tree
            .add_pane(group_id, "second", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let mut first = TerminalView::new(4, 40);
        let mut second = TerminalView::new(4, 40);
        attach(&parsing, first_id, &mut first);
        attach(&parsing, second_id, &mut second);
        offer(
            &mut first,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"ready".to_vec(),
                track: false,
            },
        );
        result(
            &parsing,
            |result| matches!(result, ParseResult::Published { target, snapshot, .. } if target.pane_id == first_id && snapshot.sequence == 1),
        );
        offer(
            &mut second,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"ready".to_vec(),
                track: false,
            },
        );
        result(
            &parsing,
            |result| matches!(result, ParseResult::Published { target, snapshot, .. } if target.pane_id == second_id && snapshot.sequence == 1),
        );
        parsing.shared.state_limit.store(MIB, Ordering::Release);
        let mut source = b"\x1b]0;".to_vec();
        source.extend(std::iter::repeat_n(b'x', 512 * 1024));
        offer(
            &mut first,
            PaneCommand::Output {
                first: 2,
                sequence: 2,
                bytes: source,
                track: false,
            },
        );
        result(
            &parsing,
            |result| matches!(result, ParseResult::Error { target, message } if target.pane_id == first_id && message.contains("cursor")),
        );
        assert!(first.has_suspended_output());
        let identity = first.identity.clone();
        assert!(!first.confirm_removed(&Arc::new(())));
        app.panes
            .insert(first_id, crate::app::PaneRuntime::Terminal(Box::new(first)));
        app.panes.insert(
            second_id,
            crate::app::PaneRuntime::Terminal(Box::new(second)),
        );
        let bytes = b"original pending wire output".to_vec();
        let pointer = bytes.as_ptr();
        app.pending_terminal_events
            .push_back(crate::connection::Received::with_retention(
                ilium_ipc::ServerEvent::ScreenUpdate {
                    pane_id: first_id,
                    first_sequence: 3,
                    sequence: 3,
                    bytes,
                },
                None,
            ));
        let mut removal = app.tree.clone();
        removal.remove_node(first_id).unwrap();
        app.pending_terminal_events
            .push_back(crate::connection::Received::with_retention(
                ilium_ipc::ServerEvent::TreeSnapshot(removal),
                None,
            ));
        assert!(crate::confirm_suspended_panes_from_pending_snapshots(
            &mut app
        ));
        assert!(!crate::confirm_suspended_panes_from_pending_snapshots(
            &mut app
        ));
        assert!(app.tree.get(first_id).is_some());
        assert_eq!(app.pending_terminal_events.len(), 2);
        let ilium_ipc::ServerEvent::ScreenUpdate { bytes, .. } =
            app.pending_terminal_events.front().unwrap().view()
        else {
            panic!("original FIFO head changed");
        };
        assert_eq!(bytes.as_ptr(), pointer);
        let Some(crate::app::PaneRuntime::Terminal(first)) = app.panes.get(&first_id) else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(&first.identity, &identity));
        assert!(first.is_confirmed_removed());
        let Some(crate::app::PaneRuntime::Terminal(second)) = app.panes.get(&second_id) else {
            unreachable!()
        };
        assert!(!second.is_confirmed_removed());
        let audit = result(
            &parsing,
            |result| matches!(result, ParseResult::Error { target, message } if target.pane_id == first_id && message.contains("ordinal") && message.contains("bytes")),
        );
        assert!(matches!(audit, ParseResult::Error { .. }));
    }
    #[test]
    fn search_navigation_checks_history_origin_after_previously_admitted_replay() {
        let (_bank, parsing) = Bank::start();
        let mut view = TerminalView::new(3, 20);
        attach(&parsing, NodeId(91), &mut view);
        offer(
            &mut view,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"old needle\r\ntail".to_vec(),
                track: false,
            },
        );
        let published = result(
            &parsing,
            |result| matches!(result, ParseResult::Published { snapshot, .. } if snapshot.sequence == 1),
        );
        let ParseResult::Published { snapshot, .. } = published else {
            unreachable!()
        };
        let origin = Arc::clone(&snapshot.history.origin);
        offer(
            &mut view,
            PaneCommand::Replay {
                sequence: 2,
                bytes: b"new journal".to_vec(),
                complete: true,
            },
        );
        offer(&mut view, PaneCommand::HistoryFenced { byte: 10, origin });
        let failure = result(
            &parsing,
            |result| matches!(result, ParseResult::Error { message, .. } if message.contains("Search history origin changed")),
        );
        assert!(matches!(failure, ParseResult::Error { .. }));
        offer(&mut view, PaneCommand::Capture { generation: 92 });
        let captured = result(&parsing, |result| {
            matches!(result, ParseResult::Capture { generation: 92, .. })
        });
        let ParseResult::Capture { snapshot, .. } = captured else {
            unreachable!()
        };
        assert!(snapshot.screen.contents().contains("new journal"));
        assert!(!snapshot.screen.contents().contains("old needle"));
    }
    #[test]
    fn actual_emitted_lease_retains_live_credit_until_last_ack_source_release() {
        let (_bank, parsing) = Bank::start();
        let limit = parsing.shared.snapshot_limit.load(Ordering::Acquire);
        assert_eq!(limit, 512 * MIB);
        let first =
            Arc::new(SnapshotCharge::reserve(&parsing.shared, MAX_PIN_BYTES, false).unwrap());
        let raw = first.clone();
        let queued = first.lease_live();
        let emitted = queued.clone();
        assert!(first._storage.shares_root(&parsing.shared.storage));
        assert_eq!(first._storage.resident_bytes(), MAX_PIN_BYTES);
        first.retire_live();
        first.retire_live();

        let filler =
            SnapshotCharge::reserve(&parsing.shared, limit - first.bytes(), false).unwrap();
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), limit);
        assert_eq!(parsing.shared.storage.snapshot().worker_bytes, limit);
        assert!(SnapshotCharge::reserve(&parsing.shared, 1, false).is_none());
        drop(queued);
        drop(emitted);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), limit);
        assert_eq!(parsing.shared.storage.snapshot().worker_bytes, limit);
        drop(first);
        assert!(SnapshotCharge::reserve(&parsing.shared, 1, false).is_none());
        drop(raw);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            limit - MAX_PIN_BYTES
        );
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            limit - MAX_PIN_BYTES
        );
        let replacement = SnapshotCharge::reserve(&parsing.shared, MAX_PIN_BYTES, false).unwrap();
        replacement.retire_live();
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), limit);
        drop(replacement);
        drop(filler);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
        assert_eq!(parsing.shared.storage.snapshot().worker_bytes, 0);
    }

    #[test]
    fn admitted_distinct_pins_share_one_debit_and_leave_publication_headroom() {
        let (_bank, parsing) = Bank::start();
        let first =
            Arc::new(SnapshotCharge::reserve(&parsing.shared, MAX_PIN_BYTES, false).unwrap());
        let pin = first.pin().unwrap();
        let cloned = first.pin().unwrap();
        assert!(Arc::ptr_eq(&pin, &cloned));
        assert_eq!(
            parsing.shared.pin_bytes.load(Ordering::Acquire),
            MAX_PIN_BYTES
        );
        first.retire_live();
        drop(first);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            MAX_PIN_BYTES
        );
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            MAX_PIN_BYTES
        );
        let next =
            Arc::new(SnapshotCharge::reserve(&parsing.shared, MAX_STATE_BYTES, false).unwrap());
        let other_current =
            SnapshotCharge::reserve(&parsing.shared, MAX_STATE_BYTES, false).unwrap();
        let replacement = SnapshotCharge::reserve(&parsing.shared, MAX_STATE_BYTES, false).unwrap();
        let limit = parsing.shared.snapshot_limit.load(Ordering::Acquire);
        assert_eq!(limit, 512 * MIB);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), limit);
        assert_eq!(parsing.shared.storage.snapshot().worker_bytes, limit);
        assert!(next.pin().is_err());
        assert!(SnapshotCharge::reserve(&parsing.shared, 1, false).is_none());
        drop(pin);
        assert_eq!(
            parsing.shared.pin_bytes.load(Ordering::Acquire),
            MAX_PIN_BYTES
        );
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), limit);
        drop(cloned);
        assert_eq!(parsing.shared.pin_bytes.load(Ordering::Acquire), 0);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            limit - MAX_PIN_BYTES
        );
        let next_pin = next.pin().unwrap();
        assert_eq!(
            parsing.shared.pin_bytes.load(Ordering::Acquire),
            MAX_STATE_BYTES
        );
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            limit - MAX_PIN_BYTES
        );
        next.retire_live();
        drop(next);
        drop(other_current);
        drop(replacement);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            MAX_STATE_BYTES
        );
        drop(next_pin);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
        assert_eq!(parsing.shared.pin_bytes.load(Ordering::Acquire), 0);
        assert_eq!(parsing.shared.storage.snapshot().worker_bytes, 0);
    }

    #[test]
    fn capture_category_rejection_does_not_wait_or_consume_live_headroom() {
        let (_bank, parsing) = Bank::start();
        let source =
            Arc::new(SnapshotCharge::reserve(&parsing.shared, MAX_STATE_BYTES, false).unwrap());
        assert!(source.reserve_capture(MAX_CAPTURE_BYTES + 1).is_err());
        assert_eq!(parsing.shared.capture_bytes.load(Ordering::Acquire), 0);
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            MAX_STATE_BYTES
        );
        let capture = source.reserve_capture(MAX_CAPTURE_BYTES).unwrap();
        assert!(!Arc::ptr_eq(&source._storage, &capture._storage));
        assert!(capture._storage.shares_root(&parsing.shared.storage));
        assert_eq!(capture._storage.resident_bytes(), MAX_CAPTURE_BYTES);
        assert!(source.reserve_capture(1).is_err());
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            MAX_STATE_BYTES
        );
        assert_eq!(
            parsing.shared.capture_bytes.load(Ordering::Acquire),
            MAX_CAPTURE_BYTES
        );
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            MAX_STATE_BYTES + MAX_CAPTURE_BYTES
        );
        source.retire_live();
        drop(source);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            MAX_CAPTURE_BYTES
        );
        let retained = capture.clone();
        drop(capture);
        assert_eq!(
            parsing.shared.capture_bytes.load(Ordering::Acquire),
            MAX_CAPTURE_BYTES
        );
        drop(retained);
        assert_eq!(parsing.shared.capture_bytes.load(Ordering::Acquire), 0);
        assert_eq!(parsing.shared.storage.snapshot().worker_bytes, 0);
    }

    #[test]
    fn every_split_escape_byte_is_preserved_and_modes_follow_the_barrier() {
        let (_bank, parsing) = Bank::start();
        let mut view = TerminalView::new(4, 30);
        attach(&parsing, NodeId(1), &mut view);
        let bytes = b"hello\x1b[31m red\x1b[0m\x1b[?2004h\x1b[?1000h";
        for (index, byte) in bytes.iter().enumerate() {
            offer(
                &mut view,
                PaneCommand::Output {
                    first: index as u64 + 1,
                    sequence: index as u64 + 1,
                    bytes: vec![*byte],
                    track: false,
                },
            );
        }
        offer(&mut view, PaneCommand::InputBarrier { generation: 7 });
        let ack = result(&parsing, |result| {
            matches!(result, ParseResult::InputBarrier { generation: 7, .. })
        });
        assert!(
            matches!(ack,ParseResult::InputBarrier {sequence,mouse:true,paste:true,..} if sequence==bytes.len() as u64)
        );
        offer(&mut view, PaneCommand::Capture { generation: 8 });
        let capture = result(&parsing, |result| {
            matches!(result, ParseResult::Capture { generation: 8, .. })
        });
        let ParseResult::Capture { snapshot, .. } = capture else {
            unreachable!()
        };
        assert!(snapshot.screen.contents().contains("hello red"));
    }
    #[test]
    fn replay_resize_and_historical_capture_keep_exact_order() {
        let (_bank, parsing) = Bank::start();
        let mut view = TerminalView::new(3, 20);
        attach(&parsing, NodeId(2), &mut view);
        offer(
            &mut view,
            PaneCommand::Replay {
                sequence: 40,
                bytes: b"old\r\nanchor\r\ntail".to_vec(),
                complete: true,
            },
        );
        offer(&mut view, PaneCommand::Resize(4, 24));
        offer(&mut view, PaneCommand::History(b"old\r\nanchor".len()));
        offer(&mut view, PaneCommand::Capture { generation: 11 });
        offer(
            &mut view,
            PaneCommand::Output {
                first: 41,
                sequence: 41,
                bytes: b"\r\nnewest".to_vec(),
                track: false,
            },
        );
        let capture = result(&parsing, |result| {
            matches!(result, ParseResult::Capture { generation: 11, .. })
        });
        let ParseResult::Capture { snapshot, .. } = capture else {
            unreachable!()
        };
        assert!(snapshot.screen.contents().contains("anchor"));
        assert!(!snapshot.screen.contents().contains("newest"));
        assert_eq!(snapshot.screen.size(), (4, 24));
    }

    #[test]
    fn large_replay_renders_a_progress_snapshot_before_its_ordinal_completes() {
        let (_bank, parsing) = Bank::start();
        let mut view = TerminalView::new(4, 30);
        attach(&parsing, NodeId(4), &mut view);
        let mut bytes = vec![b'x'; 512 * 1024];
        offer(
            &mut view,
            PaneCommand::Replay {
                sequence: 40,
                bytes,
                complete: true,
            },
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut rendered_while_pending = false;
        while Instant::now() < deadline {
            let pending = parsing.pending_work().unwrap_or_default();
            if pending.0 > 0 && view.has_initial_display() {
                assert_eq!(view.applied_ordinal, 0);
                assert!(!view
                    .with_screen(|screen| screen.contents())
                    .trim()
                    .is_empty());
                rendered_while_pending = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            rendered_while_pending,
            "visible replay content stayed blank until the entire journal completed"
        );

        let published = result(&parsing, |result| {
            matches!(result, ParseResult::Published { ordinal: 2, snapshot, .. }
                if snapshot.sequence == 40 && !snapshot.visible.contents().trim().is_empty())
        });
        let ParseResult::Published {
            target,
            ordinal,
            snapshot,
            ..
        } = published
        else {
            unreachable!()
        };
        assert!(view.accepts_parser_target(&target));
        view.install(snapshot, ordinal);
        assert!(view.can_evict_parser());
        assert!(!view
            .with_screen(|screen| screen.contents())
            .trim()
            .is_empty());
    }
    #[test]
    fn replacing_a_pane_never_reuses_the_old_engine_instance() {
        let (_bank, parsing) = Bank::start();
        let mut old = TerminalView::new(3, 20);
        attach(&parsing, NodeId(3), &mut old);
        let old_identity = old.identity.clone();
        offer(
            &mut old,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"old".to_vec(),
                track: false,
            },
        );
        drop(old);
        let mut new = TerminalView::new(3, 20);
        attach(&parsing, NodeId(3), &mut new);
        offer(
            &mut new,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"new".to_vec(),
                track: false,
            },
        );
        offer(&mut new, PaneCommand::Capture { generation: 14 });
        let capture = result(&parsing, |result| {
            matches!(result, ParseResult::Capture { generation: 14, .. })
        });
        let ParseResult::Capture {
            snapshot, target, ..
        } = capture
        else {
            unreachable!()
        };
        assert!(!Arc::ptr_eq(&old_identity, &target.identity));
        assert!(snapshot.screen.contents().contains("new"));
        assert!(!snapshot.screen.contents().contains("old"));
    }
    #[test]
    fn busy_admission_returns_original_bytes_without_waiting() {
        let (_bank, parsing) = Bank::start();
        let mut view = TerminalView::new(3, 20);
        attach(&parsing, NodeId(4), &mut view);
        let lock = parsing.shared.queue.lock().unwrap();
        let start = Instant::now();
        let rejected = view.frontend.as_mut().unwrap().submit(PaneCommand::Output {
            first: 1,
            sequence: 1,
            bytes: b"retained".to_vec(),
            track: false,
        });
        assert!(start.elapsed() < Duration::from_millis(50));
        assert!(matches!(rejected,Err(PaneCommand::Output {bytes,..}) if bytes==b"retained"));
        drop(lock);
    }
    #[test]
    fn retained_history_keeps_its_snapshot_allocation_charged() {
        let (_bank, parsing) = Bank::start();
        let mut panes = [(NodeId(5), TerminalView::new(3, 20))];
        attach(&parsing, panes[0].0, &mut panes[0].1);
        offer(
            &mut panes[0].1,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"retained history".to_vec(),
                track: false,
            },
        );
        wait_installed(&parsing, &mut panes);
        let queued = panes[0].1.painted_source();
        let emitted = queued.clone();
        let pin = queued.pinned().unwrap();
        let prepared = panes[0].1.try_preparation_snapshot().unwrap();
        let raw_history = prepared.history.clone();
        let raw_charge = prepared.allocation_charge.as_ref().unwrap().clone();
        let history = panes[0].1.try_searchable_history_snapshot().unwrap();
        let old_bytes = queued.allocation_bytes();
        let capture_bytes = queued.capture_cost().unwrap().input_bytes;
        let capture_charge = queued.capture_charge(capture_bytes).unwrap().unwrap();
        assert!(!Arc::ptr_eq(&raw_charge._storage, &capture_charge._storage));
        let mut capture = queued.with_screen(crate::smart_copy::SmartCopySnapshot::capture);
        capture.retain_allocation(capture_charge);
        let capture = Arc::new(capture);
        drop(prepared);
        offer(
            &mut panes[0].1,
            PaneCommand::Output {
                first: 2,
                sequence: 2,
                bytes: b"\r\nreplacement".to_vec(),
                track: false,
            },
        );
        wait_installed(&parsing, &mut panes);
        let current_bytes = panes[0].1.painted_source().allocation_bytes();
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            old_bytes + current_bytes
        );
        assert_eq!(parsing.shared.pin_bytes.load(Ordering::Acquire), old_bytes);
        assert_eq!(
            parsing.shared.capture_bytes.load(Ordering::Acquire),
            capture_bytes
        );
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            MAX_STATE_BYTES + old_bytes + current_bytes + capture_bytes
        );
        drop(queued);
        assert_eq!(
            emitted.with_screen(|screen| screen.contents()),
            "retained history"
        );
        drop(emitted);
        drop(pin);
        assert_eq!(parsing.shared.pin_bytes.load(Ordering::Acquire), old_bytes);
        assert_eq!(history.to_vec(), b"retained history");
        drop(history);
        assert_eq!(parsing.shared.pin_bytes.load(Ordering::Acquire), 0);
        assert_eq!(raw_history.to_vec(), b"retained history");
        drop(raw_history);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            old_bytes + current_bytes
        );
        drop(raw_charge);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            current_bytes
        );
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            MAX_STATE_BYTES + current_bytes + capture_bytes
        );
        assert!(panes[0].1.evict_parser());
        wait_for_storage(&parsing, 0, capture_bytes);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
        assert_eq!(capture.screen.contents(), "retained history");
        let retained_capture = capture.clone();
        drop(capture);
        assert_eq!(
            parsing.shared.capture_bytes.load(Ordering::Acquire),
            capture_bytes
        );
        drop(retained_capture);
        assert_eq!(parsing.shared.capture_bytes.load(Ordering::Acquire), 0);
        assert_eq!(parsing.shared.storage.snapshot().worker_bytes, 0);
    }

    #[test]
    fn raw_published_snapshot_outlives_frontend_and_engine_retirement() {
        let (_bank, parsing) = Bank::start();
        let mut panes = [(NodeId(8), TerminalView::new(3, 20))];
        attach(&parsing, panes[0].0, &mut panes[0].1);
        offer(
            &mut panes[0].1,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"raw snapshot owner".to_vec(),
                track: false,
            },
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut raw = None;
        loop {
            for snapshot in drain_and_install(&parsing, &mut panes) {
                if snapshot.sequence == 1 {
                    assert!(raw.is_none(), "one output ordinal published twice");
                    raw = Some(snapshot);
                }
            }
            if raw.is_some()
                && panes[0].1.can_evict_parser()
                && parsing.pending_work() == Some((0, 0))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "actual raw snapshot did not arrive"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let raw = raw.unwrap();
        let bytes = raw.allocation_charge.as_ref().unwrap().bytes();
        assert!(panes[0].1.evict_parser());
        wait_for_storage(&parsing, 0, bytes);
        assert_eq!(parsing.shared.pin_bytes.load(Ordering::Acquire), 0);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), bytes);
        assert_eq!(raw.visible.contents(), "raw snapshot owner");
        assert_eq!(raw.history.to_vec(), b"raw snapshot owner");
        drop(raw);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
        assert_eq!(parsing.shared.storage.snapshot().worker_bytes, 0);
    }

    #[test]
    fn blocked_publication_keeps_producer_nonblocking_and_shutdown_reports_cancellation() {
        let (_bank, mut parsing) = Bank::start();
        let limit = parsing.shared.snapshot_limit.load(Ordering::Acquire);
        assert_eq!(limit, 512 * MIB);
        let hold = SnapshotCharge::reserve(&parsing.shared, limit, false).unwrap();
        let mut view = TerminalView::new(3, 20);
        attach(&parsing, NodeId(6), &mut view);
        wait_for_pressure(&parsing, &view, ParserPressure::SnapshotPool);
        assert!(!view.can_evict_parser());
        let mut command = PaneCommand::Output {
            first: 1,
            sequence: 1,
            bytes: b"owned while blocked".to_vec(),
            track: false,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let start = Instant::now();
            let offered = view.frontend.as_mut().unwrap().submit(command);
            assert!(
                start.elapsed() < Duration::from_millis(50),
                "producer must return promptly while the owner is backpressured"
            );
            match offered {
                Ok(_) => break,
                Err(original) => command = original,
            }
            assert!(
                Instant::now() < deadline,
                "retained output was never admitted"
            );
            std::thread::yield_now();
        }
        let (commands, bytes) = loop {
            if let Some(work) = parsing.pending_work() {
                break work;
            }
            assert!(
                Instant::now() < deadline,
                "pending command accounting remained busy"
            );
            std::thread::yield_now();
        };
        assert_eq!(commands, 2);
        assert!(bytes >= b"owned while blocked".len());
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), limit);
        parsing.cancel();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(error) = parsing.failure() {
                assert!(error.contains("cancel"));
                assert!(error.contains(
                    format!("{commands} admitted commands ({bytes} retained bytes)").as_str()
                ));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "shutdown failed to report retained commands"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(view);
        drop(hold);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
        let storage = parsing.shared.storage.clone();
        let claims = parsing.shared.engine_claims.clone();
        assert_eq!(storage.snapshot().worker_bytes, MAX_STATE_BYTES);

        drop(parsing);
        assert_eq!(claims.load(Ordering::Acquire), 0);
        assert_eq!(storage.snapshot().worker_bytes, 0);
    }

    #[test]
    fn snapshot_pressure_does_not_block_another_panes_input_barrier() {
        let (_bank, parsing) = Bank::start();
        let mut first = TerminalView::new(3, 20);
        let mut second = TerminalView::new(3, 20);
        attach(&parsing, NodeId(201), &mut first);
        attach(&parsing, NodeId(202), &mut second);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            drop(parsing.collect());
            if parsing.pending_work() == Some((0, 0)) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "initial registration did not finish"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(parsing.collect());
        let hold = SnapshotCharge::reserve(
            &parsing.shared,
            parsing.shared.snapshot_limit.load(Ordering::Acquire),
            false,
        )
        .expect("all initial publications were released");
        offer(
            &mut first,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"publication waits for capacity".to_vec(),
                track: false,
            },
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if parsing.collect().iter().any(|result| {
                matches!(result, ParseResult::Error { target, message }
                    if target.pane_id == NodeId(201) && message.contains("admission blocked"))
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "snapshot pressure was not observed"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        offer(&mut second, PaneCommand::InputBarrier { generation: 77 });
        let deadline = Instant::now() + Duration::from_secs(2);
        let progressed = loop {
            if parsing.collect().iter().any(|result| {
                matches!(result, ParseResult::InputBarrier { target, generation: 77, .. }
                    if target.pane_id == NodeId(202))
            }) {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        // Release real retained storage before any assertion can unwind.
        drop(hold);
        assert!(
            progressed,
            "snapshot pressure on one pane blocked the sole parser owner from serving another pane"
        );
    }

    #[test]
    fn snapshot_pressure_on_large_pane_keeps_small_pane_publishing_visible_content() {
        let (_bank, parsing) = Bank::start();
        let mut first = TerminalView::new(24, 80);
        let mut second = TerminalView::new(3, 20);
        let first_id = NodeId(203);
        let second_id = NodeId(204);
        attach(&parsing, first_id, &mut first);
        attach(&parsing, second_id, &mut second);
        let mut panes = [(first_id, first), (second_id, second)];
        wait_installed(&parsing, &mut panes);
        panes[1]
            .1
            .frontend
            .as_ref()
            .expect("small pane parser is attached")
            .set_displayed(true);
        assert!(panes[1]
            .1
            .frontend
            .as_ref()
            .unwrap()
            .target
            .displayed
            .load(Ordering::Acquire));

        let first_bytes = panes[0].1.painted_source().allocation_bytes();
        let second_bytes = panes[1].1.painted_source().allocation_bytes();
        assert!(
            first_bytes > second_bytes + 16 * 1024,
            "large-pane generation must exceed the small-pane generation"
        );
        let retained = parsing.shared.snapshot_bytes.load(Ordering::Acquire);
        parsing
            .shared
            .snapshot_limit
            .store(retained + second_bytes + 16 * 1024, Ordering::Release);

        offer(
            &mut panes[0].1,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"large pane waits".to_vec(),
                track: false,
            },
        );
        wait_for_pressure(&parsing, &panes[0].1, ParserPressure::SnapshotPool);
        assert_eq!(panes[0].1.last_output_sequence(), 0);

        offer(
            &mut panes[1].1,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"small pane remains visible".to_vec(),
                track: false,
            },
        );
        let published = result(&parsing, |result| {
            matches!(result, ParseResult::Published { target, snapshot, .. }
                if target.pane_id == second_id
                    && snapshot.sequence == 1
                    && snapshot.visible.contents().contains("small pane remains visible"))
        });
        let ParseResult::Published {
            target,
            ordinal,
            snapshot,
            ..
        } = published
        else {
            unreachable!()
        };
        panes[1].1.install(snapshot, ordinal);
        assert!(panes[0].1.parser_pressure() == Some(ParserPressure::SnapshotPool));
        assert_eq!(panes[0].1.last_output_sequence(), 0);
        assert_eq!(panes[1].1.last_output_sequence(), 1);
        assert!(panes[1]
            .1
            .with_screen(|screen| screen.contents().contains("small pane remains visible")));

        parsing.set_budget_mib(0);
        wait_installed(&parsing, &mut panes);
        assert_eq!(panes[0].1.last_output_sequence(), 1);
        assert_eq!(
            panes[0].1.parser_pressure(),
            None,
            "the original pane must recover after capacity is restored"
        );
    }

    #[test]
    fn retiring_an_engine_wakes_the_client_loop() {
        let (_bank, parsing) = Bank::start();
        let mut panes = [(NodeId(1), TerminalView::new(24, 80))];
        attach(&parsing, panes[0].0, &mut panes[0].1);
        assert!(
            !panes[0].1.evict_parser(),
            "Register has not been consumed yet"
        );
        wait_installed(&parsing, &mut panes);
        assert_eq!(panes[0].1.with_screen(|screen| screen.size()), (24, 80));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let notification = parsing.notification();
        runtime.block_on(async {
            while tokio::time::timeout(Duration::from_millis(300), notification.notified())
                .await
                .is_ok()
            {}
        });
        assert!(panes[0].1.evict_parser());
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), notification.notified())
                .await
                .expect("an idle parser must wake the client when it releases an engine");
        });
        assert_eq!(parsing.engine_claims(), 0);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
        assert_eq!(parsing.shared.storage.snapshot().worker_bytes, 0);
    }

    #[test]
    fn default_unpooled_parser_admits_128_panes_with_distinct_contents_and_revisits() {
        let (_bank, parsing) =
            Bank::start_with_storage(0, crate::execution::terminal_storage_quota());
        assert!(!parsing.pool_enabled());
        let mut panes = Vec::with_capacity(128);
        for index in 0..128 {
            let id = NodeId(index as u64 + 1);
            let mut view = TerminalView::new(24, 80);
            attach(&parsing, id, &mut view);
            offer(
                &mut view,
                PaneCommand::Output {
                    first: 1,
                    sequence: 1,
                    bytes: format!("pane-{index:03} ready").into_bytes(),
                    track: false,
                },
            );
            panes.push((id, view));
        }
        // Admit the full 128-pane registration/output burst before waiting so
        // this exercises queued parser work instead of serial pane visits.
        wait_installed(&parsing, &mut panes);
        let original_sources: Vec<_> = panes
            .iter()
            .map(|(_, view)| view.painted_source())
            .collect();
        let original_bytes: usize = original_sources
            .iter()
            .map(|source| source.allocation_bytes())
            .sum();
        let incarnations: Vec<_> = panes
            .iter()
            .map(|(_, view)| view.frontend.as_ref().unwrap().target.alive.clone())
            .collect();
        assert_eq!(parsing.engine_claims(), 128);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            original_bytes
        );
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            128 * MAX_STATE_BYTES + original_bytes
        );
        for (index, (_, view)) in panes.iter_mut().enumerate() {
            assert_eq!(view.last_output_sequence(), 1);
            assert!(view
                .with_screen(|screen| screen.contents())
                .contains(format!("pane-{index:03} ready").as_str()));
            offer(
                view,
                PaneCommand::Output {
                    first: 2,
                    sequence: 2,
                    bytes: format!("\r\nrevisit-{index:03}").into_bytes(),
                    track: false,
                },
            );
        }
        wait_installed(&parsing, &mut panes);
        let current_bytes: usize = panes
            .iter()
            .map(|(_, view)| view.painted_source().allocation_bytes())
            .sum();
        for (index, (_, view)) in panes.iter().enumerate().rev() {
            assert_eq!(view.applied_ordinal, 3);
            assert_eq!(view.last_output_sequence(), 2);
            assert_eq!(view.with_screen(|screen| screen.size()), (24, 80));
            assert!(view
                .with_screen(|screen| screen.contents())
                .contains(format!("revisit-{index:03}").as_str()));
            assert_eq!(
                view.try_searchable_history_snapshot().unwrap().to_vec(),
                format!("pane-{index:03} ready\r\nrevisit-{index:03}").into_bytes()
            );
            assert!(Arc::ptr_eq(
                &incarnations[index],
                &view.frontend.as_ref().unwrap().target.alive
            ));
            assert!(view.parser_pressure().is_none());
            assert_eq!(
                original_sources[index].with_screen(|screen| screen.contents()),
                format!("pane-{index:03} ready")
            );
        }
        assert_eq!(parsing.engine_claims(), 128);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            original_bytes + current_bytes
        );
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            128 * MAX_STATE_BYTES + original_bytes + current_bytes
        );
        drop(original_sources);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            current_bytes
        );
        drop(panes);
        wait_for_storage(&parsing, 0, 0);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
    }

    #[test]
    fn an_evicted_hidden_pane_releases_its_engine_and_wakes_the_loop() {
        let (_bank, parsing) = Bank::start();
        let mut hidden = [(NodeId(1), TerminalView::new(24, 80))];
        attach(&parsing, hidden[0].0, &mut hidden[0].1);
        wait_installed(&parsing, &mut hidden);
        let identity = hidden[0].1.identity.clone();
        let initial = geometry_peak(24, 80);
        let resident = TerminalState::new(24, 80).retained_allocation_bytes();
        assert!(initial <= 512 * 1024);
        assert!(initial + resident > 512 * 1024);
        parsing
            .shared
            .state_limit
            .store(512 * 1024, Ordering::Release);
        let mut waiting = [(NodeId(2), TerminalView::new(24, 80))];
        attach(&parsing, waiting[0].0, &mut waiting[0].1);
        wait_for_pressure(&parsing, &waiting[0].1, ParserPressure::StatePool);
        assert!(!waiting[0].1.evict_parser());
        assert!(hidden[0].1.evict_parser());
        assert!(hidden[0].1.frontend.is_none());
        assert!(Arc::ptr_eq(&identity, &hidden[0].1.identity));
        assert!(!hidden[0].1.evict_parser());
        wait_installed(&parsing, &mut waiting);
        offer(
            &mut waiting[0].1,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"replacement owner".to_vec(),
                track: false,
            },
        );
        wait_installed(&parsing, &mut waiting);
        assert_eq!(waiting[0].1.last_output_sequence(), 1);
        assert_eq!(
            waiting[0].1.with_screen(|screen| screen.contents()),
            "replacement owner"
        );
        assert!(waiting[0].1.parser_pressure().is_none());
        assert_eq!(parsing.engine_claims(), 1);
        let current_bytes = waiting[0].1.painted_source().allocation_bytes();
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            current_bytes
        );
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            MAX_STATE_BYTES + current_bytes
        );
        drop(waiting);
        wait_for_storage(&parsing, 0, 0);
    }

    #[test]
    fn budget_changes_apply_live_without_replacing_retained_custody() {
        let (_bank, parsing) = Bank::start_with_budget(0);
        assert!(!parsing.pool_enabled());
        assert_eq!(parsing.shared.state_limit.load(Ordering::Acquire), 0);
        assert_eq!(parsing.shared.snapshot_limit.load(Ordering::Acquire), 0);
        let retained_bytes = 3 * MAX_SNAPSHOT_BYTES;
        let retained = SnapshotCharge::reserve(&parsing.shared, retained_bytes, false).unwrap();
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            retained_bytes
        );
        for (budget, expected_snapshot) in [(256, 512), (512, 768)] {
            parsing.set_budget_mib(budget);
            assert!(parsing.pool_enabled());
            assert_eq!(
                parsing.shared.state_limit.load(Ordering::Acquire),
                budget as usize * MIB
            );
            assert_eq!(
                parsing.shared.snapshot_limit.load(Ordering::Acquire),
                expected_snapshot * MIB
            );
            assert_eq!(
                parsing.shared.snapshot_bytes.load(Ordering::Acquire),
                retained_bytes
            );
            assert_eq!(
                parsing.shared.storage.snapshot().worker_bytes,
                retained_bytes
            );
            assert!(SnapshotCharge::reserve(&parsing.shared, 1, false).is_none());
        }
        parsing.set_budget_mib(1024);
        assert_eq!(
            parsing.shared.state_limit.load(Ordering::Acquire),
            1024 * MIB
        );
        assert_eq!(
            parsing.shared.snapshot_limit.load(Ordering::Acquire),
            1280 * MIB
        );
        let additional = SnapshotCharge::reserve(&parsing.shared, MAX_STATE_BYTES, false).unwrap();
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            retained_bytes + MAX_STATE_BYTES
        );
        drop(additional);
        parsing.set_budget_mib(256);
        retained.retire_live();
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            retained_bytes
        );
        assert!(SnapshotCharge::reserve(&parsing.shared, 1, false).is_none());
        parsing.set_budget_mib(0);
        assert!(!parsing.pool_enabled());
        assert_eq!(parsing.shared.snapshot_limit.load(Ordering::Acquire), 0);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            retained_bytes
        );
        let unpooled = SnapshotCharge::reserve(&parsing.shared, MAX_STATE_BYTES, false).unwrap();
        drop(retained);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            MAX_STATE_BYTES
        );
        drop(unpooled);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
        assert_eq!(parsing.shared.storage.snapshot().worker_bytes, 0);
    }

    #[test]
    fn live_off_on_transition_recovers_publication_only_after_policy_or_real_release() {
        let (_bank, parsing) = Bank::start();
        let mut panes = [(NodeId(9), TerminalView::new(3, 20))];
        attach(&parsing, panes[0].0, &mut panes[0].1);
        let mut initial = b"original\x1b]0;".to_vec();
        initial.extend(std::iter::repeat_n(b'x', 256 * 1024));
        initial.push(7);
        offer(
            &mut panes[0].1,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: initial,
                track: false,
            },
        );
        wait_installed(&parsing, &mut panes);
        let original = panes[0].1.painted_source().pinned().unwrap();
        let old_bytes = original.allocation_bytes();
        let limit = parsing.shared.snapshot_limit.load(Ordering::Acquire);
        let filler_bytes = limit - old_bytes;
        let filler = SnapshotCharge::reserve(&parsing.shared, filler_bytes, false).unwrap();
        offer(
            &mut panes[0].1,
            PaneCommand::Replay {
                sequence: 2,
                bytes: b"off resumed".to_vec(),
                complete: true,
            },
        );
        wait_for_pressure(&parsing, &panes[0].1, ParserPressure::SnapshotPool);
        assert_eq!(panes[0].1.last_output_sequence(), 1);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), limit);
        parsing.set_budget_mib(0);
        wait_installed(&parsing, &mut panes);
        let off_bytes = panes[0].1.painted_source().allocation_bytes();
        assert!(
            old_bytes > 4 * off_bytes,
            "retained original must cover two small replacements"
        );
        assert!(!parsing.pool_enabled());
        assert_eq!(panes[0].1.last_output_sequence(), 2);
        assert_eq!(
            panes[0].1.with_screen(|screen| screen.contents()),
            "off resumed"
        );
        assert_eq!(original.with_screen(|screen| screen.contents()), "original");
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            limit + off_bytes
        );
        assert_eq!(parsing.shared.pin_bytes.load(Ordering::Acquire), old_bytes);
        parsing.set_budget_mib(256);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            limit + off_bytes
        );
        offer(
            &mut panes[0].1,
            PaneCommand::Output {
                first: 3,
                sequence: 3,
                bytes: b"\r\npooled resumed".to_vec(),
                track: false,
            },
        );
        wait_for_pressure(&parsing, &panes[0].1, ParserPressure::SnapshotPool);
        assert_eq!(panes[0].1.last_output_sequence(), 2);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            limit + off_bytes
        );
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            MAX_STATE_BYTES + limit + off_bytes
        );
        drop(original);
        wait_installed(&parsing, &mut panes);
        assert!(parsing.pool_enabled());
        assert!(panes[0].1.parser_pressure().is_none());
        assert_eq!(panes[0].1.last_output_sequence(), 3);
        assert_eq!(
            panes[0]
                .1
                .try_searchable_history_snapshot()
                .unwrap()
                .to_vec(),
            b"off resumed\r\npooled resumed"
        );
        let current_bytes = panes[0].1.painted_source().allocation_bytes();
        assert_eq!(parsing.shared.pin_bytes.load(Ordering::Acquire), 0);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            filler_bytes + current_bytes
        );
        assert_eq!(
            parsing.shared.storage.snapshot().worker_bytes,
            MAX_STATE_BYTES + filler_bytes + current_bytes
        );
        drop(filler);
        drop(panes);
        wait_for_storage(&parsing, 0, 0);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
    }

    #[test]
    fn unsupported_geometry_is_rejected_before_desired_size_changes() {
        let mut view = TerminalView::new(3, 20);
        assert!(!view.admit_resize(4096, 4096));
        assert_eq!(view.desired_size, (3, 20));
        assert!(view.admission_error.is_some());
    }
}

struct ExitWake(Arc<Notify>);
impl Drop for ExitWake {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
fn cancellation_report(shared: &Shared) -> Result<(), String> {
    let queue = shared
        .queue
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let count = queue.commands.len() + queue.active_commands;
    let bytes = queue.bytes.saturating_add(queue.active_bytes);
    if count == 0 {
        Ok(())
    } else {
        Err(format!(
            "Terminal parser shutdown cancelled {count} admitted commands ({bytes} retained bytes) before completion"
        ))
    }
}
