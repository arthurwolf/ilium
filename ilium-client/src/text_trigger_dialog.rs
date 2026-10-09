use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui_textarea::TextArea;

use ilium_ipc::{TextTrigger, TextTriggerTarget};

use crate::modal;
use crate::text_prompt::TextPromptState;
mod preview;
pub(crate) use preview::{limits as preview_limits, TextTriggerPreview};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextTriggerPreviewIssue {
    Unconfigured,
    CaptureLimit,
    OutputLimit,
    CpuRequired,
    Cancelled,
    Shutdown,
    WorkerPanicked,
    ReceiptLost,
    Admission(ilium_execution::RejectReason),
    Publication(ilium_execution::RejectReason),
}

impl TextTriggerPreviewIssue {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Unconfigured => "Preview worker is unavailable in this client.",
            Self::CaptureLimit => "Preview draft exceeds the bounded capture limit.",
            Self::OutputLimit => "Preview output exceeds the bounded result limit.",
            Self::CpuRequired => "Preview work did not execute on the CPU bank.",
            Self::Cancelled => "Preview work was cancelled before completion.",
            Self::Shutdown => "Preview execution is shutting down.",
            Self::WorkerPanicked => "Preview worker panicked; the draft was retained.",
            Self::ReceiptLost => "Preview completion was lost; the draft was retained.",
            Self::Admission(reason) => preview_rejection_message(reason),
            Self::Publication(reason) => preview_publication_message(reason),
        }
    }
}

const fn preview_rejection_message(reason: ilium_execution::RejectReason) -> &'static str {
    use ilium_execution::RejectReason;
    match reason {
        RejectReason::Busy => "Preview admission is temporarily busy.",
        RejectReason::Closed => "Preview execution is closed.",
        RejectReason::QueueFull => "Preview CPU queue is full.",
        RejectReason::ServiceBankFull => "Preview admission reached an unavailable service bank.",
        RejectReason::ClientLimit => "Preview client admission limit was reached.",
        RejectReason::JobLimit => "Preview job admission limit was reached.",
        RejectReason::ServiceLimit => "Preview admission reached an unavailable service limit.",
        RejectReason::InputBytes => "Preview input-byte admission limit was reached.",
        RejectReason::ResultBytes => "Preview result-byte admission limit was reached.",
        RejectReason::WorkerLimit => "Preview worker admission limit was reached.",
        RejectReason::WorkerBytes => "Preview retirement storage admission limit was reached.",
        RejectReason::InvalidCost => "Preview resource declaration is invalid.",
        RejectReason::AccountingPoisoned => "Preview resource accounting is unavailable.",
    }
}

const fn preview_publication_message(reason: ilium_execution::RejectReason) -> &'static str {
    use ilium_execution::RejectReason;
    match reason {
        RejectReason::Closed => "Preview execution closed before publication.",
        RejectReason::QueueFull => "Preview publication queue refused the reserved job.",
        _ => preview_rejection_message(reason),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextTriggerPreviewPhase {
    Pending,
    Ready,
    Unavailable(TextTriggerPreviewIssue),
}

#[derive(Debug, Clone)]
struct TextTriggerPreviewPresentation {
    revision: u64,
    text: ilium_execution::RetiringArc<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTriggerFocus {
    Regexp,
    Message,
    Target,
    Delay,
    Sample,
    Enabled,
    Save,
}

impl TextTriggerFocus {
    pub const fn next(self) -> Self {
        match self {
            Self::Regexp => Self::Message,
            Self::Message => Self::Target,
            Self::Target => Self::Delay,
            Self::Delay => Self::Sample,
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
            Self::Delay => Self::Target,
            Self::Sample => Self::Delay,
            Self::Enabled => Self::Sample,
            Self::Save => Self::Enabled,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextTriggerDialogState {
    pub editing_index: Option<usize>,
    /// Identity captured when the editor opens; list positions can change.
    pub editing_id: Option<String>,
    pub editing_base: Option<TextTrigger>,
    draft_id: String,
    pub save_error: Option<String>,
    pub regexp: TextPromptState,
    pub message: TextPromptState,
    pub target: TextTriggerTarget,
    /// Digits only; blank means the default delay.
    pub delay: TextPromptState,
    pub sample: TextArea<'static>,
    pub enabled: bool,
    pub focus: TextTriggerFocus,
    preview_revision: u64,
    preview_phase: TextTriggerPreviewPhase,
    preview: Option<TextTriggerPreviewPresentation>,
}

impl TextTriggerDialogState {
    pub fn new(existing: Option<(usize, &TextTrigger)>) -> Self {
        let (editing_index, trigger) = match existing {
            Some((index, trigger)) => (Some(index), trigger.clone()),
            None => (None, TextTrigger::default()),
        };
        Self {
            editing_index,
            editing_id: editing_index.map(|_| trigger.id.clone()),
            editing_base: editing_index.map(|_| trigger.clone()),
            draft_id: uuid::Uuid::new_v4().to_string(),
            save_error: None,
            regexp: TextPromptState::new(trigger.regexp),
            message: TextPromptState::new(trigger.message),
            target: trigger.target,
            delay: TextPromptState::new(trigger.delay_seconds.to_string()),
            sample: TextArea::from(
                trigger
                    .sample_text
                    .split('\n')
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            ),
            enabled: trigger.enabled,
            focus: TextTriggerFocus::Regexp,
            preview_revision: 0,
            preview_phase: TextTriggerPreviewPhase::Pending,
            preview: None,
        }
    }
    pub fn identity(&self) -> &str {
        &self.draft_id
    }

    pub(crate) fn preview_revision(&self) -> u64 {
        self.preview_revision
    }

    pub(crate) fn preview_is_current(&self) -> bool {
        matches!(self.preview_phase, TextTriggerPreviewPhase::Ready)
            && self
                .preview
                .as_ref()
                .is_some_and(|preview| preview.revision == self.preview_revision)
    }

    pub(crate) fn preview_issue(&self) -> Option<TextTriggerPreviewIssue> {
        match self.preview_phase {
            TextTriggerPreviewPhase::Unavailable(issue) => Some(issue),
            _ => None,
        }
    }

    pub(crate) fn preview_text(&self) -> Option<&str> {
        self.preview.as_ref().map(|preview| preview.text.as_str())
    }

    pub(crate) fn preview_display(&self) -> (&str, &'static str) {
        match (self.preview_phase, self.preview_text()) {
            (TextTriggerPreviewPhase::Ready, Some(text)) => (text, "LIVE PREVIEW"),
            (TextTriggerPreviewPhase::Pending, Some(text)) => (text, "LIVE PREVIEW · updating…"),
            (TextTriggerPreviewPhase::Pending, None) => {
                ("Preparing preview…", "LIVE PREVIEW · preparing…")
            }
            (TextTriggerPreviewPhase::Unavailable(_), Some(text)) => {
                (text, "LIVE PREVIEW · previous result; unavailable")
            }
            (TextTriggerPreviewPhase::Unavailable(issue), None) => {
                (issue.message(), "LIVE PREVIEW · unavailable")
            }
            (TextTriggerPreviewPhase::Ready, None) => {
                ("Preparing preview…", "LIVE PREVIEW · preparing…")
            }
        }
    }

    pub(crate) fn mark_preview_dirty(&mut self) {
        self.preview_revision = self
            .preview_revision
            .checked_add(1)
            .expect("text trigger preview revision overflow");
        self.preview_phase = TextTriggerPreviewPhase::Pending;
    }

    pub(crate) fn mark_preview_pending(&mut self) -> bool {
        if matches!(self.preview_phase, TextTriggerPreviewPhase::Pending) {
            return false;
        }
        self.preview_phase = TextTriggerPreviewPhase::Pending;
        true
    }

    pub(crate) fn install_preview(
        &mut self,
        revision: u64,
        text: ilium_execution::RetiringArc<String>,
    ) -> bool {
        if revision != self.preview_revision {
            return false;
        }
        self.preview = Some(TextTriggerPreviewPresentation { revision, text });
        self.preview_phase = TextTriggerPreviewPhase::Ready;
        true
    }

    pub(crate) fn preview_unavailable(
        &mut self,
        revision: u64,
        issue: TextTriggerPreviewIssue,
    ) -> bool {
        if revision != self.preview_revision {
            return false;
        }
        let next = TextTriggerPreviewPhase::Unavailable(issue);
        if self.preview_phase == next {
            return false;
        }
        self.preview_phase = next;
        true
    }

    pub(crate) fn preview_unavailable_current(&mut self, issue: TextTriggerPreviewIssue) -> bool {
        self.preview_unavailable(self.preview_revision, issue)
    }

    pub(crate) fn release_preview(&mut self) {
        self.preview = None;
        self.preview_phase = TextTriggerPreviewPhase::Pending;
    }

    pub fn sample_text(&self) -> String {
        self.sample.lines().join("\n")
    }
    /// Parsed delay; a blank buffer is the default and an oversized one clamps.
    pub fn delay_seconds(&self) -> u32 {
        let digits = self.delay.buf.trim();
        if digits.is_empty() {
            return ilium_ipc::DEFAULT_TEXT_TRIGGER_DELAY_SECONDS;
        }
        digits
            .parse::<u64>()
            .map_or(ilium_ipc::MAX_TEXT_TRIGGER_DELAY_SECONDS, |seconds| {
                seconds.min(u64::from(ilium_ipc::MAX_TEXT_TRIGGER_DELAY_SECONDS)) as u32
            })
    }
    pub fn candidate(&self) -> TextTrigger {
        TextTrigger {
            id: self
                .editing_id
                .clone()
                .unwrap_or_else(|| self.draft_id.clone()),
            enabled: self.enabled,
            regexp: self.regexp.buf.clone(),
            message: self.message.buf.clone(),
            target: self.target,
            sample_text: self.sample_text(),
            delay_seconds: self.delay_seconds(),
        }
    }
}

pub struct TextTriggerDialogLayout {
    pub popup: Rect,
    pub regexp: Rect,
    pub message: Rect,
    pub target: Rect,
    pub delay: Rect,
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
        .split(rows[8])[0];
    TextTriggerDialogLayout {
        popup,
        regexp: rows[0],
        message: rows[1],
        target: rows[2],
        delay: rows[3],
        sample: rows[5],
        enabled: rows[6],
        preview: rows[7],
        save,
        hint: rows[9],
    }
}

/// Paints the draft-owned refusal after the ordinary UI, without changing normal dialog layout.
pub(crate) fn draw_save_error(frame: &mut ratatui::Frame<'_>, app: &crate::app::App) {
    let crate::app::Mode::TextTriggerDialog(state) = &app.mode else {
        return;
    };
    let Some(error) = state.save_error.as_deref() else {
        return;
    };
    let area = layout(frame.area()).preview;
    frame.render_widget(ratatui::widgets::Clear, area);
    let text = format!(
        "Draft retained. Retry after fixing storage; Esc/reopen to review a conflicting rule.\n{error}"
    );
    frame.render_widget(
        ratatui::widgets::Paragraph::new(text)
            .block(
                crate::theme::block(true)
                    .title(crate::theme::chrome_title("Text Trigger not saved")),
            )
            .wrap(ratatui::widgets::Wrap { trim: false }),
        area,
    );
}

/// Uses the first target row; the second row stays available to the form layout.
pub fn target_control(
    area: Rect,
    state: &TextTriggerDialogState,
) -> crate::value_control::ValueControl {
    let target = layout(area).target;
    crate::value_control::ValueControl::new(
        target,
        crate::value_control::ControlSpec {
            kind: crate::value_control::ControlKind::Choice,
            label: "Match",
            value: state.target.label(),
            label_width: 8,
            previous_enabled: true,
            next_enabled: true,
            open_enabled: true,
        },
    )
}

/// Prepare the same numeric row for rendering and pointer dispatch.
pub fn delay_control(
    area: Rect,
    state: &TextTriggerDialogState,
) -> crate::value_control::ValueControl {
    let seconds = state.delay_seconds();
    crate::value_control::ValueControl::new(
        layout(area).delay,
        crate::value_control::ControlSpec {
            kind: crate::value_control::ControlKind::Number,
            label: "Delay (seconds)",
            value: &state.delay.buf,
            label_width: 16,
            previous_enabled: seconds > 0,
            next_enabled: seconds < ilium_ipc::MAX_TEXT_TRIGGER_DELAY_SECONDS,
            open_enabled: true,
        },
    )
}

impl TextTriggerDialogState {
    /// Buttons change only the draft; the existing Save action owns persistence.
    pub fn apply_delay_control(
        &mut self,
        action: crate::value_control::ControlAction,
    ) -> Result<(), String> {
        use crate::value_control::ControlAction;
        let seconds = match action {
            ControlAction::Decrement => self.delay_seconds().saturating_sub(1),
            ControlAction::Increment => self
                .delay_seconds()
                .saturating_add(1)
                .min(ilium_ipc::MAX_TEXT_TRIGGER_DELAY_SECONDS),
            ControlAction::EditNumber => {
                self.focus = TextTriggerFocus::Delay;
                return Ok(());
            }
            _ => return Err("Unsupported text trigger delay action".into()),
        };
        self.delay = TextPromptState::new(seconds.to_string());
        self.focus = TextTriggerFocus::Delay;
        Ok(())
    }
}

#[cfg(test)]
mod delay_tests {
    use super::*;

    #[test]
    fn target_selector_paints_and_hits_both_directions_and_catalogue() {
        use crate::value_control::{ControlAction, PointerButton};
        use ratatui::{backend::TestBackend, Terminal};

        let state = TextTriggerDialogState::new(None);
        let area = Rect::new(0, 0, 100, 30);
        let control = target_control(area, &state);
        let geometry = control.geometry();
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| control.render(frame, crate::value_control::ControlStyles::default()))
            .unwrap();

        for (rectangle, glyph) in [
            (geometry.previous, "←"),
            (geometry.open, "+"),
            (geometry.next, "→"),
        ] {
            assert_eq!(
                terminal.backend().buffer()[(rectangle.x, rectangle.y)].symbol(),
                glyph
            );
        }
        let value = ratatui::layout::Position::new(geometry.value.x, geometry.value.y);
        assert_eq!(
            control.hit(value, PointerButton::Left),
            Some(ControlAction::NextChoice)
        );
        assert_eq!(
            control.hit(value, PointerButton::Right),
            Some(ControlAction::PreviousChoice)
        );
        assert_eq!(
            control.hit(
                ratatui::layout::Position::new(geometry.open.x, geometry.open.y),
                PointerButton::Left
            ),
            Some(ControlAction::OpenChoices)
        );
    }

    #[test]
    fn a_new_trigger_starts_at_sixty_seconds() {
        let state = TextTriggerDialogState::new(None);
        assert_eq!(state.delay.buf, "60");
        assert_eq!(state.candidate().delay_seconds, 60);
    }

    #[test]
    fn an_existing_trigger_keeps_its_delay_and_blank_means_default() {
        let existing = TextTrigger {
            id: "r".into(),
            delay_seconds: 0,
            ..TextTrigger::default()
        };
        let mut state = TextTriggerDialogState::new(Some((0, &existing)));
        assert_eq!(state.candidate().delay_seconds, 0);
        state.delay.buf = "125".into();
        assert_eq!(state.candidate().delay_seconds, 125);
        state.delay.buf.clear();
        assert_eq!(state.candidate().delay_seconds, 60);
        state.delay.buf = "99999999999999999999".into();
        assert_eq!(
            state.candidate().delay_seconds,
            ilium_ipc::MAX_TEXT_TRIGGER_DELAY_SECONDS
        );
    }

    #[test]
    fn delay_buttons_preserve_authored_rule_and_stop_at_domain_bounds() {
        use crate::value_control::{ControlAction, PointerButton};
        let mut state = TextTriggerDialogState::new(None);
        state.regexp = TextPromptState::new("authored.*λ");
        state.message = TextPromptState::new("retain this reply");
        state.target = TextTriggerTarget::Terminals;
        state.enabled = false;
        let before = state.candidate();
        state.apply_delay_control(ControlAction::Decrement).unwrap();
        let mut expected = before.clone();
        expected.delay_seconds = 59;
        assert_eq!(state.candidate(), expected);
        state.apply_delay_control(ControlAction::Increment).unwrap();
        assert_eq!(state.candidate(), before);

        state.delay = TextPromptState::new("0");
        let control = delay_control(Rect::new(0, 0, 80, 24), &state);
        assert_eq!(
            control.hit(
                ratatui::layout::Position::new(
                    control.geometry().previous.x,
                    control.geometry().previous.y,
                ),
                PointerButton::Left,
            ),
            None
        );
        state.apply_delay_control(ControlAction::Decrement).unwrap();
        assert_eq!(state.delay.buf, "0");
        state.delay = TextPromptState::new(ilium_ipc::MAX_TEXT_TRIGGER_DELAY_SECONDS.to_string());
        let control = delay_control(Rect::new(0, 0, 80, 24), &state);
        assert_eq!(
            control.hit(
                ratatui::layout::Position::new(
                    control.geometry().next.x,
                    control.geometry().next.y,
                ),
                PointerButton::Left,
            ),
            None
        );
        state.apply_delay_control(ControlAction::Increment).unwrap();
        assert_eq!(
            state.delay_seconds(),
            ilium_ipc::MAX_TEXT_TRIGGER_DELAY_SECONDS
        );
    }

    #[test]
    fn delay_star_retains_exact_draft_and_blank_steps_from_native_default() {
        use crate::value_control::ControlAction;
        let mut state = TextTriggerDialogState::new(None);
        state.delay = TextPromptState::new("00125");
        state.delay.cursor = 2;
        let before = state.candidate();
        state
            .apply_delay_control(ControlAction::EditNumber)
            .unwrap();
        assert_eq!(state.focus, TextTriggerFocus::Delay);
        assert_eq!(state.delay.buf, "00125");
        assert_eq!(state.delay.cursor, 2);
        assert_eq!(state.candidate(), before);
        state.delay.buf.clear();
        state.apply_delay_control(ControlAction::Increment).unwrap();
        assert_eq!(state.delay_seconds(), 61);
        let before = state.candidate();
        assert!(state
            .apply_delay_control(ControlAction::PreviousChoice)
            .is_err());
        assert_eq!(state.candidate(), before);
    }
}
