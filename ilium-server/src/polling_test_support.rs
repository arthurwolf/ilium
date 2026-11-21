//! Synthetic, task-owned acquisition fixtures; no real agent CLI or transcript.
use crate::pane::{PaneResource, TerminalOrigin, TerminalPaneRuntime};
use crate::state::ServerState;
use ilium_core::NodeId;
use ilium_platform::process_control;
use std::sync::Arc;

pub(crate) struct EvidenceFixture {
    pub(crate) state: Arc<ServerState>,
    pub(crate) transcript: std::fs::File,
    pub(crate) pane: NodeId,
    pub(crate) directory: tempfile::TempDir,
}

pub(crate) async fn evidence_fixture() -> EvidenceFixture {
    let root = tempfile::tempdir().unwrap();
    let transcript_path = root.path().join("synthetic-transcript.jsonl");
    let transcript = std::fs::File::create(&transcript_path).unwrap();
    let state = Arc::new(crate::state::test_server_state(&root));
    assert!(state
        .execution
        .set(crate::execution::ServerExecution::start().unwrap())
        .is_ok());
    let session = ilium_pty::PtySession::spawn(
        ilium_pty::PtyCommand::new("/bin/sh", root.path(), 24, 80)
            .arg("-c")
            .arg("exec cat"),
    )
    .unwrap();
    let mut runtime = TerminalPaneRuntime::new(
        session,
        TerminalOrigin::PlainShell,
        None,
        std::time::Duration::from_secs(1),
    );
    // Synthetic binding owned solely by this fixture. The real process and
    // open-file proof still run; no real agent CLI or user transcript is used.
    runtime.agent_process_key = Some(ilium_core::AgentProcessKey {
        class: ilium_core::AgentClass::Codex,
        process_id: std::process::id(),
        started_at_unix_seconds: 0,
    });
    runtime.session_agent_class = Some(ilium_core::AgentClass::Codex);
    runtime.session_process_id = Some(std::process::id());
    runtime.session_process_started_at_unix_seconds = Some(0);
    runtime.session_process_identity = Some(Arc::new(
        process_control::capture_pty_process(std::process::id()).unwrap(),
    ));
    runtime.session_id = Some("synthetic-polling-session".into());
    runtime.session_transcript_path = Some(transcript_path.clone());
    runtime.agent_generation = 7;
    let pane = NodeId(17);
    state
        .panes
        .write()
        .await
        .insert(pane, PaneResource::Terminal(Box::new(runtime)));
    EvidenceFixture {
        state,
        transcript,
        pane,
        directory: root,
    }
}
