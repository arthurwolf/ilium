use super::*;
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
};
use std::time::Instant;

const TEST_WAIT: Duration = Duration::from_secs(10);

struct BarrierObservation {
    pane_id: NodeId,
    generation: u64,
    ordinal: u64,
    sequence: u64,
    mouse: bool,
    paste: bool,
}

struct ParserFixture {
    parsing: TerminalParsing,
    execution: Execution,
    views: HashMap<NodeId, TerminalView>,
    snapshots: HashMap<NodeId, Arc<TerminalSnapshot>>,
    completed: Vec<(NodeId, u64)>,
    evidence: Vec<(NodeId, u64, VisibleTextEvidence)>,
    errors: Vec<(NodeId, String)>,
    barriers: Vec<BarrierObservation>,
}

impl ParserFixture {
    fn start() -> Self {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 1,
            input_bytes: 128 * MIB,
            result_bytes: 2 * MIB,
            worker_threads: 1,
            worker_bytes: 641 * MIB,
        });
        let empty = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let execution = Execution::start(
            quota,
            ExecutionConfig {
                cpu: empty,
                io: empty,
                service: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: None,
                    resident_bytes_per_thread: 128 * MIB,
                },
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 1,
                service_jobs: 1,
                input_bytes: 128 * MIB,
                result_bytes: 2 * MIB,
            })
            .unwrap();

        let parsing = TerminalParsing::start(client, 256).unwrap();
        Self {
            parsing,
            execution,
            views: HashMap::new(),
            snapshots: HashMap::new(),
            completed: Vec::new(),
            evidence: Vec::new(),
            errors: Vec::new(),
            barriers: Vec::new(),
        }
    }

    fn attach(&mut self, pane_id: NodeId, rows: u16, columns: u16) {
        let mut view = TerminalView::new(rows, columns);
        let deadline = Instant::now() + TEST_WAIT;
        loop {
            if self.parsing.attach(pane_id, &mut view).is_ok() {
                break;
            }
            self.drain();
            assert!(Instant::now() < deadline, "registration did not progress");
            std::thread::yield_now();
        }
        assert!(self.views.insert(pane_id, view).is_none());
        self.wait_until("initial publication", |fixture| {
            fixture.snapshots.contains_key(&pane_id)
        });
    }

    fn offer(&mut self, pane_id: NodeId, mut command: PaneCommand) -> u64 {
        let deadline = Instant::now() + TEST_WAIT;
        loop {
            match self
                .views
                .get_mut(&pane_id)
                .unwrap()
                .frontend
                .as_mut()
                .unwrap()
                .submit(command)
            {
                Ok(ordinal) => return ordinal,
                Err(original) => command = original,
            }
            self.drain();
            assert!(
                Instant::now() < deadline,
                "command admission did not progress"
            );
            std::thread::yield_now();
        }
    }

    fn output(&mut self, pane_id: NodeId, sequence: u64, bytes: Vec<u8>, track: bool) -> u64 {
        self.offer(
            pane_id,
            PaneCommand::Output {
                first: sequence,
                sequence,
                bytes,
                track,
            },
        )
    }

    fn drain(&mut self) {
        for result in self.parsing.collect() {
            match result {
                ParseResult::Published {
                    target,
                    ordinal,
                    snapshot,
                    evidence,
                } => {
                    if let Some(view) = self.views.get_mut(&target.pane_id) {
                        assert!(Arc::ptr_eq(&view.identity, &target.identity));
                        view.install(snapshot.clone(), ordinal);
                    }
                    self.snapshots.insert(target.pane_id, snapshot);
                    self.completed.push((target.pane_id, ordinal));
                    if let Some(evidence) = evidence {
                        self.evidence.push((target.pane_id, ordinal, evidence));
                    }
                }
                ParseResult::Acknowledged {
                    target,
                    ordinal,
                    evidence,
                } => {
                    if let Some(view) = self.views.get_mut(&target.pane_id) {
                        view.applied_ordinal = view.applied_ordinal.max(ordinal);
                    }
                    self.completed.push((target.pane_id, ordinal));
                    if let Some(evidence) = evidence {
                        self.evidence.push((target.pane_id, ordinal, evidence));
                    }
                }
                ParseResult::Error { target, message } => {
                    if let Some(view) = self.views.get_mut(&target.pane_id) {
                        view.admission_error = Some(message.clone());
                    }
                    self.errors.push((target.pane_id, message));
                }
                ParseResult::InputBarrier {
                    target,
                    generation,
                    ordinal,
                    sequence,
                    mouse,
                    paste,
                } => {
                    self.barriers.push(BarrierObservation {
                        pane_id: target.pane_id,
                        generation,
                        ordinal,
                        sequence,
                        mouse,
                        paste,
                    });
                }
                ParseResult::Capture { .. } | ParseResult::CaptureFailed { .. } => {
                    panic!("this fixture did not request a capture");
                }
            }
        }
    }

    fn wait_until(&mut self, label: &str, mut ready: impl FnMut(&Self) -> bool) {
        let deadline = Instant::now() + TEST_WAIT;
        loop {
            self.drain();
            if ready(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{label} did not progress; pending={:?}; completed={:?}; errors={:?}",
                self.parsing.pending_work(),
                self.completed,
                self.errors
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn wait_sequence(&mut self, pane_id: NodeId, sequence: u64) {
        let label = format!("terminal {pane_id:?} sequence {sequence}");
        self.wait_until(&label, |fixture| {
            fixture
                .snapshots
                .get(&pane_id)
                .is_some_and(|snapshot| snapshot.sequence == sequence)
        });
    }

    fn wait_idle(&mut self) {
        self.wait_until("empty command queue", |fixture| {
            fixture.parsing.pending_work() == Some((0, 0))
        });
    }

    fn wait_pressure(&mut self, pane_id: NodeId, after: usize) {
        self.wait_until("snapshot pressure", |fixture| {
            fixture
                .errors
                .iter()
                .skip(after)
                .any(|(id, message)| *id == pane_id && message.contains("admission blocked"))
        });
    }

    fn wait_barrier(&mut self, pane_id: NodeId, generation: u64) {
        self.wait_until("ordered input barrier", |fixture| {
            fixture
                .barriers
                .iter()
                .any(|barrier| barrier.pane_id == pane_id && barrier.generation == generation)
        });
    }
}

impl Drop for ParserFixture {
    fn drop(&mut self) {
        self.parsing.cancel();
        self.execution.request_shutdown(ShutdownMode::Cancel);
        let joined = self
            .execution
            .join_until_background(Instant::now() + TEST_WAIT)
            .unwrap();
        assert_eq!(joined.remaining_workers, 0, "fixture parser did not retire");
    }
}

#[test]
fn a_small_pane_publishes_while_another_waits_and_a_large_generation_is_held() {
    let mut fixture = ParserFixture::start();
    let pane_a = NodeId(8101);
    let pane_b = NodeId(8102);
    let holder_pane = NodeId(8103);
    fixture.attach(pane_a, 24, 768);
    fixture.attach(pane_b, 3, 20);
    fixture.attach(holder_pane, 64, 512);
    fixture.output(pane_a, 1, b"A before".to_vec(), false);
    fixture.output(pane_b, 1, b"B before".to_vec(), false);
    fixture.output(holder_pane, 1, b"held generation".to_vec(), false);
    fixture.wait_sequence(pane_a, 1);
    fixture.wait_sequence(pane_b, 1);
    fixture.wait_sequence(holder_pane, 1);
    fixture.wait_idle();

    let held = fixture.views[&holder_pane]
        .try_preparation_snapshot()
        .unwrap();
    let held_bytes = held.allocation_charge.as_ref().unwrap().bytes();
    let a_bytes = fixture.snapshots[&pane_a]
        .allocation_charge
        .as_ref()
        .unwrap()
        .bytes();
    let b_bytes = fixture.snapshots[&pane_b]
        .allocation_charge
        .as_ref()
        .unwrap()
        .bytes();
    let headroom = b_bytes + 64 * 1024;
    assert!(a_bytes > headroom && held_bytes > a_bytes);
    let used = fixture
        .parsing
        .shared
        .snapshot_bytes
        .load(Ordering::Acquire);
    fixture
        .parsing
        .shared
        .snapshot_limit
        .store(used + headroom, Ordering::Release);

    let error_start = fixture.errors.len();
    let suspended_bytes = b" A after".to_vec();
    let suspended_capacity = suspended_bytes.capacity();
    let a_ordinal = fixture.output(pane_a, 2, suspended_bytes, true);
    fixture.wait_pressure(pane_a, error_start);
    fixture.wait_until("retained A command accounting", |fixture| {
        fixture.parsing.pending_work() == Some((1, suspended_capacity))
    });
    let b_ordinal = fixture.output(pane_b, 2, b" B after".to_vec(), true);

    fixture.wait_sequence(pane_b, 2);
    assert!(fixture.snapshots[&pane_b]
        .visible
        .contents()
        .contains("B after"));
    assert!(fixture.completed.contains(&(pane_b, b_ordinal)));
    assert!(!fixture.completed.contains(&(pane_a, a_ordinal)));
    assert_eq!(fixture.snapshots[&pane_a].sequence, 1);
    assert_eq!(held.visible.contents(), "held generation");
    fixture.wait_until("B completed without releasing A", |fixture| {
        fixture.parsing.pending_work() == Some((1, suspended_capacity))
    });

    drop(fixture.views.remove(&holder_pane));
    drop(fixture.snapshots.remove(&holder_pane));
    fixture.wait_until("retirement while A remains blocked", |fixture| {
        fixture.parsing.engine_claims() == 2
    });
    assert!(
        fixture
            .parsing
            .shared
            .snapshot_bytes
            .load(Ordering::Acquire)
            >= held_bytes
    );
    assert_eq!(
        fixture.parsing.shared.pin_bytes.load(Ordering::Acquire),
        held_bytes
    );
    assert!(!fixture.completed.contains(&(pane_a, a_ordinal)));
    assert_eq!(held.visible.contents(), "held generation");
    drop(held);
    assert_eq!(fixture.parsing.shared.pin_bytes.load(Ordering::Acquire), 0);

    fixture.wait_sequence(pane_a, 2);
    assert_eq!(
        fixture.snapshots[&pane_a].history.to_vec(),
        b"A before A after"
    );
    assert_eq!(
        fixture
            .completed
            .iter()
            .filter(|entry| **entry == (pane_a, a_ordinal))
            .count(),
        1
    );
    fixture.wait_idle();
}

#[test]
fn publication_retries_apply_scroll_once_and_keep_tracked_output_evidence_once() {
    let mut fixture = ParserFixture::start();
    let pane_a = NodeId(8201);
    let pane_b = NodeId(8202);
    fixture.attach(pane_a, 3, 24);
    fixture.attach(pane_b, 3, 20);
    let original = (0..20)
        .map(|line| format!("line {line:02}\r\n"))
        .collect::<String>()
        .into_bytes();
    fixture.output(pane_a, 1, original.clone(), true);
    fixture.wait_sequence(pane_a, 1);
    fixture.wait_idle();
    let normal_limit = fixture
        .parsing
        .shared
        .snapshot_limit
        .load(Ordering::Acquire);
    let used = fixture
        .parsing
        .shared
        .snapshot_bytes
        .load(Ordering::Acquire);
    fixture
        .parsing
        .shared
        .snapshot_limit
        .store(used, Ordering::Release);
    let error_start = fixture.errors.len();
    let scroll_ordinal = fixture.offer(pane_a, PaneCommand::ScrollUp(3));
    fixture.wait_pressure(pane_a, error_start);
    for generation in 1..=3 {
        fixture.offer(pane_b, PaneCommand::InputBarrier { generation });
        fixture.wait_barrier(pane_b, generation);
    }
    assert!(!fixture.completed.contains(&(pane_a, scroll_ordinal)));
    assert_eq!(fixture.snapshots[&pane_a].scrollback_position, 0);
    fixture
        .parsing
        .shared
        .snapshot_limit
        .store(normal_limit, Ordering::Release);
    fixture.parsing.shared.changed.notify_all();
    fixture.wait_until("one scroll publication", |fixture| {
        fixture.completed.contains(&(pane_a, scroll_ordinal))
    });
    assert_eq!(fixture.snapshots[&pane_a].scrollback_position, 3);
    assert_eq!(
        fixture
            .completed
            .iter()
            .filter(|entry| **entry == (pane_a, scroll_ordinal))
            .count(),
        1
    );
    fixture.wait_idle();

    let used = fixture
        .parsing
        .shared
        .snapshot_bytes
        .load(Ordering::Acquire);
    fixture
        .parsing
        .shared
        .snapshot_limit
        .store(used, Ordering::Release);
    let error_start = fixture.errors.len();
    let output_ordinal = fixture.output(pane_a, 2, b"evidence marker".to_vec(), true);
    fixture.wait_pressure(pane_a, error_start);
    for generation in 4..=6 {
        fixture.offer(pane_b, PaneCommand::InputBarrier { generation });
        fixture.wait_barrier(pane_b, generation);
    }
    assert!(!fixture.completed.contains(&(pane_a, output_ordinal)));
    fixture
        .parsing
        .shared
        .snapshot_limit
        .store(normal_limit, Ordering::Release);
    fixture.parsing.shared.changed.notify_all();
    fixture.wait_sequence(pane_a, 2);
    let evidence: Vec<_> = fixture
        .evidence
        .iter()
        .filter(|(id, ordinal, _)| *id == pane_a && *ordinal == output_ordinal)
        .collect();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].2.first_sequence, 2);
    assert_eq!(evidence[0].2.sequence, 2);
    assert!(evidence[0]
        .2
        .rows
        .iter()
        .any(|row| row.text.contains("evidence marker")));
    let mut expected = original;
    expected.extend_from_slice(b"evidence marker");
    assert_eq!(fixture.snapshots[&pane_a].history.to_vec(), expected);
    assert_eq!(
        fixture
            .completed
            .iter()
            .filter(|entry| **entry == (pane_a, output_ordinal))
            .count(),
        1
    );
}

#[test]
fn output_and_replay_yield_to_other_publications_without_reordering_later_actions() {
    let mut fixture = ParserFixture::start();
    let pane_a = NodeId(8301);
    let pane_b = NodeId(8302);
    fixture.attach(pane_a, 3, 20);
    fixture.attach(pane_b, 3, 20);
    fixture.output(pane_a, 1, b"old A\r\n".to_vec(), false);
    fixture.output(pane_b, 1, b"old B".to_vec(), false);
    fixture.wait_sequence(pane_a, 1);
    fixture.wait_sequence(pane_b, 1);
    fixture.wait_idle();
    let old_origin = fixture.snapshots[&pane_a].history.origin.clone();
    let mut output = vec![b'o'; 64 * 1024];
    output.extend_from_slice(b"\r\noutput end\x1b[?1000h\x1b[?2004h");

    let shared = fixture.parsing.shared.clone();
    let results_guard = shared.results.lock().unwrap();
    fixture.offer(pane_a, PaneCommand::Fingerprint);
    fixture.wait_until("held owner", |_| {
        shared
            .queue
            .try_lock()
            .is_ok_and(|queue| queue.active_commands == 1 && queue.commands.is_empty())
    });
    let output_ordinal = fixture.output(pane_a, 2, output.clone(), true);
    let b_ordinal = fixture.output(pane_b, 2, b" B during output".to_vec(), false);
    let barrier_ordinal = fixture.offer(pane_a, PaneCommand::InputBarrier { generation: 71 });
    drop(results_guard);
    fixture.wait_sequence(pane_a, 2);
    fixture.wait_sequence(pane_b, 2);
    fixture.wait_barrier(pane_a, 71);
    let b_position = fixture
        .completed
        .iter()
        .position(|entry| *entry == (pane_b, b_ordinal))
        .unwrap();
    let a_position = fixture
        .completed
        .iter()
        .position(|entry| *entry == (pane_a, output_ordinal))
        .unwrap();
    assert!(
        b_position < a_position,
        "one large output monopolized the owner"
    );
    let mut expected = b"old A\r\n".to_vec();
    expected.extend_from_slice(&output);
    assert_eq!(fixture.snapshots[&pane_a].history.to_vec(), expected);
    let barrier = fixture
        .barriers
        .iter()
        .find(|barrier| barrier.generation == 71)
        .unwrap();
    assert_eq!(
        (
            barrier.ordinal,
            barrier.sequence,
            barrier.mouse,
            barrier.paste
        ),
        (barrier_ordinal, 2, true, true)
    );
    fixture.wait_idle();

    let prefix = b"first\r\nneedle in journal\r\n";
    let mut replay = prefix.to_vec();
    replay.extend(std::iter::repeat_n(b'r', 64 * 1024));
    replay.extend_from_slice(b"\r\nreplay end\x1b[?1000h\x1b[?2004h");
    let results_guard = shared.results.lock().unwrap();
    fixture.offer(pane_a, PaneCommand::Fingerprint);
    fixture.wait_until("held replay owner", |_| {
        shared
            .queue
            .try_lock()
            .is_ok_and(|queue| queue.active_commands == 1 && queue.commands.is_empty())
    });
    let replay_ordinal = fixture.offer(
        pane_a,
        PaneCommand::Replay {
            sequence: 7,
            bytes: replay.clone(),
            complete: true,
        },
    );
    let b_ordinal = fixture.output(pane_b, 3, b" B during replay".to_vec(), false);
    fixture.offer(pane_a, PaneCommand::Resize(5, 26));
    let history_ordinal = fixture.offer(pane_a, PaneCommand::History(prefix.len()));
    let barrier_ordinal = fixture.offer(pane_a, PaneCommand::InputBarrier { generation: 72 });
    drop(results_guard);
    fixture.wait_sequence(pane_b, 3);
    fixture.wait_barrier(pane_a, 72);
    let b_position = fixture
        .completed
        .iter()
        .position(|entry| *entry == (pane_b, b_ordinal))
        .unwrap();
    let a_position = fixture
        .completed
        .iter()
        .position(|entry| *entry == (pane_a, replay_ordinal))
        .unwrap();
    assert!(
        b_position < a_position,
        "one large replay monopolized the owner"
    );
    let snapshot = &fixture.snapshots[&pane_a];
    assert_eq!(snapshot.sequence, 7);
    assert_eq!(snapshot.history.to_vec(), replay);
    assert!(!Arc::ptr_eq(&snapshot.history.origin, &old_origin));
    assert_eq!(snapshot.visible.size(), (5, 26));
    assert!(snapshot.scrolled_back);
    assert!(snapshot.visible.contents().contains("needle in journal"));
    assert!(!snapshot.visible.contents().contains("replay end"));
    assert!(fixture.completed.contains(&(pane_a, history_ordinal)));
    let barrier = fixture
        .barriers
        .iter()
        .find(|barrier| barrier.generation == 72)
        .unwrap();
    assert_eq!(
        (
            barrier.ordinal,
            barrier.sequence,
            barrier.mouse,
            barrier.paste
        ),
        (barrier_ordinal, 7, true, true)
    );
    fixture.offer(pane_a, PaneCommand::Bottom);
    fixture.wait_until("return to replay tail", |fixture| {
        !fixture.snapshots[&pane_a].scrolled_back
    });
    assert!(fixture.snapshots[&pane_a]
        .visible
        .contents()
        .contains("replay end"));
}

#[test]
fn intact_large_output_publishes_visible_progress_before_its_final_snapshot() {
    let mut fixture = ParserFixture::start();
    let pane = NodeId(8303);
    fixture.attach(pane, 24, 80);
    fixture.output(pane, 1, b"before large output".to_vec(), false);
    fixture.wait_sequence(pane, 1);
    fixture.wait_idle();

    // Hold the final result mailbox so the test can observe a genuine in-flight
    // parser preview instead of racing with the completed immutable snapshot.
    let shared = fixture.parsing.shared.clone();
    let results_guard = shared.results.lock().unwrap();
    let mut output = vec![b'x'; 250 * 1024];
    let marker = b"PREVIEW-AT-QUANTUM";
    let marker_start = 16 * 1024 - marker.len();
    output[marker_start..marker_start + marker.len()].copy_from_slice(marker);
    let ordinal = fixture.output(pane, 2, output, false);

    let deadline = Instant::now() + TEST_WAIT;
    loop {
        let preview = fixture.views[&pane]
            .frontend
            .as_ref()
            .unwrap()
            .progress_preview();
        if let Some(preview) = preview {
            assert!(
                preview.visible.contents().contains("PREVIEW-AT-QUANTUM"),
                "the first output preview must contain bytes already parsed at its voluntary yield"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "large intact Output reached its first yield without publishing a visible preview"
        );
        std::thread::yield_now();
    }

    assert_eq!(fixture.snapshots[&pane].sequence, 1);
    assert!(!fixture.completed.contains(&(pane, ordinal)));
    drop(results_guard);

    fixture.wait_sequence(pane, 2);
    assert!(fixture.snapshots[&pane]
        .visible
        .contents()
        .contains("PREVIEW-AT-QUANTUM"));
    assert_eq!(
        fixture
            .completed
            .iter()
            .filter(|entry| **entry == (pane, ordinal))
            .count(),
        1,
        "the final snapshot must complete the original output ordinal exactly once"
    );
    fixture.wait_idle();
}

#[test]
fn a_history_match_at_128_columns_publishes_and_preserves_the_live_journal() {
    let mut fixture = ParserFixture::start();
    let pane = NodeId(8401);
    fixture.attach(pane, 4, 128);
    let prefix = b"first line\r\nneedle at 128 columns\r\n";
    let mut journal = prefix.to_vec();
    for line in 0..20 {
        journal.extend_from_slice(format!("later line {line}\r\n").as_bytes());
    }
    journal.extend_from_slice(b"live tail");
    fixture.output(pane, 1, journal.clone(), false);
    fixture.wait_sequence(pane, 1);
    fixture.wait_idle();

    let ordinal = fixture.offer(pane, PaneCommand::History(prefix.len()));
    fixture.wait_until("128-column history publication", |fixture| {
        fixture.completed.contains(&(pane, ordinal))
    });
    let snapshot = &fixture.snapshots[&pane];
    assert!(snapshot.scrolled_back);
    assert!(snapshot
        .visible
        .contents()
        .contains("needle at 128 columns"));
    assert!(!snapshot.visible.contents().contains("live tail"));
    assert_eq!(snapshot.history.to_vec(), journal);
    assert_eq!(snapshot.visible.size(), (4, 128));
    assert_eq!(snapshot.sequence, 1);
    assert_eq!(
        fixture
            .completed
            .iter()
            .filter(|entry| **entry == (pane, ordinal))
            .count(),
        1
    );
    let frozen_contents = snapshot.visible.contents();

    let resize_ordinal = fixture.offer(pane, PaneCommand::Resize(6, 192));
    fixture.wait_until("resize behind historical viewport", |fixture| {
        fixture.completed.contains(&(pane, resize_ordinal))
    });
    assert_eq!(fixture.snapshots[&pane].visible.size(), (4, 128));
    assert_eq!(fixture.snapshots[&pane].visible.contents(), frozen_contents);
    let replay = b"replacement journal at 192 columns\r\nnew live tail".to_vec();
    fixture.offer(
        pane,
        PaneCommand::Replay {
            sequence: 2,
            bytes: replay.clone(),
            complete: true,
        },
    );
    fixture.wait_sequence(pane, 2);
    assert_eq!(fixture.snapshots[&pane].visible.size(), (4, 128));
    assert_eq!(fixture.snapshots[&pane].visible.contents(), frozen_contents);
    assert_eq!(fixture.snapshots[&pane].history.to_vec(), replay);

    fixture.offer(pane, PaneCommand::Bottom);
    fixture.wait_until("resized replay tail", |fixture| {
        !fixture.snapshots[&pane].scrolled_back
    });
    assert_eq!(fixture.snapshots[&pane].visible.size(), (6, 192));
    assert!(fixture.snapshots[&pane]
        .visible
        .contents()
        .contains("new live tail"));
    assert_eq!(fixture.snapshots[&pane].history.to_vec(), replay);
}

#[test]
fn blank_registration_snapshot_does_not_count_as_initial_content() {
    let mut fixture = ParserFixture::start();
    let pane = NodeId(8501);
    fixture.attach(pane, 4, 30);

    let view = &fixture.views[&pane];
    assert_eq!(view.try_preparation_snapshot().unwrap().sequence, 0);
    assert!(!view.has_initial_display());

    fixture.output(pane, 1, b"first terminal output".to_vec(), false);
    fixture.wait_sequence(pane, 1);
    assert!(fixture.views[&pane].has_initial_display());
}

#[test]
fn large_intact_delta_publishes_first_content_before_final_ordinal() {
    let mut fixture = ParserFixture::start();
    let pane = NodeId(8502);
    fixture.attach(pane, 4, 30);
    let final_ordinal = fixture.output(pane, 1, vec![b'x'; 8 * MIB], false);

    let deadline = Instant::now() + TEST_WAIT;
    let mut saw_partial_content = false;
    while Instant::now() < deadline {
        fixture.drain();
        let pending = fixture.parsing.pending_work().unwrap_or_default();
        let view = &fixture.views[&pane];
        if pending.0 > 0
            && view.applied_ordinal < final_ordinal
            && !view
                .with_screen(|screen| screen.contents())
                .trim()
                .is_empty()
        {
            saw_partial_content = true;
            break;
        }
        std::thread::yield_now();
    }
    assert!(
        saw_partial_content,
        "large intact Delta stayed blank until its final publication"
    );

    fixture.wait_sequence(pane, 1);
    assert_eq!(fixture.views[&pane].applied_ordinal, final_ordinal);
    assert!(fixture.views[&pane]
        .with_screen(|screen| screen.contents())
        .contains('x'));
}
