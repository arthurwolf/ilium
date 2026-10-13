//! Id newtypes of `ilium:types/ids` and plugin id parsing (reference-wit.md section 4.1,
//! reference-manifest.md PKG-014).

pub use crate::generated::types::ids::{
    BufferId, ChildId, ClientId, EpochMs, GrabId, IndexId, LockId, NodeId, PluginId, RequestId,
    StreamId, TimerId, WatchId,
};

use crate::manifest::Diagnostic;

/// Longest plugin id in characters (reference-manifest.md PKG-014).
pub const PLUGIN_ID_MAX_CHARS: usize = 128;

/// Fewest and most `.`-separated segments of a plugin id (reference-manifest.md PKG-014).
pub const PLUGIN_ID_SEGMENTS: std::ops::RangeInclusive<usize> = 2..=8;

/// Parses and checks a plugin id; a bad id is `manifest.id-invalid` (reference-manifest.md PKG-014).
pub fn parse_plugin_id(text: &str) -> Result<PluginId, Diagnostic> {
    unimplemented!("api::ids::parse_plugin_id")
}
