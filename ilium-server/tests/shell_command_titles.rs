//! End-to-end title tests over the real UDS and real PTYs. The server owns
//! both the command tracker and authoritative tree, so these cover the full
//! input -> title -> broadcast path without a client-side mock.
//! Unix-only, because the feature is. A typed command only becomes a title
//! while the shell itself is the terminal's foreground process group, which is
//! how the server tells "the user is typing at a prompt" from "a running
//! command owns the terminal". Windows has no equivalent: ConPTY exposes no
//! foreground process group, `PtySession::foreground_process_group_id` reports
//! nothing there, and shell-command titles are therefore inactive rather than
//! merely untested. See docs/TODO.md.
#![cfg(unix)]

use std::time::Duration;

use ilium_core::{NodeKind, PaneTitleSource, ROOT_ID};
use ilium_ipc::{write_frame, ClientRequest, NewPaneKind, ServerEvent};

mod common;
use common::{expect_event, TestServer};

async fn create_plain_shell(
    client: &mut ilium_transport::SessionStream,
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
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
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

async fn stop(mut server: TestServer, client: &mut ilium_transport::SessionStream) {
    write_frame(client, &ClientRequest::KillSession)
        .await
        .expect("kill test session");
    // Await through a `&mut` reference rather than moving `server_task` out
    // of `server` by value: `TestServer` has a `Drop` impl (see
    // `common::TestServer`), and Rust forbids partially moving a field out
    // of a type that implements `Drop`. Keeping the field in place also
    // means that if any `.expect()` below panics (e.g. a genuine shutdown
    // timeout), unwinding still drops `server` as a whole and its `Drop`
    // impl aborts the task instead of leaving it detached and running.
    tokio::time::timeout(Duration::from_secs(5), &mut server.server_task)
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
                submission: None,
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
            inferred_icon: None,
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
            submission: None,
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
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
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
            submission: None,
        },
    )
    .await
    .expect("write cat input");
    // Wait for proof the write actually reached the pane before asking for
    // a fresh snapshot below. Without this, the upcoming `Attach` could be
    // processed (and its `TreeSnapshot` reply generated) before the
    // `KeyInput` above finishes on the server -- since both are just
    // separate frames on the same connection with no ordering guarantee
    // from the client side alone -- and the final assertion would then
    // pass by coincidence (nothing happened yet) rather than by evidence
    // that a non-shell command genuinely never triggers a retitle. `cat`'s
    // PTY line discipline echoes typed input, so its marker text is
    // guaranteed to show up in a `ScreenUpdate` for this pane once the
    // server has processed the write.
    let marker = "not-a-shell-command-title";
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::ScreenUpdate { pane_id: event_pane_id, bytes, .. }
                if *event_pane_id == pane_id
                    && String::from_utf8_lossy(bytes).contains(marker)
        )
    })
    .await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "non-shell-title-test".to_string(),
        },
    )
    .await
    .expect("request snapshot");
    let event = expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.get(pane_id).is_some()),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only matches tree snapshots");
    };
    assert_eq!(tree.get(pane_id).expect("pane exists").name, "cat");

    stop(server, &mut client).await;
}
