//! Cross-file deduplication.
//!
//! Resumed and forked sessions copy earlier history into a new transcript
//! file, so the same request (and the same compaction) can appear in several
//! traces. Per-file dedupe cannot see that; this pass can, using the 32-bit
//! identifier hash every turn carries together with its timestamp (a false
//! match needs both to collide).

use std::collections::HashSet;

use crate::trace::SessionTrace;

/// What a dedupe pass removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DedupeReport {
    /// Requests removed.
    pub turns_removed: usize,
    /// Compaction events removed.
    pub compactions_removed: usize,
}

/// Removes requests and compactions that an *earlier* trace of the slice
/// already holds. Order matters: put main sessions before subagents and sort
/// by file name for a reproducible winner, as the research did.
///
/// Derived fields (cold-cache flags, gaps) and compaction indices of the
/// affected traces are recomputed.
pub fn dedupe_across_traces(traces: &mut [SessionTrace]) -> DedupeReport {
    let mut report = DedupeReport::default();
    let mut seen_turns: HashSet<(u32, i64)> = HashSet::new();
    let mut seen_events: HashSet<(i64, u32, u32)> = HashSet::new();
    for trace in traces.iter_mut() {
        let original_events = trace.compactions.len();
        trace.compactions.retain(|event| {
            // Only events with a timestamp can be matched across files.
            event.timestamp_ms == 0
                || seen_events.insert((
                    event.timestamp_ms,
                    event.pre_tokens,
                    event.logged_post_tokens,
                ))
        });
        report.compactions_removed += original_events - trace.compactions.len();
        if trace.turn_id_hashes.len() != trace.turns.len() {
            continue;
        }
        let keep: Vec<bool> = trace
            .turns
            .iter()
            .zip(&trace.turn_id_hashes)
            .map(|(turn, &hash)| hash == 0 || seen_turns.insert((hash, turn.timestamp_ms)))
            .collect();
        if keep.iter().all(|&kept| kept) {
            continue;
        }
        let mut kept_before = Vec::with_capacity(keep.len() + 1);
        let mut kept_count = 0_u32;
        for &kept in &keep {
            kept_before.push(kept_count);
            kept_count += u32::from(kept);
        }
        kept_before.push(kept_count);
        report.turns_removed += keep.iter().filter(|&&kept| !kept).count();
        let mut flags = keep.iter();
        trace
            .turns
            .retain(|_| flags.next().copied().unwrap_or(true));
        let mut flags = keep.iter();
        trace
            .turn_id_hashes
            .retain(|_| flags.next().copied().unwrap_or(true));
        if trace.tools.is_consistent_with(keep.len()) {
            trace.tools.retain_turns(&keep);
        } else {
            trace.tools = Default::default();
        }
        trace.rebuild_after_removal(&kept_before);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentKind;
    use crate::trace::TurnSample;

    fn turn(timestamp_ms: i64, context: u32) -> TurnSample {
        TurnSample {
            timestamp_ms,
            context_tokens: context,
            input_tokens: 1,
            cache_read_tokens: context.saturating_sub(1),
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            output_tokens: 10,
            model: 0,
            flags: 0,
            gap_decaseconds: 0,
        }
    }

    fn trace_with(turns: Vec<(i64, u32, u32)>) -> SessionTrace {
        let mut trace = SessionTrace::empty(AgentKind::ClaudeCode, false);
        trace.models.push("m".into());
        for (timestamp, context, hash) in turns {
            trace.turns.push(turn(timestamp, context));
            trace.turn_id_hashes.push(hash);
        }
        trace
    }

    #[test]
    fn a_resumed_copy_keeps_only_its_new_requests() {
        let original = trace_with(vec![(1_000, 30_000, 11), (2_000, 31_000, 12)]);
        let resumed = trace_with(vec![
            (1_000, 30_000, 11),
            (2_000, 31_000, 12),
            (3_000, 32_000, 13),
        ]);
        let mut traces = vec![original, resumed];
        let report = dedupe_across_traces(&mut traces);
        assert_eq!(report.turns_removed, 2);
        assert_eq!(traces[0].turns.len(), 2);
        assert_eq!(traces[1].turns.len(), 1);
        assert_eq!(traces[1].turn_id_hashes, vec![13]);
        // The remaining request has no predecessor any more.
        assert_eq!(traces[1].turns[0].gap_decaseconds, 0);
    }

    #[test]
    fn same_hash_at_another_time_is_not_a_duplicate() {
        let mut traces = vec![
            trace_with(vec![(1_000, 30_000, 11)]),
            trace_with(vec![(9_000, 30_000, 11)]),
        ];
        assert_eq!(dedupe_across_traces(&mut traces).turns_removed, 0);
    }
}
