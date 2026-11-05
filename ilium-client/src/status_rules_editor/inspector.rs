//! Live-session facts for the rules editor: which rule picks the focused
//! pane's icon and why, and how many panes a rule matches right now.
//!
//! Facts are assembled exactly as the tree renderer assembles them for the
//! status column (`tree_ui`), so the explanation never disagrees with the
//! icon the tree shows. The evaluation itself is `ilium_core`'s
//! `explain_all`/`why`; this module only gathers client-side observations.

use ilium_core::status_rule_expression::{
    explain_all, why, Condition, Observation, PaneFacts, Rule, RuleOutcome, StatusVisual,
};
use ilium_core::{Node, NodeId, NodeKind};

use crate::app::App;

/// Why the focused pane shows its icon, ready for the preview column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneExplanation {
    pub pane_name: String,
    /// Index of the winning rule, `None` when the fallback icon applies.
    pub winner: Option<usize>,
    /// One sentence per line: the verdict, then the winning expression.
    pub lines: Vec<String>,
}

/// Evaluate every configured rule for one pane, in list order.
fn outcomes_for(app: &App, node: &Node) -> Option<Vec<RuleOutcome>> {
    outcomes_for_rules(app, node, &app.custom_status_rules.rules)
}

/// The focused pane's explanation, or `None` without a focused pane.
pub fn focused_pane_explanation(app: &App) -> Option<PaneExplanation> {
    let pane_id = app.focused_pane_id()?;
    pane_explanation(app, pane_id)
}

pub fn pane_explanation(app: &App, pane_id: NodeId) -> Option<PaneExplanation> {
    let node = app.tree.get(pane_id)?;
    let outcomes = outcomes_for(app, node)?;
    let winner = outcomes
        .iter()
        .position(|outcome| outcome.enabled && outcome.matched);
    let mut lines = Vec::new();
    match winner {
        Some(index) => {
            let rule = &app.custom_status_rules.rules[index];
            lines.push(format!("Rule {} wins: {}", index + 1, rule.name));
            lines.push(why(index + 1, rule, Some(&outcomes[index])));
        }
        None => {
            lines.push("No enabled rule matches: the fallback icon applies.".to_owned());
            let skipped = outcomes
                .iter()
                .filter(|outcome| !outcome.enabled && outcome.matched)
                .count();
            if skipped > 0 {
                lines.push(format!(
                    "{skipped} disabled rule{} would match if enabled.",
                    if skipped == 1 { "" } else { "s" }
                ));
            }
        }
    }
    Some(PaneExplanation {
        pane_name: node.name.clone(),
        winner,
        lines,
    })
}

/// How many panes of the session `rule` matches on its own, regardless of
/// its position in the list. Disabled rules are counted too, so the editor
/// can show what enabling one would affect.
pub fn matching_pane_count(app: &App, rule: &Rule<Condition, StatusVisual>) -> usize {
    let probe = [rule.clone()].map(|mut rule| {
        rule.enabled = true;
        rule
    });
    app.tree
        .panes()
        .filter(|node| {
            let Some(mut outcomes) = outcomes_for_rules(app, node, &probe) else {
                return false;
            };
            outcomes.pop().is_some_and(|outcome| outcome.matched)
        })
        .count()
}

/// Gather the pane's facts the way the tree renderer does and evaluate
/// `rules` against them.
fn outcomes_for_rules(
    app: &App,
    node: &Node,
    rules: &[Rule<Condition, StatusVisual>],
) -> Option<Vec<RuleOutcome>> {
    let NodeKind::Pane {
        status,
        scheduled_input,
        progress_monitors,
        ..
    } = &node.kind
    else {
        return None;
    };
    let now_unix_millis = crate::scheduled_input::unix_millis_now();
    let elapsed_ms = app.started_at.elapsed().as_millis();
    let shell_output = app
        .terminal_activity
        .phase(node.id, elapsed_ms)
        .map(crate::tree_ui::shell_output_phase);
    let overlay = app.cost_tracker.overlay();
    let facts = PaneFacts {
        status,
        progress: progress_monitors,
        scheduled_input: scheduled_input.is_some(),
        shell_output,
        activity_stale: crate::status_icons::detection_activity_stale(
            status,
            app.pane_detection_evidence.get(&node.id),
            now_unix_millis,
        ),
        supplementary: crate::status_rule_visual::supplementary_facts(
            app.frozen_panes.contains(&node.id),
            overlay.details.get(&node.id).map(|cost| cost.usd),
        ),
        screen_matches: &|pattern| -> Observation {
            app.custom_status_screen_observations
                .observe(node.id, pattern)
        },
    };
    Some(explain_all(rules, |condition| condition.observe(&facts)))
}

#[cfg(test)]
mod tests {
    use super::{focused_pane_explanation, matching_pane_count};
    use ilium_core::status_rule_expression::{
        default_rules, AllOf, Condition, ConditionTerm, Rule, StatusVisual, VisualKind, VisualStyle,
    };

    fn always_rule() -> Rule<Condition, StatusVisual> {
        Rule {
            id: "probe".into(),
            name: "Probe".into(),
            enabled: false,
            any_of: vec![AllOf {
                all_of: vec![ConditionTerm {
                    condition: Condition::Always,
                    negated: false,
                }],
            }],
            then: StatusVisual {
                kind: VisualKind::Glyph("●".into()),
                style: VisualStyle::default(),
            },
        }
    }

    #[test]
    fn empty_session_has_no_explanation_and_no_matches() {
        let app = crate::app::App::new("inspector".into(), std::env::temp_dir());
        assert_eq!(focused_pane_explanation(&app), None);
        assert_eq!(matching_pane_count(&app, &always_rule()), 0);
    }

    #[test]
    fn match_count_ignores_enabled_flag_and_counts_every_pane() {
        let mut app = crate::app::App::new("inspector".into(), std::env::temp_dir());
        app.custom_status_rules = default_rules();
        let group = app.tree.add_group(ilium_core::ROOT_ID, "Project").unwrap();
        let terminal = ilium_core::PaneContentKind::Terminal;
        app.tree.add_pane(group, "one", terminal).unwrap();
        app.tree.add_pane(group, "two", terminal).unwrap();
        assert_eq!(matching_pane_count(&app, &always_rule()), 2);
        let never = Rule {
            any_of: vec![AllOf {
                all_of: vec![ConditionTerm {
                    condition: Condition::Always,
                    negated: true,
                }],
            }],
            ..always_rule()
        };
        assert_eq!(matching_pane_count(&app, &never), 0);
    }
}
