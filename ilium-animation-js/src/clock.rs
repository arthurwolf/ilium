//! A host-sampled animation clock; timestamps are monotonic offsets, not civil time.
use std::time::Duration; // Permit deterministic tests without sleeping or reading global clocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)] // Invalid updates leave the clock unchanged.
pub enum ClockError {
    // Keep errors independent of the runtime and platform.
    #[error("animation speed must be finite and nonnegative")] // Zero is a supported speed.
    InvalidSpeed, // Reject NaN, infinity, and reverse motion.
    #[error("monotonic timestamp moved backwards")] // Never silently saturate a negative interval.
    Backwards, // The host must use one monotonic timestamp domain.
    #[error("animation clock overflow")] // Refuse an unrepresentable accumulated value.
    Overflow, // No infinity reaches JavaScript.
} // Civil clocks and permission-sensitive inputs are separate services.
#[derive(Clone, Copy, Debug, PartialEq)] // Values map to context.time/wall/delta/wall_delta.
pub struct ClockSample {
    pub time: f64,
    pub wall: f64,
    pub delta: f64,
    pub wall_delta: f64,
    pub suspended: bool,
} // Seconds throughout.
#[derive(Clone, Debug)] // The owner may inspect or checkpoint a clock without sharing mutable state.
pub struct AnimationClock {
    // Integrate each interval using the speed active during that interval.
    last_stamp: Duration,
    active: Duration,
    scaled: f64,
    speed: f64,
    suspended: bool, // Current integration state.
    sampled_active: Duration,
    sampled_scaled: f64, // Delta origins advance only when sampled.
} // No Instant or wall-clock dependency is hidden in the implementation.
impl AnimationClock {
    // All methods are called by the owning animation worker.
    pub fn new(now: Duration, speed: f64) -> Result<Self, ClockError> {
        // Establish the timestamp origin.
        Self::check_speed(speed)?; // Validate before creating usable state.
        Ok(Self {
            last_stamp: now,
            active: Duration::ZERO,
            scaled: 0.0,
            speed,
            suspended: false,
            sampled_active: Duration::ZERO,
            sampled_scaled: 0.0,
        }) // Start with zero elapsed time.
    } // The first sample includes only time after this origin.
    fn check_speed(speed: f64) -> Result<(), ClockError> {
        // Resource policy may impose a narrower speed range.
        if !speed.is_finite() || speed < 0.0 {
            return Err(ClockError::InvalidSpeed);
        } // Guard malformed settings.
        Ok(()) // Both ordinary speed and zero speed are valid.
    } // Speed changes never rescale elapsed history.
    fn advance(&mut self, now: Duration) -> Result<(), ClockError> {
        // Compute a complete candidate before mutation.
        let elapsed = now
            .checked_sub(self.last_stamp)
            .ok_or(ClockError::Backwards)?; // Reject another timestamp domain.
        let step = if self.suspended {
            Duration::ZERO
        } else {
            elapsed
        }; // Suspension excludes active elapsed time.
        let active = self.active.checked_add(step).ok_or(ClockError::Overflow)?; // Keep unscaled accumulation exact.
        let scaled = self.scaled + step.as_secs_f64() * self.speed; // Apply the previous interval's speed.
        if !scaled.is_finite() {
            return Err(ClockError::Overflow);
        } // Overflow does not consume the timestamp.
        self.last_stamp = now;
        self.active = active;
        self.scaled = scaled; // Publish the valid candidate atomically.
        Ok(()) // Sampling and control changes share this one integration path.
    } // A failed update leaves all fields unchanged.
    pub fn set_speed(&mut self, now: Duration, speed: f64) -> Result<(), ClockError> {
        // A setting takes effect at this timestamp.
        Self::check_speed(speed)?;
        self.advance(now)?; // Finish the old-speed interval first.
        self.speed = speed;
        Ok(()) // Future active intervals use the new speed.
    } // Changes made while suspended affect only resumed motion.
    pub fn suspend(&mut self, now: Duration) -> Result<(), ClockError> {
        // Repeated suspension is idempotent.
        self.advance(now)?;
        self.suspended = true;
        Ok(()) // Account for motion up to the first suspension.
    } // Suspension does not dispose the scene or acknowledge a frame.
    pub fn resume(&mut self, now: Duration) -> Result<(), ClockError> {
        // Consume the paused timestamp gap without motion.
        self.advance(now)?;
        self.suspended = false;
        Ok(()) // Resume from the current timestamp.
    } // Repeated resume accounts for ordinary active elapsed time.
    pub fn sample(&mut self, now: Duration) -> Result<ClockSample, ClockError> {
        // Read one coherent render-time sample.
        self.advance(now)?; // Control changes already integrated their preceding intervals.
        let sample = ClockSample {
            time: self.scaled,
            wall: self.active.as_secs_f64(),
            delta: self.scaled - self.sampled_scaled,
            wall_delta: (self.active - self.sampled_active).as_secs_f64(),
            suspended: self.suspended,
        }; // Never derive delta from render cadence.
        self.sampled_active = self.active;
        self.sampled_scaled = self.scaled; // Commit this sample's delta origins.
        Ok(sample) // A dropped presentation does not rewind the simulation clock.
    } // Civil UTC/timezone data is intentionally absent.
    pub fn reset(&mut self, now: Duration, speed: f64) -> Result<(), ClockError> {
        // Explicit scene reset establishes a new origin.
        if now < self.last_stamp {
            return Err(ClockError::Backwards);
        } // Reset cannot hide a backwards host timestamp.
        let replacement = Self::new(now, speed)?; // Validate without touching the old clock.
        *self = replacement;
        Ok(()) // Reset elapsed values and suspension together.
    } // Replay may drive these methods with deterministic monotonic sample offsets.
} // End the complete clock implementation.
#[cfg(test)] // No real time, threads, or timers are needed for these tests.
mod tests {
    // Cover integration boundaries rather than a single constant-speed example.
    use super::*; // Use the same public API as the worker.
    fn at(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    } // Deterministic host timestamps.
    #[test] // A new speed changes only future increments.
    fn speed_changes_integrate_history() {
        // Two seconds at 1x plus three seconds at 2x equals eight.
        let mut clock = AnimationClock::new(at(10), 1.0).unwrap(); // Nonzero process offset is harmless.
        clock.set_speed(at(12), 2.0).unwrap();
        let sample = clock.sample(at(15)).unwrap(); // Integrate both intervals.
        assert_eq!(
            (sample.time, sample.wall, sample.delta, sample.wall_delta),
            (8.0, 5.0, 8.0, 5.0)
        ); // No retroactive multiplication.
        assert_eq!(clock.sample(at(15)).unwrap().delta, 0.0); // Sampling the same timestamp is stable.
    } // Render cadence does not affect accumulated time.
    #[test] // Paused gaps and paused speed changes cannot create a resume jump.
    fn suspension_and_zero_speed_preserve_distinct_clocks() {
        // Zero speed still accumulates active unscaled time.
        let mut clock = AnimationClock::new(at(0), 1.0).unwrap();
        clock.suspend(at(2)).unwrap(); // Two active seconds.
        clock.suspend(at(5)).unwrap();
        clock.set_speed(at(8), 2.0).unwrap();
        clock.resume(at(10)).unwrap(); // Eight paused seconds.
        let sample = clock.sample(at(13)).unwrap();
        assert_eq!((sample.time, sample.wall), (8.0, 5.0)); // Only resumed active time advances.
        clock.set_speed(at(13), 0.0).unwrap();
        let sample = clock.sample(at(16)).unwrap(); // Freeze motion without suspending inputs.
        assert_eq!((sample.delta, sample.wall_delta), (0.0, 3.0)); // Civil/input cadence can remain unscaled.
    } // Active wall excludes suspension; civil time remains independently supplied.
    #[test] // Rejected control updates cannot partially integrate the clock.
    fn invalid_updates_roll_back() {
        // Exercise timestamp and speed validation.
        let mut clock = AnimationClock::new(at(10), 1.0).unwrap(); // Stable initial state.
        assert_eq!(
            clock.set_speed(at(12), f64::NAN),
            Err(ClockError::InvalidSpeed)
        ); // Invalid speed does not consume time.
        assert_eq!(clock.sample(at(9)), Err(ClockError::Backwards)); // Invalid timestamp does not rewind time.
        assert_eq!(clock.sample(at(12)).unwrap().time, 2.0); // The original interval remains intact.
        assert!(clock.reset(at(11), 1.0).is_err());
        clock.reset(at(12), 3.0).unwrap(); // Explicit reset also validates time.
        assert_eq!(clock.sample(at(13)).unwrap().time, 3.0); // Reset establishes a fresh origin.
    } // All error paths preserve previously usable state.
    #[test] // An overflowing interval cannot publish nonfinite values.
    fn overflow_leaves_the_timestamp_retryable() {
        // Use a finite speed whose two-second product overflows.
        let mut clock = AnimationClock::new(at(0), f64::MAX).unwrap(); // The setting itself is finite.
        assert_eq!(clock.sample(at(2)), Err(ClockError::Overflow)); // Reject the unrepresentable accumulated value.
        clock.set_speed(at(0), 1.0).unwrap();
        assert_eq!(clock.sample(at(2)).unwrap().time, 2.0); // Failure did not advance the origin.
    } // Parent policy can reject extreme speeds earlier without changing these semantics.
} // End clock regression tests.
