//! Presentation of the two pane state slots projected by
//! [`ilium_core::project_pane_signals`]: the glyph (with its animation frame
//! and emphasis) and the hover explanation for each signal.
//!
//! Row order is identity, then the long-term slot ("what is this pane
//! committed to"), then the right-now slot ("what is the process doing at
//! this moment"). This module owns only how a signal looks and what it
//! means; which signal applies is decided once, in `ilium-core`.

use ilium_core::{
    GoalState, NowSignal, ObjectiveSignal, ShellOutputPhase, TaskSignal, TASK_PROGRESS_BUCKETS,
};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::icon_settings::{IconSettings, IconTarget};
use crate::terminal_activity::{TERMINAL_ACTIVITY_FAST_FRAME_MS, TERMINAL_ACTIVITY_SLOW_FRAME_MS};
use crate::tree_ui::{
    BACKGROUND_CLOCK_FRAMES, BACKGROUND_FRAME_MS, DONE_PULSE_MS, SPINNER_FRAMES, SPINNER_FRAME_MS,
    TERMINAL_ACTIVITY_FRAMES,
};

/// Display cells reserved for the long-term slot: a two-cell emoji plus one
/// separating cell, so the right-now glyph never touches it.
pub const OBJECTIVE_COLUMN_WIDTH: usize = 3;
/// Display cells reserved for the right-now slot (one two-cell emoji, a
/// one-cell braille frame plus padding, or the two-glyph progress fill).
pub const NOW_COLUMN_WIDTH: usize = 2;

/// Left-to-right braille fill for a running task, one frame per bucket
/// `0..=TASK_PROGRESS_BUCKETS`. Each braille column rises from a lit
/// baseline, so 0 % still reads as an empty bar rather than as nothing.
/// Braille is narrow in every terminal, so the pair is always two cells.
pub const TASK_PROGRESS_FRAMES: [&str; TASK_PROGRESS_BUCKETS as usize + 1] = [
    "⣀⣀", "⣄⣀", "⣆⣀", "⣇⣀", "⣧⣀", "⣷⣀", "⣿⣀", "⣿⣄", "⣿⣆", "⣿⣇", "⣿⣧", "⣿⣷", "⣿⣿",
];

/// A hover explanation: a bold one-line header and a longer description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusExplanation {
    pub title: &'static str,
    pub body: &'static str,
}

/// Which of the two state slots a pointer is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSlot {
    Objective,
    Now,
}

/// Animates a configured glyph when it belongs to a built-in frame family,
/// starting from that glyph's own position, so customising an animated role
/// to another member of its family (for example 🕗 for the clock) keeps it
/// alive instead of silently freezing it. Any other glyph is a deliberate
/// static choice.
fn animated_from_family(
    configured: &str,
    family: &[char],
    frame_ms: u128,
    elapsed_ms: u128,
) -> String {
    let mut characters = configured.chars();
    let (Some(first), None) = (characters.next(), characters.next()) else {
        return configured.to_string();
    };
    let Some(start) = family.iter().position(|frame| *frame == first) else {
        return configured.to_string();
    };
    let offset = (elapsed_ms / frame_ms) as usize;
    family[(start + offset) % family.len()].to_string()
}

fn task_span(task: TaskSignal, icons: &IconSettings) -> Span<'static> {
    let emphasis = |unread: bool| {
        if unread {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        }
    };
    match task {
        TaskSignal::Pending => Span::styled(
            icons.glyph(IconTarget::TaskPending).to_string(),
            Style::new().fg(Color::Gray),
        ),
        TaskSignal::Running { bucket, degraded } => Span::styled(
            TASK_PROGRESS_FRAMES[usize::from(bucket.min(TASK_PROGRESS_BUCKETS))].to_string(),
            Style::new().fg(if degraded { Color::Yellow } else { Color::Cyan }),
        ),
        TaskSignal::Done { unread } => Span::styled(
            icons.glyph(IconTarget::TaskDone).to_string(),
            emphasis(unread),
        ),
        TaskSignal::Error { unread } => Span::styled(
            icons.glyph(IconTarget::TaskError).to_string(),
            emphasis(unread),
        ),
        TaskSignal::MonitorFailed { unread } => Span::styled(
            icons.glyph(IconTarget::MonitorFailed).to_string(),
            emphasis(unread).fg(Color::Yellow),
        ),
    }
}

const fn goal_icon_target(goal_state: GoalState) -> IconTarget {
    match goal_state {
        GoalState::Active => IconTarget::GoalActive,
        GoalState::Paused => IconTarget::GoalPaused,
        GoalState::Blocked => IconTarget::GoalBlocked,
        GoalState::UsageLimited => IconTarget::GoalUsageLimited,
        GoalState::Reached => IconTarget::GoalReached,
    }
}

/// The long-term slot's glyph. Nothing here animates: a goal or a task's
/// progress changes only when its underlying fact changes.
pub fn objective_span(signal: ObjectiveSignal, icons: &IconSettings) -> Span<'static> {
    match signal {
        ObjectiveSignal::None => Span::raw(""),
        ObjectiveSignal::Goal(goal_state) => {
            let style = match goal_state {
                GoalState::Blocked | GoalState::UsageLimited => {
                    Style::new().add_modifier(Modifier::BOLD)
                }
                GoalState::Active | GoalState::Paused | GoalState::Reached => Style::new(),
            };
            Span::styled(icons.glyph(goal_icon_target(goal_state)).to_string(), style)
        }
        ObjectiveSignal::Task(task) => task_span(task, icons),
        ObjectiveSignal::ScheduledInput => Span::styled(
            icons.glyph(IconTarget::ScheduledInput).to_string(),
            Style::new().fg(Color::Gray),
        ),
    }
}

/// The right-now slot's glyph at `elapsed_ms` (zero freezes every
/// animation, which is how motion level Off is applied).
pub fn now_span(signal: NowSignal, icons: &IconSettings, elapsed_ms: u128) -> Span<'static> {
    match signal {
        NowSignal::None => Span::raw(""),
        NowSignal::NeedsApproval => Span::styled(
            icons.glyph(IconTarget::WaitingApproval).to_string(),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        NowSignal::Working => Span::raw(animated_from_family(
            icons.glyph(IconTarget::Working),
            SPINNER_FRAMES,
            SPINNER_FRAME_MS,
            elapsed_ms,
        )),
        NowSignal::WaitingSubagents => {
            let configured = icons.glyph(IconTarget::WaitingBackground);
            // The historical default `◷` is a stand-in for the clock family.
            let configured = if configured == IconTarget::WaitingBackground.default_glyph() {
                "🕛"
            } else {
                configured
            };
            Span::raw(animated_from_family(
                configured,
                BACKGROUND_CLOCK_FRAMES,
                BACKGROUND_FRAME_MS,
                elapsed_ms,
            ))
        }
        NowSignal::Settling => Span::raw(
            icons
                .glyph(IconTarget::BackgroundTaskStillRunning)
                .to_string(),
        ),
        NowSignal::Parked => Span::styled(
            icons.glyph(IconTarget::Parked).to_string(),
            Style::new().fg(Color::Gray),
        ),
        NowSignal::Task(task) => task_span(task, icons),
        NowSignal::FinishedUnread => {
            let style = if (elapsed_ms / DONE_PULSE_MS).is_multiple_of(2) {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            Span::styled(icons.glyph(IconTarget::Done).to_string(), style)
        }
        NowSignal::Idle => Span::raw(icons.glyph(IconTarget::Idle).to_string()),
        NowSignal::ShellOutput(phase) => {
            let frame_ms = match phase {
                ShellOutputPhase::Fast => u128::from(TERMINAL_ACTIVITY_FAST_FRAME_MS),
                ShellOutputPhase::Slow => u128::from(TERMINAL_ACTIVITY_SLOW_FRAME_MS),
            };
            let index = (elapsed_ms / frame_ms) as usize % TERMINAL_ACTIVITY_FRAMES.len();
            Span::raw(TERMINAL_ACTIVITY_FRAMES[index].to_string())
        }
    }
}

fn task_explanation(task: TaskSignal) -> StatusExplanation {
    match task {
        TaskSignal::Pending => StatusExplanation {
            title: "Task registered, not started yet",
            body: "The agent handed a long-running task to an Ilium progress monitor, and the task's own probe reports that it has not started. Ilium polls it; the agent does not.",
        },
        TaskSignal::Running { degraded: false, .. } => StatusExplanation {
            title: "Task running",
            body: "A long-running task is being watched by an Ilium progress monitor. The bar fills left to right in twelve steps; the exact percentage and status message are in the footer under the terminal.",
        },
        TaskSignal::Running { degraded: true, .. } => StatusExplanation {
            title: "Task running, observation degraded",
            body: "Ilium's last probes of this task failed, so the bar shows the last good report. The task itself may be fine. Ilium keeps retrying and gives up after three consecutive failures.",
        },
        TaskSignal::Done { unread: true } => StatusExplanation {
            title: "Task done, not yet seen",
            body: "The monitored task reported success. Ilium delivers the result to the agent as a new message when its composer is free. Open this pane or type into it to mark the result as seen.",
        },
        TaskSignal::Done { unread: false } => StatusExplanation {
            title: "Task done",
            body: "The monitored task reported success and you have seen it. The result stays in the footer until the monitor is cleared or replaced.",
        },
        TaskSignal::Error { unread: true } => StatusExplanation {
            title: "Task failed, not yet seen",
            body: "The monitored task reported an error. Ilium delivers the failure to the agent as a new message; the error detail is in the footer. Open this pane or type into it to mark it as seen.",
        },
        TaskSignal::Error { unread: false } => StatusExplanation {
            title: "Task failed",
            body: "The monitored task reported an error and you have seen it. The detail stays in the footer until the monitor is cleared or replaced.",
        },
        TaskSignal::MonitorFailed { unread: true } => StatusExplanation {
            title: "Ilium lost sight of the task, not yet seen",
            body: "The progress probe failed repeatedly, so Ilium stopped observing. This does not mean the task failed: its outcome is unknown. Check the job directly; the probe error is in the footer.",
        },
        TaskSignal::MonitorFailed { unread: false } => StatusExplanation {
            title: "Ilium lost sight of the task",
            body: "Observation stopped after repeated probe failures, and you have seen it. The task's outcome is unknown until you check the job directly.",
        },
    }
}

/// Hover text for the long-term slot, or `None` when the slot is empty.
pub fn objective_explanation(signal: ObjectiveSignal) -> Option<StatusExplanation> {
    Some(match signal {
        ObjectiveSignal::None => return None,
        ObjectiveSignal::Goal(GoalState::Active) => StatusExplanation {
            title: "Goal active",
            body: "The agent is pursuing a persistent /goal. It keeps continuing across turns until the goal is reached, paused, or cleared. The provider's own status row is the source of this state.",
        },
        ObjectiveSignal::Goal(GoalState::Paused) => StatusExplanation {
            title: "Goal paused",
            body: "A persistent /goal is set but paused, so the agent will not continue it on its own. Ilium never pauses or resumes a goal by itself; type /goal resume to continue it.",
        },
        ObjectiveSignal::Goal(GoalState::Blocked) => StatusExplanation {
            title: "Goal stalled, needs a decision",
            body: "The agent reported that its /goal is blocked or could not be achieved. It will not continue until you unblock it, change the goal, or resume it.",
        },
        ObjectiveSignal::Goal(GoalState::UsageLimited) => StatusExplanation {
            title: "Goal stopped by usage limits",
            body: "The /goal hit an account usage limit or its token budget. It continues after the limit resets or when you resume it.",
        },
        ObjectiveSignal::Goal(GoalState::Reached) => StatusExplanation {
            title: "Goal reached",
            body: "The agent reported that its persistent /goal is achieved. Codex keeps this marker until the goal is cleared; Claude Code shows it only until the next prompt.",
        },
        ObjectiveSignal::Task(task) => task_explanation(task),
        ObjectiveSignal::ScheduledInput => StatusExplanation {
            title: "Scheduled input pending",
            body: "Ilium will type a scheduled input into this pane when its timer expires. The countdown is shown before the title.",
        },
    })
}

/// Hover text for the right-now slot, or `None` when the slot is empty.
pub fn now_explanation(signal: NowSignal) -> Option<StatusExplanation> {
    Some(match signal {
        NowSignal::None => return None,
        NowSignal::NeedsApproval => StatusExplanation {
            title: "Needs your approval",
            body: "The agent is blocked on a confirmation or a choice, such as a permission prompt or a selection menu. Nothing happens until you answer it.",
        },
        NowSignal::Working => StatusExplanation {
            title: "Working",
            body: "The agent is in the middle of a turn: thinking, calling tools, or writing output.",
        },
        NowSignal::WaitingSubagents => StatusExplanation {
            title: "Waiting on its subagents",
            body: "The agent's turn is still open, but it is blocked on background agents or tasks it dispatched itself. It resumes on its own when they finish.",
        },
        NowSignal::Settling => StatusExplanation {
            title: "Turn over, something still finishing",
            body: "The agent finished its turn, but its summary says a background shell or task it started is still running. It is not counted as finished until that settles.",
        },
        NowSignal::Parked => StatusExplanation {
            title: "Parked: waiting on its task",
            body: "The agent ended its turn to wait for the task shown in the long-term slot. Ilium polls the task and delivers the result as the agent's next message, so this does not ring as finished.",
        },
        NowSignal::Task(TaskSignal::Pending | TaskSignal::Running { .. }) => StatusExplanation {
            title: "Parked: waiting on its task",
            body: "The agent ended its turn and waits for this monitored task while its goal stays set. Ilium polls the task, delivers the result as the next message, and resumes the goal only if it paused it. The bar fills in twelve steps; the exact percentage is in the footer.",
        },
        NowSignal::Task(task) => task_explanation(task),
        NowSignal::FinishedUnread => StatusExplanation {
            title: "Finished, not yet seen",
            body: "The agent completed a turn while you were not looking at this pane. Open it or type into it to mark it as seen.",
        },
        NowSignal::Idle => StatusExplanation {
            title: "Idle",
            body: "The agent is waiting at its prompt with nothing running and nothing unread.",
        },
        NowSignal::ShellOutput(ShellOutputPhase::Fast) => StatusExplanation {
            title: "Output flowing",
            body: "This shell printed output within the last few seconds.",
        },
        NowSignal::ShellOutput(ShellOutputPhase::Slow) => StatusExplanation {
            title: "Recent output",
            body: "This shell printed output within the last minute. The animation stops after a minute of quiet.",
        },
    })
}

/// Longest line (in terminal cells) of a status tooltip's body before it
/// wraps; the popover never exceeds this plus its border.
const TOOLTIP_MAX_TEXT_WIDTH: u16 = 46;

/// Draws a bordered popover for `explanation` just below `anchor` (the
/// hovered glyph's cell), flipping above it when there is no room below and
/// clamping horizontally so it always stays on `screen`. The header is bold;
/// the longer description is wrapped beneath it.
pub fn render_tooltip(
    frame: &mut ratatui::Frame,
    screen: ratatui::layout::Rect,
    anchor: ratatui::layout::Position,
    explanation: StatusExplanation,
) {
    use ratatui::layout::Rect;
    use ratatui::text::Line;
    use ratatui::widgets::{Clear, Paragraph};
    use unicode_width::UnicodeWidthStr;

    let text_width = TOOLTIP_MAX_TEXT_WIDTH.min(screen.width.saturating_sub(4));
    if text_width < 8 {
        return;
    }
    let mut lines = vec![Line::from(Span::styled(
        explanation.title,
        Style::new().add_modifier(Modifier::BOLD),
    ))];
    let body_lines = crate::last_prompt_banner::wrap_lines(explanation.body, text_width);
    let content_width = body_lines
        .iter()
        .map(|line| line.width())
        .chain(std::iter::once(explanation.title.width()))
        .max()
        .unwrap_or(0)
        .min(usize::from(text_width)) as u16;
    lines.extend(body_lines.into_iter().map(Line::from));
    let width = content_width + 4;
    let height = (lines.len() as u16 + 2).min(screen.height);
    let below = anchor.y.saturating_add(1);
    let y = if below.saturating_add(height) <= screen.bottom() {
        below
    } else {
        anchor.y.saturating_sub(height).max(screen.y)
    };
    let x = anchor
        .x
        .min(screen.right().saturating_sub(width))
        .max(screen.x);
    let area = Rect::new(x, y, width.min(screen.width), height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(crate::theme::block(true).padding(ratatui::widgets::Padding::horizontal(1))),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_members_animate_from_their_own_frame_and_others_stay_static() {
        assert_eq!(
            animated_from_family("🕗", BACKGROUND_CLOCK_FRAMES, 220, 0),
            "🕗"
        );
        assert_ne!(
            animated_from_family("🕗", BACKGROUND_CLOCK_FRAMES, 220, 220),
            "🕗"
        );
        assert_eq!(animated_from_family("◐", SPINNER_FRAMES, 90, 900), "◐");
    }

    #[test]
    fn progress_frames_are_two_cells_and_monotonic_in_fill() {
        use unicode_width::UnicodeWidthStr;
        for frame in TASK_PROGRESS_FRAMES {
            assert_eq!(frame.width(), NOW_COLUMN_WIDTH);
        }
        let dots = |frame: &str| -> u32 {
            frame
                .chars()
                .map(|character| (u32::from(character) - 0x2800).count_ones())
                .sum()
        };
        for pair in TASK_PROGRESS_FRAMES.windows(2) {
            assert!(dots(pair[1]) > dots(pair[0]));
        }
    }

    #[test]
    fn every_non_empty_signal_has_an_explanation() {
        let tasks = [
            TaskSignal::Pending,
            TaskSignal::Running {
                bucket: 3,
                degraded: false,
            },
            TaskSignal::Running {
                bucket: 3,
                degraded: true,
            },
            TaskSignal::Done { unread: true },
            TaskSignal::Error { unread: false },
            TaskSignal::MonitorFailed { unread: true },
        ];
        for task in tasks {
            assert!(objective_explanation(ObjectiveSignal::Task(task)).is_some());
            assert!(now_explanation(NowSignal::Task(task)).is_some());
        }
        assert!(objective_explanation(ObjectiveSignal::None).is_none());
        assert!(now_explanation(NowSignal::None).is_none());
    }
}
