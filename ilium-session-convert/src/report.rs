//! Wraps the caller's event sink so every step, log line and progress value
//! goes through one place that keeps progress monotonic and polls
//! cancellation.

use crate::error::ConvertError;
use crate::ConversionEvent;

pub(crate) struct Reporter<'a> {
    sink: &'a mut (dyn FnMut(ConversionEvent) + 'a),
    is_cancelled: &'a dyn Fn() -> bool,
    total_steps: usize,
    last_progress: f32,
}

impl<'a> Reporter<'a> {
    pub(crate) fn new(
        sink: &'a mut (dyn FnMut(ConversionEvent) + 'a),
        is_cancelled: &'a dyn Fn() -> bool,
        total_steps: usize,
    ) -> Self {
        Self {
            sink,
            is_cancelled,
            total_steps,
            last_progress: 0.0,
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        (self.is_cancelled)()
    }

    /// Returns `Err(Cancelled)` once cancellation was requested.
    pub(crate) fn check_cancel(&self) -> Result<(), ConvertError> {
        if self.is_cancelled() {
            Err(ConvertError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Starts a 1-based step and raises overall progress to `progress_floor`.
    pub(crate) fn step(&mut self, index: usize, title: &str, progress_floor: f32) {
        (self.sink)(ConversionEvent::Step {
            index,
            total: self.total_steps,
            title: title.to_string(),
        });
        self.progress(progress_floor);
    }

    pub(crate) fn log(&mut self, line: impl Into<String>) {
        let line: String = line.into();
        // A log entry is one line by contract; fold any stray line breaks.
        let line = line.replace(['\r', '\n'], " ");
        (self.sink)(ConversionEvent::Log(line));
    }

    /// Reports overall progress; values below the last reported one are
    /// raised to it and values are clamped to `0.0..=1.0`.
    pub(crate) fn progress(&mut self, value: f32) {
        let clamped = if value.is_nan() {
            self.last_progress
        } else {
            value.clamp(0.0, 1.0)
        };
        let monotonic = clamped.max(self.last_progress);
        if monotonic > self.last_progress {
            self.last_progress = monotonic;
            (self.sink)(ConversionEvent::Progress(monotonic));
        }
    }
}
