//! Revision-bound preparation on the process's shared fixed OS banks.
//! Four visible pane slots; retained caches remain charged in their editors.
use crate::{
    config::LineDisplay,
    editor_pane::EditorPane,
    markdown::{
        document::{Block, Document, ImagePath},
        raster::HeaderRasterizer,
        render::{HeadingRendering, RenderedDocument},
    },
    syntax::LineTokens,
};
use ilium_core::NodeId;
use ilium_execution::{Client, Job, JobContext, JobOutcome, JobPoll, Lane, Receipt, Retained};
use ratatui_image::picker::Picker;
use std::{
    cell::RefCell,
    collections::HashMap,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Notify;

#[derive(Clone, Debug)]
pub(crate) struct PreparationKey {
    pub identity: Arc<()>,
    pub revision: u64,
    pub path: PathBuf,
    pub width: u16,
    pub rendered: bool,
    pub heading: HeadingRendering,
    pub line_display: LineDisplay,
    pub picker: String,
    pub height: u16,
    pub top: usize,
    pub tab: u8,
    pub gutter: bool,
    pub cursor: (usize, usize),
    pub follow: bool,
}
impl PreparationKey {
    pub(crate) fn same_geometry(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
            && self.revision == other.revision
            && self.path == other.path
            && self.width == other.width
            && self.tab == other.tab
            && self.gutter == other.gutter
            && self.line_display == other.line_display
            && self.height == other.height
    }
}
impl PartialEq for PreparationKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
            && self.revision == other.revision
            && self.path == other.path
            && self.width == other.width
            && self.rendered == other.rendered
            && self.heading == other.heading
            && self.line_display == other.line_display
            && self.picker == other.picker
            && self.height == other.height
            && self.top == other.top
            && self.tab == other.tab
            && self.gutter == other.gutter
            && self.cursor == other.cursor
            && self.follow == other.follow
    }
}
impl Eq for PreparationKey {}

pub(crate) struct PreparedContent {
    pub hash: u64,
    pub highlights: Option<Arc<Vec<LineTokens>>>,
    pub rendered: Option<RenderedDocument>,
}
struct Parsed {
    hash: u64,
    highlights: Option<Arc<Vec<LineTokens>>>,
    document: Option<Document>,
}
struct Loaded {
    parsed: Arc<Parsed>,
    images: HashMap<PathBuf, Vec<u8>>,
}
enum Stage {
    Parsed(Arc<Parsed>),
    Loaded(Arc<Loaded>),
    Complete(PreparedContent),
}
enum Work {
    Parse(Vec<String>),
    Load(Arc<Parsed>),
    Render(Arc<Loaded>),
}
struct PreparationJob {
    key: PreparationKey,
    picker: Picker,
    work: Work,
}
thread_local! { static RASTERIZER: RefCell<Option<HeaderRasterizer>> = const { RefCell::new(None) }; }
impl Job for PreparationJob {
    type Output = Stage;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Stage, String> {
        if context.stop_requested() {
            return Err("document preparation cancelled".into());
        }
        match self.work {
            Work::Parse(lines) => {
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                lines.hash(&mut hash);
                let highlights = crate::syntax::highlight_bounded(&self.key.path, &lines, || {
                    context.stop_requested()
                })?
                .map(Arc::new);
                if !self.key.rendered {
                    return Ok(Stage::Complete(PreparedContent {
                        hash: hash.finish(),
                        highlights,
                        rendered: None,
                    }));
                }
                let source = lines.join("\n");
                let base = self.key.path.parent().unwrap_or(std::path::Path::new("."));
                let document = crate::markdown::document::parse_bounded(&source, base)?;
                if context.stop_requested() {
                    return Err("document preparation cancelled".into());
                }
                Ok(Stage::Parsed(Arc::new(Parsed {
                    hash: hash.finish(),
                    highlights,
                    document: Some(document),
                })))
            }
            Work::Load(parsed) => {
                let mut images = HashMap::new();
                let mut total = 0usize;
                if let Some(document) = &parsed.document {
                    for block in &document.blocks {
                        if context.stop_requested() {
                            return Err("document image reads cancelled".into());
                        }
                        let Block::Image {
                            path: ImagePath::Local(path),
                            ..
                        } = block
                        else {
                            continue;
                        };
                        if images.contains_key(path)
                            || images.len() >= 16
                            || total >= 4 * 1024 * 1024
                        {
                            continue;
                        }
                        if let Some(bytes) = crate::markdown::render::read_image_bytes(path) {
                            if total.saturating_add(bytes.capacity()) > 4 * 1024 * 1024 {
                                continue;
                            }
                            total += bytes.capacity();
                            images.insert(path.clone(), bytes);
                        }
                    }
                }
                Ok(Stage::Loaded(Arc::new(Loaded { parsed, images })))
            }
            Work::Render(loaded) => {
                let document = loaded
                    .parsed
                    .document
                    .as_ref()
                    .ok_or("missing parsed Markdown")?;
                let mut rendered = RASTERIZER.with(|slot| {
                    let mut rasterizer = slot.borrow_mut();
                    let rasterizer = rasterizer.get_or_insert_with(HeaderRasterizer::new);
                    crate::markdown::render::render_prepared(
                        document,
                        &loaded.images,
                        &self.picker,
                        rasterizer,
                        self.key.width,
                        self.key.heading,
                        || context.stop_requested(),
                    )
                })?;
                crate::markdown::render::prepare_layout(
                    &mut rendered,
                    self.key.width,
                    self.key.line_display,
                )?;
                if context.stop_requested() {
                    return Err("document preparation cancelled".into());
                }
                Ok(Stage::Complete(PreparedContent {
                    hash: loaded.parsed.hash,
                    highlights: loaded.parsed.highlights.clone(),
                    rendered: Some(rendered),
                }))
            }
        }
    }
}
enum Pending {
    Running(Receipt<PreparationJob>),
    Between(Retained<Option<Stage>>),
}
struct Slot {
    id: NodeId,
    key: PreparationKey,
    picker: Picker,
    pending: Option<Pending>,
    error: Option<String>,
}
pub(crate) struct Completion {
    pub id: NodeId,
    pub key: PreparationKey,
    pub content: Retained<Result<PreparedContent, String>>,
}
pub struct DocumentPreparation {
    client: Client,
    notification: Arc<Notify>,
    slots: Vec<Slot>,
}
impl DocumentPreparation {
    pub fn new(client: Client) -> Self {
        let notification = Arc::new(Notify::new());
        let wake = notification.clone();
        Self {
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            slots: Vec::with_capacity(4),
        }
    }
    pub fn notification(&self) -> Arc<Notify> {
        self.notification.clone()
    }
    pub(crate) fn retain_visible(&mut self, ids: &[NodeId]) {
        self.slots.retain_mut(|slot| {
            let keep = ids.contains(&slot.id);
            if !keep {
                if let Some(Pending::Running(receipt)) = &slot.pending {
                    receipt.cancel();
                }
            }
            keep
        });
    }
    /// Admission precedes every large source copy; repeated busy attempts only
    /// compare the small key and retry on the ordinary event-loop cadence.
    pub(crate) fn request(
        &mut self,
        id: NodeId,
        editor: &EditorPane,
        width: u16,
        picker: &Picker,
    ) -> Result<(), String> {
        let Some(key) = editor.preparation_key(width, picker) else {
            return Ok(());
        };
        if let Some(slot) = self
            .slots
            .iter()
            .find(|slot| slot.id == id && slot.key == key)
        {
            return slot.error.clone().map_or(Ok(()), Err);
        }
        if let Some(index) = self.slots.iter().position(|slot| slot.id == id) {
            if let Some(Pending::Running(receipt)) = &self.slots[index].pending {
                receipt.cancel();
            }
            self.slots.swap_remove(index);
        }
        if self.slots.len() == 4 {
            return Err("document preparation visible-pane limit reached".into());
        }
        let reservation = self
            .client
            .try_reserve(Lane::Cpu, crate::execution::DOCUMENT_COST)
            .map_err(|reason| format!("document preparation admission: {reason:?}"))?;
        let lines = editor.textarea.lines();
        if lines.len() > 32768
            || lines.iter().any(|line| line.len() > 65536)
            || lines.iter().map(String::len).sum::<usize>() > 2 * 1024 * 1024
        {
            self.slots.push(Slot {
                id,
                key,
                picker: picker.clone(),
                pending: None,
                error: Some("document exceeds preparation limits; showing source".into()),
            });
            return Err("document exceeds preparation limits; showing source".into());
        }
        let receipt = reservation
            .submit(PreparationJob {
                key: key.clone(),
                picker: picker.clone(),
                work: Work::Parse(lines.to_vec()),
            })
            .map_err(|rejected| {
                format!("document preparation submission: {:?}", rejected.reason)
            })?;
        self.slots.push(Slot {
            id,
            key,
            picker: picker.clone(),
            pending: Some(Pending::Running(receipt)),
            error: None,
        });
        Ok(())
    }
    pub(crate) fn collect(&mut self) -> Vec<Completion> {
        let mut completed = Vec::with_capacity(4);
        for slot in &mut self.slots {
            let Some(pending) = slot.pending.take() else {
                continue;
            };
            let stage = match pending {
                Pending::Between(stage) => stage,
                Pending::Running(mut receipt) => match receipt.try_take() {
                    JobPoll::Pending => {
                        slot.pending = Some(Pending::Running(receipt));
                        continue;
                    }
                    JobPoll::Ready(outcome) => {
                        if matches!(outcome.view(), JobOutcome::Finished(Ok(Stage::Complete(_)))) {
                            completed.push(Completion {
                                id: slot.id,
                                key: slot.key.clone(),
                                content: outcome.map(|outcome| match outcome {
                                    JobOutcome::Finished(Ok(Stage::Complete(content))) => {
                                        Ok(content)
                                    }
                                    _ => Err("invalid completed document result".into()),
                                }),
                            });
                            continue;
                        }
                        if !matches!(outcome.view(), JobOutcome::Finished(Ok(_))) {
                            let error = match outcome.view() {
                                JobOutcome::Finished(Err(error)) => error.clone(),
                                JobOutcome::Panicked => {
                                    "document preparation worker panicked".into()
                                }
                                _ => "document preparation cancelled before execution".into(),
                            };
                            slot.error = Some(error.clone());
                            completed.push(Completion {
                                id: slot.id,
                                key: slot.key.clone(),
                                content: outcome.map(move |_| Err(error)),
                            });
                            continue;
                        }
                        outcome.map(|outcome| match outcome {
                            JobOutcome::Finished(Ok(stage)) => Some(stage),
                            _ => None,
                        })
                    }
                    _ => {
                        slot.error = Some("document preparation receipt lost".into());
                        continue;
                    }
                },
            };
            let lane = match stage.view() {
                Some(Stage::Parsed(_)) => Lane::Io,
                _ => Lane::Cpu,
            };
            let Ok(reservation) = self
                .client
                .try_reserve(lane, crate::execution::DOCUMENT_COST)
            else {
                slot.pending = Some(Pending::Between(stage));
                continue;
            };
            // Clone immutable stage payload only while both reservations exist.
            // Transfer its charge to the new job; old Retained drops afterwards.
            let work = match stage.view() {
                Some(Stage::Parsed(parsed)) => Work::Load(parsed.clone()),
                Some(Stage::Loaded(loaded)) => Work::Render(loaded.clone()),
                _ => {
                    slot.error = Some("invalid intermediate document result".into());
                    continue;
                }
            };
            match reservation.submit(PreparationJob {
                key: slot.key.clone(),
                picker: slot.picker.clone(),
                work,
            }) {
                Ok(receipt) => slot.pending = Some(Pending::Running(receipt)),
                Err(_) => slot.pending = Some(Pending::Between(stage)),
            }
        }
        completed
    }
    pub fn cancel(&mut self) {
        for slot in &self.slots {
            if let Some(Pending::Running(receipt)) = &slot.pending {
                receipt.cancel();
            }
        }
        self.slots.clear();
    }
}
impl Drop for DocumentPreparation {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    fn completion(preparation: &mut DocumentPreparation) -> Completion {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = preparation.collect().pop() {
                return result;
            }
            assert!(Instant::now() < deadline, "preparation did not complete");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn pane(path: &str, lines: &[&str]) -> EditorPane {
        let mut editor = EditorPane::empty();
        editor.path = Some(PathBuf::from(path));
        editor.textarea = ratatui_textarea::TextArea::from(lines.iter().copied());
        editor
    }
    #[test]
    fn worker_highlighting_matches_sequential_multiline_parser_and_is_retained() {
        let client = crate::execution::test_document_client();
        let usage = client.clone();
        let mut preparation = DocumentPreparation::new(client);
        let picker = Picker::halfblocks();
        let mut editor = pane(
            "scope.rs",
            &[
                "/* open",
                "comment */ let value = 7;",
                "let text = \"value\";",
            ],
        );
        assert!(
            editor.highlighted_lines().is_none(),
            "getter must not prepare on UI"
        );
        preparation
            .request(NodeId(1), &editor, 80, &picker)
            .unwrap();
        let prepared = completion(&mut preparation);
        assert!(editor.install_preparation(prepared, &picker, 80));
        assert_eq!(
            &*editor.highlighted_lines().unwrap(),
            &crate::syntax::highlight(&PathBuf::from("scope.rs"), editor.textarea.lines()).unwrap()
        );
        assert!(
            usage.usage().jobs > 0,
            "installed cache must retain admission"
        );
        editor.clear_preparation();
        let deadline = Instant::now() + Duration::from_secs(5);
        while usage.usage().jobs != 0 {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(usage.usage().jobs, 0);
    }
    #[test]
    fn stale_width_revision_path_and_replacement_never_publish() {
        let mut preparation = DocumentPreparation::new(crate::execution::test_client());
        let picker = Picker::halfblocks();
        let mut editor = pane("note.md", &["# Old", "body"]);
        editor.view_mode = crate::editor_pane::EditorViewMode::Rendered;
        preparation
            .request(NodeId(2), &editor, 80, &picker)
            .unwrap();
        let prepared = completion(&mut preparation);
        assert!(!editor.install_preparation(prepared, &picker, 40));
        preparation
            .request(NodeId(2), &editor, 40, &picker)
            .unwrap();
        let prepared = completion(&mut preparation);
        editor.replace_contents("# New");
        assert!(!editor.install_preparation(prepared, &picker, 40));
        preparation
            .request(NodeId(2), &editor, 40, &picker)
            .unwrap();
        let prepared = completion(&mut preparation);
        editor.retarget_path(PathBuf::from("note.rs"));
        assert!(!editor.install_preparation(prepared, &picker, 40));
        preparation
            .request(NodeId(2), &editor, 40, &picker)
            .unwrap();
        let prepared = completion(&mut preparation);
        let mut replacement = pane("note.rs", &["# New"]);
        assert!(!replacement.install_preparation(prepared, &picker, 40));
    }
    #[test]
    fn overload_keeps_source_and_cancellation_releases_pane_slot() {
        let client = crate::execution::test_document_client();
        let usage = client.clone();
        let mut preparation = DocumentPreparation::new(client);
        let picker = Picker::halfblocks();
        let mut editor = pane("large.rs", &["old"]);
        editor.replace_contents(&"x".repeat(65537));
        assert!(preparation
            .request(NodeId(3), &editor, 80, &picker)
            .is_err());
        assert_eq!(editor.textarea.lines()[0].len(), 65537);
        assert_eq!(usage.usage().jobs, 0);
        preparation.cancel();
        assert!(preparation.slots.is_empty());
        editor.replace_contents("let value = 1;");
        preparation
            .request(NodeId(3), &editor, 80, &picker)
            .unwrap();
        preparation.cancel();
        assert!(preparation.collect().is_empty());
    }
}
