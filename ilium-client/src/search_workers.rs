//! Owned background execution for workspace search.
//!
//! Search walks retained terminal journals that can be tens of megabytes, so
//! it must never execute on the client event loop that renders and accepts
//! keys. This manager permits one scan at a time, tracks its thread, and
//! lets the UI discard results by revision when newer typing supersedes it.

use std::panic::{self, AssertUnwindSafe};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};

use ilium_platform::thread_priority::{lower_current_thread, WorkerPriority};
use tokio::sync::mpsc::Sender;

use crate::search_ui::{self, SearchResult, WorkspaceSearchRequest};

/// A completed background scan delivered into the main UI event loop.
#[derive(Debug)]
pub struct SearchWorkerEvent {
    pub revision: u64,
    pub results: Vec<SearchResult>,
}

/// One tracked scan thread. Keeping the handle and cancellation flag here
/// makes worker lifetime explicit instead of leaving detached search tasks
/// behind when the client closes.
pub struct SearchWorkers {
    events_tx: Sender<SearchWorkerEvent>,
    active: Option<ActiveSearchWorker>,
}

struct ActiveSearchWorker {
    cancellation_requested: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl SearchWorkers {
    pub fn new(events_tx: Sender<SearchWorkerEvent>) -> Self {
        Self {
            events_tx,
            active: None,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.active.is_none()
    }

    /// Starts a scan only when no older scan remains owned. A worker sends
    /// exactly one event after it has finished computing (unless cancelled).
    pub fn start(&mut self, request: WorkspaceSearchRequest) -> Result<(), std::io::Error> {
        debug_assert!(
            self.active.is_none(),
            "only one workspace scan may run at once"
        );
        // The assert above is compiled out in release builds, so guard the
        // invariant for real here too: overwriting `self.active` while a
        // worker is still tracked would drop its `JoinHandle` with no
        // cancellation requested, leaking a detached thread that keeps
        // running (and keeps its captured request/results alive) with
        // nothing left able to join or stop it.
        if let Some(stale) = self.active.take() {
            Self::cancel_worker(stale);
        }
        let cancellation_requested = Arc::new(AtomicBool::new(false));
        let worker_cancellation = Arc::clone(&cancellation_requested);
        let events_tx = self.events_tx.clone();
        let revision = request.revision;
        let handle = thread::Builder::new()
            .name("ilium-workspace-search".to_string())
            .spawn(move || {
                lower_current_thread(WorkerPriority::Lowest);
                // `is_idle` has no way to observe this thread's death other
                // than the completion event it promises to send (see the
                // doc comment on `start`): a scan across arbitrary retained
                // terminal text can hit an edge case (e.g. a byte-slicing
                // bug on a UTF-8 boundary) and panic. Contain that here so a
                // single bad scan degrades to "no results" instead of
                // wedging `SearchWorkers` in `is_idle() == false` forever,
                // which would silently disable workspace search for the
                // rest of the client's run.
                let results = panic::catch_unwind(AssertUnwindSafe(|| {
                    search_ui::search_workspace(&request, || {
                        worker_cancellation.load(Ordering::Relaxed)
                    })
                }))
                .unwrap_or_else(|panic_payload| {
                    tracing::error!(
                        "workspace search worker panicked: {}",
                        panic_payload_message(&panic_payload)
                    );
                    Vec::new()
                });
                if worker_cancellation.load(Ordering::Relaxed) {
                    return;
                }
                let _ = events_tx.blocking_send(SearchWorkerEvent { revision, results });
            })?;
        self.active = Some(ActiveSearchWorker {
            cancellation_requested,
            handle,
        });
        Ok(())
    }

    /// Reaps the completed worker after its event is observed. The send is
    /// its final operation, so joining here is bounded to thread teardown,
    /// not the expensive scan itself.
    pub fn finish(&mut self) {
        let Some(active) = self.active.take() else {
            return;
        };
        if active.handle.join().is_err() {
            tracing::error!("workspace search worker panicked");
        }
    }

    /// Requests cancellation and detaches a worker's thread without joining
    /// it. Shared by `start` (replacing a stale worker) and `Drop` (final
    /// teardown): in both cases the thread's own cooperative cancellation
    /// check -- not a join -- is what stops it, so this never blocks on the
    /// scan itself.
    fn cancel_worker(active: ActiveSearchWorker) {
        active.cancellation_requested.store(true, Ordering::Relaxed);
        // Do not block terminal restoration (or the next scan) on a
        // potentially large terminal-history scan. The worker observes the
        // flag between sources and no longer owns any TUI state once it is
        // no longer tracked here.
        drop(active.handle);
    }
}

/// Extracts a human-readable message from a caught panic payload for the log
/// line in `start`. `std::panic!` payloads are almost always `&str` or
/// `String`; anything else still logs usefully rather than silently
/// swallowing the panic's presence.
fn panic_payload_message(panic_payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic_payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = panic_payload.downcast_ref::<String>() {
        return message.clone();
    }
    "non-string panic payload".to_string()
}

impl Drop for SearchWorkers {
    fn drop(&mut self) {
        let Some(active) = self.active.take() else {
            return;
        };
        Self::cancel_worker(active);
    }
}
