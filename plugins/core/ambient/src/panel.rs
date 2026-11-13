//! Geometry and hit-testing of the Animations controls panel, as pure data.
//!
//! Ported from the non-drawing half of `ilium-client/src/animation_settings_ui.rs`. Two columns:
//! the LEFT column holds a compact windowed list of the scenes with a Prev/Next line, and under it
//! the global settings every scene shares; the RIGHT column holds the selected scene's own
//! settings. Each of the three regions scrolls on its own, so a screen position is resolved
//! through a [`Scrolls`] triple. Rendering, keyboard focus, hits and scrolling all derive from the
//! same [`RowModel`], so a row that appears or disappears moves them together.
//!
//! Pure: rectangles in, rectangles and actions out. Drawing and key routing stay with the client.

use crate::hover::{wrap_lines, Rect};
use crate::rows::{AnimationRow, Region, RowKind, RowModel, RowView};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub use crate::hover::PANEL_WIDTH;
/// Rows above each list: the column heading.
pub const HEADER_ROWS: u16 = 1;
/// Rows of the selected scene's introduction, between the heading and the controls list.
pub const INTRODUCTION_ROWS: u16 = 4;
/// Default rows below the lists: two help lines, the status line and the key hint.
pub const FOOTER_ROWS: u16 = 4;
/// Maximum live preparation-log rows that may displace the settings list.
pub const MAX_ACTIVITY_VIEW_ROWS: u16 = 8;
/// Selected-control context stays visible above the saved-scene status and log.
pub const PINNED_CONTROL_HELP_ROWS: u16 = 2;
/// Rows of the Prev/Next button line under the scene list.
pub const NAV_ROWS: u16 = 1;
/// Widest button of the Prev/Next line, in cells.
pub const NAV_BUTTON_WIDTH: u16 = 8;
/// Label column of value rows in the narrower left column.
pub const GLOBAL_LABEL_WIDTH: usize = 13;
/// Label column of value rows in the right column.
pub const LABEL_COLUMN_WIDTH: usize = 18;

/// A screen cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub x: u16,
    pub y: u16,
}

impl Position {
    pub fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }
}

fn contains(rectangle: Rect, position: Position) -> bool {
    rectangle.contains(position.x, position.y)
}

/// The scroll offset of each region, in region display rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scrolls {
    pub scenes: u16,
    pub global: u16,
    pub controls: u16,
}

impl Scrolls {
    pub fn get(self, region: Region) -> u16 {
        match region {
            Region::Scenes => self.scenes,
            Region::Global => self.global,
            Region::Controls => self.controls,
        }
    }

    pub fn set(&mut self, region: Region, value: u16) {
        match region {
            Region::Scenes => self.scenes = value,
            Region::Global => self.global = value,
            Region::Controls => self.controls = value,
        }
    }
}

/// Column rectangles and row windows of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationLayout {
    pub panel: Rect,
    /// The whole left column (scene list and global settings).
    pub scenes: Rect,
    /// The whole right column (the selected scene's settings).
    pub controls: Rect,
    pub scene_heading: Rect,
    /// The visible window of the scene list (scrollbar column excluded).
    pub scene_list: Rect,
    /// The Prev/Next button line.
    pub scene_nav: Rect,
    pub global_heading: Rect,
    pub global: Rect,
    pub controls_heading: Rect,
    pub controls_intro: Rect,
    pub controls_rows: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliderGeometry {
    pub label: Rect,
    pub track: Rect,
    pub value: Rect,
}

/// What a click or press on the panel means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationHit {
    /// Focus the row (a scene row also selects its scene).
    Select(usize),
    /// Focus the row and run its primary action (toggle, dialog).
    Activate(usize),
    Slider {
        row: usize,
        value: i32,
    },
    /// A click on a region's scrollbar: scroll that region to the offset.
    ScrollTo(Region, u16),
    /// The dim "unavailable" marker of a choice row: focus the row and show the reason.
    DisabledOption(usize),
    /// The Prev button: select the previous scene, wrapping from the first to the last.
    PreviousScene,
    /// The Next button: select the next scene, wrapping from the last to the first.
    NextScene,
}

/// Width of the left column, scrollbar and help-rail columns included.
pub fn left_width(panel_width: u16) -> u16 {
    if panel_width >= 100 {
        46
    } else if panel_width >= 76 {
        34
    } else {
        (panel_width / 2).max(1).min(panel_width)
    }
}

/// Column rectangles and row windows for a footer of `footer` rows.
pub fn layout_with_footer(area: Rect, footer: u16) -> AnimationLayout {
    let width = PANEL_WIDTH.min(area.width);
    let left = left_width(width);
    let gap = u16::from(width > left);
    let available = area.height.saturating_sub(footer);
    // Left column: heading, scene window, nav line, heading, global window.
    let remaining = available.saturating_sub(2 * HEADER_ROWS + NAV_ROWS);
    let scene_rows = (remaining / 3).clamp(3, 8).min(remaining);
    let global_rows = remaining - scene_rows;
    let list_width = left.saturating_sub(2);
    let controls_x = area.x + left + gap;
    let controls_width = width.saturating_sub(left + gap);
    let intro_rows = INTRODUCTION_ROWS.min(available.saturating_sub(HEADER_ROWS));
    let scene_y = area.y + HEADER_ROWS;
    let nav_y = scene_y + scene_rows;
    let global_heading_y = nav_y + NAV_ROWS;
    AnimationLayout {
        panel: Rect::new(area.x, area.y, width, area.height),
        scenes: Rect::new(area.x, area.y, left, area.height),
        controls: Rect::new(controls_x, area.y, controls_width, area.height),
        scene_heading: Rect::new(area.x, area.y, left.saturating_sub(1), 1.min(available)),
        scene_list: Rect::new(area.x, scene_y, list_width, scene_rows),
        scene_nav: Rect::new(area.x, nav_y, list_width, u16::from(available > 3)),
        global_heading: Rect::new(
            area.x,
            global_heading_y,
            left.saturating_sub(1),
            u16::from(available > 3),
        ),
        global: Rect::new(
            area.x,
            global_heading_y + HEADER_ROWS,
            list_width,
            global_rows,
        ),
        controls_heading: Rect::new(
            controls_x,
            area.y,
            controls_width.saturating_sub(1),
            1.min(available),
        ),
        controls_intro: Rect::new(
            controls_x,
            area.y + HEADER_ROWS,
            controls_width.saturating_sub(1),
            intro_rows,
        ),
        controls_rows: Rect::new(
            controls_x,
            area.y + HEADER_ROWS + intro_rows,
            controls_width.saturating_sub(1),
            available.saturating_sub(HEADER_ROWS + intro_rows),
        ),
    }
}

/// Column rectangles with the default footer height.
pub fn layout(area: Rect) -> AnimationLayout {
    layout_with_footer(area, FOOTER_ROWS)
}

/// `text` cut to `width` terminal cells, with an ellipsis when shortened.
pub fn fit(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }
    let mut fitted = String::new();
    let mut used = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width + 1 > width {
            break;
        }
        fitted.push(character);
        used += character_width;
    }
    fitted.push('\u{2026}');
    fitted
}

/// Rows `text` needs when word-wrapped to `width` cells.
pub fn wrapped_rows(text: &str, width: u16) -> u16 {
    u16::try_from(wrap_lines(text, width.max(1)).len()).unwrap_or(u16::MAX)
}

/// The scene status report, when the status row carries more than one line.
pub fn saved_scene_activity_report(model: &RowModel) -> Option<String> {
    let row = model
        .rows()
        .iter()
        .position(|row| *row == AnimationRow::SceneStatus)?;
    let view = model.view(row)?;
    (view.help.lines().count() > 1)
        .then(|| format!("{}: {}\n{}", view.label, view.value, view.help))
}

/// Splits a status report into its pinned summary and its scrolling activity log.
pub fn saved_scene_activity_sections(report: &str) -> (String, String) {
    let mut progress = None;
    let mut elapsed = None;
    let mut stage_eta = None;
    let mut total_eta = None;
    let mut route_eta = None;
    let mut current_work = None;
    let mut activity = Vec::new();
    let mut in_activity = false;

    for line in report.lines() {
        if let Some(first_event) = line.strip_prefix("Recent activity:") {
            in_activity = true;
            let first_event = first_event.trim();
            if !first_event.is_empty() {
                activity.push(first_event);
            }
            continue;
        }
        if in_activity {
            activity.push(line);
        } else if line.starts_with("Scene Status:") || line.starts_with("Scene status:") {
            progress = Some(line);
        } else if line.starts_with("Elapsed:") {
            elapsed = Some(line);
        } else if line.starts_with("ETA:") {
            stage_eta = Some(line);
        } else if line.starts_with("Route ETA:") {
            route_eta = Some(line.split(';').next().unwrap_or(line));
        } else if line.starts_with("Now:") {
            current_work = Some(line);
        } else if line.starts_with("Total ETA:") {
            total_eta = Some(line);
        }
    }

    let current_work_and_route_eta = match (current_work, route_eta) {
        (Some(work), Some(route)) => Some(format!("{work}; {route}")),
        (Some(work), None) => Some(work.to_owned()),
        (None, Some(route)) => Some(route.to_owned()),
        (None, None) => None,
    };

    (
        [
            progress,
            current_work_and_route_eta.as_deref(),
            total_eta,
            elapsed,
            stage_eta,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n"),
        activity.join("\n"),
    )
}

/// One status line with the route ETA appended, fitted to `width`.
pub fn compact_saved_scene_status(summary: &str, width: usize) -> Option<String> {
    let progress = summary
        .lines()
        .find_map(|line| {
            line.strip_prefix("Scene Status:")
                .or_else(|| line.strip_prefix("Scene status:"))
        })?
        .trim()
        .replace("Saved worlds ", "Saved ")
        .replace(" phase ", " ");
    let route_eta = summary.lines().find_map(|line| {
        line.split_once("Route ETA:")
            .map(|(_, value)| value.split(';').next().unwrap_or(value).trim())
    });
    let status = route_eta.map_or(progress.clone(), |eta| {
        format!("{progress} · Route ETA: {eta}")
    });
    Some(fit(&status, width))
}

/// `(summary rows, activity rows)` the footer gives the saved-scene report.
pub fn activity_footer_rows(
    detail_rows: u16,
    width: u16,
    summary: &str,
    activity: &str,
) -> (u16, u16) {
    let available_rows = detail_rows.saturating_sub(pinned_control_help_rows(detail_rows));
    let total_activity_rows = wrapped_rows(activity, width);
    let reserved_activity_rows = u16::from(total_activity_rows > 0 && available_rows >= 3);
    let summary_rows =
        wrapped_rows(summary, width).min(available_rows.saturating_sub(reserved_activity_rows));
    (summary_rows, available_rows.saturating_sub(summary_rows))
}

pub fn pinned_control_help_rows(detail_rows: u16) -> u16 {
    detail_rows.saturating_sub(2).min(PINNED_CONTROL_HELP_ROWS)
}

/// The help line of the selected row: label, value, range for numbers, then its help text.
pub fn selected_control_help(model: &RowModel, selected_row: usize) -> String {
    model
        .view(selected_row)
        .map_or_else(String::new, |view| match view.number() {
            Some((minimum, maximum, _, _)) => format!(
                "{}: {} ({}..{}). {}",
                view.label, view.value, minimum, maximum, view.help
            ),
            None => format!("{}: {}. {}", view.label, view.value, view.help),
        })
}

/// The scene before or after `current` in list order, wrapping at both ends. An unknown id counts
/// as the first scene.
pub fn adjacent_scene<'a>(scenes: &'a [String], current: &str, delta: i32) -> Option<&'a str> {
    let count = i32::try_from(scenes.len())
        .ok()
        .filter(|count| *count > 0)?;
    let index = scenes
        .iter()
        .position(|candidate| candidate == current)
        .unwrap_or(0) as i32;
    scenes
        .get((index + delta).rem_euclid(count) as usize)
        .map(String::as_str)
}

/// Label column cap of value rows in a region.
pub fn label_cap(region: Region) -> usize {
    if region == Region::Controls {
        LABEL_COLUMN_WIDTH
    } else {
        GLOBAL_LABEL_WIDTH
    }
}

/// Where a choice row draws its dim marker for disabled options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisabledMarker {
    /// Columns from the row's left edge to the marker.
    pub offset: u16,
    pub width: u16,
    pub text: String,
    /// Label column width this row needs so the marker fits.
    pub label_width: usize,
    pub value_width: usize,
}

/// The `[ value ]` text of a choice, toggle, text or action row.
pub fn bracketed_value(view: &RowView) -> String {
    match view.kind {
        RowKind::Choice | RowKind::Toggle | RowKind::Text | RowKind::Action => {
            format!("[ {} ]", view.value)
        }
        _ => view.value.clone(),
    }
}

/// The dim "GPU (unavailable)" marker of a choice row, placed after the current value. The wording
/// shortens ("GPU (n/a)", "GPU x") and the label column narrows until it fits in `row_width`;
/// `None` when even the shortest form does not (the popover and help line still explain it).
pub fn disabled_marker(view: &RowView, row_width: u16, cap: usize) -> Option<DisabledMarker> {
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
    let default_label_width = cap.min(row_width / 2);
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

/// A panel placed on the screen: the area it may use, the rows it shows and the credit lines of
/// the displayed scene (the source URLs of an "inspired by" scene).
pub struct Panel<'a> {
    pub area: Rect,
    pub model: &'a RowModel,
    pub inspired_by: &'a [String],
}

impl Panel<'_> {
    /// Rows the credit footer needs: none on short screens or when the field to the right of the
    /// panel is wide enough to carry the credit itself.
    pub fn credit_footer_rows(&self) -> u16 {
        let longest = self
            .inspired_by
            .iter()
            .map(|url| UnicodeWidthStr::width(url.as_str()) + 14)
            .max()
            .unwrap_or(0);
        if self.area.height < 16
            || self.inspired_by.is_empty()
            || usize::from(
                self.area
                    .width
                    .saturating_sub(layout(self.area).panel.width),
            ) >= longest.max(16)
        {
            0
        } else {
            self.inspired_by.len() as u16 + 1
        }
    }

    /// Rows below the lists: help lines, status, key hint, credits and the saved-scene report.
    pub fn footer_rows(&self) -> u16 {
        let credits = self.credit_footer_rows();
        // The left column needs seven body rows to show its headings, navigation and at least one
        // row in both independently scrollable sections. Reclaim footer space on compact
        // terminals before either section disappears.
        let maximum_footer = self.area.height.saturating_sub(7);
        let Some(report) = saved_scene_activity_report(self.model) else {
            return (FOOTER_ROWS + credits).min(maximum_footer);
        };
        let (summary, activity) = saved_scene_activity_sections(&report);
        let width = layout(self.area).panel.width;
        let summary_rows = wrapped_rows(&summary, width);
        let activity_rows = wrapped_rows(&activity, width).min(MAX_ACTIVITY_VIEW_ROWS);
        let requested = summary_rows + activity_rows + PINNED_CONTROL_HELP_ROWS + 2 + credits;
        // On short terminals the settings regions take priority over the footer.
        requested.min(maximum_footer.max(u16::from(self.area.height > HEADER_ROWS + 2)))
    }

    /// The layout with the footer this model needs.
    pub fn geometry(&self) -> AnimationLayout {
        layout_with_footer(self.area, self.footer_rows())
    }

    fn window(&self, region: Region) -> Rect {
        let layout = self.geometry();
        window(&layout, region)
    }

    pub fn visible_rows(&self, region: Region) -> u16 {
        self.window(region).height
    }

    pub fn max_scroll(&self, region: Region) -> u16 {
        self.model
            .visual_height(region)
            .saturating_sub(self.visible_rows(region))
    }

    /// Every offset limited to what its region can scroll.
    pub fn clamped(&self, mut scrolls: Scrolls) -> Scrolls {
        for region in [Region::Scenes, Region::Global, Region::Controls] {
            scrolls.set(region, scrolls.get(region).min(self.max_scroll(region)));
        }
        scrolls
    }

    /// Scrolls just enough that `row` is visible inside its own region; the other regions keep
    /// their offsets, so the selected row can never be off screen.
    pub fn follow_selection(&self, row: usize, scrolls: Scrolls) -> Scrolls {
        let mut scrolls = scrolls;
        if let Some(region) = self.model.region(row) {
            let visible = self.visible_rows(region).max(1);
            let position = self.model.visual_row(row).unwrap_or(0);
            let current = scrolls.get(region);
            // Include the section heading above the row when scrolling up to it.
            let top = if region == Region::Scenes || visible == 1 {
                position
            } else {
                position.saturating_sub(1)
            };
            let next = if top < current {
                top
            } else if position >= current.saturating_add(visible) {
                position + 1 - visible
            } else {
                current
            };
            scrolls.set(region, next);
        }
        self.clamped(scrolls)
    }

    /// The region under a screen position: the left column splits at the global heading, the
    /// right column is the controls.
    pub fn region_at(&self, position: Position) -> Option<Region> {
        let layout = self.geometry();
        if contains(layout.scenes, position) {
            return Some(if position.y < layout.global_heading.y {
                Region::Scenes
            } else {
                Region::Global
            });
        }
        contains(layout.controls, position).then_some(Region::Controls)
    }

    /// Mouse-wheel scrolling of the region under `position` by `delta` rows. False when the
    /// pointer is over no region (the caller falls back to the controls).
    pub fn wheel_scroll(&self, scrolls: &mut Scrolls, position: Position, delta: i32) -> bool {
        let Some(region) = self.region_at(position) else {
            return false;
        };
        let maximum = i32::from(self.max_scroll(region));
        let next = (i32::from(scrolls.get(region)) + delta).clamp(0, maximum);
        scrolls.set(region, next as u16);
        true
    }

    pub fn row_rect(&self, row: usize, scrolls: Scrolls) -> Option<Rect> {
        let region = self.model.region(row)?;
        let rows = self.window(region);
        if rows.width <= 1 || rows.height == 0 {
            return None;
        }
        let relative = self
            .model
            .visual_row(row)?
            .checked_sub(scrolls.get(region))?;
        if relative >= rows.height {
            return None;
        }
        Some(Rect::new(rows.x, rows.y + relative, rows.width, 1))
    }

    pub fn row_y(&self, row: usize, scrolls: Scrolls) -> Option<u16> {
        self.row_rect(row, scrolls).map(|rectangle| rectangle.y)
    }

    /// Labels and value readouts have their own rectangles. Only the track changes a numeric
    /// value; clicking its label focuses the row without jumping it.
    pub fn slider_geometry(&self, row: usize, scrolls: Scrolls) -> Option<SliderGeometry> {
        let spec = self.model.view(row)?.slider()?;
        let rect = self.row_rect(row, scrolls)?;
        let usable = rect.width;
        if usable < 12 {
            return None;
        }
        let digits = spec
            .minimum
            .to_string()
            .len()
            .max(spec.maximum.to_string().len());
        let value_width = (digits as u16 + 6).max(10).min(usable);
        let label_width =
            if usable >= 36 { 18 } else { 14 }.min(usable.saturating_sub(value_width + 4));
        let track_width = usable.saturating_sub(label_width + value_width + 2);
        if track_width < 2 {
            return None;
        }
        Some(SliderGeometry {
            label: Rect::new(rect.x, rect.y, label_width, 1),
            track: Rect::new(rect.x + label_width + 1, rect.y, track_width, 1),
            value: Rect::new(rect.right() - value_width, rect.y, value_width, 1),
        })
    }

    /// Dragging uses the owned row, independent of the pointer's vertical position. Horizontal
    /// motion beyond either endpoint clamps to that endpoint.
    pub fn slider_value_at(&self, row: usize, scrolls: Scrolls, column: u16) -> Option<i32> {
        let geometry = self.slider_geometry(row, scrolls)?;
        let spec = self.model.view(row)?.slider()?;
        Some(spec.value_at(
            column.saturating_sub(geometry.track.x),
            geometry.track.width,
        ))
    }

    /// The row on the visual line under `position`, if any.
    pub fn row_at(&self, scrolls: Scrolls, position: Position) -> Option<usize> {
        let layout = self.geometry();
        for region in [Region::Scenes, Region::Global, Region::Controls] {
            let rows = window(&layout, region);
            if rows.width <= 1 || !contains(rows, position) {
                continue;
            }
            let visual = scrolls.get(region).checked_add(position.y - rows.y)?;
            return self
                .model
                .region_rows(region)
                .find(|&row| self.model.visual_row(row) == Some(visual));
        }
        None
    }

    pub fn hit(&self, scrolls: Scrolls, position: Position) -> Option<AnimationHit> {
        let layout = self.geometry();
        if !contains(layout.panel, position) {
            return None;
        }
        if contains(layout.scene_nav, position) {
            let nav = layout.scene_nav;
            if position.x < nav.x + NAV_BUTTON_WIDTH.min(nav.width / 2) {
                return Some(AnimationHit::PreviousScene);
            }
            if position.x
                >= nav
                    .right()
                    .saturating_sub(NAV_BUTTON_WIDTH.min(nav.width / 2))
            {
                return Some(AnimationHit::NextScene);
            }
            return None;
        }
        for region in [Region::Scenes, Region::Global, Region::Controls] {
            let rows = window(&layout, region);
            let maximum = self.max_scroll(region);
            if position.x == rows.right()
                && position.y >= rows.y
                && position.y < rows.bottom()
                && maximum > 0
            {
                let relative = position.y - rows.y;
                let target = u32::from(maximum) * u32::from(relative)
                    / u32::from(rows.height.saturating_sub(1).max(1));
                return Some(AnimationHit::ScrollTo(region, target as u16));
            }
        }
        let row = self.row_at(scrolls, position)?;
        let rect = self.row_rect(row, scrolls)?;
        if let Some(geometry) = self.slider_geometry(row, scrolls) {
            if contains(geometry.track, position) {
                return self
                    .slider_value_at(row, scrolls, position.x)
                    .map(|value| AnimationHit::Slider { row, value });
            }
        }
        let view = self.model.view(row)?;
        let cap = label_cap(self.model.region(row)?);
        if let Some(marker) = disabled_marker(view, rect.width.saturating_sub(1), cap) {
            let start = rect.x + marker.offset;
            if (start..start + marker.width).contains(&position.x) {
                return Some(AnimationHit::DisabledOption(row));
            }
        }
        Some(match view.kind {
            RowKind::Toggle | RowKind::Text | RowKind::Location | RowKind::Action => {
                AnimationHit::Activate(row)
            }
            RowKind::Choice
            | RowKind::Scene { .. }
            | RowKind::Slider(_)
            | RowKind::Number { .. }
            | RowKind::Status => AnimationHit::Select(row),
        })
    }
}

/// The row window of a region.
pub fn window(layout: &AnimationLayout, region: Region) -> Rect {
    match region {
        Region::Scenes => layout.scene_list,
        Region::Global => layout.global,
        Region::Controls => layout.controls_rows,
    }
}
