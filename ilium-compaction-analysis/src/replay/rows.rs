//! Turns a measured trace into the work stream the simulator replays.
//!
//! The replay needs, per request, only the *work*: how much the context grew,
//! the output, the small uncached input and how much of the previous context
//! missed the cache. Everything caused by an observed compaction (the
//! summarisation request, the cold rebuild, the re-reading afterwards) must be
//! removed from that stream, otherwise it would be paid twice. The two agents
//! need different surgery, so each has its own builder:
//!
//! * **Claude**: the 30 requests after each observed compaction are replaced by
//!   a mirror of the work before it (the pre-window repeated in order), which
//!   also removes the observed rework; the penalty is therefore charged in
//!   absolute terms.
//! * **Codex**: the summarisation requests are dropped and the first request
//!   after a compaction is given the growth that had accumulated; the observed
//!   rework stays in the stream, so the penalty is charged differentially
//!   (only for compactions beyond the observed count).

use crate::agent::AgentKind;
use crate::trace::{SessionTrace, TurnSample, FLAG_CACHE_COLD};

/// One replayed request.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReplayRow {
    /// Growth of the context caused by this request's new tokens.
    pub growth: u32,
    /// Output tokens.
    pub output: u32,
    /// Uncached input tokens beyond the growth (cache-granularity residue).
    pub input: u32,
    /// Fraction of the previous context that missed the cache (0..=1).
    pub miss: f32,
    /// Model index of the request (prices).
    pub model: u16,
}

/// A replayable session.
#[derive(Debug)]
pub(crate) struct SessionRows {
    /// Context of the first request.
    pub first_context: u32,
    /// The first request, billed as measured.
    pub first_turn: TurnSample,
    /// Replayed requests after the first.
    pub rows: Vec<ReplayRow>,
    /// Observed compactions whose work was removed from the stream.
    pub observed_compactions: u32,
    /// Negative growth steps clamped to zero.
    pub negative_growth_steps: u32,
}

/// How the engine decides that the trigger fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerRule {
    /// The context after the request's growth reaches the trigger (Claude).
    NextContext,
    /// The previous response's input plus output reaches the trigger (Codex).
    LastResponseTotal,
}

/// How a request's cache miss is modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissModel {
    /// A miss rewrites the whole prefix (Claude: the cache is binary).
    Binary,
    /// A measured fraction of the previous context missed (Codex).
    Fraction,
}

pub(crate) fn build_rows(trace: &SessionTrace, mirror_turns: usize) -> Option<SessionRows> {
    match trace.agent {
        AgentKind::ClaudeCode => claude_rows(trace, mirror_turns),
        AgentKind::Codex => codex_rows(trace),
    }
}

fn claude_rows(trace: &SessionTrace, mirror_turns: usize) -> Option<SessionRows> {
    let turns = &trace.turns;
    if turns.len() < 2 {
        return None;
    }
    let mut rows: Vec<ReplayRow> = (1..turns.len())
        .map(|index| {
            let turn = &turns[index];
            let previous = &turns[index - 1];
            ReplayRow {
                growth: turn.context_tokens.saturating_sub(previous.context_tokens),
                output: turn.output_tokens,
                input: turn.input_tokens,
                miss: if turn.has_flag(FLAG_CACHE_COLD) {
                    1.0
                } else {
                    0.0
                },
                model: turn.model,
            }
        })
        .collect();
    let mut boundaries: Vec<usize> = trace
        .compactions
        .iter()
        .map(|event| event.turn_index as usize)
        .filter(|&index| index > 0 && index < turns.len())
        .collect();
    boundaries.sort_unstable();
    boundaries.dedup();
    for (position, &boundary) in boundaries.iter().enumerate() {
        let previous_boundary = if position > 0 {
            boundaries[position - 1]
        } else {
            0
        };
        let window_start = (previous_boundary + 1).max(boundary.saturating_sub(mirror_turns));
        let pre_window: Vec<usize> = (window_start..boundary).collect();
        let span = mirror_turns.min(turns.len() - boundary);
        for offset in 0..span {
            let index = boundary + offset;
            if offset > 0 && boundaries.binary_search(&index).is_ok() {
                break;
            }
            let replacement = if pre_window.is_empty() {
                // No work before the compaction to mirror: nominal values of a
                // quiet request (research defaults).
                (1_100, 300, 2, 0.0)
            } else {
                let source = pre_window[offset % pre_window.len()];
                let mirrored = &rows[source.saturating_sub(1)];
                (
                    mirrored.growth,
                    mirrored.output,
                    mirrored.input,
                    mirrored.miss,
                )
            };
            let row = &mut rows[index - 1];
            row.growth = replacement.0;
            row.output = replacement.1;
            row.input = replacement.2;
            row.miss = replacement.3;
        }
    }
    Some(SessionRows {
        first_context: turns[0].context_tokens,
        first_turn: turns[0],
        rows,
        observed_compactions: boundaries.len() as u32,
        negative_growth_steps: 0,
    })
}

fn codex_rows(trace: &SessionTrace) -> Option<SessionRows> {
    let mut work = trace
        .turns
        .iter()
        .enumerate()
        .filter(|(_, turn)| !turn.is_compaction_request());
    let (first_index, first_turn) = work.next()?;
    let mut rows = Vec::with_capacity(trace.turns.len());
    let mut previous = *first_turn;
    let mut previous_index = first_index;
    let mut negative = 0_u32;
    for (index, turn) in work {
        let request_between = index > previous_index + 1;
        let (mut growth, input, miss) = if request_between {
            // The growth that accumulated up to the compaction request; the
            // rebuilt prefix of this request is modelled by the compaction.
            let request = &trace.turns[index - 1];
            (
                i64::from(request.context_tokens) - i64::from(previous.context_tokens),
                0,
                0.0,
            )
        } else {
            let growth = i64::from(turn.context_tokens) - i64::from(previous.context_tokens);
            let observed_fresh = i64::from(turn.context_tokens) - i64::from(turn.cache_read_tokens);
            let residue = observed_fresh - growth.max(0);
            let granularity_input = residue.clamp(0, 1_024);
            let missed = (residue - 1_024).max(0);
            let fraction = (missed as f64 / f64::from(previous.context_tokens.max(1))).min(1.0);
            (growth, granularity_input as u32, fraction as f32)
        };
        if growth < 0 {
            negative += 1;
            growth = 0;
        }
        rows.push(ReplayRow {
            growth: growth as u32,
            output: turn.output_tokens,
            input,
            miss,
            model: turn.model,
        });
        previous = *turn;
        previous_index = index;
    }
    let observed = trace
        .compactions
        .iter()
        .filter(|event| event.request_turn_index.is_some())
        .count() as u32;
    Some(SessionRows {
        first_context: first_turn.context_tokens,
        first_turn: *first_turn,
        rows,
        observed_compactions: observed,
        negative_growth_steps: negative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{
        CompactionEvent, CompactionTrigger, FLAG_COMPACTION_REQUEST, FLAG_FIRST_AFTER_COMPACTION,
    };

    fn turn(context: u32, read: u32, output: u32, flags: u8) -> TurnSample {
        TurnSample {
            timestamp_ms: 0,
            context_tokens: context,
            input_tokens: 1,
            cache_read_tokens: read,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            output_tokens: output,
            model: 0,
            flags,
            gap_decaseconds: 0,
        }
    }

    fn event(turn_index: u32, request: Option<u32>) -> CompactionEvent {
        CompactionEvent {
            timestamp_ms: 0,
            turn_index,
            trigger: CompactionTrigger::Auto,
            pre_tokens: 0,
            logged_post_tokens: 0,
            last_pre_context_tokens: 0,
            post_measured: true,
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
            request_turn_index: request,
        }
    }

    #[test]
    fn claude_rows_mirror_the_work_before_a_compaction() {
        let mut trace = SessionTrace::empty(AgentKind::ClaudeCode, false);
        trace.models.push("m".into());
        // Contexts 100, 110, 125, 142 then a compaction rebuild to 40 and 45.
        for (context, output) in [
            (100, 10),
            (110, 20),
            (125, 30),
            (142, 40),
            (40, 99),
            (45, 98),
        ] {
            trace.turns.push(turn(context, context, output, 0));
        }
        trace.compactions.push(event(4, None));
        let rebuilt = build_rows(&trace, 30).unwrap();
        assert_eq!(rebuilt.observed_compactions, 1);
        assert_eq!(rebuilt.first_context, 100);
        // Rows are for turns 1..6; turns 4 and 5 are replaced by a mirror of
        // turns 1, 2 and 3 (cycled in order).
        let growth: Vec<u32> = rebuilt.rows.iter().map(|row| row.growth).collect();
        let output: Vec<u32> = rebuilt.rows.iter().map(|row| row.output).collect();
        assert_eq!(growth, vec![10, 15, 17, 10, 15]);
        assert_eq!(output, vec![20, 30, 40, 20, 30]);
    }

    #[test]
    fn claude_rows_without_prior_work_use_nominal_values() {
        let mut trace = SessionTrace::empty(AgentKind::ClaudeCode, false);
        trace.models.push("m".into());
        for (context, output) in [(100, 10), (40, 99), (45, 98)] {
            trace.turns.push(turn(context, context, output, 0));
        }
        trace.compactions.push(event(1, None));
        let rows = build_rows(&trace, 30).unwrap().rows;
        assert_eq!(
            (rows[0].growth, rows[0].output, rows[0].input),
            (1_100, 300, 2)
        );
    }

    #[test]
    fn codex_rows_drop_requests_and_credit_the_pre_compaction_growth() {
        let mut trace = SessionTrace::empty(AgentKind::Codex, false);
        trace.models.push("m".into());
        trace.turns.push(turn(1_000, 0, 10, 0));
        trace.turns.push(turn(1_100, 1_000, 10, 0));
        // Compaction request over 1_250 tokens, then the rebuilt request.
        trace
            .turns
            .push(turn(1_250, 1_100, 500, FLAG_COMPACTION_REQUEST));
        trace
            .turns
            .push(turn(300, 100, 10, FLAG_FIRST_AFTER_COMPACTION));
        trace.turns.push(turn(350, 300, 10, 0));
        trace.compactions.push(event(3, Some(2)));
        let prepared = build_rows(&trace, 30).unwrap();
        assert_eq!(prepared.observed_compactions, 1);
        assert_eq!(prepared.first_context, 1_000);
        let growth: Vec<u32> = prepared.rows.iter().map(|row| row.growth).collect();
        // 100 normal; 150 = request input minus the last work input; 50.
        assert_eq!(growth, vec![100, 150, 50]);
        // Warm second request: the 99-token residue is cache granularity.
        assert_eq!(prepared.rows[0].input, 0);
        assert_eq!(prepared.rows[0].miss, 0.0);
        // The rebuilt request carries no miss and no granularity residue.
        assert_eq!(prepared.rows[1].miss, 0.0);
        assert_eq!(prepared.rows[1].input, 0);
    }
}
