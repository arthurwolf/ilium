//! Adapter around `portable-pty` + `vt100` for one terminal pane.
//!
//! This is the only file in the crate that touches `portable_pty::*` and
//! `vt100::*` directly — everything else (tree UI, agent detection, app
//! state) only ever sees `TerminalPane`'s narrow API (write bytes in, read
//! screen text out). That keeps the PTY/VT100 machinery, which is fiddly
//! and platform-dependent, isolated behind one boundary.
//!
//! The live PTY output is consumed by a background reader thread that owns
//! the *only* long-lived write access to the shared `vt100::Parser`; the
//! main thread only ever takes a read lock (`with_screen`) except when
//! resizing. The parser is wrapped in `Arc<RwLock<_>>` so both sides can
//! reach it without the reader thread blocking UI redraws for longer than
//! a single `process()` call.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

/// One terminal pane: a spawned child process behind a PTY, plus the
/// `vt100` parser that turns its raw byte stream into a renderable screen.
pub struct TerminalPane {
    // Shared with the background reader thread; `with_screen`/`screen_text`
    // take a read lock, `resize` takes a write lock. The reader thread only
    // ever holds the write lock for the duration of a single `process()`
    // call (see `spawn`), so lock contention stays minimal.
    parser: Arc<RwLock<vt100::Parser<TerminalQueryResponder>>>,
    // The pty master's write half; writing here sends bytes to the child's
    // stdin (as seen through the pty). Shared with the reader thread's
    // `TerminalQueryResponder` (see `spawn_command`), which writes terminal
    // capability-query replies back down the same channel.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    // The pty master's control handle; used for resizing.
    master: Box<dyn MasterPty + Send>,
    // The spawned child process handle; used for exit-status polling.
    child: Box<dyn Child + Send + Sync>,
    // OS pid of the directly-spawned child (the shell), if the platform
    // reported one. Root of the agent-detection process-tree walk.
    shell_pid: Option<u32>,
}

impl TerminalPane {
    /// Spawns `shell_cmd` (e.g. "/bin/bash") directly as the pty's child
    /// process, with the given starting working directory.
    pub fn spawn(shell_cmd: &str, cwd: &Path, rows: u16, cols: u16) -> anyhow::Result<Self> {
        Self::spawn_command(CommandBuilder::new(shell_cmd), cwd, rows, cols)
    }

    /// Spawns an arbitrary shell command line by running it through
    /// `shell -c command_line` (e.g. `$SHELL -c "htop"`), so ordinary
    /// shell syntax -- arguments, quoting, pipes -- works exactly like
    /// typing it at a prompt, without illium needing its own
    /// shell-word-splitter.
    pub fn spawn_shell_command(
        shell: &str,
        command_line: &str,
        cwd: &Path,
        rows: u16,
        cols: u16,
    ) -> anyhow::Result<Self> {
        let mut cmd = CommandBuilder::new(shell);
        cmd.arg("-c");
        cmd.arg(command_line);
        Self::spawn_command(cmd, cwd, rows, cols)
    }

    /// Spawns a background thread that continuously reads the PTY master
    /// and feeds bytes into the shared `vt100::Parser`.
    fn spawn_command(
        mut cmd: CommandBuilder,
        cwd: &Path,
        rows: u16,
        cols: u16,
    ) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        cmd.cwd(cwd);
        // illium's own `TERM` is inherited from whatever terminal launched
        // it -- often `tmux-256color`/`screen-256color`, since illium is
        // routinely run inside tmux itself. Left alone, the *child* shell
        // would inherit that same value and believe it's talking to a real
        // tmux/screen, which understands nonstandard private escapes such
        // as tmux's own `ESC k <title> ESC \` window-title sequence (shell
        // prompt themes emit it via `preexec` to show the running command
        // in the pane title). illium's `vt100`-based emulator doesn't
        // implement that tmux-specific protocol -- it just falls out of
        // escape parsing after the lone `ESC k` and prints the title text
        // (the command name) as literal characters, which is exactly the
        // "command name gets echoed again on the next line" bug this
        // fixes. Advertising a plain, widely-supported terminal type here
        // makes the child (and anything it runs) query/report only
        // capabilities illium's emulator actually has.
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd)?;
        // Drop our end of the slave once the child has it; keeping it open
        // would prevent the child from ever seeing EOF/HUP on its tty.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(pair.master.take_writer()?));
        let shell_pid = child.process_id();

        let parser = Arc::new(RwLock::new(vt100::Parser::new_with_callbacks(
            rows,
            cols,
            0,
            TerminalQueryResponder {
                reply_writer: Arc::clone(&writer),
            },
        )));
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
                    // so the UI thread's `with_screen`/`resize` never wait
                    // on us longer than one chunk's worth of parsing.
                    //
                    // `process()` may itself write terminal-query replies
                    // back to `writer` via `TerminalQueryResponder` (a
                    // different lock than this one), so this never
                    // deadlocks against the reply path.
                    {
                        // The lock is only ever held by this thread (here)
                        // and the owning `TerminalPane` (read/resize); a
                        // poisoned lock means one of those panicked, which
                        // we treat as unrecoverable for this pane.
                        let mut parser = parser.write().unwrap();
                        parser.process(&buf[..bytes_read]);
                    }
                }
            });
        }

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            shell_pid,
        })
    }

    /// Writes raw bytes (already-encoded key input) to the pty.
    pub fn write_input(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        // Poisoned-lock panic is an invariant violation (see `spawn`).
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    /// Forwards one host-terminal mouse event to the PTY only when the
    /// application inside it explicitly enabled an xterm mouse protocol.
    /// Coordinates are zero-based and relative to the pane content box.
    pub fn write_mouse_input(
        &mut self,
        event: MouseEvent,
        column: u16,
        row: u16,
    ) -> anyhow::Result<()> {
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
            self.write_input(&encoded)?;
        }
        Ok(())
    }

    /// Resizes both the OS pty and the vt100 parser's screen.
    pub fn resize(&self, rows: u16, cols: u16) -> anyhow::Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
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

    /// Runs `f` with a read lock on the current `vt100::Screen`, for
    /// rendering via `tui_term::widget::PseudoTerminal::new(screen)`.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        // Poisoned-lock panic is an invariant violation (see `spawn`).
        let guard = self.parser.read().unwrap();
        f(guard.screen())
    }

    /// Plain-text dump of the current screen, used as input to
    /// `agent_detect::classify_activity`.
    pub fn screen_text(&self) -> String {
        self.with_screen(|screen| screen.contents())
    }

    /// OS pid of the directly-spawned child (the shell), used as the root
    /// for `agent_detect::identify_agent`'s process-tree walk. `None` if
    /// the platform didn't report one.
    pub fn shell_pid(&self) -> Option<u32> {
        self.shell_pid
    }

    /// Non-blocking check of whether the child process has exited.
    pub fn has_exited(&mut self) -> bool {
        // `try_wait` returning `Err` means we couldn't determine status;
        // treat that as "not (known to be) exited" rather than guessing.
        matches!(self.child.try_wait(), Ok(Some(_)))
    }
}

/// Answers the small set of terminal capability-query escape sequences that
/// well-behaved terminal apps send and then block waiting for a reply.
///
/// `vt100::Parser` is a pure output *parser* -- it has no notion of writing
/// anything back to the child, so without this, any query the child sends
/// (e.g. crossterm's own `supports_keyboard_enhancement`, which every
/// ratatui-style TUI including illium itself calls at startup) goes
/// unanswered. The child then sits blocked on its own read-with-timeout
/// (crossterm's is 2 seconds) before giving up and continuing without the
/// feature -- exactly the "prints its banner, then pauses for a couple of
/// seconds before the real UI appears" symptom this fixes. A real terminal
/// emulator answers these immediately, which is why the same command run
/// directly in e.g. Ghostty or the VS Code terminal doesn't show the pause.
struct TerminalQueryResponder {
    // Same channel `TerminalPane::write_input` writes down; a reply here
    // lands on the child's stdin exactly like a keystroke would.
    reply_writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl TerminalQueryResponder {
    fn reply(&mut self, bytes: &[u8]) {
        // Best-effort: a query reply that fails to send is no worse than
        // the unanswered query illium used to produce, and a poisoned lock
        // here would mean the pane is already being torn down.
        if let Ok(mut writer) = self.reply_writer.lock() {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
    }
}

impl vt100::Callbacks for TerminalQueryResponder {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        match (i1, c) {
            // Kitty keyboard-protocol support query (`CSI ? u`). illium
            // never forwards the kitty protocol into the child pty (see
            // `encode_key_for_terminal` in app.rs, which always emits
            // legacy ANSI sequences), so the truthful answer is "no
            // progressive-enhancement flags active".
            (Some(b'?'), 'u') => self.reply(b"\x1b[?0u"),
            // Primary Device Attributes (`CSI c` / `CSI 0 c`). Answer as a
            // basic VT100-with-AVO terminal, matching what tmux/screen
            // report for a nested session.
            (None, 'c') => self.reply(b"\x1b[?1;2c"),
            // Device Status Report (`CSI 5n`): "terminal OK".
            (None, 'n') if params.first().and_then(|p| p.first()) == Some(&5) => {
                self.reply(b"\x1b[0n");
            }
            // Cursor Position Report (`CSI 6n`), answered with the real
            // cursor position so apps that poll it for cursor-relative
            // rendering (e.g. bracketed prompts) get a correct reply
            // instead of hanging.
            (None, 'n') if params.first().and_then(|p| p.first()) == Some(&6) => {
                let (row, col) = screen.cursor_position();
                self.reply(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
            }
            _ => {}
        }
    }
}

/// Encodes a crossterm mouse event using the mode and encoding requested by
/// the process inside a terminal. Returning `None` deliberately leaves
/// ordinary shell clicks alone; terminal applications opt in with xterm's
/// DECSET mouse modes before receiving any mouse input.
fn encode_mouse_event(
    event: MouseEvent,
    column: u16,
    row: u16,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    if mode == MouseProtocolMode::None || !event_is_enabled(event.kind, mode) {
        return None;
    }

    let (button_code, release) = mouse_button_code(event.kind)?;
    let modifiers = mouse_modifier_bits(event.modifiers);
    let code = button_code | modifiers;
    let column = u32::from(column) + 1;
    let row = u32::from(row) + 1;

    match encoding {
        MouseProtocolEncoding::Sgr => {
            let suffix = if release { 'm' } else { 'M' };
            Some(format!("\x1b[<{code};{column};{row}{suffix}").into_bytes())
        }
        MouseProtocolEncoding::Default => encode_legacy_mouse(code, column, row, false),
        MouseProtocolEncoding::Utf8 => encode_legacy_mouse(code, column, row, true),
    }
}

/// Whether this event class is part of the terminal's negotiated protocol.
fn event_is_enabled(kind: MouseEventKind, mode: MouseProtocolMode) -> bool {
    match kind {
        MouseEventKind::Down(_)
        | MouseEventKind::ScrollUp
        | MouseEventKind::ScrollDown
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => true,
        MouseEventKind::Up(_) => mode != MouseProtocolMode::Press,
        MouseEventKind::Drag(_) => matches!(
            mode,
            MouseProtocolMode::ButtonMotion | MouseProtocolMode::AnyMotion
        ),
        MouseEventKind::Moved => mode == MouseProtocolMode::AnyMotion,
    }
}

/// Maps a host event to xterm's Cb value before modifier bits are applied.
fn mouse_button_code(kind: MouseEventKind) -> Option<(u8, bool)> {
    match kind {
        MouseEventKind::Down(button) => Some((button_code(button)?, false)),
        // SGR identifies releases with the final lowercase `m`; its Cb
        // retains the released button instead of using legacy button 3.
        MouseEventKind::Up(button) => Some((button_code(button)?, true)),
        MouseEventKind::Drag(button) => Some((32 | button_code(button)?, false)),
        // Motion with no held button uses legacy button 3 plus bit 5.
        MouseEventKind::Moved => Some((32 | 3, false)),
        MouseEventKind::ScrollUp => Some((64, false)),
        MouseEventKind::ScrollDown => Some((65, false)),
        MouseEventKind::ScrollLeft => Some((66, false)),
        MouseEventKind::ScrollRight => Some((67, false)),
    }
}

/// Maps crossterm's physical buttons to xterm's base button codes.
fn button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
    }
}

/// Applies the xterm modifier-bit convention to a mouse report.
fn mouse_modifier_bits(modifiers: KeyModifiers) -> u8 {
    let mut bits = 0;
    if modifiers.contains(KeyModifiers::SHIFT) {
        bits |= 4;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        bits |= 8;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        bits |= 16;
    }
    bits
}

/// Encodes classic X10/UTF-8 mouse reports. Classic encoding cannot express
/// cells outside 223; SGR-capable applications receive unrestricted values.
fn encode_legacy_mouse(code: u8, column: u32, row: u32, utf8: bool) -> Option<Vec<u8>> {
    let values = [u32::from(code) + 32, column + 32, row + 32];
    if !utf8 && values.iter().any(|value| *value > 255) {
        return None;
    }

    let mut bytes = b"\x1b[M".to_vec();
    for value in values {
        if utf8 {
            let character = char::from_u32(value)?;
            let mut buffer = [0; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
        } else {
            bytes.push(value as u8);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory `Write` sink standing in for the pty writer, so the
    /// query-responder tests below exercise `TerminalQueryResponder`
    /// directly through `vt100::Parser::process` without spawning a real
    /// pty or child process.
    #[derive(Clone, Default)]
    struct RecordingSink(Arc<Mutex<Vec<u8>>>);

    impl Write for RecordingSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn parser_with_recording_responder() -> (vt100::Parser<TerminalQueryResponder>, RecordingSink) {
        let sink = RecordingSink::default();
        let reply_writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(sink.clone())));
        let parser =
            vt100::Parser::new_with_callbacks(24, 80, 0, TerminalQueryResponder { reply_writer });
        (parser, sink)
    }

    #[test]
    fn primary_device_attributes_query_gets_a_reply() {
        let (mut parser, sink) = parser_with_recording_responder();
        parser.process(b"\x1b[c");
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"\x1b[?1;2c");
    }

    #[test]
    fn kitty_keyboard_enhancement_query_reports_no_flags() {
        // This is the exact query crossterm's `supports_keyboard_enhancement`
        // sends (see `terminal/sys/unix.rs` in the crossterm crate); without
        // a reply the child blocks on it for a 2-second timeout, which is
        // the pause this fix eliminates.
        let (mut parser, sink) = parser_with_recording_responder();
        parser.process(b"\x1b[?u\x1b[c");
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"\x1b[?0u\x1b[?1;2c");
    }

    #[test]
    fn cursor_position_report_reflects_the_real_cursor() {
        let (mut parser, sink) = parser_with_recording_responder();
        // Move the cursor to row 3, col 5 (1-indexed CUP), then ask for it.
        parser.process(b"\x1b[3;5H\x1b[6n");
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"\x1b[3;5R");
    }

    fn mouse(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn sgr_press_and_release_preserve_coordinates_and_button() {
        assert_eq!(
            encode_mouse_event(
                mouse(MouseEventKind::Down(MouseButton::Left)),
                4,
                2,
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Sgr
            ),
            Some(b"\x1b[<0;5;3M".to_vec())
        );
        assert_eq!(
            encode_mouse_event(
                mouse(MouseEventKind::Up(MouseButton::Left)),
                4,
                2,
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Sgr
            ),
            Some(b"\x1b[<0;5;3m".to_vec())
        );
    }

    #[test]
    fn motion_and_modifiers_follow_xterm_bits() {
        let mut event = mouse(MouseEventKind::Drag(MouseButton::Right));
        event.modifiers = KeyModifiers::SHIFT | KeyModifiers::CONTROL;
        assert_eq!(
            encode_mouse_event(
                event,
                0,
                0,
                MouseProtocolMode::ButtonMotion,
                MouseProtocolEncoding::Sgr
            ),
            Some(b"\x1b[<54;1;1M".to_vec())
        );
    }

    #[test]
    fn press_only_mode_drops_release_and_motion() {
        assert!(encode_mouse_event(
            mouse(MouseEventKind::Up(MouseButton::Left)),
            0,
            0,
            MouseProtocolMode::Press,
            MouseProtocolEncoding::Sgr
        )
        .is_none());
        assert!(encode_mouse_event(
            mouse(MouseEventKind::Drag(MouseButton::Left)),
            0,
            0,
            MouseProtocolMode::Press,
            MouseProtocolEncoding::Sgr
        )
        .is_none());
    }

    #[test]
    fn legacy_encoding_uses_x10_prefix() {
        assert_eq!(
            encode_mouse_event(
                mouse(MouseEventKind::ScrollDown),
                1,
                3,
                MouseProtocolMode::Press,
                MouseProtocolEncoding::Default
            ),
            Some(vec![0x1b, b'[', b'M', 97, 34, 36])
        );
    }

    #[test]
    fn shell_command_spawns_through_the_requested_shell() {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let pane = TerminalPane::spawn_shell_command(&shell, "exit 0", Path::new("/tmp"), 2, 2);
        assert!(pane.is_ok());
    }
}
