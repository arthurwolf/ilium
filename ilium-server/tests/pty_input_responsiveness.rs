//! A blocked child must not monopolize the server executor or pane registry.
//! Uses isolated real PTYs and two IPC connections, never a real agent CLI.
#![cfg(unix)]

use std::time::{Duration, Instant};

use ilium_core::{NodeId, ROOT_ID};
use ilium_ipc::{read_frame, write_frame, ClientRequest, NewPaneKind, ServerEvent};
use ilium_test_fixtures::{install, FixtureBehavior};
use ilium_transport::SessionStream;

mod common;
use common::{expect_event, read_initial_state, TestServer};

async fn create_pane(client: &mut SessionStream, command: String, count: usize) -> NodeId {
    write_frame(
        client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(command),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let event = expect_event(
        client,
        Duration::from_secs(10),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == count),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!()
    };
    let project = tree.project_ids()[0];
    let group = tree.children_of(project).unwrap()[0];
    *tree.children_of(group).unwrap().last().unwrap()
}

/// Markers may span transport chunks. Only retain a small suffix, so even a
/// faulty child's output cannot make the diagnostic itself unbounded.
async fn wait_for_output(
    client: &mut SessionStream,
    pane_id: NodeId,
    marker: &[u8],
    timeout: Duration,
) -> bool {
    tokio::time::timeout(timeout, async {
        let mut suffix = Vec::new();
        loop {
            let Ok(event) = read_frame::<ServerEvent, _>(client).await else {
                return false;
            };
            if let ServerEvent::ScreenUpdate {
                pane_id: observed,
                bytes,
                ..
            } = event
            {
                if observed != pane_id {
                    continue;
                }
                suffix.extend_from_slice(&bytes);
                if suffix.windows(marker.len()).any(|window| window == marker) {
                    return true;
                }
                let discard = suffix.len().saturating_sub(marker.len());
                suffix.drain(..discard);
            }
        }
    })
    .await
    .unwrap_or(false)
}

// A current-thread runtime also proves that native PTY writes are not blocking
// the Tokio executor. A second executor thread could conceal that defect.
#[tokio::test(flavor = "current_thread")]
async fn stalled_input_does_not_freeze_a_second_panes_delivery() {
    let session_name = "pty-input-responsive";
    let mut server = TestServer::start(session_name).await;
    let mut stalled_client = server.connect().await;
    write_frame(
        &mut stalled_client,
        &ClientRequest::Attach {
            session: session_name.to_owned(),
        },
    )
    .await
    .unwrap();
    read_initial_state(&mut stalled_client, Duration::from_secs(10)).await;

    // The child exits independently after 15 seconds, so the original blocking
    // implementation has a finite failure path even before the fix is applied.
    let stalled_pane = create_pane(
        &mut stalled_client,
        "/bin/sh -c 'stty -echo -icanon; printf STALLED_READY; exec sleep 15'".to_owned(),
        1,
    )
    .await;
    assert!(
        wait_for_output(
            &mut stalled_client,
            stalled_pane,
            b"STALLED_READY",
            Duration::from_secs(10),
        )
        .await
    );

    let echo = install(
        &server.project_cwd,
        "responsive-echo",
        &FixtureBehavior::EchoSubmittedLine {
            prefix: "HEALTHY_DELIVERY:".to_owned(),
        },
    );
    let healthy_pane =
        create_pane(&mut stalled_client, format!("'{}'", echo.path.display()), 2).await;
    let mut healthy_client = server.connect().await;
    write_frame(
        &mut healthy_client,
        &ClientRequest::Attach {
            session: session_name.to_owned(),
        },
    )
    .await
    .unwrap();
    read_initial_state(&mut healthy_client, Duration::from_secs(10)).await;

    let started = Instant::now();
    write_frame(
        &mut stalled_client,
        &ClientRequest::KeyInput {
            pane_id: stalled_pane,
            bytes: vec![b'x'; 1024 * 1024],
            submission: None,
        },
    )
    .await
    .unwrap();
    // Let the first connection dispatch the large write before requesting
    // healthy input. The outer elapsed check also catches a blocked executor
    // that cannot wake this timer until the stalled child exits.
    tokio::time::sleep(Duration::from_millis(50)).await;
    write_frame(
        &mut healthy_client,
        &ClientRequest::KeyInput {
            pane_id: healthy_pane,
            bytes: b"ok\r".to_vec(),
            submission: None,
        },
    )
    .await
    .unwrap();
    let delivered = wait_for_output(
        &mut healthy_client,
        healthy_pane,
        b"HEALTHY_DELIVERY:ok",
        Duration::from_secs(1),
    )
    .await;
    let elapsed = started.elapsed();

    // Clean up all owned PTYs before asserting a responsiveness failure.
    write_frame(&mut healthy_client, &ClientRequest::KillSession)
        .await
        .unwrap();
    let shutdown = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
    assert!(shutdown.is_ok(), "owned server did not shut down promptly");
    assert!(shutdown.unwrap().unwrap().is_ok());
    assert!(
        delivered,
        "healthy pane received no echo during blocked input"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "healthy delivery waited {elapsed:?} for another pane's blocked input"
    );
}
