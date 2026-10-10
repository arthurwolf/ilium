//! One spawned PTY, its shared screen/journal read views, and a single owned
//! state actor. Only that actor parses output, commits geometry, and orders
//! input, mouse events, and terminal-query replies. Native pumps move bytes.
//! Synchronous compatibility calls wait for completed receipts; async server
//! callers clone `PtyInput` and await outside pane/tree registry locks.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crossterm::event::MouseEvent;
use portable_pty::{native_pty_system, Child, CommandBuilder, ExitStatus, PtySize};
use tokio::sync::{broadcast, watch};

use crate::admission::PtyWorkerReservations;
use crate::error::PtyError;
use crate::owner::{OwnerLimits, PtyInput, PtyOwner, TerminalState};
use crate::query::TerminalQueryResponder;
use crate::screen_reader::ScreenReader;
use ilium_platform::owned_worker::{OwnedWorker, StopToken, WorkerKind};
use ilium_platform::pty_io::ShellProbe;

/// Terminal identity exported to every application running behind Ilium's
/// `vt100` emulator. This must describe the emulator, not the terminal that
/// happens to display the outer Ilium client.
const EMULATED_TERMINAL_TYPE: &str = "xterm-256color";

/// Outer-terminal identity and multiplexer markers that would make a child
/// infer capabilities Ilium does not implement. In particular, Codex treats
/// `TERM_PROGRAM=WezTerm`, `WEZTERM_VERSION`, or `KITTY_WINDOW_ID` as proof
/// that Kitty graphics are available and then writes Base64 image payloads
/// into a PTY whose `vt100` emulator cannot render them.
const OUTER_TERMINAL_IDENTITY_ENVIRONMENT_PREFIXES: &[&str] =
    &["WEZTERM_", "KITTY_", "TMUX_", "ZELLIJ_"];

/// Returns whether an environment key must never cross this pty boundary,
/// whatever the outer process or the caller set. Two distinct reasons share
/// one filter so both are decided from the same normalized key:
///
/// - Outer terminal/multiplexer identity, which would make a child infer
///   capabilities Ilium's `vt100` emulator does not implement.
/// - `NO_COLOR`, a monochrome policy inherited by the Ilium process that must
///   not follow into its fully color-capable child PTYs.
fn is_environment_variable_filtered_at_pty_boundary(key: &OsStr) -> bool {
    // Environment names are conventionally ASCII, while normalizing here
    // also covers Windows' case-insensitive environment semantics -- an
    // inherited `no_color` suppresses color there exactly like `NO_COLOR`,
    // so both filters must read the same normalized key rather than one
    // comparing the raw one.
    let normalized_key = key.to_string_lossy().to_ascii_uppercase();

    if matches!(
        normalized_key.as_str(),
        "TERM_PROGRAM" | "TERM_PROGRAM_VERSION" | "TMUX" | "ZELLIJ" | "NO_COLOR"
    ) {
        return true;
    }

    OUTER_TERMINAL_IDENTITY_ENVIRONMENT_PREFIXES
        .iter()
        .any(|prefix| normalized_key.starts_with(prefix))
}

/// Applies the terminal-emulation contract after every caller-supplied
/// environment entry, so a pane cannot accidentally or explicitly advertise
/// outer-emulator capabilities that are absent at this PTY boundary.
fn configure_emulated_terminal_environment(
    command: &mut CommandBuilder,
    caller_environment: &[(String, String)],
) {
    // Rebuild the environment instead of maintaining an inevitably
    // incomplete list of vendor-specific variable names. This preserves the
    // user's ordinary environment while filtering whole terminal families,
    // including variables introduced by future emulator versions.
    command.env_clear();
    for (key, value) in std::env::vars_os() {
        if !is_environment_variable_filtered_at_pty_boundary(&key) {
            command.env(key, value);
        }
    }

    // Caller overrides remain supported for normal variables, but cannot
    // contradict the terminal capability contract at this boundary.
    for (key, value) in caller_environment {
        if !is_environment_variable_filtered_at_pty_boundary(key.as_ref()) {
            command.env(key, value);
        }
    }

    command.env("TERM", EMULATED_TERMINAL_TYPE);
}

/// Describes a command to spawn behind a pty: the program, its arguments,
/// starting working directory, and initial screen size.
///
/// Built with a small owned-`String` builder rather than borrowing, since
/// the command is consumed once inside [`PtySession::spawn`] and then
/// discarded -- there's no repeated-call path that would benefit from
/// borrowing instead of allocating.
pub struct PtyCommand {
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
    rows: u16,
    cols: u16,
    env: Vec<(String, String)>,
}

impl PtyCommand {
    /// Starts building a command for `program`, run with `cwd` as its
    /// working directory and an initial pty size of `rows` x `cols`.
    pub fn new(program: impl Into<String>, cwd: impl Into<PathBuf>, rows: u16, cols: u16) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            rows,
            cols,
            env: Vec::new(),
        }
    }

    /// Appends one argument (builder-style).
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Appends an extra environment variable (builder-style). `TERM` is
    /// always set by [`PtySession::spawn`] regardless of what's passed
    /// here -- see the comment there -- so setting it again is a no-op.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// An authoritative wait observation for this session's directly spawned child.
/// It says nothing about descendants such as an agent started by a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyChildExit {
    pub process_id: Option<u32>,
    pub cause: PtyExitCause,
}

/// Preserve a named signal rather than portable-pty's placeholder code of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyExitCause {
    ExitCode(u32),
    Signal(String),
}

fn retain_child_exit(
    receipt: &Mutex<Option<PtyChildExit>>,
    process_id: Option<u32>,
    status: ExitStatus,
) {
    let mut retained = receipt.lock().unwrap_or_else(|error| error.into_inner());
    if retained.is_none() {
        let cause = match status.signal() {
            Some(signal) => PtyExitCause::Signal(signal.to_string()),
            None => PtyExitCause::ExitCode(status.exit_code()),
        };
        *retained = Some(PtyChildExit { process_id, cause });
    }
}

/// Captured control for the original PTY child. Native termination and the
/// child mutex may block; execute this handle on an owned worker after
/// releasing the server registry. Sharing the original child handle preserves
/// its reaper and avoids signalling a recycled bare PID.
#[derive(Clone)]
pub struct PtyTerminationHandle {
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    child_exit: Arc<Mutex<Option<PtyChildExit>>>,
    process_id: Option<u32>,
    identity: Option<Arc<ilium_platform::process_control::PtyProcessIdentity>>,
}

impl PtyTerminationHandle {
    pub fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.child, &other.child)
    }

    pub fn terminate_process_tree(
        &self,
        timeout: std::time::Duration,
    ) -> std::io::Result<ilium_platform::process_control::PtyTerminationProof> {
        let identity = self.identity.as_ref().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "PTY child birth identity was not captured",
            )
        })?;
        ilium_platform::process_control::terminate_pty_process_tree(identity, timeout)
    }

    pub fn kill_direct_child(&self) -> Result<(), PtyError> {
        let mut child = self.child.lock().map_err(|_| {
            PtyError::Kill(std::io::Error::other("PTY child control lock poisoned"))
        })?;
        if let Ok(Some(status)) = child.try_wait() {
            retain_child_exit(&self.child_exit, self.process_id, status);
            return Ok(());
        }
        child.kill().map_err(PtyError::Kill)
    }
}

/// Equality is pointer identity, not equality of the empty marker value.
/// Keeping a clone cannot keep the native child or ordered PTY owner alive.
#[derive(Clone, Default)]
pub struct PtySessionIdentity(Arc<()>);

impl PartialEq for PtySessionIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for PtySessionIdentity {}

impl std::fmt::Debug for PtySessionIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PtySessionIdentity")
    }
}

/// Read-only native foreground observation captured from one PTY lifetime.
/// The OS inspection may block (ToolHelp on Windows); callers must execute it
/// on an admitted worker, never while holding a server registry guard.
#[derive(Clone)]
pub struct PtyShellObserver {
    identity: PtySessionIdentity,
    probe: ShellProbe,
    process_id: Option<u32>,
}

impl PtyShellObserver {
    pub fn identity(&self) -> &PtySessionIdentity {
        &self.identity
    }

    pub fn shell_owns_terminal(&self) -> Option<bool> {
        self.probe.shell_owns_terminal(self.process_id?)
    }
}

/// One spawned command behind a pty, plus the `vt100` parser that turns its
/// raw byte stream into a renderable/queryable screen.
pub struct PtySession {
    // The actor owns all parser writes, geometry control, and transport writes.
    // This handle only exposes admission and cancellation to other threads.
    #[cfg(test)]
    parser: Arc<RwLock<vt100::Parser<TerminalQueryResponder>>>,
    screen_generation: Arc<AtomicU64>,
    screen_reader: ScreenReader,
    owner: PtyOwner,
    identity: PtySessionIdentity,
    shell_probe: ShellProbe,
    // The reaper holds only the child, never parser or journal. It polls even
    // when a descendant keeps the slave fd open after the direct child exits.
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    _child_reaper: OwnedWorker,
    child_exit: Arc<Mutex<Option<PtyChildExit>>>,
    // OS pid of the directly-spawned child, if the platform reported one.
    process_id: Option<u32>,
    // Captured while the PTY child is still owned. Worktree teardown must
    // refuse a stale/reused PID rather than signal an unrelated process.
    pty_process_identity: Option<Arc<ilium_platform::process_control::PtyProcessIdentity>>,
    // Held so `subscribe_screen_changed` can hand out clones; a `watch`
    // receiver never lets its sender's send fail as "no receivers left"
    // while at least one clone (this one) is alive.
    screen_changed: watch::Receiver<()>,
    // Sender half of the raw-output-bytes broadcast; kept here (rather than
    // only inside the reader thread's closure) so `subscribe_output_bytes`
    // can hand out new receivers at any point in the session's lifetime,
    // including after every previous subscriber has dropped its receiver.
    output_bytes: broadcast::Sender<PtyOutputChunk>,
    /// Bounded, session-owned replay log. A client may attach long after a
    /// pane began producing output, so a live-only broadcast cannot be the
    /// sole source for its terminal parser or its scrollback.
    output_journal: Arc<Mutex<OutputJournal>>,
}

/// One ordered piece of raw output from a PTY. The sequence belongs to the
/// pane's lifetime, letting clients discard a live broadcast that was already
/// included in their attach-time replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOutputChunk {
    pub sequence: u64,
    /// Shared by the replay journal and every live subscriber. The reader
    /// allocates each PTY chunk once, straight from its read buffer; cloning a
    /// journal/broadcast entry then only advances a reference count instead of
    /// copying up to one whole 64 KiB read.
    pub bytes: Arc<[u8]>,
}

/// One internally-consistent visible-screen read. Detection carries the
/// generation through its lock-free classification phase so it can promptly
/// revisit a pane when newer PTY output or a resize arrives before apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSnapshot {
    pub generation: u64,
    pub text: String,
    /// Zero-based cursor row and column from the same parser frame as
    /// `text`. Automated agent input uses this to distinguish rendered
    /// placeholder text from a dirty composer whose cursor advanced through
    /// user-authored input.
    pub cursor_position: (u16, u16),
    /// Row-major coordinates of visible cells carrying the terminal's dim
    /// attribute. Codex uses this attribute for composer placeholder text but
    /// not for user-authored drafts, so automated delivery can fail closed
    /// even when the user moved a dirty draft's cursor back to column zero.
    pub dimmed_cells: Vec<(u16, u16)>,
}

impl ScreenSnapshot {
    /// Whether the cell at `row`, `column` carried the dim attribute in this
    /// exact parser frame.
    #[must_use]
    pub fn is_cell_dimmed(&self, row: u16, column: u16) -> bool {
        self.dimmed_cells.binary_search(&(row, column)).is_ok()
    }
}

/// A consistent, bounded replay of a pane's output through `through_sequence`.
/// `is_complete` is false only after the safety cap discarded the oldest
/// output; callers must reset their parser before feeding the retained tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOutputReplay {
    pub through_sequence: u64,
    pub bytes: Vec<u8>,
    pub is_complete: bool,
}

/// Exact size and sequence of a replay before its bytes are copied. A caller
/// can reserve byte capacity using this value, then call
/// [`PtySession::output_replay_if_unchanged`] to avoid cloning stale history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyOutputReplayEstimate {
    pub through_sequence: u64,
    pub first_sequence: Option<u64>,
    pub byte_len: usize,
    pub is_complete: bool,
}

/// Size and sequence for a replay or contiguous output delta before copying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyOutputRecoveryEstimate {
    Delta {
        through_sequence: u64,
        byte_len: usize,
    },
    Replay(PtyOutputReplayEstimate),
}

/// Minimal repair for one downstream consumer that last received
/// `after_sequence`. A retained contiguous tail can be appended directly;
/// only a consumer older than the journal's retained window needs a reset
/// plus full replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyOutputRecovery {
    Delta(PtyOutputChunk),
    Replay(PtyOutputReplay),
}

/// The PTY reader owns mutation of this journal; attachment handlers only
/// clone snapshots through the mutex. Keeping it here makes history survive
/// client detach/reattach without making the server retain a second parser.
struct OutputJournal {
    chunks: VecDeque<PtyOutputChunk>,
    retained_bytes: usize,
    next_sequence: u64,
    is_complete: bool,
}

impl OutputJournal {
    /// A hard byte ceiling prevents a pane that streams binary data forever
    /// from consuming unbounded server memory. It is deliberately much larger
    /// than ordinary 10,000-line agent transcripts.
    const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;

    /// Takes the reader's borrowed slice rather than an owned `Vec` so the
    /// chunk really is allocated once: `Arc<[u8]>` cannot adopt a `Vec`'s
    /// allocation (it needs room for the reference count in front of the
    /// bytes), so handing one in would copy the chunk a second time.
    #[cfg(test)]
    fn append(&mut self, bytes: &[u8]) -> PtyOutputChunk {
        self.append_shared(Arc::from(bytes))
    }

    fn append_shared(&mut self, bytes: Arc<[u8]>) -> PtyOutputChunk {
        self.next_sequence = self.next_sequence.saturating_add(1);
        let chunk = PtyOutputChunk {
            sequence: self.next_sequence,
            bytes,
        };
        self.retained_bytes = self.retained_bytes.saturating_add(chunk.bytes.len());
        self.chunks.push_back(chunk.clone());

        while self.retained_bytes > Self::MAX_RETAINED_BYTES {
            let Some(removed) = self.chunks.pop_front() else {
                break;
            };
            self.retained_bytes = self.retained_bytes.saturating_sub(removed.bytes.len());
            self.is_complete = false;
        }
        chunk
    }

    fn replay_estimate(&self) -> PtyOutputReplayEstimate {
        PtyOutputReplayEstimate {
            through_sequence: self.next_sequence,
            first_sequence: self.chunks.front().map(|chunk| chunk.sequence),
            byte_len: self.retained_bytes + usize::from(!self.is_complete) * 3,
            is_complete: self.is_complete,
        }
    }

    fn replay(&self) -> PtyOutputReplay {
        // A retained tail can begin halfway through an escape sequence or
        // depend on a mode established before the byte cap. Reset the client
        // parser first in that exceptional case so it never inherits a bogus
        // half-state from bytes that are no longer available.
        let reset_prefix = (!self.is_complete).then_some(b"\x1bc".as_slice());
        let mut bytes =
            Vec::with_capacity(self.retained_bytes + reset_prefix.map_or(0, |prefix| prefix.len()));
        if let Some(prefix) = reset_prefix {
            bytes.extend_from_slice(prefix);
        }
        for chunk in &self.chunks {
            bytes.extend_from_slice(&chunk.bytes);
        }
        PtyOutputReplay {
            through_sequence: self.next_sequence,
            bytes,
            is_complete: self.is_complete,
        }
    }

    /// Copies the largest retained replay prefix that ends at a PTY read
    /// boundary and fits within `max_bytes`. A truncated journal includes its
    /// parser reset in that budget so a consumer can safely render the tail.
    fn replay_prefix(&self, max_bytes: usize) -> Option<PtyOutputReplay> {
        self.replay_prefix_through(max_bytes, self.next_sequence)
    }

    fn replay_prefix_through(
        &self,
        max_bytes: usize,
        through_sequence: u64,
    ) -> Option<PtyOutputReplay> {
        if through_sequence > self.next_sequence {
            return None;
        }
        let reset_prefix = (!self.is_complete).then_some(b"\x1bc".as_slice());
        let reset_len = reset_prefix.map_or(0, <[u8]>::len);
        if max_bytes < reset_len {
            return None;
        }

        let mut bytes = Vec::with_capacity(max_bytes.min(self.retained_bytes + reset_len));
        if let Some(prefix) = reset_prefix {
            bytes.extend_from_slice(prefix);
        }

        let mut last_sequence = None;
        for chunk in self
            .chunks
            .iter()
            .take_while(|chunk| chunk.sequence <= through_sequence)
        {
            if bytes.len().saturating_add(chunk.bytes.len()) > max_bytes {
                break;
            }
            bytes.extend_from_slice(&chunk.bytes);
            last_sequence = Some(chunk.sequence);
        }

        last_sequence.map(|through_sequence| PtyOutputReplay {
            through_sequence,
            bytes,
            is_complete: self.is_complete,
        })
    }

    fn replay_if_unchanged(&self, expected: PtyOutputReplayEstimate) -> Option<PtyOutputReplay> {
        (self.replay_estimate() == expected).then(|| self.replay())
    }

    fn replay_through_if_retained(
        &self,
        expected: PtyOutputReplayEstimate,
    ) -> Option<PtyOutputReplay> {
        if expected.through_sequence > self.next_sequence
            || (expected.is_complete && !self.is_complete)
        {
            return None;
        }
        if let (Some(expected_first), Some(current_first)) = (
            expected.first_sequence,
            self.chunks.front().map(|chunk| chunk.sequence),
        ) {
            if current_first > expected_first {
                return None;
            }
        } else if expected.first_sequence.is_some() {
            return None;
        }

        let retained_bytes: usize = self
            .chunks
            .iter()
            .take_while(|chunk| chunk.sequence <= expected.through_sequence)
            .map(|chunk| chunk.bytes.len())
            .sum();
        let reset_prefix = (!expected.is_complete).then_some(b"\x1bc".as_slice());
        let expected_len = retained_bytes + reset_prefix.map_or(0, |prefix| prefix.len());
        if expected_len != expected.byte_len {
            return None;
        }
        let mut bytes = Vec::with_capacity(expected_len);
        if let Some(prefix) = reset_prefix {
            bytes.extend_from_slice(prefix);
        }
        for chunk in self
            .chunks
            .iter()
            .take_while(|chunk| chunk.sequence <= expected.through_sequence)
        {
            bytes.extend_from_slice(&chunk.bytes);
        }
        Some(PtyOutputReplay {
            through_sequence: expected.through_sequence,
            bytes,
            is_complete: expected.is_complete,
        })
    }

    /// Returns only output newer than `after_sequence` when every missing
    /// chunk remains retained. Falling behind the retained window requires a
    /// full replay because terminal escape streams cannot skip bytes safely.
    fn recovery_after(&self, after_sequence: u64) -> Option<PtyOutputRecovery> {
        if after_sequence >= self.next_sequence {
            return None;
        }

        let first_retained_sequence = self
            .chunks
            .front()
            .map(|chunk| chunk.sequence)
            .unwrap_or_else(|| self.next_sequence.saturating_add(1));
        if after_sequence.saturating_add(1) < first_retained_sequence {
            return Some(PtyOutputRecovery::Replay(self.replay()));
        }

        let retained_bytes = self
            .chunks
            .iter()
            .filter(|chunk| chunk.sequence > after_sequence)
            .map(|chunk| chunk.bytes.len())
            .sum();
        let mut bytes = Vec::with_capacity(retained_bytes);
        for chunk in self
            .chunks
            .iter()
            .filter(|chunk| chunk.sequence > after_sequence)
        {
            bytes.extend_from_slice(&chunk.bytes);
        }
        Some(PtyOutputRecovery::Delta(PtyOutputChunk {
            sequence: self.next_sequence,
            bytes: Arc::from(bytes),
        }))
    }

    /// Returns one bounded replay or contiguous delta without splitting a
    /// PTY read. Repeated calls from the returned sequence drain the same
    /// retained journal progressively instead of building one large frame.
    fn recovery_prefix_after(
        &self,
        after_sequence: u64,
        max_bytes: usize,
    ) -> Option<PtyOutputRecovery> {
        self.recovery_prefix_through(after_sequence, self.next_sequence, max_bytes)
    }

    fn recovery_prefix_through(
        &self,
        after_sequence: u64,
        through_sequence: u64,
        max_bytes: usize,
    ) -> Option<PtyOutputRecovery> {
        if through_sequence > self.next_sequence || through_sequence <= after_sequence {
            return None;
        }
        if after_sequence >= self.next_sequence || max_bytes == 0 {
            return None;
        }

        let first_retained_sequence = self
            .chunks
            .front()
            .map(|chunk| chunk.sequence)
            .unwrap_or_else(|| self.next_sequence.saturating_add(1));
        if after_sequence.saturating_add(1) < first_retained_sequence {
            return self
                .replay_prefix_through(max_bytes, through_sequence)
                .map(PtyOutputRecovery::Replay);
        }

        let mut bytes = Vec::with_capacity(max_bytes.min(self.retained_bytes));
        let mut last_sequence = None;
        for chunk in self
            .chunks
            .iter()
            .filter(|chunk| chunk.sequence > after_sequence && chunk.sequence <= through_sequence)
        {
            if bytes.len().saturating_add(chunk.bytes.len()) > max_bytes {
                break;
            }
            bytes.extend_from_slice(&chunk.bytes);
            last_sequence = Some(chunk.sequence);
        }

        last_sequence.map(|sequence| {
            PtyOutputRecovery::Delta(PtyOutputChunk {
                sequence,
                bytes: Arc::from(bytes),
            })
        })
    }

    fn recovery_estimate_after(&self, after_sequence: u64) -> Option<PtyOutputRecoveryEstimate> {
        if after_sequence >= self.next_sequence {
            return None;
        }
        let first_retained_sequence = self
            .chunks
            .front()
            .map(|chunk| chunk.sequence)
            .unwrap_or_else(|| self.next_sequence.saturating_add(1));
        if after_sequence.saturating_add(1) < first_retained_sequence {
            return Some(PtyOutputRecoveryEstimate::Replay(self.replay_estimate()));
        }
        let byte_len = self
            .chunks
            .iter()
            .filter(|chunk| chunk.sequence > after_sequence)
            .map(|chunk| chunk.bytes.len())
            .sum();
        Some(PtyOutputRecoveryEstimate::Delta {
            through_sequence: self.next_sequence,
            byte_len,
        })
    }

    fn recovery_if_unchanged(
        &self,
        after_sequence: u64,
        expected: PtyOutputRecoveryEstimate,
    ) -> Option<PtyOutputRecovery> {
        match expected {
            PtyOutputRecoveryEstimate::Replay(replay) => self
                .replay_through_if_retained(replay)
                .map(PtyOutputRecovery::Replay),
            PtyOutputRecoveryEstimate::Delta {
                through_sequence,
                byte_len,
            } => {
                if through_sequence > self.next_sequence || through_sequence <= after_sequence {
                    return None;
                }
                let first_retained_sequence = self
                    .chunks
                    .front()
                    .map(|chunk| chunk.sequence)
                    .unwrap_or_else(|| self.next_sequence.saturating_add(1));
                if after_sequence.saturating_add(1) < first_retained_sequence {
                    return None;
                }
                let chunks = self.chunks.iter().filter(|chunk| {
                    chunk.sequence > after_sequence && chunk.sequence <= through_sequence
                });
                let actual_bytes: usize = chunks.clone().map(|chunk| chunk.bytes.len()).sum();
                if actual_bytes != byte_len {
                    return None;
                }
                let mut bytes = Vec::with_capacity(byte_len);
                for chunk in chunks {
                    bytes.extend_from_slice(&chunk.bytes);
                }
                Some(PtyOutputRecovery::Delta(PtyOutputChunk {
                    sequence: through_sequence,
                    bytes: Arc::from(bytes),
                }))
            }
        }
    }
}

/// The smallest pty dimension this crate will ever forward to the OS pty or
/// the `vt100` parser. `vt100`'s grid arithmetic (`rows - 1`, `cols - 1`,
/// used pervasively in `Grid::new`/`set_size`/cursor clamping) underflows the
/// instant either dimension is `0` -- a `u16` panic in debug builds, and in
/// release builds a wrapped `65535` that later causes a genuine
/// out-of-bounds `Vec` index (bounds-checked regardless of profile) the next
/// time the parser processes a scrolling escape sequence. The OS pty itself
/// has no such restriction (`ioctl(TIOCSWINSZ)` accepts `0x0` without
/// complaint), so a `0` can legitimately arrive here -- e.g. a client's
/// window briefly reporting no size during a layout transition -- and must
/// be clamped at this crate's boundary rather than forwarded verbatim.
const MINIMUM_PTY_DIMENSION: u16 = 1;

/// Clamps a requested pty dimension to [`MINIMUM_PTY_DIMENSION`]; see that
/// constant's doc comment for why `0` can never reach the OS pty or the
/// `vt100` parser.
fn clamp_pty_dimension(requested: u16) -> u16 {
    requested.max(MINIMUM_PTY_DIMENSION)
}

/// Child reaping is independent of PTY output, since a descendant may keep
/// its inherited slave fd open after the direct child exits.
///
/// The reaper starts at the short interval (short-lived commands are reaped
/// promptly) and backs off to the idle interval for long-running children:
/// with hundreds of panes a fixed 50 ms poll alone cost thousands of wake-ups
/// and wait syscalls a second. Exit observation does not depend on this
/// cadence (`child_exit` queries the child itself), and cancellation wakes the
/// reaper immediately.
const CHILD_REAP_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);
const CHILD_REAP_IDLE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// A wake-up the reaper sleeps on, so cancellation does not wait a full
/// (backed-off) poll interval.
#[derive(Default)]
struct ReaperWake {
    is_woken: Mutex<bool>,
    changed: std::sync::Condvar,
}

impl ReaperWake {
    fn wake(&self) {
        *self
            .is_woken
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = true;
        self.changed.notify_all();
    }

    fn sleep(&self, timeout: std::time::Duration) {
        let is_woken = self
            .is_woken
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (mut is_woken, _) = self
            .changed
            .wait_timeout_while(is_woken, timeout, |is_woken| !*is_woken)
            .unwrap_or_else(|error| error.into_inner());
        *is_woken = false;
    }
}

impl PtySession {
    /// Spawns `command` behind a new pty and starts the transport pumps and
    /// the sole state owner that feeds its output into `vt100::Parser`.
    pub fn spawn(command: PtyCommand) -> Result<Self, PtyError> {
        Self::spawn_with_optional_quota(command, None)
    }

    /// Spawns a PTY whose persistent OS workers share this process quota.
    /// Worker charges remain held until the native threads are physically joined.
    pub fn spawn_with_quota(
        command: PtyCommand,
        quota: ilium_execution::QuotaGroup,
    ) -> Result<Self, PtyError> {
        Self::spawn_with_optional_quota(command, Some(quota))
    }

    fn spawn_with_optional_quota(
        command: PtyCommand,
        quota: Option<ilium_execution::QuotaGroup>,
    ) -> Result<Self, PtyError> {
        let reservations = PtyWorkerReservations::new(quota.as_ref())
            .map_err(|error| PtyError::Io(error.into()))?;
        // Clamped once, up front, so the OS pty size and the `vt100`
        // parser's initial size can never diverge (see
        // `MINIMUM_PTY_DIMENSION`).
        let rows = clamp_pty_dimension(command.rows);
        let cols = clamp_pty_dimension(command.cols);
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(PtyError::Open)?;

        let mut cmd = CommandBuilder::new(&command.program);
        for arg in &command.args {
            cmd.arg(arg);
        }
        cmd.cwd(&command.cwd);
        // The spawning process's terminal identity is inherited from whatever
        // displays Ilium -- commonly WezTerm or tmux. Left intact, a child
        // mistakes that outer program for its direct terminal and emits
        // private protocols this `vt100` boundary cannot render. Apply the
        // authoritative nested-terminal contract last so neither inheritance
        // nor `PtyCommand::env` can override it.
        // `NO_COLOR` is filtered at the same boundary: a policy inherited by
        // the Ilium process must not make its fully color-capable child PTYs
        // monochrome.
        configure_emulated_terminal_environment(&mut cmd, &command.env);

        let child = pair.slave.spawn_command(cmd).map_err(PtyError::Spawn)?;
        // Drop our end of the slave once the child has it; keeping it open
        // would prevent the child from ever seeing EOF/HUP on its tty.
        drop(pair.slave);

        // From here on the child is already running and nothing else owns
        // it yet -- no `PtySession` exists to `kill()` it on a later `Drop`,
        // and no reaper exists yet. If a later setup step
        // below fails (e.g. `dup`-equivalent fd exhaustion), we must kill
        // *and reap* the child explicitly before returning `Err`, or it
        // leaks as a zombie: dropping portable-pty's child does not prove its
        // process has exited or been reaped.
        let process_id = child.process_id();
        let pty_process_identity = process_id.and_then(|process_id| {
            ilium_platform::process_control::capture_pty_process(process_id)
                .ok()
                .map(Arc::new)
        });
        let child = Arc::new(Mutex::new(child));
        let child_exit = Arc::new(Mutex::new(None));
        let reaper_exit = Arc::clone(&child_exit);
        let reaper_child = Arc::clone(&child);
        let reaper_wake = Arc::new(ReaperWake::default());
        let cancellation_wake = Arc::clone(&reaper_wake);
        let child_reaper = match reservations.child_reaper.spawn(
            "ilium-pty-child-reaper",
            WorkerKind::Cooperative,
            StopToken::default(),
            move || cancellation_wake.wake(),
            move |stop| {
                let mut termination_requested = false;
                let mut poll_interval = CHILD_REAP_POLL_INTERVAL;
                loop {
                    let exited = {
                        let mut child = reaper_child
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        // A status-query failure is not proof that the owned
                        // child was reaped. Keep custody and retry; shutdown may
                        // honestly report a pending reaper if the OS never answers.
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                retain_child_exit(&reaper_exit, process_id, status);
                                true
                            }
                            Ok(None) | Err(_) => {
                                if stop.is_stopped() && !termination_requested {
                                    termination_requested = true;
                                    if let Err(error) = child.kill() {
                                        tracing::warn!(?process_id, %error, "owned PTY child termination failed; reaper custody retained");
                                    }
                                }
                                false
                            }
                        }
                    };
                    if exited {
                        break;
                    }
                    if stop.is_stopped() {
                        poll_interval = CHILD_REAP_POLL_INTERVAL;
                    }
                    reaper_wake.sleep(poll_interval);
                    poll_interval = poll_interval
                        .saturating_mul(2)
                        .min(CHILD_REAP_IDLE_POLL_INTERVAL);
                }
            },
        ) {
            Ok(worker) => worker,
            Err(error) => {
                let mut child = child.lock().unwrap_or_else(|poison| poison.into_inner());
                let _ = child.kill();
                let _ = child.wait();
                return Err(PtyError::Io(error.into()));
            }
        };
        let parser = Arc::new(RwLock::new(vt100::Parser::new_with_callbacks(
            rows,
            cols,
            0,
            TerminalQueryResponder::new(),
        )));
        let screen_generation = Arc::new(AtomicU64::new(0));
        let screen_reader = ScreenReader::new(Arc::clone(&parser), Arc::clone(&screen_generation));
        let (screen_changed_tx, screen_changed_rx) = watch::channel(());
        const OUTPUT_BYTES_CHANNEL_CAPACITY: usize = 256;
        let (output_bytes_tx, _) = broadcast::channel(OUTPUT_BYTES_CHANNEL_CAPACITY);
        let output_journal = Arc::new(Mutex::new(OutputJournal {
            chunks: VecDeque::new(),
            retained_bytes: 0,
            next_sequence: 0,
            is_complete: true,
        }));
        let journal_for_owner = Arc::clone(&output_journal);
        let broadcast_for_owner = output_bytes_tx.clone();
        let terminal = TerminalState {
            screen_reader: screen_reader.clone(),
            parser: Arc::clone(&parser),
            generation: Arc::clone(&screen_generation),
            changed: screen_changed_tx,
            publish: Box::new(move |bytes| {
                let chunk = journal_for_owner
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .append_shared(bytes);
                let _ = broadcast_for_owner.send(chunk);
            }),
        };
        let cleanup_child = Arc::clone(&child);
        let cleanup_exit = Arc::clone(&child_exit);
        let (owner, shell_probe) = PtyOwner::spawn(
            pair.master,
            terminal,
            OwnerLimits::default(),
            reservations.owner,
            move || {
                let mut child = cleanup_child
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                match child.try_wait() {
                    Ok(Some(status)) => retain_child_exit(&cleanup_exit, process_id, status),
                    Ok(None) | Err(_) => {
                        let _ = child.kill();
                        if let Ok(status) = child.wait() {
                            retain_child_exit(&cleanup_exit, process_id, status);
                        }
                    }
                }
            },
        )
        .map_err(|error| PtyError::Io(error.into()))?;

        Ok(Self {
            #[cfg(test)]
            parser,
            screen_generation,
            screen_reader,
            owner,
            identity: PtySessionIdentity::default(),
            shell_probe,
            child,
            _child_reaper: child_reaper,
            child_exit,
            process_id,
            pty_process_identity,
            screen_changed: screen_changed_rx,
            output_bytes: output_bytes_tx,
            output_journal,
        })
    }

    /// Clone under a server registry lock, then release that lock before
    /// waiting for actual delivery. The handle cannot keep the child alive.
    pub fn input_handle(&self) -> PtyInput {
        self.owner.input()
    }

    /// Cheap lifetime identity for comparing an off-lock observation with
    /// the session currently installed in a pane. It does not keep the child
    /// process alive and never authorizes input on its own.
    pub fn identity(&self) -> PtySessionIdentity {
        self.identity.clone()
    }

    /// Clone the read-only native probe while the pane registry is locked;
    /// invoke it only after dropping the registry guard, on a bounded worker.
    pub fn shell_observer(&self) -> PtyShellObserver {
        PtyShellObserver {
            identity: self.identity(),
            probe: self.shell_probe.clone(),
            process_id: self.process_id,
        }
    }

    /// Requests termination on the already-owned child reaper and stops PTY
    /// transport. This performs no native child control or blocking join.
    pub fn request_shutdown(&self) {
        self._child_reaper.ticket().cancel();
        self.owner.request_shutdown();
    }

    /// Requests direct-child termination and owner cancellation, then observes
    /// physical worker joins within one shared deadline. Pending workers retain
    /// the original child control and cannot be mistaken for a completed stop.
    /// Use only for an owned session being closed, outside Tokio and shared
    /// registry locks. Pending handles remain owned by the platform supervisor.
    pub fn shutdown_blocking(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<crate::owner::ShutdownReport, PtyError> {
        let timeout = timeout.min(std::time::Duration::from_secs(60));
        let deadline = std::time::Instant::now() + timeout;
        self.request_shutdown();
        let mut report = self
            .owner
            .shutdown_blocking(deadline.saturating_duration_since(std::time::Instant::now()));
        let ticket = self._child_reaper.ticket();
        match ticket.join_until(deadline) {
            Ok(ilium_platform::owned_worker::WorkerExit::Joined) => report.joined.push(ticket.id()),
            Ok(ilium_platform::owned_worker::WorkerExit::Panicked) => {
                report.panicked.push(ticket.id())
            }
            Err(_) => report.pending.push(ticket.id()),
        }
        Ok(report)
    }

    /// Compatibility path for synchronous callers outside Tokio/registry locks.
    pub fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        self.input_handle().write(bytes)?.wait_blocking()?;
        Ok(())
    }

    pub fn write_mouse_input(
        &self,
        event: MouseEvent,
        column: u16,
        row: u16,
    ) -> Result<(), PtyError> {
        self.input_handle()
            .write_mouse_input(event, column, row)?
            .wait_blocking()?;
        Ok(())
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        self.input_handle().resize(rows, cols)?.wait_blocking()?;
        Ok(())
    }

    /// Reads a consistent screen without waiting for parser mutation. During a
    /// resize it may return the retained pre-resize frame. The callback on a
    /// current frame holds a parser read guard and must not wait for mutation.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        self.screen_reader.with_frame(|screen, _| f(screen))
    }

    /// Clones the existing current-only reader without copying a screen or
    /// starting a worker. Capture the originating session identity alongside
    /// it before releasing a registry lock; deferred consumers must fence that
    /// identity before acting on the result.
    pub fn current_screen_reader(&self) -> ScreenReader {
        self.screen_reader.clone()
    }

    /// Current-only nonblocking read. Automation must not use retained frames.
    pub fn try_with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> Option<R> {
        self.screen_reader.try_with_frame(|screen, _| f(screen))
    }

    pub fn screen_text(&self) -> String {
        self.with_screen(|screen| screen.contents())
    }

    /// Text, cursor, attributes and generation always belong to one frame.
    /// Presentation may receive the retained frame while the parser is busy.
    pub fn screen_snapshot(&self) -> ScreenSnapshot {
        self.screen_reader.with_frame(Self::snapshot_from_screen)
    }

    /// Capture one coherent frame only after a no-allocation retained-byte
    /// preflight. Overflow is explicit; no shortened text is presented as
    /// complete detection evidence. vt100 0.16 cells expose borrowed contents.
    pub fn screen_snapshot_with_limit(&self, limit: usize) -> std::io::Result<ScreenSnapshot> {
        self.screen_reader.with_frame(|screen, generation| {
            let (rows, columns) = screen.size();
            let cells = usize::from(rows).saturating_mul(usize::from(columns));
            // Vec growth may retain twice the populated dim-cell count;
            // String growth may retain twice the visible text bytes.
            let mut bytes = cells
                .saturating_mul(12)
                .saturating_add(usize::from(rows).saturating_mul(2))
                .saturating_add(std::mem::size_of::<ScreenSnapshot>());
            if bytes > limit {
                return Err(std::io::Error::other(
                    "terminal evidence frame exceeds retained-byte limit",
                ));
            }
            for row in 0..rows {
                for column in 0..columns {
                    if let Some(cell) = screen.cell(row, column) {
                        bytes = bytes.saturating_add(cell.contents().len().saturating_mul(2));
                        if bytes > limit {
                            return Err(std::io::Error::other(
                                "terminal evidence text exceeds retained-byte limit",
                            ));
                        }
                    }
                }
            }
            Ok(Self::snapshot_from_screen(screen, generation))
        })
    }

    /// Returns no frame when mutation is underway, instead of stale evidence.
    pub fn try_screen_snapshot(&self) -> Option<ScreenSnapshot> {
        self.screen_reader
            .try_with_frame(Self::snapshot_from_screen)
    }

    fn snapshot_from_screen(screen: &vt100::Screen, generation: u64) -> ScreenSnapshot {
        let (rows, columns) = screen.size();
        let mut dimmed_cells = Vec::new();
        for row in 0..rows {
            for column in 0..columns {
                if screen
                    .cell(row, column)
                    .is_some_and(|cell| cell.has_contents() && cell.dim())
                {
                    dimmed_cells.push((row, column));
                }
            }
        }
        ScreenSnapshot {
            generation,
            text: screen.contents(),
            cursor_position: screen.cursor_position(),
            dimmed_cells,
        }
    }

    /// Returns the current visible-screen generation without cloning its text.
    pub fn screen_generation(&self) -> u64 {
        self.screen_generation.load(Ordering::Acquire)
    }

    /// OS pid of the directly-spawned child, `None` if the platform didn't
    /// report one.
    pub fn process_id(&self) -> Option<u32> {
        self.process_id
    }

    /// Capture child control under a short registry guard. Invoke its native
    /// operations only after releasing that guard, on an owned worker.
    pub fn termination_handle(&self) -> PtyTerminationHandle {
        PtyTerminationHandle {
            child: Arc::clone(&self.child),
            child_exit: Arc::clone(&self.child_exit),
            process_id: self.process_id,
            identity: self.pty_process_identity.clone(),
        }
    }

    /// Terminates the owned PTY lineage for worktree removal and waits up to
    /// `timeout` for observed descendants to be gone. This is separate from
    /// ordinary `kill()`/`Drop`, which retain their direct-child semantics.
    /// A missing birth identity or unavailable platform proof is an error;
    /// callers must also check `processes_using_directory` before removal to
    /// catch a descendant that detached before this method's process scan.
    pub fn terminate_process_tree(
        &mut self,
        timeout: std::time::Duration,
    ) -> std::io::Result<ilium_platform::process_control::PtyTerminationProof> {
        let identity = self.pty_process_identity.as_ref().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "PTY child birth identity was not captured",
            )
        })?;
        ilium_platform::process_control::terminate_pty_process_tree(identity, timeout)
    }

    /// Best-effort cwd of the directly spawned shell or command. Delegates to
    /// `ilium_platform::process_info`, which owns every OS-specific way of
    /// answering this (procfs on Linux, `proc_pidinfo` on macOS, PEB reads on
    /// Windows); a platform or process it cannot read falls back to `None` so
    /// higher layers can fall back to the project root safely.
    pub fn current_working_directory(&self) -> Option<PathBuf> {
        ilium_platform::process_info::working_directory(self.process_id?)
    }

    /// Whether the pane's own shell -- rather than a command it launched --
    /// currently owns the terminal.
    ///
    /// Phrased as the question callers actually ask, not as the mechanism that
    /// answers it, because the two platforms answer it differently and neither
    /// mechanism generalises. Unix compares the terminal's foreground process
    /// group against the shell; ConPTY has no foreground process group at all,
    /// so Windows asks whether the shell has a live child instead.
    ///
    /// `None` means "cannot tell", which is not the same as `Some(false)`:
    /// callers use this to decide whether typed text is a command worth
    /// turning into a pane title, and inferring one without knowing who owns
    /// the terminal would retitle panes from keystrokes typed into a running
    /// program.
    pub fn shell_owns_terminal(&self) -> Option<bool> {
        self.shell_probe.shell_owns_terminal(self.process_id?)
    }

    /// Non-blocking check of whether the child process has exited.
    pub fn has_exited(&mut self) -> bool {
        // Poisoned-lock panic is an invariant violation (see `spawn`).
        // `try_wait` returning `Err` means we couldn't determine status;
        // treat that as "not (known to be) exited" rather than guessing.
        self.child_exit().is_some()
    }

    /// Returns the retained direct-child exit cause, refreshing it with a
    /// non-blocking wait query when necessary. Query errors never invent an exit.
    pub fn child_exit(&self) -> Option<PtyChildExit> {
        {
            let retained = self
                .child_exit
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if retained.is_some() {
                return retained.clone();
            }
        }
        // Cleanup may be waiting to reap the child. Status inspection must
        // not wait behind it while the caller owns a server registry lock.
        let mut child = match self.child.try_lock() {
            Ok(child) => child,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        if let Ok(Some(status)) = child.try_wait() {
            retain_child_exit(&self.child_exit, self.process_id, status);
        }
        self.child_exit
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// Returns a fresh `watch::Receiver` that resolves on `.changed().await`
    /// every time the reader thread parses a new chunk of pty output. The
    /// channel carries no payload -- callers re-read the current state via
    /// `with_screen`/`screen_text` after waking, exactly as synchronous
    /// callers already do on their own schedule. See the module docs for
    /// why a payload-less signal was chosen over cloning screen snapshots
    /// through the channel.
    pub fn subscribe_screen_changed(&self) -> watch::Receiver<()> {
        self.screen_changed.clone()
    }

    /// Returns a fresh `broadcast::Receiver` that yields every ordered raw byte
    /// chunk the reader thread reads from the pty, from the moment of this
    /// call onward (chunks read before subscribing are not replayed). If
    /// the subscriber falls far enough behind that the channel's internal
    /// buffer overwrites unread chunks, the next `.recv().await` resolves
    /// to `Err(broadcast::error::RecvError::Lagged(n))` rather than
    /// silently skipping bytes -- callers must treat that as "this pane's
    /// downstream view is now out of sync" (e.g. `ilium-server` should log
    /// it) rather than ignoring it, since unlike `subscribe_screen_changed`
    /// this channel's payload is not re-derivable from current state alone.
    pub fn subscribe_output_bytes(&self) -> broadcast::Receiver<PtyOutputChunk> {
        self.output_bytes.subscribe()
    }

    /// Clones the bounded output history accumulated so far. The journal
    /// snapshot and the output sequence are captured under one mutex, so a
    /// caller can replay it and safely ignore live chunks at or below
    /// `through_sequence`.
    pub fn output_replay(&self) -> PtyOutputReplay {
        self.output_journal.lock().unwrap().replay()
    }

    /// Reads replay size and sequence without copying retained output bytes.
    /// Reserve against `byte_len` before requesting the corresponding replay.
    pub fn output_replay_estimate(&self) -> PtyOutputReplayEstimate {
        self.output_journal.lock().unwrap().replay_estimate()
    }

    /// Copies replay bytes only while the journal still matches a prior
    /// estimate. `None` means new PTY output arrived; release the old
    /// reservation and estimate again before retrying.
    pub fn output_replay_if_unchanged(
        &self,
        expected: PtyOutputReplayEstimate,
    ) -> Option<PtyOutputReplay> {
        self.output_journal
            .lock()
            .unwrap()
            .replay_if_unchanged(expected)
    }

    /// Copies the estimated replay prefix if its bytes remain retained. New
    /// output after the estimate does not force a retry or delay presentation.
    pub fn output_replay_through_if_retained(
        &self,
        expected: PtyOutputReplayEstimate,
    ) -> Option<PtyOutputReplay> {
        self.output_journal
            .lock()
            .unwrap()
            .replay_through_if_retained(expected)
    }

    /// Estimates a lag-recovery delta or full replay without copying bytes.
    pub fn output_recovery_estimate_after(
        &self,
        after_sequence: u64,
    ) -> Option<PtyOutputRecoveryEstimate> {
        self.output_journal
            .lock()
            .unwrap()
            .recovery_estimate_after(after_sequence)
    }

    /// Copies recovery bytes only if they still match a previous estimate.
    pub fn output_recovery_if_unchanged(
        &self,
        after_sequence: u64,
        expected: PtyOutputRecoveryEstimate,
    ) -> Option<PtyOutputRecovery> {
        self.output_journal
            .lock()
            .unwrap()
            .recovery_if_unchanged(after_sequence, expected)
    }

    /// Returns the smallest safe repair for a downstream parser known to
    /// contain every journal chunk through `after_sequence`.
    pub fn output_recovery_after(&self, after_sequence: u64) -> Option<PtyOutputRecovery> {
        self.output_journal
            .lock()
            .unwrap()
            .recovery_after(after_sequence)
    }

    /// Returns one bounded recovery frame from the current journal. The
    /// caller may request the next prefix from its sequence watermark.
    pub fn output_recovery_prefix_after(
        &self,
        after_sequence: u64,
        max_bytes: usize,
    ) -> Option<PtyOutputRecovery> {
        self.output_journal
            .lock()
            .unwrap()
            .recovery_prefix_after(after_sequence, max_bytes)
    }

    /// Returns one bounded recovery frame no later than a captured sequence
    /// watermark, allowing callers to yield fairly across several panes.
    pub fn output_recovery_prefix_through(
        &self,
        after_sequence: u64,
        through_sequence: u64,
        max_bytes: usize,
    ) -> Option<PtyOutputRecovery> {
        self.output_journal.lock().unwrap().recovery_prefix_through(
            after_sequence,
            through_sequence,
            max_bytes,
        )
    }

    /// Terminates the spawned child process. A no-op returning `Ok(())` if
    /// the child has already exited -- there is nothing left to kill, and
    /// treating that as an error would make normal pane teardown (the
    /// child often exits on its own right before the caller gets around to
    /// closing the pane) look like a failure.
    pub fn kill(&mut self) -> Result<(), PtyError> {
        // Poisoned-lock panic is an invariant violation (see `spawn`). The
        // exited-check and the kill must happen under the same lock
        // acquisition: `has_exited` followed by a separate `child.lock()`
        // left a window where the reader thread's own `wait()` (see
        // `spawn`) could reap the child in between, turning an already-gone
        // process into a spurious `PtyError::Kill` instead of the `Ok(())`
        // this method promises for that case.
        let mut child = self.child.lock().unwrap();
        if let Ok(Some(status)) = child.try_wait() {
            retain_child_exit(&self.child_exit, self.process_id, status);
            return Ok(());
        }
        child.kill().map_err(PtyError::Kill)
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // The existing reaper retains the original Child mutex through actual
        // exit. A caller holding registry locks must only signal cancellation;
        // a stalled native child control remains owned off the Tokio executor.
        self.request_shutdown();
    }
}

#[cfg(test)]
#[path = "owner_regression_tests.rs"]
mod owner_regression_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn admitted_pty_workers_remain_charged_until_native_shutdown_joins() {
        let worker_count = 5;
        let quota = ilium_execution::QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: worker_count,
            worker_bytes: worker_count * crate::PTY_WORKER_STACK_BYTES,
        });
        let mut session = PtySession::spawn_with_quota(
            PtyCommand::new("/bin/sh", std::env::temp_dir(), 24, 80)
                .arg("-c")
                .arg("exec cat"),
            quota.clone(),
        )
        .expect("start quota-admitted PTY");

        assert_eq!(quota.snapshot().worker_threads, worker_count);
        let report = session
            .shutdown_blocking(std::time::Duration::from_secs(3))
            .expect("bounded shutdown");
        assert!(report.pending.is_empty(), "workers remain: {report:?}");
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[cfg(unix)]
    #[test]
    fn pty_worker_overload_refuses_before_starting_the_child() {
        let worker_count = 5;
        let quota = ilium_execution::QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: worker_count - 1,
            worker_bytes: worker_count * crate::PTY_WORKER_STACK_BYTES,
        });
        let marker = std::env::temp_dir().join(format!(
            "ilium-pty-admission-{}-{}.marker",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos()
        ));

        let result = PtySession::spawn_with_quota(
            PtyCommand::new("/usr/bin/touch", std::env::temp_dir(), 24, 80)
                .arg(marker.to_string_lossy().into_owned()),
            quota.clone(),
        );

        assert!(result.is_err(), "undersized worker quota must refuse PTY");
        assert!(
            !marker.exists(),
            "refusal must precede child process launch"
        );
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[cfg(unix)]
    #[test]
    fn bounded_screen_snapshot_rejects_before_clone_and_keeps_complete_frame() {
        let mut session = PtySession::spawn(
            PtyCommand::new("/bin/sh", std::env::temp_dir(), 24, 80)
                .arg("-c")
                .arg("exec cat"),
        )
        .expect("isolated PTY");
        assert!(session.screen_snapshot_with_limit(1).is_err());
        let frame = session
            .screen_snapshot_with_limit(2 * 1024 * 1024)
            .expect("complete frame");
        assert_eq!(frame.generation, session.screen_generation());
        assert_eq!(frame.text, session.screen_snapshot().text);
        session.kill().expect("fixture cleanup");
    }

    #[test]
    fn child_exit_receipt_preserves_unsigned_codes_and_the_first_observation() {
        let receipt = Mutex::new(None);
        retain_child_exit(&receipt, Some(42), ExitStatus::with_exit_code(u32::MAX));
        retain_child_exit(&receipt, Some(42), ExitStatus::with_exit_code(0));
        assert_eq!(
            receipt.into_inner().unwrap(),
            Some(PtyChildExit {
                process_id: Some(42),
                cause: PtyExitCause::ExitCode(u32::MAX),
            }),
        );
    }

    #[test]
    fn child_exit_receipt_preserves_a_signal_without_its_placeholder_code() {
        let receipt = Mutex::new(None);
        retain_child_exit(&receipt, None, ExitStatus::with_signal("SIGTERM"));
        assert_eq!(
            receipt.into_inner().unwrap(),
            Some(PtyChildExit {
                process_id: None,
                cause: PtyExitCause::Signal("SIGTERM".to_string()),
            }),
        );
    }

    #[cfg(unix)]
    #[test]
    fn child_exit_receipt_survives_real_reaping_and_idempotent_kill() {
        for code in [0, 37] {
            let mut session = PtySession::spawn(
                PtyCommand::new("sh", std::env::temp_dir(), 24, 80)
                    .arg("-c")
                    .arg(format!("exit {code}")),
            )
            .expect("spawn owned fixture shell");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !session.has_exited() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let expected = Some(PtyChildExit {
                process_id: session.process_id(),
                cause: PtyExitCause::ExitCode(code),
            });
            assert_eq!(session.child_exit(), expected);
            session.kill().expect("already reaped child is harmless");
            assert_eq!(session.child_exit(), expected);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn worktree_termination_reaches_pty_descendant() {
        let mut session = PtySession::spawn(
            PtyCommand::new("/bin/sh", std::env::temp_dir(), 24, 80)
                .arg("-c")
                .arg("sleep 60 & echo child-started; wait"),
        )
        .expect("spawn PTY shell");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !session.screen_text().contains("child-started")
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(session.screen_text().contains("child-started"));
        let proof = session
            .terminate_process_tree(std::time::Duration::from_secs(3))
            .expect("terminate PTY lineage");
        assert!(
            proof.signalled_processes >= 2,
            "shell and background child must both be signalled"
        );
    }

    fn journal() -> OutputJournal {
        OutputJournal {
            chunks: VecDeque::new(),
            retained_bytes: 0,
            next_sequence: 0,
            is_complete: true,
        }
    }

    #[test]
    #[ignore = "manual performance benchmark"]
    fn benchmark_shared_output_chunk_clone() {
        const CLONES: usize = 100_000;
        let owned_bytes = vec![b'x'; 8192];
        let owned_started_at = std::time::Instant::now();
        for _ in 0..CLONES {
            std::hint::black_box(owned_bytes.clone());
        }
        let owned_elapsed = owned_started_at.elapsed();

        let shared_bytes: Arc<[u8]> = Arc::from(owned_bytes);
        let shared_started_at = std::time::Instant::now();
        for _ in 0..CLONES {
            std::hint::black_box(Arc::clone(&shared_bytes));
        }
        let shared_elapsed = shared_started_at.elapsed();

        println!(
            "PERF pty.output_chunk_clone owned_ns={} shared_ns={} clones={CLONES}",
            owned_elapsed.as_nanos(),
            shared_elapsed.as_nanos(),
        );
    }

    #[test]
    fn output_recovery_returns_only_the_contiguous_missing_tail() {
        let mut journal = journal();
        journal.append(b"first");
        journal.append(b"-second");
        journal.append(b"-third");

        assert_eq!(
            journal.recovery_after(1),
            Some(PtyOutputRecovery::Delta(PtyOutputChunk {
                sequence: 3,
                bytes: Arc::from(b"-second-third".as_slice()),
            }))
        );
        assert_eq!(journal.recovery_after(3), None);
    }

    #[test]
    fn replay_estimate_fences_copy_when_new_output_arrives() {
        let mut journal = journal();
        journal.append(b"first");
        let estimate = journal.replay_estimate();
        assert_eq!(estimate.through_sequence, 1);
        assert_eq!(estimate.byte_len, b"first".len());
        assert_eq!(
            journal.replay_if_unchanged(estimate).unwrap().bytes,
            b"first"
        );

        journal.append(b"-later");
        assert_ne!(journal.replay_estimate(), estimate);
        assert!(journal.replay_if_unchanged(estimate).is_none());
        assert_eq!(
            journal.replay_through_if_retained(estimate).unwrap().bytes,
            b"first"
        );

        let current = journal.replay_estimate();
        assert_eq!(current.through_sequence, 2);
        assert_eq!(current.byte_len, b"first-later".len());
        let replay = journal.replay_if_unchanged(current).unwrap();
        assert_eq!(replay.through_sequence, current.through_sequence);
        assert_eq!(replay.bytes.len(), current.byte_len);
        assert_eq!(replay.bytes, b"first-later");
    }

    #[test]
    fn replay_prefix_is_bounded_at_sequence_boundaries_and_keeps_reset() {
        let mut journal = journal();
        journal.append(b"first");
        journal.append(b"-second");

        assert!(journal.replay_prefix(4).is_none());
        let prefix = journal.replay_prefix(5).unwrap();
        assert_eq!(prefix.through_sequence, 1);
        assert_eq!(prefix.bytes, b"first");
        assert!(prefix.is_complete);

        journal.is_complete = false;
        assert!(journal.replay_prefix(7).is_none());
        let truncated_prefix = journal.replay_prefix(8).unwrap();
        assert_eq!(truncated_prefix.through_sequence, 1);
        assert_eq!(truncated_prefix.bytes, b"\x1bcfirst");
        assert!(!truncated_prefix.is_complete);
    }

    #[test]
    fn recovery_prefix_after_bounds_replay_and_contiguous_delta() {
        let mut journal = journal();
        journal.append(b"first");
        journal.append(b"-second");

        let PtyOutputRecovery::Replay(replay) = journal.recovery_prefix_after(0, 5).unwrap() else {
            panic!("a consumer behind the journal must receive a replay prefix");
        };
        assert_eq!(replay.through_sequence, 1);
        assert_eq!(replay.bytes, b"first");

        let PtyOutputRecovery::Delta(delta) = journal.recovery_prefix_after(1, 7).unwrap() else {
            panic!("a current consumer must receive a contiguous delta");
        };
        assert_eq!(delta.sequence, 2);
        assert_eq!(delta.bytes.as_ref(), b"-second");
        assert!(journal.recovery_prefix_after(1, 6).is_none());

        let PtyOutputRecovery::Delta(watermarked) =
            journal.recovery_prefix_through(0, 1, 64).unwrap()
        else {
            panic!("a prefix must not pass its captured sequence watermark");
        };
        assert_eq!(watermarked.sequence, 1);
        assert_eq!(watermarked.bytes.as_ref(), b"first");
    }

    #[test]
    fn truncated_replay_prefix_continues_with_contiguous_delta() {
        let mut journal = journal();
        journal.append(b"first");
        journal.append(b"-second");
        journal.is_complete = false;

        let PtyOutputRecovery::Replay(replay) = journal.recovery_prefix_after(0, 8).unwrap() else {
            panic!("a consumer behind a truncated journal must receive a replay prefix");
        };
        assert_eq!(replay.through_sequence, 1);
        assert_eq!(replay.bytes, b"\x1bcfirst");
        assert!(!replay.is_complete);

        let PtyOutputRecovery::Delta(delta) = journal
            .recovery_prefix_after(replay.through_sequence, 8)
            .unwrap()
        else {
            panic!("after applying the reset-bearing prefix, the next frame must be a delta");
        };
        assert_eq!(delta.sequence, 2);
        assert_eq!(delta.bytes.as_ref(), b"-second");
    }

    #[test]
    fn recovery_estimate_copies_only_the_reserved_delta_prefix() {
        let mut journal = journal();
        journal.append(b"first");
        let estimate = journal.recovery_estimate_after(0).unwrap();
        assert_eq!(
            estimate,
            PtyOutputRecoveryEstimate::Delta {
                through_sequence: 1,
                byte_len: 5,
            }
        );

        journal.append(b"-later");
        assert_eq!(
            journal.recovery_if_unchanged(0, estimate),
            Some(PtyOutputRecovery::Delta(PtyOutputChunk {
                sequence: 1,
                bytes: Arc::from(b"first".as_slice()),
            }))
        );
    }

    #[test]
    fn recovery_estimate_refuses_a_delta_evicted_before_copy() {
        let mut journal = journal();
        journal.append(b"first");
        let estimate = journal.recovery_estimate_after(0).unwrap();
        let removed = journal.chunks.pop_front().unwrap();
        journal.retained_bytes -= removed.bytes.len();
        assert!(journal.recovery_if_unchanged(0, estimate).is_none());
    }

    #[test]
    fn output_recovery_falls_back_to_replay_after_retained_history_was_lost() {
        let mut journal = journal();
        journal.append(b"discarded");
        journal.append(b"retained");
        let removed = journal.chunks.pop_front().unwrap();
        journal.retained_bytes = journal.retained_bytes.saturating_sub(removed.bytes.len());
        journal.is_complete = false;

        let Some(PtyOutputRecovery::Replay(replay)) = journal.recovery_after(0) else {
            panic!("a missing retained chunk requires full replay");
        };
        assert_eq!(replay.through_sequence, 2);
        assert!(!replay.is_complete);
        assert_eq!(replay.bytes, b"\x1bcretained");
    }
}
