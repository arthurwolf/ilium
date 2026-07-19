//! Integration tests that spawn a real pty and a trivial real command
//! (`echo`, `cat`). This crate's whole job is talking to the OS, so faking
//! that away would test nothing real -- these are deliberately not unit
//! tests with a mocked pty.

use std::time::Duration;

use ilium_pty::{PtyCommand, PtySession};

/// Blocks the calling thread until `condition` returns `true` or `timeout`
/// elapses, polling every 20ms. Used instead of a fixed `sleep` because the
/// reader thread's parse latency is not deterministic under test-runner
/// load, and a fixed sleep would be either flaky (too short) or slow (long
/// enough to never flake).
fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn spawned_echo_output_appears_on_screen() {
    let command = PtyCommand::new("echo", std::env::temp_dir(), 24, 80).arg("hello from ilium-pty");
    let session = PtySession::spawn(command).expect("spawning echo should succeed");

    let saw_output = wait_until(
        || session.screen_text().contains("hello from ilium-pty"),
        Duration::from_secs(5),
    );
    assert!(
        saw_output,
        "expected echo's output on screen, got: {:?}",
        session.screen_text()
    );
}

#[test]
fn input_written_to_cat_is_echoed_back_on_screen() {
    let command = PtyCommand::new("cat", std::env::temp_dir(), 24, 80);
    let session = PtySession::spawn(command).expect("spawning cat should succeed");

    session
        .write(b"marker-from-test\n")
        .expect("writing input to the pty should succeed");

    let saw_echo = wait_until(
        || session.screen_text().contains("marker-from-test"),
        Duration::from_secs(5),
    );
    assert!(
        saw_echo,
        "expected cat to echo written input back on screen, got: {:?}",
        session.screen_text()
    );
}

#[test]
fn resize_does_not_error_and_updates_screen_size() {
    let command = PtyCommand::new("cat", std::env::temp_dir(), 24, 80);
    let session = PtySession::spawn(command).expect("spawning cat should succeed");

    session
        .resize(30, 100)
        .expect("resizing a live pty should not error");

    let (rows, cols) = session.with_screen(|screen| screen.size());
    assert_eq!((rows, cols), (30, 100));
}

#[test]
fn resize_truncating_wide_character_keeps_reader_parser_usable() {
    let command = PtyCommand::new("sh", std::env::temp_dir(), 2, 216)
        .arg("-c")
        .arg("printf '\\033[215G界'; read answer; printf '\\033[0Jreader-alive'");
    let session = PtySession::spawn(command).expect("spawning the resize fixture should succeed");

    let saw_wide_character = wait_until(
        || session.with_screen(|screen| screen.cell(0, 214).is_some_and(vt100::Cell::is_wide)),
        Duration::from_secs(5),
    );
    assert!(
        saw_wide_character,
        "fixture must place a wide character across the old row boundary"
    );

    session
        .resize(2, 215)
        .expect("shrinking across the wide character should succeed");
    session
        .write(b"\n")
        .expect("releasing the fixture should let it emit the erase sequence");

    let parser_remained_usable = wait_until(
        || session.screen_text().contains("reader-alive"),
        Duration::from_secs(5),
    );
    assert!(
        parser_remained_usable,
        "the PTY reader must survive the erase sequence after the resize"
    );
    assert_eq!(session.with_screen(|screen| screen.size()), (2, 215));
}

#[test]
fn process_id_is_reported_for_a_spawned_child() {
    let command = PtyCommand::new("cat", std::env::temp_dir(), 24, 80);
    let session = PtySession::spawn(command).expect("spawning cat should succeed");

    assert!(
        session.process_id().is_some(),
        "expected a real OS pid for the spawned child"
    );
}

#[tokio::test]
async fn screen_changed_watch_channel_notifies_on_new_output() {
    let command = PtyCommand::new("cat", std::env::temp_dir(), 24, 80);
    let session = PtySession::spawn(command).expect("spawning cat should succeed");
    let mut screen_changed = session.subscribe_screen_changed();
    let initial_generation = session.screen_generation();

    session
        .write(b"watch-channel-marker\n")
        .expect("writing the marker should succeed");

    let notified = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if session.screen_text().contains("watch-channel-marker") {
                return;
            }
            // `changed()` only errors once the sender (owned by the reader
            // thread) is dropped, which happens when the child exits and
            // the reader thread returns -- treat that as "no more updates
            // coming" and fall through to the final check below it.
            if screen_changed.changed().await.is_err() {
                return;
            }
        }
    })
    .await;

    assert!(
        notified.is_ok(),
        "timed out waiting for a screen-changed notification"
    );
    assert!(
        session
            .screen_snapshot()
            .text
            .contains("watch-channel-marker"),
        "expected echo's output on screen after being notified, got: {:?}",
        session.screen_text()
    );
    assert!(
        session.screen_generation() > initial_generation,
        "screen generation must advance when parser-visible output arrives"
    );
}

#[tokio::test]
async fn output_bytes_broadcast_carries_the_raw_pty_bytes() {
    // `cat` (unlike `echo`) emits nothing until it receives input, and we
    // only write the marker *after* subscribing below -- that ordering is
    // what closes the race: `subscribe_output_bytes` only ever yields bytes
    // read after the subscribe call (see its doc comment), so an `echo`
    // whose output is already flowing before the test gets around to
    // subscribing could have its marker chunk broadcast to zero receivers
    // and lost forever, timing out or failing outright depending on
    // scheduling. Writing after subscribing guarantees the marker bytes
    // cannot exist yet when we start listening.
    let command = PtyCommand::new("cat", std::env::temp_dir(), 24, 80);
    let session = PtySession::spawn(command).expect("spawning cat should succeed");
    let mut output_bytes = session.subscribe_output_bytes();

    session
        .write(b"output-bytes-marker\n")
        .expect("writing input to the pty should succeed");

    let received = tokio::time::timeout(Duration::from_secs(5), async {
        let mut collected = Vec::new();
        loop {
            match output_bytes.recv().await {
                Ok(chunk) => {
                    collected.extend_from_slice(&chunk.bytes);
                    if String::from_utf8_lossy(&collected).contains("output-bytes-marker") {
                        return collected;
                    }
                }
                Err(_) => return collected,
            }
        }
    })
    .await
    .expect("timed out waiting for the output-bytes broadcast");

    assert!(
        String::from_utf8_lossy(&received).contains("output-bytes-marker"),
        "expected the raw broadcast bytes to contain the echoed marker, got: {:?}",
        String::from_utf8_lossy(&received)
    );
}

#[tokio::test]
async fn output_replay_includes_bytes_written_before_any_subscriber_attaches() {
    // This is the detach/reattach contract: output emitted before a client
    // subscribes is no longer lost just because the broadcast has no past
    // receiver. `cat` keeps the PTY alive long enough to inspect its journal.
    let command = PtyCommand::new("cat", std::env::temp_dir(), 24, 80);
    let session = PtySession::spawn(command).expect("spawning cat should succeed");
    session
        .write(b"replay-before-subscribe\n")
        .expect("writing input to the pty should succeed");

    let observed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let replay = session.output_replay();
            if String::from_utf8_lossy(&replay.bytes).contains("replay-before-subscribe") {
                return replay;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("timed out waiting for the PTY replay journal");

    assert!(observed.is_complete);
    assert!(observed.through_sequence > 0);
    assert!(String::from_utf8_lossy(&observed.bytes).contains("replay-before-subscribe"));
}

#[test]
fn kill_terminates_a_running_child_and_is_idempotent() {
    // `cat` with no arguments reads stdin forever, so it only exits when
    // killed -- exactly the "still running" case `kill` needs to prove.
    let command = PtyCommand::new("cat", std::env::temp_dir(), 24, 80);
    let mut session = PtySession::spawn(command).expect("spawning cat should succeed");
    assert!(!session.has_exited(), "cat should still be running");

    session.kill().expect("killing a live child should succeed");

    let exited = wait_until(|| session.has_exited(), Duration::from_secs(5));
    assert!(exited, "expected the child to have exited after kill");

    // Calling kill again on an already-exited child must not error.
    session
        .kill()
        .expect("killing an already-exited child should be a no-op, not an error");
}

#[cfg(unix)]
#[tokio::test]
async fn reader_thread_stops_after_drop_even_when_a_descendant_still_holds_the_tty() {
    // Reproduces the scenario `impl Drop for PtySession` exists to bound.
    // Note this specifically needs a descendant that detaches into its
    // *own* session (`setsid sleep 30`) while still inheriting the pty's
    // slave fds unredirected: Linux hangs up a pty for every fd still
    // referencing it, but only when the *session leader* of that
    // controlling terminal exits (portable_pty's spawned child always is
    // one -- see its `setsid()` call). A plain same-session background job
    // (`sleep 30 &` with no `setsid`) is already unblocked by that hangup
    // once the direct child dies, so it does not reproduce this leak; a
    // descendant that re-parents itself into a separate session is exactly
    // the "backgrounded job/daemon that didn't redirect stdio" case the
    // `Drop` doc comment describes, and the kernel does not extend that
    // same hangup to it. Before cancellation existed, the background
    // reader thread's blocking `read()` would stay stuck on such an
    // orphan's fd for the full 30 seconds (or forever, for a longer-lived
    // orphan), keeping its `Arc` clones of the parser/journal/child alive
    // the whole time.
    let command = PtyCommand::new("sh", std::env::temp_dir(), 24, 80)
        .arg("-c")
        .arg("setsid sleep 30 & sleep 100");
    let mut session = PtySession::spawn(command).expect("spawning sh should succeed");

    // Give the shell time to fork and `setsid` its orphan before killing
    // it -- killing too early could tear down the orphan along with its
    // still-forming parent instead of leaving it detached and running.
    tokio::time::sleep(Duration::from_millis(300)).await;
    session
        .kill()
        .expect("killing the still-running direct child (sh) should succeed");
    let shell_exited = wait_until(|| session.has_exited(), Duration::from_secs(5));
    assert!(
        shell_exited,
        "the directly-spawned sh should be dead after kill(), leaving only the orphaned, \
         separately-sessioned sleep holding the pty open"
    );

    // The reader thread's `watch::Sender` (moved into the thread at spawn
    // time) is only dropped when the thread itself returns; observing
    // `changed()` fail with the sender gone is a black-box proxy for "the
    // reader thread actually terminated", since this crate exposes no
    // other way to observe that thread from outside.
    let mut screen_changed = session.subscribe_screen_changed();
    drop(session);

    let reader_thread_stopped = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if screen_changed.changed().await.is_err() {
                return;
            }
        }
    })
    .await
    .is_ok();

    assert!(
        reader_thread_stopped,
        "expected the reader thread to exit (dropping its watch::Sender) within a few poll \
         intervals of Drop, well inside the 30 seconds the orphaned sleep keeps the pty open"
    );
}
