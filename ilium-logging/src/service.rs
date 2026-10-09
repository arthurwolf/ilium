//! One ordered file owner. Admission never waits for queue space or filesystem I/O.
//! The short queue mutex protects bookkeeping only.
use crate::LogDestination;
use crate::LoggingError;
use ilium_execution::{
    reserve_admitted_worker, QuotaGroup, RejectReason, StorageAdmission, WorkerAdmission,
};
use ilium_platform::{
    owned_worker::{
        worker_retained_state_bytes, OwnedWorker, StopToken, WorkerExit, WorkerKind,
        WorkerReservation, WorkerTicket,
    },
    secure_fs,
};
use std::{
    collections::VecDeque,
    fs::File,
    future::Future,
    io::{self, Write},
    mem::size_of,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc, Condvar, Mutex,
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

pub const MAX_EVENT_BYTES: usize = 1024 * 1024;
pub const MAX_RETAINED_BYTES: usize = 8 * 1024 * 1024;
const MAX_COMMANDS: usize = 256;
const CONTROL_RESERVE: usize = 32;
/// An explicit Builder request; this is not a native stack/RSS allocation bound.
pub const LOGGER_STACK_BYTES: usize = 2 * 1024 * 1024;

/// Visible Rust metadata, requested queue slots, and the original event envelope.
/// Allocator rounding, std channel internals, tracing filters, TLS and OS buffers
/// require native qualification and are not measured by these declarations.
pub fn logger_storage_bytes(path: &Path) -> Result<usize, LoggingError> {
    let fixed = MAX_RETAINED_BYTES
        + MAX_COMMANDS * size_of::<Command>()
        + size_of::<Shared>()
        + size_of::<crate::LoggerState>()
        + size_of::<crate::RetiringLogger>()
        + 4 * size_of::<usize>()
        + worker_retained_state_bytes()
        + size_of::<Arc<Shared>>()
        + 2 * size_of::<usize>()
        + size_of::<WorkerAdmission>()
        + size_of::<AtomicBool>()
        + 2 * size_of::<usize>();
    path.as_os_str()
        .as_encoded_bytes()
        .len()
        .checked_mul(2)
        .and_then(|paths| fixed.checked_add(paths))
        .ok_or(LoggingError::StorageAdmissionRejected(
            RejectReason::InvalidCost,
        ))
}

/// A separate declaration follows each control receipt and any returned I/O
/// error. Completing a control does not make retained receipts free storage.
/// The common declaration includes the larger joined-shutdown/deadline owner.
pub fn logger_control_bytes(path: &Path) -> Result<usize, LoggingError> {
    control_bytes(path.as_os_str().as_encoded_bytes().len())
}

fn control_bytes(path_bytes: usize) -> Result<usize, LoggingError> {
    let fixed = size_of::<LoggingReceipt>()
        + size_of::<Acknowledgement>()
        + size_of::<Result<(), LoggingError>>()
        + size_of::<Mutex<Option<Waker>>>()
        + size_of::<RetainedIoError>()
        + size_of::<StorageAdmission>()
        + size_of::<LoggingShutdownDeadline>()
        + size_of::<LoggingShutdownReport>()
        + 4 * size_of::<usize>();
    fixed
        .checked_add(path_bytes)
        .ok_or(LoggingError::StorageAdmissionRejected(
            RejectReason::InvalidCost,
        ))
}

#[derive(Clone)]
struct LoggingStorage {
    _admission: Arc<StorageAdmission>,
}
impl std::fmt::Debug for LoggingStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LoggingStorage(retained)")
    }
}
#[derive(Debug)]
struct RetainedIoError {
    source: io::Error,
    _storage: LoggingStorage,
}
impl std::fmt::Display for RetainedIoError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.source, formatter)
    }
}
impl std::error::Error for RetainedIoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
fn retain_io(source: io::Error, storage: &LoggingStorage) -> io::Error {
    // Preserve the native kind and original source while retaining the copied
    // path/control declaration through an escaped PrepareFile or Io error.
    io::Error::new(
        source.kind(),
        RetainedIoError {
            source,
            _storage: storage.clone(),
        },
    )
}

fn storage_error(reason: RejectReason) -> LoggingError {
    if reason == RejectReason::Busy {
        return LoggingError::AdmissionBusy;
    }
    LoggingError::StorageAdmissionRejected(reason)
}

/// Completion means preceding accepted events were processed, not merely queued.
pub struct LoggingReceipt {
    receiver: Receiver<Result<(), LoggingError>>,
    wake: Arc<Mutex<Option<Waker>>>,
    _storage: LoggingStorage,
}
impl LoggingReceipt {
    pub fn try_recv(&self) -> Result<Option<Result<(), LoggingError>>, LoggingError> {
        match self.receiver.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(LoggingError::WorkerStopped),
        }
    }
    /// Startup/process-exit adapter only; never call from interactive loops.
    pub fn wait_timeout(self, timeout: Duration) -> Result<(), LoggingError> {
        self.receiver
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => LoggingError::Deadline,
                mpsc::RecvTimeoutError::Disconnected => LoggingError::WorkerStopped,
            })?
    }
}

/// The ordered acknowledgement and native retirement are separate observations.
/// Dropping this observer does not cancel the already accepted drain command.
pub struct LoggingShutdown {
    receipt: Option<LoggingReceipt>,
    ordered: Option<Result<(), LoggingError>>,
    ticket: WorkerTicket,
    complete: bool,
    _storage: Option<LoggingStorage>,
}
impl std::fmt::Debug for LoggingShutdown {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoggingShutdown")
            .field("worker_id", &self.ticket.id())
            .field("ordered", &self.ordered)
            .field("exit", &self.ticket.exit())
            .field("complete", &self.complete)
            .finish()
    }
}
#[derive(Debug)]
pub struct LoggingShutdownReport {
    pub ordered: Result<(), LoggingError>,
    pub exit: WorkerExit,
    _storage: Option<LoggingStorage>,
}
impl LoggingShutdownReport {
    pub fn into_result(self) -> Result<(), LoggingError> {
        self.ordered?;
        if self.exit == WorkerExit::Panicked {
            return Err(LoggingError::WorkerPanicked);
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct LoggingShutdownDeadline {
    worker_id: u64,
    pending: Mutex<LoggingShutdown>,
}
impl LoggingShutdownDeadline {
    pub fn into_pending(self) -> LoggingShutdown {
        self.pending
            .into_inner()
            .unwrap_or_else(|error| error.into_inner())
    }
}
impl std::fmt::Display for LoggingShutdownDeadline {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "logging worker {} retirement deadline elapsed; original observer and ordered outcome retained", self.worker_id)
    }
}
impl std::error::Error for LoggingShutdownDeadline {}
impl LoggingShutdown {
    pub(crate) fn retiring(
        ticket: WorkerTicket,
        quota: &QuotaGroup,
        path_bytes: usize,
        error: LoggingError,
    ) -> Result<Self, LoggingError> {
        let admission = quota
            .reserve_external_storage(control_bytes(path_bytes)?)
            .map_err(storage_error)?;
        let storage = LoggingStorage {
            _admission: Arc::new(admission),
        };
        Ok(Self {
            receipt: None,
            ordered: Some(Err(error)),
            ticket,
            complete: false,
            _storage: Some(storage),
        })
    }
    /// Nonblocking observation for an existing async timer/select loop. This
    /// performs no native join and does not spawn a helper to wait for one.
    pub fn try_complete(&mut self) -> Option<LoggingShutdownReport> {
        if self.complete {
            return None;
        }
        if self.ordered.is_none() {
            let receipt = self.receipt.as_ref()?;
            match receipt.try_recv() {
                Ok(Some(result)) => self.ordered = Some(result),
                Err(error) => self.ordered = Some(Err(error)),
                Ok(None) => return None,
            }
            self.receipt.take();
        }
        let exit = self.ticket.exit()?;
        let ordered = self.ordered.take()?;
        self.complete = true;
        crate::clear_retired_initialization(self.ticket.id());
        Some(LoggingShutdownReport {
            ordered,
            exit,
            _storage: self._storage.clone(),
        })
    }
    pub fn into_deadline(self) -> LoggingShutdownDeadline {
        LoggingShutdownDeadline {
            worker_id: self.ticket.id(),
            pending: Mutex::new(self),
        }
    }
    /// Process-exit/background only. Both waits consume the same absolute
    /// deadline, and an ordered write failure still waits for actual join.
    pub fn wait_until(
        mut self,
        deadline: Instant,
    ) -> Result<LoggingShutdownReport, LoggingShutdownDeadline> {
        if self.ordered.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let outcome = self
                .receipt
                .as_ref()
                .map(|receipt| receipt.receiver.recv_timeout(remaining));
            match outcome {
                Some(Ok(result)) => self.ordered = Some(result),
                Some(Err(mpsc::RecvTimeoutError::Disconnected)) => {
                    self.ordered = Some(Err(LoggingError::WorkerStopped))
                }
                Some(Err(mpsc::RecvTimeoutError::Timeout)) | None => {
                    return Err(self.into_deadline())
                }
            }
            self.receipt.take();
        }
        let exit = match self.ticket.join_until(deadline) {
            Ok(exit) => exit,
            Err(_) => return Err(self.into_deadline()),
        };
        let ordered = self
            .ordered
            .take()
            .unwrap_or(Err(LoggingError::WorkerStopped));
        crate::clear_retired_initialization(self.ticket.id());
        Ok(LoggingShutdownReport {
            ordered,
            exit,
            _storage: self._storage.take(),
        })
    }
}

impl Future for LoggingReceipt {
    type Output = Result<(), LoggingError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        // Completion sends before taking this same mutex. Holding it across
        // the ready check and registration prevents a lost wakeup.
        let mut wake = self.wake.lock().unwrap_or_else(|error| error.into_inner());
        match self.receiver.try_recv() {
            Ok(result) => Poll::Ready(result),
            Err(TryRecvError::Disconnected) => Poll::Ready(Err(LoggingError::WorkerStopped)),
            Err(TryRecvError::Empty) => {
                if wake
                    .as_ref()
                    .is_none_or(|old| !old.will_wake(context.waker()))
                {
                    *wake = Some(context.waker().clone());
                }
                Poll::Pending
            }
        }
    }
}

struct Acknowledgement {
    sender: Option<SyncSender<Result<(), LoggingError>>>,
    wake: Arc<Mutex<Option<Waker>>>,
    // Release only after the channel endpoint and wake capture are destroyed.
    storage: LoggingStorage,
}

impl Acknowledgement {
    fn try_send(
        &self,
        result: Result<(), LoggingError>,
    ) -> Result<(), mpsc::TrySendError<Result<(), LoggingError>>> {
        let outcome = match self.sender.as_ref() {
            Some(sender) => sender.try_send(result),
            None => Err(mpsc::TrySendError::Disconnected(result)),
        };
        self.notify();
        outcome
    }

    fn notify(&self) {
        let wake = self
            .wake
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}

impl Drop for Acknowledgement {
    fn drop(&mut self) {
        // Close before waking: otherwise a resumed future could observe an
        // empty, still-open channel and register after its last wakeup.
        self.sender.take();
        self.notify();
    }
}
#[derive(Debug, Clone, Copy)]
pub struct LoggingHealth {
    pub retained_bytes: usize,
    pub queued_commands: usize,
    pub worker_id: u64,
    pub dropped_events: u64,
    pub failed_operations: u64,
    pub accepting: bool,
    pub worker_alive: bool,
}
enum Command {
    Event(Vec<u8>),
    Enable(bool, Acknowledgement),
    Flush(Acknowledgement),
    Shutdown(Acknowledgement),
}
struct Queue {
    commands: VecDeque<Command>,
    accepting: bool,
}
struct Shared {
    quota: QuotaGroup,
    path_bytes: usize,
    queue: Mutex<Queue>,
    changed: Condvar,
    enabled: Arc<AtomicBool>,
    bytes: AtomicUsize,
    dropped: AtomicU64,
    failures: AtomicU64,
    alive: AtomicBool,
    accepting: AtomicBool,
    pending: AtomicUsize,
    // Field order keeps queue/formatter state alive inside its admission.
    _storage: StorageAdmission,
}
pub(crate) struct Service {
    shared: Arc<Shared>,
    _worker: OwnedWorker,
}
pub(crate) struct ServiceAdmission {
    worker: WorkerReservation<WorkerAdmission>,
    storage: StorageAdmission,
    quota: QuotaGroup,
}
impl ServiceAdmission {
    pub(crate) fn start(
        self,
        path: PathBuf,
        destination: LogDestination,
        enabled: Arc<AtomicBool>,
        initial_file: Option<Box<dyn Write + Send>>,
        before_run: impl FnOnce() + Send + 'static,
    ) -> Result<Service, LoggingError> {
        let mut commands = VecDeque::new();
        commands
            .try_reserve_exact(MAX_COMMANDS)
            .map_err(|error| LoggingError::Io(io::Error::other(error)))?;
        let shared = Arc::new(Shared {
            quota: self.quota,
            path_bytes: path.as_os_str().as_encoded_bytes().len(),
            _storage: self.storage,
            queue: Mutex::new(Queue {
                commands,
                accepting: true,
            }),
            changed: Condvar::new(),
            enabled,
            bytes: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            alive: AtomicBool::new(true),
            accepting: AtomicBool::new(true),
            pending: AtomicUsize::new(0),
        });
        let body = Arc::clone(&shared);
        let wake = Arc::clone(&shared);
        let worker = self
            .worker
            .spawn(
                "ilium-log-writer",
                WorkerKind::SynchronousIo,
                StopToken::default(),
                move || wake.changed.notify_all(),
                move |stop| {
                    let _exit = ExitGuard(Arc::clone(&body));
                    before_run();
                    run(path, destination, body, stop, initial_file);
                },
            )
            .map_err(LoggingError::Io)?;
        Ok(Service {
            shared,
            _worker: worker,
        })
    }
}
impl Service {
    pub(crate) fn prepare(
        quota: &QuotaGroup,
        path: &Path,
    ) -> Result<ServiceAdmission, LoggingError> {
        let storage = quota
            .reserve_external_storage(logger_storage_bytes(path)?)
            .map_err(storage_error)?;
        let worker = reserve_admitted_worker(quota, LOGGER_STACK_BYTES, 0)
            .map_err(LoggingError::WorkerAdmission)?;
        Ok(ServiceAdmission {
            worker,
            storage,
            quota: quota.clone(),
        })
    }
    #[cfg(test)]
    pub fn new(path: PathBuf, enabled: Arc<AtomicBool>) -> Result<Self, LoggingError> {
        Self::start(path, enabled, None, || {})
    }
    #[cfg(test)]
    fn start(
        path: PathBuf,
        enabled: Arc<AtomicBool>,
        initial_file: Option<Box<dyn Write + Send>>,
        before_run: impl FnOnce() + Send + 'static,
    ) -> Result<Self, LoggingError> {
        let quota = fixture_quota(&path)?;
        Self::prepare(&quota, &path)?.start(
            path,
            LogDestination::LocalFile,
            enabled,
            initial_file,
            before_run,
        )
    }
    pub(crate) fn ticket(&self) -> WorkerTicket {
        self._worker.ticket()
    }
    pub fn reserve(&self, bytes: usize) -> bool {
        self.shared
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(bytes)
                    .filter(|sum| *sum <= MAX_RETAINED_BYTES)
            })
            .is_ok()
    }
    pub fn release(&self, bytes: usize) {
        self.shared.bytes.fetch_sub(bytes, Ordering::AcqRel);
    }
    pub fn dropped(&self) {
        self.shared.dropped.fetch_add(1, Ordering::Relaxed);
    }
    pub fn failed(&self) {
        self.shared.failures.fetch_add(1, Ordering::Relaxed);
    }
    pub fn event(&self, bytes: Vec<u8>) -> bool {
        // This mutex protects only bounded queue bookkeeping, never file I/O.
        // Contention alone must not discard ordinary diagnostics.
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if queue.accepting && queue.commands.len() < MAX_COMMANDS - CONTROL_RESERVE {
            queue.commands.push_back(Command::Event(bytes));
            self.shared.pending.fetch_add(1, Ordering::Relaxed);
            self.shared.changed.notify_one();
            return true;
        }
        drop(queue);
        self.release(bytes.capacity());
        self.dropped();
        false
    }
    fn control(
        &self,
        make: impl FnOnce(Acknowledgement) -> Command,
        shutdown: bool,
    ) -> Result<LoggingReceipt, LoggingError> {
        if !self.shared.alive.load(Ordering::Acquire)
            || !self.shared.accepting.load(Ordering::Acquire)
        {
            return Err(LoggingError::WorkerStopped);
        }
        let storage = self
            .shared
            .quota
            .reserve_external_storage(control_bytes(self.shared.path_bytes)?)
            .map_err(storage_error)?;
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !queue.accepting {
            return Err(LoggingError::WorkerStopped);
        }
        if queue.commands.len() >= MAX_COMMANDS {
            return Err(LoggingError::AdmissionBusy);
        }
        let storage = LoggingStorage {
            _admission: Arc::new(storage),
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let wake = Arc::new(Mutex::new(None));
        queue.commands.push_back(make(Acknowledgement {
            storage: storage.clone(),
            sender: Some(sender),
            wake: Arc::clone(&wake),
        }));
        self.shared.pending.fetch_add(1, Ordering::Relaxed);
        if shutdown {
            queue.accepting = false;
            self.shared.accepting.store(false, Ordering::Release);
        }
        self.shared.changed.notify_one();
        Ok(LoggingReceipt {
            receiver,
            wake,
            _storage: storage,
        })
    }
    pub fn enable(&self, enabled: bool) -> Result<LoggingReceipt, LoggingError> {
        self.control(|sender| Command::Enable(enabled, sender), false)
    }
    pub fn flush(&self) -> Result<LoggingReceipt, LoggingError> {
        self.control(Command::Flush, false)
    }
    pub fn shutdown(&self) -> Result<LoggingReceipt, LoggingError> {
        self.control(Command::Shutdown, true)
    }
    pub fn shutdown_joined(&self) -> Result<LoggingShutdown, LoggingError> {
        let receipt = match self.shutdown() {
            Ok(receipt) => receipt,
            Err(LoggingError::WorkerStopped) => {
                return LoggingShutdown::retiring(
                    self.ticket(),
                    &self.shared.quota,
                    self.shared.path_bytes,
                    LoggingError::WorkerStopped,
                );
            }
            Err(error) => return Err(error),
        };
        let storage = receipt._storage.clone();
        Ok(LoggingShutdown {
            receipt: Some(receipt),
            ordered: None,
            ticket: self.ticket(),
            complete: false,
            _storage: Some(storage),
        })
    }
    pub fn health(&self) -> LoggingHealth {
        LoggingHealth {
            retained_bytes: self.shared.bytes.load(Ordering::Acquire),
            queued_commands: self.shared.pending.load(Ordering::Relaxed),
            worker_id: self._worker.ticket().id(),
            dropped_events: self.shared.dropped.load(Ordering::Relaxed),
            failed_operations: self.shared.failures.load(Ordering::Relaxed),
            accepting: self.shared.accepting.load(Ordering::Acquire),
            worker_alive: self.shared.alive.load(Ordering::Acquire),
        }
    }
}
fn open(path: &Path, storage: &LoggingStorage) -> Result<File, LoggingError> {
    let prepare = || -> io::Result<File> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "log path has no parent"))?;
        secure_fs::create_private_directory(parent)?;
        let file = secure_fs::private_open_options()
            .create(true)
            .append(true)
            .open(path)?;
        secure_fs::restrict_open_file_to_owner(&file)?;
        Ok(file)
    };
    prepare().map_err(|source| LoggingError::PrepareFile {
        path: path.to_owned(),
        source: retain_io(source, storage),
    })
}
fn open_destination(
    path: &Path,
    destination: &LogDestination,
    storage: &LoggingStorage,
) -> Result<Box<dyn Write + Send>, LoggingError> {
    match destination {
        LogDestination::LocalFile => {
            open(path, storage).map(|file| Box::new(file) as Box<dyn Write + Send>)
        }
        LogDestination::Relay(endpoint) => crate::relay::RelayWriter::new(endpoint.clone())
            .map(|writer| Box::new(writer) as Box<dyn Write + Send>)
            .map_err(|error| LoggingError::Relay(error.to_string())),
    }
}
fn flush(
    file: &mut Option<Box<dyn Write + Send>>,
    failed: &mut bool,
    storage: &LoggingStorage,
) -> Result<(), LoggingError> {
    let result = file.as_mut().map_or(Ok(()), |file| {
        file.flush()
            .map_err(|error| LoggingError::Io(retain_io(error, storage)))
    });
    if result.is_err() {
        *failed = true;
    }
    result?;
    if *failed {
        return Err(LoggingError::PriorWriteFailed);
    }
    Ok(())
}
struct ExitGuard(Arc<Shared>);
impl Drop for ExitGuard {
    fn drop(&mut self) {
        let shared = &self.0;
        if std::thread::panicking() {
            shared.failures.fetch_add(1, Ordering::Relaxed);
        }
        shared.enabled.store(false, Ordering::Release);
        shared.alive.store(false, Ordering::Release);
        shared.accepting.store(false, Ordering::Release);
        let mut queue = shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.accepting = false;
        let commands = std::mem::take(&mut queue.commands);
        drop(queue);
        // Receipt wakeups may schedule arbitrary executor work. Never invoke
        // them while holding the admission mutex.
        for command in commands {
            shared.pending.fetch_sub(1, Ordering::Relaxed);
            match command {
                Command::Event(bytes) => {
                    shared.bytes.fetch_sub(bytes.capacity(), Ordering::AcqRel);
                    shared.dropped.fetch_add(1, Ordering::Relaxed);
                }
                Command::Enable(_, sender) | Command::Flush(sender) | Command::Shutdown(sender) => {
                    let _ = sender.try_send(Err(LoggingError::WorkerStopped));
                }
            }
        }
    }
}
// The in-progress event stays charged even if a writer panics. Cleanup is
// independent of the queue's cancellation guard.
struct ActiveEvent<'a> {
    bytes: Vec<u8>,
    shared: &'a Shared,
}
impl Drop for ActiveEvent<'_> {
    fn drop(&mut self) {
        self.shared
            .bytes
            .fetch_sub(self.bytes.capacity(), Ordering::AcqRel);
    }
}
fn run(
    path: PathBuf,
    destination: LogDestination,
    shared: Arc<Shared>,
    stop: StopToken,
    mut file: Option<Box<dyn Write + Send>>,
) {
    let mut failed = false;
    loop {
        let command = {
            let mut queue = shared
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while queue.commands.is_empty() && !stop.is_stopped() {
                queue = shared
                    .changed
                    .wait_timeout(queue, Duration::from_millis(100))
                    .unwrap_or_else(|error| error.into_inner())
                    .0;
            }
            if stop.is_stopped() {
                break;
            }
            let command = queue.commands.pop_front();
            if command.is_some() {
                shared.pending.fetch_sub(1, Ordering::Relaxed);
            }
            command
        };
        let Some(command) = command else {
            continue;
        };
        let (result, sender, shutdown) = match command {
            Command::Event(bytes) => {
                let event = ActiveEvent {
                    bytes,
                    shared: &shared,
                };
                if let Some(file) = file.as_mut() {
                    // Never retry a partial write: its prefix may already be durable.
                    if file.write_all(&event.bytes).is_err() {
                        failed = true;
                        shared.failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
                if file.is_none() {
                    shared.dropped.fetch_add(1, Ordering::Relaxed);
                }
                continue;
            }
            Command::Enable(enabled, sender) => {
                let result = if enabled {
                    if file.is_some() {
                        Ok(())
                    } else {
                        open_destination(&path, &destination, &sender.storage).map(|opened| {
                            file = Some(opened);
                            failed = false;
                        })
                    }
                } else {
                    let result = flush(&mut file, &mut failed, &sender.storage);
                    file = None;
                    shared.enabled.store(false, Ordering::Release);
                    result
                };
                if result.is_ok() {
                    shared.enabled.store(enabled, Ordering::Release);
                }
                tracing::callsite::rebuild_interest_cache();
                (result, sender, false)
            }
            Command::Flush(sender) => (
                flush(&mut file, &mut failed, &sender.storage),
                sender,
                false,
            ),
            Command::Shutdown(sender) => {
                shared.enabled.store(false, Ordering::Release);
                let result = flush(&mut file, &mut failed, &sender.storage);
                file = None;
                (result, sender, true)
            }
        };
        if result.is_err() {
            shared.failures.fetch_add(1, Ordering::Relaxed);
        }
        let _ = sender.try_send(result);
        if shutdown {
            break;
        }
    }
}

#[cfg(test)]
pub(crate) fn fixture_quota(path: &Path) -> Result<QuotaGroup, LoggingError> {
    let controls = logger_control_bytes(path)?
        .checked_mul(MAX_COMMANDS + 2)
        .ok_or(LoggingError::StorageAdmissionRejected(
            RejectReason::InvalidCost,
        ))?;
    let worker_bytes = logger_storage_bytes(path)?
        .checked_add(LOGGER_STACK_BYTES)
        .and_then(|bytes| bytes.checked_add(controls))
        .ok_or(LoggingError::StorageAdmissionRejected(
            RejectReason::InvalidCost,
        ))?;
    Ok(QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 1,
        worker_bytes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn logger_refuses_worker_before_opening_or_spawning_and_rolls_back_storage() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("never-opened.log");
        let storage = logger_storage_bytes(&path).expect("declaration");
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: storage + LOGGER_STACK_BYTES,
        });
        assert!(matches!(
            Service::prepare(&quota, &path),
            Err(LoggingError::WorkerAdmission(
                ilium_execution::WorkerStartError::Rejected(RejectReason::WorkerLimit)
            ))
        ));
        assert!(!path.exists(), "rejected startup must not create a log");
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(
            quota.snapshot().worker_bytes,
            0,
            "the earlier storage lease must roll back after worker refusal"
        );
    }

    #[test]
    fn asynchronous_receipt_wakes_after_ordered_worker_completion() {
        use std::future::Future;
        use std::task::{Context, Poll, Wake, Waker};

        struct CompletionWake(SyncSender<()>);
        impl Wake for CompletionWake {
            fn wake(self: Arc<Self>) {
                let _ = self.0.try_send(());
            }
        }

        let directory = tempfile::tempdir().expect("directory");
        let (service, release, identity) = parked_service(directory.path().join("async.log"));
        let worker = identity
            .recv_timeout(Duration::from_secs(5))
            .expect("started");
        let (wake_sender, wake_receiver) = mpsc::sync_channel(1);
        let waker = Waker::from(Arc::new(CompletionWake(wake_sender)));
        let mut context = Context::from_waker(&waker);
        let mut receipt = Box::pin(service.enable(true).expect("admitted"));
        assert!(matches!(
            Future::poll(receipt.as_mut(), &mut context),
            Poll::Pending
        ));
        release.send(worker).expect("release");
        wake_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("completion woke executor");
        assert!(matches!(
            Future::poll(receipt.as_mut(), &mut context),
            Poll::Ready(Ok(()))
        ));
        service
            .shutdown()
            .expect("shutdown")
            .wait_timeout(Duration::from_secs(5))
            .expect("drained");
    }

    fn parked_service(
        path: PathBuf,
    ) -> (
        Service,
        SyncSender<std::thread::ThreadId>,
        Receiver<std::thread::ThreadId>,
    ) {
        let (release, gate) = mpsc::sync_channel(1);
        let (started, thread_identity) = mpsc::sync_channel(1);
        let service = Service::start(path, Arc::new(AtomicBool::new(false)), None, move || {
            started
                .send(std::thread::current().id())
                .expect("thread identity");
            gate.recv().expect("release worker");
        })
        .expect("service");
        (service, release, thread_identity)
    }

    #[test]
    fn bounded_queue_preserves_controls_and_orders_shutdown_after_events() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("ordered.log");
        let (service, release, thread_identity) = parked_service(path.clone());
        let worker_thread = thread_identity
            .recv_timeout(Duration::from_secs(5))
            .expect("worker started");
        assert_ne!(worker_thread, std::thread::current().id());
        let enable = service.enable(true).expect("enable admission");
        // Worker is definitely parked: admission cannot depend on its progress.
        for _ in 0..MAX_COMMANDS - CONTROL_RESERVE - 1 {
            assert!(service.reserve(2));
            service.event(b"x\n".to_vec());
        }
        assert!(service.reserve(2));
        service.event(b"overflow must disappear"[..2].to_vec());
        assert_eq!(service.health().dropped_events, 1);
        let shutdown = service.shutdown().expect("reserved shutdown slot");
        assert!(matches!(service.flush(), Err(LoggingError::WorkerStopped)));
        assert!(shutdown.try_recv().expect("receipt health").is_none());
        release.send(worker_thread).expect("release");
        enable.wait_timeout(Duration::from_secs(5)).expect("enable");
        shutdown
            .wait_timeout(Duration::from_secs(5))
            .expect("shutdown");
        let contents = std::fs::read_to_string(path).expect("readback");
        assert_eq!(contents.lines().count(), MAX_COMMANDS - CONTROL_RESERVE - 1);
        assert!(contents.lines().all(|line| line == "x"));
        assert_eq!(service.health().retained_bytes, 0);
    }

    #[test]
    fn byte_admission_is_bounded_across_inflight_formatters_and_queue() {
        let directory = tempfile::tempdir().expect("directory");
        let (service, release, identity) = parked_service(directory.path().join("budget.log"));
        let worker = identity
            .recv_timeout(Duration::from_secs(5))
            .expect("started");
        assert!(service.reserve(MAX_RETAINED_BYTES));
        assert!(!service.reserve(1));
        assert_eq!(service.health().retained_bytes, MAX_RETAINED_BYTES);
        service.release(MAX_RETAINED_BYTES);
        assert!(service.reserve(1));
        service.release(1);
        let shutdown = service.shutdown().expect("shutdown");
        release.send(worker).expect("release");
        shutdown
            .wait_timeout(Duration::from_secs(5))
            .expect("drained");
    }

    #[test]
    fn logical_drop_does_not_join_a_parked_worker_and_its_ticket_remains_owned() {
        let directory = tempfile::tempdir().expect("directory");
        let (service, release, identity) = parked_service(directory.path().join("drop.log"));
        let worker = identity
            .recv_timeout(Duration::from_secs(5))
            .expect("started");
        let ticket = service._worker.ticket();
        let pending = service.flush().expect("queued receipt");
        let started = Instant::now();
        drop(service);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(ticket.exit(), None);
        release.send(worker).expect("release");
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(5))
            .is_ok());
        assert!(matches!(
            pending.wait_timeout(Duration::from_secs(5)),
            Err(LoggingError::WorkerStopped)
        ));
    }

    struct PartialFailure {
        appended: Arc<Mutex<Vec<u8>>>,
        attempts: Arc<AtomicUsize>,
    }
    impl Write for PartialFailure {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.attempts.fetch_add(1, Ordering::Relaxed) == 0 {
                let prefix = bytes.len().min(3);
                self.appended
                    .lock()
                    .expect("append state")
                    .extend_from_slice(&bytes[..prefix]);
                return Ok(prefix);
            }
            Err(io::Error::other("forced failure after appended prefix"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn partial_write_is_never_retried_and_flush_reports_prior_failure() {
        let directory = tempfile::tempdir().expect("directory");
        let appended = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let sink = PartialFailure {
            appended: Arc::clone(&appended),
            attempts: Arc::clone(&attempts),
        };
        let service = Service::start(
            directory.path().join("unused.log"),
            Arc::new(AtomicBool::new(true)),
            Some(Box::new(sink)),
            || {},
        )
        .expect("service");
        assert!(service.reserve(6));
        service.event(b"abcdef".to_vec());
        assert!(matches!(
            service
                .flush()
                .expect("flush admission")
                .wait_timeout(Duration::from_secs(5)),
            Err(LoggingError::PriorWriteFailed)
        ));
        assert!(matches!(
            service
                .shutdown()
                .expect("shutdown admission")
                .wait_timeout(Duration::from_secs(5)),
            Err(LoggingError::PriorWriteFailed)
        ));
        assert_eq!(&*appended.lock().expect("readback"), b"abc");
        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        assert_eq!(service.health().retained_bytes, 0);
    }

    #[test]
    fn failed_write_remains_visible_after_ordered_flush_and_actual_join() {
        let directory = tempfile::tempdir().expect("directory");
        let appended = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let sink = PartialFailure {
            appended: Arc::clone(&appended),
            attempts: Arc::clone(&attempts),
        };
        let service = Service::start(
            directory.path().join("injected.log"),
            Arc::new(AtomicBool::new(true)),
            Some(Box::new(sink)),
            || {},
        )
        .expect("native logger");
        let quota = service.shared.quota.clone();
        assert!(service.reserve(6));
        service.event(b"abcdef".to_vec());
        let shutdown = service.shutdown_joined().expect("ordered shutdown");
        assert_eq!(quota.snapshot().worker_threads, 1);
        let report = shutdown
            .wait_until(Instant::now() + Duration::from_secs(5))
            .expect("actual join");
        assert_eq!(report.exit, WorkerExit::Joined);
        assert!(matches!(
            report.into_result(),
            Err(LoggingError::PriorWriteFailed)
        ));
        assert_eq!(&*appended.lock().expect("sink readback"), b"abc");
        assert_eq!(
            attempts.load(Ordering::Relaxed),
            2,
            "a partial write must not be retried as a second event"
        );
        assert_eq!(
            quota.snapshot().worker_threads,
            0,
            "worker charge ends only after native retirement"
        );
        assert_eq!(service.health().retained_bytes, 0);
    }

    #[test]
    fn ordered_logger_shutdown_ack_waits_for_parked_native_tls_retirement() {
        use std::cell::RefCell;
        struct TlsPark {
            entered: SyncSender<()>,
            release: Receiver<()>,
        }
        impl Drop for TlsPark {
            fn drop(&mut self) {
                let _ = self.entered.try_send(());
                let _ = self.release.recv_timeout(Duration::from_secs(5));
            }
        }
        thread_local! {
            static TLS_PARK: RefCell<Option<TlsPark>> = const { RefCell::new(None) };
        }
        let directory = tempfile::tempdir().expect("directory");
        let (entered, tls_entered) = mpsc::sync_channel(1);
        let (release, tls_release) = mpsc::sync_channel(1);
        let service = Service::start(
            directory.path().join("tls.log"),
            Arc::new(AtomicBool::new(false)),
            None,
            move || {
                TLS_PARK.with(|slot| {
                    *slot.borrow_mut() = Some(TlsPark {
                        entered,
                        release: tls_release,
                    });
                })
            },
        )
        .expect("worker");
        let quota = service.shared.quota.clone();
        let mut shutdown = service.shutdown_joined().expect("ordered shutdown");
        tls_entered
            .recv_timeout(Duration::from_secs(5))
            .expect("TLS destructor parked");
        assert!(
            shutdown.try_complete().is_none(),
            "ACK cannot stand in for native join"
        );
        assert_eq!(quota.snapshot().worker_threads, 1);
        release.send(()).expect("release native TLS");
        shutdown
            .wait_until(Instant::now() + Duration::from_secs(5))
            .expect("joined")
            .into_result()
            .expect("flush completed");
        assert_eq!(quota.snapshot().worker_threads, 0);
    }

    struct BlockedWrite {
        started: SyncSender<()>,
        release: Receiver<()>,
    }
    impl Write for BlockedWrite {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.started.send(()).map_err(io::Error::other)?;
            self.release.recv().map_err(io::Error::other)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn blocked_sink_does_not_hold_queue_lock_or_block_logical_shutdown_owner() {
        let directory = tempfile::tempdir().expect("directory");
        let (started_sender, started) = mpsc::sync_channel(1);
        let (release, release_receiver) = mpsc::sync_channel(1);
        let sink = BlockedWrite {
            started: started_sender,
            release: release_receiver,
        };
        let service = Service::start(
            directory.path().join("unused.log"),
            Arc::new(AtomicBool::new(true)),
            Some(Box::new(sink)),
            || {},
        )
        .expect("service");
        assert!(service.reserve(2));
        service.event(b"x\n".to_vec());
        started
            .recv_timeout(Duration::from_secs(5))
            .expect("write definitely blocked");
        let flush = service
            .flush()
            .expect("control admitted without waiting for sink");
        assert!(flush.try_recv().expect("receipt connected").is_none());
        let ticket = service._worker.ticket();
        let shared = Arc::clone(&service.shared);
        let started_at = Instant::now();
        drop(service);
        assert!(started_at.elapsed() < Duration::from_secs(1));
        assert!(ticket.join_until(Instant::now()).is_err());
        release.send(()).expect("release blocked sink");
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(5))
            .is_ok());
        assert!(matches!(
            flush.wait_timeout(Duration::from_secs(5)),
            Err(LoggingError::WorkerStopped)
        ));
        assert_eq!(shared.bytes.load(Ordering::Acquire), 0);
    }

    struct PanickingWrite;
    impl Write for PanickingWrite {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            panic!("forced logger worker panic");
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn worker_panic_closes_admission_releases_bytes_and_fails_pending_receipts() {
        let directory = tempfile::tempdir().expect("directory");
        let (release, gate) = mpsc::sync_channel(1);
        let service = Service::start(
            directory.path().join("unused.log"),
            Arc::new(AtomicBool::new(true)),
            Some(Box::new(PanickingWrite)),
            move || {
                gate.recv().expect("gate");
            },
        )
        .expect("service");
        assert!(service.reserve(2));
        service.event(b"x\n".to_vec());
        let flush = service.flush().expect("pending control");
        release.send(()).expect("release");
        assert!(matches!(
            flush.wait_timeout(Duration::from_secs(5)),
            Err(LoggingError::WorkerStopped)
        ));
        assert!(!service.health().worker_alive);
        assert!(!service.health().accepting);
        assert_eq!(service.health().retained_bytes, 0);
        assert_eq!(service.health().queued_commands, 0);
        assert!(matches!(
            service.enable(true),
            Err(LoggingError::WorkerStopped)
        ));
    }

    #[test]
    fn failed_open_acknowledges_error_without_marking_enabled_and_can_retry() {
        let directory = tempfile::tempdir().expect("directory");
        let blocker = directory.path().join("not-a-directory");
        std::fs::write(&blocker, b"owned fixture").expect("fixture");
        let enabled = Arc::new(AtomicBool::new(false));
        let service = Service::new(blocker.join("log"), Arc::clone(&enabled)).expect("worker");
        assert!(matches!(
            service
                .enable(true)
                .expect("admission")
                .wait_timeout(Duration::from_secs(5)),
            Err(LoggingError::PrepareFile { .. })
        ));
        assert!(!enabled.load(Ordering::Acquire));
        assert_eq!(service.health().failed_operations, 1);
        assert!(matches!(
            service
                .enable(true)
                .expect("retry admission")
                .wait_timeout(Duration::from_secs(5)),
            Err(LoggingError::PrepareFile { .. })
        ));
        assert_eq!(service.health().failed_operations, 2);
        service
            .shutdown()
            .expect("shutdown")
            .wait_timeout(Duration::from_secs(5))
            .expect("close");
        assert_eq!(std::fs::read(blocker).expect("unchanged"), b"owned fixture");
    }
}
