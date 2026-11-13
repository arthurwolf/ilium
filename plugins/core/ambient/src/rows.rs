//! The generic row list of Settings -> Animations, ported from
//! `ilium-client/src/animation_rows.rs`.
//!
//! Rows are derived from the settings and the hosted scene, never from fixed indices: one row
//! per scene, then Display, the shared look, the pattern and motion controls, the selected
//! scene's own controls, a Location row for observer-aware scenes, playback and cache rows, the
//! scene status line and the full-screen preview action.
//!
//! The kernel derives the list from `AnimationSettings`, `AnimationKind` and the appearance
//! catalogue. Those catalogues are separate concerns (the scene registry and the shared look), so
//! the plugin takes them as a plain [`RowInputs`] value. The rules that decide which row shows,
//! in which section and with which readout live here and are the part this module owns.
//!
//! Visibility rules (each row is shown only when it can do something):
//! * Playback, Loop seconds and the cache line apply to built-in scenes only; live-only hosted
//!   scenes never use the loop cache. Loop seconds and the cache line also hide while Playback
//!   is Live.
//! * Location shows for scenes that use an observer location and for the map scene.
//! * Scene status shows for every hosted scene (even when healthy, so the rows below it never
//!   shift when a status appears or clears).
//!
//! Pure: no host call, no I/O.

use crate::control::{Control, ControlKind, ControlValue};
use crate::values::{animation_number_spec, VOXEL_LANDSCAPE_ID};
use std::time::Duration;

/// Scene id of the map scene whose Location row shows the map selection.
pub const OPENSTREETMAP_ID: &str = "openstreetmap";
/// Scene id of the scene that shows an article and has no pattern rows.
pub const WIKIPEDIA_ID: &str = "wikipedia";
/// Scene id of the semantic scene (its status row is the recommendation).
pub const SEMANTIC_ID: &str = "semantic";
/// Row id of the loop-length slider, drawn on a logarithmic track.
pub const LOOP_SECONDS_ID: &str = "loop_seconds";
/// How many numbered "scene control" help topics the catalog carries.
pub const SCENE_CONTROL_TOPICS: usize = 10;

/// One row of the Animations list.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimationRow {
    /// A scene by registry id; selecting it previews and persists it.
    Scene(String),
    /// A control shared by every scene, by stable id (`speed`, `dither`, ...).
    Common(&'static str),
    /// A control of the selected scene, by stable id.
    SceneControl(&'static str),
    Location,
    CacheStatus,
    SceneStatus,
    FullScreenPreview,
}

/// Which part of the Settings panel a row lives in: the compact scene list, the global settings
/// under it (look, display, pattern: shared by every animation) or the selected animation's own
/// settings on the right.
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

/// State of the loop cache, as the kernel reports it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CacheStatus {
    pub completed_frames: usize,
    pub total_frames: usize,
    pub estimated_bytes: usize,
    pub resident_bytes: usize,
    pub elapsed: Duration,
    pub eta: Option<Duration>,
    pub is_ready: bool,
    pub is_limited: bool,
    pub has_error: bool,
}

/// Runtime facts the row list depends on besides the settings.
#[derive(Debug, Clone, Default)]
pub struct RowContext {
    pub effective_kind: Option<String>,
    pub scene_status: Option<String>,
    pub cache: CacheStatus,
    /// Estimated RAM of the loop cache at the current screen size.
    pub loop_bytes: usize,
}

/// One scene of the registry: what the scene list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneEntry {
    pub id: String,
    pub label: String,
    pub description: String,
}

/// The catalogues the row list is derived from. The scene registry and the shared look supply
/// them; this module decides which rows they produce.
#[derive(Debug, Clone, Default)]
pub struct RowInputs {
    /// Every scene, in list order.
    pub scenes: Vec<SceneEntry>,
    /// Id of the selected scene.
    pub selected: String,
    /// Shared look controls in display order (`look_mode` first rule applies).
    pub look: Vec<Control>,
    /// Pattern controls (`look_pattern_*`), shown for every scene except Wikipedia.
    pub pattern: Vec<Control>,
    /// Every other shared control by id (`panels`, `background`, `speed`, `fps_limit`,
    /// `density`, `dither`, `playback`, `loop_seconds`, `lightness`, `hue`, `saturation`).
    pub common: Vec<Control>,
    /// The selected scene's own controls.
    pub scene_controls: Vec<Control>,
    /// The look is in Monotone mode (adds the ink colour rows under the mode).
    pub monotone: bool,
    /// The selected scene uses the observer location.
    pub uses_location: bool,
    /// The selected scene is a hosted ambient scene (it reports a status).
    pub is_hosted: bool,
    /// The scene that renders now never uses the loop cache (the registry resolves the semantic
    /// scene's recommendation before it fills the inputs).
    pub is_live_only: bool,
    /// Playback is Loop (shows loop length and cache rows).
    pub playback_loop: bool,
    /// The map scene's own selection, shown by its Location row.
    pub map_location: Option<String>,
    /// The shared observer, shown by every other Location row.
    pub observer: String,
}

impl RowInputs {
    fn common_control(&self, id: &str) -> Option<Control> {
        self.look
            .iter()
            .chain(&self.pattern)
            .chain(&self.common)
            .find(|control| control.id == id)
            .cloned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliderSpec {
    pub minimum: i32,
    pub maximum: i32,
    pub step: i32,
    pub value: i32,
    pub logarithmic: bool,
}

/// The track cell that shows `value` on a `track_width`-cell slider.
pub fn slider_thumb_offset(minimum: i32, maximum: i32, value: i32, track_width: u16) -> u16 {
    let span = (i64::from(maximum) - i64::from(minimum)).max(1);
    let value = i64::from(value.clamp(minimum, maximum)) - i64::from(minimum);
    ((value * i64::from(track_width.saturating_sub(1)) + span / 2) / span) as u16
}

/// The value under track cell `offset` of a `track_width`-cell slider, rounded to `step`.
pub fn slider_value_at(
    minimum: i32,
    maximum: i32,
    step: i32,
    offset: u16,
    track_width: u16,
) -> i32 {
    if track_width < 2 || maximum <= minimum {
        return minimum;
    }
    let offset = i64::from(offset.min(track_width - 1));
    let span = i64::from(maximum) - i64::from(minimum);
    let denominator = i64::from(track_width - 1);
    let raw = i64::from(minimum) + (span * offset + denominator / 2) / denominator;
    let step = i64::from(step.max(1));
    let rounded = i64::from(minimum) + ((raw - i64::from(minimum) + step / 2) / step) * step;
    rounded.min(i64::from(maximum)) as i32
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
        slider_value_at(self.minimum, self.maximum, self.step, offset, track_width)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    Scene {
        is_active: bool,
    },
    Slider(SliderSpec),
    Number {
        minimum: i128,
        maximum: i128,
        step: i128,
        value: i128,
    },
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
    /// Choice options that exist but cannot be selected right now (carried from the control,
    /// never re-queried from globals).
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

/// The text up to and including the first sentence end (`. `, `! `, `? ` or the end of the text).
pub fn first_sentence(text: &str) -> &str {
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

/// Status-line text explaining why a choice step could not move onto a disabled option, or
/// `None` when `control` has none.
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

    pub fn number(&self) -> Option<(i128, i128, i128, i128)> {
        match self.kind {
            RowKind::Slider(spec) => Some((
                i128::from(spec.minimum),
                i128::from(spec.maximum),
                i128::from(spec.step),
                i128::from(spec.value),
            )),
            RowKind::Number {
                minimum,
                maximum,
                step,
                value,
            } => Some((minimum, maximum, step, value)),
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
    controls: Vec<Option<Control>>,
    effective_kind: Option<String>,
}

impl RowModel {
    pub fn new(inputs: &RowInputs, context: &RowContext) -> Self {
        let rows = rows(inputs);
        let controls: Vec<_> = rows
            .iter()
            .map(|row| match row {
                AnimationRow::Common(id) => inputs.common_control(id),
                AnimationRow::SceneControl(id) => inputs
                    .scene_controls
                    .iter()
                    .find(|control| control.id == *id)
                    .cloned(),
                _ => None,
            })
            .collect();
        let views = rows
            .iter()
            .zip(&controls)
            .map(|(row, control)| {
                control.as_ref().map_or_else(
                    || row.view(inputs, context),
                    |control| control_view(row, &inputs.selected, control),
                )
            })
            .collect();
        let effective_kind = context
            .effective_kind
            .clone()
            .or_else(|| (inputs.selected != SEMANTIC_ID).then(|| inputs.selected.clone()));
        Self {
            rows,
            views,
            controls,
            effective_kind,
        }
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
                "speed"
                | "fps_limit"
                | "density"
                | "dither"
                | "look_pattern_contrast"
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

    /// Display position of a row inside its region, counting the section headings above it
    /// (headings only exist in the Global and Controls regions; the scene list is a plain list).
    pub fn visual_row(&self, row: usize) -> Option<u16> {
        let region = self.region(row)?;
        if region == Region::Scenes {
            return u16::try_from(
                self.rows[..row]
                    .iter()
                    .filter(|r| r.region() == region)
                    .count(),
            )
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
        let last = (0..self.len())
            .rev()
            .find(|row| self.region(*row) == Some(region));
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

    /// The exact options/bounds behind this displayed row. Dialogs retain this snapshot and
    /// revalidate it against current settings before committing.
    pub fn control(&self, index: usize) -> Option<&Control> {
        self.controls.get(index).and_then(Option::as_ref)
    }

    pub fn views(&self) -> &[RowView] {
        &self.views
    }

    pub fn effective_kind(&self) -> Option<&str> {
        self.effective_kind.as_deref()
    }

    /// Index of the first row of the selected scene's controls, or of the first row on the right
    /// when the scene has none: where Enter on a scene row lands.
    pub fn first_control_index(&self) -> usize {
        self.rows
            .iter()
            .position(|row| matches!(row, AnimationRow::SceneControl(_)))
            .or_else(|| self.region_rows(Region::Controls).next())
            .unwrap_or(0)
    }
}

/// The row list for the given catalogues.
pub fn rows(inputs: &RowInputs) -> Vec<AnimationRow> {
    let selected = inputs.selected.as_str();
    let mut rows: Vec<AnimationRow> = inputs
        .scenes
        .iter()
        .map(|scene| AnimationRow::Scene(scene.id.clone()))
        .collect();
    // Global settings, shared by every animation: display, color, pattern.
    rows.extend(
        ["panels", "background"]
            .into_iter()
            .map(AnimationRow::Common),
    );
    for control in &inputs.look {
        rows.push(AnimationRow::Common(control.id));
        // The single ink color of Monotone mode sits right under the mode.
        if control.id == "look_mode" && inputs.monotone {
            rows.extend(
                ["lightness", "hue", "saturation"]
                    .into_iter()
                    .map(AnimationRow::Common),
            );
        }
    }
    if selected != WIKIPEDIA_ID {
        rows.extend(
            ["speed", "fps_limit", "density", "dither"]
                .into_iter()
                .map(AnimationRow::Common),
        );
        rows.extend(
            inputs
                .pattern
                .iter()
                .map(|control| AnimationRow::Common(control.id)),
        );
    }
    // The selected animation's own settings.
    rows.extend(
        inputs
            .scene_controls
            .iter()
            .map(|control| AnimationRow::SceneControl(control.id)),
    );
    if selected == OPENSTREETMAP_ID || inputs.uses_location {
        rows.push(AnimationRow::Location);
    }
    if !inputs.is_live_only {
        rows.push(AnimationRow::Common("playback"));
        if inputs.playback_loop {
            rows.push(AnimationRow::Common("loop_seconds"));
            rows.push(AnimationRow::CacheStatus);
        }
    }
    if inputs.is_hosted || matches!(selected, WIKIPEDIA_ID | SEMANTIC_ID) {
        rows.push(AnimationRow::SceneStatus);
    }
    rows.push(AnimationRow::FullScreenPreview);
    rows
}

fn control_view(row: &AnimationRow, animation_kind: &str, control: &Control) -> RowView {
    let wide_number = matches!(row, AnimationRow::SceneControl("seed"))
        .then(|| animation_number_spec(animation_kind, control))
        .flatten();
    let kind = if let Some((minimum, maximum, step, value)) = wide_number {
        RowKind::Number {
            minimum,
            maximum,
            step,
            value,
        }
    } else {
        match &control.kind {
            ControlKind::Slider { min, max, step, .. } => RowKind::Slider(SliderSpec {
                logarithmic: control.id == LOOP_SECONDS_ID,
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
        }
    };
    let disabled_options = disabled_options_of(control);
    // Why an option is greyed out comes first: the help line is only two rows tall and the
    // reason is what the user is looking for.
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

fn format_duration(duration: Duration) -> String {
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

/// The one-line cache readout: RAM first so it survives short readout truncation.
pub fn cache_line(context: &RowContext) -> String {
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
    /// The presentation of this row for the current inputs.
    pub fn view(&self, inputs: &RowInputs, context: &RowContext) -> RowView {
        let selected = inputs.selected.as_str();
        let missing = || RowView {
            label: "?".to_owned(),
            value: String::new(),
            kind: RowKind::Status,
            help: String::new(),
            disabled_options: Vec::new(),
        };
        match self {
            Self::Scene(id) => inputs
                .scenes
                .iter()
                .find(|scene| scene.id == *id)
                .map_or_else(missing, |scene| RowView {
                    label: scene.label.clone(),
                    value: String::new(),
                    kind: RowKind::Scene {
                        is_active: scene.id == selected,
                    },
                    help: scene.description.clone(),
                    disabled_options: Vec::new(),
                }),
            Self::Common(id) => inputs
                .common_control(id)
                .map_or_else(missing, |control| control_view(self, selected, &control)),
            Self::SceneControl(id) => inputs
                .scene_controls
                .iter()
                .find(|control| control.id == *id)
                .map_or_else(missing, |control| control_view(self, selected, control)),
            Self::Location => {
                if selected == OPENSTREETMAP_ID {
                    return RowView {
                        label: "Map location".to_owned(),
                        value: inputs.map_location.clone().unwrap_or_default(),
                        kind: RowKind::Location,
                        help: "Choose a world-map point, coordinates or an address for this OSM scene. Other scenes keep their observer location. New points need your configured Overpass geometry service.".to_owned(),
                        disabled_options: Vec::new(),
                    };
                }
                RowView {
                    label: "Location".to_owned(),
                    value: inputs.observer.clone(),
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
                label: if selected == SEMANTIC_ID {
                    "Recommendation"
                } else {
                    "Scene status"
                }
                .to_owned(),
                value: context
                    .scene_status
                    .as_deref()
                    .map(|status| {
                        if selected == VOXEL_LANDSCAPE_ID {
                            status.lines().next().unwrap_or_default().to_owned()
                        } else {
                            status.to_owned()
                        }
                    })
                    .unwrap_or_else(|| {
                        if selected == SEMANTIC_ID {
                            "Awaiting tree recommendation"
                        } else {
                            "OK"
                        }
                        .to_owned()
                    }),
                kind: RowKind::Status,
                help: if selected == SEMANTIC_ID {
                    "The selected project or entry's validated recommendation from tree reorganization; selecting an entry does not make another model request."
                        .to_owned()
                } else if selected == WIKIPEDIA_ID {
                    "Article title, source URL, revision, offline state, missing images and font coverage."
                        .to_owned()
                } else if selected == VOXEL_LANDSCAPE_ID {
                    context
                        .scene_status
                        .as_deref()
                        .filter(|status| status.lines().count() > 1)
                        .map(|status| status.lines().skip(1).collect::<Vec<_>>().join("\n"))
                        .filter(|details| !details.is_empty())
                        .unwrap_or_else(|| {
                            "What the saved-world scene is doing while it prepares and renders."
                                .to_owned()
                        })
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
}
