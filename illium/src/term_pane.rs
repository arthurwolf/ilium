//! Thin wrapper around `illium_pty::PtySession` for one terminal pane.
//!
//! All the actual pty/vt100 machinery -- spawn, background reader thread,
//! terminal capability-query answering, mouse-event encoding -- lives in
//! the `illium-pty` crate now (see its module docs). This file exists so
//! the rest of the bin crate (`app.rs`, `ui.rs`) keeps its existing
//! `TerminalPane` API and doesn't need to know `illium-pty` exists; it's
//! the one place in this crate that talks to `illium_pty::*` directly.
//!
//! No behavior changes here: the bin crate is still fully synchronous and
//! never touches `PtySession::subscribe_screen_changed` (that channel
//! exists for the future async detection loop in `illium-server`, not for
//! this crate's render-on-demand call sites).

use std::path::Path;

use crossterm::event::MouseEvent;
use illium_pty::{PtyCommand, PtySession};

/// One terminal pane: a spawned child process behind a pty, plus its
/// renderable screen state.
pub struct TerminalPane {
    session: PtySession,
}

impl TerminalPane {
    /// Spawns `shell_cmd` (e.g. "/bin/bash") directly as the pty's child
    /// process, with the given starting working directory.
    pub fn spawn(shell_cmd: &str, cwd: &Path, rows: u16, cols: u16) -> anyhow::Result<Self> {
        let session = PtySession::spawn(PtyCommand::new(shell_cmd, cwd, rows, cols))?;
        Ok(Self { session })
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
        let command = PtyCommand::new(shell, cwd, rows, cols)
            .arg("-c")
            .arg(command_line);
        let session = PtySession::spawn(command)?;
        Ok(Self { session })
    }

    /// Writes raw bytes (already-encoded key input) to the pty.
    pub fn write_input(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.session.write(bytes)?;
        Ok(())
    }

    /// Forwards one host-terminal mouse event to the pty only when the
    /// application inside it explicitly enabled an xterm mouse protocol.
    /// Coordinates are zero-based and relative to the pane content box.
    pub fn write_mouse_input(
        &mut self,
        event: MouseEvent,
        column: u16,
        row: u16,
    ) -> anyhow::Result<()> {
        self.session.write_mouse_input(event, column, row)?;
        Ok(())
    }

    /// Resizes both the OS pty and the vt100 parser's screen.
    pub fn resize(&self, rows: u16, cols: u16) -> anyhow::Result<()> {
        self.session.resize(rows, cols)?;
        Ok(())
    }

    /// Runs `f` with a read lock on the current `vt100::Screen`, for
    /// rendering via `tui_term::widget::PseudoTerminal::new(screen)`.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        self.session.with_screen(f)
    }

    /// Plain-text dump of the current screen, used as input to
    /// `agent_detect::classify_activity`.
    pub fn screen_text(&self) -> String {
        self.session.screen_text()
    }

    /// OS pid of the directly-spawned child (the shell), used as the root
    /// for `agent_detect::identify_agent`'s process-tree walk. `None` if
    /// the platform didn't report one.
    pub fn shell_pid(&self) -> Option<u32> {
        self.session.process_id()
    }

    /// Non-blocking check of whether the child process has exited.
    pub fn has_exited(&mut self) -> bool {
        self.session.has_exited()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_command_spawns_through_the_requested_shell() {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let pane = TerminalPane::spawn_shell_command(&shell, "exit 0", Path::new("/tmp"), 2, 2);
        assert!(pane.is_ok());
    }
}
