//! Bounded latest-query mailbox around one supervised stateful native engine.
use super::{IconSearchRequest, IconSemanticIndex, IconSemanticSearchEvent};
use crate::icon_settings::IconPickerSearchResults;
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::{
    file_lock::ExclusiveFileLock,
    owned_worker::{spawn_owned, OwnedWorker, StopToken, WorkerExit, WorkerKind},
};
use std::{
    borrow::Cow,
    fmt, io,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

const OWNER_BYTES: usize = 512 * 1024 * 1024;
const REQUEST_RESULT_BYTES: usize = 128 * 1024;
const MAX_ERROR_BYTES: usize = 4096;

#[derive(Clone, Debug)]
pub struct IconSearchFailure(Arc<Failure>);
#[derive(Debug)]
struct Failure {
    message: Cow<'static, str>,
    _storage: Option<Arc<StorageAdmission>>,
}
impl IconSearchFailure {
    pub(crate) fn from_static(message: &'static str) -> Self {
        Self(Arc::new(Failure {
            message: Cow::Borrowed(message),
            _storage: None,
        }))
    }
    fn owned(mut message: String, storage: Arc<StorageAdmission>) -> Self {
        let mut end = message.len().min(MAX_ERROR_BYTES);
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.shrink_to_fit();
        Self(Arc::new(Failure {
            message: Cow::Owned(message),
            _storage: Some(storage),
        }))
    }
}
impl fmt::Display for IconSearchFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.message.fmt(f)
    }
}
impl PartialEq for IconSearchFailure {
    fn eq(&self, other: &Self) -> bool {
        self.0.message == other.0.message
    }
}
impl Eq for IconSearchFailure {}

struct Pending {
    request: IconSearchRequest,
    storage: Arc<StorageAdmission>,
}
#[derive(Default)]
struct Mailbox {
    pending: Option<Pending>,
    result: Option<IconSemanticSearchEvent>,
    closing: bool,
    active_revision: Option<u64>,
    latest_revision: Option<u64>,
}
struct Shared {
    mailbox: Mutex<Mailbox>,
    changed: Condvar,
    notification: Arc<tokio::sync::Notify>,
}
impl Shared {
    fn close(&self) {
        let mut mailbox = self
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        mailbox.closing = true;
        mailbox.pending = None;
        self.changed.notify_all();
        self.notification.notify_one();
    }
}
trait SearchBackend: Send {
    fn search(&mut self, query: &str) -> Result<IconPickerSearchResults, String>;
}
type Factory = Arc<dyn Fn(&StopToken) -> Result<Box<dyn SearchBackend>, String> + Send + Sync>;
struct NativeBackend {
    index: IconSemanticIndex,
    // Model teardown precedes release of the cross-client native-engine slot.
    _host_slot: ExclusiveFileLock,
}
impl SearchBackend for NativeBackend {
    fn search(&mut self, query: &str) -> Result<IconPickerSearchResults, String> {
        self.index.search(query)
    }
}

pub struct IconSearchWorkers {
    shared: Arc<Shared>,
    quota: QuotaGroup,
    factory: Factory,
    worker: Option<OwnedWorker>,
    // Keep preflight credit through physical admission refusal; repeatedly
    // dropping it would wake the same rejected UI request indefinitely.
    pending_storage: Option<Arc<StorageAdmission>>,
}
impl Default for IconSearchWorkers {
    fn default() -> Self {
        Self::new()
    }
}
impl IconSearchWorkers {
    pub fn new() -> Self {
        Self::with_factory(
            crate::execution::process_quota(),
            Arc::new(|stop| {
                let directory = super::icon_model_cache_dir().join("worker-admission");
                let mut slot = None;
                // Clients sharing the same model-cache namespace may own at most
                // two native engines. OS locks are released on process failure.
                for index in 0..2 {
                    let acquired = ExclusiveFileLock::try_acquire(
                        &directory.join(format!("engine-{index}.lock")),
                    )
                    .map_err(|error| format!("icon engine admission: {error}"))?;
                    if acquired.is_some() {
                        slot = acquired;
                        break;
                    }
                }
                let slot = slot.ok_or_else(|| {
                    "icon engine capacity is occupied by other clients".to_owned()
                })?;
                Ok(Box::new(NativeBackend {
                    index: super::build_index(stop)?,
                    _host_slot: slot,
                }))
            }),
        )
    }
    fn with_factory(quota: QuotaGroup, factory: Factory) -> Self {
        Self {
            shared: Arc::new(Shared {
                mailbox: Mutex::new(Mailbox::default()),
                changed: Condvar::new(),
                notification: Arc::new(tokio::sync::Notify::new()),
            }),
            quota,
            factory,
            worker: None,
            pending_storage: None,
        }
    }
    pub fn notification(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.shared.notification)
    }
    /// A refused query is returned intact. Typing may replace only the pending
    /// query; input bytes and semantic simulations never use this mailbox.
    pub fn try_request(&mut self, request: IconSearchRequest) -> Result<(), IconSearchRequest> {
        if request.query.len() > super::MAX_QUERY_BYTES || request.query.capacity() > 32 * 1024 {
            let Ok(mut mailbox) = self.shared.mailbox.try_lock() else {
                return Err(request);
            };
            mailbox.latest_revision = Some(request.revision);
            mailbox.result = Some(IconSemanticSearchEvent::Failed {
                revision: request.revision,
                message: IconSearchFailure::from_static(
                    "icon query exceeds the supported byte limit",
                ),
            });
            self.shared.notification.notify_one();
            return Ok(());
        }
        if self.pending_storage.is_none() {
            let Ok(storage) = self.quota.reserve_external_storage(REQUEST_RESULT_BYTES) else {
                return Err(request);
            };
            self.pending_storage = Some(Arc::new(storage));
        }
        if self
            .worker
            .as_ref()
            .is_some_and(|owner| owner.ticket().exit().is_some())
        {
            drop(self.worker.take());
            let Ok(mut mailbox) = self.shared.mailbox.try_lock() else {
                return Err(request);
            };
            mailbox.closing = false;
        }
        if self.worker.is_none() {
            let Ok(admission) = self.quota.reserve_external_worker(1, OWNER_BYTES) else {
                return Err(request);
            };
            let shared = Arc::clone(&self.shared);
            let wake = Arc::clone(&shared);
            let factory = Arc::clone(&self.factory);
            let owner = spawn_owned(
                "ilium-icon-search",
                WorkerKind::Cooperative,
                StopToken::default(),
                move || {
                    let _admission = &admission;
                    wake.close();
                },
                move |stop| run_worker(shared, factory, stop),
            );
            match owner {
                Ok(owner) => self.worker = Some(owner),
                Err(error) => {
                    tracing::error!(%error, "icon worker start failed");
                    return Err(request);
                }
            }
        }
        let Ok(mut mailbox) = self.shared.mailbox.try_lock() else {
            return Err(request);
        };
        if mailbox.closing {
            return Err(request);
        }
        let Some(storage) = self.pending_storage.take() else {
            return Err(request);
        };
        mailbox.latest_revision = Some(request.revision);
        mailbox.pending = Some(Pending { request, storage });
        self.shared.changed.notify_one();
        Ok(())
    }
    pub fn poll(&self) -> Option<IconSemanticSearchEvent> {
        self.shared.mailbox.try_lock().ok()?.result.take()
    }
    pub fn cancel(&mut self) {
        self.shared.close();
        self.pending_storage = None;
        if let Some(owner) = &self.worker {
            owner.ticket().cancel();
        }
    }
    pub async fn shutdown(mut self) -> io::Result<()> {
        self.shared.close();
        let Some(owner) = self.worker.take() else {
            return Ok(());
        };
        let ticket = owner.ticket();
        drop(owner);
        let observation = tokio::task::spawn_blocking(move || {
            ticket.join_until(Instant::now() + Duration::from_secs(5))
        })
        .await
        .map_err(|error| io::Error::other(error.to_string()))?;
        match observation {
            Ok(WorkerExit::Joined) => Ok(()),
            Ok(WorkerExit::Panicked) => Err(io::Error::other("icon engine worker panicked")),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "icon engine still supervised after shutdown deadline",
            )),
        }
    }
}
impl Drop for IconSearchWorkers {
    fn drop(&mut self) {
        self.shared.close();
        drop(self.worker.take());
    }
}
fn run_worker(shared: Arc<Shared>, factory: Factory, stop: StopToken) {
    struct Completion(Arc<Shared>);
    impl Drop for Completion {
        fn drop(&mut self) {
            let mut mailbox = self
                .0
                .mailbox
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if !mailbox.closing {
                if let Some(revision) = mailbox
                    .latest_revision
                    .take()
                    .or(mailbox.active_revision.take())
                {
                    mailbox.result = Some(IconSemanticSearchEvent::Failed {
                        revision,
                        message: IconSearchFailure::from_static(
                            "icon worker ended before publishing its result",
                        ),
                    });
                }
                mailbox.closing = true;
                mailbox.pending = None;
                self.0.notification.notify_one();
            }
        }
    }
    let _completion = Completion(Arc::clone(&shared));
    ilium_platform::thread_priority::lower_current_thread(
        ilium_platform::thread_priority::WorkerPriority::Lowest,
    );
    let mut backend = None;
    loop {
        let pending = {
            let mut mailbox = shared
                .mailbox
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while !mailbox.closing && mailbox.pending.is_none() {
                mailbox = shared
                    .changed
                    .wait(mailbox)
                    .unwrap_or_else(|error| error.into_inner());
            }
            if mailbox.closing || stop.is_stopped() {
                return;
            }
            let pending = mailbox.pending.take();
            mailbox.active_revision = pending.as_ref().map(|pending| pending.request.revision);
            pending
        };
        let Some(Pending { request, storage }) = pending else {
            continue;
        };
        let result = (|| {
            if backend.is_none() {
                backend = Some(factory(&stop)?);
            }
            if stop.is_stopped() {
                return Err("icon search cancelled".to_owned());
            }
            let Some(engine) = backend.as_mut() else {
                return Err("icon engine unavailable".to_owned());
            };
            engine
                .search(&request.query)?
                .retain_storage(Arc::clone(&storage))
                .map_err(|_| {
                    "icon result allocation unexpectedly shared before publication".to_owned()
                })
        })();
        let event = match result {
            Ok(results) => IconSemanticSearchEvent::Results {
                revision: request.revision,
                results,
            },
            Err(message) => {
                tracing::warn!(revision=request.revision, %message, "icon semantic search failed");
                IconSemanticSearchEvent::Failed {
                    revision: request.revision,
                    message: IconSearchFailure::owned(message, storage),
                }
            }
        };
        let mut mailbox = shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if mailbox.closing {
            return;
        }
        mailbox.active_revision = None;
        if mailbox.latest_revision == Some(request.revision) {
            mailbox.result = Some(event);
            shared.notification.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    const WAIT: Duration = Duration::from_secs(3);
    struct Controlled {
        entered: mpsc::SyncSender<String>,
        release: Arc<Mutex<mpsc::Receiver<()>>>,
    }
    impl SearchBackend for Controlled {
        fn search(&mut self, query: &str) -> Result<IconPickerSearchResults, String> {
            self.entered.send(query.to_owned()).unwrap();
            self.release.lock().unwrap().recv_timeout(WAIT).unwrap();
            Ok(crate::icon_settings::semantic_picker_search_results(
                Vec::new(),
            ))
        }
    }
    type Harness = (
        IconSearchWorkers,
        QuotaGroup,
        mpsc::Receiver<String>,
        mpsc::SyncSender<()>,
    );
    fn controlled() -> Harness {
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            worker_threads: 1,
            worker_bytes: OWNER_BYTES + 8 * REQUEST_RESULT_BYTES,
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
        });
        let (entered, observations) = mpsc::sync_channel(8);
        let (release, wait) = mpsc::sync_channel(8);
        let wait = Arc::new(Mutex::new(wait));
        let factory = Arc::new(
            move |_: &StopToken| -> Result<Box<dyn SearchBackend>, String> {
                Ok(Box::new(Controlled {
                    entered: entered.clone(),
                    release: Arc::clone(&wait),
                }))
            },
        );
        (
            IconSearchWorkers::with_factory(quota.clone(), factory),
            quota,
            observations,
            release,
        )
    }
    fn request(revision: u64, query: &str) -> IconSearchRequest {
        IconSearchRequest {
            revision,
            query: query.to_owned(),
        }
    }
    fn result(worker: &IconSearchWorkers) -> IconSemanticSearchEvent {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(event) = worker.poll() {
                return event;
            }
            assert!(Instant::now() < deadline, "real worker result");
            std::thread::yield_now();
        }
    }
    fn admit(worker: &mut IconSearchWorkers, mut input: IconSearchRequest) {
        let deadline = Instant::now() + WAIT;
        loop {
            match worker.try_request(input) {
                Ok(()) => return,
                Err(original) => input = original,
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[test]
    fn blocked_engine_coalesces_only_pending_queries_and_keeps_native_owner() {
        let (mut worker, quota, entered, release) = controlled();
        admit(&mut worker, request(1, "first"));
        assert_eq!(entered.recv_timeout(WAIT).unwrap(), "first");
        admit(&mut worker, request(2, "superseded"));
        admit(&mut worker, request(3, "newest"));
        assert_eq!(quota.snapshot().worker_threads, 1);
        assert_eq!(
            quota.snapshot().worker_bytes,
            OWNER_BYTES + 2 * REQUEST_RESULT_BYTES
        );
        release.send(()).unwrap();
        assert_eq!(entered.recv_timeout(WAIT).unwrap(), "newest");
        release.send(()).unwrap();
        let mut event = result(&worker);
        if matches!(event, IconSemanticSearchEvent::Results { revision: 1, .. }) {
            drop(event);
            event = result(&worker);
        }
        assert!(matches!(
            event,
            IconSemanticSearchEvent::Results { revision: 3, .. }
        ));
        let ticket = worker.worker.as_ref().unwrap().ticket();
        drop(worker);
        assert_eq!(
            ticket.join_until(Instant::now() + WAIT),
            Ok(WorkerExit::Joined)
        );
    }
    #[test]
    fn old_completion_cannot_overwrite_a_newer_query_failure() {
        let (mut worker, _, entered, release) = controlled();
        admit(&mut worker, request(1, "old"));
        entered.recv_timeout(WAIT).unwrap();
        worker
            .try_request(request(2, &"x".repeat(super::super::MAX_QUERY_BYTES + 1)))
            .unwrap();
        release.send(()).unwrap();
        let deadline = Instant::now() + WAIT;
        loop {
            if worker
                .shared
                .mailbox
                .lock()
                .unwrap()
                .active_revision
                .is_none()
            {
                break;
            }
            assert!(Instant::now() < deadline, "old callback completed");
            std::thread::yield_now();
        }
        assert!(matches!(
            worker.poll(),
            Some(IconSemanticSearchEvent::Failed { revision: 2, .. })
        ));
        let ticket = worker.worker.as_ref().unwrap().ticket();
        drop(worker);
        assert_eq!(
            ticket.join_until(Instant::now() + WAIT),
            Ok(WorkerExit::Joined)
        );
    }

    #[test]
    fn retained_immutable_result_outlives_native_join_and_all_picker_clones() {
        let (mut worker, quota, entered, release) = controlled();
        admit(&mut worker, request(1, "query"));
        entered.recv_timeout(WAIT).unwrap();
        release.send(()).unwrap();
        let IconSemanticSearchEvent::Results { results, .. } = result(&worker) else {
            panic!("result");
        };
        let clone = results.clone();
        let ticket = worker.worker.as_ref().unwrap().ticket();
        drop(worker);
        assert_eq!(
            ticket.join_until(Instant::now() + WAIT),
            Ok(WorkerExit::Joined)
        );
        drop(ticket);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, REQUEST_RESULT_BYTES);
        drop(results);
        assert_eq!(quota.snapshot().worker_bytes, REQUEST_RESULT_BYTES);
        drop(clone);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn cancellation_returns_before_blocked_native_callback_and_retains_charge() {
        let (mut worker, quota, entered, release) = controlled();
        admit(&mut worker, request(1, "blocked"));
        entered.recv_timeout(WAIT).unwrap();
        let ticket = worker.worker.as_ref().unwrap().ticket();
        let started = Instant::now();
        drop(worker);
        let returned_promptly = started.elapsed() < Duration::from_secs(1);
        let still_owned = quota.snapshot().worker_threads == 1 && ticket.exit().is_none();
        release.send(()).unwrap();
        assert_eq!(
            ticket.join_until(Instant::now() + WAIT),
            Ok(WorkerExit::Joined)
        );
        assert!(returned_promptly && still_owned);
        drop(ticket);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn refusal_returns_the_same_original_query_without_starting_an_engine() {
        let (mut worker, quota, _, _) = controlled();
        let occupied = quota.reserve_external_worker(1, 0).unwrap();
        let original = request(3, "retain exact string");
        let address = original.query.as_ptr();
        let returned = worker.try_request(original).unwrap_err();
        assert_eq!(returned.query.as_ptr(), address);
        assert!(worker.worker.is_none());
        assert_eq!(quota.snapshot().worker_bytes, REQUEST_RESULT_BYTES);
        let mut original = returned;
        for _ in 0..16 {
            original = worker.try_request(original).unwrap_err();
            assert_eq!(original.query.as_ptr(), address);
            assert_eq!(quota.snapshot().worker_bytes, REQUEST_RESULT_BYTES);
        }
        worker.cancel();
        assert_eq!(quota.snapshot().worker_bytes, 0);
        drop(occupied);
    }
}
