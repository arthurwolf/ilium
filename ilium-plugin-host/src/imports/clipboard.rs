//! Host implementation of `ilium:host/clipboard` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostClipboard` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::clipboard::HostClipboard;
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `clipboard` interface.
#[derive(Debug)]
pub struct ClipboardImport;

impl HostClipboard for ClipboardImport {
    fn read_text(&mut self) -> Option<String> {
        unimplemented!("host::imports::clipboard::read_text")
    }

    fn write_text(&mut self, _text: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::clipboard::write_text")
    }
}

#[cfg(test)]
mod tests {
    use super::ClipboardImport;
    use ilium_plugin_api::generated::host::clipboard::HostClipboard;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

    /// Largest text `write-text` accepts, in UTF-8 bytes (api-clipboard, "Write limit").
    const WRITE_LIMIT_BYTES: usize = 1_048_576;

    /// Largest text the security policy accepts for `clipboard.write` (SEC-044).
    const SECURITY_WRITE_LIMIT_BYTES: usize = 64 * 1024;

    /// Calls `write-text` on a fresh import, as the plugin side does.
    fn write(text: &str) -> Result<(), PluginError> {
        let mut clipboard = ClipboardImport;
        clipboard.write_text(text.to_string())
    }

    /// Asserts that the call returned an `invalid-argument` error.
    fn assert_invalid_argument(result: Result<(), PluginError>) {
        match result {
            Err(error) => assert_eq!(error.code, ErrorCode::InvalidArgument, "message: {}", error.message),
            Ok(()) => panic!("expected invalid-argument, got Ok"),
        }
    }

    /// REQ: SEC-044
    #[test]
    fn write_plain_text_returns_ok() {
        assert_eq!(write("hello"), Ok(()));
    }

    /// REQ: SEC-044
    #[test]
    fn write_empty_text_is_accepted() {
        assert_eq!(write(""), Ok(()));
    }

    /// REQ: SEC-044
    #[test]
    fn write_text_with_tab_newline_and_carriage_return_is_accepted() {
        assert_eq!(write("col a\tcol b\r\nline two\n"), Ok(()));
    }

    /// REQ: SEC-044
    #[test]
    fn write_text_containing_nul_is_refused() {
        assert_invalid_argument(write("a\u{0}b"));
    }

    /// REQ: SEC-044
    #[test]
    fn write_text_containing_escape_sequence_is_refused() {
        assert_invalid_argument(write("\u{1b}[31mred"));
    }

    /// REQ: SEC-044
    #[test]
    fn write_text_containing_bell_or_delete_is_refused() {
        assert_invalid_argument(write("ring\u{7}"));
        assert_invalid_argument(write("gone\u{7f}"));
    }

    /// REQ: SEC-044
    #[test]
    fn write_text_of_exactly_64_kib_is_accepted() {
        let text = "a".repeat(SECURITY_WRITE_LIMIT_BYTES);
        assert_eq!(write(&text), Ok(()));
    }

    /// REQ: SEC-044
    #[test]
    fn write_text_one_byte_over_one_mib_is_refused() {
        let text = "a".repeat(WRITE_LIMIT_BYTES + 1);
        assert_invalid_argument(write(&text));
    }

    /// REQ: SEC-044
    #[test]
    fn write_limit_counts_utf8_bytes_not_characters() {
        // Each emoji is 4 UTF-8 bytes. 262,145 of them are 1,048,580 bytes: over the limit,
        // although the character count is far below 1 MiB.
        let text = "\u{1F600}".repeat(262_145);
        assert_invalid_argument(write(&text));
    }
}
