//! Large, read-only two-state help dialog for one Settings control.

use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::config::MotionLevel;
use crate::theme;

use super::HelpTopic;

const DIALOG_WIDTH_PERCENT: u16 = 96;
const DIALOG_HEIGHT_PERCENT: u16 = 96;
const TWO_COLUMN_MIN_WIDTH: u16 = 92;
const PANEL_GAP: u16 = 1;
const FOOTER_HEIGHT: u16 = 2;
const FULL_FRAME_INTERVAL: Duration = Duration::from_millis(700);
const REDUCED_FRAME_INTERVAL: Duration = Duration::from_millis(1400);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogLayout {
    pub popup: Rect,
    pub explanation: Rect,
    pub illustration: Rect,
    pub footer: Rect,
    pub stacked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpPanel {
    Explanation,
    Illustration,
}

/// Per-opening presentation state. It never contains or mutates a setting.
#[derive(Debug, Clone)]
pub struct SettingsHelpState {
    pub topic_id: String,
    selected_frame: usize,
    playback_started_at: Instant,
    pub is_playing: bool,
    pub explanation_scroll: u16,
    pub illustration_scroll: u16,
    pub focused_panel: HelpPanel,
}

impl SettingsHelpState {
    pub fn new(topic_id: impl Into<String>, frame_count: usize, motion: MotionLevel) -> Self {
        Self {
            topic_id: topic_id.into(),
            selected_frame: 0,
            playback_started_at: Instant::now(),
            is_playing: frame_count > 1 && motion != MotionLevel::Off,
            explanation_scroll: 0,
            illustration_scroll: 0,
            focused_panel: HelpPanel::Explanation,
        }
    }

    pub fn frame_index(&self, frame_count: usize, motion: MotionLevel, now: Instant) -> usize {
        if frame_count <= 1 || !self.is_playing || motion == MotionLevel::Off {
            return self.selected_frame.min(frame_count.saturating_sub(1));
        }
        let interval = match motion {
            MotionLevel::Full => FULL_FRAME_INTERVAL,
            MotionLevel::Reduced => REDUCED_FRAME_INTERVAL,
            MotionLevel::Off => return self.selected_frame.min(frame_count - 1),
        };
        let elapsed_frames = now
            .saturating_duration_since(self.playback_started_at)
            .as_millis()
            / interval.as_millis();
        (self.selected_frame + elapsed_frames as usize) % frame_count
    }

    pub fn step_frame(&mut self, frame_count: usize, direction: i32, motion: MotionLevel) {
        if frame_count <= 1 {
            return;
        }
        let now = Instant::now();
        let current = self.frame_index(frame_count, motion, now);
        let next = if direction < 0 {
            current.checked_sub(1).unwrap_or(frame_count - 1)
        } else {
            (current + 1) % frame_count
        };
        self.selected_frame = next;
        self.playback_started_at = now;
    }

    pub fn toggle_playback(&mut self, frame_count: usize, motion: MotionLevel) {
        if frame_count <= 1 || motion == MotionLevel::Off {
            self.is_playing = false;
            return;
        }
        if self.is_playing {
            self.selected_frame = self.frame_index(frame_count, motion, Instant::now());
            self.is_playing = false;
        } else {
            self.is_playing = true;
            self.playback_started_at = Instant::now();
        }
    }

    pub fn scroll_focused_panel(&mut self, direction: i32) {
        let scroll = match self.focused_panel {
            HelpPanel::Explanation => &mut self.explanation_scroll,
            HelpPanel::Illustration => &mut self.illustration_scroll,
        };
        if direction < 0 {
            *scroll = scroll.saturating_sub(direction.unsigned_abs().min(u16::MAX as u32) as u16);
        } else {
            *scroll = scroll.saturating_add(direction.min(i32::from(u16::MAX)) as u16);
        }
    }

    pub fn focus_panel_at(&mut self, layout: DialogLayout, x: u16, y: u16) {
        if layout
            .explanation
            .contains(ratatui::layout::Position::new(x, y))
        {
            self.focused_panel = HelpPanel::Explanation;
        } else if layout
            .illustration
            .contains(ratatui::layout::Position::new(x, y))
        {
            self.focused_panel = HelpPanel::Illustration;
        }
    }
}

pub fn layout(area: Rect) -> DialogLayout {
    let popup = crate::layout::centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, area);
    let inner = popup.inner(ratatui::layout::Margin::new(1, 1));
    let footer_height = FOOTER_HEIGHT.min(inner.height);
    let content_height = inner.height.saturating_sub(footer_height);
    let content = Rect::new(inner.x, inner.y, inner.width, content_height);
    let footer = Rect::new(
        inner.x,
        inner.y.saturating_add(content_height),
        inner.width,
        footer_height,
    );
    let stacked = inner.width < TWO_COLUMN_MIN_WIDTH;

    if stacked {
        let explanation_height = content_height.div_ceil(2);
        DialogLayout {
            popup,
            explanation: Rect::new(content.x, content.y, content.width, explanation_height),
            illustration: Rect::new(
                content.x,
                content.y.saturating_add(explanation_height),
                content.width,
                content_height.saturating_sub(explanation_height),
            ),
            footer,
            stacked,
        }
    } else {
        let column_width = content.width.saturating_sub(PANEL_GAP) / 2;
        DialogLayout {
            popup,
            explanation: Rect::new(content.x, content.y, column_width, content.height),
            illustration: Rect::new(
                content
                    .x
                    .saturating_add(column_width)
                    .saturating_add(PANEL_GAP),
                content.y,
                content
                    .width
                    .saturating_sub(column_width)
                    .saturating_sub(PANEL_GAP),
                content.height,
            ),
            footer,
            stacked,
        }
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    topic: &HelpTopic,
    state: &SettingsHelpState,
    motion: MotionLevel,
    now: Instant,
) {
    let layout = layout(area);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title(&topic.title)),
        layout.popup,
    );

    let explanation = format!(
        "{}\n\nStates and change\n{}\n\nMotion\n{}\n\nAccuracy note\n{}",
        topic.explanation, topic.states, topic.motion, topic.caveat
    );
    let explanation_block = panel_block(
        "What this setting does",
        state.focused_panel == HelpPanel::Explanation,
    );
    frame.render_widget(
        Paragraph::new(explanation)
            .wrap(Wrap { trim: false })
            .scroll((state.explanation_scroll, 0))
            .block(explanation_block),
        layout.explanation,
    );

    let frame_count = topic.frames.len();
    let frame_index = state.frame_index(frame_count, motion, now);
    let illustration = topic
        .frames
        .get(frame_index)
        .map(String::as_str)
        .unwrap_or(topic.specimen.as_str());
    let frame_title = if frame_count > 1 {
        format!("Illustration · frame {}/{}", frame_index + 1, frame_count)
    } else {
        "Illustration".to_string()
    };
    let illustration_block =
        panel_block(&frame_title, state.focused_panel == HelpPanel::Illustration);
    frame.render_widget(
        Paragraph::new(illustration)
            .wrap(Wrap { trim: false })
            .scroll((state.illustration_scroll, 0))
            .block(illustration_block),
        layout.illustration,
    );

    let playback = if frame_count <= 1 || motion == MotionLevel::Off {
        "still"
    } else if state.is_playing {
        "playing"
    } else {
        "paused"
    };
    let footer = Line::from(vec![
        Span::styled("←/→", Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(" frame  "),
        Span::styled("Space", Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(format!(" {playback}  ")),
        Span::styled("Tab", Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(" panel  "),
        Span::styled("PgUp/PgDn", Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(" scroll  "),
        Span::styled("Esc", Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(" close"),
    ]);
    frame.render_widget(
        Paragraph::new(footer).wrap(Wrap { trim: true }),
        layout.footer,
    );
}

pub fn render_missing(frame: &mut Frame, area: Rect, topic_id: &str) {
    let popup = crate::layout::centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(format!(
            "The bundled Settings help topic {topic_id} could not be found.\n\nPress Esc to return to Settings."
        ))
        .wrap(Wrap { trim: false })
        .block(theme::block(true).title(theme::chrome_title("Settings help unavailable"))),
        popup,
    );
}

fn panel_block(title: &str, focused: bool) -> Block<'static> {
    let title = if focused {
        Span::styled(title.to_string(), Style::new().add_modifier(Modifier::BOLD))
    } else {
        Span::raw(title.to_string())
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(if focused {
            theme::selected_style()
        } else {
            Style::default()
        })
        .title(title)
}

#[cfg(test)]
mod tests;
