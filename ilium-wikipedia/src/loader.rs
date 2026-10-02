use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Timelike, Utc};
use ilium_ambient::source::{sleep_unless_stopped, Worker};
use rand::seq::SliceRandom;

use crate::document::MAX_HTML_BYTES;
use crate::fetch::{cached_get, fetch_image, html_url, public_url, CachedBytes, MAX_TOTAL_PIXELS};
use crate::{main_page_titles, parse_article, Block, Document};

pub enum LoaderEvent {
    Loaded(Arc<Document>),
    Status(String),
    Failed(String),
}

/// One owned, low-priority worker with one queued request and four events.
/// The presentation thread never waits for HTTP or worker cleanup.
pub struct PageLoader {
    requests: SyncSender<()>,
    events: Receiver<LoaderEvent>,
    worker: Option<Worker>,
    pending: Arc<AtomicBool>,
}

impl PageLoader {
    pub fn start(cache_dir: PathBuf) -> Result<Self, String> {
        let (requests, receiver) = mpsc::sync_channel(1);
        let (sender, events) = mpsc::sync_channel(4);
        let pending = Arc::new(AtomicBool::new(false));
        let worker_pending = Arc::clone(&pending);
        let worker = Worker::try_spawn("wikipedia", move |stop| {
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::Lowest,
            );
            run_worker(&cache_dir, receiver, sender, worker_pending, &stop);
        })
        .map_err(|error| format!("Wikipedia worker: {error}"))?;
        Ok(Self {
            requests,
            events,
            worker: Some(worker),
            pending,
        })
    }

    /// Returns false while a request is queued/loading, or after worker failure.
    pub fn request_next(&self) -> bool {
        if self.pending.swap(true, Ordering::AcqRel) {
            return false;
        }
        if self.requests.try_send(()).is_err() {
            self.pending.store(false, Ordering::Release);
            return false;
        }
        true
    }

    pub fn try_recv(&self) -> Option<LoaderEvent> {
        self.events.try_recv().ok()
    }
}

impl Drop for PageLoader {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}

// Status messages are expendable under backpressure; Loaded/Failed remain
// owned until the UI drains them or cancellation/disconnection ends the job.
fn terminal_event(sender: &SyncSender<LoaderEvent>, mut event: LoaderEvent, stop: &AtomicBool) {
    loop {
        match sender.try_send(event) {
            Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => return,
            Err(mpsc::TrySendError::Full(returned)) => event = returned,
        }
        if !sleep_unless_stopped(stop, Duration::from_millis(100)) {
            return;
        }
    }
}

fn status(sender: &SyncSender<LoaderEvent>, message: impl Into<String>) {
    let _ = sender.try_send(LoaderEvent::Status(message.into()));
}

#[derive(Default)]
struct Rotation {
    date: String,
    titles: Vec<String>,
    previous: Option<String>,
    warnings: Vec<String>,
}

impl Rotation {
    fn replace(&mut self, date: String, mut titles: Vec<String>) {
        titles.shuffle(&mut rand::rng());
        self.date = date;
        self.titles = titles;
        self.avoid_repeat();
    }

    fn avoid_repeat(&mut self) {
        if self.titles.len() > 1 && self.titles.last() == self.previous.as_ref() {
            let last = self.titles.len() - 1;
            self.titles.swap(0, last);
        }
    }

    fn next(&mut self) -> Option<String> {
        let title = self.titles.pop()?;
        self.previous = Some(title.clone());
        Some(title)
    }
}

fn daily_titles(
    cache: &Path,
    sender: &SyncSender<LoaderEvent>,
) -> Result<(String, Vec<String>, Vec<String>), String> {
    let now = Utc::now();
    // A cache entry from before UTC midnight must refresh even if younger
    // than 24 hours. Wikipedia's daily panels use UTC dates.
    let max_age = Duration::from_secs(u64::from(now.num_seconds_from_midnight()) + 1);
    let main = cached_get(
        cache,
        &html_url("Main Page")?,
        "html",
        max_age,
        MAX_HTML_BYTES,
    )?;
    let mut warnings = Vec::new();
    let date = if main.stale {
        let cached_date = main
            .cached_at
            .map(|time| DateTime::<Utc>::from(time).format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "unknown date".into());
        let warning = format!("Offline: using cached Wikipedia Main Page from {cached_date}");
        status(sender, warning.clone());
        warnings.push(warning);
        cached_date
    } else {
        now.format("%Y-%m-%d").to_string()
    };
    let html = std::str::from_utf8(&main.bytes)
        .map_err(|error| format!("Wikipedia Main Page UTF-8: {error}"))?;
    Ok((date, main_page_titles(html)?, warnings))
}

fn parse_cached_article(
    article: &CachedBytes,
    title: &str,
    date: &str,
) -> Result<Document, String> {
    let html = std::str::from_utf8(&article.bytes)
        .map_err(|error| format!("Wikipedia article UTF-8: {error}"))?;
    let mut document = parse_article(title, &public_url(title)?, date, html)?;
    if article.stale {
        let cached_date = article
            .cached_at
            .map(|time| DateTime::<Utc>::from(time).format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "unknown date".into());
        document.warnings.push(format!(
            "Offline: using cached full Wikipedia article: {title} (cached {cached_date})"
        ));
    }
    Ok(document)
}

fn load_article(
    cache: &Path,
    title: &str,
    date: &str,
    sender: &SyncSender<LoaderEvent>,
    stop: &AtomicBool,
) -> Result<Document, String> {
    status(sender, format!("Loading Wikipedia: {title}"));
    let article = cached_get(
        cache,
        &html_url(title)?,
        "html",
        Duration::from_secs(86400),
        MAX_HTML_BYTES,
    )?;
    let mut document = parse_cached_article(&article, title, date)?;
    for warning in &document.warnings {
        status(sender, warning.clone());
    }
    let mut pixels = 0_u64;
    let mut missing_images = 0;
    let mut attempted = HashSet::new();
    let mut image_errors = Vec::new();
    let mut stale_images = 0;
    let references: Vec<_> = document
        .blocks
        .iter()
        .flat_map(|block| match block {
            Block::Image(reference) => vec![reference.clone()],
            Block::Table { images, .. } => images.iter().map(|image| image.image.clone()).collect(),
            _ => Vec::new(),
        })
        .collect();
    for reference in &references {
        if stop.load(Ordering::Relaxed) {
            return Err("Wikipedia loading cancelled".into());
        }
        if !attempted.insert(reference.url.clone()) {
            continue;
        }
        match fetch_image(cache, &reference.url) {
            Ok((image, stale)) => {
                pixels += u64::from(image.width()) * u64::from(image.height());
                if pixels > MAX_TOTAL_PIXELS {
                    return Err("Wikipedia article exceeds total decoded image safety limit; article was not truncated".into());
                }
                document.images.insert(reference.url.clone(), image);
                stale_images += usize::from(stale);
            }
            Err(error) => {
                missing_images += 1;
                if image_errors.len() < 3 {
                    let source: String = reference.url.chars().take(180).collect();
                    let error: String = error.chars().take(240).collect();
                    image_errors.push(format!("Wikimedia image unavailable: {source}: {error}"));
                }
            }
        }
    }
    if missing_images != 0 || stale_images != 0 {
        let warning = format!("Wikipedia images: {missing_images} unavailable, {stale_images} served from stale cache; captions retained");
        status(sender, warning.clone());
        document.warnings.push(warning);
    }
    document.warnings.extend(image_errors);
    Ok(document)
}

fn run_worker(
    cache: &Path,
    receiver: Receiver<()>,
    sender: SyncSender<LoaderEvent>,
    pending: Arc<AtomicBool>,
    stop: &AtomicBool,
) {
    let mut rotation = Rotation::default();
    let mut failures = 0_u32;
    while !stop.load(Ordering::Relaxed) {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let mut result = Err("Wikipedia article loading failed".into());
        for attempt in 0..3 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if attempt > 0
                && !sleep_unless_stopped(
                    stop,
                    Duration::from_secs((1_u64 << failures.min(5)).min(30)),
                )
            {
                return;
            }
            result = (|| {
                if rotation.date != Utc::now().format("%Y-%m-%d").to_string()
                    || rotation.titles.is_empty()
                {
                    let (date, titles, warnings) = daily_titles(cache, &sender)?;
                    rotation.replace(date, titles);
                    rotation.warnings = warnings;
                }
                let title = rotation
                    .next()
                    .ok_or("Wikipedia Main Page has no remaining article")?;
                let mut document = load_article(cache, &title, &rotation.date, &sender, stop)?;
                document.warnings.extend(rotation.warnings.clone());
                Ok(document)
            })();
            if result.is_ok() {
                failures = 0;
                break;
            }
            failures = failures.saturating_add(1);
        }
        let event = match result {
            Ok(document) => LoaderEvent::Loaded(Arc::new(document)),
            Err(error) => LoaderEvent::Failed(error),
        };
        terminal_event(&sender, event, stop);
        pending.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_rotation_exhausts_each_article_before_repeating() {
        let mut rotation = Rotation::default();
        let titles = vec!["a".into(), "b".into(), "c".into()];
        rotation.replace("today".into(), titles.clone());
        let mut visited = std::collections::HashSet::new();
        for _ in 0..3 {
            assert!(visited.insert(rotation.next().unwrap()));
        }
        assert!(rotation.next().is_none());
        let previous = rotation.previous.clone();
        rotation.replace("today".into(), titles);
        assert_ne!(rotation.next(), previous);
    }

    #[test]
    fn offline_warning_survives_status_event_backpressure() {
        let source = CachedBytes {
            bytes: b"<body><p>Actual cached article</p></body>".to_vec(),
            stale: true,
            cached_at: Some(std::time::UNIX_EPOCH),
        };
        let document = parse_cached_article(&source, "Cached", "2026-10-02").unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.send(LoaderEvent::Status("full".into())).unwrap();
        for warning in &document.warnings {
            status(&sender, warning.clone());
        }
        assert_eq!(document.warnings.len(), 1);
        assert!(document.warnings[0].contains("Offline"));
        assert!(document.warnings[0].contains("1970-01-01"));
        assert_eq!(document.date, "2026-10-02");
        assert!(
            matches!(receiver.try_recv(), Ok(LoaderEvent::Status(message)) if message == "full")
        );
    }

    #[test]
    fn terminal_backpressure_can_be_cancelled() {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.send(LoaderEvent::Status("full".into())).unwrap();
        let stop = AtomicBool::new(true);
        terminal_event(&sender, LoaderEvent::Failed("failure".into()), &stop);
        assert!(matches!(receiver.try_recv(), Ok(LoaderEvent::Status(_))));
        assert!(receiver.try_recv().is_err());
    }
}
