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
    fn invalidate(&mut self, view: ViewTarget) {
        unimplemented!("host::imports::ui_host::invalidate")
    }

    fn open_panel(&mut self, panel: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::ui_host::open_panel")
    }

    fn close_panel(&mut self, panel: String) {
        unimplemented!("host::imports::ui_host::close_panel")
    }

    fn open_popup(&mut self, popup: String, anchor: PopupAnchor) -> Result<(), PluginError> {
        unimplemented!("host::imports::ui_host::open_popup")
    }

    fn open_popup_with(
        &mut self,
        popup: String,
        anchor: PopupAnchor,
        options: PopupOptions,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::ui_host::open_popup_with")
    }

    fn close_popup(&mut self, popup: String) {
        unimplemented!("host::imports::ui_host::close_popup")
    }

    fn widget_state(&mut self, view: ViewTarget, key: String) -> Option<WidgetState> {
        unimplemented!("host::imports::ui_host::widget_state")
    }

    fn set_widget_state(
        &mut self,
        view: ViewTarget,
        key: String,
        state: WidgetState,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::ui_host::set_widget_state")
    }

    fn display_width(&mut self, text: String) -> u16 {
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
        view: ViewTarget,
        key: String,
        first_row: u32,
        format: PixelFormat,
        data: Vec<u8>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::ui_host::write_surface")
    }
}
