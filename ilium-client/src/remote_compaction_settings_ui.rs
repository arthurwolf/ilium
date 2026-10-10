//! The "Remote compaction" settings tab and the shared privacy banner.
//!
//! One scrollable page in five sections. Geometry is produced once by
//! [`view`] and shared by rendering, mouse hit testing, help anchors and
//! keyboard scrolling, so they cannot drift apart. The closable privacy box is
//! the first row of the page while it has not been dismissed; the compaction
//! dialog draws the very same box through [`render_privacy_banner`].

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::remote_compaction_settings::{
    technique_description, technique_fidelity, RemoteCompactionRow, RemoteCompactionRowKind,
    RemoteCompactionSettings, TechniqueTarget, THRESHOLD_PERCENT_RANGE,
};
use crate::theme;
use crate::value_control::{leader_span, NUMBER_DECREMENT_GLYPH, NUMBER_INCREMENT_GLYPH};

/// Left margin shared with the other settings tabs.
const INSET: u16 = 2;
/// Width of the label column of a control line.
const LABEL_WIDTH: u16 = 30;
/// Indent of descriptions under a row.
const BODY_INDENT: u16 = 6;
/// The narrowest box the banner draws; narrower areas are clipped by ratatui.
const BANNER_MINIMUM_WIDTH: u16 = 30;
/// Columns of the `[x]` close button.
const CLOSE_BUTTON_WIDTH: u16 = 3;

/// Vertical extent of one selectable row within the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowSpan {
    pub row: RemoteCompactionRow,
    pub first_line: u16,
    pub last_line: u16,
    /// Line carrying the selector/number control (or the banner's close button).
    pub control_line: u16,
    /// Column, relative to the content area, where that control starts.
    pub control_x: u16,
}

pub struct RemoteCompactionView {
    pub lines: Vec<Line<'static>>,
    pub rows: Vec<RowSpan>,
}

/// What a click on a row asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitAction {
    /// Only move the selection.
    Select,
    /// Apply [`RemoteCompactionSettings::adjust`] with this direction:
    /// `-1`/`1` for the halves of a stepper, `0` for a toggle, the banner's
    /// close button or the prompt editor.
    Adjust(i32),
    /// Open the full catalog for a named technique selector.
    OpenChoice(TechniqueTarget),
    /// Open direct numeric entry for one bounded setting.
    EditNumber(RemoteCompactionRow),
}

/// A click, resolved to the row it landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteCompactionHit {
    pub index: usize,
    pub row: RemoteCompactionRow,
    pub action: HitAction,
}

pub fn rows(app: &App) -> Vec<RemoteCompactionRow> {
    RemoteCompactionRow::rows(&app.remote_compaction_settings)
}

/// Whether the privacy banner is still to be drawn, anywhere.
pub fn should_show_privacy_banner(settings: &RemoteCompactionSettings) -> bool {
    settings.should_show_privacy_banner()
}

// ---- privacy banner ----------------------------------------------------

/// The text of the privacy banner, naming the provider and model that would
/// actually receive the transcript.
pub fn privacy_banner_text(inference: &ilium_inference::InferenceSettings) -> String {
    let model = inference.selected_model().trim();
    let model = if model.is_empty() {
        "(no model selected)"
    } else {
        model
    };
    format!(
        "Remote compaction sends this session's transcript (your prompts, code and tool \
         output, possibly including secrets) to {} / {} as currently configured in the \
         Inference tab. That is a privacy decision.",
        inference.selected_provider.label(),
        model
    )
}

/// The banner box, ready to draw at `width` columns, and where its close
/// button sits relative to the box's top-left corner.
struct BannerBox {
    lines: Vec<Line<'static>>,
    close_button: Rect,
}

fn banner_box(text: &str, width: u16, is_selected: bool) -> BannerBox {
    let width = width.max(BANNER_MINIMUM_WIDTH);
    let border = Style::new()
        .fg(theme::accent_bg())
        .add_modifier(Modifier::BOLD);
    let close_style = if is_selected {
        theme::selected_style().add_modifier(Modifier::BOLD)
    } else {
        border
    };
    let title = "╭─ Privacy decision ";
    let right_edge = " ";
    let filler = usize::from(width).saturating_sub(
        UnicodeWidthStr::width(title)
            + usize::from(CLOSE_BUTTON_WIDTH)
            + UnicodeWidthStr::width(right_edge)
            + 2,
    );
    let mut lines = vec![Line::from(vec![
        Span::styled(title.to_owned(), border),
        Span::styled("─".repeat(filler), border),
        Span::styled(" ", border),
        Span::styled("[x]", close_style),
        Span::styled(format!("{right_edge}╮"), border),
    ])];
    let inner = usize::from(width.saturating_sub(4));
    for text_line in wrap(text, inner) {
        let padding = inner.saturating_sub(UnicodeWidthStr::width(text_line.as_str()));
        lines.push(Line::from(vec![
            Span::styled("│ ", border),
            Span::raw(text_line),
            Span::raw(" ".repeat(padding)),
            Span::styled(" │", border),
        ]));
    }
    lines.push(Line::from(Span::styled(
        format!("╰{}╯", "─".repeat(usize::from(width.saturating_sub(2)))),
        border,
    )));
    // " [x] ╮" closes the top edge: the button starts five columns from the
    // right edge of the box.
    let close_x = width.saturating_sub(CLOSE_BUTTON_WIDTH + 2);
    BannerBox {
        lines,
        close_button: Rect::new(close_x, 0, CLOSE_BUTTON_WIDTH, 1),
    }
}

/// Height in rows the banner needs at `width` columns.
pub fn privacy_banner_height(text: &str, width: u16) -> u16 {
    banner_box(text, width, false).lines.len() as u16
}

/// Draws the privacy banner at the top of `area` and returns the hit rect of
/// its close button, in screen coordinates. Callers decide whether to draw it
/// at all with [`should_show_privacy_banner`]. The returned rect is empty when
/// `area` is too small to hold the box.
pub fn render_privacy_banner(frame: &mut Frame, area: Rect, text: &str) -> Rect {
    let banner = banner_box(text, area.width, false);
    let height = banner.lines.len() as u16;
    if area.width < BANNER_MINIMUM_WIDTH || area.height < height {
        return Rect::default();
    }
    let target = Rect::new(area.x, area.y, area.width, height);
    frame.render_widget(Paragraph::new(banner.lines), target);
    Rect::new(
        target.x + banner.close_button.x,
        target.y,
        banner.close_button.width,
        1,
    )
}

// ---- page --------------------------------------------------------------

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && UnicodeWidthStr::width(candidate.as_str()) > width {
            lines.push(std::mem::take(&mut line));
            line = word.to_owned();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000 && tokens.is_multiple_of(1_000) {
        format!("{}k tokens", tokens / 1_000)
    } else {
        format!("{tokens} tokens")
    }
}

/// The agents' own compaction triggers, what they default to, and a warning
/// when one fires before the remote threshold. Empty until the tab has read
/// the configuration.
fn native_trigger_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    use crate::remote_compaction_native::{
        default_window_tokens, describe_limit, evaluate, NativeLimit,
    };
    let accent = Style::new()
        .fg(theme::accent_bg())
        .add_modifier(Modifier::BOLD);
    let warning = Style::new()
        .fg(ratatui::style::Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let triggers = &app.remote_compaction_native.triggers;
    if triggers.is_empty() {
        return Vec::new();
    }
    let threshold = app.remote_compaction_settings.threshold_percent;
    let wrap_width = usize::from(width.saturating_sub(BODY_INDENT + 3)).max(20);
    let mut lines = vec![
        Line::from(Span::styled("  NATIVE COMPACTION OF THE AGENTS", accent)),
        Line::from(""),
    ];
    for trigger in triggers {
        let window = default_window_tokens(trigger.target);
        let result = evaluate(trigger, window, threshold);
        let in_force = match result.effective_trigger_tokens {
            Some(tokens) => format!(
                "compacts at {} ({:.0}% of an assumed {} window)",
                format_tokens(tokens),
                result.effective_percent().unwrap_or_default(),
                format_tokens(window),
            ),
            None => "automatic compaction is off".to_owned(),
        };
        lines.push(Line::from(vec![
            Span::raw(format!(
                "  {:<width$}",
                trigger.target.label(),
                width = usize::from(LABEL_WIDTH)
            )),
            Span::raw(in_force),
        ]));
        let configured = if trigger.limit == NativeLimit::Default {
            "not configured".to_owned()
        } else {
            format!(
                "{} from {}",
                describe_limit(trigger.limit),
                trigger.source.describe()
            )
        };
        let detail = format!(
            "Configured: {configured}. CLI default: {} ({:.0}%).",
            format_tokens(result.default_trigger_tokens),
            result.default_percent(),
        );
        for text in wrap(&detail, wrap_width) {
            lines.push(Line::from(Span::styled(
                format!("{}{text}", " ".repeat(usize::from(BODY_INDENT))),
                dim(),
            )));
        }
        for note in &trigger.notes {
            for text in wrap(note, wrap_width) {
                lines.push(Line::from(Span::styled(
                    format!("{}{text}", " ".repeat(usize::from(BODY_INDENT))),
                    dim(),
                )));
            }
        }
        if result.is_remote_shadowed {
            let advice = match result.highest_working_percent() {
                Some(percent) => format!(
                    "Set the threshold to {percent}% or lower, or raise the agent's own limit."
                ),
                None => "Raise the agent's own limit.".to_owned(),
            };
            let text = format!(
                "Warning: {} compacts itself at {} before the {}% remote threshold ({}) is reached, so remote compaction never starts for it. {advice}",
                trigger.target.label(),
                format_tokens(result.effective_trigger_tokens.unwrap_or_default()),
                threshold,
                format_tokens(result.remote_trigger_tokens),
            );
            for line in wrap(&text, wrap_width) {
                lines.push(Line::from(Span::styled(
                    format!("{}{line}", " ".repeat(usize::from(BODY_INDENT))),
                    warning,
                )));
            }
        }
        lines.push(Line::from(""));
    }
    lines
}

fn format_seconds(seconds: u64) -> String {
    if seconds >= 60 && seconds.is_multiple_of(60) {
        format!("{} min", seconds / 60)
    } else {
        format!("{seconds} s")
    }
}

fn format_minutes(minutes: u64) -> String {
    if minutes >= 60 && minutes.is_multiple_of(60) {
        format!("{} h", minutes / 60)
    } else {
        format!("{minutes} min")
    }
}

/// The control text of a stepper or select row, or the plain value of an
/// editor / read-only row.
fn row_value(row: RemoteCompactionRow, app: &App) -> String {
    let settings = &app.remote_compaction_settings;
    match row {
        RemoteCompactionRow::Threshold => format!("{}%", settings.threshold_percent),
        RemoteCompactionRow::PauseTimeout => format_seconds(settings.pause_timeout_seconds),
        RemoteCompactionRow::Cooldown => format_minutes(settings.cooldown_minutes),
        RemoteCompactionRow::Technique(target) => settings.technique(target).label().to_owned(),
        RemoteCompactionRow::TailTokens => format_tokens(settings.tail_tokens),
        RemoteCompactionRow::ProtectedToolTokens => {
            format_tokens(settings.protected_recent_tool_tokens)
        }
        RemoteCompactionRow::ToolResultChars => {
            format!("{} characters", settings.tool_result_chars)
        }
        RemoteCompactionRow::SummarizerContextTokens => {
            format_tokens(settings.summarizer_context_tokens)
        }
        RemoteCompactionRow::KeepBackups => settings.keep_backups.to_string(),
        RemoteCompactionRow::CustomPrompt => {
            if settings.custom_prompt.trim().is_empty() {
                "Not set (built-in Claude Code prompt)".to_owned()
            } else {
                let first_line = settings.custom_prompt.lines().next().unwrap_or_default();
                let preview: String = first_line.chars().take(40).collect();
                let more = settings.custom_prompt.chars().count() > preview.chars().count();
                format!(
                    "{preview}{} ({} characters)",
                    if more { "…" } else { "" },
                    settings.custom_prompt.chars().count()
                )
            }
        }
        RemoteCompactionRow::Model => {
            let inference = &app.inference_settings;
            let model = inference.selected_model().trim();
            format!(
                "{} / {}",
                inference.selected_provider.label(),
                if model.is_empty() {
                    "(no model selected)"
                } else {
                    model
                }
            )
        }
        RemoteCompactionRow::PrivacyBanner
        | RemoteCompactionRow::Enabled
        | RemoteCompactionRow::Automatic
        | RemoteCompactionRow::RedactSecrets => String::new(),
    }
}

fn row_description(row: RemoteCompactionRow, app: &App) -> String {
    let settings = &app.remote_compaction_settings;
    match row {
        RemoteCompactionRow::PrivacyBanner => String::new(),
        RemoteCompactionRow::Enabled => "Summarize the session with the model chosen in the Inference tab when you press Compact on a Claude or Codex pane, instead of sending /compact. Off keeps today's behaviour."
            .to_owned(),
        RemoteCompactionRow::Automatic => "Start a compaction by itself once the context reaches the threshold below, at the next pause in the agent's work. Needs remote compaction switched on."
            .to_owned(),
        RemoteCompactionRow::Threshold => format!(
            "Context fill, in percent of the model window, that starts an automatic compaction. \
             Claude Code compacts on its own near 84% and Codex at 90% by default, but their \
             settings or environment can lower that; the block above shows the values in force \
             and warns when one fires first ({}-{}%).",
            THRESHOLD_PERCENT_RANGE.0, THRESHOLD_PERCENT_RANGE.1
        ),
        RemoteCompactionRow::PauseTimeout => "How long to wait for the agent to reach a clean pause before it is interrupted with Esc."
            .to_owned(),
        RemoteCompactionRow::Cooldown => "The shortest time before the same pane may be compacted again automatically."
            .to_owned(),
        RemoteCompactionRow::Technique(target) => {
            let technique = settings.technique(target);
            let scope = match target {
                TechniqueTarget::Claude => "Used for Claude Code panes.",
                TechniqueTarget::Codex => "Used for Codex panes.",
                TechniqueTarget::Other => "Used for every other agent.",
            };
            format!(
                "{scope} {} {}",
                technique_description(technique),
                technique_fidelity(technique)
            )
        }
        RemoteCompactionRow::CustomPrompt => "Instructions for the Custom technique. Enter edits them in a multi-line editor and Delete clears them; they apply only while a technique row above is set to Custom."
            .to_owned(),
        RemoteCompactionRow::TailTokens => "The newest conversation kept word for word after the summary, so the agent resumes with its most recent turns intact."
            .to_owned(),
        RemoteCompactionRow::ProtectedToolTokens => "The newest tool output left untouched; older tool results are cut down to the size below before anything is sent."
            .to_owned(),
        RemoteCompactionRow::ToolResultChars => "Characters kept, head and tail, of each older tool result that is sent to the summarizer."
            .to_owned(),
        RemoteCompactionRow::RedactSecrets => "Replace API keys, tokens, private keys and password assignments with a placeholder before the transcript leaves this machine. Best effort, not a guarantee."
            .to_owned(),
        RemoteCompactionRow::SummarizerContextTokens => "Input window assumed for the summarizer model. A longer history is summarized in chunks of this size and then merged."
            .to_owned(),
        RemoteCompactionRow::KeepBackups => "How many .bak copies of the original transcript are kept per session; older ones are removed after a compaction."
            .to_owned(),
        RemoteCompactionRow::Model => "Taken from the Inference tab (provider and model). Change it there; this page only shows what will receive the transcript."
            .to_owned(),
    }
}

/// The section a row opens, if it is the first row of one.
fn section_heading(row: RemoteCompactionRow) -> Option<&'static str> {
    match row {
        RemoteCompactionRow::Enabled => Some("BEHAVIOUR"),
        RemoteCompactionRow::Technique(TechniqueTarget::Claude) => Some("TECHNIQUE PER AGENT"),
        RemoteCompactionRow::CustomPrompt => Some("CUSTOM PROMPT"),
        RemoteCompactionRow::TailTokens => Some("INPUT SHAPING"),
        RemoteCompactionRow::Model => Some("MODEL"),
        _ => None,
    }
}

/// Builds the whole page for `width` columns.
pub fn view(app: &App, selected_row: usize, width: u16) -> RemoteCompactionView {
    let settings = &app.remote_compaction_settings;
    let all_rows = rows(app);
    let is_selected = |row: RemoteCompactionRow| all_rows.get(selected_row) == Some(&row);
    let body_width = usize::from(width.saturating_sub(BODY_INDENT + 3)).max(20);
    let selected_style = theme::selected_style().add_modifier(Modifier::BOLD);
    let accent = Style::new()
        .fg(theme::accent_bg())
        .add_modifier(Modifier::BOLD);
    let control_x = INSET + LABEL_WIDTH;

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans_out: Vec<RowSpan> = Vec::new();

    lines.push(Line::from(""));
    let mut title = vec![Span::styled(
        "Remote compaction",
        Style::new().add_modifier(Modifier::BOLD),
    )];
    if app.remote_compaction_save_deadline.is_some() {
        title.push(Span::styled("   saving…", dim()));
    }
    lines.push(Line::from(title));
    for text in wrap(
        "Shrink a Claude or Codex session by having another model summarize its transcript, \
         then resume the agent from that summary. Toggles and choices save at once; numbers \
         save shortly after the last change.",
        usize::from(width.saturating_sub(INSET + 2)),
    ) {
        lines.push(Line::from(Span::styled(format!("  {text}"), dim())));
    }
    lines.push(Line::from(""));
    lines.extend(native_trigger_lines(app, width));

    for &row in &all_rows {
        if let Some(heading) = section_heading(row) {
            lines.push(Line::from(Span::styled(format!("  {heading}"), accent)));
            lines.push(Line::from(""));
        }
        let first_line = lines.len() as u16;
        let selected = is_selected(row);
        match row.kind() {
            RemoteCompactionRowKind::Banner => {
                let banner = banner_box(
                    &privacy_banner_text(&app.inference_settings),
                    width.saturating_sub(INSET * 2),
                    selected,
                );
                for line in banner.lines {
                    let mut spans = vec![Span::raw(" ".repeat(usize::from(INSET)))];
                    spans.extend(line.spans);
                    lines.push(Line::from(spans));
                }
                spans_out.push(RowSpan {
                    row,
                    first_line,
                    last_line: lines.len() as u16 - 1,
                    control_line: first_line,
                    control_x: INSET + banner.close_button.x,
                });
                lines.push(Line::from(""));
            }
            RemoteCompactionRowKind::Toggle => {
                let is_on = match row {
                    RemoteCompactionRow::Enabled => settings.enabled,
                    RemoteCompactionRow::Automatic => settings.automatic,
                    _ => settings.redact_secrets,
                };
                let label_style = if selected {
                    selected_style
                } else if is_on {
                    Style::new().add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                };
                lines.push(Line::from(Span::styled(
                    format!("  {} {}", if is_on { "[x]" } else { "[ ]" }, row.label()),
                    label_style,
                )));
                push_description(&mut lines, row, app, body_width);
                spans_out.push(RowSpan {
                    row,
                    first_line,
                    last_line: lines.len() as u16 - 1,
                    control_line: first_line,
                    control_x: 0,
                });
                lines.push(Line::from(""));
            }
            RemoteCompactionRowKind::Stepper
            | RemoteCompactionRowKind::Select
            | RemoteCompactionRowKind::Editor
            | RemoteCompactionRowKind::ReadOnly => {
                let label = format!("  {}", row.label());
                let padding = usize::from(control_x)
                    .saturating_sub(UnicodeWidthStr::width(label.as_str()))
                    .max(2);
                let is_stepped = matches!(
                    row.kind(),
                    RemoteCompactionRowKind::Stepper | RemoteCompactionRowKind::Select
                );
                let value = row_value(row, app);
                let shown = if is_stepped {
                    if row.kind() == RemoteCompactionRowKind::Select {
                        format!("← {value} + →")
                    } else {
                        format!("{NUMBER_DECREMENT_GLYPH} {value} {NUMBER_INCREMENT_GLYPH} *")
                    }
                } else {
                    value
                };
                let control_style = if selected {
                    selected_style
                } else if row.kind() == RemoteCompactionRowKind::ReadOnly {
                    dim()
                } else {
                    Style::new().fg(theme::accent_bg())
                };
                let label_style = if selected {
                    Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else {
                    Style::new()
                };
                lines.push(Line::from(vec![
                    Span::styled(label, label_style),
                    leader_span(padding),
                    Span::styled(shown, control_style),
                ]));
                push_description(&mut lines, row, app, body_width);
                spans_out.push(RowSpan {
                    row,
                    first_line,
                    last_line: lines.len() as u16 - 1,
                    control_line: first_line,
                    control_x,
                });
                lines.push(Line::from(""));
            }
        }
    }
    for text in wrap(
        "Up/Down select · Left/Right step · + opens lists/increments · - decreases · * edits numbers · Enter opens/edits · x closes privacy · ? help",
        body_width,
    ) {
        lines.push(Line::from(Span::styled(
            format!("  {text}"),
            dim(),
        )));
    }
    RemoteCompactionView {
        lines,
        rows: spans_out,
    }
}

fn push_description(
    lines: &mut Vec<Line<'static>>,
    row: RemoteCompactionRow,
    app: &App,
    body_width: usize,
) {
    for text in wrap(&row_description(row, app), body_width) {
        lines.push(Line::from(Span::styled(
            format!("{}{text}", " ".repeat(usize::from(BODY_INDENT))),
            dim(),
        )));
    }
}

/// The content area below which the page scrolls: the page is one scrollable
/// block, the banner included.
pub fn render(frame: &mut Frame, area: Rect, app: &App, selected_row: usize, scroll: u16) {
    let view = view(app, selected_row, area.width);
    crate::settings_ui::render_scrollable(frame, area, view.lines, scroll);
    let rows = rows(app);
    for span in &view.rows {
        let Some(control) = value_control(area, scroll, span, app) else {
            continue;
        };
        let selected = rows.get(selected_row) == Some(&span.row);
        let style = if selected {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::accent_bg())
        };
        frame.render_widget(
            Paragraph::new(" ".repeat(usize::from(control.geometry().row.width))).style(style),
            control.geometry().row,
        );
        control.render(
            frame,
            crate::value_control::ControlStyles {
                background: style,
                label: style,
                value: style,
                button: style,
                disabled: style.add_modifier(Modifier::DIM),
            },
        );
    }
}

pub fn max_scroll(app: &App, selected_row: usize, content_area: Rect) -> u16 {
    (view(app, selected_row, content_area.width).lines.len() as u16)
        .saturating_sub(content_area.height)
}

/// Scroll position that keeps the selected row fully inside the viewport.
pub fn scroll_for_selection(
    app: &App,
    content_area: Rect,
    selected_row: usize,
    scroll: u16,
) -> u16 {
    let view = view(app, selected_row, content_area.width);
    let Some(span) = rows(app)
        .get(selected_row)
        .and_then(|row| view.rows.iter().find(|span| span.row == *row))
    else {
        return scroll;
    };
    let height = content_area.height.max(1);
    // The first row of a section scrolls its heading into view with it.
    if span.first_line < scroll {
        span.first_line.saturating_sub(2)
    } else if span.last_line >= scroll + height {
        (span.last_line + 1).saturating_sub(height)
    } else {
        scroll
    }
}

/// Resolves a click at `position` to the row it landed on.
pub fn hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
    app: &App,
) -> Option<RemoteCompactionHit> {
    hit_with_button(
        content_area,
        scroll,
        position,
        crate::value_control::PointerButton::Left,
        app,
    )
}

/// Resolves a click while preserving the button used on a selector's value.
pub fn hit_with_button(
    content_area: Rect,
    scroll: u16,
    position: Position,
    button: crate::value_control::PointerButton,
    app: &App,
) -> Option<RemoteCompactionHit> {
    if !content_area.contains(position) {
        return None;
    }
    let view = view(app, 0, content_area.width);
    let virtual_line = position.y - content_area.y + scroll;
    let virtual_x = position.x - content_area.x;
    let all_rows = rows(app);
    let span = view
        .rows
        .iter()
        .find(|span| (span.first_line..=span.last_line).contains(&virtual_line))?;
    let index = all_rows.iter().position(|row| *row == span.row)?;
    let action = match span.row.kind() {
        RemoteCompactionRowKind::Banner => {
            let is_close = virtual_line == span.control_line
                && (span.control_x..span.control_x + CLOSE_BUTTON_WIDTH).contains(&virtual_x);
            if is_close {
                HitAction::Adjust(0)
            } else {
                HitAction::Select
            }
        }
        RemoteCompactionRowKind::Toggle | RemoteCompactionRowKind::Editor => HitAction::Adjust(0),
        RemoteCompactionRowKind::ReadOnly => HitAction::Select,
        RemoteCompactionRowKind::Stepper | RemoteCompactionRowKind::Select => {
            let control = value_control(content_area, scroll, span, app)?;
            match control.hit(position, button) {
                Some(crate::value_control::ControlAction::PreviousChoice)
                | Some(crate::value_control::ControlAction::Decrement) => HitAction::Adjust(-1),
                Some(crate::value_control::ControlAction::NextChoice)
                | Some(crate::value_control::ControlAction::Increment) => HitAction::Adjust(1),
                Some(crate::value_control::ControlAction::OpenChoices) => {
                    let RemoteCompactionRow::Technique(target) = span.row else {
                        return None;
                    };
                    HitAction::OpenChoice(target)
                }
                Some(crate::value_control::ControlAction::EditNumber) => {
                    HitAction::EditNumber(span.row)
                }
                None => HitAction::Select,
            }
        }
    };
    Some(RemoteCompactionHit {
        index,
        row: span.row,
        action,
    })
}

/// Prepares Remote Compaction selectors and numbers with the shared settings
/// geometry, preserving this page's value formatting and row ownership.
pub fn value_control(
    content_area: Rect,
    scroll: u16,
    span: &RowSpan,
    app: &App,
) -> Option<crate::value_control::ValueControl> {
    use crate::value_control::{ControlKind, ControlSpec, ValueControl};

    let offset = span.control_line.checked_sub(scroll)?;
    if offset >= content_area.height || span.control_x >= content_area.width {
        return None;
    }
    let rect = Rect::new(
        content_area.x + span.control_x,
        content_area.y + offset,
        content_area.width - span.control_x,
        1,
    );
    let (kind, value, previous_enabled, next_enabled) = match span.row.kind() {
        RemoteCompactionRowKind::Select => {
            let RemoteCompactionRow::Technique(target) = span.row else {
                return None;
            };
            (
                ControlKind::Choice,
                app.remote_compaction_settings
                    .technique(target)
                    .label()
                    .to_owned(),
                true,
                true,
            )
        }
        RemoteCompactionRowKind::Stepper => {
            let field = crate::value_settings::SettingsNumber::Remote(span.row);
            let (_, current) = field.snapshot(app);
            let previous_enabled = field.stepped(app, -1).is_ok_and(|value| value != current);
            let next_enabled = field.stepped(app, 1).is_ok_and(|value| value != current);
            (
                ControlKind::Number,
                row_value(span.row, app),
                previous_enabled,
                next_enabled,
            )
        }
        _ => return None,
    };
    Some(ValueControl::new(
        rect,
        ControlSpec {
            kind,
            label: "",
            value: &value,
            label_width: 0,
            previous_enabled,
            next_enabled,
            open_enabled: true,
        },
    ))
}

/// The help topic anchors for every row, as (id, line, row).
pub fn help_anchors(app: &App, width: u16) -> Vec<(&'static str, u16, RemoteCompactionRow)> {
    view(app, 0, width)
        .rows
        .into_iter()
        .map(|span| (span.row.help_id(), span.first_line, span.row))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_remote_compaction::Technique;
    use ratatui::{backend::TestBackend, Terminal};

    fn app() -> App {
        App::new("remote-compaction-ui-test".to_owned(), std::env::temp_dir())
    }

    fn text(view: &RemoteCompactionView) -> String {
        view.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn banner_text_names_the_configured_provider_and_model() {
        let mut inference = ilium_inference::InferenceSettings {
            selected_provider: ilium_inference::InferenceProviderKind::Anthropic,
            ..Default::default()
        };
        inference.anthropic.model = "claude-test-model".to_owned();
        let banner = privacy_banner_text(&inference);
        assert!(banner.contains("Anthropic / claude-test-model"), "{banner}");
        assert!(banner.contains("privacy decision"));
        assert!(banner.contains("transcript"));
        inference.anthropic.model = "  ".into();
        assert!(privacy_banner_text(&inference).contains("(no model selected)"));
    }

    #[test]
    fn the_tab_shows_the_banner_until_it_is_dismissed() {
        let mut app = app();
        let page = text(&view(&app, 0, 110));
        assert!(page.contains("Privacy decision"), "{page}");
        assert!(page.contains("[x]"));
        assert!(page.contains("sends this session's transcript"));
        assert!(page.contains("Kilo Gateway"));
        assert_eq!(
            rows(&app).first(),
            Some(&RemoteCompactionRow::PrivacyBanner)
        );

        app.remote_compaction_settings.dismiss_privacy_banner();
        let page = text(&view(&app, 0, 110));
        assert!(!page.contains("Privacy decision"), "{page}");
        assert!(!page.contains("sends this session's transcript"));
        assert_eq!(rows(&app).first(), Some(&RemoteCompactionRow::Enabled));
        assert!(!should_show_privacy_banner(&app.remote_compaction_settings));
    }

    #[test]
    fn the_page_lists_every_section_row_and_technique_description() {
        let app = app();
        let page = text(&view(&app, 0, 120));
        for heading in [
            "BEHAVIOUR",
            "TECHNIQUE PER AGENT",
            "CUSTOM PROMPT",
            "INPUT SHAPING",
            "MODEL",
        ] {
            assert!(page.contains(heading), "{heading}");
        }
        for row in RemoteCompactionRow::all_rows() {
            if row == RemoteCompactionRow::PrivacyBanner {
                continue;
            }
            assert!(page.contains(row.label()), "{}", row.label());
        }
        assert!(page.contains("[ ] Remote compaction"));
        assert!(page.contains("[x] Redact secrets"));
        assert!(page.contains("- 65% + *"));
        assert!(page.contains("← Claude Code + →"));
        assert!(page.contains("← Codex + →"));
        assert!(page.contains("the real upstream prompt"));
        assert!(page.contains("- 20k tokens + *"));
        assert!(page.contains("- 2 min + *"));
        assert!(page.contains("Kilo Gateway / "));
        assert!(page.contains("Change it there"));
    }

    #[test]
    fn technique_rows_show_provenance_for_each_technique() {
        let mut app = app();
        app.remote_compaction_settings.claude_technique = Technique::BestOfAllWorlds;
        let page = text(&view(&app, 0, 120));
        assert!(page.contains("Ilium's own design"), "{page}");
        app.remote_compaction_settings.claude_technique = Technique::Custom;
        let page = text(&view(&app, 0, 120));
        assert!(page.contains("Your own instructions"));
    }

    #[test]
    fn custom_prompt_row_previews_the_text_and_its_length() {
        let mut app = app();
        assert!(text(&view(&app, 0, 120)).contains("Not set"));
        app.remote_compaction_settings.custom_prompt = "Keep the migration plan.\nSecond".into();
        let page = text(&view(&app, 0, 120));
        assert!(page.contains("Keep the migration plan."), "{page}");
        assert!(page.contains("(31 characters)"), "{page}");
        assert!(!page.contains("Second"));
    }

    #[test]
    fn rendered_page_and_dialog_banner_draw_and_report_the_close_button() {
        let mut app = app();
        let width = 110;
        let view = view(&app, 0, width);
        let height = view.lines.len() as u16 + 1;
        let area = Rect::new(0, 0, width, height);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, area, &app, 0, 0))
            .unwrap();
        let drawn = buffer_text(&terminal);
        assert!(drawn.contains("Privacy decision"), "{drawn}");
        assert!(drawn.contains("Remote compaction"));
        let span = view.rows[0];
        assert_eq!(span.row, RemoteCompactionRow::PrivacyBanner);
        let cells: String = (0..3)
            .map(|offset| {
                terminal.backend().buffer()[(span.control_x + offset, span.control_line)]
                    .symbol()
                    .to_owned()
            })
            .collect();
        assert_eq!(cells, "[x]");

        // The standalone widget draws the identical box and returns the rect.
        let text = privacy_banner_text(&app.inference_settings);
        let banner_area = Rect::new(3, 2, 80, 12);
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let mut close = Rect::default();
        terminal
            .draw(|frame| close = render_privacy_banner(frame, banner_area, &text))
            .unwrap();
        assert_eq!(close.height, 1);
        assert_eq!(close.width, 3);
        let symbols: String = (0..close.width)
            .map(|offset| {
                terminal.backend().buffer()[(close.x + offset, close.y)]
                    .symbol()
                    .to_owned()
            })
            .collect();
        assert_eq!(symbols, "[x]");
        assert_eq!(
            privacy_banner_height(&text, 80) as usize,
            buffer_text(&terminal)
                .lines()
                .filter(|line| line.contains('│') || line.contains('╭') || line.contains('╰'))
                .count()
        );
        // Too small for the box: nothing is drawn and no hit area is claimed.
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal
            .draw(|frame| close = render_privacy_banner(frame, Rect::new(0, 0, 80, 2), &text))
            .unwrap();
        assert_eq!(close, Rect::default());
        app.remote_compaction_settings.dismiss_privacy_banner();
    }

    #[test]
    fn clicks_resolve_to_toggles_steppers_the_banner_close_button_and_inert_text() {
        let mut app = app();
        let content = Rect::new(0, 0, 110, 200);
        let view_now = view(&app, 0, content.width);
        let span_of =
            |row: RemoteCompactionRow| *view_now.rows.iter().find(|span| span.row == row).unwrap();
        let all_rows = rows(&app);
        let index_of = |row: RemoteCompactionRow| all_rows.iter().position(|r| *r == row).unwrap();

        let banner = span_of(RemoteCompactionRow::PrivacyBanner);
        let close = hit(
            content,
            0,
            Position::new(banner.control_x + 1, banner.control_line),
            &app,
        )
        .unwrap();
        assert_eq!(close.action, HitAction::Adjust(0));
        assert_eq!(close.row, RemoteCompactionRow::PrivacyBanner);
        let elsewhere = hit(content, 0, Position::new(5, banner.first_line + 1), &app).unwrap();
        assert_eq!(elsewhere.action, HitAction::Select);

        let enabled = span_of(RemoteCompactionRow::Enabled);
        let toggle = hit(content, 0, Position::new(10, enabled.first_line), &app).unwrap();
        assert_eq!(toggle.action, HitAction::Adjust(0));
        assert_eq!(toggle.index, index_of(RemoteCompactionRow::Enabled));

        let threshold = span_of(RemoteCompactionRow::Threshold);
        let threshold_control = value_control(content, 0, &threshold, &app).unwrap();
        let threshold_geometry = threshold_control.geometry();
        let at = |x: u16, y: u16| hit(content, 0, Position::new(x, y), &app).unwrap().action;
        assert_eq!(
            at(threshold_geometry.previous.x, threshold_geometry.previous.y),
            HitAction::Adjust(-1)
        );
        assert_eq!(
            at(threshold_geometry.next.x, threshold_geometry.next.y),
            HitAction::Adjust(1)
        );
        assert_eq!(
            at(threshold_geometry.open.x, threshold_geometry.open.y),
            HitAction::EditNumber(RemoteCompactionRow::Threshold)
        );
        assert_eq!(
            at(threshold.control_x - 1, threshold.control_line),
            HitAction::Select
        );
        assert_eq!(
            at(threshold.control_x + 3, threshold.control_line + 1),
            HitAction::Select
        );

        let technique = span_of(RemoteCompactionRow::Technique(TechniqueTarget::Codex));
        let technique_control = value_control(content, 0, &technique, &app).unwrap();
        let technique_geometry = technique_control.geometry();
        assert_eq!(
            hit_with_button(
                content,
                0,
                Position::new(technique_geometry.value.x, technique_geometry.value.y),
                crate::value_control::PointerButton::Left,
                &app,
            )
            .unwrap()
            .action,
            HitAction::Adjust(1)
        );
        assert_eq!(
            hit_with_button(
                content,
                0,
                Position::new(technique_geometry.value.x, technique_geometry.value.y),
                crate::value_control::PointerButton::Right,
                &app,
            )
            .unwrap()
            .action,
            HitAction::Adjust(-1)
        );
        assert_eq!(
            at(technique_geometry.open.x, technique_geometry.open.y),
            HitAction::OpenChoice(TechniqueTarget::Codex)
        );
        let prompt = span_of(RemoteCompactionRow::CustomPrompt);
        assert_eq!(at(10, prompt.first_line), HitAction::Adjust(0));
        let model = span_of(RemoteCompactionRow::Model);
        assert_eq!(at(10, model.first_line), HitAction::Select);
        assert!(hit(content, 0, Position::new(200, 0), &app).is_none());

        // Scrolling shifts the resolved line.
        let scrolled = hit(content, enabled.first_line, Position::new(10, 0), &app).unwrap();
        assert_eq!(scrolled.row, RemoteCompactionRow::Enabled);
        app.remote_compaction_settings.dismiss_privacy_banner();
        assert!(rows(&app).first() != Some(&RemoteCompactionRow::PrivacyBanner));
    }

    #[test]
    fn rendered_remote_controls_use_shared_choice_and_centered_number_layouts() {
        let app = app();
        let content = Rect::new(0, 0, 110, 200);
        let page = view(&app, 0, content.width);
        let height = page.lines.len() as u16;
        let mut terminal = Terminal::new(TestBackend::new(content.width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, content, &app, 0, 0))
            .unwrap();

        for row in [
            RemoteCompactionRow::Threshold,
            RemoteCompactionRow::Technique(TechniqueTarget::Claude),
        ] {
            let span = page.rows.iter().find(|span| span.row == row).unwrap();
            let control = value_control(content, 0, span, &app).unwrap();
            let geometry = control.geometry();
            let painted: String = (geometry.row.x..geometry.row.right())
                .map(|x| {
                    terminal
                        .backend()
                        .buffer()
                        .cell((x, geometry.row.y))
                        .unwrap()
                        .symbol()
                })
                .collect();
            match row.kind() {
                RemoteCompactionRowKind::Stepper => {
                    assert!(painted.contains('-'), "{painted:?}");
                    assert!(painted.contains('+'), "{painted:?}");
                    assert!(painted.contains('*'), "{painted:?}");
                    assert!(geometry.value_slot.width > geometry.value.width);
                    assert_eq!(
                        geometry.value.x - geometry.value_slot.x,
                        (geometry.value_slot.width - geometry.value.width) / 2
                    );
                }
                RemoteCompactionRowKind::Select => {
                    assert!(painted.contains('←'), "{painted:?}");
                    assert!(painted.contains('+'), "{painted:?}");
                    assert!(painted.contains('→'), "{painted:?}");
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn settings_screen_paints_and_hits_remote_controls_in_the_same_geometry() {
        use crate::value_control::{PointerButton, NUMBER_DECREMENT_GLYPH, NUMBER_INCREMENT_GLYPH};

        let app = app();
        let state = crate::app::SettingsState {
            tab: crate::app::SettingsTab::RemoteCompaction,
            ..Default::default()
        };
        let screen = Rect::new(0, 0, 140, 180);
        let mut layout = crate::settings_ui::compute_layout_for_mode(screen, &app, &state);
        let instructions =
            crate::instruction_settings::panel_height(state.tab, layout.content_area);
        layout.content_area.y += instructions;
        layout.content_area.height = layout.content_area.height.saturating_sub(instructions);

        let page = view(&app, state.selected_row, layout.content_area.width);
        let mut terminal = Terminal::new(TestBackend::new(screen.width, screen.height)).unwrap();
        terminal
            .draw(|frame| crate::settings_ui::render(frame, screen, &app, &state))
            .unwrap();

        for row in [
            RemoteCompactionRow::Threshold,
            RemoteCompactionRow::Technique(TechniqueTarget::Claude),
        ] {
            let span = page.rows.iter().find(|span| span.row == row).unwrap();
            let control = value_control(layout.content_area, state.scroll, span, &app).unwrap();
            let geometry = control.geometry();
            let expected = match row.kind() {
                RemoteCompactionRowKind::Stepper => {
                    [NUMBER_DECREMENT_GLYPH, NUMBER_INCREMENT_GLYPH, "*"]
                }
                RemoteCompactionRowKind::Select => ["←", "→", "+"],
                _ => unreachable!(),
            };
            for (rectangle, symbol) in [
                (geometry.previous, expected[0]),
                (geometry.next, expected[1]),
                (geometry.open, expected[2]),
            ] {
                assert_eq!(
                    terminal.backend().buffer()[(rectangle.x, rectangle.y)].symbol(),
                    symbol,
                    "{row:?}"
                );
            }
            match row.kind() {
                RemoteCompactionRowKind::Stepper => {
                    assert!(geometry.previous.x < geometry.value.x);
                    assert!(geometry.value.right() <= geometry.next.x);
                    assert!(geometry.next.x < geometry.open.x);
                }
                RemoteCompactionRowKind::Select => {
                    assert!(geometry.previous.x < geometry.value.x);
                    assert!(geometry.value.right() <= geometry.open.x);
                    assert!(geometry.open.x < geometry.next.x);
                }
                _ => unreachable!(),
            }

            let open_position = Position::new(geometry.open.x, geometry.open.y);
            let expected_open = match row {
                RemoteCompactionRow::Threshold => HitAction::EditNumber(row),
                RemoteCompactionRow::Technique(target) => HitAction::OpenChoice(target),
                _ => unreachable!(),
            };
            assert_eq!(
                hit_with_button(
                    layout.content_area,
                    state.scroll,
                    open_position,
                    PointerButton::Left,
                    &app,
                )
                .unwrap()
                .action,
                expected_open
            );
        }

        let span = page
            .rows
            .iter()
            .find(|span| span.row == RemoteCompactionRow::Technique(TechniqueTarget::Claude))
            .unwrap();
        let geometry = value_control(layout.content_area, state.scroll, span, &app)
            .unwrap()
            .geometry();
        let value = Position::new(geometry.value.x, geometry.value.y);
        assert_eq!(
            hit_with_button(
                layout.content_area,
                state.scroll,
                value,
                PointerButton::Left,
                &app,
            )
            .unwrap()
            .action,
            HitAction::Adjust(1)
        );
        assert_eq!(
            hit_with_button(
                layout.content_area,
                state.scroll,
                value,
                PointerButton::Right,
                &app,
            )
            .unwrap()
            .action,
            HitAction::Adjust(-1)
        );
    }

    #[test]
    fn scroll_follows_the_selection_and_the_page_scrolls_to_its_end() {
        let app = app();
        let content = Rect::new(0, 0, 100, 20);
        assert_eq!(scroll_for_selection(&app, content, 0, 0), 0);
        let last = rows(&app).len() - 1;
        let scroll = scroll_for_selection(&app, content, last, 0);
        assert!(scroll > 0);
        let view = view(&app, last, content.width);
        let span = view.rows.last().unwrap();
        assert!(span.last_line < scroll + content.height);
        assert!(scroll <= max_scroll(&app, last, content));
        assert!(scroll_for_selection(&app, content, 0, scroll) < scroll);
    }

    #[test]
    fn every_row_has_a_help_anchor_on_its_first_line() {
        let app = app();
        let anchors = help_anchors(&app, 110);
        assert_eq!(anchors.len(), rows(&app).len());
        for (id, line, row) in anchors {
            assert_eq!(id, row.help_id());
            assert!(line > 0);
        }
    }

    #[test]
    fn a_pending_numeric_save_is_visible_in_the_title() {
        let mut app = app();
        assert!(!text(&view(&app, 0, 110)).contains("saving…"));
        app.remote_compaction_save_deadline =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(600));
        assert!(text(&view(&app, 0, 110)).contains("saving…"));
    }

    #[test]
    fn narrow_widths_never_panic_and_keep_the_banner_inside_the_page() {
        let app = app();
        for width in [20, 30, 40, 60] {
            let view = view(&app, 0, width);
            assert!(!view.lines.is_empty());
            let area = Rect::new(0, 0, width, view.lines.len() as u16);
            let mut terminal =
                Terminal::new(TestBackend::new(width, view.lines.len() as u16)).unwrap();
            terminal
                .draw(|frame| render(frame, area, &app, 0, 0))
                .unwrap();
        }
    }
}
