//! The client side of `ilium.core.kanban`: board panes, their state and what their events do.
//!
//! Moved from `ilium-client/src/app.rs`, `keys.rs` and `filesystem/board_app.rs` (board keys,
//! prompts, load and write ownership). One board pane is one `Entry`, found by the node id of
//! the pane. The pane is bound to its storage by the `new-board` command (a queue of bindings
//! consumed in the order the panes are created) and the binding is kept in the project storage
//! bucket so a restored pane finds its board again. State lives in a thread-local cell because
//! the plugin structs are unit structs.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};

use ilium_plugin_sdk::host;
use ilium_plugin_sdk::host::{Bucket, GrabSpec, HostFs, HostKeys, HostSettings, HostStorage};
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, CreatePaneSpec, Event, GrabId, Intent, KeyEvent, NodeId, OpenViewSpec,
    Submission, ToastSpec, Tone, UiAction, ViewTarget,
};
use serde_json::{json, Value};

use crate::board::BoardColumn;
use crate::control;
use crate::create_dialog;
use crate::io::{self, FolderProgress, FolderRun};
use crate::model::{default_columns, Board, CardEditorField, Edit, Storage};
use crate::view::{self, Prompt, Settings};

/// The pane kind the manifest declares, as the kernel names it.
pub const KIND: &str = "ilium.core.kanban.board";
/// The command that opens a board pane: `{ "path": "docs/plan.md", "kind": "markdown" | "folder" }`.
pub const NEW_COMMAND: &str = "ilium.core.kanban/new-board";
/// The command that reloads the board of the focused pane.
pub const RELOAD_COMMAND: &str = "ilium.core.kanban/reload";

/// Every chord a board pane takes while no text field is being edited.
pub const BOARD_CHORDS: [&str; 19] = [
    "left",
    "right",
    "up",
    "down",
    "h",
    "j",
    "k",
    "l",
    "shift+left",
    "shift+right",
    "shift+up",
    "shift+down",
    "shift+h",
    "shift+j",
    "shift+k",
    "shift+l",
    "enter",
    "n",
    "x",
];
/// The chords of the actions on the selection.
const ACTION_CHORDS: [&str; 7] = ["c", "r", "e", "d", "y", "esc", "tab"];

/// What a pending host request was for.
#[derive(Debug)]
enum Purpose {
    LoadMarkdown,
    WriteMarkdown,
    WriteFolder,
    Structure,
}

/// How the board is being written.
#[derive(Debug, Default)]
enum Write {
    #[default]
    Idle,
    Markdown,
    Folder(FolderRun),
    /// A column directory is being renamed or removed; the save follows.
    Structure,
}

/// The binding of a pane-to-be.
#[derive(Debug, Clone)]
struct Binding {
    storage: Storage,
}

#[derive(Debug)]
struct Entry {
    storage: Storage,
    board: Option<Board>,
    /// Why the board could not be shown, or the loading note.
    note: String,
    revision: Option<String>,
    write: Write,
    /// An edit was made while a write was in flight; one more write follows.
    dirty: bool,
    prompt: Option<Prompt>,
    grab: Option<(GrabId, Vec<String>)>,
    /// The next successful Markdown load may create the file when it does not exist.
    create_if_missing: bool,
    /// The width of the pane when the plugin last drew it.
    area_columns: u16,
}

#[derive(Debug, Default)]
struct State {
    entries: BTreeMap<u64, Entry>,
    requests: HashMap<u64, (u64, Purpose)>,
    pending: VecDeque<Binding>,
    settings: Settings,
    /// The open "New board" dialog.
    dialog: Option<create_dialog::Dialog>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Forgets everything and reads the settings; called on activation.
pub fn reset(settings_json: &str) {
    STATE.with(|state| {
        *state.borrow_mut() = State {
            settings: parse_settings(settings_json),
            ..State::default()
        };
    });
}

/// The board settings of a plugin settings JSON object; missing or invalid keys keep the default.
pub fn parse_settings(settings_json: &str) -> Settings {
    let mut settings = Settings::default();
    if let Ok(value) = serde_json::from_str::<Value>(settings_json) {
        if let Some(preview) = value["card_preview"].as_bool() {
            settings.card_preview = preview;
        }
        if let Some(width) = value["column_width"].as_u64() {
            settings.column_width = width.clamp(12, 60) as u16;
        }
    }
    settings
}

/// The tree of the board pane `node` for an area.
pub fn render(node: u64, area: ilium_plugin_sdk::types::Area) -> ilium_plugin_sdk::types::UiTree {
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let settings = state.settings;
        let Some(entry) = state.entries.get_mut(&node) else {
            return view::message_tree("This board is not bound to a file.");
        };
        entry.area_columns = area.columns;
        match &entry.board {
            Some(board) => view::render(board, entry.prompt.as_ref(), settings, area),
            None => view::message_tree(&entry.note),
        }
    })
}

/// The tree of the popup `view`: the "New board" dialog while it is open.
pub fn render_popup(view: &ViewTarget) -> Option<ilium_plugin_sdk::types::UiTree> {
    if *view != create_dialog::view() {
        return None;
    }
    STATE.with(|cell| {
        cell.borrow()
            .dialog
            .as_ref()
            .map(create_dialog::Dialog::render)
    })
}

/// Runs `read` on the board of pane `node`, when it is loaded.
pub fn with_board<R>(node: u64, read: impl FnOnce(&Board) -> R) -> Option<R> {
    STATE.with(|state| {
        state
            .borrow()
            .entries
            .get(&node)
            .and_then(|entry| entry.board.as_ref())
            .map(read)
    })
}

fn toast(id: &str, tone: Tone, title: &str) -> Submission {
    let title: String = title.chars().take(60).collect();
    Submission::new(Intent::Toast(ToastSpec {
        id: Some(id.to_string()),
        tone,
        title,
        body: None,
        actions: Vec::new(),
        duration_ms: Some(5000),
    }))
}

fn error_toast(message: &str) -> Submission {
    toast("kanban-error", Tone::Danger, message)
}

// ---- bindings

fn binding_key(node: u64) -> String {
    format!("board/{node}")
}

fn storage_json(storage: &Storage) -> Value {
    match storage {
        Storage::Folder { path } => json!({ "kind": "folder", "path": path }),
        Storage::MarkdownFile { path } => json!({ "kind": "markdown", "path": path }),
    }
}

fn storage_from_json(value: &Value) -> Option<Storage> {
    let path = value["path"].as_str()?.to_string();
    if path.is_empty() {
        return None;
    }
    match value["kind"].as_str()? {
        "folder" => Some(Storage::Folder { path }),
        "markdown" => Some(Storage::MarkdownFile { path }),
        _ => None,
    }
}

fn remember_binding(node: u64, storage: &Storage) {
    let mut api = host();
    let _ = HostStorage::put(
        &mut *api,
        Bucket::Project,
        binding_key(node),
        storage_json(storage).to_string().into_bytes(),
    );
}

fn recall_binding(node: u64) -> Option<Storage> {
    let mut api = host();
    let bytes = HostStorage::get(&mut *api, Bucket::Project, binding_key(node))?;
    storage_from_json(&serde_json::from_slice::<Value>(&bytes).ok()?)
}

fn forget_binding(node: u64) {
    let mut api = host();
    let _ = HostStorage::remove(&mut *api, Bucket::Project, binding_key(node));
}

/// A default title for the pane of a board file: its name without the extension.
pub fn pane_title(storage: &Storage) -> String {
    let name = storage
        .path()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("Board");
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    if stem.is_empty() {
        "Board".to_string()
    } else {
        stem.to_string()
    }
}

// ---- keys

fn wanted_chords(entry: &Entry) -> Vec<String> {
    let typing = entry.prompt.as_ref().is_some_and(Prompt::takes_text)
        || entry
            .board
            .as_ref()
            .is_some_and(|board| board.is_detail_panel_open);
    let list: Vec<&str> = if typing {
        vec!["esc"]
    } else {
        BOARD_CHORDS
            .iter()
            .chain(ACTION_CHORDS.iter())
            .copied()
            .collect()
    };
    list.into_iter().map(str::to_string).collect()
}

/// Grabs the chords the pane needs now and releases the ones it no longer needs.
fn refresh_grab(node: u64, entry: &mut Entry) {
    let wanted = wanted_chords(entry);
    if entry.grab.as_ref().is_some_and(|(_, held)| *held == wanted) {
        return;
    }
    let mut api = host();
    if let Some((grab, _)) = entry.grab.take() {
        HostKeys::release(&mut *api, grab);
    }
    if let Ok(grab) = HostKeys::grab(
        &mut *api,
        GrabSpec {
            chords: wanted.clone(),
            pane: Some(NodeId(node)),
            until_release: false,
        },
    ) {
        entry.grab = Some((grab, wanted));
    }
}

fn release_grab(entry: &mut Entry) {
    if let Some((grab, _)) = entry.grab.take() {
        let mut api = host();
        HostKeys::release(&mut *api, grab);
    }
}

// ---- loading and writing

impl State {
    fn bind(
        &mut self,
        node: u64,
        storage: Storage,
        create_if_missing: bool,
        out: &mut Vec<Submission>,
    ) {
        let mut entry = Entry {
            storage: storage.clone(),
            board: None,
            note: "Loading board…".to_string(),
            revision: None,
            write: Write::Idle,
            dirty: false,
            prompt: None,
            grab: None,
            create_if_missing,
            area_columns: 100,
        };
        remember_binding(node, &storage);
        {
            let mut api = host();
            let _ = HostFs::watch(&mut *api, io::path(storage.path()), true);
        }
        self.start_load(node, &mut entry, out);
        refresh_grab(node, &mut entry);
        self.entries.insert(node, entry);
    }

    /// Starts reading the board of `entry`. A folder is read at once; Markdown asks the host.
    fn start_load(&mut self, node: u64, entry: &mut Entry, out: &mut Vec<Submission>) {
        let exists = match io::exists(&entry.storage) {
            Ok(exists) => exists,
            Err(message) => {
                entry.note = message;
                return;
            }
        };
        if !exists {
            if !entry.create_if_missing {
                entry.note = format!("{} does not exist.", entry.storage.path());
                return;
            }
            entry.create_if_missing = false;
            let columns = default_columns();
            let storage = entry.storage.clone();
            entry.board = Some(Board::new(storage, columns));
            entry.note.clear();
            self.start_write(node, entry, out);
            return;
        }
        match entry.storage.clone() {
            Storage::Folder { path } => match io::load_folder(&path) {
                Ok(columns) => self.loaded(entry, columns, None),
                Err(message) => entry.note = message,
            },
            Storage::MarkdownFile { path } => match io::start_markdown_read(&path) {
                Ok(request) => {
                    self.requests.insert(request, (node, Purpose::LoadMarkdown));
                }
                Err(message) => entry.note = message,
            },
        }
    }

    fn loaded(&mut self, entry: &mut Entry, columns: Vec<BoardColumn>, revision: Option<String>) {
        entry.revision = revision;
        match entry.board.as_mut() {
            Some(board) => board.replace_columns(columns),
            None => {
                let columns = if columns.is_empty() {
                    default_columns()
                } else {
                    columns
                };
                entry.board = Some(Board::new(entry.storage.clone(), columns));
            }
        }
        entry.note.clear();
    }

    /// Writes the board now, or marks it for one more write when one is in flight.
    fn start_write(&mut self, node: u64, entry: &mut Entry, out: &mut Vec<Submission>) {
        if !matches!(entry.write, Write::Idle) {
            entry.dirty = true;
            return;
        }
        let Some(board) = entry.board.as_ref() else {
            return;
        };
        entry.dirty = false;
        match entry.storage.clone() {
            Storage::MarkdownFile { path } => {
                match io::start_markdown_write(&path, &board.columns, entry.revision.clone()) {
                    Ok(request) => {
                        entry.write = Write::Markdown;
                        self.requests
                            .insert(request, (node, Purpose::WriteMarkdown));
                    }
                    Err(message) => out.push(error_toast(&message)),
                }
            }
            Storage::Folder { path } => match FolderRun::start(&path, &board.columns) {
                Ok((run, started)) => {
                    for request in &started {
                        self.requests.insert(*request, (node, Purpose::WriteFolder));
                    }
                    if started.is_empty() {
                        self.write_finished(node, entry, out);
                    } else {
                        entry.write = Write::Folder(run);
                    }
                }
                Err(message) => out.push(error_toast(&message)),
            },
        }
    }

    fn write_finished(&mut self, node: u64, entry: &mut Entry, out: &mut Vec<Submission>) {
        entry.write = Write::Idle;
        if entry.dirty {
            self.start_write(node, entry, out);
        }
    }

    /// The write failed or the file changed on disk: the in-memory board is dropped for the disk's.
    fn write_failed(
        &mut self,
        node: u64,
        entry: &mut Entry,
        message: &str,
        out: &mut Vec<Submission>,
    ) {
        entry.write = Write::Idle;
        entry.dirty = false;
        out.push(error_toast(message));
        entry.create_if_missing = false;
        self.start_load(node, entry, out);
    }

    fn on_completed(
        &mut self,
        node: u64,
        purpose: Purpose,
        request: u64,
        result: &ilium_plugin_sdk::types::RequestResult,
        out: &mut Vec<Submission>,
    ) {
        let Some(mut entry) = self.entries.remove(&node) else {
            return;
        };
        match purpose {
            Purpose::LoadMarkdown => {
                if io::succeeded(&result.outcome) {
                    match io::finish_markdown_read(request) {
                        Ok(loaded) => self.loaded(&mut entry, loaded.columns, loaded.revision),
                        Err(message) => entry.note = message,
                    }
                } else {
                    entry.note = io::failure_text(&result.outcome);
                }
            }
            Purpose::WriteMarkdown => {
                if io::succeeded(&result.outcome) {
                    entry.revision = io::finish_markdown_write(request);
                    self.write_finished(node, &mut entry, out);
                } else if io::is_conflict(&result.outcome) {
                    self.write_failed(
                        node,
                        &mut entry,
                        "The board file changed on disk; it was reloaded",
                        out,
                    );
                } else {
                    let message = format!(
                        "Could not save the board: {}",
                        io::failure_text(&result.outcome)
                    );
                    self.write_failed(node, &mut entry, &message, out);
                }
            }
            Purpose::WriteFolder => {
                let progress = match &mut entry.write {
                    Write::Folder(run) if run.owns(request) => run.completed(request, result),
                    _ => FolderProgress::Running,
                };
                match progress {
                    FolderProgress::Running => {}
                    FolderProgress::Done => self.write_finished(node, &mut entry, out),
                    FolderProgress::Failed(message) => {
                        let message = format!("Could not save the board: {message}");
                        self.write_failed(node, &mut entry, &message, out);
                    }
                }
            }
            Purpose::Structure => {
                if io::succeeded(&result.outcome) {
                    entry.write = Write::Idle;
                    self.start_write(node, &mut entry, out);
                } else {
                    let message = format!(
                        "Could not change the board folder: {}",
                        io::failure_text(&result.outcome)
                    );
                    self.write_failed(node, &mut entry, &message, out);
                }
            }
        }
        refresh_grab(node, &mut entry);
        self.entries.insert(node, entry);
    }
}

// ---- edits

/// What a prompt, key or action did to a board.
enum Outcome {
    /// Nothing to write.
    Quiet,
    /// The columns changed.
    Changed,
    /// A message for the user.
    Failed(String),
}

fn edit_outcome(edit: Edit) -> Outcome {
    match edit {
        Ok(true) => Outcome::Changed,
        Ok(false) => Outcome::Quiet,
        Err(message) => Outcome::Failed(message),
    }
}

impl State {
    /// Runs one edit on the board of `node` and writes the result.
    fn edit(
        &mut self,
        node: u64,
        out: &mut Vec<Submission>,
        change: impl FnOnce(&mut Board, &mut Entry) -> Outcome,
    ) {
        let Some(mut entry) = self.entries.remove(&node) else {
            return;
        };
        if let Some(mut board) = entry.board.take() {
            let before = board.snapshot();
            let outcome = change(&mut board, &mut entry);
            match outcome {
                Outcome::Quiet => entry.board = Some(board),
                Outcome::Changed => {
                    entry.board = Some(board);
                    self.start_write(node, &mut entry, out);
                }
                Outcome::Failed(message) => {
                    entry.board = Some(before);
                    out.push(error_toast(&message));
                }
            }
        }
        refresh_grab(node, &mut entry);
        self.entries.insert(node, entry);
    }

    fn visible_columns(&self, board: &Board, area_columns: u16) -> usize {
        let detail = if board.is_detail_panel_open {
            view::detail_width(area_columns)
        } else {
            0
        };
        view::visible_column_count(
            area_columns.saturating_sub(detail),
            board.columns.len(),
            self.settings.column_width,
        )
    }

    fn on_key(&mut self, key: &KeyEvent, out: &mut Vec<Submission>) {
        let Some(node) = key.pane.map(|pane| pane.0) else {
            return;
        };
        let chord = key.chord.as_str();
        let columns = self
            .entries
            .get(&node)
            .map_or(100, |entry| entry.area_columns);
        let visible = self
            .entries
            .get(&node)
            .and_then(|entry| entry.board.as_ref())
            .map_or(1, |board| self.visible_columns(board, columns));
        let prompt = self
            .entries
            .get(&node)
            .and_then(|entry| entry.prompt.clone());
        if let Some(prompt) = prompt {
            self.on_prompt_key(node, chord, prompt, out);
            return;
        }
        let detail_open = self
            .entries
            .get(&node)
            .and_then(|entry| entry.board.as_ref())
            .is_some_and(|board| board.is_detail_panel_open);
        if detail_open {
            if chord == "esc" {
                self.edit(node, out, |board, _| {
                    board.close_card_details();
                    Outcome::Quiet
                });
            }
            return;
        }
        match chord {
            "left" | "h" => self.navigate(node, visible, out, Board::select_previous_column),
            "right" | "l" => self.navigate(node, visible, out, Board::select_next_column),
            "up" | "k" => self.navigate(node, visible, out, Board::select_previous_card),
            "down" | "j" => self.navigate(node, visible, out, Board::select_next_card),
            "shift+left" | "shift+h" => self.edit(node, out, |board, _| {
                edit_outcome(board.move_selected_card(-1))
            }),
            "shift+right" | "shift+l" => self.edit(node, out, |board, _| {
                edit_outcome(board.move_selected_card(1))
            }),
            "shift+up" | "shift+k" => self.edit(node, out, |board, _| {
                edit_outcome(board.move_selected_card_vertically(-1))
            }),
            "shift+down" | "shift+j" => self.edit(node, out, |board, _| {
                edit_outcome(board.move_selected_card_vertically(1))
            }),
            "enter" => self.navigate(node, visible, out, |board| {
                if let Some(card) = board.selected_card {
                    board.open_card_details(board.selected_column, card);
                }
            }),
            "x" => self.edit(node, out, |board, _| {
                match (board.selected_card, board.selected_card().cloned()) {
                    (Some(card), Some(selected))
                        if !crate::board::checkbox_occurrences(&selected.title).is_empty() =>
                    {
                        edit_outcome(board.toggle_card_checkbox(board.selected_column, card, 0))
                    }
                    _ => Outcome::Quiet,
                }
            }),
            "n" => self.open_prompt(node, Prompt::NewCard, out),
            "c" => self.open_prompt(node, Prompt::NewColumn, out),
            "e" => {
                let prompt = self
                    .entries
                    .get(&node)
                    .and_then(|entry| entry.board.as_ref())
                    .map(|board| {
                        if board.selected_card.is_some() {
                            Prompt::RenameCard
                        } else {
                            Prompt::RenameColumn
                        }
                    });
                if let Some(prompt) = prompt {
                    self.open_prompt(node, prompt, out);
                }
            }
            "d" => {
                let prompt = self
                    .entries
                    .get(&node)
                    .and_then(|entry| entry.board.as_ref())
                    .and_then(delete_prompt);
                if let Some(prompt) = prompt {
                    self.open_prompt(node, prompt, out);
                }
            }
            "r" => self.reload(node, out),
            _ => {}
        }
    }

    fn navigate(
        &mut self,
        node: u64,
        visible: usize,
        out: &mut Vec<Submission>,
        step: impl FnOnce(&mut Board),
    ) {
        self.edit(node, out, |board, _| {
            step(board);
            board.ensure_selected_column_visible(visible);
            Outcome::Quiet
        });
    }

    fn open_prompt(&mut self, node: u64, prompt: Prompt, out: &mut Vec<Submission>) {
        let _ = out;
        if let Some(entry) = self.entries.get_mut(&node) {
            if entry.board.is_some() && entry.prompt.is_none() {
                entry.prompt = Some(prompt);
                refresh_grab(node, entry);
            }
        }
    }

    fn close_prompt(&mut self, node: u64) {
        if let Some(entry) = self.entries.get_mut(&node) {
            entry.prompt = None;
            refresh_grab(node, entry);
        }
    }

    fn on_prompt_key(&mut self, node: u64, chord: &str, prompt: Prompt, out: &mut Vec<Submission>) {
        match (chord, &prompt) {
            ("esc" | "n", _) => self.close_prompt(node),
            ("y", Prompt::Delete { is_column, .. }) => {
                self.close_prompt(node);
                self.commit(
                    node,
                    Prompt::Delete {
                        is_column: *is_column,
                        name: String::new(),
                    },
                    "",
                    out,
                );
            }
            _ => {}
        }
    }

    /// Applies the answer of a prompt.
    fn commit(&mut self, node: u64, prompt: Prompt, text: &str, out: &mut Vec<Submission>) {
        self.close_prompt(node);
        match prompt {
            Prompt::NewCard => self.edit(node, out, |board, _| edit_outcome(board.add_card(text))),
            Prompt::NewColumn => {
                self.edit(node, out, |board, _| edit_outcome(board.add_column(text)))
            }
            Prompt::RenameCard => self.edit(node, out, |board, _| {
                edit_outcome(board.rename_selected_card(text))
            }),
            Prompt::RenameColumn => self.rename_column(node, text, out),
            Prompt::Delete {
                is_column: false, ..
            } => self.edit(node, out, |board, _| {
                edit_outcome(board.delete_selected_card())
            }),
            Prompt::Delete {
                is_column: true, ..
            } => self.delete_column(node, out),
        }
    }

    fn rename_column(&mut self, node: u64, text: &str, out: &mut Vec<Submission>) {
        let mut renamed = None;
        self.edit(node, out, |board, _| {
            match board.rename_selected_column(text) {
                Ok(Some(pair)) => {
                    renamed = Some(pair);
                    Outcome::Quiet
                }
                Ok(None) => Outcome::Quiet,
                Err(message) => Outcome::Failed(message),
            }
        });
        let Some((from, to)) = renamed else {
            return;
        };
        let Some(mut entry) = self.entries.remove(&node) else {
            return;
        };
        match entry.storage.clone() {
            Storage::MarkdownFile { .. } => self.start_write(node, &mut entry, out),
            Storage::Folder { path } => match io::start_rename(&path, &from, &to) {
                Ok(request) => {
                    entry.write = Write::Structure;
                    self.requests.insert(request, (node, Purpose::Structure));
                }
                Err(message) => {
                    out.push(error_toast(&message));
                    self.write_failed(node, &mut entry, &message, out);
                }
            },
        }
        self.entries.insert(node, entry);
    }

    fn delete_column(&mut self, node: u64, out: &mut Vec<Submission>) {
        let mut removed = None;
        self.edit(node, out, |board, _| match board.delete_selected_column() {
            Ok(title) => {
                removed = Some(title);
                Outcome::Quiet
            }
            Err(message) => Outcome::Failed(message),
        });
        let Some(title) = removed else {
            return;
        };
        let Some(mut entry) = self.entries.remove(&node) else {
            return;
        };
        match entry.storage.clone() {
            Storage::MarkdownFile { .. } => self.start_write(node, &mut entry, out),
            Storage::Folder { path } => match io::start_remove(&path, &title) {
                Ok(request) => {
                    entry.write = Write::Structure;
                    self.requests.insert(request, (node, Purpose::Structure));
                }
                Err(message) => self.write_failed(node, &mut entry, &message, out),
            },
        }
        self.entries.insert(node, entry);
    }

    /// Opens a new board pane bound to `storage`. A board file is owned by one pane at a time:
    /// each pane would otherwise keep its own copy and could overwrite the other's edits.
    fn create(
        &mut self,
        parent: Option<u64>,
        title: String,
        storage: Storage,
        out: &mut Vec<Submission>,
    ) {
        let taken = self
            .entries
            .values()
            .map(|entry| &entry.storage)
            .chain(self.pending.iter().map(|binding| &binding.storage))
            .any(|existing| existing.path() == storage.path());
        if taken {
            out.push(error_toast("That board storage is already open"));
            return;
        }
        self.pending.push_back(Binding { storage });
        out.push(Submission::new(Intent::CreatePane(CreatePaneSpec {
            parent: parent.map(NodeId),
            title,
            content_kind: KIND.to_string(),
            cwd: None,
            command: None,
            initial_input: None,
        })));
    }

    /// Applies one action of the "New board" dialog.
    fn on_dialog_action(&mut self, action: &UiAction, out: &mut Vec<Submission>) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let step = dialog.act(
            &action.key,
            &action.action_id,
            action.kind,
            action.value.clone(),
        );
        // The kernel already removed a popup it closed itself; every other end closes it here.
        let close = || Submission::new(Intent::CloseView(create_dialog::view()));
        match step {
            create_dialog::Step::Stay => self.dialog = Some(dialog),
            create_dialog::Step::Cancel => {
                if action.kind != ActionKind::Closed {
                    out.push(close());
                }
            }
            create_dialog::Step::Create(created) => {
                out.push(close());
                self.create(created.parent, created.name, created.storage, out);
            }
        }
    }

    /// Applies one control command of an agent to the board of pane `node`.
    fn control(&mut self, node: u64, command: &control::Command, out: &mut Vec<Submission>) {
        if !self
            .entries
            .get(&node)
            .is_some_and(|entry| entry.board.is_some())
        {
            out.push(error_toast("That pane is not a loaded board"));
            return;
        }
        match command.action {
            control::Action::RenameColumn | control::Action::DeleteColumn => {
                let mut selected = false;
                self.edit(node, out, |board, _| {
                    let chosen = command
                        .column
                        .ok_or_else(|| "A column is required".to_string())
                        .and_then(|column| control::select_column(board, column));
                    match chosen {
                        Ok(()) => {
                            selected = true;
                            Outcome::Quiet
                        }
                        Err(message) => Outcome::Failed(message),
                    }
                });
                if !selected {
                    return;
                }
                if command.action == control::Action::RenameColumn {
                    match control::title_of(command) {
                        Ok(title) => self.rename_column(node, title, out),
                        Err(message) => out.push(error_toast(&message)),
                    }
                } else {
                    self.delete_column(node, out);
                }
            }
            _ => self.edit(node, out, |board, _| {
                edit_outcome(control::apply(board, command))
            }),
        }
    }

    fn reload(&mut self, node: u64, out: &mut Vec<Submission>) {
        let Some(mut entry) = self.entries.remove(&node) else {
            return;
        };
        if matches!(entry.write, Write::Idle) {
            entry.create_if_missing = false;
            self.start_load(node, &mut entry, out);
        }
        self.entries.insert(node, entry);
    }

    fn on_action(&mut self, action: &UiAction, out: &mut Vec<Submission>) {
        let ViewTarget::Pane(pane) = &action.view else {
            return;
        };
        let node = pane.0;
        if !self.entries.contains_key(&node) {
            return;
        }
        let key = action.key.as_str();
        match (key, action.kind, &action.value) {
            (key, ActionKind::Selected, Some(ActionValue::Text(id)))
                if view::parse_column_key(key).is_some() =>
            {
                if let Some((column, card)) = view::parse_card_id(id) {
                    self.edit(node, out, |board, _| {
                        board.select_card(column, card);
                        Outcome::Quiet
                    });
                }
            }
            (key, ActionKind::Activated, Some(ActionValue::Text(id)))
                if view::parse_column_key(key).is_some() =>
            {
                if let Some((column, card)) = view::parse_card_id(id) {
                    self.edit(node, out, |board, _| {
                        board.open_card_details(column, card);
                        Outcome::Quiet
                    });
                }
            }
            (view::PROMPT_KEY, ActionKind::Submitted, Some(ActionValue::Text(text))) => {
                let prompt = self
                    .entries
                    .get(&node)
                    .and_then(|entry| entry.prompt.clone());
                if let Some(prompt) = prompt {
                    self.commit(node, prompt, text, out);
                }
            }
            (
                view::DETAIL_TITLE_KEY,
                ActionKind::Changed | ActionKind::Submitted,
                Some(ActionValue::Text(text)),
            ) => self.edit(node, out, |board, _| {
                edit_outcome(board.set_detail_text(CardEditorField::Title, text))
            }),
            (
                view::DETAIL_BODY_KEY,
                ActionKind::Changed | ActionKind::Submitted,
                Some(ActionValue::Text(text)),
            ) => self.edit(node, out, |board, _| {
                edit_outcome(board.set_detail_text(CardEditorField::Body, text))
            }),
            (view::DETAIL_TITLE_KEY, ActionKind::FocusGained, _) => {
                self.edit(node, out, |board, _| {
                    board.set_detail_editor_focus(CardEditorField::Title);
                    Outcome::Quiet
                })
            }
            (view::DETAIL_BODY_KEY, ActionKind::FocusGained, _) => {
                self.edit(node, out, |board, _| {
                    board.set_detail_editor_focus(CardEditorField::Body);
                    Outcome::Quiet
                })
            }
            _ => {}
        }
    }
}

/// The delete question for the selection: the selected card, or the selected column.
fn delete_prompt(board: &Board) -> Option<Prompt> {
    match board.selected_card() {
        Some(card) => Some(Prompt::Delete {
            is_column: false,
            name: card.title.clone(),
        }),
        None => board
            .columns
            .get(board.selected_column)
            .map(|column| Prompt::Delete {
                is_column: true,
                name: column.title.clone(),
            }),
    }
}

/// Reads `{ "path", "kind" }` of the new-board command.
fn binding_from_arguments(arguments_json: &str) -> Result<Binding, String> {
    let value = serde_json::from_str::<Value>(arguments_json)
        .map_err(|_| "The arguments are not JSON".to_string())?;
    let path = value["path"]
        .as_str()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "A board needs a path".to_string())?
        .to_string();
    let storage = match value["kind"].as_str().unwrap_or("markdown") {
        "folder" => Storage::Folder { path },
        "markdown" => Storage::MarkdownFile { path },
        other => return Err(format!("Unknown board storage {other}")),
    };
    Ok(Binding { storage })
}

/// Handles one batch of events.
pub fn on_events(batch: Vec<Event>) -> Vec<Submission> {
    let mut out = Vec::new();
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if invoked.command == NEW_COMMAND => {
                    let arguments = serde_json::from_str::<Value>(&invoked.arguments_json)
                        .unwrap_or(Value::Null);
                    let parent = arguments["parent"].as_u64();
                    let has_path = arguments["path"]
                        .as_str()
                        .is_some_and(|path| !path.trim().is_empty());
                    if has_path {
                        match binding_from_arguments(&invoked.arguments_json) {
                            Ok(binding) => {
                                let title = arguments["name"]
                                    .as_str()
                                    .map(str::trim)
                                    .filter(|name| !name.is_empty())
                                    .map_or_else(|| pane_title(&binding.storage), str::to_string);
                                state.create(parent, title, binding.storage, &mut out);
                            }
                            Err(message) => out.push(error_toast(&message)),
                        }
                    } else {
                        state.dialog = Some(create_dialog::Dialog::new(parent));
                        out.push(Submission::new(Intent::OpenView(OpenViewSpec {
                            view: create_dialog::view(),
                            focus: true,
                        })));
                    }
                }
                Event::CommandInvoked(invoked) if invoked.command == control::COMMAND => {
                    match control::parse(&invoked.arguments_json) {
                        Ok((pane, command)) => state.control(pane, &command, &mut out),
                        Err(message) => out.push(error_toast(&message)),
                    }
                }
                Event::UiAction(action) if action.view == create_dialog::view() => {
                    state.on_dialog_action(&action, &mut out);
                }
                Event::CommandInvoked(invoked) if invoked.command == RELOAD_COMMAND => {
                    if let Some(pane) = invoked.context.pane {
                        state.reload(pane.0, &mut out);
                    }
                }
                Event::PaneCreated(info) if info.content_kind == KIND => {
                    let node = info.node.0;
                    if info.is_restored {
                        if let Some(storage) = recall_binding(node) {
                            state.bind(node, storage, false, &mut out);
                        }
                    } else if let Some(binding) = state.pending.pop_front() {
                        state.bind(node, binding.storage, true, &mut out);
                    } else if let Some(storage) = recall_binding(node) {
                        state.bind(node, storage, false, &mut out);
                    }
                }
                Event::NodeRemoved(node) => {
                    if let Some(mut entry) = state.entries.remove(&node.0) {
                        release_grab(&mut entry);
                        forget_binding(node.0);
                    }
                }
                Event::RequestCompleted(result) => {
                    if let Some(request) = io::call_number(&result) {
                        if let Some((node, purpose)) = state.requests.remove(&request) {
                            state.on_completed(node, purpose, request, &result, &mut out);
                        } else {
                            state.folder_completed(request, &result, &mut out);
                        }
                    }
                }
                Event::KeyPressed(key) => state.on_key(&key, &mut out),
                Event::UiAction(action) => state.on_action(&action, &mut out),
                Event::FsChanged(changed) => state.on_files_changed(&changed.paths, &mut out),
                Event::SettingsChanged(_) => {
                    let mut api = host();
                    state.settings = parse_settings(&HostSettings::get_json(&mut *api));
                }
                _ => {}
            }
        }
    });
    out
}

impl State {
    /// A folder write started several requests; the map holds one entry per request, so the
    /// remaining ones are looked up by the entry that owns them.
    fn folder_completed(
        &mut self,
        request: u64,
        result: &ilium_plugin_sdk::types::RequestResult,
        out: &mut Vec<Submission>,
    ) {
        let owner = self
            .entries
            .iter()
            .find_map(|(node, entry)| match &entry.write {
                Write::Folder(run) if run.owns(request) => Some(*node),
                _ => None,
            });
        if let Some(node) = owner {
            self.on_completed(node, Purpose::WriteFolder, request, result, out);
        }
    }

    /// A board file changed outside the plugin: boards that are not being written reload.
    fn on_files_changed(&mut self, paths: &[String], out: &mut Vec<Submission>) {
        let nodes: Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                matches!(entry.write, Write::Idle)
                    && paths.iter().any(|path| {
                        path == entry.storage.path()
                            || path.starts_with(&format!("{}/", entry.storage.path()))
                    })
            })
            .map(|(node, _)| *node)
            .collect();
        for node in nodes {
            self.reload(node, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_come_from_the_json_and_are_clamped() {
        assert_eq!(parse_settings(""), Settings::default());
        let parsed = parse_settings(r#"{"card_preview":false,"column_width":99}"#);
        assert!(!parsed.card_preview);
        assert_eq!(parsed.column_width, 60);
        assert_eq!(parse_settings(r#"{"column_width":1}"#).column_width, 12);
    }

    #[test]
    fn the_new_board_command_names_a_path_and_a_storage_kind() {
        let binding = binding_from_arguments(r#"{"path":"docs/plan.md"}"#).unwrap();
        assert_eq!(
            binding.storage,
            Storage::MarkdownFile {
                path: "docs/plan.md".to_string()
            }
        );
        let folder = binding_from_arguments(r#"{"path":"board","kind":"folder"}"#).unwrap();
        assert_eq!(
            folder.storage,
            Storage::Folder {
                path: "board".to_string()
            }
        );
        assert!(binding_from_arguments(r#"{"path":"  "}"#).is_err());
        assert!(binding_from_arguments(r#"{"path":"a","kind":"zip"}"#).is_err());
        assert!(binding_from_arguments("nope").is_err());
    }

    #[test]
    fn a_binding_survives_a_round_trip_through_its_stored_json() {
        for storage in [
            Storage::Folder {
                path: "b".to_string(),
            },
            Storage::MarkdownFile {
                path: "p.md".to_string(),
            },
        ] {
            assert_eq!(storage_from_json(&storage_json(&storage)), Some(storage));
        }
        assert_eq!(storage_from_json(&json!({"kind":"folder","path":""})), None);
        assert_eq!(storage_from_json(&json!({"kind":"x","path":"a"})), None);
    }

    #[test]
    fn the_pane_title_is_the_board_file_name_without_its_extension() {
        let markdown = Storage::MarkdownFile {
            path: "docs/plan.md".to_string(),
        };
        assert_eq!(pane_title(&markdown), "plan");
        let folder = Storage::Folder {
            path: "work/board/".to_string(),
        };
        assert_eq!(pane_title(&folder), "board");
        let hidden = Storage::MarkdownFile {
            path: ".md".to_string(),
        };
        assert_eq!(pane_title(&hidden), "Board");
    }

    #[test]
    fn the_delete_question_names_the_selected_card_or_else_the_column() {
        let mut board = Board::new(
            Storage::MarkdownFile {
                path: "p.md".to_string(),
            },
            default_columns(),
        );
        assert_eq!(
            delete_prompt(&board),
            Some(Prompt::Delete {
                is_column: true,
                name: "To do".to_string()
            })
        );
        board.add_card("write").unwrap();
        assert_eq!(
            delete_prompt(&board),
            Some(Prompt::Delete {
                is_column: false,
                name: "write".to_string()
            })
        );
    }

    #[test]
    fn typing_in_a_field_grabs_only_escape_and_browsing_grabs_every_board_key() {
        let mut entry = Entry {
            storage: Storage::MarkdownFile {
                path: "p.md".to_string(),
            },
            board: Some(Board::new(
                Storage::MarkdownFile {
                    path: "p.md".to_string(),
                },
                default_columns(),
            )),
            note: String::new(),
            revision: None,
            write: Write::Idle,
            dirty: false,
            prompt: None,
            grab: None,
            create_if_missing: false,
            area_columns: 100,
        };
        assert_eq!(
            wanted_chords(&entry).len(),
            BOARD_CHORDS.len() + ACTION_CHORDS.len()
        );
        entry.prompt = Some(Prompt::NewCard);
        assert_eq!(wanted_chords(&entry), vec!["esc".to_string()]);
        entry.prompt = Some(Prompt::Delete {
            is_column: false,
            name: "x".to_string(),
        });
        assert!(wanted_chords(&entry).contains(&"y".to_string()));
    }
}
