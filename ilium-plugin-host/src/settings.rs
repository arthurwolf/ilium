//! Plugin settings schema validation and the kernel settings tabs (reference-settings.md).

use ilium_plugin_api::manifest::Diagnostic;

/// The number of frozen kernel settings tabs (reference-settings.md).
pub const KERNEL_TAB_COUNT: usize = 25;

/// The ids of the frozen kernel settings tabs, in display order (reference-settings.md).
pub fn kernel_tab_ids() -> &'static [&'static str] {
    unimplemented!("host::settings::kernel_tab_ids")
}

/// Validates a settings value against a plugin's declared schema.
pub fn validate_settings(_schema_json: &str, _value_json: &str) -> Result<(), Vec<Diagnostic>> {
    unimplemented!("host::settings::validate_settings")
}

/// Whether a plugin may write a kernel settings tab (`settings.write-kernel:<tab>`).
pub fn may_write_kernel_tab(_granted: &[ilium_plugin_api::grants::Grant], _tab: &str) -> bool {
    unimplemented!("host::settings::may_write_kernel_tab")
}
