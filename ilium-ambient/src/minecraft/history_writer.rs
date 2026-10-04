//! Nonblocking admission of full History snapshots to one owned disk worker.
//!
//! At most one pending and one in-flight History; newer pending snapshots
//! coalesce older ones. Serial numbers order caller snapshots, independently
//! of repository CAS revisions. Dropping closes admission and transfers drain
//! ownership to Worker cleanup; it never joins or touches disk on the caller.
use super::{
    history_store::{self, Repository, Snapshot},
    tours::History,
};
use crate::{
    resources::{AmbientResources, WorkerCost},
    source::Worker,
};

// One native thread; bounded 2 MiB repository documents plus decoded snapshots,
// serialization/read buffers and stack headroom. Declaration, not an RSS limit.
const HISTORY_WORKER_BYTES: usize = 64 * 1024 * 1024;
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, MutexGuard, TryLockError,
    },
    time::Duration,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Queued,
    Coalesced,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Committed,
    /// Sticky failure: reload on a worker before choosing any recovery. Never
    /// merge or resubmit an old History against an external current revision.
    ReloadRequired,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    /// On failure, covers the latest accepted serial, including pending work
    /// that cannot safely commit after the in-flight operation failed.
    pub serial: u64,
    /// Committed revision on success; observed authoritative revision on a
    /// conflict/publication-error reload; None if authority is unavailable.
    pub revision: Option<u64>,
    pub status: Status,
    pub error: Option<String>,
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("history writer busy")]
    Busy,
    #[error("history writer closed")]
    Closed,
    #[error("history writer serial must be nonzero and increasing")]
    StaleSerial,
    #[error("history writer failed; authoritative reload required")]
    ReloadRequired,
}
pub struct Writer {
    shared: Arc<Shared>,
    worker: Option<Worker>,
}
#[derive(Clone, Copy)]
struct Pending {
    serial: u64,
    history: History,
}
#[derive(Default)]
struct State {
    pending: Option<Pending>,
    receipt: Option<Receipt>,
    last_serial: u64,
    failed: bool,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    closed: AtomicBool,
    drained: AtomicBool,
}
impl Writer {
    /// Spawns only; initial_revision must come from an authoritative load.
    /// The caller supplies complete Controller histories after presentation.
    pub fn start(
        repository: Repository,
        initial_revision: u64,
        resources: &AmbientResources,
    ) -> io::Result<Self> {
        Self::start_inner(repository, initial_revision, resources, commit)
    }
    fn start_inner(
        repository: Repository,
        initial_revision: u64,
        resources: &AmbientResources,
        commit: impl Fn(&Repository, u64, History) -> Result<Snapshot, history_store::Error>
            + Send
            + 'static,
    ) -> io::Result<Self> {
        let admission = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: HISTORY_WORKER_BYTES,
            })
            .map_err(|error| {
                io::Error::other(format!("Saved history admission rejected: {error:?}"))
            })?;
        let shared = Arc::new(Shared::default());
        let worker_shared = Arc::clone(&shared);
        let worker = Worker::start_admitted("minecraft-history", admission, move |stop| {
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::Lowest,
            );
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(&repository, initial_revision, &worker_shared, &stop, commit)
            }));
            if result.is_err() {
                fail(
                    &worker_shared,
                    None,
                    "History worker panicked; authoritative reload required".into(),
                );
            }
            // Publish only after the last transaction or its sticky failure
            // receipt. A successor must not bind a catalog while this owner
            // can still commit against its old repository revision.
            worker_shared.drained.store(true, Ordering::Release);
        })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }
    /// Never waits for a mutex or disk. Busy means not accepted: retry the
    /// caller's latest full snapshot later with the same serial. Successful
    /// coalescing covers earlier pending snapshots with this complete one.
    pub fn submit(&self, serial: u64, history: History) -> Result<Admission, Error> {
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        let mut state = try_state(&self.shared)?;
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        if state.failed {
            return Err(Error::ReloadRequired);
        }
        if serial == 0 || serial <= state.last_serial {
            return Err(Error::StaleSerial);
        }
        let admission = if state.pending.is_some() {
            Admission::Coalesced
        } else {
            Admission::Queued
        };
        state.last_serial = serial;
        state.pending = Some(Pending { serial, history });
        drop(state);
        self.shared.wake.notify_one();
        Ok(admission)
    }
    /// One latest receipt; earlier successful receipts may coalesce. Terminal
    /// failure remains sticky for admission even after its receipt is taken.
    pub fn take_receipt(&self) -> Result<Option<Receipt>, Error> {
        Ok(try_state(&self.shared)?.receipt.take())
    }
    /// Admission closure alone does not prove that accepted writes finished.
    /// A host retains this Writer across scene replacement until drain ends,
    /// then inspects its receipt before starting another catalog transaction.
    /// True means no further transaction can run, including after failure;
    /// it does not certify success. ReloadRequired still requires a reload.
    /// This is one atomic read, without a mutex, disk access or thread join.
    pub fn is_drained(&self) -> bool {
        self.shared.drained.load(Ordering::Acquire)
    }
    /// Closes immediately. Accepted work drains off-thread despite the Worker
    /// stop flag. Receipts remain readable while this Writer is retained.
    pub fn close(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.wake.notify_one();
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.close();
    }
}
fn try_state(shared: &Shared) -> Result<MutexGuard<'_, State>, Error> {
    match shared.state.try_lock() {
        Ok(state) => Ok(state),
        Err(TryLockError::WouldBlock) => Err(Error::Busy),
        Err(TryLockError::Poisoned(_)) => Err(Error::ReloadRequired),
    }
}
fn worker_state(shared: &Shared) -> MutexGuard<'_, State> {
    // Only worker recovery consumes a poisoned lock; caller does not guess
    // publication state. Catch-unwind below produces a bounded sticky report.
    shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}
fn run(
    repository: &Repository,
    mut revision: u64,
    shared: &Shared,
    stop: &AtomicBool,
    commit: impl Fn(&Repository, u64, History) -> Result<Snapshot, history_store::Error>,
) {
    loop {
        let pending = {
            let mut state = worker_state(shared);
            loop {
                if state.failed {
                    return;
                }
                if let Some(pending) = state.pending.take() {
                    break pending;
                }
                if shared.closed.load(Ordering::Acquire) || stop.load(Ordering::Relaxed) {
                    // Fence admission before an independently stopped worker
                    // leaves: otherwise a later submit could be accepted with
                    // no live owner. Holding state linearizes with submit.
                    shared.closed.store(true, Ordering::Release);
                    return;
                }
                // Also notice independently raised Worker stop flags. Never
                // cancel already accepted disk transactions on scene rebuild.
                state = shared
                    .wake
                    .wait_timeout(state, Duration::from_millis(25))
                    .unwrap_or_else(|error| error.into_inner())
                    .0;
            }
        };
        match commit(repository, revision, pending.history) {
            Ok(snapshot) => {
                revision = snapshot.revision();
                worker_state(shared).receipt = Some(Receipt {
                    serial: pending.serial,
                    revision: Some(revision),
                    status: Status::Committed,
                    error: None,
                });
            }
            Err(error) => {
                let (observed, message) = failure(repository, error);
                fail(
                    shared,
                    observed,
                    format!("serial {} failed: {message}", pending.serial),
                );
                return;
            }
        }
    }
}
/// Three retries after an initial Busy result: no unbounded lock waiting and
/// no retry of CAS conflict, malformed state or uncertain publication.
fn retry_busy<T>(
    mut operation: impl FnMut() -> Result<T, history_store::Error>,
) -> Result<T, history_store::Error> {
    for attempt in 0..=3 {
        match operation() {
            Err(history_store::Error::Busy) if attempt < 3 => {
                std::thread::sleep(Duration::from_millis(25))
            }
            result => return result,
        }
    }
    Err(history_store::Error::Busy)
}
fn commit(
    repository: &Repository,
    revision: u64,
    history: History,
) -> Result<Snapshot, history_store::Error> {
    retry_busy(|| repository.commit_history(revision, history, &|| false))
}
fn failure(repository: &Repository, error: history_store::Error) -> (Option<u64>, String) {
    match error {
        history_store::Error::Conflict { current } => (
            Some(current.revision()),
            "external repository revision conflict; authoritative reload required".into(),
        ),
        error @ history_store::Error::Published { .. } => {
            // Rename may have succeeded. Read authority before reporting; an
            // observed revision is not a normal successful/durable receipt.
            match retry_busy(|| repository.load(&|| false)) {
                Ok(snapshot) => (
                    Some(snapshot.revision()),
                    format!(
                        "{error}; authoritative reload observed revision {}; reload required",
                        snapshot.revision()
                    ),
                ),
                Err(reload) => (
                    None,
                    format!("{error}; authoritative reload failed: {reload}; reload required"),
                ),
            }
        }
        other => (None, format!("{other}; authoritative reload required")),
    }
}
fn fail(shared: &Shared, revision: Option<u64>, message: String) {
    let mut state = worker_state(shared);
    let serial = state.last_serial;
    state.failed = true;
    state.pending = None;
    let mut message = format!("{message}; latest accepted serial {serial}");
    let mut end = message.len().min(512);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message.truncate(end);
    tracing::error!(serial, ?revision, error = %message, "Minecraft History writer requires reload");
    state.receipt = Some(Receipt {
        serial,
        revision,
        status: Status::ReloadRequired,
        error: Some(message),
    });
}
#[cfg(test)]
#[path = "history_writer_tests.rs"]
mod tests;
