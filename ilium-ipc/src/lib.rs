//! Shared IPC contract between `ilium-client` and `ilium-server`.
//!
//! This crate owns exactly two things: the message shapes exchanged over
//! the client/server Unix domain socket ([`protocol`]) and the
//! length-prefixed bincode framing used to put those messages on any async
//! byte stream ([`framing`]). It has no knowledge of Unix domain sockets
//! specifically -- that belongs to `ilium-server`, which owns the actual
//! `UnixListener`/`UnixStream` -- so it depends on `tokio` only for the
//! `AsyncRead`/`AsyncWrite` trait bounds its framing functions are generic
//! over, keeping it reusable over any stream type (including an
//! in-memory buffer in tests).

mod error;
mod framing;
mod protocol;

pub use error::IpcError;
pub use framing::{read_frame, write_frame, MAX_FRAME_LEN};
pub use protocol::{
    ClientRequest, MouseButton, MouseEventKind, MouseModifiers, NewPaneKind, ServerEvent,
};

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::PathBuf;

    use ilium_core::{
        AgentActivity, AgentClass, BoardStorage, NodeId, PaneStatus, SplitOrientation, Tree,
        TreeMoveDirection, ROOT_ID,
    };
    use ilium_sound::{SoundSettings, SoundSourceKind};

    use super::*;

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
            },
            ClientRequest::NewPane {
                parent_group: NodeId(1),
                kind: NewPaneKind::Command("claude".to_string()),
            },
            ClientRequest::NewPane {
                parent_group: NodeId(1),
                kind: NewPaneKind::Editor(PathBuf::from("/tmp/notes.md")),
            },
            ClientRequest::NewPane {
                parent_group: NodeId(1),
                kind: NewPaneKind::CommandWithInitialInput {
                    command_line: "codex".to_string(),
                    initial_input: "/goal inspect this line".to_string(),
                },
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
            },
            ClientRequest::RenameNode {
                node_id: NodeId(2),
                title: "renamed with short form".to_string(),
                short_title: Some("Renamed".to_string()),
            },
            ClientRequest::ResizePane {
                pane_id: NodeId(2),
                rows: 40,
                cols: 120,
            },
            ClientRequest::KeyInput {
                pane_id: NodeId(2),
                bytes: vec![0x1b, b'[', b'A'],
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
            },
            ClientRequest::SetAutomaticPaneTitle {
                pane_id: NodeId(2),
                title: "Fix Auth Bug In Login Flow".to_string(),
                short_title: Some("Fix Auth".to_string()),
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
            ClientRequest::SchedulePaneInput {
                pane_id: NodeId(2),
                delay_seconds: 3661,
                text: "cargo test".to_string(),
                send_enter: true,
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

    /// Every `ServerEvent` variant, one instance each -- same exhaustive
    /// intent as `sample_client_requests`.
    fn sample_server_events() -> Vec<ServerEvent> {
        vec![
            ServerEvent::TreeSnapshot(sample_tree()),
            ServerEvent::ScreenUpdate {
                pane_id: NodeId(2),
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
                status: PaneStatus::Agent(AgentClass::Claude, AgentActivity::Working),
            },
            ServerEvent::PaneStatusChanged {
                pane_id: NodeId(2),
                status: PaneStatus::Agent(
                    AgentClass::Other("opencode".to_string()),
                    AgentActivity::Idle,
                ),
            },
            ServerEvent::Error {
                message: "pane 2 failed to spawn: No such file or directory".to_string(),
            },
            ServerEvent::PaneSessionIdResolved {
                pane_id: NodeId(2),
                session_id: "95fd0645-3331-408b-a7e5-36e6007bfb78".to_string(),
            },
            ServerEvent::PaneEditorPathResolved {
                pane_id: NodeId(2),
                path: Some(PathBuf::from("/tmp/notes.md")),
            },
            ServerEvent::PaneEditorPathResolved {
                pane_id: NodeId(2),
                path: None,
            },
        ]
    }

    #[tokio::test]
    async fn every_client_request_variant_round_trips() {
        for request in sample_client_requests() {
            let mut buffer = Vec::new();
            write_frame(&mut buffer, &request).await.unwrap();

            let mut cursor = Cursor::new(buffer);
            let decoded: ClientRequest = read_frame(&mut cursor).await.unwrap();
            assert_eq!(decoded, request, "round trip changed the decoded value");
        }
    }

    #[tokio::test]
    async fn every_server_event_variant_round_trips() {
        for event in sample_server_events() {
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
