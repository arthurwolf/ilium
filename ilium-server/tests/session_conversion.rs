//! Server half of "Convert to": `TerminatePaneProcess` keeps the pane node
//! while stopping its process, and `ReplacePaneWithCommand` swaps a pane for
//! a new one in the same parent and position. Hermetic: real PTYs running
//! `ilium-test-fixtures` executables, a tempdir socket.

use std::time::Duration;

use ilium_core::{NodeId, ROOT_ID};
use ilium_ipc::{write_frame, ClientRequest, NewPaneKind, ServerEvent};

mod common;
use common::{expect_event, read_initial_state, TestServer};
use ilium_test_fixtures::{install, FixtureBehavior};

fn echo_fixture(dir: &std::path::Path, name: &str) -> String {
    install(
        dir,
        name,
        &FixtureBehavior::EchoSubmittedLine {
            prefix: name.to_string(),
        },
    )
    .path
    .to_string_lossy()
    .into_owned()
}

async fn new_command_pane(
    client: &mut ilium_transport::SessionStream,
    command: String,
    expected_pane_count: usize,
) -> ilium_core::Tree {
    write_frame(
        client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(command),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("send NewPane");
    let event = expect_event(client, Duration::from_secs(10), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == expected_pane_count)
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only accepts a tree snapshot")
    };
    tree
}

fn pane_ids_in_order(tree: &ilium_core::Tree) -> Vec<NodeId> {
    tree.pane_ids_in_tree_order()
}

#[tokio::test]
async fn terminate_keeps_the_pane_and_replace_swaps_it_in_place() {
    let fixtures = tempfile::tempdir().expect("fixture dir");
    let first = echo_fixture(fixtures.path(), "first-agent");
    let second = echo_fixture(fixtures.path(), "second-agent");
    let sibling = echo_fixture(fixtures.path(), "sibling-agent");

    let mut server = TestServer::start("conversion-test").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "conversion-test".to_string(),
        },
    )
    .await
    .expect("attach");
    read_initial_state(&mut client, Duration::from_secs(5)).await;

    let tree = new_command_pane(&mut client, first, 1).await;
    let converted_pane = pane_ids_in_order(&tree)[0];
    let tree = new_command_pane(&mut client, sibling, 2).await;
    let order_before = pane_ids_in_order(&tree);
    assert_eq!(order_before[0], converted_pane);
    let sibling_pane = order_before[1];
    let parent = tree.parent_of(converted_pane).expect("pane has a parent");

    write_frame(
        &mut client,
        &ClientRequest::RenameNode {
            node_id: converted_pane,
            title: "Session conversion research".to_string(),
            short_title: Some("Conversion research".to_string()),
            inferred_icon: Some("book".to_string()),
        },
    )
    .await
    .expect("rename source pane");
    expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree)
            if tree.get(converted_pane).is_some_and(|node| node.name == "Session conversion research"))
    }).await;

    // Stopping the process answers exactly once and leaves the node alone.
    write_frame(
        &mut client,
        &ClientRequest::TerminatePaneProcess {
            pane_id: converted_pane,
        },
    )
    .await
    .expect("send TerminatePaneProcess");
    let event = expect_event(&mut client, Duration::from_secs(10), |event| {
        matches!(event, ServerEvent::PaneProcessTerminated { .. })
    })
    .await;
    assert_eq!(
        event,
        ServerEvent::PaneProcessTerminated {
            pane_id: converted_pane,
            result: Ok(()),
        }
    );

    // Replacement: new pane takes the old one's parent and position.
    write_frame(
        &mut client,
        &ClientRequest::ReplacePaneWithCommand {
            pane_id: converted_pane,
            command_line: second,
        },
    )
    .await
    .expect("send ReplacePaneWithCommand");
    let event = expect_event(&mut client, Duration::from_secs(10), |event| {
        matches!(
            event,
            ServerEvent::TreeSnapshot(tree)
                if tree.get(converted_pane).is_none() && tree.panes().count() == 2
        )
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!("predicate only accepts a tree snapshot")
    };
    let order_after = pane_ids_in_order(&tree);
    assert_eq!(order_after.len(), 2);
    assert_eq!(order_after[1], sibling_pane, "sibling keeps its place");
    let replacement = order_after[0];
    assert_ne!(replacement, converted_pane);
    assert_eq!(tree.parent_of(replacement), Some(parent));
    let node = tree.get(replacement).expect("replacement exists");
    assert_eq!(node.name, "Session conversion research");
    assert_eq!(node.short_name.as_deref(), Some("Conversion research"));
    assert_eq!(node.inferred_icon.as_deref(), Some("book"));
    assert!(node.is_name_fixed);
    assert!(matches!(
        node.kind,
        ilium_core::NodeKind::Pane {
            title_source: ilium_core::PaneTitleSource::UserSpecified,
            ..
        }
    ));

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("kill session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn freeze_rejects_generic_command_without_stopping_the_pane() {
    let fixtures = tempfile::tempdir().expect("fixture dir");
    let legacy_command = echo_fixture(fixtures.path(), "legacy-agent");
    let server = TestServer::start("legacy-unfreeze-test").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "legacy-unfreeze-test".to_string(),
        },
    )
    .await
    .expect("attach");
    read_initial_state(&mut client, Duration::from_secs(5)).await;

    let tree = new_command_pane(&mut client, legacy_command.clone(), 1).await;
    let pane_id = pane_ids_in_order(&tree)[0];
    write_frame(
        &mut client,
        &ClientRequest::FreezePane {
            pane_id,
            resume_command: legacy_command,
        },
    )
    .await
    .expect("send FreezePane");
    let event = expect_event(&mut client, Duration::from_secs(10), |event| {
        matches!(event, ServerEvent::PaneFrozen { .. })
    })
    .await;
    assert_eq!(
        event,
        ServerEvent::PaneFrozen {
            pane_id,
            result: Err("freeze requires the pane's current provider session identity".into()),
        }
    );

    write_frame(&mut client, &ClientRequest::UnfreezePane { pane_id })
        .await
        .expect("send UnfreezePane");
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::Error { .. })
    })
    .await;
    assert!(matches!(event, ServerEvent::Error { message } if message.contains("is not frozen")));
}

#[tokio::test]
async fn terminating_an_unknown_pane_reports_the_failure() {
    let mut server = TestServer::start("conversion-unknown-test").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "conversion-unknown-test".to_string(),
        },
    )
    .await
    .expect("attach");
    read_initial_state(&mut client, Duration::from_secs(5)).await;

    write_frame(
        &mut client,
        &ClientRequest::TerminatePaneProcess {
            pane_id: NodeId(9999),
        },
    )
    .await
    .expect("send TerminatePaneProcess");
    let event = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::PaneProcessTerminated { .. })
    })
    .await;
    assert!(matches!(
        event,
        ServerEvent::PaneProcessTerminated { result: Err(_), .. }
    ));

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("kill session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}
