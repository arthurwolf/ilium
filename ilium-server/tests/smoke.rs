//! End-to-end smoke test: runs a real `ilium_server::run` bound to a
//! tempdir UDS socket, connects a raw `tokio::net::UnixStream` (playing
//! the part of `ilium-client`, which doesn't exist yet), and drives it
//! through `Attach` -> `NewPane` -> `KillSession` asserting the
//! `ServerEvent`s that come back. Hermetic: a tempdir socket/snapshot
//! path, no real `~/.local/share/ilium` writes, no dependency on a real
//! `claude`/`codex` binary (the plain shell it spawns is `$SHELL`, falling
//! back to `/bin/sh`, exactly like the pre-refactor bin crate's own tests).
//!
//! Shared server-startup/polling/frame-reading helpers live in
//! `tests/common/mod.rs`, alongside `live_agent_detection.rs`'s own use of
//! the same `TestServer`.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use ilium_core::{NodeId, SplitOrientation, ROOT_ID};
use ilium_ipc::{
    read_frame, write_frame, AgentDebugEventKind, ClientRequest, NewPaneKind, PaneResizeCause,
    PromptSubmissionSource, ServerEvent,
};

mod common;
use common::{expect_event, TestServer};

/// Creates an absolute-path fake Codex process which has no readable composer
/// for one second, then shows Codex's real `send a message` prompt before it
/// reads stdin. This proves initial input waits for visible readiness instead
/// of merely racing the PTY spawn.
fn write_delayed_ready_codex_binary(bin_dir: &std::path::Path) -> std::path::PathBuf {
    let script_path = bin_dir.join("codex");
    let script = "#!/bin/sh\nprintf 'starting codex...\\n'\nsleep 1\nprintf '\\033[2J\\033[H  send a message\\n'\nIFS= read -r line\nprintf 'received-after-ready:<%s>\\n' \"$line\"\nsleep 60\n";
    let mut file = std::fs::File::create(&script_path).expect("create delayed fake Codex");
    file.write_all(script.as_bytes())
        .expect("write delayed fake Codex");
    file.set_permissions(std::fs::Permissions::from_mode(0o700))
        .expect("make delayed fake Codex executable");
    script_path
}

/// A fresh session always owns one launch project. Root-level shortcuts are
/// resolved inside that project, keeping the test fixtures aligned with the
/// user-visible `project -> group -> pane` hierarchy.
fn launch_project_id(tree: &ilium_core::Tree) -> NodeId {
    tree.project_ids()
        .into_iter()
        .next()
        .expect("a launch project should exist")
}

fn first_launch_project_pane(tree: &ilium_core::Tree) -> NodeId {
    let project_id = launch_project_id(tree);
    let default_group = tree.children_of(project_id).unwrap()[0];
    tree.children_of(default_group).unwrap()[0]
}

#[tokio::test]
async fn attach_returns_a_tree_snapshot_with_an_empty_launch_project() {
    let mut server = TestServer::start("attach-test").await;
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "attach-test".to_string(),
        },
    )
    .await
    .expect("write Attach request");

    let event = expect_event(&mut client, Duration::from_secs(5), |_| true).await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        panic!("expected TreeSnapshot on attach, got {event:?}");
    };
    assert!(
        tree.get(ROOT_ID).is_some(),
        "snapshot should include the root"
    );
    assert_eq!(
        tree.children_of(ROOT_ID).unwrap().len(),
        1,
        "a brand-new session should contain its launch project"
    );
    assert!(tree.get(launch_project_id(&tree)).unwrap().is_project());
    let startup_complete = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;
    assert_eq!(startup_complete, ServerEvent::InitialStateSyncComplete);

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    tokio::time::timeout(Duration::from_secs(5), &mut server.server_task)
        .await
        .expect("server should shut down after KillSession")
        .expect("server task should not panic")
        .expect("server should exit cleanly");
}

#[tokio::test]
async fn attach_to_the_wrong_session_name_gets_an_error() {
    let mut server = TestServer::start("right-session").await;
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "wrong-session".to_string(),
        },
    )
    .await
    .expect("write Attach request");

    let event: ServerEvent = read_frame(&mut client).await.expect("read a reply");
    assert!(
        matches!(event, ServerEvent::Error { .. }),
        "expected an Error reply for a session-name mismatch, got {event:?}"
    );

    // `KillSession` is handled regardless of whether `Attach` ever
    // succeeded on this connection (see `ipc::handlers::handle_request`),
    // so use it here too -- it lets the server task spawned by
    // `TestServer::start` shut down on its own instead of relying on
    // `TestServer`'s `Drop` impl to abort it (that's only the fallback for
    // a test that panics before reaching this point).
    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn new_pane_creates_a_shell_and_broadcasts_a_tree_snapshot_containing_it() {
    let mut server = TestServer::start("new-pane-test").await;
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "new-pane-test".to_string(),
        },
    )
    .await
    .unwrap();
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::PlainShell,
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("write NewPane request");

    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches TreeSnapshot");
    };

    let project_id = launch_project_id(&tree);
    let default_group = tree.children_of(project_id).expect("project has children")[0];
    let pane_ids = tree.children_of(default_group).expect("group has children");
    assert_eq!(
        pane_ids.len(),
        1,
        "expected exactly one pane in the default group"
    );
    let pane = tree.get(pane_ids[0]).expect("pane node exists");
    assert!(pane.is_pane());

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn keyboard_submission_is_broadcast_only_after_the_pty_accepts_enter() {
    let mut server = TestServer::start("keyboard-submission-test").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "keyboard-submission-test".to_owned(),
        },
    )
    .await
    .unwrap();
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::PlainShell,
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let tree = expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == 1),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = tree else {
        unreachable!("predicate only returns a populated tree")
    };
    let pane_id = first_launch_project_pane(&tree);

    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"printf trigger-keyboard\r".to_vec(),
            submission: Some(PromptSubmissionSource::Keyboard),
        },
    )
    .await
    .unwrap();

    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::PanePromptSubmitted {
                pane_id: submitted_pane_id,
                source: PromptSubmissionSource::Keyboard,
            } if *submitted_pane_id == pane_id
        )
    })
    .await;
    assert_eq!(
        event,
        ServerEvent::PanePromptSubmitted {
            pane_id,
            source: PromptSubmissionSource::Keyboard,
        }
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn voice_submission_unblocks_a_real_pty_reader_with_enter() {
    let mut server = TestServer::start("voice-submission-test").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "voice-submission-test".to_owned(),
        },
    )
    .await
    .unwrap();
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            // `read` cannot finish on typed text alone. Its marker therefore
            // proves the carriage return reached the child as a real Enter.
            kind: NewPaneKind::Command(
                "IFS= read -r line; printf 'voice-enter-accepted:<%s>\\n' \"$line\"".to_owned(),
            ),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let tree = expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == 1),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = tree else {
        unreachable!("predicate only returns a populated tree")
    };
    let pane_id = first_launch_project_pane(&tree);

    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"message from voice\r".to_vec(),
            submission: Some(PromptSubmissionSource::VoiceControl),
        },
    )
    .await
    .unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut saw_prompt_event = false;
    let mut screen_bytes = Vec::new();
    while !saw_prompt_event
        || !String::from_utf8_lossy(&screen_bytes)
            .contains("voice-enter-accepted:<message from voice>")
    {
        let event = tokio::time::timeout_at(deadline, read_frame::<ServerEvent, _>(&mut client))
            .await
            .expect("voice submission events should arrive before timeout")
            .expect("read voice submission event");
        match event {
            ServerEvent::PanePromptSubmitted {
                pane_id: submitted_pane_id,
                source: PromptSubmissionSource::VoiceControl,
            } if submitted_pane_id == pane_id => saw_prompt_event = true,
            ServerEvent::ScreenUpdate {
                pane_id: updated_pane_id,
                bytes,
                ..
            } if updated_pane_id == pane_id => screen_bytes.extend_from_slice(&bytes),
            _ => {}
        }
    }

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn command_with_initial_input_waits_for_agent_composer_then_submits_enter() {
    let fake_bin_dir = tempfile::tempdir().expect("create fake Codex directory");
    let fake_codex_path = write_delayed_ready_codex_binary(fake_bin_dir.path());
    let mut server = TestServer::start_with_detection_config(
        "initial-input-test",
        ilium_server::config::DetectionConfig {
            working_poll_interval: Duration::from_millis(100),
            idle_poll_interval: Duration::from_millis(100),
        },
    )
    .await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "initial-input-test".to_string(),
        },
    )
    .await
    .unwrap();
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            // This process exposes no ready composer until after its startup
            // delay, then emits its marker only after the final Enter.
            kind: NewPaneKind::CommandWithInitialInput {
                command_line: fake_codex_path.to_string_lossy().to_string(),
                initial_input: "/goal inspect the selected line".to_string(),
            },
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("create command pane with initial input");

    let no_early_submission = tokio::time::timeout(Duration::from_millis(600), async {
        loop {
            let event: ServerEvent = read_frame(&mut client)
                .await
                .expect("read events before fake agent prompt appears");
            assert!(
                !matches!(
                    event,
                    ServerEvent::PanePromptSubmitted {
                        source: PromptSubmissionSource::InitialAgentPrompt,
                        ..
                    }
                ),
                "initial input must not arrive before the agent shows its composer"
            );
        }
    })
    .await;
    assert!(
        no_early_submission.is_err(),
        "the fake agent deliberately hides its composer for 1 second"
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut saw_prompt_event = false;
    let mut saw_submitted_output = false;
    while !(saw_prompt_event && saw_submitted_output) {
        let event = tokio::time::timeout_at(deadline, read_frame::<ServerEvent, _>(&mut client))
            .await
            .expect("initial input events should arrive before timeout")
            .expect("read initial input event");
        saw_prompt_event |= matches!(
            event,
            ServerEvent::PanePromptSubmitted {
                source: PromptSubmissionSource::InitialAgentPrompt,
                ..
            }
        );
        saw_submitted_output |= matches!(
            event,
            ServerEvent::ScreenUpdate { ref bytes, .. }
                if String::from_utf8_lossy(bytes)
                    .contains("received-after-ready:</goal inspect the selected line>")
        );
    }

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn manual_input_cancels_an_initial_agent_prompt_while_it_is_still_waiting() {
    let fake_bin_dir = tempfile::tempdir().expect("create fake Codex directory");
    let fake_codex_path = write_delayed_ready_codex_binary(fake_bin_dir.path());
    let mut server = TestServer::start_with_detection_config(
        "initial-input-cancel-test",
        ilium_server::config::DetectionConfig {
            working_poll_interval: Duration::from_millis(100),
            idle_poll_interval: Duration::from_millis(100),
        },
    )
    .await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "initial-input-cancel-test".to_string(),
        },
    )
    .await
    .expect("attach test client");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::CommandWithInitialInput {
                command_line: fake_codex_path.to_string_lossy().to_string(),
                initial_input: "/goal automatic task".to_string(),
            },
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("create delayed agent pane");
    let tree = expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == 1),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = tree else {
        unreachable!("predicate only accepts a tree snapshot")
    };
    let pane_id = first_launch_project_pane(&tree);

    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"manual task\r".to_vec(),
            submission: Some(PromptSubmissionSource::Keyboard),
        },
    )
    .await
    .expect("send manual input before agent composer exists");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut saw_manual_output = false;
    while !saw_manual_output {
        let event = tokio::time::timeout_at(deadline, read_frame::<ServerEvent, _>(&mut client))
            .await
            .expect("manual input should reach the delayed agent")
            .expect("read manual-input event");
        assert!(
            !matches!(
                event,
                ServerEvent::PanePromptSubmitted {
                    source: PromptSubmissionSource::InitialAgentPrompt,
                    ..
                }
            ),
            "manual terminal input must cancel the delayed automatic prompt"
        );
        saw_manual_output = matches!(
            event,
            ServerEvent::ScreenUpdate { ref bytes, .. }
                if String::from_utf8_lossy(bytes)
                    .contains("received-after-ready:<manual task>")
        );
    }

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("kill test session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn scheduled_input_executes_after_client_detaches_and_clears_its_countdown() {
    let mut server = TestServer::start("scheduled-input-test").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "scheduled-input-test".to_string(),
        },
    )
    .await
    .unwrap();
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(
                "IFS= read -r line; printf 'scheduled:<%s>\\n' \"$line\"".to_string(),
            ),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let created_tree = expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == 1),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = created_tree else {
        unreachable!("predicate only returns a tree snapshot");
    };
    let pane_id = tree.panes().next().unwrap().id;

    write_frame(
        &mut client,
        &ClientRequest::SchedulePaneInput {
            pane_id,
            delay_seconds: 1,
            text: "detached payload".to_string(),
            send_enter: true,
        },
    )
    .await
    .unwrap();
    let scheduled_tree = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if tree.scheduled_pane_inputs().count() == 1)
    })
    .await;
    assert!(matches!(scheduled_tree, ServerEvent::TreeSnapshot(_)));
    drop(client);

    tokio::time::sleep(Duration::from_millis(1300)).await;

    let mut reattached = server.connect().await;
    write_frame(
        &mut reattached,
        &ClientRequest::Attach {
            session: "scheduled-input-test".to_string(),
        },
    )
    .await
    .unwrap();
    let cleared_tree = expect_event(&mut reattached, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if tree.scheduled_pane_inputs().count() == 0)
    })
    .await;
    assert!(matches!(cleared_tree, ServerEvent::TreeSnapshot(_)));
    let replay = expect_event(&mut reattached, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::TerminalReplay { pane_id: event_pane_id, bytes, .. }
                if *event_pane_id == pane_id
                    && String::from_utf8_lossy(bytes).contains("scheduled:<detached payload>")
        )
    })
    .await;
    assert!(matches!(replay, ServerEvent::TerminalReplay { .. }));

    write_frame(&mut reattached, &ClientRequest::KillSession)
        .await
        .unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn create_split_view_atomically_moves_panes_and_persists_orientation() {
    let mut server = TestServer::start("split-view-test").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "split-view-test".to_string(),
        },
    )
    .await
    .unwrap();
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;

    let mut pane_ids = Vec::new();
    let mut parent_group = ROOT_ID;
    for expected_count in 1..=2 {
        write_frame(
            &mut client,
            &ClientRequest::NewPane {
                parent_group: ROOT_ID,
                kind: NewPaneKind::PlainShell,
                working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
            },
        )
        .await
        .unwrap();
        let event = expect_event(&mut client, Duration::from_secs(5), |event| {
            matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == expected_count)
        })
        .await;
        let ServerEvent::TreeSnapshot(tree) = event else {
            unreachable!();
        };
        parent_group = tree.children_of(launch_project_id(&tree)).unwrap()[0];
        pane_ids = tree.panes().map(|node| node.id).collect();
    }
    pane_ids.sort();

    write_frame(
        &mut client,
        &ClientRequest::CreateSplitView {
            parent_group,
            name: "Vertical split".to_string(),
            orientation: SplitOrientation::Vertical,
            pane_ids: pane_ids.clone(),
        },
    )
    .await
    .unwrap();
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if tree.all_ids().any(|id| tree.split_orientation(id).is_some()))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!();
    };
    let split_id = tree
        .all_ids()
        .find(|id| tree.split_orientation(*id).is_some())
        .unwrap();
    assert_eq!(
        tree.split_orientation(split_id),
        Some(SplitOrientation::Vertical)
    );
    assert_eq!(tree.children_of(split_id).unwrap(), pane_ids.as_slice());

    let snapshot_written = common::wait_until(
        || {
            std::fs::read_to_string(&server.snapshot_path).is_ok_and(|contents| {
                contents.contains("Vertical split") && contents.contains("\"Vertical\"")
            })
        },
        Duration::from_secs(5),
    )
    .await;
    assert!(snapshot_written, "split orientation was not persisted");

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn invalid_split_request_returns_an_error_without_mutating_the_tree() {
    let mut server = TestServer::start("invalid-split-view-test").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "invalid-split-view-test".to_string(),
        },
    )
    .await
    .unwrap();
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::NewGroup {
            parent_group: ROOT_ID,
            name: "work".to_string(),
        },
    )
    .await
    .unwrap();
    let created = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if tree.children_of(ROOT_ID).is_ok_and(|children| children.len() == 1))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree_before) = created else {
        unreachable!();
    };
    let group_id = tree_before
        .children_of(launch_project_id(&tree_before))
        .unwrap()[0];

    // A group is not an eligible split member. The domain validates every
    // requested member before inserting the split, so this must fail as one
    // transaction rather than leave an empty split behind.
    write_frame(
        &mut client,
        &ClientRequest::CreateSplitView {
            parent_group: group_id,
            name: "Invalid split".to_string(),
            orientation: SplitOrientation::Horizontal,
            pane_ids: vec![group_id],
        },
    )
    .await
    .unwrap();
    let error = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::Error { .. })
    })
    .await;
    assert!(matches!(error, ServerEvent::Error { .. }));

    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "invalid-split-view-test".to_string(),
        },
    )
    .await
    .unwrap();
    let snapshot = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree_after) = snapshot else {
        unreachable!();
    };
    assert_eq!(tree_after, tree_before);
    assert!(
        tree_after
            .all_ids()
            .all(|node_id| tree_after.split_orientation(node_id).is_none()),
        "the rejected request must not leave a split container behind"
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn reattached_client_receives_terminal_output_produced_before_it_connected() {
    let mut server = TestServer::start("terminal-replay-test").await;
    let mut first_client = server.connect().await;

    write_frame(
        &mut first_client,
        &ClientRequest::Attach {
            session: "terminal-replay-test".to_string(),
        },
    )
    .await
    .expect("attach first client");
    let _ = expect_event(&mut first_client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;

    write_frame(
        &mut first_client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::PlainShell,
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("create shell pane");
    let created_tree = expect_event(
        &mut first_client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == 1),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = created_tree else {
        unreachable!("predicate only returns a tree snapshot");
    };
    let pane_id = tree.panes().next().expect("created pane exists").id;

    write_frame(
        &mut first_client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes:
                b"for number in $(seq 1 160); do printf 'replay-line-%03d\\n' \"$number\"; done\n"
                    .to_vec(),
            submission: None,
        },
    )
    .await
    .expect("write output-producing command");
    let _ = expect_event(&mut first_client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::ScreenUpdate { pane_id: event_pane_id, bytes, .. }
                if *event_pane_id == pane_id
                    && String::from_utf8_lossy(bytes).contains("replay-line-160")
        )
    })
    .await;
    drop(first_client);

    let mut reattached_client = server.connect().await;
    write_frame(
        &mut reattached_client,
        &ClientRequest::Attach {
            session: "terminal-replay-test".to_string(),
        },
    )
    .await
    .expect("attach second client");
    let replay_event = expect_event(&mut reattached_client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::TerminalReplay { pane_id: event_pane_id, bytes, .. }
                if *event_pane_id == pane_id
                    && String::from_utf8_lossy(bytes).contains("replay-line-001")
                    && String::from_utf8_lossy(bytes).contains("replay-line-160")
        )
    })
    .await;
    let ServerEvent::TerminalReplay {
        through_sequence,
        is_complete,
        ..
    } = replay_event
    else {
        unreachable!("predicate only returns a terminal replay");
    };
    assert!(
        is_complete,
        "small text transcript should not hit the journal cap"
    );
    assert!(
        through_sequence > 0,
        "replay must carry a live-output watermark"
    );

    write_frame(&mut reattached_client, &ClientRequest::KillSession)
        .await
        .expect("kill test session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn resize_on_an_unknown_pane_returns_an_error_not_a_dropped_connection() {
    let mut server = TestServer::start("resize-error-test").await;
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::ResizePane {
            pane_id: NodeId(999),
            rows: 40,
            cols: 100,
            cause: PaneResizeCause::HostTerminal,
        },
    )
    .await
    .expect("write ResizePane request");

    let event: ServerEvent = read_frame(&mut client).await.expect("read a reply");
    assert!(
        matches!(event, ServerEvent::Error { .. }),
        "expected an Error reply for an unknown pane id, got {event:?}"
    );

    // The connection must still be usable after the error -- send a real
    // request and confirm it still gets handled.
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "resize-error-test".to_string(),
        },
    )
    .await
    .expect("write Attach request after a prior error");
    let event: ServerEvent = read_frame(&mut client).await.expect("read a reply");
    assert!(matches!(event, ServerEvent::TreeSnapshot(_)));

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn successful_resize_records_its_typed_client_cause_in_the_debug_journal() {
    let mut server = TestServer::start_with_agent_debug(
        "resize-debug-cause-test",
        ilium_server::config::DetectionConfig::default(),
    )
    .await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "resize-debug-cause-test".to_string(),
        },
    )
    .await
    .expect("attach debug-enabled client");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::PlainShell,
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("create terminal pane");
    let tree_event = expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().next().is_some()),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = tree_event else {
        unreachable!("predicate only matches TreeSnapshot");
    };
    let pane_id = tree.panes().next().expect("created pane").id;

    write_frame(
        &mut client,
        &ClientRequest::ResizePane {
            pane_id,
            rows: 40,
            cols: 100,
            cause: PaneResizeCause::TreePanelAnimation,
        },
    )
    .await
    .expect("resize terminal pane");
    write_frame(
        &mut client,
        &ClientRequest::GetPaneDebugLog {
            pane_id,
            after_sequence: None,
        },
    )
    .await
    .expect("request debug journal");
    let debug_event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::PaneDebugLogSnapshot { pane_id: event_pane_id, .. } if *event_pane_id == pane_id)
    })
    .await;
    let ServerEvent::PaneDebugLogSnapshot { entries, .. } = debug_event else {
        unreachable!("predicate only matches PaneDebugLogSnapshot");
    };
    let resize_entry = entries
        .iter()
        .find(|entry| entry.kind == AgentDebugEventKind::PaneResized)
        .expect("successful resize should be journaled");
    assert!(resize_entry.is_tree_panel_animation_resize());
    assert!(resize_entry.fields.iter().any(|field| {
        field.label == "cause" && field.value == PaneResizeCause::TreePanelAnimation.label()
    }));

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("kill debug test session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn reparent_node_moves_a_pane_into_a_different_group_at_an_index() {
    let mut server = TestServer::start("reparent-test").await;
    let mut client = server.connect().await;

    // Two groups, each with one pane: `source`'s pane will move into
    // `dest` at index 0 (ahead of the pane already there).
    write_frame(
        &mut client,
        &ClientRequest::NewGroup {
            parent_group: ROOT_ID,
            name: "source".to_string(),
        },
    )
    .await
    .unwrap();
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!()
    };
    let source_group = *tree
        .children_of(launch_project_id(&tree))
        .unwrap()
        .last()
        .expect("source group present");

    write_frame(
        &mut client,
        &ClientRequest::NewGroup {
            parent_group: ROOT_ID,
            name: "dest".to_string(),
        },
    )
    .await
    .unwrap();
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!()
    };
    let dest_group = *tree
        .children_of(launch_project_id(&tree))
        .unwrap()
        .last()
        .expect("dest group present");

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: source_group,
            kind: NewPaneKind::PlainShell,
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if !tree.children_of(source_group).unwrap().is_empty())
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!()
    };
    let moved_pane = tree.children_of(source_group).unwrap()[0];

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: dest_group,
            kind: NewPaneKind::PlainShell,
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if !tree.children_of(dest_group).unwrap().is_empty())
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::ReparentNode {
            node_id: moved_pane,
            new_parent: dest_group,
            index: Some(0),
        },
    )
    .await
    .expect("write ReparentNode request");

    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if tree.children_of(source_group).unwrap().is_empty())
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!()
    };

    assert!(
        tree.children_of(source_group).unwrap().is_empty(),
        "source group should have lost its pane"
    );
    let dest_children = tree.children_of(dest_group).unwrap();
    assert_eq!(
        dest_children.len(),
        2,
        "dest group should now have both panes"
    );
    assert_eq!(
        dest_children[0], moved_pane,
        "moved pane should be inserted at index 0"
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn close_pane_removes_it_and_a_second_client_sees_the_update() {
    let mut server = TestServer::start("close-pane-test").await;
    let mut creator = server.connect().await;
    let mut observer = server.connect().await;

    write_frame(
        &mut creator,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::PlainShell,
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let event = expect_event(&mut creator, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!()
    };
    let pane_id = first_launch_project_pane(&tree);

    write_frame(&mut creator, &ClientRequest::ClosePane { pane_id })
        .await
        .unwrap();

    // The observer connection never sent anything -- it should still see
    // the broadcast tree snapshot after the pane it never asked to create
    // gets closed by another client.
    let event = expect_event(
        &mut observer,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.get(pane_id).is_none()),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!()
    };
    assert!(
        tree.get(pane_id).is_none(),
        "closed pane should be gone from the tree"
    );

    write_frame(&mut creator, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}
