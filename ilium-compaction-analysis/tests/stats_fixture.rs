//! Corpus statistics on the synthetic Claude fixture, which contains one
//! cold-cache request after a long idle gap.

use ilium_compaction_analysis::parse::TraceBuilder;
use ilium_compaction_analysis::stats::{CorpusStats, StatsConfig};
use ilium_compaction_analysis::AgentKind;

#[test]
fn cold_cache_share_and_rewrite_cost_come_from_the_fixture() {
    let mut builder = TraceBuilder::new(AgentKind::ClaudeCode);
    for line in include_str!("fixtures/claude_session.jsonl").split('\n') {
        builder.feed_line(line.as_bytes());
    }
    let trace = builder.finish();
    // Session-level write-tier mix: the rewrite is priced at the blended weight.
    let short: f64 = trace
        .turns
        .iter()
        .map(|turn| f64::from(turn.cache_write_5m_tokens))
        .sum();
    let long: f64 = trace
        .turns
        .iter()
        .map(|turn| f64::from(turn.cache_write_1h_tokens))
        .sum();
    let blended = (short * 1.25 + long * 2.0) / (short + long);
    let stats = CorpusStats::compute(
        AgentKind::ClaudeCode,
        &[trace],
        &StatsConfig::default(),
        None,
    );
    // 9 requests; the first and the post-compaction one are not considered.
    assert_eq!(stats.cold_cache.turns_considered, 7);
    assert_eq!(stats.cold_cache.cache_cold_turns, 1);
    assert_eq!(stats.cold_cache.gap_cold_turns, 1);
    assert!((stats.cold_cache.cache_cold_share - 1.0 / 7.0).abs() < 1e-12);
    // The cold request rewrote 36,202 tokens while the context grew by 2:
    // 36,200 excess written tokens at the blended write weight.
    let total = stats.cost_mix.total_weighted;
    let expected_share = blended * 36_200.0 / total;
    assert!((stats.cold_cache.rewrite_cost_share - expected_share).abs() < 1e-9);
    assert_eq!(stats.counts.compactions, 2);
    assert_eq!(stats.counts.auto_compactions, 1);
    assert_eq!(stats.counts.manual_compactions, 1);
    assert_eq!(stats.counts.sessions_with_compaction, 1);
    assert_eq!(stats.compactions.summary_tokens.unwrap().p50 as u32, 2_000);
    assert_eq!(stats.compactions.duration_seconds.unwrap().n, 2);
    // One cycle (post-compaction request to the end) is too short to count.
    assert_eq!(stats.cycles.cycles, 0);
    let reach = stats
        .reach
        .iter()
        .find(|row| row.threshold_tokens == 150_000)
        .unwrap();
    assert_eq!(reach.sessions_reaching, 0);
    assert_eq!(stats.counts.lines_skipped, 2);
}
