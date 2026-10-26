//! One bounded admission order for commands AND raw output. The actor, never a
//! pump, removes entries. Charges remain held through the active write barrier.

use crate::delivery::{
    Completion, Delivery, DeliveryError, DeliveryFailure, DeliveryReceipt, OperationKind,
    ShutdownReason,
};
use crossterm::event::MouseEvent;
#[cfg(test)]
use ilium_platform::owned_worker::reserve_owned_worker;
use ilium_platform::owned_worker::{OwnedWorker, StopToken, WorkerKind, WorkerReservation};
use ilium_platform::pty_io::{ReadMessage, WriteFailureKind, OUTPUT_CHUNK_BYTES};
use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct OwnerLimits {
    pub commands: usize,
    pub command_bytes: usize,
    pub single_input_bytes: usize,
    pub output_chunks: usize,
    pub output_bytes: usize,
    pub command_timeout: Duration,
    pub reply_timeout: Duration,
    pub parser_lock_timeout: Duration,
}

impl Default for OwnerLimits {
    fn default() -> Self {
        Self {
            commands: 128,
            command_bytes: 1024 * 1024,
            single_input_bytes: 1024 * 1024,
            output_chunks: 16,
            output_bytes: 1024 * 1024,
            command_timeout: Duration::from_secs(2),
            reply_timeout: Duration::from_secs(2),
            parser_lock_timeout: Duration::from_secs(2),
        }
    }
}

impl OwnerLimits {
    pub(crate) fn validate(&self) -> std::io::Result<()> {
        if self.commands == 0
            || self.command_bytes < 128
            || self.single_input_bytes == 0
            || self.single_input_bytes > self.command_bytes
            || self.output_chunks == 0
            || self.output_bytes < OUTPUT_CHUNK_BYTES
            || self.command_timeout.is_zero()
            || self.reply_timeout.is_zero()
            || self.parser_lock_timeout.is_zero()
            || self.command_timeout > Duration::from_secs(60)
            || self.reply_timeout > Duration::from_secs(60)
            || self.parser_lock_timeout > Duration::from_secs(60)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid bounded PTY owner limits",
            ));
        }
        Ok(())
    }
}

pub(crate) enum Event {
    Input(Arc<[u8]>),
    Mouse(MouseEvent, u16, u16),
    Resize(u16, u16),
    Output(ReadMessage),
}

struct Entry {
    id: u64,
    event: Event,
    command: Option<OperationKind>,
    charge: usize,
    deadline: Instant,
    stop: StopToken,
    completion: Option<Arc<Completion>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueLoad {
    pub commands: usize,
    pub command_bytes: usize,
    pub output_chunks: usize,
    pub output_bytes: usize,
}

struct State {
    entries: VecDeque<Entry>,
    next_id: u64,
    load: QueueLoad,
    closed: Option<ShutdownReason>,
    wake_generation: u64,
}

pub(crate) struct Queue {
    pub(crate) limits: OwnerLimits,
    pub(crate) stop: StopToken,
    state: Mutex<State>,
    changed: Condvar,
    deadlines_changed: Condvar,
}

impl Queue {
    pub(crate) fn new(limits: OwnerLimits) -> Arc<Self> {
        Arc::new(Self {
            limits,
            stop: StopToken::default(),
            state: Mutex::new(State {
                entries: VecDeque::new(),
                next_id: 1,
                load: QueueLoad {
                    commands: 0,
                    command_bytes: 0,
                    output_chunks: 0,
                    output_bytes: 0,
                },
                closed: None,
                wake_generation: 0,
            }),
            changed: Condvar::new(),
            deadlines_changed: Condvar::new(),
        })
    }

    pub(crate) fn load(&self) -> QueueLoad {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).load
    }

    /// Queued commands have never reached the writer. An independent owned
    /// deadline worker completes them as proven zero delivery even when the
    /// state owner is held at an earlier reply or native control operation.
    #[cfg(test)]
    pub(crate) fn start_expiry_worker(self: &Arc<Self>) -> std::io::Result<OwnedWorker> {
        let reservation = reserve_owned_worker(None, ())?;
        self.start_expiry_worker_reserved(reservation)
    }

    /// Starts the deadline worker with the session's pre-child reservation.
    pub(crate) fn start_expiry_worker_reserved<C: Send + 'static>(
        self: &Arc<Self>,
        reservation: WorkerReservation<C>,
    ) -> std::io::Result<OwnedWorker> {
        let queue = Arc::clone(self);
        let wake_queue = Arc::clone(self);
        reservation.spawn(
            "ilium-pty-queued-deadlines",
            WorkerKind::Cooperative,
            self.stop.child(),
            move || wake_queue.wake(),
            move |worker_stop| queue.expiry_loop(&worker_stop),
        )
    }

    fn expiry_loop(&self, worker_stop: &StopToken) {
        loop {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if state.closed.is_some() || worker_stop.is_stopped() {
                return;
            }
            let now = Instant::now();
            // Preserve the resident deque and leave output in admission order.
            // Output traffic must not allocate and copy the entire command queue.
            let mut expired = Vec::new();
            let mut index = 0;
            while index < state.entries.len() {
                let entry = &state.entries[index];
                if entry.command.is_some() && (entry.stop.is_stopped() || now >= entry.deadline) {
                    if let Some(entry) = state.entries.remove(index) {
                        Self::uncharge(&mut state.load, true, entry.charge);
                        // Classified under the queue lock: a later cancellation
                        // must not relabel an entry that already timed out.
                        let failure = if entry.stop.is_stopped() {
                            DeliveryFailure::Cancelled
                        } else {
                            DeliveryFailure::Timeout
                        };
                        expired.push((entry, failure));
                    }
                } else {
                    index += 1;
                }
            }
            if !expired.is_empty() {
                Self::notify_changed(&self.changed, &mut state);
            }
            drop(state);
            for (entry, failure) in expired {
                if let Some(completion) = entry.completion {
                    completion.finish(Err(DeliveryError::new(Some(entry.id), failure)));
                }
            }
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if state.closed.is_some() || worker_stop.is_stopped() {
                return;
            }
            // Cancellation can race publication while the lock is released.
            // Its notification may already have fired; inspect the predicate
            // again before sleeping, rather than waiting until the deadline.
            if state
                .entries
                .iter()
                .any(|entry| entry.command.is_some() && entry.stop.is_stopped())
            {
                drop(state);
                continue;
            }
            // The deadline may have changed while completions were published.
            // Rechecking under the queue lock prevents a lost admission wake.
            let deadline = state
                .entries
                .iter()
                .filter(|entry| entry.command.is_some())
                .map(|entry| entry.deadline)
                .min();
            if let Some(deadline) = deadline {
                let wait = deadline.saturating_duration_since(Instant::now());
                if !wait.is_zero() {
                    state = self
                        .deadlines_changed
                        .wait_timeout(state, wait)
                        .unwrap_or_else(|error| error.into_inner())
                        .0;
                }
            } else {
                state = self
                    .deadlines_changed
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
            drop(state);
        }
    }

    pub(crate) fn shutdown_reason(&self) -> Option<ShutdownReason> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).closed
    }

    /// After a fatal I/O failure, keep the native control/writer alive until
    /// explicit session teardown. Failure quarantine must not itself close a
    /// ConPTY or inject EOF into a user's still-running child.
    pub(crate) fn wait_for_owner_stop(&self, stop: &StopToken) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while !stop.is_stopped() {
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub(crate) fn wake(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::notify_changed(&self.changed, &mut state);
        self.deadlines_changed.notify_all();
    }

    pub(crate) fn wake_generation(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .wake_generation
    }

    pub(crate) fn submit(
        self: &Arc<Self>,
        kind: OperationKind,
        charge: usize,
        make_event: impl FnOnce() -> Event,
    ) -> Result<DeliveryReceipt, DeliveryError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(reason) = state.closed {
            return Err(DeliveryError::new(None, DeliveryFailure::Shutdown(reason)));
        }
        if charge > self.limits.single_input_bytes {
            return Err(DeliveryError::new(
                None,
                DeliveryFailure::TooLarge {
                    requested: charge,
                    maximum: self.limits.single_input_bytes,
                },
            ));
        }
        if state.load.commands >= self.limits.commands
            || charge
                > self
                    .limits
                    .command_bytes
                    .saturating_sub(state.load.command_bytes)
        {
            return Err(DeliveryError::new(
                None,
                DeliveryFailure::Overloaded {
                    outstanding_commands: state.load.commands,
                    outstanding_bytes: state.load.command_bytes,
                },
            ));
        }
        let id = state.next_id;
        state.next_id = id
            .checked_add(1)
            .ok_or_else(|| DeliveryError::new(None, DeliveryFailure::IdentityExhausted))?;
        let completion = Completion::new();
        let stop = self.stop.child();
        // Admission and byte allocation are one bounded critical section. A
        // rejected oversized paste does not first allocate an owned copy.
        state.entries.push_back(Entry {
            id,
            event: make_event(),
            command: Some(kind),
            charge,
            deadline: Instant::now() + self.limits.command_timeout,
            stop: stop.clone(),
            completion: Some(Arc::clone(&completion)),
        });
        state.load.commands += 1;
        state.load.command_bytes += charge;
        Self::notify_changed(&self.changed, &mut state);
        self.deadlines_changed.notify_one();
        let queue = Arc::clone(self);
        Ok(DeliveryReceipt::new(id, completion, stop, move || {
            queue.wake()
        }))
    }

    /// Blocking BACKPRESSURE only on the byte-only read pump. Its one pending
    /// read chunk is at most OUTPUT_CHUNK_BYTES in addition to these quotas.
    /// An accepted resize is already in entries; this output necessarily goes
    /// after it, even while an earlier reply is still being written.
    pub(crate) fn output(&self, message: ReadMessage, stop: &StopToken) -> bool {
        let charge = match &message {
            ReadMessage::Data(bytes) => bytes.len(),
            _ => 0,
        };
        if charge > OUTPUT_CHUNK_BYTES {
            self.close(ShutdownReason::ReaderFailed);
            return false;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while state.closed.is_none()
            && !stop.is_stopped()
            && (state.load.output_chunks >= self.limits.output_chunks
                || charge
                    > self
                        .limits
                        .output_bytes
                        .saturating_sub(state.load.output_bytes))
        {
            // Also covers cancellation of just this pump while the owner is
            // still alive; its Unix wake pipe cannot wake this Condvar.
            state = self
                .changed
                .wait_timeout(state, Duration::from_millis(10))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        if state.closed.is_some() || stop.is_stopped() {
            return false;
        }
        let id = state.next_id;
        let Some(next) = id.checked_add(1) else {
            drop(state);
            self.close(ShutdownReason::OwnerPanicked);
            return false;
        };
        state.next_id = next;
        state.entries.push_back(Entry {
            id,
            event: Event::Output(message),
            command: None,
            charge,
            deadline: Instant::now(),
            stop: self.stop.child(),
            completion: None,
        });
        state.load.output_chunks += 1;
        state.load.output_bytes += charge;
        Self::notify_changed(&self.changed, &mut state);
        true
    }

    pub(crate) fn take(self: &Arc<Self>, worker_stop: &StopToken) -> Option<WorkItem> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if worker_stop.is_stopped() || state.closed.is_some() {
                return None;
            }
            if let Some(entry) = state.entries.pop_front() {
                return Some(WorkItem {
                    queue: Arc::clone(self),
                    event: Some(entry.event),
                    id: entry.id,
                    kind: entry.command,
                    charge: entry.charge,
                    deadline: entry.deadline,
                    stop: entry.stop,
                    completion: entry.completion,
                    released: Cell::new(false),
                });
            }
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// During one incomplete write, only output immediately following that
    /// write may advance. The first queued command, especially a resize,
    /// remains a barrier for every later output chunk.
    pub(crate) fn take_output_while_writing(self: &Arc<Self>) -> Option<WorkItem> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed.is_some() {
            return None;
        }
        if !matches!(
            state.entries.front().map(|entry| &entry.event),
            Some(Event::Output(_))
        ) {
            return None;
        }
        let entry = state.entries.pop_front()?;
        Some(WorkItem {
            queue: Arc::clone(self),
            event: Some(entry.event),
            id: entry.id,
            kind: None,
            charge: entry.charge,
            deadline: entry.deadline,
            stop: entry.stop,
            completion: None,
            released: Cell::new(false),
        })
    }

    pub(crate) fn wait_during_write(&self, generation: u64, timeout: Duration) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if timeout.is_zero()
            || state.wake_generation != generation
            || state.closed.is_some()
            || self.stop.is_stopped()
        {
            return false;
        }
        let _waited = self
            .changed
            .wait_timeout(state, timeout)
            .unwrap_or_else(|error| error.into_inner());
        true
    }

    pub(crate) fn close(&self, reason: ShutdownReason) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let reason = *state.closed.get_or_insert(reason);
        let entries = std::mem::take(&mut state.entries);
        for entry in &entries {
            Self::uncharge(&mut state.load, entry.command.is_some(), entry.charge);
        }
        Self::notify_changed(&self.changed, &mut state);
        self.deadlines_changed.notify_all();
        drop(state);
        // Receipt observers may immediately submit again; publish outside the
        // queue lock so teardown never nests completion and admission locks.
        for entry in entries {
            if let Some(completion) = entry.completion {
                completion.finish(Err(DeliveryError::new(
                    Some(entry.id),
                    DeliveryFailure::Shutdown(reason),
                )));
            }
        }
        // Cancel only after every queued receipt holds its shutdown verdict:
        // the per-entry stop tokens are children of this one, so stopping
        // first lets the expiry worker observe a stopped entry and settle it
        // as `Cancelled` before this drain reaches it.
        self.stop.stop();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::notify_changed(&self.changed, &mut state);
    }

    fn notify_changed(changed: &Condvar, state: &mut State) {
        state.wake_generation = state.wake_generation.wrapping_add(1);
        changed.notify_all();
    }

    fn uncharge(load: &mut QueueLoad, command: bool, bytes: usize) {
        if command {
            load.commands -= 1;
            load.command_bytes -= bytes;
        } else {
            load.output_chunks -= 1;
            load.output_bytes -= bytes;
        }
    }
}

pub(crate) struct WorkItem {
    queue: Arc<Queue>,
    pub(crate) event: Option<Event>,
    pub(crate) id: u64,
    pub(crate) kind: Option<OperationKind>,
    charge: usize,
    pub(crate) deadline: Instant,
    pub(crate) stop: StopToken,
    completion: Option<Arc<Completion>>,
    released: Cell<bool>,
}

impl WorkItem {
    pub(crate) fn finish(&self, result: Result<Delivery, DeliveryError>) {
        // A caller observing completion may immediately submit its next chunk.
        // Release the completed command's quota BEFORE making its receipt ready.
        self.release();
        if let Some(completion) = &self.completion {
            completion.finish(result);
        }
    }

    fn release(&self) {
        if !self.released.replace(true) {
            let mut state = self.queue.state.lock().unwrap_or_else(|e| e.into_inner());
            Queue::uncharge(&mut state.load, self.kind.is_some(), self.charge);
            Queue::notify_changed(&self.queue.changed, &mut state);
        }
    }
}

impl Drop for WorkItem {
    fn drop(&mut self) {
        self.release();
        // This fallback only wins if normal completion never ran (e.g. panic).
        // An active command must NEVER be labelled "zero bytes, safe to retry".
        if let Some(completion) = &self.completion {
            let failure = match self.kind {
                Some(OperationKind::Input | OperationKind::Mouse) => {
                    DeliveryFailure::UnconfirmedWrite {
                        requested: self.charge,
                        definitely_written: 0,
                        possibly_written: self.charge,
                        cause: WriteFailureKind::WorkerStopped,
                    }
                }
                _ => DeliveryFailure::OwnerLost,
            };
            completion.finish(Err(DeliveryError::new(Some(self.id), failure)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_releases_queued_quota_before_receipts_are_observed() {
        let queue = Queue::new(OwnerLimits::default());
        let receipt = queue
            .submit(OperationKind::Input, 1, || Event::Input(Arc::from([1_u8])))
            .unwrap();
        assert_eq!(queue.load().commands, 1);
        queue.close(ShutdownReason::Requested);
        assert_eq!(queue.load().commands, 0);
        assert_eq!(queue.load().command_bytes, 0);
        drop(receipt);
        assert!(queue
            .submit(OperationKind::Input, 1, || Event::Input(Arc::from([2_u8])))
            .is_err());
    }

    #[test]
    fn write_wait_observes_completion_without_queued_output() {
        let queue = Queue::new(OwnerLimits::default());
        let generation = queue.wake_generation();
        // A completed writer has no output entry; only its wake counter changes.
        queue.wake();
        assert!(queue.take_output_while_writing().is_none());
        assert!(!queue.wait_during_write(generation, Duration::from_secs(5)));
    }

    #[test]
    fn write_wait_observes_output_queued_after_owner_check() {
        let queue = Queue::new(OwnerLimits::default());
        let generation = queue.wake_generation();
        assert!(queue.take_output_while_writing().is_none());
        assert!(queue.output(
            ReadMessage::Data(Arc::from(b"pending output".as_slice())),
            &queue.stop,
        ));

        assert!(!queue.wait_during_write(generation, Duration::from_secs(5)));
    }
}
