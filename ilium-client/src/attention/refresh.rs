//! Coalescing of press-triggered refreshes: never stack, never spam.

/// What the caller should do with a refresh request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshDecision {
    /// Start a refresh now.
    StartNow,
    /// One is already running; exactly one more will run when it finishes.
    QueuedAfterCurrent,
    /// Too soon after the last start; the request is dropped.
    TooSoon,
}

#[derive(Debug, Clone)]
pub struct RefreshGate {
    minimum_interval_ms: u64,
    in_flight: bool,
    rerun_pending: bool,
    last_started_ms: Option<u64>,
}

impl RefreshGate {
    pub fn new(minimum_interval_ms: u64) -> Self {
        Self {
            minimum_interval_ms,
            in_flight: false,
            rerun_pending: false,
            last_started_ms: None,
        }
    }

    pub fn is_in_flight(&self) -> bool {
        self.in_flight
    }

    pub fn request(&mut self, now_ms: u64) -> RefreshDecision {
        if self.in_flight {
            self.rerun_pending = true;
            return RefreshDecision::QueuedAfterCurrent;
        }
        let too_soon = self
            .last_started_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < self.minimum_interval_ms);
        if too_soon {
            return RefreshDecision::TooSoon;
        }
        self.in_flight = true;
        self.last_started_ms = Some(now_ms);
        RefreshDecision::StartNow
    }

    /// Marks the running refresh finished. Returns true when exactly one queued
    /// rerun should start now (the gate has already recorded it as started).
    pub fn finished(&mut self, now_ms: u64) -> bool {
        self.in_flight = false;
        if !std::mem::take(&mut self.rerun_pending) {
            return false;
        }
        self.in_flight = true;
        self.last_started_ms = Some(now_ms);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_during_a_refresh_coalesce_into_one_rerun() {
        let mut gate = RefreshGate::new(2_000);
        assert_eq!(gate.request(0), RefreshDecision::StartNow);
        assert_eq!(gate.request(10), RefreshDecision::QueuedAfterCurrent);
        assert_eq!(gate.request(20), RefreshDecision::QueuedAfterCurrent);
        assert!(gate.finished(100));
        assert!(gate.is_in_flight());
        assert!(!gate.finished(200));
        assert!(!gate.is_in_flight());
    }

    #[test]
    fn minimum_interval_drops_key_repeat() {
        let mut gate = RefreshGate::new(2_000);
        assert_eq!(gate.request(0), RefreshDecision::StartNow);
        assert!(!gate.finished(50));
        assert_eq!(gate.request(100), RefreshDecision::TooSoon);
        assert_eq!(gate.request(2_000), RefreshDecision::StartNow);
    }
}
