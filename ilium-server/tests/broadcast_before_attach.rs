//! Kernel-owned IPC rule that stays in the server (reference-host-architecture.md, the server's
//! client connections): a connection that has not attached yet still receives tree broadcasts.
//! Twin of the original `ilium-server/src/ipc/connection.rs` unit test, driven through the real
//! server socket instead of the private writer task.

use std::time::Duration;

use ilium_core::ROOT_ID;
use ilium_ipc::{write_frame, ClientRequest, NewPaneKind, ServerEvent};

mod common;
use common::{expect_event, read_initial_state, TestServer};

const WAIT: Duration = Duration::from_secs(30);

/// REQ: PORT:ilium-server/src/ipc/connection.rs::broadcast_before_attach_is_forwarded
#[tokio::test]
async fn port_008_a_connection_that_never_attached_receives_the_tree_broadcast() {
    let server = TestServer::start("broadcast-before-attach").await;

    // The observer connects but sends no Attach: its replay barrier stays in the open phase.
    let mut observer = server.connect().await;

    // A second client attaches and changes the tree, which the server broadcasts to everyone.
    let mut owner = server.connect().await;
    write_frame(
        &mut owner,
        &ClientRequest::Attach {
            session: "broadcast-before-attach".to_owned(),
            hello: Default::default(),
        },
    )
    .await
    .expect("attach the owner connection");
    read_initial_state(&mut owner, WAIT).await;
    write_frame(
        &mut owner,
        &ClientRequest::NewPane {
            request_id: 1,
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command("/bin/cat".to_owned()),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("request one new pane");

    let received = expect_event(&mut observer, WAIT, |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    let ServerEvent::TreeSnapshot(tree) = received else {
        unreachable!("the predicate accepts only tree snapshots")
    };
    assert_eq!(tree.project_ids().len(), 1);
    let project = tree.project_ids()[0];
    let groups = tree.children_of(project).expect("project children");
    assert_eq!(groups.len(), 1);
    let panes = tree.children_of(groups[0]).expect("group children");
    assert_eq!(panes.len(), 1);
}

/// REQ: PORT:ilium-server/src/ipc/connection.rs::broadcast_before_attach_is_forwarded
#[tokio::test]
async fn port_008_later_tree_changes_keep_reaching_a_connection_that_never_attached() {
    let server = TestServer::start("broadcast-before-attach-later").await;

    let mut observer = server.connect().await;

    let mut owner = server.connect().await;
    write_frame(
        &mut owner,
        &ClientRequest::Attach {
            session: "broadcast-before-attach-later".to_owned(),
            hello: Default::default(),
        },
    )
    .await
    .expect("attach the owner connection");
    read_initial_state(&mut owner, WAIT).await;

    for request_id in 1..=2 {
        write_frame(
            &mut owner,
            &ClientRequest::NewPane {
                request_id,
                parent_group: ROOT_ID,
                kind: NewPaneKind::Command("/bin/cat".to_owned()),
                working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
            },
        )
        .await
        .expect("request a new pane");
        let received = expect_event(&mut observer, WAIT, |event| {
            matches!(event, ServerEvent::TreeSnapshot(_))
        })
        .await;
        let ServerEvent::TreeSnapshot(tree) = received else {
            unreachable!("the predicate accepts only tree snapshots")
        };
        let project = tree.project_ids()[0];
        let groups = tree.children_of(project).expect("project children");
        let panes = tree.children_of(groups[0]).expect("group children");
        assert_eq!(panes.len(), request_id as usize);
    }
}
