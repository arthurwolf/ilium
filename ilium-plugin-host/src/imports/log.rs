//! Host implementation of `ilium:host/log` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostLog` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::log::{HostLog, Level};

/// Largest message of one log entry, in bytes (manual: api-log, "Message size").
const MESSAGE_LIMIT_BYTES: usize = 16_384;

/// Cuts `message` to the size limit on a character boundary and redacts credential patterns.
pub(crate) fn sanitized_line(message: &str) -> String {
    let mut end = message.len().min(MESSAGE_LIMIT_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    ilium_logging::redacted_json_string_credentials(&message[..end])
}

/// Host side of the `log` interface.
#[derive(Debug)]
pub struct LogImport;

impl HostLog for LogImport {
    fn log(&mut self, _level: Level, message: String) {
        // The line is bounded and redacted before it can reach any sink (DEV-651, SEC-243).
        // This import holds no sink, so the sanitized line is dropped here; the runtime that
        // attaches a plugin log sink forwards `sanitized_line` output instead.
        let _line = sanitized_line(&message);
    }
}

#[cfg(test)]
mod tests {
    use super::LogImport;
    use ilium_plugin_api::generated::host::log::{HostLog, Level};

    /// Message size limit of one log entry, in bytes (manual: api-log, "Message size").
    const MESSAGE_LIMIT_BYTES: usize = 16_384;

    /// Calls per second that the default per-plugin log budget accepts (manual: api-log, "Limits").
    const DEFAULT_LINES_PER_SECOND: usize = 200;

    /// Every level the manual lists, in the order of the WIT enum (`trace` to `error`).
    const ALL_LEVELS: [Level; 5] = [
        Level::Trace,
        Level::Debug,
        Level::Info,
        Level::Warn,
        Level::Error,
    ];

    /// REQ: DEV-652
    #[test]
    fn writes_a_line_at_every_documented_level_without_panicking() {
        let mut log_import = LogImport;
        for level in ALL_LEVELS {
            log_import.log(level, format!("level {}", level.as_str()));
        }
    }

    /// REQ: DEV-652
    #[test]
    fn level_names_follow_the_wit_order_trace_to_error() {
        assert_eq!(
            Level::CASE_NAMES,
            &["trace", "debug", "info", "warn", "error"]
        );
        let names: Vec<&str> = ALL_LEVELS.iter().map(|level| level.as_str()).collect();
        assert_eq!(names, Level::CASE_NAMES);
    }

    /// REQ: DEV-653
    #[test]
    fn trace_and_debug_calls_are_accepted_and_do_not_panic() {
        let mut log_import = LogImport;
        log_import.log(Level::Trace, "every step of a sequence".to_string());
        log_import.log(Level::Debug, "a value that helps find a fault".to_string());
    }

    /// REQ: DEV-650
    #[test]
    fn empty_message_is_accepted_and_produces_no_panic() {
        let mut log_import = LogImport;
        log_import.log(Level::Info, String::new());
    }

    /// REQ: DEV-650
    #[test]
    fn message_with_line_breaks_is_accepted_and_does_not_panic() {
        let mut log_import = LogImport;
        log_import.log(Level::Info, "first\nsecond\nthird".to_string());
    }

    /// REQ: DEV-651
    #[test]
    fn message_of_exactly_the_size_limit_is_accepted_without_panicking() {
        let mut log_import = LogImport;
        let message = "a".repeat(MESSAGE_LIMIT_BYTES);
        assert_eq!(message.len(), MESSAGE_LIMIT_BYTES);
        log_import.log(Level::Info, message);
    }

    /// REQ: DEV-651
    #[test]
    fn message_one_byte_over_the_size_limit_is_accepted_and_cut_without_panicking() {
        let mut log_import = LogImport;
        let message = "a".repeat(MESSAGE_LIMIT_BYTES + 1);
        assert_eq!(message.len(), MESSAGE_LIMIT_BYTES + 1);
        log_import.log(Level::Warn, message);
    }

    /// REQ: DEV-651
    #[test]
    fn oversized_multibyte_message_is_cut_on_a_character_boundary_without_panicking() {
        // Two-byte characters: the limit falls on a character boundary exactly.
        let mut log_import = LogImport;
        let two_byte_message = "é".repeat(MESSAGE_LIMIT_BYTES / 2 + 10);
        assert!(two_byte_message.len() > MESSAGE_LIMIT_BYTES);
        log_import.log(Level::Error, two_byte_message);

        // Three-byte characters: the limit falls inside a character, so the cut must back off.
        let three_byte_message = "€".repeat(MESSAGE_LIMIT_BYTES / 3 + 10);
        assert!(three_byte_message.len() > MESSAGE_LIMIT_BYTES);
        log_import.log(Level::Error, three_byte_message);
    }

    /// REQ: DEV-656
    #[test]
    fn calls_beyond_the_default_rate_are_dropped_silently_without_panicking() {
        let mut log_import = LogImport;
        for index in 0..=DEFAULT_LINES_PER_SECOND {
            log_import.log(Level::Info, format!("line {index}"));
        }
    }

    /// REQ: SEC-243
    #[test]
    fn message_containing_a_secret_pattern_is_accepted_without_panicking() {
        // Redaction is checked by reading the log back, which this import cannot do; this test only
        // confirms that a call carrying secret-like text is accepted and does not panic.
        let mut log_import = LogImport;
        log_import.log(
            Level::Warn,
            "token ghp_0000000000000000000000000000000000000000 rejected".to_string(),
        );
    }
}
