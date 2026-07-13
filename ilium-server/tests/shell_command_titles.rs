//! End-to-end title tests over the real UDS and real PTYs. The server owns
//! both the command tracker and authoritative tree, so these cover the full
//! input -> title -> broadcast path without a client-side mock.

use std::time::Duration;

use ilium_core::{NodeKind, PaneTitleSource, ROOT_ID};
use ilium_ipc::{write_frame, ClientRequest, NewPaneKind, ServerEvent};

mod common;
use common::{expect_event, TestServer};

async fn create_plain_shell(
    client: &mut tokio::net::UnixStream,
    session_name: &str,
) -> ilium_core::NodeId {
    write_frame(
        client,
        &ClientRequest::Attach {
            session: session_name.to_string(),
        },
    )
    .await
    .expect("attach client");
    let _ = expect_event(client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;

    write_frame(
        client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::PlainShell,
        },
    )
    .await
    .expect("create plain shell");
    let event = expect_event(
        client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == 1),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches tree snapshots");
    };
    let pane_id = tree.panes().next().expect("created pane").id;
    pane_id
}

async fn stop(server: TestServer, client: &mut tokio::net::UnixStream) {
    write_frame(client, &ClientRequest::KillSession)
        .await
        .expect("kill test session");
    tokio::time::timeout(Duration::from_secs(5), server.server_task)
        .await
        .expect("server shutdown timed out")
        .expect("server task panicked")
        .expect("server returned an error");
}

#[tokio::test]
async fn completed_shell_commands_update_the_title_for_other_clients_until_user_rename() {
    let server = TestServer::start("shell-title-test").await;
    let mut creator = server.connect().await;
    let mut observer = server.connect().await;
    let pane_id = create_plain_shell(&mut creator, "shell-title-test").await;

    write_frame(
        &mut observer,
        &ClientRequest::Attach {
            session: "shell-title-test".to_string(),
        },
    )
    .await
    .expect("attach observer");
    let _ = expect_event(&mut observer, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;

    for bytes in [b"echo ".as_slice(), b"shell-title-marker\r".as_slice()] {
        write_frame(
            &mut creator,
            &ClientRequest::KeyInput {
                pane_id,
                bytes: bytes.to_vec(),
            },
        )
        .await
        .expect("write shell input");
    }

    let event = expect_event(&mut observer, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::TreeSnapshot(tree)
                if tree.get(pane_id).is_some_and(|node| node.name == "echo shell-title-marker")
        )
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches tree snapshots");
    };
    let NodeKind::Pane { title_source, .. } = &tree.get(pane_id).expect("pane exists").kind else {
        panic!("expected pane");
    };
    assert_eq!(*title_source, PaneTitleSource::Automatic);

    write_frame(
        &mut creator,
        &ClientRequest::RenameNode {
            node_id: pane_id,
            title: "manual shell name".to_string(),
            short_title: None,
        },
    )
    .await
    .expect("rename pane");
    let _ = expect_event(&mut observer, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::TreeSnapshot(tree)
                if tree.get(pane_id).is_some_and(|node| node.name == "manual shell name")
        )
    })
    .await;
    // `creator` is subscribed to the same session-wide broadcast as
    // `observer` (every attached connection is, including the one that
    // triggered the mutation -- see `ipc::connection::handle`), so the
    // rename above is already sitting unread in `creator`'s stream. Drain
    // it now: otherwise the final `expect_event(&mut creator, ...)` below
    // would match *this* stale rename snapshot instead of a snapshot
    // produced after the upcoming "must-not-replace" command, making that
    // assertion pass even if automatic titling wrongly reasserted itself.
    let _ = expect_event(&mut creator, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::TreeSnapshot(tree)
                if tree.get(pane_id).is_some_and(|node| node.name == "manual shell name")
        )
    })
    .await;

    write_frame(
        &mut creator,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"echo must-not-replace-user-name\r".to_vec(),
        },
    )
    .await
    .expect("write after user rename");
    write_frame(
        &mut creator,
        &ClientRequest::Attach {
            session: "shell-title-test".to_string(),
        },
    )
    .await
    .expect("request authoritative snapshot");
    let event = expect_event(&mut creator, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::TreeSnapshot(tree)
                if tree.get(pane_id).is_some_and(|node| node.name == "manual shell name")
        )
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches tree snapshots");
    };
    let pane = tree.get(pane_id).expect("pane exists");
    assert_eq!(pane.name, "manual shell name");
    let NodeKind::Pane { title_source, .. } = &pane.kind else {
        panic!("expected pane");
    };
    assert_eq!(*title_source, PaneTitleSource::UserSpecified);

    stop(server, &mut creator).await;
}

#[tokio::test]
async fn foreground_non_shell_commands_do_not_receive_automatic_titles() {
    let server = TestServer::start("non-shell-title-test").await;
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command("cat".to_string()),
        },
    )
    .await
    .expect("create cat pane");
    let event = expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == 1),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches tree snapshots");
    };
    let pane_id = tree.panes().next().expect("created pane").id;

    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"not-a-shell-command-title\r".to_vec(),
        },
    )
    .await
    .expect("write cat input");
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "non-shell-title-test".to_string(),
        },
    )
    .await
    .expect("request snapshot");
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches tree snapshots");
    };
    assert_eq!(tree.get(pane_id).expect("pane exists").name, "cat");

    stop(server, &mut client).await;
}
