//! The only PTY state mutator. Each admitted output chunk, input, mouse event,
//! and resize has a single queue position. A chunk's replies finish WRITING
//! before this owner dequeues the next event. No parser lock is held for writes.

use crate::admission::PtyOwnerReservations;
use crate::delivery::{
    Delivery, DeliveryError, DeliveryFailure, DeliveryReceipt, OperationKind, ShutdownReason,
};
use crate::owner_queue::{Event, Queue, WorkItem};
use crate::query::TerminalQueryResponder;
use crossterm::event::MouseEvent;
use ilium_platform::owned_worker::{OwnedWorker, StopToken, WorkerExit, WorkerKind, WorkerTicket};
use ilium_platform::pty_io::{
    self, AsyncWriter, PtyControl, ReadMessage, ShellProbe, TransportParts, WriteProgress,
};
use portable_pty::MasterPty;
use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockWriteGuard, TryLockError};
use std::time::{Duration, Instant};
use tokio::sync::watch;

pub use crate::owner_queue::{OwnerLimits, QueueLoad};
const MAX_PENDING_REPLY_BYTES: usize = 2048;
const PARSER_LOCK_RETRY: Duration = Duration::from_millis(1);

type Parser = vt100::Parser<TerminalQueryResponder>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerStatus {
    Running,
    StopRequested,
    /// Command processing ended. Worker join status is reported separately;
    /// an unacknowledged native pump/control destructor can still be retiring.
    Stopped {
        reason: ShutdownReason,
        error: Option<DeliveryError>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownReport {
    pub joined: Vec<u64>,
    pub panicked: Vec<u64>,
    /// Handles remain owned by ilium-platform's bounded join supervisor.
    pub pending: Vec<u64>,
}

/// Clone under a registry lock, release the lock, then submit/await. The handle
/// is bound to one PTY lifetime; it cannot be rebound to a replacement runtime.
#[derive(Clone)]
pub struct PtyInput {
    queue: Arc<Queue>,
    status: watch::Receiver<OwnerStatus>,
}

impl PtyInput {
    pub fn write(&self, bytes: &[u8]) -> Result<DeliveryReceipt, DeliveryError> {
        self.queue.submit(OperationKind::Input, bytes.len(), || {
            Event::Input(Arc::from(bytes))
        })
    }
    pub fn write_mouse_input(
        &self,
        event: MouseEvent,
        column: u16,
        row: u16,
    ) -> Result<DeliveryReceipt, DeliveryError> {
        // Encoding happens in queue order against the then-current mouse mode.
        self.queue.submit(OperationKind::Mouse, 128, || {
            Event::Mouse(event, column, row)
        })
    }
    pub fn resize(&self, rows: u16, cols: u16) -> Result<DeliveryReceipt, DeliveryError> {
        self.queue.submit(OperationKind::Resize, 0, || {
            Event::Resize(rows.max(1), cols.max(1))
        })
    }
    pub fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.queue, &other.queue)
    }
    pub fn queue_load(&self) -> QueueLoad {
        self.queue.load()
    }
    pub fn status(&self) -> OwnerStatus {
        self.status.borrow().clone()
    }
    pub fn subscribe_status(&self) -> watch::Receiver<OwnerStatus> {
        self.status.clone()
    }
}

/// Session constructs this once. The publishing closure appends the SHARED
/// chunk to the existing journal and broadcasts it; it must not block on I/O.
/// No read pump receives this state or any of its parser/journal Arcs.
pub(crate) struct TerminalState {
    pub(crate) screen_reader: crate::screen_reader::ScreenReader,
    pub(crate) parser: Arc<RwLock<Parser>>,
    pub(crate) generation: Arc<AtomicU64>,
    pub(crate) changed: watch::Sender<()>,
    pub(crate) publish: Box<dyn FnMut(Arc<[u8]>) + Send>,
}

pub(crate) struct PtyOwner {
    input: PtyInput,
    status: watch::Sender<OwnerStatus>,
    owned: Vec<OwnedWorker>,
    tickets: Vec<WorkerTicket>,
}

struct CleanupOnFailure(Option<Box<dyn FnOnce() + Send>>);
impl CleanupOnFailure {
    fn run(&mut self) {
        if let Some(cleanup) = self.0.take() {
            cleanup();
        }
    }
    fn disarm(&mut self) {
        self.0.take();
    }
}
impl Drop for CleanupOnFailure {
    fn drop(&mut self) {
        self.run();
    }
}

impl PtyOwner {
    /// `setup_failure` kills/reaps ONLY the child just spawned by this session.
    /// It runs before dropping native control on every ordinary setup error.
    /// A successful owner never invokes it and never signals a user process.
    pub(crate) fn spawn(
        master: Box<dyn MasterPty + Send>,
        terminal: TerminalState,
        limits: OwnerLimits,
        reservations: PtyOwnerReservations,
        setup_failure: impl FnOnce() + Send + 'static,
    ) -> io::Result<(Self, ShellProbe)> {
        let mut setup_failure: Option<Box<dyn FnOnce() + Send>> = Some(Box::new(setup_failure));
        if let Err(error) = limits.validate() {
            setup_failure.take().unwrap()();
            return Err(error);
        }
        let queue = Queue::new(limits);
        let prepared =
            match pty_io::prepare(&*master, queue.stop.clone(), reservations.native_writer) {
                Ok(prepared) => prepared,
                Err(error) => {
                    setup_failure.take().unwrap()();
                    return Err(error);
                }
            };
        let TransportParts {
            control,
            writer,
            reader,
            shell_probe,
        } = prepared.attach_master(master);
        // Declared after control/writer so setup cleanup runs before their drop.
        let mut failure = CleanupOnFailure(setup_failure);
        let expiry_worker = queue.start_expiry_worker_reserved(reservations.expiry)?;
        let writer_wake_queue = Arc::clone(&queue);
        let writer = AsyncWriter::spawn_reserved_with_completion_wake(
            writer,
            queue.stop.child(),
            || failure.run(),
            move || writer_wake_queue.wake(),
            reservations.writer,
        )?;
        let writer_tickets = writer.tickets();
        let output_queue = Arc::clone(&queue);
        let reader_worker = match reader.spawn_reserved(
            queue.stop.child(),
            move |message, stop| output_queue.output(message, stop),
            reservations.reader,
        ) {
            Ok(worker) => worker,
            Err(error) => {
                failure.run();
                return Err(error);
            }
        };
        let (status, receiver) = watch::channel(OwnerStatus::Running);
        // Spawn a waiting actor BEFORE moving native control into its closure;
        // failed thread creation therefore cannot destroy master before cleanup.
        let (start, ready) = std::sync::mpsc::sync_channel::<Engine>(1);
        let actor_queue = Arc::clone(&queue);
        let wake_queue = Arc::clone(&queue);
        let actor_status = status.clone();
        let owner_worker = match reservations.owner.spawn(
            "ilium-pty-owner",
            WorkerKind::Cooperative,
            StopToken::default(),
            move || wake_queue.wake(),
            move |stop| {
                let guard = OwnerExitGuard {
                    queue: actor_queue,
                    status: actor_status,
                };
                loop {
                    if stop.is_stopped() {
                        return;
                    }
                    match ready.recv_timeout(Duration::from_millis(10)) {
                        Ok(engine) => {
                            engine.run(&stop, &guard);
                            return;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
            },
        ) {
            Ok(worker) => worker,
            Err(error) => {
                failure.run();
                return Err(error);
            }
        };
        let engine = Engine {
            queue: Arc::clone(&queue),
            terminal,
            control,
            writer,
            active: None,
            pending_replies: VecDeque::new(),
            pending_reply_bytes: 0,
        };
        if let Err(std::sync::mpsc::SendError(engine)) = start.send(engine) {
            failure.run();
            queue.close(ShutdownReason::OwnerPanicked);
            drop(engine);
            return Err(io::Error::other("PTY owner exited during startup"));
        }
        let mut tickets = vec![
            owner_worker.ticket(),
            reader_worker.ticket(),
            expiry_worker.ticket(),
        ];
        tickets.extend(writer_tickets);
        failure.disarm();
        Ok((
            Self {
                input: PtyInput {
                    queue,
                    status: receiver,
                },
                status,
                owned: vec![owner_worker, reader_worker, expiry_worker],
                tickets,
            },
            shell_probe,
        ))
    }

    pub(crate) fn input(&self) -> PtyInput {
        self.input.clone()
    }

    pub(crate) fn request_shutdown(&self) {
        self.input.queue.close(ShutdownReason::Requested);
        self.status.send_if_modified(|status| {
            if matches!(status, OwnerStatus::Running) {
                *status = OwnerStatus::StopRequested;
                true
            } else {
                false
            }
        });
        for ticket in &self.tickets {
            ticket.cancel();
        }
    }

    /// The session must kill/reap its owned child separately. Do not call this
    /// blocking join from an executor thread or under a shared registry lock.
    pub(crate) fn shutdown_blocking(&self, timeout: Duration) -> ShutdownReport {
        self.request_shutdown();
        let deadline = Instant::now() + timeout.min(Duration::from_secs(60));
        let mut report = ShutdownReport {
            joined: Vec::new(),
            panicked: Vec::new(),
            pending: Vec::new(),
        };
        for ticket in &self.tickets {
            match ticket.join_until(deadline) {
                Ok(WorkerExit::Joined) => report.joined.push(ticket.id()),
                Ok(WorkerExit::Panicked) => report.panicked.push(ticket.id()),
                Err(_) => report.pending.push(ticket.id()),
            }
        }
        report
    }
}

impl Drop for PtyOwner {
    fn drop(&mut self) {
        self.request_shutdown();
        // RAII owners also cancel on error/unwind. The supervisor owns the real
        // handles and their bounded retirement, so Drop never blocks on join.
        self.owned.clear();
    }
}

struct OwnerExitGuard {
    queue: Arc<Queue>,
    status: watch::Sender<OwnerStatus>,
}
impl OwnerExitGuard {
    fn stop(&self, reason: ShutdownReason, error: Option<DeliveryError>) {
        self.queue.close(reason);
        self.status
            .send_replace(OwnerStatus::Stopped { reason, error });
    }
}
impl Drop for OwnerExitGuard {
    fn drop(&mut self) {
        let stopped = {
            let current = self.status.borrow();
            matches!(&*current, OwnerStatus::Stopped { .. })
        };
        if !stopped {
            self.stop(
                self.queue
                    .shutdown_reason()
                    .unwrap_or(ShutdownReason::OwnerPanicked),
                None,
            );
        }
    }
}

struct Engine {
    queue: Arc<Queue>,
    terminal: TerminalState,
    writer: AsyncWriter,
    control: Box<dyn PtyControl>,
    active: Option<ActiveWrite>,
    pending_replies: VecDeque<Arc<[u8]>>,
    pending_reply_bytes: usize,
}

enum ActiveWrite {
    Input { item: WorkItem, requested: usize },
    Reply { requested: usize },
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Also runs on panic, BEFORE native destructors. Field order then
        // releases parser/journal state, cancels the writer, and drops control.
        // A normal run has already fixed the queue's first shutdown reason.
        self.queue.close(ShutdownReason::OwnerPanicked);
        // Settle panic-path receipts before any native destructor can block.
        // WorkItem's fallback conservatively reports unknown delivery.
        self.active.take();
    }
}

impl Engine {
    fn run(mut self, stop: &StopToken, guard: &OwnerExitGuard) {
        loop {
            if self.active.is_some() {
                if let Some((reason, error)) = self.poll_active() {
                    guard.stop(reason, error);
                    break;
                }
                if self.active.is_some() {
                    // One partial input/reply owns the writer exclusively.
                    // Parse only output that precedes the next queued command;
                    // a resize ahead of output remains an absolute barrier.
                    let wake_generation = self.queue.wake_generation();
                    if let Some(output) = self.queue.take_output_while_writing() {
                        if let Some((reason, error)) = self.execute(output) {
                            guard.stop(reason, error);
                            break;
                        }
                    } else {
                        self.queue.wait_during_write(
                            wake_generation,
                            self.writer.next_poll_delay().unwrap_or(Duration::ZERO),
                        );
                    }
                    continue;
                }
            }
            if stop.is_stopped() || self.queue.shutdown_reason().is_some() {
                break;
            }
            if let Some(reply) = self.pending_replies.pop_front() {
                self.pending_reply_bytes -= reply.len();
                if let Some((reason, error)) = self.begin_reply(reply) {
                    guard.stop(reason, error);
                    break;
                }
                continue;
            }
            let Some(item) = self.queue.take(stop) else {
                break;
            };
            if let Some((reason, error)) = self.execute(item) {
                guard.stop(reason, error);
                break;
            }
        }
        let stopped = {
            let current = guard.status.borrow();
            matches!(&*current, OwnerStatus::Stopped { .. })
        };
        if !stopped {
            guard.stop(
                self.queue
                    .shutdown_reason()
                    .unwrap_or(ShutdownReason::Requested),
                None,
            );
        }
        // Queue close cancels the active operation too, but it only settles
        // QUEUED receipts. Retire the active transport receipt before parking:
        // poll observes either an exact completion or the bounded cancellation
        // grace's unconfirmed result. Keep the original terminal reason.
        while self.active.is_some() {
            let _ = self.poll_active();
            if self.active.is_some() {
                let wake_generation = self.queue.wake_generation();
                self.queue.wait_during_write(
                    wake_generation,
                    self.writer.next_poll_delay().unwrap_or(Duration::ZERO),
                );
            }
        }
        // A failed stream is quarantined, not silently retried and not killed.
        // Keep native master/writer ownership until the SESSION requests close.
        // This wait has no parser lock; the owner ticket's wake closes the race.
        self.queue.wait_for_owner_stop(stop);
        // Engine::drop and its field order release parser/journal clones BEFORE
        // native writer/control destruction, including during panic unwinding.
    }

    fn execute(&mut self, mut item: WorkItem) -> Option<(ShutdownReason, Option<DeliveryError>)> {
        let event = item.event.take().expect("one execution per work item");
        if item.kind.is_some() && (item.stop.is_stopped() || Instant::now() >= item.deadline) {
            let failure = if item.stop.is_stopped() {
                DeliveryFailure::Cancelled
            } else {
                DeliveryFailure::Timeout
            };
            item.finish(Err(DeliveryError::new(Some(item.id), failure)));
            return None;
        }
        match event {
            Event::Input(bytes) => self.begin_input(item, bytes),
            Event::Mouse(event, column, row) => {
                let parser = Arc::clone(&self.terminal.parser);
                let guard = match parser_lock(&parser, item.deadline, &item.stop) {
                    Ok(guard) => guard,
                    Err(failure) => {
                        let fatal = matches!(failure, DeliveryFailure::OwnerLost);
                        let error = DeliveryError::new(Some(item.id), failure);
                        item.finish(Err(error.clone()));
                        return fatal.then_some((ShutdownReason::ParserUnavailable, Some(error)));
                    }
                };
                let screen = guard.screen();
                let encoded = crate::mouse::encode_mouse_event(
                    event,
                    column,
                    row,
                    screen.mouse_protocol_mode(),
                    screen.mouse_protocol_encoding(),
                );
                drop(guard);
                self.begin_input(item, Arc::from(encoded.unwrap_or_default()))
            }
            Event::Resize(rows, cols) => {
                let result = self.resize(&item, rows, cols);
                let fatal = result.as_ref().err().is_some_and(|error| {
                    matches!(
                        error.failure,
                        DeliveryFailure::Resize {
                            restored: false,
                            ..
                        } | DeliveryFailure::OwnerLost
                    )
                });
                let error = result.as_ref().err().cloned();
                item.finish(result);
                fatal.then_some((ShutdownReason::GeometryLost, error))
            }
            Event::Output(ReadMessage::Data(bytes)) => self.output(bytes),
            Event::Output(ReadMessage::Eof) => Some((ShutdownReason::Eof, None)),
            Event::Output(ReadMessage::Error(error)) => Some((
                ShutdownReason::ReaderFailed,
                Some(DeliveryError::new(None, DeliveryFailure::Io(error))),
            )),
        }
    }

    fn begin_input(
        &mut self,
        item: WorkItem,
        bytes: Arc<[u8]>,
    ) -> Option<(ShutdownReason, Option<DeliveryError>)> {
        let requested = bytes.len();
        if requested == 0 {
            item.finish(Ok(Delivery {
                operation_id: item.id,
                kind: item.kind.unwrap(),
                bytes_written: 0,
                size: None,
            }));
            return None;
        }
        match self.writer.begin(bytes, item.deadline, item.stop.clone()) {
            Ok(()) => {
                // Move the active work item without releasing its quota or
                // completing its receipt. The queue owns no other copy.
                self.active = Some(ActiveWrite::Input { item, requested });
                None
            }
            Err(failure) => {
                // Zero-byte cancellation on a usable writer can leave the pane
                // usable. Partial, unsettled, or broken writes quarantine it.
                let fatal = !failure.reusable || failure.definitely_written > 0 || !failure.settled;
                let error = DeliveryError::from_write(Some(item.id), requested, failure);
                item.finish(Err(error.clone()));
                fatal.then_some((ShutdownReason::WriterFailed, Some(error)))
            }
        }
    }

    fn begin_reply(&mut self, bytes: Arc<[u8]>) -> Option<(ShutdownReason, Option<DeliveryError>)> {
        let requested = bytes.len();
        match self.writer.begin(
            bytes,
            Instant::now() + self.queue.limits.reply_timeout,
            self.queue.stop.child(),
        ) {
            Ok(()) => {
                self.active = Some(ActiveWrite::Reply { requested });
                None
            }
            Err(failure) => Some((
                ShutdownReason::WriterFailed,
                Some(DeliveryError::from_write(None, requested, failure)),
            )),
        }
    }

    fn poll_active(&mut self) -> Option<(ShutdownReason, Option<DeliveryError>)> {
        let WriteProgress::Finished(result) = self.writer.poll() else {
            return None;
        };
        let Some(active) = self.active.take() else {
            return Some((
                ShutdownReason::OwnerPanicked,
                Some(DeliveryError::new(None, DeliveryFailure::OwnerLost)),
            ));
        };
        match active {
            ActiveWrite::Input { item, requested } => match result {
                Ok(success) => {
                    item.finish(Ok(Delivery {
                        operation_id: item.id,
                        kind: item.kind.unwrap(),
                        bytes_written: success.written,
                        size: None,
                    }));
                    (!success.reusable).then_some((ShutdownReason::WriterFailed, None))
                }
                Err(failure) => {
                    let fatal =
                        !failure.reusable || failure.definitely_written > 0 || !failure.settled;
                    let error = DeliveryError::from_write(Some(item.id), requested, failure);
                    item.finish(Err(error.clone()));
                    fatal.then_some((ShutdownReason::WriterFailed, Some(error)))
                }
            },
            ActiveWrite::Reply { requested } => match result {
                Ok(success) if success.reusable && success.written == requested => None,
                Ok(_) => Some((ShutdownReason::WriterFailed, None)),
                Err(error) => Some((
                    ShutdownReason::WriterFailed,
                    Some(DeliveryError::from_write(None, requested, error)),
                )),
            },
        }
    }

    fn output(&mut self, bytes: Arc<[u8]>) -> Option<(ShutdownReason, Option<DeliveryError>)> {
        let parser = Arc::clone(&self.terminal.parser);
        let pending = {
            let mut parser = match parser_lock(
                &parser,
                Instant::now() + self.queue.limits.parser_lock_timeout,
                &self.queue.stop,
            ) {
                Ok(parser) => parser,
                Err(failure) => {
                    // Keep the raw final evidence even when this parser can
                    // no longer accept it. Do not advance parser generation.
                    (self.terminal.publish)(Arc::clone(&bytes));
                    let _ = self.terminal.changed.send(());
                    return Some((
                        ShutdownReason::ParserUnavailable,
                        Some(DeliveryError::new(None, failure)),
                    ));
                }
            };
            parser.process(&bytes);
            self.terminal.generation.fetch_add(1, Ordering::Release);
            parser.callbacks_mut().take_pending_replies()
        };
        // Preserve output, including the query that caused a terminal failure.
        (self.terminal.publish)(bytes);
        let _ = self.terminal.changed.send(());
        if pending.len() > MAX_PENDING_REPLY_BYTES.saturating_sub(self.pending_reply_bytes) {
            return Some((
                ShutdownReason::WriterFailed,
                Some(DeliveryError::new(
                    None,
                    DeliveryFailure::TooLarge {
                        requested: self.pending_reply_bytes + pending.len(),
                        maximum: MAX_PENDING_REPLY_BYTES,
                    },
                )),
            ));
        }
        if !pending.is_empty() {
            self.pending_reply_bytes += pending.len();
            self.pending_replies.push_back(Arc::from(pending));
        }
        None
    }

    fn resize(&mut self, item: &WorkItem, rows: u16, cols: u16) -> Result<Delivery, DeliveryError> {
        let parser = Arc::clone(&self.terminal.parser);
        let mut parser = parser_lock(&parser, item.deadline, &item.stop)
            .map_err(|failure| DeliveryError::new(Some(item.id), failure))?;
        self.terminal
            .screen_reader
            .retain_before_resize(parser.screen());
        let previous = parser.screen().size();
        let requested = (rows, cols);
        // The parser WRITE lock is already held before the FIRST OS call.
        // Do not shrink/reflow the parser and try to undo it on failure: lost
        // rows/wrap state cannot be restored by setting its old dimensions.
        let result = self.control.resize(rows, cols);
        let observed = self.control.size();
        let verified = result.is_ok() && observed.as_ref().is_ok_and(|size| *size == requested);
        if !verified {
            let message = format!("resize result={result:?}; observed={observed:?}");
            let restored = if observed.as_ref().is_ok_and(|size| *size == previous) {
                true
            } else {
                // Best-effort rollback of the OS half; parser is STILL pristine.
                let rollback = self.control.resize(previous.0, previous.1);
                rollback.is_ok() && self.control.size().is_ok_and(|size| size == previous)
            };
            drop(parser);
            let _ = self.terminal.changed.send(());
            return Err(DeliveryError::new(
                Some(item.id),
                DeliveryFailure::Resize {
                    requested,
                    previous,
                    restored,
                    message,
                },
            ));
        }
        parser.screen_mut().set_size(rows, cols);
        self.terminal.screen_reader.advance_resize_epoch();
        self.terminal.generation.fetch_add(1, Ordering::Release);
        drop(parser);
        let _ = self.terminal.changed.send(());
        Ok(Delivery {
            operation_id: item.id,
            kind: OperationKind::Resize,
            bytes_written: 0,
            size: Some(requested),
        })
    }
}

fn parser_lock<'a>(
    parser: &'a RwLock<Parser>,
    deadline: Instant,
    stop: &StopToken,
) -> Result<RwLockWriteGuard<'a, Parser>, DeliveryFailure> {
    loop {
        if stop.is_stopped() {
            return Err(DeliveryFailure::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(DeliveryFailure::ParserBusy);
        }
        match parser.try_write() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => return Err(DeliveryFailure::OwnerLost),
            Err(TryLockError::WouldBlock) => std::thread::sleep(PARSER_LOCK_RETRY),
        }
    }
}

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
