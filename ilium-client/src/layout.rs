//! Shared screen geometry for rendering, resize handling, and mouse
//! hit-testing. Keeping these rectangles in one module prevents the PTY size,
//! rendered pane, and pointer-coordinate conversion from drifting apart.

use std::time::{Duration, Instant};

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Fresh-install width used while a focus-dependent left panel is inactive.
pub const DEFAULT_UNFOCUSED_TREE_WIDTH: u16 = 24;
/// Fresh-install width used while a focus-dependent left panel is active.
pub const DEFAULT_FOCUSED_TREE_WIDTH: u16 = 44;
/// Fresh-install width used by the fixed policy.
pub const DEFAULT_FIXED_TREE_WIDTH: u16 = 32;
/// Fresh-install terminal-width breakpoint for the responsive policy.
pub const DEFAULT_MINIMUM_TERMINAL_WIDTH: u16 = 120;
/// Smallest and largest left-panel widths accepted by Settings and config.
pub const MIN_TREE_WIDTH: u16 = 16;
pub const MAX_TREE_WIDTH: u16 = 80;
/// Bounds keep the responsive breakpoint practical without coupling it to
/// one monitor, terminal emulator, or current panel-width choice.
pub const MINIMUM_TERMINAL_WIDTH: u16 = 40;
pub const MAXIMUM_TERMINAL_WIDTH: u16 = 500;
/// A short transition keeps the panel responsive while still making the
/// change legible instead of snapping across thirty-two terminal cells.
pub const TREE_WIDTH_ANIMATION_DURATION: Duration = Duration::from_millis(180);
/// The event loop temporarily uses this 30 Hz cadence while the transition
/// is live. Spatial movement remains smooth at terminal-cell granularity
/// without competing with input or terminal-output processing every 16 ms.
pub const TREE_WIDTH_ANIMATION_FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// A bordered pane needs two border cells plus at least one content cell.
const MINIMUM_PANE_WIDTH: u16 = 3;
/// Width reserved for the always-visible voice control at the right edge of
/// the footer. Keeping it in shared geometry makes rendering and mouse input
/// use the exact same hit target.
pub const VOICE_CONTROL_WIDTH: u16 = 22;

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
    motion_enabled: bool,
}

impl TreeWidthAnimation {
    /// Starts in the ordinary pane-focused state without an initial flourish,
    /// resting at `initial_width`.
    pub fn new(now: Instant, initial_width: u16) -> Self {
        Self {
            transition_start_width: initial_width,
            current_width: initial_width,
            target_width: initial_width,
            transition_started_at: now,
            is_animating: false,
            motion_enabled: true,
        }
    }

    /// Disables spatial width easing while preserving the same focus-derived endpoint.
    pub fn set_motion_enabled(&mut self, enabled: bool, now: Instant) {
        self.motion_enabled = enabled;
        if !enabled {
            self.current_width = self.target_width;
            self.transition_start_width = self.target_width;
            self.is_animating = false;
            self.transition_started_at = now;
        }
    }

    /// Applies a discrete settings change immediately. Focus and terminal
    /// transitions use [`Self::update`]; editing the selected policy should
    /// make its resolved result authoritative without replaying an old target.
    pub fn snap_to(&mut self, width: u16, now: Instant) -> u16 {
        self.transition_start_width = width;
        self.current_width = width;
        self.target_width = width;
        self.transition_started_at = now;
        self.is_animating = false;
        width
    }

    /// Advances toward an explicit policy-derived endpoint and returns the
    /// width to use for this frame. A direction change samples the previous
    /// transition first, then starts from that visible width without a jump.
    pub fn update(&mut self, requested_target: u16, now: Instant) -> u16 {
        let sampled_width = self.sample(now);

        if !self.motion_enabled {
            self.current_width = requested_target;
            self.target_width = requested_target;
            self.transition_start_width = requested_target;
            self.is_animating = false;
            return requested_target;
        }

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

/// The complete geometry of one rendered ilium frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiLayout {
    pub screen_area: Rect,
    pub tree_area: Rect,
    pub pane_area: Rect,
    pub pane_content_area: Rect,
    pub status_area: Rect,
    pub voice_control_area: Rect,
}

impl UiLayout {
    /// Computes the ordinary pane-focused geometry from the full area.
    pub fn from_screen_area(screen_area: Rect) -> Self {
        Self::from_screen_area_with_tree_width(screen_area, DEFAULT_UNFOCUSED_TREE_WIDTH)
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
        let footer_area = rows[1];
        let voice_control_width = VOICE_CONTROL_WIDTH.min(footer_area.width);
        let status_area = Rect::new(
            footer_area.x,
            footer_area.y,
            footer_area.width.saturating_sub(voice_control_width),
            footer_area.height,
        );
        let voice_control_area = Rect::new(
            status_area.right(),
            footer_area.y,
            voice_control_width,
            footer_area.height,
        );
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
        // separate borders side by side. Clamped to `main_area.width` so a
        // zero-width terminal never yields a one-column tree rect past the
        // screen edge, which would render and mouse-hit-test off screen.
        let tree_area = Rect::new(
            columns[0].x,
            columns[0].y,
            columns[0].width.saturating_add(1).min(main_area.width),
            columns[0].height,
        );
        let pane_area = columns[1];

        // The pane's Block consumes one cell on every edge. `MINIMUM_PANE_WIDTH`
        // guarantees at least one content column whenever `main_area.width >= 3`,
        // but on a pathologically narrow terminal (`main_area.width` itself below
        // 3) `pane_area` can shrink to 0 or 1 cells wide/tall -- clamp the offset
        // and size to `pane_area`'s own bounds so content never spills past its
        // containing pane, instead of blindly assuming a cell that isn't there.
        let content_x = pane_area.x.saturating_add(1).min(pane_area.right());
        let content_y = pane_area.y.saturating_add(1).min(pane_area.bottom());
        let pane_content_area = Rect::new(
            content_x,
            content_y,
            pane_area
                .width
                .saturating_sub(2)
                .max(1)
                .min(pane_area.right().saturating_sub(content_x)),
            pane_area
                .height
                .saturating_sub(2)
                .max(1)
                .min(pane_area.bottom().saturating_sub(content_y)),
        );

        Self {
            screen_area,
            tree_area,
            pane_area,
            pane_content_area,
            status_area,
            voice_control_area,
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
///
/// Clamps each percentage to 100 first -- a caller-supplied value above 100
/// would otherwise underflow the `100 - percent` margin math below (`u16`
/// subtraction panics on overflow in debug builds and wraps to a huge
/// number in release, producing a garbage `Rect` either way).
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let percent_x = percent_x.min(100);
    let percent_y = percent_y.min(100);

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

    /// The animation and expanded-width assertions below use fixed 32/64
    /// widths (a 1:2 ratio) so their arithmetic does not move whenever the
    /// user-tunable fresh-install defaults do.
    const TEST_COLLAPSED_TREE_WIDTH: u16 = 32;
    const TEST_EXPANDED_TREE_WIDTH: u16 = 64;
    const EXPANDED_TREE_WIDTH: u16 = TEST_EXPANDED_TREE_WIDTH;

    #[test]
    fn pane_content_excludes_status_tree_and_border() {
        let layout = UiLayout::from_screen_area(Rect::new(0, 0, 120, 40));
        assert_eq!(
            layout.tree_area,
            Rect::new(0, 0, DEFAULT_UNFOCUSED_TREE_WIDTH + 1, 39)
        );
        assert_eq!(
            layout.pane_area,
            Rect::new(
                DEFAULT_UNFOCUSED_TREE_WIDTH,
                0,
                120 - DEFAULT_UNFOCUSED_TREE_WIDTH,
                39
            )
        );
        assert_eq!(
            layout.pane_content_size(),
            (37, 120 - DEFAULT_UNFOCUSED_TREE_WIDTH - 2)
        );
        assert_eq!(layout.status_area, Rect::new(0, 39, 98, 1));
        assert_eq!(layout.voice_control_area, Rect::new(98, 39, 22, 1));
        assert_eq!(layout.status_area.right(), layout.voice_control_area.x);
        assert_eq!(
            layout.voice_control_area.right(),
            layout.screen_area.right()
        );
    }

    #[test]
    fn voice_control_owns_the_complete_footer_on_extremely_narrow_screens() {
        let layout = UiLayout::from_screen_area(Rect::new(0, 0, 12, 5));

        assert_eq!(layout.status_area.width, 0);
        assert_eq!(layout.voice_control_area, Rect::new(0, 4, 12, 1));
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
    fn tree_width_animation_eases_between_explicit_policy_widths() {
        let started_at = Instant::now();
        let mut animation = TreeWidthAnimation::new(started_at, TEST_COLLAPSED_TREE_WIDTH);

        assert_eq!(
            animation.update(TEST_EXPANDED_TREE_WIDTH, started_at),
            TEST_COLLAPSED_TREE_WIDTH
        );
        assert!(animation.is_animating());

        let quarter = started_at + TREE_WIDTH_ANIMATION_DURATION / 4;
        let midpoint = started_at + TREE_WIDTH_ANIMATION_DURATION / 2;
        let three_quarters = started_at + TREE_WIDTH_ANIMATION_DURATION * 3 / 4;
        assert!(
            animation.update(TEST_EXPANDED_TREE_WIDTH, quarter)
                < TEST_COLLAPSED_TREE_WIDTH + TEST_COLLAPSED_TREE_WIDTH / 4
        );
        assert_eq!(
            animation.update(TEST_EXPANDED_TREE_WIDTH, midpoint),
            TEST_COLLAPSED_TREE_WIDTH + TEST_COLLAPSED_TREE_WIDTH / 2
        );
        assert!(
            animation.update(TEST_EXPANDED_TREE_WIDTH, three_quarters)
                > TEST_COLLAPSED_TREE_WIDTH + TEST_COLLAPSED_TREE_WIDTH * 3 / 4
        );

        assert_eq!(
            animation.update(
                TEST_EXPANDED_TREE_WIDTH,
                started_at + TREE_WIDTH_ANIMATION_DURATION
            ),
            EXPANDED_TREE_WIDTH
        );
        assert!(!animation.is_animating());
    }

    #[test]
    fn repeated_expansion_requests_do_not_restart_or_reverse_progress() {
        let started_at = Instant::now();
        let mut animation = TreeWidthAnimation::new(started_at, TEST_COLLAPSED_TREE_WIDTH);
        animation.update(TEST_EXPANDED_TREE_WIDTH, started_at);

        let mut sampled_widths = Vec::new();
        for step in 1..=12 {
            let sampled_at = started_at + TREE_WIDTH_ANIMATION_DURATION * step / 12;
            sampled_widths.push(animation.update(TEST_EXPANDED_TREE_WIDTH, sampled_at));
        }

        assert!(sampled_widths.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(sampled_widths.last(), Some(&EXPANDED_TREE_WIDTH));
        assert!(!animation.is_animating());
    }

    #[test]
    fn tree_width_animation_reverses_from_the_visible_width() {
        let started_at = Instant::now();
        let mut animation = TreeWidthAnimation::new(started_at, TEST_COLLAPSED_TREE_WIDTH);
        animation.update(TEST_EXPANDED_TREE_WIDTH, started_at);

        let midpoint = started_at + TREE_WIDTH_ANIMATION_DURATION / 2;
        assert_eq!(animation.update(TEST_EXPANDED_TREE_WIDTH, midpoint), 48);
        assert_eq!(animation.update(TEST_COLLAPSED_TREE_WIDTH, midpoint), 48);

        let collapse_midpoint = midpoint + TREE_WIDTH_ANIMATION_DURATION / 2;
        assert_eq!(
            animation.update(TEST_COLLAPSED_TREE_WIDTH, collapse_midpoint),
            40
        );
        assert_eq!(
            animation.update(
                TEST_COLLAPSED_TREE_WIDTH,
                midpoint + TREE_WIDTH_ANIMATION_DURATION
            ),
            TEST_COLLAPSED_TREE_WIDTH
        );
        assert!(!animation.is_animating());
    }

    #[test]
    fn pane_content_area_never_exceeds_pane_area_on_pathologically_narrow_screens() {
        for screen_area in [
            Rect::new(0, 0, 1, 1),
            Rect::new(0, 0, 2, 3),
            Rect::new(0, 0, 0, 0),
        ] {
            let layout = UiLayout::from_screen_area(screen_area);
            let pane_area = layout.pane_area;
            let content_area = layout.pane_content_area;
            assert!(
                content_area.x >= pane_area.x
                    && content_area.y >= pane_area.y
                    && content_area.right() <= pane_area.right()
                    && content_area.bottom() <= pane_area.bottom(),
                "content_area {content_area:?} escaped pane_area {pane_area:?} for screen {screen_area:?}"
            );
        }
    }

    #[test]
    fn snap_to_applies_a_settings_width_without_animating() {
        let started_at = Instant::now();
        let mut animation = TreeWidthAnimation::new(started_at, TEST_COLLAPSED_TREE_WIDTH);

        let width = animation.snap_to(20, started_at);
        assert_eq!(width, 20);
        assert_eq!(animation.current_width(), 20);
        assert!(!animation.is_animating());
    }
}
