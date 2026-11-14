//! Custom status rules evaluated against one live pane from the client's
//! retained state.
//!
//! This module owns the one definition of a live pane's client-side rule
//! facts ([`client_pane_facts`]): activity freshness, frozen screen, spend,
//! keyboard focus, retained blocking errors and the node's own fields. Three
//! consumers evaluate the rules and must agree on every fact: the tree renderer
//! (`tree_ui::pane_label_with_icons`, which sees a render context and not
//! `App`), the hover tooltip of the single Custom status slot and the redraw
//! scheduler (both through [`with_pane_facts`]), and the rules editor's
//! inspector. A new client fact is added here once.

use ilium_core::status_rule_expression::{
    explain_all, first_match, why, Observation, PaneFacts, RuleOutcome, StatusVisual,
    SupplementaryFacts,
};
use ilium_core::{Node, NodeId, NodeKind, PaneStatus};

use crate::app::App;
use crate::status_icons::TooltipContent;
use crate::status_rule_visual::{next_frame_delay, visual_description, ProgressCue};

/// Every rule's outcome for one pane, plus the winner the tree shows.
pub(crate) struct LivePaneVerdict<'a> {
    /// Zero-based index of the first enabled matching rule.
    pub rule_index: Option<usize>,
    /// One outcome per rule, in rule order, including disabled rules.
    pub outcomes: Vec<RuleOutcome>,
    /// The visual on screen: the winner's, else the fallback, else nothing.
    pub visual: Option<&'a StatusVisual>,
}

/// Where the user is looking: the tree cursor and the terminals on screen in
/// the right panel. Captured once per render so every pane row reads the
/// same view.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PaneViewFacts {
    /// The tree row under the cursor (`App::selected_node_id`).
    pub selected_node_id: Option<NodeId>,
    /// The right panel's terminals (`App::displayed_pane_slots`).
    pub displayed_panes: [Option<NodeId>; 4],
}

impl PaneViewFacts {
    pub(crate) fn from_app(app: &App) -> Self {
        Self {
            selected_node_id: app.selected_node_id(),
            displayed_panes: app.displayed_pane_slots(),
        }
    }
}

/// The client state a pane's rule facts are derived from, passed explicitly
/// so the tree renderer (render context) and the `App`-based consumers feed
/// the same derivation.
pub(crate) struct ClientPaneFactSources<'a> {
    /// The pane's tree node; supplies the bookmark and the prompt queue.
    pub node: &'a Node,
    /// The status the rules are evaluated against (the node's own status).
    pub status: &'a PaneStatus,
    /// Whether the user froze the pane's agent screen.
    pub frozen: bool,
    /// The agent session's spend in US dollars, when the cost tracker has it.
    pub session_cost_usd: Option<f64>,
    /// Fraction (0..=1) of the context window in use, when the cost tracker
    /// has read it from the transcript.
    pub context_fill: Option<f64>,
    /// The pane whose terminal holds keyboard focus (`App::focused_pane_id`).
    pub focused_pane_id: Option<NodeId>,
    /// The server's latest detection evidence for this pane, if any.
    pub detection_evidence: Option<&'a ilium_ipc::PaneDetectionEvidence>,
    /// Wall clock used to age the detection evidence.
    pub current_unix_millis: u64,
    /// Whether a retained blocking error targets exactly this pane
    /// (`App::pane_has_blocking_error`); `None` when not observed.
    pub blocking_error: Option<bool>,
    /// Whether the session has a retained session-level error
    /// (`App::session_has_blocking_error`); `None` when not observed.
    pub session_error: Option<bool>,
    /// The tree cursor and on-screen terminals; `None` when the caller has
    /// no view (previews, geometry-only renders), which leaves the
    /// `pane-selected` and `pane-visible` conditions unknown.
    pub view: Option<PaneViewFacts>,
}

/// The client-derived part of a pane's rule facts. The pane status, progress
/// monitors, scheduled input, shell output phase and screen matches come
/// straight from retained state and need no derivation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ClientPaneFacts {
    /// `None` when freshness was not observed, so stale rules stay unknown.
    pub activity_stale: Option<bool>,
    pub supplementary: SupplementaryFacts,
}

/// Derive the client facts every rule consumer uses. Spend is rounded to
/// whole cents (negative spend reads as zero); a missing spend stays unknown.
pub(crate) fn client_pane_facts(sources: ClientPaneFactSources<'_>) -> ClientPaneFacts {
    let ClientPaneFactSources {
        node,
        status,
        frozen,
        session_cost_usd,
        context_fill,
        focused_pane_id,
        detection_evidence,
        current_unix_millis,
        blocking_error,
        session_error,
        view,
    } = sources;
    let supplementary = SupplementaryFacts {
        frozen: Some(frozen),
        // Rounding then clamping keeps the cast in range for any real spend.
        session_cost_cents: session_cost_usd.map(|usd| (usd * 100.0).round().max(0.0) as u64),
        focused: Some(focused_pane_id == Some(node.id)),
        blocking_error,
        session_error,
        selected: view.map(|view| view.selected_node_id == Some(node.id)),
        visible: view.map(|view| view.displayed_panes.contains(&Some(node.id))),
        // Clamped to the window and rounded to tenths of a percent; a
        // non-finite fill (corrupt transcript figures) stays unknown.
        context_permille: context_fill
            .filter(|fill| fill.is_finite())
            .map(|fill| (fill.clamp(0.0, 1.0) * 1000.0).round() as u16),
        ..SupplementaryFacts::default()
    }
    .with_node(node);
    ClientPaneFacts {
        activity_stale: crate::status_icons::detection_activity_stale(
            status,
            detection_evidence,
            current_unix_millis,
        ),
        supplementary,
    }
}

/// Build the pane's facts from `App` once and hand them to `consumer`.
/// `None` when the node is not a pane. The screen closure defers to the
/// connection-scoped observations, which answer `Unknown` for an unwatched
/// pane.
pub(crate) fn with_pane_facts<T>(
    app: &App,
    node_id: NodeId,
    consumer: impl FnOnce(&PaneFacts<'_>) -> T,
) -> Option<T> {
    let node = app.tree.get(node_id)?;
    let NodeKind::Pane {
        status,
        progress_monitors,
        ..
    } = &node.kind
    else {
        return None;
    };
    let shell_output = app
        .terminal_activity
        .phase(node_id, app.started_at.elapsed().as_millis())
        .map(crate::tree_ui::shell_output_phase);
    let cost_overlay = app.cost_tracker.overlay();
    let pane_cost = cost_overlay.details.get(&node_id);
    let client = client_pane_facts(ClientPaneFactSources {
        node,
        status,
        frozen: app.frozen_panes.contains(&node_id),
        session_cost_usd: pane_cost.map(|cost| cost.usd),
        context_fill: pane_cost.and_then(|cost| cost.context_fill),
        focused_pane_id: app.focused_pane_id(),
        detection_evidence: app.pane_detection_evidence.get(&node_id),
        current_unix_millis: crate::wall_clock::unix_millis_now(),
        blocking_error: Some(app.pane_has_blocking_error(node_id)),
        session_error: Some(app.session_has_blocking_error()),
        view: Some(PaneViewFacts::from_app(app)),
    });
    let observations = &app.custom_status_screen_observations;
    let screen_matches = |pattern: &_| observations.observe(node_id, pattern);
    let facts = PaneFacts {
        status,
        progress: progress_monitors,
        scheduled_input: false,
        shell_output,
        activity_stale: client.activity_stale,
        supplementary: client.supplementary,
        screen_matches: &screen_matches,
    };
    Some(consumer(&facts))
}

/// The visual the tree shows for this pane, without explaining it, with the
/// pane's progress cue (unmeasured progress frames pulse, measured ones do
/// not). Cheap enough for the redraw scheduler to call on every maintenance
/// tick.
pub(crate) fn winning_visual(app: &App, node_id: NodeId) -> Option<(&StatusVisual, ProgressCue)> {
    let rules = &app.custom_status_rules;
    with_pane_facts(app, node_id, |facts| {
        first_match(&rules.rules, |condition| condition.observe(facts))
            .map(|rule| &rule.then)
            .or(rules.fallback.as_ref())
            .map(|visual| (visual, ProgressCue::from_monitors(facts.progress)))
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
    let visuals: Vec<(&StatusVisual, ProgressCue)> = app
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
            winning_visual(&app, pane_id).map(|(visual, _)| visual),
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
    fn client_facts_cover_focus_spend_node_fields_and_unknown_freshness() {
        use super::{client_pane_facts, ClientPaneFactSources};

        let (app, pane_id) = app_with_pane(PaneStatus::PlainShell);
        let node = app.tree.get(pane_id).unwrap();
        let sources = |focused_pane_id, session_cost_usd| ClientPaneFactSources {
            node,
            status: &PaneStatus::PlainShell,
            frozen: true,
            session_cost_usd,
            context_fill: None,
            focused_pane_id,
            detection_evidence: None,
            current_unix_millis: 1_000,
            blocking_error: Some(true),
            session_error: None,
            view: None,
        };
        let facts = client_pane_facts(sources(Some(pane_id), Some(1.2349)));
        assert_eq!(facts.supplementary.focused, Some(true));
        assert_eq!(facts.supplementary.frozen, Some(true));
        assert_eq!(facts.supplementary.session_cost_cents, Some(123));
        assert_eq!(facts.supplementary.bookmarked, Some(false));
        assert_eq!(facts.supplementary.queued_prompts, None);
        // Error facts are copied through unchanged; unobserved stays None.
        assert_eq!(facts.supplementary.blocking_error, Some(true));
        assert_eq!(facts.supplementary.session_error, None);
        // No detection evidence: freshness stays unknown, never "fresh".
        assert_eq!(facts.activity_stale, None);
        // Focus elsewhere or on no pane at all is an observed false.
        let elsewhere = client_pane_facts(sources(Some(NodeId(999_999)), Some(-0.4)));
        assert_eq!(elsewhere.supplementary.focused, Some(false));
        assert_eq!(elsewhere.supplementary.session_cost_cents, Some(0));
        let unfocused = client_pane_facts(sources(None, None));
        assert_eq!(unfocused.supplementary.focused, Some(false));
        assert_eq!(unfocused.supplementary.session_cost_cents, None);
        // Without a view and a context figure, those facts stay unknown.
        assert_eq!(unfocused.supplementary.selected, None);
        assert_eq!(unfocused.supplementary.visible, None);
        assert_eq!(unfocused.supplementary.context_permille, None);
        assert_eq!(unfocused.supplementary.prompt_queue_held, None);
    }

    #[test]
    fn client_facts_read_selection_visibility_and_context_fill() {
        use super::{client_pane_facts, ClientPaneFactSources, PaneViewFacts};

        let (app, pane_id) = app_with_pane(PaneStatus::PlainShell);
        let node = app.tree.get(pane_id).unwrap();
        let sources = |view, context_fill| ClientPaneFactSources {
            node,
            status: &PaneStatus::PlainShell,
            frozen: false,
            session_cost_usd: None,
            context_fill,
            focused_pane_id: None,
            detection_evidence: None,
            current_unix_millis: 1_000,
            blocking_error: None,
            session_error: None,
            view: Some(view),
        };
        let other = NodeId(999_999);
        let looking = PaneViewFacts {
            selected_node_id: Some(pane_id),
            displayed_panes: [Some(other), Some(pane_id), None, None],
        };
        let facts = client_pane_facts(sources(looking, Some(0.8004)));
        assert_eq!(facts.supplementary.selected, Some(true));
        assert_eq!(facts.supplementary.visible, Some(true));
        assert_eq!(facts.supplementary.context_permille, Some(800));

        // Nothing selected and an empty right panel are observed falses.
        let away = client_pane_facts(sources(PaneViewFacts::default(), Some(1.7)));
        assert_eq!(away.supplementary.selected, Some(false));
        assert_eq!(away.supplementary.visible, Some(false));
        // A fill above the window is clamped; a corrupt one stays unknown.
        assert_eq!(away.supplementary.context_permille, Some(1000));
        let corrupt = client_pane_facts(sources(PaneViewFacts::default(), Some(f64::NAN)));
        assert_eq!(corrupt.supplementary.context_permille, None);
        let negative = client_pane_facts(sources(PaneViewFacts::default(), Some(-0.2)));
        assert_eq!(negative.supplementary.context_permille, Some(0));
    }

    #[test]
    fn live_facts_observe_the_tree_cursor_and_the_right_panel() {
        use ilium_core::status_rule_expression::Observation;

        let (mut app, pane_id) = app_with_pane(PaneStatus::PlainShell);
        let observe = |app: &App| {
            super::with_pane_facts(app, pane_id, |facts| {
                (
                    Condition::PaneSelected.observe(facts),
                    Condition::PaneVisible.observe(facts),
                )
            })
        };
        // The live builder always observes the view: an empty right panel
        // is a known false, never unknown.
        app.right_panel_target = crate::app::RightPanelTarget::Empty;
        let (_, visible) = observe(&app).unwrap();
        assert_eq!(visible, Observation::False);

        app.select_node(pane_id);
        app.right_panel_target = crate::app::RightPanelTarget::Pane { pane_id };
        assert_eq!(observe(&app), Some((Observation::True, Observation::True)));
    }

    #[test]
    fn live_facts_observe_focus_as_false_while_the_tree_has_focus() {
        use ilium_core::status_rule_expression::Observation;

        let (mut app, pane_id) = app_with_pane(PaneStatus::PlainShell);
        app.focus = crate::app::FocusTarget::Tree;
        assert_eq!(app.focused_pane_id(), None);
        let observed =
            super::with_pane_facts(&app, pane_id, |facts| Condition::PaneFocused.observe(facts));
        assert_eq!(observed, Some(Observation::False));
    }

    #[test]
    fn live_facts_separate_pane_errors_from_session_errors() {
        use ilium_core::status_rule_expression::Observation;

        let (mut app, pane_id) = app_with_pane(PaneStatus::PlainShell);
        let observe = |app: &App| {
            super::with_pane_facts(app, pane_id, |facts| {
                (
                    Condition::BlockingError.observe(facts),
                    Condition::SessionError.observe(facts),
                )
            })
        };
        assert_eq!(
            observe(&app),
            Some((Observation::False, Observation::False))
        );
        // A session-level error is a session fact, never this pane's error.
        app.replace_blocking_errors(vec![ilium_ipc::BlockingError {
            node_id: None,
            operation: "session".to_owned(),
            message: "snapshot write failed".to_owned(),
        }]);
        assert_eq!(observe(&app), Some((Observation::False, Observation::True)));
        app.record_local_blocking_error(pane_id, "Send input", "pipe closed");
        assert_eq!(observe(&app), Some((Observation::True, Observation::True)));
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
    fn a_matched_pulsing_rule_wakes_the_clock_at_the_pulse_rate() {
        let (mut app, _) = app_with_pane(PaneStatus::from_activity(
            AgentClass::Codex,
            AgentActivity::WaitingApproval,
            None,
        ));
        // The shipped approval rule is a static built-in icon that pulses.
        app.ui_settings.motion_level = MotionLevel::Full;
        assert_eq!(animation_frame_delay(&app, 0), Some(500));
        assert_eq!(animation_frame_delay(&app, 600), Some(400));
        app.ui_settings.motion_level = MotionLevel::Reduced;
        assert_eq!(animation_frame_delay(&app, 0), Some(1_000));
        app.ui_settings.motion_level = MotionLevel::Off;
        assert_eq!(animation_frame_delay(&app, 0), None);
        // Without the pulse the same icon needs no redraws at all.
        for rule in &mut app.custom_status_rules.rules {
            rule.then.style.pulse = false;
        }
        app.ui_settings.motion_level = MotionLevel::Full;
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
