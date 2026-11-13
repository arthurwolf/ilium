//! Shared helpers of the port twins: server and client hosts and a node builder.

use std::path::PathBuf;

use ilium_plugin_core_tree::{TreeClient, TreeServer};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::NodeClass;

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
