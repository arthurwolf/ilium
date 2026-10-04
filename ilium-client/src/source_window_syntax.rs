//! Sequential physical-line syntax for immutable authored-source windows.
//! Text is copied in64KiB UTF-safe pages, assembled and parsed only on CPU.
//! A physical line is NEVER presented to syntect as independent page fragments.
use crate::{document_preparation::PreparationKey, source_stream::Viewport};
use ilium_core::NodeId;
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, RejectReason, Retiring,
    RetiringArc, StorageAdmission,
};
use ratatui::style::Style;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Notify;
const PAGE: usize = 64 * 1024;
const PARSER_BYTES: usize = 32 * 1024 * 1024;
pub(crate) struct WindowStyles {
    pub rows: Vec<Vec<Style>>,
    _worker_thread: Option<std::thread::ThreadId>,
    _allocation: Arc<StorageAdmission>,
}
struct State {
    key: PreparationKey,
    base: RetiringArc<Viewport>,
    physical: usize,
    byte: usize,
    line: Option<String>,
    line_capacity: usize,
    line_allocation: Option<Arc<StorageAdmission>>,
    recognized: bool,
    initialized: bool,
    styles: Vec<Vec<Style>>,
    prefix: Vec<String>,
    prefix_allocations: Vec<Arc<StorageAdmission>>,
    prefix_bytes: usize,
    fixed_bytes: usize,
    _worker_thread: Option<std::thread::ThreadId>,
    styles_allocation: Arc<StorageAdmission>,
    _parser_allocation: Arc<StorageAdmission>,
    #[cfg(test)]
    destruction_probe: Option<DestructionProbe>,
}
#[cfg(test)]
struct DestructionProbe(std::sync::mpsc::Sender<std::thread::ThreadId>);
#[cfg(test)]
impl Drop for DestructionProbe {
    fn drop(&mut self) {
        let _ = self.0.send(std::thread::current().id());
    }
}
struct SyntaxJob {
    state: Retiring<State>,
    text: Option<Retiring<String>>,
    end_of_line: bool,
    initialize_only: bool,
}
enum Continuation {
    State(Retiring<State>),
    Done(RetiringArc<WindowStyles>),
}
fn finish_on_cpu(state: Retiring<State>) -> Result<Continuation, String> {
    let styles = state
        .try_map_on_cpu(|state| WindowStyles {
            rows: state.styles,
            _worker_thread: state._worker_thread,
            _allocation: state.styles_allocation,
        })
        .map_err(|_state| "source syntax finish has no CPU owner".to_owned())?;
    Ok(Continuation::Done(Arc::new(styles)))
}
impl Job for SyntaxJob {
    type Output = Continuation;
    type Error = String;
    fn run(mut self, context: JobContext) -> Result<Continuation, String> {
        let state: &mut State = &mut self.state;
        state._worker_thread = Some(std::thread::current().id());
        if context.stop_requested() {
            return Err("window syntax cancelled".into());
        }
        if !state.initialized {
            // The pinned Oniguruma ParseState contains raw Region pointers.
            // Its parser is created and destroyed only inside THIS CPU call;
            // the typed continuation contains only Send strings/guards.
            state.recognized = crate::syntax::SequentialHighlight::new(&state.key.path).is_some();
            state.initialized = true;
            let count = state
                .base
                .rows
                .last()
                .map_or(0, |row| row.physical.saturating_add(1));
            state.prefix = Vec::with_capacity(count);
            state.prefix_allocations = Vec::with_capacity(count);
            state.styles = state
                .base
                .rows
                .iter()
                .map(|row| vec![Style::new(); row.glyphs.len()])
                .collect();
            if !state.recognized {
                return finish_on_cpu(self.state);
            }
        }
        if self.initialize_only {
            return Ok(Continuation::State(self.state));
        }
        if state.line.is_none() {
            state.line = Some(String::with_capacity(state.line_capacity));
        }
        let line_capacity = state.line_capacity;
        let line = state
            .line
            .as_mut()
            .ok_or("source syntax line assembly owner unavailable")?;
        let text = self
            .text
            .as_deref()
            .ok_or("source syntax capture missing")?;
        if line
            .len()
            .checked_add(text.len())
            .is_none_or(|bytes| bytes > line_capacity)
        {
            return Err("original syntax line revision changed".into());
        }
        line.push_str(text);
        state.byte += text.len();
        if self.end_of_line {
            let line = state
                .line
                .take()
                .ok_or("physical-line syntax original unavailable")?;
            state.prefix_bytes = state
                .prefix_bytes
                .checked_add(line.len())
                .ok_or("syntax prefix length overflow")?;
            state.prefix.push(line);
            state.prefix_allocations.push(
                state
                    .line_allocation
                    .take()
                    .ok_or("syntax physical-line allocation unavailable")?,
            );
            state.physical += 1;
            state.byte = 0;
        }
        let until = state
            .base
            .rows
            .last()
            .map_or(0, |row| row.physical.saturating_add(1));
        let done = state.physical >= until;
        if done {
            if let Some(mut parser) = crate::syntax::SequentialHighlight::new(&state.key.path) {
                for (physical, line) in state.prefix.iter().enumerate() {
                    if context.stop_requested() {
                        return Err("source syntax prefix cancelled".into());
                    }
                    parser.style_window_line(
                        line,
                        physical,
                        &state.base.rows,
                        &mut state.styles,
                    )?;
                }
            }
            // Native parser, original prefix buffers and their source debits
            // retire before returning a visible style projection to the UI.
            state.prefix.clear();
            state.prefix_allocations.clear();
        }
        if done {
            finish_on_cpu(self.state)
        } else {
            Ok(Continuation::State(self.state))
        }
    }
}
enum Pending {
    State(Retiring<State>),
    Captured(SyntaxJob, JobCost),
    Running(Receipt<SyntaxJob>),
    Done,
}
struct Slot {
    id: NodeId,
    key: PreparationKey,
    base: RetiringArc<Viewport>,
    pending: Option<Pending>,
    retry_at: Option<Instant>,
    error: Option<String>,
}
pub(crate) struct StyledWindow {
    pub id: NodeId,
    pub key: PreparationKey,
    pub base: RetiringArc<Viewport>,
    pub styles: RetiringArc<WindowStyles>,
}
struct PrefixPreflight {
    id: NodeId,
    key: PreparationKey,
    base: RetiringArc<Viewport>,
    next: usize,
    bytes: usize,
}
pub(crate) struct SourceWindowSyntax {
    budget: Arc<crate::editor_capture_budget::CaptureBudget>,
    client: Client,
    slots: Vec<Slot>,
    preflights: Vec<PrefixPreflight>,
}
fn terminal(reason: RejectReason) -> bool {
    matches!(
        reason,
        RejectReason::Closed | RejectReason::InvalidCost | RejectReason::AccountingPoisoned
    )
}
impl SourceWindowSyntax {
    pub fn new(client: Client, notification: Arc<Notify>) -> Self {
        Self {
            budget: Arc::new(crate::editor_capture_budget::CaptureBudget::new()),
            client: client.with_completion_wake(move || notification.notify_one()),
            slots: Vec::with_capacity(4),
            preflights: Vec::with_capacity(4),
        }
    }
    pub fn set_capture_budget(&mut self, budget: Arc<crate::editor_capture_budget::CaptureBudget>) {
        self.budget = budget;
    }
    #[cfg(test)]
    pub fn begin_capture_turn(&self) {
        self.budget.begin_turn();
    }
    pub fn request(
        &mut self,
        id: NodeId,
        key: PreparationKey,
        base: RetiringArc<Viewport>,
        lines: &[String],
    ) -> Result<(), String> {
        self.preflights.retain(|pending| {
            pending.id != id
                || pending.key.same_geometry(&key) && Arc::ptr_eq(&pending.base.rows, &base.rows)
        });
        self.slots.retain_mut(|slot| {
            if slot.id != id
                || slot.key.same_geometry(&key) && Arc::ptr_eq(&slot.base.rows, &base.rows)
            {
                return true;
            }
            self.budget
                .cancel(id, crate::editor_capture_budget::Kind::Syntax);
            if let Some(Pending::Running(receipt)) = &slot.pending {
                receipt.cancel();
            }
            false
        });
        if !self.slots.iter().any(|slot| slot.id == id) {
            if self.slots.len() == 4 {
                return Err("visible source syntax admission is full".into());
            }
            let prefix_count = base
                .rows
                .last()
                .map_or(0, |row| row.physical.saturating_add(1));
            // Length metadata is also bounded UI work. The historical proposal
            // summed every preceding line in one turn before copying; this
            // cursor uses the same fair capture credit, visiting <=1024 lines.
            if !self.preflights.iter().any(|pending| pending.id == id) {
                if self.preflights.len() == 4 {
                    return Err("source syntax prefix preflight is full".into());
                }
                self.preflights.push(PrefixPreflight {
                    id,
                    key: key.clone(),
                    base: base.clone(),
                    next: 0,
                    bytes: 0,
                });
            }
            let pending = self
                .preflights
                .iter_mut()
                .find(|pending| pending.id == id)
                .ok_or("syntax preflight owner missing")?;
            if pending.next < prefix_count {
                let Some(credit) =
                    self.budget
                        .take(id, crate::editor_capture_budget::Kind::Syntax, 1024)
                else {
                    return Ok(());
                };
                let end = pending.next.saturating_add(credit.lines).min(prefix_count);
                let start = pending.next;
                for physical in start..end {
                    let line = lines
                        .get(physical)
                        .ok_or("syntax prefix revision changed")?;
                    pending.bytes = pending
                        .bytes
                        .checked_add(line.len())
                        .ok_or("source syntax retirement prefix overflow")?;
                    pending.next += 1;
                }
                self.budget.finish(
                    credit,
                    (end - start) * std::mem::size_of::<usize>(),
                    end - start,
                );
                if pending.next < prefix_count {
                    return Ok(());
                }
            }
            let prefix_bytes = pending.bytes;
            let quota = self.client.quota_group();
            let fixed_bytes = prefix_count
                .checked_mul(
                    std::mem::size_of::<String>() + std::mem::size_of::<Arc<StorageAdmission>>(),
                )
                .and_then(|bytes| bytes.checked_add(PARSER_BYTES + 4096))
                .ok_or("source syntax prefix metadata overflow")?;
            let parser_allocation = Arc::new(
                quota
                    .reserve_external_storage(fixed_bytes)
                    .map_err(|reason| format!("source parser-state admission: {reason:?}"))?,
            );
            let cells = usize::from(key.width)
                .checked_mul(usize::from(key.height))
                .ok_or("source style capacity overflow")?;
            let styles_bytes = cells
                .checked_mul(std::mem::size_of::<Style>())
                .and_then(|bytes| {
                    bytes.checked_add(
                        usize::from(key.height) * std::mem::size_of::<Vec<Style>>() + 4096,
                    )
                })
                .ok_or("source style capacity overflow")?;
            let styles_allocation = Arc::new(
                quota
                    .reserve_external_storage(styles_bytes)
                    .map_err(|reason| format!("source visible-style admission: {reason:?}"))?,
            );
            // Reserve final-owner custody before constructing mutable prefix,
            // line and style buffers. The prefix bound is taken from this exact
            // authored revision; each original per-line guard remains in State.
            let retirement_bytes = prefix_bytes
                .checked_mul(2)
                .and_then(|bytes| bytes.checked_add(fixed_bytes))
                .and_then(|bytes| bytes.checked_add(styles_bytes))
                .and_then(|bytes| bytes.checked_add(PAGE + 4096))
                .ok_or("source syntax retirement declaration overflow")?;
            let retirement = self
                .client
                .retirement()
                .try_reserve::<State>(retirement_bytes)
                .map_err(|reason| format!("source syntax retirement admission: {reason:?}"))?;
            // This metadata map contains only actual visible glyphs, never a
            // token/string for every hidden source row or wrapped visual row.
            let styles = Vec::new();
            let state = State {
                key: key.clone(),
                base: base.clone(),
                physical: 0,
                byte: 0,
                line: None,
                line_capacity: 0,
                line_allocation: None,
                recognized: false,
                initialized: false,
                styles,
                prefix: Vec::new(),
                prefix_allocations: Vec::new(),
                prefix_bytes: 0,
                fixed_bytes,
                _worker_thread: None,
                styles_allocation,
                _parser_allocation: parser_allocation,
                #[cfg(test)]
                destruction_probe: None,
            };
            self.preflights.retain(|pending| pending.id != id);
            self.slots.push(Slot {
                id,
                key,
                base,
                pending: Some(Pending::State(retirement.attach(state))),
                retry_at: None,
                error: None,
            });
        }
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.id == id)
            .ok_or("source syntax owner unavailable")?;
        if let Some(error) = &slot.error {
            return Err(error.clone());
        }
        let Some(pending) = slot.pending.take() else {
            return Ok(());
        };
        let Pending::State(mut state) = pending else {
            slot.pending = Some(pending);
            return Ok(());
        };
        if slot.retry_at.is_some_and(|at| at > Instant::now()) {
            slot.pending = Some(Pending::State(state));
            return Ok(());
        }
        if !state.initialized {
            let cost = JobCost {
                input_bytes: state.fixed_bytes + PAGE,
                result_bytes: PAGE + 4096,
            };
            slot.pending = Some(Pending::Captured(
                SyntaxJob {
                    state,
                    text: None,
                    end_of_line: false,
                    initialize_only: true,
                },
                cost,
            ));
            slot.retry_at = Some(Instant::now());
            return Ok(());
        }
        let Some(line) = lines.get(state.physical) else {
            slot.pending = Some(Pending::Done);
            return Ok(());
        };
        if state.line_allocation.is_none() {
            let bytes = line
                .len()
                .checked_mul(2)
                .and_then(|bytes| bytes.checked_add(PAGE + 4096))
                .ok_or("source syntax line capacity overflow")?;
            match self.client.quota_group().reserve_external_storage(bytes) {
                Ok(allocation) => {
                    state.line_capacity = line.len();
                    state.line_allocation = Some(Arc::new(allocation));
                }
                Err(reason) => {
                    slot.pending = Some(Pending::State(state));
                    slot.retry_at = Some(Instant::now() + Duration::from_millis(20));
                    return Err(format!("source original-line syntax admission: {reason:?}"));
                }
            }
        }
        if state.byte > line.len() || !line.is_char_boundary(state.byte) {
            slot.error = Some("source syntax revision changed".into());
            return Err("source syntax revision changed".into());
        }
        let Some(credit) = self
            .budget
            .take(id, crate::editor_capture_budget::Kind::Syntax, 1)
        else {
            slot.pending = Some(Pending::State(state));
            slot.retry_at = Some(Instant::now() + Duration::from_millis(20));
            return Ok(());
        };
        let mut end = state
            .byte
            .saturating_add(PAGE.min(credit.bytes))
            .min(line.len());
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        let cost = JobCost {
            input_bytes: state
                .prefix_bytes
                .checked_add(state.line_capacity)
                .and_then(|bytes| bytes.checked_mul(2))
                .and_then(|bytes| bytes.checked_add(state.fixed_bytes + PAGE))
                .ok_or("source syntax CPU cost overflow")?,
            result_bytes: PAGE + 4096,
        };
        let maximum = match self.client.maximum_cpu_job_cost() {
            Ok(maximum) => maximum,
            Err(reason) => {
                self.budget.finish(credit, 0, 0);
                let message = format!("source syntax CPU unavailable: {reason:?}");
                slot.pending = Some(Pending::State(state));
                if terminal(reason) {
                    slot.error = Some(message.clone());
                    slot.retry_at = None;
                } else {
                    slot.retry_at = Some(Instant::now() + Duration::from_millis(20));
                }
                return Err(message);
            }
        };
        if cost.input_bytes > maximum.input_bytes || cost.result_bytes > maximum.result_bytes {
            self.budget.finish(credit, 0, 0);
            slot.pending = Some(Pending::State(state));
            slot.error=Some("Source text remains available; exact physical-line syntax exceeds current CPU admission".into());
            return Err(slot.error.clone().unwrap_or_default());
        }
        let text_retirement = match self.client.retirement().try_reserve::<String>(PAGE + 4096) {
            Ok(retirement) => retirement,
            Err(reason) => {
                self.budget.finish(credit, 0, 0);
                let message = format!("source syntax captured-page retirement: {reason:?}");
                slot.pending = Some(Pending::State(state));
                if terminal(reason) {
                    slot.error = Some(message.clone());
                    slot.retry_at = None;
                } else {
                    slot.retry_at = Some(Instant::now() + Duration::from_millis(20));
                }
                return Err(message);
            }
        };
        let text = text_retirement.attach(line[state.byte..end].to_owned());
        self.budget
            .finish(credit, text.len(), usize::from(end == line.len()));
        slot.pending = Some(Pending::Captured(
            SyntaxJob {
                state,
                text: Some(text),
                end_of_line: end == line.len(),
                initialize_only: false,
            },
            cost,
        ));
        slot.retry_at = Some(Instant::now());
        Ok(())
    }
    pub fn collect(&mut self) -> Vec<StyledWindow> {
        let mut completed = Vec::with_capacity(4);
        let now = Instant::now();
        for slot in &mut self.slots {
            let Some(pending) = slot.pending.take() else {
                continue;
            };
            match pending {
                Pending::Captured(job, cost) => {
                    if slot.retry_at.is_some_and(|at| at > now) {
                        slot.pending = Some(Pending::Captured(job, cost));
                        continue;
                    }
                    match self.client.try_reserve(Lane::Cpu, cost) {
                        Ok(reservation) => match reservation.submit(job) {
                            Ok(receipt) => {
                                slot.pending = Some(Pending::Running(receipt));
                                slot.retry_at = None;
                            }
                            Err(rejected) => {
                                if terminal(rejected.reason) {
                                    slot.error = Some(format!(
                                        "source syntax submission: {:?}",
                                        rejected.reason
                                    ));
                                    slot.retry_at = None;
                                } else {
                                    slot.pending = Some(Pending::Captured(rejected.value, cost));
                                    slot.retry_at = Some(now + Duration::from_millis(20));
                                }
                            }
                        },
                        Err(reason) => {
                            if terminal(reason) {
                                slot.error = Some(format!("source syntax admission: {reason:?}"));
                                slot.retry_at = None;
                            } else {
                                slot.pending = Some(Pending::Captured(job, cost));
                                slot.retry_at = Some(now + Duration::from_millis(20));
                            }
                        }
                    }
                }
                Pending::Running(mut receipt) => match receipt.try_take() {
                    JobPoll::Pending => slot.pending = Some(Pending::Running(receipt)),
                    JobPoll::Ready(retained) => {
                        let (outcome, hold) = retained.into_parts();
                        match outcome {
                            JobOutcome::Finished(Ok(continuation)) => {
                                match continuation {
                                    Continuation::Done(styles) => {
                                        completed.push(StyledWindow {
                                            id: slot.id,
                                            key: slot.key.clone(),
                                            base: slot.base.clone(),
                                            styles,
                                        });
                                        slot.pending = Some(Pending::Done);
                                    }
                                    Continuation::State(state) => {
                                        slot.pending = Some(Pending::State(state));
                                    }
                                }
                                slot.retry_at = None;
                            }
                            JobOutcome::Finished(Err(error)) => slot.error = Some(error),
                            JobOutcome::NotStarted { mut job, .. } => {
                                job.state.set_retention(hold.clone());
                                if let Some(text) = &mut job.text {
                                    text.set_retention(hold.clone());
                                }
                                drop(job);
                                slot.error =
                                    Some("Source window syntax cancelled before execution".into());
                            }
                            _ => {
                                slot.error = Some("Source window syntax cancelled or failed".into())
                            }
                        }
                        drop(hold);
                    }
                    _ => slot.error = Some("Source window syntax receipt lost".into()),
                },
                other => slot.pending = Some(other),
            }
        }
        completed
    }
    pub fn retry_delay(&self, now: Instant) -> Option<Duration> {
        if !self.preflights.is_empty() {
            return Some(Duration::from_millis(20));
        }
        self.slots
            .iter()
            .filter_map(|slot| slot.retry_at.map(|at| at.saturating_duration_since(now)))
            .min()
    }
    pub fn retain_visible(&mut self, visible: &[NodeId]) {
        self.preflights
            .retain(|pending| visible.contains(&pending.id));
        for slot in &self.slots {
            if !visible.contains(&slot.id) {
                self.budget
                    .cancel(slot.id, crate::editor_capture_budget::Kind::Syntax);
            }
        }
        self.slots.retain(|slot| {
            if visible.contains(&slot.id) {
                return true;
            }
            if let Some(Pending::Running(receipt)) = &slot.pending {
                receipt.cancel();
            }
            false
        });
    }
    pub fn cancel_pane(&mut self, id: NodeId) {
        self.budget
            .cancel(id, crate::editor_capture_budget::Kind::Syntax);
        self.preflights.retain(|pending| pending.id != id);
        self.slots.retain(|slot| {
            if slot.id != id {
                return true;
            }
            if let Some(Pending::Running(receipt)) = &slot.pending {
                receipt.cancel();
            }
            false
        });
    }
    pub fn cancel(&mut self) {
        self.retain_visible(&[]);
    }
}

impl Drop for SourceWindowSyntax {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unpublished_mutable_syntax_state_retires_on_existing_cpu_bank() {
        let client = crate::execution::test_document_client();
        let mut editor = crate::editor_pane::EditorPane::empty();
        editor.path = Some(std::path::PathBuf::from("cancelled.rs"));
        editor.textarea = ratatui_textarea::TextArea::new(vec!["let value = 1;".into()]);
        editor.set_preparation_height(2);
        let picker = ratatui_image::picker::Picker::halfblocks();
        let key = editor.window_preparation_key(80, &picker).unwrap();
        let allocation = Arc::new(client.quota_group().reserve_external_storage(4096).unwrap());
        let rows = client
            .retirement()
            .try_reserve::<Vec<crate::source_stream::Row>>(4096)
            .unwrap()
            .attach_shared(Vec::new());
        let base = client
            .retirement()
            .try_reserve::<Viewport>(4096)
            .unwrap()
            .attach_shared(Viewport {
                identity: Arc::new(()),
                top: 0,
                left: 0,
                rows,
                total_rows: Some(0),
                scanned_until: crate::source_stream::Cursor {
                    physical: 0,
                    byte: 0,
                },
                minimap: None,
                styles: None,
                allocation,
            });
        let mut syntax = SourceWindowSyntax::new(client, Arc::new(Notify::new()));
        syntax
            .request(NodeId(1), key, base, editor.textarea.lines())
            .unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let guard = match syntax.slots[0].pending.as_mut().unwrap() {
            Pending::Captured(job, _) => {
                job.state.destruction_probe = Some(DestructionProbe(sender));
                Arc::downgrade(&job.state.styles_allocation)
            }
            _ => panic!("initial syntax state was not captured"),
        };
        let ui_thread = std::thread::current().id();
        syntax.cancel();
        assert_ne!(
            receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
            ui_thread
        );
        assert!(guard.upgrade().is_none());
    }
    #[test]
    fn real_bank_window_and_syntax_share_capture_budget_without_copying_original_source() {
        use crate::editor_capture_budget::{CaptureBudget, Kind};
        let client = crate::execution::test_document_client();
        let mut editor = crate::editor_pane::EditorPane::empty();
        editor.path = Some(std::path::PathBuf::from("shared.rs"));
        editor.show_line_numbers = false;
        editor.line_display = crate::config::LineDisplay::Wrap;
        editor.textarea =
            ratatui_textarea::TextArea::new(vec![format!("/*{}*/", "x".repeat(100000))]);
        editor.set_preparation_height(4);
        let original = editor.textarea.lines()[0].as_ptr();
        let picker = ratatui_image::picker::Picker::halfblocks();
        let key = editor.window_preparation_key(80, &picker).unwrap();
        let pane = NodeId(1);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut bootstrap =
            crate::source_window_preparation::SourceWindowPreparation::new(client.clone());
        let base = loop {
            bootstrap.begin_capture_turn();
            bootstrap
                .request(pane, key.clone(), 0, editor.textarea.lines())
                .unwrap();
            if let Some(completion) = bootstrap.collect().pop() {
                break completion.viewport;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        bootstrap.cancel();
        drop(bootstrap);
        let budget = Arc::new(CaptureBudget::new());
        let mut syntax = SourceWindowSyntax::new(client.clone(), Arc::new(Notify::new()));
        syntax.set_capture_budget(budget.clone());
        syntax
            .request(pane, key.clone(), base.clone(), editor.textarea.lines())
            .unwrap();
        loop {
            syntax.collect();
            if matches!(&syntax.slots[0].pending, Some(Pending::State(state)) if state.initialized)
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let mut windows = crate::source_window_preparation::SourceWindowPreparation::new(client);
        windows.set_capture_budget(budget.clone());
        budget.begin_turn();
        windows
            .request(pane, key.clone(), 0, editor.textarea.lines())
            .unwrap();
        syntax
            .request(pane, key, base, editor.textarea.lines())
            .unwrap();
        // Reserve the parent-owned lanes' exact remaining credits. Their adapter
        // bodies are deliberately not invented in this current window fixture.
        let whole = budget.take(pane, Kind::Whole, 1).unwrap();
        let context = budget.take(pane, Kind::Context, 1).unwrap();
        assert_eq!(whole.bytes, 64 * 1024);
        assert_eq!(context.bytes, 64 * 1024);
        assert_eq!(budget.remaining().0, 0);
        assert!(budget.take(NodeId(2), Kind::Window, 1).is_none());
        assert_eq!(editor.textarea.lines()[0].as_ptr(), original);
        assert_eq!(editor.textarea.lines()[0].len(), 100004);
        windows.cancel();
        syntax.cancel();
        budget.begin_turn();
        assert!(budget.take(NodeId(2), Kind::Window, 1).is_some());
    }
    #[test]
    fn real_bank_window_syntax_preserves_multiline_scope_and_unsplit_70k_line() {
        let client = crate::execution::test_document_client();
        let mut editor = crate::editor_pane::EditorPane::empty();
        editor.path = Some(std::path::PathBuf::from("scope.rs"));
        editor.show_line_numbers = false;
        editor.line_display = crate::config::LineDisplay::Wrap;
        editor.textarea = ratatui_textarea::TextArea::new(vec![
            "/* opening".into(),
            format!("{}*/ let answer = 42;", "x".repeat(70000)),
        ]);
        editor.set_preparation_height(4);
        let picker = ratatui_image::picker::Picker::halfblocks();
        let key = editor.window_preparation_key(80, &picker).unwrap();
        let mut windows =
            crate::source_window_preparation::SourceWindowPreparation::new(client.clone());
        let deadline = Instant::now() + Duration::from_secs(10);
        let base = loop {
            windows.begin_capture_turn();
            windows
                .request(NodeId(1), key.clone(), 0, editor.textarea.lines())
                .unwrap();
            if let Some(completion) = windows.collect().pop() {
                break completion.viewport;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        let mut syntax = SourceWindowSyntax::new(client, Arc::new(Notify::new()));
        let styled = loop {
            syntax.begin_capture_turn();
            syntax
                .request(
                    NodeId(1),
                    key.clone(),
                    base.clone(),
                    editor.textarea.lines(),
                )
                .unwrap();
            if let Some(completion) = syntax.collect().pop() {
                break completion.styles;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        let oracle =
            crate::syntax::highlight(std::path::Path::new("scope.rs"), editor.textarea.lines())
                .unwrap();
        for (row, styles) in base.rows.iter().zip(&styled.rows) {
            for (glyph, style) in row.glyphs.iter().zip(styles) {
                let expected = oracle[row.physical]
                    .iter()
                    .find(|(range, _)| range.contains(&glyph.byte))
                    .unwrap()
                    .1;
                assert_eq!(
                    *style, expected,
                    "source physical {} byte {}",
                    row.physical, glyph.byte
                );
            }
        }
        assert_ne!(styled._worker_thread, Some(std::thread::current().id()));
        let held = Arc::downgrade(&styled._allocation);
        syntax.cancel();
        drop(syntax);
        assert!(held.upgrade().is_some());
        drop(styled);
        let retirement_deadline = Instant::now() + Duration::from_secs(3);
        while held.upgrade().is_some() && Instant::now() < retirement_deadline {
            std::thread::yield_now();
        }
        assert!(held.upgrade().is_none());
    }
}
