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
use ilium_execution::{
    Client, Job, JobContext, JobOutcome, JobPoll, Lane, Receipt, Reservation, Retained,
    RetirementReservation, Retiring, RetiringArc,
};
use ratatui_image::picker::Picker;
use std::{
    cell::RefCell,
    collections::HashMap,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Notify;

// Physical lifetime declarations, separate from the job's transient peak.
// Source is limited to2MiB/32768lines; tokens and styled Markdown to8MiB.
// Image reads retain at most4MiB. Rendered layout uses the existing32MiB
// result envelope. These cooperative declarations do not prove native RSS.
const SOURCE_STORAGE: usize = 4 * 1024 * 1024;
const HIGHLIGHT_STORAGE: usize = 32 * 1024 * 1024;
const PARSED_STORAGE: usize = 16 * 1024 * 1024;
const IMAGE_STORAGE: usize = 8 * 1024 * 1024;
const TEXT_STORAGE: usize = 16 * 1024 * 1024;
const RENDERED_STORAGE: usize = 32 * 1024 * 1024;

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
    pub highlights: Option<RetiringArc<Vec<LineTokens>>>,
    pub rendered: Option<Retiring<RenderedDocument>>,
}
struct Parsed {
    hash: u64,
    highlights: Option<RetiringArc<Vec<LineTokens>>>,
    document: Option<Document>,
}
struct Loaded {
    parsed: RetiringArc<Parsed>,
    images: HashMap<PathBuf, Vec<u8>>,
}
enum Stage {
    Parsed(RetiringArc<Parsed>),
    Loaded(RetiringArc<Loaded>),
    Complete(PreparedContent),
}
struct ParseWork {
    lines: Retiring<Vec<String>>,
    highlights: RetirementReservation<Vec<LineTokens>>,
    parsed: Option<RetirementReservation<Parsed>>,
}
enum Work {
    Parse(ParseWork),
    Load(RetiringArc<Parsed>, RetirementReservation<Loaded>),
    Render(
        RetiringArc<Loaded>,
        RetirementReservation<RenderedDocument>,
        RetirementReservation<crate::markdown::render::TextArena>,
    ),
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
            Work::Parse(ParseWork {
                lines,
                highlights: output,
                parsed,
            }) => lines
                .try_consume_on_cpu(|lines| {
                    let mut hash = std::collections::hash_map::DefaultHasher::new();
                    lines.hash(&mut hash);
                    let highlights =
                        crate::syntax::highlight_bounded(&self.key.path, &lines, || {
                            context.stop_requested()
                        })?
                        .map(|lines| output.attach_shared(lines));
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
                    let output = parsed.ok_or("missing parsed-document retirement admission")?;
                    Ok(Stage::Parsed(output.attach_shared(Parsed {
                        hash: hash.finish(),
                        highlights,
                        document: Some(document),
                    })))
                })
                .map_err(|_| "document source preparation requires the CPU bank")?,
            Work::Load(parsed, output) => {
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
                Ok(Stage::Loaded(
                    output.attach_shared(Loaded { parsed, images }),
                ))
            }
            Work::Render(loaded, output, text) => {
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
                        text,
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
                    rendered: Some(output.attach(rendered)),
                }))
            }
        }
    }
}
struct Capture {
    next: usize,
    bytes: usize,
    reservation: Reservation,
    work: ParseWork,
}
enum Pending {
    Capture(Box<Capture>),
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
    capture_budget: Option<Arc<crate::editor_capture_budget::CaptureBudget>>,
}
impl DocumentPreparation {
    pub fn new(client: Client) -> Self {
        let notification = Arc::new(Notify::new());
        let wake = notification.clone();
        Self {
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            slots: Vec::with_capacity(4),
            capture_budget: None,
        }
    }
    pub(crate) fn set_capture_budget(
        &mut self,
        budget: Arc<crate::editor_capture_budget::CaptureBudget>,
    ) {
        self.capture_budget = Some(budget);
    }
    pub fn notification(&self) -> Arc<Notify> {
        self.notification.clone()
    }
    pub(crate) fn retain_visible(&mut self, ids: &[NodeId]) {
        self.slots.retain_mut(|slot| {
            let keep = ids.contains(&slot.id);
            if !keep {
                if let Some(budget) = &self.capture_budget {
                    budget.cancel(slot.id, crate::editor_capture_budget::Kind::Document);
                }
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
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.id == id && slot.key == key)
        {
            return self.advance_capture(index, editor.textarea.lines());
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
        if lines.len() > 32768 {
            self.slots.push(Slot {
                id,
                key,
                picker: picker.clone(),
                pending: None,
                error: Some("document exceeds preparation limits; showing source".into()),
            });
            return Err("document exceeds preparation limits; showing source".into());
        }
        // Retire the source even when submission/cancellation prevents the
        // callback. Independently shared token leaves retain their own envelope.
        let retirement = self.client.retirement();
        let source = retirement
            .try_reserve::<Vec<String>>(SOURCE_STORAGE)
            .map_err(|reason| format!("document source retirement admission: {reason:?}"))?;
        let highlights = retirement
            .try_reserve::<Vec<LineTokens>>(HIGHLIGHT_STORAGE)
            .map_err(|reason| format!("document token retirement admission: {reason:?}"))?;
        let parsed = if key.rendered {
            Some(
                retirement
                    .try_reserve::<Parsed>(PARSED_STORAGE)
                    .map_err(|reason| format!("Markdown retirement admission: {reason:?}"))?,
            )
        } else {
            None
        };
        self.slots.push(Slot {
            id,
            key,
            picker: picker.clone(),
            pending: Some(Pending::Capture(Box::new(Capture {
                next: 0,
                bytes: 0,
                reservation,
                work: ParseWork {
                    // Even partial captures retire away from the UI on
                    // revision replacement, cancellation or pane removal.
                    lines: source.attach(Vec::new()),
                    highlights,
                    parsed,
                },
            }))),
            error: None,
        });
        self.advance_capture(self.slots.len() - 1, lines)
    }

    fn advance_capture(&mut self, index: usize, lines: &[String]) -> Result<(), String> {
        let slot = &mut self.slots[index];
        if let Some(error) = &slot.error {
            return Err(error.clone());
        }
        let Some(Pending::Capture(capture)) = &mut slot.pending else {
            return Ok(());
        };
        // The interactive root supplies its shared turn budget. Independent
        // callers still receive a bounded single-call capture.
        let independent_budget;
        let budget = if let Some(budget) = &self.capture_budget {
            budget.as_ref()
        } else {
            independent_budget = crate::editor_capture_budget::CaptureBudget::new();
            &independent_budget
        };
        let Some(credit) = budget.take(slot.id, crate::editor_capture_budget::Kind::Document, 1024)
        else {
            self.notification.notify_one();
            return Ok(());
        };
        let mut bytes = 0;
        let mut count = 0;
        let mut error = None;
        while capture.next < lines.len() && count < credit.lines {
            let line = &lines[capture.next];
            if line.len() > 65536 || capture.bytes.saturating_add(line.len()) > 2 * 1024 * 1024 {
                error = Some("document exceeds preparation limits; showing source".to_string());
                break;
            }
            if line.len() > credit.bytes - bytes {
                break;
            }
            capture.work.lines.push(line.clone());
            capture.next += 1;
            capture.bytes += line.len();
            bytes += line.len();
            count += 1;
        }
        budget.finish(credit, bytes, count);
        if let Some(error) = error {
            slot.pending = None;
            slot.error = Some(error.clone());
            return Err(error);
        }
        if capture.next < lines.len() {
            // The existing completion wake schedules a new capture turn. The
            // same revision key is checked again before borrowing more lines.
            self.notification.notify_one();
            return Ok(());
        }
        let Some(Pending::Capture(capture)) = slot.pending.take() else {
            return Err("document capture ownership lost".into());
        };
        let Capture {
            reservation, work, ..
        } = *capture;
        match reservation.submit(PreparationJob {
            key: slot.key.clone(),
            picker: slot.picker.clone(),
            work: Work::Parse(work),
        }) {
            Ok(receipt) => {
                slot.pending = Some(Pending::Running(receipt));
                Ok(())
            }
            Err(rejected) => {
                // Original source and output slots still have typed retirement
                // even when publication fails; source preparation is replaceable.
                let error = format!("document preparation submission: {:?}", rejected.reason);
                slot.error = Some(error.clone());
                Err(error)
            }
        }
    }
    pub(crate) fn collect(&mut self) -> Vec<Completion> {
        let mut completed = Vec::with_capacity(4);
        for slot in &mut self.slots {
            let Some(pending) = slot.pending.take() else {
                continue;
            };
            let stage = match pending {
                Pending::Capture(capture) => {
                    slot.pending = Some(Pending::Capture(capture));
                    continue;
                }
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
            let retirement = self.client.retirement();
            let work = match stage.view() {
                Some(Stage::Parsed(parsed)) => {
                    let Ok(output) = retirement.try_reserve::<Loaded>(IMAGE_STORAGE) else {
                        slot.pending = Some(Pending::Between(stage));
                        continue;
                    };
                    Work::Load(parsed.clone(), output)
                }
                Some(Stage::Loaded(loaded)) => {
                    let Ok(output) = retirement.try_reserve::<RenderedDocument>(RENDERED_STORAGE)
                    else {
                        slot.pending = Some(Pending::Between(stage));
                        continue;
                    };
                    let Ok(text) =
                        retirement.try_reserve::<crate::markdown::render::TextArena>(TEXT_STORAGE)
                    else {
                        slot.pending = Some(Pending::Between(stage));
                        continue;
                    };
                    Work::Render(loaded.clone(), output, text)
                }
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
            if let Some(budget) = &self.capture_budget {
                budget.cancel(slot.id, crate::editor_capture_budget::Kind::Document);
            }
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
    fn shared_turn_capture_limits_copying_and_preserves_cross_page_syntax() {
        let client = crate::execution::test_document_client();
        let mut preparation = DocumentPreparation::new(client);
        let budget = Arc::new(crate::editor_capture_budget::CaptureBudget::new());
        preparation.set_capture_budget(budget.clone());
        let picker = Picker::halfblocks();
        let lines: Vec<String> = (0..2050)
            .map(|row| match row {
                1023 => "/* opening a cross-page comment".into(),
                1025 => "closing */ let value = 7;".into(),
                _ => "let small_line = 1;".into(),
            })
            .collect();
        let mut editor = pane("capture.rs", &["unused"]);
        editor.textarea = ratatui_textarea::TextArea::from(lines);
        preparation
            .request(NodeId(91), &editor, 80, &picker)
            .unwrap();
        let captured = match preparation.slots[0].pending.as_ref().unwrap() {
            Pending::Capture(capture) => capture.next,
            _ => panic!("whole source must not be submitted in one turn"),
        };
        assert_eq!(captured, 1024);
        preparation
            .request(NodeId(91), &editor, 80, &picker)
            .unwrap();
        assert!(matches!(preparation.slots[0].pending.as_ref(),
            Some(Pending::Capture(capture)) if capture.next == captured));
        for _ in 0..2 {
            budget.begin_turn();
            preparation
                .request(NodeId(91), &editor, 80, &picker)
                .unwrap();
        }
        let prepared = completion(&mut preparation);
        assert!(editor.install_preparation(prepared, &picker, 80));
        assert_eq!(
            &*editor.highlighted_lines().unwrap(),
            &crate::syntax::highlight(&PathBuf::from("capture.rs"), editor.textarea.lines())
                .unwrap()
        );
    }

    #[test]
    fn revision_change_discards_partial_capture_and_cancellation_releases_job() {
        let client = crate::execution::test_document_client();
        let usage = client.clone();
        let mut preparation = DocumentPreparation::new(client);
        let budget = Arc::new(crate::editor_capture_budget::CaptureBudget::new());
        preparation.set_capture_budget(budget.clone());
        let picker = Picker::halfblocks();
        let mut editor = pane("revision.rs", &["unused"]);
        editor.textarea = ratatui_textarea::TextArea::from((0..2050).map(|_| "old"));
        preparation
            .request(NodeId(92), &editor, 80, &picker)
            .unwrap();
        assert!(matches!(
            preparation.slots[0].pending,
            Some(Pending::Capture(_))
        ));
        editor.replace_contents("let replacement = 1;");
        budget.begin_turn();
        preparation
            .request(NodeId(92), &editor, 80, &picker)
            .unwrap();
        let prepared = completion(&mut preparation);
        assert!(editor.install_preparation(prepared, &picker, 80));
        assert_eq!(
            &*editor.highlighted_lines().unwrap(),
            &crate::syntax::highlight(&PathBuf::from("revision.rs"), editor.textarea.lines())
                .unwrap()
        );
        editor.clear_preparation();
        preparation.cancel();
        assert_eq!(usage.usage().jobs, 0);
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

    fn owned_retirement_bank() -> (
        ilium_execution::Execution,
        ilium_execution::QuotaGroup,
        Client,
    ) {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
        };
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 4,
            jobs: 4,
            service_jobs: 0,
            input_bytes: 256 * 1024 * 1024,
            result_bytes: 128 * 1024 * 1024,
            worker_threads: 1,
            worker_bytes: 256 * 1024 * 1024,
        });
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 4,
                    priority: None,
                    resident_bytes_per_thread: 64 * 1024 * 1024,
                },
                io: disabled,
                service: disabled,
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 4,
                service_jobs: 0,
                input_bytes: 256 * 1024 * 1024,
                result_bytes: 128 * 1024 * 1024,
            })
            .unwrap();
        (execution, quota, client)
    }

    #[test]
    fn independent_highlight_clone_retains_physical_charge_and_final_drop_waits_for_cpu() {
        let (mut execution, quota, client) = owned_retirement_bank();
        let baseline = quota.snapshot().worker_bytes;
        let mut preparation = DocumentPreparation::new(client.clone());
        let picker = Picker::halfblocks();
        let mut editor = pane("leaf.rs", &["/* scope", "end */ let value = 1;"]);
        preparation
            .request(NodeId(41), &editor, 80, &picker)
            .unwrap();
        let prepared = completion(&mut preparation);
        let independent = prepared
            .content
            .view()
            .as_ref()
            .unwrap()
            .highlights
            .as_ref()
            .unwrap()
            .clone();
        assert!(editor.install_preparation(prepared, &picker, 80));
        editor.clear_preparation();
        preparation.cancel();
        assert_eq!(
            client.usage().jobs,
            0,
            "outer result debit may release; leaf storage must not"
        );
        assert!(quota.snapshot().worker_bytes >= baseline + HIGHLIGHT_STORAGE);
        let deadline = Instant::now() + Duration::from_secs(5);
        while execution.monitor().health().lanes[Lane::Cpu as usize].retirement_live != 1 {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let (started_sender, started_receiver) = std::sync::mpsc::sync_channel(1);
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(0);
        let blocker = client
            .try_submit(
                Lane::Cpu,
                ilium_execution::JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
                move |_| {
                    started_sender.send(()).unwrap();
                    release_receiver.recv().unwrap();
                    Ok::<(), String>(())
                },
            )
            .unwrap();
        started_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let completed = execution.monitor().health().lanes[Lane::Cpu as usize].retirement_completed;
        drop(independent);
        let parked = execution.monitor().health();
        assert_eq!(parked.lanes[Lane::Cpu as usize].retirement_live, 1);
        assert_eq!(parked.lanes[Lane::Cpu as usize].retirement_queued, 1);
        assert_eq!(
            parked.lanes[Lane::Cpu as usize].retirement_completed,
            completed
        );
        assert!(quota.snapshot().worker_bytes >= baseline + HIGHLIGHT_STORAGE);
        release_sender.send(()).unwrap();
        drop(blocker);
        drop(preparation);
        drop(client);
        execution.request_shutdown(ilium_execution::ShutdownMode::Drain);
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(report.shutdown_complete);
        assert_eq!(report.health.lanes[Lane::Cpu as usize].retirement_live, 0);
        assert!(report.health.lanes[Lane::Cpu as usize].retirement_completed > completed);
    }

    #[test]
    fn cancelled_queued_document_keeps_its_original_storage_until_cpu_can_retire() {
        let (mut execution, quota, client) = owned_retirement_bank();
        let baseline = quota.snapshot().worker_bytes;
        let (started_sender, started_receiver) = std::sync::mpsc::sync_channel(1);
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(0);
        let blocker = client
            .try_submit(
                Lane::Cpu,
                ilium_execution::JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
                move |_| {
                    started_sender.send(()).unwrap();
                    release_receiver.recv().unwrap();
                    Ok::<(), String>(())
                },
            )
            .unwrap();
        started_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let mut preparation = DocumentPreparation::new(client.clone());
        let picker = Picker::halfblocks();
        let editor = pane("queued.rs", &["let untouched = 7;"]);
        preparation
            .request(NodeId(42), &editor, 80, &picker)
            .unwrap();
        preparation.cancel();
        assert!(preparation.collect().is_empty());
        assert_eq!(editor.textarea.lines(), &["let untouched = 7;"]);
        assert!(execution.monitor().health().lanes[Lane::Cpu as usize].retirement_live >= 1);
        assert!(quota.snapshot().worker_bytes >= baseline + SOURCE_STORAGE);
        release_sender.send(()).unwrap();
        drop(blocker);
        drop(preparation);
        drop(client);
        execution.request_shutdown(ilium_execution::ShutdownMode::Drain);
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(report.shutdown_complete);
        assert_eq!(report.health.lanes[Lane::Cpu as usize].retirement_live, 0);
    }
    #[test]
    fn cloned_rendered_text_keeps_original_arena_until_cpu_retirement() {
        use crate::markdown::render::{TextArena, TextPreparation};
        let (mut execution, quota, client) = owned_retirement_bank();
        let baseline = quota.snapshot().worker_bytes;
        let reservation = client
            .retirement()
            .try_reserve::<TextArena>(TEXT_STORAGE)
            .unwrap();
        let mut receipt = client
            .try_submit(
                Lane::Cpu,
                ilium_execution::JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
                move |_| {
                    let mut preparation = TextPreparation::new(reservation);
                    let text =
                        preparation.capture(std::sync::Arc::new(vec![ratatui::text::Line::from(
                            "original rendered text",
                        )]));
                    preparation.finish()?;
                    Ok::<_, String>(text)
                },
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            match receipt.try_take() {
                JobPoll::Ready(result) => break result,
                JobPoll::Pending => {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                _ => panic!("rendered text receipt lost"),
            }
        };
        let (outcome, retention) = result.into_parts();
        let JobOutcome::Finished(Ok(text)) = outcome else {
            panic!("rendered text failed")
        };
        let independent = text.clone();
        drop(text);
        drop(retention);
        drop(receipt);
        assert_eq!(independent[0].to_string(), "original rendered text");
        assert_eq!(client.usage().jobs, 0);
        assert!(quota.snapshot().worker_bytes >= baseline + TEXT_STORAGE);
        let (started_sender, started_receiver) = std::sync::mpsc::sync_channel(1);
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(0);
        let blocker = client
            .try_submit(
                Lane::Cpu,
                ilium_execution::JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
                move |_| {
                    started_sender.send(()).unwrap();
                    release_receiver.recv().unwrap();
                    Ok::<(), String>(())
                },
            )
            .unwrap();
        started_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let completed = execution.monitor().health().lanes[Lane::Cpu as usize].retirement_completed;
        drop(independent);
        let parked = execution.monitor().health();
        assert_eq!(parked.lanes[Lane::Cpu as usize].retirement_live, 1);
        assert_eq!(parked.lanes[Lane::Cpu as usize].retirement_queued, 1);
        assert_eq!(
            parked.lanes[Lane::Cpu as usize].retirement_completed,
            completed
        );
        assert!(quota.snapshot().worker_bytes >= baseline + TEXT_STORAGE);
        release_sender.send(()).unwrap();
        drop(blocker);
        drop(client);
        execution.request_shutdown(ilium_execution::ShutdownMode::Drain);
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(report.shutdown_complete);
        assert_eq!(report.health.lanes[Lane::Cpu as usize].retirement_live, 0);
        assert!(report.health.lanes[Lane::Cpu as usize].retirement_completed > completed);
    }
}
