//! Exercises the actual shared encoder and FrameWriter with blocked transport
//! flushes. This is a component regression, not a real-socket or TUI proof.
use super::*;
use std::{
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
    let before_storage = fixture.worker_bytes();
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
    let held_storage = fixture.worker_bytes();
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
        fixture.worker_bytes(),
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

#[tokio::test]
async fn authoritative_text_trigger_snapshot_replaces_an_undersized_queue_lease() {
    let fixture = Fixture::new();
    let mut current = ilium_ipc::TextTriggerSettings::default();
    current.triggers.push(ilium_ipc::TextTrigger {
        message: "x".repeat(1024 * 1024),
        ..Default::default()
    });
    let current_bytes = crate::text_triggers::settings_bytes(&current).unwrap();
    let current_storage = fixture
        .general
        .reserve_storage(current_bytes)
        .await
        .unwrap();
    {
        let mut versioned = fixture.state.text_trigger_settings.write().await;
        versioned.settings = current;
        versioned.retention = Some(current_storage);
    }

    let mut writer = FrameWriter::new(tokio::io::sink());
    let mut delivered = HashMap::new();
    let result = write_server_event_admitted(
        &mut writer,
        Arc::new(ServerEvent::TextTriggersChanged {
            settings: ilium_ipc::TextTriggerSettings::default(),
        }),
        Some(Arc::new(fixture.quota.reserve_external_storage(1).unwrap())),
        &mut delivered,
        Some(&fixture.state),
    )
    .await;
    assert!(
        result.is_ok(),
        "authoritative snapshot must acquire exact writer storage: {result:?}"
    );

    {
        let mut versioned = fixture.state.text_trigger_settings.write().await;
        versioned.settings = ilium_ipc::TextTriggerSettings::default();
        versioned.retention = None;
    }
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
    event_retained_bytes: usize,
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
    output_quotas: [ilium_execution::QuotaGroup; 2],
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
            worker_bytes: self.client.foundation.quota_group().snapshot().worker_bytes
                + self
                    .output_quotas
                    .iter()
                    .map(|quota| quota.snapshot().worker_bytes)
                    .sum::<usize>(),
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
    output: &crate::execution::ServerOutput,
) -> Option<OriginalDropNotice> {
    let ServerEvent::ScreenUpdate { pane_id, .. } = event else {
        return None;
    };
    let probe = ORIGINAL_NOTICES.get()?.lock().unwrap().remove(pane_id)?;
    Some(OriginalDropNotice {
        probe,
        client: client.clone(),
        output_quotas: [output.storage.quota_group(), output.metadata.quota_group()],
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
        notice.probe.advance(2);
        if let Some(gate) = gate {
            let _ = gate.recv();
        }
    }
}
pub(super) fn observe_storage_wait(stored: &StoredServerEvent) {
    if let Some(notice) = &stored.drop_notice {
        notice.probe.advance(1);
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
        event_retained_bytes: event.retained_bytes(),
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
    output: crate::execution::ServerOutput,
    visible_output: crate::execution::ServerOutput,
    quota: ilium_execution::QuotaGroup,
    monitor: ilium_execution::ExecutionMonitor,
    baseline: usize,
}
impl Fixture {
    fn new() -> Self {
        let owner = crate::execution::ServerExecution::start().expect("private real server bank");
        let encoder = owner.encoder.clone();
        let general = owner.client.clone();
        let output = owner.output.clone();
        let visible_output = owner.visible_output.clone();
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
            output,
            visible_output,
            quota,
            monitor,
            baseline,
        }
    }
    fn worker_bytes(&self) -> usize {
        self.quota.snapshot().worker_bytes
            + self.output.storage.quota_group().snapshot().worker_bytes
            + self
                .visible_output
                .storage
                .quota_group()
                .snapshot()
                .worker_bytes
            + self.output.metadata.quota_group().snapshot().worker_bytes
    }
    async fn wait_baseline(&self) {
        let notification = self.encoder.completion_notification();
        tokio::time::timeout(WAIT, async {
            loop {
                let notified = notification.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.worker_bytes() == self.baseline
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
            owner.test_join_until_background(
                std::time::Instant::now() + crate::execution::PHYSICAL_JOIN_TEST_WAIT,
            )
        })
        .await
        .unwrap()
        .unwrap();
        assert!(
            report.shutdown_complete,
            "physical shutdown retains an original: {report:?}"
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
    expected_frame: bool,
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
    assert_eq!(notice.frame_address != 0, expected_frame);
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
        expected_encoder_jobs
            * (probe.event_retained_bytes + std::mem::size_of::<StoredServerEvent>()),
        "original input debit survives until CPU destruction before storage transfer"
    );
    assert_eq!(
        notice.encoder_result_bytes,
        expected_encoder_jobs * crate::execution::SERVER_CODEC_RESULT_BYTES,
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
    let charged = fixture.worker_bytes();
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
    assert_eq!(fixture.worker_bytes(), charged);
    assert!(matches!(
        destroyed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert_eq!(fixture.monitor.health().lanes[0].retirement_queued, 1);
    drop(blockers);
    assert_original_destroyed(destroyed, &probe, &threads, charged, 0, true).await;
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
async fn output_wait_cancellation_keeps_the_raw_original_in_cpu_custody_without_encoder_credit() {
    let fixture = Fixture::new();
    let output_quota = fixture.output.storage.quota_group();
    let occupied = output_quota
        .reserve_external_storage(output_quota.snapshot().limits.worker_bytes)
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
    drive_to_phase(&mut pending, &probe, 1).await;
    assert!(gate.state.lock().unwrap().bytes.is_empty());
    assert_eq!(probe.frame_address.load(Ordering::Acquire), 0);
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    let blockers = block_cpu(&fixture).await;
    let threads = blockers.threads.clone();
    let charged = fixture.worker_bytes();
    drop(pending);
    assert!(delivered.is_empty());
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    assert_eq!(fixture.worker_bytes(), charged);
    assert!(matches!(
        destroyed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert_eq!(fixture.monitor.health().lanes[0].retirement_queued, 1);
    drop(blockers);
    assert_original_destroyed(destroyed, &probe, &threads, charged, 0, false).await;
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
    // Stop on the actual encoding callback's publication gate. No assumption
    // about intermediate storage or quota-gate scheduling is needed.
    drive_to_phase(&mut pending, &probe, 2).await;
    drop(allow_publication);
    // Taking both real CPU owners proves the encoding callback returned and
    // published (or attempted publication). Never poll the writer again.
    let blockers = block_cpu(&fixture).await;
    let threads = blockers.threads.clone();
    assert_eq!(
        probe.phase.load(Ordering::Acquire),
        2,
        "encoded result must still be in receipt, not transport flush"
    );
    let charged = fixture.worker_bytes();
    assert_eq!(fixture.encoder.foundation.usage().jobs, 1);
    drop(pending);
    assert!(delivered.is_empty());
    assert_eq!(
        fixture.encoder.foundation.usage().jobs,
        1,
        "outer Retained destruction must not release the only guard"
    );
    assert_eq!(fixture.worker_bytes(), charged);
    assert!(matches!(
        destroyed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert_eq!(fixture.monitor.health().lanes[0].retirement_queued, 1);
    drop(blockers);
    assert_original_destroyed(destroyed, &probe, &threads, charged, 1, true).await;
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
    let charged = fixture.worker_bytes();
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
    assert_eq!(fixture.worker_bytes(), charged);
    drop(blockers);
    assert_original_destroyed(destroyed, &probe, &threads, charged, 0, true).await;
    let final_report = tokio::task::spawn_blocking(move || {
        owner.test_join_until_background(
            std::time::Instant::now() + crate::execution::PHYSICAL_JOIN_TEST_WAIT,
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert!(final_report.shutdown_complete, "{final_report:?}");
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

#[tokio::test]
async fn full_visible_snapshot_flushes_while_general_and_ordinary_output_storage_are_full() {
    let fixture = Fixture::new();
    let recovery = fixture
        .state
        .execution
        .get()
        .unwrap()
        .visible_recovery
        .clone();
    let pane_id = ilium_core::NodeId(991);
    let bytes = vec![b'x'; 2 * 1024 * 1024];
    let ordinary = fixture.output.storage.quota_group();
    let ordinary_pressure = ordinary
        .reserve_external_storage(ordinary.snapshot().limits.worker_bytes)
        .unwrap();
    let producer = recovery
        .reserve_storage(2 * bytes.len() + 16 * 1024)
        .await
        .unwrap();
    let snapshot = fixture.quota.snapshot();
    let pressure = fixture
        .general
        .reserve_storage(snapshot.limits.worker_bytes - snapshot.worker_bytes)
        .await
        .unwrap();
    let gate = FlushGate::new();
    let mut writer = FrameWriter::new(gate.clone());
    let mut delivered = HashMap::new();
    let mut pending = Box::pin(write_server_event_admitted(
        &mut writer,
        Arc::new(ServerEvent::TerminalSnapshot {
            pane_id,
            through_sequence: 0,
            bytes: bytes.clone(),
            is_complete: true,
        }),
        Some(producer),
        &mut delivered,
        Some(&fixture.state),
    ));
    tokio::time::timeout(WAIT, async {
        tokio::select! {
            result = &mut pending => panic!("writer finished before controlled flush: {result:?}"),
            _ = gate.entered.notified() => {}
        }
    })
    .await
    .expect("protected recovery must reach flush without general storage credit");
    assert_eq!(
        fixture.quota.snapshot().worker_bytes,
        snapshot.limits.worker_bytes
    );
    assert_eq!(
        recovery.quota_group().snapshot().worker_bytes,
        2 * bytes.len() + 16 * 1024
    );
    let visible_bytes = fixture
        .visible_output
        .storage
        .quota_group()
        .snapshot()
        .worker_bytes;
    assert!(visible_bytes >= 2 * bytes.len());
    assert!(
        visible_bytes < 8 * 1024 * 1024,
        "the conservative window must shrink before blocked flush"
    );
    assert_eq!(
        ordinary.snapshot().worker_bytes,
        ordinary.snapshot().limits.worker_bytes
    );
    gate.release();
    tokio::time::timeout(WAIT, pending).await.unwrap().unwrap();
    assert_eq!(delivered.get(&pane_id), Some(&0));
    let wire = gate.state.lock().unwrap().bytes.clone();
    let decoded: ServerEvent = ilium_ipc::read_frame(&mut wire.as_slice()).await.unwrap();
    assert!(
        matches!(decoded, ServerEvent::TerminalSnapshot { bytes: actual, through_sequence: 0, is_complete: true, .. } if actual == bytes)
    );
    // The exact protected payload and envelope charges retire on the same CPU
    // owner; general pressure remains held until physical output has completed.
    let wake = recovery.completion_notification();
    tokio::time::timeout(WAIT, async {
        loop {
            let notified = wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if recovery.quota_group().snapshot().worker_bytes == 0 {
                break;
            }
            notified.await;
        }
    })
    .await
    .expect("protected custody must release after CPU retirement");
    assert_eq!(
        fixture.quota.snapshot().worker_bytes,
        snapshot.limits.worker_bytes
    );
    drop(pressure);
    drop(ordinary_pressure);
    fixture.finish().await;
}

#[tokio::test]
async fn foreign_storage_cannot_pay_for_or_bypass_private_output_admission() {
    let fixture = Fixture::new();
    let output_quota = fixture.output.storage.quota_group();
    let foreign = ilium_execution::QuotaGroup::new(output_quota.snapshot().limits);
    let producer = Arc::new(
        foreign
            .reserve_external_storage(SERVER_EVENT_PREPARATION_BYTES)
            .unwrap(),
    );
    let pressure = output_quota
        .reserve_external_storage(output_quota.snapshot().limits.worker_bytes)
        .unwrap();
    let (event, probe, _destroyed, _registration) = observed_event(128);
    let gate = FlushGate::new();
    let mut writer = FrameWriter::new(gate.clone());
    let mut delivered = HashMap::new();
    let mut pending = Box::pin(write_server_event_admitted(
        &mut writer,
        Arc::new(event),
        Some(producer),
        &mut delivered,
        Some(&fixture.state),
    ));
    drive_to_phase(&mut pending, &probe, 1).await;
    assert!(gate.state.lock().unwrap().bytes.is_empty());
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    assert_eq!(
        fixture
            .state
            .execution
            .get()
            .unwrap()
            .visible_recovery
            .quota_group()
            .snapshot()
            .worker_bytes,
        0
    );
    drop(pending);
    assert!(delivered.is_empty());
    drop(pressure);
    fixture.wait_baseline().await;
    assert_eq!(
        foreign.snapshot().worker_bytes,
        0,
        "original foreign lease retires with its exact payload"
    );
    fixture.finish().await;
}

#[tokio::test]
async fn general_admitted_reply_flushes_at_one_hundred_percent_general_worker_storage() {
    let fixture = Fixture::new();
    let (sender, mut receiver) = DirectEventSender::admitted_channel(1, fixture.general.clone());
    let (event, probe, destroyed, _registration) = observed_event(32768);
    let pane_id = match &event {
        ServerEvent::ScreenUpdate { pane_id, .. } => *pane_id,
        _ => unreachable!(),
    };
    sender.send(event).await.unwrap();
    let queued = receiver.recv_queued().await.unwrap();
    let ledger = fixture.quota.snapshot();
    let pressure = fixture
        .quota
        .reserve_external_storage(ledger.limits.worker_bytes - ledger.worker_bytes)
        .unwrap();
    let gate = FlushGate::new();
    let mut frame_writer = FrameWriter::new(gate.clone());
    let mut delivered = HashMap::new();
    let mut pending = Box::pin(write_queued_server_event(
        &mut frame_writer,
        queued,
        &mut delivered,
        Some(&fixture.state),
    ));
    tokio::time::timeout(WAIT, async {
        tokio::select! {
            () = gate.entered.notified() => {}
            result = pending.as_mut() => panic!("queued reply failed before actual flush: {result:?}"),
        }
    }).await.expect("a prepaid general reply must flush without new general metadata/storage");
    assert_eq!(
        fixture.quota.snapshot().worker_bytes,
        ledger.limits.worker_bytes
    );
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    let blockers = block_cpu(&fixture).await;
    let charged = fixture.worker_bytes();
    let threads = blockers.threads.clone();
    gate.release();
    pending.await.unwrap();
    assert_eq!(delivered.get(&pane_id), Some(&1));
    assert_eq!(
        fixture.quota.snapshot().worker_bytes,
        ledger.limits.worker_bytes,
        "producer storage stays with the original while CPU retirement is blocked"
    );
    drop(blockers);
    assert_original_destroyed(destroyed, &probe, &threads, charged, 0, true).await;
    drop(pressure);
    fixture.finish().await;
}

#[tokio::test]
async fn two_unadmitted_storage_waiters_cannot_hold_encoder_credits_needed_by_a_prepaid_reply() {
    let fixture = Fixture::new();
    let (sender, mut receiver) = DirectEventSender::admitted_channel(1, fixture.general.clone());
    sender
        .send(ServerEvent::Error {
            message: "already prepaid third reply".into(),
        })
        .await
        .unwrap();
    let queued = receiver.recv_queued().await.unwrap();
    let ledger = fixture.quota.snapshot();
    // Synthetic general pressure leaves enough metadata for the old writer,
    // but not either two-megabyte event plus its encoded frame. This isolates
    // the encoder-credit cycle from the separately tested metadata cycle.
    let pressure = fixture
        .quota
        .reserve_external_storage(ledger.limits.worker_bytes - ledger.worker_bytes - 64 * 1024)
        .unwrap();
    let first_gate = FlushGate::new();
    let second_gate = FlushGate::new();
    let release = ReleaseOnDrop([first_gate.clone(), second_gate.clone()]);
    let first_flush = first_gate.clone();
    let second_flush = second_gate.clone();
    let (first_event, first_probe, _first_destroyed, _first_registration) =
        observed_event(2 * 1024 * 1024);
    let (second_event, second_probe, _second_destroyed, _second_registration) =
        observed_event(2 * 1024 * 1024);
    let run = |gate, event, state: Arc<ServerState>| async move {
        let mut writer = FrameWriter::new(gate);
        let mut delivered = HashMap::new();
        let result = write_server_event(&mut writer, event, &mut delivered, Some(&state)).await;
        (result, delivered)
    };
    let first = tokio::spawn(run(first_gate, first_event, fixture.state.clone()));
    let second = tokio::spawn(run(second_gate, second_event, fixture.state.clone()));
    tokio::time::timeout(WAIT, async {
        first_probe.wait(1).await;
        second_probe.wait(1).await;
    })
    .await
    .unwrap();
    let mut third_writer = FrameWriter::new(tokio::io::sink());
    let mut third_delivered = HashMap::new();
    let third = tokio::time::timeout(
        WAIT,
        write_queued_server_event(
            &mut third_writer,
            queued,
            &mut third_delivered,
            Some(&fixture.state),
        ),
    )
    .await;
    // Attachment is the old storage-wait witness, not a GREEN flush witness.
    // Only measure zero encoder jobs after both real flush gates have entered.
    // On RED, third times out and cleanup below immediately releases pressure.
    let reached_flush = if third.is_ok() {
        tokio::time::timeout(WAIT, async {
            first_flush.entered.notified().await;
            second_flush.entered.notified().await;
        })
        .await
        .is_ok()
    } else {
        false
    };
    let held_jobs = fixture.encoder.foundation.usage().jobs;
    let both_pending = !first.is_finished() && !second.is_finished();
    // On the original RED path the two jobs are waiting on general storage.
    // Release that synthetic pressure before joining, so failure leaves no
    // stranded native work or Tokio tasks.
    drop(pressure);
    drop(release);
    let (first, second) = tokio::time::timeout(WAIT, async {
        (first.await.unwrap(), second.await.unwrap())
    })
    .await
    .unwrap();
    first.0.unwrap();
    second.0.unwrap();
    assert!(
        third.is_ok(),
        "two old storage-wait encoders must not starve an admitted reply"
    );
    third.unwrap().unwrap();
    assert!(
        reached_flush,
        "successful third reply must coexist with both actual blocked flushes"
    );
    assert!(
        both_pending,
        "the two real socket flushes remain unacknowledged"
    );
    assert_eq!(held_jobs, 0);
    fixture.finish().await;
}

#[tokio::test]
async fn selected_session_frame_does_not_wait_on_its_own_later_queued_direct_reply() {
    let fixture = Fixture::new();
    let (direct_tx, direct_rx) = DirectEventSender::admitted_channel(2, fixture.general.clone());
    let (broadcast_tx, broadcast_rx) = tokio::sync::broadcast::channel::<Arc<ServerEvent>>(2);
    let (_attach_tx, attach_rx) = watch::channel(AttachPhase::Ready);
    let (control_tx, control_rx) = mpsc::channel(2);
    let gate = FlushGate::new();
    let writer_gate = gate.clone();
    let state = fixture.state.clone();
    let mut writer = tokio::spawn(async move {
        write_replies(
            writer_gate,
            broadcast_rx,
            direct_rx,
            attach_rx,
            control_rx,
            Some(state),
        )
        .await;
    });
    let (applied, ack) = oneshot::channel();
    control_tx
        .send(StreamControlCommand {
            control: StreamControl::StreamAllTerminals,
            applied,
        })
        .await
        .unwrap();
    tokio::time::timeout(WAIT, ack).await.unwrap().unwrap();
    let ledger = fixture.quota.snapshot();
    // This deliberately large declaration is owned by the later queued reply,
    // not by an unrelated pressure lease retained by the test. Flushing that
    // reply is the operation that would free the selected frame's old pool.
    let queued_storage = Arc::new(
        fixture
            .quota
            .reserve_external_storage(ledger.limits.worker_bytes - ledger.worker_bytes - 64 * 1024)
            .unwrap(),
    );
    let (event, probe, _destroyed, _registration) = observed_event(2 * 1024 * 1024);
    broadcast_tx.send(Arc::new(event)).unwrap();
    tokio::time::timeout(WAIT, probe.wait(1)).await.unwrap();
    direct_tx
        .send_with_storage(
            ServerEvent::Error {
                message: "queued behind the selected session frame".into(),
            },
            Some(queued_storage),
        )
        .await
        .unwrap();
    drop(direct_tx);
    gate.release();
    let completed = tokio::time::timeout(WAIT, &mut writer).await;
    if completed.is_err() {
        writer.abort();
        let _ = writer.await;
    }
    assert!(
        completed.is_ok(),
        "writer must drain the session frame and the reply that owns general storage"
    );
    completed.unwrap().unwrap();
    let wire = gate.state.lock().unwrap().bytes.clone();
    let mut wire = wire.as_slice();
    assert!(matches!(
        ilium_ipc::read_frame::<ServerEvent, _>(&mut wire)
            .await
            .unwrap(),
        ServerEvent::ScreenUpdate { .. }
    ));
    assert!(
        matches!(ilium_ipc::read_frame::<ServerEvent, _>(&mut wire).await.unwrap(), ServerEvent::Error { message } if message == "queued behind the selected session frame")
    );
    assert!(
        wire.is_empty(),
        "the frame selected first stays before the later direct reply"
    );
    fixture.finish().await;
}

#[tokio::test]
async fn journal_storage_releases_only_after_its_real_frame_flushes_under_full_general_pressure() {
    let fixture = Fixture::new();
    assert!(fixture
        .state
        .events
        .initialize_journal(fixture.quota.clone()));
    let subscription = fixture.state.events.subscribe_ordered().unwrap();
    fixture
        .state
        .events
        .try_publish_ordered(
            ServerEvent::NodeActivityChanged {
                node_id: ilium_core::NodeId(9191),
                activity_revision: 7,
            },
            256,
        )
        .unwrap();
    let ledger = fixture.quota.snapshot();
    let journal_bytes = ledger.worker_bytes - fixture.baseline;
    assert!(journal_bytes > 0);
    let pressure = fixture
        .quota
        .reserve_external_storage(ledger.limits.worker_bytes - ledger.worker_bytes)
        .unwrap();
    let (direct_tx, direct_rx) = DirectEventSender::admitted_channel(1, fixture.general.clone());
    let (_broadcast_tx, broadcast_rx) = tokio::sync::broadcast::channel::<Arc<ServerEvent>>(1);
    let (_attach_tx, attach_rx) = watch::channel(AttachPhase::Ready);
    let (_control_tx, control_rx) = mpsc::channel(1);
    let gate = FlushGate::new();
    let writer_gate = gate.clone();
    let state = fixture.state.clone();
    let mut writer = tokio::spawn(async move {
        write_replies_ordered(
            writer_gate,
            broadcast_rx,
            subscription,
            direct_rx,
            attach_rx,
            control_rx,
            Some(state),
        )
        .await;
    });
    tokio::time::timeout(WAIT, gate.entered.notified())
        .await
        .expect("journal output must reach real flush with general storage full");
    assert_eq!(
        fixture.quota.snapshot().worker_bytes,
        ledger.limits.worker_bytes,
        "a blocked flush cannot advance the journal cursor"
    );
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    drop(direct_tx);
    gate.release();
    let completed = tokio::time::timeout(WAIT, &mut writer).await;
    if completed.is_err() {
        writer.abort();
        let _ = writer.await;
    }
    completed.unwrap().unwrap();
    assert_eq!(
        fixture.quota.snapshot().worker_bytes,
        ledger.limits.worker_bytes - journal_bytes,
        "only the physically flushed cursor releases journal producer storage"
    );
    let wire = gate.state.lock().unwrap().bytes.clone();
    let decoded: ServerEvent = ilium_ipc::read_frame(&mut wire.as_slice()).await.unwrap();
    assert!(matches!(
        decoded,
        ServerEvent::NodeActivityChanged {
            activity_revision: 7,
            ..
        }
    ));
    drop(pressure);
    fixture.finish().await;
}

#[tokio::test]
async fn metadata_guarantee_keeps_the_original_shared_sixty_four_retirement_slots() {
    let fixture = Fixture::new();
    let general = fixture.quota.snapshot();
    let general_pressure = fixture
        .quota
        .reserve_external_storage(general.limits.worker_bytes - general.worker_bytes)
        .unwrap();
    let output_quota = fixture.output.storage.quota_group();
    let output_pressure = output_quota
        .reserve_external_storage(output_quota.snapshot().limits.worker_bytes)
        .unwrap();
    let mut reservations = Vec::new();
    for _ in 0..ilium_execution::RETIREMENT_SLOTS {
        reservations.push(
            fixture
                .output
                .metadata
                .reserve_retirement::<StoredServerEvent>(std::mem::size_of::<StoredServerEvent>())
                .await
                .unwrap(),
        );
    }
    assert_eq!(ilium_execution::RETIREMENT_SLOTS, 64);
    assert_eq!(
        fixture
            .output
            .metadata
            .quota_group()
            .snapshot()
            .worker_bytes,
        SERVER_EVENT_RETIREMENT_BYTES
    );
    let snapshot_owner = fixture.general.foundation.retirement();
    assert!(
        matches!(
            snapshot_owner.try_reserve::<crate::SnapshotRestoreFuture>(4096),
            Err(ilium_execution::RejectReason::QueueFull)
        ),
        "snapshot restoration and output must still share the existing retirement slots"
    );
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    drop(reservations);
    drop(output_pressure);
    drop(general_pressure);
    fixture.finish().await;
}

#[tokio::test]
async fn encoder_concurrency_stays_at_two_despite_the_new_output_byte_roots() {
    let fixture = Fixture::new();
    let cost = ilium_execution::JobCost {
        input_bytes: 4096,
        result_bytes: 4096,
    };
    let first = fixture
        .encoder
        .reserve(ilium_execution::Lane::Cpu, cost)
        .await
        .unwrap();
    let second = fixture
        .encoder
        .reserve(ilium_execution::Lane::Cpu, cost)
        .await
        .unwrap();
    assert_eq!(fixture.encoder.foundation.usage().jobs, 2);
    assert!(matches!(
        fixture
            .encoder
            .foundation
            .try_reserve(ilium_execution::Lane::Cpu, cost),
        Err(ilium_execution::RejectReason::JobLimit)
    ));
    drop(first);
    drop(second);
    fixture.finish().await;
}

#[tokio::test]
async fn retained_input_above_the_existing_aggregate_encoder_ceiling_is_refused_without_a_socket_byte(
) {
    let fixture = Fixture::new();
    // Empty length but a large allocation distinguishes retained allocation
    // sizing from wire sizing. This is labelled synthetic test input.
    let bytes = Vec::with_capacity(crate::execution::SERVER_ENCODER_MAX_INPUT_BYTES);
    let event = ServerEvent::ScreenUpdate {
        pane_id: ilium_core::NodeId(9292),
        first_sequence: 1,
        sequence: 1,
        bytes,
    };
    let gate = FlushGate::new();
    let mut writer = FrameWriter::new(gate.clone());
    let mut delivered = HashMap::new();
    let result = tokio::time::timeout(
        WAIT,
        write_server_event(&mut writer, event, &mut delivered, Some(&fixture.state)),
    )
    .await
    .expect("immutable input oversize must fail, never wait for a larger quota");
    assert!(result.is_err());
    assert!(gate.state.lock().unwrap().bytes.is_empty());
    assert!(delivered.is_empty());
    fixture.finish().await;
}

struct ReleaseAllOnDrop(Vec<FlushGate>);
impl Drop for ReleaseAllOnDrop {
    fn drop(&mut self) {
        for gate in &self.0 {
            gate.release();
        }
    }
}

#[tokio::test]
async fn all_sixty_four_visible_prefixes_fit_without_lowering_the_connection_capacity_gate() {
    let fixture = Fixture::new();
    let count = crate::ipc::MAX_CONCURRENT_CONNECTIONS;
    assert_eq!(count, 64);
    let recovery = fixture
        .state
        .execution
        .get()
        .unwrap()
        .visible_recovery
        .clone();
    let ordinary = fixture.output.storage.quota_group();
    let ordinary_pressure = ordinary
        .reserve_external_storage(ordinary.snapshot().limits.worker_bytes)
        .unwrap();
    let ledger = fixture.quota.snapshot();
    let general_pressure = fixture
        .quota
        .reserve_external_storage(ledger.limits.worker_bytes - ledger.worker_bytes)
        .unwrap();
    let mut release = ReleaseAllOnDrop(Vec::new());
    let mut tasks = Vec::new();
    for index in 0..count {
        let bytes = vec![b'x'; 256 * 1024];
        let producer = recovery
            .reserve_storage(2 * bytes.len() + 16 * 1024)
            .await
            .unwrap();
        let gate = FlushGate::new();
        release.0.push(gate.clone());
        let state = fixture.state.clone();
        tasks.push(tokio::spawn(async move {
            let mut writer = FrameWriter::new(gate);
            let mut delivered = HashMap::new();
            let pane_id = ilium_core::NodeId(10_000 + index as u64);
            let result = write_server_event_admitted(
                &mut writer,
                Arc::new(ServerEvent::ScreenUpdate {
                    pane_id,
                    first_sequence: 1,
                    sequence: 1,
                    bytes,
                }),
                Some(producer),
                &mut delivered,
                Some(&state),
            )
            .await;
            (result, delivered)
        }));
    }
    tokio::time::timeout(WAIT, async {
        for gate in &release.0 {
            gate.entered.notified().await;
        }
    })
    .await
    .expect("every unchanged connection slot must reach an independently blocked real flush");
    assert!(tasks.iter().all(|task| !task.is_finished()));
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    assert_eq!(fixture.monitor.health().lanes[0].retirement_live, count);
    assert_eq!(
        fixture
            .output
            .metadata
            .quota_group()
            .snapshot()
            .worker_bytes,
        SERVER_EVENT_RETIREMENT_BYTES
    );
    let visible_bytes = fixture
        .visible_output
        .storage
        .quota_group()
        .snapshot()
        .worker_bytes;
    assert!(visible_bytes >= count * 2 * 256 * 1024);
    assert!(
        visible_bytes < 64 * 1024 * 1024,
        "64 small flushed frames do not each retain a full preparation window"
    );
    assert_eq!(
        fixture.quota.snapshot().worker_bytes,
        ledger.limits.worker_bytes
    );
    drop(release);
    for (index, task) in tasks.into_iter().enumerate() {
        let (result, delivered) = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
        result.unwrap();
        assert_eq!(
            delivered.get(&ilium_core::NodeId(10_000 + index as u64)),
            Some(&1)
        );
    }
    drop(general_pressure);
    drop(ordinary_pressure);
    fixture.finish().await;
}

#[tokio::test]
async fn wire_oversize_is_refused_before_transport_without_growing_the_window() {
    let fixture = Fixture::new();
    let event = ServerEvent::ScreenUpdate {
        pane_id: ilium_core::NodeId(9393),
        first_sequence: 1,
        sequence: 1,
        bytes: vec![b'x'; ilium_ipc::MAX_FRAME_LEN as usize + 1],
    };
    let gate = FlushGate::new();
    let mut writer = FrameWriter::new(gate.clone());
    let mut delivered = HashMap::new();
    let result = tokio::time::timeout(
        WAIT,
        write_server_event(&mut writer, event, &mut delivered, Some(&fixture.state)),
    )
    .await
    .expect("wire oversize must be a bounded refusal, never an admission wait");
    assert!(result.is_err());
    assert!(gate.state.lock().unwrap().bytes.is_empty());
    assert!(delivered.is_empty());
    // The typed callback error retains codec credit until its consumer drops it.
    drop(result);
    fixture.finish().await;
}

#[tokio::test]
async fn retained_input_above_one_hundred_twenty_eight_mib_still_fits_the_original_aggregate_contract(
) {
    let fixture = Fixture::new();
    assert_eq!(
        fixture
            .encoder
            .foundation
            .maximum_cpu_job_cost()
            .unwrap()
            .input_bytes,
        256 * 1024 * 1024
    );
    // Labelled synthetic sparse allocation: retained backing is large, wire
    // length is small. The original encoder admitted this one-event cost.
    let mut bytes = Vec::with_capacity(crate::execution::SERVER_CODEC_INPUT_BYTES + 4096);
    bytes.extend_from_slice(b"synthetic sparse source");
    let pane_id = ilium_core::NodeId(9494);
    let event = ServerEvent::ScreenUpdate {
        pane_id,
        first_sequence: 1,
        sequence: 1,
        bytes,
    };
    let retained = event.retained_bytes() + std::mem::size_of::<StoredServerEvent>();
    assert!(retained > 128 * 1024 * 1024 && retained < 256 * 1024 * 1024);
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
            result = pending.as_mut() => panic!("original supported sparse event refused before flush: {result:?}"),
        }
    }).await.expect("supported aggregate input must reach actual flush");
    assert_eq!(fixture.encoder.foundation.usage().jobs, 0);
    assert!(fixture.output.storage.quota_group().snapshot().worker_bytes >= retained);
    gate.release();
    pending.await.unwrap();
    assert_eq!(delivered.get(&pane_id), Some(&1));
    fixture.finish().await;
}

#[tokio::test]
async fn two_actual_small_encoder_callbacks_can_be_active_before_either_publishes() {
    let fixture = Fixture::new();
    let (first_event, first_probe, _first_destroyed, _first_registration) = observed_event(1024);
    let (second_event, second_probe, _second_destroyed, _second_registration) =
        observed_event(2048);
    let declared_input = first_probe.event_retained_bytes
        + second_probe.event_retained_bytes
        + 2 * std::mem::size_of::<StoredServerEvent>();
    let (first_release, first_gate) = std::sync::mpsc::channel();
    let (second_release, second_gate) = std::sync::mpsc::channel();
    *first_probe.before_publish.lock().unwrap() = Some(first_gate);
    *second_probe.before_publish.lock().unwrap() = Some(second_gate);
    let run = |event, state: Arc<ServerState>| async move {
        let mut writer = FrameWriter::new(tokio::io::sink());
        let mut delivered = HashMap::new();
        write_server_event(&mut writer, event, &mut delivered, Some(&state)).await
    };
    let first = tokio::spawn(run(first_event, fixture.state.clone()));
    let second = tokio::spawn(run(second_event, fixture.state.clone()));
    tokio::time::timeout(WAIT, async {
        first_probe.wait(2).await;
        second_probe.wait(2).await;
    })
    .await
    .expect("both actual CPU encoders must enter before-publish gates concurrently");
    assert_eq!(fixture.encoder.foundation.usage().jobs, 2);
    assert_eq!(
        fixture.encoder.foundation.usage().input_bytes,
        declared_input
    );
    assert_eq!(
        fixture.encoder.foundation.usage().result_bytes,
        2 * crate::execution::SERVER_CODEC_RESULT_BYTES
    );
    assert!(first_probe.frame_address.load(Ordering::Acquire) != 0);
    assert!(second_probe.frame_address.load(Ordering::Acquire) != 0);
    drop(first_release);
    drop(second_release);
    tokio::time::timeout(WAIT, first)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(WAIT, second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    fixture.finish().await;
}
