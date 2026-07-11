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
//! To let async callers (the detection loop, later, in `illium-server`)
//! observe screen changes without polling, the reader thread also notifies
//! a `tokio::sync::watch::channel(())` after every chunk it parses. The
//! channel carries no payload -- it is purely a "something changed, go
//! re-read the shared parser via `with_screen`/`screen_text`" signal, not a
//! snapshot of the screen itself. That keeps the (fairly large) `vt100`
//! screen state single-sourced in the `Arc<RwLock<_>>` instead of cloning
//! it through a channel on every byte chunk, and it matches exactly how
//! synchronous callers already read the screen: on demand, not by being
//! handed a copy.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use crossterm::event::MouseEvent;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tokio::sync::watch;

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
    // The pty master's control handle; used for resizing.
    master: Box<dyn MasterPty + Send>,
    // The spawned child process handle; used for exit-status polling.
    child: Box<dyn Child + Send + Sync>,
    // OS pid of the directly-spawned child, if the platform reported one.
    process_id: Option<u32>,
    // Held so `subscribe_screen_changed` can hand out clones; a `watch`
    // receiver never lets its sender's send fail as "no receivers left"
    // while at least one clone (this one) is alive.
    screen_changed: watch::Receiver<()>,
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
        // The spawning process's own `TERM` is inherited from whatever
        // terminal launched it -- often `tmux-256color`/`screen-256color`,
        // since illium is routinely run inside tmux itself. Left alone, the
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
        // is always applied, unconditionally of `command.env`, since it's
        // a correctness fix for every pty this crate spawns, not a
        // per-command choice.
        cmd.env("TERM", "xterm-256color");
        for (key, value) in &command.env {
            cmd.env(key, value);
        }

        let child = pair.slave.spawn_command(cmd).map_err(PtyError::Spawn)?;
        // Drop our end of the slave once the child has it; keeping it open
        // would prevent the child from ever seeing EOF/HUP on its tty.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(PtyError::Io)?;
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(pair.master.take_writer().map_err(PtyError::Io)?));
        let process_id = child.process_id();

        let parser = Arc::new(RwLock::new(vt100::Parser::new_with_callbacks(
            command.rows,
            command.cols,
            0,
            TerminalQueryResponder {
                reply_writer: Arc::clone(&writer),
            },
        )));
        let (screen_changed_tx, screen_changed_rx) = watch::channel(());
        {
            let parser = Arc::clone(&parser);
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
                }
            });
        }

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            process_id,
            screen_changed: screen_changed_rx,
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
        self.master
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

    /// Non-blocking check of whether the child process has exited.
    pub fn has_exited(&mut self) -> bool {
        // `try_wait` returning `Err` means we couldn't determine status;
        // treat that as "not (known to be) exited" rather than guessing.
        matches!(self.child.try_wait(), Ok(Some(_)))
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
}
