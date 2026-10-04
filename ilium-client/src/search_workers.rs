//! Finite workspace scans on the client's existing fixed OS CPU bank.
//! One receipt owns the active scan; cancellation never joins a thread on UI.
use crate::search_ui::{self, SearchResult, WorkspaceSearchRequest};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, Reservation,
    StorageAdmission,
};
use std::sync::Arc;
use tokio::sync::Notify;

#[derive(Clone, Debug)]
pub(crate) enum SourceFence {
    Terminal {
        pane: ilium_core::NodeId,
        instance: Arc<()>,
        origin: Arc<()>,
    },
    Editor {
        pane: ilium_core::NodeId,
        instance: Arc<()>,
        revision: u64,
    },
    Board {
        pane: ilium_core::NodeId,
        instance: Arc<()>,
        revision: u64,
    },
}
#[derive(Debug)]
pub struct SearchWorkerEvent {
    pub revision: u64,
    pub(crate) identity: Arc<()>,
    pub results: Vec<SearchResult>,
    pub error: Option<String>,
    pub(crate) fences: Vec<SourceFence>,
    // Payload fields must die before their allocation charge.
    pub(crate) retention: Option<Arc<StorageAdmission>>,
}
struct SearchOutput {
    fences: Vec<SourceFence>,
    results: Vec<SearchResult>,
    storage: StorageAdmission,
}

struct SearchJob {
    request: WorkspaceSearchRequest,
}
impl Job for SearchJob {
    type Output = SearchOutput;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, String> {
        if context.stop_requested() {
            return Err("workspace search cancelled".into());
        }
        let results = search_ui::search_workspace(&self.request, || context.stop_requested());
        if context.stop_requested() {
            return Err("workspace search cancelled".into());
        }
        let bytes = result_bytes(&results).saturating_add(
            self.request
                .fences
                .capacity()
                .saturating_mul(std::mem::size_of::<SourceFence>()),
        );
        if bytes > context.cost().result_bytes {
            return Err("workspace search result exceeds admitted bytes".into());
        }
        let storage = crate::execution::process_quota()
            .reserve_external_storage(bytes)
            .map_err(|error| format!("workspace search result storage: {error:?}"))?;
        Ok(SearchOutput {
            results,
            fences: self.request.fences,
            storage,
        })
    }
}
struct Active {
    revision: u64,
    identity: Arc<()>,
    receipt: Receipt<SearchJob>,
}
pub struct SearchAdmission {
    reservation: Reservation,
    cost: JobCost,
    owner: Arc<()>,
}
pub struct SearchWorkers {
    identity: Arc<()>,
    client: Client,
    notification: Arc<Notify>,
    active: Option<Active>,
}
impl SearchWorkers {
    pub fn new(client: Client) -> Self {
        let notification = Arc::new(Notify::new());
        let wake = Arc::clone(&notification);
        Self {
            identity: Arc::new(()),
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            active: None,
        }
    }
    pub fn notification(&self) -> Arc<Notify> {
        Arc::clone(&self.notification)
    }
    pub fn is_idle(&self) -> bool {
        self.active.is_none()
    }
    /// Admission precedes source cloning and history pinning. Scratch is included.
    pub fn reserve(&self, cost: JobCost) -> Result<SearchAdmission, String> {
        if !self.is_idle() {
            return Err("a workspace search is still retiring".into());
        }
        self.client
            .try_reserve(Lane::Cpu, cost)
            .map(|reservation| SearchAdmission {
                reservation,
                cost,
                owner: Arc::clone(&self.identity),
            })
            .map_err(|error| format!("{error:?}"))
    }
    pub fn start(
        &mut self,
        admission: SearchAdmission,
        request: WorkspaceSearchRequest,
    ) -> Result<(), String> {
        if !self.is_idle() {
            return Err("a workspace search is still retiring".into());
        }
        if !Arc::ptr_eq(&self.identity, &admission.owner) {
            return Err("workspace search admission belongs to another frontend".into());
        }
        if captured_input_bytes(&request) > admission.cost.input_bytes {
            return Err("workspace search captures exceed admitted bytes".into());
        }
        if result_upper_bound(&request) > admission.cost.result_bytes {
            return Err("workspace search results exceed admitted bytes".into());
        }
        let revision = request.revision;
        let identity = Arc::clone(&request.identity);
        let receipt = admission
            .reservation
            .submit(SearchJob { request })
            .map_err(|error| format!("{:?}", error.reason))?;
        self.active = Some(Active {
            revision,
            identity,
            receipt,
        });
        Ok(())
    }
    /// Cancel a superseded query, but keep its charged receipt until completion.
    pub fn reconcile(&self, revision: Option<(&Arc<()>, u64)>) {
        if let Some(active) = &self.active {
            if !revision.is_some_and(|(identity, revision)| {
                Arc::ptr_eq(identity, &active.identity) && revision == active.revision
            }) {
                active.receipt.cancel();
            }
        }
    }
    pub fn collect(&mut self) -> Option<SearchWorkerEvent> {
        let active = self.active.as_mut()?;
        let revision = active.revision;
        let identity = Arc::clone(&active.identity);
        let event = match active.receipt.try_take() {
            JobPoll::Pending => return None,
            JobPoll::Ready(outcome) => {
                let (outcome, finite_hold) = outcome.into_parts();
                let event = match outcome {
                    JobOutcome::Finished(Ok(output)) => SearchWorkerEvent {
                        revision,
                        identity,
                        results: output.results,
                        fences: output.fences,
                        error: None,
                        retention: Some(Arc::new(output.storage)),
                    },
                    JobOutcome::Finished(Err(error)) => SearchWorkerEvent {
                        revision,
                        identity,
                        results: Vec::new(),
                        fences: Vec::new(),
                        error: Some(error),
                        retention: None,
                    },
                    JobOutcome::NotStarted { .. } => SearchWorkerEvent {
                        revision,
                        identity,
                        results: Vec::new(),
                        fences: Vec::new(),
                        error: Some("workspace search cancelled before execution".into()),
                        retention: None,
                    },
                    JobOutcome::Panicked => SearchWorkerEvent {
                        revision,
                        identity,
                        results: Vec::new(),
                        fences: Vec::new(),
                        error: Some("workspace search worker panicked".into()),
                        retention: None,
                    },
                };
                drop(finite_hold);
                event
            }
            JobPoll::Lost | JobPoll::Taken => SearchWorkerEvent {
                revision,
                identity,
                results: Vec::new(),
                fences: Vec::new(),
                error: Some("workspace search completion lost".into()),
                retention: None,
            },
        };
        self.active = None;
        Some(event)
    }
}
impl Drop for SearchWorkers {
    fn drop(&mut self) {
        self.reconcile(None);
    }
}

fn captured_input_bytes(request: &WorkspaceSearchRequest) -> usize {
    use search_ui::WorkspaceSearchContent;
    let mut bytes = 4096_usize
        .saturating_add(request.query.capacity().saturating_mul(2))
        .saturating_add(
            request
                .sources
                .capacity()
                .saturating_mul(std::mem::size_of::<search_ui::WorkspaceSearchSource>()),
        )
        .saturating_add(
            request
                .fences
                .capacity()
                .saturating_mul(std::mem::size_of::<SourceFence>()),
        );
    let mut scratch = 0;
    for source in &request.sources {
        bytes = bytes
            .saturating_add(source.object_name.capacity())
            .saturating_add(source.automatic_title.as_ref().map_or(0, String::capacity))
            .saturating_add(source.path.as_ref().map_or(0, |path| path.capacity()))
            .saturating_add(source.last_command.as_ref().map_or(0, String::capacity));
        match &source.content {
            WorkspaceSearchContent::Terminal(history) => {
                scratch = scratch.max(history.len().saturating_mul(8))
            }
            WorkspaceSearchContent::Text(entries) => {
                bytes = bytes.saturating_add(
                    entries
                        .capacity()
                        .saturating_mul(std::mem::size_of::<search_ui::WorkspaceSearchText>()),
                );
                for entry in entries {
                    bytes = bytes.saturating_add(entry.text.capacity());
                    scratch = scratch.max(entry.text.len());
                }
            }
        }
    }
    bytes.saturating_add(scratch)
}

fn result_upper_bound(request: &WorkspaceSearchRequest) -> usize {
    let metadata = request
        .sources
        .iter()
        .map(|source| {
            source
                .object_name
                .len()
                .saturating_add(source.automatic_title.as_ref().map_or(0, String::len))
                .saturating_add(
                    source
                        .path
                        .as_ref()
                        .map_or(0, |path| path.as_os_str().len()),
                )
                .saturating_add(source.last_command.as_ref().map_or(0, String::len))
        })
        .max()
        .unwrap_or(0);
    800_usize
        .saturating_mul(
            metadata
                .saturating_mul(2)
                .saturating_add(request.query.len().saturating_mul(4))
                .saturating_add(2048),
        )
        .saturating_add(
            request
                .fences
                .capacity()
                .saturating_mul(std::mem::size_of::<SourceFence>()),
        )
        .saturating_add(4096)
}

pub(crate) fn result_bytes(results: &Vec<SearchResult>) -> usize {
    results
        .iter()
        .fold(
            results
                .capacity()
                .saturating_mul(std::mem::size_of::<SearchResult>()),
            |bytes, result| {
                bytes
                    .saturating_add(result.object_name.capacity())
                    .saturating_add(result.automatic_title.as_ref().map_or(0, String::capacity))
                    .saturating_add(result.path.as_ref().map_or(0, |path| path.capacity()))
                    .saturating_add(result.last_command.as_ref().map_or(0, String::capacity))
                    .saturating_add(result.before.capacity())
                    .saturating_add(result.matched.capacity())
                    .saturating_add(result.after.capacity())
            },
        )
        .saturating_add(4096)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
        RejectReason, ShutdownMode, StartError,
    };
    use std::time::{Duration, Instant};

    struct Bank {
        execution: Execution,
        client: Client,
    }
    impl Bank {
        fn new() -> Self {
            let quota = QuotaGroup::new(QuotaLimits {
                clients: 4,
                jobs: 4,
                service_jobs: 0,
                input_bytes: 8 * 1024 * 1024,
                result_bytes: 8 * 1024 * 1024,
                worker_threads: 1,
                worker_bytes: 8 * 1024 * 1024,
            });
            let lane = |threads, queue_slots| LaneConfig {
                threads,
                queue_slots,
                priority: None,
                resident_bytes_per_thread: 1024,
            };
            let deadline = Instant::now() + Duration::from_secs(5);
            let execution = loop {
                match Execution::start(
                    quota.clone(),
                    ExecutionConfig {
                        cpu: lane(1, 2),
                        io: lane(0, 0),
                        service: lane(0, 0),
                    },
                ) {
                    Ok(execution) => break execution,
                    Err(StartError::Admission(RejectReason::Busy)) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(1))
                    }
                    Err(error) => panic!("search fixture startup: {error:?}"),
                }
            };
            let client = execution
                .client(ClientLimits {
                    jobs: 4,
                    service_jobs: 0,
                    input_bytes: 8 * 1024 * 1024,
                    result_bytes: 8 * 1024 * 1024,
                })
                .unwrap();
            Self { execution, client }
        }
    }
    impl Drop for Bank {
        fn drop(&mut self) {
            self.execution.request_shutdown(ShutdownMode::Cancel);
            self.execution
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap();
        }
    }
    fn cost() -> JobCost {
        JobCost {
            input_bytes: 1024 * 1024,
            result_bytes: 4 * 1024 * 1024,
        }
    }
    fn request() -> WorkspaceSearchRequest {
        WorkspaceSearchRequest {
            fences: Vec::new(),
            identity: Arc::new(()),
            revision: 1,
            query: "needle".into(),
            sources: vec![search_ui::WorkspaceSearchSource {
                pane_id: ilium_core::NodeId(1),
                kind: search_ui::SearchObjectKind::File,
                object_name: "file".into(),
                automatic_title: None,
                path: None,
                last_command: None,
                content: search_ui::WorkspaceSearchContent::Text(vec![
                    search_ui::WorkspaceSearchText {
                        text: "before needle after".into(),
                        location: search_ui::SearchLocation::Editor { line: 7 },
                    },
                ]),
            }],
        }
    }
    fn completion(workers: &mut SearchWorkers) -> SearchWorkerEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(event) = workers.collect() {
                return event;
            }
            assert!(Instant::now() < deadline, "search completion timeout");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn blocked_shared_cpu_does_not_block_producer_and_cancel_keeps_one_receipt() {
        let bank = Bank::new();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = bank
            .client
            .try_reserve(Lane::Cpu, cost())
            .unwrap()
            .submit(move |_context: JobContext| -> Result<(), String> {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            })
            .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut workers = SearchWorkers::new(bank.client.clone());
        let started = Instant::now();
        workers
            .start(workers.reserve(cost()).unwrap(), request())
            .unwrap();
        workers.reconcile(None);
        assert!(workers.collect().is_none());
        assert!(!workers.is_idle());
        assert!(workers.reserve(cost()).is_err());
        assert!(started.elapsed() < Duration::from_millis(100));
        release_tx.send(()).unwrap();
        let event = completion(&mut workers);
        assert!(event.error.is_some());
        assert!(event.results.is_empty());
        assert!(workers.is_idle());
        drop(blocker);
        assert_eq!(bank.client.usage().input_bytes, 0);
    }
    #[test]
    fn displayed_results_release_finite_input_credit_and_keep_measured_storage() {
        let bank = Bank::new();
        let mut workers = SearchWorkers::new(bank.client.clone());
        workers
            .start(workers.reserve(cost()).unwrap(), request())
            .unwrap();
        let event = completion(&mut workers);
        assert!(event.error.is_none(), "{:?}", event.error);
        assert_eq!(event.results.len(), 1);
        assert_eq!(
            event.results[0].location,
            search_ui::SearchLocation::Editor { line: 7 }
        );
        assert!(event.retention.is_some());
        assert_eq!(bank.client.usage().input_bytes, 0);
        assert_eq!(bank.client.usage().result_bytes, 0);
        let next = workers.reserve(cost()).unwrap();
        drop(next);
        assert_eq!(event.results[0].matched, "needle");
    }
    #[test]
    fn oversize_capture_is_rejected_before_any_job_is_published() {
        let bank = Bank::new();
        let workers = SearchWorkers::new(bank.client.clone());
        assert!(workers
            .reserve(JobCost {
                input_bytes: 9 * 1024 * 1024,
                result_bytes: 1024
            })
            .is_err());
        assert!(workers.is_idle());
        assert_eq!(bank.client.usage().input_bytes, 0);
    }
    #[test]
    fn another_frontend_cannot_submit_using_the_reserved_cpu_slot() {
        let bank = Bank::new();
        let first = SearchWorkers::new(bank.client.clone());
        let mut second = SearchWorkers::new(bank.client.clone());
        assert!(second
            .start(first.reserve(cost()).unwrap(), request())
            .is_err());
        assert!(second.is_idle());
        assert_eq!(bank.client.usage().input_bytes, 0);
    }
}
