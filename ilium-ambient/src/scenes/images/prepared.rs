//! Source-audited BMP CPU preparation on the existing host bank. Other codecs
//! keep their legacy adapter until their own constructor/temporary proof closes.
use super::decode::{decode_image, DecodeLimits, DecodedImage};
use crate::resources::{AmbientResources, Stored};
use ilium_execution::{Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, RejectReason};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const RETRY: Duration = Duration::from_millis(100);
// Pinned BMP diagnostics logical<512/capacity<=1024 (exhaustive scalar/static
// error format inventory in image-preparation-worker.md). Authored name length
// is charged separately in formatting peak/result; no arbitrary decoder allowed.
const DIAGNOSTIC_BYTES: usize = 1024;

struct BmpJob {
    source: Stored<Vec<u8>>,
    name: Stored<String>,
    limits: DecodeLimits,
    max_width: u32,
    max_height: u32,
    resources: AmbientResources,
    stop: Arc<AtomicBool>,
    _capture_storage: Arc<ilium_execution::StorageAdmission>,
}
impl Job for BmpJob {
    type Output = DecodedImage;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() || self.stop.load(Ordering::Acquire) {
            return Err("image preparation cancelled".to_owned());
        }
        let image = decode_image(
            self.source.view(),
            &self.limits,
            self.max_width,
            self.max_height,
            &self.resources,
        )
        .map_err(|error| format!("{}: {error}", self.name.view()))?;
        if context.stop_requested() || self.stop.load(Ordering::Acquire) {
            return Err("image preparation cancelled".to_owned());
        }
        Ok(image)
    }
}

fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let header = u32::from_le_bytes(bytes.get(14..18)?.try_into().ok()?);
    match header {
        12 => Some((
            u32::from(u16::from_le_bytes(bytes.get(18..20)?.try_into().ok()?)),
            u32::from(u16::from_le_bytes(bytes.get(20..22)?.try_into().ok()?)),
        )),
        40 | 52 | 56 | 108 | 124 => {
            let width = i32::from_le_bytes(bytes.get(18..22)?.try_into().ok()?);
            let height = i32::from_le_bytes(bytes.get(22..26)?.try_into().ok()?);
            Some((u32::try_from(width).ok()?, height.unsigned_abs()))
        }
        _ => None,
    }
}
fn cost(
    bytes: &[u8],
    limits: DecodeLimits,
    target_width: u32,
    target_height: u32,
    name_bytes: usize,
) -> Result<JobCost, String> {
    if bytes.len() as u64 > limits.max_file_bytes {
        return Err("file exceeds image preparation byte limit".to_owned());
    }
    let (width, height) = dimensions(bytes)
        .ok_or_else(|| "not a readable image (invalid bitmap dimensions/header)".to_owned())?;
    let pixels = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || width > limits.max_width
        || height > limits.max_height
        || pixels > limits.max_pixels
    {
        return Err(format!("image is too large ({width} x {height} pixels)"));
    }
    let checked = || -> Option<usize> {
        let native = usize::try_from(pixels).ok()?.checked_mul(4)?;
        // Constructor raw palette1024 + RGBpalette768, indexed row≤4W+4,
        // RLE absolute opcode u8 gives atmost256 bytes. No codec helper thread.
        let codec = native
            .checked_add(1024 + 768 + 256)?
            .checked_add((width as usize).checked_mul(4)?.checked_add(4)?)?
            .checked_add(std::mem::size_of::<
                image::codecs::bmp::BmpDecoder<std::io::Cursor<&[u8]>>,
            >())?;
        let mut peak = codec.max(native.checked_mul(2)?); // native plus RGBA conversion
        if width > target_width || height > target_height {
            // Preserve aspect resize; these rectangular upper bounds include
            // actual rounded target and intermediate, never upscale.
            let target_width = width.min(target_width.max(1)) as usize;
            let target_height = height.min(target_height.max(1)) as usize;
            let output = target_width.checked_mul(target_height)?.checked_mul(4)?;
            let vertical = (width as usize)
                .checked_mul(target_height)?
                .checked_mul(16)?;
            // Triangle weights use push from empty (min4) then clear/reuse;
            // power-of-two capacity plus old growth:6 bytes per float slot.
            let weights = (width.max(height) as usize)
                .checked_next_power_of_two()?
                .max(4)
                .checked_mul(6)?;
            let resize = native
                .checked_add(vertical)?
                .checked_add(output)?
                .checked_add(weights)?;
            peak = peak.max(resize).max(output.checked_mul(2)?);
        }
        peak.checked_add(3 * DIAGNOSTIC_BYTES)?
            .checked_add(name_bytes.checked_mul(3)?)?
            .checked_add(std::mem::size_of::<BmpJob>())?
            .checked_add(std::mem::size_of::<JobOutcome<BmpJob>>())
    };
    Ok(JobCost {
        input_bytes: checked().ok_or_else(|| "bitmap preparation cost overflow".to_owned())?,
        result_bytes: name_bytes
            .checked_mul(2)
            .and_then(|bytes| {
                bytes.checked_add(
                    DIAGNOSTIC_BYTES
                        + std::mem::size_of::<JobOutcome<BmpJob>>()
                        + std::mem::size_of::<ilium_execution::Retention>(),
                )
            })
            .ok_or_else(|| "bitmap diagnostic cost overflow".to_owned())?,
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

/// The original encoded allocation moves once into the CPU callback; pressure
/// retains it at this loader's FIFO head without another source clone.
pub(super) struct PreparationEnv<'a> {
    pub resources: &'a AmbientResources,
    pub stop: &'a Arc<AtomicBool>,
    pub capture_storage: &'a Arc<ilium_execution::StorageAdmission>,
    pub emergency: &'a Arc<Stored<String>>,
}
pub(super) fn bmp(
    source: Stored<Vec<u8>>,
    name: &str,
    limits: DecodeLimits,
    target: (u32, u32),
    env: PreparationEnv<'_>,
) -> Result<DecodedImage, Arc<Stored<String>>> {
    let PreparationEnv {
        resources,
        stop,
        capture_storage,
        emergency,
    } = env;
    let (target_width, target_height) = target;
    let fail = |message| super::failure::retain(message, resources, stop, emergency, None);
    let cost = cost(
        source.view(),
        limits,
        target_width,
        target_height,
        name.len(),
    )
    .map_err(fail)?;
    let maximum = resources.finite().usage().limits;
    if cost.input_bytes > maximum.input_bytes || cost.result_bytes > maximum.result_bytes {
        return Err(fail(
            "bitmap preparation exceeds available resource capacity".to_owned(),
        ));
    }
    let name_bytes = name
        .len()
        .checked_add(std::mem::size_of::<Stored<String>>())
        .and_then(|bytes| {
            bytes.checked_add(
                std::mem::size_of::<ilium_execution::StorageAdmission>()
                    + 2 * std::mem::size_of::<usize>(),
            )
        })
        .ok_or_else(|| fail("bitmap name storage cost overflow".to_owned()))?;
    let name_storage = loop {
        if stop.load(Ordering::Acquire) {
            return Err(emergency.clone());
        }
        match resources.reserve_storage(name_bytes) {
            Ok(storage) => break storage,
            Err(RejectReason::WorkerBytes | RejectReason::Busy) => std::thread::sleep(RETRY),
            Err(reason) => return Err(fail(format!("bitmap name admission refused: {reason:?}"))),
        }
    };
    // BEFORE cloning; rejected submission keeps this original guarded copy.
    let original_name = Stored::new(name.to_owned(), name_storage);
    let mut original = Some((source, original_name));
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
                    "bitmap preparation admission refused: {reason:?}"
                )))
            }
        };
        let Some((source, name)) = original.take() else {
            return Err(fail(
                "bitmap preparation lost its original source".to_owned(),
            ));
        };
        let job = BmpJob {
            source,
            name,
            limits,
            max_width: target_width,
            max_height: target_height,
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
                        "bitmap preparation admission refused: {reason:?}"
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
                            "{}: bitmap preparation not started ({reason:?})",
                            job.name.view()
                        ),
                        resources,
                        stop,
                        emergency,
                        Some(retention),
                    )),
                    JobOutcome::Panicked => Err(fail("bitmap preparation failed".to_owned())),
                };
            }
            JobPoll::Lost | JobPoll::Taken => {
                return Err(fail("bitmap preparation owner retired".to_owned()))
            }
        }
    }
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
        let empty = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 4,
                    priority: None,
                    resident_bytes_per_thread: 1024,
                },
                io: empty,
                service: empty,
            },
        )
        .unwrap();
        let resources = AmbientResources::new(
            execution
                .client(ClientLimits {
                    jobs: 4,
                    service_jobs: 0,
                    input_bytes: 128 * 1024 * 1024,
                    result_bytes: 128 * 1024 * 1024,
                })
                .unwrap(),
        );
        (execution, resources, quota)
    }
    fn fixture() -> Vec<u8> {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            4,
            2,
            image::Rgb([11, 22, 33]),
        ));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Bmp).unwrap();
        bytes.into_inner()
    }
    fn stored_fixture(
        bytes: Vec<u8>,
        resources: &AmbientResources,
    ) -> (
        Stored<Vec<u8>>,
        std::sync::Weak<ilium_execution::StorageAdmission>,
    ) {
        // Fixture encoding is test-owned. Production obtains this original guard
        // from finite encoded IO before CPU preparation.
        let guard = resources
            .reserve_storage(
                bytes.capacity()
                    + std::mem::size_of::<Stored<Vec<u8>>>()
                    + std::mem::size_of::<ilium_execution::StorageAdmission>()
                    + 2 * std::mem::size_of::<usize>(),
            )
            .unwrap();
        let witness = Arc::downgrade(&guard);
        (Stored::new(bytes, guard), witness)
    }
    #[test]
    fn finite_bitmap_matches_backend_pixels_and_preserves_named_error() {
        let (mut execution, resources, quota) = isolated();
        let bytes = fixture();
        let expected = decode_image(&bytes, &DecodeLimits::default(), 2, 1, &resources).unwrap();
        let (stored, witness) = stored_fixture(bytes, &resources);
        let capture = resources.reserve_storage(1024).unwrap();
        let emergency = super::super::failure::fallback(capture.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let env = || PreparationEnv {
            resources: &resources,
            stop: &stop,
            capture_storage: &capture,
            emergency: &emergency,
        };
        let actual = bmp(
            stored,
            "fixture.bmp",
            DecodeLimits::default(),
            (2, 1),
            env(),
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            witness.strong_count(),
            0,
            "original encoded source survives only through actual callback"
        );
        assert_eq!(quota.snapshot().jobs, 0);
        let mut malformed = fixture();
        malformed.truncate(54); // Valid dimensions, incomplete actual pixel body.
        let direct =
            decode_image(&malformed, &DecodeLimits::default(), 4, 2, &resources).unwrap_err();
        let (stored, _) = stored_fixture(malformed, &resources);
        let failure =
            bmp(stored, "broken.bmp", DecodeLimits::default(), (4, 2), env()).unwrap_err();
        assert_eq!(failure.view(), &format!("broken.bmp: {direct}"));
        assert_eq!(quota.snapshot().jobs, 0);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + Duration::from_secs(5))
            .unwrap();
        drop(capture);
        drop(emergency);
        drop(resources);
        drop(execution);
        assert!(
            quota.snapshot().worker_bytes > 0,
            "pixels/error remain charged after bank exit"
        );
        drop(expected);
        drop(actual);
        drop(failure);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn cancelled_queued_bitmap_keeps_source_guard_until_actual_cpu_retirement() {
        let (mut execution, resources, quota) = isolated();
        let (started, arrival) = std::sync::mpsc::sync_channel(1);
        let (resume, gate) = std::sync::mpsc::sync_channel(0);
        let mut blocker = resources
            .finite()
            .try_submit(
                Lane::Cpu,
                JobCost {
                    input_bytes: 1024,
                    result_bytes: 1024,
                },
                move |_: JobContext| {
                    started.send(()).unwrap();
                    gate.recv().unwrap();
                    Ok::<(), ()>(())
                },
            )
            .unwrap();
        arrival.recv_timeout(Duration::from_secs(5)).unwrap();
        let (stored, witness) = stored_fixture(fixture(), &resources);
        let capture = resources.reserve_storage(1024).unwrap();
        let emergency = super::super::failure::fallback(capture.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_resources = resources.clone();
        let worker_capture = capture.clone();
        let worker_emergency = emergency.clone();
        let producer = std::thread::spawn(move || {
            bmp(
                stored,
                "queued.bmp",
                DecodeLimits::default(),
                (4, 2),
                PreparationEnv {
                    resources: &worker_resources,
                    stop: &worker_stop,
                    capture_storage: &worker_capture,
                    emergency: &worker_emergency,
                },
            )
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while quota.snapshot().jobs != 2 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        stop.store(true, Ordering::Release);
        let failed = producer.join().unwrap().unwrap_err();
        assert_eq!(failed.view(), super::super::failure::FALLBACK);
        assert_eq!(quota.snapshot().jobs, 2);
        assert_eq!(
            witness.strong_count(),
            1,
            "queued original source cannot release before actual bank retirement"
        );
        resume.send(()).unwrap();
        loop {
            match blocker.try_take() {
                JobPoll::Ready(outcome) => {
                    drop(outcome);
                    break;
                }
                JobPoll::Pending => {
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                _ => panic!("blocker receipt lost"),
            }
        }
        drop(blocker); // Empty receipt metadata keeps its finite claim until Drop.
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(witness.strong_count(), 0);
        assert_eq!(quota.snapshot().jobs, 0);
        drop(failed);
        drop(capture);
        drop(emergency);
        drop(resources);
        drop(execution);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn impossible_bitmap_resize_cost_refuses_before_cpu_constructor() {
        let mut bytes = fixture();
        bytes[18..22].copy_from_slice(&16384i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&3906i32.to_le_bytes());
        let cost = cost(&bytes, DecodeLimits::default(), 3840, 2160, 8).unwrap();
        assert!(
            cost.input_bytes > 128 * 1024 * 1024,
            "Triangle/source coexistence must not hide behind nominal leaf cap"
        );
    }
}
