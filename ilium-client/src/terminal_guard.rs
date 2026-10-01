//! RAII guard that enters raw mode / the alternate screen / mouse capture
//! on construction and restores the terminal to normal on drop -- including
//! on panic unwind, so a crash never leaves the user's shell stuck in raw
//! mode. Ported unchanged in spirit from the pre-client/server bin's own
//! `main.rs::TerminalGuard`; owning terminal lifecycle here (rather than
//! leaving it to the `ilium` bin) matches ilium-client's ARCHITECTURE.md role as
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
    disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, DisableLineWrap,
    EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen,
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
        // Line wrapping off for the whole session. This TUI positions every
        // cell absolutely and never relies on the cursor wrapping, but leaving
        // wrapping enabled is not merely unused: writing the terminal's
        // bottom-right cell -- which the footer's own last cell is -- wraps the
        // cursor, and on the last row a wrap scrolls the whole screen up by
        // one. Hosts differ on whether they defer that wrap until the next
        // character, so on Windows the frame ended up one row high with the
        // footer sitting where the next full-screen overlay painted over it,
        // and ratatui (which writes only cell diffs, and sees no change in
        // those cells) never put it back.
        if let Err(source) = execute!(
            io::stdout(),
            EnterAlternateScreen,
            DisableLineWrap,
            EnableBracketedPaste,
            EnableMouseCapture,
            EnableFocusChange
        ) {
            // `execute!` writes each command's escape sequence in order and
            // bails out on the first failure, so any prefix of these four
            // commands may already have taken effect on the real terminal
            // even though we're about to return an error. Best-effort undo
            // all four (mirrors the Drop impl / the keyboard-enhancement
            // error path below) rather than leaving the terminal stuck in
            // the alternate screen or with mouse capture enabled.
            let _ = execute!(
                io::stdout(),
                DisableFocusChange,
                DisableMouseCapture,
                DisableBracketedPaste,
                EnableLineWrap,
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
                    EnableLineWrap,
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
            EnableLineWrap,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}

/// Keeps the frame's bottom-right cell out of the cell diff when `enabled`.
///
/// Windows ConPTY scrolls the whole screen when that cell is written, even
/// with line wrap disabled, leaving every later differential write one row
/// off. The footer's last cell is blank padding, so never writing it costs
/// nothing visible and removes the scroll.
pub(crate) fn skip_bottom_right_cell(frame: &mut ratatui::Frame, enabled: bool) {
    let area = frame.area();
    if !enabled || area.width == 0 || area.height == 0 {
        return;
    }
    frame.buffer_mut()[(area.right() - 1, area.bottom() - 1)]
        .set_diff_option(ratatui::buffer::CellDiffOption::Skip);
}

#[cfg(test)]
mod skip_tests {
    use super::skip_bottom_right_cell;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn only_the_bottom_right_cell_is_skipped_and_only_when_enabled() {
        let mut terminal = Terminal::new(TestBackend::new(6, 3)).unwrap();
        for enabled in [false, true] {
            terminal
                .draw(|frame| {
                    skip_bottom_right_cell(frame, enabled);
                    let buffer = frame.buffer_mut();
                    for y in 0..3u16 {
                        for x in 0..6u16 {
                            let expected = enabled && (x, y) == (5, 2);
                            assert_eq!(
                                buffer[(x, y)].diff_option == ratatui::buffer::CellDiffOption::Skip,
                                expected,
                                "{x},{y} enabled={enabled}"
                            );
                        }
                    }
                })
                .unwrap();
        }
    }
}
