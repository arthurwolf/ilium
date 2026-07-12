//! End-to-end smoke test: runs a real `illium_server::run` bound to a
//! tempdir UDS socket, connects a raw `tokio::net::UnixStream` (playing
//! the part of `illium-client`, which doesn't exist yet), and drives it
//! through `Attach` -> `NewPane` -> `KillSession` asserting the
//! `ServerEvent`s that come back. Hermetic: a tempdir socket/snapshot
//! path, no real `~/.local/share/illium` writes, no dependency on a real
//! `claude`/`codex` binary (the plain shell it spawns is `$SHELL`, falling
//! back to `/bin/sh`, exactly like the pre-refactor bin crate's own tests).
//!
//! Shared server-startup/polling/frame-reading helpers live in
//! `tests/common/mod.rs`, alongside `live_agent_detection.rs`'s own use of
//! the same `TestServer`.

use std::time::Duration;

use illium_core::{NodeId, ROOT_ID};
use illium_ipc::{read_frame, write_frame, ClientRequest, NewPaneKind, ServerEvent};

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
}
