//! Exhaustive owned-allocation visitor. This runs on a CPU codec worker,
//! never on the event loop; capacities count unused allocations too.
use crate::*;
use ilium_core::{
    AgentActivity, AgentClass, NodeActivityRevision, NodeId, PaneProgress, PaneStatus,
    ProgressTaskReport, Tree,
};
use std::mem::{size_of, size_of_val};
use std::path::PathBuf;
trait HeapBytes {
    fn heap_bytes(&self) -> usize;
}
macro_rules! scalars { ($($kind:ty),*) => { $(impl HeapBytes for $kind { fn heap_bytes(&self)->usize { 0 } })* }; }
scalars!(
    bool,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    f32,
    f64,
    ()
);
// Submission origins are fieldless; an exhaustive match requires revisiting
// their accounting if a future origin introduces retained data.
impl HeapBytes for PromptSubmissionSource {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Keyboard
            | Self::VoiceControl
            | Self::ScheduledInput
            | Self::QueuedPrompt
            | Self::InitialAgentPrompt
            | Self::ToolbarAction
            | Self::AskForUpdate
            | Self::TextTrigger
            | Self::ProgressResult => 0,
        }
    }
}
impl HeapBytes for String {
    fn heap_bytes(&self) -> usize {
        self.capacity()
    }
}
impl HeapBytes for PathBuf {
    fn heap_bytes(&self) -> usize {
        self.capacity()
    }
}
impl<T: HeapBytes> HeapBytes for Option<T> {
    fn heap_bytes(&self) -> usize {
        self.as_ref().map_or(0, HeapBytes::heap_bytes)
    }
}
impl<T: HeapBytes, E: HeapBytes> HeapBytes for Result<T, E> {
    fn heap_bytes(&self) -> usize {
        match self {
            Ok(v) => v.heap_bytes(),
            Err(e) => e.heap_bytes(),
        }
    }
}
impl<T: HeapBytes> HeapBytes for Box<T> {
    fn heap_bytes(&self) -> usize {
        size_of::<T>().saturating_add(HeapBytes::heap_bytes(self.as_ref()))
    }
}
impl<T: HeapBytes> HeapBytes for Vec<T> {
    fn heap_bytes(&self) -> usize {
        self.iter().fold(
            self.capacity().saturating_mul(size_of::<T>()),
            |sum, value| sum.saturating_add(HeapBytes::heap_bytes(value)),
        )
    }
}
impl<A: HeapBytes, B: HeapBytes> HeapBytes for (A, B) {
    fn heap_bytes(&self) -> usize {
        HeapBytes::heap_bytes(&self.0).saturating_add(HeapBytes::heap_bytes(&self.1))
    }
}
macro_rules! domain { ($($kind:ty),*)=>{$(impl HeapBytes for $kind { fn heap_bytes(&self)->usize { ilium_core::AllocationSize::heap_bytes(self) } })*}; }
domain!(
    ilium_core::Node,
    NodeId,
    NodeActivityRevision,
    Tree,
    PaneStatus,
    PaneProgress,
    ProgressTaskReport,
    AgentClass,
    AgentActivity
);

impl ServerEvent {
    /// Cooperative owned allocation cost, independent of bincode wire size.
    pub fn retained_bytes(&self) -> usize {
        size_of_val(self).saturating_add(HeapBytes::heap_bytes(self))
    }
}
impl HeapBytes for PaneDetectionEvidence {
    fn heap_bytes(&self) -> usize {
        let PaneDetectionEvidence {
            applied_status,
            identity,
            activity,
            goal,
        } = self;
        0usize
            .saturating_add(applied_status.heap_bytes())
            .saturating_add(identity.heap_bytes())
            .saturating_add(activity.heap_bytes())
            .saturating_add(goal.heap_bytes())
    }
}
impl HeapBytes for DetectionReason {
    fn heap_bytes(&self) -> usize {
        let DetectionReason {
            rule,
            observed,
            context,
        } = self;
        0usize
            .saturating_add(rule.heap_bytes())
            .saturating_add(observed.heap_bytes())
            .saturating_add(context.heap_bytes())
    }
}
impl HeapBytes for AgentDetectionSettings {
    fn heap_bytes(&self) -> usize {
        let AgentDetectionSettings {
            working_poll_seconds,
            idle_poll_seconds,
            custom_signatures,
        } = self;
        0usize
            .saturating_add(working_poll_seconds.heap_bytes())
            .saturating_add(idle_poll_seconds.heap_bytes())
            .saturating_add(custom_signatures.heap_bytes())
    }
}
impl HeapBytes for CustomAgentSignature {
    fn heap_bytes(&self) -> usize {
        let CustomAgentSignature {
            name_substring,
            class,
        } = self;
        0usize
            .saturating_add(name_substring.heap_bytes())
            .saturating_add(class.heap_bytes())
    }
}
impl HeapBytes for AgentDetectionSettingsError {
    fn heap_bytes(&self) -> usize {
        let AgentDetectionSettingsError { message } = self;
        0usize.saturating_add(message.heap_bytes())
    }
}
impl HeapBytes for ProgressMonitorRejection {
    fn heap_bytes(&self) -> usize {
        let ProgressMonitorRejection { code, message } = self;
        0usize
            .saturating_add(code.heap_bytes())
            .saturating_add(message.heap_bytes())
    }
}
impl HeapBytes for ProgressMonitorPreflight {
    fn heap_bytes(&self) -> usize {
        let ProgressMonitorPreflight {
            report,
            checked_at_unix_millis,
        } = self;
        0usize
            .saturating_add(report.heap_bytes())
            .saturating_add(checked_at_unix_millis.heap_bytes())
    }
}
impl HeapBytes for ProgressMonitorAccepted {
    fn heap_bytes(&self) -> usize {
        let ProgressMonitorAccepted {
            monitor_id,
            progress,
        } = self;
        0usize
            .saturating_add(monitor_id.heap_bytes())
            .saturating_add(progress.heap_bytes())
    }
}
impl HeapBytes for ProgressWaitOutcome {
    fn heap_bytes(&self) -> usize {
        let ProgressWaitOutcome {
            monitor_id,
            end: _,
            progress,
            composer_notice_suppressed,
        } = self;
        0usize
            .saturating_add(monitor_id.heap_bytes())
            .saturating_add(progress.heap_bytes())
            .saturating_add(composer_notice_suppressed.heap_bytes())
    }
}
impl HeapBytes for ProgressMonitorStatus {
    fn heap_bytes(&self) -> usize {
        let ProgressMonitorStatus {
            pane_id,
            progress_monitors,
        } = self;
        0usize
            .saturating_add(pane_id.heap_bytes())
            .saturating_add(progress_monitors.heap_bytes())
    }
}
impl HeapBytes for WorkspaceGitVersion {
    fn heap_bytes(&self) -> usize {
        let WorkspaceGitVersion {
            major,
            minor,
            patch,
        } = self;
        0usize
            .saturating_add(major.heap_bytes())
            .saturating_add(minor.heap_bytes())
            .saturating_add(patch.heap_bytes())
    }
}
impl HeapBytes for WorkspaceWorktreeFact {
    fn heap_bytes(&self) -> usize {
        let WorkspaceWorktreeFact {
            path,
            branch,
            created_by_ilium,
            is_dirty,
            occupied_pane_id,
        } = self;
        0usize
            .saturating_add(path.heap_bytes())
            .saturating_add(branch.heap_bytes())
            .saturating_add(created_by_ilium.heap_bytes())
            .saturating_add(is_dirty.heap_bytes())
            .saturating_add(occupied_pane_id.heap_bytes())
    }
}
impl HeapBytes for RepoFacts {
    fn heap_bytes(&self) -> usize {
        let RepoFacts {
            repo_common_dir,
            checkout_root,
            project_subpath,
            current_branch,
            default_base_ref,
            default_base_commit,
            local_branches,
            worktrees,
            source_dirty_count,
            main_dirty_count,
            has_gitmodules,
            git_version,
        } = self;
        0usize
            .saturating_add(repo_common_dir.heap_bytes())
            .saturating_add(checkout_root.heap_bytes())
            .saturating_add(project_subpath.heap_bytes())
            .saturating_add(current_branch.heap_bytes())
            .saturating_add(default_base_ref.heap_bytes())
            .saturating_add(default_base_commit.heap_bytes())
            .saturating_add(local_branches.heap_bytes())
            .saturating_add(worktrees.heap_bytes())
            .saturating_add(source_dirty_count.heap_bytes())
            .saturating_add(main_dirty_count.heap_bytes())
            .saturating_add(has_gitmodules.heap_bytes())
            .saturating_add(git_version.heap_bytes())
    }
}
impl HeapBytes for WorkspaceGitStatus {
    fn heap_bytes(&self) -> usize {
        let WorkspaceGitStatus {
            branch,
            detached,
            ahead,
            behind,
            staged,
            modified,
            untracked,
            conflicted,
            upstream,
            last_commit_subject,
            checked_at_unix_millis,
            full_checked_at_unix_millis,
            missing,
        } = self;
        0usize
            .saturating_add(branch.heap_bytes())
            .saturating_add(detached.heap_bytes())
            .saturating_add(ahead.heap_bytes())
            .saturating_add(behind.heap_bytes())
            .saturating_add(staged.heap_bytes())
            .saturating_add(modified.heap_bytes())
            .saturating_add(untracked.heap_bytes())
            .saturating_add(conflicted.heap_bytes())
            .saturating_add(upstream.heap_bytes())
            .saturating_add(last_commit_subject.heap_bytes())
            .saturating_add(checked_at_unix_millis.heap_bytes())
            .saturating_add(full_checked_at_unix_millis.heap_bytes())
            .saturating_add(missing.heap_bytes())
    }
}
impl HeapBytes for WorkspacePruneTarget {
    fn heap_bytes(&self) -> usize {
        let WorkspacePruneTarget {
            repo_common_dir,
            worktree_root,
            workspace_id,
            creation_branch,
            base_ref,
            base_commit,
            created_at_unix,
            metadata_directory,
            root_device,
            root_inode,
            metadata_device,
            metadata_inode,
            expected_head,
        } = self;
        0usize
            .saturating_add(repo_common_dir.heap_bytes())
            .saturating_add(worktree_root.heap_bytes())
            .saturating_add(workspace_id.heap_bytes())
            .saturating_add(creation_branch.heap_bytes())
            .saturating_add(base_ref.heap_bytes())
            .saturating_add(base_commit.heap_bytes())
            .saturating_add(created_at_unix.heap_bytes())
            .saturating_add(metadata_directory.heap_bytes())
            .saturating_add(root_device.heap_bytes())
            .saturating_add(root_inode.heap_bytes())
            .saturating_add(metadata_device.heap_bytes())
            .saturating_add(metadata_inode.heap_bytes())
            .saturating_add(expected_head.heap_bytes())
    }
}
impl HeapBytes for WorkspaceInventoryEntry {
    fn heap_bytes(&self) -> usize {
        let WorkspaceInventoryEntry {
            path,
            branch,
            head,
            is_main,
            is_locked,
            is_prunable,
            owner,
            target,
            occupied_pane_ids,
            protected_paths,
            safe_blockers,
            discard_blockers,
            merge_target,
        } = self;
        0usize
            .saturating_add(path.heap_bytes())
            .saturating_add(branch.heap_bytes())
            .saturating_add(head.heap_bytes())
            .saturating_add(is_main.heap_bytes())
            .saturating_add(is_locked.heap_bytes())
            .saturating_add(is_prunable.heap_bytes())
            .saturating_add(owner.heap_bytes())
            .saturating_add(target.heap_bytes())
            .saturating_add(occupied_pane_ids.heap_bytes())
            .saturating_add(protected_paths.heap_bytes())
            .saturating_add(safe_blockers.heap_bytes())
            .saturating_add(discard_blockers.heap_bytes())
            .saturating_add(merge_target.heap_bytes())
    }
}
impl HeapBytes for WorkspaceInventory {
    fn heap_bytes(&self) -> usize {
        let WorkspaceInventory {
            repo_common_dir,
            control_directory,
            total_worktrees,
            truncated,
            entries,
        } = self;
        0usize
            .saturating_add(repo_common_dir.heap_bytes())
            .saturating_add(control_directory.heap_bytes())
            .saturating_add(total_worktrees.heap_bytes())
            .saturating_add(truncated.heap_bytes())
            .saturating_add(entries.heap_bytes())
    }
}
impl HeapBytes for WorkspacePruneResult {
    fn heap_bytes(&self) -> usize {
        let WorkspacePruneResult {
            outcome,
            mutation_attempted,
            path_present,
            registration_present,
            metadata_present,
            branch_outcome,
            reasons,
        } = self;
        0usize
            .saturating_add(outcome.heap_bytes())
            .saturating_add(mutation_attempted.heap_bytes())
            .saturating_add(path_present.heap_bytes())
            .saturating_add(registration_present.heap_bytes())
            .saturating_add(metadata_present.heap_bytes())
            .saturating_add(branch_outcome.heap_bytes())
            .saturating_add(reasons.heap_bytes())
    }
}
impl HeapBytes for TextTriggerSettings {
    fn heap_bytes(&self) -> usize {
        let TextTriggerSettings { triggers } = self;
        0usize.saturating_add(triggers.heap_bytes())
    }
}
impl HeapBytes for TextTrigger {
    fn heap_bytes(&self) -> usize {
        let TextTrigger {
            id,
            enabled,
            regexp,
            message,
            target,
            sample_text,
            delay_seconds,
        } = self;
        0usize
            .saturating_add(delay_seconds.heap_bytes())
            .saturating_add(id.heap_bytes())
            .saturating_add(enabled.heap_bytes())
            .saturating_add(regexp.heap_bytes())
            .saturating_add(message.heap_bytes())
            .saturating_add(target.heap_bytes())
            .saturating_add(sample_text.heap_bytes())
    }
}
impl HeapBytes for VoiceTextRejection {
    fn heap_bytes(&self) -> usize {
        let VoiceTextRejection { code, message } = self;
        0usize
            .saturating_add(code.heap_bytes())
            .saturating_add(message.heap_bytes())
    }
}
impl HeapBytes for VoiceTextAccepted {
    fn heap_bytes(&self) -> usize {
        let VoiceTextAccepted {
            sentence_count,
            phase,
            started_voice,
        } = self;
        0usize
            .saturating_add(sentence_count.heap_bytes())
            .saturating_add(phase.heap_bytes())
            .saturating_add(started_voice.heap_bytes())
    }
}
impl HeapBytes for AgentDebugEntry {
    fn heap_bytes(&self) -> usize {
        let AgentDebugEntry {
            sequence,
            occurred_at_unix_millis,
            severity,
            source,
            kind,
            summary,
            fields,
            correlation_id,
            context,
            metadata,
        } = self;
        0usize
            .saturating_add(sequence.heap_bytes())
            .saturating_add(occurred_at_unix_millis.heap_bytes())
            .saturating_add(severity.heap_bytes())
            .saturating_add(source.heap_bytes())
            .saturating_add(kind.heap_bytes())
            .saturating_add(summary.heap_bytes())
            .saturating_add(fields.heap_bytes())
            .saturating_add(correlation_id.heap_bytes())
            .saturating_add(context.heap_bytes())
            .saturating_add(metadata.heap_bytes())
    }
}
impl HeapBytes for AgentDebugField {
    fn heap_bytes(&self) -> usize {
        let AgentDebugField {
            label,
            value,
            presentation,
        } = self;
        0usize
            .saturating_add(label.heap_bytes())
            .saturating_add(value.heap_bytes())
            .saturating_add(presentation.heap_bytes())
    }
}
impl HeapBytes for AgentDebugContext {
    fn heap_bytes(&self) -> usize {
        let AgentDebugContext {
            class,
            activity,
            process_id,
            session_id,
            title_generation,
        } = self;
        0usize
            .saturating_add(class.heap_bytes())
            .saturating_add(activity.heap_bytes())
            .saturating_add(process_id.heap_bytes())
            .saturating_add(session_id.heap_bytes())
            .saturating_add(title_generation.heap_bytes())
    }
}
impl HeapBytes for AgentDebugEventMetadata {
    fn heap_bytes(&self) -> usize {
        let AgentDebugEventMetadata { pane_resize_cause } = self;
        0usize.saturating_add(pane_resize_cause.heap_bytes())
    }
}
impl HeapBytes for ServerEvent {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::TreeSnapshot(value0) => 0usize.saturating_add(value0.heap_bytes()),
            Self::ScreenUpdate {
                pane_id,
                first_sequence,
                sequence,
                bytes,
            } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(first_sequence.heap_bytes())
                .saturating_add(sequence.heap_bytes())
                .saturating_add(bytes.heap_bytes()),
            Self::PaneStatusChanged { pane_id, status } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(status.heap_bytes()),
            Self::Error { message } | Self::PaneResizeRejected { message, .. } => {
                0usize.saturating_add(message.heap_bytes())
            }
            Self::PaneSessionIdResolved {
                pane_id,
                session_id,
                process_id,
                title_generation,
                transcript_path,
            } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(session_id.heap_bytes())
                .saturating_add(process_id.heap_bytes())
                .saturating_add(title_generation.heap_bytes())
                .saturating_add(transcript_path.heap_bytes()),
            Self::PaneSessionIdCleared {
                pane_id,
                title_generation,
            } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(title_generation.heap_bytes()),
            Self::PaneEditorPathResolved { pane_id, path } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(path.heap_bytes()),
            Self::TerminalReplay {
                pane_id,
                through_sequence,
                bytes,
                is_complete,
            } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(through_sequence.heap_bytes())
                .saturating_add(bytes.heap_bytes())
                .saturating_add(is_complete.heap_bytes()),
            Self::PaneSessionTitleCleared {
                pane_id,
                title_generation,
            } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(title_generation.heap_bytes()),
            Self::SessionRecoveryAvailable { pane_count } => {
                0usize.saturating_add(pane_count.heap_bytes())
            }
            Self::InitialStateSyncComplete => 0usize,
            Self::PanePromptSubmitted { pane_id, source } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(source.heap_bytes()),
            Self::DebugLoggingChanged { enabled } => 0usize.saturating_add(enabled.heap_bytes()),
            Self::AgentDebugMenuChanged { enabled } => 0usize.saturating_add(enabled.heap_bytes()),
            Self::PaneDebugLogSnapshot {
                pane_id,
                through_sequence,
                retained_from_sequence,
                dropped_entry_count,
                entries,
            } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(through_sequence.heap_bytes())
                .saturating_add(retained_from_sequence.heap_bytes())
                .saturating_add(dropped_entry_count.heap_bytes())
                .saturating_add(entries.heap_bytes()),
            Self::PaneDebugEntryAppended { pane_id, entry } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(entry.heap_bytes()),
            Self::NodeActivityChanged {
                node_id,
                activity_revision,
            } => 0usize
                .saturating_add(node_id.heap_bytes())
                .saturating_add(activity_revision.heap_bytes()),
            Self::NodeFocusCheckpointChanged {
                node_id,
                activity_revision,
            } => 0usize
                .saturating_add(node_id.heap_bytes())
                .saturating_add(activity_revision.heap_bytes()),
            Self::ProjectRestructureApplied {
                project_id,
                checkpoint_activity_revisions,
            } => 0usize
                .saturating_add(project_id.heap_bytes())
                .saturating_add(checkpoint_activity_revisions.heap_bytes()),
            Self::ProjectRestructureRejected {
                project_id,
                message,
            } => 0usize
                .saturating_add(project_id.heap_bytes())
                .saturating_add(message.heap_bytes()),
            Self::PaneLastPromptChanged {
                pane_id,
                last_prompt,
            } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(last_prompt.heap_bytes()),
            Self::PaneProgressChanged {
                pane_id,
                progress_monitors,
            } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(progress_monitors.heap_bytes()),
            Self::ProgressMonitorEnabledChanged { enabled } => {
                0usize.saturating_add(enabled.heap_bytes())
            }
            Self::TextTriggersChanged { settings } => 0usize.saturating_add(settings.heap_bytes()),
            Self::ProgressMonitorCheckCompleted {
                request_id,
                pane_id,
                result,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(result.heap_bytes()),
            Self::ProgressMonitorSetCompleted {
                request_id,
                pane_id,
                result,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(result.heap_bytes()),
            Self::ProgressMonitorStatusReported {
                request_id,
                pane_id,
                result,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(result.heap_bytes()),
            Self::ProgressMonitorCleared {
                request_id,
                pane_id,
                result,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(result.heap_bytes()),
            Self::ProgressWaitCompleted {
                request_id,
                pane_id,
                result,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(result.heap_bytes()),
            Self::VoiceTextOffered {
                request_id,
                sentences,
                start_voice,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(sentences.heap_bytes())
                .saturating_add(start_voice.heap_bytes()),
            Self::VoiceTextResult { request_id, result } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(result.heap_bytes()),
            Self::RepoFactsReported {
                request_id,
                project,
                result,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(project.heap_bytes())
                .saturating_add(result.heap_bytes()),
            Self::WorkspaceCreateProgress { request_id, stage } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(stage.heap_bytes()),
            Self::WorkspaceCreated {
                request_id,
                pane_id,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(pane_id.heap_bytes()),
            Self::WorkspaceCreateFailed { request_id, error } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(error.heap_bytes()),
            Self::PaneGitStatusChanged { pane_id, status } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(status.heap_bytes()),
            Self::WorkspaceRemoved {
                request_id,
                pane_id,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(pane_id.heap_bytes()),
            Self::WorkspaceRemovalBlocked {
                request_id,
                pane_id,
                reasons,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(reasons.heap_bytes()),
            Self::WorkspaceInventoryReported {
                request_id,
                project,
                result,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(project.heap_bytes())
                .saturating_add(result.heap_bytes()),
            Self::WorkspacePruneCompleted {
                request_id,
                project,
                target,
                result,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(project.heap_bytes())
                .saturating_add(target.heap_bytes())
                .saturating_add(result.heap_bytes()),
            Self::WorkspaceCloseOfferReported {
                request_id,
                pane_id,
                can_offer,
            } => 0usize
                .saturating_add(request_id.heap_bytes())
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(can_offer.heap_bytes()),
            Self::PaneDetectionEvidenceChanged { pane_id, evidence } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(evidence.heap_bytes()),
            Self::PaneStateSnapshot {
                tree,
                detection_evidence,
            } => 0usize
                .saturating_add(tree.heap_bytes())
                .saturating_add(detection_evidence.heap_bytes()),
            Self::PaneDetectedStateChanged {
                pane_id,
                status,
                evidence,
            } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(status.heap_bytes())
                .saturating_add(evidence.heap_bytes()),
            Self::AgentDetectionSettingsChanged { result, .. } => {
                0usize.saturating_add(result.heap_bytes())
            }
            Self::PaneProcessTerminated { pane_id, result } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(result.heap_bytes()),
            Self::PaneFrozen { pane_id, result } => 0usize
                .saturating_add(pane_id.heap_bytes())
                .saturating_add(result.heap_bytes()),
            Self::AntigravityStatuslineCompleted { result, .. } => {
                0usize.saturating_add(result.heap_bytes())
            }
            Self::SoundPreviewCompleted { succeeded } => {
                0usize.saturating_add(succeeded.heap_bytes())
            }
            Self::PaneNodeChanged(node) => 0usize.saturating_add(node.heap_bytes()),
        }
    }
}
impl HeapBytes for ProgressMonitorRejectionCode {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Disabled => 0usize,
            Self::InvalidRequest => 0usize,
            Self::InvalidProbeReport => 0usize,
            Self::ProbeSpawnFailed => 0usize,
            Self::ProbeTimedOut => 0usize,
            Self::ProbeExitedNonZero => 0usize,
            Self::ProbeOutputTooLarge => 0usize,
            Self::ProbeIoFailed => 0usize,
            Self::PaneNotFound => 0usize,
            Self::StaleMonitor => 0usize,
            Self::TooManyMonitors => 0usize,
        }
    }
}
impl HeapBytes for WorkspaceCreateStage {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::CreatingWorktree => 0usize,
            Self::Preparing => 0usize,
            Self::Starting => 0usize,
            Self::RunningSetup => 0usize,
        }
    }
}
impl HeapBytes for WorkspaceInventoryOwner {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Owned => 0usize,
            Self::Foreign => 0usize,
            Self::Unavailable { reason } => 0usize.saturating_add(reason.heap_bytes()),
        }
    }
}
impl HeapBytes for WorkspacePruneOutcome {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Removed => 0usize,
            Self::Blocked => 0usize,
            Self::Uncertain => 0usize,
        }
    }
}
impl HeapBytes for WorkspacePruneBranchOutcome {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Kept => 0usize,
            Self::Deleted => 0usize,
            Self::Absent => 0usize,
            Self::Unknown => 0usize,
        }
    }
}
impl HeapBytes for TextTriggerTarget {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Agents => 0usize,
            Self::Terminals => 0usize,
            Self::Both => 0usize,
        }
    }
}
impl HeapBytes for VoiceTextRejectionCode {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::InvalidRequest => 0usize,
            Self::NoVoiceClient => 0usize,
            Self::VoiceOff => 0usize,
            Self::VoiceUnavailable => 0usize,
            Self::ClientUnresponsive => 0usize,
        }
    }
}
impl HeapBytes for VoiceTextPhase {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Connecting => 0usize,
            Self::Listening => 0usize,
            Self::Recording => 0usize,
            Self::Thinking => 0usize,
            Self::Speaking => 0usize,
        }
    }
}
impl HeapBytes for AgentDebugSeverity {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Trace => 0usize,
            Self::Information => 0usize,
            Self::Success => 0usize,
            Self::Warning => 0usize,
            Self::Error => 0usize,
        }
    }
}
impl HeapBytes for AgentDebugSource {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Server => 0usize,
            Self::Client => 0usize,
            Self::Detector => 0usize,
            Self::SessionDiscovery => 0usize,
            Self::Pty => 0usize,
            Self::Persistence => 0usize,
            Self::Inference => 0usize,
            Self::Voice => 0usize,
        }
    }
}
impl HeapBytes for AgentDebugEventKind {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::RecordingEnabled => 0usize,
            Self::RecordingDisabled => 0usize,
            Self::PaneCreated => 0usize,
            Self::PaneRestored => 0usize,
            Self::PaneFocused => 0usize,
            Self::PaneResized => 0usize,
            Self::PaneClosed => 0usize,
            Self::PtyInputWritten => 0usize,
            Self::PtyOutputClosed => 0usize,
            Self::AgentDetected => 0usize,
            Self::AgentLost => 0usize,
            Self::AgentChanged => 0usize,
            Self::DetectionCycle => 0usize,
            Self::ActivityChanged => 0usize,
            Self::GoalDetected => 0usize,
            Self::GoalCleared => 0usize,
            Self::ExecutionStarted => 0usize,
            Self::ExecutionFinished => 0usize,
            Self::ApprovalRequested => 0usize,
            Self::BackgroundWait => 0usize,
            Self::SessionDiscovery => 0usize,
            Self::SessionResolved => 0usize,
            Self::SessionCleared => 0usize,
            Self::ConversationCleared => 0usize,
            Self::PromptSubmitted => 0usize,
            Self::ScheduledInputCreated => 0usize,
            Self::ScheduledInputReplaced => 0usize,
            Self::ScheduledInputDelivered => 0usize,
            Self::ScheduledInputCleared => 0usize,
            Self::PromptQueued => 0usize,
            Self::QueuedPromptDelivered => 0usize,
            Self::PromptQueueCleared => 0usize,
            Self::TriggerEvaluated => 0usize,
            Self::TriggerActionStarted => 0usize,
            Self::TriggerActionFinished => 0usize,
            Self::TitleInferenceRequested => 0usize,
            Self::TitleInferenceSucceeded => 0usize,
            Self::TitleInferenceFailed => 0usize,
            Self::TitleInferenceDiscarded => 0usize,
            Self::TitleApplied => 0usize,
            Self::VoiceAction => 0usize,
            Self::PersistenceSaved => 0usize,
            Self::IpcRequest => 0usize,
            Self::Error => 0usize,
            Self::RetentionNotice => 0usize,
            Self::Custom(value0) => 0usize.saturating_add(value0.heap_bytes()),
        }
    }
}
impl HeapBytes for AgentDebugFieldPresentation {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Plain => 0usize,
            Self::Code => 0usize,
            Self::Multiline => 0usize,
            Self::Sensitive => 0usize,
        }
    }
}
impl HeapBytes for PaneResizeCause {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::HostTerminal => 0usize,
            Self::TreePanelAnimation => 0usize,
            Self::RightPanelPresentation => 0usize,
            Self::UserInterfaceSettings => 0usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resize_refusal_counts_unused_message_capacity() {
        let event = ServerEvent::PaneResizeRejected {
            pane_id: NodeId(9),
            rows: 24,
            cols: 80,
            message: String::with_capacity(8192),
        };
        assert_eq!(event.retained_bytes(), size_of::<ServerEvent>() + 8192);
    }
    #[test]
    fn trigger_sample_text_and_unused_vector_capacity_remain_accounted() {
        let event = ServerEvent::TextTriggersChanged {
            settings: TextTriggerSettings {
                triggers: vec![TextTrigger {
                    sample_text: String::with_capacity(32768),
                    ..TextTrigger::default()
                }],
            },
        };
        assert!(
            event.retained_bytes() >= size_of::<ServerEvent>() + size_of::<TextTrigger>() + 32768
        );
        let bytes = Vec::<u8>::with_capacity(65536);
        let event = ServerEvent::TerminalReplay {
            pane_id: NodeId(9),
            through_sequence: 2,
            bytes,
            is_complete: true,
        };
        assert_eq!(event.retained_bytes(), size_of::<ServerEvent>() + 65536);
    }
    #[test]
    fn retained_debug_identity_and_empty_entry_storage_are_counted() {
        let entries = Vec::<AgentDebugEntry>::with_capacity(128);
        let event = ServerEvent::PaneDebugLogSnapshot {
            pane_id: NodeId(5),
            through_sequence: 0,
            retained_from_sequence: 0,
            dropped_entry_count: 0,
            entries,
        };
        assert_eq!(
            event.retained_bytes(),
            size_of::<ServerEvent>() + 128 * size_of::<AgentDebugEntry>()
        );
        let event = ServerEvent::VoiceTextResult {
            request_id: 1,
            result: Err(VoiceTextRejection {
                code: VoiceTextRejectionCode::VoiceOff,
                message: String::with_capacity(8192),
            }),
        };
        assert_eq!(event.retained_bytes(), size_of::<ServerEvent>() + 8192);
    }

    #[test]
    fn antigravity_statusline_completion_counts_unused_result_capacity() {
        let event = ServerEvent::AntigravityStatuslineCompleted {
            generation: 19,
            result: Err(String::with_capacity(8192)),
        };

        assert_eq!(event.retained_bytes(), size_of::<ServerEvent>() + 8192);
    }
}
