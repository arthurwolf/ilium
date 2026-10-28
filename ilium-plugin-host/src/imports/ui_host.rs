//! Host implementation of `ilium:host/ui-host` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostUiHost` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::ui_host::HostUiHost;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ui::{
    Density, IconMode, PixelFormat, PopupAnchor, PopupOptions, ThemeExtras, ThemeTokens,
    ViewTarget, WidgetState,
};
use std::collections::{BTreeMap, BTreeSet};

/// Pixel rows of one surface node.
#[derive(Debug, Default)]
struct SurfaceBuffer {
    row_bytes: usize,
    height: u32,
    rows: Vec<u8>,
}

/// Maximum open popups in one view (WID popup rule).
const MAX_OPEN_POPUPS: usize = 4;

/// Host side of the `ui-host` interface.
///
/// The committed tree is owned by the renderer; this import holds the declared popup, panel,
/// widget-state and surface keys the renderer registers and answers calls against them.
#[derive(Debug, Default)]
pub struct UiHostImport {
    panels: BTreeSet<String>,
    open_panels: BTreeSet<String>,
    popups: BTreeSet<String>,
    open_popups: BTreeSet<String>,
    widget_states: BTreeMap<(String, String), WidgetState>,
    surfaces: BTreeMap<(String, String), SurfaceBuffer>,
    invalidated: Vec<ViewTarget>,
    theme_tokens: ThemeTokens,
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

fn view_key(view: &ViewTarget) -> String {
    format!("{view:?}")
}

impl UiHostImport {
    /// Registers a panel the committed tree declares.
    pub fn declare_panel(&mut self, panel: &str) {
        self.panels.insert(panel.to_owned());
    }

    /// Registers a popup node of the committed tree.
    pub fn declare_popup(&mut self, popup: &str) {
        self.popups.insert(popup.to_owned());
    }

    /// Registers a widget key with its initial state.
    pub fn declare_widget(&mut self, view: &ViewTarget, key: &str, state: WidgetState) {
        self.widget_states
            .insert((view_key(view), key.to_owned()), state);
    }

    /// Registers a surface node of `row_bytes` per row and `height` rows.
    pub fn declare_surface(&mut self, view: &ViewTarget, key: &str, row_bytes: usize, height: u32) {
        self.surfaces.insert(
            (view_key(view), key.to_owned()),
            SurfaceBuffer {
                row_bytes,
                height,
                rows: vec![0; row_bytes.saturating_mul(height as usize)],
            },
        );
    }

    /// Replaces the theme the plugin sees.
    pub fn set_theme(&mut self, tokens: ThemeTokens) {
        self.theme_tokens = tokens;
    }

    /// Views invalidated since the renderer last drained them.
    pub fn take_invalidated(&mut self) -> Vec<ViewTarget> {
        std::mem::take(&mut self.invalidated)
    }

    fn open_popup_checked(&mut self, popup: String) -> Result<(), PluginError> {
        if !self.popups.contains(&popup) {
            return Err(error(ErrorCode::NotFound, "no popup node with that key"));
        }
        if self.open_popups.contains(&popup) {
            return Ok(());
        }
        if self.open_popups.len() >= MAX_OPEN_POPUPS {
            return Err(error(
                ErrorCode::QuotaExceeded,
                "at most four popups may be open in one view",
            ));
        }
        self.open_popups.insert(popup);
        Ok(())
    }
}

/// Terminal cell width of one scalar value; combining marks and joiners take none.
fn char_cells(c: char) -> usize {
    let cp = c as u32;
    match cp {
        0x0300..=0x036F
        | 0x0483..=0x0489
        | 0x0591..=0x05BD
        | 0x0610..=0x061A
        | 0x064B..=0x065F
        | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF
        | 0x200B..=0x200F
        | 0x20D0..=0x20FF
        | 0xFE00..=0xFE0F
        | 0xFE20..=0xFE2F
        | 0xE0100..=0xE01EF => 0,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD => 2,
        _ if c.is_control() => 0,
        _ => 1,
    }
}

impl HostUiHost for UiHostImport {
    fn invalidate(&mut self, view: ViewTarget) {
        if !self.invalidated.contains(&view) {
            self.invalidated.push(view);
        }
    }

    fn open_panel(&mut self, panel: String) -> Result<(), PluginError> {
        if !self.panels.contains(&panel) {
            return Err(error(ErrorCode::NotFound, "no panel with that name"));
        }
        self.open_panels.insert(panel);
        Ok(())
    }

    fn close_panel(&mut self, panel: String) {
        self.open_panels.remove(&panel);
    }

    fn open_popup(&mut self, popup: String, _anchor: PopupAnchor) -> Result<(), PluginError> {
        self.open_popup_checked(popup)
    }

    fn open_popup_with(
        &mut self,
        popup: String,
        _anchor: PopupAnchor,
        _options: PopupOptions,
    ) -> Result<(), PluginError> {
        self.open_popup_checked(popup)
    }

    fn close_popup(&mut self, popup: String) {
        self.open_popups.remove(&popup);
    }

    fn widget_state(&mut self, view: ViewTarget, key: String) -> Option<WidgetState> {
        self.widget_states.get(&(view_key(&view), key)).cloned()
    }

    fn set_widget_state(
        &mut self,
        view: ViewTarget,
        key: String,
        state: WidgetState,
    ) -> Result<(), PluginError> {
        let Some(current) = self.widget_states.get_mut(&(view_key(&view), key)) else {
            return Err(error(ErrorCode::NotFound, "no widget with that key"));
        };
        if std::mem::discriminant(current) != std::mem::discriminant(&state) {
            return Err(error(
                ErrorCode::InvalidArgument,
                "the state case does not fit the widget",
            ));
        }
        *current = state;
        Ok(())
    }

    fn display_width(&mut self, text: String) -> u16 {
        let cells: usize = text.chars().map(char_cells).sum();
        u16::try_from(cells).unwrap_or(u16::MAX)
    }

    fn theme(&mut self) -> ThemeTokens {
        self.theme_tokens.clone()
    }

    fn theme_extras(&mut self) -> ThemeExtras {
        ThemeExtras {
            reduced_motion: false,
            density: Density::Normal,
            info: self.theme_tokens.accent,
            selection: self.theme_tokens.accent,
            border: self.theme_tokens.muted,
            icon_mode: if self.theme_tokens.stable_glyphs {
                IconMode::Stable
            } else {
                IconMode::Normal
            },
        }
    }

    fn write_surface(
        &mut self,
        view: ViewTarget,
        key: String,
        first_row: u32,
        _format: PixelFormat,
        data: Vec<u8>,
    ) -> Result<(), PluginError> {
        let Some(surface) = self.surfaces.get_mut(&(view_key(&view), key)) else {
            return Err(error(ErrorCode::NotFound, "no surface with that key"));
        };
        if surface.row_bytes == 0 || data.len() % surface.row_bytes != 0 {
            return Err(error(
                ErrorCode::InvalidArgument,
                "data length is not a whole number of rows",
            ));
        }
        if first_row >= surface.height {
            return Ok(());
        }
        let start = first_row as usize * surface.row_bytes;
        let writable = surface.rows.len() - start;
        let count = data.len().min(writable);
        surface.rows[start..start + count].copy_from_slice(&data[..count]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::UiHostImport;
    use ilium_plugin_api::generated::host::ui_host::HostUiHost;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ui::{
        PopupAnchor, PopupOptions, ViewTarget, WidgetState,
    };

    fn default_popup_options() -> PopupOptions {
        PopupOptions {
            modal: false,
            pinnable: true,
            dismiss_on_leave: true,
            close_on_escape: true,
        }
    }

    fn assert_error_code(result: Result<(), PluginError>, expected_code: ErrorCode) {
        match result {
            Err(error) => assert_eq!(error.code, expected_code),
            Ok(()) => panic!("expected error {expected_code:?}, got Ok(())"),
        }
    }

    /// REQ: HST-251
    #[test]
    fn invalidate_panel_view_completes_without_error_code() {
        let mut host = UiHostImport::default();
        host.invalidate(ViewTarget::Panel("notes".into()));
    }

    /// REQ: HST-495
    #[test]
    fn open_popup_unknown_name_returns_not_found() {
        let mut host = UiHostImport::default();
        assert_error_code(
            host.open_popup("missing-popup".into(), PopupAnchor::BelowKey("help".into())),
            ErrorCode::NotFound,
        );
    }

    /// REQ: HST-495
    #[test]
    fn open_popup_empty_name_returns_not_found() {
        let mut host = UiHostImport::default();
        assert_error_code(
            host.open_popup(String::new(), PopupAnchor::AtCell((0, 0))),
            ErrorCode::NotFound,
        );
    }

    /// REQ: HST-495
    #[test]
    fn open_popup_with_unknown_name_and_options_returns_not_found() {
        let mut host = UiHostImport::default();
        assert_error_code(
            host.open_popup_with(
                "missing-popup".into(),
                PopupAnchor::BelowKey("row-3".into()),
                default_popup_options(),
            ),
            ErrorCode::NotFound,
        );
    }

    /// REQ: WID-029
    #[test]
    fn close_popup_unopened_id_is_noop() {
        let mut host = UiHostImport::default();
        host.close_popup("never-opened".into());
    }

    /// REQ: WID-029
    #[test]
    fn close_popup_empty_id_is_noop() {
        let mut host = UiHostImport::default();
        host.close_popup(String::new());
    }

    /// REQ: WID-009
    #[test]
    fn widget_state_unknown_key_returns_none() {
        let mut host = UiHostImport::default();
        let state = host.widget_state(ViewTarget::Panel("notes".into()), "no-such-key".into());
        assert_eq!(state, None);
    }

    /// REQ: WID-009
    #[test]
    fn set_widget_state_unknown_key_returns_not_found() {
        let mut host = UiHostImport::default();
        assert_error_code(
            host.set_widget_state(
                ViewTarget::Panel("notes".into()),
                "no-such-key".into(),
                WidgetState::Selected(Some(0)),
            ),
            ErrorCode::NotFound,
        );
    }

    /// REQ: WID-019
    #[test]
    fn display_width_empty_string_is_zero_cells() {
        let mut host = UiHostImport::default();
        assert_eq!(host.display_width(String::new()), 0);
    }

    /// REQ: WID-019
    #[test]
    fn display_width_ascii_counts_one_cell_per_character() {
        let mut host = UiHostImport::default();
        assert_eq!(host.display_width("abc".into()), 3);
    }

    /// REQ: WID-019
    #[test]
    fn display_width_three_wide_glyphs_is_six_cells() {
        let mut host = UiHostImport::default();
        assert_eq!(host.display_width("漢字語".into()), 6);
    }

    /// REQ: WID-019
    #[test]
    fn display_width_combining_mark_adds_no_cells() {
        let mut host = UiHostImport::default();
        assert_eq!(host.display_width("e\u{0301}".into()), 1);
    }
}
