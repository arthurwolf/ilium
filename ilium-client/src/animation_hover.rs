//! The hover popover of Settings -> Animations that explains why a choice
//! option (for example the GPU renderer) is disabled.
//!
//! The mouse path only records which row the pointer rests on; the client
//! tick reveals the popover once the pointer has been still for
//! `HOVER_DELAY`, the same split the context-menu submenu hover uses. Any key
//! or click, leaving the row, scrolling and leaving Settings dismiss it.
//! `popover_geometry` is a pure function of the screen, the controls panel
//! and the row, so the popover can be tested for staying on screen.

use crate::{
    animation_rows::{DisabledOption, RowModel},
    animation_settings_ui::{control_ink, layout, row_y, Scrolls},
    app::{App, Mode, SettingsTab},
    last_prompt_banner::wrap_lines,
};
use ratatui::{
    layout::Rect,
    style::Modifier,
    widgets::{Clear, Paragraph},
    Frame,
};
use std::time::{Duration, Instant};

/// How long the pointer must rest on the row before the popover appears.
pub const HOVER_DELAY: Duration = Duration::from_millis(150);
/// Preferred and minimum popover widths, border included.
const MAXIMUM_WIDTH: u16 = 46;
const MINIMUM_SIDE_WIDTH: u16 = 24;
/// A popover needs a border, one text row and a border.
const MINIMUM_HEIGHT: u16 = 3;

/// The row the pointer rests on and whether the delay has elapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationHover {
    pub row: usize,
    pub since: Instant,
    pub is_shown: bool,
}

/// Where the popover goes and the wrapped text lines it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopoverGeometry {
    pub rectangle: Rect,
    pub lines: Vec<String>,
}

/// The popover title and body for the disabled options of one row.
pub fn popover_text(options: &[DisabledOption]) -> (String, String) {
    match options {
        [] => (String::new(), String::new()),
        [only] => (format!(" {} unavailable ", only.label), only.reason.clone()),
        many => (
            " Unavailable options ".to_owned(),
            many.iter()
                .map(|option| format!("{}: {}", option.label, option.reason))
                .collect::<Vec<_>>()
                .join("\n\n"),
        ),
    }
}

/// Places the popover for the row drawn at `anchor_y`. It goes to the right
/// of the controls `panel` (over the field) when that is wide enough, so it
/// never covers the row; otherwise above or below the row inside the panel
/// columns. The result always lies inside `area` and never includes the last
/// row, which belongs to the voice affordance. `None` when nothing fits.
pub fn popover_geometry(
    area: Rect,
    panel: Rect,
    anchor_y: u16,
    body: &str,
) -> Option<PopoverGeometry> {
    let usable_bottom = if area.height >= 3 {
        area.bottom() - 1
    } else {
        area.bottom()
    };
    let usable_height = usable_bottom.saturating_sub(area.y);
    if usable_height < MINIMUM_HEIGHT || area.width < 4 {
        return None;
    }
    let wrapped = |width: u16| wrap_lines(body, width.saturating_sub(2).max(1));

    // Beside the panel: the row stays fully visible.
    let side_x = panel.right().saturating_add(1);
    let side_room = area.right().saturating_sub(side_x);
    if side_room >= MINIMUM_SIDE_WIDTH {
        let width = side_room.min(MAXIMUM_WIDTH);
        let mut lines = wrapped(width);
        let height = (lines.len() as u16 + 2).min(usable_height);
        lines.truncate(usize::from(height - 2));
        let y = anchor_y
            .min(usable_bottom.saturating_sub(height))
            .max(area.y);
        return Some(PopoverGeometry {
            rectangle: Rect::new(side_x, y, width, height),
            lines,
        });
    }

    // Narrow screens: over the panel, on whichever side of the row has room.
    let width = panel.width.min(area.width).max(4);
    let x = panel.x.max(area.x);
    let space_below = usable_bottom.saturating_sub(anchor_y.saturating_add(1));
    let space_above = anchor_y.saturating_sub(area.y);
    let mut lines = wrapped(width);
    let wanted = lines.len() as u16 + 2;
    let is_below = if wanted <= space_below {
        true
    } else if wanted <= space_above {
        false
    } else {
        space_below >= space_above
    };
    let space = if is_below { space_below } else { space_above };
    if space < MINIMUM_HEIGHT {
        return None;
    }
    let height = wanted.min(space);
    lines.truncate(usize::from(height - 2));
    if lines.len() < wrapped(width).len() {
        if let Some(last) = lines.last_mut() {
            last.push('\u{2026}');
        }
    }
    let y = if is_below {
        anchor_y + 1
    } else {
        anchor_y - height
    };
    Some(PopoverGeometry {
        rectangle: Rect::new(x, y, width.min(area.right() - x), height),
        lines,
    })
}

/// Draws the popover of the hovered row, if it is shown and still valid.
pub fn render(frame: &mut Frame, area: Rect, app: &App, model: &RowModel, scroll: Scrolls) {
    let Some(hover) = app.animation_hover.filter(|hover| hover.is_shown) else {
        return;
    };
    let Some(view) = model.view(hover.row) else {
        return;
    };
    if view.disabled_options.is_empty() {
        return;
    }
    let Some(anchor_y) = row_y(area, model, hover.row, scroll) else {
        return;
    };
    let (title, body) = popover_text(&view.disabled_options);
    let Some(geometry) = popover_geometry(area, layout(area).panel, anchor_y, &body) else {
        return;
    };
    let ink = control_ink(app);
    frame.render_widget(Clear, geometry.rectangle);
    let block = crate::theme::block(false)
        .title(title)
        .style(ink)
        .border_style(ink.add_modifier(Modifier::DIM));
    frame.render_widget(
        Paragraph::new(geometry.lines.join("\n"))
            .style(ink)
            .block(block),
        geometry.rectangle,
    );
}

impl App {
    /// Records where the pointer rests: `Some(row)` for a row with disabled
    /// options, `None` anywhere else. Resting on the same row keeps the
    /// original timestamp so the delay is measured from first arrival.
    pub fn set_animation_hover(&mut self, row: Option<usize>, now: Instant) {
        self.animation_hover = match (row, self.animation_hover) {
            (Some(row), Some(hover)) if hover.row == row => Some(hover),
            (Some(row), _) => Some(AnimationHover {
                row,
                since: now,
                is_shown: false,
            }),
            (None, _) => None,
        };
    }

    /// Dismisses the popover (any key or click, leaving the row).
    pub fn clear_animation_hover(&mut self) {
        self.animation_hover = None;
    }

    /// Reveals the popover after the pointer has rested long enough and
    /// drops it when Settings -> Animations is no longer on screen. Returns
    /// whether the picture changed.
    pub fn tick_animation_hover(&mut self, now: Instant) -> bool {
        let Some(mut hover) = self.animation_hover else {
            return false;
        };
        let is_on_screen = matches!(
            &self.mode,
            Mode::Settings(state)
                if state.tab == SettingsTab::Animations && !state.animation_fullscreen
        );
        if !is_on_screen {
            self.animation_hover = None;
            return hover.is_shown;
        }
        if hover.is_shown || now.saturating_duration_since(hover.since) < HOVER_DELAY {
            return false;
        }
        hover.is_shown = true;
        self.animation_hover = Some(hover);
        true
    }

    pub fn set_plugin_issue_hover(&mut self, row: Option<usize>, now: Instant) {
        self.plugin_issue_hover = match (row, self.plugin_issue_hover) {
            (Some(row), Some(hover)) if hover.row == row => Some(hover),
            (Some(row), _) => Some(AnimationHover {
                row,
                since: now,
                is_shown: false,
            }),
            (None, _) => None,
        };
    }

    pub fn clear_plugin_issue_hover(&mut self) {
        self.plugin_issue_hover = None;
    }

    pub fn tick_plugin_issue_hover(&mut self, now: Instant) -> bool {
        let Some(mut hover) = self.plugin_issue_hover else {
            return false;
        };
        let is_on_screen = matches!(
            &self.mode,
            Mode::Settings(state)
                if state.tab == SettingsTab::Animations
                    && !state.animation_fullscreen
                    && state.animation_source_tab == crate::animation_plugins::AnimationSourceTab::Plugin
        );
        if !is_on_screen {
            self.plugin_issue_hover = None;
            return hover.is_shown;
        }
        if hover.is_shown || now.saturating_duration_since(hover.since) < HOVER_DELAY {
            return false;
        }
        hover.is_shown = true;
        self.plugin_issue_hover = Some(hover);
        true
    }
}
