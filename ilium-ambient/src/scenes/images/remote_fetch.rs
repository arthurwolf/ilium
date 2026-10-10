//! HTTPS cache reads and downloads on the host's bounded I/O lane.
use crate::resources::{AmbientResources, Stored};
#[cfg(test)]
use crate::source::cache_file_name;
use crate::source::{fetch_cached, FetchError};
use ilium_execution::{Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, RejectReason};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const RETRY: Duration = Duration::from_millis(100);
const IO_OVERHEAD: usize = 64 * 1024;

pub(super) struct FetchedBytes {
    pub(super) bytes: Stored<Vec<u8>>,
    #[cfg(test)]
    worker_thread_name: String,
}

struct FetchJob {
    cache_dir: PathBuf,
    url: String,
    extension: String,
    max_age: Duration,
    max_bytes: usize,
    timeout: Duration,
    stop: Arc<AtomicBool>,
    _capture_storage: Arc<ilium_execution::StorageAdmission>,
    _response_storage: Arc<ilium_execution::StorageAdmission>,
}

impl Job for FetchJob {
    type Output = FetchedBytes;
    type Error = String;

    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() || self.stop.load(Ordering::Acquire) {
            return Err(FetchError::Cancelled.to_string());
        }
        let bytes = fetch_cached(
            &self.cache_dir,
            &self.url,
            &self.extension,
            self.max_age,
            self.max_bytes,
            self.timeout,
        )
        .map_err(|error| error.to_string())?;
        if context.stop_requested() || self.stop.load(Ordering::Acquire) {
            return Err(FetchError::Cancelled.to_string());
        }
        Ok(FetchedBytes {
            bytes: Stored::new(bytes, self._response_storage),
            #[cfg(test)]
            worker_thread_name: std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_owned(),
        })
    }
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

/// Retain the image loader's FIFO head while waiting for bounded I/O admission.
/// The response remains storage-charged after its finite receipt is collected.
#[allow(clippy::too_many_arguments)] // Explicit audited fetch limits and resource authorities vary independently.
pub(super) fn fetch(
    cache_dir: &Path,
    url: &str,
    extension: &str,
    max_age: Duration,
    max_bytes: usize,
    timeout: Duration,
    resources: &AmbientResources,
    stop: &Arc<AtomicBool>,
    capture_storage: &Arc<ilium_execution::StorageAdmission>,
) -> Result<FetchedBytes, String> {
    let cost = JobCost {
        input_bytes: max_bytes
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(IO_OVERHEAD))
            .and_then(|bytes| bytes.checked_add(url.len().checked_mul(2)?))
            .and_then(|bytes| bytes.checked_add(cache_dir.as_os_str().len()))
            .and_then(|bytes| bytes.checked_add(extension.len()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<FetchJob>()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<JobOutcome<FetchJob>>()))
            .ok_or_else(|| "image download cost overflow".to_owned())?,
        result_bytes: max_bytes
            .checked_add(std::mem::size_of::<FetchedBytes>())
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<JobOutcome<FetchJob>>()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<ilium_execution::Retention>()))
            .ok_or_else(|| "image download result cost overflow".to_owned())?,
    };
    let limits = resources.finite().usage().limits;
    if cost.input_bytes > limits.input_bytes || cost.result_bytes > limits.result_bytes {
        return Err("image download exceeds available resource capacity".to_owned());
    }
    let response_storage_bytes = max_bytes
        .checked_add(std::mem::size_of::<Stored<Vec<u8>>>())
        .and_then(|bytes| {
            bytes.checked_add(std::mem::size_of::<ilium_execution::StorageAdmission>())
        })
        .and_then(|bytes| bytes.checked_add(2 * std::mem::size_of::<usize>()))
        .ok_or_else(|| "image download storage cost overflow".to_owned())?;
    if response_storage_bytes
        > resources
            .finite()
            .quota_group()
            .snapshot()
            .limits
            .worker_bytes
    {
        return Err("image download storage exceeds available resource capacity".to_owned());
    }
    let mut receipt = loop {
        if stop.load(Ordering::Acquire) {
            return Err(FetchError::Cancelled.to_string());
        }
        let admission = match resources.finite().try_reserve(Lane::Io, cost) {
            Ok(admission) => admission,
            Err(reason) if retryable(reason) => {
                std::thread::sleep(RETRY);
                continue;
            }
            Err(reason) => return Err(format!("image download admission refused: {reason:?}")),
        };
        let response_storage = match resources.reserve_storage(response_storage_bytes) {
            Ok(storage) => storage,
            Err(reason) if retryable(reason) => {
                drop(admission);
                std::thread::sleep(RETRY);
                continue;
            }
            Err(reason) => return Err(format!("image download storage refused: {reason:?}")),
        };
        let job = FetchJob {
            cache_dir: cache_dir.to_path_buf(),
            url: url.to_owned(),
            extension: extension.to_owned(),
            max_age,
            max_bytes,
            timeout,
            stop: stop.clone(),
            _capture_storage: capture_storage.clone(),
            _response_storage: response_storage,
        };
        match admission.submit(job) {
            Ok(receipt) => break receipt,
            Err(rejected) if retryable(rejected.reason) => {
                drop(rejected.value);
                std::thread::sleep(RETRY);
            }
            Err(rejected) => {
                return Err(format!(
                    "image download admission refused: {:?}",
                    rejected.reason
                ));
            }
        }
    };
    loop {
        if stop.load(Ordering::Acquire) {
            receipt.cancel();
            return Err(FetchError::Cancelled.to_string());
        }
        match receipt.try_take() {
            JobPoll::Pending => std::thread::sleep(RETRY),
            JobPoll::Ready(outcome) => {
                let (outcome, retention) = outcome.into_parts();
                return match outcome {
                    JobOutcome::Finished(Ok(fetched)) => {
                        // The response's Stored wrapper owns the longer-lived
                        // byte reservation; the receipt charge ends at transfer.
                        drop(retention);
                        Ok(fetched)
                    }
                    JobOutcome::Finished(Err(error)) => Err(error),
                    JobOutcome::NotStarted { reason, .. } => {
                        Err(format!("image download job not started ({reason:?})"))
                    }
                    JobOutcome::Panicked => Err("image download worker failed".to_owned()),
                };
            }
            JobPoll::Lost | JobPoll::Taken => {
                return Err("image download owner retired".to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn cached_image_read_runs_on_the_shared_io_bank() {
        let cache_dir = tempfile::tempdir().expect("unique fixture directory");
        let url = "https://example.invalid/image.png";
        let content = b"fixture image bytes";
        fs::write(cache_dir.path().join(cache_file_name(url, "img")), content)
            .expect("cached fixture");
        let resources = crate::resources::test_resources();
        let quota = resources.finite().quota_group();
        let baseline = quota.snapshot().worker_bytes;
        let capture = resources.reserve_storage(1024).expect("capture storage");
        let with_capture = quota.snapshot().worker_bytes;
        let stop = Arc::new(AtomicBool::new(false));
        let result = fetch(
            cache_dir.path(),
            url,
            "img",
            Duration::from_secs(3600),
            1024,
            Duration::from_secs(1),
            &resources,
            &stop,
            &capture,
        )
        .expect("cache hit");
        assert_eq!(result.bytes.view(), content);
        assert!(result.worker_thread_name.starts_with("ilium-exec-io-"));
        assert!(quota.snapshot().worker_bytes > with_capture);
        drop(result);
        assert_eq!(quota.snapshot().worker_bytes, with_capture);
        drop(capture);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
    }
}
