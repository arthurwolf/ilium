//! Owned article loading and page rotation; rendering stays a pure projection.
use std::sync::Arc;
use std::time::Duration;

use ilium_ambient::resources::{AmbientResources, Stored};
use ilium_wikipedia::{Document, LoaderEvent, PageLoader};

use super::{scroll::PageScroll, WikipediaSettings};

#[derive(Default)]
pub(crate) struct WikipediaRuntime {
    loader: Option<PageLoader>,
    document: Option<Arc<Stored<Arc<Document>>>>,
    prefetched: Option<Arc<Stored<Arc<Document>>>>,
    scroll: Option<PageScroll>,
    last_elapsed: Option<Duration>,
    is_clock_reset: bool,
    retry_after: Duration,
    pending: bool,
    generation: u64,
    loading_status: String,
    status: String,
}

impl std::fmt::Debug for WikipediaRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WikipediaRuntime")
            .field("status", &self.status)
            .field("pending", &self.pending)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl WikipediaRuntime {
    pub fn release(&mut self) {
        // PageLoader transfers bounded cancellation cleanup off the UI thread.
        *self = Self::default();
    }

    pub fn document(&self) -> Option<&Document> {
        self.document
            .as_ref()
            .map(|article| article.view().as_ref())
    }

    pub fn loaded_document(&self) -> Option<&Arc<Stored<Arc<Document>>>> {
        self.document.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn inject_document_for_test(
        &mut self,
        document: Arc<Document>,
        settings: &WikipediaSettings,
    ) {
        self.activate(document, settings, Duration::ZERO);
        self.refresh_status();
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn status(&self) -> Option<&str> {
        (!self.status.is_empty()).then_some(self.status.as_str())
    }

    pub fn scroll_position(&self) -> f64 {
        self.scroll.as_ref().map_or(0.0, PageScroll::position)
    }

    /// A hidden or unprepared article has no displayed reading time. Keep the
    /// position and both dwell counters, but establish the next frame's clock
    /// baseline so layout/admission/reflow waiting cannot skip unseen content.
    pub fn hold_clock(&mut self, elapsed: Duration) {
        if elapsed.is_zero() || self.last_elapsed.is_some_and(|previous| elapsed < previous) {
            self.is_clock_reset = true;
        }
        self.last_elapsed = Some(elapsed);
    }

    /// Called only while a real background or deliberate preview is visible.
    /// Tests inject semantic documents and never start HTTP incidentally.
    pub fn poll(
        &mut self,
        settings: &WikipediaSettings,
        elapsed: Duration,
        resources: Option<&AmbientResources>,
    ) {
        #[cfg(not(test))]
        if self.loader.is_none() && elapsed >= self.retry_after {
            let Some(resources) = resources else {
                self.loading_status = "Wikipedia worker admission is not configured".into();
                self.retry_after = elapsed.saturating_add(Duration::from_secs(30));
                self.refresh_status();
                return;
            };
            match PageLoader::start(
                ilium_ambient::source::default_cache_dir().join("wikipedia"),
                resources,
            ) {
                Ok(loader) => self.loader = Some(loader),
                Err(error) => {
                    self.loading_status = error;
                    self.retry_after = elapsed.saturating_add(Duration::from_secs(1));
                }
            }
        }
        while let Some(event) = self.loader.as_ref().and_then(PageLoader::try_recv) {
            match event {
                LoaderEvent::Loaded(document) => {
                    self.pending = false;
                    self.loading_status.clear();
                    let document = Arc::new(document);
                    if self.document.is_none() {
                        self.activate_loaded(document, settings, elapsed);
                    } else {
                        self.prefetched = Some(document);
                    }
                }
                LoaderEvent::Status(status) => self.loading_status = status,
                LoaderEvent::Failed(error) => {
                    self.pending = false;
                    self.loading_status = error;
                    self.retry_after = elapsed.saturating_add(Duration::from_secs(30));
                }
                LoaderEvent::AdmissionRefused(reason) => {
                    self.pending = false;
                    self.loading_status = reason;
                    self.retry_after = elapsed.saturating_add(Duration::from_secs(1));
                }
            }
        }
        if !self.pending && self.prefetched.is_none() && elapsed >= self.retry_after {
            if let Some(loader) = &self.loader {
                self.pending = loader.request_next();
                if self.pending && self.document.is_none() {
                    self.loading_status = "Loading today's Wikipedia articles".into();
                }
            }
        }
        self.refresh_status();
    }

    /// `maximum_rows` comes from the complete laid-out document, never a
    /// truncated viewport. Returns true when the displayed article changes.
    pub fn advance(
        &mut self,
        settings: &WikipediaSettings,
        elapsed: Duration,
        maximum_rows: f64,
        speed_percent: u16,
    ) -> bool {
        let previous = self.last_elapsed.replace(elapsed);
        if elapsed.is_zero() {
            // This also covers an article first loaded while Motion Off is
            // already active: no positive clock has been seen to rewind yet.
            self.is_clock_reset = true;
            if let Some(scroll) = &mut self.scroll {
                // Reflow may shorten the page while frozen; clamp the viewport
                // without spending dwell or consuming a ready prefetch.
                scroll.advance_with_speed(0.0, maximum_rows, 0, 0, settings.dwell_seconds);
            }
            return false;
        }
        let delta = match previous {
            Some(previous) if elapsed < previous => {
                self.is_clock_reset = true;
                0.0
            }
            Some(previous) if self.is_clock_reset => {
                // Motion Off supplies a frozen zero clock. The first resumed
                // frame establishes a baseline instead of skipping content.
                if elapsed > previous {
                    self.is_clock_reset = false;
                }
                0.0
            }
            Some(previous) => elapsed.saturating_sub(previous).as_secs_f64(),
            None => 0.0,
        };
        let Some(scroll) = &mut self.scroll else {
            return false;
        };
        let is_finished = scroll.advance_with_speed(
            delta,
            maximum_rows,
            settings.scroll_tenths,
            speed_percent,
            settings.dwell_seconds,
        );
        if !is_finished {
            return false;
        }
        let Some(document) = self.prefetched.take() else {
            // Hold the actual page at its bottom while the next load finishes.
            return false;
        };
        self.activate_loaded(document, settings, elapsed);
        self.refresh_status();
        true
    }

    fn activate_loaded(
        &mut self,
        document: Arc<Stored<Arc<Document>>>,
        settings: &WikipediaSettings,
        elapsed: Duration,
    ) {
        self.document = Some(document);
        self.scroll = Some(PageScroll::new(settings.dwell_seconds));
        self.last_elapsed = Some(elapsed);
        self.is_clock_reset = false;
        self.generation = self.generation.wrapping_add(1);
    }

    #[cfg(test)]
    fn activate(
        &mut self,
        document: Arc<Document>,
        settings: &WikipediaSettings,
        elapsed: Duration,
    ) {
        let storage =
            ilium_ambient::resources::AmbientResources::new(crate::execution::test_client())
                .reserve_storage(1)
                .expect("test storage admission");
        self.activate_loaded(Arc::new(Stored::new(document, storage)), settings, elapsed);
    }

    fn refresh_status(&mut self) {
        self.status = if let Some(document) = &self.document {
            let document = document.view();
            let revision = document
                .revision
                .as_deref()
                .map_or(String::new(), |revision| format!(" · revision {revision}"));
            let mut status = format!(
                "{} · {}{revision} · Article source: {}",
                document.title, document.date, document.url
            );
            for warning in &document.warnings {
                status.push_str(" · ");
                status.push_str(warning);
            }
            if !self.loading_status.is_empty() {
                status.push_str(" · ");
                status.push_str(&self.loading_status);
            }
            status
        } else {
            self.loading_status.clone()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    use std::time::Instant;

    fn article(title: &str) -> Arc<Document> {
        Arc::new(
            ilium_wikipedia::parse_article(
                title,
                "https://en.wikipedia.org/wiki/Test",
                "2026-10-02",
                "<p>Actual semantic test article.</p>",
            )
            .unwrap(),
        )
    }

    fn stored_article(title: &str) -> Arc<Stored<Arc<Document>>> {
        let storage =
            ilium_ambient::resources::AmbientResources::new(crate::execution::test_client())
                .reserve_storage(1)
                .expect("test storage admission");
        Arc::new(Stored::new(article(title), storage))
    }

    #[test]
    fn article_storage_follows_current_and_prefetched_ownership() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 64,
            worker_threads: 1,
            worker_bytes: 2 * 1024 * 1024,
        });
        let execution = Execution::start(
            quota.clone(),
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
                jobs: 1,
                service_jobs: 0,
                input_bytes: 4096,
                result_bytes: 64,
            })
            .unwrap();
        let resources = AmbientResources::new(client);
        let first = Arc::new(Stored::new(
            article("First"),
            resources.reserve_storage(16).unwrap(),
        ));
        let second = Arc::new(Stored::new(
            article("Second"),
            resources.reserve_storage(16).unwrap(),
        ));
        let settings = WikipediaSettings {
            dwell_seconds: 0,
            scroll_tenths: 10,
            ..Default::default()
        };
        let mut runtime = WikipediaRuntime::default();
        runtime.activate_loaded(first, &settings, Duration::ZERO);
        runtime.prefetched = Some(second);
        assert_eq!(quota.snapshot().result_bytes, 32);
        assert!(runtime.advance(&settings, Duration::from_secs(1), 0.0, 100));
        assert_eq!(runtime.document().unwrap().title, "Second");
        assert_eq!(quota.snapshot().result_bytes, 16);
        runtime.release();
        assert_eq!(quota.snapshot().result_bytes, 0);
        let mut execution = execution;
        execution.request_shutdown(ShutdownMode::Cancel);
        let joined = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(joined.remaining_workers, 0);
    }

    #[test]
    fn prefetch_waits_for_full_scroll_and_bottom_dwell() {
        let settings = WikipediaSettings {
            dwell_seconds: 2,
            scroll_tenths: 10,
            ..Default::default()
        };
        let mut runtime = WikipediaRuntime::default();
        runtime.activate(article("First"), &settings, Duration::ZERO);
        runtime.prefetched = Some(stored_article("Second"));
        assert!(!runtime.advance(&settings, Duration::from_secs(12), 10.0, 100));
        assert_eq!(runtime.document().unwrap().title, "First");
        assert!(runtime.advance(&settings, Duration::from_secs(14), 10.0, 100));
        assert_eq!(runtime.document().unwrap().title, "Second");
        assert_eq!(runtime.scroll_position(), 0.0);
        assert_eq!(runtime.generation(), 2);
    }

    #[test]
    fn pause_and_missing_prefetch_keep_the_current_article() {
        let mut settings = WikipediaSettings {
            dwell_seconds: 2,
            scroll_tenths: 0,
            ..Default::default()
        };
        let mut runtime = WikipediaRuntime::default();
        runtime.activate(article("First"), &settings, Duration::ZERO);
        runtime.prefetched = Some(stored_article("Second"));
        assert!(!runtime.advance(&settings, Duration::from_secs(100), 0.0, 100));
        settings.scroll_tenths = 10;
        runtime.prefetched = None;
        assert!(!runtime.advance(&settings, Duration::from_secs(200), 0.0, 100));
        assert_eq!(runtime.document().unwrap().title, "First");
        runtime.prefetched = Some(stored_article("Second"));
        assert!(runtime.advance(&settings, Duration::from_secs(201), 0.0, 100));
        runtime.release();
        assert!(runtime.document().is_none());
        assert!(runtime.status().is_none());
    }

    #[test]
    fn delayed_initial_layout_and_admission_do_not_spend_unseen_reading_time() {
        let settings = WikipediaSettings {
            dwell_seconds: 2,
            scroll_tenths: 10,
            ..Default::default()
        };
        let mut runtime = WikipediaRuntime::default();
        runtime.activate(article("First"), &settings, Duration::ZERO);
        for second in [100, 101, 102] {
            runtime.hold_clock(Duration::from_secs(second));
        }
        assert!(!runtime.advance(&settings, Duration::from_secs(102), 100.0, 100));
        assert_eq!(runtime.scroll_position(), 0.0);
        assert!(!runtime.advance(&settings, Duration::from_secs(103), 100.0, 100));
        assert!(!runtime.advance(&settings, Duration::from_secs(104), 100.0, 100));
        assert_eq!(runtime.scroll_position(), 0.0);
        assert!(!runtime.advance(&settings, Duration::from_secs(105), 100.0, 100));
        assert_eq!(runtime.scroll_position(), 1.0);
    }

    #[test]
    fn reflow_wait_keeps_the_last_visible_position_and_dwell_state() {
        let settings = WikipediaSettings {
            dwell_seconds: 2,
            scroll_tenths: 10,
            ..Default::default()
        };
        let mut runtime = WikipediaRuntime::default();
        runtime.activate(article("First"), &settings, Duration::ZERO);
        runtime.advance(&settings, Duration::from_secs(10), 100.0, 100);
        assert_eq!(runtime.scroll_position(), 8.0);
        runtime.hold_clock(Duration::from_secs(100));
        runtime.hold_clock(Duration::from_secs(101));
        runtime.advance(&settings, Duration::from_secs(101), 100.0, 100);
        assert_eq!(runtime.scroll_position(), 8.0);
        runtime.advance(&settings, Duration::from_secs(102), 100.0, 100);
        assert_eq!(runtime.scroll_position(), 9.0);
    }

    #[test]
    fn common_speed_multiplies_travel_without_shortening_dwell() {
        let settings = WikipediaSettings {
            dwell_seconds: 10,
            scroll_tenths: 10,
            ..Default::default()
        };
        let mut runtime = WikipediaRuntime::default();
        runtime.activate(article("First"), &settings, Duration::ZERO);
        assert!(!runtime.advance(&settings, Duration::from_secs(9), 100.0, 300));
        assert_eq!(runtime.scroll_position(), 0.0);
        runtime.advance(&settings, Duration::from_secs(10), 100.0, 300);
        assert_eq!(runtime.scroll_position(), 0.0);
        runtime.advance(&settings, Duration::from_secs(11), 100.0, 300);
        assert_eq!(runtime.scroll_position(), 3.0);
    }

    #[test]
    fn motion_off_clock_rewind_cannot_skip_unseen_content_on_resume() {
        let settings = WikipediaSettings {
            dwell_seconds: 2,
            scroll_tenths: 10,
            ..Default::default()
        };
        let mut runtime = WikipediaRuntime::default();
        runtime.activate(article("First"), &settings, Duration::ZERO);
        runtime.advance(&settings, Duration::from_secs(100), 1000.0, 100);
        assert_eq!(runtime.scroll_position(), 98.0);
        runtime.advance(&settings, Duration::ZERO, 1000.0, 100);
        runtime.advance(&settings, Duration::ZERO, 1000.0, 100);
        runtime.advance(&settings, Duration::from_secs(101), 1000.0, 100);
        assert_eq!(runtime.scroll_position(), 98.0);
        runtime.advance(&settings, Duration::from_secs(102), 1000.0, 100);
        assert_eq!(runtime.scroll_position(), 99.0);
    }

    #[test]
    fn an_article_loaded_while_motion_is_off_starts_at_the_top_on_resume() {
        let settings = WikipediaSettings {
            dwell_seconds: 2,
            scroll_tenths: 10,
            ..Default::default()
        };
        let mut runtime = WikipediaRuntime::default();
        runtime.activate(article("First"), &settings, Duration::ZERO);
        runtime.advance(&settings, Duration::ZERO, 1000.0, 100);
        runtime.advance(&settings, Duration::ZERO, 1000.0, 100);
        runtime.advance(&settings, Duration::from_secs(100), 1000.0, 100);
        assert_eq!(runtime.scroll_position(), 0.0);
        runtime.advance(&settings, Duration::from_secs(103), 1000.0, 100);
        assert_eq!(runtime.scroll_position(), 1.0);
    }

    #[test]
    fn motion_off_holds_a_finished_article_when_its_prefetch_arrives() {
        let settings = WikipediaSettings {
            dwell_seconds: 2,
            scroll_tenths: 10,
            ..Default::default()
        };
        let mut runtime = WikipediaRuntime::default();
        runtime.activate(article("First"), &settings, Duration::ZERO);
        assert!(!runtime.advance(&settings, Duration::from_secs(4), 0.0, 100));
        runtime.prefetched = Some(stored_article("Second"));
        assert!(!runtime.advance(&settings, Duration::ZERO, 0.0, 100));
        assert_eq!(runtime.document().unwrap().title, "First");
    }

    #[test]
    fn warnings_survive_prefetch_status_and_repeated_frame_buckets() {
        let settings = WikipediaSettings::default();
        let mut document = (*article("Cached article")).clone();
        document
            .warnings
            .push("Offline cached source from 2026-10-01".into());
        let mut runtime = WikipediaRuntime::default();
        runtime.activate(Arc::new(document), &settings, Duration::ZERO);
        runtime.loading_status = "Loading next page".into();
        runtime.poll(&settings, Duration::ZERO, None);
        assert!(runtime.status().unwrap().contains("2026-10-01"));
        assert!(runtime.status().unwrap().contains("Loading next page"));
        runtime.advance(&settings, Duration::ZERO, 50.0, 100);
        runtime.advance(&settings, Duration::ZERO, 50.0, 100);
        assert_eq!(runtime.scroll_position(), 0.0);
    }
}
