//! Explicit host-supplied execution and allocation resources for scene adapters.
//!
//! This owns no execution bank. Clones share the host's admitted finite client
//! and physical quota; they do not multiply limits. Native/library peak costs
//! remain adapter declarations, never an allocator or RSS guarantee.
use ilium_execution::{Client, QuotaGroup, RejectReason, StorageAdmission, WorkerAdmission};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
#[error("Ambient resources are not configured")]
pub struct MissingResources;

#[derive(Clone)]
pub struct AmbientResources {
    finite: Client,
    quota: QuotaGroup,
}
impl std::fmt::Debug for AmbientResources {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AmbientResources")
            .finish_non_exhaustive()
    }
}
impl AmbientResources {
    /// The composition root supplies an existing admitted client.
    /// No threads, fallback bank or independent quota are created here.
    pub fn new(finite: Client) -> Self {
        let quota = finite.quota_group();
        Self { finite, quota }
    }
    /// The existing client admits either CPU or IO jobs by their explicit lane.
    pub fn finite(&self) -> &Client {
        &self.finite
    }
    /// Admit BEFORE capturing/cloning worker inputs or spawning native helpers.
    /// Include every owned native/library thread and peak resident allocation.
    pub fn reserve_worker(&self, cost: WorkerCost) -> Result<WorkerReservation, RejectReason> {
        if cost.threads == 0 || cost.resident_bytes == 0 {
            return Err(RejectReason::InvalidCost);
        }
        self.quota
            .reserve_external_worker(cost.threads, cost.resident_bytes)
            .map(|physical| WorkerReservation { physical })
    }
    /// Reserve before allocating independently retained immutable storage.
    /// A cloned guard covers only the same allocation, never a heap copy.
    pub fn reserve_storage(&self, bytes: usize) -> Result<Arc<StorageAdmission>, RejectReason> {
        self.quota.reserve_external_storage(bytes).map(Arc::new)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerCost {
    pub threads: usize,
    pub resident_bytes: usize,
}

/// Transfer to the actual supervised join owner, not the callback body.
#[must_use]
pub struct WorkerReservation {
    pub(crate) physical: WorkerAdmission,
}
impl std::fmt::Debug for WorkerReservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkerReservation")
            .finish_non_exhaustive()
    }
}

/// Immutable storage charge follows the original value through publication.
/// Callers must reserve its complete capacity before creating that allocation.
#[derive(Debug)]
#[must_use]
pub struct Stored<T> {
    value: T,
    // Last: payload destruction precedes release of its allocation credit.
    storage: Arc<StorageAdmission>,
}
impl<T> Stored<T> {
    pub fn new(value: T, storage: Arc<StorageAdmission>) -> Self {
        Self { value, storage }
    }
    pub fn view(&self) -> &T {
        &self.value
    }
    pub fn into_parts(self) -> (T, Arc<StorageAdmission>) {
        (self.value, self.storage)
    }
}

/// One explicit isolated composition for ambient unit fixtures. Pure scene
/// environments share it rather than starting an execution per constructor.
#[cfg(test)]
fn create_test_resources() -> (ilium_execution::Execution, AmbientResources) {
    use ilium_execution::{ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaLimits};
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 8,
        jobs: 16,
        service_jobs: 0,
        input_bytes: 512 * 1024 * 1024,
        result_bytes: 512 * 1024 * 1024,
        worker_threads: 16,
        worker_bytes: 2304 * 1024 * 1024,
    });
    let lane = LaneConfig {
        threads: 2,
        queue_slots: 8,
        priority: None,
        resident_bytes_per_thread: 1024 * 1024,
    };
    let execution = Execution::start(
        quota,
        ExecutionConfig {
            cpu: lane,
            io: lane,
            service: LaneConfig {
                threads: 0,
                queue_slots: 0,
                priority: None,
                resident_bytes_per_thread: 0,
            },
        },
    )
    .unwrap();
    let client = execution
        .client(ClientLimits {
            jobs: 16,
            service_jobs: 0,
            input_bytes: 512 * 1024 * 1024,
            result_bytes: 512 * 1024 * 1024,
        })
        .unwrap();
    (execution, AmbientResources::new(client))
}

/// A process-lifecycle test gets its own quota so unrelated parallel fixtures
/// cannot prevent the decoder worker from reaching the timeout under test.
#[cfg(test)]
pub(crate) fn isolated_test_resources() -> (ilium_execution::Execution, AmbientResources) {
    create_test_resources()
}

#[cfg(test)]
pub(crate) fn test_resources() -> AmbientResources {
    static FIXTURE: std::sync::OnceLock<(ilium_execution::Execution, AmbientResources)> =
        std::sync::OnceLock::new();
    FIXTURE.get_or_init(create_test_resources).1.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Worker;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaLimits, ShutdownMode,
    };
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    fn isolated() -> (Execution, AmbientResources, QuotaGroup) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 4,
            jobs: 4,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
            worker_threads: 2,
            worker_bytes: 64 * 1024,
        });
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 2,
                    priority: None,
                    resident_bytes_per_thread: 1024,
                },
                io: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
                service: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
            },
        )
        .unwrap();
        let finite = execution
            .client(ClientLimits {
                jobs: 4,
                service_jobs: 0,
                input_bytes: 4096,
                result_bytes: 4096,
            })
            .unwrap();
        (execution, AmbientResources::new(finite), quota)
    }

    #[test]
    fn blocked_actual_callback_keeps_physical_credit_through_last_join_ticket() {
        let (mut execution, resources, quota) = isolated();
        let baseline = quota.snapshot().worker_bytes;
        let admission = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: 1024,
            })
            .unwrap();
        let (started, start) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let worker = Worker::start_admitted("charged-blocked", admission, move |_| {
            started.send(()).unwrap();
            gate.recv().unwrap();
        })
        .unwrap();
        let ticket = worker.join_observer().unwrap();
        start.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(worker);
        assert_eq!(quota.snapshot().worker_threads, 2);
        assert!(matches!(
            resources.reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: 1024
            }),
            Err(RejectReason::WorkerLimit)
        ));
        assert!(ticket.exit().is_none());
        release.send(()).unwrap();
        ticket
            .join_until(Instant::now() + Duration::from_secs(5))
            .unwrap();
        // The actual join is complete; the explicitly retained ticket still
        // owns the supervisor wake state and its original physical claim.
        assert_eq!(quota.snapshot().worker_threads, 2);
        drop(ticket);
        assert_eq!(quota.snapshot().worker_threads, 1);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
        execution.request_shutdown(ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }

    #[test]
    fn immutable_original_storage_survives_worker_exit_and_rejects_before_growth() {
        let (mut execution, resources, quota) = isolated();
        let baseline = quota.snapshot().worker_bytes;
        let storage = resources.reserve_storage(64 * 1024 - baseline).unwrap();
        assert!(matches!(
            resources.reserve_storage(1),
            Err(RejectReason::WorkerBytes)
        ));
        let original = vec![42u8; 32];
        let pointer = original.as_ptr();
        let retained = Arc::new(Stored::new(original, storage));
        let queued = Arc::clone(&retained);
        drop(retained);
        execution.request_shutdown(ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(queued.view().as_ptr(), pointer);
        assert!(quota.snapshot().worker_bytes >= 64 * 1024 - baseline);
        drop(queued);
        assert_eq!(quota.snapshot().worker_bytes, baseline - 1024);
        drop(resources);
        drop(execution);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
