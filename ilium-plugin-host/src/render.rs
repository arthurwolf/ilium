//! UI tree caps, retained-tree diffing and contribution replication (reference-ui.md).

use ilium_plugin_api::generated::types::ui::UiTree;

/// What changed between two trees (reference-ui.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiDelta {
    /// JSON pointers of the changed nodes.
    pub changed_paths: Vec<String>,
}

/// Diffs a retained tree against the next one.
pub fn diff(previous: &UiTree, next: &UiTree) -> UiDelta {
    unimplemented!("host::render::diff")
}

/// Strips control and bidi characters from plugin text and cuts it to `limit` characters
/// (reference-extension-security.md SEC-380).
pub fn sanitize_plugin_text(text: &str, limit: usize) -> String {
    unimplemented!("host::render::sanitize_plugin_text")
}
