//! One UI-turn budget shared by whole-source, window, syntax and context capture.
//! Captures reserve before copying. A fixed FIFO prevents one pane/stage from
//! consuming the next turn repeatedly while other ready sources wait.
use ilium_core::NodeId;
use std::{collections::VecDeque, sync::Mutex};
const TURN_BYTES: usize = 256 * 1024;
const TURN_LINES: usize = 1024;
const PAGE_BYTES: usize = 64 * 1024;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    #[cfg(test)]
    Whole,
    Window,
    Syntax,
    #[cfg(test)]
    Context,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Consumer {
    pane: NodeId,
    kind: Kind,
}
struct State {
    bytes: usize,
    lines: usize,
    waiting: VecDeque<Consumer>,
    captured: Vec<Consumer>,
}
pub(crate) struct Credit {
    pub bytes: usize,
    pub lines: usize,
}
pub(crate) struct CaptureBudget {
    state: Mutex<State>,
}
impl CaptureBudget {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                bytes: TURN_BYTES,
                lines: TURN_LINES,
                waiting: VecDeque::with_capacity(16),
                captured: Vec::with_capacity(16),
            }),
        }
    }
    pub fn begin_turn(&self) {
        if let Ok(mut state) = self.state.try_lock() {
            state.bytes = TURN_BYTES;
            state.lines = TURN_LINES;
            state.captured.clear();
        }
    }
    pub fn take(&self, pane: NodeId, kind: Kind, line_limit: usize) -> Option<Credit> {
        let mut state = self.state.try_lock().ok()?;
        let consumer = Consumer { pane, kind };
        if !state.waiting.contains(&consumer) {
            if state.waiting.len() == 16 {
                return None;
            }
            state.waiting.push_back(consumer);
        }
        if state.waiting.front() != Some(&consumer)
            || state.bytes == 0
            || state.lines == 0
            || state.captured.contains(&consumer)
        {
            return None;
        }
        state.waiting.pop_front();
        state.captured.push(consumer);
        let bytes = state.bytes.min(PAGE_BYTES);
        let lines = state.lines.min(line_limit);
        state.bytes -= bytes;
        state.lines -= lines;
        Some(Credit { bytes, lines })
    }
    #[cfg(test)]
    pub fn remaining(&self) -> (usize, usize) {
        let state = self.state.lock().unwrap();
        (state.bytes, state.lines)
    }
    pub fn finish(&self, credit: Credit, bytes: usize, lines: usize) {
        if let Ok(mut state) = self.state.try_lock() {
            state.bytes = (state.bytes + credit.bytes.saturating_sub(bytes)).min(TURN_BYTES);
            state.lines = (state.lines + credit.lines.saturating_sub(lines)).min(TURN_LINES);
        }
    }
    pub fn cancel(&self, pane: NodeId, kind: Kind) {
        if let Ok(mut state) = self.state.try_lock() {
            state
                .waiting
                .retain(|consumer| consumer.pane != pane || consumer.kind != kind);
        }
    }
    pub fn retain_visible(&self, visible: &[NodeId]) {
        if let Ok(mut state) = self.state.try_lock() {
            state
                .waiting
                .retain(|consumer| visible.contains(&consumer.pane));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_pane_stage_cannot_recapture_in_one_turn_even_when_bytes_remain() {
        let budget = CaptureBudget::new();
        let first = budget.take(NodeId(1), Kind::Window, 1).unwrap();
        budget.finish(first, 64 * 1024, 0);
        assert!(budget.take(NodeId(1), Kind::Window, 1).is_none());
        budget.begin_turn();
        assert!(budget.take(NodeId(1), Kind::Window, 1).is_some());
    }
    #[test]
    fn aggregate_capture_is_bounded_and_refused_pane_gets_next_turn_before_prior_producer() {
        let budget = CaptureBudget::new();
        let mut bytes = 0;
        let mut lines = 0;
        for kind in [Kind::Window, Kind::Whole, Kind::Syntax, Kind::Context] {
            let credit = budget.take(NodeId(1), kind, 1).unwrap();
            bytes += credit.bytes;
            lines += credit.lines;
        }
        assert_eq!(bytes, TURN_BYTES);
        assert_eq!(lines, 4);
        assert!(budget.take(NodeId(2), Kind::Window, 1).is_none());
        budget.begin_turn();
        assert!(budget.take(NodeId(1), Kind::Window, 1).is_none());
        assert!(budget.take(NodeId(2), Kind::Window, 1).is_some());
        assert!(budget.take(NodeId(1), Kind::Window, 1).is_some());
        budget.take(NodeId(3), Kind::Context, 1);
        budget.cancel(NodeId(3), Kind::Context);
        budget.begin_turn();
        let credit = budget.take(NodeId(1), Kind::Whole, TURN_LINES).unwrap();
        assert_eq!(credit.lines, TURN_LINES);
        assert!(budget.take(NodeId(2), Kind::Syntax, 1).is_none());
    }
}
