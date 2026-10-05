//! The inner replay loop: one session, one trigger, one list of draws.

use super::rows::{MissModel, ReplayRow, TriggerRule};
use crate::price::{PriceWeights, TokenCounts};

/// A measured post-compaction rebuild: what the first request after a
/// compaction looked like. Sampled jointly (size and cache split together).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PostDraw {
    /// Context of the first post-compaction request.
    pub context_tokens: f64,
    /// Uncached input tokens of that request.
    pub input_tokens: f64,
    /// Cache-read tokens of that request.
    pub cache_read_tokens: f64,
    /// 5-minute-tier cache-write tokens of that request.
    pub cache_write_5m_tokens: f64,
    /// 1-hour-tier cache-write tokens of that request.
    pub cache_write_1h_tokens: f64,
    /// Summary tokens the compaction request produced.
    pub summary_tokens: f64,
}

/// Prices of one model for the engine: relative always, dollars when known.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EngineWeights {
    pub relative: PriceWeights,
    pub dollars: Option<PriceWeights>,
}

/// Everything the loop needs about the session and the rules.
pub(crate) struct EngineSession<'a> {
    pub rows: &'a [ReplayRow],
    pub first_context: f64,
    pub first_output: f64,
    pub weights: &'a [EngineWeights],
    pub write_5m_share: f64,
    pub trigger_rule: TriggerRule,
    pub miss_model: MissModel,
    pub min_turns_between: u32,
}

/// Cost accumulated by one run.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RunTotals {
    /// Cost in input-token equivalents, rework excluded.
    pub weighted: f64,
    /// Cost in dollars (0 when the session has unknown prices).
    pub dollars: f64,
    /// Compactions triggered.
    pub compactions: u32,
}

struct Accumulator<'a> {
    weights: &'a [EngineWeights],
    totals: RunTotals,
}

impl Accumulator<'_> {
    fn charge(&mut self, model: u16, tokens: &TokenCounts) {
        let last = self.weights.len().saturating_sub(1);
        let weights = &self.weights[usize::from(model).min(last)];
        self.totals.weighted += weights.relative.cost(tokens);
        if let Some(dollars) = weights.dollars {
            self.totals.dollars += dollars.cost(tokens);
        }
    }
}

/// Replays `session` with the compaction trigger at `trigger_tokens`.
///
/// `initial_cost` is the first request, billed as measured. Cost model (the
/// research's, unit = input-token equivalents):
///
/// * ordinary request: the previous context is read from the cache at the read
///   rate except the missed fraction, which is rewritten at the write rate; the
///   growth is written; plus the output and the small uncached input.
/// * compaction (when the trigger fires and at least `min_turns_between`
///   requests passed since the last one): the compaction request over the new
///   context (cache read at the read rate while warm, otherwise full input
///   rate; no cache write), the summary output, and the rebuild of the drawn
///   post-compaction prefix (its own cache split and write tiers). The request
///   that triggered it still pays its own output and input.
pub(crate) fn run_session(
    session: &EngineSession<'_>,
    trigger_tokens: u32,
    initial_cost: &TokenCounts,
    initial_model: u16,
    draw_pool: &[PostDraw],
    draw_list: &[u32],
) -> RunTotals {
    let mut accumulator = Accumulator {
        weights: session.weights,
        totals: RunTotals::default(),
    };
    accumulator.charge(initial_model, initial_cost);
    let trigger = f64::from(trigger_tokens);
    let mut current = session.first_context;
    let mut previous_output = session.first_output;
    let mut since_compaction = u32::MAX;
    let mut draw_cursor = 0_usize;
    for row in session.rows {
        let growth = f64::from(row.growth);
        let next = current + growth;
        let reached = match session.trigger_rule {
            TriggerRule::NextContext => next >= trigger,
            TriggerRule::LastResponseTotal => current + previous_output >= trigger,
        };
        let miss = match session.miss_model {
            MissModel::Binary => f64::from(row.miss).round(),
            MissModel::Fraction => f64::from(row.miss),
        };
        let read_tokens = (1.0 - miss) * current;
        let fresh_tokens = growth + miss * current;
        let output = f64::from(row.output);
        let input = f64::from(row.input);
        let fires =
            reached && since_compaction >= session.min_turns_between && !draw_pool.is_empty();
        if fires {
            let draw =
                draw_pool[draw_list[draw_cursor % draw_list.len()] as usize % draw_pool.len()];
            draw_cursor += 1;
            // The compaction request itself: nothing is cache-written.
            accumulator.charge(
                row.model,
                &TokenCounts {
                    input: input + fresh_tokens + draw.input_tokens,
                    output: output + draw.summary_tokens,
                    cache_read: read_tokens + draw.cache_read_tokens,
                    cache_write_5m: draw.cache_write_5m_tokens,
                    cache_write_1h: draw.cache_write_1h_tokens,
                },
            );
            accumulator.totals.compactions += 1;
            current = draw.context_tokens;
            since_compaction = 0;
        } else {
            accumulator.charge(
                row.model,
                &TokenCounts {
                    input,
                    output,
                    cache_read: read_tokens,
                    cache_write_5m: fresh_tokens * session.write_5m_share,
                    cache_write_1h: fresh_tokens * (1.0 - session.write_5m_share),
                },
            );
            current = next;
            since_compaction = since_compaction.saturating_add(1);
        }
        previous_output = output;
    }
    accumulator.totals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_weights() -> Vec<EngineWeights> {
        vec![EngineWeights {
            relative: PriceWeights {
                input: 1.0,
                output: 5.0,
                cache_read: 0.1,
                cache_write_5m: 1.25,
                cache_write_1h: 2.0,
            },
            dollars: None,
        }]
    }

    fn quiet_rows(count: usize, growth: u32) -> Vec<ReplayRow> {
        vec![
            ReplayRow {
                growth,
                output: 0,
                input: 0,
                miss: 0.0,
                model: 0,
            };
            count
        ]
    }

    fn session<'a>(rows: &'a [ReplayRow], weights: &'a [EngineWeights]) -> EngineSession<'a> {
        EngineSession {
            rows,
            first_context: 20_000.0,
            first_output: 0.0,
            weights,
            write_5m_share: 0.0,
            trigger_rule: TriggerRule::NextContext,
            miss_model: MissModel::Binary,
            min_turns_between: 5,
        }
    }

    #[test]
    fn without_compaction_cost_is_read_plus_write() {
        let weights = flat_weights();
        let rows = quiet_rows(3, 10_000);
        let totals = run_session(
            &session(&rows, &weights),
            u32::MAX,
            &TokenCounts::default(),
            0,
            &[],
            &[0],
        );
        // Contexts 20k, 30k, 40k are read at 0.1; 10k written at 2.0 each.
        let expected = 0.1 * (20_000.0 + 30_000.0 + 40_000.0) + 2.0 * 30_000.0;
        assert!((totals.weighted - expected).abs() < 1e-6);
        assert_eq!(totals.compactions, 0);
    }

    #[test]
    fn compaction_pays_request_summary_and_rebuild_and_honours_the_guard() {
        let weights = flat_weights();
        let rows = quiet_rows(8, 10_000);
        let draw = PostDraw {
            context_tokens: 20_000.0,
            input_tokens: 0.0,
            cache_read_tokens: 5_000.0,
            cache_write_5m_tokens: 0.0,
            cache_write_1h_tokens: 15_000.0,
            summary_tokens: 1_000.0,
        };
        let totals = run_session(
            &session(&rows, &weights),
            50_000,
            &TokenCounts::default(),
            0,
            &[draw],
            &[0],
        );
        // Contexts: 20k -> 30k -> 40k -> 50k fires (3 turns since start is
        // allowed because the counter starts saturated), then 5 guarded turns
        // pass before the next compaction can fire.
        assert_eq!(totals.compactions, 1);
        // Turn 1, 2 ordinary; turn 3 compaction request over 50k (read 40k at
        // 0.1, 10k fresh at 1.0) + summary 1k*5 + rebuild 5k*0.1 + 15k*2.0.
        let ordinary = 0.1 * 20_000.0 + 2.0 * 10_000.0 + 0.1 * 30_000.0 + 2.0 * 10_000.0;
        let compaction = 0.1 * 40_000.0 + 10_000.0 + 5.0 * 1_000.0 + 0.1 * 5_000.0 + 2.0 * 15_000.0;
        // Five more ordinary turns from a 20k context growing by 10k.
        let after: f64 = (0..5)
            .map(|step| 0.1 * (20_000.0 + 10_000.0 * f64::from(step)) + 2.0 * 10_000.0)
            .sum();
        assert!((totals.weighted - (ordinary + compaction + after)).abs() < 1e-6);
    }

    #[test]
    fn cold_requests_rewrite_the_whole_prefix_and_codex_rule_uses_last_total() {
        let weights = flat_weights();
        let mut rows = quiet_rows(1, 10_000);
        rows[0].miss = 1.0;
        let totals = run_session(
            &session(&rows, &weights),
            u32::MAX,
            &TokenCounts::default(),
            0,
            &[],
            &[0],
        );
        assert!((totals.weighted - 2.0 * 30_000.0).abs() < 1e-6);

        let mut codex = session(&rows, &weights);
        codex.trigger_rule = TriggerRule::LastResponseTotal;
        codex.first_output = 15_000.0;
        let pool = [PostDraw {
            context_tokens: 12_000.0,
            input_tokens: 0.0,
            cache_read_tokens: 12_000.0,
            cache_write_5m_tokens: 0.0,
            cache_write_1h_tokens: 0.0,
            summary_tokens: 0.0,
        }];
        // 20k context + 15k previous output = 35k >= 30k: fires on the first row.
        let fired = run_session(&codex, 30_000, &TokenCounts::default(), 0, &pool, &[0]);
        assert_eq!(fired.compactions, 1);
    }
}
