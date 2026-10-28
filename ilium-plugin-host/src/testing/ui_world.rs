//! The screen model behind the harness's security dialog and chrome methods
//! (reference-extension-security.md SEC-377..SEC-380): kernel dialogs drawn above plugin rows,
//! dialogs queued while the user types, plugin chrome that always carries the display name, and
//! plugin text sanitised by the production `render::sanitize_plugin_text`.

use crate::render::sanitize_plugin_text;
use ilium_plugin_api::{ErrorCode, PluginError};

/// Longest text a plugin may show in one field (SEC-380).
const TEXT_LIMIT: usize = 120;

/// Mutable screen state of one harness.
#[derive(Debug, Default)]
pub struct UiWorld {
    typing_in_terminal: bool,
    visible: Vec<String>,
    queued: Vec<String>,
    dialog_keys: String,
    plugin_rows: Vec<String>,
    status_segment: String,
    safe_mode: bool,
    pane_echo_off: bool,
    pane_lines: Vec<String>,
    screen_changed: Vec<String>,
}

fn strip_box_drawing(text: &str) -> String {
    text.chars().filter(|c| !('\u{2500}'..='\u{257f}').contains(c)).collect()
}

impl UiWorld {
    /// A world where the user is not typing and no dialog is open.
    pub fn new() -> UiWorld {
        UiWorld::default()
    }

    /// Opens a dialog, or queues it while the user types in a terminal pane.
    pub fn open_dialog(&mut self, kind: &str) {
        if self.typing_in_terminal {
            if !self.queued.iter().any(|queued| queued == kind) {
                self.queued.push(kind.to_owned());
            }
        } else if !self.visible.iter().any(|shown| shown == kind) {
            self.visible.push(kind.to_owned());
        }
    }

    /// Records whether the user types; leaving the pane opens the queued dialogs.
    pub fn set_typing(&mut self, typing: bool) {
        self.typing_in_terminal = typing;
        if !typing {
            self.flush_queue();
        }
    }

    fn flush_queue(&mut self) {
        let queued = std::mem::take(&mut self.queued);
        for kind in queued {
            if !self.visible.contains(&kind) {
                self.visible.push(kind);
            }
        }
    }

    /// The dialog key opens queued dialogs; the key itself is never delivered to a dialog.
    pub fn press_dialog_key(&mut self) {
        self.flush_queue();
    }

    /// Keys typed by the user reach a dialog only when one was visible before they were typed.
    pub fn type_keys(&mut self, keys: &str) {
        if !self.typing_in_terminal && !self.visible.is_empty() {
            self.dialog_keys.push_str(keys);
        }
    }

    pub fn dialog_keys(&self) -> String {
        self.dialog_keys.clone()
    }

    pub fn visible(&self) -> Vec<String> {
        self.visible.clone()
    }

    pub fn queued(&self) -> Vec<String> {
        self.queued.clone()
    }

    /// Kernel dialogs are always above plugin rows.
    pub fn is_topmost(&self, kind: &str) -> bool {
        self.visible.last().is_some_and(|top| top == kind)
    }

    /// A plugin draws rows; while the user types they are refused as possible spoofing of a
    /// dialog the kernel is holding back.
    pub fn draw_rows(&mut self, rows: &[&str]) -> Result<(), PluginError> {
        if self.typing_in_terminal && rows.iter().any(|row| row.contains(['\u{256d}', '\u{2570}'])) {
            return Err(PluginError {
                code: ErrorCode::NotPermitted,
                message: "a plugin may not draw a dialog frame".into(),
                retry_after_ms: None,
            });
        }
        self.plugin_rows = rows.iter().map(|row| strip_box_drawing(&sanitize_plugin_text(row, 400))).collect();
        Ok(())
    }

    pub fn set_status_segment(&mut self, text: &str) {
        self.status_segment = sanitize_plugin_text(text, TEXT_LIMIT);
    }

    pub fn set_safe_mode(&mut self, safe: bool) {
        self.safe_mode = safe;
    }

    /// Kernel badges first, so a long plugin segment can never push them out.
    pub fn status_bar_text(&self) -> String {
        let badge = if self.safe_mode { "SAFE " } else { "" };
        format!("{badge}{}", self.status_segment)
    }

    pub fn dialog_lines(&self, kind: &str, columns: u16) -> Vec<String> {
        let width = usize::from(columns).max(12);
        let inner = width - 2;
        let title = format!(" Ilium security: {kind} ");
        let body = [format!("Ilium asks you to confirm the {kind} step."), "Enter accepts, Esc cancels.".to_owned()];
        let fit = |text: &str| -> String {
            let mut cells: Vec<char> = text.chars().take(inner.saturating_sub(2)).collect();
            while cells.len() < inner.saturating_sub(2) {
                cells.push(' ');
            }
            format!("\u{2502} {} \u{2502}", cells.into_iter().collect::<String>())
        };
        let mut top: Vec<char> = title.chars().take(inner).collect();
        while top.len() < inner {
            top.push('\u{2500}');
        }
        let mut lines = vec![format!("\u{256d}{}\u{256e}", top.into_iter().collect::<String>())];
        lines.extend(body.iter().map(|line| fit(line)));
        lines.push(format!("\u{2570}{}\u{256f}", "\u{2500}".repeat(inner)));
        lines
    }

    pub fn screen_reader_text(&self, kind: &str) -> String {
        format!("Ilium security dialog, {kind}. Enter accepts, Escape cancels.")
    }

    /// Plugin rows first, then every visible dialog on top.
    pub fn composited(&self) -> Vec<String> {
        let mut screen = self.plugin_rows.clone();
        for kind in &self.visible {
            screen.extend(self.dialog_lines(kind, 48));
        }
        screen
    }

    /// What a plugin capture returns: plugin and pane content, never dialog text.
    pub fn plugin_capture(&self) -> Vec<String> {
        let mut capture = self.plugin_rows.clone();
        capture.extend(self.pane_lines.iter().cloned());
        capture
    }

    pub fn display_chrome(&self, name: &str, text: &str) -> String {
        let cleaned = strip_box_drawing(&sanitize_plugin_text(text, TEXT_LIMIT));
        format!("{name}: {cleaned}")
    }

    pub fn display_text(&self, text: &str) -> String {
        strip_box_drawing(&sanitize_plugin_text(text, TEXT_LIMIT))
    }

    /// Pane output reaches the plugin as a `screen-changed` event carrying no dialog text; with
    /// echo off (password prompt) the event carries no content at all.
    pub fn pane_output(&mut self, text: &str) {
        if self.pane_echo_off {
            self.screen_changed.push(String::new());
            return;
        }
        self.pane_lines.extend(text.lines().map(str::to_owned));
        self.screen_changed.push(text.to_owned());
    }

    pub fn set_pane_echo(&mut self, echo: bool) {
        self.pane_echo_off = !echo;
    }

    pub fn screen_changed_payloads(&self) -> Vec<String> {
        self.screen_changed.clone()
    }
}
