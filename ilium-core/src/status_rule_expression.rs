//! Boolean expressions used by custom status-icon rules.
//!
//! An expression is a disjunction of conjunctions: each inner group is
//! joined with AND, and groups are joined with OR. Missing observations do
//! not become true merely because a condition is negated.

use serde::{Deserialize, Serialize};

use crate::{
    AgentAvailability, AgentClass, AgentExitOutcome, AgentTurn, GoalState, PaneProgress, PaneStatus, ProgressAttention,
    ProgressMonitorHealth, ProgressTaskStatus, ShellOutputPhase, has_live_progress,
    representative_progress,
};

/// The result of observing a single condition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Observation {
    True,
    False,
    Unknown,
}

/// Facts available independently of the rules that consume them. Screen
/// matches are supplied as observations by the owner of the terminal screen.
pub struct PaneFacts<'a> {
    pub status: &'a PaneStatus,
    pub progress: &'a [PaneProgress],
    pub scheduled_input: bool,
    pub shell_output: Option<ShellOutputPhase>,
    /// None means the activity freshness has not been observed.
    pub activity_stale: Option<bool>,
    pub screen_matches: &'a dyn Fn(&ScreenPattern) -> Observation,
}

/// An expression applies only to the currently visible terminal grid.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScreenPattern {
    pub pattern: String,
    pub ignore_case: bool,
    pub region: ScreenRegion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScreenRegion {
    All,
    FirstLines(u16),
    LastLines(u16),
}

/// Conditions that can be evaluated from the existing pane and progress facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Condition {
    Always,
    PaneKind(PaneKind),
    EditorDirty,
    AgentClass(AgentClass),
    AgentAvailability(AvailabilityKind),
    AgentTurn(AgentTurn),
    GoalSet,
    GoalState(GoalState),
    CompletionUnread,
    ActivityStale,
    MonitorCountAtLeast(u32),
    TaskStatus(ProgressTaskStatus),
    AnyTaskStatus(ProgressTaskStatus),
    MonitorHealthy,
    MonitorDegraded,
    MonitorFailed,
    AnyMonitorFailed,
    MonitorUnread,
    ProgressMeasurable,
    ProgressAtLeast(u8),
    ProgressBelow(u8),
    ProgressBetween { minimum: u8, maximum: u8 },
    Parked,
    ScheduledInput,
    ShellOutputRecent,
    ShellOutputFast,
    ShellOutputSlow,
    ScreenMatches(ScreenPattern),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaneKind {
    Agent,
    PlainShell,
    AgentUnavailable,
    Editor,
    Board,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AvailabilityKind {
    Unverified,
    ShellForeground,
    Exited,
    ExitSuccess,
    ExitFailure,
}

impl Condition {
    /// Evaluate a condition without treating unavailable facts as false.
    pub fn observe(&self, facts: &PaneFacts<'_>) -> Observation {
        let agent = facts.status.agent_state();
        let monitor = representative_progress(facts.progress);
        let result = match self {
            Self::Always => Some(true),
            Self::PaneKind(kind) => Some(matches!(
                (kind, facts.status),
                (PaneKind::Agent, PaneStatus::Agent(_))
                    | (PaneKind::PlainShell, PaneStatus::PlainShell)
                    | (PaneKind::AgentUnavailable, PaneStatus::AgentUnavailable(_))
                    | (PaneKind::Editor, PaneStatus::Editor { .. })
                    | (PaneKind::Board, PaneStatus::Board)
            )),
            Self::EditorDirty => match facts.status {
                PaneStatus::Editor { dirty } => Some(*dirty),
                _ => None,
            },
            Self::AgentClass(class) => agent.map(|agent| &agent.class == class),
            Self::AgentAvailability(expected) => facts.status.agent_recovery().map(|recovery| {
                match expected {
                    AvailabilityKind::Unverified => recovery.availability == AgentAvailability::Unverified,
                    AvailabilityKind::ShellForeground => recovery.availability == AgentAvailability::ShellForeground,
                    AvailabilityKind::Exited => matches!(recovery.availability, AgentAvailability::Exited(_)),
                    AvailabilityKind::ExitSuccess => matches!(recovery.availability, AgentAvailability::Exited(AgentExitOutcome::ExitCode(0))),
                    AvailabilityKind::ExitFailure => matches!(recovery.availability, AgentAvailability::Exited(AgentExitOutcome::ExitCode(code)) if code != 0),
                }
            }),
            Self::AgentTurn(turn) => agent.map(|agent| &agent.turn == turn),
            Self::GoalSet => agent.map(|agent| agent.goal.is_some()),
            Self::GoalState(goal) => agent.map(|agent| agent.goal.as_ref() == Some(goal)),
            Self::CompletionUnread => agent.map(|agent| agent.completion_unread),
            Self::ActivityStale => facts.activity_stale,
            Self::MonitorCountAtLeast(count) => Some(facts.progress.len() >= *count as usize),
            Self::TaskStatus(status) => monitor.map(|monitor| &monitor.report.status == status),
            Self::AnyTaskStatus(status) => Some(facts.progress.iter().any(|monitor| &monitor.report.status == status)),
            Self::MonitorHealthy => monitor.map(|monitor| matches!(monitor.monitor_health, ProgressMonitorHealth::Healthy)),
            Self::MonitorDegraded => monitor.map(|monitor| matches!(monitor.monitor_health, ProgressMonitorHealth::Degraded { .. })),
            Self::MonitorFailed => monitor.map(|monitor| monitor.monitor_health.is_failed()),
            Self::AnyMonitorFailed => Some(facts.progress.iter().any(|monitor| monitor.monitor_health.is_failed())),
            Self::MonitorUnread => monitor.map(|monitor| monitor.attention == ProgressAttention::Unread),
            Self::ProgressMeasurable => monitor.map(PaneProgress::has_measurable_progress),
            Self::ProgressAtLeast(percent) => monitor.filter(|monitor| monitor.has_measurable_progress()).map(|monitor| monitor.report.percent >= f32::from(*percent)),
            Self::ProgressBelow(percent) => monitor.filter(|monitor| monitor.has_measurable_progress()).map(|monitor| monitor.report.percent < f32::from(*percent)),
            Self::ProgressBetween { minimum, maximum } => monitor.filter(|monitor| monitor.has_measurable_progress()).map(|monitor| (f32::from(*minimum)..=f32::from(*maximum)).contains(&monitor.report.percent)),
            Self::Parked => agent.map(|agent| agent.turn == AgentTurn::Idle && has_live_progress(facts.progress)),
            Self::ScheduledInput => Some(facts.scheduled_input),
            Self::ShellOutputRecent => match facts.status {
                PaneStatus::PlainShell => Some(facts.shell_output.is_some()),
                _ => None,
            },
            Self::ShellOutputFast => match facts.status {
                PaneStatus::PlainShell => Some(facts.shell_output == Some(ShellOutputPhase::Fast)),
                _ => None,
            },
            Self::ShellOutputSlow => match facts.status {
                PaneStatus::PlainShell => Some(facts.shell_output == Some(ShellOutputPhase::Slow)),
                _ => None,
            },
            Self::ScreenMatches(pattern) => return (facts.screen_matches)(pattern),
        };
        match result {
            Some(true) => Observation::True,
            Some(false) => Observation::False,
            None => Observation::Unknown,
        }
    }
}

/// One condition and its optional NOT operator.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConditionTerm<C> {
    pub condition: C,
    pub negated: bool,
}

/// A group of conditions joined by AND.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AllOf<C> {
    pub all_of: Vec<ConditionTerm<C>>,
}

/// A visible connector between adjacent terms in the one-line rule editor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Connector {
    And,
    Or,
}

/// Convert the editor's flat line into AND groups. A malformed line is
/// rejected instead of silently dropping a condition or changing its meaning.
pub fn from_line<C>(
    terms: Vec<ConditionTerm<C>>,
    connectors: &[Connector],
) -> Result<Vec<AllOf<C>>, &'static str> {
    if connectors.len() != terms.len().saturating_sub(1) {
        return Err("a connector is required between each pair of conditions");
    }
    let mut terms = terms.into_iter();
    let Some(first) = terms.next() else {
        return Ok(Vec::new());
    };
    let mut groups = vec![AllOf { all_of: vec![first] }];
    for (connector, term) in connectors.iter().zip(terms) {
        match connector {
            Connector::And => {
                // Invariant: the first term inserted the first group.
                if let Some(group) = groups.last_mut() {
                    group.all_of.push(term);
                }
            }
            Connector::Or => groups.push(AllOf { all_of: vec![term] }),
        }
    }
    Ok(groups)
}

/// Expand persisted groups back into a flat editor line.
pub fn to_line<C: Clone>(groups: &[AllOf<C>]) -> Result<(Vec<ConditionTerm<C>>, Vec<Connector>), &'static str> {
    let mut terms = Vec::new();
    let mut connectors = Vec::new();
    for group in groups {
        if group.all_of.is_empty() {
            return Err("an AND group cannot be empty");
        }
        for (index, term) in group.all_of.iter().enumerate() {
            if !terms.is_empty() {
                connectors.push(if index == 0 { Connector::Or } else { Connector::And });
            }
            terms.push(term.clone());
        }
    }
    Ok((terms, connectors))
}

/// A named, ordered rule. Disabled rules are retained but never match.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Rule<C, V> {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub any_of: Vec<AllOf<C>>,
    pub then: V,
}

/// Presentation requested by a matching rule. Rendering and terminal-width
/// validation live in the client, which owns the icon and theme settings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StatusVisual {
    pub kind: VisualKind,
    pub style: VisualStyle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisualKind {
    Hidden,
    /// Stable key of an existing configurable icon target.
    BuiltIn(String),
    Glyph(String),
    BrailleAnimation { preset: String, frames_per_second: u8 },
    FrameAnimation {
        frames: Vec<String>,
        frames_per_second: u8,
        ping_pong: bool,
    },
    ProgressFrames { frames: Vec<String> },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct VisualStyle {
    /// Theme colour name or an explicit RGB colour.
    pub color: Option<String>,
    pub bold: bool,
    pub dim: bool,
}

/// User-owned rules are ordered and persisted together with their fallback.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StatusRules {
    pub defaults_version: u32,
    pub rules: Vec<Rule<Condition, StatusVisual>>,
    pub fallback: Option<StatusVisual>,
}

/// Validate structural limits before saving a ruleset. Regex syntax and cell
/// widths are validated by their respective server and terminal adapters.
pub fn validate(rules: &StatusRules) -> Vec<String> {
    let mut errors = Vec::new();
    let mut ids = std::collections::HashSet::new();
    let mut enabled_watches: Vec<&ScreenPattern> = Vec::new();
    if rules.rules.len() > 64 {
        errors.push("A ruleset may contain at most 64 rules".to_owned());
    }
    for rule in &rules.rules {
        if rule.id.is_empty() || !ids.insert(&rule.id) {
            errors.push(format!("Rule {:?} has an empty or duplicate identifier", rule.name));
        }
        if rule.name.chars().count() > 48 {
            errors.push(format!("Rule {:?} has a name longer than 48 characters", rule.id));
        }
        if rule.any_of.iter().any(|group| group.all_of.is_empty()) {
            errors.push(format!("Rule {:?} contains an empty AND group", rule.name));
        }
        if rule.any_of.len() > 8 || rule.any_of.iter().any(|group| group.all_of.len() > 8) {
            errors.push(format!("Rule {:?} exceeds the eight-group or eight-condition limit", rule.name));
        }
        if rule.any_of.iter().map(|group| group.all_of.len()).sum::<usize>() > 64 {
            errors.push(format!("Rule {:?} has more than 64 conditions", rule.name));
        }
        for term in rule.any_of.iter().flat_map(|group| &group.all_of) {
            if let Condition::ScreenMatches(pattern) = &term.condition {
                if rule.enabled && !enabled_watches.contains(&pattern) {
                    enabled_watches.push(pattern);
                }
            }
            match &term.condition {
                Condition::ProgressAtLeast(value) | Condition::ProgressBelow(value) if *value > 100 => {
                    errors.push(format!("Rule {:?} has an invalid progress percentage", rule.name));
                }
                Condition::ProgressBetween { minimum, maximum } if minimum > maximum || *maximum > 100 => {
                    errors.push(format!("Rule {:?} has an invalid progress range", rule.name));
                }
                Condition::ScreenMatches(pattern) if pattern.pattern.chars().count() > 256 => {
                    errors.push(format!("Rule {:?} has a screen pattern longer than 256 characters", rule.name));
                }
                _ => {}
            }
        }
    }
    if enabled_watches.len() > 16 {
        errors.push("A ruleset may enable at most 16 distinct screen patterns".to_owned());
    }
    errors
}

/// Fresh starting priorities for users who have not saved their own rules.
pub fn default_rules() -> StatusRules {
    use AgentTurn::{Idle, Settling, WaitingApproval, WaitingSubagents, Working};
    use Condition::{AgentTurn as Turn, GoalState as Goal};
    use GoalState::{Active, Blocked, Paused, Reached, UsageLimited};

    fn rule(id: &str, name: &str, icon: &str, color: &str, terms: Vec<ConditionTerm<Condition>>) -> Rule<Condition, StatusVisual> {
        Rule {
            id: id.to_owned(),
            name: name.to_owned(),
            enabled: true,
            any_of: vec![AllOf { all_of: terms }],
            then: StatusVisual {
                kind: VisualKind::BuiltIn(icon.to_owned()),
                style: VisualStyle { color: Some(color.to_owned()), ..VisualStyle::default() },
            },
        }
    }
    let yes = |condition| ConditionTerm { condition, negated: false };
    let not = |condition| ConditionTerm { condition, negated: true };
    let mut rules = vec![
        rule("approval", "Needs your approval", "waiting_approval", "warning", vec![yes(Turn(WaitingApproval))]),
        rule("agent-lost", "Agent lost", "agent_unavailable", "danger", vec![yes(Condition::PaneKind(PaneKind::AgentUnavailable))]),
        rule("task-failed", "Task failed", "task_error", "danger", vec![yes(Condition::TaskStatus(ProgressTaskStatus::Error))]),
        rule("monitor-lost", "Monitor lost", "monitor_failed", "danger", vec![yes(Condition::MonitorFailed), not(Condition::TaskStatus(ProgressTaskStatus::Done)), not(Condition::TaskStatus(ProgressTaskStatus::Error))]),
        rule("goal-blocked", "Goal blocked", "goal_blocked", "warning", vec![yes(Goal(Blocked))]),
        rule("usage-limited", "Usage limit reached", "goal_usage_limited", "warning", vec![yes(Goal(UsageLimited))]),
        rule("goal-paused", "Goal paused", "goal_paused", "info", vec![yes(Goal(Paused)), not(Turn(Working))]),
        rule("task-unread", "Task finished, result unread", "task_done", "success", vec![yes(Condition::TaskStatus(ProgressTaskStatus::Done)), yes(Condition::MonitorUnread)]),
        rule("goal-reached", "Goal reached", "goal_reached", "success", vec![yes(Goal(Reached))]),
        rule("completion-unread", "Finished, not seen yet", "done", "accent", vec![yes(Condition::CompletionUnread), not(Condition::Parked)]),
        // Disabled until the user enables it. Keep the example above broad
        // activity and idle rules so enabling it can actually affect agents.
        rule("screen-error-example", "Screen mentions an error", "task_error", "danger", vec![yes(Condition::ScreenMatches(ScreenPattern { pattern: "\\b(error|failed|panic)\\b".to_owned(), ignore_case: true, region: ScreenRegion::LastLines(20) }))]),
        rule("parked-progress", "Parked, task progressing", "parked", "info", vec![yes(Condition::Parked), yes(Condition::ProgressMeasurable)]),
        rule("parked", "Parked, waiting for task", "parked", "info", vec![yes(Condition::Parked)]),
        rule("subagents", "Waiting for background agents", "waiting_background", "info", vec![yes(Turn(WaitingSubagents))]),
        rule("settling", "Background work still running", "background_task_still_running", "info", vec![yes(Turn(Settling))]),
        rule("working-goal", "Working toward a goal", "working", "accent", vec![yes(Turn(Working)), yes(Goal(Active))]),
        rule("working", "Working", "working", "success", vec![yes(Turn(Working))]),
        rule("pending", "Task pending", "task_pending", "muted", vec![yes(Condition::TaskStatus(ProgressTaskStatus::NotStartedYet))]),
        rule("scheduled", "Next prompt scheduled", "scheduled_input", "muted", vec![yes(Condition::ScheduledInput)]),
        rule("goal-idle", "Goal active, agent idle", "goal_active", "muted", vec![yes(Goal(Active)), yes(Turn(Idle))]),
        rule("shell-busy", "Terminal busy", "working", "muted", vec![yes(Condition::PaneKind(PaneKind::PlainShell)), yes(Condition::ShellOutputRecent)]),
        rule("idle", "Idle", "idle", "muted", vec![yes(Turn(Idle))]),
    ];
    if let Some(example) = rules.iter_mut().find(|rule| rule.id == "screen-error-example") {
        example.enabled = false;
    }
    if let Some(rule) = rules.iter_mut().find(|rule| rule.id == "parked-progress") {
        rule.then.kind = VisualKind::ProgressFrames {
            frames: ["🌑", "🌒", "🌓", "🌔", "🌕"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        };
    }
    if let Some(rule) = rules.iter_mut().find(|rule| rule.id == "working-goal") {
        rule.then.kind = VisualKind::BrailleAnimation {
            preset: "spinner".to_owned(),
            frames_per_second: 8,
        };
    }
    if let Some(rule) = rules.iter_mut().find(|rule| rule.id == "shell-busy") {
        rule.then.kind = VisualKind::BrailleAnimation {
            preset: "loop".to_owned(),
            frames_per_second: 8,
        };
    }
    StatusRules { defaults_version: 1, rules, fallback: None }
}

/// Choose the first enabled matching rule, preserving the user's priority order.
pub fn first_match<C, V>(
    rules: &[Rule<C, V>],
    mut observe: impl FnMut(&C) -> Observation,
) -> Option<&Rule<C, V>> {
    rules
        .iter()
        .find(|rule| rule.enabled && matches(&rule.any_of, &mut observe))
}

/// Report a provably shadowed rule only when an earlier enabled rule contains
/// a conjunction that is included in every conjunction of the later rule.
/// This conservative test never guesses about relationships between facts.
pub fn shadowed_by<C: PartialEq, V>(rules: &[Rule<C, V>], index: usize) -> Option<usize> {
    let rule = rules.get(index)?;
    if !rule.enabled || rule.any_of.is_empty() || rule.any_of.iter().any(|group| group.all_of.is_empty()) {
        return None;
    }
    rules[..index].iter().position(|earlier| {
        earlier.enabled && earlier.any_of.iter().any(|earlier_group| {
            !earlier_group.all_of.is_empty() && rule.any_of.iter().all(|later_group| {
                earlier_group.all_of.iter().all(|term| later_group.all_of.contains(term))
            })
        })
    })
}

/// Evaluate AND groups joined by OR. Empty expressions and groups never match.
pub fn matches<C>(groups: &[AllOf<C>], mut observe: impl FnMut(&C) -> Observation) -> bool {
    groups.iter().any(|group| {
        !group.all_of.is_empty()
            && group.all_of.iter().all(|term| match observe(&term.condition) {
                Observation::True => !term.negated,
                Observation::False => term.negated,
                Observation::Unknown => false,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::{
        AllOf, Condition, ConditionTerm, Connector, Observation, PaneFacts, Rule, StatusRules,
        StatusVisual, VisualKind, VisualStyle, default_rules, first_match, from_line, matches,
        shadowed_by, to_line, validate,
    };
    use crate::{AgentClass, AgentState, AgentTurn, GoalState, PaneStatus, ProgressTaskStatus};

    fn term(condition: Observation, negated: bool) -> ConditionTerm<Observation> {
        ConditionTerm { condition, negated }
    }

    #[test]
    fn and_binds_more_tightly_than_or() {
        let expression = [
            AllOf {
                all_of: vec![term(Observation::True, false)],
            },
            AllOf {
                all_of: vec![
                    term(Observation::False, false),
                    term(Observation::False, false),
                ],
            },
        ];
        assert!(matches(&expression, |condition| *condition));
    }

    #[test]
    fn expression_truth_table_is_a_or_b_and_c() {
        for a in [false, true] {
            for b in [false, true] {
                for c in [false, true] {
                    let expression = [
                        AllOf { all_of: vec![ConditionTerm { condition: a, negated: false }] },
                        AllOf { all_of: vec![
                            ConditionTerm { condition: b, negated: false },
                            ConditionTerm { condition: c, negated: false },
                        ] },
                    ];
                    let actual = matches(&expression, |value| if *value { Observation::True } else { Observation::False });
                    assert_eq!(actual, a || (b && c), "a={a}, b={b}, c={c}");
                }
            }
        }
    }

    #[test]
    fn flat_editor_line_round_trips_without_changing_connectors() {
        let terms = vec![term(Observation::True, false), term(Observation::False, true), term(Observation::True, false)];
        let connectors = vec![Connector::Or, Connector::And];
        let groups = from_line(terms.clone(), &connectors).expect("well-formed line");
        assert_eq!(groups.iter().map(|group| group.all_of.len()).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(to_line(&groups).expect("valid groups"), (terms, connectors));
        assert!(from_line(vec![term(Observation::True, false)], &[Connector::And]).is_err());
        assert!(to_line(&[AllOf::<()> { all_of: vec![] }]).is_err());
    }

    #[test]
    fn negation_never_turns_an_unknown_observation_into_a_match() {
        for negated in [false, true] {
            let expression = [AllOf {
                all_of: vec![term(Observation::Unknown, negated)],
            }];
            assert!(!matches(&expression, |condition| *condition));
        }
        let expression = [AllOf {
            all_of: vec![term(Observation::False, true)],
        }];
        assert!(matches(&expression, |condition| *condition));
    }

    #[test]
    fn empty_expression_and_empty_group_do_not_match() {
        assert!(!matches::<()>(&[], |_| Observation::True));
        assert!(!matches(&[AllOf::<()> { all_of: vec![] }], |_| {
            Observation::True
        }));
    }

    #[test]
    fn first_enabled_match_wins_and_order_can_be_changed() {
        let rule = |id: &str, enabled, condition| Rule {
            id: id.to_owned(),
            name: id.to_owned(),
            enabled,
            any_of: vec![AllOf {
                all_of: vec![term(condition, false)],
            }],
            then: id.to_owned(),
        };
        let mut rules = vec![
            rule("disabled", false, Observation::True),
            rule("first", true, Observation::True),
            rule("second", true, Observation::True),
        ];
        assert_eq!(
            first_match(&rules, |condition| *condition).map(|rule| rule.id.as_str()),
            Some("first")
        );
        rules.swap(1, 2);
        assert_eq!(
            first_match(&rules, |condition| *condition).map(|rule| rule.id.as_str()),
            Some("second")
        );
        assert!(first_match(&rules, |_| Observation::Unknown).is_none());
    }

    #[test]
    fn shadow_analysis_requires_a_provable_subset() {
        let make = |id: &str, terms: Vec<ConditionTerm<Observation>>| Rule {
            id: id.into(), name: id.into(), enabled: true,
            any_of: vec![AllOf { all_of: terms }], then: (),
        };
        let rules = vec![
            make("earlier", vec![term(Observation::True, false)]),
            make("shadowed", vec![term(Observation::True, false), term(Observation::False, true)]),
            make("distinct", vec![term(Observation::False, true)]),
        ];
        assert_eq!(shadowed_by(&rules, 1), Some(0));
        assert_eq!(shadowed_by(&rules, 2), None);
    }

    #[test]
    fn goal_and_turn_facts_are_observed_only_for_a_live_agent() {
        let status = PaneStatus::Agent(AgentState {
            class: AgentClass::Codex,
            turn: AgentTurn::Idle,
            goal: Some(GoalState::Paused),
            completion_unread: false,
        });
        let facts = PaneFacts {
            status: &status,
            progress: &[],
            scheduled_input: false,
            shell_output: None,
            activity_stale: None,
            screen_matches: &|_| Observation::Unknown,
        };
        assert_eq!(Condition::GoalSet.observe(&facts), Observation::True);
        assert_eq!(
            Condition::GoalState(GoalState::Paused).observe(&facts),
            Observation::True
        );
        assert_eq!(
            Condition::GoalState(GoalState::Blocked).observe(&facts),
            Observation::False
        );
        assert_eq!(Condition::Parked.observe(&facts), Observation::False);
        assert_eq!(Condition::AnyMonitorFailed.observe(&facts), Observation::False);
        assert_eq!(Condition::AnyTaskStatus(ProgressTaskStatus::Error).observe(&facts), Observation::False);

        let shell = PaneStatus::PlainShell;
        let facts = PaneFacts { status: &shell, ..facts };
        assert_eq!(Condition::GoalSet.observe(&facts), Observation::Unknown);
        assert_eq!(
            Condition::TaskStatus(crate::ProgressTaskStatus::Running).observe(&facts),
            Observation::Unknown
        );
    }

    #[test]
    fn any_monitor_conditions_include_nonrepresentative_monitors() {
        use crate::{PaneProgress, ProgressAttention, ProgressMonitorHealth, ProgressTaskReport, ProgressTaskStatus};

        let monitor = |id, status, health, attention| PaneProgress {
            monitor_id: id,
            report: ProgressTaskReport {
                job_id: format!("task-{id}"), status, percent: 50.0,
                message: "Example".into(), details: String::new(), error: None,
                eta_seconds: None, indeterminate: false,
            },
            monitor_health: health,
            last_observed_unix_millis: 0,
            timing: Default::default(),
            attention,
        };
        let progress = [
            monitor(1, ProgressTaskStatus::Running, ProgressMonitorHealth::Healthy, ProgressAttention::Acknowledged),
            monitor(2, ProgressTaskStatus::Error, ProgressMonitorHealth::Failed { consecutive_failures: 1, last_error: "unavailable".into() }, ProgressAttention::Acknowledged),
        ];
        let status = PaneStatus::PlainShell;
        let facts = PaneFacts {
            status: &status, progress: &progress, scheduled_input: false,
            shell_output: None, activity_stale: None, screen_matches: &|_| Observation::Unknown,
        };
        assert_eq!(Condition::TaskStatus(ProgressTaskStatus::Running).observe(&facts), Observation::True);
        assert_eq!(Condition::MonitorFailed.observe(&facts), Observation::False);
        assert_eq!(Condition::AnyTaskStatus(ProgressTaskStatus::Error).observe(&facts), Observation::True);
        assert_eq!(Condition::AnyMonitorFailed.observe(&facts), Observation::True);
    }

    #[test]
    fn validation_rejects_invalid_progress_and_duplicate_ids() {
        let rule = Rule {
            id: "one".to_owned(),
            name: "Invalid progress".to_owned(),
            enabled: true,
            any_of: vec![AllOf {
                all_of: vec![ConditionTerm {
                    condition: Condition::ProgressAtLeast(101),
                    negated: false,
                }],
            }],
            then: StatusVisual {
                kind: VisualKind::Hidden,
                style: VisualStyle::default(),
            },
        };
        let rules = StatusRules {
            defaults_version: 1,
            rules: vec![rule.clone(), rule],
            fallback: None,
        };
        let errors = validate(&rules);
        assert!(errors.iter().any(|error| error.contains("duplicate")));
        assert!(errors.iter().any(|error| error.contains("percentage")));
    }

    #[test]
    fn screen_watch_limit_counts_distinct_enabled_patterns() {
        use super::{ScreenPattern, ScreenRegion};

        let mut rules = default_rules();
        let example_index = rules.rules.iter().position(|rule| rule.id == "screen-error-example").unwrap();
        let mut example = rules.rules.remove(example_index);
        example.enabled = true;
        for index in 0..17 {
            let mut rule = example.clone();
            rule.id = format!("screen-{index}");
            rule.any_of[0].all_of[0].condition = Condition::ScreenMatches(ScreenPattern {
                pattern: "shared".to_owned(),
                ignore_case: false,
                region: ScreenRegion::All,
            });
            rules.rules.push(rule);
        }
        assert!(validate(&rules).is_empty());
        for (index, rule) in rules.rules.iter_mut().rev().take(17).enumerate() {
            if let Condition::ScreenMatches(pattern) = &mut rule.any_of[0].all_of[0].condition {
                pattern.pattern = format!("pattern-{index}");
            }
        }
        assert!(validate(&rules).iter().any(|error| error.contains("16 distinct")));
        rules.rules.last_mut().unwrap().enabled = false;
        assert!(validate(&rules).is_empty());
    }

    #[test]
    fn defaults_have_unique_valid_ids_and_the_screen_example_is_disabled() {
        let rules = default_rules();
        assert!(validate(&rules).is_empty());
        let example = rules.rules.iter().find(|rule| rule.id == "screen-error-example");
        assert!(example.is_some_and(|rule| !rule.enabled));
        let example_index = rules.rules.iter().position(|rule| rule.id == "screen-error-example").unwrap();
        let idle_index = rules.rules.iter().position(|rule| rule.id == "idle").unwrap();
        assert!(example_index < idle_index);
        assert_eq!(rules.rules.first().map(|rule| rule.id.as_str()), Some("approval"));
    }
}
