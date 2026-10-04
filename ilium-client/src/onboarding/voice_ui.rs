//! Configuration rows and the isolated live lightbulb demonstration.
//! Every displayed connection/transcript/tool result comes from the runtime.

use super::voice_runtime::VoiceDemoState;
use crate::{config::VoiceSettings, voice_settings::VoiceRow};
use ilium_voice::{VoiceConnectionState, VoiceInputMode};
use ratatui::{
    layout::{Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
    Frame,
};
use std::borrow::Cow;

const CANVAS: Color = Color::Rgb(13, 16, 23);
const PANEL: Color = Color::Rgb(24, 29, 40);
const INK: Color = Color::Rgb(224, 231, 244);
const MUTED: Color = Color::Rgb(144, 157, 179);
const ACCENT: Color = Color::Rgb(242, 188, 105);
const SUCCESS: Color = Color::Rgb(116, 210, 194);

pub const CONFIG_ROWS: [VoiceRow; 11] = [
    VoiceRow::Enabled,
    VoiceRow::ApiKey,
    VoiceRow::Model,
    VoiceRow::Voice,
    VoiceRow::InputMode,
    VoiceRow::InputDevice,
    VoiceRow::OutputDevice,
    VoiceRow::OutputVolume,
    VoiceRow::ReasoningEffort,
    VoiceRow::VadEagerness,
    VoiceRow::PauseMediaWhileActive,
];

#[derive(Debug, Default)]
pub struct VoiceUiState {
    pub focus: usize,
    pub hovered: Option<usize>,
    pub scroll: u16,
    /// Independent reading offset for the wide transcript column.
    pub scene_scroll: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceAction {
    Configure(VoiceRow),
    Test,
    Stop,
    StartPushToTalk,
    StopPushToTalk,
}

/// Configuration targets, followed by Test, Stop and Record/Send.
pub const TARGET_COUNT: usize = CONFIG_ROWS.len() + 3;

impl VoiceUiState {
    pub fn move_focus(&mut self, area: Rect, state: &VoiceDemoState, direction: i32) {
        self.focus = (self.focus as i64 + i64::from(direction.signum()))
            .rem_euclid(TARGET_COUNT as i64) as usize;
        self.reveal_focus(area, state);
    }

    pub fn reveal_focus(&mut self, area: Rect, state: &VoiceDemoState) {
        let geometry = Geometry::new(area, state);
        if self.focus < CONFIG_ROWS.len() {
            let start = self.focus as u16 * 2;
            let end = start + 2;
            if start < self.scroll {
                self.scroll = start;
            } else if end > self.scroll.saturating_add(geometry.body.height) {
                self.scroll = end.saturating_sub(geometry.body.height);
            }
        }
        self.scroll = self.scroll.min(geometry.max_scroll());
        self.scene_scroll = self.scene_scroll.min(geometry.max_scene_scroll());
    }

    /// Page keys scroll the reading panel; narrow screens scroll the shared
    /// stacked content. Configuration focus reveal remains independent.
    pub fn scroll_by(&mut self, area: Rect, state: &VoiceDemoState, amount: i32) {
        let geometry = Geometry::new(area, state);
        if geometry.scene.is_some() {
            self.scene_scroll =
                adjusted_scroll(self.scene_scroll, amount, geometry.max_scene_scroll());
        } else {
            self.scroll = adjusted_scroll(self.scroll, amount, geometry.max_scroll());
        }
    }

    /// Mouse wheel scrolling follows the column under the pointer.
    pub fn scroll_at(
        &mut self,
        area: Rect,
        state: &VoiceDemoState,
        amount: i32,
        position: Position,
    ) {
        let geometry = Geometry::new(area, state);
        if geometry.scene.is_some_and(|scene| scene.contains(position)) {
            self.scene_scroll =
                adjusted_scroll(self.scene_scroll, amount, geometry.max_scene_scroll());
        } else {
            self.scroll = adjusted_scroll(self.scroll, amount, geometry.max_scroll());
        }
    }

    pub fn focused_action(
        &self,
        state: &VoiceDemoState,
        settings: &VoiceSettings,
    ) -> Option<VoiceAction> {
        action(self.focus, state, settings)
    }
}

struct Geometry {
    body: Rect,
    controls: [Rect; 3],
    scene: Option<Rect>,
    content_height: u16,
    scene_height: u16,
}

impl Geometry {
    fn new(area: Rect, state: &VoiceDemoState) -> Self {
        let top = area.height.min(1);
        let footer = area.height.saturating_sub(top).min(2);
        let width = area.width.saturating_sub(u16::from(area.width > 1));
        let wide = width >= 88;
        let body_width = if wide { width / 2 } else { width };
        let body = Rect::new(
            area.x,
            area.y + top,
            body_width,
            area.height.saturating_sub(top + footer),
        );
        let scene = wide.then(|| {
            Rect::new(
                area.x + body_width + 2,
                body.y,
                width.saturating_sub(body_width + 2),
                body.height,
            )
        });
        let y = area.bottom().saturating_sub(footer);
        let controls = std::array::from_fn(|index| {
            let left = u32::from(width) * index as u32 / 3;
            let right = u32::from(width) * (index as u32 + 1) / 3;
            Rect::new(
                area.x + left as u16,
                y,
                (right - left) as u16,
                footer.min(1),
            )
        });
        let scene_width = scene.map_or(body.width, |rect| rect.width);
        let scene_height = scene_paragraph(state)
            .line_count(scene_width)
            .min(usize::from(u16::MAX)) as u16;
        let content_height =
            (CONFIG_ROWS.len() as u16 * 2).saturating_add(if wide { 0 } else { scene_height });
        Self {
            body,
            controls,
            scene,
            content_height,
            scene_height,
        }
    }

    fn max_scroll(&self) -> u16 {
        self.content_height.saturating_sub(self.body.height)
    }

    fn max_scene_scroll(&self) -> u16 {
        self.scene
            .map_or(0, |scene| self.scene_height.saturating_sub(scene.height))
    }

    fn config_rect(&self, index: usize, scroll: u16) -> Option<Rect> {
        let offset = (index as u16 * 2).checked_sub(scroll)?;
        if offset + 2 > self.body.height {
            return None;
        }
        Some(Rect::new(
            self.body.x,
            self.body.y + offset,
            self.body.width,
            2,
        ))
    }

    fn scene_rect(&self, scroll: u16, scene_scroll: u16) -> Option<(Rect, u16)> {
        if let Some(scene) = self.scene {
            return Some((scene, scene_scroll.min(self.max_scene_scroll())));
        }
        let start = CONFIG_ROWS.len() as u16 * 2;
        let offset = start.saturating_sub(scroll);
        let clipped = scroll.saturating_sub(start);
        if offset >= self.body.height {
            return None;
        }
        Some((
            Rect::new(
                self.body.x,
                self.body.y + offset,
                self.body.width,
                self.body
                    .height
                    .saturating_sub(offset)
                    .min(self.scene_height.saturating_sub(clipped)),
            ),
            clipped,
        ))
    }
}

pub(crate) fn config_row_area(
    area: Rect,
    ui: &VoiceUiState,
    state: &VoiceDemoState,
    index: usize,
) -> Option<Rect> {
    let geometry = Geometry::new(area, state);
    geometry.config_rect(index, ui.scroll.min(geometry.max_scroll()))
}

pub fn focus_at(
    area: Rect,
    ui: &VoiceUiState,
    state: &VoiceDemoState,
    position: Position,
) -> Option<usize> {
    let geometry = Geometry::new(area, state);
    let scroll = ui.scroll.min(geometry.max_scroll());
    for index in 0..CONFIG_ROWS.len() {
        if geometry
            .config_rect(index, scroll)
            .is_some_and(|rect| rect.contains(position))
        {
            return Some(index);
        }
    }
    geometry
        .controls
        .iter()
        .position(|rect| rect.contains(position))
        .map(|index| CONFIG_ROWS.len() + index)
}

pub fn hit(
    area: Rect,
    ui: &VoiceUiState,
    state: &VoiceDemoState,
    settings: &VoiceSettings,
    position: Position,
) -> Option<VoiceAction> {
    action(focus_at(area, ui, state, position)?, state, settings)
}

fn action(index: usize, state: &VoiceDemoState, settings: &VoiceSettings) -> Option<VoiceAction> {
    if let Some(row) = CONFIG_ROWS.get(index) {
        return Some(VoiceAction::Configure(*row));
    }
    match index.checked_sub(CONFIG_ROWS.len())? {
        0 => Some(VoiceAction::Test),
        1 if state.can_stop || state.is_running => Some(VoiceAction::Stop),
        2 if state.is_running
            && settings.input_mode == VoiceInputMode::PushToTalk
            && matches!(
                state.connection.as_ref(),
                VoiceConnectionState::Listening | VoiceConnectionState::Recording
            ) =>
        {
            Some(
                if matches!(state.connection.as_ref(), VoiceConnectionState::Recording) {
                    VoiceAction::StopPushToTalk
                } else {
                    VoiceAction::StartPushToTalk
                },
            )
        }
        _ => None,
    }
}

pub fn config_rows(settings: &VoiceSettings) -> Vec<(VoiceRow, &'static str, Cow<'_, str>)> {
    CONFIG_ROWS
        .into_iter()
        .map(|row| {
            let (label, value) = match row {
                VoiceRow::Enabled => (
                    "Enable voice after setup",
                    if settings.enabled { "On" } else { "Off" }.into(),
                ),
                VoiceRow::ApiKey => (
                    "OpenAI API key",
                    if settings.api_key.trim().is_empty() {
                        "Not set / environment".into()
                    } else {
                        "••••••••  configured".into()
                    },
                ),
                VoiceRow::Model => ("Realtime model", settings.model.label().into()),
                VoiceRow::Voice => ("Speaking voice", settings.voice.label().into()),
                VoiceRow::InputMode => ("Turn detection", settings.input_mode.label().into()),
                VoiceRow::InputDevice => (
                    "Microphone",
                    settings
                        .input_device_name
                        .as_deref()
                        .unwrap_or("System default")
                        .into(),
                ),
                VoiceRow::OutputDevice => (
                    "Speaker",
                    settings
                        .output_device_name
                        .as_deref()
                        .unwrap_or("System default")
                        .into(),
                ),
                VoiceRow::OutputVolume => (
                    "Output volume",
                    format!("{}%", settings.output_volume_percent).into(),
                ),
                VoiceRow::ReasoningEffort => {
                    ("Reasoning", settings.reasoning_effort.label().into())
                }
                VoiceRow::VadEagerness => {
                    ("VAD responsiveness", settings.vad_eagerness.label().into())
                }
                VoiceRow::PauseMediaWhileActive => (
                    "Pause other media",
                    if settings.pause_media_while_active {
                        "On"
                    } else {
                        "Off"
                    }
                    .into(),
                ),
                _ => unreachable!("CONFIG_ROWS contains only demo configuration fields"),
            };
            (row, label, value)
        })
        .collect()
}

pub fn connection_label(state: &VoiceConnectionState) -> &'static str {
    match state {
        VoiceConnectionState::Disabled => "Ready to test",
        VoiceConnectionState::Connecting => "Connecting…",
        VoiceConnectionState::Listening => "Listening",
        VoiceConnectionState::Recording => "Recording · select Send",
        VoiceConnectionState::Thinking => "Thinking…",
        VoiceConnectionState::Speaking => "Speaking",
        VoiceConnectionState::Failed(_) => "Test failed",
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    settings: &VoiceSettings,
    state: &VoiceDemoState,
    ui: &VoiceUiState,
) {
    if area.is_empty() {
        return;
    }
    let geometry = Geometry::new(area, state);
    let scroll = ui.scroll.min(geometry.max_scroll());
    frame.render_widget(Block::default().style(Style::new().bg(CANVAS)), area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" VOICE  /  {}", connection_label(&state.connection)),
                Style::new().fg(ACCENT).bold(),
            ),
            Span::styled(
                format!("  ·  light {}", if state.bulb_on { "ON" } else { "OFF" }),
                Style::new().fg(if state.bulb_on { ACCENT } else { MUTED }),
            ),
        ])),
        Rect::new(area.x, area.y, area.width, 1),
    );
    for (index, (_, label, value)) in config_rows(settings).into_iter().enumerate() {
        let Some(rect) = geometry.config_rect(index, scroll) else {
            continue;
        };
        let style = target_style(index, ui, true);
        frame.render_widget(Block::default().style(style), rect);
        frame.render_widget(
            Paragraph::new(format!(" {label}")).style(style),
            Rect::new(rect.x, rect.y, rect.width, 1),
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("   "),
                Span::raw(value),
                Span::raw("  ›"),
            ]))
            .style(if index == ui.focus || ui.hovered == Some(index) {
                style
            } else {
                style.fg(MUTED)
            }),
            Rect::new(rect.x, rect.y + 1, rect.width, 1),
        );
    }
    if let Some((scene, clipped)) = geometry.scene_rect(scroll, ui.scene_scroll) {
        render_scene(frame, scene, state, clipped);
    }
    for (index, rect) in geometry.controls.iter().enumerate() {
        let focus = CONFIG_ROWS.len() + index;
        let enabled = action(focus, state, settings).is_some();
        let label = match index {
            0 if state.is_running => "↻ Retry test",
            0 => "▷ Test voice",
            1 => "□ Stop",
            _ if settings.input_mode != VoiceInputMode::PushToTalk => "Auto listening",
            _ if matches!(state.connection.as_ref(), VoiceConnectionState::Recording) => {
                "↑ Send turn"
            }
            _ => "● Record",
        };
        frame.render_widget(
            Paragraph::new(label)
                .centered()
                .style(target_style(focus, ui, enabled)),
            *rect,
        );
    }
    if area.height >= 3 {
        let hint = if geometry.scene.is_some() && geometry.max_scene_scroll() > 0 {
            format!(
                " PgUp/PgDn transcripts {}/{} · Tab settings",
                ui.scene_scroll.min(geometry.max_scene_scroll()),
                geometry.max_scene_scroll()
            )
        } else if geometry.max_scroll() > 0 {
            format!(
                " PgUp/PgDn scroll {}/{} · Tab focus",
                scroll,
                geometry.max_scroll()
            )
        } else {
            " Say: turn the light on, then turn it off".into()
        };
        frame.render_widget(
            Paragraph::new(hint).style(Style::new().fg(MUTED)),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }
}

fn target_style(index: usize, ui: &VoiceUiState, enabled: bool) -> Style {
    if !enabled {
        return Style::new().fg(MUTED).bg(PANEL);
    }
    if index == ui.focus || ui.hovered == Some(index) {
        Style::new().fg(CANVAS).bg(ACCENT).bold()
    } else {
        Style::new().fg(INK).bg(PANEL)
    }
}

fn adjusted_scroll(current: u16, amount: i32, maximum: u16) -> u16 {
    i32::from(current)
        .saturating_add(amount)
        .clamp(0, i32::from(maximum)) as u16
}

fn render_scene(frame: &mut Frame, rect: Rect, state: &VoiceDemoState, clipped: u16) {
    if !rect.is_empty() {
        frame.render_widget(scene_paragraph(state).scroll((clipped, 0)), rect);
    }
}

/// Measurement and paint use the same paragraph, including Unicode cell
/// widths, newlines, whitespace-preserving wrapping and provider errors.
fn scene_paragraph(state: &VoiceDemoState) -> Paragraph<'_> {
    let bulb = if state.bulb_on {
        "       \\  |  /\n     -- .---. --\n       /     \\\n      |  /\\/ |\n       \\  |  /\n        '==='\n         |_|"
    } else {
        "\n        .---.\n       /     \\\n      |  .-.  |\n       \\     /\n        '==='\n         |_|"
    };
    let color = if state.bulb_on { ACCENT } else { MUTED };
    let mut lines = vec![Line::styled(
        "LIGHTBULB LAB",
        Style::new().fg(ACCENT).bold(),
    )];
    lines.extend(
        bulb.lines()
            .map(|line| Line::styled(line, Style::new().fg(color))),
    );
    lines.push(Line::styled(
        format!(
            "Light {} · {} tool receipts",
            if state.bulb_on { "ON" } else { "OFF" },
            state.acknowledged_calls
        ),
        Style::new().fg(if state.bulb_on { SUCCESS } else { MUTED }),
    ));
    if let VoiceConnectionState::Failed(error) = state.connection.as_ref() {
        lines.extend(
            error
                .split('\n')
                .map(|line| Line::styled(line, Style::new().fg(ACCENT))),
        );
    } else {
        lines.push(Line::styled(
            state
                .last_tool_status
                .as_deref()
                .unwrap_or("Test opens your microphone and Realtime"),
            Style::new().fg(MUTED),
        ));
    }
    lines.push(Line::from("Only this bulb can be controlled."));
    append_transcript(&mut lines, "You: ", ACCENT, &state.user_transcript);
    append_transcript(&mut lines, "Voice: ", SUCCESS, &state.assistant_transcript);
    Paragraph::new(lines)
        .style(Style::new().fg(INK).bg(PANEL))
        .wrap(Wrap { trim: false })
}

fn append_transcript<'a>(
    lines: &mut Vec<Line<'a>>,
    label: &'static str,
    color: Color,
    transcript: &'a str,
) {
    let transcript = if transcript.is_empty() {
        "—"
    } else {
        transcript
    };
    for (index, line) in transcript.split('\n').enumerate() {
        let prefix = if index == 0 { label } else { "" };
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::new().fg(color)),
            Span::raw(line),
        ]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn every_configuration_and_live_action_is_reachable_on_small_and_wide_screens() {
        let settings = VoiceSettings {
            input_mode: VoiceInputMode::PushToTalk,
            ..VoiceSettings::default()
        };
        let state = VoiceDemoState {
            is_running: true,
            connection: VoiceConnectionState::Listening.into(),
            ..VoiceDemoState::default()
        };
        for area in [Rect::new(0, 0, 38, 9), Rect::new(0, 0, 118, 33)] {
            let mut ui = VoiceUiState::default();
            for index in 0..TARGET_COUNT {
                ui.focus = index;
                ui.reveal_focus(area, &state);
                let geometry = Geometry::new(area, &state);
                let rect = if index < CONFIG_ROWS.len() {
                    geometry.config_rect(index, ui.scroll).unwrap()
                } else {
                    geometry.controls[index - CONFIG_ROWS.len()]
                };
                let position = Position::new(rect.x, rect.y);
                assert_eq!(focus_at(area, &ui, &state, position), Some(index));
                assert!(hit(area, &ui, &state, &settings, position).is_some());
            }
        }
    }

    #[test]
    fn render_never_exposes_configured_key_and_keeps_test_visible() {
        let settings = VoiceSettings {
            api_key: "never-print-this-fixture-key".into(),
            ..VoiceSettings::default()
        };
        for (width, height) in [(40, 16), (80, 24), (120, 40), (200, 60)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let ui = VoiceUiState::default();
            terminal
                .draw(|frame| {
                    render(
                        frame,
                        frame.area(),
                        &settings,
                        &VoiceDemoState::default(),
                        &ui,
                    )
                })
                .unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(!text.contains(&settings.api_key));
            assert!(text.contains("Test voice"));
        }
    }

    #[test]
    fn recording_controls_follow_real_lifecycle_and_input_mode() {
        let mut state = VoiceDemoState::default();
        let settings = VoiceSettings {
            input_mode: VoiceInputMode::PushToTalk,
            ..VoiceSettings::default()
        };
        assert_eq!(action(CONFIG_ROWS.len() + 2, &state, &settings), None);
        state.is_running = true;
        state.connection = VoiceConnectionState::Listening.into();
        assert_eq!(
            action(CONFIG_ROWS.len() + 2, &state, &settings),
            Some(VoiceAction::StartPushToTalk)
        );
        state.connection = VoiceConnectionState::Recording.into();
        assert_eq!(
            action(CONFIG_ROWS.len() + 2, &state, &settings),
            Some(VoiceAction::StopPushToTalk)
        );
    }

    #[test]
    fn narrow_scrolling_reveals_real_transcripts_and_keyboard_focus_wraps() {
        let area = Rect::new(0, 0, 38, 9);
        let settings = VoiceSettings::default();
        let state = VoiceDemoState {
            user_transcript: "turn on".into(),
            assistant_transcript: "Light on".into(),
            ..VoiceDemoState::default()
        };
        let mut ui = VoiceUiState::default();
        ui.move_focus(area, &state, -1);
        assert_eq!(ui.focus, TARGET_COUNT - 1);
        ui.move_focus(area, &state, 1);
        assert_eq!(
            ui.focused_action(&state, &settings),
            Some(VoiceAction::Configure(VoiceRow::Enabled))
        );
        ui.scroll_by(area, &state, i32::MAX);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| render(frame, area, &settings, &state, &ui))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("turn on"));
        assert!(text.contains("Light on"));
        ui.scroll_by(area, &state, i32::MIN);
        assert_eq!(ui.scroll, 0);
    }

    #[test]
    fn long_user_assistant_and_errors_are_fully_reachable_in_stacked_and_wide_layouts() {
        let settings = VoiceSettings::default();
        let state = VoiceDemoState {
            user_transcript: format!("{}\nUSER_TAIL", "user 灯 words ".repeat(150)).into(),
            assistant_transcript: format!("{}\nASSISTANT_TAIL", "assistant é words ".repeat(120))
                .into(),
            connection: VoiceConnectionState::Failed(format!(
                "{}\nERROR_TAIL",
                "provider detail ".repeat(130)
            ))
            .into(),
            ..VoiceDemoState::default()
        };
        for area in [
            Rect::new(0, 0, 38, 9),
            Rect::new(0, 0, 40, 16),
            Rect::new(0, 0, 120, 24),
        ] {
            let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
            let mut ui = VoiceUiState::default();
            let geometry = Geometry::new(area, &state);
            let maximum = if geometry.scene.is_some() {
                geometry.max_scene_scroll()
            } else {
                geometry.max_scroll()
            };
            assert!(maximum > 15);
            let mut visible = String::new();
            for _ in 0..=maximum {
                terminal
                    .draw(|frame| render(frame, area, &settings, &state, &ui))
                    .unwrap();
                visible.push_str(
                    &terminal
                        .backend()
                        .buffer()
                        .content
                        .iter()
                        .map(|cell| cell.symbol())
                        .collect::<String>(),
                );
                ui.scroll_by(area, &state, 1);
            }
            for marker in ["USER_TAIL", "ASSISTANT_TAIL", "ERROR_TAIL"] {
                assert!(visible.contains(marker), "{marker} unreachable in {area:?}");
            }
            if geometry.scene.is_some() {
                assert_eq!(ui.scroll, 0, "Reading transcript must not move settings");
                assert_eq!(ui.scene_scroll, maximum);
            } else {
                assert_eq!(ui.scroll, maximum);
            }
        }
    }

    #[test]
    fn wide_mouse_scroll_and_focus_reveal_preserve_independent_reading_offsets() {
        let area = Rect::new(0, 0, 120, 16);
        let state = VoiceDemoState {
            assistant_transcript: "long response ".repeat(160).into(),
            ..VoiceDemoState::default()
        };
        let geometry = Geometry::new(area, &state);
        let scene = geometry.scene.unwrap();
        let mut ui = VoiceUiState::default();
        ui.scroll_at(area, &state, i32::MAX, Position::new(scene.x, scene.y));
        let reading_offset = ui.scene_scroll;
        assert_eq!(reading_offset, geometry.max_scene_scroll());
        assert_eq!(ui.scroll, 0);
        ui.scroll_at(
            area,
            &state,
            i32::MAX,
            Position::new(area.x, geometry.body.y),
        );
        assert_eq!(ui.scroll, geometry.max_scroll());
        assert_eq!(ui.scene_scroll, reading_offset);
        ui.focus = 0;
        ui.reveal_focus(area, &state);
        assert_eq!(ui.scroll, 0);
        assert_eq!(ui.scene_scroll, reading_offset);
        assert_eq!(
            focus_at(
                area,
                &ui,
                &state,
                Position::new(geometry.body.x, geometry.body.y)
            ),
            Some(0)
        );
    }
}
