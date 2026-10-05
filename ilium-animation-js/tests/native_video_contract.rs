//! Synthetic, finite uncompressed fixture codec. It tests the real admitted
//! service/worker transport, not ffmpeg availability or production containment.
use ilium_animation_js::{
    error::{AnimationError, Result},
    native_video::*,
};
use ilium_execution::{
    Client, ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
    ShutdownMode,
};
use ilium_platform::owned_worker::StopToken;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};
fn authority() -> VideoAuthority {
    VideoAuthority {
        package_digest: "a".repeat(64),
        instance_id: 8,
        plan_revision: 4,
        authorization_epoch: 9,
    }
}
struct Authorization {
    allowed: AtomicBool,
}
impl VideoAuthorization for Authorization {
    fn check(&self, bound: &VideoAuthority, id: u64, _operation: VideoOperation) -> Result<()> {
        if !self.allowed.load(Ordering::Acquire) || bound != &authority() || id != 7 {
            return Err(AnimationError::PermissionDenied(
                "fixture grant rejected".into(),
            ));
        }
        Ok(())
    }
}
struct Host {
    execution: Execution,
    client: Client,
    quota: QuotaGroup,
}
impl Host {
    fn new() -> Self {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 8,
            jobs: 8,
            service_jobs: 0,
            input_bytes: 128 * 1024,
            result_bytes: 128 * 1024,
            worker_threads: 16,
            worker_bytes: 64 * 1024 * 1024,
        });
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
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: bank,
                io: empty,
                service: empty,
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 4,
                service_jobs: 0,
                input_bytes: 64 * 1024,
                result_bytes: 64 * 1024,
            })
            .unwrap();
        Self {
            execution,
            client,
            quota,
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.execution.request_shutdown(ShutdownMode::Cancel);
        let _ = self
            .execution
            .join_until_background(Instant::now() + Duration::from_secs(2));
    }
}
#[derive(Default)]
struct ProbeState {
    reads: u64,
    opened: bool,
    closing: bool,
    closed: bool,
    read_gate: bool,
    close_gate: bool,
}
struct Probe {
    state: Mutex<ProbeState>,
    changed: Condvar,
}
impl Probe {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ProbeState {
                read_gate: true,
                close_gate: true,
                ..ProbeState::default()
            }),
            changed: Condvar::new(),
        })
    }
    fn wait(&self, predicate: impl Fn(&ProbeState) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut state = self.state.lock().unwrap();
        while !predicate(&state) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "fixture event deadline");
            state = self.changed.wait_timeout(state, remaining).unwrap().0;
        }
    }
}
struct Interrupt(Arc<Probe>);
impl CodecInterrupt for Interrupt {
    fn cancel(&self) {
        self.0.changed.notify_all();
    }
}
struct Factory {
    probe: Arc<Probe>,
    invalid_time: bool,
    seekable: bool,
    wrong_geometry: bool,
}
impl VideoDecoderFactory for Factory {
    fn budget(&self) -> CodecBudget {
        CodecBudget {
            native_tasks: 0,
            native_resident_bytes: 0,
            scratch_bytes: 4096,
        }
    }
    fn interrupt(&self) -> Arc<dyn CodecInterrupt> {
        Arc::new(Interrupt(Arc::clone(&self.probe)))
    }
    fn open(
        &self,
        input: Arc<VerifiedVideoInput>,
        geometry: VideoGeometry,
        _stop: &StopToken,
    ) -> Result<Box<dyn VideoDecoder>> {
        let bytes = input.encoded()?;
        if bytes.len() < 12 || &bytes[..4] != b"FV01" {
            return Err(AnimationError::Runtime("invalid fixture header".into()));
        }
        let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        if count == 0
            || size == 0
            || count.checked_mul(size).and_then(|n| n.checked_add(12)) != Some(bytes.len())
        {
            return Err(AnimationError::Runtime(
                "fixture plane inventory mismatch".into(),
            ));
        }
        let mut state = self.probe.state.lock().unwrap();
        state.opened = true;
        self.probe.changed.notify_all();
        drop(state);
        let info = VideoInfo {
            geometry: if self.wrong_geometry {
                VideoGeometry {
                    width: 1,
                    ..geometry
                }
            } else {
                geometry
            },
            duration: Some(Duration::from_millis(count as u64 * 100)),
            seekable: self.seekable,
        };
        Ok(Box::new(Decoder {
            input,
            probe: Arc::clone(&self.probe),
            index: 0,
            count,
            size,
            info,
            invalid_time: self.invalid_time,
        }))
    }
}
struct Decoder {
    input: Arc<VerifiedVideoInput>,
    probe: Arc<Probe>,
    index: usize,
    count: usize,
    size: usize,
    info: VideoInfo,
    invalid_time: bool,
}
impl VideoDecoder for Decoder {
    fn info(&self) -> VideoInfo {
        self.info
    }
    fn fill_frame(&mut self, output: &mut [u8], stop: &StopToken) -> Result<Option<FrameStamp>> {
        let mut state = self.probe.state.lock().unwrap();
        while !state.read_gate && !stop.is_stopped() {
            state = self
                .probe
                .changed
                .wait_timeout(state, Duration::from_millis(20))
                .unwrap()
                .0;
        }
        if stop.is_stopped() {
            return Err(AnimationError::Runtime("fixture cancelled".into()));
        }
        if self.index >= self.count {
            return Ok(None);
        }
        if output.len() != self.size {
            return Err(AnimationError::Runtime("fixture output shape".into()));
        }
        let start = 12 + self.index * self.size;
        output.copy_from_slice(&self.input.encoded()?[start..start + self.size]);
        let pts = Duration::from_millis(self.index as u64 * 100);
        self.index += 1;
        state.reads += 1;
        self.probe.changed.notify_all();
        Ok(Some(FrameStamp {
            pts,
            duration: if self.invalid_time {
                Duration::ZERO
            } else {
                Duration::from_millis(100)
            },
        }))
    }
    fn seek(&mut self, position: Duration, _stop: &StopToken) -> Result<()> {
        if !self.info.seekable {
            return Err(AnimationError::Runtime("fixture unseekable".into()));
        }
        self.index = (position.as_millis() / 100) as usize;
        Ok(())
    }
    fn close_and_wait(&mut self, _stop: &StopToken) -> Result<()> {
        let mut state = self.probe.state.lock().unwrap();
        state.closing = true;
        self.probe.changed.notify_all();
        while !state.close_gate {
            state = self.probe.changed.wait(state).unwrap();
        }
        state.closed = true;
        self.probe.changed.notify_all();
        Ok(())
    }
}
fn fixture(count: usize) -> Vec<u8> {
    let mut bytes = b"FV01".to_vec();
    bytes.extend_from_slice(&(count as u32).to_le_bytes());
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    for index in 0..count {
        bytes.extend_from_slice(&[index as u8; 16]);
    }
    bytes
}
fn limits() -> VideoLimits {
    VideoLimits {
        geometry: VideoGeometry {
            width: 2,
            height: 2,
            format: VideoPixelFormat::Rgba8,
        },
        queue_frames: 3,
        lookahead: Duration::from_millis(250),
        maximum_pts: Duration::from_secs(60),
        maximum_frames: 10000,
        maximum_frame_duration: Duration::from_secs(1),
    }
}
fn setup(
    host: &Host,
    probe: Arc<Probe>,
    factory: impl FnOnce(Arc<Probe>) -> Factory,
) -> (NativeVideo, Arc<Authorization>) {
    let auth = Arc::new(Authorization {
        allowed: AtomicBool::new(true),
    });
    let input = VerifiedVideoInput::from_encoded(
        &host.client,
        auth.as_ref(),
        authority(),
        7,
        &fixture(200),
        4096,
        &StopToken::default(),
    )
    .unwrap();
    let service = NativeVideo::start(
        host.client.clone(),
        input,
        auth.clone(),
        Arc::new(factory(probe)),
        limits(),
        StopToken::default(),
    )
    .unwrap();
    (service, auth)
}
fn ordinary(probe: Arc<Probe>) -> Factory {
    Factory {
        probe,
        invalid_time: false,
        seekable: true,
        wrong_geometry: false,
    }
}
/// Bounded test synchronization with the service's scalar state; no production
/// source polling/fetcher or user-owned process is involved.
fn until(service: &NativeVideo, predicate: impl Fn(VideoStatus) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate(service.status().unwrap()) {
        assert!(
            Instant::now() < deadline,
            "video state deadline: {:?}",
            service.status().unwrap()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn bounded_backlog_coalesces_forward_targets_without_delivering_future_frames() {
    let host = Host::new();
    let probe = Probe::new();
    let (service, _) = setup(&host, probe, ordinary);
    until(&service, |status| status.decoded_frames >= 4);
    let initial = service.latest_at(Duration::ZERO).unwrap().unwrap();
    assert_eq!(initial.stamp.pts, Duration::ZERO);
    assert_eq!(initial.bytes(), &[0; 16]);
    assert!(service.status().unwrap().queued_frames <= 3);
    service.latest_at(Duration::from_secs(10)).unwrap();
    until(&service, |status| status.decoded_frames >= 104);
    let latest = service.latest_at(Duration::from_secs(10)).unwrap().unwrap();
    assert_eq!(latest.stamp.pts, Duration::from_secs(10));
    assert_eq!(latest.bytes(), &[100; 16]);
    assert!(service.status().unwrap().queued_frames <= 3);
    assert!(service.status().unwrap().discarded_frames >= 90);
    assert!(service.latest_at(Duration::ZERO).is_err());
    assert_eq!(
        service.close_until(Instant::now() + Duration::from_secs(2)),
        CloseState::Joined
    );
}
#[test]
fn pause_freezes_effective_time_and_seek_invalidates_prior_generation() {
    let host = Host::new();
    let probe = Probe::new();
    let (service, _) = setup(&host, Arc::clone(&probe), ordinary);
    until(&service, |status| status.decoded_frames >= 4);
    let initial = service.latest_at(Duration::ZERO).unwrap().unwrap();
    service.pause().unwrap();
    let frozen = service.latest_at(Duration::from_secs(5)).unwrap().unwrap();
    assert_eq!(frozen.stamp.pts, initial.stamp.pts);
    assert_eq!(service.status().unwrap().target, Duration::ZERO);
    let generation = service.seek(Duration::from_secs(2)).unwrap();
    assert!(service.latest_at(Duration::from_secs(2)).unwrap().is_none());
    service.resume().unwrap();
    until(&service, |status| status.decoded_frames >= 8);
    let sought = service.latest_at(Duration::from_secs(2)).unwrap().unwrap();
    assert_eq!(sought.generation, generation);
    assert_eq!(sought.stamp.pts, Duration::from_secs(2));
    assert_eq!(sought.bytes(), &[20; 16]);
    assert_eq!(initial.bytes(), &[0; 16]);
    assert!(service.seek(Duration::from_secs(30)).is_err());
    assert_eq!(
        service.close_until(Instant::now() + Duration::from_secs(2)),
        CloseState::Joined
    );
}
#[test]
fn revocation_blocks_snapshot_and_native_continuation_before_publication() {
    let host = Host::new();
    let probe = Probe::new();
    let (service, auth) = setup(&host, probe, ordinary);
    until(&service, |status| status.decoded_frames >= 4);
    auth.allowed.store(false, Ordering::Release);
    assert!(service.latest_at(Duration::from_secs(1)).is_err());
    service.cancel();
    assert_eq!(
        service.close_until(Instant::now() + Duration::from_secs(2)),
        CloseState::Joined
    );
}
#[test]
fn actual_thread_retirement_keeps_original_physical_and_client_credit_after_timeout() {
    let host = Host::new();
    let baseline = host.quota.snapshot();
    let probe = Probe::new();
    probe.state.lock().unwrap().close_gate = false;
    let (service, _) = setup(&host, Arc::clone(&probe), ordinary);
    until(&service, |status| status.decoded_frames >= 4);
    let retained = service.latest_at(Duration::ZERO).unwrap().unwrap();
    assert_eq!(
        service.close_until(Instant::now() + Duration::from_millis(20)),
        CloseState::Retiring
    );
    probe.wait(|state| state.closing);
    assert!(host.quota.snapshot().worker_threads > baseline.worker_threads);
    assert!(host.quota.snapshot().jobs > baseline.jobs);
    {
        let mut state = probe.state.lock().unwrap();
        state.close_gate = true;
        probe.changed.notify_all();
    }
    assert_eq!(
        service.close_until(Instant::now() + Duration::from_secs(2)),
        CloseState::Joined
    );
    drop(service);
    assert_eq!(
        host.quota.snapshot().worker_threads,
        baseline.worker_threads
    );
    assert_eq!(host.quota.snapshot().jobs, baseline.jobs);
    assert!(host.quota.snapshot().worker_bytes > baseline.worker_bytes);
    assert_eq!(retained.bytes(), &[0; 16]);
    drop(retained);
    assert_eq!(host.quota.snapshot().worker_bytes, baseline.worker_bytes);
}
#[test]
fn blocking_decode_is_owned_and_cancel_wakes_only_its_fixture_domain() {
    let host = Host::new();
    let probe = Probe::new();
    probe.state.lock().unwrap().read_gate = false;
    let (service, _) = setup(&host, Arc::clone(&probe), ordinary);
    probe.wait(|state| state.opened);
    assert_eq!(
        service.close_until(Instant::now() + Duration::from_secs(2)),
        CloseState::Joined
    );
    probe.wait(|state| state.closed);
    assert_eq!(probe.state.lock().unwrap().reads, 0);
}
#[test]
fn malformed_timing_geometry_and_input_inventory_fail_without_publishing() {
    for (invalid_time, wrong_geometry) in [(true, false), (false, true)] {
        let host = Host::new();
        let probe = Probe::new();
        let (service, _) = setup(&host, probe, |probe| Factory {
            probe,
            invalid_time,
            seekable: true,
            wrong_geometry,
        });
        until(&service, |status| status.phase == VideoPhase::Failed);
        assert_eq!(
            service.status().unwrap().failure,
            Some(VideoFailure::Protocol)
        );
        assert!(service.latest_at(Duration::ZERO).unwrap().is_none());
        assert_eq!(
            service.close_until(Instant::now() + Duration::from_secs(2)),
            CloseState::Joined
        );
    }
    let host = Host::new();
    let auth = Arc::new(Authorization {
        allowed: AtomicBool::new(true),
    });
    let input = VerifiedVideoInput::from_encoded(
        &host.client,
        auth.as_ref(),
        authority(),
        7,
        b"malformed",
        4096,
        &StopToken::default(),
    )
    .unwrap();
    let service = NativeVideo::start(
        host.client.clone(),
        input,
        auth,
        Arc::new(ordinary(Probe::new())),
        limits(),
        StopToken::default(),
    )
    .unwrap();
    until(&service, |status| status.phase == VideoPhase::Failed);
    assert_eq!(service.status().unwrap().failure, Some(VideoFailure::Codec));
    assert_eq!(
        service.close_until(Instant::now() + Duration::from_secs(2)),
        CloseState::Joined
    );
}
#[test]
fn input_authority_quota_scope_caps_and_unsupported_seek_are_not_bypassed() {
    let host = Host::new();
    let denied = Authorization {
        allowed: AtomicBool::new(false),
    };
    let baseline = host.quota.snapshot();
    assert!(VerifiedVideoInput::from_encoded(
        &host.client,
        &denied,
        authority(),
        7,
        &fixture(2),
        4096,
        &StopToken::default()
    )
    .is_err());
    assert_eq!(host.quota.snapshot().worker_bytes, baseline.worker_bytes);
    let auth = Arc::new(Authorization {
        allowed: AtomicBool::new(true),
    });
    assert!(VerifiedVideoInput::from_encoded(
        &host.client,
        auth.as_ref(),
        authority(),
        7,
        &fixture(2),
        8,
        &StopToken::default()
    )
    .is_err());
    let input = VerifiedVideoInput::from_encoded(
        &host.client,
        auth.as_ref(),
        authority(),
        7,
        &fixture(2),
        4096,
        &StopToken::default(),
    )
    .unwrap();
    let other = Host::new();
    assert!(NativeVideo::start(
        other.client.clone(),
        input,
        auth,
        Arc::new(ordinary(Probe::new())),
        limits(),
        StopToken::default()
    )
    .is_err());
    let (service, _) = setup(&host, Probe::new(), |probe| Factory {
        seekable: false,
        ..ordinary(probe)
    });
    until(&service, |status| status.info.is_some());
    assert!(service.seek(Duration::from_secs(1)).is_err());
    assert_eq!(
        service.close_until(Instant::now() + Duration::from_secs(2)),
        CloseState::Joined
    );
}

/// This fixture owns its finite bytes under the original root; it supplies no
/// path, URL or process authority to the service or decoder.
struct FiniteResource {
    quota: QuotaGroup,
    bytes: Vec<u8>,
    cancelled: AtomicBool,
    _storage: ilium_execution::StorageAdmission,
}
impl BrokerVideoResource for FiniteResource {
    fn quota_group(&self) -> QuotaGroup {
        self.quota.clone()
    }
    fn maximum_bytes(&self) -> usize {
        self.bytes.len()
    }
    fn read_at(&self, offset: u64, output: &mut [u8], stop: &StopToken) -> Result<usize> {
        if stop.is_stopped() || self.cancelled.load(Ordering::Acquire) {
            return Err(AnimationError::Runtime("fixture resource cancelled".into()));
        }
        let offset = usize::try_from(offset).unwrap();
        let length = output.len().min(self.bytes.len().saturating_sub(offset));
        output[..length].copy_from_slice(&self.bytes[offset..offset + length]);
        Ok(length)
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
#[test]
fn finite_resources_clip_reads_and_preserve_original_root_custody() {
    let host = Host::new();
    let auth = Authorization {
        allowed: AtomicBool::new(true),
    };
    let baseline = host.quota.snapshot().worker_bytes;
    let resource = Arc::new(FiniteResource {
        quota: host.quota.clone(),
        bytes: vec![10, 20, 30, 40],
        cancelled: AtomicBool::new(false),
        _storage: host.quota.reserve_external_storage(4).unwrap(),
    });
    let input = VerifiedVideoInput::from_resource(
        &host.client,
        &auth,
        authority(),
        7,
        resource.clone(),
        &StopToken::default(),
    )
    .unwrap();
    assert!(input.encoded().is_err());
    let mut output = [99; 8];
    assert_eq!(
        input
            .read_at(2, &mut output, &StopToken::default())
            .unwrap(),
        2
    );
    assert_eq!(output, [30, 40, 99, 99, 99, 99, 99, 99]);
    assert_eq!(
        input
            .read_at(4, &mut output, &StopToken::default())
            .unwrap(),
        0
    );
    let other = Host::new();
    assert!(VerifiedVideoInput::from_resource(
        &other.client,
        &auth,
        authority(),
        7,
        resource.clone(),
        &StopToken::default(),
    )
    .is_err());
    drop(resource);
    assert!(host.quota.snapshot().worker_bytes >= baseline + 516);
    drop(input);
    assert_eq!(host.quota.snapshot().worker_bytes, baseline);
}

#[test]
fn recorded_wait_reads_final_due_pixels_from_the_real_decoder_service() {
    let host = Host::new();
    let probe = Probe::new();
    let (service, _) = setup(&host, probe, ordinary);
    until(&service, |status| status.decoded_frames >= 4);
    let outcome = (|| -> Result<()> {
        for (target_ms, expected_ms, expected_byte) in [(250, 200, 2), (950, 900, 9)] {
            let frame = service
                .wait_recorded_at(
                    Duration::from_millis(target_ms),
                    Instant::now() + Duration::from_secs(3),
                    &StopToken::default(),
                )?
                .ok_or_else(|| AnimationError::Runtime("fixture recorded frame absent".into()))?;
            assert_eq!(frame.stamp.pts, Duration::from_millis(expected_ms));
            assert_eq!(frame.bytes(), &[expected_byte; 16]);
        }
        assert!(
            service
                .wait_recorded_at(
                    Duration::ZERO,
                    Instant::now() + Duration::from_secs(1),
                    &StopToken::default(),
                )
                .is_err(),
            "recorded backwards motion requires an explicit seek"
        );
        Ok(())
    })();
    assert_eq!(
        service.close_until(Instant::now() + Duration::from_secs(3)),
        CloseState::Joined
    );
    outcome.expect("real admitted decoder must finalize the latest due pixels");
    assert!(
        service
            .wait_recorded_at(
                Duration::from_millis(950),
                Instant::now() + Duration::from_secs(1),
                &StopToken::default(),
            )
            .is_err(),
        "a retired decoder must not supply recorded pixels"
    );
}

#[test]
fn recorded_wait_rejects_caller_stop_without_releasing_decoder_custody() {
    let host = Host::new();
    let probe = Probe::new();
    let (service, _) = setup(&host, probe, ordinary);
    let stop = StopToken::default();
    stop.stop();
    let denied = service
        .wait_recorded_at(
            Duration::ZERO,
            Instant::now() + Duration::from_secs(1),
            &stop,
        )
        .is_err();
    assert_eq!(
        service.close_until(Instant::now() + Duration::from_secs(3)),
        CloseState::Joined
    );
    assert!(denied, "caller cancellation must deny recorded delivery");
}
