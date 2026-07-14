//! `PaneResource`: what `ilium-server` keeps in its pane registry for one
//! tree node -- either a live pty-backed terminal, or (for an editor pane)
//! just the path it points at, since editor content/editing stays
//! client-local (see README "Architecture": the server only needs to know
//! "this NodeId is an editor pointing at this path" for tree persistence
//! and for other attached clients to know what's open).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ilium_core::AgentClass;
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
    /// UUID supplied by ilium to an exact fresh `claude` launch. It is not
    /// published or persisted as an active session until detection confirms
    /// that this pane actually owns a live Claude process.
    pub pending_generated_session_id: Option<String>,
    pub detection_schedule: DetectionSchedule,
    /// This pane's agent session/thread ID, once `crate::session_id`
    /// discovers one. Rechecked while an agent is detected because `/resume`
    /// can replace the active session inside an existing terminal pane.
    pub session_id: Option<String>,
    /// Agent class that owned `session_id`; prevents a later different CLI in
    /// the same terminal from inheriting stale identity.
    pub session_agent_class: Option<AgentClass>,
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
            pending_generated_session_id,
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
            },
            session_id: None,
            session_agent_class: None,
            session_process_id: None,
            forward_task,
        }
    }

    /// Cancels this pane's background forwarder task. Called when the pane
    /// is closed; does not touch `session` itself (killing the child
    /// process is the caller's separate responsibility via
    /// `session.kill()`, since a pane can also be torn down after its
    /// child already exited on its own).
    pub fn abort_background_tasks(&self) {
        self.forward_task.abort();
    }
}

/// Returns true only for interactive commands known to replace the active
/// conversation within an already-running agent process. Invalidating on a
/// false positive would suppress a correct ID, so ordinary prompt content and
/// commands that keep the current conversation are deliberately excluded.
pub fn invalidates_agent_session_identity(submitted_line: &str) -> bool {
    matches!(
        submitted_line.split_whitespace().next(),
        Some("/resume" | "/branch" | "/new")
    )
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
    /// `BASE_TICK_INTERVAL` regardless of its classified status, since a
    /// pane the user is actually looking at should never lag behind the
    /// coarser working/idle tiers.
    pub client_focused: bool,
    /// Last time `crate::detection::force_check` actually pulled
    /// `next_due` forward for this pane, used to debounce repeated
    /// force-check requests (focus transitions, Enter keypresses) to at
    /// most one every `crate::detection::FORCE_CHECK_DEBOUNCE`.
    pub last_forced: Option<Instant>,
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
    pub fn abort_background_tasks(&self) {
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
    fn only_session_replacing_slash_commands_invalidate_identity() {
        for command in ["/resume", "/resume abc", "/branch release", "/new"] {
            assert!(invalidates_agent_session_identity(command));
        }
        for input in [
            "please run /resume",
            "/clear",
            "/fork investigate",
            "resume",
        ] {
            assert!(!invalidates_agent_session_identity(input));
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
