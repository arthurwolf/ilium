//! Host implementation of `ilium:host/ui-host` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostUiHost` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::ui_host::HostUiHost;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ui::{
    PixelFormat, PopupAnchor, PopupOptions, ThemeExtras, ThemeTokens, ViewTarget, WidgetState,
};

/// Host side of the `ui-host` interface.
#[derive(Debug)]
pub struct UiHostImport;

impl HostUiHost for UiHostImport {
    fn invalidate(&mut self, _view: ViewTarget) {
        unimplemented!("host::imports::ui_host::invalidate")
    }

    fn open_panel(&mut self, _panel: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::ui_host::open_panel")
    }

    fn close_panel(&mut self, _panel: String) {
        unimplemented!("host::imports::ui_host::close_panel")
    }

    fn open_popup(&mut self, _popup: String, _anchor: PopupAnchor) -> Result<(), PluginError> {
        unimplemented!("host::imports::ui_host::open_popup")
    }

    fn open_popup_with(
        &mut self,
        _popup: String,
        _anchor: PopupAnchor,
        _options: PopupOptions,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::ui_host::open_popup_with")
    }

    fn close_popup(&mut self, _popup: String) {
        unimplemented!("host::imports::ui_host::close_popup")
    }

    fn widget_state(&mut self, _view: ViewTarget, _key: String) -> Option<WidgetState> {
        unimplemented!("host::imports::ui_host::widget_state")
    }

    fn set_widget_state(
        &mut self,
        _view: ViewTarget,
        _key: String,
        _state: WidgetState,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::ui_host::set_widget_state")
    }

    fn display_width(&mut self, _text: String) -> u16 {
        unimplemented!("host::imports::ui_host::display_width")
    }

    fn theme(&mut self) -> ThemeTokens {
        unimplemented!("host::imports::ui_host::theme")
    }

    fn theme_extras(&mut self) -> ThemeExtras {
        unimplemented!("host::imports::ui_host::theme_extras")
    }

    fn write_surface(
        &mut self,
        _view: ViewTarget,
        _key: String,
        _first_row: u32,
        _format: PixelFormat,
        _data: Vec<u8>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::ui_host::write_surface")
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
        let mut host = UiHostImport;
        host.invalidate(ViewTarget::Panel("notes".into()));
    }

    /// REQ: HST-495
    #[test]
    fn open_popup_unknown_name_returns_not_found() {
        let mut host = UiHostImport;
        assert_error_code(
            host.open_popup("missing-popup".into(), PopupAnchor::BelowKey("help".into())),
            ErrorCode::NotFound,
        );
    }

    /// REQ: HST-495
    #[test]
    fn open_popup_empty_name_returns_not_found() {
        let mut host = UiHostImport;
        assert_error_code(
            host.open_popup(String::new(), PopupAnchor::AtCell((0, 0))),
            ErrorCode::NotFound,
        );
    }

    /// REQ: HST-495
    #[test]
    fn open_popup_with_unknown_name_and_options_returns_not_found() {
        let mut host = UiHostImport;
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
        let mut host = UiHostImport;
        host.close_popup("never-opened".into());
    }

    /// REQ: WID-029
    #[test]
    fn close_popup_empty_id_is_noop() {
        let mut host = UiHostImport;
        host.close_popup(String::new());
    }

    /// REQ: WID-009
    #[test]
    fn widget_state_unknown_key_returns_none() {
        let mut host = UiHostImport;
        let state = host.widget_state(ViewTarget::Panel("notes".into()), "no-such-key".into());
        assert_eq!(state, None);
    }

    /// REQ: WID-009
    #[test]
    fn set_widget_state_unknown_key_returns_not_found() {
        let mut host = UiHostImport;
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
        let mut host = UiHostImport;
        assert_eq!(host.display_width(String::new()), 0);
    }

    /// REQ: WID-019
    #[test]
    fn display_width_ascii_counts_one_cell_per_character() {
        let mut host = UiHostImport;
        assert_eq!(host.display_width("abc".into()), 3);
    }

    /// REQ: WID-019
    #[test]
    fn display_width_three_wide_glyphs_is_six_cells() {
        let mut host = UiHostImport;
        assert_eq!(host.display_width("漢字語".into()), 6);
    }

    /// REQ: WID-019
    #[test]
    fn display_width_combining_mark_adds_no_cells() {
        let mut host = UiHostImport;
        assert_eq!(host.display_width("e\u{0301}".into()), 1);
    }
}
