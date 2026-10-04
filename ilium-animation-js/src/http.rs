//! Native HTTPS transport for admitted broker jobs. No script runs here.
//! Each hop uses a fresh pool and a resolver containing only the DNS answers
//! already checked by the broker; environment proxies are disabled.
use crate::{
    error::{AnimationError, Result},
    network::{classify_address, validate_https_origin, NetworkAddressClass},
    permissions::HttpMethod,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    net::{SocketAddr, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use ureq::unversioned::{
    resolver::{ResolvedSocketAddrs, Resolver},
    transport::{DefaultConnector, NextTimeout},
};
use url::Url;

const MAX_BODY: usize = 4 * 1024 * 1024;
const MAX_RESPONSE: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpOptions {
    pub url: String,
    #[serde(default = "get_method")]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<Value>,
    pub response: String,
    pub max_bytes: usize,
    pub timeout_ms: u64,
    /// Opaque broker credential handle, never an actual password or token.
    #[serde(default)]
    pub credential: Option<String>,
}
fn get_method() -> String {
    "GET".into()
}
impl HttpOptions {
    pub fn validate(&self) -> Result<()> {
        validate_url(&self.url)?;
        if !matches!(self.method.as_str(), "GET" | "HEAD" | "POST")
            || !matches!(self.response.as_str(), "bytes" | "text" | "json")
        {
            return failure("unsupported HTTP method/response");
        }
        if self.max_bytes == 0
            || self.max_bytes > MAX_RESPONSE
            || !(1..=30000).contains(&self.timeout_ms)
        {
            return failure("HTTP budget");
        }
        if self.headers.len() > 32 {
            return failure("HTTP header count");
        }
        let mut bytes = 0usize;
        let mut names = BTreeSet::new();
        for (name, value) in &self.headers {
            let lower = name.to_ascii_lowercase();
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                || !names.insert(lower.clone())
                || value.bytes().any(|byte| byte.is_ascii_control())
                || matches!(
                    lower.as_str(),
                    "host"
                        | "authorization"
                        | "proxy-authorization"
                        | "cookie"
                        | "connection"
                        | "upgrade"
                        | "transfer-encoding"
                        | "content-length"
                        | "te"
                        | "trailer"
                        | "user-agent"
                )
            {
                return failure("script cannot set transport or credential headers");
            }
            bytes = bytes.saturating_add(name.len()).saturating_add(value.len());
        }
        if bytes > 8192 {
            return failure("HTTP header budget");
        }
        if self.body.is_some() && self.method != "POST" {
            return failure("body requires POST");
        }
        self.body_bytes()?;
        if self
            .credential
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
        {
            return failure("invalid credential handle");
        }
        Ok(())
    }
    fn body_bytes(&self) -> Result<Vec<u8>> {
        match &self.body {
            None => Ok(Vec::new()),
            Some(Value::String(text)) if text.len() <= MAX_BODY => Ok(text.as_bytes().to_vec()),
            Some(Value::Array(array)) if array.len() <= MAX_BODY => array
                .iter()
                .map(|value| {
                    value
                        .as_u64()
                        .and_then(|value| u8::try_from(value).ok())
                        .ok_or_else(|| {
                            AnimationError::Runtime("HTTP body must contain bytes".into())
                        })
                })
                .collect(),
            _ => failure("HTTP body budget/type"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpPhase {
    /// Checks the exact origin before DNS or any outbound acquisition.
    Preflight,
    Dispatch,
    Delivery,
}

/// Implement with a host-created operation ticket. Dispatch records the native
/// operation as issued; delivery checks the current instance, plan and epoch.
/// Local addresses additionally require network.local, not just an origin grant.
pub trait HttpAuthority {
    fn authorize(
        &mut self,
        phase: HttpPhase,
        method: HttpMethod,
        url: &Url,
        addresses: &[SocketAddr],
    ) -> Result<()>;
    fn credential_headers(&mut self, handle: &str, url: &Url) -> Result<BTreeMap<String, String>>;
}
pub trait DnsResolver {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>>;
}
pub struct SystemDns;
impl DnsResolver for SystemDns {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>> {
        // Run only on an already admitted I/O owner. getaddrinfo may outlive a
        // logical deadline; that owner's physical charge remains until exit.
        let mut addresses = Vec::new();
        for address in (host, port).to_socket_addrs()? {
            if !addresses.contains(&address) {
                addresses.push(address);
            }
            if addresses.len() > 16 {
                return failure("DNS answer budget");
            }
        }
        Ok(addresses)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
    pub final_url: String,
}
#[derive(Debug, Clone)]
struct PinnedResolver {
    host: String,
    port: u16,
    addresses: Vec<SocketAddr>,
}
impl Resolver for PinnedResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        _config: &ureq::config::Config,
        _timeout: NextTimeout,
    ) -> std::result::Result<ResolvedSocketAddrs, ureq::Error> {
        if uri.host() != Some(self.host.as_str())
            || uri.port_u16().unwrap_or(443) != self.port
            || uri.scheme_str() != Some("https")
        {
            return Err(ureq::Error::HostNotFound);
        }
        let mut result = self.empty();
        for address in &self.addresses {
            result.push(*address);
        }
        if result.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(result)
    }
}

/// Blocking native operation. Invoke inside an admitted job; its caller owns
/// the result retention and dispatch/delivery ticket, even after cancellation.
pub fn request(
    options: &HttpOptions,
    authority: &mut impl HttpAuthority,
    dns: &impl DnsResolver,
    stop: &AtomicBool,
) -> Result<HttpResponse> {
    request_with_reader(options, authority, dns, stop, |head, reader| {
        buffered_response(head, reader, &options.response)
    })
}

/// Source-service adapter: the actual admitted source wrapper reads directly
/// from the native guarded body, before any JSON/text/byte-Value expansion.
/// The caller supplies its SAME original root and owns the admitted I/O job.
pub fn request_source_response(
    options: &HttpOptions,
    authority: &mut impl HttpAuthority,
    dns: &impl DnsResolver,
    stop: &AtomicBool,
    quota: &ilium_execution::QuotaGroup,
) -> Result<crate::sources::SourceHttpResponse> {
    if options.max_bytes == 0 || options.max_bytes > 32_000_000 {
        return Err(AnimationError::Budget("source HTTP response bytes".into()));
    }
    request_with_reader(options, authority, dns, stop, |head, reader| {
        crate::sources::SourceHttpResponse::read(
            head.status,
            reader,
            quota,
            options.max_bytes,
            stop,
        )
    })
}

#[derive(Debug, Clone, Copy)]
pub struct StreamLimits {
    pub max_line_bytes: usize,
    pub max_records: usize,
}
struct LineSink<'a> {
    limits: StreamLimits,
    records: usize,
    callback: &'a mut dyn FnMut(&[u8]) -> Result<bool>,
}
impl LineSink<'_> {
    fn deliver(&mut self, line: &[u8]) -> Result<bool> {
        if line.len() > self.limits.max_line_bytes {
            return failure("HTTP stream line budget");
        }
        self.records += 1;
        let keep_reading = (self.callback)(line.strip_suffix(b"\r").unwrap_or(line))?;
        Ok(keep_reading && self.records < self.limits.max_records)
    }
}

/// Consume a finite prefix of an NDJSON/SSE source without waiting for EOF.
/// Every delivered line rechecks authority; closing this call closes its body.
pub fn stream_lines(
    options: &HttpOptions,
    authority: &mut impl HttpAuthority,
    dns: &impl DnsResolver,
    stop: &AtomicBool,
    limits: StreamLimits,
    callback: &mut dyn FnMut(&[u8]) -> Result<bool>,
) -> Result<HttpResponse> {
    if limits.max_line_bytes == 0
        || limits.max_line_bytes > 65536
        || limits.max_records == 0
        || limits.max_records > 65536
    {
        return failure("HTTP stream budget");
    }
    request_with_reader(options, authority, dns, stop, |head, reader| {
        line_response(
            head,
            reader,
            LineSink {
                limits,
                records: 0,
                callback,
            },
        )
    })
}

/// Status/filtered headers are metadata, not an eager JSON-expanded body.
#[derive(Debug)]
pub struct HttpResponseHead {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub final_url: String,
}

struct ReadContext<'a> {
    authority: &'a mut dyn HttpAuthority,
    method: HttpMethod,
    url: &'a Url,
    addresses: &'a [SocketAddr],
    stop: &'a AtomicBool,
    deadline: Instant,
    max_bytes: usize,
}

/// Native-only bounded body access. A chunk is privately staged and only copied
/// into the consumer's buffer after current delivery authorization succeeds.
/// The caller already owns original I/O, scratch and output-retention admission.
/// This type exposes checking/read access, never authority mutators or raw body.
pub struct HttpBodyReader<'a> {
    raw: &'a mut dyn Read,
    context: ReadContext<'a>,
    received: usize,
    scratch: [u8; 8192],
    failure: Option<AnimationError>,
}
impl HttpBodyReader<'_> {
    /// Stream consumers recheck before publishing each record from a prior chunk.
    pub fn check_delivery(&mut self) -> Result<()> {
        if self.failure.is_some() {
            return failure("HTTP body reader previously failed");
        }
        let result = check_active(self.context.stop, self.context.deadline).and_then(|()| {
            self.context.authority.authorize(
                HttpPhase::Delivery,
                self.context.method,
                self.context.url,
                self.context.addresses,
            )
        });
        if let Err(error) = result {
            self.failure = Some(error);
            return failure("HTTP body delivery failed");
        }
        Ok(())
    }
    fn failed_read(&mut self, error: AnimationError) -> std::io::Error {
        self.failure = Some(error);
        std::io::Error::other("HTTP body read failed")
    }
}
impl Read for HttpBodyReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        self.check_delivery()
            .map_err(|_| std::io::Error::other("HTTP body delivery failed"))?;
        if output.is_empty() {
            return Ok(0);
        }
        // At the bound probe one byte for EOF without exposing overflow bytes.
        let remaining = self.context.max_bytes - self.received;
        let limit = output.len().min(self.scratch.len()).min(remaining.max(1));
        let count = match self.raw.read(&mut self.scratch[..limit]) {
            Ok(count) => count,
            Err(error) => return Err(self.failed_read(AnimationError::Io(error))),
        };
        self.check_delivery()
            .map_err(|_| std::io::Error::other("HTTP body delivery failed"))?;
        if count > limit {
            return Err(self.failed_read(AnimationError::Runtime(
                "HTTP reader returned an invalid count".into(),
            )));
        }
        let Some(total) = self
            .received
            .checked_add(count)
            .filter(|total| *total <= self.context.max_bytes)
        else {
            return Err(self.failed_read(AnimationError::Runtime("HTTP response budget".into())));
        };
        self.received = total;
        output[..count].copy_from_slice(&self.scratch[..count]);
        Ok(count)
    }
}

fn consume_response<'a, T>(
    head: HttpResponseHead,
    raw: &'a mut dyn Read,
    context: ReadContext<'a>,
    consumer: impl FnOnce(HttpResponseHead, &mut HttpBodyReader<'_>) -> Result<T>,
) -> Result<T> {
    let mut reader = HttpBodyReader {
        raw,
        context,
        received: 0,
        scratch: [0; 8192],
        failure: None,
    };
    reader
        .check_delivery()
        .map_err(|fallback| reader.failure.take().unwrap_or(fallback))?; // No metadata/body callback before current authorization.
    let result = consumer(head, &mut reader);
    let final_check = reader.check_delivery();
    if let Some(error) = reader.failure.take() {
        return Err(error);
    }
    final_check?;
    result
}

fn buffered_response(
    head: HttpResponseHead,
    reader: &mut HttpBodyReader<'_>,
    response: &str,
) -> Result<HttpResponse> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    let body = match response {
        "bytes" => Value::Array(
            bytes
                .into_iter()
                .map(|byte| Value::from(u64::from(byte)))
                .collect(),
        ),
        "text" => Value::String(
            String::from_utf8(bytes)
                .map_err(|_| AnimationError::Runtime("HTTP text is not UTF-8".into()))?,
        ),
        "json" => serde_json::from_slice(&bytes)?,
        _ => return failure("unsupported response"),
    };
    Ok(HttpResponse {
        status: head.status,
        headers: head.headers,
        body,
        final_url: head.final_url,
    })
}

fn line_response(
    head: HttpResponseHead,
    reader: &mut HttpBodyReader<'_>,
    mut sink: LineSink<'_>,
) -> Result<HttpResponse> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    'body: loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        while let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
            reader.check_delivery()?;
            if !sink.deliver(&bytes[..end])? {
                bytes.clear();
                break 'body;
            }
            bytes.drain(..=end);
        }
        if bytes.len() > sink.limits.max_line_bytes {
            return failure("HTTP stream line budget");
        }
    }
    if !bytes.is_empty() {
        reader.check_delivery()?;
        sink.deliver(&bytes)?;
    }
    Ok(HttpResponse {
        status: head.status,
        headers: head.headers,
        body: Value::Null,
        final_url: head.final_url,
    })
}

/// Blocking native raw consumer inside the caller's already admitted I/O job.
/// No eager serde Value/body expansion occurs before this consumer. Reserve
/// input/scratch and T's retained allocations BEFORE calling; this function does
/// not create a quota, executor or permit uncharged copies. The bounded reader
/// holds its original ticket until return; getaddrinfo/read may physically outlive
/// cancellation, so caller custody/admission must survive actual native exit.
pub fn request_with_reader<T>(
    options: &HttpOptions,
    authority: &mut impl HttpAuthority,
    dns: &impl DnsResolver,
    stop: &AtomicBool,
    consumer: impl FnOnce(HttpResponseHead, &mut HttpBodyReader<'_>) -> Result<T>,
) -> Result<T> {
    options.validate()?;
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(options.timeout_ms))
        .ok_or_else(|| AnimationError::Runtime("HTTP deadline overflow".into()))?;
    let mut url = validate_url(&options.url)?;
    let mut method = options.method.clone();
    let mut body = options.body_bytes()?;
    let mut charged_origins = BTreeSet::new();
    for hop in 0..=5 {
        check_active(stop, deadline)?;
        let actual_method = match method.as_str() {
            "GET" => HttpMethod::Get,
            "HEAD" => HttpMethod::Head,
            "POST" => HttpMethod::Post,
            _ => return failure("unsupported native HTTP method"),
        };
        authority.authorize(HttpPhase::Preflight, actual_method, &url, &[])?;
        let origin = url.origin().ascii_serialization();
        if charged_origins.insert(origin.clone()) {
            reserve_origin(&origin)?;
        }
        let host = url
            .host_str()
            .ok_or_else(|| AnimationError::Runtime("HTTP hostname missing".into()))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| AnimationError::Runtime("HTTP port missing".into()))?;
        let addresses = dns.resolve(host, port)?;
        check_active(stop, deadline)?;
        validate_addresses(&addresses, port)?;
        authority.authorize(HttpPhase::Dispatch, actual_method, &url, &addresses)?;
        let mut headers = options.headers.clone();
        if let Some(handle) = &options.credential {
            // The broker must bind the credential to this exact redirect origin.
            for (name, value) in authority.credential_headers(handle, &url)? {
                headers.insert(name, value);
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .max_response_header_size(16384)
            .user_agent(concat!(
                "ilium/",
                env!("CARGO_PKG_VERSION"),
                " animation-runtime"
            ))
            .timeout_global(Some(remaining))
            .timeout_connect(Some(remaining.min(Duration::from_secs(5))))
            .timeout_recv_body(Some(remaining.min(Duration::from_secs(1))))
            .build();
        let agent = ureq::Agent::with_parts(
            config,
            DefaultConnector::default(),
            PinnedResolver {
                host: host.into(),
                port,
                addresses: addresses.clone(),
            },
        );
        let mut builder = ureq::http::Request::builder()
            .method(method.as_str())
            .uri(url.as_str());
        for (name, value) in &headers {
            builder = builder.header(name, value);
        }
        let wire = builder
            .body(body.as_slice())
            .map_err(|error| AnimationError::Runtime(error.to_string()))?;
        check_active(stop, deadline)?;
        let mut response = agent
            .run(wire)
            .map_err(|error| AnimationError::Runtime(format!("HTTP: {error}")))?;
        let status = response.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            if hop == 5 {
                return failure("HTTP redirect limit");
            }
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| AnimationError::Runtime("redirect without valid Location".into()))?;
            let next = url
                .join(location)
                .map_err(|error| AnimationError::Runtime(error.to_string()))?;
            validate_url(next.as_str())?;
            authority.authorize(HttpPhase::Delivery, actual_method, &url, &addresses)?;
            if status == 303 || matches!(status, 301 | 302) && method == "POST" {
                method = "GET".into();
                body.clear();
            }
            url = next;
            continue;
        }
        let mut response_headers = BTreeMap::new();
        for (name, value) in response.headers() {
            if matches!(name.as_str(), "set-cookie" | "proxy-authenticate") {
                continue;
            }
            if let Ok(value) = value.to_str() {
                response_headers.insert(name.as_str().into(), value.into());
            }
        }
        let mut raw = response.body_mut().as_reader();
        return consume_response(
            HttpResponseHead {
                status,
                headers: response_headers,
                final_url: url.as_str().into(),
            },
            &mut raw,
            ReadContext {
                authority,
                method: actual_method,
                url: &url,
                addresses: &addresses,
                stop,
                deadline,
                max_bytes: options.max_bytes,
            },
            consumer,
        );
    }
    failure("HTTP redirect limit")
}
fn validate_url(input: &str) -> Result<Url> {
    if input.len() > 4096
        || input
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
    {
        return failure("invalid HTTP URL");
    }
    let url = Url::parse(input).map_err(|error| AnimationError::Runtime(error.to_string()))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return failure("HTTP requires credential-free HTTPS URL");
    }
    validate_https_origin(&url.origin().ascii_serialization())?;
    Ok(url)
}
fn validate_addresses(addresses: &[SocketAddr], port: u16) -> Result<()> {
    if addresses.is_empty()
        || addresses.len() > 16
        || addresses.iter().any(|address| {
            address.port() != port
                || classify_address(address.ip()) == NetworkAddressClass::Forbidden
        })
    {
        return failure("forbidden or invalid DNS answer");
    }
    Ok(())
}
fn check_active(stop: &AtomicBool, deadline: Instant) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        return failure("HTTP cancelled");
    }
    if Instant::now() >= deadline {
        return failure("HTTP deadline expired");
    }
    Ok(())
}
fn reserve_origin(origin: &str) -> Result<()> {
    static RATES: OnceLock<Mutex<BTreeMap<String, Instant>>> = OnceLock::new();
    let now = Instant::now();
    let mut rates = RATES
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| AnimationError::Runtime("HTTP rate ledger poisoned".into()))?;
    rates.retain(|_, time| now.saturating_duration_since(*time) < Duration::from_secs(60));
    if rates
        .get(origin)
        .is_some_and(|time| now.saturating_duration_since(*time) < Duration::from_secs(1))
    {
        return failure("HTTP origin rate limit; retry after 1000 ms");
    }
    if rates.len() >= 256 && !rates.contains_key(origin) {
        return failure("HTTP origin ledger budget");
    }
    rates.insert(origin.into(), now);
    Ok(())
}
fn failure<T>(message: &str) -> Result<T> {
    Err(AnimationError::Runtime(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_dns_cannot_resolve_a_second_host_or_port() {
        let resolver = PinnedResolver {
            host: "example.org".into(),
            port: 443,
            addresses: vec!["1.1.1.1:443".parse().unwrap()],
        };
        let config = ureq::Agent::config_builder().proxy(None).build();
        let timeout = NextTimeout {
            after: ureq::unversioned::transport::time::Duration::Exact(Duration::from_secs(1)),
            reason: ureq::Timeout::Resolve,
        };
        let permitted = resolver
            .resolve(
                &"https://example.org/path".parse().unwrap(),
                &config,
                timeout,
            )
            .unwrap();
        assert_eq!(
            permitted.iter().copied().collect::<Vec<_>>(),
            vec!["1.1.1.1:443".parse::<SocketAddr>().unwrap()]
        );
        for uri in [
            "https://other.example/",
            "https://example.org:444/",
            "http://example.org/",
        ] {
            assert!(resolver
                .resolve(&uri.parse().unwrap(), &config, timeout)
                .is_err());
        }
        assert!(validate_addresses(&["1.1.1.1:444".parse().unwrap()], 443).is_err());
        assert!(validate_addresses(&["192.0.2.1:443".parse().unwrap()], 443).is_err());
        assert!(config.proxy().is_none());
    }
}

#[cfg(test)]
#[path = "http_reader_contract.rs"]
mod reader_contract;
