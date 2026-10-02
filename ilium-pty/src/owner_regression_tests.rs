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

#[cfg(unix)]
#[test]
fn resize_does_not_change_os_geometry_while_a_screen_reader_holds_the_old_parser() {
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
fn stalled_child_input_returns_a_bounded_error_without_freezing_screen_reads() {
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
        "stalled write must report an error within3s (elapsed {:?})",
        delivery_elapsed
    );
    assert!(
        matches!(&bounded_read, Ok(elapsed) if *elapsed < Duration::from_millis(100)),
        "stalled writer blocked parser reads: {:?}",
        bounded_read
    );
}
