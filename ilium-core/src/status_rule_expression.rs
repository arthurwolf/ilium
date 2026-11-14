//! Boolean expressions used by custom status-icon rules.
//!
//! An expression is a disjunction of conjunctions: each inner group is
//! joined with AND, and groups are joined with OR. Missing observations do
//! not become true merely because a condition is negated.
//!
//! This file owns the rule model and the evaluation primitives. The sibling
//! modules own one concern each so the editor, the configuration adapter,
//! the documentation and the tests share a single definition:
//! `registry` (condition catalogue, labels, categories, explanations),
//! `validation` (typed issues), `explanation` (per-rule outcomes and "why"
//! text), `defaults` (the shipped rule set) and `demo` (named situations).

use serde::{Deserialize, Serialize};

use crate::{
    has_live_progress, representative_progress, AgentAvailability, AgentClass, AgentExitOutcome,
    AgentTurn, GoalState, PaneProgress, PaneStatus, ProgressAttention, ProgressMonitorHealth,
    ProgressTaskStatus, ShellOutputPhase,
};

pub mod defaults;
pub mod demo;
pub mod explanation;
pub mod registry;
pub mod validation;

pub use defaults::{default_rules, DEFAULTS_VERSION, SCREEN_EXAMPLE_RULE_ID};
pub use demo::{demo_situations, DemoSituation};
pub use explanation::{explain_all, term_text, why, GroupOutcome, RuleOutcome, TermOutcome};
pub use registry::{condition_catalog, ConditionCategory};
pub use validation::{
    error_summary, has_errors, validate, IssueKind, IssueSeverity, TermPath, ValidationIssue,
    MAXIMUM_CONDITIONS_PER_GROUP, MAXIMUM_CONDITIONS_PER_RULE, MAXIMUM_ENABLED_SCREEN_PATTERNS,
    MAXIMUM_GROUPS_PER_RULE, MAXIMUM_NAME_CHARS, MAXIMUM_PATTERN_CHARS, MAXIMUM_RULES,
};

/// The result of observing a single condition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Observation {
    True,
    False,
    Unknown,
}

/// Facts outside the pane's [`PaneStatus`]: client-only observations (frozen
/// screen, spend, keyboard focus) and the pane node's own fields (bookmark, prompt queue).
/// `None` means the fact was not observed for this pane, so the matching
/// conditions stay `Unknown` instead of silently reading false.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SupplementaryFacts {
    /// Whether the pane's agent screen is frozen by the user.
    pub frozen: Option<bool>,
    /// The agent session's spend so far, in US cents.
    pub session_cost_cents: Option<u64>,
    /// Whether the user bookmarked the pane in the sidebar.
    pub bookmarked: Option<bool>,
    /// How many prompts wait in the pane's prompt queue. The kernel keeps no
    /// queue (the Scheduler plugin owns it), so this stays unknown unless an
    /// owner of the queue supplies it.
    pub queued_prompts: Option<u32>,
    /// Whether this pane's terminal holds keyboard focus. False for every
    /// pane while focus is on the tree, a dialog or Settings.
    pub focused: Option<bool>,
    /// Whether this pane has a retained blocking error (a failed action
    /// whose target is exactly this pane).
    pub blocking_error: Option<bool>,
    /// Whether the session has a retained session-level error, one that
    /// belongs to no particular pane. Identical for every pane of a session.
    pub session_error: Option<bool>,
    /// Whether the tree cursor rests on this pane's row, whichever part of
    /// the window holds keyboard focus.
    pub selected: Option<bool>,
    /// Whether the pane's terminal is on screen in the right panel, alone
    /// or as a split-view member.
    pub visible: Option<bool>,
    /// Whether the prompt queue is held on a head prompt whose delivery was
    /// interrupted. The prompt is never replayed, so the queue waits for the
    /// user. Supplied by the owner of the queue; unknown otherwise.
    pub prompt_queue_held: Option<bool>,
    /// How full the agent's context window was at its latest prompt, in
    /// tenths of a percent (0..=1000).
    pub context_permille: Option<u16>,
}

impl SupplementaryFacts {
    /// Add the facts the pane's tree node carries. Every builder of live
    /// pane facts (tree renderer, hover tooltip, editor inspector) calls this
    /// so the three cannot disagree about a node fact.
    pub fn with_node(self, node: &crate::Node) -> Self {
        let crate::NodeKind::Pane { .. } = &node.kind else {
            return self;
        };
        Self {
            bookmarked: Some(node.is_bookmarked),
            ..self
        }
    }
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
    pub supplementary: SupplementaryFacts,
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
/// Adding a variant also requires a registry entry (`registry.rs`), which the
/// registry tests enforce.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Condition {
    Always,
    PaneKind(PaneKind),
    EditorDirty,
    PaneFrozen,
    /// The pane's terminal holds keyboard focus.
    PaneFocused,
    /// The pane has a retained blocking error.
    BlockingError,
    /// The session has a retained session-level error.
    SessionError,
    PaneBookmarked,
    PromptsQueued,
    /// The prompt queue is held on a head whose delivery was interrupted.
    PromptQueueHeld,
    /// The tree cursor rests on the pane's row.
    PaneSelected,
    /// The pane's terminal is on screen in the right panel.
    PaneVisible,
    AgentClass(AgentClass),
    /// Any agent other than the built-in Claude Code, Codex and Antigravity
    /// (custom signatures).
    OtherAgent,
    AgentAvailability(AvailabilityKind),
    AgentTurn(AgentTurn),
    GoalSet,
    GoalState(GoalState),
    CompletionUnread,
    ActivityStale,
    /// A live agent whose activity freshness was never observed.
    ActivityUnchecked,
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
    ProgressBetween {
        minimum: u8,
        maximum: u8,
    },
    EtaKnown,
    EtaBelowMinutes(u16),
    Parked,
    ScheduledInput,
    ShellOutputRecent,
    ShellOutputFast,
    ShellOutputSlow,
    /// Whole US dollars; the client supplies the session spend in cents.
    SessionCostAbove(u32),
    /// The agent's context window is more than this percentage full.
    ContextAbovePercent(u8),
    ScreenMatches(ScreenPattern),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaneKind {
    Agent,
    PlainShell,
    AgentUnavailable,
    Editor,
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
        let eta_seconds = monitor.and_then(|monitor| monitor.report.eta_seconds);
        let result = match self {
            Self::Always => Some(true),
            Self::PaneKind(kind) => Some(matches!(
                (kind, facts.status),
                (PaneKind::Agent, PaneStatus::Agent(_))
                    | (PaneKind::PlainShell, PaneStatus::PlainShell)
                    | (PaneKind::AgentUnavailable, PaneStatus::AgentUnavailable(_))
                    | (PaneKind::Editor, PaneStatus::Editor { .. })
            )),
            Self::EditorDirty => match facts.status {
                PaneStatus::Editor { dirty } => Some(*dirty),
                _ => None,
            },
            Self::PaneFrozen => facts.supplementary.frozen,
            Self::PaneFocused => facts.supplementary.focused,
            Self::BlockingError => facts.supplementary.blocking_error,
            Self::SessionError => facts.supplementary.session_error,
            Self::PaneBookmarked => facts.supplementary.bookmarked,
            Self::PromptsQueued => facts.supplementary.queued_prompts.map(|count| count > 0),
            Self::PromptQueueHeld => facts.supplementary.prompt_queue_held,
            Self::PaneSelected => facts.supplementary.selected,
            Self::PaneVisible => facts.supplementary.visible,
            Self::AgentClass(class) => agent.map(|agent| &agent.class == class),
            Self::OtherAgent => agent.map(|agent| matches!(agent.class, AgentClass::Other(_))),
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
            // Known exactly when a live agent runs: the freshness check is
            // either reported (false) or missing (true).
            Self::ActivityUnchecked => agent.map(|_| facts.activity_stale.is_none()),
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
            // Without a monitor the estimate is unobservable; with a monitor
            // but no estimate it is known to be absent.
            Self::EtaKnown => monitor.map(|_| eta_seconds.is_some()),
            Self::EtaBelowMinutes(minutes) => eta_seconds.map(|seconds| seconds < u64::from(*minutes) * 60),
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
            Self::SessionCostAbove(dollars) => facts.supplementary.session_cost_cents.map(|cents| cents > u64::from(*dollars) * 100),
            // Compared in tenths of a percent so 80.4% is above 80%.
            Self::ContextAbovePercent(percent) => facts.supplementary.context_permille.map(|permille| permille > u16::from(*percent) * 10),
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
    let mut groups = vec![AllOf {
        all_of: vec![first],
    }];
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
pub fn to_line<C: Clone>(
    groups: &[AllOf<C>],
) -> Result<(Vec<ConditionTerm<C>>, Vec<Connector>), &'static str> {
    let mut terms = Vec::new();
    let mut connectors = Vec::new();
    for group in groups {
        if group.all_of.is_empty() {
            return Err("an AND group cannot be empty");
        }
        for (index, term) in group.all_of.iter().enumerate() {
            if !terms.is_empty() {
                connectors.push(if index == 0 {
                    Connector::Or
                } else {
                    Connector::And
                });
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
    BrailleAnimation {
        preset: String,
        frames_per_second: u8,
    },
    FrameAnimation {
        frames: Vec<String>,
        frames_per_second: u8,
        ping_pong: bool,
    },
    ProgressFrames {
        frames: Vec<String>,
    },
}

/// Every field may be left out of a hand-written `config.toml`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VisualStyle {
    /// Theme colour name or an explicit RGB colour.
    pub color: Option<String>,
    pub bold: bool,
    pub dim: bool,
    /// Draw attention with a pulsing icon. The client renderer decides how
    /// the pulse honours the user's motion setting.
    pub pulse: bool,
}

/// The `[status_rules]` format version this build reads and writes.
pub const STATUS_RULES_SCHEMA_VERSION: u32 = 1;

const fn status_rules_schema_version() -> u32 {
    STATUS_RULES_SCHEMA_VERSION
}

/// User-owned rules are ordered and persisted together with their fallback.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StatusRules {
    /// Format version of the stored table; absent means version 1.
    #[serde(default = "status_rules_schema_version")]
    pub version: u32,
    /// Which shipped default set the rules started from, so a later Ilium
    /// can offer new defaults without overwriting the user's edits.
    pub defaults_version: u32,
    pub rules: Vec<Rule<Condition, StatusVisual>>,
    pub fallback: Option<StatusVisual>,
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
    if !rule.enabled
        || rule.any_of.is_empty()
        || rule.any_of.iter().any(|group| group.all_of.is_empty())
    {
        return None;
    }
    rules[..index].iter().position(|earlier| {
        earlier.enabled
            && earlier.any_of.iter().any(|earlier_group| {
                !earlier_group.all_of.is_empty()
                    && rule.any_of.iter().all(|later_group| {
                        earlier_group
                            .all_of
                            .iter()
                            .all(|term| later_group.all_of.contains(term))
                    })
            })
    })
}

/// Whether one term holds under an observation: an unknown fact never
/// satisfies a term, negated or not.
pub fn term_holds(negated: bool, observation: Observation) -> bool {
    match observation {
        Observation::True => !negated,
        Observation::False => negated,
        Observation::Unknown => false,
    }
}

/// Evaluate AND groups joined by OR. Empty expressions and groups never match.
pub fn matches<C>(groups: &[AllOf<C>], mut observe: impl FnMut(&C) -> Observation) -> bool {
    groups.iter().any(|group| {
        !group.all_of.is_empty()
            && group
                .all_of
                .iter()
                .all(|term| term_holds(term.negated, observe(&term.condition)))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        first_match, from_line, matches, shadowed_by, to_line, AllOf, Condition, ConditionTerm,
        Connector, Observation, PaneFacts, Rule, SupplementaryFacts,
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
                        AllOf {
                            all_of: vec![ConditionTerm {
                                condition: a,
                                negated: false,
                            }],
                        },
                        AllOf {
                            all_of: vec![
                                ConditionTerm {
                                    condition: b,
                                    negated: false,
                                },
                                ConditionTerm {
                                    condition: c,
                                    negated: false,
                                },
                            ],
                        },
                    ];
                    let actual = matches(&expression, |value| {
                        if *value {
                            Observation::True
                        } else {
                            Observation::False
                        }
                    });
                    assert_eq!(actual, a || (b && c), "a={a}, b={b}, c={c}");
                }
            }
        }
    }

    #[test]
    fn flat_editor_line_round_trips_without_changing_connectors() {
        let terms = vec![
            term(Observation::True, false),
            term(Observation::False, true),
            term(Observation::True, false),
        ];
        let connectors = vec![Connector::Or, Connector::And];
        let groups = from_line(terms.clone(), &connectors).expect("well-formed line");
        assert_eq!(
            groups
                .iter()
                .map(|group| group.all_of.len())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
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
            id: id.into(),
            name: id.into(),
            enabled: true,
            any_of: vec![AllOf { all_of: terms }],
            then: (),
        };
        let rules = vec![
            make("earlier", vec![term(Observation::True, false)]),
            make(
                "shadowed",
                vec![
                    term(Observation::True, false),
                    term(Observation::False, true),
                ],
            ),
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
            supplementary: SupplementaryFacts::default(),
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
        assert_eq!(
            Condition::AnyMonitorFailed.observe(&facts),
            Observation::False
        );
        assert_eq!(
            Condition::AnyTaskStatus(ProgressTaskStatus::Error).observe(&facts),
            Observation::False
        );

        let shell = PaneStatus::PlainShell;
        let facts = PaneFacts {
            status: &shell,
            ..facts
        };
        assert_eq!(Condition::GoalSet.observe(&facts), Observation::Unknown);
        assert_eq!(
            Condition::TaskStatus(crate::ProgressTaskStatus::Running).observe(&facts),
            Observation::Unknown
        );
    }

    #[test]
    fn any_monitor_conditions_include_nonrepresentative_monitors() {
        use crate::{
            PaneProgress, ProgressAttention, ProgressMonitorHealth, ProgressTaskReport,
            ProgressTaskStatus,
        };

        let monitor = |id, status, health, attention| PaneProgress {
            monitor_id: id,
            report: ProgressTaskReport {
                job_id: format!("task-{id}"),
                status,
                percent: 50.0,
                message: "Example".into(),
                details: String::new(),
                error: None,
                eta_seconds: None,
                indeterminate: false,
            },
            monitor_health: health,
            last_observed_unix_millis: 0,
            timing: Default::default(),
            attention,
        };
        let progress = [
            monitor(
                1,
                ProgressTaskStatus::Running,
                ProgressMonitorHealth::Healthy,
                ProgressAttention::Acknowledged,
            ),
            monitor(
                2,
                ProgressTaskStatus::Error,
                ProgressMonitorHealth::Failed {
                    consecutive_failures: 1,
                    last_error: "unavailable".into(),
                },
                ProgressAttention::Acknowledged,
            ),
        ];
        let status = PaneStatus::PlainShell;
        let facts = PaneFacts {
            status: &status,
            progress: &progress,
            scheduled_input: false,
            shell_output: None,
            activity_stale: None,
            supplementary: SupplementaryFacts::default(),
            screen_matches: &|_| Observation::Unknown,
        };
        assert_eq!(
            Condition::TaskStatus(ProgressTaskStatus::Running).observe(&facts),
            Observation::True
        );
        assert_eq!(Condition::MonitorFailed.observe(&facts), Observation::False);
        assert_eq!(
            Condition::AnyTaskStatus(ProgressTaskStatus::Error).observe(&facts),
            Observation::True
        );
        assert_eq!(
            Condition::AnyMonitorFailed.observe(&facts),
            Observation::True
        );
    }

    #[test]
    fn activity_unchecked_and_custom_agent_need_a_live_agent() {
        let custom = PaneStatus::Agent(AgentState {
            class: AgentClass::Other("aider".to_owned()),
            turn: AgentTurn::Working,
            goal: None,
            completion_unread: false,
        });
        let mut facts = PaneFacts {
            status: &custom,
            progress: &[],
            scheduled_input: false,
            shell_output: None,
            activity_stale: None,
            supplementary: SupplementaryFacts::default(),
            screen_matches: &|_| Observation::Unknown,
        };
        assert_eq!(Condition::OtherAgent.observe(&facts), Observation::True);
        assert_eq!(
            Condition::AgentClass(AgentClass::Other("aider".to_owned())).observe(&facts),
            Observation::True
        );
        assert_eq!(
            Condition::AgentClass(AgentClass::Other("goose".to_owned())).observe(&facts),
            Observation::False
        );
        assert_eq!(
            Condition::ActivityUnchecked.observe(&facts),
            Observation::True
        );
        // The stale condition itself stays unobservable, which is exactly
        // why "unchecked" exists.
        assert_eq!(
            Condition::ActivityStale.observe(&facts),
            Observation::Unknown
        );
        for stale in [false, true] {
            facts.activity_stale = Some(stale);
            assert_eq!(
                Condition::ActivityUnchecked.observe(&facts),
                Observation::False
            );
        }

        let codex = PaneStatus::Agent(AgentState {
            class: AgentClass::Codex,
            turn: AgentTurn::Idle,
            goal: None,
            completion_unread: false,
        });
        facts.status = &codex;
        assert_eq!(Condition::OtherAgent.observe(&facts), Observation::False);

        let shell = PaneStatus::PlainShell;
        facts.status = &shell;
        facts.activity_stale = None;
        assert_eq!(Condition::OtherAgent.observe(&facts), Observation::Unknown);
        assert_eq!(
            Condition::ActivityUnchecked.observe(&facts),
            Observation::Unknown
        );
    }

    #[test]
    fn node_facts_feed_the_bookmark_condition_only() {
        use crate::{PaneContentKind, Tree, ROOT_ID};

        let status = PaneStatus::PlainShell;
        let mut facts = PaneFacts {
            status: &status,
            progress: &[],
            scheduled_input: false,
            shell_output: None,
            activity_stale: None,
            supplementary: SupplementaryFacts::default(),
            screen_matches: &|_| Observation::Unknown,
        };
        assert_eq!(
            Condition::PaneBookmarked.observe(&facts),
            Observation::Unknown
        );

        let mut tree = Tree::new();
        let group_id = tree.add_group(ROOT_ID, "group").unwrap();
        let pane_id = tree
            .add_pane(group_id, "pane", PaneContentKind::Terminal)
            .unwrap();
        let pane = tree.get(pane_id).unwrap();
        facts.supplementary = SupplementaryFacts::default().with_node(pane);
        assert_eq!(facts.supplementary.bookmarked, Some(false));
        // The kernel keeps no prompt queue, so those facts stay unobserved.
        assert_eq!(facts.supplementary.queued_prompts, None);
        assert_eq!(facts.supplementary.prompt_queue_held, None);
        assert_eq!(
            Condition::PaneBookmarked.observe(&facts),
            Observation::False
        );

        facts.supplementary.bookmarked = Some(true);
        facts.supplementary.queued_prompts = Some(3);
        assert_eq!(Condition::PaneBookmarked.observe(&facts), Observation::True);
        assert_eq!(Condition::PromptsQueued.observe(&facts), Observation::True);

        // A group carries neither fact, so nothing is invented for it.
        let group = tree.get(group_id).unwrap();
        assert_eq!(
            SupplementaryFacts::default().with_node(group),
            SupplementaryFacts::default()
        );
    }

    #[test]
    fn view_and_context_conditions_read_their_facts_or_stay_unknown() {
        let status = PaneStatus::Agent(AgentState::from_activity(
            AgentClass::Claude,
            crate::AgentActivity::Idle,
            None,
        ));
        let mut facts = PaneFacts {
            status: &status,
            progress: &[],
            scheduled_input: false,
            shell_output: None,
            activity_stale: None,
            supplementary: SupplementaryFacts::default(),
            screen_matches: &|_| Observation::Unknown,
        };
        for condition in [
            Condition::PaneSelected,
            Condition::PaneVisible,
            Condition::ContextAbovePercent(80),
        ] {
            assert_eq!(
                condition.observe(&facts),
                Observation::Unknown,
                "{condition:?}"
            );
        }

        facts.supplementary.selected = Some(true);
        facts.supplementary.visible = Some(false);
        assert_eq!(Condition::PaneSelected.observe(&facts), Observation::True);
        assert_eq!(Condition::PaneVisible.observe(&facts), Observation::False);

        // Boundary: strictly above, measured in tenths of a percent.
        facts.supplementary.context_permille = Some(800);
        assert_eq!(
            Condition::ContextAbovePercent(80).observe(&facts),
            Observation::False
        );
        facts.supplementary.context_permille = Some(801);
        assert_eq!(
            Condition::ContextAbovePercent(80).observe(&facts),
            Observation::True
        );
        // Empty window reads false; 0% threshold is true for any use.
        facts.supplementary.context_permille = Some(0);
        assert_eq!(
            Condition::ContextAbovePercent(0).observe(&facts),
            Observation::False
        );
        facts.supplementary.context_permille = Some(1000);
        assert_eq!(
            Condition::ContextAbovePercent(100).observe(&facts),
            Observation::False
        );
        assert_eq!(
            Condition::ContextAbovePercent(99).observe(&facts),
            Observation::True
        );
    }

    #[test]
    fn hand_written_tables_may_omit_the_version_and_style_flags() {
        let rules: super::StatusRules = toml::from_str(
            "defaults_version = 1\nrules = []\n[fallback]\nkind = \"hidden\"\n[fallback.style]\nbold = true\n",
        )
        .expect("parseable");
        assert_eq!(rules.version, super::STATUS_RULES_SCHEMA_VERSION);
        let style = rules.fallback.expect("fallback").style;
        assert!(style.bold && !style.dim && !style.pulse && style.color.is_none());
    }

    #[test]
    fn eta_frozen_and_cost_conditions_stay_unknown_without_their_facts() {
        use crate::{PaneProgress, ProgressAttention, ProgressMonitorHealth, ProgressTaskReport};

        let status = PaneStatus::PlainShell;
        let mut facts = PaneFacts {
            status: &status,
            progress: &[],
            scheduled_input: false,
            shell_output: None,
            activity_stale: None,
            supplementary: SupplementaryFacts::default(),
            screen_matches: &|_| Observation::Unknown,
        };
        assert_eq!(Condition::EtaKnown.observe(&facts), Observation::Unknown);
        assert_eq!(
            Condition::EtaBelowMinutes(5).observe(&facts),
            Observation::Unknown
        );
        assert_eq!(Condition::PaneFrozen.observe(&facts), Observation::Unknown);
        assert_eq!(Condition::PaneFocused.observe(&facts), Observation::Unknown);
        assert_eq!(
            Condition::SessionCostAbove(1).observe(&facts),
            Observation::Unknown
        );

        let mut monitor = PaneProgress {
            monitor_id: 1,
            report: ProgressTaskReport {
                job_id: "task".into(),
                status: ProgressTaskStatus::Running,
                percent: 10.0,
                message: "Example".into(),
                details: String::new(),
                error: None,
                eta_seconds: None,
                indeterminate: false,
            },
            monitor_health: ProgressMonitorHealth::Healthy,
            last_observed_unix_millis: 0,
            timing: Default::default(),
            attention: ProgressAttention::Acknowledged,
        };
        let without_eta = [monitor.clone()];
        facts.progress = &without_eta;
        assert_eq!(Condition::EtaKnown.observe(&facts), Observation::False);
        assert_eq!(
            Condition::EtaBelowMinutes(5).observe(&facts),
            Observation::Unknown
        );

        monitor.report.eta_seconds = Some(299);
        let with_eta = [monitor];
        facts.progress = &with_eta;
        assert_eq!(Condition::EtaKnown.observe(&facts), Observation::True);
        assert_eq!(
            Condition::EtaBelowMinutes(5).observe(&facts),
            Observation::True
        );
        assert_eq!(
            Condition::EtaBelowMinutes(4).observe(&facts),
            Observation::False
        );

        facts.supplementary = SupplementaryFacts {
            frozen: Some(true),
            session_cost_cents: Some(250),
            focused: Some(false),
            ..SupplementaryFacts::default()
        };
        assert_eq!(Condition::PaneFrozen.observe(&facts), Observation::True);
        assert_eq!(Condition::PaneFocused.observe(&facts), Observation::False);
        // Unobserved error facts stay Unknown rather than reading false.
        assert_eq!(
            Condition::BlockingError.observe(&facts),
            Observation::Unknown
        );
        assert_eq!(
            Condition::SessionError.observe(&facts),
            Observation::Unknown
        );
        facts.supplementary.blocking_error = Some(true);
        facts.supplementary.session_error = Some(false);
        assert_eq!(Condition::BlockingError.observe(&facts), Observation::True);
        assert_eq!(Condition::SessionError.observe(&facts), Observation::False);
        facts.supplementary.focused = Some(true);
        assert_eq!(Condition::PaneFocused.observe(&facts), Observation::True);
        assert_eq!(
            Condition::SessionCostAbove(2).observe(&facts),
            Observation::True
        );
        assert_eq!(
            Condition::SessionCostAbove(3).observe(&facts),
            Observation::False
        );
    }
}
