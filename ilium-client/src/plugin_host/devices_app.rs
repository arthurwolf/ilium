//! `App` side of the devices client plugins reach through the event loop: the clipboard
//! (`Output::Clipboard`) and the file picker of `fs.pick` (`Output::PickPath`).
//!
//! A child module of `app`, so it can open the explorer overlay with the App's execution client
//! and keep the clipboard requests next to the terminal's own. A plugin's clipboard call runs on
//! the one clipboard helper process the terminal copy and paste use; its completion is matched
//! by id in `accept_clipboard_completion`. A plugin's picker is pushed over whatever is on
//! screen and returns to it when the user picks or dismisses.

use std::path::PathBuf;
use std::sync::mpsc;

use crossterm::event::Event;
use ilium_plugin_api::generated::host::fs::{PickKind, PickSpec};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

use super::{App, Mode};
use crate::explorer_overlay::{ExplorerOutcome, ExplorerOverlay};
use crate::plugin_host::clipboard_backend::{
    completed_reply, PluginClipboardReply, PluginClipboardRequest,
};
use crate::plugin_host::file_picker::{path_answers, PluginPick};

/// A plugin clipboard call the helper process is running.
#[derive(Debug)]
pub(crate) struct PendingPluginClipboard {
    plugin: String,
    reading: bool,
    reply: mpsc::Sender<PluginClipboardReply>,
}

impl App {
    /// Runs a plugin's clipboard call on the client's clipboard service; the answer follows
    /// when the helper completes it.
    pub(crate) fn run_plugin_clipboard(
        &mut self,
        plugin: String,
        request: PluginClipboardRequest,
        reply: mpsc::Sender<PluginClipboardReply>,
    ) {
        let Some(clipboard) = &self.terminal_clipboard else {
            let _ = reply.send(Err(PluginError::new(
                ErrorCode::Unsupported,
                "the client has no clipboard route",
            )));
            return;
        };
        let reading = matches!(request, PluginClipboardRequest::Read);
        let operation = match request {
            PluginClipboardRequest::Read => crate::terminal_clipboard::Operation::Read,
            PluginClipboardRequest::Write(text) => {
                crate::terminal_clipboard::Operation::Write(text)
            }
        };
        match clipboard.submit(operation) {
            Ok(id) => {
                self.plugin_clipboard_waiting.insert(
                    id,
                    PendingPluginClipboard {
                        plugin,
                        reading,
                        reply,
                    },
                );
            }
            Err((_operation, error)) => {
                let _ = reply.send(Err(PluginError::new(
                    ErrorCode::Busy,
                    format!("the clipboard is busy: {error}"),
                )));
            }
        }
    }

    /// Answers the plugin whose clipboard call finished as `id`; `false` when `id` is not a
    /// plugin's. A read names the plugin in the status line and logs the length only.
    pub(crate) fn finish_plugin_clipboard(
        &mut self,
        id: u64,
        result: &Result<String, String>,
    ) -> bool {
        let Some(pending) = self.plugin_clipboard_waiting.remove(&id) else {
            return false;
        };
        let reply = completed_reply(pending.reading, result);
        if pending.reading {
            if let Ok(text) = &reply {
                let length = text.as_ref().map_or(0, String::len);
                tracing::info!(plugin = %pending.plugin, length, "clipboard read by a plugin");
                self.status_message = Some(format!("clipboard read by {}", pending.plugin));
            }
        }
        let _ = pending.reply.send(reply);
        true
    }

    /// Whether a plugin's file picker is on screen or waiting under another screen.
    fn plugin_file_picker_open(&self) -> bool {
        std::iter::once(&self.mode)
            .chain(self.modal_stack.iter())
            .any(|mode| matches!(mode, Mode::PluginFilePicker(..)))
    }

    /// Opens the file picker `plugin` asked for, over whatever is on screen. One plugin picker
    /// at a time: a second request while one is open is dismissed at once.
    pub(crate) fn open_plugin_file_picker(
        &mut self,
        plugin: String,
        spec: PickSpec,
        reply: mpsc::Sender<Option<PathBuf>>,
    ) {
        let pick = PluginPick::new(plugin, spec, reply);
        if self.plugin_file_picker_open() {
            self.status_message = Some(format!(
                "{} asked for a file while another picker is open",
                pick.plugin()
            ));
            pick.answer(None);
            return;
        }
        let folder_label = match pick.spec().kind {
            PickKind::OpenDirectory => Some(match pick.spec().title.trim() {
                "" => "Choose Folder".to_owned(),
                title => title.to_owned(),
            }),
            PickKind::OpenFile | PickKind::SaveFile => None,
        };
        let root = self.session_cwd.clone();
        match self.open_explorer_overlay(&root, folder_label.as_deref()) {
            Ok(overlay) => {
                self.status_message = Some(pick.prompt());
                self.push_modal(Mode::PluginFilePicker(Box::new(overlay), pick));
            }
            Err(error) => {
                self.status_message = Some(format!("Could not open the file picker: {error}"));
                pick.answer(None);
            }
        }
    }

    /// One key or mouse event for a plugin's picker, which the input handler moved out of
    /// `mode`. A pick the plugin cannot use keeps the picker open with a hint; an Escape the
    /// overlay did not consume dismisses it.
    pub(crate) fn plugin_file_picker_event(
        &mut self,
        mut overlay: Box<ExplorerOverlay>,
        pick: PluginPick,
        event: &Event,
        escape: bool,
    ) {
        match overlay.handle(event, self.layout.screen_area) {
            Ok(ExplorerOutcome::Picked(path)) => match path_answers(pick.spec(), &path) {
                Ok(()) => {
                    pick.answer(Some(path));
                    self.pop_modal();
                }
                Err(hint) => {
                    self.status_message = Some(format!("{}: {hint}", pick.plugin()));
                    self.mode = Mode::PluginFilePicker(overlay, pick);
                }
            },
            Ok(ExplorerOutcome::Ignored) if escape => {
                pick.answer(None);
                self.pop_modal();
            }
            Ok(_) => self.mode = Mode::PluginFilePicker(overlay, pick),
            Err(error) => {
                self.status_message = Some(format!("File picker error: {error}"));
                self.mode = Mode::PluginFilePicker(overlay, pick);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;

    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use ilium_plugin_api::generated::host::fs::{PickKind, PickSpec};
    use ilium_plugin_api::generated::types::errors::ErrorCode;

    use super::super::{App, Mode};
    use crate::plugin_host::clipboard_backend::PluginClipboardRequest;

    fn app(cwd: PathBuf) -> App {
        App::new("test-session".to_owned(), cwd)
    }

    fn spec(kind: PickKind) -> PickSpec {
        PickSpec {
            kind,
            title: String::new(),
            extensions: Vec::new(),
        }
    }

    #[test]
    fn without_a_clipboard_service_a_plugin_call_is_unsupported() {
        let mut app = app(std::env::temp_dir());
        app.terminal_clipboard = None;
        let (reply, answer) = mpsc::channel();
        app.run_plugin_clipboard("clips".to_owned(), PluginClipboardRequest::Read, reply);
        let refused = answer.recv().expect("answered").expect_err("no route");
        assert_eq!(refused.code, ErrorCode::Unsupported);
        assert!(app.plugin_clipboard_waiting.is_empty());
    }

    #[test]
    fn a_plugin_read_waits_for_its_completion_and_names_the_plugin() {
        let mut app = app(std::env::temp_dir());
        app.terminal_clipboard = Some(crate::terminal_clipboard::ClipboardService::fixture_queue(
            crate::execution::test_client(),
        ));
        let (reply, answer) = mpsc::channel();
        app.run_plugin_clipboard("clips".to_owned(), PluginClipboardRequest::Read, reply);
        let id = *app
            .plugin_clipboard_waiting
            .keys()
            .next()
            .expect("the read is pending");
        assert!(
            answer.try_recv().is_err(),
            "no answer before the helper completes"
        );
        assert!(!app.finish_plugin_clipboard(id + 1_000, &Ok("other".to_owned())));
        assert!(app.finish_plugin_clipboard(id, &Ok("copied".to_owned())));
        assert_eq!(
            answer.recv().expect("answered"),
            Ok(Some("copied".to_owned()))
        );
        assert_eq!(
            app.status_message.as_deref(),
            Some("clipboard read by clips")
        );
        assert!(app.plugin_clipboard_waiting.is_empty());
    }

    #[test]
    fn escape_dismisses_a_plugin_picker_and_returns_to_the_screen_below() {
        let folder = tempfile::tempdir().expect("scratch");
        let mut app = app(folder.path().to_path_buf());
        let (reply, answer) = mpsc::channel();
        app.open_plugin_file_picker("clips".to_owned(), spec(PickKind::OpenFile), reply);
        assert!(matches!(app.mode, Mode::PluginFilePicker(..)));
        assert_eq!(
            app.status_message.as_deref(),
            Some("clips asks for a file to open")
        );

        // A second request while the first picker is open is dismissed at once.
        let (second, second_answer) = mpsc::channel();
        app.open_plugin_file_picker("other".to_owned(), spec(PickKind::OpenFile), second);
        assert_eq!(second_answer.recv().expect("answered"), None);

        let Mode::PluginFilePicker(overlay, pick) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            panic!("the plugin picker is open");
        };
        let escape = Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.plugin_file_picker_event(overlay, pick, &escape, true);
        assert_eq!(answer.recv().expect("answered"), None);
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app.modal_stack.is_empty());
    }
}
