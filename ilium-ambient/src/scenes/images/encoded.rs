//! Concrete local-file preparation on the host's existing finite IO bank.
//! This stage does not admit the legacy image decoder or HTTP/TLS allocation.
use crate::resources::{AmbientResources, Stored};
use crate::source::{read_bounded_file_scalar, FileReadFailure};
use ilium_execution::{Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, RejectReason};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const RETRY_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub(super) enum ReadError {
    File(FileReadFailure),
    Admission(RejectReason),
    Retired,
}
impl std::fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(FileReadFailure::Io { kind, raw_os_error }) => {
                let error = raw_os_error.map_or_else(
                    || std::io::Error::from(*kind),
                    std::io::Error::from_raw_os_error,
                );
                write!(formatter, "request failed: {error}")
            }
            Self::File(FileReadFailure::Cancelled) => formatter.write_str("request cancelled"),
            Self::File(FileReadFailure::Timeout) => formatter.write_str("request timeout"),
            Self::File(FileReadFailure::TooLarge(limit)) => {
                write!(formatter, "response larger than {limit} bytes")
            }
            Self::File(FileReadFailure::Allocation) => {
                formatter.write_str("request failed: response allocation failed")
            }
            Self::Admission(reason) => {
                write!(formatter, "file preparation admission refused: {reason:?}")
            }
            Self::Retired => formatter.write_str("file preparation owner retired"),
        }
    }
}
struct ReadJob {
    path: PathBuf,
    limit: usize,
    stop: Arc<AtomicBool>,
    // Same stop/configuration allocation, retained through actual callback exit.
    _capture_storage: Arc<ilium_execution::StorageAdmission>,
    // Last: independently admitted path copy survives even rejected submission.
    _path_storage: Arc<ilium_execution::StorageAdmission>,
}
impl Job for ReadJob {
    type Output = Vec<u8>;
    type Error = FileReadFailure;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        read_bounded_file_scalar(&self.path, self.limit, || {
            context.stop_requested() || self.stop.load(Ordering::Acquire)
        })
    }
}
fn read_cost(limit: usize) -> Result<JobCost, RejectReason> {
    // source's concrete collector clamps explicit requested capacity at limit
    // BEFORE try_reserve_exact. Old and replacement buffers each fit limit,
    // including non-power-of-two short reads; no amortized rounding assumption.
    // This declares requested layouts, not excess allocator capacity or RSS.
    let capacity = limit;
    let input_bytes = capacity
        .checked_add(capacity)
        .and_then(|bytes| bytes.checked_add(8192))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<ReadJob>()))
        .ok_or(RejectReason::InvalidCost)?;
    let result_bytes = capacity
        .checked_add(std::mem::size_of::<JobOutcome<ReadJob>>())
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<ilium_execution::Retention>()))
        .ok_or(RejectReason::InvalidCost)?;
    Ok(JobCost {
        input_bytes,
        result_bytes,
    })
}
fn retryable(reason: RejectReason) -> bool {
    matches!(
        reason,
        RejectReason::Busy
            | RejectReason::QueueFull
            | RejectReason::JobLimit
            | RejectReason::InputBytes
            | RejectReason::ResultBytes
            | RejectReason::WorkerLimit
            | RejectReason::WorkerBytes
    )
}
fn cancelled(stop: &AtomicBool) -> Result<(), ReadError> {
    if stop.load(Ordering::Acquire) {
        Err(ReadError::File(FileReadFailure::Cancelled))
    } else {
        Ok(())
    }
}

/// Called only by the existing background loader, never the animation/UI loop.
/// It retains the original borrowed request at the FIFO head during pressure.
/// The legacy native loader will be removed in the full owner conversion stage.
pub(super) fn read_local(
    path: &Path,
    limit: usize,
    resources: &AmbientResources,
    stop: &Arc<AtomicBool>,
    capture_storage: &Arc<ilium_execution::StorageAdmission>,
) -> Result<Stored<Vec<u8>>, ReadError> {
    let cost = read_cost(limit).map_err(ReadError::Admission)?;
    let limits = resources.finite().usage().limits;
    if cost.input_bytes > limits.input_bytes || cost.result_bytes > limits.result_bytes {
        return Err(ReadError::Admission(RejectReason::InvalidCost));
    }
    let path_bytes = path
        .as_os_str()
        .len()
        .checked_add(std::mem::size_of::<PathBuf>())
        .and_then(|bytes| {
            bytes.checked_add(
                std::mem::size_of::<ilium_execution::StorageAdmission>()
                    + 2 * std::mem::size_of::<usize>(),
            )
        })
        .ok_or(ReadError::Admission(RejectReason::InvalidCost))?;
    let mut receipt = loop {
        cancelled(stop)?;
        let reservation = match resources.finite().try_reserve(Lane::Io, cost) {
            Ok(reservation) => reservation,
            Err(reason) if retryable(reason) => {
                std::thread::sleep(RETRY_INTERVAL);
                continue;
            }
            Err(reason) => return Err(ReadError::Admission(reason)),
        };
        let path_storage = match resources.reserve_storage(path_bytes) {
            Ok(storage) => storage,
            Err(reason) if retryable(reason) => {
                drop(reservation);
                std::thread::sleep(RETRY_INTERVAL);
                continue;
            }
            Err(reason) => return Err(ReadError::Admission(reason)),
        };
        // Both finite and independent original-capture admission precede cloning.
        let job = ReadJob {
            path: path.to_path_buf(),
            limit,
            stop: stop.clone(),
            _capture_storage: capture_storage.clone(),
            _path_storage: path_storage,
        };
        match reservation.submit(job) {
            Ok(receipt) => break receipt,
            Err(rejected) if retryable(rejected.reason) => {
                drop(rejected.value);
                std::thread::sleep(RETRY_INTERVAL);
            }
            Err(rejected) => return Err(ReadError::Admission(rejected.reason)),
        }
    };
    let outcome = loop {
        if stop.load(Ordering::Acquire) {
            receipt.cancel();
            return Err(ReadError::File(FileReadFailure::Cancelled));
        }
        match receipt.try_take() {
            JobPoll::Pending => std::thread::sleep(RETRY_INTERVAL),
            JobPoll::Ready(outcome) => break outcome,
            JobPoll::Lost | JobPoll::Taken => return Err(ReadError::Retired),
        }
    };
    let (outcome, retention) = outcome.into_parts();
    let bytes = match outcome {
        JobOutcome::Finished(Ok(bytes)) => retention.retain(bytes),
        JobOutcome::Finished(Err(error)) => return Err(ReadError::File(error)),
        JobOutcome::NotStarted { .. } | JobOutcome::Panicked => return Err(ReadError::Retired),
    };
    let storage_bytes = bytes
        .view()
        .capacity()
        .checked_add(std::mem::size_of::<Stored<Vec<u8>>>())
        .and_then(|bytes| {
            bytes.checked_add(
                std::mem::size_of::<ilium_execution::StorageAdmission>()
                    + 2 * std::mem::size_of::<usize>(),
            )
        })
        .ok_or(ReadError::Admission(RejectReason::InvalidCost))?;
    // Original finite receipt remains charged while independent storage is busy.
    let storage = loop {
        cancelled(stop)?;
        if !resources.finite().is_open() {
            return Err(ReadError::Retired);
        }
        match resources.reserve_storage(storage_bytes) {
            Ok(storage) => break storage,
            Err(reason) if retryable(reason) => std::thread::sleep(RETRY_INTERVAL),
            Err(reason) => return Err(ReadError::Admission(reason)),
        }
    };
    let (bytes, retention) = bytes.into_parts();
    let stored = Stored::new(bytes, storage);
    drop(retention);
    Ok(stored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    fn isolated() -> (Execution, AmbientResources, QuotaGroup) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 4,
            service_jobs: 0,
            input_bytes: 128 * 1024 * 1024,
            result_bytes: 128 * 1024 * 1024,
            worker_threads: 1,
            worker_bytes: 8 * 1024 * 1024,
        });
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
                io: LaneConfig {
                    threads: 1,
                    queue_slots: 4,
                    priority: None,
                    resident_bytes_per_thread: 1024,
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
        let client = execution
            .client(ClientLimits {
                jobs: 4,
                service_jobs: 0,
                input_bytes: 128 * 1024 * 1024,
                result_bytes: 128 * 1024 * 1024,
            })
            .unwrap();
        (execution, AmbientResources::new(client), quota)
    }
    #[test]
    fn original_encoded_capacity_is_transferred_before_finite_credit_release_and_last_consumer() {
        let (mut execution, resources, quota) = isolated();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("original.bin");
        let original = vec![37u8; 16 * 1024 + 1];
        std::fs::write(&path, &original).unwrap();
        let capture = resources.reserve_storage(1024).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stored = read_local(&path, original.len(), &resources, &stop, &capture).unwrap();
        assert_eq!(stored.view(), &original);
        assert_eq!(quota.snapshot().jobs, 0);
        assert_eq!(quota.snapshot().input_bytes, 0);
        assert_eq!(quota.snapshot().result_bytes, 0);
        let pointer = stored.view().as_ptr();
        let consumer = Arc::new(stored);
        let last = consumer.clone();
        drop(consumer);
        assert_eq!(last.view().as_ptr(), pointer);
        drop(capture);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(quota.snapshot().worker_threads, 0);
        drop(resources);
        drop(execution);
        assert!(quota.snapshot().worker_bytes >= last.view().capacity());
        drop(last);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn blocked_io_bank_keeps_cancelled_original_file_capture_until_actual_queue_retirement() {
        let (mut execution, resources, quota) = isolated();
        let (started, start) = std::sync::mpsc::sync_channel(1);
        let (release, gate) = std::sync::mpsc::sync_channel(1);
        let mut blocker = resources
            .finite()
            .try_submit(
                Lane::Io,
                JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
                move |_: JobContext| {
                    started.send(()).unwrap();
                    gate.recv().unwrap();
                    Ok::<(), ()>(())
                },
            )
            .unwrap();
        start.recv_timeout(Duration::from_secs(5)).unwrap();
        let before_captures = quota.snapshot().worker_bytes;
        let capture = resources.reserve_storage(1024).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let request_stop = stop.clone();
        let worker_resources = resources.clone();
        let waiter = std::thread::spawn(move || {
            read_local(
                Path::new("/synthetic/not-opened-while-bank-blocked"),
                32,
                &worker_resources,
                &request_stop,
                &capture,
            )
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while quota.snapshot().jobs != 2 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        stop.store(true, Ordering::Release);
        assert!(matches!(
            waiter.join().unwrap(),
            Err(ReadError::File(FileReadFailure::Cancelled))
        ));
        // The producer returned cancellation, but the IO callback is still
        // genuinely blocked and original queued path/stop storage cannot vanish.
        assert_eq!(quota.snapshot().jobs, 2);
        assert!(quota.snapshot().worker_bytes > before_captures + 1024);
        release.send(()).unwrap();
        loop {
            match blocker.try_take() {
                JobPoll::Pending => {
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                JobPoll::Ready(result) => {
                    assert!(matches!(result.view(), JobOutcome::Finished(Ok(()))));
                    drop(result);
                    break;
                }
                JobPoll::Taken | JobPoll::Lost => panic!("actual blocked callback result lost"),
            }
        }
        // Empty-but-live Receipt deliberately retains its finite metadata claim.
        drop(blocker);
        execution.request_shutdown(ShutdownMode::Drain);
        execution.join_until_background(deadline).unwrap();
        assert_eq!(quota.snapshot().jobs, 0);
        drop(resources);
        drop(execution);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
