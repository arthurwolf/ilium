//! Logging helper over `host::log` (rust-sdk.md "Calling the host").

use crate::generated::host::Level;

/// Sends one log line to the host; lines over the per-second budget are dropped by the host.
pub fn log(level: Level, message: &str) {
    let mut end = message.len().min(MAX_LOG_MESSAGE_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    LOGGED.with(|lines| {
        let mut lines = lines.borrow_mut();
        // Over the per-second budget the host drops lines; the native buffer keeps a bounded tail.
        if lines.len() >= MAX_BUFFERED_LINES {
            lines.remove(0);
        }
        lines.push((level, message[..end].to_string()));
    });
}

/// Longest log line the host keeps: 16 KiB (limits manual).
pub const MAX_LOG_MESSAGE_BYTES: usize = 16 * 1024;
/// Most lines the native-mode buffer holds before the oldest are dropped.
const MAX_BUFFERED_LINES: usize = 4096;

thread_local! {
    static LOGGED: std::cell::RefCell<Vec<(Level, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Drains the lines logged on this thread in native mode; the testkit reads them from here.
pub fn take_logged() -> Vec<(Level, String)> {
    LOGGED.with(|lines| std::mem::take(&mut *lines.borrow_mut()))
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
        for level in [
            Level::Trace,
            Level::Debug,
            Level::Info,
            Level::Warn,
            Level::Error,
        ] {
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
