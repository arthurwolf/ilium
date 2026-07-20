//! Pure presentation for the full-right-panel per-agent debug history.
//! Server schema remains style-free; this module maps semantic kinds to
//! icons/colour and performs safe terminal text rendering.

use chrono::{Local, TimeZone};
use ilium_ipc::{
    AgentDebugEntry, AgentDebugEventKind, AgentDebugFieldPresentation, AgentDebugSeverity,
    AgentDebugSource,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{
    AgentDebugLogCache, AgentDebugLogFilter, AgentDebugLogScrollPosition, AgentDebugLogViewState,
    App,
};
use crate::theme;

const BACK_BUTTON_WIDTH: u16 = 17;
const SAVE_BUTTON_WIDTH: u16 = 14;
const RESIZE_FILTER_BUTTON_WIDTH: u16 = 24;
const TOOLBAR_GAP: u16 = 2;

pub fn render(frame: &mut Frame, area: Rect, app: &App, state: &AgentDebugLogViewState) {
    let pane_name = app
        .tree
        .get(state.pane_id)
        .map(|node| node.name.as_str())
        .unwrap_or("closed pane");
    let cache = app.agent_debug_logs.get(&state.pane_id);
    let filter = app.agent_debug_log_filter;
    let entry_count = cache.map_or(0, |cache| filter.visible_entry_count(cache));
    let title = format!(" 🐞 Agent debug log · {pane_name} · {entry_count} meaningful events ");
    let block = theme::block(true).title(theme::chrome_title(&title));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.is_empty() {
        return;
    }
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " ← Back to agent ",
                Style::new()
                    .fg(theme::accent_fg())
                    .bg(theme::accent_bg())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                " 💾 Save log ",
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            resize_filter_span(filter),
            Span::raw("  "),
            Span::styled(
                cache_summary(cache, filter),
                Style::new().add_modifier(Modifier::DIM),
            ),
        ])),
        regions[0],
    );

    let lines = history_lines(cache, filter);
    let content_width = regions[1].width.saturating_sub(1).max(1);
    let total_lines = estimated_wrapped_line_count(&lines, content_width);
    let visible_lines = usize::from(regions[1].height);
    let maximum_top = total_lines.saturating_sub(visible_lines);
    let top = match state.scroll_position {
        AgentDebugLogScrollPosition::FromNewest(offset) => maximum_top.saturating_sub(offset),
        AgentDebugLogScrollPosition::FromOldest(offset) => offset.min(maximum_top),
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
        regions[1],
    );
    if total_lines > visible_lines {
        let mut scrollbar_state = ScrollbarState::new(maximum_top).position(top);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some(" "))
                .style(theme::border_style(true)),
            regions[1],
            &mut scrollbar_state,
        );
    }
    frame.render_widget(
        Paragraph::new(Span::styled(
            "↑↓ / wheel inspect · End newest · R panel resizes · S save · Esc / q back",
            Style::new().add_modifier(Modifier::DIM),
        )),
        regions[2],
    );
}

pub fn back_button_area(area: Rect) -> Rect {
    toolbar_button_area(area, 0, BACK_BUTTON_WIDTH)
}

pub fn save_button_area(area: Rect) -> Rect {
    toolbar_button_area(
        area,
        BACK_BUTTON_WIDTH.saturating_add(TOOLBAR_GAP),
        SAVE_BUTTON_WIDTH,
    )
}

pub fn resize_filter_button_area(area: Rect) -> Rect {
    toolbar_button_area(
        area,
        BACK_BUTTON_WIDTH
            .saturating_add(TOOLBAR_GAP)
            .saturating_add(SAVE_BUTTON_WIDTH)
            .saturating_add(TOOLBAR_GAP),
        RESIZE_FILTER_BUTTON_WIDTH,
    )
}

fn toolbar_button_area(area: Rect, offset: u16, width: u16) -> Rect {
    let inner = theme::block(true).inner(area);
    Rect::new(
        inner.x.saturating_add(offset),
        inner.y,
        width.min(inner.width.saturating_sub(offset)),
        1.min(inner.height),
    )
}

fn resize_filter_span(filter: AgentDebugLogFilter) -> Span<'static> {
    if filter.is_tree_panel_animation_resize_hidden {
        Span::styled(
            " ◉ Panel resizes hidden ",
            Style::new()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " ○ Panel resizes shown  ",
            Style::new().fg(Color::Gray).bg(Color::DarkGray),
        )
    }
}

fn cache_summary(cache: Option<&AgentDebugLogCache>, filter: AgentDebugLogFilter) -> String {
    let Some(cache) = cache else {
        return "Requesting retained history…".to_string();
    };
    if cache.is_loading {
        return "Loading complete retained history…".to_string();
    }
    let dropped = if cache.dropped_entry_count == 0 {
        String::new()
    } else {
        format!(
            " · {} older events discarded by retention",
            cache.dropped_entry_count
        )
    };
    let visible_count = filter.visible_entry_count(cache);
    let hidden_count = filter.hidden_entry_count(cache);
    let filter_summary = if hidden_count == 0 {
        String::new()
    } else {
        format!(" · {hidden_count} panel resize events hidden")
    };
    format!("{visible_count} events shown{filter_summary}{dropped}")
}

fn history_lines(
    cache: Option<&AgentDebugLogCache>,
    filter: AgentDebugLogFilter,
) -> Vec<Line<'static>> {
    let Some(cache) = cache else {
        return vec![Line::from(""), Line::from("  ⏳ Requesting history…")];
    };
    if cache.entries.is_empty() {
        return vec![
            Line::from(""),
            Line::from(Span::styled(
                "  ◌ No events captured yet",
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Events appear here as detection, prompts, sessions, and LLM work occur.",
                Style::new().add_modifier(Modifier::DIM),
            )),
        ];
    }

    if filter.visible_entry_count(cache) == 0 {
        return vec![
            Line::from(""),
            Line::from(Span::styled(
                "  ◉ All retained events are hidden by the panel-resize filter",
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Press R or use the toolbar switch to reveal left-panel animation resizes.",
                Style::new().add_modifier(Modifier::DIM),
            )),
        ];
    }

    let mut lines = Vec::new();
    for entry in cache.entries.iter().filter(|entry| filter.includes(entry)) {
        lines.push(Line::from(""));
        lines.push(event_heading(entry));
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(
                sanitize_terminal_text(&entry.summary),
                Style::new().add_modifier(Modifier::BOLD),
            ),
        ]));
        if let Some(context) = context_summary(entry) {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(context, Style::new().add_modifier(Modifier::DIM)),
            ]));
        }
        if let Some(correlation_id) = entry.correlation_id.as_deref() {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    format!("↳ correlation: {}", sanitize_terminal_text(correlation_id)),
                    Style::new().add_modifier(Modifier::DIM),
                ),
            ]));
        }
        for field in &entry.fields {
            let value = sanitize_terminal_text(&field.value);
            let prefix = match field.presentation {
                AgentDebugFieldPresentation::Plain => "  ",
                AgentDebugFieldPresentation::Code => "⌘ ",
                AgentDebugFieldPresentation::Multiline => "↳ ",
                AgentDebugFieldPresentation::Sensitive => "🔒 ",
            };
            let value_style = match field.presentation {
                AgentDebugFieldPresentation::Code => Style::new().fg(Color::Cyan),
                AgentDebugFieldPresentation::Sensitive => Style::new().fg(Color::Yellow),
                AgentDebugFieldPresentation::Plain | AgentDebugFieldPresentation::Multiline => {
                    Style::new()
                }
            };
            for (index, value_line) in value.split('\n').enumerate() {
                lines.push(Line::from(vec![
                    Span::styled(
                        if index == 0 {
                            format!("    {prefix}{}: ", sanitize_terminal_text(&field.label))
                        } else {
                            "      │ ".to_string()
                        },
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                    Span::styled(value_line.to_string(), value_style),
                ]));
            }
        }
    }
    lines.push(Line::from(""));
    lines
}

fn event_heading(entry: &AgentDebugEntry) -> Line<'static> {
    let (icon, category) = event_identity(&entry.kind);
    let severity_style = match entry.severity {
        AgentDebugSeverity::Trace => Style::new().fg(Color::DarkGray),
        AgentDebugSeverity::Information => Style::new().fg(Color::Cyan),
        AgentDebugSeverity::Success => Style::new().fg(Color::Green),
        AgentDebugSeverity::Warning => Style::new().fg(Color::Yellow),
        AgentDebugSeverity::Error => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
    };
    Line::from(vec![
        Span::styled(
            format!("  {}  ", format_timestamp(entry.occurred_at_unix_millis)),
            Style::new().add_modifier(Modifier::DIM),
        ),
        Span::styled(format!("{icon} {category}"), severity_style),
        Span::styled(
            format!("  · {}", source_label(entry.source)),
            Style::new().add_modifier(Modifier::DIM),
        ),
    ])
}

pub(crate) fn event_identity(kind: &AgentDebugEventKind) -> (&'static str, &'static str) {
    match kind {
        AgentDebugEventKind::DetectionCycle => ("⌁", "DETECTION"),
        AgentDebugEventKind::AgentDetected
        | AgentDebugEventKind::AgentLost
        | AgentDebugEventKind::AgentChanged
        | AgentDebugEventKind::ActivityChanged => ("◉", "AGENT"),
        AgentDebugEventKind::GoalDetected | AgentDebugEventKind::GoalCleared => ("⚑", "GOAL"),
        AgentDebugEventKind::ExecutionStarted
        | AgentDebugEventKind::ExecutionFinished
        | AgentDebugEventKind::ApprovalRequested
        | AgentDebugEventKind::BackgroundWait => ("⚙", "EXECUTION"),
        AgentDebugEventKind::SessionDiscovery
        | AgentDebugEventKind::SessionResolved
        | AgentDebugEventKind::SessionCleared
        | AgentDebugEventKind::ConversationCleared => ("⌘", "SESSION"),
        AgentDebugEventKind::PromptSubmitted
        | AgentDebugEventKind::ScheduledInputCreated
        | AgentDebugEventKind::ScheduledInputReplaced
        | AgentDebugEventKind::ScheduledInputDelivered
        | AgentDebugEventKind::ScheduledInputCleared
        | AgentDebugEventKind::PromptQueued
        | AgentDebugEventKind::QueuedPromptDelivered
        | AgentDebugEventKind::PromptQueueCleared
        | AgentDebugEventKind::PtyInputWritten => ("➜", "INPUT"),
        AgentDebugEventKind::TitleInferenceRequested
        | AgentDebugEventKind::TitleInferenceSucceeded
        | AgentDebugEventKind::TitleInferenceFailed
        | AgentDebugEventKind::TitleInferenceDiscarded
        | AgentDebugEventKind::TitleApplied => ("✦", "LLM TITLE"),
        AgentDebugEventKind::TriggerEvaluated
        | AgentDebugEventKind::TriggerActionStarted
        | AgentDebugEventKind::TriggerActionFinished => ("⌁", "TRIGGER"),
        AgentDebugEventKind::PaneCreated
        | AgentDebugEventKind::PaneRestored
        | AgentDebugEventKind::PaneFocused
        | AgentDebugEventKind::PaneResized
        | AgentDebugEventKind::PaneClosed
        | AgentDebugEventKind::PtyOutputClosed => ("▣", "PANE"),
        AgentDebugEventKind::RecordingEnabled
        | AgentDebugEventKind::RecordingDisabled
        | AgentDebugEventKind::PersistenceSaved
        | AgentDebugEventKind::RetentionNotice => ("◆", "JOURNAL"),
        AgentDebugEventKind::VoiceAction => ("◍", "VOICE"),
        AgentDebugEventKind::IpcRequest => ("⇄", "IPC"),
        AgentDebugEventKind::Error => ("⚠", "ERROR"),
        AgentDebugEventKind::Custom(_) => ("•", "EVENT"),
    }
}

pub(crate) fn format_timestamp(unix_millis: i64) -> String {
    Local
        .timestamp_millis_opt(unix_millis)
        .single()
        .map_or_else(
            || "0000-00-00 00:00:00.000".to_string(),
            |timestamp| timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
        )
}

fn context_summary(entry: &AgentDebugEntry) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(class) = entry.context.class.as_ref() {
        parts.push(match class {
            ilium_core::AgentClass::Claude => "Claude Code".to_string(),
            ilium_core::AgentClass::Codex => "Codex".to_string(),
            ilium_core::AgentClass::Antigravity => "Antigravity".to_string(),
            ilium_core::AgentClass::Other(name) => name.clone(),
        });
    }
    if let Some(activity) = entry.context.activity {
        parts.push(
            match activity {
                ilium_core::AgentActivity::Working => "working",
                ilium_core::AgentActivity::WaitingApproval => "waiting for approval",
                ilium_core::AgentActivity::WaitingBackground => "waiting for background work",
                ilium_core::AgentActivity::Idle => "idle",
                ilium_core::AgentActivity::Done => "done",
            }
            .to_string(),
        );
    }
    if let Some(process_id) = entry.context.process_id {
        parts.push(format!("process {process_id}"));
    }
    if let Some(session_id) = entry.context.session_id.as_deref() {
        parts.push(format!("session {session_id}"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn source_label(source: AgentDebugSource) -> &'static str {
    match source {
        AgentDebugSource::Server => "session server",
        AgentDebugSource::Client => "TUI client",
        AgentDebugSource::Detector => "detection engine",
        AgentDebugSource::SessionDiscovery => "session verifier",
        AgentDebugSource::Pty => "terminal boundary",
        AgentDebugSource::Persistence => "snapshot persistence",
        AgentDebugSource::Inference => "inference worker",
        AgentDebugSource::Voice => "voice control",
    }
}

fn sanitize_terminal_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character == '\n' || character == '\t' || !character.is_control() {
                character
            } else {
                '�'
            }
        })
        .collect()
}

fn estimated_wrapped_line_count(lines: &[Line<'_>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| {
            let line_width = line
                .spans
                .iter()
                .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                .sum::<usize>();
            line_width.max(1).div_ceil(width)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resize_entry(
        sequence: u64,
        summary: &str,
        pane_resize_cause: Option<ilium_ipc::PaneResizeCause>,
    ) -> AgentDebugEntry {
        AgentDebugEntry {
            sequence,
            occurred_at_unix_millis: 1_700_000_000_000,
            severity: AgentDebugSeverity::Information,
            source: AgentDebugSource::Pty,
            kind: AgentDebugEventKind::PaneResized,
            summary: summary.to_string(),
            fields: Vec::new(),
            correlation_id: None,
            context: Default::default(),
            metadata: ilium_ipc::AgentDebugEventMetadata { pane_resize_cause },
        }
    }

    fn rendered_text(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn terminal_control_sequences_are_rendered_inert() {
        assert_eq!(
            sanitize_terminal_text("safe\u{1b}[31m red"),
            "safe�[31m red"
        );
    }

    #[test]
    fn timestamps_use_european_date_order() {
        assert!(format_timestamp(1_700_000_000_000).starts_with("2023-11-"));
    }

    #[test]
    fn default_filter_hides_only_typed_tree_panel_animation_resizes() {
        let cache = AgentDebugLogCache {
            entries: vec![
                resize_entry(
                    1,
                    "tree animation resize",
                    Some(ilium_ipc::PaneResizeCause::TreePanelAnimation),
                ),
                resize_entry(
                    2,
                    "host terminal resize",
                    Some(ilium_ipc::PaneResizeCause::HostTerminal),
                ),
                resize_entry(3, "legacy resize", None),
            ],
            ..AgentDebugLogCache::default()
        };
        let default_filter = AgentDebugLogFilter::default();

        let filtered = rendered_text(&history_lines(Some(&cache), default_filter));

        assert!(!filtered.contains("tree animation resize"));
        assert!(filtered.contains("host terminal resize"));
        assert!(filtered.contains("legacy resize"));
        assert_eq!(default_filter.visible_entry_count(&cache), 2);
        assert_eq!(default_filter.hidden_entry_count(&cache), 1);

        let unfiltered = rendered_text(&history_lines(
            Some(&cache),
            AgentDebugLogFilter {
                is_tree_panel_animation_resize_hidden: false,
            },
        ));
        assert!(unfiltered.contains("tree animation resize"));
    }
}
