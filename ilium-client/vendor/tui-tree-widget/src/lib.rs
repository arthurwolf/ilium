//! Widget built to show Tree Data structures.
//!
//! Tree widget [`Tree`] is generated with [`TreeItem`]s (which itself can contain [`TreeItem`] children to form the tree structure).
//! The user interaction state (like the current selection) is stored in the [`TreeState`].

use std::collections::HashSet;

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Style};
use ratatui_core::widgets::{StatefulWidget, Widget};
pub use ratatui_widgets::block::Block;
pub use ratatui_widgets::scrollbar::{Scrollbar, ScrollbarState};
use unicode_width::UnicodeWidthStr as _;

pub use crate::flatten::Flattened;
pub use crate::tree_item::TreeItem;
pub use crate::tree_state::TreeState;

mod flatten;
mod tree_item;
mod tree_state;

/// A `Tree` which can be rendered.
///
/// The generic argument `Identifier` is used to keep the state like the currently selected or opened [`TreeItem`]s in the [`TreeState`].
/// For more information see [`TreeItem`].
///
/// # Example
///
/// ```
/// # use tui_tree_widget::{Tree, TreeItem, TreeState};
/// # use ratatui::backend::TestBackend;
/// # use ratatui::Terminal;
/// # use ratatui::widgets::Block;
/// # let mut terminal = Terminal::new(TestBackend::new(32, 32)).unwrap();
/// let mut state = TreeState::default();
///
/// let item = TreeItem::new_leaf("l", "leaf");
/// let items = vec![item];
///
/// terminal.draw(|frame| {
///     let area = frame.area();
///
///     let tree_widget = Tree::new(&items)
///         .expect("all item identifiers are unique")
///         .block(Block::bordered().title("Tree Widget"));
///
///     frame.render_stateful_widget(tree_widget, area, &mut state);
/// })?;
/// # Ok::<(), std::convert::Infallible>(())
/// ```
#[must_use]
#[derive(Debug, Clone)]
pub struct Tree<'a, Identifier> {
    items: &'a [TreeItem<'a, Identifier>],

    block: Option<Block<'a>>,
    scrollbar: Option<Scrollbar<'a>>,
    /// Style used as a base style for the widget
    style: Style,

    /// Style used to render selected item
    highlight_style: Style,
    /// Symbol in front of the selected item (Shift all items to the right)
    highlight_symbol: &'a str,

    /// Symbol displayed in front of a closed node (As in the children are currently not visible)
    node_closed_symbol: &'a str,
    /// Symbol displayed in front of an open node. (As in the children are currently visible)
    node_open_symbol: &'a str,
    /// Symbol displayed in front of a node without children.
    node_no_children_symbol: &'a str,
    /// Draws marked subtree separators and reserves their rows for scrolling.
    subtree_separators: bool,
    subtree_separator_symbol: &'a str,
    subtree_separator_style: Style,
}

impl<'a, Identifier> Tree<'a, Identifier>
where
    Identifier: Clone + PartialEq + Eq + core::hash::Hash,
{
    /// Create a new `Tree`.
    ///
    /// # Errors
    ///
    /// Errors when there are duplicate identifiers in the children.
    pub fn new(items: &'a [TreeItem<'a, Identifier>]) -> std::io::Result<Self> {
        let identifiers = items
            .iter()
            .map(|item| &item.identifier)
            .collect::<HashSet<_>>();
        if identifiers.len() != items.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "The items contain duplicate identifiers",
            ));
        }

        Ok(Self {
            items,
            block: None,
            scrollbar: None,
            style: Style::new(),
            highlight_style: Style::new(),
            highlight_symbol: "",
            node_closed_symbol: "\u{25b6} ", // Arrow to right
            node_open_symbol: "\u{25bc} ",   // Arrow down
            node_no_children_symbol: "  ",
            subtree_separators: false,
            subtree_separator_symbol: "─",
            subtree_separator_style: Style::new().fg(Color::DarkGray),
        })
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Show the scrollbar when rendering this widget.
    ///
    /// Experimental: Can change on any release without any additional notice.
    /// Its there to test and experiment with whats possible with scrolling widgets.
    /// Also see <https://github.com/ratatui-org/ratatui/issues/174>
    pub const fn experimental_scrollbar(mut self, scrollbar: Option<Scrollbar<'a>>) -> Self {
        self.scrollbar = scrollbar;
        self
    }

    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub const fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    pub const fn highlight_symbol(mut self, highlight_symbol: &'a str) -> Self {
        self.highlight_symbol = highlight_symbol;
        self
    }

    pub const fn node_closed_symbol(mut self, symbol: &'a str) -> Self {
        self.node_closed_symbol = symbol;
        self
    }

    pub const fn node_open_symbol(mut self, symbol: &'a str) -> Self {
        self.node_open_symbol = symbol;
        self
    }

    pub const fn node_no_children_symbol(mut self, symbol: &'a str) -> Self {
        self.node_no_children_symbol = symbol;
        self
    }

    /// Enables separators after marked items' complete visible subtrees.
    pub const fn subtree_separators(mut self, enabled: bool) -> Self {
        self.subtree_separators = enabled;
        self
    }

    /// Sets the style used for enabled subtree separators.
    pub const fn subtree_separator_style(mut self, style: Style) -> Self {
        self.subtree_separator_style = style;
        self
    }
}

#[test]
#[should_panic = "duplicate identifiers"]
fn tree_new_errors_with_duplicate_identifiers() {
    let item = TreeItem::new_leaf("same", "text");
    let another = item.clone();
    let items = [item, another];
    let _: Tree<_> = Tree::new(&items).unwrap();
}

impl<Identifier> StatefulWidget for Tree<'_, Identifier>
where
    Identifier: Clone + PartialEq + Eq + core::hash::Hash,
{
    type State = TreeState<Identifier>;

    #[expect(clippy::too_many_lines)]
    fn render(self, full_area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        buf.set_style(full_area, self.style);

        // Get the inner area inside a possible block, otherwise use the full area
        let area = self.block.map_or(full_area, |block| {
            let inner_area = block.inner(full_area);
            block.render(full_area, buf);
            inner_area
        });

        state.last_area = area;
        state.last_rendered_rows.clear();
        state.last_item_heights.clear();
        if area.width < 1 || area.height < 1 {
            return;
        }

        let visible = state.flatten(self.items);
        let separator_after_indices = if self.subtree_separators {
            subtree_separator_indices(&visible)
        } else {
            HashSet::new()
        };
        state.last_biggest_index = visible.len().saturating_sub(1);
        state.last_item_heights.extend(
            visible
                .iter()
                .enumerate()
                .map(|(index, flattened)| {
                    flattened.item.height().max(1)
                        + usize::from(separator_after_indices.contains(&index))
                }),
        );
        if visible.is_empty() {
            return;
        }
        let available_height = area.height as usize;

        let ensure_index_in_view =
            if state.ensure_selected_in_view_on_next_render && !state.selected.is_empty() {
                visible
                    .iter()
                    .position(|flattened| flattened.identifier == state.selected)
            } else {
                None
            };

        // Ensure last line is still visible
        let mut start = state.offset.min(state.last_biggest_index);

        if let Some(ensure_index_in_view) = ensure_index_in_view {
            start = start.min(ensure_index_in_view);
        }

        let mut end = start;
        let mut height = 0;
        for item_height in state.last_item_heights.iter().skip(start).copied() {
            if height + item_height > available_height {
                // A two-line row in a one-line viewport must still paint its
                // first line and remain an accessible scroll target.
                if height == 0 {
                    height = available_height;
                    end += 1;
                }
                break;
            }
            height += item_height;
            end += 1;
        }

        if let Some(ensure_index_in_view) = ensure_index_in_view {
            while ensure_index_in_view >= end {
                height += state.last_item_heights[end].min(available_height);
                end += 1;
                while height > available_height {
                    height =
                        height.saturating_sub(state.last_item_heights[start].min(available_height));
                    start += 1;
                }
            }
        }

        state.offset = start;
        state.ensure_selected_in_view_on_next_render = false;

        if let Some(scrollbar) = self.scrollbar {
            let total_lines: usize = state.last_item_heights.iter().sum();
            let first_line: usize = state.last_item_heights.iter().take(start).sum();
            let mut scrollbar_state = ScrollbarState::new(total_lines)
                .position(first_line)
                .viewport_content_length(available_height);
            let scrollbar_area = Rect {
                // Inner height to be exactly as the content
                y: area.y,
                height: area.height,
                // Outer width to stay on the right border
                x: full_area.x,
                width: full_area.width,
            };
            scrollbar.render(scrollbar_area, buf, &mut scrollbar_state);
        }

        let blank_symbol = " ".repeat(self.highlight_symbol.width());
        // Reused across every visible row instead of allocating a fresh
        // `String` per row/frame via `.repeat()` inside the render loop.
        let mut indent_buf = String::new();

        let mut current_height = 0;
        let has_selection = !state.selected.is_empty();
        #[expect(clippy::cast_possible_truncation)]
        for (visible_index, flattened) in visible
            .iter()
            .enumerate()
            .skip(state.offset)
            .take(end - start)
        {
            let Flattened { identifier, item } = flattened;

            let x = area.x;
            let y = area.y + current_height;
            let height = item
                .height()
                .min(available_height.saturating_sub(usize::from(current_height)))
                as u16;
            current_height += height;

            let area = Rect {
                x,
                y,
                width: area.width,
                height,
            };

            let text = &item.text;
            let item_style = text.style;

            let is_selected = state.selected == *identifier;
            let after_highlight_symbol_x = if has_selection {
                let symbol = if is_selected {
                    self.highlight_symbol
                } else {
                    &blank_symbol
                };
                let (x, _) = buf.set_stringn(x, y, symbol, area.width as usize, item_style);
                x
            } else {
                x
            };

            let after_depth_x = {
                // One guide glyph per depth level (half of the old `depth * 2`
                // blank-space run), drawn in a fixed dark grey rather than the
                // row's own style so nesting stays visible without competing
                // with the item's text color.
                let depth = flattened.depth();
                let indent_style = Style::new().fg(Color::DarkGray);
                indent_buf.clear();
                for _ in 0..depth {
                    indent_buf.push('\u{203a}');
                }
                let (after_indent_x, _) = buf.set_stringn(
                    after_highlight_symbol_x,
                    y,
                    &indent_buf,
                    depth,
                    indent_style,
                );
                let symbol = if item.children.is_empty() {
                    self.node_no_children_symbol
                } else if state.opened.contains(identifier) {
                    self.node_open_symbol
                } else {
                    self.node_closed_symbol
                };
                let max_width = area.width.saturating_sub(after_indent_x - x);
                let (x, _) =
                    buf.set_stringn(after_indent_x, y, symbol, max_width as usize, item_style);
                x
            };

            let text_area = Rect {
                x: after_depth_x,
                width: area.width.saturating_sub(after_depth_x - x),
                ..area
            };
            text.render(text_area, buf);

            if is_selected {
                buf.set_style(area, self.highlight_style);
            }

            state
                .last_rendered_rows
                .push((area.y, area.height, visible_index));

            if separator_after_indices.contains(&visible_index) {
                if current_height < available_height as u16 {
                    let separator_y = area.y.saturating_add(height);
                    let separator = self
                        .subtree_separator_symbol
                        .repeat(area.width as usize);
                    buf.set_stringn(
                        area.x,
                        separator_y,
                        &separator,
                        area.width as usize,
                        self.subtree_separator_style,
                    );
                }
                current_height = current_height.saturating_add(1);
            }
        }
        // Reuse the existing Vec's allocation across renders (this runs every
        // frame in a TUI redraw loop) instead of dropping it and collecting
        // into a brand-new Vec each time.
        state.last_identifiers.clear();
        state
            .last_identifiers
            .extend(visible.into_iter().map(|flattened| flattened.identifier));
    }
}

/// Finds each marked item's last currently visible descendant. The
/// separator belongs to that rendered row, while remaining absent from the
/// identifier list so it cannot be selected or hit-tested.
fn subtree_separator_indices<Identifier>(
    visible: &[Flattened<'_, Identifier>],
) -> HashSet<usize> {
    let mut separators = HashSet::new();
    for (index, flattened) in visible.iter().enumerate() {
        if !flattened.item.separator_after_subtree {
            continue;
        }
        let subtree_depth = flattened.identifier.len();
        let subtree_end = visible
            .iter()
            .enumerate()
            .skip(index + 1)
            .find(|(_, candidate)| candidate.identifier.len() <= subtree_depth)
            .map_or(visible.len().saturating_sub(1), |(next, _)| next - 1);
        separators.insert(subtree_end);
    }
    separators
}

impl<Identifier> Widget for Tree<'_, Identifier>
where
    Identifier: Clone + Eq + core::hash::Hash,
{
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut state = TreeState::default();
        StatefulWidget::render(self, area, buf, &mut state);
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use ratatui_core::layout::Position;

    #[must_use]
    #[track_caller]
    fn render(width: u16, height: u16, state: &mut TreeState<&'static str>) -> Buffer {
        let items = TreeItem::example();
        let tree = Tree::new(&items).unwrap();
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        StatefulWidget::render(tree, area, &mut buffer, state);
        buffer
    }

    #[test]
    fn does_not_panic() {
        _ = render(0, 0, &mut TreeState::default());
        _ = render(10, 0, &mut TreeState::default());
        _ = render(0, 10, &mut TreeState::default());
        _ = render(10, 10, &mut TreeState::default());
    }

    #[test]
    fn two_line_item_owns_both_rendered_rows() {
        let items = [
            TreeItem::new_leaf("agent", "Agent\nbranch"),
            TreeItem::new_leaf("next", "Next"),
        ];
        let tree = Tree::new(&items).unwrap();
        let area = Rect::new(0, 0, 12, 3);
        let mut buffer = Buffer::empty(area);
        let mut state = TreeState::default();
        StatefulWidget::render(tree, area, &mut buffer, &mut state);

        assert_eq!(items[0].height(), 2);
        assert_eq!(state.rendered_at(Position::new(2, 0)), Some(&["agent"][..]));
        assert_eq!(state.rendered_at(Position::new(2, 1)), Some(&["agent"][..]));
        assert_eq!(state.rendered_at(Position::new(2, 2)), Some(&["next"][..]));
    }

    #[test]
    fn subtree_separator_follows_visible_descendants_and_has_no_tree_hit_target() {
        let items = [
            TreeItem::new("project-a", "Project A", vec![TreeItem::new_leaf("pane-a", "Pane A")])
                .unwrap()
                .separator_after_subtree(),
            TreeItem::new_leaf("project-b", "Project B"),
        ];
        let tree = Tree::new(&items).unwrap().subtree_separators(true);
        let area = Rect::new(0, 0, 14, 4);
        let mut buffer = Buffer::empty(area);
        let mut state = TreeState::default();
        state.open(vec!["project-a"]);

        StatefulWidget::render(tree, area, &mut buffer, &mut state);

        assert_eq!(state.total_line_count(), 4);
        assert_eq!(state.rendered_at(Position::new(0, 2)), None);
        assert_eq!(buffer[(0, 2)].symbol(), "─");
        assert_eq!(state.rendered_at(Position::new(0, 3)), Some(&["project-b"][..]));

        assert!(state.scroll_down(1));
        let scroll_area = Rect::new(0, 0, 14, 2);
        let mut scroll_buffer = Buffer::empty(scroll_area);
        StatefulWidget::render(
            Tree::new(&items).unwrap().subtree_separators(true),
            scroll_area,
            &mut scroll_buffer,
            &mut state,
        );
        assert_eq!(state.first_visible_line(), 1);
        assert_eq!(scroll_buffer[(0, 1)].symbol(), "─");
        assert_eq!(state.rendered_at(Position::new(0, 1)), None);
        assert!(!state.click_at(Position::new(0, 1)));

        assert!(state.scroll_down(2));
        let mut final_buffer = Buffer::empty(scroll_area);
        StatefulWidget::render(
            Tree::new(&items).unwrap().subtree_separators(true),
            scroll_area,
            &mut final_buffer,
            &mut state,
        );
        assert_eq!(state.first_visible_line(), 3);
        assert_eq!(state.rendered_at(Position::new(0, 0)), Some(&["project-b"][..]));
    }

    #[test]
    fn mixed_height_scroll_uses_terminal_lines_and_clips_a_tall_first_item() {
        let items = [
            TreeItem::new_leaf("plain", "Plain"),
            TreeItem::new_leaf("agent", "Agent\nbranch"),
            TreeItem::new_leaf("tail", "Tail"),
        ];
        let area = Rect::new(0, 0, 12, 3);
        let mut state = TreeState::default();
        let mut buffer = Buffer::empty(area);
        StatefulWidget::render(Tree::new(&items).unwrap(), area, &mut buffer, &mut state);
        assert_eq!(state.total_line_count(), 4);
        assert_eq!(state.rendered_at(Position::new(2, 0)), Some(&["plain"][..]));
        assert_eq!(state.rendered_at(Position::new(2, 1)), Some(&["agent"][..]));
        assert_eq!(state.rendered_at(Position::new(2, 2)), Some(&["agent"][..]));

        assert!(state.scroll_down(2));
        assert_eq!(state.get_offset(), 2);
        assert_eq!(state.first_visible_line(), 3);
        assert!(state.scroll_up(2));
        assert_eq!(state.get_offset(), 1);

        let short_area = Rect::new(0, 0, 12, 1);
        let mut short_buffer = Buffer::empty(short_area);
        StatefulWidget::render(
            Tree::new(&items).unwrap(),
            short_area,
            &mut short_buffer,
            &mut state,
        );
        assert_eq!(state.rendered_at(Position::new(2, 0)), Some(&["agent"][..]));
        assert_eq!(
            state.rendered_rows().next().map(|(_, _, height)| height),
            Some(1)
        );
    }

    #[test]
    fn nothing_open() {
        let buffer = render(10, 4, &mut TreeState::default());
        #[rustfmt::skip]
        let expected = Buffer::with_lines([
            "  Alfa    ",
            "▶ Bravo   ",
            "  Hotel   ",
            "          ",
        ]);
        assert_eq!(buffer, expected);
    }

    /// Applies the dark-grey indent style the render loop puts on each `›`
    /// guide glyph, at the given `(x, y)` cells, so expected buffers built
    /// from plain strings still match on style.
    fn with_indent_style(mut buffer: Buffer, cells: &[(u16, u16)]) -> Buffer {
        let indent_style = Style::new().fg(Color::DarkGray);
        for &(x, y) in cells {
            buffer.set_style(Rect::new(x, y, 1, 1), indent_style);
        }
        buffer
    }

    #[test]
    fn depth_one() {
        let mut state = TreeState::default();
        state.open(vec!["b"]);
        let buffer = render(13, 7, &mut state);
        let expected = Buffer::with_lines([
            "  Alfa       ",
            "▼ Bravo      ",
            "›  Charlie   ",
            "›▶ Delta     ",
            "›  Golf      ",
            "  Hotel      ",
            "             ",
        ]);
        let expected = with_indent_style(expected, &[(0, 2), (0, 3), (0, 4)]);
        assert_eq!(buffer, expected);
    }

    #[test]
    fn depth_two() {
        let mut state = TreeState::default();
        state.open(vec!["b"]);
        state.open(vec!["b", "d"]);
        let buffer = render(15, 9, &mut state);
        let expected = Buffer::with_lines([
            "  Alfa         ",
            "▼ Bravo        ",
            "›  Charlie     ",
            "›▼ Delta       ",
            "››  Echo       ",
            "››  Foxtrot    ",
            "›  Golf        ",
            "  Hotel        ",
            "               ",
        ]);
        let expected = with_indent_style(
            expected,
            &[(0, 2), (0, 3), (0, 4), (1, 4), (0, 5), (1, 5), (0, 6)],
        );
        assert_eq!(buffer, expected);
    }
}
