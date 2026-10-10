//! The ordered background loader: discovers the image list and serves image
//! requests. Local reads and image decoding use the shared finite I/O and CPU
//! banks. Owned by the scene through `source::Worker`; Drop signals teardown
//! and physical joining remains with platform supervision.

use super::decode::{DecodeLimits, DecodedImage};
use super::discover::{
    discover_admitted, is_url_list_file, parse_url_list_text, url_name, MAX_URLS,
};
use super::mailbox::{Mailbox, Rejected};
use super::settings::{display_name, expand_home, ImageSource, ImagesMode, ImagesSettings};
use super::ImagesStartError;
use crate::resources::WorkerCost;
use crate::source::Worker;
use ilium_execution::Retention;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Cached downloads are reused for a month; a failed refresh falls back to
/// the stale copy (see `fetch_cached`).
const CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// Bounded so dropping the scene never waits long on a stuck download.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_MAX_BYTES: usize = 32 * 1024 * 1024;
const URL_LIST_MAX_BYTES: usize = 1024 * 1024;
const LOADER_RESIDENT_BYTES: usize = 2 * 1024 * 1024;

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

#[derive(Debug)]
pub struct LoaderConfig {
    pub list: ListSpec,
    /// Directory for downloaded files (`<env.cache_dir>/images`).
    pub cache_dir: PathBuf,
    pub limits: DecodeLimits,
    pub resources: crate::resources::AmbientResources,
    pub failure_fallback: Arc<crate::resources::Stored<String>>,
    // Last: the original configuration stays charged through actual worker exit.
    pub capture_storage: Arc<ilium_execution::StorageAdmission>,
}

#[derive(Debug, Clone, Copy)]
pub struct LoadRequest {
    pub index: usize,
    pub generation: u32,
    pub max_width: u32,
    pub max_height: u32,
}

pub(super) enum WorkerEvent {
    /// The discovered list (never empty; emptiness is `ListFailed`).
    List {
        list: super::list::SharedList,
    },
    ListFailed(Arc<crate::resources::Stored<String>>),
    Loaded {
        index: usize,
        generation: u32,
        image: Arc<DecodedImage>,
    },
    Failed {
        index: usize,
        generation: u32,
        message: Arc<crate::resources::Stored<String>>,
    },
}

/// Handle to the worker thread.
pub(super) struct ImageLoader {
    requests: Arc<Mailbox<LoadRequest, 8>>,
    events: Arc<Mailbox<WorkerEvent, 2>>,
    // Last: source::Worker transfers actual join supervision after mailbox
    // closure; callback-held Arcs keep original queued storage alive.
    _worker: Worker,
}

impl ImageLoader {
    pub(super) fn start(config: LoaderConfig) -> Result<Self, ImagesStartError> {
        let reservation = config.resources.reserve_worker(WorkerCost {
            threads: 1,
            resident_bytes: LOADER_RESIDENT_BYTES,
        })?;
        let request_sender = Mailbox::new(config.capture_storage.clone());
        let request_receiver = request_sender.clone();
        let event_receiver = Mailbox::new(config.capture_storage.clone());
        let event_sender = event_receiver.clone();
        let worker = Worker::start_admitted("images", reservation, move |stop| {
            // Covers normal return and unwinding: a retired native owner must
            // never continue accepting requests that nobody can consume.
            struct Retirement(Arc<Mailbox<LoadRequest, 8>>);
            impl Drop for Retirement {
                fn drop(&mut self) {
                    self.0.close();
                }
            }
            let _retirement = Retirement(request_receiver.clone());
            run(config, &stop, &request_receiver, &event_sender);
        })?;
        Ok(Self {
            requests: request_sender,
            events: event_receiver,
            _worker: worker,
        })
    }

    pub(super) fn request(&self, request: LoadRequest) -> Result<(), Rejected<LoadRequest>> {
        self.requests.try_send(request)
    }

    /// Non-blocking.
    pub(super) fn try_recv(&self) -> Option<WorkerEvent> {
        self.events.try_recv()
    }
}

impl Drop for ImageLoader {
    fn drop(&mut self) {
        self.requests.close();
        self.events.close();
    }
}

/// Both mailbox allocations are admitted by the shared constructor capture.
pub(super) fn mailbox_storage_bytes() -> usize {
    Mailbox::<LoadRequest, 8>::allocation_bytes() + Mailbox::<WorkerEvent, 2>::allocation_bytes()
}

fn run(
    config: LoaderConfig,
    stop: &Arc<AtomicBool>,
    requests: &Mailbox<LoadRequest, 8>,
    events: &Mailbox<WorkerEvent, 2>,
) {
    let _capture_storage = &config.capture_storage;
    let entries = match build_list(&config, stop) {
        Ok(built) => {
            let BuiltList {
                entries,
                notes,
                scan_retention,
            } = built;
            let original = super::list::DiscoveredList { entries, notes };
            let retained = super::list::retain(original, &config.resources, stop);
            drop(scan_retention);
            let list = match retained {
                Ok(list) => list,
                Err((reason, original)) => {
                    drop(original); // Explicit canceled/refused publication, never list success.
                    if !stop.load(Ordering::Acquire) {
                        tracing::warn!(?reason, "Images discovered-list publication refused");
                        let _ = events.send(
                            WorkerEvent::ListFailed(config.failure_fallback.clone()),
                            stop,
                        );
                    }
                    return;
                }
            };
            if events
                .send(WorkerEvent::List { list: list.clone() }, stop)
                .is_err()
            {
                return;
            }
            list
        }
        Err(message) => {
            let message = super::failure::retain(
                message,
                &config.resources,
                stop,
                &config.failure_fallback,
                None,
            );
            let _ = events.send(WorkerEvent::ListFailed(message), stop);
            return;
        }
    };
    while !stop.load(Ordering::Relaxed) {
        let Some(request) = requests.recv(stop) else {
            return;
        };
        let outcome = match entries.view().entries.get(request.index) {
            Some(entry) => load_entry(entry, &config, &request, stop),
            None => Err(LoadFailure::Legacy(
                "Image request index is absent from the discovered list".to_owned(),
            )),
        };
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
                message: match message {
                    LoadFailure::Legacy(message) => super::failure::retain(
                        message,
                        &config.resources,
                        stop,
                        &config.failure_fallback,
                        None,
                    ),
                    LoadFailure::Prepared(message) => message,
                },
            },
        };
        if events.send(event, stop).is_err() {
            return;
        }
    }
}

struct BuiltList {
    entries: Vec<Entry>,
    notes: Vec<String>,
    // Last: keep the finite scan result charged until list retention succeeds.
    scan_retention: Option<Retention>,
}

impl BuiltList {
    fn new(entries: Vec<Entry>, notes: Vec<String>) -> Self {
        Self {
            entries,
            notes,
            scan_retention: None,
        }
    }
}

type ListResult = Result<BuiltList, String>;
type UrlListResult = Result<(Vec<Entry>, Vec<String>), String>;

fn build_list(config: &LoaderConfig, stop: &Arc<AtomicBool>) -> ListResult {
    match &config.list {
        ListSpec::Single(ImageSource::Local(path)) => {
            if path.as_os_str().is_empty() {
                return Err("Choose an image file".to_owned());
            }
            Ok(BuiltList::new(
                vec![Entry::file(expand_home(&path.to_string_lossy()))],
                Vec::new(),
            ))
        }
        ListSpec::Single(source) => match source.url() {
            Some(url) if !url.is_empty() => {
                Ok(BuiltList::new(vec![Entry::url(url.to_owned())], Vec::new()))
            }
            _ => Err("Enter an image URL".to_owned()),
        },
        ListSpec::Folders { spec, recursive } => {
            if spec.trim().is_empty() {
                return Err("Enter one or more folders".to_owned());
            }
            let found = discover_admitted(
                spec,
                *recursive,
                &config.resources,
                stop,
                &config.capture_storage,
            )
            .map_err(|error| format!("Image folder scan failed: {error}"))?;
            let (found, retention) = found.into_parts();
            let mut notes = found.errors;
            if found.truncated {
                notes
                    .push("Image folder scan results were truncated by resource limits".to_owned());
            }
            if found.files.is_empty() {
                let reason = notes
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("No images found in {spec}"));
                return Err(reason);
            }
            Ok(BuiltList {
                entries: found.files.into_iter().map(Entry::file).collect(),
                notes,
                scan_retention: Some(retention),
            })
        }
        ListSpec::UrlList(text) => build_url_list(text, config, stop)
            .map(|(entries, notes)| BuiltList::new(entries, notes)),
    }
}

fn build_url_list(text: &str, config: &LoaderConfig, stop: &Arc<AtomicBool>) -> UrlListResult {
    let mut urls: Vec<String> = Vec::new();
    let mut notes = Vec::new();
    for item in super::discover::split_list(text) {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if !item.starts_with("https://") {
            notes.push(format!("Skipped non-https URL: {item}"));
        } else if is_url_list_file(item) {
            match super::remote_fetch::fetch(
                &config.cache_dir,
                item,
                "txt",
                Duration::from_secs(3600),
                URL_LIST_MAX_BYTES,
                DOWNLOAD_TIMEOUT,
                &config.resources,
                stop,
                &config.capture_storage,
            ) {
                Ok(fetched) => {
                    urls.extend(parse_url_list_text(&String::from_utf8_lossy(
                        fetched.bytes.view(),
                    )));
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

enum LoadFailure {
    Legacy(String),
    Prepared(Arc<crate::resources::Stored<String>>),
}
impl From<String> for LoadFailure {
    fn from(message: String) -> Self {
        Self::Legacy(message)
    }
}

fn load_entry(
    entry: &Entry,
    config: &LoaderConfig,
    request: &LoadRequest,
    stop: &Arc<AtomicBool>,
) -> Result<DecodedImage, LoadFailure> {
    let bytes = match &entry.source {
        EntrySource::File(path) => {
            let max_bytes = usize::try_from(config.limits.max_file_bytes)
                .map_err(|_| format!("{} file limit cannot be represented", entry.name))?;
            let stored = super::encoded::read_local(
                path,
                max_bytes,
                &config.resources,
                stop,
                &config.capture_storage,
            )
            .map_err(|error| match error {
                super::encoded::ReadError::File(crate::source::FileReadFailure::TooLarge(_)) => {
                    format!("{} is too large", entry.name)
                }
                error => format!("Cannot read {}: {error}", path.display()),
            })?;
            if stored.view().starts_with(b"BM") {
                return super::prepared::bmp(
                    stored,
                    &entry.name,
                    config.limits,
                    (request.max_width, request.max_height),
                    super::prepared::PreparationEnv {
                        resources: &config.resources,
                        stop,
                        capture_storage: &config.capture_storage,
                        emergency: &config.failure_fallback,
                    },
                )
                .map_err(LoadFailure::Prepared);
            }
            if stored.view().starts_with(b"\x89PNG\r\n\x1a\n") {
                return super::png_prepared::png(
                    stored,
                    &entry.name,
                    config.limits,
                    (request.max_width, request.max_height),
                    super::prepared::PreparationEnv {
                        resources: &config.resources,
                        stop,
                        capture_storage: &config.capture_storage,
                        emergency: &config.failure_fallback,
                    },
                )
                .map_err(LoadFailure::Prepared);
            }
            stored
        }
        EntrySource::Url(url) => {
            super::remote_fetch::fetch(
                &config.cache_dir,
                url,
                "img",
                CACHE_MAX_AGE,
                DOWNLOAD_MAX_BYTES,
                DOWNLOAD_TIMEOUT,
                &config.resources,
                stop,
                &config.capture_storage,
            )
            .map_err(|error| format!("Download failed for {}: {error}", entry.name))?
            .bytes
        }
    };
    super::generic_prepared::decode(
        bytes,
        &entry.name,
        config.limits,
        (request.max_width, request.max_height),
        super::prepared::PreparationEnv {
            resources: &config.resources,
            stop,
            capture_storage: &config.capture_storage,
            emergency: &config.failure_fallback,
        },
    )
    .map_err(LoadFailure::Prepared)
}

#[cfg(test)]
mod tests {
    use super::super::decode::fixtures::{jpeg_bytes, png_bytes};
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
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
        let capture_storage = crate::resources::test_resources()
            .reserve_storage(4096)
            .expect("explicit fixture configuration storage");
        LoaderConfig {
            list,
            cache_dir: cache,
            limits: DecodeLimits::default(),
            resources: crate::resources::test_resources(),
            failure_fallback: super::super::failure::fallback(capture_storage.clone()),
            capture_storage,
        }
    }

    fn constrained_resources() -> (Execution, crate::resources::AmbientResources) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 4,
            service_jobs: 0,
            input_bytes: 64 * 1024 * 1024,
            result_bytes: 64 * 1024 * 1024,
            worker_threads: 4,
            worker_bytes: 8 * 1024 * 1024,
        });
        let lane = LaneConfig {
            threads: 1,
            queue_slots: 2,
            priority: None,
            resident_bytes_per_thread: 1024,
        };
        let empty = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane,
                io: lane,
                service: empty,
            },
        )
        .expect("isolated finite image lanes");
        let finite = execution
            .client(ClientLimits {
                jobs: 4,
                service_jobs: 0,
                input_bytes: 64 * 1024 * 1024,
                result_bytes: 64 * 1024 * 1024,
            })
            .expect("isolated image client");
        (execution, crate::resources::AmbientResources::new(finite))
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
        ))
        .expect("real fixture loader start");
        let WorkerEvent::List { list } = wait_event(&loader) else {
            panic!("expected list");
        };
        assert_eq!(list.view().entries.len(), 2);
        assert_eq!(list.view().entries[0].name, "a.png");
        loader
            .request(LoadRequest {
                index: 0,
                generation: 3,
                max_width: 20,
                max_height: 20,
            })
            .expect("fixture request admitted");
        loader
            .request(LoadRequest {
                index: 1,
                generation: 3,
                max_width: 20,
                max_height: 20,
            })
            .expect("fixture request admitted");
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
                assert!(message.view().contains("bad.png"), "{}", message.view());
            }
            _ => panic!("expected failure"),
        }
    }

    #[test]
    fn generic_codec_decode_runs_on_the_shared_cpu_bank() {
        let root = tempfile::tempdir().expect("tempdir");
        let image_path = root.path().join("generic.jpg");
        std::fs::write(&image_path, jpeg_bytes(3840, 2160, |_, _| [80, 120, 160]))
            .expect("write 4K JPEG fixture");
        let loader = ImageLoader::start(config(
            ListSpec::Single(ImageSource::Local(image_path)),
            root.path().join("cache"),
        ))
        .expect("real fixture loader start");
        assert!(matches!(wait_event(&loader), WorkerEvent::List { .. }));
        loader
            .request(LoadRequest {
                index: 0,
                generation: 11,
                max_width: 20,
                max_height: 20,
            })
            .expect("fixture request admitted");

        match wait_event(&loader) {
            WorkerEvent::Loaded {
                index,
                generation,
                image,
            } => {
                assert_eq!((index, generation), (0, 11));
                assert_eq!((image.width, image.height), (20, 11));
                assert!(
                    image.decode_thread_name.starts_with("ilium-exec-cpu-"),
                    "decoded on {:?}",
                    image.decode_thread_name
                );
            }
            WorkerEvent::Failed { message, .. } => {
                panic!("generic codec decode failed: {}", message.view())
            }
            _ => panic!("expected decoded JPEG"),
        }
    }

    #[test]
    fn impossible_download_storage_capacity_fails_without_retrying_forever() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mut execution, resources) = constrained_resources();
        let capture_storage = resources
            .reserve_storage(4096)
            .expect("loader configuration storage");
        let loader = ImageLoader::start(LoaderConfig {
            list: ListSpec::Single(ImageSource::Url(
                "https://example.invalid/image.jpg".to_owned(),
            )),
            cache_dir: root.path().join("cache"),
            limits: DecodeLimits::default(),
            resources: resources.clone(),
            failure_fallback: super::super::failure::fallback(capture_storage.clone()),
            capture_storage,
        })
        .expect("loader fits within the small worker quota");
        assert!(matches!(wait_event(&loader), WorkerEvent::List { .. }));
        loader
            .request(LoadRequest {
                index: 0,
                generation: 1,
                max_width: 16,
                max_height: 16,
            })
            .expect("request admitted");
        match wait_event(&loader) {
            WorkerEvent::Failed { message, .. } => assert!(
                message
                    .view()
                    .contains("download storage exceeds available resource capacity"),
                "{}",
                message.view()
            ),
            _ => panic!("expected permanent storage-capacity refusal"),
        }
        drop(loader);
        execution.request_shutdown(ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .expect("finite image test lanes retire");
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
        ))
        .expect("real fixture loader start");
        match wait_event(&loader) {
            WorkerEvent::ListFailed(message) => assert!(message.view().contains("No images found")),
            _ => panic!("expected failure"),
        }
        let missing = root.path().join("missing.png");
        let loader = ImageLoader::start(config(
            ListSpec::Single(ImageSource::Local(missing)),
            root.path().join("cache"),
        ))
        .expect("real fixture loader start");
        assert!(matches!(wait_event(&loader), WorkerEvent::List { .. }));
        loader
            .request(LoadRequest {
                index: 0,
                generation: 0,
                max_width: 10,
                max_height: 10,
            })
            .expect("fixture request admitted");
        match wait_event(&loader) {
            WorkerEvent::Failed { message, .. } => assert!(message.view().contains("Cannot read")),
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
        ))
        .expect("real fixture loader start");
        let WorkerEvent::List { list } = wait_event(&loader) else {
            panic!("expected list");
        };
        assert_eq!(list.view().entries.len(), 1);
        assert_eq!(
            list.view().notes.len(),
            1,
            "the http URL is skipped with a note"
        );
        loader
            .request(LoadRequest {
                index: 0,
                generation: 0,
                max_width: 10,
                max_height: 10,
            })
            .expect("fixture request admitted");
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
        ))
        .expect("real fixture loader start");
        let started = Instant::now();
        drop(loader);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
