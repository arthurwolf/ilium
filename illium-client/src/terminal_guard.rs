//! RAII guard that enters raw mode / the alternate screen / mouse capture
//! on construction and restores the terminal to normal on drop -- including
//! on panic unwind, so a crash never leaves the user's shell stuck in raw
//! mode. Ported unchanged in spirit from the pre-client/server bin's own
//! `main.rs::TerminalGuard`; owning terminal lifecycle here (rather than
//! leaving it to the `illium` bin) matches illium-client's README role as
//! "the ratatui TUI" -- the bin becomes a thin CLI dispatcher in the next
//! stage, not the thing that manages raw mode.

use std::io;

use crossterm::event::{
    DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
    LeaveAlternateScreen,
};

use crate::error::ClientError;

pub struct TerminalGuard {
    keyboard_enhancement_pushed: bool,
}

impl TerminalGuard {
    pub fn enter() -> Result<Self, ClientError> {
        enable_raw_mode().map_err(ClientError::TerminalSetup)?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableFocusChange
        )
        .map_err(ClientError::TerminalSetup)?;

        // Not every terminal supports the Kitty keyboard protocol; only
        // push the enhancement flags when the terminal says it can
        // disambiguate keys, and remember to pop them again in `Drop`.
        let keyboard_enhancement_pushed = supports_keyboard_enhancement().unwrap_or(false);
        if keyboard_enhancement_pushed {
            execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )
            .map_err(ClientError::TerminalSetup)?;
        }

        Ok(Self {
            keyboard_enhancement_pushed,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Best-effort at every step: this runs during panic unwinding too,
        // where an earlier failure shouldn't stop us from attempting the
        // rest -- it's the last chance to leave the terminal usable.
        if self.keyboard_enhancement_pushed {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(
            io::stdout(),
            DisableFocusChange,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}
