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
    let segments: Vec<&str> = text.split('.').collect();
    let valid = text.len() <= PLUGIN_ID_MAX_CHARS
        && PLUGIN_ID_SEGMENTS.contains(&segments.len())
        && segments.iter().all(|segment| {
            (1..=32).contains(&segment.len())
                && !segment.starts_with('-')
                && !segment.ends_with('-')
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
    if !valid {
        return Err(Diagnostic::error(crate::error::Code::from_static("manifest.id-invalid"), "id", "plugin id must have 2 to 8 lower-case dot-separated segments of 1 to 32 characters, at most 128 characters total"));
    }
    // Reserved namespaces depend on release/tier admission, not identifier syntax.
    Ok(PluginId(text.to_owned()))
}
