//! Shared helpers: hosts for both sides, node builders and submission readers.

use std::path::PathBuf;

use ilium_plugin_core_tree::{TreeClient, TreeServer};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{Intent, NodeClass, NodeId, Submission};

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/tree).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-tree.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<TreeServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<TreeClient>();
    host
}

/// Adds a node to the fake tree of the host.
pub fn add_node(host: &mut FakeHost, parent: Option<u64>, name: &str, class: NodeClass) -> u64 {
    let content_kind = match class {
        NodeClass::Pane => Some("ilium.terminal".to_string()),
        _ => None,
    };
    host.tree().add(TreeNode { id: 0, parent, name: name.to_string(), class, content_kind, agent: None })
}

/// The intents of a submission list, in order.
pub fn intents(submissions: &[Submission]) -> Vec<Intent> {
    submissions.iter().map(|submission| submission.intent.clone()).collect()
}

/// Node ids named by every `close-node` intent in the list.
pub fn closed_nodes(submissions: &[Submission]) -> Vec<NodeId> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::CloseNode(node) => Some(*node),
            _ => None,
        })
        .collect()
}

/// The sort key the page's "Order the tree" sample computes: the first four bytes of the lower-case title, base 256.
pub fn expected_sort_key(title: &str) -> i64 {
    title.to_lowercase().bytes().take(4).fold(0i64, |key, byte| key * 256 + i64::from(byte))
}

/// Decoration specs with slot `order`, as `(node, sort_key)` pairs in submission order.
pub fn order_keys(submissions: &[Submission]) -> Vec<(NodeId, Option<i64>)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SetDecoration(spec) if spec.slot == "order" => Some((spec.node, spec.sort_key)),
            _ => None,
        })
        .collect()
}
