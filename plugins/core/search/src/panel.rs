//! The search panel's view: what the client draws for `ilium.core.search/search`.
//!
//! The tree holds the real widgets: a framed block with a query field (`query`) and the
//! grouped result list (`results`). Beside them it carries a hidden text twin of the same
//! frame, one paragraph of plain lines. A client skips hidden nodes; a renderer that only
//! draws paragraphs (the test host) shows the twin, so a test can read the panel as text and
//! compare it with the figures of the manual page.

use ilium_plugin_sdk::types::{
    Alignment, Area, BlockSpec, BlockTitle, BorderType, Borders, Constraint, Direction, Flex,
    LayoutSpec, Margin, NodeFlags, Padding, ParagraphSpec, Spacing, Span, Style, TextInputSpec,
    TitlePosition, TreeItem, TreeListSpec, UiNode, UiTree, Widget,
};

use crate::client::{with_state, Hit, PanelState};

/// Key of the query field.
pub const QUERY_KEY: &str = "query";
/// Key of the result list.
pub const RESULTS_KEY: &str = "results";
/// The frame title.
const TITLE: &str = "Workspace search";
/// The kernel waits this long after the last keystroke before it reports the query.
const DEBOUNCE_MS: u32 = 150;
/// The mark in front of the selected result.
const SELECTION_MARK: &str = "▸";
/// The cursor glyph at the end of the typed query.
const CURSOR: &str = "▏";

/// A single plain span.
fn span(text: impl Into<String>) -> Span {
    Span {
        text: text.into(),
        style: Style::default(),
    }
}

/// The text with line breaks and tabs replaced by spaces.
fn flat(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

/// The heading of the group of hits of `kind`.
fn heading(kind: &str) -> String {
    match kind {
        "agent" => "Agents".to_string(),
        "terminal" => "Terminals".to_string(),
        "file" => "Files".to_string(),
        "meaning" => "Meaning".to_string(),
        other => {
            let mut letters = other.chars();
            match letters.next() {
                Some(first) => first.to_uppercase().chain(letters).collect(),
                None => String::new(),
            }
        }
    }
}

/// The secondary lines of a hit: score and pane, the text around the match, the last command.
fn detail_lines(hit: &Hit) -> Vec<String> {
    let mut lines = Vec::new();
    let mut meta = Vec::new();
    if let Some(score) = hit.score {
        meta.push(format!("score {score:.2}"));
    }
    if let Some(pane) = &hit.pane {
        meta.push(format!("pane: {}", flat(pane)));
    }
    if hit.kind == "file" {
        if let Some(line) = hit.line {
            meta.push(format!("line {}", u64::from(line) + 1));
        }
    }
    if !meta.is_empty() {
        lines.push(meta.join("   "));
    }
    if hit.has_context() {
        lines.push(flat(&hit.context_line()));
    }
    if let Some(command) = &hit.last_command {
        lines.push(format!("last command: {}", flat(command)));
    }
    lines
}

/// What the body of the panel says when there is nothing to list.
fn empty_note(state: &PanelState) -> Option<String> {
    if state.pending {
        Some("Searching…".to_string())
    } else if state.answered && !state.draft.is_empty() {
        Some("No matches".to_string())
    } else {
        None
    }
}

/// The body lines and, for each hit, the lines it spans.
fn body_lines(state: &PanelState) -> (Vec<String>, Option<(usize, usize)>) {
    let mut lines = Vec::new();
    let mut selected_span = None;
    for (index, hit) in state.hits.iter().enumerate() {
        if index == 0 || state.hits[index - 1].kind != hit.kind {
            lines.push(heading(&hit.kind));
        }
        let first = lines.len();
        let mark = if index == state.selected {
            SELECTION_MARK
        } else {
            " "
        };
        lines.push(format!("  {mark} {}", flat(&hit.title)));
        for detail in detail_lines(hit) {
            lines.push(format!("    {detail}"));
        }
        if index == state.selected {
            selected_span = Some((first, lines.len() - 1));
        }
    }
    if lines.is_empty() {
        lines.extend(empty_note(state).map(|note| format!("  {note}")));
    }
    (lines, selected_span)
}

fn clip(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

fn pad(text: &str, width: usize) -> String {
    let mut padded = clip(text, width);
    let missing = width.saturating_sub(padded.chars().count());
    padded.push_str(&" ".repeat(missing));
    padded
}

fn frame_top(columns: usize) -> String {
    let head = format!("╭─ {TITLE} ");
    let used = head.chars().count();
    if columns > used {
        format!("{head}{}╮", "─".repeat(columns - used - 1))
    } else {
        clip(&head, columns)
    }
}

fn frame_bottom(columns: usize) -> String {
    if columns >= 2 {
        format!("╰{}╯", "─".repeat(columns - 2))
    } else {
        clip("╰", columns)
    }
}

/// One row inside the frame; `thumb` marks a scrollbar thumb cell on the right border.
fn frame_row(text: &str, columns: usize, thumb: bool) -> String {
    if columns < 5 {
        return clip(text, columns);
    }
    let inner = columns - 4;
    let right = if thumb { "█" } else { "│" };
    format!("│ {} {right}", pad(text, inner))
}

/// The query row: the label, the end of the typed text that fits, and the cursor.
fn query_text(draft: &str, inner: usize) -> String {
    let label = "Search ";
    let room = inner.saturating_sub(label.chars().count() + CURSOR.chars().count());
    let typed: Vec<char> = flat(draft).chars().collect();
    let shown: String = typed[typed.len().saturating_sub(room)..].iter().collect();
    format!("{label}{shown}{CURSOR}")
}

/// The panel as plain lines for a screen of `columns` by `rows`.
fn twin_lines(state: &PanelState, columns: usize, rows: usize) -> Vec<String> {
    if rows == 0 {
        return Vec::new();
    }
    let mut screen = vec![frame_top(columns)];
    if rows == 1 {
        return screen;
    }
    let interior = rows - 2;
    let inner = columns.saturating_sub(4);
    let (body, selected_span) = body_lines(state);
    let capacity = interior.saturating_sub(2);
    let mut start = 0;
    if let Some((first, last)) = selected_span {
        if last >= capacity {
            start = last + 1 - capacity;
        }
        start = start.min(first);
    }
    let overflow = body.len() > capacity;
    let thumb_len = if overflow {
        (capacity * capacity / body.len()).max(1)
    } else {
        0
    };
    let travel = capacity.saturating_sub(thumb_len);
    let scrollable = body.len().saturating_sub(capacity).max(1);
    let thumb_start = if overflow {
        start * travel / scrollable
    } else {
        0
    };
    let mut rows_inside = vec![
        frame_row(&query_text(&state.draft, inner), columns, false),
        frame_row("", columns, false),
    ];
    for row in 0..capacity {
        let text = body.get(start + row).map_or("", String::as_str);
        let thumb = overflow && row >= thumb_start && row < thumb_start + thumb_len;
        rows_inside.push(frame_row(text, columns, thumb));
    }
    rows_inside.truncate(interior);
    while rows_inside.len() < interior {
        rows_inside.push(frame_row("", columns, false));
    }
    screen.extend(rows_inside);
    screen.push(frame_bottom(columns));
    screen
}

/// The result list items: a heading row per group, then one row per hit.
fn list_items(state: &PanelState) -> Vec<TreeItem> {
    let mut items = Vec::new();
    for (index, hit) in state.hits.iter().enumerate() {
        if index == 0 || state.hits[index - 1].kind != hit.kind {
            items.push(TreeItem {
                id: format!("group:{}", hit.kind),
                label: vec![span(heading(&hit.kind))],
                depth: 0,
                expandable: false,
                expanded: true,
                icon: None,
                trailing: None,
                badge: None,
                tooltip: None,
                draggable: false,
                drop_target: false,
            });
        }
        let trailing = detail_lines(hit).join("  ");
        items.push(TreeItem {
            id: state.item_id(index),
            label: vec![span(flat(&hit.title))],
            depth: 1,
            expandable: false,
            expanded: false,
            icon: None,
            trailing: if trailing.is_empty() {
                None
            } else {
                Some(vec![span(trailing)])
            },
            badge: None,
            tooltip: None,
            draggable: false,
            drop_target: false,
        });
    }
    items
}

fn node(key: Option<&str>, widget: Widget, children: Vec<u32>, flags: NodeFlags) -> UiNode {
    UiNode {
        key: key.map(str::to_string),
        widget,
        children,
        flags,
        tooltip: None,
        tab_index: None,
        disabled_reason: None,
    }
}

/// The tree of the search panel for an area of `area` cells.
pub fn render(area: Area) -> UiTree {
    with_state(|state| {
        let twin = ParagraphSpec {
            lines: twin_lines(state, usize::from(area.columns), usize::from(area.rows))
                .into_iter()
                .map(|line| vec![span(line)])
                .collect(),
            rich: None,
            wrap: Some(false),
            alignment: Alignment::Left,
            style: Style::default(),
            scrollable: false,
        };
        let frame = BlockSpec {
            borders: Borders::ALL,
            border_type: BorderType::Rounded,
            titles: vec![BlockTitle {
                text: vec![span(TITLE)],
                position: TitlePosition::Top,
                alignment: Alignment::Left,
            }],
            padding: Padding {
                left: 1,
                right: 1,
                top: 0,
                bottom: 0,
            },
            style: Style::default(),
            border_style: Style::default(),
        };
        let layout = LayoutSpec {
            direction: Direction::Vertical,
            constraints: vec![
                Constraint::Length(1),
                Constraint::Fill(1),
                Constraint::Length(0),
            ],
            flex: Flex::Legacy,
            spacing: Spacing::Space(0),
            margin: Margin::default(),
        };
        let field = TextInputSpec {
            label: "Search".to_string(),
            initial: state.draft.clone(),
            placeholder: Some("Type to search".to_string()),
            debounce_ms: DEBOUNCE_MS,
            ..TextInputSpec::default()
        };
        let list = TreeListSpec {
            items: list_items(state),
            multi_select: false,
        };
        let mut field_node = node(
            Some(QUERY_KEY),
            Widget::TextInput(field),
            Vec::new(),
            NodeFlags::FOCUSABLE,
        );
        field_node.tab_index = Some(1);
        let mut list_node = node(
            Some(RESULTS_KEY),
            Widget::TreeList(list),
            Vec::new(),
            NodeFlags::FOCUSABLE,
        );
        list_node.tab_index = Some(2);
        UiTree {
            version: 1,
            nodes: vec![
                node(None, Widget::Block(frame), vec![1], NodeFlags::default()),
                node(
                    None,
                    Widget::Layout(layout),
                    vec![2, 3, 4],
                    NodeFlags::default(),
                ),
                field_node,
                list_node,
                node(None, Widget::Paragraph(twin), Vec::new(), NodeFlags::HIDDEN),
            ],
            root: 0,
        }
    })
}
