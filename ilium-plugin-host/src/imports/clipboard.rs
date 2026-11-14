//! Host implementation of `ilium:host/clipboard` (api-clipboard).
//!
//! The import owns the contract rules: the `clipboard.read` / `clipboard.write` grants, the 1 MiB
//! write limit, the refusal of control characters (SEC-044) and the 1 MiB cut of a read. What
//! reaches the desktop clipboard is a [`ClipboardBackend`] the composition root supplies
//! (`wiring::wave_two::ClientBackendInputs::clipboard`); the client routes it through its own
//! clipboard helper process. A process without a backend leaves the import unlinked, so the
//! default [`ClipboardImport`] keeps the text in memory (tests and the harness).

use std::sync::{Arc, Mutex, PoisonError};

use ilium_plugin_api::generated::host::clipboard::HostClipboard;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

use super::env::ImportEnv;

/// Largest text `write-text` accepts and `read-text` returns, in UTF-8 bytes (api-clipboard,
/// "Write limit" and "Plain text only").
pub const CLIPBOARD_TEXT_LIMIT_BYTES: usize = 1_048_576;

/// Grant that lets a plugin read the clipboard (high risk).
pub const READ_GRANT: &str = "clipboard.read";
/// Grant that lets a plugin write the clipboard.
pub const WRITE_GRANT: &str = "clipboard.write";

/// The desktop clipboard of a process. Calls block the plugin's executor thread, so a backend
/// answers within a short bound and fails with `timed-out` rather than waiting forever.
pub trait ClipboardBackend: Send + Sync {
    /// The plain text on the clipboard, `None` when it holds no text. `plugin` is the reading
    /// plugin, which the backend names in the client footer and the log.
    fn read_text(&self, plugin: &str) -> Result<Option<String>, PluginError>;

    /// Replaces the clipboard text. The import has already checked the size and the characters.
    fn write_text(&self, plugin: &str, text: &str) -> Result<(), PluginError>;
}

impl std::fmt::Debug for dyn ClipboardBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ClipboardBackend")
    }
}

/// A clipboard that lives in memory: what the last write put there.
#[derive(Debug, Default)]
pub struct MemoryClipboard {
    text: Mutex<Option<String>>,
}

impl MemoryClipboard {
    /// Puts `text` on the clipboard as another program would.
    pub fn set(&self, text: Option<String>) {
        *self.text.lock().unwrap_or_else(PoisonError::into_inner) = text;
    }

    /// The text on the clipboard.
    pub fn get(&self) -> Option<String> {
        self.text
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl ClipboardBackend for MemoryClipboard {
    fn read_text(&self, _plugin: &str) -> Result<Option<String>, PluginError> {
        Ok(self.get())
    }

    fn write_text(&self, _plugin: &str, text: &str) -> Result<(), PluginError> {
        self.set(Some(text.to_owned()));
        Ok(())
    }
}

/// Host side of the `clipboard` interface of one plugin instance.
#[derive(Debug)]
pub struct ClipboardImport {
    env: ImportEnv,
    backend: Arc<dyn ClipboardBackend>,
}

impl Default for ClipboardImport {
    /// Every grant held, and an in-memory clipboard.
    fn default() -> Self {
        Self::new(
            ImportEnv::unrestricted(),
            Arc::new(MemoryClipboard::default()),
        )
    }
}

impl ClipboardImport {
    /// The import of the instance `env` describes, reaching the clipboard through `backend`.
    pub fn new(env: ImportEnv, backend: Arc<dyn ClipboardBackend>) -> Self {
        Self { env, backend }
    }
}

impl HostClipboard for ClipboardImport {
    fn read_text(&mut self) -> Option<String> {
        // `read-text` has no error result: a missing grant, a failed backend and an empty
        // clipboard all read as nothing.
        if !self.env.holds(READ_GRANT) {
            return None;
        }
        let text = self.backend.read_text(self.env.plugin()).ok().flatten()?;
        Some(cut_at_char_boundary(text, CLIPBOARD_TEXT_LIMIT_BYTES))
    }

    fn write_text(&mut self, text: String) -> Result<(), PluginError> {
        if !self.env.holds(WRITE_GRANT) {
            return Err(refuse(
                ErrorCode::NotPermitted,
                "the clipboard.write grant is not held",
            ));
        }
        check_written_text(&text)?;
        self.backend.write_text(self.env.plugin(), &text)
    }
}

/// Refuses a text the clipboard must not receive: larger than 1 MiB, or holding a control
/// character other than tab, line feed and carriage return (those could inject terminal escape
/// sequences when pasted).
pub fn check_written_text(text: &str) -> Result<(), PluginError> {
    if text.len() > CLIPBOARD_TEXT_LIMIT_BYTES {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            "clipboard text is larger than 1 MiB",
        ));
    }
    let has_control = text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\t' | '\n' | '\r'));
    if has_control {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            "clipboard text contains a control character",
        ));
    }
    Ok(())
}

/// `text` cut to at most `limit` bytes, at a character boundary.
fn cut_at_char_boundary(mut text: String, limit: usize) -> String {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

/// Builds a refusal with the given code and a plain-text message.
fn refuse(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{ClipboardBackend, ClipboardImport, MemoryClipboard};
    use crate::imports::env::ImportEnv;
    use ilium_plugin_api::generated::host::clipboard::HostClipboard;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use std::sync::{Arc, Mutex};

    /// Largest text `write-text` accepts, in UTF-8 bytes (api-clipboard, "Write limit").
    const WRITE_LIMIT_BYTES: usize = 1_048_576;

    /// Largest text the security policy accepts for `clipboard.write` (SEC-044).
    const SECURITY_WRITE_LIMIT_BYTES: usize = 64 * 1024;

    /// Calls `write-text` on a fresh import, as the plugin side does.
    fn write(text: &str) -> Result<(), PluginError> {
        let mut clipboard = ClipboardImport::default();
        clipboard.write_text(text.to_string())
    }

    /// Asserts that the call returned an `invalid-argument` error.
    fn assert_invalid_argument(result: Result<(), PluginError>) {
        match result {
            Err(error) => assert_eq!(
                error.code,
                ErrorCode::InvalidArgument,
                "message: {}",
                error.message
            ),
            Ok(()) => panic!("expected invalid-argument, got Ok"),
        }
    }

    /// An import holding only `grants`, over `backend`.
    fn granted(grants: &[&str], backend: Arc<dyn ClipboardBackend>) -> ClipboardImport {
        let env = ImportEnv::unrestricted()
            .with_identity("test.clipboard", 1)
            .with_grants(grants.iter().map(|grant| (*grant).to_owned()).collect());
        ClipboardImport::new(env, backend)
    }

    /// Records which plugin called and fails every call when `failing` is set.
    #[derive(Default)]
    struct RecordingBackend {
        callers: Mutex<Vec<String>>,
        failing: bool,
    }

    impl ClipboardBackend for RecordingBackend {
        fn read_text(&self, plugin: &str) -> Result<Option<String>, PluginError> {
            self.callers
                .lock()
                .expect("callers")
                .push(plugin.to_owned());
            if self.failing {
                return Err(PluginError::new(ErrorCode::TimedOut, "slow clipboard"));
            }
            Ok(Some("copied".to_owned()))
        }

        fn write_text(&self, plugin: &str, _text: &str) -> Result<(), PluginError> {
            self.callers
                .lock()
                .expect("callers")
                .push(plugin.to_owned());
            if self.failing {
                return Err(PluginError::new(ErrorCode::Unsupported, "no route"));
            }
            Ok(())
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

    #[test]
    fn a_written_text_reaches_the_backend_and_reads_back() {
        let backend = Arc::new(MemoryClipboard::default());
        let mut clipboard = granted(
            &["clipboard.read", "clipboard.write"],
            Arc::clone(&backend) as Arc<dyn ClipboardBackend>,
        );
        clipboard
            .write_text("from the plugin".to_owned())
            .expect("written");
        assert_eq!(backend.get().as_deref(), Some("from the plugin"));
        assert_eq!(clipboard.read_text().as_deref(), Some("from the plugin"));
    }

    #[test]
    fn a_refused_text_never_reaches_the_backend() {
        let backend = Arc::new(RecordingBackend::default());
        let mut clipboard = granted(
            &["clipboard.write"],
            Arc::clone(&backend) as Arc<dyn ClipboardBackend>,
        );
        assert!(clipboard
            .write_text("\u{1b}]52;c;x\u{7}".to_owned())
            .is_err());
        assert!(backend.callers.lock().expect("callers").is_empty());
    }

    #[test]
    fn reading_without_the_read_grant_returns_nothing_and_never_asks_the_backend() {
        let backend = Arc::new(RecordingBackend::default());
        let mut clipboard = granted(
            &["clipboard.write"],
            Arc::clone(&backend) as Arc<dyn ClipboardBackend>,
        );
        assert_eq!(clipboard.read_text(), None);
        assert!(backend.callers.lock().expect("callers").is_empty());
    }

    #[test]
    fn writing_without_the_write_grant_is_not_permitted() {
        let mut clipboard = granted(
            &["clipboard.read"],
            Arc::new(MemoryClipboard::default()) as Arc<dyn ClipboardBackend>,
        );
        let refused = clipboard
            .write_text("text".to_owned())
            .expect_err("refused");
        assert_eq!(refused.code, ErrorCode::NotPermitted);
    }

    #[test]
    fn the_backend_learns_which_plugin_reads_and_a_failed_read_is_nothing() {
        let backend = Arc::new(RecordingBackend {
            failing: true,
            ..RecordingBackend::default()
        });
        let mut clipboard = granted(
            &["clipboard.read", "clipboard.write"],
            Arc::clone(&backend) as Arc<dyn ClipboardBackend>,
        );
        assert_eq!(clipboard.read_text(), None);
        let failed = clipboard
            .write_text("text".to_owned())
            .expect_err("no route");
        assert_eq!(failed.code, ErrorCode::Unsupported);
        assert_eq!(
            *backend.callers.lock().expect("callers"),
            vec!["test.clipboard".to_owned(), "test.clipboard".to_owned()]
        );
    }

    #[test]
    fn a_read_longer_than_one_mib_is_cut_at_a_character_boundary() {
        let backend = Arc::new(MemoryClipboard::default());
        // 262,145 four-byte characters: the 1 MiB cut falls inside the last one.
        backend.set(Some("\u{1F600}".repeat(262_145)));
        let mut clipboard = granted(
            &["clipboard.read"],
            Arc::clone(&backend) as Arc<dyn ClipboardBackend>,
        );
        let read = clipboard.read_text().expect("text");
        assert_eq!(read.len(), WRITE_LIMIT_BYTES);
        assert!(read.chars().all(|character| character == '\u{1F600}'));
    }

    #[test]
    fn an_empty_clipboard_reads_as_nothing() {
        let mut clipboard = granted(
            &["clipboard.read"],
            Arc::new(MemoryClipboard::default()) as Arc<dyn ClipboardBackend>,
        );
        assert_eq!(clipboard.read_text(), None);
    }
}
