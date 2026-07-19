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
        let next = (current as i8 + direction).rem_euclid(choices.len() as i8) as usize;
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
