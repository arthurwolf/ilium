//! Logging helper over `host::log` (rust-sdk.md "Calling the host").

use crate::generated::host::Level;

/// Sends one log line to the host; lines over the per-second budget are dropped by the host.
pub fn log(_level: Level, _message: &str) {
    unimplemented!("rust-sdk.log")
}

#[cfg(test)]
mod spec_tests {
    use super::log;
    use crate::generated::host::Level;

    /// REQ: DEV-652
    #[test]
    fn level_case_names_follow_threshold_order() {
        assert_eq!(
            Level::CASE_NAMES,
            &["trace", "debug", "info", "warn", "error"]
        );
    }

    /// REQ: DEV-652
    #[test]
    fn log_accepts_every_level() {
        for level in [Level::Trace, Level::Debug, Level::Info, Level::Warn, Level::Error] {
            log(level, "level check");
        }
    }

    /// REQ: DEV-650
    #[test]
    fn log_passes_multi_line_message_through() {
        log(Level::Info, "first line\nsecond line");
    }

    /// REQ: DEV-650
    #[test]
    fn log_accepts_empty_message() {
        log(Level::Debug, "");
    }

    /// REQ: DEV-651
    #[test]
    fn log_accepts_message_over_16_kib() {
        let oversized_message = "x".repeat(20_000);
        log(Level::Warn, &oversized_message);
    }

    /// REQ: DEV-656
    #[test]
    fn log_burst_over_rate_limit_returns_normally() {
        for index in 0..1_000 {
            log(Level::Info, &format!("burst line {index}"));
        }
    }
}
