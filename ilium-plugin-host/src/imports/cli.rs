//! Host implementation of `ilium:host/cli` (api-cli; capability-cli-contribute).
//!
//! Tracks, per invocation id, whether the invocation has finished and how much standard input it
//! has requested. Output is validated here; delivery to the terminal belongs to the CLI router.

use ilium_plugin_api::generated::host::cli::{HostCli, PaneSelection};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::NodeId;
use std::collections::{HashMap, HashSet};

/// Largest text one output call accepts, in bytes (api-cli "Output forms").
const OUTPUT_LIMIT_BYTES: usize = 64 * 1024;
/// Total standard input one invocation may read (api-cli "Standard input").
const STDIN_TOTAL_LIMIT_BYTES: u64 = 4 * 1024 * 1024;

/// Host side of the `cli` interface.
#[derive(Debug, Default)]
pub struct CliImport {
    finished: HashSet<u64>,
    stdin_requested: HashMap<u64, u64>,
    stdin_requests: HashMap<u64, Vec<u8>>,
    next_request: u64,
}

impl CliImport {
    fn ensure_open(&self, invocation: u64) -> Result<(), PluginError> {
        if self.finished.contains(&invocation) {
            return Err(PluginError::new(
                ErrorCode::NotFound,
                "the invocation has already finished",
            ));
        }
        Ok(())
    }

    fn check_text(&self, invocation: u64, text: &str) -> Result<(), PluginError> {
        self.ensure_open(invocation)?;
        if text.len() > OUTPUT_LIMIT_BYTES {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "output is larger than 64 KiB",
            ));
        }
        Ok(())
    }
}

fn invalid(message: &str) -> PluginError {
    PluginError::new(ErrorCode::InvalidArgument, message)
}

impl HostCli for CliImport {
    fn write_line(&mut self, invocation: u64, jsonl: String) -> Result<(), PluginError> {
        self.check_text(invocation, &jsonl)?;
        if jsonl.contains('\n') || jsonl.contains('\r') {
            return Err(invalid("a JSONL line must not contain a raw newline"));
        }
        let value: serde_json::Value = serde_json::from_str(&jsonl)
            .map_err(|error| invalid(&format!("line is not valid JSON: {error}")))?;
        let has_string_type = value
            .as_object()
            .and_then(|object| object.get("type"))
            .is_some_and(serde_json::Value::is_string);
        if !has_string_type {
            return Err(invalid("a JSONL line must be an object with a string \"type\""));
        }
        Ok(())
    }

    fn write_error(&mut self, invocation: u64, text: String) -> Result<(), PluginError> {
        self.check_text(invocation, &text)
    }

    fn write_text(&mut self, invocation: u64, text: String) -> Result<(), PluginError> {
        self.check_text(invocation, &text)
    }

    fn write_stderr(&mut self, invocation: u64, text: String) -> Result<(), PluginError> {
        self.check_text(invocation, &text)
    }

    fn finish(&mut self, invocation: u64, _exit_code: u8) {
        self.finished.insert(invocation);
    }

    fn read_stdin(&mut self, invocation: u64, max_bytes: u32) -> Result<u64, PluginError> {
        self.ensure_open(invocation)?;
        if max_bytes == 0 {
            return Err(invalid("max-bytes must be at least 1"));
        }
        let requested = self.stdin_requested.entry(invocation).or_insert(0);
        if *requested >= STDIN_TOTAL_LIMIT_BYTES {
            return Err(PluginError::new(
                ErrorCode::QuotaExceeded,
                "the invocation has used its 4 MiB of standard input",
            ));
        }
        *requested = (*requested + u64::from(max_bytes)).min(STDIN_TOTAL_LIMIT_BYTES);
        self.next_request += 1;
        let request = self.next_request;
        // No stdin source is attached to this import: the read completes at once with end of input.
        self.stdin_requests.insert(request, Vec::new());
        Ok(request)
    }

    fn take_stdin(&mut self, request: u64) -> Option<Vec<u8>> {
        self.stdin_requests.remove(&request)
    }

    fn is_tty(&mut self, _invocation: u64) -> (bool, bool) {
        (false, false)
    }

    fn caller_pane(&mut self, _invocation: u64) -> Option<NodeId> {
        None
    }

    fn selected_panes(&mut self, _invocation: u64) -> Option<PaneSelection> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::CliImport;
    use ilium_plugin_api::generated::host::cli::HostCli;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

    /// Invocation id used by every test. The host assigns ids; this module only receives them.
    const INVOCATION: u64 = 1;

    /// Largest text one `write-line`, `write-text` or `write-error` call accepts, in bytes
    /// (api-cli "Output forms"; capability-cli-contribute "Functions": over 64 KiB is invalid-argument).
    const OUTPUT_LIMIT_BYTES: usize = 64 * 1024;

    /// Total standard input one invocation may read (api-cli "Standard input": at most 4 MiB).
    const STDIN_TOTAL_LIMIT_BYTES: u32 = 4 * 1024 * 1024;

    /// Fixed head and tail of the padded JSON object built by `jsonl_object_of_len`.
    const OBJECT_HEAD: &str = r#"{"type":"result","pad":""#;
    const OBJECT_TAIL: &str = r#""}"#;

    /// Builds one JSON object with a string `type` field whose UTF-8 length is exactly `total_bytes`.
    fn jsonl_object_of_len(total_bytes: usize) -> String {
        let padding = total_bytes - OBJECT_HEAD.len() - OBJECT_TAIL.len();
        format!("{OBJECT_HEAD}{}{OBJECT_TAIL}", "a".repeat(padding))
    }

    /// Returns the value of an `Ok` result and panics with the error code and message otherwise.
    fn expect_ok<T>(result: Result<T, PluginError>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected Ok, got {:?}: {}", error.code, error.message),
        }
    }

    /// Asserts that the result is an error with the given code.
    fn expect_code<T: std::fmt::Debug>(result: Result<T, PluginError>, expected: ErrorCode) {
        match result {
            Err(error) => assert_eq!(error.code, expected, "message: {}", error.message),
            Ok(value) => panic!("expected {expected:?}, got Ok({value:?})"),
        }
    }

    /// REQ: API-005
    #[test]
    fn write_line_accepts_one_object_with_string_type() {
        let mut cli = CliImport::default();
        expect_ok(cli.write_line(INVOCATION, r#"{"type":"result","ok":true}"#.to_string()));
    }

    /// REQ: API-062
    #[test]
    fn write_line_accepts_object_of_exactly_64_kib() {
        let mut cli = CliImport::default();
        expect_ok(cli.write_line(INVOCATION, jsonl_object_of_len(OUTPUT_LIMIT_BYTES)));
    }

    /// REQ: API-062
    #[test]
    fn write_line_rejects_object_one_byte_over_64_kib() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, jsonl_object_of_len(OUTPUT_LIMIT_BYTES + 1)),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_empty_input() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, String::new()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_object_without_type() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, r#"{"name":"notes"}"#.to_string()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_non_string_type() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, r#"{"type":1}"#.to_string()),
            ErrorCode::InvalidArgument,
        );
        expect_code(
            cli.write_line(INVOCATION, r#"{"type":null}"#.to_string()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_json_that_is_not_an_object() {
        let mut cli = CliImport::default();
        for text in ["[]", r#""text""#, "42", "null", "true"] {
            expect_code(
                cli.write_line(INVOCATION, text.to_string()),
                ErrorCode::InvalidArgument,
            );
        }
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_raw_newline_inside_or_after_the_object() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, "{\"type\":\"result\"}\n".to_string()),
            ErrorCode::InvalidArgument,
        );
        expect_code(
            cli.write_line(INVOCATION, "{\"type\":\n\"result\"}".to_string()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_rejects_malformed_json() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_line(INVOCATION, r#"{"type":"#.to_string()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_line_after_finish_returns_not_found() {
        let mut cli = CliImport::default();
        cli.finish(INVOCATION, 0);
        expect_code(
            cli.write_line(INVOCATION, r#"{"type":"result"}"#.to_string()),
            ErrorCode::NotFound,
        );
    }

    /// REQ: API-062
    #[test]
    fn write_text_rejects_text_one_byte_over_64_kib() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_text(INVOCATION, "t".repeat(OUTPUT_LIMIT_BYTES + 1)),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_text_after_finish_returns_not_found() {
        let mut cli = CliImport::default();
        cli.finish(INVOCATION, 0);
        expect_code(
            cli.write_text(INVOCATION, "hello".to_string()),
            ErrorCode::NotFound,
        );
    }

    /// REQ: API-062
    #[test]
    fn write_error_accepts_text_of_exactly_64_kib() {
        let mut cli = CliImport::default();
        expect_ok(cli.write_error(INVOCATION, "e".repeat(OUTPUT_LIMIT_BYTES)));
    }

    /// REQ: API-062
    #[test]
    fn write_error_rejects_text_one_byte_over_64_kib() {
        let mut cli = CliImport::default();
        expect_code(
            cli.write_error(INVOCATION, "e".repeat(OUTPUT_LIMIT_BYTES + 1)),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-005
    #[test]
    fn write_error_accepts_plain_text_message() {
        let mut cli = CliImport::default();
        expect_ok(cli.write_error(INVOCATION, "hello: pass --pane or --all-panes".to_string()));
    }

    /// REQ: API-005
    #[test]
    fn write_error_after_finish_returns_not_found() {
        let mut cli = CliImport::default();
        cli.finish(INVOCATION, 1);
        expect_code(
            cli.write_error(INVOCATION, "late".to_string()),
            ErrorCode::NotFound,
        );
    }

    /// REQ: API-005
    #[test]
    fn finish_unknown_or_repeated_invocation_is_ignored() {
        let mut cli = CliImport::default();
        cli.finish(INVOCATION, 0);
        cli.finish(INVOCATION, 0);
        cli.finish(9_999, 0);
    }

    /// REQ: API-005
    #[test]
    fn finish_accepts_largest_exit_code() {
        let mut cli = CliImport::default();
        cli.finish(INVOCATION, u8::MAX);
    }

    /// REQ: API-005
    #[test]
    fn read_stdin_accepts_smallest_request_of_one_byte() {
        let mut cli = CliImport::default();
        expect_ok(cli.read_stdin(INVOCATION, 1));
    }

    /// REQ: API-061
    #[test]
    fn read_stdin_accepts_full_four_mib_total_in_one_call() {
        let mut cli = CliImport::default();
        expect_ok(cli.read_stdin(INVOCATION, STDIN_TOTAL_LIMIT_BYTES));
    }

    /// REQ: API-061
    #[test]
    fn read_stdin_beyond_four_mib_total_returns_quota_exceeded() {
        let mut cli = CliImport::default();
        expect_ok(cli.read_stdin(INVOCATION, STDIN_TOTAL_LIMIT_BYTES));
        expect_code(cli.read_stdin(INVOCATION, 1), ErrorCode::QuotaExceeded);
    }
}
