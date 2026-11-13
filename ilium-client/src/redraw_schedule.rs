//! Client redraw timing: output cadence and temporary frame-storage refusal.
use std::time::{Duration, Instant};

pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(33);

pub(crate) struct RedrawSchedule {
    last_draw_at: Instant,
    retry_at: Option<Instant>,
    refusal_started_at: Option<Instant>,
    last_warning_at: Option<Instant>,
    reported_refusal: bool,
}

impl RedrawSchedule {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            last_draw_at: now,
            retry_at: None,
            refusal_started_at: None,
            last_warning_at: None,
            reported_refusal: false,
        }
    }

    pub(crate) fn delay(&self, immediate: bool, now: Instant) -> Duration {
        let cadence = if immediate {
            Duration::ZERO
        } else {
            FRAME_INTERVAL.saturating_sub(now.saturating_duration_since(self.last_draw_at))
        };
        // Input bypasses the output cadence, never a failed admission's retry.
        let retry = self.retry_at.map_or(Duration::ZERO, |deadline| {
            deadline.saturating_duration_since(now)
        });
        cadence.max(retry)
    }

    /// Return a content-free warning observation only for a prolonged refusal.
    pub(crate) fn admission_refused(&mut self, now: Instant) -> Option<Duration> {
        self.retry_at = Some(now + FRAME_INTERVAL);
        let started = *self.refusal_started_at.get_or_insert(now);
        let elapsed = now.saturating_duration_since(started);
        if elapsed < Duration::from_millis(250)
            || self.last_warning_at.is_some_and(|previous| {
                now.saturating_duration_since(previous) < Duration::from_secs(5)
            })
        {
            return None;
        }
        self.last_warning_at = Some(now);
        self.reported_refusal = true;
        Some(elapsed)
    }

    pub(crate) fn submitted(&mut self, now: Instant) -> Option<Duration> {
        let recovered = self.refusal_started_at.take().and_then(|started| {
            self.reported_refusal
                .then(|| now.saturating_duration_since(started))
        });
        self.last_draw_at = now;
        self.retry_at = None;
        self.reported_refusal = false;
        // Keep last_warning_at across episodes: intermittent pressure cannot spam logs.
        recovered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_due_redraw_has_a_positive_retry_delay() {
        let started = Instant::now();
        let mut schedule = RedrawSchedule::new(started);
        let now = started + FRAME_INTERVAL;
        let _ = schedule.admission_refused(now);

        for immediate in [false, true] {
            assert_eq!(schedule.delay(immediate, now), FRAME_INTERVAL);
            assert_eq!(
                schedule.delay(immediate, now + Duration::from_millis(32)),
                Duration::from_millis(1)
            );
            assert_eq!(
                schedule.delay(immediate, now + FRAME_INTERVAL),
                Duration::ZERO
            );
        }
    }

    #[test]
    fn ready_input_flood_cannot_repeatedly_retry_refused_frame() {
        let started = Instant::now();
        let mut schedule = RedrawSchedule::new(started);
        let mut attempts = 0;
        for micros in 0..10_000 {
            let now = started + Duration::from_micros(micros);
            if schedule.delay(true, now).is_zero() {
                attempts += 1;
                let _ = schedule.admission_refused(now);
            }
        }
        assert_eq!(
            attempts, 1,
            "ready events must not cause repeated storage admission attempts"
        );
    }

    #[test]
    fn successful_retry_restores_normal_output_and_input_cadence() {
        let started = Instant::now();
        let mut schedule = RedrawSchedule::new(started);
        let _ = schedule.admission_refused(started);
        let submitted = started + FRAME_INTERVAL;
        let _ = schedule.submitted(submitted);
        assert_eq!(schedule.delay(false, submitted), FRAME_INTERVAL);
        assert_eq!(schedule.delay(true, submitted), Duration::ZERO);
    }

    #[test]
    fn quiet_retries_and_late_failures_keep_a_fresh_deadline() {
        let started = Instant::now();
        for immediate in [false, true] {
            let mut schedule = RedrawSchedule::new(started);
            let mut now = started + FRAME_INTERVAL;
            for _ in 0..4 {
                assert!(schedule.delay(immediate, now).is_zero());
                let _ = schedule.admission_refused(now);
                assert_eq!(schedule.delay(immediate, now), FRAME_INTERVAL);
                now += FRAME_INTERVAL;
            }
            let late = now + Duration::from_secs(1);
            let _ = schedule.admission_refused(late);
            assert_eq!(schedule.delay(immediate, late), FRAME_INTERVAL);
        }
    }

    #[test]
    fn warning_throttle_survives_recovery_and_episodes_reset() {
        let started = Instant::now();
        let mut schedule = RedrawSchedule::new(started);
        assert!(schedule.admission_refused(started).is_none());
        assert!(schedule
            .admission_refused(started + Duration::from_millis(249))
            .is_none());
        let warned = started + Duration::from_millis(250);
        assert_eq!(
            schedule.admission_refused(warned),
            Some(Duration::from_millis(250))
        );
        assert!(schedule.submitted(warned + FRAME_INTERVAL).is_some());
        assert!(schedule.submitted(warned + FRAME_INTERVAL).is_none());
        let next = warned + Duration::from_secs(1);
        assert!(schedule.admission_refused(next).is_none());
        assert!(schedule
            .admission_refused(next + Duration::from_millis(250))
            .is_none());
        assert!(schedule
            .submitted(next + Duration::from_millis(251))
            .is_none());
        assert_eq!(schedule.last_warning_at, Some(warned));
        assert_eq!(schedule.refusal_started_at, None);
    }
}
