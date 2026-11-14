//! The file picker of client plugins (`fs.pick`, api-fs "Picker").
//!
//! `FsImport::pick` runs this on a worker thread of its own, so it may block until the user
//! answers. The picker itself is the client's explorer overlay, opened by the event loop as
//! `Mode::PluginFilePicker` on `Output::PickPath`; the App answers on the reply channel when the
//! user picks or dismisses it. The import turns the chosen path into the scope `picked/<n>`.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use ilium_plugin_api::generated::host::fs::{PickKind, PickSpec};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_host::imports::env::CancelFlag;
use ilium_plugin_host::imports::fs::FilePicker;

use super::host::{Asked, Output, OutputPort};

/// A picker a plugin opened (`Mode::PluginFilePicker`): who asked, what for, and where the
/// answer goes. Dropping it unanswered dismisses the picker for the plugin.
#[derive(Debug)]
pub struct PluginPick {
    plugin: String,
    spec: PickSpec,
    reply: mpsc::Sender<Option<PathBuf>>,
}

impl PluginPick {
    /// The picker `plugin` asked for with `spec`, answering on `reply`.
    pub(crate) fn new(
        plugin: String,
        spec: PickSpec,
        reply: mpsc::Sender<Option<PathBuf>>,
    ) -> Self {
        Self {
            plugin,
            spec,
            reply,
        }
    }

    /// The plugin that asked.
    pub fn plugin(&self) -> &str {
        &self.plugin
    }

    /// What the plugin asked for.
    pub fn spec(&self) -> &PickSpec {
        &self.spec
    }

    /// Answers the plugin: the chosen path, or `None` when the user dismissed the picker. A
    /// plugin that stopped waiting is not told.
    pub(crate) fn answer(self, path: Option<PathBuf>) {
        let _ = self.reply.send(path);
    }

    /// The status line shown while the picker is open.
    pub fn prompt(&self) -> String {
        let wanted = match self.spec.kind {
            PickKind::OpenFile => "a file to open",
            PickKind::OpenDirectory => "a folder",
            PickKind::SaveFile => "a file to save to",
        };
        let title = self.spec.title.trim();
        if title.is_empty() {
            format!("{} asks for {wanted}", self.plugin)
        } else {
            format!("{} asks for {wanted}: {title}", self.plugin)
        }
    }
}

/// The `fs.pick` backend of the client's plugin host.
#[derive(Debug, Clone)]
pub struct ClientFilePicker {
    port: OutputPort,
}

impl ClientFilePicker {
    /// A picker that asks the event loop through `port`.
    pub(crate) fn new(port: OutputPort) -> Self {
        Self { port }
    }
}

impl FilePicker for ClientFilePicker {
    fn pick(
        &self,
        plugin: &str,
        spec: &PickSpec,
        cancel: &CancelFlag,
    ) -> Result<Option<PathBuf>, PluginError> {
        // The user takes as long as they need; the instance ending or the host stopping ends
        // the wait (the open picker then answers nobody).
        let asked = self.port.ask(
            |reply| Output::PickPath {
                plugin: plugin.to_owned(),
                spec: spec.clone(),
                reply,
            },
            None,
            &|| cancel.is_set(),
        );
        match asked {
            Asked::Answered(path) => Ok(path),
            // Dropped: the picker closed without an answer (another flow closed every modal).
            Asked::Dropped | Asked::Stopped => Ok(None),
            Asked::TimedOut => Err(PluginError::new(
                ErrorCode::TimedOut,
                "the file picker did not answer",
            )),
        }
    }
}

/// Whether `path`, chosen in a picker opened for `spec`, answers it: a folder for
/// `open-directory`, a file for the others, and for files one of `spec.extensions` when the
/// plugin named any (compared without the dot and without case).
pub(crate) fn path_answers(spec: &PickSpec, path: &Path) -> Result<(), String> {
    match spec.kind {
        PickKind::OpenDirectory => {
            if path.is_dir() {
                Ok(())
            } else {
                Err("choose a folder".to_owned())
            }
        }
        PickKind::OpenFile | PickKind::SaveFile => {
            if path.is_dir() {
                return Err("choose a file".to_owned());
            }
            if spec.extensions.is_empty() {
                return Ok(());
            }
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            let accepted = spec.extensions.iter().any(|wanted| {
                wanted
                    .trim_start_matches('.')
                    .eq_ignore_ascii_case(&extension)
            });
            if accepted {
                Ok(())
            } else {
                Err(format!(
                    "choose a {} file",
                    spec.extensions
                        .iter()
                        .map(|wanted| format!(".{}", wanted.trim_start_matches('.')))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{path_answers, PluginPick};
    use ilium_plugin_api::generated::host::fs::{PickKind, PickSpec};

    fn spec(kind: PickKind, extensions: &[&str]) -> PickSpec {
        PickSpec {
            kind,
            title: "Choose".to_owned(),
            extensions: extensions.iter().map(|text| (*text).to_owned()).collect(),
        }
    }

    #[test]
    fn a_file_picker_accepts_only_the_named_extensions() {
        let folder = tempfile::tempdir().expect("scratch");
        let clip = folder.path().join("intro.MP4");
        let notes = folder.path().join("notes.txt");
        std::fs::write(&clip, b"v").expect("clip");
        std::fs::write(&notes, b"n").expect("notes");
        let wanted = spec(PickKind::OpenFile, &[".mp4", "webm"]);
        assert_eq!(path_answers(&wanted, &clip), Ok(()));
        let refused = path_answers(&wanted, &notes).expect_err("wrong extension");
        assert!(refused.contains(".mp4, .webm"));
        assert_eq!(path_answers(&spec(PickKind::OpenFile, &[]), &notes), Ok(()));
        assert!(path_answers(&wanted, folder.path()).is_err());
    }

    #[test]
    fn a_folder_picker_accepts_only_folders() {
        let folder = tempfile::tempdir().expect("scratch");
        let file = folder.path().join("a.txt");
        std::fs::write(&file, b"a").expect("file");
        let wanted = spec(PickKind::OpenDirectory, &[]);
        assert_eq!(path_answers(&wanted, folder.path()), Ok(()));
        assert!(path_answers(&wanted, &file).is_err());
    }

    #[test]
    fn a_plugin_pick_names_the_plugin_and_delivers_one_answer() {
        let (reply, answers) = std::sync::mpsc::channel();
        let pick = PluginPick::new(
            "clips".to_owned(),
            spec(PickKind::OpenDirectory, &[]),
            reply,
        );
        assert_eq!(pick.plugin(), "clips");
        assert_eq!(pick.prompt(), "clips asks for a folder: Choose");
        pick.answer(Some(std::path::PathBuf::from("/videos")));
        assert_eq!(
            answers.recv().expect("answered"),
            Some(std::path::PathBuf::from("/videos"))
        );
        // A plugin that stopped waiting is not an error for the picker.
        let (gone, dropped) = std::sync::mpsc::channel();
        drop(dropped);
        PluginPick::new("clips".to_owned(), spec(PickKind::OpenFile, &[]), gone).answer(None);
    }
}
