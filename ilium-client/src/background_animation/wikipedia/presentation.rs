//! One article owner shared by the workspace field and its Settings preview.
//! Layout preparation has one process-wide admission slot. Cancellation leaves
//! that slot occupied until the old CPU worker actually exits.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use ilium_ambient::source::Worker;
use ilium_wikipedia::Document;

use super::render::{RenderCell, RenderError, WikipediaRenderer};
use super::{RenderMode, WikipediaRuntime, WikipediaSettings};

static PREPARATION_ACTIVE: AtomicBool = AtomicBool::new(false);

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
    receiver: Receiver<Result<WikipediaRenderer, RenderError>>,
    worker: Option<Worker>,
}

impl Preparation {
    fn start(
        key: PreparationKey,
        document: Arc<Document>,
        settings: WikipediaSettings,
    ) -> Result<Option<Self>, String> {
        if PREPARATION_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(None);
        }
        let permit = PreparationPermit;
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = Worker::try_spawn("wikipedia-layout", move |stop| {
            let _permit = permit;
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::Lowest,
            );
            let mut renderer = WikipediaRenderer::new();
            let result = renderer
                .prepare_cancellable(document, key.columns, &settings, &stop)
                .map(|_| renderer);
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
    runtime: WikipediaRuntime,
    renderer: Option<WikipediaRenderer>,
    prepared: Option<PreparationKey>,
    pending: Option<Preparation>,
    failure: Option<(PreparationKey, String)>,
    frame_error: Option<String>,
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
    pub fn release(&mut self) {
        self.pending = None;
        self.renderer = None;
        self.prepared = None;
        self.failure = None;
        self.frame_error = None;
        self.runtime.release();
    }

    pub fn cell(&self, x: u16, y: u16) -> Option<&RenderCell> {
        self.renderer.as_ref()?.cell(x, y)
    }

    pub fn status(&self) -> Option<String> {
        let source = self.runtime.status().unwrap_or("Loading Wikipedia");
        if let Some((_, error)) = &self.failure {
            return Some(format!("{source} · {error}"));
        }
        if let Some(error) = &self.frame_error {
            return Some(format!("{source} · {error}"));
        }
        if self.pending.is_some() || (self.runtime.document().is_some() && self.renderer.is_none())
        {
            return Some(format!("{source} · Preparing page layout"));
        }
        if let Some(stats) = self
            .renderer
            .as_ref()
            .and_then(WikipediaRenderer::layout_stats)
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
        self.runtime.poll(settings, elapsed);
        let Some(document) = self.runtime.document().cloned() else {
            self.pending = None;
            self.renderer = None;
            self.prepared = None;
            self.failure = None;
            return;
        };
        let key = PreparationKey::new(self.runtime.generation(), columns, settings);
        if self.renderer.is_none() || self.prepared != Some(key) {
            // Also run on the frame that publishes a completed worker result:
            // no wall time spent waiting for layout was visible to the reader.
            self.runtime.hold_clock(elapsed);
            self.renderer = None;
            self.prepared = None;
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

        if self.renderer.is_none() && self.pending.is_none() && self.failure.is_none() {
            match Preparation::start(key, document.clone(), *settings) {
                Ok(preparation) => self.pending = preparation,
                Err(error) => self.failure = Some((key, error)),
            }
        }
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        if let Err(error) = renderer.prepare(document, columns, settings) {
            self.renderer = None;
            self.prepared = None;
            self.failure = Some((key, error.to_string()));
            return;
        }
        let maximum_rows = f64::from(renderer.total_rows().saturating_sub(u32::from(rows)));
        if self
            .runtime
            .advance(settings, elapsed, maximum_rows, speed_percent)
        {
            self.renderer = None;
            self.prepared = None;
            return;
        }
        if let Err(error) = renderer.render(rows, self.runtime.scroll_position()) {
            self.frame_error = Some(error.to_string());
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
        self.renderer = Some(renderer);
        self.prepared = Some(key);
        self.pending = None;
        self.failure = None;
        self.frame_error = None;
    }

    #[cfg(test)]
    pub(crate) fn layout_count_for_test(&self) -> Option<u64> {
        self.renderer
            .as_ref()
            .map(|renderer| renderer.work_stats().layouts)
    }
}
