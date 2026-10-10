//! Owned polling with a single coalesced snapshot; never a growing frame queue.
use super::model::FeedState;
use crate::{
    resources::{AmbientResources, WorkerCost},
    source::{sleep_unless_stopped, Worker},
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const POLLER_WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;
const POLLER_WORKER_RESIDENT_BYTES: usize = 4 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn live_poller_charges_its_native_worker_and_retires_after_join() {
        let (_execution, resources) = crate::resources::isolated_test_resources();
        let quota = resources.finite().quota_group();
        let before = quota.snapshot();
        let (entered_tx, entered_rx) = mpsc::channel();
        let poller = Poller::start(
            &resources,
            "poller-admission-regression",
            Duration::from_secs(60),
            Duration::from_secs(1),
            move |stop| {
                entered_tx.send(()).unwrap();
                while !stop.load(std::sync::atomic::Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(((), None))
            },
        )
        .expect("poller starts");
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("poller begins its first fetch");
        let ticket = poller
            .worker
            .as_ref()
            .and_then(Worker::join_observer)
            .expect("native worker ticket");

        assert_eq!(
            quota.snapshot().worker_threads,
            before.worker_threads + 1,
            "live polling consumes the shared process worker allowance"
        );
        assert_eq!(
            ticket.metadata().requested_stack_bytes,
            Some(POLLER_WORKER_STACK_BYTES),
            "polling worker requests a stack covered by its resident debit"
        );

        drop(poller);
        assert_eq!(
            ticket.join_until(std::time::Instant::now() + Duration::from_secs(2)),
            Ok(ilium_platform::owned_worker::WorkerExit::Joined)
        );
        assert_eq!(quota.snapshot().worker_threads, before.worker_threads);
        drop(ticket);
    }

    #[test]
    fn live_poller_refuses_before_start_when_shared_worker_quota_is_full() {
        let (_execution, resources) = crate::resources::isolated_test_resources();
        let quota = resources.finite().quota_group();
        let before = quota.snapshot();
        let remaining_threads = before
            .limits
            .worker_threads
            .saturating_sub(before.worker_threads);
        assert!(remaining_threads > 0, "test quota has a free worker slot");
        let blocker = resources
            .reserve_worker(WorkerCost {
                threads: remaining_threads,
                resident_bytes: 1,
            })
            .expect("occupy the complete remaining worker allowance");
        let (called_tx, called_rx) = mpsc::channel();
        let result = Poller::<u32>::start(
            &resources,
            "poller-admission-refusal",
            Duration::from_secs(60),
            Duration::from_secs(1),
            move |_| {
                called_tx.send(()).unwrap();
                Ok((1, None))
            },
        );
        assert!(result.is_err(), "worker admission must be explicit");
        assert!(
            called_rx.try_recv().is_err(),
            "refusal must precede the fetch"
        );
        drop(blocker);
    }

    #[test]
    fn failed_request_keeps_last_good_data_and_observation_timestamp() {
        let mut snapshot = Snapshot::<u32>::default();
        snapshot.apply(1000, Ok((42, Some(500))));
        snapshot.apply(2000, Err("offline".into()));
        assert_eq!(snapshot.data.as_deref(), Some(&42));
        assert_eq!(snapshot.state.observed_ms, Some(500));
        assert_eq!(snapshot.state.received_ms, Some(1000));
        assert_eq!(snapshot.state.error.as_deref(), Some("offline"));
        snapshot.apply(3000, Ok((42, Some(500))));
        assert_eq!(snapshot.state.observed_ms, Some(500));
        assert!(snapshot.state.error.is_none());
    }

    #[test]
    fn malformed_provider_refresh_preserves_last_good_but_valid_empty_replaces_it() {
        use super::super::{model::Earthquake, parse};

        let first = br#"{"type":"FeatureCollection","features":[{"id":"fixture","properties":{"time":500,"mag":null},"geometry":{"type":"Point","coordinates":[0,0]}}]}"#;
        let mut snapshot = Snapshot::<Vec<Earthquake>>::default();
        snapshot.apply(
            1000,
            parse::usgs(first).map(|decoded| (decoded.items, Some(500))),
        );
        let previous = Arc::clone(snapshot.data.as_ref().unwrap());

        let malformed = br#"{"type":"FeatureCollection","features":[null,{}]}"#;
        snapshot.apply(
            2000,
            parse::usgs(malformed).map(|decoded| (decoded.items, None)),
        );
        assert!(Arc::ptr_eq(snapshot.data.as_ref().unwrap(), &previous));
        assert_eq!(snapshot.state.received_ms, Some(1000));
        assert_eq!(snapshot.state.observed_ms, Some(500));
        assert!(snapshot.state.error.is_some());

        let empty = br#"{"type":"FeatureCollection","features":[]}"#;
        snapshot.apply(
            3000,
            parse::usgs(empty).map(|decoded| (decoded.items, None)),
        );
        assert!(snapshot.data.as_ref().unwrap().is_empty());
        assert_eq!(snapshot.state.received_ms, Some(3000));
        assert_eq!(snapshot.state.observed_ms, None);
        assert!(snapshot.state.error.is_none());
    }
    #[test]
    fn retry_schedule_respects_source_floor_and_caps_exponential_backoff() {
        assert_eq!(
            retry_delay(Duration::from_secs(60), 0),
            Duration::from_secs(60)
        );
        assert_eq!(
            retry_delay(Duration::from_secs(60), 2),
            Duration::from_secs(240)
        );
        assert_eq!(
            retry_delay(Duration::from_secs(60), 32),
            Duration::from_secs(900)
        );
        assert_eq!(
            retry_delay(Duration::from_secs(1200), 32),
            Duration::from_secs(1200)
        );
    }
}

#[derive(Debug)]
pub struct Snapshot<T> {
    pub data: Option<Arc<T>>,
    pub state: FeedState,
}

impl<T> Default for Snapshot<T> {
    fn default() -> Self {
        Self {
            data: None,
            state: FeedState::default(),
        }
    }
}

impl<T> Clone for Snapshot<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            state: self.state.clone(),
        }
    }
}

impl<T> Snapshot<T> {
    fn apply(&mut self, received_ms: i64, result: Result<(T, Option<i64>), String>) {
        match result {
            Ok((data, observed_ms)) => {
                self.data = Some(Arc::new(data));
                self.state.received(received_ms, observed_ms);
            }
            Err(error) => self.state.failed(error),
        }
    }
}

fn retry_delay(interval: Duration, failures: u32) -> Duration {
    interval
        .saturating_mul(1_u32 << failures.min(8))
        .min(Duration::from_secs(900).max(interval))
}

/// One owned source request loop. Closures must bound network and decode work;
/// dropping the poller requests stop and transfers the bounded join to the
/// existing ambient reaper so the presentation thread cannot stall on HTTP.
pub struct Poller<T> {
    snapshot: Arc<Mutex<Arc<Snapshot<T>>>>,
    worker: Option<Worker>,
}

impl<T: Send + Sync + 'static> Poller<T> {
    pub fn start(
        resources: &AmbientResources,
        name: &str,
        requested: Duration,
        minimum: Duration,
        mut fetch: impl FnMut(&std::sync::atomic::AtomicBool) -> Result<(T, Option<i64>), String>
            + Send
            + 'static,
    ) -> Result<Self, String> {
        // Reserve the persistent native thread before allocating retained state
        // or moving the fetch closure into a worker. The shared host quota is
        // also charged until the platform supervisor physically joins it.
        let admission = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: POLLER_WORKER_RESIDENT_BYTES,
            })
            .map_err(|error| format!("live source worker admission unavailable: {error:?}"))?;
        let interval = requested
            .min(Duration::from_secs(86400))
            .max(minimum)
            .max(Duration::from_secs(1));
        let snapshot = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
        let worker_snapshot = Arc::clone(&snapshot);
        let worker = Worker::start_admitted_with_stack(
            name,
            admission,
            Some(POLLER_WORKER_STACK_BYTES),
            move |stop| {
                ilium_platform::thread_priority::lower_current_thread(
                    ilium_platform::thread_priority::WorkerPriority::Lowest,
                );
                let mut failures = 0_u32;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let result = fetch(&stop);
                    failures = if result.is_ok() {
                        0
                    } else {
                        failures.saturating_add(1)
                    };
                    let received_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |elapsed| {
                            elapsed.as_millis().min(i64::MAX as u128) as i64
                        });
                    if let Ok(mut guard) = worker_snapshot.lock() {
                        let mut next = (**guard).clone();
                        next.apply(received_ms, result);
                        *guard = Arc::new(next);
                    }
                    if !sleep_unless_stopped(&stop, retry_delay(interval, failures)) {
                        break;
                    }
                }
            },
        )
        .map_err(|error| format!("could not start live source worker: {error}"))?;
        Ok(Self {
            snapshot,
            worker: Some(worker),
        })
    }

    /// Never wait for a worker lock on a render path. Hosts retain their
    /// previous Arc when this returns None and paint the latest good data.
    pub fn try_snapshot(&self) -> Option<Arc<Snapshot<T>>> {
        self.snapshot
            .try_lock()
            .ok()
            .map(|guard| Arc::clone(&guard))
    }
}

impl<T> Drop for Poller<T> {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}
