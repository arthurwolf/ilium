//! Completion records private to one job execution.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::requests::Outcome;

#[derive(Debug)]
struct Entry {
    plugin: String,
    instance: u64,
    outcome: Option<Outcome>,
    completed_at: Option<Instant>,
}

/// A separate ceiling for completed-but-uncollected job results, not the pending-call limit.
const MAX_RETAINED_REQUESTS: usize = 4096;
const RESULT_RETENTION: Duration = Duration::from_secs(30);

/// Requests made by one job, isolated from its parent and sibling jobs.
#[derive(Debug, Clone)]
pub struct JobRequests {
    entries: Arc<(Mutex<HashMap<u64, Entry>>, Condvar)>,
    cancelled: Arc<AtomicBool>,
}

impl JobRequests {
    pub fn new(cancelled: Arc<AtomicBool>) -> Self {
        Self { entries: Arc::new((Mutex::new(HashMap::new()), Condvar::new())), cancelled }
    }

    pub fn register(&self, plugin: &str, instance: u64, request: RequestId) {
        let mut entries = self.entries.0.lock().unwrap_or_else(|error| error.into_inner());
        entries.insert(request.0, Entry { plugin: plugin.to_owned(), instance, outcome: None, completed_at: None });
    }

    pub fn complete(&self, request: RequestId, outcome: Outcome) {
        let mut entries = self.entries.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = entries.get_mut(&request.0) {
            if entry.outcome.is_none() {
                entry.outcome = Some(outcome);
                entry.completed_at = Some(Instant::now());
                self.entries.1.notify_all();
            }
        }
    }

    /// Called before starting a slow call so retained results cannot grow without bound.
    pub fn ensure_capacity(&self) -> Result<(), PluginError> {
        let mut entries = self.entries.0.lock().unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        entries.retain(|_, entry| entry.completed_at.is_none_or(|at| now.saturating_duration_since(at) < RESULT_RETENTION));
        if entries.len() >= MAX_RETAINED_REQUESTS {
            return Err(PluginError { code: ErrorCode::Busy, message: "job request results are at capacity".to_owned(), retry_after_ms: None });
        }
        Ok(())
    }

    pub fn wait(&self, plugin: &str, instance: u64, request: RequestId, timeout: Duration) -> Result<(), PluginError> {
        let failure = |code: ErrorCode, message: &str| PluginError { code, message: message.to_owned(), retry_after_ms: None };
        let deadline = Instant::now().checked_add(timeout).unwrap_or_else(Instant::now);
        let mut entries = self.entries.0.lock().unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        entries.retain(|_, entry| entry.completed_at.is_none_or(|at| now.saturating_duration_since(at) < RESULT_RETENTION));
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return Err(failure(ErrorCode::Cancelled, "job was cancelled"));
            }
            let Some(entry) = entries.get(&request.0).filter(|entry| entry.plugin == plugin && entry.instance == instance) else {
                return Err(failure(ErrorCode::NotFound, "request is not owned by this job"));
            };
            if let Some(outcome) = entry.outcome.clone() {
                entries.remove(&request.0);
                return match outcome {
                    Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_) | Outcome::Duplicate(_) => Ok(()),
                    Outcome::Refused(error) | Outcome::Failed(error) => Err(error),
                    Outcome::Cancelled => Err(failure(ErrorCode::Cancelled, "request was cancelled")),
                };
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(failure(ErrorCode::TimedOut, "request did not finish in time"));
            }
            let (next, _) = self.entries.1.wait_timeout(entries, remaining.min(Duration::from_millis(100)))
                .unwrap_or_else(|error| error.into_inner());
            entries = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_interrupts_a_pending_wait() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let book = JobRequests::new(Arc::clone(&cancelled));
        book.register("test.plugin", 1, RequestId(7));
        cancelled.store(true, Ordering::Release);
        let failure = book.wait("test.plugin", 1, RequestId(7), Duration::from_secs(1)).unwrap_err();
        assert_eq!(failure.code, ErrorCode::Cancelled);
    }

    #[test]
    fn result_capacity_refuses_new_work_without_discarding_a_recent_completion() {
        let book = JobRequests::new(Arc::new(AtomicBool::new(false)));
        for id in 0..MAX_RETAINED_REQUESTS {
            let request = RequestId(id as u64);
            book.register("test.plugin", 1, request);
            book.complete(request, Outcome::Ok);
        }
        assert_eq!(book.ensure_capacity().unwrap_err().code, ErrorCode::Busy);
        book.wait("test.plugin", 1, RequestId(0), Duration::ZERO).unwrap();
        assert!(book.ensure_capacity().is_ok());
    }

    #[test]
    fn expired_completed_results_free_capacity() {
        let book = JobRequests::new(Arc::new(AtomicBool::new(false)));
        let request = RequestId(7);
        book.register("test.plugin", 1, request);
        book.complete(request, Outcome::Ok);
        let mut entries = book.entries.0.lock().unwrap();
        entries.get_mut(&request.0).unwrap().completed_at = Some(Instant::now() - RESULT_RETENTION);
        drop(entries);
        assert!(book.ensure_capacity().is_ok());
        assert_eq!(book.wait("test.plugin", 1, request, Duration::ZERO).unwrap_err().code, ErrorCode::NotFound);
    }
}
