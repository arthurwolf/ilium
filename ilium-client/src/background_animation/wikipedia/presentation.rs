//! One article owner shared by the workspace field and its Settings preview.
//! Layout preparation has one process-wide admission slot. Cancellation leaves
//! that slot occupied until the old CPU worker actually exits.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use ilium_ambient::resources::{AmbientResources, Stored, WorkerCost};
use ilium_ambient::source::Worker;
use ilium_execution::{RejectReason, StorageAdmission};
use ilium_wikipedia::Document;

use super::render::{RenderCell, RenderError, WikipediaRenderer};
use super::{RenderMode, WikipediaRuntime, WikipediaSettings};

static PREPARATION_ACTIVE: AtomicBool = AtomicBool::new(false);
const MIB: usize = 1024 * 1024;
const LAYOUT_WORKER_BYTES: usize = 192 * MIB;
const LAYOUT_RESULT_BYTES: usize = 96 * MIB;

/// The renderer's bounded layout, font state, and raster strips outlive its
/// worker. Keep both its output charge and source-document lease with it.
struct PreparedRenderer {
    renderer: WikipediaRenderer,
    _article: Arc<Stored<Arc<Document>>>,
    _storage: Arc<StorageAdmission>,
}

struct PreparationPermit;

impl Drop for PreparationPermit {
    fn drop(&mut self) {
        PREPARATION_ACTIVE.store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PreparationKey {
    document_generation: u64,
    columns: u16,
    mode: RenderMode,
    zoom: u16,
}

impl PreparationKey {
    fn new(generation: u64, columns: u16, settings: &WikipediaSettings) -> Self {
        Self {
            document_generation: generation,
            columns,
            mode: settings.render_mode,
            zoom: if settings.render_mode == RenderMode::Braille {
                settings.zoom_percent
            } else {
                100
            },
        }
    }
}

struct Preparation {
    key: PreparationKey,
    receiver: Receiver<Result<PreparedRenderer, RenderError>>,
    worker: Option<Worker>,
}

impl Preparation {
    fn start(
        key: PreparationKey,
        article: Arc<Stored<Arc<Document>>>,
        settings: WikipediaSettings,
        resources: &AmbientResources,
    ) -> Result<Option<Self>, String> {
        let reservation = match resources.reserve_worker(WorkerCost {
            threads: 1,
            resident_bytes: LAYOUT_WORKER_BYTES,
        }) {
            Ok(reservation) => reservation,
            Err(RejectReason::Busy | RejectReason::WorkerLimit | RejectReason::WorkerBytes) => {
                return Ok(None);
            }
            Err(reason) => return Err(format!("Wikipedia layout admission: {reason:?}")),
        };
        let storage = match resources.reserve_storage(LAYOUT_RESULT_BYTES) {
            Ok(storage) => storage,
            Err(RejectReason::Busy | RejectReason::ResultBytes) => return Ok(None),
            Err(reason) => return Err(format!("Wikipedia layout storage admission: {reason:?}")),
        };
        if PREPARATION_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(None);
        }
        let permit = PreparationPermit;
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = Worker::start_admitted("wikipedia-layout", reservation, move |stop| {
            let _permit = permit;
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::Lowest,
            );
            let mut renderer = WikipediaRenderer::new();
            let result = renderer
                .prepare_cancellable(article.view().clone(), key.columns, &settings, &stop)
                .map(|_| PreparedRenderer {
                    renderer,
                    _article: article,
                    _storage: storage,
                });
            if !stop.load(Ordering::Relaxed) {
                let _ = sender.try_send(result);
            }
        });
        match worker {
            Ok(worker) => Ok(Some(Self {
                key,
                receiver,
                worker: Some(worker),
            })),
            Err(error) => {
                PREPARATION_ACTIVE.store(false, Ordering::Release);
                Err(format!("Wikipedia layout worker: {error}"))
            }
        }
    }
}

impl Drop for Preparation {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}

#[derive(Default)]
pub(crate) struct WikipediaPresentation {
    resources: Option<AmbientResources>,
    runtime: WikipediaRuntime,
    renderer: Option<PreparedRenderer>,
    prepared: Option<PreparationKey>,
    pending: Option<Preparation>,
    failure: Option<(PreparationKey, String)>,
    frame_error: Option<String>,
    last_good_scroll: f64,
}

impl std::fmt::Debug for WikipediaPresentation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WikipediaPresentation")
            .field("prepared", &self.prepared)
            .field("pending", &self.pending.as_ref().map(|pending| pending.key))
            .field("failure", &self.failure)
            .finish_non_exhaustive()
    }
}

impl WikipediaPresentation {
    pub fn configure_resources(&mut self, resources: AmbientResources) {
        self.resources = Some(resources);
    }

    pub fn release(&mut self) {
        self.pending = None;
        self.renderer = None;
        self.prepared = None;
        self.failure = None;
        self.frame_error = None;
        self.last_good_scroll = 0.0;
        self.runtime.release();
    }

    pub fn cell(&self, x: u16, y: u16) -> Option<&RenderCell> {
        self.renderer.as_ref()?.renderer.cell(x, y)
    }

    pub fn status(&self) -> Option<String> {
        let source = self.runtime.status().unwrap_or("Loading Wikipedia");
        if let Some((_, error)) = &self.failure {
            return Some(format!("{error} · {source}"));
        }
        if let Some(error) = &self.frame_error {
            return Some(format!("{error} · {source}"));
        }
        if self.pending.is_some() || (self.runtime.document().is_some() && self.renderer.is_none())
        {
            return Some(format!("Preparing page layout · {source}"));
        }
        if let Some(stats) = self
            .renderer
            .as_ref()
            .and_then(|prepared| prepared.renderer.layout_stats())
        {
            if stats.missing_font_glyphs > 0 || stats.unavailable_images > 0 {
                return Some(format!(
                    "{source} · {} missing font glyphs · {} unavailable images",
                    stats.missing_font_glyphs, stats.unavailable_images
                ));
            }
        }
        Some(source.to_owned())
    }

    /// Polling the loader is nonblocking. A completed worker result is only
    /// published for the current document/geometry generation.
    pub fn render(
        &mut self,
        settings: &WikipediaSettings,
        columns: u16,
        rows: u16,
        elapsed: Duration,
        speed_percent: u16,
    ) {
        self.frame_error = None;
        self.runtime
            .poll(settings, elapsed, self.resources.as_ref());
        let Some(article) = self.runtime.loaded_document().cloned() else {
            self.pending = None;
            self.renderer = None;
            self.prepared = None;
            self.failure = None;
            return;
        };
        let document = article.view().clone();
        let key = PreparationKey::new(self.runtime.generation(), columns, settings);
        let has_current_renderer = self.prepared == Some(key);
        if !has_current_renderer {
            // Also run on the frame that publishes a completed worker result:
            // no wall time spent waiting for layout was visible to the reader.
            self.runtime.hold_clock(elapsed);
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.key != key)
        {
            self.pending = None;
        }
        if self
            .failure
            .as_ref()
            .is_some_and(|(failed, _)| *failed != key)
        {
            self.failure = None;
        }

        let outcome = self
            .pending
            .as_ref()
            .map(|pending| pending.receiver.try_recv());
        match outcome {
            Some(Ok(Ok(renderer))) => {
                self.pending = None;
                self.renderer = Some(renderer);
                self.prepared = Some(key);
            }
            Some(Ok(Err(RenderError::Cancelled))) => {
                self.pending = None;
            }
            Some(Ok(Err(error))) => {
                self.pending = None;
                self.failure = Some((key, error.to_string()));
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.pending = None;
                self.failure = Some((key, "Wikipedia layout worker disconnected".into()));
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        if self.prepared != Some(key) && self.pending.is_none() && self.failure.is_none() {
            if let Some(resources) = self.resources.as_ref() {
                match Preparation::start(key, article, *settings, resources) {
                    Ok(preparation) => self.pending = preparation,
                    Err(error) => self.failure = Some((key, error)),
                }
            } else {
                self.frame_error = Some("Wikipedia execution resources are not configured".into());
            }
        }
        let Some(prepared) = self.renderer.as_mut() else {
            return;
        };
        if self.prepared != Some(key) {
            if let Err(error) = prepared
                .renderer
                .render_retained(rows, self.last_good_scroll)
            {
                self.frame_error = Some(error.to_string());
            }
            return;
        }
        if let Err(error) = prepared.renderer.prepare(document, columns, settings) {
            self.renderer = None;
            self.prepared = None;
            self.failure = Some((key, error.to_string()));
            return;
        }
        let maximum_rows = f64::from(
            prepared
                .renderer
                .total_rows()
                .saturating_sub(u32::from(rows)),
        );
        if self
            .runtime
            .advance(settings, elapsed, maximum_rows, speed_percent)
        {
            // Keep the prior complete article visible until its replacement is
            // prepared and admitted on a later frame.
            return;
        }
        let scroll = self.runtime.scroll_position();
        if let Err(error) = prepared.renderer.render(rows, scroll) {
            self.frame_error = Some(error.to_string());
        } else {
            self.last_good_scroll = scroll;
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_document_for_test(
        &mut self,
        document: Arc<Document>,
        settings: &WikipediaSettings,
        columns: u16,
    ) {
        self.runtime
            .inject_document_for_test(Arc::clone(&document), settings);
        let key = PreparationKey::new(self.runtime.generation(), columns, settings);
        let mut renderer = WikipediaRenderer::new();
        renderer.prepare(document, columns, settings).unwrap();
        let resources = AmbientResources::new(crate::execution::test_client());
        let storage = resources.reserve_storage(1).expect("test renderer storage");
        let article = self
            .runtime
            .loaded_document()
            .cloned()
            .expect("injected test article");
        self.renderer = Some(PreparedRenderer {
            renderer,
            _article: article,
            _storage: storage,
        });
        self.prepared = Some(key);
        self.pending = None;
        self.failure = None;
        self.frame_error = None;
    }

    #[cfg(test)]
    pub(crate) fn layout_count_for_test(&self) -> Option<u64> {
        self.renderer
            .as_ref()
            .map(|renderer| renderer.renderer.work_stats().layouts)
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
    };
    use std::time::Instant;

    fn execution_fixture(
        worker_threads: usize,
        worker_bytes: usize,
        result_bytes: usize,
    ) -> (Execution, QuotaGroup, AmbientResources) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 2,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes,
            worker_threads,
            worker_bytes,
        });
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: None,
                    resident_bytes_per_thread: MIB,
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
                result_bytes,
            })
            .unwrap();
        let resources = AmbientResources::new(client);
        (execution, quota, resources)
    }

    #[test]
    fn preparation_and_failures_precede_long_source_provenance() {
        let settings = WikipediaSettings::default();
        let document = Arc::new(
            ilium_wikipedia::parse_article(
                "A long source title that exceeds the visible Settings value",
                "https://en.wikipedia.org/wiki/A_long_source_title",
                "2026-10-02",
                "<p>Semantic status fixture.</p>",
            )
            .unwrap(),
        );
        let mut host = WikipediaPresentation::default();
        host.runtime.inject_document_for_test(document, &settings);
        let pending = host.status().unwrap();
        assert!(pending.starts_with("Preparing page layout"));
        assert!(pending.contains("https://en.wikipedia.org/wiki/A_long_source_title"));
        let key = PreparationKey::new(host.runtime.generation(), 80, &settings);
        host.failure = Some((key, "Image memory limit".to_owned()));
        assert!(host.status().unwrap().starts_with("Image memory limit"));
        host.failure = None;
        host.frame_error = Some("Viewport rendering failed".to_owned());
        assert!(host
            .status()
            .unwrap()
            .starts_with("Viewport rendering failed"));
    }

    #[test]
    fn layout_worker_refusal_does_not_spawn_or_claim_the_active_slot() {
        let (mut execution, quota, resources) = execution_fixture(1, 2 * MIB, 1024);
        let article = resources.reserve_storage(1).unwrap();
        let document = Arc::new(
            ilium_wikipedia::parse_article(
                "Admission fixture",
                "https://en.wikipedia.org/wiki/Admission_fixture",
                "2026-10-02",
                "<p>Bounded layout fixture.</p>",
            )
            .unwrap(),
        );
        let stored = Arc::new(Stored::new(document, article));
        let settings = WikipediaSettings::default();
        let key = PreparationKey::new(1, 80, &settings);
        assert!(matches!(
            Preparation::start(key, stored, settings, &resources),
            Ok(None)
        ));
        assert_eq!(quota.snapshot().worker_threads, 1);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        let joined = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(joined.remaining_workers, 0);
    }

    #[test]
    fn layout_admission_refusal_keeps_the_last_good_article_frame_visible() {
        let (mut execution, quota, resources) = execution_fixture(2, 256 * MIB, 1024);
        let settings = WikipediaSettings {
            render_mode: RenderMode::Text,
            ..Default::default()
        };
        let document = Arc::new(
            ilium_wikipedia::parse_article(
                "Last good article",
                "https://en.wikipedia.org/wiki/Last_good_article",
                "2026-10-02",
                "<p>The previously admitted article remains visible.</p>",
            )
            .unwrap(),
        );
        let mut host = WikipediaPresentation::default();
        host.inject_document_for_test(document, &settings, 40);
        host.configure_resources(resources);
        host.render(&settings, 40, 10, Duration::ZERO, 100);
        let prior_row: Vec<_> = (0..40)
            .map(|column| host.cell(column, 0).unwrap().symbol().to_owned())
            .collect();

        // A width change can admit its worker but not the retained layout.
        // The UI must keep rendering the clipped prior frame and retry later.
        host.render(&settings, 41, 10, Duration::from_secs(1), 100);
        let retained_row: Vec<_> = (0..40)
            .map(|column| host.cell(column, 0).unwrap().symbol().to_owned())
            .collect();
        assert!(
            host.failure.is_none(),
            "storage pressure should defer layout"
        );
        assert!(
            host.pending.is_none(),
            "refused layout must not claim the active slot"
        );
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(host.layout_count_for_test(), Some(1));
        assert_eq!(retained_row, prior_row);
        host.release();
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        let joined = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(joined.remaining_workers, 0);
    }

    #[test]
    fn layout_result_keeps_its_storage_charge_after_worker_publication() {
        let (mut execution, quota, resources) = execution_fixture(2, 256 * MIB, 128 * MIB);
        let article_storage = resources.reserve_storage(1).unwrap();
        let document = Arc::new(
            ilium_wikipedia::parse_article(
                "Charged layout result",
                "https://en.wikipedia.org/wiki/Charged_layout_result",
                "2026-10-02",
                "<p>Worker output remains admitted after publication.</p>",
            )
            .unwrap(),
        );
        let article = Arc::new(Stored::new(document, article_storage));
        let settings = WikipediaSettings {
            render_mode: RenderMode::Text,
            ..Default::default()
        };
        let key = PreparationKey::new(1, 40, &settings);
        let preparation = Preparation::start(key, article.clone(), settings, &resources)
            .unwrap()
            .expect("layout worker admitted");
        let prepared = preparation
            .receiver
            .recv_timeout(Duration::from_secs(20))
            .expect("layout worker result")
            .expect("layout preparation");
        assert_eq!(quota.snapshot().result_bytes, 1 + LAYOUT_RESULT_BYTES);
        drop(prepared);
        assert_eq!(quota.snapshot().result_bytes, 1);
        drop(article);
        assert_eq!(quota.snapshot().result_bytes, 0);
        drop(preparation);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        let joined = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(joined.remaining_workers, 0);
    }
}
