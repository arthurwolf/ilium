//! Bounded execution for codecs without a source-audited specialized adapter.

use super::decode::{
    decode_image_cancellable_with_alloc, inspect_dimensions_cancellable, DecodeLimits, DecodedImage,
};
use crate::resources::{AmbientResources, Stored};
use ilium_execution::{Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, RejectReason};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const RETRY: Duration = Duration::from_millis(100);
const METADATA_ALLOWANCE: usize = 64 * 1024;

struct MetadataJob {
    source: Arc<Stored<Vec<u8>>>,
    limits: DecodeLimits,
    stop: Arc<AtomicBool>,
}

impl Job for MetadataJob {
    type Output = Metadata;
    type Error = String;

    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        let cancelled = || context.stop_requested() || self.stop.load(Ordering::Acquire);
        let dimensions =
            inspect_dimensions_cancellable(self.source.view(), &self.limits, &cancelled)?;
        Ok(Metadata {
            dimensions,
            #[cfg(test)]
            worker_thread_name: std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_owned(),
        })
    }
}

struct Metadata {
    dimensions: (u32, u32),
    #[cfg(test)]
    worker_thread_name: String,
}

struct GenericJob {
    source: Arc<Stored<Vec<u8>>>,
    name: Stored<String>,
    limits: DecodeLimits,
    target: (u32, u32),
    max_alloc: u64,
    resources: AmbientResources,
    stop: Arc<AtomicBool>,
    _capture_storage: Arc<ilium_execution::StorageAdmission>,
}

impl Job for GenericJob {
    type Output = DecodedImage;
    type Error = String;

    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        let cancelled = || context.stop_requested() || self.stop.load(Ordering::Acquire);
        decode_image_cancellable_with_alloc(
            self.source.view(),
            &self.limits,
            self.target.0,
            self.target.1,
            &self.resources,
            &cancelled,
            self.max_alloc,
        )
        .map_err(|error| format!("{}: {error}", self.name.view()))
    }
}

fn cost(
    source: &[u8],
    dimensions: (u32, u32),
    name_bytes: usize,
    target: (u32, u32),
    max_alloc: u64,
) -> Result<JobCost, String> {
    let decoder_allowance = usize::try_from(max_alloc / 32)
        .map_err(|_| "image preparation cost overflow".to_owned())?;
    let pixels = usize::try_from(u64::from(dimensions.0) * u64::from(dimensions.1))
        .map_err(|_| "image preparation cost overflow".to_owned())?;
    // The pinned image 0.25 resizer holds an RGBA32F vertical intermediate
    // (up to 16 bytes per source pixel) while retaining the decoded source and
    // allocates the resized output before replacing it. EXIF quarter-turns can
    // also transiently retain both source and rotated images. Reserve a
    // conservative 36 bytes/source pixel for those overlapping buffers, plus
    // encoded copies and bounded metadata. `max_alloc` is a non-strict limit on
    // simultaneous decoder allocations; some decoders may ignore it, so it is
    // not treated as the sole bound on the complete preparation peak.
    let input_bytes = pixels
        .checked_mul(36)
        .and_then(|bytes| bytes.checked_add(source.len().checked_mul(2)?))
        .and_then(|bytes| bytes.checked_add(METADATA_ALLOWANCE))
        .and_then(|bytes| bytes.checked_add(name_bytes.checked_mul(3)?))
        .and_then(|bytes| bytes.checked_add(decoder_allowance))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<GenericJob>()))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<JobOutcome<GenericJob>>()))
        .ok_or_else(|| "image preparation cost overflow".to_owned())?;
    let output_pixels = u64::from(dimensions.0.min(target.0.max(1)))
        .checked_mul(u64::from(dimensions.1.min(target.1.max(1))))
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| "image result cost overflow".to_owned())?;
    let result_bytes = output_pixels
        .checked_mul(std::mem::size_of::<[u8; 3]>())
        .and_then(|bytes| bytes.checked_add(name_bytes.checked_mul(2)?))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<ilium_execution::Retention>()))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<JobOutcome<GenericJob>>()))
        .ok_or_else(|| "image result cost overflow".to_owned())?;
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
    )
}

fn inspect_dimensions(
    source: Arc<Stored<Vec<u8>>>,
    limits: DecodeLimits,
    resources: &AmbientResources,
    stop: &Arc<AtomicBool>,
) -> Result<Metadata, String> {
    let cost = JobCost {
        input_bytes: source
            .view()
            .len()
            .checked_add(METADATA_ALLOWANCE)
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<MetadataJob>()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<JobOutcome<MetadataJob>>()))
            .ok_or_else(|| "image metadata cost overflow".to_owned())?,
        result_bytes: std::mem::size_of::<Metadata>()
            .checked_add(std::mem::size_of::<JobOutcome<MetadataJob>>())
            .ok_or_else(|| "image metadata cost overflow".to_owned())?,
    };
    let quota_limits = resources.finite().usage().limits;
    if cost.input_bytes > quota_limits.input_bytes || cost.result_bytes > quota_limits.result_bytes
    {
        return Err("image metadata exceeds available resource capacity".to_owned());
    }
    let mut receipt = loop {
        if stop.load(Ordering::Acquire) {
            return Err("image preparation cancelled".to_owned());
        }
        let admission = match resources.finite().try_reserve(Lane::Cpu, cost) {
            Ok(admission) => admission,
            Err(reason) if retryable(reason) => {
                std::thread::sleep(RETRY);
                continue;
            }
            Err(reason) => return Err(format!("image metadata admission refused: {reason:?}")),
        };
        let job = MetadataJob {
            source: source.clone(),
            limits,
            stop: stop.clone(),
        };
        match admission.submit(job) {
            Ok(receipt) => break receipt,
            Err(rejected) if retryable(rejected.reason) => {
                std::thread::sleep(RETRY);
            }
            Err(rejected) => {
                return Err(format!(
                    "image metadata admission refused: {:?}",
                    rejected.reason
                ));
            }
        }
    };
    loop {
        if stop.load(Ordering::Acquire) {
            receipt.cancel();
            return Err("image preparation cancelled".to_owned());
        }
        match receipt.try_take() {
            JobPoll::Pending => std::thread::sleep(RETRY),
            JobPoll::Ready(outcome) => {
                let (outcome, _) = outcome.into_parts();
                return match outcome {
                    JobOutcome::Finished(result) => result,
                    JobOutcome::NotStarted { reason, .. } => {
                        Err(format!("image metadata job not started ({reason:?})"))
                    }
                    JobOutcome::Panicked => Err("image metadata inspection failed".to_owned()),
                };
            }
            JobPoll::Lost | JobPoll::Taken => return Err("image metadata owner retired".to_owned()),
        }
    }
}

/// Keep the original encoded allocation at the image loader's FIFO head while
/// waiting for a bounded CPU reservation. The finite receipt owns execution,
/// cancellation, panic reporting and result retention.
pub(super) fn decode(
    source: Stored<Vec<u8>>,
    name: &str,
    limits: DecodeLimits,
    target: (u32, u32),
    resources: &AmbientResources,
    stop: &Arc<AtomicBool>,
    capture_storage: &Arc<ilium_execution::StorageAdmission>,
    emergency: &Arc<Stored<String>>,
) -> Result<DecodedImage, Arc<Stored<String>>> {
    let fail = |message| super::failure::retain(message, resources, stop, emergency, None);
    let source = Arc::new(source);
    let metadata = inspect_dimensions(source.clone(), limits, resources, stop)
        .map_err(|message| fail(message))?;
    let dimensions = metadata.dimensions;
    let max_alloc = u64::from(dimensions.0)
        .checked_mul(u64::from(dimensions.1))
        .and_then(|pixels| pixels.checked_mul(6))
        .ok_or_else(|| fail("image decoder allocation cost overflow".to_owned()))?
        .min(1 << 30);
    let cost = cost(source.view(), dimensions, name.len(), target, max_alloc)
        .map_err(|message| fail(message))?;
    let maximum = resources.finite().usage().limits;
    if cost.input_bytes > maximum.input_bytes || cost.result_bytes > maximum.result_bytes {
        return Err(fail(
            "image preparation exceeds available resource capacity".to_owned(),
        ));
    }
    let name_storage_bytes = name
        .len()
        .checked_add(std::mem::size_of::<Stored<String>>())
        .and_then(|bytes| {
            bytes.checked_add(
                std::mem::size_of::<ilium_execution::StorageAdmission>()
                    + 2 * std::mem::size_of::<usize>(),
            )
        })
        .ok_or_else(|| fail("image name storage cost overflow".to_owned()))?;
    if name_storage_bytes
        > resources
            .finite()
            .quota_group()
            .snapshot()
            .limits
            .worker_bytes
    {
        return Err(fail(
            "image name storage exceeds available resource capacity".to_owned(),
        ));
    }
    let name_storage = loop {
        if stop.load(Ordering::Acquire) {
            return Err(emergency.clone());
        }
        match resources.reserve_storage(name_storage_bytes) {
            Ok(storage) => break storage,
            Err(RejectReason::Busy | RejectReason::WorkerBytes) => std::thread::sleep(RETRY),
            Err(reason) => return Err(fail(format!("image name admission refused: {reason:?}"))),
        }
    };
    let mut original = Some((source, Stored::new(name.to_owned(), name_storage)));
    let mut receipt = loop {
        if stop.load(Ordering::Acquire) {
            return Err(emergency.clone());
        }
        let admission = match resources.finite().try_reserve(Lane::Cpu, cost) {
            Ok(admission) => admission,
            Err(reason) if retryable(reason) => {
                std::thread::sleep(RETRY);
                continue;
            }
            Err(reason) => {
                return Err(fail(format!(
                    "image preparation admission refused: {reason:?}"
                )));
            }
        };
        let Some((source, name)) = original.take() else {
            return Err(fail(
                "image preparation lost its original source".to_owned(),
            ));
        };
        let job = GenericJob {
            source,
            name,
            limits,
            target,
            max_alloc,
            resources: resources.clone(),
            stop: stop.clone(),
            _capture_storage: capture_storage.clone(),
        };
        match admission.submit(job) {
            Ok(receipt) => break receipt,
            Err(rejected) => {
                let reason = rejected.reason;
                original = Some((rejected.value.source, rejected.value.name));
                if !retryable(reason) {
                    return Err(fail(format!(
                        "image preparation admission refused: {reason:?}"
                    )));
                }
                std::thread::sleep(RETRY);
            }
        }
    };
    loop {
        if stop.load(Ordering::Acquire) {
            receipt.cancel();
            return Err(emergency.clone());
        }
        match receipt.try_take() {
            JobPoll::Pending => std::thread::sleep(RETRY),
            JobPoll::Ready(outcome) => {
                let (outcome, retention) = outcome.into_parts();
                return match outcome {
                    JobOutcome::Finished(Ok(image)) => Ok(image),
                    JobOutcome::Finished(Err(message)) => Err(super::failure::retain(
                        message,
                        resources,
                        stop,
                        emergency,
                        Some(retention),
                    )),
                    JobOutcome::NotStarted { job, reason } => Err(super::failure::retain(
                        format!(
                            "{}: image preparation not started ({reason:?})",
                            job.name.view()
                        ),
                        resources,
                        stop,
                        emergency,
                        Some(retention),
                    )),
                    JobOutcome::Panicked => Err(fail("image preparation failed".to_owned())),
                };
            }
            JobPoll::Lost | JobPoll::Taken => {
                return Err(fail("image preparation owner retired".to_owned()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::decode::{fixtures::jpeg_bytes, DecodeLimits};
    use super::{cost, inspect_dimensions};
    use crate::resources::{test_resources, Stored};
    use std::sync::{atomic::AtomicBool, Arc};

    #[test]
    fn four_k_generic_image_fits_the_ambient_working_set_budget() {
        let dimensions = (3840, 2160);
        let pixels = u64::from(dimensions.0) * u64::from(dimensions.1);
        let cost = cost(&[0; 1024], dimensions, 32, (1024, 576), pixels * 6)
            .expect("4K image cost is representable");
        assert!(cost.input_bytes > 128 * 1024 * 1024);
        assert!(
            cost.input_bytes <= 384 * 1024 * 1024,
            "4K source should fit the ambient child budget, got {} bytes",
            cost.input_bytes
        );
    }

    #[test]
    fn generic_image_metadata_inspection_runs_on_the_cpu_bank() {
        let bytes = jpeg_bytes(8, 6, |_, _| [80, 120, 160]);
        let resources = test_resources();
        let storage = resources
            .reserve_storage(bytes.capacity())
            .expect("fixture storage");
        let source = Arc::new(Stored::new(bytes, storage));
        let metadata = inspect_dimensions(
            source,
            DecodeLimits::default(),
            &resources,
            &Arc::new(AtomicBool::new(false)),
        )
        .expect("valid JPEG metadata");
        assert_eq!(metadata.dimensions, (8, 6));
        assert!(
            metadata.worker_thread_name.starts_with("ilium-exec-cpu-"),
            "{}",
            metadata.worker_thread_name
        );
    }
}
