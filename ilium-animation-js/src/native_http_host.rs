//! Finite native SDK HTTP owner: original IO receipt -> CPU receipt -> genuine
//! authorized helper copy/ACK. No new execution bank and no nested job waits.
pub use crate::native_http_authority::{BoundCredentialHeaders, HostCredentialAdapter};
use crate::{
    engine::{ArraySpec, CompletionState, EngineLimits, HostRequest, ServiceValue, TypedArrayKind},
    error::{AnimationError, Result},
    http::{self, DnsResolver, HttpOptions, HttpResponseHead},
    native_http_authority::{NativeHttpAuthority, NativeHttpAuthorityFactory},
    permissions::{HttpMethod, OperationNeed, PermissionBroker},
    runtime::{PackageInstance, ServiceOperation},
    sources::SourceHttpResponse,
};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, QuotaGroup, Receipt, Retained,
    Retention, StorageAdmission,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    mem::size_of,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

const REGISTRY_BYTES: usize = 128 * 1024;
const PREPARATION_BYTES: usize = 64 * 1024;
const MAX_PENDING: usize = 16;
const MAX_REQUEST_BODY: usize = 4 * 1024 * 1024;
const TRANSPORT_SCRATCH: usize = 4 * 1024 * 1024;
const DECODE_SCRATCH: usize = 1024 * 1024;
const HEADER_RETENTION: usize = 2 * 1024 * 1024;

fn invalid(message: &'static str) -> AnimationError {
    AnimationError::Runtime(message.into())
}
fn permission(message: &'static str) -> AnimationError {
    AnimationError::PermissionDenied(message.into())
}
fn cost_overflow() -> AnimationError {
    AnimationError::Budget("native HTTP cost overflow".into())
}
fn error_code(error: &AnimationError) -> &'static str {
    match error {
        AnimationError::PermissionDenied(_) => "permission_denied",
        AnimationError::Budget(_) => "budget_exceeded",
        AnimationError::Json(_) => "invalid_json",
        _ => "http_failed",
    }
}
fn failure_value(
    code: &'static str,
    quota: &QuotaGroup,
    limits: &EngineLimits,
) -> Result<ServiceValue> {
    // Fixed bounded native errors; never echo credential/query/body/error strings.
    let message = match code {
        "http_effects_unknown" => "Native HTTP callback panicked; remote effects are unknown.",
        "http_not_started" => "The admitted HTTP job was never started.",
        "http_decode_not_started" => {
            "HTTP finished, but the response decode job was never started."
        }
        "http_decode_panicked" => "HTTP finished, but the response decode callback panicked.",
        _ => "Native HTTP request did not complete successfully.",
    };
    let value = json!({"ok":false,"error":{"code":code,"message":message}});
    ServiceValue::copy_from_host(&value, &[], &BTreeMap::new(), limits, quota.clone())
}
struct SharedCredentialAdapter(Arc<Mutex<Box<dyn HostCredentialAdapter>>>);
impl HostCredentialAdapter for SharedCredentialAdapter {
    fn bound_headers(
        &mut self,
        principal: &crate::permissions::PackageIdentity,
        handle: &str,
        origin: &str,
        quota: &QuotaGroup,
    ) -> Result<BoundCredentialHeaders> {
        self.0
            .lock()
            .map_err(|_| permission("selected credential backend poisoned"))?
            .bound_headers(principal, handle, origin, quota)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ResponseKind {
    Bytes,
    Text,
    Json,
}
enum RequestBody {
    None,
    Text,
    Binary { name: String, bytes: usize },
}
struct PreparedRequest {
    options: HttpOptions,
    body: RequestBody,
    body_bytes: usize,
    response: ResponseKind,
    _metadata: StorageAdmission,
}
impl PreparedRequest {
    /// Validate borrowed immutable SDK wire shape BEFORE allocating request data.
    fn parse(request: &HostRequest, limits: &EngineLimits, quota: &QuotaGroup) -> Result<Self> {
        if request.method != "http.request"
            || !request.payload.shares_root(quota)
            || request.is_cancelled()
        {
            return Err(permission("HTTP input owner/lifetime"));
        }
        request.payload.validate_limits(limits)?;
        let fields = request
            .payload
            .metadata()
            .as_object()
            .ok_or_else(|| invalid("HTTP options record"))?;
        if fields.len() > 9
            || fields.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "url"
                        | "method"
                        | "headers"
                        | "body"
                        | "response"
                        | "max_bytes"
                        | "timeout_ms"
                        | "credential"
                )
            })
        {
            return Err(invalid("HTTP option fields"));
        }
        let url = fields
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| url.len() <= 4096)
            .ok_or_else(|| invalid("HTTP URL"))?;
        let method = fields.get("method").map_or(Ok("GET"), |value| {
            value.as_str().ok_or_else(|| invalid("HTTP method"))
        })?;
        let response = match fields.get("response").and_then(Value::as_str) {
            Some("bytes") => ResponseKind::Bytes,
            Some("text") => ResponseKind::Text,
            Some("json") => ResponseKind::Json,
            _ => return Err(invalid("HTTP response kind")),
        };
        let max_bytes = fields
            .get("max_bytes")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| *v > 0 && *v <= 32_000_000)
            .ok_or_else(|| invalid("HTTP response bound"))?;
        let timeout_ms = fields
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .filter(|v| (1..=30000).contains(v))
            .ok_or_else(|| invalid("HTTP timeout"))?;
        // Keep the receiver's ORIGINAL constrained service limits. No cap growth.
        let maximum = if response == ResponseKind::Bytes {
            limits.pending_bytes.saturating_sub(32 * 1024)
        } else {
            limits.json_bytes.saturating_sub(32 * 1024)
        };
        if max_bytes > maximum {
            return Err(AnimationError::Budget(
                "HTTP response exceeds original service envelope".into(),
            ));
        }
        let credential = fields
            .get("credential")
            .map(|value| {
                value
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control))
                    .ok_or_else(|| invalid("HTTP credential handle"))
            })
            .transpose()?;
        let body = fields.get("body");
        let mut binary_name = None;
        let body_bytes = match body {
            None => 0,
            Some(Value::String(text))
                if text.len() <= MAX_REQUEST_BODY && request.payload.arrays().is_empty() =>
            {
                text.len()
            }
            Some(Value::Object(marker)) if marker.len() == 1 => {
                let name = marker
                    .get("$ilium_binary")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("HTTP binary marker"))?;
                let arrays = request.payload.arrays();
                if arrays.len() != 1
                    || arrays[0].name != name
                    || arrays[0].kind != TypedArrayKind::U8
                    || arrays[0].elements > MAX_REQUEST_BODY
                    || request.payload.planes().len() != 1
                {
                    return Err(invalid("HTTP binary request inventory"));
                }
                let bytes = request
                    .payload
                    .planes()
                    .get(name)
                    .ok_or_else(|| invalid("HTTP binary plane"))?;
                if bytes.len() != arrays[0].elements {
                    return Err(invalid("HTTP binary request shape"));
                }
                binary_name = Some(name);
                bytes.len()
            }
            _ => return Err(invalid("HTTP request body must be string or U8")),
        };
        if binary_name.is_none() && !request.payload.arrays().is_empty() {
            return Err(invalid("unused HTTP input plane"));
        }
        if body.is_some() && method != "POST" {
            return Err(invalid("HTTP body requires POST"));
        }
        let headers = fields
            .get("headers")
            .map(|value| value.as_object().ok_or_else(|| invalid("HTTP headers")))
            .transpose()?;
        if headers.is_some_and(|h| h.len() > 32) {
            return Err(invalid("HTTP header count"));
        }
        let mut header_bytes = 0usize;
        if let Some(headers) = headers {
            for (name, value) in headers {
                let value = value.as_str().ok_or_else(|| invalid("HTTP header value"))?;
                header_bytes = header_bytes
                    .checked_add(name.len())
                    .and_then(|n| n.checked_add(value.len()))
                    .filter(|n| *n <= 8192)
                    .ok_or_else(|| invalid("HTTP header budget"))?;
            }
        }
        let metadata = quota
            .reserve_external_storage(PREPARATION_BYTES)
            .map_err(|_| AnimationError::Budget("HTTP option metadata admission".into()))?;
        let headers = headers
            .map(|headers| {
                headers
                    .iter()
                    .map(|(key, value)| {
                        value
                            .as_str()
                            .map(|text| (key.clone(), text.to_owned()))
                            .ok_or_else(|| invalid("HTTP header value"))
                    })
                    .collect::<Result<BTreeMap<_, _>>>()
            })
            .transpose()?
            .unwrap_or_default();
        let options = HttpOptions {
            url: url.into(),
            method: method.into(),
            headers,
            body: None,
            response: match response {
                ResponseKind::Bytes => "bytes",
                ResponseKind::Text => "text",
                ResponseKind::Json => "json",
            }
            .into(),
            max_bytes,
            timeout_ms,
            credential: credential.map(str::to_owned),
        };
        options.validate()?; // SAME native origin/method/header/credential policy, before DNS.
        let body = if let Some(name) = binary_name {
            RequestBody::Binary {
                name: name.into(),
                bytes: body_bytes,
            }
        } else if body.is_some() {
            RequestBody::Text
        } else {
            RequestBody::None
        };
        Ok(Self {
            options,
            body,
            body_bytes,
            response,
            _metadata: metadata,
        })
    }
    fn method(&self) -> Result<HttpMethod> {
        match self.options.method.as_str() {
            "GET" => Ok(HttpMethod::Get),
            "HEAD" => Ok(HttpMethod::Head),
            "POST" => Ok(HttpMethod::Post),
            _ => Err(invalid("HTTP method")),
        }
    }
    fn io_cost(&self, wire_bytes: usize) -> Result<JobCost> {
        // Existing transport accepts Value byte arrays: explicitly charge that
        // temporary expansion + body copies rather than calling it zero-copy.
        let input_bytes = self
            .body_bytes
            .checked_mul(size_of::<Value>() + 8)
            .and_then(|v| wire_bytes.checked_mul(2).and_then(|w| v.checked_add(w)))
            .and_then(|v| v.checked_add(TRANSPORT_SCRATCH))
            .ok_or_else(cost_overflow)?;
        let result_bytes = self
            .options
            .max_bytes
            .checked_add(HEADER_RETENTION)
            .ok_or_else(cost_overflow)?;
        Ok(JobCost {
            input_bytes,
            result_bytes,
        })
    }
    fn native_body(&self, request: &HostRequest) -> Result<Option<Value>> {
        match &self.body {
            RequestBody::None => Ok(None),
            RequestBody::Text => request
                .payload
                .metadata()
                .get("body")
                .and_then(Value::as_str)
                .map(|s| Some(Value::String(s.to_owned())))
                .ok_or_else(|| invalid("HTTP retained text body")),
            RequestBody::Binary { name, bytes } => {
                let data = request
                    .payload
                    .planes()
                    .get(name)
                    .filter(|data| data.len() == *bytes)
                    .ok_or_else(|| invalid("HTTP retained binary shape"))?;
                let mut values = Vec::new();
                values.try_reserve_exact(*bytes).map_err(|_| {
                    AnimationError::Budget("HTTP bounded body expansion allocation".into())
                })?;
                values.extend(data.iter().map(|byte| Value::from(*byte)));
                Ok(Some(Value::Array(values)))
            }
        }
    }
}
struct PreparationBudget {
    max_requests: Option<u64>,
    max_bytes: Option<u64>,
    attempted: u64,
    reserved_bytes: u64,
}
impl PreparationBudget {
    fn from_plan(plan: &crate::plan::AnimationPlan) -> Result<Self> {
        let preparation = plan
            .preparation
            .as_ref()
            .map(|value| {
                value
                    .as_object()
                    .ok_or_else(|| invalid("native preparation record"))
            })
            .transpose()?;
        let http = preparation.and_then(|value| value.get("http"));
        let field = |name: &str| -> Result<Option<u64>> {
            http.map(|value| {
                value
                    .as_object()
                    .ok_or_else(|| invalid("HTTP native preparation budget"))
            })
            .transpose()?
            .and_then(|value| value.get(name))
            .map(|value| {
                value
                    .as_u64()
                    .filter(|v| *v > 0 && *v <= 256 * 1024 * 1024)
                    .ok_or_else(|| invalid("HTTP native preparation bound"))
            })
            .transpose()
        };
        Ok(Self {
            max_requests: field("max_requests")?,
            max_bytes: field("max_bytes")?,
            attempted: 0,
            reserved_bytes: 0,
        })
    }
    fn next(
        &self,
        phase: crate::engine::ServicePhase,
        maximum: usize,
    ) -> Result<Option<(u64, u64)>> {
        if phase != crate::engine::ServicePhase::Create {
            return Ok(None);
        }
        let requests = self.attempted.checked_add(1).ok_or_else(cost_overflow)?;
        let bytes = self
            .reserved_bytes
            .checked_add(u64::try_from(maximum).map_err(|_| cost_overflow())?)
            .ok_or_else(cost_overflow)?;
        if self.max_requests.is_some_and(|limit| requests > limit)
            || self.max_bytes.is_some_and(|limit| bytes > limit)
        {
            return Err(AnimationError::Budget(
                "declared HTTP preparation budget".into(),
            ));
        }
        Ok(Some((requests, bytes))) // Reserve maximum response body, not an invented physical-byte observation.
    }
    fn record(&mut self, next: Option<(u64, u64)>) {
        if let Some((requests, bytes)) = next {
            self.attempted = requests;
            self.reserved_bytes = bytes;
        }
    }
}

struct NativeDns(Arc<dyn DnsResolver + Send + Sync>);
impl DnsResolver for NativeDns {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<std::net::SocketAddr>> {
        self.0.resolve(host, port)
    }
}
struct RawResponse {
    head: HttpResponseHead,
    body: SourceHttpResponse,
}
struct HttpIoJob {
    prepared: PreparedRequest,
    request: HostRequest,
    factory: NativeHttpAuthorityFactory,
    client: Client,
    dns: NativeDns,
    quota: QuotaGroup,
}
impl Job for HttpIoJob {
    type Output = RawResponse;
    type Error = AnimationError;
    fn run(mut self, context: JobContext) -> Result<RawResponse> {
        let mut authority: NativeHttpAuthority = self.factory.enter(&context, &self.client)?;
        let remaining = self.request.remaining_ms();
        if remaining == 0 || context.stop_requested() || self.request.is_cancelled() {
            return Err(permission("HTTP original deadline/cancellation"));
        }
        self.prepared.options.timeout_ms = self.prepared.options.timeout_ms.min(remaining);
        self.prepared.options.body = self.prepared.native_body(&self.request)?; // Actual admitted IO input cost, not queue-time eager expansion.
        let stop = AtomicBool::new(false); // Original request/job cancellation is additionally checked by authority on every native phase/read.
        let raw = http::request_with_reader(
            &self.prepared.options,
            &mut authority,
            &self.dns,
            &stop,
            |head, reader| {
                let body = SourceHttpResponse::read(
                    head.status,
                    reader,
                    &self.quota,
                    self.prepared.options.max_bytes,
                    &stop,
                )?;
                Ok(RawResponse { head, body })
            },
        )?;
        authority.with_body_delivery(|| Ok(raw)) // Guarded bounded native move only, no JSON/text/image/CPU decode in IO body.
    }
}
struct HttpDecodeJob {
    raw: Retained<RawResponse>, // Original IO input/output reservation survives this move.
    request: HostRequest,
    kind: ResponseKind,
    limits: EngineLimits,
    quota: QuotaGroup,
}
impl Job for HttpDecodeJob {
    type Output = ServiceValue;
    type Error = AnimationError;
    fn run(self, context: JobContext) -> Result<ServiceValue> {
        if context.stop_requested() || self.request.is_cancelled() {
            return Err(permission("HTTP decode cancelled"));
        }
        let output = decode_response(self.raw.view(), self.kind, &self.limits, &self.quota)?;
        if context.stop_requested() || self.request.is_cancelled() {
            return Err(permission("HTTP decoded delivery cancelled"));
        }
        Ok(output)
    }
}
fn decode_cost(raw: &RawResponse, kind: ResponseKind) -> Result<JobCost> {
    let multiplier = match kind {
        ResponseKind::Bytes => 2,
        ResponseKind::Text => 8,
        ResponseKind::Json => 32,
    };
    let input_bytes = raw
        .body
        .as_bytes()
        .len()
        .checked_mul(multiplier)
        .and_then(|n| n.checked_add(DECODE_SCRATCH))
        .ok_or_else(cost_overflow)?;
    let result_bytes = raw
        .body
        .as_bytes()
        .len()
        .checked_add(64 * 1024)
        .ok_or_else(cost_overflow)?;
    Ok(JobCost {
        input_bytes,
        result_bytes,
    })
}
/// Lexical byte/node/depth/token preflight BEFORE allocating a serde Value tree.
/// Syntax and duplicate-key behavior still belong to actual serde_json parser.
fn preflight_json(bytes: &[u8]) -> Result<()> {
    let (mut i, mut nodes, mut depth) = (0usize, 0usize, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\r' | b'\n' | b'\t' | b':' | b',' => {
                i += 1;
                continue;
            }
            b'{' | b'[' => {
                depth += 1;
                nodes += 1;
                i += 1;
                if depth > 24 {
                    return Err(AnimationError::Budget("HTTP JSON depth".into()));
                }
            }
            b'}' | b']' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("HTTP JSON shape"))?;
                i += 1;
            }
            b'"' => {
                nodes += 1;
                i += 1;
                let mut closed = false;
                while i < bytes.len() {
                    if bytes[i] == b'"' {
                        i += 1;
                        closed = true;
                        break;
                    }
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if !closed {
                    return Err(invalid("HTTP JSON string"));
                }
            }
            _ => {
                nodes += 1;
                let start = i;
                while i < bytes.len()
                    && !matches!(
                        bytes[i],
                        b' ' | b'\r'
                            | b'\n'
                            | b'\t'
                            | b':'
                            | b','
                            | b'{'
                            | b'}'
                            | b'['
                            | b']'
                            | b'"'
                    )
                {
                    i += 1;
                }
                if i - start > 128 {
                    return Err(AnimationError::Budget("HTTP JSON scalar token".into()));
                }
            }
        }
        if nodes > 3500 {
            return Err(AnimationError::Budget("HTTP JSON nodes".into()));
        }
    }
    if depth != 0 {
        return Err(invalid("HTTP JSON unclosed container"));
    }
    Ok(())
}
fn decode_response(
    raw: &RawResponse,
    kind: ResponseKind,
    limits: &EngineLimits,
    quota: &QuotaGroup,
) -> Result<ServiceValue> {
    if !(100..=599).contains(&raw.head.status)
        || raw.head.headers.len() > 64
        || raw.head.final_url.len() > 4096
    {
        return Err(invalid("HTTP native response metadata"));
    }
    let mut header_bytes = 0usize;
    for (name, value) in &raw.head.headers {
        header_bytes = header_bytes
            .checked_add(name.len())
            .and_then(|n| n.checked_add(value.len()))
            .filter(|n| *n <= 8192)
            .ok_or_else(|| invalid("HTTP response header bound"))?;
    }
    let mut planes = BTreeMap::new();
    let mut arrays = Vec::new();
    let body = match kind {
        ResponseKind::Bytes => {
            let bytes = raw.body.as_bytes();
            let mut copied = Vec::new();
            copied
                .try_reserve_exact(bytes.len())
                .map_err(|_| AnimationError::Budget("HTTP binary projection allocation".into()))?;
            copied.extend_from_slice(bytes);
            planes.insert("b0".into(), copied);
            arrays.push(ArraySpec {
                name: "b0".into(),
                kind: TypedArrayKind::U8,
                elements: bytes.len(),
            });
            json!({"$ilium_binary":"b0"})
        }
        ResponseKind::Text => Value::String(
            std::str::from_utf8(raw.body.as_bytes())
                .map_err(|_| invalid("HTTP response UTF-8"))?
                .to_owned(),
        ),
        ResponseKind::Json => {
            preflight_json(raw.body.as_bytes())?;
            serde_json::from_slice(raw.body.as_bytes())?
        }
    };
    let value = json!({"ok":true,"value":{"status":raw.head.status,"headers":raw.head.headers,"body":body,"final_url":raw.head.final_url}});
    ServiceValue::copy_from_host(&value, &arrays, &planes, limits, quota.clone())
}
enum Stage {
    Io(Receipt<HttpIoJob>),
    Cpu(Receipt<HttpDecodeJob>),
    LostIo(Receipt<HttpIoJob>),
    LostCpu(Receipt<HttpDecodeJob>),
    Cleanup { _retention: Retention },
    UnissuedCleanup { _job: Option<Box<HttpIoJob>> },
}
struct Pending {
    operation: ServiceOperation,
    kind: ResponseKind,
    stage: Stage,
}
#[derive(Debug)]
pub enum HttpObservation {
    Completion(CompletionState),
    Refused(&'static str),
    Lost,
    CleanupFailed,
}
#[derive(Debug)]
pub struct HttpEvent {
    pub request_id: u64,
    pub observation: HttpObservation,
}
/// Native embedding supplies the EXISTING finite Client configured with its
/// ORIGINAL completion wake (bounded hint to its existing event channel).
/// Own one host per original PackageInstance; retain it while native jobs run,
/// even when helper is closed. No callback here synchronously waits for a job.
pub struct NativeHttpHost {
    client: Client,
    quota: QuotaGroup,
    owner: Arc<Mutex<PermissionBroker>>,
    limits: EngineLimits,
    dns: Arc<dyn DnsResolver + Send + Sync>,
    credentials: Option<Arc<Mutex<Box<dyn HostCredentialAdapter>>>>,
    pending: BTreeMap<(u64, u64, u64, u64), Pending>,
    closed: bool,
    preparation: PreparationBudget,
    _metadata: StorageAdmission,
}
impl NativeHttpHost {
    pub fn new(
        instance: &PackageInstance,
        client: Client,
        dns: Arc<dyn DnsResolver + Send + Sync>,
        credentials: Option<Arc<Mutex<Box<dyn HostCredentialAdapter>>>>,
    ) -> Result<Self> {
        let quota = client.quota_group();
        let owner = instance.native_http_owner(&quota)?;
        let metadata = quota
            .reserve_external_storage(REGISTRY_BYTES)
            .map_err(|_| AnimationError::Budget("HTTP registry admission".into()))?;
        Ok(Self {
            client,
            quota,
            owner,
            limits: instance.engine_limits().clone(),
            dns,
            credentials,
            pending: BTreeMap::new(),
            closed: false,
            preparation: PreparationBudget::from_plan(instance.plan())?,
            _metadata: metadata,
        })
    }
    fn refusal(
        &self,
        instance: &mut PackageInstance,
        request: &HostRequest,
        code: &'static str,
    ) -> Result<CompletionState> {
        instance.complete_http_refusal(request, failure_value(code, &self.quota, &self.limits)?)
    }
    /// Return unrelated requests intact. Recognized calls produce a structured
    /// SDK Result refusal or retain their REAL operation/receipt for a later wake.
    pub fn dispatch(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
    ) -> Result<Option<HostRequest>> {
        if request.method != "http.request" {
            return Ok(Some(request));
        }
        instance.check_http_owner(&self.owner, &self.quota)?;
        let key = (
            request.id,
            request.authority.instance_id,
            request.authority.plan_generation,
            request.authority.authorization_epoch,
        );
        if self.pending.contains_key(&key) {
            return Err(invalid("duplicate native HTTP request capsule"));
        }
        if self.closed || self.pending.len() >= MAX_PENDING.min(self.limits.pending_requests) {
            self.refusal(instance, &request, "http_busy")?;
            return Ok(None);
        }
        let prepared = match PreparedRequest::parse(&request, &self.limits, &self.quota) {
            Ok(v) => v,
            Err(e) => {
                self.refusal(instance, &request, error_code(&e))?;
                return Ok(None);
            }
        };
        let cost = match prepared.io_cost(request.payload.wire_bytes()) {
            Ok(cost) => cost,
            Err(error) => {
                self.refusal(instance, &request, error_code(&error))?;
                return Ok(None);
            }
        };
        let preparation = match self
            .preparation
            .next(request.phase, prepared.options.max_bytes)
        {
            Ok(next) => next,
            Err(error) => {
                self.refusal(instance, &request, error_code(&error))?;
                return Ok(None);
            }
        };
        let need = match prepared.method().and_then(|method| {
            OperationNeed::http_preflight(&prepared.options.url, method)
                .map_err(|_| permission("HTTP endpoint scope"))
        }) {
            Ok(need) => need,
            Err(error) => {
                self.refusal(instance, &request, error_code(&error))?;
                return Ok(None);
            }
        };
        let operation = match instance.dispatch_http_service(request.clone(), need) {
            Ok(v) => v,
            Err(e) => {
                self.refusal(instance, &request, error_code(&e))?;
                return Ok(None);
            }
        };
        let provider = self.credentials.as_ref().map(|provider| {
            Box::new(SharedCredentialAdapter(Arc::clone(provider)))
                as Box<dyn HostCredentialAdapter>
        });
        let factory = match instance.http_authority_factory(&operation, provider) {
            Ok(v) => v,
            Err(e) => {
                if let Err(settle) = instance.settle_http_terminal(&operation) {
                    self.pending.insert(
                        key,
                        Pending {
                            operation,
                            kind: prepared.response,
                            stage: Stage::UnissuedCleanup { _job: None },
                        },
                    );
                    return Err(settle);
                }
                self.refusal(instance, &request, error_code(&e))?;
                return Ok(None);
            }
        };
        let kind = prepared.response;
        let job = HttpIoJob {
            prepared,
            request: request.clone(),
            factory,
            client: self.client.clone(),
            dns: NativeDns(Arc::clone(&self.dns)),
            quota: self.quota.clone(),
        };
        let client = self.client.clone();
        let budget = &mut self.preparation;
        let submitted = instance.commit_service(&operation, || {
            budget.record(preparation); // Only inside actual original bounded IO issue, never on denied grants.
            client.try_submit(Lane::Io, cost, job).map_err(Box::new)
        });
        match submitted {
            Ok(Ok(receipt)) => {
                self.pending.insert(
                    key,
                    Pending {
                        operation,
                        kind,
                        stage: Stage::Io(receipt),
                    },
                );
            }
            Ok(Err(rejected)) => {
                // Rejected carries actual unstarted input; keep it alive through refusal ACK.
                let output = failure_value("http_admission_refused", &self.quota, &self.limits);
                let delivered =
                    output.and_then(|value| instance.complete_authorized(&operation, value));
                if let Err(settle) = instance.settle_http_terminal(&operation) {
                    self.pending.insert(
                        key,
                        Pending {
                            operation,
                            kind,
                            stage: Stage::UnissuedCleanup {
                                _job: Some(Box::new(rejected.value)),
                            },
                        },
                    );
                    return Err(settle);
                }
                drop(rejected);
                delivered?;
            }
            Err(error) => {
                if let Err(settle) = instance.settle_http_terminal(&operation) {
                    self.pending.insert(
                        key,
                        Pending {
                            operation,
                            kind,
                            stage: Stage::UnissuedCleanup { _job: None },
                        },
                    );
                    return Err(settle);
                }
                self.refusal(instance, &request, error_code(&error))?;
            }
        }
        Ok(None)
    }
    fn finish(
        &self,
        instance: &mut PackageInstance,
        operation: &ServiceOperation,
        output: Result<ServiceValue>,
        retention: Retention,
    ) -> (HttpEvent, Option<Retention>) {
        let id = operation.request().id;
        let output = match output {
            Ok(value) => Ok(value),
            Err(error) => failure_value(error_code(&error), &self.quota, &self.limits),
        };
        let result = output.and_then(|value| instance.complete_authorized(operation, value));
        // Actual terminal outcome held by caller. deliver may already have
        // removed genuine ticket before a failed helper ACK; terminal settlement
        // accepts ONLY that same issuer's already-absent operation.
        match instance.settle_http_terminal(operation) {
            Ok(()) => {
                drop(retention);
                (
                    HttpEvent {
                        request_id: id,
                        observation: match result {
                            Ok(state) => HttpObservation::Completion(state),
                            Err(_) => HttpObservation::Refused("http_delivery_withheld"),
                        },
                    },
                    None,
                )
            }
            Err(_) => (
                HttpEvent {
                    request_id: id,
                    observation: HttpObservation::CleanupFailed,
                },
                Some(retention),
            ),
        }
    }
    /// Invoke ONLY from the original Client completion wake/event, never a
    /// timer loop. Pending is retained; no callback/body/thread wait occurs here.
    pub fn on_completion_wake(&mut self, instance: &mut PackageInstance) -> Result<Vec<HttpEvent>> {
        instance.check_http_owner(&self.owner, &self.quota)?;
        let ids: Vec<_> = self.pending.keys().copied().collect();
        let mut events = Vec::new();
        for id in ids {
            let Some(mut entry) = self.pending.remove(&id) else {
                continue;
            };
            let mut stage = entry.stage;
            match &mut stage {
                Stage::Io(receipt) => {
                    let fallback_hold = receipt.retention();
                    match receipt.try_take() {
                        JobPoll::Pending => {
                            entry.stage = stage;
                            self.pending.insert(id, entry);
                        }
                        JobPoll::Lost | JobPoll::Taken => {
                            entry.stage = match stage {
                                Stage::Io(receipt) => Stage::LostIo(receipt),
                                other => other,
                            };
                            self.pending.insert(id, entry);
                            events.push(HttpEvent {
                                request_id: id.0,
                                observation: HttpObservation::Lost,
                            });
                        }
                        JobPoll::Ready(outcome) => {
                            let (outcome, retention) = outcome.into_parts();
                            match outcome {
                                JobOutcome::Finished(Ok(raw)) => {
                                    let cost = decode_cost(&raw, entry.kind);
                                    let job = HttpDecodeJob {
                                        raw: retention.retain(raw),
                                        request: entry.operation.request().clone(),
                                        kind: entry.kind,
                                        limits: self.limits.clone(),
                                        quota: self.quota.clone(),
                                    };
                                    let submitted = cost.and_then(|cost| {
                                        instance.with_http_operation_authority(
                                            &entry.operation,
                                            || {
                                                self.client
                                                    .try_submit(Lane::Cpu, cost, job)
                                                    .map_err(Box::new)
                                            },
                                        )
                                    });
                                    match submitted {
                                        Ok(Ok(receipt)) => {
                                            entry.stage = Stage::Cpu(receipt);
                                            self.pending.insert(id, entry);
                                        }
                                        Ok(Err(rejected)) => {
                                            let hold = rejected.value.raw.into_parts().1;
                                            let (event, cleanup) = self.finish(
                                                instance,
                                                &entry.operation,
                                                Err(AnimationError::Budget(
                                                    "HTTP decode admission refused".into(),
                                                )),
                                                hold,
                                            );
                                            events.push(event);
                                            if let Some(hold) = cleanup {
                                                entry.stage = Stage::Cleanup { _retention: hold };
                                                self.pending.insert(id, entry);
                                            }
                                        }
                                        Err(_) => {
                                            // Original IO body is terminal; CPU was NOT submitted.
                                            let output = failure_value(
                                                "http_decode_withheld",
                                                &self.quota,
                                                &self.limits,
                                            );
                                            let result = output.and_then(|value| {
                                                instance
                                                    .complete_authorized(&entry.operation, value)
                                            });
                                            let settle =
                                                instance.settle_http_terminal(&entry.operation);
                                            events.push(HttpEvent {
                                                request_id: id.0,
                                                observation: if settle.is_err() {
                                                    HttpObservation::CleanupFailed
                                                } else if let Ok(state) = result {
                                                    HttpObservation::Completion(state)
                                                } else {
                                                    HttpObservation::Refused("http_decode_withheld")
                                                },
                                            });
                                            // job/IO retention was dropped by failed submission closure; no native body remains. Keep original operation on cleanup error.
                                            if settle.is_err() {
                                                entry.stage = Stage::Cleanup {
                                                    _retention: fallback_hold,
                                                };
                                                self.pending.insert(id, entry);
                                            }
                                        }
                                    }
                                }
                                JobOutcome::Finished(Err(error)) => {
                                    let (event, cleanup) = self.finish(
                                        instance,
                                        &entry.operation,
                                        Err(error),
                                        retention,
                                    );
                                    events.push(event);
                                    if let Some(hold) = cleanup {
                                        entry.stage = Stage::Cleanup { _retention: hold };
                                        self.pending.insert(id, entry);
                                    }
                                }
                                JobOutcome::NotStarted { job, .. } => {
                                    let hold = retention.retain(job);
                                    let event = self.finish(
                                        instance,
                                        &entry.operation,
                                        failure_value(
                                            "http_not_started",
                                            &self.quota,
                                            &self.limits,
                                        ),
                                        hold.into_parts().1,
                                    );
                                    events.push(event.0);
                                    if let Some(hold) = event.1 {
                                        entry.stage = Stage::Cleanup { _retention: hold };
                                        self.pending.insert(id, entry);
                                    }
                                }
                                JobOutcome::Panicked => {
                                    let (event, cleanup) = self.finish(
                                        instance,
                                        &entry.operation,
                                        failure_value(
                                            "http_effects_unknown",
                                            &self.quota,
                                            &self.limits,
                                        ),
                                        retention,
                                    );
                                    events.push(event);
                                    if let Some(hold) = cleanup {
                                        entry.stage = Stage::Cleanup { _retention: hold };
                                        self.pending.insert(id, entry);
                                    }
                                }
                            }
                        }
                    }
                }
                Stage::Cpu(receipt) => match receipt.try_take() {
                    JobPoll::Pending => {
                        entry.stage = stage;
                        self.pending.insert(id, entry);
                    }
                    JobPoll::Lost | JobPoll::Taken => {
                        entry.stage = match stage {
                            Stage::Cpu(receipt) => Stage::LostCpu(receipt),
                            other => other,
                        };
                        self.pending.insert(id, entry);
                        events.push(HttpEvent {
                            request_id: id.0,
                            observation: HttpObservation::Lost,
                        });
                    }
                    JobPoll::Ready(outcome) => {
                        let (outcome, retention) = outcome.into_parts();
                        let output = match outcome {
                            JobOutcome::Finished(value) => value,
                            JobOutcome::NotStarted { job, .. } => {
                                drop(job);
                                failure_value("http_decode_not_started", &self.quota, &self.limits)
                            }
                            JobOutcome::Panicked => {
                                failure_value("http_decode_panicked", &self.quota, &self.limits)
                            }
                        };
                        let (event, cleanup) =
                            self.finish(instance, &entry.operation, output, retention);
                        events.push(event);
                        if let Some(hold) = cleanup {
                            entry.stage = Stage::Cleanup { _retention: hold };
                            self.pending.insert(id, entry);
                        }
                    }
                },
                Stage::LostIo(_)
                | Stage::LostCpu(_)
                | Stage::Cleanup { .. }
                | Stage::UnissuedCleanup { .. } => {
                    entry.stage = stage;
                    self.pending.insert(id, entry);
                } // Never re-poll a Lost/Taken receipt or pretend it proves physical exit.
            }
        }
        Ok(events)
    }
    pub fn cancel_all(&mut self) {
        self.closed = true;
        for entry in self.pending.values() {
            entry.operation.request().stop_token().stop();
            match &entry.stage {
                Stage::Io(r) | Stage::LostIo(r) => r.cancel(),
                Stage::Cpu(r) | Stage::LostCpu(r) => r.cancel(),
                Stage::Cleanup { .. } | Stage::UnissuedCleanup { .. } => {}
            }
        }
    }
    pub fn is_drained(&self) -> bool {
        self.pending.is_empty()
    } // Lost/cleanup entries remain explicitly NOT drained.
}
impl Drop for NativeHttpHost {
    fn drop(&mut self) {
        self.cancel_all();
    } // Body/queued outcome own independent original bank guards. Drop is abandonment, NOT terminal settlement/join proof.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{ServiceAuthority, ServiceBudget, ServicePhase};
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaLimits, ShutdownMode,
    };
    use std::{
        io::Cursor,
        time::{Duration, Instant},
    };
    fn quota() -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 4,
            service_jobs: 0,
            input_bytes: 32 * 1024 * 1024,
            result_bytes: 8 * 1024 * 1024,
            worker_threads: 4,
            worker_bytes: 32 * 1024 * 1024,
        })
    }
    fn request(
        metadata: Value,
        arrays: Vec<ArraySpec>,
        planes: BTreeMap<String, Vec<u8>>,
        quota: &QuotaGroup,
    ) -> HostRequest {
        let limits = EngineLimits::default();
        let payload = ServiceValue::copy_request_from_host(
            &metadata,
            &arrays,
            &planes,
            &limits,
            quota.clone(),
            &ServiceBudget::new(&limits),
        )
        .unwrap();
        HostRequest::from_transport(
            1,
            "http.request".into(),
            30000,
            "f".repeat(64),
            ServiceAuthority {
                instance_id: 1,
                plan_generation: 1,
                authorization_epoch: 1,
            },
            ServicePhase::Async,
            payload,
        )
        .unwrap()
    }
    fn options() -> Value {
        json!({"url":"https://example.org/data","method":"POST","response":"bytes","max_bytes":4096,"timeout_ms":1000})
    }
    #[test]
    fn exact_sdk_binary_and_text_bodies_are_retained_and_bounded() {
        let quota = quota();
        let _fixture = quota.reserve_external_storage(65536).unwrap();
        let mut metadata = options();
        metadata["body"] = json!({"$ilium_binary":"b0"});
        let request = request(
            metadata,
            vec![ArraySpec {
                name: "b0".into(),
                kind: TypedArrayKind::U8,
                elements: 3,
            }],
            BTreeMap::from([("b0".into(), vec![0, 255, 128])]),
            &quota,
        );
        let prepared = PreparedRequest::parse(&request, &EngineLimits::default(), &quota).unwrap();
        let _staging = quota
            .reserve_external_storage(
                prepared
                    .io_cost(request.payload.wire_bytes())
                    .unwrap()
                    .input_bytes,
            )
            .unwrap();
        assert_eq!(
            prepared.native_body(&request).unwrap(),
            Some(json!([0, 255, 128]))
        );
        assert!(
            PreparedRequest::parse(&request, &EngineLimits::default(), &super::tests::quota())
                .is_err()
        ); // Equal limits, foreign original root.
        let mut metadata = options();
        metadata["body"] = Value::String("π\nbody".into());
        let text = super::tests::request(metadata, vec![], BTreeMap::new(), &quota);
        let prepared = PreparedRequest::parse(&text, &EngineLimits::default(), &quota).unwrap();
        assert_eq!(
            prepared.native_body(&text).unwrap(),
            Some(Value::String("π\nbody".into()))
        );
    }
    #[test]
    fn non_u8_unused_plane_body_method_and_shape_are_refused_before_transport() {
        let quota = quota();
        let _fixture = quota.reserve_external_storage(65536).unwrap();
        let mut metadata = options();
        metadata["body"] = json!({"$ilium_binary":"b0"});
        let float = request(
            metadata.clone(),
            vec![ArraySpec {
                name: "b0".into(),
                kind: TypedArrayKind::F32,
                elements: 1,
            }],
            BTreeMap::from([("b0".into(), 1_f32.to_ne_bytes().to_vec())]),
            &quota,
        );
        assert!(PreparedRequest::parse(&float, &EngineLimits::default(), &quota).is_err());
        metadata["method"] = Value::String("GET".into());
        let with_body = request(
            metadata,
            vec![ArraySpec {
                name: "b0".into(),
                kind: TypedArrayKind::U8,
                elements: 1,
            }],
            BTreeMap::from([("b0".into(), vec![1])]),
            &quota,
        );
        assert!(PreparedRequest::parse(&with_body, &EngineLimits::default(), &quota).is_err());
        let before_unused = quota.snapshot().worker_bytes;
        // The genuine service transport rejects an unreferenced plane before
        // a HostRequest can exist; do not bypass it to reach HTTP parsing.
        let limits = EngineLimits::default();
        let unused = ServiceValue::copy_request_from_host(
            &options(),
            &[ArraySpec {
                name: "b0".into(),
                kind: TypedArrayKind::U8,
                elements: 1,
            }],
            &BTreeMap::from([("b0".into(), vec![1])]),
            &limits,
            quota.clone(),
            &ServiceBudget::new(&limits),
        );
        assert!(
            matches!(unused, Err(AnimationError::Runtime(message)) if message == "unreferenced service binary plane")
        );
        assert_eq!(quota.snapshot().worker_bytes, before_unused);
        let mut metadata = options();
        metadata["guest_demand_id"] = Value::String("allowed_other_right".into());
        let injected = request(metadata, vec![], BTreeMap::new(), &quota);
        assert!(PreparedRequest::parse(&injected, &EngineLimits::default(), &quota).is_err());
    }
    #[test]
    fn response_cap_is_original_instance_limit_not_a_new_default() {
        let quota = quota();
        let _fixture = quota.reserve_external_storage(65536).unwrap();
        let mut metadata = options();
        metadata["max_bytes"] = json!(256 * 1024);
        metadata["response"] = json!("json");
        let request = request(metadata, vec![], BTreeMap::new(), &quota);
        let limits = EngineLimits {
            json_bytes: 64 * 1024,
            ..EngineLimits::default()
        };
        assert!(PreparedRequest::parse(&request, &limits, &quota).is_err());
    }
    #[test]
    fn json_preflight_refuses_depth_nodes_long_atoms_before_value_tree() {
        let quota = quota();
        let _fixture = quota.reserve_external_storage(1024 * 1024).unwrap();
        assert!(preflight_json(br#"{"literal":"{[}]","escaped":"\\\"","n":1}"#).is_ok());
        let deep = format!("{}0{}", "[".repeat(25), "]".repeat(25));
        assert!(preflight_json(deep.as_bytes()).is_err());
        let nodes = format!("[{}]", vec!["0"; 3501].join(","));
        assert!(preflight_json(nodes.as_bytes()).is_err());
        assert!(preflight_json("1".repeat(129).as_bytes()).is_err());
        assert!(preflight_json(br#"{"unterminated":"x"#).is_err());
    }
    fn raw(bytes: &[u8], quota: &QuotaGroup) -> RawResponse {
        // Task-local synthetic network result; no actual HTTP/DNS is performed.
        let body = SourceHttpResponse::read(
            200,
            &mut Cursor::new(bytes),
            quota,
            4096,
            &AtomicBool::new(false),
        )
        .unwrap();
        RawResponse {
            head: HttpResponseHead {
                status: 200,
                headers: BTreeMap::from([("content-type".into(), "fixture".into())]),
                final_url: "https://example.org/fixture".into(),
            },
            body,
        }
    }
    #[test]
    fn response_projection_preserves_u8_text_json_and_forbids_marker_forgery() {
        let quota = quota();
        let _decode = quota.reserve_external_storage(4 * 1024 * 1024).unwrap();
        let limits = EngineLimits::default();
        let bytes = decode_response(
            &raw(&[0, 255, 128], &quota),
            ResponseKind::Bytes,
            &limits,
            &quota,
        )
        .unwrap();
        assert_eq!(bytes.arrays().len(), 1);
        assert_eq!(bytes.arrays()[0].kind, TypedArrayKind::U8);
        assert_eq!(bytes.planes()["b0"], vec![0, 255, 128]);
        let text = decode_response(
            &raw("π".as_bytes(), &quota),
            ResponseKind::Text,
            &limits,
            &quota,
        )
        .unwrap();
        assert_eq!(text.metadata()["value"]["body"], json!("π"));
        let value = decode_response(
            &raw(br#"{"x":[1,true,null]}"#, &quota),
            ResponseKind::Json,
            &limits,
            &quota,
        )
        .unwrap();
        assert_eq!(
            value.metadata()["value"]["body"],
            json!({"x":[1,true,null]})
        );
        assert!(decode_response(
            &raw(br#"{"$ilium_binary":"b0"}"#, &quota),
            ResponseKind::Json,
            &limits,
            &quota
        )
        .is_err());
        assert!(
            decode_response(&raw(&[255], &quota), ResponseKind::Text, &limits, &quota).is_err()
        );
    }
    #[test]
    fn costs_cover_binary_expansion_and_do_not_charge_bytes_as_json() {
        let quota = quota();
        let _fixture = quota.reserve_external_storage(65536).unwrap();
        let raw = raw(b"fixture", &quota);
        let bytes = decode_cost(&raw, ResponseKind::Bytes).unwrap();
        let json = decode_cost(&raw, ResponseKind::Json).unwrap();
        assert!(json.input_bytes > bytes.input_bytes);
        let mut metadata = options();
        metadata["body"] = json!({"$ilium_binary":"b0"});
        let request = request(
            metadata,
            vec![ArraySpec {
                name: "b0".into(),
                kind: TypedArrayKind::U8,
                elements: 3,
            }],
            BTreeMap::from([("b0".into(), vec![0, 255, 128])]),
            &quota,
        );
        let prepared = PreparedRequest::parse(&request, &EngineLimits::default(), &quota).unwrap();
        assert!(
            prepared
                .io_cost(request.payload.wire_bytes())
                .unwrap()
                .input_bytes
                >= TRANSPORT_SCRATCH + 3 * size_of::<Value>()
        );
    }
    #[test]
    fn preparation_reservation_is_create_only_and_debits_only_actual_issue_attempts() {
        let mut budget = PreparationBudget {
            max_requests: Some(2),
            max_bytes: Some(8192),
            attempted: 0,
            reserved_bytes: 0,
        };
        let first = budget.next(ServicePhase::Create, 4096).unwrap();
        assert_eq!(first, Some((1, 4096)));
        assert_eq!(budget.attempted, 0); // A refused grant before issue does not consume this budget.
        assert_eq!(budget.reserved_bytes, 0);
        budget.record(first); // Called inside the genuine original try_submit boundary only.
        assert_eq!(budget.next(ServicePhase::Async, usize::MAX).unwrap(), None);
        assert!(budget.next(ServicePhase::Create, 4097).is_err());
        let second = budget.next(ServicePhase::Create, 4096).unwrap();
        budget.record(second);
        assert_eq!(budget.attempted, 2);
        assert_eq!(budget.reserved_bytes, 8192);
        assert!(budget.next(ServicePhase::Create, 1).is_err());
        assert_eq!(budget.next(ServicePhase::Async, 1).unwrap(), None);
    }

    #[test]
    fn actual_http_io_uses_original_committed_ticket_and_dns_refusal_is_not_success() {
        use crate::permissions::{
            CallPhase, Capability, Ceiling, Demand, PackageIdentity, PermissionPlan,
            PermissionRequest, Right, Scope, UserChoice,
        };
        use std::collections::BTreeSet;
        struct RefusingDns;
        impl DnsResolver for RefusingDns {
            fn resolve(&self, _: &str, _: u16) -> Result<Vec<std::net::SocketAddr>> {
                Err(invalid("task-local native DNS refusal"))
            }
        }
        let quota = quota();
        let _fixture = quota.reserve_external_storage(1024 * 1024).unwrap();
        let worker = LaneConfig {
            threads: 1,
            queue_slots: 2,
            priority: None,
            resident_bytes_per_thread: 1024,
        };
        let mut execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
                io: worker,
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
                input_bytes: 16 * 1024 * 1024,
                result_bytes: 4 * 1024 * 1024,
            })
            .unwrap();
        let right = Right {
            id: Capability::NetworkHttp,
            scope: Scope::Network {
                origins: BTreeSet::from(["https://example.org".into()]),
                methods: BTreeSet::from([HttpMethod::Post]),
            },
        };
        let ceiling = Ceiling {
            permissions: vec![right.clone()],
        };
        let mut native = PermissionBroker::new(
            PackageIdentity::unverified(
                "http_real_io_refusal_fixture".into(),
                b"synthetic native identity",
            )
            .unwrap(),
            ceiling.clone(),
            ceiling,
        )
        .unwrap();
        let review = native
            .prepare(
                1,
                1,
                PermissionPlan {
                    permissions: vec![PermissionRequest {
                        request_id: Some("http".into()),
                        id: right.id,
                        scope: right.scope,
                        required: false,
                        reason: "Task-local native negative IO contract".into(),
                    }],
                    demands: vec![Demand {
                        demand_id: "http".into(),
                        request_ids: BTreeSet::from(["http".into()]),
                    }],
                },
                BTreeMap::new(),
            )
            .unwrap();
        let active = native
            .resolve(
                review,
                BTreeMap::from([("http".into(), UserChoice::AllowSession)]),
            )
            .unwrap()
            .activation
            .unwrap();
        let limits = EngineLimits::default();
        let payload = ServiceValue::copy_request_from_host(
            &options(),
            &[],
            &BTreeMap::new(),
            &limits,
            quota.clone(),
            &ServiceBudget::new(&limits),
        )
        .unwrap();
        let request = HostRequest::from_transport(
            1,
            "http.request".into(),
            30000,
            "f".repeat(64),
            ServiceAuthority {
                instance_id: active.plan.instance_id,
                plan_generation: active.plan.plan_revision,
                authorization_epoch: active.plan.authorization_epoch,
            },
            ServicePhase::Async,
            payload,
        )
        .unwrap();
        let prepared = PreparedRequest::parse(&request, &limits, &quota).unwrap();
        let cost = prepared.io_cost(request.payload.wire_bytes()).unwrap();
        let ticket = Arc::new(
            native
                .dispatch(
                    &active.channel,
                    CallPhase::Async,
                    "http",
                    vec![OperationNeed::http_preflight(
                        &prepared.options.url,
                        prepared.method().unwrap(),
                    )
                    .unwrap()],
                )
                .unwrap(),
        );
        let broker = Arc::new(Mutex::new(native));
        let factory = NativeHttpAuthorityFactory::from_native(
            Arc::clone(&broker),
            active.channel,
            Arc::clone(&ticket),
            request.clone(),
            quota.clone(),
            None,
        )
        .unwrap();
        let job = HttpIoJob {
            request,
            prepared,
            factory,
            client: client.clone(),
            dns: NativeDns(Arc::new(RefusingDns)),
            quota: quota.clone(),
        };
        let mut io = broker
            .lock()
            .unwrap()
            .commit(&ticket, || {
                client.try_submit(Lane::Io, cost, job).map_err(Box::new)
            })
            .unwrap()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let retained = loop {
            match io.try_take() {
                JobPoll::Ready(value) => break value,
                JobPoll::Pending if Instant::now() < deadline => std::thread::yield_now(),
                _ => panic!("actual native IO outcome missing"),
            }
        };
        assert!(matches!(retained.view(), JobOutcome::Finished(Err(_)))); // Actual transport/body ran and refused; no fabricated HTTP success or helper Result.
        assert!(quota.snapshot().jobs >= 1);
        broker
            .lock()
            .unwrap()
            .settle_without_delivery(&ticket)
            .unwrap(); // Only after original callback actually returned.
        drop(retained);
        drop(io);
        drop(client);
        drop(broker);
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
}
