//! Public-API unit gap tests for `ilium-pty`. Every test drives a real pty
//! and a trivial `/bin/sh` child, because the behaviour under test (writer
//! ordering, journal retention, child reaping, mouse-mode encoding) exists
//! only once bytes really cross a pty. Chunk-sensitive assertions rely on
//! `sleep` gaps between `printf` calls so each write is one PTY read.

#![cfg(unix)]

use std::future::Future;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ilium_execution::{QuotaGroup, QuotaLimits};
use ilium_pty::{
    screen_text_from_replay, DeliveryFailure, OwnerLimits, OwnerStatus, PtyCommand,
    PtyOutputRecovery, PtyOutputRecoveryEstimate, PtySession, ScreenSnapshot, ShutdownReason,
    PTY_WORKERS_PER_SESSION, PTY_WORKER_STACK_BYTES,
};
use tokio::sync::watch;
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

const WAIT: Duration = Duration::from_secs(15);
const HALF_MIB: usize = 512 * 1024;
const FLOOD_BYTES: usize = 40_000_000;
const THREE_CHUNKS: &str = "printf AAAA; sleep 0.3; printf BBBB; sleep 0.3; printf CCCC; sleep 30";
const SGR_MOUSE_MODES: &str = r"\033[?1002h\033[?1006h";
const DEVICE_ATTRIBUTES_PROBE: &str = concat!(
    r#"stty raw -echo min 0 time 3; printf '\033[c'; "#,
    r#"c=$(dd bs=1 count=1 2>/dev/null | wc -c); "#,
    r#"printf 'got=%s END' "$c"; sleep 30"#,
);

fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a current-thread runtime for the test")
        .block_on(future)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn shell(script: &str) -> PtyCommand {
    PtyCommand::new("/bin/sh", std::env::temp_dir(), 24, 80)
        .arg("-c")
        .arg(script)
}

fn spawn_shell(script: &str) -> PtySession {
    PtySession::spawn(shell(script)).expect("spawn the test shell")
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("ilium-pty-unit-gaps-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the scratch directory");
    dir
}

fn wait_for_output(session: &PtySession, expected: &[u8]) {
    let arrived = wait_until(|| session.output_replay().bytes == expected, WAIT);
    assert!(
        arrived,
        "journal never reached {:?}; it holds {:?}",
        String::from_utf8_lossy(expected),
        String::from_utf8_lossy(&session.output_replay().bytes)
    );
}

/// Three PTY reads, so three journal chunks with sequences 1, 2 and 3.
fn spawn_three_chunks() -> PtySession {
    let session = spawn_shell(THREE_CHUNKS);
    wait_for_output(&session, b"AAAABBBBCCCC");
    let estimate = session.output_replay_estimate();
    assert_eq!(
        estimate.through_sequence, 3,
        "three reads make three chunks"
    );
    assert_eq!(estimate.first_sequence, Some(1));
    session
}

/// Prints `A`, then floods more than the 32 MiB journal cap so the first
/// chunk is evicted. The gap after `A` lets a test capture an estimate first.
fn flood_script() -> String {
    format!("printf A; sleep 1; head -c {FLOOD_BYTES} /dev/zero; sleep 30")
}

fn wait_for_eviction(session: &PtySession) {
    let evicted = wait_until(
        || !session.output_replay_estimate().is_complete,
        Duration::from_secs(120),
    );
    assert!(evicted, "the flood never exceeded the retention cap");
}

/// The sequence and bytes a recovery delivers, whichever variant carries them.
fn recovery_parts(recovery: Option<PtyOutputRecovery>) -> Option<(u64, Vec<u8>)> {
    match recovery? {
        PtyOutputRecovery::Delta(chunk) => Some((chunk.sequence, chunk.bytes.to_vec())),
        PtyOutputRecovery::Replay(replay) => Some((replay.through_sequence, replay.bytes)),
    }
}

fn snapshot_containing(session: &PtySession, needle: &str) -> ScreenSnapshot {
    let mut found = None;
    let arrived = wait_until(
        || {
            found = session
                .try_screen_snapshot()
                .filter(|snapshot| snapshot.text.contains(needle));
            found.is_some()
        },
        WAIT,
    );
    assert!(arrived, "no free-parser snapshot contained {needle:?}");
    found.expect("a matching snapshot was just observed")
}

fn fits(session: &PtySession, limit: usize) -> bool {
    session.screen_snapshot_with_limit(limit).is_ok()
}

fn resize_epoch(session: &PtySession) -> u64 {
    let reader = session.current_screen_reader();
    let mut epoch = None;
    let read = wait_until(
        || {
            epoch = reader.try_with_screen_and_resize_epoch(|_, epoch_value| epoch_value);
            epoch.is_some()
        },
        WAIT,
    );
    assert!(
        read,
        "the parser stayed busy while reading the resize epoch"
    );
    epoch.expect("the epoch was read")
}

async fn wait_for_stopped(mut receiver: watch::Receiver<OwnerStatus>) -> OwnerStatus {
    loop {
        let current = receiver.borrow_and_update().clone();
        if matches!(current, OwnerStatus::Stopped { .. }) {
            return current;
        }
        receiver
            .changed()
            .await
            .expect("the owner keeps its status sender alive");
    }
}

fn mouse_event(kind: MouseEventKind) -> MouseEvent {
    MouseEvent {
        kind,
        column: 4,
        row: 2,
        modifiers: KeyModifiers::NONE,
    }
}

/// A child that requests `modes`, reads exactly `read_count` raw bytes and
/// prints them as lowercase hex, then prints `END`. Raw mode turns echo off,
/// so the hex dump is the only way the test can see what the child received.
fn mouse_child(modes: &str, read_count: usize) -> PtySession {
    spawn_shell(&format!(
        "stty raw -echo; printf '{modes}'; \
         dd bs=1 count={read_count} 2>/dev/null | od -An -tx1 | tr -d ' \\n'; \
         printf END; sleep 30"
    ))
}

fn wait_for_received_hex(session: &PtySession, expected_hex: &str) {
    wait_for_output_containing(session, format!("{expected_hex}END").as_bytes());
}

fn wait_for_mouse_mode(
    session: &PtySession,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) {
    let ready = wait_until(
        || {
            session.with_screen(|screen| {
                screen.mouse_protocol_mode() == mode && screen.mouse_protocol_encoding() == encoding
            })
        },
        WAIT,
    );
    assert!(ready, "the child never enabled mouse mode {mode:?}");
}

fn quota_for_sessions(sessions: usize) -> QuotaGroup {
    let worker_threads = sessions * PTY_WORKERS_PER_SESSION;
    QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads,
        worker_bytes: worker_threads * PTY_WORKER_STACK_BYTES,
    })
}

// ---------------------------------------------------------------------------
// DeliveryReceipt::cancel_and_wait
// ---------------------------------------------------------------------------

#[test]
fn cancel_and_wait_after_completion_returns_the_stored_result() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let receipt = input.write(b"x").expect("admit a one-byte write");
    let observer = receipt.observer();
    let stored = block_on(observer.wait());
    assert!(stored.is_ok(), "a one-byte write should be delivered");

    let again = block_on(receipt.cancel_and_wait());
    assert_eq!(
        again, stored,
        "cancelling a finished receipt must not change its result"
    );
}

#[test]
fn cancel_and_wait_on_a_queued_write_reports_cancelled() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let _blocking = input
        .write(&vec![b'x'; HALF_MIB])
        .expect("admit the blocking write");
    let queued = input.write(b"y").expect("admit the queued write");

    let result =
        block_on(async move { tokio::time::timeout(WAIT, queued.cancel_and_wait()).await })
            .expect("cancelling a queued write completes within the wait bound");
    let error = result.expect_err("a cancelled queued write must not report success");
    assert_eq!(
        error.failure,
        DeliveryFailure::Cancelled,
        "a write cancelled before it started delivers nothing"
    );
    assert!(error.proves_zero_delivery());
}

#[test]
fn cancel_racing_an_in_progress_write_reports_accurate_partial_delivery() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let blocking = input
        .write(&vec![b'x'; HALF_MIB])
        .expect("admit the large write");

    let result =
        block_on(async move { tokio::time::timeout(WAIT, blocking.cancel_and_wait()).await })
            .expect("cancelling the large write completes within the wait bound");
    match result {
        Ok(delivery) => assert_eq!(delivery.bytes_written, HALF_MIB),
        Err(error) => match error.failure {
            DeliveryFailure::Cancelled => {}
            DeliveryFailure::PartialWrite {
                requested, written, ..
            } => {
                assert_eq!(requested, HALF_MIB);
                assert!(
                    written < requested,
                    "a partial write must report fewer bytes than requested"
                );
            }
            DeliveryFailure::UnconfirmedWrite { requested, .. } => {
                assert_eq!(requested, HALF_MIB);
            }
            other => panic!("unexpected failure for a cancelled large write: {other:?}"),
        },
    }
}

#[test]
fn waiting_twice_on_the_same_completion_gives_the_same_result() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let receipt = input.write(b"x").expect("admit a one-byte write");
    let observer = receipt.observer();

    let first = block_on(observer.wait());
    let second = block_on(observer.wait());
    assert_eq!(
        first, second,
        "a second wait must return the same completion"
    );
    assert_eq!(
        observer.result(),
        Some(first),
        "result() must agree with wait()"
    );
}

// ---------------------------------------------------------------------------
// PtyInput::same_session
// ---------------------------------------------------------------------------

#[test]
fn same_session_is_true_for_two_handles_from_one_owner() {
    let session = spawn_shell("sleep 30");
    let first = session.input_handle();
    let second = session.input_handle();
    assert!(first.same_session(&second));
}

#[test]
fn same_session_is_false_for_handles_from_different_sessions() {
    let first_session = spawn_shell("sleep 30");
    let second_session = spawn_shell("sleep 30");
    let first = first_session.input_handle();
    let second = second_session.input_handle();
    assert!(!first.same_session(&second));
}

#[test]
fn same_session_is_true_for_a_clone_of_a_handle() {
    let session = spawn_shell("sleep 30");
    let handle = session.input_handle();
    let clone = handle.clone();
    assert!(handle.same_session(&clone));
}

#[test]
fn same_session_still_compares_identity_after_owner_shutdown() {
    let session = spawn_shell("sleep 30");
    let other_session = spawn_shell("sleep 30");
    let handle = session.input_handle();
    let peer = session.input_handle();
    session.request_shutdown();
    let stopped = block_on(async move {
        tokio::time::timeout(WAIT, wait_for_stopped(handle.subscribe_status())).await
    });
    assert!(
        stopped.is_ok(),
        "the owner should stop after request_shutdown"
    );

    let handle = session.input_handle();
    assert!(
        handle.same_session(&peer),
        "identity survives owner shutdown"
    );
    assert!(!handle.same_session(&other_session.input_handle()));
}

// ---------------------------------------------------------------------------
// PtyInput::subscribe_status
// ---------------------------------------------------------------------------

#[test]
fn subscribe_status_initial_value_equals_status_at_subscribe_time() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let receiver = input.subscribe_status();
    assert_eq!(*receiver.borrow(), input.status());
    assert_eq!(input.status(), OwnerStatus::Running);
}

#[test]
fn subscribe_status_observes_the_transition_after_request_shutdown() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let receiver = input.subscribe_status();
    session.request_shutdown();
    let status =
        block_on(async move { tokio::time::timeout(WAIT, wait_for_stopped(receiver)).await })
            .expect("the owner reports a stopped status within the wait bound");
    assert!(
        matches!(
            status,
            OwnerStatus::Stopped {
                reason: ShutdownReason::Requested,
                ..
            }
        ),
        "expected a requested stop, got {status:?}"
    );
}

#[test]
fn late_status_subscriber_sees_the_final_status_without_waiting() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    session.request_shutdown();
    let early = input.subscribe_status();
    let stopped =
        block_on(async move { tokio::time::timeout(WAIT, wait_for_stopped(early)).await });
    assert!(
        stopped.is_ok(),
        "the owner should stop after request_shutdown"
    );

    let late = input.subscribe_status();
    assert!(
        matches!(*late.borrow(), OwnerStatus::Stopped { .. }),
        "a late subscriber must see the final status immediately"
    );
}

#[test]
fn dropping_a_status_receiver_does_not_affect_the_owner() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let receiver = input.subscribe_status();
    drop(receiver);
    assert_eq!(input.status(), OwnerStatus::Running);
    let receipt = input.write(b"x").expect("the owner still admits input");
    assert!(receipt.wait_blocking().is_ok());
}

// ---------------------------------------------------------------------------
// PtyInput::queue_load and the owner command queue (Queue::submit)
// ---------------------------------------------------------------------------

#[test]
fn queue_load_counts_a_write_until_its_result_is_final() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let receipt = input
        .write(&vec![b'x'; HALF_MIB])
        .expect("admit a half-MiB write");
    let load = input.queue_load();
    assert_eq!(load.commands, 1);
    assert_eq!(load.command_bytes, HALF_MIB);

    let _outcome =
        block_on(async move { tokio::time::timeout(WAIT, receipt.cancel_and_wait()).await });
    let drained = wait_until(
        || {
            let load = input.queue_load();
            load.commands == 0 && load.command_bytes == 0
        },
        WAIT,
    );
    assert!(drained, "the finished write must release its queue charge");
}

#[test]
fn submit_when_the_command_queue_is_full_returns_overloaded() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let mut receipts = vec![input
        .write(&vec![b'x'; HALF_MIB])
        .expect("admit the blocking write")];
    let mut rejection = None;
    for _ in 0..200 {
        match input.write(b"y") {
            Ok(receipt) => receipts.push(receipt),
            Err(error) => {
                rejection = Some(error);
                break;
            }
        }
    }
    let error = rejection.expect("the command queue must refuse writes once full");
    let limit = OwnerLimits::default().commands;
    match error.failure {
        DeliveryFailure::Overloaded {
            outstanding_commands,
            ..
        } => assert_eq!(
            outstanding_commands, limit,
            "backpressure at the command limit"
        ),
        other => panic!("expected Overloaded backpressure, got {other:?}"),
    }
}

#[test]
fn submit_after_shutdown_returns_the_shutdown_reason() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    session.request_shutdown();
    let stopped = block_on(async {
        tokio::time::timeout(WAIT, wait_for_stopped(input.subscribe_status())).await
    });
    assert!(
        stopped.is_ok(),
        "the owner should stop after request_shutdown"
    );

    let error = input
        .write(b"x")
        .expect_err("a closed queue must refuse new writes");
    assert!(
        matches!(error.failure, DeliveryFailure::Shutdown(_)),
        "expected a shutdown reason, got {:?}",
        error.failure
    );
    assert_eq!(
        error.operation_id, None,
        "rejected before admission has no id"
    );
}

#[test]
fn shutdown_leaves_no_queued_item_charged() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let _blocking = input
        .write(&vec![b'x'; HALF_MIB])
        .expect("admit the blocking write");
    let queued: Vec<_> = (0..8)
        .map(|_| input.write(b"y").expect("admit a queued write"))
        .collect();

    session.request_shutdown();
    let drained = wait_until(
        || {
            let load = input.queue_load();
            load.commands == 0 && load.command_bytes == 0
        },
        WAIT,
    );
    assert!(drained, "shutdown must release every queued charge");
    for receipt in queued {
        let result = block_on(async move { tokio::time::timeout(WAIT, receipt.wait()).await })
            .expect("each queued write finishes after shutdown");
        let error = result.expect_err("no queued write may succeed after shutdown");
        assert!(error.proves_zero_delivery(), "got {:?}", error.failure);
    }
}

#[test]
fn queued_write_past_its_command_timeout_fails_with_timeout() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let _blocking = input
        .write(&vec![b'x'; HALF_MIB])
        .expect("admit the blocking write");
    let queued = input.write(b"y").expect("admit the queued write");

    let result = block_on(async move { tokio::time::timeout(WAIT, queued.wait()).await })
        .expect("the expiry worker finishes the queued write");
    let error = result.expect_err("a write queued behind a blocked write must not succeed");
    assert_eq!(error.failure, DeliveryFailure::Timeout);
    assert!(error.proves_zero_delivery());
}

// ---------------------------------------------------------------------------
// PtyInput::write_mouse_input
// ---------------------------------------------------------------------------

#[test]
fn write_mouse_input_encodes_a_press_in_sgr_when_the_child_requested_it() {
    let session = mouse_child(SGR_MOUSE_MODES, 9);
    wait_for_mouse_mode(
        &session,
        MouseProtocolMode::ButtonMotion,
        MouseProtocolEncoding::Sgr,
    );
    let input = session.input_handle();
    let delivery = input
        .write_mouse_input(mouse_event(MouseEventKind::Down(MouseButton::Left)), 4, 2)
        .expect("admit the press")
        .wait_blocking()
        .expect("deliver the press");
    assert_eq!(
        delivery.bytes_written, 9,
        "receipt reports the written byte count"
    );
    wait_for_received_hex(&session, "1b5b3c303b353b334d");
}

#[test]
fn write_mouse_input_encodes_a_release_with_a_lowercase_m() {
    let session = mouse_child(SGR_MOUSE_MODES, 9);
    wait_for_mouse_mode(
        &session,
        MouseProtocolMode::ButtonMotion,
        MouseProtocolEncoding::Sgr,
    );
    let input = session.input_handle();
    input
        .write_mouse_input(mouse_event(MouseEventKind::Up(MouseButton::Left)), 4, 2)
        .expect("admit the release")
        .wait_blocking()
        .expect("deliver the release");
    wait_for_received_hex(&session, "1b5b3c303b353b336d");
}

#[test]
fn write_mouse_input_encodes_a_drag_when_button_motion_is_enabled() {
    let session = mouse_child(SGR_MOUSE_MODES, 10);
    wait_for_mouse_mode(
        &session,
        MouseProtocolMode::ButtonMotion,
        MouseProtocolEncoding::Sgr,
    );
    let input = session.input_handle();
    let delivery = input
        .write_mouse_input(mouse_event(MouseEventKind::Drag(MouseButton::Left)), 4, 2)
        .expect("admit the drag")
        .wait_blocking()
        .expect("deliver the drag");
    assert_eq!(delivery.bytes_written, 10);
    wait_for_received_hex(&session, "1b5b3c33323b353b334d");
}

#[test]
fn write_mouse_input_ignores_a_drag_outside_button_motion_mode() {
    let session = spawn_shell("printf '\\033[?1000h'; sleep 30");
    wait_for_mouse_mode(
        &session,
        MouseProtocolMode::PressRelease,
        MouseProtocolEncoding::Default,
    );
    let input = session.input_handle();
    let delivery = input
        .write_mouse_input(mouse_event(MouseEventKind::Drag(MouseButton::Left)), 4, 2)
        .expect("admit the drag")
        .wait_blocking()
        .expect("a declined drag still completes");
    assert_eq!(
        delivery.bytes_written, 0,
        "press-only mode does not encode drags"
    );
}

#[test]
fn write_mouse_input_without_a_mouse_mode_is_accepted_with_zero_bytes() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    let delivery = input
        .write_mouse_input(mouse_event(MouseEventKind::Down(MouseButton::Left)), 4, 2)
        .expect("admit the click")
        .wait_blocking()
        .expect("a click with no mouse mode completes");
    assert_eq!(
        delivery.bytes_written, 0,
        "ordinary shell clicks are left alone"
    );
}

#[test]
fn write_mouse_input_after_owner_exit_reports_a_shutdown_reason() {
    let session = spawn_shell("sleep 30");
    let input = session.input_handle();
    session.request_shutdown();
    let stopped = block_on(async {
        tokio::time::timeout(WAIT, wait_for_stopped(input.subscribe_status())).await
    });
    assert!(
        stopped.is_ok(),
        "the owner should stop after request_shutdown"
    );

    let error = input
        .write_mouse_input(mouse_event(MouseEventKind::Down(MouseButton::Left)), 4, 2)
        .expect_err("a stopped owner must refuse mouse input");
    assert!(
        matches!(error.failure, DeliveryFailure::Shutdown(_)),
        "got {:?}",
        error.failure
    );
}

// ---------------------------------------------------------------------------
// PtyTerminationHandle
// ---------------------------------------------------------------------------

#[test]
fn kill_direct_child_kills_a_live_child() {
    let session = spawn_shell("sleep 30");
    let handle = session.termination_handle();
    handle
        .kill_direct_child()
        .expect("killing a live direct child succeeds");
    assert!(
        wait_until(|| session.child_exit().is_some(), WAIT),
        "the killed child must be observed as exited"
    );
}

#[test]
fn kill_direct_child_after_exit_returns_ok_without_panicking() {
    let session = spawn_shell("exit 0");
    assert!(wait_until(|| session.child_exit().is_some(), WAIT));
    let handle = session.termination_handle();
    handle
        .kill_direct_child()
        .expect("an exited child is a no-op, not an error");
    handle
        .kill_direct_child()
        .expect("a repeated kill after exit is still a no-op");
}

#[test]
fn termination_handles_from_one_session_refer_to_the_same_session() {
    let session = spawn_shell("sleep 30");
    let other_session = spawn_shell("sleep 30");
    let first = session.termination_handle();
    let second = session.termination_handle();
    assert!(first.same_session(&second));
    assert!(!first.same_session(&other_session.termination_handle()));
}

#[test]
fn terminate_process_tree_via_the_handle_stops_the_child() {
    let session = spawn_shell("sleep 60");
    let handle = session.termination_handle();
    let result = handle.terminate_process_tree(Duration::from_secs(10));
    assert!(
        result.is_ok(),
        "tree termination should succeed: {:?}",
        result.err()
    );
    assert!(
        wait_until(|| session.child_exit().is_some(), WAIT),
        "the tree's direct child must be reaped after termination"
    );
}

// ---------------------------------------------------------------------------
// PtySession::current_working_directory
// ---------------------------------------------------------------------------

#[test]
fn current_working_directory_is_the_spawn_directory_for_a_fresh_shell() {
    let dir = scratch_dir("spawn-cwd");
    let expected = std::fs::canonicalize(&dir).expect("canonicalize the scratch directory");
    let command = PtyCommand::new("/bin/sh", dir.clone(), 24, 80)
        .arg("-c")
        .arg("sleep 30");
    let session = PtySession::spawn(command).expect("spawn the shell in the scratch directory");
    let observed = wait_until(
        || session.current_working_directory().as_deref() == Some(expected.as_path()),
        WAIT,
    );
    assert!(
        observed,
        "cwd was {:?}, expected {:?}",
        session.current_working_directory(),
        expected
    );
    drop(session);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn current_working_directory_is_none_once_the_process_is_gone() {
    let session = spawn_shell("exit 0");
    assert!(wait_until(|| session.child_exit().is_some(), WAIT));
    assert_eq!(session.current_working_directory(), None);
}

#[test]
fn current_working_directory_follows_a_shell_cd() {
    let target_dir = scratch_dir("cwd-target");
    let expected = std::fs::canonicalize(&target_dir).expect("canonicalize the target");
    let session = spawn_shell(&format!("cd {} && sleep 30", expected.display()));
    let observed = wait_until(
        || session.current_working_directory().as_deref() == Some(expected.as_path()),
        WAIT,
    );
    assert!(observed, "the shell's cd must move the reported cwd");
    drop(session);
    let _ = std::fs::remove_dir_all(&target_dir);
}

// ---------------------------------------------------------------------------
// PtySession::try_screen_snapshot, screen_generation, snapshot limits
// ---------------------------------------------------------------------------

#[test]
fn try_screen_snapshot_returns_a_frame_when_the_parser_is_free() {
    let session = spawn_shell("printf hello; sleep 30");
    let snapshot = snapshot_containing(&session, "hello");
    assert!(snapshot.text.contains("hello"));
}

#[test]
fn try_screen_snapshot_matches_screen_text_for_the_same_generation() {
    let session = spawn_shell("printf hello; sleep 30");
    let snapshot = snapshot_containing(&session, "hello");
    assert_eq!(snapshot.text, session.screen_text());
    assert_eq!(snapshot.generation, session.screen_generation());
}

#[test]
fn snapshot_after_resize_reflects_the_new_dimensions() {
    let session = spawn_shell("sleep 0.5; stty size; sleep 30");
    session.resize(7, 33).expect("resize the pty");
    let snapshot = snapshot_containing(&session, "7 33");
    assert!(
        snapshot.text.lines().count() <= 7,
        "a 7-row screen cannot show more than 7 lines: {:?}",
        snapshot.text
    );
}

#[test]
fn screen_generation_increases_after_every_output_chunk() {
    let session = spawn_shell(THREE_CHUNKS);
    let mut generations = Vec::new();
    for needle in ["AAAA", "BBBB", "CCCC"] {
        let arrived = wait_until(|| session.screen_text().contains(needle), WAIT);
        assert!(arrived, "screen never showed {needle}");
        generations.push(session.screen_generation());
    }
    assert!(
        generations[0] < generations[1] && generations[1] < generations[2],
        "generations must increase per chunk: {generations:?}"
    );
}

#[test]
fn screen_snapshot_with_a_limit_below_the_frame_is_an_other_error() {
    let session = spawn_shell("printf frame; sleep 30");
    let error = match session.screen_snapshot_with_limit(1) {
        Ok(_) => panic!("a one-byte limit cannot hold a 24x80 frame"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), ErrorKind::Other);
}

#[test]
fn screen_snapshot_limit_boundary_is_exactly_the_frame_size() {
    let session = spawn_shell("printf frame; sleep 30");
    let _ = snapshot_containing(&session, "frame");
    let mut low = 0usize;
    let mut high = 1usize << 20;
    assert!(
        fits(&session, high),
        "a 1 MiB limit must hold a 24x80 frame"
    );
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if fits(&session, middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    assert!(fits(&session, high), "the exact frame size must succeed");
    assert!(
        !fits(&session, high - 1),
        "one byte under the frame size must fail"
    );
}

// ---------------------------------------------------------------------------
// PtySession::shell_observer
// ---------------------------------------------------------------------------

#[test]
fn shell_observer_identity_matches_only_its_own_session() {
    let first = spawn_shell("sleep 30");
    let second = spawn_shell("sleep 30");
    let observer = first.shell_observer();
    assert!(*observer.identity() == first.identity());
    assert!(*observer.identity() != second.identity());
}

#[test]
fn shell_owns_terminal_reports_true_once_the_shell_is_foreground() {
    let session = spawn_shell("sleep 30");
    let observer = session.shell_observer();
    let owns = wait_until(|| observer.shell_owns_terminal() == Some(true), WAIT);
    assert!(
        owns,
        "the shell started the pty and should own its foreground group"
    );
}

#[test]
fn shell_observer_outliving_the_session_does_not_claim_the_terminal() {
    let session = spawn_shell("sleep 30");
    let observer = session.shell_observer();
    drop(session);
    let released = wait_until(|| observer.shell_owns_terminal() != Some(true), WAIT);
    assert!(
        released,
        "a dropped session's shell must not still own the terminal"
    );
}

// ---------------------------------------------------------------------------
// PtySession output journal: history, replay and recovery
// ---------------------------------------------------------------------------

#[test]
fn output_history_before_is_empty_for_a_session_without_output() {
    let session = spawn_shell("sleep 30");
    let history = session.output_history_before(1, 1024);
    assert!(history.bytes.is_empty());
}

#[test]
fn output_history_before_returns_newest_chunks_within_the_byte_limit() {
    let session = spawn_three_chunks();
    let exact = session.output_history_before(4, 8);
    assert_eq!(
        exact.bytes,
        b"BBBBCCCC".to_vec(),
        "exactly the limit still fits"
    );
    assert_eq!(exact.first_sequence, 2);

    let short = session.output_history_before(4, 7);
    assert_eq!(
        short.bytes,
        b"CCCC".to_vec(),
        "one byte short drops the older chunk"
    );
    assert_eq!(short.first_sequence, 3);
}

#[test]
fn output_history_before_beyond_the_newest_sequence_returns_every_chunk() {
    let session = spawn_three_chunks();
    let history = session.output_history_before(u64::MAX, 1 << 20);
    assert_eq!(history.bytes, b"AAAABBBBCCCC".to_vec());
    assert_eq!(history.first_sequence, 1);
    assert!(history.reaches_journal_start);
}

#[test]
fn output_retention_window_boundaries_are_reported_as_truncation() {
    let session = spawn_shell(&flood_script());
    wait_for_output(&session, b"A");
    wait_for_eviction(&session);
    let estimate = session.output_replay_estimate();
    let first = estimate
        .first_sequence
        .expect("the flood leaves retained output");
    assert!(first > 2, "eviction must drop the oldest chunks");

    let history = session.output_history_before(1, 1 << 20);
    assert!(
        history.bytes.is_empty(),
        "nothing older than the window survives"
    );
    assert_eq!(history.first_sequence, 1);
    assert!(!history.reaches_journal_start);

    assert!(
        matches!(
            session.output_recovery_after(first - 1),
            Some(PtyOutputRecovery::Delta(_))
        ),
        "a consumer at the window's first retained chunk receives a contiguous delta"
    );

    let recovery = session.output_recovery_after(first - 2);
    assert!(
        matches!(
            &recovery,
            Some(PtyOutputRecovery::Replay(replay))
                if !replay.is_complete && replay.bytes.starts_with(b"\x1bc")
        ),
        "a consumer older than the window needs a reset-prefixed replay"
    );

    assert!(matches!(
        session.output_recovery_estimate_after(first - 2),
        Some(PtyOutputRecoveryEstimate::Replay(_))
    ));
}

#[test]
fn output_replay_through_if_retained_declines_an_evicted_prefix() {
    let session = spawn_shell(&flood_script());
    wait_for_output(&session, b"A");
    let estimate = session.output_replay_estimate();
    assert_eq!(estimate.first_sequence, Some(1));
    wait_for_eviction(&session);
    assert!(
        session
            .output_replay_through_if_retained(estimate)
            .is_none(),
        "an evicted prefix must not be replayed partially"
    );
    assert!(
        session.output_replay_if_unchanged(estimate).is_none(),
        "the estimate no longer matches the journal"
    );
}

#[test]
fn output_recovery_prefix_through_stops_exactly_at_the_requested_sequence() {
    let session = spawn_three_chunks();
    assert_eq!(
        recovery_parts(session.output_recovery_prefix_through(0, 2, 1 << 20)),
        Some((2, b"AAAABBBB".to_vec()))
    );
}

#[test]
fn output_recovery_prefix_through_rejects_a_range_ending_before_its_start() {
    let session = spawn_three_chunks();
    assert!(session
        .output_recovery_prefix_through(2, 1, 1 << 20)
        .is_none());
}

#[test]
fn output_recovery_prefix_after_the_newest_sequence_is_none() {
    let session = spawn_three_chunks();
    assert!(session.output_recovery_prefix_after(3, 1 << 20).is_none());
}

#[test]
fn output_recovery_estimate_matches_the_bytes_returned_by_recovery_after() {
    let session = spawn_three_chunks();
    let estimate = session.output_recovery_estimate_after(1);
    let Some(PtyOutputRecoveryEstimate::Delta {
        through_sequence,
        byte_len,
    }) = estimate
    else {
        panic!("expected a contiguous delta estimate, got {estimate:?}");
    };
    let recovery = recovery_parts(session.output_recovery_after(1));
    let (sequence, bytes) = recovery.expect("a retained tail is recoverable");
    assert_eq!(sequence, through_sequence);
    assert_eq!(bytes.len(), byte_len);
    assert_eq!(bytes, b"BBBBCCCC".to_vec());
}

#[test]
fn output_recovery_if_unchanged_returns_the_estimated_delta() {
    let session = spawn_three_chunks();
    let recovery = session.output_recovery_if_unchanged(
        1,
        PtyOutputRecoveryEstimate::Delta {
            through_sequence: 3,
            byte_len: 8,
        },
    );
    assert_eq!(
        recovery_parts(recovery),
        Some((3, b"BBBBCCCC".to_vec())),
        "a matching estimate copies the delta"
    );
}

#[test]
fn output_recovery_if_unchanged_declines_a_stale_byte_count() {
    let session = spawn_three_chunks();
    assert!(session
        .output_recovery_if_unchanged(
            1,
            PtyOutputRecoveryEstimate::Delta {
                through_sequence: 3,
                byte_len: 99,
            },
        )
        .is_none());
}

#[test]
fn output_recovery_after_a_future_sequence_is_none() {
    let session = spawn_three_chunks();
    assert!(session.output_recovery_after(3).is_none());
    assert!(session.output_recovery_estimate_after(3).is_none());
}

#[test]
fn output_replay_if_unchanged_on_an_empty_history_returns_an_empty_replay() {
    let session = spawn_shell("sleep 30");
    let estimate = session.output_replay_estimate();
    assert_eq!(estimate.through_sequence, 0);
    assert_eq!(estimate.first_sequence, None);
    let replay = session
        .output_replay_if_unchanged(estimate)
        .expect("an unchanged empty journal replays");
    assert!(replay.bytes.is_empty());
    assert!(replay.is_complete);
}

#[test]
fn output_replay_if_unchanged_declines_after_new_output() {
    let session = spawn_shell("printf A; sleep 0.5; printf B; sleep 30");
    wait_for_output(&session, b"A");
    let stale = session.output_replay_estimate();
    wait_for_output(&session, b"AB");
    assert!(
        session.output_replay_if_unchanged(stale).is_none(),
        "new output after the estimate must force a fresh estimate"
    );
    let fresh = session.output_replay_estimate();
    assert!(session.output_replay_if_unchanged(fresh).is_some());
}

#[test]
fn output_replay_through_if_retained_returns_the_estimated_prefix_after_newer_output() {
    let session = spawn_shell("printf A; sleep 0.5; printf B; sleep 30");
    wait_for_output(&session, b"A");
    let estimate = session.output_replay_estimate();
    wait_for_output(&session, b"AB");
    let replay = session
        .output_replay_through_if_retained(estimate)
        .expect("the estimated prefix is still retained");
    assert_eq!(replay.through_sequence, 1);
    assert_eq!(replay.bytes, b"A".to_vec());
}

// ---------------------------------------------------------------------------
// attach_snapshot (the public consumer of the crate-private snapshot_bytes)
// ---------------------------------------------------------------------------

#[test]
fn attach_snapshot_of_an_empty_screen_is_a_valid_replay() {
    let session = spawn_shell("sleep 30");
    let mut snapshot = None;
    let ready = wait_until(
        || {
            snapshot = session.attach_snapshot();
            snapshot.is_some()
        },
        WAIT,
    );
    assert!(ready, "an idle parser must produce an attach snapshot");
    let snapshot = snapshot.expect("the snapshot was just observed");
    assert!(snapshot.is_complete);
    assert_eq!(screen_text_from_replay(&snapshot.bytes, 24, 80), "");
}

#[test]
fn attach_snapshot_round_trips_wide_text_and_attributes() {
    let session = spawn_shell(r"printf '漢字 \033[1;31mred\033[0m\r\nsecond'; sleep 30");
    let arrived = wait_until(|| session.screen_text().contains("second"), WAIT);
    assert!(arrived, "the child's output never reached the screen");
    let mut snapshot = None;
    let ready = wait_until(
        || {
            snapshot = session.attach_snapshot();
            snapshot.is_some()
        },
        WAIT,
    );
    assert!(ready, "the attach snapshot must be available");
    let snapshot = snapshot.expect("the snapshot was just observed");

    assert_eq!(
        screen_text_from_replay(&snapshot.bytes, 24, 80),
        "漢字 red\nsecond"
    );
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(&snapshot.bytes);
    let screen = parser.screen();
    assert_eq!(
        screen.cell(0, 0).expect("the wide cell exists").contents(),
        "漢"
    );
    let red = screen.cell(0, 5).expect("the red cell exists");
    assert_eq!(red.contents(), "r");
    assert!(red.bold(), "the bold attribute must survive the round trip");
    assert_eq!(red.fgcolor(), vt100::Color::Idx(1));
}

// ---------------------------------------------------------------------------
// Resize epoch (ScreenReader), observed through the public resize call
// ---------------------------------------------------------------------------

#[test]
fn resize_advances_the_resize_epoch_by_one_per_call_even_at_the_same_size() {
    let session = spawn_shell("sleep 30");
    let before = resize_epoch(&session);
    session.resize(24, 80).expect("same-size resize");
    assert_eq!(resize_epoch(&session), before + 1);
    session.resize(30, 100).expect("resize to a new size");
    assert_eq!(resize_epoch(&session), before + 2);
}

#[test]
fn ordinary_output_does_not_advance_the_resize_epoch() {
    let session = spawn_shell("printf A; sleep 0.3; printf B; sleep 30");
    let before = resize_epoch(&session);
    let arrived = wait_until(|| session.screen_text().contains('B'), WAIT);
    assert!(arrived, "the output never arrived");
    assert_eq!(resize_epoch(&session), before);
}

#[test]
fn concurrent_resizes_from_two_threads_advance_once_per_call() {
    fn resize_many(input: ilium_pty::PtyInput) {
        for _ in 0..25 {
            input
                .resize(24, 80)
                .expect("admit the resize")
                .wait_blocking()
                .expect("apply the resize");
        }
    }

    let session = spawn_shell("sleep 30");
    let before = resize_epoch(&session);
    let input = session.input_handle();
    let first_handle = input.clone();
    let second_handle = input.clone();
    std::thread::scope(|scope| {
        scope.spawn(move || resize_many(first_handle));
        scope.spawn(move || resize_many(second_handle));
    });
    assert_eq!(resize_epoch(&session), before + 50);
}

// ---------------------------------------------------------------------------
// Terminal query replies (TerminalQueryResponder via PtyCommand)
// ---------------------------------------------------------------------------

#[test]
fn disabled_automatic_replies_send_no_device_attributes_answer() {
    let command = shell(DEVICE_ATTRIBUTES_PROBE).automatic_terminal_query_replies(false);
    let session = PtySession::spawn(command).expect("spawn the query probe");
    wait_for_output_containing(&session, b"got=0 END");
}

#[test]
fn default_automatic_replies_answer_device_attributes_queries() {
    let session = spawn_shell(DEVICE_ATTRIBUTES_PROBE);
    wait_for_output_containing(&session, b"got=1 END");
}

#[test]
fn explicitly_enabled_replies_match_the_default_behaviour() {
    let command = shell(DEVICE_ATTRIBUTES_PROBE).automatic_terminal_query_replies(true);
    let session = PtySession::spawn(command).expect("spawn the query probe");
    wait_for_output_containing(&session, b"got=1 END");
}

fn wait_for_output_containing(session: &PtySession, needle: &[u8]) {
    let arrived = wait_until(|| contains(&session.output_replay().bytes, needle), WAIT);
    assert!(
        arrived,
        "output never contained {:?}; it holds {:?}",
        String::from_utf8_lossy(needle),
        String::from_utf8_lossy(&session.output_replay().bytes)
    );
}

// ---------------------------------------------------------------------------
// Process quota admission (PtyOwnerReservations via spawn_with_quota)
// ---------------------------------------------------------------------------

#[test]
fn spawn_without_a_quota_admits_the_session() {
    let session = PtySession::spawn(shell("sleep 30"));
    assert!(session.is_ok(), "an unquota'd spawn must succeed");
}

#[test]
fn quota_charge_is_released_after_the_session_is_dropped() {
    let quota = quota_for_sessions(1);
    let session = PtySession::spawn_with_quota(shell("sleep 30"), quota.clone())
        .expect("the quota admits one PTY session");
    assert_eq!(quota.snapshot().worker_threads, PTY_WORKERS_PER_SESSION);
    drop(session);
    let released = wait_until(|| quota.snapshot().worker_threads == 0, WAIT);
    assert!(
        released,
        "dropping the session must release its worker charge"
    );
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn spawn_with_an_exhausted_quota_fails_without_a_partial_charge() {
    let worker_threads = PTY_WORKERS_PER_SESSION - 1;
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads,
        worker_bytes: worker_threads * PTY_WORKER_STACK_BYTES,
    });
    let result = PtySession::spawn_with_quota(shell("sleep 30"), quota.clone());
    assert!(
        result.is_err(),
        "one worker short of a full session must be refused"
    );
    let snapshot = quota.snapshot();
    assert_eq!(
        snapshot.worker_threads, 0,
        "no partial worker charge may remain"
    );
    assert_eq!(snapshot.worker_bytes, 0);
}

#[test]
fn concurrent_spawns_never_exceed_the_quota() {
    let quota = quota_for_sessions(3);
    let accepted: Mutex<Vec<PtySession>> = Mutex::new(Vec::new());
    let rejected = AtomicUsize::new(0);
    let clones: Vec<QuotaGroup> = (0..8).map(|_| quota.clone()).collect();
    std::thread::scope(|scope| {
        for clone in clones {
            let accepted = &accepted;
            let rejected = &rejected;
            scope.spawn(
                move || match PtySession::spawn_with_quota(shell("sleep 30"), clone) {
                    Ok(session) => accepted
                        .lock()
                        .expect("the accepted list stays unpoisoned")
                        .push(session),
                    Err(_) => {
                        rejected.fetch_add(1, Ordering::SeqCst);
                    }
                },
            );
        }
    });
    let accepted = accepted
        .into_inner()
        .expect("the accepted list stays unpoisoned");
    assert_eq!(accepted.len(), 3, "exactly the admitted sessions start");
    assert_eq!(rejected.load(Ordering::SeqCst), 5);
    assert_eq!(quota.snapshot().worker_threads, 3 * PTY_WORKERS_PER_SESSION);
    drop(accepted);
}
