//! Core-plugin gap tests for `core-plugin-tree`: replacing the tree plugin closes no pane, restarts
//! no PTY, and leaves the tree identical by node id, kind, parent and order.

use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

const TREE: &str = "ilium.core.tree";
const REPLACEMENT: &str = "com.example.tree-swap";

/// REQ: COR-216
#[test]
fn cor_216_replacing_the_tree_plugin_closes_no_pane_restarts_no_pty_and_keeps_the_tree_identical() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.open_pane("main");
    server.open_pane("side");
    let tree_before = server.tree_nodes();
    let pty_restarts_before = server.pty_restarts();
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    server.enable(REPLACEMENT);
    assert_eq!(
        server.tree_nodes(),
        tree_before,
        "node id, kind, parent and order must match before and after the switch"
    );
    assert_eq!(server.pane_count(), 2, "no pane may close");
    assert_eq!(
        server.pty_restarts(),
        pty_restarts_before,
        "no PTY may restart"
    );
}

/// REQ: COR-216
#[test]
fn cor_216_tree_has_nodes_before_and_after_the_switch_so_equality_is_not_vacuous() {
    let rig = OpsRig::new();
    let server = rig.start_server("s2");
    server.open_pane("main");
    let tree_before = server.tree_nodes();
    assert!(!tree_before.is_empty(), "the tree must hold nodes to compare");
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    server.enable(REPLACEMENT);
    assert_eq!(server.tree_nodes().len(), tree_before.len());
}
