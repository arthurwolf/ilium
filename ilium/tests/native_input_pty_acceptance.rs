//! Explicit isolated acceptance harness; no server, user session or network.
//! Run with ILIUM_NATIVE_INPUT_TEST_EXE pointing to the frozen client test
//! executable. Receipt/control paths live only under this test's temp directory.
use ilium_pty::{PtyCommand, PtySession, PtyTerminationHandle};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

struct Fixture(PtySession);
fn terminate_owned(handle: &PtyTerminationHandle) -> Value {
    let result = handle.terminate_process_tree(Duration::from_secs(5));
    let value = match result {
        Ok(proof) => {
            serde_json::json!({"type":"cleanup","kill_invoked":true,"termination_proof":true,"signalled_processes":proof.signalled_processes})
        }
        Err(error) => {
            serde_json::json!({"type":"cleanup","kill_invoked":true,"termination_proof":false,"error":error.to_string()})
        }
    };
    println!("{value}");
    value
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.0.child_exit().is_none() {
            terminate_owned(&self.0.termination_handle());
        }
    }
}
struct Feed {
    worker: Option<std::thread::JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    killer: PtyTerminationHandle,
    result: std::sync::mpsc::Receiver<Result<(), String>>,
}
impl Drop for Feed {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.cancel.store(true, Ordering::Release);
            terminate_owned(&self.killer);
            let _ = worker.join();
        }
    }
}
struct Watchdog {
    cancel: std::sync::mpsc::Sender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Watchdog {
    fn start(handle: PtyTerminationHandle, deadline: Instant, output: PathBuf) -> Self {
        let (cancel, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let outcome = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()));
            let value = match outcome {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    serde_json::json!({"type":"watchdog","kill_invoked":false,"cancelled":true})
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // The invoked receipt precedes native termination; a hung
                    // platform call must not appear to be a successful kill.
                    std::fs::write(
                        &output,
                        b"{\"type\":\"watchdog\",\"kill_invoked\":true,\"termination_proof\":null}",
                    )
                    .unwrap();
                    terminate_owned(&handle)
                }
            };
            std::fs::write(output, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        });
        Self {
            cancel,
            worker: Some(worker),
        }
    }
}
impl Drop for Watchdog {
    fn drop(&mut self) {
        let _ = self.cancel.send(());
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn rows(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}
fn rss(system: &mut sysinfo::System, process_id: u32) -> Option<u64> {
    let id = sysinfo::Pid::from_u32(process_id);
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[id]), true);
    system
        .process(id)
        .map(|process| process.memory())
        .filter(|memory| *memory > 0)
}
fn wait_receipt(
    path: &Path,
    kind: &str,
    child: &Fixture,
    deadline: Instant,
    system: &mut sysinfo::System,
    peak: &mut u64,
) -> Value {
    loop {
        if let Some(value) = rows(path).into_iter().find(|value| value["type"] == kind) {
            return value;
        }
        if let Some(memory) = child.0.process_id().and_then(|id| rss(system, id)) {
            *peak = (*peak).max(memory);
        }
        assert!(
            child.0.child_exit().is_none(),
            "fixture exited before {kind}: {}",
            child.0.screen_text()
        );
        assert!(
            Instant::now() < deadline,
            "PTY receipt {kind} timed out: {}",
            child.0.screen_text()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn feed(child: &Fixture, case: &str, initial: bool) -> Feed {
    let input = child.0.input_handle();
    let killer = child.0.termination_handle();
    let cancel = Arc::new(AtomicBool::new(false));
    let body = cancel.clone();
    let case = case.to_string();
    let (sender, result) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let send = |bytes: &[u8]| -> Result<(), String> {
            if body.load(Ordering::Acquire) {
                return Err("fixture feed cancelled".to_string());
            }
            input
                .write(bytes)
                .map_err(|error| error.to_string())?
                .wait_blocking()
                .map(|_| ())
                .map_err(|error| error.to_string())
        };
        let result = (|| {
            if matches!(case.as_str(), "release" | "cancel") {
                if initial {
                    send(&[b"\x1b[200~".as_slice(), &[b'a'; 294]].concat())?;
                } else {
                    send(b"\x1b[201~z")?;
                }
            } else if case == "large" {
                send(b"\x1b[200~")?;
                let chunk = [b'a'; 16 * 1024];
                for _ in 0..(40 * 1024 * 1024 / chunk.len()) {
                    send(&chunk)?;
                }
                send(b"\x1b[201~z")?;
            } else {
                send(b"\x1b[200~beforequery\x1b[201~z")?;
            }
            Ok(())
        })();
        let _ = sender.send(result);
    });
    Feed {
        worker: Some(worker),
        cancel,
        killer,
        result,
    }
}
fn finish_feed(
    mut writer: Feed,
    child: &Fixture,
    deadline: Instant,
    system: &mut sysinfo::System,
    peak: &mut u64,
) {
    loop {
        match writer.result.recv_timeout(Duration::from_millis(10)) {
            Ok(result) => {
                result.unwrap();
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("fixture writer failed: {error}"),
        }
        if let Some(memory) = child.0.process_id().and_then(|id| rss(system, id)) {
            *peak = (*peak).max(memory);
        }
        assert!(Instant::now() < deadline, "PTY feed deadline elapsed");
    }
    writer.worker.take().unwrap().join().unwrap();
}
fn expected_digest(case: &str) -> String {
    let mut hash = Sha256::new();
    if case == "large" {
        let chunk = [b'a'; 16 * 1024];
        for _ in 0..(40 * 1024 * 1024 / chunk.len()) {
            hash.update(chunk);
        }
    } else if case == "release" {
        hash.update([b'a'; 294]);
    } else {
        hash.update(b"beforequery");
    }
    format!("{:x}", hash.finalize())
}

#[test]
#[ignore = "explicit real-PTY acceptance; requires ILIUM_NATIVE_INPUT_TEST_EXE and isolated output"]
fn owned_native_input_real_pty_acceptance() {
    let executable = PathBuf::from(
        std::env::var_os("ILIUM_NATIVE_INPUT_TEST_EXE")
            .expect("explicit frozen client fixture executable"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let evidence = PathBuf::from(
        std::env::var_os("ILIUM_NATIVE_PTY_EVIDENCE")
            .expect("explicit task-owned evidence directory"),
    );
    std::fs::create_dir(&evidence).expect("fresh non-overwriting evidence directory");
    for case in ["release", "cancel", "query", "large"] {
        let state = tempfile::tempdir().unwrap();
        let receipt_path = state.path().join("receipt.jsonl");
        let control_path = state.path().join("control.txt");
        let deadline = Instant::now() + Duration::from_secs(180);
        let command = PtyCommand::new(executable.to_string_lossy(), state.path(), 40, 120)
            .arg("--ignored")
            .arg("--exact")
            .arg("terminal_input_owner::pty_child_tests::real_pty_child")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env("ILIUM_NATIVE_PTY_CASE", case)
            .env(
                "ILIUM_NATIVE_PTY_RECEIPT",
                receipt_path.to_string_lossy().into_owned(),
            )
            .env(
                "ILIUM_NATIVE_PTY_CONTROL",
                control_path.to_string_lossy().into_owned(),
            );
        let child = Fixture(PtySession::spawn(command).unwrap());
        let identity = serde_json::json!({"type":"owned_child","case":case,"process_id":child.0.process_id(),"fixture_executable":executable,"identity_owner":"existing PtyTerminationHandle retains birth-checked native identity; scalar PID receipt alone is not kill authority"});
        std::fs::write(
            evidence.join(format!("{case}-identity.json")),
            serde_json::to_vec_pretty(&identity).unwrap(),
        )
        .unwrap();
        let watchdog = Watchdog::start(
            child.0.termination_handle(),
            deadline,
            evidence.join(format!("{case}-watchdog.json")),
        );
        let mut system = sysinfo::System::new();
        let mut peak = 0;
        let ready = wait_receipt(
            &receipt_path,
            "ready",
            &child,
            deadline,
            &mut system,
            &mut peak,
        );
        let baseline_rss = child.0.process_id().and_then(|id| rss(&mut system, id));
        finish_feed(
            feed(&child, case, true),
            &child,
            deadline,
            &mut system,
            &mut peak,
        );
        if matches!(case, "release" | "cancel") {
            wait_receipt(
                &receipt_path,
                "blocked",
                &child,
                deadline,
                &mut system,
                &mut peak,
            );
            std::fs::write(&control_path, case).unwrap();
            if case == "release" {
                finish_feed(
                    feed(&child, case, false),
                    &child,
                    deadline,
                    &mut system,
                    &mut peak,
                );
            }
        }
        let done = wait_receipt(
            &receipt_path,
            "done",
            &child,
            deadline,
            &mut system,
            &mut peak,
        );
        if case != "cancel" {
            let paste = rows(&receipt_path)
                .into_iter()
                .find(|value| value["type"] == "paste")
                .unwrap();
            assert_eq!(paste["sha256"], expected_digest(case));
            assert_eq!(done["following_key"], "z");
        } else {
            assert!(rows(&receipt_path)
                .iter()
                .any(|value| value["type"] == "retained"));
        }
        loop {
            if let Some(exit) = child.0.child_exit() {
                assert_eq!(exit.cause, ilium_pty::PtyExitCause::ExitCode(0));
                break;
            }
            assert!(Instant::now() < deadline, "owned child exit timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
        let captured = std::fs::read(&receipt_path).unwrap();
        std::fs::write(evidence.join(format!("{case}-child.jsonl")), captured).unwrap();
        drop(watchdog); // Successful exit cancels, wakes and joins the watchdog.
        let summary = serde_json::json!({"type":"result","case":case,"ready":ready,"done":done,"baseline_child_rss_bytes":baseline_rss,"sampled_peak_child_rss_bytes":(peak>0).then_some(peak),"rss_sampling_interval_ms":10,"rss_is_sampled_not_a_bound":true,"host":std::env::consts::OS,"actual_pty":true});
        std::fs::write(
            evidence.join(format!("{case}-summary.json")),
            serde_json::to_vec_pretty(&summary).unwrap(),
        )
        .unwrap();
        println!("{summary}");
    }
}
