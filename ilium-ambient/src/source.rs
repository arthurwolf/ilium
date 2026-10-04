//! Shared plumbing for scenes that fetch data or run helper processes.
//!
//! Network reads use `http_get` or `http_stream_lines`: a proper User-Agent, bounded
//! size and time, and a process-wide minimum spacing per host so a scene can
//! never hammer a public service (keep the user's IP reputation clean).

use ilium_platform::owned_worker::{spawn_owned, OwnedWorker, StopToken, WorkerKind};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
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

/// A cancellable background owner. The platform supervisor retains each real
/// join handle through thread exit; dropping a scene never waits on native I/O.
pub struct Worker {
    stop: Arc<AtomicBool>,
    owner: Option<OwnedWorker>,
}

impl Worker {
    /// Start only with an already admitted host resource reservation. The wake
    /// closure is kept by platform supervision through real join and the last
    /// ticket; callback return and logical scene Drop cannot release it early.
    pub fn start_admitted(
        name: &str,
        reservation: crate::resources::WorkerReservation,
        task: impl FnOnce(Arc<AtomicBool>) + Send + 'static,
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let wake_stop = Arc::clone(&stop);
        let owner = spawn_owned(
            &format!("ilium-ambient-{name}"),
            WorkerKind::Cooperative,
            StopToken::default(),
            move || {
                let _physical = &reservation.physical;
                wake_stop.store(true, Ordering::Release);
            },
            move |_| task(thread_stop),
        )?;
        Ok(Self {
            stop,
            owner: Some(owner),
        })
    }
    /// Request cancellation and leave actual joining to the bounded platform
    /// supervisor. Live and retiring owners share its admission limit.
    pub fn stop_in_background(self) {
        drop(self);
    }

    pub fn spawn(name: &str, task: impl FnOnce(Arc<AtomicBool>) + Send + 'static) -> Self {
        Self::try_spawn(name, task).unwrap_or_else(|error| {
            tracing::warn!(%error, "ambient worker could not start");
            Self {
                stop: Arc::new(AtomicBool::new(true)),
                owner: None,
            }
        })
    }

    /// Preserve spawn/admission failure for callers exposing readiness.
    pub fn try_spawn(
        name: &str,
        task: impl FnOnce(Arc<AtomicBool>) + Send + 'static,
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let wake_stop = Arc::clone(&stop);
        let owner = spawn_owned(
            &format!("ilium-ambient-{name}"),
            WorkerKind::Cooperative,
            StopToken::default(),
            move || wake_stop.store(true, Ordering::Release),
            move |_| task(thread_stop),
        )?;
        Ok(Self {
            stop,
            owner: Some(owner),
        })
    }

    pub fn is_stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    #[cfg(test)]
    pub(crate) fn join_observer(&self) -> Option<ilium_platform::owned_worker::WorkerTicket> {
        self.owner.as_ref().map(OwnedWorker::ticket)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Dropping OwnedWorker requests cancellation. Its actual JoinHandle
        // remains in the bounded supervisor, including blocked callbacks.
        drop(self.owner.take());
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
    reader: impl Read,         // ureq supplies the decoded response reader.
    max_bytes: usize,          // Provider-specific decoded-byte limit.
    deadline: Instant,         // Includes the time spent waiting for host admission.
    stop: Option<&AtomicBool>, // Non-live callers retain their existing non-stoppable API.
) -> Result<Vec<u8>, FetchError> {
    read_bounded_stream(reader, max_bytes, Some(deadline), stop)
}

/// Read a complete local file using the same byte guard as cached/HTTP bodies.
/// A metadata length is not authoritative: growing files are checked on every
/// read and an extra byte is detected, never returned as truncated success.
pub fn read_bounded_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, FetchError> {
    let file = std::fs::File::open(path).map_err(|error| FetchError::Request(error.to_string()))?;
    read_bounded_stream(file, max_bytes, None, None)
}

/// Read an original file for a finite job, checking its owner/bank cancellation
/// before opening and before/after each read. A blocked OS read still retires
/// only when the OS returns; the existing IO bank retains its physical custody.
pub fn read_bounded_file_with_cancel(
    path: &Path,
    max_bytes: usize,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<u8>, FetchError> {
    if cancelled() {
        return Err(FetchError::Cancelled);
    }
    let file = std::fs::File::open(path).map_err(|error| FetchError::Request(error.to_string()))?;
    read_bounded_stream_with_cancel(file, max_bytes, None, cancelled)
}

fn read_bounded_stream(
    reader: impl Read,
    max_bytes: usize,
    deadline: Option<Instant>,
    stop: Option<&AtomicBool>,
) -> Result<Vec<u8>, FetchError> {
    read_bounded_stream_with_cancel(reader, max_bytes, deadline, || {
        stop.is_some_and(|flag| flag.load(Ordering::Relaxed))
    })
}

/// Scalar failures for the concrete File producer. No diagnostic String is
/// constructed inside a finite file-read callback. Generic/custom readers are
/// not covered: their io::Error may itself own arbitrary data.
#[derive(Debug)]
pub(crate) enum FileReadFailure {
    Io {
        kind: std::io::ErrorKind,
        raw_os_error: Option<i32>,
    },
    Cancelled,
    Timeout,
    TooLarge(usize),
    Allocation,
}

pub(crate) fn read_bounded_file_scalar(
    path: &Path,
    max_bytes: usize,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<u8>, FileReadFailure> {
    let scalar_io = |error: std::io::Error| FileReadFailure::Io {
        kind: error.kind(),
        raw_os_error: error.raw_os_error(),
    };
    if cancelled() {
        return Err(FileReadFailure::Cancelled);
    }
    let file = std::fs::File::open(path).map_err(scalar_io)?;
    read_bounded_stream_result(file, max_bytes, None, cancelled).map_err(|error| match error {
        ReadFailure::Io(error) => scalar_io(error),
        ReadFailure::Cancelled => FileReadFailure::Cancelled,
        ReadFailure::Timeout => FileReadFailure::Timeout,
        ReadFailure::TooLarge(limit) => FileReadFailure::TooLarge(limit),
        ReadFailure::Allocation => FileReadFailure::Allocation,
    })
}

enum ReadFailure {
    Io(std::io::Error),
    Cancelled,
    Timeout,
    TooLarge(usize),
    Allocation,
}

fn read_bounded_stream_with_cancel(
    reader: impl Read,
    max_bytes: usize,
    deadline: Option<Instant>,
    cancelled: impl FnMut() -> bool,
) -> Result<Vec<u8>, FetchError> {
    // Preserve the legacy HTTP/provider/custom-reader diagnostic boundary.
    read_bounded_stream_result(reader, max_bytes, deadline, cancelled).map_err(
        |error| match error {
            ReadFailure::Io(error) => FetchError::Request(error.to_string()),
            ReadFailure::Cancelled => FetchError::Cancelled,
            ReadFailure::Timeout => FetchError::Timeout,
            ReadFailure::TooLarge(limit) => FetchError::TooLarge(limit),
            ReadFailure::Allocation => FetchError::Request("response allocation failed".into()),
        },
    )
}

fn read_bounded_stream_result(
    mut reader: impl Read,
    max_bytes: usize,
    deadline: Option<Instant>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<u8>, ReadFailure> {
    // Oversize/cancelled data is never returned as a truncated success.
    let mut bytes = Vec::new(); // Do not preallocate an attacker-selected size.
    let mut chunk = [0_u8; 8192]; // Fixed-size collection scratch space.
    loop {
        // The socket's ureq global timeout still bounds a blocked individual read.
        if cancelled() {
            return Err(ReadFailure::Cancelled);
        } // Check before reading.
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(ReadFailure::Timeout);
        } // Include body processing in the deadline.
        let remaining = max_bytes.saturating_sub(bytes.len()); // No subtraction underflow.
        let wanted = chunk.len().min(remaining.saturating_add(1)); // Read at most one byte beyond the hard bound.
        let read = match reader.read(&mut chunk[..wanted]) {
            // Read decoded bytes without changing interrupted-read semantics.
            Ok(read) => read, // Continue with the actual byte count.
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue, // Retry only after rechecking stop/deadline.
            Err(error) => return Err(ReadFailure::Io(error)), // Other read failures retain last-good caller state.
        }; // End the bounded read attempt.
        if cancelled() {
            return Err(ReadFailure::Cancelled);
        } // Do not publish a just-cancelled EOF.
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(ReadFailure::Timeout);
        } // An over-deadline body is not a success.
        if read > remaining {
            return Err(ReadFailure::TooLarge(max_bytes));
        } // Reject instead of keeping the first max_bytes.
        if read == 0 {
            return Ok(bytes);
        } // Successful complete body receipt.
        let required = bytes.len() + read; // read <= remaining, so this cannot overflow.
        if required > bytes.capacity() {
            // Explicit requested capacity: short reads need not start with a
            // power of two. Clamp geometric growth BEFORE allocation instead
            // of assuming RawVec's amortized growth stays below next_power_of_two.
            let target = bytes
                .capacity()
                .max(8)
                .checked_mul(2)
                .unwrap_or(max_bytes)
                .max(required)
                .min(max_bytes);
            bytes
                .try_reserve_exact(target - bytes.len())
                .map_err(|_| ReadFailure::Allocation)?;
        }
        bytes.extend_from_slice(&chunk[..read]); // Logical body length never exceeds max_bytes.
    } // End finite collection.
} // End bounded complete stream reader.

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
            match read_bounded_file(&path, max_bytes) {
                Ok(bytes) => return Ok(bytes),
                Err(FetchError::TooLarge(limit)) => return Err(FetchError::TooLarge(limit)),
                Err(_) => {}
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
        Err(error) => match read_bounded_file(&path, max_bytes) {
            Ok(bytes) => Ok(bytes),
            Err(FetchError::TooLarge(limit)) => Err(FetchError::TooLarge(limit)),
            Err(_) => Err(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_collector_clamps_non_power_of_two_short_read_growth_before_allocation() {
        struct ShortFirstRead {
            inner: std::io::Cursor<Vec<u8>>,
            first: bool,
        }
        impl std::io::Read for ShortFirstRead {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                let count = if self.first {
                    self.first = false;
                    output.len().min(3)
                } else {
                    output.len()
                };
                std::io::Read::read(&mut self.inner, &mut output[..count])
            }
        }
        let original = vec![42; 32768];
        let mut reader = ShortFirstRead {
            inner: std::io::Cursor::new(original.clone()),
            first: true,
        };
        let bytes = match read_bounded_stream_result(&mut reader, original.len(), None, || false) {
            Ok(bytes) => bytes,
            Err(_) => panic!("bounded complete fixture rejected"),
        };
        assert_eq!(bytes, original);
        assert!(
            bytes.capacity() <= original.len(),
            "actual collector capacity escaped its admitted ceiling"
        );
    }

    #[test]
    fn scalar_file_failure_preserves_os_identity_and_legacy_diagnostic() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("missing-original");
        let original = std::fs::File::open(&path).unwrap_err();
        let legacy = read_bounded_file(&path, 8).unwrap_err();
        assert_eq!(legacy.to_string(), format!("request failed: {original}"));
        match read_bounded_file_scalar(&path, 8, || false) {
            Err(FileReadFailure::Io { kind, raw_os_error }) => {
                assert_eq!(kind, original.kind());
                assert_eq!(raw_os_error, original.raw_os_error());
            }
            other => panic!("wrong actual file failure: {other:?}"),
        }
        assert!(matches!(
            read_bounded_file_scalar(&path, 8, || true),
            Err(FileReadFailure::Cancelled)
        ));
        std::fs::write(&path, b"original").unwrap();
        assert!(matches!(
            read_bounded_file_scalar(&path, 7, || false),
            Err(FileReadFailure::TooLarge(7))
        ));
        assert_eq!(
            read_bounded_file_scalar(&path, 8, || false).unwrap(),
            b"original"
        );
    }

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
    fn worker_drop_requests_stop_and_supervisor_joins_the_thread() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = Worker::try_spawn("test", move |stop| {
            while !stop.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = sender.send(());
        })
        .unwrap();
        let ticket = worker.owner.as_ref().unwrap().ticket();
        drop(worker);
        assert_eq!(
            ticket
                .join_until(Instant::now() + Duration::from_secs(2))
                .unwrap(),
            ilium_platform::owned_worker::WorkerExit::Joined,
        );
        assert!(
            receiver.try_recv().is_ok(),
            "actual join follows callback completion"
        );
    }

    #[test]
    fn blocked_callback_cannot_block_scene_drop_and_remains_owned() {
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = Worker::try_spawn("blocked-drop-test", move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        })
        .unwrap();
        let stop = worker.stop_flag();
        let ticket = worker.owner.as_ref().unwrap().ticket();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (dropped_tx, dropped_rx) = std::sync::mpsc::channel();
        let dropper = std::thread::spawn(move || {
            drop(worker);
            dropped_tx.send(()).unwrap();
        });
        // Always release the blocked callback, even if the nonblocking-drop
        // assertion fails, so the fixture cannot leave a hung owned worker.
        let dropped = dropped_rx.recv_timeout(Duration::from_secs(2));
        let was_stopped = stop.load(Ordering::Acquire);
        let was_still_owned = ticket.exit().is_none();
        release_tx.send(()).unwrap();
        dropper.join().unwrap();
        assert!(dropped.is_ok(), "scene Drop waited for a blocked callback");
        assert!(was_stopped);
        assert!(was_still_owned, "blocked callback was reported joined");
        assert_eq!(
            ticket
                .join_until(Instant::now() + Duration::from_secs(2))
                .unwrap(),
            ilium_platform::owned_worker::WorkerExit::Joined
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
    fn fresh_and_stale_cache_reads_enforce_the_same_complete_file_bound() {
        let directory = tempfile::tempdir().unwrap();
        // A network path is deterministically rejected, without DNS/HTTP.
        let url = "http://bounded-cache.invalid/image";
        let path = directory.path().join(cache_file_name(url, "img"));
        std::fs::write(&path, b"abcd").unwrap();
        assert_eq!(
            fetch_cached(
                directory.path(),
                url,
                "img",
                Duration::from_secs(60),
                4,
                Duration::from_secs(1)
            )
            .unwrap(),
            b"abcd"
        );
        std::fs::write(&path, b"abcde").unwrap();
        assert!(matches!(
            fetch_cached(
                directory.path(),
                url,
                "img",
                Duration::from_secs(60),
                4,
                Duration::from_secs(1)
            ),
            Err(FetchError::TooLarge(4))
        ));
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
            .unwrap();
        assert!(matches!(
            fetch_cached(
                directory.path(),
                url,
                "img",
                Duration::ZERO,
                4,
                Duration::from_secs(1)
            ),
            Err(FetchError::TooLarge(4))
        ));
        std::fs::write(&path, b"abcd").unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
            .unwrap();
        assert_eq!(
            fetch_cached(
                directory.path(),
                url,
                "img",
                Duration::ZERO,
                4,
                Duration::from_secs(1)
            )
            .unwrap(),
            b"abcd"
        );
    }

    #[test]
    fn file_growth_after_the_first_read_is_refused_without_truncated_success() {
        use std::io::Write;
        struct GrowingFile {
            reader: std::fs::File,
            path: PathBuf,
            appended: bool,
        }
        impl Read for GrowingFile {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                let read = self.reader.read(output)?;
                if read != 0 && !self.appended {
                    std::fs::OpenOptions::new()
                        .append(true)
                        .open(&self.path)?
                        .write_all(b"e")?;
                    self.appended = true;
                }
                Ok(read)
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("growing.img");
        std::fs::write(&path, b"abcd").unwrap();
        let reader = GrowingFile {
            reader: std::fs::File::open(&path).unwrap(),
            path: path.clone(),
            appended: false,
        };
        assert!(matches!(
            read_bounded_stream(reader, 4, None, None),
            Err(FetchError::TooLarge(4))
        ));
        assert_eq!(std::fs::read(path).unwrap(), b"abcde");
    }

    #[test]
    fn finite_file_cancellation_prevents_open_and_post_read_publication() {
        struct CancellingReader<'a>(&'a AtomicBool);
        impl Read for CancellingReader<'_> {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                self.0.store(true, Ordering::Relaxed);
                if output.is_empty() {
                    return Ok(0);
                }
                output[0] = b'x';
                Ok(1)
            }
        }
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("never-opened");
        assert!(matches!(
            read_bounded_file_with_cancel(&missing, 4, || true),
            Err(FetchError::Cancelled)
        ));
        let stop = AtomicBool::new(false);
        assert!(matches!(
            read_bounded_stream_with_cancel(CancellingReader(&stop), 4, None, || stop
                .load(Ordering::Relaxed)),
            Err(FetchError::Cancelled)
        ));
        let path = root.path().join("complete");
        std::fs::write(&path, b"abcd").unwrap();
        assert_eq!(
            read_bounded_file_with_cancel(&path, 4, || false).unwrap(),
            b"abcd"
        );
    }

    #[test]
    fn bounded_file_handles_exact_zero_and_missing_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bounded.img");
        assert!(matches!(
            read_bounded_file(&path, 4),
            Err(FetchError::Request(_))
        ));
        std::fs::write(&path, b"").unwrap();
        assert!(read_bounded_file(&path, 0).unwrap().is_empty());
        std::fs::write(&path, b"x").unwrap();
        assert!(matches!(
            read_bounded_file(&path, 0),
            Err(FetchError::TooLarge(0))
        ));
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
