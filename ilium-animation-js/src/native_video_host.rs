//! Actual Video acquisition and decoder owner for one accepted PackageInstance.
//! Script records are lookup data only. Finite bytes are acquired through the
//! original package, selected-resource or reviewed HTTP owners before the
//! fixed pipe-only decoder sees them. No URL/path/guest argument reaches FFmpeg.
use crate::{
    engine::{CompletionState, HostRequest, ServiceValue},
    error::{AnimationError, Result},
    http::HttpOptions,
    manifest::AnimationMode,
    native_asset_host::{NativeAssetHost, VideoAssetSelection},
    native_draw_host::NativeDrawHost,
    native_http_authority::NativeHttpAuthorityFactory,
    native_http_host::{NativeDns, NativeHttpHost},
    native_storage::{RetainedBytes, SelectedStorage, StorageCancellation},
    native_video::{
        BrokerVideoResource, NativeVideo, VerifiedVideoInput, VideoDecoderFactory, VideoGeometry,
        VideoLimits, VideoPhase, VideoPixelFormat, VideoStatus,
    },
    native_video_decoder::{FfmpegDecoderFactory, FfmpegLimits},
    permissions::{HttpMethod, OperationNeed},
    plan_authorization::operation_demand,
    replay::{FrozenEvidence, GrantLineage, NativeEvidenceInput, ReplayHistory, ReplayReceipt},
    runtime::{PackageInstance, ServiceOperation},
    sources::{http_options, SourceHttpResponse},
    surface::Shape,
};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, QuotaGroup, Receipt, Retention,
    StorageAdmission,
};
use ilium_platform::{
    animation_sandbox::{self, SandboxLimits},
    owned_worker::StopToken,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MAX_VIDEO_HANDLES: usize = 8;
const MAX_PENDING: usize = 16;
const SELECTED_MAX_BYTES: usize = 8 * 1024 * 1024;
const HTTP_MAX_BYTES: usize = 32_000_000;
const REGISTRY_BYTES: usize = 128 * 1024;

fn invalid(message: &'static str) -> AnimationError {
    AnimationError::Runtime(format!("native video: {message}"))
}
fn denied(message: &'static str) -> AnimationError {
    AnimationError::PermissionDenied(format!("native video: {message}"))
}
fn no_planes(request: &HostRequest) -> Result<()> {
    if !request.payload.arrays().is_empty() || !request.payload.planes().is_empty() {
        return Err(invalid("unexpected binary planes"));
    }
    Ok(())
}
fn fields<'a>(request: &'a HostRequest, allowed: &[&str]) -> Result<&'a Map<String, Value>> {
    no_planes(request)?;
    let fields = request
        .payload
        .metadata()
        .as_object()
        .ok_or_else(|| invalid("options must be a record"))?;
    if fields.len() > allowed.len() || fields.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid("unknown Video option"));
    }
    Ok(fields)
}
fn positive(fields: &Map<String, Value>, name: &str, maximum: u64) -> Result<u32> {
    fields
        .get(name)
        .and_then(Value::as_u64)
        .filter(|value| (1..=maximum).contains(value))
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| invalid("Video integer bound"))
}
fn handle(fields: &Map<String, Value>) -> Result<&str> {
    if fields.get("kind").and_then(Value::as_str) != Some("media.video") {
        return Err(denied("foreign Video handle kind"));
    }
    let id = fields
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| id.starts_with("video-") && id.len() <= 64)
        .ok_or_else(|| denied("unknown Video handle"))?;
    Ok(id)
}

enum SourceChoice {
    Asset {
        projection: Value,
        relative_path: String,
    },
    Url(String),
}
struct OpenOptions {
    source: SourceChoice,
    max_pixels: u32,
    max_fps: u32,
}

struct VideoDecodeLimits {
    max_pixels: u32,
    max_fps: u32,
}

struct SelectedVideoSource {
    request_id: String,
    resource: Arc<SelectedStorage>,
    relative_path: String,
}

impl OpenOptions {
    fn parse(request: &HostRequest) -> Result<Self> {
        let options = fields(
            request,
            &["asset", "relative_path", "url", "max_pixels", "max_fps"],
        )?;
        Self::parse_fields(options)
    }
    fn parse_fields(options: &Map<String, Value>) -> Result<Self> {
        if options.len() > 5
            || options.keys().any(|key| {
                !["asset", "relative_path", "url", "max_pixels", "max_fps"].contains(&key.as_str())
            })
        {
            return Err(invalid("unknown Video option"));
        }
        let max_pixels = positive(options, "max_pixels", 4 * 1024 * 1024)?;
        let max_fps = positive(options, "max_fps", 120)?;
        let source = match (options.get("asset"), options.get("url")) {
            (Some(asset), None) => {
                let projected = asset
                    .as_object()
                    .filter(|value| {
                        value.len() == 2
                            && value.get("kind").and_then(Value::as_str) == Some("asset")
                            && value
                                .get("id")
                                .and_then(Value::as_str)
                                .is_some_and(|id| !id.is_empty() && id.len() <= 128)
                    })
                    .ok_or_else(|| denied("foreign Video asset projection"))?;
                let relative_path = options
                    .get("relative_path")
                    .map(|value| value.as_str().ok_or_else(|| invalid("Video relative path")))
                    .transpose()?
                    .unwrap_or("")
                    .to_owned();
                SourceChoice::Asset {
                    projection: Value::Object(projected.clone()),
                    relative_path,
                }
            }
            (None, Some(url)) if !options.contains_key("relative_path") => {
                let url = url
                    .as_str()
                    .filter(|url| !url.is_empty() && url.len() <= 4096)
                    .ok_or_else(|| invalid("Video URL"))?;
                SourceChoice::Url(url.to_owned())
            }
            _ => return Err(invalid("exactly one Video acquisition option is required")),
        };
        Ok(Self {
            source,
            max_pixels,
            max_fps,
        })
    }
}

#[cfg(test)]
mod option_tests {
    use super::*;

    #[test]
    fn video_open_requires_one_original_source_and_bounded_integer_limits() {
        let selected = json!({"asset":{"id":"selected-1","kind":"asset"},
            "relative_path":"clip.mp4","max_pixels":400,"max_fps":25});
        let options = OpenOptions::parse_fields(selected.as_object().unwrap()).unwrap();
        assert!(matches!(options.source, SourceChoice::Asset { .. }));
        assert_eq!(options.max_pixels, 400);
        assert_eq!(options.max_fps, 25);
        for invalid_options in [
            json!({"max_pixels":400,"max_fps":25}),
            json!({"asset":{"id":"selected-1","kind":"asset"},"url":"https://example.invalid/v.mp4","max_pixels":400,"max_fps":25}),
            json!({"url":"https://example.invalid/v.mp4","relative_path":"clip.mp4","max_pixels":400,"max_fps":25}),
            json!({"url":"https://example.invalid/v.mp4","max_pixels":0,"max_fps":25}),
            json!({"url":"https://example.invalid/v.mp4","max_pixels":400,"max_fps":121}),
            json!({"url":"https://example.invalid/v.mp4","max_pixels":400,"max_fps":25,"grant":"allow-all"}),
        ] {
            assert!(OpenOptions::parse_fields(invalid_options.as_object().unwrap()).is_err());
        }
    }

    #[test]
    fn video_emission_history_survives_more_than_one_clip_lifetime() {
        let mut history = VideoHistoryState::default();
        for sequence in 1..=14_402_u64 {
            let value: [u8; 32] = Sha256::digest(sequence.to_le_bytes()).into();
            history.commit(sequence, value).unwrap();
        }
        assert_eq!(history.recent.len(), 14_400);
        assert_eq!(history.emitted_count, 14_402);
        assert_eq!(history.retired_through, 2);
        let latest: [u8; 32] = Sha256::digest(14_402_u64.to_le_bytes()).into();
        let chain = history.chain;
        history.commit(14_402, latest).unwrap();
        assert_eq!(history.emitted_count, 14_402);
        assert_eq!(history.chain, chain);
        assert!(history.commit(14_402, [9; 32]).is_err());
        assert!(history.commit(1, [1; 32]).is_err());
        assert!(history.commit(14_403, [3; 32]).is_ok());
        assert_eq!(history.recent.len(), 14_400);
    }

    struct RejectedVideoProbeJob {
        identity: Arc<()>,
        storage: Arc<StorageAdmission>,
    }

    impl Job for RejectedVideoProbeJob {
        type Output = ();
        type Error = AnimationError;

        fn run(self, _context: JobContext) -> Result<Self::Output> {
            Ok(())
        }
    }

    #[test]
    fn reserved_video_rejection_preserves_exact_job_owner_until_caller_handles_refusal() {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaLimits, ShutdownMode,
        };
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 2,
            service_jobs: 0,
            input_bytes: 1024 * 1024,
            result_bytes: 1024 * 1024,
            worker_threads: 2,
            worker_bytes: 8 * 1024 * 1024,
        });
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let mut execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: disabled,
                io: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: None,
                    resident_bytes_per_thread: 1024,
                },
                service: disabled,
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 2,
                service_jobs: 0,
                input_bytes: 1024 * 1024,
                result_bytes: 1024 * 1024,
            })
            .unwrap();
        let storage = Arc::new(quota.reserve_external_storage(4096).unwrap());
        let identity = Arc::new(());
        let baseline = quota.snapshot();
        let reservation = client
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes: 1,
                    result_bytes: 1,
                },
            )
            .unwrap();
        assert_eq!(quota.snapshot().jobs, baseline.jobs + 1);
        let pending_job = Cell::new(Some(RejectedVideoProbeJob {
            identity: Arc::clone(&identity),
            storage: Arc::clone(&storage),
        }));
        let job = pending_job.take().unwrap();
        match reservation.submit(job) {
            Ok(_) => panic!("undersized reservation must reject before queue publication"),
            Err(rejected) => pending_job.set(Some(rejected.value)),
        }
        let recovered = pending_job
            .take()
            .expect("rejected Video job must remain in exact caller custody");
        assert!(Arc::ptr_eq(&recovered.identity, &identity));
        assert!(Arc::ptr_eq(&recovered.storage, &storage));
        assert_eq!(quota.snapshot().jobs, baseline.jobs);
        assert_eq!(quota.snapshot().worker_bytes, baseline.worker_bytes);
        drop(recovered);
        drop(storage);
        drop(client);
        execution.request_shutdown(ShutdownMode::Drain);
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(report.remaining_workers, 0);
    }

    #[test]
    fn video_history_rejects_stale_or_foreign_replay_principal() {
        let source = crate::native_video::VideoAuthority {
            package_digest: "a".repeat(64),
            instance_id: 3,
            plan_revision: 5,
            authorization_epoch: 7,
        };
        let mut replay = crate::replay::ReplayAuthority {
            package_digest: source.package_digest.clone(),
            instance_id: 3,
            revision: 5,
            authorization_epoch: 7,
        };
        assert!(matching_video_authority(&source, &replay));
        replay.authorization_epoch = 8;
        assert!(!matching_video_authority(&source, &replay));
        replay.authorization_epoch = 7;
        replay.instance_id = 4;
        assert!(!matching_video_authority(&source, &replay));
        replay.instance_id = 3;
        replay.revision = 6;
        assert!(!matching_video_authority(&source, &replay));
    }
}

struct SelectedVideoJob {
    resource: Arc<SelectedStorage>,
    relative_path: String,
    cancellation: StorageCancellation,
    quota: QuotaGroup,
}
impl Job for SelectedVideoJob {
    type Output = Arc<RetainedBytes>;
    type Error = AnimationError;
    fn run(self, context: JobContext) -> Result<Self::Output> {
        if context.stop_requested() {
            return Err(invalid("selected Video job stopped"));
        }
        let bytes = self.resource.read_after_issue(
            &self.relative_path,
            SELECTED_MAX_BYTES,
            &self.cancellation,
            &self.quota,
        )?;
        if context.stop_requested() {
            return Err(invalid("selected Video job stopped after read"));
        }
        Ok(bytes)
    }
}
struct HttpVideoJob {
    options: HttpOptions,
    request: HostRequest,
    factory: NativeHttpAuthorityFactory,
    client: Client,
    dns: NativeDns,
    stop: Arc<AtomicBool>,
}
impl Job for HttpVideoJob {
    type Output = SourceHttpResponse;
    type Error = AnimationError;
    fn run(mut self, context: JobContext) -> Result<Self::Output> {
        if context.stop_requested() || self.request.is_cancelled() {
            return Err(denied("Video HTTP job stopped"));
        }
        let remaining = self.request.remaining_ms();
        if remaining == 0 {
            return Err(denied("Video HTTP deadline expired"));
        }
        self.options.timeout_ms = self.options.timeout_ms.min(remaining);
        let mut authority = self.factory.enter(&context, &self.client)?;
        let response = authority.request_source_response(&self.options, &self.dns, &self.stop)?;
        if context.stop_requested() || self.request.is_cancelled() {
            return Err(denied("Video HTTP body delivery stopped"));
        }
        Ok(response)
    }
}
struct SelectedVideoBytes {
    bytes: Arc<RetainedBytes>,
    quota: QuotaGroup,
}
struct HttpVideoBytes {
    bytes: Arc<SourceHttpResponse>,
    quota: QuotaGroup,
}
fn read_bytes(bytes: &[u8], offset: u64, output: &mut [u8], stop: &StopToken) -> Result<usize> {
    if stop.is_stopped() {
        return Err(invalid("finite Video read stopped"));
    }
    let offset = usize::try_from(offset).map_err(|_| invalid("finite Video read offset"))?;
    if offset >= bytes.len() {
        return Ok(0);
    }
    let count = output.len().min(bytes.len() - offset);
    output[..count].copy_from_slice(&bytes[offset..offset + count]);
    Ok(count)
}
impl BrokerVideoResource for SelectedVideoBytes {
    fn quota_group(&self) -> QuotaGroup {
        self.quota.clone()
    }
    fn maximum_bytes(&self) -> usize {
        self.bytes.view().len()
    }
    fn read_at(&self, offset: u64, output: &mut [u8], stop: &StopToken) -> Result<usize> {
        read_bytes(self.bytes.view(), offset, output, stop)
    }
    fn cancel(&self) {}
}
impl BrokerVideoResource for HttpVideoBytes {
    fn quota_group(&self) -> QuotaGroup {
        self.quota.clone()
    }
    fn maximum_bytes(&self) -> usize {
        self.bytes.as_bytes().len()
    }
    fn read_at(&self, offset: u64, output: &mut [u8], stop: &StopToken) -> Result<usize> {
        read_bytes(self.bytes.as_bytes(), offset, output, stop)
    }
    fn cancel(&self) {}
}

enum AcquisitionReceipt {
    Selected(Receipt<SelectedVideoJob>),
    Http(Receipt<HttpVideoJob>),
}
enum AcquiredBytes {
    Selected(Arc<RetainedBytes>),
    Http(Arc<SourceHttpResponse>),
}
enum VideoSource<'a> {
    Bundle(&'a [u8]),
    Resource(Arc<dyn BrokerVideoResource>),
}
struct PendingOpen {
    request: HostRequest,
    operation: Option<ServiceOperation>,
    receipt: Option<AcquisitionReceipt>,
    retention: Option<Retention>,
    acquired: Option<AcquiredBytes>,
    video: Option<NativeVideo>,
    result: Option<ServiceValue>,
    resource_id: u64,
    max_pixels: u32,
    max_fps: u32,
    selected_cancellation: Option<StorageCancellation>,
    http_stop: Option<Arc<AtomicBool>>,
    lineage: Vec<GrantLineage>,
    // A lost receipt or uncertain helper copy must remain in this map until
    // the separate physical owner proves terminal retirement.
    uncertain: bool,
}
struct ActiveVideo {
    video: NativeVideo,
    _input_retention: Option<Retention>,
    resource_id: u64,
    revision: u64,
    current_image: Option<String>,
    staged_old_image: Option<String>,
    latest: Option<Value>,
    latest_sequence: Option<(u64, u64)>,
    last_phase: Option<VideoPhase>,
    closing: Option<HostRequest>,
    lineage: Vec<GrantLineage>,
    recorded_token: Option<u64>,
    recorded_history: Option<Arc<VideoReplayHistory>>,
}

/// The actual immutable input stays alive in FrozenEvidence after both the
/// helper and decoder retire. Surviving-dot receipts are committed only after
/// the terminal compositor supplies ReplayFlushedProof, never during sampling.
struct VideoReplayHistory {
    _input: Arc<VerifiedVideoInput>,
    source_digest: [u8; 32],
    payload: Vec<u8>,
    authority: crate::native_video::VideoAuthority,
    records: Mutex<VideoHistoryState>,
    _admission: StorageAdmission,
}
fn matching_video_authority(
    source: &crate::native_video::VideoAuthority,
    replay: &crate::replay::ReplayAuthority,
) -> bool {
    replay.package_digest == source.package_digest
        && replay.instance_id == source.instance_id
        && replay.revision == source.plan_revision
        && replay.authorization_epoch == source.authorization_epoch
}
/// Terminal proofs are single use and carry a globally increasing native
/// presentation sequence. Keep the last 14,400 receipts for exact retry
/// comparison; old unknown receipts fail closed after eviction. The digest
/// chain records every successful flush without retaining every allocation.
#[derive(Default)]
struct VideoHistoryState {
    recent: BTreeMap<u64, [u8; 32]>,
    retired_through: u64,
    chain: [u8; 32],
    emitted_count: u64,
}
impl VideoHistoryState {
    fn commit(&mut self, sequence: u64, value: [u8; 32]) -> Result<()> {
        if sequence == 0 || sequence <= self.retired_through {
            return Err(denied("retired Video emission receipt"));
        }
        if let Some(previous) = self.recent.get(&sequence) {
            return if previous == &value {
                Ok(())
            } else {
                Err(denied("Video history receipt changed after terminal flush"))
            };
        }
        let count = self
            .emitted_count
            .checked_add(1)
            .ok_or_else(|| invalid("Video emission history count exhausted"))?;
        let mut chain = Sha256::new();
        chain.update(b"ilium-video-emission-history-v1");
        chain.update(self.chain);
        chain.update(sequence.to_le_bytes());
        chain.update(value);
        self.chain = chain.finalize().into();
        self.emitted_count = count;
        self.recent.insert(sequence, value);
        if self.recent.len() > 14_400 {
            let oldest = *self
                .recent
                .keys()
                .next()
                .ok_or_else(|| invalid("Video history eviction key missing"))?;
            self.recent.remove(&oldest);
            self.retired_through = oldest;
        }
        Ok(())
    }
}
impl ReplayHistory for VideoReplayHistory {
    fn emitted(
        &self,
        source_digest: [u8; 32],
        frozen_evidence: &[u8],
        receipt: &ReplayReceipt,
        surviving_dots: &[usize],
    ) -> Result<()> {
        if source_digest != self.source_digest
            || frozen_evidence != self.payload.as_slice()
            || surviving_dots.is_empty()
            || !matching_video_authority(&self.authority, &receipt.authority)
        {
            return Err(denied(
                "Video emitted evidence differs from original source",
            ));
        }
        let mut digest = Sha256::new();
        digest.update(b"ilium-video-surviving-dots-v1");
        digest.update(receipt.clip.hex().as_bytes());
        digest.update((receipt.frame as u64).to_le_bytes());
        digest.update(receipt.lease_sequence.to_le_bytes());
        for dot in surviving_dots {
            digest.update((*dot as u64).to_le_bytes());
        }
        let value: [u8; 32] = digest.finalize().into();
        self.records
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .commit(receipt.lease_sequence, value)
    }
}

pub struct NativeVideoHost {
    client: Client,
    quota: QuotaGroup,
    geometry: VideoGeometry,
    stop: StopToken,
    actor_wake: Arc<dyn Fn() + Send + Sync>,
    next_resource_id: u64,
    closed: bool,
    mode: AnimationMode,
    pre_render_video_attempted: bool,
    recording_ready: bool,
    active: BTreeMap<String, ActiveVideo>,
    pending: BTreeMap<u64, PendingOpen>,
    custody: BTreeMap<u64, HostRequest>,
    refusals: BTreeMap<u64, (HostRequest, ServiceValue)>,
    owned_images: BTreeSet<String>,
    image_closures: BTreeSet<String>,
    seeded_closures: BTreeSet<String>,
    _metadata: StorageAdmission,
}
impl NativeVideoHost {
    /// Geometry comes from the checked actual viewport, never a guest size.
    pub fn new(
        client: Client,
        quota: QuotaGroup,
        shape: Shape,
        mode: AnimationMode,
        stop: StopToken,
        actor_wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        if !client.quota_group().shares_root(&quota) {
            return Err(denied("foreign Video client"));
        }
        shape.layout().map_err(|error| {
            AnimationError::Runtime(format!("actual Video viewport shape: {error}"))
        })?;
        let width = shape
            .cell_width
            .checked_mul(2)
            .ok_or_else(|| invalid("viewport width"))?;
        let height = shape
            .cell_height
            .checked_mul(4)
            .ok_or_else(|| invalid("viewport height"))?;
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || (width as usize)
                .checked_mul(height as usize)
                .is_none_or(|pixels| pixels > 4 * 1024 * 1024)
        {
            return Err(AnimationError::Budget(
                "actual Video viewport geometry".into(),
            ));
        }
        let metadata = quota
            .reserve_external_storage(REGISTRY_BYTES)
            .map_err(|error| {
                AnimationError::Budget(format!("Video registry admission: {error:?}"))
            })?;
        Ok(Self {
            client,
            quota,
            geometry: VideoGeometry {
                width,
                height,
                format: VideoPixelFormat::Rgba8,
            },
            stop,
            actor_wake,
            next_resource_id: 1,
            closed: false,
            mode,
            pre_render_video_attempted: false,
            recording_ready: false,
            active: BTreeMap::new(),
            pending: BTreeMap::new(),
            custody: BTreeMap::new(),
            refusals: BTreeMap::new(),
            owned_images: BTreeSet::new(),
            image_closures: BTreeSet::new(),
            seeded_closures: BTreeSet::new(),
            _metadata: metadata,
        })
    }
    fn assign_resource_id(&mut self) -> Result<u64> {
        let id = self.next_resource_id;
        self.next_resource_id = id
            .checked_add(1)
            .ok_or_else(|| invalid("Video resource ID exhausted"))?;
        Ok(id)
    }
    fn limits(&self, max_pixels: u32, max_fps: u32) -> Result<(VideoLimits, FfmpegLimits)> {
        let pixels = (self.geometry.width as usize)
            .checked_mul(self.geometry.height as usize)
            .ok_or_else(|| invalid("Video viewport pixel overflow"))?;
        if pixels > max_pixels as usize || max_fps == 0 || max_fps > 120 {
            return Err(AnimationError::Budget("Video max_pixels/max_fps".into()));
        }
        let decoder = FfmpegLimits {
            executable: animation_sandbox::video_decoder_executable_path()?,
            sandbox: SandboxLimits::default(),
            fps_numerator: max_fps,
            fps_denominator: 1,
            maximum_frames: 10_000_000,
        };
        let service = VideoLimits {
            geometry: self.geometry,
            queue_frames: 12,
            lookahead: Duration::from_secs(2),
            maximum_pts: Duration::from_secs(86_400),
            maximum_frames: 10_000_000,
            maximum_frame_duration: Duration::from_secs(60),
        };
        Ok((service, decoder))
    }
    fn make_video(
        &self,
        instance: &PackageInstance,
        resource_id: u64,
        max_pixels: u32,
        max_fps: u32,
        input: VideoSource<'_>,
    ) -> Result<NativeVideo> {
        let (limits, decoder_limits) = self.limits(max_pixels, max_fps)?;
        let (authority, authorization) = instance.native_video_authorization(resource_id)?;
        let verified = match input {
            VideoSource::Bundle(bytes) => VerifiedVideoInput::from_encoded(
                &self.client,
                authorization.as_ref(),
                authority,
                resource_id,
                bytes,
                bytes.len(),
                &self.stop,
            )?,
            VideoSource::Resource(resource) => VerifiedVideoInput::from_resource(
                &self.client,
                authorization.as_ref(),
                authority,
                resource_id,
                resource,
                &self.stop,
            )?,
        };
        let factory: Arc<dyn VideoDecoderFactory> = Arc::new(FfmpegDecoderFactory::new(
            self.quota.clone(),
            decoder_limits,
        )?);
        NativeVideo::start_with_wake(
            self.client.clone(),
            verified,
            authorization,
            factory,
            limits,
            self.stop.child(),
            Arc::clone(&self.actor_wake),
        )
    }
    fn result(&self, instance: &PackageInstance, value: Value) -> Result<ServiceValue> {
        ServiceValue::copy_from_host(
            &value,
            &[],
            &BTreeMap::new(),
            instance.engine_limits(),
            self.quota.clone(),
        )
    }
    fn refusal_value(
        &self,
        instance: &PackageInstance,
        error: &AnimationError,
    ) -> Result<ServiceValue> {
        let code = match error {
            AnimationError::PermissionDenied(_) => "permission_denied",
            AnimationError::Budget(_) => "budget_exceeded",
            _ => "video_failed",
        };
        self.result(
            instance,
            json!({"ok":false,"error":{"code":code,"message":"Native Video request was refused."}}),
        )
    }
    fn open_value(&self, instance: &PackageInstance, resource_id: u64) -> Result<ServiceValue> {
        let id = format!("video-{resource_id}");
        self.result(
            instance,
            json!({"ok":true,"value":{
                "id":id,"kind":"media.video","revision":1,
                "status":{"state":"preparing"},"latest":null
            }}),
        )
    }
    fn refuse(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
        error: &AnimationError,
    ) -> Result<()> {
        let value = self.refusal_value(instance, error)?;
        let request_id = request.id;
        self.refusals.insert(request_id, (request, value));
        let (request, value) = self
            .refusals
            .get(&request_id)
            .ok_or_else(|| invalid("Video refusal custody lost"))?;
        match instance.complete_baseline_media(request, value.clone()) {
            Ok(CompletionState::Delivered) => {
                self.refusals.remove(&request_id);
                self.custody.remove(&request_id);
                Ok(())
            }
            Ok(_) => {
                self.closed = true;
                Ok(())
            }
            Err(error) => {
                self.closed = true;
                Err(error)
            }
        }
    }
    pub fn dispatch(
        &mut self,
        instance: &mut PackageInstance,
        assets: &NativeAssetHost,
        http: &mut NativeHttpHost,
        drawing: &mut NativeDrawHost,
        request: HostRequest,
    ) -> Result<Option<HostRequest>> {
        let handled = matches!(
            request.method.as_str(),
            "media.video.open" | "media.video.pause" | "media.video.seek" | "media.video.close"
        ) || (request.method == "media.images.close"
            && request
                .payload
                .metadata()
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.starts_with("video-image-")));
        if !handled {
            return Ok(Some(request));
        }
        if self.custody.contains_key(&request.id) {
            return Err(invalid("duplicate original Video request"));
        }
        self.custody.insert(request.id, request.clone());
        let method = request.method.clone();
        match method.as_str() {
            "media.video.open" => self.open(instance, assets, http, request)?,
            "media.video.pause" | "media.video.seek" | "media.video.close" => {
                self.control(instance, request)?
            }
            "media.images.close"
                if request
                    .payload
                    .metadata()
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id.starts_with("video-image-")) =>
            {
                self.close_image(instance, drawing, request)?
            }
            _ => return Err(invalid("Video dispatcher method mismatch")),
        }
        Ok(None)
    }
    fn open(
        &mut self,
        instance: &mut PackageInstance,
        assets: &NativeAssetHost,
        http: &mut NativeHttpHost,
        request: HostRequest,
    ) -> Result<()> {
        if let Err(error) = instance.check_baseline_media(&request) {
            return self.refuse(instance, request, &error);
        }
        if self.closed
            || self.stop.is_stopped()
            || self.active.len() + self.pending.len() >= MAX_VIDEO_HANDLES
            || self.pending.len() >= MAX_PENDING
        {
            return self.refuse(
                instance,
                request,
                &AnimationError::Budget("Video owner capacity".into()),
            );
        }
        if self.mode == AnimationMode::PreRendered {
            self.pre_render_video_attempted = true;
        }
        let options = match OpenOptions::parse(&request) {
            Ok(options) => options,
            Err(error) => return self.refuse(instance, request, &error),
        };
        if let Err(error) = self.limits(options.max_pixels, options.max_fps) {
            return self.refuse(instance, request, &error);
        }
        let resource_id = self.assign_resource_id()?;
        match options.source {
            SourceChoice::Asset {
                projection,
                relative_path,
            } => {
                let selection = match assets.video_asset(instance, &projection, &relative_path) {
                    Ok(selection) => selection,
                    Err(error) => return self.refuse(instance, request, &error),
                };
                match selection {
                    VideoAssetSelection::Bundle(bytes) => {
                        let started = self.make_video(
                            instance,
                            resource_id,
                            options.max_pixels,
                            options.max_fps,
                            VideoSource::Bundle(bytes),
                        );
                        let mut pending = PendingOpen {
                            request,
                            operation: None,
                            receipt: None,
                            retention: None,
                            acquired: None,
                            video: None,
                            result: None,
                            resource_id,
                            max_pixels: options.max_pixels,
                            max_fps: options.max_fps,
                            selected_cancellation: None,
                            http_stop: None,
                            lineage: Vec::new(),
                            uncertain: false,
                        };
                        let request_id = pending.request.id;
                        let value = match started {
                            Ok(video) => {
                                pending.video = Some(video);
                                self.open_value(instance, resource_id)
                            }
                            Err(error) => self.refusal_value(instance, &error),
                        };
                        match value {
                            Ok(value) => pending.result = Some(value),
                            Err(error) => {
                                if let Some(video) = &pending.video {
                                    video.cancel();
                                }
                                pending.uncertain = true;
                                self.closed = true;
                                self.pending.insert(request_id, pending);
                                return Err(error);
                            }
                        }
                        self.pending.insert(request_id, pending);
                        self.publish_open(instance, request_id)
                    }
                    VideoAssetSelection::Selected {
                        request_id,
                        resource,
                    } => self.open_selected(
                        instance,
                        request,
                        SelectedVideoSource {
                            request_id,
                            resource,
                            relative_path,
                        },
                        resource_id,
                        VideoDecodeLimits {
                            max_pixels: options.max_pixels,
                            max_fps: options.max_fps,
                        },
                    ),
                }
            }
            SourceChoice::Url(url) => self.open_http(
                instance,
                http,
                request,
                url,
                resource_id,
                VideoDecodeLimits {
                    max_pixels: options.max_pixels,
                    max_fps: options.max_fps,
                },
            ),
        }
    }
    fn refuse_code(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
        code: &'static str,
        message: &'static str,
    ) -> Result<()> {
        let value = self.result(
            instance,
            json!({"ok":false,"error":{"code":code,"message":message}}),
        )?;
        let request_id = request.id;
        self.refusals.insert(request_id, (request, value));
        let (request, value) = self
            .refusals
            .get(&request_id)
            .ok_or_else(|| invalid("Video refusal custody lost"))?;
        match instance.complete_baseline_media(request, value.clone()) {
            Ok(CompletionState::Delivered) => {
                self.refusals.remove(&request_id);
                self.custody.remove(&request_id);
                Ok(())
            }
            Ok(_) => {
                self.closed = true;
                Ok(())
            }
            Err(error) => {
                self.closed = true;
                Err(error)
            }
        }
    }
    fn open_selected(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
        source: SelectedVideoSource,
        resource_id: u64,
        limits: VideoDecodeLimits,
    ) -> Result<()> {
        let SelectedVideoSource {
            request_id,
            resource,
            relative_path,
        } = source;
        let VideoDecodeLimits {
            max_pixels,
            max_fps,
        } = limits;
        let reservation = match self.client.try_reserve(
            Lane::Io,
            JobCost {
                input_bytes: 16 * 1024,
                result_bytes: SELECTED_MAX_BYTES + 256 * 1024,
            },
        ) {
            Ok(reservation) => reservation,
            Err(error) => {
                return self.refuse(
                    instance,
                    request,
                    &AnimationError::Budget(format!("Video selected admission: {error:?}")),
                )
            }
        };
        let need = match resource.operation_need(false) {
            Ok(need) => need,
            Err(error) => return self.refuse(instance, request, &error),
        };
        let operation = match instance.dispatch_service(
            request.clone(),
            &operation_demand(&request_id),
            vec![need],
        ) {
            Ok(operation) => operation,
            Err(error) => return self.refuse(instance, request, &error),
        };
        let cancellation = StorageCancellation::default();
        let job = SelectedVideoJob {
            resource,
            relative_path,
            cancellation: cancellation.clone(),
            quota: self.quota.clone(),
        };
        let id = request.id;
        self.pending.insert(
            id,
            PendingOpen {
                request,
                operation: Some(operation),
                receipt: None,
                retention: None,
                acquired: None,
                video: None,
                result: None,
                resource_id,
                max_pixels,
                max_fps,
                selected_cancellation: Some(cancellation),
                http_stop: None,
                lineage: Vec::new(),
                uncertain: false,
            },
        );
        let pending = self
            .pending
            .get(&id)
            .ok_or_else(|| invalid("Video selected operation custody"))?;
        let submitted = instance.commit_service(
            pending
                .operation
                .as_ref()
                .ok_or_else(|| invalid("Video selected ticket"))?,
            || reservation.submit(job),
        );
        match submitted {
            Ok(Ok(receipt)) => {
                self.pending
                    .get_mut(&id)
                    .ok_or_else(|| invalid("Video selected receipt custody"))?
                    .receipt = Some(AcquisitionReceipt::Selected(receipt));
                Ok(())
            }
            Ok(Err(rejected)) => {
                // The reservation returned its unstarted input; no selected
                // disk effect can be inferred from a rejected submission.
                drop(rejected);
                let error = AnimationError::Budget("Video selected IO submission refused".into());
                let value = self.refusal_value(instance, &error)?;
                self.pending
                    .get_mut(&id)
                    .ok_or_else(|| invalid("Video selected rejection custody"))?
                    .result = Some(value);
                self.publish_open(instance, id)
            }
            Err(error) => self.unissued_refusal(instance, id, error),
        }
    }
    fn unissued_refusal(
        &mut self,
        instance: &mut PackageInstance,
        id: u64,
        error: AnimationError,
    ) -> Result<()> {
        let pending = self
            .pending
            .get(&id)
            .ok_or_else(|| invalid("Video unissued custody"))?;
        if let Some(operation) = &pending.operation {
            if let Err(settle) = instance.settle_service(operation) {
                self.closed = true;
                return Err(settle);
            }
        }
        let pending = self
            .pending
            .remove(&id)
            .ok_or_else(|| invalid("Video unissued removal"))?;
        self.refuse(instance, pending.request, &error)
    }
    fn open_http(
        &mut self,
        instance: &mut PackageInstance,
        http: &mut NativeHttpHost,
        request: HostRequest,
        url: String,
        resource_id: u64,
        limits: VideoDecodeLimits,
    ) -> Result<()> {
        let VideoDecodeLimits {
            max_pixels,
            max_fps,
        } = limits;
        let options = http_options(url, HTTP_MAX_BYTES, "bytes");
        if let Err(error) = options.validate() {
            return self.refuse(instance, request, &error);
        }
        let preparation = match http.video_preparation(instance, &request, options.max_bytes) {
            Ok(preparation) => preparation,
            Err(error) => return self.refuse(instance, request, &error),
        };
        let (client, dns) = http.video_transport();
        if !client.quota_group().shares_root(&self.quota) {
            return self.refuse(instance, request, &denied("foreign HTTP Video client"));
        }
        let cost = JobCost {
            input_bytes: 64 * 1024,
            result_bytes: HTTP_MAX_BYTES + 256 * 1024,
        };
        let reservation = match client.try_reserve(Lane::Io, cost) {
            Ok(reservation) => reservation,
            Err(error) => {
                return self.refuse(
                    instance,
                    request,
                    &AnimationError::Budget(format!("Video HTTP admission: {error:?}")),
                )
            }
        };
        let need = match OperationNeed::http_preflight(&options.url, HttpMethod::Get) {
            Ok(need) => need,
            Err(error) => {
                return self.refuse(
                    instance,
                    request,
                    &AnimationError::PermissionDenied(format!("Video HTTP scope: {error}")),
                )
            }
        };
        let operation = match instance.dispatch_video_http_service(request.clone(), need) {
            Ok(operation) => operation,
            Err(error) => return self.refuse(instance, request, &error),
        };
        let id = request.id;
        self.pending.insert(
            id,
            PendingOpen {
                request: request.clone(),
                operation: Some(operation),
                receipt: None,
                retention: None,
                acquired: None,
                video: None,
                result: None,
                resource_id,
                max_pixels,
                max_fps,
                selected_cancellation: None,
                http_stop: None,
                lineage: Vec::new(),
                uncertain: false,
            },
        );
        let operation = self
            .pending
            .get(&id)
            .and_then(|pending| pending.operation.as_ref())
            .ok_or_else(|| invalid("Video HTTP operation custody"))?;
        let factory = match instance.http_authority_factory(operation, None) {
            Ok(factory) => factory,
            Err(error) => return self.unissued_refusal(instance, id, error),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let job = HttpVideoJob {
            options,
            request,
            factory,
            client: client.clone(),
            dns,
            stop: Arc::clone(&stop),
        };
        self.pending
            .get_mut(&id)
            .ok_or_else(|| invalid("Video HTTP stop custody"))?
            .http_stop = Some(stop);
        let operation = self
            .pending
            .get(&id)
            .and_then(|pending| pending.operation.as_ref())
            .ok_or_else(|| invalid("Video HTTP issue custody"))?;
        let pending_job = Cell::new(Some(job));
        let submitted = instance.commit_service(operation, || {
            http.record_video_preparation(preparation);
            let job = pending_job.take().expect("one Video HTTP issue");
            match reservation.submit(job) {
                Ok(receipt) => Some(receipt),
                Err(rejected) => {
                    pending_job.set(Some(rejected.value));
                    None
                }
            }
        });
        match submitted {
            Ok(Some(receipt)) => {
                self.pending
                    .get_mut(&id)
                    .ok_or_else(|| invalid("Video HTTP receipt custody"))?
                    .receipt = Some(AcquisitionReceipt::Http(receipt));
                Ok(())
            }
            Ok(None) => {
                let rejected_job = pending_job
                    .take()
                    .ok_or_else(|| invalid("Video HTTP rejected job custody"))?;
                let value = self.refusal_value(
                    instance,
                    &AnimationError::Budget("Video HTTP submission refused".into()),
                )?;
                self.pending
                    .get_mut(&id)
                    .ok_or_else(|| invalid("Video HTTP rejection custody"))?
                    .result = Some(value);
                let result = self.publish_open(instance, id);
                drop(rejected_job);
                result
            }
            Err(error) => {
                let unissued_job = pending_job.take();
                let result = self.unissued_refusal(instance, id, error);
                drop(unissued_job);
                result
            }
        }
    }
    fn publish_open(&mut self, instance: &mut PackageInstance, id: u64) -> Result<()> {
        let mut pending = self
            .pending
            .remove(&id)
            .ok_or_else(|| invalid("Video pending open lost"))?;
        let published = (|| -> Result<Option<CompletionState>> {
            if pending.result.is_none() {
                if self.mode == AnimationMode::PreRendered {
                    if let Some(operation) = &pending.operation {
                        pending.lineage = instance.video_replay_lineage(operation)?;
                    }
                }
                let acquired = match pending.acquired.take() {
                    Some(acquired) => acquired,
                    None => return Ok(None),
                };
                let source: Option<Arc<dyn BrokerVideoResource>> = match acquired {
                    AcquiredBytes::Selected(bytes) => Some(Arc::new(SelectedVideoBytes {
                        bytes,
                        quota: self.quota.clone(),
                    })),
                    AcquiredBytes::Http(bytes) => {
                        if !(200..300).contains(&bytes.status()) || bytes.as_bytes().is_empty() {
                            pending.result = Some(self.refusal_value(
                                instance,
                                &invalid("Video HTTP response status/body"),
                            )?);
                            None
                        } else {
                            Some(Arc::new(HttpVideoBytes {
                                bytes,
                                quota: self.quota.clone(),
                            }))
                        }
                    }
                };
                if let Some(source) = source {
                    match self.make_video(
                        instance,
                        pending.resource_id,
                        pending.max_pixels,
                        pending.max_fps,
                        VideoSource::Resource(source),
                    ) {
                        Ok(video) => {
                            pending.video = Some(video);
                            pending.result = Some(self.open_value(instance, pending.resource_id)?);
                        }
                        Err(error) => pending.result = Some(self.refusal_value(instance, &error)?),
                    }
                }
            }
            let value = pending
                .result
                .as_ref()
                .ok_or_else(|| invalid("Video open result missing"))?
                .clone();
            let delivered = if let Some(operation) = &pending.operation {
                instance.complete_authorized(operation, value)?
            } else {
                instance.complete_baseline_media(&pending.request, value)?
            };
            Ok(Some(delivered))
        })();
        match published {
            Ok(None) => {
                self.pending.insert(id, pending);
                Ok(())
            }
            Ok(Some(CompletionState::Delivered)) => {
                self.custody.remove(&id);
                if let Some(video) = pending.video.take() {
                    let key = format!("video-{}", pending.resource_id);
                    if self.active.contains_key(&key) {
                        self.closed = true;
                        self.pending.insert(id, pending);
                        return Err(invalid("duplicate original Video handle"));
                    }
                    self.active.insert(
                        key,
                        ActiveVideo {
                            video,
                            _input_retention: pending.retention.take(),
                            resource_id: pending.resource_id,
                            revision: 1,
                            current_image: None,
                            staged_old_image: None,
                            latest: None,
                            latest_sequence: None,
                            last_phase: None,
                            closing: None,
                            lineage: pending.lineage.clone(),
                            recorded_token: None,
                            recorded_history: None,
                        },
                    );
                }
                Ok(())
            }
            Ok(Some(_)) => {
                pending.uncertain = true;
                if let Some(video) = &pending.video {
                    video.cancel();
                }
                self.closed = true;
                self.pending.insert(id, pending);
                Ok(())
            }
            Err(error) => {
                pending.uncertain = true;
                if let Some(video) = &pending.video {
                    video.cancel();
                }
                self.closed = true;
                self.pending.insert(id, pending);
                Err(error)
            }
        }
    }
    /// Called only on the existing finite native actor wake. Pending and lost
    /// receipts are retained; no new timer or synchronous decoder read occurs.
    pub fn on_completion_wake(&mut self, instance: &mut PackageInstance) -> Result<()> {
        let ids: Vec<u64> = self.pending.keys().copied().collect();
        for id in ids {
            let poll = match self
                .pending
                .get_mut(&id)
                .and_then(|pending| pending.receipt.as_mut())
            {
                Some(AcquisitionReceipt::Selected(receipt)) => match receipt.try_take() {
                    JobPoll::Pending => continue,
                    JobPoll::Lost | JobPoll::Taken => {
                        self.closed = true;
                        self.pending
                            .get_mut(&id)
                            .ok_or_else(|| invalid("Video lost selected receipt"))?
                            .uncertain = true;
                        continue;
                    }
                    JobPoll::Ready(outcome) => {
                        let (outcome, retention) = outcome.into_parts();
                        let acquired = match outcome {
                            JobOutcome::Finished(Ok(bytes)) => Some(AcquiredBytes::Selected(bytes)),
                            _ => None,
                        };
                        Some((acquired, retention))
                    }
                },
                Some(AcquisitionReceipt::Http(receipt)) => match receipt.try_take() {
                    JobPoll::Pending => continue,
                    JobPoll::Lost | JobPoll::Taken => {
                        self.closed = true;
                        self.pending
                            .get_mut(&id)
                            .ok_or_else(|| invalid("Video lost HTTP receipt"))?
                            .uncertain = true;
                        continue;
                    }
                    JobPoll::Ready(outcome) => {
                        let (outcome, retention) = outcome.into_parts();
                        let acquired = match outcome {
                            JobOutcome::Finished(Ok(bytes)) => {
                                Some(AcquiredBytes::Http(Arc::new(bytes)))
                            }
                            _ => None,
                        };
                        Some((acquired, retention))
                    }
                },
                None => None,
            };
            let Some((acquired, retention)) = poll else {
                continue;
            };
            let failure = if acquired.is_none() {
                Some(self.refusal_value(instance, &invalid("Video acquisition did not finish"))?)
            } else {
                None
            };
            let pending = self
                .pending
                .get_mut(&id)
                .ok_or_else(|| invalid("Video acquired custody"))?;
            pending.receipt = None;
            pending.retention = Some(retention);
            pending.acquired = acquired;
            pending.result = failure;
            if self.closed {
                // The actual finite result has arrived. Do not start a
                // decoder after revocation; settle its original ticket now.
                if let Some(operation) = &pending.operation {
                    instance.settle_service(operation)?;
                }
                self.pending.remove(&id);
                continue;
            }
            self.publish_open(instance, id)?;
        }
        if !self.closed {
            self.finish_closes(instance)?;
        }
        Ok(())
    }
    fn deliver_control(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
        value: ServiceValue,
    ) -> Result<()> {
        let id = request.id;
        self.refusals.insert(id, (request, value));
        let (request, value) = self
            .refusals
            .get(&id)
            .ok_or_else(|| invalid("Video control custody lost"))?;
        match instance.complete_baseline_media(request, value.clone()) {
            Ok(CompletionState::Delivered) => {
                self.refusals.remove(&id);
                self.custody.remove(&id);
                Ok(())
            }
            Ok(_) => {
                self.closed = true;
                Ok(())
            }
            Err(error) => {
                self.closed = true;
                Err(error)
            }
        }
    }
    fn control(&mut self, instance: &mut PackageInstance, request: HostRequest) -> Result<()> {
        if let Err(error) = instance.check_baseline_media(&request) {
            return self.refuse(instance, request, &error);
        }
        let allowed: &[&str] = if request.method == "media.video.seek" {
            &["id", "kind", "time_seconds"]
        } else if request.method == "media.video.pause" {
            &["id", "kind", "paused"]
        } else {
            &["id", "kind"]
        };
        let fields = match fields(&request, allowed) {
            Ok(fields) => fields,
            Err(error) => return self.refuse(instance, request, &error),
        };
        let id = match handle(fields) {
            Ok(id) => id.to_owned(),
            Err(error) => return self.refuse(instance, request, &error),
        };
        if !self.active.contains_key(&id) {
            return self.refuse(instance, request, &denied("unknown original Video handle"));
        }
        if self
            .active
            .get(&id)
            .is_some_and(|active| active.closing.is_some())
        {
            return self.refuse_code(
                instance,
                request,
                "closed_handle",
                "Video close is pending.",
            );
        }
        if request.method == "media.video.close" {
            let active = self
                .active
                .get_mut(&id)
                .ok_or_else(|| invalid("Video close owner vanished"))?;
            active.closing = Some(request);
            active.video.cancel();
            return self.finish_closes(instance);
        }
        let action = if request.method == "media.video.pause" {
            let paused = match fields.get("paused").and_then(Value::as_bool) {
                Some(paused) => paused,
                None => return self.refuse(instance, request, &invalid("Video pause boolean")),
            };
            let active = self
                .active
                .get(&id)
                .ok_or_else(|| invalid("Video pause owner"))?;
            if paused {
                active.video.pause()
            } else {
                active.video.resume()
            }
        } else {
            let seconds = match fields.get("time_seconds").and_then(Value::as_f64) {
                Some(value) if value.is_finite() && (0.0..=86_400.0).contains(&value) => value,
                _ => return self.refuse(instance, request, &invalid("Video seek time")),
            };
            let active = self
                .active
                .get(&id)
                .ok_or_else(|| invalid("Video seek owner"))?;
            let status = match active.video.status() {
                Ok(status) => status,
                Err(error) => return self.refuse(instance, request, &error),
            };
            match status.info {
                None => {
                    return self.refuse_code(
                        instance,
                        request,
                        "not_ready",
                        "Video decoder has not opened.",
                    )
                }
                Some(info) if !info.seekable => {
                    return self.refuse_code(
                        instance,
                        request,
                        "unsupported",
                        "The admitted pipe decoder does not support seek.",
                    )
                }
                Some(_) => {}
            }
            let position =
                Duration::try_from_secs_f64(seconds).map_err(|_| invalid("Video seek time"))?;
            active.video.seek(position).map(|_| ())
        };
        if let Err(error) = action {
            return self.refuse(instance, request, &error);
        }
        let active = self
            .active
            .get_mut(&id)
            .ok_or_else(|| invalid("Video control owner"))?;
        active.revision = active
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("Video control revision exhausted"))?;
        let status = active.video.status()?;
        active.last_phase = Some(status.phase);
        let snapshot = Self::snapshot_value(&id, active, status);
        let response = self.result(instance, json!({"ok":true,"value":snapshot}))?;
        self.deliver_control(instance, request, response)
    }
    fn snapshot_status(status: VideoStatus) -> Value {
        match status.phase {
            VideoPhase::Opening | VideoPhase::Stopping => json!({"state":"preparing"}),
            VideoPhase::Playing | VideoPhase::Paused | VideoPhase::Ended => {
                json!({"state":"ready"})
            }
            VideoPhase::Exited => json!({"state":"closed"}),
            VideoPhase::Failed => json!({"state":"error","error":{
                "code":"video_failed","message":"The native Video decoder failed."
            }}),
        }
    }
    fn snapshot_value(id: &str, active: &ActiveVideo, status: VideoStatus) -> Value {
        json!({"id":id,"kind":"media.video","revision":active.revision,
            "status":Self::snapshot_status(status),"latest":active.latest.clone()})
    }
    fn finish_closes(&mut self, instance: &mut PackageInstance) -> Result<()> {
        let ids: Vec<String> = self
            .active
            .iter()
            .filter(|(_, active)| {
                active.closing.is_some() && active.video.physical_exit().is_some()
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let active = self
                .active
                .get(&id)
                .ok_or_else(|| invalid("Video close owner lost"))?;
            let request = active
                .closing
                .as_ref()
                .ok_or_else(|| invalid("Video close request lost"))?;
            let revision = active
                .revision
                .checked_add(1)
                .ok_or_else(|| invalid("Video close revision exhausted"))?;
            let result = self.result(
                instance,
                json!({"ok":true,"value":{
                    "id":id,"kind":"media.video","revision":revision,"status":{"state":"closed"}
                }}),
            )?;
            match instance.complete_baseline_media(request, result) {
                Ok(CompletionState::Delivered) => {
                    self.custody.remove(&request.id);
                    self.active.remove(&id);
                }
                Ok(_) => self.closed = true,
                Err(error) => {
                    self.closed = true;
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    fn close_image(
        &mut self,
        instance: &mut PackageInstance,
        drawing: &mut NativeDrawHost,
        request: HostRequest,
    ) -> Result<()> {
        if let Err(error) = instance.check_baseline_media(&request) {
            return self.refuse(instance, request, &error);
        }
        let fields = match fields(&request, &["id", "kind"]) {
            Ok(fields) => fields,
            Err(error) => return self.refuse(instance, request, &error),
        };
        if fields.get("kind").and_then(Value::as_str) != Some("image") {
            return self.refuse(instance, request, &denied("foreign Video image kind"));
        }
        let id = match fields.get("id").and_then(Value::as_str) {
            Some(id)
                if self.owned_images.contains(id) && drawing.video_image_handle(id).is_some() =>
            {
                id.to_owned()
            }
            _ => return self.refuse(instance, request, &denied("unknown original Video image")),
        };
        let value = self.result(instance, json!({"ok":true,"value":null}))?;
        let request_id = request.id;
        self.refusals.insert(request_id, (request, value));
        let (request, value) = self
            .refusals
            .get(&request_id)
            .ok_or_else(|| invalid("Video image close custody"))?;
        match instance.complete_baseline_media(request, value.clone()) {
            Ok(CompletionState::Delivered) => {
                drawing.release_video_image(&id)?;
                self.owned_images.remove(&id);
                for active in self.active.values_mut() {
                    if active.current_image.as_deref() == Some(&id) {
                        active.current_image = None;
                        active.latest = None;
                        active.latest_sequence = None;
                        active.revision = active
                            .revision
                            .checked_add(1)
                            .ok_or_else(|| invalid("Video image close revision"))?;
                    }
                }
                self.refusals.remove(&request_id);
                self.custody.remove(&request_id);
                Ok(())
            }
            Ok(_) => {
                self.closed = true;
                Ok(())
            }
            Err(error) => {
                self.closed = true;
                Err(error)
            }
        }
    }
    /// Actor-frame publication only. `latest_at` observes a due decoded Arc;
    /// it does no decoding or network/disk I/O on this UI path.
    pub fn snapshots(
        &mut self,
        instance: &mut PackageInstance,
        drawing: &mut NativeDrawHost,
        position: Duration,
    ) -> Result<ServiceValue> {
        if self.closed {
            return Err(denied("Video owner retired"));
        }
        let ids: Vec<String> = self.active.keys().cloned().collect();
        for id in ids {
            let (resource_id, phase, sequence, due) = {
                let active = self
                    .active
                    .get(&id)
                    .ok_or_else(|| invalid("Video snapshot owner"))?;
                let status = active.video.status()?;
                let due = if active.closing.is_some() {
                    None
                } else if self.mode == AnimationMode::PreRendered && self.recording_ready {
                    active.video.wait_recorded_at(
                        position,
                        Instant::now() + Duration::from_secs(10),
                        &self.stop,
                    )?
                } else {
                    active.video.latest_at(position)?
                };
                (
                    active.resource_id,
                    status.phase,
                    active.latest_sequence,
                    due,
                )
            };
            let is_new = due
                .as_ref()
                .is_some_and(|frame| sequence != Some((frame.generation, frame.sequence)));
            if is_new {
                let frame = due.ok_or_else(|| invalid("Video due frame lost"))?;
                if self.owned_images.len() >= 64 {
                    return Err(AnimationError::Budget("Video native image registry".into()));
                }
                let token = self
                    .active
                    .get(&id)
                    .and_then(|active| active.recorded_token);
                let image = drawing.retain_video_frame(instance, &frame, resource_id, token)?;
                let key = image
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("Video registry descriptor"))?
                    .to_owned();
                self.owned_images.insert(key.clone());
                let active = self
                    .active
                    .get_mut(&id)
                    .ok_or_else(|| invalid("Video snapshot owner lost"))?;
                active.staged_old_image = active.current_image.replace(key);
                active.revision = active
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| invalid("Video frame revision exhausted"))?;
                active.latest_sequence = Some((frame.generation, frame.sequence));
                active.latest = Some(json!({"image":image,
                    "time_seconds":frame.stamp.pts.as_secs_f64(),
                    "revision":active.revision}));
            }
            let active = self
                .active
                .get_mut(&id)
                .ok_or_else(|| invalid("Video phase owner lost"))?;
            if active.last_phase != Some(phase) {
                active.revision = active
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| invalid("Video phase revision exhausted"))?;
                active.last_phase = Some(phase);
            }
        }
        let mut values = Vec::with_capacity(self.active.len() + self.image_closures.len());
        for (id, active) in &self.active {
            values.push(Self::snapshot_value(id, active, active.video.status()?));
        }
        for id in &self.image_closures {
            values.push(json!({"id":id,"kind":"image","revision":1,"status":{"state":"closed"}}));
        }
        if values.len() > 64 {
            return Err(AnimationError::Budget(
                "Video service snapshot inventory".into(),
            ));
        }
        self.seeded_closures = self.image_closures.clone();
        ServiceValue::copy_from_host(
            &Value::Array(values),
            &[],
            &BTreeMap::new(),
            instance.engine_limits(),
            self.quota.clone(),
        )
    }
    /// Call after the helper's typed frame-seed copy and ACK, never merely
    /// after the metadata was constructed.
    pub fn acknowledge_seeded_snapshots(&mut self, drawing: &mut NativeDrawHost) -> Result<()> {
        for id in std::mem::take(&mut self.seeded_closures) {
            self.image_closures.remove(&id);
        }
        for active in self.active.values_mut() {
            if let Some(old) = active.staged_old_image.take() {
                drawing.release_video_image(&old)?;
                self.owned_images.remove(&old);
                self.image_closures.insert(old);
            }
        }
        Ok(())
    }
    pub fn recorded_count(&self) -> usize {
        if self.mode == AnimationMode::PreRendered {
            self.active.len()
        } else {
            0
        }
    }
    pub fn recording_ready(&self) -> bool {
        self.recording_ready
    }
    /// Build compact evidence from the original admitted immutable input.
    /// The 32 MiB URL and 8 MiB selected caps remain in their acquisition
    /// owners; the existing Video input ceiling is 256 MiB. Hashing does not
    /// allocate a replacement source body.
    pub fn recording_evidence(&mut self) -> Result<Vec<NativeEvidenceInput>> {
        if self.mode != AnimationMode::PreRendered
            || self.closed
            || self.active.is_empty()
            || self.active.len() > MAX_VIDEO_HANDLES
            || self.active.values().any(|active| active.closing.is_some())
        {
            return Err(denied("recorded Video sources are not active and finite"));
        }
        let ids: Vec<String> = self.active.keys().cloned().collect();
        let mut entries = Vec::with_capacity(ids.len());
        for id in ids {
            let active = self
                .active
                .get(&id)
                .ok_or_else(|| invalid("recorded Video owner"))?;
            active.video.status()?;
            let input = active.video.retained_input();
            if !input.quota_group().shares_root(&self.quota)
                || input.maximum_bytes() == 0
                || input.maximum_bytes() > 256 * 1024 * 1024
            {
                return Err(denied(
                    "recorded Video input is not original finite custody",
                ));
            }
            let mut digest = Sha256::new();
            let mut buffer = [0u8; 65_536];
            let mut offset = 0u64;
            loop {
                let count = input.read_at(offset, &mut buffer, &self.stop)?;
                if count == 0 {
                    break;
                }
                digest.update(&buffer[..count]);
                offset = offset
                    .checked_add(count as u64)
                    .ok_or_else(|| invalid("recorded Video input offset"))?;
            }
            if offset != input.maximum_bytes() as u64 {
                return Err(invalid("recorded Video source length changed"));
            }
            let source_digest: [u8; 32] = digest.finalize().into();
            let payload = serde_json::to_vec(&json!({
                "kind":"ilium.recorded_video.v1",
                "resource_id":active.resource_id,
                "source_sha256":source_digest.iter().map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                "source_bytes":offset,
                "width":self.geometry.width,
                "height":self.geometry.height,
            }))?;
            let admission = self
                .quota
                .reserve_external_storage(14_400 * 96)
                .map_err(|error| {
                    AnimationError::Budget(format!("Video emitted history admission: {error:?}"))
                })?;
            let authority = input.authority().clone();
            let history = Arc::new(VideoReplayHistory {
                _input: input,
                source_digest,
                payload: payload.clone(),
                authority,
                records: Mutex::new(VideoHistoryState::default()),
                _admission: admission,
            });
            let lineage = active.lineage.clone();
            let active = self
                .active
                .get_mut(&id)
                .ok_or_else(|| invalid("recorded Video owner changed"))?;
            active.recorded_history = Some(Arc::clone(&history));
            let history: Arc<dyn ReplayHistory> = history;
            entries.push(NativeEvidenceInput {
                source_digest,
                lineage,
                payload,
                history,
            });
        }
        Ok(entries)
    }
    /// `FrozenEvidence` is the only token issuer. Resolve by source digest and
    /// exact payload because its token-key order is unrelated to input order.
    pub fn bind_recording(&mut self, evidence: &FrozenEvidence) -> Result<()> {
        if self.mode != AnimationMode::PreRendered || self.active.is_empty() {
            return Err(denied("recorded Video binding mode"));
        }
        for active in self.active.values_mut() {
            let history = active
                .recorded_history
                .as_ref()
                .ok_or_else(|| denied("recorded Video history missing"))?;
            active.recorded_token = Some(
                evidence
                    .token_for_source(history.source_digest, &history.payload)?
                    .evidence_key(),
            );
        }
        self.recording_ready = true;
        Ok(())
    }
    /// Native helper retirement and decoder exit are separate proofs. No
    /// input/worker/image admission is dropped while either can still use it.
    pub fn release_terminal_after_helper_retirement(
        &mut self,
        drawing: &mut NativeDrawHost,
    ) -> Result<()> {
        if !self.closed {
            return Ok(());
        }
        if self
            .active
            .values()
            .any(|active| active.video.physical_exit().is_none())
            || self.pending.values().any(|pending| {
                pending
                    .video
                    .as_ref()
                    .is_some_and(|video| video.physical_exit().is_none())
            })
            || self
                .pending
                .values()
                .any(|pending| pending.receipt.is_some())
        {
            return Ok(());
        }
        drawing.release_video_images_after_helper_retirement()?;
        self.active.clear();
        self.pending.clear();
        self.refusals.clear();
        self.custody.clear();
        self.owned_images.clear();
        self.image_closures.clear();
        self.seeded_closures.clear();
        Ok(())
    }
    pub fn is_drained(&self) -> bool {
        self.closed
            && self.active.is_empty()
            && self.pending.is_empty()
            && self.refusals.is_empty()
            && self.custody.is_empty()
            && self.owned_images.is_empty()
    }
    pub fn finite_work_drained(&self) -> bool {
        self.active
            .values()
            .all(|active| active.video.physical_exit().is_some())
            && self.pending.values().all(|pending| {
                pending.receipt.is_none()
                    && pending
                        .video
                        .as_ref()
                        .is_none_or(|video| video.physical_exit().is_some())
            })
    }
    /// Background preparation retirement only. Keep every original owner on a
    /// timed-out join; a decoder exit must be observed before helper release.
    pub fn retire_recorded_until(&mut self, deadline: Instant) -> Result<bool> {
        self.revoke();
        for active in self.active.values() {
            if active.video.close_until(deadline) != crate::native_video::CloseState::Joined {
                return Ok(false);
            }
        }
        for pending in self.pending.values() {
            if let Some(video) = &pending.video {
                if video.close_until(deadline) != crate::native_video::CloseState::Joined {
                    return Ok(false);
                }
            }
            if pending.receipt.is_some() {
                return Ok(false);
            }
        }
        Ok(self.finite_work_drained())
    }
    pub fn pre_render_video_attempted(&self) -> bool {
        self.pre_render_video_attempted
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    pub fn revoke(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.stop.stop();
        for pending in self.pending.values_mut() {
            if let Some(cancellation) = &pending.selected_cancellation {
                cancellation.cancel();
            }
            if let Some(stop) = &pending.http_stop {
                stop.store(true, Ordering::Release);
            }
            if let Some(receipt) = &pending.receipt {
                match receipt {
                    AcquisitionReceipt::Selected(receipt) => receipt.cancel(),
                    AcquisitionReceipt::Http(receipt) => receipt.cancel(),
                }
            }
            if let Some(video) = &pending.video {
                video.cancel();
            }
        }
        for active in self.active.values() {
            active.video.cancel();
        }
    }
}
