//! Port twins of the core plugin `ilium.core.keys-and-help`: FakeHost twins of the original
//! ilium-client tests whose behaviour moves behind the plugin API (class adapt-to-plugin-api in
//! tools/test_port_map.tsv). One module per original file. The plugin is linked as a library and
//! driven through `FakeHost` in native mode.

mod common;
mod help_rs;
mod keyboard_ui_rs;
mod keymap_rs;
mod keys_rs;
mod settings_help_rs;
mod value_keyboard_rs;
