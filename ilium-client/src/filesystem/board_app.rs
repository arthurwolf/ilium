use super::boards::BoardLoadTarget;
use crate::app::{App, Mode, PaneRuntime};
use ilium_core::{BoardStorage, NodeId, NodeKind, PaneContentKind};
use std::sync::Arc;

#[derive(PartialEq, Eq)]
pub(crate) enum BoardDialog {
    Card(ilium_core::NodeId, String),
    Column(ilium_core::NodeId, String),
    Rename(ilium_core::NodeId, crate::app::BoardRenameTarget, String),
    Delete(ilium_core::NodeId, crate::app::BoardDeleteTarget),
}

pub(crate) struct BoardDialogReceipt {
    identity: Arc<()>,
    revision: u64,
    dialog: BoardDialog,
}
fn current_dialog(mode: &Mode) -> Option<BoardDialog> {
    match mode {
        Mode::BoardCardPrompt(pane, state) => Some(BoardDialog::Card(*pane, state.buf.clone())),
        Mode::BoardColumnPrompt(pane, state) => Some(BoardDialog::Column(*pane, state.buf.clone())),
        Mode::BoardRenamePrompt(pane, target, state) => {
            Some(BoardDialog::Rename(*pane, *target, state.buf.clone()))
        }
        Mode::BoardDeleteConfirm(pane, target) => Some(BoardDialog::Delete(*pane, *target)),
        _ => None,
    }
}

impl App {
    pub(crate) fn remember_board_modal(&mut self, pane_id: NodeId) {
        let Some(dialog) = current_dialog(&self.mode) else {
            return;
        };
        if self.pending_board_dialogs.len() >= 32 {
            self.status_message =
                Some("Board saved acknowledgement pending; dialog retained".into());
            return;
        }
        if let Some(PaneRuntime::Board(board)) = self.panes.get(&pane_id) {
            if let Some(writer) = &board.writer {
                self.pending_board_dialogs.push(BoardDialogReceipt {
                    identity: writer.identity(),
                    revision: board.intent_revision(),
                    dialog,
                });
            }
        }
    }

    pub(crate) fn request_board_load_from_tree(&mut self, pane_id: NodeId) -> bool {
        let Some(NodeKind::Pane {
            board_storage: Some(storage),
            ..
        }) = self.tree.get(pane_id).map(|node| &node.kind)
        else {
            return false;
        };
        let result = self
            .board_files
            .as_mut()
            .ok_or_else(|| "Board file worker unavailable".to_owned())
            .and_then(|files| files.request_pane(pane_id, storage));
        if let Err(error) = result {
            if !self.pending_board_loads.contains(&pane_id) && self.pending_board_loads.len() < 16 {
                self.pending_board_loads.push(pane_id);
            }
            self.status_message = Some(error);
            return false;
        }
        self.pending_board_loads
            .retain(|pending| *pending != pane_id);
        true
    }
    pub(crate) fn request_board_load(&mut self, pane_id: NodeId, storage: BoardStorage) -> bool {
        let result = self
            .board_files
            .as_mut()
            .ok_or_else(|| "Board file worker unavailable".to_owned())
            .and_then(|files| {
                files.request(
                    BoardLoadTarget::Pane(pane_id, storage.clone()),
                    storage,
                    true,
                )
            });
        if let Err(error) = result {
            self.status_message = Some(error);
            return false;
        }
        true
    }
    pub(crate) fn collect_board_files(&mut self) -> bool {
        let Some(mut files) = self.board_files.take() else {
            return false;
        };
        // Canonical tree paths remain owned while a source-storage reservation
        // is unavailable. Retry admission only; no IO job waits for that space.
        self.pending_board_loads.retain(|pane_id| {
            self.tree.get(*pane_id).is_some_and(|node| {
                matches!(
                    &node.kind,
                    NodeKind::Pane {
                        content: PaneContentKind::Board,
                        board_storage: Some(_),
                        ..
                    }
                )
            })
        });
        self.pending_board_loads.retain(|pane_id| {
            let Some(NodeKind::Pane {
                board_storage: Some(storage),
                ..
            }) = self.tree.get(*pane_id).map(|node| &node.kind)
            else {
                return false;
            };
            files.request_pane(*pane_id, storage).is_err()
        });
        let mut changed = false;
        if let Some((parent, initial, result)) = files.poll_suggestion() {
            if let Mode::CreateBoard(state) = &mut self.mode {
                if state.parent_group == parent && state.path.buf == initial.display().to_string() {
                    match result {
                        Ok(path) => {
                            state.path =
                                crate::text_prompt::TextPromptState::new(path.display().to_string())
                        }
                        Err(error) => self.status_message = Some(error),
                    }
                    changed = true;
                }
            }
        }

        while let Some(loaded) = files.poll_read() {
            changed = true;
            if let Ok(source) = &loaded.result {
                if !matches!(loaded.target, BoardLoadTarget::Pane(..)) {
                    if files.is_closing() {
                        tracing::warn!(
                            "completed board open cancelled during client shutdown; disk data retained"
                        );
                        self.status_message = Some(
                            "Board remains on disk; opening cancelled during client shutdown"
                                .into(),
                        );
                        continue;
                    }
                    if !self.try_finish_new_board(&loaded.target, source) {
                        files.retain_handoff(loaded);
                        break;
                    }
                }
            }
            match loaded.result {
                Err(error) => {
                    let operation = if matches!(loaded.target, BoardLoadTarget::Create(..)) {
                        "create"
                    } else {
                        "load"
                    };
                    self.status_message = Some(format!("Could not {operation} board: {error}"));
                }
                Ok(source) => match loaded.target {
                    BoardLoadTarget::Pane(pane_id, expected) => {
                        if !matches!(self.tree.get(pane_id).map(|node|&node.kind),Some(NodeKind::Pane{content:PaneContentKind::Board,board_storage:Some(storage),..}) if *storage==expected)
                        {
                            continue;
                        }
                        let old_revision = match self.panes.get(&pane_id) {
                            Some(PaneRuntime::Board(board)) => board.content_revision(),
                            _ => 0,
                        };
                        let source_changed = match self.panes.get(&pane_id) {
                            Some(PaneRuntime::Board(board)) => board.columns != source.columns,
                            _ => false,
                        };
                        match files.attach(source) {
                            Ok(mut board) => {
                                board.set_reload_revision(old_revision + u64::from(source_changed));
                                self.panes
                                    .insert(pane_id, PaneRuntime::Board(Box::new(board)));
                                if source_changed {
                                    self.record_client_node_activity(pane_id);
                                }
                            }
                            Err(error) => self.status_message = Some(error),
                        }
                    }
                    BoardLoadTarget::Create(state, _) => {
                        let matching = matches!(&self.mode,Mode::CreateBoard(current) if current.parent_group==state.parent_group && current.path.buf==state.path.buf && current.name.buf==state.name.buf && current.storage_kind==state.storage_kind);
                        // Creation was admitted and durable even if its dialog
                        // was later cancelled. Preserve the file; only the
                        // unchanged modal may open its new pane automatically.
                        if matching {
                            self.mode = Mode::Normal;
                        }
                    }
                    BoardLoadTarget::Open { .. } => {}
                },
            }
        }
        while let Some(ack) = files.poll_write() {
            changed = true;
            let pane = self
                .panes
                .iter_mut()
                .find_map(|(pane_id, runtime)| match runtime {
                    PaneRuntime::Board(board)
                        if board.writer.as_ref().is_some_and(|writer| {
                            Arc::ptr_eq(&writer.identity(), &ack.identity)
                        }) =>
                    {
                        Some((*pane_id, board))
                    }
                    _ => None,
                });
            let dialog = ack
                .revision
                .and_then(|revision| {
                    self.pending_board_dialogs.iter().position(|pending| {
                        pending.revision == revision
                            && Arc::ptr_eq(&pending.identity, &ack.identity)
                    })
                })
                .map(|index| self.pending_board_dialogs.remove(index).dialog);
            match ack.result {
                Ok(revision) => {
                    if let Some((pane_id, board)) = pane {
                        let before = board.content_revision();
                        board.acknowledge_revision(revision);
                        if before != board.content_revision() {
                            self.record_client_node_activity(pane_id);
                        }
                    }
                    if dialog.is_some() && dialog == current_dialog(&self.mode) {
                        self.mode = Mode::Normal;
                    }
                    self.status_message = Some("Board saved to disk".into());
                }
                Err(error) => {
                    if error.unchanged {
                        if let (Some((_, board)), Some(revision), Some(rollback)) =
                            (pane, ack.revision, ack.rollback)
                        {
                            board.apply_failed_rollback(revision, rollback);
                        }
                    }
                    self.status_message = Some(format!(
                        "Board edits remain unsaved: {}; failed authored data retained",
                        error.message
                    ))
                }
            }
        }
        self.board_files = Some(files);
        changed
    }
    /// False retains the original completed result at the ordered read head.
    /// Derivative path/name allocations and focus happen only after admission.
    pub(crate) fn try_finish_new_board(
        &mut self,
        target: &BoardLoadTarget,
        source: &crate::board::BoardSource,
    ) -> bool {
        let parent_group = match target {
            BoardLoadTarget::Create(state, _) => {
                if !matches!(&self.mode, Mode::CreateBoard(current) if current.parent_group == state.parent_group && current.path.buf == state.path.buf && current.name.buf == state.name.buf && current.storage_kind == state.storage_kind)
                {
                    self.status_message = Some(
                        "Board created on disk; opening cancelled after dialog changed".into(),
                    );
                    return true;
                }
                state.parent_group
            }
            BoardLoadTarget::Open { parent_group, .. } => *parent_group,
            BoardLoadTarget::Pane(..) => return true,
        };
        if !self
            .tree
            .get(parent_group)
            .is_some_and(ilium_core::Node::accepts_normal_children)
        {
            self.status_message =
                Some("Board remains on disk; opening cancelled after parent group removal".into());
            return true;
        }
        if let Some(pane) = self.board_pane_for_path(source.storage.path()) {
            self.focus_pane(pane);
            self.status_message = Some("That board storage is already open".into());
            return true;
        }
        let Some(client) = &self.outbound_admission else {
            self.status_message = Some(
                "Board remains on disk; opening retained because request admission is unavailable"
                    .into(),
            );
            return false;
        };
        let path_bytes = super::boards::storage_path_capacity(&source.storage);
        let name_bytes = match target {
            BoardLoadTarget::Create(state, _) => state.name.buf.capacity(),
            _ => path_bytes.saturating_mul(3),
        };
        // Covers path clone, lossy name conversion, pending-focus name and
        // request metadata while the original charged source remains alive.
        let bytes = path_bytes
            .saturating_mul(4)
            .saturating_add(name_bytes.saturating_mul(3))
            .saturating_add(4096);
        let reservation = match crate::ipc_preparation::reserve_request(client, bytes) {
            Ok(reservation) => reservation,
            Err(reason) => {
                self.status_message = Some(format!(
                    "Board remains on disk; opening retained pending request admission: {reason:?}"
                ));
                return false;
            }
        };
        let name = match target {
            BoardLoadTarget::Create(state, _) if !state.name.buf.trim().is_empty() => {
                state.name.buf.trim().to_owned()
            }
            BoardLoadTarget::Create(..) => "Board".into(),
            _ => source
                .storage
                .path()
                .file_stem()
                .map(|name| name.to_string_lossy().into_owned())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Board".into()),
        };
        let request = ilium_ipc::ClientRequest::NewBoard {
            parent_group,
            name: name.clone(),
            storage: source.storage.clone(),
        };
        let request = match reservation.retain(request) {
            Ok(request) => request,
            Err(rejected) => {
                self.status_message = Some(format!(
                    "Board opening retained; request storage refused: {:?}",
                    rejected.reason
                ));
                return false;
            }
        };
        self.record_pending_pane_focus(parent_group, PaneContentKind::Board, name);
        self.publish_terminal_request_after_baseline(request);
        true
    }
}
