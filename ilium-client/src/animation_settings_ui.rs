//! Geometry, hit-testing and rendering of the Animations controls panel.
//!
//! The panel is drawn over the full-screen live field (which the compositor
//! paints, see `background_composition::compose`). Render and input share the
//! functions below: every position is derived from the `RowModel`, so a row
//! that appears or disappears moves rendering, keyboard focus, mouse hits,
//! scrolling and the scrollbar together.

use crate::{
    animation_rows::{AnimationRow, RowKind, RowModel, RowView},
    app::{App, SettingsState},
};
use ratatui::{
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Clear, Paragraph, Wrap},
    Frame,
};
use unicode_width::UnicodeWidthStr;

/// Width of the controls panel; the field shows to its right and behind the
/// settings chrome.
pub const PANEL_WIDTH: u16 = 108;
/// Rows above the list (title and a separator).
const HEADER_ROWS: u16 = 2;
/// Rows below the list: two help lines, the status line and the key hint.
const FOOTER_ROWS: u16 = 4;

#[derive(Debug, Clone, Copy)]
pub struct AnimationLayout {
    /// The opaque controls panel, at the left edge of the settings content.
    pub panel: Rect,
    pub scenes: Rect,
    pub controls: Rect,
}

#[derive(Debug, Clone, Copy)]
pub struct SliderGeometry {
    pub label: Rect,
    pub track: Rect,
    pub value: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationHit {
    /// Focus the row (a scene row also selects its scene).
    Select(usize),
    /// Focus the row and run its primary action (toggle, cycle, dialog).
    Activate(usize),
    Slider {
        row: usize,
        value: i32,
    },
    ScrollTo(u16),
    /// The dim "unavailable" marker of a choice row: focus the row and show
    /// the reason, never change the value.
    DisabledOption(usize),
}

pub fn layout(area: Rect) -> AnimationLayout {
    let width = PANEL_WIDTH.min(area.width);
    let scene_width = if width >= 100 {
        32
    } else if width >= 76 {
        26
    } else {
        (width / 3).max(1).min(width)
    };
    let gap = u16::from(width > scene_width);
    AnimationLayout {
        panel: Rect::new(area.x, area.y, width, area.height),
        scenes: Rect::new(area.x, area.y, scene_width, area.height),
        controls: Rect::new(
            area.x + scene_width + gap,
            area.y,
            width.saturating_sub(scene_width + gap),
            area.height,
        ),
    }
}

fn footer_rows(area: Rect, model: &RowModel) -> u16 {
    if area.height < 12 || area.width.saturating_sub(layout(area).panel.width) >= 16 {
        return FOOTER_ROWS;
    }
    let credits = model
        .rows()
        .iter()
        .zip(model.views())
        .find_map(|(row, view)| match (row, &view.kind) {
            (AnimationRow::Scene(kind), RowKind::Scene { is_active: true }) => {
                Some(kind.inspired_by().len())
            }
            _ => None,
        })
        .unwrap_or(0);
    FOOTER_ROWS + if credits > 0 { credits as u16 + 1 } else { 0 }
}

fn visible_rows(area: Rect, model: &RowModel) -> u16 {
    area.height
        .saturating_sub(HEADER_ROWS + footer_rows(area, model))
}

pub fn max_scroll(area: Rect, model: &RowModel) -> u16 {
    model
        .visual_height()
        .saturating_sub(visible_rows(area, model))
}

pub fn scroll_for_selection(area: Rect, model: &RowModel, row: usize, scroll: u16) -> u16 {
    let visible = visible_rows(area, model).max(1);
    let row = model
        .visual_row(row.min(model.len().saturating_sub(1)))
        .unwrap_or(0);
    let scroll = if row < scroll {
        row
    } else if row >= scroll.saturating_add(visible) {
        row + 1 - visible
    } else {
        scroll
    };
    scroll.min(max_scroll(area, model))
}

pub fn row_rect(area: Rect, model: &RowModel, row: usize, scroll: u16) -> Option<Rect> {
    let geometry = layout(area);
    let column = if matches!(model.row(row)?, AnimationRow::Scene(_)) {
        geometry.scenes
    } else {
        geometry.controls
    };
    if column.width <= 1 || visible_rows(area, model) == 0 {
        return None;
    }
    let relative = model.visual_row(row)?.checked_sub(scroll)?;
    if relative >= visible_rows(area, model) {
        return None;
    }
    Some(Rect::new(
        column.x,
        column.y + HEADER_ROWS + relative,
        column.width.saturating_sub(1),
        1,
    ))
}

pub fn row_y(area: Rect, model: &RowModel, row: usize, scroll: u16) -> Option<u16> {
    row_rect(area, model, row, scroll).map(|rectangle| rectangle.y)
}

/// Labels and value readouts have their own rectangles. Only the track changes
/// a numeric value; clicking its label focuses the row without jumping it.
pub fn slider_geometry(
    area: Rect,
    model: &RowModel,
    row: usize,
    scroll: u16,
) -> Option<SliderGeometry> {
    model.view(row)?.slider()?;
    let y = row_y(area, model, row, scroll)?;
    let panel = layout(area).controls;
    let usable = panel.width.saturating_sub(1);
    if usable < 12 {
        return None;
    }
    let value_width = 5;
    let label_width = if usable >= 36 {
        18
    } else {
        usable.saturating_sub(10).min(14)
    };
    let track_width = usable.saturating_sub(label_width + value_width + 2);
    if track_width < 2 {
        return None;
    }
    Some(SliderGeometry {
        label: Rect::new(panel.x, y, label_width, 1),
        track: Rect::new(panel.x + label_width + 1, y, track_width, 1),
        value: Rect::new(panel.right() - 1 - value_width, y, value_width, 1),
    })
}

/// Dragging uses the owned row, independent of the pointer's vertical position.
/// Horizontal motion beyond either endpoint clamps to that endpoint.
pub fn slider_value_at(
    area: Rect,
    model: &RowModel,
    row: usize,
    scroll: u16,
    column: u16,
) -> Option<i32> {
    let geometry = slider_geometry(area, model, row, scroll)?;
    let spec = model.view(row)?.slider()?;
    Some(spec.value_at(
        column.saturating_sub(geometry.track.x),
        geometry.track.width,
    ))
}

pub fn hit(area: Rect, model: &RowModel, scroll: u16, position: Position) -> Option<AnimationHit> {
    let panel = layout(area).panel;
    if !panel.contains(position) {
        return None;
    }
    let relative = position.y.checked_sub(panel.y + HEADER_ROWS)?;
    let visible = visible_rows(area, model);
    let maximum = max_scroll(area, model);
    if position.x == panel.right().saturating_sub(1) && relative < visible && maximum > 0 {
        let target =
            u32::from(maximum) * u32::from(relative) / u32::from(visible.saturating_sub(1).max(1));
        return Some(AnimationHit::ScrollTo(target as u16));
    }
    let row = (0..model.len()).find(|&row| {
        row_rect(area, model, row, scroll).is_some_and(|rect| rect.contains(position))
    })?;
    let panel = row_rect(area, model, row, scroll)?;
    if let Some(geometry) = slider_geometry(area, model, row, scroll) {
        if geometry.track.contains(position) {
            return slider_value_at(area, model, row, scroll, position.x)
                .map(|value| AnimationHit::Slider { row, value });
        }
    }
    let view = model.view(row)?;
    if let Some(marker) = disabled_marker(view, panel.width.saturating_sub(1)) {
        let start = panel.x + marker.offset;
        if (start..start + marker.width).contains(&position.x) {
            return Some(AnimationHit::DisabledOption(row));
        }
    }
    Some(match view.kind {
        RowKind::Choice | RowKind::Toggle | RowKind::Text | RowKind::Location | RowKind::Action => {
            AnimationHit::Activate(row)
        }
        RowKind::Scene { .. } | RowKind::Slider(_) | RowKind::Status => AnimationHit::Select(row),
    })
}

/// Opaque ink for the controls panel. Both schemes name an explicit
/// background so the compositor never reveals the field behind the text.
pub fn control_ink(app: &App) -> Style {
    match app.ui_settings.color_scheme {
        crate::theme::ColorScheme::Light => Style::default().fg(Color::Black).bg(Color::White),
        crate::theme::ColorScheme::Dark => Style::default().fg(Color::White).bg(Color::Black),
    }
}

/// `text` cut to `width` terminal cells, with an ellipsis when shortened.
fn fit(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }
    let mut fitted = String::new();
    let mut used = 0;
    for character in text.chars() {
        let character_width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width + 1 > width {
            break;
        }
        fitted.push(character);
        used += character_width;
    }
    fitted.push('\u{2026}');
    fitted
}

fn draw_slider(
    frame: &mut Frame,
    area: Rect,
    model: &RowModel,
    row: usize,
    view: &RowView,
    scroll: u16,
    style: Style,
) {
    let Some(spec) = view.slider() else {
        return;
    };
    let Some(y) = row_y(area, model, row, scroll) else {
        return;
    };
    let Some(geometry) = slider_geometry(area, model, row, scroll) else {
        let panel = layout(area).controls;
        frame.render_widget(
            Paragraph::new(fit(
                &format!("{} {}", view.label, view.value),
                usize::from(panel.width.saturating_sub(1)),
            ))
            .style(style),
            Rect::new(panel.x, y, panel.width.saturating_sub(1), 1),
        );
        return;
    };
    frame.render_widget(
        Paragraph::new(fit(&view.label, usize::from(geometry.label.width))).style(style),
        geometry.label,
    );
    frame.render_widget(
        Paragraph::new(format!("{:>5}", fit(&view.value, 5))).style(style),
        geometry.value,
    );
    let thumb = spec.thumb_offset(geometry.track.width);
    for offset in 0..geometry.track.width {
        let symbol = if offset == thumb {
            '\u{25cf}'
        } else {
            '\u{2500}'
        };
        frame.buffer_mut()[(geometry.track.x + offset, y)]
            .set_char(symbol)
            .set_style(style);
    }
}

fn draw_scrollbar(frame: &mut Frame, area: Rect, model: &RowModel, scroll: u16, ink: Style) {
    let panel = layout(area).panel;
    let visible = visible_rows(area, model);
    let maximum = max_scroll(area, model);
    if visible == 0 || maximum == 0 || panel.width == 0 {
        return;
    }
    let thumb_size = (u32::from(visible) * u32::from(visible)
        / u32::from(model.visual_height().max(1)))
    .max(1) as u16;
    let travel = visible.saturating_sub(thumb_size);
    let thumb_start =
        (u32::from(scroll.min(maximum)) * u32::from(travel) / u32::from(maximum)) as u16;
    for offset in 0..visible {
        let character = if (thumb_start..thumb_start + thumb_size).contains(&offset) {
            '\u{2503}'
        } else {
            '\u{2502}'
        };
        frame.buffer_mut()[(panel.right() - 1, panel.y + HEADER_ROWS + offset)]
            .set_char(character)
            .set_style(ink.add_modifier(Modifier::DIM));
    }
}

/// Widest label column of a value row.
const LABEL_COLUMN_WIDTH: usize = 18;

/// Where a choice row draws its dim marker for disabled options.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DisabledMarker {
    /// Columns from the row's left edge to the marker.
    offset: u16,
    width: u16,
    text: String,
    /// Label column width this row needs so the marker fits.
    label_width: usize,
    value_width: usize,
}

/// The `[ value ]` text of a choice, toggle, text or action row.
fn bracketed_value(view: &RowView) -> String {
    match view.kind {
        RowKind::Choice | RowKind::Toggle | RowKind::Text | RowKind::Action => {
            format!("[ {} ]", view.value)
        }
        _ => view.value.clone(),
    }
}

/// The dim "GPU (unavailable)" marker of a choice row, placed after the
/// current value. The wording shortens ("GPU (n/a)", "GPU \u{2717}") and the
/// label column narrows until it fits in `row_width`; `None` when even the
/// shortest form does not (the popover and help line still explain it).
fn disabled_marker(view: &RowView, row_width: u16) -> Option<DisabledMarker> {
    if view.disabled_options.is_empty() || !matches!(view.kind, RowKind::Choice) {
        return None;
    }
    let row_width = usize::from(row_width);
    let value_width = UnicodeWidthStr::width(bracketed_value(view).as_str());
    let forms: [fn(&str) -> String; 3] = [
        |label| format!("{label} (unavailable)"),
        |label| format!("{label} (n/a)"),
        |label| format!("{label} \u{2717}"),
    ];
    let default_label_width = LABEL_COLUMN_WIDTH.min(row_width / 2);
    let narrowed = (UnicodeWidthStr::width(view.label.as_str()) + 1).min(default_label_width);
    for form in forms {
        let text = view
            .disabled_options
            .iter()
            .map(|option| form(&option.label))
            .collect::<Vec<_>>()
            .join(" ");
        let width = UnicodeWidthStr::width(text.as_str());
        for label_width in [default_label_width, narrowed] {
            let offset = label_width + 1 + value_width + 1;
            if offset + width <= row_width {
                return Some(DisabledMarker {
                    offset: offset as u16,
                    width: width as u16,
                    text,
                    label_width,
                    value_width,
                });
            }
        }
    }
    // Two-column compact layouts keep the unavailable choice discoverable.
    // The selected row's footer carries the unabridged current value/reason.
    let label_width = UnicodeWidthStr::width(view.label.as_str()).min(row_width / 3);
    let text = view
        .disabled_options
        .iter()
        .map(|option| format!("{} \u{2717}", option.label))
        .collect::<Vec<_>>()
        .join(" ");
    let width = UnicodeWidthStr::width(text.as_str());
    let value_width = row_width.saturating_sub(label_width + width + 2);
    if value_width >= 4 {
        return Some(DisabledMarker {
            offset: (label_width + value_width + 2) as u16,
            width: width as u16,
            text,
            label_width,
            value_width,
        });
    }
    None
}

/// Text of one non-slider, non-scene row: label column then value.
fn draw_value_row(frame: &mut Frame, row_area: Rect, view: &RowView, style: Style) {
    let marker = disabled_marker(view, row_area.width);
    let label_width = marker.as_ref().map_or_else(
        || LABEL_COLUMN_WIDTH.min(usize::from(row_area.width) / 2),
        |marker| marker.label_width,
    );
    let value_width = marker.as_ref().map_or_else(
        || usize::from(row_area.width).saturating_sub(label_width + 1),
        |marker| marker.value_width,
    );
    let value = bracketed_value(view);
    let text = format!(
        "{:<label_width$} {}",
        fit(&view.label, label_width),
        fit(&value, value_width)
    );
    frame.render_widget(Paragraph::new(text).style(style), row_area);
    if let Some(marker) = marker {
        frame.render_widget(
            Paragraph::new(marker.text).style(style.add_modifier(Modifier::DIM)),
            Rect::new(row_area.x + marker.offset, row_area.y, marker.width, 1),
        );
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App, state: &SettingsState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let panel = layout(area).panel;
    let ink = control_ink(app);
    let model = app.animation_row_model();
    frame.render_widget(Clear, panel);
    frame.render_widget(Block::default().style(ink), panel);
    frame.render_widget(
        Paragraph::new("Scenes \u{2014} select to preview").style(ink.add_modifier(Modifier::BOLD)),
        Rect::new(panel.x, panel.y, panel.width, 1),
    );
    let controls = layout(area).controls;
    frame.render_widget(
        Paragraph::new("Settings").style(ink.add_modifier(Modifier::BOLD)),
        Rect::new(controls.x, controls.y, controls.width, 1),
    );
    let mut previous_section = "";
    for row in crate::background_animation::AnimationKind::ALL.len()..model.len() {
        let section = model.section(row);
        if section == previous_section {
            continue;
        }
        previous_section = section;
        if let Some(relative) = model
            .visual_row(row)
            .and_then(|offset| offset.checked_sub(1))
            .and_then(|offset| offset.checked_sub(state.scroll))
        {
            if relative < visible_rows(area, &model) {
                frame.render_widget(
                    Paragraph::new(fit(
                        &format!("─ {section} ─"),
                        usize::from(controls.width.saturating_sub(1)),
                    ))
                    .style(ink.add_modifier(Modifier::BOLD | Modifier::DIM)),
                    Rect::new(
                        controls.x,
                        controls.y + HEADER_ROWS + relative,
                        controls.width.saturating_sub(1),
                        1,
                    ),
                );
            }
        }
    }
    for (row, view) in model.views().iter().enumerate() {
        let Some(row_area) = row_rect(area, &model, row, state.scroll) else {
            continue;
        };
        let style = if row == state.selected_row {
            ink.add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            ink
        };
        frame.render_widget(
            Paragraph::new(" ".repeat(usize::from(row_area.width))).style(style),
            row_area,
        );
        match &view.kind {
            RowKind::Scene { is_active } => {
                frame.render_widget(
                    Paragraph::new(fit(
                        &format!(
                            "{} {:2}. {}",
                            if *is_active { "\u{25cf}" } else { " " },
                            row + 1,
                            view.label
                        ),
                        usize::from(row_area.width),
                    ))
                    .style(style),
                    row_area,
                );
            }
            RowKind::Slider(_) => {
                draw_slider(frame, area, &model, row, view, state.scroll, style);
            }
            _ => draw_value_row(frame, row_area, view, style),
        }
    }
    draw_scrollbar(frame, area, &model, state.scroll, ink);
    let footer_height = footer_rows(area, &model);
    if panel.height <= HEADER_ROWS + footer_height {
        return;
    }
    let footer_top = panel.bottom() - footer_height;
    let help = model
        .view(state.selected_row)
        .map_or_else(String::new, |view| match view.slider() {
            Some(spec) => format!(
                "{}: {} ({}..{}). {}",
                view.label, view.value, spec.minimum, spec.maximum, view.help
            ),
            None => format!("{}: {}. {}", view.label, view.value, view.help),
        });
    frame.render_widget(
        Paragraph::new(help).style(ink).wrap(Wrap { trim: true }),
        Rect::new(panel.x, footer_top, panel.width, 2),
    );
    frame.render_widget(
        Paragraph::new(fit(
            app.status_message
                .as_deref()
                .unwrap_or("Saved automatically for this project."),
            usize::from(panel.width),
        ))
        .style(ink),
        Rect::new(panel.x, footer_top + 2, panel.width, 1),
    );
    frame.render_widget(
        Paragraph::new(fit(
            "\u{2191}\u{2193} row \u{b7} \u{2190}\u{2192} adjust \u{b7} Enter set \u{b7} f full screen",
            usize::from(panel.width),
        ))
        .style(ink.add_modifier(Modifier::DIM)),
        Rect::new(panel.x, footer_top + 3, panel.width, 1),
    );
    render_inspired_by(
        frame,
        area,
        app,
        if footer_height > FOOTER_ROWS {
            0
        } else {
            panel.width
        },
    );
}

/// The one line shown over the field while the controls are hidden.
pub fn render_fullscreen_hint(frame: &mut Frame, area: Rect, app: &App) {
    // The bottom row belongs to the persistent voice affordance.
    if area.height < 2 || area.width == 0 {
        return;
    }
    let text = fit(
        " Full screen preview \u{b7} any key or click returns ",
        usize::from(area.width),
    );
    let width = UnicodeWidthStr::width(text.as_str()) as u16;
    frame.render_widget(
        Paragraph::new(text).style(control_ink(app).add_modifier(Modifier::DIM)),
        Rect::new(area.x, area.bottom() - 2, width.min(area.width), 1),
    );
}

/// Bottom-right credit for the selected scene's inspiration. Part of the
/// demo only: the Animations settings tab draws it, real use never does.
/// `left_inset` keeps it clear of the controls panel.
pub fn render_inspired_by(frame: &mut Frame, area: Rect, app: &App, left_inset: u16) {
    let urls = app.animation_settings.kind.inspired_by();
    // The bottom row belongs to the persistent voice affordance.
    if urls.is_empty() || area.height < 2 {
        return;
    }
    let free_width = area.width.saturating_sub(left_inset);
    if free_width < 16 {
        return;
    }
    // One line per URL, growing upward from the row above the voice
    // affordance, so every credited source stays whole when there is room.
    for (index, url) in urls.iter().enumerate() {
        let line = if index == 0 {
            format!("\u{ab}inspired by {url}\u{bb}")
        } else {
            format!("\u{ab}and {url}\u{bb}")
        };
        let Some(y) = (area.bottom() - 2).checked_sub((urls.len() - 1 - index) as u16) else {
            continue;
        };
        if y < area.y {
            continue;
        }
        let text = fit(&line, usize::from(free_width));
        let width = (UnicodeWidthStr::width(text.as_str()) as u16).min(free_width);
        frame.render_widget(
            Paragraph::new(text).style(control_ink(app).add_modifier(Modifier::DIM)),
            Rect::new(area.right() - width, y, width, 1),
        );
    }
}

/// The Animations row whose help topic anchors are shown, by row index.
pub fn help_id_for_row(model: &RowModel, app: &App, row: usize) -> Option<String> {
    let scene_controls = app.animation_settings.scene_controls();
    model
        .row(row)
        .map(|animation_row: &AnimationRow| animation_row.help_id(&scene_controls))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation_rows::{rows, AnimationRow, RowContext};
    use crate::app::{Mode, SettingsTab};
    use crate::background_animation::test_support::{fake_host, FakeProbe};
    use crate::background_animation::{AnimationKind, AnimationPlaybackMode, AnimationSettings};
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ilium_ambient::{Control, ControlValue};
    use ratatui::{backend::TestBackend, Terminal};
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    #[test]
    fn overhaul_scene_and_controls_use_separate_columns_with_wide_tracks() {
        let settings = AnimationSettings::default();
        let model = RowModel::new(&settings, &RowContext::default());
        let area = Rect::new(0, 0, 120, 40);
        let scene_y = row_y(area, &model, 0, 0).unwrap();
        let control = model.first_control_index();
        let control_y = row_y(area, &model, control, 0).unwrap();
        assert!(control_y < scene_y + AnimationKind::ALL.len() as u16);
        let slider = model
            .rows()
            .iter()
            .position(|row| *row == AnimationRow::Common("speed"))
            .unwrap();
        let geometry = slider_geometry(area, &model, slider, 0).unwrap();
        assert!(
            geometry.track.width >= 32,
            "track should double its old width: {geometry:?}"
        );
        assert!(geometry.label.x >= 24, "controls must be beside scenes");
    }

    fn key(app: &mut App, code: KeyCode) {
        crate::keys::handle_event(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn pointer(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
        crate::mouse::handle_mouse_event(
            app,
            MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
        );
    }

    fn rendered_dots(terminal: &Terminal<TestBackend>, rectangle: Rect) -> Vec<(String, Color)> {
        (rectangle.top()..rectangle.bottom())
            .flat_map(|y| (rectangle.left()..rectangle.right()).map(move |x| (x, y)))
            .filter_map(|(x, y)| {
                let cell = &terminal.backend().buffer()[(x, y)];
                cell.symbol()
                    .chars()
                    .any(|glyph| ('\u{2801}'..='\u{28ff}').contains(&glyph))
                    .then(|| (cell.symbol().to_owned(), cell.fg))
            })
            .collect()
    }

    /// An app on Settings -> Animations at `width` x `height`, hosting a fake
    /// scene so hosted kinds need no network, ffmpeg or audio device.
    fn settings_app(width: u16, height: u16) -> (App, Arc<FakeProbe>, tempfile::TempDir) {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.set_screen_area(Rect::new(0, 0, width, height));
        let probe = FakeProbe::new();
        *app.animation_frame.host_mut() = fake_host(&probe);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        });
        (app, probe, project)
    }

    fn draw(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| crate::ui::draw(frame, app)).unwrap();
        terminal
    }

    fn content_area(app: &App) -> Rect {
        crate::settings_ui::compute_layout(app.layout.screen_area).content_area
    }

    fn row_index(app: &App, wanted: &AnimationRow) -> usize {
        app.animation_row_model()
            .rows()
            .iter()
            .position(|row| row == wanted)
            .unwrap_or_else(|| panic!("row {wanted:?} is not in the list"))
    }

    fn selected_row(app: &App) -> usize {
        let Mode::Settings(state) = &app.mode else {
            panic!("Settings stays open");
        };
        state.selected_row
    }

    fn set_selected_row(app: &mut App, row: usize) {
        let content = content_area(app);
        let model = app.animation_row_model();
        let Mode::Settings(state) = &mut app.mode else {
            panic!("Settings stays open");
        };
        state.selected_row = row;
        state.scroll = scroll_for_selection(content, &model, row, state.scroll);
    }

    /// Presses Up/Down until `target` is the selected row.
    fn press_until_row(app: &mut App, target: usize) {
        for _ in 0..200 {
            let current = selected_row(app);
            if current == target {
                return;
            }
            key(
                app,
                if current < target {
                    KeyCode::Down
                } else {
                    KeyCode::Up
                },
            );
        }
        panic!("could not reach row {target}");
    }

    fn text_control(value: &str) -> Control {
        Control::text(
            "fake_path",
            "Image folder",
            value,
            "path or URL",
            "Where the images come from.",
        )
    }

    #[test]
    fn row_lists_follow_the_selected_scene() {
        let context = RowContext::default();
        for kind in AnimationKind::ALL {
            let settings = AnimationSettings {
                kind,
                ..Default::default()
            };
            let list = rows(&settings, &context);
            assert_eq!(
                &list[..AnimationKind::ALL.len()],
                AnimationKind::ALL
                    .iter()
                    .copied()
                    .map(AnimationRow::Scene)
                    .collect::<Vec<_>>()
                    .as_slice(),
                "scenes come first, in catalog order"
            );
            assert_eq!(
                list[AnimationKind::ALL.len()],
                AnimationRow::Common("background")
            );
            assert_eq!(list.last(), Some(&AnimationRow::FullScreenPreview));
            for id in [
                "speed",
                "density",
                "dither",
                "lightness",
                "hue",
                "saturation",
            ] {
                assert!(
                    list.contains(&AnimationRow::Common(id)),
                    "{kind:?} shows {id}"
                );
            }
            let controls = settings.scene_controls();
            let control_rows = list
                .iter()
                .filter(|row| matches!(row, AnimationRow::SceneControl(_)))
                .count();
            assert_eq!(control_rows, controls.len(), "{kind:?}");
            if !kind.is_ambient() {
                // The shoreline lists its style choice and, in the default Rich
                // style, fifteen more sliders after the four named ones.
                let expected = if kind == AnimationKind::Shoreline {
                    4 + 1 + 15
                } else if kind == AnimationKind::QuietPond {
                    5
                } else {
                    4
                };
                assert_eq!(
                    control_rows, expected,
                    "legacy scenes keep four named sliders"
                );
                assert!(list.contains(&AnimationRow::Common("playback")));
                assert!(list.contains(&AnimationRow::Common("loop_seconds")));
                assert!(list.contains(&AnimationRow::CacheStatus));
                assert!(!list.contains(&AnimationRow::SceneStatus));
                assert!(!list.contains(&AnimationRow::Location));
            } else {
                for hidden in [
                    AnimationRow::Common("playback"),
                    AnimationRow::Common("loop_seconds"),
                    AnimationRow::CacheStatus,
                ] {
                    assert!(!list.contains(&hidden), "{kind:?} is live-only");
                }
                assert!(list.contains(&AnimationRow::SceneStatus));
                assert_eq!(
                    list.contains(&AnimationRow::Location),
                    kind.ambient().unwrap().uses_location(),
                    "{kind:?}"
                );
            }
        }
    }

    #[test]
    fn dependent_rows_hide_and_show() {
        let mut settings = AnimationSettings::default();
        let context = RowContext::default();
        assert!(rows(&settings, &context).contains(&AnimationRow::CacheStatus));
        settings.playback_mode = AnimationPlaybackMode::Live;
        let live = rows(&settings, &context);
        assert!(live.contains(&AnimationRow::Common("playback")));
        assert!(!live.contains(&AnimationRow::Common("loop_seconds")));
        assert!(!live.contains(&AnimationRow::CacheStatus));
        // A scene that paints its own colors hides the palette rows.
        let colored = RowContext {
            scene_uses_cell_colors: true,
            ..Default::default()
        };
        let list = rows(&settings, &colored);
        for id in ["lightness", "hue", "saturation"] {
            assert!(!list.contains(&AnimationRow::Common(id)), "{id} hides");
        }
        assert!(list.contains(&AnimationRow::Common("speed")));
        // Location only for observer-aware scenes.
        let stars = AnimationSettings {
            kind: AnimationKind::Stars,
            ..Default::default()
        };
        assert!(rows(&stars, &context).contains(&AnimationRow::Location));
        let video = AnimationSettings {
            kind: AnimationKind::Video,
            ..Default::default()
        };
        assert!(!rows(&video, &context).contains(&AnimationRow::Location));
    }

    #[test]
    fn dependent_rows_follow_the_hosted_scene_through_the_app() {
        let (mut app, probe, _project) = settings_app(140, 40);
        app.animation_settings.kind = AnimationKind::Images;
        draw(&mut app, 140, 40);
        assert!(app
            .animation_row_model()
            .rows()
            .contains(&AnimationRow::Common("lightness")));
        probe.uses_colors.store(true, Ordering::SeqCst);
        // The scene reports colors once it renders its next frame.
        app.started_at -= std::time::Duration::from_secs(1);
        draw(&mut app, 140, 40);
        assert!(!app
            .animation_row_model()
            .rows()
            .contains(&AnimationRow::Common("lightness")));
        assert!(app
            .animation_row_model()
            .rows()
            .contains(&AnimationRow::SceneStatus));
        *probe.status.lock().unwrap() = Some("Downloading 40%".to_owned());
        let view = app
            .animation_row_model()
            .view(row_index(&app, &AnimationRow::SceneStatus))
            .cloned()
            .unwrap();
        assert_eq!(view.value, "Downloading 40%");
    }

    #[test]
    fn real_render_keeps_every_row_reachable_over_a_full_screen_field() {
        for (width, height) in [(80, 24), (100, 30), (140, 40)] {
            let (mut app, _probe, _project) = settings_app(width, height);
            let area = content_area(&app);
            let model = app.animation_row_model();
            let count = model.len();
            let panel = layout(area).panel;
            for row in 0..count {
                let scroll = scroll_for_selection(area, &model, row, 0);
                app.mode = Mode::Settings(SettingsState {
                    tab: SettingsTab::Animations,
                    selected_row: row,
                    scroll,
                    ..Default::default()
                });
                let y = row_y(area, &model, row, scroll).expect("row visible after scrolling");
                let terminal = draw(&mut app, width, height);
                let row_area = row_rect(area, &model, row, scroll).unwrap();
                assert!(terminal.backend().buffer()[(row_area.x, y)]
                    .modifier
                    .contains(Modifier::REVERSED));
                let model = app.animation_row_model();
                if let Some(geometry) = slider_geometry(area, &model, row, scroll) {
                    assert!(geometry.track.width >= 2);
                    assert!((geometry.track.x..geometry.track.right()).any(|x| terminal
                        .backend()
                        .buffer()[(x, y)]
                        .symbol()
                        == "\u{25cf}"));
                }
            }
            // The field is the whole screen: same dimensions, and dots appear
            // outside the (opaque) panel.
            assert_eq!(
                (app.animation_frame.width(), app.animation_frame.height()),
                (width, height)
            );
            let terminal = draw(&mut app, width, height);
            let right_of_panel = Rect::new(
                panel.right(),
                0,
                width.saturating_sub(panel.right()),
                height,
            );
            assert!(!rendered_dots(&terminal, right_of_panel).is_empty());
            assert!(
                rendered_dots(&terminal, panel).is_empty(),
                "the panel is opaque"
            );
        }
    }

    #[test]
    fn preview_palette_matches_the_shared_hsl_for_both_canvases() {
        for scheme in [
            crate::theme::ColorScheme::Dark,
            crate::theme::ColorScheme::Light,
        ] {
            let (mut app, _probe, _project) = settings_app(140, 40);
            app.ui_settings.color_scheme = scheme;
            for (lightness, hue, saturation, expected) in [
                (60, 210, 0, Color::Rgb(153, 153, 153)),
                (50, 120, 100, Color::Rgb(0, 255, 0)),
            ] {
                app.animation_settings.lightness_percent = lightness;
                app.animation_settings.hue_degrees = hue;
                app.animation_settings.saturation_percent = saturation;
                app.animation_cache = Default::default();
                let terminal = draw(&mut app, 140, 40);
                let dots = rendered_dots(&terminal, Rect::new(0, 0, 140, 40));
                assert!(!dots.is_empty());
                assert!(dots.iter().all(|(_, ink)| *ink == expected));
            }
        }
    }

    #[test]
    fn a_color_scene_preview_paints_colored_braille_cells() {
        let (mut app, probe, _project) = settings_app(140, 40);
        probe.uses_colors.store(true, Ordering::SeqCst);
        app.animation_settings.kind = AnimationKind::Images;
        app.animation_settings.density_percent = 100;
        let terminal = draw(&mut app, 140, 40);
        let colors = rendered_dots(&terminal, Rect::new(0, 0, 140, 40))
            .into_iter()
            .map(|(_, color)| color)
            .collect::<std::collections::HashSet<_>>();
        assert!(colors.contains(&Color::Rgb(200, 20, 40)), "{colors:?}");
        assert!(colors.contains(&Color::Rgb(20, 40, 200)), "{colors:?}");
    }

    #[test]
    fn keyboard_reaches_all_scenes_and_all_lower_controls_with_narrow_scrolling() {
        let (mut app, _probe, project) = settings_app(80, 24);
        let area = content_area(&app);
        let scene_count = AnimationKind::ALL.len();
        for scene in 0..scene_count {
            if scene > 0 {
                key(&mut app, KeyCode::Down);
            }
            assert_eq!(app.animation_settings.kind, AnimationKind::ALL[scene]);
            assert_eq!(selected_row(&app), scene);
        }
        // The last scene (Solar system) is a live-only hosted scene: no playback rows.
        assert_eq!(app.animation_settings.kind, AnimationKind::SolarSystem);
        let model = app.animation_row_model();
        let count = model.len();
        for row in scene_count..count {
            key(&mut app, KeyCode::Down);
            let Mode::Settings(state) = &app.mode else {
                panic!("Settings stays open");
            };
            assert_eq!(state.selected_row, row);
            assert!(
                row_y(area, &model, row, state.scroll).is_some(),
                "row {row} remains visible"
            );
        }
        let Mode::Settings(state) = &app.mode else {
            panic!("Settings stays open");
        };
        assert!(state.scroll > 0, "80x24 needs scrolling");
        // A built-in scene's slider persists through the same path.
        app.settings_adjust_animation_row(9, 1);
        assert_eq!(app.animation_settings.kind, AnimationKind::QuietPond);
        let before = app.animation_settings.quiet_pond.drift_percent;
        let drift = row_index(&app, &AnimationRow::SceneControl("scene_control_3"));
        app.settings_set_animation_slider(drift, 175);
        assert!(app.animation_settings.quiet_pond.drift_percent > before);
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation,
            app.animation_settings
        );
    }

    #[test]
    fn keyboard_and_mouse_select_the_same_persisted_scene() {
        let (mut app, _probe, project) = settings_app(140, 40);
        key(&mut app, KeyCode::Down);
        assert_eq!(app.animation_settings.kind, AnimationKind::MoonlitWater);
        let area = content_area(&app);
        let model = app.animation_row_model();
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            area.x,
            row_y(area, &model, 5, 0).unwrap(),
        );
        assert_eq!(app.animation_settings.kind, AnimationKind::Kelp);
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation
                .kind,
            AnimationKind::Kelp
        );
    }

    #[test]
    fn actual_rendered_tracks_accept_exact_endpoints_and_labels_only_focus() {
        for (width, height) in [(80, 24), (140, 40)] {
            let (mut app, _probe, project) = settings_app(width, height);
            let area = content_area(&app);
            let slider_rows: Vec<usize> = app
                .animation_row_model()
                .views()
                .iter()
                .enumerate()
                .filter(|(_, view)| view.slider().is_some())
                .map(|(row, _)| row)
                .collect();
            assert!(slider_rows.len() >= 8, "four scene + four shared sliders");
            for row in slider_rows {
                let model = app.animation_row_model();
                let scroll = scroll_for_selection(area, &model, row, 0);
                set_selected_row(&mut app, row);
                let geometry = slider_geometry(area, &model, row, scroll).unwrap();
                let spec = model.view(row).unwrap().slider().unwrap();
                pointer(
                    &mut app,
                    MouseEventKind::Down(MouseButton::Left),
                    geometry.label.x,
                    geometry.label.y,
                );
                let after_label = app
                    .animation_row_model()
                    .view(row)
                    .unwrap()
                    .slider()
                    .unwrap();
                assert_eq!(after_label.value, spec.value, "label must not jump");
                for (column, expected) in [
                    (geometry.track.x, spec.minimum),
                    (geometry.track.right() - 1, spec.maximum),
                ] {
                    pointer(
                        &mut app,
                        MouseEventKind::Down(MouseButton::Left),
                        column,
                        geometry.track.y,
                    );
                    let value = app
                        .animation_row_model()
                        .view(row)
                        .unwrap()
                        .slider()
                        .unwrap()
                        .value;
                    assert_eq!(value, expected, "row {row} endpoint");
                    pointer(
                        &mut app,
                        MouseEventKind::Up(MouseButton::Left),
                        column,
                        geometry.track.y,
                    );
                    assert_eq!(
                        crate::project_config::load(project.path())
                            .unwrap()
                            .animation,
                        app.animation_settings
                    );
                }
            }
        }
    }

    #[test]
    fn owned_slider_drag_clamps_outside_the_track_and_releases_before_global_voice_control() {
        let (mut app, _probe, _project) = settings_app(140, 40);
        let area = content_area(&app);
        let lightness = row_index(&app, &AnimationRow::Common("lightness"));
        let model = app.animation_row_model();
        let scroll = scroll_for_selection(area, &model, lightness, 0);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            selected_row: lightness,
            scroll,
            ..Default::default()
        });
        let model = app.animation_row_model();
        let geometry = slider_geometry(area, &model, lightness, scroll).unwrap();
        let voice_enabled = app.voice_settings.enabled;
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            geometry.track.x,
            geometry.track.y,
        );
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 139, 39);
        assert_eq!(app.animation_settings.lightness_percent, 100);
        let voice_area = app.layout.voice_control_area;
        assert!(voice_area.width > 0 && voice_area.height > 0);
        pointer(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            voice_area.x,
            voice_area.y,
        );
        let Mode::Settings(state) = &app.mode else {
            panic!("owned release stays in Settings");
        };
        assert_eq!(state.animation_slider_drag, None);
        assert_eq!(app.voice_settings.enabled, voice_enabled);
        pointer(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            geometry.track.x,
            geometry.track.y,
        );
        assert_eq!(
            app.animation_settings.lightness_percent, 100,
            "unowned drag is inert"
        );
    }

    #[test]
    fn palette_and_named_controls_survive_scene_switches_and_failed_saves() {
        let (mut app, _probe, project) = settings_app(140, 40);
        let row = |app: &App, id: &'static str| row_index(app, &AnimationRow::Common(id));
        let lightness = row(&app, "lightness");
        let hue = row(&app, "hue");
        let saturation = row(&app, "saturation");
        app.settings_set_animation_slider(lightness, 35);
        app.settings_set_animation_slider(hue, 125);
        app.settings_set_animation_slider(saturation, 50);
        app.settings_adjust_animation_row(5, 1);
        let first_control = app.animation_row_model().first_control_index();
        app.settings_set_animation_slider(first_control, 175);
        app.settings_adjust_animation_row(1, 1);
        app.settings_set_animation_slider(first_control, 35);
        app.settings_adjust_animation_row(5, 1);
        assert_eq!(app.animation_settings.kelp.plant_density_percent, 175);
        assert_eq!(
            app.animation_settings.moonlit_water.wave_strength_percent,
            35
        );
        assert_eq!(
            (
                app.animation_settings.lightness_percent,
                app.animation_settings.hue_degrees,
                app.animation_settings.saturation_percent
            ),
            (35, 125, 50)
        );
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation,
            app.animation_settings
        );
        let before = app.animation_settings.clone();
        std::fs::remove_file(project.path().join(".ilium/config.yaml")).unwrap();
        std::fs::create_dir(project.path().join(".ilium/config.yaml")).unwrap();
        // The scene was switched above, and scenes list different row counts.
        let lightness = row(&app, "lightness");
        app.settings_set_animation_slider(lightness, 80);
        assert_eq!(app.animation_settings, before);
        assert!(app
            .status_message
            .as_deref()
            .unwrap()
            .contains("Could not save"));
    }

    #[test]
    fn actual_keyboard_changes_keep_palette_and_named_values_after_scene_switches() {
        let (mut app, _probe, project) = settings_app(80, 24);
        for _ in 0..5 {
            key(&mut app, KeyCode::Down);
        }
        assert_eq!(app.animation_settings.kind, AnimationKind::Kelp);
        key(&mut app, KeyCode::Enter);
        let first_control = app.animation_row_model().first_control_index();
        assert_eq!(
            selected_row(&app),
            first_control,
            "Enter focuses scene controls"
        );
        assert_eq!(app.animation_settings.kind, AnimationKind::Kelp);
        key(&mut app, KeyCode::Right);
        assert_eq!(app.animation_settings.kelp.plant_density_percent, 105);
        for common in ["lightness", "hue", "saturation"] {
            let target = row_index(&app, &AnimationRow::Common(common));
            press_until_row(&mut app, target);
            key(&mut app, KeyCode::Right);
        }
        assert_eq!(
            (
                app.animation_settings.lightness_percent,
                app.animation_settings.hue_degrees,
                app.animation_settings.saturation_percent
            ),
            (61, 211, 1)
        );
        press_until_row(&mut app, 1);
        assert_eq!(app.animation_settings.kind, AnimationKind::MoonlitWater);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Right);
        assert_eq!(
            app.animation_settings.moonlit_water.wave_strength_percent,
            105
        );
        press_until_row(&mut app, 5);
        assert_eq!(app.animation_settings.kind, AnimationKind::Kelp);
        assert_eq!(app.animation_settings.kelp.plant_density_percent, 105);
        let reloaded = crate::project_config::load(project.path())
            .unwrap()
            .animation;
        assert_eq!(reloaded, app.animation_settings);
        assert_eq!(
            (
                reloaded.lightness_percent,
                reloaded.hue_degrees,
                reloaded.saturation_percent
            ),
            (61, 211, 1)
        );
        assert_eq!(reloaded.moonlit_water.wave_strength_percent, 105);
    }

    #[test]
    fn choice_toggle_and_action_rows_activate_from_keyboard_and_mouse() {
        // Tall enough to show every row of the default (Rich shoreline) list.
        let (mut app, _probe, project) = settings_app(140, 80);
        let area = content_area(&app);
        let background = row_index(&app, &AnimationRow::Common("background"));
        set_selected_row(&mut app, background);
        key(&mut app, KeyCode::Enter);
        assert!(app.animation_settings.enabled);
        key(&mut app, KeyCode::Left);
        assert!(!app.animation_settings.enabled);
        let dither = row_index(&app, &AnimationRow::Common("dither"));
        let model = app.animation_row_model();
        let row_area = row_rect(area, &model, dither, 0).unwrap();
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            row_area.x + 2,
            row_area.y,
        );
        assert_eq!(
            app.animation_settings.dither,
            crate::background_animation::DitherMode::Stippled
        );
        let playback = row_index(&app, &AnimationRow::Common("playback"));
        set_selected_row(&mut app, playback);
        key(&mut app, KeyCode::Char(' '));
        assert_eq!(
            app.animation_settings.playback_mode,
            AnimationPlaybackMode::Live
        );
        // Live playback removes the dependent loop rows.
        assert!(!app
            .animation_row_model()
            .rows()
            .contains(&AnimationRow::CacheStatus));
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation,
            app.animation_settings
        );
    }

    #[test]
    fn a_text_row_opens_a_prefilled_prompt_and_applies_through_set_control() {
        let (mut app, _probe, _project) = settings_app(140, 40);
        crate::background_animation::test_controls::install(vec![text_control("~/Pictures")]);
        let text_row = row_index(&app, &AnimationRow::SceneControl("fake_path"));
        set_selected_row(&mut app, text_row);
        key(&mut app, KeyCode::Enter);
        let Mode::AnimationTextPrompt(target, state) = &app.mode else {
            panic!("Enter on a text row opens the shared prompt");
        };
        assert_eq!(target.control, "fake_path");
        assert_eq!(target.hint, "path or URL");
        assert_eq!(state.buf, "~/Pictures", "prefilled with the current value");
        assert_eq!(app.modal_stack.len(), 1, "stacked over Settings");
        // Rejected value: the message shows in the prompt and it stays open.
        for _ in 0.."~/Pictures".len() {
            key(&mut app, KeyCode::Backspace);
        }
        for character in "bad".chars() {
            key(&mut app, KeyCode::Char(character));
        }
        key(&mut app, KeyCode::Enter);
        let Mode::AnimationTextPrompt(target, state) = &app.mode else {
            panic!("a rejected value keeps the prompt open");
        };
        assert_eq!(target.error.as_deref(), Some("That value is not accepted"));
        assert_eq!(state.buf, "bad");
        assert_eq!(
            app.status_message.as_deref(),
            Some("That value is not accepted")
        );
        // Accepted value: applied, prompt closed, Settings restored.
        for _ in 0..3 {
            key(&mut app, KeyCode::Backspace);
        }
        for character in "/tmp/wallpapers".chars() {
            key(&mut app, KeyCode::Char(character));
        }
        key(&mut app, KeyCode::Enter);
        assert!(matches!(app.mode, Mode::Settings(_)));
        assert!(app.modal_stack.is_empty());
        assert_eq!(
            app.animation_settings
                .scene_control("fake_path")
                .unwrap()
                .value,
            ControlValue::Text("/tmp/wallpapers".to_owned())
        );
        // Esc cancels without applying anything.
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('x'));
        key(&mut app, KeyCode::Esc);
        assert!(matches!(app.mode, Mode::Settings(_)));
        assert_eq!(
            app.animation_settings
                .scene_control("fake_path")
                .unwrap()
                .value,
            ControlValue::Text("/tmp/wallpapers".to_owned())
        );
    }

    #[test]
    fn a_failed_control_edit_shows_the_message_and_changes_nothing() {
        let (mut app, _probe, project) = settings_app(140, 40);
        crate::background_animation::test_controls::install(vec![text_control("keep")]);
        let before = app.animation_settings.clone();
        assert_eq!(
            app.apply_animation_text_input("fake_path", "bad".to_owned()),
            Err("That value is not accepted".to_owned())
        );
        assert_eq!(app.animation_settings, before);
        assert_eq!(
            app.status_message.as_deref(),
            Some("That value is not accepted")
        );
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation,
            AnimationSettings::default(),
            "nothing was persisted"
        );
    }

    /// The screen row carrying the credit, if any.
    fn credit_row(terminal: &Terminal<TestBackend>, width: u16, height: u16) -> Option<String> {
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .find(|row| row.contains("inspired by"))
    }

    #[test]
    fn inspired_by_credit_shows_bottom_right_in_the_demo_only_for_credited_scenes() {
        let (mut app, _probe, _project) = settings_app(120, 36);
        app.animation_settings.kind = AnimationKind::DitherWater;
        let windowed = draw(&mut app, 120, 36);
        let credit = credit_row(&windowed, 120, 36).expect("credit is drawn in the demo");
        assert!(credit.contains("\u{ab}inspired by https://www.reddit.com/"));
        let panel = layout(content_area(&app)).panel;
        let model = app.animation_row_model();
        assert!(
            footer_rows(content_area(&app), &model) > FOOTER_ROWS,
            "compact credits get reserved footer rows below the controls"
        );
        let credit_y = content_area(&app).bottom() - 2;
        assert!(model.rows().iter().enumerate().all(|(row, _)| row_y(
            content_area(&app),
            &model,
            row,
            0
        )
        .is_none_or(|y| y < credit_y)));
        assert!(panel.contains(Position::new(panel.x, credit_y)));
        key(&mut app, KeyCode::Char('f'));
        let full = draw(&mut app, 120, 36);
        assert!(
            credit_row(&full, 120, 36).is_some_and(|row| row.trim_end().ends_with(
                "https://www.reddit.com/r/PixelArt/comments/1sqw4hf/1bit_water_animation/\u{bb}"
            )),
            "the whole URL fits full screen"
        );
        // Scenes with no recorded inspiration show no credit.
        app.animation_settings.kind = AnimationKind::Shoreline;
        let plain = draw(&mut app, 120, 36);
        assert!(credit_row(&plain, 120, 36).is_none());
    }

    #[test]
    fn inspired_by_credit_never_appears_outside_the_settings_demo() {
        let (mut app, _probe, _project) = settings_app(120, 36);
        app.animation_settings.kind = AnimationKind::DitherWater;
        app.mode = Mode::Normal;
        let terminal = draw(&mut app, 120, 36);
        for y in 0..36 {
            let row: String = (0..120)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_owned())
                .collect();
            assert!(!row.contains("inspired by"), "row {y}: {row:?}");
        }
    }

    #[test]
    fn full_screen_preview_hides_the_controls_and_shares_the_screen_geometry() {
        let (mut app, _probe, _project) = settings_app(120, 36);
        let normal = draw(&mut app, 120, 36);
        let panel = layout(content_area(&app)).panel;
        assert!(rendered_dots(&normal, panel).is_empty());
        // Same field dimensions and dots for the panel view and full screen.
        assert_eq!(
            (app.animation_frame.width(), app.animation_frame.height()),
            (120, 36)
        );
        key(&mut app, KeyCode::Char('f'));
        let Mode::Settings(state) = &app.mode else {
            panic!("Settings stays open");
        };
        assert!(state.animation_fullscreen);
        let full = draw(&mut app, 120, 36);
        assert_eq!(
            (app.animation_frame.width(), app.animation_frame.height()),
            (120, 36)
        );
        assert!(
            !rendered_dots(&full, panel).is_empty(),
            "the field fills the region the panel covered"
        );
        let hint = (0..120)
            .map(|x| full.backend().buffer()[(x, 34)].symbol().to_owned())
            .collect::<String>();
        assert!(hint.contains("any key or click returns"), "{hint:?}");
        // Any key returns and is consumed (Esc must not close Settings).
        key(&mut app, KeyCode::Esc);
        let Mode::Settings(state) = &app.mode else {
            panic!("the returning key must not close Settings");
        };
        assert!(!state.animation_fullscreen);
        // The clickable row enters it and a click returns.
        let full_row = row_index(&app, &AnimationRow::FullScreenPreview);
        let area = content_area(&app);
        let model = app.animation_row_model();
        let scroll = scroll_for_selection(area, &model, full_row, 0);
        set_selected_row(&mut app, full_row);
        let row_area = row_rect(area, &model, full_row, scroll).unwrap();
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            row_area.x + 3,
            row_area.y,
        );
        let Mode::Settings(state) = &app.mode else {
            panic!("Settings stays open");
        };
        assert!(state.animation_fullscreen);
        pointer(&mut app, MouseEventKind::Down(MouseButton::Left), 60, 20);
        let Mode::Settings(state) = &app.mode else {
            panic!("Settings stays open");
        };
        assert!(!state.animation_fullscreen);
    }

    #[test]
    fn preview_advances_while_disabled_or_off_and_help_covers_it() {
        let (mut app, _probe, _project) = settings_app(140, 40);
        app.ui_settings.motion_level = crate::config::MotionLevel::Off;
        let first = draw(&mut app, 140, 40).backend().buffer().clone();
        app.started_at = std::time::Instant::now() - std::time::Duration::from_secs(5);
        let second = draw(&mut app, 140, 40).backend().buffer().clone();
        assert_ne!(first, second);
        assert!(!app.animation_settings.enabled);
        assert!(app.is_animation_preview_visible());
        let parent = std::mem::replace(&mut app.mode, Mode::Normal);
        app.push_modal_over(
            parent,
            Mode::SettingsHelp(crate::settings_help::dialog::SettingsHelpState::new(
                "AN-01",
                1,
                crate::config::MotionLevel::Off,
            )),
        );
        assert!(!app.is_animation_preview_visible());
    }

    #[test]
    fn row_and_hit_geometry_never_escape_short_or_scrolled_controls() {
        let (app, _probe, _project) = settings_app(100, 30);
        let model = app.animation_row_model();
        for (width, height) in [(100, 30), (60, 24), (20, 8), (5, 3), (0, 0)] {
            let area = Rect::new(3, 4, width, height);
            for row in 0..model.len() {
                let scroll = scroll_for_selection(area, &model, row, 0);
                if let Some(y) = row_y(area, &model, row, scroll) {
                    assert!(layout(area).panel.contains(Position::new(area.x, y)));
                    let expected = match model.view(row).unwrap().kind {
                        RowKind::Choice
                        | RowKind::Toggle
                        | RowKind::Text
                        | RowKind::Location
                        | RowKind::Action => AnimationHit::Activate(row),
                        _ => AnimationHit::Select(row),
                    };
                    assert_eq!(
                        hit(
                            area,
                            &model,
                            scroll,
                            Position::new(row_rect(area, &model, row, scroll).unwrap().x, y)
                        ),
                        Some(expected)
                    );
                }
            }
        }
    }

    #[test]
    fn scroll_and_scrollbar_derive_from_the_row_count() {
        let area = Rect::new(0, 0, 60, 20);
        let model = RowModel::new(&AnimationSettings::default(), &RowContext::default());
        assert_eq!(
            max_scroll(area, &model),
            model
                .visual_height()
                .saturating_sub(visible_rows(area, &model))
        );
        let far = scroll_for_selection(area, &model, model.len() - 1, 0);
        assert!(row_y(area, &model, model.len() - 1, far).is_some());
        assert_eq!(scroll_for_selection(area, &model, 0, far), 0);
    }
}
