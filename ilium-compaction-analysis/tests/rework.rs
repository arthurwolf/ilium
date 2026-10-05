//! Measured rework on hand-built traces with a known excess of re-reads.
//!
//! Layout (window K = 10 requests, 30-request cycles, compactions at
//! positions 10, 40, 70, ...): every request costs exactly 10,000 weighted
//! tokens (100,000 tokens read from the cache at 0.1, nothing else), so costs
//! are exact.
//!
//! * B window (the 10 requests before a compaction): 1 read call per request,
//!   each on a path never read before.
//! * A window (the 10 requests after it): 2 read calls per request, the first
//!   re-reading the path of the matching B request, the second a fresh search;
//!   the request at offset 3 edits.
//! * Control window (middle of the cycle): 1 read call per request on fresh
//!   paths.
//!
//! Hand-computed per compaction: the read-call excess is 20 - 10 = 10 calls at
//! `read price 0.1 x context 100,000 = 10,000` each (no non-read cost), so
//! 100,000; the lost-re-read excess is 10 requests x 10,000 = 100,000 against
//! the control and against the previous window; the reading-request excess
//! versus the control is 10 - 10 = 0.

use ilium_compaction_analysis::optimize::{optimize, ObservedCompactions, OptimizeConfig};
use ilium_compaction_analysis::replay::{
    DrawPoolInfo, DrawSource, ReworkAccounting, ReworkModel, ReworkSource, SessionSim, SimTable,
    TriggerOutcome,
};
use ilium_compaction_analysis::rework::{
    measure, measure_with, CentralEstimator, EstimatorKind, ReworkConfig,
};
use ilium_compaction_analysis::semantics::AgentSemantics;
use ilium_compaction_analysis::trace::{
    CompactionEvent, CompactionTrigger, SessionTrace, ToolAccumulator, TurnSample, TOOL_FLAG_EDIT,
};
use ilium_compaction_analysis::AgentKind;

const K: usize = 10;

fn path_hash(turn: usize) -> u32 {
    4 * (10_000 + turn as u32)
}

fn search_hash(turn: usize) -> u32 {
    4 * (50_000 + turn as u32) + 1
}

fn event(turn_index: usize) -> CompactionEvent {
    CompactionEvent {
        timestamp_ms: 1_000 + turn_index as i64,
        turn_index: turn_index as u32,
        trigger: CompactionTrigger::Auto,
        pre_tokens: 200_000,
        logged_post_tokens: 0,
        last_pre_context_tokens: 100_000,
        post_measured: true,
        post_context_tokens: 100_000,
        post_input_tokens: 0,
        post_cache_read_tokens: 100_000,
        post_cache_write_5m_tokens: 0,
        post_cache_write_1h_tokens: 0,
        summary_tokens: 1_000,
        summary_estimated: false,
        duration_ms: 1_000,
        precomputed: false,
        replacement_history_tokens: 0,
        model: 0,
        request_turn_index: None,
    }
}

fn trace(agent: AgentKind, compactions: usize) -> SessionTrace {
    trace_with(agent, compactions, &|_| true)
}

/// `rereads(index)` says whether the window after compaction `index`
/// re-reads the paths of the window before it (else the first read of each
/// request is a fresh path).
fn trace_with(
    agent: AgentKind,
    compactions: usize,
    rereads: &dyn Fn(usize) -> bool,
) -> SessionTrace {
    let mut trace = SessionTrace::empty(agent, false);
    trace.models.push(
        if agent == AgentKind::Codex {
            "gpt-6.1-sol"
        } else {
            "claude-sonnet-5-5"
        }
        .to_string(),
    );
    let length = 10 + 30 * compactions;
    let positions: Vec<usize> = (0..compactions).map(|index| 10 + 30 * index).collect();
    for turn in 0..length {
        trace.turns.push(TurnSample {
            timestamp_ms: 1_000_000 + turn as i64 * 10_000,
            context_tokens: 100_000,
            input_tokens: 0,
            cache_read_tokens: 100_000,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            output_tokens: 0,
            model: 0,
            flags: 0,
            gap_decaseconds: 1,
        });
        trace.turn_id_hashes.push(turn as u32 + 1);
        let after_offset = positions
            .iter()
            .position(|&position| turn >= position && turn < position + K)
            .map(|index| (index, turn - positions[index]));
        let mut accumulator = ToolAccumulator::default();
        match after_offset {
            Some((index, offset)) => {
                let position = turn - offset;
                accumulator.tool_calls = 2;
                accumulator.read_calls = 2;
                accumulator.push_read_hash(if rereads(index) {
                    path_hash(position - K + offset)
                } else {
                    path_hash(turn)
                });
                accumulator.push_read_hash(search_hash(turn));
                if offset == 3 {
                    accumulator.flags |= TOOL_FLAG_EDIT;
                }
            }
            None => {
                accumulator.tool_calls = 1;
                accumulator.read_calls = 1;
                accumulator.push_read_hash(path_hash(turn));
            }
        }
        trace.tools.push(&accumulator);
    }
    trace.compactions = positions.iter().map(|&position| event(position)).collect();
    trace
}

fn small_config(min_compactions: usize) -> ReworkConfig {
    ReworkConfig {
        window_turns: K,
        min_compactions,
        min_control_cycle_turns: 20,
        bootstrap_resamples: 200,
        seed: 3,
    }
}

fn summary(
    measured: &ilium_compaction_analysis::rework::MeasuredRework,
    kind: EstimatorKind,
) -> ilium_compaction_analysis::rework::EstimatorSummary {
    *measured
        .estimators
        .iter()
        .find(|summary| summary.kind == kind)
        .unwrap()
}

#[test]
fn claude_rework_matches_the_hand_computation() {
    let traces = vec![trace(AgentKind::ClaudeCode, 5)];
    let measured = measure_with(&traces, AgentKind::ClaudeCode, None, &small_config(5)).unwrap();
    assert_eq!(measured.examined_compactions, 5);
    // Central policy: median of the estimators at the minimum (min 5: the
    // previous-window estimator has only 4 compactions and is left out), the
    // means being 100,000 / 100,000 / 0 -> 100,000.
    assert_eq!(
        measured.central_estimator,
        CentralEstimator::MedianOfEstimators
    );
    assert_eq!(measured.central_estimators.len(), 3);
    assert!(!measured
        .central_estimators
        .contains(&EstimatorKind::LostRereadTurnsVsPre));
    assert!(!measured.noisy);
    assert_eq!(measured.compactions, 5);
    assert!((measured.tokens_per_compaction - 100_000.0).abs() < 1e-6);
    assert!((measured.median_tokens - 100_000.0).abs() < 1e-6);
    assert!((measured.ci_low_tokens - 100_000.0).abs() < 1e-6);
    assert!((measured.ci_high_tokens - 100_000.0).abs() < 1e-6);

    let calls = summary(&measured, EstimatorKind::ExcessCallsVsPre);
    assert_eq!(calls.compactions, 5);
    assert!((calls.mean_tokens - 100_000.0).abs() < 1e-6);
    let lost_control = summary(&measured, EstimatorKind::LostRereadTurnsVsControl);
    assert_eq!(lost_control.compactions, 5);
    assert!((lost_control.mean_tokens - 100_000.0).abs() < 1e-6);
    let reading = summary(&measured, EstimatorKind::ReadingTurnsVsControl);
    assert!(reading.mean_tokens.abs() < 1e-9);
    // The first compaction has no previous compaction: 4 of 5 compactions.
    let lost_pre = summary(&measured, EstimatorKind::LostRereadTurnsVsPre);
    assert_eq!(lost_pre.compactions, 4);
    assert!((lost_pre.mean_tokens - 100_000.0).abs() < 1e-6);
    // Spread covers the estimators that reached the minimum (5): 0 to 100,000.
    assert!(measured.spread_low_tokens.abs() < 1e-9);
    assert!((measured.spread_high_tokens - 100_000.0).abs() < 1e-6);

    // Window means.
    assert_eq!(measured.after.windows, 5);
    assert_eq!(measured.after.read_calls, 20.0);
    assert_eq!(measured.before.read_calls, 10.0);
    assert_eq!(measured.control.read_calls, 10.0);
    assert_eq!(measured.after.lost_reread_turns, 10.0);
    assert_eq!(measured.control.lost_reread_turns, 0.0);
    assert_eq!(measured.after.reading_turns, 10.0);
    assert_eq!(measured.after.turns_to_first_edit, 3.0);
    assert_eq!(measured.before.turns_to_first_edit, 10.0);
    assert!((measured.after.weighted_cost_per_turn - 10_000.0).abs() < 1e-6);
    assert!((measured.after.lost_path_share - 1.0).abs() < 1e-12);
    assert_eq!(measured.control.lost_path_share, 0.0);
    assert!(measured
        .describe()
        .starts_with("rework measured from 5 compactions (100000 weighted tokens"));
}

#[test]
fn the_central_value_is_the_median_of_the_estimators_for_both_agents() {
    for agent in [AgentKind::ClaudeCode, AgentKind::Codex] {
        let traces = vec![trace(agent, 5)];
        // Minimum 4: all four estimators qualify, means 100,000 / 100,000 / 0 /
        // 100,000; the median of four is the mean of the middle two = 100,000.
        let all = measure_with(&traces, agent, None, &small_config(4)).unwrap();
        assert_eq!(all.central_estimators.len(), 4);
        assert!((all.tokens_per_compaction - 100_000.0).abs() < 1e-6);
        assert_eq!(all.compactions, 4);
        assert!(all.spread_low_tokens.abs() < 1e-9);
        assert!((all.spread_high_tokens - 100_000.0).abs() < 1e-6);
        // Minimum 5: three estimators, median of (100,000, 100,000, 0).
        let strict = measure_with(&traces, agent, None, &small_config(5)).unwrap();
        assert_eq!(strict.central_estimators.len(), 3);
        assert!((strict.tokens_per_compaction - 100_000.0).abs() < 1e-6);
        assert_eq!(strict.compactions, 5);
    }
}

#[test]
fn a_disagreeing_estimator_does_not_drive_the_central_value() {
    // The windows after the compactions read two FRESH paths per request:
    // excess calls are still +10 per compaction (100,000) but nothing is
    // re-read and the reading-request counts equal the control's, so the
    // other estimators are 0. Means: 100,000 / 0 / 0 (/ 0): the median is 0,
    // not the single excess-calls value.
    let traces = vec![trace_with(AgentKind::ClaudeCode, 5, &|_| false)];
    for minimum in [4, 5] {
        let measured =
            measure_with(&traces, AgentKind::ClaudeCode, None, &small_config(minimum)).unwrap();
        assert!(measured.tokens_per_compaction.abs() < 1e-9);
        assert!(
            (summary(&measured, EstimatorKind::ExcessCallsVsPre).mean_tokens - 100_000.0).abs()
                < 1e-6
        );
        assert!((measured.spread_high_tokens - 100_000.0).abs() < 1e-6);
    }
}

#[test]
fn a_noisy_measurement_is_flagged_and_blended_with_the_prior() {
    // Six compactions; only index 2 re-reads. Estimator means (min 5, so the
    // previous-window estimator with 5 compactions qualifies too):
    //   excess calls          100,000 (+10 calls each, 6 compactions)
    //   lost re-read vs M     100,000 / 6 = 16,666.67
    //   reading vs M          0
    //   lost re-read vs B     100,000 / 5 = 20,000 (index 0 has no previous)
    // Central = median of four = (16,666.67 + 20,000) / 2 = 18,333.33. A
    // resample without index 2 (probability (5/6)^6) has median 0, so the
    // 2.5th percentile is 0 and the spread low end is 0: noisy.
    let traces = vec![trace_with(AgentKind::ClaudeCode, 6, &|index| index == 2)];
    let measured = measure_with(&traces, AgentKind::ClaudeCode, None, &small_config(5)).unwrap();
    assert_eq!(measured.central_estimators.len(), 4);
    assert!((measured.mean_tokens - 18_333.333_333).abs() < 1e-3);
    assert!(measured.ci_low_tokens <= 0.0);
    assert!(measured.noisy);
    assert!(measured.describe().contains("noisy"));
    let model = ReworkModel::from_measurement(AgentKind::ClaudeCode, Some(&measured));
    assert!(model.blended_with_prior);
    assert_eq!(model.source, ReworkSource::Measured);
    // 0.5 x 18,333.33 + 0.5 x 72,000
    assert!((model.tokens_per_compaction - 45_166.666_667).abs() < 1e-3);
    assert!(model
        .describe()
        .contains("blended 50/50 with the research prior"));
    // The recorded interval stays the measured one.
    assert_eq!(
        model.measured_ci_tokens,
        Some((measured.ci_low_tokens, measured.ci_high_tokens))
    );
}

#[test]
fn the_noise_guard_needs_both_a_zero_reaching_interval_and_a_wide_spread() {
    use ilium_compaction_analysis::rework::is_noisy;
    assert!(is_noisy(-11_000.0, 21_000.0, 95_000.0)); // 4.5x and CI below zero
    assert!(is_noisy(0.0, 0.0, 10.0)); // zero low end is unbounded
    assert!(!is_noisy(5_000.0, 1_000.0, 95_000.0)); // CI above zero
    assert!(!is_noisy(-1.0, 30_000.0, 90_000.0)); // exactly 3x is not more than 3x
    assert!(!is_noisy(-1.0, 40_000.0, 90_000.0)); // narrow spread
    assert!(!is_noisy(-1.0, 0.0, 0.0)); // nothing measured
}

#[test]
fn a_clean_measurement_is_used_as_is() {
    let traces = vec![trace(AgentKind::ClaudeCode, 5)];
    let mut measured =
        measure_with(&traces, AgentKind::ClaudeCode, None, &small_config(5)).unwrap();
    assert!(!measured.noisy);
    let model = ReworkModel::from_measurement(AgentKind::ClaudeCode, Some(&measured));
    assert!(!model.blended_with_prior);
    assert!((model.tokens_per_compaction - 100_000.0).abs() < 1e-6);
    // Forcing the flag blends the same measurement: 0.5 x 100,000 + 0.5 x 72,000.
    measured.noisy = true;
    let blended = ReworkModel::from_measurement(AgentKind::ClaudeCode, Some(&measured));
    assert!((blended.tokens_per_compaction - 86_000.0).abs() < 1e-6);
}

#[test]
fn fewer_than_thirty_compactions_give_none() {
    let few = vec![trace(AgentKind::ClaudeCode, 29)];
    assert!(measure(&few, AgentKind::ClaudeCode).is_none());
    let enough = vec![trace(AgentKind::ClaudeCode, 30)];
    // The default window is 30 requests and the control needs 100-request
    // cycles, so use the default minimum with the small geometry.
    let config = ReworkConfig {
        min_compactions: 30,
        ..small_config(30)
    };
    let measured = measure_with(&enough, AgentKind::ClaudeCode, None, &config).unwrap();
    assert_eq!(measured.compactions, 30);
    assert!((measured.tokens_per_compaction - 100_000.0).abs() < 1e-6);
    assert!(measure_with(&few, AgentKind::ClaudeCode, None, &small_config(30)).is_none());
}

#[test]
fn subagents_traces_without_features_and_other_agents_are_ignored() {
    let mut subagent = trace(AgentKind::ClaudeCode, 5);
    subagent.is_subagent = true;
    assert!(measure_with(&[subagent], AgentKind::ClaudeCode, None, &small_config(1)).is_none());
    let mut without_features = trace(AgentKind::ClaudeCode, 5);
    without_features.tools = Default::default();
    assert!(measure_with(
        &[without_features],
        AgentKind::ClaudeCode,
        None,
        &small_config(1)
    )
    .is_none());
    let codex = trace(AgentKind::Codex, 5);
    assert!(measure_with(&[codex], AgentKind::ClaudeCode, None, &small_config(1)).is_none());
}

#[test]
fn the_measurement_becomes_the_rework_model() {
    let traces = vec![trace(AgentKind::ClaudeCode, 5)];
    let measured = measure_with(&traces, AgentKind::ClaudeCode, None, &small_config(5)).unwrap();
    let model = ReworkModel::from_measurement(AgentKind::ClaudeCode, Some(&measured));
    assert_eq!(model.source, ReworkSource::Measured);
    assert_eq!(model.measured_from_compactions, Some(5));
    assert!((model.tokens_per_compaction - 100_000.0).abs() < 1e-6);
    assert_eq!(
        model.measured_ci_tokens.map(|(low, _)| low.round()),
        Some(100_000.0)
    );
    assert!(model
        .describe()
        .contains("rework measured from 5 compactions"));
    let prior = ReworkModel::from_measurement(AgentKind::ClaudeCode, None);
    assert_eq!(prior.source, ReworkSource::Prior);
    assert_eq!(prior.tokens_per_compaction, 72_000.0);
    // Sensitivity multipliers scale the measured value.
    assert_eq!(model.with_multiplier(0.5).effective_tokens(), 50_000.0);
}

fn flat_table(compactions: [f64; 4]) -> SimTable {
    let triggers = [100_000, 150_000, 200_000, 250_000];
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
        outcomes: compactions
            .iter()
            .map(|&count| TriggerOutcome {
                cost: 1_000.0,
                usd: None,
                compactions: count,
            })
            .collect(),
    };
    SimTable {
        agent: AgentKind::ClaudeCode,
        triggers: triggers.to_vec(),
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

#[test]
fn the_optimizer_uses_the_measured_rework_and_labels_it() {
    let traces = vec![trace(AgentKind::ClaudeCode, 30)];
    let config = OptimizeConfig::for_corpus(AgentKind::ClaudeCode, &traces, None);
    // `for_corpus` measures with the research geometry (30-request windows
    // need 100-request cycles), which this 30-request-cycle trace lacks:
    // the prior is kept.
    assert_eq!(config.rework.source, ReworkSource::Prior);

    let measured = measure_with(&traces, AgentKind::ClaudeCode, None, &small_config(30)).unwrap();
    let optimize_config = OptimizeConfig::for_agent(AgentKind::ClaudeCode).with_rework(
        ReworkModel::from_measurement(AgentKind::ClaudeCode, Some(&measured)),
    );
    let table = flat_table([8.0, 4.0, 2.0, 1.0]);
    let recommendation = optimize(
        &table,
        &ObservedCompactions::from_sizes([180_000; 40]),
        &AgentSemantics::default_for(AgentKind::ClaudeCode),
        &optimize_config,
    )
    .unwrap();
    assert_eq!(recommendation.rework.source, ReworkSource::Measured);
    assert!(recommendation
        .basis
        .contains("rework measured from 30 compactions (100000 weighted tokens"));
    assert!(recommendation.warnings.iter().any(|warning| warning
        .contains("rework measured from 30 compactions")
        && warning.contains("0.5x, 1x and 2x")));
    assert!(!recommendation
        .warnings
        .iter()
        .any(|warning| warning.contains("research prior")));
    // 12 sessions x (1,000 + 100,000 per compaction x compactions).
    let costs: Vec<f64> = recommendation
        .candidates
        .iter()
        .map(|row| row.cost)
        .collect();
    assert_eq!(
        costs,
        vec![9_612_000.0, 4_812_000.0, 2_412_000.0, 1_212_000.0]
    );
    // Sensitivity rows stay at 0.5x / 1x / 2x of the measured value.
    let multipliers: Vec<f64> = recommendation
        .rework_sensitivity
        .iter()
        .map(|row| row.multiplier)
        .collect();
    assert_eq!(multipliers, vec![0.5, 1.0, 2.0]);
    assert_eq!(recommendation.trigger_tokens, 250_000);

    // With the prior the totals are lower (72,000 per compaction).
    let prior_recommendation = optimize(
        &table,
        &ObservedCompactions::from_sizes([180_000; 40]),
        &AgentSemantics::default_for(AgentKind::ClaudeCode),
        &OptimizeConfig::for_agent(AgentKind::ClaudeCode),
    )
    .unwrap();
    assert_eq!(
        prior_recommendation.candidates[0].cost,
        12.0 * (1_000.0 + 72_000.0 * 8.0)
    );
    assert!(prior_recommendation.basis.contains("research prior"));
}

#[test]
fn the_optimizer_labels_a_blended_noisy_measurement() {
    let traces = vec![trace_with(AgentKind::ClaudeCode, 6, &|index| index == 2)];
    let measured = measure_with(&traces, AgentKind::ClaudeCode, None, &small_config(5)).unwrap();
    assert!(measured.noisy);
    let config = OptimizeConfig::for_agent(AgentKind::ClaudeCode).with_rework(
        ReworkModel::from_measurement(AgentKind::ClaudeCode, Some(&measured)),
    );
    let recommendation = optimize(
        &flat_table([8.0, 4.0, 2.0, 1.0]),
        &ObservedCompactions::from_sizes([180_000; 40]),
        &AgentSemantics::default_for(AgentKind::ClaudeCode),
        &config,
    )
    .unwrap();
    assert!(recommendation
        .basis
        .contains("blended 50/50 with the research prior"));
    assert!(recommendation
        .warnings
        .iter()
        .any(|warning| warning.contains("noisy") && warning.contains("blended")));
    // 12 sessions x (1,000 + 45,166.67 per compaction x 8 compactions).
    let expected = 12.0 * (1_000.0 + (0.5 * 18_333.333_333 + 36_000.0) * 8.0);
    assert!((recommendation.candidates[0].cost - expected).abs() < 1.0);
}
