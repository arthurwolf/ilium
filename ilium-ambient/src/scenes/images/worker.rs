//! The background loader: discovers the image list, then loads and decodes
//! images on request. Owned by the scene through `source::Worker`, so
//! dropping the scene stops and joins the thread.

use super::decode::{decode_image, DecodeLimits, DecodedImage};
use super::discover::{discover_images, is_url_list_file, parse_url_list_text, url_name, MAX_URLS};
use super::settings::{display_name, expand_home, ImageSource, ImagesMode, ImagesSettings};
use crate::source::{fetch_cached, Worker};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

/// Cached downloads are reused for a month; a failed refresh falls back to
/// the stale copy (see `fetch_cached`).
const CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// Bounded so dropping the scene never waits long on a stuck download.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_MAX_BYTES: usize = 32 * 1024 * 1024;
const URL_LIST_MAX_BYTES: usize = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntrySource {
    File(PathBuf),
    Url(String),
}

/// One entry of the image list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub source: EntrySource,
    /// Short name for the status line.
    pub name: String,
}

impl Entry {
    pub fn file(path: PathBuf) -> Self {
        Self {
            name: display_name(&path),
            source: EntrySource::File(path),
        }
    }

    pub fn url(url: String) -> Self {
        Self {
            name: url_name(&url),
            source: EntrySource::Url(url),
        }
    }

    /// Stable text identity, used to derive per-image random motion.
    pub fn key(&self) -> String {
        match &self.source {
            EntrySource::File(path) => path.to_string_lossy().into_owned(),
            EntrySource::Url(url) => url.clone(),
        }
    }
}

/// What the worker should list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListSpec {
    Single(ImageSource),
    Folders { spec: String, recursive: bool },
    UrlList(String),
}

impl ListSpec {
    pub fn from_settings(settings: &ImagesSettings) -> Self {
        match settings.mode {
            ImagesMode::Single => Self::Single(settings.source.clone()),
            ImagesMode::Folders => Self::Folders {
                spec: settings.folders.clone(),
                recursive: settings.recursive,
            },
            ImagesMode::UrlList => Self::UrlList(settings.urls.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoaderConfig {
    pub list: ListSpec,
    /// Directory for downloaded files (`<env.cache_dir>/images`).
    pub cache_dir: PathBuf,
    pub limits: DecodeLimits,
}

#[derive(Debug, Clone, Copy)]
pub struct LoadRequest {
    pub index: usize,
    pub generation: u32,
    pub max_width: u32,
    pub max_height: u32,
}

pub enum WorkerEvent {
    /// The discovered list (never empty; emptiness is `ListFailed`).
    List {
        entries: Vec<Entry>,
        /// Non-fatal notes (unreadable folder, list truncated).
        notes: Vec<String>,
    },
    ListFailed(String),
    Loaded {
        index: usize,
        generation: u32,
        image: Arc<DecodedImage>,
    },
    Failed {
        index: usize,
        generation: u32,
        message: String,
    },
}

/// Handle to the worker thread.
pub struct ImageLoader {
    requests: Sender<LoadRequest>,
    events: Receiver<WorkerEvent>,
    // Declared last: dropping the loader stops and joins the thread after the
    // channels are gone.
    _worker: Worker,
}

impl ImageLoader {
    pub fn start(config: LoaderConfig) -> Self {
        let (request_sender, request_receiver) = channel();
        let (event_sender, event_receiver) = channel();
        let worker = Worker::spawn("images", move |stop| {
            run(config, &stop, &request_receiver, &event_sender);
        });
        Self {
            requests: request_sender,
            events: event_receiver,
            _worker: worker,
        }
    }

    pub fn request(&self, request: LoadRequest) {
        let _ = self.requests.send(request);
    }

    /// Non-blocking.
    pub fn try_recv(&self) -> Option<WorkerEvent> {
        self.events.try_recv().ok()
    }
}

fn run(
    config: LoaderConfig,
    stop: &AtomicBool,
    requests: &Receiver<LoadRequest>,
    events: &Sender<WorkerEvent>,
) {
    let entries = match build_list(&config, stop) {
        Ok((entries, notes)) => {
            if events
                .send(WorkerEvent::List {
                    entries: entries.clone(),
                    notes,
                })
                .is_err()
            {
                return;
            }
            entries
        }
        Err(message) => {
            let _ = events.send(WorkerEvent::ListFailed(message));
            return;
        }
    };
    while !stop.load(Ordering::Relaxed) {
        let request = match requests.recv_timeout(POLL_INTERVAL) {
            Ok(request) => request,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let Some(entry) = entries.get(request.index) else {
            continue;
        };
        let outcome = load_entry(entry, &config, &request);
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let event = match outcome {
            Ok(image) => WorkerEvent::Loaded {
                index: request.index,
                generation: request.generation,
                image: Arc::new(image),
            },
            Err(message) => WorkerEvent::Failed {
                index: request.index,
                generation: request.generation,
                message,
            },
        };
        if events.send(event).is_err() {
            return;
        }
    }
}

type ListResult = Result<(Vec<Entry>, Vec<String>), String>;

fn build_list(config: &LoaderConfig, stop: &AtomicBool) -> ListResult {
    match &config.list {
        ListSpec::Single(ImageSource::Local(path)) => {
            if path.as_os_str().is_empty() {
                return Err("Choose an image file".to_owned());
            }
            Ok((
                vec![Entry::file(expand_home(&path.to_string_lossy()))],
                Vec::new(),
            ))
        }
        ListSpec::Single(source) => match source.url() {
            Some(url) if !url.is_empty() => Ok((vec![Entry::url(url.to_owned())], Vec::new())),
            _ => Err("Enter an image URL".to_owned()),
        },
        ListSpec::Folders { spec, recursive } => {
            if spec.trim().is_empty() {
                return Err("Enter one or more folders".to_owned());
            }
            let found = discover_images(spec, *recursive, stop);
            let mut notes = found.errors;
            if found.truncated {
                notes.push("Only the first 20000 images are used".to_owned());
            }
            if found.files.is_empty() {
                let reason = notes
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("No images found in {spec}"));
                return Err(reason);
            }
            Ok((found.files.into_iter().map(Entry::file).collect(), notes))
        }
        ListSpec::UrlList(text) => build_url_list(text, config, stop),
    }
}

fn build_url_list(text: &str, config: &LoaderConfig, stop: &AtomicBool) -> ListResult {
    let mut urls: Vec<String> = Vec::new();
    let mut notes = Vec::new();
    for item in super::discover::split_list(text) {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if !item.starts_with("https://") {
            notes.push(format!("Skipped non-https URL: {item}"));
        } else if is_url_list_file(item) {
            match fetch_cached(
                &config.cache_dir,
                item,
                "txt",
                Duration::from_secs(3600),
                URL_LIST_MAX_BYTES,
                DOWNLOAD_TIMEOUT,
            ) {
                Ok(bytes) => {
                    urls.extend(parse_url_list_text(&String::from_utf8_lossy(&bytes)));
                }
                Err(error) => notes.push(format!("Cannot fetch list {item}: {error}")),
            }
        } else {
            urls.push(item.to_owned());
        }
    }
    let mut seen = std::collections::HashSet::new();
    urls.retain(|url| seen.insert(url.clone()));
    urls.truncate(MAX_URLS);
    if urls.is_empty() {
        return Err(notes
            .into_iter()
            .next()
            .unwrap_or_else(|| "No image URLs given".to_owned()));
    }
    Ok((urls.into_iter().map(Entry::url).collect(), notes))
}

fn load_entry(
    entry: &Entry,
    config: &LoaderConfig,
    request: &LoadRequest,
) -> Result<DecodedImage, String> {
    let bytes = match &entry.source {
        EntrySource::File(path) => {
            let length = std::fs::metadata(path)
                .map_err(|error| format!("Cannot read {}: {error}", path.display()))?
                .len();
            if length > config.limits.max_file_bytes {
                return Err(format!("{} is too large", entry.name));
            }
            std::fs::read(path)
                .map_err(|error| format!("Cannot read {}: {error}", path.display()))?
        }
        EntrySource::Url(url) => fetch_cached(
            &config.cache_dir,
            url,
            "img",
            CACHE_MAX_AGE,
            DOWNLOAD_MAX_BYTES,
            DOWNLOAD_TIMEOUT,
        )
        .map_err(|error| format!("Download failed for {}: {error}", entry.name))?,
    };
    decode_image(
        &bytes,
        &config.limits,
        request.max_width,
        request.max_height,
    )
    .map_err(|error| format!("{}: {error}", entry.name))
}

#[cfg(test)]
mod tests {
    use super::super::decode::fixtures::png_bytes;
    use super::*;
    use std::time::Instant;

    fn wait_event(loader: &ImageLoader) -> WorkerEvent {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(event) = loader.try_recv() {
                return event;
            }
            assert!(Instant::now() < deadline, "worker event timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn config(list: ListSpec, cache: PathBuf) -> LoaderConfig {
        LoaderConfig {
            list,
            cache_dir: cache,
            limits: DecodeLimits::default(),
        }
    }

    #[test]
    fn lists_then_loads_local_files_off_thread() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            root.path().join("a.png"),
            png_bytes(40, 20, |_, _| [200, 10, 10]),
        )
        .expect("write");
        std::fs::write(root.path().join("bad.png"), b"garbage").expect("write");
        let loader = ImageLoader::start(config(
            ListSpec::Folders {
                spec: root.path().to_string_lossy().into_owned(),
                recursive: true,
            },
            root.path().join("cache"),
        ));
        let WorkerEvent::List { entries, .. } = wait_event(&loader) else {
            panic!("expected list");
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "a.png");
        loader.request(LoadRequest {
            index: 0,
            generation: 3,
            max_width: 20,
            max_height: 20,
        });
        loader.request(LoadRequest {
            index: 1,
            generation: 3,
            max_width: 20,
            max_height: 20,
        });
        match wait_event(&loader) {
            WorkerEvent::Loaded {
                index,
                generation,
                image,
            } => {
                assert_eq!((index, generation), (0, 3));
                assert_eq!((image.width, image.height), (20, 10));
            }
            _ => panic!("expected image"),
        }
        match wait_event(&loader) {
            WorkerEvent::Failed { index, message, .. } => {
                assert_eq!(index, 1);
                assert!(message.contains("bad.png"), "{message}");
            }
            _ => panic!("expected failure"),
        }
    }

    #[test]
    fn empty_folder_and_missing_file_report_useful_errors() {
        let root = tempfile::tempdir().expect("tempdir");
        let loader = ImageLoader::start(config(
            ListSpec::Folders {
                spec: root.path().to_string_lossy().into_owned(),
                recursive: true,
            },
            root.path().join("cache"),
        ));
        match wait_event(&loader) {
            WorkerEvent::ListFailed(message) => assert!(message.contains("No images found")),
            _ => panic!("expected failure"),
        }
        let missing = root.path().join("missing.png");
        let loader = ImageLoader::start(config(
            ListSpec::Single(ImageSource::Local(missing)),
            root.path().join("cache"),
        ));
        assert!(matches!(wait_event(&loader), WorkerEvent::List { .. }));
        loader.request(LoadRequest {
            index: 0,
            generation: 0,
            max_width: 10,
            max_height: 10,
        });
        match wait_event(&loader) {
            WorkerEvent::Failed { message, .. } => assert!(message.contains("Cannot read")),
            _ => panic!("expected failure"),
        }
    }

    #[test]
    fn cached_downloads_are_used_without_the_network() {
        let root = tempfile::tempdir().expect("tempdir");
        let cache = root.path().join("cache");
        std::fs::create_dir_all(&cache).expect("mkdir");
        let url = "https://example.invalid/pic.png";
        std::fs::write(
            cache.join(crate::source::cache_file_name(url, "img")),
            png_bytes(10, 10, |_, _| [1, 2, 3]),
        )
        .expect("write");
        let loader = ImageLoader::start(config(
            ListSpec::UrlList(format!("{url};http://insecure.invalid/x.png")),
            cache,
        ));
        let WorkerEvent::List { entries, notes } = wait_event(&loader) else {
            panic!("expected list");
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(notes.len(), 1, "the http URL is skipped with a note");
        loader.request(LoadRequest {
            index: 0,
            generation: 0,
            max_width: 10,
            max_height: 10,
        });
        assert!(matches!(wait_event(&loader), WorkerEvent::Loaded { .. }));
    }

    #[test]
    fn dropping_the_loader_stops_the_worker_promptly() {
        let root = tempfile::tempdir().expect("tempdir");
        let loader = ImageLoader::start(config(
            ListSpec::Folders {
                spec: root.path().to_string_lossy().into_owned(),
                recursive: true,
            },
            root.path().join("cache"),
        ));
        let started = Instant::now();
        drop(loader);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
