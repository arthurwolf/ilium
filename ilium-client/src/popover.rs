//! Footer agent launcher popover. Geometry is shared by rendering and pointer
//! hit testing, so a visible choice is always clickable at the same cells.

use std::time::{Duration, Instant};

use ilium_core::{AgentProvider, BuiltinAgentProvider};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use crate::theme::{self, ColorScheme};

pub const HOVER_DELAY: Duration = Duration::from_millis(200);
pub const LEAVE_DELAY: Duration = Duration::from_millis(250);

const WIDE_WIDTH: u16 = 42;
const WIDE_HEIGHT: u16 = 4;
const COMPACT_MIN_WIDTH: u16 = 18;
const COMPACT_WIDTH: u16 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopoverChoice {
    NewWorktree,
    ExistingWorktree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopoverHit {
    Anchor,
    Choice(PopoverChoice),
    Surface,
}

/// A pending hover, visible preview, or pinned menu for one footer button.
/// The parent owns switching buttons and opening the selected creation dialog.
#[derive(Debug, Clone)]
pub struct AgentPopover {
    pub provider: BuiltinAgentProvider,
    pub anchor: Rect,
    pub pinned: bool,
    pub hovered: Option<PopoverHit>,
    pub enabled: bool,
    pub unavailable_reason: Option<String>,
    entered_at: Instant,
    left_at: Option<Instant>,
}

impl AgentPopover {
    pub fn hover(
        provider: BuiltinAgentProvider,
        anchor: Rect,
        now: Instant,
        enabled: bool,
        unavailable_reason: Option<String>,
    ) -> Self {
        Self {
            provider,
            anchor,
            pinned: false,
            hovered: Some(PopoverHit::Anchor),
            enabled,
            unavailable_reason,
            entered_at: now,
            left_at: None,
        }
    }

    pub fn pinned(
        provider: BuiltinAgentProvider,
        anchor: Rect,
        now: Instant,
        enabled: bool,
        unavailable_reason: Option<String>,
    ) -> Self {
        let mut popover = Self::hover(provider, anchor, now, enabled, unavailable_reason);
        popover.pinned = true;
        popover
    }

    /// A preview that left before its delay elapsed never flashes open.
    pub fn is_visible(&self, now: Instant) -> bool {
        self.pinned
            || (now.duration_since(self.entered_at) >= HOVER_DELAY
                && self
                    .left_at
                    .is_none_or(|left_at| left_at.duration_since(self.entered_at) >= HOVER_DELAY))
    }

    /// Records pointer dwell over the button or its popup. Adjacent popup
    /// and anchor rectangles leave no dead cell to cross between them.
    pub fn pointer_moved(&mut self, position: Position, bounds: Rect, now: Instant) {
        let hit = if self.anchor.contains(position) {
            Some(PopoverHit::Anchor)
        } else if self.is_visible(now) {
            layout(bounds, self).and_then(|geometry| geometry.hit(position))
        } else {
            None
        };
        self.hovered = hit;
        if hit.is_some() {
            if self
                .left_at
                .is_some_and(|left_at| left_at.duration_since(self.entered_at) < HOVER_DELAY)
            {
                self.entered_at = now;
            }
            self.left_at = None;
        } else if self.left_at.is_none() {
            self.left_at = Some(now);
        }
    }

    pub fn should_close(&self, now: Instant) -> bool {
        !self.pinned
            && self
                .left_at
                .is_some_and(|left_at| now.duration_since(left_at) >= LEAVE_DELAY)
    }

    /// Right-click or a keyboard action can pin a pending hover immediately.
    pub fn pin(&mut self) {
        self.pinned = true;
        self.left_at = None;
    }

    pub fn unavailable_reason(&self, _choice: PopoverChoice) -> Option<&str> {
        if self.enabled {
            None
        } else {
            Some(
                self.unavailable_reason
                    .as_deref()
                    .unwrap_or("Worktrees are unavailable"),
            )
        }
    }
}

/// All drawn and interactive cells, derived once from the panel and button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopoverLayout {
    pub area: Rect,
    pub anchor: Rect,
    pub header: Rect,
    pub choices: [(PopoverChoice, Rect); 2],
    pub compact: bool,
}

impl PopoverLayout {
    pub fn hit(&self, position: Position) -> Option<PopoverHit> {
        if self.anchor.contains(position) {
            return Some(PopoverHit::Anchor);
        }
        if !self.area.contains(position) {
            return None;
        }
        self.choices
            .iter()
            .find_map(|(choice, rect)| {
                rect.contains(position)
                    .then_some(PopoverHit::Choice(*choice))
            })
            .or(Some(PopoverHit::Surface))
    }
}

/// Places the popup immediately above the footer icon, within the tree panel.
/// The one-row version fits Ilium's usual narrow sidebar without changing
/// toolbar geometry. Very small panels cannot fit two legible choices.
pub fn layout(bounds: Rect, popover: &AgentPopover) -> Option<PopoverLayout> {
    let available_width = bounds.width.saturating_sub(2);
    let available_height = popover.anchor.y.saturating_sub(bounds.y.saturating_add(1));
    if available_width < COMPACT_MIN_WIDTH || available_height == 0 {
        return None;
    }

    let compact = available_width < WIDE_WIDTH || available_height < WIDE_HEIGHT;
    let width = if compact {
        available_width.min(COMPACT_WIDTH)
    } else {
        WIDE_WIDTH
    };
    let height = if compact { 1 } else { WIDE_HEIGHT };
    let min_x = bounds.x.saturating_add(1);
    let max_x = bounds.right().saturating_sub(1).saturating_sub(width);
    let x = popover.anchor.x.clamp(min_x, max_x);
    let y = popover.anchor.y.saturating_sub(height);
    let area = Rect::new(x, y, width, height);
    let (header, choices) = if compact {
        (
            Rect::default(),
            [
                (PopoverChoice::NewWorktree, Rect::new(x + 1, y, 6, 1)),
                (PopoverChoice::ExistingWorktree, Rect::new(x + 7, y, 11, 1)),
            ],
        )
    } else {
        (
            Rect::new(x + 2, y + 1, width - 4, 1),
            [
                (PopoverChoice::NewWorktree, Rect::new(x + 2, y + 2, 12, 1)),
                (
                    PopoverChoice::ExistingWorktree,
                    Rect::new(x + 15, y + 2, 13, 1),
                ),
            ],
        )
    };
    Some(PopoverLayout {
        area,
        anchor: popover.anchor,
        header,
        choices,
        compact,
    })
}

/// Render using the exact [`PopoverLayout`] used by click hit testing.
pub fn render(
    frame: &mut Frame,
    geometry: &PopoverLayout,
    popover: &AgentPopover,
    scheme: ColorScheme,
) {
    frame.render_widget(Clear, geometry.area);
    let background = theme::muted_accent_bg(scheme);
    frame.render_widget(
        Paragraph::new("").style(Style::new().bg(background)),
        geometry.area,
    );
    if !geometry.compact {
        frame.render_widget(theme::block(false), geometry.area);
        frame.render_widget(
            Paragraph::new(format!("Click icon: new {} here", popover.provider.label()))
                .style(Style::new().fg(label_color(scheme))),
            geometry.header,
        );
    }
    for (choice, area) in geometry.choices {
        let label = match (geometry.compact, choice) {
            (true, PopoverChoice::NewWorktree) => "[New…]",
            (true, PopoverChoice::ExistingWorktree) => "[Existing…]",
            (false, PopoverChoice::NewWorktree) => "[Worktree…]",
            (false, PopoverChoice::ExistingWorktree) => "[Existing…]",
        };
        let selected = popover.hovered == Some(PopoverHit::Choice(choice));
        let style = if !popover.enabled {
            Style::new().fg(disabled_color(scheme))
        } else if selected {
            Style::new()
                .fg(theme::accent_fg())
                .bg(theme::accent_bg())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(label_color(scheme))
        };
        frame.render_widget(Paragraph::new(label).style(style), area);
    }
}

fn label_color(scheme: ColorScheme) -> Color {
    match scheme {
        ColorScheme::Dark => Color::Rgb(0xd0, 0xd4, 0xe4),
        ColorScheme::Light => Color::Rgb(0x2d, 0x35, 0x49),
    }
}

fn disabled_color(scheme: ColorScheme) -> Color {
    match scheme {
        ColorScheme::Dark => Color::Rgb(0x7b, 0x80, 0x94),
        ColorScheme::Light => Color::Rgb(0x75, 0x7b, 0x88),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn popover(now: Instant) -> AgentPopover {
        AgentPopover::hover(
            BuiltinAgentProvider::Claude,
            Rect::new(14, 22, 3, 2),
            now,
            true,
            None,
        )
    }

    #[test]
    fn hover_delay_and_leave_grace_are_independent() {
        let start = Instant::now();
        let mut state = popover(start);
        assert!(!state.is_visible(start + HOVER_DELAY - Duration::from_millis(1)));
        assert!(state.is_visible(start + HOVER_DELAY));
        state.pointer_moved(
            Position::new(80, 10),
            Rect::new(0, 0, 80, 24),
            start + HOVER_DELAY,
        );
        assert!(!state.should_close(start + HOVER_DELAY + LEAVE_DELAY - Duration::from_millis(1)));
        assert!(state.should_close(start + HOVER_DELAY + LEAVE_DELAY));
    }

    #[test]
    fn leaving_before_hover_delay_never_opens_preview() {
        let start = Instant::now();
        let mut state = popover(start);
        state.pointer_moved(
            Position::new(80, 10),
            Rect::new(0, 0, 80, 24),
            start + Duration::from_millis(50),
        );
        assert!(!state.is_visible(start + HOVER_DELAY));
        assert!(state.should_close(start + Duration::from_millis(300)));
    }

    #[test]
    fn returning_after_an_aborted_hover_starts_a_fresh_delay() {
        let start = Instant::now();
        let mut state = popover(start);
        let bounds = Rect::new(0, 0, 80, 24);
        state.pointer_moved(
            Position::new(80, 10),
            bounds,
            start + Duration::from_millis(50),
        );
        let returned = start + Duration::from_millis(180);
        state.pointer_moved(Position::new(14, 22), bounds, returned);
        assert!(!state.is_visible(start + HOVER_DELAY));
        assert!(state.is_visible(returned + HOVER_DELAY));
    }

    #[test]
    fn crossing_directly_between_button_and_popover_preserves_preview() {
        let start = Instant::now();
        let mut state = popover(start);
        let bounds = Rect::new(0, 0, 80, 24);
        let geometry = layout(bounds, &state).expect("wide layout");
        assert_eq!(geometry.area.bottom(), state.anchor.y);
        let now = start + HOVER_DELAY;
        state.pointer_moved(Position::new(state.anchor.x, state.anchor.y), bounds, now);
        state.pointer_moved(
            Position::new(geometry.area.x, geometry.area.bottom() - 1),
            bounds,
            now,
        );
        assert!(!state.should_close(now + LEAVE_DELAY));
    }

    #[test]
    fn narrow_sidebar_has_one_row_with_clickable_choices() {
        let state = popover(Instant::now());
        let geometry = layout(Rect::new(0, 0, 24, 24), &state).expect("compact layout");
        assert!(geometry.compact);
        assert_eq!(geometry.area.height, 1);
        assert_eq!(geometry.area.bottom(), state.anchor.y);
        for (choice, area) in geometry.choices {
            assert!(geometry.area.contains(Position::new(area.x, area.y)));
            assert_eq!(
                geometry.hit(Position::new(area.x, area.y)),
                Some(PopoverHit::Choice(choice))
            );
        }
    }

    #[test]
    fn pinning_opens_immediately_and_does_not_expire() {
        let start = Instant::now();
        let mut state = popover(start);
        state.pin();
        assert!(state.is_visible(start));
        state.pointer_moved(Position::new(80, 10), Rect::new(0, 0, 80, 24), start);
        assert!(!state.should_close(start + Duration::from_secs(2)));
    }
}
