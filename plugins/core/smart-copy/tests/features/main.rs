//! Feature tests of the core plugin `ilium.core.smart-copy` (manual page core-plugin-smart-copy.md).
//!
//! Features F-016 (text selection and the terminal right-click menu), F-065 (Smart Copy light) and
//! F-066 (Smart Copy AI), the page samples, the manifest, the extension points and the topic.

mod command_freeze;
mod common;
mod doc_page;
mod hooks_topic;
#[path = "../host_ext.rs"]
mod host_ext;
mod light_candidates;
mod light_claims;
mod manifest;
mod menu;
mod picker_ai;
mod picker_render;
mod selection;
mod settings;
