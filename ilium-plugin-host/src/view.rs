//! Static validation of plugin view documents (guide-accessibility, USR-306).

use ilium_plugin_api::PluginError;

/// Validates a view JSON document; a focusable widget without a label is refused.
pub fn validate_view(_view_json: &str) -> Result<(), PluginError> {
    unimplemented!("host::view::validate_view")
}
