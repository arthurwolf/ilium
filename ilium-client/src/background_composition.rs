//! Final-buffer decoration; terminal bytes and render caches remain authoritative.
//!
//! Compose after the ordinary workspace and before overlays. Only known ambient
//! regions and safe native blank cells accept Braille or article text. Wide glyph
//! spans, native continuations/cursors, styling, and diff controls stay untouched.

use std::time::Duration;

use ratatui::buffer::{Buffer, Cell, CellDiffOption};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Mode, PaneRuntime, RightPanelTarget};
use crate::config::MotionLevel;

/// Cadence of the built-in scenes and of any hosted scene that has not
/// declared its own (`Scene::frames_per_second`).
pub const DEFAULT_FRAMES_PER_SECOND: u32 = 30;
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// One absolute clock shared by the session compositor and Settings preview,
/// at the default 12 frames per second. See [`quantized_elapsed_at`].
pub fn quantized_elapsed(elapsed: Duration) -> Duration {
    quantized_elapsed_at(elapsed, DEFAULT_FRAMES_PER_SECOND)
}

/// The start of the frame bucket containing `elapsed` at `frames_per_second`.
/// Integer boundaries (`ceil(bucket * 1e9 / fps)` ns) do not accumulate the
/// drift of a rounded interval: at 12 fps they alternate 33_333_333 and
/// 33_333_334 ns. An input redraw inside a bucket reuses its exact engine
/// cache key rather than advancing motion.
pub fn quantized_elapsed_at(elapsed: Duration, frames_per_second: u32) -> Duration {
    let bucket = elapsed_bucket(elapsed, frames_per_second);
    let nanos = bucket_start_nanos(bucket, frames_per_second);
    // The bucket boundary never exceeds `elapsed`, which is a Duration, so
    // its whole seconds and fractional nanoseconds fit their respective types.
    Duration::new(
        (nanos / NANOS_PER_SECOND) as u64,
        (nanos % NANOS_PER_SECOND) as u32,
    )
}

fn frames_per_second_or_default(frames_per_second: u32) -> u128 {
    u128::from(frames_per_second.max(1))
}

fn bucket_start_nanos(bucket: u128, frames_per_second: u32) -> u128 {
    (bucket * NANOS_PER_SECOND).div_ceil(frames_per_second_or_default(frames_per_second))
}

/// Index of the frame bucket containing `elapsed`.
pub fn elapsed_bucket(elapsed: Duration, frames_per_second: u32) -> u128 {
    // Even Duration::MAX's nanoseconds times 30 fit comfortably in u128.
    elapsed.as_nanos() * frames_per_second_or_default(frames_per_second) / NANOS_PER_SECOND
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
            || (ambient_is_visible(app)
                && (app.ui_settings.motion_level != MotionLevel::Off
                    || app.animation_settings.kind
                        == crate::background_animation::AnimationKind::Wikipedia)))
}

/// `None` means animation contributes no recurring deadline. Preview remains
/// live when deliberately opened, even with ambient disabled or Motion Off.
/// The bucket size follows the field on screen (`App::animation_frames_per_second`).
pub fn animation_frame_bucket(app: &App, elapsed: Duration) -> Option<u128> {
    live_animation_is_visible(app)
        .then(|| elapsed_bucket(elapsed, app.animation_frames_per_second()))
}

/// Positive delay to the next absolute frame boundary. This must be minimized
/// with existing maintenance/output delays rather than replacing those clocks.
/// A scene at 1 frame per second wakes the loop once a second, not twelve times.
pub fn animation_frame_delay(app: &App, elapsed: Duration) -> Option<Duration> {
    if !live_animation_is_visible(app) {
        return None;
    }
    let frames_per_second = app.animation_frames_per_second();
    let next_bucket = elapsed_bucket(elapsed, frames_per_second) + 1;
    let next_nanos = bucket_start_nanos(next_bucket, frames_per_second);
    let delay_nanos = next_nanos - elapsed.as_nanos();
    // A single frame is at most one second, including Duration::MAX input.
    Some(Duration::from_nanos(delay_nanos as u64))
}

/// Renders the field for the current settings into the shared frame, through
/// the loop cache when the configuration allows it.
fn render_field(
    app: &mut App,
    settings: &crate::background_animation::AnimationSettings,
    area: Rect,
    elapsed: Duration,
) {
    let pointer = app.animation_pointer(area);
    app.animation_frame.pointer(pointer);
    let cache_ready = if settings.uses_loop_cache() {
        let mut cache = app.animation_cache.borrow_mut();
        cache.step(settings, area.width, area.height, 8);
        cache.status().is_ready && cache.copy_frame_into(elapsed, &mut app.animation_frame)
    } else {
        app.animation_cache.borrow_mut().pause();
        false
    };
    if !cache_ready {
        app.animation_frame
            .render(settings, area.width, area.height, elapsed);
    }
}

/// Render one screen-wide field, then reveal it only through safe workspace
/// blanks. Coordinates stay relative to the whole buffer, never each pane.
/// Motion Off uses a stable zero-time scene and has no animation timer.
///
/// The same field also backs the Settings -> Animations preview: identical
/// dimensions, coordinates, clock and hosted scene, revealed through every
/// safe blank of the settings screen (its controls panel is opaque). The
/// hosted scene and article worker are dropped whenever neither surface is visible.
pub fn compose(buffer: &mut Buffer, app: &mut App, elapsed: Duration) {
    let is_preview = app.is_animation_preview_visible();
    if buffer.area.is_empty() || !(is_preview || ambient_is_visible(app)) {
        app.animation_frame.release_hosts();
        app.animation_cache.borrow_mut().pause();
        return;
    }
    let mut settings = app.animation_settings.normalized();
    if settings.kind == crate::background_animation::AnimationKind::OpenStreetMap
        && crate::layout::osm_attribution_area(buffer.area).is_empty()
    {
        // A tiny terminal cannot show both a map and the complete credit.
        app.animation_frame.release_hosts();
        app.animation_cache.borrow_mut().pause();
        return;
    }
    let frozen_article = !is_preview
        && app.ui_settings.motion_level == MotionLevel::Off
        && settings.kind == crate::background_animation::AnimationKind::Wikipedia;
    if frozen_article {
        // Keep wall time for loader retries and readiness, but pause article travel.
        settings.wikipedia.scroll_tenths = 0;
    }
    // Motion Off freezes the ambient background; the explicit preview stays live.
    let elapsed =
        if !is_preview && app.ui_settings.motion_level == MotionLevel::Off && !frozen_article {
            Duration::ZERO
        } else {
            quantized_elapsed_at(elapsed, app.animation_frames_per_second())
        };
    render_field(app, &settings, buffer.area, elapsed);
    let (red, green, blue) = settings.foreground_rgb();
    let foreground = Color::Rgb(red, green, blue);
    let look = LookPaint {
        appearance: &settings.appearance,
        ink: [red, green, blue],
        columns: app.animation_frame.width(),
        rows: app.animation_frame.height(),
        seconds: elapsed.as_secs_f32(),
    };
    if is_preview {
        let area = buffer.area;
        // Article letters in label whitespace become part of the UI wording.
        // Keep the same page coordinates, but leave Settings chrome opaque.
        let preview_area =
            if settings.kind == crate::background_animation::AnimationKind::OpenStreetMap {
                let credit_rows = crate::layout::osm_attribution_area(area).height;
                Rect::new(
                    area.x,
                    area.y,
                    area.width,
                    area.height.saturating_sub(1 + credit_rows),
                )
            } else if app.animation_frame.is_wikipedia() && settings.wikipedia.uses_native_text() {
                match &app.mode {
                    Mode::Settings(state) if !state.animation_fullscreen => {
                        crate::settings_ui::compute_layout_for_mode(area, app, state).content_area
                    }
                    _ => Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1)),
                }
            } else {
                area
            };
        let opaque_panel = match &app.mode {
            Mode::Settings(state) if !state.animation_fullscreen => Some(
                crate::animation_settings_ui::layout(
                    crate::settings_ui::compute_layout_for_mode(area, app, state).content_area,
                )
                .panel,
            ),
            _ => None,
        };
        paint_region_with_field(
            buffer,
            preview_area,
            None,
            foreground,
            |column, row| {
                if opaque_panel.is_some_and(|panel| {
                    panel.contains(Position::new(area.x + column, area.y + row))
                }) {
                    FieldCell::Empty
                } else {
                    field_cell(app, &look, column, row)
                }
            },
            app.animation_frame.is_wikipedia(),
        );
        return;
    }
    if settings.panels.shows_left() {
        paint_region_with_field(
            buffer,
            panel_inner(app.layout.tree_area),
            None,
            foreground,
            |column, row| field_cell(app, &look, column, row),
            app.animation_frame.is_wikipedia(),
        );
    }

    if !settings.panels.shows_right()
        || matches!(app.right_panel_target, RightPanelTarget::Chatroom { .. })
    {
        return;
    }
    let viewports = app.pane_viewports();
    if viewports.is_empty() {
        // This is draw_pane's known empty/loading placeholder, not an unknown
        // editor, board, search, settings, or chatroom surface.
        paint_region_with_field(
            buffer,
            panel_inner(app.layout.pane_area),
            None,
            foreground,
            |column, row| field_cell(app, &look, column, row),
            app.animation_frame.is_wikipedia(),
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
            paint_region_with_field(
                buffer,
                area,
                Some(screen),
                foreground,
                |column, row| field_cell(app, &look, column, row),
                app.animation_frame.is_wikipedia(),
            );
        });
    }
}

/// Everything needed to colour one ink cell through the shared look.
struct LookPaint<'a> {
    appearance: &'a ilium_ambient::style::Appearance,
    ink: [u8; 3],
    /// Field size in cells, for the position of a cell on the screen.
    columns: u16,
    rows: u16,
    seconds: f32,
}

impl LookPaint<'_> {
    /// The colour of a cell. `None` means "paint the plain ink": nothing in
    /// the look changes it and the scene supplied no colour.
    fn color(&self, app: &App, column: u16, row: u16, is_text: bool) -> Option<Color> {
        let scene = app
            .animation_frame
            .cell_color(column, row)
            .map(|(r, g, b)| [r, g, b]);
        if self.appearance.is_neutral() {
            return scene.map(|[r, g, b]| Color::Rgb(r, g, b));
        }
        let coverage = if is_text {
            1.0
        } else {
            app.animation_frame
                .packed_cells()
                .get(usize::from(row) * usize::from(self.columns) + usize::from(column))
                .map_or(1.0, |bits| f32::from(bits.count_ones() as u8) / 8.0)
        };
        let fraction = |value: u16, extent: u16| {
            if extent <= 1 {
                0.5
            } else {
                f32::from(value) / f32::from(extent - 1)
            }
        };
        let [r, g, b] = self.appearance.shade(
            self.ink,
            scene,
            &ilium_ambient::style::CellContext {
                coverage,
                x: fraction(column, self.columns),
                y: fraction(row, self.rows),
                seconds: self.seconds,
            },
        );
        Some(Color::Rgb(r, g, b))
    }
}

enum FieldCell {
    Empty,
    Continuation,
    Native {
        symbol: char,
        color: Option<Color>,
    },
    Ink {
        symbol: String,
        color: Option<Color>,
        modifier: Modifier,
    },
}

fn field_cell(app: &App, look: &LookPaint<'_>, column: u16, row: u16) -> FieldCell {
    let frame = &app.animation_frame;
    if frame.is_wikipedia() {
        if frame.article_is_continuation(column, row) {
            return FieldCell::Continuation;
        }
        let Some(symbol) = frame.article_symbol(column, row) else {
            return FieldCell::Empty;
        };
        let (bold, italic) = frame.article_style(column, row);
        let mut modifier = Modifier::empty();
        if bold {
            modifier |= Modifier::BOLD;
        }
        if italic {
            modifier |= Modifier::ITALIC;
        }
        return FieldCell::Ink {
            symbol: symbol.to_owned(),
            color: look.color(app, column, row, true),
            modifier,
        };
    }
    if let Some(symbol) = frame.native_glyph(column, row) {
        return FieldCell::Native {
            symbol,
            color: look.color(app, column, row, true),
        };
    }
    let symbol = frame.glyph(column, row);
    if symbol == ' ' {
        return FieldCell::Empty;
    }
    FieldCell::Ink {
        symbol: symbol.to_string(),
        color: look.color(app, column, row, false),
        modifier: Modifier::empty(),
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

/// Modifiers that leave a blank cell visually empty. Underline, reverse and
/// strike-through draw ink or a filled block even on a space, so they stay out.
const INKLESS_MODIFIERS: Modifier = Modifier::BOLD.union(Modifier::DIM).union(Modifier::ITALIC);

/// A symbol that draws nothing: an ordinary space, NBSP and other width-1
/// Unicode whitespace, or the empty Braille pattern. Agent TUIs emit these
/// (for example Claude Code's `>` prompt is followed by NBSP) instead of
/// leaving the cell untouched.
fn is_inkless_symbol(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol
            .chars()
            .all(|character| character.is_whitespace() || character == '\u{2800}')
}

/// An explicit black fill that agent TUIs paint over their "empty" rows. On
/// the dark terminals ilium targets it is indistinguishable from the default
/// background, so it counts as void; any other fill is intentional colour.
fn is_void_background(color: Color) -> bool {
    matches!(
        color,
        Color::Reset | Color::Black | Color::Indexed(0 | 16) | Color::Rgb(0, 0, 0)
    )
}

/// A cell the animation may overwrite: nothing visible is drawn there, whether
/// the program left it untouched or wrote a styled-but-inkless blank.
fn is_safe_blank(cell: &Cell) -> bool {
    is_inkless_symbol(cell.symbol())
        && is_void_background(cell.bg)
        && (cell.modifier - INKLESS_MODIFIERS).is_empty()
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

#[cfg(test)]
fn paint_region(
    buffer: &mut Buffer,
    region: Rect,
    screen: Option<&vt100::Screen>,
    foreground: Color,
    glyph: impl FnMut(u16, u16) -> char,
) {
    paint_region_with_colors(buffer, region, screen, foreground, glyph, |_, _| None);
}

#[cfg(test)]
fn paint_region_with_colors(
    buffer: &mut Buffer,
    region: Rect,
    screen: Option<&vt100::Screen>,
    foreground: Color,
    glyph: impl FnMut(u16, u16) -> char,
    mut cell_color: impl FnMut(u16, u16) -> Option<Color>,
) {
    paint_region_with_style(
        buffer,
        region,
        screen,
        foreground,
        glyph,
        |column, row| (cell_color(column, row), Modifier::empty()),
        false,
    );
}

/// Legacy tests exercise the same compositor through a char-only source.
#[cfg(test)]
fn paint_region_with_style(
    buffer: &mut Buffer,
    region: Rect,
    screen: Option<&vt100::Screen>,
    foreground: Color,
    mut glyph: impl FnMut(u16, u16) -> char,
    mut cell_style: impl FnMut(u16, u16) -> (Option<Color>, Modifier),
    allow_text: bool,
) {
    paint_region_with_field(
        buffer,
        region,
        screen,
        foreground,
        |column, row| {
            let (color, modifier) = cell_style(column, row);
            FieldCell::Ink {
                symbol: glyph(column, row).to_string(),
                color,
                modifier,
            }
        },
        allow_text,
    );
}

fn safe_target(
    buffer: &Buffer,
    region: Rect,
    screen: Option<&vt100::Screen>,
    cursor: Option<Position>,
    column: u16,
    row: u16,
) -> bool {
    let cell = &buffer[(column, row)];
    UnicodeWidthStr::width(cell.symbol()) == 1
        && is_safe_blank(cell)
        && cursor != Some(Position::new(column, row))
        && !screen
            .and_then(|screen| screen.cell(row - region.y, column - region.x))
            .is_some_and(vt100::Cell::is_wide_continuation)
}

/// A two-column article glyph is committed only with a typed continuation
/// and two safe native destination cells. A scene's typed native glyph uses
/// one safe cell; untyped ordinary scene ink remains Braille-only.
fn paint_region_with_field(
    buffer: &mut Buffer,
    region: Rect,
    screen: Option<&vt100::Screen>,
    foreground: Color,
    mut field: impl FnMut(u16, u16) -> FieldCell,
    allow_text: bool,
) {
    let clipped = region.intersection(buffer.area);
    if clipped.is_empty() {
        return;
    }
    let cursor = screen.and_then(|screen| visible_cursor(screen, region));
    for row in clipped.top()..clipped.bottom() {
        // Article letters and image dots must preserve native word/table spacing.
        // Wikipedia starts outside that span; ordinary scenes keep their texture.
        let native_text_span = {
            let occupied = |column: &u16| !is_inkless_symbol(buffer[(*column, row)].symbol());
            (clipped.left()..clipped.right())
                .find(occupied)
                .zip((clipped.left()..clipped.right()).rfind(occupied))
                .map(|(first, last)| first..=last)
        };
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
            if column < clipped.left() || !safe_target(buffer, region, screen, cursor, column, row)
            {
                continue;
            }
            let (symbol, color, modifier, is_native) =
                match field(column - buffer.area.x, row - buffer.area.y) {
                    FieldCell::Ink {
                        symbol,
                        color,
                        modifier,
                    } => (symbol, color, modifier, false),
                    FieldCell::Native { symbol, color } => {
                        (symbol.to_string(), color, Modifier::empty(), true)
                    }
                    FieldCell::Empty | FieldCell::Continuation => continue,
                };
            let symbol_width = crate::background_animation::wikipedia_symbol_width(&symbol);
            let is_braille = symbol_width == Some(1)
                && symbol.chars().count() == 1
                && symbol
                    .chars()
                    .next()
                    .is_some_and(|character| ('\u{2801}'..='\u{28ff}').contains(&character));
            let is_native_text = is_native
                && symbol_width == Some(1)
                && symbol.chars().count() == 1
                && symbol
                    .chars()
                    .next()
                    .is_some_and(|character| !character.is_control());
            let Some(symbol_width) = symbol_width.filter(|_| {
                if is_native {
                    is_native_text
                } else {
                    allow_text || is_braille
                }
            }) else {
                continue;
            };
            if (allow_text || is_native_text)
                && native_text_span.as_ref().is_some_and(|span| {
                    span.contains(&column)
                        || (symbol_width == 2 && span.contains(&column.saturating_add(1)))
                })
            {
                continue;
            }
            if symbol_width == 2 {
                let next_column = column.saturating_add(1);
                if next_column >= clipped.right()
                    || !matches!(
                        field(next_column - buffer.area.x, row - buffer.area.y),
                        FieldCell::Continuation
                    )
                    || !safe_target(buffer, region, screen, cursor, next_column, row)
                {
                    continue;
                }
            }
            let color = color.unwrap_or(foreground);
            let cell = &mut buffer[(column, row)];
            // Drop the blank's bold/dim/italic so the field keeps one look, and
            // reset an explicit black fill to the default background.
            cell.set_symbol(&symbol).set_fg(color).set_bg(Color::Reset);
            cell.modifier = modifier & (Modifier::BOLD | Modifier::ITALIC);
            if symbol_width == 2 {
                let next = &mut buffer[(column + 1, row)];
                next.set_symbol(" ").set_fg(color).set_bg(Color::Reset);
                next.modifier = Modifier::empty();
                remaining_continuations = 1;
            }
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
    use ratatui::widgets::{Clear, Widget};

    fn paint_all(buffer: &mut Buffer) {
        let area = buffer.area;
        paint_region(buffer, area, None, Color::White, |_, _| '\u{28ff}');
    }

    #[test]
    fn typed_native_scene_digits_paint_safe_blanks_without_replacing_terminal_text() {
        let area = Rect::new(0, 0, 12, 1);
        let mut buffer = Buffer::empty(area);
        buffer[(3, 0)].set_char('A').set_fg(Color::Yellow);
        buffer[(5, 0)].set_char('B').set_fg(Color::Yellow);
        paint_region_with_field(
            &mut buffer,
            area,
            None,
            Color::White,
            |column, _| FieldCell::Native {
                symbol: if column == 1 { '.' } else { '3' },
                color: Some(Color::Green),
            },
            false,
        );
        assert_eq!(buffer[(0, 0)].symbol(), "3");
        assert_eq!(buffer[(1, 0)].symbol(), ".");
        assert_eq!(buffer[(0, 0)].fg, Color::Green);
        assert_eq!(buffer[(3, 0)].symbol(), "A");
        assert_eq!(buffer[(4, 0)].symbol(), " ");
        assert_eq!(buffer[(5, 0)].symbol(), "B");
        assert_eq!(buffer[(6, 0)].symbol(), "3");
    }

    #[test]
    fn typed_native_scene_glyphs_cannot_borrow_article_width_permission() {
        let area = Rect::new(0, 0, 2, 1);
        for allow_text in [false, true] {
            for symbol in ['界', '\n', '\u{1b}'] {
                let mut buffer = Buffer::empty(area);
                paint_region_with_field(
                    &mut buffer,
                    area,
                    None,
                    Color::White,
                    |column, _| {
                        if column == 0 {
                            FieldCell::Native {
                                symbol,
                                color: None,
                            }
                        } else {
                            FieldCell::Continuation
                        }
                    },
                    allow_text,
                );
                assert_eq!(buffer[(0, 0)].symbol(), " ");
                assert_eq!(buffer[(1, 0)].symbol(), " ");
            }
        }
    }

    #[test]
    fn wikipedia_text_styles_only_safe_blanks() {
        let area = Rect::new(0, 0, 5, 1);
        let mut buffer = Buffer::empty(area);
        buffer[(1, 0)].set_char('P').set_fg(Color::Yellow);
        buffer[(2, 0)].set_bg(Color::Blue);
        buffer[(3, 0)].modifier = Modifier::UNDERLINED;
        let original = buffer.clone();
        paint_region_with_style(
            &mut buffer,
            area,
            None,
            Color::White,
            |_, _| 'A',
            |_, _| (Some(Color::Cyan), Modifier::BOLD | Modifier::ITALIC),
            true,
        );
        assert_eq!(buffer[(0, 0)].symbol(), "A");
        assert_eq!(buffer[(0, 0)].fg, Color::Cyan);
        assert_eq!(buffer[(0, 0)].modifier, Modifier::BOLD | Modifier::ITALIC);
        for column in 1..=3 {
            assert_eq!(buffer[(column, 0)], original[(column, 0)]);
        }
    }

    #[test]
    fn article_letters_preserve_native_word_spacing_and_source_bytes() {
        let area = Rect::new(0, 0, 22, 1);
        let mut parser = vt100::Parser::new(1, 22, 0);
        parser.process(b"\x1b[?25l\x1b[1;3H\x1b[3;32mNative two words\x1b[0m");
        let source_before = parser.screen().contents_formatted();
        let mut buffer = Buffer::empty(area);
        buffer.set_string(
            2,
            0,
            "Native two words",
            ratatui::style::Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::ITALIC),
        );
        let native = buffer.clone();
        paint_region_with_field(
            &mut buffer,
            area,
            Some(parser.screen()),
            Color::White,
            |_, _| FieldCell::Ink {
                symbol: "A".into(),
                color: None,
                modifier: Modifier::empty(),
            },
            true,
        );
        for column in 2..18 {
            assert_eq!(
                buffer[(column, 0)],
                native[(column, 0)],
                "native text at {column}"
            );
        }
        assert_eq!(buffer[(0, 0)].symbol(), "A");
        assert_eq!(buffer[(21, 0)].symbol(), "A");
        assert_eq!(parser.screen().contents_formatted(), source_before);

        // Wikipedia image dots also preserve the native word separators.
        let mut braille = native;
        paint_region_with_field(
            &mut braille,
            area,
            Some(parser.screen()),
            Color::White,
            |_, _| FieldCell::Ink {
                symbol: "⣿".into(),
                color: None,
                modifier: Modifier::empty(),
            },
            true,
        );
        assert_eq!(braille[(8, 0)].symbol(), " ");
    }

    #[test]
    fn wikipedia_text_preview_preserves_settings_navigation_spaces() {
        use crate::background_animation::AnimationKind;
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("host".into(), project.path().to_path_buf());
        let area = Rect::new(0, 0, 160, 48);
        app.set_screen_area(area);
        app.animation_settings.kind = AnimationKind::Wikipedia;
        app.animation_settings
            .set_scene_control("wiki_render_mode", ilium_ambient::ControlValue::Index(1))
            .unwrap();
        app.animation_settings.wikipedia.scroll_tenths = 0;
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        });
        // A synthetic long article puts ink behind both chrome and the preview.
        let html = format!("<p>{}</p>", "article words ".repeat(2000));
        let document = std::sync::Arc::new(
            ilium_wikipedia::parse_article(
                "Preview fixture",
                "https://en.wikipedia.org/wiki/Fixture",
                "2026-10-02",
                &html,
            )
            .unwrap(),
        );
        app.animation_frame.inject_wikipedia_document_for_test(
            document,
            &app.animation_settings.wikipedia,
            area.width,
        );
        let geometry = crate::settings_ui::compute_layout(area);
        let mut buffer = Buffer::empty(area);
        buffer.set_string(2, 6, "Agent Cost", ratatui::style::Style::default());
        let before = buffer.clone();
        compose(&mut buffer, &mut app, Duration::ZERO);
        for region in [
            geometry.header_area,
            geometry.tab_list_area,
            geometry.help_rail_area,
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        ] {
            for row in region.top()..region.bottom() {
                for column in region.left()..region.right() {
                    assert_eq!(
                        buffer[(column, row)],
                        before[(column, row)],
                        "chrome at {column},{row}"
                    );
                }
            }
        }
        let panel = crate::animation_settings_ui::layout(geometry.content_area).panel;
        assert!(
            (panel.right()..geometry.content_area.right()).any(|column| {
                (geometry.content_area.top()..geometry.content_area.bottom())
                    .any(|row| buffer[(column, row)].symbol() != " ")
            }),
            "the article remains visible beside the controls"
        );
    }

    #[test]
    fn wide_article_text_requires_its_reserved_second_blank() {
        let area = Rect::new(0, 0, 4, 1);
        let draw = |buffer: &mut Buffer, reserve: bool| {
            paint_region_with_field(
                buffer,
                area,
                None,
                Color::White,
                |column, _| match column {
                    0 => FieldCell::Ink {
                        symbol: "界".into(),
                        color: Some(Color::Cyan),
                        modifier: Modifier::BOLD,
                    },
                    1 if reserve => FieldCell::Continuation,
                    _ => FieldCell::Empty,
                },
                true,
            );
        };
        let mut valid = Buffer::empty(area);
        draw(&mut valid, true);
        assert_eq!(valid[(0, 0)].symbol(), "界");
        assert_eq!(valid[(1, 0)].symbol(), " ");
        assert_eq!(valid[(0, 0)].fg, Color::Cyan);
        assert_eq!(valid[(0, 0)].modifier, Modifier::BOLD);

        let mut unreserved = Buffer::empty(area);
        draw(&mut unreserved, false);
        assert_eq!(unreserved[(0, 0)].symbol(), " ");

        let mut styled = Buffer::empty(area);
        styled[(1, 0)].set_bg(Color::Blue);
        let original = styled.clone();
        draw(&mut styled, true);
        assert_eq!(styled, original);

        let mut boundary = Buffer::empty(area);
        paint_region_with_field(
            &mut boundary,
            Rect::new(0, 0, 1, 1),
            None,
            Color::White,
            |column, _| {
                if column == 0 {
                    FieldCell::Ink {
                        symbol: "界".into(),
                        color: None,
                        modifier: Modifier::empty(),
                    }
                } else {
                    FieldCell::Continuation
                }
            },
            true,
        );
        assert_eq!(boundary[(0, 0)].symbol(), " ");
    }

    #[test]
    fn wide_article_text_preserves_native_cursor_and_continuation() {
        let area = Rect::new(0, 0, 4, 1);
        let mut parser = vt100::Parser::new(1, 4, 0);
        parser.process("\u{1b}[1;2H".as_bytes());
        let mut buffer = Buffer::empty(area);
        paint_region_with_field(
            &mut buffer,
            area,
            Some(parser.screen()),
            Color::White,
            |column, _| match column {
                0 => FieldCell::Ink {
                    symbol: "界".into(),
                    color: None,
                    modifier: Modifier::empty(),
                },
                1 => FieldCell::Continuation,
                _ => FieldCell::Empty,
            },
            true,
        );
        assert_eq!(buffer[(0, 0)].symbol(), " ");
        parser.process("\u{1b}[1;1H界\u{1b}[1;4H".as_bytes());
        let mut buffer = Buffer::empty(area);
        // Mirror the already-rendered native glyph. The clipped region starts
        // after its leading cell, so that cell still reserves its continuation.
        buffer[(0, 0)].set_symbol("界");
        paint_region_with_field(
            &mut buffer,
            Rect::new(1, 0, 3, 1),
            Some(parser.screen()),
            Color::White,
            |_, _| FieldCell::Ink {
                symbol: "A".into(),
                color: None,
                modifier: Modifier::empty(),
            },
            true,
        );
        assert_eq!(buffer[(1, 0)].symbol(), " ");
    }

    #[test]
    fn wikipedia_overlay_preserves_native_cursor_wide_cells_and_screen_bytes() {
        let area = Rect::new(0, 0, 8, 1);
        let mut parser = vt100::Parser::new(1, 8, 0);
        parser.process("界\u{1b}[1;5H".as_bytes());
        let source_before = parser.screen().contents_formatted();
        let mut buffer = Buffer::empty(area);
        buffer[(0, 0)].set_symbol("界");
        let original = buffer.clone();
        paint_region_with_style(
            &mut buffer,
            area,
            Some(parser.screen()),
            Color::White,
            |_, _| 'A',
            |_, _| (None, Modifier::BOLD),
            true,
        );
        for column in [0, 1, 4] {
            assert_eq!(buffer[(column, 0)], original[(column, 0)]);
        }
        assert_eq!(buffer[(2, 0)].symbol(), "A");
        assert_eq!(parser.screen().contents_formatted(), source_before);
    }

    #[test]
    fn article_overlay_rejects_wide_combining_and_control_characters() {
        for character in ['界', '\u{301}', '\n', '\u{1b}', '\u{2800}'] {
            let area = Rect::new(0, 0, 1, 1);
            let mut buffer = Buffer::empty(area);
            paint_region_with_style(
                &mut buffer,
                area,
                None,
                Color::White,
                |_, _| character,
                |_, _| (None, Modifier::empty()),
                true,
            );
            assert_eq!(buffer[(0, 0)].symbol(), " ");
        }
        let area = Rect::new(0, 0, 1, 1);
        let mut buffer = Buffer::empty(area);
        paint_region_with_colors(
            &mut buffer,
            area,
            None,
            Color::White,
            |_, _| 'A',
            |_, _| None,
        );
        assert_eq!(buffer[(0, 0)].symbol(), " ");
    }

    #[test]
    fn protected_cells_retain_symbols_styles_and_diff_controls() {
        let area = Rect::new(0, 0, 8, 1);
        let mut buffer = Buffer::empty(area);
        buffer[(1, 0)].set_symbol("T");
        buffer[(2, 0)].set_bg(Color::Rgb(33, 58, 43));
        buffer[(3, 0)].modifier = Modifier::REVERSED;
        buffer[(4, 0)].modifier = Modifier::UNDERLINED;
        buffer[(5, 0)].set_diff_option(CellDiffOption::Skip);
        buffer[(6, 0)].set_diff_option(CellDiffOption::AlwaysUpdate);
        buffer[(7, 0)].set_bg(Color::Indexed(4));
        let original = buffer.clone();

        paint_all(&mut buffer);

        for column in 1..=7 {
            assert_eq!(buffer[(column, 0)], original[(column, 0)]);
        }
        assert_eq!(buffer[(0, 0)].symbol(), "\u{28ff}");
        assert_eq!(buffer[(0, 0)].fg, Color::White);
        assert_eq!(buffer[(0, 0)].bg, Color::Reset);
    }

    #[test]
    fn inkless_blanks_are_painted_and_restyled_like_void() {
        let area = Rect::new(0, 0, 12, 1);
        let mut buffer = Buffer::empty(area);
        buffer[(0, 0)].set_bg(Color::Black);
        buffer[(1, 0)].set_bg(Color::Indexed(0));
        buffer[(2, 0)].set_bg(Color::Indexed(16));
        buffer[(3, 0)].set_bg(Color::Rgb(0, 0, 0));
        buffer[(4, 0)].modifier = Modifier::BOLD;
        buffer[(5, 0)].modifier = Modifier::DIM;
        buffer[(6, 0)].modifier = Modifier::ITALIC | Modifier::DIM;
        buffer[(7, 0)].set_symbol("\u{a0}");
        buffer[(8, 0)].set_symbol("\u{2800}");
        buffer[(9, 0)].set_symbol("\u{2009}");
        buffer[(10, 0)].set_fg(Color::Cyan);

        paint_all(&mut buffer);

        for column in 0..=10 {
            let cell = &buffer[(column, 0)];
            assert_eq!(cell.symbol(), "\u{28ff}", "column {column}");
            assert_eq!(cell.fg, Color::White, "column {column}");
            assert_eq!(cell.bg, Color::Reset, "column {column}");
            assert!(cell.modifier.is_empty(), "column {column}");
        }
    }

    #[test]
    fn agent_tui_blanks_from_a_real_vt100_screen_are_painted() {
        let mut parser = vt100::Parser::new(3, 20, 0);
        // Dim and bold spaces, NBSP after a prompt glyph, an explicit black
        // fill, then a tinted diff row and an underlined space that must stay.
        parser.process(
            b"\x1b[2m     \x1b[0m\x1b[1m   \x1b[0m\r\n\
              >\xc2\xa0\x1b[40m   \x1b[0m\r\n\
              \x1b[48;2;33;58;43m   \x1b[0m\x1b[4m \x1b[0m",
        );
        let area = Rect::new(0, 0, 20, 3);
        let mut buffer = Buffer::empty(area);
        tui_term::widget::PseudoTerminal::new(parser.screen()).render(area, &mut buffer);
        paint_region(
            &mut buffer,
            area,
            Some(parser.screen()),
            Color::White,
            |_, _| '\u{28ff}',
        );

        for column in 0..8 {
            assert_eq!(
                buffer[(column, 0)].symbol(),
                "\u{28ff}",
                "row 0 col {column}"
            );
        }
        assert_eq!(buffer[(0, 1)].symbol(), ">");
        for column in 1..6 {
            assert_eq!(
                buffer[(column, 1)].symbol(),
                "\u{28ff}",
                "row 1 col {column}"
            );
            assert_eq!(buffer[(column, 1)].bg, Color::Reset);
        }
        for column in 0..4 {
            assert_ne!(
                buffer[(column, 2)].symbol(),
                "\u{28ff}",
                "row 2 col {column}"
            );
        }
        assert_eq!(buffer[(10, 2)].symbol(), "\u{28ff}");
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
            Some(Duration::from_nanos(33_333_334))
        );
        let before_boundary = Duration::from_nanos(33_333_333);
        assert_eq!(quantized_elapsed(before_boundary), Duration::ZERO);
        assert_eq!(
            animation_frame_delay(&app, before_boundary),
            Some(Duration::from_nanos(1))
        );
        let boundary = Duration::from_nanos(33_333_334);
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
            Some(30)
        );
        assert!(animation_frame_delay(&app, Duration::MAX).unwrap() > Duration::ZERO);
        let maximum_sample = quantized_elapsed(Duration::MAX);
        assert_eq!(quantized_elapsed(maximum_sample), maximum_sample);
    }

    #[test]
    fn completed_hillside_cache_keeps_composition_off_the_simulation_path() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("cached".to_owned(), project.path().to_path_buf());
        let area = Rect::new(0, 0, 40, 12);
        app.set_screen_area(area);
        app.animation_settings.enabled = true;
        app.animation_settings.kind = crate::background_animation::AnimationKind::WindyHillside;
        app.animation_settings.loop_seconds = 1;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !app
            .animation_cache
            .borrow_mut()
            .step(&app.animation_settings, 40, 12, 8)
        {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        for index in 0..120 {
            let mut buffer = Buffer::empty(area);
            compose(&mut buffer, &mut app, Duration::from_millis(index * 33));
        }
        assert_eq!(app.animation_frame.geometry_render_count, 0);
        assert_eq!(
            (app.animation_frame.width(), app.animation_frame.height()),
            (40, 12)
        );
        assert!(app.animation_cache.borrow().status().is_ready);
    }

    #[test]
    fn disabled_off_and_opaque_states_add_no_timer_but_preview_remains_live() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("clock".to_owned(), project.path().to_path_buf());
        app.set_screen_area(Rect::new(0, 0, 80, 24));
        assert_eq!(animation_frame_delay(&app, Duration::ZERO), None);
        app.ui_settings.motion_level = MotionLevel::Reduced;
        assert_eq!(animation_frame_bucket(&app, Duration::ZERO), None);
        app.animation_settings.enabled = true;
        app.ui_settings.motion_level = MotionLevel::Off;
        assert!(ambient_is_visible(&app));
        assert_eq!(animation_frame_delay(&app, Duration::ZERO), None);
        app.mode = Mode::Help;
        assert!(!ambient_is_visible(&app));
        assert_eq!(animation_frame_delay(&app, Duration::ZERO), None);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..SettingsState::default()
        });
        app.animation_settings.enabled = false;
        app.ui_settings.motion_level = MotionLevel::Off;
        assert!(
            animation_frame_delay(&app, Duration::ZERO).is_some(),
            "the opened preview stays live even with ambient disabled and Motion Off"
        );
        app.modal_stack.push(Mode::Help);
        assert_eq!(animation_frame_delay(&app, Duration::ZERO), None);
    }

    #[test]
    fn loop_composition_starts_cache_build_while_live_mode_does_not() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("cache".to_owned(), project.path().to_path_buf());
        app.set_screen_area(Rect::new(0, 0, 40, 12));
        app.animation_settings.enabled = true;
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 12));
        compose(&mut buffer, &mut app, Duration::ZERO);
        assert!(app.animation_cache.borrow().status().total_frames > 0);
        app.animation_settings.playback_mode =
            crate::background_animation::AnimationPlaybackMode::Live;
        app.animation_cache = Default::default();
        compose(&mut buffer, &mut app, Duration::ZERO);
        assert_eq!(app.animation_cache.borrow().status().total_frames, 0);
    }

    fn ambient_app() -> (
        App,
        std::sync::Arc<crate::background_animation::test_support::FakeProbe>,
        tempfile::TempDir,
    ) {
        use crate::background_animation::test_support::{fake_host, FakeProbe};
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("host".to_owned(), project.path().to_path_buf());
        app.set_screen_area(Rect::new(0, 0, 40, 12));
        let probe = FakeProbe::new();
        *app.animation_frame.host_mut() = fake_host(&probe);
        app.animation_settings.enabled = true;
        app.animation_settings.kind = crate::background_animation::AnimationKind::Stars;
        app.animation_settings.density_percent = 100;
        (app, probe, project)
    }

    fn compose_at(app: &mut App, seconds: u64) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 12));
        compose(&mut buffer, app, Duration::from_secs(seconds));
        buffer
    }

    fn braille_cells(buffer: &Buffer) -> usize {
        buffer
            .content()
            .iter()
            .filter(|cell| {
                cell.symbol()
                    .chars()
                    .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
            })
            .count()
    }

    #[test]
    fn per_scene_frame_rates_use_exact_absolute_boundaries() {
        for frames_per_second in [1_u32, 2, 5, 12, 24, 30] {
            let fps = u128::from(frames_per_second);
            for second in [0_u64, 1, 2, 59, 3600] {
                let elapsed = Duration::from_secs(second);
                assert_eq!(quantized_elapsed_at(elapsed, frames_per_second), elapsed);
                assert_eq!(
                    elapsed_bucket(elapsed, frames_per_second),
                    u128::from(second) * fps,
                    "{frames_per_second} fps at {second}s"
                );
            }
            // Boundaries are ceil(bucket * 1e9 / fps): no accumulated drift.
            for bucket in [1_u128, 2, 3, 7, fps, fps * 60 + 1] {
                let boundary = (bucket * 1_000_000_000).div_ceil(fps);
                let at = Duration::from_nanos(boundary as u64);
                let before = Duration::from_nanos(boundary as u64 - 1);
                assert_eq!(elapsed_bucket(at, frames_per_second), bucket);
                assert_eq!(elapsed_bucket(before, frames_per_second), bucket - 1);
                assert_eq!(quantized_elapsed_at(at, frames_per_second), at);
                assert!(quantized_elapsed_at(before, frames_per_second) < at);
            }
            let maximum = quantized_elapsed_at(Duration::MAX, frames_per_second);
            assert_eq!(quantized_elapsed_at(maximum, frames_per_second), maximum);
        }
        // The default helper is the 30 fps clock.
        let sample = Duration::from_millis(1234);
        assert_eq!(quantized_elapsed(sample), quantized_elapsed_at(sample, 30));
    }

    #[test]
    fn a_slow_scene_wakes_the_loop_once_per_frame_not_twelve_times_a_second() {
        use crate::background_animation::AnimationKind;
        let (mut app, probe, _project) = ambient_app();
        app.animation_settings.kind = AnimationKind::Stars;
        app.ui_settings.motion_level = MotionLevel::Reduced;
        compose_at(&mut app, 0);
        for (frames_per_second, expected_wakeups) in [(1_u32, 10_u32), (2, 20), (12, 120)] {
            probe
                .fps
                .store(frames_per_second, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(app.animation_frames_per_second(), frames_per_second);
            let mut now = Duration::ZERO;
            let mut wakeups = 0;
            while now < Duration::from_secs(10) {
                let delay = animation_frame_delay(&app, now).unwrap();
                assert!(delay > Duration::ZERO);
                assert!(
                    delay <= Duration::from_secs(1) / frames_per_second + Duration::from_nanos(1)
                );
                now += delay;
                wakeups += 1;
            }
            assert_eq!(wakeups, expected_wakeups, "{frames_per_second} fps");
        }
        probe.fps.store(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            animation_frame_delay(&app, Duration::from_millis(250)),
            Some(Duration::from_millis(750))
        );
        assert_eq!(
            animation_frame_bucket(&app, Duration::from_millis(2999)),
            Some(2)
        );
        assert_eq!(
            animation_frame_bucket(&app, Duration::from_secs(3)),
            Some(3)
        );
        // A hosted scene's rate is clamped to 1..=30.
        probe.fps.store(500, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(app.animation_frames_per_second(), 30);
        probe.fps.store(0, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(app.animation_frames_per_second(), 1);
    }

    #[test]
    fn built_in_scenes_and_unhosted_kinds_keep_the_thirty_fps_clock() {
        let (mut app, probe, _project) = ambient_app();
        probe.fps.store(1, std::sync::atomic::Ordering::SeqCst);
        // No scene is hosted yet: the default cadence applies.
        assert_eq!(app.animation_frames_per_second(), 30);
        compose_at(&mut app, 0);
        assert_eq!(app.animation_frames_per_second(), 1);
        app.animation_settings.kind = crate::background_animation::AnimationKind::Kelp;
        assert_eq!(app.animation_frames_per_second(), 30);
    }

    #[test]
    fn compose_hosts_one_scene_and_reuses_the_render_inside_a_frame_bucket() {
        use std::sync::atomic::Ordering;
        let (mut app, probe, _project) = ambient_app();
        let first = compose_at(&mut app, 3);
        assert!(braille_cells(&first) > 0);
        assert_eq!(probe.constructed.load(Ordering::SeqCst), 1);
        assert_eq!(probe.rendered.load(Ordering::SeqCst), 1);
        compose_at(&mut app, 3);
        assert_eq!(probe.rendered.load(Ordering::SeqCst), 1, "same bucket");
        compose_at(&mut app, 4);
        assert_eq!(probe.rendered.load(Ordering::SeqCst), 2);
        assert_eq!(probe.constructed.load(Ordering::SeqCst), 1);
        assert_eq!(probe.alive(), 1);
    }

    #[test]
    fn a_color_scene_paints_its_own_cell_colors_and_others_use_the_palette() {
        use std::sync::atomic::Ordering;
        let (mut app, probe, _project) = ambient_app();
        let plain = compose_at(&mut app, 0);
        let (red, green, blue) = app.animation_settings.foreground_rgb();
        assert!(plain
            .content()
            .iter()
            .filter(|cell| cell.symbol() != " ")
            .all(|cell| cell.fg == Color::Rgb(red, green, blue)));
        probe.uses_colors.store(true, Ordering::SeqCst);
        let colored = compose_at(&mut app, 1);
        let colors: std::collections::HashSet<_> = colored
            .content()
            .iter()
            .filter(|cell| cell.symbol() != " ")
            .map(|cell| cell.fg)
            .collect();
        assert!(colors.contains(&Color::Rgb(200, 20, 40)), "{colors:?}");
        assert!(colors.contains(&Color::Rgb(20, 40, 200)), "{colors:?}");
    }

    fn ink_colors(buffer: &Buffer) -> Vec<Color> {
        buffer
            .content()
            .iter()
            .filter(|cell| cell.symbol() != " ")
            .map(|cell| cell.fg)
            .collect()
    }

    fn brightness_of(color: Color) -> u32 {
        match color {
            Color::Rgb(red, green, blue) => u32::from(red) + u32::from(green) + u32::from(blue),
            _ => 0,
        }
    }

    #[test]
    fn the_shared_look_recolors_every_scene_and_brightness_dims_it() {
        use ilium_ambient::style::{ColorMode, ColorSource};
        let (mut app, probe, _project) = ambient_app();
        // A scene without its own colors: monotone ink by default.
        let (red, green, blue) = app.animation_settings.foreground_rgb();
        assert!(ink_colors(&compose_at(&mut app, 0))
            .iter()
            .all(|color| *color == Color::Rgb(red, green, blue)));
        // A palette colors it by position, so several colors appear.
        app.animation_settings.appearance.palette = 1;
        app.animation_settings.appearance.source = ColorSource::Horizontal;
        let colored = ink_colors(&compose_at(&mut app, 0));
        let distinct: std::collections::HashSet<_> = colored.iter().copied().collect();
        assert!(distinct.len() > 2, "{distinct:?}");
        // Brightness dims the same picture.
        let bright: u32 = colored.iter().map(|color| brightness_of(*color)).sum();
        app.animation_settings.appearance.brightness_percent = 25;
        let dim: u32 = ink_colors(&compose_at(&mut app, 0))
            .iter()
            .map(|color| brightness_of(*color))
            .sum();
        assert!(dim * 100 < bright * 35, "{dim} vs {bright}");
        // Greyscale has equal channels; monotone has exactly one color.
        app.animation_settings.appearance.brightness_percent = 100;
        app.animation_settings.appearance.mode = ColorMode::Greyscale;
        assert!(ink_colors(&compose_at(&mut app, 0))
            .iter()
            .all(|color| matches!(
                color,
                Color::Rgb(r, g, b) if r == g && g == b
            )));
        app.animation_settings.appearance.mode = ColorMode::Monotone;
        let mono: std::collections::HashSet<_> =
            ink_colors(&compose_at(&mut app, 0)).into_iter().collect();
        assert_eq!(mono.len(), 1);
        // A scene's own colors go through the palette too.
        probe
            .uses_colors
            .store(true, std::sync::atomic::Ordering::SeqCst);
        app.animation_settings.appearance.mode = ColorMode::Color;
        app.animation_settings.appearance.palette = 0;
        let own: std::collections::HashSet<_> =
            ink_colors(&compose_at(&mut app, 1)).into_iter().collect();
        assert!(own.contains(&Color::Rgb(200, 20, 40)));
        app.animation_settings.appearance.palette = 20;
        let recolored: std::collections::HashSet<_> =
            ink_colors(&compose_at(&mut app, 1)).into_iter().collect();
        assert!(
            !recolored.contains(&Color::Rgb(200, 20, 40)),
            "{recolored:?}"
        );
    }

    #[test]
    fn the_panel_choice_limits_where_the_animation_shows() {
        use crate::background_animation::PanelTarget;
        let (mut app, _probe, _project) = ambient_app();
        let both = braille_cells(&compose_at(&mut app, 0));
        app.animation_settings.panels = PanelTarget::Left;
        let left = braille_cells(&compose_at(&mut app, 0));
        app.animation_settings.panels = PanelTarget::Right;
        let right = braille_cells(&compose_at(&mut app, 0));
        assert!(left > 0 && right > 0, "left {left} right {right}");
        assert!(left < both && right < both, "{left} {right} {both}");
        assert_eq!(left + right, both, "the panels do not overlap");
    }

    #[test]
    fn the_hosted_scene_is_rebuilt_on_key_change_and_dropped_when_not_shown() {
        use crate::background_animation::AnimationKind;
        use std::sync::atomic::Ordering;
        let (mut app, probe, _project) = ambient_app();
        compose_at(&mut app, 0);
        assert_eq!(probe.constructed.load(Ordering::SeqCst), 1);
        // Same settings: no rebuild.
        compose_at(&mut app, 1);
        assert_eq!(probe.constructed.load(Ordering::SeqCst), 1);
        // A location change rebuilds a location scene (the crate's scene key).
        app.animation_settings.ambient.location =
            ilium_ambient::GeoLocation::new("Nairobi", -1.29, 36.82);
        compose_at(&mut app, 2);
        assert_eq!(probe.constructed.load(Ordering::SeqCst), 2);
        assert_eq!(
            probe.alive(),
            1,
            "the old scene was dropped before the new one"
        );
        // A different kind rebuilds.
        app.animation_settings.kind = AnimationKind::Spectrum;
        compose_at(&mut app, 3);
        assert_eq!(probe.constructed.load(Ordering::SeqCst), 3);
        assert_eq!(probe.alive(), 1);
        // A built-in scene owns no hosted scene.
        app.animation_settings.kind = AnimationKind::Kelp;
        compose_at(&mut app, 4);
        assert_eq!(probe.alive(), 0);
        // Back to a hosted kind builds again; hiding drops it.
        app.animation_settings.kind = AnimationKind::Spectrum;
        compose_at(&mut app, 5);
        assert_eq!(probe.alive(), 1);
        app.mode = Mode::Help;
        compose_at(&mut app, 6);
        assert_eq!(probe.alive(), 0, "no surface shows the field");
        app.mode = Mode::Normal;
        compose_at(&mut app, 7);
        assert_eq!(probe.alive(), 1);
        app.animation_settings.enabled = false;
        compose_at(&mut app, 8);
        assert_eq!(probe.alive(), 0);
        drop(app);
        assert_eq!(probe.alive(), 0, "exit leaves nothing running");
    }

    #[test]
    fn the_preview_and_the_ambient_background_share_one_scene_instance() {
        use std::sync::atomic::Ordering;
        let (mut app, probe, _project) = ambient_app();
        compose_at(&mut app, 0);
        assert_eq!(probe.constructed.load(Ordering::SeqCst), 1);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..SettingsState::default()
        });
        let preview = compose_at(&mut app, 1);
        assert!(braille_cells(&preview) > 0, "the preview shows the field");
        assert_eq!(
            probe.constructed.load(Ordering::SeqCst),
            1,
            "shared instance"
        );
        assert_eq!(probe.alive(), 1);
        app.mode = Mode::Normal;
        compose_at(&mut app, 2);
        assert_eq!(probe.constructed.load(Ordering::SeqCst), 1);
        // The preview alone (ambient disabled) also hosts exactly one scene.
        app.animation_settings.enabled = false;
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..SettingsState::default()
        });
        compose_at(&mut app, 3);
        assert_eq!(probe.constructed.load(Ordering::SeqCst), 1);
        assert_eq!(probe.alive(), 1);
    }

    #[test]
    fn preview_field_matches_the_screen_and_covers_it_entirely() {
        let (mut app, _probe, _project) = ambient_app();
        app.animation_settings.enabled = false;
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..SettingsState::default()
        });
        let buffer = compose_at(&mut app, 2);
        assert_eq!(
            (app.animation_frame.width(), app.animation_frame.height()),
            (buffer.area.width, buffer.area.height)
        );
        // The fake scene lights the whole top row: the preview reaches every
        // column of the screen, not a smaller rectangle.
        for column in 0..buffer.area.width {
            assert!(
                buffer[(column, 0)].symbol() != " ",
                "column {column} of the top row shows the field"
            );
        }
    }

    #[test]
    fn a_panicking_scene_is_replaced_by_a_message_and_the_client_survives() {
        use std::sync::atomic::Ordering;
        let (mut app, probe, _project) = ambient_app();
        compose_at(&mut app, 0);
        probe.panic_next_render.store(true, Ordering::SeqCst);
        let buffer = compose_at(&mut app, 1);
        assert_eq!(braille_cells(&buffer), 0, "the failed frame is blank");
        let status = app.animation_frame.host().status().unwrap();
        assert!(status.contains("fake scene exploded"), "{status}");
        // Later frames keep running the message scene: no panic, no rebuild loop.
        compose_at(&mut app, 2);
        assert_eq!(probe.constructed.load(Ordering::SeqCst), 1);
        // Changing the settings key starts a fresh scene.
        app.animation_settings.kind = crate::background_animation::AnimationKind::Spectrum;
        let recovered = compose_at(&mut app, 3);
        assert!(braille_cells(&recovered) > 0);
    }

    #[test]
    fn motion_off_freezes_the_ambient_scene_at_time_zero() {
        use std::sync::atomic::Ordering;
        let (mut app, probe, _project) = ambient_app();
        app.ui_settings.motion_level = MotionLevel::Off;
        compose_at(&mut app, 9);
        assert_eq!(probe.last_time_ms.load(Ordering::SeqCst), 0);
        compose_at(&mut app, 20);
        assert_eq!(
            probe.rendered.load(Ordering::SeqCst),
            1,
            "no motion, no re-render"
        );
    }
}
