//! Source-audited PNG CPU preparation on the existing host bank. Other codecs
//! keep their legacy adapter until their own constructor/temporary proof closes.
use super::decode::{decode_png_image, DecodeLimits, DecodedImage};
use crate::resources::{AmbientResources, Stored};
use ilium_execution::{Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, RejectReason};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const RETRY: Duration = Duration::from_millis(100);
// Closed PNG scalar/static diagnostics logical<384/capacity<=1024.
// Authored name length is separately charged; no custom decoder/reader error.
// Exact requested-layout proof and remaining qualification are in proposal README.
const DIAGNOSTIC_BYTES: usize = 1024;

struct PngJob {
    source: Stored<Vec<u8>>,
    name: Stored<String>,
    limits: DecodeLimits,
    max_width: u32,
    max_height: u32,
    resources: AmbientResources,
    stop: Arc<AtomicBool>,
    preflight: Option<PreflightError>,
    _capture_storage: Arc<ilium_execution::StorageAdmission>,
}
impl Job for PngJob {
    type Output = DecodedImage;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() || self.stop.load(Ordering::Acquire) {
            return Err("image preparation cancelled".to_owned());
        }
        if let Some(error) = self.preflight {
            return Err(format!("{}: {error}", self.name.view()));
        }
        let image = decode_png_image(
            self.source.view(),
            &self.limits,
            (self.max_width, self.max_height),
            &self.resources,
            &|| context.stop_requested() || self.stop.load(Ordering::Acquire),
        )
        .map_err(|error| format!("{}: {error}", self.name.view()))?;
        if context.stop_requested() || self.stop.load(Ordering::Acquire) {
            return Err("image preparation cancelled".to_owned());
        }
        Ok(image)
    }
}

#[derive(Clone, Copy, Debug)]
enum PreflightError {
    Layout(super::png_layout::LayoutRefusal, u32, u32),
    Overflow,
    ResourceOverload,
}
impl std::fmt::Display for PreflightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Layout(super::png_layout::LayoutRefusal::Dimensions, width, height) => {
                write!(f, "image is too large ({width} x {height} pixels)")
            }
            Self::Layout(reason, _, _) => {
                write!(f, "not a readable image (PNG layout refused: {reason:?})")
            }
            Self::Overflow => write!(f, "PNG preparation cost overflow"),
            Self::ResourceOverload => {
                write!(f, "PNG preparation exceeds available resource capacity")
            }
        }
    }
}
fn diagnostic_cost(name_bytes: usize) -> Option<JobCost> {
    Some(JobCost {
        input_bytes: (3 * DIAGNOSTIC_BYTES)
            .checked_add(name_bytes.checked_mul(3)?)?
            .checked_add(std::mem::size_of::<PngJob>())?
            .checked_add(std::mem::size_of::<JobOutcome<PngJob>>())?,
        result_bytes: name_bytes
            .checked_mul(2)?
            .checked_add(DIAGNOSTIC_BYTES)?
            .checked_add(std::mem::size_of::<JobOutcome<PngJob>>())?
            .checked_add(std::mem::size_of::<ilium_execution::Retention>())?,
    })
}
pub(super) const METADATA_LIMIT: usize = 4 * 1024 * 1024;

fn vector_peak<T>(count: usize) -> Option<usize> {
    if count == 0 {
        return Some(0);
    }
    let capacity = count.checked_next_power_of_two()?.max(4);
    capacity
        .checked_add(capacity / 2)?
        .checked_mul(std::mem::size_of::<T>())
}
fn cost(
    bytes: &[u8],
    limits: DecodeLimits,
    target_width: u32,
    target_height: u32,
    name_bytes: usize,
) -> Result<JobCost, PreflightError> {
    let layout = super::png_layout::inspect(
        bytes,
        usize::try_from(limits.max_file_bytes).map_err(|_| PreflightError::Overflow)?,
        limits.max_width,
        limits.max_height,
        limits.max_pixels,
    )
    .map_err(|reason| {
        let word = |position| {
            bytes
                .get(position..position + 4)
                .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
                .map(u32::from_be_bytes)
                .unwrap_or(0)
        };
        PreflightError::Layout(reason, word(16), word(20))
    })?;
    let checked = || -> Option<usize> {
        debug_assert!(layout.raw_bytes >= layout.largest_raw_row);
        let width = layout.width as usize;
        let height = layout.height as usize;
        let native = width
            .checked_mul(height)?
            .checked_mul(layout.native_pixel_bytes)?;
        let raw_chunk = METADATA_LIMIT.checked_add(128)?.checked_mul(2)?;
        let metadata = layout
            .copied_metadata_payload
            .checked_mul(2)?
            .checked_add(layout.largest_exif_payload)?;
        let records = layout
            .text_records
            .into_iter()
            .try_fold(0usize, |sum, n| sum.checked_add(n))?;
        let text = layout
            .text_payload
            .checked_mul(6)?
            .checked_add(records.checked_mul(64)?)?
            .checked_add(vector_peak::<png::text_metadata::TEXtChunk>(
                layout.text_records[0],
            )?)?
            .checked_add(vector_peak::<png::text_metadata::ZTXtChunk>(
                layout.text_records[1],
            )?)?
            .checked_add(vector_peak::<png::text_metadata::ITXtChunk>(
                layout.text_records[2],
            )?)?;
        let profile = METADATA_LIMIT.checked_mul(3)?;
        // Secondary table len≤32768; arbitrary resize jumps require capacity≤65536.
        // Two tables per inflater, two coexisting inflaters in iCCP path.
        let inflater = (393216usize + 18432)
            .checked_mul(2)?
            .checked_add(std::mem::size_of::<fdeflate::Decompressor>())?;
        let row = layout.largest_raw_row;
        let shift = row
            .checked_mul(4)?
            .checked_add(63)?
            .checked_div(64)?
            .checked_mul(64)?
            .max(128 * 1024);
        let buffer_bound = shift
            .checked_add(row.checked_mul(2)?)?
            .checked_add(32768 + 8192)?
            .max(128 * 1024)
            .max(16384);
        let rows = buffer_bound
            .checked_mul(3)?
            .checked_add(row.max(8).checked_mul(9)?)?;
        let codec = native
            .checked_add(raw_chunk)?
            .checked_add(metadata)?
            .checked_add(text)?
            .checked_add(profile)?
            .checked_add(inflater)?
            .checked_add(rows)?
            .checked_add(1024)?
            .checked_add(std::mem::size_of::<
                image::codecs::png::PngDecoder<super::decode::CancelCursor<'_>>,
            >())?;
        let mut peak = codec.max(native.checked_mul(2)?);
        // Orientation can swap dimensions even when original dimensions fit.
        for (source_width, source_height) in [(width, height), (height, width)] {
            if source_width > target_width as usize || source_height > target_height as usize {
                let target_width = source_width.min(target_width.max(1) as usize);
                let target_height = source_height.min(target_height.max(1) as usize);
                let output = target_width
                    .checked_mul(target_height)?
                    .checked_mul(layout.native_pixel_bytes)?;
                let vertical = source_width.checked_mul(target_height)?.checked_mul(16)?;
                let weights = source_width
                    .max(source_height)
                    .checked_next_power_of_two()?
                    .max(4)
                    .checked_mul(6)?;
                peak = peak.max(
                    native
                        .checked_add(vertical)?
                        .checked_add(output)?
                        .checked_add(weights)?,
                );
                peak = peak.max(
                    output.checked_add(target_width.checked_mul(target_height)?.checked_mul(4)?)?,
                );
            } else {
                peak = peak.max(native.checked_add(width.checked_mul(height)?.checked_mul(4)?)?);
            }
        }
        peak.checked_add(3 * DIAGNOSTIC_BYTES + 160)?
            .checked_add(std::mem::size_of::<png::DecodingError>())?
            .checked_add(std::mem::size_of::<image::ImageError>())?
            .checked_add(name_bytes.checked_mul(3)?)?
            .checked_add(std::mem::size_of::<PngJob>())?
            .checked_add(std::mem::size_of::<JobOutcome<PngJob>>())
    };
    Ok(JobCost {
        input_bytes: checked().ok_or(PreflightError::Overflow)?,
        result_bytes: name_bytes
            .checked_mul(2)
            .and_then(|bytes| {
                bytes.checked_add(
                    DIAGNOSTIC_BYTES
                        + std::mem::size_of::<JobOutcome<PngJob>>()
                        + std::mem::size_of::<ilium_execution::Retention>(),
                )
            })
            .ok_or(PreflightError::Overflow)?,
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
use super::prepared::PreparationEnv;
pub(super) fn png(
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
    let maximum = resources.finite().usage().limits;
    let checked = cost(
        source.view(),
        limits,
        target_width,
        target_height,
        name.len(),
    );
    let (cost, preflight) = match checked {
        Ok(cost)
            if cost.input_bytes <= maximum.input_bytes
                && cost.result_bytes <= maximum.result_bytes =>
        {
            (cost, None)
        }
        refused => {
            let error = match refused {
                Ok(_) => PreflightError::ResourceOverload,
                Err(error) => error,
            };
            let Some(cost) = diagnostic_cost(name.len()) else {
                tracing::error!("PNG diagnostic capacity overflow");
                return Err(emergency.clone());
            };
            if cost.input_bytes > maximum.input_bytes || cost.result_bytes > maximum.result_bytes {
                tracing::error!("PNG diagnostic capacity refused");
                return Err(emergency.clone());
            }
            (cost, Some(error))
        }
    };
    let name_bytes = name
        .len()
        .checked_add(std::mem::size_of::<Stored<String>>())
        .and_then(|bytes| {
            bytes.checked_add(
                std::mem::size_of::<ilium_execution::StorageAdmission>()
                    + 2 * std::mem::size_of::<usize>(),
            )
        })
        .ok_or_else(|| fail("PNG name storage cost overflow".to_owned()))?;
    if name_bytes
        > resources
            .finite()
            .quota_group()
            .snapshot()
            .limits
            .worker_bytes
    {
        tracing::error!("PNG name storage exceeds resource capacity");
        return Err(emergency.clone());
    }
    let name_storage = loop {
        if stop.load(Ordering::Acquire) {
            return Err(emergency.clone());
        }
        match resources.reserve_storage(name_bytes) {
            Ok(storage) => break storage,
            Err(RejectReason::WorkerBytes | RejectReason::Busy) => std::thread::sleep(RETRY),
            Err(reason) => return Err(fail(format!("PNG name admission refused: {reason:?}"))),
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
                    "PNG preparation admission refused: {reason:?}"
                )));
            }
        };
        let Some((source, name)) = original.take() else {
            return Err(fail("PNG preparation lost its original source".to_owned()));
        };
        let job = PngJob {
            source,
            name,
            limits,
            max_width: target_width,
            max_height: target_height,
            resources: resources.clone(),
            stop: stop.clone(),
            preflight,
            _capture_storage: capture_storage.clone(),
        };
        match admission.submit(job) {
            Ok(receipt) => break receipt,
            Err(rejected) => {
                let reason = rejected.reason;
                original = Some((rejected.value.source, rejected.value.name));
                if !retryable(reason) {
                    return Err(fail(format!(
                        "PNG preparation admission refused: {reason:?}"
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
                            "{}: PNG preparation not started ({reason:?})",
                            job.name.view()
                        ),
                        resources,
                        stop,
                        emergency,
                        Some(retention),
                    )),
                    JobOutcome::Panicked => Err(fail("PNG preparation failed".to_owned())),
                };
            }
            JobPoll::Lost | JobPoll::Taken => {
                return Err(fail("PNG preparation owner retired".to_owned()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::decode::decode_image;
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
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
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
    fn finite_png_matches_backend_pixels_and_preserves_named_error() {
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
        let actual = png(
            stored,
            "fixture.png",
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
            png(stored, "broken.png", DecodeLimits::default(), (4, 2), env()).unwrap_err();
        assert_eq!(failure.view(), &format!("broken.png: {direct}"));
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
    fn cancelled_queued_png_keeps_source_guard_until_actual_cpu_retirement() {
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
            png(
                stored,
                "queued.png",
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
    fn impossible_png_resize_cost_refuses_before_cpu_constructor() {
        let mut bytes = fixture();
        bytes[16..20].copy_from_slice(&16384u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&3906u32.to_be_bytes());
        let cost = cost(&bytes, DecodeLimits::default(), 3840, 2160, 8).unwrap();
        assert!(
            cost.input_bytes > 128 * 1024 * 1024,
            "Triangle/source coexistence must not hide behind nominal leaf cap"
        );
    }
    fn chunk(bytes: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(payload);
        let mut crc = !0u32;
        for byte in kind.iter().chain(payload) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        bytes.extend_from_slice(&(!crc).to_be_bytes());
    }
    fn interlaced_fixture() -> Vec<u8> {
        let mut raw = Vec::new();
        // Uniform8x8RGBA; exact Adam7 pass extents1x1/1x1/2x1/2x2/4x2/4x4/8x4.
        for (width, height) in [(1, 1), (1, 1), (2, 1), (2, 2), (4, 2), (4, 4), (8, 4)] {
            for _ in 0..height {
                raw.extend_from_slice(&[0]);
                for _ in 0..width {
                    raw.extend_from_slice(&[11, 22, 33, 255]);
                }
            }
        }
        assert_eq!(raw.len(), 271);
        let length = raw.len() as u16;
        let mut zlib = vec![0x78, 0x01, 0x01];
        zlib.extend_from_slice(&length.to_le_bytes());
        zlib.extend_from_slice(&(!length).to_le_bytes());
        zlib.extend_from_slice(&raw);
        let (mut a, mut b) = (1u32, 0u32);
        for byte in raw {
            a = (a + u32::from(byte)) % 65521;
            b = (b + a) % 65521;
        }
        zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&8u32.to_be_bytes());
        ihdr.extend_from_slice(&8u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 1]);
        chunk(&mut png, b"IHDR", &ihdr);
        chunk(&mut png, b"IDAT", &zlib);
        chunk(&mut png, b"IEND", &[]);
        png
    }
    #[test]
    fn actual_adam7_fixture_matches_original_pixels_without_fullframe_scratch_declaration() {
        let (mut execution, resources, quota) = isolated();
        let bytes = interlaced_fixture();
        let original = decode_image(&bytes, &DecodeLimits::default(), 8, 8, &resources).unwrap();
        let (source, witness) = stored_fixture(bytes, &resources);
        let capture = resources.reserve_storage(1024).unwrap();
        let emergency = super::super::failure::fallback(capture.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let actual = png(
            source,
            "adam7.png",
            DecodeLimits::default(),
            (8, 8),
            PreparationEnv {
                resources: &resources,
                stop: &stop,
                capture_storage: &capture,
                emergency: &emergency,
            },
        )
        .unwrap();
        assert_eq!(actual, original);
        assert_eq!(actual.pixels, vec![[11, 22, 33]; 64]);
        assert_eq!(witness.strong_count(), 0);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + Duration::from_secs(5))
            .unwrap();
        drop(capture);
        drop(emergency);
        drop(resources);
        drop(execution);
        assert!(quota.snapshot().worker_bytes > 0);
        drop(actual);
        drop(original);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn ordinary_4k_png_real_cpu_output_matches_existing_triangle_and_name_contract() {
        let (mut execution, resources, quota) = isolated();
        let original = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            3840,
            2160,
            image::Rgb([11, 22, 33]),
        ));
        let mut encoded = std::io::Cursor::new(Vec::new());
        original
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        drop(original);
        let bytes = encoded.into_inner();
        let declaration = cost(&bytes, DecodeLimits::default(), 64, 36, 8).unwrap();
        assert!(
            declaration.input_bytes < 128 * 1024 * 1024,
            "row producer, not fullframe unfilter scratch"
        );
        let expected = decode_image(&bytes, &DecodeLimits::default(), 64, 36, &resources).unwrap();
        let (source, witness) = stored_fixture(bytes, &resources);
        let capture = resources.reserve_storage(1024).unwrap();
        let emergency = super::super::failure::fallback(capture.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let actual = png(
            source,
            "large.png",
            DecodeLimits::default(),
            (64, 36),
            PreparationEnv {
                resources: &resources,
                stop: &stop,
                capture_storage: &capture,
                emergency: &emergency,
            },
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(witness.strong_count(), 0);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + Duration::from_secs(5))
            .unwrap();
        drop(capture);
        drop(emergency);
        drop(resources);
        drop(execution);
        drop(actual);
        drop(expected);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn named_crc_failure_matches_original_backend_and_retains_error_until_last_consumer() {
        let (mut execution, resources, quota) = isolated();
        let mut bytes = fixture();
        bytes[32] ^= 1; // Actual IHDR CRC corruption, not altered dimensions.
        let expected =
            decode_image(&bytes, &DecodeLimits::default(), 4, 2, &resources).unwrap_err();
        let (source, witness) = stored_fixture(bytes, &resources);
        let capture = resources.reserve_storage(1024).unwrap();
        let emergency = super::super::failure::fallback(capture.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let error = png(
            source,
            "crc.png",
            DecodeLimits::default(),
            (4, 2),
            PreparationEnv {
                resources: &resources,
                stop: &stop,
                capture_storage: &capture,
                emergency: &emergency,
            },
        )
        .unwrap_err();
        assert_eq!(error.view(), &format!("crc.png: {expected}"));
        assert_eq!(witness.strong_count(), 0);
        let consumer = error.clone();
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + Duration::from_secs(5))
            .unwrap();
        drop(capture);
        drop(emergency);
        drop(resources);
        drop(execution);
        drop(error);
        assert!(quota.snapshot().worker_bytes > 0);
        drop(consumer);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn canceled_actual_png_constructor_keeps_original_until_blocked_cpu_callback_retires() {
        let (mut execution, resources, quota) = isolated();
        let bytes = fixture();
        let declaration = cost(&bytes, DecodeLimits::default(), 4, 2, 0).unwrap();
        let (source, witness) = stored_fixture(bytes, &resources);
        let job_resources = resources.clone();
        let (started, arrival) = std::sync::mpsc::sync_channel(1);
        let (resume, gate) = std::sync::mpsc::sync_channel(0);
        let mut receipt = resources
            .finite()
            .try_submit(
                Lane::Cpu,
                JobCost {
                    input_bytes: declaration.input_bytes + 1024,
                    result_bytes: declaration.result_bytes + 1024,
                },
                move |context: JobContext| {
                    let count = std::cell::Cell::new(0usize);
                    decode_png_image(
                        source.view(),
                        &DecodeLimits::default(),
                        (4, 2),
                        &job_resources,
                        &|| {
                            let next = count.get() + 1;
                            count.set(next);
                            if next == 2 {
                                // Inside real PNG constructor's first fill_buf, not a fake decoder.
                                started.send(()).unwrap();
                                gate.recv().unwrap();
                            }
                            context.stop_requested()
                        },
                    )
                },
            )
            .unwrap();
        arrival.recv_timeout(Duration::from_secs(5)).unwrap();
        receipt.cancel();
        assert_eq!(witness.strong_count(), 1);
        assert_eq!(quota.snapshot().jobs, 1);
        resume.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let outcome = loop {
            match receipt.try_take() {
                JobPoll::Ready(outcome) => break outcome,
                JobPoll::Pending => {
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                _ => panic!("original constructor outcome lost"),
            }
        };
        match outcome.view() {
            JobOutcome::Finished(Err(message)) => {
                assert_eq!(message, "image preparation cancelled")
            }
            _ => panic!("actual constructor cancellation must report its original failure"),
        }
        assert_eq!(witness.strong_count(), 0);
        drop(receipt); // Result still retains finite claim after actual callback ends.
        assert_eq!(quota.snapshot().jobs, 1);
        drop(outcome);
        assert_eq!(quota.snapshot().jobs, 0);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + Duration::from_secs(5))
            .unwrap();
        drop(resources);
        drop(execution);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    fn maximum_code_length_fixture() -> Vec<u8> {
        fn bits(out: &mut Vec<u8>, position: &mut usize, value: u16, count: usize) {
            for bit in 0..count {
                if *position & 7 == 0 {
                    out.push(0);
                }
                let last = out.len() - 1;
                out[last] |= (((value >> bit) & 1) as u8) << (*position % 8);
                *position += 1;
            }
        }
        let mut body = Vec::new();
        let mut position = 0;
        bits(&mut body, &mut position, 5, 3); // Final dynamic Huffman block.
        bits(&mut body, &mut position, 0, 5);
        bits(&mut body, &mut position, 0, 5);
        bits(&mut body, &mut position, 15, 4);
        for symbol in [
            16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
        ] {
            bits(&mut body, &mut position, if symbol < 16 { 4 } else { 0 }, 3);
        }
        for symbol in 0..257 {
            let length = match symbol {
                0..=13 => symbol + 1,
                14 | 256 => 15,
                _ => 0,
            } as u16;
            bits(&mut body, &mut position, length.reverse_bits() >> 12, 4);
        }
        bits(&mut body, &mut position, 1u16.reverse_bits() >> 12, 4); // One distance code, length1.
        for _ in 0..5 {
            bits(&mut body, &mut position, 0, 1);
        } // Filter0 and transparent RGBA.
        bits(&mut body, &mut position, 32767, 15); // Maximum-length canonical EOF.
        let mut zlib = vec![0x78, 0x01];
        zlib.extend_from_slice(&body);
        zlib.extend_from_slice(&[0, 5, 0, 1]);
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&1u32.to_be_bytes());
        ihdr.extend_from_slice(&1u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &ihdr);
        chunk(&mut png, b"IDAT", &zlib);
        chunk(&mut png, b"IEND", &[]);
        png
    }
    #[test]
    fn actual_maximum_huffman_code_png_matches_original_decoder() {
        let (mut execution, resources, quota) = isolated();
        let bytes = maximum_code_length_fixture();
        let expected = decode_image(&bytes, &DecodeLimits::default(), 1, 1, &resources).unwrap();
        assert_eq!(expected.pixels, vec![[0, 0, 0]]);
        let (source, witness) = stored_fixture(bytes, &resources);
        let capture = resources.reserve_storage(1024).unwrap();
        let emergency = super::super::failure::fallback(capture.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let actual = png(
            source,
            "huffman15.png",
            DecodeLimits::default(),
            (1, 1),
            PreparationEnv {
                resources: &resources,
                stop: &stop,
                capture_storage: &capture,
                emergency: &emergency,
            },
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(witness.strong_count(), 0);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + Duration::from_secs(5))
            .unwrap();
        drop(capture);
        drop(emergency);
        drop(resources);
        drop(execution);
        drop(actual);
        drop(expected);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
