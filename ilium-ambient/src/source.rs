//! Shared plumbing for scenes that fetch data or run helper processes.
//!
//! Network reads use `http_get` or `http_stream_lines`: a proper User-Agent, bounded
//! size and time, and a process-wide minimum spacing per host so a scene can
//! never hammer a public service (keep the user's IP reputation clean).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const USER_AGENT: &str = concat!(
    "ilium/",
    env!("CARGO_PKG_VERSION"),
    " (terminal multiplexer ambient background; +https://github.com/arthurwolf/ilium)"
);

/// Default cache root: the platform cache directory for ilium.
pub fn default_cache_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "ilium")
        .map(|dirs| dirs.cache_dir().join("ambient"))
        .unwrap_or_else(|| std::env::temp_dir().join("ilium-ambient"))
}

/// An owned background thread. The closure receives a stop flag it must poll
/// between units of work. Dropping the worker raises the flag and joins the
/// thread, so a scene that owns a `Worker` cannot leak it.
pub struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// Transfer cleanup ownership away from the presentation thread. The
    /// caller must bound admission of tasks that can block indefinitely.
    pub fn stop_in_background(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let Some(handle) = self.handle.take() else {
            return;
        };
        static REAPER: OnceLock<std::sync::mpsc::Sender<JoinHandle<()>>> = OnceLock::new();
        let sender = REAPER.get_or_init(|| {
            let (sender, receiver) = std::sync::mpsc::channel::<JoinHandle<()>>();
            let result = std::thread::Builder::new()
                .name("ilium-ambient-reaper".into())
                .spawn(move || {
                    let mut pending: Vec<JoinHandle<()>> = Vec::new();
                    loop {
                        match receiver.recv_timeout(Duration::from_millis(25)) {
                            Ok(handle) => pending.push(handle),
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        }
                        let mut index = 0;
                        while index < pending.len() {
                            if pending[index].is_finished() {
                                let _ = pending.swap_remove(index).join();
                            } else {
                                index += 1;
                            }
                        }
                    }
                });
            if let Err(error) = result {
                tracing::warn!(%error, "ambient reaper could not start");
            }
            sender
        });
        if sender.send(handle).is_err() {
            tracing::warn!("ambient cleanup owner unavailable");
        }
    }

    pub fn spawn(name: &str, task: impl FnOnce(Arc<AtomicBool>) + Send + 'static) -> Self {
        Self::try_spawn(name, task).unwrap_or_else(|_| Self {
            stop: Arc::new(AtomicBool::new(false)),
            handle: None,
        })
    }

    /// Start an owned worker while preserving a spawn failure for callers
    /// that expose readiness or recovery status.
    pub fn try_spawn(
        name: &str,
        task: impl FnOnce(Arc<AtomicBool>) + Send + 'static,
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name(format!("ilium-ambient-{name}"))
            .spawn(move || task(thread_stop))?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    pub fn is_stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // Tasks poll the flag between short operations; a blocked HTTP
            // call is bounded by its timeout, so this join is bounded too.
            let _ = handle.join();
        }
    }
}

/// Sleep in small slices so a stop request is honoured within ~100 ms.
/// Returns `true` when the full duration elapsed, `false` when stopped.
pub fn sleep_unless_stopped(stop: &AtomicBool, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(
            Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    !stop.load(Ordering::Relaxed)
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("only https:// URLs are fetched: {0}")]
    NotHttps(String),
    #[error("request failed: {0}")]
    Request(String),
    #[error("HTTP status {0}")] // Preserve machine-readable status for provider retry policies.
    HttpStatus(u16), // Do not infer HTTP status by parsing an arbitrary error message.
    #[error("request cancelled")]
    Cancelled,
    #[error("request timeout, including host admission")]
    Timeout,
    #[error("response larger than {0} bytes")]
    TooLarge(usize),
}

static LAST_REQUEST: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
const MIN_HOST_SPACING: Duration = Duration::from_millis(250);

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn wait_for_host_slot(
    url: &str,
    deadline: Instant,
    stop: Option<&AtomicBool>,
) -> Result<Duration, FetchError> {
    let table = LAST_REQUEST.get_or_init(Default::default);
    let host = host_of(url);
    loop {
        if stop.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(FetchError::Cancelled);
        }
        let now = Instant::now();
        let remaining = deadline.saturating_duration_since(now);
        if remaining.is_zero() {
            return Err(FetchError::Timeout);
        }
        let wait = {
            let mut table = table
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let ready_at = table
                .get(&host)
                .map_or(now, |last| *last + MIN_HOST_SPACING);
            if ready_at <= now {
                // Reserve only an admitted request; cancelled waiters never build a future queue.
                table.insert(host.clone(), now);
                return Ok(remaining);
            }
            ready_at.saturating_duration_since(now)
        };
        std::thread::sleep(wait.min(remaining).min(Duration::from_millis(20)));
    }
}

/// Blocking GET for use from worker threads only, never from `Scene::render`.
pub fn http_get(url: &str, max_bytes: usize, timeout: Duration) -> Result<Vec<u8>, FetchError> {
    http_get_inner(url, max_bytes, timeout, None)
}

/// GET that prevents a stopped live worker from starting a queued request.
pub fn http_get_stoppable(
    url: &str,
    max_bytes: usize,
    timeout: Duration,
    stop: &AtomicBool,
) -> Result<Vec<u8>, FetchError> {
    http_get_inner(url, max_bytes, timeout, Some(stop))
}

fn http_get_inner(
    url: &str,
    max_bytes: usize,
    timeout: Duration,
    stop: Option<&AtomicBool>,
) -> Result<Vec<u8>, FetchError> {
    if !url.starts_with("https://") {
        return Err(FetchError::NotHttps(url.to_owned()));
    }
    let deadline = Instant::now() + timeout; // Admission and body collection share one deadline.
    let timeout = wait_for_host_slot(url, deadline, stop)?; // Only the remaining budget reaches ureq.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(USER_AGENT)
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(request_error)?; // Keep structured HTTP status.
    read_bounded_body(response.body_mut().as_reader(), max_bytes, deadline, stop)
    // Count decoded bytes and check cancellation between chunks.
} // End the worker-only HTTP request.

fn request_error(error: ureq::Error) -> FetchError {
    // Preserve typed status without changing successful HTTP behavior.
    match error {
        // ureq's documented error enum is non-exhaustive.
        ureq::Error::StatusCode(status) => FetchError::HttpStatus(status), // Provider policy can recognize 429 and 5xx exactly.
        ureq::Error::Timeout(_) => FetchError::Timeout, // Preserve timeout classification.
        error => FetchError::Request(error.to_string()), // Preserve other transport diagnostics.
    } // End transport-error conversion.
} // End typed conversion.
fn read_bounded_body(
    // Worker-only collection; this does not make blocked socket reads instantly cancellable.
    mut reader: impl Read,     // ureq supplies the decoded response reader.
    max_bytes: usize,          // Provider-specific decoded-byte limit.
    deadline: Instant,         // Includes the time spent waiting for host admission.
    stop: Option<&AtomicBool>, // Non-live callers retain their existing non-stoppable API.
) -> Result<Vec<u8>, FetchError> {
    // Oversize/cancelled data is never returned as a truncated success.
    let mut bytes = Vec::new(); // Do not preallocate an attacker-selected size.
    let mut chunk = [0_u8; 8192]; // Fixed-size collection scratch space.
    loop {
        // The socket's ureq global timeout still bounds a blocked individual read.
        if stop.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(FetchError::Cancelled);
        } // Check before reading.
        if Instant::now() >= deadline {
            return Err(FetchError::Timeout);
        } // Include body processing in the deadline.
        let remaining = max_bytes.saturating_sub(bytes.len()); // No subtraction underflow.
        let wanted = chunk.len().min(remaining.saturating_add(1)); // Read at most one byte beyond the hard bound.
        let read = match reader.read(&mut chunk[..wanted]) {
            // Read decoded bytes without changing interrupted-read semantics.
            Ok(read) => read, // Continue with the actual byte count.
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue, // Retry only after rechecking stop/deadline.
            Err(error) => return Err(FetchError::Request(error.to_string())), // Other read failures retain last-good caller state.
        }; // End the bounded read attempt.
        if stop.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(FetchError::Cancelled);
        } // Do not publish a just-cancelled EOF.
        if Instant::now() >= deadline {
            return Err(FetchError::Timeout);
        } // An over-deadline body is not a success.
        if read > remaining {
            return Err(FetchError::TooLarge(max_bytes));
        } // Reject instead of keeping the first max_bytes.
        if read == 0 {
            return Ok(bytes);
        } // Successful complete body receipt.
        bytes
            .try_reserve(read)
            .map_err(|_| FetchError::Request("response allocation failed".into()))?; // Fallible growth.
        bytes.extend_from_slice(&chunk[..read]); // Logical body length never exceeds max_bytes.
    } // End finite collection.
} // End bounded HTTP body reader.

/// Read an HTTPS NDJSON connection on an owned worker. The callback receives
/// complete bounded lines and returns false to stop. A finite connection
/// lifetime bounds cancellation during a blocked read; callers back off
/// before reconnecting and retain the last successfully decoded event.
pub fn http_stream_lines(
    url: &str,
    max_line_bytes: usize,
    timeout: Duration,
    stop: &AtomicBool,
    mut receive: impl FnMut(&[u8]) -> bool,
) -> Result<(), FetchError> {
    use std::io::BufRead;
    if !url.starts_with("https://") {
        return Err(FetchError::NotHttps(url.to_owned()));
    }
    let timeout = wait_for_host_slot(url, Instant::now() + timeout, Some(stop))?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(USER_AGENT)
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(request_error)?; // Preserve 429 for the stream owner's cooldown policy.
    let mut reader = std::io::BufReader::new(response.body_mut().as_reader());
    let limit = max_line_bytes.clamp(1, 1_048_576);
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(FetchError::Cancelled);
        }
        let mut line = Vec::new();
        let read = reader
            .by_ref()
            .take(limit as u64 + 1)
            .read_until(b'\n', &mut line)
            .map_err(|error| FetchError::Request(error.to_string()))?;
        if read == 0 {
            return Ok(());
        }
        if line.len() > limit {
            return Err(FetchError::TooLarge(limit));
        }
        if !receive(&line) {
            return Ok(());
        }
    }
}

/// Stable, filesystem-safe file name for a URL (FNV-1a hash + short suffix).
pub fn cache_file_name(url: &str, extension: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}.{extension}")
}

/// Fetch through an on-disk cache. A cached file younger than `max_age` is
/// returned without touching the network. Writes are atomic (temp + rename).
/// A stale cached file is returned when the network fails.
pub fn fetch_cached(
    cache_dir: &Path,
    url: &str,
    extension: &str,
    max_age: Duration,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Vec<u8>, FetchError> {
    let path = cache_dir.join(cache_file_name(url, extension));
    let cached_age = std::fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok());
    if let Some(age) = cached_age {
        if age <= max_age {
            if let Ok(bytes) = std::fs::read(&path) {
                return Ok(bytes);
            }
        }
    }
    match http_get(url, max_bytes, timeout) {
        Ok(bytes) => {
            if std::fs::create_dir_all(cache_dir).is_ok() {
                let temporary = path.with_extension(format!("{extension}.tmp"));
                if std::fs::write(&temporary, &bytes).is_ok() {
                    let _ = std::fs::rename(&temporary, &path);
                }
            }
            Ok(bytes)
        }
        Err(error) => match std::fs::read(&path) {
            Ok(bytes) => Ok(bytes),
            Err(_) => Err(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_worker_never_admits_a_request_or_reserves_a_future_slot() {
        let stop = AtomicBool::new(true);
        let url = "https://stopped-worker.invalid/data";
        assert!(matches!(
            http_get_stoppable(url, 10, Duration::from_secs(1), &stop),
            Err(FetchError::Cancelled)
        ));
        assert!(!LAST_REQUEST
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .contains_key(&host_of(url)));
    }

    #[test]
    fn host_admission_obeys_timeout_without_extending_the_queue() {
        let url = "https://admission-budget.invalid/data";
        let reserved = Instant::now() + Duration::from_secs(1);
        LAST_REQUEST
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(host_of(url), reserved);
        assert!(matches!(
            wait_for_host_slot(url, Instant::now() + Duration::from_millis(10), None),
            Err(FetchError::Timeout)
        ));
        assert_eq!(
            LAST_REQUEST
                .get_or_init(Default::default)
                .lock()
                .unwrap()
                .get(&host_of(url)),
            Some(&reserved)
        );
    }

    #[test]
    fn worker_drop_stops_and_joins_the_thread() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = Worker::spawn("test", move |stop| {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = sender.send(());
        });
        drop(worker);
        assert!(
            receiver.try_recv().is_ok(),
            "thread finished before drop returned"
        );
    }

    #[test]
    fn non_https_urls_are_refused_without_network() {
        assert!(matches!(
            http_get("http://example.com/x", 10, Duration::from_secs(1)),
            Err(FetchError::NotHttps(_))
        ));
    }

    #[test]
    fn cache_names_are_stable_and_distinct() {
        assert_eq!(
            cache_file_name("https://a/x", "png"),
            cache_file_name("https://a/x", "png")
        );
        assert_ne!(
            cache_file_name("https://a/x", "png"),
            cache_file_name("https://a/y", "png")
        );
    }

    #[test]
    fn fresh_cache_is_served_without_the_network() {
        let directory = tempfile::tempdir().unwrap();
        // Not https: any network attempt would fail, so success proves the cache path.
        let url = "https://example.invalid/tile.png";
        std::fs::write(
            directory.path().join(cache_file_name(url, "png")),
            b"cached",
        )
        .unwrap();
        let bytes = fetch_cached(
            directory.path(),
            url,
            "png",
            Duration::from_secs(60),
            100,
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(bytes, b"cached");
    }
}

#[cfg(test)] // Pure I/O fixtures; no real HTTP requests.
mod bounded_body_tests {
    // Test the same collector used by http_get_stoppable.
    use super::*; // Access typed errors and the internal bounded reader.
    #[test] // Exactly-at-limit bodies succeed; one additional decoded byte fails.
    fn body_bound_is_exact_and_not_a_truncation_rule() {
        // Cursor models the decoded reader, not compressed Content-Length.
        let deadline = Instant::now() + Duration::from_secs(5); // Finite fixture budget.
        assert_eq!(
            read_bounded_body(std::io::Cursor::new(b"abcd"), 4, deadline, None).unwrap(),
            b"abcd"
        ); // Complete exact-size body.
        assert!(matches!(
            read_bounded_body(std::io::Cursor::new(b"abcde"), 4, deadline, None),
            Err(FetchError::TooLarge(4))
        )); // No four-byte partial success.
        assert!(
            read_bounded_body(std::io::Cursor::new(b""), 0, deadline, None)
                .unwrap()
                .is_empty()
        ); // Legitimate empty zero-limit body.
        assert!(matches!(
            read_bounded_body(std::io::Cursor::new(b"x"), 0, deadline, None),
            Err(FetchError::TooLarge(0))
        )); // Zero is still an exact bound.
    } // End body-bound test.

    struct CancellingReader<'a>(&'a AtomicBool); // Raises cancellation during a read operation.
    impl Read for CancellingReader<'_> {
        // No blocking operations or threads in this fixture.
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            // Return one byte while cancelling.
            self.0.store(true, Ordering::Relaxed); // Simulate stop arriving during body receipt.
            if output.is_empty() {
                return Ok(0);
            } // Respect the Read contract.
            output[0] = b'x'; // A received byte must not cause cancelled publication.
            Ok(1) // The collector observes the flag immediately after this read.
        } // End synthetic read.
    } // End synthetic reader implementation.
    #[test] // Check cancellation both before and during collection, including expired budgets.
    fn body_collection_rejects_cancelled_and_expired_results() {
        // No latency or socket guarantees are inferred.
        let stop = AtomicBool::new(true); // Stop before the first read.
        let deadline = Instant::now() + Duration::from_secs(5); // Bounded fixture deadline.
        assert!(matches!(
            read_bounded_body(std::io::Cursor::new(b"ok"), 10, deadline, Some(&stop)),
            Err(FetchError::Cancelled)
        )); // Early stop.
        stop.store(false, Ordering::Relaxed); // Permit the synthetic first read.
        assert!(matches!(
            read_bounded_body(CancellingReader(&stop), 10, deadline, Some(&stop)),
            Err(FetchError::Cancelled)
        )); // Stop during read.
        assert!(matches!(
            read_bounded_body(std::io::Cursor::new(b"ok"), 10, Instant::now(), None),
            Err(FetchError::Timeout)
        )); // Expired admission/body budget.
    } // End cancellation test.
    #[test] // HTTP retries must not depend on human-readable error strings.
    fn preserves_http_status_codes() {
        // Construct the documented ureq error variant directly.
        assert!(matches!(
            request_error(ureq::Error::StatusCode(429)),
            FetchError::HttpStatus(429)
        )); // Rate-limit classification.
        assert!(matches!(
            request_error(ureq::Error::StatusCode(503)),
            FetchError::HttpStatus(503)
        )); // Temporary server failure classification.
    } // End typed-error test.
} // End transport fixtures.
