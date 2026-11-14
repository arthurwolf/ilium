//! The clipboard of client plugins (`ilium:host/clipboard`).
//!
//! The import (`ilium_plugin_host::imports::clipboard`) checks the grants and the limits; this
//! backend carries an admitted call to the event loop as `Output::Clipboard`, where the App runs
//! it on the client's one clipboard helper process (`terminal_clipboard`), the same route the
//! terminal copy and paste take. The plugin's executor thread waits for the answer for a short
//! bound, so a stuck clipboard owner costs the plugin a `timed-out`, never a hang.

use std::time::Duration;

use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_host::imports::clipboard::ClipboardBackend;

use super::host::{Asked, Output, OutputPort};

/// How long a plugin waits for the clipboard helper.
const CLIPBOARD_WAIT: Duration = Duration::from_secs(3);

/// What a plugin asks of the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginClipboardRequest {
    /// Read the clipboard text.
    Read,
    /// Replace the clipboard text.
    Write(String),
}

/// The answer to a [`PluginClipboardRequest`]: the text read (`None` for an empty clipboard or a
/// write), or why it failed.
pub type PluginClipboardReply = Result<Option<String>, PluginError>;

/// The clipboard backend of the client's plugin host.
#[derive(Debug, Clone)]
pub struct ClientClipboard {
    port: OutputPort,
}

impl ClientClipboard {
    /// A backend that asks the event loop through `port`.
    pub(crate) fn new(port: OutputPort) -> Self {
        Self { port }
    }

    fn ask(&self, plugin: &str, request: PluginClipboardRequest) -> PluginClipboardReply {
        let asked = self.port.ask(
            |reply| Output::Clipboard {
                plugin: plugin.to_owned(),
                request,
                reply,
            },
            Some(CLIPBOARD_WAIT),
            &|| false,
        );
        answer_of(asked)
    }
}

/// The plugin-facing result of a clipboard question.
fn answer_of(asked: Asked<PluginClipboardReply>) -> PluginClipboardReply {
    match asked {
        Asked::Answered(reply) => reply,
        Asked::TimedOut => Err(PluginError::new(
            ErrorCode::TimedOut,
            "the clipboard did not answer within 3 seconds",
        )),
        Asked::Dropped => Err(PluginError::new(
            ErrorCode::Unsupported,
            "the client has no clipboard route",
        )),
        Asked::Stopped => Err(PluginError::new(
            ErrorCode::Cancelled,
            "the plugin host is stopping",
        )),
    }
}

impl ClipboardBackend for ClientClipboard {
    fn read_text(&self, plugin: &str) -> Result<Option<String>, PluginError> {
        self.ask(plugin, PluginClipboardRequest::Read)
    }

    fn write_text(&self, plugin: &str, text: &str) -> Result<(), PluginError> {
        self.ask(plugin, PluginClipboardRequest::Write(text.to_owned()))
            .map(|_| ())
    }
}

/// The reply the App sends when the clipboard helper finished `request`: a read answers its
/// text (`None` when the clipboard is empty), a write answers nothing, and a helper failure is
/// `unsupported` (no clipboard program) with the helper's message.
pub(crate) fn completed_reply(
    request_was_read: bool,
    result: &Result<String, String>,
) -> PluginClipboardReply {
    match result {
        Ok(text) if request_was_read && !text.is_empty() => Ok(Some(text.clone())),
        Ok(_) => Ok(None),
        Err(message) => Err(PluginError::new(
            ErrorCode::Unsupported,
            format!("clipboard unavailable: {message}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{answer_of, completed_reply};
    use crate::plugin_host::host::Asked;
    use ilium_plugin_api::generated::types::errors::ErrorCode;

    #[test]
    fn a_completed_read_answers_its_text_and_an_empty_clipboard_answers_nothing() {
        assert_eq!(
            completed_reply(true, &Ok("copied".to_owned())),
            Ok(Some("copied".to_owned()))
        );
        assert_eq!(completed_reply(true, &Ok(String::new())), Ok(None));
        assert_eq!(completed_reply(false, &Ok("ignored".to_owned())), Ok(None));
    }

    #[test]
    fn a_failed_helper_is_unsupported_with_its_message() {
        let failed = completed_reply(true, &Err("no xclip".to_owned())).expect_err("failed");
        assert_eq!(failed.code, ErrorCode::Unsupported);
        assert!(failed.message.contains("no xclip"));
    }

    #[test]
    fn an_unanswered_question_becomes_a_plugin_error() {
        assert_eq!(
            answer_of(Asked::TimedOut).expect_err("timed out").code,
            ErrorCode::TimedOut
        );
        assert_eq!(
            answer_of(Asked::Dropped).expect_err("dropped").code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            answer_of(Asked::Stopped).expect_err("stopped").code,
            ErrorCode::Cancelled
        );
        assert_eq!(answer_of(Asked::Answered(Ok(None))), Ok(None));
    }
}
