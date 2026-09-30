//! Shared plumbing for scenes that fetch data or run helper processes.
//!
//! Every network fetch goes through `http_get`: a proper User-Agent, bounded
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
    pub fn spawn(name: &str, task: impl FnOnce(Arc<AtomicBool>) + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name(format!("ilium-ambient-{name}"))
            .spawn(move || task(thread_stop))
            .ok();
        Self { stop, handle }
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
        std::thread::sleep(Duration::from_millis(100).min(deadline - Instant::now()));
    }
    !stop.load(Ordering::Relaxed)
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("only https:// URLs are fetched: {0}")]
    NotHttps(String),
    #[error("request failed: {0}")]
    Request(String),
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

fn wait_for_host_slot(url: &str) {
    let table = LAST_REQUEST.get_or_init(Default::default);
    let host = host_of(url);
    let wait = {
        let mut table = table
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        let ready_at = table
            .get(&host)
            .map_or(now, |last| (*last + MIN_HOST_SPACING).max(now));
        table.insert(host, ready_at);
        ready_at.saturating_duration_since(now)
    };
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
}

/// Blocking GET for use from worker threads only, never from `Scene::render`.
pub fn http_get(url: &str, max_bytes: usize, timeout: Duration) -> Result<Vec<u8>, FetchError> {
    if !url.starts_with("https://") {
        return Err(FetchError::NotHttps(url.to_owned()));
    }
    wait_for_host_slot(url);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(USER_AGENT)
        .build()
        .into();
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| FetchError::Request(error.to_string()))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| FetchError::Request(error.to_string()))?;
    if bytes.len() > max_bytes {
        return Err(FetchError::TooLarge(max_bytes));
    }
    Ok(bytes)
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
