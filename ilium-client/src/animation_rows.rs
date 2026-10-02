//! The generic row list of Settings -> Animations.
//!
//! Rows are derived from the settings and the hosted scene, never from fixed
//! indices: one row per scene, then Background, the selected scene's own
//! controls, a Location row for observer-aware scenes, the shared controls
//! that are meaningful for the selected scene, a scene status line and the
//! full-screen preview action. Rendering, keyboard, mouse, scrolling and help
//! anchors all read the same `RowModel`.
//!
//! Visibility rules (each row is shown only when it can do something):
//! * Playback, Loop seconds and the cache line apply to built-in scenes only;
//!   live-only hosted scenes never use the loop cache. Loop seconds and the
//!   cache line also hide while Playback is Live.
//! * Lightness / Hue / Saturation tint a scene's dots. They hide when the
//!   hosted scene supplies its own per-cell colors (`uses_cell_colors()`),
//!   because the palette would have no effect.
//! * Location shows for scenes whose kind `uses_location()`.
//! * Scene status shows for every hosted scene (even when healthy, so the
//!   rows below it never shift when a status appears or clears).

use crate::background_animation::{
    slider_thumb_offset, AnimationCacheStatus, AnimationKind, AnimationPlaybackMode,
    AnimationSettings,
};
use ilium_ambient::{Control, ControlKind, ControlValue};

/// One row of the Animations list.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimationRow {
    /// A scene; selecting it previews and persists it.
    Scene(AnimationKind),
    /// A control shared by every scene, by stable id (`speed`, `dither`, ...).
    Common(&'static str),
    /// A control of the selected scene, by stable id.
    SceneControl(&'static str),
    Location,
    CacheStatus,
    SceneStatus,
    FullScreenPreview,
}

/// Which part of the Settings panel a row lives in: the compact scene list,
/// the global settings under it (look, display, pattern: shared by every
/// animation) or the selected animation's own settings on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Scenes,
    Global,
    Controls,
}

impl AnimationRow {
    pub fn region(&self) -> Region {
        match self {
            Self::Scene(_) => Region::Scenes,
            Self::Common("playback" | "loop_seconds") => Region::Controls,
            Self::Common(_) => Region::Global,
            Self::SceneControl(_)
            | Self::Location
            | Self::CacheStatus
            | Self::SceneStatus
            | Self::FullScreenPreview => Region::Controls,
        }
    }
}

/// Runtime facts the row list depends on besides the settings.
#[derive(Debug, Clone, Default)]
pub struct RowContext {
    /// The hosted scene paints its own per-cell colors.
    pub scene_uses_cell_colors: bool,
    pub scene_status: Option<String>,
    pub cache: AnimationCacheStatus,
    /// Estimated RAM of the loop cache at the current screen size.
    pub loop_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliderSpec {
    pub minimum: i32,
    pub maximum: i32,
    pub step: i32,
    pub value: i32,
    pub logarithmic: bool,
}

impl SliderSpec {
    pub fn thumb_offset(self, track_width: u16) -> u16 {
        if self.logarithmic && self.minimum > 0 && self.maximum > self.minimum {
            let fraction = (f64::from(self.value.clamp(self.minimum, self.maximum))
                / f64::from(self.minimum))
            .ln()
                / (f64::from(self.maximum) / f64::from(self.minimum)).ln();
            return (fraction * f64::from(track_width.saturating_sub(1))).round() as u16;
        }
        slider_thumb_offset(self.minimum, self.maximum, self.value, track_width)
    }

    pub fn value_at(self, offset: u16, track_width: u16) -> i32 {
        if self.logarithmic && self.minimum > 0 && self.maximum > self.minimum && track_width > 1 {
            let fraction = f64::from(offset.min(track_width - 1)) / f64::from(track_width - 1);
            return (f64::from(self.minimum)
                * (f64::from(self.maximum) / f64::from(self.minimum)).powf(fraction))
            .round()
            .clamp(f64::from(self.minimum), f64::from(self.maximum)) as i32;
        }
        crate::background_animation::slider_value_at(
            self.minimum,
            self.maximum,
            self.step,
            offset,
            track_width,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    Scene { is_active: bool },
    Slider(SliderSpec),
    Choice,
    Toggle,
    Text,
    Location,
    Status,
    Action,
}

/// Everything the renderer and hit-testing need to know about one row.
#[derive(Debug, Clone, PartialEq)]
pub struct RowView {
    pub label: String,
    /// Display text of the value column (slider readout, choice label, ...).
    pub value: String,
    pub kind: RowKind,
    pub help: String,
    /// Choice options that exist but cannot be selected right now (carried
    /// from the scene's `Control`, never re-queried from globals).
    pub disabled_options: Vec<DisabledOption>,
}

/// A choice option the user can see but not select, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisabledOption {
    /// The option's own label, for example `GPU`.
    pub label: String,
    /// Multi-sentence explanation and fix, shown in the hover popover.
    pub reason: String,
}

impl DisabledOption {
    /// One line for the help and status lines: the reason's first sentence.
    pub fn notice(&self) -> String {
        format!(
            "{} unavailable: {}",
            self.label,
            first_sentence(&self.reason)
        )
    }
}

/// The text up to and including the first sentence end (`. `, `! `, `? ` or
/// the end of the text).
fn first_sentence(text: &str) -> &str {
    let text = text.trim();
    text.char_indices()
        .find(|(index, character)| {
            matches!(character, '.' | '!' | '?')
                && text[index + character.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        })
        .map_or(text, |(index, character)| {
            &text[..index + character.len_utf8()]
        })
}

/// The disabled options of `control` with their option labels.
fn disabled_options_of(control: &Control) -> Vec<DisabledOption> {
    let ControlKind::Choice { options } = &control.kind else {
        return Vec::new();
    };
    control
        .disabled_options
        .iter()
        .filter_map(|(index, reason)| {
            options.get(*index).map(|label| DisabledOption {
                label: (*label).to_owned(),
                reason: reason.clone(),
            })
        })
        .collect()
}

/// Status-line text explaining why a choice step could not move onto a
/// disabled option, or `None` when `control` has none.
pub fn disabled_notice(control: &Control) -> Option<String> {
    disabled_options_of(control)
        .first()
        .map(DisabledOption::notice)
}

impl RowView {
    pub fn slider(&self) -> Option<SliderSpec> {
        match self.kind {
            RowKind::Slider(spec) => Some(spec),
            _ => None,
        }
    }

    /// Status-line text for the first disabled option, if any.
    pub fn disabled_notice(&self) -> Option<String> {
        self.disabled_options.first().map(DisabledOption::notice)
    }
}

/// What activating a row asks the surrounding UI to do.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimationRowOutcome {
    /// Nothing more to do (the value, if any, was already applied).
    Done,
    /// Open the single-line prompt for a Text control.
    TextPrompt {
        control: &'static str,
        label: String,
        hint: &'static str,
        current: String,
    },
    LocationPicker,
    FullScreenPreview,
}

pub struct RowModel {
    rows: Vec<AnimationRow>,
    views: Vec<RowView>,
}

impl RowModel {
    pub fn new(settings: &AnimationSettings, context: &RowContext) -> Self {
        let rows = rows(settings, context);
        let scene_controls = settings.scene_controls();
        let views = rows
            .iter()
            .map(|row| row.view(settings, &scene_controls, context))
            .collect();
        Self { rows, views }
    }

    pub fn region(&self, row: usize) -> Option<Region> {
        self.row(row).map(AnimationRow::region)
    }

    /// The heading of the section a row sits under, within its region.
    pub fn section(&self, row: usize) -> &'static str {
        match self.row(row) {
            Some(AnimationRow::Scene(_)) => "Scenes",
            Some(AnimationRow::Common("background" | "panels")) => "Display",
            Some(AnimationRow::Common(
                "speed" | "fps_limit" | "density" | "dither" | "look_pattern_contrast"
                | "look_pattern_invert",
            )) => "Pattern and motion",
            Some(AnimationRow::Common("playback" | "loop_seconds") | AnimationRow::CacheStatus) => {
                "Playback and cache"
            }
            Some(AnimationRow::Common(_)) => "Color",
            Some(AnimationRow::SceneControl(_) | AnimationRow::Location) => "Scene settings",
            _ => "Preview and status",
        }
    }

    /// Display position of a row inside its region, counting the section
    /// headings above it (headings only exist in the Global and Controls
    /// regions; the scene list is a plain list).
    pub fn visual_row(&self, row: usize) -> Option<u16> {
        let region = self.region(row)?;
        if region == Region::Scenes {
            return u16::try_from(self.rows[..row].iter().filter(|r| r.region() == region).count())
                .ok();
        }
        let mut position = 0u16;
        let mut previous = "";
        for index in (0..=row).filter(|index| self.region(*index) == Some(region)) {
            let section = self.section(index);
            if section != previous {
                position = position.saturating_add(1);
                previous = section;
            }
            if index == row {
                return Some(position);
            }
            position = position.saturating_add(1);
        }
        None
    }

    /// Rows (headings included) a region needs to show everything.
    pub fn visual_height(&self, region: Region) -> u16 {
        let last = (0..self.len()).rev().find(|row| self.region(*row) == Some(region));
        last.and_then(|row| self.visual_row(row))
            .map_or(0, |position| position.saturating_add(1))
    }

    /// Indexes of every row in a region, in display order.
    pub fn region_rows(&self, region: Region) -> impl Iterator<Item = usize> + '_ {
        (0..self.len()).filter(move |row| self.region(*row) == Some(region))
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn rows(&self) -> &[AnimationRow] {
        &self.rows
    }

    pub fn row(&self, index: usize) -> Option<&AnimationRow> {
        self.rows.get(index)
    }

    pub fn view(&self, index: usize) -> Option<&RowView> {
        self.views.get(index)
    }

    pub fn views(&self) -> &[RowView] {
        &self.views
    }

    /// Index of the first row of the selected scene's controls, or of the
    /// first row on the right when the scene has none: where Enter on a
    /// scene row lands.
    pub fn first_control_index(&self) -> usize {
        self.rows
            .iter()
            .position(|row| matches!(row, AnimationRow::SceneControl(_)))
            .or_else(|| self.region_rows(Region::Controls).next())
            .unwrap_or(0)
    }
}

/// The ordered row list for `settings`.
pub fn rows(settings: &AnimationSettings, _context: &RowContext) -> Vec<AnimationRow> {
    let kind = settings.kind;
    let mut rows: Vec<AnimationRow> = AnimationKind::ALL
        .iter()
        .copied()
        .map(AnimationRow::Scene)
        .collect();
    // Global settings, shared by every animation: display, color, pattern.
    rows.extend(["panels", "background"].into_iter().map(AnimationRow::Common));
    for control in settings.appearance.controls() {
        rows.push(AnimationRow::Common(control.id));
        // The single ink color of Monotone mode sits right under the mode.
        if control.id == "look_mode" && settings.appearance.mode == ilium_ambient::style::ColorMode::Monotone {
            rows.extend(["lightness", "hue", "saturation"].into_iter().map(AnimationRow::Common));
        }
    }
    if kind != AnimationKind::Wikipedia {
        rows.extend(
            ["speed", "fps_limit", "density", "dither"]
                .into_iter()
                .map(AnimationRow::Common),
        );
        rows.extend(
            settings
                .appearance
                .pattern_controls()
                .into_iter()
                .map(|control| AnimationRow::Common(control.id)),
        );
    }
    // The selected animation's own settings.
    rows.extend(
        settings
            .scene_controls()
            .into_iter()
            .map(|control| AnimationRow::SceneControl(control.id)),
    );
    if kind
        .ambient()
        .is_some_and(|ambient| ambient.uses_location())
    {
        rows.push(AnimationRow::Location);
    }
    if !kind.is_live_only() {
        rows.push(AnimationRow::Common("playback"));
        if settings.playback_mode == AnimationPlaybackMode::Loop {
            rows.push(AnimationRow::Common("loop_seconds"));
            rows.push(AnimationRow::CacheStatus);
        }
    }
    if kind.is_ambient() || kind == AnimationKind::Wikipedia {
        rows.push(AnimationRow::SceneStatus);
    }
    rows.push(AnimationRow::FullScreenPreview);
    rows
}

fn control_view(control: &Control) -> RowView {
    let kind = match &control.kind {
        ControlKind::Slider { min, max, step, .. } => RowKind::Slider(SliderSpec {
            logarithmic: control.id == "loop_seconds",
            minimum: *min,
            maximum: *max,
            step: *step,
            value: match &control.value {
                ControlValue::Number(number) => *number,
                _ => *min,
            },
        }),
        ControlKind::Choice { .. } => RowKind::Choice,
        ControlKind::Toggle => RowKind::Toggle,
        ControlKind::Text { .. } => RowKind::Text,
    };
    let disabled_options = disabled_options_of(control);
    // Why an option is greyed out comes first: the help line is only two
    // rows tall and the reason is what the user is looking for.
    let mut help = String::new();
    for option in &disabled_options {
        help.push_str(&option.notice());
        help.push(' ');
    }
    help.push_str(control.help);
    if let Some(detail) = &control.help_detail {
        help.push(' ');
        help.push_str(detail);
    }
    RowView {
        label: control.label.to_owned(),
        value: control.display_value(),
        kind,
        help,
        disabled_options,
    }
}

fn format_duration(duration: std::time::Duration) -> String {
    if duration.is_zero() {
        return "done".to_owned();
    }
    format!("{}s", duration.as_secs().max(1))
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn cache_line(context: &RowContext) -> String {
    let status = &context.cache;
    let percent = status
        .completed_frames
        .saturating_mul(100)
        .checked_div(status.total_frames)
        .unwrap_or(0)
        .min(100);
    let eta = status.eta.map_or_else(|| "--".to_owned(), format_duration);
    let filled = percent / 10;
    format!(
        "RAM {} / {} [{}{}] {:>3}% {} ETA {}",
        format_bytes(status.resident_bytes),
        format_bytes(if status.estimated_bytes > 0 {
            status.estimated_bytes
        } else {
            context.loop_bytes
        }),
        "=".repeat(filled),
        ".".repeat(10usize.saturating_sub(filled)),
        percent,
        if status.is_ready {
            "ready"
        } else if status.is_limited {
            "limit; live"
        } else if status.has_error {
            "failed; live"
        } else {
            "building"
        },
        eta
    )
}

impl AnimationRow {
    /// The presentation of this row for the current settings.
    pub fn view(
        &self,
        settings: &AnimationSettings,
        scene_controls: &[Control],
        context: &RowContext,
    ) -> RowView {
        let missing = || RowView {
            label: "?".to_owned(),
            value: String::new(),
            kind: RowKind::Status,
            help: String::new(),
            disabled_options: Vec::new(),
        };
        match self {
            Self::Scene(kind) => RowView {
                label: kind.label().to_owned(),
                value: String::new(),
                kind: RowKind::Scene {
                    is_active: *kind == settings.kind,
                },
                help: kind.description().to_owned(),
                disabled_options: Vec::new(),
            },
            Self::Common(id) => settings
                .common_control(id)
                .map_or_else(missing, |control| control_view(&control)),
            Self::SceneControl(id) => scene_controls
                .iter()
                .find(|control| control.id == *id)
                .map_or_else(missing, control_view),
            Self::Location => {
                let location = &settings.ambient.location;
                RowView {
                    label: "Location".to_owned(),
                    value: format!("{} ({})", location.label, location.coordinate_text()),
                    kind: RowKind::Location,
                    help: "Where on Earth the sky, weather and city lights are shown for. Shared by all location scenes.".to_owned(),
                    disabled_options: Vec::new(),
                }
            }
            Self::CacheStatus => RowView {
                label: "Cache".to_owned(),
                value: cache_line(context),
                kind: RowKind::Status,
                help: "Packed-frame RAM now / projected, then progress and ETA. Generation runs in the background; oversized caches retain Live playback.".to_owned(),
                disabled_options: Vec::new(),
            },
            Self::SceneStatus => RowView {
                label: "Scene status".to_owned(),
                value: context.scene_status.clone().unwrap_or_else(|| "OK".to_owned()),
                kind: RowKind::Status,
                help: if settings.kind == AnimationKind::Wikipedia {
                    "Article title, source URL, revision, offline state, missing images and font coverage."
                        .to_owned()
                } else {
                    "What the running scene reports: downloads, missing tools, missing devices."
                        .to_owned()
                },
                disabled_options: Vec::new(),
            },
            Self::FullScreenPreview => RowView {
                label: "Full screen preview".to_owned(),
                value: "f".to_owned(),
                kind: RowKind::Action,
                help: "Hide the controls so the live field fills the whole terminal. Any key or click returns.".to_owned(),
                disabled_options: Vec::new(),
            },
        }
    }

    /// The stable Settings-help topic of this row (see `settings_help`).
    /// `scene_controls` gives the position of a scene control among its
    /// siblings: controls added to a scene later fall back to the shared
    /// numbered topics instead of needing their own catalog entry.
    pub fn help_id(&self, scene_controls: &[Control]) -> String {
        match self {
            Self::Scene(AnimationKind::GalacticEmpires) => "AN-53".to_owned(),
            Self::Scene(AnimationKind::Wikipedia) => "AN-54".to_owned(),
            Self::Scene(AnimationKind::SolarSystem) => "AN-49".to_owned(),
            Self::Scene(AnimationKind::HexExpedition) => "AN-50".to_owned(),
            Self::Scene(AnimationKind::VectorTd) => "AN-51".to_owned(),
            Self::Scene(AnimationKind::VoxelLandscape) => "AN-52".to_owned(),
            Self::Scene(AnimationKind::TopographicMaps) => "AN-56".to_owned(),
            Self::Scene(AnimationKind::OpenStreetMap) => "AN-55".to_owned(),
            Self::Scene(AnimationKind::Graph) => "AN-57".to_owned(),
            Self::Scene(AnimationKind::Pi) => "AN-58".to_owned(),
            Self::Scene(AnimationKind::Earthquakes) => "AN-59".to_owned(),
            Self::Scene(AnimationKind::Aircraft) => "AN-60".to_owned(),
            Self::Scene(AnimationKind::Boats) => "AN-61".to_owned(),
            Self::Scene(AnimationKind::Chess) => "AN-62".to_owned(),
            Self::Scene(AnimationKind::Carpet) => "AN-63".to_owned(),
            Self::Scene(kind) => {
                let index = AnimationKind::ALL
                    .iter()
                    .position(|candidate| candidate == kind)
                    .unwrap_or(0);
                format!("AN-{:02}", index + 1)
            }
            Self::Common(id) if style_help_id(id).is_some() => {
                style_help_id(id).unwrap_or_default().to_owned()
            }
            Self::Common(id) => {
                let offset = match *id {
                    "background" => 0,
                    "speed" => 1,
                    "density" => 2,
                    "dither" => 3,
                    "lightness" => 4,
                    "hue" => 5,
                    "saturation" => 6,
                    "playback" => 7,
                    _ => 8,
                };
                format!("AN-{:02}", 25 + 1 + offset)
            }
            Self::CacheStatus => format!("AN-{:02}", 25 + 10),
            Self::Location => format!("AN-{:02}", 25 + 11),
            Self::SceneStatus => format!("AN-{:02}", 25 + 12),
            Self::FullScreenPreview => format!("AN-{:02}", 25 + 13),
            Self::SceneControl(id) => {
                let position = scene_controls
                    .iter()
                    .position(|control| control.id == *id)
                    .unwrap_or(0)
                    .min(SCENE_CONTROL_TOPICS - 1);
                format!("AN-{:02}", 25 + 14 + position)
            }
        }
    }
}

/// Help topic ids of the shared look, display and pattern rows. They are
/// separate from the numbered scene topics so they never renumber them.
pub const STYLE_HELP_IDS: [(&str, &str); 20] = [
    ("look_preset", "AN-C01"),
    ("look_mode", "AN-C02"),
    ("look_palette", "AN-C03"),
    ("look_source", "AN-C04"),
    ("look_reverse", "AN-C05"),
    ("look_shift", "AN-C06"),
    ("look_spread", "AN-C07"),
    ("look_grey_hue", "AN-C08"),
    ("look_grey_tint", "AN-C09"),
    ("look_brightness", "AN-C10"),
    ("look_contrast", "AN-C11"),
    ("look_gamma", "AN-C12"),
    ("look_saturation", "AN-C13"),
    ("look_hue_shift", "AN-C14"),
    ("look_invert", "AN-C15"),
    ("look_vignette", "AN-C16"),
    ("look_pattern_contrast", "AN-C17"),
    ("look_pattern_invert", "AN-C18"),
    ("fps_limit", "AN-C19"),
    ("panels", "AN-C20"),
];

fn style_help_id(id: &str) -> Option<&'static str> {
    STYLE_HELP_IDS
        .iter()
        .find(|(row, _)| *row == id)
        .map(|(_, help)| *help)
}

/// How many numbered "scene control" help topics the catalog carries.
pub const SCENE_CONTROL_TOPICS: usize = 10;

#[cfg(test)]
/// Every help id the row model can produce: scene rows, common and special
/// rows, then the numbered scene-control topics.
pub fn help_ids() -> Vec<String> {
    let count = 25 + 13 + SCENE_CONTROL_TOPICS + 4;
    let mut ids: Vec<String> = (1..=count)
        .map(|number| format!("AN-{number:02}"))
        .collect();
    if !ids.iter().any(|id| id == "AN-54") {
        ids.push("AN-54".to_owned());
    }
    if !ids.iter().any(|id| id == "AN-53") {
        ids.push("AN-53".to_owned());
    }
    ids.extend(STYLE_HELP_IDS.iter().map(|(_, help)| (*help).to_owned()));
    if !ids.iter().any(|id| id == "AN-55") {
        ids.push("AN-55".to_owned());
    }
    if !ids.iter().any(|id| id == "AN-56") {
        ids.push("AN-56".to_owned());
    }
    for number in 57..=63 {
        ids.push(format!("AN-{number:02}"));
    }
    ids
}

#[cfg(test)]
mod overhaul_tests {
    use super::*;
    #[test]
    fn overhaul_ram_usage_is_visible_before_progress_details() {
        let context = RowContext {
            loop_bytes: 1024 * 1024,
            ..Default::default()
        };
        let text = cache_line(&context);
        assert!(
            text.starts_with("RAM "),
            "RAM must survive short readout truncation: {text}"
        );
    }

    #[test]
    fn overhaul_loop_duration_uses_a_logarithmic_track() {
        let settings = AnimationSettings::default();
        let model = RowModel::new(&settings, &RowContext::default());
        let index = model
            .rows()
            .iter()
            .position(|row| *row == AnimationRow::Common("loop_seconds"))
            .unwrap();
        let slider = model.view(index).unwrap().slider().unwrap();
        assert_eq!(slider.value_at(0, 101), 1);
        assert_eq!(slider.value_at(100, 101), 120);
        assert!((10..=12).contains(&slider.value_at(50, 101)));
    }
}
