use super::*;
use ilium_platform::owned_worker::{reserve_owned_worker, spawn_owned};
use ilium_platform::pty_io::{IoFailure, PtyWriter, WriteFailure, WriteFailureKind, WriteSuccess};
use std::sync::{mpsc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Trace {
    Started(Vec<u8>),
    Written(Vec<u8>),
    Resize(u16, u16),
}

struct Gate {
    ignore_cancellation: bool,
    started: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
struct GateControl {
    started: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}
fn gate() -> (Gate, GateControl) {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    (
        Gate {
            ignore_cancellation: false,
            started: started_tx,
            release: release_rx,
        },
        GateControl {
            started: started_rx,
            release: release_tx,
        },
    )
}

struct Writer {
    trace: Arc<Mutex<Vec<Trace>>>,
    gate: Option<Gate>,
    fail_after: Option<usize>,
}
impl PtyWriter for Writer {
    fn write_until(
        &mut self,
        bytes: Arc<[u8]>,
        deadline: Instant,
        stop: StopToken,
    ) -> Result<WriteSuccess, WriteFailure> {
        self.trace
            .lock()
            .unwrap()
            .push(Trace::Started(bytes.to_vec()));
        if let Some(gate) = self.gate.take() {
            gate.started.send(()).unwrap();
            loop {
                if !gate.ignore_cancellation && stop.is_stopped() {
                    return Err(WriteFailure::exact(WriteFailureKind::Cancelled, 0, true));
                }
                if !gate.ignore_cancellation && Instant::now() >= deadline {
                    return Err(WriteFailure::exact(WriteFailureKind::Timeout, 0, true));
                }
                match gate.release.recv_timeout(Duration::from_millis(1)) {
                    Ok(()) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        panic!("test gate lost its controller")
                    }
                }
            }
        }
        if let Some(prefix) = self.fail_after.take() {
            let prefix = prefix.min(bytes.len());
            self.trace
                .lock()
                .unwrap()
                .push(Trace::Written(bytes[..prefix].to_vec()));
            return Err(WriteFailure::exact(
                WriteFailureKind::Io(IoFailure {
                    kind: io::ErrorKind::BrokenPipe,
                    message: "scripted partial write".into(),
                }),
                prefix,
                false,
            ));
        }
        self.trace
            .lock()
            .unwrap()
            .push(Trace::Written(bytes.to_vec()));
        Ok(WriteSuccess {
            written: bytes.len(),
            reusable: true,
        })
    }
}

struct ControlState {
    size: (u16, u16),
    fail_next: bool,
    fail_rollback: bool,
    resize_gate: Option<Gate>,
}
struct Control {
    state: Arc<Mutex<ControlState>>,
    parser: Arc<RwLock<Parser>>,
    trace: Arc<Mutex<Vec<Trace>>>,
}
impl PtyControl for Control {
    fn resize(&mut self, rows: u16, cols: u16) -> Result<(), IoFailure> {
        assert!(
            matches!(self.parser.try_read(), Err(TryLockError::WouldBlock)),
            "the parser must be write-locked BEFORE OS resize"
        );
        let gate = self.state.lock().unwrap().resize_gate.take();
        if let Some(gate) = gate {
            gate.started.send(()).unwrap();
            gate.release.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        self.trace.lock().unwrap().push(Trace::Resize(rows, cols));
        let mut state = self.state.lock().unwrap();
        if state.fail_rollback && (rows, cols) == (64, 80) {
            return Err(IoFailure {
                kind: io::ErrorKind::Other,
                message: "rollback refused".into(),
            });
        }
        state.size = (rows, cols);
        if std::mem::take(&mut state.fail_next) {
            Err(IoFailure {
                kind: io::ErrorKind::Other,
                message: "changed OS then failed".into(),
            })
        } else {
            Ok(())
        }
    }
    fn size(&self) -> Result<(u16, u16), IoFailure> {
        Ok(self.state.lock().unwrap().size)
    }
}

struct Harness {
    owner: PtyOwner,
    input: PtyInput,
    parser: Arc<RwLock<Parser>>,
    generation: Arc<AtomicU64>,
    screen_reader: crate::screen_reader::ScreenReader,
    published: Arc<Mutex<Vec<Vec<u8>>>>,
    trace: Arc<Mutex<Vec<Trace>>>,
    control: Arc<Mutex<ControlState>>,
}
impl Harness {
    fn new(limits: OwnerLimits, gate: Option<Gate>, fail_after: Option<usize>) -> Self {
        limits.validate().unwrap();
        let queue = Queue::new(limits);
        let parser = Arc::new(RwLock::new(Parser::new_with_callbacks(
            64,
            80,
            0,
            TerminalQueryResponder::new(),
        )));
        let generation = Arc::new(AtomicU64::new(0));
        let published = Arc::new(Mutex::new(Vec::new()));
        let capture = Arc::clone(&published);
        let trace = Arc::new(Mutex::new(Vec::new()));
        let control = Arc::new(Mutex::new(ControlState {
            size: (64, 80),
            fail_next: false,
            fail_rollback: false,
            resize_gate: None,
        }));
        let (changed, _) = watch::channel(());
        let terminal = TerminalState {
            screen_reader: crate::screen_reader::ScreenReader::new(
                Arc::clone(&parser),
                Arc::clone(&generation),
            ),
            parser: Arc::clone(&parser),
            generation: Arc::clone(&generation),
            changed,
            publish: Box::new(move |bytes| capture.lock().unwrap().push(bytes.to_vec())),
        };
        let screen_reader = terminal.screen_reader.clone();
        let expiry_worker = queue.start_expiry_worker().unwrap();
        // Production wires writer completion back to the owner queue (see
        // `Session` spawn in owner.rs); the harness must do the same, or a
        // completed write leaves the owner asleep until its write deadline.
        let completion_queue = Arc::clone(&queue);
        let writer = AsyncWriter::spawn_reserved_with_completion_wake(
            Box::new(Writer {
                trace: Arc::clone(&trace),
                gate,
                fail_after,
            }),
            queue.stop.child(),
            || {},
            move || completion_queue.wake(),
            reserve_owned_worker(None, ()).unwrap(),
        )
        .unwrap();
        let writer_tickets = writer.tickets();
        let engine = Engine {
            queue: Arc::clone(&queue),
            terminal,
            control: Box::new(Control {
                state: Arc::clone(&control),
                parser: Arc::clone(&parser),
                trace: Arc::clone(&trace),
            }),
            writer,
            active: None,
            pending_replies: VecDeque::new(),
            pending_reply_bytes: 0,
        };
        let (status, receiver) = watch::channel(OwnerStatus::Running);
        let actor_status = status.clone();
        let actor_queue = Arc::clone(&queue);
        let wake_queue = Arc::clone(&queue);
        let worker = spawn_owned(
            "ilium-owner-test",
            WorkerKind::Cooperative,
            StopToken::default(),
            move || wake_queue.wake(),
            move |stop| {
                let guard = OwnerExitGuard {
                    queue: actor_queue,
                    status: actor_status,
                };
                engine.run(&stop, &guard);
            },
        )
        .unwrap();
        let mut tickets = vec![worker.ticket(), expiry_worker.ticket()];
        tickets.extend(writer_tickets);
        let input = PtyInput {
            queue,
            status: receiver,
        };
        let owner = PtyOwner {
            input: input.clone(),
            status,
            owned: vec![worker, expiry_worker],
            tickets,
        };
        Self {
            owner,
            input,
            parser,
            generation,
            screen_reader,
            published,
            trace,
            control,
        }
    }
    fn output(&self, bytes: &[u8]) {
        assert!(self
            .input
            .queue
            .output(ReadMessage::Data(Arc::from(bytes)), &self.input.queue.stop));
    }
    fn fence(&self) {
        self.input.write(b"fence").unwrap().wait_blocking().unwrap();
    }
    fn written(&self) -> Vec<Vec<u8>> {
        self.trace
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Trace::Written(bytes) => Some(bytes.clone()),
                _ => None,
            })
            .collect()
    }
    fn resizes(&self) -> Vec<(u16, u16)> {
        self.trace
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Trace::Resize(rows, cols) => Some((*rows, *cols)),
                _ => None,
            })
            .collect()
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        let report = self.owner.shutdown_blocking(Duration::from_secs(2));
        if std::thread::panicking() {
            return;
        }
        assert!(
            report.pending.is_empty(),
            "unjoined test workers: {report:?}"
        );
        assert!(
            report.panicked.is_empty(),
            "panicked test workers: {report:?}"
        );
    }
}

fn eventually(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !predicate() {
        assert!(Instant::now() < deadline, "condition never became true");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn reply_completion_precedes_resize_and_later_output_uses_new_geometry() {
    let (writer_gate, release) = gate();
    let harness = Harness::new(OwnerLimits::default(), Some(writer_gate), None);
    harness.output(b"\x1b[61;1H\x1b[6n");
    release
        .started
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let resize = harness.input.resize(60, 80).unwrap();
    let observer = resize.observer();
    harness.output(b"\x1b[64;1H\x1b[6n");
    assert!(harness.resizes().is_empty());
    assert!(
        observer.result().is_none(),
        "queue acceptance must not acknowledge completion"
    );
    assert_eq!(harness.parser.read().unwrap().screen().size(), (64, 80));
    assert_eq!(
        harness.published.lock().unwrap().len(),
        1,
        "later output is staged"
    );
    release.release.send(()).unwrap();
    assert_eq!(resize.wait_blocking().unwrap().size, Some((60, 80)));
    eventually(|| harness.written().len() == 2);
    assert_eq!(
        *harness.trace.lock().unwrap(),
        vec![
            Trace::Started(b"\x1b[61;1R".to_vec()),
            Trace::Written(b"\x1b[61;1R".to_vec()),
            Trace::Resize(60, 80),
            Trace::Started(b"\x1b[60;1R".to_vec()),
            Trace::Written(b"\x1b[60;1R".to_vec()),
        ]
    );
    assert_eq!(harness.generation.load(Ordering::Acquire), 3);
}

#[test]
fn failed_reply_never_allows_a_queued_resize_or_later_input() {
    let (writer_gate, release) = gate();
    let harness = Harness::new(OwnerLimits::default(), Some(writer_gate), Some(2));
    harness.output(b"\x1b[61;1H\x1b[6n");
    release
        .started
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let resize = harness.input.resize(60, 80).unwrap();
    let input = harness
        .input
        .write(b"must-not-follow-partial-reply")
        .unwrap();
    release.release.send(()).unwrap();
    assert!(matches!(
        resize.wait_blocking().unwrap_err().failure,
        DeliveryFailure::Shutdown(ShutdownReason::WriterFailed)
    ));
    assert!(input.wait_blocking().is_err());
    assert!(harness.resizes().is_empty());
    assert_eq!(harness.written(), vec![b"\x1b[".to_vec()]);
    assert_eq!(harness.parser.read().unwrap().screen().size(), (64, 80));
    eventually(|| {
        matches!(
            harness.input.status(),
            OwnerStatus::Stopped {
                error: Some(DeliveryError {
                    failure: DeliveryFailure::PartialWrite { written: 2, .. },
                    ..
                }),
                ..
            }
        )
    });
}

#[test]
fn stalled_input_keeps_parser_readable_and_times_out_without_false_success() {
    let (writer_gate, release) = gate();
    let limits = OwnerLimits {
        command_timeout: Duration::from_millis(80),
        ..OwnerLimits::default()
    };
    let harness = Harness::new(limits, Some(writer_gate), None);
    let input = harness.input.write(b"pending").unwrap();
    release
        .started
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert_eq!(harness.parser.read().unwrap().screen().size(), (64, 80));
    assert!(input.observer().result().is_none());
    assert_eq!(
        input.wait_blocking().unwrap_err().failure,
        DeliveryFailure::Timeout
    );
    assert!(harness.written().is_empty());
    harness.fence(); // a proven zero-byte timeout on a reusable writer is recoverable
}

#[test]
fn newly_queued_output_advances_while_an_input_write_is_pending() {
    let (writer_gate, release) = gate();
    let harness = Harness::new(OwnerLimits::default(), Some(writer_gate), None);
    let input = harness.input.write(b"blocked input").unwrap();
    release
        .started
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    harness.output(b"visible during write");
    eventually(|| {
        harness
            .parser
            .read()
            .unwrap()
            .screen()
            .contents()
            .contains("visible during write")
    });
    assert!(
        harness.written().is_empty(),
        "the native write is still gated"
    );
    assert_eq!(harness.published.lock().unwrap().len(), 1);

    release.release.send(()).unwrap();
    assert_eq!(input.wait_blocking().unwrap().bytes_written, 13);
}

#[test]
fn active_partial_input_has_exact_prefix_and_is_never_replayed() {
    let harness = Harness::new(OwnerLimits::default(), None, Some(3));
    let failure = harness
        .input
        .write(b"abcdef")
        .unwrap()
        .wait_blocking()
        .unwrap_err();
    assert!(matches!(
        failure.failure,
        DeliveryFailure::PartialWrite {
            requested: 6,
            written: 3,
            ..
        }
    ));
    assert!(!failure.proves_zero_delivery());
    eventually(|| matches!(harness.input.status(), OwnerStatus::Stopped { .. }));
    assert!(harness.input.write(b"abcdef").is_err());
    assert_eq!(harness.written(), vec![b"abc".to_vec()]);
}

#[test]
fn failed_resize_rolls_back_os_without_reflowing_or_truncating_parser() {
    let harness = Harness::new(OwnerLimits::default(), None, None);
    let original = b"\x1b[50;1Hkept-lower-row\x1b[64;80HX";
    harness.output(original);
    harness.fence();
    let before = {
        let parser = harness.parser.read().unwrap();
        (
            parser.screen().contents(),
            parser.screen().cursor_position(),
        )
    };
    harness.control.lock().unwrap().fail_next = true;
    let error = harness
        .input
        .resize(10, 20)
        .unwrap()
        .wait_blocking()
        .unwrap_err();
    assert!(matches!(
        error.failure,
        DeliveryFailure::Resize { restored: true, .. }
    ));
    assert_eq!(harness.control.lock().unwrap().size, (64, 80));
    let parser = harness.parser.read().unwrap();
    assert_eq!(parser.screen().size(), (64, 80));
    assert_eq!(
        (
            parser.screen().contents(),
            parser.screen().cursor_position()
        ),
        before
    );
    drop(parser);
    harness.output(b"Z");
    harness.fence();
    let mut oracle = Parser::new_with_callbacks(64, 80, 0, TerminalQueryResponder::new());
    oracle.process(original);
    oracle.process(b"Z");
    let parser = harness.parser.read().unwrap();
    assert_eq!(parser.screen().contents(), oracle.screen().contents());
    assert_eq!(
        parser.screen().cursor_position(),
        oracle.screen().cursor_position()
    );
}

#[test]
fn unverified_resize_rollback_closes_the_owner_instead_of_parsing_mismatched_geometry() {
    let harness = Harness::new(OwnerLimits::default(), None, None);
    {
        let mut state = harness.control.lock().unwrap();
        state.fail_next = true;
        state.fail_rollback = true;
    }
    let error = harness
        .input
        .resize(10, 20)
        .unwrap()
        .wait_blocking()
        .unwrap_err();
    assert!(matches!(
        error.failure,
        DeliveryFailure::Resize {
            restored: false,
            ..
        }
    ));
    eventually(|| {
        matches!(
            harness.input.status(),
            OwnerStatus::Stopped {
                reason: ShutdownReason::GeometryLost,
                ..
            }
        )
    });
    assert!(harness.input.write(b"do-not-send").is_err());
    assert_eq!(harness.parser.read().unwrap().screen().size(), (64, 80));
}

#[test]
fn competing_resizers_commit_in_their_admission_order() {
    let harness = Harness::new(OwnerLimits::default(), None, None);
    let mut threads = Vec::new();
    for index in 0..12 {
        let input = harness.input.clone();
        threads.push(std::thread::spawn(move || {
            let size = (20 + index, 60 + index);
            let receipt = input.resize(size.0, size.1).unwrap();
            (receipt.operation_id, size, receipt)
        }));
    }
    let mut receipts: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    receipts.sort_by_key(|(id, _, _)| *id);
    let expected: Vec<_> = receipts.iter().map(|(_, size, _)| *size).collect();
    for (_, size, receipt) in receipts {
        assert_eq!(receipt.wait_blocking().unwrap().size, Some(size));
    }
    assert_eq!(harness.resizes(), expected);
    assert_eq!(
        harness.parser.read().unwrap().screen().size(),
        *expected.last().unwrap()
    );
    assert_eq!(
        harness.control.lock().unwrap().size,
        *expected.last().unwrap()
    );
}

#[test]
fn queue_count_and_bytes_include_the_active_stalled_write() {
    let (writer_gate, release) = gate();
    let limits = OwnerLimits {
        commands: 1,
        command_bytes: 128,
        single_input_bytes: 128,
        ..OwnerLimits::default()
    };
    let harness = Harness::new(limits, Some(writer_gate), None);
    let active = harness.input.write(&[b'a'; 128]).unwrap();
    release
        .started
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert_eq!(harness.input.queue_load().commands, 1);
    assert_eq!(harness.input.queue_load().command_bytes, 128);
    let error = match harness.input.write(b"extra") {
        Err(error) => error,
        Ok(_) => panic!("overload accepted"),
    };
    assert_eq!(error.operation_id, None);
    assert!(matches!(error.failure, DeliveryFailure::Overloaded { .. }));
    let error = match harness.input.write(&[0; 129]) {
        Err(error) => error,
        Ok(_) => panic!("oversize accepted"),
    };
    assert!(matches!(error.failure, DeliveryFailure::TooLarge { .. }));
    release.release.send(()).unwrap();
    assert_eq!(active.wait_blocking().unwrap().bytes_written, 128);
    eventually(|| harness.input.queue_load().commands == 0);
}

#[test]
fn output_backpressure_does_not_overtake_an_already_accepted_resize() {
    let (writer_gate, release) = gate();
    let limits = OwnerLimits {
        output_chunks: 1,
        ..OwnerLimits::default()
    };
    let harness = Harness::new(limits, Some(writer_gate), None);
    harness.output(b"\x1b[61;1H\x1b[6n");
    release
        .started
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let resize = harness.input.resize(60, 80).unwrap();
    let queue = Arc::clone(&harness.input.queue);
    let producer = std::thread::spawn(move || {
        queue.output(
            ReadMessage::Data(Arc::from(b"\x1b[64;1H\x1b[6n".as_slice())),
            &queue.stop,
        )
    });
    eventually(|| harness.input.queue_load().output_chunks == 1);
    assert!(harness.resizes().is_empty());
    assert_eq!(harness.parser.read().unwrap().screen().size(), (64, 80));
    release.release.send(()).unwrap();
    resize.wait_blocking().unwrap();
    assert!(producer.join().unwrap());
    eventually(|| harness.written().len() == 2);
    assert_eq!(
        harness.written(),
        vec![b"\x1b[61;1R".to_vec(), b"\x1b[60;1R".to_vec()]
    );
}

#[test]
fn shutdown_rejects_queued_work_cancels_active_io_and_joins_owned_worker() {
    let (writer_gate, release) = gate();
    let harness = Harness::new(OwnerLimits::default(), Some(writer_gate), None);
    let active = harness.input.write(b"active").unwrap();
    release
        .started
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let queued = harness.input.write(b"queued").unwrap();
    let resize = harness.input.resize(10, 20).unwrap();
    let report = harness.owner.shutdown_blocking(Duration::from_secs(2));
    let mut expected: Vec<_> = harness.owner.tickets.iter().map(WorkerTicket::id).collect();
    let mut joined = report.joined.clone();
    expected.sort_unstable();
    joined.sort_unstable();
    assert_eq!(joined, expected, "every actual owned worker must be joined");
    assert!(report.panicked.is_empty());
    assert!(report.pending.is_empty());
    assert_eq!(
        active.wait_blocking().unwrap_err().failure,
        DeliveryFailure::Cancelled
    );
    assert_eq!(
        queued.wait_blocking().unwrap_err().failure,
        DeliveryFailure::Shutdown(ShutdownReason::Requested)
    );
    assert!(resize.wait_blocking().is_err());
    assert!(harness.written().is_empty());
    assert!(harness.resizes().is_empty());
}

#[test]
fn parser_reader_cannot_make_owner_cancellation_wait_forever() {
    let limits = OwnerLimits {
        parser_lock_timeout: Duration::from_millis(40),
        ..OwnerLimits::default()
    };
    let harness = Harness::new(limits, None, None);
    let read_guard = harness.parser.read().unwrap();
    harness.output(b"pending-output");
    let resize = harness.input.resize(10, 20).unwrap();
    assert!(matches!(
        resize.wait_blocking().unwrap_err().failure,
        DeliveryFailure::Shutdown(ShutdownReason::ParserUnavailable)
    ));
    drop(read_guard);
    assert!(harness.resizes().is_empty());
}

#[test]
fn zero_resize_is_clamped_in_the_ordered_owner() {
    let harness = Harness::new(OwnerLimits::default(), None, None);
    assert_eq!(
        harness
            .input
            .resize(0, 0)
            .unwrap()
            .wait_blocking()
            .unwrap()
            .size,
        Some((1, 1))
    );
    assert_eq!(harness.parser.read().unwrap().screen().size(), (1, 1));
    assert_eq!(harness.control.lock().unwrap().size, (1, 1));
}

#[tokio::test(flavor = "current_thread")]
async fn aborting_a_waiter_cancels_io_but_retains_observable_final_completion() {
    let (writer_gate, release) = gate();
    let harness = Harness::new(OwnerLimits::default(), Some(writer_gate), None);
    let receipt = harness.input.write(b"cancel-me").unwrap();
    let observer = receipt.observer();
    let wait_task = tokio::spawn(async move { receipt.wait().await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while release.started.try_recv().is_err() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    // This timer can advance even though the only Tokio thread awaits PTY I/O.
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(!wait_task.is_finished());
    wait_task.abort();
    assert!(wait_task.await.unwrap_err().is_cancelled());
    let result = tokio::time::timeout(Duration::from_secs(2), observer.wait())
        .await
        .unwrap();
    assert_eq!(result.unwrap_err().failure, DeliveryFailure::Cancelled);
    assert!(harness.written().is_empty());
}
// Append inside the exact B1 owner_tests module; uses its Harness, Writer and gate APIs.
// This is an independent acceptance test: no resize is admitted while input is pending.
#[test]
fn ordinary_pending_input_does_not_stop_actual_output_parsing() {
    let (writer_gate, controller) = gate();
    let harness = Harness::new(OwnerLimits::default(), Some(writer_gate), None);
    let input = harness.input.write(b"held-input").unwrap();
    let observer = input.observer();

    // Do not assert while the harness is live: a failed assertion followed by
    // Harness::drop's own assertion could otherwise double-panic during cleanup.
    let writer_started = controller
        .started
        .recv_timeout(Duration::from_secs(2))
        .is_ok();
    let generation_before = harness.generation.load(Ordering::Acquire);
    let was_pending_before_output = observer.result().is_none();
    let admitted = harness.input.queue.output(
        ReadMessage::Data(Arc::from(b"OUTPUT-PARSED-DURING-INPUT".as_slice())),
        &harness.input.queue.stop,
    );

    let deadline = Instant::now() + Duration::from_millis(100);
    let mut advanced_while_input_pending = false;
    loop {
        // Both parser access and generation read describe one guarded frame.
        // Reading an unchanged snapshot is deliberately insufficient.
        if let Ok(parser) = harness.parser.try_read() {
            let generation = harness.generation.load(Ordering::Acquire);
            let contains_output = parser
                .screen()
                .contents()
                .contains("OUTPUT-PARSED-DURING-INPUT");
            drop(parser);
            if generation > generation_before && contains_output && observer.result().is_none() {
                advanced_while_input_pending = true;
                break;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    // Release the transport gate before waiting or asserting, on either outcome.
    let gate_released = controller.release.send(()).is_ok();
    let delivery = input.wait_blocking();
    let report = harness.owner.shutdown_blocking(Duration::from_secs(2));
    // Execute the existing assertion-bearing destructor before our assertions,
    // while no unwind is active. Retain its outcome as a normal test result.
    let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(harness)));

    assert!(cleanup.is_ok(), "B1 harness teardown failed");
    assert!(report.pending.is_empty(), "unjoined workers: {report:?}");
    assert!(report.panicked.is_empty(), "panicked workers: {report:?}");
    assert!(
        writer_started,
        "ordinary input never entered the writer gate"
    );
    assert!(
        was_pending_before_output,
        "input had already completed before output admission"
    );
    assert!(
        admitted,
        "ordinary output was rejected before the resize-free observation"
    );
    assert!(
        gate_released,
        "writer gate controller disappeared before release"
    );
    assert_eq!(delivery.unwrap().bytes_written, b"held-input".len());
    assert!(
        advanced_while_input_pending,
        "actual parser content and generation must advance within 100 ms while ordinary input remains pending; readable stale snapshots do not satisfy this gate"
    );
}
// Append inside exact B1 owner_tests module; uses its existing Harness/gate APIs.
#[test]
fn queued_input_deadline_completes_before_an_earlier_reply_barrier_is_released() {
    let (writer_gate, controller) = gate();
    let limits = OwnerLimits {
        command_timeout: Duration::from_millis(30),
        reply_timeout: Duration::from_millis(250),
        ..OwnerLimits::default()
    };
    let harness = Harness::new(limits, Some(writer_gate), None);
    harness.output(b"\x1b[61;1H\x1b[6n");
    let reply_started = controller
        .started
        .recv_timeout(Duration::from_secs(2))
        .is_ok();
    let (input, admission_error) = match harness.input.write(b"must-expire-before-reply-release") {
        Ok(input) => (Some(input), None),
        Err(error) => (None, Some(error)),
    };
    let observer = input.as_ref().map(|input| input.observer());
    let observed_deadline = Instant::now() + Duration::from_millis(100);
    let mut before_release = None;
    while let Some(observer) = observer.as_ref() {
        if let Some(result) = observer.result() {
            before_release = Some(result);
            break;
        }
        if Instant::now() >= observed_deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // Capture native-write history BEFORE allowing the reply to complete.
    let writes_before_release = harness.written();
    let gate_released = controller.release.send(()).is_ok();
    // Original B1 rejects this on eventual dequeue. A corrected implementation
    // must already have completed it during the observation above.
    let eventual = input.map(|input| input.wait_blocking());
    let report = harness.owner.shutdown_blocking(Duration::from_secs(2));
    let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(harness)));

    assert!(cleanup.is_ok(), "B1 harness teardown failed");
    assert!(report.pending.is_empty(), "unjoined workers: {report:?}");
    assert!(report.panicked.is_empty(), "panicked workers: {report:?}");
    assert!(
        reply_started,
        "cursor reply never entered the controlled write barrier"
    );
    assert!(
        gate_released,
        "reply gate controller disappeared before release"
    );
    assert!(
        admission_error.is_none(),
        "ordinary command admission failed: {admission_error:?}"
    );
    assert!(
        writes_before_release.is_empty(),
        "the controlled reply or later input completed before release"
    );
    let error = before_release
        .expect("queued command must complete within 100 ms, before releasing the earlier reply")
        .expect_err("an expired queued input cannot report delivery success");
    assert_eq!(error.failure, DeliveryFailure::Timeout);
    assert!(
        error.proves_zero_delivery(),
        "queued expiry must prove no input was delivered"
    );
    assert_eq!(
        eventual.unwrap().unwrap_err(),
        error,
        "receipt must retain its already-published timeout result"
    );
}

#[test]
fn query_reply_never_splits_an_active_input_payload() {
    let (writer_gate, controller) = gate();
    let harness = Harness::new(OwnerLimits::default(), Some(writer_gate), None);
    let input = harness.input.write(b"one-complete-input").unwrap();
    let started = controller
        .started
        .recv_timeout(Duration::from_secs(2))
        .is_ok();
    harness.output(b"\x1b[61;1H\x1b[6n");
    let parsed_query = {
        let deadline = Instant::now() + Duration::from_millis(100);
        loop {
            if harness.generation.load(Ordering::Acquire) > 0 {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    let no_reply_before_input = harness.written().is_empty();
    let released = controller.release.send(()).is_ok();
    let delivered = input.wait_blocking();
    eventually(|| harness.written().len() == 2);
    let writes = harness.written();
    let report = harness.owner.shutdown_blocking(Duration::from_secs(2));
    let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(harness)));

    assert!(cleanup.is_ok());
    assert!(report.pending.is_empty() && report.panicked.is_empty());
    assert!(started && parsed_query && no_reply_before_input && released);
    assert_eq!(
        delivered.unwrap().bytes_written,
        b"one-complete-input".len()
    );
    assert_eq!(
        writes,
        vec![b"one-complete-input".to_vec(), b"\x1b[61;1R".to_vec()]
    );
}

// Simulates an opaque native call which has entered the writer but has not
// acknowledged cancellation. Terminal output must settle admission receipts
// while retaining this worker; the test releases it before checking joins.
fn terminal_output_retires_active_receipt(message: ReadMessage, reason: ShutdownReason) {
    let (mut writer_gate, controller) = gate();
    writer_gate.ignore_cancellation = true;
    let harness = Harness::new(OwnerLimits::default(), Some(writer_gate), None);
    let receipt = harness.input.write(b"uncertain-input").unwrap();
    let observer = receipt.observer();
    let started = controller
        .started
        .recv_timeout(Duration::from_secs(2))
        .is_ok();
    let admitted = harness
        .input
        .queue
        .output(message, &harness.input.queue.stop);
    let deadline = Instant::now() + Duration::from_secs(1);
    while observer.result().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    let before_release = observer.result();
    let load = harness.input.queue_load();
    let status = harness.input.status();
    let rejected = harness.input.write(b"no-replay").is_err();
    let still_owned = harness
        .owner
        .tickets
        .iter()
        .any(|ticket| ticket.exit().is_none());
    let released = controller.release.send(()).is_ok();
    let report = harness.owner.shutdown_blocking(Duration::from_secs(2));
    let final_result = observer.result();
    drop(receipt);
    drop(harness);

    assert!(started && admitted && released && still_owned);
    let error = before_release
        .expect("terminal failure must settle the active receipt before native return")
        .expect_err("unacknowledged native write cannot report success");
    assert!(matches!(
        error.failure,
        DeliveryFailure::UnconfirmedWrite {
            requested: 15,
            definitely_written: 0,
            possibly_written: 15,
            ..
        }
    ));
    assert!(!error.proves_zero_delivery());
    assert_eq!(
        final_result,
        Some(Err(error)),
        "later native return cannot rewrite a published receipt"
    );
    assert_eq!(load.commands, 0);
    assert_eq!(load.command_bytes, 0);
    assert!(matches!(status, OwnerStatus::Stopped { reason: observed, .. } if observed == reason));
    assert!(rejected);
    assert!(
        report.pending.is_empty() && report.panicked.is_empty(),
        "{report:?}"
    );
}

#[test]
fn eof_settles_active_unconfirmed_input_before_session_teardown() {
    terminal_output_retires_active_receipt(ReadMessage::Eof, ShutdownReason::Eof);
}

#[test]
fn reader_failure_settles_active_unconfirmed_input_before_session_teardown() {
    terminal_output_retires_active_receipt(
        ReadMessage::Error(IoFailure {
            kind: io::ErrorKind::BrokenPipe,
            message: "forced reader failure".into(),
        }),
        ShutdownReason::ReaderFailed,
    );
}

fn terminal_failure_settles_active_input(message: ReadMessage) {
    let (writer_gate, release) = gate();
    let harness = Harness::new(OwnerLimits::default(), Some(writer_gate), None);
    let receipt = harness.input.write(b"pending input").unwrap();
    let observer = receipt.observer();
    release
        .started
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert!(harness
        .input
        .queue
        .output(message, &harness.input.queue.stop));
    eventually(|| matches!(harness.input.status(), OwnerStatus::Stopped { .. }));
    let deadline = Instant::now() + Duration::from_millis(300);
    while observer.result().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    let disposition = observer.result();
    let commands = harness.input.queue_load().commands;
    // Release all owned work before an assertion, even on the broken actor.
    receipt.cancel();
    drop(receipt);
    drop(harness);
    assert!(
        matches!(disposition, Some(Err(_))),
        "terminal owner left its active receipt unsettled: {disposition:?}"
    );
    assert_eq!(commands, 0, "terminal owner retained active command quota");
}

#[test]
fn terminal_output_failure_eof_settles_active_input_before_session_close() {
    terminal_failure_settles_active_input(ReadMessage::Eof);
}

#[test]
fn terminal_output_failure_reader_error_settles_active_input_before_session_close() {
    terminal_failure_settles_active_input(ReadMessage::Error(IoFailure {
        kind: io::ErrorKind::BrokenPipe,
        message: "forced reader failure".into(),
    }));
}

#[test]
fn parser_failure_preserves_final_raw_bytes_and_settles_active_input() {
    let (writer_gate, controller) = gate();
    let harness = Harness::new(
        OwnerLimits {
            parser_lock_timeout: Duration::from_millis(20),
            ..OwnerLimits::default()
        },
        Some(writer_gate),
        None,
    );
    let receipt = harness.input.write(b"pending").unwrap();
    let observer = receipt.observer();
    controller
        .started
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let held_parser = harness.parser.read().unwrap();
    harness.output(b"FINAL-PARSER-FAILURE-EVIDENCE");
    eventually(|| observer.result().is_some());
    let published = harness.published.lock().unwrap().clone();
    let status = harness.input.status();
    let result = observer.result();
    drop(held_parser);
    drop(receipt);
    drop(harness);
    assert!(matches!(
        status,
        OwnerStatus::Stopped {
            reason: ShutdownReason::ParserUnavailable,
            ..
        }
    ));
    assert!(matches!(result, Some(Err(_))));
    assert_eq!(published, vec![b"FINAL-PARSER-FAILURE-EVIDENCE".to_vec()]);
}

#[test]
fn native_resize_stall_keeps_the_latest_consistent_presentation_frame() {
    let harness = Harness::new(OwnerLimits::default(), None, None);
    harness.output(b"\x1b[2mLATEST\x1b[0m");
    harness.fence();
    let generation = harness.generation.load(Ordering::Acquire);
    let (gate, control) = gate();
    harness.control.lock().unwrap().resize_gate = Some(gate);
    let receipt = harness.input.resize(10, 20).unwrap();
    control
        .started
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let started = Instant::now();
    let observed = harness.screen_reader.with_frame(|screen, revision| {
        (
            screen.contents(),
            screen.cursor_position(),
            screen.size(),
            screen.cell(0, 0).unwrap().dim(),
            revision,
        )
    });
    let elapsed = started.elapsed();
    let current_unavailable = harness.screen_reader.try_with_frame(|_, _| ()).is_none();
    let epoch_current_unavailable = harness
        .screen_reader
        .clone()
        .try_with_screen_and_resize_epoch(|_, _| ())
        .is_none();
    control.release.send(()).unwrap();
    receipt.wait_blocking().unwrap();
    assert!(elapsed < Duration::from_millis(100));
    assert_eq!(
        observed,
        ("LATEST".into(), (0, 6), (64, 80), true, generation)
    );
    assert!(current_unavailable);
    assert!(epoch_current_unavailable);
    assert_eq!(
        harness
            .screen_reader
            .try_with_screen_and_resize_epoch(|screen, epoch| (screen.size(), epoch)),
        Some(((10, 20), 1))
    );
    assert_eq!(
        harness
            .screen_reader
            .try_with_frame(|screen, _| screen.size()),
        Some((10, 20))
    );
}

#[test]
fn resize_epoch_fences_same_size_resizes_without_rejecting_output_or_rollback() {
    let harness = Harness::new(OwnerLimits::default(), None, None);
    let current = || {
        harness
            .screen_reader
            .clone()
            .try_with_screen_and_resize_epoch(|screen, epoch| (screen.size(), epoch))
            .unwrap()
    };
    assert_eq!(current(), ((64, 80), 0));
    harness.output(b"ordinary output");
    harness.fence();
    assert_eq!(current(), ((64, 80), 0));
    harness
        .input
        .resize(64, 80)
        .unwrap()
        .wait_blocking()
        .unwrap();
    assert_eq!(current(), ((64, 80), 1));
    harness
        .input
        .resize(12, 30)
        .unwrap()
        .wait_blocking()
        .unwrap();
    harness
        .input
        .resize(64, 80)
        .unwrap()
        .wait_blocking()
        .unwrap();
    assert_eq!(current(), ((64, 80), 3));
    harness.control.lock().unwrap().fail_next = true;
    let error = harness
        .input
        .resize(10, 20)
        .unwrap()
        .wait_blocking()
        .unwrap_err();
    assert!(matches!(
        error.failure,
        DeliveryFailure::Resize { restored: true, .. }
    ));
    assert_eq!(current(), ((64, 80), 3));
    let writer = harness.parser.write().unwrap();
    assert!(harness
        .screen_reader
        .clone()
        .try_with_screen_and_resize_epoch(|_, _| ())
        .is_none());
    drop(writer);
    harness.output(b"more output");
    harness.fence();
    assert_eq!(current(), ((64, 80), 3));
}
