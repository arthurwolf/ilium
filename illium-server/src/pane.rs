//! `PaneResource`: what `illium-server` keeps in its pane registry for one
//! tree node -- either a live pty-backed terminal, or (for an editor pane)
//! just the path it points at, since editor content/editing stays
//! client-local (see README "Architecture": the server only needs to know
//! "this NodeId is an editor pointing at this path" for tree persistence
//! and for other attached clients to know what's open).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use illium_pty::{PtyCommand, PtyError, PtySession};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

/// Default pty size a newly-created terminal pane starts at, before the
/// client that requested it reports its real viewport via `ResizePane`.
/// `illium_ipc::ClientRequest::NewPane` carries no size (only
/// `ResizePane` does), so the server picks a reasonable starting point
/// rather than blocking pane creation on a size the client hasn't sent
/// yet.
pub const DEFAULT_PANE_ROWS: u16 = 24;
pub const DEFAULT_PANE_COLS: u16 = 80;

/// What a terminal pane was spawned to run -- kept separate from
/// `illium_ipc::NewPaneKind` (which also has an `Editor` variant that can
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
    /// client didn't otherwise specify one (`illium_ipc::ClientRequest::NewPane`
    /// carries no title -- naming/renaming is a separate, later concern
    /// handled by `RenameNode` and, client-side, title inference).
    pub fn default_pane_name(&self) -> &str {
        match self {
            TerminalOrigin::PlainShell => "shell",
            TerminalOrigin::Command(command_line) => command_line,
        }
    }
}

/// One live pty-backed terminal pane: the pty session itself, what it was
/// spawned to run, this pane's adaptive detection schedule, and the
/// background task that forwards its raw output bytes to attached clients.
pub struct TerminalPaneRuntime {
    pub session: PtySession,
    pub origin: TerminalOrigin,
    pub detection_schedule: DetectionSchedule,
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
        initial_poll_interval: Duration,
        forward_task: JoinHandle<()>,
    ) -> Self {
        Self {
            session,
            origin,
            detection_schedule: DetectionSchedule {
                // Checked on the very next detection tick rather than
                // waiting a full interval -- a freshly-spawned pane's
                // status (e.g. "is this actually an agent CLI") is not
                // yet known and should resolve promptly.
                next_due: Instant::now(),
                current_interval: initial_poll_interval,
            },
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

/// This pane's adaptive poll schedule, owned alongside it so the detection
/// loop (`crate::detection`) can read/update it under the same pane
/// registry lock it already needs to read `session.screen_text()` from --
/// no second lock, no risk of the schedule and the session state it
/// describes drifting out of sync under concurrent access.
pub struct DetectionSchedule {
    pub next_due: Instant,
    pub current_interval: Duration,
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
    Terminal(TerminalPaneRuntime),
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
pub fn spawn_terminal_session(origin: &TerminalOrigin, cwd: &Path) -> Result<PtySession, PtyError> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let command = match origin {
        TerminalOrigin::PlainShell => {
            PtyCommand::new(shell, cwd, DEFAULT_PANE_ROWS, DEFAULT_PANE_COLS)
        }
        TerminalOrigin::Command(command_line) => {
            PtyCommand::new(shell, cwd, DEFAULT_PANE_ROWS, DEFAULT_PANE_COLS)
                .arg("-c")
                .arg(command_line)
        }
    };
    PtySession::spawn(command)
}
