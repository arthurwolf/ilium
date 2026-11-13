//! The general renderer of plugin views (foundation contract section 19.1).
//!
//! A plugin describes a panel, a settings page or an onboarding step with a `UiTree`. This
//! module turns such a tree and a rectangle into a `Scene`: lines of text at fixed places,
//! frames, and the regions that answer a click or the keyboard with a `ui-action`. Composing is
//! pure, so drawing and hit-testing always agree; `draw` only paints a scene.
//!
//! The kernel keeps the cursor (`ViewCursor`); the plugin keeps every value. Text from a plugin
//! is sanitized before it reaches a cell. Widgets outside the supported subset contribute only
//! their children and are named in `Scene::skipped`, so a view never fails as a whole.

use crossterm::event::KeyCode;
use ilium_plugin_api::generated::types::layout::{Constraint, Direction};
use ilium_plugin_api::generated::types::text::{
    Color as PluginColor, Modifiers, RichSpan, Span as PluginSpan, Style as PluginStyle, Tone,
    TriState,
};
use ilium_plugin_api::generated::types::ui::{
    ActionKind, ActionValue, ButtonRole, ChoiceStyle, NodeFlags, ProgressState, UiAction, UiNode,
    UiTree, ViewTarget, Widget,
};
use ilium_plugin_host::render::sanitize_plugin_text;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::widget_rig::solve_row;
use crate::theme;
use crate::value_control::leader_span;

/// Longest text of one span, in characters, after sanitizing.
const SPAN_LIMIT: usize = 4096;
/// Deepest nesting the composer follows; a deeper tree is cut there.
const DEPTH_LIMIT: usize = 32;
/// The key dialog buttons report under when the dialog node has no key.
const DIALOG_KEY: &str = "dialog";

/// A region that answers a click, or Enter and Space when it has the cursor.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hit {
    pub area: Rect,
    pub key: String,
    pub action_id: String,
    pub kind: ActionKind,
    pub value: Option<ActionValue>,
}

/// A view laid out for one rectangle.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Scene {
    /// Lines of text, each with the one-row rectangle it is clipped to.
    pub texts: Vec<(Rect, Line<'static>)>,
    /// Bordered frames with their titles.
    pub frames: Vec<(Rect, String)>,
    /// Regions that send an action, in reading order.
    pub hits: Vec<Hit>,
    /// WIT names of widgets this renderer does not draw yet.
    pub skipped: Vec<&'static str>,
}

/// Where the keyboard cursor of a view sits: an index into `Scene::hits`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ViewCursor {
    pub focus: usize,
}

fn clean(text: &str) -> String {
    sanitize_plugin_text(text, SPAN_LIMIT)
}

fn plugin_color(color: &PluginColor) -> Option<Color> {
    match color {
        PluginColor::Rgb(rgb) => {
            let [_, red, green, blue] = rgb.to_be_bytes();
            Some(Color::Rgb(red, green, blue))
        }
        PluginColor::Indexed(index) => Some(Color::Indexed(*index)),
        // Theme roles are not mapped yet: the text keeps the colour of its surroundings.
        PluginColor::Role(_) => None,
    }
}

fn modifier(flags: Modifiers) -> Modifier {
    let mut modifier = Modifier::empty();
    for (flag, mapped) in [
        (Modifiers::BOLD, Modifier::BOLD),
        (Modifiers::DIM, Modifier::DIM),
        (Modifiers::ITALIC, Modifier::ITALIC),
        (Modifiers::UNDERLINED, Modifier::UNDERLINED),
    ] {
        if flags.0 & flag.0 != 0 {
            modifier |= mapped;
        }
    }
    modifier
}

fn style(style: &PluginStyle) -> Style {
    let mut mapped = Style::new()
        .add_modifier(modifier(style.add))
        .remove_modifier(modifier(style.remove));
    if let Some(color) = style.fg.as_ref().and_then(plugin_color) {
        mapped = mapped.fg(color);
    }
    if let Some(color) = style.bg.as_ref().and_then(plugin_color) {
        mapped = mapped.bg(color);
    }
    mapped
}

fn tone_style(tone: Tone) -> Style {
    match tone {
        Tone::Info | Tone::Success => Style::new(),
        Tone::Warning | Tone::Danger => Style::new().add_modifier(Modifier::BOLD),
        Tone::Muted => Style::new().add_modifier(Modifier::DIM),
    }
}

fn styled(spans: &[PluginSpan]) -> Line<'static> {
    Line::from(
        spans
            .iter()
            .map(|span| Span::styled(clean(&span.text), style(&span.style)))
            .collect::<Vec<_>>(),
    )
}

fn rich(spans: &[RichSpan]) -> Line<'static> {
    Line::from(
        spans
            .iter()
            .map(|span| Span::styled(clean(&span.text), style(&span.style)))
            .collect::<Vec<_>>(),
    )
}

fn plain(spans: &[PluginSpan]) -> String {
    clean(
        &spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
    )
}

fn rows(count: usize) -> u16 {
    u16::try_from(count).unwrap_or(u16::MAX)
}

fn is_hidden(node: &UiNode) -> bool {
    node.flags.0 & NodeFlags::HIDDEN.0 != 0
}

fn node(tree: &UiTree, index: u32) -> Option<&UiNode> {
    tree.nodes.get(usize::try_from(index).ok()?)
}

fn key_of(node: &UiNode) -> String {
    node.key.clone().unwrap_or_default()
}

/// The rows a node wants at a given width.
fn measure(tree: &UiTree, index: u32, width: u16, depth: usize) -> u16 {
    let Some(node) = node(tree, index).filter(|node| !is_hidden(node)) else {
        return 0;
    };
    if depth > DEPTH_LIMIT {
        return 0;
    }
    let children = |width: u16| -> u16 {
        node.children
            .iter()
            .map(|child| measure(tree, *child, width, depth + 1))
            .fold(0u16, u16::saturating_add)
    };
    match &node.widget {
        Widget::Layout(layout) => match layout.direction {
            Direction::Vertical => children(width),
            Direction::Horizontal => {
                let share = width / rows(node.children.len()).max(1);
                node.children
                    .iter()
                    .map(|child| measure(tree, *child, share, depth + 1))
                    .max()
                    .unwrap_or(0)
            }
        },
        Widget::Block(block) => {
            let border = if block.borders.0 != 0 { 2 } else { 0 };
            children(width.saturating_sub(border))
                .saturating_add(border)
                .saturating_add(block.padding.top)
                .saturating_add(block.padding.bottom)
        }
        Widget::Dialog(dialog) => 1u16
            .saturating_add(rows(dialog.message.as_ref().map_or(0, Vec::len)))
            .saturating_add(children(width))
            .saturating_add(u16::from(!dialog.buttons.is_empty())),
        Widget::Paragraph(paragraph) => match &paragraph.rich {
            Some(lines) if paragraph.lines.is_empty() => rows(lines.len()),
            _ => rows(paragraph.lines.len()),
        },
        Widget::Markdown(markdown) => rows(markdown.source.lines().count()),
        Widget::List(list) if list.items.is_empty() => {
            rows(list.empty.as_ref().map_or(0, Vec::len))
        }
        Widget::List(list) => rows(
            list.items
                .iter()
                .map(|item| item.lines.len().max(1))
                .sum::<usize>(),
        ),
        Widget::Table(table) => rows(table.rows.len()).saturating_add(1),
        Widget::Menu(menu) => rows(menu.items.len()),
        Widget::CheckList(list) => rows(list.items.len()),
        Widget::Choice(choice) => match choice.style {
            ChoiceStyle::Stepper => 1,
            ChoiceStyle::List | ChoiceStyle::Dialog => {
                rows(choice.options.len()).saturating_add(u16::from(!choice.label.is_empty()))
            }
        },
        Widget::Notice(notice) => {
            rows(notice.text.len()).saturating_add(u16::from(notice.title.is_some()))
        }
        Widget::ProgressRow(progress) => 1 + u16::from(progress.message.is_some()),
        Widget::Separator(_)
        | Widget::LeaderRow(_)
        | Widget::KeyHints(_)
        | Widget::Tabs(_)
        | Widget::Gauge(_)
        | Widget::LineGauge(_)
        | Widget::Button(_)
        | Widget::Toggle(_)
        | Widget::Number(_)
        | Widget::TextInput(_)
        | Widget::Badge(_)
        | Widget::Spinner(_)
        | Widget::Icon(_) => 1,
        // Containers and widgets this renderer does not draw take the room of their children.
        _ => children(width),
    }
}

struct Composer<'a> {
    tree: &'a UiTree,
    scene: Scene,
}

impl Composer<'_> {
    fn text(&mut self, area: Rect, row: u16, line: Line<'static>) {
        if row < area.height && area.width > 0 {
            self.scene
                .texts
                .push((Rect::new(area.x, area.y + row, area.width, 1), line));
        }
    }

    fn hit(
        &mut self,
        area: Rect,
        row: u16,
        key: &str,
        action_id: &str,
        kind: ActionKind,
        value: Option<ActionValue>,
    ) {
        // A widget without a key cannot name itself in an action, so it takes no input.
        if row < area.height && area.width > 0 && !key.is_empty() {
            self.scene.hits.push(Hit {
                area: Rect::new(area.x, area.y + row, area.width, 1),
                key: key.to_owned(),
                action_id: action_id.to_owned(),
                kind,
                value,
            });
        }
    }

    /// Stacks the children of a node top to bottom, each with the rows it wants.
    fn stack(&mut self, children: &[u32], area: Rect, depth: usize) {
        let mut top = 0u16;
        for child in children {
            let wanted = measure(self.tree, *child, area.width, depth + 1);
            let height = wanted.min(area.height.saturating_sub(top));
            if height == 0 {
                continue;
            }
            self.place(
                *child,
                Rect::new(area.x, area.y + top, area.width, height),
                depth + 1,
            );
            top = top.saturating_add(height);
        }
    }

    fn layout(
        &mut self,
        direction: Direction,
        constraints: &[Constraint],
        children: &[u32],
        area: Rect,
        depth: usize,
    ) {
        // A layout whose constraints do not match its children falls back to natural sizes.
        let solved = constraints.len() == children.len() && !children.is_empty();
        match direction {
            Direction::Vertical if !solved => self.stack(children, area, depth),
            Direction::Vertical => {
                let mut top = area.y;
                for (child, height) in children.iter().zip(solve_row(area.height, constraints)) {
                    self.place(
                        *child,
                        Rect::new(area.x, top, area.width, height),
                        depth + 1,
                    );
                    top = top.saturating_add(height);
                }
            }
            Direction::Horizontal => {
                let widths = if solved {
                    solve_row(area.width, constraints)
                } else {
                    let even = vec![Constraint::Fill(1); children.len()];
                    solve_row(area.width, &even)
                };
                let mut left = area.x;
                for (child, width) in children.iter().zip(widths) {
                    self.place(
                        *child,
                        Rect::new(left, area.y, width, area.height),
                        depth + 1,
                    );
                    left = left.saturating_add(width);
                }
            }
        }
    }

    fn place(&mut self, index: u32, area: Rect, depth: usize) {
        let tree = self.tree;
        let Some(node) = node(tree, index).filter(|node| !is_hidden(node)) else {
            return;
        };
        if depth > DEPTH_LIMIT || area.width == 0 || area.height == 0 {
            return;
        }
        let key = key_of(node);
        let enabled = node.disabled_reason.is_none();
        match &node.widget {
            Widget::Layout(layout) => self.layout(
                layout.direction,
                &layout.constraints,
                &node.children,
                area,
                depth,
            ),
            Widget::Block(block) => {
                let border = u16::from(block.borders.0 != 0);
                if border == 1 {
                    let title = block
                        .titles
                        .first()
                        .map(|title| plain(&title.text))
                        .unwrap_or_default();
                    self.scene.frames.push((area, title));
                }
                let left = border.saturating_add(block.padding.left);
                let top = border.saturating_add(block.padding.top);
                let inner = Rect::new(
                    area.x.saturating_add(left),
                    area.y.saturating_add(top),
                    area.width
                        .saturating_sub(left)
                        .saturating_sub(border.saturating_add(block.padding.right)),
                    area.height
                        .saturating_sub(top)
                        .saturating_sub(border.saturating_add(block.padding.bottom)),
                );
                self.stack(&node.children, inner, depth);
            }
            Widget::Dialog(dialog) => {
                let mut row = 0u16;
                self.text(area, row, styled(&dialog.title).patch_style(Modifier::BOLD));
                row += 1;
                for line in dialog.message.iter().flatten() {
                    self.text(area, row, styled(line));
                    row = row.saturating_add(1);
                }
                let buttons = u16::from(!dialog.buttons.is_empty());
                let body = Rect::new(
                    area.x,
                    area.y.saturating_add(row),
                    area.width,
                    area.height.saturating_sub(row).saturating_sub(buttons),
                );
                self.stack(&node.children, body, depth);
                if buttons == 1 {
                    let dialog_key = if key.is_empty() { DIALOG_KEY } else { &key };
                    let mut left = area.x;
                    let bottom = area.height - 1;
                    for button in &dialog.buttons {
                        let label = format!("[ {} ]", clean(&button.label));
                        let width = rows(unicode_width::UnicodeWidthStr::width(label.as_str()))
                            .min(area.right().saturating_sub(left));
                        let place = Rect::new(left, area.y, width, area.height);
                        self.text(place, bottom, Line::from(label));
                        self.hit(
                            place,
                            bottom,
                            dialog_key,
                            &button.id,
                            ActionKind::Activated,
                            None,
                        );
                        left = left.saturating_add(width).saturating_add(1);
                    }
                }
            }
            Widget::Paragraph(paragraph) => {
                let base = style(&paragraph.style);
                match &paragraph.rich {
                    Some(lines) if paragraph.lines.is_empty() => {
                        for (row, line) in lines.iter().enumerate() {
                            self.text(area, rows(row), rich(line).patch_style(base));
                        }
                    }
                    _ => {
                        for (row, line) in paragraph.lines.iter().enumerate() {
                            self.text(area, rows(row), styled(line).patch_style(base));
                        }
                    }
                }
            }
            // Markdown is shown as its source text until the client's Markdown view is shared.
            Widget::Markdown(markdown) => {
                for (row, line) in markdown.source.lines().enumerate() {
                    self.text(area, rows(row), Line::from(clean(line)));
                }
            }
            Widget::Separator(separator) => {
                let label = separator.label.as_deref().map(plain).unwrap_or_default();
                let used = unicode_width::UnicodeWidthStr::width(label.as_str());
                let rule = "─".repeat(usize::from(area.width).saturating_sub(used));
                self.text(
                    area,
                    0,
                    Line::from(vec![
                        Span::raw(label),
                        Span::styled(rule, Style::new().add_modifier(Modifier::DIM)),
                    ]),
                );
            }
            Widget::LeaderRow(leader) => {
                let label = styled(&leader.label);
                let value = styled(&leader.value);
                let gap = usize::from(area.width)
                    .saturating_sub(label.width())
                    .saturating_sub(value.width());
                let mut spans = label.spans;
                spans.push(leader_span(gap));
                spans.extend(value.spans);
                self.text(area, 0, Line::from(spans));
            }
            Widget::KeyHints(hints) => {
                let text = hints
                    .iter()
                    .map(|hint| format!("{} {}", clean(&hint.keys), clean(&hint.label)))
                    .collect::<Vec<_>>()
                    .join("   ");
                self.text(
                    area,
                    0,
                    Line::styled(text, Style::new().add_modifier(Modifier::DIM)),
                );
            }
            Widget::Tabs(tabs) => {
                let mut left = area.x;
                for (title, index) in tabs.titles.iter().zip(0u32..) {
                    let line = styled(title);
                    let width = rows(line.width()).min(area.right().saturating_sub(left));
                    let place = Rect::new(left, area.y, width, area.height);
                    self.text(place, 0, line);
                    self.hit(
                        place,
                        0,
                        &key,
                        "",
                        ActionKind::Selected,
                        Some(ActionValue::Index(index)),
                    );
                    left = left.saturating_add(width).saturating_add(2);
                }
            }
            Widget::List(list) if list.items.is_empty() => {
                for (row, line) in list.empty.iter().flatten().enumerate() {
                    self.text(area, rows(row), styled(line));
                }
            }
            Widget::List(list) => {
                let mut row = 0u16;
                for (item, index) in list.items.iter().zip(0u32..) {
                    let base = style(&item.style);
                    if item.lines.is_empty() {
                        row = row.saturating_add(1);
                        continue;
                    }
                    for line in &item.lines {
                        self.text(area, row, styled(line).patch_style(base));
                        if item.disabled_reason.is_none() && enabled {
                            self.hit(
                                area,
                                row,
                                &key,
                                item.id.as_deref().unwrap_or_default(),
                                ActionKind::Selected,
                                Some(ActionValue::Index(index)),
                            );
                        }
                        row = row.saturating_add(1);
                    }
                }
            }
            Widget::Table(table) => {
                let gap = " ".repeat(usize::from(table.column_spacing.max(1)));
                let header = table
                    .columns
                    .iter()
                    .map(|column| plain(&column.title))
                    .collect::<Vec<_>>()
                    .join(&gap);
                self.text(
                    area,
                    0,
                    Line::styled(header, Style::new().add_modifier(Modifier::BOLD)),
                );
                for (table_row, index) in table.rows.iter().zip(0u32..) {
                    let row = rows(usize::try_from(index).unwrap_or(usize::MAX)).saturating_add(1);
                    let cells = table_row
                        .cells
                        .iter()
                        .map(|cell| cell.first().map(|line| plain(line)).unwrap_or_default())
                        .collect::<Vec<_>>()
                        .join(&gap);
                    self.text(area, row, Line::styled(cells, style(&table_row.style)));
                    self.hit(
                        area,
                        row,
                        &key,
                        table_row.id.as_deref().unwrap_or_default(),
                        ActionKind::Selected,
                        Some(ActionValue::Index(index)),
                    );
                }
            }
            Widget::Menu(menu) => {
                for (row, item) in menu.items.iter().enumerate() {
                    let shortcut = item.shortcut.as_deref().map(clean).unwrap_or_default();
                    let label = clean(&item.label);
                    let gap = usize::from(area.width)
                        .saturating_sub(unicode_width::UnicodeWidthStr::width(label.as_str()))
                        .saturating_sub(unicode_width::UnicodeWidthStr::width(shortcut.as_str()));
                    let item_style = if item.enabled {
                        Style::new()
                    } else {
                        Style::new().add_modifier(Modifier::DIM)
                    };
                    self.text(
                        area,
                        rows(row),
                        Line::styled(format!("{label}{}{shortcut}", " ".repeat(gap)), item_style),
                    );
                    if item.enabled && enabled {
                        self.hit(area, rows(row), &key, &item.id, ActionKind::Activated, None);
                    }
                }
            }
            Widget::CheckList(list) => {
                for (row, item) in list.items.iter().enumerate() {
                    let checkbox = match item.state {
                        TriState::On => "[x]",
                        TriState::Mixed => "[-]",
                        TriState::Off => "[ ]",
                    };
                    let usable = item.disabled_reason.is_none() && enabled;
                    let item_style = if usable {
                        Style::new()
                    } else {
                        Style::new().add_modifier(Modifier::DIM)
                    };
                    self.text(
                        area,
                        rows(row),
                        Line::styled(format!("{checkbox} {}", plain(&item.label)), item_style),
                    );
                    if usable {
                        let next = if item.state == TriState::On {
                            TriState::Off
                        } else {
                            TriState::On
                        };
                        self.hit(
                            area,
                            rows(row),
                            &key,
                            "",
                            ActionKind::Toggled,
                            Some(ActionValue::ItemState((item.id.clone(), next))),
                        );
                    }
                }
            }
            Widget::Choice(choice) => {
                let count = u32::try_from(choice.options.len()).unwrap_or(u32::MAX);
                match choice.style {
                    ChoiceStyle::Stepper => {
                        let current = usize::try_from(choice.selected)
                            .ok()
                            .and_then(|selected| choice.options.get(selected))
                            .map(|option| clean(option))
                            .unwrap_or_default();
                        let value = format!("‹ {current} ›");
                        let label = clean(&choice.label);
                        let gap = usize::from(area.width)
                            .saturating_sub(unicode_width::UnicodeWidthStr::width(label.as_str()))
                            .saturating_sub(unicode_width::UnicodeWidthStr::width(value.as_str()));
                        self.text(
                            area,
                            0,
                            Line::from(vec![Span::raw(label), leader_span(gap), Span::raw(value)]),
                        );
                        if count > 1 && enabled {
                            self.hit(
                                area,
                                0,
                                &key,
                                "",
                                ActionKind::Changed,
                                Some(ActionValue::Index((choice.selected + 1) % count)),
                            );
                        }
                    }
                    ChoiceStyle::List | ChoiceStyle::Dialog => {
                        let mut row = 0u16;
                        if !choice.label.is_empty() {
                            self.text(area, row, Line::from(clean(&choice.label)));
                            row += 1;
                        }
                        for (option, index) in choice.options.iter().zip(0u32..) {
                            let selected = index == choice.selected;
                            let radio = if selected { "(•)" } else { "( )" };
                            self.text(area, row, Line::from(format!("{radio} {}", clean(option))));
                            // The option already selected takes no click, as in a popup.
                            if !selected && enabled {
                                self.hit(
                                    area,
                                    row,
                                    &key,
                                    "",
                                    ActionKind::Changed,
                                    Some(ActionValue::Index(index)),
                                );
                            }
                            row = row.saturating_add(1);
                        }
                    }
                }
            }
            Widget::Toggle(toggle) => {
                let value = if toggle.value { "[x]" } else { "[ ]" };
                let label = clean(&toggle.label);
                let gap = usize::from(area.width)
                    .saturating_sub(unicode_width::UnicodeWidthStr::width(label.as_str()))
                    .saturating_sub(3);
                self.text(
                    area,
                    0,
                    Line::from(vec![Span::raw(label), leader_span(gap), Span::raw(value)]),
                );
                if enabled {
                    self.hit(
                        area,
                        0,
                        &key,
                        "",
                        ActionKind::Toggled,
                        Some(ActionValue::Flag(!toggle.value)),
                    );
                }
            }
            Widget::Button(button) => {
                let label = format!("[ {} ]", clean(&button.label));
                let width =
                    rows(unicode_width::UnicodeWidthStr::width(label.as_str())).min(area.width);
                let usable = button.enabled && enabled;
                let button_style = match (usable, button.role) {
                    (false, _) => Style::new().add_modifier(Modifier::DIM),
                    (true, ButtonRole::Primary | ButtonRole::Danger) => {
                        Style::new().add_modifier(Modifier::BOLD)
                    }
                    (true, ButtonRole::Secondary) => Style::new(),
                };
                let place = Rect::new(area.x, area.y, width, area.height);
                self.text(place, 0, Line::styled(label, button_style));
                if usable {
                    self.hit(place, 0, &key, "", ActionKind::Activated, None);
                }
            }
            // Values are shown; editing them in place is not built yet.
            Widget::Number(number) => {
                let unit = number.unit.as_deref().map(clean).unwrap_or_default();
                let value = format!("{}{unit}", number.value);
                let label = clean(&number.label);
                let gap = usize::from(area.width)
                    .saturating_sub(unicode_width::UnicodeWidthStr::width(label.as_str()))
                    .saturating_sub(unicode_width::UnicodeWidthStr::width(value.as_str()));
                self.text(
                    area,
                    0,
                    Line::from(vec![Span::raw(label), leader_span(gap), Span::raw(value)]),
                );
                self.scene.skipped.push("number (read-only)");
            }
            Widget::TextInput(input) => {
                let value = if input.secret {
                    "•".repeat(input.initial.chars().count().min(16))
                } else {
                    clean(&input.initial)
                };
                let label = clean(&input.label);
                let gap = usize::from(area.width)
                    .saturating_sub(unicode_width::UnicodeWidthStr::width(label.as_str()))
                    .saturating_sub(unicode_width::UnicodeWidthStr::width(value.as_str()));
                self.text(
                    area,
                    0,
                    Line::from(vec![Span::raw(label), leader_span(gap), Span::raw(value)]),
                );
                self.scene.skipped.push("text-input (read-only)");
            }
            Widget::Badge(badge) => {
                let mut text = badge.label.as_deref().map(clean).unwrap_or_default();
                if let Some(count) = badge.count {
                    text = format!("{text} {count}").trim().to_owned();
                }
                if badge.dot && text.is_empty() {
                    text = "•".to_owned();
                }
                self.text(area, 0, Line::styled(text, tone_style(badge.tone)));
            }
            Widget::Spinner(spinner) => {
                let label = spinner.label.as_deref().map(plain).unwrap_or_default();
                self.text(area, 0, Line::from(format!("… {label}")));
            }
            Widget::Notice(notice) => {
                let tone = tone_style(notice.tone);
                let mut row = 0u16;
                if let Some(title) = &notice.title {
                    self.text(
                        area,
                        row,
                        Line::styled(clean(title), tone.add_modifier(Modifier::BOLD)),
                    );
                    row += 1;
                }
                for line in &notice.text {
                    self.text(area, row, rich(line).patch_style(tone));
                    row = row.saturating_add(1);
                }
            }
            Widget::Gauge(gauge) => {
                let label = gauge.label.as_deref().map(plain).unwrap_or_default();
                self.text(area, 0, gauge_line(gauge.ratio, &label, area.width));
            }
            Widget::LineGauge(gauge) => {
                let label = gauge.label.as_deref().map(plain).unwrap_or_default();
                self.text(area, 0, gauge_line(gauge.ratio, &label, area.width));
            }
            Widget::ProgressRow(progress) => {
                let mark = match progress.state {
                    ProgressState::Waiting => "·",
                    ProgressState::Running => "…",
                    ProgressState::Done => "✓",
                    ProgressState::Error => "✗",
                };
                let percent = progress
                    .percent
                    .map(|percent| format!(" {percent:.0}%"))
                    .unwrap_or_default();
                self.text(
                    area,
                    0,
                    Line::from(format!("{mark} {}{percent}", plain(&progress.title))),
                );
                if let Some(message) = &progress.message {
                    self.text(
                        area,
                        1,
                        Line::styled(clean(message), Style::new().add_modifier(Modifier::DIM)),
                    );
                }
            }
            Widget::Icon(icon) => self.text(area, 0, Line::from(clean(&icon.name))),
            Widget::Clear => {}
            // Scroll views, popups and splits show their children stacked, unscrolled.
            other => {
                if !matches!(
                    other,
                    Widget::ScrollView(_) | Widget::Popup(_) | Widget::Split(_)
                ) {
                    self.scene.skipped.push(other.case_name());
                }
                self.stack(&node.children, area, depth);
            }
        }
    }
}

fn gauge_line(ratio: f64, label: &str, width: u16) -> Line<'static> {
    let ratio = if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let label_width = unicode_width::UnicodeWidthStr::width(label);
    let cells = usize::from(width).saturating_sub(label_width + 1);
    // The product is within 0..=cells, so the conversion cannot truncate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let filled = ((cells as f64) * ratio).round() as usize;
    Line::from(vec![
        Span::raw("█".repeat(filled.min(cells))),
        Span::styled(
            "░".repeat(cells.saturating_sub(filled)),
            Style::new().add_modifier(Modifier::DIM),
        ),
        Span::raw(format!(" {label}")),
    ])
}

/// Lays a view out for a rectangle. A tree whose root does not exist gives an empty scene.
pub(crate) fn compose(tree: &UiTree, area: Rect) -> Scene {
    let mut composer = Composer {
        tree,
        scene: Scene::default(),
    };
    composer.place(tree.root, area, 0);
    composer.scene
}

/// Paints a scene; the hit under the cursor is highlighted when the view has the keyboard.
pub(crate) fn draw(frame: &mut Frame, scene: &Scene, cursor: ViewCursor, focused: bool) {
    for (area, title) in &scene.frames {
        frame.render_widget(theme::block(false).title(theme::chrome_title(title)), *area);
    }
    for (area, line) in &scene.texts {
        frame.render_widget(Paragraph::new(line.clone()), *area);
    }
    if focused {
        if let Some(hit) = scene.hits.get(cursor.focus) {
            frame
                .buffer_mut()
                .set_style(hit.area, theme::selected_style());
        }
    }
}

fn action(view: &ViewTarget, hit: &Hit) -> UiAction {
    UiAction {
        view: view.clone(),
        key: hit.key.clone(),
        action_id: hit.action_id.clone(),
        kind: hit.kind,
        value: hit.value.clone(),
    }
}

/// The action of a click, if it lands on a region that takes one. The cursor follows the click.
pub(crate) fn click(
    scene: &Scene,
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    position: Position,
) -> Option<UiAction> {
    let index = scene
        .hits
        .iter()
        .position(|hit| hit.area.contains(position))?;
    cursor.focus = index;
    scene.hits.get(index).map(|hit| action(view, hit))
}

/// One key on a view: Up, Down, Tab and Shift+Tab move the cursor, Enter and Space send the
/// action of the region under it. Every other key does nothing here.
pub(crate) fn key(
    scene: &Scene,
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    key: KeyCode,
) -> Option<UiAction> {
    let last = scene.hits.len().saturating_sub(1);
    cursor.focus = cursor.focus.min(last);
    match key {
        KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
            cursor.focus = (cursor.focus + 1).min(last);
            None
        }
        KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
            cursor.focus = cursor.focus.saturating_sub(1);
            None
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            scene.hits.get(cursor.focus).map(|hit| action(view, hit))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_api::generated::types::layout::{Alignment, Flex, Margin, Spacing};
    use ilium_plugin_api::generated::types::ui::{
        ButtonSpec, ChoiceSpec, DisabledMarker, LayoutSpec, ParagraphSpec, ToggleSpec, ToggleStyle,
    };

    fn node(key: Option<&str>, widget: Widget, children: Vec<u32>) -> UiNode {
        UiNode {
            key: key.map(str::to_owned),
            widget,
            children,
            flags: NodeFlags::default(),
            tooltip: None,
            tab_index: None,
            disabled_reason: None,
        }
    }

    fn span(text: &str) -> Vec<PluginSpan> {
        vec![PluginSpan {
            text: text.to_owned(),
            style: PluginStyle::default(),
        }]
    }

    fn sample() -> UiTree {
        let paragraph = ParagraphSpec {
            lines: vec![span("Hello\u{1b}[31m")],
            rich: None,
            wrap: None,
            alignment: Alignment::Left,
            style: PluginStyle::default(),
            scrollable: false,
        };
        let choice = ChoiceSpec {
            label: String::new(),
            options: vec!["One".to_owned(), "Two".to_owned()],
            selected: 0,
            style: ChoiceStyle::List,
            items: Vec::new(),
            source: None,
            selected_id: None,
            filter: false,
            view_modes: Vec::new(),
            disabled_marker: DisabledMarker::Reason,
            allow_custom: false,
        };
        let layout = LayoutSpec {
            direction: Direction::Vertical,
            constraints: Vec::new(),
            flex: Flex::Start,
            spacing: Spacing::Space(0),
            margin: Margin::default(),
        };
        UiTree {
            version: 1,
            root: 0,
            nodes: vec![
                node(None, Widget::Layout(layout), vec![1, 2, 3, 4]),
                node(None, Widget::Paragraph(paragraph), vec![]),
                node(Some("pick"), Widget::Choice(choice), vec![]),
                node(
                    Some("flag"),
                    Widget::Toggle(ToggleSpec {
                        label: "Flag".to_owned(),
                        value: false,
                        style: ToggleStyle::Checkbox,
                        tri_state: false,
                        icon_on: None,
                        icon_off: None,
                    }),
                    vec![],
                ),
                node(
                    Some("go"),
                    Widget::Button(ButtonSpec {
                        label: "Go".to_owned(),
                        icon: None,
                        enabled: true,
                        role: ButtonRole::Primary,
                    }),
                    vec![],
                ),
            ],
        }
    }

    #[test]
    fn a_view_is_stacked_top_to_bottom_and_its_text_is_sanitized() {
        let view = ViewTarget::Panel("panel".to_owned());
        let scene = compose(&sample(), Rect::new(2, 1, 30, 10));
        let first = &scene.texts[0];
        assert_eq!(first.0, Rect::new(2, 1, 30, 1));
        assert_eq!(first.1.to_string(), "Hello");
        // Paragraph, two options, toggle, button: five rows.
        assert_eq!(scene.texts.len(), 5);
        assert_eq!(scene.texts[4].0.y, 5);
    }

    #[test]
    fn a_click_sends_the_action_of_the_widget_under_it() {
        let view = ViewTarget::Panel("panel".to_owned());
        let scene = compose(&sample(), Rect::new(0, 0, 30, 10));
        let mut cursor = ViewCursor::default();
        // The selected option takes no click; the other one changes the choice.
        assert_eq!(click(&scene, &view, &mut cursor, Position::new(1, 1)), None);
        let changed = click(&scene, &view, &mut cursor, Position::new(1, 2)).expect("an action");
        assert_eq!(changed.key, "pick");
        assert_eq!(changed.kind, ActionKind::Changed);
        assert_eq!(changed.value, Some(ActionValue::Index(1)));
        let toggled = click(&scene, &view, &mut cursor, Position::new(1, 3)).expect("an action");
        assert_eq!(toggled.value, Some(ActionValue::Flag(true)));
        let pressed = click(&scene, &view, &mut cursor, Position::new(1, 4)).expect("an action");
        assert_eq!(pressed.key, "go");
        assert_eq!(pressed.kind, ActionKind::Activated);
        assert_eq!(pressed.view, view);
    }

    #[test]
    fn the_keyboard_walks_the_regions_and_enter_sends_the_focused_one() {
        let view = ViewTarget::SettingsPage("page".to_owned());
        let scene = compose(&sample(), Rect::new(0, 0, 30, 10));
        let mut cursor = ViewCursor::default();
        assert_eq!(key(&scene, &view, &mut cursor, KeyCode::Down), None);
        assert_eq!(key(&scene, &view, &mut cursor, KeyCode::Down), None);
        assert_eq!(key(&scene, &view, &mut cursor, KeyCode::Down), None);
        assert_eq!(cursor.focus, 2);
        let pressed = key(&scene, &view, &mut cursor, KeyCode::Enter).expect("an action");
        assert_eq!(pressed.key, "go");
    }

    #[test]
    fn a_view_without_room_or_without_a_root_draws_nothing() {
        let view = ViewTarget::Panel("panel".to_owned());
        assert_eq!(compose(&sample(), Rect::new(0, 0, 0, 0)), Scene::default());
        let empty = UiTree {
            version: 1,
            root: 7,
            nodes: Vec::new(),
        };
        assert_eq!(compose(&empty, Rect::new(0, 0, 10, 10)), Scene::default());
    }
}
