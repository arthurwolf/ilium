//! Dialog state, validation, and shared geometry for scheduling terminal input.
//!
//! The detached server owns the deadline and execution; this client module is
//! deliberately limited to collecting a relative duration plus the payload and
//! producing a validated `ClientRequest` through `App`.

use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::KeyCode;
use ilium_core::NodeId;
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};

use crate::modal;
use crate::text_prompt::{self, TextPromptState};

const DEFAULT_SECONDS: &str = "30";
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;

/// Wall-clock value matching the detached server's persisted deadline unit.
/// A pre-epoch clock falls back to zero so the countdown stays visible while
/// the server reports the actual scheduling error through IPC.
pub fn unix_millis_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

/// The currently active dialog control, ordered exactly as Tab traverses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduledInputFocus {
    Hours,
    Minutes,
    Seconds,
    Text,
    SendEnter,
    ScheduleButton,
}

impl ScheduledInputFocus {
    pub const fn next(self) -> Self {
        match self {
            Self::Hours => Self::Minutes,
            Self::Minutes => Self::Seconds,
            Self::Seconds => Self::Text,
            Self::Text => Self::SendEnter,
            Self::SendEnter => Self::ScheduleButton,
            Self::ScheduleButton => Self::Hours,
        }
    }

    pub const fn previous(self) -> Self {
        match self {
            Self::Hours => Self::ScheduleButton,
            Self::Minutes => Self::Hours,
            Self::Seconds => Self::Minutes,
            Self::Text => Self::Seconds,
            Self::SendEnter => Self::Text,
            Self::ScheduleButton => Self::SendEnter,
        }
    }
}

/// Client-local values for one open scheduling dialog.
pub struct ScheduledInputDialogState {
    pub pane_id: NodeId,
    pub hours: TextPromptState,
    pub minutes: TextPromptState,
    pub seconds: TextPromptState,
    pub text: TextPromptState,
    pub send_enter: bool,
    pub focus: ScheduledInputFocus,
}

impl ScheduledInputDialogState {
    pub fn new(pane_id: NodeId) -> Self {
        Self {
            pane_id,
            hours: TextPromptState::new(""),
            minutes: TextPromptState::new(""),
            seconds: TextPromptState::new(DEFAULT_SECONDS),
            text: TextPromptState::new(""),
            send_enter: true,
            focus: ScheduledInputFocus::Hours,
        }
    }

    /// Parses and bounds the three duration fields. Minutes and seconds use
    /// clock-style 0-59 ranges; hours remains unbounded up to `u64` arithmetic.
    pub fn delay_seconds(&self) -> Result<u64, String> {
        let hours = parse_duration_field(&self.hours.buf, "Hours")?;
        let minutes = parse_duration_field(&self.minutes.buf, "Minutes")?;
        let seconds = parse_duration_field(&self.seconds.buf, "Seconds")?;
        if minutes >= SECONDS_PER_MINUTE {
            return Err("Minutes must be between 0 and 59".to_string());
        }
        if seconds >= SECONDS_PER_MINUTE {
            return Err("Seconds must be between 0 and 59".to_string());
        }
        let total = hours
            .checked_mul(SECONDS_PER_HOUR)
            .and_then(|value| value.checked_add(minutes * SECONDS_PER_MINUTE))
            .and_then(|value| value.checked_add(seconds))
            .ok_or_else(|| "The duration is too large".to_string())?;
        if total == 0 {
            return Err("Choose a delay of at least one second".to_string());
        }
        Ok(total)
    }

    /// Validates both the countdown and the three supported payload forms:
    /// text only, text plus Enter, or Enter only.
    pub fn validated_request(&self) -> Result<(u64, String, bool), String> {
        let delay_seconds = self.delay_seconds()?;
        if self.text.buf.is_empty() && !self.send_enter {
            return Err("Enter text, enable Send Enter, or use both".to_string());
        }
        Ok((delay_seconds, self.text.buf.clone(), self.send_enter))
    }

    /// Numeric controls ignore non-digits instead of allowing an invalid
    /// character that only surfaces as an error after the whole form is done.
    pub fn handle_numeric_key(&mut self, code: KeyCode) {
        let prompt = match self.focus {
            ScheduledInputFocus::Hours => &mut self.hours,
            ScheduledInputFocus::Minutes => &mut self.minutes,
            ScheduledInputFocus::Seconds => &mut self.seconds,
            _ => return,
        };
        if matches!(code, KeyCode::Char(character) if !character.is_ascii_digit()) {
            return;
        }
        let _ = text_prompt::handle_key(prompt, code);
    }

    pub fn handle_text_key(&mut self, code: KeyCode) {
        if matches!(code, KeyCode::Char(character) if character.is_control()) {
            return;
        }
        let _ = text_prompt::handle_key(&mut self.text, code);
    }

    /// Paste uses the same per-character filters as typing, keeping numeric
    /// fields strict while allowing arbitrary Unicode in the payload field.
    pub fn paste(&mut self, pasted: &str) {
        for character in pasted.chars() {
            match self.focus {
                ScheduledInputFocus::Hours
                | ScheduledInputFocus::Minutes
                | ScheduledInputFocus::Seconds => {
                    if character.is_ascii_digit() {
                        self.handle_numeric_key(KeyCode::Char(character));
                    }
                }
                ScheduledInputFocus::Text => self.handle_text_key(KeyCode::Char(character)),
                ScheduledInputFocus::SendEnter | ScheduledInputFocus::ScheduleButton => {}
            }
        }
    }
}

fn parse_duration_field(value: &str, label: &str) -> Result<u64, String> {
    if value.is_empty() {
        return Ok(0);
    }
    value
        .parse::<u64>()
        .map_err(|_| format!("{label} must be a whole number"))
}

/// Every rectangle used by both rendering and mouse hit-testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledInputDialogLayout {
    pub popup: Rect,
    pub subtitle: Rect,
    pub duration_label: Rect,
    pub hours: Rect,
    pub minutes: Rect,
    pub seconds: Rect,
    pub payload_label: Rect,
    pub text: Rect,
    pub send_enter: Rect,
    pub schedule_button: Rect,
    pub hint: Rect,
}

/// Builds a spacious form on normal terminals and safely collapses inside a
/// smaller screen. Spacers are explicit layout rows rather than fake blank
/// text, so rendering and click geometry remain identical.
pub fn dialog_layout(screen_area: Rect) -> ScheduledInputDialogLayout {
    let popup = modal::centered_fixed_rect(76, 20, screen_area);
    let inner = Rect::new(
        popup.x.saturating_add(3),
        popup.y.saturating_add(2),
        popup.width.saturating_sub(6),
        popup.height.saturating_sub(4),
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let duration_fields = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(18),
            Constraint::Length(18),
            Constraint::Length(18),
        ])
        .flex(Flex::SpaceBetween)
        .split(rows[3]);
    let schedule_button = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22)])
        .flex(Flex::Center)
        .split(rows[9])[0];
    ScheduledInputDialogLayout {
        popup,
        subtitle: rows[0],
        duration_label: rows[2],
        hours: duration_fields[0],
        minutes: duration_fields[1],
        seconds: duration_fields[2],
        payload_label: rows[5],
        text: rows[6],
        send_enter: rows[7],
        schedule_button,
        hint: rows[10],
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Position;

    use super::*;

    #[test]
    fn validates_text_only_text_and_enter_and_enter_only() {
        let mut state = ScheduledInputDialogState::new(NodeId(7));
        state.text = TextPromptState::new("status");
        state.send_enter = false;
        assert_eq!(
            state.validated_request().unwrap(),
            (30, "status".to_string(), false)
        );

        state.send_enter = true;
        assert_eq!(
            state.validated_request().unwrap(),
            (30, "status".to_string(), true)
        );

        state.text = TextPromptState::new("");
        assert_eq!(
            state.validated_request().unwrap(),
            (30, String::new(), true)
        );
    }

    #[test]
    fn rejects_zero_invalid_clock_fields_and_empty_payload() {
        let mut state = ScheduledInputDialogState::new(NodeId(7));
        state.hours = TextPromptState::new("0");
        state.minutes = TextPromptState::new("0");
        state.seconds = TextPromptState::new("0");
        assert_eq!(
            state.delay_seconds().unwrap_err(),
            "Choose a delay of at least one second"
        );

        state.minutes = TextPromptState::new("60");
        assert_eq!(
            state.delay_seconds().unwrap_err(),
            "Minutes must be between 0 and 59"
        );

        state.minutes = TextPromptState::new("0");
        state.seconds = TextPromptState::new("1");
        state.send_enter = false;
        assert_eq!(
            state.validated_request().unwrap_err(),
            "Enter text, enable Send Enter, or use both"
        );
    }

    #[test]
    fn numeric_input_filters_letters_and_duration_uses_checked_units() {
        let mut state = ScheduledInputDialogState::new(NodeId(7));
        state.hours = TextPromptState::new("");
        state.handle_numeric_key(KeyCode::Char('a'));
        state.handle_numeric_key(KeyCode::Char('2'));
        state.minutes = TextPromptState::new("3");
        state.seconds = TextPromptState::new("4");
        assert_eq!(state.delay_seconds().unwrap(), 7384);
    }

    #[test]
    fn dialog_layout_is_aerated_and_clamped_to_small_screens() {
        let screen = Rect::new(0, 0, 100, 30);
        let layout = dialog_layout(screen);
        assert!(layout
            .popup
            .contains(Position::new(layout.hours.x, layout.hours.y)));
        assert!(layout.hours.right() < layout.minutes.x);
        assert!(layout.minutes.right() < layout.seconds.x);
        assert!(layout.text.y > layout.duration_label.y);

        let small = Rect::new(0, 0, 24, 10);
        let small_layout = dialog_layout(small);
        assert!(small_layout.popup.width <= small.width);
        assert!(small_layout.popup.height <= small.height);
    }
}
