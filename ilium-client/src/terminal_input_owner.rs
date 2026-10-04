//! One admitted native reader owns capability queries and normal input.
//! Parser backing storage is admitted before growth; completed payload leases
//! travel with their original envelopes. Shutdown retains the source itself.

#[cfg(test)]
use crossterm::event::TerminalQueryItem;
use crossterm::{
    event::{
        native::{
            NativeInput, NativeInputReader, NativePasteCompletion, NativeReadError,
            NativeRefusalKind, NativeStorage, NativeStorageLease, NativeStorageRefusal,
        },
        Event, KeyboardEnhancementFlags, PushKeyboardEnhancementFlags, TerminalQueryReader,
    },
    execute,
};
use ilium_execution::{QuotaGroup, RejectReason, StorageAdmission, WorkerAdmission};
use ilium_platform::owned_worker::{
    reserve_owned_worker, worker_retained_state_bytes, OwnedWorker, StopToken, WorkerExit,
    WorkerKind, WorkerReservation, WorkerTicket,
};
use ratatui_image::{
    errors::Errors as ImageError,
    picker::{
        cap_parser::{Parser as ImageParser, QueryStdioOptions, Response},
        Picker,
    },
};
use std::{
    collections::VecDeque,
    fmt,
    io::{self, Write},
    mem::size_of,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc::error::TryRecvError, oneshot, Notify};

pub const INPUT_QUEUE_CAPACITY: usize = 256;
const IMAGE_QUERY_RESPONSE_CAPACITY: usize = 16;
const PASTE_DELIMITER_BYTES: usize = 12;
const PASTE_REQUEST_WIRE_BYTES: usize = 4 + 8 + 8 + 1 + 1;
pub const INPUT_STACK_BYTES: usize = 2 * 1024 * 1024;
// The original dispatch loop holds its current event and at most one lookahead.
// A producer acquires a queue slot before reading, so these two extra envelopes
// also cover its one native result. Retaining envelopes applies backpressure.
const INPUT_EVENT_SLOTS: usize = INPUT_QUEUE_CAPACITY + 2;
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const KEYBOARD_QUERY_TIMEOUT: Duration = Duration::from_millis(2000);
static INPUT_CLAIMED: AtomicBool = AtomicBool::new(false);

/// Visible Rust state and requested queue/envelope capacity. Stack mappings,
/// allocator rounding, Notify internals and upstream/native parser allocations
/// remain qualification boundaries. Paste capacity is separately admitted.
pub fn input_storage_bytes() -> usize {
    size_of::<Shared>()
        + size_of::<InputStorage>()
        + 4 * size_of::<usize>()
        + (INPUT_QUEUE_CAPACITY + INPUT_EVENT_SLOTS) * size_of::<InputEvent>()
        + size_of::<InputFailure>()
        // Native Paste may wait in the terminal queue and then be returned as
        // an owned shutdown failure. At most 258 native slots coexist; include
        // captured identity and the two bounded failure-box headers per slot.
        + INPUT_EVENT_SLOTS * (size_of::<crate::terminal_input::NativePaste>() + size_of::<InputFailure>() + 4 * size_of::<usize>())
        + size_of::<InputShutdownReport>()
        + size_of::<crate::error::InputRunError>()
        + size_of::<InputRetirement>()
        + size_of::<InputRetirementDeadline>()
        + size_of::<InputOwner>()
        + size_of::<InputReceiver>()
        + size_of::<InputReservation>()
        + size_of::<InputSession>()
        + size_of::<Picker>()
        + size_of::<ImageParser>()
        + size_of::<oneshot::Sender<io::Result<Option<Picker>>>>()
        + size_of::<oneshot::Receiver<io::Result<Option<Picker>>>>()
        + size_of::<Arc<AtomicBool>>()
        + size_of::<AtomicBool>()
        + IMAGE_QUERY_RESPONSE_CAPACITY * size_of::<Response>()
        // A complete query reply is at most 256 bytes; the pinned crossterm
        // source can also hold an incomplete candidate and decoded records.
        + 2 * 256
        + 64 * 1024
        + worker_retained_state_bytes()
        + size_of::<Arc<Shared>>()
        + 2 * size_of::<usize>()
        + size_of::<InputCustody>()
        + size_of::<ExclusiveInput>()
        + size_of::<RootNativeStorage>()
        // Completed envelopes plus two overlapping parser backings and one
        // capability reply own at most this many adapter lease boxes.
        + (INPUT_EVENT_SLOTS + 3) * (size_of::<NativeLease>() + 2 * size_of::<usize>())
        + size_of::<NativeInputCustody>()
        + size_of::<InputDriver>()
        + 2 * size_of::<usize>()
}

#[derive(Debug, thiserror::Error)]
pub enum InputStartError {
    #[error("another terminal input session is active or still retiring")]
    AlreadyOwned,
    #[error("terminal input admission rejected: {0:?}")]
    Admission(RejectReason),
    #[error("terminal input worker could not start: {0}")]
    Io(#[source] io::Error),
}

struct ExclusiveInput;
impl Drop for ExclusiveInput {
    fn drop(&mut self) {
        INPUT_CLAIMED.store(false, Ordering::Release);
    }
}
/// The caller retains this share through terminal restoration. Native record
/// custody owns the other share until actual join, independently of tickets.
pub(crate) struct InputSession {
    _claim: Arc<ExclusiveInput>,
    _storage: Arc<InputStorage>,
}
struct InputCustody {
    _admission: WorkerAdmission,
    _claim: Arc<ExclusiveInput>,
}
struct InputStorage {
    active: Mutex<usize>,
    available: Condvar,
    _admission: StorageAdmission,
}
struct InputSlot {
    storage: Arc<InputStorage>,
}
impl Drop for InputSlot {
    fn drop(&mut self) {
        let mut active = self
            .storage
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *active -= 1;
        drop(active);
        self.storage.available.notify_all();
    }
}

/// `dispatch` keeps both admissions until the synchronous callback returns.
/// The existing App owns any subsequent copies or moved domain payloads; its
/// downstream storage cannot be inferred from the raw Event handoff alone.
pub struct InputEvent {
    event: Option<Event>,
    payload_bytes: usize,
    payload: Option<StorageAdmission>,
    native_payload: Option<Box<dyn NativeStorageLease>>,
    _slot: InputSlot,
}
impl InputEvent {
    pub fn view(&self) -> &Event {
        self.event
            .as_ref()
            .expect("an input envelope is consumed exactly once")
    }
    /// Fixed-int bincode UserKeyInput overhead: enum4, NodeId8, Vec length8,
    /// absent submission1 and absent prompt_epoch1. Mode is checked again at
    /// the original ordered barrier before a bracketed derivative is allocated.
    pub(crate) fn paste_wire_bytes(&self, bracketed: bool) -> usize {
        match self.view() {
            Event::Paste(text) => text
                .len()
                .saturating_add(PASTE_REQUEST_WIRE_BYTES)
                .saturating_add(if bracketed { PASTE_DELIMITER_BYTES } else { 0 }),
            _ => unreachable!("only native Paste envelopes use the wire allowance"),
        }
    }
    pub fn retained_payload_bytes(&self) -> usize {
        self.payload_bytes
    }
    pub fn unadmitted_payload_bytes(&self) -> usize {
        if self.payload.is_none() && self.native_payload.is_none() {
            self.payload_bytes
        } else {
            0
        }
    }
    /// Existing hover-motion coalescing is an explicit disposition. Other
    /// original input cannot pass through this method or be discarded by it.
    pub(crate) fn coalesce_motion(self) -> Result<(), Self> {
        if !matches!(
            self.view(),
            Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Moved,
                ..
            })
        ) {
            return Err(self);
        }
        drop(self);
        Ok(())
    }
    /// Move the original Paste allocation while retaining its native lease
    /// through construction/publication of the separately admitted request.
    pub(crate) fn dispatch_paste<R>(self, dispatch: impl FnOnce(String) -> R) -> R {
        self.dispatch(|event| match event {
            Event::Paste(text) => dispatch(text),
            _ => unreachable!("native terminal paste captures only Event::Paste"),
        })
    }
    pub fn dispatch<R>(mut self, dispatch: impl FnOnce(Event) -> R) -> R {
        let event = self
            .event
            .take()
            .expect("an input envelope is consumed exactly once");
        let result = dispatch(event);
        drop(self);
        result
    }
}
impl fmt::Debug for InputEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InputEvent")
            .field("retained_payload_bytes", &self.payload_bytes)
            .field("unadmitted_payload_bytes", &self.unadmitted_payload_bytes())
            .finish_non_exhaustive()
    }
}

pub struct InputFailure {
    kind: InputFailureKind,
}
enum InputFailureKind {
    Read {
        source: io::Error,
        _storage: Arc<InputStorage>,
    },
    Refused {
        reason: RejectReason,
        original: InputEvent,
    },
    NativeRead {
        source: NativeReadError,
        custody: NativeInputCustody,
    },
    Shutdown(Box<InputShutdownReport>),
    Combined {
        first: Box<InputFailure>,
        next: Box<InputFailure>,
    },
    Undispatched {
        original: InputEvent,
        previous: Option<Box<InputFailure>>,
    },
}
impl InputFailure {
    pub(crate) fn combine(first: Option<Self>, next: Self) -> Self {
        match first {
            Some(first) => Self {
                kind: InputFailureKind::Combined {
                    first: Box::new(first),
                    next: Box::new(next),
                },
            },
            None => next,
        }
    }
    pub fn additional_failure(&self) -> Option<&Self> {
        match &self.kind {
            InputFailureKind::Combined { next, .. } => Some(next),
            InputFailureKind::Undispatched { previous, .. } => previous.as_deref(),
            _ => None,
        }
    }
    pub(crate) fn undispatched(original: InputEvent, previous: Option<Self>) -> Self {
        Self {
            kind: InputFailureKind::Undispatched {
                original,
                previous: previous.map(Box::new),
            },
        }
    }
    pub(crate) fn refused(reason: RejectReason, original: InputEvent) -> Self {
        Self {
            kind: InputFailureKind::Refused { reason, original },
        }
    }
    pub fn rejection_reason(&self) -> Option<RejectReason> {
        match &self.kind {
            InputFailureKind::Refused { reason, .. } => Some(*reason),
            InputFailureKind::NativeRead {
                source: NativeReadError::Admission(_),
                custody,
            } => custody.admission_reason(),
            InputFailureKind::Combined { first, .. } => first.rejection_reason(),
            _ => None,
        }
    }
    pub fn native_custody(&self) -> Option<&NativeInputCustody> {
        match &self.kind {
            InputFailureKind::NativeRead { custody, .. } => Some(custody),
            InputFailureKind::Combined { first, next } => {
                first.native_custody().or_else(|| next.native_custody())
            }
            InputFailureKind::Shutdown(report) => report.native_custody(),
            InputFailureKind::Undispatched { previous, .. } => {
                previous.as_deref().and_then(Self::native_custody)
            }
            _ => None,
        }
    }
    pub fn original(&self) -> Option<&InputEvent> {
        match &self.kind {
            InputFailureKind::Refused { original, .. }
            | InputFailureKind::Undispatched { original, .. } => Some(original),
            InputFailureKind::Combined { first, .. } => first.original(),
            _ => None,
        }
    }
    pub fn shutdown_report(&self) -> Option<&InputShutdownReport> {
        match &self.kind {
            InputFailureKind::Shutdown(report) => Some(report),
            InputFailureKind::Combined { first, .. } => first.shutdown_report(),
            _ => None,
        }
    }
}
impl fmt::Display for InputFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            InputFailureKind::Combined { first, .. } => write!(formatter, "{first}; another original input failure remains in custody"),
            InputFailureKind::NativeRead { source, .. } => write!(formatter, "native terminal input failed: {source}; original source retained"),
            InputFailureKind::Read { source, .. } => write!(formatter, "terminal input read failed: {source}"),
            InputFailureKind::Refused { reason, original } => write!(formatter,
                "terminal input retained one whole original after {reason:?}; {} payload bytes are outside admission",
                original.unadmitted_payload_bytes()),
            InputFailureKind::Undispatched { original, previous } => write!(formatter,
                "terminal input shutdown retained an undispatched original ({} payload bytes); previous failure={}",
                original.retained_payload_bytes(), previous.is_some()),
            InputFailureKind::Shutdown(report) => write!(formatter,
                "terminal input {:?}; {} undispatched originals, reader failure={}, retained native input={} remain in custody",
                report.exit, report.pending.len(), report.failure.is_some(), report.native_custody().is_some_and(NativeInputCustody::has_retained_input)),
        }
    }
}
impl fmt::Debug for InputFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}
impl std::error::Error for InputFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            InputFailureKind::Read { source, .. } => Some(source),
            InputFailureKind::NativeRead { source, .. } => Some(source),
            InputFailureKind::Combined { first, .. } => Some(first.as_ref()),
            InputFailureKind::Shutdown(report) => report.failure.as_deref().map(|error| error as _),
            InputFailureKind::Refused { .. } => None,
            InputFailureKind::Undispatched { previous, .. } => {
                previous.as_deref().map(|error| error as _)
            }
        }
    }
}

struct State {
    events: VecDeque<InputEvent>,
    failure: Option<InputFailure>,
    producer_done: bool,
    receiver_open: bool,
    retiring: bool,
    queue_excess: Option<StorageAdmission>,
    native: Option<NativeInputCustody>,
}
struct Shared {
    state: Mutex<State>,
    available: Condvar,
    ready: Notify,
    stop: StopToken,
    quota: QuotaGroup,
    storage: Arc<InputStorage>,
}
impl Shared {
    fn wake(&self) {
        self.available.notify_all();
        self.storage.available.notify_all();
        self.ready.notify_waiters();
    }
    fn close(&self, retiring: bool) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.receiver_open = false;
        state.retiring |= retiring;
        self.stop.stop();
        drop(state);
        self.wake();
    }
    fn reserve_slot(&self) -> Option<InputSlot> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        while state.events.len() >= INPUT_QUEUE_CAPACITY && !self.stop.is_stopped() {
            state = self
                .available
                .wait_timeout(state, INPUT_POLL_INTERVAL)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        if self.stop.is_stopped() {
            return None;
        }
        drop(state);
        let mut active = self
            .storage
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while *active >= INPUT_EVENT_SLOTS && !self.stop.is_stopped() {
            active = self
                .storage
                .available
                .wait_timeout(active, INPUT_POLL_INTERVAL)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        if self.stop.is_stopped() {
            return None;
        }
        *active += 1;
        Some(InputSlot {
            storage: Arc::clone(&self.storage),
        })
    }
    // A capability probe must resolve when the existing admitted FIFO fills.
    // The normal reader may wait for a consumer; startup cannot, since that
    // consumer starts only after the probe has returned.
    fn query_slot(&self) -> Option<InputSlot> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if self.stop.is_stopped() || state.events.len() >= INPUT_QUEUE_CAPACITY {
            return None;
        }
        drop(state);
        let mut active = self
            .storage
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *active >= INPUT_EVENT_SLOTS {
            return None;
        }
        *active += 1;
        Some(InputSlot {
            storage: Arc::clone(&self.storage),
        })
    }

    fn fail(&self, failure: InputFailure) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.failure = Some(failure);
        self.stop.stop();
        drop(state);
        self.wake();
    }
}

pub(crate) struct InputReservation {
    worker: WorkerReservation<InputCustody>,
    quota: QuotaGroup,
    storage: Arc<InputStorage>,
    claim: Arc<ExclusiveInput>,
}
impl InputReservation {
    /// Call before entering raw mode or running terminal capability queries.
    pub(crate) fn prepare(quota: &QuotaGroup) -> Result<Self, InputStartError> {
        INPUT_CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| InputStartError::AlreadyOwned)?;
        let claim = ExclusiveInput;
        let storage = quota
            .reserve_external_storage(input_storage_bytes())
            .map_err(InputStartError::Admission)?;
        let admission = quota
            .reserve_external_worker(1, INPUT_STACK_BYTES)
            .map_err(InputStartError::Admission)?;
        let claim = Arc::new(claim);
        let worker = reserve_owned_worker(
            Some(INPUT_STACK_BYTES),
            InputCustody {
                _admission: admission,
                _claim: Arc::clone(&claim),
            },
        )
        .map_err(InputStartError::Io)?;
        Ok(Self {
            worker,
            quota: quota.clone(),
            claim,
            storage: Arc::new(InputStorage {
                active: Mutex::new(0),
                available: Condvar::new(),
                _admission: storage,
            }),
        })
    }
    pub(crate) fn session_claim(&self) -> InputSession {
        InputSession {
            _claim: Arc::clone(&self.claim),
            _storage: Arc::clone(&self.storage),
        }
    }
    /// Start the one admitted native reader. It performs the image query first,
    /// then continues as the ordinary crossterm event owner on that same thread.
    pub(crate) fn start_with_image_probe(
        self,
        keyboard_enhancement_pushed: Arc<AtomicBool>,
    ) -> Result<
        (
            InputOwner,
            InputReceiver,
            oneshot::Receiver<io::Result<Option<Picker>>>,
        ),
        InputStartError,
    > {
        let (probe_sender, probe_receiver) = oneshot::channel();
        let factory = InputReaderFactory::Native(Arc::clone(&self.claim));
        let (owner, receiver) = self.start_with_reader_and_probe(
            factory,
            Some((probe_sender, keyboard_enhancement_pushed)),
        )?;
        Ok((owner, receiver, probe_receiver))
    }

    #[cfg(test)]
    fn start_with_reader(
        self,
        read: impl FnMut(Duration) -> io::Result<Option<Event>> + Send + 'static,
    ) -> Result<(InputOwner, InputReceiver), InputStartError> {
        self.start_with_reader_and_probe(InputReaderFactory::Injected(Box::new(read)), None)
    }

    fn start_with_reader_and_probe(
        self,
        factory: InputReaderFactory,
        probe_sender: Option<(oneshot::Sender<io::Result<Option<Picker>>>, Arc<AtomicBool>)>,
    ) -> Result<(InputOwner, InputReceiver), InputStartError> {
        let mut events = VecDeque::new();
        events
            .try_reserve_exact(INPUT_QUEUE_CAPACITY)
            .map_err(|error| InputStartError::Io(io::Error::other(error)))?;
        let excess = (events.capacity() - INPUT_QUEUE_CAPACITY)
            .checked_mul(size_of::<InputEvent>())
            .ok_or(InputStartError::Admission(RejectReason::InvalidCost))?;
        let queue_excess = if excess == 0 {
            None
        } else {
            Some(
                self.quota
                    .reserve_external_storage(excess)
                    .map_err(InputStartError::Admission)?,
            )
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                events,
                failure: None,
                producer_done: false,
                receiver_open: true,
                retiring: false,
                queue_excess,
                native: None,
            }),
            available: Condvar::new(),
            ready: Notify::new(),
            stop: StopToken::default(),
            quota: self.quota,
            storage: self.storage,
        });
        let wake = Arc::clone(&shared);
        let body = Arc::clone(&shared);
        let worker = self
            .worker
            .spawn(
                "ilium-terminal-input",
                WorkerKind::SynchronousIo,
                shared.stop.clone(),
                move || wake.wake(),
                move |_| {
                    let _exit = ReaderExit(Arc::clone(&body));
                    let mut input = match InputDriver::new(Arc::clone(&body), factory) {
                        Ok(input) => input,
                        Err(source) => {
                            if let Some((sender, _)) = probe_sender {
                                let _ = sender.send(Err(io::Error::other(
                                    "native input source construction failed",
                                )));
                            }
                            body.fail(InputFailure {
                                kind: InputFailureKind::Read {
                                    source,
                                    _storage: Arc::clone(&body.storage),
                                },
                            });
                            return;
                        }
                    };
                    if let Some((sender, pushed)) = probe_sender {
                        let _ = sender.send(prepare_terminal_probes(&body, &pushed, &mut input));
                    }
                    read_owned_events(&body, &mut input);
                },
            )
            .map_err(InputStartError::Io)?;
        Ok((
            InputOwner {
                worker,
                shared: Arc::clone(&shared),
            },
            InputReceiver { shared },
        ))
    }
}

/// Recovery owns the physical source, not a copy of diagnostic input. Field
/// order drops the source before its fixed storage and exclusive terminal claim.
pub struct NativeInputCustody {
    reader: NativeInputReader,
    _storage: Arc<InputStorage>,
    _claim: Arc<ExclusiveInput>,
    admission: Arc<RootNativeStorage>,
}
impl NativeInputCustody {
    pub fn retained_original(&self) -> (&[u8], &[u8]) {
        self.reader.retained_original()
    }
    pub fn admission_reason(&self) -> Option<RejectReason> {
        *self
            .admission
            .last_refusal
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
    pub fn has_retained_input(&self) -> bool {
        self.reader.has_retained_input()
    }
}

struct RootNativeStorage {
    mandatory_bytes: usize,
    last_refusal: Mutex<Option<RejectReason>>,
    quota: QuotaGroup,
    storage: Arc<InputStorage>,
    stop: StopToken,
}
struct NativeLease {
    admission: Option<StorageAdmission>,
    storage: Arc<InputStorage>,
}
impl Drop for NativeLease {
    fn drop(&mut self) {
        // Release credit before waking the waiter that will try to acquire it.
        drop(self.admission.take());
        self.storage.available.notify_all();
    }
}
impl RootNativeStorage {
    fn refusal_kind(
        &self,
        reason: RejectReason,
        bytes: usize,
        retained_bytes: usize,
    ) -> NativeRefusalKind {
        match reason {
            RejectReason::Busy => NativeRefusalKind::Busy,
            RejectReason::Closed => NativeRefusalKind::Closed,
            RejectReason::WorkerBytes => {
                // Only immutable limits participate in feasibility. Concurrent
                // usage snapshots cannot prove what another owner may release.
                let limit = self.quota.snapshot().limits.worker_bytes;
                let required = self
                    .mandatory_bytes
                    .checked_add(retained_bytes)
                    .and_then(|value| value.checked_add(bytes));
                if required.is_some_and(|required| required <= limit) {
                    NativeRefusalKind::Busy
                } else {
                    NativeRefusalKind::Limit
                }
            }
            _ => NativeRefusalKind::Limit,
        }
    }
    fn reserve_backing(
        &self,
        bytes: usize,
        retained_bytes: usize,
    ) -> Result<Box<dyn NativeStorageLease>, NativeStorageRefusal> {
        if self.stop.is_stopped() {
            *self
                .last_refusal
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(RejectReason::Closed);
            return Err(NativeStorageRefusal {
                kind: NativeRefusalKind::Closed,
                requested: bytes,
            });
        }
        let required = self
            .mandatory_bytes
            .checked_add(retained_bytes)
            .and_then(|value| value.checked_add(bytes));
        if !required.is_some_and(|required| required <= self.quota.snapshot().limits.worker_bytes) {
            // This immutable cost proof does not acquire or speculate about
            // another owner's credit. No new backing or debit is created.
            *self
                .last_refusal
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(RejectReason::WorkerBytes);
            return Err(NativeStorageRefusal {
                kind: NativeRefusalKind::Limit,
                requested: bytes,
            });
        }
        self.quota
            .reserve_external_storage(bytes)
            .map(|admission| {
                Box::new(NativeLease {
                    admission: Some(admission),
                    storage: Arc::clone(&self.storage),
                }) as Box<dyn NativeStorageLease>
            })
            .map_err(|reason| {
                *self
                    .last_refusal
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(reason);
                NativeStorageRefusal {
                    kind: self.refusal_kind(reason, bytes, retained_bytes),
                    requested: bytes,
                }
            })
    }
}

impl NativeStorage for RootNativeStorage {
    fn paste_completion(
        &self,
        payload_bytes: usize,
        backing_capacity: usize,
    ) -> NativePasteCompletion {
        // Mode is owned by the later pane barrier. The minimum request framing
        // must fit here; that barrier still checks the actual bracketed mode.
        // No semantic or wire budget is raised to accommodate parser slack.
        if backing_capacity.saturating_add(PASTE_DELIMITER_BYTES) > crate::terminal_input::MAX_BYTES
            && payload_bytes.saturating_add(PASTE_DELIMITER_BYTES)
                <= crate::terminal_input::MAX_BYTES
            && payload_bytes.saturating_add(PASTE_REQUEST_WIRE_BYTES)
                <= ilium_ipc::MAX_FRAME_LEN as usize
        {
            NativePasteCompletion::Compact
        } else {
            NativePasteCompletion::KeepBacking
        }
    }

    fn reserve(&self, bytes: usize) -> Result<Box<dyn NativeStorageLease>, NativeStorageRefusal> {
        self.reserve_backing(bytes, 0)
    }
    fn reserve_replacement(
        &self,
        new_bytes: usize,
        retained_bytes: usize,
    ) -> Result<Box<dyn NativeStorageLease>, NativeStorageRefusal> {
        self.reserve_backing(new_bytes, retained_bytes)
    }
}

enum InputReaderFactory {
    Native(Arc<ExclusiveInput>),
    #[cfg(test)]
    NativeObserved(
        Arc<ExclusiveInput>,
        std::sync::mpsc::Sender<Arc<RootNativeStorage>>,
    ),
    #[cfg(test)]
    Injected(Box<dyn FnMut(Duration) -> io::Result<Option<Event>> + Send>),
}
enum InputReader {
    Native(Option<NativeInputCustody>),
    #[cfg(test)]
    Injected(Box<dyn FnMut(Duration) -> io::Result<Option<Event>> + Send>),
}
struct NativeReply {
    bytes: Vec<u8>,
    _storage: Option<Box<dyn NativeStorageLease>>,
}
enum ReadItem {
    Event(Event, Option<Box<dyn NativeStorageLease>>),
    Reply(NativeReply),
}
struct InputDriver {
    shared: Arc<Shared>,
    reader: InputReader,
}
impl InputDriver {
    fn new(shared: Arc<Shared>, factory: InputReaderFactory) -> io::Result<Self> {
        let reader = match factory {
            #[cfg(test)]
            InputReaderFactory::NativeObserved(claim, observer) => {
                // Construct through the exact production path. The bounded
                // test handoff observes admission only, never reads the TTY.
                let driver = Self::new(shared, InputReaderFactory::Native(claim))?;
                if let InputReader::Native(Some(custody)) = &driver.reader {
                    observer
                        .send(Arc::clone(&custody.admission))
                        .map_err(|_| io::Error::other("isolated native fixture observer closed"))?;
                }
                return Ok(driver);
            }
            InputReaderFactory::Native(claim) => {
                let storage = Arc::new(RootNativeStorage {
                    mandatory_bytes: input_storage_bytes().saturating_add(INPUT_STACK_BYTES),
                    last_refusal: Mutex::new(None),
                    quota: shared.quota.clone(),
                    storage: Arc::clone(&shared.storage),
                    stop: shared.stop.clone(),
                });
                InputReader::Native(Some(NativeInputCustody {
                    reader: NativeInputReader::new(storage.clone())?,
                    admission: storage,
                    _storage: Arc::clone(&shared.storage),
                    _claim: claim,
                }))
            }
            #[cfg(test)]
            InputReaderFactory::Injected(read) => InputReader::Injected(read),
        };
        Ok(Self { shared, reader })
    }
    fn query_mode(&mut self, active: bool) {
        // The query guard belongs to this same physical reader. Every normal
        // return path (including capability errors) disables it before reading
        // ordinary events again; incomplete reply originals then replay FIFO.
        match &mut self.reader {
            InputReader::Native(Some(custody)) => custody.reader.set_terminal_query_mode(active),
            InputReader::Native(None) => {}
            #[cfg(test)]
            InputReader::Injected(_) => {}
        }
    }
    fn next(&mut self, timeout: Duration) -> io::Result<Option<ReadItem>> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.shared.stop.is_stopped() {
                return Ok(None);
            }
            let result = match &mut self.reader {
                InputReader::Native(Some(custody)) => custody
                    .reader
                    .read(deadline.saturating_duration_since(Instant::now()))
                    .map(|item| {
                        item.map(|item| {
                            let (input, lease) = item.into_parts();
                            match input {
                                NativeInput::Event(event) => ReadItem::Event(event, lease),
                                NativeInput::Reply(reply) => ReadItem::Reply(NativeReply {
                                    bytes: reply,
                                    _storage: lease,
                                }),
                            }
                        })
                    }),
                InputReader::Native(None) => {
                    return Err(io::Error::other(
                        "native source is retained in failure custody",
                    ))
                }
                #[cfg(test)]
                InputReader::Injected(read) => read(timeout)
                    .map(|item| item.map(|event| ReadItem::Event(event, None)))
                    .map_err(NativeReadError::Io),
            };
            match result {
                Ok(item) => return Ok(item),
                Err(NativeReadError::Admission(refusal))
                    if refusal.kind == NativeRefusalKind::Busy =>
                {
                    // No native read or parser byte acknowledgement while
                    // awaiting credit. Own lease/slot drops and cancellation
                    // notify this Condvar. Unrelated root releases have no
                    // subscriber API, so use the existing bounded100ms fallback.
                    if !await_native_credit(&self.shared, deadline) {
                        return Ok(None);
                    }
                }
                Err(source) => {
                    let failure = match &mut self.reader {
                        InputReader::Native(custody) => InputFailure { kind: InputFailureKind::NativeRead {
                            source, custody: custody.take().expect("the native source is moved exactly once on its terminal failure"),
                        } },
                        #[cfg(test)]
                        InputReader::Injected(_) => InputFailure { kind: InputFailureKind::Read {
                            source: match source { NativeReadError::Io(error) => error, NativeReadError::Admission(error) => io::Error::other(error) },
                            _storage: Arc::clone(&self.shared.storage),
                        } },
                    };
                    self.shared.fail(failure);
                    return Err(io::Error::other(
                        "terminal input failure retains its original source",
                    ));
                }
            }
        }
    }
}
// Native-operation waiting, not agent job polling. Cancellation and the
// existing deadline bound this wait even without a root release subscription.
fn await_native_credit(shared: &Shared, deadline: Instant) -> bool {
    if shared.stop.is_stopped() || Instant::now() >= deadline {
        return false;
    }
    let active = shared
        .storage
        .active
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if shared.stop.is_stopped() {
        return false;
    }
    let _wait = shared
        .storage
        .available
        .wait_timeout(
            active,
            deadline
                .saturating_duration_since(Instant::now())
                .min(INPUT_POLL_INTERVAL),
        )
        .unwrap_or_else(|error| error.into_inner());
    !shared.stop.is_stopped() && Instant::now() < deadline
}

impl Drop for InputDriver {
    fn drop(&mut self) {
        match &mut self.reader {
            InputReader::Native(custody) => {
                if let Some(custody) = custody.take() {
                    let mut state = self
                        .shared
                        .state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    state.native = Some(custody);
                }
            }
            #[cfg(test)]
            InputReader::Injected(_) => {}
        }
    }
}

struct ReaderExit(Arc<Shared>);
impl Drop for ReaderExit {
    fn drop(&mut self) {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.producer_done = true;
        drop(state);
        self.0.wake();
    }
}
fn prepare_terminal_probes(
    shared: &Shared,
    keyboard_enhancement_pushed: &AtomicBool,
    input: &mut InputDriver,
) -> io::Result<Option<Picker>> {
    // The platform-specific request is exposed by crossterm, but its filtered
    // reader is deliberately not used: that reader can queue ordinary input
    // without acquiring one of this client's finite input envelopes first.
    let keyboard_supported =
        TerminalQueryReader::keyboard_support_request().is_some_and(|request| {
            match query_keyboard_support(shared, input, request) {
                Ok(supported) => supported,
                Err(error) => {
                    tracing::warn!(%error, "terminal keyboard capability query failed");
                    false
                }
            }
        });
    if shared.stop.is_stopped() {
        return Ok(None);
    }
    if keyboard_supported {
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        keyboard_enhancement_pushed.store(true, Ordering::Release);
    }
    // A saturated startup FIFO has no UI consumer until this routine returns.
    // Do not send an image request whose replies we cannot read yet.
    let Some(image_query_slot) = shared.query_slot() else {
        return Ok(None);
    };
    drop(image_query_slot);
    Ok(probe_terminal_image_support(shared, input))
}

fn query_keyboard_support(
    shared: &Shared,
    input: &mut InputDriver,
    request: &[u8],
) -> io::Result<bool> {
    input.query_mode(true);
    let result = (|| {
        io::stdout().write_all(request)?;
        io::stdout().flush()?;
        collect_owned_keyboard_query(
            shared,
            |remaining| input.next(remaining),
            Instant::now() + KEYBOARD_QUERY_TIMEOUT,
        )
    })();
    input.query_mode(false);
    result
}

#[cfg(test)]
fn collect_keyboard_query(
    shared: &Shared,
    mut next: impl FnMut(Duration) -> io::Result<Option<TerminalQueryItem>>,
    deadline: Instant,
) -> io::Result<bool> {
    collect_owned_keyboard_query(
        shared,
        |remaining| {
            next(remaining).map(|item| {
                item.map(|item| match item {
                    TerminalQueryItem::Event(event) => ReadItem::Event(event, None),
                    TerminalQueryItem::Reply(reply) => ReadItem::Reply(NativeReply {
                        bytes: reply,
                        _storage: None,
                    }),
                })
            })
        },
        deadline,
    )
}

fn collect_owned_keyboard_query(
    shared: &Shared,
    mut next: impl FnMut(Duration) -> io::Result<Option<ReadItem>>,
    deadline: Instant,
) -> io::Result<bool> {
    loop {
        if shared.stop.is_stopped() {
            return Ok(false);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        // Even a reply read owns one slot until it is identified. Ordinary
        // input and Paste move into the exact existing FIFO, without a second
        // read when the queue is full before the UI is available to drain it.
        let Some(slot) = shared.query_slot() else {
            return Ok(false);
        };
        match next(remaining.min(INPUT_POLL_INTERVAL))? {
            Some(ReadItem::Event(event, lease)) => {
                if !enqueue_leased_original(shared, slot, event, lease, Some(deadline)) {
                    return Ok(false);
                }
            }
            Some(ReadItem::Reply(reply)) => {
                drop(slot);
                if is_keyboard_flags_reply(&reply.bytes) {
                    return Ok(true);
                }
                if is_primary_device_reply(&reply.bytes) {
                    return Ok(false);
                }
            }
            None => drop(slot),
        }
    }
}

fn is_keyboard_flags_reply(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x1b[?")
        && bytes.ends_with(b"u")
        && bytes.len() > 4
        && bytes[3..bytes.len() - 1].iter().all(u8::is_ascii_digit)
}

fn is_primary_device_reply(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x1b[?") && bytes.ends_with(b"c")
}

fn probe_terminal_image_support(shared: &Shared, input: &mut InputDriver) -> Option<Picker> {
    let result =
        Picker::from_owned_query_with_options(QueryStdioOptions::default(), |query, timeout| {
            collect_image_query(shared, input, query, timeout)
        });
    match result {
        Ok(picker) => Some(picker),
        Err(error) => {
            tracing::warn!(%error, "terminal image capability query failed");
            None
        }
    }
}

fn collect_image_query(
    shared: &Shared,
    input: &mut InputDriver,
    query: &str,
    timeout: Duration,
) -> Result<Vec<Response>, ImageError> {
    input.query_mode(true);
    let result = collect_owned_image_query(shared, input, query, timeout);
    input.query_mode(false);
    result
}

fn collect_owned_image_query(
    shared: &Shared,
    input: &mut InputDriver,
    query: &str,
    timeout: Duration,
) -> Result<Vec<Response>, ImageError> {
    io::stdout().write_all(query.as_bytes())?;
    io::stdout().flush()?;
    let start = Instant::now();
    let mut parser = ImageParser::new();
    let mut responses = Vec::new();
    loop {
        if shared.stop.is_stopped() || start.elapsed() >= timeout {
            return Err(ImageError::NoStdinResponse);
        }
        // Never read an ordinary event unless its envelope already has a slot.
        // If startup input fills the unchanged queue, end capability detection
        // and let the UI start draining it.
        let slot = shared.query_slot().ok_or(ImageError::NoStdinResponse)?;
        let remaining = timeout
            .saturating_sub(start.elapsed())
            .min(INPUT_POLL_INTERVAL);
        match input.next(remaining)? {
            Some(ReadItem::Event(event, lease)) => {
                if !enqueue_leased_original(shared, slot, event, lease, Some(start + timeout)) {
                    return Err(ImageError::NoStdinResponse);
                }
            }
            Some(ReadItem::Reply(reply)) => {
                drop(slot);
                for byte in reply.bytes.iter().copied() {
                    for response in parser.push(char::from(byte)) {
                        if response == Response::Status {
                            return Ok(responses);
                        }
                        if responses.len() == IMAGE_QUERY_RESPONSE_CAPACITY {
                            return Err(ImageError::NoStdinResponse);
                        }
                        responses.push(response);
                    }
                }
            }
            None => drop(slot),
        }
    }
}

fn read_owned_events(shared: &Shared, input: &mut InputDriver) {
    while let Some(slot) = shared.reserve_slot() {
        let (event, lease) = loop {
            if shared.stop.is_stopped() {
                return;
            }
            match input.next(INPUT_POLL_INTERVAL) {
                Ok(Some(ReadItem::Event(event, lease))) => break (event, lease),
                // A late report is metadata, never ordinary input. Its lease is
                // dropped here after its original reply allocation is dropped.
                Ok(Some(ReadItem::Reply(_))) | Ok(None) => {}
                Err(_) => return, // Driver retained the typed original failure.
            }
        };
        if !enqueue_leased_original(shared, slot, event, lease, None) {
            return;
        }
    }
}

#[cfg(test)]
fn enqueue_original(
    shared: &Shared,
    slot: InputSlot,
    event: Event,
    deadline: Option<Instant>,
) -> bool {
    enqueue_leased_original(shared, slot, event, None, deadline)
}

fn enqueue_leased_original(
    shared: &Shared,
    slot: InputSlot,
    event: Event,
    native_payload: Option<Box<dyn NativeStorageLease>>,
    deadline: Option<Instant>,
) -> bool {
    let payload_bytes = match &event {
        Event::Paste(text) => text.capacity(),
        _ => 0,
    };
    let mut original = InputEvent {
        event: Some(event),
        payload_bytes,
        payload: None,
        native_payload,
        _slot: slot,
    };
    if payload_bytes != 0 && original.native_payload.is_none() {
        loop {
            match shared.quota.reserve_external_storage(payload_bytes) {
                Ok(admission) => {
                    original.payload = Some(admission);
                    break;
                }
                Err(RejectReason::Busy)
                    if !shared.stop.is_stopped()
                        && !deadline.is_some_and(|when| Instant::now() >= when) =>
                {
                    // This same allocation remains in custody; no subsequent
                    // native input read can overtake it.
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(reason) => {
                    shared.fail(InputFailure {
                        kind: InputFailureKind::Refused { reason, original },
                    });
                    return false;
                }
            }
        }
    }
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    state.events.push_back(original);
    drop(state);
    shared.ready.notify_one();
    true
}

pub(crate) struct InputReceiver {
    shared: Arc<Shared>,
}
impl InputReceiver {
    pub(crate) fn close(&mut self) {
        self.shared.close(false);
    }
    pub(crate) fn try_recv(&mut self) -> Result<Result<InputEvent, InputFailure>, TryRecvError> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.retiring || !state.receiver_open {
            return Err(TryRecvError::Disconnected);
        }
        if let Some(event) = state.events.pop_front() {
            drop(state);
            self.shared.available.notify_one();
            return Ok(Ok(event));
        }
        if let Some(failure) = state.failure.take() {
            return Ok(Err(failure));
        }
        if state.producer_done {
            return Err(TryRecvError::Disconnected);
        }
        Err(TryRecvError::Empty)
    }
    pub(crate) async fn recv(&mut self) -> Option<Result<InputEvent, InputFailure>> {
        loop {
            let shared = Arc::clone(&self.shared);
            let notified = shared.ready.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.try_recv() {
                Ok(event) => return Some(event),
                Err(TryRecvError::Disconnected) => return None,
                Err(TryRecvError::Empty) => notified.await,
            }
        }
    }
}
impl Drop for InputReceiver {
    fn drop(&mut self) {
        self.shared.close(false);
    }
}

pub(crate) struct InputOwner {
    worker: OwnedWorker,
    shared: Arc<Shared>,
}
impl InputOwner {
    pub(crate) fn request_stop(&self) {
        self.shared.close(true);
    }
    pub(crate) fn stop(self) -> InputRetirement {
        self.request_stop();
        let ticket = self.worker.ticket();
        drop(self.worker);
        InputRetirement {
            ticket,
            shared: self.shared,
            complete: false,
        }
    }
}

pub struct InputRetirement {
    ticket: WorkerTicket,
    shared: Arc<Shared>,
    complete: bool,
}
impl InputRetirement {
    pub fn try_complete(&mut self) -> Option<InputShutdownReport> {
        if self.complete {
            return None;
        }
        let exit = self.ticket.exit()?;
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let pending = std::mem::take(&mut state.events);
        let failure = state.failure.take();
        let queue_excess = state.queue_excess.take();
        let native = state.native.take();
        drop(state);
        let failure = failure.map(Box::new);
        self.complete = true;
        Some(InputShutdownReport {
            exit,
            pending,
            failure,
            _queue_excess: queue_excess,
            native,
            _storage: Arc::clone(&self.shared.storage),
        })
    }
    pub fn into_deadline(self) -> InputRetirementDeadline {
        InputRetirementDeadline {
            worker_id: self.ticket.id(),
            pending: Mutex::new(self),
        }
    }
}
impl fmt::Debug for InputRetirement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InputRetirement")
            .field("worker_id", &self.ticket.id())
            .field("exit", &self.ticket.exit())
            .field("complete", &self.complete)
            .finish()
    }
}
#[derive(Debug)]
pub struct InputRetirementDeadline {
    worker_id: u64,
    pending: Mutex<InputRetirement>,
}
impl InputRetirementDeadline {
    pub fn into_pending(self) -> InputRetirement {
        self.pending
            .into_inner()
            .unwrap_or_else(|error| error.into_inner())
    }
}
impl fmt::Display for InputRetirementDeadline {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "terminal input worker {} retirement deadline elapsed; original FIFO and native owner retained", self.worker_id)
    }
}
impl std::error::Error for InputRetirementDeadline {}

pub struct InputShutdownReport {
    pub exit: WorkerExit,
    pending: VecDeque<InputEvent>,
    failure: Option<Box<InputFailure>>,
    _queue_excess: Option<StorageAdmission>,
    native: Option<NativeInputCustody>,
    _storage: Arc<InputStorage>,
}
impl InputShutdownReport {
    pub fn native_custody(&self) -> Option<&NativeInputCustody> {
        self.native.as_ref().or_else(|| {
            self.failure
                .as_deref()
                .and_then(InputFailure::native_custody)
        })
    }
    pub fn pending_events(&self) -> impl Iterator<Item = &InputEvent> {
        self.pending.iter()
    }
    pub fn failure(&self) -> Option<&InputFailure> {
        self.failure.as_deref()
    }
    pub fn into_result(self) -> Result<(), InputFailure> {
        if self.exit == WorkerExit::Joined
            && self.pending.is_empty()
            && self.failure.is_none()
            && !self
                .native
                .as_ref()
                .is_some_and(NativeInputCustody::has_retained_input)
        {
            return Ok(());
        }
        Err(InputFailure {
            kind: InputFailureKind::Shutdown(Box::new(self)),
        })
    }
}
impl fmt::Debug for InputShutdownReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InputShutdownReport")
            .field("exit", &self.exit)
            .field("pending_events", &self.pending.len())
            .field(
                "native_retained_input",
                &self
                    .native
                    .as_ref()
                    .is_some_and(NativeInputCustody::has_retained_input),
            )
            .field("failure", &self.failure)
            .finish()
    }
}

#[cfg(test)]
pub(crate) fn paste_fixture(text: String) -> (InputEvent, QuotaGroup) {
    let bytes = text.capacity();
    let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 1,
        worker_bytes: input_storage_bytes() + bytes + 4096,
    });
    let storage = Arc::new(InputStorage {
        active: Mutex::new(1),
        available: Condvar::new(),
        _admission: quota
            .reserve_external_storage(input_storage_bytes())
            .unwrap(),
    });
    let payload = if bytes == 0 {
        None
    } else {
        Some(quota.reserve_external_storage(bytes).unwrap())
    };
    (
        InputEvent {
            event: Some(Event::Paste(text)),
            payload_bytes: bytes,
            payload,
            native_payload: None,
            _slot: InputSlot { storage },
        },
        quota,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc::{self, RecvTimeoutError},
        time::Instant,
    };

    static INPUT_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn quota() -> QuotaGroup {
        QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 1,
            worker_bytes: INPUT_STACK_BYTES + input_storage_bytes() + 2 * 1024 * 1024,
        })
    }

    fn query_test_shared() -> Shared {
        let quota = quota();
        let admission = quota
            .reserve_external_storage(input_storage_bytes())
            .expect("query storage admission");
        Shared {
            state: Mutex::new(State {
                events: VecDeque::with_capacity(INPUT_QUEUE_CAPACITY),
                failure: None,
                producer_done: false,
                receiver_open: true,
                retiring: false,
                queue_excess: None,
                native: None,
            }),
            available: Condvar::new(),
            ready: Notify::new(),
            stop: StopToken::default(),
            quota,
            storage: Arc::new(InputStorage {
                active: Mutex::new(0),
                available: Condvar::new(),
                _admission: admission,
            }),
        }
    }

    #[test]
    fn native_paste_storage_lease_survives_original_consuming_callback() {
        let (event, quota) = paste_fixture("original".into());
        let charged = quota.snapshot().worker_bytes;
        event.dispatch_paste(|text| {
            assert_eq!(text, "original");
            assert_eq!(quota.snapshot().worker_bytes, charged);
            drop(text);
            assert_eq!(quota.snapshot().worker_bytes, charged);
        });
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn keyboard_query_admits_first_key_and_exact_paste_before_flags() {
        let shared = query_test_shared();
        let mut paste = String::from("early paste");
        paste.reserve(1024);
        let paste_pointer = paste.as_ptr();
        let mut items = VecDeque::from([
            TerminalQueryItem::Event(Event::Key(crossterm::event::KeyCode::Char('n').into())),
            TerminalQueryItem::Event(Event::Paste(paste)),
            TerminalQueryItem::Reply(b"\x1b[?1u".to_vec()),
        ]);
        let mut reads = 0;
        let supported = collect_keyboard_query(
            &shared,
            |timeout| {
                assert!(timeout <= INPUT_POLL_INTERVAL);
                reads += 1;
                Ok(items.pop_front())
            },
            Instant::now() + KEYBOARD_QUERY_TIMEOUT,
        )
        .expect("keyboard reply");
        assert!(supported);
        assert_eq!(reads, 3);
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let key = state.events.pop_front().expect("first key retained");
        assert!(
            matches!(key.dispatch(|event| event), Event::Key(key) if key.code == crossterm::event::KeyCode::Char('n'))
        );
        let pasted = state.events.pop_front().expect("first paste retained");
        let Event::Paste(text) = pasted.dispatch(|event| event) else {
            panic!("paste identity");
        };
        assert_eq!(text, "early paste");
        assert_eq!(text.as_ptr(), paste_pointer);
        assert!(state.events.is_empty());
    }

    #[test]
    fn keyboard_query_full_queue_does_not_issue_another_read() {
        let shared = query_test_shared();
        let deadline = Instant::now() + KEYBOARD_QUERY_TIMEOUT;
        for _ in 0..INPUT_QUEUE_CAPACITY {
            let slot = shared.query_slot().expect("admitted slot");
            assert!(enqueue_original(
                &shared,
                slot,
                Event::Resize(1, 1),
                Some(deadline)
            ));
        }
        assert!(!collect_keyboard_query(
            &shared,
            |_| panic!("native read after full queue"),
            deadline
        )
        .expect("full queue resolves unsupported"));
        assert_eq!(
            shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .events
                .len(),
            INPUT_QUEUE_CAPACITY
        );
    }

    #[test]
    fn keyboard_query_cancellation_and_deadline_do_not_read_late_reply() {
        let unsupported_shared = query_test_shared();
        assert!(!collect_keyboard_query(
            &unsupported_shared,
            |_| Ok(Some(TerminalQueryItem::Reply(b"\x1b[?64;4c".to_vec()))),
            Instant::now() + KEYBOARD_QUERY_TIMEOUT,
        )
        .expect("device attributes without flags mean unsupported"));

        let shared = query_test_shared();
        let mut reads = 0;
        let supported = collect_keyboard_query(
            &shared,
            |_| {
                reads += 1;
                shared.stop.stop();
                Ok(None)
            },
            Instant::now() + KEYBOARD_QUERY_TIMEOUT,
        )
        .expect("cancelled query");
        assert!(!supported);
        assert_eq!(reads, 1, "stop prevents another native read");

        let late_shared = query_test_shared();
        let late = collect_keyboard_query(
            &late_shared,
            |_| panic!("late keyboard reply read after timeout"),
            Instant::now(),
        )
        .expect("expired query");
        assert!(!late);
        assert!(is_keyboard_flags_reply(b"\x1b[?0u"));
        assert!(!is_keyboard_flags_reply(b"\x1b[?u"));
        assert!(is_primary_device_reply(b"\x1b[?64;4c"));
    }

    #[test]
    fn admission_refusal_releases_the_exclusive_claim_before_terminal_entry() {
        let _serial = INPUT_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let denied = QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: input_storage_bytes() + INPUT_STACK_BYTES,
        });
        assert!(matches!(
            InputReservation::prepare(&denied),
            Err(InputStartError::Admission(RejectReason::WorkerLimit))
        ));
        assert_eq!(
            denied.snapshot().worker_bytes,
            0,
            "the storage reservation must roll back with rejected startup"
        );
        let allowed = quota();
        let reservation = InputReservation::prepare(&allowed)
            .expect("failed startup released the exclusive terminal claim");
        assert_eq!(allowed.snapshot().worker_threads, 1);
        drop(reservation);
        assert_eq!(allowed.snapshot().worker_threads, 0);
        assert_eq!(allowed.snapshot().worker_bytes, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn full_queue_preserves_every_paste_and_its_original_allocation() {
        let _serial = INPUT_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let quota = quota();
        let reservation = InputReservation::prepare(&quota).expect("input admission");
        let session = reservation.session_claim();
        let (send_event, read_event) = mpsc::channel::<Event>();
        let (read_notice, read_count) = mpsc::channel::<usize>();
        let mut read_number = 0;
        let (owner, mut receiver) = reservation
            .start_with_reader(move |timeout| match read_event.recv_timeout(timeout) {
                Ok(event) => {
                    read_number += 1;
                    let _ = read_notice.send(read_number);
                    Ok(Some(event))
                }
                Err(RecvTimeoutError::Timeout) => Ok(None),
                Err(RecvTimeoutError::Disconnected) => {
                    Err(io::Error::from(io::ErrorKind::BrokenPipe))
                }
            })
            .expect("native input owner");
        let mut first = String::from("paste-000");
        first.reserve(4096);
        let first_pointer = first.as_ptr();
        send_event
            .send(Event::Paste(first))
            .expect("first original");
        for index in 1..=INPUT_QUEUE_CAPACITY {
            send_event
                .send(Event::Paste(format!("paste-{index:03}")))
                .expect("original event");
        }
        for expected in 1..=INPUT_QUEUE_CAPACITY {
            assert_eq!(
                read_count
                    .recv_timeout(Duration::from_secs(5))
                    .expect("read progress"),
                expected
            );
        }
        let queue_deadline = Instant::now() + Duration::from_secs(5);
        while owner
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .events
            .len()
            != INPUT_QUEUE_CAPACITY
        {
            assert!(
                Instant::now() < queue_deadline,
                "reader did not fill the admitted queue"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            owner
                .shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .events
                .len(),
            INPUT_QUEUE_CAPACITY
        );
        assert!(
            read_count.try_recv().is_err(),
            "the 257th native read waits behind a full queue"
        );

        let first = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("first delivery deadline")
            .expect("receiver open")
            .expect("first event");
        assert_eq!(first.unadmitted_payload_bytes(), 0);
        let Event::Paste(first_text) = first.dispatch(|event| event) else {
            panic!("paste identity");
        };
        assert_eq!(first_text, "paste-000");
        assert_eq!(
            first_text.as_ptr(),
            first_pointer,
            "the original paste allocation was moved, not copied"
        );
        assert_eq!(
            read_count
                .recv_timeout(Duration::from_secs(5))
                .expect("next read"),
            INPUT_QUEUE_CAPACITY + 1
        );
        for index in 1..=INPUT_QUEUE_CAPACITY {
            let envelope = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
                .await
                .expect("ordered delivery deadline")
                .expect("receiver open")
                .expect("event");
            let Event::Paste(text) = envelope.dispatch(|event| event) else {
                panic!("paste event");
            };
            assert_eq!(text, format!("paste-{index:03}"));
        }
        let ticket = owner.worker.ticket();
        let mut retirement = owner.stop();
        assert_eq!(
            ticket.join_until(Instant::now() + Duration::from_secs(5)),
            Ok(WorkerExit::Joined)
        );
        retirement
            .try_complete()
            .expect("retirement report")
            .into_result()
            .expect("no originals remain undispatched");
        drop(receiver);
        drop(session);
        assert_eq!(quota.snapshot().worker_threads, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blocked_read_and_receiver_closure_retain_the_original_for_retirement() {
        let _serial = INPUT_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let quota = quota();
        let reservation = InputReservation::prepare(&quota).expect("input admission");
        let session = reservation.session_claim();
        let (read_started, started) = mpsc::channel();
        let (send_event, read_event) = mpsc::channel::<Event>();
        let (owner, mut receiver) = reservation
            .start_with_reader(move |_| {
                let _ = read_started.send(());
                read_event.recv().map(Some).map_err(io::Error::other)
            })
            .expect("reader");
        started
            .recv_timeout(Duration::from_secs(5))
            .expect("native read blocked");
        receiver.close();
        let ticket = owner.worker.ticket();
        let mut retirement = owner.stop();
        assert!(
            retirement.try_complete().is_none(),
            "blocked native read is not a join"
        );
        let deadline = retirement.into_deadline();
        assert!(deadline
            .to_string()
            .contains("original FIFO and native owner retained"));
        let mut retirement = deadline.into_pending();
        assert!(
            retirement.try_complete().is_none(),
            "deadline preserves the pending native ticket"
        );
        assert!(matches!(
            InputReservation::prepare(&quota),
            Err(InputStartError::AlreadyOwned)
        ));
        let mut paste = String::from("after-close");
        paste.reserve(1024);
        let pointer = paste.as_ptr();
        send_event
            .send(Event::Paste(paste))
            .expect("release read with original");
        assert_eq!(
            ticket.join_until(Instant::now() + Duration::from_secs(5)),
            Ok(WorkerExit::Joined)
        );
        let report = retirement.try_complete().expect("actual native retirement");
        let pending: Vec<_> = report.pending_events().collect();
        assert_eq!(pending.len(), 1);
        assert!(
            matches!(pending[0].view(), Event::Paste(text) if text == "after-close" && text.as_ptr() == pointer)
        );
        assert!(
            report.into_result().is_err(),
            "undispatched original is explicit failure custody"
        );
        drop(session);
        drop(receiver);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert!(
            InputReservation::prepare(&quota).is_ok(),
            "claim releases only after native join and presentation owner drop"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_failure_is_delivered_once_and_does_not_become_normal_end_of_stream() {
        let _serial = INPUT_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let quota = quota();
        let reservation = InputReservation::prepare(&quota).expect("input admission");
        let (owner, mut receiver) = reservation
            .start_with_reader(|_| {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "synthetic native read failure",
                ))
            })
            .expect("reader");
        let failure = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("failure delivery deadline")
            .expect("one failure")
            .expect_err("read failed");
        assert!(failure
            .to_string()
            .contains("synthetic native read failure"));
        assert!(failure.original().is_none());
        assert!(receiver.recv().await.is_none(), "failure is not repeated");
        let ticket = owner.worker.ticket();
        let mut retirement = owner.stop();
        assert_eq!(
            ticket.join_until(Instant::now() + Duration::from_secs(5)),
            Ok(WorkerExit::Joined)
        );
        assert!(retirement
            .try_complete()
            .expect("joined")
            .pending_events()
            .next()
            .is_none());
    }
}

#[cfg(test)]
#[path = "terminal_input_native_tests.rs"]
mod native_tests;

#[cfg(test)]
#[path = "terminal_input_pty_child_tests.rs"]
mod pty_child_tests;
