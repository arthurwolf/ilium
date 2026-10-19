//! Fake clock (reference-test-harness.md "Fake clock"). Time zone is UTC.

/// Read handle on the fake clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FakeClock {
    instant_ms: u64,
}

impl FakeClock {
    /// A clock at the given fake instant, milliseconds since the Unix epoch.
    pub fn at(instant_ms: u64) -> FakeClock {
        FakeClock { instant_ms }
    }

    /// Current fake instant in milliseconds since the Unix epoch.
    pub fn now(&self) -> u64 {
        self.instant_ms
    }
}
