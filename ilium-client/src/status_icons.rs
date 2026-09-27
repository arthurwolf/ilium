//! Presentation of the two pane state slots projected by
//! [`ilium_core::project_pane_signals`]: the glyph (with its animation frame
//! and emphasis) and the hover explanation for each signal.
//!
//! Row order is identity, then the long-term slot ("what is this pane
//! committed to"), then the right-now slot ("what is the process doing at
//! this moment"). This module owns only how a signal looks and what it
//! means; which signal applies is decided once, in `ilium-core`.

use ilium_core::{
    GoalState, NowSignal, ObjectiveSignal, PaneStatus, PaneWorkspace, ShellOutputPhase, TaskSignal,
    TASK_PROGRESS_BUCKETS,
};
use ilium_ipc::DetectionReason;
use ilium_ipc::WorkspaceGitStatus;
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

/// Tooltip text assembled from pane provenance and live Git facts. It owns
/// its text because neither branch names nor paths have a static lifetime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooltipContent {
    pub title: String,
    pub body: String,
    pub reason: Option<String>,
}

/// The renderer accepts both the existing static signal explanations and
/// worktree tooltips without allocating static text on each hover frame.
pub trait TooltipExplanation {
    fn title(&self) -> &str;
    fn body(&self) -> &str;
    fn reason(&self) -> Option<&str> {
        None
    }
}

impl TooltipExplanation for StatusExplanation {
    fn title(&self) -> &str {
        self.title
    }

    fn body(&self) -> &str {
        self.body
    }
}

impl TooltipExplanation for &TooltipContent {
    fn title(&self) -> &str {
        &self.title
    }

    fn body(&self) -> &str {
        &self.body
    }

    fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

/// Which of the two state slots a pointer is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSlot {
    Identity,
    Objective,
    Now,
}

/// Hover text for the pane-kind icon. Terminal identity comes from the
/// server's process-tree classification, not from a glyph name.
pub fn identity_explanation(status: &PaneStatus) -> StatusExplanation {
    match status {
        PaneStatus::PlainShell => StatusExplanation {
            title: "Plain terminal",
            body: "No supported agent process was identified in this terminal's process tree.",
        },
        PaneStatus::Agent(class, _) | PaneStatus::AgentWithGoal(class, _, _) => match class {
            ilium_core::AgentClass::Claude => StatusExplanation {
                title: "Claude Code agent",
                body: "Ilium identified a Claude Code process in this terminal's process tree.",
            },
            ilium_core::AgentClass::Codex => StatusExplanation {
                title: "Codex agent",
                body: "Ilium identified a Codex process in this terminal's process tree.",
            },
            ilium_core::AgentClass::Antigravity => StatusExplanation {
                title: "Antigravity agent",
                body: "Ilium identified an Antigravity process in this terminal's process tree.",
            },
            ilium_core::AgentClass::Other(_) => StatusExplanation {
                title: "Custom agent",
                body: "Ilium matched a configured custom agent signature in this terminal's process tree.",
            },
        },
        PaneStatus::Editor { dirty: true } => StatusExplanation {
            title: "Editor with unsaved changes",
            body: "This tree entry is an editor pane whose current buffer has unsaved changes.",
        },
        PaneStatus::Editor { dirty: false } => StatusExplanation {
            title: "Editor",
            body: "This tree entry is an editor pane whose current buffer is saved.",
        },
        PaneStatus::Board => StatusExplanation {
            title: "Board",
            body: "This tree entry is a board pane.",
        },
    }
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
            task_progress_frame(icons, bucket).to_string(),
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

/// Maps the domain's twelve progress buckets to any configured family with
/// two through thirteen frames. Started work always advances past frame zero.
pub(crate) fn task_progress_frame(icons: &IconSettings, bucket: u8) -> &str {
    let frames = &icons.task_progress_frames;
    let steps = frames.len().saturating_sub(1);
    let bucket = usize::from(bucket.min(TASK_PROGRESS_BUCKETS));
    let index = if bucket == 0 {
        0
    } else {
        (bucket * steps).div_ceil(usize::from(TASK_PROGRESS_BUCKETS))
    };
    frames.get(index).map(String::as_str).unwrap_or("")
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
            body: "A long-running task has an Ilium progress monitor, and the task's own probe reports that it has not started. Ilium polls it; the pane's process does not.",
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
            body: "The monitored task reported success. Ilium delivers the result to a supported agent when its composer is free; a plain shell keeps the report in the footer. Open this pane or type into it to mark the result as seen.",
        },
        TaskSignal::Done { unread: false } => StatusExplanation {
            title: "Task done",
            body: "The monitored task reported success and you have seen it. The result stays in the footer until the monitor is cleared or replaced.",
        },
        TaskSignal::Error { unread: true } => StatusExplanation {
            title: "Task failed, not yet seen",
            body: "The monitored task reported an error. Ilium delivers the failure to a supported agent; a plain shell keeps the report in the footer. Open this pane or type into it to mark it as seen.",
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

/// Explains a worktree pane from its persisted creation facts and an optional
/// live Git observation. A missing observation is stated as such; it is not
/// treated as a clean worktree or as evidence that the path is missing.
pub fn workspace_explanation(
    workspace: &PaneWorkspace,
    git_status: Option<&WorkspaceGitStatus>,
) -> TooltipContent {
    let mut details = vec![
        format!("Branch: {}", safe_tooltip_text(&workspace.branch)),
        format!(
            "Base: {} @ {}",
            safe_tooltip_text(&workspace.base_ref),
            safe_tooltip_text(&workspace.base_commit)
        ),
        format!(
            "Worktree: {}",
            safe_tooltip_text(&workspace.worktree_root.to_string_lossy())
        ),
        format!(
            "Repository Git dir: {}",
            safe_tooltip_text(&workspace.repo_common_dir.to_string_lossy())
        ),
        format!(
            "Created: {}",
            chrono::DateTime::from_timestamp(workspace.created_at_unix, 0)
                .map(|time| time.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ),
    ];

    match git_status {
        None => {
            details.push("Live Git status: not yet checked".to_string());
            details.push("Upstream: not yet checked".to_string());
            details.push("Last commit: not yet checked".to_string());
        }
        Some(status) if status.missing => {
            details.push("Live Git status: worktree missing".to_string());
            details.push("Upstream: unavailable".to_string());
            details.push("Last commit: unavailable".to_string());
        }
        Some(status) => {
            if status.detached {
                details.push("Current checkout: detached HEAD".to_string());
            } else if let Some(current_branch) = &status.branch {
                if *current_branch != workspace.branch {
                    details.push(format!(
                        "Current branch: {}",
                        safe_tooltip_text(current_branch)
                    ));
                }
            }
            if status.full_checked_at_unix_millis.is_some() {
                details.push(format!(
                    "Changes: {} staged, {} modified, {} untracked, {} conflicted",
                    status.staged, status.modified, status.untracked, status.conflicted
                ));
            } else {
                details.push("Changes: not yet checked".to_string());
            }
            match &status.upstream {
                Some(upstream) => details.push(format!(
                    "Upstream: {} (+{} / -{})",
                    safe_tooltip_text(upstream),
                    status.ahead,
                    status.behind
                )),
                None => details.push("Upstream: none configured".to_string()),
            }
            details.push(format!(
                "Last commit: {}",
                status
                    .last_commit_subject
                    .as_deref()
                    .map(safe_tooltip_text)
                    .unwrap_or_else(|| "unavailable".to_string())
            ));
        }
    }

    details.push(if workspace.created_by_ilium {
        "Created by Ilium. Removal requires a clean, merged branch and no processes in the worktree."
            .to_string()
    } else {
        "Existing worktree. Ilium will not remove a worktree it did not create.".to_string()
    });

    TooltipContent {
        title: "Agent worktree".to_string(),
        body: details.join("\n"),
        reason: Some(format!(
            "Why: this pane's saved workspace record binds branch «{}» to worktree «{}»; live Git facts above are shown only when a current observation exists.",
            safe_tooltip_text(&workspace.branch),
            safe_tooltip_text(&workspace.worktree_root.to_string_lossy()),
        )),
    }
}

pub(crate) fn safe_tooltip_text(value: &str) -> String {
    value
        .chars()
        .take(240)
        .map(|character| {
            if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect()
}

/// Formats server-owned detector provenance without treating the captured
/// terminal line as markup or instructions. The server supplies the actual
/// predicate and line selected at classification time.
pub fn detection_reason_text(reason: &DetectionReason) -> String {
    let mut text = format!("Why: {}.", safe_tooltip_text(&reason.rule));
    if let Some(observed) = &reason.observed {
        text.push_str(&format!(" Observed «{}».", safe_tooltip_text(observed)));
    }
    if !reason.context.is_empty() {
        text.push(' ');
        text.push_str(&safe_tooltip_text(&reason.context));
    }
    text
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
    explanation: impl TooltipExplanation,
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
        explanation.title().to_string(),
        Style::new().add_modifier(Modifier::BOLD),
    ))];
    let body_lines = crate::last_prompt_banner::wrap_lines(explanation.body(), text_width);
    let reason_lines = explanation
        .reason()
        .map(|reason| crate::last_prompt_banner::wrap_lines(reason, text_width));
    let content_width = body_lines
        .iter()
        .map(|line| line.width())
        .chain(reason_lines.iter().flatten().map(|line| line.width()))
        .chain(std::iter::once(explanation.title().width()))
        .max()
        .unwrap_or(0)
        .min(usize::from(text_width)) as u16;
    lines.extend(body_lines.into_iter().map(Line::from));
    if let Some(reason_lines) = reason_lines {
        lines.push(Line::from(""));
        lines.extend(reason_lines.into_iter().map(|line| {
            Line::from(Span::styled(
                line.to_string(),
                Style::new().fg(Color::DarkGray),
            ))
        }));
    }
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

    use std::path::PathBuf;

    fn workspace() -> PaneWorkspace {
        PaneWorkspace {
            workspace_id: Some("workspace-1".to_string()),
            repo_common_dir: PathBuf::from("/repo/.git"),
            worktree_root: PathBuf::from("/repo.worktrees/agent/fix"),
            branch: "agent/fix".to_string(),
            base_ref: "main".to_string(),
            base_commit: "3f2a9c1".to_string(),
            created_by_ilium: true,
            created_at_unix: 1_700_000_000,
        }
    }

    #[test]
    fn workspace_tooltip_distinguishes_unchecked_live_state_from_a_clean_checkout() {
        let tooltip = workspace_explanation(&workspace(), None);
        assert!(tooltip.body.contains("Branch: agent/fix"));
        assert!(tooltip.body.contains("Base: main @ 3f2a9c1"));
        assert!(tooltip.body.contains("Worktree: /repo.worktrees/agent/fix"));
        assert!(tooltip.body.contains("Repository Git dir: /repo/.git"));
        assert!(tooltip.body.contains("Created: 2023-11-14 22:13:20 UTC"));
        assert!(tooltip.body.contains("Live Git status: not yet checked"));
        assert!(!tooltip.body.contains("0 staged"));
    }

    #[test]
    fn workspace_tooltip_reports_live_counts_and_foreign_ownership() {
        let status = WorkspaceGitStatus {
            branch: Some("agent/fix".to_string()),
            detached: false,
            ahead: 2,
            behind: 1,
            staged: 3,
            modified: 4,
            untracked: 5,
            conflicted: 0,
            upstream: Some("origin/agent/fix".to_string()),
            last_commit_subject: Some("Finish fix".to_string()),
            checked_at_unix_millis: 1,
            full_checked_at_unix_millis: Some(1),
            missing: false,
        };
        let mut workspace = workspace();
        workspace.created_by_ilium = false;
        let tooltip = workspace_explanation(&workspace, Some(&status));
        assert!(tooltip
            .body
            .contains("Changes: 3 staged, 4 modified, 5 untracked, 0 conflicted"));
        assert!(tooltip
            .body
            .contains("Upstream: origin/agent/fix (+2 / -1)"));
        assert!(tooltip.body.contains("Last commit: Finish fix"));
        assert!(tooltip.body.contains("will not remove"));
    }

    #[test]
    fn missing_worktree_and_control_characters_are_explicit() {
        let mut workspace = workspace();
        workspace.branch = "agent/evil\nname".to_string();
        let status = WorkspaceGitStatus {
            branch: None,
            detached: false,
            ahead: 0,
            behind: 0,
            staged: 0,
            modified: 0,
            untracked: 0,
            conflicted: 0,
            upstream: None,
            last_commit_subject: None,
            checked_at_unix_millis: 1,
            full_checked_at_unix_millis: None,
            missing: true,
        };
        let tooltip = workspace_explanation(&workspace, Some(&status));
        assert!(tooltip.body.contains("Branch: agent/evil�name"));
        assert!(tooltip.body.contains("Live Git status: worktree missing"));
        assert!(!tooltip.body.contains("Changes:"));
    }

    #[test]
    fn owned_worktree_tooltip_renders_above_a_second_line_anchor_near_bottom() {
        use ratatui::backend::TestBackend;
        use ratatui::layout::Position;
        use ratatui::Terminal;

        let tooltip = workspace_explanation(&workspace(), None);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                render_tooltip(frame, frame.area(), Position::new(4, 22), &tooltip);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..24)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect();
        assert!(rows[..22].iter().any(|row| row.contains("Agent worktree")));
        assert!(rows[..22]
            .iter()
            .any(|row| row.contains("Branch: agent/fix")));
        assert!(!rows[23].contains("Agent worktree"));
    }

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
    fn shorter_progress_family_maps_zero_and_bucket_boundaries() {
        let icons = IconSettings {
            task_progress_frames: crate::icon_settings::task_progress_preset_frames(2),
            ..IconSettings::default()
        };
        for (bucket, expected) in [
            (0, "🌑"),
            (1, "🌒"),
            (3, "🌒"),
            (4, "🌓"),
            (6, "🌓"),
            (7, "🌔"),
            (9, "🌔"),
            (10, "🌕"),
            (12, "🌕"),
        ] {
            assert_eq!(task_progress_frame(&icons, bucket), expected);
        }
        assert_eq!(
            crate::icon_settings::task_progress_preset_index(&icons.task_progress_frames),
            Some(2)
        );
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
        }
        assert!(objective_explanation(ObjectiveSignal::None).is_none());
        assert!(now_explanation(NowSignal::None).is_none());
    }

    #[test]
    fn provenance_is_rendered_below_the_description_in_muted_text() {
        use ratatui::backend::TestBackend;
        use ratatui::layout::Position;
        use ratatui::Terminal;

        let explanation = TooltipContent {
            title: "Goal paused".to_string(),
            body: "The goal is paused.".to_string(),
            reason: Some("Why: provider footer matched goal paused.".to_string()),
        };
        let mut terminal = Terminal::new(TestBackend::new(70, 16)).unwrap();
        terminal
            .draw(|frame| {
                render_tooltip(frame, frame.area(), Position::new(2, 1), &explanation);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered_rows: Vec<String> = (0..16)
            .map(|y| (0..70).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        let body_row = rendered_rows
            .iter()
            .position(|row| row.contains("The goal is paused."))
            .unwrap();
        let reason_row = rendered_rows
            .iter()
            .position(|row| row.contains("Why: provider footer matched goal paused."))
            .unwrap();
        assert!(reason_row > body_row + 1);
        let reason_cell = &buffer[(4, reason_row as u16)];
        assert_eq!(reason_cell.fg, Color::DarkGray);
    }

    #[test]
    fn detector_provenance_names_the_rule_and_sanitizes_terminal_text() {
        let reason = DetectionReason {
            rule: "Claude notice starts with Goal paused".to_string(),
            observed: Some("Goal paused\u{1b}[31m because of quota".to_string()),
            context: "retained for the same PID".to_string(),
        };
        let text = detection_reason_text(&reason);
        assert!(text.contains("Claude notice starts with Goal paused"));
        assert!(text.contains("Goal paused�[31m because of quota"));
        assert!(text.contains("retained for the same PID"));
        assert!(!text.contains('\u{1b}'));
    }
}
