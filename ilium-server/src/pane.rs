//! `PaneResource`: what `ilium-server` keeps in its pane registry for one
//! tree node -- either a live pty-backed terminal, or (for an editor pane)
//! just the path it points at, since editor content/editing stays
//! client-local (see README "Architecture": the server only needs to know
//! "this NodeId is an editor pointing at this path" for tree persistence
//! and for other attached clients to know what's open).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ilium_core::{AgentClass, AgentProvider, BuiltinAgentProvider, SessionIdentityTransitionRule};
use ilium_pty::{PtyCommand, PtyError, PtySession};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

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
    /// their exact launch text.
    pub fn pane_name_without_stale_session(&self) -> &str {
        match self {
            TerminalOrigin::Command(command) if command.starts_with("claude --resume ") => "claude",
            TerminalOrigin::Command(command) if command.starts_with("codex resume ") => "codex",
            _ => self.default_pane_name(),
        }
    }
}

/// One live pty-backed terminal pane: the pty session itself, what it was
/// spawned to run, this pane's adaptive detection schedule, and the
/// background task that forwards its raw output bytes to attached clients.
pub struct TerminalPaneRuntime {
    pub session: PtySession,
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
    /// Forwards `session.subscribe_output_bytes()` chunks to the session's
    /// broadcast channel as `ServerEvent::ScreenUpdate` frames. Owned here
    /// so closing this pane has a single, unambiguous place to cancel it
    /// (see `CLAUDE.md`'s async-task-ownership rule) -- `abort_background_tasks`
    /// is the only way this handle is ever touched after creation.
    forward_task: JoinHandle<()>,
    /// One cancellable task that waits for a newly-launched agent's visible
    /// composer before submitting its one-shot initial request. It is owned by
    /// the pane so closing the pane or manually typing into it cannot leave a
    /// delayed prompt writing into a reused terminal.
    initial_prompt_task: Option<JoinHandle<()>>,
}

impl TerminalPaneRuntime {
    pub fn new(
        session: PtySession,
        origin: TerminalOrigin,
        pending_generated_session_id: Option<String>,
        initial_poll_interval: Duration,
        forward_task: JoinHandle<()>,
    ) -> Self {
        Self {
            session,
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
            },
            session_id: None,
            session_agent_class: None,
            detected_agent_process_id: None,
            detected_agent_class: None,
            session_process_id: None,
            forward_task,
            initial_prompt_task: None,
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

    /// Cancels this pane's background forwarder task. Called when the pane
    /// is closed; does not touch `session` itself (killing the child
    /// process is the caller's separate responsibility via
    /// `session.kill()`, since a pane can also be torn down after its
    /// child already exited on its own).
    pub fn abort_background_tasks(&mut self) {
        self.forward_task.abort();
        self.cancel_initial_prompt_delivery();
    }
}

/// Identity boundary for a server-retained goal signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedGoalOwner {
    pub process_id: u32,
    pub agent_class: AgentClass,
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

impl Drop for TerminalPaneRuntime {
    /// Belt-and-braces guard against a leaked forwarder task: dropping a
    /// `JoinHandle` on its own does *not* cancel the underlying tokio task
    /// (it merely detaches it), so if some future close/teardown path ever
    /// forgot to call `abort_background_tasks` before letting this value
    /// go out of scope, `forward_task` would keep running -- and keep
    /// whatever it captured (the pty's output receiver, the broadcast
    /// sender) alive -- for the rest of the process's life. `abort` is
    /// idempotent, so this is a no-op on the normal path where the caller
    /// already aborted it explicitly.
    fn drop(&mut self) {
        self.forward_task.abort();
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
            let session_id = uuid::Uuid::new_v4().to_string();
            TerminalLaunchPlan {
                command_line: Some(format!("claude --session-id '{session_id}'")),
                session_id: Some(session_id),
            }
        }
        TerminalOrigin::Command(command_line) => TerminalLaunchPlan {
            command_line: Some(command_line.clone()),
            session_id: None,
        },
    }
}

pub fn spawn_terminal_session(
    origin: &TerminalOrigin,
    cwd: &Path,
) -> Result<SpawnedTerminalSession, PtyError> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let launch_plan = terminal_launch_plan(origin);
    let command = match launch_plan.command_line {
        None => PtyCommand::new(shell, cwd, DEFAULT_PANE_ROWS, DEFAULT_PANE_COLS),
        Some(command_line) => PtyCommand::new(shell, cwd, DEFAULT_PANE_ROWS, DEFAULT_PANE_COLS)
            .arg("-c")
            .arg(command_line),
    };
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
            Some(format!("claude --session-id '{session_id}'"))
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
    fn cleared_standard_resume_titles_drop_the_stale_id() {
        assert_eq!(
            TerminalOrigin::Command(
                "claude --resume '11111111-1111-4111-8111-111111111111'".to_string()
            )
            .pane_name_without_stale_session(),
            "claude"
        );
        assert_eq!(
            TerminalOrigin::Command("custom resume value".to_string())
                .pane_name_without_stale_session(),
            "custom resume value"
        );
    }
}
