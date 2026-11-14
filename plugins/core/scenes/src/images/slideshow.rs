//! Slideshow order and scheduler. Pure: time is injected, image availability
//! is asked through a closure, nothing here touches a thread or the disk.

use super::motion::{ease, hash64};
use super::settings::{Easing, SlideOrder};
use std::time::Duration;

/// Deterministic order of `len` list indices.
pub fn build_order(len: usize, order: SlideOrder, seed: u64) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..len).collect();
    if order == SlideOrder::Shuffle {
        // Fisher-Yates with a splitmix64 stream.
        let mut state = hash64(seed ^ 0x5851_f42d_4c95_7f2d);
        for position in (1..len).rev() {
            state = hash64(state);
            let other = (state % (position as u64 + 1)) as usize;
            indices.swap(position, other);
        }
    }
    indices
}

/// Whether the image at a position can be shown yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Ready,
    Pending,
    Failed,
}

/// Display and cross-fade lengths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub display: Duration,
    pub transition: Duration,
}

impl Timing {
    pub fn new(display_seconds: u16, transition_seconds: u16) -> Self {
        Self {
            display: Duration::from_secs(u64::from(display_seconds.max(1))),
            transition: Duration::from_secs(u64::from(transition_seconds)),
        }
    }

    /// The cross-fade never takes more than half the display time.
    pub fn effective_transition(&self) -> Duration {
        self.transition.min(self.display / 2)
    }

    /// Length of one slide's Ken-Burns move: its display time plus the fade
    /// that covers the next slide's arrival, so the motion never stops
    /// while the slide is still visible.
    pub fn move_duration(&self) -> Duration {
        self.display + self.effective_transition()
    }
}

/// One slide on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slide {
    /// Position within the order.
    pub position: usize,
    /// Scheduler time at which the slide started.
    pub started: Duration,
    /// Number of slides shown before this one (parity alternates pans).
    pub serial: u64,
}

impl Slide {
    /// Raw Ken-Burns progress 0..=1 at `now`.
    pub fn progress(&self, now: Duration, timing: &Timing) -> f32 {
        let total = timing.move_duration().as_secs_f32();
        if total <= 0.0 {
            return 1.0;
        }
        (now.saturating_sub(self.started).as_secs_f32() / total).clamp(0.0, 1.0)
    }
}

/// What to draw at one instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapshot {
    pub current: Slide,
    /// The slide fading out and its remaining weight (`1 - fade`).
    pub outgoing: Option<Slide>,
    /// Weight of `current`, 0..=1 (1 when not fading).
    pub fade: f32,
}

#[derive(Debug, Default)]
pub struct Scheduler {
    current: Option<Slide>,
    outgoing: Option<Slide>,
    shown: u64,
}

impl Scheduler {
    pub fn current(&self) -> Option<Slide> {
        self.current
    }

    pub fn is_fading(&self) -> bool {
        self.outgoing.is_some()
    }

    /// Advance the state machine to `now`. `len` is the number of positions;
    /// `availability(position)` tells whether that slide is shown-ready. The
    /// scheduler waits (keeps the current slide) for pending images and
    /// skips failed ones.
    pub fn update(
        &mut self,
        now: Duration,
        timing: &Timing,
        len: usize,
        availability: impl Fn(usize) -> Availability,
    ) {
        if len == 0 {
            *self = Self::default();
            return;
        }
        let Some(current) = self.current else {
            for position in 0..len {
                match availability(position) {
                    Availability::Ready => {
                        self.start(position, now);
                        return;
                    }
                    Availability::Pending => return,
                    Availability::Failed => {}
                }
            }
            return;
        };
        if current.position >= len {
            *self = Self::default();
            return;
        }
        let age = now.saturating_sub(current.started);
        if self.outgoing.is_some() && age >= timing.effective_transition() {
            self.outgoing = None;
        }
        if self.outgoing.is_some() || age < timing.display {
            return;
        }
        for step in 1..len {
            let position = (current.position + step) % len;
            match availability(position) {
                Availability::Ready => {
                    let leaving = current;
                    self.start(position, now);
                    if !timing.effective_transition().is_zero() {
                        self.outgoing = Some(leaving);
                    }
                    return;
                }
                Availability::Pending => return,
                Availability::Failed => {}
            }
        }
    }

    fn start(&mut self, position: usize, now: Duration) {
        self.current = Some(Slide {
            position,
            started: now,
            serial: self.shown,
        });
        self.shown += 1;
        self.outgoing = None;
    }

    /// The draw state at `now`.
    pub fn snapshot(&self, now: Duration, timing: &Timing) -> Option<Snapshot> {
        let current = self.current?;
        let transition = timing.effective_transition().as_secs_f32();
        let (outgoing, fade) = match self.outgoing {
            Some(outgoing) if transition > 0.0 => {
                let raw = now.saturating_sub(current.started).as_secs_f32() / transition;
                // The blend always uses a smooth curve, independent of the
                // motion easing, so a cross-fade never looks like a wipe.
                (Some(outgoing), ease(Easing::EaseInOut, raw))
            }
            _ => (None, 1.0),
        };
        Some(Snapshot {
            current,
            outgoing,
            fade,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(value: f32) -> Duration {
        Duration::from_secs_f32(value)
    }

    fn timing() -> Timing {
        Timing::new(10, 2)
    }

    fn all_ready(_: usize) -> Availability {
        Availability::Ready
    }

    #[test]
    fn sequential_order_is_the_identity() {
        assert_eq!(
            build_order(5, SlideOrder::Sequential, 1),
            vec![0, 1, 2, 3, 4]
        );
        assert!(build_order(0, SlideOrder::Shuffle, 1).is_empty());
    }

    #[test]
    fn shuffle_is_a_deterministic_permutation() {
        let a = build_order(50, SlideOrder::Shuffle, 42);
        let b = build_order(50, SlideOrder::Shuffle, 42);
        let c = build_order(50, SlideOrder::Shuffle, 43);
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
        assert_ne!(a, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn effective_transition_is_capped_at_half_the_display_time() {
        assert_eq!(Timing::new(10, 30).effective_transition(), secs(5.0));
        assert_eq!(Timing::new(10, 2).effective_transition(), secs(2.0));
        assert_eq!(Timing::new(10, 2).move_duration(), secs(12.0));
    }

    #[test]
    fn slides_advance_on_schedule_with_a_crossfade() {
        let timing = timing();
        let mut scheduler = Scheduler::default();
        scheduler.update(secs(0.0), &timing, 3, all_ready);
        assert_eq!(scheduler.current().map(|slide| slide.position), Some(0));
        scheduler.update(secs(9.9), &timing, 3, all_ready);
        assert_eq!(scheduler.current().map(|slide| slide.position), Some(0));
        assert!(!scheduler.is_fading());

        scheduler.update(secs(10.0), &timing, 3, all_ready);
        let snapshot = scheduler.snapshot(secs(10.0), &timing).expect("snapshot");
        assert_eq!(snapshot.current.position, 1);
        assert_eq!(snapshot.outgoing.map(|slide| slide.position), Some(0));
        assert!(snapshot.fade.abs() < 1e-6);

        let halfway = scheduler.snapshot(secs(11.0), &timing).expect("snapshot");
        assert!((halfway.fade - 0.5).abs() < 1e-5);

        scheduler.update(secs(12.0), &timing, 3, all_ready);
        assert!(!scheduler.is_fading());
        let done = scheduler.snapshot(secs(12.0), &timing).expect("snapshot");
        assert_eq!(done.fade, 1.0);
        assert!(done.outgoing.is_none());
    }

    #[test]
    fn positions_wrap_around() {
        let timing = Timing::new(3, 0);
        let mut scheduler = Scheduler::default();
        let mut seen = Vec::new();
        for tick in 0..8u64 {
            scheduler.update(Duration::from_secs(tick * 3), &timing, 3, all_ready);
            seen.push(scheduler.current().map(|slide| slide.position));
        }
        assert_eq!(
            seen,
            [0, 1, 2, 0, 1, 2, 0, 1].map(Some).to_vec(),
            "zero transition switches instantly and wraps"
        );
        assert!(!scheduler.is_fading());
    }

    #[test]
    fn failed_images_are_skipped_and_pending_ones_awaited() {
        let timing = timing();
        let mut scheduler = Scheduler::default();
        let availability = |position: usize| match position {
            0 => Availability::Ready,
            1 => Availability::Failed,
            2 => Availability::Pending,
            _ => Availability::Ready,
        };
        scheduler.update(secs(0.0), &timing, 4, availability);
        scheduler.update(secs(25.0), &timing, 4, availability);
        assert_eq!(
            scheduler.current().map(|slide| slide.position),
            Some(0),
            "waits for the pending image instead of jumping past it"
        );
        let later = |position: usize| match position {
            1 => Availability::Failed,
            _ => Availability::Ready,
        };
        scheduler.update(secs(26.0), &timing, 4, later);
        assert_eq!(scheduler.current().map(|slide| slide.position), Some(2));
    }

    #[test]
    fn first_slide_skips_failures_and_waits_for_pending() {
        let timing = timing();
        let mut scheduler = Scheduler::default();
        scheduler.update(secs(0.0), &timing, 2, |position| {
            if position == 0 {
                Availability::Failed
            } else {
                Availability::Pending
            }
        });
        assert!(scheduler.current().is_none());
        scheduler.update(secs(1.0), &timing, 2, |position| {
            if position == 0 {
                Availability::Failed
            } else {
                Availability::Ready
            }
        });
        assert_eq!(scheduler.current().map(|slide| slide.position), Some(1));
    }

    #[test]
    fn single_playable_image_never_switches() {
        let timing = timing();
        let mut scheduler = Scheduler::default();
        let availability = |position: usize| {
            if position == 0 {
                Availability::Ready
            } else {
                Availability::Failed
            }
        };
        for tick in 0..10u64 {
            scheduler.update(Duration::from_secs(tick * 7), &timing, 3, availability);
        }
        assert_eq!(scheduler.current().map(|slide| slide.position), Some(0));
        assert!(!scheduler.is_fading());
    }

    #[test]
    fn slide_motion_progress_is_continuous_across_the_handover() {
        let timing = timing();
        let mut scheduler = Scheduler::default();
        scheduler.update(secs(0.0), &timing, 2, all_ready);
        let first = scheduler.current().expect("slide");
        scheduler.update(secs(10.0), &timing, 2, all_ready);
        // The outgoing slide is still moving at handover and reaches exactly
        // progress 1.0 when its fade ends.
        let outgoing = scheduler
            .snapshot(secs(10.0), &timing)
            .and_then(|snapshot| snapshot.outgoing)
            .expect("outgoing");
        assert_eq!(outgoing, first);
        assert!((outgoing.progress(secs(10.0), &timing) - 10.0 / 12.0).abs() < 1e-5);
        assert!((outgoing.progress(secs(12.0), &timing) - 1.0).abs() < 1e-5);
        let current = scheduler.current().expect("slide");
        assert_eq!(current.progress(secs(10.0), &timing), 0.0);
        assert_eq!(current.serial, 1);
    }

    #[test]
    fn empty_list_resets_the_scheduler() {
        let mut scheduler = Scheduler::default();
        scheduler.update(secs(0.0), &timing(), 2, all_ready);
        scheduler.update(secs(1.0), &timing(), 0, all_ready);
        assert!(scheduler.current().is_none());
    }
}
