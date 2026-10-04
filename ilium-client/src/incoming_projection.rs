//! Storage ownership of original IPC allocations installed in UI projections.
//! Keys replace only complete state; journal entries retain individual owners.
use crate::{
    app::{AgentDebugLogCache, App},
    connection::EventRetention,
};
use ilium_core::{NodeId, Tree};
use ilium_ipc::ServerEvent;
use std::{collections::HashMap, mem::Discriminant};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Slot {
    Tree,
    DetectionSettings,
    DetectionSettingsError,
    Status,
    Evidence,
    Session,
    EditorPath,
    LastPrompt,
    Progress,
    Git,
    Debug(u64),
    Global(Discriminant<ServerEvent>),
}
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct Key {
    slot: Slot,
    scope: Option<NodeId>,
}
pub(crate) struct ProjectionUpdate {
    keys: Vec<Key>,
    reset_tree: bool,
    reset_evidence: bool,
    clear_session: Option<NodeId>,
    clear_detection_error: bool,
    pub(crate) derived_retention: Option<EventRetention>,
}
#[derive(Default)]
pub(crate) struct ProjectionRetention {
    owners: HashMap<Key, EventRetention>,
    // Remains charged even when pruning leaves spare buckets allocated.
    metadata_retention: Option<EventRetention>,
}
impl ProjectionRetention {
    pub(crate) fn prepare(event: &ServerEvent, app: &App) -> ProjectionUpdate {
        use ServerEvent::*;
        let global = Slot::Global(std::mem::discriminant(event));
        let mut update = ProjectionUpdate {
            keys: Vec::new(),
            reset_tree: false,
            reset_evidence: false,
            clear_session: None,
            clear_detection_error: false,
            derived_retention: None,
        };
        let (slot, scope) = match event {
            TreeSnapshot(_) => {
                update.reset_tree = true;
                (Slot::Tree, None)
            }
            PaneStateSnapshot {
                detection_evidence, ..
            } => {
                update.reset_tree = true;
                update.reset_evidence = true;
                update
                    .keys
                    .extend(detection_evidence.iter().map(|(pane_id, _)| Key {
                        slot: Slot::Evidence,
                        scope: Some(*pane_id),
                    }));
                (Slot::Tree, None)
            }
            PaneDetectedStateChanged {
                pane_id,
                status,
                evidence,
            } => {
                if !app
                    .tree
                    .get(*pane_id)
                    .is_some_and(ilium_core::Node::is_pane)
                {
                    return update;
                }
                if &evidence.applied_status == status {
                    update.keys.push(Key {
                        slot: Slot::Evidence,
                        scope: Some(*pane_id),
                    });
                }
                (Slot::Status, Some(*pane_id))
            }
            PaneStatusChanged { pane_id, .. } => (Slot::Status, Some(*pane_id)),
            PaneDetectionEvidenceChanged { pane_id, evidence } => {
                if !app.tree.get(*pane_id).is_some_and(|node| matches!(&node.kind,ilium_core::NodeKind::Pane{status,..} if status==&evidence.applied_status)) {return update;}
                (Slot::Evidence, Some(*pane_id))
            }
            PaneSessionIdResolved { pane_id, .. } => (Slot::Session, Some(*pane_id)),
            PaneSessionIdCleared { pane_id, .. } => {
                update.clear_session = Some(*pane_id);
                return update;
            }
            PaneEditorPathResolved { pane_id, path } => {
                if path.is_none() {
                    return update;
                }
                (Slot::EditorPath, Some(*pane_id))
            }
            PaneLastPromptChanged { pane_id, .. } => (Slot::LastPrompt, Some(*pane_id)),
            PaneProgressChanged { pane_id, .. } => (Slot::Progress, Some(*pane_id)),
            PaneGitStatusChanged { pane_id, .. } => (Slot::Git, Some(*pane_id)),
            PaneDebugLogSnapshot {
                pane_id, entries, ..
            } => {
                update.keys.extend(entries.iter().map(|entry| Key {
                    slot: Slot::Debug(entry.sequence),
                    scope: Some(*pane_id),
                }));
                return update;
            }
            PaneDebugEntryAppended { pane_id, entry } => {
                (Slot::Debug(entry.sequence), Some(*pane_id))
            }
            ProjectRestructureRejected { project_id, .. } => (global, Some(*project_id)),
            ScreenUpdate { .. }
            | TerminalReplay { .. }
            | VoiceTextOffered { .. }
            | VoiceTextResult { .. }
            | ProgressMonitorCheckCompleted { .. }
            | ProgressMonitorSetCompleted { .. }
            | ProgressMonitorStatusReported { .. }
            | ProgressMonitorCleared { .. }
            | ProjectRestructureApplied { .. }
            | InitialStateSyncComplete
            | NodeActivityChanged { .. }
            | NodeFocusCheckpointChanged { .. }
            | SessionRecoveryAvailable { .. }
            | PanePromptSubmitted { .. }
            | DebugLoggingChanged { .. }
            | AgentDebugMenuChanged { .. }
            | ProgressMonitorEnabledChanged { .. }
            | WorkspaceCreateProgress { .. }
            | WorkspaceCreated { .. }
            | WorkspaceRemoved { .. }
            | WorkspaceCloseOfferReported { .. }
            | PaneSessionTitleCleared { .. }
            | RepoFactsReported { .. }
            | WorkspaceCreateFailed { .. }
            | WorkspaceInventoryReported { .. }
            | WorkspacePruneCompleted { .. }
            | Error { .. }
            | PaneResizeRejected { .. }
            | WorkspaceRemovalBlocked { .. } => return update,
            AgentDetectionSettingsChanged { result, .. } => match result {
                Ok(_) => {
                    update.clear_detection_error = true;
                    (Slot::DetectionSettings, None)
                }
                Err(_) => (Slot::DetectionSettingsError, None),
            },
            TextTriggersChanged { .. } | PaneProcessTerminated { .. } => (global, None),
        };
        update.keys.push(Key { slot, scope });
        update
    }
    /// Reserve bucket storage before ANY consumer changes. Retain prior credit
    /// until actual growth has released the old table; no admission in commit.
    pub(crate) fn prepare_owned(
        event: &ServerEvent,
        app: &mut App,
        owner: Option<&EventRetention>,
    ) -> Result<ProjectionUpdate, ilium_execution::RejectReason> {
        let result = Self::prepare_owned_inner(event, app, owner);
        if let Some(owner) = owner {
            owner.note_projection_result(result.as_ref().err().copied());
        }
        result
    }
    fn prepare_owned_inner(
        event: &ServerEvent,
        app: &mut App,
        owner: Option<&EventRetention>,
    ) -> Result<ProjectionUpdate, ilium_execution::RejectReason> {
        let mut update = Self::prepare(event, app);
        if let Some(bytes) = app.incoming_derivation_bytes(event) {
            let owner = owner.ok_or(ilium_execution::RejectReason::InvalidCost)?;
            update.derived_retention = Some(owner.try_reserve_derived(bytes)?);
        }
        let registry = &mut app.incoming_projection;
        let target = registry
            .owners
            .len()
            .checked_add(update.keys.len())
            .ok_or(ilium_execution::RejectReason::WorkerBytes)?;
        if target > registry.owners.capacity() {
            let owner = owner.ok_or(ilium_execution::RejectReason::InvalidCost)?;
            let bytes =
                metadata_layout(target).ok_or(ilium_execution::RejectReason::WorkerBytes)?;
            let credit = owner.try_reserve_derived(bytes)?;
            registry
                .owners
                .try_reserve(update.keys.len())
                .map_err(|_| ilium_execution::RejectReason::WorkerBytes)?;
            registry.metadata_retention = Some(credit);
        }
        Ok(update)
    }
    /// Commit after the old projection payload has physically been dropped.
    pub(crate) fn commit(&mut self, update: ProjectionUpdate, owner: Option<&EventRetention>) {
        if update.reset_tree {
            self.owners.retain(|key, _| {
                !matches!(
                    key.slot,
                    Slot::Tree | Slot::Status | Slot::LastPrompt | Slot::Progress
                )
            });
        }
        if update.reset_evidence {
            self.owners
                .retain(|key, _| !matches!(key.slot, Slot::Evidence));
        }
        if let Some(pane_id) = update.clear_session {
            self.owners.remove(&Key {
                slot: Slot::Session,
                scope: Some(pane_id),
            });
        }
        if update.clear_detection_error {
            self.owners.remove(&Key {
                slot: Slot::DetectionSettingsError,
                scope: None,
            });
        }
        if let Some(owner) = owner {
            for key in update.keys {
                self.owners.insert(key, owner.clone());
            }
        }
    }
    pub(crate) fn prune(&mut self, tree: &Tree, logs: &HashMap<NodeId, AgentDebugLogCache>) {
        self.owners.retain(|key, _| {
            let Some(pane_id) = key.scope else {
                return true;
            };
            if tree.get(pane_id).is_none() {
                return false;
            }
            match key.slot {
                Slot::Debug(sequence) => logs.get(&pane_id).is_some_and(|cache| {
                    cache
                        .log
                        .entries
                        .binary_search_by_key(&sequence, |entry| entry.sequence)
                        .is_ok()
                }),
                _ => true,
            }
        });
    }
}
// The source-pinned hashbrown0.16.1 envelope reviewed for bounded decoding.
fn metadata_layout(count: usize) -> Option<usize> {
    let buckets = count.checked_mul(2)?.max(16).checked_next_power_of_two()?;
    let align = std::mem::align_of::<(Key, EventRetention)>().max(16);
    let offset = buckets
        .checked_mul(std::mem::size_of::<(Key, EventRetention)>())?
        .checked_add(align - 1)?
        & !(align - 1);
    offset.checked_add(buckets)?.checked_add(16)
}
/// Temporary preparation-key Vec only. Registry bucket storage has an
/// independent last-owner guard, rather than sharing payload-entry lifetimes.
pub(crate) fn projection_metadata_bytes(event: &ServerEvent) -> usize {
    let slots = match event {
        ServerEvent::PaneDebugLogSnapshot { entries, .. } => entries.len().saturating_add(1),
        ServerEvent::PaneStateSnapshot {
            detection_evidence, ..
        } => detection_evidence.len().saturating_add(1),
        ServerEvent::PaneDetectedStateChanged { .. } => 2,
        _ => 1,
    };
    slots
        .saturating_mul(2)
        .saturating_mul(std::mem::size_of::<Key>())
        .saturating_add(512)
}
