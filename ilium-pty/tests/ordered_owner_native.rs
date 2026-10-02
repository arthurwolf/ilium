//! Real-PTY regression for full-duplex output and handle-owned teardown.
//! No Codex executable, fake parser, or test-only transport is involved.

#[cfg(unix)]
mod unix {
    use ilium_pty::{OwnerStatus, PtyCommand, PtySession};
    use std::time::{Duration, Instant};

    fn wait_until(mut predicate: impl FnMut() -> bool, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if predicate() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn parses_child_output_while_a_nonreading_child_blocks_input() {
        let command = PtyCommand::new("sh", std::env::temp_dir(), 24, 80)
            .arg("-c")
            .arg("stty -echo -icanon; printf 'READY\\n'; sleep 0.15; printf 'DURING\\n'; exec sleep 5");
        let mut session = PtySession::spawn(command).expect("spawn native nonreader");
        let ready = wait_until(
            || session.screen_text().contains("READY"),
            Duration::from_secs(2),
        );
        let input = session.input_handle();
        let receipt = input
            .write(&vec![b'X'; 1024 * 1024])
            .expect("bounded admission");
        let observer = receipt.observer();
        let parsed_while_pending = wait_until(
            || session.screen_text().contains("DURING") && observer.result().is_none(),
            Duration::from_secs(1),
        );
        receipt.cancel();
        let final_delivery = receipt.wait_blocking();
        let shutdown = session.shutdown_blocking(Duration::from_secs(2));
        drop(session);
        let closed = wait_until(
            || {
                !matches!(
                    input.status(),
                    OwnerStatus::Running | OwnerStatus::StopRequested
                )
            },
            Duration::from_secs(2),
        );

        assert!(ready, "fixture never produced its first marker");
        assert!(
            parsed_while_pending,
            "native output must parse while input remains blocked"
        );
        assert!(
            final_delivery.is_err(),
            "cancelled blocked input cannot report success"
        );
        let shutdown = shutdown.expect("direct child cleanup failed");
        assert!(
            shutdown.pending.is_empty(),
            "workers still retiring: {shutdown:?}"
        );
        assert!(
            shutdown.panicked.is_empty(),
            "worker panicked: {shutdown:?}"
        );
        assert!(
            shutdown.joined.len() >= 5,
            "missing owned workers: {shutdown:?}"
        );
        assert!(closed, "owner did not settle after its session was dropped");
        assert!(
            input.write(b"late").is_err(),
            "a cloned handle reopened a closed PTY"
        );
    }
}
