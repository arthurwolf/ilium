//! Shared IPC contract between `ilium-client` and `ilium-server`.
//!
//! This crate owns the message shapes exchanged between the two processes
//! ([`protocol`]), the length-prefixed bincode framing used to put those
//! messages on any async byte stream ([`framing`]), and the pane
//! environment-variable names both sides address a pane by ([`pane_env`]).
//! It has no knowledge of where those bytes actually travel -- the Unix
//! domain socket / Windows named pipe, and the `UnixListener`/`UnixStream`
//! behind it, belong to `ilium-transport` -- so it depends on `tokio` only
//! for the `AsyncRead`/`AsyncWrite` trait bounds its framing functions are
//! generic over, keeping it reusable over any stream type (including an
//! in-memory buffer in tests).

mod allocation;
mod bounded_decode;
mod error;
mod framing;
mod protocol;
mod startup;
mod terminal_bytes;
mod text_trigger;
mod voice_text;

pub use bounded_decode::{deserialize_allocation_checked, AllocationDecodeError, BoundedMessage};
pub use error::IpcError;
pub use framing::{
    decode_bounded_frame, decode_frame, encode_frame, encoded_capacity_bound, read_frame,
    write_frame, EncodedFrame, FrameReader, FrameWriter, MAX_FRAME_LEN,
};
pub use ilium_agent_debug::{
    AgentDebugContext, AgentDebugEntry, AgentDebugEventDraft, AgentDebugEventKind,
    AgentDebugEventMetadata, AgentDebugField, AgentDebugFieldPresentation, AgentDebugSeverity,
    AgentDebugSource, PaneDebugLog, PaneResizeCause,
};
pub use protocol::{
    AgentDetectionSettings, AgentDetectionSettingsError, ClientRequest, CustomAgentSignature,
    DetectionReason, MouseButton, MouseEventKind, MouseModifiers, NewPaneKind,
    NewPaneWorkingDirectory, PaneDetectionEvidence, PaneTitleObservation, ProgressMonitorAccepted,
    ProgressMonitorPreflight, ProgressMonitorRejection, ProgressMonitorRejectionCode,
    ProgressMonitorStatus, PromptSubmissionSource, RepoFacts, ServerEvent, WorkspaceClosePolicy,
    WorkspaceCreateSpec, WorkspaceCreateStage, WorkspaceDisposition, WorkspaceGitStatus,
    WorkspaceGitVersion, WorkspaceInventory, WorkspaceInventoryEntry, WorkspaceInventoryOwner,
    WorkspacePruneBranchOutcome, WorkspacePruneBranchPolicy, WorkspacePruneMode,
    WorkspacePruneOutcome, WorkspacePruneResult, WorkspacePruneTarget, WorkspaceWorktreeFact,
};
pub use startup::{
    clear_startup_progress, publish_startup_progress, read_startup_progress, startup_progress_path,
    StartupProgress,
};
pub use text_trigger::{
    TextTrigger, TextTriggerSettings, TextTriggerTarget, DEFAULT_TEXT_TRIGGER_DELAY_SECONDS,
    MAX_TEXT_TRIGGER_DELAY_SECONDS,
};
pub use voice_text::{
    normalize_voice_sentences, VoiceTextAccepted, VoiceTextPhase, VoiceTextRejection,
    VoiceTextRejectionCode, VoiceTextResult, MAX_VOICE_TEXT_SENTENCES,
    MAX_VOICE_TEXT_SENTENCE_CHARS,
};

/// Environment variables `ilium-server` injects into every spawned terminal
/// pane (see `ilium-server`'s `pane::spawn_terminal_session`), so a process
/// running inside it -- e.g. the `ilium progress set`/`ilium progress clear`
/// CLI subcommands -- can address this exact pane on this exact server
/// without already knowing the session's runtime-directory layout. Defined
/// here (rather than in `ilium-server`, which the `ilium` CLI binary
/// deliberately never links in as a library -- see that binary's own module
/// doc) so both the injecting side and the reading side share one contract.
pub mod pane_env {
    /// This pane's `NodeId` (its `.0`, formatted as a plain decimal string).
    pub const PANE_ID: &str = "ILIUM_PANE_ID";
    /// The session name this pane belongs to, as passed to `Connection::connect`.
    pub const SESSION_NAME: &str = "ILIUM_SESSION_NAME";
    /// This session's Unix domain socket path.
    pub const SESSION_SOCKET: &str = "ILIUM_SESSION_SOCKET";
    /// Canonical root of this pane's Git worktree, present only for a
    /// workspace-backed terminal pane.
    pub const WORKTREE: &str = "ILIUM_WORKTREE";
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::PathBuf;

    use ilium_core::{
        AgentActivity, AgentClass, BoardStorage, BuiltinAgentProvider, NodeId, PaneStatus,
        PromptQueueDelivery, SplitOrientation, Tree, TreeMoveDirection, ROOT_ID,
    };
    use ilium_sound::{SoundSettings, SoundSourceKind};
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    enum LegacyPaneStatus {
        PlainShell,
        Agent(AgentClass, AgentActivity),
        AgentWithGoal(AgentClass, AgentActivity, ilium_core::GoalState),
        Editor { dirty: bool },
        Board,
    }

    #[test]
    fn canonical_agent_status_keeps_the_legacy_bincode_layout() {
        let cases = [
            (
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None),
                LegacyPaneStatus::Agent(AgentClass::Claude, AgentActivity::Working),
            ),
            (
                PaneStatus::from_activity(
                    AgentClass::Codex,
                    AgentActivity::Done,
                    Some(ilium_core::GoalState::Active),
                ),
                LegacyPaneStatus::AgentWithGoal(
                    AgentClass::Codex,
                    AgentActivity::Done,
                    ilium_core::GoalState::Active,
                ),
            ),
            (PaneStatus::PlainShell, LegacyPaneStatus::PlainShell),
            (
                PaneStatus::Editor { dirty: true },
                LegacyPaneStatus::Editor { dirty: true },
            ),
            (PaneStatus::Board, LegacyPaneStatus::Board),
        ];

        for (current, legacy) in cases {
            assert_eq!(
                bincode::serialize(&current).expect("serialize canonical status"),
                bincode::serialize(&legacy).expect("serialize legacy status"),
                "canonical status changed the established wire bytes for {current:?}"
            );
            assert_eq!(
                bincode::deserialize::<PaneStatus>(
                    &bincode::serialize(&legacy).expect("serialize legacy status")
                )
                .expect("deserialize legacy status"),
                current,
            );
        }

        let state = ilium_core::AgentState {
            class: AgentClass::Codex,
            turn: ilium_core::AgentTurn::Settling,
            goal: Some(ilium_core::GoalState::Paused),
            completion_unread: false,
        };
        assert_eq!(
            bincode::deserialize::<ilium_core::AgentState>(
                &bincode::serialize(&state).expect("serialize canonical agent state")
            )
            .expect("deserialize canonical agent state"),
            state
        );
    }

    /// Every `ClientRequest` variant, one instance each, so a new variant
    /// added later without a matching round-trip case here is an obvious
    /// gap in the test list (not enforced by the compiler, but keeping
    /// this list exhaustive against the enum definition next to it makes
    /// the omission easy to spot in review).
    fn sample_client_requests() -> Vec<ClientRequest> {
        vec![
            ClientRequest::Attach {
                session: "main".to_string(),
            },
            ClientRequest::NewPane {
                parent_group: NodeId(1),
                kind: NewPaneKind::PlainShell,
                working_directory: NewPaneWorkingDirectory::ProjectRoot,
            },
            ClientRequest::NewPane {
                parent_group: NodeId(1),
                kind: NewPaneKind::Command("claude".to_string()),
                working_directory: NewPaneWorkingDirectory::FocusedTerminal,
            },
            ClientRequest::NewPane {
                parent_group: NodeId(1),
                kind: NewPaneKind::Editor(PathBuf::from("/tmp/notes.md")),
                working_directory: NewPaneWorkingDirectory::LastUsed,
            },
            ClientRequest::NewPane {
                parent_group: NodeId(1),
                kind: NewPaneKind::PlainShell,
                working_directory: NewPaneWorkingDirectory::WorkspacePane(NodeId(2)),
            },
            ClientRequest::NewPane {
                parent_group: NodeId(1),
                kind: NewPaneKind::CommandWithInitialInput {
                    command_line: "codex".to_string(),
                    initial_input: "/goal inspect this line".to_string(),
                },
                working_directory: NewPaneWorkingDirectory::ProjectRoot,
            },
            ClientRequest::ClosePane { pane_id: NodeId(2) },
            ClientRequest::MoveNode {
                node_id: NodeId(2),
                direction: TreeMoveDirection::Up,
            },
            ClientRequest::RenameNode {
                node_id: NodeId(2),
                title: "renamed".to_string(),
                short_title: None,
                inferred_icon: None,
            },
            ClientRequest::RenameNode {
                node_id: NodeId(2),
                title: "renamed with short form".to_string(),
                short_title: Some("Renamed".to_string()),
                inferred_icon: Some("✏️".to_string()),
            },
            ClientRequest::ResizePane {
                pane_id: NodeId(2),
                rows: 40,
                cols: 120,
                cause: PaneResizeCause::HostTerminal,
            },
            ClientRequest::KeyInput {
                pane_id: NodeId(2),
                bytes: vec![0x1b, b'[', b'A'],
                submission: None,
            },
            ClientRequest::UserKeyInput {
                pane_id: NodeId(2),
                bytes: b"direct user input\r".to_vec(),
                submission: Some(PromptSubmissionSource::Keyboard),
                prompt_epoch: Some("epoch-1".to_string()),
            },
            ClientRequest::ReportAgentPromptFromTranscript {
                pane_id: NodeId(2),
                expected_session_id: "session-1".to_string(),
                prompt_epoch: "epoch-1".to_string(),
                last_prompt: "exact\ntrailing  ".to_string(),
            },
            ClientRequest::MouseInput {
                pane_id: NodeId(2),
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: 5,
                modifiers: MouseModifiers {
                    shift: true,
                    alt: false,
                    control: false,
                },
            },
            ClientRequest::Detach,
            ClientRequest::KillSession,
            ClientRequest::NewGroup {
                parent_group: ROOT_ID,
                name: "backend".to_string(),
            },
            ClientRequest::ReparentNode {
                node_id: NodeId(2),
                new_parent: NodeId(3),
                index: Some(1),
            },
            ClientRequest::ReparentNode {
                node_id: NodeId(2),
                new_parent: ROOT_ID,
                index: None,
            },
            ClientRequest::SetAutomaticPaneTitle {
                pane_id: NodeId(2),
                title: "git status".to_string(),
                short_title: None,
                inferred_icon: None,
            },
            ClientRequest::SetAutomaticPaneTitle {
                pane_id: NodeId(2),
                title: "Fix Auth Bug In Login Flow".to_string(),
                short_title: Some("Fix Auth".to_string()),
                inferred_icon: Some("🔐".to_string()),
            },
            ClientRequest::SetPaneFocus {
                pane_id: NodeId(2),
                focused: true,
            },
            ClientRequest::SetPaneFocus {
                pane_id: NodeId(2),
                focused: false,
            },
            ClientRequest::RestartServer,
            ClientRequest::NewFolder {
                parent_group: NodeId(1),
                path: PathBuf::from("/tmp/project"),
            },
            ClientRequest::NewProject {
                path: PathBuf::from("/tmp/second-project"),
            },
            ClientRequest::ChangeProjectFolder {
                project_id: NodeId(1),
                path: PathBuf::from("/tmp/relocated-project"),
            },
            ClientRequest::NewBoard {
                parent_group: NodeId(1),
                name: "Work".to_string(),
                storage: BoardStorage::MarkdownFile {
                    path: PathBuf::from("/tmp/work.md"),
                },
            },
            ClientRequest::CreateSplitView {
                parent_group: NodeId(1),
                name: "Vertical split".to_string(),
                orientation: SplitOrientation::Vertical,
                pane_ids: vec![NodeId(2), NodeId(3)],
            },
            ClientRequest::UpdateSoundSettings {
                settings: SoundSettings::default(),
            },
            ClientRequest::PreviewSound {
                source: SoundSourceKind::SoundFile,
                file: Some(PathBuf::from("/usr/share/sounds/example.oga")),
            },
            ClientRequest::PreviewSoundSettings {
                settings: SoundSettings {
                    source: SoundSourceKind::Generated,
                    ..SoundSettings::default()
                },
            },
            ClientRequest::SchedulePaneInput {
                pane_id: NodeId(2),
                delay_seconds: 3661,
                text: "cargo test".to_string(),
                send_enter: true,
            },
            ClientRequest::SubmitTerminalText {
                pane_id: NodeId(2),
                text: "Explain this codebase".to_string(),
                source: PromptSubmissionSource::TextTrigger,
            },
            ClientRequest::SubmitTerminalText {
                pane_id: NodeId(2),
                text: "Task completed".to_string(),
                source: PromptSubmissionSource::ProgressResult,
            },
            ClientRequest::EnqueuePrompt {
                pane_id: NodeId(2),
                text: "cargo test".to_string(),
                delivery: PromptQueueDelivery::Once,
            },
            ClientRequest::EnqueuePrompt {
                pane_id: NodeId(2),
                text: "cargo test".to_string(),
                delivery: PromptQueueDelivery::Times { remaining_runs: 3 },
            },
            ClientRequest::EnqueuePrompt {
                pane_id: NodeId(2),
                text: "cargo test".to_string(),
                delivery: PromptQueueDelivery::Forever,
            },
            ClientRequest::ClearPromptQueue { pane_id: NodeId(2) },
            ClientRequest::SetSessionPaneTitle {
                pane_id: NodeId(2),
                expected_session_id: "95fd0645-3331-408b-a7e5-36e6007bfb78".to_string(),
                expected_title_generation: 0,
                expected_presentation_revision: 0,
                expected_process_id: Some(123),
                title: "Fix Auth Bug In Login Flow".to_string(),
                short_title: Some("Fix Auth".to_string()),
                inferred_icon: Some("🔐".to_string()),
                title_source: ilium_core::PaneTitleSource::Automatic,
            },
            ClientRequest::SetSessionPaneTitle {
                pane_id: NodeId(2),
                expected_session_id: "95fd0645-3331-408b-a7e5-36e6007bfb78".to_string(),
                expected_title_generation: 0,
                expected_presentation_revision: 0,
                expected_process_id: Some(123),
                title: "My Agent Name".to_string(),
                short_title: None,
                inferred_icon: None,
                title_source: ilium_core::PaneTitleSource::UserSpecified,
            },
            ClientRequest::ApplyRestructurePlan {
                plan: ilium_core::RestructurePlan {
                    children: vec![
                        ilium_core::RestructureNode::Group {
                            title: "Auth refactor".to_string(),
                            short_title: Some("Auth".to_string()),
                            icon: Some("🔐".to_string()),
                            children: vec![
                                ilium_core::RestructureNode::Pane {
                                    id: NodeId(2),
                                    title: "Backend agent".to_string(),
                                    short_title: None,
                                    icon: Some("🔧".to_string()),
                                },
                                ilium_core::RestructureNode::ExistingSplitView {
                                    id: NodeId(5),
                                    children: vec![ilium_core::RestructureNode::Pane {
                                        id: NodeId(3),
                                        title: "Frontend shell".to_string(),
                                        short_title: None,
                                        icon: Some("🖥️".to_string()),
                                    }],
                                },
                            ],
                        },
                        ilium_core::RestructureNode::Folder {
                            id: NodeId(4),
                            title: "Project root".to_string(),
                            short_title: None,
                            icon: Some("📁".to_string()),
                        },
                    ],
                },
                title_observations: Vec::new(),
            },
            ClientRequest::RevertLastRestructure,
            ClientRequest::ApplyProjectRestructurePlan {
                project_id: NodeId(1),
                title_observations: Vec::new(),
                plan: ilium_core::RestructurePlan {
                    children: vec![ilium_core::RestructureNode::Pane {
                        id: NodeId(2),
                        title: "Scoped shell".to_string(),
                        short_title: None,
                        icon: Some("🖥️".to_string()),
                    }],
                },
                inference_activity_revisions: vec![ilium_core::NodeActivityRevision {
                    node_id: NodeId(2),
                    activity_revision: 7,
                }],
            },
            ClientRequest::RevertProjectRestructure {
                project_id: NodeId(1),
            },
            ClientRequest::ResolveSessionRecovery { restore: true },
            ClientRequest::UpdateDebugLogging { enabled: true },
            ClientRequest::UpdateAgentDebugMenu { enabled: true },
            ClientRequest::GetPaneDebugLog {
                pane_id: NodeId(2),
                after_sequence: Some(7),
            },
            ClientRequest::RecordAgentDebugEvent {
                pane_id: NodeId(2),
                expected_session_id: Some("95fd0645-3331-408b-a7e5-36e6007bfb78".to_string()),
                expected_title_generation: 3,
                event: AgentDebugEventDraft::information(
                    AgentDebugEventKind::TitleInferenceRequested,
                    "Automatic title requested",
                ),
            },
            ClientRequest::SetNodeBookmarked {
                node_id: NodeId(2),
                is_bookmarked: true,
            },
            ClientRequest::RecordNodeActivity { node_id: NodeId(2) },
            ClientRequest::AttachInteractive {
                session: "main".to_string(),
            },
            ClientRequest::SetVisiblePanes {
                pane_ids: vec![NodeId(2), NodeId(3)],
            },
            ClientRequest::DiscardTerminalDelivery {
                pane_ids: vec![NodeId(2)],
            },
            ClientRequest::SetNodeExpanded {
                node_id: NodeId(2),
                expanded: true,
            },
            ClientRequest::SetNodeLockedClosed {
                node_id: NodeId(2),
                locked_closed: true,
            },
            ClientRequest::ReportLastPromptFromTranscript {
                pane_id: NodeId(2),
                expected_session_id: "95fd0645-3331-408b-a7e5-36e6007bfb78".to_string(),
                last_prompt: "fix the login bug".to_string(),
            },
            ClientRequest::CheckPaneProgressMonitor {
                request_id: 40,
                pane_id: NodeId(2),
                command: "/tmp/render_progress.sh".to_string(),
            },
            ClientRequest::SetPaneProgressMonitor {
                request_id: 41,
                pane_id: NodeId(2),
                command: "/tmp/render_progress.sh".to_string(),
                interval_seconds: 1,
            },
            ClientRequest::GetPaneProgressMonitorStatus {
                request_id: 42,
                pane_id: NodeId(2),
            },
            ClientRequest::ClearPaneProgressMonitor {
                request_id: 43,
                pane_id: NodeId(2),
                expected_monitor_id: Some(7),
            },
            ClientRequest::UpdateProgressMonitorEnabled { enabled: true },
            ClientRequest::RegisterVoiceTextReceiver,
            ClientRequest::SubmitVoiceText {
                request_id: 50,
                sentences: vec!["open the settings".to_string(), "close it".to_string()],
                start_voice: true,
            },
            ClientRequest::AnswerVoiceText {
                request_id: 50,
                result: Ok(VoiceTextAccepted {
                    sentence_count: 2,
                    phase: VoiceTextPhase::Listening,
                    started_voice: false,
                }),
            },
            ClientRequest::AnswerVoiceText {
                request_id: 51,
                result: Err(VoiceTextRejection::new(
                    VoiceTextRejectionCode::VoiceOff,
                    "voice control is off",
                )),
            },
            ClientRequest::QueryRepoFacts {
                request_id: 60,
                project: NodeId(1),
            },
            ClientRequest::CreateAgentInWorkspace {
                request_id: 61,
                parent_group: NodeId(1),
                provider: BuiltinAgentProvider::Codex,
                spec: WorkspaceCreateSpec::New {
                    branch: "agent/feature".to_string(),
                    base_ref: "main".to_string(),
                    path: PathBuf::from("/tmp/project.worktrees/agent-feature"),
                },
                initial_input: Some("Implement feature".to_string()),
            },
            ClientRequest::CreateAgentInWorkspace {
                request_id: 62,
                parent_group: NodeId(1),
                provider: BuiltinAgentProvider::Claude,
                spec: WorkspaceCreateSpec::Existing {
                    path: PathBuf::from("/tmp/project.worktrees/existing"),
                },
                initial_input: None,
            },
            ClientRequest::CreateAgentInWorkspace {
                request_id: 64,
                parent_group: NodeId(1),
                provider: BuiltinAgentProvider::Codex,
                spec: WorkspaceCreateSpec::NewAtDefaultPath {
                    branch: "agent/default-path".to_string(),
                    base_ref: None,
                },
                initial_input: None,
            },
            ClientRequest::CreateAgentInWorkspace {
                request_id: 65,
                parent_group: NodeId(1),
                provider: BuiltinAgentProvider::Codex,
                spec: WorkspaceCreateSpec::NewWithSetup {
                    branch: "agent/setup".to_string(),
                    base_ref: "main".to_string(),
                    path: PathBuf::from("/tmp/project.worktrees/agent-setup"),
                    setup_command: "printf ready".to_string(),
                },
                initial_input: None,
            },
            ClientRequest::CreateAgentInWorkspace {
                request_id: 66,
                parent_group: NodeId(1),
                provider: BuiltinAgentProvider::Codex,
                spec: WorkspaceCreateSpec::NewAtDefaultPathWithSetup {
                    branch: "agent/configured-default".to_string(),
                    base_ref: Some("main".to_string()),
                    setup_command: "printf ready".to_string(),
                },
                initial_input: None,
            },
            ClientRequest::QueryWorkspaceInventory {
                request_id: 80,
                project: NodeId(1),
            },
            ClientRequest::PruneWorkspace {
                request_id: 81,
                project: NodeId(1),
                target: crate::WorkspacePruneTarget {
                    repo_common_dir: PathBuf::from("/tmp/project/.git"),
                    worktree_root: PathBuf::from("/tmp/project.worktrees/retained"),
                    workspace_id: "00000000-0000-4000-8000-000000000001".into(),
                    creation_branch: "agent/retained".into(),
                    base_ref: "refs/heads/main".into(),
                    base_commit: "a".repeat(40),
                    created_at_unix: 1_700_000_000,
                    metadata_directory: PathBuf::from("/tmp/project/.git/worktrees/retained"),
                    root_device: 1,
                    root_inode: 2,
                    metadata_device: 1,
                    metadata_inode: 3,
                    expected_head: "a".repeat(40),
                },
                mode: crate::WorkspacePruneMode::Safe,
                branch_policy: crate::WorkspacePruneBranchPolicy::Keep,
            },
            ClientRequest::PruneWorkspace {
                request_id: 82,
                project: NodeId(1),
                target: crate::WorkspacePruneTarget {
                    repo_common_dir: PathBuf::from("/tmp/project/.git"),
                    worktree_root: PathBuf::from("/tmp/project.worktrees/retained"),
                    workspace_id: "00000000-0000-4000-8000-000000000001".into(),
                    creation_branch: "agent/retained".into(),
                    base_ref: "refs/heads/main".into(),
                    base_commit: "a".repeat(40),
                    created_at_unix: 1_700_000_000,
                    metadata_directory: PathBuf::from("/tmp/project/.git/worktrees/retained"),
                    root_device: 1,
                    root_inode: 2,
                    metadata_device: 1,
                    metadata_inode: 3,
                    expected_head: "a".repeat(40),
                },
                mode: crate::WorkspacePruneMode::DiscardFiles {
                    confirmed_path: PathBuf::from("/tmp/project.worktrees/retained"),
                },
                branch_policy: crate::WorkspacePruneBranchPolicy::DeleteIfSafe,
            },
            ClientRequest::RefreshPaneGitStatus { pane_id: NodeId(2) },
            ClientRequest::RemoveWorkspace {
                request_id: 63,
                pane_id: NodeId(2),
                force_path: None,
                remove_branch: false,
            },
            ClientRequest::ClosePaneWithWorkspaceDisposition {
                request_id: 64,
                pane_id: NodeId(2),
                disposition: WorkspaceDisposition::RemoveWorktreeAndBranch,
            },
            ClientRequest::CreateAgentInWorkspace {
                request_id: 84,
                parent_group: NodeId(1),
                provider: BuiltinAgentProvider::Codex,
                spec: WorkspaceCreateSpec::NewWithOptions {
                    branch: "agent/options".into(),
                    base_ref: "main".into(),
                    path: PathBuf::from("/tmp/project.worktrees/options"),
                    setup_command: String::new(),
                    close_policy: WorkspaceClosePolicy::OfferRemovalWhenSafe,
                },
                initial_input: None,
            },
            ClientRequest::CreateAgentInWorkspace {
                request_id: 85,
                parent_group: NodeId(1),
                provider: BuiltinAgentProvider::Codex,
                spec: WorkspaceCreateSpec::ExistingWithOptions {
                    path: PathBuf::from("/tmp/project.worktrees/options"),
                    close_policy: WorkspaceClosePolicy::Keep,
                },
                initial_input: None,
            },
            ClientRequest::QueryWorkspaceCloseOffer {
                request_id: 86,
                pane_id: NodeId(2),
            },
        ]
    }

    fn sample_tree() -> Tree {
        let mut tree = Tree::new();
        let group = tree
            .add_group(ROOT_ID, "work")
            .expect("root accepts group children");
        tree.add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .expect("group accepts pane children");
        tree
    }

    fn sample_progress() -> ilium_core::PaneProgress {
        ilium_core::PaneProgress::new(
            7,
            ilium_core::ProgressTaskReport::new(
                "render-42".to_string(),
                ilium_core::ProgressTaskStatus::Running,
                42.5,
                "frame 1200/3000, ETA 8m".to_string(),
                String::new(),
                None,
            )
            .expect("valid sample report"),
            1_700_000_000_000,
        )
        .expect("valid sample progress")
    }

    /// Every `ServerEvent` variant, one instance each -- same exhaustive
    /// intent as `sample_client_requests`.
    fn sample_server_events() -> Vec<ServerEvent> {
        vec![
            ServerEvent::TreeSnapshot(sample_tree()),
            ServerEvent::PaneStateSnapshot {
                tree: sample_tree(),
                detection_evidence: vec![(
                    NodeId(2),
                    PaneDetectionEvidence {
                        applied_status: PaneStatus::PlainShell,
                        identity: Some(DetectionReason {
                            rule: "no registered agent process".into(),
                            observed: None,
                            context: "pane shell only".into(),
                        }),
                        activity: None,
                        goal: None,
                    },
                )],
            },
            ServerEvent::ScreenUpdate {
                pane_id: NodeId(2),
                first_sequence: 7,
                sequence: 7,
                bytes: b"hello from the pty".to_vec(),
            },
            ServerEvent::TerminalReplay {
                pane_id: NodeId(2),
                through_sequence: 7,
                bytes: b"hello from the pty".to_vec(),
                is_complete: true,
            },
            ServerEvent::PaneStatusChanged {
                pane_id: NodeId(2),
                status: PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None),
            },
            ServerEvent::PaneStatusChanged {
                pane_id: NodeId(2),
                status: PaneStatus::from_activity(
                    AgentClass::Other("opencode".to_string()),
                    AgentActivity::Idle,
                    None,
                ),
            },
            ServerEvent::PaneStatusChanged {
                pane_id: NodeId(2),
                status: PaneStatus::from_activity(
                    AgentClass::Codex,
                    AgentActivity::Working,
                    Some(ilium_core::GoalState::Active),
                ),
            },
            ServerEvent::Error {
                message: "pane 2 failed to spawn: No such file or directory".to_string(),
            },
            ServerEvent::PaneResizeRejected {
                pane_id: NodeId(2),
                rows: 24,
                cols: 80,
                message: "failed to resize pane: parser busy".to_string(),
            },
            ServerEvent::PaneSessionIdResolved {
                pane_id: NodeId(2),
                session_id: "95fd0645-3331-408b-a7e5-36e6007bfb78".to_string(),
                process_id: Some(12345),
                title_generation: 0,
            },
            ServerEvent::PaneSessionIdCleared {
                pane_id: NodeId(2),
                title_generation: 1,
            },
            ServerEvent::PaneSessionTitleCleared {
                pane_id: NodeId(2),
                title_generation: 2,
            },
            ServerEvent::PaneEditorPathResolved {
                pane_id: NodeId(2),
                path: Some(PathBuf::from("/tmp/notes.md")),
            },
            ServerEvent::PaneEditorPathResolved {
                pane_id: NodeId(2),
                path: None,
            },
            ServerEvent::SessionRecoveryAvailable { pane_count: 3 },
            ServerEvent::InitialStateSyncComplete,
            ServerEvent::PanePromptSubmitted {
                pane_id: NodeId(2),
                source: PromptSubmissionSource::Keyboard,
            },
            ServerEvent::DebugLoggingChanged { enabled: true },
            ServerEvent::AgentDebugMenuChanged { enabled: true },
            ServerEvent::PaneDebugLogSnapshot {
                pane_id: NodeId(2),
                through_sequence: 1,
                retained_from_sequence: 1,
                dropped_entry_count: 0,
                entries: vec![sample_debug_entry()],
            },
            ServerEvent::PaneDebugEntryAppended {
                pane_id: NodeId(2),
                entry: sample_debug_entry(),
            },
            ServerEvent::NodeActivityChanged {
                node_id: NodeId(2),
                activity_revision: 7,
            },
            ServerEvent::NodeFocusCheckpointChanged {
                node_id: NodeId(2),
                activity_revision: 7,
            },
            ServerEvent::ProjectRestructureApplied {
                project_id: NodeId(1),
                checkpoint_activity_revisions: vec![ilium_core::NodeActivityRevision {
                    node_id: NodeId(2),
                    activity_revision: 7,
                }],
            },
            ServerEvent::ProjectRestructureRejected {
                project_id: NodeId(1),
                message: "project changed during inference".to_string(),
            },
            ServerEvent::PaneLastPromptChanged {
                pane_id: NodeId(2),
                last_prompt: Some("fix the login bug".to_string()),
            },
            ServerEvent::PaneLastPromptChanged {
                pane_id: NodeId(2),
                last_prompt: None,
            },
            ServerEvent::PaneProgressChanged {
                pane_id: NodeId(2),
                progress: Some(sample_progress()),
            },
            ServerEvent::PaneProgressChanged {
                pane_id: NodeId(2),
                progress: None,
            },
            ServerEvent::ProgressMonitorEnabledChanged { enabled: false },
            ServerEvent::ProgressMonitorCheckCompleted {
                request_id: 40,
                pane_id: NodeId(2),
                result: Ok(ProgressMonitorPreflight {
                    report: sample_progress().report,
                    checked_at_unix_millis: 1_700_000_000_000,
                }),
            },
            ServerEvent::ProgressMonitorSetCompleted {
                request_id: 41,
                pane_id: NodeId(2),
                result: Ok(ProgressMonitorAccepted {
                    monitor_id: 7,
                    progress: sample_progress(),
                }),
            },
            ServerEvent::ProgressMonitorStatusReported {
                request_id: 42,
                pane_id: NodeId(2),
                result: Ok(ProgressMonitorStatus {
                    pane_id: NodeId(2),
                    progress: Some(sample_progress()),
                }),
            },
            ServerEvent::ProgressMonitorCleared {
                request_id: 43,
                pane_id: NodeId(2),
                result: Ok(Some(7)),
            },
            ServerEvent::VoiceTextOffered {
                request_id: 50,
                sentences: vec!["open the settings".to_string()],
                start_voice: false,
            },
            ServerEvent::VoiceTextResult {
                request_id: 50,
                result: Ok(VoiceTextAccepted {
                    sentence_count: 1,
                    phase: VoiceTextPhase::Connecting,
                    started_voice: true,
                }),
            },
            ServerEvent::VoiceTextResult {
                request_id: 51,
                result: Err(VoiceTextRejection::new(
                    VoiceTextRejectionCode::NoVoiceClient,
                    "no voice client is attached",
                )),
            },
            ServerEvent::WorkspaceInventoryReported {
                request_id: 80,
                project: NodeId(1),
                result: Ok(crate::WorkspaceInventory {
                    repo_common_dir: PathBuf::from("/tmp/project/.git"),
                    control_directory: PathBuf::from("/tmp/project"),
                    total_worktrees: 1,
                    truncated: false,
                    entries: vec![crate::WorkspaceInventoryEntry {
                        path: PathBuf::from("/tmp/project.worktrees/retained"),
                        branch: Some("agent/retained".into()),
                        head: Some("a".repeat(40)),
                        is_main: false,
                        is_locked: false,
                        is_prunable: false,
                        owner: crate::WorkspaceInventoryOwner::Owned,
                        target: Some(crate::WorkspacePruneTarget {
                            repo_common_dir: PathBuf::from("/tmp/project/.git"),
                            worktree_root: PathBuf::from("/tmp/project.worktrees/retained"),
                            workspace_id: "00000000-0000-4000-8000-000000000001".into(),
                            creation_branch: "agent/retained".into(),
                            base_ref: "refs/heads/main".into(),
                            base_commit: "a".repeat(40),
                            created_at_unix: 1_700_000_000,
                            metadata_directory: PathBuf::from(
                                "/tmp/project/.git/worktrees/retained",
                            ),
                            root_device: 1,
                            root_inode: 2,
                            metadata_device: 1,
                            metadata_inode: 3,
                            expected_head: "a".repeat(40),
                        }),
                        occupied_pane_ids: Vec::new(),
                        protected_paths: Vec::new(),
                        safe_blockers: Vec::new(),
                        discard_blockers: Vec::new(),
                        merge_target: Some("refs/heads/main".into()),
                    }],
                }),
            },
            ServerEvent::WorkspaceInventoryReported {
                request_id: 83,
                project: NodeId(1),
                result: Err("ownership inspection unavailable".into()),
            },
            ServerEvent::WorkspacePruneCompleted {
                request_id: 81,
                project: NodeId(1),
                target: crate::WorkspacePruneTarget {
                    repo_common_dir: PathBuf::from("/tmp/project/.git"),
                    worktree_root: PathBuf::from("/tmp/project.worktrees/retained"),
                    workspace_id: "00000000-0000-4000-8000-000000000001".into(),
                    creation_branch: "agent/retained".into(),
                    base_ref: "refs/heads/main".into(),
                    base_commit: "a".repeat(40),
                    created_at_unix: 1_700_000_000,
                    metadata_directory: PathBuf::from("/tmp/project/.git/worktrees/retained"),
                    root_device: 1,
                    root_inode: 2,
                    metadata_device: 1,
                    metadata_inode: 3,
                    expected_head: "a".repeat(40),
                },
                result: crate::WorkspacePruneResult {
                    outcome: crate::WorkspacePruneOutcome::Uncertain,
                    mutation_attempted: true,
                    path_present: Some(false),
                    registration_present: None,
                    metadata_present: Some(true),
                    branch_outcome: crate::WorkspacePruneBranchOutcome::Kept,
                    reasons: vec!["inspect partial effects; do not retry automatically".into()],
                },
            },
            ServerEvent::WorkspaceCloseOfferReported {
                request_id: 86,
                pane_id: NodeId(2),
                can_offer: true,
            },
            ServerEvent::PaneDetectionEvidenceChanged {
                pane_id: NodeId(2),
                evidence: PaneDetectionEvidence {
                    applied_status: PaneStatus::from_activity(
                        AgentClass::Codex,
                        AgentActivity::Idle,
                        Some(ilium_core::GoalState::Paused),
                    ),
                    identity: Some(DetectionReason {
                        rule: "process name contains codex".into(),
                        observed: Some("codex".into()),
                        context: "PID 42 below pane shell".into(),
                    }),
                    activity: None,
                    goal: Some(DetectionReason {
                        rule: "rightmost Codex footer goal segment".into(),
                        observed: Some("Goal paused (/goal resume)".into()),
                        context: "same process".into(),
                    }),
                },
            },
            ServerEvent::PaneDetectedStateChanged {
                pane_id: NodeId(2),
                status: PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Working, None),
                evidence: PaneDetectionEvidence {
                    applied_status: PaneStatus::from_activity(
                        AgentClass::Codex,
                        AgentActivity::Working,
                        None,
                    ),
                    identity: Some(DetectionReason {
                        rule: "process signature codex".into(),
                        observed: Some("codex".into()),
                        context: "same live process".into(),
                    }),
                    activity: Some(DetectionReason {
                        rule: "Codex live status".into(),
                        observed: Some("Working for 2s".into()),
                        context: "live turn".into(),
                    }),
                    goal: None,
                },
            },
            ServerEvent::RepoFactsReported {
                request_id: 60,
                project: NodeId(1),
                result: Ok(RepoFacts {
                    repo_common_dir: PathBuf::from("/tmp/project/.git"),
                    checkout_root: PathBuf::from("/tmp/project"),
                    project_subpath: PathBuf::from("api"),
                    current_branch: Some("main".to_string()),
                    default_base_ref: "main".to_string(),
                    default_base_commit: "a1b2c3".to_string(),
                    local_branches: vec!["main".to_string()],
                    worktrees: vec![WorkspaceWorktreeFact {
                        path: PathBuf::from("/tmp/project.worktrees/agent-feature"),
                        branch: Some("agent/feature".to_string()),
                        created_by_ilium: true,
                        is_dirty: false,
                        occupied_pane_id: Some(NodeId(2)),
                    }],
                    source_dirty_count: 1,
                    main_dirty_count: 1,
                    has_gitmodules: true,
                    git_version: WorkspaceGitVersion {
                        major: 2,
                        minor: 46,
                        patch: 0,
                    },
                }),
            },
            ServerEvent::RepoFactsReported {
                request_id: 61,
                project: NodeId(1),
                result: Err("not a Git repository".to_string()),
            },
            ServerEvent::WorkspaceCreateProgress {
                request_id: 61,
                stage: WorkspaceCreateStage::CreatingWorktree,
            },
            ServerEvent::WorkspaceCreateProgress {
                request_id: 65,
                stage: WorkspaceCreateStage::RunningSetup,
            },
            ServerEvent::WorkspaceCreated {
                request_id: 61,
                pane_id: NodeId(2),
            },
            ServerEvent::WorkspaceCreateFailed {
                request_id: 62,
                error: "branch already exists".to_string(),
            },
            ServerEvent::PaneGitStatusChanged {
                pane_id: NodeId(2),
                status: WorkspaceGitStatus {
                    branch: Some("agent/feature".to_string()),
                    detached: false,
                    ahead: 1,
                    behind: 0,
                    staged: 2,
                    modified: 1,
                    untracked: 3,
                    conflicted: 0,
                    upstream: Some("origin/main".to_string()),
                    last_commit_subject: Some("Add feature".to_string()),
                    checked_at_unix_millis: 1_700_000_000_000,
                    full_checked_at_unix_millis: Some(1_700_000_000_000),
                    missing: false,
                },
            },
            ServerEvent::WorkspaceRemoved {
                request_id: 63,
                pane_id: NodeId(2),
            },
            ServerEvent::WorkspaceRemovalBlocked {
                request_id: 64,
                pane_id: NodeId(2),
                reasons: vec!["worktree has uncommitted changes".to_string()],
            },
        ]
    }

    fn sample_debug_entry() -> AgentDebugEntry {
        let mut log = PaneDebugLog::default();
        log.append(
            1_700_000_000_000,
            AgentDebugSource::Inference,
            AgentDebugContext::default(),
            AgentDebugEventDraft::information(
                AgentDebugEventKind::TitleInferenceSucceeded,
                "Title inference completed",
            )
            .with_fields(vec![AgentDebugField::plain("provider", "Kilo Gateway")]),
        )
        .expect("non-detection sample must append")
    }

    #[tokio::test]
    async fn every_client_request_variant_round_trips() {
        for request in sample_client_requests() {
            let bounded =
                decode_bounded_frame::<ClientRequest>(&encode_frame(&request).unwrap()).unwrap();
            assert_eq!(bounded, request, "bounded decode changed request variant");
            let mut buffer = Vec::new();
            write_frame(&mut buffer, &request).await.unwrap();

            let mut cursor = Cursor::new(buffer);
            let decoded: ClientRequest = read_frame(&mut cursor).await.unwrap();
            assert_eq!(decoded, request, "round trip changed the decoded value");
        }
    }

    #[test]
    fn request_diagnostics_keep_payloads_out_and_classify_input_frequency() {
        let major = ClientRequest::UpdateDebugLogging { enabled: true };
        assert_eq!(major.diagnostic_name(), "update_debug_logging");
        assert!(!major.is_high_frequency_diagnostic());

        let ordinary_key = ClientRequest::KeyInput {
            pane_id: NodeId(2),
            bytes: b"secret command".to_vec(),
            submission: None,
        };
        assert_eq!(ordinary_key.diagnostic_name(), "key_input");
        assert!(ordinary_key.is_high_frequency_diagnostic());

        let submitted_key = ClientRequest::KeyInput {
            pane_id: NodeId(2),
            bytes: b"cargo test\r".to_vec(),
            submission: Some(PromptSubmissionSource::Keyboard),
        };
        assert!(!submitted_key.is_high_frequency_diagnostic());
        let direct_user_input = ClientRequest::UserKeyInput {
            pane_id: NodeId(2),
            bytes: b"user text".to_vec(),
            submission: None,
            prompt_epoch: None,
        };
        assert_eq!(direct_user_input.diagnostic_name(), "user_key_input");
        assert!(direct_user_input.is_high_frequency_diagnostic());
        let submitted_user_input = ClientRequest::UserKeyInput {
            pane_id: NodeId(2),
            bytes: b"\r".to_vec(),
            submission: Some(PromptSubmissionSource::Keyboard),
            prompt_epoch: Some("epoch-2".to_string()),
        };
        assert!(!submitted_user_input.is_high_frequency_diagnostic());
    }

    /// The voice-text messages form one contiguous block of variants at the
    /// end of each enum. bincode's fixed-width encoding puts the variant
    /// index in the first four bytes.
    #[test]
    fn voice_text_variants_are_contiguous() {
        fn variant_index<T: serde::Serialize>(value: &T) -> u32 {
            let bytes = bincode::serialize(value).expect("serializable");
            u32::from_le_bytes(bytes[..4].try_into().expect("four-byte variant index"))
        }
        let register = variant_index(&ClientRequest::RegisterVoiceTextReceiver);
        let submit = variant_index(&ClientRequest::SubmitVoiceText {
            request_id: 1,
            sentences: Vec::new(),
            start_voice: false,
        });
        let answer = variant_index(&ClientRequest::AnswerVoiceText {
            request_id: 1,
            result: Err(VoiceTextRejection::new(
                VoiceTextRejectionCode::VoiceOff,
                "",
            )),
        });
        assert_eq!([submit, answer], [register + 1, register + 2]);

        let offered = variant_index(&ServerEvent::VoiceTextOffered {
            request_id: 1,
            sentences: Vec::new(),
            start_voice: false,
        });
        let result = variant_index(&ServerEvent::VoiceTextResult {
            request_id: 1,
            result: Err(VoiceTextRejection::new(
                VoiceTextRejectionCode::VoiceOff,
                "",
            )),
        });
        assert_eq!(result, offered + 1);
    }

    #[test]
    fn workspace_variants_preserve_close_pane_wire_layout() {
        fn variant_index<T: serde::Serialize>(value: &T) -> u32 {
            let bytes = bincode::serialize(value).expect("serializable");
            u32::from_le_bytes(bytes[..4].try_into().expect("four-byte variant index"))
        }

        assert_eq!(
            bincode::serialize(&ClientRequest::ClosePane { pane_id: NodeId(2) })
                .expect("serializable"),
            [
                2_u32.to_le_bytes().as_slice(),
                2_u64.to_le_bytes().as_slice()
            ]
            .concat(),
        );

        let last_existing_request = variant_index(&ClientRequest::AnswerVoiceText {
            request_id: 1,
            result: Err(VoiceTextRejection::new(
                VoiceTextRejectionCode::VoiceOff,
                "",
            )),
        });
        let first_workspace_request = variant_index(&ClientRequest::QueryRepoFacts {
            request_id: 2,
            project: NodeId(1),
        });
        assert_eq!(first_workspace_request, last_existing_request + 1);

        let last_existing_event = variant_index(&ServerEvent::VoiceTextResult {
            request_id: 1,
            result: Err(VoiceTextRejection::new(
                VoiceTextRejectionCode::VoiceOff,
                "",
            )),
        });
        let first_workspace_event = variant_index(&ServerEvent::RepoFactsReported {
            request_id: 2,
            project: NodeId(1),
            result: Err(String::new()),
        });
        assert_eq!(first_workspace_event, last_existing_event + 1);

        let old_request_tail = variant_index(&ClientRequest::ClosePaneWithWorkspaceDisposition {
            request_id: 70,
            pane_id: NodeId(2),
            disposition: crate::WorkspaceDisposition::Keep,
        });
        let inventory_request = variant_index(&ClientRequest::QueryWorkspaceInventory {
            request_id: 71,
            project: NodeId(1),
        });
        let prune_request = sample_client_requests()
            .into_iter()
            .find(|request| matches!(request, ClientRequest::PruneWorkspace { .. }))
            .expect("prune request sample");
        assert_eq!(inventory_request, old_request_tail + 1);
        assert_eq!(variant_index(&prune_request), inventory_request + 1);
        assert_eq!(
            variant_index(&ClientRequest::QueryWorkspaceCloseOffer {
                request_id: 72,
                pane_id: NodeId(2)
            }),
            variant_index(&prune_request) + 1
        );

        let old_event_tail = variant_index(&ServerEvent::WorkspaceRemovalBlocked {
            request_id: 70,
            pane_id: NodeId(2),
            reasons: Vec::new(),
        });
        let inventory_event = variant_index(&ServerEvent::WorkspaceInventoryReported {
            request_id: 71,
            project: NodeId(1),
            result: Err(String::new()),
        });
        let prune_event = sample_server_events()
            .into_iter()
            .find(|event| matches!(event, ServerEvent::WorkspacePruneCompleted { .. }))
            .expect("prune event sample");
        assert_eq!(inventory_event, old_event_tail + 1);
        assert_eq!(variant_index(&prune_event), inventory_event + 1);
        assert_eq!(
            variant_index(&ServerEvent::WorkspaceCloseOfferReported {
                request_id: 72,
                pane_id: NodeId(2),
                can_offer: false
            }),
            variant_index(&prune_event) + 1
        );
    }

    #[tokio::test]
    async fn every_server_event_variant_round_trips() {
        for event in sample_server_events() {
            let bounded =
                decode_bounded_frame::<ServerEvent>(&encode_frame(&event).unwrap()).unwrap();
            assert_eq!(bounded, event, "bounded decode changed event variant");
            let mut buffer = Vec::new();
            write_frame(&mut buffer, &event).await.unwrap();

            let mut cursor = Cursor::new(buffer);
            let decoded: ServerEvent = read_frame(&mut cursor).await.unwrap();
            assert_eq!(decoded, event, "round trip changed the decoded value");
        }
    }

    #[tokio::test]
    async fn multiple_frames_back_to_back_read_in_order() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &ClientRequest::Detach)
            .await
            .unwrap();
        write_frame(&mut buffer, &ClientRequest::KillSession)
            .await
            .unwrap();

        let mut cursor = Cursor::new(buffer);
        let first: ClientRequest = read_frame(&mut cursor).await.unwrap();
        let second: ClientRequest = read_frame(&mut cursor).await.unwrap();
        assert_eq!(first, ClientRequest::Detach);
        assert_eq!(second, ClientRequest::KillSession);
    }

    #[tokio::test]
    async fn a_truncated_frame_returns_an_error_not_a_panic() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &ClientRequest::KillSession)
            .await
            .unwrap();
        // Chop off the last few bytes to simulate a connection dying
        // mid-frame -- the length header still promises the original size.
        buffer.truncate(buffer.len() - 3);

        let mut cursor = Cursor::new(buffer);
        let result: Result<ClientRequest, IpcError> = read_frame(&mut cursor).await;
        assert!(
            matches!(result, Err(IpcError::TruncatedFrame { .. })),
            "expected TruncatedFrame, got {result:?}"
        );
    }

    #[tokio::test]
    async fn a_bad_shape_frame_errors_instead_of_misparsing_as_the_wrong_variant() {
        // A well-formed, fully-delivered frame whose payload was encoded
        // as a `ServerEvent` but is read back as a `ClientRequest` -- the
        // two enums don't share a bincode-compatible shape, so this must
        // fail rather than silently decode as some unrelated variant.
        let mut buffer = Vec::new();
        write_frame(
            &mut buffer,
            &ServerEvent::Error {
                message: "not a client request".to_string(),
            },
        )
        .await
        .unwrap();

        let mut cursor = Cursor::new(buffer);
        let result: Result<ClientRequest, IpcError> = read_frame(&mut cursor).await;
        assert!(
            result.is_err(),
            "decoding a ServerEvent frame as a ClientRequest should fail, got {result:?}"
        );
    }
}
