//! The view of a board pane: what the client draws for `ViewTarget::Pane(<board pane>)`.
//!
//! Moved from `ilium-client/src/board_ui.rs`. The columns sit side by side, each a framed list of
//! its cards (the selected card carries a mark, a card with a body shows its first line when the
//! card preview setting is on); the detail panel of the selected card opens at the right with a
//! title field and a body text area; a one-row prompt at the bottom takes the name of a new
//! card or column, a rename, or the answer to a delete question.

use ilium_plugin_sdk::types::{
    Alignment, Area, BlockSpec, BlockTitle, BorderType, Borders, Constraint, Direction, Flex,
    LayoutSpec, Margin, NodeFlags, Padding, ParagraphSpec, Spacing, Span, Style, TextAreaSpec,
    TextInputSpec, TitlePosition, TreeItem, TreeListSpec, UiNode, UiTree, Widget,
};

use crate::board::BoardColumn;
use crate::model::{Board, CardEditorField};

/// Key of the title field of the detail panel.
pub const DETAIL_TITLE_KEY: &str = "detail-title";
/// Key of the body text area of the detail panel.
pub const DETAIL_BODY_KEY: &str = "detail-body";
/// Key of the prompt row.
pub const PROMPT_KEY: &str = "prompt";
/// The columns of a board list use the key `col-<index>`.
pub const COLUMN_KEY_PREFIX: &str = "col-";
/// The cards of a list use the item id `<column>.<card>`.
pub const CARD_ID_SEPARATOR: char = '.';
/// Share of the width the detail panel takes, in percent.
const DETAIL_PERCENT: u16 = 40;
/// The mark in front of the selected card.
const SELECTION_MARK: &str = "▸ ";

/// What the prompt row asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    NewCard,
    NewColumn,
    RenameCard,
    RenameColumn,
    /// Delete the selected card or column; answered with `y` or `n`.
    Delete {
        is_column: bool,
        name: String,
    },
}

impl Prompt {
    /// The label of the prompt row.
    pub fn label(&self) -> String {
        match self {
            Prompt::NewCard => "New card".to_string(),
            Prompt::NewColumn => "New column".to_string(),
            Prompt::RenameCard => "Rename card".to_string(),
            Prompt::RenameColumn => "Rename column".to_string(),
            Prompt::Delete { is_column, name } => format!(
                "Delete {} \"{name}\"? y = delete, n = keep",
                if *is_column { "column" } else { "card" }
            ),
        }
    }

    /// True when the prompt takes typed text.
    pub fn takes_text(&self) -> bool {
        !matches!(self, Prompt::Delete { .. })
    }
}

/// The user's board settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub card_preview: bool,
    pub column_width: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            card_preview: true,
            column_width: 24,
        }
    }
}

/// How many columns fit side by side in a body `columns` cells wide.
pub fn visible_column_count(columns: u16, column_count: usize, column_width: u16) -> usize {
    let fitting = usize::from(columns / column_width.max(1)).max(1);
    fitting.min(column_count.max(1))
}

/// The cells the detail panel takes of a pane `columns` wide.
pub fn detail_width(columns: u16) -> u16 {
    (u32::from(columns) * u32::from(DETAIL_PERCENT) / 100) as u16
}

/// The key of the list of column `index`.
pub fn column_key(index: usize) -> String {
    format!("{COLUMN_KEY_PREFIX}{index}")
}

/// The item id of one card.
pub fn card_id(column: usize, card: usize) -> String {
    format!("{column}{CARD_ID_SEPARATOR}{card}")
}

/// Splits a card item id into its column and card index.
pub fn parse_card_id(id: &str) -> Option<(usize, usize)> {
    let (column, card) = id.split_once(CARD_ID_SEPARATOR)?;
    Some((column.parse().ok()?, card.parse().ok()?))
}

/// The column index of a list key.
pub fn parse_column_key(key: &str) -> Option<usize> {
    key.strip_prefix(COLUMN_KEY_PREFIX)?.parse().ok()
}

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

fn node(key: Option<String>, widget: Widget, flags: NodeFlags) -> UiNode {
    UiNode {
        key,
        widget,
        children: Vec::new(),
        flags,
        tooltip: None,
        tab_index: None,
        disabled_reason: None,
    }
}

/// Appends nodes so that every child index is greater than its parent's.
#[derive(Default)]
struct Builder {
    nodes: Vec<UiNode>,
}

impl Builder {
    fn leaf(&mut self, node: UiNode) -> u32 {
        self.nodes.push(node);
        (self.nodes.len() - 1) as u32
    }

    fn parent(&mut self, node: UiNode, children: impl FnOnce(&mut Builder) -> Vec<u32>) -> u32 {
        let index = self.leaf(node);
        let kids = children(self);
        self.nodes[index as usize].children = kids;
        index
    }
}

fn layout(direction: Direction, constraints: Vec<Constraint>) -> Widget {
    Widget::Layout(LayoutSpec {
        direction,
        constraints,
        flex: Flex::Legacy,
        spacing: Spacing::Space(0),
        margin: Margin::default(),
    })
}

fn frame(title: String) -> Widget {
    Widget::Block(BlockSpec {
        borders: Borders::ALL,
        border_type: BorderType::Rounded,
        titles: vec![BlockTitle {
            text: vec![span(title)],
            position: TitlePosition::Top,
            alignment: Alignment::Left,
        }],
        padding: Padding {
            left: 0,
            right: 0,
            top: 0,
            bottom: 0,
        },
        style: Style::default(),
        border_style: Style::default(),
    })
}

fn text_paragraph(lines: Vec<String>) -> Widget {
    Widget::Paragraph(ParagraphSpec {
        lines: lines.into_iter().map(|line| vec![span(line)]).collect(),
        rich: None,
        wrap: Some(false),
        alignment: Alignment::Left,
        style: Style::default(),
        scrollable: false,
    })
}

/// The list items of one column.
fn column_items(
    column_index: usize,
    column: &BoardColumn,
    selected: Option<usize>,
    preview: bool,
) -> Vec<TreeItem> {
    column
        .cards
        .iter()
        .enumerate()
        .map(|(card_index, card)| {
            let mark = if selected == Some(card_index) {
                SELECTION_MARK
            } else {
                "  "
            };
            let first_body_line = card
                .body
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(|line| vec![span(flat(line.trim()))]);
            TreeItem {
                id: card_id(column_index, card_index),
                label: vec![span(format!("{mark}{}", flat(&card.title)))],
                trailing: if preview { first_body_line } else { None },
                draggable: true,
                drop_target: true,
                ..TreeItem::default()
            }
        })
        .collect()
}

/// The tree shown while the board is loading or could not be read.
pub fn message_tree(text: &str) -> UiTree {
    let mut builder = Builder::default();
    builder.leaf(node(
        None,
        text_paragraph(text.lines().map(str::to_string).collect()),
        NodeFlags::default(),
    ));
    UiTree {
        version: 1,
        nodes: builder.nodes,
        root: 0,
    }
}

/// The tree of a loaded board for a pane of `area` cells.
pub fn render(board: &Board, prompt: Option<&Prompt>, settings: Settings, area: Area) -> UiTree {
    let detail_open = board.is_detail_panel_open && board.detail_editor.is_some();
    let detail_columns = if detail_open {
        detail_width(area.columns)
    } else {
        0
    };
    let body_columns = area.columns.saturating_sub(detail_columns);
    let visible = visible_column_count(body_columns, board.columns.len(), settings.column_width);
    let first = board
        .column_scroll
        .min(board.columns.len().saturating_sub(visible));
    let shown = first..(first + visible).min(board.columns.len());

    let header = format!(
        "{}   n card  c column  e rename  d delete  r reload  Enter details  Shift+arrows move",
        board.storage.path()
    );
    let mut builder = Builder::default();
    let prompt_rows = u16::from(prompt.is_some());
    builder.parent(
        node(
            None,
            layout(
                Direction::Vertical,
                vec![
                    Constraint::Length(1),
                    Constraint::Fill(1),
                    Constraint::Length(prompt_rows),
                ],
            ),
            NodeFlags::default(),
        ),
        |builder| {
            let header_node = builder.leaf(node(
                None,
                text_paragraph(vec![flat(&header)]),
                NodeFlags::default(),
            ));
            let mut body_constraints: Vec<Constraint> =
                shown.clone().map(|_| Constraint::Fill(1)).collect();
            if detail_open {
                body_constraints.push(Constraint::Length(detail_columns));
            }
            let body = builder.parent(
                node(
                    None,
                    layout(Direction::Horizontal, body_constraints),
                    NodeFlags::default(),
                ),
                |builder| {
                    let mut kids = Vec::new();
                    for column_index in shown.clone() {
                        let column = &board.columns[column_index];
                        let is_selected_column = column_index == board.selected_column;
                        let title = format!(
                            "{}{} ({})",
                            if is_selected_column { "▶ " } else { "" },
                            flat(&column.title),
                            column.cards.len()
                        );
                        let selected_card = if is_selected_column {
                            board.selected_card
                        } else {
                            None
                        };
                        let list = Widget::TreeList(TreeListSpec {
                            items: column_items(
                                column_index,
                                column,
                                selected_card,
                                settings.card_preview,
                            ),
                            multi_select: false,
                        });
                        kids.push(builder.parent(
                            node(None, frame(title), NodeFlags::default()),
                            |builder| {
                                let mut list_node = node(
                                    Some(column_key(column_index)),
                                    list,
                                    NodeFlags::FOCUSABLE,
                                );
                                list_node.tab_index = Some(1);
                                vec![builder.leaf(list_node)]
                            },
                        ));
                    }
                    if let Some(editor) = board.detail_editor.as_ref().filter(|_| detail_open) {
                        kids.push(detail_panel(builder, editor));
                    }
                    kids
                },
            );
            let mut kids = vec![header_node, body];
            if let Some(prompt) = prompt {
                kids.push(prompt_row(builder, prompt));
            }
            kids
        },
    );
    UiTree {
        version: 1,
        nodes: builder.nodes,
        root: 0,
    }
}

fn detail_panel(builder: &mut Builder, editor: &crate::model::DetailEditor) -> u32 {
    builder.parent(
        node(None, frame("Card".to_string()), NodeFlags::default()),
        |builder| {
            vec![builder.parent(
                node(
                    None,
                    layout(
                        Direction::Vertical,
                        vec![Constraint::Length(1), Constraint::Fill(1)],
                    ),
                    NodeFlags::default(),
                ),
                |builder| {
                    let mut title = node(
                        Some(DETAIL_TITLE_KEY.to_string()),
                        Widget::TextInput(TextInputSpec {
                            label: "Title".to_string(),
                            initial: editor.title.clone(),
                            debounce_ms: 250,
                            ..TextInputSpec::default()
                        }),
                        NodeFlags::FOCUSABLE,
                    );
                    title.tab_index = Some(2);
                    let mut body = node(
                        Some(DETAIL_BODY_KEY.to_string()),
                        Widget::TextArea(TextAreaSpec {
                            initial: editor.body.clone(),
                            placeholder: Some("Details".to_string()),
                            wrap: true,
                            line_numbers: false,
                            max_bytes: 0,
                            debounce_ms: 250,
                            tab_moves_focus: true,
                        }),
                        NodeFlags::FOCUSABLE,
                    );
                    body.tab_index = Some(3);
                    vec![builder.leaf(title), builder.leaf(body)]
                },
            )]
        },
    )
}

fn prompt_row(builder: &mut Builder, prompt: &Prompt) -> u32 {
    if prompt.takes_text() {
        let mut input = node(
            Some(PROMPT_KEY.to_string()),
            Widget::TextInput(TextInputSpec {
                label: prompt.label(),
                submit_on_enter: true,
                max_length: 200,
                ..TextInputSpec::default()
            }),
            NodeFlags::FOCUSABLE,
        );
        input.tab_index = Some(0);
        builder.leaf(input)
    } else {
        builder.leaf(node(
            None,
            text_paragraph(vec![prompt.label()]),
            NodeFlags::default(),
        ))
    }
}

/// True when the detail field `focus` is the one `field` names.
pub fn is_field(key: &str, focus: CardEditorField) -> bool {
    match focus {
        CardEditorField::Title => key == DETAIL_TITLE_KEY,
        CardEditorField::Body => key == DETAIL_BODY_KEY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::BoardCard;
    use crate::model::{default_columns, Storage};

    fn board() -> Board {
        let mut columns = default_columns();
        columns[0].cards.push(BoardCard {
            title: "write docs".to_string(),
            body: "\nfirst line\nsecond".to_string(),
        });
        columns[0].cards.push(BoardCard {
            title: "ship".to_string(),
            body: String::new(),
        });
        Board::new(
            Storage::MarkdownFile {
                path: "plan.md".to_string(),
            },
            columns,
        )
    }

    fn area() -> Area {
        Area {
            columns: 100,
            rows: 30,
        }
    }

    /// Every child index is greater than its parent and each node but the root has one parent.
    fn assert_well_formed(tree: &UiTree) {
        let mut parents = vec![0_u32; tree.nodes.len()];
        for (index, node) in tree.nodes.iter().enumerate() {
            for child in &node.children {
                assert!(*child as usize > index, "child before parent");
                assert!((*child as usize) < tree.nodes.len());
                parents[*child as usize] += 1;
            }
        }
        for (index, count) in parents.iter().enumerate() {
            assert_eq!(*count, u32::from(index != 0), "node {index}");
        }
    }

    fn keys(tree: &UiTree) -> Vec<String> {
        tree.nodes
            .iter()
            .filter_map(|node| node.key.clone())
            .collect()
    }

    #[test]
    fn card_ids_and_column_keys_round_trip() {
        assert_eq!(parse_card_id(&card_id(3, 12)), Some((3, 12)));
        assert_eq!(parse_card_id("x.1"), None);
        assert_eq!(parse_column_key(&column_key(7)), Some(7));
        assert_eq!(parse_column_key("detail-title"), None);
    }

    #[test]
    fn every_column_gets_a_keyed_list_and_the_tree_is_well_formed() {
        let tree = render(&board(), None, Settings::default(), area());
        assert_well_formed(&tree);
        assert_eq!(keys(&tree), vec!["col-0", "col-1", "col-2"]);
    }

    #[test]
    fn the_selected_card_carries_the_mark_and_the_preview_follows_the_setting() {
        let mut state = board();
        state.select_card(0, 1);
        let tree = render(&state, None, Settings::default(), area());
        let items = tree
            .nodes
            .iter()
            .find_map(|node| match &node.widget {
                Widget::TreeList(list) if node.key.as_deref() == Some("col-0") => {
                    Some(list.items.clone())
                }
                _ => None,
            })
            .expect("column 0 list");
        assert!(items[1].label[0].text.starts_with(SELECTION_MARK));
        assert!(items[0].label[0].text.starts_with("  "));
        assert_eq!(items[0].trailing.as_ref().unwrap()[0].text, "first line");
        assert!(items[1].trailing.is_none());
        let without = render(
            &state,
            None,
            Settings {
                card_preview: false,
                ..Settings::default()
            },
            area(),
        );
        assert!(without.nodes.iter().all(|node| match &node.widget {
            Widget::TreeList(list) => list.items.iter().all(|item| item.trailing.is_none()),
            _ => true,
        }));
    }

    #[test]
    fn a_narrow_pane_shows_the_columns_that_fit_from_the_scroll_position() {
        let mut state = board();
        state.column_scroll = 1;
        let narrow = Area {
            columns: 50,
            rows: 20,
        };
        let tree = render(&state, None, Settings::default(), narrow);
        assert_well_formed(&tree);
        assert_eq!(keys(&tree), vec!["col-1", "col-2"]);
        assert_eq!(visible_column_count(10, 5, 24), 1);
        assert_eq!(visible_column_count(200, 3, 24), 3);
    }

    #[test]
    fn the_detail_panel_adds_a_title_field_and_a_body_area() {
        let mut state = board();
        state.open_card_details(0, 0);
        let tree = render(&state, None, Settings::default(), area());
        assert_well_formed(&tree);
        let keys = keys(&tree);
        assert!(keys.contains(&DETAIL_TITLE_KEY.to_string()));
        assert!(keys.contains(&DETAIL_BODY_KEY.to_string()));
        assert_eq!(detail_width(100), 40);
    }

    #[test]
    fn a_text_prompt_is_an_input_and_a_delete_question_is_plain_text() {
        let state = board();
        let tree = render(&state, Some(&Prompt::NewCard), Settings::default(), area());
        assert_well_formed(&tree);
        assert!(keys(&tree).contains(&PROMPT_KEY.to_string()));
        let delete = Prompt::Delete {
            is_column: false,
            name: "ship".to_string(),
        };
        let tree = render(&state, Some(&delete), Settings::default(), area());
        assert_well_formed(&tree);
        assert!(!keys(&tree).contains(&PROMPT_KEY.to_string()));
        assert!(delete.label().contains("y = delete"));
    }

    #[test]
    fn a_message_tree_is_one_paragraph() {
        let tree = message_tree("Loading board...");
        assert_well_formed(&tree);
        assert_eq!(tree.nodes.len(), 1);
    }
}
