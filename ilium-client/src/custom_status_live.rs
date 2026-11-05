//! Custom status rules evaluated against one live pane from the client's
//! retained state.
//!
//! The tree renderer (`tree_ui::pane_label_with_icons`) evaluates the rules
//! from its render context and cannot see `App`. Two consumers need the same
//! verdict outside a render pass: the hover tooltip of the single Custom
//! status slot (which rule won and why) and the redraw scheduler (which
//! animations are actually on screen). Both derive the pane facts here, with
//! the same shared helpers the renderer uses (`shell_output_phase`,
//! `detection_activity_stale`, `supplementary_facts`), so the two paths cannot
//! disagree about a fact.

use ilium_core::status_rule_expression::{
    explain_all, first_match, why, Observation, PaneFacts, RuleOutcome, StatusVisual,
};
use ilium_core::{NodeId, NodeKind};

use crate::app::App;
use crate::status_icons::TooltipContent;
use crate::status_rule_visual::{next_frame_delay, supplementary_facts, visual_description};

/// Every rule's outcome for one pane, plus the winner the tree shows.
pub(crate) struct LivePaneVerdict<'a> {
    /// Zero-based index of the first enabled matching rule.
    pub rule_index: Option<usize>,
    /// One outcome per rule, in rule order, including disabled rules.
    pub outcomes: Vec<RuleOutcome>,
    /// The visual on screen: the winner's, else the fallback, else nothing.
    pub visual: Option<&'a StatusVisual>,
}

/// Build the pane's facts once and hand them to `consumer`. `None` when the
/// node is not a pane. The screen closure defers to the connection-scoped
/// observations, which answer `Unknown` for an unwatched pane.
fn with_pane_facts<T>(
    app: &App,
    node_id: NodeId,
    consumer: impl FnOnce(&PaneFacts<'_>) -> T,
) -> Option<T> {
    let node = app.tree.get(node_id)?;
    let NodeKind::Pane {
        status,
        progress_monitors,
        scheduled_input,
        ..
    } = &node.kind
    else {
        return None;
    };
    let shell_output = app
        .terminal_activity
        .phase(node_id, app.started_at.elapsed().as_millis())
        .map(crate::tree_ui::shell_output_phase);
    let activity_stale = crate::status_icons::detection_activity_stale(
        status,
        app.pane_detection_evidence.get(&node_id),
        crate::scheduled_input::unix_millis_now(),
    );
    let supplementary = supplementary_facts(
        app.frozen_panes.contains(&node_id),
        app.cost_tracker
            .overlay()
            .details
            .get(&node_id)
            .map(|cost| cost.usd),
    );
    let observations = &app.custom_status_screen_observations;
    let screen_matches = |pattern: &_| observations.observe(node_id, pattern);
    let facts = PaneFacts {
        status,
        progress: progress_monitors,
        scheduled_input: scheduled_input.is_some(),
        shell_output,
        activity_stale,
        supplementary,
        screen_matches: &screen_matches,
    };
    Some(consumer(&facts))
}

/// The visual the tree shows for this pane, without explaining it. Cheap
/// enough for the redraw scheduler to call on every maintenance tick.
pub(crate) fn winning_visual(app: &App, node_id: NodeId) -> Option<&StatusVisual> {
    let rules = &app.custom_status_rules;
    with_pane_facts(app, node_id, |facts| {
        first_match(&rules.rules, |condition| condition.observe(facts))
            .map(|rule| &rule.then)
            .or(rules.fallback.as_ref())
    })
    .flatten()
}

/// Every rule's outcome for the pane, for the hover explanation.
pub(crate) fn evaluate(app: &App, node_id: NodeId) -> Option<LivePaneVerdict<'_>> {
    let rules = &app.custom_status_rules;
    with_pane_facts(app, node_id, |facts| {
        let outcomes = explain_all(&rules.rules, |condition| condition.observe(facts));
        let rule_index = outcomes
            .iter()
            .position(|outcome| outcome.enabled && outcome.matched);
        let visual = rule_index
            .map(|index| &rules.rules[index].then)
            .or(rules.fallback.as_ref());
        LivePaneVerdict {
            rule_index,
            outcomes,
            visual,
        }
    })
}

/// Hover text for the Custom status slot. Always `Some` for a pane, including
/// when nothing matched: the slot is still on screen and the hit test treats
/// a slot without tooltip as absent.
pub(crate) fn tooltip(app: &App, node_id: NodeId) -> Option<TooltipContent> {
    let verdict = evaluate(app, node_id)?;
    let rules = &app.custom_status_rules;
    let shown = verdict.visual.map(visual_description);
    if let Some(index) = verdict.rule_index {
        let rule = &rules.rules[index];
        return Some(TooltipContent {
            title: crate::status_icons::safe_tooltip_text(&rule.name),
            body: format!("Shows {}.", shown.unwrap_or_default()),
            reason: Some(format!(
                "Why: {}.",
                crate::status_icons::safe_tooltip_text(&why(
                    index + 1,
                    rule,
                    verdict.outcomes.get(index)
                ))
            )),
        });
    }
    let enabled = rules.rules.iter().filter(|rule| rule.enabled).count();
    let unobserved = rules
        .rules
        .iter()
        .zip(&verdict.outcomes)
        .filter(|(rule, outcome)| {
            rule.enabled
                && outcome.groups.iter().any(|group| {
                    group
                        .terms
                        .iter()
                        .any(|term| term.observation == Observation::Unknown)
                })
        })
        .count();
    let body = match shown {
        Some(fallback) => format!("Shows the fallback: {fallback}."),
        None => "The status slot stays empty.".to_owned(),
    };
    let mut reason = format!(
        "Why: none of the {enabled} enabled custom rule{} matched this pane",
        if enabled == 1 { "" } else { "s" }
    );
    if unobserved > 0 {
        reason.push_str(&format!(
            "; {unobserved} rule{} depend{} on a fact that was not observed",
            if unobserved == 1 { "" } else { "s" },
            if unobserved == 1 { "s" } else { "" }
        ));
    }
    reason.push('.');
    Some(TooltipContent {
        title: "No custom rule matched".to_owned(),
        body,
        reason: Some(reason),
    })
}

/// Time until the next frame of an animation that is actually on screen:
/// only visuals matched by a pane in the tree count, so an idle tree under
/// Custom mode schedules no redraws even when the rule set contains
/// animations. `None` when nothing animates or motion is Off.
pub(crate) fn animation_frame_delay(app: &App, elapsed_millis: u128) -> Option<u64> {
    let motion = app.ui_settings.motion_level;
    if motion == crate::config::MotionLevel::Off {
        return None;
    }
    let visuals: Vec<&StatusVisual> = app
        .tree
        .panes()
        .filter_map(|node| winning_visual(app, node.id))
        .collect();
    next_frame_delay(visuals, motion, elapsed_millis)
}

#[cfg(test)]
mod tests {
    use super::{animation_frame_delay, evaluate, tooltip, winning_visual};
    use crate::app::App;
    use crate::config::MotionLevel;
    use ilium_core::status_rule_expression::{
        default_rules, AllOf, Condition, ConditionTerm, Rule, StatusVisual, VisualKind, VisualStyle,
    };
    use ilium_core::{
        AgentActivity, AgentClass, GoalState, NodeId, PaneContentKind, PaneStatus, ROOT_ID,
    };

    fn app_with_pane(status: PaneStatus) -> (App, NodeId) {
        let mut app = App::new("custom-status-live".to_owned(), std::env::temp_dir());
        app.ui_settings.agent_monitoring_mode =
            crate::agent_monitoring::AgentMonitoringMode::Custom;
        let group_id = app.tree.add_group(ROOT_ID, "agents").unwrap();
        let pane_id = app
            .tree
            .add_pane(group_id, "agent", PaneContentKind::Terminal)
            .unwrap();
        app.tree.set_pane_status(pane_id, status).unwrap();
        (app, pane_id)
    }

    #[test]
    fn winner_and_tooltip_name_the_matching_default_rule() {
        let (app, pane_id) = app_with_pane(PaneStatus::from_activity(
            AgentClass::Codex,
            AgentActivity::WaitingApproval,
            None,
        ));
        let verdict = evaluate(&app, pane_id).unwrap();
        let index = verdict.rule_index.unwrap();
        assert_eq!(app.custom_status_rules.rules[index].id, "approval");
        assert_eq!(
            winning_visual(&app, pane_id),
            Some(&app.custom_status_rules.rules[index].then)
        );
        let hover = tooltip(&app, pane_id).unwrap();
        assert_eq!(hover.title, app.custom_status_rules.rules[index].name);
        assert!(
            hover.body.starts_with("Shows the built-in "),
            "{}",
            hover.body
        );
        let reason = hover.reason.unwrap();
        assert!(
            reason.starts_with(&format!("Why: Custom rule {} ", index + 1)),
            "{reason}"
        );
        assert!(reason.contains("agent needs approval"), "{reason}");
    }

    #[test]
    fn no_match_still_yields_a_tooltip_and_counts_unobserved_rules() {
        let (mut app, pane_id) = app_with_pane(PaneStatus::PlainShell);
        app.custom_status_rules.rules = vec![Rule {
            id: "screen".to_owned(),
            name: "Screen says error".to_owned(),
            enabled: true,
            any_of: vec![AllOf {
                all_of: vec![ConditionTerm {
                    condition: Condition::ScreenMatches(
                        ilium_core::status_rule_expression::ScreenPattern {
                            pattern: "error".to_owned(),
                            ignore_case: true,
                            region: ilium_core::status_rule_expression::ScreenRegion::All,
                        },
                    ),
                    negated: false,
                }],
            }],
            then: StatusVisual {
                kind: VisualKind::Glyph("!".to_owned()),
                style: VisualStyle::default(),
            },
        }];
        app.custom_status_rules.fallback = None;
        assert!(winning_visual(&app, pane_id).is_none());
        let hover = tooltip(&app, pane_id).unwrap();
        assert_eq!(hover.title, "No custom rule matched");
        assert_eq!(hover.body, "The status slot stays empty.");
        assert_eq!(
            hover.reason.as_deref(),
            Some("Why: none of the 1 enabled custom rule matched this pane; 1 rule depends on a fact that was not observed.")
        );
        app.custom_status_rules.fallback = Some(StatusVisual {
            kind: VisualKind::Glyph("·".to_owned()),
            style: VisualStyle::default(),
        });
        assert_eq!(
            tooltip(&app, pane_id).unwrap().body,
            "Shows the fallback: the glyph ·."
        );
    }

    #[test]
    fn non_pane_nodes_have_no_verdict() {
        let (app, _) = app_with_pane(PaneStatus::PlainShell);
        assert!(evaluate(&app, ROOT_ID).is_none());
        assert!(tooltip(&app, ROOT_ID).is_none());
        assert!(winning_visual(&app, ROOT_ID).is_none());
    }

    #[test]
    fn redraws_follow_matched_animations_not_the_rule_set() {
        let (mut app, pane_id) = app_with_pane(PaneStatus::from_activity(
            AgentClass::Codex,
            AgentActivity::Idle,
            None,
        ));
        assert_eq!(app.custom_status_rules, default_rules());
        // Motion defaults to Reduced; the frame maths below assume Full.
        app.ui_settings.motion_level = MotionLevel::Full;
        // The defaults contain two animated rules, but the idle agent matches
        // neither, so nothing is scheduled.
        assert_eq!(animation_frame_delay(&app, 0), None);
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(
                    AgentClass::Codex,
                    AgentActivity::Working,
                    Some(GoalState::Active),
                ),
            )
            .unwrap();
        assert_eq!(animation_frame_delay(&app, 0), Some(125));
        assert_eq!(animation_frame_delay(&app, 100), Some(25));
        app.ui_settings.motion_level = MotionLevel::Reduced;
        assert_eq!(animation_frame_delay(&app, 0), Some(500));
        app.ui_settings.motion_level = MotionLevel::Off;
        assert_eq!(animation_frame_delay(&app, 0), None);
    }

    #[test]
    fn empty_rule_set_schedules_nothing() {
        let (mut app, _) = app_with_pane(PaneStatus::from_activity(
            AgentClass::Codex,
            AgentActivity::Working,
            Some(GoalState::Active),
        ));
        app.custom_status_rules.rules.clear();
        app.custom_status_rules.fallback = None;
        assert_eq!(animation_frame_delay(&app, 0), None);
        assert!(winning_visual(&app, NodeId(999_999)).is_none());
    }
}
