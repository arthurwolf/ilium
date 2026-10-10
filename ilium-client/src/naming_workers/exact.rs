//! Finite transcript probes. Retry cadence belongs to this bounded owner,
//! never to a sleeping I/O worker or a native thread for each pane.
use super::*;
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, RejectReason, Rejected,
    StorageAdmission,
};
use std::collections::VecDeque;
const MAX_CONTEXTS: usize = 128;
const MAX_ACTIVE: usize = 4;
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const SOURCE_BYTES: usize = 2 * 1024 * 1024;
static LIVE_SOURCES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct SourceSlot;
impl SourceSlot {
    fn acquire() -> Result<Self, RejectReason> {
        LIVE_SOURCES
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_CONTEXTS).then_some(count + 1)
            })
            .map(|_| Self)
            .map_err(|_| RejectReason::QueueFull)
    }
}
impl Drop for SourceSlot {
    fn drop(&mut self) {
        LIVE_SOURCES.fetch_sub(1, Ordering::AcqRel);
    }
}
/// One source slot follows exact evidence until the final outbound owner admits it.
pub struct ExactSource {
    pub(crate) storage: Arc<StorageAdmission>,
    _slot: SourceSlot,
}
struct Original {
    request: ExactAgentPromptTranscriptRequest,
    generation: u64,
    // Cloned original contexts and their outputs share one physical lease.
    hold: Arc<ExactSource>,
}
struct Cursor {
    original: Arc<Original>,
    attempt: u32,
    due: Instant,
}
struct Probe {
    cursor: Cursor,
}
struct Probed {
    cursor: Cursor,
    result: Result<Option<String>, String>,
}
impl Job for Probe {
    type Output = Probed;
    type Error = std::convert::Infallible;
    fn run(self, context: JobContext) -> Result<Probed, Self::Error> {
        let request = &self.cursor.original.request;
        let result = (|| {
            if context.stop_requested() {
                return Ok(None);
            }
            // The pre-Enter path was already selected by the ownership service.
            // Reverify that exact path; broad discovery is unnecessary here.
            let locator = TranscriptLocator::new_bounded(
                &request.home,
                &request.project_path,
                ilium_agent_session::TranscriptReadLimits {
                    line_bytes: 1024 * 1024,
                    total_read_bytes: 16 * 1024 * 1024,
                    scanned_entries: 1024,
                    retained_path_bytes: 64 * 1024,
                },
            );
            let verified = locator
                .transcript_from_path(&request.agent_class, &request.verified_path)
                .is_some_and(|verified| verified.session_id == request.session_id);
            if locator.read_limit_reached() {
                return Err("Exact transcript identity exceeded bounded evidence limits".into());
            }
            if !verified || context.stop_requested() {
                return Ok(None);
            }
            crate::agent_prompt_transcript::exact_user_prompt_after(
                &request.agent_class,
                &request.verified_path,
                request.baseline_length,
                request.submitted_after,
            )
            .map_err(|error| error.to_string())
        })();
        Ok(Probed {
            cursor: self.cursor,
            result,
        })
    }
}
struct Active {
    original: Arc<Original>,
    receipt: Receipt<Probe>,
}
pub(super) struct ExactWorkers {
    client: Client,
    storage_quota: ilium_execution::QuotaGroup,
    contexts: HashMap<NodeId, Arc<Original>>,
    pending: VecDeque<Cursor>,
    active: Vec<Active>,
    ready: VecDeque<(NodeId, u64, finite::Prepared)>,
    generation: u64,
    closed: bool,
}
impl ExactWorkers {
    pub(super) fn new(client: Client) -> Self {
        Self {
            client,
            storage_quota: crate::execution::process_quota(),
            contexts: HashMap::new(),
            pending: VecDeque::new(),
            active: Vec::new(),
            ready: VecDeque::new(),
            generation: 0,
            closed: false,
        }
    }
    pub(super) fn request(
        &mut self,
        request: ExactAgentPromptTranscriptRequest,
    ) -> Result<(), Box<Rejected<ExactAgentPromptTranscriptRequest>>> {
        let reject = |reason, value| Box::new(Rejected { reason, value });
        if self.closed {
            return Err(reject(RejectReason::Closed, request));
        }
        let class_bytes = match &request.agent_class {
            AgentClass::Other(name) => name.capacity(),
            _ => 0,
        };
        let bytes = std::mem::size_of::<Original>()
            .saturating_add(request.home.capacity())
            .saturating_add(request.project_path.capacity())
            .saturating_add(request.verified_path.capacity())
            .saturating_add(request.session_id.capacity())
            .saturating_add(request.prompt_epoch.capacity())
            .saturating_add(class_bytes);
        if bytes > MAX_REQUEST_BYTES {
            return Err(reject(RejectReason::InvalidCost, request));
        }
        if !self.contexts.contains_key(&request.pane_id) && self.contexts.len() >= MAX_CONTEXTS {
            return Err(reject(RejectReason::QueueFull, request));
        }
        let slot = match SourceSlot::acquire() {
            Ok(slot) => slot,
            Err(reason) => return Err(reject(reason, request)),
        };
        let storage = match self.storage_quota.reserve_external_storage(SOURCE_BYTES) {
            Ok(storage) => Arc::new(storage),
            Err(reason) => return Err(reject(reason, request)),
        };
        let hold = Arc::new(ExactSource {
            storage,
            _slot: slot,
        });
        let Some(generation) = self.generation.checked_add(1) else {
            return Err(reject(RejectReason::Closed, request));
        };
        self.generation = generation;
        let pane = request.pane_id;
        // Admission succeeds before replacing the previous desired context.
        self.cancel(pane);
        let original = Arc::new(Original {
            request,
            generation,
            hold,
        });
        self.contexts.insert(pane, Arc::clone(&original));
        self.pending.push_back(Cursor {
            original,
            attempt: 0,
            due: Instant::now() + LAST_PROMPT_TRANSCRIPT_INITIAL_DELAY,
        });
        self.collect();
        Ok(())
    }
    fn is_current(&self, original: &Original) -> bool {
        self.contexts
            .get(&original.request.pane_id)
            .is_some_and(|current| current.generation == original.generation)
    }
    fn complete(&mut self, original: Arc<Original>, result: Result<Option<String>, String>) {
        if !self.is_current(&original) || self.closed {
            return;
        }
        let request = &original.request;
        let event = match result {
            Ok(last_prompt) => NamingWorkerEvent::ExactPrepared {
                result: ExactAgentPromptTranscriptResult {
                    pane_id: request.pane_id,
                    session_id: request.session_id.clone(),
                    prompt_epoch: request.prompt_epoch.clone(),
                    last_prompt,
                },
                source_hold: Arc::clone(&original.hold),
            },
            Err(error) => NamingWorkerEvent::ExactTranscriptFailed {
                pane_id: request.pane_id,
                session_id: request.session_id.clone(),
                prompt_epoch: request.prompt_epoch.clone(),
                error,
            },
        };
        self.ready.push_back((
            request.pane_id,
            original.generation,
            finite::Prepared {
                event,
                source_hold: Arc::clone(&original.hold.storage),
            },
        ));
    }
    pub(super) fn collect(&mut self) {
        let mut index = 0;
        while index < self.active.len() {
            match self.active[index].receipt.try_take() {
                JobPoll::Pending => index += 1,
                JobPoll::Ready(outcome) => {
                    let active = self.active.remove(index);
                    let (outcome, hold) = outcome.into_parts();
                    match outcome {
                        JobOutcome::Finished(Ok(mut probed))
                            if self.is_current(&probed.cursor.original) && !self.closed =>
                        {
                            if matches!(&probed.result, Ok(None))
                                && probed.cursor.attempt + 1 < LAST_PROMPT_TRANSCRIPT_MAX_ATTEMPTS
                            {
                                probed.cursor.attempt += 1;
                                probed.cursor.due =
                                    Instant::now() + LAST_PROMPT_TRANSCRIPT_RETRY_INTERVAL;
                                self.pending.push_back(probed.cursor);
                            } else {
                                self.complete(probed.cursor.original, probed.result);
                            }
                        }
                        JobOutcome::Finished(Err(never)) => match never {},
                        JobOutcome::NotStarted { job, .. }
                            if self.is_current(&job.cursor.original) && !self.closed =>
                        {
                            self.pending.push_front(job.cursor)
                        }
                        JobOutcome::Panicked => self.complete(
                            active.original,
                            Err("Exact transcript probe panicked".into()),
                        ),
                        _ => {}
                    }
                    drop(hold);
                }
                JobPoll::Lost | JobPoll::Taken => {
                    let active = self.active.remove(index);
                    self.complete(
                        active.original,
                        Err("Exact transcript probe receipt lost".into()),
                    );
                }
            }
        }
        let now = Instant::now();
        let count = self.pending.len();
        for _ in 0..count {
            if self.closed || self.active.len() >= MAX_ACTIVE {
                break;
            }
            let Some(cursor) = self.pending.pop_front() else {
                break;
            };
            if !self.is_current(&cursor.original) {
                continue;
            }
            if cursor.due > now {
                self.pending.push_back(cursor);
                continue;
            }
            let original = Arc::clone(&cursor.original);
            match self.client.try_submit(
                Lane::Io,
                JobCost {
                    input_bytes: 64 * 1024 * 1024,
                    result_bytes: SOURCE_BYTES,
                },
                Probe { cursor },
            ) {
                Ok(receipt) => self.active.push(Active { original, receipt }),
                Err(rejected) => {
                    if matches!(
                        rejected.reason,
                        RejectReason::Busy
                            | RejectReason::QueueFull
                            | RejectReason::JobLimit
                            | RejectReason::InputBytes
                            | RejectReason::ResultBytes
                    ) {
                        self.pending.push_front(rejected.value.cursor);
                        break;
                    }
                    self.complete(
                        original,
                        Err(format!(
                            "Exact transcript probe did not start: {:?}",
                            rejected.reason
                        )),
                    );
                }
            }
        }
    }
    pub(super) fn publish(&mut self, sender: &Sender<NamingWorkerEvent>) {
        while let Some((pane, generation, prepared)) = self.ready.pop_front() {
            if self
                .contexts
                .get(&pane)
                .is_none_or(|current| current.generation != generation)
            {
                continue;
            }
            let event = NamingWorkerEvent::Prepared {
                event: Box::new(prepared.event),
                source_hold: prepared.source_hold,
            };
            match sender.try_send(event) {
                Ok(()) => {
                    self.contexts.remove(&pane);
                }
                Err(tokio::sync::mpsc::error::TrySendError::Full(
                    NamingWorkerEvent::Prepared { event, source_hold },
                )) => {
                    self.ready.push_front((
                        pane,
                        generation,
                        finite::Prepared {
                            event: *event,
                            source_hold,
                        },
                    ));
                    break;
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    self.close();
                    break;
                }
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    unreachable!("only Prepared is published")
                }
            }
        }
    }
    pub(super) fn cancel(&mut self, pane: NodeId) {
        self.contexts.remove(&pane);
        self.pending
            .retain(|cursor| cursor.original.request.pane_id != pane);
        self.ready.retain(|(id, _, _)| *id != pane);
        for active in &self.active {
            if active.original.request.pane_id == pane {
                active.receipt.cancel();
            }
        }
    }
    pub(super) fn cancel_stale(&mut self, mut keep: impl FnMut(NodeId, &str) -> bool) {
        let stale: Vec<NodeId> = self
            .contexts
            .iter()
            .filter_map(|(pane, original)| {
                (!keep(*pane, &original.request.session_id)).then_some(*pane)
            })
            .collect();
        for pane in stale {
            self.cancel(pane);
        }
    }
    #[cfg(test)]
    pub(super) fn context_count_for_test(&self) -> usize {
        self.contexts.len()
    }
    #[cfg(test)]
    pub(super) fn active_count_for_test(&self) -> usize {
        self.active.len()
    }
    #[cfg(test)]
    pub(super) fn ready_count_for_test(&self) -> usize {
        self.ready.len()
    }
    pub(super) fn delivery_pending(&self) -> bool {
        !self.ready.is_empty()
    }
    pub(super) fn close(&mut self) {
        if self.closed {
            return;
        }
        // Observe completed receipts before defining the cancellation boundary.
        // Completed evidence remains deliverable; pending computation is cancelled.
        self.collect();
        self.closed = true;
        self.pending.clear();
        let ready = &self.ready;
        self.contexts.retain(|pane, current| {
            ready
                .iter()
                .any(|(id, generation, _)| id == pane && *generation == current.generation)
        });
        for active in &self.active {
            active.receipt.cancel();
        }
    }
}
impl Drop for ExactWorkers {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    #[test]
    fn thirty_two_panes_use_one_real_io_thread_and_release_all_receipts_on_shutdown() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 4,
            service_jobs: 0,
            input_bytes: 128 * 1024 * 1024,
            result_bytes: 8 * 1024 * 1024,
            worker_threads: 1,
            worker_bytes: 128 * 1024 * 1024,
        });
        let bank = |threads| LaneConfig {
            threads,
            queue_slots: if threads == 0 { 0 } else { 4 },
            priority: None,
            resident_bytes_per_thread: if threads == 0 { 0 } else { 1024 * 1024 },
        };
        let mut owner = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: bank(0),
                io: bank(1),
                service: bank(0),
            },
        )
        .unwrap();
        let client = owner
            .client(ClientLimits {
                jobs: 4,
                service_jobs: 0,
                input_bytes: 128 * 1024 * 1024,
                result_bytes: 8 * 1024 * 1024,
            })
            .unwrap();
        let mut workers = ExactWorkers::new(client);
        workers.storage_quota = quota.clone();
        let home = tempfile::tempdir().unwrap();
        for id in 0..32 {
            let request = super::super::tests::exact_prompt_request_for_test(
                home.path(),
                NodeId(2000 + id),
                "retained-original-epoch",
            );
            workers.request(request).unwrap();
        }
        assert_eq!(workers.contexts.len(), 32);
        assert_eq!(
            quota.snapshot().worker_threads,
            1,
            "no native owner per pane"
        );
        assert!(quota.snapshot().worker_bytes >= 32 * SOURCE_BYTES);
        for cursor in &mut workers.pending {
            cursor.due = Instant::now();
        }
        workers.collect();
        assert!(workers.active.len() <= MAX_ACTIVE);
        workers.close();
        owner.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            owner
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
        drop(workers);
        drop(owner);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().jobs, 0);
        assert_eq!(quota.snapshot().result_bytes, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
