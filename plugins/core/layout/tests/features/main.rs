//! Feature tests of the Layout core plugin (`ilium.core.layout`, manual page core-plugin-layout.md).
//! Lane core-tree-term, part features.

mod common;
mod extension_points;
mod follow_focus;
mod manifest;
mod preview_render;
mod resize;
mod split_client;
#[path = "../shared/split_host.rs"]
mod split_host;
mod split_server;
