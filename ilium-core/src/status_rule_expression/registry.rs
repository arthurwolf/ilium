//! The condition registry: one place that knows every condition's starter
//! value, category, label, one-line explanation and serialized key. The
//! editor list, the help text, the "why" explanation and the documentation
//! all read from here, so a new `Condition` variant is added once.

use super::{AvailabilityKind, Condition, PaneKind, ScreenPattern, ScreenRegion};
use crate::{AgentClass, AgentTurn, GoalState, ProgressTaskStatus};

/// Groups shown as headings in the condition picker and the documentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ConditionCategory {
    Pane,
    Agent,
    Goal,
    Monitor,
    Progress,
    Shell,
    Screen,
    /// Facts only the terminal client observes (frozen screen, spend).
    Client,
}

impl ConditionCategory {
    pub const ALL: [Self; 8] = [
        Self::Pane,
        Self::Agent,
        Self::Goal,
        Self::Monitor,
        Self::Progress,
        Self::Shell,
        Self::Screen,
        Self::Client,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Pane => "Pane",
            Self::Agent => "Agent",
            Self::Goal => "Goal",
            Self::Monitor => "Monitors",
            Self::Progress => "Progress",
            Self::Shell => "Terminal",
            Self::Screen => "Screen text",
            Self::Client => "Pane view",
        }
    }
}

const fn task_status_word(status: ProgressTaskStatus) -> &'static str {
    match status {
        ProgressTaskStatus::NotStartedYet => "pending",
        ProgressTaskStatus::Running => "running",
        ProgressTaskStatus::Error => "failed",
        ProgressTaskStatus::Done => "done",
    }
}

impl Condition {
    /// The serialized tag of the variant, as written in `config.toml`.
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::PaneKind(_) => "pane-kind",
            Self::EditorDirty => "editor-dirty",
            Self::PaneFrozen => "pane-frozen",
            Self::AgentClass(_) => "agent-class",
            Self::AgentAvailability(_) => "agent-availability",
            Self::AgentTurn(_) => "agent-turn",
            Self::GoalSet => "goal-set",
            Self::GoalState(_) => "goal-state",
            Self::CompletionUnread => "completion-unread",
            Self::ActivityStale => "activity-stale",
            Self::MonitorCountAtLeast(_) => "monitor-count-at-least",
            Self::TaskStatus(_) => "task-status",
            Self::AnyTaskStatus(_) => "any-task-status",
            Self::MonitorHealthy => "monitor-healthy",
            Self::MonitorDegraded => "monitor-degraded",
            Self::MonitorFailed => "monitor-failed",
            Self::AnyMonitorFailed => "any-monitor-failed",
            Self::MonitorUnread => "monitor-unread",
            Self::ProgressMeasurable => "progress-measurable",
            Self::ProgressAtLeast(_) => "progress-at-least",
            Self::ProgressBelow(_) => "progress-below",
            Self::ProgressBetween { .. } => "progress-between",
            Self::EtaKnown => "eta-known",
            Self::EtaBelowMinutes(_) => "eta-below-minutes",
            Self::Parked => "parked",
            Self::ScheduledInput => "scheduled-input",
            Self::ShellOutputRecent => "shell-output-recent",
            Self::ShellOutputFast => "shell-output-fast",
            Self::ShellOutputSlow => "shell-output-slow",
            Self::SessionCostAbove(_) => "session-cost-above",
            Self::ScreenMatches(_) => "screen-matches",
        }
    }

    pub const fn category(&self) -> ConditionCategory {
        match self {
            Self::Always | Self::PaneKind(_) | Self::EditorDirty | Self::ScheduledInput => {
                ConditionCategory::Pane
            }
            Self::AgentClass(_)
            | Self::AgentAvailability(_)
            | Self::AgentTurn(_)
            | Self::CompletionUnread
            | Self::ActivityStale
            | Self::Parked => ConditionCategory::Agent,
            Self::GoalSet | Self::GoalState(_) => ConditionCategory::Goal,
            Self::MonitorCountAtLeast(_)
            | Self::TaskStatus(_)
            | Self::AnyTaskStatus(_)
            | Self::MonitorHealthy
            | Self::MonitorDegraded
            | Self::MonitorFailed
            | Self::AnyMonitorFailed
            | Self::MonitorUnread => ConditionCategory::Monitor,
            Self::ProgressMeasurable
            | Self::ProgressAtLeast(_)
            | Self::ProgressBelow(_)
            | Self::ProgressBetween { .. }
            | Self::EtaKnown
            | Self::EtaBelowMinutes(_) => ConditionCategory::Progress,
            Self::ShellOutputRecent | Self::ShellOutputFast | Self::ShellOutputSlow => {
                ConditionCategory::Shell
            }
            Self::ScreenMatches(_) => ConditionCategory::Screen,
            Self::PaneFrozen | Self::SessionCostAbove(_) => ConditionCategory::Client,
        }
    }

    /// Short lower-case text used in the editor list, the selected rule and
    /// the "why" explanation (`NOT` is prefixed by the caller).
    pub fn label(&self) -> String {
        match self {
            Self::Always => "always".into(),
            Self::PaneKind(kind) => format!(
                "pane is {}",
                match kind {
                    PaneKind::Agent => "an agent",
                    PaneKind::PlainShell => "a plain terminal",
                    PaneKind::AgentUnavailable => "a lost agent",
                    PaneKind::Editor => "an editor",
                    PaneKind::Board => "a board",
                }
            ),
            Self::EditorDirty => "editor has unsaved changes".into(),
            Self::PaneFrozen => "pane is frozen".into(),
            Self::AgentClass(class) => format!(
                "agent is {}",
                match class {
                    AgentClass::Claude => "Claude Code",
                    AgentClass::Codex => "Codex",
                    AgentClass::Antigravity => "Antigravity",
                    AgentClass::Other(name) => name.as_str(),
                }
            ),
            Self::AgentAvailability(availability) => match availability {
                AvailabilityKind::Unverified => "lost agent is unverified",
                AvailabilityKind::ShellForeground => "lost agent left a shell in front",
                AvailabilityKind::Exited => "lost agent has exited",
                AvailabilityKind::ExitSuccess => "lost agent exited cleanly",
                AvailabilityKind::ExitFailure => "lost agent exited with an error",
            }
            .into(),
            Self::AgentTurn(turn) => match turn {
                AgentTurn::Working => "agent is working",
                AgentTurn::WaitingApproval => "agent needs approval",
                AgentTurn::WaitingSubagents => "agent is waiting for subagents",
                AgentTurn::Settling => "agent is settling",
                AgentTurn::Idle => "agent is idle",
            }
            .into(),
            Self::GoalSet => "goal is set".into(),
            Self::GoalState(goal) => format!(
                "goal is {}",
                match goal {
                    GoalState::Active => "active",
                    GoalState::Paused => "paused",
                    GoalState::Blocked => "blocked",
                    GoalState::UsageLimited => "usage limited",
                    GoalState::Reached => "reached",
                }
            ),
            Self::CompletionUnread => "completion is unread".into(),
            Self::ActivityStale => "activity check is stale".into(),
            Self::MonitorCountAtLeast(count) => format!("at least {count} monitors"),
            Self::TaskStatus(status) => {
                format!("selected monitored task is {}", task_status_word(*status))
            }
            Self::AnyTaskStatus(status) => {
                format!("any monitored task is {}", task_status_word(*status))
            }
            Self::MonitorHealthy => "monitor is healthy".into(),
            Self::MonitorDegraded => "monitor is degraded".into(),
            Self::MonitorFailed => "monitor has failed".into(),
            Self::AnyMonitorFailed => "any monitor has failed".into(),
            Self::MonitorUnread => "monitor result is unread".into(),
            Self::ProgressMeasurable => "progress is measurable".into(),
            Self::ProgressAtLeast(value) => format!("progress is at least {value}%"),
            Self::ProgressBelow(value) => format!("progress is below {value}%"),
            Self::ProgressBetween { minimum, maximum } => {
                format!("progress is {minimum}–{maximum}%")
            }
            Self::EtaKnown => "task has a time estimate".into(),
            Self::EtaBelowMinutes(minutes) => format!("task finishes in under {minutes} min"),
            Self::Parked => "agent is parked".into(),
            Self::ScheduledInput => "input is scheduled".into(),
            Self::ShellOutputRecent => "terminal has recent output".into(),
            Self::ShellOutputFast => "terminal output is fast".into(),
            Self::ShellOutputSlow => "terminal output is slow".into(),
            Self::SessionCostAbove(dollars) => format!("session cost is above ${dollars}"),
            Self::ScreenMatches(pattern) => format!(
                "visible screen matches /{}/{}",
                pattern.pattern,
                if pattern.ignore_case { "i" } else { "" }
            ),
        }
    }

    /// One sentence for the help rail and the documentation: what is
    /// observed, and when the condition cannot be observed at all.
    pub const fn explain(&self) -> &'static str {
        match self {
            Self::Always => "Matches every pane. Use it as the last rule to pick a fallback icon.",
            Self::PaneKind(_) => "The kind of pane: a live agent, a plain terminal, a lost agent, an editor or a board. Custom icons apply to agent and terminal panes.",
            Self::EditorDirty => "The editor pane has unsaved changes. Unknown for any other pane kind.",
            Self::PaneFrozen => "You froze the pane's screen. Known only for panes the client is currently displaying.",
            Self::AgentClass(_) => "Which agent CLI runs in the pane. Unknown unless a live agent is detected.",
            Self::AgentAvailability(_) => "Why a lost agent is lost: unverified, a shell took the foreground, or the process exited (cleanly or with an error). Unknown for live agents and terminals.",
            Self::AgentTurn(_) => "The agent's current turn phase: working, needing approval, waiting for subagents, settling or idle. Unknown without a live agent.",
            Self::GoalSet => "A /goal is attached to the agent, whatever its state. Unknown without a live agent.",
            Self::GoalState(_) => "The goal's state: active, paused, blocked, usage limited or reached. False when no goal is set; unknown without a live agent.",
            Self::CompletionUnread => "The agent finished a turn you have not looked at yet. Unknown without a live agent.",
            Self::ActivityStale => "The activity detector has not confirmed the agent's state recently. Unknown until the detector reports freshness.",
            Self::MonitorCountAtLeast(_) => "The pane has at least this many progress monitors; zero monitors is always known.",
            Self::TaskStatus(_) => "The selected (representative) monitored task is pending, running, failed or done. Unknown without a monitor.",
            Self::AnyTaskStatus(_) => "Any of the pane's monitored tasks has this status; false without monitors.",
            Self::MonitorHealthy => "The selected monitor's probe is answering normally. Unknown without a monitor.",
            Self::MonitorDegraded => "The selected monitor's probe is failing intermittently. Unknown without a monitor.",
            Self::MonitorFailed => "The selected monitor's probe has stopped answering. Unknown without a monitor.",
            Self::AnyMonitorFailed => "Any monitor's probe has stopped answering; false without monitors.",
            Self::MonitorUnread => "The selected monitor's latest outcome has not been acknowledged. Unknown without a monitor.",
            Self::ProgressMeasurable => "The selected task reports a percentage or a time estimate rather than an indeterminate spinner. Unknown without a monitor.",
            Self::ProgressAtLeast(_) => "The selected task's percentage is at or above the threshold. Unknown unless progress is measurable.",
            Self::ProgressBelow(_) => "The selected task's percentage is under the threshold. Unknown unless progress is measurable.",
            Self::ProgressBetween { .. } => "The selected task's percentage lies in the range, both ends included. Unknown unless progress is measurable.",
            Self::EtaKnown => "The selected task reports a remaining-time estimate. False with a monitor that gives none; unknown without a monitor.",
            Self::EtaBelowMinutes(_) => "The selected task's estimate is under this many minutes. Unknown without an estimate.",
            Self::Parked => "The agent is idle while a monitored task is still live: it is waiting for the task, not for you. Unknown without a live agent.",
            Self::ScheduledInput => "A prompt is queued to be sent to this pane later.",
            Self::ShellOutputRecent => "A plain terminal printed output recently. Unknown for agent panes.",
            Self::ShellOutputFast => "A plain terminal is printing quickly, like a build log. Unknown for agent panes.",
            Self::ShellOutputSlow => "A plain terminal is printing slowly, like a long test. Unknown for agent panes.",
            Self::SessionCostAbove(_) => "The agent session's tracked spend exceeds this many dollars. Unknown until cost tracking has a figure for the pane.",
            Self::ScreenMatches(_) => "A regular expression matches the pane's visible screen text, optionally only its first or last lines. Unknown until the screen has been scanned.",
        }
    }
}

/// A starter value for each supported condition; parameter editors can then
/// refine thresholds and patterns without changing the condition's identity.
/// Every `Condition` variant appears at least once (enforced by a test).
pub fn condition_catalog() -> Vec<Condition> {
    vec![
        Condition::Always,
        Condition::PaneKind(PaneKind::Agent),
        Condition::PaneKind(PaneKind::AgentUnavailable),
        Condition::PaneKind(PaneKind::PlainShell),
        Condition::PaneKind(PaneKind::Editor),
        Condition::EditorDirty,
        Condition::PaneKind(PaneKind::Board),
        Condition::ScheduledInput,
        Condition::AgentClass(AgentClass::Claude),
        Condition::AgentClass(AgentClass::Codex),
        Condition::AgentClass(AgentClass::Antigravity),
        Condition::AgentAvailability(AvailabilityKind::Unverified),
        Condition::AgentAvailability(AvailabilityKind::ShellForeground),
        Condition::AgentAvailability(AvailabilityKind::Exited),
        Condition::AgentAvailability(AvailabilityKind::ExitSuccess),
        Condition::AgentAvailability(AvailabilityKind::ExitFailure),
        Condition::AgentTurn(AgentTurn::Working),
        Condition::AgentTurn(AgentTurn::WaitingApproval),
        Condition::AgentTurn(AgentTurn::WaitingSubagents),
        Condition::AgentTurn(AgentTurn::Settling),
        Condition::AgentTurn(AgentTurn::Idle),
        Condition::CompletionUnread,
        Condition::ActivityStale,
        Condition::Parked,
        Condition::GoalSet,
        Condition::GoalState(GoalState::Active),
        Condition::GoalState(GoalState::Paused),
        Condition::GoalState(GoalState::Blocked),
        Condition::GoalState(GoalState::UsageLimited),
        Condition::GoalState(GoalState::Reached),
        Condition::MonitorCountAtLeast(1),
        Condition::TaskStatus(ProgressTaskStatus::NotStartedYet),
        Condition::TaskStatus(ProgressTaskStatus::Running),
        Condition::TaskStatus(ProgressTaskStatus::Error),
        Condition::TaskStatus(ProgressTaskStatus::Done),
        Condition::AnyTaskStatus(ProgressTaskStatus::NotStartedYet),
        Condition::AnyTaskStatus(ProgressTaskStatus::Running),
        Condition::AnyTaskStatus(ProgressTaskStatus::Error),
        Condition::AnyTaskStatus(ProgressTaskStatus::Done),
        Condition::MonitorHealthy,
        Condition::MonitorDegraded,
        Condition::MonitorFailed,
        Condition::AnyMonitorFailed,
        Condition::MonitorUnread,
        Condition::ProgressMeasurable,
        Condition::ProgressAtLeast(50),
        Condition::ProgressBelow(50),
        Condition::ProgressBetween {
            minimum: 25,
            maximum: 75,
        },
        Condition::EtaKnown,
        Condition::EtaBelowMinutes(5),
        Condition::ShellOutputRecent,
        Condition::ShellOutputFast,
        Condition::ShellOutputSlow,
        Condition::PaneFrozen,
        Condition::SessionCostAbove(5),
        // Screen matching stays last: the editor cycles through this list and
        // wraps from the screen pattern back to `Always`.
        Condition::ScreenMatches(ScreenPattern {
            pattern: "error".to_owned(),
            ignore_case: true,
            region: ScreenRegion::All,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde::{Deserialize, Serialize};

    use super::{condition_catalog, ConditionCategory};
    use crate::status_rule_expression::{Condition, ConditionTerm};

    #[derive(Serialize, Deserialize)]
    struct Document {
        terms: Vec<ConditionTerm<Condition>>,
    }

    #[test]
    fn every_catalogue_condition_round_trips_through_toml() {
        let terms = condition_catalog()
            .into_iter()
            .enumerate()
            .map(|(index, condition)| ConditionTerm {
                condition,
                negated: index % 2 == 1,
            })
            .collect::<Vec<_>>();
        let document = Document {
            terms: terms.clone(),
        };
        let text = toml::to_string(&document).expect("serializable");
        let parsed: Document = toml::from_str(&text).expect("parseable");
        assert_eq!(parsed.terms, terms);
    }

    #[test]
    fn registry_keys_match_the_serialized_tags() {
        for condition in condition_catalog() {
            let text = toml::to_string(&Document {
                terms: vec![ConditionTerm {
                    condition: condition.clone(),
                    negated: false,
                }],
            })
            .expect("serializable");
            assert!(
                text.contains(condition.key()),
                "{:?} serializes as {text:?}, which does not contain its key {:?}",
                condition,
                condition.key()
            );
        }
    }

    #[test]
    fn catalogue_covers_every_variant_without_duplicates_and_has_text_for_each() {
        let catalogue = condition_catalog();
        let mut seen: Vec<&Condition> = Vec::new();
        for condition in &catalogue {
            assert!(
                !seen.contains(&condition),
                "duplicate catalogue entry {condition:?}"
            );
            seen.push(condition);
            assert!(!condition.label().is_empty());
            assert!(
                condition.explain().ends_with('.'),
                "{condition:?} explanation must be a sentence"
            );
            assert!(ConditionCategory::ALL.contains(&condition.category()));
        }
        let keys: HashSet<&str> = catalogue.iter().map(Condition::key).collect();
        assert_eq!(
            keys.len(),
            32,
            "every Condition variant has a catalogue entry: {keys:?}"
        );
        for category in ConditionCategory::ALL {
            assert!(
                catalogue
                    .iter()
                    .any(|condition| condition.category() == category),
                "{category:?} is empty"
            );
            assert!(!category.label().is_empty());
        }
    }

    #[test]
    fn labels_mention_their_parameters() {
        assert_eq!(
            Condition::ProgressBetween {
                minimum: 25,
                maximum: 75
            }
            .label(),
            "progress is 25–75%"
        );
        assert_eq!(
            Condition::EtaBelowMinutes(5).label(),
            "task finishes in under 5 min"
        );
        assert_eq!(
            Condition::SessionCostAbove(5).label(),
            "session cost is above $5"
        );
        assert_eq!(
            Condition::MonitorCountAtLeast(2).label(),
            "at least 2 monitors"
        );
    }
}
