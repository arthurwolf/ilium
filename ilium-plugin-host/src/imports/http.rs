//! Host implementation of `ilium:host/http` (api-http.md, api-http-streams.md).
//!
//! `fetch` and `open-stream` run on worker threads of the import. Plain `http` goes to loopback
//! only and is spoken here over a TCP socket; `https` is spoken by `ureq` (rustls). The host a
//! plugin may reach is the `http:<host-pattern>` grants of its instance. When a request ends,
//! `request-completed` is queued for the plugin; a stream queues `http-stream-data` for every
//! chunk and `http-stream-closed` at its end. All state belongs to the import of one plugin
//! instance: dropping it cancels its workers.

use ilium_plugin_api::generated::host::http::{
    HostHttp, HttpRequest, HttpResponse, StreamFraming, StreamRequest,
};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::events::{
    Event, HttpStreamClosedEvent, HttpStreamDataEvent, StreamOutcome,
};
use ilium_plugin_api::generated::types::ids::{RequestId, StreamId};

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::errors::ErrorCode;
use ilium_plugin_api::generated::types::Header;
use ilium_plugin_api::generated::types::Outcome;

use super::env::{completion_event, ImportEnv};
use crate::runtime::pending_events::EventSink;

/// Longest URL, in bytes.
const MAX_URL_BYTES: usize = 8 * 1024;
/// Most request headers.
const MAX_HEADERS: usize = 64;
/// Longest request body, in bytes.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
/// Accepted `timeout_millis` range.
const MIN_TIMEOUT_MILLIS: u32 = 1;
const MAX_TIMEOUT_MILLIS: u32 = 300_000;
/// Pending requests one plugin may hold before `busy`.
const PENDING_REQUEST_LIMIT: usize = 64;
/// Open streams one plugin may hold before `quota-exceeded`.
const STREAMS_PER_PLUGIN_LIMIT: usize = 4;
/// Largest `max_chunk_bytes` of a stream.
const MAX_CHUNK_BYTES: u32 = 64 * 1024;
/// Largest response the host reads into memory, in bytes.
const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
/// How long `take_response` waits for an in-flight request before reporting nothing.
const TAKE_RESPONSE_PATIENCE: Duration = Duration::from_secs(2);
/// How often a blocked socket read looks at the cancel flag.
const CANCEL_POLL: Duration = Duration::from_millis(100);
/// A stream worker pauses reading while this many events wait for the plugin, so a slow plugin
/// slows the socket down instead of losing chunks.
const STREAM_BACKLOG_EVENTS: usize = 1_024;
/// Request headers a plugin may not set (SEC-079).
const FORBIDDEN_HEADERS: [&str; 5] = [
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "upgrade",
];

/// Completion slot shared between a request thread and `take_response`.
type ResponseSlot = Arc<(Mutex<Option<Result<HttpResponse, PluginError>>>, Condvar)>;

/// What the import tells the worker of one request or stream.
#[derive(Debug, Default)]
struct WorkerControl {
    /// Set when the plugin closed the stream or the import dropped.
    cancel: AtomicBool,
    /// Set by the worker once it queued its last event.
    done: AtomicBool,
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// A parsed `scheme://host[:port]/path` URL.
struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path_and_query: String,
}

impl ParsedUrl {
    /// True for the loopback hosts that may use plain `http` (SEC-077).
    fn is_loopback(&self) -> bool {
        matches!(
            self.host.as_str(),
            "127.0.0.1" | "localhost" | "[::1]" | "::1"
        )
    }
}

/// Parses `url`, refusing anything that is not `scheme://host...` with a valid port.
fn parse_url(url: &str, allowed_schemes: &[&str]) -> Result<ParsedUrl, PluginError> {
    let bad = |message: &str| error(ErrorCode::InvalidArgument, message);
    if url.is_empty() || url.len() > MAX_URL_BYTES {
        return Err(bad("url must be 1 to 8192 bytes"));
    }
    if url
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(bad("url holds whitespace or control characters"));
    }
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| bad("url has no scheme"))?;
    let scheme = scheme.to_ascii_lowercase();
    if !allowed_schemes.contains(&scheme.as_str()) {
        return Err(bad("url scheme is not allowed here"));
    }
    let (authority, path_and_query) = match rest.find(['/', '?']) {
        Some(index) if rest.as_bytes()[index] == b'/' => (&rest[..index], rest[index..].to_owned()),
        Some(index) => (&rest[..index], format!("/{}", &rest[index..])),
        None => (rest, "/".to_owned()),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(bad("url host is missing or carries credentials"));
    }
    let default_port = if scheme == "https" || scheme == "wss" {
        443
    } else {
        80
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => {
            let port = port
                .parse::<u16>()
                .map_err(|_| bad("url port is invalid"))?;
            (host, port)
        }
        _ => (authority, default_port),
    };
    if host.is_empty() {
        return Err(bad("url host is empty"));
    }
    Ok(ParsedUrl {
        scheme,
        host: host.to_ascii_lowercase(),
        port,
        path_and_query,
    })
}

/// The grant string that names a destination: `http:<host>` on the default port of the scheme,
/// `http:<host>:<port>` otherwise.
fn grant_of(parsed: &ParsedUrl) -> String {
    let default_port = if parsed.scheme == "https" { 443 } else { 80 };
    if parsed.port == default_port {
        format!("http:{}", parsed.host)
    } else {
        format!("http:{}:{}", parsed.host, parsed.port)
    }
}

/// Checks the request shape and destination; returns the parsed URL. The destination must be
/// covered by one of the plugin's `http:` grants.
fn check_request(request: &HttpRequest, env: &ImportEnv) -> Result<ParsedUrl, PluginError> {
    let parsed = parse_url(&request.url, &["http", "https"])?;
    if request.headers.len() > MAX_HEADERS {
        return Err(error(ErrorCode::InvalidArgument, "at most 64 headers"));
    }
    let forbidden = request.headers.iter().any(|header: &Header| {
        FORBIDDEN_HEADERS.contains(&header.name.to_ascii_lowercase().as_str())
    });
    if forbidden {
        return Err(error(
            ErrorCode::InvalidArgument,
            "header name is not settable by a plugin",
        ));
    }
    if request
        .body
        .as_ref()
        .is_some_and(|body| body.len() > MAX_BODY_BYTES)
    {
        return Err(error(
            ErrorCode::InvalidArgument,
            "request body exceeds 8 MiB",
        ));
    }
    if !(MIN_TIMEOUT_MILLIS..=MAX_TIMEOUT_MILLIS).contains(&request.timeout_millis) {
        return Err(error(
            ErrorCode::InvalidArgument,
            "timeout must be 1 to 300000 ms",
        ));
    }
    if request.method.is_empty() || !request.method.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(error(
            ErrorCode::InvalidArgument,
            "method must be an HTTP token",
        ));
    }
    if parsed.scheme == "http" && !parsed.is_loopback() {
        return Err(error(
            ErrorCode::InvalidArgument,
            "plain http is only allowed to loopback",
        ));
    }
    if !env.holds(&grant_of(&parsed)) {
        return Err(error(
            ErrorCode::NotPermitted,
            "no http grant covers this host",
        ));
    }
    Ok(parsed)
}

/// The status line, headers and body reader of an answered request.
struct Opened {
    status: u16,
    headers: Vec<Header>,
    body: Box<dyn Read + Send>,
}

fn io_failure(failure: &std::io::Error) -> PluginError {
    match failure.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
            error(ErrorCode::TimedOut, "request timed out")
        }
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => {
            error(ErrorCode::Network, "malformed http response")
        }
        _ => error(ErrorCode::Network, "connection failed"),
    }
}

/// Splits a response head into its status and headers.
fn parse_head(head: &[u8]) -> Result<(u16, Vec<Header>), PluginError> {
    let malformed = || error(ErrorCode::Network, "malformed http response");
    let head = std::str::from_utf8(head).map_err(|_| malformed())?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(malformed)?;
    let headers: Vec<Header> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| Header {
            name: name.trim().to_owned(),
            value: value.trim().to_owned(),
        })
        .collect();
    Ok((status, headers))
}

/// A body reader that gives up when the cancel flag is set and bounds the whole exchange by a
/// deadline; the socket reads in short slices so it notices both.
struct SocketBody {
    stream: TcpStream,
    leftover: std::io::Cursor<Vec<u8>>,
    control: Arc<WorkerControl>,
    deadline: Option<Instant>,
}

impl Read for SocketBody {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let from_leftover = self.leftover.read(buffer)?;
        if from_leftover > 0 {
            return Ok(from_leftover);
        }
        loop {
            if self.control.cancel.load(Ordering::Acquire) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "cancelled",
                ));
            }
            if self
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "timed out",
                ));
            }
            match self.stream.read(buffer) {
                Err(failure)
                    if matches!(
                        failure.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                other => return other,
            }
        }
    }
}

/// Longest chunk-size or trailer line of a chunked body.
const MAX_CHUNK_LINE_BYTES: u64 = 4096;
/// Most trailer fields a chunked body may end with.
const MAX_TRAILER_LINES: usize = 64;

fn malformed_body(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.to_owned())
}

/// Decodes a `Transfer-Encoding: chunked` body (RFC 9112 section 7.1): chunk extensions are
/// ignored, trailer fields are read and dropped, and a malformed size line or a body that ends
/// inside a chunk is an `InvalidData` or `UnexpectedEof` error. The reader yields only the
/// decoded bytes, so every size limit applies to the payload, not to the framing.
struct ChunkedBody<R: Read> {
    inner: std::io::BufReader<R>,
    /// Bytes left in the current chunk.
    remaining: u64,
    done: bool,
}

impl<R: Read> ChunkedBody<R> {
    fn new(inner: R) -> Self {
        Self {
            inner: std::io::BufReader::new(inner),
            remaining: 0,
            done: false,
        }
    }

    /// One CRLF-terminated line without its terminator.
    fn line(&mut self) -> std::io::Result<Vec<u8>> {
        use std::io::BufRead;
        let mut line = Vec::new();
        (&mut self.inner)
            .take(MAX_CHUNK_LINE_BYTES)
            .read_until(b'\n', &mut line)?;
        if line.last() != Some(&b'\n') {
            return Err(if line.len() as u64 >= MAX_CHUNK_LINE_BYTES {
                malformed_body("chunk line is too long")
            } else {
                std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "chunked body ended early",
                )
            });
        }
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        Ok(line)
    }

    /// The size of the next chunk, from a `<hex>[;extension]` line.
    fn next_size(&mut self) -> std::io::Result<u64> {
        let line = self.line()?;
        let text =
            std::str::from_utf8(&line).map_err(|_| malformed_body("chunk size is not text"))?;
        let digits = text.split(';').next().unwrap_or_default().trim();
        if digits.is_empty() || digits.len() > 16 {
            return Err(malformed_body("chunk size is malformed"));
        }
        u64::from_str_radix(digits, 16).map_err(|_| malformed_body("chunk size is malformed"))
    }

    /// Reads the trailer section up to the blank line that ends the body.
    fn skip_trailers(&mut self) -> std::io::Result<()> {
        for _ in 0..=MAX_TRAILER_LINES {
            if self.line()?.is_empty() {
                return Ok(());
            }
        }
        Err(malformed_body("too many trailer fields"))
    }
}

impl<R: Read> Read for ChunkedBody<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.done || buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let size = self.next_size()?;
            if size == 0 {
                self.skip_trailers()?;
                self.done = true;
                return Ok(0);
            }
            self.remaining = size;
        }
        let wanted = buffer
            .len()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        let count = self.inner.read(&mut buffer[..wanted])?;
        if count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "chunked body ended inside a chunk",
            ));
        }
        self.remaining -= count as u64;
        if self.remaining == 0 && !self.line()?.is_empty() {
            return Err(malformed_body("chunk data is longer than its size"));
        }
        Ok(count)
    }
}

/// Whether a response head declares a chunked body.
fn is_chunked(headers: &[Header]) -> bool {
    headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("transfer-encoding")
            && header
                .value
                .split(',')
                .any(|coding| coding.trim().eq_ignore_ascii_case("chunked"))
    })
}

/// Speaks plain HTTP/1.1 (`Connection: close`) to a loopback server and returns once the response
/// head has arrived. A chunked body is decoded.
fn open_plain(
    parsed: &ParsedUrl,
    request: &HttpRequest,
    control: &Arc<WorkerControl>,
    whole_exchange_deadline: bool,
) -> Result<Opened, PluginError> {
    let timeout = Duration::from_millis(u64::from(request.timeout_millis));
    let deadline = Instant::now() + timeout;
    let address = (parsed.host.trim_matches(['[', ']']), parsed.port)
        .to_socket_addrs()
        .map_err(|_| error(ErrorCode::Network, "host did not resolve"))?
        .next()
        .ok_or_else(|| error(ErrorCode::Network, "host did not resolve"))?;
    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|_| error(ErrorCode::Network, "connection failed"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|failure| io_failure(&failure))?;
    let body = request.body.clone().unwrap_or_default();
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\nContent-Length: {}\r\n",
        request.method.to_ascii_uppercase(),
        parsed.path_and_query,
        parsed.host,
        parsed.port,
        body.len()
    );
    for header in &request.headers {
        head.push_str(&format!("{}: {}\r\n", header.name, header.value));
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&body))
        .map_err(|failure| io_failure(&failure))?;
    stream
        .set_read_timeout(Some(CANCEL_POLL))
        .map_err(|failure| io_failure(&failure))?;
    let mut reader = SocketBody {
        stream,
        leftover: std::io::Cursor::new(Vec::new()),
        control: Arc::clone(control),
        deadline: Some(deadline),
    };
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    let split = loop {
        if let Some(position) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if raw.len() > 64 * 1024 {
            return Err(error(ErrorCode::Network, "malformed http response"));
        }
        match reader.read(&mut chunk) {
            Ok(0) => return Err(error(ErrorCode::Network, "malformed http response")),
            Ok(count) => raw.extend_from_slice(&chunk[..count]),
            Err(failure) => return Err(io_failure(&failure)),
        }
    };
    let (status, mut headers) = parse_head(&raw[..split])?;
    reader.leftover = std::io::Cursor::new(raw[split + 4..].to_vec());
    if !whole_exchange_deadline {
        reader.deadline = None;
    }
    if is_chunked(&headers) {
        // RFC 9112 section 6.3: the transfer coding wins over a Content-Length sent with it.
        headers.retain(|header| !header.name.eq_ignore_ascii_case("content-length"));
        return Ok(Opened {
            status,
            headers,
            body: Box::new(ChunkedBody::new(reader)),
        });
    }
    Ok(Opened {
        status,
        headers,
        body: Box::new(reader),
    })
}

/// Speaks `https` through `ureq` and returns once the response head has arrived. Redirects are
/// not followed and an HTTP error status is a normal response.
fn open_secure(
    request: &HttpRequest,
    whole_exchange_deadline: bool,
) -> Result<Opened, PluginError> {
    let timeout = Duration::from_millis(u64::from(request.timeout_millis));
    let mut configuration = ureq::Agent::config_builder();
    configuration = configuration
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_connect(Some(timeout));
    if whole_exchange_deadline {
        configuration = configuration.timeout_global(Some(timeout));
    }
    let agent = ureq::Agent::new_with_config(configuration.build());
    let mut builder = ureq::http::Request::builder()
        .method(request.method.to_ascii_uppercase().as_str())
        .uri(request.url.as_str());
    for header in &request.headers {
        builder = builder.header(header.name.as_str(), header.value.as_str());
    }
    let invalid_request = |_| error(ErrorCode::InvalidArgument, "the request cannot be built");
    let outcome = match request.body.as_deref().filter(|body| !body.is_empty()) {
        None => agent.run(builder.body(()).map_err(invalid_request)?),
        Some(body) => agent.run(builder.body(body.to_vec()).map_err(invalid_request)?),
    };
    let response = outcome.map_err(|failure| match failure {
        ureq::Error::Timeout(_) => error(ErrorCode::TimedOut, "request timed out"),
        _ => error(ErrorCode::Network, "connection failed"),
    })?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| Header {
            name: name.as_str().to_owned(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect();
    let reader = response.into_body().into_reader();
    Ok(Opened {
        status,
        headers,
        body: Box::new(reader),
    })
}

/// Opens a request on the transport its scheme needs.
fn open_request(
    parsed: &ParsedUrl,
    request: &HttpRequest,
    control: &Arc<WorkerControl>,
    whole_exchange_deadline: bool,
) -> Result<Opened, PluginError> {
    if parsed.scheme == "https" {
        open_secure(request, whole_exchange_deadline)
    } else {
        open_plain(parsed, request, control, whole_exchange_deadline)
    }
}

/// Performs one exchange and reads the whole body (at most 8 MiB).
fn perform(
    parsed: &ParsedUrl,
    request: &HttpRequest,
    control: &Arc<WorkerControl>,
) -> Result<HttpResponse, PluginError> {
    let opened = open_request(parsed, request, control, true)?;
    let mut body = Vec::new();
    opened
        .body
        .take(MAX_RESPONSE_BYTES)
        .read_to_end(&mut body)
        .map_err(|failure| io_failure(&failure))?;
    let declared = opened
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-length"))
        .and_then(|header| header.value.parse::<usize>().ok());
    if let Some(length) = declared {
        body.truncate(length);
    }
    Ok(HttpResponse {
        status: opened.status,
        headers: opened.headers,
        body,
    })
}

/// Cuts a byte stream into the chunks one framing delivers.
struct Framer {
    framing: StreamFraming,
    max_chunk_bytes: usize,
    pending: Vec<u8>,
    /// `data:` lines of the server-sent event being read.
    event_data: Vec<Vec<u8>>,
}

impl Framer {
    fn new(framing: StreamFraming, max_chunk_bytes: u32) -> Self {
        Self {
            framing,
            max_chunk_bytes: max_chunk_bytes as usize,
            pending: Vec::new(),
            event_data: Vec::new(),
        }
    }

    fn split_long(&self, record: Vec<u8>, out: &mut Vec<Vec<u8>>) {
        if record.len() <= self.max_chunk_bytes {
            out.push(record);
        } else {
            out.extend(record.chunks(self.max_chunk_bytes).map(<[u8]>::to_vec));
        }
    }

    /// Feeds bytes that arrived; returns the chunks they complete.
    fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if bytes.is_empty() {
            return out;
        }
        if matches!(self.framing, StreamFraming::Raw) {
            self.split_long(bytes.to_vec(), &mut out);
            return out;
        }
        self.pending.extend_from_slice(bytes);
        while let Some(position) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = self.pending.drain(..=position).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.line(line, &mut out);
        }
        out
    }

    /// Ends the stream; returns what is left over.
    fn finish(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let mut line = std::mem::take(&mut self.pending);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.line(line, &mut out);
        }
        if matches!(self.framing, StreamFraming::ServerSentEvents) && !self.event_data.is_empty() {
            let record = self.event_data.drain(..).collect::<Vec<_>>().join(&b'\n');
            self.split_long(record, &mut out);
        }
        out
    }

    fn line(&mut self, line: Vec<u8>, out: &mut Vec<Vec<u8>>) {
        match self.framing {
            StreamFraming::Lines => self.split_long(line, out),
            StreamFraming::Ndjson => {
                if !line.iter().all(u8::is_ascii_whitespace) {
                    self.split_long(line, out);
                }
            }
            StreamFraming::ServerSentEvents => {
                if line.is_empty() {
                    if !self.event_data.is_empty() {
                        let record = self.event_data.drain(..).collect::<Vec<_>>().join(&b'\n');
                        self.split_long(record, out);
                    }
                } else if let Some(data) = line.strip_prefix(b"data:") {
                    self.event_data
                        .push(data.strip_prefix(b" ").unwrap_or(data).to_vec());
                }
            }
            StreamFraming::Raw => {}
        }
    }
}

/// One open stream of the plugin.
#[derive(Debug)]
struct StreamEntry {
    control: Arc<WorkerControl>,
    worker: Option<thread::JoinHandle<()>>,
}

impl StreamEntry {
    fn is_open(&self) -> bool {
        !self.control.done.load(Ordering::Acquire)
    }
}

impl Drop for StreamEntry {
    fn drop(&mut self) {
        self.control.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The worker of one stream: connect, deliver the chunks, close.
fn run_stream(
    parsed: ParsedUrl,
    request: StreamRequest,
    stream: StreamId,
    control: Arc<WorkerControl>,
    sink: EventSink,
) {
    let closed = |outcome: StreamOutcome| {
        sink.send(Event::HttpStreamClosed(HttpStreamClosedEvent {
            stream,
            outcome,
        }));
    };
    let opened = match open_request(&parsed, &request.request, &control, false) {
        Ok(opened) => opened,
        Err(failure) => {
            if !control.cancel.load(Ordering::Acquire) {
                closed(StreamOutcome::Failed(failure));
            }
            control.done.store(true, Ordering::Release);
            return;
        }
    };
    let Opened {
        status,
        headers,
        mut body,
    } = opened;
    let mut framer = Framer::new(request.framing, request.max_chunk_bytes);
    let mut first = Some((status, headers));
    let send_chunk = |chunk: Vec<u8>, first: &mut Option<(u16, Vec<Header>)>| {
        let (status, headers) = match first.take() {
            Some((status, headers)) => (Some(status), Some(headers)),
            None => (None, None),
        };
        while sink.backlog() >= STREAM_BACKLOG_EVENTS && !control.cancel.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(5));
        }
        sink.send(Event::HttpStreamData(HttpStreamDataEvent {
            stream,
            status,
            headers,
            chunk,
        }));
    };
    let mut buffer = vec![0u8; 16 * 1024];
    let outcome = loop {
        if control.cancel.load(Ordering::Acquire) {
            break StreamOutcome::Cancelled;
        }
        match body.read(&mut buffer) {
            Ok(0) => {
                for chunk in framer.finish() {
                    send_chunk(chunk, &mut first);
                }
                // A stream with no body still reports its status and headers once.
                if first.is_some() {
                    send_chunk(Vec::new(), &mut first);
                }
                break StreamOutcome::Ended;
            }
            Ok(count) => {
                let chunks = framer.push(&buffer[..count]);
                for chunk in chunks {
                    send_chunk(chunk, &mut first);
                }
            }
            Err(failure) if failure.kind() == std::io::ErrorKind::Interrupted => {
                break StreamOutcome::Cancelled;
            }
            Err(failure) => break StreamOutcome::Failed(io_failure(&failure)),
        }
    };
    // A cancelled stream was closed by the plugin (or its instance ended); nobody waits for it.
    if !matches!(outcome, StreamOutcome::Cancelled) {
        closed(outcome);
    }
    control.done.store(true, Ordering::Release);
}

/// Host side of the `http` interface of one plugin instance.
#[derive(Debug)]
pub struct HttpImport {
    env: ImportEnv,
    requests: HashMap<u64, ResponseSlot>,
    request_controls: Vec<Arc<WorkerControl>>,
    streams: HashMap<u64, StreamEntry>,
    next_stream: u64,
    /// How long `take-response` waits for a fetch still in flight.
    patience: Duration,
}

impl Default for HttpImport {
    fn default() -> Self {
        Self::new(ImportEnv::unrestricted())
    }
}

impl Drop for HttpImport {
    fn drop(&mut self) {
        for control in &self.request_controls {
            control.cancel.store(true, Ordering::Release);
        }
        // Stream entries cancel and join their workers when dropped.
    }
}

impl HttpImport {
    /// The import of the instance `env` describes.
    pub fn new(env: ImportEnv) -> Self {
        Self {
            env,
            requests: HashMap::new(),
            request_controls: Vec::new(),
            streams: HashMap::new(),
            next_stream: 0,
            patience: TAKE_RESPONSE_PATIENCE,
        }
    }

    /// An import whose `take-response` never waits: a response that is not ready yet is
    /// reported as not ready at once. For guests that run inside a render frame (scenes).
    pub fn without_waiting(mut self) -> Self {
        self.patience = Duration::ZERO;
        self
    }

    /// How many streams are open (not yet ended or closed).
    pub fn open_streams(&self) -> usize {
        self.streams
            .values()
            .filter(|entry| entry.is_open())
            .count()
    }

    /// Waits up to `budget` for a fetch to finish and maps it to its completion outcome (`ok` for
    /// any HTTP status, `failed(code)` for a transport failure); `None` when the request is
    /// unknown here or still running after the budget. The response stays available for
    /// `take-response`.
    pub fn await_outcome(&self, request: RequestId, budget: Duration) -> Option<Outcome> {
        let slot = self.requests.get(&request.0).cloned()?;
        let (lock, ready) = &*slot;
        let guard = lock.lock().ok()?;
        let (guard, _) = ready
            .wait_timeout_while(guard, budget, |held| held.is_none())
            .ok()?;
        match guard.as_ref()? {
            Ok(_) => Some(Outcome::Ok),
            Err(failure) => Some(Outcome::Failed(failure.clone())),
        }
    }
}

impl HostHttp for HttpImport {
    fn fetch(&mut self, request: HttpRequest) -> Result<RequestId, PluginError> {
        let parsed = check_request(&request, &self.env)?;
        if self.requests.len() >= PENDING_REQUEST_LIMIT {
            return Err(error(
                ErrorCode::Busy,
                "64 http requests are already pending",
            ));
        }
        let id = self.env.next_request();
        let slot: ResponseSlot = Arc::new((Mutex::new(None), Condvar::new()));
        self.requests.insert(id.0, Arc::clone(&slot));
        let control = Arc::new(WorkerControl::default());
        self.request_controls
            .retain(|held| !held.done.load(Ordering::Acquire));
        self.request_controls.push(Arc::clone(&control));
        let sink = self.env.sink();
        let guard = sink.worker();
        // The worker owns its slot and control; it ends by itself when the exchange finishes,
        // times out or is cancelled.
        thread::spawn(move || {
            let _guard = guard;
            let outcome = perform(&parsed, &request, &control);
            let completion = match &outcome {
                Ok(_) => Outcome::Ok,
                Err(failure) => Outcome::Failed(failure.clone()),
            };
            let (lock, ready) = &*slot;
            if let Ok(mut held) = lock.lock() {
                *held = Some(outcome);
                ready.notify_all();
            }
            control.done.store(true, Ordering::Release);
            if !control.cancel.load(Ordering::Acquire) {
                sink.send(completion_event(id, completion));
            }
        });
        Ok(id)
    }

    /// A request that is still in flight is waited for up to two seconds, then reported as not ready.
    fn take_response(&mut self, request: RequestId) -> Option<HttpResponse> {
        let slot = self.requests.get(&request.0).cloned()?;
        let (lock, ready) = &*slot;
        let guard = lock.lock().ok()?;
        let (mut guard, _) = ready
            .wait_timeout_while(guard, self.patience, |held| held.is_none())
            .ok()?;
        let outcome = guard.take()?;
        drop(guard);
        self.requests.remove(&request.0);
        outcome.ok()
    }

    fn open_stream(&mut self, request: StreamRequest) -> Result<StreamId, PluginError> {
        if request.max_chunk_bytes == 0 || request.max_chunk_bytes > MAX_CHUNK_BYTES {
            return Err(error(
                ErrorCode::InvalidArgument,
                "max chunk must be 1 to 65536 bytes",
            ));
        }
        let parsed = check_request(&request.request, &self.env)?;
        if self.open_streams() >= STREAMS_PER_PLUGIN_LIMIT {
            return Err(error(
                ErrorCode::QuotaExceeded,
                "4 streams are already open",
            ));
        }
        self.streams.retain(|_, entry| entry.is_open());
        self.next_stream += 1;
        let stream = StreamId(self.next_stream);
        let control = Arc::new(WorkerControl::default());
        let worker_control = Arc::clone(&control);
        let sink = self.env.sink();
        let guard = sink.worker();
        let worker = thread::spawn(move || {
            let _guard = guard;
            run_stream(parsed, request, stream, worker_control, sink);
        });
        self.streams.insert(
            stream.0,
            StreamEntry {
                control,
                worker: Some(worker),
            },
        );
        Ok(stream)
    }

    fn close_stream(&mut self, stream: StreamId) {
        // Dropping the entry cancels the worker and waits for it; an unknown id is ignored.
        self.streams.remove(&stream.0);
    }

    /// The `websocket` feature is unstable and off unless the kernel enables it.
    fn open_websocket(
        &mut self,
        url: String,
        _protocols: Vec<String>,
    ) -> Result<StreamId, PluginError> {
        parse_url(&url, &["ws", "wss"])?;
        Err(error(
            ErrorCode::Unsupported,
            "feature websocket is not enabled",
        ))
    }

    fn send_websocket(&mut self, stream: StreamId, _text: String) -> Result<(), PluginError> {
        let is_open = self
            .streams
            .get(&stream.0)
            .is_some_and(StreamEntry::is_open);
        if is_open {
            Err(error(ErrorCode::Unsupported, "stream is not a websocket"))
        } else {
            Err(error(ErrorCode::NotFound, "no open stream has this id"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    use crate::testing::imports_fixture::ImportsFixture;
    use ilium_plugin_api::generated::host::{HttpRequest, StreamFraming, StreamRequest};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::{Header, Outcome, RequestId};
    use ilium_plugin_api::Tier;

    /// Default `timeout_millis` for requests that are not testing the timeout.
    const DEFAULT_TIMEOUT_MILLIS: u32 = 5_000;

    /// Largest and smallest accepted `timeout_millis` (api-http, Limits).
    const MAX_TIMEOUT_MILLIS: u32 = 300_000;

    /// Largest URL accepted by `fetch`, in bytes (api-http, Limits).
    const MAX_URL_BYTES: usize = 8 * 1024;

    /// Largest number of headers accepted by `fetch` (api-http, Limits).
    const MAX_HEADERS: usize = 64;

    /// Largest request body accepted by `fetch`, in bytes (SEC-082, API-062).
    const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

    /// Pending requests a plugin may hold before `busy` (ABI-013).
    const PENDING_REQUEST_LIMIT: usize = 64;

    /// Open streams a plugin may hold before `quota-exceeded` (api-http-streams, Limits).
    const STREAMS_PER_PLUGIN_LIMIT: usize = 4;

    /// Largest `max_chunk_bytes` accepted by `open_stream` (api-http-streams, WIT excerpt).
    const MAX_CHUNK_BYTES: u32 = 64 * 1024;

    /// How long a test waits for a completion event before failing.
    const COMPLETION_WAIT: Duration = Duration::from_secs(5);

    /// A minimal well-formed HTTP/1.1 response served by the loopback test server.
    const OK_RESPONSE: &[u8] =
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

    /// Binds a loopback listener on a free port. The caller keeps it alive while requests run.
    fn bind_loopback() -> (TcpListener, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
        let port = listener.local_addr().expect("listener address").port();
        (listener, port)
    }

    /// A T2 fixture granting the loopback port, plus any extra host grants.
    fn fixture_granting(port: u16, extra_grants: &[&str]) -> ImportsFixture {
        let loopback_grant = format!("http:127.0.0.1:{port}");
        let mut grants: Vec<&str> = extra_grants.to_vec();
        grants.push(loopback_grant.as_str());
        ImportsFixture::builder()
            .tier(Tier::T2)
            .grants(&grants)
            .build()
    }

    /// A `GET` request with the default timeout and no headers or body.
    fn get(url: String) -> HttpRequest {
        HttpRequest {
            method: "GET".to_owned(),
            url,
            headers: Vec::new(),
            body: None,
            timeout_millis: DEFAULT_TIMEOUT_MILLIS,
        }
    }

    /// A plain-HTTP URL on the loopback test server.
    fn loopback_url(port: u16, path: &str) -> String {
        format!("http://127.0.0.1:{port}{path}")
    }

    /// Unwraps a successful result, failing with the error code otherwise.
    fn expect_ok<T: std::fmt::Debug>(result: Result<T, PluginError>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected success, got {:?} ({})", error.code, error.message),
        }
    }

    /// Asserts that the call failed with the expected error code.
    fn expect_code<T: std::fmt::Debug>(result: Result<T, PluginError>, expected: ErrorCode) {
        match result {
            Err(error) => assert_eq!(error.code, expected, "message: {}", error.message),
            Ok(value) => panic!("expected {expected:?}, got Ok({value:?})"),
        }
    }

    /// Accepts one connection, reads the request head and answers with `pieces`, pausing between
    /// them so the client sees them in separate reads.
    fn serve_in_pieces(
        listener: TcpListener,
        pieces: &'static [&'static [u8]],
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                for piece in pieces {
                    let _ = stream.write_all(piece);
                    let _ = stream.flush();
                    thread::sleep(Duration::from_millis(20));
                }
            }
        })
    }

    /// A reader that hands out one byte per call, so every boundary falls between reads.
    struct ByteAtATime(std::io::Cursor<Vec<u8>>);

    impl Read for ByteAtATime {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let end = buffer.len().min(1);
            self.0.read(&mut buffer[..end])
        }
    }

    fn decode_chunked(wire: &[u8]) -> std::io::Result<Vec<u8>> {
        let mut decoded = Vec::new();
        super::ChunkedBody::new(ByteAtATime(std::io::Cursor::new(wire.to_vec())))
            .read_to_end(&mut decoded)?;
        Ok(decoded)
    }

    #[test]
    fn a_chunked_body_split_across_reads_is_decoded() {
        let decoded =
            decode_chunked(b"4\r\nWiki\r\n5\r\npedia\r\nE\r\n in\r\n\r\nchunks.\r\n0\r\n\r\n")
                .expect("decoded");
        assert_eq!(decoded, b"Wikipedia in\r\n\r\nchunks.".to_vec());
    }

    #[test]
    fn chunk_extensions_and_trailers_are_dropped() {
        let decoded = decode_chunked(
            b"3;name=value\r\nabc\r\n0;last\r\nExpires: never\r\nX-Trace: 1\r\n\r\n",
        )
        .expect("decoded");
        assert_eq!(decoded, b"abc".to_vec());
    }

    #[test]
    fn a_malformed_chunk_size_is_invalid_data() {
        let failure = decode_chunked(b"zz\r\nabc\r\n0\r\n\r\n").expect_err("malformed");
        assert_eq!(failure.kind(), std::io::ErrorKind::InvalidData);
        let too_long = decode_chunked(b"2\r\nabc\r\n0\r\n\r\n").expect_err("overlong chunk");
        assert_eq!(too_long.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_chunked_body_cut_inside_a_chunk_is_unexpected_eof() {
        let failure = decode_chunked(b"a\r\nabc").expect_err("truncated");
        assert_eq!(failure.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn the_size_limit_counts_decoded_bytes() {
        let mut decoded = Vec::new();
        super::ChunkedBody::new(std::io::Cursor::new(
            b"5\r\nhello\r\n5\r\nworld\r\n0\r\n\r\n".to_vec(),
        ))
        .take(7)
        .read_to_end(&mut decoded)
        .expect("decoded");
        assert_eq!(decoded, b"hellowo".to_vec());
    }

    #[test]
    fn fetch_decodes_a_chunked_response_from_the_network() {
        let (listener, port) = bind_loopback();
        let _responder = serve_in_pieces(
            listener,
            &[
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhel",
                b"lo\r\n6;ext=1\r\n world\r",
                b"\n0\r\nX-Trailer: yes\r\n\r\n",
            ],
        );
        let mut fixture = fixture_granting(port, &[]);
        let id = expect_ok(fixture.http().fetch(get(loopback_url(port, "/chunked"))));
        let outcome = fixture.wait_outcome(id, COMPLETION_WAIT);
        assert!(matches!(outcome, Some(Outcome::Ok)), "got {outcome:?}");
        let response = fixture.http().take_response(id).expect("response");
        assert_eq!(response.body, b"hello world".to_vec());
    }

    #[test]
    fn fetch_fails_on_a_malformed_chunked_response() {
        let (listener, port) = bind_loopback();
        let _responder = serve_once(
            listener,
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nnot-hex\r\nabc\r\n0\r\n\r\n",
        );
        let mut fixture = fixture_granting(port, &[]);
        let id = expect_ok(fixture.http().fetch(get(loopback_url(port, "/bad"))));
        let outcome = fixture.wait_outcome(id, COMPLETION_WAIT);
        match outcome {
            Some(Outcome::Failed(failure)) => assert_eq!(failure.code, ErrorCode::Network),
            other => panic!("expected failed(network), got {other:?}"),
        }
    }

    /// Accepts one connection, reads the request head and answers with `response`.
    fn serve_once(listener: TcpListener, response: &'static [u8]) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(response);
            }
        })
    }

    /// A URL of exactly `length` bytes on the loopback server, padded with `a` in the query.
    fn url_of_length(port: u16, length: usize) -> String {
        let prefix = loopback_url(port, "/?q=");
        let padding = length - prefix.len();
        format!("{prefix}{}", "a".repeat(padding))
    }

    /// REQ: ABI-013
    #[test]
    fn fetch_granted_loopback_get_returns_request_id() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let id = expect_ok(fixture.http().fetch(get(loopback_url(port, "/"))));
        assert!(
            id.0 > 0,
            "request id should be a fresh non-zero id, got {id:?}"
        );
    }

    /// REQ: API-062
    #[test]
    fn fetch_empty_body_and_no_headers_are_valid() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.body = Some(Vec::new());
        request.headers = Vec::new();
        let _ = expect_ok(fixture.http().fetch(request));
    }

    /// REQ: API-062
    #[test]
    fn fetch_body_at_exactly_8_mib_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.method = "POST".to_owned();
        request.body = Some(vec![0u8; MAX_BODY_BYTES]);
        let _ = expect_ok(fixture.http().fetch(request));
    }

    /// REQ: API-062
    #[test]
    fn fetch_body_one_byte_over_8_mib_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.method = "POST".to_owned();
        request.body = Some(vec![0u8; MAX_BODY_BYTES + 1]);
        expect_code(fixture.http().fetch(request), ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn fetch_url_of_exactly_8_kib_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let url = url_of_length(port, MAX_URL_BYTES);
        assert_eq!(url.len(), MAX_URL_BYTES);
        let _ = expect_ok(fixture.http().fetch(get(url)));
    }

    /// REQ: API-062
    #[test]
    fn fetch_url_one_byte_over_8_kib_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let url = url_of_length(port, MAX_URL_BYTES + 1);
        assert_eq!(url.len(), MAX_URL_BYTES + 1);
        expect_code(fixture.http().fetch(get(url)), ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn fetch_empty_url_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        expect_code(
            fixture.http().fetch(get(String::new())),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-062
    #[test]
    fn fetch_unparsable_url_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &["http:api.example.com"]);
        expect_code(
            fixture.http().fetch(get("not a url".to_owned())),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: SEC-077
    #[test]
    fn fetch_plain_http_to_public_host_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &["http:api.example.com"]);
        expect_code(
            fixture
                .http()
                .fetch(get("http://api.example.com/v1/status".to_owned())),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: SEC-079
    #[test]
    fn fetch_forbidden_host_header_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.headers = vec![Header {
            name: "Host".to_owned(),
            value: "api.example.com".to_owned(),
        }];
        expect_code(fixture.http().fetch(request), ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn fetch_exactly_64_headers_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.headers = (0..MAX_HEADERS)
            .map(|index| Header {
                name: format!("x-test-{index}"),
                value: "v".to_owned(),
            })
            .collect();
        let _ = expect_ok(fixture.http().fetch(request));
    }

    /// REQ: API-062
    #[test]
    fn fetch_65_headers_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.headers = (0..=MAX_HEADERS)
            .map(|index| Header {
                name: format!("x-test-{index}"),
                value: "v".to_owned(),
            })
            .collect();
        expect_code(fixture.http().fetch(request), ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn fetch_timeout_of_1_ms_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.timeout_millis = 1;
        let _ = expect_ok(fixture.http().fetch(request));
    }

    /// REQ: API-062
    #[test]
    fn fetch_timeout_of_300_000_ms_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.timeout_millis = MAX_TIMEOUT_MILLIS;
        let _ = expect_ok(fixture.http().fetch(request));
    }

    /// REQ: API-062
    #[test]
    fn fetch_timeout_of_0_ms_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.timeout_millis = 0;
        expect_code(fixture.http().fetch(request), ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn fetch_timeout_over_300_000_ms_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.timeout_millis = MAX_TIMEOUT_MILLIS + 1;
        expect_code(fixture.http().fetch(request), ErrorCode::InvalidArgument);
    }

    /// REQ: SEC-013
    #[test]
    fn fetch_host_without_grant_is_not_permitted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        expect_code(
            fixture
                .http()
                .fetch(get("https://api.example.com/v1/status".to_owned())),
            ErrorCode::NotPermitted,
        );
    }

    /// REQ: ABI-013
    #[test]
    fn fetch_65th_pending_request_is_busy() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        for _ in 0..PENDING_REQUEST_LIMIT {
            let mut request = get(loopback_url(port, "/"));
            request.timeout_millis = MAX_TIMEOUT_MILLIS;
            let _ = expect_ok(fixture.http().fetch(request));
        }
        let mut request = get(loopback_url(port, "/"));
        request.timeout_millis = MAX_TIMEOUT_MILLIS;
        expect_code(fixture.http().fetch(request), ErrorCode::Busy);
    }

    /// REQ: ABI-041
    #[test]
    fn take_response_for_unknown_request_id_returns_none() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        assert!(fixture.http().take_response(RequestId(u64::MAX)).is_none());
    }

    /// REQ: ABI-041
    #[test]
    fn take_response_returns_completed_response_once() {
        let (listener, port) = bind_loopback();
        let _responder = serve_once(listener, OK_RESPONSE);
        let mut fixture = fixture_granting(port, &[]);
        let id = expect_ok(fixture.http().fetch(get(loopback_url(port, "/"))));
        let outcome = fixture.wait_outcome(id, COMPLETION_WAIT);
        assert!(
            matches!(outcome, Some(Outcome::Ok)),
            "expected the request to complete ok, got {outcome:?}"
        );
        let response = fixture
            .http()
            .take_response(id)
            .expect("completed response should be held for one take");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok".to_vec());
        assert!(
            fixture.http().take_response(id).is_none(),
            "a second take of the same request must return none"
        );
    }

    /// REQ: ABI-041
    #[test]
    fn take_response_keeps_http_error_status_as_a_normal_response() {
        let (listener, port) = bind_loopback();
        let _responder = serve_once(
            listener,
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\nConnection: close\r\n\r\nno",
        );
        let mut fixture = fixture_granting(port, &[]);
        let id = expect_ok(fixture.http().fetch(get(loopback_url(port, "/missing"))));
        let outcome = fixture.wait_outcome(id, COMPLETION_WAIT);
        assert!(
            matches!(outcome, Some(Outcome::Ok)),
            "an HTTP 404 is not a transport failure, got {outcome:?}"
        );
        let response = fixture
            .http()
            .take_response(id)
            .expect("404 response should still be taken");
        assert_eq!(response.status, 404);
    }

    /// REQ: API-070
    #[test]
    fn open_stream_granted_raw_returns_stream_id() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let stream = StreamRequest {
            request: get(loopback_url(port, "/events")),
            framing: StreamFraming::Raw,
            max_chunk_bytes: 1024,
        };
        let _ = expect_ok(fixture.http().open_stream(stream));
    }

    /// REQ: API-070
    #[test]
    fn open_stream_max_chunk_of_64_kib_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let stream = StreamRequest {
            request: get(loopback_url(port, "/events")),
            framing: StreamFraming::Lines,
            max_chunk_bytes: MAX_CHUNK_BYTES,
        };
        let _ = expect_ok(fixture.http().open_stream(stream));
    }

    /// REQ: API-070
    #[test]
    fn open_stream_max_chunk_over_64_kib_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let stream = StreamRequest {
            request: get(loopback_url(port, "/events")),
            framing: StreamFraming::Lines,
            max_chunk_bytes: MAX_CHUNK_BYTES + 1,
        };
        expect_code(
            fixture.http().open_stream(stream),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: SEC-013
    #[test]
    fn open_stream_host_without_grant_is_not_permitted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let stream = StreamRequest {
            request: get("https://feed.example.com/v1/events".to_owned()),
            framing: StreamFraming::ServerSentEvents,
            max_chunk_bytes: 1024,
        };
        expect_code(fixture.http().open_stream(stream), ErrorCode::NotPermitted);
    }

    /// REQ: API-070
    #[test]
    fn open_stream_fifth_stream_is_quota_exceeded() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        for _ in 0..STREAMS_PER_PLUGIN_LIMIT {
            let stream = StreamRequest {
                request: get(loopback_url(port, "/events")),
                framing: StreamFraming::Raw,
                max_chunk_bytes: 1024,
            };
            let _ = expect_ok(fixture.http().open_stream(stream));
        }
        let stream = StreamRequest {
            request: get(loopback_url(port, "/events")),
            framing: StreamFraming::Raw,
            max_chunk_bytes: 1024,
        };
        expect_code(fixture.http().open_stream(stream), ErrorCode::QuotaExceeded);
    }

    /// REQ: API-070
    #[test]
    fn close_stream_frees_a_slot_for_a_new_stream() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut first_stream = None;
        for _ in 0..STREAMS_PER_PLUGIN_LIMIT {
            let stream = StreamRequest {
                request: get(loopback_url(port, "/events")),
                framing: StreamFraming::Raw,
                max_chunk_bytes: 1024,
            };
            let id = expect_ok(fixture.http().open_stream(stream));
            if first_stream.is_none() {
                first_stream = Some(id);
            }
        }
        let first_stream = first_stream.expect("the first stream id");
        fixture.http().close_stream(first_stream);
        let stream = StreamRequest {
            request: get(loopback_url(port, "/events")),
            framing: StreamFraming::Raw,
            max_chunk_bytes: 1024,
        };
        let _ = expect_ok(fixture.http().open_stream(stream));
    }
}
