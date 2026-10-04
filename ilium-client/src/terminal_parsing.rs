//! One persistent OS owner for ordered terminal streams and immutable results.
use crate::{
    terminal_activity::VisibleTextEvidence,
    terminal_view::{OrderedOutputCursor, TerminalSnapshot, TerminalState, TerminalView},
};
use ilium_core::NodeId;
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, StorageAdmission,
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
const MAX_STATE_BYTES: usize = 128 * MIB;
const MAX_SNAPSHOT_BYTES: usize = 256 * MIB;
const MAX_PIN_BYTES: usize = 128 * MIB;
const MAX_CAPTURE_BYTES: usize = 128 * MIB;
const IMMUTABLE_STORAGE_BYTES: usize = 512 * MIB;
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
    suspended_output: Arc<AtomicBool>,
    confirmed_removed: Arc<AtomicBool>,
    _claim: Arc<EngineClaim>,
}
struct EngineClaim(Arc<AtomicUsize>);
impl Drop for EngineClaim {
    fn drop(&mut self) {
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
    reported_blocked: bool,
}
#[derive(Clone, Copy)]
struct StateDeferral {
    required_bytes: usize,
    impossible: bool,
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
    storage: Arc<StorageAdmission>,
    engine_claims: Arc<AtomicUsize>,
    #[cfg(test)]
    state_limit: AtomicUsize,
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
#[derive(Debug)]
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
    _storage: Arc<StorageAdmission>,
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
            if self.charged.swap(false, Ordering::AcqRel) {
                self.used.fetch_sub(self.bytes, Ordering::AcqRel);
            }
            self.released.notify_all();
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
            _storage: self._storage.clone(),
        }))
    }
    fn reserve(shared: &Shared, bytes: usize, capture: bool) -> Option<Self> {
        let (used, limit) = if capture {
            (&shared.capture_bytes, MAX_CAPTURE_BYTES)
        } else {
            (&shared.snapshot_bytes, MAX_SNAPSHOT_BYTES)
        };
        reserve_counter(used, bytes, limit)?;
        Some(Self {
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
            _storage: shared.storage.clone(),
        })
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
            reported_blocked: false,
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
    pub fn start(client: Client) -> Result<Self, String> {
        let storage = crate::execution::process_quota()
            .reserve_external_storage(IMMUTABLE_STORAGE_BYTES)
            .map_err(|error| format!("terminal immutable storage admission: {error:?}"))?;
        Self::start_with_storage(client, Arc::new(storage))
    }
    fn start_with_storage(client: Client, storage: Arc<StorageAdmission>) -> Result<Self, String> {
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
            #[cfg(test)]
            state_limit: AtomicUsize::new(MAX_STATE_BYTES),
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
    pub(crate) fn attach(&self, pane_id: NodeId, view: &mut TerminalView) -> Result<(), String> {
        if view.frontend.is_some() {
            return Ok(());
        }
        let (rows, cols) = view.desired_size;
        validate_geometry(rows, cols)?;
        self.shared
            .engine_claims
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |claims| {
                (claims < 16).then_some(claims + 1)
            })
            .map_err(|_| {
                "Terminal parser engine admission limit (16 live or retiring panes)".to_string()
            })?;
        let claim = Arc::new(EngineClaim(self.shared.engine_claims.clone()));
        let target = PaneTarget {
            pane_id,
            identity: view.identity.clone(),
            alive: Arc::new(AtomicBool::new(true)),
            suspended_output: Arc::new(AtomicBool::new(false)),
            confirmed_removed: Arc::new(AtomicBool::new(false)),
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
        let drained = results.drain(..).collect();
        drop(results);
        self.shared.changed.notify_one();
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
    target: PaneTarget,
    state: TerminalState,
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
        loop {
            if context.stop_requested() {
                return cancellation_report(&shared);
            }
            engines.retain(|_, engine| engine.target.alive.load(Ordering::Acquire));
            let mut queue = shared
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let eligible = |queue: &Queue, deferred: &VecDeque<Command>| {
                queue.commands.iter().position(|command| {
                    !deferred.iter().any(|pending| {
                        command.target.pane_id == pending.target.pane_id
                            && Arc::ptr_eq(&command.target.identity, &pending.target.identity)
                    })
                })
            };
            while eligible(&queue, &deferred).is_none()
                && !context.stop_requested()
                && !queue.closed
            {
                queue = shared
                    .changed
                    .wait_timeout(queue, Duration::from_millis(100))
                    .unwrap_or_else(|error| error.into_inner())
                    .0;
                if !deferred.is_empty() {
                    break;
                }
            }
            if context.stop_requested() || queue.closed {
                drop(queue);
                return cancellation_report(&shared);
            }
            let position = eligible(&queue, &deferred);
            let mut command = if !deferred.is_empty() && (retry_deferred || position.is_none()) {
                retry_deferred = false;
                // Invariant: guarded by !deferred.is_empty(); exclusively owned.
                deferred.pop_front().expect("owned deferred command exists")
            } else if let Some(position) = position {
                retry_deferred = !deferred.is_empty();
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
            drop(queue);
            if !command.target.alive.load(Ordering::Acquire) {
                if command.target.suspended_output.load(Ordering::Acquire) {
                    push_result(&shared, ParseResult::Error {
                        target: command.target.clone(),
                        message: format!("Confirmed pane invalidation canceled admitted terminal ordinal{} after{} bytes; remaining original command explicitly released", command.ordinal, command.cursor.consumed),
                    }, &context)?;
                }
                let mut queue = shared
                    .queue
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                queue.active_bytes -= command.bytes;
                queue.active_commands -= 1;
                continue;
            }
            let mut evidence = None;
            let result: Result<(), String> = (|| {
                if let PaneCommand::Register { rows, cols, budget } = &command.body {
                    validate_geometry(*rows, *cols)?;
                    if engines.len() >= 16 {
                        return Err("terminal parser engine limit reached (16 panes)".into());
                    }
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
                                        try_state_budget(
                                            &mut engines,
                                            state.input_peak_bytes(chunk),
                                            &shared,
                                            &context,
                                            &command.target,
                                            &mut command.blocked,
                                        )
                                    },
                                )?;
                            }
                            PaneCommand::Replay {
                                sequence, bytes, ..
                            } => {
                                if !command.cursor.replay_started
                                    && *sequence > engine.state.output_sequence()
                                {
                                    let (rows, columns) =
                                        engine.state.with_screen(|screen| screen.size());
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
                                        try_state_budget(
                                            &mut engines,
                                            state.input_peak_bytes(chunk),
                                            &shared,
                                            &context,
                                            &command.target,
                                            &mut command.blocked,
                                        )
                                    },
                                )?;
                            }
                            PaneCommand::Resize(rows, cols) => {
                                validate_geometry(*rows, *cols)?;
                                let old = engine.state.with_screen(|screen| screen.size());
                                let peak = engine.state.retained_allocation_bytes().saturating_add(
                                    if *rows <= old.0 && *cols <= old.1 {
                                        0
                                    } else {
                                        geometry_peak(*rows, *cols)
                                    },
                                );
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
                    return Ok(());
                }
                let engine = engines
                    .get(&command.target.pane_id)
                    .ok_or("terminal state disappeared")?;
                // Only complete screen payloads coalesce. Every parsed output's
                // semantic evidence and ordinal remain in their original place.
                if let Ok(mut results) = shared.results.lock() {
                    for result in results.iter_mut() {
                        let same = matches!(result,ParseResult::Published { target,.. } if target.pane_id==command.target.pane_id && Arc::ptr_eq(&target.identity,&command.target.identity));
                        if same {
                            let placeholder = ParseResult::Error {
                                target: command.target.clone(),
                                message: String::new(),
                            };
                            if let ParseResult::Published {
                                target,
                                ordinal,
                                evidence,
                                ..
                            } = std::mem::replace(result, placeholder)
                            {
                                *result = ParseResult::Acknowledged {
                                    target,
                                    ordinal,
                                    evidence,
                                };
                            }
                        }
                    }
                }
                let size = engine.state.retained_allocation_bytes();
                let charge = wait_snapshot_budget(&shared, size, &context, &command.target)?;
                let mut snapshot = engine.state.publish();
                let charge = Arc::new(charge);
                snapshot.history.retain_charge(charge.clone());
                snapshot.allocation_charge = Some(charge);
                push_result(
                    &shared,
                    ParseResult::Published {
                        target: command.target.clone(),
                        ordinal: command.ordinal,
                        snapshot: Arc::new(snapshot),
                        evidence,
                    },
                    &context,
                )
            })();
            if let Some(blocked) = command.blocked {
                if matches!(
                    command.body,
                    PaneCommand::Output { .. } | PaneCommand::Replay { .. }
                ) {
                    command
                        .target
                        .suspended_output
                        .store(true, Ordering::Release);
                }
                if !command.reported_blocked {
                    let message = if blocked.impossible {
                        format!("Terminal command requires {} bytes and cannot fit the128MiB engine admission; original bytes and exact cursor remain retained. Close this pane to cancel explicitly", blocked.required_bytes)
                    } else {
                        format!("Terminal engine memory backpressure ({} bytes required); original command/cursor retained while other panes continue", blocked.required_bytes)
                    };
                    push_result(
                        &shared,
                        ParseResult::Error {
                            target: command.target.clone(),
                            message,
                        },
                        &context,
                    )?;
                    command.reported_blocked = true;
                }
                deferred.push_back(command);
                shared.notification.notify_one();
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
            }
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
            return Err("terminal parser cancelled during publication".into());
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
    engines.retain(|_, engine| engine.target.alive.load(Ordering::Acquire));
    let used = engines
        .values()
        .map(|engine| engine.state.retained_allocation_bytes())
        .sum::<usize>();
    #[cfg(test)]
    let limit = shared.state_limit.load(Ordering::Acquire);
    #[cfg(not(test))]
    let limit = {
        let _ = shared;
        MAX_STATE_BYTES
    };
    if used.saturating_add(peak) <= limit {
        return Ok(());
    }
    *deferred = Some(StateDeferral {
        required_bytes: peak,
        impossible: peak > limit,
    });
    Err("terminal state allocation deferred before processing the next byte slice".into())
}

fn wait_snapshot_budget(
    shared: &Shared,
    bytes: usize,
    context: &JobContext,
    target: &PaneTarget,
) -> Result<SnapshotCharge, String> {
    if bytes > MAX_STATE_BYTES {
        return Err("Terminal publication exceeds its128MiB single-generation limit".into());
    }
    let mut reported = false;
    loop {
        if let Some(charge) = SnapshotCharge::reserve(shared, bytes, false) {
            return Ok(charge);
        }
        if context.stop_requested() || !target.alive.load(Ordering::Acquire) {
            return Err("terminal publication cancelled before acknowledgement".into());
        }
        if !reported {
            push_result(shared,ParseResult::Error {target:target.clone(),message:format!("Terminal immutable snapshot admission blocked ({bytes} bytes required); release retained histories or close a pane after server confirmation")},context)?;
            reported = true;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    use std::time::Instant;

    struct Bank(Execution);
    impl Bank {
        fn start() -> (Self, TerminalParsing) {
            let quota = QuotaGroup::new(QuotaLimits {
                clients: 1,
                jobs: 1,
                service_jobs: 1,
                input_bytes: 128 * MIB,
                result_bytes: 2 * MIB,
                worker_threads: 1,
                // State128 + independent immutable storage512 + metadata headroom.
                worker_bytes: 641 * MIB,
            });
            let empty = LaneConfig {
                threads: 0,
                queue_slots: 0,
                priority: None,
                resident_bytes_per_thread: 0,
            };
            let storage = Arc::new(
                quota
                    .reserve_external_storage(IMMUTABLE_STORAGE_BYTES)
                    .unwrap(),
            );
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
            let parsing = TerminalParsing::start_with_storage(client, storage).unwrap();
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
        let first =
            Arc::new(SnapshotCharge::reserve(&parsing.shared, MAX_PIN_BYTES, false).unwrap());
        let queued = first.lease_live();
        let emitted = queued.clone();
        first.retire_live();
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            MAX_PIN_BYTES
        );
        let replacement = SnapshotCharge::reserve(&parsing.shared, MAX_PIN_BYTES, false).unwrap();
        assert!(SnapshotCharge::reserve(&parsing.shared, 1, false).is_none());
        drop(queued);
        assert!(SnapshotCharge::reserve(&parsing.shared, 1, false).is_none());
        drop(emitted);
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            MAX_PIN_BYTES
        );
        assert!(SnapshotCharge::reserve(&parsing.shared, 1, false).is_some());
        replacement.retire_live();
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
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
        let next =
            Arc::new(SnapshotCharge::reserve(&parsing.shared, MAX_PIN_BYTES, false).unwrap());
        assert!(next.pin().is_err());
        let replacement = SnapshotCharge::reserve(&parsing.shared, MAX_PIN_BYTES, false).unwrap();
        assert_eq!(
            parsing.shared.snapshot_bytes.load(Ordering::Acquire),
            MAX_SNAPSHOT_BYTES
        );
        drop(pin);
        assert_eq!(
            parsing.shared.pin_bytes.load(Ordering::Acquire),
            MAX_PIN_BYTES
        );
        drop(cloned);
        assert_eq!(parsing.shared.pin_bytes.load(Ordering::Acquire), 0);
        assert!(next.pin().is_ok());
        drop(replacement);
    }
    #[test]
    fn capture_category_rejection_does_not_wait_or_consume_live_headroom() {
        let (_bank, parsing) = Bank::start();
        assert!(SnapshotCharge::reserve(&parsing.shared, MAX_CAPTURE_BYTES + 1, true).is_none());
        let capture = SnapshotCharge::reserve(&parsing.shared, MAX_CAPTURE_BYTES, true).unwrap();
        assert!(SnapshotCharge::reserve(&parsing.shared, 1, true).is_none());
        assert!(SnapshotCharge::reserve(&parsing.shared, MAX_PIN_BYTES, false).is_some());
        drop(capture);
        assert_eq!(parsing.shared.capture_bytes.load(Ordering::Acquire), 0);
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
        let mut view = TerminalView::new(3, 20);
        attach(&parsing, NodeId(5), &mut view);
        offer(
            &mut view,
            PaneCommand::Output {
                first: 1,
                sequence: 1,
                bytes: b"retained history".to_vec(),
                track: false,
            },
        );
        let published = result(
            &parsing,
            |result| matches!(result,ParseResult::Published {snapshot,..} if snapshot.sequence==1),
        );
        let ParseResult::Published { snapshot, .. } = published else {
            unreachable!()
        };
        let history = snapshot.history.clone();
        drop(snapshot);
        assert!(parsing.shared.snapshot_bytes.load(Ordering::Acquire) > 0);
        assert_eq!(history.to_vec(), b"retained history");
        drop(history);
        assert_eq!(parsing.shared.snapshot_bytes.load(Ordering::Acquire), 0);
    }
    #[test]
    fn blocked_publication_keeps_producer_nonblocking_and_shutdown_reports_cancellation() {
        let (_bank, mut parsing) = Bank::start();
        let hold = SnapshotCharge::reserve(&parsing.shared, MAX_SNAPSHOT_BYTES, false).unwrap();
        let mut view = TerminalView::new(3, 20);
        attach(&parsing, NodeId(6), &mut view);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if parsing.collect().iter().any(|result|matches!(result,ParseResult::Error {message,..} if message.contains("admission blocked"))) {break;}
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut command = PaneCommand::Output {
            first: 1,
            sequence: 1,
            bytes: b"owned while blocked".to_vec(),
            track: false,
        };
        loop {
            let start = Instant::now();
            match view.frontend.as_mut().unwrap().submit(command) {
                Ok(_) => break,
                Err(original) => command = original,
            }
            assert!(
                start.elapsed() < Duration::from_millis(50),
                "producer must return promptly while the owner is backpressured"
            );
            assert!(
                Instant::now() < deadline,
                "retained output was never admitted"
            );
            std::thread::yield_now();
        }
        assert!(parsing.pending_work().unwrap().1 >= b"owned while blocked".len());
        parsing.cancel();
        loop {
            if let Some(error) = parsing.failure() {
                assert!(error.contains("cancel"));
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(hold);
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
        Err(format!("Terminal parser shutdown cancelled {count} admitted commands ({bytes} retained bytes) before completion"))
    }
}
