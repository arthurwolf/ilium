//! Feature tests of the core plugin `ilium.core.agent-detect` (manual page core-plugin-agent-detect.md).
//!
//! Features F-026 (agent identity), F-027 (activity classification), F-028 (pane status signals),
//! F-029 (goal tracking), F-030 (display modes and custom signatures), F-034 (agent icons and identifiers),
//! the interstitial prompt rule (COR-011), the kernel tab settings, the manifest, the extension points,
//! the services and topics, and the Agents panel renders.

mod classify;
mod common;
mod decorations;
mod display_modes;
mod doc_page;
mod extension_points;
mod goal;
#[path = "../host_ext.rs"]
mod host_ext;
mod identify;
mod interstitial;
mod manifest;
mod panel;
mod settings;
mod signals;
