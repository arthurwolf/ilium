//! Opt-in bounded scalar observations; no worker, media copy or log writer.
//!
//! Enable only in a task-owned process with ILIUM_VIDEO_DIAGNOSTIC=1 and
//! the existing debug log sink. The last event is an exhaustion marker.
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

const MAX_EVENTS: usize = 1024;
static NEXT_SCENE: AtomicU64 = AtomicU64::new(0);
static EVENTS: EventBudget = EventBudget(AtomicUsize::new(0));

struct EventBudget(AtomicUsize);
impl EventBudget {
    fn take(&self) -> Option<usize> {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                (used < MAX_EVENTS).then_some(used + 1)
            })
            .ok()
            .map(|used| used + 1)
    }
}

pub(super) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("ILIUM_VIDEO_DIAGNOSTIC").is_some_and(|value| value == "1")
    })
}

pub(super) struct Diagnostics {
    id: u64,
    renders: AtomicU64,
    decoded: AtomicU64,
    wall_ms: AtomicU64,
    source_bits: AtomicU64,
}
impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            id: NEXT_SCENE
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .map_or(0, |id| id + 1),
            renders: AtomicU64::new(0),
            decoded: AtomicU64::new(0),
            wall_ms: AtomicU64::new(0),
            source_bits: AtomicU64::new(0),
        }
    }
}
impl Diagnostics {
    pub(super) fn id(&self) -> u64 {
        self.id
    }

    // Render-side observations do not log, allocate, lock or inspect a process.
    pub(super) fn rendered(&self, wall: Duration) {
        self.wall_ms.store(
            wall.as_millis().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
        self.renders.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn decoded(&self, source_seconds: f64) {
        self.source_bits
            .store(source_seconds.to_bits(), Ordering::Relaxed);
        self.decoded.fetch_add(1, Ordering::Relaxed);
    }

    // Callers supply only fixed scalars, enums and a validated 64-byte digest.
    // These are independent samples, not a coherent snapshot or a terminal ACK.
    pub(super) fn emit(&self, phase: &'static str, facts: impl std::fmt::Debug) {
        if self.id == 0 || !enabled() {
            return;
        }
        let Some(event) = EVENTS.take() else {
            return;
        };
        if event == MAX_EVENTS {
            tracing::warn!(
                target: "ilium_video_diagnostic", event, scene = self.id,
                "Video diagnostic exhausted; later absence is not evidence"
            );
            return;
        }
        tracing::warn!(
            target: "ilium_video_diagnostic", event, scene = self.id, phase,
            render_calls = self.renders.load(Ordering::Relaxed),
            decoded_frames = self.decoded.load(Ordering::Relaxed),
            render_wall_ms = self.wall_ms.load(Ordering::Relaxed),
            decoded_source_seconds = f64::from_bits(self.source_bits.load(Ordering::Relaxed)),
            facts = ?facts, "Video diagnostic"
        );
    }
}

/// Results sampled by the actual read path, before stream Drop can kill/reap.
/// None means the operation was not performed, never successful completion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeRead {
    pub expected: usize,
    pub received: usize,
    pub read_error: Option<std::io::ErrorKind>,
    pub watchdog_expired: bool,
    pub slot_closed: bool,
    pub eof_reaped: Option<bool>,
    pub exit_success: Option<bool>,
    pub elapsed_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_budget_is_finite_and_cannot_wrap_or_reopen() {
        let budget = EventBudget(AtomicUsize::new(0));
        for event in 1..=MAX_EVENTS {
            assert_eq!(budget.take(), Some(event));
        }
        for _ in 0..32 {
            assert_eq!(budget.take(), None);
        }
        assert_eq!(budget.0.load(Ordering::Relaxed), MAX_EVENTS);
    }

    #[test]
    fn diagnostic_render_and_decode_progress_are_distinct() {
        assert!(std::mem::size_of::<Diagnostics>() <= 64);
        assert!(std::mem::size_of::<NativeRead>() <= 128);
        let first = Diagnostics::default();
        let second = Diagnostics::default();
        assert_ne!(first.id(), second.id());
        first.rendered(Duration::from_millis(600));
        first.rendered(Duration::from_millis(700));
        assert_eq!(first.renders.load(Ordering::Relaxed), 2);
        assert_eq!(first.decoded.load(Ordering::Relaxed), 0);
        first.decoded(6.25);
        assert_eq!(first.decoded.load(Ordering::Relaxed), 1);
        assert_eq!(
            f64::from_bits(first.source_bits.load(Ordering::Relaxed)),
            6.25
        );
        assert_eq!(second.renders.load(Ordering::Relaxed), 0);
    }
}
