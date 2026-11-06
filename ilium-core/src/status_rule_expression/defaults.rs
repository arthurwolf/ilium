//! The shipped status rules: top to bottom "needs you now", then "something
//! is wrong", "waiting on something", "making progress", and finally "calm".

use super::{
    AllOf, Condition, ConditionTerm, PaneKind, Rule, ScreenPattern, ScreenRegion, StatusRules,
    StatusVisual, VisualKind, VisualStyle, STATUS_RULES_SCHEMA_VERSION,
};
use crate::{AgentTurn, GoalState, ProgressTaskStatus};

/// Version of the shipped rule set, stored with saved rules so a later Ilium
/// can offer new defaults without overwriting edits.
pub const DEFAULTS_VERSION: u32 = 2;

/// Identifier of the disabled example rule that shows the screen condition.
pub const SCREEN_EXAMPLE_RULE_ID: &str = "screen-error-example";

/// Rules whose icon pulses: the two "look at me now" situations of the plan
/// (rows 1 and 10).
const PULSING_RULE_IDS: [&str; 2] = ["approval", "completion-unread"];

#[derive(Clone, Copy)]
enum Weight {
    Regular,
    Bold,
    Dim,
}

struct DefaultRule {
    id: &'static str,
    name: &'static str,
    kind: VisualKind,
    color: &'static str,
    weight: Weight,
    terms: Vec<ConditionTerm<Condition>>,
}

fn built_in(icon: &str) -> VisualKind {
    VisualKind::BuiltIn(icon.to_owned())
}

fn frames(frames: &[&str]) -> Vec<String> {
    frames.iter().map(|frame| (*frame).to_owned()).collect()
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

/// Fresh starting priorities for users who have not saved their own rules.
pub fn default_rules() -> StatusRules {
    use AgentTurn::{Idle, Settling, WaitingApproval, WaitingSubagents, Working};
    use Condition::{AgentTurn as Turn, GoalState as Goal};
    use GoalState::{Active, Blocked, Paused, Reached, UsageLimited};
    use Weight::{Bold, Dim, Regular};

    let rule = |id, name, kind, color, weight, terms| DefaultRule {
        id,
        name,
        kind,
        color,
        weight,
        terms,
    };
    let task = |status| Condition::TaskStatus(status);
    let table = vec![
        rule(
            "approval",
            "Needs your approval",
            built_in("waiting_approval"),
            "warning",
            Bold,
            vec![yes(Turn(WaitingApproval))],
        ),
        rule(
            "agent-lost",
            "Agent lost",
            built_in("agent_unavailable"),
            "danger",
            Regular,
            vec![yes(Condition::PaneKind(PaneKind::AgentUnavailable))],
        ),
        rule(
            "blocking-error",
            "Blocking error",
            // The warning glyph, the same built-in the monitor-lost rule uses.
            built_in("monitor_failed"),
            "danger",
            Regular,
            vec![yes(Condition::BlockingError)],
        ),
        rule(
            "task-failed",
            "Task failed",
            built_in("task_error"),
            "danger",
            Bold,
            vec![yes(task(ProgressTaskStatus::Error))],
        ),
        rule(
            "monitor-lost",
            "Monitor lost",
            built_in("monitor_failed"),
            "danger",
            Regular,
            vec![
                yes(Condition::MonitorFailed),
                not(task(ProgressTaskStatus::Done)),
                not(task(ProgressTaskStatus::Error)),
            ],
        ),
        rule(
            "goal-blocked",
            "Goal blocked",
            built_in("goal_blocked"),
            "warning",
            Bold,
            vec![yes(Goal(Blocked))],
        ),
        rule(
            "usage-limited",
            "Usage limit reached",
            built_in("goal_usage_limited"),
            "warning",
            Regular,
            vec![yes(Goal(UsageLimited))],
        ),
        rule(
            "goal-paused",
            "Goal paused",
            built_in("goal_paused"),
            "info",
            Regular,
            vec![yes(Goal(Paused)), not(Turn(Working))],
        ),
        rule(
            "task-unread",
            "Task finished, result unread",
            built_in("task_done"),
            "success",
            Bold,
            vec![
                yes(task(ProgressTaskStatus::Done)),
                yes(Condition::MonitorUnread),
            ],
        ),
        rule(
            "goal-reached",
            "Goal reached",
            built_in("goal_reached"),
            "success",
            Regular,
            vec![yes(Goal(Reached))],
        ),
        rule(
            "completion-unread",
            "Finished, not seen yet",
            built_in("done"),
            "accent",
            Bold,
            vec![yes(Condition::CompletionUnread), not(Condition::Parked)],
        ),
        // Plan row 11 asks the parked progress icon to turn to the warning
        // colour while the monitor is degraded. Colour is a rule's visual, so
        // that tint is its own rule directly above the calm one.
        rule(
            "parked-progress-unsteady",
            "Parked, task monitor unsteady",
            VisualKind::ProgressFrames {
                frames: frames(&["🌑", "🌒", "🌓", "🌔", "🌕"]),
            },
            "warning",
            Regular,
            vec![
                yes(Condition::Parked),
                yes(Condition::ProgressMeasurable),
                yes(Condition::MonitorDegraded),
            ],
        ),
        rule(
            "parked-progress",
            "Parked, task progressing",
            VisualKind::ProgressFrames {
                frames: frames(&["🌑", "🌒", "🌓", "🌔", "🌕"]),
            },
            "info",
            Regular,
            vec![yes(Condition::Parked), yes(Condition::ProgressMeasurable)],
        ),
        rule(
            "parked",
            "Parked, waiting for task",
            built_in("parked"),
            "info",
            Regular,
            vec![yes(Condition::Parked)],
        ),
        rule(
            "subagents",
            "Waiting for background agents",
            built_in("waiting_background"),
            "info",
            Regular,
            vec![yes(Turn(WaitingSubagents))],
        ),
        rule(
            "settling",
            "Background work still running",
            built_in("background_task_still_running"),
            "info",
            Regular,
            vec![yes(Turn(Settling))],
        ),
        rule(
            "working-goal",
            "Working toward a goal",
            VisualKind::BrailleAnimation {
                preset: "spinner".to_owned(),
                frames_per_second: 8,
            },
            "accent",
            Regular,
            vec![yes(Turn(Working)), yes(Goal(Active))],
        ),
        rule(
            "working",
            "Working",
            built_in("working"),
            "success",
            Regular,
            vec![yes(Turn(Working))],
        ),
        rule(
            "pending",
            "Task pending",
            built_in("task_pending"),
            "muted",
            Regular,
            vec![yes(task(ProgressTaskStatus::NotStartedYet))],
        ),
        rule(
            "scheduled",
            "Next prompt scheduled",
            built_in("scheduled_input"),
            "muted",
            Regular,
            vec![yes(Condition::ScheduledInput)],
        ),
        rule(
            "goal-idle",
            "Goal active, agent idle",
            built_in("goal_active"),
            "muted",
            Regular,
            vec![yes(Goal(Active)), yes(Turn(Idle))],
        ),
        rule(
            "shell-busy",
            "Terminal busy",
            VisualKind::BrailleAnimation {
                preset: "loop".to_owned(),
                frames_per_second: 8,
            },
            "muted",
            Regular,
            vec![
                yes(Condition::PaneKind(PaneKind::PlainShell)),
                yes(Condition::ShellOutputRecent),
            ],
        ),
        rule(
            "idle",
            "Idle",
            built_in("idle"),
            "muted",
            Dim,
            vec![yes(Turn(Idle))],
        ),
        // Rule 22 of the plan: ships disabled, last, so the feature is
        // discoverable without false alarms. A user who enables it moves it up
        // (above the activity and idle rules) for it to affect agents.
        rule(
            SCREEN_EXAMPLE_RULE_ID,
            "Screen mentions an error",
            VisualKind::Glyph("!".to_owned()),
            "danger",
            Bold,
            vec![yes(Condition::ScreenMatches(ScreenPattern {
                pattern: "\\b(error|failed|panic)\\b".to_owned(),
                ignore_case: true,
                region: ScreenRegion::LastLines(20),
            }))],
        ),
    ];
    let rules = table
        .into_iter()
        .map(|entry| Rule {
            id: entry.id.to_owned(),
            name: entry.name.to_owned(),
            enabled: entry.id != SCREEN_EXAMPLE_RULE_ID,
            any_of: vec![AllOf {
                all_of: entry.terms,
            }],
            then: StatusVisual {
                kind: entry.kind,
                style: VisualStyle {
                    color: Some(entry.color.to_owned()),
                    bold: matches!(entry.weight, Weight::Bold),
                    dim: matches!(entry.weight, Weight::Dim),
                    pulse: PULSING_RULE_IDS.contains(&entry.id),
                },
            },
        })
        .collect();
    StatusRules {
        version: STATUS_RULES_SCHEMA_VERSION,
        defaults_version: DEFAULTS_VERSION,
        rules,
        fallback: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{default_rules, SCREEN_EXAMPLE_RULE_ID};
    use crate::status_rule_expression::{validate, VisualKind};

    #[test]
    fn defaults_have_unique_valid_ids_and_the_screen_example_is_disabled() {
        let rules = default_rules();
        assert_eq!(rules.rules.len(), 24);
        assert!(rules.fallback.is_none());
        assert!(
            validate(&rules).is_empty(),
            "defaults must produce no errors and no warnings: {:?}",
            validate(&rules)
        );
        let example = rules
            .rules
            .iter()
            .find(|rule| rule.id == SCREEN_EXAMPLE_RULE_ID);
        assert!(example.is_some_and(|rule| !rule.enabled));
        assert!(rules.rules.iter().filter(|rule| !rule.enabled).count() == 1);
        let example_index = rules
            .rules
            .iter()
            .position(|rule| rule.id == SCREEN_EXAMPLE_RULE_ID)
            .unwrap();
        let idle_index = rules
            .rules
            .iter()
            .position(|rule| rule.id == "idle")
            .unwrap();
        assert_eq!(
            idle_index, 22,
            "idle is rule 22 of the plan, after the split row 11 and the blocking-error rule"
        );
        assert_eq!(
            example_index, 23,
            "the disabled example (plan rule 23) is last"
        );
        assert_eq!(
            rules.rules.first().map(|rule| rule.id.as_str()),
            Some("approval")
        );
        assert_eq!(
            rules.rules.last().map(|rule| rule.id.as_str()),
            Some(SCREEN_EXAMPLE_RULE_ID)
        );
    }

    #[test]
    fn defaults_carry_the_planned_bold_and_dim_styles() {
        let rules = default_rules();
        let style = |id: &str| {
            &rules
                .rules
                .iter()
                .find(|rule| rule.id == id)
                .unwrap()
                .then
                .style
        };
        for bold in [
            "approval",
            "task-failed",
            "goal-blocked",
            "task-unread",
            "completion-unread",
            SCREEN_EXAMPLE_RULE_ID,
        ] {
            assert!(
                style(bold).bold && !style(bold).dim,
                "{bold} should be bold"
            );
        }
        assert!(style("idle").dim && !style("idle").bold);
        for rule in &rules.rules {
            assert!(
                rule.then.style.color.is_some(),
                "{} needs a theme colour",
                rule.id
            );
            assert!(
                !(rule.then.style.bold && rule.then.style.dim),
                "{} cannot be bold and dim",
                rule.id
            );
        }
        let plain: Vec<_> = rules
            .rules
            .iter()
            .filter(|rule| !rule.then.style.bold && !rule.then.style.dim)
            .map(|rule| rule.id.as_str())
            .collect();
        assert_eq!(plain.len(), 16, "{plain:?}");
    }

    #[test]
    fn defaults_use_the_planned_animated_visuals() {
        let rules = default_rules();
        let kind = |id: &str| {
            &rules
                .rules
                .iter()
                .find(|rule| rule.id == id)
                .unwrap()
                .then
                .kind
        };
        assert!(
            matches!(kind("parked-progress"), VisualKind::ProgressFrames { frames } if frames.len() == 5)
        );
        assert!(
            matches!(kind("working-goal"), VisualKind::BrailleAnimation { preset, frames_per_second: 8 } if preset == "spinner")
        );
        assert!(
            matches!(kind("shell-busy"), VisualKind::BrailleAnimation { preset, frames_per_second: 8 } if preset == "loop")
        );
        assert!(matches!(kind(SCREEN_EXAMPLE_RULE_ID), VisualKind::Glyph(glyph) if glyph == "!"));
        assert_eq!(kind("parked-progress-unsteady"), kind("parked-progress"));
    }

    #[test]
    fn only_the_look_at_me_now_rules_pulse_and_the_unsteady_monitor_is_tinted() {
        let rules = default_rules();
        let pulsing: Vec<&str> = rules
            .rules
            .iter()
            .filter(|rule| rule.then.style.pulse)
            .map(|rule| rule.id.as_str())
            .collect();
        assert_eq!(pulsing, ["approval", "completion-unread"]);
        let position = |id: &str| rules.rules.iter().position(|rule| rule.id == id).unwrap();
        assert_eq!(
            position("parked-progress-unsteady") + 1,
            position("parked-progress")
        );
        let tint = &rules.rules[position("parked-progress-unsteady")].then.style;
        assert_eq!(tint.color.as_deref(), Some("warning"));
    }
}
