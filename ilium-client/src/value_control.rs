//! Prepared single-row controls; retain this object with the displayed row snapshot.
//! The caller owns focus, option identities, validation, mutation and persistence.
use crossterm::event::KeyCode;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    Choice,
    Number,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Right,
    Other,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    PreviousChoice,
    NextChoice,
    OpenChoices,
    Decrement,
    Increment,
    EditNumber,
}
#[derive(Debug, Clone, Copy)]
pub struct ControlSpec<'a> {
    pub kind: ControlKind,
    pub label: &'a str,
    pub value: &'a str,
    pub label_width: u16,
    pub previous_enabled: bool,
    pub next_enabled: bool,
    pub open_enabled: bool,
}
#[derive(Debug, Clone, Copy)]
pub struct ControlGeometry {
    pub row: Rect,
    pub label: Rect,
    pub value_slot: Rect,
    pub value: Rect,
    pub previous: Rect,
    pub next: Rect,
    pub open: Rect,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct ControlStyles {
    pub background: Style,
    pub label: Style,
    pub value: Style,
    pub button: Style,
    pub disabled: Style,
}
#[derive(Debug, Clone)]
pub struct ValueControl {
    kind: ControlKind,
    geometry: ControlGeometry,
    label_text: String,
    value_text: String,
    value_ink: Vec<Rect>,
    previous_enabled: bool,
    next_enabled: bool,
    open_enabled: bool,
}
impl ValueControl {
    pub fn new(area: Rect, spec: ControlSpec<'_>) -> Self {
        let width = area.width.min(u16::MAX - area.x);
        let height = area.height.min(1).min(u16::MAX - area.y);
        let row = Rect::new(area.x, area.y, width, height);
        let empty = Rect::new(row.x, row.y, 0, 0);
        let mut prepared = Self {
            kind: spec.kind,
            geometry: ControlGeometry {
                row,
                label: empty,
                value_slot: empty,
                value: empty,
                previous: empty,
                next: empty,
                open: empty,
            },
            label_text: String::new(),
            value_text: String::new(),
            value_ink: Vec::new(),
            previous_enabled: spec.previous_enabled,
            next_enabled: spec.next_enabled,
            open_enabled: spec.open_enabled,
        };
        if row.width == 0 || row.height == 0 {
            return prepared;
        }
        let requested_label = if spec.label.is_empty() {
            0
        } else {
            spec.label_width
        };
        // Size step targets for their rendered glyphs so wide numeric symbols
        // keep a complete, independently clickable cell range.
        let step_width = step_button_width(spec.kind, row.width);
        let chrome = 2 * step_width + 4;
        let value_cells = cell_width(spec.value).min(usize::from(row.width)) as u16;
        let mut label_width = requested_label.min(row.width.saturating_sub(chrome + 2));
        let label_gap = u16::from(label_width > 0);
        let initial_control_width = row.width - label_width - label_gap;
        if initial_control_width > chrome {
            let current_value_slot = initial_control_width - chrome;
            if value_cells > current_value_slot {
                // Release unused label budget before clipping a readable value.
                // Very long labels may use half the row; the rest remains
                // available to the value and the three distinct buttons.
                let label_floor = cell_width(spec.label)
                    .min(usize::from(label_width))
                    .min(usize::from(row.width / 2)) as u16;
                let largest_value_slot = row
                    .width
                    .saturating_sub(label_floor)
                    .saturating_sub(label_gap)
                    .saturating_sub(chrome);
                let target_value_slot = value_cells.saturating_add(2).min(largest_value_slot);
                label_width -= target_value_slot
                    .saturating_sub(current_value_slot)
                    .min(label_width - label_floor);
            }
        }
        let label_gap = u16::from(label_width > 0);
        let control_x = row.x + label_width + label_gap;
        let control_width = row.width - label_width - label_gap;
        let geometry = &mut prepared.geometry;
        geometry.label = Rect::new(row.x, row.y, label_width, 1);
        prepared.label_text = clip_cells(spec.label, label_width);
        if control_width < 2 * step_width + 2 {
            geometry.open = Rect::new(control_x + control_width - 1, row.y, 1, 1);
            geometry.value_slot = Rect::new(control_x, row.y, control_width - 1, 1);
        } else {
            let gap = match spec.kind {
                ControlKind::Choice => u16::from(control_width > chrome),
                ControlKind::Number => {
                    u16::from(control_width > chrome.saturating_add(2).saturating_add(value_cells))
                }
            };
            geometry.previous = Rect::new(control_x, row.y, step_width, 1);
            geometry.value_slot = Rect::new(
                control_x + step_width + gap,
                row.y,
                control_width - (2 * step_width + 1) - 3 * gap,
                1,
            );
            let middle = Rect::new(
                control_x + control_width - 1 - gap - step_width,
                row.y,
                step_width,
                1,
            );
            let last = Rect::new(control_x + control_width - 1, row.y, 1, 1);
            match spec.kind {
                ControlKind::Choice => {
                    geometry.open = middle;
                    geometry.next = last;
                }
                ControlKind::Number => {
                    geometry.next = middle;
                    geometry.open = last;
                }
            }
        }
        prepared.value_text = clip_cells(spec.value, geometry.value_slot.width);
        let value_width = cell_width(&prepared.value_text) as u16;
        let padding = if spec.kind == ControlKind::Number {
            (geometry.value_slot.width - value_width) / 2
        } else {
            0
        };
        geometry.value = Rect::new(geometry.value_slot.x + padding, row.y, value_width, 1);
        let mut ink_x = geometry.value.x;
        for grapheme in UnicodeSegmentation::graphemes(prepared.value_text.as_str(), true) {
            let ink_width = UnicodeWidthStr::width(grapheme) as u16;
            if !grapheme.chars().all(char::is_whitespace) && ink_width > 0 {
                prepared
                    .value_ink
                    .push(Rect::new(ink_x, row.y, ink_width, 1));
            }
            ink_x += ink_width;
        }
        prepared
    }
    pub fn geometry(&self) -> ControlGeometry {
        self.geometry
    }
    pub fn value_ink(&self) -> &[Rect] {
        &self.value_ink
    }
    pub fn render(&self, frame: &mut Frame, styles: ControlStyles) {
        let geometry = self.geometry;
        if geometry.row.width == 0 || geometry.row.height == 0 {
            return;
        }
        paint(
            frame,
            geometry.row,
            &" ".repeat(usize::from(geometry.row.width)),
            styles.background,
        );
        paint(frame, geometry.label, &self.label_text, styles.label);
        paint(frame, geometry.value, &self.value_text, styles.value);
        let glyphs = match (self.kind, geometry.previous.width) {
            (ControlKind::Choice, _) => ["←", "→", "+"],
            (ControlKind::Number, 1) => ["-", "+", "*"],
            (ControlKind::Number, _) => [NUMBER_DECREMENT_GLYPH, NUMBER_INCREMENT_GLYPH, "*"],
        };
        let buttons = [
            (geometry.previous, self.previous_enabled),
            (geometry.next, self.next_enabled),
            (geometry.open, self.open_enabled),
        ];
        for (index, (rectangle, enabled)) in buttons.into_iter().enumerate() {
            let style = if enabled {
                styles.button
            } else {
                styles.disabled
            };
            paint(frame, rectangle, glyphs[index], style);
        }
    }
    pub fn hit(&self, position: Position, button: PointerButton) -> Option<ControlAction> {
        if !self.geometry.row.contains(position) {
            return None;
        }
        let [previous, next, open] = self.actions();
        // Word gaps belong to the value; only its outer padding is inert.
        if self
            .value_ink
            .first()
            .zip(self.value_ink.last())
            .is_some_and(|(first, last)| {
                position.y == first.y && (first.x..last.right()).contains(&position.x)
            })
        {
            let action = match (self.kind, button) {
                (ControlKind::Choice, PointerButton::Left) => next,
                (ControlKind::Choice, PointerButton::Right) => previous,
                (ControlKind::Number, PointerButton::Left) => open,
                _ => return None,
            };
            return self.allowed(action);
        }
        if button != PointerButton::Left {
            return None;
        }
        let action = if self.geometry.previous.contains(position) {
            previous
        } else if self.geometry.next.contains(position) {
            next
        } else if self.geometry.open.contains(position) {
            open
        } else {
            return None;
        };
        self.allowed(action)
    }
    pub fn key_action(&self, code: KeyCode, focused: bool) -> Option<ControlAction> {
        if !focused {
            return None;
        }
        let [previous, next, open] = self.actions();
        let action = match code {
            KeyCode::Left => previous,
            KeyCode::Right => next,
            KeyCode::Enter => open,
            KeyCode::Char('+') if self.kind == ControlKind::Choice => open,
            KeyCode::Char('+') => next,
            KeyCode::Char('-') if self.kind == ControlKind::Number => previous,
            KeyCode::Char('*') if self.kind == ControlKind::Number => open,
            _ => return None,
        };
        self.allowed(action)
    }
    fn actions(&self) -> [ControlAction; 3] {
        match self.kind {
            ControlKind::Choice => [
                ControlAction::PreviousChoice,
                ControlAction::NextChoice,
                ControlAction::OpenChoices,
            ],
            ControlKind::Number => [
                ControlAction::Decrement,
                ControlAction::Increment,
                ControlAction::EditNumber,
            ],
        }
    }
    fn allowed(&self, action: ControlAction) -> Option<ControlAction> {
        let enabled = match action {
            ControlAction::PreviousChoice | ControlAction::Decrement => self.previous_enabled,
            ControlAction::NextChoice | ControlAction::Increment => self.next_enabled,
            ControlAction::OpenChoices | ControlAction::EditNumber => self.open_enabled,
        };
        enabled.then_some(action)
    }
}
/// Heavy Unicode decrement marker used by numeric controls.
pub const NUMBER_DECREMENT_GLYPH: &str = "➖";
/// Heavy Unicode increment marker used by numeric controls.
pub const NUMBER_INCREMENT_GLYPH: &str = "➕";
fn step_button_width(kind: ControlKind, row_width: u16) -> u16 {
    match kind {
        ControlKind::Choice => 1,
        ControlKind::Number if row_width < 6 => 1,
        ControlKind::Number => 2,
    }
}
pub fn cell_width(text: &str) -> usize {
    UnicodeSegmentation::graphemes(text, true)
        .map(UnicodeWidthStr::width)
        .sum()
}
pub fn clip_cells(text: &str, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    let limit = usize::from(width);
    let mut output = String::new();
    let mut used = 0_usize;
    for source in UnicodeSegmentation::graphemes(text, true) {
        let grapheme = if source.chars().any(char::is_control) {
            " "
        } else {
            source
        };
        let next_width = UnicodeWidthStr::width(grapheme);
        if next_width == 0 {
            continue;
        }
        if used + next_width <= limit {
            output.push_str(grapheme);
            used += next_width;
            continue;
        }
        if used == limit {
            if let Some(last) = UnicodeSegmentation::graphemes(output.as_str(), true).next_back() {
                let keep_bytes = output.len() - last.len();
                output.truncate(keep_bytes);
            }
        }
        output.push('…');
        return output;
    }
    output
}
fn paint(frame: &mut Frame, rectangle: Rect, text: &str, style: Style) {
    if rectangle.width == 0 || rectangle.height == 0 {
        return;
    }
    frame.render_widget(Paragraph::new(text).style(style), rectangle);
}
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    fn spec(kind: ControlKind, value: &str) -> ControlSpec<'_> {
        ControlSpec {
            kind,
            label: "Mode",
            value,
            label_width: 0,
            previous_enabled: true,
            next_enabled: true,
            open_enabled: true,
        }
    }
    #[test]
    fn short_numeric_values_keep_their_units_in_narrow_settings_rows() {
        for value in ["8 MiB", "1000 ms", "0.75"] {
            let control = ValueControl::new(
                Rect::new(31, 3, 48, 1),
                ControlSpec {
                    label: "A descriptive settings label",
                    label_width: 42,
                    ..spec(ControlKind::Number, value)
                },
            );
            assert_eq!(control.value_text, value);
            let geometry = control.geometry();
            assert_eq!(
                geometry.value.x - geometry.value_slot.x,
                (geometry.value_slot.width - geometry.value.width) / 2
            );
            assert!(geometry.previous.right() <= geometry.value_slot.x);
            assert!(geometry.value_slot.right() <= geometry.next.x);
            assert!(geometry.next.right() <= geometry.open.x);
            assert!(geometry.open.right() <= geometry.row.right());
        }
    }

    #[test]
    fn five_cells_keep_compact_step_targets_and_direct_entry_visible() {
        let control = ValueControl::new(Rect::new(0, 0, 5, 1), spec(ControlKind::Number, "7"));
        let geometry = control.geometry();
        assert_eq!(geometry.previous, Rect::new(0, 0, 1, 1));
        assert_eq!(geometry.value_slot, Rect::new(1, 0, 2, 1));
        assert_eq!(geometry.value, Rect::new(1, 0, 1, 1));
        assert_eq!(geometry.next, Rect::new(3, 0, 1, 1));
        assert_eq!(geometry.open, Rect::new(4, 0, 1, 1));

        let mut terminal = Terminal::new(TestBackend::new(5, 1)).unwrap();
        terminal
            .draw(|frame| control.render(frame, ControlStyles::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), "-");
        assert_eq!(buffer[(1, 0)].symbol(), "7");
        assert_eq!(buffer[(3, 0)].symbol(), "+");
        assert_eq!(buffer[(4, 0)].symbol(), "*");

        assert_eq!(
            control.hit(Position::new(0, 0), PointerButton::Left),
            Some(ControlAction::Decrement)
        );
        assert_eq!(
            control.hit(Position::new(1, 0), PointerButton::Left),
            Some(ControlAction::EditNumber)
        );
        assert_eq!(
            control.hit(Position::new(3, 0), PointerButton::Left),
            Some(ControlAction::Increment)
        );
        assert_eq!(
            control.hit(Position::new(4, 0), PointerButton::Left),
            Some(ControlAction::EditNumber)
        );
    }

    #[test]
    fn six_cells_render_two_cell_numeric_step_targets_with_full_hit_ranges() {
        let control = ValueControl::new(Rect::new(0, 0, 6, 1), spec(ControlKind::Number, "7"));
        let geometry = control.geometry();
        assert_eq!(geometry.previous, Rect::new(0, 0, 2, 1));
        assert_eq!(geometry.value_slot, Rect::new(2, 0, 1, 1));
        assert_eq!(geometry.value, Rect::new(2, 0, 1, 1));
        assert_eq!(geometry.next, Rect::new(3, 0, 2, 1));
        assert_eq!(geometry.open, Rect::new(5, 0, 1, 1));

        let mut terminal = Terminal::new(TestBackend::new(6, 1)).unwrap();
        terminal
            .draw(|frame| control.render(frame, ControlStyles::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), "➖");
        assert_eq!(buffer[(2, 0)].symbol(), "7");
        assert_eq!(buffer[(3, 0)].symbol(), "➕");
        assert_eq!(buffer[(5, 0)].symbol(), "*");

        for x in 0..2 {
            assert_eq!(
                control.hit(Position::new(x, 0), PointerButton::Left),
                Some(ControlAction::Decrement)
            );
        }
        for x in 3..5 {
            assert_eq!(
                control.hit(Position::new(x, 0), PointerButton::Left),
                Some(ControlAction::Increment)
            );
        }
    }

    #[test]
    fn narrow_layouts_paint_only_their_actual_targets() {
        let expected_rows = [
            "",
            "+",
            "…+",
            "AB+",
            "←…+→",
            "←AB+→",
            "←AB +→",
            "← … + →",
            "← AB + →",
        ];
        for width in 0_u16..=8 {
            let mut options = spec(ControlKind::Choice, "AB");
            options.label_width = u16::MAX;
            let control = ValueControl::new(Rect::new(2, 1, width, 2), options);
            let mut terminal = Terminal::new(TestBackend::new(14, 4)).unwrap();
            terminal
                .draw(|frame| control.render(frame, ControlStyles::default()))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let painted: String = (2..2 + width)
                .map(|x| buffer.cell((x, 1)).unwrap().symbol())
                .collect();
            assert_eq!(painted, expected_rows[usize::from(width)], "width {width}");
            for x in 0_u16..14 {
                let expected = match buffer.cell((x, 1)).unwrap().symbol() {
                    "←" => Some(ControlAction::PreviousChoice),
                    "→" | "A" | "B" | "…" => Some(ControlAction::NextChoice),
                    "+" => Some(ControlAction::OpenChoices),
                    _ => None,
                };
                assert_eq!(
                    control.hit(Position::new(x, 1), PointerButton::Left),
                    expected,
                    "width {width}, x {x}"
                );
                assert_eq!(control.hit(Position::new(x, 2), PointerButton::Left), None);
            }
        }
    }
    #[test]
    fn narrow_numbers_keep_step_and_entry_targets_visible() {
        for width in 1_u16..=10 {
            let control =
                ValueControl::new(Rect::new(0, 0, width, 1), spec(ControlKind::Number, "42"));
            let mut terminal = Terminal::new(TestBackend::new(12, 2)).unwrap();
            terminal
                .draw(|frame| control.render(frame, ControlStyles::default()))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let geometry = control.geometry();
            assert_eq!(geometry.open.width, 1, "width {width}");
            if width == 5 || width >= 7 {
                assert_eq!(control.value_text, "42", "width {width}");
            }
            let (decrement_glyph, increment_glyph) = if width < 6 {
                ("-", "+")
            } else {
                (NUMBER_DECREMENT_GLYPH, NUMBER_INCREMENT_GLYPH)
            };
            let buttons = [
                (geometry.previous, decrement_glyph, ControlAction::Decrement),
                (geometry.next, increment_glyph, ControlAction::Increment),
                (geometry.open, "*", ControlAction::EditNumber),
            ];
            for (rectangle, glyph, action) in buttons {
                if rectangle.width == 0 {
                    continue;
                }
                assert_eq!(rectangle.width, cell_width(glyph) as u16, "width {width}");
                assert_eq!(
                    buffer.cell((rectangle.x, 0)).unwrap().symbol(),
                    glyph,
                    "width {width}"
                );
                assert_eq!(
                    control.hit(Position::new(rectangle.x, 0), PointerButton::Left),
                    Some(action),
                    "width {width}"
                );
            }
            if width < 4 {
                assert_eq!(geometry.previous.width, 0, "width {width}");
                assert_eq!(geometry.next.width, 0, "width {width}");
            } else if width < 6 {
                assert_eq!(geometry.previous.width, 1, "width {width}");
                assert_eq!(geometry.next.width, 1, "width {width}");
            } else {
                assert_eq!(geometry.previous.width, 2, "width {width}");
                assert_eq!(geometry.next.width, 2, "width {width}");
            }
            assert_eq!(
                control.hit(
                    Position::new(geometry.open.x, geometry.open.y),
                    PointerButton::Left
                ),
                Some(ControlAction::EditNumber),
                "width {width}"
            );
            for x in 0..width {
                let position = Position::new(x, 0);
                let expected = if geometry.previous.contains(position) {
                    Some(ControlAction::Decrement)
                } else if geometry.next.contains(position) {
                    Some(ControlAction::Increment)
                } else if geometry.open.contains(position)
                    || control.value_ink().iter().any(|ink| ink.contains(position))
                {
                    Some(ControlAction::EditNumber)
                } else {
                    None
                };
                assert_eq!(
                    control.hit(position, PointerButton::Left),
                    expected,
                    "width {width}, x {x}"
                );
            }
        }
    }
    #[test]
    fn number_buttons_use_heavy_unicode_glyphs_and_two_cell_targets() {
        let control = ValueControl::new(Rect::new(0, 0, 18, 1), spec(ControlKind::Number, "42"));
        let geometry = control.geometry();
        assert_eq!(geometry.previous.width, 2);
        assert_eq!(geometry.next.width, 2);
        assert_eq!(geometry.open.width, 1);
        let left_padding = geometry.value.x - geometry.value_slot.x;
        let right_padding = geometry.value_slot.right() - geometry.value.right();
        assert!(left_padding.abs_diff(right_padding) <= 1);

        let mut terminal = Terminal::new(TestBackend::new(18, 2)).unwrap();
        terminal
            .draw(|frame| control.render(frame, ControlStyles::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        for (rectangle, expected, action) in [
            (geometry.previous, "➖", ControlAction::Decrement),
            (geometry.next, "➕", ControlAction::Increment),
            (geometry.open, "*", ControlAction::EditNumber),
        ] {
            assert_eq!(buffer[(rectangle.x, rectangle.y)].symbol(), expected);
            assert_eq!(rectangle.width, cell_width(expected) as u16);
            assert_eq!(
                control.hit(Position::new(rectangle.x, rectangle.y), PointerButton::Left,),
                Some(action),
            );
        }
    }
    #[test]
    fn internal_value_spaces_belong_to_the_clickable_value() {
        for (kind, left, right) in [
            (
                ControlKind::Choice,
                ControlAction::NextChoice,
                Some(ControlAction::PreviousChoice),
            ),
            (ControlKind::Number, ControlAction::EditNumber, None),
        ] {
            let control =
                ValueControl::new(Rect::new(0, 0, 36, 1), spec(kind, " Local OSM extract "));
            let value = control.geometry().value;
            for offset in [6, 10] {
                let position = Position::new(value.x + offset, value.y);
                assert_eq!(control.hit(position, PointerButton::Left), Some(left));
                assert_eq!(control.hit(position, PointerButton::Right), right);
            }
            for x in [value.x, value.right() - 1] {
                assert_eq!(
                    control.hit(Position::new(x, value.y), PointerButton::Left),
                    None
                );
            }
        }
    }

    #[test]
    fn labels_outer_spaces_and_non_left_buttons_do_not_fall_through() {
        let mut options = spec(ControlKind::Choice, "A B");
        options.label_width = 4;
        let control = ValueControl::new(Rect::new(10, 4, 24, 2), options);
        for x in 9_u16..=34 {
            let expected = match x {
                15 => Some(ControlAction::PreviousChoice),
                17..=19 | 33 => Some(ControlAction::NextChoice),
                31 => Some(ControlAction::OpenChoices),
                _ => None,
            };
            assert_eq!(
                control.hit(Position::new(x, 4), PointerButton::Left),
                expected,
                "x {x}"
            );
            let reverse = if (17..=19).contains(&x) {
                Some(ControlAction::PreviousChoice)
            } else {
                None
            };
            assert_eq!(
                control.hit(Position::new(x, 4), PointerButton::Right),
                reverse,
                "x {x}"
            );
            assert_eq!(control.hit(Position::new(x, 4), PointerButton::Other), None);
            assert_eq!(control.hit(Position::new(x, 5), PointerButton::Left), None);
        }
    }
    #[test]
    fn number_is_centered_and_padding_is_inert() {
        let control = ValueControl::new(Rect::new(2, 1, 13, 1), spec(ControlKind::Number, "42"));
        assert_eq!(control.geometry().value_slot, Rect::new(5, 1, 5, 1));
        assert_eq!(control.geometry().value, Rect::new(6, 1, 2, 1));
        let mut terminal = Terminal::new(TestBackend::new(18, 3)).unwrap();
        terminal
            .draw(|frame| control.render(frame, ControlStyles::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((2, 1)).unwrap().symbol(), "➖");
        assert_eq!(buffer.cell((11, 1)).unwrap().symbol(), "➕");
        assert_eq!(buffer.cell((14, 1)).unwrap().symbol(), "*");
        for x in 1_u16..=15 {
            let expected = match x {
                2 | 3 => Some(ControlAction::Decrement),
                6 | 7 | 14 => Some(ControlAction::EditNumber),
                11 | 12 => Some(ControlAction::Increment),
                _ => None,
            };
            assert_eq!(
                control.hit(Position::new(x, 1), PointerButton::Left),
                expected
            );
            assert_eq!(control.hit(Position::new(x, 1), PointerButton::Right), None);
        }
    }
    #[test]
    fn eighty_column_settings_keep_short_numbers_and_units_readable() {
        // 80 columns - 26 tab cells - 3 gap - 3 help cells - 2 row inset.
        let row = Rect::new(31, 0, 46, 1);
        for (label, value, expected_slot) in [
            ("Scrollback budget", "8 MiB", 7),
            ("Autosave delay", "1000 ms", 9),
        ] {
            let control = ValueControl::new(
                row,
                ControlSpec {
                    kind: ControlKind::Number,
                    label,
                    value,
                    label_width: 39,
                    previous_enabled: true,
                    next_enabled: true,
                    open_enabled: true,
                },
            );
            let geometry = control.geometry();
            assert_eq!(geometry.value_slot.width, expected_slot);
            assert!(geometry.label.width >= cell_width(label) as u16);
            assert_eq!(geometry.value.width, cell_width(value) as u16);
            assert_eq!(geometry.value.x, geometry.value_slot.x + 1);
            assert_eq!(geometry.value.right() + 1, geometry.value_slot.right());
            assert!(geometry.previous.right() < geometry.value_slot.x);
            assert!(geometry.value_slot.right() < geometry.next.x);
            assert!(geometry.next.right() < geometry.open.x);

            let mut terminal = Terminal::new(TestBackend::new(80, 2)).unwrap();
            terminal
                .draw(|frame| control.render(frame, ControlStyles::default()))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let painted_label: String = (geometry.label.x
                ..geometry.label.x + cell_width(label) as u16)
                .map(|x| buffer.cell((x, 0)).unwrap().symbol())
                .collect();
            let painted_value: String = (geometry.value.x..geometry.value.right())
                .map(|x| buffer.cell((x, 0)).unwrap().symbol())
                .collect();
            assert_eq!(painted_label, label);
            assert_eq!(painted_value, value);
            for (rectangle, glyph, action) in [
                (
                    geometry.previous,
                    NUMBER_DECREMENT_GLYPH,
                    ControlAction::Decrement,
                ),
                (
                    geometry.next,
                    NUMBER_INCREMENT_GLYPH,
                    ControlAction::Increment,
                ),
                (geometry.open, "*", ControlAction::EditNumber),
            ] {
                assert_eq!(buffer.cell((rectangle.x, 0)).unwrap().symbol(), glyph);
                assert_eq!(
                    control.hit(Position::new(rectangle.x, 0), PointerButton::Left),
                    Some(action)
                );
                assert_eq!(
                    control.hit(Position::new(rectangle.x, 0), PointerButton::Right),
                    None
                );
            }
            for x in geometry.value.x..geometry.value.right() {
                assert_eq!(
                    control.hit(Position::new(x, 0), PointerButton::Left),
                    Some(ControlAction::EditNumber)
                );
                assert_eq!(control.hit(Position::new(x, 0), PointerButton::Right), None);
            }
            for x in [geometry.value_slot.x, geometry.value_slot.right() - 1] {
                assert_eq!(control.hit(Position::new(x, 0), PointerButton::Left), None);
            }
        }
    }

    #[test]
    fn narrow_choice_borrows_blank_label_cells_without_changing_actions() {
        let control = ValueControl::new(
            Rect::new(31, 0, 46, 1),
            ControlSpec {
                kind: ControlKind::Choice,
                label: "New pane directory",
                value: "Project root",
                label_width: 39,
                previous_enabled: true,
                next_enabled: true,
                open_enabled: true,
            },
        );
        let geometry = control.geometry();
        assert_eq!(geometry.value.width, cell_width("Project root") as u16);
        assert!(geometry.label.width >= cell_width("New pane directory") as u16);
        let mut terminal = Terminal::new(TestBackend::new(80, 2)).unwrap();
        terminal
            .draw(|frame| control.render(frame, ControlStyles::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let painted: String = (geometry.value.x..geometry.value.right())
            .map(|x| buffer.cell((x, 0)).unwrap().symbol())
            .collect();
        assert_eq!(painted, "Project root");
        for (rectangle, glyph, action) in [
            (geometry.previous, "←", ControlAction::PreviousChoice),
            (geometry.open, "+", ControlAction::OpenChoices),
            (geometry.next, "→", ControlAction::NextChoice),
        ] {
            assert_eq!(buffer.cell((rectangle.x, 0)).unwrap().symbol(), glyph);
            assert_eq!(
                control.hit(Position::new(rectangle.x, 0), PointerButton::Left),
                Some(action)
            );
        }
        let word_gap = Position::new(geometry.value.x + 7, 0);
        assert_eq!(
            control.hit(word_gap, PointerButton::Left),
            Some(ControlAction::NextChoice)
        );
        assert_eq!(
            control.hit(word_gap, PointerButton::Right),
            Some(ControlAction::PreviousChoice)
        );
        assert_eq!(
            control.hit(Position::new(geometry.open.x, 0), PointerButton::Right),
            None
        );
    }

    #[test]
    fn long_label_and_wide_grapheme_share_the_bounded_row() {
        let mut options = spec(ControlKind::Choice, "界e\u{301} item");
        options.label = "An extremely long label";
        options.label_width = 22;
        let control = ValueControl::new(Rect::new(0, 0, 30, 1), options);
        let geometry = control.geometry();
        assert_eq!(geometry.label.width, 15);
        assert_eq!(geometry.value_slot.width, 8);
        assert_eq!(geometry.value.width, cell_width(options.value) as u16);
        assert!(geometry.next.right() <= geometry.row.right());
        let mut terminal = Terminal::new(TestBackend::new(30, 1)).unwrap();
        terminal
            .draw(|frame| control.render(frame, ControlStyles::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((geometry.value.x, 0)).unwrap().symbol(), "界");
        assert_eq!(
            buffer.cell((geometry.value.x + 2, 0)).unwrap().symbol(),
            "e\u{301}"
        );
        for x in geometry.value.x..geometry.value.x + 4 {
            assert_eq!(
                control.hit(Position::new(x, 0), PointerButton::Left),
                Some(ControlAction::NextChoice)
            );
        }
        options.value = "界e\u{301} item with more text";
        let clipped = ValueControl::new(Rect::new(0, 0, 30, 1), options);
        assert_eq!(clipped.geometry().label.width, 15);
        assert_eq!(clipped.geometry().value_slot.width, 8);
        assert_eq!(cell_width(&clipped.value_text), 8);
        assert!(clipped.value_text.ends_with('…'));
        assert!(clipped.geometry().next.right() <= clipped.geometry().row.right());
    }
    #[test]
    fn clipping_keeps_unicode_whole_and_single_line() {
        assert_eq!(clip_cells("anything", 0), "");
        assert_eq!(clip_cells("界", 1), "…");
        assert_eq!(clip_cells("界x", 2), "…");
        assert_eq!(clip_cells("e\u{301}xy", 2), "e\u{301}…");
        assert_eq!(clip_cells("\u{301}", 4), "");
        assert_eq!(clip_cells("a\nb\tc\r\nd", 20), "a b c d");
        let emoji = "👩‍❤️‍💋‍👩";
        let emoji_width = cell_width(emoji) as u16;
        assert_eq!(clip_cells(emoji, emoji_width), emoji);
        assert_eq!(clip_cells(emoji, emoji_width.saturating_sub(1)), "…");
        for width in 0_u16..=12 {
            assert!(cell_width(&clip_cells("界e\u{301} 👩‍❤️‍💋‍👩 tail", width)) <= usize::from(width));
        }
    }
    #[test]
    fn wide_and_combining_value_cells_share_the_painted_geometry() {
        let control = ValueControl::new(
            Rect::new(1, 1, 11, 1),
            spec(ControlKind::Number, "界e\u{301}"),
        );
        assert_eq!(control.geometry().value, Rect::new(4, 1, 3, 1));
        let mut terminal = Terminal::new(TestBackend::new(14, 3)).unwrap();
        terminal
            .draw(|frame| control.render(frame, ControlStyles::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((4, 1)).unwrap().symbol(), "界");
        assert_eq!(buffer.cell((6, 1)).unwrap().symbol(), "e\u{301}");
        for x in 4_u16..=6 {
            assert_eq!(
                control.hit(Position::new(x, 1), PointerButton::Left),
                Some(ControlAction::EditNumber)
            );
        }
        for x in [3_u16, 7] {
            assert_eq!(control.hit(Position::new(x, 1), PointerButton::Left), None);
        }
    }
    #[test]
    fn disabled_actions_are_inert_for_mouse_and_keyboard() {
        for kind in [ControlKind::Choice, ControlKind::Number] {
            for disabled in 0_usize..3 {
                let mut options = spec(kind, "7");
                options.previous_enabled = disabled != 0;
                options.next_enabled = disabled != 1;
                options.open_enabled = disabled != 2;
                let control = ValueControl::new(Rect::new(0, 0, 9, 1), options);
                let rectangles = [
                    control.geometry().previous,
                    control.geometry().next,
                    control.geometry().open,
                ];
                let actions = if kind == ControlKind::Choice {
                    [
                        ControlAction::PreviousChoice,
                        ControlAction::NextChoice,
                        ControlAction::OpenChoices,
                    ]
                } else {
                    [
                        ControlAction::Decrement,
                        ControlAction::Increment,
                        ControlAction::EditNumber,
                    ]
                };
                for index in 0_usize..3 {
                    let expected = (index != disabled).then_some(actions[index]);
                    assert_eq!(
                        control.hit(Position::new(rectangles[index].x, 0), PointerButton::Left),
                        expected
                    );
                    assert_eq!(
                        control.key_action(
                            [KeyCode::Left, KeyCode::Right, KeyCode::Enter][index],
                            true
                        ),
                        expected
                    );
                }
                let value_position = Position::new(control.geometry().value.x, 0);
                let value_index = if kind == ControlKind::Choice { 1 } else { 2 };
                assert_eq!(
                    control.hit(value_position, PointerButton::Left),
                    (disabled != value_index).then_some(actions[value_index])
                );
                let reverse = if kind == ControlKind::Choice && disabled != 0 {
                    Some(ControlAction::PreviousChoice)
                } else {
                    None
                };
                assert_eq!(control.hit(value_position, PointerButton::Right), reverse);
            }
        }
    }
    #[test]
    fn keyboard_actions_require_focus_and_keep_their_distinct_meanings() {
        let number = ValueControl::new(Rect::new(4, 2, 0, 1), spec(ControlKind::Number, "10"));
        assert_eq!(
            number.key_action(KeyCode::Char('*'), true),
            Some(ControlAction::EditNumber)
        );
        assert_eq!(
            number.key_action(KeyCode::Char('+'), true),
            Some(ControlAction::Increment)
        );
        assert_eq!(
            number.key_action(KeyCode::Char('-'), true),
            Some(ControlAction::Decrement)
        );
        assert_eq!(number.key_action(KeyCode::Enter, false), None);
        assert_eq!(number.key_action(KeyCode::Esc, true), None);
        assert_eq!(number.key_action(KeyCode::Tab, true), None);
        assert_eq!(number.hit(Position::new(4, 2), PointerButton::Left), None);
        let choice = ValueControl::new(Rect::new(0, 0, 1, 1), spec(ControlKind::Choice, "A"));
        assert_eq!(
            choice.key_action(KeyCode::Char('+'), true),
            Some(ControlAction::OpenChoices)
        );
        assert_eq!(choice.key_action(KeyCode::Char('*'), true), None);
        assert_eq!(
            choice.key_action(KeyCode::Left, true),
            Some(ControlAction::PreviousChoice)
        );
    }
    #[test]
    fn empty_and_extreme_allocations_have_no_phantom_cells() {
        for area in [Rect::new(7, 3, 0, 4), Rect::new(7, 3, 12, 0)] {
            let control = ValueControl::new(area, spec(ControlKind::Number, "9"));
            let geometry = control.geometry();
            for rectangle in [
                geometry.previous,
                geometry.next,
                geometry.open,
                geometry.value,
            ] {
                assert!(rectangle.width == 0 || rectangle.height == 0);
            }
            assert!(control.value_ink().is_empty());
            assert_eq!(control.hit(Position::new(7, 3), PointerButton::Left), None);
        }
        let area = Rect {
            x: u16::MAX - 2,
            y: 4,
            width: 20,
            height: 2,
        };
        let control = ValueControl::new(area, spec(ControlKind::Choice, "A"));
        assert_eq!(control.geometry().row, Rect::new(u16::MAX - 2, 4, 2, 1));
        assert_eq!(
            control.hit(Position::new(u16::MAX - 1, 4), PointerButton::Left),
            Some(ControlAction::OpenChoices)
        );
        assert_eq!(
            control.hit(Position::new(u16::MAX, 4), PointerButton::Left),
            None
        );
        let bottom = ValueControl::new(
            Rect {
                x: 0,
                y: u16::MAX,
                width: 9,
                height: 1,
            },
            spec(ControlKind::Number, "1"),
        );
        assert_eq!(bottom.geometry().row.height, 0);
        assert_eq!(
            bottom.hit(Position::new(0, u16::MAX), PointerButton::Left),
            None
        );
    }
}
