//! Integration tests that spawn a real pty and a trivial real command
//! (`echo`, `cat`). This crate's whole job is talking to the OS, so faking
//! that away would test nothing real -- these are deliberately not unit
//! tests with a mocked pty.

use std::time::Duration;

use illium_pty::{PtyCommand, PtySession};

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
    let command =
        PtyCommand::new("echo", std::env::temp_dir(), 24, 80).arg("hello from illium-pty");
    let session = PtySession::spawn(command).expect("spawning echo should succeed");

    let saw_output = wait_until(
        || session.screen_text().contains("hello from illium-pty"),
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
    let command = PtyCommand::new("echo", std::env::temp_dir(), 24, 80).arg("watch-channel-marker");
    let session = PtySession::spawn(command).expect("spawning echo should succeed");
    let mut screen_changed = session.subscribe_screen_changed();

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
        session.screen_text().contains("watch-channel-marker"),
        "expected echo's output on screen after being notified, got: {:?}",
        session.screen_text()
    );
}
