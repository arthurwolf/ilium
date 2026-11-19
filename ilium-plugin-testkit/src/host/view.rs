//! Plain rendering of a plugin's UI tree for `FakeHost::render`, `click` and `last_tree`.
//!
//! This is not the client's layout. Widgets are written top to bottom, one text line each,
//! from the left edge, and cut at the screen size. It exists so a test can read the text a
//! view shows and click what it offers; a test that depends on exact placement belongs in
//! the client. The actions a click sends match the client popup host
//! (`ilium-client/src/plugin_host/popup.rs`).

use ilium_plugin_sdk_core::types::{
    ActionKind, ActionValue, BorderType, Borders, Span, TriState, UiTree, ViewTarget, Widget,
};

use super::Screen;

/// A clickable stretch of one rendered row and the action a click on it sends.
#[derive(Debug, Clone)]
pub(super) struct Hit {
    pub(super) row: u16,
    pub(super) first_column: u16,
    /// One past the last column.
    pub(super) end_column: u16,
    pub(super) key: String,
    pub(super) action_id: String,
    pub(super) kind: ActionKind,
    pub(super) value: Option<ActionValue>,
}

/// What the host remembers of a rendered view.
#[derive(Debug, Clone)]
pub(super) struct RenderedView {
    pub(super) target: ViewTarget,
    pub(super) tree: UiTree,
    pub(super) hits: Vec<Hit>,
}

/// A clickable stretch of a line before the line has a row.
struct Stretch {
    first: usize,
    end: usize,
    key: String,
    action_id: String,
    kind: ActionKind,
    value: Option<ActionValue>,
}

#[derive(Default)]
struct Line {
    text: String,
    stretches: Vec<Stretch>,
}

impl Line {
    fn plain(text: String) -> Line {
        Line {
            text,
            stretches: Vec::new(),
        }
    }

    /// A line that is one clickable stretch from end to end.
    fn clickable(
        text: String,
        key: &str,
        action_id: &str,
        kind: ActionKind,
        value: Option<ActionValue>,
    ) -> Line {
        let end = text.chars().count();
        Line {
            text,
            stretches: vec![Stretch {
                first: 0,
                end,
                key: key.to_owned(),
                action_id: action_id.to_owned(),
                kind,
                value,
            }],
        }
    }
}

fn text_of(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// Most nodes one render visits; guards against a tree whose child lists form a cycle.
const NODE_BUDGET: usize = 65_536;

fn write_node(tree: &UiTree, index: u32, lines: &mut Vec<Line>, budget: &mut usize, columns: usize) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    let Some(node) = tree.nodes.get(index as usize) else {
        return;
    };
    let key = node.key.as_deref().unwrap_or("");
    let mut after: Vec<Line> = Vec::new();
    match &node.widget {
        Widget::Paragraph(spec) => {
            lines.extend(spec.lines.iter().map(|line| Line::plain(text_of(line))));
        }
        Widget::Dialog(spec) => {
            lines.push(Line::plain(text_of(&spec.title)));
            if let Some(message) = &spec.message {
                lines.extend(message.iter().map(|line| Line::plain(text_of(line))));
            }
            // The buttons share one row below the dialog body.
            let mut row = Line::default();
            for button in &spec.buttons {
                if !row.text.is_empty() {
                    row.text.push_str("  ");
                }
                let first = row.text.chars().count();
                row.text.push_str(&format!("[ {} ]", button.label));
                row.stretches.push(Stretch {
                    first,
                    end: row.text.chars().count(),
                    // The client names a dialog without a key "dialog".
                    key: node.key.clone().unwrap_or_else(|| "dialog".to_owned()),
                    action_id: button.id.clone(),
                    kind: ActionKind::Activated,
                    value: None,
                });
            }
            if !row.text.is_empty() {
                after.push(row);
            }
        }
        Widget::Choice(spec) => {
            if !spec.label.is_empty() {
                lines.push(Line::plain(spec.label.clone()));
            }
            for (position, option) in spec.options.iter().enumerate() {
                let index = u32::try_from(position).unwrap_or(u32::MAX);
                // As in the client, the option already selected sends nothing.
                lines.push(if index == spec.selected {
                    Line::plain(format!("(*) {option}"))
                } else {
                    Line::clickable(
                        format!("( ) {option}"),
                        key,
                        "",
                        ActionKind::Changed,
                        Some(ActionValue::Index(index)),
                    )
                });
            }
        }
        Widget::CheckList(spec) => {
            for item in &spec.items {
                let (mark, next) = match item.state {
                    TriState::On => ("[x]", TriState::Off),
                    TriState::Off => ("[ ]", TriState::On),
                    TriState::Mixed => ("[-]", TriState::On),
                };
                let text = format!("{mark} {}", text_of(&item.label));
                lines.push(if item.disabled_reason.is_some() {
                    Line::plain(text)
                } else {
                    Line::clickable(
                        text,
                        key,
                        "",
                        ActionKind::Toggled,
                        Some(ActionValue::ItemState((item.id.clone(), next))),
                    )
                });
            }
        }
        Widget::Toggle(spec) => {
            let marker = if spec.value { "[x]" } else { "[ ]" };
            let text = format!("{} {}", spec.label, marker);
            lines.push(if node.disabled_reason.is_some() {
                Line::plain(text)
            } else {
                Line::clickable(text, key, "", ActionKind::Toggled, Some(ActionValue::Flag(!spec.value)))
            });
        }
        Widget::Button(spec) => {
            let text = format!("[ {} ]", spec.label);
            lines.push(if spec.enabled {
                Line::clickable(text, key, "", ActionKind::Activated, None)
            } else {
                Line::plain(text)
            });
        }
        Widget::Block(spec) if spec.borders.0 == Borders::ALL.0 && columns >= 4 => {
            let mut inner: Vec<Line> = Vec::new();
            let left = usize::from(spec.padding.left);
            let right = usize::from(spec.padding.right);
            let inner_columns = columns.saturating_sub(2 + left + right);
            for child in &node.children {
                write_node(tree, *child, &mut inner, budget, inner_columns);
            }
            let (top_left, top_right, bottom_left, bottom_right) = match spec.border_type {
                BorderType::Rounded => ('╭', '╮', '╰', '╯'),
                _ => ('┌', '┐', '└', '┘'),
            };
            let title = spec.titles.first().map(|title| text_of(&title.text)).unwrap_or_default();
            let mut top = String::new();
            top.push(top_left);
            top.push('─');
            if !title.is_empty() {
                top.push(' ');
                top.push_str(&title);
                top.push(' ');
            }
            while top.chars().count() < columns - 1 {
                top.push('─');
            }
            top.push(top_right);
            lines.push(Line::plain(top));
            for line in inner {
                let used = line.text.chars().count();
                let mut text = String::from('│');
                text.push_str(&" ".repeat(left));
                text.push_str(&line.text);
                text.push_str(&" ".repeat(inner_columns.saturating_sub(used) + right));
                text.push('│');
                let shift = 1 + left;
                lines.push(Line {
                    text,
                    stretches: line
                        .stretches
                        .into_iter()
                        .map(|mut stretch| {
                            stretch.first += shift;
                            stretch.end += shift;
                            stretch
                        })
                        .collect(),
                });
            }
            let mut bottom = String::new();
            bottom.push(bottom_left);
            bottom.push_str(&"─".repeat(columns - 2));
            bottom.push(bottom_right);
            lines.push(Line::plain(bottom));
            return;
        }
        // Every other widget shows only what its children show.
        _ => {}
    }
    for child in &node.children {
        write_node(tree, *child, lines, budget, columns);
    }
    lines.append(&mut after);
}

/// Draws a tree on a screen of the given size and lists what can be clicked.
pub(super) fn draw(tree: &UiTree, columns: u16, rows: u16) -> (Screen, Vec<Hit>) {
    let mut lines = Vec::new();
    let mut budget = NODE_BUDGET;
    write_node(tree, tree.root, &mut lines, &mut budget, usize::from(columns));
    let mut screen = Screen::blank(columns, rows);
    let mut hits = Vec::new();
    let width = usize::from(columns);
    for (row, line) in (0..rows).zip(lines) {
        for (column, character) in line.text.chars().take(width).enumerate() {
            let cell = usize::from(row) * width + column;
            if let Some(cell) = screen.cells.get_mut(cell) {
                cell.symbol = character.to_string();
            }
        }
        for stretch in line.stretches {
            let cut = |column: usize| u16::try_from(column.min(width)).unwrap_or(columns);
            if stretch.first < width {
                hits.push(Hit {
                    row,
                    first_column: cut(stretch.first),
                    end_column: cut(stretch.end),
                    key: stretch.key,
                    action_id: stretch.action_id,
                    kind: stretch.kind,
                    value: stretch.value,
                });
            }
        }
    }
    (screen, hits)
}
