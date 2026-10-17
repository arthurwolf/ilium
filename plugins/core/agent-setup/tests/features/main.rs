//! Feature tests of the core plugin `ilium.core.agent-setup` (manual page core-plugin-agent-setup.md).
//!
//! Features F-037 (configure agent, agent config files) and F-063 (agent setup: hooks and instruction blocks),
//! the cross-cutting rules COR-011 (the plugin owns the Setup tab and the files), COR-050 (exact manifest),
//! COR-030 / COR-033 / COR-022 (kernel tab `setup`), the service `install`, the three extension points,
//! the block markers and the five statuses, the safety rules, the refresh timing, the Setup tab renders.

mod commands;
mod common;
mod doc_page;
mod extension_points;
#[path = "../host_ext.rs"]
mod host_ext;
mod manifest;
mod markers;
mod safety;
mod service;
mod settings;
mod setup_tab;
mod status;
mod timing;
