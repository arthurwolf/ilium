//! Original-bank source actor: CPU planning/parser -> authenticated raw IO -> CPU.
//! This module never waits for a bank callback, invents a script handle, or
//! considers a native transport suspension to be a provider failure.
use crate::{
    engine::{ArraySpec, HostRequest, ServiceValue, TypedArrayKind},
    error::{AnimationError, Result},
    http::{self, DnsResolver, HttpOptions},
    native_draw_host::NativeDrawHost,
    native_http_authority::NativeHttpAuthorityFactory,
    native_media::{MediaLimits, NativeMedia},
    permissions::{Channel, PermissionBroker, SourceCaptureFence},
    runtime::{PackageInstance, ServiceOperation, SourceFeedOperation},
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
use ilium_platform::owned_worker::StopToken;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// Original feed authority and its current refresh deadline; no new admission.
pub(crate) type FeedContext = (
    Arc<Mutex<PermissionBroker>>,
    Channel,
    QuotaGroup,
    StopToken,
    Instant,
);

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
    pub fn recognized(method: &str) -> bool {
        matches!(
            method,
            "sources.series.open"
                | "sources.series.close"
                | "sources.earthquakes.open"
                | "sources.earthquakes.close"
                | "sources.aircraft.open"
                | "sources.aircraft.close"
                | "sources.boats.open"
                | "sources.boats.close"
                | "sources.chess.open"
                | "sources.chess.close"
                | "sources.weather.open"
                | "sources.weather.close"
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
    feed: Option<SourceFeedLifetime>,
    _admission: StorageAdmission,
}
struct SourceFeedLifetime {
    stop: StopToken,
    cycle_deadline: Option<Instant>,
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
            feed: None,
            _admission: admission,
        };
        value.check()?;
        Ok(value)
    }
    pub(crate) fn check(&self) -> Result<()> {
        if let Some(feed) = &self.feed {
            if feed.stop.is_stopped()
                || feed
                    .cycle_deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
            {
                return Err(denied("source persistent feed cancelled or cycle expired"));
            }
        } else if self.request.is_cancelled() || !self.request.payload.shares_root(&self.quota) {
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
    fn promote_to_feed(&mut self, stop: StopToken) -> Result<()> {
        if self.feed.is_some() || stop.is_stopped() {
            return Err(denied(
                "source feed ownership already transferred or stopped",
            ));
        }
        // Called while the opener is still retained, before its helper copy.
        // A refused ACK stops and drops this prepared lifetime; a Delivered
        // ACK can transfer it without any later fallible admission.
        let coordinates = self
            .broker
            .lock()
            .map_err(|_| denied("source native owner poisoned"))?
            .channel_coordinates(&self.channel)
            .map_err(|error| AnimationError::PermissionDenied(error.to_string()))?;
        if coordinates
            != (
                self.request.authority.instance_id,
                self.request.authority.plan_generation,
                self.request.authority.authorization_epoch,
            )
        {
            return Err(denied("source feed channel changed before transfer"));
        }
        self.feed = Some(SourceFeedLifetime {
            stop,
            cycle_deadline: None,
        });
        Ok(())
    }
    fn begin_feed_cycle(&mut self) -> Result<Instant> {
        self.check()?;
        let feed = self
            .feed
            .as_mut()
            .ok_or_else(|| denied("source feed lifetime not transferred"))?;
        if feed.cycle_deadline.is_some() {
            return Err(denied("source feed cycle already active"));
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        feed.cycle_deadline = Some(deadline);
        Ok(deadline)
    }
    fn finish_feed_cycle(&mut self) -> Result<()> {
        let feed = self
            .feed
            .as_mut()
            .ok_or_else(|| denied("source feed lifetime not transferred"))?;
        feed.cycle_deadline = None;
        Ok(())
    }
    fn remaining_ms(&self) -> u64 {
        match &self.feed {
            Some(feed) => feed.cycle_deadline.map_or(0, |deadline| {
                u64::try_from(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis(),
                )
                .unwrap_or(u64::MAX)
            }),
            None => self.request.remaining_ms(),
        }
    }
    fn stop_token(&self) -> StopToken {
        self.feed
            .as_ref()
            .map_or_else(|| self.request.stop_token(), |feed| feed.stop.clone())
    }
    pub(crate) fn feed_context(&self) -> Result<FeedContext> {
        self.check()?;
        let feed = self
            .feed
            .as_ref()
            .ok_or_else(|| denied("source feed not transferred"))?;
        let deadline = feed
            .cycle_deadline
            .ok_or_else(|| denied("source feed refresh not begun"))?;
        Ok((
            Arc::clone(&self.broker),
            self.channel.clone(),
            self.quota.clone(),
            feed.stop.clone(),
            deadline,
        ))
    }
    fn capture_context(
        &self,
    ) -> Result<(Arc<Mutex<PermissionBroker>>, SourceCaptureFence, QuotaGroup)> {
        self.check()?;
        let fence = self
            .broker
            .lock()
            .map_err(|_| denied("source native owner poisoned"))?
            .source_capture_fence(&self.channel)
            .map_err(|error| AnimationError::PermissionDenied(error.to_string()))?;
        Ok((Arc::clone(&self.broker), fence, self.quota.clone()))
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
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.quota.shares_root(quota)
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
/// Offline qualification replaces only the socket/body transport. The real
/// package review, committed ticket, original IO job and native URL/hop
/// authority still run. This type exists only in unit tests or builds that
/// explicitly enable `qualification-test-support`.
#[cfg(any(test, feature = "qualification-test-support"))]
#[doc(hidden)]
pub struct OfflineSourceHttp {
    exact_url: String,
    public_address: std::net::SocketAddr,
    bodies: Mutex<std::collections::VecDeque<Vec<u8>>>,
}
#[cfg(any(test, feature = "qualification-test-support"))]
impl OfflineSourceHttp {
    #[doc(hidden)]
    pub fn new(
        exact_url: String,
        public_address: std::net::SocketAddr,
        bodies: Vec<Vec<u8>>,
    ) -> Result<Arc<Self>> {
        let url = url::Url::parse(&exact_url)
            .map_err(|error| AnimationError::Runtime(error.to_string()))?;
        if url.scheme() != "https"
            || public_address.port() != 443
            || crate::network::classify_address(public_address.ip())
                != crate::network::NetworkAddressClass::Public
            || bodies.is_empty()
        {
            return Err(denied("offline source fixture authority is invalid"));
        }
        Ok(Arc::new(Self {
            exact_url,
            public_address,
            bodies: Mutex::new(bodies.into()),
        }))
    }

    fn receive(
        &self,
        options: &HttpOptions,
        kind: TransportKind,
        authority: &mut crate::native_http_authority::NativeHttpAuthority,
        quota: &QuotaGroup,
        stop: &AtomicBool,
    ) -> Result<CachedTransport> {
        use crate::{
            http::{HttpAuthority, HttpPhase},
            permissions::HttpMethod,
        };
        options.validate()?;
        if !matches!(kind, TransportKind::Body)
            || options.method != "GET"
            || options.url != self.exact_url
            || options.body.is_some()
            || options.credential.is_some()
            || !options.headers.is_empty()
            || stop.load(Ordering::Acquire)
        {
            return Err(denied("offline source transport target or kind mismatch"));
        }
        let url = url::Url::parse(&options.url)
            .map_err(|error| AnimationError::Runtime(error.to_string()))?;
        if url.scheme() != "https" || self.public_address.port() != 443 {
            return Err(denied("offline source transport requires exact HTTPS"));
        }
        authority.authorize(HttpPhase::Preflight, HttpMethod::Get, &url, &[])?;
        let address = [self.public_address];
        authority.authorize(HttpPhase::Dispatch, HttpMethod::Get, &url, &address)?;
        authority.authorize(HttpPhase::Delivery, HttpMethod::Get, &url, &address)?;
        let body = self
            .bodies
            .lock()
            .map_err(|_| denied("offline source body owner poisoned"))?
            .pop_front()
            .ok_or_else(|| invalid("offline source body sequence exhausted"))?;
        let response = SourceHttpResponse::read(
            200,
            &mut std::io::Cursor::new(body),
            quota,
            options.max_bytes,
            stop,
        )?;
        authority.with_body_delivery(|| Ok(CachedTransport::Body(response)))
    }
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
    #[cfg(any(test, feature = "qualification-test-support"))]
    offline_http: Option<Arc<OfflineSourceHttp>>,
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
        let token = s.authority.stop_token();
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
    fn prepare_refresh(&mut self, clock: SourceClock) -> Result<()> {
        let mut state = self.dispatcher.actor_client().state()?;
        state.authority.check()?;
        if state.pending.is_some() {
            return Err(invalid("source previous transport still pending"));
        }
        // The dispatcher snapshot and prepared drawing handles retain their
        // own admitted pixel Arcs. Release this cycle's decoder handles before
        // admitting the next native image batch.
        let handles: Vec<_> = state
            .images
            .values()
            .map(NativeSourceImage::native_handle)
            .collect();
        for handle in handles {
            state.media.close(handle)?;
        }
        state.images.clear();
        state.authority.begin_feed_cycle()?;
        state.cursor = 0;
        state.exchanges.clear();
        state.encoded = 0;
        state.refresh = None;
        state.refresh_checked = false;
        self.clock = clock;
        Ok(())
    }
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
                .dispatch_native(request.clone(), self.clock, &self.stop)
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
        #[cfg(any(test, feature = "qualification-test-support"))]
        let offline_http = { run.dispatcher.actor_client().state()?.offline_http.clone() };
        #[cfg(any(test, feature = "qualification-test-support"))]
        if let Some(fixture) = offline_http {
            let transport =
                fixture.receive(&options, self.plan.kind, &mut authority, &self.quota, &stop);
            run.install(self.plan, transport)?;
            drop(previous);
            return Ok(SourceIoStep { run });
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

fn try_submit_preserving_rejection<J: Job>(
    client: &Client,
    lane: Lane,
    cost: JobCost,
    pending: &Cell<Option<J>>,
) -> Option<Receipt<J>> {
    let job = pending.take().expect("one source job issue");
    match client.try_submit(lane, cost, job) {
        Ok(receipt) => Some(receipt),
        Err(rejected) => {
            pending.set(Some(rejected.value));
            None
        }
    }
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
    #[cfg(any(test, feature = "qualification-test-support"))]
    #[doc(hidden)]
    pub offline_http: Option<Arc<OfflineSourceHttp>>,
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
/// A native feed descriptor and its distinct retained JSON charge. Image IDs
/// are actual prepared allocations; the caller rolls them back on refused ACK.
pub struct ProjectedFeedDescriptor {
    metadata: Value,
    imported: Vec<String>,
    metadata_bound: usize,
    _admission: StorageAdmission,
}
pub struct PreparedFeedRegistration {
    owner: Arc<Mutex<PermissionBroker>>,
    quota: QuotaGroup,
    replay_lineage: Vec<crate::replay::GrantLineage>,
    worker_stop: Arc<AtomicBool>,
    stop: StopToken,
    registry: StorageAdmission,
}
impl PreparedFeedRegistration {
    pub fn stop(&self) {
        self.stop.stop();
        self.worker_stop.store(true, Ordering::Release);
    }
}
impl ProjectedFeedDescriptor {
    pub fn metadata(&self) -> &Value {
        &self.metadata
    }
    pub fn imported(&self) -> &[String] {
        &self.imported
    }
    pub fn metadata_bound(&self) -> usize {
        self.metadata_bound
    }
    pub fn mark_error(&mut self) -> Result<()> {
        let revision = self.metadata["revision"]
            .as_u64()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| invalid("source feed status revision exhausted"))?;
        self.metadata["revision"] = json!(revision);
        self.metadata["status"] = json!({"state":"error"});
        if sources::types::json_owned_bytes(&self.metadata)? > self.metadata_bound {
            return Err(invalid("source feed error status admission"));
        }
        Ok(())
    }
    /// A delivered image-close ACK retires its native draw registration. Do
    /// not reseed the old image ID from a still-cached weather snapshot.
    pub fn image_closed(&mut self, id: &str) -> Result<()> {
        if !self.imported.iter().any(|image| image == id) || self.metadata["latest"].is_null() {
            return Ok(());
        }
        let revision = self.metadata["revision"]
            .as_u64()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| invalid("source feed image-close revision exhausted"))?;
        self.metadata["revision"] = json!(revision);
        self.metadata["latest"] = Value::Null;
        if sources::types::json_owned_bytes(&self.metadata)? > self.metadata_bound {
            return Err(invalid("source feed image-close admission"));
        }
        Ok(())
    }
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
    /// Check the same original channel and quota immediately before a native
    /// client binds images or copies typed planes from this terminal receipt.
    pub fn authorized_output(&self, instance: &PackageInstance) -> Result<&NativeSourceOutput> {
        {
            let state = self.step.view().run.dispatcher.actor_client().state()?;
            state.authority.check()?;
            instance.check_http_owner(&state.authority.broker, &state.authority.quota)?;
        }
        self.output()
    }
    /// A complete native result copy is separately charged before helper ACK.
    /// This does not publish, settle, or reinterpret source slot integers.
    pub fn copy_result(
        &self,
        instance: &PackageInstance,
        metadata: &Value,
        arrays: &[crate::engine::ArraySpec],
        planes: &BTreeMap<String, Vec<u8>>,
    ) -> Result<ServiceValue> {
        self.authorized_output(instance)?;
        let state = self.step.view().run.dispatcher.actor_client().state()?;
        ServiceValue::copy_from_host(
            metadata,
            arrays,
            planes,
            instance.engine_limits(),
            state.authority.quota.clone(),
        )
    }
    /// Construct a real helper completion from the admitted provider result.
    /// The only image descriptors accepted here are imported from the exact
    /// output's native allocations. Imported IDs remain caller-owned until
    /// Delivered ACK; the caller releases them on any other terminal outcome.
    pub fn copy_operation_result(
        &self,
        instance: &mut PackageInstance,
        drawing: &mut NativeDrawHost,
    ) -> Result<(ServiceValue, Vec<String>)> {
        let NativeSourceOutput::Operation(output) = self.authorized_output(instance)? else {
            return Err(invalid("feed opening requires retained native feed owner"));
        };
        let images = output.native_images();
        if images.len() > 64 {
            return Err(invalid("source image facade inventory"));
        }
        let quota = {
            let state = self.step.view().run.dispatcher.actor_client().state()?;
            state.authority.quota.clone()
        };
        if let Some(samples) = output.binary_f32() {
            if self.request.method != "sources.geography.elevation"
                || !images.is_empty()
                || samples.is_empty()
                || samples.iter().any(|sample| !sample.is_finite())
            {
                return Err(invalid("native source f32 output identity"));
            }
            let bytes = samples
                .len()
                .checked_mul(4)
                .ok_or_else(|| invalid("native source f32 byte overflow"))?;
            let _scratch = charge(&quota, bytes)?;
            let mut plane = Vec::new();
            plane
                .try_reserve_exact(bytes)
                .map_err(|_| invalid("source f32 copy allocation"))?;
            for sample in samples {
                plane.extend_from_slice(&sample.to_ne_bytes());
            }
            let mut planes = BTreeMap::new();
            planes.insert("b0".into(), plane);
            let value = self.copy_result(
                instance,
                &json!({"ok":true,"value":{"$ilium_binary":"b0"}}),
                &[ArraySpec {
                    name: "b0".into(),
                    kind: TypedArrayKind::F32,
                    elements: samples.len(),
                }],
                &planes,
            )?;
            return Ok((value, Vec::new()));
        }
        if self.request.method == "sources.geography.elevation" {
            return Err(invalid("native elevation missing typed product"));
        }
        let scratch_bytes = sources::types::json_owned_bytes(output.view())?
            .checked_add(64 * 1024)
            .ok_or_else(|| invalid("source descriptor copy size"))?;
        let _scratch = charge(&quota, scratch_bytes)?;
        let mut projected = output.view().clone();
        let mut imported = Vec::new();
        imported
            .try_reserve_exact(images.len())
            .map_err(|_| invalid("source import list allocation"))?;
        let mut used = vec![false; images.len()];
        let result = (|| -> Result<ServiceValue> {
            match self.request.method.as_str() {
                "sources.osm.tile" if !images.is_empty() => {
                    import_image_slot(
                        &mut projected["image"],
                        images,
                        &mut used,
                        drawing,
                        instance,
                        &mut imported,
                    )?;
                }
                "sources.wikipedia.article" if !images.is_empty() => {
                    let entries = projected["images"]
                        .as_array_mut()
                        .ok_or_else(|| invalid("article image result schema"))?;
                    for entry in entries {
                        import_image_slot(
                            &mut entry["image"],
                            images,
                            &mut used,
                            drawing,
                            instance,
                            &mut imported,
                        )?;
                    }
                }
                _ if !images.is_empty() => return Err(invalid("unmapped source image result")),
                _ => {}
            }
            if used.iter().any(|seen| !seen) {
                return Err(invalid("source image allocation not projected"));
            }
            self.copy_result(
                instance,
                &json!({"ok":true,"value":projected}),
                &[],
                &BTreeMap::new(),
            )
        })();
        match result {
            Ok(value) => Ok((value, imported)),
            Err(error) => {
                for key in imported.iter().rev() {
                    drawing.release_source_image(key)?;
                }
                Err(error)
            }
        }
    }
    pub fn is_feed(&self) -> bool {
        matches!(self.step.view().run.call, SourceCall::Feed(_))
    }
    pub fn prepare_feed_transfer(
        &self,
        instance: &PackageInstance,
        client: &Client,
        stop: StopToken,
    ) -> Result<PreparedFeedRegistration> {
        if !self.is_feed() {
            return Err(invalid("operation cannot transfer feed"));
        }
        self.authorized_output(instance)?;
        let mut state = self.step.view().run.dispatcher.actor_client().state()?;
        let quota = state.authority.quota.clone();
        if !client.quota_group().shares_root(&quota) {
            return Err(denied("feed client original quota mismatch"));
        }
        let registry = charge(&quota, 64 * 1024)?;
        let replay_lineage = self
            .operation
            .as_ref()
            .ok_or_else(|| denied("feed replay requires its committed open operation"))
            .and_then(|operation| instance.source_replay_lineage(operation))?;
        state.authority.promote_to_feed(stop.clone())?;
        Ok(PreparedFeedRegistration {
            owner: Arc::clone(&state.authority.broker),
            quota,
            replay_lineage,
            worker_stop: Arc::clone(&self.step.view().run.stop),
            stop,
            registry,
        })
    }
    pub fn copy_feed_open_result(
        &self,
        instance: &mut PackageInstance,
        drawing: &mut NativeDrawHost,
        native_id: &str,
    ) -> Result<(ServiceValue, ProjectedFeedDescriptor)> {
        let NativeSourceOutput::Snapshot(snapshot) = self.authorized_output(instance)? else {
            return Err(invalid("operation cannot open native feed"));
        };
        let descriptor = project_feed_descriptor(
            instance,
            drawing,
            native_id,
            &self.request.method,
            snapshot.as_deref(),
            &self
                .step
                .view()
                .run
                .dispatcher
                .actor_client()
                .state()?
                .authority
                .quota,
            FeedProjectionState {
                service_revision: None,
                provider_failed: false,
            },
        )?;
        match self.copy_result(
            instance,
            &json!({"ok":true,"value":descriptor.metadata}),
            &[],
            &BTreeMap::new(),
        ) {
            Ok(value) => Ok((value, descriptor)),
            Err(error) => {
                for image in descriptor.imported.iter().rev() {
                    drawing.release_source_image(image)?;
                }
                Err(error)
            }
        }
    }
    /// The original receipt charge remains attached to the entire dispatcher
    /// when a successfully delivered opener becomes a long-lived feed actor.
    pub fn into_feed_run(self) -> NativeSourceFeedRun {
        let (step, hold) = self.step.into_parts();
        NativeSourceFeedRun {
            run: hold.retain(step.run),
        }
    }
    /// Native root first binds genuine image/feed handles and binary elevation;
    /// this owner does not turn slot numbers/native metadata into script handles.
    /// Keep self (and original receipts/images) alive through actual copy ACK.
    pub fn publish(
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
    pub fn settle(&self, instance: &mut PackageInstance) -> Result<()> {
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
pub struct NativeSourceFeedRun {
    run: Retained<SourceRun>,
}
impl NativeSourceFeedRun {
    pub fn next_due_ms(&self) -> Result<u64> {
        self.run
            .view()
            .dispatcher
            .next_due_ms(
                self.run
                    .view()
                    .handle
                    .ok_or_else(|| invalid("feed handle absent"))?,
            )
            .ok_or_else(|| invalid("feed due state absent"))
    }
    pub fn close(self) -> Result<()> {
        let (mut run, hold) = self.run.into_parts();
        run.dispatcher
            .close(run.handle.ok_or_else(|| invalid("feed handle absent"))?)?;
        drop(run);
        drop(hold);
        Ok(())
    }
}
enum FeedStage {
    Cpu {
        receipt: Receipt<SourceCpuJob>,
        operation: Option<SourceFeedOperation>,
    },
    Io {
        receipt: Receipt<SourceIoJob>,
        operation: SourceFeedOperation,
    },
    LostCpu {
        _receipt: Receipt<SourceCpuJob>,
        _operation: Option<SourceFeedOperation>,
    },
    LostIo {
        _receipt: Receipt<SourceIoJob>,
        _operation: SourceFeedOperation,
    },
    Closed,
}
enum FeedOwnedState {
    Ready(Retained<SourceRun>),
    Completion(SourceFeedCompletion),
}
pub struct NativeSourceFeedHost {
    client: Client,
    quota: QuotaGroup,
    owner: Arc<Mutex<PermissionBroker>>,
    replay_lineage: Vec<crate::replay::GrantLineage>,
    dns: Arc<dyn DnsResolver + Send + Sync>,
    credentials: Option<Arc<Mutex<Box<dyn crate::native_http_host::HostCredentialAdapter>>>>,
    stop: StopToken,
    worker_stop: Arc<AtomicBool>,
    owned: Option<FeedOwnedState>,
    stage: Option<FeedStage>,
    blocked_until_ms: u64,
    _registry: StorageAdmission,
}
pub struct SourceFeedCompletion {
    step: Retained<CpuStep>,
    operation: Option<SourceFeedOperation>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFeedEvent {
    Complete,
    Failed(&'static str),
    Lost,
}
impl SourceFeedCompletion {
    pub fn project(
        &self,
        instance: &mut PackageInstance,
        drawing: &mut NativeDrawHost,
        native_id: &str,
        service_revision: u64,
    ) -> Result<ProjectedFeedDescriptor> {
        let run = &self.step.view().run;
        let state = run.dispatcher.actor_client().state()?;
        state.authority.check()?;
        instance.check_http_owner(&state.authority.broker, &state.authority.quota)?;
        let quota = state.authority.quota.clone();
        drop(state);
        let snapshot = match &self.step.view().output {
            Ok(Some(NativeSourceOutput::Snapshot(value))) => value.clone(),
            Ok(Some(NativeSourceOutput::Operation(_))) => {
                return Err(invalid("feed produced operation"));
            }
            _ => run
                .dispatcher
                .latest(run.handle.ok_or_else(|| invalid("feed handle missing"))?)?,
        };
        let method = match &run.call {
            SourceCall::Feed(SourceDemand::Series(_)) => "sources.series.open",
            SourceCall::Feed(SourceDemand::Earthquakes(_)) => "sources.earthquakes.open",
            SourceCall::Feed(SourceDemand::Aircraft(_)) => "sources.aircraft.open",
            SourceCall::Feed(SourceDemand::Boats(_)) => "sources.boats.open",
            SourceCall::Feed(SourceDemand::Chess(_)) => "sources.chess.open",
            SourceCall::Feed(SourceDemand::Weather(_)) => "sources.weather.open",
            SourceCall::Operation(_) => return Err(invalid("operation cannot refresh feed")),
        };
        let failed = self.step.view().output.is_err();
        project_feed_descriptor(
            instance,
            drawing,
            native_id,
            method,
            snapshot.as_deref(),
            &quota,
            FeedProjectionState {
                service_revision: Some(service_revision),
                provider_failed: failed,
            },
        )
    }
    pub fn deliver<T>(
        &self,
        instance: &mut PackageInstance,
        publish: impl FnOnce() -> T,
    ) -> Result<T> {
        if let Some(operation) = &self.operation {
            instance.deliver_source_feed(operation, publish)
        } else {
            let context = self
                .step
                .view()
                .run
                .dispatcher
                .actor_client()
                .state()?
                .authority
                .feed_context()?;
            instance.with_source_feed_planning(context, publish)
        }
    }
    pub fn settle(&self, instance: &mut PackageInstance) -> Result<()> {
        if let Some(operation) = &self.operation {
            instance.settle_source_feed_terminal(operation)?;
        }
        Ok(())
    }
    pub fn into_run(self) -> NativeSourceFeedRun {
        let (step, hold) = self.step.into_parts();
        NativeSourceFeedRun {
            run: hold.retain(step.run),
        }
    }
}
impl NativeSourceFeedHost {
    pub fn from_delivered(
        run: NativeSourceFeedRun,
        client: Client,
        dns: Arc<dyn DnsResolver + Send + Sync>,
        credentials: Option<Arc<Mutex<Box<dyn crate::native_http_host::HostCredentialAdapter>>>>,
        prepared: PreparedFeedRegistration,
    ) -> Self {
        Self {
            client,
            quota: prepared.quota,
            owner: prepared.owner,
            replay_lineage: prepared.replay_lineage,
            dns,
            credentials,
            stop: prepared.stop,
            worker_stop: prepared.worker_stop,
            owned: Some(FeedOwnedState::Ready(run.run)),
            stage: None,
            blocked_until_ms: 0,
            _registry: prepared.registry,
        }
    }
    /// Retain the exact latest admitted snapshot from a completed feed cycle.
    /// Capture refuses while refresh state is unsettled; it never takes a
    /// receipt, starts network work, or fabricates a source from metadata.
    #[cfg(feature = "v8-runtime")]
    pub fn capture_latest(
        &self,
        instance: &PackageInstance,
        max_resident_bytes: usize,
    ) -> Result<Arc<crate::native_source_capture::NativeCapturedFeed>> {
        if self.stage.is_some() {
            return Err(invalid(
                "source feed capture refused during pending refresh",
            ));
        }
        let Some(FeedOwnedState::Ready(retained)) = self.owned.as_ref() else {
            return Err(invalid(
                "source feed capture requires an acknowledged quiescent snapshot",
            ));
        };
        let run = retained.view();
        let handle = run
            .handle
            .ok_or_else(|| invalid("source feed capture handle absent"))?;
        let snapshot = run
            .dispatcher
            .latest(handle)?
            .ok_or_else(|| invalid("source feed has no completed snapshot to capture"))?;
        let family = match &run.call {
            SourceCall::Feed(SourceDemand::Series(_)) => crate::replay::InputFamily::Series,
            SourceCall::Feed(SourceDemand::Earthquakes(_)) => {
                crate::replay::InputFamily::Earthquakes
            }
            SourceCall::Feed(SourceDemand::Aircraft(_)) => crate::replay::InputFamily::Aircraft,
            SourceCall::Feed(SourceDemand::Boats(_)) => crate::replay::InputFamily::Boats,
            SourceCall::Feed(SourceDemand::Chess(_)) => crate::replay::InputFamily::Chess,
            SourceCall::Feed(SourceDemand::Weather(_)) => crate::replay::InputFamily::Weather,
            SourceCall::Operation(_) => {
                return Err(invalid("operation cannot be captured as feed"))
            }
        };
        let state = run.dispatcher.actor_client().state()?;
        let (owner, fence, quota) = state.authority.capture_context()?;
        drop(state);
        if !Arc::ptr_eq(&owner, &self.owner) || !quota.shares_root(&self.quota) {
            return Err(denied("source feed capture host owner/root mismatch"));
        }
        crate::native_source_capture::NativeCapturedFeed::from_native_source(
            instance,
            owner,
            fence,
            quota,
            family,
            snapshot,
            self.replay_lineage.clone(),
            max_resident_bytes,
        )
    }
    pub fn next_due_ms(&self) -> Result<Option<u64>> {
        if self.stage.is_some() {
            return Ok(None);
        }
        let Some(FeedOwnedState::Ready(run)) = self.owned.as_ref() else {
            return Ok(None);
        };
        Ok(Some(
            run.view()
                .dispatcher
                .next_due_ms(
                    run.view()
                        .handle
                        .ok_or_else(|| invalid("feed handle absent"))?,
                )
                .ok_or_else(|| invalid("feed due state absent"))?
                .max(self.blocked_until_ms),
        ))
    }
    pub fn begin_due(&mut self, instance: &PackageInstance, clock: SourceClock) -> Result<bool> {
        if self.stop.is_stopped()
            || self
                .next_due_ms()?
                .is_none_or(|due| clock.monotonic_ms < due)
        {
            return Ok(false);
        }
        if self.stage.is_some() {
            return Ok(false);
        }
        if !matches!(self.owned.as_ref(), Some(FeedOwnedState::Ready(_))) {
            return Ok(false);
        }
        let Some(FeedOwnedState::Ready(retained)) = self.owned.take() else {
            unreachable!("checked source feed ready state");
        };
        let (mut run, hold) = retained.into_parts();
        if let Err(error) = run.prepare_refresh(clock) {
            self.owned = Some(FeedOwnedState::Ready(hold.retain(run)));
            self.blocked_until_ms = clock.monotonic_ms.saturating_add(1_000);
            return Err(error);
        }
        let context = match run
            .dispatcher
            .actor_client()
            .state()
            .and_then(|state| state.authority.feed_context())
        {
            Ok(context) => context,
            Err(error) => {
                if let Ok(mut state) = run.dispatcher.actor_client().state() {
                    let _ = state.authority.finish_feed_cycle();
                }
                self.owned = Some(FeedOwnedState::Ready(hold.retain(run)));
                self.blocked_until_ms = clock.monotonic_ms.saturating_add(1_000);
                return Err(error);
            }
        };
        let fallback = hold.clone();
        let job = SourceCpuJob {
            run,
            previous: Some(hold),
        };
        let pending = Cell::new(Some(job));
        let issued = instance.with_source_feed_planning(context, || {
            // This synchronous gate invokes its issue closure at most once.
            try_submit_preserving_rejection(&self.client, Lane::Cpu, cpu_cost(), &pending)
        });
        match issued {
            Ok(Some(receipt)) => {
                self.stage = Some(FeedStage::Cpu {
                    receipt,
                    operation: None,
                })
            }
            Ok(None) => {
                let job = pending
                    .take()
                    .expect("rejected source CPU job must remain in caller custody");
                let run = job.run;
                run.dispatcher
                    .actor_client()
                    .state()?
                    .authority
                    .finish_feed_cycle()?;
                self.owned = Some(FeedOwnedState::Ready(fallback.retain(run)));
                self.blocked_until_ms = clock.monotonic_ms.saturating_add(1_000);
                return Err(AnimationError::Budget(
                    "source feed CPU admission refused".into(),
                ));
            }
            Err(error) => {
                if let Some(job) = pending.take() {
                    let run = job.run;
                    run.dispatcher
                        .actor_client()
                        .state()?
                        .authority
                        .finish_feed_cycle()?;
                    self.owned = Some(FeedOwnedState::Ready(fallback.retain(run)));
                }
                return Err(error);
            }
        }
        Ok(true)
    }
    fn cpu_resume(
        &mut self,
        instance: &mut PackageInstance,
        run: SourceRun,
        hold: Retention,
        operation: Option<SourceFeedOperation>,
    ) -> Result<()> {
        let context = match run
            .dispatcher
            .actor_client()
            .state()
            .and_then(|state| state.authority.feed_context())
        {
            Ok(context) => context,
            Err(error) => {
                if let Ok(mut state) = run.dispatcher.actor_client().state() {
                    let _ = state.authority.finish_feed_cycle();
                }
                self.owned = Some(FeedOwnedState::Ready(hold.retain(run)));
                if let Some(ticket) = operation.as_ref() {
                    instance.settle_source_feed_terminal(ticket)?;
                }
                return Err(error);
            }
        };
        let fallback = hold.clone();
        let job = SourceCpuJob {
            run,
            previous: Some(hold),
        };
        let pending = Cell::new(Some(job));
        let issued = match &operation {
            Some(ticket) => instance.with_source_feed_cpu_authority(ticket, || {
                try_submit_preserving_rejection(&self.client, Lane::Cpu, cpu_cost(), &pending)
            }),
            None => instance.with_source_feed_planning(context, || {
                try_submit_preserving_rejection(&self.client, Lane::Cpu, cpu_cost(), &pending)
            }),
        };
        match issued {
            Ok(Some(receipt)) => self.stage = Some(FeedStage::Cpu { receipt, operation }),
            Ok(None) => {
                let job = pending
                    .take()
                    .expect("rejected source CPU continuation must remain in caller custody");
                let run = job.run;
                run.dispatcher
                    .actor_client()
                    .state()?
                    .authority
                    .finish_feed_cycle()?;
                self.blocked_until_ms = run.clock.monotonic_ms.saturating_add(1_000);
                self.owned = Some(FeedOwnedState::Ready(fallback.retain(run)));
                if let Some(ticket) = operation {
                    instance.settle_source_feed_terminal(&ticket)?;
                }
                return Err(AnimationError::Budget(
                    "source feed CPU continuation refused".into(),
                ));
            }
            Err(error) => {
                if let Some(job) = pending.take() {
                    let run = job.run;
                    run.dispatcher
                        .actor_client()
                        .state()?
                        .authority
                        .finish_feed_cycle()?;
                    self.owned = Some(FeedOwnedState::Ready(fallback.retain(run)));
                }
                if let Some(ticket) = operation {
                    instance.settle_source_feed_terminal(&ticket)?;
                }
                return Err(error);
            }
        }
        Ok(())
    }
    fn fetch(
        &mut self,
        instance: &mut PackageInstance,
        mut run: SourceRun,
        hold: Retention,
        operation: Option<SourceFeedOperation>,
    ) -> Result<()> {
        let mut cleanup_operation = operation.clone();
        let result = (|| -> Result<()> {
            let plan = run.take_plan()?;
            let need = crate::permissions::OperationNeed::http_preflight(
                &plan.options.url,
                crate::permissions::HttpMethod::Get,
            )
            .map_err(|_| invalid("source feed endpoint invalid"))?;
            let initial = operation.is_none();
            let operation = match operation {
                Some(ticket) => ticket,
                None => {
                    let state = run.dispatcher.actor_client().state()?;
                    instance.dispatch_source_feed_http_service(&state.authority, need.clone())?
                }
            };
            cleanup_operation = Some(operation.clone());
            if initial {
                let admitted = instance
                    .with_source_feed_unissued(&operation, || run.admit_refresh_at_issue())?;
                if !admitted {
                    instance.settle_source_feed_terminal(&operation)?;
                    return self.cpu_resume(instance, run, hold, None);
                }
            }
            let selected = self.credentials.as_ref().map(|value| {
                Box::new(SourceCredentialAdapter(Arc::clone(value)))
                    as Box<dyn crate::native_http_host::HostCredentialAdapter>
            });
            let factory = instance.source_feed_http_authority_factory(&operation, selected)?;
            let cost = io_cost(&plan)?;
            let fallback = hold.clone();
            let job = SourceIoJob {
                run: hold.retain(run),
                plan,
                factory,
                client: self.client.clone(),
                dns: Arc::clone(&self.dns),
                quota: self.quota.clone(),
            };
            let pending = Cell::new(Some(job));
            let issued = if initial {
                instance.commit_source_feed(&operation, || {
                    try_submit_preserving_rejection(&self.client, Lane::Io, cost, &pending)
                })
            } else {
                instance.with_source_feed_http_authority(&operation, &[need], || {
                    try_submit_preserving_rejection(&self.client, Lane::Io, cost, &pending)
                })
            };
            match issued {
                Ok(Some(receipt)) => self.stage = Some(FeedStage::Io { receipt, operation }),
                Ok(None) => {
                    let job = pending
                        .take()
                        .expect("rejected source IO job must remain in caller custody");
                    let (run, _held) = job.run.into_parts();
                    run.dispatcher
                        .actor_client()
                        .state()?
                        .authority
                        .finish_feed_cycle()?;
                    self.blocked_until_ms = run.clock.monotonic_ms.saturating_add(1_000);
                    self.owned = Some(FeedOwnedState::Ready(fallback.retain(run)));
                    instance.settle_source_feed_terminal(&operation)?;
                    return Err(AnimationError::Budget(
                        "source feed IO admission refused".into(),
                    ));
                }
                Err(error) => {
                    if let Some(job) = pending.take() {
                        let (run, _original_hold) = job.run.into_parts();
                        run.dispatcher
                            .actor_client()
                            .state()?
                            .authority
                            .finish_feed_cycle()?;
                        self.owned = Some(FeedOwnedState::Ready(fallback.retain(run)));
                    }
                    instance.settle_source_feed_terminal(&operation)?;
                    return Err(error);
                }
            }
            Ok(())
        })();
        if result.is_err() {
            if let Some(ticket) = cleanup_operation.as_ref() {
                instance.settle_source_feed_terminal(ticket)?;
            }
            if self.stage.is_none() && self.owned.is_none() {
                self.stage = Some(FeedStage::Closed);
            }
        }
        result
    }
    /// Only a genuine finite completion wake takes receipts; frame ticks call
    /// begin_due only and never infer worker completion from elapsed time.
    pub fn on_completion_wake(
        &mut self,
        instance: &mut PackageInstance,
    ) -> Result<Option<SourceFeedEvent>> {
        instance.check_http_owner(&self.owner, &self.quota)?;
        let Some(stage) = self.stage.take() else {
            return Ok(None);
        };
        match stage {
            FeedStage::Cpu {
                mut receipt,
                operation,
            } => match receipt.try_take() {
                JobPoll::Pending => {
                    self.stage = Some(FeedStage::Cpu { receipt, operation });
                    Ok(None)
                }
                JobPoll::Lost | JobPoll::Taken => {
                    self.stage = Some(FeedStage::LostCpu {
                        _receipt: receipt,
                        _operation: operation,
                    });
                    Ok(Some(SourceFeedEvent::Lost))
                }
                JobPoll::Ready(held) => {
                    let (outcome, hold) = held.into_parts();
                    if self.stop.is_stopped() {
                        if let Some(ticket) = operation.as_ref() {
                            instance.settle_source_feed_terminal(ticket)?;
                        }
                        drop(outcome);
                        drop(hold);
                        self.stage = Some(FeedStage::Closed);
                        return Ok(Some(SourceFeedEvent::Failed("source_feed_cancelled")));
                    }
                    match outcome {
                        JobOutcome::Finished(Ok(step))
                            if step.output.as_ref().is_ok_and(Option::is_none) =>
                        {
                            self.fetch(instance, step.run, hold, operation)?;
                            Ok(None)
                        }
                        JobOutcome::Finished(Ok(step)) => {
                            if self.owned.is_some() {
                                self.stage = Some(FeedStage::Closed);
                                return Err(invalid("source feed completion already retained"));
                            }
                            self.owned = Some(FeedOwnedState::Completion(SourceFeedCompletion {
                                step: hold.retain(step),
                                operation,
                            }));
                            Ok(Some(SourceFeedEvent::Complete))
                        }
                        JobOutcome::NotStarted { job, .. } => {
                            let (run, previous) = (job.run, job.previous);
                            run.dispatcher
                                .actor_client()
                                .state()?
                                .authority
                                .finish_feed_cycle()?;
                            self.owned = Some(FeedOwnedState::Ready(hold.retain(run)));
                            drop(previous);
                            if let Some(ticket) = operation {
                                instance.settle_source_feed_terminal(&ticket)?;
                            }
                            Ok(Some(SourceFeedEvent::Failed("source_feed_cpu_not_started")))
                        }
                        JobOutcome::Finished(Err(_)) | JobOutcome::Panicked => {
                            self.stage = Some(FeedStage::Closed);
                            if let Some(ticket) = operation {
                                instance.settle_source_feed_terminal(&ticket)?;
                            }
                            Ok(Some(SourceFeedEvent::Failed("source_feed_cpu_failed")))
                        }
                    }
                }
            },
            FeedStage::Io {
                mut receipt,
                operation,
            } => match receipt.try_take() {
                JobPoll::Pending => {
                    self.stage = Some(FeedStage::Io { receipt, operation });
                    Ok(None)
                }
                JobPoll::Lost | JobPoll::Taken => {
                    self.stage = Some(FeedStage::LostIo {
                        _receipt: receipt,
                        _operation: operation,
                    });
                    Ok(Some(SourceFeedEvent::Lost))
                }
                JobPoll::Ready(held) => {
                    let (outcome, hold) = held.into_parts();
                    if self.stop.is_stopped() {
                        instance.settle_source_feed_terminal(&operation)?;
                        drop(outcome);
                        drop(hold);
                        self.stage = Some(FeedStage::Closed);
                        return Ok(Some(SourceFeedEvent::Failed("source_feed_cancelled")));
                    }
                    match outcome {
                        JobOutcome::Finished(Ok(step)) => self
                            .cpu_resume(instance, step.run, hold, Some(operation))
                            .map(|_| None),
                        JobOutcome::NotStarted { job, .. } => {
                            let (run, previous) = job.run.into_parts();
                            run.dispatcher
                                .actor_client()
                                .state()?
                                .authority
                                .finish_feed_cycle()?;
                            self.owned = Some(FeedOwnedState::Ready(hold.retain(run)));
                            drop(previous);
                            instance.settle_source_feed_terminal(&operation)?;
                            Ok(Some(SourceFeedEvent::Failed("source_feed_io_not_started")))
                        }
                        JobOutcome::Finished(Err(_)) | JobOutcome::Panicked => {
                            self.stage = Some(FeedStage::Closed);
                            instance.settle_source_feed_terminal(&operation)?;
                            Ok(Some(SourceFeedEvent::Failed("source_feed_io_failed")))
                        }
                    }
                }
            },
            other => {
                self.stage = Some(other);
                Ok(None)
            }
        }
    }
    pub fn take_completion(&mut self) -> Result<SourceFeedCompletion> {
        match self.owned.take() {
            Some(FeedOwnedState::Completion(completion)) => Ok(completion),
            other => {
                self.owned = other;
                Err(invalid("source feed completion custody absent"))
            }
        }
    }
    pub fn resume(&mut self, completion: SourceFeedCompletion) -> Result<()> {
        if self.stage.is_some() || self.owned.is_some() {
            return Err(invalid("feed completion while actor active"));
        }
        let run = completion.into_run();
        run.run
            .view()
            .dispatcher
            .actor_client()
            .state()?
            .authority
            .finish_feed_cycle()?;
        self.owned = Some(FeedOwnedState::Ready(run.run));
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.stop.stop();
        self.worker_stop.store(true, Ordering::Release);
        match self.stage.as_ref() {
            Some(FeedStage::Cpu { receipt, .. })
            | Some(FeedStage::LostCpu {
                _receipt: receipt, ..
            }) => receipt.cancel(),
            Some(FeedStage::Io { receipt, .. })
            | Some(FeedStage::LostIo {
                _receipt: receipt, ..
            }) => receipt.cancel(),
            _ => {}
        }
        if self.owned.take().is_some() {
            self.stage = Some(FeedStage::Closed);
        }
    }
    pub fn is_drained(&self) -> bool {
        matches!(self.stage, Some(FeedStage::Closed))
    }
}
struct FeedProjectionState {
    service_revision: Option<u64>,
    provider_failed: bool,
}

fn project_feed_descriptor(
    instance: &mut PackageInstance,
    drawing: &mut NativeDrawHost,
    native_id: &str,
    method: &str,
    snapshot: Option<&AdmittedSourceSnapshot>,
    quota: &QuotaGroup,
    state: FeedProjectionState,
) -> Result<ProjectedFeedDescriptor> {
    let FeedProjectionState {
        service_revision,
        provider_failed,
    } = state;
    let kind = method
        .strip_suffix(".open")
        .ok_or_else(|| invalid("feed method suffix"))?;
    if !matches!(
        kind,
        "sources.series"
            | "sources.earthquakes"
            | "sources.aircraft"
            | "sources.boats"
            | "sources.chess"
            | "sources.weather"
    ) || native_id.len() > 128
        || !native_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(invalid("native feed descriptor identity"));
    }
    let bytes = match snapshot {
        Some(value) => value.view().owned_bytes()?,
        None => 0,
    };
    let bound = bytes
        .checked_mul(2)
        .and_then(|value| value.checked_add(64 * 1024))
        .ok_or_else(|| invalid("feed snapshot projection bound"))?;
    let admission = charge(quota, bound)?;
    let mut imported = Vec::new();
    let result = (|| -> Result<Value> {
        let mut latest = match snapshot {
            Some(value) => serde_json::to_value(value.view())?,
            None => Value::Null,
        };
        if let Some(value) = snapshot {
            let images = value.native_images();
            if images.len() > 64 {
                return Err(invalid("feed image inventory"));
            }
            if kind == "sources.weather" {
                let mut used = BTreeMap::new();
                let layers = latest["layers"]
                    .as_array_mut()
                    .ok_or_else(|| invalid("weather layers schema"))?;
                for layer in layers {
                    if let Some(tiles) = layer.get_mut("tiles").and_then(Value::as_array_mut) {
                        for tile in tiles {
                            import_feed_image_slot(
                                &mut tile["image"],
                                images,
                                &mut used,
                                drawing,
                                instance,
                                &mut imported,
                            )?;
                        }
                    }
                    if let Some(frames) = layer.get_mut("frames").and_then(Value::as_array_mut) {
                        for frame in frames {
                            if frame.get("image").is_some() {
                                import_feed_image_slot(
                                    &mut frame["image"],
                                    images,
                                    &mut used,
                                    drawing,
                                    instance,
                                    &mut imported,
                                )?;
                            }
                            if let Some(tiles) =
                                frame.get_mut("tiles").and_then(Value::as_array_mut)
                            {
                                for tile in tiles {
                                    import_feed_image_slot(
                                        &mut tile["image"],
                                        images,
                                        &mut used,
                                        drawing,
                                        instance,
                                        &mut imported,
                                    )?;
                                }
                            }
                        }
                    }
                }
                if used.len() != images.len() {
                    return Err(invalid("weather image allocation not projected"));
                }
            } else if !images.is_empty() {
                return Err(invalid("nonweather feed contains native images"));
            }
        }
        let revision = service_revision
            .unwrap_or_else(|| latest.get("revision").and_then(Value::as_u64).unwrap_or(0));
        let state = if provider_failed || latest["status"] == "error" {
            "error"
        } else if snapshot.is_none() {
            "preparing"
        } else {
            "ready"
        };
        Ok(
            json!({"id":native_id,"kind":kind,"revision":revision,"status":{"state":state},"latest":latest}),
        )
    })();
    match result {
        Ok(metadata) => match sources::types::json_owned_bytes(&metadata) {
            Ok(owned) if owned <= bound => Ok(ProjectedFeedDescriptor {
                metadata,
                imported,
                metadata_bound: bound,
                _admission: admission,
            }),
            outcome => {
                for image in imported.iter().rev() {
                    drawing.release_source_image(image)?;
                }
                match outcome {
                    Err(error) => Err(error),
                    Ok(_) => Err(invalid("feed snapshot projection exceeded admission")),
                }
            }
        },
        Err(error) => {
            for image in imported.iter().rev() {
                drawing.release_source_image(image)?;
            }
            Err(error)
        }
    }
}
fn import_feed_image_slot(
    marker: &mut Value,
    images: &[NativeSourceImage],
    used: &mut BTreeMap<usize, Value>,
    drawing: &mut NativeDrawHost,
    instance: &mut PackageInstance,
    imported: &mut Vec<String>,
) -> Result<()> {
    let source = marker
        .as_object()
        .ok_or_else(|| invalid("feed image marker"))?;
    if source.len() != 3 {
        return Err(invalid("feed image marker fields"));
    }
    let slot = source
        .get("native_image_slot")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| invalid("feed image slot"))?;
    let image = images
        .get(slot)
        .ok_or_else(|| invalid("feed image slot bound"))?;
    let pixels = image.admitted_pixels().view();
    if source.get("width").and_then(Value::as_u64) != Some(u64::from(pixels.width))
        || source.get("height").and_then(Value::as_u64) != Some(u64::from(pixels.height))
    {
        return Err(invalid("feed image dimensions"));
    }
    if let Some(descriptor) = used.get(&slot) {
        *marker = descriptor.clone();
        return Ok(());
    }
    let descriptor = drawing.retain_source_image(instance, image)?;
    let key = descriptor["id"]
        .as_str()
        .ok_or_else(|| invalid("feed image registration"))?
        .to_owned();
    imported.push(key);
    used.insert(slot, descriptor.clone());
    *marker = descriptor;
    Ok(())
}
fn import_image_slot(
    marker: &mut Value,
    images: &[NativeSourceImage],
    used: &mut [bool],
    drawing: &mut NativeDrawHost,
    instance: &mut PackageInstance,
    imported: &mut Vec<String>,
) -> Result<()> {
    let source = marker
        .as_object()
        .ok_or_else(|| invalid("source image marker"))?;
    if source.len() != 3 {
        return Err(invalid("source image marker fields"));
    }
    let slot = source
        .get("native_image_slot")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| invalid("source image slot"))?;
    let image = images
        .get(slot)
        .ok_or_else(|| invalid("source image slot bound"))?;
    if used[slot] {
        return Err(invalid("source image slot duplicated"));
    }
    let pixels = image.admitted_pixels().view();
    if source.get("width").and_then(Value::as_u64) != Some(u64::from(pixels.width))
        || source.get("height").and_then(Value::as_u64) != Some(u64::from(pixels.height))
    {
        return Err(invalid("source image marker dimensions"));
    }
    let descriptor = drawing.retain_source_image(instance, image)?;
    let key = descriptor["id"]
        .as_str()
        .ok_or_else(|| invalid("native source image registration ID"))?
        .to_owned();
    imported.push(key);
    *marker = descriptor;
    used[slot] = true;
    Ok(())
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
    pub fn copy_error(&self, instance: &PackageInstance) -> Result<ServiceValue> {
        instance.check_http_owner(&self.owner, &self.quota)?;
        if self.request.is_cancelled() {
            return Err(denied("source error request retired"));
        }
        ServiceValue::copy_from_host(
            &json!({"ok":false,"error":{"code":self.code,"message":"Native source operation failed."}}),
            &[],
            &BTreeMap::new(),
            instance.engine_limits(),
            self.quota.clone(),
        )
    }
    /// Only an admitted, native-produced structured error may be passed here.
    /// The original ticket and terminal retention survive a refused copy ACK.
    pub fn publish_error(
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
    pub fn settle(&self, instance: &mut PackageInstance) -> Result<()> {
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
            #[cfg(any(test, feature = "qualification-test-support"))]
            offline_http,
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
            #[cfg(any(test, feature = "qualification-test-support"))]
            offline_http,
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
#[path = "native_source_qualification_tests.rs"]
mod native_source_qualification_tests;

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

    #[test]
    fn offline_source_fixture_requires_exact_https_authority_and_bounded_responses() {
        let public_address = "93.184.216.34:443".parse().unwrap();
        assert!(OfflineSourceHttp::new(
            "http://earthquake.usgs.gov/feed".into(),
            public_address,
            vec![b"fixture".to_vec()],
        )
        .is_err());
        assert!(OfflineSourceHttp::new(
            "https://earthquake.usgs.gov/feed".into(),
            "93.184.216.34:444".parse().unwrap(),
            vec![b"fixture".to_vec()],
        )
        .is_err());
        assert!(OfflineSourceHttp::new(
            "https://earthquake.usgs.gov/feed".into(),
            "127.0.0.1:443".parse().unwrap(),
            vec![b"fixture".to_vec()],
        )
        .is_err());
        assert!(OfflineSourceHttp::new(
            "https://earthquake.usgs.gov/feed".into(),
            "[::ffff:127.0.0.1]:443".parse().unwrap(),
            vec![b"fixture".to_vec()],
        )
        .is_err());
        assert!(OfflineSourceHttp::new(
            "https://earthquake.usgs.gov/feed".into(),
            public_address,
            Vec::new(),
        )
        .is_err());
    }

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
    #[test]
    fn process_cadence_shares_one_original_quota_and_last_owner_charge() {
        let quota = quota();
        let baseline = quota.snapshot().worker_bytes;
        let cadence = Arc::new(SourceCadence::new(quota.clone()).unwrap());
        assert!(cadence.shares_root(&quota));
        assert_eq!(quota.snapshot().worker_bytes, baseline + 128 * 1024);
        let second_actor = Arc::clone(&cadence);
        assert!(cadence.admit(&quota, "usgs", 1000, 5000).unwrap());
        assert!(!second_actor.admit(&quota, "usgs", 2000, 5000).unwrap());
        assert!(second_actor.admit(&quota, "usgs", 6000, 5000).unwrap());
        let foreign = self::quota();
        assert!(!second_actor.shares_root(&foreign));
        assert!(second_actor.admit(&foreign, "usgs", 12_000, 5000).is_err());
        drop(cadence);
        assert_eq!(quota.snapshot().worker_bytes, baseline + 128 * 1024);
        drop(second_actor);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
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
            offline_http: None,
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
    fn admitted_feed_dispatcher_refreshes_after_opener_retirement_with_injected_body() {
        let quota = quota();
        let mut run = run("sources.earthquakes.open", geo(), &quota);
        let handle = run.handle.unwrap();
        assert!(run.cpu_step().unwrap().is_none());
        let first = run.take_plan().unwrap();
        let fixture = br#"{"type":"FeatureCollection","features":[]}"#;
        let response = SourceHttpResponse::read(
            200,
            &mut Cursor::new(fixture),
            &quota,
            4096,
            &AtomicBool::new(false),
        )
        .unwrap();
        run.install(first, Ok(CachedTransport::Body(response)))
            .unwrap();
        let Some(NativeSourceOutput::Snapshot(Some(first))) = run.cpu_step().unwrap() else {
            panic!("first injected provider snapshot missing")
        };
        let sources::SourceSnapshot::Geographic(first) = first.view() else {
            panic!("first geographic snapshot missing")
        };
        assert_eq!(first.metadata.revision, 1);

        let stop = StopToken::default();
        {
            let mut state = run.dispatcher.actor_client().state().unwrap();
            state.authority.promote_to_feed(stop.clone()).unwrap();
            state.authority.request.stop_token().stop();
        }
        let due = run.dispatcher.next_due_ms(handle).unwrap();
        run.prepare_refresh(SourceClock {
            monotonic_ms: due.saturating_add(1),
            epoch_ms: 1_700_000_000_000_i64 + due as i64,
        })
        .unwrap();
        assert!(run.cpu_step().unwrap().is_none());
        let second = run.take_plan().unwrap();
        let response = SourceHttpResponse::read(
            200,
            &mut Cursor::new(fixture),
            &quota,
            4096,
            &AtomicBool::new(false),
        )
        .unwrap();
        run.install(second, Ok(CachedTransport::Body(response)))
            .unwrap();
        let Some(NativeSourceOutput::Snapshot(Some(second))) = run.cpu_step().unwrap() else {
            panic!("refresh provider snapshot missing")
        };
        let sources::SourceSnapshot::Geographic(second) = second.view() else {
            panic!("refresh geographic snapshot missing")
        };
        assert_eq!(second.metadata.revision, 2);
        assert!(run.dispatcher.next_due_ms(handle).unwrap() > due);
        run.dispatcher
            .actor_client()
            .state()
            .unwrap()
            .authority
            .finish_feed_cycle()
            .unwrap();
        stop.stop();
        assert!(run.dispatcher.actor_client().check().is_err());
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
    fn delivered_feed_transfer_outlives_opener_but_not_feed_stop_or_broker_epoch() {
        let quota = quota();
        let mut run = run(
            "sources.series.open",
            json!({"provider":"crypto","max_samples":1,"interval_ms":60000}),
            &quota,
        );
        let token = StopToken::default();
        let broker = {
            let mut state = run.dispatcher.actor_client().state().unwrap();
            let broker = Arc::clone(&state.authority.broker);
            state.authority.promote_to_feed(token.clone()).unwrap();
            state.authority.request.stop_token().stop();
            broker
        };
        assert!(run.dispatcher.actor_client().check().is_ok());
        run.prepare_refresh(SourceClock {
            monotonic_ms: 61_000,
            epoch_ms: 1_700_000_060_000,
        })
        .unwrap();
        assert!(run
            .dispatcher
            .actor_client()
            .state()
            .unwrap()
            .authority
            .feed_context()
            .is_ok());
        assert!(
            run.dispatcher
                .actor_client()
                .state()
                .unwrap()
                .authority
                .remaining_ms()
                > 0
        );
        run.dispatcher
            .actor_client()
            .state()
            .unwrap()
            .authority
            .finish_feed_cycle()
            .unwrap();
        token.stop();
        assert!(run.dispatcher.actor_client().check().is_err());

        let run = super::tests::run(
            "sources.series.open",
            json!({"provider":"crypto","max_samples":1,"interval_ms":60000}),
            &quota,
        );
        let replay = run.dispatcher.actor_client().clone();
        replay
            .state()
            .unwrap()
            .authority
            .promote_to_feed(StopToken::default())
            .unwrap();
        let owner = Arc::clone(&replay.state().unwrap().authority.broker);
        let _invalidation = owner.lock().unwrap().reset_all_decisions().unwrap();
        assert!(replay.check().is_err());
        drop(broker);
    }
    #[test]
    fn closed_native_weather_image_is_not_reseeded_from_cached_feed_latest() {
        let quota = quota();
        let baseline = quota.snapshot().worker_bytes;
        let mut descriptor = ProjectedFeedDescriptor {
            metadata: json!({"id":"source-feed-1","kind":"sources.weather","revision":1,
                "status":{"state":"ready"},"latest":{"revision":1,"available":true,
                "layers":[{"tiles":[{"image":{"id":"source-image-1","kind":"image"}}]}]}}),
            imported: vec!["source-image-1".into()],
            metadata_bound: 64 * 1024,
            _admission: charge(&quota, 64 * 1024).unwrap(),
        };
        descriptor.image_closed("source-image-foreign").unwrap();
        assert_eq!(descriptor.metadata()["revision"], 1);
        descriptor.image_closed("source-image-1").unwrap();
        assert_eq!(descriptor.metadata()["revision"], 2);
        assert!(descriptor.metadata()["latest"].is_null());
        descriptor.image_closed("source-image-1").unwrap();
        assert_eq!(descriptor.metadata()["revision"], 2);
        drop(descriptor);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
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
    fn refused_source_submit_restores_exact_unqueued_job_without_new_debit() {
        use ilium_execution::{ClientLimits, Execution, ExecutionConfig, LaneConfig, ShutdownMode};
        let quota = quota();
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
                input_bytes: 8 * 1024 * 1024,
                result_bytes: 8 * 1024 * 1024,
            })
            .unwrap();
        let run = run(
            "sources.astronomy.observe",
            json!({"epoch_ms":0,"latitude":0.0,"longitude":0.0}),
            &quota,
        );
        let stop = Arc::clone(&run.stop);
        let baseline = quota.snapshot();
        let pending = Cell::new(Some(SourceCpuJob {
            run,
            previous: None,
        }));
        assert!(
            try_submit_preserving_rejection(&client, Lane::Cpu, cpu_cost(), &pending,).is_none()
        );
        let recovered = pending
            .take()
            .expect("disabled source lane must return the exact unqueued job");
        assert!(Arc::ptr_eq(&recovered.run.stop, &stop));
        assert_eq!(quota.snapshot().jobs, baseline.jobs);
        assert_eq!(quota.snapshot().worker_bytes, baseline.worker_bytes);
        let stage_bytes = std::hint::black_box(std::mem::size_of::<FeedStage>());
        let ready_bytes = std::hint::black_box(std::mem::size_of::<Retained<SourceRun>>());
        let event_bytes = std::hint::black_box(std::mem::size_of::<SourceFeedEvent>());
        let pointer_bytes = std::hint::black_box(std::mem::size_of::<usize>());
        let owned_bytes = std::hint::black_box(std::mem::size_of::<FeedOwnedState>());
        let completion_bytes = std::hint::black_box(std::mem::size_of::<SourceFeedCompletion>());
        assert!(
            stage_bytes < ready_bytes,
            "ready payload must not be stored inline in the feed stage enum"
        );
        assert!(
            event_bytes <= 3 * pointer_bytes,
            "feed completion payload must stay in its already admitted host owner"
        );
        assert!(
            owned_bytes < ready_bytes + completion_bytes,
            "mutually exclusive retained feed owners must share one inline slot"
        );
        drop(recovered);
        drop(client);
        execution.request_shutdown(ShutdownMode::Drain);
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(report.remaining_workers, 0);
    }

    #[test]
    fn ready_feed_cancel_releases_retention_only_after_original_run_is_dropped() {
        use ilium_execution::{ClientLimits, Execution, ExecutionConfig, LaneConfig, ShutdownMode};
        let quota = quota();
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let mut execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: None,
                    resident_bytes_per_thread: 1024,
                },
                io: disabled,
                service: disabled,
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 2,
                service_jobs: 0,
                input_bytes: 8 * 1024 * 1024,
                result_bytes: 8 * 1024 * 1024,
            })
            .unwrap();
        let run = run(
            "sources.series.open",
            json!({"provider":"crypto","max_samples":1,"interval_ms":60000}),
            &quota,
        );
        let worker_stop = Arc::clone(&run.stop);
        let owner = Arc::clone(
            &run.dispatcher
                .actor_client()
                .state()
                .unwrap()
                .authority
                .broker,
        );
        let baseline_jobs = quota.snapshot().jobs;
        let reservation = client.try_reserve(Lane::Cpu, cpu_cost()).unwrap();
        let retained = reservation.retention().retain(run);
        drop(reservation);
        assert_eq!(quota.snapshot().jobs, baseline_jobs + 1);
        let mut feed = NativeSourceFeedHost {
            client: client.clone(),
            quota: quota.clone(),
            owner,
            replay_lineage: Vec::new(),
            dns: Arc::new(http::SystemDns),
            credentials: None,
            stop: StopToken::default(),
            worker_stop: Arc::clone(&worker_stop),
            owned: Some(FeedOwnedState::Ready(retained)),
            stage: None,
            blocked_until_ms: 0,
            _registry: charge(&quota, 64 * 1024).unwrap(),
        };
        feed.cancel();
        assert!(feed.is_drained());
        assert!(worker_stop.load(Ordering::Acquire));
        assert_eq!(quota.snapshot().jobs, baseline_jobs);
        drop(feed);
        drop(client);
        execution.request_shutdown(ShutdownMode::Drain);
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(report.remaining_workers, 0);
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
