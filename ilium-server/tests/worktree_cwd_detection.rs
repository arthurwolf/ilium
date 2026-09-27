//! A pane started from a focused terminal's subdirectory keeps that exact cwd
//! for live agent-session discovery and in the durable session tree.

use std::time::Duration;

use ilium_core::{NodeKind, Tree, ROOT_ID};
use ilium_ipc::{write_frame, ClientRequest, NewPaneKind, NewPaneWorkingDirectory, ServerEvent};
use ilium_server::config::DetectionConfig;
use ilium_test_fixtures::{install, FixtureBehavior};

mod common;
use common::{expect_event, wait_until, TestServer};

#[cfg(not(windows))]
#[tokio::test]
async fn focused_subdirectory_agent_resolves_session_and_persists_its_cwd() {
    let fake_binary_dir = tempfile::tempdir().unwrap();
    let fake_codex = install(
        fake_binary_dir.path(),
        "codex",
        &FixtureBehavior::HoldArgument { argument_index: 3 },
    );
    let mut server = TestServer::start_with_detection_config(
        "worktree-cwd-detection",
        DetectionConfig {
            working_poll_interval: Duration::from_millis(100),
            idle_poll_interval: Duration::from_millis(100),
            auto_answer_interstitial_prompts: true,
        },
    )
    .await;
    let subdirectory = server.project_cwd.join("feature-subdirectory");
    std::fs::create_dir(&subdirectory).unwrap();
    let session_id = "7b269c6d-5c89-4df5-9ed5-54ded64abb00";
    let transcript_dir = server.home_dir.join(".codex/sessions/2026/09/26");
    std::fs::create_dir_all(&transcript_dir).unwrap();
    let transcript = transcript_dir.join(format!("rollout-2026-09-26T09-00-00-{session_id}.jsonl"));
    std::fs::write(
        &transcript,
        serde_json::json!({
            "type": "session_meta",
            "payload": {"id": session_id, "cwd": subdirectory}
        })
        .to_string(),
    )
    .unwrap();

    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "worktree-cwd-detection".into(),
        },
    )
    .await
    .unwrap();
    expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::PlainShell,
            working_directory: NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let shell_tree = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(shell_tree) = shell_tree else {
        unreachable!()
    };
    let shell_id = shell_tree
        .all_ids()
        .find(|node_id| {
            matches!(
                shell_tree.get(*node_id).map(|node| &node.kind),
                Some(NodeKind::Pane { .. })
            )
        })
        .unwrap();
    write_frame(
        &mut client,
        &ClientRequest::SetPaneFocus {
            pane_id: shell_id,
            focused: true,
        },
    )
    .await
    .unwrap();
    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id: shell_id,
            bytes: format!(
                "cd {}\rprintf '__CWD_READY__:%s\\n' \"$PWD\"\r",
                subdirectory.display()
            )
            .into_bytes(),
            submission: None,
        },
    )
    .await
    .unwrap();
    expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::ScreenUpdate { pane_id, bytes, .. }
                if *pane_id == shell_id
                    && String::from_utf8_lossy(bytes).contains(&format!("__CWD_READY__:{}", subdirectory.display()))
        )
    })
    .await;

    let command = format!(
        "{} resume {session_id} {}",
        fake_codex.path.display(),
        transcript.display()
    );
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(command),
            working_directory: NewPaneWorkingDirectory::FocusedTerminal,
        },
    )
    .await
    .unwrap();
    let agent_tree = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if tree.all_ids().any(|id| id != shell_id && matches!(tree.get(id).map(|node| &node.kind), Some(NodeKind::Pane { .. }))))
    })
    .await;
    let ServerEvent::TreeSnapshot(agent_tree) = agent_tree else {
        unreachable!()
    };
    let agent_id = agent_tree
        .all_ids()
        .find(|node_id| {
            *node_id != shell_id
                && matches!(
                    agent_tree.get(*node_id).map(|node| &node.kind),
                    Some(NodeKind::Pane { .. })
                )
        })
        .unwrap();
    assert_eq!(agent_tree.pane_cwd(agent_id), Some(subdirectory.as_path()));

    let resolved = expect_event(&mut client, Duration::from_secs(30), |event| {
        matches!(
            event,
            ServerEvent::PaneSessionIdResolved { pane_id, session_id: resolved_id, .. }
                if *pane_id == agent_id && resolved_id == session_id
        )
    })
    .await;
    assert!(matches!(
        resolved,
        ServerEvent::PaneSessionIdResolved { .. }
    ));

    let snapshot_path = server.snapshot_path.clone();
    assert!(
        wait_until(
            || {
                std::fs::read(&snapshot_path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                    .and_then(|snapshot| {
                        serde_json::from_value::<Tree>(snapshot["tree"].clone()).ok()
                    })
                    .is_some_and(|tree| tree.pane_cwd(agent_id) == Some(subdirectory.as_path()))
            },
            Duration::from_secs(5)
        )
        .await,
        "server did not persist the agent's launch directory"
    );
    let snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&snapshot_path).unwrap()).unwrap();
    let restored_tree: Tree = serde_json::from_value(snapshot["tree"].clone()).unwrap();
    assert_eq!(
        restored_tree.pane_cwd(agent_id),
        Some(subdirectory.as_path())
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), &mut server.server_task)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
}
