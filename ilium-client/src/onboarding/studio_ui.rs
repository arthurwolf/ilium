//! A sound instrument with one geometry model for paint, focus and gestures.
//! Audio is never rendered here: the oscilloscope reads `SoundStudio::preview`.

use ilium_sound::{SoundDesign, SoundEvent, Waveform};
use ratatui::{
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use super::studio::{SoundControl, SoundStudio};

const CANVAS: Color = Color::Rgb(13, 16, 23);
const PANEL: Color = Color::Rgb(24, 29, 40);
const INK: Color = Color::Rgb(224, 231, 244);
const MUTED: Color = Color::Rgb(144, 157, 179);
const ACCENT: Color = Color::Rgb(242, 188, 105);
const SIGNAL: Color = Color::Rgb(116, 210, 194);
const TRACK: Color = Color::Rgb(58, 69, 87);

const CONTROL_GROUPS: [(&str, &[SoundControl]); 4] = [
    (
        "TONE",
        &[
            SoundControl::Pitch,
            SoundControl::Sweep,
            SoundControl::Brightness,
            SoundControl::Noise,
        ],
    ),
    (
        "ENVELOPE",
        &[
            SoundControl::Attack,
            SoundControl::Decay,
            SoundControl::Sustain,
            SoundControl::Release,
        ],
    ),
    (
        "MOTION & HARMONY",
        &[
            SoundControl::PulseRate,
            SoundControl::PulseDepth,
            SoundControl::Harmony,
            SoundControl::HarmonyMix,
        ],
    ),
    ("OUTPUT", &[SoundControl::Duration, SoundControl::Volume]),
];

#[derive(Debug, Default)]
pub struct StudioUiState {
    pub focus: usize,
    pub hovered: Option<usize>,
    pub scroll: u16,
    pub dragging: Option<SoundControl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioPreset {
    Glass,
    Orbit,
    Wood,
    Beacon,
}

impl StudioPreset {
    pub const ALL: [Self; 4] = [Self::Glass, Self::Orbit, Self::Wood, Self::Beacon];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Glass => "Glass",
            Self::Orbit => "Orbit",
            Self::Wood => "Wood",
            Self::Beacon => "Beacon",
        }
    }

    /// Complete designs, rather than ornamental choices disconnected from PCM.
    pub fn design(self) -> SoundDesign {
        match self {
            Self::Glass => SoundDesign {
                waveform: Waveform::Sine,
                pitch_hz: 1040,
                pitch_slide_cents: 100,
                attack_ms: 4,
                decay_ms: 170,
                sustain_percent: 20,
                release_ms: 270,
                pulse_rate_tenths_hz: 0,
                pulse_depth_percent: 0,
                harmony_semitones: 12,
                harmony_mix_percent: 32,
                brightness_percent: 22,
                noise_percent: 0,
                duration_ms: 650,
                volume_percent: 55,
            },
            Self::Orbit => SoundDesign {
                waveform: Waveform::Triangle,
                pitch_hz: 440,
                pitch_slide_cents: 950,
                attack_ms: 65,
                decay_ms: 130,
                sustain_percent: 60,
                release_ms: 260,
                pulse_rate_tenths_hz: 55,
                pulse_depth_percent: 45,
                harmony_semitones: 7,
                harmony_mix_percent: 30,
                brightness_percent: 65,
                noise_percent: 3,
                duration_ms: 950,
                volume_percent: 60,
            },
            Self::Wood => SoundDesign {
                waveform: Waveform::Triangle,
                pitch_hz: 290,
                pitch_slide_cents: -700,
                attack_ms: 2,
                decay_ms: 120,
                sustain_percent: 8,
                release_ms: 100,
                pulse_rate_tenths_hz: 0,
                pulse_depth_percent: 0,
                harmony_semitones: 0,
                harmony_mix_percent: 0,
                brightness_percent: 38,
                noise_percent: 18,
                duration_ms: 280,
                volume_percent: 65,
            },
            Self::Beacon => SoundDesign {
                waveform: Waveform::Square,
                pitch_hz: 660,
                pitch_slide_cents: 0,
                attack_ms: 12,
                decay_ms: 60,
                sustain_percent: 75,
                release_ms: 110,
                pulse_rate_tenths_hz: 40,
                pulse_depth_percent: 90,
                harmony_semitones: 5,
                harmony_mix_percent: 15,
                brightness_percent: 70,
                noise_percent: 0,
                duration_ms: 1000,
                volume_percent: 50,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioAction {
    EditNumber(SoundControl),
    SetValue(SoundControl, i32),
    SetPosition {
        control: SoundControl,
        numerator: u16,
        denominator: u16,
    },
    Preset(StudioPreset),
    ToggleEvent(SoundEvent),
    Play,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioTarget {
    Waveform(i32),
    Preset(StudioPreset),
    Slider(SoundControl),
    Event(SoundEvent),
    Play,
    Save,
}

pub fn focus_targets() -> Vec<StudioTarget> {
    (0..4)
        .map(StudioTarget::Waveform)
        .chain(StudioPreset::ALL.into_iter().map(StudioTarget::Preset))
        .chain(
            CONTROL_GROUPS
                .iter()
                .flat_map(|(_, controls)| controls.iter().copied())
                .map(StudioTarget::Slider),
        )
        .chain(SoundEvent::ALL.into_iter().map(StudioTarget::Event))
        .chain([StudioTarget::Play, StudioTarget::Save])
        .collect()
}

impl StudioUiState {
    pub fn move_focus(&mut self, area: Rect, direction: i32) {
        let count = focus_targets().len();
        self.focus =
            (self.focus as i64 + i64::from(direction.signum())).rem_euclid(count as i64) as usize;
        self.reveal_focus(area);
    }

    pub fn reveal_focus(&mut self, area: Rect) {
        let geometry = geometry(area, self);
        if let Some(element) = geometry
            .elements
            .iter()
            .find(|element| element.focus == Some(self.focus))
        {
            if element.pinned {
                self.scroll = self.scroll.min(geometry.max_scroll());
                return;
            }
            let start = element.logical_y;
            let end = start.saturating_add(element.height);
            let height = geometry.viewport.height;
            if start < self.scroll {
                self.scroll = start;
            } else if end > self.scroll.saturating_add(height) {
                self.scroll = end.saturating_sub(height);
            }
        }
        self.scroll = self.scroll.min(geometry.max_scroll());
    }

    pub fn scroll_by(&mut self, area: Rect, amount: i32) {
        self.scroll = i32::from(self.scroll)
            .saturating_add(amount)
            .clamp(0, i32::from(geometry(area, self).max_scroll())) as u16;
    }

    pub fn focused_action(&self) -> Option<StudioAction> {
        action_for_target(*focus_targets().get(self.focus)?)
    }

    /// Keyboard edits use exactly the existing control's step policy.
    pub fn adjust_focused(&self, studio: &SoundStudio, direction: i32) -> Option<StudioAction> {
        match *focus_targets().get(self.focus)? {
            StudioTarget::Slider(control) => {
                let mut design = studio.draft.design.clone();
                control.adjust(&mut design, direction);
                Some(StudioAction::SetValue(control, control.value(&design)))
            }
            _ => self.focused_action(),
        }
    }
}

fn action_for_target(target: StudioTarget) -> Option<StudioAction> {
    Some(match target {
        StudioTarget::Waveform(value) => StudioAction::SetValue(SoundControl::Waveform, value),
        StudioTarget::Preset(preset) => StudioAction::Preset(preset),
        StudioTarget::Slider(_) => return None,
        StudioTarget::Event(event) => StudioAction::ToggleEvent(event),
        StudioTarget::Play => StudioAction::Play,
        StudioTarget::Save => StudioAction::Save,
    })
}

#[derive(Debug, Clone, Copy)]
enum ElementKind {
    Heading(&'static str),
    WavePlot,
    Envelope,
    Target(StudioTarget),
}

#[derive(Debug)]
struct Element {
    kind: ElementKind,
    x: u16,
    width: u16,
    logical_y: u16,
    height: u16,
    focus: Option<usize>,
    pinned: bool,
}

#[derive(Debug)]
struct StudioGeometry {
    viewport: Rect,
    elements: Vec<Element>,
    content_height: u16,
}

impl StudioGeometry {
    fn max_scroll(&self) -> u16 {
        self.content_height.saturating_sub(self.viewport.height)
    }

    fn rect(&self, element: &Element, scroll: u16) -> Option<Rect> {
        if element.pinned {
            return Some(Rect::new(
                element.x,
                element.logical_y,
                element.width,
                element.height,
            ));
        }
        // Interactive rows are only painted when their whole hit rectangle fits.
        // This avoids a clipped label presenting an invisible gesture target.
        let y = element.logical_y.checked_sub(scroll)?;
        if y.saturating_add(element.height) > self.viewport.height {
            return None;
        }
        Some(Rect::new(
            element.x,
            self.viewport.y + y,
            element.width,
            element.height,
        ))
    }
}

fn geometry(area: Rect, _ui: &StudioUiState) -> StudioGeometry {
    let top = area.height.min(1);
    let footer = area.height.saturating_sub(top).min(2);
    let viewport = Rect::new(
        area.x,
        area.y + top,
        area.width,
        area.height.saturating_sub(top + footer),
    );
    let targets = focus_targets();
    let mut elements = Vec::new();
    let mut push = |kind, x, width, logical_y, height, pinned| {
        let focus = match kind {
            ElementKind::Target(target) => targets.iter().position(|value| *value == target),
            _ => None,
        };
        elements.push(Element {
            kind,
            x,
            width,
            logical_y,
            height,
            focus,
            pinned,
        });
    };
    // Reserve a separate scrollbar cell so it never paints over a control's
    // hit rectangle, even at the narrowest supported wizard width.
    let width = area.width.saturating_sub(u16::from(area.width > 1));
    let x = area.x;
    let wide = width >= 88;
    push(
        ElementKind::Heading("OSCILLATOR  /  shape your signal"),
        x,
        width,
        0,
        1,
        false,
    );
    for index in 0..4 {
        let left = width * index / 4;
        let right = width * (index + 1) / 4;
        push(
            ElementKind::Target(StudioTarget::Waveform(i32::from(index))),
            x + left,
            right - left,
            1,
            1,
            false,
        );
    }
    let preview_height = if wide {
        7
    } else if viewport.height >= 11 {
        4
    } else {
        3
    };
    let preview_gap = u16::from(!wide && viewport.height >= 11);
    if wide {
        let first = width * 2 / 3;
        push(
            ElementKind::WavePlot,
            x,
            first.saturating_sub(1),
            3,
            preview_height,
            false,
        );
        push(
            ElementKind::Envelope,
            x + first + 1,
            width.saturating_sub(first + 1),
            3,
            preview_height,
            false,
        );
    } else {
        push(ElementKind::WavePlot, x, width, 3, preview_height, false);
        push(
            ElementKind::Envelope,
            x,
            width,
            3 + preview_height + preview_gap,
            preview_height,
            false,
        );
    }
    let presets_y = if wide {
        3 + preview_height
    } else {
        3 + preview_height * 2 + preview_gap
    };
    push(
        ElementKind::Heading("STARTING POINTS  /  then make it yours"),
        x,
        width,
        presets_y,
        1,
        false,
    );
    for (index, preset) in StudioPreset::ALL.into_iter().enumerate() {
        let left = width * index as u16 / 4;
        let right = width * (index as u16 + 1) / 4;
        push(
            ElementKind::Target(StudioTarget::Preset(preset)),
            x + left,
            right - left,
            presets_y + 1,
            1,
            false,
        );
    }
    let mut current_y = presets_y + 3;
    for (group_index, (label, controls)) in CONTROL_GROUPS.iter().enumerate() {
        let column = if wide { group_index % 2 } else { 0 };
        let column_width = if wide { (width - 2) / 2 } else { width };
        let column_x = x + column as u16 * (column_width + 2);
        push(
            ElementKind::Heading(label),
            column_x,
            column_width,
            current_y,
            1,
            false,
        );
        for (index, control) in controls.iter().enumerate() {
            push(
                ElementKind::Target(StudioTarget::Slider(*control)),
                column_x,
                column_width,
                current_y + 1 + index as u16 * 2,
                2,
                false,
            );
        }
        if !wide || column == 1 {
            let rows = if wide {
                controls.len().max(CONTROL_GROUPS[group_index - 1].1.len())
            } else {
                controls.len()
            };
            current_y += 2 + rows as u16 * 2;
        }
    }
    push(
        ElementKind::Heading("PLAY THIS SOUND WHEN…"),
        x,
        width,
        current_y,
        1,
        false,
    );
    current_y += 1;
    for (index, event) in SoundEvent::ALL.into_iter().enumerate() {
        let column = if wide { index % 2 } else { 0 };
        let column_width = if wide { (width - 2) / 2 } else { width };
        let y = current_y + if wide { index as u16 / 2 } else { index as u16 };
        push(
            ElementKind::Target(StudioTarget::Event(event)),
            x + column as u16 * (column_width + 2),
            column_width,
            y,
            1,
            false,
        );
    }
    let content_height = current_y + if wide { 3 } else { 6 };
    let footer_y = area.bottom().saturating_sub(footer);
    let half = width / 2;
    push(
        ElementKind::Target(StudioTarget::Play),
        x,
        half,
        footer_y,
        footer.min(1),
        true,
    );
    push(
        ElementKind::Target(StudioTarget::Save),
        x + half,
        width - half,
        footer_y,
        footer.min(1),
        true,
    );
    StudioGeometry {
        viewport,
        elements,
        content_height,
    }
}

/// Returns the keyboard index under the pointer, including slider labels.
pub fn focus_at(area: Rect, ui: &StudioUiState, position: Position) -> Option<usize> {
    let geometry = geometry(area, ui);
    geometry.elements.iter().find_map(|element| {
        let rect = geometry.rect(element, ui.scroll.min(geometry.max_scroll()))?;
        (rect.contains(position)).then_some(element.focus).flatten()
    })
}

fn slider_track(rect: Rect) -> Rect {
    Rect::new(
        rect.x + u16::from(rect.width > 2),
        rect.y + 1,
        rect.width.saturating_sub(2).max(u16::from(rect.width > 0)),
        1,
    )
}

fn numeric_control_at(
    area: Rect,
    studio: &SoundStudio,
    ui: &StudioUiState,
    control: SoundControl,
) -> Option<(Rect, crate::value_control::ValueControl)> {
    use crate::value_control::{ControlKind, ControlSpec, ValueControl};
    let geometry = geometry(area, ui);
    let element = geometry.elements.iter().find(|element| {
        matches!(element.kind,
        ElementKind::Target(StudioTarget::Slider(value)) if value == control)
    })?;
    let rect = geometry.rect(element, ui.scroll.min(geometry.max_scroll()))?;
    let (minimum, maximum) = control.range();
    let value = control.value(&studio.draft.design);
    let display = control.display(&studio.draft.design);
    let chrome = ValueControl::new(
        Rect::new(rect.x, rect.y, rect.width, 1),
        ControlSpec {
            kind: ControlKind::Number,
            label: control.label(),
            value: &display,
            label_width: (rect.width / 2).min(20),
            previous_enabled: value > minimum,
            next_enabled: value < maximum,
            open_enabled: true,
        },
    );
    Some((rect, chrome))
}

/// Only a track press starts dragging; clicking the exact-entry star cannot.
pub fn is_slider_track(area: Rect, ui: &StudioUiState, position: Position) -> bool {
    let geometry = geometry(area, ui);
    geometry.elements.iter().any(|element| {
        matches!(element.kind, ElementKind::Target(StudioTarget::Slider(_)))
            && geometry
                .rect(element, ui.scroll.min(geometry.max_scroll()))
                .is_some_and(|rect| slider_track(rect).contains(position))
    })
}

pub fn control_hit(
    area: Rect,
    studio: &SoundStudio,
    ui: &StudioUiState,
    position: Position,
    button: crate::value_control::PointerButton,
) -> Option<StudioAction> {
    use crate::value_control::ControlAction;
    for control in SoundControl::ALL
        .into_iter()
        .filter(|value| *value != SoundControl::Waveform)
    {
        let Some((_, chrome)) = numeric_control_at(area, studio, ui, control) else {
            continue;
        };
        let Some(action) = chrome.hit(position, button) else {
            continue;
        };
        return match action {
            ControlAction::EditNumber => Some(StudioAction::EditNumber(control)),
            ControlAction::Decrement | ControlAction::Increment => {
                let mut design = studio.draft.design.clone();
                control.adjust(
                    &mut design,
                    if action == ControlAction::Decrement {
                        -1
                    } else {
                        1
                    },
                );
                Some(StudioAction::SetValue(control, control.value(&design)))
            }
            _ => None,
        };
    }
    None
}

fn slider_action(control: SoundControl, track: Rect, column: u16) -> StudioAction {
    let denominator = track.width.saturating_sub(1);
    StudioAction::SetPosition {
        control,
        numerator: column.saturating_sub(track.x).min(denominator),
        denominator,
    }
}

pub fn hit(area: Rect, ui: &StudioUiState, position: Position) -> Option<StudioAction> {
    let geometry = geometry(area, ui);
    for element in &geometry.elements {
        let Some(rect) = geometry.rect(element, ui.scroll.min(geometry.max_scroll())) else {
            continue;
        };
        if !rect.contains(position) {
            continue;
        }
        if let ElementKind::Target(target) = element.kind {
            if let StudioTarget::Slider(control) = target {
                let track = slider_track(rect);
                return track
                    .contains(position)
                    .then(|| slider_action(control, track, position.x));
            }
            return action_for_target(target);
        }
    }
    None
}

/// Drag remains bound to the original slider even when the pointer leaves it.
/// Parent sets `dragging` on slider press and clears it on mouse release.
pub fn drag(area: Rect, ui: &StudioUiState, position: Position) -> Option<StudioAction> {
    let control = ui.dragging?;
    let geometry = geometry(area, ui);
    let element = geometry.elements.iter().find(|element| matches!(element.kind, ElementKind::Target(StudioTarget::Slider(value)) if value == control))?;
    let rect = geometry.rect(element, ui.scroll.min(geometry.max_scroll()))?;
    Some(slider_action(control, slider_track(rect), position.x))
}

pub fn render(frame: &mut Frame, area: Rect, studio: &SoundStudio, ui: &StudioUiState) {
    if area.is_empty() {
        return;
    }
    frame.render_widget(Block::default().style(Style::new().bg(CANVAS)), area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" SOUND STUDIO ", Style::new().fg(ACCENT).bold()),
            Span::styled("/ custom notification", Style::new().fg(MUTED)),
        ])),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let geometry = geometry(area, ui);
    let scroll = ui.scroll.min(geometry.max_scroll());
    for element in &geometry.elements {
        let Some(rect) = geometry.rect(element, scroll) else {
            continue;
        };
        if rect.is_empty() {
            continue;
        }
        let focused = element.focus == Some(ui.focus);
        let hovered = element.focus.is_some() && element.focus == ui.hovered;
        let style = if focused || hovered {
            Style::new()
                .bg(ACCENT)
                .fg(CANVAS)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().bg(PANEL).fg(INK)
        };
        match element.kind {
            ElementKind::Heading(label) => frame.render_widget(
                Paragraph::new(label).style(Style::new().fg(ACCENT).bold()),
                rect,
            ),
            ElementKind::WavePlot => render_wave(frame, rect, studio),
            ElementKind::Envelope => render_envelope(frame, rect, &studio.draft.design),
            ElementKind::Target(StudioTarget::Slider(control)) => {
                if let Some((_, chrome)) = numeric_control_at(area, studio, ui, control) {
                    chrome.render(
                        frame,
                        crate::value_control::ControlStyles {
                            background: style,
                            label: style,
                            value: style,
                            button: style,
                            disabled: Style::new().bg(PANEL).fg(MUTED),
                        },
                    );
                }
                let track = slider_track(rect);
                let position = (control.position(&studio.draft.design)
                    * f64::from(track.width.saturating_sub(1)))
                .round() as u16;
                let spans = (0..track.width)
                    .map(|index| {
                        let glyph = if index == position { "●" } else { "━" };
                        Span::styled(
                            glyph,
                            Style::new().fg(if index <= position { ACCENT } else { TRACK }),
                        )
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(Paragraph::new(Line::from(spans)), track);
            }
            ElementKind::Target(target) => {
                let (label, selected) = match target {
                    StudioTarget::Waveform(value) => {
                        let names = ["∿ Sine", "△ Triangle", "╱ Saw", "⊓ Square"];
                        (
                            names[value as usize].to_string(),
                            SoundControl::Waveform.value(&studio.draft.design) == value,
                        )
                    }
                    StudioTarget::Preset(preset) => (
                        preset.label().into(),
                        studio.draft.design == preset.design(),
                    ),
                    StudioTarget::Event(event) => {
                        let enabled = studio.draft.events.is_enabled(event);
                        (
                            format!("{} {}", if enabled { "☑" } else { "☐" }, event_label(event)),
                            enabled,
                        )
                    }
                    StudioTarget::Play => ("▷ Play sound".into(), false),
                    StudioTarget::Save => ("✓ Save design".into(), false),
                    StudioTarget::Slider(_) => continue,
                };
                let style = if selected && !focused && !hovered {
                    style.fg(ACCENT).bold()
                } else {
                    style
                };
                frame.render_widget(Paragraph::new(label).style(style).centered(), rect);
            }
        }
    }
    if area.height >= 3 {
        let footer = if geometry.max_scroll() > 0 {
            format!(
                " Tab focus · ←/→ edit · scroll {}/{}",
                scroll,
                geometry.max_scroll()
            )
        } else {
            " Tab focus · ←/→ edit · Enter activate".into()
        };
        frame.render_widget(
            Paragraph::new(footer).style(Style::new().fg(MUTED)),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }
    if geometry.max_scroll() > 0 && geometry.viewport.height > 0 && area.width > 0 {
        let travel = geometry.viewport.height.saturating_sub(1);
        let y = geometry.viewport.y
            + (u32::from(scroll) * u32::from(travel) / u32::from(geometry.max_scroll())) as u16;
        frame.render_widget(
            Paragraph::new("▐").style(Style::new().fg(ACCENT)),
            Rect::new(area.right() - 1, y, 1, 1),
        );
    }
}

fn event_label(event: SoundEvent) -> &'static str {
    match event {
        SoundEvent::AgentFinished => "Agent finished",
        SoundEvent::ApprovalRequired => "Approval needed",
        SoundEvent::AgentStarted => "Agent started",
        SoundEvent::WaitingBackground => "Background waiting",
        SoundEvent::TaskSucceeded => "Task succeeded",
        SoundEvent::TaskFailed => "Task failed / lost",
    }
}

fn render_wave(frame: &mut Frame, rect: Rect, studio: &SoundStudio) {
    let block = crate::theme::block(false)
        .title(crate::theme::chrome_title("Waveform"))
        .style(Style::new().bg(PANEL));
    let content = block.inner(rect);
    frame.render_widget(block, rect);
    let preview = studio.preview_columns();
    if preview.is_empty() || content.is_empty() {
        return;
    }
    let peak = preview
        .iter()
        .map(|column| i32::from(column.min).abs().max(i32::from(column.max).abs()))
        .max()
        .unwrap_or(0);
    let plot = if content.height >= 4 {
        frame.render_widget(
            Paragraph::new(format!(
                " PCM  ·  {} ms  ·  peak {:.0}%",
                studio.draft.design.duration_ms,
                f64::from(peak) * 100.0 / 32767.0
            ))
            .style(Style::new().fg(MUTED)),
            Rect::new(content.x, content.y, content.width, 1),
        );
        Rect::new(content.x, content.y + 1, content.width, content.height - 1)
    } else {
        content
    };
    if plot.is_empty() {
        return;
    }
    for column in 0..plot.width {
        // Aggregate cached extrema; never discard a peak during a narrow resize.
        let start = usize::from(column) * preview.len() / usize::from(plot.width);
        let end = ((usize::from(column) + 1) * preview.len() / usize::from(plot.width))
            .max(start + 1)
            .min(preview.len());
        let samples = &preview[start.min(preview.len() - 1)..end];
        let low = samples.iter().map(|value| value.min).min().unwrap_or(0);
        let high = samples.iter().map(|value| value.max).max().unwrap_or(0);
        let to_row = |sample: i16| {
            ((1.0 - f64::from(sample) / 32768.0) * 0.5 * f64::from(plot.height.saturating_sub(1)))
                .round() as u16
        };
        let top = to_row(high).min(plot.height - 1);
        let bottom = to_row(low).min(plot.height - 1);
        for row in 0..plot.height {
            let (glyph, color) = if row >= top && row <= bottom {
                ("│", SIGNAL)
            } else if row == plot.height / 2 {
                ("─", TRACK)
            } else {
                (" ", PANEL)
            };
            frame.render_widget(
                Paragraph::new(glyph).style(Style::new().fg(color).bg(PANEL)),
                Rect::new(plot.x + column, plot.y + row, 1, 1),
            );
        }
    }
}

fn render_envelope(frame: &mut Frame, rect: Rect, design: &SoundDesign) {
    let block = crate::theme::block(false)
        .title(crate::theme::chrome_title("Envelope"))
        .style(Style::new().bg(PANEL));
    let content = block.inner(rect);
    frame.render_widget(block, rect);
    let height = content.height;
    if height == 0 || content.width == 0 {
        return;
    }
    let duration = f64::from(design.duration_ms);
    let scale =
        (duration / f64::from(design.attack_ms + design.decay_ms + design.release_ms)).min(1.0);
    let attack = f64::from(design.attack_ms) * scale;
    let decay = f64::from(design.decay_ms) * scale;
    let release = f64::from(design.release_ms) * scale;
    let sustain = f64::from(design.sustain_percent) / 100.0;
    for column in 0..content.width {
        let time = f64::from(column) / f64::from(content.width.saturating_sub(1).max(1)) * duration;
        let level = if time < attack {
            time / attack
        } else if time < attack + decay {
            1.0 - (1.0 - sustain) * (time - attack) / decay
        } else if time < duration - release {
            sustain
        } else {
            sustain * (duration - time) / release
        };
        let row = ((1.0 - level.clamp(0.0, 1.0)) * f64::from(height - 1)).round() as u16;
        frame.render_widget(
            Paragraph::new("•").style(Style::new().fg(ACCENT)),
            Rect::new(content.x + column, content.y + row, 1, 1),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn every_control_is_reachable_at_minimum_and_wide_sizes() {
        for area in [
            Rect::new(0, 0, 38, 9),
            Rect::new(0, 0, 80, 16),
            Rect::new(0, 0, 118, 33),
            Rect::new(3, 2, 198, 53),
        ] {
            let mut ui = StudioUiState::default();
            for index in 0..focus_targets().len() {
                ui.focus = index;
                ui.reveal_focus(area);
                let geometry = geometry(area, &ui);
                let element = geometry
                    .elements
                    .iter()
                    .find(|item| item.focus == Some(index))
                    .unwrap();
                let rect = geometry.rect(element, ui.scroll).unwrap();
                assert!(area.contains(Position::new(rect.x, rect.y)));
                assert_eq!(
                    focus_at(area, &ui, Position::new(rect.x, rect.y)),
                    Some(index)
                );
            }
        }
    }

    #[test]
    fn numeric_chrome_and_drag_track_use_separate_exact_hit_regions() {
        use crate::value_control::{ControlAction, PointerButton};
        let studio = SoundStudio::new(ilium_sound::SoundSettings::default());
        for width in [24, 40, 80, 140] {
            let area = Rect::new(0, 0, width, 28);
            let mut terminal = Terminal::new(TestBackend::new(width, 28)).unwrap();
            for control in SoundControl::ALL
                .into_iter()
                .filter(|value| *value != SoundControl::Waveform)
            {
                let mut ui = StudioUiState {
                    focus: focus_targets()
                        .iter()
                        .position(|target| *target == StudioTarget::Slider(control))
                        .unwrap(),
                    ..StudioUiState::default()
                };
                ui.reveal_focus(area);
                let (_, chrome) = numeric_control_at(area, &studio, &ui, control).unwrap();
                terminal
                    .draw(|frame| render(frame, area, &studio, &ui))
                    .unwrap();
                let geometry = chrome.geometry();
                assert_eq!(
                    terminal.backend().buffer()[(geometry.open.x, geometry.open.y)].symbol(),
                    "*"
                );
                assert_eq!(
                    chrome.hit(
                        Position::new(geometry.open.x, geometry.open.y),
                        PointerButton::Left
                    ),
                    Some(ControlAction::EditNumber)
                );
                assert_eq!(
                    control_hit(
                        area,
                        &studio,
                        &ui,
                        Position::new(geometry.open.x, geometry.open.y),
                        PointerButton::Left
                    ),
                    Some(StudioAction::EditNumber(control))
                );
                assert!(!is_slider_track(
                    area,
                    &ui,
                    Position::new(geometry.open.x, geometry.open.y)
                ));
                let track =
                    slider_track(numeric_control_at(area, &studio, &ui, control).unwrap().0);
                assert!(is_slider_track(area, &ui, Position::new(track.x, track.y)));
                assert!(hit(area, &ui, Position::new(track.x, track.y)).is_some());
            }
        }
    }

    #[test]
    fn slider_gestures_reach_endpoints_and_drag_clamps_outside_track() {
        let area = Rect::new(0, 0, 80, 24);
        for control in SoundControl::ALL
            .into_iter()
            .filter(|value| *value != SoundControl::Waveform)
        {
            let mut ui = StudioUiState {
                focus: focus_targets()
                    .iter()
                    .position(|value| *value == StudioTarget::Slider(control))
                    .unwrap(),
                dragging: Some(control),
                ..StudioUiState::default()
            };
            ui.reveal_focus(area);
            let mut design = SoundDesign::default();
            for (x, expected) in [(0, control.range().0), (u16::MAX, control.range().1)] {
                let StudioAction::SetPosition {
                    numerator,
                    denominator,
                    ..
                } = drag(area, &ui, Position::new(x, 0)).unwrap()
                else {
                    panic!("slider action");
                };
                control.set_position(&mut design, numerator, denominator);
                assert_eq!(control.value(&design), expected);
            }
        }
    }

    #[test]
    fn painted_controls_and_mouse_targets_share_rectangles() {
        let studio = SoundStudio::new(ilium_sound::SoundSettings::default());
        let area = Rect::new(0, 0, 40, 16);
        let mut terminal = Terminal::new(TestBackend::new(40, 16)).unwrap();
        let mut ui = StudioUiState::default();
        for index in 0..focus_targets().len() {
            ui.focus = index;
            ui.reveal_focus(area);
            terminal
                .draw(|frame| render(frame, area, &studio, &ui))
                .unwrap();
            let geometry = geometry(area, &ui);
            let element = geometry
                .elements
                .iter()
                .find(|value| value.focus == Some(index))
                .unwrap();
            let rect = geometry.rect(element, ui.scroll).unwrap();
            let position = match element.kind {
                ElementKind::Target(StudioTarget::Slider(_)) => {
                    Position::new(slider_track(rect).x, rect.y + 1)
                }
                _ => Position::new(rect.x + rect.width / 2, rect.y),
            };
            assert!(hit(area, &ui, position).is_some());
            assert_eq!(focus_at(area, &ui, position), Some(index));
            let cell = &terminal.backend().buffer()[(position.x, position.y)];
            if matches!(element.kind, ElementKind::Target(StudioTarget::Slider(_))) {
                assert_ne!(cell.symbol(), " ");
            } else {
                // Centered button labels can contain spaces; their full painted
                // background still establishes the actual click rectangle.
                assert_eq!(cell.bg, ACCENT);
            }
        }
    }

    #[test]
    fn keyboard_focus_wraps_and_edits_use_the_real_control_step() {
        let area = Rect::new(0, 0, 38, 9);
        let studio = SoundStudio::new(ilium_sound::SoundSettings::default());
        let mut ui = StudioUiState::default();
        assert_eq!(
            ui.focused_action(),
            Some(StudioAction::SetValue(SoundControl::Waveform, 0))
        );
        ui.move_focus(area, -1);
        assert_eq!(ui.focused_action(), Some(StudioAction::Save));
        ui.move_focus(area, 1);
        assert_eq!(ui.focus, 0);
        ui.focus = focus_targets()
            .iter()
            .position(|target| *target == StudioTarget::Slider(SoundControl::Pitch))
            .unwrap();
        ui.reveal_focus(area);
        assert_eq!(
            ui.adjust_focused(&studio, 1),
            Some(StudioAction::SetValue(
                SoundControl::Pitch,
                i32::from(studio.draft.design.pitch_hz) + 20
            ))
        );
        ui.scroll_by(area, i32::MAX);
        assert_eq!(ui.scroll, geometry(area, &ui).max_scroll());
        ui.scroll_by(area, i32::MIN);
        assert_eq!(ui.scroll, 0);
    }

    #[test]
    fn render_handles_empty_tiny_and_extremely_wide_viewports() {
        let studio = SoundStudio::new(ilium_sound::SoundSettings::default());
        for (width, height) in [(1, 1), (2, 3), (40, 16), (200, 60), (500, 16)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render(frame, frame.area(), &studio, &StudioUiState::default()))
                .unwrap();
        }
    }

    fn preview_rects(area: Rect) -> [Rect; 2] {
        let geometry = geometry(area, &StudioUiState::default());
        [ElementKind::WavePlot, ElementKind::Envelope].map(|kind| {
            let element = geometry
                .elements
                .iter()
                .find(|element| {
                    matches!(
                        (element.kind, kind),
                        (ElementKind::WavePlot, ElementKind::WavePlot)
                            | (ElementKind::Envelope, ElementKind::Envelope)
                    )
                })
                .expect("preview element");
            geometry.rect(element, 0).expect("visible preview")
        })
    }

    #[test]
    fn sound_studio_previews_use_titled_rounded_frames_at_regular_and_compact_sizes() {
        let studio = SoundStudio::new(ilium_sound::SoundSettings::default());
        for area in [Rect::new(0, 0, 118, 33), Rect::new(0, 0, 40, 16)] {
            let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
            terminal
                .draw(|frame| render(frame, area, &studio, &StudioUiState::default()))
                .unwrap();

            let buffer = terminal.backend().buffer();
            let [waveform, envelope] = preview_rects(area);
            for (kind, title) in [
                (ElementKind::WavePlot, "Waveform"),
                (ElementKind::Envelope, "Envelope"),
            ] {
                let rect = if matches!(kind, ElementKind::WavePlot) {
                    waveform
                } else {
                    envelope
                };
                assert_eq!(
                    buffer[(rect.x, rect.y)].symbol(),
                    "╭",
                    "{title} at {area:?}"
                );
                assert_eq!(
                    buffer[(rect.x, rect.bottom() - 1)].symbol(),
                    "╰",
                    "{title} at {area:?}"
                );
                assert_eq!(
                    buffer[(rect.right() - 1, rect.y)].symbol(),
                    "╮",
                    "{title} at {area:?}",
                );
                assert_eq!(
                    buffer[(rect.right() - 1, rect.bottom() - 1)].symbol(),
                    "╯",
                    "{title} at {area:?}"
                );
                let rendered: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
                assert!(rendered.contains(title), "missing {title} at {area:?}");
            }
            crate::ui_capture::save(
                &format!(
                    "sound-studio-previews-framed-{}x{}",
                    area.width, area.height
                ),
                &terminal,
            );
        }
    }

    #[test]
    fn compact_sound_studio_previews_keep_a_row_between_their_frames() {
        let [waveform, envelope] = preview_rects(Rect::new(0, 0, 40, 16));
        assert!(waveform.bottom() < envelope.y);
    }
}
