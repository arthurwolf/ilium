//! Presentation of the two pane state slots projected by
//! [`ilium_core::project_pane_signals`]: the glyph (with its animation frame
//! and emphasis) and the hover explanation for each signal.
//!
//! Row order is identity, then the long-term slot ("what is this pane
//! committed to"), then the right-now slot ("what is the process doing at
//! this moment"). This module owns only how a signal looks and what it
//! means; which signal applies is decided once, in `ilium-core`.

use ilium_core::{
    GoalState, NowSignal, ObjectiveSignal, PaneContentKind, PaneStatus, PaneWorkspace,
    ShellOutputPhase, TaskSignal, TASK_PROGRESS_BUCKETS,
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
        PaneStatus::Agent(agent) => match &agent.class {
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
        PaneStatus::AgentUnavailable(recovery) => StatusExplanation {
            title: "Former agent; recovery available",
            body: recovery.availability.explanation(),
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

pub(crate) fn missing_identity_evidence_reason(content: PaneContentKind) -> String {
    match content {
        PaneContentKind::Terminal => "Why: the server has not sent this terminal's process-classification evidence yet.".to_string(),
        PaneContentKind::Editor => "Why: the saved tree marks this as an editor pane, so the icon identifies its pane kind rather than an agent process.".to_string(),
        PaneContentKind::Board => "Why: the saved tree marks this as a board pane, so the icon identifies its pane kind rather than an agent process.".to_string(),
    }
}

/// Explains the state used by the tree's identity icon. Detection-backed
/// panes name their recorded predicate; local editor dirty state comes from
/// the authoritative server-owned pane status rather than an agent detector.
pub(crate) fn identity_provenance_reason(
    status: &PaneStatus,
    detection: Option<&ilium_ipc::DetectionReason>,
) -> String {
    if let PaneStatus::AgentUnavailable(recovery) = status {
        return format!(
            "Why: this pane previously belonged to a {} process; {}. Its live composer is not established.",
            safe_tooltip_text(recovery.process.class.label()),
            recovery.availability.label(),
        );
    }
    if let Some(detection) = detection {
        return detection_reason_text(detection);
    }

    match status {
        PaneStatus::Editor { dirty: true } => {
            "Why: the server-owned pane status reports dirty=true, so the editor is marked as having unsaved changes.".to_string()
        }
        PaneStatus::Editor { dirty: false } => {
            "Why: the server-owned pane status reports dirty=false, so the editor is shown as saved.".to_string()
        }
        PaneStatus::Board => {
            "Why: the server-owned pane status identifies this entry as a board pane.".to_string()
        }
        PaneStatus::Agent(agent) => {
            let class_name = match &agent.class {
                ilium_core::AgentClass::Claude => "Claude Code",
                ilium_core::AgentClass::Codex => "Codex",
                ilium_core::AgentClass::Antigravity => "Antigravity",
                ilium_core::AgentClass::Other(name) => name,
            };
            format!(
                "Why: the server's current status identifies a {} agent; its matching detector details were not included yet.",
                safe_tooltip_text(class_name)
            )
        }
        PaneStatus::AgentUnavailable(_) => unreachable!("handled before detection evidence"),
        PaneStatus::PlainShell => {
            missing_identity_evidence_reason(PaneContentKind::Terminal)
        }
    }
}

fn bounded_shell_reason(text: &str) -> String {
    const LIMIT: usize = 512;
    const MARKER: &str = " [truncated]";

    let mut result = String::new();
    for (index, character) in text.chars().enumerate() {
        if index >= LIMIT {
            let retained = LIMIT - MARKER.chars().count();
            result = result.chars().take(retained).collect();
            result.push_str(MARKER);
            return result;
        }

        let unsafe_character = character.is_control()
            || matches!(
                character,
                '\u{061c}'
                    | '\u{200e}'
                    | '\u{200f}'
                    | '\u{2028}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}'
            );
        result.push(if unsafe_character {
            '\u{fffd}'
        } else {
            character
        });
    }
    result
}

pub(crate) fn shell_output_reason(
    phase: ShellOutputPhase,
    snapshot: Option<crate::terminal_activity::TerminalActivitySnapshot<'_>>,
) -> Option<String> {
    use crate::terminal_activity::{
        TerminalActivityCause, TerminalActivityPhase, TERMINAL_ACTIVITY_FAST_WINDOW_MS,
        TERMINAL_ACTIVITY_VISIBLE_WINDOW_MS,
    };

    let snapshot = snapshot?;
    let matches_phase = matches!(
        (phase, snapshot.phase),
        (ShellOutputPhase::Fast, TerminalActivityPhase::Fast)
            | (ShellOutputPhase::Slow, TerminalActivityPhase::Slow)
    );
    if !matches_phase {
        return None;
    }

    let (lower_ms, upper_ms) = match snapshot.phase {
        TerminalActivityPhase::Fast => (0, TERMINAL_ACTIVITY_FAST_WINDOW_MS),
        TerminalActivityPhase::Slow => (
            TERMINAL_ACTIVITY_FAST_WINDOW_MS,
            TERMINAL_ACTIVITY_VISIBLE_WINDOW_MS,
        ),
    };
    let mut reason = match snapshot.cause {
        TerminalActivityCause::VisibleTextChanged(evidence) => format!(
            "Why: latest event: parsed visible-cell text fingerprint changed while handling live seq {}..{}.",
            evidence.first_sequence, evidence.sequence
        ),
        TerminalActivityCause::KeyInputQueued { byte_count } => format!(
            "Why: latest event: input queued: KeyInput; bytes.len() = {} > 0. Server/PTY receipt and execution unconfirmed.",
            byte_count
        ),
        TerminalActivityCause::TerminalTextQueued => "Why: latest event: input queued: SubmitTerminalText. Server/PTY receipt and execution unconfirmed.".to_owned(),
    };
    reason.push_str(&format!(
        " {:?}: {} <= age_ms {} < {}.",
        snapshot.phase, lower_ms, snapshot.age_ms, upper_ms
    ));

    if let TerminalActivityCause::VisibleTextChanged(evidence) = snapshot.cause {
        match evidence.changed_rows {
            None => reason.push_str(" Row layouts were not comparable."),
            Some(0) => reason.push_str(" Row hashes did not localize the fingerprint change."),
            Some(count) => reason.push_str(&format!(
                " {} changed row positions; {} sampled. Scrolling/erasure can do this.",
                count,
                evidence.rows.len()
            )),
        }
        if !evidence.rows.is_empty() {
            reason.push_str(" Samples join cell text; empty cells omitted.");
        }
        for row in &evidence.rows {
            if row.blank {
                reason.push_str(&format!(" r{} blank at update.", row.row_number));
                continue;
            }
            reason.push_str(&format!(" r{} at update «{}»", row.row_number, row.text));
            if row.truncated {
                reason.push_str(" [truncated]");
            }
            reason.push('.');
        }
    }

    Some(bounded_shell_reason(&reason))
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

fn task_span(task: TaskSignal, icons: &IconSettings, use_stable_glyphs: bool) -> Span<'static> {
    let emphasis = |unread: bool| {
        if unread {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        }
    };
    match task {
        TaskSignal::Pending => Span::styled(
            icons
                .glyph_for_display(IconTarget::TaskPending, use_stable_glyphs)
                .to_string(),
            Style::new().fg(Color::Gray),
        ),
        TaskSignal::Running { bucket, degraded } => Span::styled(
            task_progress_frame(icons, bucket).to_string(),
            Style::new().fg(if degraded { Color::Yellow } else { Color::Cyan }),
        ),
        TaskSignal::Done { unread } => Span::styled(
            icons
                .glyph_for_display(IconTarget::TaskDone, use_stable_glyphs)
                .to_string(),
            emphasis(unread),
        ),
        TaskSignal::Error { unread } => Span::styled(
            icons
                .glyph_for_display(IconTarget::TaskError, use_stable_glyphs)
                .to_string(),
            emphasis(unread),
        ),
        TaskSignal::MonitorFailed { unread } => Span::styled(
            icons
                .glyph_for_display(IconTarget::MonitorFailed, use_stable_glyphs)
                .to_string(),
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
pub fn objective_span(
    signal: ObjectiveSignal,
    icons: &IconSettings,
    use_stable_glyphs: bool,
) -> Span<'static> {
    match signal {
        ObjectiveSignal::None => Span::raw(""),
        ObjectiveSignal::Goal(goal_state) => {
            let style = match goal_state {
                GoalState::Blocked | GoalState::UsageLimited => {
                    Style::new().add_modifier(Modifier::BOLD)
                }
                GoalState::Active | GoalState::Paused | GoalState::Reached => Style::new(),
            };
            Span::styled(
                icons
                    .glyph_for_display(goal_icon_target(goal_state), use_stable_glyphs)
                    .to_string(),
                style,
            )
        }
        ObjectiveSignal::Task(task) => task_span(task, icons, use_stable_glyphs),
        ObjectiveSignal::ScheduledInput => Span::styled(
            icons
                .glyph_for_display(IconTarget::ScheduledInput, use_stable_glyphs)
                .to_string(),
            Style::new().fg(Color::Gray),
        ),
    }
}

/// The right-now slot's glyph at `elapsed_ms` (zero freezes every
/// animation, which is how motion level Off is applied).
pub fn now_span(
    signal: NowSignal,
    icons: &IconSettings,
    elapsed_ms: u128,
    use_stable_glyphs: bool,
) -> Span<'static> {
    match signal {
        NowSignal::None => Span::raw(""),
        NowSignal::AgentUnavailable(_) => Span::styled(
            icons
                .glyph_for_display(IconTarget::AgentUnavailable, use_stable_glyphs)
                .to_string(),
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        NowSignal::NeedsApproval => Span::styled(
            icons
                .glyph_for_display(IconTarget::WaitingApproval, use_stable_glyphs)
                .to_string(),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        NowSignal::Working => Span::raw(animated_from_family(
            icons.glyph_for_display(IconTarget::Working, use_stable_glyphs),
            SPINNER_FRAMES,
            SPINNER_FRAME_MS,
            elapsed_ms,
        )),
        NowSignal::WaitingSubagents => {
            let configured =
                icons.glyph_for_display(IconTarget::WaitingBackground, use_stable_glyphs);
            // The historical default `◷` is a stand-in for the clock family.
            let configured = if !use_stable_glyphs
                && configured == IconTarget::WaitingBackground.default_glyph()
            {
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
                .glyph_for_display(IconTarget::BackgroundTaskStillRunning, use_stable_glyphs)
                .to_string(),
        ),
        NowSignal::Parked => Span::styled(
            icons
                .glyph_for_display(IconTarget::Parked, use_stable_glyphs)
                .to_string(),
            Style::new().fg(Color::Gray),
        ),
        NowSignal::FinishedUnread => {
            let style = if (elapsed_ms / DONE_PULSE_MS).is_multiple_of(2) {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            Span::styled(
                icons
                    .glyph_for_display(IconTarget::Done, use_stable_glyphs)
                    .to_string(),
                style,
            )
        }
        NowSignal::Idle => Span::raw(
            icons
                .glyph_for_display(IconTarget::Idle, use_stable_glyphs)
                .to_string(),
        ),
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
            body: "The provider surfaced a persistent /goal in its paused state. Ilium reports that state but does not control whether or when the provider resumes it.",
        },
        ObjectiveSignal::Goal(GoalState::Blocked) => StatusExplanation {
            title: "Goal stalled, needs a decision",
            body: "The agent reported that its /goal is blocked or could not be achieved. It will not continue until you unblock it, change the goal, or resume it.",
        },
        ObjectiveSignal::Goal(GoalState::UsageLimited) => StatusExplanation {
            title: "Goal stopped by usage limits",
            body: "The provider reports that the goal stopped after hitting an account usage limit or token budget. Ilium does not control when that limit resets or how the provider continues.",
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
        NowSignal::AgentUnavailable(availability) => StatusExplanation {
            title: "Agent unavailable",
            body: availability.explanation(),
        },
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
            body: "The agent is idle while Ilium has a live progress monitor linked to this pane, so the turn is shown as parked instead of finished. When the monitor ends, Ilium reports its result in the footer; delivery to the agent depends on provider and pane state.",
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
            title: "Terminal activity",
            body: "This client queued input or observed changed visible terminal text within the last five seconds. WHY identifies the latest event.",
        },
        NowSignal::ShellOutput(ShellOutputPhase::Slow) => StatusExplanation {
            title: "Recent terminal activity",
            body: "The latest client input-queue or visible-text observation is five to sixty seconds old. The animation stops at sixty seconds.",
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

    let text_width = TOOLTIP_MAX_TEXT_WIDTH.min(screen.width.saturating_sub(4));
    if text_width < 8 {
        return;
    }
    let title = Line::from(Span::styled(
        explanation.title().to_string(),
        Style::new().add_modifier(Modifier::BOLD),
    ));
    let mut body_lines = crate::last_prompt_banner::wrap_lines(explanation.body(), text_width);
    let mut reason_lines = explanation
        .reason()
        .map(|reason| crate::last_prompt_banner::wrap_lines(reason, text_width));

    let content_capacity = screen.height.saturating_sub(2) as usize;
    let mut lines = Vec::new();
    if let Some(reason_lines) = &mut reason_lines {
        match content_capacity {
            0 => {}
            1 => {
                truncate_tooltip_lines(reason_lines, 1, text_width);
                lines.extend(
                    reason_lines.drain(..).map(|line| {
                        Line::from(Span::styled(line, Style::new().fg(Color::DarkGray)))
                    }),
                );
            }
            2 => {
                truncate_tooltip_lines(reason_lines, 1, text_width);
                lines.push(title);
                lines.extend(
                    reason_lines.drain(..).map(|line| {
                        Line::from(Span::styled(line, Style::new().fg(Color::DarkGray)))
                    }),
                );
            }
            _ => {
                truncate_tooltip_lines(reason_lines, content_capacity - 2, text_width);
                let body_capacity = content_capacity - 2 - reason_lines.len();
                truncate_tooltip_lines(&mut body_lines, body_capacity, text_width);
                lines.push(title);
                lines.extend(body_lines.drain(..).map(Line::from));
                lines.push(Line::from(""));
                lines.extend(
                    reason_lines.drain(..).map(|line| {
                        Line::from(Span::styled(line, Style::new().fg(Color::DarkGray)))
                    }),
                );
            }
        }
    } else if content_capacity > 0 {
        truncate_tooltip_lines(&mut body_lines, content_capacity - 1, text_width);
        lines.push(title);
        lines.extend(body_lines.into_iter().map(Line::from));
    }
    let content_width = lines
        .iter()
        .map(|line| line.width())
        .max()
        .unwrap_or(0)
        .min(usize::from(text_width)) as u16;
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

fn truncate_tooltip_lines(lines: &mut Vec<String>, max_lines: usize, max_width: u16) {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    if lines.len() <= max_lines {
        return;
    }
    lines.truncate(max_lines);
    let Some(last_line) = lines.last_mut() else {
        return;
    };
    let ellipsis_width = UnicodeWidthStr::width("…");
    let content_width = usize::from(max_width).saturating_sub(ellipsis_width);
    let mut prefix = String::new();
    let mut width = 0;
    for grapheme in last_line.graphemes(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if width + grapheme_width > content_width {
            break;
        }
        prefix.push_str(grapheme);
        width += grapheme_width;
    }
    *last_line = format!("{}…", prefix.trim_end());
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
        for goal in [
            GoalState::Active,
            GoalState::Paused,
            GoalState::Blocked,
            GoalState::UsageLimited,
            GoalState::Reached,
        ] {
            assert!(objective_explanation(ObjectiveSignal::Goal(goal)).is_some());
        }
        assert!(objective_explanation(ObjectiveSignal::ScheduledInput).is_some());
        assert!(objective_explanation(ObjectiveSignal::None).is_none());

        for signal in [
            NowSignal::AgentUnavailable(ilium_core::AgentAvailability::Unverified),
            NowSignal::NeedsApproval,
            NowSignal::Working,
            NowSignal::WaitingSubagents,
            NowSignal::Settling,
            NowSignal::Parked,
            NowSignal::FinishedUnread,
            NowSignal::Idle,
            NowSignal::ShellOutput(ShellOutputPhase::Fast),
            NowSignal::ShellOutput(ShellOutputPhase::Slow),
        ] {
            assert!(now_explanation(signal).is_some());
        }
        assert!(now_explanation(NowSignal::None).is_none());
    }

    #[test]
    fn unavailable_agent_has_distinct_recovery_glyph_and_explicit_uncertainty() {
        let icons = IconSettings::default();
        let signal = NowSignal::AgentUnavailable(ilium_core::AgentAvailability::Unverified);
        assert_eq!(now_span(signal, &icons, 0, false).content, "🛟");
        assert_eq!(now_span(signal, &icons, 0, true).content, "R");
        let explanation = now_explanation(signal).unwrap();
        assert!(explanation
            .body
            .contains("no crash or clean exit is inferred"));
        assert!(explanation
            .body
            .contains("did not establish a live agent composer"));
    }

    #[test]
    fn goal_and_parked_explanations_describe_observed_state_without_claiming_control() {
        let paused = objective_explanation(ObjectiveSignal::Goal(GoalState::Paused)).unwrap();
        assert!(paused.body.contains("provider surfaced"));
        assert!(paused.body.contains("does not control"));
        assert!(!paused.body.contains("/goal resume"));

        let usage_limited =
            objective_explanation(ObjectiveSignal::Goal(GoalState::UsageLimited)).unwrap();
        assert!(usage_limited.body.contains("provider reports"));
        assert!(usage_limited.body.contains("does not control"));

        let parked = now_explanation(NowSignal::Parked).unwrap();
        assert!(parked
            .body
            .contains("live progress monitor linked to this pane"));
        assert!(parked.body.contains("depends on provider and pane state"));
    }

    #[test]
    fn missing_identity_provenance_matches_each_pane_content_kind() {
        let terminal = missing_identity_evidence_reason(PaneContentKind::Terminal);
        assert!(terminal.contains("process-classification evidence"));

        for (content, pane_name) in [
            (PaneContentKind::Editor, "editor pane"),
            (PaneContentKind::Board, "board pane"),
        ] {
            let reason = missing_identity_evidence_reason(content);
            assert!(reason.contains(pane_name));
            assert!(reason.contains("rather than an agent process"));
            assert!(!reason.contains("classification evidence yet"));
        }
    }

    #[test]
    fn identity_provenance_fallback_matches_the_server_status_kind() {
        let agent = PaneStatus::from_activity(
            ilium_core::AgentClass::Codex,
            ilium_core::AgentActivity::Working,
            None,
        );
        let reason = identity_provenance_reason(&agent, None);
        assert!(reason.contains("Codex agent"));
        assert!(reason.contains("detector details were not included yet"));
        assert!(!reason.contains("process-classification evidence yet"));

        let editor = identity_provenance_reason(&PaneStatus::Editor { dirty: true }, None);
        assert!(editor.contains("dirty=true"));
        let board = identity_provenance_reason(&PaneStatus::Board, None);
        assert!(board.contains("board pane"));

        let detector = DetectionReason {
            rule: "process marker matched".to_string(),
            observed: Some("agent banner".to_string()),
            context: "current process tree".to_string(),
        };
        assert_eq!(
            identity_provenance_reason(&PaneStatus::Editor { dirty: true }, Some(&detector)),
            detection_reason_text(&detector)
        );
    }

    #[test]
    fn shell_activity_provenance_reports_the_latest_cause_and_suppresses_stale_phase() {
        use crate::terminal_activity::{TerminalActivityCause, TerminalActivityTracker};

        let pane_id = ilium_core::NodeId(43);
        let mut tracker = TerminalActivityTracker::default();
        tracker.record_with_cause(
            pane_id,
            100,
            TerminalActivityCause::KeyInputQueued { byte_count: 1 },
        );
        let snapshot = tracker.snapshot(pane_id, 100);

        let reason = shell_output_reason(ShellOutputPhase::Fast, snapshot)
            .expect("matching key-input activity");
        assert!(reason.contains("KeyInput"));
        assert!(reason.contains("bytes.len() = 1 > 0"));
        assert!(reason.contains("Server/PTY receipt and execution unconfirmed"));
        assert!(!reason.contains("visible-cell text fingerprint changed"));
        assert!(shell_output_reason(ShellOutputPhase::Slow, snapshot).is_none());
    }

    #[test]
    fn shell_activity_reason_sanitizes_and_bounds_observed_terminal_text() {
        use crate::terminal_activity::{
            TerminalActivityCause, TerminalActivityTracker, VisibleRowEvidence, VisibleTextEvidence,
        };

        let pane_id = ilium_core::NodeId(44);
        let mut tracker = TerminalActivityTracker::default();
        tracker.record_with_cause(
            pane_id,
            0,
            TerminalActivityCause::VisibleTextChanged(VisibleTextEvidence {
                first_sequence: 9,
                sequence: 10,
                changed_rows: Some(1),
                rows: vec![VisibleRowEvidence {
                    row_number: 2,
                    text: format!("unsafe\u{202e}{}", "x".repeat(600)),
                    blank: false,
                    truncated: true,
                }],
            }),
        );

        let reason = shell_output_reason(ShellOutputPhase::Fast, tracker.snapshot(pane_id, 0))
            .expect("matching visible-text observation");
        assert!(reason.contains("seq 9..10"));
        assert!(reason.contains("r2 at update"));
        assert!(reason.contains("[truncated]"));
        assert!(!reason.contains('\u{202e}'));
        assert!(reason.chars().count() <= 512);
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
    fn provenance_stays_visible_when_a_long_body_exceeds_the_screen_height() {
        use ratatui::backend::TestBackend;
        use ratatui::layout::Position;
        use ratatui::Terminal;

        let explanation = TooltipContent {
            title: "Task running".to_string(),
            body: "Long explanation detail.\n".repeat(12),
            reason: Some("Why: monitor #4 reported the selected task as running.".to_string()),
        };
        let mut terminal = Terminal::new(TestBackend::new(48, 8)).unwrap();
        terminal
            .draw(|frame| {
                render_tooltip(frame, frame.area(), Position::new(2, 3), &explanation);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..8)
            .map(|y| (0..48).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        assert!(rows.iter().any(|row| row.contains("Why: monitor #4")));
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
