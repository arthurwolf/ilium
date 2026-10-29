//! The mini toolbar in the bottom-right corner of the last-prompt banner,
//! the per-pane prompt history it navigates, and the state of the full-text
//! dialog its "+" button opens.
//!
//! Layout is positional: every button owns a fixed slot, so a hidden button
//! (an Older button at the oldest entry, a Newer button at the newest) leaves
//! an empty slot and the other buttons never move.

use std::collections::VecDeque;

use ratatui::layout::{Position, Rect};

/// Glyph of the agent-toolbar button that re-opens a closed banner.
pub const REOPEN_GLYPH: &str = "\u{276f}";

/// Prompts kept per pane. Older entries are dropped first.
pub const MAX_HISTORY_ENTRIES: usize = 100;

/// Columns of one button slot: the glyph plus one separating blank.
const SLOT_WIDTH: u16 = 2;
/// Slots, left to right: Older, Newer, Copy, Expand, Close.
const SLOT_COUNT: u16 = 5;
/// Columns the whole toolbar occupies.
pub const TOOLBAR_WIDTH: u16 = SLOT_WIDTH * SLOT_COUNT;
/// Narrowest banner that still gets a toolbar; below this the text keeps the
/// whole width.
const MIN_BANNER_WIDTH: u16 = 40;

/// One click target of the banner toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptToolbarButton {
    /// Shows the previous (older) submitted prompt.
    Older,
    /// Shows the next (newer) submitted prompt.
    Newer,
    /// Copies the displayed prompt to the clipboard.
    Copy,
    /// Opens the dialog with the full, untruncated text.
    Expand,
    /// Hides the banner for this agent only.
    Close,
}

impl PromptToolbarButton {
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Older => "\u{25b2}",
            Self::Newer => "\u{25bc}",
            Self::Copy => "\u{29c9}",
            Self::Expand => "+",
            Self::Close => "\u{2715}",
        }
    }

    pub const fn tooltip(self) -> &'static str {
        match self {
            Self::Older => "Show the previous prompt",
            Self::Newer => "Show the next prompt",
            Self::Copy => "Copy this prompt to the clipboard",
            Self::Expand => "Show the full prompt text",
            Self::Close => "Hide the prompt banner for this agent",
        }
    }

    const fn slot(self) -> u16 {
        match self {
            Self::Older => 0,
            Self::Newer => 1,
            Self::Copy => 2,
            Self::Expand => 3,
            Self::Close => 4,
        }
    }
}

/// Whether the banner of width `banner_width` carries a toolbar.
pub fn has_toolbar(banner_width: u16) -> bool {
    banner_width >= MIN_BANNER_WIDTH
}

/// Columns the banner text may use once the toolbar's columns are taken out.
pub fn text_width_after_toolbar(text_width: u16) -> u16 {
    if has_toolbar(text_width) {
        text_width - TOOLBAR_WIDTH
    } else {
        text_width
    }
}

/// Which navigation buttons currently have somewhere to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolbarVisibility {
    pub older: bool,
    pub newer: bool,
}

/// Rect of `button` in the bottom row of `banner`, or `None` when the banner
/// has no toolbar or the button is hidden.
pub fn button_rect(
    banner: Rect,
    button: PromptToolbarButton,
    visibility: ToolbarVisibility,
) -> Option<Rect> {
    if banner.height == 0 || !has_toolbar(banner.width) {
        return None;
    }
    let hidden = match button {
        PromptToolbarButton::Older => !visibility.older,
        PromptToolbarButton::Newer => !visibility.newer,
        _ => false,
    };
    if hidden {
        return None;
    }
    let first_x = banner.right().saturating_sub(TOOLBAR_WIDTH);
    Some(Rect::new(
        first_x + button.slot() * SLOT_WIDTH,
        banner.bottom() - 1,
        1,
        1,
    ))
}

const ALL_BUTTONS: [PromptToolbarButton; 5] = [
    PromptToolbarButton::Older,
    PromptToolbarButton::Newer,
    PromptToolbarButton::Copy,
    PromptToolbarButton::Expand,
    PromptToolbarButton::Close,
];

/// Every visible button with its rect, left to right.
pub fn visible_buttons(
    banner: Rect,
    visibility: ToolbarVisibility,
) -> Vec<(PromptToolbarButton, Rect)> {
    ALL_BUTTONS
        .into_iter()
        .filter_map(|button| Some((button, button_rect(banner, button, visibility)?)))
        .collect()
}

/// The button under `position`. The slot's trailing blank column is part of
/// the hit target so the one-cell glyph is easy to hit.
pub fn button_at(
    banner: Rect,
    visibility: ToolbarVisibility,
    position: Position,
) -> Option<PromptToolbarButton> {
    visible_buttons(banner, visibility)
        .into_iter()
        .find(|(_, rect)| {
            position.y == rect.y && position.x >= rect.x && position.x < rect.x + SLOT_WIDTH
        })
        .map(|(button, _)| button)
}

/// Submitted prompts of one pane, oldest first, plus which one the banner
/// shows. `cursor == None` follows the newest prompt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptHistory {
    entries: VecDeque<String>,
    cursor: Option<usize>,
}

impl PromptHistory {
    /// Records a newly submitted prompt and jumps back to it. Empty text and
    /// an immediate repeat of the newest entry are ignored (the latter only
    /// re-follows the newest entry).
    pub fn record(&mut self, prompt: &str) {
        self.cursor = None;
        if prompt.is_empty() || self.entries.back().is_some_and(|last| last == prompt) {
            return;
        }
        if self.entries.len() == MAX_HISTORY_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(prompt.to_string());
    }

    fn displayed_index(&self) -> Option<usize> {
        self.cursor.or_else(|| self.entries.len().checked_sub(1))
    }

    /// The prompt the banner shows.
    pub fn displayed(&self) -> Option<&str> {
        self.entries
            .get(self.displayed_index()?)
            .map(String::as_str)
    }

    pub fn visibility(&self) -> ToolbarVisibility {
        let index = self.displayed_index().unwrap_or(0);
        ToolbarVisibility {
            older: index > 0,
            newer: index + 1 < self.entries.len(),
        }
    }

    /// Moves to the previous entry; false at the oldest one.
    pub fn show_older(&mut self) -> bool {
        match self.displayed_index() {
            Some(index) if index > 0 => {
                self.cursor = Some(index - 1);
                true
            }
            _ => false,
        }
    }

    /// Moves to the next entry; false at the newest one.
    pub fn show_newer(&mut self) -> bool {
        match self.displayed_index() {
            Some(index) if index + 1 < self.entries.len() => {
                self.cursor = (index + 2 < self.entries.len()).then_some(index + 1);
                true
            }
            _ => false,
        }
    }

    /// Merges the prompts read from the agent's transcript (oldest first)
    /// ahead of the live entries. Live entries that the transcript already
    /// ends with are not repeated; newer live ones stay after it. The shown
    /// prompt stays the same one.
    pub fn backfill(&mut self, transcript_prompts: &[String]) {
        let shown = self
            .cursor
            .and_then(|index| self.entries.get(index).cloned());
        let transcript: Vec<&String> = transcript_prompts
            .iter()
            .filter(|prompt| !prompt.is_empty())
            .collect();
        let mut merged: VecDeque<String> = VecDeque::new();
        for prompt in &transcript {
            if merged.back() != Some(*prompt) {
                merged.push_back((*prompt).clone());
            }
        }
        // Live entries before the first one found in the transcript were
        // submitted outside it (rare); keep them after the transcript.
        let overlap_end = self
            .entries
            .iter()
            .rposition(|entry| merged.contains(entry))
            .map_or(0, |index| index + 1);
        for entry in self.entries.iter().skip(overlap_end) {
            if merged.back() != Some(entry) {
                merged.push_back(entry.clone());
            }
        }
        while merged.len() > MAX_HISTORY_ENTRIES {
            merged.pop_front();
        }
        self.entries = merged;
        self.cursor = shown.and_then(|text| self.entries.iter().rposition(|entry| *entry == text));
    }

    /// The newest recorded prompt.
    pub fn newest(&self) -> Option<&str> {
        self.entries.back().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// State of the full-text dialog: which pane it describes and how far its
/// text is scrolled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastPromptDialogState {
    pub pane_id: ilium_core::NodeId,
    pub text: String,
    pub scroll: u16,
}

impl LastPromptDialogState {
    pub fn new(pane_id: ilium_core::NodeId, text: String) -> Self {
        Self {
            pane_id,
            text,
            scroll: 0,
        }
    }
}

/// Dialog rect, centred in `screen` with a margin.
pub fn dialog_area(screen: Rect) -> Rect {
    let width = screen
        .width
        .saturating_sub(8)
        .clamp(1, 100)
        .min(screen.width);
    let height = screen
        .height
        .saturating_sub(4)
        .clamp(1, 30)
        .min(screen.height);
    Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    )
}

const COPY_LABEL: &str = "[ Copy ]";
const CLOSE_LABEL: &str = "[ Close ]";

/// Footer buttons on the dialog's bottom border row: Copy then Close.
pub fn dialog_copy_button(dialog: Rect) -> Rect {
    let y = dialog.bottom().saturating_sub(1);
    Rect::new(dialog.x + 2, y, COPY_LABEL.len() as u16, 1)
}

pub fn dialog_close_button(dialog: Rect) -> Rect {
    let y = dialog.bottom().saturating_sub(1);
    Rect::new(
        dialog.x + 2 + COPY_LABEL.len() as u16 + 2,
        y,
        CLOSE_LABEL.len() as u16,
        1,
    )
}

pub fn dialog_copy_label() -> &'static str {
    COPY_LABEL
}

pub fn dialog_close_label() -> &'static str {
    CLOSE_LABEL
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(entries: &[&str]) -> PromptHistory {
        let mut history = PromptHistory::default();
        for entry in entries {
            history.record(entry);
        }
        history
    }

    #[test]
    fn newest_prompt_is_shown_and_only_older_is_available() {
        let history = history(&["a", "b", "c"]);
        assert_eq!(history.displayed(), Some("c"));
        let visibility = history.visibility();
        assert!(visibility.older && !visibility.newer);
    }

    #[test]
    fn navigation_hides_buttons_at_both_ends() {
        let mut history = history(&["a", "b", "c"]);
        assert!(history.show_older());
        assert_eq!(history.displayed(), Some("b"));
        assert!(history.visibility().older && history.visibility().newer);
        assert!(history.show_older());
        assert_eq!(history.displayed(), Some("a"));
        assert!(!history.visibility().older && history.visibility().newer);
        assert!(!history.show_older());
        assert!(history.show_newer() && history.show_newer());
        assert_eq!(history.displayed(), Some("c"));
        assert!(!history.visibility().newer);
        assert!(!history.show_newer());
    }

    #[test]
    fn backfill_puts_transcript_prompts_before_live_ones_without_repeating() {
        let mut history = history(&["c"]);
        history.backfill(&["a".to_string(), "b".to_string(), "c".to_string()]);
        assert_eq!(history.len(), 3);
        assert_eq!(history.displayed(), Some("c"));
        assert!(history.visibility().older);
        assert!(history.show_older());
        assert_eq!(history.displayed(), Some("b"));
    }

    #[test]
    fn backfill_keeps_live_prompts_newer_than_the_transcript_and_the_shown_one() {
        let mut history = history(&["b", "z"]);
        history.show_older();
        history.backfill(&["a".to_string(), "b".to_string()]);
        assert_eq!(history.len(), 3);
        assert_eq!(history.displayed(), Some("b"));
        assert!(history.show_newer());
        assert_eq!(history.displayed(), Some("z"));
    }

    #[test]
    fn recording_a_prompt_returns_to_the_newest_and_dedups() {
        let mut history = history(&["a", "b"]);
        history.show_older();
        history.record("c");
        assert_eq!(history.displayed(), Some("c"));
        history.record("c");
        assert_eq!(history.len(), 3);
        history.record("");
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn history_is_bounded() {
        let mut history = PromptHistory::default();
        for index in 0..MAX_HISTORY_ENTRIES + 5 {
            history.record(&format!("prompt {index}"));
        }
        assert_eq!(history.len(), MAX_HISTORY_ENTRIES);
        assert_eq!(
            history.displayed(),
            Some(format!("prompt {}", MAX_HISTORY_ENTRIES + 4).as_str())
        );
    }

    #[test]
    fn hidden_navigation_buttons_leave_the_other_buttons_in_place() {
        let banner = Rect::new(10, 5, 60, 3);
        let all = ToolbarVisibility {
            older: true,
            newer: true,
        };
        let none = ToolbarVisibility {
            older: false,
            newer: false,
        };
        for button in [
            PromptToolbarButton::Copy,
            PromptToolbarButton::Expand,
            PromptToolbarButton::Close,
        ] {
            assert_eq!(
                button_rect(banner, button, all),
                button_rect(banner, button, none)
            );
        }
        assert!(button_rect(banner, PromptToolbarButton::Older, none).is_none());
        assert!(button_rect(banner, PromptToolbarButton::Newer, none).is_none());
        assert_eq!(visible_buttons(banner, none).len(), 3);
        assert_eq!(visible_buttons(banner, all).len(), 5);
    }

    #[test]
    fn buttons_sit_on_the_bottom_row_at_the_right_edge() {
        let banner = Rect::new(0, 0, 60, 4);
        let visibility = ToolbarVisibility {
            older: true,
            newer: true,
        };
        let close = button_rect(banner, PromptToolbarButton::Close, visibility).unwrap();
        assert_eq!(close.y, 3);
        assert!(close.right() <= banner.right());
        let older = button_rect(banner, PromptToolbarButton::Older, visibility).unwrap();
        assert_eq!(older.x, banner.right() - TOOLBAR_WIDTH);
    }

    #[test]
    fn narrow_banners_have_no_toolbar() {
        let banner = Rect::new(0, 0, MIN_BANNER_WIDTH - 1, 2);
        let visibility = ToolbarVisibility {
            older: true,
            newer: true,
        };
        assert!(visible_buttons(banner, visibility).is_empty());
        assert_eq!(text_width_after_toolbar(30), 30);
        assert_eq!(text_width_after_toolbar(60), 60 - TOOLBAR_WIDTH);
    }

    #[test]
    fn button_at_hits_the_slot_and_ignores_hidden_ones() {
        let banner = Rect::new(0, 0, 60, 2);
        let none = ToolbarVisibility {
            older: false,
            newer: false,
        };
        let copy = button_rect(banner, PromptToolbarButton::Copy, none).unwrap();
        assert_eq!(
            button_at(banner, none, Position::new(copy.x, copy.y)),
            Some(PromptToolbarButton::Copy)
        );
        let older_x = banner.right() - TOOLBAR_WIDTH;
        assert_eq!(button_at(banner, none, Position::new(older_x, 1)), None);
        assert_eq!(button_at(banner, none, Position::new(copy.x, 0)), None);
    }

    #[test]
    fn dialog_area_fits_small_screens() {
        let area = dialog_area(Rect::new(0, 0, 10, 3));
        assert!(area.width <= 10 && area.height <= 3);
    }
}

#[cfg(test)]
mod app_flow_tests {
    use super::*;
    use crate::app::App;
    use ilium_core::{AgentActivity, AgentClass, NodeId, PaneContentKind, PaneStatus, ROOT_ID};

    fn app_with_agent() -> (App, NodeId) {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Idle, None),
            )
            .unwrap();
        (app, pane_id)
    }

    #[test]
    fn banner_is_enabled_by_default() {
        let (app, _) = app_with_agent();
        assert!(app.ui_settings.last_prompt_enabled);
    }

    #[test]
    fn history_navigation_changes_the_displayed_prompt_and_hides_ends() {
        let (mut app, pane) = app_with_agent();
        app.record_last_prompt(pane, Some("first"));
        app.record_last_prompt(pane, Some("second"));
        assert_eq!(app.display_last_agent_prompt(pane), Some("second"));
        let visibility = app.last_prompt_toolbar_visibility(pane);
        assert!(visibility.older && !visibility.newer);

        app.execute_last_prompt_button(pane, PromptToolbarButton::Older);
        assert_eq!(app.display_last_agent_prompt(pane), Some("first"));
        let visibility = app.last_prompt_toolbar_visibility(pane);
        assert!(!visibility.older && visibility.newer);

        app.execute_last_prompt_button(pane, PromptToolbarButton::Newer);
        assert_eq!(app.display_last_agent_prompt(pane), Some("second"));
    }

    #[test]
    fn close_hides_only_that_agent_and_offers_reopen() {
        let (mut app, pane) = app_with_agent();
        let other = app
            .tree
            .add_pane(
                app.tree.parent_of(pane).unwrap(),
                "codex",
                PaneContentKind::Terminal,
            )
            .unwrap();
        app.tree
            .set_pane_status(
                other,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Idle, None),
            )
            .unwrap();
        app.record_last_prompt(pane, Some("hello"));
        app.record_last_prompt(other, Some("world"));
        assert!(app.shows_last_prompt_banner(pane));
        assert!(!app.last_prompt_reopen_available(pane));

        app.execute_last_prompt_button(pane, PromptToolbarButton::Close);
        assert!(!app.shows_last_prompt_banner(pane));
        assert!(app.shows_last_prompt_banner(other));
        assert!(app.last_prompt_reopen_available(pane));
        assert!(!app.last_prompt_reopen_available(other));

        app.reopen_last_prompt_banner(pane);
        assert!(app.shows_last_prompt_banner(pane));
        assert!(!app.last_prompt_reopen_available(pane));
    }

    #[test]
    fn reopen_is_not_offered_when_the_setting_is_off() {
        let (mut app, pane) = app_with_agent();
        app.record_last_prompt(pane, Some("hello"));
        app.close_last_prompt_banner(pane);
        app.ui_settings.last_prompt_enabled = false;
        assert!(!app.last_prompt_reopen_available(pane));
        assert!(!app.shows_last_prompt_banner(pane));
    }

    #[test]
    fn expand_opens_the_full_text_dialog() {
        let (mut app, pane) = app_with_agent();
        app.record_last_prompt(pane, Some("the full text"));
        app.execute_last_prompt_button(pane, PromptToolbarButton::Expand);
        match &app.mode {
            crate::app::Mode::LastPromptDialog(state) => {
                assert_eq!(state.pane_id, pane);
                assert_eq!(state.text, "the full text");
            }
            _ => panic!("expected the last prompt dialog"),
        }
    }
}
