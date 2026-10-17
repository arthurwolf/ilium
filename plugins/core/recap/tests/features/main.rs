//! Feature tests of the Recap core plugin (`ilium.core.recap`, manual page core-plugin-recap.md).
//! Lane core-services, part features.

mod cli_args;
mod cli_gate;
mod common;
mod doc_page;
mod extension_points;
#[path = "../host_ext.rs"]
mod host_ext;
mod jobs_and_service;
mod manifest;
