//! Small helpers shared by the telemetry tests.

use std::time::Duration;

use ilium_plugin_host::calls::CallClass;

/// Every call class of the call-class table (reference-runtime-model.md).
pub const ALL_CLASSES: [CallClass; 10] = [
    CallClass::Event,
    CallClass::Command,
    CallClass::Render,
    CallClass::Annotate,
    CallClass::Frame,
    CallClass::Detector,
    CallClass::Activate,
    CallClass::Deactivate,
    CallClass::Migrate,
    CallClass::Job,
];

/// Milliseconds as a duration, exact to the microsecond.
pub fn us(micros: u64) -> Duration {
    Duration::from_micros(micros)
}

/// Whole milliseconds as a duration.
pub fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// One hour in milliseconds.
pub const HOUR_MS: u64 = 3_600_000;
