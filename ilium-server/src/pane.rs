//! `PaneResource`: what `ilium-server` keeps in its pane registry for one
//! tree node -- either a live pty-backed terminal, or (for an editor pane)
//! just the path it points at, since editor content/editing stays
//! client-local (see ARCHITECTURE.md "Crate roles": the server only needs to know
//! "this NodeId is an editor pointing at this path" for tree persistence
//! and for other attached clients to know what's open).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ilium_core::{
    AgentClass, AgentProvider, BuiltinAgentProvider, GoalState, NodeId, PaneProgress,
    SessionIdentityTransitionRule,
};
use ilium_ipc::ProgressMonitorStatus;
use ilium_pty::{PtyCommand, PtyError, PtySession};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::progress_monitor::{
    ProgressMonitorFence, ProgressMonitorGeneration, ProgressMonitorRegistration,
};
use crate::shell_title::ShellCommandTracker;

/// Default pty size a newly-created terminal pane starts at, before the
/// client that requested it reports its real viewport via `ResizePane`.
/// `ilium_ipc::ClientRequest::NewPane` carries no size (only
/// `ResizePane` does), so the server picks a reasonable starting point
/// rather than blocking pane creation on a size the client hasn't sent
/// yet.
pub const DEFAULT_PANE_ROWS: u16 = 24;
pub const DEFAULT_PANE_COLS: u16 = 80;
/// Deliberately neutral title shown after an agent discards its conversation.
/// The next verified session may replace it through normal title inference.
pub const FRESH_AGENT_TITLE: &str = "<new>";

/// What Ilium durably knows about one automated progress-owned PTY delivery.
/// `DeliveredToPty` deliberately does not claim the agent consumed the text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProgressDeliveryState {
    #[default]
    NotQueued,
    Queued,
    Attempted,
    DeliveredToPty,
    Uncertain,
}

impl From<ProgressDeliveryState> for crate::persistence::PersistedProgressDeliveryState {
    fn from(value: ProgressDeliveryState) -> Self {
        match value {
            ProgressDeliveryState::NotQueued => Self::NotQueued,
            ProgressDeliveryState::Queued => Self::Queued,
            ProgressDeliveryState::Attempted => Self::Attempted,
            ProgressDeliveryState::DeliveredToPty => Self::DeliveredToPty,
            ProgressDeliveryState::Uncertain => Self::Uncertain,
        }
    }
}

impl From<crate::persistence::PersistedProgressDeliveryState> for ProgressDeliveryState {
    fn from(value: crate::persistence::PersistedProgressDeliveryState) -> Self {
        match value {
            crate::persistence::PersistedProgressDeliveryState::NotQueued => Self::NotQueued,
            crate::persistence::PersistedProgressDeliveryState::Queued => Self::Queued,
            crate::persistence::PersistedProgressDeliveryState::Attempted => Self::Attempted,
            crate::persistence::PersistedProgressDeliveryState::DeliveredToPty => {
                Self::DeliveredToPty
            }
            crate::persistence::PersistedProgressDeliveryState::Uncertain => Self::Uncertain,
        }
    }
}

/// Server-owned operational state for one accepted progress registration.
/// The pure tree carries only `latest_progress`; command and delivery state
/// stay beside the PTY because they control server-side work.
#[derive(Debug, Clone)]
pub struct ProgressMonitorRuntimeState {
    pub monitor_id: u64,
    pub command: String,
    pub interval: Duration,
    pub latest_progress: PaneProgress,
    pub result_delivery: ProgressDeliveryState,
}

/// What a terminal pane was spawned to run -- kept separate from
/// `ilium_ipc::NewPaneKind` (which also has an `Editor` variant that can
/// never apply to a `TerminalOrigin`) so this type has no invalid state to
/// accidentally construct. Also what the crash-recovery snapshot persists
/// per terminal pane (see `crate::persistence`), since it's exactly the
/// information needed to respawn the same kind of pane later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalOrigin {
    /// The user's default shell (`$SHELL`, falling back to `/bin/sh`).
    PlainShell,
    /// A specific command line, run via `$SHELL -c <command_line>` so
    /// ordinary shell syntax (arguments, quoting, pipes) works without
    /// this crate needing its own shell-word-splitter.
    Command(String),
}

impl TerminalOrigin {
    /// The name a freshly-created pane is given in the tree when the
    /// client didn't otherwise specify one (`ilium_ipc::ClientRequest::NewPane`
    /// carries no title -- naming/renaming is a separate, later concern
    /// handled by `RenameNode` and, client-side, title inference).
    pub fn default_pane_name(&self) -> &str {
        match self {
            TerminalOrigin::PlainShell => "shell",
            TerminalOrigin::Command(command_line) => command_line,
        }
    }

    /// Name shown after a known session is invalidated. Standard persisted
    /// resume commands drop their now-stale ID; arbitrary user commands keep
    /// their exact launch text. Routed through the shared
    /// `BuiltinAgentProvider` registry (rather than one hardcoded prefix
    /// literal per provider) so a new registry entry does not also need a
    /// matching branch here -- see CLAUDE.md's registry-not-if/else rule.
    pub fn pane_name_without_stale_session(&self) -> &str {
        match self {
            TerminalOrigin::Command(command) => BuiltinAgentProvider::resume_binding(command)
                .map(|(provider, _session_id)| provider.command_line())
                .unwrap_or_else(|| self.default_pane_name()),
            TerminalOrigin::PlainShell => self.default_pane_name(),
        }
    }
}

/// One live pty-backed terminal pane: the pty session itself, what it was
/// spawned to run, this pane's adaptive detection schedule, and the
/// background task that forwards its raw output bytes to attached clients.
pub struct TerminalPaneRuntime {
    pub session: PtySession,
    /// Serializes semantic input for this pane across the gap between an
    /// automated text write and its later Enter. Other panes remain usable.
    pub input_gate: std::sync::Arc<Mutex<()>>,
    pub origin: TerminalOrigin,
    pub shell_command_tracker: Option<ShellCommandTracker>,
    /// Observes submitted agent slash commands only so an in-process
    /// `/resume`-style transition can invalidate launch-time identity before
    /// discovery sees the replacement transcript. It is never used to infer
    /// an ID from content.
    pub session_command_tracker: ShellCommandTracker,
    /// While true, launch arguments describe the pre-transition session and
    /// are forbidden as identity evidence. Only a PID-held transcript can
    /// resolve the new session and clear this flag.
    pub is_session_identity_invalidated: bool,
    /// Session ID cleared by an in-process transition. The same agent PID may
    /// briefly retain its old transcript descriptor while `/resume` switches
    /// sessions, so this ID stays inadmissible until a different verified ID
    /// is found or a replacement process takes ownership.
    pub invalidated_session_id: Option<String>,
    /// Connects the input event that invalidated a session to the later
    /// verified replacement identity, even when discovery needs several
    /// asynchronous ticks to observe the provider's new transcript.
    pub pending_session_transition_correlation_id: Option<String>,
    /// UUID supplied by ilium to an exact fresh `claude` launch. It is not
    /// published or persisted as an active session until detection confirms
    /// that this pane actually owns a live Claude process.
    pub pending_generated_session_id: Option<String>,
    /// Changes whenever the pane's visible agent conversation is reset.
    /// Title workers carry this through IPC so an older result cannot rename
    /// the newly-fresh pane after `/clear`.
    pub title_generation: u64,
    /// Edge detector for provider-specific fresh screens. A fresh screen can
    /// persist for many detection ticks, but it must reset titles once only.
    pub is_showing_fresh_agent_screen: bool,
    /// Durable goal ownership confirmed for this exact detected agent process.
    /// An inconclusive screen sample retains it; explicit completion,
    /// `/goal clear`, or a different PID/class removes it so one process can never
    /// leak its flag into a replacement CLI in the same terminal pane.
    pub confirmed_goal_owner: Option<ConfirmedGoalOwner>,
    /// Identifies one continuous goal owner independently of its phase.
    /// Active -> paused -> active transitions retain the epoch; clearing the
    /// goal or replacing its process/provider advances it.
    pub goal_owner_epoch: u64,
    pub detection_schedule: DetectionSchedule,
    /// This pane's agent session/thread ID, once `crate::session_id`
    /// discovers one. Rechecked while an agent is detected because `/resume`
    /// can replace the active session inside an existing terminal pane.
    pub session_id: Option<String>,
    /// Agent class that owned `session_id`; prevents a later different CLI in
    /// the same terminal from inheriting stale identity.
    pub session_agent_class: Option<AgentClass>,
    /// Most recently detected live agent process, independent of whether
    /// transcript discovery has accepted a session identity for it. Agent
    /// input and lifecycle diagnostics need this PID even during unresolved
    /// or invalidated session windows.
    pub detected_agent_process_id: Option<u32>,
    /// Most recently verified agent class from the live process tree. This is
    /// intentionally independent of `session_agent_class`: a newly-launched
    /// CLI can expose its prompt before transcript discovery has accepted an
    /// identity, letting initial prompt delivery prefer a process-confirmed
    /// provider when it has already arrived.
    pub detected_agent_class: Option<AgentClass>,
    /// Exact detected process that owned `session_id`. A replacement process
    /// may safely use its own startup arguments even when the previous agent
    /// invalidated launch-time identity with an in-process session command.
    pub session_process_id: Option<u32>,
    /// OS pid of the agent process this pane already sent an auto-answer key
    /// to for a known interstitial dialog (see
    /// `ilium_detect::interstitial_prompt_response`). Keyed to the pid, not
    /// to a screen/detection generation counter: the dialog can repaint
    /// (e.g. nothing external, but any redraw bumps `screen_generation`)
    /// while still on screen, and re-keying on generation would resend the
    /// answer every tick -- for a numbered-choice prompt that types straight
    /// into the next composer, repeated digits can get typed and even
    /// submitted. At most one auto-answer per agent process, ever; cleared
    /// only when a different pid is detected.
    pub auto_answered_interstitial_prompt_for_pid: Option<u32>,
    /// Forwards `session.subscribe_output_bytes()` chunks to the session's
    /// broadcast channel as `ServerEvent::ScreenUpdate` frames. Owned here
    /// so closing this pane has a single, unambiguous place to cancel it
    /// (see `CLAUDE.md`'s async-task-ownership rule) -- `abort_background_tasks`
    /// is the only way this handle is ever touched after creation.
    forward_task: Option<JoinHandle<()>>,
    /// One cancellable task that waits for a newly-launched agent's visible
    /// composer before submitting its one-shot initial request. It is owned by
    /// the pane so closing the pane or manually typing into it cannot leave a
    /// delayed prompt writing into a reused terminal.
    initial_prompt_task: Option<JoinHandle<()>>,
    /// This pane's active progress-monitor loop (see
    /// `crate::progress_monitor`), if `SetPaneProgressMonitor` started one.
    /// Owned here so replacing it (a fresh `SetPaneProgressMonitor` call) or
    /// closing the pane has a single, unambiguous place to cancel the
    /// previous run -- same rationale as `initial_prompt_task`.
    progress_monitor_task: Option<JoinHandle<()>>,
    /// Pause/result/resume waiter for the active monitor. Separate from the
    /// probe task so terminal probing can stop while a safe composer is still
    /// pending. Replacement and clear cancel both.
    progress_delivery_task: Option<JoinHandle<()>>,
    /// Atomic generation fence shared with the probe and delivery tasks.
    pub progress_monitor_generation: ProgressMonitorGeneration,
    /// Serializes registration replacement/clear with the final readiness
    /// check and full text-to-Enter submission of progress-owned effects.
    pub progress_effect_gate: std::sync::Arc<Mutex<()>>,
    pub progress_monitor: Option<ProgressMonitorRuntimeState>,
    /// Goal-owner epoch in which the user typed `/goal pause` at the
    /// keyboard. Agent-requested resume refuses that pause; typing
    /// `/goal resume` or a new goal owner (new epoch) releases it.
    pub user_paused_goal_epoch: Option<u64>,
    /// Pending agent-requested `/goal resume` (see `crate::goal_control`).
    pub agent_goal_resume: Option<AgentGoalResumeRequest>,
    /// Waiter that submits `agent_goal_resume` at the next safe composer.
    /// Owned here so closing the pane or cancelling the request stops it.
    agent_goal_resume_task: Option<JoinHandle<()>>,
    /// Idle paused-goal reminder bookkeeping (see
    /// `crate::goal_control::spawn_idle_reminder`).
    pub goal_reminder: Option<GoalReminderState>,
    /// Goal-owner epoch in which detection last saw the goal `Active`. A
    /// pause whose epoch was never seen active has an unknown origin (it
    /// predates this server, for example) and fails closed.
    pub goal_active_seen_epoch: Option<u64>,
    /// When Ilium last wrote an agent-requested `/goal resume`. Detection
    /// needs a moment to observe `Active`; during that window the pause is
    /// reported as resume-queued so nothing queues a duplicate resume.
    pub goal_resume_submitted_at: Option<Instant>,
}

/// One continuous episode in which this pane's goal stayed resumable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalReminderState {
    pub goal_owner_epoch: u64,
    pub resumable_since: Instant,
    pub is_delivered: bool,
}

/// One agent-requested resume, fenced to the paused goal it was issued for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentGoalResumeRequest {
    pub goal_owner_epoch: u64,
    pub process_id: u32,
}

impl TerminalPaneRuntime {
    pub fn new(
        session: PtySession,
        origin: TerminalOrigin,
        pending_generated_session_id: Option<String>,
        initial_poll_interval: Duration,
    ) -> Self {
        Self {
            session,
            input_gate: std::sync::Arc::new(Mutex::new(())),
            shell_command_tracker: matches!(&origin, TerminalOrigin::PlainShell)
                .then(ShellCommandTracker::default),
            session_command_tracker: ShellCommandTracker::default(),
            is_session_identity_invalidated: false,
            invalidated_session_id: None,
            pending_session_transition_correlation_id: None,
            pending_generated_session_id,
            title_generation: 0,
            is_showing_fresh_agent_screen: false,
            confirmed_goal_owner: None,
            goal_owner_epoch: 0,
            origin,
            detection_schedule: DetectionSchedule {
                // Checked on the very next detection tick rather than
                // waiting a full interval -- a freshly-spawned pane's
                // status (e.g. "is this actually an agent CLI") is not
                // yet known and should resolve promptly.
                next_due: Instant::now(),
                current_interval: initial_poll_interval,
                client_focused: false,
                last_forced: None,
                request_generation: 0,
                identity_system_generation: None,
                cached_identity: None,
                cached_screen_classification: None,
            },
            session_id: None,
            session_agent_class: None,
            detected_agent_process_id: None,
            detected_agent_class: None,
            session_process_id: None,
            auto_answered_interstitial_prompt_for_pid: None,
            forward_task: None,
            initial_prompt_task: None,
            progress_monitor_task: None,
            progress_delivery_task: None,
            progress_monitor_generation: ProgressMonitorGeneration::default(),
            progress_effect_gate: std::sync::Arc::new(Mutex::new(())),
            progress_monitor: None,
            user_paused_goal_epoch: None,
            agent_goal_resume: None,
            agent_goal_resume_task: None,
            goal_reminder: None,
            goal_active_seen_epoch: None,
            goal_resume_submitted_at: None,
        }
    }

    /// Installs the output forwarder only after this runtime is present in
    /// the pane registry. A command can print its first line immediately on
    /// spawn; starting the forwarder before registration would let a Text
    /// Trigger observe that line but fail to write its reply into a runtime
    /// which has not yet become addressable.
    pub fn set_forward_task(&mut self, task: JoinHandle<()>) {
        if let Some(previous_task) = self.forward_task.replace(task) {
            previous_task.abort();
        }
    }

    /// Installs the pane-owned waiter for a one-shot initial agent prompt.
    pub fn set_initial_prompt_task(&mut self, task: JoinHandle<()>) {
        if let Some(previous_task) = self.initial_prompt_task.replace(task) {
            previous_task.abort();
        }
    }

    /// Cancels a not-yet-submitted automatic prompt once the user starts
    /// interacting with this newly-created terminal themselves.
    pub fn cancel_initial_prompt_delivery(&mut self) {
        if let Some(task) = self.initial_prompt_task.take() {
            task.abort();
        }
    }

    /// Installs this pane's progress-monitor loop task, aborting any
    /// previous one -- a fresh `SetPaneProgressMonitor` call always replaces
    /// rather than stacking a second concurrent loop on the same pane.
    pub fn set_progress_monitor_task(&mut self, task: JoinHandle<()>) {
        if let Some(previous_task) = self.progress_monitor_task.replace(task) {
            previous_task.abort();
        }
    }

    pub fn set_progress_delivery_task(&mut self, task: JoinHandle<()>) {
        if let Some(previous_task) = self.progress_delivery_task.replace(task) {
            previous_task.abort();
        }
    }

    pub fn cancel_progress_delivery_task(&mut self) {
        if let Some(task) = self.progress_delivery_task.take() {
            task.abort();
        }
    }

    pub fn set_agent_goal_resume_task(&mut self, task: JoinHandle<()>) {
        if let Some(previous_task) = self.agent_goal_resume_task.replace(task) {
            previous_task.abort();
        }
    }

    /// Drops a pending agent-requested resume and stops its waiter.
    pub fn cancel_agent_goal_resume(&mut self) {
        self.agent_goal_resume = None;
        if let Some(task) = self.agent_goal_resume_task.take() {
            task.abort();
        }
    }

    /// Records keyboard `/goal pause` and `/goal resume` so agent-requested
    /// resume never overrides a pause the user chose.
    pub fn observe_user_goal_command(&mut self, submitted_line: &str) {
        if pauses_agent_goal(submitted_line) {
            self.user_paused_goal_epoch = Some(self.goal_owner_epoch);
        } else if resumes_agent_goal(submitted_line) || clears_agent_goal(submitted_line) {
            self.user_paused_goal_epoch = None;
        }
    }

    /// Stops all automated progress work without discarding the accepted
    /// registration, sticky terminal evidence, or generation. Used by the
    /// global feature switch; explicit clear remains the destructive action.
    pub fn stop_progress_tasks_preserving_state(&mut self) {
        if let Some(task) = self.progress_monitor_task.take() {
            task.abort();
        }
        self.cancel_progress_delivery_task();
    }

    /// Commits an already-preflighted registration. The caller must hold the
    /// pane's `progress_effect_gate`, making replacement atomic with respect
    /// to any delivery's final validation and delayed Enter.
    pub fn install_progress_monitor(
        &mut self,
        registration: ProgressMonitorRegistration,
    ) -> Result<ProgressMonitorFence, String> {
        registration.validate().map_err(|error| error.to_string())?;
        // Validate every rejectable condition before aborting the old tasks so
        // a rejected replacement leaves the previous accepted monitor intact.
        let fence = self
            .progress_monitor_generation
            .activate(registration.monitor_id)
            .map_err(|error| error.to_string())?;
        if let Some(task) = self.progress_monitor_task.take() {
            task.abort();
        }
        if let Some(task) = self.progress_delivery_task.take() {
            task.abort();
        }
        self.progress_monitor = Some(ProgressMonitorRuntimeState {
            monitor_id: registration.monitor_id,
            command: registration.command,
            interval: registration.interval,
            latest_progress: registration.initial_progress,
            result_delivery: ProgressDeliveryState::NotQueued,
        });
        Ok(fence)
    }

    pub fn is_current_progress_monitor(&self, monitor_id: u64) -> bool {
        self.progress_monitor_generation.current() == Some(monitor_id)
            && self
                .progress_monitor
                .as_ref()
                .is_some_and(|monitor| monitor.monitor_id == monitor_id)
    }

    pub fn update_progress_monitor_progress(
        &mut self,
        monitor_id: u64,
        progress: PaneProgress,
    ) -> bool {
        if progress.monitor_id != monitor_id || !self.is_current_progress_monitor(monitor_id) {
            return false;
        }
        let Some(monitor) = self.progress_monitor.as_mut() else {
            return false;
        };
        monitor.latest_progress = progress;
        true
    }

    pub fn progress_monitor_status(&self, pane_id: NodeId) -> ProgressMonitorStatus {
        ProgressMonitorStatus {
            pane_id,
            progress: self
                .progress_monitor
                .as_ref()
                .map(|monitor| monitor.latest_progress.clone()),
        }
    }

    pub fn progress_monitor_snapshot(
        &self,
        pane_id: NodeId,
    ) -> Option<crate::persistence::PersistedProgressMonitor> {
        let monitor = self.progress_monitor.as_ref()?;
        Some(crate::persistence::PersistedProgressMonitor {
            pane_id,
            command: monitor.command.clone(),
            interval_seconds: monitor.interval.as_secs(),
            latest_progress: monitor.latest_progress.clone(),
            result_delivery: monitor.result_delivery.into(),
        })
    }

    /// Applies crash-recovery delivery evidence. Only a caller that separately
    /// checks `Queued` may retry it; attempted/uncertain states remain sticky
    /// evidence.
    pub fn restore_progress_delivery_state(
        &mut self,
        result_delivery: crate::persistence::PersistedProgressDeliveryState,
    ) -> Result<(), String> {
        let monitor = self
            .progress_monitor
            .as_mut()
            .ok_or_else(|| "no progress monitor is installed".to_string())?;
        monitor.result_delivery = result_delivery.into();
        Ok(())
    }

    /// Replaces detector-owned goal evidence while maintaining a stable
    /// epoch across phase-only transitions of the same process/provider.
    pub fn update_confirmed_goal_owner(&mut self, owner: Option<ConfirmedGoalOwner>) {
        let same_continuous_owner =
            same_goal_owner_identity(self.confirmed_goal_owner.as_ref(), owner.as_ref());
        if !same_continuous_owner && self.confirmed_goal_owner != owner {
            self.goal_owner_epoch = self.goal_owner_epoch.wrapping_add(1).max(1);
        }
        self.confirmed_goal_owner = owner;
        if self
            .confirmed_goal_owner
            .as_ref()
            .is_some_and(|owner| owner.goal_state == GoalState::Active)
        {
            self.goal_active_seen_epoch = Some(self.goal_owner_epoch);
        }
    }

    /// Explicit goal clearing is an identity boundary even before the next
    /// detector pass sees the provider's footer disappear.
    pub fn clear_confirmed_goal_owner(&mut self) {
        if self.confirmed_goal_owner.take().is_some() {
            self.goal_owner_epoch = self.goal_owner_epoch.wrapping_add(1).max(1);
        }
    }

    /// Cancels this pane's active progress-monitor loop, if any. Used by
    /// `ClearPaneProgressMonitor` and by the server's own progress-monitor
    /// setting being disabled mid-run.
    pub fn cancel_progress_monitor(&mut self) {
        if let Some(task) = self.progress_monitor_task.take() {
            task.abort();
        }
        if let Some(task) = self.progress_delivery_task.take() {
            task.abort();
        }
        if let Some(monitor) = self.progress_monitor.take() {
            self.progress_monitor_generation
                .clear_if_current(monitor.monitor_id);
        }
    }

    /// Cancels this pane's background forwarder task. Called when the pane
    /// is closed; does not touch `session` itself (killing the child
    /// process is the caller's separate responsibility via
    /// `session.kill()`, since a pane can also be torn down after its
    /// child already exited on its own).
    pub fn abort_background_tasks(&mut self) {
        if let Some(forward_task) = self.forward_task.take() {
            forward_task.abort();
        }
        self.cancel_initial_prompt_delivery();
        self.cancel_progress_monitor();
        self.cancel_agent_goal_resume();
    }
}

fn same_goal_owner_identity(
    previous: Option<&ConfirmedGoalOwner>,
    next: Option<&ConfirmedGoalOwner>,
) -> bool {
    previous.zip(next).is_some_and(|(previous, next)| {
        previous.process_id == next.process_id && previous.agent_class == next.agent_class
    })
}

/// Identity boundary for a server-retained goal signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedGoalOwner {
    pub process_id: u32,
    pub agent_class: AgentClass,
    pub goal_state: ilium_core::GoalState,
}

/// Returns the exact provider rule that invalidates a persisted identity.
/// Unknown/custom agents are accepted only when every built-in provider
/// reports the same rule, preserving the existing fail-closed consensus.
pub fn agent_session_identity_transition_rule(
    agent_class: Option<&AgentClass>,
    submitted_line: &str,
) -> Option<SessionIdentityTransitionRule> {
    if let Some(provider) = agent_class.and_then(AgentClass::provider) {
        return provider.session_identity_transition_rule(submitted_line);
    }

    let shared_rule =
        BuiltinAgentProvider::ALL[0].session_identity_transition_rule(submitted_line)?;
    BuiltinAgentProvider::ALL
        .into_iter()
        .all(|provider| {
            provider.session_identity_transition_rule(submitted_line) == Some(shared_rule)
        })
        .then_some(shared_rule)
}

/// Returns true only for the shared interactive command that starts a fresh
/// visible conversation without necessarily replacing the agent process or
/// its externally discoverable session identity.
pub fn clears_agent_conversation(submitted_line: &str) -> bool {
    submitted_line.trim() == "/clear"
}

/// Returns true only for the explicit command that removes a persistent goal.
/// Other `/goal` commands edit, pause, or resume the same attached goal and
/// must not clear the sidebar signal while their footer is temporarily hidden.
pub fn clears_agent_goal(submitted_line: &str) -> bool {
    submitted_line.split_whitespace().eq(["/goal", "clear"])
}

/// A submitted `/goal pause` (Codex).
pub fn pauses_agent_goal(submitted_line: &str) -> bool {
    submitted_line.split_whitespace().eq(["/goal", "pause"])
}

/// A submitted `/goal resume` (Codex).
pub fn resumes_agent_goal(submitted_line: &str) -> bool {
    submitted_line.split_whitespace().eq(["/goal", "resume"])
}

impl Drop for TerminalPaneRuntime {
    /// Belt-and-braces guard against leaked tasks: dropping a `JoinHandle`
    /// on its own does *not* cancel the underlying tokio task (it merely
    /// detaches it), so if some future close/teardown path ever forgot to
    /// call `abort_background_tasks` before letting this value go out of
    /// scope, `forward_task` would keep running -- and keep whatever it
    /// captured (the pty's output receiver, the broadcast sender) alive --
    /// for the rest of the process's life, and a still-pending
    /// `initial_prompt_task` would eventually fire and write its one-shot
    /// prompt into whatever pty session has since reused this pane's slot.
    /// Both aborts are idempotent, so this is a no-op on the normal path
    /// where the caller already aborted them explicitly.
    fn drop(&mut self) {
        self.abort_background_tasks();
    }
}

/// This pane's adaptive poll schedule, owned alongside it so the detection
/// loop (`crate::detection`) can read/update it under the same pane
/// registry lock it already needs to read `session.screen_text()` from --
/// no second lock, no risk of the schedule and the session state it
/// describes drifting out of sync under concurrent access.
pub struct DetectionSchedule {
    pub next_due: Instant,
    pub current_interval: Duration,
    /// Whether the attached client currently has this pane as its active
    /// view (`ilium_ipc::ClientRequest::SetPaneFocus`). While true,
    /// `crate::detection::interval_for` pins this pane to the loop's own
    /// the focused fast tier regardless of its classified status, since a
    /// pane the user is actually looking at should never lag behind the
    /// coarser working/idle tiers.
    pub client_focused: bool,
    /// Start of the current `crate::detection::force_check` debounce window.
    /// Repeated requests inside it are coalesced at the window boundary.
    pub last_forced: Option<Instant>,
    /// Increments for every user-triggered recheck request, including requests
    /// coalesced by the debounce window. A detection pass captures this value
    /// and cannot overwrite a newer request with its stale deadline/status.
    pub request_generation: u64,
    /// Process identity result associated with one refreshed `System` table.
    pub identity_system_generation: Option<u64>,
    pub cached_identity: Option<ilium_detect::AgentIdentity>,
    pub cached_screen_classification: Option<crate::detection::ScreenClassificationCache>,
}

/// What a pane resource should be built from -- either a terminal to spawn
/// per [`TerminalOrigin`], or an editor pointing at a (possibly
/// not-yet-chosen) path. Shared by the crash-recovery snapshot schema
/// (`crate::persistence::PaneSnapshot`, which needs exactly this to record
/// what to respawn) and by `crate::ipc::handlers::spawn_and_register_pane`
/// (which needs exactly this to actually do the respawning for both a
/// client-initiated `NewPane` and startup crash-recovery restoration), so it
/// lives here, next to the `TerminalOrigin` it wraps, rather than being
/// defined once per caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneSnapshotKind {
    Terminal(TerminalOrigin),
    Editor { path: Option<PathBuf> },
}

/// One entry in the server's pane registry: either a live terminal or an
/// editor pane's known file path.
pub enum PaneResource {
    Terminal(Box<TerminalPaneRuntime>),
    /// `None` means the editor pane was created with no file chosen yet.
    Editor {
        path: Option<PathBuf>,
    },
}

impl PaneResource {
    /// Cancels any background tasks this resource owns. A no-op for
    /// `Editor` (it owns none). Called on `ClosePane`/session teardown
    /// before the resource is dropped.
    pub fn abort_background_tasks(&mut self) {
        if let PaneResource::Terminal(runtime) = self {
            runtime.abort_background_tasks();
        }
    }
}

/// Spawns the pty for a new terminal pane per `origin`, at the default
/// starting size (see [`DEFAULT_PANE_ROWS`]/[`DEFAULT_PANE_COLS`]), rooted
/// at `cwd`. Synchronous and does not touch tokio -- the caller is
/// responsible for spawning the async forwarder task around the returned
/// session's `subscribe_output_bytes()` receiver.
pub struct SpawnedTerminalSession {
    pub session: PtySession,
    /// Known before launch only for a fresh exact `claude` command, where
    /// ilium supplies Claude Code's supported `--session-id` UUID itself.
    pub session_id: Option<String>,
}

struct TerminalLaunchPlan {
    command_line: Option<String>,
    session_id: Option<String>,
}

/// Builds the shell command and any identity ilium can know before spawn.
/// Exact fresh Claude launches receive a UUID through Claude's supported
/// `--session-id` flag; all other commands remain byte-for-byte user-owned.
fn terminal_launch_plan(origin: &TerminalOrigin) -> TerminalLaunchPlan {
    match origin {
        TerminalOrigin::PlainShell => TerminalLaunchPlan {
            command_line: None,
            session_id: None,
        },
        TerminalOrigin::Command(command_line) if command_line == "claude" => {
            // Unquoted deliberately: this command line is later fed whole to
            // `$SHELL -c` on Unix but to `cmd.exe /C` on Windows (see
            // `shell_command`), and single quotes are not a quote character
            // to `cmd.exe` -- they would reach `claude` as literal characters
            // in the id. A v4 UUID is only hex digits and hyphens, so it
            // never needs quoting on either shell.
            let session_id = uuid::Uuid::new_v4().to_string();
            TerminalLaunchPlan {
                command_line: Some(format!("claude --session-id {session_id}")),
                session_id: Some(session_id),
            }
        }
        TerminalOrigin::Command(command_line) => TerminalLaunchPlan {
            command_line: Some(command_line.clone()),
            session_id: None,
        },
    }
}

/// The interactive shell a pane runs, and the flag that makes it execute one
/// command line and exit.
///
/// `$SHELL` is the user's own choice wherever it is set, which on Unix is
/// essentially always. It is normally unset on Windows, and the Unix fallback
/// is not merely unhelpful there but fatal: `/bin/sh` does not exist, so every
/// pane spawn fails outright. Windows falls back to `%COMSPEC%`, and its
/// command flag is `/C` rather than `-c`.
fn shell_command() -> (String, &'static str) {
    if cfg!(windows) {
        let shell = std::env::var("SHELL")
            .or_else(|_| std::env::var("COMSPEC"))
            .unwrap_or_else(|_| "cmd.exe".to_string());
        // A `SHELL` pointing at a POSIX shell (Git Bash, MSYS) still takes
        // `-c`; only the `cmd.exe` family uses `/C`. Decide from the
        // executable's file stem, not the whole path -- a POSIX shell that
        // merely lives under a directory containing "cmd" (e.g.
        // `C:\cmder\...\bash.exe`) must still get `-c`.
        let is_cmd_family = Path::new(&shell)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.eq_ignore_ascii_case("cmd"));
        let flag = if is_cmd_family { "/C" } else { "-c" };
        (shell, flag)
    } else {
        (
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            "-c",
        )
    }
}

/// This session's identity, injected as environment variables into every
/// spawned terminal pane so a process running inside it -- e.g. the `ilium
/// progress set` CLI subcommand -- can address this exact pane on this exact
/// server without the caller needing to already know the session's
/// runtime-directory layout. No existing feature needed a pane to identify
/// itself this way (the file-backed chatroom feature is project-scoped, not
/// pane-scoped, and deliberately avoids the server entirely).
pub struct PaneIdentityEnv<'a> {
    pub pane_id: NodeId,
    pub session_name: &'a str,
    pub socket_path: &'a Path,
}

pub fn spawn_terminal_session(
    origin: &TerminalOrigin,
    cwd: &Path,
    identity: &PaneIdentityEnv<'_>,
) -> Result<SpawnedTerminalSession, PtyError> {
    let (shell, command_flag) = shell_command();
    let launch_plan = terminal_launch_plan(origin);
    let command = match launch_plan.command_line {
        None => PtyCommand::new(shell, cwd, DEFAULT_PANE_ROWS, DEFAULT_PANE_COLS),
        Some(command_line) => PtyCommand::new(shell, cwd, DEFAULT_PANE_ROWS, DEFAULT_PANE_COLS)
            .arg(command_flag)
            .arg(command_line),
    };
    let command = command
        .env(ilium_ipc::pane_env::PANE_ID, identity.pane_id.0.to_string())
        .env(ilium_ipc::pane_env::SESSION_NAME, identity.session_name)
        .env(
            ilium_ipc::pane_env::SESSION_SOCKET,
            identity.socket_path.to_string_lossy().into_owned(),
        );
    Ok(SpawnedTerminalSession {
        session: PtySession::spawn(command)?,
        session_id: launch_plan.session_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_fresh_claude_launch_gets_a_matching_uuid_argument() {
        let plan = terminal_launch_plan(&TerminalOrigin::Command("claude".to_string()));
        let session_id = plan.session_id.expect("generated session id");

        assert!(uuid::Uuid::parse_str(&session_id).is_ok());
        assert_eq!(
            plan.command_line,
            Some(format!("claude --session-id {session_id}"))
        );
    }

    #[test]
    fn non_exact_commands_are_not_rewritten() {
        for command_line in ["codex", "claude --dangerously-skip-permissions"] {
            let plan = terminal_launch_plan(&TerminalOrigin::Command(command_line.to_string()));
            assert_eq!(plan.command_line.as_deref(), Some(command_line));
            assert_eq!(plan.session_id, None);
        }
    }

    #[test]
    fn provider_specific_and_consensus_session_transitions_invalidate_identity() {
        for command in ["/resume", "/resume abc", "/branch release", "/new"] {
            assert!(
                agent_session_identity_transition_rule(Some(&AgentClass::Claude), command)
                    .is_some()
            );
            assert!(agent_session_identity_transition_rule(None, command).is_some());
        }
        for input in ["please run /resume", "/fork investigate", "resume"] {
            assert!(
                agent_session_identity_transition_rule(Some(&AgentClass::Codex), input).is_none()
            );
        }
        assert!(
            agent_session_identity_transition_rule(Some(&AgentClass::Codex), "/clear").is_some()
        );
        assert_eq!(
            agent_session_identity_transition_rule(Some(&AgentClass::Codex), "/clear"),
            Some(SessionIdentityTransitionRule::ClaudeOrCodexClearStartsFreshSession)
        );
        assert_eq!(
            agent_session_identity_transition_rule(Some(&AgentClass::Claude), "/clear"),
            Some(SessionIdentityTransitionRule::ClaudeOrCodexClearStartsFreshSession)
        );
        assert_eq!(
            agent_session_identity_transition_rule(None, "/resume another"),
            Some(SessionIdentityTransitionRule::SharedNewSessionCommand)
        );
        assert!(agent_session_identity_transition_rule(None, "/clear").is_none());
    }

    #[test]
    fn only_the_exact_clear_command_resets_visible_agent_history() {
        assert!(clears_agent_conversation("/clear"));
        assert!(clears_agent_conversation("  /clear  "));
        for command in ["/clear later", "please /clear", "/clearance", "/new"] {
            assert!(!clears_agent_conversation(command));
        }
    }

    #[test]
    fn only_the_exact_goal_clear_command_removes_confirmed_goal_ownership() {
        assert!(clears_agent_goal("/goal clear"));
        assert!(clears_agent_goal("  /goal   clear  "));
        for command in [
            "/goal",
            "/goal pause",
            "/goal resume",
            "/goal clear later",
            "please /goal clear",
        ] {
            assert!(!clears_agent_goal(command));
        }
    }

    #[test]
    fn only_exact_goal_pause_and_resume_commands_are_recognised() {
        assert!(pauses_agent_goal(" /goal   pause "));
        assert!(resumes_agent_goal("/goal resume"));
        for command in [
            "/goal",
            "/goal pause now",
            "please /goal pause",
            "/goal resume later",
        ] {
            assert!(!pauses_agent_goal(command), "{command}");
            assert!(!resumes_agent_goal(command), "{command}");
        }
    }

    #[test]
    fn goal_phase_changes_preserve_identity_but_clear_and_replacement_do_not() {
        let active = ConfirmedGoalOwner {
            process_id: 42,
            agent_class: AgentClass::Codex,
            goal_state: GoalState::Active,
        };
        let paused = ConfirmedGoalOwner {
            goal_state: GoalState::Paused,
            ..active.clone()
        };
        let replacement = ConfirmedGoalOwner {
            process_id: 43,
            ..active.clone()
        };

        assert!(same_goal_owner_identity(Some(&active), Some(&paused)));
        assert!(!same_goal_owner_identity(Some(&active), None));
        assert!(!same_goal_owner_identity(None, Some(&active)));
        assert!(!same_goal_owner_identity(Some(&active), Some(&replacement)));
    }

    #[test]
    fn cleared_standard_resume_titles_drop_the_stale_id() {
        assert_eq!(
            TerminalOrigin::Command(
                "claude --resume '11111111-1111-4111-8111-111111111111'".to_string()
            )
            .pane_name_without_stale_session(),
            "claude"
        );
        assert_eq!(
            TerminalOrigin::Command(
                "codex resume '11111111-1111-4111-8111-111111111111'".to_string()
            )
            .pane_name_without_stale_session(),
            "codex"
        );
        // Regression test: Antigravity is a full `BuiltinAgentProvider::ALL`
        // registry entry alongside Claude and Codex, so its persisted resume
        // command must drop its stale ID exactly like the other two instead
        // of falling through to the raw, still-stale command line.
        assert_eq!(
            TerminalOrigin::Command(
                "agy --conversation '11111111-1111-4111-8111-111111111111'".to_string()
            )
            .pane_name_without_stale_session(),
            "agy"
        );
        assert_eq!(
            TerminalOrigin::Command("custom resume value".to_string())
                .pane_name_without_stale_session(),
            "custom resume value"
        );
    }
}
