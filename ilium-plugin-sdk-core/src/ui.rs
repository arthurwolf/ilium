//! UI tree builder (rust-sdk.md "Building a UI tree"). Widget constructors are plain data; the
//! tree assembly and validation are stubs.

/// Widget constructors, one per `Widget` case, taking the case payload.
pub mod widgets {
    use crate::generated::types::*;

    /// Widget `layout` (rust-sdk.md "Building a UI tree").
    pub fn layout(spec: LayoutSpec) -> Widget {
        Widget::Layout(spec)
    }

    /// Widget `block` (rust-sdk.md "Building a UI tree").
    pub fn block(spec: BlockSpec) -> Widget {
        Widget::Block(spec)
    }

    /// Widget `tabs` (rust-sdk.md "Building a UI tree").
    pub fn tabs(spec: TabsSpec) -> Widget {
        Widget::Tabs(spec)
    }

    /// Widget `popup` (rust-sdk.md "Building a UI tree").
    pub fn popup(spec: PopupSpec) -> Widget {
        Widget::Popup(spec)
    }

    /// Widget `clear` (rust-sdk.md "Building a UI tree").
    pub fn clear() -> Widget {
        Widget::Clear
    }

    /// Widget `scrollbar` (rust-sdk.md "Building a UI tree").
    pub fn scrollbar(spec: ScrollbarSpec) -> Widget {
        Widget::Scrollbar(spec)
    }

    /// Widget `split` (rust-sdk.md "Building a UI tree").
    pub fn split(spec: SplitSpec) -> Widget {
        Widget::Split(spec)
    }

    /// Widget `separator` (rust-sdk.md "Building a UI tree").
    pub fn separator(spec: SeparatorSpec) -> Widget {
        Widget::Separator(spec)
    }

    /// Widget `dialog` (rust-sdk.md "Building a UI tree").
    pub fn dialog(spec: DialogSpec) -> Widget {
        Widget::Dialog(spec)
    }

    /// Widget `scroll_view` (rust-sdk.md "Building a UI tree").
    pub fn scroll_view(spec: ScrollViewSpec) -> Widget {
        Widget::ScrollView(spec)
    }

    /// Widget `paragraph` (rust-sdk.md "Building a UI tree").
    pub fn paragraph(spec: ParagraphSpec) -> Widget {
        Widget::Paragraph(spec)
    }

    /// Widget `markdown` (rust-sdk.md "Building a UI tree").
    pub fn markdown(spec: MarkdownSpec) -> Widget {
        Widget::Markdown(spec)
    }

    /// Widget `leader_row` (rust-sdk.md "Building a UI tree").
    pub fn leader_row(spec: LeaderSpec) -> Widget {
        Widget::LeaderRow(spec)
    }

    /// Widget `key_hints` (rust-sdk.md "Building a UI tree").
    pub fn key_hints(spec: Vec<KeyHint>) -> Widget {
        Widget::KeyHints(spec)
    }

    /// Widget `diff_view` (rust-sdk.md "Building a UI tree").
    pub fn diff_view(spec: DiffViewSpec) -> Widget {
        Widget::DiffView(spec)
    }

    /// Widget `list` (rust-sdk.md "Building a UI tree").
    pub fn list(spec: ListSpec) -> Widget {
        Widget::List(spec)
    }

    /// Widget `table` (rust-sdk.md "Building a UI tree").
    pub fn table(spec: TableSpec) -> Widget {
        Widget::Table(spec)
    }

    /// Widget `tree_list` (rust-sdk.md "Building a UI tree").
    pub fn tree_list(spec: TreeListSpec) -> Widget {
        Widget::TreeList(spec)
    }

    /// Widget `menu` (rust-sdk.md "Building a UI tree").
    pub fn menu(spec: MenuSpec) -> Widget {
        Widget::Menu(spec)
    }

    /// Widget `check_list` (rust-sdk.md "Building a UI tree").
    pub fn check_list(spec: CheckListSpec) -> Widget {
        Widget::CheckList(spec)
    }

    /// Widget `list_editor` (rust-sdk.md "Building a UI tree").
    pub fn list_editor(spec: ListEditorSpec) -> Widget {
        Widget::ListEditor(spec)
    }

    /// Widget `gauge` (rust-sdk.md "Building a UI tree").
    pub fn gauge(spec: GaugeSpec) -> Widget {
        Widget::Gauge(spec)
    }

    /// Widget `line_gauge` (rust-sdk.md "Building a UI tree").
    pub fn line_gauge(spec: LineGaugeSpec) -> Widget {
        Widget::LineGauge(spec)
    }

    /// Widget `sparkline` (rust-sdk.md "Building a UI tree").
    pub fn sparkline(spec: SparklineSpec) -> Widget {
        Widget::Sparkline(spec)
    }

    /// Widget `bar_chart` (rust-sdk.md "Building a UI tree").
    pub fn bar_chart(spec: BarChartSpec) -> Widget {
        Widget::BarChart(spec)
    }

    /// Widget `chart` (rust-sdk.md "Building a UI tree").
    pub fn chart(spec: ChartSpec) -> Widget {
        Widget::Chart(spec)
    }

    /// Widget `canvas` (rust-sdk.md "Building a UI tree").
    pub fn canvas(spec: CanvasSpec) -> Widget {
        Widget::Canvas(spec)
    }

    /// Widget `calendar` (rust-sdk.md "Building a UI tree").
    pub fn calendar(spec: CalendarSpec) -> Widget {
        Widget::Calendar(spec)
    }

    /// Widget `progress_row` (rust-sdk.md "Building a UI tree").
    pub fn progress_row(spec: ProgressRowSpec) -> Widget {
        Widget::ProgressRow(spec)
    }

    /// Widget `meter` (rust-sdk.md "Building a UI tree").
    pub fn meter(spec: MeterSpec) -> Widget {
        Widget::Meter(spec)
    }

    /// Widget `minimap` (rust-sdk.md "Building a UI tree").
    pub fn minimap(spec: MinimapSpec) -> Widget {
        Widget::Minimap(spec)
    }

    /// Widget `button` (rust-sdk.md "Building a UI tree").
    pub fn button(spec: ButtonSpec) -> Widget {
        Widget::Button(spec)
    }

    /// Widget `toggle` (rust-sdk.md "Building a UI tree").
    pub fn toggle(spec: ToggleSpec) -> Widget {
        Widget::Toggle(spec)
    }

    /// Widget `choice` (rust-sdk.md "Building a UI tree").
    pub fn choice(spec: ChoiceSpec) -> Widget {
        Widget::Choice(spec)
    }

    /// Widget `number` (rust-sdk.md "Building a UI tree").
    pub fn number(spec: NumberSpec) -> Widget {
        Widget::Number(spec)
    }

    /// Widget `text_input` (rust-sdk.md "Building a UI tree").
    pub fn text_input(spec: TextInputSpec) -> Widget {
        Widget::TextInput(spec)
    }

    /// Widget `text_area` (rust-sdk.md "Building a UI tree").
    pub fn text_area(spec: TextAreaSpec) -> Widget {
        Widget::TextArea(spec)
    }

    /// Widget `schema_form` (rust-sdk.md "Building a UI tree").
    pub fn schema_form(spec: SchemaFormSpec) -> Widget {
        Widget::SchemaForm(spec)
    }

    /// Widget `key_capture` (rust-sdk.md "Building a UI tree").
    pub fn key_capture(spec: KeyCaptureSpec) -> Widget {
        Widget::KeyCapture(spec)
    }

    /// Widget `color_picker` (rust-sdk.md "Building a UI tree").
    pub fn color_picker(spec: ColorPickerSpec) -> Widget {
        Widget::ColorPicker(spec)
    }

    /// Widget `path_field` (rust-sdk.md "Building a UI tree").
    pub fn path_field(spec: PathFieldSpec) -> Widget {
        Widget::PathField(spec)
    }

    /// Widget `location_picker` (rust-sdk.md "Building a UI tree").
    pub fn location_picker(spec: LocationPickerSpec) -> Widget {
        Widget::LocationPicker(spec)
    }

    /// Widget `badge` (rust-sdk.md "Building a UI tree").
    pub fn badge(spec: BadgeSpec) -> Widget {
        Widget::Badge(spec)
    }

    /// Widget `spinner` (rust-sdk.md "Building a UI tree").
    pub fn spinner(spec: SpinnerSpec) -> Widget {
        Widget::Spinner(spec)
    }

    /// Widget `notice` (rust-sdk.md "Building a UI tree").
    pub fn notice(spec: NoticeSpec) -> Widget {
        Widget::Notice(spec)
    }

    /// Widget `icon` (rust-sdk.md "Building a UI tree").
    pub fn icon(spec: IconSpec) -> Widget {
        Widget::Icon(spec)
    }

    /// Widget `image` (rust-sdk.md "Building a UI tree").
    pub fn image(spec: ImageSpec) -> Widget {
        Widget::Image(spec)
    }

    /// Widget `surface` (rust-sdk.md "Building a UI tree").
    pub fn surface(spec: SurfaceSpec) -> Widget {
        Widget::Surface(spec)
    }

    /// Widget `code_view` (rust-sdk.md "Building a UI tree").
    pub fn code_view(spec: CodeViewSpec) -> Widget {
        Widget::CodeView(spec)
    }

    /// Widget `terminal_view` (rust-sdk.md "Building a UI tree").
    pub fn terminal_view(spec: TerminalViewSpec) -> Widget {
        Widget::TerminalView(spec)
    }

    /// Widget `video` (rust-sdk.md "Building a UI tree").
    pub fn video(spec: VideoSpec) -> Widget {
        Widget::Video(spec)
    }
}

/// Tree and node builders.
pub mod build {
    use crate::generated::types::*;

    const UI_TREE_VERSION: u32 = 1;
    const MAX_TREE_NODES: usize = 4_096;
    const MAX_TREE_KIB: usize = 1_024;
    const MAX_TREE_DEPTH: usize = 32;
    const MAX_KEY_CHARS: usize = 64;
    const MAX_DISABLED_REASON_CHARS: usize = 40;

    fn invalid(code: &str, detail: &str) -> PluginError {
        PluginError::new(ErrorCode::InvalidArgument, format!("{code}: {detail}"))
    }

    /// Collects nodes and links children by index (reference-wit.md section 4.9: flat node list, root index).
    #[derive(Debug, Clone, Default)]
    pub struct Tree {
        nodes: Vec<UiNode>,
    }

    impl Tree {
        /// An empty tree.
        pub fn new() -> Tree {
            Tree { nodes: Vec::new() }
        }

        /// Adds a node for the widget and returns its builder.
        pub fn node(&mut self, widget: Widget) -> NodeBuilder<'_> {
            let index = self.nodes.len() as u32;
            self.nodes.push(UiNode {
                key: None,
                widget,
                children: Vec::new(),
                flags: NodeFlags::default(),
                tooltip: None,
                tab_index: None,
                disabled_reason: None,
            });
            NodeBuilder { tree: self, index }
        }

        /// Finishes the tree with the given root node index; checks limits (node count, depth).
        pub fn finish(self, root: u32) -> Result<UiTree, PluginError> {
            self.validate(root)?;
            let tree = UiTree {
                version: UI_TREE_VERSION,
                nodes: self.nodes,
                root,
            };
            let encoded_bytes = serde_json::to_vec(&tree).map(|bytes| bytes.len()).unwrap_or(0);
            if encoded_bytes > MAX_TREE_KIB * 1024 {
                return Err(invalid("ui.too-large", "the tree is over 1,024 KiB"));
            }
            Ok(tree)
        }

        /// Checks reference-wit.md section 4.10: root 0, forward child indexes, one parent per node,
        /// node count, depth, unique keys and the disabled-reason length.
        fn validate(&self, root: u32) -> Result<(), PluginError> {
            let node_count = self.nodes.len();
            if root != 0 || node_count == 0 {
                return Err(invalid("ui.bad-root", "the root node index must be 0"));
            }
            if node_count > MAX_TREE_NODES {
                return Err(invalid("ui.too-many-nodes", "more than 4,096 nodes"));
            }
            let mut parent_count = vec![0u32; node_count];
            let mut depth = vec![1usize; node_count];
            let mut keys = std::collections::HashSet::new();
            for (index, node) in self.nodes.iter().enumerate() {
                for &child in &node.children {
                    let child = child as usize;
                    if child <= index || child >= node_count {
                        return Err(invalid(
                            "ui.bad-child",
                            "a child index must be greater than its parent and inside the tree",
                        ));
                    }
                    parent_count[child] += 1;
                    depth[child] = depth[child].max(depth[index] + 1);
                }
                if depth[index] > MAX_TREE_DEPTH {
                    return Err(invalid("ui.too-deep", "deeper than 32 levels"));
                }
                if let Some(key) = &node.key {
                    let key_chars = key.chars().count();
                    if !(1..=MAX_KEY_CHARS).contains(&key_chars) {
                        return Err(invalid("ui.bad-key", "a key has 1 to 64 characters"));
                    }
                    if !keys.insert(key.as_str()) {
                        return Err(invalid("ui.duplicate-key", "a key is used twice in one view"));
                    }
                }
                if let Some(reason) = &node.disabled_reason {
                    if reason.chars().count() > MAX_DISABLED_REASON_CHARS {
                        return Err(invalid(
                            "ui.disabled-reason-too-long",
                            "a disabled reason has at most 40 characters",
                        ));
                    }
                }
            }
            if parent_count[0] != 0 || parent_count[1..].iter().any(|&count| count != 1) {
                return Err(invalid(
                    "ui.bad-child",
                    "every node except the root appears exactly once in one children list",
                ));
            }
            Ok(())
        }

        /// Number of nodes added so far.
        pub fn len(&self) -> usize {
            self.nodes.len()
        }

        /// True when no node was added.
        pub fn is_empty(&self) -> bool {
            self.nodes.is_empty()
        }
    }

    /// Chained setters for the optional properties of one node.
    #[derive(Debug)]
    pub struct NodeBuilder<'a> {
        tree: &'a mut Tree,
        index: u32,
    }

    impl NodeBuilder<'_> {
        /// The node this builder edits; the builder owns the only live borrow of the tree.
        fn node_mut(&mut self) -> &mut UiNode {
            &mut self.tree.nodes[self.index as usize]
        }
        /// Index of the node in the tree.
        pub fn index(&self) -> u32 {
            self.index
        }
        /// Number of nodes in the tree this builder belongs to.
        pub fn tree_len(&self) -> usize {
            self.tree.len()
        }
        /// Sets the stable key that actions refer to.
        pub fn key(mut self, key: impl Into<String>) -> Self {
            self.node_mut().key = Some(key.into());
            self
        }
        /// Sets the node flags.
        pub fn flags(mut self, flags: NodeFlags) -> Self {
            self.node_mut().flags = flags;
            self
        }
        /// Sets the tooltip.
        pub fn tooltip(mut self, tooltip: Tooltip) -> Self {
            self.node_mut().tooltip = Some(tooltip);
            self
        }
        /// Sets the tab order index.
        pub fn tab_index(mut self, tab_index: i16) -> Self {
            self.node_mut().tab_index = Some(tab_index);
            self
        }
        /// Disables the node and records why.
        pub fn disabled_reason(mut self, reason: impl Into<String>) -> Self {
            self.node_mut().disabled_reason = Some(reason.into());
            self
        }
        /// Appends a child by node index.
        pub fn child(mut self, child: u32) -> Self {
            self.node_mut().children.push(child);
            self
        }
        /// Appends several children by node index.
        pub fn children(mut self, children: &[u32]) -> Self {
            self.node_mut().children.extend_from_slice(children);
            self
        }
        /// Ends the builder and returns the node index.
        pub fn done(self) -> u32 {
            self.index
        }
    }
}

#[cfg(test)]
mod spec_tests {
    use super::build::Tree;
    use super::widgets;
    use crate::generated::types::Widget;

    /// Builds a tree of `node_count` nodes where node 0 is the root and lists every other node as a child.
    fn flat_tree_with_nodes(node_count: u32) -> Tree {
        let mut tree = Tree::new();
        let child_indices: Vec<u32> = (1..node_count).collect();
        tree.node(widgets::clear()).children(&child_indices).done();
        for _ in 1..node_count {
            tree.node(widgets::clear()).done();
        }
        tree
    }

    /// Builds a chain of `node_count` nodes, each the only child of the previous one; node 0 is the root.
    fn chain_tree(node_count: u32) -> Tree {
        let mut tree = Tree::new();
        for index in 0..node_count {
            let builder = tree.node(widgets::clear());
            let builder = if index + 1 < node_count {
                builder.child(index + 1)
            } else {
                builder
            };
            builder.done();
        }
        tree
    }

    #[test]
    fn finished_tree_has_root_zero_and_forward_child_indices() {
        let mut tree = Tree::new();
        tree.node(widgets::clear()).child(1).done();
        tree.node(widgets::key_hints(Vec::new())).done();
        let built = tree
            .finish(0)
            .expect("a root-zero tree with a forward child is valid");
        assert_eq!(built.root, 0);
        assert_eq!(built.nodes[0].widget, Widget::Clear);
        assert_eq!(built.nodes[0].children, vec![1]);
        assert!(built.nodes[1].children.is_empty());
    }

    #[test]
    fn child_index_past_the_last_node_is_rejected() {
        let mut tree = Tree::new();
        tree.node(widgets::clear()).child(99).done();
        assert!(tree.finish(0).is_err());
    }

    #[test]
    fn backward_child_index_is_rejected() {
        let mut tree = Tree::new();
        tree.node(widgets::clear()).done();
        tree.node(widgets::clear()).child(0).done();
        assert!(tree.finish(0).is_err());
    }

    #[test]
    fn nonzero_root_is_rejected() {
        let mut tree = Tree::new();
        tree.node(widgets::clear()).done();
        tree.node(widgets::clear()).done();
        assert!(tree.finish(1).is_err());
    }

    #[test]
    fn tree_at_the_node_limit_is_accepted() {
        assert!(flat_tree_with_nodes(4_096).finish(0).is_ok());
    }

    #[test]
    fn tree_over_the_node_limit_is_rejected() {
        assert!(flat_tree_with_nodes(4_097).finish(0).is_err());
    }

    #[test]
    fn chain_of_32_levels_is_accepted() {
        assert!(chain_tree(32).finish(0).is_ok());
    }

    #[test]
    fn chain_of_33_levels_is_too_deep() {
        assert!(chain_tree(33).finish(0).is_err());
    }

    #[test]
    fn duplicate_key_in_one_tree_is_rejected() {
        let mut tree = Tree::new();
        tree.node(widgets::clear()).children(&[1, 2]).done();
        tree.node(widgets::clear()).key("save").done();
        tree.node(widgets::clear()).key("save").done();
        assert!(tree.finish(0).is_err());
    }

    #[test]
    fn disabled_reason_over_40_characters_is_rejected() {
        let mut tree = Tree::new();
        tree.node(widgets::clear())
            .disabled_reason("r".repeat(41))
            .done();
        assert!(tree.finish(0).is_err());
    }
}
