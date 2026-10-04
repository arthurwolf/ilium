//! Concrete pipe-fed ffmpeg codec. Production activation requires the platform
//! `spawn_video_decoder` prerequisite: private namespace/cgroup/owned kill-reap.
//! This decoder accepts no script paths, URLs, command strings or native handles.
//! Constant-rate output deliberately resamples original variable frame timing;
//! PTS is exact rational output timing. Streaming stdin does not support seeking.
use crate::{
    error::{AnimationError, Result},
    native_video::{
        CodecBudget, CodecInterrupt, FrameStamp, VerifiedVideoInput, VideoDecoder,
        VideoDecoderFactory, VideoGeometry, VideoInfo, VideoPixelFormat,
    },
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::{
    animation_sandbox::{self, SandboxCancel, SandboxChild, SandboxLimits},
    owned_worker::{self, OwnedWorker, StopToken, WorkerKind},
};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::ChildStdout,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
// Trusted admission profile for the fixed one-input/one-output FFmpeg 8 CLI:
// reserve two bwrap launcher/PID-1 control slots, one FFmpeg main slot, five
// scheduler roles (demux/decode/filter/encode/mux), and three possible companion
// slots for the explicitly single-threaded codec/filter execution stages. The
// last three are deliberate headroom, not observed threads; some control and
// scheduler roles can reuse tasks. The actual four-task denial proves only that
// four is insufficient. Eleven is a conservative configuration floor for this
// fixed pipeline, neither a measured minimum nor a guarantee for arbitrary
// codec libraries or future FFmpeg builds. The cgroup remains the hard ceiling;
// an extra-thread build fails closed and requires a new actual qualification.
const FFMPEG_PROFILE_TASK_RESERVATION: u32 = 11;

fn failure(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("ffmpeg video decoder: {message}"))
}
/// All fields are trusted host configuration, never script-provided arguments.
#[derive(Debug, Clone)]
pub struct FfmpegLimits {
    pub executable: PathBuf,
    pub sandbox: SandboxLimits,
    pub fps_numerator: u32,
    pub fps_denominator: u32,
    pub maximum_frames: u64,
}
impl FfmpegLimits {
    fn validate(&self) -> Result<()> {
        if !self.executable.is_absolute()
            || self.fps_numerator == 0
            || self.fps_numerator > 120_000
            || self.fps_denominator == 0
            || self.fps_denominator > 1001
            || self.fps_numerator as u64 > 120 * self.fps_denominator as u64
            || self.fps_numerator < self.fps_denominator
            || self.maximum_frames == 0
            || self.maximum_frames > 10_000_000
            || !(64 * 1024 * 1024..=1024 * 1024 * 1024).contains(&self.sandbox.memory_bytes)
            || !(FFMPEG_PROFILE_TASK_RESERVATION..=16).contains(&self.sandbox.maximum_tasks)
            || !(1..=86_400).contains(&self.sandbox.cpu_seconds)
        {
            return Err(AnimationError::Budget("ffmpeg trusted limits".into()));
        }
        Ok(())
    }
    fn timestamp(&self, index: u64) -> Result<Duration> {
        let nanos = (index as u128)
            .checked_mul(self.fps_denominator as u128)
            .and_then(|v| v.checked_mul(1_000_000_000))
            .ok_or_else(|| failure("PTS overflow"))?
            / self.fps_numerator as u128;
        Ok(Duration::from_nanos(
            u64::try_from(nanos).map_err(|_| failure("PTS range"))?,
        ))
    }
}
struct Interrupt {
    cancelled: AtomicBool,
    child: Mutex<Option<SandboxCancel>>,
}
impl CodecInterrupt for Interrupt {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(child) = self
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            let _ = child.terminate();
        }
    }
}
/// One factory per playback session. The service reserves its declared physical
/// budget before open. Namespace/cgroup creation failure is terminal; never use
/// an ambient std::process fallback. Quota declaration is NOT a measured RSS cap.
pub struct FfmpegDecoderFactory {
    quota: QuotaGroup,
    limits: FfmpegLimits,
    interrupt: Arc<Interrupt>,
    opened: AtomicBool,
}
impl FfmpegDecoderFactory {
    pub fn new(quota: QuotaGroup, limits: FfmpegLimits) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            quota,
            limits,
            interrupt: Arc::new(Interrupt {
                cancelled: AtomicBool::new(false),
                child: Mutex::new(None),
            }),
            opened: AtomicBool::new(false),
        })
    }
    /// Snapshot the exact owned decoder cgroup while this factory has a child.
    /// A post-retirement caller gets an error, never a guessed zero-usage result.
    pub fn resource_usage(&self) -> std::io::Result<animation_sandbox::SandboxUsage> {
        self.interrupt
            .child
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .ok_or_else(|| std::io::Error::other("ffmpeg decoder has no active resource domain"))?
            .resource_usage()
    }
    fn arguments(&self, geometry: VideoGeometry) -> Result<Vec<String>> {
        let pixels = (geometry.width as usize)
            .checked_mul(geometry.height as usize)
            .ok_or_else(|| failure("geometry overflow"))?;
        if geometry.width == 0
            || geometry.height == 0
            || geometry.width > 8192
            || geometry.height > 8192
            || pixels > 4 * 1024 * 1024
        {
            return Err(AnimationError::Budget("ffmpeg output geometry".into()));
        }
        let pixel_format = match geometry.format {
            VideoPixelFormat::Gray8 => "gray",
            VideoPixelFormat::Rgb8 => "rgb24",
            VideoPixelFormat::Rgba8 => "rgba",
        };
        let filter = format!(
            "scale={}:{}:flags=bilinear,fps={}/{}",
            geometry.width, geometry.height, self.limits.fps_numerator, self.limits.fps_denominator
        );
        Ok([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-threads",
            "1",
            "-filter_threads",
            "1",
            "-filter_complex_threads",
            "1",
            "-max_alloc",
            "16777216",
            "-probesize",
            "1048576",
            "-analyzeduration",
            "2000000",
            "-protocol_whitelist",
            "pipe",
            "-format_whitelist",
            "mov,matroska,avi,mpegts,gif,apng,image2pipe,ppm_pipe,png_pipe,bmp_pipe,mjpeg",
            "-i",
            "pipe:0",
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-vf",
            &filter,
            "-pix_fmt",
            pixel_format,
            "-threads",
            "1",
            "-fps_mode",
            "cfr",
            "-frames:v",
            &self.limits.maximum_frames.to_string(),
            "-f",
            "rawvideo",
            "pipe:1",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect())
    }
}
impl VideoDecoderFactory for FfmpegDecoderFactory {
    fn budget(&self) -> CodecBudget {
        CodecBudget {
            // This factory adds two original-root credits for its separate
            // encoded-input writer and a host-side task margin. NativeVideo
            // adds two more for its reader/owner and lifecycle margin. The
            // full pre-spawn physical reservation is cgroup cap + 4.
            native_tasks: self.limits.sandbox.maximum_tasks as usize + 2,
            native_resident_bytes: self.limits.sandbox.memory_bytes as usize + 4 * 1024 * 1024,
            scratch_bytes: 128 * 1024,
        }
    }
    fn interrupt(&self) -> Arc<dyn CodecInterrupt> {
        self.interrupt.clone()
    }
    fn open(
        &self,
        input: Arc<VerifiedVideoInput>,
        geometry: VideoGeometry,
        stop: &StopToken,
    ) -> Result<Box<dyn VideoDecoder>> {
        if stop.is_stopped() || self.interrupt.cancelled.load(Ordering::Acquire) {
            return Err(failure("cancelled before open"));
        }
        if !self.quota.shares_root(&input.quota_group()) {
            return Err(AnimationError::PermissionDenied(
                "ffmpeg original root".into(),
            ));
        }
        if self.opened.swap(true, Ordering::AcqRel) {
            return Err(failure("factory already opened"));
        }
        let arguments = self.arguments(geometry)?;
        let writer_storage = self
            .quota
            .reserve_external_storage(65536)
            .map_err(|e| AnimationError::Budget(format!("ffmpeg writer: {e:?}")))?;
        let mut child = animation_sandbox::spawn_video_decoder(
            &self.limits.executable,
            &arguments,
            self.limits.sandbox,
        )?;
        let cancel = child.cancel_handle();
        *self
            .interrupt
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(cancel.clone());
        if self.interrupt.cancelled.load(Ordering::Acquire) || stop.is_stopped() {
            let _ = child.shutdown();
            return Err(failure("cancelled during open"));
        }
        let stdin = child
            .take_stdin()
            .ok_or_else(|| failure("missing owned stdin"))?;
        let stdout = child
            .take_stdout()
            .ok_or_else(|| failure("missing owned stdout"))?;
        let outcome = Arc::new(Mutex::new(None));
        let writer_outcome = outcome.clone();
        let wake = cancel.clone();
        let writer = owned_worker::spawn_owned(
            "ilium-video-encoded-write",
            WorkerKind::SynchronousIo,
            StopToken::default(),
            move || {
                let _ = wake.terminate();
            },
            move |writer_stop| {
                let _storage: StorageAdmission = writer_storage;
                ilium_platform::thread_priority::lower_current_thread(
                    ilium_platform::thread_priority::WorkerPriority::BelowNormal,
                );
                let result = write_input(stdin, &input, &writer_stop);
                *writer_outcome.lock().unwrap_or_else(|e| e.into_inner()) = Some(result.is_ok());
                if result.is_err() {
                    let _ = cancel.terminate();
                }
            },
        )?;
        Ok(Box::new(FfmpegDecoder {
            child: Some(child),
            stdout,
            writer,
            outcome,
            geometry,
            limits: self.limits.clone(),
            next_index: 0,
            closed: false,
            interrupt: self.interrupt.clone(),
        }))
    }
}
fn write_input(
    mut stdin: std::process::ChildStdin,
    input: &VerifiedVideoInput,
    stop: &StopToken,
) -> Result<()> {
    let mut scratch = [0u8; 65536];
    let mut offset = 0u64;
    loop {
        if stop.is_stopped() {
            return Err(failure("writer cancelled"));
        }
        let read = input.read_at(offset, &mut scratch, stop)?;
        if read == 0 {
            return Ok(());
        }
        stdin.write_all(&scratch[..read])?;
        offset = offset
            .checked_add(read as u64)
            .ok_or_else(|| failure("input offset overflow"))?;
    }
}
struct FfmpegDecoder {
    child: Option<SandboxChild>,
    stdout: ChildStdout,
    writer: OwnedWorker,
    outcome: Arc<Mutex<Option<bool>>>,
    geometry: VideoGeometry,
    limits: FfmpegLimits,
    next_index: u64,
    closed: bool,
    interrupt: Arc<Interrupt>,
}
impl FfmpegDecoder {
    fn retire(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.interrupt.cancel();
        self.writer.ticket().cancel();
        let result = self.child.as_mut().map(SandboxChild::shutdown).transpose();
        // Deadlines transfer no custody: remain here, holding service admissions,
        // until the actual input worker exits. A misbehaving broker reader can
        // therefore leave retirement pending, never appear safely closed.
        while self
            .writer
            .ticket()
            .join_until(Instant::now() + Duration::from_secs(1))
            .is_err()
        {}
        self.child.take();
        self.interrupt
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.closed = true;
        result.map(|_| ()).map_err(AnimationError::from)
    }
}
impl VideoDecoder for FfmpegDecoder {
    fn info(&self) -> VideoInfo {
        VideoInfo {
            geometry: self.geometry,
            duration: None,
            seekable: false,
        }
    }
    fn fill_frame(&mut self, output: &mut [u8], stop: &StopToken) -> Result<Option<FrameStamp>> {
        if self.closed || stop.is_stopped() {
            return Err(failure("decoder stopped"));
        }
        let channels = match self.geometry.format {
            VideoPixelFormat::Gray8 => 1,
            VideoPixelFormat::Rgb8 => 3,
            VideoPixelFormat::Rgba8 => 4,
        };
        let expected = self.geometry.width as usize * self.geometry.height as usize * channels;
        if output.len() != expected {
            return Err(failure("fixed output plane shape mismatch"));
        }
        if self.next_index >= self.limits.maximum_frames {
            return Ok(None);
        }
        let mut filled = 0;
        while filled < output.len() {
            match self.stdout.read(&mut output[filled..]) {
                Ok(0) if filled == 0 => {
                    let status = self
                        .child
                        .as_mut()
                        .ok_or_else(|| failure("missing child"))?
                        .wait_for_exit()?;
                    if !status.success() {
                        return Err(failure("ffmpeg exited unsuccessfully"));
                    }
                    if *self.outcome.lock().unwrap_or_else(|e| e.into_inner()) == Some(false) {
                        return Err(failure("encoded writer failed"));
                    }
                    return Ok(None);
                }
                Ok(0) => return Err(failure("truncated raw frame")),
                Ok(read) => filled += read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        let pts = self.limits.timestamp(self.next_index)?;
        let end = self.limits.timestamp(self.next_index + 1)?;
        self.next_index += 1;
        Ok(Some(FrameStamp {
            pts,
            duration: end.saturating_sub(pts),
        }))
    }
    fn close_and_wait(&mut self, _stop: &StopToken) -> Result<()> {
        self.retire()
    }
}
impl Drop for FfmpegDecoder {
    fn drop(&mut self) {
        let _ = self.retire();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_video::{
        CloseState, NativeVideo, VideoAuthority, VideoAuthorization, VideoLimits, VideoOperation,
        VideoPhase,
    };
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaLimits, ShutdownMode,
    };
    fn limits() -> FfmpegLimits {
        FfmpegLimits {
            executable: PathBuf::from("/usr/bin/ffmpeg"),
            sandbox: SandboxLimits {
                memory_bytes: 128 * 1024 * 1024,
                maximum_tasks: FFMPEG_PROFILE_TASK_RESERVATION,
                cpu_seconds: 30,
            },
            fps_numerator: 25,
            fps_denominator: 1,
            maximum_frames: 25,
        }
    }
    fn root() -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 4,
            service_jobs: 0,
            input_bytes: 65536,
            result_bytes: 65536,
            worker_threads: 16,
            worker_bytes: 256 * 1024 * 1024,
        })
    }
    #[test]
    fn rational_output_timestamps_and_fixed_pipe_arguments_are_finite() {
        let mut configuration = limits();
        configuration.fps_numerator = 30000;
        configuration.fps_denominator = 1001;
        let factory = FfmpegDecoderFactory::new(root(), configuration).unwrap();
        assert_eq!(factory.budget().native_tasks, 13);
        assert_eq!(factory.budget().native_resident_bytes, 132 * 1024 * 1024);
        assert_eq!(
            factory.limits.timestamp(30000).unwrap(),
            Duration::from_secs(1001)
        );
        let geometry = VideoGeometry {
            width: 2,
            height: 2,
            format: VideoPixelFormat::Rgba8,
        };
        let arguments = factory.arguments(geometry).unwrap();
        assert_eq!(
            arguments.iter().filter(|s| s.as_str() == "pipe:0").count(),
            1
        );
        assert_eq!(
            arguments.iter().filter(|s| s.as_str() == "pipe:1").count(),
            1
        );
        assert!(arguments
            .windows(2)
            .any(|pair| pair == ["-protocol_whitelist", "pipe"]));
        assert!(arguments.contains(&"scale=2:2:flags=bilinear,fps=30000/1001".into()));
        assert!(!arguments
            .iter()
            .any(|s| s.contains("http") || s.contains("file:")));
        assert!(factory
            .arguments(VideoGeometry {
                width: 0,
                ..geometry
            })
            .is_err());
        let mut invalid = limits();
        invalid.fps_denominator = 0;
        assert!(FfmpegDecoderFactory::new(root(), invalid).is_err());
        let mut insufficient_tasks = limits();
        insufficient_tasks.sandbox.maximum_tasks = FFMPEG_PROFILE_TASK_RESERVATION - 1;
        assert!(matches!(
            FfmpegDecoderFactory::new(root(), insufficient_tasks),
            Err(AnimationError::Budget(_))
        ));
    }
    struct Allow;
    impl VideoAuthorization for Allow {
        fn check(&self, _: &VideoAuthority, id: u64, _: VideoOperation) -> Result<()> {
            if id != 1 {
                return Err(AnimationError::PermissionDenied("fixture id".into()));
            }
            Ok(())
        }
    }
    /// Real codec/OS qualification: requires installed ffmpeg, bwrap and a
    /// delegated private cgroup. Failure is a qualification failure, never skip.
    #[test]
    #[ignore = "requires delegated Linux codec sandbox and actual ffmpeg"]
    fn actual_ffmpeg_decodes_verified_finite_ppm_and_retires_owned_domain() {
        let quota = root();
        let empty = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        // A real admitted execution bank is required even when all video work
        // uses separately owned native workers. Its existing charge is baseline.
        let bank = LaneConfig {
            threads: 1,
            queue_slots: 2,
            priority: None,
            resident_bytes_per_thread: 4 * 1024 * 1024,
        };
        let mut execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: bank,
                io: empty,
                service: empty,
            },
        )
        .unwrap();
        assert_eq!(quota.snapshot().worker_threads, 1);
        let client = execution
            .client(ClientLimits {
                jobs: 2,
                service_jobs: 0,
                input_bytes: 32768,
                result_bytes: 32768,
            })
            .unwrap();
        let auth = Arc::new(Allow);
        let mut ppm = b"P6\n2 2\n255\n".to_vec();
        ppm.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
        let input = VerifiedVideoInput::from_encoded(
            &client,
            auth.as_ref(),
            VideoAuthority {
                package_digest: "a".repeat(64),
                instance_id: 1,
                plan_revision: 1,
                authorization_epoch: 1,
            },
            1,
            &ppm,
            4096,
            &StopToken::default(),
        )
        .unwrap();
        let geometry = VideoGeometry {
            width: 2,
            height: 2,
            format: VideoPixelFormat::Rgba8,
        };
        // The original root has 16 task credits. One bank worker plus the
        // codec's cap + 4 reservation exactly fills it; a competing credit
        // must therefore reject before FFmpeg or its writer can be spawned.
        let competing = quota.reserve_external_worker(1, 1024 * 1024).unwrap();
        assert_eq!(quota.snapshot().worker_threads, 2);
        let rejected_factory =
            Arc::new(FfmpegDecoderFactory::new(quota.clone(), limits()).unwrap());
        assert!(matches!(
            NativeVideo::start(
                client.clone(),
                Arc::clone(&input),
                auth.clone(),
                rejected_factory.clone(),
                VideoLimits {
                    geometry,
                    queue_frames: 2,
                    lookahead: Duration::from_millis(100),
                    maximum_pts: Duration::from_secs(2),
                    maximum_frames: 25,
                    maximum_frame_duration: Duration::from_secs(1),
                },
                StopToken::default(),
            ),
            Err(AnimationError::Budget(_))
        ));
        assert!(rejected_factory.resource_usage().is_err());
        assert_eq!(quota.snapshot().worker_threads, 2);
        drop(competing);
        assert_eq!(quota.snapshot().worker_threads, 1);
        let factory = Arc::new(FfmpegDecoderFactory::new(quota.clone(), limits()).unwrap());
        let video = NativeVideo::start(
            client.clone(),
            input,
            auth,
            factory.clone(),
            VideoLimits {
                geometry,
                queue_frames: 2,
                lookahead: Duration::from_millis(100),
                maximum_pts: Duration::from_secs(2),
                maximum_frames: 25,
                maximum_frame_duration: Duration::from_secs(1),
            },
            StopToken::default(),
        )
        .unwrap();
        assert_eq!(quota.snapshot().worker_threads, 16);
        let deadline = Instant::now() + Duration::from_secs(10);
        let frame = loop {
            if let Some(frame) = video.latest_at(Duration::ZERO).unwrap() {
                break frame;
            }
            let status = video.status().unwrap();
            assert!(status.failure.is_none(), "real decoder failure: {status:?}");
            assert!(
                Instant::now() < deadline,
                "real decoder produced no frame: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(
            frame.bytes(),
            &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]
        );
        // Let the finite FFmpeg pipeline reach natural EOF under the same
        // original deadline before taking final kernel counters. NativeVideo
        // retains the latest frame and the cgroup until explicit close.
        loop {
            let status = video.status().unwrap();
            assert!(status.failure.is_none(), "real decoder failure: {status:?}");
            if status.phase == VideoPhase::Ended {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "real decoder did not reach EOF: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        // The owned cgroup, not the declared quota, supplies these counters.
        let usage = factory.resource_usage().unwrap();
        assert_eq!(usage.maximum_tasks, FFMPEG_PROFILE_TASK_RESERVATION);
        assert!(usage.current_tasks <= usage.maximum_tasks);
        if let Some(peak) = usage.peak_tasks {
            assert!(
                (1..=usage.maximum_tasks).contains(&peak),
                "unexpected decoder task peak: {usage:?}"
            );
        } // Linux 5.15 has no pids.peak; pids.max/events remain authoritative.
        assert_eq!(usage.task_limit_events, 0, "task denial: {usage:?}");
        assert_eq!(usage.maximum_memory_bytes, limits().sandbox.memory_bytes);
        assert!(usage.current_memory_bytes <= usage.maximum_memory_bytes);
        if let Some(peak_memory_bytes) = usage.peak_memory_bytes {
            assert!(peak_memory_bytes <= usage.maximum_memory_bytes);
        } // Linux 5.15 has no memory.peak; never fabricate a lifetime high-water mark.
        eprintln!("qualified finite-PPM decoder cgroup: {usage:?}");
        assert_eq!(quota.snapshot().worker_threads, 16);
        assert_eq!(frame.stamp.pts, Duration::ZERO);
        assert_eq!(frame.stamp.duration, Duration::from_millis(40));
        assert!(video.seek(Duration::ZERO).is_err());
        assert_eq!(
            video.close_until(Instant::now() + Duration::from_secs(5)),
            CloseState::Joined
        );
        assert_eq!(quota.snapshot().worker_threads, 16);
        assert!(factory.resource_usage().is_err());
        drop(frame);
        drop(video);
        assert_eq!(quota.snapshot().worker_threads, 1);
        drop(client);
        execution.request_shutdown(ShutdownMode::Cancel);
        let _ = execution.join_until_background(Instant::now() + Duration::from_secs(2));
    }
}
