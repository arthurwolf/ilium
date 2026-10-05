//! Hand-computed golden scenario.
//!
//! Three identical Claude sessions: a first request of 20,000 tokens, then 40
//! requests that each add exactly 10,000 tokens (warm cache, no output, no
//! uncached input). Weights: input 1, output 5, cache read 0.1, 1-hour write
//! 2.0. One measured compaction supplies the only post-compaction draw: a
//! 20,000-token prefix with 5,000 cache-read and 15,000 written tokens and a
//! 1,000-token summary.
//!
//! Per session, in weighted tokens (derivations in the test bodies):
//!
//! | trigger | compactions | cost      |
//! |---------|-------------|-----------|
//! | 60,000  | 7           | 1,194,500 |
//! | 100,000 | 5           | 1,187,500 |
//! | 200,000 | 2           | 1,283,000 |
//! | 500,000 | 0           | 1,700,000 |
//!
//! With the 72,000-token Claude rework prior added per compaction the
//! optimum moves from 100,000 to 200,000.

use ilium_compaction_analysis::optimize::{
    optimize, three_way_comparison, ObservedCompactions, OptimizeConfig, PointQuality,
};
use ilium_compaction_analysis::replay::{simulate, DrawSource, ReworkModel, SimConfig};
use ilium_compaction_analysis::semantics::AgentSemantics;
use ilium_compaction_analysis::trace::{
    CompactionEvent, CompactionTrigger, SessionTrace, TurnSample,
};
use ilium_compaction_analysis::AgentKind;

const SESSIONS: usize = 3;
const GRID: [u32; 4] = [60_000, 100_000, 200_000, 500_000];

fn warm_turn(context: u32, previous: u32, index: i64) -> TurnSample {
    TurnSample {
        timestamp_ms: 1_000_000 + index * 30_000,
        context_tokens: context,
        input_tokens: 0,
        cache_read_tokens: previous,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: context - previous,
        output_tokens: 0,
        model: 0,
        flags: 0,
        gap_decaseconds: 3,
    }
}

fn golden_trace() -> SessionTrace {
    let mut trace = SessionTrace::empty(AgentKind::ClaudeCode, false);
    trace.models.push("claude-sonnet-5-5".to_string());
    let mut first = warm_turn(20_000, 0, 0);
    first.gap_decaseconds = 0;
    trace.turns.push(first);
    for step in 1..=40_i64 {
        let context = 20_000 + 10_000 * step as u32;
        trace.turns.push(warm_turn(context, context - 10_000, step));
    }
    // The single measured compaction (placed after the last request so that
    // it contributes a draw without altering the replayed work).
    trace.compactions.push(CompactionEvent {
        timestamp_ms: 2_000_000,
        turn_index: 41,
        trigger: CompactionTrigger::Auto,
        pre_tokens: 200_000,
        logged_post_tokens: 0,
        last_pre_context_tokens: 200_000,
        post_measured: true,
        post_context_tokens: 20_000,
        post_input_tokens: 0,
        post_cache_read_tokens: 5_000,
        post_cache_write_5m_tokens: 0,
        post_cache_write_1h_tokens: 15_000,
        summary_tokens: 1_000,
        summary_estimated: true,
        duration_ms: 10_000,
        precomputed: false,
        replacement_history_tokens: 0,
        model: 0,
        request_turn_index: None,
    });
    trace
}

fn corpus() -> Vec<SessionTrace> {
    (0..SESSIONS).map(|_| golden_trace()).collect()
}

fn golden_config() -> SimConfig {
    let mut config = SimConfig::for_agent(AgentKind::ClaudeCode);
    config.triggers = GRID.to_vec();
    config.draw_lists = 3;
    config.min_regime_draw_events = 1;
    config
}

#[test]
fn replay_reproduces_the_hand_computed_costs() {
    let table = simulate(AgentKind::ClaudeCode, &corpus(), &golden_config(), None).unwrap();
    assert_eq!(table.sessions.len(), SESSIONS);
    assert_eq!(table.draw_pool.source, DrawSource::RecentRegime);
    let expected = [
        (60_000, 7.0, 1_194_500.0),
        (100_000, 5.0, 1_187_500.0),
        (200_000, 2.0, 1_283_000.0),
        (500_000, 0.0, 1_700_000.0),
    ];
    for (trigger, compactions, cost) in expected {
        let index = table.trigger_index(trigger).unwrap();
        for session in &table.sessions {
            let outcome = session.outcomes[index];
            assert_eq!(outcome.compactions, compactions, "compactions at {trigger}");
            assert!(
                (outcome.cost - cost).abs() < 1e-6,
                "cost at {trigger}: {} != {cost}",
                outcome.cost
            );
            assert!(outcome.usd.is_none());
        }
    }
    // No-compaction reference: 860k read + 800k written + 40k first request.
    assert!((table.sessions[0].no_compaction_cost - 1_700_000.0).abs() < 1e-6);
}

#[test]
fn optimum_follows_the_rework_penalty() {
    let table = simulate(AgentKind::ClaudeCode, &corpus(), &golden_config(), None).unwrap();
    let rework_off = ReworkModel::prior_for(AgentKind::ClaudeCode).with_multiplier(0.0);
    let totals = table.total_costs(&rework_off, &|_| true);
    assert!((totals[1] - 3.0 * 1_187_500.0).abs() < 1e-6);
    let best_without = totals
        .iter()
        .enumerate()
        .min_by(|left, right| left.1.total_cmp(right.1))
        .map(|(index, _)| GRID[index])
        .unwrap();
    assert_eq!(best_without, 100_000);

    // With 72,000 per compaction: 60k 1,698,500; 100k 1,547,500; 200k
    // 1,427,000; 500k 1,700,000 (per session).
    let prior = ReworkModel::prior_for(AgentKind::ClaudeCode);
    let totals = table.total_costs(&prior, &|_| true);
    let expected = [1_698_500.0, 1_547_500.0, 1_427_000.0, 1_700_000.0];
    for (total, per_session) in totals.iter().zip(expected) {
        assert!((total - 3.0 * per_session).abs() < 1e-6);
    }
}

#[test]
fn recommendation_picks_the_hand_derived_optimum_with_support() {
    let table = simulate(AgentKind::ClaudeCode, &corpus(), &golden_config(), None).unwrap();
    let semantics = AgentSemantics::default_for(AgentKind::ClaudeCode);
    let mut config = OptimizeConfig::for_agent(AgentKind::ClaudeCode);

    // Observed compactions around 200k: the optimum has support.
    let observed = ObservedCompactions::from_sizes([190_000, 200_000, 210_000]);
    let recommendation = optimize(&table, &observed, &semantics, &config).unwrap();
    assert_eq!(recommendation.trigger_tokens, 200_000);
    assert_eq!(recommendation.setting_value, 233_000);
    assert!(!recommendation.extrapolated);
    assert_eq!(recommendation.bands.argmin_tokens, 200_000);
    assert_eq!(recommendation.bands.within_2_percent.members, vec![200_000]);
    assert!(recommendation.bootstrap.resamples >= 200);
    assert_eq!(recommendation.bootstrap.argmin_median, 200_000);
    assert_eq!(recommendation.candidates.len(), GRID.len());
    assert!((recommendation.candidates[2].observed_support - 2.0 / 3.0).abs() < 1e-12);

    // Rework sensitivity: 200k is the best at 0.5x, 1x and 2x (at 0x it is 100k).
    let by_multiplier: Vec<(f64, u32)> = recommendation
        .rework_sensitivity
        .iter()
        .map(|row| (row.multiplier, row.argmin_tokens))
        .collect();
    assert_eq!(
        by_multiplier,
        vec![(0.5, 200_000), (1.0, 200_000), (2.0, 200_000)]
    );

    // No observed compaction at or below the optimum: plain argmin, flagged.
    let none_below = ObservedCompactions::from_sizes([567_000; 5]);
    let extrapolated = optimize(&table, &none_below, &semantics, &config).unwrap();
    assert_eq!(extrapolated.trigger_tokens, 200_000);
    assert!(extrapolated.extrapolated);
    assert_eq!(extrapolated.observed_support_at_pick, 0.0);
    assert!(extrapolated.basis.contains("0% of observed compactions"));
    assert!(extrapolated
        .warnings
        .iter()
        .any(|warning| warning.contains("extrapolated")));

    // Without rework the 2% band is {60k, 100k}; support at 50k picks the
    // lowest supported member (60k), support only at 70k picks 100k.
    config.rework = config.rework.with_multiplier(0.0);
    let low_support = ObservedCompactions::from_sizes([50_000, 55_000]);
    let picked_low = optimize(&table, &low_support, &semantics, &config).unwrap();
    assert_eq!(picked_low.bands.argmin_tokens, 100_000);
    assert_eq!(
        picked_low.bands.within_2_percent.members,
        vec![60_000, 100_000]
    );
    assert_eq!(picked_low.trigger_tokens, 60_000);
    assert!(!picked_low.extrapolated);
    let mid_support = ObservedCompactions::from_sizes([70_000, 80_000]);
    let picked_mid = optimize(&table, &mid_support, &semantics, &config).unwrap();
    assert_eq!(picked_mid.trigger_tokens, 100_000);
    assert!(!picked_mid.extrapolated);
}

#[test]
fn optimizer_is_deterministic_for_a_seed() {
    let table = simulate(AgentKind::ClaudeCode, &corpus(), &golden_config(), None).unwrap();
    let semantics = AgentSemantics::default_for(AgentKind::ClaudeCode);
    let config = OptimizeConfig::for_agent(AgentKind::ClaudeCode);
    let observed = ObservedCompactions::from_sizes([200_000; 10]);
    let first = optimize(&table, &observed, &semantics, &config).unwrap();
    let second = optimize(&table, &observed, &semantics, &config).unwrap();
    assert_eq!(first, second);
    let again = simulate(AgentKind::ClaudeCode, &corpus(), &golden_config(), None).unwrap();
    assert_eq!(again, table);
}

#[test]
fn three_way_comparison_prices_default_current_and_recommended() {
    let table = simulate(AgentKind::ClaudeCode, &corpus(), &golden_config(), None).unwrap();
    let rework = ReworkModel::prior_for(AgentKind::ClaudeCode);
    // CLI default 967k lies beyond the grid; current 150k is interpolated.
    let points = three_way_comparison(&table, &rework, 967_000, Some(150_000), 200_000);
    assert_eq!(points.len(), 3);
    assert_eq!(points[0].label, "default");
    assert_eq!(points[0].quality, PointQuality::OutsideGrid);
    assert_eq!(points[1].label, "current");
    assert_eq!(points[1].quality, PointQuality::Interpolated);
    // Midpoint of 100k (1,547,500) and 200k (1,427,000) per session.
    assert!((points[1].cost - 3.0 * 1_487_250.0).abs() < 1e-6);
    assert_eq!(points[2].quality, PointQuality::Exact);
    assert!(points[2].relative_to_recommended.abs() < 1e-12);
    assert!(points[0].relative_to_recommended > 0.19);
    assert!(points[2].relative_to_default < 0.0);
}

#[test]
fn empty_or_small_corpora_are_refused() {
    let small = SessionTrace::empty(AgentKind::ClaudeCode, false);
    let result = simulate(AgentKind::ClaudeCode, &[small], &golden_config(), None);
    assert!(result.is_err());
    let mut config = golden_config();
    config.triggers.clear();
    assert!(simulate(AgentKind::ClaudeCode, &corpus(), &config, None).is_err());
}

#[test]
fn per_model_optima_reflect_each_models_cache_read_price() {
    // Five Sonnet-priced and five Opus 5.5-priced copies of the scenario: the
    // research prices Opus 5.5 cache reads at 0.05 instead of 0.1, so carrying
    // context is cheaper and compacting early pays off less.
    let mut traces = Vec::new();
    for index in 0..10 {
        let mut trace = golden_trace();
        if index >= 5 {
            trace.models[0] = "claude-opus-5-5".to_string();
        }
        traces.push(trace);
    }
    let table = simulate(AgentKind::ClaudeCode, &traces, &golden_config(), None).unwrap();
    let observed = ObservedCompactions::from_sizes([190_000, 200_000, 210_000]);
    let recommendation = optimize(
        &table,
        &observed,
        &AgentSemantics::default_for(AgentKind::ClaudeCode),
        &OptimizeConfig::for_agent(AgentKind::ClaudeCode),
    )
    .unwrap();
    let families: Vec<&str> = recommendation
        .per_model
        .iter()
        .map(|model| model.family.as_str())
        .collect();
    assert_eq!(families.len(), 2);
    assert!(families.contains(&"sonnet") && families.contains(&"opus"));
    let sonnet = recommendation
        .per_model
        .iter()
        .find(|model| model.family == "sonnet")
        .unwrap();
    let opus = recommendation
        .per_model
        .iter()
        .find(|model| model.family == "opus")
        .unwrap();
    assert_eq!(sonnet.argmin_tokens, 200_000);
    assert!(opus.argmin_tokens >= sonnet.argmin_tokens);
    assert!(recommendation
        .warnings
        .iter()
        .any(|warning| warning.contains("cannot be applied per model")));
}

#[test]
fn summary_rows_carry_totals_per_model_and_median() {
    let table = simulate(AgentKind::ClaudeCode, &corpus(), &golden_config(), None).unwrap();
    let rework = ReworkModel::prior_for(AgentKind::ClaudeCode).with_multiplier(0.0);
    let rows = table.summary(&rework);
    assert_eq!(rows.len(), GRID.len());
    assert_eq!(rows[1].trigger_tokens, 100_000);
    assert!((rows[1].weighted_cost - 3.0 * 1_187_500.0).abs() < 1e-6);
    assert!((rows[1].median_session_cost - 1_187_500.0).abs() < 1e-6);
    assert_eq!(rows[1].compactions, 15.0);
    assert_eq!(rows[1].per_model_cost.len(), 1);
    assert_eq!(rows[1].per_model_cost[0].0, "sonnet");
    assert!((rows[1].per_model_cost[0].1 - rows[1].weighted_cost).abs() < 1e-6);
    assert!(rows[1].usd.is_none());
}

#[test]
fn triggers_off_the_grid_are_reported() {
    let table = simulate(AgentKind::ClaudeCode, &corpus(), &golden_config(), None).unwrap();
    assert_eq!(table.require_trigger_index(100_000).unwrap(), 1);
    assert!(matches!(
        table.require_trigger_index(123_456),
        Err(ilium_compaction_analysis::AnalysisError::TriggerNotSimulated(123_456))
    ));
}
