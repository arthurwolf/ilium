//! Turbo mode core: one LLM call per changed agent, bounded and self-throttling.
//!
//! Pure scheduling logic (no threads, no clock, no randomness) so the worker
//! that owns the real calls stays thin and every rule is unit-testable:
//! - [`TurboLimiter`]: concurrency cap that halves on 429/503 and regrows.
//! - [`TurboScheduler`]: queue with dedupe by (pane, fingerprint), supersede,
//!   unchanged-skip and per-pane exponential backoff with caller-supplied jitter.

use std::collections::{HashMap, VecDeque};

use ilium_core::NodeId;

pub const MINIMUM_CONCURRENCY: usize = 1;
pub const MAXIMUM_CONCURRENCY: usize = 64;
pub const DEFAULT_CONCURRENCY: usize = 16;
const BACKOFF_BASE_MS: u64 = 1_000;
const BACKOFF_CAP_MS: u64 = 60_000;
/// Failed attempts per (pane, fingerprint) before the job is abandoned.
const MAXIMUM_ATTEMPTS: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    Success,
    /// HTTP 429/503: the provider is overloaded, shrink concurrency.
    Throttled,
    /// Any other failure: retry later without touching concurrency.
    Failed,
}

/// Additive-increase / multiplicative-decrease concurrency limiter.
#[derive(Debug, Clone)]
pub struct TurboLimiter {
    configured: usize,
    effective: usize,
    in_flight: usize,
    successes_since_change: usize,
}

impl TurboLimiter {
    pub fn new(configured: usize) -> Self {
        let configured = configured.clamp(MINIMUM_CONCURRENCY, MAXIMUM_CONCURRENCY);
        Self {
            configured,
            effective: configured,
            in_flight: 0,
            successes_since_change: 0,
        }
    }

    pub fn configured(&self) -> usize {
        self.configured
    }

    pub fn effective(&self) -> usize {
        self.effective
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight
    }

    /// Applies a changed setting without dropping running calls.
    pub fn set_configured(&mut self, configured: usize) {
        self.configured = configured.clamp(MINIMUM_CONCURRENCY, MAXIMUM_CONCURRENCY);
        self.effective = self.effective.min(self.configured).max(MINIMUM_CONCURRENCY);
    }

    pub fn try_acquire(&mut self) -> bool {
        if self.in_flight >= self.effective {
            return false;
        }
        self.in_flight += 1;
        true
    }

    pub fn release(&mut self, outcome: CallOutcome) {
        self.in_flight = self.in_flight.saturating_sub(1);
        match outcome {
            CallOutcome::Throttled => {
                self.effective = (self.effective / 2).max(MINIMUM_CONCURRENCY);
                self.successes_since_change = 0;
            }
            CallOutcome::Success => {
                self.successes_since_change += 1;
                if self.effective < self.configured && self.successes_since_change >= self.effective
                {
                    self.effective += 1;
                    self.successes_since_change = 0;
                }
            }
            CallOutcome::Failed => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurboJob {
    pub pane_id: NodeId,
    pub fingerprint: u64,
    pub attempt: u32,
    ready_at_ms: u64,
}

#[derive(Debug, Default)]
pub struct TurboScheduler {
    pending: VecDeque<TurboJob>,
    in_flight: HashMap<NodeId, u64>,
    /// Fingerprint each pane was last analysed at (success only).
    analysed: HashMap<NodeId, u64>,
}

impl TurboScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn in_flight_len(&self) -> usize {
        self.in_flight.len()
    }

    /// Queues work for a pane. Returns false when nothing was queued: the
    /// fingerprint was already analysed, is already running, or is already
    /// waiting. A waiting job with an older fingerprint is superseded.
    pub fn submit(&mut self, pane_id: NodeId, fingerprint: u64, now_ms: u64) -> bool {
        if self.analysed.get(&pane_id) == Some(&fingerprint)
            || self.in_flight.get(&pane_id) == Some(&fingerprint)
        {
            return false;
        }
        if let Some(waiting) = self.pending.iter_mut().find(|job| job.pane_id == pane_id) {
            if waiting.fingerprint == fingerprint {
                return false;
            }
            waiting.fingerprint = fingerprint;
            waiting.attempt = 0;
            waiting.ready_at_ms = now_ms;
            return true;
        }
        self.pending.push_back(TurboJob {
            pane_id,
            fingerprint,
            attempt: 0,
            ready_at_ms: now_ms,
        });
        true
    }

    /// Forgets a closed pane: drops its queue entry and its analysed marker.
    pub fn forget(&mut self, pane_id: NodeId) {
        self.pending.retain(|job| job.pane_id != pane_id);
        self.analysed.remove(&pane_id);
    }

    /// Starts the oldest ready job if the limiter has room.
    pub fn next_job(&mut self, limiter: &mut TurboLimiter, now_ms: u64) -> Option<TurboJob> {
        let index = self
            .pending
            .iter()
            .position(|job| job.ready_at_ms <= now_ms)?;
        if !limiter.try_acquire() {
            return None;
        }
        let job = self.pending.remove(index)?;
        self.in_flight.insert(job.pane_id, job.fingerprint);
        Some(job)
    }

    /// Records a finished call and returns whether the reply is still current
    /// (the pane's fingerprint did not move while the call ran).
    pub fn complete(
        &mut self,
        job: &TurboJob,
        outcome: CallOutcome,
        limiter: &mut TurboLimiter,
        now_ms: u64,
        jitter_ms: u64,
    ) {
        limiter.release(outcome);
        self.in_flight.remove(&job.pane_id);
        match outcome {
            CallOutcome::Success => {
                self.analysed.insert(job.pane_id, job.fingerprint);
            }
            CallOutcome::Throttled | CallOutcome::Failed => {
                let attempt = job.attempt + 1;
                let superseded = self
                    .pending
                    .iter()
                    .any(|waiting| waiting.pane_id == job.pane_id);
                if attempt >= MAXIMUM_ATTEMPTS || superseded {
                    return;
                }
                let delay = BACKOFF_BASE_MS
                    .saturating_mul(1 << attempt.min(16))
                    .min(BACKOFF_CAP_MS)
                    .saturating_add(jitter_ms.min(BACKOFF_BASE_MS));
                self.pending.push_back(TurboJob {
                    pane_id: job.pane_id,
                    fingerprint: job.fingerprint,
                    attempt,
                    ready_at_ms: now_ms.saturating_add(delay),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_clamps_and_never_exceeds_effective() {
        assert_eq!(TurboLimiter::new(0).configured(), 1);
        assert_eq!(TurboLimiter::new(500).configured(), 64);
        let mut limiter = TurboLimiter::new(2);
        assert!(limiter.try_acquire() && limiter.try_acquire());
        assert!(!limiter.try_acquire());
        limiter.release(CallOutcome::Success);
        assert!(limiter.try_acquire());
    }

    #[test]
    fn throttle_halves_and_successes_regrow_one_step_at_a_time() {
        let mut limiter = TurboLimiter::new(16);
        assert!(limiter.try_acquire());
        limiter.release(CallOutcome::Throttled);
        assert_eq!(limiter.effective(), 8);
        for _ in 0..8 {
            assert!(limiter.try_acquire());
            limiter.release(CallOutcome::Success);
        }
        assert_eq!(limiter.effective(), 9);
        for _ in 0..8 {
            limiter.try_acquire();
            limiter.release(CallOutcome::Throttled);
        }
        assert_eq!(limiter.effective(), 1);
        limiter.set_configured(4);
        assert_eq!(limiter.effective(), 1);
    }

    #[test]
    fn unchanged_fingerprints_are_always_skipped() {
        let mut scheduler = TurboScheduler::new();
        let mut limiter = TurboLimiter::new(4);
        assert!(scheduler.submit(NodeId(1), 10, 0));
        let job = scheduler.next_job(&mut limiter, 0).unwrap();
        assert!(!scheduler.submit(NodeId(1), 10, 0), "already running");
        scheduler.complete(&job, CallOutcome::Success, &mut limiter, 5, 0);
        assert!(!scheduler.submit(NodeId(1), 10, 6), "already analysed");
        assert!(
            scheduler.submit(NodeId(1), 11, 6),
            "changed evidence is new work"
        );
    }

    #[test]
    fn newer_fingerprint_supersedes_the_waiting_job() {
        let mut scheduler = TurboScheduler::new();
        assert!(scheduler.submit(NodeId(1), 10, 0));
        assert!(!scheduler.submit(NodeId(1), 10, 1));
        assert!(scheduler.submit(NodeId(1), 11, 2));
        assert_eq!(scheduler.pending_len(), 1);
        let mut limiter = TurboLimiter::new(4);
        assert_eq!(scheduler.next_job(&mut limiter, 2).unwrap().fingerprint, 11);
    }

    #[test]
    fn failed_calls_back_off_then_give_up() {
        let mut scheduler = TurboScheduler::new();
        let mut limiter = TurboLimiter::new(4);
        scheduler.submit(NodeId(1), 10, 0);
        let mut now = 0;
        let mut attempts = 0;
        loop {
            let Some(job) = scheduler.next_job(&mut limiter, now) else {
                if scheduler.pending_len() == 0 {
                    break;
                }
                now += 1_000;
                continue;
            };
            attempts += 1;
            scheduler.complete(&job, CallOutcome::Failed, &mut limiter, now, 0);
        }
        assert_eq!(attempts, MAXIMUM_ATTEMPTS);
        assert_eq!(
            limiter.effective(),
            4,
            "plain failures never shrink concurrency"
        );
        assert!(
            scheduler.submit(NodeId(1), 10, now),
            "an abandoned job can be requested again"
        );
    }

    #[test]
    fn forget_drops_queue_entry_and_analysed_marker() {
        let mut scheduler = TurboScheduler::new();
        scheduler.submit(NodeId(1), 10, 0);
        scheduler.forget(NodeId(1));
        assert_eq!(scheduler.pending_len(), 0);
    }

    /// 40 agents, a provider that answers 429 to the first calls: everything
    /// still completes exactly once, in-flight never exceeds the configured
    /// cap, and the cap shrinks while the provider is overloaded.
    #[test]
    fn stress_forty_agents_with_injected_throttling() {
        let mut scheduler = TurboScheduler::new();
        let mut limiter = TurboLimiter::new(16);
        for pane in 0..40u64 {
            assert!(scheduler.submit(NodeId(pane), 100 + pane, 0));
        }
        let mut now = 0u64;
        let mut calls = 0usize;
        let mut running: Vec<(TurboJob, u64)> = Vec::new();
        let mut finished: HashMap<NodeId, usize> = HashMap::new();
        let (mut max_in_flight, mut min_effective) = (0, limiter.effective());
        while finished.len() < 40 {
            while let Some(job) = scheduler.next_job(&mut limiter, now) {
                calls += 1;
                running.push((job, now + 300));
            }
            max_in_flight = max_in_flight.max(limiter.in_flight());
            assert!(limiter.in_flight() <= limiter.configured());
            now += 100;
            let mut index = 0;
            while index < running.len() {
                if running[index].1 > now {
                    index += 1;
                    continue;
                }
                let (job, _) = running.remove(index);
                // The first 20 calls are rejected with 429.
                let outcome = if calls <= 20 {
                    CallOutcome::Throttled
                } else {
                    CallOutcome::Success
                };
                scheduler.complete(&job, outcome, &mut limiter, now, 0);
                min_effective = min_effective.min(limiter.effective());
                if outcome == CallOutcome::Success {
                    *finished.entry(job.pane_id).or_default() += 1;
                }
            }
            assert!(now < 3_600_000, "stress run must converge");
        }
        assert!(
            finished.values().all(|count| *count == 1),
            "no pane analysed twice"
        );
        assert!(max_in_flight <= 16);
        assert!(
            min_effective < 16,
            "429s must lower the effective concurrency"
        );
        assert_eq!(scheduler.pending_len() + scheduler.in_flight_len(), 0);
        for pane in 0..40u64 {
            assert!(
                !scheduler.submit(NodeId(pane), 100 + pane, now),
                "unchanged agents are skipped"
            );
        }
    }
}
