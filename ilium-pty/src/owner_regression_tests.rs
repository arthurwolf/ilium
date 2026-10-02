//! Real transport regressions for the source-proven ownership windows.
#![cfg(unix)]

use super::*;
use std::time::{Duration, Instant};

/// Tests own these children; unwind must also release a blocked transport.
struct TestChildGuard(Arc<PtySession>);

impl Drop for TestChildGuard {
    fn drop(&mut self) {
        if let Ok(mut child) = self.0.child.lock() {
            let _ = child.kill();
        }
    }
}

#[test]
fn screen_reads_do_not_wait_for_a_parser_mutation_lock() {
    let directory = std::env::temp_dir();
    let session = Arc::new(
        PtySession::spawn(
            PtyCommand::new("/bin/sh", &directory, 64, 80)
                .arg("-c")
                .arg("exec sleep 30"),
        )
        .unwrap(),
    );
    let child_guard = TestChildGuard(Arc::clone(&session));
    // Native resize holds this same write guard before calling the OS. Keep
    // it held until the independent reader deadline has been observed.
    let parser_guard = session.parser.write().unwrap();
    let reader_session = Arc::clone(&session);
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let size = reader_session.with_screen(vt100::Screen::size);
        let snapshot = reader_session.screen_snapshot();
        sender.send((size, snapshot)).unwrap();
    });
    let bounded = receiver.recv_timeout(Duration::from_millis(100));
    drop(parser_guard);
    // Release and join before asserting, including on the unfixed path.
    reader.join().unwrap();
    drop(child_guard);
    drop(session);
    assert!(bounded.is_ok(), "parser mutation blocked screen reads");
    assert_eq!(bounded.unwrap().0, (64, 80));
}

#[cfg(unix)]
#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "run through the owned external watchdog"
)]
fn resize_does_not_change_os_geometry_while_a_screen_reader_holds_the_old_parser_scenario() {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "ilium-geometry-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    let marker = root.join("size.txt");
    let script = format!(
        "while :; do stty size > '{}.next'; mv '{}.next' '{}'; sleep 0.02; done",
        marker.display(),
        marker.display(),
        marker.display()
    );
    let session = Arc::new(
        PtySession::spawn(
            PtyCommand::new("/bin/sh", &root, 64, 80)
                .arg("-c")
                .arg(script),
        )
        .unwrap(),
    );
    let child_guard = TestChildGuard(Arc::clone(&session));
    let deadline = Instant::now() + Duration::from_secs(3);
    while std::fs::read_to_string(&marker).unwrap_or_default().trim() != "64 80"
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), "64 80");
    let child_session = Arc::clone(&session);
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let mut resize_thread = None;
    let observed = session.with_screen(|screen| {
        assert_eq!(screen.size(), (64, 80));
        resize_thread = Some(std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            child_session.resize(60, 80)
        }));
        started_rx.recv().unwrap();
        let deadline = Instant::now() + Duration::from_millis(200);
        while Instant::now() < deadline {
            if std::fs::read_to_string(&marker).unwrap_or_default().trim() == "60 80" {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::read_to_string(&marker).unwrap_or_default()
    });
    let result = resize_thread.unwrap().join().unwrap();
    session.child.lock().unwrap().kill().unwrap();
    drop(child_guard);
    drop(session);
    std::fs::remove_file(&marker).unwrap();
    let next = root.join("size.txt.next");
    if next.exists() {
        std::fs::remove_file(next).unwrap();
    }
    std::fs::remove_dir(&root).unwrap();
    result.unwrap();
    assert_eq!(
        observed.trim(),
        "64 80",
        "OS size must not change while parser still exposes the old geometry"
    );
}

#[cfg(unix)]
#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "run through the owned external watchdog"
)]
fn stalled_child_input_returns_a_bounded_error_without_freezing_screen_reads_scenario() {
    let session = Arc::new(
        PtySession::spawn(
            PtyCommand::new("/bin/sh", std::env::temp_dir(), 24, 80)
                .arg("-c")
                .arg("stty -echo -icanon; printf READY; exec sleep 15"),
        )
        .unwrap(),
    );
    let child_guard = TestChildGuard(Arc::clone(&session));
    let deadline = Instant::now() + Duration::from_secs(3);
    while !session.screen_text().contains("READY") && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(session.screen_text().contains("READY"));
    let writer_session = Arc::clone(&session);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let writer_thread = std::thread::spawn(move || {
        let result = writer_session.write(&vec![b'x'; 1024 * 1024]);
        done_tx
            .send(result.err().map(|error| format!("{error:?}")))
            .unwrap();
    });
    let started = Instant::now();
    std::thread::sleep(Duration::from_millis(100));
    // A blocked parser read must become a test failure rather than preventing
    // the test from ever reaching its timeout and owned-child cleanup.
    let reader_session = Arc::clone(&session);
    let (read_tx, read_rx) = std::sync::mpsc::channel();
    let reader_thread = std::thread::spawn(move || {
        let started = Instant::now();
        let _ = reader_session.screen_snapshot();
        let _ = read_tx.send(started.elapsed());
    });
    let bounded_read = read_rx.recv_timeout(Duration::from_millis(100));
    let bounded_result = done_rx.recv_timeout(Duration::from_secs(3));
    let delivery_elapsed = started.elapsed();
    session.child.lock().unwrap().kill().unwrap();
    drop(child_guard);
    writer_thread.join().unwrap();
    reader_thread.join().unwrap();
    drop(session);
    assert!(
        matches!(&bounded_result, Ok(Some(error)) if error.contains("TimedOut") || error.contains("Timeout")),
        "stalled write must report an error within3s (elapsed {:?}, result {:?})",
        delivery_elapsed,
        bounded_result
    );
    assert!(
        matches!(&bounded_read, Ok(elapsed) if *elapsed < Duration::from_millis(100)),
        "stalled writer blocked parser reads: {:?}",
        bounded_read
    );
}

// A transport regression can block its own cleanup joins. Keep the deadline
// outside that process so the workspace runner can always report a failure.
#[cfg(target_os = "linux")]
fn run_bounded_scenario(name: &str) {
    let log_path =
        std::env::temp_dir().join(format!("ilium-owner-{}-{name}.log", std::process::id()));
    let log = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&log_path)
        .unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("session::owner_regression_tests::{name}_scenario"),
            "--ignored",
            "--nocapture",
        ])
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let identity = ilium_platform::process_control::capture_pty_process(child.id()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let (outcome, timed_out) = loop {
        if let Some(outcome) = child.try_wait().unwrap() {
            break (outcome, false);
        }
        if Instant::now() >= deadline {
            let cleanup = ilium_platform::process_control::terminate_pty_process_tree(
                &identity,
                Duration::from_secs(3),
            );
            if cleanup.is_err() {
                let _ = child.kill();
            }
            let outcome = child.wait().unwrap();
            assert!(
                cleanup.is_ok(),
                "owned scenario cleanup failed: {cleanup:?}; log={}",
                log_path.display()
            );
            break (outcome, true);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let diagnostics = std::fs::read_to_string(&log_path).unwrap_or_default();
    std::fs::remove_file(&log_path).unwrap();
    assert!(
        !timed_out,
        "owner scenario exceeded its external deadline: {diagnostics}"
    );
    assert!(outcome.success(), "owner scenario failed: {diagnostics}");
}

#[cfg(target_os = "linux")]
#[test]
fn resize_does_not_change_os_geometry_while_a_screen_reader_holds_the_old_parser() {
    run_bounded_scenario(
        "resize_does_not_change_os_geometry_while_a_screen_reader_holds_the_old_parser",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn stalled_child_input_returns_a_bounded_error_without_freezing_screen_reads() {
    run_bounded_scenario(
        "stalled_child_input_returns_a_bounded_error_without_freezing_screen_reads",
    );
}
