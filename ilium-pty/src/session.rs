//! `PtySession`: spawn a command behind a pty, get a handle to write input,
//! resize, and read screen state. This is the entire contract this crate
//! exposes -- no tree, no agent detection, nothing that knows what the
//! spawned command *is*, only that it's a process behind a pty.
//!
//! The live pty output is consumed by a background reader thread that owns
//! the *only* long-lived write access to the shared `vt100::Parser`; other
//! code only ever takes a read lock (`with_screen`) except when resizing.
//! The parser is wrapped in `Arc<RwLock<_>>` so both sides can reach it
//! without the reader thread blocking a caller's read for longer than a
//! single `process()` call.
//!
//! `portable_pty`'s I/O is blocking (`std::io::Read`/`Write`, not tokio),
//! so the reader stays a dedicated `std::thread` rather than an async task.
//! To let async callers (the detection loop, later, in `ilium-server`)
//! observe screen changes without polling, the reader thread also notifies
//! a `tokio::sync::watch::channel(())` after every chunk it parses. The
//! channel carries no payload -- it is purely a "something changed, go
//! re-read the shared parser via `with_screen`/`screen_text`" signal, not a
//! snapshot of the screen itself. That keeps the (fairly large) `vt100`
//! screen state single-sourced in the `Arc<RwLock<_>>` instead of cloning
//! it through a channel on every byte chunk, and it matches exactly how
//! synchronous callers already read the screen: on demand, not by being
//! handed a copy.
//!
//! The reader thread separately broadcasts the *raw* bytes it read (before
//! `vt100` parsing) over a `tokio::sync::broadcast::channel`. This is for
//! `ilium-server`'s IPC layer, which forwards `ScreenUpdate` frames to
//! attached clients as raw bytes so each client can drive its own
//! `vt100::Parser` for rendering (see `ilium-ipc::ServerEvent::ScreenUpdate`
//! doc comment for why raw bytes were chosen over a server-computed diff).
//! `broadcast` rather than another `watch` because this payload is a byte
//! chunk, not a "something changed" pulse -- every chunk matters and none
//! may be skipped, and `broadcast` (unlike `watch`) supports that plus
//! multiple independent subscribers (`ilium-server` may run more than one
//! forwarder per pane across reconnects).
//!
//! The reader thread's own cancellation path is `PtySession::drop`, via
//! `reader_should_stop` and the `CancellableReader` trait -- see their doc
//! comments. That exists specifically so a killed pane's thread (and the
//! `Arc` clones of the parser/journal/child it holds) doesn't stay alive
//! for the rest of the server process just because some descendant of the
//! spawned child inherited the pty's slave fd and is still holding it open.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crossterm::event::MouseEvent;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd};
use tokio::sync::{broadcast, watch};

use crate::error::PtyError;
use crate::mouse::encode_mouse_event;
use crate::query::TerminalQueryResponder;

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

/// One spawned command behind a pty, plus the `vt100` parser that turns its
/// raw byte stream into a renderable/queryable screen.
pub struct PtySession {
    // Shared with the background reader thread; `with_screen`/`resize` take
    // a read/write lock respectively (see module docs for the locking
    // discipline).
    parser: Arc<RwLock<vt100::Parser<TerminalQueryResponder>>>,
    /// Monotonic revision of the visible parser grid. The reader increments it
    /// while holding the parser write lock, so [`Self::screen_snapshot`] can
    /// return text and a revision that describe exactly the same frame.
    screen_generation: Arc<AtomicU64>,
    // The pty master's write half; writing here sends bytes to the child's
    // stdin (as seen through the pty). Shared with the reader thread's
    // `TerminalQueryResponder`, which writes terminal capability-query
    // replies back down the same channel.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    // The pty master's control handle; used for resizing. Wrapped in a
    // `Mutex` (rather than a bare field) because `portable_pty::PtyPair`
    // hands this back as `Box<dyn MasterPty + Send>` -- not `+ Sync` -- so
    // without this wrapper `PtySession` itself would not be `Sync`, which
    // `ilium-server` needs (it shares pane state across concurrently
    // running tokio tasks via `Arc<ServerState>`). `MasterPty::resize`
    // only takes `&self`, so this mutex is purely a marker/synchronizer
    // for concurrent callers, not protecting any actual interior state
    // this crate owns.
    master: Mutex<Box<dyn MasterPty + Send>>,
    // The spawned child process handle; used for exit-status polling.
    // Wrapped in `Arc<Mutex<_>>` (like `writer` above) so the background
    // reader thread can share it: `portable_pty`'s `Child::kill` (used by
    // `kill`/`Drop` below) sends SIGHUP and only escalates to an
    // un-reaped SIGKILL if the child ignores that for ~250ms, so relying
    // solely on some *future* caller of `has_exited`/`kill` to collect the
    // exit status would leave a stubborn child as a zombie for the rest
    // of this (potentially long-lived) server process's life. The reader
    // thread reaps it itself once its read loop ends (see `spawn`).
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    // OS pid of the directly-spawned child, if the platform reported one.
    process_id: Option<u32>,
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
    /// Cancellation flag for the background reader thread, set by `Drop`.
    /// The thread checks this between *and during* its waits for pty
    /// output -- on unix via a bounded `poll()` (`PollableMasterReader`),
    /// and on every other target via a bounded channel receive against a
    /// decoupled pump thread (`BlockingMasterReader`) -- so it can exit,
    /// and release its `Arc` clones of `parser`/`output_journal`/`child`
    /// above, even when a descendant of the spawned child is still holding
    /// the pty's slave fd open and would otherwise keep a plain blocking
    /// `read()` stuck forever. See `CancellableReader`.
    reader_should_stop: Arc<AtomicBool>,
}

/// One ordered piece of raw output from a PTY. The sequence belongs to the
/// pane's lifetime, letting clients discard a live broadcast that was already
/// included in their attach-time replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOutputChunk {
    pub sequence: u64,
    pub bytes: Vec<u8>,
}

/// One internally-consistent visible-screen read. Detection carries the
/// generation through its lock-free classification phase so it can promptly
/// revisit a pane when newer PTY output or a resize arrives before apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSnapshot {
    pub generation: u64,
    pub text: String,
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

    fn append(&mut self, bytes: Vec<u8>) -> PtyOutputChunk {
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
            bytes,
        }))
    }
}

/// Outcome of one attempt to fetch the next chunk of pty output, reported
/// by a [`CancellableReader`] to the background reader thread's loop in
/// [`PtySession::spawn`].
enum ReadOutcome {
    /// `usize` bytes were read into the caller's buffer.
    Data(usize),
    /// The pty's slave side is fully closed; nothing more will ever arrive.
    Eof,
    /// The owning `PtySession` asked this thread to stop (see
    /// `reader_should_stop`) before any more data arrived.
    Stopped,
    /// An unrecoverable I/O error; treated the same as `Eof` by the caller.
    Error,
}

/// Reads the next chunk of a pty's output while being able to notice a
/// cancellation request even when no output is currently available. A plain
/// blocking `Read::read` cannot do this: killing the directly-spawned child
/// (always the pty's session leader and controlling-terminal owner --
/// `portable_pty` calls `setsid()`/`TIOCSCTTY` for it) makes Linux hang up
/// the pty for every remaining fd still referencing it, which is enough to
/// unblock a plain `read()` for most descendants. It is *not* enough for one
/// that called `setsid()` itself and re-parented into its own session while
/// still holding the inherited, unredirected slave fd (verified empirically
/// -- see the crate's test suite): that descendant is no longer a member of
/// the session the hangup tears down, so it can keep the fd open
/// indefinitely, and a `read()` blocked on it has no way to be woken by
/// anything other than bytes actually arriving. See the `unix` impl below
/// for how this is solved with a bounded `poll()` wait instead of an
/// unbounded `read()`.
trait CancellableReader: Send {
    fn read_next(&mut self, buf: &mut [u8], should_stop: &AtomicBool) -> ReadOutcome;
}

/// Unix `CancellableReader`: an owned, `CLOEXEC` duplicate of the pty
/// master's fd, polled with a short timeout so the reader thread's loop
/// wakes up regularly to re-check `should_stop` instead of only ever waking
/// on incoming bytes.
#[cfg(unix)]
struct PollableMasterReader {
    file: std::fs::File,
}

#[cfg(unix)]
impl PollableMasterReader {
    /// How long a single `poll()` call waits before returning control to
    /// the caller to re-check `should_stop`. Short enough that cancellation
    /// (pane close) is never noticeably delayed; long enough to avoid
    /// spinning the thread on an idle pane.
    const POLL_TIMEOUT_MILLIS: libc::c_int = 200;

    /// Duplicates `master`'s raw fd (via `F_DUPFD_CLOEXEC`, matching
    /// `portable_pty`'s own `cloexec()` treatment of the master fd it
    /// hands out elsewhere) into a `std::fs::File` this struct owns
    /// outright, so it can be polled independently of `portable_pty`'s own
    /// `try_clone_reader`, which only ever exposes a plain blocking
    /// `Box<dyn Read + Send>`.
    fn duplicate_from(master: &(dyn MasterPty + Send)) -> Result<Self, PtyError> {
        let master_fd = master
            .as_raw_fd()
            .ok_or_else(|| PtyError::Io(anyhow::anyhow!("pty master exposed no raw fd")))?;
        // SAFETY: `master_fd` is a valid, currently-open fd owned by
        // `master` for the duration of this call; `fcntl(F_DUPFD_CLOEXEC)`
        // only reads it to create an independent duplicate, it does not
        // consume or close it.
        let duplicated_fd = unsafe { libc::fcntl(master_fd, libc::F_DUPFD_CLOEXEC, 0) };
        if duplicated_fd < 0 {
            return Err(PtyError::Io(anyhow::Error::from(
                std::io::Error::last_os_error(),
            )));
        }
        // SAFETY: `duplicated_fd` was just returned by a successful
        // `fcntl(F_DUPFD_CLOEXEC)` above, so it is a fresh, valid,
        // exclusively-owned fd -- nothing else holds or will close it.
        let file = unsafe { std::fs::File::from_raw_fd(duplicated_fd) };
        Ok(Self { file })
    }
}

#[cfg(unix)]
impl CancellableReader for PollableMasterReader {
    fn read_next(&mut self, buf: &mut [u8], should_stop: &AtomicBool) -> ReadOutcome {
        loop {
            if should_stop.load(Ordering::Acquire) {
                return ReadOutcome::Stopped;
            }
            let mut poll_fd = libc::pollfd {
                fd: self.file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: `poll_fd` is a single, valid, stack-local `pollfd`
            // entry and `1` matches its own length; `poll` only reads/
            // writes through the pointer for the duration of this call.
            let poll_result = unsafe { libc::poll(&mut poll_fd, 1, Self::POLL_TIMEOUT_MILLIS) };
            if poll_result < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return ReadOutcome::Error;
            }
            if poll_result == 0 {
                // Timed out with nothing readable -- loop back around to
                // re-check `should_stop` rather than polling forever.
                continue;
            }
            // `revents` may report `POLLHUP`/`POLLERR` instead of (or
            // alongside) `POLLIN` once the slave side closes; reading
            // either yields the real answer (`Ok(0)`/`EIO` below) without
            // needing to interpret the bitmask ourselves.
            match self.file.read(buf) {
                Ok(0) => return ReadOutcome::Eof,
                Ok(bytes_read) => return ReadOutcome::Data(bytes_read),
                // `portable_pty`'s own reader translates EIO (raised once
                // the slave is fully closed) to a clean EOF -- see
                // `PtyFd::read` in `portable-pty`'s `unix.rs`. Match that
                // behavior here since this bypasses that wrapper.
                Err(error) if error.raw_os_error() == Some(libc::EIO) => return ReadOutcome::Eof,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return ReadOutcome::Error,
            }
        }
    }
}

/// One outcome of a single `read()` call made by the dedicated pump thread
/// `BlockingMasterReader` spawns (see its doc comment) -- forwarded to the
/// owning `CancellableReader::read_next` over a bounded channel.
#[cfg(not(unix))]
enum MasterReadMessage {
    /// Bytes read from the pty master. Owned (`Vec<u8>`, not a borrow) since
    /// it must cross the channel to a different thread.
    Data(Vec<u8>),
    /// The pty's slave side is fully closed; nothing more will ever arrive.
    Eof,
    /// An unrecoverable I/O error occurred on the underlying `read()`.
    Error,
}

/// Non-unix `CancellableReader`. There is no portable equivalent of
/// `poll()` available here, so a single thread cannot both block on
/// `Read::read` and re-check `should_stop` on a bounded interval the way
/// `PollableMasterReader` does on unix. Splitting the work across two
/// threads recovers that invariant for the state that actually matters:
///
/// - A dedicated **pump thread**, spawned once by [`Self::spawn_pumping_from`]
///   and owning nothing but the raw `Box<dyn Read + Send>` handle and a
///   bounded channel's sender half, does the actual blocking `read()` calls
///   in an unbroken loop and forwards each outcome down the channel. It
///   holds no `Arc` clone of `parser`/`output_journal`/`child` -- those
///   never reach this thread -- so if a descendant of the killed child is
///   still holding the pty's slave fd open (see the module docs' worked
///   example) and this thread's `read()` blocks forever, the *only* things
///   it leaks for the rest of the process are itself (one OS thread) and
///   the raw reader handle. That is a small, bounded, non-growing cost --
///   nothing like the per-pane `vt100` screen grid, the up-to-32-MiB output
///   journal, or the child handle the old single-thread design also kept
///   captive.
/// - `read_next` itself runs on the existing background reader thread (see
///   `PtySession::spawn`), which is the one holding those `Arc`s. It never
///   calls `read()`; it only ever waits on the channel with a bounded
///   timeout, exactly mirroring the unix `poll()` loop's shape, so it can
///   still notice `should_stop` and return `Stopped` -- releasing its
///   `Arc` clones -- within about one timeout interval regardless of
///   whether the pump thread's `read()` ever returns.
///
/// This does not make the pump thread's own blocked `read()` cancellable --
/// only a platform-native mechanism (e.g. an `OVERLAPPED` read against a
/// ConPTY named pipe, or `WaitForMultipleObjects` against a cancellation
/// event) can do that, and `ilium` has no Windows users today to justify
/// the new platform-specific dependency and unsafe FFI that would require.
/// Revisit with that primitive if Windows support is actually planned; the
/// invariant to preserve is the one this split already restores for every
/// other piece of per-pane state.
#[cfg(not(unix))]
struct BlockingMasterReader {
    read_messages: std::sync::mpsc::Receiver<MasterReadMessage>,
    /// Bytes already pulled off `read_messages` but not yet handed to a
    /// caller, when the pump thread's chunk was larger than the caller's
    /// `buf`. Keeps `read_next` correct for any `buf` length rather than
    /// assuming it always matches the pump thread's own internal read size.
    pending_bytes: Vec<u8>,
}

#[cfg(not(unix))]
impl BlockingMasterReader {
    /// How long a single channel receive waits before returning control to
    /// the caller to re-check `should_stop`. Mirrors
    /// `PollableMasterReader::POLL_TIMEOUT_MILLIS` on unix: short enough
    /// that cancellation (pane close) is never noticeably delayed, long
    /// enough to avoid spinning the thread on an idle pane.
    const RECV_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);

    /// Bound on in-flight, not-yet-consumed read messages. Without a bound
    /// the pump thread could read arbitrarily far ahead of a slow consumer
    /// (the owning reader thread spends real time inside `parser.process()`
    /// and the journal mutex between receives), turning the channel itself
    /// into the same kind of unbounded per-pane growth this fix removes
    /// elsewhere. A small bound is enough to keep the pump thread from
    /// idling on backpressure during ordinary bursts; once full, its
    /// blocking `send` simply waits for the consumer, which is exactly the
    /// backpressure a single-threaded blocking `read()` used to provide for
    /// free.
    const CHANNEL_CAPACITY: usize = 16;

    /// Spawns the pump thread described in this type's doc comment and
    /// returns the `CancellableReader` side that receives from it.
    ///
    /// The pump thread's `JoinHandle` is deliberately not kept: it may
    /// block on `read()` forever (see the doc comment above), so joining it
    /// from anywhere -- including `PtySession::drop` -- could block the
    /// joiner for the rest of the process's life. Its cancellation path is
    /// structural, not a stop flag: once `read_next`'s caller (the
    /// background reader thread) observes `should_stop` and returns, it
    /// drops this `BlockingMasterReader`, which drops `read_messages`: the
    /// pump thread's next `send` then fails immediately (a `sync_channel`
    /// send errors as soon as its receiver is dropped, even mid-block on a
    /// full queue) and the pump thread exits on its own without ever
    /// needing to complete another `read()`. Only a pump thread already
    /// blocked *inside* `read()` at that moment can't observe this --
    /// exactly the residual case this type's doc comment names.
    fn spawn_pumping_from(mut reader: Box<dyn Read + Send>) -> Self {
        let (message_sender, read_messages) = std::sync::mpsc::sync_channel(Self::CHANNEL_CAPACITY);
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                let message = match reader.read(&mut buf) {
                    Ok(0) => MasterReadMessage::Eof,
                    Ok(bytes_read) => MasterReadMessage::Data(buf[..bytes_read].to_vec()),
                    Err(_) => MasterReadMessage::Error,
                };
                // `Eof`/`Error` are terminal for the underlying handle --
                // nothing meaningful can be read from it again -- so there
                // is nothing left to pump either way once one is sent.
                let is_terminal = !matches!(message, MasterReadMessage::Data(_));
                if message_sender.send(message).is_err() || is_terminal {
                    break;
                }
            }
        });
        Self {
            read_messages,
            pending_bytes: Vec::new(),
        }
    }
}

#[cfg(not(unix))]
impl CancellableReader for BlockingMasterReader {
    fn read_next(&mut self, buf: &mut [u8], should_stop: &AtomicBool) -> ReadOutcome {
        loop {
            if !self.pending_bytes.is_empty() {
                let length = self.pending_bytes.len().min(buf.len());
                buf[..length].copy_from_slice(&self.pending_bytes[..length]);
                self.pending_bytes.drain(..length);
                return ReadOutcome::Data(length);
            }
            if should_stop.load(Ordering::Acquire) {
                return ReadOutcome::Stopped;
            }
            match self.read_messages.recv_timeout(Self::RECV_TIMEOUT) {
                Ok(MasterReadMessage::Data(bytes)) => {
                    self.pending_bytes = bytes;
                    continue;
                }
                Ok(MasterReadMessage::Eof) => return ReadOutcome::Eof,
                Ok(MasterReadMessage::Error) => return ReadOutcome::Error,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                // The pump thread only ever exits after sending a terminal
                // message (see `spawn_pumping_from`), so an unexpected
                // disconnect (e.g. it panicked) has no more information to
                // offer than a clean `Eof` does.
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return ReadOutcome::Eof,
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

impl PtySession {
    /// Spawns `command` behind a new pty and starts the background reader
    /// thread that feeds its output into the shared `vt100::Parser`.
    pub fn spawn(command: PtyCommand) -> Result<Self, PtyError> {
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
        // Codex itself is commonly launched with `NO_COLOR=1`, but that
        // policy applies to its parent process's logs, not to terminals
        // ilium emulates. Leaving it inherited makes every child shell,
        // Claude, Codex, and color-aware CLI deliberately monochrome even
        // though this PTY advertises full xterm-256color support.
        cmd.env_remove("NO_COLOR");
        for (key, value) in &command.env {
            cmd.env(key, value);
        }
        // The spawning process's own `TERM` is inherited from whatever
        // terminal launched it -- often `tmux-256color`/`screen-256color`,
        // since ilium is routinely run inside tmux itself. Left alone, the
        // *child* shell would inherit that same value and believe it's
        // talking to a real tmux/screen, which understands nonstandard
        // private escapes such as tmux's own `ESC k <title> ESC \`
        // window-title sequence (shell prompt themes emit it via
        // `preexec` to show the running command in the pane title). The
        // `vt100`-based emulator behind this pty doesn't implement that
        // tmux-specific protocol -- it just falls out of escape parsing
        // after the lone `ESC k` and prints the title text (the command
        // name) as literal characters, i.e. "the command name gets echoed
        // again on the next line". Advertising a plain, widely-supported
        // terminal type here makes the child (and anything it runs)
        // query/report only capabilities this emulator actually has. This
        // is applied last and unconditionally of `command.env` -- deliberately
        // *after* the loop above, so a caller-supplied `TERM` (via
        // `PtyCommand::env`) can never win the `CommandBuilder`'s
        // last-write-wins map merge and leak tmux/screen-specific escapes
        // into this `vt100` emulator. This is a correctness fix for every
        // pty this crate spawns, not a per-command choice.
        cmd.env("TERM", "xterm-256color");

        let mut child = pair.slave.spawn_command(cmd).map_err(PtyError::Spawn)?;
        // Drop our end of the slave once the child has it; keeping it open
        // would prevent the child from ever seeing EOF/HUP on its tty.
        drop(pair.slave);

        // From here on the child is already running and nothing else owns
        // it yet -- no `PtySession` exists to `kill()` it on a later `Drop`,
        // and no reader thread exists yet to reap it either (that thread is
        // only spawned once setup below succeeds). If either setup step
        // below fails (e.g. `dup`-equivalent fd exhaustion), we must kill
        // *and reap* the child explicitly before returning `Err`, or it
        // leaks as a zombie: `child` is a plain `std::process::Child` under
        // the hood, which neither terminates nor reaps its process on drop.
        let mut reader = match Self::open_cancellable_reader(&*pair.master) {
            Ok(reader) => reader,
            Err(err) => {
                let _ = child.kill();
                // `kill()` above only guarantees reaping when the child
                // dies promptly after SIGHUP; reap explicitly so this
                // early-return path can never leave a zombie behind (see
                // the `child` field doc comment).
                let _ = child.wait();
                return Err(err);
            }
        };
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = match pair.master.take_writer() {
            Ok(writer) => Arc::new(Mutex::new(writer)),
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PtyError::Io(err));
            }
        };
        let process_id = child.process_id();
        let child = Arc::new(Mutex::new(child));

        let parser = Arc::new(RwLock::new(vt100::Parser::new_with_callbacks(
            rows,
            cols,
            0,
            TerminalQueryResponder {
                reply_writer: Arc::clone(&writer),
            },
        )));
        let screen_generation = Arc::new(AtomicU64::new(0));
        let (screen_changed_tx, screen_changed_rx) = watch::channel(());
        // Capacity is chunks-buffered, not bytes: at 8KiB per chunk this
        // comfortably absorbs a slow/momentarily-disconnected subscriber
        // (e.g. `ilium-server`'s forwarder task between polls) without
        // unbounded memory growth. A lagging subscriber gets
        // `RecvError::Lagged` rather than silently missing data forever --
        // the caller decides how to handle that (see `subscribe_output_bytes`).
        const OUTPUT_BYTES_CHANNEL_CAPACITY: usize = 256;
        let (output_bytes_tx, _) = broadcast::channel(OUTPUT_BYTES_CHANNEL_CAPACITY);
        let output_journal = Arc::new(Mutex::new(OutputJournal {
            chunks: VecDeque::new(),
            retained_bytes: 0,
            next_sequence: 0,
            is_complete: true,
        }));
        let reader_should_stop = Arc::new(AtomicBool::new(false));
        {
            let parser = Arc::clone(&parser);
            let screen_generation = Arc::clone(&screen_generation);
            let output_bytes_tx = output_bytes_tx.clone();
            let output_journal = Arc::clone(&output_journal);
            let child = Arc::clone(&child);
            let reader_should_stop = Arc::clone(&reader_should_stop);
            // Not keeping the `JoinHandle` around, but this thread is not
            // fire-and-forget: `reader_should_stop` (set by `Drop` below) is
            // its cancellation path, checked by `CancellableReader::read_next`
            // between -- and *during*, via a bounded `poll()` on unix or a
            // bounded channel receive against a decoupled pump thread on
            // every other target (see `PollableMasterReader`/
            // `BlockingMasterReader`) -- waits for more pty output. That is
            // what lets this thread exit (and release its `Arc` clones of
            // `parser`/`output_journal`/`child`) promptly even if a
            // descendant of the spawned child is still holding the pty's
            // slave fd open, which killing only the direct child (see the
            // `child` field doc comment) cannot by itself unblock a plain
            // blocking `read()` from. Not joining the handle is deliberate
            // too: joining from `Drop` could block the caller for up to one
            // wait interval (`POLL_TIMEOUT_MILLIS`/`RECV_TIMEOUT`), which
            // callers tearing down a pane synchronously should not pay.
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                while let ReadOutcome::Data(bytes_read) =
                    reader.read_next(&mut buf, &reader_should_stop)
                {
                    // A chunk may have arrived in the same instant `Drop`
                    // requested a stop; drop it rather than growing the
                    // parser/journal state past the point the owner asked
                    // this thread to stop retaining anything.
                    if reader_should_stop.load(Ordering::Acquire) {
                        break;
                    }
                    // Scope the write guard to this single `process()` call
                    // so a concurrent `with_screen`/`resize` never waits on
                    // us longer than one chunk's worth of parsing.
                    //
                    // `process()` may itself write terminal-query replies
                    // back to `writer` via `TerminalQueryResponder` (a
                    // different lock than this one), so this never
                    // deadlocks against the reply path.
                    {
                        // The lock is only ever held by this thread (here)
                        // and the owning `PtySession` (read/resize); a
                        // poisoned lock means one of those panicked, which
                        // we treat as unrecoverable for this pane.
                        let mut parser = parser.write().unwrap();
                        parser.process(&buf[..bytes_read]);
                        screen_generation.fetch_add(1, Ordering::Release);
                    }
                    // Best-effort: `send` only errors once every receiver
                    // (including the one kept alive by this `PtySession`)
                    // has been dropped, i.e. the pane is already gone and
                    // this thread is about to exit on its own via the next
                    // failed read.
                    let _ = screen_changed_tx.send(());
                    // Also best-effort: a `broadcast::Sender::send` only
                    // errors when there are currently zero receivers (no
                    // client attached right now), which is a normal state
                    // for a detached pane, not a failure.
                    let output_chunk = output_journal
                        .lock()
                        .unwrap()
                        .append(buf[..bytes_read].to_vec());
                    let _ = output_bytes_tx.send(output_chunk);
                }
                // The read loop above ends once the pty's slave side is
                // fully closed (EOF), an unrecoverable I/O error occurs, or
                // `Drop` requested a stop. `kill()`/`Drop` deliberately only
                // *signal* the direct child (see the `child` field doc
                // comment for why), so this thread is the one place that
                // actually collects its exit status -- without this, a
                // child that needed a SIGKILL escalation to die would be
                // left as a zombie for the rest of this process's life.
                // Blocking here is fine even on the `Stopped` path: `Drop`
                // always kills the direct child *before* requesting a
                // stop (see `impl Drop for PtySession`), and SIGKILL is
                // unignorable, so this `wait()` only ever blocks on a
                // process that is already dying, never on an orphaned
                // descendant this thread was never waiting on in the first
                // place.
                let _ = child.lock().unwrap().wait();
            });
        }

        Ok(Self {
            parser,
            screen_generation,
            writer,
            master: Mutex::new(pair.master),
            child,
            process_id,
            screen_changed: screen_changed_rx,
            output_bytes: output_bytes_tx,
            reader_should_stop,
            output_journal,
        })
    }

    /// Opens the `CancellableReader` the background reader thread will use
    /// for the lifetime of this session. Unix gets a `poll()`-capable
    /// duplicate of the master fd; every other target falls back to
    /// `portable_pty`'s own plain blocking reader (see `BlockingMasterReader`).
    #[cfg(unix)]
    fn open_cancellable_reader(
        master: &(dyn MasterPty + Send),
    ) -> Result<Box<dyn CancellableReader>, PtyError> {
        Ok(Box::new(PollableMasterReader::duplicate_from(master)?))
    }

    #[cfg(not(unix))]
    fn open_cancellable_reader(
        master: &(dyn MasterPty + Send),
    ) -> Result<Box<dyn CancellableReader>, PtyError> {
        let reader = master.try_clone_reader().map_err(PtyError::Io)?;
        Ok(Box::new(BlockingMasterReader::spawn_pumping_from(reader)))
    }

    /// Writes raw bytes (already-encoded key input) to the pty.
    pub fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        // Poisoned-lock panic is an invariant violation (see `spawn`).
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    /// Forwards one host-terminal mouse event to the pty only when the
    /// application inside it explicitly enabled an xterm mouse protocol.
    /// Coordinates are zero-based and relative to the pane content box.
    pub fn write_mouse_input(
        &self,
        event: MouseEvent,
        column: u16,
        row: u16,
    ) -> Result<(), PtyError> {
        let encoded = self.with_screen(|screen| {
            encode_mouse_event(
                event,
                column,
                row,
                screen.mouse_protocol_mode(),
                screen.mouse_protocol_encoding(),
            )
        });
        if let Some(encoded) = encoded {
            self.write(&encoded)?;
        }
        Ok(())
    }

    /// Resizes both the OS pty and the `vt100` parser's screen. `rows`/`cols`
    /// are clamped to [`MINIMUM_PTY_DIMENSION`] before use -- see that
    /// constant's doc comment for why a `0` here must never reach the
    /// `vt100` parser.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        let rows = clamp_pty_dimension(rows);
        let cols = clamp_pty_dimension(cols);
        // Poisoned-lock panic is an invariant violation (see `spawn`).
        self.master
            .lock()
            .unwrap()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(PtyError::Resize)?;
        // See the comment in `spawn`'s reader thread: a poisoned lock here
        // means some other holder already panicked, which we can't recover
        // from anyway.
        let mut parser = self.parser.write().unwrap();
        parser.screen_mut().set_size(rows, cols);
        self.screen_generation.fetch_add(1, Ordering::Release);
        Ok(())
    }

    /// Runs `f` with a read lock on the current `vt100::Screen`.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        // Poisoned-lock panic is an invariant violation (see `spawn`).
        let guard = self.parser.read().unwrap();
        f(guard.screen())
    }

    /// Plain-text dump of the current screen.
    pub fn screen_text(&self) -> String {
        self.with_screen(|screen| screen.contents())
    }

    /// Returns visible text and its monotonic parser generation under one read
    /// lock, preventing a detection pass from pairing text from one frame with
    /// the revision of another.
    pub fn screen_snapshot(&self) -> ScreenSnapshot {
        let parser = self.parser.read().unwrap();
        ScreenSnapshot {
            generation: self.screen_generation.load(Ordering::Acquire),
            text: parser.screen().contents(),
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

    /// Best-effort cwd of the directly spawned shell or command. Linux
    /// exposes it through procfs; other targets deliberately return `None`
    /// so higher layers can fall back to the project root safely.
    pub fn current_working_directory(&self) -> Option<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            let process_id = self.process_id?;
            std::fs::read_link(format!("/proc/{process_id}/cwd")).ok()
        }

        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    pub fn foreground_process_group_id(&self) -> Option<u32> {
        #[cfg(unix)]
        {
            let process_group_id = self.master.lock().unwrap().process_group_leader()?;
            u32::try_from(process_group_id).ok()
        }

        #[cfg(not(unix))]
        {
            None
        }
    }

    /// Non-blocking check of whether the child process has exited.
    pub fn has_exited(&mut self) -> bool {
        // Poisoned-lock panic is an invariant violation (see `spawn`).
        // `try_wait` returning `Err` means we couldn't determine status;
        // treat that as "not (known to be) exited" rather than guessing.
        matches!(self.child.lock().unwrap().try_wait(), Ok(Some(_)))
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

    /// Returns the smallest safe repair for a downstream parser known to
    /// contain every journal chunk through `after_sequence`.
    pub fn output_recovery_after(&self, after_sequence: u64) -> Option<PtyOutputRecovery> {
        self.output_journal
            .lock()
            .unwrap()
            .recovery_after(after_sequence)
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
        if matches!(child.try_wait(), Ok(Some(_))) {
            return Ok(());
        }
        child.kill().map_err(PtyError::Kill)
    }
}

impl Drop for PtySession {
    /// Best-effort kill of the spawned child, then an unconditional request
    /// for the background reader thread (see `spawn`) to stop, so a
    /// `PtySession` dropped without a preceding, successful `kill()` call
    /// (an error path, a future call site that forgets, a panic unwind)
    /// can't leave that thread -- and its `Arc` clones of the (fairly
    /// large) `vt100::Parser` screen state, the bounded output journal, and
    /// the child handle -- alive for the rest of this (potentially
    /// long-lived) server process.
    ///
    /// Order matters: the child is killed *before* `reader_should_stop` is
    /// set. `kill()` sends an unignorable SIGKILL-equivalent to the direct
    /// child, so it dies promptly regardless of whether anything is
    /// reading its output; only once that signal is sent do we ask the
    /// reader thread to stop, so its final `child.lock().unwrap().wait()`
    /// (see `spawn`) blocks on an already-dying process rather than a live
    /// one still waiting to be killed.
    ///
    /// This does not depend on the direct child's tty actually closing.
    /// Killing the direct child -- the pty's session leader -- makes Linux
    /// hang up the pty for every fd still referencing it *within that
    /// session*, which already unblocks a plain `read()` for most
    /// descendants (an ordinary backgrounded job, for instance). It does
    /// not reach a descendant that called `setsid()` itself and re-parented
    /// into its own session while still holding the inherited, unredirected
    /// slave fd (some daemonizing code does this without also redirecting
    /// stdio away) -- that fd can stay open indefinitely. For exactly that
    /// remaining case, `CancellableReader::read_next` re-checks
    /// `reader_should_stop` on a bounded interval -- via `poll()` on unix
    /// (`PollableMasterReader`), or via a bounded receive against a
    /// decoupled pump thread everywhere else (`BlockingMasterReader`) --
    /// instead of only ever waking on incoming bytes, so the reader thread
    /// exits within about one wait interval regardless of what any
    /// descendant does. That descendant itself is
    /// not signaled -- `kill()`/`Drop` only ever reach the directly-spawned
    /// child, the same limitation every terminal multiplexer has -- but its
    /// orphaned output no longer has anywhere in this process left to
    /// accumulate into once the reader thread has exited and dropped its
    /// `Arc` clones.
    ///
    /// Matches `kill()`'s own semantics: a child that already exited (the
    /// ordinary case, since callers normally call `kill()` explicitly
    /// before a `PtySession` is dropped) is not an error, and a failure to
    /// signal an already-gone process is not worth surfacing from `Drop`.
    fn drop(&mut self) {
        if !self.has_exited() {
            // Poisoned-lock panic is an invariant violation (see `spawn`).
            let _ = self.child.lock().unwrap().kill();
        }
        self.reader_should_stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal() -> OutputJournal {
        OutputJournal {
            chunks: VecDeque::new(),
            retained_bytes: 0,
            next_sequence: 0,
            is_complete: true,
        }
    }

    #[test]
    fn output_recovery_returns_only_the_contiguous_missing_tail() {
        let mut journal = journal();
        journal.append(b"first".to_vec());
        journal.append(b"-second".to_vec());
        journal.append(b"-third".to_vec());

        assert_eq!(
            journal.recovery_after(1),
            Some(PtyOutputRecovery::Delta(PtyOutputChunk {
                sequence: 3,
                bytes: b"-second-third".to_vec(),
            }))
        );
        assert_eq!(journal.recovery_after(3), None);
    }

    #[test]
    fn output_recovery_falls_back_to_replay_after_retained_history_was_lost() {
        let mut journal = journal();
        journal.append(b"discarded".to_vec());
        journal.append(b"retained".to_vec());
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
