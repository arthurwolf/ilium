//! The general renderer of plugin views (foundation contract section 19.1).
//!
//! A plugin describes a panel, a settings page or an onboarding step with a `UiTree`. This
//! module turns such a tree and a rectangle into a `Scene`: lines of text at fixed places,
//! frames, and the regions that answer a click or the keyboard with a `ui-action`. Composing is
//! pure, so drawing and hit-testing always agree; `draw` only paints a scene.
//!
//! The kernel keeps the cursor (`ViewCursor`) and, after the first interaction, the contents of
//! the editable widgets (text inputs, numbers, scroll offsets and split positions, see
//! `ui_state`); the plugin keeps every other value. Text from a plugin is sanitized before it
//! reaches a cell. Widgets outside the supported subset contribute only their children and are
//! named in `Scene::skipped`, so a view never fails as a whole. A popup draws nothing inline:
//! it floats above the view and is shown by `popup`.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};
use ilium_plugin_api::generated::types::layout::{Constraint, Direction};
use ilium_plugin_api::generated::types::text::{
    Color as PluginColor, Modifiers, RichSpan, Span as PluginSpan, Style as PluginStyle, Tone,
    TriState,
};
use ilium_plugin_api::generated::types::ui::{
    ActionKind, ActionValue, ButtonRole, ChoiceStyle, NodeFlags, NumberSpec, NumberStyle,
    ProgressState, ScrollViewSpec, SplitSpec, TextInputSpec, UiAction, UiNode, UiTree, ViewTarget,
    Widget,
};
use ilium_plugin_host::render::sanitize_plugin_text;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::ui_state::{
    bounds, format_number, scroll_offset, DueChange, EditOutcome, NumberEdit, NumberOutcome,
    ScrollState, TextEdit, WidgetStates,
};
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
    /// The scroll view that clips this region: its index in `Scene::scrolls` and the row of the
    /// region inside the scrolled content. An empty `area` means the region is scrolled out of view.
    pub scroll: Option<(usize, u16)>,
}

/// What an input slot edits.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InputKind {
    Text(TextInputSpec),
    Number(NumberSpec),
}

/// An editable widget of a scene, tied to the hit that gives it the keyboard.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InputSlot {
    /// Index into `Scene::hits`.
    pub hit: usize,
    pub key: String,
    /// The cells that show the value (the text field, the stepper, the slider track).
    pub field: Rect,
    pub kind: InputKind,
}

/// A scroll view of a scene.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScrollSlot {
    /// Empty for a scroll view without a key, which the user cannot scroll.
    pub key: String,
    /// The viewport with its scrollbar column; the wheel works inside it.
    pub area: Rect,
    pub bar: Option<Rect>,
    /// Rows of the viewport before any clipping by an outer scroll view.
    pub rows: u16,
    pub max_offset: u16,
    pub offset: u16,
}

/// A split of a scene.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SplitSlot {
    pub key: String,
    pub area: Rect,
    pub divider: Rect,
    /// The two children are stacked (the divider is a row), not side by side.
    pub stacked: bool,
    pub resizable: bool,
    pub min_first: u16,
    pub min_second: u16,
    pub position: u16,
    /// Index into `Scene::hits` of the focusable divider.
    pub hit: Option<usize>,
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
    /// Text inputs and numbers that take the keyboard.
    pub inputs: Vec<InputSlot>,
    pub scrolls: Vec<ScrollSlot>,
    pub splits: Vec<SplitSlot>,
}

/// A pointer drag in progress; it keeps what it needs, so moving needs no new scene.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Drag {
    Scrollbar {
        key: String,
        bar: Rect,
        max_offset: u16,
    },
    Split {
        key: String,
        area: Rect,
        stacked: bool,
        min_first: u16,
        min_second: u16,
        position: u16,
    },
    Slider {
        key: String,
        track: Rect,
        spec: NumberSpec,
    },
}

/// Where the keyboard cursor of a view sits (an index into `Scene::hits`), the contents of its
/// editable widgets and the drag in progress.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ViewCursor {
    pub focus: usize,
    pub states: WidgetStates,
    pub drag: Option<Drag>,
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
        Widget::ScrollView(spec) => match spec.content_rows {
            0 => children(width),
            rows => u16::try_from(rows).unwrap_or(u16::MAX),
        },
        Widget::Split(spec) => match spec.direction {
            Direction::Horizontal => node
                .children
                .iter()
                .map(|child| measure(tree, *child, width, depth + 1))
                .max()
                .unwrap_or(0),
            Direction::Vertical => children(width).saturating_add(1),
        },
        // A popup floats above the view and takes no room in it.
        Widget::Popup(_) => 0,
        // Containers and widgets this renderer does not draw take the room of their children.
        _ => children(width),
    }
}

struct Composer<'a> {
    tree: &'a UiTree,
    states: &'a WidgetStates,
    scene: Scene,
}

/// The lengths of the scene lists when a scroll view started placing its content.
struct Mark {
    texts: usize,
    frames: usize,
    hits: usize,
    inputs: usize,
    scrolls: usize,
    splits: usize,
}

/// The display width of a text in cells.
fn cells(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

/// The longest start of `text` that fits `room` cells.
fn fit(text: &str, room: usize) -> String {
    let mut used = 0usize;
    let mut fitted = String::new();
    for character in text.chars() {
        let width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if used + width > room {
            break;
        }
        used += width;
        fitted.push(character);
    }
    fitted
}

/// The cells of the first part of a split: `floor((span-1)*position/1000)`, kept inside the
/// minimums of both parts (the first minimum wins when they cannot both fit).
pub(crate) fn split_first(span: u16, position: u16, min_first: u16, min_second: u16) -> u16 {
    let available = u32::from(span.saturating_sub(1));
    let wanted = available * u32::from(position.min(1000)) / 1000;
    let low = u32::from(min_first);
    let high = available.saturating_sub(u32::from(min_second));
    let first = if low > high {
        low.min(available)
    } else {
        wanted.clamp(low, high)
    };
    u16::try_from(first).unwrap_or(u16::MAX)
}

/// The per mille position that makes the first part `first` cells of a split `span` cells long.
pub(crate) fn split_position(span: u16, first: u16) -> u16 {
    let available = u32::from(span.saturating_sub(1));
    if available == 0 {
        return 0;
    }
    let position = (u32::from(first) * 1000).div_ceil(available).min(1000);
    u16::try_from(position).unwrap_or(1000)
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
    ) -> Option<usize> {
        // A widget without a key cannot name itself in an action, so it takes no input.
        if row < area.height && area.width > 0 && !key.is_empty() {
            self.scene.hits.push(Hit {
                area: Rect::new(area.x, area.y + row, area.width, 1),
                key: key.to_owned(),
                action_id: action_id.to_owned(),
                kind,
                value,
                scroll: None,
            });
            Some(self.scene.hits.len() - 1)
        } else {
            None
        }
    }

    fn mark(&self) -> Mark {
        Mark {
            texts: self.scene.texts.len(),
            frames: self.scene.frames.len(),
            hits: self.scene.hits.len(),
            inputs: self.scene.inputs.len(),
            scrolls: self.scene.scrolls.len(),
            splits: self.scene.splits.len(),
        }
    }

    /// Moves everything placed since `mark` up by `offset` rows and cuts it to `viewport`. A
    /// region scrolled out of view keeps its place in the lists with an empty area, so the
    /// keyboard can still reach it and scroll it back.
    fn clip(&mut self, mark: &Mark, viewport: Rect, offset: u16, slot: usize) {
        let top = viewport.y.saturating_add(offset);
        let bottom = top.saturating_add(viewport.height);
        let window = |rect: Rect| -> Option<Rect> {
            let start = rect.y.max(top);
            let end = rect.bottom().min(bottom);
            (start < end).then(|| {
                Rect::new(
                    rect.x,
                    start.saturating_sub(offset),
                    rect.width,
                    end - start,
                )
            })
        };
        let texts = self.scene.texts.split_off(mark.texts);
        self.scene.texts.extend(
            texts
                .into_iter()
                .filter_map(|(rect, line)| window(rect).map(|rect| (rect, line))),
        );
        let frames = self.scene.frames.split_off(mark.frames);
        self.scene
            .frames
            .extend(frames.into_iter().filter_map(|(rect, title)| {
                (rect.y >= top && rect.bottom() <= bottom).then(|| {
                    (
                        Rect::new(rect.x, rect.y - offset, rect.width, rect.height),
                        title,
                    )
                })
            }));
        for hit in &mut self.scene.hits[mark.hits..] {
            if hit.scroll.is_none() {
                hit.scroll = Some((slot, hit.area.y.saturating_sub(viewport.y)));
            }
            hit.area = window(hit.area).unwrap_or_default();
        }
        for input in &mut self.scene.inputs[mark.inputs..] {
            input.field = window(input.field).unwrap_or_default();
        }
        for inner in &mut self.scene.scrolls[mark.scrolls..] {
            inner.area = window(inner.area).unwrap_or_default();
            inner.bar = inner.bar.and_then(window);
        }
        for inner in &mut self.scene.splits[mark.splits..] {
            inner.area = window(inner.area).unwrap_or_default();
            inner.divider = window(inner.divider).unwrap_or_default();
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

    /// A text input: label, leader and an underlined field that shows the edit state the user
    /// has built (the plugin's `initial` until the first interaction).
    fn text_input(&mut self, input: &TextInputSpec, key: &str, area: Rect, enabled: bool) {
        let field_width = (area.width / 2).max(10).min(area.width);
        let label_room = area.width - field_width;
        let label = fit(&clean(&input.label), usize::from(label_room));
        let gap = usize::from(label_room).saturating_sub(cells(&label));
        let field = Rect::new(area.x + label_room, area.y, field_width, 1);
        let seeded;
        let edit = match self.states.texts.get(key) {
            Some(edit) => edit,
            None => {
                seeded = TextEdit::seeded(&input.initial);
                &seeded
            }
        };
        let layout = edit.layout(input, usize::from(field_width));
        let mut base = Style::new().add_modifier(Modifier::UNDERLINED);
        if !enabled {
            base = base.add_modifier(Modifier::DIM);
        }
        if !edit.is_valid(input) {
            base = base.fg(Color::Red);
        }
        let mut spans = vec![Span::raw(label), leader_span(gap)];
        let mut used = 0usize;
        match input.placeholder.as_deref() {
            Some(hint) if edit.text().is_empty() => {
                let hint = fit(&clean(hint), usize::from(field_width));
                used = cells(&hint);
                spans.push(Span::styled(hint, base.add_modifier(Modifier::DIM)));
            }
            _ => {
                for cell in &layout.cells {
                    used += cells(&cell.text);
                    let cell_style = if cell.selected {
                        base.add_modifier(Modifier::REVERSED)
                    } else {
                        base
                    };
                    spans.push(Span::styled(cell.text.clone(), cell_style));
                }
            }
        }
        if used < usize::from(field_width) {
            spans.push(Span::styled(
                " ".repeat(usize::from(field_width) - used),
                base,
            ));
        }
        self.text(area, 0, Line::from(spans));
        if enabled {
            if let Some(hit) = self.hit(area, 0, key, "", ActionKind::Changed, None) {
                self.scene.inputs.push(InputSlot {
                    hit,
                    key: key.to_owned(),
                    field,
                    kind: InputKind::Text(input.clone()),
                });
            }
        }
    }

    /// A number: a stepper `‹ value ›` or a slider track, after the label and its leader.
    fn number(&mut self, spec: &NumberSpec, key: &str, area: Rect, enabled: bool) {
        let seeded;
        let edit = match self.states.numbers.get(key) {
            Some(edit) => edit,
            None => {
                seeded = NumberEdit::seeded(spec);
                &seeded
            }
        };
        let shown = match &edit.typed {
            Some(typed) => format!("{typed}_"),
            None => format_number(spec, edit.value),
        };
        let shown = clean(&shown);
        let tone = if enabled {
            Style::new()
        } else {
            Style::new().add_modifier(Modifier::DIM)
        };
        let label = clean(&spec.label);
        let (field, line) = match spec.style {
            NumberStyle::Stepper => {
                let value = fit(&format!("‹ {shown} ›"), usize::from(area.width));
                let field_width = rows(cells(&value));
                let label_room = area.width - field_width;
                let label = fit(&label, usize::from(label_room));
                let gap = usize::from(label_room).saturating_sub(cells(&label));
                (
                    Rect::new(area.x + label_room, area.y, field_width, 1),
                    Line::from(vec![
                        Span::raw(label),
                        leader_span(gap),
                        Span::styled(value, tone),
                    ]),
                )
            }
            NumberStyle::Slider => {
                let field_width = (area.width / 2).max(14).min(area.width);
                let label_room = area.width - field_width;
                let label = fit(&label, usize::from(label_room));
                let gap = usize::from(label_room).saturating_sub(cells(&label));
                let value = fit(&shown, usize::from(field_width).saturating_sub(2));
                let track = usize::from(field_width).saturating_sub(cells(&value) + 1);
                let (low, high) = bounds(spec);
                let ratio = if high > low {
                    ((edit.value - low) / (high - low)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let denominator =
                    f64::from(u32::try_from(track.saturating_sub(1)).unwrap_or(1)).max(1.0);
                let knob = (0..track)
                    .min_by(|left, right| {
                        let at = |cell: &usize| {
                            f64::from(u32::try_from(*cell).unwrap_or(0)) / denominator
                        };
                        (at(left) - ratio)
                            .abs()
                            .total_cmp(&(at(right) - ratio).abs())
                    })
                    .unwrap_or(0);
                let mut spans = vec![Span::raw(label), leader_span(gap)];
                if track > 0 {
                    spans.push(Span::styled("━".repeat(knob), tone));
                    spans.push(Span::styled("●", tone));
                    spans.push(Span::styled(
                        "─".repeat(track.saturating_sub(knob + 1)),
                        Style::new().add_modifier(Modifier::DIM),
                    ));
                }
                spans.push(Span::styled(format!(" {value}"), tone));
                (
                    Rect::new(area.x + label_room, area.y, rows(track), 1),
                    Line::from(spans),
                )
            }
        };
        self.text(area, 0, line);
        if enabled {
            if let Some(hit) = self.hit(area, 0, key, "", ActionKind::Changed, None) {
                self.scene.inputs.push(InputSlot {
                    hit,
                    key: key.to_owned(),
                    field,
                    kind: InputKind::Number(spec.clone()),
                });
            }
        }
    }

    /// A scroll view: the child is laid out at `content-rows` height (one column narrower while
    /// the scrollbar shows) and clipped to the area at the offset the host keeps.
    fn scroll_view(
        &mut self,
        spec: &ScrollViewSpec,
        key: &str,
        children: &[u32],
        area: Rect,
        depth: usize,
    ) {
        let bar = spec.show_scrollbar && area.width > 1;
        let width = area.width - u16::from(bar);
        let content = match spec.content_rows {
            0 => children
                .iter()
                .map(|child| measure(self.tree, *child, width, depth + 1))
                .fold(0u16, u16::saturating_add),
            rows => u16::try_from(rows).unwrap_or(u16::MAX),
        }
        .max(1);
        let max_offset = content.saturating_sub(area.height);
        let offset = scroll_offset(self.states.scrolls.get(key), spec.follow_tail, max_offset);
        let viewport = Rect::new(area.x, area.y, width, area.height);
        let slot = self.scene.scrolls.len();
        let mark = self.mark();
        self.stack(children, Rect::new(area.x, area.y, width, content), depth);
        self.clip(&mark, viewport, offset, slot);
        let bar_area = bar.then(|| Rect::new(area.x + width, area.y, 1, area.height));
        if let Some(bar_area) = bar_area {
            self.scrollbar(bar_area, content, offset);
        }
        self.scene.scrolls.push(ScrollSlot {
            key: key.to_owned(),
            area,
            bar: bar_area,
            rows: area.height,
            max_offset,
            offset,
        });
    }

    /// The scrollbar column: a muted track with a thumb sized to the visible share.
    fn scrollbar(&mut self, bar: Rect, content: u16, offset: u16) {
        let track = u32::from(bar.height);
        let total = u32::from(content).max(track);
        let thumb = (track * track / total).clamp(1, track);
        let travel = track - thumb;
        let max_offset = total - track;
        let start = if max_offset == 0 {
            0
        } else {
            u32::from(offset) * travel / max_offset
        };
        for row in 0..track {
            let on_thumb = row >= start && row < start + thumb;
            let (glyph, glyph_style) = if on_thumb {
                ("┃", Style::new())
            } else {
                ("│", Style::new().add_modifier(Modifier::DIM))
            };
            self.text(
                bar,
                u16::try_from(row).unwrap_or(u16::MAX),
                Line::styled(glyph, glyph_style),
            );
        }
    }

    /// A split: two children and a one-cell divider the user can drag when `resizable`.
    fn split(&mut self, spec: &SplitSpec, key: &str, children: &[u32], area: Rect, depth: usize) {
        let stacked = matches!(spec.direction, Direction::Vertical);
        let span = if stacked { area.height } else { area.width };
        let [first_child, second_child] = children else {
            self.stack(children, area, depth);
            return;
        };
        if span < 2 {
            return;
        }
        let position = self
            .states
            .splits
            .get(key)
            .copied()
            .unwrap_or(spec.position)
            .min(1000);
        let first = split_first(span, position, spec.min_first, spec.min_second);
        let second = span - 1 - first;
        let (first_area, divider, second_area) = if stacked {
            (
                Rect::new(area.x, area.y, area.width, first),
                Rect::new(area.x, area.y + first, area.width, 1),
                Rect::new(area.x, area.y + first + 1, area.width, second),
            )
        } else {
            (
                Rect::new(area.x, area.y, first, area.height),
                Rect::new(area.x + first, area.y, 1, area.height),
                Rect::new(area.x + first + 1, area.y, second, area.height),
            )
        };
        self.place(*first_child, first_area, depth + 1);
        self.place(*second_child, second_area, depth + 1);
        let rule = Style::new().add_modifier(Modifier::DIM);
        if stacked {
            self.text(
                divider,
                0,
                Line::styled("─".repeat(usize::from(divider.width)), rule),
            );
        } else {
            for row in 0..divider.height {
                self.text(divider, row, Line::styled("│", rule));
            }
        }
        let hit = (spec.resizable && !key.is_empty()).then(|| {
            self.scene.hits.push(Hit {
                area: divider,
                key: key.to_owned(),
                action_id: String::new(),
                kind: ActionKind::Resized,
                value: None,
                scroll: None,
            });
            self.scene.hits.len() - 1
        });
        self.scene.splits.push(SplitSlot {
            key: key.to_owned(),
            area,
            divider,
            stacked,
            resizable: spec.resizable,
            min_first: spec.min_first,
            min_second: spec.min_second,
            position,
            hit,
        });
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
            Widget::Number(number) => self.number(number, &key, area, enabled),
            Widget::TextInput(input) => self.text_input(input, &key, area, enabled),
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
            Widget::ScrollView(spec) => self.scroll_view(spec, &key, &node.children, area, depth),
            Widget::Split(spec) => self.split(spec, &key, &node.children, area, depth),
            // A popup floats above the view; `popup` shows it, so nothing is placed inline.
            Widget::Popup(_) => {}
            other => {
                self.scene.skipped.push(other.case_name());
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
    compose_with(tree, area, &WidgetStates::default())
}

/// Lays a view out with the editable state the user has built so far.
pub(crate) fn compose_with(tree: &UiTree, area: Rect, states: &WidgetStates) -> Scene {
    let mut composer = Composer {
        tree,
        states,
        scene: Scene::default(),
    };
    composer.place(tree.root, area, 0);
    composer.scene
}

/// Paints a scene; the hit under the cursor is highlighted when the view has the keyboard, and
/// the terminal caret sits in the text input that has it.
pub(crate) fn draw(frame: &mut Frame, scene: &Scene, cursor: &ViewCursor, focused: bool) {
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
        if let Some((slot, InputKind::Text(spec))) = scene
            .inputs
            .iter()
            .find(|slot| slot.hit == cursor.focus)
            .map(|slot| (slot, &slot.kind))
        {
            if slot.field.width > 0 && slot.field.height > 0 {
                let seeded;
                let edit = match cursor.states.texts.get(&slot.key) {
                    Some(edit) => edit,
                    None => {
                        seeded = TextEdit::seeded(&spec.initial);
                        &seeded
                    }
                };
                let layout = edit.layout(spec, usize::from(slot.field.width));
                frame.set_cursor_position(Position::new(
                    slot.field.x.saturating_add(layout.caret_column),
                    slot.field.y,
                ));
            }
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

fn plain_action(
    view: &ViewTarget,
    key: &str,
    kind: ActionKind,
    value: Option<ActionValue>,
) -> UiAction {
    UiAction {
        view: view.clone(),
        key: key.to_owned(),
        action_id: String::new(),
        kind,
        value,
    }
}

fn due_action(view: &ViewTarget, change: DueChange) -> UiAction {
    match change {
        DueChange::Text(key, text) => plain_action(
            view,
            &key,
            ActionKind::Changed,
            Some(ActionValue::Text(text)),
        ),
        DueChange::Number(key, value) => plain_action(
            view,
            &key,
            ActionKind::Changed,
            Some(ActionValue::Number(value)),
        ),
    }
}

/// Sends the edits whose debounce has run out at `now`.
fn release_due(view: &ViewTarget, cursor: &mut ViewCursor, now: Instant, out: &mut Vec<UiAction>) {
    out.extend(
        cursor
            .states
            .take_due(now)
            .into_iter()
            .map(|change| due_action(view, change)),
    );
}

/// The edits whose debounce has run out, for the event loop to send between inputs.
pub(crate) fn tick(view: &ViewTarget, cursor: &mut ViewCursor, now: Instant) -> Vec<UiAction> {
    let mut out = Vec::new();
    release_due(view, cursor, now, &mut out);
    out
}

/// When the next unsent edit falls due, so the event loop can wake for it.
pub(crate) fn next_due(cursor: &ViewCursor) -> Option<Instant> {
    cursor.states.next_due()
}

/// Whether Escape belongs to the view (it closes a number entry) instead of leaving it.
pub(crate) fn captures_escape(cursor: &ViewCursor) -> bool {
    cursor.states.entry_open()
}

/// Moves the keyboard cursor to a region. The widget it leaves sends its unsent edit, and a
/// region inside a scroll view scrolls into view.
fn focus_hit(
    scene: &Scene,
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    index: usize,
    out: &mut Vec<UiAction>,
) {
    if cursor.focus != index {
        if let Some(previous) = scene.hits.get(cursor.focus) {
            if let Some(change) = cursor.states.flush(&previous.key) {
                out.push(due_action(view, change));
            }
        }
    }
    cursor.focus = index;
    let Some((slot, row)) = scene.hits.get(index).and_then(|hit| hit.scroll) else {
        return;
    };
    let Some(scroll) = scene.scrolls.get(slot) else {
        return;
    };
    let target = if row < scroll.offset {
        row
    } else if row >= scroll.offset.saturating_add(scroll.rows) {
        (row + 1).saturating_sub(scroll.rows)
    } else {
        return;
    };
    scroll_to(scroll, view, cursor, target, out);
}

/// Scrolls a view to `offset` rows and tells the plugin when that moved it.
fn scroll_to(
    slot: &ScrollSlot,
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    offset: u16,
    out: &mut Vec<UiAction>,
) {
    if slot.key.is_empty() {
        return;
    }
    let offset = offset.min(slot.max_offset);
    if offset == slot.offset {
        return;
    }
    cursor.states.scrolls.insert(
        slot.key.clone(),
        ScrollState {
            offset,
            detached: offset < slot.max_offset,
        },
    );
    out.push(plain_action(
        view,
        &slot.key,
        ActionKind::Scrolled,
        Some(ActionValue::Index(u32::from(offset))),
    ));
}

/// The scroll view the keyboard scrolls: the one around the focused region, else the first
/// scroll view of the scene that the user can scroll.
fn keyboard_scroll<'a>(scene: &'a Scene, cursor: &ViewCursor) -> Option<&'a ScrollSlot> {
    scene
        .hits
        .get(cursor.focus)
        .and_then(|hit| hit.scroll)
        .and_then(|(slot, _)| scene.scrolls.get(slot))
        .filter(|slot| !slot.key.is_empty())
        .or_else(|| scene.scrolls.iter().find(|slot| !slot.key.is_empty()))
}

/// Scrolls the keyboard's scroll view by `rows` (negative is up).
fn scroll_keyboard(
    scene: &Scene,
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    rows: i32,
    out: &mut Vec<UiAction>,
) {
    if let Some(slot) = keyboard_scroll(scene, cursor) {
        let target = i32::from(slot.offset).saturating_add(rows).max(0);
        scroll_to(
            slot,
            view,
            cursor,
            u16::try_from(target).unwrap_or(u16::MAX),
            out,
        );
    }
}

/// The action of a click, with the keyboard cursor following it. A text input places its caret,
/// a number steps or sets its slider, a scrollbar or a divider starts a drag; any other region
/// sends its action. Debounced edits that fell due go first.
pub(crate) fn click(
    scene: &Scene,
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    position: Position,
    now: Instant,
) -> Vec<UiAction> {
    let mut out = Vec::new();
    release_due(view, cursor, now, &mut out);
    if let Some(slot) = scene
        .scrolls
        .iter()
        .find(|slot| slot.bar.is_some_and(|bar| bar.contains(position)))
    {
        if let Some(bar) = slot.bar.filter(|_| !slot.key.is_empty()) {
            cursor.drag = Some(Drag::Scrollbar {
                key: slot.key.clone(),
                bar,
                max_offset: slot.max_offset,
            });
            drag(view, cursor, position, &mut out);
        }
        return out;
    }
    if let Some(slot) = scene
        .splits
        .iter()
        .find(|slot| slot.resizable && slot.divider.contains(position))
    {
        if let Some(index) = slot.hit {
            focus_hit(scene, view, cursor, index, &mut out);
        }
        cursor.drag = Some(Drag::Split {
            key: slot.key.clone(),
            area: slot.area,
            stacked: slot.stacked,
            min_first: slot.min_first,
            min_second: slot.min_second,
            position: slot.position,
        });
        return out;
    }
    let Some(index) = scene
        .hits
        .iter()
        .position(|hit| hit.area.contains(position))
    else {
        return out;
    };
    focus_hit(scene, view, cursor, index, &mut out);
    if let Some(slot) = scene.inputs.iter().find(|slot| slot.hit == index) {
        click_input(slot, view, cursor, position, &mut out);
        return out;
    }
    if scene.splits.iter().any(|slot| slot.hit == Some(index)) {
        return out;
    }
    if let Some(hit) = scene.hits.get(index) {
        out.push(action(view, hit));
    }
    out
}

fn click_input(
    slot: &InputSlot,
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    position: Position,
    out: &mut Vec<UiAction>,
) {
    if !slot.field.contains(position) {
        return;
    }
    let column = position.x - slot.field.x;
    match &slot.kind {
        InputKind::Text(spec) => {
            cursor
                .states
                .texts
                .entry(slot.key.clone())
                .or_insert_with(|| TextEdit::seeded(&spec.initial))
                .place_caret(spec, usize::from(slot.field.width), column);
        }
        InputKind::Number(spec) => {
            cursor
                .states
                .number_specs
                .insert(slot.key.clone(), spec.clone());
            let edit = cursor
                .states
                .numbers
                .entry(slot.key.clone())
                .or_insert_with(|| NumberEdit::seeded(spec));
            let outcome = match spec.style {
                NumberStyle::Stepper => {
                    let direction = if column < slot.field.width / 2 { -1 } else { 1 };
                    edit.step(spec, direction)
                }
                NumberStyle::Slider => edit.set_from_track(spec, column, slot.field.width),
            };
            if let NumberOutcome::Changed(value) = outcome {
                out.push(plain_action(
                    view,
                    &slot.key,
                    ActionKind::Changed,
                    Some(ActionValue::Number(value)),
                ));
            }
            if spec.style == NumberStyle::Slider {
                cursor.drag = Some(Drag::Slider {
                    key: slot.key.clone(),
                    track: slot.field,
                    spec: spec.clone(),
                });
            }
        }
    }
}

/// The pointer moved with the button down: continues the drag that a click started.
pub(crate) fn drag(
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    position: Position,
    out: &mut Vec<UiAction>,
) {
    let Some(current) = cursor.drag.clone() else {
        return;
    };
    match current {
        Drag::Scrollbar {
            key,
            bar,
            max_offset,
        } => {
            let travel = bar.height.saturating_sub(1).max(1);
            let row = position
                .y
                .clamp(bar.y, bar.bottom().saturating_sub(1))
                .saturating_sub(bar.y);
            let offset = u32::from(row) * u32::from(max_offset) / u32::from(travel);
            let offset = u16::try_from(offset).unwrap_or(u16::MAX).min(max_offset);
            let previous = cursor.states.scrolls.get(&key).map(|state| state.offset);
            if previous != Some(offset) {
                cursor.states.scrolls.insert(
                    key.clone(),
                    ScrollState {
                        offset,
                        detached: offset < max_offset,
                    },
                );
                out.push(plain_action(
                    view,
                    &key,
                    ActionKind::Scrolled,
                    Some(ActionValue::Index(u32::from(offset))),
                ));
            }
        }
        Drag::Split {
            key,
            area,
            stacked,
            min_first,
            min_second,
            ..
        } => {
            let (span, start, at) = if stacked {
                (area.height, area.y, position.y)
            } else {
                (area.width, area.x, position.x)
            };
            let first = at.saturating_sub(start);
            let low = min_first;
            let high = span.saturating_sub(1).saturating_sub(min_second);
            let first = if low > high {
                low.min(span.saturating_sub(1))
            } else {
                first.clamp(low, high)
            };
            let moved = split_position(span, first);
            cursor.states.splits.insert(key.clone(), moved);
            cursor.drag = Some(Drag::Split {
                key,
                area,
                stacked,
                min_first,
                min_second,
                position: moved,
            });
        }
        Drag::Slider { key, track, spec } => {
            let column = position
                .x
                .clamp(track.x, track.right().saturating_sub(1))
                .saturating_sub(track.x);
            let edit = cursor
                .states
                .numbers
                .entry(key.clone())
                .or_insert_with(|| NumberEdit::seeded(&spec));
            let before = edit.value;
            if let NumberOutcome::Changed(value) = edit.set_from_track(&spec, column, track.width) {
                if (value - before).abs() > f64::EPSILON {
                    out.push(plain_action(
                        view,
                        &key,
                        ActionKind::Changed,
                        Some(ActionValue::Number(value)),
                    ));
                }
            }
        }
    }
}

/// The button went up: a split drag ends here, so the plugin hears `resized` once.
pub(crate) fn release(view: &ViewTarget, cursor: &mut ViewCursor) -> Vec<UiAction> {
    match cursor.drag.take() {
        Some(Drag::Split { key, position, .. }) => vec![plain_action(
            view,
            &key,
            ActionKind::Resized,
            Some(ActionValue::Number(f64::from(position))),
        )],
        _ => Vec::new(),
    }
}

/// The wheel over a scroll view moves it three rows per notch; `notches` is negative upward.
pub(crate) fn wheel(
    scene: &Scene,
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    position: Position,
    notches: i32,
) -> Vec<UiAction> {
    let mut out = Vec::new();
    let innermost = scene
        .scrolls
        .iter()
        .filter(|slot| !slot.key.is_empty() && slot.area.contains(position))
        .min_by_key(|slot| u32::from(slot.area.width) * u32::from(slot.area.height));
    if let Some(slot) = innermost {
        let target = i32::from(slot.offset)
            .saturating_add(notches.saturating_mul(3))
            .max(0);
        scroll_to(
            slot,
            view,
            cursor,
            u16::try_from(target).unwrap_or(u16::MAX),
            &mut out,
        );
    }
    out
}

/// A bracketed paste into the focused text input: one change, or `None` when the keyboard is
/// not in a text input.
pub(crate) fn paste(
    scene: &Scene,
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    pasted: &str,
    now: Instant,
) -> Option<Vec<UiAction>> {
    let slot = scene.inputs.iter().find(|slot| slot.hit == cursor.focus)?;
    let InputKind::Text(spec) = &slot.kind else {
        return None;
    };
    let mut out = Vec::new();
    cursor
        .states
        .texts
        .entry(slot.key.clone())
        .or_insert_with(|| TextEdit::seeded(&spec.initial))
        .paste(pasted, spec, now);
    release_due(view, cursor, now, &mut out);
    Some(out)
}

/// A key on the divider that has the keyboard: the arrows move it by 50 per mille, Home and End
/// send it to the minimum bounds. Returns the `resized` action when it moved.
fn split_key(
    slot: &SplitSlot,
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    code: KeyCode,
) -> Option<UiAction> {
    let span = if slot.stacked {
        slot.area.height
    } else {
        slot.area.width
    };
    let available = span.saturating_sub(1);
    let moved = match code {
        KeyCode::Left | KeyCode::Up => slot.position.saturating_sub(50),
        KeyCode::Right | KeyCode::Down => slot.position.saturating_add(50).min(1000),
        KeyCode::Home => split_position(span, slot.min_first.min(available)),
        KeyCode::End => split_position(span, available.saturating_sub(slot.min_second)),
        _ => return None,
    };
    cursor.states.splits.insert(slot.key.clone(), moved);
    Some(plain_action(
        view,
        &slot.key,
        ActionKind::Resized,
        Some(ActionValue::Number(f64::from(moved))),
    ))
}

/// One key on a view. A focused text input or number takes the keys it edits with; Up, Down,
/// Tab and Shift+Tab then move the cursor (Up and Down scroll at the first and last region),
/// PageUp, PageDown, Home and End scroll, and Enter and Space send the action of the region
/// under the cursor. Debounced edits that fell due go first.
pub(crate) fn key(
    scene: &Scene,
    view: &ViewTarget,
    cursor: &mut ViewCursor,
    event: KeyEvent,
    now: Instant,
) -> Vec<UiAction> {
    let last = scene.hits.len().saturating_sub(1);
    cursor.focus = cursor.focus.min(last);
    let mut out = Vec::new();
    release_due(view, cursor, now, &mut out);
    let focused = scene.inputs.iter().find(|slot| slot.hit == cursor.focus);
    if let Some(slot) = focused {
        match &slot.kind {
            InputKind::Text(spec) => {
                let edit = cursor
                    .states
                    .texts
                    .entry(slot.key.clone())
                    .or_insert_with(|| TextEdit::seeded(&spec.initial));
                match edit.apply_key(event, spec, now) {
                    EditOutcome::Ignored => {}
                    EditOutcome::Moved => return out,
                    EditOutcome::Edited => {
                        release_due(view, cursor, now, &mut out);
                        return out;
                    }
                    EditOutcome::Submitted => {
                        let text = edit.text().to_owned();
                        let pending = edit.flush();
                        if let Some(pending) = pending {
                            out.push(due_action(view, DueChange::Text(slot.key.clone(), pending)));
                        }
                        out.push(plain_action(
                            view,
                            &slot.key,
                            ActionKind::Submitted,
                            Some(ActionValue::Text(text)),
                        ));
                        return out;
                    }
                }
            }
            InputKind::Number(spec) => {
                cursor
                    .states
                    .number_specs
                    .insert(slot.key.clone(), spec.clone());
                let edit = cursor
                    .states
                    .numbers
                    .entry(slot.key.clone())
                    .or_insert_with(|| NumberEdit::seeded(spec));
                match edit.apply_key(event, spec, now) {
                    NumberOutcome::Ignored => {}
                    NumberOutcome::Typing => return out,
                    NumberOutcome::Changed(value) => {
                        out.push(plain_action(
                            view,
                            &slot.key,
                            ActionKind::Changed,
                            Some(ActionValue::Number(value)),
                        ));
                        return out;
                    }
                }
            }
        }
    }
    if let Some(slot) = scene
        .splits
        .iter()
        .find(|slot| slot.hit == Some(cursor.focus))
    {
        if let Some(resized) = split_key(slot, view, cursor, event.code) {
            out.push(resized);
            return out;
        }
    }
    let viewport = keyboard_scroll(scene, cursor)
        .map_or(1, |slot| i32::from(slot.rows).saturating_sub(1).max(1));
    match event.code {
        KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
            if cursor.focus >= last {
                scroll_keyboard(scene, view, cursor, 1, &mut out);
            } else {
                focus_hit(scene, view, cursor, cursor.focus + 1, &mut out);
            }
        }
        KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
            if cursor.focus == 0 {
                scroll_keyboard(scene, view, cursor, -1, &mut out);
            } else {
                focus_hit(scene, view, cursor, cursor.focus - 1, &mut out);
            }
        }
        KeyCode::PageDown => scroll_keyboard(scene, view, cursor, viewport, &mut out),
        KeyCode::PageUp => scroll_keyboard(scene, view, cursor, -viewport, &mut out),
        KeyCode::Home => scroll_keyboard(scene, view, cursor, i32::MIN / 2, &mut out),
        KeyCode::End => scroll_keyboard(scene, view, cursor, i32::MAX / 2, &mut out),
        KeyCode::Enter | KeyCode::Char(' ') => {
            let editable = focused.is_some()
                || scene
                    .splits
                    .iter()
                    .any(|slot| slot.hit == Some(cursor.focus));
            if !editable {
                if let Some(hit) = scene.hits.get(cursor.focus) {
                    out.push(action(view, hit));
                }
            }
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use ilium_plugin_api::generated::types::layout::{Alignment, Flex, Margin, Spacing};
    use ilium_plugin_api::generated::types::ui::{
        ButtonSpec, ChoiceSpec, DisabledMarker, LayoutSpec, ParagraphSpec, PopupSpec, ToggleSpec,
        ToggleStyle,
    };
    use std::time::Duration;

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

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn first(mut actions: Vec<UiAction>) -> UiAction {
        assert_eq!(actions.len(), 1, "{actions:?}");
        actions.remove(0)
    }

    #[test]
    fn a_click_sends_the_action_of_the_widget_under_it() {
        let view = ViewTarget::Panel("panel".to_owned());
        let scene = compose(&sample(), Rect::new(0, 0, 30, 10));
        let mut cursor = ViewCursor::default();
        let now = Instant::now();
        // The selected option takes no click; the other one changes the choice.
        assert!(click(&scene, &view, &mut cursor, Position::new(1, 1), now).is_empty());
        let changed = first(click(&scene, &view, &mut cursor, Position::new(1, 2), now));
        assert_eq!(changed.key, "pick");
        assert_eq!(changed.kind, ActionKind::Changed);
        assert_eq!(changed.value, Some(ActionValue::Index(1)));
        let toggled = first(click(&scene, &view, &mut cursor, Position::new(1, 3), now));
        assert_eq!(toggled.value, Some(ActionValue::Flag(true)));
        let pressed = first(click(&scene, &view, &mut cursor, Position::new(1, 4), now));
        assert_eq!(pressed.key, "go");
        assert_eq!(pressed.kind, ActionKind::Activated);
        assert_eq!(pressed.view, view);
    }

    #[test]
    fn the_keyboard_walks_the_regions_and_enter_sends_the_focused_one() {
        let view = ViewTarget::SettingsPage("page".to_owned());
        let scene = compose(&sample(), Rect::new(0, 0, 30, 10));
        let mut cursor = ViewCursor::default();
        let now = Instant::now();
        for _ in 0..3 {
            assert!(key(&scene, &view, &mut cursor, press(KeyCode::Down), now).is_empty());
        }
        assert_eq!(cursor.focus, 2);
        let pressed = first(key(&scene, &view, &mut cursor, press(KeyCode::Enter), now));
        assert_eq!(pressed.key, "go");
    }

    fn column(rows: Vec<(Option<&str>, Widget, Vec<u32>)>) -> UiTree {
        let layout = LayoutSpec {
            direction: Direction::Vertical,
            constraints: Vec::new(),
            flex: Flex::Start,
            spacing: Spacing::Space(0),
            margin: Margin::default(),
        };
        let count = u32::try_from(rows.len()).unwrap_or(0);
        let mut nodes = vec![node(None, Widget::Layout(layout), (1..=count).collect())];
        nodes.extend(
            rows.into_iter()
                .map(|(key, widget, children)| node(key, widget, children)),
        );
        UiTree {
            version: 1,
            root: 0,
            nodes,
        }
    }

    fn line(text: &str) -> Widget {
        Widget::Paragraph(ParagraphSpec {
            lines: vec![span(text)],
            rich: None,
            wrap: None,
            alignment: Alignment::Left,
            style: PluginStyle::default(),
            scrollable: false,
        })
    }

    fn text_input(initial: &str, debounce_ms: u32, submit_on_enter: bool) -> Widget {
        Widget::TextInput(TextInputSpec {
            label: "Name".to_owned(),
            initial: initial.to_owned(),
            debounce_ms,
            submit_on_enter,
            ..TextInputSpec::default()
        })
    }

    fn number(style: NumberStyle) -> Widget {
        Widget::Number(NumberSpec {
            label: "Level".to_owned(),
            value: 5.0,
            minimum: 0.0,
            maximum: 10.0,
            step: 1.0,
            unit: None,
            style,
            steps: Vec::new(),
            special_values: Vec::new(),
            scale: 1,
            debounce_ms: 0,
        })
    }

    fn row_text(scene: &Scene, row: u16) -> String {
        scene
            .texts
            .iter()
            .find(|(area, _)| area.y == row)
            .map(|(_, line)| line.to_string())
            .unwrap_or_default()
    }

    fn type_text(scene: &Scene, view: &ViewTarget, cursor: &mut ViewCursor, text: &str) {
        for character in text.chars() {
            key(
                scene,
                view,
                cursor,
                press(KeyCode::Char(character)),
                Instant::now(),
            );
        }
    }

    #[test]
    fn a_text_input_edits_in_place_and_reports_each_change() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = column(vec![(Some("name"), text_input("ab", 0, false), vec![])]);
        let area = Rect::new(0, 0, 40, 3);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, area, &cursor.states);
        assert_eq!(scene.hits.len(), 1);
        assert!(scene.skipped.is_empty());
        let changed = first(key(
            &scene,
            &view,
            &mut cursor,
            press(KeyCode::Char('c')),
            Instant::now(),
        ));
        assert_eq!(changed.key, "name");
        assert_eq!(changed.kind, ActionKind::Changed);
        assert_eq!(changed.value, Some(ActionValue::Text("abc".to_owned())));
        // The host owns the contents now: the next frame shows them.
        let scene = compose_with(&tree, area, &cursor.states);
        assert!(
            row_text(&scene, 0).contains("abc"),
            "{}",
            row_text(&scene, 0)
        );
        let erased = first(key(
            &scene,
            &view,
            &mut cursor,
            press(KeyCode::Backspace),
            Instant::now(),
        ));
        assert_eq!(erased.value, Some(ActionValue::Text("ab".to_owned())));
    }

    #[test]
    fn a_debounced_text_input_sends_once_the_delay_has_run_out() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = column(vec![(Some("name"), text_input("", 200, false), vec![])]);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, Rect::new(0, 0, 40, 3), &cursor.states);
        let start = Instant::now();
        assert!(key(&scene, &view, &mut cursor, press(KeyCode::Char('x')), start).is_empty());
        assert!(next_due(&cursor).is_some());
        assert!(tick(&view, &mut cursor, start).is_empty());
        let released = first(tick(&view, &mut cursor, start + Duration::from_millis(250)));
        assert_eq!(released.value, Some(ActionValue::Text("x".to_owned())));
        assert!(next_due(&cursor).is_none());
    }

    #[test]
    fn enter_submits_a_text_input_and_leaving_it_sends_its_unsent_edit() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = column(vec![
            (Some("name"), text_input("", 500, true), vec![]),
            (Some("go"), line("x"), vec![]),
        ]);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, Rect::new(0, 0, 40, 4), &cursor.states);
        let now = Instant::now();
        key(&scene, &view, &mut cursor, press(KeyCode::Char('q')), now);
        let sent = key(&scene, &view, &mut cursor, press(KeyCode::Enter), now);
        assert_eq!(sent.len(), 2, "{sent:?}");
        assert_eq!(sent[0].kind, ActionKind::Changed);
        assert_eq!(sent[1].kind, ActionKind::Submitted);
        assert_eq!(sent[1].value, Some(ActionValue::Text("q".to_owned())));
    }

    #[test]
    fn a_click_in_a_text_input_places_the_caret() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = column(vec![(Some("name"), text_input("abcd", 0, false), vec![])]);
        let area = Rect::new(0, 0, 40, 3);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, area, &cursor.states);
        let field = scene.inputs[0].field;
        assert!(click(
            &scene,
            &view,
            &mut cursor,
            Position::new(field.x + 1, field.y),
            Instant::now()
        )
        .is_empty());
        let scene = compose_with(&tree, area, &cursor.states);
        let edited = first(key(
            &scene,
            &view,
            &mut cursor,
            press(KeyCode::Char('X')),
            Instant::now(),
        ));
        assert_eq!(edited.value, Some(ActionValue::Text("aXbcd".to_owned())));
    }

    #[test]
    fn a_stepper_changes_with_the_arrows_and_a_typed_entry() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = column(vec![(Some("level"), number(NumberStyle::Stepper), vec![])]);
        let area = Rect::new(0, 0, 40, 3);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, area, &cursor.states);
        assert!(
            row_text(&scene, 0).contains("‹ 5 ›"),
            "{}",
            row_text(&scene, 0)
        );
        let up = first(key(
            &scene,
            &view,
            &mut cursor,
            press(KeyCode::Right),
            Instant::now(),
        ));
        assert_eq!(up.value, Some(ActionValue::Number(6.0)));
        let now = Instant::now();
        assert!(key(&scene, &view, &mut cursor, press(KeyCode::Enter), now).is_empty());
        assert!(captures_escape(&cursor));
        type_text(&scene, &view, &mut cursor, "8");
        let typed = first(key(&scene, &view, &mut cursor, press(KeyCode::Enter), now));
        assert_eq!(typed.value, Some(ActionValue::Number(8.0)));
        assert!(!captures_escape(&cursor));
        let scene = compose_with(&tree, area, &cursor.states);
        assert!(
            row_text(&scene, 0).contains("‹ 8 ›"),
            "{}",
            row_text(&scene, 0)
        );
    }

    #[test]
    fn a_slider_follows_a_click_and_a_drag_on_its_track() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = column(vec![(Some("level"), number(NumberStyle::Slider), vec![])]);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, Rect::new(0, 0, 40, 3), &cursor.states);
        let track = scene.inputs[0].field;
        assert!(track.width > 2);
        let end = Position::new(track.right() - 1, track.y);
        let now = Instant::now();
        let set = first(click(&scene, &view, &mut cursor, end, now));
        assert_eq!(set.value, Some(ActionValue::Number(10.0)));
        let mut moved = Vec::new();
        drag(
            &view,
            &mut cursor,
            Position::new(track.x, track.y),
            &mut moved,
        );
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].value, Some(ActionValue::Number(0.0)));
        assert!(release(&view, &mut cursor).is_empty());
        assert!(cursor.drag.is_none());
    }

    fn log_tree(rows: u32, follow_tail: bool) -> UiTree {
        let body = Widget::Layout(LayoutSpec {
            direction: Direction::Vertical,
            constraints: Vec::new(),
            flex: Flex::Start,
            spacing: Spacing::Space(0),
            margin: Margin::default(),
        });
        let scroll = Widget::ScrollView(ScrollViewSpec {
            content_rows: rows,
            show_scrollbar: true,
            follow_tail,
        });
        let mut nodes = vec![
            node(Some("log"), scroll, vec![1]),
            node(None, body, (2..2 + rows).collect()),
        ];
        for index in 0..rows {
            nodes.push(node(None, line(&format!("row {index}")), vec![]));
        }
        UiTree {
            version: 1,
            root: 0,
            nodes,
        }
    }

    #[test]
    fn a_scroll_view_clips_its_child_and_the_wheel_scrolls_it() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = log_tree(10, false);
        let area = Rect::new(0, 0, 20, 3);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, area, &cursor.states);
        assert_eq!(row_text(&scene, 0).trim_end(), "row 0");
        assert_eq!(row_text(&scene, 2).trim_end(), "row 2");
        assert!(scene.texts.iter().all(|(rect, _)| rect.bottom() <= 3));
        assert_eq!(scene.scrolls.len(), 1);
        assert_eq!(scene.scrolls[0].max_offset, 7);
        let scrolled = first(wheel(&scene, &view, &mut cursor, Position::new(2, 1), 1));
        assert_eq!(scrolled.kind, ActionKind::Scrolled);
        assert_eq!(scrolled.value, Some(ActionValue::Index(3)));
        let scene = compose_with(&tree, area, &cursor.states);
        assert_eq!(row_text(&scene, 0).trim_end(), "row 3");
        // The wheel never goes past either end.
        assert!(wheel(&scene, &view, &mut cursor, Position::new(2, 1), -9).len() == 1);
        let scene = compose_with(&tree, area, &cursor.states);
        assert!(wheel(&scene, &view, &mut cursor, Position::new(2, 1), -1).is_empty());
        // A position outside the view scrolls nothing.
        assert!(wheel(&scene, &view, &mut cursor, Position::new(30, 1), 1).is_empty());
    }

    #[test]
    fn a_scroll_view_follows_the_tail_until_the_user_scrolls_away() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = log_tree(10, true);
        let area = Rect::new(0, 0, 20, 3);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, area, &cursor.states);
        assert_eq!(row_text(&scene, 0).trim_end(), "row 7");
        wheel(&scene, &view, &mut cursor, Position::new(2, 1), -1);
        let scene = compose_with(&tree, area, &cursor.states);
        assert_eq!(row_text(&scene, 0).trim_end(), "row 4");
        assert!(cursor.states.scrolls["log"].detached);
    }

    #[test]
    fn a_dragged_scrollbar_sets_the_offset_from_the_pointer_row() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = log_tree(10, false);
        let area = Rect::new(0, 0, 20, 3);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, area, &cursor.states);
        let bar = scene.scrolls[0].bar.expect("a scrollbar");
        assert_eq!(bar, Rect::new(19, 0, 1, 3));
        click(
            &scene,
            &view,
            &mut cursor,
            Position::new(19, 2),
            Instant::now(),
        );
        assert_eq!(cursor.states.scrolls["log"].offset, 7);
        let mut moved = Vec::new();
        drag(&view, &mut cursor, Position::new(19, 0), &mut moved);
        assert_eq!(cursor.states.scrolls["log"].offset, 0);
        assert_eq!(moved.len(), 1);
    }

    #[test]
    fn moving_the_keyboard_to_a_hidden_region_scrolls_it_into_view() {
        let view = ViewTarget::Panel("panel".to_owned());
        let mut nodes = vec![node(
            Some("list"),
            Widget::ScrollView(ScrollViewSpec {
                content_rows: 6,
                show_scrollbar: false,
                follow_tail: false,
            }),
            vec![1],
        )];
        let body = Widget::Layout(LayoutSpec {
            direction: Direction::Vertical,
            constraints: Vec::new(),
            flex: Flex::Start,
            spacing: Spacing::Space(0),
            margin: Margin::default(),
        });
        nodes.push(node(None, body, (2..8).collect()));
        for index in 0..6 {
            nodes.push(node(
                Some(&format!("t{index}")),
                Widget::Toggle(ToggleSpec {
                    label: format!("Toggle {index}"),
                    value: false,
                    style: ToggleStyle::Checkbox,
                    tri_state: false,
                    icon_on: None,
                    icon_off: None,
                }),
                vec![],
            ));
        }
        let tree = UiTree {
            version: 1,
            root: 0,
            nodes,
        };
        let area = Rect::new(0, 0, 20, 3);
        let mut cursor = ViewCursor::default();
        let now = Instant::now();
        let scene = compose_with(&tree, area, &cursor.states);
        assert_eq!(scene.hits.len(), 6);
        // The three hidden toggles keep their place, with an empty area.
        assert_eq!(scene.hits[3].area, Rect::default());
        for _ in 0..2 {
            assert!(key(&scene, &view, &mut cursor, press(KeyCode::Down), now).is_empty());
        }
        let scrolled = first(key(&scene, &view, &mut cursor, press(KeyCode::Down), now));
        assert_eq!(scrolled.kind, ActionKind::Scrolled);
        assert_eq!(scrolled.value, Some(ActionValue::Index(1)));
        let scene = compose_with(&tree, area, &cursor.states);
        assert_eq!(scene.hits[3].area, Rect::new(0, 2, 20, 1));
        assert_eq!(cursor.focus, 3);
    }

    fn split_tree(resizable: bool) -> UiTree {
        let split = Widget::Split(SplitSpec {
            direction: Direction::Horizontal,
            position: 500,
            min_first: 0,
            min_second: 0,
            resizable,
        });
        UiTree {
            version: 1,
            root: 0,
            nodes: vec![
                node(Some("main"), split, vec![1, 2]),
                node(None, line("left"), vec![]),
                node(None, line("right"), vec![]),
            ],
        }
    }

    #[test]
    fn a_split_divides_its_area_around_a_divider_and_the_drag_reports_once() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = split_tree(true);
        let area = Rect::new(0, 0, 21, 2);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, area, &cursor.states);
        assert_eq!(scene.splits[0].divider, Rect::new(10, 0, 1, 2));
        assert!(scene
            .texts
            .iter()
            .any(|(rect, line)| rect.x == 0 && line.to_string().starts_with("left")));
        assert!(scene
            .texts
            .iter()
            .any(|(rect, line)| rect.x == 11 && line.to_string().starts_with("right")));
        assert_eq!(scene.hits.len(), 1);
        let now = Instant::now();
        assert!(click(&scene, &view, &mut cursor, Position::new(10, 0), now).is_empty());
        let mut moved = Vec::new();
        drag(&view, &mut cursor, Position::new(14, 1), &mut moved);
        assert!(moved.is_empty());
        let scene = compose_with(&tree, area, &cursor.states);
        assert_eq!(scene.splits[0].divider.x, 14);
        let resized = first(release(&view, &mut cursor));
        assert_eq!(resized.kind, ActionKind::Resized);
        assert_eq!(resized.value, Some(ActionValue::Number(700.0)));
    }

    #[test]
    fn the_keyboard_moves_a_focused_divider_and_a_fixed_split_takes_no_input() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = split_tree(true);
        let area = Rect::new(0, 0, 21, 2);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, area, &cursor.states);
        let resized = first(key(
            &scene,
            &view,
            &mut cursor,
            press(KeyCode::Right),
            Instant::now(),
        ));
        assert_eq!(resized.value, Some(ActionValue::Number(550.0)));
        let fixed = compose_with(&split_tree(false), area, &WidgetStates::default());
        assert!(fixed.hits.is_empty());
    }

    #[test]
    fn split_geometry_keeps_the_minimums_and_round_trips_a_position() {
        assert_eq!(split_first(21, 500, 0, 0), 10);
        assert_eq!(split_first(21, 0, 4, 0), 4);
        assert_eq!(split_first(21, 1000, 0, 6), 14);
        // When both minimums cannot fit, the first one wins.
        assert_eq!(split_first(10, 500, 8, 8), 8);
        for first in 0..=20u16 {
            assert_eq!(split_first(21, split_position(21, first), 0, 0), first);
        }
        assert_eq!(split_position(1, 0), 0);
    }

    #[test]
    fn a_popup_draws_nothing_inline() {
        let popup = Widget::Popup(PopupSpec {
            id: "menu".to_owned(),
            title: None,
            modal: false,
            width: Constraint::Fill(1),
            height: Constraint::Fill(1),
        });
        let tree = UiTree {
            version: 1,
            root: 0,
            nodes: vec![
                node(None, popup, vec![1]),
                node(None, line("inside"), vec![]),
            ],
        };
        let scene = compose(&tree, Rect::new(0, 0, 20, 5));
        assert_eq!(scene, Scene::default());
    }

    #[test]
    fn a_paste_goes_to_the_focused_text_input_only() {
        let view = ViewTarget::Panel("panel".to_owned());
        let tree = column(vec![
            (Some("name"), text_input("", 0, false), vec![]),
            (
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
        ]);
        let mut cursor = ViewCursor::default();
        let scene = compose_with(&tree, Rect::new(0, 0, 40, 3), &cursor.states);
        let now = Instant::now();
        let pasted = paste(&scene, &view, &mut cursor, "hello\nworld", now).expect("an input");
        let pasted = first(pasted);
        assert_eq!(
            pasted.value,
            Some(ActionValue::Text("helloworld".to_owned()))
        );
        cursor.focus = 1;
        assert!(paste(&scene, &view, &mut cursor, "x", now).is_none());
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
