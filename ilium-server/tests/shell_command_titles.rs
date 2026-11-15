//! End-to-end title test over the real UDS and real PTYs. The server owns
//! both the command tracker and authoritative tree. Naming a plain shell pane after
//! the command typed into it belongs to the titles plugin (it receives the line as
//! `user-line-submitted`), so only the kernel's own gate is tested here.
//! Runs on every platform. A typed command only becomes a title while the
//! shell itself owns the terminal, which is how the server tells "the user is
//! typing at a prompt" from "a running command owns the terminal". Unix
//! answers that with the terminal's foreground process group; ConPTY has none,
//! so Windows answers it by asking whether the shell has a live child. Both
//! sit behind `PtySession::shell_owns_terminal`, so this file tests the
//! behaviour rather than either mechanism.
//!
//! Every command typed here is one both `sh` and `cmd.exe` understand.

use std::time::Duration;

use ilium_core::ROOT_ID;
use ilium_ipc::{write_frame, ClientRequest, NewPaneKind, ServerEvent};

mod common;
use common::{expect_event, read_initial_state, TestServer};

/// A foreground command that is not a shell (so it must never receive an
/// automatic title) and that keeps reading stdin instead of exiting
/// immediately once spawned, so the typed marker below has a live pane to
/// land in. `cat` with no arguments blocks on stdin forever. Windows has no
/// `cat`; `findstr x` is the nearest stock equivalent that keeps reading
/// stdin instead of exiting immediately, mirroring
/// `ilium/tests/pty_tui_smoke.rs`'s `IDLE_PANE_ARGUMENTS`. It also becomes
/// the pane's default title (`TerminalOrigin::default_pane_name`), so the
/// command line and the title asserted against it cannot drift apart.
#[cfg(unix)]
const NON_SHELL_PROBE_COMMAND: &str = "cat";
#[cfg(windows)]
const NON_SHELL_PROBE_COMMAND: &str = "findstr x";

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
async fn foreground_non_shell_commands_do_not_receive_automatic_titles() {
    let server = TestServer::start("non-shell-title-test").await;
    let mut client = server.connect().await;

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            request_id: 0,
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(NON_SHELL_PROBE_COMMAND.to_string()),
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
            hello: Default::default(),
        },
    )
    .await
    .expect("request snapshot");
    let (tree, _) = read_initial_state(&mut client, Duration::from_secs(5)).await;
    assert_eq!(
        tree.get(pane_id).expect("pane exists").name,
        NON_SHELL_PROBE_COMMAND
    );

    stop(server, &mut client).await;
}
