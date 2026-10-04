//! Broker-owned video service, independent of ambient source discovery/fetchers.
//! No paths, URLs, commands, filesystem or subprocess primitives are accepted.
//! A concrete codec factory must own its native handles and hard resource domain;
//! the existing ambient ffmpeg factory is not wired here because it permits raw
//! URL/file acquisition. This service supplies admission, timing, cancellation,
//! bounded backlog and immutable frame custody around an injected native codec.
use crate::error::{AnimationError, Result};
use ilium_execution::{Client, JobCost, QuotaGroup, Retained, StorageAdmission, WorkerAdmission};
use ilium_platform::owned_worker::{self, OwnedWorker, StopToken, WorkerExit};
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
fn failure(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("native video: {message}"))
}
fn stopped(stop: &StopToken) -> Result<()> {
    if stop.is_stopped() {
        Err(failure("cancelled"))
    } else {
        Ok(())
    }
}
fn storage(quota: &QuotaGroup, bytes: usize) -> Result<StorageAdmission> {
    quota
        .reserve_external_storage(bytes.max(1))
        .map_err(|error| AnimationError::Budget(format!("video storage admission: {error:?}")))
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoAuthority {
    pub package_digest: String,
    pub instance_id: u64,
    pub plan_revision: u64,
    pub authorization_epoch: u64,
}
impl VideoAuthority {
    fn validate(&self) -> Result<()> {
        if self.package_digest.len() != 64
            || !self.package_digest.bytes().all(|c| c.is_ascii_hexdigit())
            || self.instance_id == 0
        {
            return Err(AnimationError::PermissionDenied(
                "invalid native video principal".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoOperation {
    Acquire,
    Open,
    Deliver,
    Status,
    Pause,
    Resume,
    Seek,
}
/// Trusted, nonblocking owner callback. Recheck current grant/epoch at each
/// effect/data-delivery boundary; codec/input identifiers cannot choose grants.
pub trait VideoAuthorization: Send + Sync {
    fn check(
        &self,
        authority: &VideoAuthority,
        resource_id: u64,
        operation: VideoOperation,
    ) -> Result<()>;
}
/// Parent-verified pinned resource or finite acquisition subscription. Its own
/// allocations/FDs already carry this root's custody, and cancel affects only
/// this subscription. Implement all OS access in the platform adapter.
pub trait BrokerVideoResource: Send + Sync {
    fn quota_group(&self) -> QuotaGroup;
    fn maximum_bytes(&self) -> usize;
    fn read_at(&self, offset: u64, output: &mut [u8], stop: &StopToken) -> Result<usize>;
    fn cancel(&self);
}
/// Immutable finite encoded input, admitted before copying from broker bytes.
/// Native pinned/stream factories may use a separate broker-owned resource
/// registry keyed by resource_id, never a script-authored pathname or URL.
pub struct VerifiedVideoInput {
    authority: VideoAuthority,
    resource_id: u64,
    encoded: Vec<u8>,
    maximum_bytes: usize,
    resource: Option<Arc<dyn BrokerVideoResource>>,
    root: QuotaGroup,
    _storage: StorageAdmission,
}
impl VerifiedVideoInput {
    pub fn from_encoded(
        client: &Client,
        authorization: &dyn VideoAuthorization,
        authority: VideoAuthority,
        resource_id: u64,
        encoded: &[u8],
        maximum_bytes: usize,
        stop: &StopToken,
    ) -> Result<Arc<Self>> {
        stopped(stop)?;
        authority.validate()?;
        if resource_id == 0
            || maximum_bytes == 0
            || maximum_bytes > 256 * 1024 * 1024
            || encoded.is_empty()
            || encoded.len() > maximum_bytes
        {
            return Err(AnimationError::Budget("native video encoded input".into()));
        }
        authorization.check(&authority, resource_id, VideoOperation::Acquire)?;
        let root = client.quota_group();
        let admission = storage(&root, encoded.len() + 512)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(encoded.len())
            .map_err(|_| failure("encoded input allocation"))?;
        for chunk in encoded.chunks(65536) {
            stopped(stop)?;
            bytes.extend_from_slice(chunk);
        }
        authorization.check(&authority, resource_id, VideoOperation::Acquire)?;
        Ok(Arc::new(Self {
            authority,
            resource_id,
            maximum_bytes: bytes.len(),
            encoded: bytes,
            resource: None,
            root,
            _storage: admission,
        }))
    }
    pub fn from_resource(
        client: &Client,
        authorization: &dyn VideoAuthorization,
        authority: VideoAuthority,
        resource_id: u64,
        resource: Arc<dyn BrokerVideoResource>,
        stop: &StopToken,
    ) -> Result<Arc<Self>> {
        stopped(stop)?;
        authority.validate()?;
        let maximum_bytes = resource.maximum_bytes();
        if resource_id == 0 || maximum_bytes == 0 || maximum_bytes > 256 * 1024 * 1024 {
            return Err(AnimationError::Budget(
                "native video finite resource bound".into(),
            ));
        }
        authorization.check(&authority, resource_id, VideoOperation::Acquire)?;
        let root = client.quota_group();
        if !root.shares_root(&resource.quota_group()) {
            return Err(AnimationError::PermissionDenied(
                "video resource must retain original quota".into(),
            ));
        }
        let admission = storage(&root, 512)?;
        Ok(Arc::new(Self {
            authority,
            resource_id,
            encoded: Vec::new(),
            maximum_bytes,
            resource: Some(resource),
            root,
            _storage: admission,
        }))
    }
    pub fn read_at(&self, offset: u64, output: &mut [u8], stop: &StopToken) -> Result<usize> {
        stopped(stop)?;
        let maximum = self.maximum_bytes;
        let offset =
            usize::try_from(offset).map_err(|_| failure("video resource offset overflow"))?;
        if offset >= maximum {
            return Ok(0);
        }
        let length = output.len().min(maximum - offset);
        if let Some(resource) = &self.resource {
            let read = resource.read_at(offset as u64, &mut output[..length], stop)?;
            if read > length {
                return Err(failure("video resource exceeded read bound"));
            }
            Ok(read)
        } else {
            output[..length].copy_from_slice(&self.encoded[offset..offset + length]);
            Ok(length)
        }
    }
    fn cancel_resource(&self) {
        if let Some(resource) = &self.resource {
            resource.cancel();
        }
    }
    /// Native adapter admission check; never expose the quota object to scripts.
    pub fn quota_group(&self) -> QuotaGroup {
        self.root.clone()
    }
    pub fn authority(&self) -> &VideoAuthority {
        &self.authority
    }
    pub fn resource_id(&self) -> u64 {
        self.resource_id
    }
    pub fn encoded(&self) -> Result<&[u8]> {
        if self.resource.is_some() {
            return Err(failure(
                "video input is a pinned/stream resource; use bounded read_at",
            ));
        }
        Ok(&self.encoded)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoPixelFormat {
    Gray8,
    Rgb8,
    Rgba8,
}
impl VideoPixelFormat {
    fn channels(self) -> usize {
        match self {
            Self::Gray8 => 1,
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoGeometry {
    pub width: u32,
    pub height: u32,
    pub format: VideoPixelFormat,
}
#[derive(Debug, Clone, Copy)]
pub struct VideoLimits {
    pub geometry: VideoGeometry,
    pub queue_frames: usize,
    pub lookahead: Duration,
    pub maximum_pts: Duration,
    pub maximum_frames: u64,
    pub maximum_frame_duration: Duration,
}
impl VideoLimits {
    fn frame_bytes(self) -> Result<usize> {
        let pixels = (self.geometry.width as usize)
            .checked_mul(self.geometry.height as usize)
            .ok_or_else(|| failure("video dimensions overflow"))?;
        if self.geometry.width == 0
            || self.geometry.height == 0
            || self.geometry.width > 8192
            || self.geometry.height > 8192
            || pixels > 4 * 1024 * 1024
            || self.queue_frames == 0
            || self.queue_frames > 12
            || self.lookahead > Duration::from_secs(2)
            || self.maximum_pts.is_zero()
            || self.maximum_pts > Duration::from_secs(86400)
            || self.maximum_frames == 0
            || self.maximum_frames > 10_000_000
            || self.maximum_frame_duration.is_zero()
            || self.maximum_frame_duration > Duration::from_secs(60)
        {
            return Err(AnimationError::Budget(
                "native video geometry/time/backlog limits".into(),
            ));
        }
        Ok(pixels * self.geometry.format.channels())
    }
}
#[derive(Debug, Clone, Copy)]
pub struct CodecBudget {
    pub native_tasks: usize,
    pub native_resident_bytes: usize,
    pub scratch_bytes: usize,
}
#[derive(Debug, Clone, Copy)]
pub struct VideoInfo {
    pub geometry: VideoGeometry,
    pub duration: Option<Duration>,
    pub seekable: bool,
}
#[derive(Debug, Clone, Copy)]
pub struct FrameStamp {
    pub pts: Duration,
    pub duration: Duration,
}
/// Private native-domain wake capability. It must be nonblocking, idempotent,
/// and scoped to this factory instance; never signal an arbitrary caller PID.
pub trait CodecInterrupt: Send + Sync {
    fn cancel(&self);
}
/// Library/process adapter. fill_frame writes exactly the supplied fixed-size
/// plane (no replacement Vec). Return ordered source PTS; seek resets ordering.
/// close_and_wait MUST retain native handles/leases and return only after actual
/// native exit/reap, on success OR error. A stuck close stays inside this owned
/// worker; a caller deadline does not release its physical admission.
pub trait VideoDecoder: Send {
    fn info(&self) -> VideoInfo;
    fn fill_frame(&mut self, output: &mut [u8], stop: &StopToken) -> Result<Option<FrameStamp>>;
    fn seek(&mut self, _position: Duration, _stop: &StopToken) -> Result<()> {
        Err(failure("codec seek unsupported"))
    }
    fn close_and_wait(&mut self, stop: &StopToken) -> Result<()>;
}
/// A dedicated factory per session. budget is declaration, not an RSS limit.
/// open must retain partially created native handles until actual cleanup even
/// when returning an error. Production factory must enforce its declared process/task/memory domain and
/// transfer custody into real native join owners; no ambient default factory.
pub trait VideoDecoderFactory: Send + Sync {
    fn budget(&self) -> CodecBudget;
    fn interrupt(&self) -> Arc<dyn CodecInterrupt>;
    fn open(
        &self,
        input: Arc<VerifiedVideoInput>,
        geometry: VideoGeometry,
        stop: &StopToken,
    ) -> Result<Box<dyn VideoDecoder>>;
}
#[derive(Debug)]
pub struct TimedVideoFrame {
    pub generation: u64,
    pub sequence: u64,
    pub stamp: FrameStamp,
    pub geometry: VideoGeometry,
    pub authority: VideoAuthority,
    pub resource_id: u64,
    pixels: Vec<u8>,
    _storage: StorageAdmission,
}
impl TimedVideoFrame {
    pub fn bytes(&self) -> &[u8] {
        &self.pixels
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoPhase {
    Opening,
    Playing,
    Paused,
    Ended,
    Stopping,
    Exited,
    Failed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFailure {
    Authorization,
    Codec,
    Protocol,
    Budget,
}
#[derive(Debug, Clone, Copy)]
pub struct VideoStatus {
    pub phase: VideoPhase,
    pub queued_frames: usize,
    pub decoded_frames: u64,
    pub discarded_frames: u64,
    pub generation: u64,
    pub target: Duration,
    pub info: Option<VideoInfo>,
    pub failure: Option<VideoFailure>,
    pub actual_exit: Option<WorkerExit>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseState {
    Joined,
    Retiring,
}
#[derive(Clone, Copy)]
struct Seek {
    position: Duration,
    generation: u64,
}
struct State {
    phase: VideoPhase,
    queue: VecDeque<Arc<TimedVideoFrame>>,
    latest: Option<Arc<TimedVideoFrame>>,
    target: Duration,
    generation: u64,
    decoded: u64,
    discarded: u64,
    info: Option<VideoInfo>,
    failure: Option<VideoFailure>,
    paused: bool,
    seek: Option<Seek>,
    stop: bool,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    _metadata: StorageAdmission,
}
#[derive(Clone)]
struct WorkerCustody {
    _physical: Arc<WorkerAdmission>,
    _request: Arc<Retained<()>>,
    _scratch: Arc<StorageAdmission>,
    _input: Arc<VerifiedVideoInput>,
}
pub struct NativeVideo {
    shared: Arc<Shared>,
    worker: OwnedWorker,
    authorization: Arc<dyn VideoAuthorization>,
    input: Arc<VerifiedVideoInput>,
    limits: VideoLimits,
}
impl NativeVideo {
    pub fn start(
        client: Client,
        input: Arc<VerifiedVideoInput>,
        authorization: Arc<dyn VideoAuthorization>,
        factory: Arc<dyn VideoDecoderFactory>,
        limits: VideoLimits,
        stop: StopToken,
    ) -> Result<Self> {
        stopped(&stop)?;
        let frame_bytes = limits.frame_bytes()?;
        let quota = client.quota_group();
        if !quota.shares_root(&input.root) {
            return Err(AnimationError::PermissionDenied(
                "video resource/admissions must share original quota root".into(),
            ));
        }
        authorization.check(input.authority(), input.resource_id(), VideoOperation::Open)?;
        let budget = factory.budget();
        if budget.native_tasks > 32
            || budget.native_resident_bytes > 1024 * 1024 * 1024
            || budget.scratch_bytes > 512 * 1024 * 1024
        {
            return Err(AnimationError::Budget(
                "native video codec declaration".into(),
            ));
        }
        let physical = Arc::new(
            quota
                .reserve_external_worker(
                    budget.native_tasks + 2,
                    budget
                        .native_resident_bytes
                        .checked_add(4 * 1024 * 1024)
                        .ok_or_else(|| failure("codec physical budget overflow"))?,
                )
                .map_err(|error| {
                    AnimationError::Budget(format!("video physical admission: {error:?}"))
                })?,
        );
        let reservation = client
            .try_reserve_external(JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            })
            .map_err(|error| {
                AnimationError::Budget(format!("video client admission: {error:?}"))
            })?;
        let request = Arc::new(reservation.retain(()).map_err(|error| {
            AnimationError::Budget(format!("video client retention: {:?}", error.reason))
        })?);
        let scratch = Arc::new(storage(&quota, budget.scratch_bytes.max(1))?);
        let metadata = storage(
            &quota,
            limits.queue_frames * std::mem::size_of::<Arc<TimedVideoFrame>>() + 4096,
        )?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                phase: VideoPhase::Opening,
                queue: VecDeque::with_capacity(limits.queue_frames),
                latest: None,
                target: Duration::ZERO,
                generation: 1,
                decoded: 0,
                discarded: 0,
                info: None,
                failure: None,
                paused: false,
                seek: None,
                stop: false,
            }),
            changed: Condvar::new(),
            _metadata: metadata,
        });
        let interrupt = factory.interrupt();
        let wake = Arc::clone(&shared);
        let custody = WorkerCustody {
            _physical: physical,
            _request: request,
            _scratch: scratch,
            _input: Arc::clone(&input),
        };
        let wake_custody = custody.clone();
        let worker_shared = Arc::clone(&shared);
        let worker_input = Arc::clone(&input);
        let worker_auth = Arc::clone(&authorization);
        let worker = owned_worker::spawn_owned(
            "ilium-native-video",
            owned_worker::WorkerKind::SynchronousIo,
            stop,
            move || {
                let _retain = &wake_custody;
                wake_custody._input.cancel_resource();
                interrupt.cancel();
                wake.changed.notify_all();
            },
            move |stop| {
                let _custody = custody;
                ilium_platform::thread_priority::lower_current_thread(
                    ilium_platform::thread_priority::WorkerPriority::BelowNormal,
                );
                run_worker(
                    VideoWorkerContext {
                        shared: worker_shared,
                        input: worker_input,
                        authorization: worker_auth,
                        factory,
                        quota,
                        limits,
                        frame_bytes,
                    },
                    stop,
                );
            },
        )?;
        Ok(Self {
            shared,
            worker,
            authorization,
            input,
            limits,
        })
    }
    fn authorize(&self, operation: VideoOperation) -> Result<()> {
        self.authorization
            .check(self.input.authority(), self.input.resource_id(), operation)
    }
    /// Coalesced playback target and latest due immutable frame. No future frame
    /// is delivered; superseded targets do not form an unbounded command queue.
    pub fn latest_at(&self, position: Duration) -> Result<Option<Arc<TimedVideoFrame>>> {
        self.authorize(VideoOperation::Deliver)?;
        if position > self.limits.maximum_pts {
            return Err(AnimationError::Budget("video playback target".into()));
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.stop {
            return Err(failure("video service retired"));
        }
        if !state.paused {
            if position < state.target {
                return Err(failure("backwards playback requires explicit seek"));
            }
            state.target = position;
        }
        let target = state.target;
        while state
            .queue
            .front()
            .is_some_and(|frame| frame.stamp.pts <= target)
        {
            if let Some(frame) = state.queue.pop_front() {
                if state.latest.replace(frame).is_some() {
                    state.discarded = state.discarded.saturating_add(1);
                }
            }
        }
        let frame = state.latest.clone();
        self.shared.changed.notify_all();
        Ok(frame)
    }
    pub fn pause(&self) -> Result<()> {
        self.authorize(VideoOperation::Pause)?;
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.stop {
            return Err(failure("video service retired"));
        }
        state.paused = true;
        if state.phase == VideoPhase::Playing {
            state.phase = VideoPhase::Paused;
        }
        self.shared.changed.notify_all();
        Ok(())
    }
    pub fn resume(&self) -> Result<()> {
        self.authorize(VideoOperation::Resume)?;
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.stop {
            return Err(failure("video service retired"));
        }
        state.paused = false;
        if state.phase == VideoPhase::Paused {
            state.phase = VideoPhase::Playing;
        }
        self.shared.changed.notify_all();
        Ok(())
    }
    pub fn seek(&self, position: Duration) -> Result<u64> {
        self.authorize(VideoOperation::Seek)?;
        if position > self.limits.maximum_pts {
            return Err(AnimationError::Budget("video seek position".into()));
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.stop || !state.info.is_some_and(|info| info.seekable) {
            return Err(failure("video seek unavailable/not ready"));
        }
        if state
            .info
            .and_then(|info| info.duration)
            .is_some_and(|duration| position > duration)
        {
            return Err(AnimationError::Budget(
                "video seek exceeds clip duration".into(),
            ));
        }
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| failure("video generation exhausted"))?;
        let generation = state.generation;
        state.seek = Some(Seek {
            position,
            generation,
        });
        state.target = position;
        state.queue.clear();
        state.latest = None;
        if state.phase == VideoPhase::Ended {
            state.phase = if state.paused {
                VideoPhase::Paused
            } else {
                VideoPhase::Playing
            };
        }
        self.shared.changed.notify_all();
        Ok(generation)
    }
    pub fn status(&self) -> Result<VideoStatus> {
        self.authorize(VideoOperation::Status)?;
        let state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        Ok(VideoStatus {
            phase: state.phase,
            queued_frames: state.queue.len(),
            decoded_frames: state.decoded,
            discarded_frames: state.discarded,
            generation: state.generation,
            target: state.target,
            info: state.info,
            failure: state.failure,
            actual_exit: self.worker.ticket().exit(),
        })
    }
    /// Signal only; suitable for event/UI code. Actual join can remain pending.
    pub fn cancel(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.stop = true;
        state.phase = if self.worker.ticket().exit().is_some() {
            if state.failure.is_some() {
                VideoPhase::Failed
            } else {
                VideoPhase::Exited
            }
        } else {
            VideoPhase::Stopping
        };
        state.queue.clear();
        state.latest = None;
        drop(state);
        self.worker.ticket().cancel();
        self.shared.changed.notify_all();
    }
    /// Dedicated background owner only. Timeout keeps the supervisor, codec,
    /// resource, client reservation and physical admission alive until actual exit.
    pub fn close_until(&self, deadline: Instant) -> CloseState {
        self.cancel();
        if self.worker.ticket().join_until(deadline).is_ok() {
            CloseState::Joined
        } else {
            CloseState::Retiring
        }
    }
}
impl Drop for NativeVideo {
    fn drop(&mut self) {
        self.cancel();
    }
}
fn set_failure(shared: &Shared, kind: VideoFailure) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    state.failure = Some(kind);
    state.phase = VideoPhase::Failed;
    shared.changed.notify_all();
}
/// The complete already-admitted source/codec graph moves onto its join owner.
/// Custody guards remain in the surrounding worker closure through actual exit.
struct VideoWorkerContext {
    shared: Arc<Shared>,
    input: Arc<VerifiedVideoInput>,
    authorization: Arc<dyn VideoAuthorization>,
    factory: Arc<dyn VideoDecoderFactory>,
    quota: QuotaGroup,
    limits: VideoLimits,
    frame_bytes: usize,
}
fn run_worker(context: VideoWorkerContext, stop: StopToken) {
    let VideoWorkerContext {
        shared,
        input,
        authorization,
        factory,
        quota,
        limits,
        frame_bytes,
    } = context;
    if authorization
        .check(input.authority(), input.resource_id(), VideoOperation::Open)
        .is_err()
    {
        set_failure(&shared, VideoFailure::Authorization);
        return;
    }
    let mut decoder = match factory.open(Arc::clone(&input), limits.geometry, &stop) {
        Ok(decoder) => decoder,
        Err(_) => {
            set_failure(&shared, VideoFailure::Codec);
            return;
        }
    };
    let info = decoder.info();
    if info.geometry != limits.geometry
        || info
            .duration
            .is_some_and(|duration| duration > limits.maximum_pts)
    {
        set_failure(&shared, VideoFailure::Protocol);
        let _ = decoder.close_and_wait(&stop);
        return;
    }
    {
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.info = Some(info);
        state.phase = if state.paused {
            VideoPhase::Paused
        } else {
            VideoPhase::Playing
        };
        shared.changed.notify_all();
    }
    let mut previous_pts = None;
    let mut read_count = 0u64;
    let mut ended = false;
    loop {
        let action = {
            let mut state = shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            loop {
                if stop.is_stopped() || state.stop {
                    break None;
                }
                if let Some(seek) = state.seek.take() {
                    break Some((state.generation, Some(seek)));
                }
                let ahead = state.queue.back().is_some_and(|frame| {
                    frame.stamp.pts >= state.target.saturating_add(limits.lookahead)
                });
                if !state.paused && !ended && state.queue.len() < limits.queue_frames && !ahead {
                    break Some((state.generation, None));
                }
                state = shared
                    .changed
                    .wait_timeout(state, Duration::from_millis(100))
                    .unwrap_or_else(|error| error.into_inner())
                    .0;
            }
        };
        let Some((generation, seek)) = action else {
            break;
        };
        if let Some(seek) = seek {
            if authorization
                .check(input.authority(), input.resource_id(), VideoOperation::Seek)
                .is_err()
            {
                set_failure(&shared, VideoFailure::Authorization);
                break;
            }
            if decoder.seek(seek.position, &stop).is_err() {
                set_failure(&shared, VideoFailure::Codec);
                break;
            }
            previous_pts = None;
            ended = false;
            let mut state = shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.generation == seek.generation {
                state.phase = if state.paused {
                    VideoPhase::Paused
                } else {
                    VideoPhase::Playing
                };
            }
            continue;
        }
        if read_count >= limits.maximum_frames {
            set_failure(&shared, VideoFailure::Budget);
            break;
        }
        if authorization
            .check(
                input.authority(),
                input.resource_id(),
                VideoOperation::Deliver,
            )
            .is_err()
        {
            set_failure(&shared, VideoFailure::Authorization);
            break;
        }
        let admission = match storage(&quota, frame_bytes + 512) {
            Ok(admission) => admission,
            Err(_) => {
                set_failure(&shared, VideoFailure::Budget);
                break;
            }
        };
        let mut pixels = Vec::new();
        if pixels.try_reserve_exact(frame_bytes).is_err() {
            set_failure(&shared, VideoFailure::Budget);
            break;
        }
        pixels.resize(frame_bytes, 0);
        let stamp = match decoder.fill_frame(&mut pixels, &stop) {
            Ok(Some(stamp)) => stamp,
            Ok(None) => {
                ended = true;
                let mut state = shared
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                state.phase = VideoPhase::Ended;
                shared.changed.notify_all();
                continue;
            }
            Err(_) => {
                if !stop.is_stopped() {
                    set_failure(&shared, VideoFailure::Codec);
                }
                break;
            }
        };
        read_count += 1;
        if stamp.pts > limits.maximum_pts
            || stamp.duration.is_zero()
            || stamp.duration > limits.maximum_frame_duration
            || stamp
                .pts
                .checked_add(stamp.duration)
                .is_none_or(|end| end > limits.maximum_pts)
            || previous_pts.is_some_and(|previous| stamp.pts <= previous)
        {
            set_failure(&shared, VideoFailure::Protocol);
            break;
        }
        previous_pts = Some(stamp.pts);
        if authorization
            .check(
                input.authority(),
                input.resource_id(),
                VideoOperation::Deliver,
            )
            .is_err()
        {
            set_failure(&shared, VideoFailure::Authorization);
            break;
        }
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.decoded = state.decoded.saturating_add(1);
        if state.stop || stop.is_stopped() || state.generation != generation {
            state.discarded = state.discarded.saturating_add(1);
            continue;
        }
        while state.queue.len() >= limits.queue_frames {
            state.queue.pop_front();
            state.discarded = state.discarded.saturating_add(1);
        }
        let frame = Arc::new(TimedVideoFrame {
            generation,
            sequence: read_count,
            stamp,
            geometry: limits.geometry,
            authority: input.authority().clone(),
            resource_id: input.resource_id(),
            pixels,
            _storage: admission,
        });
        if stamp.pts <= state.target {
            if state.latest.replace(frame).is_some() {
                state.discarded = state.discarded.saturating_add(1);
            }
        } else {
            state.queue.push_back(frame);
        }
        shared.changed.notify_all();
    }
    let closed = decoder.close_and_wait(&stop);
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if closed.is_err() && state.failure.is_none() {
        state.failure = Some(VideoFailure::Codec);
    }
    state.queue.clear();
    state.latest = None;
    state.phase = if state.failure.is_some() {
        VideoPhase::Failed
    } else {
        VideoPhase::Exited
    };
    shared.changed.notify_all();
}
