//! Feature tests of the core plugin `ilium.core.keys-and-help` (manual page
//! core-plugin-keys-and-help.md).
//!
//! Features F-007 (prefix, keybindings, presets, remap: the Keyboard tab and the key reference
//! side) and F-010 (help and key reference), the manifest, the 35 actions and two presets, the
//! key reference and Keyboard tab renders, key capture, help topics, the plugin manager, the
//! events, the extension points, the service and the topic. The plugin is linked as a library and
//! driven through `FakeHost` / `FakeSession` in native mode.

mod actions_table;
mod common;
mod doc_page;
mod events;
mod extension_points;
mod help_overlay;
mod help_topics;
mod key_capture;
mod key_reference;
mod keyboard_tab;
mod manifest;
mod plugin_manager;
mod service;
