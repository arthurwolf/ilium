//! Startup offer for installing Ilium's optional agent guidance.
//!
//! This module is intentionally presentation-only.  It does not inspect or
//! change instruction files, configuration, or projects: its caller supplies
//! the current setup facts and acts on the returned [`SetupPromptOutcome`].

use std::path::PathBuf;

use crossterm::event::KeyCode;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::modal;
use crate::theme;

/// The instruction-file scope the offer applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupPromptScope {
    /// The user's shared instruction file.
    Global {
        chatroom_file: PathBuf,
        progress_file: PathBuf,
    },
    /// One opened Ilium project.  The path is displayed so the offer cannot
    /// silently imply that a similarly named project is being changed.
    Project(PathBuf),
}

impl SetupPromptScope {
    /// A concise, human-readable description of the exact target.
    pub fn target_label(&self) -> String {
        match self {
            Self::Global {
                chatroom_file,
                progress_file,
            } if chatroom_file == progress_file => {
                format!("Global instructions ({})", chatroom_file.display())
            }
            Self::Global {
                chatroom_file,
                progress_file,
            } => format!(
                "Global Chatroom: {}\nGlobal Progress: {}",
                chatroom_file.display(),
                progress_file.display()
            ),
            Self::Project(path) => format!("Project instructions ({})", path.display()),
        }
    }
}

/// The control currently selected in a setup offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupPromptFocus {
    Chatroom,
    Progress,
    Apply,
    NotNow,
    NeverAsk,
}

impl SetupPromptFocus {
    const ALL: [Self; 5] = [
        Self::Chatroom,
        Self::Progress,
        Self::Apply,
        Self::NotNow,
        Self::NeverAsk,
    ];
}

/// A user decision produced by [`SetupPromptState::handle_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupPromptOutcome {
    /// The prompt remains open after a navigation or selection change.
    Continue,
    /// Install the independently selected feature blocks.
    Apply { chatroom: bool, progress: bool },
    /// Close this instance but allow a future offer.
    NotNow,
    /// Close this instance and persist an opt-out at its scope.
    NeverAsk,
}

/// Pure interaction state for a single setup offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupPromptState {
    pub scope: SetupPromptScope,
    /// `true` only when the Chatroom instruction block is absent at `scope`.
    pub chatroom_needs_setup: bool,
    /// `true` only when the Progress instruction block is absent at `scope`.
    pub progress_needs_setup: bool,
    pub chatroom_selected: bool,
    pub progress_selected: bool,
    pub focus: SetupPromptFocus,
}

impl SetupPromptState {
    /// Starts with every missing feature selected. Existing feature blocks
    /// remain visible but cannot be selected or changed by this dialog.
    pub fn new(
        scope: SetupPromptScope,
        chatroom_needs_setup: bool,
        progress_needs_setup: bool,
    ) -> Self {
        let mut state = Self {
            scope,
            chatroom_needs_setup,
            progress_needs_setup,
            chatroom_selected: chatroom_needs_setup,
            progress_selected: progress_needs_setup,
            focus: SetupPromptFocus::Chatroom,
        };
        state.focus = state.first_available_focus();
        state
    }

    /// Returns whether at least one new instruction block may be installed.
    pub const fn has_available_feature(&self) -> bool {
        self.chatroom_needs_setup || self.progress_needs_setup
    }

    /// Toggles one installable feature. Existing setup is deliberately inert.
    pub fn toggle(&mut self, focus: SetupPromptFocus) {
        match focus {
            SetupPromptFocus::Chatroom if self.chatroom_needs_setup => {
                self.chatroom_selected = !self.chatroom_selected;
            }
            SetupPromptFocus::Progress if self.progress_needs_setup => {
                self.progress_selected = !self.progress_selected;
            }
            _ => {}
        }
    }

    /// Moves through actionable controls, skipping feature rows that are
    /// already configured because they must not be toggled.
    pub fn move_focus(&mut self, direction: i8) {
        let index = SetupPromptFocus::ALL
            .iter()
            .position(|candidate| *candidate == self.focus)
            .unwrap_or(0);
        for offset in 1..=SetupPromptFocus::ALL.len() {
            let next = if direction < 0 {
                (index + SetupPromptFocus::ALL.len() - offset) % SetupPromptFocus::ALL.len()
            } else {
                (index + offset) % SetupPromptFocus::ALL.len()
            };
            let candidate = SetupPromptFocus::ALL[next];
            if self.is_focusable(candidate) {
                self.focus = candidate;
                return;
            }
        }
    }

    /// Handles the small, terminal-native keyboard contract for the dialog.
    pub fn handle_key(&mut self, key: KeyCode) -> SetupPromptOutcome {
        match key {
            KeyCode::Up | KeyCode::Left | KeyCode::BackTab => {
                self.move_focus(-1);
                SetupPromptOutcome::Continue
            }
            KeyCode::Down | KeyCode::Right | KeyCode::Tab => {
                self.move_focus(1);
                SetupPromptOutcome::Continue
            }
            KeyCode::Char(' ') => {
                self.toggle(self.focus);
                SetupPromptOutcome::Continue
            }
            KeyCode::Enter => self.activate_focus(),
            KeyCode::Esc | KeyCode::Char('n' | 'N') => SetupPromptOutcome::NotNow,
            KeyCode::Char('x' | 'X') => SetupPromptOutcome::NeverAsk,
            _ => SetupPromptOutcome::Continue,
        }
    }

    /// Applies the action represented by a mouse click on `focus`.
    pub fn activate(&mut self, focus: SetupPromptFocus) -> SetupPromptOutcome {
        if !self.is_focusable(focus) {
            return SetupPromptOutcome::Continue;
        }
        self.focus = focus;
        self.activate_focus()
    }

    fn activate_focus(&mut self) -> SetupPromptOutcome {
        match self.focus {
            SetupPromptFocus::Chatroom | SetupPromptFocus::Progress => {
                self.toggle(self.focus);
                SetupPromptOutcome::Continue
            }
            SetupPromptFocus::Apply => SetupPromptOutcome::Apply {
                chatroom: self.chatroom_needs_setup && self.chatroom_selected,
                progress: self.progress_needs_setup && self.progress_selected,
            },
            SetupPromptFocus::NotNow => SetupPromptOutcome::NotNow,
            SetupPromptFocus::NeverAsk => SetupPromptOutcome::NeverAsk,
        }
    }

    fn first_available_focus(&self) -> SetupPromptFocus {
        SetupPromptFocus::ALL
            .into_iter()
            .find(|focus| self.is_focusable(*focus))
            .unwrap_or(SetupPromptFocus::Apply)
    }

    fn is_focusable(&self, focus: SetupPromptFocus) -> bool {
        match focus {
            SetupPromptFocus::Chatroom => self.chatroom_needs_setup,
            SetupPromptFocus::Progress => self.progress_needs_setup,
            SetupPromptFocus::Apply | SetupPromptFocus::NotNow | SetupPromptFocus::NeverAsk => true,
        }
    }
}

/// Geometry shared by rendering and [`hit_test`] so controls cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupPromptLayout {
    pub popup: Rect,
    pub target: Rect,
    pub chatroom: Rect,
    pub progress: Rect,
    pub apply: Rect,
    pub not_now: Rect,
    pub never_ask: Rect,
    pub hint: Rect,
}

/// Computes the clamped, centered startup-offer geometry.
pub fn layout(screen_area: Rect) -> SetupPromptLayout {
    let popup = modal::centered_fixed_rect(76, 16, screen_area);
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
    SetupPromptLayout {
        popup,
        target: rows[0],
        chatroom: rows[1],
        progress: rows[2],
        apply: rows[3],
        not_now: rows[4],
        never_ask: rows[5],
        hint: rows[6],
    }
}

/// Maps a mouse position to the control it visibly lands on.
pub fn hit_test(screen_area: Rect, position: Position) -> Option<SetupPromptFocus> {
    let layout = layout(screen_area);
    [
        (layout.chatroom, SetupPromptFocus::Chatroom),
        (layout.progress, SetupPromptFocus::Progress),
        (layout.apply, SetupPromptFocus::Apply),
        (layout.not_now, SetupPromptFocus::NotNow),
        (layout.never_ask, SetupPromptFocus::NeverAsk),
    ]
    .into_iter()
    .find_map(|(area, focus)| area.contains(position).then_some(focus))
}

/// Renders the setup offer over the current workspace.
pub fn render(frame: &mut Frame, screen_area: Rect, state: &SetupPromptState) {
    let layout = layout(screen_area);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("Set up agent facilities")),
        layout.popup,
    );
    frame.render_widget(
        Paragraph::new(match &state.scope {
            SetupPromptScope::Project(_) => format!(
                "{}\nChoose which optional instructions to install. Chatroom setup also creates or repairs CHATROOM.md and agent hooks.",
                state.scope.target_label()
            ),
            SetupPromptScope::Global { .. } => format!(
                "{}\nChoose which optional instructions to install.",
                state.scope.target_label()
            ),
        })
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center),
        layout.target,
    );
    render_feature_row(
        frame,
        layout.chatroom,
        "Chatroom coordination",
        state.chatroom_needs_setup,
        state.chatroom_selected,
        state.focus == SetupPromptFocus::Chatroom,
    );
    render_feature_row(
        frame,
        layout.progress,
        "Progress monitor",
        state.progress_needs_setup,
        state.progress_selected,
        state.focus == SetupPromptFocus::Progress,
    );
    render_action_row(
        frame,
        layout.apply,
        "Enter",
        "Apply selected setup",
        state.focus == SetupPromptFocus::Apply,
    );
    render_action_row(
        frame,
        layout.not_now,
        "N / Esc",
        "Not now",
        state.focus == SetupPromptFocus::NotNow,
    );
    render_action_row(
        frame,
        layout.never_ask,
        "X",
        "Never ask again",
        state.focus == SetupPromptFocus::NeverAsk,
    );
    frame.render_widget(
        Paragraph::new("Arrow keys move · Space toggles a feature · Enter activates")
            .style(Style::new().add_modifier(Modifier::DIM))
            .alignment(Alignment::Center),
        layout.hint,
    );
}

fn render_feature_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    needs_setup: bool,
    selected: bool,
    focused: bool,
) {
    let (prefix, suffix) = if needs_setup {
        (
            if selected { "[x]" } else { "[ ]" },
            if selected {
                "will be installed"
            } else {
                "will not be installed"
            },
        )
    } else {
        ("[✓]", "already set up")
    };
    let style = if focused {
        theme::selected_style().add_modifier(Modifier::BOLD)
    } else if !needs_setup {
        Style::new().add_modifier(Modifier::DIM)
    } else {
        Style::new()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {prefix} {label}"), style),
            Span::styled(format!(" — {suffix}"), style.add_modifier(Modifier::DIM)),
        ])),
        area,
    );
}

fn render_action_row(frame: &mut Frame, area: Rect, key: &str, label: &str, focused: bool) {
    let style = if focused {
        theme::selected_style().add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {key} "), style.add_modifier(Modifier::UNDERLINED)),
            Span::styled(label, style),
        ])),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn global_scope() -> SetupPromptScope {
        SetupPromptScope::Global {
            chatroom_file: PathBuf::from("/tmp/chatroom-CLAUDE.md"),
            progress_file: PathBuf::from("/tmp/progress-CLAUDE.md"),
        }
    }

    #[test]
    fn missing_features_start_selected_and_existing_features_cannot_toggle() {
        let mut state = SetupPromptState::new(global_scope(), false, true);
        assert_eq!(state.focus, SetupPromptFocus::Progress);
        assert!(!state.chatroom_selected);
        assert!(state.progress_selected);

        state.toggle(SetupPromptFocus::Chatroom);
        assert!(!state.chatroom_selected);
        state.toggle(SetupPromptFocus::Progress);
        assert!(!state.progress_selected);
    }

    #[test]
    fn focus_skips_already_configured_feature_rows() {
        let mut state = SetupPromptState::new(global_scope(), false, true);
        state.move_focus(-1);
        assert_eq!(state.focus, SetupPromptFocus::NeverAsk);
        state.move_focus(1);
        assert_eq!(state.focus, SetupPromptFocus::Progress);
    }

    #[test]
    fn unavailable_mouse_feature_row_is_inert() {
        let mut state = SetupPromptState::new(global_scope(), false, true);
        assert_eq!(
            state.activate(SetupPromptFocus::Chatroom),
            SetupPromptOutcome::Continue
        );
        assert_eq!(state.focus, SetupPromptFocus::Progress);
        assert!(!state.chatroom_selected);
    }

    #[test]
    fn apply_reports_only_selected_missing_features() {
        let mut state = SetupPromptState::new(global_scope(), true, true);
        state.toggle(SetupPromptFocus::Progress);
        state.focus = SetupPromptFocus::Apply;
        assert_eq!(
            state.handle_key(KeyCode::Enter),
            SetupPromptOutcome::Apply {
                chatroom: true,
                progress: false,
            }
        );
    }

    #[test]
    fn keyboard_shortcuts_return_non_mutating_decisions() {
        let mut state = SetupPromptState::new(global_scope(), true, false);
        assert_eq!(
            state.handle_key(KeyCode::Char('n')),
            SetupPromptOutcome::NotNow
        );
        assert_eq!(
            state.handle_key(KeyCode::Char('x')),
            SetupPromptOutcome::NeverAsk
        );
    }

    #[test]
    fn hit_test_tracks_each_visible_control_row() {
        let screen = Rect::new(0, 0, 100, 30);
        let dialog = layout(screen);
        assert_eq!(
            hit_test(screen, Position::new(dialog.chatroom.x, dialog.chatroom.y)),
            Some(SetupPromptFocus::Chatroom)
        );
        assert_eq!(
            hit_test(
                screen,
                Position::new(dialog.never_ask.x, dialog.never_ask.y)
            ),
            Some(SetupPromptFocus::NeverAsk)
        );
        assert_eq!(hit_test(screen, Position::new(0, 0)), None);
    }
}
