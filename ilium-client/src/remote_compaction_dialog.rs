//! Dialog model and renderer for remote compaction: step list, overall
//! progress, token-usage bars for every part of the run, a log, and the
//! closable privacy banner. Pure presentation; the flow that drives it is in
//! `remote_compaction_flow`.

use std::cell::Cell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ilium_core::{AgentProvider, BuiltinAgentProvider, NodeId};
use ilium_remote_compaction::{AgentKind, CompactionEvent, Technique, TokenBreakdown};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Gauge, Paragraph};
use ratatui::Frame;

use crate::modal;
use crate::remote_compaction_settings_ui::{privacy_banner_height, render_privacy_banner};
use crate::session_stats_ui::compact_count;
use crate::theme;

/// Upper bound on retained log lines.
const MAX_LOG_LINES: usize = 500;

const DIALOG_WIDTH: u16 = 86;
const DIALOG_BASE_HEIGHT: u16 = 30;

/// Fixed stages shown in the step list, in order. The compaction crate's own
/// four steps (read, prepare, summarize, write) sit between stop and restart.
const STAGE_WAIT: usize = 0;
const STAGE_STOP: usize = 1;
const STAGE_FIRST_CRATE_STEP: usize = 2;
const CRATE_STEP_COUNT: usize = 4;
const STAGE_RESTART: usize = STAGE_FIRST_CRATE_STEP + CRATE_STEP_COUNT;
const STAGE_TITLES: [&str; STAGE_RESTART + 1] = [
    "Wait for a safe pause",
    "Stop the agent",
    "Read the transcript",
    "Prepare the summarizer input",
    "Summarize with the remote model",
    "Write the compacted transcript",
    "Restart the agent",
];

/// Where the run currently is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCompactionPhase {
    /// Polling the transcript until the agent is between turns.
    WaitingForPause,
    /// The server was asked to terminate the agent process.
    StoppingAgent,
    Compacting,
    Restarting,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct RemoteCompactionDialogState {
    pub pane_id: NodeId,
    pub provider: BuiltinAgentProvider,
    pub agent: AgentKind,
    pub session_id: String,
    pub project_cwd: PathBuf,
    pub transcript_path: PathBuf,
    pub technique: Technique,
    /// `provider / model` that receives the transcript.
    pub destination: String,
    pub is_automatic: bool,
    pub phase: RemoteCompactionPhase,
    /// True once the server confirmed the agent process is gone.
    pub is_agent_stopped: bool,
    pub progress: f32,
    /// 1-based crate step currently running (0 before the first).
    pub current_step: usize,
    pub tokens: TokenBreakdown,
    pub log: Vec<String>,
    pub started_at: Instant,
    /// Where the privacy banner's close button was drawn last frame.
    pub banner_close_rect: Cell<Rect>,
}

/// Everything a run needs to know about the pane it compacts.
#[derive(Debug, Clone)]
pub struct RemoteCompactionPlan {
    pub pane_id: NodeId,
    pub provider: BuiltinAgentProvider,
    pub agent: AgentKind,
    pub session_id: String,
    pub project_cwd: PathBuf,
    pub transcript_path: PathBuf,
    pub technique: Technique,
    /// `provider / model` that receives the transcript.
    pub destination: String,
    pub is_automatic: bool,
}

impl RemoteCompactionDialogState {
    pub fn new(plan: RemoteCompactionPlan) -> Self {
        let RemoteCompactionPlan {
            pane_id,
            provider,
            agent,
            session_id,
            project_cwd,
            transcript_path,
            technique,
            destination,
            is_automatic,
        } = plan;
        let mut state = Self {
            pane_id,
            provider,
            agent,
            session_id,
            project_cwd,
            transcript_path,
            technique,
            destination,
            is_automatic,
            phase: RemoteCompactionPhase::WaitingForPause,
            is_agent_stopped: false,
            progress: 0.0,
            current_step: 0,
            tokens: TokenBreakdown::default(),
            log: Vec::new(),
            started_at: Instant::now(),
            banner_close_rect: Cell::new(Rect::default()),
        };
        state.push_log(format!(
            "{} compaction of the {} session using the {} technique",
            if is_automatic { "Automatic" } else { "Manual" },
            provider.label(),
            technique.label()
        ));
        state.push_log("Waiting for the agent to reach a safe pause…".to_string());
        state
    }

    pub fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > MAX_LOG_LINES {
            let overflow = self.log.len() - MAX_LOG_LINES;
            self.log.drain(..overflow);
        }
    }

    pub fn apply_event(&mut self, event: CompactionEvent) {
        match event {
            CompactionEvent::Step {
                index,
                total,
                title,
            } => {
                self.current_step = index;
                self.push_log(format!("[{index}/{total}] {title}"));
            }
            CompactionEvent::Log(line) => self.push_log(line),
            CompactionEvent::Progress(value) => {
                self.progress = value.clamp(self.progress, 1.0);
            }
            CompactionEvent::Tokens(tokens) => self.tokens = tokens,
        }
    }

    pub fn fail(&mut self, message: String) {
        self.push_log(format!("Failed: {message}"));
        self.phase = RemoteCompactionPhase::Failed(message);
    }

    pub fn is_failed(&self) -> bool {
        matches!(self.phase, RemoteCompactionPhase::Failed(_))
    }

    /// The step-list row that is running now (or failed, when failed).
    pub fn active_stage(&self) -> usize {
        match &self.phase {
            RemoteCompactionPhase::WaitingForPause => STAGE_WAIT,
            RemoteCompactionPhase::StoppingAgent => STAGE_STOP,
            RemoteCompactionPhase::Compacting => (STAGE_FIRST_CRATE_STEP
                + self.current_step.saturating_sub(1))
            .clamp(STAGE_FIRST_CRATE_STEP, STAGE_RESTART - 1),
            RemoteCompactionPhase::Restarting => STAGE_RESTART,
            RemoteCompactionPhase::Failed(_) => {
                if !self.is_agent_stopped && self.current_step == 0 {
                    STAGE_WAIT
                } else if self.current_step == 0 {
                    STAGE_STOP
                } else {
                    (STAGE_FIRST_CRATE_STEP + self.current_step - 1)
                        .clamp(STAGE_FIRST_CRATE_STEP, STAGE_RESTART - 1)
                }
            }
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// The exact command that resumes the (possibly compacted) session.
    pub fn resume_command(&self) -> String {
        self.provider.resume_command(&self.session_id)
    }
}

/// Centers the dialog inside the frozen pane's viewport, falling back to the
/// whole screen when the pane is not on screen.
pub fn dialog_area(pane_area: Option<Rect>, screen: Rect, banner_rows: u16) -> Rect {
    let host = pane_area
        .filter(|area| area.width >= 40 && area.height >= 14)
        .unwrap_or(screen);
    modal::centered_fixed_rect(DIALOG_WIDTH, DIALOG_BASE_HEIGHT + banner_rows, host)
}

/// Rows the privacy banner takes inside a dialog of this width, or zero
/// when it is already dismissed (`banner_text` is `None`).
pub fn banner_rows(banner_text: Option<&str>) -> u16 {
    banner_text.map_or(0, |text| privacy_banner_height(text, DIALOG_WIDTH - 2) + 1)
}

fn phase_line(state: &RemoteCompactionDialogState) -> Line<'static> {
    let label = state.provider.label();
    match &state.phase {
        RemoteCompactionPhase::WaitingForPause => Line::from(Span::styled(
            format!("Waiting for {label} to finish its current turn…"),
            Style::new().fg(Color::Yellow),
        )),
        RemoteCompactionPhase::StoppingAgent => Line::from(Span::styled(
            format!("Stopping {label}…"),
            Style::new().fg(Color::Yellow),
        )),
        RemoteCompactionPhase::Compacting => Line::from(Span::styled(
            "Compacting the conversation…",
            Style::new().fg(Color::Cyan),
        )),
        RemoteCompactionPhase::Restarting => Line::from(Span::styled(
            format!("Restarting {label} on the compacted session…"),
            Style::new().fg(Color::Green),
        )),
        RemoteCompactionPhase::Failed(message) => Line::from(Span::styled(
            format!("Failed: {message}"),
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
    }
}

fn step_lines(state: &RemoteCompactionDialogState) -> Vec<Line<'static>> {
    let active = state.active_stage();
    STAGE_TITLES
        .iter()
        .enumerate()
        .map(|(stage, title)| {
            let marker = if stage < active {
                Span::styled("✓ ", Style::new().fg(Color::Green))
            } else if stage == active && state.is_failed() {
                Span::styled("✗ ", Style::new().fg(Color::Red))
            } else if stage == active {
                Span::styled("▶ ", Style::new().fg(Color::Cyan))
            } else {
                Span::styled("· ", Style::new().add_modifier(Modifier::DIM))
            };
            Line::from(vec![marker, Span::raw((*title).to_string())])
        })
        .collect()
}

/// One labelled bar: `label  ████░░░░  value  note`.
fn bar_line(
    label: &str,
    value: u64,
    maximum: u64,
    bar_width: usize,
    color: Color,
    note: String,
) -> Line<'static> {
    let filled = if maximum == 0 {
        0
    } else {
        ((value as f64 / maximum as f64) * bar_width as f64).round() as usize
    }
    .min(bar_width);
    // A non-zero value always shows at least one cell.
    let filled = if value > 0 { filled.max(1) } else { 0 };
    Line::from(vec![
        Span::styled(format!("{label:<20}"), Style::new().fg(Color::Gray)),
        Span::styled("█".repeat(filled), Style::new().fg(color)),
        Span::styled(
            "░".repeat(bar_width - filled),
            Style::new().add_modifier(Modifier::DIM),
        ),
        Span::raw(format!(" {:>6}", compact_count(value))),
        Span::styled(
            format!("  {note}"),
            Style::new().add_modifier(Modifier::DIM),
        ),
    ])
}

fn percent_of_window(value: u64, window: Option<u64>) -> String {
    match window {
        Some(window) if window > 0 => {
            format!(
                "{:.0}% of {}",
                value as f64 * 100.0 / window as f64,
                compact_count(window)
            )
        }
        _ => String::new(),
    }
}

/// Token bars for every part of the run. Bars share one scale: the largest
/// figure shown, so their lengths compare directly.
pub fn token_lines(tokens: &TokenBreakdown, bar_width: usize) -> Vec<Line<'static>> {
    let masked_input = tokens
        .conversation_total
        .saturating_sub(tokens.masked_savings);
    let maximum = [
        tokens.before_context,
        tokens.conversation_total,
        tokens.summarizer_input,
        tokens.summarizer_output,
        tokens.tail_kept,
        tokens.after_context,
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    let window = tokens.window;
    vec![
        bar_line(
            "Context before",
            tokens.before_context,
            maximum,
            bar_width,
            Color::Yellow,
            percent_of_window(tokens.before_context, window),
        ),
        bar_line(
            "Conversation",
            tokens.conversation_total,
            maximum,
            bar_width,
            Color::Blue,
            format!("after masking {}", compact_count(masked_input)),
        ),
        bar_line(
            "Summarizer input",
            tokens.summarizer_input,
            maximum,
            bar_width,
            Color::Magenta,
            "sent to the remote model".to_string(),
        ),
        bar_line(
            "Summary output",
            tokens.summarizer_output,
            maximum,
            bar_width,
            Color::Cyan,
            "returned by the model".to_string(),
        ),
        bar_line(
            "Recent tail kept",
            tokens.tail_kept,
            maximum,
            bar_width,
            Color::Green,
            "kept word for word".to_string(),
        ),
        bar_line(
            "Context after",
            tokens.after_context,
            maximum,
            bar_width,
            Color::Green,
            percent_of_window(tokens.after_context, window),
        ),
    ]
}

fn hint_text(state: &RemoteCompactionDialogState) -> &'static str {
    match &state.phase {
        RemoteCompactionPhase::Failed(_) if state.is_agent_stopped => {
            "Enter resume the original session   Esc close"
        }
        RemoteCompactionPhase::Failed(_) => "Esc close",
        RemoteCompactionPhase::WaitingForPause | RemoteCompactionPhase::Compacting => "Esc cancel",
        RemoteCompactionPhase::StoppingAgent | RemoteCompactionPhase::Restarting => "",
    }
}

fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Draws the dialog over the frozen pane. `banner_text` is `Some` while the
/// privacy banner has not been dismissed.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &RemoteCompactionDialogState,
    banner_text: Option<&str>,
) {
    frame.render_widget(Clear, area);
    let title = format!("Remote compaction · {}", state.provider.label());
    let border_color = if state.is_failed() {
        Color::Red
    } else {
        Color::Cyan
    };
    let block = theme::block(true)
        .title(theme::chrome_title(&title))
        .border_style(Style::new().fg(border_color).add_modifier(Modifier::BOLD));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let banner_height = banner_text.map_or(0, |text| privacy_banner_height(text, inner.width));
    let banner_gap = u16::from(banner_height > 0);
    state.banner_close_rect.set(Rect::default());
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(banner_height + banner_gap),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(STAGE_TITLES.len() as u16),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Min(2),
            Constraint::Length(1),
        ])
        .split(inner);

    if let Some(text) = banner_text {
        let close = render_privacy_banner(frame, rows[0], text);
        state.banner_close_rect.set(close);
    }

    let heading = Line::from(vec![
        Span::styled(
            format!("{} · {}", state.technique.label(), state.destination),
            Style::new().add_modifier(Modifier::DIM),
        ),
        Span::styled(
            format!("   {}", format_elapsed(state.elapsed())),
            Style::new().add_modifier(Modifier::DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(heading), rows[1]);
    frame.render_widget(Paragraph::new(phase_line(state)), rows[2]);
    frame.render_widget(Paragraph::new(step_lines(state)), rows[3]);

    let ratio = f64::from(state.progress.clamp(0.0, 1.0));
    let gauge_color = if state.is_failed() {
        Color::Red
    } else {
        Color::Cyan
    };
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::new().fg(gauge_color))
            .ratio(ratio)
            .label(format!("{:.0}%", ratio * 100.0)),
        rows[4],
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Token usage",
            Style::new().add_modifier(Modifier::BOLD),
        ))),
        rows[5],
    );
    let bar_width = usize::from(inner.width).saturating_sub(48).clamp(8, 28);
    frame.render_widget(
        Paragraph::new(token_lines(&state.tokens, bar_width)),
        rows[6],
    );

    let log_area = rows[8];
    let visible = usize::from(log_area.height);
    let start = state.log.len().saturating_sub(visible);
    let log_lines: Vec<Line> = state.log[start..]
        .iter()
        .map(|line| Line::from(Span::styled(line.clone(), Style::new().fg(Color::Gray))))
        .collect();
    frame.render_widget(Paragraph::new(log_lines), log_area);

    let hint = if banner_text.is_some() && !hint_text(state).is_empty() {
        format!("{}   x hide privacy notice", hint_text(state))
    } else {
        hint_text(state).to_string()
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::new().add_modifier(Modifier::DIM),
        ))),
        rows[9],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn state() -> RemoteCompactionDialogState {
        RemoteCompactionDialogState::new(RemoteCompactionPlan {
            pane_id: NodeId(3),
            provider: BuiltinAgentProvider::Claude,
            agent: AgentKind::Claude,
            session_id: "11111111-1111-4111-8111-111111111111".to_string(),
            project_cwd: PathBuf::from("/tmp/project"),
            transcript_path: PathBuf::from("/tmp/project/session.jsonl"),
            technique: Technique::ClaudeCode,
            destination: "Kilo Gateway / test-model".to_string(),
            is_automatic: false,
        })
    }

    fn screen_text(state: &RemoteCompactionDialogState, banner: Option<&str>) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 44)).unwrap();
        terminal
            .draw(|frame| {
                let area = dialog_area(None, frame.area(), banner_rows(banner));
                render(frame, area, state, banner);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn events_update_steps_progress_tokens_and_log() {
        let mut dialog = state();
        dialog.phase = RemoteCompactionPhase::Compacting;
        dialog.apply_event(CompactionEvent::Step {
            index: 3,
            total: 4,
            title: "Summarizing".to_string(),
        });
        dialog.apply_event(CompactionEvent::Progress(0.5));
        dialog.apply_event(CompactionEvent::Progress(0.2));
        dialog.apply_event(CompactionEvent::Tokens(TokenBreakdown {
            before_context: 130_000,
            ..TokenBreakdown::default()
        }));
        assert_eq!(dialog.current_step, 3);
        assert_eq!(dialog.active_stage(), STAGE_FIRST_CRATE_STEP + 2);
        assert!(
            (dialog.progress - 0.5).abs() < f32::EPSILON,
            "progress never goes back"
        );
        assert_eq!(dialog.tokens.before_context, 130_000);
        assert_eq!(
            dialog.log.last().map(String::as_str),
            Some("[3/4] Summarizing")
        );
    }

    #[test]
    fn failure_marks_the_running_stage() {
        let mut dialog = state();
        dialog.fail("no network".to_string());
        assert_eq!(dialog.active_stage(), STAGE_WAIT);
        dialog.is_agent_stopped = true;
        assert_eq!(dialog.active_stage(), STAGE_STOP);
        dialog.current_step = 3;
        assert_eq!(dialog.active_stage(), STAGE_FIRST_CRATE_STEP + 2);
    }

    #[test]
    fn render_shows_stages_tokens_destination_and_banner() {
        let mut dialog = state();
        dialog.tokens = TokenBreakdown {
            before_context: 130_000,
            conversation_total: 140_000,
            masked_savings: 40_000,
            summarizer_input: 100_000,
            summarizer_output: 9_000,
            tail_kept: 20_000,
            after_context: 29_000,
            window: Some(200_000),
        };
        let text = screen_text(
            &dialog,
            Some("Your session is sent to Kilo Gateway / test-model."),
        );
        for expected in [
            "Remote compaction",
            "Wait for a safe pause",
            "Restart the agent",
            "Context before",
            "Summarizer input",
            "Context after",
            "65% of 200",
            "Kilo Gateway / test-model",
            "Your session is sent",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in\n{text}");
        }
        assert!(
            dialog.banner_close_rect.get().width > 0,
            "the close button rect is published for mouse hit testing"
        );
    }

    #[test]
    fn render_without_banner_publishes_no_close_rect() {
        let dialog = state();
        let text = screen_text(&dialog, None);
        assert!(!text.contains("privacy"));
        assert_eq!(dialog.banner_close_rect.get(), Rect::default());
    }

    #[test]
    fn failed_dialog_offers_resume_once_the_agent_is_stopped() {
        let mut dialog = state();
        dialog.fail("boom".to_string());
        assert_eq!(hint_text(&dialog), "Esc close");
        dialog.is_agent_stopped = true;
        assert!(hint_text(&dialog).contains("resume the original session"));
    }

    #[test]
    fn bars_scale_to_the_largest_figure_and_show_small_values() {
        let tokens = TokenBreakdown {
            before_context: 100_000,
            tail_kept: 1,
            ..TokenBreakdown::default()
        };
        let lines = token_lines(&tokens, 10);
        let line_text = |line: &Line| {
            line.spans
                .iter()
                .map(|s| s.content.to_string())
                .collect::<String>()
        };
        assert!(line_text(&lines[0]).contains(&"█".repeat(10)));
        assert!(
            line_text(&lines[4]).contains('█'),
            "non-zero values keep one cell"
        );
        assert!(!line_text(&lines[3]).contains('█'), "zero shows no cells");
    }
}
