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

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use crossterm::event::MouseEvent;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
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
}

/// One ordered piece of raw output from a PTY. The sequence belongs to the
/// pane's lifetime, letting clients discard a live broadcast that was already
/// included in their attach-time replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOutputChunk {
    pub sequence: u64,
    pub bytes: Vec<u8>,
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
}

impl PtySession {
    /// Spawns `command` behind a new pty and starts the background reader
    /// thread that feeds its output into the shared `vt100::Parser`.
    pub fn spawn(command: PtyCommand) -> Result<Self, PtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: command.rows,
                cols: command.cols,
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
        let mut reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(err) => {
                let _ = child.kill();
                // `kill()` above only guarantees reaping when the child
                // dies promptly after SIGHUP; reap explicitly so this
                // early-return path can never leave a zombie behind (see
                // the `child` field doc comment).
                let _ = child.wait();
                return Err(PtyError::Io(err));
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
            command.rows,
            command.cols,
            0,
            TerminalQueryResponder {
                reply_writer: Arc::clone(&writer),
            },
        )));
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
        {
            let parser = Arc::clone(&parser);
            let output_bytes_tx = output_bytes_tx.clone();
            let output_journal = Arc::clone(&output_journal);
            let child = Arc::clone(&child);
            // Deliberately not keeping the `JoinHandle` around: this thread
            // has no way to be woken up short of the pty actually reaching
            // EOF/error, so the correct "cancellation path" for it (see
            // the crate-level rule that every spawned task needs one) is
            // ensuring the child gets killed -- which unblocks the read
            // and lets the thread exit on its own -- rather than joining
            // it, which could block the caller indefinitely if joined
            // from `Drop` before the child has actually exited. See
            // `impl Drop for PtySession` below for where that
            // cancellation is triggered.
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    let read_result = reader.read(&mut buf);
                    let bytes_read = match read_result {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
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
                // The read loop above only ends once the pty's slave side
                // is fully closed (EOF) or errors, which happens once the
                // child is gone. `kill()`/`Drop` below deliberately only
                // *signal* the child (see the `child` field doc comment
                // for why), so this thread is the one place that actually
                // collects the exit status -- without this, a child that
                // needed a SIGKILL escalation to die would be left as a
                // zombie for the rest of this process's life. Blocking
                // here is fine: by this point the child is already dead
                // or dying, and nothing else waits on this thread.
                let _ = child.lock().unwrap().wait();
            });
        }

        Ok(Self {
            parser,
            writer,
            master: Mutex::new(pair.master),
            child,
            process_id,
            screen_changed: screen_changed_rx,
            output_bytes: output_bytes_tx,
            output_journal,
        })
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

    /// Resizes both the OS pty and the `vt100` parser's screen.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
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
        self.parser
            .write()
            .unwrap()
            .screen_mut()
            .set_size(rows, cols);
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

    /// OS pid of the directly-spawned child, `None` if the platform didn't
    /// report one.
    pub fn process_id(&self) -> Option<u32> {
        self.process_id
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

    /// Terminates the spawned child process. A no-op returning `Ok(())` if
    /// the child has already exited -- there is nothing left to kill, and
    /// treating that as an error would make normal pane teardown (the
    /// child often exits on its own right before the caller gets around to
    /// closing the pane) look like a failure.
    pub fn kill(&mut self) -> Result<(), PtyError> {
        if self.has_exited() {
            return Ok(());
        }
        // Poisoned-lock panic is an invariant violation (see `spawn`).
        self.child.lock().unwrap().kill().map_err(PtyError::Kill)
    }
}

impl Drop for PtySession {
    /// Best-effort kill of the spawned child on teardown, so a `PtySession`
    /// dropped without a preceding, successful `kill()` call (an error
    /// path, a future call site that forgets, a panic unwind) can't leave
    /// the background reader thread (see `spawn`) blocked in `read()` on a
    /// still-running child forever. That thread owns its own clone of
    /// `Arc<RwLock<vt100::Parser>>` (the "fairly large" screen state
    /// described in the module docs), a clone of the `output_bytes`
    /// broadcast sender, a clone of the `Arc<Mutex<_>>`-wrapped child
    /// handle (which it uses to reap the exit status once its read loop
    /// ends), and a separately-cloned pty reader file descriptor -- none
    /// of those are released until the thread's `read` call returns,
    /// which only happens once the child (and thus the pty's slave side)
    /// is gone. Killing it here bounds that to "until the OS finishes
    /// tearing down a killed process" instead of "until the
    /// process happens to exit on its own, which may be never."
    ///
    /// Matches `kill()`'s own semantics: a child that already exited (the
    /// ordinary case, since callers normally call `kill()` explicitly
    /// before a `PtySession` is dropped) is not an error, and a failure to
    /// signal an already-gone process is not worth surfacing from `Drop`.
    /// Like `kill()`, this only signals the directly-spawned child, not
    /// any descendant that may have inherited the pty and kept its slave
    /// side open independently -- the same limitation every terminal
    /// multiplexer has, and not fixable from inside this crate.
    fn drop(&mut self) {
        if !self.has_exited() {
            // Poisoned-lock panic is an invariant violation (see `spawn`).
            let _ = self.child.lock().unwrap().kill();
        }
    }
}
