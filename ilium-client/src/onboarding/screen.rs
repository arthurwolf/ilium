//! Shared responsive geometry for drawing and hit testing the wizard.

use ratatui::{
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use super::state::Step;

const CANVAS: Color = Color::Rgb(13, 16, 23);
const PANEL: Color = Color::Rgb(24, 29, 40);
const INK: Color = Color::Rgb(224, 231, 244);
const MUTED: Color = Color::Rgb(144, 157, 179);
const ACCENT: Color = Color::Rgb(242, 188, 105);

#[derive(Debug, Default)]
pub struct WizardUi {
    pub(crate) identity: std::sync::Arc<()>,
    pub focus: usize,
    pub footer_focus: Option<usize>,
    pub hovered: Option<usize>,
    pub scroll: u16,
    pub studio: Option<super::studio::SoundStudio>,
    pub studio_ui: super::studio_ui::StudioUiState,
    pub practice: super::practice::PracticeState,
    pub keyboard_ui: super::keyboard_ui::KeyboardUiState,
    pub keyboard_editing: bool,
    pub practice_started: Option<std::time::Instant>,
    pub voice_ui: super::voice_ui::VoiceUiState,
    pub voice_state: super::voice_runtime::VoiceDemoState,
    pub voice_action: Option<super::voice_ui::VoiceAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Choice(usize),
    DefaultPreview,
    Back,
    Continue,
    Skip,
}

#[derive(Debug)]
pub struct Geometry {
    pub header: Rect,
    pub steps: Rect,
    pub content: Rect,
    pub back: Rect,
    pub next: Rect,
    pub skip: Rect,
}

impl Geometry {
    pub fn new(area: Rect) -> Self {
        let inset = u16::from(area.width >= 30);
        let width = area.width.saturating_sub(2 * inset);
        let x = area.x + inset;
        let footer_y = area.bottom().saturating_sub(2).max(area.y);
        let content_y = (area.y + 5).min(footer_y);
        let button_width = (width / 3).min(22);
        Self {
            header: Rect::new(x, area.y, width, area.height.min(2)),
            steps: Rect::new(
                x,
                (area.y + 2).min(area.bottom()),
                width,
                area.height.saturating_sub(2).min(2),
            ),
            content: Rect::new(x, content_y, width, footer_y.saturating_sub(content_y)),
            back: Rect::new(
                x,
                footer_y,
                button_width,
                area.bottom().saturating_sub(footer_y),
            ),
            next: Rect::new(
                x + width.saturating_sub(button_width),
                footer_y,
                button_width,
                area.bottom().saturating_sub(footer_y),
            ),
            skip: Rect::new(
                x + button_width,
                footer_y,
                width.saturating_sub(2 * button_width),
                area.bottom().saturating_sub(footer_y),
            ),
        }
    }

    pub fn choice_skip(&self) -> Rect {
        Rect::new(
            self.content.x,
            self.content.bottom().saturating_sub(1),
            self.content.width,
            self.content.height.min(1),
        )
    }

    pub fn card_offset(&self, step: Step, index: usize) -> u16 {
        (0..index.min(3))
            .map(|candidate| card_height(step, candidate, self.content.width).saturating_add(1))
            .fold(0, u16::saturating_add)
    }

    pub fn max_scroll(&self, step: Step) -> u16 {
        self.card_offset(step, 3)
            .saturating_sub(self.content.height.saturating_sub(2))
    }

    pub fn cards(&self, scroll: u16) -> Vec<(usize, Rect, u16)> {
        self.cards_for_step(Step::AiChoice, scroll)
    }

    pub fn cards_for_step(&self, step: Step, scroll: u16) -> Vec<(usize, Rect, u16)> {
        let view = Rect {
            height: self.content.height.saturating_sub(2),
            ..self.content
        };
        if view.width >= 88 {
            let width = view.width.saturating_sub(4) / 3;
            return (0..3)
                .map(|index| {
                    (
                        index,
                        Rect::new(
                            view.x + index as u16 * (width + 2),
                            view.y,
                            width,
                            view.height,
                        ),
                        0,
                    )
                })
                .collect();
        }
        let mut cards = Vec::new();
        for index in 0..3 {
            let start = self.card_offset(step, index);
            let end = start.saturating_add(card_height(step, index, view.width));
            let view_end = scroll.saturating_add(view.height);
            if start >= view_end || end <= scroll {
                continue;
            }
            let clipped = start.max(scroll);
            cards.push((
                index,
                Rect::new(
                    view.x,
                    view.y + clipped.saturating_sub(scroll),
                    view.width,
                    end.min(view_end).saturating_sub(clipped),
                ),
                clipped.saturating_sub(start),
            ));
        }
        cards
    }

    pub fn preview_button(&self, scroll: u16) -> Rect {
        self.cards_for_step(Step::SoundChoice, scroll)
            .into_iter()
            .find(|(index, _, _)| *index == 0)
            .filter(|(_, area, _)| area.height >= 4)
            .map(|(_, area, _)| {
                Rect::new(
                    area.x + 1,
                    area.bottom().saturating_sub(2),
                    area.width.saturating_sub(2).min(20),
                    1,
                )
            })
            .unwrap_or_default()
    }

    pub fn hit(&self, position: Position, scroll: u16) -> Option<Hit> {
        self.hit_for_step(Step::AiChoice, position, scroll)
    }

    pub fn hit_for_step(&self, step: Step, position: Position, scroll: u16) -> Option<Hit> {
        if self.back.contains(position) {
            return Some(Hit::Back);
        }
        if self.next.contains(position) {
            return Some(Hit::Continue);
        }
        if self.skip.contains(position)
            || (matches!(
                step,
                Step::AiChoice | Step::SoundChoice | Step::KeyboardChoice
            ) && self.choice_skip().contains(position))
        {
            return Some(Hit::Skip);
        }
        if step == Step::SoundChoice && self.preview_button(scroll).contains(position) {
            return Some(Hit::DefaultPreview);
        }
        self.cards_for_step(step, scroll)
            .into_iter()
            .find(|(_, area, _)| area.contains(position))
            .map(|(index, _, _)| Hit::Choice(index))
    }
}

pub fn render_shell(frame: &mut Frame, area: Rect, step: Step) -> Geometry {
    let geometry = Geometry::new(area);
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(Style::new().bg(CANVAS)), area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " ILIUM ",
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("/ Make it yours", Style::new().fg(INK)),
        ])),
        geometry.header,
    );
    let full_width = Step::ALL
        .iter()
        .map(|candidate| candidate.title().chars().count() + 5)
        .sum::<usize>();
    let short_width = Step::ALL
        .iter()
        .map(|candidate| short_title(*candidate).chars().count() + 5)
        .sum::<usize>();
    let available = usize::from(geometry.steps.width);
    let wide = available >= full_width;
    let compact = available < short_width;
    let dense = available < Step::ALL.len() * 4;
    let mut spans = Vec::new();
    for candidate in Step::ALL {
        let selected = candidate == step;
        let background = if selected {
            ACCENT
        } else if candidate.number() < step.number() {
            Color::Rgb(31, 48, 46)
        } else {
            PANEL
        };
        let foreground = if selected {
            CANVAS
        } else if candidate.number() < step.number() {
            Color::Rgb(151, 196, 173)
        } else {
            MUTED
        };
        let label = if compact {
            ""
        } else if wide {
            candidate.title()
        } else {
            short_title(candidate)
        };
        spans.push(Span::styled(
            if dense {
                candidate.number().to_string()
            } else if compact {
                format!(" {} ", candidate.number())
            } else {
                format!(" {} {label} ", candidate.number())
            },
            Style::new().fg(foreground).bg(background),
        ));
        spans.push(Span::styled("▶", Style::new().fg(background).bg(CANVAS)));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), geometry.steps);
    for (rectangle, label) in [
        (geometry.back, "‹ Back\nAlt+←"),
        (geometry.skip, "Skip\nCtrl+S"),
        (
            geometry.next,
            if step == Step::Voice {
                "Finish ›\nCtrl+Enter"
            } else {
                "Continue ›\nCtrl+Enter"
            },
        ),
    ] {
        frame.render_widget(
            Paragraph::new(label)
                .centered()
                .style(Style::new().fg(INK).bg(PANEL)),
            rectangle,
        );
    }
    geometry
}

pub fn render_choices(frame: &mut Frame, geometry: &Geometry, step: Step, ui: &WizardUi) {
    let choices = choice_text(step);
    for (index, rectangle, clipped) in geometry.cards_for_step(step, ui.scroll) {
        let active = ui.hovered == Some(index) || (ui.hovered.is_none() && ui.focus == index);
        let (foreground, background) = if active {
            (CANVAS, ACCENT)
        } else {
            (INK, PANEL)
        };
        let (title, subtitle, body) = choices[index];
        let mut lines = vec![
            Line::from(Span::styled(
                title,
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Line::from(subtitle),
            Line::default(),
        ];
        lines.extend(body.lines().map(|line| Line::from(line.to_owned())));
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((clipped, 0))
                .style(Style::new().fg(foreground).bg(background))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::new().fg(if active { ACCENT } else { MUTED })),
                ),
            rectangle,
        );
    }
    let label = match step {
        Step::AiChoice => "Skip AI assistance · keep naming and organization manual",
        Step::SoundChoice => "Keep notifications silent",
        _ => "Keep my current controls",
    };
    frame.render_widget(
        Paragraph::new(label)
            .centered()
            .style(Style::new().fg(INK).bg(PANEL)),
        geometry.choice_skip(),
    );
    if step == Step::SoundChoice {
        frame.render_widget(
            Paragraph::new("▶ Play signature").style(Style::new().fg(CANVAS).bg(ACCENT)),
            geometry.preview_button(ui.scroll),
        );
    }
}

fn choice_text(step: Step) -> [(&'static str, &'static str, &'static str); 3] {
    match step {
        Step::AiChoice => [
            ("Kilo Gateway", "Cloud · no-cost models", "Free AI naming and organization. It sometimes just fails.\n\nData is sent to a hosted provider and may be used for training.\n\nChoose this if convenience matters most."),
            ("Paid APIs", "Cloud · your account", "Choose your provider and model.\n\nFast, capable models; usage costs money. Your provider's data policy applies.\n\nBring an API key."),
            ("Local Ollama", "Your machine · your data", "Run naming and organization locally.\n\nNo API usage charge; speed and model quality depend on your hardware.\n\nRequires a running Ollama instance."),
        ],
        Step::SoundChoice => [
            ("Ilium signature", "A little chirp", "The bundled notification sound.\n\nA short, gentle chirp, available even without system sound files.\n\nPreview it before choosing."),
            ("System sounds", "Familiar by design", "Choose a sound installed on this computer.\n\nBrowse your Ubuntu, Windows or macOS sound catalog.\n\nKeep the desktop's familiar character."),
            ("Sound studio", "Make something yours", "Shape your own notification.\n\nPlay with tone, rhythm, harmony, texture and envelopes.\n\nDesign it, hear it, keep it."),
        ],
        Step::KeyboardChoice => [
            ("tmux", "Ctrl+B · familiar muscle memory", "Start with Ilium's tmux-style preset.\n\nCommon pane actions follow familiar bindings. Ilium's tree adds a few new moves.\n\nTry them next."),
            ("GNU Screen", "Ctrl+A · familiar muscle memory", "Start with Ilium's Screen-style preset.\n\nUse the prefix you already know. Ctrl+A shadows the shell's beginning-of-line shortcut.\n\nPractice without risk."),
            ("Custom", "Your hands, your rules", "Choose your own prefixes and bindings.\n\nKeep your current mapping, or build a new one with conflict checks.\n\nTest everything in the playground."),
        ],
        _ => [("", "", ""); 3],
    }
}

fn card_height(step: Step, index: usize, width: u16) -> u16 {
    let (title, subtitle, body) = choice_text(step)[index];
    let paragraph =
        Paragraph::new(format!("{title}\n{subtitle}\n\n{body}")).wrap(Wrap { trim: false });
    paragraph
        .line_count(width.saturating_sub(2))
        .min(usize::from(u16::MAX - 2)) as u16
        + 2
}

fn short_title(step: Step) -> &'static str {
    match step {
        Step::AiChoice => "AI",
        Step::AiConfiguration => "Connect",
        Step::SoundChoice => "Sound",
        Step::SoundConfiguration => "Studio",
        Step::KeyboardChoice => "Keys",
        Step::KeyboardPractice => "Play",
        Step::Voice => "Voice",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn all_seven_chevrons_fit_between_label_breakpoints() {
        for width in 40..=160 {
            let mut terminal = Terminal::new(TestBackend::new(width, 16)).unwrap();
            terminal
                .draw(|frame| {
                    render_shell(frame, frame.area(), Step::Voice);
                })
                .unwrap();
            let geometry = Geometry::new(Rect::new(0, 0, width, 16));
            let buffer = terminal.backend().buffer();
            let row = (geometry.steps.x..geometry.steps.right())
                .map(|x| buffer[(x, geometry.steps.y)].symbol())
                .collect::<String>();
            assert_eq!(row.matches('▶').count(), 7, "width {width}: {row}");
            assert!(row.contains('7'), "last step missing at width {width}");
        }
    }

    #[test]
    fn all_choice_cards_are_reachable_at_narrow_sizes() {
        for (width, height) in [(40, 16), (80, 24), (120, 40), (200, 60)] {
            let area = Rect::new(0, 0, width, height);
            let geometry = Geometry::new(area);
            for index in 0..3 {
                let scroll = if width < 90 {
                    geometry.card_offset(Step::AiChoice, index)
                } else {
                    0
                };
                let (_, card, _) = geometry
                    .cards(scroll)
                    .into_iter()
                    .find(|(candidate, _, _)| *candidate == index)
                    .expect("focused card visible");
                assert!(area.contains(Position::new(card.x, card.y)));
                assert_eq!(
                    geometry.hit(Position::new(card.x, card.y), scroll),
                    Some(Hit::Choice(index))
                );
            }
            assert!(geometry.next.height > 0);
            assert!(geometry.content.height > 0);
        }
    }

    #[test]
    fn hover_inverts_the_same_rectangle_that_mouse_hits() {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| {
                let geometry = render_shell(frame, frame.area(), Step::AiChoice);
                render_choices(
                    frame,
                    &geometry,
                    Step::AiChoice,
                    &WizardUi {
                        hovered: Some(1),
                        ..WizardUi::default()
                    },
                );
            })
            .unwrap();
        let geometry = Geometry::new(Rect::new(0, 0, 120, 30));
        let cards = geometry.cards(0);
        let selected = cards[1].1;
        let idle = cards[0].1;
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(selected.x + 1, selected.y + 1)].bg, ACCENT);
        assert_eq!(buffer[(idle.x + 1, idle.y + 1)].bg, PANEL);
    }
}
