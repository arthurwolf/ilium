//! Original-bank source actor: CPU planning/parser -> authenticated raw IO -> CPU.
//! This module never waits for a bank callback, invents a script handle, or
//! considers a native transport suspension to be a provider failure.
use crate::{
    engine::{HostRequest, ServiceValue},
    error::{AnimationError, Result},
    http::{self, DnsResolver, HttpOptions},
    native_http_authority::NativeHttpAuthorityFactory,
    native_media::{MediaLimits, NativeMedia},
    permissions::{Channel, PermissionBroker},
    runtime::{PackageInstance, ServiceOperation},
    sources::{
        self, AdmittedSourceSnapshot, AdmittedSourceValue, NativeSourceClient,
        NativeSourceHeightfield, NativeSourceImage, SourceClock, SourceDemand, SourceDispatcher,
        SourceHandle, SourceHttpResponse, SourceRequest,
    },
};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, QuotaGroup, Receipt, Retained,
    Retention, StorageAdmission,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

fn invalid(text: &'static str) -> AnimationError {
    AnimationError::Runtime(text.into())
}
fn denied(text: &'static str) -> AnimationError {
    AnimationError::PermissionDenied(text.into())
}
fn charge(quota: &QuotaGroup, bytes: usize) -> Result<StorageAdmission> {
    quota
        .reserve_external_storage(bytes)
        .map_err(|_| AnimationError::Budget("native source actor admission".into()))
}
/// Closed native dispatcher set. No arbitrary service names or provider code.
#[derive(Debug, Clone)]
pub enum SourceCall {
    Feed(SourceDemand),
    Operation(SourceRequest),
}
impl SourceCall {
    pub(crate) fn recognized(method: &str) -> bool {
        matches!(
            method,
            "sources.series.open"
                | "sources.earthquakes.open"
                | "sources.aircraft.open"
                | "sources.boats.open"
                | "sources.chess.open"
                | "sources.weather.open"
                | "sources.geography.coastlines"
                | "sources.geography.elevation"
                | "sources.geography.project"
                | "sources.chess.discover"
                | "sources.wikipedia.search"
                | "sources.wikipedia.article"
                | "sources.osm.geocode"
                | "sources.osm.tile"
                | "sources.astronomy.catalogue"
                | "sources.astronomy.observe"
        )
    }
    pub(crate) fn parse(request: &HostRequest) -> Result<Self> {
        if !request.payload.arrays().is_empty() {
            return Err(invalid("source options cannot carry binary planes"));
        }
        let value = request.payload.metadata();
        let allowed: &[&str] = match request.method.as_str() {
            "sources.series.open" => &[
                "provider",
                "source_id",
                "max_samples",
                "interval_ms",
                "window_minutes",
            ],
            "sources.earthquakes.open" | "sources.aircraft.open" | "sources.boats.open" => &[
                "bounds",
                "max_entities",
                "max_hz",
                "fields",
                "credential",
                "provider",
            ],
            "sources.chess.open" => &["game_id", "max_hz"],
            "sources.weather.open" => &[
                "bounds",
                "max_entities",
                "max_hz",
                "fields",
                "credential",
                "provider",
                "layers",
                "image_width",
                "image_height",
                "max_frames",
                "max_tiles",
                "history_hours",
                "anchor_epoch_ms",
            ],
            "sources.geography.coastlines" => &["body", "bounds", "max_points", "seed"],
            "sources.geography.elevation" => &["body", "bounds", "width", "height", "seed"],
            "sources.geography.project" => &["latitude", "longitude", "projection"],
            "sources.chess.discover" => &["max_games"],
            "sources.wikipedia.search" | "sources.osm.geocode" => &["query", "max_results"],
            "sources.wikipedia.article" => &["title", "max_bytes", "max_images"],
            "sources.osm.tile" => &["x", "y", "zoom", "format"],
            "sources.astronomy.catalogue" => &["name", "max_stars"],
            "sources.astronomy.observe" => &["epoch_ms", "latitude", "longitude"],
            _ => return Err(invalid("closed native source method")),
        };
        if value
            .as_object()
            .is_none_or(|fields| fields.keys().any(|key| !allowed.contains(&key.as_str())))
        {
            return Err(invalid("unknown source option"));
        }

        let family = match request.method.as_str() {
            "sources.series.open" => "series",
            "sources.earthquakes.open" => "earthquakes",
            "sources.aircraft.open" => "aircraft",
            "sources.boats.open" => "boats",
            "sources.chess.open" => "chess",
            "sources.weather.open" => "weather",
            _ => "",
        };
        if !family.is_empty() {
            return Ok(Self::Feed(serde_json::from_value(
                json!({"family":family,"options":value}),
            )?));
        }
        let operation = match request.method.as_str() {
            "sources.geography.coastlines" => "geography_coastlines",
            "sources.geography.elevation" => "geography_elevation",
            "sources.geography.project" => "geography_project",
            "sources.chess.discover" => "chess_discover",
            "sources.wikipedia.search" => "wikipedia_search",
            "sources.wikipedia.article" => "wikipedia_article",
            "sources.osm.geocode" => "osm_geocode",
            "sources.osm.tile" => "osm_tile",
            "sources.astronomy.catalogue" => "astronomy_catalogue",
            "sources.astronomy.observe" => "astronomy_observe",
            _ => return Err(invalid("unsupported native source call")),
        };
        Ok(Self::Operation(serde_json::from_value(
            json!({"operation":operation,"options":value}),
        )?))
    }
    fn canonical(&self) -> Result<Value> {
        match self {
            Self::Feed(v) => Ok(serde_json::to_value(v)?),
            Self::Operation(v) => Ok(serde_json::to_value(v)?),
        }
    }
    fn same(&self, other: &Self) -> Result<bool> {
        Ok(self.canonical()? == other.canonical()?)
    }
}
/// Minted only by the current runtime's private native channel. This is a
/// planning gate: it authorizes NO network/device/file acquisition.
pub(crate) struct SourcePlanningAuthority {
    broker: Arc<Mutex<PermissionBroker>>,
    channel: Channel,
    request: HostRequest,
    quota: QuotaGroup,
    _admission: StorageAdmission,
}
impl SourcePlanningAuthority {
    pub(crate) fn from_runtime(
        broker: Arc<Mutex<PermissionBroker>>,
        channel: Channel,
        request: HostRequest,
        quota: QuotaGroup,
    ) -> Result<Self> {
        let admission = charge(&quota, 64 * 1024)?;
        let value = Self {
            broker,
            channel,
            request,
            quota,
            _admission: admission,
        };
        value.check()?;
        Ok(value)
    }
    fn check(&self) -> Result<()> {
        if self.request.is_cancelled() || !self.request.payload.shares_root(&self.quota) {
            return Err(denied("source planning original request retired"));
        }
        let broker = self
            .broker
            .lock()
            .map_err(|_| denied("source native owner poisoned"))?;
        let coordinates = broker
            .channel_coordinates(&self.channel)
            .map_err(|e| AnimationError::PermissionDenied(e.to_string()))?;
        if coordinates
            != (
                self.request.authority.instance_id,
                self.request.authority.plan_generation,
                self.request.authority.authorization_epoch,
            )
        {
            return Err(denied("source planning private channel mismatch"));
        }
        Ok(())
    }
}
/// Native supplied policy; never grow these limits on refusal. Media policy is
/// the caller's original native policy, independently validated by NativeMedia.
#[derive(Clone, Copy)]
pub struct SourceActorLimits {
    pub pages: usize,
    pub encoded_bytes: usize,
    pub media: MediaLimits,
}
impl SourceActorLimits {
    fn constrained(
        mut self,
        request: &HostRequest,
        plan: &crate::plan::AnimationPlan,
    ) -> Result<Self> {
        self.validate()?;
        if request.phase != crate::engine::ServicePhase::Create {
            return Ok(self);
        }
        let Some(preparation) = plan.preparation.as_ref() else {
            return Ok(self);
        };
        let fields = preparation
            .as_object()
            .ok_or_else(|| invalid("source preparation schema"))?;
        let Some(http) = fields.get("http") else {
            return Ok(self);
        };
        let http = http
            .as_object()
            .ok_or_else(|| invalid("source HTTP preparation schema"))?;
        for (key, target) in [
            ("max_requests", &mut self.pages),
            ("max_bytes", &mut self.encoded_bytes),
        ] {
            if let Some(value) = http.get(key) {
                let bound = value
                    .as_u64()
                    .and_then(|v| usize::try_from(v).ok())
                    .filter(|v| *v > 0)
                    .ok_or_else(|| invalid("source HTTP preparation bound"))?;
                *target = (*target).min(bound);
            }
        }
        Ok(self) // Native policy only narrows to actual declared create reservations, never grows.
    }
    fn validate(self) -> Result<()> {
        if self.pages == 0
            || self.pages > 256
            || self.encoded_bytes == 0
            || self.encoded_bytes > 256 * 1024 * 1024
        {
            return Err(invalid("source actor policy bounds"));
        }
        Ok(())
    }
}
/// One ORIGINAL host-owned process cadence instance must be shared by all
/// source actors. Construction admits its complete bounded map first; this
/// module does not secretly create a per-package cadence or an executor.
pub struct SourceCadence {
    quota: QuotaGroup,
    entries: Mutex<BTreeMap<String, u64>>,
    _admission: StorageAdmission,
}
impl SourceCadence {
    pub fn new(quota: QuotaGroup) -> Result<Self> {
        let admission = charge(&quota, 128 * 1024)?;
        Ok(Self {
            quota,
            entries: Mutex::new(BTreeMap::new()),
            _admission: admission,
        })
    }
    fn admit(&self, quota: &QuotaGroup, key: &str, now: u64, interval: u64) -> Result<bool> {
        if !self.quota.shares_root(quota) || key.is_empty() || key.len() > 64 || interval == 0 {
            return Err(denied("source cadence original owner/key mismatch"));
        }
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| denied("source cadence poisoned"))?;
        if entries.get(key).is_some_and(|due| now < *due) {
            return Ok(false);
        }
        if !entries.contains_key(key) && entries.len() >= 128 {
            return Err(invalid("source cadence key bound"));
        }
        entries.insert(key.into(), now.saturating_add(interval));
        Ok(true)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum TransportKind {
    Body,
    Lines { line_bytes: usize, records: usize },
}
struct HttpPlan {
    options: HttpOptions,
    kind: TransportKind,
    key: [u8; 32],
}
impl HttpPlan {
    fn from_native(options: &HttpOptions, kind: TransportKind) -> Result<Self> {
        options.validate()?;
        if options.method != "GET" || options.body.is_some() {
            return Err(invalid("native source GET-only transport contract"));
        }
        // Source replay never silently repeats a mutating provider request.
        let value = json!({"url":options.url,"method":options.method,"headers":options.headers,
            "body":options.body,"response":options.response,"max_bytes":options.max_bytes,
            "timeout_ms":options.timeout_ms,"credential":options.credential,
            "stream":match kind { TransportKind::Body => None, TransportKind::Lines {line_bytes,records} => Some((line_bytes,records)) }});
        Ok(Self {
            options: options.clone(),
            kind,
            key: Sha256::digest(serde_json::to_vec(&value)?).into(),
        })
    }
}
struct NativeLines {
    status: u16,
    lines: Vec<Vec<u8>>,
    _admission: StorageAdmission,
}
#[derive(Clone, Copy)]
struct NativeLineBounds {
    line_bytes: usize,
    records: usize,
}
fn read_native_lines(
    status: u16,
    reader: &mut (impl std::io::Read + ?Sized),
    max_bytes: usize,
    bounds: NativeLineBounds,
    quota: &QuotaGroup,
    stop: &AtomicBool,
) -> Result<NativeLines> {
    let NativeLineBounds {
        line_bytes,
        records,
    } = bounds;
    if !(100..=599).contains(&status)
        || max_bytes == 0
        || max_bytes > 32_000_000
        || line_bytes == 0
        || line_bytes > 262144
        || records == 0
        || records > 1024
    {
        return Err(invalid("source native line bounds"));
    }
    let capacity = max_bytes
        .checked_add(
            records
                .checked_mul(64)
                .ok_or_else(|| invalid("source stream records overflow"))?,
        )
        .and_then(|v| v.checked_add(line_bytes))
        .ok_or_else(|| invalid("source stream admission overflow"))?;
    let admission = charge(quota, capacity)?;
    let mut lines = Vec::new();
    lines
        .try_reserve_exact(records)
        .map_err(|_| invalid("source line allocation"))?;
    let mut working = Vec::new();
    working
        .try_reserve_exact(line_bytes)
        .map_err(|_| invalid("source framing allocation"))?;
    let mut total = 0usize;
    let mut block = [0u8; 8192];
    'capture: loop {
        sources::cancelled(stop)?;
        let count = reader
            .read(&mut block)
            .map_err(|e| AnimationError::Runtime(e.to_string()))?;
        sources::cancelled(stop)?;
        if count == 0 {
            if !working.is_empty() {
                lines.push(copy_line(&working)?);
            }
            break;
        }
        for byte in &block[..count] {
            total = total
                .checked_add(1)
                .ok_or_else(|| invalid("source raw line bytes overflow"))?;
            if total > max_bytes {
                return Err(invalid("source raw line byte bound"));
            }
            if *byte != b'\n' {
                if working.len() >= line_bytes {
                    return Err(invalid("source raw line length"));
                }
                working.push(*byte);
                continue;
            }
            lines.push(copy_line(&working)?);
            working.clear();
            if lines.len() >= records {
                break 'capture;
            }
        }
    }
    Ok(NativeLines {
        status,
        lines,
        _admission: admission,
    })
}
fn copy_line(line: &[u8]) -> Result<Vec<u8>> {
    let bytes = line.strip_suffix(b"\r").unwrap_or(line);
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(bytes.len())
        .map_err(|_| invalid("source line allocation"))?;
    owned.extend_from_slice(bytes);
    Ok(owned)
}
enum CachedTransport {
    Body(SourceHttpResponse),
    Lines(NativeLines),
    Failure,
}
struct Exchange {
    key: [u8; 32],
    transport: CachedTransport,
}
struct ReplayState {
    authority: SourcePlanningAuthority,
    call: SourceCall,
    cursor: usize,
    exchanges: Vec<Exchange>,
    pending: Option<HttpPlan>,
    encoded: usize,
    media: NativeMedia,
    images: BTreeMap<[u8; 32], NativeSourceImage>,
    terrain: BTreeMap<(String, u32), Arc<NativeSourceHeightfield>>,
    cadence: Arc<SourceCadence>,
    refresh: Option<(String, u64, u64, bool)>,
    refresh_checked: bool,
    limits: SourceActorLimits,
    _admission: StorageAdmission,
}
#[derive(Clone)]
struct ReplayClient(Arc<Mutex<ReplayState>>);
impl ReplayClient {
    fn state(&self) -> Result<std::sync::MutexGuard<'_, ReplayState>> {
        self.0
            .lock()
            .map_err(|_| denied("source replay owner poisoned"))
    }
    fn check(&self) -> Result<()> {
        self.state()?.authority.check()
    }
    fn exchange(&self, plan: HttpPlan) -> Result<usize> {
        let mut s = self.state()?;
        s.authority.check()?;
        if s.pending.is_some() {
            return Err(invalid("native source transport suspended"));
        }
        let index = s.cursor;
        s.cursor += 1;
        if let Some(existing) = s.exchanges.get(index) {
            if existing.key != plan.key {
                return Err(invalid("source replay branch changed"));
            }
            return Ok(index);
        }
        if index != s.exchanges.len() || index >= s.limits.pages {
            return Err(invalid("source transport page bound"));
        }
        s.pending = Some(plan);
        Err(invalid("native source transport suspended"))
    }
}
impl NativeSourceClient for ReplayClient {
    fn suspended(&self) -> Result<bool> {
        Ok(self.state()?.pending.is_some())
    }
    fn authorize_demand(&self, demand: &SourceDemand) -> Result<()> {
        self.check()?;
        let s = self.state()?;
        if !s.call.same(&SourceCall::Feed(demand.clone()))? {
            return Err(denied("source demand changed"));
        }
        Ok(())
    }
    fn authorize_operation(&self, request: &SourceRequest) -> Result<()> {
        self.check()?;
        let s = self.state()?;
        if !s.call.same(&SourceCall::Operation(request.clone()))? {
            return Err(denied("source operation changed"));
        }
        Ok(())
    }
    fn admit_refresh(&mut self, key: &str, now: u64, interval: u64) -> Result<bool> {
        let mut s = self.state()?;
        s.authority.check()?;
        if let Some((saved, time, period, decision)) = &s.refresh {
            if saved != key || *time != now || *period != interval {
                return Err(invalid("source refresh replay changed"));
            }
            return Ok(*decision);
        }
        // Pure planning may discover a URL, but cannot debit global refresh
        // before the exact native HTTP grant is selected. Root validates the
        // genuine unissued ticket before recording the real cadence decision.
        s.refresh = Some((key.into(), now, interval, true));
        Ok(true)
    }
    fn request_bytes(
        &mut self,
        options: &HttpOptions,
        quota: &QuotaGroup,
        _stop: &AtomicBool,
    ) -> Result<SourceHttpResponse> {
        let index = self.exchange(HttpPlan::from_native(options, TransportKind::Body)?)?;
        let s = self.state()?;
        if !quota.shares_root(&s.authority.quota) {
            return Err(denied("source body foreign quota"));
        }
        match &s.exchanges[index].transport {
            CachedTransport::Body(body) => body.copy_for_replay(quota, options.max_bytes),
            CachedTransport::Failure => Err(invalid("actual source transport failed")),
            _ => Err(invalid("source transport replay kind")),
        }
    }
    fn stream_lines(
        &mut self,
        options: &HttpOptions,
        stop: &AtomicBool,
        line_bytes: usize,
        records: usize,
        callback: &mut dyn FnMut(&[u8]) -> Result<bool>,
    ) -> Result<u16> {
        if line_bytes == 0 || line_bytes > 262144 || records == 0 || records > 1024 {
            return Err(invalid("source line budget"));
        }
        let index = self.exchange(HttpPlan::from_native(
            options,
            TransportKind::Lines {
                line_bytes,
                records,
            },
        )?)?;
        let s = self.state()?;
        match &s.exchanges[index].transport {
            CachedTransport::Lines(capture) => {
                for line in &capture.lines {
                    sources::cancelled(stop)?;
                    s.authority.check()?;
                    if !callback(line)? {
                        break;
                    }
                }
                Ok(capture.status)
            }
            CachedTransport::Failure => Err(invalid("actual source stream failed")),
            _ => Err(invalid("source line replay kind")),
        }
    }
    fn admit_process_baseline(&mut self, key: &'static str, bytes: usize) -> Result<()> {
        self.check()?;
        let s = self.state()?;
        crate::native_source_baseline::admit_process_baseline(&s.authority.quota, key, bytes)
    }
    fn decode_image(
        &mut self,
        bytes: &[u8],
        max_pixels: usize,
        quota: &QuotaGroup,
        stop: &AtomicBool,
    ) -> Result<NativeSourceImage> {
        sources::cancelled(stop)?;
        self.check()?;
        let mut s = self.state()?;
        if !quota.shares_root(&s.authority.quota)
            || max_pixels == 0
            || max_pixels > s.limits.media.pixels
        {
            return Err(denied("source media bound/root"));
        }
        let key: [u8; 32] = Sha256::digest(bytes).into();
        if let Some(image) = s.images.get(&key) {
            return Ok(image.clone());
        }
        let token = s.authority.request.stop_token();
        let handle = s.media.decode(bytes, &token)?;
        let image = NativeSourceImage::from_native(&s.media, handle)?;
        s.images.insert(key, image.clone());
        Ok(image)
    }
    fn terrain(
        &mut self,
        body: &str,
        seed: u32,
        quota: &QuotaGroup,
        stop: &AtomicBool,
    ) -> Result<Arc<NativeSourceHeightfield>> {
        sources::cancelled(stop)?;
        self.check()?;
        let mut s = self.state()?;
        if !quota.shares_root(&s.authority.quota) {
            return Err(denied("source terrain foreign quota"));
        }
        let key = (body.to_owned(), seed);
        if let Some(field) = s.terrain.get(&key) {
            return Ok(Arc::clone(field));
        }
        if !s.terrain.is_empty() {
            return Err(invalid("source terrain cache bound"));
        }
        let world = serde_json::from_value(json!(body))?;
        let field = NativeSourceHeightfield::load_native(world, seed, quota, stop)?;
        s.terrain.insert(key, Arc::clone(&field));
        Ok(field)
    }
}

pub enum NativeSourceOutput {
    Operation(Arc<AdmittedSourceValue>),
    Snapshot(Option<Arc<AdmittedSourceSnapshot>>),
}
struct SourceRun {
    dispatcher: SourceDispatcher<ReplayClient>,
    call: SourceCall,
    handle: Option<SourceHandle>,
    clock: SourceClock,
    stop: Arc<AtomicBool>,
}
impl SourceRun {
    fn cpu_step(&mut self) -> Result<Option<NativeSourceOutput>> {
        {
            let mut s = self.dispatcher.actor_client().state()?;
            s.authority.check()?;
            s.cursor = 0;
            if s.pending.is_some() {
                return Err(invalid("source pending transport not consumed"));
            }
        }
        let result = match &self.call {
            SourceCall::Feed(_) => self
                .dispatcher
                .poll(
                    self.handle
                        .ok_or_else(|| invalid("actual source handle missing"))?,
                    self.clock,
                    &self.stop,
                )
                .map(NativeSourceOutput::Snapshot),
            SourceCall::Operation(request) => self
                .dispatcher
                .dispatch(request.clone(), self.clock, &self.stop)
                .map(NativeSourceOutput::Operation),
        };
        if self.dispatcher.actor_client().suspended()? {
            return Ok(None);
        }
        result.map(Some)
    }
    fn admit_refresh_at_issue(&self) -> Result<bool> {
        let mut s = self.dispatcher.actor_client().state()?;
        if s.refresh_checked {
            return Ok(true);
        }
        let Some((key, now, interval, _)) = s.refresh.clone() else {
            s.refresh_checked = true;
            return Ok(true);
        };
        let decision = s.cadence.admit(&s.authority.quota, &key, now, interval)?;
        s.refresh = Some((key, now, interval, decision));
        s.refresh_checked = true;
        Ok(decision)
    }
    fn take_plan(&mut self) -> Result<HttpPlan> {
        self.dispatcher
            .actor_client()
            .state()?
            .pending
            .take()
            .ok_or_else(|| invalid("native HTTP continuation absent"))
    }
    fn install(&mut self, plan: HttpPlan, result: Result<CachedTransport>) -> Result<()> {
        let mut s = self.dispatcher.actor_client().state()?;
        let transport = match result {
            Ok(v) => v,
            Err(_) => CachedTransport::Failure,
        }; // Actual terminal failure, not a fabricated body/success; replay lets native provider error/backoff run normally.
        let bytes = match &transport {
            CachedTransport::Body(v) => v.as_bytes().len(),
            CachedTransport::Lines(v) => v
                .lines
                .iter()
                .try_fold(0usize, |sum, line| sum.checked_add(line.len()))
                .ok_or_else(|| invalid("source stream byte overflow"))?,
            CachedTransport::Failure => 0,
        };
        let total = s
            .encoded
            .checked_add(bytes)
            .ok_or_else(|| invalid("source encoded overflow"))?;
        if total > s.limits.encoded_bytes || s.exchanges.len() >= s.limits.pages {
            return Err(invalid("source replay retention bound"));
        }
        s.exchanges.push(Exchange {
            key: plan.key,
            transport,
        });
        s.encoded = total;
        Ok(())
    }
}
struct CpuStep {
    run: SourceRun,
    output: Result<Option<NativeSourceOutput>>,
}
struct SourceCpuJob {
    run: SourceRun,
    previous: Option<Retention>,
}
impl Job for SourceCpuJob {
    type Output = CpuStep;
    type Error = AnimationError;
    fn run(mut self, context: JobContext) -> Result<CpuStep> {
        if context.stop_requested() {
            self.run.stop.store(true, Ordering::Release);
        }
        sources::cancelled(&self.run.stop)?;
        let output = self
            .run
            .cpu_step()
            .map_err(|_| invalid("native source provider failed"));
        if context.stop_requested() {
            self.run.stop.store(true, Ordering::Release);
        }
        sources::cancelled(&self.run.stop)?;
        self.run.dispatcher.actor_client().check()?;
        // Previous CPU/IO reservation survives all parser/media/terrain work;
        // every retained output/run allocation also has its own original debit.
        drop(self.previous.take());
        Ok(CpuStep {
            run: self.run,
            output,
        })
    }
}
struct SourceIoJob {
    run: Retained<SourceRun>,
    plan: HttpPlan,
    factory: NativeHttpAuthorityFactory,
    client: Client,
    dns: Arc<dyn DnsResolver + Send + Sync>,
    quota: QuotaGroup,
}
struct SourceIoStep {
    run: SourceRun,
}
struct SharedDns(Arc<dyn DnsResolver + Send + Sync>);
impl DnsResolver for SharedDns {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<std::net::SocketAddr>> {
        self.0.resolve(host, port)
    }
}
impl Job for SourceIoJob {
    type Output = SourceIoStep;
    type Error = AnimationError;
    fn run(self, context: JobContext) -> Result<SourceIoStep> {
        let mut authority = self.factory.enter(&context, &self.client)?;
        let (mut run, previous) = self.run.into_parts();
        let stop = Arc::clone(&run.stop);
        let mut options = self.plan.options.clone();
        let remaining = run
            .dispatcher
            .actor_client()
            .state()?
            .authority
            .request
            .remaining_ms();
        if remaining == 0 {
            return Err(denied("source original deadline"));
        }
        options.timeout_ms = options.timeout_ms.min(remaining);
        {
            let state = run.dispatcher.actor_client().state()?;
            let capacity = state
                .limits
                .encoded_bytes
                .checked_sub(state.encoded)
                .ok_or_else(|| invalid("source remaining retention"))?;
            options.max_bytes = options.max_bytes.min(capacity);
            if options.max_bytes == 0 {
                return Err(AnimationError::Budget(
                    "source encoded retention exhausted".into(),
                ));
            }
        }
        let transport = match self.plan.kind {
            TransportKind::Body => http::request_with_reader(
                &options,
                &mut authority,
                &SharedDns(self.dns),
                &stop,
                |head, reader| {
                    Ok(CachedTransport::Body(SourceHttpResponse::read(
                        head.status,
                        reader,
                        &self.quota,
                        options.max_bytes,
                        &stop,
                    )?))
                },
            ),
            TransportKind::Lines {
                line_bytes,
                records,
            } => http::request_with_reader(
                &options,
                &mut authority,
                &SharedDns(self.dns),
                &stop,
                |head, reader| {
                    Ok(CachedTransport::Lines(read_native_lines(
                        head.status,
                        reader,
                        options.max_bytes,
                        NativeLineBounds {
                            line_bytes,
                            records,
                        },
                        &self.quota,
                        &stop,
                    )?))
                },
            ),
        };
        let transport = match transport {
            Ok(value) => authority.with_body_delivery(|| Ok(value)),
            Err(error) => Err(error),
        };
        run.install(self.plan, transport)?;
        // Install moves guarded raw owners without CPU parsing. Initial CPU
        // hold stays alive through actual IO callback return.
        drop(previous);
        Ok(SourceIoStep { run })
    }
}

const STAGE_BYTES: usize = 256 * 1024;
fn cpu_cost() -> JobCost {
    JobCost {
        input_bytes: STAGE_BYTES,
        result_bytes: STAGE_BYTES,
    }
}
fn io_cost(plan: &HttpPlan) -> Result<JobCost> {
    let line_scratch = match plan.kind {
        TransportKind::Body => 8192,
        TransportKind::Lines {
            line_bytes,
            records,
        } => line_bytes
            .checked_add(records * 64)
            .ok_or_else(|| invalid("source IO cost overflow"))?,
    };
    Ok(JobCost {
        input_bytes: 4 * 1024 * 1024 + STAGE_BYTES + line_scratch,
        result_bytes: plan
            .options
            .max_bytes
            .checked_add(STAGE_BYTES)
            .ok_or_else(|| invalid("source IO cost overflow"))?,
    })
}
enum SourceStage {
    Cpu {
        receipt: Receipt<SourceCpuJob>,
        operation: Option<ServiceOperation>,
    },
    Io {
        receipt: Receipt<SourceIoJob>,
        operation: ServiceOperation,
    },
    LostCpu {
        _receipt: Receipt<SourceCpuJob>,
        _operation: Option<ServiceOperation>,
    },
    LostIo {
        _receipt: Receipt<SourceIoJob>,
        _operation: ServiceOperation,
    },
    Refused(Box<SourceFailure>),
}
pub struct SourceActorEnvironment {
    pub client: Client,
    pub dns: Arc<dyn DnsResolver + Send + Sync>,
    pub cadence: Arc<SourceCadence>,
    pub limits: SourceActorLimits,
    pub credentials: Option<Arc<Mutex<Box<dyn crate::native_http_host::HostCredentialAdapter>>>>,
}
struct SourceCredentialAdapter(Arc<Mutex<Box<dyn crate::native_http_host::HostCredentialAdapter>>>);
impl crate::native_http_host::HostCredentialAdapter for SourceCredentialAdapter {
    fn bound_headers(
        &mut self,
        principal: &crate::permissions::PackageIdentity,
        handle: &str,
        origin: &str,
        quota: &QuotaGroup,
    ) -> Result<crate::native_http_host::BoundCredentialHeaders> {
        self.0
            .lock()
            .map_err(|_| denied("selected source credential owner poisoned"))?
            .bound_headers(principal, handle, origin, quota)
    }
}

/// A single true acquisition/refresh transaction. Retained native dispatcher
/// state/clock/revision survives all its HTTP continuation stages. Binding an
/// opened feed to a longer-lived script handle is a distinct native root duty.
pub struct NativeSourceHost {
    client: Client,
    quota: QuotaGroup,
    owner: Arc<Mutex<PermissionBroker>>,
    credentials: Option<Arc<Mutex<Box<dyn crate::native_http_host::HostCredentialAdapter>>>>,
    request: HostRequest,
    dns: Arc<dyn DnsResolver + Send + Sync>,
    stage: Option<SourceStage>,
    stop: Arc<AtomicBool>,
    _registry: StorageAdmission,
}
pub struct SourceCompletion {
    request: HostRequest,
    step: Retained<CpuStep>,
    operation: Option<ServiceOperation>,
}
impl SourceCompletion {
    pub fn request(&self) -> &HostRequest {
        &self.request
    }
    pub fn output(&self) -> Result<&NativeSourceOutput> {
        self.step
            .view()
            .output
            .as_ref()
            .map_err(|_| invalid("native source provider error"))?
            .as_ref()
            .ok_or_else(|| invalid("source terminal output absent"))
    }
    /// Native root first binds genuine image/feed handles and binary elevation;
    /// this owner does not turn slot numbers/native metadata into script handles.
    /// Keep self (and original receipts/images) alive through actual copy ACK.
    pub(crate) fn publish(
        &self,
        instance: &mut PackageInstance,
        value: ServiceValue,
    ) -> Result<crate::engine::CompletionState> {
        {
            let state = self.step.view().run.dispatcher.actor_client().state()?;
            state.authority.check()?;
            instance.check_http_owner(&state.authority.broker, &state.authority.quota)?;
        }
        match &self.operation {
            Some(operation) => instance.complete_authorized(operation, value),
            None => instance.complete_baseline_source(&self.request, value),
        }
    }
    /// Actual terminal receipt custody permits cleanup after failed publication;
    /// cancellation alone never creates this completion owner.
    pub(crate) fn settle(&self, instance: &mut PackageInstance) -> Result<()> {
        {
            let state = self.step.view().run.dispatcher.actor_client().state()?;
            instance.check_http_owner(&state.authority.broker, &state.authority.quota)?;
        }
        if let Some(operation) = &self.operation {
            instance.settle_http_terminal(operation)?;
        }
        Ok(())
    }
}
pub struct SourceFailure {
    owner: Arc<Mutex<PermissionBroker>>,
    quota: QuotaGroup,
    pub code: &'static str,
    request: HostRequest,
    operation: Option<ServiceOperation>,
    _retention: Retention,
    _run: Option<Retained<SourceRun>>,
}
impl SourceFailure {
    /// Only an admitted, native-produced structured error may be passed here.
    /// The original ticket and terminal retention survive a refused copy ACK.
    pub(crate) fn publish_error(
        &self,
        instance: &mut PackageInstance,
        value: ServiceValue,
    ) -> Result<crate::engine::CompletionState> {
        instance.check_http_owner(&self.owner, &self.quota)?;
        if self.request.is_cancelled() {
            return Err(denied("source error publication expired or cancelled"));
        }
        match &self.operation {
            Some(operation) => instance.complete_authorized(operation, value),
            None => instance.complete_baseline_source(&self.request, value),
        }
    }
    /// Actual callback terminal/not-started input is already held here. A Lost
    /// receipt NEVER produces this token. Root may publish a fixed error first.
    pub(crate) fn settle(&self, instance: &mut PackageInstance) -> Result<()> {
        instance.check_http_owner(&self.owner, &self.quota)?;
        if let Some(operation) = &self.operation {
            instance.settle_http_terminal(operation)?;
        }
        Ok(())
    }
    pub fn request(&self) -> &HostRequest {
        &self.request
    }
}
pub enum SourceEvent {
    Complete(SourceCompletion),
    Failed(SourceFailure),
    Lost,
}
impl NativeSourceHost {
    pub fn start(
        instance: &mut PackageInstance,
        request: HostRequest,
        clock: SourceClock,
        environment: SourceActorEnvironment,
    ) -> Result<Self> {
        let SourceActorEnvironment {
            client,
            dns,
            cadence,
            limits,
            credentials,
        } = environment;
        let limits = limits.constrained(&request, instance.plan())?;
        let quota = client.quota_group();
        let owner = instance.native_http_owner(&quota)?;
        let registry = charge(&quota, 64 * 1024)?;
        if request.payload.wire_bytes() > 64 * 1024 {
            return Err(invalid("source options wire bound"));
        }
        let metadata = charge(&quota, 4 * 1024 * 1024)?;
        let call = SourceCall::parse(&request)?;
        let authority = instance.source_planning_authority(&request, &quota)?;
        let media = NativeMedia::new(quota.clone(), limits.media)?;
        let stop = Arc::new(AtomicBool::new(false));
        let replay = ReplayClient(Arc::new(Mutex::new(ReplayState {
            authority,
            call: call.clone(),
            cursor: 0,
            exchanges: Vec::new(),
            pending: None,
            encoded: 0,
            media,
            images: BTreeMap::new(),
            terrain: BTreeMap::new(),
            cadence,
            refresh: None,
            refresh_checked: false,
            limits,
            _admission: metadata,
        })));
        let mut dispatcher = SourceDispatcher::new(replay, quota.clone())?;
        let handle = match &call {
            SourceCall::Feed(demand) => Some(dispatcher.open(demand.clone(), clock)?),
            _ => None,
        };
        let run = SourceRun {
            dispatcher,
            call,
            handle,
            clock,
            stop: Arc::clone(&stop),
        };
        let job = SourceCpuJob {
            run,
            previous: None,
        };
        let receipt = instance
            .with_baseline_source(&request, || {
                client
                    .try_submit(Lane::Cpu, cpu_cost(), job)
                    .map_err(Box::new)
            })?
            .map_err(|_| AnimationError::Budget("source planning admission refused".into()))?;
        Ok(Self {
            client,
            quota,
            owner,
            credentials,
            request,
            dns,
            stage: Some(SourceStage::Cpu {
                receipt,
                operation: None,
            }),
            stop,
            _registry: registry,
        })
    }
    fn refuse(&mut self, code: &'static str, operation: Option<ServiceOperation>, hold: Retention) {
        self.stage = Some(SourceStage::Refused(Box::new(SourceFailure {
            owner: Arc::clone(&self.owner),
            quota: self.quota.clone(),
            code,
            request: self.request.clone(),
            operation,
            _retention: hold,
            _run: None,
        })));
    }
    fn refused_event(&mut self) -> Option<SourceEvent> {
        match self.stage.take() {
            Some(SourceStage::Refused(failure)) => Some(SourceEvent::Failed(*failure)),
            other => {
                self.stage = other;
                None
            }
        }
    }
    fn cpu_resume(
        &mut self,
        instance: &mut PackageInstance,
        run: SourceRun,
        previous: Retention,
        operation: Option<ServiceOperation>,
    ) {
        let fallback = previous.clone();
        let job = SourceCpuJob {
            run,
            previous: Some(previous),
        };
        let issued = match &operation {
            Some(operation) => instance.with_source_cpu_authority(operation, || {
                self.client
                    .try_submit(Lane::Cpu, cpu_cost(), job)
                    .map_err(Box::new)
            }),
            None => instance.with_baseline_source(&self.request, || {
                self.client
                    .try_submit(Lane::Cpu, cpu_cost(), job)
                    .map_err(Box::new)
            }),
        };
        match issued {
            Ok(Ok(receipt)) => self.stage = Some(SourceStage::Cpu { receipt, operation }),
            Ok(Err(rejected)) => {
                drop(rejected);
                self.refuse("source_cpu_admission_refused", operation, fallback);
            }
            Err(_) => self.refuse("source_cpu_issue_withheld", operation, fallback),
        }
    }
    fn fetch(
        &mut self,
        instance: &mut PackageInstance,
        mut run: SourceRun,
        hold: Retention,
        operation: Option<ServiceOperation>,
    ) {
        let plan = match run.take_plan() {
            Ok(v) => v,
            Err(_) => {
                self.refuse("source_continuation_invalid", operation, hold);
                return;
            }
        };
        let need = match crate::permissions::OperationNeed::http_preflight(
            &plan.options.url,
            crate::permissions::HttpMethod::Get,
        ) {
            Ok(v) => v,
            Err(_) => {
                self.refuse("source_endpoint_invalid", operation, hold);
                return;
            }
        };
        let initial = operation.is_none();
        let operation = match operation {
            Some(v) => v,
            None => match instance.dispatch_source_http_service(self.request.clone(), need.clone())
            {
                Ok(v) => v,
                Err(_) => {
                    self.refuse("source_endpoint_denied", None, hold);
                    return;
                }
            },
        };
        if initial {
            match instance
                .with_source_unissued_authority(&operation, || run.admit_refresh_at_issue())
            {
                Ok(true) => {}
                Ok(false) => {
                    if instance.settle_http_terminal(&operation).is_err() {
                        self.refuse("source_cleanup_failed", Some(operation), hold);
                        return;
                    }
                    self.cpu_resume(instance, run, hold, None);
                    return;
                }
                Err(_) => {
                    self.refuse("source_refresh_withheld", Some(operation), hold);
                    return;
                }
            }
        }
        let selected = self.credentials.as_ref().map(|value| {
            Box::new(SourceCredentialAdapter(Arc::clone(value)))
                as Box<dyn crate::native_http_host::HostCredentialAdapter>
        });
        let factory = match instance.http_authority_factory(&operation, selected) {
            Ok(v) => v,
            Err(_) => {
                self.refuse("source_authority_withheld", Some(operation), hold);
                return;
            }
        };
        let cost = match io_cost(&plan) {
            Ok(v) => v,
            Err(_) => {
                self.refuse("source_io_cost_refused", Some(operation), hold);
                return;
            }
        };
        let fallback = hold.clone();
        let job = SourceIoJob {
            run: hold.retain(run),
            plan,
            factory,
            client: self.client.clone(),
            dns: Arc::clone(&self.dns),
            quota: self.quota.clone(),
        };
        let issued = if initial {
            instance.commit_service(&operation, || {
                self.client
                    .try_submit(Lane::Io, cost, job)
                    .map_err(Box::new)
            })
        } else {
            instance.with_source_http_authority(&operation, &[need], || {
                self.client
                    .try_submit(Lane::Io, cost, job)
                    .map_err(Box::new)
            })
        };
        match issued {
            Ok(Ok(receipt)) => self.stage = Some(SourceStage::Io { receipt, operation }),
            Ok(Err(rejected)) => {
                drop(rejected);
                self.refuse("source_io_admission_refused", Some(operation), fallback);
            }
            Err(_) => self.refuse("source_io_issue_withheld", Some(operation), fallback),
        }
    }
    /// ONLY from original client's completion wake. No repeated wait/poll loop.
    pub fn on_completion_wake(
        &mut self,
        instance: &mut PackageInstance,
    ) -> Result<Option<SourceEvent>> {
        instance.check_http_owner(&self.owner, &self.quota)?;
        let Some(stage) = self.stage.take() else {
            return Ok(None);
        };
        match stage {
            SourceStage::Cpu {
                mut receipt,
                operation,
            } => match receipt.try_take() {
                JobPoll::Pending => {
                    self.stage = Some(SourceStage::Cpu { receipt, operation });
                    Ok(None)
                }
                JobPoll::Lost | JobPoll::Taken => {
                    self.stage = Some(SourceStage::LostCpu {
                        _receipt: receipt,
                        _operation: operation,
                    });
                    Ok(Some(SourceEvent::Lost))
                }
                JobPoll::Ready(held) => {
                    let (outcome, hold) = held.into_parts();
                    match outcome {
                        JobOutcome::Finished(Ok(step))
                            if step.output.as_ref().is_ok_and(Option::is_some) =>
                        {
                            Ok(Some(SourceEvent::Complete(SourceCompletion {
                                request: self.request.clone(),
                                step: hold.retain(step),
                                operation,
                            })))
                        }
                        JobOutcome::Finished(Ok(step))
                            if step.output.as_ref().is_ok_and(Option::is_none) =>
                        {
                            self.fetch(instance, step.run, hold, operation);
                            Ok(self.refused_event())
                        }
                        JobOutcome::Finished(Ok(step)) => {
                            let run = hold.clone().retain(step.run);
                            Ok(Some(SourceEvent::Failed(SourceFailure {
                                owner: Arc::clone(&self.owner),
                                quota: self.quota.clone(),
                                code: "source_provider_failed",
                                request: self.request.clone(),
                                operation,
                                _retention: hold,
                                _run: Some(run),
                            })))
                        }
                        JobOutcome::Finished(Err(_)) => {
                            Ok(Some(SourceEvent::Failed(SourceFailure {
                                owner: Arc::clone(&self.owner),
                                quota: self.quota.clone(),
                                code: "source_provider_failed",
                                request: self.request.clone(),
                                operation,
                                _retention: hold,
                                _run: None,
                            })))
                        }
                        JobOutcome::NotStarted { job, .. } => {
                            drop(job);
                            Ok(Some(SourceEvent::Failed(SourceFailure {
                                owner: Arc::clone(&self.owner),
                                quota: self.quota.clone(),
                                code: "source_cpu_not_started",
                                request: self.request.clone(),
                                operation,
                                _retention: hold,
                                _run: None,
                            })))
                        }
                        JobOutcome::Panicked => Ok(Some(SourceEvent::Failed(SourceFailure {
                            owner: Arc::clone(&self.owner),
                            quota: self.quota.clone(),
                            code: "source_cpu_panicked",
                            request: self.request.clone(),
                            operation,
                            _retention: hold,
                            _run: None,
                        }))),
                    }
                }
            },
            SourceStage::Io {
                mut receipt,
                operation,
            } => match receipt.try_take() {
                JobPoll::Pending => {
                    self.stage = Some(SourceStage::Io { receipt, operation });
                    Ok(None)
                }
                JobPoll::Lost | JobPoll::Taken => {
                    self.stage = Some(SourceStage::LostIo {
                        _receipt: receipt,
                        _operation: operation,
                    });
                    Ok(Some(SourceEvent::Lost))
                }
                JobPoll::Ready(held) => {
                    let (outcome, hold) = held.into_parts();
                    match outcome {
                        JobOutcome::Finished(Ok(step)) => {
                            self.cpu_resume(instance, step.run, hold, Some(operation));
                            Ok(self.refused_event())
                        }
                        JobOutcome::Finished(Err(_)) => {
                            Ok(Some(SourceEvent::Failed(SourceFailure {
                                owner: Arc::clone(&self.owner),
                                quota: self.quota.clone(),
                                code: "source_transport_failed",
                                request: self.request.clone(),
                                operation: Some(operation),
                                _retention: hold,
                                _run: None,
                            })))
                        }
                        JobOutcome::NotStarted { job, .. } => {
                            drop(job);
                            Ok(Some(SourceEvent::Failed(SourceFailure {
                                owner: Arc::clone(&self.owner),
                                quota: self.quota.clone(),
                                code: "source_io_not_started",
                                request: self.request.clone(),
                                operation: Some(operation),
                                _retention: hold,
                                _run: None,
                            })))
                        }
                        JobOutcome::Panicked => Ok(Some(SourceEvent::Failed(SourceFailure {
                            owner: Arc::clone(&self.owner),
                            quota: self.quota.clone(),
                            code: "source_effects_unknown",
                            request: self.request.clone(),
                            operation: Some(operation),
                            _retention: hold,
                            _run: None,
                        }))),
                    }
                }
            },
            SourceStage::Refused(failure) => Ok(Some(SourceEvent::Failed(*failure))),
            lost => {
                self.stage = Some(lost);
                Ok(None)
            } // Do not re-poll Lost or claim actual retirement.
        }
    }
    pub fn cancel(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.request.stop_token().stop();
        match self.stage.as_ref() {
            Some(SourceStage::Cpu { receipt, .. })
            | Some(SourceStage::LostCpu {
                _receipt: receipt, ..
            }) => receipt.cancel(),
            Some(SourceStage::Io { receipt, .. })
            | Some(SourceStage::LostIo {
                _receipt: receipt, ..
            }) => receipt.cancel(),
            Some(SourceStage::Refused(_)) | None => {}
        }
    }
    pub fn is_drained(&self) -> bool {
        self.stage.is_none()
    }
}
impl Drop for NativeSourceHost {
    fn drop(&mut self) {
        // A returned terminal event owns the remaining publication lifetime.
        // Dropping the now-drained scheduler must not cancel that transferred
        // request before the native root can perform its current copy ACK.
        if self.stage.is_some() {
            self.cancel();
        }
    }
} // Signal active stages only; original guards survive until actual bank exit.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::{EngineLimits, ServiceAuthority, ServiceBudget, ServicePhase},
        permissions::{Ceiling, PackageIdentity, PermissionPlan},
    };
    use std::{
        io::Cursor,
        time::{Duration, Instant},
    };
    fn quota() -> QuotaGroup {
        QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 2,
            jobs: 8,
            service_jobs: 0,
            input_bytes: 32 * 1024 * 1024,
            result_bytes: 32 * 1024 * 1024,
            worker_threads: 4,
            worker_bytes: 768 * 1024 * 1024,
        })
    }
    fn native_request(
        method: &str,
        metadata: Value,
        quota: &QuotaGroup,
    ) -> (HostRequest, Arc<Mutex<PermissionBroker>>, Channel) {
        native_request_with_phase(method, metadata, quota, ServicePhase::Async)
    }
    fn native_request_with_phase(
        method: &str,
        metadata: Value,
        quota: &QuotaGroup,
        phase: ServicePhase,
    ) -> (HostRequest, Arc<Mutex<PermissionBroker>>, Channel) {
        let mut broker = PermissionBroker::new(
            PackageIdentity::unverified("source_fixture".into(), b"task-local pure source fixture")
                .unwrap(),
            Ceiling {
                permissions: vec![],
            },
            Ceiling {
                permissions: vec![],
            },
        )
        .unwrap();
        let review = broker
            .prepare(
                1,
                1,
                PermissionPlan {
                    permissions: vec![],
                    demands: vec![],
                },
                BTreeMap::new(),
            )
            .unwrap();
        let active = broker
            .resolve(review, BTreeMap::new())
            .unwrap()
            .activation
            .unwrap();
        let limits = EngineLimits::default();
        let payload = ServiceValue::copy_request_from_host(
            &metadata,
            &[],
            &BTreeMap::new(),
            &limits,
            quota.clone(),
            &ServiceBudget::new(&limits),
        )
        .unwrap();
        let request = HostRequest::from_transport(
            1,
            method.into(),
            30000,
            "f".repeat(64),
            ServiceAuthority {
                instance_id: active.plan.instance_id,
                plan_generation: active.plan.plan_revision,
                authorization_epoch: active.plan.authorization_epoch,
            },
            phase,
            payload,
        )
        .unwrap();
        (request, Arc::new(Mutex::new(broker)), active.channel)
    }
    #[test]
    fn declared_create_http_limits_only_narrow_native_actor_policy() {
        let quota = quota();
        let (request, _, _) = native_request(
            "sources.astronomy.observe",
            json!({"epoch_ms": 1_700_000_000_000_i64}),
            &quota,
        );
        let mut plan: crate::plan::AnimationPlan = serde_json::from_value(json!({
            "format": "gray32", "fps": 30.0, "inputs": {},
            "preparation": {"http": {"max_requests": 2, "max_bytes": 4096}}
        }))
        .unwrap();
        let original = SourceActorLimits {
            pages: 8,
            encoded_bytes: 65536,
            media: MediaLimits::default(),
        };
        let live = original.constrained(&request, &plan).unwrap();
        assert_eq!((live.pages, live.encoded_bytes), (8, 65536));
        let (request, _, _) = native_request_with_phase(
            "sources.astronomy.observe",
            json!({"epoch_ms": 1_700_000_000_000_i64}),
            &quota,
            ServicePhase::Create,
        );
        let create = original.constrained(&request, &plan).unwrap();
        assert_eq!((create.pages, create.encoded_bytes), (2, 4096));
        plan.preparation = Some(json!({"http": {"max_requests": 99, "max_bytes": 999999}}));
        let bounded = original.constrained(&request, &plan).unwrap();
        assert_eq!((bounded.pages, bounded.encoded_bytes), (8, 65536));
        plan.preparation = Some(json!({"http": {"max_requests": 0}}));
        assert!(original.constrained(&request, &plan).is_err());
        plan.preparation = Some(json!({"http": {"max_bytes": "4096"}}));
        assert!(original.constrained(&request, &plan).is_err());
    }
    fn run(method: &str, metadata: Value, quota: &QuotaGroup) -> SourceRun {
        let _preparse = charge(quota, 128 * 1024).unwrap();
        let (request, broker, channel) = native_request(method, metadata, quota);
        let authority =
            SourcePlanningAuthority::from_runtime(broker, channel, request.clone(), quota.clone())
                .unwrap();
        let call = SourceCall::parse(&request).unwrap();
        let media_limits = MediaLimits {
            handles: 16,
            pixels: 1024 * 1024,
            decoder_scratch_bytes: 32 * 1024 * 1024,
            ..MediaLimits::default()
        };
        let media = NativeMedia::new(quota.clone(), media_limits).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let state = ReplayState {
            authority,
            call: call.clone(),
            cursor: 0,
            exchanges: vec![],
            pending: None,
            encoded: 0,
            media,
            images: BTreeMap::new(),
            terrain: BTreeMap::new(),
            cadence: Arc::new(SourceCadence::new(quota.clone()).unwrap()),
            refresh: None,
            refresh_checked: false,
            limits: SourceActorLimits {
                pages: 8,
                encoded_bytes: 16 * 1024 * 1024,
                media: media_limits,
            },
            _admission: charge(quota, 4 * 1024 * 1024).unwrap(),
        };
        let mut dispatcher =
            SourceDispatcher::new(ReplayClient(Arc::new(Mutex::new(state))), quota.clone())
                .unwrap();
        let clock = SourceClock {
            monotonic_ms: 1000,
            epoch_ms: 1_700_000_000_000,
        };
        let handle = match &call {
            SourceCall::Feed(demand) => Some(dispatcher.open(demand.clone(), clock).unwrap()),
            _ => None,
        };
        SourceRun {
            dispatcher,
            call,
            handle,
            clock,
            stop,
        }
    }
    fn geo() -> Value {
        json!({"bounds":{"west":-180.0,"east":180.0,"south":-90.0,"north":90.0},"max_entities":2,"max_hz":1.0,"fields":[]})
    }
    #[test]
    fn closed_call_mapping_covers_six_demands_and_ten_native_operations() {
        let quota = quota();
        let _inputs = charge(&quota, 1024 * 1024).unwrap();
        let cases = vec![
            (
                "sources.series.open",
                json!({"provider":"crypto","max_samples":1,"interval_ms":60000}),
            ),
            ("sources.earthquakes.open", geo()),
            ("sources.aircraft.open", geo()),
            ("sources.boats.open", geo()),
            ("sources.chess.open", json!({"game_id":"tv","max_hz":1.0})),
            ("sources.weather.open", {
                let mut v = geo();
                v["layers"] = json!(["world_ir"]);
                v
            }),
            (
                "sources.geography.coastlines",
                json!({"body":"earth","bounds":geo()["bounds"],"max_points":64}),
            ),
            (
                "sources.geography.elevation",
                json!({"body":"earth","bounds":geo()["bounds"],"width":8,"height":8}),
            ),
            (
                "sources.geography.project",
                json!({"latitude":0.0,"longitude":0.0,"projection":"mercator"}),
            ),
            ("sources.chess.discover", json!({"max_games":1})),
            (
                "sources.wikipedia.search",
                json!({"query":"Rust","max_results":1}),
            ),
            (
                "sources.wikipedia.article",
                json!({"title":"Rust","max_bytes":4096,"max_images":0}),
            ),
            (
                "sources.osm.geocode",
                json!({"query":"Paris","max_results":1}),
            ),
            (
                "sources.osm.tile",
                json!({"x":0,"y":0,"zoom":0,"format":"raster"}),
            ),
            (
                "sources.astronomy.catalogue",
                json!({"name":"bright_stars","max_stars":1}),
            ),
            (
                "sources.astronomy.observe",
                json!({"epoch_ms":0,"latitude":0.0,"longitude":0.0}),
            ),
        ];
        assert_eq!(cases.len(), 16);
        for (method, metadata) in cases {
            let (request, _, _) = native_request(method, metadata, &quota);
            assert!(SourceCall::parse(&request).is_ok(), "{method}");
        }
        let (request, _, _) = native_request(
            "sources.wikipedia.search",
            json!({"query":"Rust","max_results":1,"demand_id":"guest"}),
            &quota,
        );
        assert!(SourceCall::parse(&request).is_err());
        assert!(!SourceCall::recognized("sources.wikipedia.eval"));
    }
    #[test]
    fn typed_suspension_preserves_actual_slot_due_revision_and_never_calls_network() {
        let quota = quota();
        let mut run = run("sources.earthquakes.open", geo(), &quota);
        let handle = run.handle.unwrap();
        let due = run.dispatcher.next_due_ms(handle).unwrap();
        assert!(run.cpu_step().unwrap().is_none());
        assert_eq!(run.dispatcher.next_due_ms(handle), Some(due));
        assert!(run.dispatcher.latest(handle).unwrap().is_none());
        let plan = run.take_plan().unwrap();
        assert!(plan.options.url.starts_with("https://earthquake.usgs.gov/"));
        // Pure parser fixture only, NOT an executed HTTP response/success proof.
        let raw = SourceHttpResponse::read(
            200,
            &mut Cursor::new(br#"{"type":"FeatureCollection","features":[]}"#),
            &quota,
            4096,
            &AtomicBool::new(false),
        )
        .unwrap();
        run.install(plan, Ok(CachedTransport::Body(raw))).unwrap();
        let output = run.cpu_step().unwrap().unwrap();
        let NativeSourceOutput::Snapshot(Some(snapshot)) = output else {
            panic!("real native snapshot absent")
        };
        let sources::SourceSnapshot::Geographic(value) = snapshot.view() else {
            panic!("native geographic shape")
        };
        assert_eq!(value.metadata.revision, 1);
        assert!(value.entities.is_empty());
        assert!(run.dispatcher.next_due_ms(handle).unwrap() > due);
    }
    #[test]
    fn actual_provider_error_uses_existing_backoff_while_suspension_does_not() {
        let quota = quota();
        let mut run = run("sources.earthquakes.open", geo(), &quota);
        let handle = run.handle.unwrap();
        let due = run.dispatcher.next_due_ms(handle).unwrap();
        assert!(run.cpu_step().unwrap().is_none());
        assert_eq!(run.dispatcher.next_due_ms(handle), Some(due));
        let plan = run.take_plan().unwrap();
        // Explicit injected failed-transport fixture; not a native successful effect.
        run.install(plan, Err(invalid("injected terminal refusal")))
            .unwrap();
        assert!(run.cpu_step().is_err());
        assert!(run.dispatcher.next_due_ms(handle).unwrap() > due);
        assert!(run.dispatcher.latest(handle).unwrap().is_none());
    }
    #[test]
    fn replay_refuses_branch_changes_and_mutating_provider_requests() {
        let quota = quota();
        let mut run = run(
            "sources.wikipedia.search",
            json!({"query":"Rust","max_results":1}),
            &quota,
        );
        assert!(run.cpu_step().unwrap().is_none());
        let plan = run.take_plan().unwrap();
        let first = plan.options.clone();
        let raw = SourceHttpResponse::read(
            200,
            &mut Cursor::new(b"{}"),
            &quota,
            64,
            &AtomicBool::new(false),
        )
        .unwrap();
        run.install(plan, Ok(CachedTransport::Body(raw))).unwrap();
        let replay = run.dispatcher.actor_client().clone();
        replay.state().unwrap().cursor = 0;
        let mut other = first.clone();
        other.url = "https://example.org/different".into();
        assert!(replay
            .exchange(HttpPlan::from_native(&other, TransportKind::Body).unwrap())
            .is_err());
        assert!(replay.state().unwrap().pending.is_none());
        other.method = "POST".into();
        assert!(HttpPlan::from_native(&other, TransportKind::Body).is_err());
    }
    #[test]
    fn cadence_is_tentative_in_planning_and_real_shared_debit_happens_once() {
        let quota = quota();
        let mut run = run("sources.earthquakes.open", geo(), &quota);
        assert!(run.cpu_step().unwrap().is_none());
        let replay = run.dispatcher.actor_client().clone();
        assert!(replay
            .state()
            .unwrap()
            .cadence
            .entries
            .lock()
            .unwrap()
            .is_empty());
        // This unit tests cadence mechanism only. Production invokes this
        // under runtime's genuine selected unissued-operation guard.
        assert!(run.admit_refresh_at_issue().unwrap());
        assert!(run.admit_refresh_at_issue().unwrap());
        let state = replay.state().unwrap();
        assert_eq!(state.cadence.entries.lock().unwrap().len(), 1);
        assert!(!state
            .cadence
            .admit(&quota, "usgs", run.clock.monotonic_ms, 60000)
            .unwrap());
        assert!(state
            .cadence
            .admit(
                &super::tests::quota(),
                "usgs",
                run.clock.monotonic_ms,
                60000
            )
            .is_err());
    }
    #[test]
    fn private_channel_epoch_and_original_cancellation_fence_all_replay() {
        let quota = quota();
        let run = run(
            "sources.wikipedia.search",
            json!({"query":"Rust","max_results":1}),
            &quota,
        );
        let replay = run.dispatcher.actor_client().clone();
        assert!(replay.check().is_ok());
        let broker = Arc::clone(&replay.state().unwrap().authority.broker);
        let _invalidation = broker.lock().unwrap().reset_all_decisions().unwrap();
        assert!(replay.check().is_err());
        let run = super::tests::run(
            "sources.wikipedia.search",
            json!({"query":"Rust","max_results":1}),
            &quota,
        );
        let replay = run.dispatcher.actor_client().clone();
        replay
            .state()
            .unwrap()
            .authority
            .request
            .stop_token()
            .stop();
        assert!(replay.check().is_err());
    }
    #[test]
    fn real_original_cpu_wake_returns_native_observation_without_io_bank() {
        use ilium_execution::{ClientLimits, Execution, ExecutionConfig, LaneConfig, ShutdownMode};
        let quota = quota();
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        let mut execution = Execution::start(
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
        let client = execution
            .client(ClientLimits {
                jobs: 4,
                service_jobs: 0,
                input_bytes: 8 * 1024 * 1024,
                result_bytes: 8 * 1024 * 1024,
            })
            .unwrap()
            .with_completion_wake(move || {
                let _ = tx.try_send(());
            });
        let run = run(
            "sources.astronomy.observe",
            json!({"epoch_ms":0,"latitude":0.0,"longitude":0.0}),
            &quota,
        );
        let mut receipt = client
            .try_submit(
                Lane::Cpu,
                cpu_cost(),
                SourceCpuJob {
                    run,
                    previous: None,
                },
            )
            .unwrap();
        rx.recv_timeout(Duration::from_secs(2)).unwrap(); // Genuine original completion wake, no agent polling loop.
        let JobPoll::Ready(held) = receipt.try_take() else {
            panic!("actual CPU wake without terminal outcome")
        };
        let JobOutcome::Finished(Ok(step)) = held.view() else {
            panic!("actual native CPU did not finish")
        };
        let Some(NativeSourceOutput::Operation(value)) = step.output.as_ref().unwrap() else {
            panic!("native observation missing")
        };
        assert_eq!(value.view()["epoch_ms"], 0);
        assert_eq!(
            value.view()["units"],
            "unit_direction_not_physical_position"
        );
        assert_eq!(value.view()["bodies"].as_array().unwrap().len(), 7);
        assert!(step
            .run
            .dispatcher
            .actor_client()
            .state()
            .unwrap()
            .exchanges
            .is_empty());
        assert!(quota.snapshot().jobs >= 1);
        let authority_state = step.run.dispatcher.actor_client().state().unwrap();
        let terminal_request = authority_state.authority.request.clone();
        let scheduler = NativeSourceHost {
            client: client.clone(),
            quota: quota.clone(),
            owner: Arc::clone(&authority_state.authority.broker),
            credentials: None,
            request: terminal_request.clone(),
            dns: Arc::new(http::SystemDns),
            stage: None,
            stop: Arc::clone(&step.run.stop),
            _registry: charge(&quota, 64 * 1024).unwrap(),
        };
        drop(authority_state);
        drop(scheduler);
        assert!(
            !terminal_request.is_cancelled(),
            "transferred actual terminal publication survives scheduler drop"
        );
        drop(held);
        drop(receipt);
        drop(client);
        execution.request_shutdown(ShutdownMode::Drain);
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(report.remaining_workers, 0);
        assert_eq!(
            report
                .health
                .lanes
                .iter()
                .map(|lane| lane.joined)
                .sum::<usize>(),
            1
        );
    }
    #[test]
    fn raw_framer_supports_source_line_budget_and_preserves_bytes_without_json_parsing() {
        let quota = quota();
        let stop = AtomicBool::new(false);
        let _fixture = charge(&quota, 200_000).unwrap();
        let mut data = vec![b'x'; 70_000];
        data.extend_from_slice(b"\r\n\nnot-json\nignored\n");
        let before = quota.snapshot().worker_bytes;
        let capture = read_native_lines(
            200,
            &mut Cursor::new(data),
            100_000,
            NativeLineBounds {
                line_bytes: 262144,
                records: 3,
            },
            &quota,
            &stop,
        )
        .unwrap();
        assert_eq!(capture.lines.len(), 3);
        assert_eq!(capture.lines[0].len(), 70_000);
        assert!(capture.lines[1].is_empty());
        assert_eq!(capture.lines[2], b"not-json");
        assert!(quota.snapshot().worker_bytes > before);
        drop(capture);
        assert_eq!(quota.snapshot().worker_bytes, before);
    }
    #[test]
    fn raw_framer_refuses_oversize_and_postread_cancel_before_retaining_lines() {
        let quota = quota();
        let stop = AtomicBool::new(false);
        assert!(read_native_lines(
            200,
            &mut Cursor::new(b"abcdef"),
            3,
            NativeLineBounds {
                line_bytes: 8,
                records: 1
            },
            &quota,
            &stop
        )
        .is_err());
        assert!(read_native_lines(
            200,
            &mut Cursor::new(b"abcd\n"),
            64,
            NativeLineBounds {
                line_bytes: 3,
                records: 1
            },
            &quota,
            &stop
        )
        .is_err());
        struct CancelOnRead<'a>(&'a AtomicBool);
        impl std::io::Read for CancelOnRead<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                buffer[0] = b'\n';
                self.0.store(true, Ordering::Release);
                Ok(1)
            }
        }
        let before = quota.snapshot().worker_bytes;
        assert!(read_native_lines(
            200,
            &mut CancelOnRead(&stop),
            64,
            NativeLineBounds {
                line_bytes: 8,
                records: 1
            },
            &quota,
            &stop
        )
        .is_err());
        assert_eq!(quota.snapshot().worker_bytes, before);
    }
}
