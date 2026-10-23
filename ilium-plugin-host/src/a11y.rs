//! Accessible rendering of plugin views: attribute cues, focus order, screen-reader mode and
//! reduced motion (guide-accessibility, USR-300 .. USR-307).

use std::time::Duration;

/// The accessibility settings that change how a view is drawn.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct A11ySettings {
    /// Colour is off; meaning must show in attributes.
    pub no_color: bool,
    /// Linear screen-reader layout with role words.
    pub screen_reader: bool,
    /// At most one full redraw per second and no decorative animation.
    pub reduced_motion: bool,
    /// Theme name.
    pub theme: String,
    /// Contrast setting, `normal` or `high`.
    pub contrast: String,
}

/// The text attributes of one cell run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStyle {
    /// Bold text.
    pub bold: bool,
    /// Reverse video.
    pub reverse: bool,
    /// Underlined text.
    pub underline: bool,
}

/// A key the accessibility screen handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Moves focus to the next focusable element.
    Tab,
    /// Closes a modal dialog.
    Esc,
}

/// One rendered frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    _private: (),
}

impl Frame {
    /// Every row of the frame as text.
    pub fn rows(&self) -> Vec<String> {
        unimplemented!("host::a11y::Frame::rows")
    }

    /// The first row containing `needle`.
    pub fn row_containing(&self, _needle: &str) -> Option<String> {
        unimplemented!("host::a11y::Frame::row_containing")
    }

    /// The attributes of the cells that show `needle`.
    pub fn style_of(&self, _needle: &str) -> CellStyle {
        unimplemented!("host::a11y::Frame::style_of")
    }
}

/// Renders a view JSON document into a frame under the settings.
pub fn render_view(_view_json: &str, _settings: &A11ySettings) -> Frame {
    unimplemented!("host::a11y::render_view")
}

/// A live accessible screen over one view.
#[derive(Debug)]
pub struct A11yScreen {
    _private: (),
}

impl A11yScreen {
    /// Opens a screen over a view JSON document.
    pub fn open(_view_json: &str, _settings: &A11ySettings) -> A11yScreen {
        unimplemented!("host::a11y::A11yScreen::open")
    }

    /// The ids of the focusable elements in Tab order.
    pub fn focus_order(&self) -> Vec<String> {
        unimplemented!("host::a11y::A11yScreen::focus_order")
    }

    /// The id of the focused element.
    pub fn focused_id(&self) -> &str {
        unimplemented!("host::a11y::A11yScreen::focused_id")
    }

    /// Sends a key.
    pub fn press(&mut self, _key: Key) {
        unimplemented!("host::a11y::A11yScreen::press")
    }

    /// Whether the screen is closed.
    pub fn is_closed(&self) -> bool {
        unimplemented!("host::a11y::A11yScreen::is_closed")
    }

    /// The current frame.
    pub fn frame(&self) -> Frame {
        unimplemented!("host::a11y::A11yScreen::frame")
    }

    /// How many elements show a focus indicator.
    pub fn focus_indicator_count(&self) -> usize {
        unimplemented!("host::a11y::A11yScreen::focus_indicator_count")
    }

    /// The terminal cursor position (column, row).
    pub fn cursor_position(&self) -> (u16, u16) {
        unimplemented!("host::a11y::A11yScreen::cursor_position")
    }

    /// The first cell of the focused element (column, row).
    pub fn focused_cell(&self) -> (u16, u16) {
        unimplemented!("host::a11y::A11yScreen::focused_cell")
    }

    /// Advances the screen clock.
    pub fn advance(&mut self, _by: Duration) {
        unimplemented!("host::a11y::A11yScreen::advance")
    }

    /// Full-view redraws during the last second.
    pub fn redraws_in_last_second(&self) -> u32 {
        unimplemented!("host::a11y::A11yScreen::redraws_in_last_second")
    }

    /// Whether host decorative animations run.
    pub fn host_decorations_running(&self) -> bool {
        unimplemented!("host::a11y::A11yScreen::host_decorations_running")
    }

    /// The log codes written so far.
    pub fn log_codes(&self) -> Vec<String> {
        unimplemented!("host::a11y::A11yScreen::log_codes")
    }
}
