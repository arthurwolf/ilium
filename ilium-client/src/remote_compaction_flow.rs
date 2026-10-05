//! The remote-compaction flow owned by `App`: start (manual button or the
//! automatic context monitor), wait for a safe pause, stop the agent, run the
//! worker, resume the agent on the compacted session, and recover on failure.
//!
//! Sequence for one run (see `docs/plan/remote-compaction.md` section 4):
//!
//! 1. [`App::action_remote_compact`] opens the dialog and hands the worker an
//!    `AwaitPause` job. The pane stays live while it waits.
//! 2. Once the transcript is between turns the pane is frozen and the server
//!    is asked to terminate the agent process.
//! 3. The server's answer starts the `Compact` job.
//! 4. A successful run replaces the pane with `<agent> resume <same id>`; a
//!    failed one leaves the transcript untouched and offers to resume it.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use ilium_core::{AgentProvider, BuiltinAgentProvider, NodeId};
use ilium_ipc::ClientRequest;
use ilium_remote_compaction::{AgentKind, CompactionRequest};

use crate::app::{App, Mode};
use crate::remote_compaction_dialog::{
    RemoteCompactionDialogState, RemoteCompactionPhase, RemoteCompactionPlan,
};
use crate::remote_compaction_worker::{RemoteCompactionJob, RemoteCompactionWorkerEvent};

/// Consecutive failures after which a pane is left alone until its context
/// drops back below the threshold.
const MAX_CONSECUTIVE_FAILURES: u8 = 3;

/// Hysteresis: failures and cooldown reset once the context fill is this many
/// percentage points under the threshold (the agent compacted itself, or the
/// user cleared the conversation).
const RESET_MARGIN_PERCENT: f64 = 5.0;

/// Per-pane state of the automatic trigger.
#[derive(Debug, Clone, Copy, Default)]
struct PaneMonitorState {
    last_attempt: Option<Instant>,
    consecutive_failures: u8,
}

/// Decides when a pane's context fill calls for an automatic compaction.
#[derive(Debug, Default)]
pub struct RemoteCompactionMonitor {
    panes: HashMap<NodeId, PaneMonitorState>,
}

impl RemoteCompactionMonitor {
    /// True when a pane at `fill_percent` should be compacted now.
    pub fn should_trigger(
        &mut self,
        pane_id: NodeId,
        fill_percent: f64,
        threshold_percent: f64,
        cooldown: Duration,
        now: Instant,
    ) -> bool {
        let state = self.panes.entry(pane_id).or_default();
        if fill_percent < threshold_percent - RESET_MARGIN_PERCENT {
            *state = PaneMonitorState::default();
            return false;
        }
        if fill_percent < threshold_percent {
            return false;
        }
        if state.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            return false;
        }
        state
            .last_attempt
            .is_none_or(|attempt| now.saturating_duration_since(attempt) >= cooldown)
    }

    pub fn record_attempt(&mut self, pane_id: NodeId, now: Instant) {
        self.panes.entry(pane_id).or_default().last_attempt = Some(now);
    }

    pub fn record_failure(&mut self, pane_id: NodeId) {
        let state = self.panes.entry(pane_id).or_default();
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
    }

    pub fn record_success(&mut self, pane_id: NodeId) {
        self.panes.entry(pane_id).or_default().consecutive_failures = 0;
    }

    pub fn forget(&mut self, pane_id: NodeId) {
        self.panes.remove(&pane_id);
    }
}

fn agent_kind_for(provider: BuiltinAgentProvider) -> Option<AgentKind> {
    match provider {
        BuiltinAgentProvider::Claude => Some(AgentKind::Claude),
        BuiltinAgentProvider::Codex => Some(AgentKind::Codex),
        BuiltinAgentProvider::Antigravity => None,
    }
}

impl App {
    /// Provider and model that receive the transcript, as shown in the dialog.
    pub(crate) fn remote_compaction_destination(&self) -> String {
        let model = self.inference_settings.selected_model().trim();
        format!(
            "{} / {}",
            self.inference_settings.selected_provider.label(),
            if model.is_empty() {
                "(no model selected)"
            } else {
                model
            }
        )
    }

    /// The run `pane_id` would get right now, or `None` when remote
    /// compaction is off, another run is active, or the pane has no agent
    /// with a verified transcript. Reads the file system (verification).
    pub(crate) fn remote_compaction_plan_for(
        &self,
        pane_id: NodeId,
        is_automatic: bool,
    ) -> Option<RemoteCompactionPlan> {
        if !self.remote_compaction_settings.enabled || self.remote_compaction.is_some() {
            return None;
        }
        let (class, session_id, project_cwd) = self.last_prompt_transcript_context(pane_id)?;
        let provider = class.provider()?;
        let agent = agent_kind_for(provider)?;
        let home_dir = directories::BaseDirs::new()?.home_dir().to_path_buf();
        let transcript_path = crate::agent_history_path::verified_jsonl_history_path(
            &home_dir,
            &project_cwd,
            &class,
            &session_id,
        )?;
        Some(RemoteCompactionPlan {
            pane_id,
            provider,
            agent,
            session_id,
            project_cwd,
            transcript_path,
            technique: self.remote_compaction_settings.options_for(agent).technique,
            destination: self.remote_compaction_destination(),
            is_automatic,
        })
    }

    /// Whether the toolbar Compact button should run remote compaction for
    /// this pane instead of sending `/compact`.
    pub fn remote_compaction_replaces_compact(&self, pane_id: NodeId) -> bool {
        self.remote_compaction_plan_for(pane_id, false).is_some()
    }

    /// Opens the dialog and starts waiting for a safe pause. Returns whether a
    /// run started.
    pub fn action_remote_compact(&mut self, pane_id: NodeId, is_automatic: bool) -> bool {
        let Some(plan) = self.remote_compaction_plan_for(pane_id, is_automatic) else {
            return false;
        };
        let interrupt_after =
            Duration::from_secs(self.remote_compaction_settings.pause_timeout_seconds);
        self.pending_remote_compaction_job = Some(RemoteCompactionJob::AwaitPause {
            pane_id,
            agent: plan.agent,
            transcript_path: plan.transcript_path.clone(),
            interrupt_after,
        });
        self.remote_compaction_monitor
            .record_attempt(pane_id, Instant::now());
        self.remote_compaction = Some(Box::new(RemoteCompactionDialogState::new(plan)));
        self.mode = Mode::RemoteCompaction;
        true
    }

    pub fn take_pending_remote_compaction_job(&mut self) -> Option<RemoteCompactionJob> {
        self.pending_remote_compaction_job.take()
    }

    pub fn take_pending_remote_compaction_cancel(&mut self) -> bool {
        std::mem::take(&mut self.pending_remote_compaction_cancel)
    }

    /// Server answer to `TerminatePaneProcess`: on success the compaction job
    /// is requested, otherwise the dialog reports the failure.
    pub fn apply_remote_pane_process_terminated(
        &mut self,
        pane_id: NodeId,
        result: Result<(), String>,
    ) {
        let context_window = self
            .session_stats
            .entry(pane_id)
            .and_then(|entry| entry.stats.as_ref())
            .and_then(|stats| stats.context_window);
        let Some(state) = self.remote_compaction.as_mut() else {
            return;
        };
        if state.pane_id != pane_id || state.phase != RemoteCompactionPhase::StoppingAgent {
            return;
        }
        match result {
            Ok(()) => {
                state.is_agent_stopped = true;
                state.phase = RemoteCompactionPhase::Compacting;
                state.push_log(format!(
                    "{} process stopped; screen frozen",
                    state.provider.label()
                ));
                let options = self.remote_compaction_settings.options_for(state.agent);
                self.pending_remote_compaction_job = Some(RemoteCompactionJob::Compact {
                    pane_id,
                    request: CompactionRequest {
                        agent: state.agent,
                        transcript_path: state.transcript_path.clone(),
                        session_id: state.session_id.clone(),
                        project_cwd: state.project_cwd.clone(),
                        options,
                        context_window_tokens: context_window,
                    },
                    inference_settings: Box::new(self.inference_settings.clone()),
                });
            }
            Err(message) => {
                state.fail(message);
                self.remote_compaction_monitor.record_failure(pane_id);
            }
        }
    }

    /// Applies one worker message.
    pub fn apply_remote_compaction_worker_event(&mut self, event: RemoteCompactionWorkerEvent) {
        match event {
            RemoteCompactionWorkerEvent::InterruptRequested { pane_id } => {
                let Some(state) = self
                    .remote_compaction
                    .as_mut()
                    .filter(|s| s.pane_id == pane_id)
                else {
                    return;
                };
                state.push_log(
                    "The agent is still busy after the pause timeout; sending Escape".to_string(),
                );
                if self
                    .send_terminal_bytes(pane_id, b"\x1b".to_vec(), None)
                    .is_err()
                {
                    if let Some(state) = self.remote_compaction.as_mut() {
                        state.push_log("Escape was rejected before terminal admission".to_string());
                    }
                }
            }
            RemoteCompactionWorkerEvent::PauseReached { pane_id, result } => {
                self.apply_remote_pause_reached(pane_id, result);
            }
            RemoteCompactionWorkerEvent::Progress { pane_id, event } => {
                if let Some(state) = self
                    .remote_compaction
                    .as_mut()
                    .filter(|s| s.pane_id == pane_id)
                {
                    state.apply_event(event);
                }
            }
            RemoteCompactionWorkerEvent::Finished { pane_id, result } => {
                self.apply_remote_compaction_finished(pane_id, result);
            }
        }
    }

    fn apply_remote_pause_reached(&mut self, pane_id: NodeId, result: Result<(), String>) {
        let Some(state) = self
            .remote_compaction
            .as_mut()
            .filter(|s| s.pane_id == pane_id)
        else {
            return;
        };
        if state.phase != RemoteCompactionPhase::WaitingForPause {
            return;
        }
        match result {
            Ok(()) => {
                state.phase = RemoteCompactionPhase::StoppingAgent;
                state.push_log("Safe pause reached; stopping the agent process…".to_string());
                self.frozen_panes.insert(pane_id);
                self.queue_request(ClientRequest::TerminatePaneProcess { pane_id });
            }
            Err(message) => {
                state.fail(message);
                self.remote_compaction_monitor.record_failure(pane_id);
            }
        }
    }

    fn apply_remote_compaction_finished(
        &mut self,
        pane_id: NodeId,
        result: Result<ilium_remote_compaction::CompactionOutcome, String>,
    ) {
        let Some(state) = self
            .remote_compaction
            .as_mut()
            .filter(|s| s.pane_id == pane_id)
        else {
            return;
        };
        match result {
            Ok(outcome) => {
                state.tokens = outcome.tokens.clone();
                state.phase = RemoteCompactionPhase::Restarting;
                state.progress = 1.0;
                let command = state.resume_command();
                let label = state.provider.label();
                state.push_log(format!("Starting: {command}"));
                let used_fallback = outcome.used_fallback;
                self.remember_replacement_focus(pane_id);
                self.queue_request(ClientRequest::ReplacePaneWithCommand {
                    pane_id,
                    command_line: command,
                });
                self.remote_compaction = None;
                if matches!(self.mode, Mode::RemoteCompaction) {
                    self.mode = Mode::Normal;
                }
                self.remote_compaction_monitor.record_success(pane_id);
                self.status_message = Some(if used_fallback {
                    format!(
                        "Conversation compacted with the offline fallback summary; {label} resumed"
                    )
                } else {
                    format!("Conversation compacted remotely; {label} resumed")
                });
            }
            Err(message) => {
                state.fail(message);
                self.remote_compaction_monitor.record_failure(pane_id);
            }
        }
    }

    /// Keyboard contract of `Mode::RemoteCompaction`.
    pub fn handle_remote_compaction_key(&mut self, code: KeyCode) {
        if matches!(code, KeyCode::Char('x') | KeyCode::Delete) {
            self.dismiss_remote_compaction_privacy_banner();
            return;
        }
        let Some(state) = self.remote_compaction.as_mut() else {
            self.mode = Mode::Normal;
            return;
        };
        let pane_id = state.pane_id;
        match (&state.phase, code) {
            (RemoteCompactionPhase::Failed(_), KeyCode::Enter) if state.is_agent_stopped => {
                let command = state.resume_command();
                let label = state.provider.label();
                self.remember_replacement_focus(pane_id);
                self.queue_request(ClientRequest::ReplacePaneWithCommand {
                    pane_id,
                    command_line: command,
                });
                self.close_remote_compaction_dialog();
                self.status_message = Some(format!("Resumed the original {label} session"));
            }
            (RemoteCompactionPhase::Failed(_), KeyCode::Esc) => {
                self.frozen_panes.remove(&pane_id);
                self.close_remote_compaction_dialog();
            }
            (RemoteCompactionPhase::WaitingForPause, KeyCode::Esc) => {
                // Nothing was stopped yet, so cancelling just ends the wait.
                self.pending_remote_compaction_cancel = true;
                self.close_remote_compaction_dialog();
                self.status_message = Some("Remote compaction cancelled".to_string());
            }
            (RemoteCompactionPhase::Compacting, KeyCode::Esc) => {
                state.push_log("Cancelling…".to_string());
                self.pending_remote_compaction_cancel = true;
            }
            _ => {}
        }
    }

    fn close_remote_compaction_dialog(&mut self) {
        self.remote_compaction = None;
        self.mode = Mode::Normal;
    }

    /// Automatic trigger: starts a run for the active agent pane once its
    /// context fill reaches the configured threshold. Only the active pane is
    /// considered because the dialog is modal and must never grab the
    /// keyboard from work on another pane. Returns whether a run started.
    pub fn tick_remote_compaction_monitor(&mut self, now: Instant) -> bool {
        let settings = &self.remote_compaction_settings;
        if !settings.enabled
            || !settings.automatic
            || self.remote_compaction.is_some()
            || !matches!(self.mode, Mode::Normal)
        {
            return false;
        }
        let Some(pane_id) = self.active_pane_id() else {
            return false;
        };
        let Some(fill) = self
            .session_stats
            .entry(pane_id)
            .and_then(|entry| entry.stats.as_ref())
            .and_then(|stats| stats.context_fill())
        else {
            return false;
        };
        let threshold = f64::from(settings.threshold_percent);
        let cooldown = Duration::from_secs(settings.cooldown_minutes * 60);
        if !self.remote_compaction_monitor.should_trigger(
            pane_id,
            fill * 100.0,
            threshold,
            cooldown,
            now,
        ) {
            return false;
        }
        self.action_remote_compact(pane_id, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANE: NodeId = NodeId(9);

    #[test]
    fn monitor_triggers_at_the_threshold_only() {
        let mut monitor = RemoteCompactionMonitor::default();
        let now = Instant::now();
        let cooldown = Duration::from_secs(600);
        assert!(!monitor.should_trigger(PANE, 64.9, 65.0, cooldown, now));
        assert!(monitor.should_trigger(PANE, 65.0, 65.0, cooldown, now));
    }

    #[test]
    fn monitor_waits_out_the_cooldown_after_an_attempt() {
        let mut monitor = RemoteCompactionMonitor::default();
        let start = Instant::now();
        let cooldown = Duration::from_secs(600);
        monitor.record_attempt(PANE, start);
        assert!(!monitor.should_trigger(
            PANE,
            80.0,
            65.0,
            cooldown,
            start + Duration::from_secs(599)
        ));
        assert!(monitor.should_trigger(
            PANE,
            80.0,
            65.0,
            cooldown,
            start + Duration::from_secs(600)
        ));
    }

    #[test]
    fn monitor_stops_after_repeated_failures_until_the_context_drops() {
        let mut monitor = RemoteCompactionMonitor::default();
        let now = Instant::now();
        let cooldown = Duration::ZERO;
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            monitor.record_failure(PANE);
        }
        assert!(!monitor.should_trigger(PANE, 90.0, 65.0, cooldown, now));
        // Within the hysteresis margin nothing resets.
        assert!(!monitor.should_trigger(PANE, 61.0, 65.0, cooldown, now));
        assert!(!monitor.should_trigger(PANE, 90.0, 65.0, cooldown, now));
        // A real drop (the agent compacted itself) re-arms the pane.
        assert!(!monitor.should_trigger(PANE, 10.0, 65.0, cooldown, now));
        assert!(monitor.should_trigger(PANE, 90.0, 65.0, cooldown, now));
    }

    #[test]
    fn a_success_clears_the_failure_streak() {
        let mut monitor = RemoteCompactionMonitor::default();
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            monitor.record_failure(PANE);
        }
        monitor.record_success(PANE);
        assert!(monitor.should_trigger(PANE, 90.0, 65.0, Duration::ZERO, Instant::now()));
    }

    #[test]
    fn only_claude_and_codex_map_to_a_transcript_format() {
        assert_eq!(
            agent_kind_for(BuiltinAgentProvider::Claude),
            Some(AgentKind::Claude)
        );
        assert_eq!(
            agent_kind_for(BuiltinAgentProvider::Codex),
            Some(AgentKind::Codex)
        );
        assert_eq!(agent_kind_for(BuiltinAgentProvider::Antigravity), None);
    }

    mod app_flow {
        use super::*;
        use crate::app::PaneRuntime;
        use crate::terminal_view::TerminalView;
        use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus};
        use ilium_remote_compaction::{CompactionOutcome, Technique, TokenBreakdown};

        const SESSION_ID: &str = "11111111-1111-4111-8111-111111111111";

        fn app_with_dialog() -> (App, NodeId) {
            let mut app = App::new("test".to_string(), std::env::temp_dir());
            let group = app.tree.add_group(ilium_core::ROOT_ID, "work").unwrap();
            let pane = app
                .tree
                .add_pane(group, "agent", PaneContentKind::Terminal)
                .unwrap();
            app.tree
                .set_pane_status(
                    pane,
                    PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Idle, None),
                )
                .unwrap();
            app.panes.insert(
                pane,
                PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
            );
            app.remote_compaction = Some(Box::new(RemoteCompactionDialogState::new(
                RemoteCompactionPlan {
                    pane_id: pane,
                    provider: BuiltinAgentProvider::Claude,
                    agent: AgentKind::Claude,
                    session_id: SESSION_ID.to_string(),
                    project_cwd: std::env::temp_dir(),
                    transcript_path: std::env::temp_dir().join("t.jsonl"),
                    technique: Technique::ClaudeCode,
                    destination: "Kilo Gateway / m".to_string(),
                    is_automatic: false,
                },
            )));
            app.mode = Mode::RemoteCompaction;
            app.take_outbound_requests();
            (app, pane)
        }

        fn outcome() -> CompactionOutcome {
            CompactionOutcome {
                session_id: SESSION_ID.to_string(),
                transcript_path: std::env::temp_dir().join("t.jsonl"),
                backup_path: std::env::temp_dir().join("t.jsonl.bak"),
                tokens: TokenBreakdown::default(),
                summary_chars: 10,
                chunks: 1,
                used_fallback: false,
                redactions: 0,
            }
        }

        #[test]
        fn disabled_feature_never_starts_and_never_replaces_compact() {
            let (mut app, pane) = app_with_dialog();
            app.remote_compaction = None;
            app.mode = Mode::Normal;
            assert!(!app.remote_compaction_settings.enabled);
            assert!(!app.action_remote_compact(pane, false));
            assert!(!app.remote_compaction_replaces_compact(pane));
            assert!(app.remote_compaction.is_none());
        }

        #[test]
        fn reaching_the_pause_freezes_the_pane_and_stops_the_agent() {
            let (mut app, pane) = app_with_dialog();
            app.apply_remote_compaction_worker_event(RemoteCompactionWorkerEvent::PauseReached {
                pane_id: pane,
                result: Ok(()),
            });
            assert!(app.frozen_panes.contains(&pane));
            assert_eq!(
                app.take_outbound_requests(),
                vec![ClientRequest::TerminatePaneProcess { pane_id: pane }]
            );
            assert_eq!(
                app.remote_compaction.as_ref().unwrap().phase,
                RemoteCompactionPhase::StoppingAgent
            );
        }

        #[test]
        fn a_missed_pause_fails_without_freezing() {
            let (mut app, pane) = app_with_dialog();
            app.apply_remote_compaction_worker_event(RemoteCompactionWorkerEvent::PauseReached {
                pane_id: pane,
                result: Err("late".to_string()),
            });
            assert!(!app.frozen_panes.contains(&pane));
            assert!(app.remote_compaction.as_ref().unwrap().is_failed());
            assert!(app.take_outbound_requests().is_empty());
            app.handle_remote_compaction_key(KeyCode::Esc);
            assert!(app.remote_compaction.is_none());
            assert!(matches!(app.mode, Mode::Normal));
        }

        #[test]
        fn a_stopped_agent_requests_the_compaction_job() {
            let (mut app, pane) = app_with_dialog();
            app.apply_remote_compaction_worker_event(RemoteCompactionWorkerEvent::PauseReached {
                pane_id: pane,
                result: Ok(()),
            });
            app.apply_remote_pane_process_terminated(pane, Ok(()));
            let Some(RemoteCompactionJob::Compact { request, .. }) =
                app.take_pending_remote_compaction_job()
            else {
                panic!("compaction job requested");
            };
            assert_eq!(request.session_id, SESSION_ID);
            assert_eq!(request.agent, AgentKind::Claude);
            assert!(app.remote_compaction.as_ref().unwrap().is_agent_stopped);
        }

        #[test]
        fn a_failed_stop_does_not_start_the_worker() {
            let (mut app, pane) = app_with_dialog();
            app.apply_remote_compaction_worker_event(RemoteCompactionWorkerEvent::PauseReached {
                pane_id: pane,
                result: Ok(()),
            });
            app.apply_remote_pane_process_terminated(pane, Err("busy".to_string()));
            assert!(app.take_pending_remote_compaction_job().is_none());
            assert!(app.remote_compaction.as_ref().unwrap().is_failed());
        }

        #[test]
        fn success_resumes_the_same_session_and_closes_the_dialog() {
            let (mut app, pane) = app_with_dialog();
            app.apply_remote_compaction_worker_event(RemoteCompactionWorkerEvent::Finished {
                pane_id: pane,
                result: Ok(outcome()),
            });
            assert_eq!(
                app.take_outbound_requests(),
                vec![ClientRequest::ReplacePaneWithCommand {
                    pane_id: pane,
                    command_line: format!("claude --resume {SESSION_ID}"),
                }]
            );
            assert!(app.remote_compaction.is_none());
            assert!(matches!(app.mode, Mode::Normal));
        }

        #[test]
        fn failure_after_the_stop_offers_to_resume_the_original_session() {
            let (mut app, pane) = app_with_dialog();
            app.remote_compaction.as_mut().unwrap().is_agent_stopped = true;
            app.apply_remote_compaction_worker_event(RemoteCompactionWorkerEvent::Finished {
                pane_id: pane,
                result: Err("model unavailable".to_string()),
            });
            assert!(app.remote_compaction.as_ref().unwrap().is_failed());
            assert!(app.take_outbound_requests().is_empty());
            app.handle_remote_compaction_key(KeyCode::Enter);
            assert_eq!(
                app.take_outbound_requests(),
                vec![ClientRequest::ReplacePaneWithCommand {
                    pane_id: pane,
                    command_line: format!("claude --resume {SESSION_ID}"),
                }]
            );
            assert!(app.remote_compaction.is_none());
        }

        #[test]
        fn escape_while_waiting_cancels_without_touching_the_agent() {
            let (mut app, _pane) = app_with_dialog();
            app.handle_remote_compaction_key(KeyCode::Esc);
            assert!(app.take_pending_remote_compaction_cancel());
            assert!(app.remote_compaction.is_none());
            assert!(app.take_outbound_requests().is_empty());
        }

        #[test]
        fn escape_while_compacting_asks_the_worker_to_cancel_and_keeps_the_dialog() {
            let (mut app, pane) = app_with_dialog();
            app.remote_compaction.as_mut().unwrap().phase = RemoteCompactionPhase::Compacting;
            app.handle_remote_compaction_key(KeyCode::Esc);
            assert!(app.take_pending_remote_compaction_cancel());
            assert!(app
                .remote_compaction
                .as_ref()
                .is_some_and(|s| s.pane_id == pane));
        }

        #[test]
        fn x_dismisses_the_privacy_banner_for_good() {
            let (mut app, _pane) = app_with_dialog();
            assert!(app.remote_compaction_settings.should_show_privacy_banner());
            app.handle_remote_compaction_key(KeyCode::Char('x'));
            assert!(!app.remote_compaction_settings.should_show_privacy_banner());
        }

        #[test]
        fn interrupt_request_sends_escape_to_the_pane() {
            let (mut app, pane) = app_with_dialog();
            app.apply_remote_compaction_worker_event(
                RemoteCompactionWorkerEvent::InterruptRequested { pane_id: pane },
            );
            assert_eq!(
                app.take_outbound_requests(),
                vec![ClientRequest::KeyInput {
                    pane_id: pane,
                    bytes: b"\x1b".to_vec(),
                    submission: None,
                }]
            );
        }
    }
}
