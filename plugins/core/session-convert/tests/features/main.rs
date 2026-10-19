//! Feature tests of the core plugin `ilium.core.session-convert` (manual page core-plugin-session-convert.md).
//!
//! Feature F-077 (convert a session between Claude and Codex), the manifest, the menu, the freeze and
//! journal flow, the Codex to Claude job, the atomic transcript write, the private app-server, the
//! replacement of the frozen pane, the progress dialog, the extension points and the service.

mod app_server;
mod common;
mod dialog;
mod doc_page;
mod extension_points;
mod finish;
mod grants;
mod job;
mod manifest;
mod menu;
mod service;
mod start;
mod write_verify;
