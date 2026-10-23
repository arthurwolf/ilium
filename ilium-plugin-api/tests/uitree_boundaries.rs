use ilium_plugin_api::generated::types::{NodeFlags, UiNode, UiTree, Widget};
use ilium_plugin_api::tier::Tier;
use ilium_plugin_api::uitree::{UiCaps, check_ui_tree, check_ui_tree_encoded};

const CAPS: UiCaps = UiCaps {
    max_nodes: 4096,
    max_kib: 1024,
    max_depth: 32,
};
fn chain(count: u32) -> UiTree {
    UiTree {
        version: 1,
        root: 0,
        nodes: (0..count)
            .map(|index| UiNode {
                key: None,
                widget: Widget::Clear,
                children: if index + 1 < count {
                    vec![index + 1]
                } else {
                    vec![]
                },
                flags: NodeFlags(0),
                tooltip: None,
                tab_index: None,
                disabled_reason: None,
            })
            .collect(),
    }
}
fn code(tree: &UiTree) -> String {
    check_ui_tree(tree, CAPS).unwrap_err().as_str().to_owned()
}
#[test]
fn depth_counts_root_and_includes_hidden_nodes() {
    assert!(check_ui_tree(&chain(32), CAPS).is_ok());
    assert_eq!(code(&chain(33)), "ui.too-deep");
    assert_eq!(code(&chain(0)), "ui.invalid-tree");
}
#[test]
fn disconnected_and_repeated_edges_are_rejected() {
    let mut tree = chain(3);
    tree.nodes[0].children.clear();
    assert_eq!(code(&tree), "ui.invalid-tree");
    tree.nodes[0].children = vec![1, 1];
    assert_eq!(code(&tree), "ui.invalid-tree");
}
#[test]
fn keys_are_global_and_count_unicode_scalars() {
    let mut tree = chain(2);
    tree.nodes[0].key = Some("é".repeat(64));
    assert!(check_ui_tree(&tree, CAPS).is_ok());
    tree.nodes[1].key = tree.nodes[0].key.clone();
    assert_eq!(code(&tree), "ui.duplicate-key");
    tree.nodes[1].key = Some("é".repeat(65));
    assert_eq!(code(&tree), "ui.invalid-tree");
}
#[test]
fn actual_encoded_byte_cap_is_inclusive() {
    let tree = chain(1);
    let caps = UiCaps { max_kib: 1, ..CAPS };
    assert!(check_ui_tree_encoded(&tree, caps, Tier::T2, 1024).is_ok());
    assert_eq!(
        check_ui_tree_encoded(&tree, caps, Tier::T2, 1025)
            .unwrap_err()
            .as_str(),
        "ui.too-large"
    );
}
#[test]
fn effective_node_cap_is_inclusive_without_reducing_capacity() {
    let mut tree = chain(4096);
    for node in &mut tree.nodes {
        node.children.clear();
    }
    tree.nodes[0].children = (1..4096).collect();
    assert!(check_ui_tree(&tree, CAPS).is_ok());
    let caps = UiCaps {
        max_nodes: 4095,
        ..CAPS
    };
    assert_eq!(
        check_ui_tree(&tree, caps).unwrap_err().as_str(),
        "ui.too-large"
    );
}
#[test]
fn mirror_encoding_counts_payloads_without_allocating_a_second_tree() {
    let mut tree = chain(1);
    tree.nodes[0].disabled_reason = Some("x".repeat(2048));
    assert_eq!(
        check_ui_tree(&tree, UiCaps { max_kib: 1, ..CAPS })
            .unwrap_err()
            .as_str(),
        "ui.too-large"
    );
}

#[test]
fn list_editor_limit_uses_actual_tier_instead_of_tree_capacity() {
    use ilium_plugin_api::generated::types::ListEditorSpec;
    let mut tree = chain(1);
    tree.nodes[0].key = Some("items".into());
    tree.nodes[0].widget = Widget::ListEditor(ListEditorSpec {
        items: vec![String::new(); 65],
        item_format: None,
        object_fields: vec![],
        reorderable: false,
        max_items: 1024,
        item_states: vec![],
        placeholder: None,
    });
    assert!(check_ui_tree_encoded(&tree, CAPS, Tier::T1, 1024).is_ok());
    assert_eq!(
        check_ui_tree_encoded(&tree, CAPS, Tier::T2, 1024)
            .unwrap_err()
            .as_str(),
        "ui.invalid-tree"
    );
    if let Widget::ListEditor(spec) = &mut tree.nodes[0].widget {
        spec.items.resize(1025, String::new());
    }
    assert_eq!(
        check_ui_tree_encoded(&tree, CAPS, Tier::T1, 1024)
            .unwrap_err()
            .as_str(),
        "ui.invalid-tree"
    );
}

#[test]
fn caller_cannot_raise_fixed_depth_ceiling() {
    let caps = UiCaps {
        max_depth: 100,
        ..CAPS
    };
    assert_eq!(
        check_ui_tree(&chain(33), caps).unwrap_err().as_str(),
        "ui.too-deep"
    );
}
