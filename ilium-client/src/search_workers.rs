//! Owned background execution for workspace search.
//!
//! Search walks retained terminal journals that can be tens of megabytes, so
//! it must never execute on the client event loop that renders and accepts
//! keys. This manager permits one scan at a time, tracks its thread, and
//! lets the UI discard results by revision when newer typing supersedes it.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};

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
        let cancellation_requested = Arc::new(AtomicBool::new(false));
        let worker_cancellation = Arc::clone(&cancellation_requested);
        let events_tx = self.events_tx.clone();
        let revision = request.revision;
        let handle = thread::Builder::new()
            .name("ilium-workspace-search".to_string())
            .spawn(move || {
                let results = search_ui::search_workspace(&request, || {
                    worker_cancellation.load(Ordering::Relaxed)
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
}

impl Drop for SearchWorkers {
    fn drop(&mut self) {
        let Some(active) = self.active.take() else {
            return;
        };
        active.cancellation_requested.store(true, Ordering::Relaxed);
        // Do not block terminal restoration on a potentially large final
        // terminal-history scan. The worker observes the flag between sources
        // and no longer owns any TUI state after this manager is dropped.
        drop(active.handle);
    }
}
