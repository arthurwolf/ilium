//! Feature tests of the core plugin `ilium.core.settings` (manual page core-plugin-settings.md).
//!
//! Features F-089 (Guided setup), F-090 (Settings UI), F-086 (Debug file logging), F-091 (config
//! persistence and live mirror) and F-092 (themes, motion, density, stable glyphs) as far as the
//! page assigns them to this plugin, the manifest, the 25 tabs, the Plugins tab, the extension
//! points, the services and the topic. The plugin is linked as a library and driven through
//! `FakeHost` / `FakeSession` in native mode.

mod autosave;
mod common;
mod doc_page;
mod events;
mod extension_points;
mod manifest;
mod onboarding;
mod plugins_tab;
mod render;
mod rows;
mod tab_values;
mod tabs;
