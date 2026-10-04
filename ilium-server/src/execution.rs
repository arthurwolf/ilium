//! Process-owned finite CPU/I/O execution. Start before coordination loops;
//! receipt waiting uses completion notifications, never timed polling.
use ilium_execution::{
    Client, ClientLimits, Execution, ExecutionConfig, Job, JobCost, JobOutcome, JobPoll, Lane,
    LaneConfig, QuotaGroup, QuotaLimits, Receipt, Retained, ShutdownMode,
};
use std::{io, sync::Arc};
use tokio::sync::Notify;

const MIB: usize = 1024 * 1024;
// Distinct tenants reserve output headroom even while request handlers wait
// for backpressured PTY/direct replies. These are declared allocation limits,
// not an RSS estimate. They are shared across connections, never per client.
const GENERAL_INPUT_BYTES: usize = 512 * MIB;
const GENERAL_RESULT_BYTES: usize = 128 * MIB;
const PROCESS_INPUT_BYTES: usize = GENERAL_INPUT_BYTES + 4 * 128 * MIB + 2 * 128 * MIB;
const PROCESS_RESULT_BYTES: usize = GENERAL_RESULT_BYTES + 4 * 192 * MIB + 2 * 192 * MIB;

/// The existing session root and its admission/completion notification, created
/// before process logging and moved into the later execution bank. Construction
/// alone starts no worker and never designates an independent fixture as the
/// permanent owner of the process supervisor.
pub struct ServerResources {
    quota: QuotaGroup,
    completed: Arc<Notify>,
}

impl ServerResources {
    pub fn new() -> Self {
        let completed = Arc::new(Notify::new());
        let admission_wake = Arc::clone(&completed);
        let quota = QuotaGroup::new_with_admission_wake(
            QuotaLimits {
                clients: 32,
                jobs: 80,
                service_jobs: 0,
                input_bytes: PROCESS_INPUT_BYTES,
                result_bytes: PROCESS_RESULT_BYTES,
                worker_threads: 16,
                worker_bytes: 512 * MIB,
            },
            move || admission_wake.notify_waiters(),
        );
        Self { quota, completed }
    }

    pub fn quota_group(&self) -> QuotaGroup {
        self.quota.clone()
    }

    /// Process bootstrap only, before logger or bank creation. Reusing this
    /// same root is idempotent; a separately created root cannot adopt it.
    pub fn initialize_process(&self) -> io::Result<bool> {
        ilium_execution::initialize_process_supervisor(&self.quota).map_err(io::Error::other)
    }

    /// Binary bootstrap only, before constructing the existing Tokio runtime.
    pub fn initialize_runtime(
        &self,
        thread_capacity: usize,
        stack_bytes: usize,
    ) -> io::Result<bool> {
        ilium_execution::initialize_process_runtime_admission(
            &self.quota,
            thread_capacity,
            stack_bytes,
        )
        .map_err(io::Error::other)
    }
}

impl Default for ServerResources {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct ServerExecution {
    // This owner is retained until the State drops. Its destructor signals
    // cancellation without synchronously joining blocked native callbacks.
    _owner: Execution,
    pub(crate) client: ExecutionClient,
    pub(crate) decoder: ExecutionClient,
    pub(crate) encoder: ExecutionClient,
}
#[derive(Clone)]
pub(crate) struct ExecutionClient {
    pub(crate) foundation: Client,
    quota: QuotaGroup,
    completed: Arc<Notify>,
}
pub(crate) enum ExecutionError<E> {
    Rejected(ilium_execution::RejectReason),
    Failed(Retained<E>),
    Panicked,
    Cancelled,
    Lost,
}
impl<E: std::fmt::Debug> std::fmt::Debug for ExecutionError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(reason) => formatter.debug_tuple("Rejected").field(reason).finish(),
            Self::Failed(error) => formatter.debug_tuple("Failed").field(error.view()).finish(),
            Self::Panicked => formatter.write_str("Panicked"),
            Self::Cancelled => formatter.write_str("Cancelled"),
            Self::Lost => formatter.write_str("Lost"),
        }
    }
}
impl<E: std::fmt::Display> std::fmt::Display for ExecutionError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(reason) => write!(formatter, "execution admission rejected: {reason:?}"),
            Self::Failed(error) => write!(formatter, "execution callback failed: {}", error.view()),
            Self::Panicked => formatter.write_str("execution callback panicked"),
            Self::Cancelled => formatter.write_str("execution callback cancelled before starting"),
            Self::Lost => formatter.write_str("execution callback completion was lost"),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for ExecutionError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Failed(error) => Some(error.view()),
            _ => None,
        }
    }
}
impl ServerExecution {
    pub(crate) fn quota_group(&self) -> QuotaGroup {
        self.client.quota.clone()
    }

    #[cfg(test)]
    pub(crate) fn start() -> io::Result<Self> {
        Self::start_with_resources(ServerResources::new())
    }

    pub(crate) fn start_with_resources(resources: ServerResources) -> io::Result<Self> {
        let ServerResources { quota, completed } = resources;
        let lane = |threads, resident_bytes_per_thread| LaneConfig {
            threads,
            queue_slots: if threads == 0 { 0 } else { 80 },
            priority: Some(ilium_execution::WorkerPriority::BelowNormal),
            resident_bytes_per_thread,
        };
        let owner = Execution::start(
            quota.clone(),
            ExecutionConfig {
                // Each CPU owner retains a measured regex LRU of at most
                // 16 MiB; the remaining allowance covers cache metadata and
                // thread-local working state through actual thread teardown.
                cpu: lane(2, 32 * MIB),
                io: lane(2, 0),
                service: lane(0, 0),
            },
        )
        .map_err(|error| io::Error::other(format!("server execution bootstrap: {error:?}")))?;
        let wake = Arc::clone(&completed);
        // A bounded waiter set is supplied by foundation's 64-job quota.
        // Notify stores no payload and waking cannot wait for a consumer.
        let foundation = owner
            .client(ClientLimits {
                jobs: 60,
                service_jobs: 0,
                input_bytes: GENERAL_INPUT_BYTES,
                result_bytes: GENERAL_RESULT_BYTES,
            })
            .map_err(|error| io::Error::other(format!("server execution client: {error:?}")))?
            .with_completion_wake(move || wake.notify_waiters());
        let codec = |jobs| -> io::Result<ExecutionClient> {
            let wake = Arc::clone(&completed);
            let foundation = owner
                .client(ClientLimits {
                    jobs,
                    service_jobs: 0,
                    input_bytes: jobs * 128 * MIB,
                    result_bytes: jobs * 192 * MIB,
                })
                .map_err(|error| io::Error::other(format!("codec tenant: {error:?}")))?
                .with_completion_wake(move || wake.notify_waiters());
            Ok(ExecutionClient {
                foundation,
                quota: quota.clone(),
                completed: Arc::clone(&completed),
            })
        };
        let decoder = codec(4)?;
        let encoder = codec(2)?;
        Ok(Self {
            _owner: owner,
            client: ExecutionClient {
                foundation,
                quota,
                completed,
            },
            decoder,
            encoder,
        })
    }
    pub(crate) fn request_shutdown(&self) {
        self._owner.request_shutdown(ShutdownMode::Cancel);
        self.client.completed.notify_waiters();
    }
}
struct CancellableReceipt<J: Job>(Receipt<J>);
impl<J: Job> Drop for CancellableReceipt<J> {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
impl ExecutionClient {
    pub(crate) fn completion_notification(&self) -> Arc<Notify> {
        Arc::clone(&self.completed)
    }

    pub(crate) fn try_reserve_storage(
        &self,
        bytes: usize,
    ) -> Result<Arc<ilium_execution::StorageAdmission>, ilium_execution::RejectReason> {
        if bytes == 0 || bytes > self.quota.snapshot().limits.worker_bytes {
            return Err(ilium_execution::RejectReason::InvalidCost);
        }
        if !self.foundation.is_open() {
            return Err(ilium_execution::RejectReason::Closed);
        }
        self.quota.reserve_external_storage(bytes).map(Arc::new)
    }

    pub(crate) async fn reserve_storage(
        &self,
        bytes: usize,
    ) -> Result<Arc<ilium_execution::StorageAdmission>, ilium_execution::RejectReason> {
        if bytes == 0 || bytes > self.quota.snapshot().limits.worker_bytes {
            return Err(ilium_execution::RejectReason::InvalidCost);
        }
        loop {
            let notified = self.completed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.try_reserve_storage(bytes) {
                Ok(reservation) => return Ok(reservation),
                Err(ilium_execution::RejectReason::Busy) => {
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
                Err(ilium_execution::RejectReason::WorkerBytes) => notified.await,
                Err(error) => return Err(error),
            }
        }
    }
    /// Wait for capacity outside the coordinating loops. A closed bank is an
    /// explicit error; only temporary admission pressure waits for release.
    pub(crate) async fn reserve(
        &self,
        lane: Lane,
        cost: JobCost,
    ) -> Result<ilium_execution::Reservation, ilium_execution::RejectReason> {
        loop {
            let notified = self.completed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.foundation.try_reserve(lane, cost) {
                Ok(reservation) => return Ok(reservation),
                Err(ilium_execution::RejectReason::Busy) => {
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await
                }
                Err(
                    ilium_execution::RejectReason::QueueFull
                    | ilium_execution::RejectReason::JobLimit
                    | ilium_execution::RejectReason::InputBytes
                    | ilium_execution::RejectReason::ResultBytes,
                ) => notified.await,
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) async fn run<J: Job>(
        &self,
        lane: Lane,
        cost: JobCost,
        job: J,
    ) -> Result<Retained<J::Output>, ExecutionError<J::Error>> {
        let reservation = self
            .foundation
            .try_reserve(lane, cost)
            .map_err(ExecutionError::Rejected)?;
        self.run_reserved(reservation, job).await
    }
    pub(crate) async fn run_reserved<J: Job>(
        &self,
        reservation: ilium_execution::Reservation,
        job: J,
    ) -> Result<Retained<J::Output>, ExecutionError<J::Error>> {
        let receipt = reservation
            .submit(job)
            .map_err(|rejected| ExecutionError::Rejected(rejected.reason))?;
        let mut receipt = CancellableReceipt(receipt);
        loop {
            let notified = self.completed.notified();
            tokio::pin!(notified);
            // Register before checking the receipt, closing the wake-before-
            // await race for notify_waiters without any polling timer.
            notified.as_mut().enable();
            match receipt.0.try_take() {
                JobPoll::Pending => notified.await,
                JobPoll::Ready(outcome) => {
                    match outcome.view() {
                        JobOutcome::Finished(Ok(_)) => {
                            return Ok(outcome.map(|outcome| match outcome {
                                JobOutcome::Finished(Ok(output)) => output,
                                // Invariant: view() above proved this exact variant;
                                // Retained owns it exclusively, with no mutation API.
                                _ => unreachable!("validated execution outcome changed"),
                            }));
                        }
                        JobOutcome::Finished(Err(_)) => {
                            return Err(ExecutionError::Failed(outcome.map(
                                |outcome| match outcome {
                                    JobOutcome::Finished(Err(error)) => error,
                                    // Invariant: exclusive view above proved this variant.
                                    _ => unreachable!("validated execution error changed"),
                                },
                            )));
                        }
                        JobOutcome::NotStarted { .. } => return Err(ExecutionError::Cancelled),
                        JobOutcome::Panicked => return Err(ExecutionError::Panicked),
                    }
                }
                JobPoll::Lost | JobPoll::Taken => return Err(ExecutionError::Lost),
            }
        }
    }
}

#[cfg(test)]
fn test_owner() -> &'static ServerExecution {
    static OWNER: std::sync::OnceLock<ServerExecution> = std::sync::OnceLock::new();
    OWNER.get_or_init(|| ServerExecution::start().expect("shared real server fixture bank"))
}

#[cfg(test)]
pub(crate) fn test_general_client() -> ExecutionClient {
    test_owner().client.clone()
}

#[cfg(test)]
pub(crate) fn test_codec_client(is_decoder: bool) -> ExecutionClient {
    let owner = test_owner();
    if is_decoder {
        owner.decoder.clone()
    } else {
        owner.encoder.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn storage_waiters_wake_on_release_and_shutdown_without_blocking_cpu() {
        let owner = ServerExecution::start().expect("execution");
        // Fill the remaining bank storage, including its metadata debit.
        let snapshot = owner.client.quota.snapshot();
        let storage_bytes = snapshot
            .limits
            .worker_bytes
            .checked_sub(snapshot.worker_bytes)
            .expect("execution residents must fit the bank");
        assert!(storage_bytes >= MIB);
        let held = owner
            .client
            .reserve_storage(storage_bytes)
            .await
            .expect("storage");
        assert!(matches!(
            owner.client.try_reserve_storage(MIB),
            Err(ilium_execution::RejectReason::WorkerBytes)
        ));
        let client = owner.client.clone();
        let mut waiting = tokio::spawn(async move { client.reserve_storage(MIB).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut waiting)
                .await
                .is_err()
        );
        let quick = owner
            .client
            .run(
                Lane::Cpu,
                JobCost {
                    input_bytes: 64,
                    result_bytes: 64,
                },
                |_| Ok::<_, io::Error>(42),
            )
            .await
            .expect("storage pressure must not block finite CPU work");
        assert_eq!(*quick.view(), 42);
        drop(held);
        let acquired = tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .expect("admission release notification")
            .expect("waiter")
            .expect("storage");
        drop(acquired);
        let held = owner
            .client
            .reserve_storage(storage_bytes)
            .await
            .expect("storage");
        let client = owner.client.clone();
        let mut waiting = tokio::spawn(async move { client.reserve_storage(MIB).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut waiting)
                .await
                .is_err()
        );
        owner.request_shutdown();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), waiting)
                .await
                .expect("shutdown notification")
                .expect("waiter"),
            Err(ilium_execution::RejectReason::Closed)
        ));
        drop(held);
    }

    #[test]
    fn blocked_handlers_and_general_jobs_keep_two_output_admissions_available() {
        let owner = ServerExecution::start().expect("execution");
        let general = owner
            .client
            .foundation
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes: GENERAL_INPUT_BYTES,
                    result_bytes: GENERAL_RESULT_BYTES,
                },
            )
            .expect("general envelope");
        let cost = JobCost {
            input_bytes: 128 * MIB,
            result_bytes: 192 * MIB,
        };
        let readers: Vec<_> = (0..4)
            .map(|_| {
                owner
                    .decoder
                    .foundation
                    .try_reserve(Lane::Cpu, cost)
                    .expect("decoded request envelope")
            })
            .collect();
        assert!(matches!(
            owner.decoder.foundation.try_reserve(Lane::Cpu, cost),
            Err(ilium_execution::RejectReason::JobLimit)
        ));
        let writers: Vec<_> = (0..2)
            .map(|_| {
                owner
                    .encoder
                    .foundation
                    .try_reserve(Lane::Cpu, cost)
                    .expect("output headroom must survive waiting handlers")
            })
            .collect();
        assert!(matches!(
            owner.encoder.foundation.try_reserve(Lane::Cpu, cost),
            // The process envelope is now exactly full; root byte checks
            // precede this tenant's third-job limit.
            Err(ilium_execution::RejectReason::InputBytes)
        ));
        drop((general, readers, writers));
    }

    #[tokio::test]
    async fn blocked_io_leaves_coordination_and_cpu_jobs_responsive() {
        let owner = ServerExecution::start().expect("execution");
        let client = owner.client.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let caller = std::thread::current().id();
        let blocked = tokio::spawn(async move {
            client
                .run(
                    Lane::Io,
                    JobCost {
                        input_bytes: 1024,
                        result_bytes: 128,
                    },
                    move |_| -> std::io::Result<()> {
                        let _ = started_tx.send(std::thread::current().id());
                        release_rx.recv().expect("release");
                        Ok(())
                    },
                )
                .await
        });
        assert_ne!(started_rx.await.expect("started"), caller);
        let quick = tokio::time::timeout(
            Duration::from_secs(2),
            owner.client.run(
                Lane::Cpu,
                JobCost {
                    input_bytes: 64,
                    result_bytes: 64,
                },
                |_| Ok::<_, io::Error>(42),
            ),
        )
        .await
        .expect("coordination timer remains responsive")
        .expect("CPU result");
        assert_eq!(*quick.view(), 42);
        release_tx.send(()).expect("release");
        drop(blocked.await.expect("waiter").expect("IO result"));
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn cancelled_waiter_signals_active_worker_and_panics_report_failure() {
        let owner = ServerExecution::start().expect("execution");
        let client = owner.client.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let waiter = tokio::spawn(async move {
            client
                .run(
                    Lane::Io,
                    JobCost {
                        input_bytes: 1024,
                        result_bytes: 128,
                    },
                    move |context: ilium_execution::JobContext| -> std::io::Result<()> {
                        let _ = started_tx.send(());
                        release_rx.recv().expect("release");
                        let _ = finished_tx.send(context.stop_requested());
                        Ok(())
                    },
                )
                .await
        });
        started_rx.await.expect("started");
        waiter.abort();
        assert!(matches!(waiter.await, Err(error) if error.is_cancelled()));
        release_tx.send(()).expect("release");
        assert!(finished_rx.await.expect("worker stopped"));
        let failure = owner
            .client
            .run(
                Lane::Cpu,
                JobCost {
                    input_bytes: 64,
                    result_bytes: 64,
                },
                |_| -> std::io::Result<()> {
                    panic!("forcing worker panic");
                },
            )
            .await;
        assert!(matches!(failure, Err(ExecutionError::Panicked)));
        let result = owner
            .client
            .run(
                Lane::Cpu,
                JobCost {
                    input_bytes: 64,
                    result_bytes: 64,
                },
                |_| Ok::<_, io::Error>(7),
            )
            .await
            .expect("bank survives panic");
        assert_eq!(*result.view(), 7);
        owner.request_shutdown();
    }
}
