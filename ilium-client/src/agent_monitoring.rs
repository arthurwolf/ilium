//! Client presentation policy and settings for monitoring detected agents.

use ilium_core::{
    project_pane_signals, AgentTurn, GoalState, NowSignal, ObjectiveSignal, PaneProgress,
    PaneSignals, PaneStatus, ProgressTaskStatus, ShellOutputPhase, TaskSignal,
};

use crate::icon_settings::IconTarget;

/// Controls which of the two existing pane-status slots are visible in the
/// tree. This changes presentation only; detector and acknowledgement state
/// remain shared across clients.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AgentMonitoringMode {
    #[default]
    Normal,
    Attention,
}

pub const STATUS_ICON_TARGETS: [IconTarget; 17] = [
    IconTarget::Working,
    IconTarget::WaitingBackground,
    IconTarget::BackgroundTaskStillRunning,
    IconTarget::WaitingApproval,
    IconTarget::Done,
    IconTarget::Idle,
    IconTarget::GoalActive,
    IconTarget::GoalPaused,
    IconTarget::GoalBlocked,
    IconTarget::GoalUsageLimited,
    IconTarget::GoalReached,
    IconTarget::Parked,
    IconTarget::TaskPending,
    IconTarget::TaskDone,
    IconTarget::TaskError,
    IconTarget::MonitorFailed,
    IconTarget::ScheduledInput,
];

pub fn general_icon_targets() -> Vec<IconTarget> {
    IconTarget::ALL
        .into_iter()
        .filter(|target| !STATUS_ICON_TARGETS.contains(target))
        .collect()
}

impl AgentMonitoringMode {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Attention => "attention",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Attention => "Attention",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "attention" => Some(Self::Attention),
            _ => None,
        }
    }
}

/// Returns the one highest-priority status glyph for Attention mode. Identity
/// is still rendered separately. A terminal error remains actionable after
/// acknowledgement; successful task and turn completion do not.
pub fn attention_status_target(
    status: &PaneStatus,
    progress: Option<&PaneProgress>,
) -> Option<IconTarget> {
    let agent = status.agent_state();
    if agent.is_some_and(|state| state.turn == AgentTurn::WaitingApproval) {
        return Some(IconTarget::WaitingApproval);
    }

    if let Some(progress) = progress {
        if !progress.report.status.is_terminal() && progress.monitor_health.is_failed() {
            return Some(IconTarget::MonitorFailed);
        }
        if progress.report.status == ProgressTaskStatus::Error {
            return Some(IconTarget::TaskError);
        }
    }

    if let Some(goal) = agent.and_then(|state| state.goal) {
        let target = match goal {
            GoalState::Active => None,
            GoalState::Paused => Some(IconTarget::GoalPaused),
            GoalState::Blocked => Some(IconTarget::GoalBlocked),
            GoalState::UsageLimited => Some(IconTarget::GoalUsageLimited),
            GoalState::Reached => Some(IconTarget::GoalReached),
        };
        if target.is_some() {
            return target;
        }
    }

    if progress.is_some_and(|progress| {
        progress.report.status == ProgressTaskStatus::Done
            && progress.attention == ilium_core::ProgressAttention::Unread
    }) {
        return Some(IconTarget::TaskDone);
    }

    agent
        .filter(|state| state.completion_unread)
        .map(|_| IconTarget::Done)
}

/// Splits the chosen target into the existing objective/current-activity
/// columns so their established glyph, width, and hover explanation remain
/// accurate while Attention mode renders at most one status icon.
pub fn attention_status_signals(
    status: &PaneStatus,
    progress: Option<&PaneProgress>,
) -> (ObjectiveSignal, NowSignal) {
    attention_signals_for_target(status, progress, attention_status_target(status, progress))
}

/// Selects the exact pair shown in the tree so rendering and hover provenance
/// use one mode-specific decision, including the Attention priority rule.
pub fn displayed_pane_signals(
    mode: AgentMonitoringMode,
    status: &PaneStatus,
    progress: Option<&PaneProgress>,
    has_scheduled_input: bool,
    shell_output: Option<ShellOutputPhase>,
) -> PaneSignals {
    if mode == AgentMonitoringMode::Normal {
        return project_pane_signals(status, progress, has_scheduled_input, shell_output);
    }

    let target = attention_status_target(status, progress);
    let (objective, now) = attention_signals_for_target(status, progress, target);
    let selection_rule = attention_selection_rule(target);
    PaneSignals {
        objective,
        now,
        objective_rule: if objective == ObjectiveSignal::None {
            "A9"
        } else {
            selection_rule
        },
        now_rule: if now == NowSignal::None {
            "A9"
        } else {
            selection_rule
        },
    }
}

fn attention_signals_for_target(
    status: &PaneStatus,
    progress: Option<&PaneProgress>,
    target: Option<IconTarget>,
) -> (ObjectiveSignal, NowSignal) {
    let Some(target) = target else {
        return (ObjectiveSignal::None, NowSignal::None);
    };
    match target {
        IconTarget::GoalActive
        | IconTarget::GoalPaused
        | IconTarget::GoalBlocked
        | IconTarget::GoalUsageLimited
        | IconTarget::GoalReached => (
            status
                .agent_state()
                .and_then(|state| state.goal)
                .map_or(ObjectiveSignal::None, ObjectiveSignal::Goal),
            NowSignal::None,
        ),
        IconTarget::MonitorFailed | IconTarget::TaskError | IconTarget::TaskDone => (
            progress.map_or(ObjectiveSignal::None, |progress| {
                ObjectiveSignal::Task(TaskSignal::from_progress(progress))
            }),
            NowSignal::None,
        ),
        IconTarget::WaitingApproval => (ObjectiveSignal::None, NowSignal::NeedsApproval),
        IconTarget::Done => (ObjectiveSignal::None, NowSignal::FinishedUnread),
        _ => (ObjectiveSignal::None, NowSignal::None),
    }
}

fn attention_selection_rule(target: Option<IconTarget>) -> &'static str {
    match target {
        Some(IconTarget::WaitingApproval) => {
            "Attention priority 1: approval is checked before monitor, task, goal, and unread-turn statuses"
        }
        Some(IconTarget::MonitorFailed) => {
            "Attention priority 2: a failed live monitor is checked after approval and before task errors or goals"
        }
        Some(IconTarget::TaskError) => {
            "Attention priority 3: a task error is checked after approval and monitor health, before goals"
        }
        Some(
            IconTarget::GoalPaused
            | IconTarget::GoalBlocked
            | IconTarget::GoalUsageLimited
            | IconTarget::GoalReached,
        ) => {
            "Attention priority 4: a non-active goal is checked after approval and task errors"
        }
        Some(IconTarget::TaskDone) => {
            "Attention priority 5: an unread successful monitor report is checked after approval, failures, and non-active goals"
        }
        Some(IconTarget::Done) => {
            "Attention priority 6: an unread completed turn is checked after approval, monitor/task failures, non-active goals, and unread monitor success"
        }
        _ => "No Attention status was selected",
    }
}

#[cfg(test)]
mod tests {
    use ilium_core::{
        AgentActivity, AgentClass, GoalState, PaneProgress, PaneStatus, ProgressAttention,
        ProgressMonitorHealth, ProgressTaskReport, ProgressTaskStatus,
    };

    use crate::icon_settings::IconTarget;

    use super::attention_status_target;

    fn agent(activity: AgentActivity, goal: Option<GoalState>) -> PaneStatus {
        PaneStatus::from_activity(AgentClass::Codex, activity, goal)
    }

    fn progress(status: ProgressTaskStatus, unread: bool) -> PaneProgress {
        let percent = if status == ProgressTaskStatus::Done {
            100.0
        } else {
            40.0
        };
        let report = ProgressTaskReport::new(
            "agent-monitoring-test".to_string(),
            status,
            percent,
            "test report".to_string(),
            (status == ProgressTaskStatus::Error).then(|| "test failure".to_string()),
        )
        .unwrap();
        let mut progress = PaneProgress::new(1, report, 0).unwrap();
        if !unread {
            progress.attention = ProgressAttention::Acknowledged;
        }
        progress
    }

    #[test]
    fn attention_prioritizes_approval_over_monitor_and_goal_errors() {
        let mut monitor = progress(ProgressTaskStatus::Running, true);
        monitor.monitor_health = ProgressMonitorHealth::Failed {
            consecutive_failures: 3,
            last_error: "probe failed".to_string(),
        };
        assert_eq!(
            attention_status_target(
                &agent(AgentActivity::WaitingApproval, Some(GoalState::Blocked)),
                Some(&monitor),
            ),
            Some(IconTarget::WaitingApproval)
        );
    }

    #[test]
    fn monitor_failure_precedes_task_failure_and_goal_attention() {
        let mut monitor = progress(ProgressTaskStatus::Running, true);
        monitor.monitor_health = ProgressMonitorHealth::Failed {
            consecutive_failures: 3,
            last_error: "probe failed".to_string(),
        };
        assert_eq!(
            attention_status_target(
                &agent(AgentActivity::Idle, Some(GoalState::Blocked)),
                Some(&monitor),
            ),
            Some(IconTarget::MonitorFailed)
        );
    }

    #[test]
    fn task_failure_precedes_blocked_goal() {
        assert_eq!(
            attention_status_target(
                &agent(AgentActivity::Idle, Some(GoalState::Blocked)),
                Some(&progress(ProgressTaskStatus::Error, false)),
            ),
            Some(IconTarget::TaskError)
        );
    }

    #[test]
    fn goal_attention_is_ordered_and_unread_success_is_retained() {
        for (goal, expected) in [
            (GoalState::Blocked, IconTarget::GoalBlocked),
            (GoalState::UsageLimited, IconTarget::GoalUsageLimited),
            (GoalState::Paused, IconTarget::GoalPaused),
            (GoalState::Reached, IconTarget::GoalReached),
        ] {
            assert_eq!(
                attention_status_target(&agent(AgentActivity::Idle, Some(goal)), None),
                Some(expected)
            );
        }
        assert_eq!(
            attention_status_target(
                &agent(AgentActivity::Idle, None),
                Some(&progress(ProgressTaskStatus::Done, true)),
            ),
            Some(IconTarget::TaskDone)
        );
        assert_eq!(
            attention_status_target(&agent(AgentActivity::Done, None), None),
            Some(IconTarget::Done)
        );
    }

    #[test]
    fn acknowledged_success_and_healthy_activity_need_no_attention() {
        assert_eq!(
            attention_status_target(
                &agent(AgentActivity::Working, Some(GoalState::Active)),
                Some(&progress(ProgressTaskStatus::Running, true)),
            ),
            None
        );
        assert_eq!(
            attention_status_target(
                &agent(AgentActivity::Idle, None),
                Some(&progress(ProgressTaskStatus::Done, false)),
            ),
            None
        );
        assert_eq!(
            attention_status_target(&agent(AgentActivity::Idle, None), None),
            None
        );
    }

    #[test]
    fn degraded_probe_is_suppressed_until_monitoring_fails() {
        let mut monitor = progress(ProgressTaskStatus::Running, true);
        monitor.monitor_health = ProgressMonitorHealth::Degraded {
            consecutive_failures: 1,
            last_error: "retrying".to_string(),
        };
        assert_eq!(
            attention_status_target(&agent(AgentActivity::Idle, None), Some(&monitor)),
            None
        );
        monitor.monitor_health = ProgressMonitorHealth::Failed {
            consecutive_failures: 3,
            last_error: "stopped retrying".to_string(),
        };
        assert_eq!(
            attention_status_target(&agent(AgentActivity::Idle, None), Some(&monitor)),
            Some(IconTarget::MonitorFailed)
        );
    }
}
