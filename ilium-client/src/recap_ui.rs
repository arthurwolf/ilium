//! App-side behaviour of pane recaps: the manual refresh worker, the recap
//! popover on the pane header dot (short on hover, long when pinned) and the
//! phase-based progress shown while a recap runs.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ilium_core::NodeId;
use ilium_ipc::{ClientRequest, RecapTrigger};
use ratatui::layout::Position;

use crate::app::{App, Mode, RightPanelTarget};
use crate::popover::HOVER_DELAY;
use crate::recap_state::StoredRecap;
use crate::recap_worker::{
    phase_label, phase_percent, RecapWorkerRequest, RecapWorkerResult, PHASE_DONE,
};
use crate::status_icons::TooltipContent;
use crate::theme;

/// Width of the text progress bar, in cells.
const BAR_WIDTH: usize = 20;

/// The recap popover of one pane: a hover preview or a pinned window.
#[derive(Debug)]
pub struct RecapPopover {
    pub pane_id: NodeId,
    pub pinned: bool,
    entered_at: Instant,
}

impl RecapPopover {
    fn is_visible(&self, now: Instant) -> bool {
        self.pinned || now.duration_since(self.entered_at) >= HOVER_DELAY
    }
}

/// Progress of the manual recap running for one pane.
#[derive(Debug)]
pub struct RecapProgress {
    phase: Arc<AtomicU8>,
    started_at: Instant,
}

impl App {
    fn recap_ui_active(&self) -> bool {
        self.modal_stack.is_empty()
            && matches!(self.mode, Mode::Normal)
            && !matches!(
                self.right_panel_target,
                RightPanelTarget::Chatroom { .. } | RightPanelTarget::Resource { .. }
            )
    }

    /// The pane whose recap dot sits under `position`.
    pub fn recap_icon_pane_at(&self, position: Position) -> Option<NodeId> {
        let viewport = self.pane_viewport_at(position)?;
        let is_icon = theme::chrome_recap_cell(viewport.outer_area) == position;
        (is_icon && self.is_known_agent_pane(viewport.pane_id)).then_some(viewport.pane_id)
    }

    pub fn take_pending_recap_requests(&mut self) -> Vec<RecapWorkerRequest> {
        std::mem::take(&mut self.pending_recap_requests)
    }

    /// Starts a manual recap for an agent pane. Does nothing for other panes,
    /// for a pane without a resolved session, or while one is in flight.
    pub fn action_request_recap(&mut self, pane_id: NodeId) {
        let Some(session_id) = self.agent_session_ids.get(&pane_id).cloned() else {
            return;
        };
        let Some(agent) = self.tree.get(pane_id).and_then(|node| match &node.kind {
            ilium_core::NodeKind::Pane { status, .. } => status.agent_state().cloned(),
            _ => None,
        }) else {
            return;
        };
        let Some(recap_generation) = self.recap.begin_manual(pane_id) else {
            return;
        };
        let phase = Arc::new(AtomicU8::new(0));
        self.recap_progress.insert(
            pane_id,
            RecapProgress {
                phase: Arc::clone(&phase),
                started_at: Instant::now(),
            },
        );
        self.recap_errors.remove(&pane_id);
        let project_dir = self
            .tree
            .pane_cwd(pane_id)
            .unwrap_or(&self.session_cwd)
            .to_path_buf();
        self.pending_recap_requests.push(RecapWorkerRequest {
            pane_id,
            session_id,
            recap_generation,
            class: agent.class.clone(),
            project_dir,
            home: std::path::PathBuf::new(),
            activity: format!("{:?}", agent.activity()).to_lowercase(),
            progress: phase,
        });
    }

    /// Records a recap that could not run (admission refused, no home).
    pub fn recap_failed(&mut self, pane_id: NodeId, message: String) {
        self.recap.finish_manual(pane_id);
        self.recap_progress.remove(&pane_id);
        self.recap_errors.insert(pane_id, message);
    }

    /// Applies a finished manual recap: a stale session is dropped, a good
    /// result goes to the server, which fences it again.
    pub fn finish_recap_worker(&mut self, outcome: RecapWorkerResult) {
        let RecapWorkerResult {
            pane_id,
            session_id,
            recap_generation,
            result,
            ..
        } = outcome;
        self.recap.finish_manual(pane_id);
        if let Some(progress) = self.recap_progress.remove(&pane_id) {
            progress.phase.store(PHASE_DONE, Ordering::Relaxed);
        }
        if self.agent_session_ids.get(&pane_id) != Some(&session_id) {
            return;
        }
        match result {
            Ok(texts) => {
                self.recap_errors.remove(&pane_id);
                self.queue_request(ClientRequest::ApplyRecap {
                    pane_id,
                    session_id,
                    recap_generation,
                    trigger: RecapTrigger::Manual,
                    short: texts.short,
                    long: Some(texts.long),
                });
            }
            Err(message) => {
                self.recap_errors.insert(pane_id, message);
            }
        }
    }

    /// Whether any pane recap is running, so the UI keeps redrawing.
    pub fn recap_is_running(&self) -> bool {
        !self.recap_progress.is_empty()
    }

    fn ensure_recap(&mut self, pane_id: NodeId) {
        let Some(session_id) = self.agent_session_ids.get(&pane_id) else {
            return;
        };
        if self.recap.recap_for(pane_id, session_id).is_none() {
            self.action_request_recap(pane_id);
        }
    }

    /// Claims pointer events on the recap dot. Returns `true` when consumed.
    pub fn handle_recap_popover_mouse(&mut self, mouse: MouseEvent, position: Position) -> bool {
        if !self.recap_ui_active() {
            self.recap_popover = None;
            return false;
        }
        let Some(pane_id) = self.recap_icon_pane_at(position) else {
            if self
                .recap_popover
                .as_ref()
                .is_some_and(|popover| !popover.pinned)
            {
                self.recap_popover = None;
            }
            return false;
        };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let was_pinned = self
                    .recap_popover
                    .as_ref()
                    .is_some_and(|popover| popover.pinned && popover.pane_id == pane_id);
                self.recap_popover = (!was_pinned).then(|| RecapPopover {
                    pane_id,
                    pinned: true,
                    entered_at: Instant::now(),
                });
                if !was_pinned {
                    self.ensure_recap(pane_id);
                }
                true
            }
            MouseEventKind::Moved => {
                let keep_pinned = self
                    .recap_popover
                    .as_ref()
                    .is_some_and(|popover| popover.pinned);
                if !keep_pinned {
                    self.recap_popover = Some(RecapPopover {
                        pane_id,
                        pinned: false,
                        entered_at: Instant::now(),
                    });
                    self.ensure_recap(pane_id);
                }
                false
            }
            _ => false,
        }
    }

    /// The text bar and label for a running recap, `None` when idle.
    fn recap_progress_line(&self, pane_id: NodeId) -> Option<String> {
        let progress = self.recap_progress.get(&pane_id)?;
        let phase = progress.phase.load(Ordering::Relaxed);
        let percent = usize::from(phase_percent(phase, progress.started_at.elapsed()));
        let filled = percent * BAR_WIDTH / 100;
        Some(format!(
            "[{}{}] {percent:>3}% {}",
            "#".repeat(filled),
            "-".repeat(BAR_WIDTH - filled),
            phase_label(phase),
        ))
    }

    /// Cached text for a pane, hover shows the short recap and pinned the long.
    fn recap_body(&self, pane_id: NodeId, is_pinned: bool) -> String {
        let stored: Option<&StoredRecap> = self
            .agent_session_ids
            .get(&pane_id)
            .and_then(|session_id| self.recap.recap_for(pane_id, session_id));
        match stored {
            Some(recap) if is_pinned => recap.long.clone().unwrap_or_else(|| recap.short.clone()),
            Some(recap) => recap.short.clone(),
            None => match self.recap_errors.get(&pane_id) {
                Some(message) => format!("Recap failed: {message}"),
                None if self.recap_progress.contains_key(&pane_id) => "Writing recap...".into(),
                None => "No recap yet.".into(),
            },
        }
    }

    /// Tooltip for the open recap popover, `None` when hidden.
    pub fn recap_popover_tooltip(&self) -> Option<(Position, TooltipContent)> {
        let popover = self.recap_popover.as_ref()?;
        if !popover.is_visible(Instant::now()) {
            return None;
        }
        let viewport = self.pane_viewport(popover.pane_id)?;
        let anchor = theme::chrome_recap_cell(viewport.outer_area);
        Some((
            anchor,
            TooltipContent {
                title: "Recap".to_string(),
                body: self.recap_body(popover.pane_id, popover.pinned),
                reason: self.recap_progress_line(popover.pane_id),
            },
        ))
    }

    /// Tooltip for the tree refresh icon: cached short recap plus progress.
    pub fn recap_refresh_tooltip(&self) -> Option<(Position, TooltipContent)> {
        let (pane_id, anchor) = self.hovered_recap_refresh?;
        Some((
            anchor,
            TooltipContent {
                title: "Refresh title and recap".to_string(),
                body: self.recap_body(pane_id, false),
                reason: self.recap_progress_line(pane_id),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_without_session_starts_nothing() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.action_request_recap(NodeId(5));
        assert!(app.take_pending_recap_requests().is_empty());
        assert!(!app.recap_is_running());
    }

    #[test]
    fn failed_recap_clears_progress_and_keeps_message() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let pane = NodeId(5);
        assert!(app.recap.begin_manual(pane).is_some());
        app.recap_failed(pane, "no provider".into());
        assert!(!app.recap.is_in_flight(pane));
        assert_eq!(app.recap_body(pane, false), "Recap failed: no provider");
    }

    #[test]
    fn stale_session_result_is_not_sent() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let pane = NodeId(5);
        app.agent_session_ids.insert(pane, "new".into());
        app.finish_recap_worker(RecapWorkerResult {
            pane_id: pane,
            session_id: "old".into(),
            recap_generation: 1,
            result: Ok(crate::recap_worker::RecapTexts {
                short: "s".into(),
                long: "l".into(),
            }),
            automatic_ai_decision: crate::naming_workers::AutomaticAiDecision::new(0, true),
        });
        assert!(app.take_outbound_requests().is_empty());
    }
}
