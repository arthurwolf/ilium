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

    #[test]
    fn current_reader_epoch_tracks_native_resize_and_never_follows_a_replacement_session() {
        let mut original = PtySession::spawn(
            PtyCommand::new("sh", std::env::temp_dir(), 24, 80)
                .arg("-c")
                .arg("stty -echo; printf 'ORIGINAL_READY\\n'; while IFS= read -r line; do printf 'GEOMETRY:'; stty size; done"),
        )
        .expect("spawn owned original session");
        let reader = original.current_screen_reader();
        let ready = wait_until(
            || original.screen_text().contains("ORIGINAL_READY"),
            Duration::from_secs(2),
        );
        let initial =
            reader.try_with_screen_and_resize_epoch(|screen, epoch| (screen.size(), epoch));
        let same_size = original.resize(24, 80);
        let after_same =
            reader.try_with_screen_and_resize_epoch(|screen, epoch| (screen.size(), epoch));
        let away = original.resize(10, 30);
        let back = original.resize(24, 80);
        let after_back =
            reader.try_with_screen_and_resize_epoch(|screen, epoch| (screen.size(), epoch));
        let input = original.write(b"probe\n");
        let native_geometry = wait_until(
            || original.screen_text().contains("GEOMETRY:24 80"),
            Duration::from_secs(2),
        );
        let after_output =
            reader.try_with_screen_and_resize_epoch(|screen, epoch| (screen.size(), epoch));
        let original_shutdown = original.shutdown_blocking(Duration::from_secs(2));
        drop(original);

        let mut replacement = PtySession::spawn(
            PtyCommand::new("sh", std::env::temp_dir(), 12, 42)
                .arg("-c")
                .arg("printf 'REPLACEMENT_READY\\n'; exec cat"),
        )
        .expect("spawn owned replacement session");
        let replacement_ready = wait_until(
            || replacement.screen_text().contains("REPLACEMENT_READY"),
            Duration::from_secs(2),
        );
        let old_frame = reader.try_with_screen_and_resize_epoch(|screen, epoch| {
            (
                screen.size(),
                epoch,
                screen.contents().contains("REPLACEMENT_READY"),
            )
        });
        let replacement_frame = replacement
            .current_screen_reader()
            .try_with_screen_and_resize_epoch(|screen, epoch| (screen.size(), epoch));
        let replacement_shutdown = replacement.shutdown_blocking(Duration::from_secs(2));
        drop(replacement);

        // Settle both owned sessions before assertions, including on failure.
        for result in [original_shutdown, replacement_shutdown] {
            let report = result.expect("owned child cleanup");
            assert!(report.pending.is_empty(), "unjoined workers: {report:?}");
            assert!(report.panicked.is_empty(), "panicked workers: {report:?}");
        }
        assert!(
            ready && replacement_ready,
            "native fixtures did not become ready"
        );
        assert!(same_size.is_ok() && away.is_ok() && back.is_ok() && input.is_ok());
        assert_eq!(initial, Some(((24, 80), 0)));
        assert_eq!(after_same, Some(((24, 80), 1)));
        assert_eq!(after_back, Some(((24, 80), 3)));
        assert!(
            native_geometry,
            "child stty did not observe committed OS geometry"
        );
        assert_eq!(after_output, Some(((24, 80), 3)));
        assert_eq!(old_frame, Some(((24, 80), 3, false)));
        assert_eq!(replacement_frame, Some(((12, 42), 0)));
    }
}
