//! Shared helpers of the port twins: a server host and the sort-key order of the tree.

use std::path::PathBuf;

use ilium_plugin_core_tree::{TreeClient, TreeServer};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{Intent, NodeClass, NodeId, Submission};

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(path)
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<TreeServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(path)
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<TreeClient>();
    host
}

/// Adds a node to the fake tree and returns its id.
pub fn add_node(host: &mut FakeHost, parent: Option<u64>, name: &str, class: NodeClass) -> u64 {
    let content_kind = match class {
        NodeClass::Pane => Some("ilium.terminal".to_string()),
        _ => None,
    };
    host.tree().add(TreeNode {
        id: 0,
        parent,
        name: name.to_string(),
        class,
        content_kind,
        agent: None,
    })
}

/// `(node, sort_key)` of every `order` decoration, in submission order.
pub fn order_keys(submissions: &[Submission]) -> Vec<(NodeId, Option<i64>)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SetDecoration(spec) if spec.slot == "order" => Some((spec.node, spec.sort_key)),
            _ => None,
        })
        .collect()
}

/// Node ids ordered the way a kernel sorting by the submitted keys does: key ascending, ties in creation (id) order.
pub fn sorted_by_key(submissions: &[Submission], ids: &[u64]) -> Vec<u64> {
    let keys = order_keys(submissions);
    let key_of = |id: u64| {
        keys.iter()
            .rev()
            .find(|(node, _)| *node == NodeId(id))
            .and_then(|(_, key)| *key)
            .expect("a key for the node")
    };
    let mut ordered: Vec<u64> = ids.to_vec();
    ordered.sort_by_key(|id| (key_of(*id), *id));
    ordered
}
