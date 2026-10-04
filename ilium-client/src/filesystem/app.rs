use super::editors::{EditorCompletion, LoadTarget, SavePurpose, SaveTarget};
use super::ordered::WriteCompletion;
use crate::app::{App, Mode, PaneRuntime};
use ilium_core::{NodeId, NodeKind, PaneContentKind};
use ilium_execution::JobOutcome;
use std::path::PathBuf;
use std::sync::Arc;

impl App {
    pub(crate) async fn drain_filesystem(&mut self) -> std::io::Result<()> {
        self.pending_editor_loads.clear();
        self.pending_board_loads.clear();
        self.editor_load_retry_positions.clear();
        // These are replaceable reads, not accepted writes. Release their
        // original decoded paths before releasing the incoming allocation.
        self.restored_editor_paths
            .retain(|pane, _| !self.editor_path_event_holds.contains_key(pane));
        self.editor_path_event_holds.clear();
        self.close_explorers();
        if let Some(files) = &mut self.sidebar_files {
            files.close();
        }
        if let Some(files) = &mut self.integration_files {
            files.close_admission();
        }
        if let Some(files) = &mut self.editor_files {
            files.close_admission();
        }
        if let Some(boards) = &mut self.board_files {
            boards.close_admission();
        }
        if let Some(configurations) = &mut self.configuration_files {
            configurations.close_admission();
        }
        let editor_notification = self.editor_files.as_ref().map(|files| files.notification());
        let board_notification = self.board_files.as_ref().map(|files| files.notification());
        let configuration_notification = self
            .configuration_files
            .as_ref()
            .map(|files| files.notification());
        let integration_notification = self
            .integration_files
            .as_ref()
            .map(|files| files.notification());
        // Absent owners have no wake source. A stack-owned never-notified
        // fallback avoids allocating a placeholder service or busy polling.
        let absent = tokio::sync::Notify::new();
        let animation_notification = self.animation_frame.notification();
        let animation_admission = self.animation_frame.admission_notification();
        let execution_admission = crate::execution::admission_notification();
        let drain = async {
            loop {
                // Register every dependency before observing progress. A
                // release between collection and awaiting cannot be lost.
                let editor_ready = editor_notification.as_deref().unwrap_or(&absent).notified();
                let board_ready = board_notification.as_deref().unwrap_or(&absent).notified();
                let configuration_ready = configuration_notification
                    .as_deref()
                    .unwrap_or(&absent)
                    .notified();
                let integration_ready = integration_notification
                    .as_deref()
                    .unwrap_or(&absent)
                    .notified();
                let animation_ready = animation_notification.notified();
                let admission_ready = animation_admission.notified();
                let execution_ready = execution_admission.notified();
                tokio::pin!(
                    editor_ready,
                    board_ready,
                    configuration_ready,
                    integration_ready,
                    animation_ready,
                    admission_ready,
                    execution_ready
                );
                editor_ready.as_mut().enable();
                board_ready.as_mut().enable();
                configuration_ready.as_mut().enable();
                integration_ready.as_mut().enable();
                animation_ready.as_mut().enable();
                admission_ready.as_mut().enable();
                execution_ready.as_mut().enable();
                self.animation_frame.collect();
                self.collect_editor_files();
                if self
                    .editor_files
                    .as_ref()
                    .is_none_or(|files| files.pending() == 0)
                    && self
                        .board_files
                        .as_ref()
                        .is_none_or(|files| files.pending() == 0)
                    && self
                        .configuration_files
                        .as_ref()
                        .is_none_or(|files| files.pending() == 0)
                    && self
                        .integration_files
                        .as_ref()
                        .is_none_or(|files| files.pending() == 0)
                {
                    return;
                }
                tokio::select! {
                    _=editor_ready=>{},
                    _=board_ready=>{},
                    _=configuration_ready=>{},
                    _=integration_ready=>{},
                    _=animation_ready=>{},
                    _=admission_ready=>{},
                    _=execution_ready=>{},
                }
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), drain)
            .await
            .map_err(|_| {
                std::io::Error::other(
                    "Filesystem drain deadline; pending write publication is unconfirmed",
                )
            })
    }
    #[cfg(test)]
    pub(crate) fn settle_filesystem_for_test(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            self.animation_frame.collect();
            self.collect_editor_files();
            if self.editor_path_event_holds.is_empty()
                && self.editor_load_retry_positions.is_empty()
                && self
                    .terminal_baselines
                    .as_ref()
                    .is_none_or(|files| !files.pending())
                && self
                    .sidebar_files
                    .as_ref()
                    .is_none_or(|files| !files.pending())
                && !self.explorer_preparation_pending()
                && self.pending_editor_loads.is_empty()
                && self.pending_board_loads.is_empty()
                && self
                    .editor_files
                    .as_ref()
                    .is_none_or(|files| files.pending() == 0)
                && self
                    .board_files
                    .as_ref()
                    .is_none_or(|files| files.pending() == 0)
                && self
                    .configuration_files
                    .as_ref()
                    .is_none_or(|files| files.pending() == 0)
                && self
                    .integration_files
                    .as_ref()
                    .is_none_or(|files| files.pending() == 0)
            {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Filesystem work did not settle: {:?}",
                self.status_message
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    pub(crate) fn remember_editor_path(
        &mut self,
        pane_id: NodeId,
        path: PathBuf,
    ) -> Result<(), String> {
        if path.capacity() > 64 * 1024 {
            return Err("Editor path exceeds retained byte limit".into());
        }
        if !matches!(
            self.tree.get(pane_id).map(|node| &node.kind),
            Some(NodeKind::Pane {
                content: PaneContentKind::Editor,
                ..
            })
        ) {
            return Err("Editor path no longer belongs to a live editor".into());
        }
        let same = self.restored_editor_paths.get(&pane_id) == Some(&path);
        if !same || !self.editor_path_holds.contains_key(&pane_id) {
            match crate::execution::process_quota()
                .reserve_external_storage(path.capacity().saturating_mul(4).saturating_add(4096))
            {
                Ok(hold) => {
                    self.editor_path_holds.insert(pane_id, Arc::new(hold));
                    self.editor_path_event_holds.remove(&pane_id);
                }
                Err(reason) => {
                    let Some(original) = self.processing_event_retention.clone() else {
                        return Err(format!("Editor path admission pending: {reason:?}"));
                    };
                    // Keep the original decoded allocation; derive nothing until
                    // storage is admitted. Its incoming credit bounds this map.
                    self.editor_path_event_holds.insert(pane_id, original);
                    self.editor_load_retry_positions
                        .insert(pane_id, (None, None));
                    self.editor_path_holds.remove(&pane_id);
                    self.restored_editor_paths.insert(pane_id, path);
                    self.status_message = Some(format!(
                        "Editor path retained; preparation admission pending: {reason:?}"
                    ));
                    return Ok(());
                }
            }
        }
        self.restored_editor_paths.insert(pane_id, path);
        Ok(())
    }
    fn retry_editor_path_admissions(&mut self) {
        let ids: Vec<_> = self.editor_load_retry_positions.keys().copied().collect();
        for pane_id in ids {
            let live = matches!(
                self.tree.get(pane_id).map(|node| &node.kind),
                Some(NodeKind::Pane {
                    content: PaneContentKind::Editor,
                    ..
                })
            );
            if !live {
                self.restored_editor_paths.remove(&pane_id);
                self.editor_path_holds.remove(&pane_id);
                self.editor_path_event_holds.remove(&pane_id);
                self.editor_load_retry_positions.remove(&pane_id);
                continue;
            }
            let Some(path) = self.restored_editor_paths.get(&pane_id) else {
                self.editor_path_event_holds.remove(&pane_id);
                self.editor_load_retry_positions.remove(&pane_id);
                continue;
            };
            if !self.editor_path_holds.contains_key(&pane_id) {
                let Ok(hold) = crate::execution::process_quota().reserve_external_storage(
                    path.capacity().saturating_mul(4).saturating_add(4096),
                ) else {
                    continue;
                };
                self.editor_path_holds.insert(pane_id, Arc::new(hold));
            }
            if self.panes.contains_key(&pane_id) {
                self.editor_path_event_holds.remove(&pane_id);
                self.editor_load_retry_positions.remove(&pane_id);
                continue;
            }
            if self.pending_editor_loads.len() >= 16 {
                continue;
            }
            let (line, column) = self
                .editor_load_retry_positions
                .remove(&pane_id)
                .unwrap_or((None, None));
            self.pending_editor_loads
                .retain(|target| target.pane_id != pane_id);
            self.pending_editor_loads.push(LoadTarget {
                pane_id,
                path: path.clone(),
                line,
                column,
                source_hold: None,
            });
            self.editor_load_errors.remove(&pane_id);
            self.editor_path_event_holds.remove(&pane_id);
        }
    }
    /// Queue finite load preparation. Missing bootstrap is explicit; there is
    /// no synchronous production fallback into the coordinator loop.
    pub(crate) fn request_editor_load(
        &mut self,
        pane_id: NodeId,
        path: PathBuf,
        line: Option<u32>,
        column: Option<u32>,
    ) -> bool {
        if path.capacity() > 64 * 1024 {
            self.status_message = Some("Editor path exceeds retained byte limit".into());
            return false;
        }
        // This covers the map path, pending target, and finite-job path
        // copies before any of them are constructed.
        if !self.editor_path_holds.contains_key(&pane_id)
            || self.restored_editor_paths.get(&pane_id) != Some(&path)
        {
            let hold = match crate::execution::process_quota()
                .reserve_external_storage(path.capacity().saturating_mul(4).saturating_add(4096))
            {
                Ok(hold) => Arc::new(hold),
                Err(reason) => {
                    self.status_message =
                        Some(format!("Editor path admission pending: {reason:?}"));
                    return false;
                }
            };
            self.editor_path_holds.insert(pane_id, hold);
        }
        self.restored_editor_paths.insert(pane_id, path.clone());
        self.editor_load_retry_positions
            .insert(pane_id, (line, column));
        self.editor_load_errors.remove(&pane_id);
        if let Some(index) = self
            .pending_editor_loads
            .iter()
            .position(|target| target.pane_id == pane_id)
        {
            self.pending_editor_loads.remove(index);
        }
        if self.pending_editor_loads.len() >= 16 {
            self.status_message =
                Some("Editor load queue full; file path retained for retry".into());
            return false;
        }
        self.pending_editor_loads.push(LoadTarget {
            pane_id,
            path,
            line,
            column,
            source_hold: None,
        });
        self.editor_load_retry_positions.remove(&pane_id);
        self.collect_editor_files();
        true
    }
    pub(crate) fn editor_load_error(&self, pane_id: NodeId) -> Option<&str> {
        self.editor_load_errors.get(&pane_id).map(String::as_str)
    }
    pub(crate) fn collect_editor_files(&mut self) -> bool {
        self.retry_editor_path_admissions();
        let configuration_changed = self.collect_configuration_files()
            | self.collect_board_files()
            | self.collect_explorers()
            | self.collect_sidebar_files()
            | self.collect_integration_files()
            | self.collect_terminal_baselines();
        self.editor_load_errors.retain(|pane, _| {
            matches!(
                self.tree.get(*pane).map(|node| &node.kind),
                Some(NodeKind::Pane {
                    content: PaneContentKind::Editor,
                    ..
                })
            )
        });

        let Some(mut files) = self.editor_files.take() else {
            return configuration_changed;
        };
        let mut changed = configuration_changed;
        while let Some(completion) = files.poll() {
            changed = true;
            match completion {
                EditorCompletion::Loaded {
                    target,
                    outcome,
                    current,
                } => {
                    let valid = current
                        && !self.panes.contains_key(&target.pane_id)
                        && self.restored_editor_paths.get(&target.pane_id) == Some(&target.path)
                        && matches!(
                            self.tree.get(target.pane_id).map(|node| &node.kind),
                            Some(NodeKind::Pane {
                                content: PaneContentKind::Editor,
                                ..
                            })
                        );
                    let (outcome, retention) = outcome.into_parts();
                    let _retained = retention.retain(outcome).map(|outcome| {
                        if !valid {
                            return;
                        }
                        match outcome {
                            JobOutcome::Finished(Ok(source)) => {
                                let mut editor =
                                    crate::editor_pane::EditorPane::from_source(source);
                                editor.apply_defaults(&self.editor_settings);
                                if let Some(line) = target.line {
                                    editor.jump_to_location(
                                        line.saturating_sub(1) as usize,
                                        target.column.unwrap_or(1).saturating_sub(1) as usize,
                                    );
                                }
                                self.editor_load_errors.remove(&target.pane_id);
                                self.panes
                                    .insert(target.pane_id, PaneRuntime::Editor(Box::new(editor)));
                                self.rebuild_rendered_markdown(target.pane_id);
                            }
                            JobOutcome::Finished(Err(error)) => {
                                self.editor_load_errors.insert(target.pane_id, error);
                            }
                            JobOutcome::NotStarted { .. } => {
                                self.editor_load_errors.insert(
                                    target.pane_id,
                                    "Editor load cancelled before execution".into(),
                                );
                            }
                            JobOutcome::Panicked => {
                                self.editor_load_errors
                                    .insert(target.pane_id, "Editor load worker failed".into());
                            }
                        }
                    });
                }
                EditorCompletion::LoadLost { target, current } => {
                    if current {
                        self.editor_load_errors.insert(
                            target.pane_id,
                            "Editor load result lost; retry opening the file".into(),
                        );
                    }
                }
                EditorCompletion::Saved { target, completion } => {
                    let result = match completion {
                        WriteCompletion::Outcome { outcome, .. } => {
                            let mut result = None;
                            let _retained = outcome.map(|outcome| {
                                result = Some(match outcome {
                                    JobOutcome::Finished(result) => result,
                                    JobOutcome::NotStarted { .. } => {
                                        Err("Save never started; buffer remains unsaved".into())
                                    }
                                    JobOutcome::Panicked => {
                                        Err("Save worker failed; publication state is unknown"
                                            .into())
                                    }
                                });
                            });
                            result.unwrap_or_else(|| Err("Save outcome unavailable".into()))
                        }
                        WriteCompletion::Rejected { rejection, .. } => {
                            Err(format!("Save did not run: {:?}", rejection.reason))
                        }
                        WriteCompletion::Lost { .. } => {
                            Err("Save result lost; publication state is unknown".into())
                        }
                    };
                    let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&target.pane_id)
                    else {
                        continue;
                    };
                    if !Arc::ptr_eq(&editor.instance_identity(), &target.identity) {
                        continue;
                    }
                    editor.pending_saves = editor.pending_saves.saturating_sub(1);
                    let latest = editor
                        .latest_save_operation
                        .as_ref()
                        .is_some_and(|operation| Arc::ptr_eq(operation, &target.operation));
                    if !latest {
                        continue;
                    }
                    editor.latest_save_operation = None;
                    match result {
                        Ok(saved) if editor.path == target.old_path => {
                            if matches!(target.purpose, SavePurpose::SaveAs) {
                                editor.retarget_path(saved.path.clone());
                            }
                            editor.acknowledge_saved_revision(saved.source_revision);
                            let dirty = editor.dirty;
                            if matches!(target.purpose, SavePurpose::SaveAs) {
                                self.rebuild_rendered_markdown(target.pane_id);
                                if let Some(name) =
                                    saved.path.file_name().and_then(|name| name.to_str())
                                {
                                    self.request_rename(
                                        target.pane_id,
                                        name.to_owned(),
                                        None,
                                        None,
                                    );
                                }
                                if matches!(&self.mode, Mode::SaveAs(id, prompt) if *id == target.pane_id && target.prompt_input.as_ref() == Some(&prompt.buf))
                                {
                                    self.mode = Mode::Normal;
                                }
                            }
                            self.status_message = Some(
                                if dirty {
                                    "Previous revision saved; newer edits remain unsaved"
                                } else {
                                    "Saved"
                                }
                                .into(),
                            );
                            if matches!(target.purpose, SavePurpose::OpenAsBoard) && !dirty {
                                self.request_board_from_markdown_editor(target.pane_id);
                            }
                        }
                        Ok(_) => {
                            self.status_message = Some(
                                "Saved previous destination; current editor remains unchanged"
                                    .into(),
                            )
                        }
                        Err(error) => {
                            self.status_message =
                                Some(format!("Save failed; buffer remains unsaved: {error}"))
                        }
                    }
                }
                EditorCompletion::UnmatchedWrite(_) => {
                    self.status_message = Some(
                        "Save acknowledgement identity mismatch; publication state is unknown"
                            .into(),
                    )
                }
            }
        }
        while let Some(target) = self.pending_editor_loads.first_mut() {
            if !self.tree.get(target.pane_id).is_some_and(|node| {
                matches!(
                    node.kind,
                    NodeKind::Pane {
                        content: PaneContentKind::Editor,
                        ..
                    }
                )
            }) {
                self.pending_editor_loads.remove(0);
                continue;
            }
            match files.request_load(target) {
                Ok(()) => {
                    self.pending_editor_loads.remove(0);
                }
                Err(error) => {
                    self.status_message = Some(error);
                    break;
                }
            }
        }
        self.editor_files = Some(files);
        changed
    }
    pub(crate) fn enqueue_editor_save(
        &mut self,
        pane_id: NodeId,
        path: PathBuf,
        purpose: SavePurpose,
    ) -> Result<(), String> {
        if path.capacity() > 64 * 1024 {
            return Err("Editor destination exceeds retained path limit".into());
        }
        let prompt_input = match &self.mode {
            Mode::SaveAs(id, prompt) if *id == pane_id => {
                if prompt.buf.len() > 64 * 1024 {
                    return Err("Save As input exceeds retained path limit".into());
                }
                Some(prompt.buf.clone())
            }
            _ => None,
        };
        let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&pane_id) else {
            return Err("Pane is not an editor".into());
        };
        let operation = Arc::new(());
        let target = SaveTarget {
            pane_id,
            identity: editor.instance_identity(),
            revision: editor.content_revision(),
            old_path: editor.path.clone(),
            purpose,
            operation: Arc::clone(&operation),
            prompt_input,
        };
        let files = self.editor_files.as_mut().ok_or_else(|| {
            "Filesystem workers are unavailable; buffer remains unsaved".to_string()
        })?;
        files.save(target, path, editor.textarea.lines())?;
        editor.pending_saves += 1;
        editor.latest_save_operation = Some(operation);
        editor.acknowledge_autosave_admission();
        self.status_message = Some("Saving…".into());
        Ok(())
    }
}

impl App {
    fn close_explorers(&mut self) {
        fn close(mode: &mut Mode) {
            match mode {
                Mode::Explorer(overlay, _)
                | Mode::FolderExplorer(overlay, _)
                | Mode::ProjectFolderExplorer(overlay, _)
                | Mode::BoardPathPicker(overlay) => overlay.close_preparation(),
                _ => {}
            }
        }
        close(&mut self.mode);
        for mode in &mut self.modal_stack {
            close(mode);
        }
        self.explorer_execution = None;
    }
    #[cfg(test)]
    fn explorer_preparation_pending(&self) -> bool {
        fn pending(mode: &Mode) -> bool {
            match mode {
                Mode::Explorer(overlay, _)
                | Mode::FolderExplorer(overlay, _)
                | Mode::ProjectFolderExplorer(overlay, _)
                | Mode::BoardPathPicker(overlay) => overlay.preparation_pending(),
                _ => false,
            }
        }
        pending(&self.mode) || self.modal_stack.iter().any(pending)
    }
    fn collect_explorers(&mut self) -> bool {
        fn poll_mode(mode: &mut Mode) -> bool {
            match mode {
                Mode::Explorer(overlay, _)
                | Mode::FolderExplorer(overlay, _)
                | Mode::ProjectFolderExplorer(overlay, _)
                | Mode::BoardPathPicker(overlay) => overlay.poll(),
                _ => false,
            }
        }
        let mut changed = poll_mode(&mut self.mode);
        for mode in &mut self.modal_stack {
            changed |= poll_mode(mode);
        }
        changed
    }
}
