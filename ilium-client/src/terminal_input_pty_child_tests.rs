//! Ignored subprocess entrypoint: only the task-owned real PTY harness runs it.
//! Receipt files are JSONL; large input is never copied into a diagnostic body.
use super::*;
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, path::PathBuf};

struct RawMode;
impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}
fn receipt(path: &PathBuf, value: serde_json::Value) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    serde_json::to_writer(&mut file, &value).unwrap();
    writeln!(file).unwrap();
    file.flush().unwrap();
}
fn control(path: &PathBuf, deadline: Instant) -> String {
    loop {
        if let Ok(value) = std::fs::read_to_string(path) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "isolated fixture control timeout"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn next(
    receiver: &mut InputReceiver,
    deadline: Instant,
    quota: &QuotaGroup,
    peak: &mut usize,
) -> InputEvent {
    loop {
        *peak = (*peak).max(quota.snapshot().worker_bytes);
        match receiver.try_recv() {
            Ok(Ok(event)) => return event,
            Ok(Err(error)) => panic!("native fixture failed: {error}"),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => panic!("native fixture disconnected"),
        }
        assert!(Instant::now() < deadline, "isolated fixture event timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn joined(owner: InputOwner, deadline: Instant) -> (InputRetirement, InputShutdownReport) {
    let mut retirement = owner.stop();
    loop {
        if let Some(report) = retirement.try_complete() {
            assert_eq!(report.exit, WorkerExit::Joined);
            return (retirement, report);
        }
        assert!(Instant::now() < deadline, "native physical join timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
#[ignore = "subprocess entrypoint; requires task-owned PTY and explicit receipt/control paths"]
fn real_pty_child() {
    let case = std::env::var("ILIUM_NATIVE_PTY_CASE").unwrap();
    assert!(matches!(
        case.as_str(),
        "large" | "query" | "release" | "cancel"
    ));
    let output = PathBuf::from(std::env::var_os("ILIUM_NATIVE_PTY_RECEIPT").unwrap());
    let commands = PathBuf::from(std::env::var_os("ILIUM_NATIVE_PTY_CONTROL").unwrap());
    let deadline = Instant::now() + Duration::from_secs(180);
    crossterm::terminal::enable_raw_mode().unwrap();
    let _raw = RawMode;
    let constrained = matches!(case.as_str(), "release" | "cancel");
    let quota = if constrained {
        QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 1,
            worker_bytes: input_storage_bytes() + INPUT_STACK_BYTES + 1024,
        })
    } else {
        // Exact production composition-root limit, with no enlarged fixture cap.
        crate::execution::process_quota()
    };
    let reservation = InputReservation::prepare(&quota).unwrap();
    let baseline = quota.snapshot().worker_bytes;
    let mut other_owner = constrained.then(|| quota.reserve_external_storage(768).unwrap());
    let pushed = Arc::new(AtomicBool::new(false));
    let (admission_sender, admission_receiver) = std::sync::mpsc::channel();
    let (owner, mut receiver, mut probe) = if case == "query" {
        let (owner, receiver, probe) = reservation.start_with_image_probe(pushed.clone()).unwrap();
        (owner, receiver, Some(probe))
    } else {
        let factory =
            InputReaderFactory::NativeObserved(Arc::clone(&reservation.claim), admission_sender);
        let (owner, receiver) = reservation
            .start_with_reader_and_probe(factory, None)
            .unwrap();
        (owner, receiver, None)
    };
    let observed = if case == "query" {
        None
    } else {
        Some(
            admission_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap(),
        )
    };
    receipt(
        &output,
        serde_json::json!({"type":"ready","case":case,"baseline_worker_bytes":baseline,"root_worker_limit":quota.snapshot().limits.worker_bytes}),
    );
    if constrained {
        loop {
            if quota.snapshot().worker_bytes == baseline + 1024
                && *observed.as_ref().unwrap().last_refusal.lock().unwrap()
                    == Some(RejectReason::WorkerBytes)
            {
                break;
            }
            assert!(Instant::now() < deadline, "native prefix not admitted");
            std::thread::sleep(Duration::from_millis(2));
        }
        // The same reader has admitted the initial 256-byte allocation and
        // cannot acquire its 512-byte replacement while this owner holds 768.
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
        receipt(
            &output,
            serde_json::json!({"type":"blocked","case":case,"worker_bytes":quota.snapshot().worker_bytes}),
        );
        assert_eq!(
            control(&commands, deadline).trim(),
            if case == "cancel" {
                "cancel"
            } else {
                "release"
            }
        );
        if case == "cancel" {
            let (retirement, report) = joined(owner, deadline);
            let custody = report.native_custody().unwrap();
            assert!(custody.has_retained_input());
            let (prefix, tail) = custody.retained_original();
            assert_eq!(prefix.len(), 256);
            let expected = [b"\x1b[200~".as_slice(), &[b'a'; 294]].concat();
            assert!(prefix.len() + tail.len() <= expected.len());
            assert_eq!(prefix, &expected[..prefix.len()]);
            assert_eq!(tail, &expected[prefix.len()..prefix.len() + tail.len()]);
            assert_eq!(custody.admission_reason(), Some(RejectReason::WorkerBytes));
            assert!(matches!(
                InputReservation::prepare(&quota),
                Err(InputStartError::AlreadyOwned)
            ));
            receipt(
                &output,
                serde_json::json!({"type":"retained","prefix_bytes":prefix.len(),"fixed_tail_bytes":tail.len(),"unconsumed_os_bytes":expected.len()-prefix.len()-tail.len(),"joined":true,"worker_bytes_with_custody":quota.snapshot().worker_bytes}),
            );
            drop(report);
            drop(retirement);
            drop(receiver);
            drop(other_owner.take());
            drop(observed);
            assert_eq!(quota.snapshot().worker_bytes, 0);
            drop(InputReservation::prepare(&quota).unwrap());
            assert_eq!(quota.snapshot().worker_bytes, 0);
            receipt(
                &output,
                serde_json::json!({"type":"done","case":case,"whole_source_held_through_join":true,"claim_reacquired_after_custody_drop":true}),
            );
            return;
        }
        drop(other_owner.take());
    }
    if let Some(probe) = probe.as_mut() {
        loop {
            match probe.try_recv() {
                Ok(value) => {
                    value.unwrap();
                    break;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(error) => panic!("owned query completion failed: {error}"),
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        // The existing PTY emulator replies with primary DA, and deliberately
        // does not advertise Kitty keyboard enhancement. This is a negative
        // capability query through the actual owned production probe path.
        assert!(!pushed.load(Ordering::Acquire));
        receipt(
            &output,
            serde_json::json!({"type":"query_complete","keyboard_enhancement":false,"same_worker_handles_image_and_normal_input":true}),
        );
    }
    let mut peak = baseline;
    let event = next(&mut receiver, deadline, &quota, &mut peak);
    let (bytes, capacity, digest) = match event.view() {
        Event::Paste(text) => (
            text.len(),
            text.capacity(),
            format!("{:x}", Sha256::digest(text.as_bytes())),
        ),
        _ => panic!("expected one original Paste"),
    };
    let expected_bytes = if case == "large" {
        40 * 1024 * 1024
    } else if case == "release" {
        294
    } else {
        11
    };
    assert_eq!(bytes, expected_bytes);
    if case == "large" {
        assert_eq!(capacity, bytes, "large Paste was not compacted");
    }
    receipt(
        &output,
        serde_json::json!({"type":"paste","case":case,"bytes":bytes,"capacity":capacity,"sha256":digest,"sampled_peak_root_worker_bytes":peak,"completed_root_worker_bytes":quota.snapshot().worker_bytes,"root_values_are_not_rss":true}),
    );
    event.dispatch(|_| ());
    let key = next(&mut receiver, deadline, &quota, &mut peak);
    assert!(
        matches!(key.view(), Event::Key(key) if key.code==crossterm::event::KeyCode::Char('z'))
    );
    key.dispatch(|_| ());
    let (retirement, report) = joined(owner, deadline);
    assert_eq!(report.pending_events().count(), 0);
    assert!(report.failure().is_none());
    assert!(!report.native_custody().unwrap().has_retained_input());
    report.into_result().unwrap();
    drop(retirement);
    drop(receiver);
    drop(observed);
    assert_eq!(quota.snapshot().worker_bytes, 0);
    drop(InputReservation::prepare(&quota).unwrap());
    receipt(
        &output,
        serde_json::json!({"type":"done","case":case,"following_key":"z","joined":true,"remaining_worker_bytes":quota.snapshot().worker_bytes,"query_owned":case=="query"}),
    );
}
