//! Feature tests of the core plugin `ilium.core.notifications` (manual page core-plugin-notifications.md).
//!
//! Features F-047 (Sound alerts), F-048 (Desktop notifications) and F-049 (Coalescing of redundant
//! task alerts), the attention marks, the page samples, the manifest, the Sound settings tab, the
//! extension points, the service and the topic. The plugin is linked as a library and driven through
//! `FakeHost` in native mode.

mod attention;
mod common;
mod dedup;
mod desktop;
mod doc_page;
mod extension_points;
mod manifest;
mod merge;
mod mute;
mod redundant;
mod render;
mod settings;
mod sounds;
