//! Shared helpers of the render lane. Real types come from ilium-plugin-api; the rigs that drive
//! the host render pipeline do not exist in the skeleton yet and are listed in API_GAPS.md.

#![allow(dead_code)]

use ilium_plugin_api::generated::types::{NodeFlags, UiNode, UiTree, Widget};
use ilium_plugin_api::uitree::{check_ui_tree, UiCaps};
use ilium_plugin_api::Code;

pub use ilium_plugin_host::testing::render::{
    cell_width, frame_budget_ms, hit_layers_in_order, hit_stack_tree, install_prerender, sanitise,
    scene_memory_limit_mib, set_decoration_with_frames, tab_next, tab_order, tooltip_shown,
    tree_with_text, tree_with_tooltip, widget_families, Font, Rig, SceneRig, SurfaceRig,
};

pub const CAPS: UiCaps = UiCaps {
    max_nodes: 4096,
    max_kib: 1024,
    max_depth: 32,
};

pub fn node(key: Option<&str>, children: Vec<u32>) -> UiNode {
    UiNode {
        key: key.map(str::to_string),
        widget: Widget::Clear,
        children,
        flags: NodeFlags(0),
        tooltip: None,
        tab_index: None,
        disabled_reason: None,
    }
}

/// A line of `depth` nodes, node i the only child of node i-1 (root is level 1).
pub fn chain(depth: u32) -> UiTree {
    let nodes = (0..depth)
        .map(|i| node(None, if i + 1 < depth { vec![i + 1] } else { vec![] }))
        .collect();
    UiTree {
        version: 1,
        nodes,
        root: 0,
    }
}

/// A line with a key on every node after the root.
pub fn keyed_chain(keys: &[&str]) -> UiTree {
    let count = keys.len() as u32 + 1;
    let mut nodes = vec![node(None, vec![1])];
    for (i, key) in keys.iter().enumerate() {
        let index = i as u32 + 1;
        nodes.push(node(
            Some(key),
            if index + 1 < count {
                vec![index + 1]
            } else {
                vec![]
            },
        ));
    }
    UiTree {
        version: 1,
        nodes,
        root: 0,
    }
}

/// A root with `count - 1` direct children.
pub fn chain_flat(count: u32) -> UiTree {
    let mut nodes = vec![node(None, (1..count).collect())];
    nodes.extend((1..count).map(|_| node(None, vec![])));
    UiTree {
        version: 1,
        nodes,
        root: 0,
    }
}

/// Two branches, each with a node keyed "save".
pub fn duplicate_key_tree() -> UiTree {
    UiTree {
        version: 1,
        nodes: vec![
            node(None, vec![1, 2]),
            node(Some("left"), vec![3]),
            node(Some("right"), vec![4]),
            node(Some("save"), vec![]),
            node(Some("save"), vec![]),
        ],
        root: 0,
    }
}

pub fn check(tree: &UiTree) -> Result<(), Code> {
    check_ui_tree(tree, CAPS)
}

pub fn code_of(tree: &UiTree) -> String {
    check(tree)
        .expect_err("tree must be refused")
        .as_str()
        .to_string()
}

/// Trees that break one named rule each, with the expected trap code.
pub fn named_failure_trees() -> Vec<(UiTree, &'static str)> {
    let mut invalid = chain(3);
    invalid.root = 2;
    vec![
        (invalid, "ui.invalid-tree"),
        (duplicate_key_tree(), "ui.duplicate-key"),
        (chain(33), "ui.too-deep"),
        (chain_flat(4097), "ui.too-large"),
    ]
}

pub fn rig_t1() -> Rig {
    Rig::new("T1")
}

pub fn rig_t2() -> Rig {
    Rig::new("T2")
}

pub fn rig_scene() -> Rig {
    Rig::new("T2").with_scene()
}

pub fn rig_scene_prerendered(fps: u32) -> Rig {
    Rig::new("T2").with_prerendered_scene(fps)
}
