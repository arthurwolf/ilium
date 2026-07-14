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

use std::time::Duration;

use ilium_core::{NodeId, SplitOrientation, ROOT_ID};
use ilium_ipc::{read_frame, write_frame, ClientRequest, NewPaneKind, ServerEvent};

mod common;
use common::{expect_event, TestServer};

#[tokio::test]
async fn attach_returns_a_tree_snapshot_with_just_the_root() {
    let server = TestServer::start("attach-test").await;
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
        0,
        "a brand-new session's tree should have no panes yet"
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    tokio::time::timeout(Duration::from_secs(5), server.server_task)
        .await
        .expect("server should shut down after KillSession")
        .expect("server task should not panic")
        .expect("server should exit cleanly");
}

#[tokio::test]
async fn attach_to_the_wrong_session_name_gets_an_error() {
    let server = TestServer::start("right-session").await;
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
    // so use it here too -- otherwise the server task spawned by
    // `TestServer::start` would keep running (holding its UDS listener
    // and detection loop alive) for the rest of this test binary's
    // process, since `TestServer` has no `Drop` that aborts it.
    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("write KillSession request");
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}

#[tokio::test]
async fn new_pane_creates_a_shell_and_broadcasts_a_tree_snapshot_containing_it() {
    let server = TestServer::start("new-pane-test").await;
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

    let default_group = tree
        .children_of(ROOT_ID)
        .expect("root is a group")
        .first()
        .copied()
        .expect("a default group should have been created for the pane");
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
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}

#[tokio::test]
async fn command_with_initial_input_writes_the_prompt_then_submits_enter() {
    let server = TestServer::start("initial-input-test").await;
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
            // This process emits its marker only after `read` receives the
            // final Enter, proving both writes in the new server path landed.
            kind: NewPaneKind::CommandWithInitialInput {
                command_line: "IFS= read -r line; printf 'submitted:<%s>\\n' \"$line\"".to_string(),
                initial_input: "/goal inspect the selected line".to_string(),
            },
        },
    )
    .await
    .expect("create command pane with initial input");

    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::ScreenUpdate { bytes, .. }
                if String::from_utf8_lossy(bytes)
                    .contains("submitted:</goal inspect the selected line>")
        )
    })
    .await;
    assert!(matches!(event, ServerEvent::ScreenUpdate { .. }));

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}

#[tokio::test]
async fn scheduled_input_executes_after_client_detaches_and_clears_its_countdown() {
    let server = TestServer::start("scheduled-input-test").await;
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
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}

#[tokio::test]
async fn create_split_view_atomically_moves_panes_and_persists_orientation() {
    let server = TestServer::start("split-view-test").await;
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
        parent_group = tree.children_of(ROOT_ID).unwrap()[0];
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
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}

#[tokio::test]
async fn invalid_split_request_returns_an_error_without_mutating_the_tree() {
    let server = TestServer::start("invalid-split-view-test").await;
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
    let group_id = tree_before.children_of(ROOT_ID).unwrap()[0];

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
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}

#[tokio::test]
async fn reattached_client_receives_terminal_output_produced_before_it_connected() {
    let server = TestServer::start("terminal-replay-test").await;
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
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}

#[tokio::test]
async fn resize_on_an_unknown_pane_returns_an_error_not_a_dropped_connection() {
    let server = TestServer::start("resize-error-test").await;
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::ResizePane {
            pane_id: NodeId(999),
            rows: 40,
            cols: 100,
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
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}

#[tokio::test]
async fn reparent_node_moves_a_pane_into_a_different_group_at_an_index() {
    let server = TestServer::start("reparent-test").await;
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
        .children_of(ROOT_ID)
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
        .children_of(ROOT_ID)
        .unwrap()
        .last()
        .expect("dest group present");

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: source_group,
            kind: NewPaneKind::PlainShell,
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
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}

#[tokio::test]
async fn close_pane_removes_it_and_a_second_client_sees_the_update() {
    let server = TestServer::start("close-pane-test").await;
    let mut creator = server.connect().await;
    let mut observer = server.connect().await;

    write_frame(
        &mut creator,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::PlainShell,
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
    let group = tree.children_of(ROOT_ID).unwrap()[0];
    let pane_id = tree.children_of(group).unwrap()[0];

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
    let _ = tokio::time::timeout(Duration::from_secs(5), server.server_task).await;
}
