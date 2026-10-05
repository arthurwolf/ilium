//! The three branches of the pick rule and the grid-limit flag, on synthetic
//! simulation tables with hand-chosen costs (no replay involved).
//!
//! Twelve identical sessions; per-session cost at the grid
//! 100k / 150k / 200k / 250k is `[1000, 1010, 1100, 1300]` (weighted tokens),
//! so the argmin is 100k and the 2% band (cost <= 1020) is {100k, 150k}.

use ilium_compaction_analysis::optimize::{
    optimize, ObservedCompactions, OptimizeConfig, PickRule, Recommendation,
};
use ilium_compaction_analysis::replay::{
    DrawPoolInfo, DrawSource, ReworkAccounting, ReworkModel, SessionSim, SimTable, TriggerOutcome,
};
use ilium_compaction_analysis::semantics::AgentSemantics;
use ilium_compaction_analysis::trace::{CompactionEvent, CompactionTrigger, SessionTrace};
use ilium_compaction_analysis::AgentKind;

const GRID: [u32; 4] = [100_000, 150_000, 200_000, 250_000];

fn table(costs: [f64; 4]) -> SimTable {
    let session = |index: usize| SessionSim {
        trace_index: index,
        model_family: "sonnet".to_string(),
        dominant_model: "claude-sonnet-5-5".to_string(),
        turns: 500,
        max_context_tokens: 400_000,
        negative_growth_steps: 0,
        measured_cost: 0.0,
        measured_usd: None,
        observed_compactions: 0,
        no_compaction_cost: 0.0,
        rework_usd_per_weighted_token: None,
        outcomes: costs
            .iter()
            .map(|&cost| TriggerOutcome {
                cost,
                usd: None,
                compactions: 0.0,
            })
            .collect(),
    };
    SimTable {
        agent: AgentKind::ClaudeCode,
        triggers: GRID.to_vec(),
        rework_accounting: ReworkAccounting::Absolute,
        rework: ReworkModel::prior_for(AgentKind::ClaudeCode),
        draw_pool: DrawPoolInfo {
            source: DrawSource::RecentRegime,
            events: 1,
        },
        sessions: (0..12).map(session).collect(),
        sessions_skipped: 0,
        warnings: Vec::new(),
    }
}

fn recommend(costs: [f64; 4], observed_sizes: &[u32]) -> Recommendation {
    optimize(
        &table(costs),
        &ObservedCompactions::from_sizes(observed_sizes.iter().copied()),
        &AgentSemantics::default_for(AgentKind::ClaudeCode),
        &OptimizeConfig::for_agent(AgentKind::ClaudeCode),
    )
    .unwrap()
}

const FALLING: [f64; 4] = [1_000.0, 1_010.0, 1_100.0, 1_300.0];

#[test]
fn rule_a_no_support_anywhere_is_the_plain_argmin_flagged_extrapolated() {
    // Every observed compaction fired at 567k: no candidate has support.
    let recommendation = recommend(FALLING, &[567_000; 40]);
    assert_eq!(recommendation.pick_rule, PickRule::PlainArgmin);
    assert_eq!(recommendation.trigger_tokens, 100_000);
    assert!(recommendation.extrapolated);
    assert_eq!(recommendation.unsupported_argmin_tokens, None);
    assert_eq!(recommendation.unsupported_relative_saving, None);
    assert_eq!(recommendation.observed_support_at_pick, 0.0);
    assert!(recommendation.basis.contains("0% of observed compactions"));
    // Argmin is the lowest grid point.
    assert!(recommendation.grid_floor_limited);
}

#[test]
fn rule_b_lowest_band_member_with_support() {
    // Support starts at 150k (all compactions at or below it), so the band
    // {100k, 150k} intersects the supported set {150k, 200k, 250k} at 150k.
    let recommendation = recommend(FALLING, &[140_000; 40]);
    assert_eq!(recommendation.pick_rule, PickRule::BandWithSupport);
    assert_eq!(recommendation.trigger_tokens, 150_000);
    assert!(!recommendation.extrapolated);
    assert_eq!(recommendation.bands.argmin_tokens, 100_000);
    assert_eq!(
        recommendation.bands.within_2_percent.members,
        vec![100_000, 150_000]
    );
    assert_eq!(recommendation.unsupported_argmin_tokens, None);
    assert_eq!(recommendation.unsupported_relative_saving, None);
}

#[test]
fn rule_c_cheapest_supported_candidate_with_the_unsupported_argmin_reported() {
    // Supported set {200k, 250k}; the band {100k, 150k} has no support.
    let recommendation = recommend(
        FALLING,
        &[190_000; 20]
            .iter()
            .chain(&[195_000; 20])
            .copied()
            .collect::<Vec<_>>(),
    );
    assert_eq!(recommendation.pick_rule, PickRule::SupportedOptimum);
    assert_eq!(recommendation.trigger_tokens, 200_000);
    assert!(!recommendation.extrapolated);
    assert_eq!(recommendation.observed_support_at_pick, 1.0);
    assert_eq!(recommendation.unsupported_argmin_tokens, Some(100_000));
    // 1 - 1000 / 1100 per session, identical across sessions.
    let saving = recommendation.unsupported_relative_saving.unwrap();
    assert!(
        (saving - (1.0 - 1_000.0 / 1_100.0)).abs() < 1e-12,
        "{saving}"
    );
    assert!(recommendation.basis.contains("supported optimum"));
    assert!(recommendation.warnings.iter().any(|warning| warning
        .contains("cost keeps falling below 200000 tokens")
        && warning.contains("no observed compactions there")));
    // The pick is judged against the best supported candidate in the
    // bootstrap, so it is stable even though it is 10% above the argmin.
    assert_eq!(recommendation.bootstrap.pick_within_band_share, 1.0);
    assert_ne!(
        recommendation.confidence,
        ilium_compaction_analysis::optimize::Confidence::High
    );
    // Setting follows the pick (Claude offset 33k).
    assert_eq!(recommendation.setting_value, 233_000);
}

#[test]
fn grid_limit_flag_marks_both_edges_and_clears_inside_the_grid() {
    let interior = recommend([1_100.0, 1_000.0, 1_050.0, 1_300.0], &[140_000; 40]);
    assert!(!interior.grid_floor_limited);
    assert_eq!(interior.bands.argmin_tokens, 150_000);
    assert!(!interior
        .warnings
        .iter()
        .any(|warning| warning.contains("widen the grid")));

    let ceiling = recommend([1_300.0, 1_200.0, 1_100.0, 1_000.0], &[140_000; 40]);
    assert!(ceiling.grid_floor_limited);
    assert!(ceiling
        .warnings
        .iter()
        .any(|warning| warning.contains("highest simulated grid point")
            && warning.contains("widen the grid")));

    let floor = recommend(FALLING, &[567_000; 40]);
    assert!(floor
        .warnings
        .iter()
        .any(|warning| warning.contains("lowest simulated grid point")));
}

fn trace_with_compaction(is_subagent: bool, pre_tokens: u32) -> SessionTrace {
    let mut trace = SessionTrace::empty(AgentKind::ClaudeCode, is_subagent);
    trace.compactions.push(CompactionEvent {
        timestamp_ms: 1,
        turn_index: 0,
        trigger: CompactionTrigger::Auto,
        pre_tokens,
        logged_post_tokens: 0,
        last_pre_context_tokens: 0,
        post_measured: false,
        post_context_tokens: 0,
        post_input_tokens: 0,
        post_cache_read_tokens: 0,
        post_cache_write_5m_tokens: 0,
        post_cache_write_1h_tokens: 0,
        summary_tokens: 0,
        summary_estimated: false,
        duration_ms: 0,
        precomputed: false,
        replacement_history_tokens: 0,
        model: 0,
        request_turn_index: None,
    });
    trace
}

#[test]
fn support_can_be_computed_from_main_sessions_only() {
    let traces = vec![
        trace_with_compaction(false, 200_000),
        trace_with_compaction(true, 100_000),
    ];
    let everything = ObservedCompactions::from_traces(&traces, AgentKind::ClaudeCode);
    let main_only = ObservedCompactions::main_sessions_only(&traces, AgentKind::ClaudeCode);
    assert_eq!(everything.count(), 2);
    assert_eq!(main_only.count(), 1);
    assert_eq!(everything.fraction_at_or_below(100_000), 0.5);
    assert_eq!(main_only.fraction_at_or_below(100_000), 0.0);
}
