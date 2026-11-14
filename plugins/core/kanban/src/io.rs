//! Reading and writing a board through the `fs` host import.
//!
//! Moved from `ilium-client/src/filesystem/boards.rs` and the persistence half of
//! `ilium-client/src/board.rs`. A folder board is read synchronously (a card file is at most
//! 1 MiB, the whole board at most 1 MiB). A Markdown board is read and written with a revision
//! token: `read-with-revision` gives the text and its token, `write-atomic` replaces the file
//! only when the token still matches, so an edit made outside Ilium is never overwritten; the
//! plugin reloads instead. Writes are asynchronous; `Writer` keeps one write in flight and folds
//! every edit made meanwhile into one follow-up write.

use std::collections::HashSet;

use ilium_plugin_sdk::host;
use ilium_plugin_sdk::host::{EntryKind, HostFs, WriteMode};
use ilium_plugin_sdk::types::{ErrorCode, FsPath, Outcome, RequestId, RequestRef, RequestResult};

use crate::board::{parse_markdown_board, serialize_markdown_board, BoardColumn};
use crate::model::{default_columns, Storage};
use crate::store::{
    columns_from_listing, is_card_file, join, plan_folder_save, CardFile, ColumnListing, FolderOp,
    FolderState,
};

/// The file scope every board lives in.
pub const SCOPE: &str = "project";
/// Most entries one directory listing asks for.
const LIST_LIMIT: u32 = 4096;
/// Most bytes one card file or Markdown board may hold.
const FILE_LIMIT: u32 = 1024 * 1024;

pub fn path(relative: &str) -> FsPath {
    FsPath {
        scope: SCOPE.to_string(),
        path: relative.to_string(),
    }
}

/// The request number of a completed host call.
pub fn call_number(result: &RequestResult) -> Option<u64> {
    match result.request {
        RequestRef::Call(RequestId(number)) => Some(number),
        RequestRef::Submission(_) => None,
    }
}

pub fn succeeded(outcome: &Outcome) -> bool {
    matches!(
        outcome,
        Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_)
    )
}

/// True when the outcome is a refused conditional write (the file changed on disk).
pub fn is_conflict(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Failed(error) | Outcome::Refused(error) if error.code == ErrorCode::Conflict)
}

/// The text of a failed outcome.
pub fn failure_text(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Failed(error) | Outcome::Refused(error) => error.message.clone(),
        Outcome::Cancelled => "cancelled".to_string(),
        _ => "the write did not complete".to_string(),
    }
}

// ---- reading

/// What a read gives back.
pub struct Loaded {
    pub columns: Vec<BoardColumn>,
    /// The revision token of a Markdown board; none for a folder board.
    pub revision: Option<String>,
}

/// True when the board path exists and is not a symlink.
pub fn exists(storage: &Storage) -> Result<bool, String> {
    let mut api = host();
    match HostFs::metadata(&mut *api, path(storage.path())) {
        Ok(metadata) if metadata.kind == EntryKind::Symlink => {
            Err("Board path is a symbolic link".to_string())
        }
        Ok(_) => Ok(true),
        Err(error) if error.code == ErrorCode::NotFound => Ok(false),
        Err(error) => Err(error.message),
    }
}

/// Reads a folder board: every column directory in name order with its Markdown cards.
pub fn load_folder(root: &str) -> Result<Vec<BoardColumn>, String> {
    let mut api = host();
    let entries = match HostFs::list_dir(&mut *api, path(root), LIST_LIMIT) {
        Ok(entries) => entries,
        Err(error) if error.code == ErrorCode::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.message),
    };
    let mut directories: Vec<String> = entries
        .into_iter()
        .filter(|entry| entry.metadata.kind == EntryKind::Directory)
        .map(|entry| entry.name)
        .collect();
    directories.sort();
    let mut listing = Vec::new();
    for name in directories {
        let column_path = join(root, &name);
        let mut files: Vec<String> = HostFs::list_dir(&mut *api, path(&column_path), LIST_LIMIT)
            .map_err(|error| error.message)?
            .into_iter()
            .filter(|entry| entry.metadata.kind == EntryKind::File && is_card_file(&entry.name))
            .map(|entry| entry.name)
            .collect();
        files.sort();
        let mut cards = Vec::new();
        for file in files {
            let bytes = HostFs::read(&mut *api, path(&join(&column_path, &file)), 0, FILE_LIMIT)
                .map_err(|error| format!("Could not read {file}: {}", error.message))?;
            cards.push(CardFile {
                name: file,
                text: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        listing.push(ColumnListing { name, cards });
    }
    columns_from_listing(&listing)
}

/// The Markdown card files each column directory holds now, for planning a save.
pub fn folder_state(root: &str) -> FolderState {
    let mut api = host();
    let mut state = FolderState::default();
    let Ok(entries) = HostFs::list_dir(&mut *api, path(root), LIST_LIMIT) else {
        return state;
    };
    for entry in entries {
        if entry.metadata.kind != EntryKind::Directory {
            continue;
        }
        let files = HostFs::list_dir(&mut *api, path(&join(root, &entry.name)), LIST_LIMIT)
            .unwrap_or_default()
            .into_iter()
            .filter(|file| file.metadata.kind == EntryKind::File && is_card_file(&file.name))
            .map(|file| file.name)
            .collect();
        state.columns.push((entry.name, files));
    }
    state
}

/// Starts reading a Markdown board with its revision; the answer is a `take_revisioned` on the
/// returned request.
pub fn start_markdown_read(file: &str) -> Result<u64, String> {
    let mut api = host();
    HostFs::read_with_revision(&mut *api, path(file))
        .map(|request| request.0)
        .map_err(|error| error.message)
}

/// Parses the answer of a Markdown read.
pub fn finish_markdown_read(request: u64) -> Result<Loaded, String> {
    let mut api = host();
    let revisioned = HostFs::take_revisioned(&mut *api, RequestId(request))
        .ok_or_else(|| "The board file could not be read".to_string())?;
    let text = String::from_utf8_lossy(&revisioned.bytes).into_owned();
    let columns = parse_markdown_board(&text);
    crate::store::validate_limits(&columns)?;
    Ok(Loaded {
        columns,
        revision: Some(revisioned.revision),
    })
}

// ---- writing

/// Starts replacing a Markdown board when its revision is still `expected` (none creates it).
pub fn start_markdown_write(
    file: &str,
    columns: &[BoardColumn],
    expected: Option<String>,
) -> Result<u64, String> {
    let mut api = host();
    HostFs::write_atomic(
        &mut *api,
        path(file),
        serialize_markdown_board(columns).into_bytes(),
        expected,
    )
    .map(|request| request.0)
    .map_err(|error| error.message)
}

/// The new revision after a successful `write-atomic`.
pub fn finish_markdown_write(request: u64) -> Option<String> {
    let mut api = host();
    HostFs::take_written(&mut *api, RequestId(request))
}

/// A folder write in progress: the writes run together, the removals follow once every write
/// has succeeded.
#[derive(Debug, Default)]
pub struct FolderRun {
    outstanding: HashSet<u64>,
    removals: Vec<String>,
    removing: bool,
    failure: Option<String>,
}

/// How a folder run stands after one of its requests completed.
#[derive(Debug, PartialEq, Eq)]
pub enum FolderProgress {
    /// Requests are still running.
    Running,
    /// Every operation succeeded.
    Done,
    /// An operation failed; the run is over once nothing is outstanding.
    Failed(String),
}

impl FolderRun {
    /// Runs the operations of a plan: directories at once, writes together, removals later.
    /// Returns the run and the request numbers it started.
    pub fn start(root: &str, columns: &[BoardColumn]) -> Result<(FolderRun, Vec<u64>), String> {
        let ops = plan_folder_save(root, &folder_state(root), columns)?;
        let mut api = host();
        let mut run = FolderRun::default();
        for op in ops {
            match op {
                FolderOp::CreateDir(directory) => {
                    match HostFs::create_dir(&mut *api, path(&directory)) {
                        Ok(()) => {}
                        Err(error) if error.code == ErrorCode::Conflict => {}
                        Err(error) => return Err(error.message),
                    }
                }
                FolderOp::WriteFile { path: file, text } => {
                    let request = HostFs::write(
                        &mut *api,
                        path(&file),
                        text.into_bytes(),
                        WriteMode::Replace,
                    )
                    .map_err(|error| error.message)?;
                    run.outstanding.insert(request.0);
                }
                FolderOp::RemoveFile(file) => run.removals.push(file),
            }
        }
        let started = run.outstanding.iter().copied().collect();
        Ok((run, started))
    }

    pub fn owns(&self, request: u64) -> bool {
        self.outstanding.contains(&request)
    }

    /// Records one completed request; may start the removals.
    pub fn completed(&mut self, request: u64, result: &RequestResult) -> FolderProgress {
        self.outstanding.remove(&request);
        if !succeeded(&result.outcome) && self.failure.is_none() {
            self.failure = Some(failure_text(&result.outcome));
        }
        if !self.outstanding.is_empty() {
            return FolderProgress::Running;
        }
        if let Some(failure) = &self.failure {
            return FolderProgress::Failed(failure.clone());
        }
        if !self.removing && !self.removals.is_empty() {
            self.removing = true;
            let mut api = host();
            for file in std::mem::take(&mut self.removals) {
                match HostFs::remove(&mut *api, path(&file)) {
                    Ok(request) => {
                        self.outstanding.insert(request.0);
                    }
                    Err(error) => return FolderProgress::Failed(error.message),
                }
            }
            return if self.outstanding.is_empty() {
                FolderProgress::Done
            } else {
                FolderProgress::Running
            };
        }
        FolderProgress::Done
    }
}

/// Renames a column directory of a folder board; synchronous from the plugin's view because the
/// following save relies on the directory being in place.
pub fn start_rename(root: &str, from: &str, to: &str) -> Result<u64, String> {
    let mut api = host();
    HostFs::rename(&mut *api, path(&join(root, from)), path(&join(root, to)))
        .map(|request| request.0)
        .map_err(|error| error.message)
}

/// Removes the (empty) directory of a deleted column.
pub fn start_remove(root: &str, column: &str) -> Result<u64, String> {
    let mut api = host();
    HostFs::remove(&mut *api, path(&join(root, column)))
        .map(|request| request.0)
        .map_err(|error| error.message)
}

/// The columns of a board that does not exist yet.
pub fn new_board_columns() -> Vec<BoardColumn> {
    default_columns()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_sdk::types::PluginError;

    fn result(outcome: Outcome) -> RequestResult {
        RequestResult {
            request: RequestRef::Call(RequestId(1)),
            outcome,
        }
    }

    #[test]
    fn a_refused_conditional_write_is_a_conflict() {
        let conflict = PluginError {
            code: ErrorCode::Conflict,
            message: "changed".to_string(),
            retry_after_ms: None,
        };
        assert!(is_conflict(&Outcome::Failed(conflict.clone())));
        assert!(is_conflict(&Outcome::Refused(conflict)));
        assert!(!is_conflict(&Outcome::Ok));
        assert!(!succeeded(&Outcome::Cancelled));
        assert!(succeeded(&Outcome::Ok));
    }

    #[test]
    fn only_a_call_result_has_a_request_number() {
        assert_eq!(call_number(&result(Outcome::Ok)), Some(1));
        let submission = RequestResult {
            request: RequestRef::Submission(4),
            outcome: Outcome::Ok,
        };
        assert_eq!(call_number(&submission), None);
    }

    #[test]
    fn a_failed_write_reports_its_message() {
        let error = PluginError {
            code: ErrorCode::Io,
            message: "disk full".to_string(),
            retry_after_ms: None,
        };
        assert_eq!(failure_text(&Outcome::Failed(error)), "disk full");
        assert_eq!(failure_text(&Outcome::Cancelled), "cancelled");
    }
}
