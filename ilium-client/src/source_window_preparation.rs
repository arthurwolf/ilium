//! Revision-bound authored-source continuation using the SAME document bank.
use crate::{
    document_preparation::PreparationKey,
    source_stream::{Chunk, ContinueJob, FrameCustody, Stream, Viewport, CHUNK},
};
use ilium_core::NodeId;
use ilium_execution::{
    Client, JobCost, JobOutcome, JobPoll, Lane, Receipt, RejectReason, Retiring, RetiringArc,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Notify;
const COST: JobCost = JobCost {
    input_bytes: crate::presentation::MAX_FRAME_BYTES + STATE_BYTES,
    result_bytes: crate::presentation::MAX_FRAME_BYTES,
};
const STATE_BYTES: usize = crate::source_stream::STATE_STORAGE_BYTES;
const FRAME_BYTES: usize = crate::presentation::MAX_FRAME_BYTES;
enum Pending {
    State(Retiring<Stream>),
    Captured(ContinueJob),
    Running(Receipt<ContinueJob>),
    Finished(Retiring<Stream>),
}
struct Slot {
    id: NodeId,
    key: PreparationKey,
    pending: Option<Pending>,
    wanted: PreparationKey,
    left: usize,
    wanted_left: usize,
    retry_at: Option<Instant>,
    error: Option<String>,
}
pub(crate) struct WindowCompletion {
    pub id: NodeId,
    pub key: PreparationKey,
    pub viewport: RetiringArc<Viewport>,
}
pub(crate) struct SourceWindowPreparation {
    budget: Arc<crate::editor_capture_budget::CaptureBudget>,
    client: Client,
    notification: Arc<Notify>,
    slots: Vec<Slot>,
}
fn terminal(reason: RejectReason) -> bool {
    matches!(
        reason,
        RejectReason::Closed | RejectReason::InvalidCost | RejectReason::AccountingPoisoned
    )
}
impl SourceWindowPreparation {
    pub fn new(client: Client) -> Self {
        Self::with_notification(client, Arc::new(Notify::new()))
    }
    pub fn with_notification(client: Client, notification: Arc<Notify>) -> Self {
        let wake = notification.clone();
        Self {
            budget: Arc::new(crate::editor_capture_budget::CaptureBudget::new()),
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            slots: Vec::with_capacity(4),
        }
    }
    pub fn set_capture_budget(&mut self, budget: Arc<crate::editor_capture_budget::CaptureBudget>) {
        self.budget = budget;
    }
    #[cfg(test)]
    pub fn begin_capture_turn(&self) {
        self.budget.begin_turn();
    }
    pub fn cancel_pane(&mut self, id: NodeId) {
        self.budget
            .cancel(id, crate::editor_capture_budget::Kind::Window);
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
    pub fn notification(&self) -> Arc<Notify> {
        self.notification.clone()
    }
    pub fn retry_delay(&self, now: Instant) -> Option<Duration> {
        self.slots
            .iter()
            .filter_map(|slot| slot.retry_at.map(|at| at.saturating_duration_since(now)))
            .min()
    }
    pub fn retain_visible(&mut self, visible: &[NodeId]) {
        self.budget.retain_visible(visible);
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
    /// At most ONE64KiB chunk is copied per pane per event-loop turn. Selection
    /// and viewport changes only update wanted geometry while CPU owns state.
    /// Content/width/tab changes cancel; already painted old windows survive.
    pub fn request(
        &mut self,
        id: NodeId,
        key: PreparationKey,
        left: usize,
        lines: &[String],
    ) -> Result<(), String> {
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.id == id && !slot.key.same_geometry(&key))
        {
            if let Some(Pending::Running(receipt)) = &self.slots[index].pending {
                receipt.cancel();
            }
            self.budget
                .cancel(id, crate::editor_capture_budget::Kind::Window);
            self.slots.swap_remove(index);
        }
        if !self.slots.iter().any(|slot| slot.id == id) {
            if self.slots.len() == 4 {
                return Err("source window visible-pane admission is full".into());
            }
            let retirement = self
                .client
                .retirement()
                .try_reserve::<Stream>(STATE_BYTES + FRAME_BYTES)
                .map_err(|reason| format!("source mutable-stream retirement: {reason:?}"))?;
            let quota = self.client.quota_group();
            let state = Arc::new(
                quota
                    .reserve_external_storage(STATE_BYTES)
                    .map_err(|reason| format!("source continuation storage: {reason:?}"))?,
            );
            let frame = Arc::new(
                quota
                    .reserve_external_storage(FRAME_BYTES)
                    .map_err(|reason| format!("source window storage: {reason:?}"))?,
            );
            let frame_custody = FrameCustody::reserve(&self.client)
                .map_err(|reason| format!("source viewport retirement: {reason:?}"))?;
            let gutter = if key.gutter {
                crate::editor_highlight::line_number_gutter_width(lines.len())
            } else {
                0
            };
            let mut stream = Stream::new(
                Arc::new(()),
                lines.len(),
                usize::from(key.width.saturating_sub(gutter).max(1)),
                key.line_display == crate::config::LineDisplay::Clip,
                left,
                key.tab,
                key.top,
                key.height,
                state,
                frame,
                frame_custody,
            )?;
            stream.set_minimap_content_width(key.width);
            stream.set_cursor(key.cursor, key.follow);
            self.slots.push(Slot {
                id,
                key: key.clone(),
                wanted: key.clone(),
                left,
                wanted_left: left,
                pending: Some(Pending::State(retirement.attach(stream))),
                retry_at: None,
                error: None,
            });
        }
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.id == id)
            .ok_or("source continuation ownership missing")?;
        slot.wanted = key;
        slot.wanted_left = left;
        let Some(pending) = slot.pending.take() else {
            return slot.error.clone().map_or(Ok(()), Err);
        };
        let finished = matches!(&pending, Pending::Finished(_));
        let cursor_changed =
            slot.wanted.follow && (!slot.key.follow || slot.wanted.cursor != slot.key.cursor);
        let mut stream = match pending {
            Pending::State(mut stream) | Pending::Finished(mut stream) => {
                if slot.wanted.top == slot.key.top
                    && slot.wanted.height == slot.key.height
                    && slot.wanted_left == slot.left
                    && !cursor_changed
                {
                    if finished {
                        slot.pending = Some(Pending::Finished(stream));
                        return Ok(());
                    }
                    stream
                } else {
                    if let Err(reason) = stream.ensure_frame_custody(&self.client) {
                        slot.pending = Some(Pending::Finished(stream));
                        slot.retry_at = Some(Instant::now() + Duration::from_millis(20));
                        return Err(format!("source seek retirement: {reason:?}"));
                    }
                    let frame = match self
                        .client
                        .quota_group()
                        .reserve_external_storage(FRAME_BYTES)
                    {
                        Ok(frame) => Arc::new(frame),
                        Err(reason) => {
                            slot.pending = Some(Pending::Finished(stream));
                            slot.retry_at = Some(Instant::now() + Duration::from_millis(20));
                            return Err(format!("source new window admission: {reason:?}"));
                        }
                    };
                    stream.set_cursor(slot.wanted.cursor, slot.wanted.follow);
                    slot.key = slot.wanted.clone();
                    slot.left = slot.wanted_left;
                    slot.pending = Some(Pending::Captured(ContinueJob::Seek {
                        stream,
                        top: slot.key.top,
                        height: slot.key.height,
                        left: slot.left,
                        cursor: cursor_changed.then_some(slot.key.cursor),
                        allocation: frame,
                    }));
                    slot.retry_at = Some(Instant::now());
                    return Ok(());
                }
            }
            other => {
                slot.pending = Some(other);
                return Ok(());
            }
        };
        stream.set_cursor(slot.wanted.cursor, slot.wanted.follow);
        if let Err(reason) = stream.ensure_frame_custody(&self.client) {
            slot.pending = Some(Pending::State(stream));
            slot.retry_at = Some(Instant::now() + Duration::from_millis(20));
            return Err(format!("source viewport retirement: {reason:?}"));
        }
        // Key/source fence was checked above BEFORE touching borrowed lines.
        let Some(credit) = self
            .budget
            .take(id, crate::editor_capture_budget::Kind::Window, 1)
        else {
            slot.pending = Some(Pending::State(stream));
            slot.retry_at = Some(Instant::now() + Duration::from_millis(20));
            return Ok(());
        };
        let chunk_retirement = match self.client.retirement().try_reserve::<Chunk>(CHUNK + 4096) {
            Ok(retirement) => retirement,
            Err(reason) => {
                self.budget.finish(credit, 0, 0);
                slot.pending = Some(Pending::State(stream));
                slot.retry_at = Some(Instant::now() + Duration::from_millis(20));
                return Err(format!("source captured-chunk retirement: {reason:?}"));
            }
        };
        let chunk = match stream.capture(lines, credit.bytes) {
            Ok(chunk) => {
                self.budget
                    .finish(credit, chunk.captured_bytes(), chunk.completed_line());
                chunk
            }
            Err(error) => {
                self.budget.finish(credit, 0, 0);
                slot.error = Some(error.clone());
                slot.pending = Some(Pending::State(stream));
                return Err(error);
            }
        };
        slot.pending = Some(Pending::Captured(ContinueJob::Capture {
            stream,
            chunk: chunk_retirement.attach(chunk),
        }));
        slot.retry_at = Some(Instant::now());
        Ok(())
    }
    pub fn collect(&mut self) -> Vec<WindowCompletion> {
        let mut output = Vec::with_capacity(4);
        let now = Instant::now();
        for slot in &mut self.slots {
            let Some(pending) = slot.pending.take() else {
                continue;
            };
            match pending {
                Pending::Captured(job) => {
                    if slot.retry_at.is_some_and(|at| at > now) {
                        slot.pending = Some(Pending::Captured(job));
                        continue;
                    }
                    match self.client.try_reserve(Lane::Cpu, COST) {
                        Ok(reservation) => match reservation.submit(job) {
                            Ok(receipt) => {
                                slot.retry_at = None;
                                slot.pending = Some(Pending::Running(receipt));
                            }
                            Err(rejected) => {
                                if terminal(rejected.reason) {
                                    slot.error = Some(format!(
                                        "source continuation submission: {:?}",
                                        rejected.reason
                                    ));
                                    slot.retry_at = None;
                                } else {
                                    slot.pending = Some(Pending::Captured(rejected.value));
                                    slot.retry_at = Some(now + Duration::from_millis(20));
                                }
                            }
                        },
                        Err(reason) => {
                            if terminal(reason) {
                                slot.error =
                                    Some(format!("source continuation admission: {reason:?}"));
                                slot.retry_at = None;
                            } else {
                                slot.pending = Some(Pending::Captured(job));
                                slot.retry_at = Some(now + Duration::from_millis(20));
                            }
                        }
                    }
                }
                Pending::Running(mut receipt) => match receipt.try_take() {
                    JobPoll::Pending => slot.pending = Some(Pending::Running(receipt)),
                    JobPoll::Ready(result) => {
                        let (outcome, retention) = result.into_parts();
                        match outcome {
                            JobOutcome::Finished(Ok(mut continuation)) => {
                                if let Some(viewport) = continuation.window.take() {
                                    output.push(WindowCompletion {
                                        id: slot.id,
                                        key: slot.key.clone(),
                                        viewport,
                                    });
                                }
                                slot.pending = continuation.stream.take().map(|stream| {
                                    if continuation.total_rows.is_some() {
                                        Pending::Finished(stream)
                                    } else {
                                        Pending::State(stream)
                                    }
                                });
                                // Independent state/frame storage already owns every
                                // derivative; completed finite receipt cannot starve slots.
                                drop(retention);
                                slot.retry_at = None;
                            }
                            JobOutcome::Finished(Err(error)) => slot.error = Some(error),
                            JobOutcome::NotStarted { mut job, .. } => {
                                match &mut job {
                                    ContinueJob::Capture { stream, chunk } => {
                                        stream.set_retention(retention.clone());
                                        chunk.set_retention(retention.clone());
                                    }
                                    ContinueJob::Seek { stream, .. } => {
                                        stream.set_retention(retention.clone());
                                    }
                                }
                                // No retry follows explicit shutdown cancellation;
                                // the original finite debit reaches the last CPU destructor.
                                drop(job);
                                slot.error =
                                    Some("source continuation cancelled before execution".into());
                            }
                            _ => {
                                slot.error = Some("source continuation cancelled or failed".into())
                            }
                        }
                    }
                    _ => slot.error = Some("source continuation receipt lost".into()),
                },
                other => slot.pending = Some(other),
            }
        }
        output
    }
    pub fn cancel(&mut self) {
        for slot in &self.slots {
            self.budget
                .cancel(slot.id, crate::editor_capture_budget::Kind::Window);
        }
        for slot in &self.slots {
            if let Some(Pending::Running(receipt)) = &slot.pending {
                receipt.cancel();
            }
        }
        self.slots.clear();
    }
}

impl Drop for SourceWindowPreparation {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edited_source_releases_installed_owner_but_keeps_emitted_original_leaf_alive() {
        let mut editor = crate::editor_pane::EditorPane::empty();
        editor.path = Some(std::path::PathBuf::from("original.rs"));
        editor.show_line_numbers = false;
        editor.textarea = ratatui_textarea::TextArea::new(vec!["a\tb界".into()]);
        let picker = ratatui_image::picker::Picker::halfblocks();
        editor.prepare_test_source_window(16, 2, &picker);
        let installed = editor.installed_window().unwrap().clone();
        let old_revision = installed.key.revision;
        let allocation = Arc::downgrade(&installed.viewport.allocation);
        let emitted = crate::source_window_surface::PaintedWindow {
            installed,
            content_area: ratatui::layout::Rect::new(10, 20, 16, 2),
            minimap_area: None,
        };
        assert_eq!(
            emitted.position(11, 20),
            Some((0, 1)),
            "tab's first cell maps original character"
        );
        assert_eq!(
            emitted.position(12, 20),
            Some((0, 1)),
            "tab's second cell maps same original character"
        );
        editor.insert_text("changed");
        assert_ne!(old_revision, editor.content_revision());
        assert!(editor.installed_window().is_none());
        assert!(
            allocation.upgrade().is_some(),
            "last emitted frame owns original rows and allocation"
        );
        drop(emitted);
        let deadline = Instant::now() + Duration::from_secs(3);
        while allocation.upgrade().is_some() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            allocation.upgrade().is_none(),
            "final frame retirement releases original allocation"
        );
    }
    #[test]
    fn real_bank_publishes_authored_33m_window_and_keeps_frame_until_last_ack_owner() {
        let client = crate::execution::test_document_client();
        let mut editor = crate::editor_pane::EditorPane::empty();
        editor.show_line_numbers = false;
        editor.line_display = crate::config::LineDisplay::Wrap;
        editor.textarea = ratatui_textarea::TextArea::new(vec![
            "a".repeat(crate::filesystem::editor::MAX_EDITOR_SOURCE_BYTES + 1)
        ]);
        editor.set_preparation_height(4);
        editor.source_retirement = Some(client.retirement());
        let picker = ratatui_image::picker::Picker::halfblocks();
        let key = editor.window_preparation_key(80, &picker).unwrap();
        let mut owner = SourceWindowPreparation::new(client.clone());
        owner
            .request(NodeId(1), key.clone(), 0, editor.textarea.lines())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let window = loop {
            owner.begin_capture_turn();
            if let Some(completion) = owner.collect().pop() {
                break completion.viewport;
            }
            assert!(
                Instant::now() < deadline,
                "real continuation did not publish first usable window"
            );
            std::thread::yield_now();
        };
        assert_eq!(window.rows.len(), 4);
        assert!(window.rows.iter().all(|row| row.glyphs.len() == 80));
        let painted = crate::source_window_surface::PaintedWindow {
            installed: client
                .retirement()
                .try_reserve::<crate::source_window_surface::InstalledWindow>(4096)
                .unwrap()
                .attach_shared(crate::source_window_surface::InstalledWindow {
                    key: key.clone(),
                    physical_count: 1,
                    viewport: window.clone(),
                }),
            content_area: ratatui::layout::Rect::new(10, 20, 80, 4),
            minimap_area: None,
        };
        assert_eq!(
            painted.position(13, 21),
            Some((0, 83)),
            "ACK row maps original columns across wraps"
        );
        assert_eq!(painted.position(9, 20), None);
        editor.install_window(
            &WindowCompletion {
                id: NodeId(1),
                key: key.clone(),
                viewport: window.clone(),
            },
            &picker,
            80,
        );
        editor.scroll_source_view(3, 4, 80);
        assert_eq!(
            editor.source_scroll_row(),
            3,
            "unknown total must not clamp authored continuation to zero"
        );
        drop(painted);
        assert_eq!(
            editor.textarea.lines()[0].len(),
            crate::filesystem::editor::MAX_EDITOR_SOURCE_BYTES + 1
        );
        assert!(
            window.total_rows.is_none(),
            "first viewport need not wait for full33MiB geometry"
        );
        let allocation = Arc::downgrade(&window.allocation);
        let emitted_owner = window.clone();
        owner.cancel();
        drop(owner);
        drop(window);
        drop(editor);
        assert!(
            allocation.upgrade().is_some(),
            "actual emitted window still owns its body guard"
        );
        drop(emitted_owner);
        let retirement_deadline = Instant::now() + Duration::from_secs(3);
        while allocation.upgrade().is_some() && Instant::now() < retirement_deadline {
            std::thread::yield_now();
        }
        assert!(allocation.upgrade().is_none());
    }
}
