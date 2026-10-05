//! Window metrics of one compaction: the raw material of the estimators.

use std::collections::HashSet;

use crate::price::{price_of, resolve_trace_prices, PriceLookup, ResolvedPrice, TokenCounts};
use crate::trace::{SessionTrace, ToolOffsets, HASH_KIND_MASK, HASH_KIND_PATH};

/// Window geometry.
pub(super) struct SampleSettings {
    pub window_turns: usize,
    pub min_control_cycle_turns: usize,
}

/// Metrics of one window of work requests.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct WindowMetrics {
    pub read_calls: u32,
    pub reading_turns: u32,
    pub lost_reread_turns: u32,
    pub distinct_paths: u32,
    pub lost_paths: u32,
    pub repeated_calls: u32,
    pub lost_command_repeats: u32,
    pub turns_to_first_edit: u32,
    pub tool_calls: u32,
    pub weighted_cost_per_turn: f64,
    pub nonread_cost_total: f64,
    pub mean_context: f64,
}

/// The windows of one compaction.
#[derive(Debug, Clone)]
pub(super) struct CompactionSample {
    pub after: WindowMetrics,
    pub before: Option<WindowMetrics>,
    pub control: Option<WindowMetrics>,
    pub has_previous_compaction: bool,
    /// Relative cache-read price of the model that served the window.
    pub read_price: f64,
}

/// Per-session arrays indexed by work position (compaction requests removed).
struct SessionView<'a> {
    trace: &'a SessionTrace,
    offsets: ToolOffsets,
    work: Vec<usize>,
    cost: Vec<f64>,
    nonread: Vec<f64>,
    context: Vec<f64>,
}

impl SessionView<'_> {
    /// Path and command hashes of the work requests before `position`.
    fn history_before(&self, position: usize) -> (HashSet<u32>, HashSet<u32>) {
        let mut paths = HashSet::new();
        let mut commands = HashSet::new();
        for &turn in &self.work[..position.min(self.work.len())] {
            for &hash in self.reads_of(turn) {
                if hash & HASH_KIND_MASK == HASH_KIND_PATH {
                    paths.insert(hash);
                }
            }
            commands.extend(self.commands_of(turn).iter().copied());
        }
        (paths, commands)
    }

    fn reads_of(&self, turn: usize) -> &[u32] {
        &self.trace.tools.read_hashes[self.offsets.read[turn]..self.offsets.read[turn + 1]]
    }

    fn commands_of(&self, turn: usize) -> &[u32] {
        &self.trace.tools.command_hashes[self.offsets.command[turn]..self.offsets.command[turn + 1]]
    }

    /// Metrics of the work positions `lo..hi`; `skip_first_cost` leaves the
    /// first request out of the cost means (the post-compaction rebuild).
    fn window(
        &self,
        lo: usize,
        hi: usize,
        lost_paths: &HashSet<u32>,
        lost_commands: &HashSet<u32>,
        skip_first_cost: bool,
    ) -> WindowMetrics {
        let mut metrics = WindowMetrics::default();
        let mut seen_paths: HashSet<u32> = HashSet::new();
        let mut seen_calls: HashSet<u32> = HashSet::new();
        let mut first_edit: Option<usize> = None;
        for position in lo..hi {
            let turn = self.work[position];
            let row = self.trace.tools.turns[turn];
            metrics.read_calls += u32::from(row.read_calls);
            metrics.tool_calls += u32::from(row.tool_calls);
            if row.read_calls > 0 {
                metrics.reading_turns += 1;
            }
            let mut turn_rereads_lost = false;
            for &hash in self.reads_of(turn) {
                if hash & HASH_KIND_MASK == HASH_KIND_PATH && seen_paths.insert(hash) {
                    metrics.distinct_paths += 1;
                    if lost_paths.contains(&hash) {
                        metrics.lost_paths += 1;
                        turn_rereads_lost = true;
                    }
                }
                if !seen_calls.insert(hash) {
                    metrics.repeated_calls += 1;
                }
            }
            for &hash in self.commands_of(turn) {
                if !seen_calls.insert(hash) {
                    metrics.repeated_calls += 1;
                }
                if lost_commands.contains(&hash) {
                    metrics.lost_command_repeats += 1;
                }
            }
            if turn_rereads_lost {
                metrics.lost_reread_turns += 1;
            }
            if first_edit.is_none() && row.has_any_write() {
                first_edit = Some(position - lo);
            }
        }
        let length = hi - lo;
        metrics.turns_to_first_edit = first_edit.unwrap_or(length) as u32;
        let counted_from = if skip_first_cost { lo + 1 } else { lo };
        let counted = (hi - counted_from).max(1) as f64;
        let cost_sum: f64 = (counted_from..hi).map(|position| self.cost[position]).sum();
        let nonread_sum: f64 = (counted_from..hi)
            .map(|position| self.nonread[position])
            .sum();
        metrics.weighted_cost_per_turn = cost_sum / counted;
        metrics.nonread_cost_total = nonread_sum / counted * length as f64;
        metrics.mean_context =
            (lo..hi).map(|position| self.context[position]).sum::<f64>() / length.max(1) as f64;
        metrics
    }
}

/// Builds the per-session arrays; `None` when the trace carries no usable tool
/// features (an old cache or a trace parsed without them).
fn view<'a>(trace: &'a SessionTrace, prices: &[ResolvedPrice]) -> Option<SessionView<'a>> {
    if !trace.tools.is_consistent_with(trace.turns.len()) {
        return None;
    }
    let share_5m = trace.cache_write_5m_share();
    let work: Vec<usize> = trace
        .turns
        .iter()
        .enumerate()
        .filter(|(_, turn)| !turn.is_compaction_request())
        .map(|(index, _)| index)
        .collect();
    let mut cost = Vec::with_capacity(work.len());
    let mut nonread = Vec::with_capacity(work.len());
    let mut context = Vec::with_capacity(work.len());
    let mut previous_context = 0_u32;
    for &index in &work {
        let turn = &trace.turns[index];
        let weights = &price_of(prices, turn.model).relative;
        cost.push(weights.cost(&TokenCounts::of_turn(turn)));
        // Growth of the context, plus a small allowance, bounds what the
        // request legitimately wrote; the rest of a cold write is a rewrite.
        let growth = turn.context_tokens.saturating_sub(previous_context);
        let written = f64::from(turn.cache_write_tokens().min(growth.saturating_add(1_500)));
        nonread.push(
            weights.output * f64::from(turn.output_tokens)
                + weights.input * f64::from(turn.input_tokens)
                + weights.blended_write(share_5m) * written,
        );
        context.push(f64::from(turn.context_tokens));
        previous_context = turn.context_tokens;
    }
    Some(SessionView {
        trace,
        offsets: trace.tools.offsets(),
        work,
        cost,
        nonread,
        context,
    })
}

/// Appends one sample per eligible measured compaction of the trace.
pub(super) fn collect_compaction_samples(
    trace: &SessionTrace,
    lookup: Option<PriceLookup<'_>>,
    settings: &SampleSettings,
    out: &mut Vec<CompactionSample>,
) {
    let prices = resolve_trace_prices(trace, lookup);
    let Some(session) = view(trace, &prices) else {
        return;
    };
    // Work positions of every compaction (measured or not delimit cycles).
    let mut boundaries: Vec<(usize, bool)> = trace
        .compactions
        .iter()
        .filter(|event| event.turn_index > 0)
        .map(|event| {
            let position = session
                .work
                .partition_point(|&turn| turn < event.turn_index as usize);
            (position, event.is_measured())
        })
        .collect();
    boundaries.sort_unstable();
    boundaries.dedup_by_key(|entry| entry.0);
    let window = settings.window_turns;
    for (index, &(position, measured)) in boundaries.iter().enumerate() {
        if !measured || position == 0 || position >= session.work.len() {
            continue;
        }
        let next = boundaries
            .get(index + 1)
            .map_or(session.work.len(), |entry| entry.0);
        if position + window > next {
            continue;
        }
        let previous = if index > 0 {
            boundaries[index - 1].0
        } else {
            0
        };
        let (lost_paths, lost_commands) = session.history_before(position);
        let after = session.window(
            position,
            position + window,
            &lost_paths,
            &lost_commands,
            true,
        );
        let before = (position >= previous + window).then(|| {
            let (earlier_paths, earlier_commands) = if index > 0 {
                session.history_before(previous)
            } else {
                (HashSet::new(), HashSet::new())
            };
            session.window(
                position - window,
                position,
                &earlier_paths,
                &earlier_commands,
                false,
            )
        });
        let cycle = next - position;
        let control = (cycle >= settings.min_control_cycle_turns && cycle >= window).then(|| {
            let start = position + (cycle - window) / 2;
            session.window(start, start + window, &lost_paths, &lost_commands, false)
        });
        let model = trace.turns[session.work[position]].model;
        out.push(CompactionSample {
            after,
            before,
            control,
            has_previous_compaction: index > 0,
            read_price: price_of(&prices, model).relative.cache_read,
        });
    }
}
