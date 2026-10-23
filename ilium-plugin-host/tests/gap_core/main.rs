//! Lane `core-gap`: requirement tests for the core-plugin rows of the core parity register
//! (COR-091..110, 200..230). One module per manual page; each test is tagged `/// REQ: <ID>`.
//!
//! Modules: reference_kernel_and_boot (COR-091, 092, 107, 108, 110, 200, 207),
//! reference_config_and_data_migration (COR-096), core_plugins (COR-097, 098, 099, 200, 203, 204,
//! 205, 208, 209, 210, 213, 220, 221, 222, 223, 224, 225, 226, 228, 229, 230),
//! reference_persistence_and_restart (COR-100, 101, 102), roadmap (COR-103, 104),
//! core_plugin_voice (COR-105, 106), core_plugin_sessions (COR-214), core_plugin_settings
//! (COR-215), core_plugin_tree (COR-216), core_plugin_layout (COR-217), core_plugin_terminal
//! (COR-218), core_plugin_agent_detect (COR-219).
//!
//! The tests drive the `ilium_plugin_host::testing::ops` rig. Run with `--features testing`.

mod core_plugin_agent_detect;
mod core_plugin_layout;
mod core_plugin_sessions;
mod core_plugin_settings;
mod core_plugin_terminal;
mod core_plugin_tree;
mod core_plugin_voice;
mod core_plugins;
mod reference_config_and_data_migration;
mod reference_kernel_and_boot;
mod reference_persistence_and_restart;
mod roadmap;
