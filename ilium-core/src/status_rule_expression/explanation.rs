//! Per-rule evaluation outcomes and the human-readable "why" text shown when
//! a pane's icon comes from a custom rule.

use super::{term_holds, AllOf, Condition, ConditionTerm, Observation, Rule};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TermOutcome {
    pub negated: bool,
    pub observation: Observation,
    /// Whether the term holds; an unknown observation never holds.
    pub satisfied: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupOutcome {
    pub satisfied: bool,
    pub terms: Vec<TermOutcome>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleOutcome {
    pub enabled: bool,
    /// Whether the rule's expression holds, whether or not it is enabled or
    /// an earlier rule already won.
    pub matched: bool,
    pub groups: Vec<GroupOutcome>,
}

impl RuleOutcome {
    /// Index of the first satisfied AND group.
    pub fn satisfied_group(&self) -> Option<usize> {
        self.groups.iter().position(|group| group.satisfied)
    }
}

fn explain_groups<C>(
    groups: &[AllOf<C>],
    observe: &mut impl FnMut(&C) -> Observation,
) -> Vec<GroupOutcome> {
    groups
        .iter()
        .map(|group| {
            let terms: Vec<TermOutcome> = group
                .all_of
                .iter()
                .map(|term| {
                    let observation = observe(&term.condition);
                    TermOutcome {
                        negated: term.negated,
                        observation,
                        satisfied: term_holds(term.negated, observation),
                    }
                })
                .collect();
            GroupOutcome {
                satisfied: !terms.is_empty() && terms.iter().all(|term| term.satisfied),
                terms,
            }
        })
        .collect()
}

/// Evaluate every rule, including disabled ones and those below the winner,
/// so an editor can show which rules would match the selected pane. The
/// winner is the first outcome with `enabled && matched`, exactly as
/// [`super::first_match`] chooses it.
pub fn explain_all<C, V>(
    rules: &[Rule<C, V>],
    mut observe: impl FnMut(&C) -> Observation,
) -> Vec<RuleOutcome> {
    rules
        .iter()
        .map(|rule| {
            let groups = explain_groups(&rule.any_of, &mut observe);
            RuleOutcome {
                enabled: rule.enabled,
                matched: groups.iter().any(|group| group.satisfied),
                groups,
            }
        })
        .collect()
}

/// One term as text: `NOT agent is working`.
pub fn term_text(term: &ConditionTerm<Condition>) -> String {
    if term.negated {
        format!("NOT {}", term.condition.label())
    } else {
        term.condition.label()
    }
}

fn group_text(group: &AllOf<Condition>, outcome: Option<&GroupOutcome>) -> String {
    group
        .all_of
        .iter()
        .enumerate()
        .map(|(index, term)| {
            let text = term_text(term);
            match outcome.and_then(|outcome| outcome.terms.get(index)) {
                Some(TermOutcome {
                    observation: Observation::Unknown,
                    ..
                }) => format!("{text} (not observed)"),
                _ => text,
            }
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Why a rule applies, for the pane inspector and the editor preview:
/// `Custom rule 3 "Goal paused": goal is paused AND NOT agent is working`.
/// `number` is the rule's 1-based position. With an outcome, only the
/// satisfied AND group is shown; without one (or when nothing is satisfied),
/// the whole expression is shown with ` OR ` between groups and unknown
/// facts marked `(not observed)`.
pub fn why<V>(number: usize, rule: &Rule<Condition, V>, outcome: Option<&RuleOutcome>) -> String {
    let expression = match outcome.and_then(RuleOutcome::satisfied_group) {
        Some(index) => group_text(
            &rule.any_of[index],
            outcome.and_then(|outcome| outcome.groups.get(index)),
        ),
        None if rule.any_of.is_empty() => "no conditions".to_owned(),
        None => rule
            .any_of
            .iter()
            .enumerate()
            .map(|(index, group)| {
                group_text(group, outcome.and_then(|outcome| outcome.groups.get(index)))
            })
            .collect::<Vec<_>>()
            .join(" OR "),
    };
    format!("Custom rule {number} {:?}: {expression}", rule.name)
}

#[cfg(test)]
mod tests {
    use super::{explain_all, term_text, why};
    use crate::status_rule_expression::{AllOf, Condition, ConditionTerm, Observation, Rule};
    use crate::{AgentTurn, GoalState};

    fn rule(
        name: &str,
        enabled: bool,
        groups: Vec<Vec<ConditionTerm<Condition>>>,
    ) -> Rule<Condition, ()> {
        Rule {
            id: name.to_owned(),
            name: name.to_owned(),
            enabled,
            any_of: groups.into_iter().map(|all_of| AllOf { all_of }).collect(),
            then: (),
        }
    }

    fn yes(condition: Condition) -> ConditionTerm<Condition> {
        ConditionTerm {
            condition,
            negated: false,
        }
    }

    fn not(condition: Condition) -> ConditionTerm<Condition> {
        ConditionTerm {
            condition,
            negated: true,
        }
    }

    #[test]
    fn explain_all_reports_every_rule_and_agrees_with_first_match() {
        let rules = vec![
            rule("Disabled", false, vec![vec![yes(Condition::Always)]]),
            rule(
                "Goal paused",
                true,
                vec![vec![
                    yes(Condition::GoalState(GoalState::Paused)),
                    not(Condition::AgentTurn(AgentTurn::Working)),
                ]],
            ),
            rule(
                "Idle",
                true,
                vec![vec![yes(Condition::AgentTurn(AgentTurn::Idle))]],
            ),
            rule("Stale", true, vec![vec![yes(Condition::ActivityStale)]]),
        ];
        let observe = |condition: &Condition| match condition {
            Condition::Always
            | Condition::GoalState(GoalState::Paused)
            | Condition::AgentTurn(AgentTurn::Idle) => Observation::True,
            Condition::AgentTurn(AgentTurn::Working) => Observation::False,
            _ => Observation::Unknown,
        };
        let outcomes = explain_all(&rules, observe);
        assert_eq!(
            outcomes
                .iter()
                .map(|outcome| (outcome.enabled, outcome.matched))
                .collect::<Vec<_>>(),
            vec![(false, true), (true, true), (true, true), (true, false),]
        );
        let winner = outcomes
            .iter()
            .position(|outcome| outcome.enabled && outcome.matched);
        let first = crate::status_rule_expression::first_match(&rules, observe)
            .map(|rule| rule.name.as_str());
        assert_eq!((winner, first), (Some(1), Some("Goal paused")));
        assert_eq!(outcomes[1].satisfied_group(), Some(0));
        assert_eq!(
            outcomes[3].groups[0].terms[0].observation,
            Observation::Unknown
        );
        assert!(!outcomes[3].groups[0].terms[0].satisfied);
    }

    #[test]
    fn why_describes_the_satisfied_group_or_the_whole_expression() {
        let paused = rule(
            "Goal paused",
            true,
            vec![vec![
                yes(Condition::GoalState(GoalState::Paused)),
                not(Condition::AgentTurn(AgentTurn::Working)),
            ]],
        );
        let outcomes =
            explain_all(
                std::slice::from_ref(&paused),
                |condition: &Condition| match condition {
                    Condition::GoalState(_) => Observation::True,
                    _ => Observation::False,
                },
            );
        assert_eq!(
            why(3, &paused, Some(&outcomes[0])),
            "Custom rule 3 \"Goal paused\": goal is paused AND NOT agent is working"
        );

        let either = rule(
            "Either",
            true,
            vec![
                vec![yes(Condition::ActivityStale)],
                vec![yes(Condition::ScheduledInput)],
            ],
        );
        let outcomes =
            explain_all(
                std::slice::from_ref(&either),
                |condition: &Condition| match condition {
                    Condition::ScheduledInput => Observation::False,
                    _ => Observation::Unknown,
                },
            );
        assert_eq!(
            why(1, &either, Some(&outcomes[0])),
            "Custom rule 1 \"Either\": activity check is stale (not observed) OR input is scheduled"
        );
        assert_eq!(
            why(1, &either, None),
            "Custom rule 1 \"Either\": activity check is stale OR input is scheduled"
        );
        assert_eq!(
            why(2, &rule("Empty", true, vec![]), None),
            "Custom rule 2 \"Empty\": no conditions"
        );
        assert_eq!(term_text(&not(Condition::Parked)), "NOT agent is parked");
    }
}
