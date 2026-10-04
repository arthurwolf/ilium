//! Client-only form state for a completion-driven terminal prompt queue.

use crossterm::event::KeyCode;
use ilium_core::{NodeId, PromptQueueDelivery};
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};

use crate::modal;
use crate::text_prompt::{self, TextPromptState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptQueueFocus {
    Text,
    Delivery,
    Times,
    EnqueueButton,
}

impl PromptQueueFocus {
    pub const fn next(self) -> Self {
        match self {
            Self::Text => Self::Delivery,
            Self::Delivery => Self::Times,
            Self::Times => Self::EnqueueButton,
            Self::EnqueueButton => Self::Text,
        }
    }

    pub const fn previous(self) -> Self {
        match self {
            Self::Text => Self::EnqueueButton,
            Self::Delivery => Self::Text,
            Self::Times => Self::Delivery,
            Self::EnqueueButton => Self::Times,
        }
    }
}

pub struct PromptQueueDialogState {
    pub pane_id: NodeId,
    pub text: TextPromptState,
    pub delivery_choice: PromptQueueDelivery,
    pub times: TextPromptState,
    pub focus: PromptQueueFocus,
}

impl PromptQueueDialogState {
    pub fn new(pane_id: NodeId) -> Self {
        Self {
            pane_id,
            text: TextPromptState::new(""),
            delivery_choice: PromptQueueDelivery::Once,
            times: TextPromptState::new("2"),
            focus: PromptQueueFocus::Text,
        }
    }

    pub fn cycle_delivery(&mut self, direction: i8) {
        let choices = [
            PromptQueueDelivery::Once,
            PromptQueueDelivery::Times { remaining_runs: 2 },
            PromptQueueDelivery::Forever,
        ];
        let current = choices
            .iter()
            .position(|choice| {
                std::mem::discriminant(choice) == std::mem::discriminant(&self.delivery_choice)
            })
            .unwrap_or(0);
        // Widen to i32 before adding: `direction` is caller-supplied and an
        // i8 sum of `current` (0..=2) with a `direction` near i8::MIN/MAX
        // would overflow and panic under debug overflow checks.
        let next = (current as i32 + direction as i32).rem_euclid(choices.len() as i32) as usize;
        self.delivery_choice = choices[next].clone();
    }

    pub fn validated_request(&self) -> Result<(String, PromptQueueDelivery), String> {
        if self.text.buf.trim().is_empty() {
            return Err("Write a prompt before enqueueing it".to_string());
        }
        let delivery = match self.delivery_choice {
            PromptQueueDelivery::Once => PromptQueueDelivery::Once,
            PromptQueueDelivery::Forever => PromptQueueDelivery::Forever,
            PromptQueueDelivery::Times { .. } => {
                let remaining_runs = self
                    .times
                    .buf
                    .parse::<u32>()
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| "Run count must be a positive whole number".to_string())?;
                PromptQueueDelivery::Times { remaining_runs }
            }
        };
        Ok((self.text.buf.clone(), delivery))
    }

    pub fn handle_text_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Enter => {
                let _ = text_prompt::handle_key(&mut self.text, KeyCode::Char('\n'));
            }
            _ => {
                let _ = text_prompt::handle_key(&mut self.text, code);
            }
        }
    }

    pub fn handle_times_key(&mut self, code: KeyCode) {
        if matches!(code, KeyCode::Char(character) if !character.is_ascii_digit()) {
            return;
        }
        let _ = text_prompt::handle_key(&mut self.times, code);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptQueueDialogLayout {
    pub popup: Rect,
    pub subtitle: Rect,
    pub prompt_label: Rect,
    pub text: Rect,
    pub delivery_label: Rect,
    pub delivery: Rect,
    pub times: Rect,
    pub warning: Rect,
    pub enqueue_button: Rect,
    pub hint: Rect,
}

pub fn dialog_layout(screen_area: Rect) -> PromptQueueDialogLayout {
    let popup = modal::centered_fixed_rect(76, 21, screen_area);
    let inner = Rect::new(
        popup.x + 3,
        popup.y + 2,
        popup.width.saturating_sub(6),
        popup.height.saturating_sub(4),
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let enqueue_button = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16)])
        .flex(Flex::Center)
        .split(rows[8])[0];
    PromptQueueDialogLayout {
        popup,
        subtitle: rows[0],
        prompt_label: rows[2],
        text: rows[3],
        delivery_label: rows[4],
        delivery: rows[5],
        times: rows[6],
        warning: rows[7],
        enqueue_button,
        hint: rows[9],
    }
}

impl PromptQueueDialogState {
    pub fn delivery_control(&self, area: Rect) -> crate::value_control::ValueControl {
        use crate::value_control::{ControlKind, ControlSpec, ValueControl};
        let value = match self.delivery_choice {
            PromptQueueDelivery::Once => "Once",
            PromptQueueDelivery::Times { .. } => "Run X times",
            PromptQueueDelivery::Forever => "Enqueue forever (DANGER)",
        };
        ValueControl::new(
            area,
            ControlSpec {
                kind: ControlKind::Choice,
                label: "",
                value,
                label_width: 0,
                previous_enabled: true,
                next_enabled: true,
                open_enabled: true,
            },
        )
    }
}

impl PromptQueueDialogState {
    /// Prepare the exact repeat-count row used by rendering and mouse dispatch.
    pub fn times_control(&self, area: Rect) -> crate::value_control::ValueControl {
        use crate::value_control::{ControlKind, ControlSpec, ValueControl};
        let enabled = matches!(self.delivery_choice, PromptQueueDelivery::Times { .. });
        let value = if enabled {
            self.times.buf.as_str()
        } else {
            "(only for Run X times)"
        };
        let count = self
            .times
            .buf
            .parse::<u32>()
            .ok()
            .filter(|count| *count > 0);
        ValueControl::new(
            ratatui::widgets::Block::bordered().inner(area),
            ControlSpec {
                kind: ControlKind::Number,
                label: "",
                value,
                label_width: 0,
                previous_enabled: enabled && count.is_some_and(|count| count > 1),
                next_enabled: enabled && count.is_some_and(|count| count < u32::MAX),
                open_enabled: enabled,
            },
        )
    }

    /// Buttons edit the draft count; enqueueing remains the separate form action.
    pub fn apply_times_control(
        &mut self,
        action: crate::value_control::ControlAction,
    ) -> Result<(), String> {
        use crate::value_control::ControlAction;
        use crate::value_number::{NumberSpec, NumberValue};
        if !matches!(self.delivery_choice, PromptQueueDelivery::Times { .. }) {
            return Err("Select Run X times before editing the repeat count".into());
        }
        let direction = match action {
            ControlAction::Decrement => -1,
            ControlAction::Increment => 1,
            ControlAction::EditNumber => {
                self.focus = PromptQueueFocus::Times;
                return Ok(());
            }
            _ => return Err("Unsupported repeat-count action".into()),
        };
        let spec = NumberSpec::Integer {
            minimum: 1,
            maximum: i128::from(u32::MAX),
        };
        let value = spec.parse(&self.times.buf)?;
        let NumberValue::Integer(next) = spec.stepped(value, NumberValue::Integer(1), direction)?
        else {
            return Err("Run count must be a whole number".into());
        };
        self.times = TextPromptState::new(next.to_string());
        self.focus = PromptQueueFocus::Times;
        Ok(())
    }
}

#[cfg(test)]
mod value_tests {
    use super::*;
    use crate::value_control::{ControlAction, PointerButton};
    use ratatui::layout::Position;
    fn times() -> PromptQueueDialogState {
        let mut s = PromptQueueDialogState::new(NodeId(1));
        s.delivery_choice = PromptQueueDelivery::Times { remaining_runs: 2 };
        s
    }
    #[test]
    fn repeat_count_buttons_step_and_saturate_without_submitting() {
        let mut s = times();
        s.apply_times_control(ControlAction::Increment).unwrap();
        assert_eq!(s.times.buf, "3");
        s.apply_times_control(ControlAction::Decrement).unwrap();
        assert_eq!(s.times.buf, "2");
        s.times = TextPromptState::new("1");
        s.apply_times_control(ControlAction::Decrement).unwrap();
        assert_eq!(s.times.buf, "1");
        s.times = TextPromptState::new(u32::MAX.to_string());
        s.apply_times_control(ControlAction::Increment).unwrap();
        assert_eq!(s.times.buf, u32::MAX.to_string());
        assert!(s.text.buf.is_empty());
    }
    #[test]
    fn direct_entry_focus_preserves_draft_and_invalid_numbers() {
        let mut s = times();
        s.times = TextPromptState::new("not a count");
        s.apply_times_control(ControlAction::EditNumber).unwrap();
        assert_eq!(s.focus, PromptQueueFocus::Times);
        assert_eq!(s.times.buf, "not a count");
        assert!(s.apply_times_control(ControlAction::Increment).is_err());
        assert_eq!(s.times.buf, "not a count");
    }
    #[test]
    fn repeat_count_has_no_active_controls_outside_times_mode() {
        let s = PromptQueueDialogState::new(NodeId(1));
        let c = s.times_control(Rect::new(10, 3, 21, 3));
        for x in 10..31 {
            assert_eq!(
                c.hit(Position::new(x, c.geometry().row.y), PointerButton::Left),
                None
            );
        }
        assert_eq!(c.key_action(KeyCode::Enter, true), None);
    }
    #[test]
    fn repeat_control_geometry_uses_inner_row_and_centers_value() {
        let s = times();
        let c = s.times_control(Rect::new(10, 3, 21, 3));
        assert_eq!(c.geometry().row, Rect::new(11, 4, 19, 1));
        assert_eq!(c.geometry().value, Rect::new(19, 4, 1, 1));
        assert_eq!(
            c.hit(Position::new(11, 4), PointerButton::Left),
            Some(ControlAction::Decrement)
        );
        assert_eq!(
            c.hit(Position::new(27, 4), PointerButton::Left),
            Some(ControlAction::Increment)
        );
        assert_eq!(
            c.hit(Position::new(29, 4), PointerButton::Left),
            Some(ControlAction::EditNumber)
        );
        assert_eq!(c.hit(Position::new(12, 4), PointerButton::Left), None);
    }
}

#[cfg(test)]
mod delivery_control_tests {
    use super::*;
    use crate::value_control::{ControlAction, ControlStyles, PointerButton};
    use ratatui::{backend::TestBackend, layout::Position, Terminal};
    #[test]
    fn delivery_glyphs_value_clicks_and_option_button_share_rendered_cells() {
        let state = PromptQueueDialogState::new(NodeId(1));
        let control = state.delivery_control(Rect::new(0, 0, 40, 1));
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).unwrap();
        terminal
            .draw(|frame| control.render(frame, ControlStyles::default()))
            .unwrap();
        let geometry = control.geometry();
        for (area, glyph, action) in [
            (geometry.previous, "←", ControlAction::PreviousChoice),
            (geometry.open, "+", ControlAction::OpenChoices),
            (geometry.next, "→", ControlAction::NextChoice),
        ] {
            assert_eq!(
                terminal
                    .backend()
                    .buffer()
                    .cell((area.x, area.y))
                    .unwrap()
                    .symbol(),
                glyph
            );
            assert_eq!(
                control.hit(Position::new(area.x, area.y), PointerButton::Left),
                Some(action)
            );
        }
        assert_eq!(
            control.hit(
                Position::new(geometry.value.x, geometry.value.y),
                PointerButton::Left
            ),
            Some(ControlAction::NextChoice)
        );
        assert_eq!(
            control.hit(
                Position::new(geometry.value.x, geometry.value.y),
                PointerButton::Right
            ),
            Some(ControlAction::PreviousChoice)
        );
        assert_eq!(
            control.hit(
                Position::new(geometry.open.x, geometry.open.y),
                PointerButton::Right
            ),
            None
        );
        assert_eq!(
            control.key_action(KeyCode::Enter, true),
            Some(ControlAction::OpenChoices)
        );
    }
}
