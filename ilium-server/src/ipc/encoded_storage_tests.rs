//! Exercises the actual shared encoder and FrameWriter with blocked transport
//! flushes. This is a component regression, not a real-socket or TUI proof.
use super::*;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    time::Duration,
};
use tokio::sync::Notify;

#[derive(Default)]
struct FlushGateState {
    released: bool,
    fail: bool,
    waiter: Option<Waker>,
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct FlushGate {
    state: Arc<Mutex<FlushGateState>>,
    entered: Arc<Notify>,
}
impl FlushGate {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(FlushGateState::default())),
            entered: Arc::new(Notify::new()),
        }
    }
    fn fail(&self) {
        self.state.lock().unwrap().fail = true;
        self.release();
    }
    fn release(&self) {
        let waiter = {
            let mut state = self.state.lock().expect("flush state");
            state.released = true;
            state.waiter.take()
        };
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }
}
struct ReleaseOnDrop([FlushGate; 2]);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        for gate in &self.0 {
            gate.release();
        }
    }
}
impl AsyncWrite for FlushGate {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.state
            .lock()
            .expect("flush state")
            .bytes
            .extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let mut state = self.state.lock().expect("flush state");
        if state.released {
            return Poll::Ready(if state.fail {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "controlled physical flush failure",
                ))
            } else {
                Ok(())
            });
        }
        state.waiter = Some(context.waker().clone());
        self.entered.notify_one();
        Poll::Pending
    }
    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.poll_flush(context)
    }
}

#[tokio::test]
async fn two_blocked_flushes_do_not_hold_all_encoder_jobs_or_claim_delivery() {
    let fixture = Fixture::new();
    let quota = fixture.quota.clone();
    let before_storage = fixture.baseline;
    let first_gate = FlushGate::new();
    let second_gate = FlushGate::new();
    let release = ReleaseOnDrop([first_gate.clone(), second_gate.clone()]);
    let first_gate_for_writer = first_gate.clone();
    let second_gate_for_writer = second_gate.clone();
    let writer = |gate, pane, state: Arc<ServerState>| async move {
        let mut frame_writer = FrameWriter::new(gate);
        let mut delivered = HashMap::new();
        let event = ServerEvent::ScreenUpdate {
            pane_id: ilium_core::NodeId(pane),
            first_sequence: 1,
            sequence: 1,
            bytes: b"ordered-output".to_vec(),
        };
        let result =
            write_server_event(&mut frame_writer, event, &mut delivered, Some(&state)).await;
        (result, delivered)
    };
    let first = tokio::spawn(writer(first_gate_for_writer, 1, fixture.state.clone()));
    let second = tokio::spawn(writer(second_gate_for_writer, 2, fixture.state.clone()));
    tokio::time::timeout(Duration::from_secs(5), async {
        first_gate.entered.notified().await;
        second_gate.entered.notified().await;
    })
    .await
    .expect("both actual frame writers must reach blocked flush");
    assert!(
        !first.is_finished() && !second.is_finished(),
        "no flush acknowledgement before release"
    );
    let held_jobs = fixture.encoder.foundation.usage().jobs;
    let held_storage = quota.snapshot().worker_bytes;
    let held_wire_bytes = first_gate.state.lock().unwrap().bytes.len()
        + second_gate.state.lock().unwrap().bytes.len();
    let mut third_writer = FrameWriter::new(tokio::io::sink());
    let mut third_delivered = HashMap::new();
    let third = tokio::time::timeout(
        Duration::from_secs(1),
        write_server_event(
            &mut third_writer,
            ServerEvent::Error {
                message: "small reply".to_owned(),
            },
            &mut third_delivered,
            Some(&fixture.state),
        ),
    )
    .await;
    // Settle the actual blocked tasks before the discriminating assertion,
    // including on the original RED path. A test failure must not strand them.
    drop(release);
    let (first, second) = tokio::time::timeout(Duration::from_secs(5), async {
        (
            first.await.expect("first writer task"),
            second.await.expect("second writer task"),
        )
    })
    .await
    .expect("released writers must finish");
    first.0.expect("first flush");
    second.0.expect("second flush");
    assert_eq!(first.1.get(&ilium_core::NodeId(1)), Some(&1));
    assert_eq!(second.1.get(&ilium_core::NodeId(2)), Some(&1));
    assert!(
        third.is_ok(),
        "two blocked transport flushes must not starve a third encoder job"
    );
    third.unwrap().expect("third frame flush");
    assert!(
        held_storage >= before_storage + held_wire_bytes,
        "blocked transport frames retain shared byte admission"
    );
    fixture.wait_baseline().await;
    assert_eq!(
        quota.snapshot().worker_bytes,
        before_storage,
        "actual frame storage releases after both flushes"
    );
    assert_eq!(
        held_jobs, 0,
        "socket-held frames must release finite encoder job credit"
    );
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    fixture.finish().await;
}

#[derive(Default)]
struct FailedFlush {
    bytes: Vec<u8>,
}
impl AsyncWrite for FailedFlush {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.bytes.extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "controlled physical flush failure",
        )))
    }
    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.poll_flush(context)
    }
}

#[tokio::test]
async fn failed_actual_flush_releases_stored_bytes_without_delivery_acknowledgement() {
    let fixture = Fixture::new();
    let client = fixture.encoder.clone();
    let quota = fixture.quota.clone();
    let before = fixture.baseline;
    let mut writer = FrameWriter::new(FailedFlush::default());
    let mut delivered = HashMap::new();
    let result = write_server_event(
        &mut writer,
        ServerEvent::ScreenUpdate {
            pane_id: ilium_core::NodeId(9),
            first_sequence: 1,
            sequence: 1,
            bytes: b"original failed-output bytes".to_vec(),
        },
        &mut delivered,
        Some(&fixture.state),
    )
    .await;
    assert!(result.is_err(), "actual FrameWriter flush must fail");
    assert!(
        delivered.is_empty(),
        "failed flush cannot claim emitted terminal sequence"
    );
    fixture.wait_baseline().await;
    assert_eq!(client.foundation.usage().jobs, 0);
    assert_eq!(
        quota.snapshot().worker_bytes,
        before,
        "failed transport releases actual stored frame admission"
    );
    fixture.finish().await;
}

// Synthetic observation only: these hooks are attached to the production
// StoredServerEvent, never a stand-in payload. Pointer checks track the real
// event Vec and the real EncodedFrame field in its stable retirement envelope.
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
const WAIT: Duration = Duration::from_secs(5);
static NEXT_PANE: AtomicU64 = AtomicU64::new(8_000_000);
static ORIGINAL_NOTICES: std::sync::OnceLock<
    Mutex<HashMap<ilium_core::NodeId, Arc<OriginalProbe>>>,
> = std::sync::OnceLock::new();

struct OriginalProbe {
    event_pointer: usize,
    frame_address: AtomicUsize,
    phase: AtomicU8,
    changed: Notify,
    destroyed: Mutex<Option<oneshot::Sender<OriginalDestruction>>>,
    before_publish: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}
impl OriginalProbe {
    fn advance(&self, phase: u8) {
        self.phase.store(phase, Ordering::Release);
        self.changed.notify_waiters();
    }
    async fn wait(&self, phase: u8) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.phase.load(Ordering::Acquire) >= phase {
                return;
            }
            notified.await;
        }
    }
}
#[derive(Debug)]
struct OriginalDestruction {
    thread: std::thread::ThreadId,
    is_cpu_thread: bool,
    event_pointer: usize,
    frame_address: usize,
    worker_bytes: usize,
    encoder_jobs: usize,
    encoder_input_bytes: usize,
    encoder_result_bytes: usize,
}
pub(super) struct OriginalDropNotice {
    probe: Arc<OriginalProbe>,
    client: crate::execution::ExecutionClient,
    observation: Option<OriginalDestruction>,
}
impl OriginalDropNotice {
    pub(super) fn before_original_drop(
        &mut self,
        event: &ServerEvent,
        frame: Option<&ilium_ipc::EncodedFrame>,
    ) {
        self.observation = Some(OriginalDestruction {
            thread: std::thread::current().id(),
            is_cpu_thread: std::thread::current()
                .name()
                .is_some_and(|name| name.starts_with("ilium-exec-cpu-")),
            event_pointer: event_pointer(event),
            frame_address: frame.map_or(0, |frame| frame as *const _ as usize),
            worker_bytes: self.client.foundation.quota_group().snapshot().worker_bytes,
            encoder_jobs: self.client.foundation.usage().jobs,
            encoder_input_bytes: self.client.foundation.usage().input_bytes,
            encoder_result_bytes: self.client.foundation.usage().result_bytes,
        });
    }
}
impl Drop for OriginalDropNotice {
    fn drop(&mut self) {
        // StoredServerEvent fields before this notice have now been destroyed.
        // Their Retiring envelope's charges still exist outside the payload.
        if let Some(sender) = self.probe.destroyed.lock().unwrap().take() {
            let _ = sender.send(self.observation.take().expect("original drop was observed"));
        }
    }
}
fn event_pointer(event: &ServerEvent) -> usize {
    match event {
        ServerEvent::ScreenUpdate { bytes, .. } => bytes.as_ptr() as usize,
        _ => panic!("synthetic original probe only registers ScreenUpdate"),
    }
}
pub(super) fn take_original_notice(
    event: &ServerEvent,
    client: &crate::execution::ExecutionClient,
) -> Option<OriginalDropNotice> {
    let ServerEvent::ScreenUpdate { pane_id, .. } = event else {
        return None;
    };
    let probe = ORIGINAL_NOTICES.get()?.lock().unwrap().remove(pane_id)?;
    Some(OriginalDropNotice {
        probe,
        client: client.clone(),
        observation: None,
    })
}
pub(super) fn observe_encoded_original(stored: &StoredServerEvent) {
    if let Some(notice) = &stored.drop_notice {
        assert_eq!(event_pointer(&stored.event), notice.probe.event_pointer);
        let frame = stored.frame.as_ref().expect("encoded original");
        notice
            .probe
            .frame_address
            .store(frame as *const _ as usize, Ordering::Release);
        let gate = notice.probe.before_publish.lock().unwrap().take();
        notice.probe.advance(1);
        if let Some(gate) = gate {
            let _ = gate.recv();
        }
    }
}
pub(super) fn observe_storage_wait(stored: &StoredServerEvent) {
    if let Some(notice) = &stored.drop_notice {
        notice.probe.advance(2);
    }
}
struct ProbeRegistration(ilium_core::NodeId);
impl Drop for ProbeRegistration {
    fn drop(&mut self) {
        if let Some(notices) = ORIGINAL_NOTICES.get() {
            notices.lock().unwrap().remove(&self.0);
        }
    }
}
fn observed_event(
    bytes: usize,
) -> (
    ServerEvent,
    Arc<OriginalProbe>,
    oneshot::Receiver<OriginalDestruction>,
    ProbeRegistration,
) {
    let pane_id = ilium_core::NodeId(NEXT_PANE.fetch_add(1, Ordering::Relaxed));
    let event = ServerEvent::ScreenUpdate {
        pane_id,
        first_sequence: 1,
        sequence: 1,
        bytes: vec![0x5a; bytes],
    };
    let (destroyed, receiver) = oneshot::channel();
    let probe = Arc::new(OriginalProbe {
        event_pointer: event_pointer(&event),
        frame_address: AtomicUsize::new(0),
        phase: AtomicU8::new(0),
        changed: Notify::new(),
        destroyed: Mutex::new(Some(destroyed)),
        before_publish: Mutex::new(None),
    });
    ORIGINAL_NOTICES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap()
        .insert(pane_id, probe.clone());
    (event, probe, receiver, ProbeRegistration(pane_id))
}
async fn drive_to_phase<F: std::future::Future>(
    future: &mut Pin<Box<F>>,
    probe: &OriginalProbe,
    phase: u8,
) {
    tokio::time::timeout(WAIT, async {
        tokio::select! {
            biased;
            () = probe.wait(phase) => {}
            _ = future.as_mut() => panic!("writer returned before expected phase {phase}"),
        }
    })
    .await
    .expect("production writer did not reach expected phase");
}

/// Each regression owns a real server bank. Parallel tests cannot perturb its
/// root baseline or occupy its two encoder credits via the shared test bank.
struct Fixture {
    state: Arc<ServerState>,
    sound: tokio::task::JoinHandle<()>,
    encoder: crate::execution::ExecutionClient,
    general: crate::execution::ExecutionClient,
    quota: ilium_execution::QuotaGroup,
    monitor: ilium_execution::ExecutionMonitor,
    baseline: usize,
}
impl Fixture {
    fn new() -> Self {
        let owner = crate::execution::ServerExecution::start().expect("private real server bank");
        let encoder = owner.encoder.clone();
        let general = owner.client.clone();
        let quota = owner.quota_group();
        let monitor = owner.test_monitor();
        let (sound_requests, sound) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "encoded-retirement-synthetic-fixture".into(),
            session_cwd: std::env::temp_dir(),
            home_dir: std::env::temp_dir(),
            snapshot_path: std::env::temp_dir().join("unused-encoded-retirement.snapshot"),
            socket_path: std::env::temp_dir().join("unused-encoded-retirement.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: false,
        }));
        assert!(state.execution.set(owner).is_ok());
        let baseline = quota.snapshot().worker_bytes;
        Self {
            state,
            sound,
            encoder,
            general,
            quota,
            monitor,
            baseline,
        }
    }
    async fn wait_baseline(&self) {
        let notification = self.encoder.completion_notification();
        tokio::time::timeout(WAIT, async {
            loop {
                let notified = notification.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.quota.snapshot().worker_bytes == self.baseline
                    && self.encoder.foundation.usage().jobs == 0
                    && self.general.foundation.usage().jobs == 0
                {
                    return;
                }
                notified.await;
            }
        })
        .await
        .expect("original storage and finite credit did not release");
    }
    fn take_owner(&mut self) -> crate::execution::ServerExecution {
        Arc::get_mut(&mut self.state)
            .expect("fixture state not shared after writer ends")
            .execution
            .take()
            .expect("fixture owns execution")
    }
    async fn finish(mut self) {
        if self.encoder.foundation.is_open() {
            self.wait_baseline().await;
        }
        let mut owner = self.take_owner();
        owner.request_shutdown();
        let report = tokio::task::spawn_blocking(move || {
            owner.test_join_until_background(std::time::Instant::now() + WAIT)
        })
        .await
        .unwrap()
        .unwrap();
        assert!(
            report.shutdown_complete,
            "physical shutdown retains an original"
        );
        assert_eq!(report.remaining_workers, 0);
        assert_eq!(report.health.lanes[0].retirement_live, 0);
        assert_eq!(self.quota.snapshot().worker_threads, 0);
        self.sound.abort();
        let _ = (&mut self.sound).await;
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.sound.abort();
        if let Some(owner) = self.state.execution.get() {
            owner.request_shutdown();
        }
    }
}

struct BlockedCpu {
    releases: Vec<std::sync::mpsc::Sender<()>>,
    threads: Vec<std::thread::ThreadId>,
}
impl Drop for BlockedCpu {
    fn drop(&mut self) {
        // Disconnecting also releases every task on panic/cancellation.
        self.releases.clear();
    }
}
async fn block_cpu(fixture: &Fixture) -> BlockedCpu {
    let mut blockers = BlockedCpu {
        releases: Vec::new(),
        threads: Vec::new(),
    };
    for _ in 0..2 {
        let (release, released) = std::sync::mpsc::channel();
        let (started, entered) = oneshot::channel();
        let reservation = fixture
            .general
            .reserve(
                ilium_execution::Lane::Cpu,
                ilium_execution::JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
            )
            .await
            .unwrap();
        let receipt = reservation
            .submit(move |_| {
                let _ = started.send(std::thread::current().id());
                let _ = released.recv();
                Ok::<(), std::convert::Infallible>(())
            })
            .unwrap();
        blockers.releases.push(release);
        blockers
            .threads
            .push(tokio::time::timeout(WAIT, entered).await.unwrap().unwrap());
        // Receipt drop does not cancel this callback; its original job hold
        // remains until the controlled native barrier actually returns.
        drop(receipt);
    }
    assert_ne!(blockers.threads[0], blockers.threads[1]);
    blockers
}
async fn assert_original_destroyed(
    receiver: oneshot::Receiver<OriginalDestruction>,
    probe: &OriginalProbe,
    cpu_threads: &[std::thread::ThreadId],
    charged: usize,
    expected_encoder_jobs: usize,
) {
    let notice = tokio::time::timeout(WAIT, receiver).await.unwrap().unwrap();
    assert!(notice.is_cpu_thread);
    assert!(cpu_threads.contains(&notice.thread));
    assert_ne!(notice.thread, std::thread::current().id());
    assert_eq!(
        notice.event_pointer, probe.event_pointer,
        "same original event Vec"
    );
    assert_eq!(
        notice.frame_address,
        probe.frame_address.load(Ordering::Acquire),
        "same encoded frame field"
    );
    assert_ne!(notice.frame_address, 0);
    assert_eq!(
        notice.worker_bytes, charged,
        "shared bytes survive actual original destruction"
    );
    assert_eq!(
        notice.encoder_jobs, expected_encoder_jobs,
        "finite debit disposition at destruction"
    );
    assert_eq!(
        notice.encoder_input_bytes,
        expected_encoder_jobs * SERVER_CODEC_COST.input_bytes,
        "original input debit survives until CPU destruction before storage transfer"
    );
    assert_eq!(
        notice.encoder_result_bytes,
        expected_encoder_jobs * SERVER_CODEC_COST.result_bytes,
        "original result debit survives until CPU destruction before storage transfer"
    );
}

#[derive(Clone, Copy)]
enum FlushOutcome {
    Success,
    Failure,
    Cancel,
}
async fn assert_flush_retirement(outcome: FlushOutcome) {
    let fixture = Fixture::new();
    let (event, probe, mut destroyed, _registration) = observed_event(32768);
    let pane = match &event {
        ServerEvent::ScreenUpdate { pane_id, .. } => *pane_id,
        _ => unreachable!(),
    };
    let gate = FlushGate::new();
    let mut writer = FrameWriter::new(gate.clone());
    let mut delivered = HashMap::new();
    let mut pending = Box::pin(write_server_event(
        &mut writer,
        event,
        &mut delivered,
        Some(&fixture.state),
    ));
    tokio::time::timeout(WAIT, async {
        tokio::select! {
            () = gate.entered.notified() => {}
            _ = pending.as_mut() => panic!("flush gate was bypassed"),
        }
    })
    .await
    .unwrap();
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    let blockers = block_cpu(&fixture).await;
    let threads = blockers.threads.clone();
    let charged = fixture.quota.snapshot().worker_bytes;
    assert!(charged > fixture.baseline);
    match outcome {
        FlushOutcome::Success => {
            gate.release();
            pending.await.unwrap();
        }
        FlushOutcome::Failure => {
            gate.fail();
            assert!(pending.await.is_err());
        }
        FlushOutcome::Cancel => drop(pending),
    }
    assert_eq!(
        delivered.get(&pane),
        matches!(outcome, FlushOutcome::Success).then_some(&1)
    );
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    assert_eq!(fixture.quota.snapshot().worker_bytes, charged);
    assert!(matches!(
        destroyed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert_eq!(fixture.monitor.health().lanes[0].retirement_queued, 1);
    drop(blockers);
    assert_original_destroyed(destroyed, &probe, &threads, charged, 0).await;
    fixture.finish().await;
}
#[tokio::test]
async fn successful_actual_flush_retires_originals_on_blocked_cpu_after_ack() {
    assert_flush_retirement(FlushOutcome::Success).await;
}
#[tokio::test]
async fn failed_actual_flush_retires_originals_on_blocked_cpu_without_ack() {
    assert_flush_retirement(FlushOutcome::Failure).await;
}
#[tokio::test]
async fn cancelled_actual_flush_retires_originals_on_blocked_cpu_without_ack() {
    assert_flush_retirement(FlushOutcome::Cancel).await;
}

#[tokio::test]
async fn storage_wait_cancellation_keeps_original_codec_debit_until_cpu_destruction() {
    let fixture = Fixture::new();
    // Leave room for preallocated envelope metadata, but not this actual
    // multi-megabyte event plus encoded frame. No producer budget is simulated.
    let snapshot = fixture.quota.snapshot();
    let occupied = fixture
        .general
        .reserve_storage(snapshot.limits.worker_bytes - snapshot.worker_bytes - 64 * 1024)
        .await
        .unwrap();
    let (event, probe, mut destroyed, _registration) = observed_event(2 * 1024 * 1024);
    let gate = FlushGate::new();
    let mut writer = FrameWriter::new(gate.clone());
    let mut delivered = HashMap::new();
    let mut pending = Box::pin(write_server_event(
        &mut writer,
        event,
        &mut delivered,
        Some(&fixture.state),
    ));
    drive_to_phase(&mut pending, &probe, 2).await;
    assert!(
        gate.state.lock().unwrap().bytes.is_empty(),
        "no socket byte before storage admission"
    );
    assert_eq!(fixture.encoder.foundation.usage().jobs, 1);
    let blockers = block_cpu(&fixture).await;
    let threads = blockers.threads.clone();
    let charged = fixture.quota.snapshot().worker_bytes;
    drop(pending);
    assert!(delivered.is_empty());
    assert_eq!(
        fixture.encoder.foundation.usage().jobs,
        1,
        "CPU retirement still owns original codec debit"
    );
    assert_eq!(fixture.quota.snapshot().worker_bytes, charged);
    assert!(matches!(
        destroyed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert_eq!(fixture.monitor.health().lanes[0].retirement_queued, 1);
    drop(blockers);
    assert_original_destroyed(destroyed, &probe, &threads, charged, 1).await;
    drop(occupied);
    fixture.finish().await;
}

#[tokio::test]
async fn unconsumed_encoded_receipt_keeps_original_debit_until_blocked_cpu_retirement() {
    let fixture = Fixture::new();
    let (event, probe, mut destroyed, _registration) = observed_event(65536);
    let (allow_publication, before_publication) = std::sync::mpsc::channel();
    *probe.before_publish.lock().unwrap() = Some(before_publication);
    let mut writer = FrameWriter::new(tokio::io::sink());
    let mut delivered = HashMap::new();
    let mut pending = Box::pin(write_server_event(
        &mut writer,
        event,
        &mut delivered,
        Some(&fixture.state),
    ));
    // Poll only until the encode job has been submitted. run_reserved cannot
    // observe its result on this current-thread test while this future is idle.
    // Busy/metadata admission can yield first, so repeat only until the real
    // input attaches; the registry removal is the precise attachment witness.
    tokio::time::timeout(WAIT, async {
        loop {
            std::future::poll_fn(|context| {
                assert!(
                    pending.as_mut().poll(context).is_pending(),
                    "writer finished before receipt fixture armed"
                );
                Poll::Ready(())
            })
            .await;
            if !ORIGINAL_NOTICES
                .get()
                .unwrap()
                .lock()
                .unwrap()
                .contains_key(&_registration.0)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(WAIT, probe.wait(1)).await.unwrap();
    drop(allow_publication);
    // Taking both real CPU owners proves the encoding callback returned and
    // published (or attempted publication). Never poll the writer again.
    let blockers = block_cpu(&fixture).await;
    let threads = blockers.threads.clone();
    assert_eq!(
        probe.phase.load(Ordering::Acquire),
        1,
        "result must still be in receipt, not storage wait"
    );
    let charged = fixture.quota.snapshot().worker_bytes;
    assert_eq!(fixture.encoder.foundation.usage().jobs, 1);
    drop(pending);
    assert!(delivered.is_empty());
    assert_eq!(
        fixture.encoder.foundation.usage().jobs,
        1,
        "outer Retained destruction must not release the only guard"
    );
    assert_eq!(fixture.quota.snapshot().worker_bytes, charged);
    assert!(matches!(
        destroyed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert_eq!(fixture.monitor.health().lanes[0].retirement_queued, 1);
    drop(blockers);
    assert_original_destroyed(destroyed, &probe, &threads, charged, 1).await;
    fixture.finish().await;
}

#[tokio::test]
async fn physical_shutdown_cannot_finish_before_cancelled_socket_originals_retire() {
    let mut fixture = Fixture::new();
    let (event, probe, mut destroyed, _registration) = observed_event(32768);
    let gate = FlushGate::new();
    let mut writer = FrameWriter::new(gate.clone());
    let mut delivered = HashMap::new();
    let mut pending = Box::pin(write_server_event(
        &mut writer,
        event,
        &mut delivered,
        Some(&fixture.state),
    ));
    tokio::time::timeout(WAIT, async {
        tokio::select! {
            () = gate.entered.notified() => {}
            _ = pending.as_mut() => panic!("expected blocked socket"),
        }
    })
    .await
    .unwrap();
    let blockers = block_cpu(&fixture).await;
    let threads = blockers.threads.clone();
    let charged = fixture.quota.snapshot().worker_bytes;
    drop(pending);
    assert!(delivered.is_empty());
    assert!(matches!(
        destroyed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    let mut owner = fixture.take_owner();
    owner.request_shutdown();
    let (mut owner, first) = tokio::task::spawn_blocking(move || {
        let first = owner
            .test_join_until_background(std::time::Instant::now() + Duration::from_millis(50))
            .unwrap();
        (owner, first)
    })
    .await
    .unwrap();
    assert!(!first.shutdown_complete);
    assert!(first.remaining_workers >= 2);
    assert_eq!(first.health.lanes[0].retirement_live, 1);
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    // I/O workers may have joined; their declared resident bytes are zero, so
    // the charged worker-byte equality still witnesses the original envelope.
    assert_eq!(fixture.quota.snapshot().worker_bytes, charged);
    drop(blockers);
    assert_original_destroyed(destroyed, &probe, &threads, charged, 0).await;
    let final_report = tokio::task::spawn_blocking(move || {
        owner.test_join_until_background(std::time::Instant::now() + WAIT)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(final_report.shutdown_complete);
    assert_eq!(final_report.remaining_workers, 0);
    assert_eq!(final_report.health.lanes[0].retirement_live, 0);
    assert_eq!(fixture.quota.snapshot().worker_threads, 0);
}

#[tokio::test]
async fn terminal_preattachment_refusal_returns_the_same_raw_original_as_typed_source() {
    let fixture = Fixture::new();
    fixture.state.execution.get().unwrap().request_shutdown();
    let event = ServerEvent::ScreenUpdate {
        pane_id: ilium_core::NodeId(9),
        first_sequence: 1,
        sequence: 1,
        bytes: b"synthetic refused original".to_vec(),
    };
    let pointer = event_pointer(&event);
    let mut writer = FrameWriter::new(tokio::io::sink());
    let mut delivered = HashMap::new();
    let result = write_server_event(&mut writer, event, &mut delivered, Some(&fixture.state)).await;
    let ilium_ipc::IpcError::Io(error) = result.unwrap_err() else {
        panic!("typed I/O source expected");
    };
    let refusal = error
        .into_inner()
        .unwrap()
        .downcast::<ServerEventAdmissionRefusal>()
        .unwrap();
    assert_eq!(refusal.reason, ilium_execution::RejectReason::Closed);
    assert_eq!(event_pointer(&refusal.original), pointer);
    assert!(delivered.is_empty());
    // This is deliberately NOT reported as CPU retirement: no envelope was
    // admitted. S2 producers must own this raw refusal and its cancellation.
    drop(refusal);
    fixture.finish().await;
}
