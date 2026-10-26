//! Size checks of a `ui-tree` returned by `views.render` (reference-errors-and-limits.md
//! "Requestable limits" `ui_tree_nodes`, `ui_tree_kib`; reference-wit.md section 4.10).

use crate::error::Code;
use crate::generated::types::{SpinnerFrames, UiTree, Widget};
use crate::tier::Tier;
use std::collections::BTreeSet;
use std::io::{self, Write};

/// The caps a rendered tree must fit (reference-errors-and-limits.md "Requestable limits").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiCaps {
    /// Most nodes, the `ui_tree_nodes` limit.
    pub max_nodes: u32,
    /// Most encoded KiB, the `ui_tree_kib` limit.
    pub max_kib: u32,
    /// Deepest nesting, counting the root as one (normally 32; reference-wit section 4.10).
    pub max_depth: u32,
}

/// Checks a tree using its JSON mirror encoding and the third-party widget limits.
/// Returned `ui.*` detail codes must be wrapped in `trap.invalid-ui-tree` by the host.
/// Component callers must use [`check_ui_tree_encoded`] with the actual encoded size;
/// JSON mirror size is not the component canonical ABI allocation size.
pub fn check_ui_tree(tree: &UiTree, caps: UiCaps) -> Result<(), Code> {
    let mut counter = EncodingCounter {
        bytes: 0,
        limit: u64::from(caps.max_kib) * 1024,
    };
    // Streaming avoids allocating another copy of an untrusted, possibly huge tree.
    serde_json::to_writer(&mut counter, tree).map_err(|_| detail("ui.too-large"))?;
    check_ui_tree_encoded(tree, caps, Tier::T3, counter.bytes)
}

/// Validates the structural and fixed widget limits, using the caller's real wire-size
/// measurement and package tier. Target-specific widget eligibility and render deadlines
/// belong to the host, which has the view target and invocation context.
pub fn check_ui_tree_encoded(
    tree: &UiTree,
    caps: UiCaps,
    tier: Tier,
    encoded_bytes: u64,
) -> Result<(), Code> {
    if tree.nodes.len() as u64 > u64::from(caps.max_nodes)
        || encoded_bytes > u64::from(caps.max_kib) * 1024
    {
        return Err(detail("ui.too-large"));
    }
    if tree.root != 0 || tree.nodes.is_empty() {
        return Err(detail("ui.invalid-tree"));
    }
    let mut depths = vec![0u32; tree.nodes.len()];
    depths[0] = 1;
    let mut keys = BTreeSet::new();
    for (index, node) in tree.nodes.iter().enumerate() {
        if depths[index] == 0 {
            return Err(detail("ui.invalid-tree"));
        }
        if depths[index] > caps.max_depth.min(32) {
            return Err(detail("ui.too-deep"));
        }
        if let Some(key) = &node.key {
            if key.is_empty() || key.chars().count() > 64 {
                return Err(detail("ui.invalid-tree"));
            }
            if !keys.insert(key.as_str()) {
                return Err(detail("ui.duplicate-key"));
            }
        }
        let sends_actions = matches!(
            &node.widget,
            Widget::Button(_)
                | Widget::Toggle(_)
                | Widget::Choice(_)
                | Widget::Number(_)
                | Widget::TextInput(_)
                | Widget::TextArea(_)
                | Widget::SchemaForm(_)
                | Widget::KeyCapture(_)
                | Widget::ColorPicker(_)
                | Widget::PathField(_)
                | Widget::LocationPicker(_)
                | Widget::CheckList(_)
                | Widget::ListEditor(_)
                | Widget::Menu(_)
        );
        if sends_actions && node.key.is_none() {
            return Err(detail("ui.invalid-tree"));
        }
        let invalid_widget = match &node.widget {
            Widget::Layout(spec) => spec.constraints.len() != node.children.len(),
            Widget::CheckList(spec) => spec.items.len() > 256,
            Widget::ListEditor(spec) => spec.items.len() > if tier == Tier::T1 { 1024 } else { 64 },
            Widget::Meter(spec) => spec.segments.len() > 8,
            Widget::Minimap(spec) => spec.marks.len() > 4096,
            Widget::Spinner(spec) => {
                matches!(&spec.frames, SpinnerFrames::Custom(frames) if frames.len() > 16)
            }
            _ => false,
        };
        if invalid_widget {
            return Err(detail("ui.invalid-tree"));
        }
        for &child in &node.children {
            let child = child as usize;
            if child <= index || child >= depths.len() || depths[child] != 0 {
                return Err(detail("ui.invalid-tree"));
            }
            depths[child] = depths[index].saturating_add(1);
        }
    }
    Ok(())
}

fn detail(code: &'static str) -> Code {
    Code::from_static(code)
}

struct EncodingCounter {
    bytes: u64,
    limit: u64,
}

impl Write for EncodingCounter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let bytes = self.bytes.saturating_add(buffer.len() as u64);
        if bytes > self.limit {
            return Err(io::Error::other(
                "UI tree encoding exceeds the effective limit",
            ));
        }
        self.bytes = bytes;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
