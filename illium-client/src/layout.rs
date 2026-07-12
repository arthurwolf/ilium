//! Shared screen geometry for rendering, resize handling, and mouse
//! hit-testing. Keeping these rectangles in one module prevents the PTY size,
//! rendered pane, and pointer-coordinate conversion from drifting apart.

use std::time::{Duration, Instant};

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Width of the left tree column, in terminal columns.
pub const TREE_WIDTH: u16 = 32;
/// Width reached while the pointer or keyboard focus is in the tree panel.
pub const EXPANDED_TREE_WIDTH: u16 = TREE_WIDTH * 2;
/// A short transition keeps the panel responsive while still making the
/// change legible instead of snapping across thirty-two terminal cells.
pub const TREE_WIDTH_ANIMATION_DURATION: Duration = Duration::from_millis(180);
/// The event loop temporarily uses this cadence while the transition is live.
pub const TREE_WIDTH_ANIMATION_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// A bordered pane needs two border cells plus at least one content cell.
const MINIMUM_PANE_WIDTH: u16 = 3;

/// Time-based presentation state for the tree panel's width. The state owns
/// reversal as well as ordinary expansion/collapse, so input handlers only
/// express the desired endpoint and never manipulate animation progress.
#[derive(Debug, Clone)]
pub struct TreeWidthAnimation {
    transition_start_width: u16,
    current_width: u16,
    target_width: u16,
    transition_started_at: Instant,
    is_animating: bool,
}

impl TreeWidthAnimation {
    /// Starts in the ordinary pane-focused state without an initial flourish.
    pub fn new(now: Instant) -> Self {
        Self {
            transition_start_width: TREE_WIDTH,
            current_width: TREE_WIDTH,
            target_width: TREE_WIDTH,
            transition_started_at: now,
            is_animating: false,
        }
    }

    /// Advances toward the focus-derived endpoint and returns the width to
    /// use for this frame. A direction change samples the old transition
    /// first, then starts from that exact visible cell width without a jump.
    pub fn update(&mut self, is_expanded: bool, now: Instant) -> u16 {
        let requested_target = if is_expanded {
            EXPANDED_TREE_WIDTH
        } else {
            TREE_WIDTH
        };
        let sampled_width = self.sample(now);

        if requested_target != self.target_width {
            self.transition_start_width = sampled_width;
            self.current_width = sampled_width;
            self.target_width = requested_target;
            self.transition_started_at = now;
            self.is_animating = sampled_width != requested_target;
            return sampled_width;
        }

        self.current_width = sampled_width;
        if now.saturating_duration_since(self.transition_started_at)
            >= TREE_WIDTH_ANIMATION_DURATION
        {
            self.current_width = self.target_width;
            self.is_animating = false;
        }

        self.current_width
    }

    /// The last width emitted by `update`, used when only the terminal's
    /// outer dimensions changed and animation time itself did not advance.
    pub const fn current_width(&self) -> u16 {
        self.current_width
    }

    /// Whether the event loop should keep requesting animation-rate frames.
    pub const fn is_animating(&self) -> bool {
        self.is_animating
    }

    /// Samples the active transition with a symmetric cubic ease-in-out.
    fn sample(&self, now: Instant) -> u16 {
        if !self.is_animating {
            return self.current_width;
        }

        let elapsed = now.saturating_duration_since(self.transition_started_at);
        if elapsed >= TREE_WIDTH_ANIMATION_DURATION {
            return self.target_width;
        }

        let progress = elapsed.as_secs_f64() / TREE_WIDTH_ANIMATION_DURATION.as_secs_f64();
        let eased_progress = ease_in_out_cubic(progress);
        let start_width = f64::from(self.transition_start_width);
        let width_delta = f64::from(self.target_width) - start_width;
        (start_width + width_delta * eased_progress).round() as u16
    }
}

/// Symmetric easing makes both expansion and collapse start and settle
/// softly while still completing quickly in the middle of the transition.
fn ease_in_out_cubic(progress: f64) -> f64 {
    if progress < 0.5 {
        4.0 * progress * progress * progress
    } else {
        1.0 - (-2.0 * progress + 2.0).powi(3) / 2.0
    }
}

/// The complete geometry of one rendered illium frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiLayout {
    pub screen_area: Rect,
    pub tree_area: Rect,
    pub pane_area: Rect,
    pub pane_content_area: Rect,
    pub status_area: Rect,
}

impl UiLayout {
    /// Computes the ordinary pane-focused geometry from the full area.
    pub fn from_screen_area(screen_area: Rect) -> Self {
        Self::from_screen_area_with_tree_width(screen_area, TREE_WIDTH)
    }

    /// Computes every stable region for one frame from the animated tree
    /// width. On narrow terminals the tree yields enough room for the pane's
    /// borders and one content cell rather than covering the pane entirely.
    pub fn from_screen_area_with_tree_width(screen_area: Rect, tree_width: u16) -> Self {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(screen_area);
        let main_area = rows[0];
        let status_area = rows[1];
        let maximum_tree_width = main_area.width.saturating_sub(MINIMUM_PANE_WIDTH);
        let constrained_tree_width = tree_width.min(maximum_tree_width);

        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(constrained_tree_width),
                Constraint::Min(MINIMUM_PANE_WIDTH),
            ])
            .split(main_area);

        // Widened by one column so the tree's right border lands on the
        // same column as the pane's left border -- `theme::block`'s
        // `merge_borders` then fuses the two into a single connected
        // divider (a `┬`/`┴` joint at top/bottom) instead of drawing two
        // separate borders side by side.
        let tree_area = Rect::new(
            columns[0].x,
            columns[0].y,
            columns[0].width.saturating_add(1),
            columns[0].height,
        );
        let pane_area = columns[1];

        // The pane's Block consumes one cell on every edge. Saturating math
        // keeps this valid even in a terminal narrower than the tree column.
        let pane_content_area = Rect::new(
            pane_area.x.saturating_add(1),
            pane_area.y.saturating_add(1),
            pane_area.width.saturating_sub(2).max(1),
            pane_area.height.saturating_sub(2).max(1),
        );

        Self {
            screen_area,
            tree_area,
            pane_area,
            pane_content_area,
            status_area,
        }
    }

    /// Returns the rows/columns that the focused PTY must use.
    pub fn pane_content_size(self) -> (u16, u16) {
        (self.pane_content_area.height, self.pane_content_area.width)
    }
}

/// Standard ratatui "centered popup" layout helper: `percent_x` /
/// `percent_y` of `area`, centered on both axes. Shared by every
/// percentage-sized overlay (`ui::draw`'s explorer popup, `help::render`)
/// so their geometry can never drift apart from each other.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_content_excludes_status_tree_and_border() {
        let layout = UiLayout::from_screen_area(Rect::new(0, 0, 120, 40));
        assert_eq!(layout.tree_area, Rect::new(0, 0, TREE_WIDTH + 1, 39));
        assert_eq!(layout.pane_area, Rect::new(TREE_WIDTH, 0, 88, 39));
        assert_eq!(layout.pane_content_size(), (37, 86));
    }

    #[test]
    fn expanded_tree_uses_twice_the_normal_split_width() {
        let layout = UiLayout::from_screen_area_with_tree_width(
            Rect::new(0, 0, 120, 40),
            EXPANDED_TREE_WIDTH,
        );

        assert_eq!(layout.tree_area.width, EXPANDED_TREE_WIDTH + 1);
        assert_eq!(layout.pane_area.x, EXPANDED_TREE_WIDTH);
        assert_eq!(layout.pane_content_size(), (37, 54));
    }

    #[test]
    fn expanded_tree_preserves_a_bordered_pane_on_narrow_terminals() {
        let layout = UiLayout::from_screen_area_with_tree_width(
            Rect::new(0, 0, 50, 20),
            EXPANDED_TREE_WIDTH,
        );

        assert_eq!(layout.pane_area.width, MINIMUM_PANE_WIDTH);
        assert_eq!(layout.pane_content_area.width, 1);
        assert_eq!(layout.tree_area.right(), layout.pane_area.x + 1);

        let narrower_than_normal = UiLayout::from_screen_area_with_tree_width(
            Rect::new(0, 0, 20, 10),
            EXPANDED_TREE_WIDTH,
        );
        assert_eq!(narrower_than_normal.pane_area, Rect::new(17, 0, 3, 9));
        assert_eq!(
            narrower_than_normal.pane_content_area,
            Rect::new(18, 1, 1, 7)
        );
        assert!(narrower_than_normal.tree_area.right() <= 20);
    }

    #[test]
    fn tree_width_animation_eases_to_double_width() {
        let started_at = Instant::now();
        let mut animation = TreeWidthAnimation::new(started_at);

        assert_eq!(animation.update(true, started_at), TREE_WIDTH);
        assert!(animation.is_animating());

        let quarter = started_at + TREE_WIDTH_ANIMATION_DURATION / 4;
        let midpoint = started_at + TREE_WIDTH_ANIMATION_DURATION / 2;
        let three_quarters = started_at + TREE_WIDTH_ANIMATION_DURATION * 3 / 4;
        assert!(animation.update(true, quarter) < TREE_WIDTH + TREE_WIDTH / 4);
        assert_eq!(
            animation.update(true, midpoint),
            TREE_WIDTH + TREE_WIDTH / 2
        );
        assert!(animation.update(true, three_quarters) > TREE_WIDTH + TREE_WIDTH * 3 / 4);

        assert_eq!(
            animation.update(true, started_at + TREE_WIDTH_ANIMATION_DURATION),
            EXPANDED_TREE_WIDTH
        );
        assert!(!animation.is_animating());
    }

    #[test]
    fn repeated_expansion_requests_do_not_restart_or_reverse_progress() {
        let started_at = Instant::now();
        let mut animation = TreeWidthAnimation::new(started_at);
        animation.update(true, started_at);

        let mut sampled_widths = Vec::new();
        for step in 1..=12 {
            let sampled_at = started_at + TREE_WIDTH_ANIMATION_DURATION * step / 12;
            sampled_widths.push(animation.update(true, sampled_at));
        }

        assert!(sampled_widths.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(sampled_widths.last(), Some(&EXPANDED_TREE_WIDTH));
        assert!(!animation.is_animating());
    }

    #[test]
    fn tree_width_animation_reverses_from_the_visible_width() {
        let started_at = Instant::now();
        let mut animation = TreeWidthAnimation::new(started_at);
        animation.update(true, started_at);

        let midpoint = started_at + TREE_WIDTH_ANIMATION_DURATION / 2;
        assert_eq!(animation.update(true, midpoint), 48);
        assert_eq!(animation.update(false, midpoint), 48);

        let collapse_midpoint = midpoint + TREE_WIDTH_ANIMATION_DURATION / 2;
        assert_eq!(animation.update(false, collapse_midpoint), 40);
        assert_eq!(
            animation.update(false, midpoint + TREE_WIDTH_ANIMATION_DURATION),
            TREE_WIDTH
        );
        assert!(!animation.is_animating());
    }
}
