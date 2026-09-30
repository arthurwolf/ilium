//! "Convert to" for agent panes: the dialog model, its renderer, and the
//! background worker that runs `ilium-session-convert`.
//!
//! The flow is owned by `App` (see `App::action_convert_session`): the pane's
//! agent process is stopped on the server, the pane's last screen is frozen
//! client-side, this dialog shows the step list / progress bar / log while a
//! blocking worker converts the transcript, and on success the server swaps
//! the frozen pane for a new pane resuming the converted session.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossterm::event::KeyCode;
use ilium_core::{AgentProvider, BuiltinAgentProvider, NodeId};
use ilium_ipc::ClientRequest;
use ilium_session_convert::{
    convert_session, ConversionEvent, ConversionOutcome, ConversionRequest,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Gauge, Paragraph};
use ratatui::Frame;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::app::{App, Mode};
use crate::modal;
use crate::theme;

/// Upper bound on retained log lines; a conversion emits tens of lines, this
/// only guards against a runaway producer.
const MAX_LOG_LINES: usize = 500;

/// Where the conversion currently is. `Failed` keeps the agent-stopped fact so
/// the dialog knows whether "resume the original session" is offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionPhase {
    StoppingAgent,
    Converting,
    Starting,
    Failed(String),
}

/// Everything the dialog displays plus what recovery needs.
#[derive(Debug, Clone)]
pub struct ConversionDialogState {
    pub pane_id: NodeId,
    pub source: BuiltinAgentProvider,
    pub target: BuiltinAgentProvider,
    pub source_session_id: String,
    pub project_cwd: PathBuf,
    pub phase: ConversionPhase,
    /// True once the server confirmed the source agent process is gone.
    pub is_agent_stopped: bool,
    pub progress: f32,
    /// Converter step titles, in order; index `i` is converter step `i + 1`.
    pub steps: Vec<String>,
    pub total_steps: usize,
    /// 1-based converter step currently running (0 before the first).
    pub current_step: usize,
    pub log: Vec<String>,
}

impl ConversionDialogState {
    pub fn new(
        pane_id: NodeId,
        source: BuiltinAgentProvider,
        target: BuiltinAgentProvider,
        source_session_id: String,
        project_cwd: PathBuf,
    ) -> Self {
        let mut state = Self {
            pane_id,
            source,
            target,
            source_session_id,
            project_cwd,
            phase: ConversionPhase::StoppingAgent,
            is_agent_stopped: false,
            progress: 0.0,
            steps: Vec::new(),
            total_steps: 0,
            current_step: 0,
            log: Vec::new(),
        };
        state.push_log(format!("Stopping the {} agent process…", source.label()));
        state
    }

    pub fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > MAX_LOG_LINES {
            let overflow = self.log.len() - MAX_LOG_LINES;
            self.log.drain(..overflow);
        }
    }

    pub fn apply_event(&mut self, event: ConversionEvent) {
        match event {
            ConversionEvent::Step {
                index,
                total,
                title,
            } => {
                self.total_steps = total;
                self.current_step = index;
                if self.steps.len() < index {
                    self.steps.resize(index, String::new());
                }
                if let Some(slot) = self.steps.get_mut(index.saturating_sub(1)) {
                    *slot = title.clone();
                }
                self.push_log(format!("[{index}/{total}] {title}"));
            }
            ConversionEvent::Log(line) => self.push_log(line),
            ConversionEvent::Progress(value) => {
                self.progress = value.clamp(self.progress, 1.0);
            }
        }
    }

    pub fn fail(&mut self, message: String) {
        self.push_log(format!("Failed: {message}"));
        self.phase = ConversionPhase::Failed(message);
    }

    pub fn is_failed(&self) -> bool {
        matches!(self.phase, ConversionPhase::Failed(_))
    }

    /// The exact command that resumes `session_id` for `provider`.
    pub fn resume_command(provider: BuiltinAgentProvider, session_id: &str) -> String {
        provider.resume_command(session_id)
    }
}

/// Where a replaced pane sat, for handing it focus to the new pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplacementFocus {
    pub old_pane_id: NodeId,
    pub parent: NodeId,
    pub index: usize,
    pub was_focused: bool,
}

/// Work the event loop hands to a blocking worker.
#[derive(Debug, Clone)]
pub struct ConversionJob {
    pub pane_id: NodeId,
    pub request: ConversionRequest,
}

/// Messages a running worker sends back to the event loop.
#[derive(Debug)]
pub enum ConversionWorkerEvent {
    Progress {
        pane_id: NodeId,
        event: ConversionEvent,
    },
    Finished {
        pane_id: NodeId,
        result: Result<ConversionOutcome, String>,
    },
}

struct ActiveWorker {
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// Owns the single in-flight conversion worker so it can be cancelled and is
/// never left running past the client.
pub struct ConversionWorkers {
    events: mpsc::Sender<ConversionWorkerEvent>,
    active: Option<ActiveWorker>,
}

impl ConversionWorkers {
    pub fn new(events: mpsc::Sender<ConversionWorkerEvent>) -> Self {
        Self {
            events,
            active: None,
        }
    }

    pub fn spawn(&mut self, job: ConversionJob) {
        self.reap_finished();
        if let Some(previous) = self.active.take() {
            previous.cancel.store(true, Ordering::SeqCst);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let events = self.events.clone();
        let handle = tokio::task::spawn_blocking(move || {
            let pane_id = job.pane_id;
            let progress_events = events.clone();
            let mut sink = |event: ConversionEvent| {
                // The receiver only disappears while the client shuts down.
                let _ = progress_events
                    .blocking_send(ConversionWorkerEvent::Progress { pane_id, event });
            };
            let result = convert_session(&job.request, &worker_cancel, &mut sink)
                .map_err(|error| error.to_string());
            let _ = events.blocking_send(ConversionWorkerEvent::Finished { pane_id, result });
        });
        self.active = Some(ActiveWorker { cancel, handle });
    }

    /// Asks the running conversion (if any) to stop at its next checkpoint.
    pub fn cancel(&self) {
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::SeqCst);
        }
    }

    fn reap_finished(&mut self) {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.handle.is_finished())
        {
            self.active = None;
        }
    }
}

impl Drop for ConversionWorkers {
    fn drop(&mut self) {
        self.cancel();
    }
}

const DIALOG_WIDTH: u16 = 78;
const DIALOG_HEIGHT: u16 = 24;

/// Centers the dialog inside the frozen pane's viewport (falling back to the
/// whole screen when the pane is not on screen).
pub fn dialog_area(pane_area: Option<Rect>, screen: Rect) -> Rect {
    let host = pane_area
        .filter(|area| area.width >= 30 && area.height >= 10)
        .unwrap_or(screen);
    modal::centered_fixed_rect(DIALOG_WIDTH, DIALOG_HEIGHT, host)
}

fn phase_line(state: &ConversionDialogState) -> Line<'static> {
    match &state.phase {
        ConversionPhase::StoppingAgent => Line::from(Span::styled(
            format!("Stopping {}…", state.source.label()),
            Style::new().fg(Color::Yellow),
        )),
        ConversionPhase::Converting => Line::from(Span::styled(
            "Converting the session transcript…",
            Style::new().fg(Color::Cyan),
        )),
        ConversionPhase::Starting => Line::from(Span::styled(
            format!("Starting {}…", state.target.label()),
            Style::new().fg(Color::Green),
        )),
        ConversionPhase::Failed(message) => Line::from(Span::styled(
            format!("Failed: {message}"),
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
    }
}

fn step_lines(state: &ConversionDialogState) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let stop_marker = if state.is_agent_stopped {
        Span::styled("✓ ", Style::new().fg(Color::Green))
    } else if state.phase == ConversionPhase::StoppingAgent {
        Span::styled("▶ ", Style::new().fg(Color::Yellow))
    } else {
        Span::raw("· ")
    };
    lines.push(Line::from(vec![
        stop_marker,
        Span::raw(format!("Stop the {} agent", state.source.label())),
    ]));
    for (index, title) in state.steps.iter().enumerate() {
        let step_number = index + 1;
        let marker = if state.phase == ConversionPhase::Starting
            || step_number < state.current_step
        {
            Span::styled("✓ ", Style::new().fg(Color::Green))
        } else if step_number == state.current_step && !state.is_failed() {
            Span::styled("▶ ", Style::new().fg(Color::Cyan))
        } else if step_number == state.current_step {
            Span::styled("✗ ", Style::new().fg(Color::Red))
        } else {
            Span::raw("· ")
        };
        lines.push(Line::from(vec![marker, Span::raw(title.clone())]));
    }
    let start_marker = if state.phase == ConversionPhase::Starting {
        Span::styled("▶ ", Style::new().fg(Color::Green))
    } else {
        Span::raw("· ")
    };
    lines.push(Line::from(vec![
        start_marker,
        Span::raw(format!("Start {} on the new session", state.target.label())),
    ]));
    lines
}

fn hint_text(state: &ConversionDialogState) -> &'static str {
    match &state.phase {
        ConversionPhase::Failed(_) if state.is_agent_stopped => {
            "Enter resume the original session   Esc close"
        }
        ConversionPhase::Failed(_) => "Esc close",
        ConversionPhase::Converting => "Esc cancel",
        ConversionPhase::StoppingAgent | ConversionPhase::Starting => "",
    }
}

/// Draws the dialog over the frozen pane.
pub fn render(frame: &mut Frame, area: Rect, state: &ConversionDialogState) {
    frame.render_widget(Clear, area);
    let title = format!(
        "Convert {} session to {}",
        state.source.label(),
        state.target.label()
    );
    let border_color = if state.is_failed() {
        Color::Red
    } else {
        Color::Cyan
    };
    let block = theme::block(true)
        .title(theme::chrome_title(&title))
        .border_style(Style::new().fg(border_color).add_modifier(Modifier::BOLD));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let step_rows = (state.steps.len() + 2).clamp(3, 10) as u16;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(step_rows),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(2),
            Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(Paragraph::new(phase_line(state)), rows[0]);
    frame.render_widget(Paragraph::new(step_lines(state)), rows[1]);

    let ratio = f64::from(state.progress.clamp(0.0, 1.0));
    let gauge_color = if state.is_failed() {
        Color::Red
    } else {
        Color::Cyan
    };
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::new().fg(gauge_color))
            .ratio(ratio)
            .label(format!("{:.0}%", ratio * 100.0)),
        rows[2],
    );

    let log_area = rows[4];
    let visible = usize::from(log_area.height);
    let start = state.log.len().saturating_sub(visible);
    let log_lines: Vec<Line> = state.log[start..]
        .iter()
        .map(|line| Line::from(Span::styled(line.clone(), Style::new().fg(Color::Gray))))
        .collect();
    frame.render_widget(Paragraph::new(log_lines), log_area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint_text(state),
            Style::new().add_modifier(Modifier::DIM),
        ))),
        rows[5],
    );
}

impl App {
    /// The provider a pane's session can be converted to: only a Claude or
    /// Codex agent with a resolved session outside a worktree, and only while
    /// no other conversion is running.
    pub(crate) fn conversion_target_for(&self, pane_id: NodeId) -> Option<BuiltinAgentProvider> {
        if self.conversion.is_some() || self.tree.pane_workspace(pane_id).is_some() {
            return None;
        }
        let (class, _session_id, _cwd) = self.last_prompt_transcript_context(pane_id)?;
        match class.provider()? {
            BuiltinAgentProvider::Claude => Some(BuiltinAgentProvider::Codex),
            BuiltinAgentProvider::Codex => Some(BuiltinAgentProvider::Claude),
            BuiltinAgentProvider::Antigravity => None,
        }
    }

    /// Starts a conversion: verifies the source transcript while the agent is
    /// still alive, then freezes the pane, opens the dialog, and asks the
    /// server to stop the agent process.
    pub fn action_convert_session(&mut self, pane_id: NodeId, target: BuiltinAgentProvider) {
        let Some((class, session_id, project_cwd)) = self.last_prompt_transcript_context(pane_id)
        else {
            self.status_message = Some("No agent session to convert".to_string());
            return;
        };
        let Some(source) = class.provider() else {
            self.status_message = Some("This agent cannot be converted".to_string());
            return;
        };
        let Some(home_dir) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
        else {
            self.status_message = Some("Cannot convert: home directory unavailable".to_string());
            return;
        };
        if crate::agent_history_path::verified_jsonl_history_path(
            &home_dir,
            &project_cwd,
            &class,
            &session_id,
        )
        .is_none()
        {
            self.status_message = Some(
                "Cannot convert: this session has no verified transcript yet".to_string(),
            );
            return;
        }
        self.frozen_panes.insert(pane_id);
        self.conversion = Some(Box::new(ConversionDialogState::new(
            pane_id,
            source,
            target,
            session_id,
            project_cwd,
        )));
        self.mode = Mode::ConvertSession;
        self.queue_request(ClientRequest::TerminatePaneProcess { pane_id });
    }

    /// Server answer to `TerminatePaneProcess`: on success the blocking
    /// conversion worker is requested, otherwise the dialog reports failure.
    pub fn apply_pane_process_terminated(&mut self, pane_id: NodeId, result: Result<(), String>) {
        let Some(state) = self.conversion.as_mut() else {
            return;
        };
        if state.pane_id != pane_id || state.phase != ConversionPhase::StoppingAgent {
            return;
        }
        match result {
            Ok(()) => {
                state.is_agent_stopped = true;
                state.phase = ConversionPhase::Converting;
                state.push_log(format!(
                    "{} process stopped; screen frozen",
                    state.source.label()
                ));
                let Some(home_dir) =
                    directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
                else {
                    state.fail("home directory unavailable".to_string());
                    return;
                };
                self.pending_conversion_start = Some(ConversionJob {
                    pane_id,
                    request: ConversionRequest {
                        home_dir,
                        project_cwd: state.project_cwd.clone(),
                        source: state.source,
                        target: state.target,
                        source_session_id: state.source_session_id.clone(),
                        codex_home: None,
                        codex_executable: None,
                    },
                });
            }
            Err(message) => state.fail(message),
        }
    }

    /// Applies one worker message; a successful conversion asks the server to
    /// swap the frozen pane for the converted session and closes the dialog.
    pub fn apply_conversion_worker_event(&mut self, event: ConversionWorkerEvent) {
        match event {
            ConversionWorkerEvent::Progress { pane_id, event } => {
                if let Some(state) = self.conversion.as_mut().filter(|s| s.pane_id == pane_id) {
                    state.apply_event(event);
                }
            }
            ConversionWorkerEvent::Finished { pane_id, result } => {
                let Some(state) = self.conversion.as_mut().filter(|s| s.pane_id == pane_id) else {
                    return;
                };
                match result {
                    Ok(outcome) => {
                        let command = ConversionDialogState::resume_command(
                            state.target,
                            &outcome.new_session_id,
                        );
                        let label = state.target.label();
                        state.phase = ConversionPhase::Starting;
                        state.progress = 1.0;
                        state.push_log(format!("Starting: {command}"));
                        self.remember_replacement_focus(pane_id);
                        self.queue_request(ClientRequest::ReplacePaneWithCommand {
                            pane_id,
                            command_line: command,
                        });
                        self.conversion = None;
                        if matches!(self.mode, Mode::ConvertSession) {
                            self.mode = Mode::Normal;
                        }
                        self.status_message = Some(format!("Session converted; {label} started"));
                    }
                    Err(message) => state.fail(message),
                }
            }
        }
    }

    fn remember_replacement_focus(&mut self, pane_id: NodeId) {
        let Some(parent) = self.tree.parent_of(pane_id) else {
            return;
        };
        let Some(index) = self
            .tree
            .children_of(parent)
            .ok()
            .and_then(|children| children.iter().position(|child| *child == pane_id))
        else {
            return;
        };
        self.pending_replacement_focus = Some(ReplacementFocus {
            old_pane_id: pane_id,
            parent,
            index,
            was_focused: self.active_pane_id() == Some(pane_id),
        });
    }

    /// Called after every tree snapshot: once the replaced pane is gone, the
    /// pane now at its position takes over focus when the old one had it.
    pub fn apply_pending_replacement_focus(&mut self) {
        let Some(pending) = self.pending_replacement_focus else {
            return;
        };
        if self.tree.get(pending.old_pane_id).is_some() {
            return;
        }
        self.pending_replacement_focus = None;
        if !pending.was_focused {
            return;
        }
        let replacement = self
            .tree
            .children_of(pending.parent)
            .ok()
            .and_then(|children| children.get(pending.index).copied())
            .filter(|id| self.tree.get(*id).is_some_and(|node| node.is_pane()));
        if let Some(replacement) = replacement {
            self.focus_pane(replacement);
        }
    }

    pub fn take_pending_conversion_start(&mut self) -> Option<ConversionJob> {
        self.pending_conversion_start.take()
    }

    pub fn take_pending_conversion_cancel(&mut self) -> bool {
        std::mem::take(&mut self.pending_conversion_cancel)
    }

    /// Keyboard contract of `Mode::ConvertSession`.
    pub fn handle_conversion_key(&mut self, code: KeyCode) {
        let Some(state) = self.conversion.as_mut() else {
            self.mode = Mode::Normal;
            return;
        };
        match (&state.phase, code) {
            (ConversionPhase::Failed(_), KeyCode::Enter) if state.is_agent_stopped => {
                let command =
                    ConversionDialogState::resume_command(state.source, &state.source_session_id);
                let pane_id = state.pane_id;
                let label = state.source.label();
                self.remember_replacement_focus(pane_id);
                self.queue_request(ClientRequest::ReplacePaneWithCommand {
                    pane_id,
                    command_line: command,
                });
                self.conversion = None;
                self.mode = Mode::Normal;
                self.status_message = Some(format!("Resumed the original {label} session"));
            }
            (ConversionPhase::Failed(_), KeyCode::Esc) => {
                let pane_id = state.pane_id;
                self.frozen_panes.remove(&pane_id);
                self.conversion = None;
                self.mode = Mode::Normal;
            }
            (ConversionPhase::Converting, KeyCode::Esc) => {
                state.push_log("Cancelling…".to_string());
                self.pending_conversion_cancel = true;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ConversionDialogState {
        ConversionDialogState::new(
            NodeId(7),
            BuiltinAgentProvider::Claude,
            BuiltinAgentProvider::Codex,
            "11111111-1111-4111-8111-111111111111".to_string(),
            PathBuf::from("/tmp/project"),
        )
    }

    #[test]
    fn step_events_populate_the_step_list_and_log() {
        let mut dialog = state();
        dialog.apply_event(ConversionEvent::Step {
            index: 2,
            total: 4,
            title: "Parse".to_string(),
        });
        dialog.apply_event(ConversionEvent::Log("read 12 lines".to_string()));
        assert_eq!(dialog.total_steps, 4);
        assert_eq!(dialog.current_step, 2);
        assert_eq!(dialog.steps, vec![String::new(), "Parse".to_string()]);
        assert_eq!(dialog.log.last().map(String::as_str), Some("read 12 lines"));
    }

    #[test]
    fn progress_never_moves_backwards_or_past_one() {
        let mut dialog = state();
        dialog.apply_event(ConversionEvent::Progress(0.6));
        dialog.apply_event(ConversionEvent::Progress(0.2));
        assert!((dialog.progress - 0.6).abs() < f32::EPSILON);
        dialog.apply_event(ConversionEvent::Progress(3.0));
        assert!((dialog.progress - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn failure_is_terminal_and_logged() {
        let mut dialog = state();
        dialog.fail("codex is not installed".to_string());
        assert!(dialog.is_failed());
        assert_eq!(
            dialog.log.last().map(String::as_str),
            Some("Failed: codex is not installed")
        );
    }

    #[test]
    fn log_is_bounded() {
        let mut dialog = state();
        for index in 0..(MAX_LOG_LINES + 50) {
            dialog.push_log(format!("line {index}"));
        }
        assert_eq!(dialog.log.len(), MAX_LOG_LINES);
        assert_eq!(
            dialog.log.last().map(String::as_str),
            Some(format!("line {}", MAX_LOG_LINES + 49).as_str())
        );
    }

    #[test]
    fn resume_command_uses_the_provider_grammar() {
        assert_eq!(
            ConversionDialogState::resume_command(BuiltinAgentProvider::Codex, "abc"),
            "codex resume abc"
        );
        assert_eq!(
            ConversionDialogState::resume_command(BuiltinAgentProvider::Claude, "abc"),
            "claude --resume abc"
        );
    }

    mod app_flow {
        use super::*;
        use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus};
        use ilium_session_convert::ConversionOutcome;

        const SESSION_ID: &str = "11111111-1111-4111-8111-111111111111";

        fn app_with_agent(class: AgentClass) -> (App, NodeId) {
            let mut app = App::new("test".to_string(), std::env::temp_dir());
            let group = app.tree.add_group(ilium_core::ROOT_ID, "work").unwrap();
            let pane = app
                .tree
                .add_pane(group, "agent", PaneContentKind::Terminal)
                .unwrap();
            app.tree
                .set_pane_status(
                    pane,
                    PaneStatus::from_activity(class, AgentActivity::Idle, None),
                )
                .unwrap();
            app.agent_session_ids.insert(pane, SESSION_ID.to_string());
            (app, pane)
        }

        fn begin(app: &mut App, pane: NodeId, source: BuiltinAgentProvider, target: BuiltinAgentProvider) {
            app.frozen_panes.insert(pane);
            app.conversion = Some(Box::new(ConversionDialogState::new(
                pane,
                source,
                target,
                SESSION_ID.to_string(),
                std::env::temp_dir(),
            )));
            app.mode = Mode::ConvertSession;
        }

        #[test]
        fn only_the_opposite_builtin_provider_is_offered() {
            let (app, pane) = app_with_agent(AgentClass::Claude);
            assert_eq!(app.conversion_target_for(pane), Some(BuiltinAgentProvider::Codex));
            let (app, pane) = app_with_agent(AgentClass::Codex);
            assert_eq!(app.conversion_target_for(pane), Some(BuiltinAgentProvider::Claude));
            let (app, pane) = app_with_agent(AgentClass::Antigravity);
            assert_eq!(app.conversion_target_for(pane), None);
        }

        #[test]
        fn a_pane_without_a_resolved_session_is_not_convertible() {
            let (mut app, pane) = app_with_agent(AgentClass::Claude);
            app.agent_session_ids.remove(&pane);
            assert_eq!(app.conversion_target_for(pane), None);
        }

        #[test]
        fn no_second_conversion_is_offered_while_one_is_active() {
            let (mut app, pane) = app_with_agent(AgentClass::Claude);
            begin(&mut app, pane, BuiltinAgentProvider::Claude, BuiltinAgentProvider::Codex);
            assert_eq!(app.conversion_target_for(pane), None);
        }

        #[test]
        fn a_stopped_agent_starts_the_worker_and_a_failed_stop_does_not() {
            let (mut app, pane) = app_with_agent(AgentClass::Claude);
            begin(&mut app, pane, BuiltinAgentProvider::Claude, BuiltinAgentProvider::Codex);
            app.apply_pane_process_terminated(pane, Ok(()));
            let job = app.take_pending_conversion_start().expect("worker requested");
            assert_eq!(job.pane_id, pane);
            assert_eq!(job.request.source, BuiltinAgentProvider::Claude);
            assert_eq!(job.request.target, BuiltinAgentProvider::Codex);
            assert_eq!(job.request.source_session_id, SESSION_ID);
            let dialog = app.conversion.as_ref().unwrap();
            assert!(dialog.is_agent_stopped);
            assert_eq!(dialog.phase, ConversionPhase::Converting);

            let (mut app, pane) = app_with_agent(AgentClass::Claude);
            begin(&mut app, pane, BuiltinAgentProvider::Claude, BuiltinAgentProvider::Codex);
            app.apply_pane_process_terminated(pane, Err("still running".to_string()));
            assert!(app.take_pending_conversion_start().is_none());
            let dialog = app.conversion.as_ref().unwrap();
            assert!(dialog.is_failed());
            assert!(!dialog.is_agent_stopped);
        }

        #[test]
        fn success_replaces_the_pane_with_the_converted_session_and_closes_the_dialog() {
            let (mut app, pane) = app_with_agent(AgentClass::Claude);
            begin(&mut app, pane, BuiltinAgentProvider::Claude, BuiltinAgentProvider::Codex);
            app.apply_pane_process_terminated(pane, Ok(()));
            let _ = app.take_outbound_requests();
            app.apply_conversion_worker_event(ConversionWorkerEvent::Finished {
                pane_id: pane,
                result: Ok(ConversionOutcome {
                    new_session_id: "22222222-2222-4222-8222-222222222222".to_string(),
                    target_transcript_path: PathBuf::from("/tmp/x.jsonl"),
                    converted_items: 2,
                    dropped_items: 0,
                }),
            });
            let requests = app.take_outbound_requests();
            assert_eq!(
                requests,
                vec![ClientRequest::ReplacePaneWithCommand {
                    pane_id: pane,
                    command_line: "codex resume 22222222-2222-4222-8222-222222222222".to_string(),
                }]
            );
            assert!(app.conversion.is_none());
            assert!(matches!(app.mode, Mode::Normal));
        }

        #[test]
        fn failure_after_the_stop_offers_resuming_the_original_session() {
            let (mut app, pane) = app_with_agent(AgentClass::Codex);
            begin(&mut app, pane, BuiltinAgentProvider::Codex, BuiltinAgentProvider::Claude);
            app.apply_pane_process_terminated(pane, Ok(()));
            app.apply_conversion_worker_event(ConversionWorkerEvent::Finished {
                pane_id: pane,
                result: Err("transcript unreadable".to_string()),
            });
            assert!(app.conversion.as_ref().unwrap().is_failed());
            let _ = app.take_outbound_requests();
            app.handle_conversion_key(KeyCode::Enter);
            assert_eq!(
                app.take_outbound_requests(),
                vec![ClientRequest::ReplacePaneWithCommand {
                    pane_id: pane,
                    command_line: format!("codex resume {SESSION_ID}"),
                }]
            );
            assert!(app.conversion.is_none());
        }

        #[test]
        fn escape_closes_a_failed_dialog_and_unfreezes_the_pane() {
            let (mut app, pane) = app_with_agent(AgentClass::Claude);
            begin(&mut app, pane, BuiltinAgentProvider::Claude, BuiltinAgentProvider::Codex);
            app.apply_pane_process_terminated(pane, Err("nope".to_string()));
            app.handle_conversion_key(KeyCode::Enter);
            assert!(app.conversion.is_some(), "Enter must not resume a live agent");
            app.handle_conversion_key(KeyCode::Esc);
            assert!(app.conversion.is_none());
            assert!(!app.frozen_panes.contains(&pane));
            assert!(matches!(app.mode, Mode::Normal));
        }

        #[test]
        fn escape_while_converting_requests_cancellation() {
            let (mut app, pane) = app_with_agent(AgentClass::Claude);
            begin(&mut app, pane, BuiltinAgentProvider::Claude, BuiltinAgentProvider::Codex);
            app.apply_pane_process_terminated(pane, Ok(()));
            app.handle_conversion_key(KeyCode::Esc);
            assert!(app.take_pending_conversion_cancel());
            assert!(app.conversion.is_some());
        }

        #[test]
        fn the_replacement_takes_focus_when_the_old_pane_had_it() {
            let (mut app, pane) = app_with_agent(AgentClass::Claude);
            let parent = app.tree.parent_of(pane).unwrap();
            app.pending_replacement_focus = Some(ReplacementFocus {
                old_pane_id: pane,
                parent,
                index: 0,
                was_focused: true,
            });
            app.apply_pending_replacement_focus();
            assert!(
                app.pending_replacement_focus.is_some(),
                "the old pane still exists, so the handoff must wait"
            );
            let replacement = app
                .tree
                .add_pane(parent, "new", PaneContentKind::Terminal)
                .unwrap();
            app.tree.remove_node(pane).unwrap();
            app.apply_pending_replacement_focus();
            assert!(app.pending_replacement_focus.is_none());
            assert_eq!(app.active_pane_id(), Some(replacement));
        }
    }
}
