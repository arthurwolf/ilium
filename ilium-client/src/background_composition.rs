//! Final-buffer decoration; terminal bytes and render caches remain authoritative.
//!
//! Compose after the ordinary workspace and before overlays. Only known ambient
//! regions and plain ASCII-space cells accept Braille. Wide glyph spans, native
//! terminal continuations/cursors, styling, and diff controls remain untouched.

use std::time::Duration;

use ratatui::buffer::{Buffer, Cell, CellDiffOption};
use ratatui::layout::{Position, Rect};
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Mode, PaneRuntime, RightPanelTarget};
use crate::config::MotionLevel;
use crate::theme::ColorScheme;

const FRAMES_PER_SECOND: u128 = 12;
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// One absolute clock shared by the session compositor and Settings preview.
/// Integer boundaries alternate 83_333_333/83_333_334 ns and do not accumulate
/// the drift of an 83 ms interval. An input redraw inside a bucket reuses its
/// exact engine cache key rather than advancing motion.
pub fn quantized_elapsed(elapsed: Duration) -> Duration {
    let bucket = elapsed_bucket(elapsed);
    let nanos = (bucket * NANOS_PER_SECOND).div_ceil(FRAMES_PER_SECOND);
    // The bucket boundary never exceeds `elapsed`, which is a Duration, so
    // its whole seconds and fractional nanoseconds fit their respective types.
    Duration::new(
        (nanos / NANOS_PER_SECOND) as u64,
        (nanos % NANOS_PER_SECOND) as u32,
    )
}

fn elapsed_bucket(elapsed: Duration) -> u128 {
    // Even Duration::MAX's nanoseconds times 12 fit comfortably in u128.
    elapsed.as_nanos() * FRAMES_PER_SECOND / NANOS_PER_SECOND
}

/// Ambient composition owns only the ordinary workspace. Suspended settings,
/// full-screen modes, and modal stacks remain opaque, including Smart Copy's
/// immutable inspection surface.
pub fn ambient_is_visible(app: &App) -> bool {
    app.animation_settings.enabled
        && app.modal_stack.is_empty()
        && matches!(app.mode, Mode::Normal)
        && app.smart_copy_session.is_none()
        && !app.layout.screen_area.is_empty()
}

fn live_animation_is_visible(app: &App) -> bool {
    !app.layout.screen_area.is_empty()
        && (app.is_animation_preview_visible()
            || (ambient_is_visible(app) && app.ui_settings.motion_level != MotionLevel::Off))
}

/// `None` means animation contributes no recurring deadline. Preview remains
/// live when deliberately opened, even with ambient disabled or Motion Off.
pub fn animation_frame_bucket(app: &App, elapsed: Duration) -> Option<u128> {
    live_animation_is_visible(app).then(|| elapsed_bucket(elapsed))
}

/// Positive delay to the next absolute frame boundary. This must be minimized
/// with existing maintenance/output delays rather than replacing those clocks.
pub fn animation_frame_delay(app: &App, elapsed: Duration) -> Option<Duration> {
    if !live_animation_is_visible(app) {
        return None;
    }
    let next_bucket = elapsed_bucket(elapsed) + 1;
    let next_nanos = (next_bucket * NANOS_PER_SECOND).div_ceil(FRAMES_PER_SECOND);
    let delay_nanos = next_nanos - elapsed.as_nanos();
    // A single frame is at most 83_333_334 ns, including Duration::MAX input.
    Some(Duration::from_nanos(delay_nanos as u64))
}

/// Render one screen-wide field, then reveal it only through safe workspace
/// blanks. Coordinates stay relative to the whole buffer, never each pane.
/// Motion Off uses a stable zero-time scene and has no animation timer.
pub fn compose(buffer: &mut Buffer, app: &mut App, elapsed: Duration) {
    if !ambient_is_visible(app) || buffer.area.is_empty() {
        return;
    }
    let settings = app.animation_settings.normalized();
    let elapsed = if app.ui_settings.motion_level == MotionLevel::Off {
        Duration::ZERO
    } else {
        quantized_elapsed(elapsed)
    };
    app.animation_frame
        .render(&settings, buffer.area.width, buffer.area.height, elapsed);
    let foreground = match app.ui_settings.color_scheme {
        ColorScheme::Dark => Color::White,
        ColorScheme::Light => Color::Black,
    };
    paint_region(
        buffer,
        panel_inner(app.layout.tree_area),
        None,
        foreground,
        |column, row| app.animation_frame.glyph(column, row),
    );

    if matches!(app.right_panel_target, RightPanelTarget::Chatroom { .. }) {
        return;
    }
    let viewports = app.pane_viewports();
    if viewports.is_empty() {
        // This is draw_pane's known empty/loading placeholder, not an unknown
        // editor, board, search, settings, or chatroom surface.
        paint_region(
            buffer,
            panel_inner(app.layout.pane_area),
            None,
            foreground,
            |column, row| app.animation_frame.glyph(column, row),
        );
        return;
    }
    for viewport in viewports {
        let Some(PaneRuntime::Terminal(terminal)) = app.panes.get(&viewport.pane_id) else {
            continue;
        };
        let area = app
            .completed_agent_close_action(viewport)
            .map_or(viewport.content_area, |action| action.terminal_area);
        terminal.with_screen(|screen| {
            paint_region(buffer, area, Some(screen), foreground, |column, row| {
                app.animation_frame.glyph(column, row)
            });
        });
    }
}

fn panel_inner(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

fn is_safe_blank(cell: &Cell) -> bool {
    cell.symbol() == " "
        && cell.bg == Color::Reset
        && cell.modifier.is_empty()
        && cell.diff_option == CellDiffOption::None
}

fn visible_cursor(screen: &vt100::Screen, area: Rect) -> Option<Position> {
    if screen.hide_cursor() {
        return None;
    }
    let (row, column) = screen.cursor_position();
    // Mirror tui-term's Screen adapter: a historical viewport offsets the
    // drawing-grid cursor by its independently retained scrollback position.
    let row = row.saturating_add(u16::try_from(screen.scrollback()).unwrap_or(u16::MAX));
    (row < area.height && column < area.width)
        .then(|| Position::new(area.x.saturating_add(column), area.y.saturating_add(row)))
}

fn paint_region(
    buffer: &mut Buffer,
    region: Rect,
    screen: Option<&vt100::Screen>,
    foreground: Color,
    mut glyph: impl FnMut(u16, u16) -> char,
) {
    let clipped = region.intersection(buffer.area);
    if clipped.is_empty() {
        return;
    }
    let cursor = screen.and_then(|screen| visible_cursor(screen, region));
    for row in clipped.top()..clipped.bottom() {
        let mut remaining_continuations = 0;
        // Begin at the buffer's left edge: a wide leading glyph can lie
        // outside the allowed region while its continuation lies inside it.
        for column in buffer.area.left()..clipped.right() {
            if remaining_continuations > 0 {
                remaining_continuations -= 1;
                continue;
            }
            let cell = &buffer[(column, row)];
            let width = UnicodeWidthStr::width(cell.symbol());
            remaining_continuations = width.saturating_sub(1);
            if column < clipped.left()
                || width != 1
                || !is_safe_blank(cell)
                || cursor == Some(Position::new(column, row))
            {
                continue;
            }
            let native_continuation = screen
                .and_then(|screen| screen.cell(row - region.y, column - region.x))
                .is_some_and(vt100::Cell::is_wide_continuation);
            if native_continuation {
                continue;
            }
            let character = glyph(column - buffer.area.x, row - buffer.area.y);
            // Empty Braille remains an ordinary space. An invalid engine glyph
            // must not introduce a wide symbol into this single-cell overlay.
            if !('\u{2801}'..='\u{28ff}').contains(&character) {
                continue;
            }
            let cell = &mut buffer[(column, row)];
            cell.set_char(character).set_fg(foreground);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{SettingsState, SettingsTab};
    use crate::terminal_selection::{SelectionPoint, TerminalSelection};
    use crate::terminal_view::TerminalView;
    use ilium_core::NodeId;
    use ratatui::style::Modifier;
    use ratatui::widgets::{Clear, Widget};

    fn paint_all(buffer: &mut Buffer) {
        let area = buffer.area;
        paint_region(buffer, area, None, Color::White, |_, _| '\u{28ff}');
    }

    #[test]
    fn protected_cells_retain_symbols_styles_and_diff_controls() {
        let area = Rect::new(0, 0, 10, 1);
        let mut buffer = Buffer::empty(area);
        buffer[(1, 0)].set_symbol("T");
        buffer[(2, 0)].set_bg(Color::Black);
        buffer[(3, 0)].modifier = Modifier::REVERSED;
        buffer[(4, 0)].modifier = Modifier::BOLD;
        buffer[(5, 0)].set_diff_option(CellDiffOption::Skip);
        buffer[(6, 0)].set_diff_option(CellDiffOption::AlwaysUpdate);
        buffer[(7, 0)].set_symbol("\u{2800}");
        buffer[(8, 0)].set_symbol("\u{a0}");
        buffer[(9, 0)].set_fg(Color::Cyan);
        let original = buffer.clone();

        paint_all(&mut buffer);

        for column in 1..=8 {
            assert_eq!(buffer[(column, 0)], original[(column, 0)]);
        }
        for column in [0, 9] {
            assert_eq!(buffer[(column, 0)].symbol(), "\u{28ff}");
            assert_eq!(buffer[(column, 0)].fg, Color::White);
            assert_eq!(buffer[(column, 0)].bg, Color::Reset);
            assert!(buffer[(column, 0)].modifier.is_empty());
        }
    }

    #[test]
    fn cjk_and_vs16_continuations_stay_empty_before_final_diff_repair() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 9, 1));
        buffer[(1, 0)].set_symbol("界");
        buffer[(5, 0)].set_symbol("🖥️");
        let original = buffer.clone();
        assert_eq!(UnicodeWidthStr::width("🖥️"), 2);

        paint_all(&mut buffer);

        for column in [1, 2, 5, 6] {
            assert_eq!(buffer[(column, 0)], original[(column, 0)]);
        }
        assert_eq!(buffer[(3, 0)].symbol(), "\u{28ff}");
        assert_eq!(buffer[(7, 0)].symbol(), "\u{28ff}");
    }

    #[test]
    fn a_wide_lead_outside_the_allowed_rectangle_still_protects_its_tail() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        buffer[(2, 0)].set_symbol("界");
        let tail = buffer[(3, 0)].clone();

        paint_region(
            &mut buffer,
            Rect::new(3, 0, 3, 1),
            None,
            Color::White,
            |_, _| '\u{28ff}',
        );

        assert_eq!(buffer[(3, 0)], tail);
        assert_eq!(buffer[(4, 0)].symbol(), "\u{28ff}");
        assert_eq!(buffer[(6, 0)].symbol(), " ");
    }

    #[test]
    fn native_continuation_metadata_is_protected_independently_of_buffer_width() {
        let mut parser = vt100::Parser::new(1, 8, 0);
        parser.process("\u{1b}[?25l界".as_bytes());
        assert!(parser.screen().cell(0, 1).unwrap().is_wide_continuation());
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        // Isolate the metadata guard from the separate Unicode-width guard.
        buffer[(0, 0)].set_symbol(".");
        let area = buffer.area;

        paint_region(
            &mut buffer,
            area,
            Some(parser.screen()),
            Color::White,
            |_, _| '\u{28ff}',
        );

        assert_eq!(buffer[(1, 0)].symbol(), " ");
        assert_eq!(buffer[(2, 0)].symbol(), "\u{28ff}");
    }

    #[test]
    fn a_plain_native_cursor_cell_is_protected_even_without_cursor_styling() {
        let mut parser = vt100::Parser::new(2, 8, 0);
        parser.process(b"\x1b[1;4H");
        let region = Rect::new(2, 1, 8, 2);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 4));
        let cursor = Position::new(5, 1);
        assert_eq!(visible_cursor(parser.screen(), region), Some(cursor));
        let original_cursor = buffer[(cursor.x, cursor.y)].clone();

        paint_region(
            &mut buffer,
            region,
            Some(parser.screen()),
            Color::White,
            |_, _| '\u{28ff}',
        );

        assert_eq!(buffer[(cursor.x, cursor.y)], original_cursor);
        assert_eq!(buffer[(4, 1)].symbol(), "\u{28ff}");
        parser.process(b"\x1b[?25l");
        assert_eq!(visible_cursor(parser.screen(), region), None);
    }

    #[test]
    fn separate_regions_sample_the_same_field_with_nonzero_buffer_origin() {
        let mut buffer = Buffer::empty(Rect::new(10, 20, 8, 2));
        let glyph = |column, row| {
            if column == 5 && row == 1 {
                '\u{2802}'
            } else {
                '\u{2801}'
            }
        };
        paint_region(
            &mut buffer,
            Rect::new(11, 20, 2, 2),
            None,
            Color::White,
            glyph,
        );
        paint_region(
            &mut buffer,
            Rect::new(15, 20, 2, 2),
            None,
            Color::White,
            glyph,
        );

        assert_eq!(buffer[(11, 21)].symbol(), "\u{2801}");
        assert_eq!(buffer[(15, 21)].symbol(), "\u{2802}");
        assert_eq!(buffer[(14, 21)].symbol(), " ");
        assert_eq!(buffer[(17, 21)].symbol(), " ");
    }

    #[test]
    fn empty_and_outside_regions_are_no_ops_and_empty_braille_stays_ascii_space() {
        let mut buffer = Buffer::empty(Rect::new(4, 5, 3, 2));
        let original = buffer.clone();
        paint_region(
            &mut buffer,
            Rect::new(0, 0, 0, 0),
            None,
            Color::White,
            |_, _| '\u{28ff}',
        );
        paint_region(
            &mut buffer,
            Rect::new(100, 100, 3, 2),
            None,
            Color::White,
            |_, _| '\u{28ff}',
        );
        assert_eq!(buffer, original);
        let area = buffer.area;
        paint_region(&mut buffer, area, None, Color::White, |_, _| '\u{2800}');
        assert_eq!(buffer, original);
    }

    #[test]
    fn later_clear_keeps_the_entire_overlay_rectangle_opaque() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 6));
        paint_all(&mut buffer);
        let overlay = Rect::new(3, 2, 6, 2);
        Clear.render(overlay, &mut buffer);
        for row in overlay.top()..overlay.bottom() {
            for column in overlay.left()..overlay.right() {
                assert_eq!(buffer[(column, row)], Cell::default());
            }
        }
        assert_eq!(buffer[(2, 2)].symbol(), "\u{28ff}");
    }

    #[test]
    fn decoration_cannot_enter_terminal_cache_history_screen_or_selection_text() {
        let mut terminal = TerminalView::new(3, 20);
        terminal.feed("\u{1b}[?25lA 界 B\r\nraw history".as_bytes());
        let area = Rect::new(0, 0, 20, 3);
        let selection = TerminalSelection {
            pane_id: NodeId(1),
            anchor: SelectionPoint::new(0, 0),
            cursor: SelectionPoint::new(0, 5),
        };
        let before_screen = terminal.with_screen(vt100::Screen::contents);
        let before_history = terminal.searchable_history();
        let before_copy =
            terminal.with_screen(|screen| crate::terminal_selection::text(screen, &selection));
        let mut clean_frame = Buffer::empty(area);
        terminal.render_screen(area, &mut clean_frame);
        let mut decorated_frame = clean_frame.clone();
        terminal.with_screen(|screen| {
            paint_region(
                &mut decorated_frame,
                area,
                Some(screen),
                Color::White,
                |_, _| '\u{28ff}',
            );
        });
        assert_ne!(decorated_frame, clean_frame);

        let mut reloaded_frame = Buffer::empty(area);
        terminal.render_screen(area, &mut reloaded_frame);
        assert_eq!(reloaded_frame, clean_frame);
        assert_eq!(terminal.with_screen(vt100::Screen::contents), before_screen);
        assert_eq!(terminal.searchable_history(), before_history);
        assert_eq!(
            terminal.with_screen(|screen| crate::terminal_selection::text(screen, &selection)),
            before_copy
        );
    }

    #[test]
    fn exact_clock_reuses_buckets_and_has_positive_absolute_deadlines() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("clock".to_owned(), project.path().to_path_buf());
        app.set_screen_area(Rect::new(0, 0, 80, 24));
        app.animation_settings.enabled = true;
        app.ui_settings.motion_level = MotionLevel::Reduced;
        assert_eq!(
            animation_frame_delay(&app, Duration::ZERO),
            Some(Duration::from_nanos(83_333_334))
        );
        let before_boundary = Duration::from_nanos(83_333_333);
        assert_eq!(quantized_elapsed(before_boundary), Duration::ZERO);
        assert_eq!(
            animation_frame_delay(&app, before_boundary),
            Some(Duration::from_nanos(1))
        );
        let boundary = Duration::from_nanos(83_333_334);
        assert_eq!(quantized_elapsed(boundary), boundary);
        assert_eq!(animation_frame_bucket(&app, boundary), Some(1));
        assert_eq!(
            quantized_elapsed(boundary + Duration::from_millis(1)),
            boundary
        );
        assert_eq!(
            quantized_elapsed(Duration::from_secs(1)),
            Duration::from_secs(1)
        );
        assert_eq!(
            animation_frame_bucket(&app, Duration::from_secs(1)),
            Some(12)
        );
        assert!(animation_frame_delay(&app, Duration::MAX).unwrap() > Duration::ZERO);
        let maximum_sample = quantized_elapsed(Duration::MAX);
        assert_eq!(quantized_elapsed(maximum_sample), maximum_sample);
    }

    #[test]
    fn disabled_off_and_opaque_states_add_no_timer_but_preview_remains_live() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("clock".to_owned(), project.path().to_path_buf());
        app.set_screen_area(Rect::new(0, 0, 80, 24));
        assert_eq!(animation_frame_delay(&app, Duration::ZERO), None);
        app.animation_settings.enabled = true;
        app.ui_settings.motion_level = MotionLevel::Off;
        assert!(ambient_is_visible(&app));
        assert_eq!(animation_frame_delay(&app, Duration::ZERO), None);
        app.ui_settings.motion_level = MotionLevel::Reduced;
        assert!(animation_frame_delay(&app, Duration::ZERO).is_some());
        app.mode = Mode::Help;
        assert!(!ambient_is_visible(&app));
        assert_eq!(animation_frame_delay(&app, Duration::ZERO), None);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..SettingsState::default()
        });
        app.animation_settings.enabled = false;
        app.ui_settings.motion_level = MotionLevel::Off;
        assert!(animation_frame_delay(&app, Duration::ZERO).is_some());
        app.modal_stack.push(Mode::Help);
        assert_eq!(animation_frame_delay(&app, Duration::ZERO), None);
    }
}
