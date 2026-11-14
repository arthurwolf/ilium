//! Preparation of the exact immutable frame captured by a context-menu click.
use crate::{
    terminal_selection::TerminalSelection,
    terminal_view::{PaintedTerminal, PreparationSnapshot},
};
use ilium_core::NodeId;
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, RejectReason, Retained,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Notify;
#[derive(Clone)]
pub(crate) struct ContextKey {
    pub pane_id: NodeId,
    pub identity: Arc<()>,
    pub generation: u64,
    pub column: u16,
    pub row: u16,
    pub snapshot_ordinal: u64,
}
/// Agent class, session identity, pane launch directory and the optional
/// server-verified transcript path (a hint re-verified before use).
pub(crate) type HistoryContext = (ilium_core::AgentClass, String, PathBuf, Option<PathBuf>);

pub(crate) struct ContextRequest {
    pub key: ContextKey,
    pub source_row: usize,
    pub column: usize,
    pub selection: Option<TerminalSelection>,
    pub cwd: PathBuf,
    pub home: Option<PathBuf>,
    pub history_context: Option<HistoryContext>,
    /// Link detectors of the enabled plugins (G5); the menu asks them before the kernel.
    pub detectors: Arc<[crate::plugin_host::links::LinkDetector]>,
}
pub(crate) struct ContextText {
    pub source_line_text: String,
    pub visible_contents: String,
    pub full_history: String,
    pub selection_text: Option<String>,
    pub open_target: Option<crate::open_target::OpenTarget>,
    /// The plugin link detector match under the clicked cell, if any.
    pub plugin_link: Option<crate::plugin_host::links::LinkMatch>,
    pub history_path: Option<PathBuf>,
    pub warning: Option<String>,
    pub source_hold: Option<Retained<()>>,
}
struct ContextJob {
    snapshot: PreparationSnapshot,
    source_row: usize,
    column: usize,
    selection: Option<TerminalSelection>,
}
impl Job for ContextJob {
    type Output = ContextText;
    type Error = String;
    fn run(self, context: JobContext) -> Result<ContextText, String> {
        if context.stop_requested() {
            return Err("context text preparation cancelled".into());
        }
        let bytes = self.snapshot.history.to_vec();
        let stripped = strip_ansi_escapes::strip(&bytes);
        let full_history = String::from_utf8_lossy(&stripped).into_owned();
        if context.stop_requested() {
            return Err("context text preparation cancelled".into());
        }
        let visible_contents = self.snapshot.visible.contents();
        let source_line_text = visible_contents
            .lines()
            .nth(self.source_row)
            .map(|line| line.trim_end().to_owned())
            .unwrap_or_default();
        let selection_text = self.selection.as_ref().and_then(|selection| {
            crate::terminal_selection::text(&self.snapshot.visible, selection)
        });
        let column =
            crate::terminal_view::cell_column_to_byte_offset(&source_line_text, self.column);
        let osc_target = self
            .snapshot
            .links
            .iter()
            .rev()
            .find_map(|(label, target)| {
                let start = source_line_text.find(label)?;
                (start..start + label.len())
                    .contains(&column)
                    .then(|| target.clone())
            });
        // Only URL recognition here: path metadata is a separate finite I/O job.
        let open_target = osc_target
            .as_deref()
            .and_then(crate::open_target::resolve_url);
        Ok(ContextText {
            source_line_text,
            visible_contents,
            full_history,
            selection_text,
            open_target,
            plugin_link: None,
            history_path: None,
            warning: None,
            source_hold: None,
        })
    }
}
struct ResolveJob {
    text: ContextText,
    column: usize,
    cwd: PathBuf,
    home: Option<PathBuf>,
    history_context: Option<HistoryContext>,
    detectors: Arc<[crate::plugin_host::links::LinkDetector]>,
}
impl Job for ResolveJob {
    type Output = ContextText;
    type Error = String;
    fn run(mut self, context: JobContext) -> Result<ContextText, String> {
        if context.stop_requested() {
            return Err("context target resolution cancelled".into());
        }
        // The plugin link detectors (G5) are the only text detection; an OSC 8 link is the fallback.
        let (target, plugin_link) = crate::open_target::resolve_at_with_plugins(
            &self.text.source_line_text,
            self.column,
            &self.cwd,
            self.home.as_deref(),
            &self.detectors,
        );
        self.text.open_target = target.or(self.text.open_target);
        self.text.plugin_link = plugin_link;
        if let (Some(home), Some((class, session, path, transcript_hint))) =
            (&self.home, &self.history_context)
        {
            if matches!(
                class,
                ilium_core::AgentClass::Claude | ilium_core::AgentClass::Codex
            ) {
                let locator = ilium_agent_session::TranscriptLocator::new_bounded(
                    home,
                    path,
                    ilium_agent_session::INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS,
                );
                self.text.history_path = crate::agent_history_path::resolve_history_path(
                    &locator,
                    class,
                    session,
                    transcript_hint.as_deref(),
                );
                if locator.read_limit_reached() {
                    // Optional path verification must not discard the already
                    // captured screen/selection or disable prompt recovery.
                    self.text.history_path = None;
                    self.text.warning = Some(
                        "Transcript path unavailable: history scan exceeded its safety limit. Screen and prompt recovery are still available.".into(),
                    );
                }
            }
        }
        Ok(self.text)
    }
}
enum Pending {
    Cpu(Receipt<ContextJob>),
    Between(Retained<JobOutcome<ContextJob>>),
    Io(Receipt<ResolveJob>),
}
struct Slot {
    key: ContextKey,
    column: usize,
    cwd: PathBuf,
    home: Option<PathBuf>,
    pending: Pending,
    history_context: Option<HistoryContext>,
    detectors: Arc<[crate::plugin_host::links::LinkDetector]>,
}
pub(crate) struct ContextCompletion {
    pub key: ContextKey,
    pub text: Result<Retained<ContextText>, String>,
}
pub(crate) struct PreparedLink {
    #[cfg(test)]
    pub worker_thread: std::thread::ThreadId,
    pub link: Option<crate::open_target::TerminalLink>,
    pub storage: Option<Arc<ilium_execution::StorageAdmission>>,
}
pub(crate) struct LinkRequest {
    pub id: u64,
    pub pane_id: NodeId,
    pub row: usize,
    pub column: usize,
    /// Link detectors of the enabled plugins (G5), the only source of detected links.
    pub detectors: Arc<[crate::plugin_host::links::LinkDetector]>,
}
pub(crate) struct LinkCompletion {
    pub id: u64,
    pub pane_id: NodeId,
    pub identity: Arc<()>,
    pub result: Result<Retained<PreparedLink>, String>,
}
struct LinkSlot {
    id: u64,
    pane_id: NodeId,
    identity: Arc<()>,
    receipt: Receipt<LinkJob>,
}
struct LinkJob {
    source: PreparationSnapshot,
    row: usize,
    column: usize,
    detectors: Arc<[crate::plugin_host::links::LinkDetector]>,
}
impl Job for LinkJob {
    type Output = PreparedLink;
    type Error = String;
    fn run(self, context: JobContext) -> Result<PreparedLink, String> {
        if context.stop_requested() {
            return Err("Terminal link preparation cancelled".into());
        }
        let (_, columns) = self.source.visible.size();
        let line = self
            .source
            .visible
            .rows(0, columns)
            .nth(self.row)
            .unwrap_or_default();
        // A plugin detector whose match covers the cell is the only detection (contract G5);
        // the OSC 8 hyperlinks the foreground program emitted are the fallback.
        let plugin_link =
            crate::plugin_host::links::plugin_link_at(&self.detectors, &line, self.column).map(
                |found| crate::open_target::TerminalLink::Plugin {
                    plugin: found.plugin,
                    detector_id: found.detector_id,
                    command: found.command,
                    text: found.text,
                },
            );
        let link = plugin_link.or_else(|| {
            let column = crate::terminal_view::cell_column_to_byte_offset(&line, self.column);
            self.source.links.iter().rev().find_map(|(label, target)| {
                let start = line.find(label)?;
                (start..start + label.len())
                    .contains(&column)
                    .then(|| crate::open_target::TerminalLink::Url(target.clone()))
            })
        });
        if context.stop_requested() {
            return Err("Terminal link preparation cancelled".into());
        }
        let bytes = std::mem::size_of::<PreparedLink>()
            + match &link {
                Some(crate::open_target::TerminalLink::Url(url)) => url.capacity(),
                Some(crate::open_target::TerminalLink::Plugin {
                    plugin,
                    detector_id,
                    command,
                    text,
                }) => {
                    plugin.capacity()
                        + detector_id.capacity()
                        + command.capacity()
                        + text.capacity()
                }
                None => 0,
            }
            + 128;
        let storage = if link.is_some() {
            Some(Arc::new(
                crate::execution::process_quota()
                    .reserve_external_storage(bytes)
                    .map_err(|reason| {
                        format!("Terminal link result storage admission: {reason:?}")
                    })?,
            ))
        } else {
            None
        };
        Ok(PreparedLink {
            link,
            storage,
            #[cfg(test)]
            worker_thread: std::thread::current().id(),
        })
    }
}
pub(crate) struct TerminalContextPreparation {
    client: Client,
    notification: Arc<Notify>,
    slot: Option<Slot>,
    links: Vec<LinkSlot>,
}
impl TerminalContextPreparation {
    pub(crate) fn new(client: Client) -> Self {
        let notification = Arc::new(Notify::new());
        let wake = notification.clone();
        Self {
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            slot: None,
            links: Vec::new(),
        }
    }
    pub(crate) fn notification(&self) -> Arc<Notify> {
        self.notification.clone()
    }
    pub(crate) fn request_link(
        &mut self,
        source: &PaintedTerminal,
        request: LinkRequest,
    ) -> Result<usize, String> {
        let LinkRequest {
            id,
            pane_id,
            row,
            column,
            detectors,
        } = request;
        if self.links.len() >= 8 {
            return Err(
                "Terminal link preparation queue is full; retry after pending clicks".into(),
            );
        }
        let (_, columns) = source.with_screen(|screen| screen.size());
        let result_bytes = usize::from(columns)
            .saturating_mul(128)
            .saturating_add(4096)
            .max(128 * 1024);
        let source_bytes = source.allocation_bytes();
        let cost = JobCost {
            input_bytes: source_bytes,
            result_bytes,
        };
        let reservation = self
            .client
            .try_reserve(Lane::Cpu, cost)
            .map_err(|reason| format!("Terminal link CPU admission: {reason:?}"))?;
        let job = LinkJob {
            source: source.preparation_snapshot()?,
            row,
            column,
            detectors,
        };
        let receipt = reservation.submit(job).map_err(|rejected| {
            format!(
                "Terminal link preparation submission: {:?}",
                rejected.reason
            )
        })?;
        self.links.push(LinkSlot {
            id,
            pane_id,
            identity: source.identity.clone(),
            receipt,
        });
        Ok(result_bytes)
    }
    pub(crate) fn cancel_link(&mut self, id: u64) {
        if let Some(index) = self.links.iter().position(|slot| slot.id == id) {
            self.links.remove(index).receipt.cancel();
        }
    }
    pub(crate) fn collect_link(&mut self) -> Option<LinkCompletion> {
        for index in 0..self.links.len() {
            let result = match self.links[index].receipt.try_take() {
                JobPoll::Pending => continue,
                JobPoll::Ready(outcome) => {
                    let error = match outcome.view() {
                        JobOutcome::Finished(Ok(_)) => None,
                        JobOutcome::Finished(Err(error)) => Some(error.clone()),
                        JobOutcome::NotStarted { .. } => {
                            Some("Terminal link preparation cancelled before execution".into())
                        }
                        JobOutcome::Panicked => Some("Terminal link preparation panicked".into()),
                    };
                    match error {
                        Some(error) => Err(error),
                        None => Ok(outcome.map(|outcome| match outcome {
                            JobOutcome::Finished(Ok(value)) => value,
                            _ => unreachable!("owned successful link discriminant changed"),
                        })),
                    }
                }
                JobPoll::Lost | JobPoll::Taken => {
                    Err("Terminal link preparation completion lost".into())
                }
            };
            let slot = self.links.remove(index);
            return Some(LinkCompletion {
                id: slot.id,
                pane_id: slot.pane_id,
                identity: slot.identity,
                result,
            });
        }
        None
    }
    pub(crate) fn request(
        &mut self,
        view: &PaintedTerminal,
        request: ContextRequest,
    ) -> Result<(), String> {
        let ContextRequest {
            key,
            source_row,
            column,
            selection,
            cwd,
            home,
            history_context,
            detectors,
        } = request;
        self.cancel();
        let cost = view.preparation_cost()?;
        let reservation = self
            .client
            .try_reserve(Lane::Cpu, cost)
            .map_err(|reason| format!("Context text preparation admission: {reason:?}"))?;
        let job = ContextJob {
            snapshot: view.preparation_snapshot()?,
            source_row,
            column,
            selection,
        };
        let receipt = reservation
            .submit(job)
            .map_err(|rejected| format!("Context text preparation: {:?}", rejected.reason))?;
        self.slot = Some(Slot {
            key,
            column,
            cwd,
            home,
            history_context,
            detectors,
            pending: Pending::Cpu(receipt),
        });
        Ok(())
    }
    pub(crate) fn collect(&mut self) -> Option<ContextCompletion> {
        let mut slot = self.slot.take()?;
        match &mut slot.pending {
            Pending::Cpu(receipt) => match receipt.try_take() {
                JobPoll::Pending => {
                    self.slot = Some(slot);
                    return None;
                }
                JobPoll::Ready(outcome) => slot.pending = Pending::Between(outcome),
                JobPoll::Lost | JobPoll::Taken => {
                    return Some(ContextCompletion {
                        key: slot.key,
                        text: Err("Context preparation completion was lost".into()),
                    });
                }
            },
            Pending::Io(receipt) => match receipt.try_take() {
                JobPoll::Pending => {
                    self.slot = Some(slot);
                    return None;
                }
                JobPoll::Ready(outcome) => return Some(completed(slot.key, outcome)),
                JobPoll::Lost | JobPoll::Taken => {
                    return Some(ContextCompletion {
                        key: slot.key,
                        text: Err("Context resolution completion was lost".into()),
                    });
                }
            },
            Pending::Between(_) => {}
        }
        let Pending::Between(outcome) = slot.pending else {
            self.slot = Some(slot);
            return None;
        };
        if !matches!(outcome.view(), JobOutcome::Finished(Ok(_))) {
            return Some(completed(slot.key, outcome));
        }
        // The CPU reservation remains inside the next output until the menu
        // releases its captured text. Only resolver metadata needs a new debit.
        let cost = JobCost {
            input_bytes: 8 * 1024 * 1024,
            result_bytes: 128 * 1024,
        };
        let reservation = match self.client.try_reserve(Lane::Io, cost) {
            Ok(value) => value,
            Err(
                RejectReason::Closed | RejectReason::InvalidCost | RejectReason::AccountingPoisoned,
            ) => {
                return Some(ContextCompletion {
                    key: slot.key,
                    text: Err("Context resolver unavailable".into()),
                });
            }
            Err(_) => {
                slot.pending = Pending::Between(outcome);
                self.slot = Some(slot);
                return None;
            }
        };
        let mut text = None;
        let source_hold = outcome.map(|outcome| {
            if let JobOutcome::Finished(Ok(value)) = outcome {
                text = Some(value);
            }
        });
        let Some(mut text) = text else {
            return Some(ContextCompletion {
                key: slot.key,
                text: Err("Context source unavailable".into()),
            });
        };
        text.source_hold = Some(source_hold);
        let job = ResolveJob {
            text,
            column: slot.column,
            cwd: slot.cwd.clone(),
            home: slot.home.clone(),
            history_context: slot.history_context.clone(),
            detectors: Arc::clone(&slot.detectors),
        };
        match reservation.submit(job) {
            Ok(receipt) => {
                slot.pending = Pending::Io(receipt);
                self.slot = Some(slot);
                None
            }
            Err(rejected) => Some(ContextCompletion {
                key: slot.key,
                text: Err(format!(
                    "Context resolver submission rejected: {:?}",
                    rejected.reason
                )),
            }),
        }
    }
    pub(crate) fn cancel(&mut self) {
        if let Some(slot) = self.slot.take() {
            match slot.pending {
                Pending::Cpu(receipt) => receipt.cancel(),
                Pending::Io(receipt) => receipt.cancel(),
                Pending::Between(_) => {}
            }
        }
    }
}
impl Drop for TerminalContextPreparation {
    fn drop(&mut self) {
        for slot in self.links.drain(..) {
            slot.receipt.cancel();
        }
        self.cancel();
    }
}

fn completed<J: Job<Output = ContextText, Error = String>>(
    key: ContextKey,
    outcome: Retained<JobOutcome<J>>,
) -> ContextCompletion {
    let error = match outcome.view() {
        JobOutcome::Finished(Ok(_)) => None,
        JobOutcome::Finished(Err(error)) => Some(error.clone()),
        JobOutcome::NotStarted { .. } => {
            Some("Context preparation cancelled before starting".into())
        }
        JobOutcome::Panicked => Some("Context preparation panicked".into()),
    };
    let text = if let Some(error) = error {
        Err(error)
    } else {
        Ok(outcome.map(|outcome| match outcome {
            JobOutcome::Finished(Ok(text)) => text,
            // The immutable discriminant was inspected immediately above.
            _ => unreachable!("successful context outcome changed during owned move"),
        }))
    };
    ContextCompletion { key, text }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_view::TerminalView;
    use std::time::{Duration, Instant};
    fn request_when_admitted(
        preparation: &mut TerminalContextPreparation,
        view: &TerminalView,
        request: impl Fn() -> ContextRequest,
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match preparation.request(&view.painted_source(), request()) {
                Ok(()) => return,
                Err(error)
                    if error == "Context text preparation admission: Busy"
                        && Instant::now() < deadline =>
                {
                    std::thread::yield_now();
                }
                Err(error) => panic!("context fixture admission failed: {error}"),
            }
        }
    }
    fn collect(preparation: &mut TerminalContextPreparation) -> ContextCompletion {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = preparation.collect() {
                return result;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn a_plugin_link_detector_is_the_only_source_of_detected_links() {
        let manifests = vec![(
            "example.issues".to_owned(),
            r##"{"contributes":{"commands":[{"id":"open"}],
                "link_detectors":[{"id":"issue-ref","regex":"#[0-9]+","command":"open"}]}}"##
                .to_owned(),
        )];
        let detectors: Arc<[crate::plugin_host::links::LinkDetector]> =
            crate::plugin_host::links::active_detectors(&manifests).into();
        let mut owner = TerminalContextPreparation::new(crate::execution::test_client());
        let mut view = TerminalView::new(4, 80);
        view.feed(b"see #42 in ./original.rs:12");
        let source = view.painted_source();
        for (id, column) in [(91, 5), (92, 15)] {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match owner.request_link(
                    &source,
                    LinkRequest {
                        id,
                        pane_id: NodeId(72),
                        row: 0,
                        column,
                        detectors: Arc::clone(&detectors),
                    },
                ) {
                    Ok(_) => break,
                    Err(error)
                        if error == "Terminal link CPU admission: Busy"
                            && Instant::now() < deadline =>
                    {
                        std::thread::yield_now()
                    }
                    Err(error) => panic!("link fixture admission: {error}"),
                }
            }
            let completion = loop {
                if let Some(completion) = owner.collect_link() {
                    break completion;
                }
                assert!(Instant::now() < deadline, "link completion deadline");
                std::thread::yield_now();
            };
            let output = completion.result.unwrap();
            let output = output.view();
            if id == 91 {
                assert_eq!(
                    output.link,
                    Some(crate::open_target::TerminalLink::Plugin {
                        plugin: "example.issues".to_owned(),
                        detector_id: "issue-ref".to_owned(),
                        command: "example.issues/open".to_owned(),
                        text: "#42".to_owned(),
                    })
                );
            } else {
                // No detector covers the cell, and the kernel detects no links itself.
                assert_eq!(output.link, None);
            }
        }
    }
    #[test]
    fn ctrl_link_cpu_job_preserves_original_source_path_offsets_and_no_link() {
        let mut owner = TerminalContextPreparation::new(crate::execution::test_client());
        let mut view = TerminalView::new(4, 80);
        view.feed(b"./original.rs:12:7 plain words");
        assert_eq!(b"./original.rs:12:7 plain words"[24], b' ');
        let source = view.painted_source();
        view.feed(b"\r\nhttps://newer.test");
        let detectors: Arc<[crate::plugin_host::links::LinkDetector]> =
            crate::plugin_host::links::active_detectors(&[(
                "ilium.core.terminal".to_owned(),
                include_str!("../../plugins/core/terminal/manifest.json").to_owned(),
            )])
            .into();
        for (id, column) in [(81, 4), (82, 24)] {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match owner.request_link(
                    &source,
                    LinkRequest {
                        id,
                        pane_id: NodeId(71),
                        row: 0,
                        column,
                        detectors: Arc::clone(&detectors),
                    },
                ) {
                    Ok(_) => break,
                    Err(error)
                        if error == "Terminal link CPU admission: Busy"
                            && Instant::now() < deadline =>
                    {
                        std::thread::yield_now()
                    }
                    Err(error) => panic!("link fixture admission: {error}"),
                }
            }
            let completion = loop {
                if let Some(completion) = owner.collect_link() {
                    break completion;
                }
                assert!(Instant::now() < deadline, "link completion deadline");
                std::thread::yield_now();
            };
            assert_eq!(completion.id, id);
            assert!(Arc::ptr_eq(&completion.identity, &view.identity));
            let output = completion.result.unwrap();
            let output = output.view();
            assert_ne!(output.worker_thread, std::thread::current().id());
            if id == 81 {
                assert_eq!(
                    output.link,
                    Some(crate::open_target::TerminalLink::Plugin {
                        plugin: "ilium.core.terminal".to_owned(),
                        detector_id: "path-line".to_owned(),
                        command: "ilium.core.terminal/link-open".to_owned(),
                        text: "./original.rs:12".to_owned(),
                    })
                );
                assert!(output.storage.is_some());
            } else {
                assert!(output.link.is_none());
            }
        }
    }

    #[test]
    fn click_snapshot_preserves_history_selection_and_osc_link_after_later_output() {
        let mut view = TerminalView::new(4, 60);
        view.feed(b"\x1b[32mold\x1b[0m\r\n\x1b]8;;https://example.test\x1b\\link\x1b]8;;\x1b\\");
        let mut preparation = TerminalContextPreparation::new(crate::execution::test_client());
        let identity = view.identity.clone();
        request_when_admitted(&mut preparation, &view, || ContextRequest {
            key: ContextKey {
                pane_id: NodeId(1),
                identity: identity.clone(),
                generation: 7,
                column: 0,
                row: 0,
                snapshot_ordinal: 0,
            },
            source_row: 1,
            column: 1,
            selection: None,
            cwd: PathBuf::from("/"),
            home: None,
            history_context: None,
            detectors: Arc::from(Vec::new()),
        });
        view.feed(b"\r\nnewer output");
        let completion = collect(&mut preparation);
        assert_eq!(completion.key.generation, 7);
        let text = completion.text.unwrap();
        assert!(text.view().full_history.contains("old"));
        assert!(!text.view().full_history.contains("newer output"));
        assert!(!text.view().full_history.contains('\x1b'));
        assert_eq!(text.view().source_line_text, "link");
        assert!(text.view().open_target.is_some());
        assert!(text.view().source_hold.is_some());
    }
    #[test]
    fn bounded_transcript_lookup_keeps_captured_recovery_text_available() {
        for class in [
            ilium_core::AgentClass::Codex,
            ilium_core::AgentClass::Claude,
        ] {
            let home = tempfile::tempdir().unwrap();
            let session = "11111111-1111-4111-8111-111111111111";
            // Claude derives its project directory name from the canonical cwd, which is
            // not "-" for "/" on every OS (Windows resolves it to a drive root).
            let path = match class {
                ilium_core::AgentClass::Codex => home
                    .path()
                    .join(".codex/sessions/2026/10/03")
                    .join(format!("rollout-2026-10-03T12-00-00-{session}.jsonl")),
                _ => ilium_agent_session::claude_transcript_path(
                    home.path(),
                    std::path::Path::new("/"),
                    session,
                ),
            };
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let metadata = match class {
                ilium_core::AgentClass::Codex => serde_json::json!({
                    "type": "session_meta", "payload": {"id": session, "cwd": "/"},
                    "synthetic_fixture": "x".repeat(65 * 1024)
                }),
                _ => serde_json::json!({
                    "type": "user", "sessionId": session, "cwd": "/",
                    "message": {"content": "x".repeat(65 * 1024)},
                    "synthetic_fixture": true
                }),
            };
            std::fs::write(&path, format!("{metadata}\n")).unwrap();
            let mut view = TerminalView::new(4, 60);
            view.feed("FATAL café 日本語 🦀".as_bytes());
            let mut preparation = TerminalContextPreparation::new(crate::execution::test_client());
            request_when_admitted(&mut preparation, &view, || ContextRequest {
                key: ContextKey {
                    pane_id: NodeId(1),
                    identity: view.identity.clone(),
                    generation: 1,
                    column: 0,
                    row: 0,
                    snapshot_ordinal: 0,
                },
                source_row: 0,
                column: 0,
                selection: None,
                cwd: PathBuf::from("/"),
                home: Some(home.path().to_owned()),
                history_context: Some((class.clone(), session.into(), PathBuf::from("/"), None)),
                detectors: Arc::from(Vec::new()),
            });
            let completion = collect(&mut preparation);
            let text = completion.text.unwrap_or_else(|error| {
                    panic!("bounded transcript lookup must preserve captured recovery text for {class:?}: {error}")
                });
            assert!(
                text.view().warning.as_deref().is_some_and(
                    |warning| warning.contains("history scan exceeded its safety limit")
                ),
                "both provider fixtures must actually reach the bounded scan failure"
            );
            assert_eq!(text.view().source_line_text, "FATAL café 日本語 🦀");
            assert!(text
                .view()
                .visible_contents
                .contains("FATAL café 日本語 🦀"));
            assert!(text.view().full_history.contains("FATAL café 日本語 🦀"));
            assert!(
                text.view().history_path.is_none(),
                "partial scan must never authorize a transcript path"
            );
            assert!(
                text.view().source_hold.is_some(),
                "captured text keeps its execution credit"
            );
            assert_eq!(
                std::fs::read_to_string(path).unwrap(),
                format!("{metadata}\n")
            );
        }
    }
}
