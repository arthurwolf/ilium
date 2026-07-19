//! RAII guard that enters raw mode / the alternate screen / mouse capture
//! on construction and restores the terminal to normal on drop -- including
//! on panic unwind, so a crash never leaves the user's shell stuck in raw
//! mode. Ported unchanged in spirit from the pre-client/server bin's own
//! `main.rs::TerminalGuard`; owning terminal lifecycle here (rather than
//! leaving it to the `ilium` bin) matches ilium-client's README role as
//! "the ratatui TUI" -- the bin becomes a thin CLI dispatcher in the next
//! stage, not the thing that manages raw mode.

use std::io;

use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
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

        // If any later step fails, we've already mutated real terminal
        // state (raw mode, possibly the alt screen/mouse capture/focus
        // change too) but haven't constructed `Self` yet, so `Drop` will
        // never run to restore it. Unwind whatever already succeeded
        // before propagating the error -- this constructor is the only
        // chance to do so.
        if let Err(source) = execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableMouseCapture,
            EnableFocusChange
        ) {
            // `execute!` writes each command's escape sequence in order and
            // bails out on the first failure, so any prefix of these three
            // commands may already have taken effect on the real terminal
            // even though we're about to return an error. Best-effort undo
            // all three (mirrors the Drop impl / the keyboard-enhancement
            // error path below) rather than leaving the terminal stuck in
            // the alternate screen or with mouse capture enabled.
            let _ = execute!(
                io::stdout(),
                DisableFocusChange,
                DisableMouseCapture,
                DisableBracketedPaste,
                LeaveAlternateScreen
            );
            let _ = disable_raw_mode();
            return Err(ClientError::TerminalSetup(source));
        }

        // Not every terminal supports the Kitty keyboard protocol; only
        // push the enhancement flags when the terminal says it can
        // disambiguate keys, and remember to pop them again in `Drop`.
        let keyboard_enhancement_pushed = supports_keyboard_enhancement().unwrap_or(false);
        if keyboard_enhancement_pushed {
            if let Err(source) = execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            ) {
                let _ = execute!(
                    io::stdout(),
                    DisableFocusChange,
                    DisableMouseCapture,
                    DisableBracketedPaste,
                    LeaveAlternateScreen
                );
                let _ = disable_raw_mode();
                return Err(ClientError::TerminalSetup(source));
            }
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
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}
