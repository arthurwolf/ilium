//! Feature tests of the core plugin `ilium.core.progress` (manual page core-plugin-progress.md).
//!
//! Features F-059 (progress monitor), F-060 (`ilium progress`), F-061 (progress footer in the
//! tree) and F-108 (`ilium wait`), the behaviour contract progress-B01 to progress-B15, the four
//! kernel settings the plugin reads, the extension points, the service, the topic, the manifest
//! and the page itself. Lane core-automation, part features.

mod cli_exit;
mod common;
mod delivery;
mod doc_page;
mod extension_points;
mod footer_client;
#[path = "../host_ext.rs"]
mod host_ext;
mod http_route;
mod manifest;
mod probe_contract;
mod restore;
mod set_cli;
mod settings;
mod status_clear;
mod wait_alias;
mod wait_records;
mod watchers;
