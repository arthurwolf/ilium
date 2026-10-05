//! One bounded encoded clip, exposed to ffmpeg through seekable loopback HTTP.

use super::discover::MediaInput;
use super::http_range::select_range;
use super::series;
use crate::resources::{AmbientResources, Stored, WorkerCost};
use crate::source::{sleep_unless_stopped, Worker, USER_AGENT};
use sha2::{Digest, Sha256};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const MAX_MEDIA_BYTES: usize = 64 * 1024 * 1024;
const MAX_BODIES: usize = 2;
static ACTIVE_BODIES: AtomicUsize = AtomicUsize::new(0);

struct BodyAdmission;
impl BodyAdmission {
    fn acquire() -> Result<Self, String> {
        ACTIVE_BODIES
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_BODIES).then_some(count + 1)
            })
            .map(|_| Self)
            .map_err(|_| "Previous RAM video is still stopping; retry shortly".into())
    }
}
impl Drop for BodyAdmission {
    fn drop(&mut self) {
        ACTIVE_BODIES.fetch_sub(1, Ordering::AcqRel);
    }
}

struct MediaBody {
    bytes: Stored<Vec<u8>>,
    _admission: BodyAdmission,
}

pub(super) struct RamMedia {
    pub source: MediaInput,
    pub endpoint: MediaInput,
    _worker: Worker,
}

#[derive(Default)]
struct ProviderGate {
    next: Option<Instant>,
    unrepresentable_cooldown: bool,
}

fn provider_slot() -> &'static Mutex<ProviderGate> {
    static SLOT: OnceLock<Mutex<ProviderGate>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(ProviderGate::default()))
}

fn wait_for_provider(stop: &AtomicBool) -> Result<(), String> {
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("Download cancelled".into());
        }
        let delay = {
            let mut slot = provider_slot()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let now = Instant::now();
            if slot.unrepresentable_cooldown {
                return Err(
                    "Provider requested a cooldown beyond the supported clock range".into(),
                );
            }
            match slot.next {
                Some(next) if next > now => {
                    next.duration_since(now).min(Duration::from_millis(100))
                }
                _ => {
                    slot.next = Some(now + Duration::from_secs(32));
                    return Ok(());
                }
            }
        };
        if !sleep_unless_stopped(stop, delay) {
            return Err("Download cancelled".into());
        }
    }
}

fn defer_provider(seconds: u64) {
    let mut slot = provider_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match Instant::now().checked_add(Duration::from_secs(seconds)) {
        Some(next) => slot.next = Some(slot.next.map_or(next, |current| current.max(next))),
        None => slot.unrepresentable_cooldown = true,
    }
}

fn collect_body(
    mut reader: impl Read,
    expected_bytes: usize,
    expected_sha256: &str,
    stop: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    if expected_bytes == 0 || expected_bytes > MAX_MEDIA_BYTES {
        return Err("Video source exceeds the RAM admission limit".into());
    }
    if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err("Download cancelled or timed out".into());
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(expected_bytes)
        .map_err(|_| "Not enough memory for video".to_owned())?;
    if bytes.capacity() > MAX_MEDIA_BYTES {
        return Err("Video allocation exceeds RAM limit".into());
    }
    let mut chunk = [0_u8; 8192];
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("Download cancelled".into());
        }
        if Instant::now() >= deadline {
            return Err("Video download timed out".into());
        }
        let remaining = expected_bytes.saturating_sub(bytes.len());
        let wanted = chunk.len().min(remaining.saturating_add(1));
        let read = match reader.read(&mut chunk[..wanted]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result.map_err(|error| format!("Video download read failed: {error}"))?,
        };
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err("Download cancelled or timed out".into());
        }
        if read > remaining {
            return Err("Video source exceeds recorded length".into());
        }
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err("Download cancelled or timed out".into());
    }
    if bytes.len() != expected_bytes || format!("{:x}", Sha256::digest(&bytes)) != expected_sha256 {
        return Err("Video source integrity check failed; catalogue refresh required".into());
    }
    Ok(bytes)
}

fn collect_body_admitted(
    reader: impl Read,
    expected_bytes: usize,
    expected_sha256: &str,
    stop: &AtomicBool,
    deadline: Instant,
    resources: &AmbientResources,
) -> Result<Stored<Vec<u8>>, String> {
    if expected_bytes == 0 || expected_bytes > MAX_MEDIA_BYTES {
        return Err("Video source exceeds the RAM admission limit".into());
    }
    let storage = resources
        .reserve_storage(MAX_MEDIA_BYTES)
        .map_err(|error| format!("Video RAM host admission rejected: {error:?}"))?;
    let bytes = collect_body(reader, expected_bytes, expected_sha256, stop, deadline)?;
    Ok(Stored::new(bytes, storage))
}

impl RamMedia {
    #[cfg(test)]
    pub(super) fn join_observer(&self) -> Option<ilium_platform::owned_worker::WorkerTicket> {
        self._worker.join_observer()
    }

    pub fn acquire(
        input: &MediaInput,
        stop: &AtomicBool,
        resources: &AmbientResources,
    ) -> Result<Self, String> {
        let entry = series::entry(input)?;
        let admission = BodyAdmission::acquire()?;
        wait_for_provider(stop)?;
        let MediaInput::Url(url) = input else {
            return Err("Expected a remote source".into());
        };
        // Disable automatic redirects: the catalogue pins exact HTTPS representations.
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(60)))
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_recv_response(Some(Duration::from_secs(10)))
            .timeout_recv_body(Some(Duration::from_secs(60)))
            .user_agent(USER_AGENT)
            .build();
        let agent = ilium_http::agent(config);
        let mut response = agent
            .get(url)
            .header("Accept-Encoding", "identity")
            .call()
            .map_err(|error| format!("Video download failed: {error}"))?;
        let status = response.status().as_u16();
        if status != 200 {
            let fallback = match status {
                403 => 600,
                429 => 120,
                500..=599 => 60,
                _ => 32,
            };
            let retry = response
                .headers()
                .get("retry-after")
                .and_then(|header| header.to_str().ok())
                .and_then(|text| {
                    text.parse::<u64>().ok().or_else(|| {
                        chrono::DateTime::parse_from_rfc2822(text).ok().map(|date| {
                            date.timestamp()
                                .saturating_sub(
                                    chrono::DateTime::<chrono::Utc>::from(
                                        std::time::SystemTime::now(),
                                    )
                                    .timestamp(),
                                )
                                .max(0) as u64
                        })
                    })
                })
                .unwrap_or(fallback);
            defer_provider(retry.max(fallback));
            return Err(format!(
                "Video source returned HTTP {status}; provider cooldown applied"
            ));
        }
        if response
            .headers()
            .get("content-length")
            .and_then(|header| header.to_str().ok())
            .and_then(|text| text.parse::<usize>().ok())
            .is_some_and(|length| length != entry.expected_download_bytes)
        {
            return Err("Video source length changed; catalogue refresh required".into());
        }
        let bytes = collect_body_admitted(
            response.body_mut().as_reader(),
            entry.expected_download_bytes,
            &entry.download_sha256,
            stop,
            Instant::now() + Duration::from_secs(60),
            resources,
        )?;
        Self::serve(
            input.clone(),
            MediaBody {
                bytes,
                _admission: admission,
            },
            resources,
        )
        .map_err(|error| format!("Cannot start RAM video endpoint: {error}"))
    }

    fn serve(
        source: MediaInput,
        body: MediaBody,
        resources: &AmbientResources,
    ) -> io::Result<Self> {
        let reservation = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: 8 * 1024 * 1024,
            })
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!("Video RAM endpoint host admission rejected: {error:?}"),
                )
            })?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let token = format!(
            "/{:016x}{:016x}",
            std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish(),
            std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish()
        );
        let endpoint = MediaInput::Url(format!(
            "http://127.0.0.1:{}{token}",
            listener.local_addr()?.port()
        ));
        let mut connections = Vec::new();
        connections
            .try_reserve_exact(MAX_CONNECTIONS)
            .map_err(io::Error::other)?;
        let worker = Worker::start_admitted("video-ram", reservation, move |stop| {
            while !stop.load(Ordering::Acquire) {
                for _ in 0..MAX_CONNECTIONS {
                    match listener.accept() {
                        Ok((stream, _)) if connections.len() < MAX_CONNECTIONS => {
                            if let Ok(connection) = Connection::new(stream) {
                                connections.push(connection);
                            }
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => return,
                    }
                }
                let mut index = 0;
                let mut made_progress = false;
                while index < connections.len() {
                    let finished = connections[index]
                        .advance(&token, body.bytes.view())
                        .unwrap_or(true);
                    made_progress |= connections[index].made_progress;
                    if finished {
                        connections.swap_remove(index);
                    } else {
                        index += 1;
                    }
                }
                if made_progress {
                    std::thread::yield_now();
                } else if !sleep_unless_stopped(&stop, Duration::from_millis(10)) {
                    break;
                }
            }
            drop(connections);
            // The admission follows the body through actual supervised thread exit.
            drop(body);
        })?;
        Ok(Self {
            source,
            endpoint,
            _worker: worker,
        })
    }
}

const MAX_CONNECTIONS: usize = 8;

struct Reply {
    header: Vec<u8>,
    header_sent: usize,
    body_start: usize,
    body_end: usize,
}

struct Connection {
    stream: TcpStream,
    header: [u8; 4096],
    received: usize,
    reply: Option<Reply>,
    deadline: Instant,
    made_progress: bool,
}

impl Connection {
    fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            header: [0; 4096],
            received: 0,
            reply: None,
            deadline: Instant::now() + Duration::from_secs(2),
            made_progress: false,
        })
    }

    // Each connection performs bounded work per round; backpressure cannot
    // prevent a subsequent probe or range seek from receiving its response.
    fn advance(&mut self, token: &str, bytes: &[u8]) -> io::Result<bool> {
        self.made_progress = false;
        if Instant::now() >= self.deadline {
            return Ok(true);
        }
        if self.reply.is_none() {
            if self.received == self.header.len() {
                return Ok(true);
            }
            match self.stream.read(&mut self.header[self.received..]) {
                Ok(0) => return Ok(true),
                Ok(count) => {
                    self.received += count;
                    self.made_progress = true;
                }
                Err(error) if transient(&error) => return Ok(false),
                Err(error) => return Err(error),
            }
            let Some(end) = self.header[..self.received]
                .windows(4)
                .position(|part| part == b"\r\n\r\n")
            else {
                return Ok(false);
            };
            self.reply = Some(reply_for(&self.header[..end + 4], token, bytes.len())?);
            self.deadline = Instant::now() + Duration::from_secs(10);
        }
        let Some(reply) = self.reply.as_mut() else {
            return Ok(false);
        };
        let pending = if reply.header_sent < reply.header.len() {
            &reply.header[reply.header_sent..]
        } else if reply.body_start < reply.body_end {
            &bytes[reply.body_start..reply.body_end.min(reply.body_start + 65536)]
        } else {
            return Ok(true);
        };
        match self.stream.write(pending) {
            Ok(0) => Ok(true),
            Ok(count) => {
                self.made_progress = true;
                if reply.header_sent < reply.header.len() {
                    reply.header_sent += count;
                } else {
                    reply.body_start += count;
                }
                self.deadline = Instant::now() + Duration::from_secs(10);
                Ok(false)
            }
            Err(error) if transient(&error) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

fn transient(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

fn reply_for(header: &[u8], token: &str, length: usize) -> io::Result<Reply> {
    let text = std::str::from_utf8(header).map_err(io::Error::other)?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default();
    let path = first.next().unwrap_or_default();
    let version = first.next().unwrap_or_default();
    let empty = |header: String| Reply {
        header: header.into_bytes(),
        header_sent: 0,
        body_start: 0,
        body_end: 0,
    };
    if !matches!(method, "GET" | "HEAD")
        || path != token
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || first.next().is_some()
    {
        return Ok(empty(
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        ));
    }
    let mut range = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return Err(io::Error::other("Malformed RAM request header"));
        };
        if name.eq_ignore_ascii_case("Range") {
            if range.is_some() {
                return Err(io::Error::other("Multiple RAM request ranges"));
            }
            range = Some(value.trim());
        }
    }
    let selected = match select_range(range, length) {
        Ok(selected) => selected,
        Err(_) => return Ok(empty(format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{length}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"))),
    };
    let status = if selected.partial {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    let content_range = if selected.partial {
        format!(
            "Content-Range: bytes {}-{}/{length}\r\n",
            selected.start,
            selected.end - 1
        )
    } else {
        String::new()
    };
    Ok(Reply {
        header: format!("HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\n{content_range}Connection: close\r\n\r\n", selected.end - selected.start).into_bytes(),
        header_sent: 0,
        body_start: selected.start,
        body_end: if method == "HEAD" { selected.start } else { selected.end },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Endpoint fixtures share the real process-wide two-body admission limit.
    // Concurrent tests must wait for a slot rather than assume one is free.
    fn acquire_fixture_admission() -> BodyAdmission {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(admission) = BodyAdmission::acquire() {
                return admission;
            }
            assert!(
                Instant::now() < deadline,
                "RAM fixture admission did not become available"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn collection_rejects_truncation_extra_bytes_changed_content_cancellation_and_oversize() {
        let sha = format!("{:x}", Sha256::digest(b"abc"));
        let stop = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(3);
        assert_eq!(
            collect_body(&b"abc"[..], 3, &sha, &stop, deadline).unwrap(),
            b"abc"
        );
        for source in [&b"ab"[..], &b"abcd"[..], &b"abd"[..]] {
            assert!(collect_body(source, 3, &sha, &stop, deadline).is_err());
        }
        assert!(collect_body(&b""[..], MAX_MEDIA_BYTES + 1, &sha, &stop, deadline).is_err());
        assert!(collect_body(&b"abc"[..], 3, &sha, &stop, Instant::now()).is_err());
        stop.store(true, Ordering::Release);
        assert!(collect_body(&b"abc"[..], 3, &sha, &stop, deadline).is_err());
    }

    #[test]
    fn host_storage_rejection_precedes_even_the_first_body_read() {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
        };
        struct MustNotRead;
        impl Read for MustNotRead {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("body read before host admission")
            }
        }
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 2,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
            worker_threads: 1,
            worker_bytes: 16 * 1024 * 1024,
        });
        let mut execution = Execution::start(
            quota,
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: None,
                    resident_bytes_per_thread: 1024 * 1024,
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
                jobs: 2,
                service_jobs: 0,
                input_bytes: 4096,
                result_bytes: 4096,
            })
            .unwrap();
        let resources = AmbientResources::new(client);
        let stop = AtomicBool::new(false);
        let error = collect_body_admitted(
            MustNotRead,
            3,
            "unused",
            &stop,
            Instant::now() + Duration::from_secs(1),
            &resources,
        )
        .unwrap_err();
        assert!(error.contains("host admission rejected"));
        drop(resources);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }

    fn request(endpoint: &MediaInput, request: &str) -> Vec<u8> {
        let MediaInput::Url(url) = endpoint else {
            panic!("test endpoint must be a URL");
        };
        let address = url
            .strip_prefix("http://")
            .unwrap()
            .split('/')
            .next()
            .unwrap();
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        response
    }

    #[test]
    fn ready_full_body_is_not_throttled_below_the_probe_deadline() {
        let length = 32 * 1024 * 1024;
        let media = RamMedia::serve(
            MediaInput::Url("https://example.invalid/synthetic-throughput-test".into()),
            MediaBody {
                bytes: Stored::new(
                    vec![7; length],
                    crate::resources::test_resources()
                        .reserve_storage(MAX_MEDIA_BYTES)
                        .unwrap(),
                ),
                _admission: acquire_fixture_admission(),
            },
            &crate::resources::test_resources(),
        )
        .unwrap();
        let MediaInput::Url(url) = &media.endpoint else {
            panic!("test endpoint must be a URL")
        };
        let path = format!("/{}", url.split('/').next_back().unwrap());
        let started = Instant::now();
        let response = request(
            &media.endpoint,
            &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        );
        let elapsed = started.elapsed();
        let split = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap();
        assert_eq!(response.len() - split - 4, length);
        assert!(response[split + 4..].iter().all(|byte| *byte == 7));
        let ticket = media.join_observer();
        drop(media);
        if let Some(ticket) = ticket {
            ticket
                .join_until(Instant::now() + Duration::from_secs(3))
                .unwrap();
        }
        assert!(
            elapsed < Duration::from_secs(3),
            "ready RAM body took {elapsed:?}, exceeds native probe deadline"
        );
    }

    #[test]
    fn blocked_reader_does_not_delay_a_second_range_request() {
        let media = RamMedia::serve(
            MediaInput::Url("https://example.invalid/synthetic-fairness-test".into()),
            MediaBody {
                bytes: Stored::new(
                    vec![7; 8 * 1024 * 1024],
                    crate::resources::test_resources()
                        .reserve_storage(MAX_MEDIA_BYTES)
                        .unwrap(),
                ),
                _admission: acquire_fixture_admission(),
            },
            &crate::resources::test_resources(),
        )
        .unwrap();
        let MediaInput::Url(url) = &media.endpoint else {
            panic!("test endpoint must be a URL");
        };
        let address = url
            .strip_prefix("http://")
            .unwrap()
            .split('/')
            .next()
            .unwrap();
        let path = format!("/{}", url.split('/').next_back().unwrap());
        let mut blocked = TcpStream::connect(address).unwrap();
        blocked
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        blocked
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .unwrap();
        let mut first_byte = [0; 1];
        blocked.read_exact(&mut first_byte).unwrap();
        assert_eq!(first_byte[0], b'H');
        let response = request(
            &media.endpoint,
            &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-2\r\n\r\n"),
        );
        let split = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap();
        assert!(response.starts_with(b"HTTP/1.1 206 Partial Content"));
        assert_eq!(&response[split + 4..], &[7, 7, 7]);
        drop(blocked);
        let ticket = media.join_observer();
        drop(media);
        if let Some(ticket) = ticket {
            ticket
                .join_until(Instant::now() + Duration::from_secs(3))
                .unwrap();
        }
    }

    #[test]
    fn real_loopback_endpoint_serves_exact_ranges_head_and_refuses_other_paths() {
        // Keep an unrelated admission alive through this fixture's cleanup.
        // A process-global count cannot establish this body's own lifetime.
        let unrelated_admission = acquire_fixture_admission();
        let storage = crate::resources::test_resources()
            .reserve_storage(MAX_MEDIA_BYTES)
            .unwrap();
        let observed_storage = std::sync::Arc::downgrade(&storage);
        let body = MediaBody {
            bytes: Stored::new(b"0123456789".to_vec(), storage),
            _admission: acquire_fixture_admission(),
        };
        let media = RamMedia::serve(
            MediaInput::Url("https://example.invalid/synthetic-test".into()),
            body,
            &crate::resources::test_resources(),
        )
        .unwrap();
        let MediaInput::Url(url) = &media.endpoint else {
            panic!("test endpoint must be a URL");
        };
        let path = format!("/{}", url.split('/').next_back().unwrap());
        for (header, status, expected) in [
            ("", "200 OK", "0123456789"),
            ("Range: bytes=2-4\r\n", "206 Partial Content", "234"),
            ("Range: bytes=7-\r\n", "206 Partial Content", "789"),
            ("Range: bytes=-3\r\n", "206 Partial Content", "789"),
            ("Range: bytes=10-\r\n", "416 Range Not Satisfiable", ""),
        ] {
            let response = request(
                &media.endpoint,
                &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{header}\r\n"),
            );
            let text = String::from_utf8(response).unwrap();
            let (headers, bytes) = text.split_once("\r\n\r\n").unwrap();
            assert!(
                headers.starts_with(&format!("HTTP/1.1 {status}")),
                "{headers}"
            );
            assert_eq!(bytes, expected);
            assert!(headers.contains(&format!("Content-Length: {}", expected.len())));
        }
        let head = String::from_utf8(request(
            &media.endpoint,
            &format!("HEAD {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        ))
        .unwrap();
        assert!(head.contains("Content-Length: 10\r\n"));
        assert!(head.ends_with("\r\n\r\n"));
        let rejected = String::from_utf8(request(
            &media.endpoint,
            "GET /wrong-token HTTP/1.1\r\nHost: localhost\r\n\r\n",
        ))
        .unwrap();
        assert!(rejected.starts_with("HTTP/1.1 404"));
        let ticket = media._worker.join_observer().unwrap();
        drop(media);
        ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .unwrap();
        assert!(
            observed_storage.upgrade().is_none(),
            "this RAM body's allocation credit survives the actual worker join"
        );
        drop(unrelated_admission);
    }
}
