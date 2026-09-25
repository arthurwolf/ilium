use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui_textarea::TextArea;

use ilium_ipc::{TextTrigger, TextTriggerTarget};

use crate::modal;
use crate::text_prompt::TextPromptState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTriggerFocus {
    Regexp,
    Message,
    Target,
    Sample,
    Enabled,
    Save,
}

impl TextTriggerFocus {
    pub const fn next(self) -> Self {
        match self {
            Self::Regexp => Self::Message,
            Self::Message => Self::Target,
            Self::Target => Self::Sample,
            Self::Sample => Self::Enabled,
            Self::Enabled => Self::Save,
            Self::Save => Self::Regexp,
        }
    }
    pub const fn previous(self) -> Self {
        match self {
            Self::Regexp => Self::Save,
            Self::Message => Self::Regexp,
            Self::Target => Self::Message,
            Self::Sample => Self::Target,
            Self::Enabled => Self::Sample,
            Self::Save => Self::Enabled,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextTriggerDialogState {
    pub editing_index: Option<usize>,
    pub regexp: TextPromptState,
    pub message: TextPromptState,
    pub target: TextTriggerTarget,
    pub sample: TextArea<'static>,
    pub enabled: bool,
    pub focus: TextTriggerFocus,
}

impl TextTriggerDialogState {
    pub fn new(existing: Option<(usize, &TextTrigger)>) -> Self {
        let (editing_index, trigger) = match existing {
            Some((index, trigger)) => (Some(index), trigger.clone()),
            None => (None, TextTrigger::default()),
        };
        Self {
            editing_index,
            regexp: TextPromptState::new(trigger.regexp),
            message: TextPromptState::new(trigger.message),
            target: trigger.target,
            sample: TextArea::from(
                trigger
                    .sample_text
                    .split('\n')
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            ),
            enabled: trigger.enabled,
            focus: TextTriggerFocus::Regexp,
        }
    }
    pub fn sample_text(&self) -> String {
        self.sample.lines().join("\n")
    }
    pub fn candidate(&self) -> TextTrigger {
        TextTrigger {
            id: self
                .editing_index
                .map(|_| String::new())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            enabled: self.enabled,
            regexp: self.regexp.buf.clone(),
            message: self.message.buf.clone(),
            target: self.target,
            sample_text: self.sample_text(),
        }
    }
}

pub struct TextTriggerDialogLayout {
    pub popup: Rect,
    pub regexp: Rect,
    pub message: Rect,
    pub target: Rect,
    pub sample: Rect,
    pub enabled: Rect,
    pub preview: Rect,
    pub save: Rect,
    pub hint: Rect,
}
pub fn layout(area: Rect) -> TextTriggerDialogLayout {
    let popup = modal::centered_fixed_rect(94, 28, area);
    let inner = Rect::new(
        popup.x + 2,
        popup.y + 2,
        popup.width.saturating_sub(4),
        popup.height.saturating_sub(4),
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let save = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16)])
        .flex(Flex::Center)
        .split(rows[7])[0];
    TextTriggerDialogLayout {
        popup,
        regexp: rows[0],
        message: rows[1],
        target: rows[2],
        sample: rows[4],
        enabled: rows[5],
        preview: rows[6],
        save,
        hint: rows[8],
    }
}
